use std::collections::BTreeMap;
use std::collections::HashMap;

use serde_derive::{Deserialize, Serialize};

use crate::repo::Id;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum ResourceState {
    NotDeployed,
    Deployed {
        version: Id,
        expected_version: Id,
        #[serde(flatten)]
        status: RolloutStatusReason,
    },
}
