use std::collections::VecDeque;

use chrono::{DateTime, Utc};
use indexmap::IndexMap;

use super::repo::Id;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result")]
pub enum TransitionResult {
    /// The transition was performed successfully.
    Success { committed_version: Id },
    /// The transition was not applicable for some reason, or there was no change.
    Skipped(SkipReason),
    /// The transition might be applicable soon, and the source env should not
    /// be changed until then.
    Blocked,
    /// A precondition check was negative.
    CheckFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason")]
pub enum SkipReason {
    Scheduled { time: DateTime<Utc> },
    TargetLocked,
    SourceMissing,
    NoChange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransitionStatusInfo {
    pub successful_runs: VecDeque<TransitionSuccessfulRunInfo>,
    pub last_run: Option<TransitionRunInfo>,
}

impl TransitionStatusInfo {
    pub fn new() -> TransitionStatusInfo {
        TransitionStatusInfo {
            successful_runs: VecDeque::with_capacity(101),
            last_run: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransitionSuccessfulRunInfo {
    pub time: DateTime<Utc>,
    pub committed_version: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransitionRunInfo {
    pub time: Option<DateTime<Utc>>,
    // TODO add version: Id
    #[serde(flatten)]
    pub result: TransitionResult,
}

pub type AllTransitionStatus = IndexMap<String, TransitionStatusInfo>;
