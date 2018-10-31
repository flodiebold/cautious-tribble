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
extern crate bus;
extern crate crossbeam;
extern crate futures;
extern crate reqwest;
extern crate serde_yaml;
extern crate structopt;
extern crate warp;

extern crate common;
#[cfg(test)]
extern crate git_fixture;

use std::path::PathBuf;
use std::process;
use std::sync::{Arc, Mutex, RwLock};

use bus::Bus;
use failure::Error;
use structopt::StructOpt;

use common::aggregator::{FullStatus, Message};

mod api;
mod config;
mod deployer_watch;
mod transitioner_watch;
mod versions_watch;

use crate::config::Config;

#[derive(Debug, StructOpt)]
struct Options {
    /// The location of the configuration file.
    #[structopt(short = "c", long = "config", parse(from_os_str))]
    config: PathBuf,
}

pub struct ServiceState {
    config: config::Config,
    full_status: RwLock<Arc<FullStatus>>,
    bus: Mutex<Bus<Arc<Message>>>,
}

fn serve(config: Config) -> Result<(), Error> {
    let bus = Mutex::new(Bus::new(100));
    let full_status = Default::default();
    let service_state = Arc::new(ServiceState {
        config,
        full_status,
        bus,
    });

    let versions_watch = versions_watch::start(service_state.clone())?;
    let api = api::start(service_state.clone());
    let deployer_watch = deployer_watch::start(service_state.clone());
    let transitioner_watch = transitioner_watch::start(service_state.clone());

    info!("Aggregator running.");

    api.join().unwrap();
    deployer_watch.join().unwrap();
    transitioner_watch.join().unwrap();
    versions_watch.join().unwrap();

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
