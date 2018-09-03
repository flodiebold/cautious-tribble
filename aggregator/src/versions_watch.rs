use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use failure::Error;
use git2::{Commit, Oid};
use serde_yaml;

use common::aggregator::{
    Message, ResourceId, ResourceRepoChange, ResourceRepoCommit, ResourceVersion, VersionsAnalysis,
};
use common::repo::{self, GitResourceRepo, ResourceRepo};

use super::ServiceState;

fn analyze_commit<'repo>(
    repo: &'repo GitResourceRepo,
    commit: Commit<'repo>,
    analysis: &VersionsAnalysis,
) -> Result<ResourceRepoCommit, Error> {
    let mut changes = Vec::with_capacity(2);

    let env_path = PathBuf::from("dev"); // FIXME

    repo.walk_commit(
        &env_path.join("version"),
        repo::oid_to_id(commit.id()),
        |entry| {
            if entry.last_change != repo::oid_to_id(commit.id()) {
                return Ok(());
            }
            // FIXME don't fail if anything is invalid here!
            let content: HashMap<String, String> = serde_yaml::from_slice(&entry.content)?;
            let name = entry
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| format_err!("Invalid file name {:?}", entry.path))?
                .to_string();
            let resource_id = ResourceId(name);
            if analysis
                .resources
                .get(&resource_id)
                .map(|r| r.versions.contains_key(&entry.content_id))
                .unwrap_or(false)
            {
                return Ok(());
            }
            // let base_file_name = env_path.join("base").join(&entry.path);
            // let base_file_content = match repo.get(&base_file_name) {
            //     Ok(Some(content)) => content,
            //     Ok(None) => {
            //         // FIXME report error (base file not found)
            //         eprintln!("error: base file {:?} not found", base_file_name);
            //         return Ok(());
            //     }
            //     Err(e) => bail!(e),
            // };
            // let base_file_content = serde_yaml::from_slice(&base_file_content)?;
            let version_name = content.get("version").expect("FIXME");
            let version = ResourceVersion {
                id: entry.content_id,
                introduced_in: repo::oid_to_id(commit.id()),
                version: version_name.clone(),
            };
            changes.push(ResourceRepoChange::NewVersion {
                resource: resource_id,
                version,
            });
            Ok(())
        },
    )?;

    Ok(ResourceRepoCommit {
        id: repo::oid_to_id(commit.id()),
        message: commit.message().unwrap_or("[invalid utf8]").to_string(),
        changes,
    })
}

fn analyze_commits(
    repo: &GitResourceRepo,
    analysis: &mut VersionsAnalysis,
    from: Option<Oid>,
    to: Oid,
) -> Result<(), Error> {
    let mut revwalk = repo.repo.revwalk()?;
    revwalk.push(to)?;
    if let Some(from) = from {
        revwalk.hide(from)?;
    }
    for oid in revwalk {
        let oid = oid?;
        let commit = repo.repo.find_commit(oid)?;

        let analyzed_commit = analyze_commit(repo, commit, analysis)?;

        analysis.add_commit(analyzed_commit);
    }
    Ok(())
}

pub fn start(service_state: Arc<ServiceState>) -> Result<thread::JoinHandle<()>, Error> {
    let mut repo = repo::GitResourceRepo::open(
        &service_state.config.common.versions_checkout_path,
        service_state.config.common.versions_url.clone(),
    )?;
    // TODO move to the ResourceRepo abstraction
    let mut last_head = None;
    let mut last_analysis: VersionsAnalysis = Default::default();
    let handle = thread::spawn(move || loop {
        if last_head != Some(repo.head) {
            let mut new_analysis = last_analysis.clone();
            if let Err(e) = analyze_commits(&repo, &mut new_analysis, last_head, repo.head) {
                error!("Error analyzing commits: {}", e);
            } else {
                last_head = Some(repo.head);
                last_analysis = new_analysis;

                let counter = {
                    let mut write_lock = service_state.full_status.write().unwrap();
                    let mut full_status = Arc::make_mut(&mut write_lock);
                    full_status.counter += 1;
                    full_status.analysis = last_analysis.clone();
                    full_status.counter
                };
                service_state
                    .bus
                    .lock()
                    .unwrap()
                    .broadcast(Arc::new(Message::Versions {
                        counter,
                        analysis: last_analysis.clone(),
                    }));
            }
        }

        thread::sleep(Duration::from_secs(1));

        if let Err(e) = repo.update() {
            error!("Error updating versions repo: {}", e);
            thread::sleep(Duration::from_secs(1));
        }
    });
    Ok(handle)
}
