use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use failure::Error;
use serde_yaml;

use common;

use deployment::{self, dummy, kubernetes};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Deployer {
    Kubernetes(kubernetes::Config),
    Dummy(dummy::Config),
}

impl Deployer {
    pub fn create(&self) -> Result<Box<deployment::Deployer>, Error> {
        use self::Deployer::*;
        Ok(match *self {
            Kubernetes(ref conf) => Box::new(conf.create()?),
            Dummy(ref conf) => Box::new(conf.create()),
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
