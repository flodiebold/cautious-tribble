use kubeclient::clients::KubeClient;
use kubeclient::resources::Deployment;
use std::collections::HashMap;

use failure::{bail, format_err, Error, ResultExt};
use k8s_openapi::v1_10::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kubeclient::clients::ReadClient;
use kubeclient::resources::{Kind, Resource as KubeResource};
use kubeclient::Kubernetes;
use log::{debug, info, warn};
use serde_derive::{Deserialize, Serialize};
use serde_json::json;

use common::deployment::{ResourceState, RolloutStatusReason};
use common::repo::Id;

use super::{Deployer, Resource};

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
}

impl Deployer for KubernetesDeployer {
    fn retrieve_current_state(
        &mut self,
        resources: &[Resource],
    ) -> Result<HashMap<String, ResourceState>, Error> {
        let mut result = HashMap::with_capacity(resources.len());

        for d in resources {
            let kind = determine_kind(&d.merged_content)?;

            let state = match kind {
                Kind::Deployment => get_deployment_state(&self.client.deployments(), d)?,
                // TODO these should work
                Kind::DaemonSet => bail!("Unsupported resource type: {:?}", kind),
                Kind::Pod => bail!("Unsupported resource type: {:?}", kind),

                Kind::Service => get_simple_resource_state(&self.client.services(), d)?,
                Kind::ConfigMap => get_simple_resource_state(&self.client.config_maps(), d)?,
                Kind::Secret => get_simple_resource_state(&self.client.secrets(), d)?,
                Kind::NetworkPolicy | Kind::Node => {
                    bail!("Unsupported resource type: {:?}", kind);
                }
            };

            if let Some(state) = state {
                result.insert(d.name.clone(), state);
            } else {
                warn!("Resource {} does not exist", d.name);
                result.insert(d.name.clone(), ResourceState::NotDeployed);
            }
        }

        Ok(result)
    }

    fn deploy(&mut self, resource: &Resource) -> Result<(), Error> {
        use serde_json::{self, Value};
        let mut data: Value = resource.merged_content.clone(); // TODO
        {
            let metadata = data
                .get_mut("metadata")
                .ok_or_else(|| format_err!("bad resource: no metadata"))?
                .as_object_mut()
                .ok_or_else(|| format_err!("bad resource: metadata not an object"))?;
            // TODO check name
            let annotations = metadata
                .entry("annotations")
                .or_insert(json!({}))
                .as_object_mut()
                .ok_or_else(|| format_err!("bad resource: annotations not an object"))?;

            let value = json!(resource.version.to_string());
            annotations.insert(VERSION_ANNOTATION.to_string(), value);
        }

        let data = serde_json::to_string(&data)?;

        self.kubectl_apply(&data)?;

        Ok(())
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
    client: &KubeClient<Deployment>,
    d: &Resource,
) -> Result<Option<ResourceState>, Error> {
    let kube_deployment = get_kubernetes_resource(client, &d.name)?;

    let kube_deployment = if let Some(k) = kube_deployment {
        k
    } else {
        return Ok(None);
    };

    let rollout_status = determine_rollout_status(&d.name, &kube_deployment);

    let state = to_resource_state(&d, &kube_deployment, rollout_status);

    Ok(Some(state))
}

fn get_simple_resource_state<T: KubeResource>(
    client: &KubeClient<T>,
    r: &Resource,
) -> Result<Option<ResourceState>, Error> {
    let resource = get_kubernetes_resource(&client, &r.name)?;

    let resource = if let Some(k) = resource {
        k
    } else {
        return Ok(None);
    };

    let state = to_resource_state(&r, &resource, RolloutStatusReason::Clean);

    Ok(Some(state))
}

fn to_resource_state<T: KubeResource>(
    resource: &Resource,
    kube_resource: &T,
    rollout_status: RolloutStatusReason,
) -> ResourceState {
    let version_annotation = kube_resource
        .metadata()
        .annotations
        .as_ref()
        .and_then(|ann| ann.get(VERSION_ANNOTATION))
        .map(|s| s.as_str());

    let version = version_annotation.unwrap_or("");

    ResourceState::Deployed {
        version: version.parse().unwrap_or_else(|_| Id([0; 20])),
        expected_version: resource.version,
        status: rollout_status,
    }
}

fn get_kubernetes_resource<T: KubeResource>(
    client: &KubeClient<T>,
    name: &str,
) -> Result<Option<T>, Error> {
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

fn determine_kind(data: &serde_json::Value) -> Result<Kind, Error> {
    // TODO: it should be possible to do this without cloning
    let resource: MinimalResource = serde_json::from_value(data.clone())?;
    Ok(resource.kind)
}
