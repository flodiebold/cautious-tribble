#[macro_use] extern crate failure;
extern crate git2;
extern crate kubeclient;
extern crate serde;
#[macro_use] extern crate serde_derive;
extern crate serde_yaml;

use std::time::Duration;
use std::thread;
use std::path::Path;
use std::process;
use std::collections::HashMap;

use failure::{ResultExt, Error};

use git2::{Repository};

mod deployment;

/// The hash of a commit in the versions repo
pub type VersionHash = git2::Oid;

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct DeploymentSpec {
    name: String,
    tag: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Deployment {
    spec: DeploymentSpec,
    version: VersionHash,
    file_name: String,
}

fn get_deployments(repo: &Repository) -> Result<Vec<Deployment>, Error> {
    let mut blob_id_by_deployment = HashMap::new();
    let mut deployments = HashMap::<String, Deployment>::new();

    // collect current versions of all deployments
    let head = repo.find_reference("refs/dm_head")
        .context("refs/dm_head not found")?;
    let head_id = head.peel_to_commit()?.id();
    let tree = head.peel_to_tree()?;
    let prod_deployments_obj = tree.get_path(Path::new("prod/deployments"))
        .context("prod/deployments not found")?
        .to_object(&repo)?;
    let prod_deployments = match prod_deployments_obj.into_tree() {
        Ok(t) => t,
        Err(_) => bail!("prod/deployments is not a tree!")
    };

    for entry in prod_deployments.iter() {
        let obj = entry.to_object(&repo)?;

        if let Some(blob) = obj.as_blob() {
            let spec = serde_yaml::from_slice::<DeploymentSpec>(blob.content())
                .context("could not deserialize deployment")?;

            let name = spec.name.clone();
            let deployment = Deployment {
                spec,
                version: head_id,
                file_name: entry.name().ok_or_else(|| format_err!("non-utf8 file name"))?.to_string()
            };

            blob_id_by_deployment.insert(name.clone(), blob.id());
            deployments.insert(name, deployment);
        }
    }

    // now walk back to find the oldest revision that contains each deployment
    let mut revwalk = repo.revwalk()?;

    revwalk.push(head_id)?;

    for rev_result in revwalk {
        let commit = repo.find_commit(rev_result?)?;
        let tree = commit.tree()?;

        let prod_deployments_obj = tree.get_path(Path::new("prod/deployments"))
            .context("prod/deployments not found")?
        .to_object(&repo)?;
        let prod_deployments = match prod_deployments_obj.into_tree() {
            Ok(t) => t,
            Err(_) => bail!("prod/deployments is not a tree!")
        };

        for (name, deployment) in deployments.iter_mut() {
            let blob_id = *blob_id_by_deployment.get(name)
                .expect("this should never happen");

            let tree_entry = match prod_deployments.get_name(&deployment.file_name) {
                Some(f) => f,
                None => {
                    continue
                }
            };

            if tree_entry.id() == blob_id {
                deployment.version = commit.id();
            }
        }

        // TODO stop when all deployments have changed
    }

    Ok(deployments.into_iter().map(|(_, v)| v).collect())
}

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

fn run() -> Result<(), Error> {
    let url = "file:///home/florian/Projekte/privat/new-dm/versions";
    let checkout_path = "./versions_checkout";
    let repo = init_or_open(checkout_path)?;

    let mut deployer: Box<deployment::Deployer> = Box::new(deployment::DummyDeployer::new());

    loop {
        update(&repo, url)?;

        let deployments = get_deployments(&repo)?;

        deployer.deploy(&deployments)?;

        thread::sleep(Duration::from_millis(1000));
        // return Ok(());
    }
}

fn main() {
    match run() {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    }
}
