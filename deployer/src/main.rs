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
use std::collections::HashMap;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam::sync::ArcCell;
use failure::Error;
use structopt::StructOpt;

use common::deployment::{AllDeployerStatus, DeployerStatus, RolloutStatus};
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

    let mut last_version = HashMap::new();

    let service_state = Arc::new(ServiceState {
        latest_status: ArcCell::new(Arc::new(AllDeployerStatus::empty())),
        config,
    });

    api::start(service_state.clone());

    loop {
        git::update(&repo, &service_state.config.common.versions_url)?;

        for (env, deployer) in deployers.iter_mut() {
            let version = git::get_head_commit(&repo)?.id().into();

            let mut latest_status = service_state.latest_status.get();

            let mut rollout_status = latest_status
                .deployers
                .get(&*env)
                .map(|d| d.rollout_status.clone());

            let mut status_by_deployment = latest_status
                .deployers
                .get(&*env)
                .map(|d| d.status_by_deployment.clone())
                .unwrap_or_else(HashMap::new);

            let mut last_successfully_deployed_version = latest_status
                .deployers
                .get(&*env)
                .and_then(|d| d.last_successfully_deployed_version);

            if let Some(deployments) = deployment::get_deployments(&repo, env, last_version.get(env).cloned())? {
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
                    deployment::get_deployments(&repo, env, last_successfully_deployed_version)?
                {
                    let (new_rollout_status, new_status_by_deployment) =
                        deployer.check_rollout_status(&deployments.deployments)?;
                    rollout_status = Some(new_rollout_status);
                    status_by_deployment.extend(new_status_by_deployment.into_iter());

                    if rollout_status == Some(RolloutStatus::Clean) {
                        last_successfully_deployed_version = Some(version);
                    }
                }
            }

            // TODO actually set last_successfully_deployed_version

            if let Some(rollout_status) = rollout_status {
                let new_status = DeployerStatus {
                    deployed_version: version,
                    last_successfully_deployed_version,
                    rollout_status,
                    status_by_deployment,
                };
                Arc::make_mut(&mut latest_status)
                    .deployers
                    .insert(env.to_string(), new_status);
                service_state.latest_status.set(latest_status);
            }
            last_version.insert(env.clone(), version);
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
