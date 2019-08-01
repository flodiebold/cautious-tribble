use std::path::PathBuf;
use std::process;
use std::sync::{atomic::AtomicU32, Arc, RwLock};

use failure::Error;
use log::info;
use serde_derive::Deserialize;

use common::aggregator::{FullStatus, Message};

mod api;
mod deployer_watch;
mod transitioner_watch;
mod versions_watch;

#[derive(Debug, Deserialize)]
struct Env {
    #[serde(flatten)]
    common: common::Env,
    api_port: Option<u16>,
    ui_path: Option<PathBuf>,
    deployer_url: Option<String>,
    transitioner_url: Option<String>,
}

pub struct ServiceState {
    env: Env,
    full_status: RwLock<Arc<FullStatus>>,
    client_counter: AtomicU32,
    receivers: RwLock<Vec<(u32, futures::sync::mpsc::Sender<Message>)>>,
}

fn serve(env: Env) -> Result<(), Error> {
    let receivers = RwLock::new(Vec::with_capacity(100));
    let full_status = Default::default();
    let service_state = Arc::new(ServiceState {
        env,
        full_status,
        client_counter: AtomicU32::new(0),
        receivers,
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
