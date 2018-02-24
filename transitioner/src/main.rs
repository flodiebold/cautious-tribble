extern crate failure;
extern crate git2;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_yaml;

extern crate common;

use std::time::Duration;
use std::thread;
use std::process;

use failure::Error;

use git2::{ObjectType, Repository, Signature};

use common::git::{self, TreeZipper};

mod locks;

#[derive(Debug, Serialize, Deserialize)]
struct Transition {
    source: String,
    target: String,
}

#[derive(Debug)]
enum TransitionResult {
    /// The transition was performed successfully.
    Success,
    /// The transition was not applicable for some reason, or there was no change.
    Skipped,
    /// The transition might be applicable soon, and the source env should not
    /// be changed until then.
    Blocked,
    /// A precondition check was negative.
    CheckFailed,
}

fn run_transition(transition: &Transition, repo: &Repository) -> Result<TransitionResult, Error> {
    let target_locks = locks::load_locks(repo, &transition.target)?;
    if target_locks.env_lock.is_locked() {
        return Ok(TransitionResult::Skipped);
    }

    let head_commit = git::get_head_commit(repo)?;

    let tree = head_commit.tree()?;
    let mut source = TreeZipper::from(repo, tree.clone());
    source.descend(&transition.source)?;
    source.descend("deployments")?;
    let source_deployments = if let Some(t) = source.into_inner() {
        t
    } else {
        return Ok(TransitionResult::Skipped);
    };

    let mut target = TreeZipper::from(repo, tree.clone());
    target.descend(&transition.target)?;
    target.descend("deployments")?;

    target.rebuild(|b| {
        // copy over blob references
        for entry in source_deployments.iter() {
            if entry.kind() == Some(ObjectType::Blob) {
                // find corresponding entry in target deployments
                b.insert(entry.name_bytes(), entry.id(), entry.filemode())?;
            }
        }
        Ok(())
    })?;

    target.ascend()?;
    target.ascend()?;

    let new_tree = target.into_inner().expect("new tree should not be None");

    if new_tree.id() == tree.id() {
        // nothing changed
        return Ok(TransitionResult::Skipped);
    }

    let signature = Signature::now("DM Transitioner", "n/a")?;

    repo.commit(
        Some("refs/dm_head"),
        &signature,
        &signature,
        &format!("Mirroring {} to {}", transition.source, transition.target),
        &new_tree,
        &[&head_commit],
    )?;

    let url = "../versions.git";
    git::push(repo, url)?;

    Ok(TransitionResult::Success)
}

fn run() -> Result<(), Error> {
    let url = "../versions.git";
    let checkout_path = "./versions_checkout";
    let repo = git::init_or_open(checkout_path)?;

    let transitions = vec![
        Transition {
            source: "available".to_owned(),
            target: "dev".to_owned(),
        },
        Transition {
            source: "dev".to_owned(),
            target: "prod".to_owned(),
        },
    ];

    loop {
        git::update(&repo, url)?;

        for transition in transitions.iter() {
            match run_transition(&transition, &repo) {
                Ok(TransitionResult::Success) => break,
                Ok(TransitionResult::Skipped) => continue,
                // TODO we could instead just block transitions that touch the source env
                Ok(TransitionResult::Blocked) => break,
                Ok(TransitionResult::CheckFailed) => continue,
                Err(error) => {
                    eprintln!("Transition failed: {}\n{}", error, error.backtrace());
                    for cause in error.causes() {
                        eprintln!("caused by: {}", cause);
                    }
                    break;
                }
            }
        }

        thread::sleep(Duration::from_millis(1000));
    }
}

fn main() {
    match run() {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("{}\n{}", e, e.backtrace());
            for cause in e.causes() {
                eprintln!("caused by: {}", cause);
            }
            process::exit(1);
        }
    }
}
