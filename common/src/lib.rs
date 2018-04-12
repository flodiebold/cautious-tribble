#[macro_use]
extern crate failure;
extern crate git2;
extern crate serde;
#[macro_use]
extern crate serde_derive;

pub mod git;
pub mod deployment;
mod config;

pub use config::{Config as Config};
