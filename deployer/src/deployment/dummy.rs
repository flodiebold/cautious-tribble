use failure::Error;

use Deployment;
use super::Deployer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config;

impl Config {
    pub fn create(&self) -> DummyDeployer {
        DummyDeployer
    }
}

pub struct DummyDeployer;

impl Deployer for DummyDeployer {
    fn deploy(&mut self, _deployments: &[Deployment]) -> Result<(), Error> {
        Ok(())
    }
}
