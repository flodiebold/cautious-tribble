use std::collections::HashMap;

use kubeclient::Kubernetes;
use kubeclient::clients::ReadClient;
use failure::{Error, ResultExt, SyncFailure};

use {Deployment, VersionHash};
use super::Deployer;

const VERSION_ANNOTATION: &str = "new-dm/version";

pub struct KubernetesDeployer {
    client: Kubernetes,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum DeploymentState {
    NotDeployed,
    Deployed(VersionHash),
}

impl KubernetesDeployer {
    pub fn new(config: &str) -> Result<KubernetesDeployer, Error> {
        Ok(KubernetesDeployer {
            client: Kubernetes::load_conf(config)
                .map_err(SyncFailure::new)?
                .namespace("default"),
        })
    }

    fn retrieve_current_state(
        &mut self,
        deployments: &[Deployment],
    ) -> Result<HashMap<String, DeploymentState>, Error> {
        let mut result = HashMap::with_capacity(deployments.len());

        for d in deployments {
            // TODO don't need to do exists, just do proper error handling in kubeclient and handle 404
            let exists = self.client
                .deployments()
                .exists(&d.name)
                .map_err(SyncFailure::new)?;

            if !exists {
                eprintln!("Deployment {} does not exist", d.name);
                result.insert(d.name.clone(), DeploymentState::NotDeployed);
                continue;
            }

            let kube_deployment = self.client
                .deployments()
                .get(&d.name)
                .map_err(SyncFailure::new)?;

            let version_annotation = kube_deployment
                .metadata
                .annotations
                .as_ref()
                .and_then(|ann| ann.get(VERSION_ANNOTATION));

            let version = if let Some(v) = version_annotation {
                v
            } else {
                eprintln!(
                    "Deployment {} does not have a version annotation! Ignoring.",
                    d.name
                );
                continue;
            };

            result.insert(d.name.clone(), DeploymentState::Deployed(version.parse()?));
        }

        Ok(result)
    }

    fn do_deploy(&mut self, deployment: &Deployment) -> Result<(), Error> {
        use serde_yaml::{self, Mapping, Value};
        let mut data: Value = serde_yaml::from_slice(&deployment.content)?;
        let mut root = data.as_mapping_mut()
            .ok_or_else(|| format_err!("bad deployment yaml: root not a mapping"))?;
        {
            let metadata = root.get_mut(&Value::String("metadata".to_owned()))
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
        // TODO: configure kubectl correctly with multiple clusters
        use std::process::{Command, Stdio};
        use std::io::Write;

        let mut process = Command::new("kubectl")
            .args(&["apply", "-f", "-"])
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

        eprintln!(
            "kubectl stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );

        eprintln!(
            "kubectl stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        Ok(())
    }
}

impl Deployer for KubernetesDeployer {
    fn deploy(&mut self, deployments: &[Deployment]) -> Result<(), Error> {
        let current_state = self.retrieve_current_state(deployments)?;

        for d in deployments {
            eprintln!("looking at {}", d.name);
            let deployed_version = if let Some(v) = current_state.get(&d.name) {
                *v
            } else {
                eprintln!("no known version, not deploying");
                continue;
            };

            if deployed_version == DeploymentState::Deployed(d.version) {
                eprintln!("same version, not deploying");
                continue;
            }

            eprintln!(
                "Deploying {} version {} with content {}",
                d.name,
                d.version,
                ::std::str::from_utf8(&d.content)?
            );

            self.do_deploy(d)
                .with_context(|_| format!("Error deploying {}", d.name))?;
        }

        Ok(())
    }
}
