use std::collections::HashMap;
use std::path::PathBuf;

use failure::Error;
use git2::{ErrorCode, Repository};
use regex;
use serde_yaml;

use common::deployment::{DeployerStatus, DeploymentState, RolloutStatus};
use common::git::{self, TreeZipper, VersionHash};

pub mod kubernetes;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Deployable {
    pub name: String,
    pub base_file_name: PathBuf,
    pub version_file_name: Option<PathBuf>,
    pub version_content: serde_yaml::Mapping,
    pub merged_content: serde_yaml::Value,
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
    zipper.descend("deployable")?;

    for (file_name, entry) in zipper.walk(true) {
        let obj = entry?.to_object(&repo)?;

        if let Some(blob) = obj.as_blob() {
            let content = serde_yaml::from_slice(blob.content())?;
            // FIXME don't fail everything if content is invalid, report the
            // deployable as errored
            let name = file_name
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| format_err!("Invalid file name {:?}", file_name))?
                .to_string();
            let deployment = Deployable {
                name: name.clone(),
                base_file_name: PathBuf::from("deployable").join(file_name),
                merged_content: content,
                version_content: serde_yaml::Mapping::new(),
                version_file_name: None,
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

    zipper.ascend()?;
    let mut base_zipper = zipper.clone();
    base_zipper.descend("base")?;

    zipper.descend("version")?;

    if let Some(base_tree) = base_zipper.into_inner() {
        for (file_name, entry) in zipper.walk(true) {
            let obj = entry?.to_object(&repo)?;

            if let Some(blob) = obj.as_blob() {
                // FIXME don't fail everything if content is invalid
                let content = if let serde_yaml::Value::Mapping(m) =
                    serde_yaml::from_slice(blob.content())?
                {
                    m
                } else {
                    // FIXME report error
                    eprintln!("error: versions file not a mapping");
                    continue;
                };
                let name = file_name
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| format_err!("Invalid file name {:?}", file_name))?
                    .to_string();
                let base_file_name = PathBuf::from("base").join(&file_name);
                let base_file = match base_tree.get_path(&file_name) {
                    Ok(tree_entry) => {
                        let obj = tree_entry.to_object(&repo)?;
                        if let Some(blob) = obj.as_blob() {
                            blob.clone()
                        } else {
                            // FIXME report error (base file not a file)
                            eprintln!("error: base file not a file");
                            continue;
                        }
                    }
                    Err(ref e) if e.code() == ErrorCode::NotFound => {
                        // FIXME report error (base file not found)
                        eprintln!("error: base file {:?} not found", base_file_name);
                        continue;
                    }
                    Err(e) => bail!(e),
                };
                let base_file_content = serde_yaml::from_slice(base_file.content())?;
                let deployment = Deployable {
                    name: name.clone(),
                    base_file_name,
                    merged_content: merge_deployable(base_file_content, &content),
                    version_content: content,
                    version_file_name: Some(PathBuf::from("version").join(&file_name)),
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
    }

    zipper.ascend()?;

    // now walk back to find the oldest revision that contains each deployment
    // TODO: abstract this into a separate function that just takes a list of
    // files and returns when they last changed
    let mut revwalk = repo.revwalk()?;

    revwalk.push(head_commit.id())?;

    // TODO: remove deployments that didn't change since last_version?

    for rev_result in revwalk {
        let commit = repo.find_commit(rev_result?)?;
        let tree = commit.tree()?;

        let mut zipper = TreeZipper::from(repo, tree);
        zipper.descend(env)?;
        let deployments_tree = if let Some(t) = zipper.into_inner() {
            t
        } else {
            break;
        };

        for (name, deployment) in &mut deployments {
            let blob_id = if let Some(id) = blob_id_by_deployment.get(name) {
                *id
            } else {
                continue;
            };

            let tree_entry = match deployments_tree.get_path(&deployment.base_file_name) {
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

fn merge_deployable(
    mut base: serde_yaml::Value,
    version_content: &serde_yaml::Mapping,
) -> serde_yaml::Value {
    use serde_yaml::*;
    // TODO rewrite everything about this
    fn merge_mut(base: &mut Value, version_content: &Mapping) {
        match base {
            Value::String(s) => {
                let regex = regex::Regex::new("\\$version").unwrap();
                let replaced = regex
                    .replace_all(&s, move |_cap: &regex::Captures<'_>| {
                        let version = version_content
                            .get(&Value::String("version".to_owned()))
                            .unwrap(); // FIXME
                        match version {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            _ => String::new(),
                        }
                    }).into_owned();
                *s = replaced;
            }
            Value::Sequence(s) => {
                for element in s {
                    merge_mut(element, version_content);
                }
            }
            Value::Mapping(m) => {
                for (_, value) in m {
                    merge_mut(value, version_content);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
    merge_mut(&mut base, version_content);
    base
}

pub fn deploy(
    deployer: &mut kubernetes::KubernetesDeployer,
    deployments: &[Deployable],
) -> Result<(), Error> {
    let current_state = deployer.retrieve_current_state(deployments)?;

    for d in deployments {
        debug!("looking at {}", d.name);
        let deployed_version = if let Some(v) = current_state.get(&d.name) {
            v.clone()
        } else {
            warn!("no known version for {}, not deploying", d.name);
            continue;
        };

        if let DeploymentState::Deployed { version, .. } = deployed_version {
            if version == d.version {
                info!("same version for {}, not deploying", d.name);
                continue;
            }
        }

        info!(
            "Deploying {} version {} with content {}",
            d.name,
            d.version,
            serde_yaml::to_string(&d.merged_content).unwrap_or_default() // FIXME
        );

        match deployer.deploy(d) {
            Ok(()) => {}
            Err(e) => {
                // TODO: maybe instead mark the service as failing to deploy
                // and don't try again?
                error!("Deployment of {} failed: {}\n{}", d.name, e, e.backtrace());
                for cause in e.causes() {
                    error!("caused by: {}", cause);
                }
            }
        }
    }

    Ok(())
}

pub fn check_rollout_status(
    deployer: &mut kubernetes::KubernetesDeployer,
    deployments: &[Deployable],
) -> Result<(RolloutStatus, HashMap<String, DeploymentState>), Error> {
    let current_state = deployer.retrieve_current_state(deployments)?;

    let combined = current_state
        .iter()
        .map(|(_, v)| v)
        .map(|d| match d {
            DeploymentState::NotDeployed => RolloutStatus::Outdated,
            DeploymentState::Deployed {
                status,
                version,
                expected_version,
            }
                if version == expected_version =>
            {
                status.clone().into()
            }
            DeploymentState::Deployed { status, .. } => {
                RolloutStatus::Outdated.combine(status.clone().into())
            }
        }).fold(RolloutStatus::Clean, RolloutStatus::combine);

    Ok((combined, current_state))
}

pub fn new_deployer_status(version: VersionHash) -> DeployerStatus {
    DeployerStatus {
        deployed_version: version,
        last_successfully_deployed_version: None,
        rollout_status: RolloutStatus::InProgress,
        status_by_deployment: HashMap::new(),
    }
}

pub fn deploy_env(
    version: VersionHash,
    deployer: &mut kubernetes::KubernetesDeployer,
    repo: &Repository,
    env: &str,
    last_version: Option<VersionHash>,
    last_status: Option<DeployerStatus>,
) -> Result<DeployerStatus, Error> {
    let mut env_status = last_status.unwrap_or_else(|| new_deployer_status(version));
    if let Some(deployments) = get_deployments(repo, env, last_version)? {
        info!(
            "Got a change for {} to version {:?}, now deploying...",
            env, version
        );
        deploy(deployer, &deployments.deployments)?;

        env_status.deployed_version = version;
        env_status.rollout_status = RolloutStatus::InProgress;

        info!("Deployed {} up to {:?}", env, version);
    }

    if env_status.rollout_status == RolloutStatus::InProgress {
        if let Some(deployments) =
            get_deployments(&repo, env, env_status.last_successfully_deployed_version)?
        {
            let (new_rollout_status, new_status_by_deployment) =
                check_rollout_status(deployer, &deployments.deployments)?;
            env_status.rollout_status = new_rollout_status;
            env_status
                .status_by_deployment
                .extend(new_status_by_deployment.into_iter());
        }
    }

    if env_status.rollout_status == RolloutStatus::Clean {
        env_status.last_successfully_deployed_version = Some(version);
    }

    Ok(env_status)
}

#[cfg(test)]
mod test {
    use super::*;
    use git_fixture;
    use serde_yaml::Value;
    use std::path::Path;

    #[test]
    fn test_get_deployments_no_deployments() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_deployments_no_deployments.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result = get_deployments(&fixture.repo, "available", None).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().deployments.len(), 0);
    }

    #[test]
    fn test_get_deployments_1() {
        let fixture =
            git_fixture::RepoFixture::from_str(include_str!("./fixtures/get_deployments_1.yaml"))
                .unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result = get_deployments(&fixture.repo, "available", None).unwrap();
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.deployments.len(), 1);
        assert_eq!(info.deployments[0].name, "foo");
        assert_eq!(
            info.deployments[0].base_file_name,
            Path::new("deployable/foo")
        );
        assert_eq!(
            info.deployments[0].merged_content,
            Value::String("blubb".to_owned())
        );
        assert_eq!(
            info.deployments[0].version,
            fixture.get_commit("head").unwrap().into()
        )
    }

    #[test]
    fn test_get_deployments_2() {
        let fixture =
            git_fixture::RepoFixture::from_str(include_str!("./fixtures/get_deployments_2.yaml"))
                .unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result = get_deployments(&fixture.repo, "available", None).unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.deployments.sort_by_key(|d| d.name.clone());
        assert_eq!(info.deployments.len(), 2);
        assert_eq!(info.deployments[0].name, "bar");
        assert_eq!(
            info.deployments[0].base_file_name,
            Path::new("deployable/bar")
        );
        assert_eq!(
            info.deployments[0].merged_content,
            Value::String("xx".to_owned())
        );
        assert_eq!(
            info.deployments[0].version,
            fixture.get_commit("head").unwrap().into()
        );
        assert_eq!(info.deployments[1].name, "foo");
        assert_eq!(
            info.deployments[1].base_file_name,
            Path::new("deployable/foo")
        );
        assert_eq!(
            info.deployments[1].merged_content,
            Value::String("blubb".to_owned())
        );
        assert_eq!(
            info.deployments[1].version,
            fixture.get_commit("head").unwrap().into()
        );
    }

    #[test]
    fn test_get_deployments_separated() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_deployments_separated.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result = get_deployments(&fixture.repo, "available", None).unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.deployments.sort_by_key(|d| d.name.clone());
        assert_eq!(info.deployments.len(), 2);
        assert_eq!(info.deployments[0].name, "nothing");
        assert_eq!(
            info.deployments[0].base_file_name,
            Path::new("base/nothing")
        );
        assert_eq!(
            info.deployments[0].version_file_name,
            Some(PathBuf::from("version/nothing"))
        );
        assert_eq!(
            info.deployments[0].merged_content,
            Value::String("blubb".to_owned())
        );
        assert_eq!(
            info.deployments[0].version,
            fixture.get_commit("head").unwrap().into()
        );
        assert_eq!(info.deployments[1].name, "simple");
        assert_eq!(info.deployments[1].base_file_name, Path::new("base/simple"));
        assert_eq!(
            info.deployments[1].version_file_name,
            Some(PathBuf::from("version/simple"))
        );
        assert_eq!(
            info.deployments[1].merged_content,
            Value::Mapping(
                [(
                    Value::String("the_version_is".to_owned()),
                    Value::String("blubb".to_owned())
                )]
                    .iter()
                    .cloned()
                    .collect()
            )
        );
        assert_eq!(
            info.deployments[1].version,
            fixture.get_commit("head").unwrap().into()
        );
    }

    #[test]
    fn test_get_deployments_subdir() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_deployments_glob.yaml"
        )).unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result = get_deployments(&fixture.repo, "available", None).unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.deployments.sort_by_key(|d| d.name.clone());
        assert_eq!(info.deployments.len(), 3);
        assert_eq!(info.deployments[0].name, "bar");
        assert_eq!(
            info.deployments[0].base_file_name,
            Path::new("deployable/bar")
        );
        assert_eq!(info.deployments[1].name, "baz");
        assert_eq!(
            info.deployments[1].base_file_name,
            Path::new("deployable/subdir/baz")
        );
        assert_eq!(info.deployments[2].name, "blub");
        assert_eq!(
            info.deployments[2].base_file_name,
            Path::new("deployable/othersubdir/blub")
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
        assert_eq!(
            info.deployments[0].merged_content,
            Value::String("yy".to_owned())
        );
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
        assert_eq!(
            info.deployments[0].merged_content,
            Value::String("yy".to_owned())
        );
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
