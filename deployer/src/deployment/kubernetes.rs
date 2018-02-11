use kubeclient::{Kubernetes};
use kubeclient::clients::ReadClient;
use failure::{Error, SyncFailure};

use ::Deployment;
use super::Deployer;

const VERSION_ANNOTATION: &str = "new-dm/version";

pub struct KubernetesDeployer {
    client: Kubernetes
}

impl KubernetesDeployer {
    pub fn new(config: &str) -> Result<KubernetesDeployer, Error> {
        Ok(KubernetesDeployer {
            client: Kubernetes::load_conf(config)
                .map_err(SyncFailure::new)?
                .namespace("default")
        })
    }

    fn retrieve_current_status(&mut self, deployments: &[Deployment]) -> Result<Vec<Deployment>, Error> {
        let result = Vec::with_capacity(deployments.len());

        for d in deployments {
            let kube_deployment = self.client
                .deployments()
                .get(&d.spec.name)
                .map_err(SyncFailure::new)?;

            let version_annotation = kube_deployment
                .metadata
                .annotations
                .as_ref()
                .and_then(|ann| ann.get(VERSION_ANNOTATION));
        }

        Ok(result)
    }
}

impl Deployer for KubernetesDeployer {
    fn deploy(&mut self, deployments: &[Deployment]) -> Result<(), Error> {
        Ok(())
    }
}
