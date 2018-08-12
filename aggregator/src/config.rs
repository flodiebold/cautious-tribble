use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use failure::Error;
use serde_yaml;

use common;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub common: common::Config,
}

impl Config {
    pub fn load(file: &Path) -> Result<Config, Error> {
        Ok(serde_yaml::from_reader(File::open(file)?)?)
    }
}
