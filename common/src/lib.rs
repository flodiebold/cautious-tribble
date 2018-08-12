#[macro_use]
extern crate failure;
extern crate git2;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate chrono;
#[cfg(test)]
extern crate git_fixture;
extern crate tempfile;

pub mod aggregator;
mod config;
pub mod deployment;
pub mod git;
pub mod repo;
pub mod transitions;

pub use config::Config;
