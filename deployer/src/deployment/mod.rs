use std::collections::HashMap;

use failure::Error;

use ::Deployment;

pub mod kubernetes;

pub trait Deployer {
    fn deploy(&mut self, deployments: &[Deployment]) -> Result<(), Error>;
}

pub struct DummyDeployer {
    current: HashMap<String, Deployment>
}

impl DummyDeployer {
    pub fn new() -> DummyDeployer {
        DummyDeployer {
            current: HashMap::new()
        }
    }
}

impl Deployer for DummyDeployer {
    fn deploy(&mut self, deployments: &[Deployment]) -> Result<(), Error> {
        for d in deployments {
            if let Some(current_deployment) = self.current.get(&d.spec.name) {
                if current_deployment.version != d.version {
                    eprintln!("Deployment {}: tag {:?} ({}) -> tag {:?} ({}); message: {}",
                              d.spec.name,
                              current_deployment.spec.tag, current_deployment.version,
                              d.spec.tag, d.version,
                              d.message);
                }
            } else {
                eprintln!("Deployment {}: -> tag {:?} ({}); message: {}",
                          d.spec.name, d.spec.tag, d.version, d.message);
            }

            self.current.insert(d.spec.name.clone(), d.clone());
        }

        Ok(())
    }
}
