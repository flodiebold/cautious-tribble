use std::sync::Arc;
use std::thread;
use std::time::Duration;

use failure::{bail, Error};
use log::{error, trace};
use reqwest;

use common::aggregator::Message;
use common::deployment::AllDeployerStatus;

use super::ServiceState;
use crate::Env;

fn get_current_deployer_status(env: &Env) -> Result<AllDeployerStatus, Error> {
    if let Some(deployer_url) = env.deployer_url.as_ref() {
        Ok(reqwest::get(&format!("{}/status", deployer_url))?
            .error_for_status()?
            .json()?)
    } else {
        bail!("no deployer url configured");
    }
}

pub fn start(service_state: Arc<ServiceState>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut last_status = Default::default();
        loop {
            trace!("Retrieve deployer status...");
            let status = match get_current_deployer_status(&service_state.env) {
                Ok(status) => status,
                Err(e) => {
                    error!(
                        "Retrieving deployer status failed: {}\n{}",
                        e,
                        e.backtrace()
                    );
                    for cause in e.iter_causes() {
                        error!("caused by: {}", cause);
                    }

                    thread::sleep(Duration::from_secs(1));

                    continue;
                }
            };

            if last_status != status {
                trace!("Deployer status changed: {:?}", status);

                let counter = service_state.update_status(|full_status| {
                    full_status.deployers = status.clone();
                });

                // TODO send just a diff here:
                service_state.send_to_all_clients(Message::DeployerStatus {
                    counter,
                    content: status.clone(),
                });

                last_status = status;
            } else {
                trace!("Deployer status unchanged");
            }

            thread::sleep(Duration::from_secs(1));
        }
    })
}
