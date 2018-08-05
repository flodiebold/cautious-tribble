use std::collections::HashMap;

use failure::Error;
use serde_json;

use common::deployment::{DeploymentState, RolloutStatusReason};
use common::repo::Id;

use super::{Deployable, Deployer};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {}

impl Config {
    pub fn create(&self) -> Result<MockDeployer, Error> {
        MockDeployer::new(self)
    }
}

struct MockResource {
    version: Id,
    #[allow(dead_code)]
    content: serde_json::Value,
}

pub struct MockDeployer {
    resources: HashMap<String, MockResource>,
}

impl MockDeployer {
    fn new(_config: &Config) -> Result<MockDeployer, Error> {
        Ok(MockDeployer {
            resources: HashMap::new(),
        })
    }
}

impl Deployer for MockDeployer {
    fn retrieve_current_state(
        &mut self,
        resources: &[Deployable],
    ) -> Result<HashMap<String, DeploymentState>, Error> {
        let mut result = HashMap::new();
        for resource in resources {
            let mock = self.resources.get(&resource.name);
            let state = if let Some(mock) = mock {
                DeploymentState::Deployed {
                    version: mock.version,
                    expected_version: resource.version,
                    status: RolloutStatusReason::Clean,
                }
            } else {
                DeploymentState::NotDeployed
            };

            result.insert(resource.name.clone(), state);
        }
        Ok(result)
    }

    fn deploy(&mut self, resource: &Deployable) -> Result<(), Error> {
        // TODO: allow simulating errors etc. by setting properties in the content
        self.resources.insert(
            resource.name.clone(),
            MockResource {
                version: resource.version,
                content: resource.merged_content.clone(),
            },
        );
        Ok(())
    }
}
