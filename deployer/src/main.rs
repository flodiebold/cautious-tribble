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
extern crate serde_yaml;
extern crate serde_json;
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

#[derive(Debug, Clone, Serialize)]
pub enum RolloutStatus {
    InProgress,
    Clean,
    Failed { message: String },
}

fn serialize_hash<S: serde::Serializer>(
    hash: &deployment::VersionHash,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{}", hash))
}

#[derive(Debug, Clone, Serialize)]
pub struct DeployerStatus {
    #[serde(serialize_with = "serialize_hash")]
    deployed_version: deployment::VersionHash,
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
            if let Some(deployments) = deployment::get_deployments(&repo, env, last_version)? {
                let result = deployer.deploy(&deployments.deployments);

                if let Err(e) = result {
                    error!("Deployment failed: {}\n{}", e, e.backtrace());
                    for cause in e.causes() {
                        error!("caused by: {}", cause);
                    }
                    continue;
                }

                let version = git::get_head_commit(&repo)?.id();

                let new_status = DeployerStatus {
                    deployed_version: version,
                    rollout_status: RolloutStatus::Clean, // TODO
                };
                let mut latest_status = service_state.latest_status.get();
                Arc::make_mut(&mut latest_status)
                    .deployers
                    .insert(env.to_string(), new_status);
                service_state.latest_status.set(latest_status);

                last_version = Some(version); // TODO doesn't this need to be per env?
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
