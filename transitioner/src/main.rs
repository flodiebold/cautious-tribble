extern crate failure;
extern crate git2;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde_yaml;
#[macro_use]
extern crate structopt;
#[macro_use]
extern crate log;
extern crate crossbeam;
extern crate env_logger;
extern crate gotham;
extern crate hyper;
extern crate mime;
extern crate reqwest;

extern crate common;
#[cfg(test)]
extern crate git_fixture;

use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use failure::Error;
use git2::{ObjectType, Repository, Signature};
use structopt::StructOpt;

use common::git::{self, TreeZipper, VersionHash};

mod api;
mod config;
mod locks;
mod precondition;

use config::Config;
use precondition::{Precondition, PreconditionResult};

#[derive(Debug, StructOpt)]
struct Options {
    /// The location of the configuration file.
    #[structopt(short = "c", long = "config", parse(from_os_str))]
    config: PathBuf,
}

pub struct ServiceState {
    config: Config,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    source: String,
    target: String,
    #[serde(default)]
    preconditions: Vec<Precondition>,
}

#[derive(Debug, PartialEq, Eq)]
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

#[derive(Debug)]
pub struct PendingTransitionInfo {
    source: String,
    target: String,
    current_version: VersionHash,
}

fn run_transition(
    transition: &Transition,
    repo: &Repository,
    service_state: &ServiceState,
) -> Result<TransitionResult, Error> {
    let target_locks = locks::load_locks(repo, &transition.target)?;
    if target_locks.env_lock.is_locked() {
        return Ok(TransitionResult::Skipped);
    }

    let head_commit = git::get_head_commit(repo)?;

    let pending_transition = PendingTransitionInfo {
        source: transition.source.clone(),
        target: transition.target.clone(),
        current_version: head_commit.id().into(),
    };

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

    for precondition in &transition.preconditions {
        match precondition::check_precondition(&pending_transition, precondition, service_state)? {
            PreconditionResult::Blocked => {
                return Ok(TransitionResult::Blocked);
            }
            PreconditionResult::Failed => {
                return Ok(TransitionResult::CheckFailed);
            }
            PreconditionResult::Success => {}
        }
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

    info!(
        "Made commit {} mirroring {} to {}. Pushing...",
        commit, transition.source, transition.target
    );

    git::push(repo, &service_state.config.common.versions_url)?;

    info!("Pushed.");

    Ok(TransitionResult::Success)
}

fn run_one_transition(repo: &Repository, service_state: &ServiceState) -> Result<(), Error> {
    for transition in service_state.config.transitions.iter() {
        match run_transition(&transition, &repo, service_state)? {
            TransitionResult::Success => break,
            TransitionResult::Skipped => continue,
            // TODO we could instead just block transitions that touch the source env
            TransitionResult::Blocked => break,
            TransitionResult::CheckFailed => continue,
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use git_fixture::RepoFixture;

    fn make_config(s: &str, repo: &git2::Repository) -> Result<config::Config, Error> {
        let mut c: config::Config = serde_yaml::from_str(s)?;
        c.common.versions_url = repo.path().to_string_lossy().into_owned();
        c.common.versions_checkout_path = repo.path().to_string_lossy().into_owned();
        Ok(c)
    }

    #[test]
    fn test_transition_source_missing() {
        let fixture = RepoFixture::from_str(include_str!(
            "./fixtures/transition_source_missing.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config =
            make_config(include_str!("./fixtures/simple_config.yaml"), &fixture.repo).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        let result = run_transition(&state.config.transitions[0], &fixture.repo, &state).unwrap();

        assert_eq!(result, TransitionResult::Skipped);
        assert_eq!(
            fixture.repo.refname_to_id("refs/dm_head").unwrap(),
            fixture.get_commit("head").unwrap()
        );
    }

    #[test]
    fn test_transition_target_created() {
        let fixture = RepoFixture::from_str(include_str!(
            "./fixtures/transition_target_created.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config =
            make_config(include_str!("./fixtures/simple_config.yaml"), &fixture.repo).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(&fixture.repo, &state).unwrap();

        fixture.assert_ref_matches("refs/dm_head", "expected");
    }

    #[test]
    fn test_transition_target_changed() {
        let fixture = RepoFixture::from_str(include_str!(
            "./fixtures/transition_target_changed.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config =
            make_config(include_str!("./fixtures/simple_config.yaml"), &fixture.repo).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(&fixture.repo, &state).unwrap();

        fixture.assert_ref_matches("refs/dm_head", "expected");
    }

    #[test]
    fn test_transition_priority() {
        let fixture =
            RepoFixture::from_str(include_str!("./fixtures/transition_priority.yaml")).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config = make_config(
            include_str!("./fixtures/three_envs_config.yaml"),
            &fixture.repo,
        ).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(&fixture.repo, &state).unwrap();

        fixture.assert_ref_matches("refs/dm_head", "expected");
    }

    #[test]
    fn test_second_transition_runs() {
        let fixture =
            RepoFixture::from_str(include_str!("./fixtures/second_transition_runs.yaml")).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config = make_config(
            include_str!("./fixtures/three_envs_config.yaml"),
            &fixture.repo,
        ).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(&fixture.repo, &state).unwrap();

        fixture.assert_ref_matches("refs/dm_head", "expected");
    }

    #[test]
    fn test_prod_locked() {
        let fixture = RepoFixture::from_str(include_str!("./fixtures/prod_locked.yaml")).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config = make_config(
            include_str!("./fixtures/three_envs_config.yaml"),
            &fixture.repo,
        ).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(&fixture.repo, &state).unwrap();

        fixture.assert_ref_matches("refs/dm_head", "expected");
    }

    #[test]
    fn test_both_locked() {
        let fixture = RepoFixture::from_str(include_str!("./fixtures/both_locked.yaml")).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config = make_config(
            include_str!("./fixtures/three_envs_config.yaml"),
            &fixture.repo,
        ).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(&fixture.repo, &state).unwrap();

        assert_eq!(
            fixture.repo.refname_to_id("refs/dm_head").unwrap(),
            fixture.get_commit("head").unwrap()
        );
    }
}

fn run() -> Result<(), Error> {
    env_logger::init();
    let options = Options::from_args();
    let config = config::Config::load(&options.config)?;
    let repo = git::init_or_open(&config.common.versions_checkout_path)?;

    let client = reqwest::Client::new();
    let service_state = Arc::new(ServiceState { config, client });

    api::start(service_state.clone());

    info!("Transitioner running.");

    loop {
        git::update(&repo, &service_state.config.common.versions_url)?;

        if let Err(error) = run_one_transition(&repo, &service_state) {
            error!("Transition failed: {}\n{}", error, error.backtrace());
            for cause in error.causes() {
                error!("caused by: {}", cause);
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
