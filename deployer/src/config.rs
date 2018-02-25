use std::path::Path;
use std::fs::File;
use std::collections::BTreeMap;

use failure::Error;
use serde_yaml;

use common;

use deployment::{self, kubernetes };

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Deployer {
    Kubernetes(kubernetes::Config),
}

impl Deployer {
    pub fn create(&self) -> Result<Box<deployment::Deployer>, Error> {
        use self::Deployer::*;
        Ok(match *self {
            Kubernetes(ref conf) => Box::new(kubernetes::KubernetesDeployer::new(conf)?)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub common: common::Config,
    pub deployers: BTreeMap<String, Deployer>,
}

impl Config {
    pub fn load(file: &Path) -> Result<Config, Error> {
        Ok(serde_yaml::from_reader(File::open(file)?)?)
    }
}
