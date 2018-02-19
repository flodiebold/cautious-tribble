#[macro_use]
extern crate failure;
extern crate git2;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_yaml;

use std::time::Duration;
use std::thread;
use std::path::Path;
use std::process;

use failure::{Error, ResultExt};

use git2::{ObjectType, Repository, Signature};

mod locks;

// TODO maybe move these to a common library
fn update(repo: &Repository, url: &str) -> Result<(), Error> {
    let mut remote = repo.remote_anonymous(url)
        .context("creating remote failed")?;

    remote
        .fetch(&["+refs/heads/master:refs/dm_head"], None, None)
        .context("fetch failed")?;

    Ok(())
}

fn push(repo: &Repository, url: &str) -> Result<(), Error> {
    let mut remote = repo.remote_anonymous(url)
        .context("creating remote failed")?;

    // TODO according to git2 documentation: Note that you'll likely want to use
    // RemoteCallbacks and set push_update_reference to test whether all the
    // references were pushed successfully.
    remote
        .push(&["+refs/dm_head:refs/heads/master"], None)
        .context("push failed")?;

    Ok(())
}

fn init_or_open(checkout_path: &str) -> Result<Repository, Error> {
    let repo = if Path::new(checkout_path).is_dir() {
        Repository::open(checkout_path).context("open failed")?
    } else {
        Repository::init_bare(checkout_path).context("init --bare failed")?
    };

    Ok(repo)
}

#[derive(Debug)]
struct Transition {
    source: String,
    target: String,
}

#[derive(Debug)]
enum TransitionResult {
    /// The transition was performed successfully.
    Success,
    /// The transition was not applicable for some reason, or there was no change.
    Skipped,
    /// The transition would be applicable, but is currently blocked and needs
    /// to be tried again.
    Blocked,
    /// A precondition check was negative.
    CheckFailed,
}

fn run_transition(transition: &Transition, repo: &Repository) -> Result<TransitionResult, Error> {
    let target_locks = locks::load_locks(repo, &transition.target)?;
    if target_locks.env_lock.is_locked() {
        return Ok(TransitionResult::Skipped);
    }

    let head = repo.find_reference("refs/dm_head")
        .context("refs/dm_head not found")?;
    let head_commit = head.peel_to_commit()?;

    let tree = head.peel_to_tree()?;
    let source_deployments_obj = tree.get_path(&Path::new(&transition.source).join("deployments"))
        .context("source deployments folder not found")?
        .to_object(&repo)?;
    let source_deployments = match source_deployments_obj.into_tree() {
        Ok(t) => t,
        Err(_) => bail!("(source)/deployments is not a tree!"),
    };

    let target_deployments_obj = tree.get_path(&Path::new(&transition.target).join("deployments"))
        .context("target deployments folder not found")?
        .to_object(&repo)?;
    let target_deployments = match target_deployments_obj.into_tree() {
        Ok(t) => t,
        Err(_) => bail!("(target)/deployments is not a tree!"),
    };
    let mut target_deployments_builder = repo.treebuilder(Some(&target_deployments))?;

    // copy over blob references
    for entry in source_deployments.iter() {
        if entry.kind() == Some(ObjectType::Blob) {
            // find corresponding entry in target deployments
            target_deployments_builder.insert(entry.name_bytes(), entry.id(), entry.filemode())?;
        }
    }

    let new_target_deployments_oid = target_deployments_builder.write()?;

    if new_target_deployments_oid == target_deployments.id() {
        // nothing changed
        return Ok(TransitionResult::Skipped);
    }

    // TODO refactor this:
    let old_target_entry = tree.get_name(&transition.target).unwrap();
    let mut target_tree_builder =
        repo.treebuilder(Some(old_target_entry.to_object(repo)?.as_tree().unwrap()))?;
    target_tree_builder.insert(
        "deployments",
        new_target_deployments_oid,
        old_target_entry.filemode(),
    )?;
    let new_target_tree_oid = target_tree_builder.write()?;

    let mut tree_builder = repo.treebuilder(Some(&tree))?;
    tree_builder.insert(
        &transition.target,
        new_target_tree_oid,
        old_target_entry.filemode(),
    )?;
    let new_tree_oid = tree_builder.write()?;
    let new_tree = repo.find_tree(new_tree_oid)?;

    let signature = Signature::now("DM Transitioner", "n/a")?;

    let commit = repo.commit(
        Some("refs/dm_head"),
        &signature,
        &signature,
        &format!("Mirroring {} to {}", transition.source, transition.target),
        &new_tree,
        &[&head_commit],
    )?;

    let url = "../versions.git";
    push(repo, url)?;

    Ok(TransitionResult::Success)
}

fn run() -> Result<(), Error> {
    let url = "../versions.git";
    let checkout_path = "./versions_checkout";
    let repo = init_or_open(checkout_path)?;

    let transitions = vec![
        Transition {
            source: "available".to_owned(),
            target: "prod".to_owned(),
        },
    ];

    loop {
        update(&repo, url)?;

        for transition in transitions.iter() {
            match run_transition(&transition, &repo) {
                Ok(TransitionResult::Success) => break,
                Ok(TransitionResult::Skipped) => continue,
                // TODO we could instead just block transitions that touch the source env
                Ok(TransitionResult::Blocked) => break,
                Ok(TransitionResult::CheckFailed) => continue,
                Err(error) => {
                    eprintln!("Transition failed: {}\n{}", error, error.backtrace());
                    for cause in error.causes() {
                        eprintln!("caused by: {}", cause);
                    }
                    break;
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
