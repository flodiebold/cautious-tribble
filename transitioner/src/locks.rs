use git2::Repository;
use failure::{Error, ResultExt};
use serde_yaml;

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
    let head = repo.find_reference("refs/dm_head")
        .context("refs/dm_head not found")?;

    let tree = head.peel_to_tree()?;

    let env_tree_obj = tree.get_name(&env)
        .ok_or_else(|| format_err!("env {} not found in repo", env))?
        .to_object(repo)?;
    let env_tree = env_tree_obj.peel_to_tree()?;

    let locks_file = env_tree.get_name("locks.yaml");

    let locks_blob = if let Some(entry) = locks_file {
        // TODO go back to peel_to_blob if https://github.com/alexcrichton/git2-rs/issues/299 is fixed
        // entry.to_object(repo)?.peel_to_blob()?
        entry
            .to_object(repo)?
            .peel(::git2::ObjectType::Blob)?
            .into_blob()
            .unwrap()
    } else {
        return Ok(Locks::default());
    };

    let locks = serde_yaml::from_slice(locks_blob.content())
        .with_context(|_| format!("deserializing locks.yaml for env {} failed", env))?;

    Ok(locks)
}
