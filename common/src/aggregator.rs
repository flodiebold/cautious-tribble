use deployment::AllDeployerStatus;
use transitions::AllTransitionStatus;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FullStatus {
    pub counter: usize,
    pub deployers: AllDeployerStatus,
    pub transitions: AllTransitionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    DeployerStatus {
        counter: usize,
        #[serde(flatten)]
        content: AllDeployerStatus,
    },
    TransitionStatus {
        counter: usize,
        #[serde(flatten)]
        content: AllTransitionStatus,
    },
}
