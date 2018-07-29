use kubeclient::clients::KubeClient;
use kubeclient::resources::Deployment;
use std::collections::HashMap;

use failure::{Error, ResultExt};
use k8s_openapi::v1_10::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kubeclient::clients::ReadClient;
use kubeclient::resources::Kind;
use kubeclient::resources::Resource;
use kubeclient::Kubernetes;
use serde_yaml;

use common::deployment::{DeploymentState, RolloutStatus, RolloutStatusReason};
use common::git::VersionHash;

use super::Deployable;

const VERSION_ANNOTATION: &str = "new-dm/version";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    kubeconf: String,
    namespace: String,
    glob: Option<String>,
}

impl Config {
    pub fn create(&self) -> Result<KubernetesDeployer, Error> {
        KubernetesDeployer::new(self)
    }
}

pub struct KubernetesDeployer {
    kubeconf: String,
    namespace: String,
    client: Kubernetes,
}

impl KubernetesDeployer {
    fn new(config: &Config) -> Result<KubernetesDeployer, Error> {
        Ok(KubernetesDeployer {
            kubeconf: config.kubeconf.clone(),
            namespace: config.namespace.clone(),
            client: Kubernetes::load_conf(&config.kubeconf)?.namespace(&config.namespace),
        })
    }

    fn retrieve_current_state(
        &mut self,
        deployables: &[Deployable],
    ) -> Result<HashMap<String, DeploymentState>, Error> {
        let mut result = HashMap::with_capacity(deployables.len());

        for d in deployables {
            let kind = determine_kind(&d.merged_content)?;

            let state = match kind {
                Kind::Deployment => get_deployment_state(self.client.deployments(), d)?,
                // TODO these should work
                Kind::DaemonSet => bail!("Unsupported deployable type: {:?}", kind),
                Kind::Pod => bail!("Unsupported deployable type: {:?}", kind),

                Kind::Service => get_simple_deployable_state(self.client.services(), d)?,
                Kind::ConfigMap => get_simple_deployable_state(self.client.config_maps(), d)?,
                Kind::Secret => get_simple_deployable_state(self.client.secrets(), d)?,
                Kind::NetworkPolicy | Kind::Node => {
                    bail!("Unsupported deployable type: {:?}", kind);
                }
            };

            if let Some(state) = state {
                result.insert(d.name.clone(), state);
            } else {
                warn!("Deployable {} does not exist", d.name);
                result.insert(d.name.clone(), DeploymentState::NotDeployed);
            }
        }

        Ok(result)
    }

    fn do_deploy(&mut self, deployment: &Deployable) -> Result<(), Error> {
        use serde_yaml::{self, Mapping, Value};
        let mut data: Value = deployment.merged_content.clone(); // TODO
        let root = data
            .as_mapping_mut()
            .ok_or_else(|| format_err!("bad deployment yaml: root not a mapping"))?;
        {
            let metadata = root
                .get_mut(&Value::String("metadata".to_owned()))
                .ok_or_else(|| format_err!("bad deployment yaml: no metadata"))?
                .as_mapping_mut()
                .ok_or_else(|| format_err!("bad deployment yaml: metadata not a mapping"))?;
            // TODO check name
            let annotations_key = Value::String("annotations".to_owned());
            if !metadata.contains_key(&annotations_key) {
                metadata.insert(annotations_key.clone(), Value::Mapping(Mapping::new()));
            }
            let annotations = metadata
                .get_mut(&annotations_key)
                .expect("just inserted mapping")
                .as_mapping_mut()
                .ok_or_else(|| format_err!("bad deployment yaml: annotations not a mapping"))?;

            let key = Value::String(VERSION_ANNOTATION.to_owned());
            let value = Value::String(format!("{}", deployment.version));
            annotations.insert(key, value);
        }

        let data = serde_yaml::to_string(root)?;

        self.kubectl_apply(&data)?;

        Ok(())
    }

