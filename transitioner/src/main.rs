#[macro_use] extern crate failure;
extern crate git2;
extern crate serde;
#[macro_use] extern crate serde_derive;
extern crate serde_yaml;

use std::time::Duration;
use std::thread;
use std::path::Path;
use std::process;

use failure::{ResultExt, Error};

use git2::{Repository};

// TODO maybe move these to a common library
fn update(repo: &Repository, url: &str) -> Result<(), Error> {
    let mut remote = repo.remote_anonymous(url)
        .context("creating remote failed")?;

    remote.fetch(&["+refs/heads/master:refs/dm_head"], None, None)
        .context("fetch failed")?;

    Ok(())
}

fn init_or_open(checkout_path: &str) -> Result<Repository, Error> {
    let repo = if Path::new(checkout_path).is_dir() {
        Repository::open(checkout_path)
            .context("open failed")?
    } else {
        Repository::init_bare(checkout_path)
            .context("init --bare failed")?
    };

    Ok(repo)
}

struct Transition {
    source: String,
    target: String,
}

enum TransitionResult {
    /// The transition was performed successfully.
    Success,
    /// The transition was not applicable for some reason.
    Skipped,
    /// The transition would be applicable, but is currently blocked and needs
    /// to be tried again.
    Blocked
}

fn run_transition(transition: &Transition, repo: &Repository) -> Result<TransitionResult, Error> {
    Ok(TransitionResult::Success)
}

fn run() -> Result<(), Error> {
    let url = "../versions";
    let checkout_path = "./versions_checkout";
    let repo = init_or_open(checkout_path)?;

    let transitions = vec![
        Transition {
            source: "available".to_owned(),
            target: "prod".to_owned(),
        }
    ];

    loop {
        update(&repo, url)?;

        for transition in transitions.iter() {
            match run_transition(&transition, &repo) {
                Ok(TransitionResult::Success) => break,
                Ok(TransitionResult::Skipped) => continue,
                Ok(TransitionResult::Blocked) => break,
                Err(error) => {
                    eprintln!("Transition failed: {}", error);
                    break
                }
            }
        }

        thread::sleep(Duration::from_millis(1000));
    }
}

fn main() {
    match run() {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("{}\n{}", e, e.backtrace());
            for cause in e.causes() {
                eprintln!("caused by: {}", cause);
            }
            process::exit(1);
        }
    }
}
