use failure::Error;
use log::{error, info, warn};
use serde_derive::{Deserialize, Serialize};

use common::deployment::{AllDeployerStatus, RolloutStatus};

use super::{PendingTransitionInfo, ServiceState};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Precondition {
    SourceClean,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PreconditionResult {
    Success,
    // TODO return more information here
    Blocked { message: String },
    Failed { message: String },
}

pub fn check_precondition(
    transition: &PendingTransitionInfo,
    precondition: &Precondition,
    service_state: &ServiceState,
) -> Result<PreconditionResult, Error> {
    match precondition {
        Precondition::SourceClean => check_source_clean(transition, service_state),
    }
}

fn check_source_clean(
    transition: &PendingTransitionInfo,
    service_state: &ServiceState,
) -> Result<PreconditionResult, Error> {
    let deployer_url = if let Some(url) = service_state.env.deployer_url.as_ref() {
        url
    } else {
        error!(
            "Transition failed: SourceClean check failed because no deployer url is configured!"
        );
        return Ok(PreconditionResult::Failed {
            message: "no deployer url configured".to_string(),
        });
    };
    let url = format!("{}/status", deployer_url);
    let status: AllDeployerStatus = service_state
        .client
        .get(&url)
        .send()?
        .error_for_status()?
        .json()?;

    let env_status = if let Some(env_status) = status.deployers.get(&transition.source) {
        env_status
    } else {
        return Ok(PreconditionResult::Blocked {
            message: "deployer does not yet know about source env".to_string(),
        });
    };

    if env_status.deployed_version != transition.current_version {
        return Ok(PreconditionResult::Blocked {
            message: format!(
                "deployer is on version {}, we're on version {}",
                env_status.deployed_version, transition.current_version
            ),
        });
    }

    match env_status.rollout_status {
        RolloutStatus::InProgress => {
            info!("Transition blocked: Rollout still in progress");
            Ok(PreconditionResult::Blocked {
                message: "rollout still in progress".to_string(),
            })
        }
        RolloutStatus::Outdated => {
            info!("Transition blocked: Changes pending");
            Ok(PreconditionResult::Blocked {
                message: "changes pending".to_string(),
            })
        }
        RolloutStatus::Clean => {
            info!("SourceClean check ok");
            Ok(PreconditionResult::Success)
        }
        RolloutStatus::Failed => {
            warn!("Transition failed: Source rollout failed");
            Ok(PreconditionResult::Failed {
                message: "rollout failed".to_string(),
            })
        }
    }
}
