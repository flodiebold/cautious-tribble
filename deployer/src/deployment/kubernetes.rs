use std::collections::HashMap;

use kubeclient::{Kubernetes};
use kubeclient::clients::ReadClient;
use failure::{Error, SyncFailure};

use ::{Deployment, VersionHash};
use super::Deployer;

const VERSION_ANNOTATION: &str = "new-dm/version";

pub struct KubernetesDeployer {
    client: Kubernetes
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum DeploymentState {
    NotDeployed,
    Deployed(VersionHash)
}

impl KubernetesDeployer {
    pub fn new(config: &str) -> Result<KubernetesDeployer, Error> {
        Ok(KubernetesDeployer {
            client: Kubernetes::load_conf(config)
                .map_err(SyncFailure::new)?
                .namespace("default")
        })
    }

    fn retrieve_current_state(&mut self, deployments: &[Deployment]) -> Result<HashMap<String, DeploymentState>, Error> {
        let mut result = HashMap::with_capacity(deployments.len());

        for d in deployments {
            // TODO don't need to do exists, just do proper error handling in kubeclient and handle 404
            let exists = self.client
                .deployments()
                .exists(&d.spec.name)
                .map_err(SyncFailure::new)?;

            if !exists {
                eprintln!("Deployment {} does not exist", d.spec.name);
                result.insert(d.spec.name.clone(), DeploymentState::NotDeployed);
                continue;
            }

            let kube_deployment = self.client
                .deployments()
                .get(&d.spec.name)
                .map_err(SyncFailure::new)?;

            let version_annotation = kube_deployment
                .metadata
                .annotations
                .as_ref()
                .and_then(|ann| ann.get(VERSION_ANNOTATION));

            let version = if let Some(v) = version_annotation { v } else {
                eprintln!("Deployment {} does not have a version annotation! Ignoring.", d.spec.name);
                continue;
            };

            result.insert(d.spec.name.clone(), DeploymentState::Deployed(version.parse()?));
        }

        Ok(result)
    }
}

impl Deployer for KubernetesDeployer {
    fn deploy(&mut self, deployments: &[Deployment]) -> Result<(), Error> {
        let current_state = self.retrieve_current_state(deployments)?;

        for d in deployments {
            eprintln!("looking at {}", d.spec.name);
            let deployed_version = if let Some(v) = current_state.get(&d.spec.name) { *v } else {
                eprintln!("no known version, not deploying");
                continue;
            };

            if deployed_version == DeploymentState::Deployed(d.version) {
                eprintln!("same version, not deploying");
                continue;
            }

            eprintln!("Deploying {} version {} (tag: {:?})", d.spec.name, d.version, d.spec.tag);
        }

        Ok(())
    }
}
