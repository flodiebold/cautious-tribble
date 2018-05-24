use std::fs::File;
use std::path::Path;

use failure::Error;
use serde_yaml;
use indexmap::IndexMap;

use common;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub common: common::Config,
    pub transitions: IndexMap<String, super::Transition>,
    pub deployer_url: Option<String>,
}

impl Config {
    pub fn load(file: &Path) -> Result<Config, Error> {
        Ok(serde_yaml::from_reader(File::open(file)?)?)
    }
}
