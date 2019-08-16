use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use failure::{format_err, Error};
use git2::{Commit, Oid, Sort};
use log::error;
use regex::Regex;
use serde_yaml;

use common::aggregator::{
    EnvName, Message, ResourceId, ResourceRepoChange, ResourceRepoCommit, ResourceVersion,
    VersionsAnalysis,
};
use common::chrono::{TimeZone, Utc};
use common::repo::{self, GitResourceRepo, ResourceRepo};

use super::ServiceState;

fn remove_trailers(mut msg: String) -> (String, HashMap<String, String>) {
    // TODO: handle multiline etc.
    let r = Regex::new(r"\s*(?m)^([a-zA-Z-]+)\s*:\s*(.*)$\s*\z").unwrap();
    let mut trailers = HashMap::new();
    while let Some(m) = r.captures(&msg) {
        let start = m.get(0).expect("first group always exists").start();
        let key = m[1].to_string();
        let value = m[2].to_string();
        trailers.insert(key, value);
        msg.truncate(start);
    }
    (msg, trailers)
}

fn split_log_message(msg: &str) -> (String, Option<String>) {
    let r = Regex::new(r"(?m)\n\s*\n").unwrap();
    let (header, body) = if let Some(m) = r.find(msg) {
        let pos = m.start();
        let body = msg[m.end()..].trim().to_string();
        if !body.is_empty() {
            (&msg[..pos], Some(body))
        } else {
            (&msg[..pos], None)
        }
    } else {
        (msg, None)
    };

    (header.trim().replace('\n', " "), body)
}

