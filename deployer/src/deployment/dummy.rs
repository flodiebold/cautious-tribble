use failure::Error;
use std::collections::HashMap;

use super::{Deployer, Deployment, DeploymentState};
use RolloutStatus;

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

    fn check_rollout_status(
        &mut self,
        _deployments: &[Deployment],
    ) -> Result<(RolloutStatus, HashMap<String, DeploymentState>), Error> {
        Ok((RolloutStatus::Clean, HashMap::new()))
    }
}
