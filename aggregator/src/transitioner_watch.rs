use std::sync::Arc;
use std::thread;
use std::time::Duration;

use failure::{bail, Error};
use log::{error, trace};
use reqwest;

use common::aggregator::Message;
use common::chrono::{self, Utc};
use common::transitions::AllTransitionStatus;

use super::ServiceState;
use crate::Env;

fn get_current_transitioner_status(env: &Env) -> Result<AllTransitionStatus, Error> {
    if let Some(transitioner_url) = env.transitioner_url.as_ref() {
        Ok(reqwest::get(&format!("{}/status", transitioner_url))?
            .error_for_status()?
            .json()?)
    } else {
        bail!("no transitioner url configured");
    }
}

pub fn start(service_state: Arc<ServiceState>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut last_status = Default::default();
        loop {
            trace!("Retrieve transitioner status...");
            let mut status = match get_current_transitioner_status(&service_state.env) {
                Ok(status) => status,
                Err(e) => {
                    error!(
                        "Retrieving transitioner status failed: {}\n{}",
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

            // If the last run for a transition happened within the last 10
            // seconds, we remove the time. This way, we don't send an update
            // every second.
            for (_, transition_status) in &mut status {
                if let Some(last_run) = &mut transition_status.last_run {
                    let within_last_10_seconds = if let Some(time) = last_run.time {
                        (time - Utc::now()) < chrono::Duration::seconds(10)
                    } else {
                        false
                    };

                    if within_last_10_seconds {
                        last_run.time = None;
                    }
                }
            }

            if last_status != status {
                trace!("Transitioner status changed: {:?}", status);

                let counter = service_state.update_status(|full_status| {
                    full_status.transitions = status.clone();
                });

                service_state.send_to_all_clients(Message::TransitionStatus {
                    counter,
                    transitions: status.clone(),
                });

                last_status = status;
            } else {
                trace!("Transitioner status unchanged");
            }

            thread::sleep(Duration::from_secs(1));
        }
    })
}
