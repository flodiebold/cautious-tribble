use deployment::AllDeployerStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    DeployerStatus(AllDeployerStatus),
}
