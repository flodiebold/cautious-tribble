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
            if let Some(current_deployment) = self.current.get(&d.name) {
                if current_deployment.version != d.version {
                    eprintln!("Deployment {}: {} -> {}; message: {}",
                              d.name,
                              current_deployment.version,
                              d.version,
                              d.message);
                }
            } else {
                eprintln!("Deployment {}: -> {}; message: {}",
                          d.name, d.version, d.message);
            }

            self.current.insert(d.name.clone(), d.clone());
        }

        Ok(())
    }
}
