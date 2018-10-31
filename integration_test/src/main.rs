#![allow(unused)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use failure::Error;

mod integration_test;

use crate::integration_test::*;

fn main() -> Result<(), Error> {
    let mut test = IntegrationTest::new_playground();
    let fixture = test.git_fixture(include_str!("./example.yaml"));
    fixture.set_ref("refs/heads/master", "base").unwrap();
    // FIXME make it possible to interrupt during this
    test.run_deployer(include_str!("./config_playground.yaml"))
        .run_transitioner(include_str!("./config_playground.yaml"))
        .run_aggregator(include_str!("./config_playground.yaml"))
        .wait_ready()
        .connect_to_aggregator_socket()
        .wait_transition("prod", 1);
    fixture.apply("refs/heads/master", "c1");
    test.wait_transition("prod", 2);
    fixture.apply("refs/heads/master", "c2");
    test.wait_transition("prod", 3)
        .wait_env_rollout_done("prod");

    eprintln!("Playground running");

    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(2, Arc::clone(&term))?; // int
    signal_hook::flag::register(15, Arc::clone(&term))?; // term

    loop {
        thread::sleep(std::time::Duration::from_millis(1000));
        if term.load(Ordering::Relaxed) {
            eprintln!("Stopping playground");
            return Ok(());
        }
    }
}
