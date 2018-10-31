use std::fs::File;
use std::path::Path;

use failure::Error;
use serde_derive::{Deserialize, Serialize};
use serde_yaml;

use common;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub common: common::Config,
    pub deployer_url: Option<String>,
    pub transitioner_url: Option<String>,
}

impl Config {
    pub fn load(file: &Path) -> Result<Config, Error> {
        Ok(serde_yaml::from_reader(File::open(file)?)?)
    }
}
