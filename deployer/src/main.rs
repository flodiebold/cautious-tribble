extern crate env_logger;
#[macro_use]
extern crate failure;
extern crate git2;
extern crate kubeclient;
#[macro_use]
extern crate log;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde_yaml;
#[macro_use]
extern crate structopt;
extern crate crossbeam;
extern crate gotham;
extern crate hyper;
extern crate mime;

extern crate common;
#[cfg(test)]
extern crate git_fixture;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam::sync::ArcCell;
use failure::Error;
use structopt::StructOpt;

use common::git;

mod api;
mod config;
mod deployment;

use config::Config;

#[derive(Debug, StructOpt)]
struct Options {
    /// The location of the configuration file.
    #[structopt(short = "c", long = "config", parse(from_os_str))]
    config: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum RolloutStatus {
    InProgress,
    Clean,
    Failed { message: String },
}

impl RolloutStatus {
    pub fn combine(self, other: RolloutStatus) -> RolloutStatus {
        use RolloutStatus::*;
        match self {
            Clean => other,
            InProgress => match other {
                Clean | InProgress => InProgress,
                Failed { message } => Failed { message },
            },
            Failed { mut message } => match other {
                Clean | InProgress => Failed { message },
                Failed { message: other_message } => {
                    message.push_str("\n");
                    message.push_str(&other_message);
                    Failed { message }
                }
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct HashWrapper(deployment::VersionHash);

impl serde::Serialize for HashWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}", self.0))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeployerStatus {
    deployed_version: HashWrapper,
    last_successfully_deployed_version: Option<HashWrapper>,
    rollout_status: RolloutStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct AllDeployerStatus {
    deployers: BTreeMap<String, DeployerStatus>,
}

impl AllDeployerStatus {
    pub fn empty() -> AllDeployerStatus {
        AllDeployerStatus {
            deployers: BTreeMap::new(),
        }
    }
}

pub struct ServiceState {
    latest_status: ArcCell<AllDeployerStatus>,
    config: config::Config,
}

fn run() -> Result<(), Error> {
    env_logger::init();
    let options = Options::from_args();
    let config = Config::load(&options.config)?;
    let repo = git::init_or_open(&config.common.versions_checkout_path)?;

    let mut deployers = config
        .deployers
        .iter()
        .map(|(env, deployer_config)| deployer_config.create().map(|d| (env.to_owned(), d)))
        .collect::<Result<BTreeMap<_, _>, Error>>()?;

    let mut last_version = None;

    let service_state = Arc::new(ServiceState {
        latest_status: ArcCell::new(Arc::new(AllDeployerStatus::empty())),
        config,
    });

    api::start(service_state.clone());

    loop {
        git::update(&repo, &service_state.config.common.versions_url)?;

        for (env, deployer) in deployers.iter_mut() {
            let version = git::get_head_commit(&repo)?.id();

            let mut latest_status = service_state.latest_status.get();

            let mut rollout_status = latest_status
                .deployers
                .get(&*env)
                .map(|d| d.rollout_status.clone());

            let mut last_successfully_deployed_version = latest_status
                .deployers
                .get(&*env)
                .and_then(|d| d.last_successfully_deployed_version);

            if let Some(deployments) = deployment::get_deployments(&repo, env, last_version)? {
                info!(
                    "Got a change for {} to version {:?}, now deploying...",
                    env, version
                );
                let result = deployer.deploy(&deployments.deployments);

                rollout_status = Some(RolloutStatus::InProgress);

                if let Err(e) = result {
                    error!("Deployment failed: {}\n{}", e, e.backtrace());
                    for cause in e.causes() {
                        error!("caused by: {}", cause);
                    }

                    continue;
                }

                info!("Deployed {} up to {:?}", env, version);
            }

            if rollout_status == Some(RolloutStatus::InProgress) {
                if let Some(deployments) =
                    deployment::get_deployments(&repo, env, last_successfully_deployed_version.map(|v| v.0))?
                {
                    rollout_status = Some(deployer.check_rollout_status(&deployments.deployments)?);

                    if rollout_status == Some(RolloutStatus::Clean) {
                        last_successfully_deployed_version = Some(HashWrapper(version));
                    }
                }
            }

            // TODO actually set last_successfully_deployed_version

            if let Some(rollout_status) = rollout_status {
                let new_status = DeployerStatus {
                    deployed_version: HashWrapper(version),
                    last_successfully_deployed_version,
                    rollout_status,
                };
                Arc::make_mut(&mut latest_status)
                    .deployers
                    .insert(env.to_string(), new_status);
                service_state.latest_status.set(latest_status);
            }
            last_version = Some(version); // TODO doesn't this need to be per env?
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
