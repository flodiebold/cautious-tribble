use std::fs::File;
use std::path::Path;

use failure::Error;
use indexmap::IndexMap;
use serde_derive::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub transitions: IndexMap<String, super::Transition>,
}

impl Config {
    pub fn load(file: &Path) -> Result<Config, Error> {
        Ok(serde_yaml::from_reader(File::open(file)?)?)
    }
}
