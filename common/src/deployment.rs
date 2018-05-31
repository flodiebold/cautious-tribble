use std::collections::BTreeMap;
use std::collections::HashMap;

use git::VersionHash;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployerStatus {
    pub deployed_version: VersionHash,
    pub last_successfully_deployed_version: Option<VersionHash>,
    pub rollout_status: RolloutStatus,
    // TODO rename to by_deployable
    pub status_by_deployment: HashMap<String, DeploymentState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllDeployerStatus {
    pub deployers: BTreeMap<String, DeployerStatus>,
}

impl AllDeployerStatus {
    pub fn empty() -> AllDeployerStatus {
        AllDeployerStatus {
            deployers: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RolloutStatusReason {
    Clean,
    Failed { message: String },
    NotYetObserved,
    NotAllUpdated { expected: i32, updated: i32 },
    OldReplicasPending { number: i32 },
    UpdatedUnavailable { updated: i32, available: i32 },
    NoStatus,
}

impl From<RolloutStatusReason> for RolloutStatus {
    fn from(r: RolloutStatusReason) -> RolloutStatus {
        match r {
            RolloutStatusReason::Clean => RolloutStatus::Clean,
            RolloutStatusReason::Failed { .. } => RolloutStatus::Failed,
            RolloutStatusReason::NotYetObserved
            | RolloutStatusReason::NotAllUpdated { .. }
            | RolloutStatusReason::OldReplicasPending { .. }
            | RolloutStatusReason::UpdatedUnavailable { .. }
            | RolloutStatusReason::NoStatus { .. } => RolloutStatus::InProgress,
        }
    }
}

// TODO rename to DeployableState
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentState {
    NotDeployed,
    Deployed {
        version: VersionHash,
        expected_version: VersionHash,
        status: RolloutStatusReason,
    },
}
