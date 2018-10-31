use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use failure::Error;
use regex;
use serde_json;
use serde_yaml;

use common::deployment::{DeployerStatus, ResourceState, RolloutStatus};
use common::repo::{Id, ResourceRepo};

pub mod kubernetes;
pub mod mock;

#[derive(Debug, PartialEq, Clone)]
pub struct Resource {
    pub name: String,
    pub base_file_name: PathBuf,
    pub version_file_name: Option<PathBuf>,
    pub version_content: BTreeMap<String, String>,
    pub merged_content: serde_json::Value,
    pub version: Id,
    pub message: String,
}

#[derive(Debug, PartialEq, Clone)]
pub struct ResourcesInfo {
    pub resources: Vec<Resource>,
}

pub trait Deployer {
    fn retrieve_current_state(
        &mut self,
        resources: &[Resource],
    ) -> Result<HashMap<String, ResourceState>, Error>;

    fn deploy(&mut self, resource: &Resource) -> Result<(), Error>;
}

impl Deployer for Box<dyn Deployer> {
    fn retrieve_current_state(
        &mut self,
        resources: &[Resource],
    ) -> Result<HashMap<String, ResourceState>, Error> {
        (**self).retrieve_current_state(resources)
    }

    fn deploy(&mut self, resource: &Resource) -> Result<(), Error> {
        (**self).deploy(resource)
    }
}

pub fn get_resources(
    repo: &impl ResourceRepo,
    env: &str,
    last_version: Option<Id>,
) -> Result<Option<ResourcesInfo>, Error> {
    let mut resources = HashMap::<String, Resource>::new();

    // collect current versions of all resources
    let current_version = repo.version();

    if last_version
        .map(|id| id == current_version)
        .unwrap_or(false)
    {
        return Ok(None);
    }

    let env_path = &Path::new(env);

    repo.walk(&env_path.join("deployable"), |entry| {
        // FIXME don't fail everything if content is invalid, report the
        // resource as errored
        let content = serde_yaml::from_slice(&entry.content)?;
        let name = entry
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format_err!("Invalid file name {:?}", entry.path))?
            .to_string();
        let resource = Resource {
            name: name.clone(),
            base_file_name: env_path.join("deployable").join(entry.path),
            merged_content: content,
            version_content: BTreeMap::new(),
            version_file_name: None,
            version: entry.last_change,
            message: entry.change_message,
        };

        resources.insert(name, resource);
        Ok(())
    })?;

    repo.walk(&env_path.join("version"), |entry| {
        // FIXME don't fail everything if content is invalid
        let content = serde_yaml::from_slice(&entry.content)?;
        let name = entry
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format_err!("Invalid file name {:?}", entry.path))?
            .to_string();
        let base_file_name = env_path.join("base").join(&entry.path);
        let base_file_content = match repo.get(&base_file_name) {
            Ok(Some(content)) => content,
            Ok(None) => {
                // FIXME report error (base file not found)
                eprintln!("error: base file {:?} not found", base_file_name);
                return Ok(());
            }
            Err(e) => bail!(e),
        };
        let base_file_content = serde_yaml::from_slice(&base_file_content)?;
        let resource = Resource {
            name: name.clone(),
            base_file_name,
            merged_content: merge_resource(base_file_content, &content),
            version_content: content,
            version_file_name: Some(env_path.join("version").join(&entry.path)),
            version: entry.last_change,
            message: entry.change_message,
        };

        resources.insert(name, resource);
        Ok(())
    })?;

    let result = ResourcesInfo {
        resources: resources.into_iter().map(|(_, v)| v).collect(),
    };

    Ok(Some(result))
}

