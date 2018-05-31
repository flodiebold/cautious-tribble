use failure::Error;

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
    Blocked,
    Failed,
}

pub fn check_precondition(
    transition: &PendingTransitionInfo,
    precondition: &Precondition,
    service_state: &ServiceState,
) -> Result<PreconditionResult, Error> {
    match precondition {
        &Precondition::SourceClean => check_source_clean(transition, service_state),
    }
}

fn check_source_clean(
    transition: &PendingTransitionInfo,
    service_state: &ServiceState,
) -> Result<PreconditionResult, Error> {
    let deployer_url = if let Some(url) = service_state.config.deployer_url.as_ref() {
        url
    } else {
        error!(
            "Transition failed: SourceClean check failed because no deployer url is configured!"
        );
        return Ok(PreconditionResult::Failed);
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
        info!("Transition blocked: Deployer does not yet know about source env");
        return Ok(PreconditionResult::Blocked);
    };

    if env_status.deployed_version != transition.current_version {
        info!(
            "Transition blocked: Deployer is on commit {}",
            env_status.deployed_version
        );
        return Ok(PreconditionResult::Blocked);
    }

    match env_status.rollout_status {
        RolloutStatus::InProgress => {
            info!("Transition blocked: Rollout still in progress");
            Ok(PreconditionResult::Blocked)
        }
        RolloutStatus::Outdated => {
            info!("Transition blocked: Changes pending");
            Ok(PreconditionResult::Blocked)
        }
        RolloutStatus::Clean => {
            info!("SourceClean check ok");
            Ok(PreconditionResult::Success)
        }
        RolloutStatus::Failed => {
            warn!("Transition failed: Source rollout failed");
            Ok(PreconditionResult::Failed)
        }
    }
}
