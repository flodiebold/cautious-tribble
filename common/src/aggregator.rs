use deployment::AllDeployerStatus;
use transitions::AllTransitionStatus;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FullStatus {
    pub counter: usize,
    #[serde(flatten)]
    pub deployers: AllDeployerStatus,
    pub transitions: AllTransitionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    FullStatus(FullStatus),
    DeployerStatus {
        counter: usize,
        #[serde(flatten)]
        content: AllDeployerStatus,
    },
    TransitionStatus {
        counter: usize,
        transitions: AllTransitionStatus,
    },
}