    fn kubectl_apply(&self, data: &str) -> Result<(), Error> {
        // TODO: use kube API instead
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut process = Command::new("kubectl")
            .args(&[
                "apply",
                "--namespace",
                &self.namespace,
                "--kubeconfig",
                &self.kubeconf,
                "-f",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("running kubectl failed")?;

        process
            .stdin
            .as_mut()
            .unwrap()
            .write_all(data.as_bytes())
            .context("piping data to kubectl failed")?;

        let output = process
            .wait_with_output()
            .context("kubectl wasn't running")?;

        if !output.status.success() {
            bail!(
                "kubectl failed; stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        debug!(
            "kubectl stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );

        debug!(
            "kubectl stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        Ok(())
    }

    // TODO not k8s-specific, move to deployment module
    pub fn deploy(&mut self, deployments: &[Deployable]) -> Result<(), Error> {
        let current_state = self.retrieve_current_state(deployments)?;

        for d in deployments {
            debug!("looking at {}", d.name);
            let deployed_version = if let Some(v) = current_state.get(&d.name) {
                v.clone()
            } else {
                warn!("no known version for {}, not deploying", d.name);
                continue;
            };

            if let DeploymentState::Deployed { version, .. } = deployed_version {
                if version == d.version {
                    info!("same version for {}, not deploying", d.name);
                    continue;
                }
            }

            info!(
                "Deploying {} version {} with content {}",
                d.name,
                d.version,
                serde_yaml::to_string(&d.merged_content).unwrap_or(String::new()) // FIXME
            );

            match self.do_deploy(d) {
                Ok(()) => {}
                Err(e) => {
                    // TODO: maybe instead mark the service as failing to deploy
                    // and don't try again?
                    error!("Deployment of {} failed: {}\n{}", d.name, e, e.backtrace());
                    for cause in e.causes() {
                        error!("caused by: {}", cause);
                    }
                }
            }
        }

        Ok(())
    }

    // TODO not k8s-specific, move to deployment module
    pub fn check_rollout_status(
        &mut self,
        deployments: &[Deployable],
    ) -> Result<(RolloutStatus, HashMap<String, DeploymentState>), Error> {
        let current_state = self.retrieve_current_state(deployments)?;

        let combined = current_state
            .iter()
            .map(|(_, v)| v)
            .map(|d| match d {
                DeploymentState::NotDeployed => RolloutStatus::Outdated,
                DeploymentState::Deployed {
                    status,
                    version,
                    expected_version,
                }
                    if version == expected_version =>
                {
                    status.clone().into()
                }
                DeploymentState::Deployed { status, .. } => {
                    RolloutStatus::Outdated.combine(status.clone().into())
                }
            })
            .fold(RolloutStatus::Clean, RolloutStatus::combine);

        Ok((combined, current_state))
    }
}

fn determine_rollout_status(
    name: &str,
    dep: &::kubeclient::resources::Deployment,
) -> RolloutStatusReason {
    if let Some(status) = &dep.status {
        if dep.metadata.generation > status.observed_generation {
            return RolloutStatusReason::NotYetObserved;
        }

        let progressing_condition = status
            .conditions
            .as_ref()
            .and_then(|c| c.iter().find(|c| c.type_ == "Progressing"));

        if progressing_condition
            .as_ref()
            .and_then(|c| c.reason.as_ref())
            .map(|r| r == "ProgressDeadlineExceeded")
            .unwrap_or(false)
        {
            return RolloutStatusReason::Failed {
                message: format!("Deployment {} exceeded its progress deadline", name),
            };
        }

        let updated_replicas = status.updated_replicas.unwrap_or(0);

        if dep
            .spec
            .replicas
            .map(|r| r > updated_replicas)
            .unwrap_or(false)
        {
            // not enough replicas yet
            return RolloutStatusReason::NotAllUpdated {
                expected: dep.spec.replicas.unwrap(),
                updated: updated_replicas,
            };
        }

        if status
            .replicas
            .map(|r| r > updated_replicas)
            .unwrap_or(false)
        {
            // old replicas remaining
            return RolloutStatusReason::OldReplicasPending {
                number: status.replicas.unwrap() - updated_replicas,
            };
        }

        if status.available_replicas.unwrap_or(0) < updated_replicas {
            // not all updated replicas available
            return RolloutStatusReason::UpdatedUnavailable {
                updated: updated_replicas,
                available: status.available_replicas.unwrap_or(0),
            };
        }

        info!("Deployment {} is clean: {:?}", name, status);

        RolloutStatusReason::Clean
    } else {
        // TODO maybe instead return that the status could not be determined in these cases?
        warn!("Deployment {} has no status!", name);
        RolloutStatusReason::NoStatus
    }
}

fn get_deployment_state(
    client: KubeClient<Deployment>,
    d: &Deployable,
) -> Result<Option<DeploymentState>, Error> {
    let kube_deployment = get_kubernetes_resource(client, &d.name)?;

    let kube_deployment = if let Some(k) = kube_deployment {
        k
    } else {
        return Ok(None);
    };

    let rollout_status = determine_rollout_status(&d.name, &kube_deployment);

    let state = to_deployable_state(&d, &kube_deployment, rollout_status);

    Ok(Some(state))
}

fn get_simple_deployable_state<T: Resource>(
    client: KubeClient<T>,
    d: &Deployable,
) -> Result<Option<DeploymentState>, Error> {
    let resource = get_kubernetes_resource(client, &d.name)?;

    let resource = if let Some(k) = resource {
        k
    } else {
        return Ok(None);
    };

    let state = to_deployable_state(&d, &resource, RolloutStatusReason::Clean);

    Ok(Some(state))
}

fn to_deployable_state<T: Resource>(
    deployable: &Deployable,
    resource: &T,
    rollout_status: RolloutStatusReason,
) -> DeploymentState {
    let version_annotation = resource
        .metadata()
        .annotations
        .as_ref()
        .and_then(|ann| ann.get(VERSION_ANNOTATION))
        .map(|s| s.as_str());

    let version = version_annotation.unwrap_or("");

    let state = DeploymentState::Deployed {
        version: version
            .parse()
            .unwrap_or(VersionHash::from_bytes(&[0; 20]).unwrap()),
        expected_version: deployable.version,
        status: rollout_status,
    };

    state
}

fn get_kubernetes_resource<T: Resource>(client: KubeClient<T>, name: &str) -> Result<Option<T>, Error> {
    let result = match client.get(name) {
        Ok(r) => r,
        Err(ref e) if e.http_status() == Some(404) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    Ok(Some(result))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct MinimalResource {
    api_version: String,
    kind: Kind,
    metadata: ObjectMeta,
}

fn determine_kind(data: &serde_yaml::Value) -> Result<Kind, Error> {
    // TODO: it should be possible to do this without cloning
    let resource: MinimalResource = serde_yaml::from_value(data.clone())?;
    Ok(resource.kind)
}
