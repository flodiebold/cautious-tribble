use std::sync::Arc;
use std::thread;
use std::time::Duration;

use failure::Error;
use reqwest;

use common::aggregator::Message;
use common::deployment::AllDeployerStatus;

use super::ServiceState;
use crate::config::Config;

fn get_current_deployer_status(config: &Config) -> Result<AllDeployerStatus, Error> {
    if let Some(deployer_url) = config.deployer_url.as_ref() {
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
            let status = match get_current_deployer_status(&service_state.config) {
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

                    continue;
                }
            };

            if last_status != status {
                trace!("Deployer status changed: {:?}", status);

                let counter = {
                    let mut write_lock = service_state.full_status.write().unwrap();
                    let full_status = Arc::make_mut(&mut write_lock);
                    full_status.counter += 1;
                    full_status.deployers = status.clone();
                    full_status.counter
                };

                // TODO send just a diff here:
                service_state
                    .bus
                    .lock()
                    .unwrap()
                    .broadcast(Arc::new(Message::DeployerStatus {
                        counter,
                        content: status.clone(),
                    }));

                last_status = status;
            } else {
                trace!("Deployer status unchanged");
            }

            thread::sleep(Duration::from_secs(1));
        }
    })
}
