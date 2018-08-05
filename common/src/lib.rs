#[macro_use]
extern crate failure;
extern crate git2;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[cfg(test)]
extern crate git_fixture;
extern crate tempfile;

mod config;
pub mod deployment;
pub mod git;
pub mod repo;

pub use config::Config;
