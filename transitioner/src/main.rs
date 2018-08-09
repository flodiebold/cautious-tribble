#[macro_use]
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
extern crate chrono;
extern crate cron;
extern crate crossbeam;
extern crate env_logger;
extern crate indexmap;
extern crate reqwest;
extern crate warp;

extern crate common;
#[cfg(test)]
extern crate git_fixture;

use std::path::PathBuf;
use std::process;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use cron::Schedule;
use failure::Error;
use git2::{ObjectType, Repository, Signature};
use structopt::StructOpt;

use common::git::{self, TreeZipper};
use common::repo::{oid_to_id, Id};

mod api;
mod config;
mod locks;
mod precondition;
mod transition_state;

use config::Config;
use precondition::{Precondition, PreconditionResult};
use transition_state::{TransitionState, TransitionStates};

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

#[derive(Debug, Clone, Deserialize)]
pub struct Transition {
    source: String,
    target: String,
    #[serde(default)]
    preconditions: Vec<Precondition>,
    schedule: Option<String>,
}

impl Transition {
    fn next_scheduled_time(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let schedule_str = self.schedule.as_ref()?;
        let schedule = match Schedule::from_str(&schedule_str) {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "Could not parse schedule definition {:?}: {}",
                    schedule_str, e
                );
                return None;
            }
        };
        schedule.after(&now).next()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TransitionResult {
    /// The transition was performed successfully.
    Success,
    /// The transition was not applicable for some reason, or there was no change.
    Skipped(SkipReason),
    /// The transition might be applicable soon, and the source env should not
    /// be changed until then.
    Blocked,
    /// A precondition check was negative.
    CheckFailed,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
enum SkipReason {
    Scheduled { time: DateTime<Utc> },
    TargetLocked,
    SourceMissing,
    NoChange,
}

#[derive(Debug)]
pub struct PendingTransitionInfo {
    source: String,
    target: String,
    current_version: Id,
}

fn run_transition(
    name: &str,
    transition: &Transition,
    repo: &Repository,
    service_state: &ServiceState,
    now: DateTime<Utc>,
) -> Result<TransitionResult, Error> {
    let mut transition_states = TransitionStates::load(repo)?;
    let transition_state = transition_states.0.get(name).cloned().unwrap_or_default();
    if let Some(time) = transition_state.scheduled {
        if time >= now {
            // before scheduled time
            return Ok(TransitionResult::Skipped(SkipReason::Scheduled { time }));
        }
    }

    let target_locks = locks::load_locks(repo, &transition.target)?;
    if target_locks.env_lock.is_locked() {
        return Ok(TransitionResult::Skipped(SkipReason::TargetLocked));
    }

    let head_commit = git::get_head_commit(repo)?;

    let pending_transition = PendingTransitionInfo {
        source: transition.source.clone(),
        target: transition.target.clone(),
        current_version: oid_to_id(head_commit.id()),
    };

    let new_state = TransitionState {
        scheduled: transition.next_scheduled_time(now),
    };

    transition_states.insert(name, new_state);

    let tree = head_commit.tree()?;
    let mut source = TreeZipper::from(repo, tree.clone());
    source.descend(&transition.source)?;
    source.descend("version")?;
    if !source.exists() {
        return Ok(TransitionResult::Skipped(SkipReason::SourceMissing));
    };

    let mut target = TreeZipper::from(repo, tree.clone());
    target.descend(&transition.target)?;
    target.descend("version")?;

    let mut last_path = PathBuf::new();
    for (path, entry) in source.walk(true) {
        let entry = entry?;
        if entry.kind() == Some(ObjectType::Blob) {
            target.rebuild(|b| {
                b.insert(entry.name_bytes(), entry.id(), entry.filemode())?;
                Ok(())
            })?;
        } else if entry.kind() == Some(ObjectType::Tree) {
            while !path.starts_with(&last_path) {
                last_path.pop();
                target.ascend()?;
            }
            for component in path
                .strip_prefix(&last_path)
                .expect("should be a prefix")
                .components()
            {
                match component {
                    ::std::path::Component::Normal(part) => {
                        if let Some(part) = part.to_str() {
                            target.descend(part)?;
                        } else {
                            bail!("Non-utf8 path in env {}: {:?}", name, path);
                        }
                    }
                    _ => {
                        bail!("unexpected path component in file name: {:?}", component);
                    }
                }
            }
            last_path.clone_from(&path);
        }
    }

    for _ in last_path.components() {
        target.ascend()?;
    }

    target.ascend()?;
    target.ascend()?;

    target.rebuild(|b| transition_states.save(repo, b))?;

    let new_tree = target.into_inner().expect("new tree should not be None");

    if new_tree.id() == tree.id() {
        // nothing changed
        return Ok(TransitionResult::Skipped(SkipReason::NoChange));
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

fn run_one_transition(
    repo: &Repository,
    service_state: &ServiceState,
    now: DateTime<Utc>,
) -> Result<(), Error> {
    for (name, transition) in service_state.config.transitions.iter() {
        match run_transition(name, transition, &repo, service_state, now)? {
            TransitionResult::Success => break,
            TransitionResult::Skipped(..) => continue,
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

    fn test_time() -> DateTime<Utc> {
        "2018-01-01T00:00:00.000Z".parse().unwrap()
    }

    #[test]
    fn test_transition_source_missing() {
        let fixture =
            RepoFixture::from_str(include_str!("./fixtures/transition_source_missing.yaml"))
                .unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config =
            make_config(include_str!("./fixtures/simple_config.yaml"), &fixture.repo).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        let result = run_transition(
            "prod",
            &state.config.transitions.get("prod").unwrap(),
            &fixture.repo,
            &state,
            test_time(),
        ).unwrap();

        assert_eq!(result, TransitionResult::Skipped(SkipReason::SourceMissing));
        assert_eq!(
            fixture.repo.refname_to_id("refs/dm_head").unwrap(),
            fixture.get_commit("head").unwrap()
        );
    }

    #[test]
    fn test_transition_target_created() {
        let fixture =
            RepoFixture::from_str(include_str!("./fixtures/transition_target_created.yaml"))
                .unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config =
            make_config(include_str!("./fixtures/simple_config.yaml"), &fixture.repo).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(&fixture.repo, &state, test_time()).unwrap();

        fixture.assert_ref_matches("refs/dm_head", "expected");
    }

    #[test]
    fn test_transition_target_changed() {
        let fixture =
            RepoFixture::from_str(include_str!("./fixtures/transition_target_changed.yaml"))
                .unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config =
            make_config(include_str!("./fixtures/simple_config.yaml"), &fixture.repo).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(&fixture.repo, &state, test_time()).unwrap();

        fixture.assert_ref_matches("refs/dm_head", "expected");
    }

    #[test]
    fn test_transition_subdirs() {
        let fixture =
            RepoFixture::from_str(include_str!("./fixtures/transition_subdirs.yaml")).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config =
            make_config(include_str!("./fixtures/simple_config.yaml"), &fixture.repo).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(&fixture.repo, &state, test_time()).unwrap();

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

        run_one_transition(&fixture.repo, &state, test_time()).unwrap();

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

        run_one_transition(&fixture.repo, &state, test_time()).unwrap();

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

        run_one_transition(&fixture.repo, &state, test_time()).unwrap();

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

        run_one_transition(&fixture.repo, &state, test_time()).unwrap();

        assert_eq!(
            fixture.repo.refname_to_id("refs/dm_head").unwrap(),
            fixture.get_commit("head").unwrap()
        );
    }

    #[test]
    fn test_timed_transition_pending() {
        let fixture =
            RepoFixture::from_str(include_str!("./fixtures/timed_transition_pending.yaml"))
                .unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config = make_config(
            include_str!("./fixtures/timed_transition_config.yaml"),
            &fixture.repo,
        ).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        let result = run_transition(
            "prod",
            &state.config.transitions.get("prod").unwrap(),
            &fixture.repo,
            &state,
            "2018-01-01T00:00:00Z".parse().unwrap(),
        ).unwrap();

        assert_eq!(
            result,
            TransitionResult::Skipped(SkipReason::Scheduled {
                time: "2018-01-01T00:00:00Z".parse().unwrap()
            })
        );
        assert_eq!(
            fixture.repo.refname_to_id("refs/dm_head").unwrap(),
            fixture.get_commit("head").unwrap()
        );
    }

    #[test]
    fn test_timed_transition_runs() {
        let fixture =
            RepoFixture::from_str(include_str!("./fixtures/timed_transition_pending.yaml"))
                .unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config = make_config(
            include_str!("./fixtures/timed_transition_config.yaml"),
            &fixture.repo,
        ).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(
            &fixture.repo,
            &state,
            "2018-01-01T00:00:01Z".parse().unwrap(),
        ).unwrap();

        fixture.assert_ref_matches("refs/dm_head", "expected");
    }

    #[test]
    fn test_timed_transition_without_schedule() {
        let fixture = RepoFixture::from_str(include_str!(
            "./fixtures/timed_transition_pending_no_schedule.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let config = make_config(
            include_str!("./fixtures/timed_transition_config_no_schedule.yaml"),
            &fixture.repo,
        ).unwrap();
        let client = reqwest::Client::new();
        let state = ServiceState { config, client };

        run_one_transition(
            &fixture.repo,
            &state,
            "2018-01-01T00:00:01Z".parse().unwrap(),
        ).unwrap();

        fixture.assert_ref_matches("refs/dm_head", "expected");
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

        if let Err(error) = run_one_transition(&repo, &service_state, Utc::now()) {
            error!("Transition failed: {}\n{}", error, error.backtrace());
            for cause in error.iter_causes() {
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
            for cause in e.iter_causes() {
                eprintln!("caused by: {}", cause);
            }
            process::exit(1);
        }
    }
}
