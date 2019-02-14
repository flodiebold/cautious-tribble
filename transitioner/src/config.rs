use failure::Error;
use indexmap::IndexMap;
use serde_derive::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
pub struct Config {
    pub transitions: IndexMap<String, super::Transition>,
}

impl Config {
    pub fn load(data: &[u8]) -> Result<Config, Error> {
        Ok(serde_yaml::from_slice(data)?)
    }
}
