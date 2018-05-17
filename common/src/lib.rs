#[macro_use]
extern crate failure;
extern crate git2;
extern crate serde;
#[macro_use]
extern crate serde_derive;

mod config;
pub mod deployment;
pub mod git;

pub use config::Config;
