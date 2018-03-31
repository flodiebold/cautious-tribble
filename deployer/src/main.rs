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
#[macro_use]
extern crate structopt;

extern crate common;
#[cfg(test)]
extern crate git_fixture;

use std::time::Duration;
use std::thread;
use std::path::PathBuf;
use std::process;
use std::collections::BTreeMap;

use failure::Error;
use structopt::StructOpt;

use common::git;

mod deployment;
mod config;

use config::Config;

#[derive(Debug, StructOpt)]
struct Options {
    /// The location of the configuration file.
    #[structopt(short = "c", long = "config", parse(from_os_str))]
    config: PathBuf,
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

    loop {
        git::update(&repo, &config.common.versions_url)?;

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

                last_version = Some(git::get_head_commit(&repo)?.id());
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
