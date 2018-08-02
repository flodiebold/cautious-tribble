extern crate env_logger;
#[macro_use]
extern crate failure;
extern crate git2;
extern crate k8s_openapi;
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
extern crate warp;

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

use crossbeam::atomic::ArcCell;
use failure::Error;
use structopt::StructOpt;

use common::deployment::{AllDeployerStatus, DeployerStatus, RolloutStatus};
use common::git::{self, VersionHash};

mod api;
mod config;
mod deployment;

use config::Config;

#[derive(Debug, StructOpt)]
struct Options {
    /// The location of the configuration file.
    #[structopt(short = "c", long = "config", parse(from_os_str))]
    config: PathBuf,
    #[structopt(subcommand)]
    command: Command,
}

#[derive(Debug, StructOpt)]
enum Command {
    #[structopt(name = "serve")]
    /// Run the deployer as a server, constantly checking for updates.
    Serve,
    #[structopt(name = "check")]
    /// Compare the current state of the cluster to the intended state.
    Check,
    #[structopt(name = "deploy")]
    /// Deploy the intended state to the cluster.
    Deploy,
}

pub struct ServiceState {
    latest_status: ArcCell<AllDeployerStatus>,
    config: config::Config,
}

fn new_deployer_status(version: VersionHash) -> DeployerStatus {
    DeployerStatus {
        deployed_version: version,
        last_successfully_deployed_version: None,
        rollout_status: RolloutStatus::InProgress,
        status_by_deployment: HashMap::new(),
    }
}

fn deploy_env(
    version: VersionHash,
    deployer: &mut deployment::kubernetes::KubernetesDeployer,
    repo: &git2::Repository,
    env: &str,
    last_version: Option<VersionHash>,
    last_status: Option<DeployerStatus>,
) -> Result<DeployerStatus, Error> {
    let mut env_status = last_status.unwrap_or_else(|| new_deployer_status(version));
    if let Some(deployments) = deployment::get_deployments(repo, env, last_version)? {
        info!(
            "Got a change for {} to version {:?}, now deploying...",
            env, version
        );
        deployer.deploy(&deployments.deployments)?;

        env_status.deployed_version = version;
        env_status.rollout_status = RolloutStatus::InProgress;

        info!("Deployed {} up to {:?}", env, version);
    }

    if env_status.rollout_status == RolloutStatus::InProgress {
        if let Some(deployments) =
            deployment::get_deployments(&repo, env, env_status.last_successfully_deployed_version)?
        {
            let (new_rollout_status, new_status_by_deployment) =
                deployer.check_rollout_status(&deployments.deployments)?;
            env_status.rollout_status = new_rollout_status;
            env_status
                .status_by_deployment
                .extend(new_status_by_deployment.into_iter());
        }
    }

    if env_status.rollout_status == RolloutStatus::Clean {
        env_status.last_successfully_deployed_version = Some(version);
    }

    Ok(env_status)
}

fn serve(config: Config) -> Result<(), Error> {
    let repo = git::init_or_open(&config.common.versions_checkout_path)?;

    let mut deployers = config
        .deployers
        .iter()
        .map(|(env, deployer_config)| deployer_config.create().map(|d| (env.to_owned(), d)))
        .collect::<Result<BTreeMap<_, _>, Error>>()?;

    let service_state = Arc::new(ServiceState {
        latest_status: ArcCell::new(Arc::new(AllDeployerStatus::empty())),
        config,
    });

    api::start(service_state.clone());

    let mut last_version = HashMap::new();

    loop {
        git::update(&repo, &service_state.config.common.versions_url)?;

        for (env, deployer) in deployers.iter_mut() {
            let version = git::get_head_commit(&repo)?.id().into();

            let mut latest_status = service_state.latest_status.get();

            let env_status = latest_status.deployers.get(&*env).cloned();

            let env_status = match deploy_env(
                version,
                deployer,
                &repo,
                env,
                last_version.get(env).cloned(),
                env_status,
            ) {
                Ok(s) => s,
                Err(e) => {
                    error!("Deployment failed: {}\n{}", e, e.backtrace());
                    for cause in e.causes() {
                        error!("caused by: {}", cause);
                    }

                    continue;
                }
            };

            Arc::make_mut(&mut latest_status)
                .deployers
                .insert(env.to_string(), env_status);
            service_state.latest_status.set(latest_status);
            last_version.insert(env.clone(), version);
        }

        thread::sleep(Duration::from_millis(1000));
    }
}

fn deploy(config: Config) -> Result<(), Error> {
    let repo = git::init_or_open(&config.common.versions_checkout_path)?;

    let mut deployers = config
        .deployers
        .iter()
        .map(|(env, deployer_config)| deployer_config.create().map(|d| (env.to_owned(), d)))
        .collect::<Result<BTreeMap<_, _>, Error>>()?;

    git::update(&repo, &config.common.versions_url)?;

    for (env, deployer) in deployers.iter_mut() {
        let version = git::get_head_commit(&repo)?.id().into();

        let env_status = deploy_env(version, deployer, &repo, env, None, None)?;

        println!("Status of {}: {:?}", env, env_status);
    }

    Ok(())
}

fn check(config: Config) -> Result<(), Error> {
    let repo = git::init_or_open(&config.common.versions_checkout_path)?;

    let mut deployers = config
        .deployers
        .iter()
        .map(|(env, deployer_config)| deployer_config.create().map(|d| (env.to_owned(), d)))
        .collect::<Result<BTreeMap<_, _>, Error>>()?;

    git::update(&repo, &config.common.versions_url)?;

    for (env, deployer) in deployers.iter_mut() {
        let version = git::get_head_commit(&repo)?.id().into();

        let mut env_status = new_deployer_status(version);

        if let Some(deployments) = deployment::get_deployments(&repo, env, None)? {
            let (new_rollout_status, new_status_by_deployment) =
                deployer.check_rollout_status(&deployments.deployments)?;

            env_status.rollout_status = new_rollout_status;
            env_status
                .status_by_deployment
                .extend(new_status_by_deployment.into_iter());
        }

        println!("Status of {}: {:?}", env, env_status);
    }

    Ok(())
}

fn run() -> Result<(), Error> {
    env_logger::init();
    let options = Options::from_args();
    let config = Config::load(&options.config)?;

    match options.command {
        Command::Serve => serve(config),
        Command::Check => check(config),
        Command::Deploy => deploy(config),
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
