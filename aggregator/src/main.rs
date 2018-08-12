extern crate env_logger;
#[macro_use]
extern crate failure;
extern crate git2;
#[macro_use]
extern crate log;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate serde_yaml;
#[macro_use]
extern crate structopt;
extern crate crossbeam;
extern crate reqwest;
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

mod api;
mod config;

use config::Config;

#[derive(Debug, StructOpt)]
struct Options {
    /// The location of the configuration file.
    #[structopt(short = "c", long = "config", parse(from_os_str))]
    config: PathBuf,
}

pub struct ServiceState {
    // latest_status: ArcCell<AllDeployerStatus>,
    config: config::Config,
}

fn serve(config: Config) -> Result<(), Error> {
    let service_state = Arc::new(ServiceState { config });

    let handle = api::start(service_state.clone());

    handle.join().unwrap();

    Ok(())
}

fn run() -> Result<(), Error> {
    env_logger::init();
    let options = Options::from_args();
    let config = Config::load(&options.config)?;

    serve(config)
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