fn merge_resource(
    mut base: serde_json::Value,
    version_content: &BTreeMap<String, String>,
) -> serde_json::Value {
    use serde_json::*;
    // TODO rewrite everything about this
    fn merge_mut(base: &mut Value, version_content: &BTreeMap<String, String>) {
        match base {
            Value::String(s) => {
                let regex = regex::Regex::new("\\$version").unwrap();
                let replaced = regex
                    .replace_all(&s, move |_cap: &regex::Captures<'_>| {
                        version_content.get("version").cloned().unwrap_or_default()
                    })
                    .into_owned();
                *s = replaced;
            }
            Value::Array(s) => {
                for element in s {
                    merge_mut(element, version_content);
                }
            }
            Value::Object(m) => {
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

pub fn deploy(deployer: &mut impl Deployer, resources: &[Resource]) -> Result<(), Error> {
    let current_state = deployer.retrieve_current_state(resources)?;

    for d in resources {
        debug!("looking at {}", d.name);
        let deployed_version = if let Some(v) = current_state.get(&d.name) {
            v.clone()
        } else {
            warn!("no known version for {}, not deploying", d.name);
            continue;
        };

        if let ResourceState::Deployed { version, .. } = deployed_version {
            if version == d.version {
                info!("same version for {}, not deploying", d.name);
                continue;
            }
        }

        info!(
            "Deploying {} version {} with content {}",
            d.name,
            d.version,
            serde_json::to_string(&d.merged_content).unwrap_or_default() // FIXME
        );

        match deployer.deploy(d) {
            Ok(()) => {}
            Err(e) => {
                // TODO: maybe instead mark the service as failing to deploy
                // and don't try again?
                error!("Deployment of {} failed: {}\n{}", d.name, e, e.backtrace());
                for cause in e.iter_causes() {
                    error!("caused by: {}", cause);
                }
            }
        }
    }

    Ok(())
}

pub fn check_rollout_status(
    deployer: &mut impl Deployer,
    resources: &[Resource],
) -> Result<(RolloutStatus, HashMap<String, ResourceState>), Error> {
    let current_state = deployer.retrieve_current_state(resources)?;

    let combined = current_state
        .iter()
        .map(|(_, v)| v)
        .map(|d| match d {
            ResourceState::NotDeployed => RolloutStatus::Outdated,
            ResourceState::Deployed {
                status,
                version,
                expected_version,
            } if version == expected_version => status.clone().into(),
            ResourceState::Deployed { status, .. } => {
                RolloutStatus::Outdated.combine(status.clone().into())
            }
        })
        .fold(RolloutStatus::Clean, RolloutStatus::combine);

    Ok((combined, current_state))
}

pub fn new_deployer_status(version: Id) -> DeployerStatus {
    DeployerStatus {
        deployed_version: version,
        last_successfully_deployed_version: None,
        rollout_status: RolloutStatus::InProgress,
        status_by_resource: HashMap::new(),
    }
}

pub fn deploy_env(
    deployer: &mut impl Deployer,
    repo: &impl ResourceRepo,
    env: &str,
    last_version: Option<Id>,
    last_status: Option<DeployerStatus>,
) -> Result<DeployerStatus, Error> {
    let version = repo.version();
    let mut env_status = last_status.unwrap_or_else(|| new_deployer_status(version));
    if let Some(resources) = get_resources(repo, env, last_version)? {
        info!(
            "Got a change for {} to version {:?}, now deploying...",
            env, version
        );
        deploy(deployer, &resources.resources)?;

        env_status.deployed_version = version;
        env_status.rollout_status = RolloutStatus::InProgress;

        info!("Deployed {} up to {:?}", env, version);
    }

    if env_status.rollout_status == RolloutStatus::InProgress {
        if let Some(resources) =
            get_resources(repo, env, env_status.last_successfully_deployed_version)?
        {
            let (new_rollout_status, new_status_by_resource) =
                check_rollout_status(deployer, &resources.resources)?;
            env_status.rollout_status = new_rollout_status;
            env_status
                .status_by_resource
                .extend(new_status_by_resource.into_iter());
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
    use common::repo;
    use git_fixture;
    use std::path::Path;

    fn make_resource_repo(git_fixture: git_fixture::RepoFixture, head: &str) -> impl ResourceRepo {
        let head = git_fixture.get_commit(head).unwrap();
        let (repo, tempdir) = git_fixture.into_inner();
        let inner = repo::GitResourceRepo::from_repo(repo, head, String::new());
        repo::GitResourceRepoWithTempDir { inner, tempdir }
    }

    #[test]
    fn test_get_resources_no_resources() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_resources_no_resources.yaml"
        ))
        .unwrap();
        let result =
            get_resources(&make_resource_repo(fixture, "head"), "available", None).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().resources.len(), 0);
    }

    #[test]
    fn test_get_resources_1() {
        let fixture =
            git_fixture::RepoFixture::from_str(include_str!("./fixtures/get_resources_1.yaml"))
                .unwrap();
        let head = repo::oid_to_id(fixture.get_commit("head").unwrap());
        let result =
            get_resources(&make_resource_repo(fixture, "head"), "available", None).unwrap();
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.resources.len(), 1);
        assert_eq!(info.resources[0].name, "foo");
        assert_eq!(
            info.resources[0].base_file_name,
            Path::new("available/deployable/foo")
        );
        assert_eq!(info.resources[0].merged_content, json!("blubb"));
        assert_eq!(info.resources[0].version, head)
    }

    #[test]
    fn test_get_resources_2() {
        let fixture =
            git_fixture::RepoFixture::from_str(include_str!("./fixtures/get_resources_2.yaml"))
                .unwrap();
        let head = repo::oid_to_id(fixture.get_commit("head").unwrap());
        let result =
            get_resources(&make_resource_repo(fixture, "head"), "available", None).unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.resources.sort_by_key(|d| d.name.clone());
        assert_eq!(info.resources.len(), 2);
        assert_eq!(info.resources[0].name, "bar");
        assert_eq!(
            info.resources[0].base_file_name,
            Path::new("available/deployable/bar")
        );
        assert_eq!(info.resources[0].merged_content, json!("xx"));
        assert_eq!(info.resources[0].version, head);
        assert_eq!(info.resources[1].name, "foo");
        assert_eq!(
            info.resources[1].base_file_name,
            Path::new("available/deployable/foo")
        );
        assert_eq!(info.resources[1].merged_content, json!("blubb"));
        assert_eq!(info.resources[1].version, head);
    }

    #[test]
    fn test_get_resources_separated() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_resources_separated.yaml"
        ))
        .unwrap();
        let head = repo::oid_to_id(fixture.get_commit("head").unwrap());
        let result =
            get_resources(&make_resource_repo(fixture, "head"), "available", None).unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.resources.sort_by_key(|d| d.name.clone());
        assert_eq!(info.resources.len(), 2);
        assert_eq!(info.resources[0].name, "nothing");
        assert_eq!(
            info.resources[0].base_file_name,
            Path::new("available/base/nothing")
        );
        assert_eq!(
            info.resources[0].version_file_name,
            Some(PathBuf::from("available/version/nothing"))
        );
        assert_eq!(info.resources[0].merged_content, json!("blubb"));
        assert_eq!(info.resources[0].version, head);
        assert_eq!(info.resources[1].name, "simple");
        assert_eq!(
            info.resources[1].base_file_name,
            Path::new("available/base/simple")
        );
        assert_eq!(
            info.resources[1].version_file_name,
            Some(PathBuf::from("available/version/simple"))
        );
        assert_eq!(
            info.resources[1].merged_content,
            json!({
                "the_version_is": "blubb"
            })
        );
        assert_eq!(info.resources[1].version, head);
    }

    #[test]
    fn test_get_resources_subdir() {
        let fixture =
            git_fixture::RepoFixture::from_str(include_str!("./fixtures/get_resources_glob.yaml"))
                .unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let result =
            get_resources(&make_resource_repo(fixture, "head"), "available", None).unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.resources.sort_by_key(|d| d.name.clone());
        assert_eq!(info.resources.len(), 3);
        assert_eq!(info.resources[0].name, "bar");
        assert_eq!(
            info.resources[0].base_file_name,
            Path::new("available/deployable/bar")
        );
        assert_eq!(info.resources[1].name, "baz");
        assert_eq!(
            info.resources[1].base_file_name,
            Path::new("available/deployable/subdir/baz")
        );
        assert_eq!(info.resources[2].name, "blub");
        assert_eq!(
            info.resources[2].base_file_name,
            Path::new("available/deployable/othersubdir/blub")
        );
    }

    #[test]
    fn test_get_resources_changed() {
        let fixture = git_fixture::RepoFixture::from_str(include_str!(
            "./fixtures/get_resources_changed.yaml"
        ))
        .unwrap();
        fixture.set_ref("refs/dm_head", "head").unwrap();
        let first = repo::oid_to_id(fixture.get_commit("first").unwrap());
        let head = repo::oid_to_id(fixture.get_commit("head").unwrap());
        let result = get_resources(
            &make_resource_repo(fixture, "head"),
            "available",
            Some(first),
        )
        .unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.resources.sort_by_key(|d| d.name.clone());
        assert_eq!(info.resources.len(), 2);
        assert_eq!(info.resources[0].name, "bar");
        assert_eq!(info.resources[0].merged_content, json!("yy"));
        assert_eq!(info.resources[0].version, head);
        assert_eq!(info.resources[1].name, "foo");
        assert_eq!(info.resources[1].version, first);
    }

    #[test]
    fn test_get_resources_added() {
        let fixture =
            git_fixture::RepoFixture::from_str(include_str!("./fixtures/get_resources_added.yaml"))
                .unwrap();
        let first = repo::oid_to_id(fixture.get_commit("first").unwrap());
        let head = repo::oid_to_id(fixture.get_commit("head").unwrap());
        let result = get_resources(
            &make_resource_repo(fixture, "head"),
            "available",
            Some(first),
        )
        .unwrap();
        assert!(result.is_some());
        let mut info = result.unwrap();
        info.resources.sort_by_key(|d| d.name.clone());
        assert_eq!(info.resources.len(), 2);
        assert_eq!(info.resources[0].name, "bar");
        assert_eq!(info.resources[0].merged_content, json!("yy"));
        assert_eq!(info.resources[0].version, head);
        assert_eq!(info.resources[1].name, "foo");
        assert_eq!(info.resources[1].version, first);
    }
}
