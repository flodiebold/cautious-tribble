use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam::atomic::ArcCell;
use failure::Error;
use log::error;
use serde_derive::Deserialize;

use common::deployment::AllDeployerStatus;
use common::repo::{self, ResourceRepo};

mod api;
mod config;
mod deployment;

use crate::config::Config;

#[derive(Debug, Deserialize, Clone)]
struct Env {
    #[serde(flatten)]
    common: common::Env,
    api_port: Option<u16>,
}

pub struct ServiceState {
    latest_status: ArcCell<AllDeployerStatus>,
    env: Env,
}

fn serve(env: Env) -> Result<(), Error> {
    let mut repo = repo::GitResourceRepo::open(env.common.clone())?;

    let config = repo
        .get(Path::new("deployers.yaml"))?
        .map_or(Ok(Config::default()), |data| Config::load(&data))?;

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

fn run() -> Result<(), Error> {
    env_logger::init();
    let env = envy::from_env()?;

    serve(env)
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
