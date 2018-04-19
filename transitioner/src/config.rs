use std::path::Path;
use std::fs::File;

use failure::Error;
use serde_yaml;

use common;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub common: common::Config,
    pub transitions: Vec<super::Transition>,
    pub deployer_url: Option<String>,
}

impl Config {
    pub fn load(file: &Path) -> Result<Config, Error> {
        Ok(serde_yaml::from_reader(File::open(file)?)?)
    }
}
