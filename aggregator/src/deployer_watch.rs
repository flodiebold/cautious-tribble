use std::sync::Arc;
use std::thread;
use std::time::Duration;

use failure::Error;
use reqwest;

use common::aggregator::Message;
use common::deployment::AllDeployerStatus;

use super::ServiceState;
use config::Config;

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
        let mut last_status = None;
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

            if last_status.as_ref() != Some(&status) {
                trace!("Deployer status changed: {:?}", status);

                service_state
                    .bus
                    .lock()
                    .unwrap()
                    .broadcast(Arc::new(Message::DeployerStatus(status.clone())));

                last_status = Some(status);
            } else {
                trace!("Deployer status unchanged");
            }

            thread::sleep(Duration::from_secs(1));
        }
    })
}
