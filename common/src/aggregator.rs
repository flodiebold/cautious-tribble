use deployment::AllDeployerStatus;
use transitions::AllTransitionStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    DeployerStatus(AllDeployerStatus),
    TransitionStatus(AllTransitionStatus),
}
