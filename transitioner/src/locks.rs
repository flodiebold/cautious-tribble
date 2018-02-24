use git2::Repository;
use failure::{Error, ResultExt};
use serde_yaml;

use super::TreeZipper;

#[derive(Debug, Serialize, Deserialize)]
pub struct Lock {
    pub reasons: Vec<String>,
}

impl Lock {
    pub fn is_locked(&self) -> bool {
        !self.reasons.is_empty()
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
    pub env_lock: Lock,
}

impl Default for Locks {
    fn default() -> Locks {
        Locks {
            env_lock: Lock::default(),
        }
    }
}

pub fn load_locks<'repo>(repo: &'repo Repository, env: &str) -> Result<Locks, Error> {
    let head = ::get_head_commit(repo)?;
    let tree = head.tree()?;

    let mut zipper = TreeZipper::from(repo, tree);
    zipper.descend(env)?;

    let locks_blob = if let Some(blob) = zipper.get_blob("locks.yaml")? {
        blob
    } else {
        return Ok(Locks::default());
    };

    let locks = serde_yaml::from_slice(locks_blob.content())
        .with_context(|_| format!("deserializing locks.yaml for env {} failed", env))?;

    Ok(locks)
}
