use std::collections::BTreeMap;
use std::collections::HashMap;

use repo::Id;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum RolloutStatus {
    InProgress,
    Clean,
    Outdated,
    Failed,
}

impl RolloutStatus {
    pub fn combine(self, other: RolloutStatus) -> RolloutStatus {
        use self::RolloutStatus::*;
        match self {
            Clean => other,
            InProgress => match other {
                Clean | InProgress | Outdated => InProgress,
                Failed => Failed,
            },
            Outdated => match other {
                Clean | Outdated => Outdated,
                InProgress => InProgress,
                Failed => Failed,
            },
            Failed => Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeployerStatus {
    pub deployed_version: Id,
    pub last_successfully_deployed_version: Option<Id>,
    pub rollout_status: RolloutStatus,
    pub status_by_resource: HashMap<String, ResourceState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AllDeployerStatus {
    pub deployers: BTreeMap<String, DeployerStatus>,
}

impl AllDeployerStatus {
    pub fn empty() -> AllDeployerStatus {
        Default::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason")]
pub enum RolloutStatusReason {
    Clean,
    Failed { message: String },
    NotYetObserved,
    NotAllUpdated { expected: i32, updated: i32 },
    OldReplicasPending { number: i32 },
    UpdatedUnavailable { updated: i32, available: i32 },
    NoStatus,
    NotFound,
    ValidationError {
        #[serde(flatten)]
        error: ResourceValidationError
    }
}

impl From<RolloutStatusReason> for RolloutStatus {
    fn from(r: RolloutStatusReason) -> RolloutStatus {
        match r {
            RolloutStatusReason::Clean => RolloutStatus::Clean,
            RolloutStatusReason::Failed { .. }
            | RolloutStatusReason::ValidationError { .. } => RolloutStatus::Failed,
            RolloutStatusReason::NotYetObserved
            | RolloutStatusReason::NotAllUpdated { .. }
            | RolloutStatusReason::OldReplicasPending { .. }
            | RolloutStatusReason::UpdatedUnavailable { .. }
            | RolloutStatusReason::NoStatus { .. }
            | RolloutStatusReason::NotFound => RolloutStatus::InProgress,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceState {
    expected_version: Id,
    status: RolloutStatusReason,
    deployed_version: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error")]
pub enum ResourceValidationError {
    BaseFileParsing(ParsingError),
    VersionFileParsing(ParsingError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsingError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}
