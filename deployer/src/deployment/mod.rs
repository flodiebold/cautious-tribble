use failure::Error;

use ::Deployment;

pub mod kubernetes;
pub mod dummy;

pub trait Deployer {
    fn deploy(&mut self, deployments: &[Deployment]) -> Result<(), Error>;
}
