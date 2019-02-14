use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam::atomic::ArcCell;
use failure::Error;
use log::error;
use serde_derive::Deserialize;
use structopt::StructOpt;

use common::deployment::AllDeployerStatus;
use common::repo::{self, ResourceRepo};

mod api;
mod config;
mod deployment;

use crate::config::Config;

#[derive(Debug, StructOpt)]
struct Options {
    /// The location of the configuration file.
    #[structopt(short = "c", long = "config", parse(from_os_str))]
    config: PathBuf,
    #[structopt(subcommand)]
    command: Command,
}

#[derive(Debug, Deserialize, Clone)]
struct Env {
    #[serde(flatten)]
    common: common::Env,
    api_port: Option<u16>,
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
    env: Env,
}

fn serve(config: Config, env: Env) -> Result<(), Error> {
    let mut repo = repo::GitResourceRepo::open(env.common.clone())?;

    let mut deployers = config
        .deployers
        .iter()
        .map(|(env, deployer_config)| deployer_config.create().map(|d| (env.to_owned(), d)))
        .collect::<Result<BTreeMap<_, _>, Error>>()?;

    let service_state = Arc::new(ServiceState {
        latest_status: ArcCell::new(Arc::new(AllDeployerStatus::empty())),
        env,
    });

    api::start(service_state.clone());

    let mut last_version = HashMap::new();

    loop {
        repo.update()?;

        let version = repo.version();

        for (env, deployer) in &mut deployers {
            let mut latest_status = service_state.latest_status.get();

            let env_status = latest_status.deployers.get(&*env).cloned();

            let env_status = match deployment::deploy_env(
                deployer,
                &repo,
                env,
                last_version.get(env).cloned(),
                env_status,
            ) {
                Ok(s) => s,
                Err(e) => {
                    error!("Deployment failed: {}\n{}", e, e.backtrace());
                    for cause in e.iter_causes() {
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

fn deploy(config: Config, env: Env) -> Result<(), Error> {
    let repo = repo::GitResourceRepo::open(env.common.clone())?;

    let mut deployers = config
        .deployers
        .iter()
        .map(|(env, deployer_config)| deployer_config.create().map(|d| (env.to_owned(), d)))
        .collect::<Result<BTreeMap<_, _>, Error>>()?;

    for (env, deployer) in &mut deployers {
        let env_status = deployment::deploy_env(deployer, &repo, env, None, None)?;

        println!("Status of {}: {:?}", env, env_status);
    }

    Ok(())
}

fn check(config: Config, env: Env) -> Result<(), Error> {
    let repo = repo::GitResourceRepo::open(env.common.clone())?;

    let mut deployers = config
        .deployers
        .iter()
        .map(|(env, deployer_config)| deployer_config.create().map(|d| (env.to_owned(), d)))
        .collect::<Result<BTreeMap<_, _>, Error>>()?;

    for (env, deployer) in &mut deployers {
        let version = repo.version();

        let mut env_status = deployment::new_deployer_status(version);

        if let Some(resources) = deployment::get_resources(&repo, env, None)? {
            let (new_rollout_status, new_status_by_resource) =
                deployment::check_rollout_status(deployer, &resources.resources)?;

            env_status.rollout_status = new_rollout_status;
            env_status
                .status_by_resource
                .extend(new_status_by_resource.into_iter());
        }

        println!("Status of {}: {:?}", env, env_status);
    }

    Ok(())
}

fn run() -> Result<(), Error> {
    env_logger::init();
    let options = Options::from_args();
    let config = Config::load(&options.config)?;
    let env = envy::from_env()?;

    match options.command {
        Command::Serve => serve(config, env),
        Command::Check => check(config, env),
        Command::Deploy => deploy(config, env),
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
