use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use failure::Error;
use serde_derive::{Deserialize, Serialize};
use serde_yaml;

use crate::deployment::{kubernetes, mock, Deployer};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DeployerConfig {
    Kubernetes(kubernetes::Config),
    Mock(mock::Config),
}

impl DeployerConfig {
    pub fn create(&self) -> Result<Box<dyn Deployer>, Error> {
        match self {
            DeployerConfig::Kubernetes(c) => c.create().map(|d| Box::new(d) as Box<dyn Deployer>),
            DeployerConfig::Mock(c) => c.create().map(|d| Box::new(d) as Box<dyn Deployer>),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub deployers: BTreeMap<String, DeployerConfig>,
}

impl Config {
    pub fn load(file: &Path) -> Result<Config, Error> {
        Ok(serde_yaml::from_reader(File::open(file)?)?)
    }
}
