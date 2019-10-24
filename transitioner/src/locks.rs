use failure::{Error, ResultExt};
use git2::Repository;

use common::git::{self, TreeZipper};

pub use common::transitions::{Lock, Locks};

pub fn load_locks<'repo>(repo: &'repo Repository, env: &str) -> Result<Locks, Error> {
    let head = git::get_head_commit(repo)?;
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
