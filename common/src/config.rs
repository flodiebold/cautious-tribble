use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub versions_url: String,
    pub versions_checkout_path: String,
    pub api_port: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Env {
    pub versions_url: String,
    pub versions_checkout_path: String,
    pub ssh_username: Option<String>,
    pub ssh_public_key: Option<String>,
    pub ssh_private_key: Option<String>,
    // pub api_port: Option<u16>, // doesn't work with envy in #[serde(flatten)]
}
