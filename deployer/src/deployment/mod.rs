use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use failure::Error;
use git2::{ErrorCode, Repository};

use common::deployment::{DeploymentState, RolloutStatus};
use common::git::{self, TreeZipper, VersionHash};

pub mod dummy;
pub mod kubernetes;

pub trait Deployer {
    fn deploy(&mut self, deployments: &[Deployable]) -> Result<(), Error>;

    fn check_rollout_status(
        &mut self,
        deployables: &[Deployable],
    ) -> Result<(RolloutStatus, HashMap<String, DeploymentState>), Error>;
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Deployable {
    pub name: String,
    pub file_name: PathBuf,
    pub content: Vec<u8>,
    pub version: VersionHash,
    pub message: String,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct DeploymentsInfo {
    pub deployments: Vec<Deployable>,
}

pub fn get_deployments(
    repo: &Repository,
    env: &str,
    last_version: Option<VersionHash>,
) -> Result<Option<DeploymentsInfo>, Error> {
    let mut blob_id_by_deployment = HashMap::new();
    let mut deployments = HashMap::<String, Deployable>::new();

    // collect current versions of all deployments
    let head_commit = git::get_head_commit(repo)?;

    if last_version
        .map(|id| id == head_commit.id().into())
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
            let deployment = Deployable {
                name: name.clone(),
                file_name,
                content,
                version: head_commit.id().into(),
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
                deployment.version = commit.id().into();
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

#[cfg(test)]
mod test {
    use super::*;
    use git_fixture;

    #[test]
    fn test_get_deployments_no_deployments() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_deployments_no_deployments.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result = get_deployments(&fixture.repo, "available", None).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_deployments_1() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_deployments_1.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result = get_deployments(&fixture.repo, "available", None).unwrap();
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.deployments.len(), 1);
        assert_eq!(info.deployments[0].name, "foo");
        assert_eq!(info.deployments[0].file_name, Path::new("foo"));
        assert_eq!(info.deployments[0].content, "blubb".as_bytes());
        assert_eq!(
            info.deployments[0].version,
            fixture.get_commit("head").unwrap().into()
        )
    }

    #[test]
    fn test_get_deployments_2() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_deployments_2.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result = get_deployments(&fixture.repo, "available", None).unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.deployments.sort_by_key(|d| d.name.clone());
        assert_eq!(info.deployments.len(), 2);
        assert_eq!(info.deployments[0].name, "bar");
        assert_eq!(info.deployments[0].file_name, Path::new("bar"));
        assert_eq!(info.deployments[0].content, "xx".as_bytes());
        assert_eq!(
            info.deployments[0].version,
            fixture.get_commit("head").unwrap().into()
        );
        assert_eq!(info.deployments[1].name, "foo");
        assert_eq!(info.deployments[1].file_name, Path::new("foo"));
        assert_eq!(info.deployments[1].content, "blubb".as_bytes());
        assert_eq!(
            info.deployments[1].version,
            fixture.get_commit("head").unwrap().into()
        );
    }

    #[test]
    fn test_get_deployments_changed() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_deployments_changed.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result = get_deployments(
            &fixture.repo,
            "available",
            fixture.commits.get("first").cloned().map(VersionHash::from),
        ).unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.deployments.sort_by_key(|d| d.name.clone());
        assert_eq!(info.deployments.len(), 2);
        assert_eq!(info.deployments[0].name, "bar");
        assert_eq!(info.deployments[0].content, "yy".as_bytes());
        assert_eq!(
            info.deployments[0].version,
            fixture.get_commit("head").unwrap().into()
        );
        assert_eq!(info.deployments[1].name, "foo");
        assert_eq!(
            info.deployments[1].version,
            fixture.get_commit("first").unwrap().into()
        );
    }

    #[test]
    fn test_get_deployments_added() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_deployments_added.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result = get_deployments(
            &fixture.repo,
            "available",
            fixture.commits.get("first").cloned().map(VersionHash::from),
        ).unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.deployments.sort_by_key(|d| d.name.clone());
        assert_eq!(info.deployments.len(), 2);
        assert_eq!(info.deployments[0].name, "bar");
        assert_eq!(info.deployments[0].content, "yy".as_bytes());
        assert_eq!(
            info.deployments[0].version,
            fixture.get_commit("head").unwrap().into()
        );
        assert_eq!(info.deployments[1].name, "foo");
        assert_eq!(
            info.deployments[1].version,
            fixture.get_commit("first").unwrap().into()
        );
    }
}
