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

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
struct Deployment {
    name: String,
    tag: String,
}

fn get_deployments(repo: &Repository) -> Result<Vec<Deployment>, Error> {
    let head = repo.find_reference("refs/dm_head")
        .context("refs/dm_head not found")?;

    let tree = head.peel_to_tree()
        .context("could not peel refs/dm_head to tree")?;

    let prod_deployments_obj = tree.get_path(Path::new("prod/deployments"))
        .context("prod/deployments not found")?
        .to_object(&repo)
        .context("to_object failed")?;

    let prod_deployments = match prod_deployments_obj.into_tree() {
        Ok(t) => t,
        Err(_) => bail!("prod/deployments is not a tree!")
    };

    let mut deployments = Vec::new();

    for entry in prod_deployments.iter() {
        let obj = entry.to_object(&repo)
            .context("to_object failed")?;

        if let Some(blob) = obj.as_blob() {
            let deployment = serde_yaml::from_slice(blob.content())
                .context("could not deserialize deployment")?;

            deployments.push(deployment);
        }
    }

    Ok(deployments)
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

    loop {
        update(&repo, url)?;

        let deployments = get_deployments(&repo)?;

        thread::sleep(Duration::from_millis(1000));
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
