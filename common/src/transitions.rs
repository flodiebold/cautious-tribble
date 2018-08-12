use std::collections::VecDeque;

use chrono::{DateTime, Utc};

use super::repo::Id;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionResult {
    /// The transition was performed successfully.
    Success(Id),
    /// The transition was not applicable for some reason, or there was no change.
    Skipped(SkipReason),
    /// The transition might be applicable soon, and the source env should not
    /// be changed until then.
    Blocked,
    /// A precondition check was negative.
    CheckFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkipReason {
    Scheduled { time: DateTime<Utc> },
    TargetLocked,
    SourceMissing,
    NoChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionSuccessfulRunInfo {
    pub time: DateTime<Utc>,
    pub committed_version: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionRunInfo {
    pub time: DateTime<Utc>,
    // TODO add version: Id
    pub result: TransitionResult,
}
