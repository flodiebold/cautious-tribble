extern crate failure;
extern crate git2;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_yaml;
#[macro_use]
extern crate structopt;
#[macro_use]
extern crate log;
extern crate env_logger;

extern crate common;

use std::path::PathBuf;
use std::time::Duration;
use std::thread;
use std::process;

use failure::Error;
use git2::{ObjectType, Repository, Signature};
use structopt::StructOpt;

use common::git::{self, TreeZipper};

mod locks;
mod config;

use config::Config;

#[derive(Debug, StructOpt)]
struct Options {
    /// The location of the configuration file.
    #[structopt(short = "c", long = "config", parse(from_os_str))]
    config: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
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

fn run_transition(
    transition: &Transition,
    repo: &Repository,
    config: &Config,
) -> Result<TransitionResult, Error> {
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

    let commit = repo.commit(
        Some("refs/dm_head"),
        &signature,
        &signature,
        &format!("Mirroring {} to {}", transition.source, transition.target),
        &new_tree,
        &[&head_commit],
    )?;

    info!("Made commit {} mirroring {} to {}. Pushing...", commit, transition.source, transition.target);

    git::push(repo, &config.common.versions_url)?;

    info!("Pushed.");

    Ok(TransitionResult::Success)
}

fn run() -> Result<(), Error> {
    env_logger::init();
    let options = Options::from_args();
    let config = config::Config::load(&options.config)?;
    let repo = git::init_or_open(&config.common.versions_checkout_path)?;

    info!("Transitioner running.");

    loop {
        git::update(&repo, &config.common.versions_url)?;

        for transition in config.transitions.iter() {
            match run_transition(&transition, &repo, &config) {
                Ok(TransitionResult::Success) => break,
                Ok(TransitionResult::Skipped) => continue,
                // TODO we could instead just block transitions that touch the source env
                Ok(TransitionResult::Blocked) => break,
                Ok(TransitionResult::CheckFailed) => continue,
                Err(error) => {
                    error!("Transition failed: {}\n{}", error, error.backtrace());
                    for cause in error.causes() {
                        error!("caused by: {}", cause);
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