fn analyze_commit<'repo>(
    repo: &'repo GitResourceRepo,
    commit: Commit<'repo>,
    analysis: &VersionsAnalysis,
) -> Result<ResourceRepoCommit, Error> {
    let mut changes = Vec::with_capacity(2);

    let envs = [
        PathBuf::from("latest"),
        PathBuf::from("dev"),
        PathBuf::from("prod"),
    ]; // FIXME

    let msg = commit.message().unwrap_or("[invalid utf8]").to_string();
    let (msg, _trailers) = remove_trailers(msg);
    let (msg_header, msg_body) = split_log_message(&msg);

    let commit_id = repo::oid_to_id(commit.id());

    for env_path in &envs {
        let env = EnvName(env_path.to_str().unwrap().to_string());
        repo.walk_commit(&env_path.join("version"), commit_id, |entry| {
            if entry.last_change != commit_id {
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
                .map(|r| !r.versions.contains_key(&entry.content_id))
                .unwrap_or(true)
            {
                let version_name = content
                    .get("version")
                    .ok_or_else(|| format_err!("No version found"))?;
                let change_log = msg_body.clone().unwrap_or_else(String::new);
                let version = ResourceVersion {
                    version_id: entry.content_id,
                    introduced_in: commit_id,
                    version: version_name.clone(),
                    change_log,
                };
                let change = ResourceRepoChange::Version {
                    resource: resource_id.clone(),
                    version,
                };
                if !changes.contains(&change) {
                    changes.push(change);
                }
            }
            let previous_version_id = analysis
                .resources
                .get(&resource_id)
                .and_then(|r| r.version_by_env.get(&env))
                .map(|v| *v);
            if previous_version_id
                .map(|v| v != entry.content_id)
                .unwrap_or(true)
            {
                changes.push(ResourceRepoChange::VersionDeployed {
                    resource: resource_id,
                    env: env.clone(),
                    previous_version_id,
                    version_id: entry.content_id,
                });
            }
            Ok(())
        })?;
        repo.walk_commit(&env_path.join("base"), commit_id, |entry| {
            if entry.last_change != commit_id {
                return Ok(());
            }
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
                .and_then(|r| r.base_data.get(&env))
                .map(|v| *v == entry.content_id)
                .unwrap_or(false)
            {
                return Ok(());
            }
            changes.push(ResourceRepoChange::BaseData {
                resource: resource_id,
                env: env.clone(),
                content_id: entry.content_id,
            });
            Ok(())
        })?;
        repo.walk_commit(&env_path.join("deployable"), commit_id, |entry| {
            if entry.last_change != commit_id {
                return Ok(());
            }
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
                .and_then(|r| r.base_data.get(&env))
                .map(|v| *v == entry.content_id)
                .unwrap_or(false)
            {
                return Ok(());
            }
            changes.push(ResourceRepoChange::Deployable {
                resource: resource_id,
                env: env.clone(),
                content_id: entry.content_id,
            });
            Ok(())
        })?;
    }

    Ok(ResourceRepoCommit {
        id: commit_id,
        message: msg_header,
        long_message: msg_body.unwrap_or_else(String::new),
        author_name: commit
            .author()
            .name()
            .unwrap_or("[invalid utf8]")
            .to_string(),
        author_email: commit
            .author()
            .email()
            .unwrap_or("[invalid utf8]")
            .to_string(),
        time: Utc
            .timestamp_opt(commit.author().when().seconds(), 0)
            .single()
            .unwrap_or_else(|| Utc.timestamp(0, 0)),
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
    revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE);
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
    let mut repo = repo::GitResourceRepo::open(service_state.env.common.clone())?;
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

                let counter = service_state.update_status(|full_status| {
                    full_status.analysis = last_analysis.clone();
                });
                service_state.send_to_all_clients(Message::Versions {
                    counter,
                    analysis: last_analysis.clone(),
                });
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

#[cfg(test)]
mod test {
    use super::*;
    use git_fixture;

    #[test]
    fn remove_trailers_noop() {
        let s = r"Foo bar
Baz.";
        let (s2, trailers) = remove_trailers(s.to_string());
        assert_eq!(s, s2);
        assert!(trailers.is_empty());
    }
    #[test]
    fn remove_trailers_one() {
        let s = r"Foo bar
Baz.

DM-Transition: blubb

";
        let (s2, trailers) = remove_trailers(s.to_string());
        assert_eq!("Foo bar\nBaz.", s2);
        assert_eq!(trailers["DM-Transition"], "blubb");
    }
    #[test]
    fn remove_trailers_multiple() {
        let s = r"Foo bar
Baz.

DM-Transition: blubb

DM-Source: foo
DM-Target: bar

";
        let (s2, trailers) = remove_trailers(s.to_string());
        assert_eq!("Foo bar\nBaz.", s2);
        assert_eq!(trailers["DM-Transition"], "blubb");
        assert_eq!(trailers["DM-Source"], "foo");
        assert_eq!(trailers["DM-Target"], "bar");
    }

    #[test]
    fn split_log_message_noop() {
        let s = "Foo bar baz.";
        let (header, body) = split_log_message(s);
        assert_eq!(header, s);
        assert_eq!(body, None);
    }

    #[test]
    fn split_log_message_merge_lines() {
        let s = "Foo bar
baz.";
        let (header, body) = split_log_message(s);
        assert_eq!(header, "Foo bar baz.");
        assert_eq!(body, None);
    }

    #[test]
    fn split_log_message_1() {
        let s = "Foo bar

baz.";
        let (header, body) = split_log_message(s);
        assert_eq!(header, "Foo bar");
        assert_eq!(body.unwrap(), "baz.");
    }

    fn make_resource_repo(
        git_fixture: git_fixture::RepoFixture,
        head: &str,
    ) -> common::repo::GitResourceRepoWithTempDir {
        let head = git_fixture.get_commit(head).unwrap();
        let (repo, tempdir) = git_fixture.into_inner();
        let env = common::Env {
            versions_url: String::new(),
            versions_checkout_path: String::new(),
            ssh_public_key: None,
            ssh_private_key: None,
            ssh_username: None,
        };
        let inner = GitResourceRepo::from_repo(repo, head, env);
        common::repo::GitResourceRepoWithTempDir { inner, tempdir }
    }

    #[test]
    fn analyze_commits_1() {
        let fixture =
            git_fixture::RepoFixture::from_str(include_str!("./fixtures/test_repo1.yaml")).unwrap();
        let repo = make_resource_repo(fixture, "head");
        let mut analysis = VersionsAnalysis::default();
        analyze_commits(&repo.inner, &mut analysis, None, repo.inner.head).unwrap();
        assert_eq!(analysis.history.len(), 2);

        assert_eq!(analysis.history[0].message, "Commit first");
        assert_eq!(analysis.history[1].message, "Commit head");

        let foo_id = ResourceId("foo".to_string());
        let env = EnvName("dev".to_string());
        let first_commit_id = analysis.history[0].id;
        let version_1_id = "b82551848c644f63b8517a7bdf8be9a992e6f4da".parse().unwrap();
        let expected_changes = &[
            ResourceRepoChange::Version {
                resource: foo_id.clone(),
                version: ResourceVersion {
                    version_id: version_1_id,
                    introduced_in: first_commit_id,
                    version: "1".to_string(),
                    change_log: "".to_string(),
                },
            },
            ResourceRepoChange::VersionDeployed {
                resource: foo_id.clone(),
                env: env.clone(),
                previous_version_id: None,
                version_id: version_1_id,
            },
            ResourceRepoChange::BaseData {
                resource: foo_id.clone(),
                env: env.clone(),
                content_id: "59c5d2b4bc66e952a99b3b18a89cbc1e6704ffa0".parse().unwrap(),
            },
        ];
        assert_eq!(analysis.history[0].changes, expected_changes);

        let head_commit_id = analysis.history[1].id;
        let version_2_id = "22817d2a9c7fc1f62d5670ca1e44948446543973".parse().unwrap();
        let expected_changes_2 = &[
            ResourceRepoChange::Version {
                resource: foo_id.clone(),
                version: ResourceVersion {
                    version_id: version_2_id,
                    introduced_in: head_commit_id,
                    version: "2".to_string(),
                    change_log: "".to_string(),
                },
            },
            ResourceRepoChange::VersionDeployed {
                resource: foo_id.clone(),
                env: env.clone(),
                previous_version_id: Some(version_1_id),
                version_id: version_2_id,
            },
        ];
        assert_eq!(analysis.history[1].changes, expected_changes_2);
    }
}
