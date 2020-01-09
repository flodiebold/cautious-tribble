use std::collections::HashMap;

use failure::{bail, format_err, Error, ResultExt};
use k8s_openapi::{api, apimachinery::pkg::apis::meta::v1::ObjectMeta};
use kubernetes::config::Configuration as KubeConfig;
use log::{debug, info, warn};
use serde_derive::{Deserialize, Serialize};
use serde_json::json;

use common::deployment::{ResourceState, RolloutStatusReason};
use common::repo::Id;

use super::{Deployer, Resource};
use crate::Env;

const VERSION_ANNOTATION: &str = "new-dm/version";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    namespace: String,
    context: Option<String>,
    cluster: Option<String>,
    user: Option<String>,
    glob: Option<String>,
}

impl Config {
    pub(crate) fn create(&self, env: &Env) -> Result<KubernetesDeployer, Error> {
        KubernetesDeployer::new(env, self)
    }
}

pub struct KubernetesDeployer {
    namespace: String,
    context: Option<String>,
    cluster: Option<String>,
    user: Option<String>,
    client: KubeConfig,
}

impl KubernetesDeployer {
    fn new(_env: &Env, config: &Config) -> Result<KubernetesDeployer, Error> {
        let options = kubernetes::config::ConfigOptions {
            cluster: config.cluster.clone(),
            context: config.context.clone(),
            user: config.user.clone(),
        };
        let configuration = kubernetes::config::incluster_config()
            .or_else(|_| kubernetes::config::load_kube_config_with(options))?;
        Ok(KubernetesDeployer {
            client: configuration,
            context: config.context.clone(),
            cluster: config.cluster.clone(),
            user: config.user.clone(),
            namespace: config.namespace.clone(),
        })
    }
    fn kubectl_apply(&self, data: &str) -> Result<(), Error> {
        // TODO: use kube API instead
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut builder = Command::new("kubectl");
        builder.args(&["apply", "--namespace", &self.namespace]);
        if let Some(context) = &self.context {
            builder.args(&["--context", &context]);
        }
        if let Some(cluster) = &self.cluster {
            builder.args(&["--cluster", &cluster]);
        }
        if let Some(user) = &self.user {
            builder.args(&["--user", &user]);
        }
        builder.args(&["-f", "-"]);
        let mut process = builder
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
            let kind = determine_kind(&d.content)?;

            let state = match kind {
                Kind::Deployment => get_deployment_state(&self.client, &self.namespace, d)?,
                // TODO these should work
                Kind::DaemonSet => bail!("Unsupported resource type: {:?}", kind),
                Kind::Pod => bail!("Unsupported resource type: {:?}", kind),

                Kind::Service => get_simple_resource_state::<api::core::v1::Service>(
                    &self.client,
                    &self.namespace,
                    d,
                )?,
                Kind::ConfigMap => get_simple_resource_state::<api::core::v1::ConfigMap>(
                    &self.client,
                    &self.namespace,
                    d,
                )?,
                Kind::Secret => get_simple_resource_state::<api::core::v1::Secret>(
                    &self.client,
                    &self.namespace,
                    d,
                )?,
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
        use serde_json::Value;
        let mut data: Value = resource.content.clone(); // TODO
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

fn determine_rollout_status(name: &str, dep: &api::apps::v1::Deployment) -> RolloutStatusReason {
    if let Some(status) = &dep.status {
        if dep
            .metadata
            .as_ref()
            .and_then(|m| m.generation)
            .unwrap_or(0)
            > status.observed_generation.unwrap_or(0)
        {
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
            .as_ref()
            .and_then(|s| s.replicas)
            .map(|r| r > updated_replicas)
            .unwrap_or(false)
        {
            // not enough replicas yet
            return RolloutStatusReason::NotAllUpdated {
                expected: dep.spec.as_ref().unwrap().replicas.unwrap(),
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
    config: &KubeConfig,
    namespace: &str,
    d: &Resource,
) -> Result<Option<ResourceState>, Error> {
    let kube_deployment =
        get_kubernetes_resource::<api::apps::v1::Deployment>(config, namespace, &d.name)?;

    let kube_deployment = if let Some(k) = kube_deployment {
        k
    } else {
        return Ok(None);
    };

    let rollout_status = determine_rollout_status(&d.name, &kube_deployment);

    let state = to_resource_state(&d, &kube_deployment, rollout_status);

    Ok(Some(state))
}

fn get_simple_resource_state<
    T: k8s_openapi::Resource
        + k8s_openapi::Metadata<Ty = ObjectMeta>
        + for<'de> serde::Deserialize<'de>,
>(
    config: &KubeConfig,
    namespace: &str,
    r: &Resource,
) -> Result<Option<ResourceState>, Error> {
    let resource = get_kubernetes_resource::<T>(&config, namespace, &r.name)?;

    let resource = if let Some(k) = resource {
        k
    } else {
        return Ok(None);
    };

    let state = to_resource_state(&r, &resource, RolloutStatusReason::Clean);

    Ok(Some(state))
}

fn to_resource_state<T: k8s_openapi::Metadata<Ty = ObjectMeta>>(
    resource: &Resource,
    kube_resource: &T,
    rollout_status: RolloutStatusReason,
) -> ResourceState {
    let version_annotation = kube_resource
        .metadata()
        .and_then(|m| m.annotations.as_ref())
        .and_then(|ann| ann.get(VERSION_ANNOTATION))
        .map(|s| s.as_str());

    let version = version_annotation.unwrap_or("");

    ResourceState::Deployed {
        version: version.parse().unwrap_or_else(|_| Id([0; 20])),
        expected_version: resource.version,
        status: rollout_status,
    }
}

fn get_kubernetes_resource<
    T: k8s_openapi::Resource
        + k8s_openapi::Metadata<Ty = ObjectMeta>
        + for<'de> serde::Deserialize<'de>,
>(
    config: &KubeConfig,
    namespace: &str,
    name: &str,
) -> Result<Option<T>, Error> {
    let url = format!(
        "{}/{}/{}/namespaces/{}/{}s/{}",
        config.base_path,
        if T::group() == "" { "api" } else { "apis" },
        T::api_version(),
        namespace,
        T::kind().to_ascii_lowercase(),
        name
    );
    let response = config.client.get(&url).send()?;
    debug!("GET {} => {}", url, response.status());
    let result = match response.error_for_status() {
        Ok(mut r) => r.json()?,
        Err(e) => {
            if e.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                return Ok(None);
            }
            return Err(e.into());
        }
    };
    Ok(Some(result))
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    DaemonSet,
    Deployment,
    ConfigMap,
    NetworkPolicy,
    Node,
    Pod,
    Secret,
    Service,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct MinimalResource {
    api_version: String,
    kind: Kind,
    metadata: ObjectMeta,
}

fn determine_kind(data: &serde_json::Value) -> Result<Kind, Error> {
    let resource: MinimalResource = serde_json::from_value(data.clone())?;
    Ok(resource.kind)
}
