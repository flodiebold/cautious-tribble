use failure::Error;

use ::Deployment;

pub mod kubernetes;

pub trait Deployer {
    fn deploy(&mut self, deployments: &[Deployment]) -> Result<(), Error>;
}
