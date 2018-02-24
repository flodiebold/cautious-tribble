#[macro_use]
extern crate failure;
extern crate git2;
extern crate kubeclient;
extern crate serde;
extern crate serde_yaml;

extern crate common;

use std::ffi::OsStr;
use std::time::Duration;
use std::thread;
use std::path::{Path, PathBuf};
use std::process;
use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;

use failure::Error;

use git2::{ErrorCode, Repository};

use common::git::{self, TreeZipper};

mod deployment;

/// The hash of a commit in the versions repo
pub type VersionHash = git2::Oid;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Deployment {
    name: String,
    file_name: PathBuf,
    content: Vec<u8>,
    version: VersionHash,
    message: String,
}

pub struct DeploymentsInfo {
    deployments: Vec<Deployment>,
}

fn get_deployments(
    repo: &Repository,
    env: &str,
    last_version: Option<VersionHash>,
) -> Result<Option<DeploymentsInfo>, Error> {
    let mut blob_id_by_deployment = HashMap::new();
    let mut deployments = HashMap::<String, Deployment>::new();

    // collect current versions of all deployments
    let head_commit = git::get_head_commit(repo)?;

    if last_version
        .map(|id| id == head_commit.id())
        .unwrap_or(false)
    {
        return Ok(None);
    }

    let tree = head_commit.tree()?;
    let mut zipper = TreeZipper::from(repo, tree);
    zipper.descend(env)?;
    zipper.descend("deployments")?;
    let deployments_tree = if let Some(t) = zipper.into_inner() {
        t
    } else {
        // deployments folder does not exist
        return Ok(None);
    };

    for entry in deployments_tree.iter() {
        let obj = entry.to_object(&repo)?;

        if let Some(blob) = obj.as_blob() {
            let content = blob.content().to_owned();
            let file_name = Path::new(OsStr::from_bytes(entry.name_bytes())).to_path_buf();
            let name = file_name
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| format_err!("Invalid file name {:?}", file_name))?
                .to_string();
            let deployment = Deployment {
                name: name.clone(),
                file_name,
                content,
                version: head_commit.id(),
                message: head_commit
                    .message()
                    .unwrap_or("[invalid utf8]")
                    .to_string(),
            };

            blob_id_by_deployment.insert(name.clone(), blob.id());
            deployments.insert(name, deployment);
        }
    }

    // now walk back to find the oldest revision that contains each deployment
    let mut revwalk = repo.revwalk()?;

    revwalk.push(head_commit.id())?;

    // TODO: remove deployments that didn't change since last_version?

    for rev_result in revwalk {
        let commit = repo.find_commit(rev_result?)?;
        let tree = commit.tree()?;

        let mut zipper = TreeZipper::from(repo, tree);
        zipper.descend(env)?;
        zipper.descend("deployments")?;
        let deployments_tree = if let Some(t) = zipper.into_inner() {
            t
        } else {
            break;
        };

        for (name, deployment) in deployments.iter_mut() {
            let blob_id = if let Some(id) = blob_id_by_deployment.get(name) {
                *id
            } else {
                continue;
            };

            let tree_entry = match deployments_tree.get_path(&deployment.file_name) {
                Ok(f) => f,
                Err(ref e) if e.code() == ErrorCode::NotFound => continue,
                Err(e) => Err(e)?,
            };

            if tree_entry.id() == blob_id {
                deployment.version = commit.id();
                deployment.message = commit.message().unwrap_or("[invalid utf8]").to_string();
            } else {
                // remove to stop looking at changes for this one
                blob_id_by_deployment.remove(name);
            }
        }

        if blob_id_by_deployment.is_empty() {
            break;
        }
    }

    let result = DeploymentsInfo {
        deployments: deployments.into_iter().map(|(_, v)| v).collect(),
    };

    Ok(Some(result))
}

fn run() -> Result<(), Error> {
    let url = "../versions.git";
    let checkout_path = "./versions_checkout";
    let repo = git::init_or_open(checkout_path)?;

    // let mut deployer: Box<deployment::Deployer> = Box::new(deployment::DummyDeployer::new());
    let mut deployers = {
        let mut m = HashMap::<_, Box<deployment::Deployer>>::new();
        m.insert(
            "prod",
            Box::new(deployment::kubernetes::KubernetesDeployer::new(
                "/home/florian/.kube/config",
                "default",
            )?),
        );
        m.insert(
            "dev",
            Box::new(deployment::kubernetes::KubernetesDeployer::new(
                "/home/florian/.kube/config",
                "dev",
            )?),
        );
        m
    };
    let mut last_version = None;

    let envs = &["dev", "prod"];

    loop {
        git::update(&repo, url)?;

        for env in envs {
            if let Some(deployments) = get_deployments(&repo, env, last_version)? {
                let deployer = if let Some(d) = deployers.get_mut(env) {
                    d
                } else {
                    // no deployer, ignore
                    continue;
                };
                let result = deployer.deploy(&deployments.deployments);

                if let Err(e) = result {
                    eprintln!("Deployment failed: {}\n{}", e, e.backtrace());
                    for cause in e.causes() {
                        eprintln!("caused by: {}", cause);
                    }
                    continue;
                }

                last_version = Some(git::get_head_commit(&repo)?.id());
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
