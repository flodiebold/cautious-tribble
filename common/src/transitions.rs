use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde_derive::{Deserialize, Serialize};

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
    Blocked { message: String },
    /// A precondition check was negative.
    CheckFailed { message: String },
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Lock {
    pub reasons: Vec<String>,
}

impl Lock {
    pub fn is_locked(&self) -> bool {
        !self.reasons.is_empty()
    }

    pub fn add_reason(&mut self, reason: &str) {
        if !self.reasons.iter().any(|r| r == reason) {
            self.reasons.push(reason.to_owned());
        }
    }

    pub fn remove_reason(&mut self, reason: &str) {
        self.reasons.retain(|r| r != reason);
    }
}

impl Default for Lock {
    fn default() -> Lock {
        Lock {
            reasons: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Locks {
    #[serde(default)]
    pub env_lock: Lock,
    #[serde(default)]
    pub resource_locks: HashMap<String, Lock>,
}

impl Locks {
    pub fn resource_is_locked(&self, resource: &str) -> bool {
        self.resource_locks
            .get(resource)
            .map_or(false, |l| l.is_locked())
    }
}

impl Default for Locks {
    fn default() -> Locks {
        Locks {
            env_lock: Lock::default(),
            resource_locks: HashMap::default(),
        }
    }
}
