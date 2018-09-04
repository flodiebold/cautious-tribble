use std::collections::HashMap;

use indexmap::IndexMap;

use deployment::AllDeployerStatus;
use repo::Id;
use transitions::AllTransitionStatus;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceVersion {
    pub version_id: Id,
    pub introduced_in: Id,
    pub version: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceId(pub String);

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EnvName(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceStatus {
    pub name: String,
    pub versions: IndexMap<Id, ResourceVersion>, // TODO serialize as array
    pub base_data: HashMap<EnvName, Id>,
    pub version_by_env: HashMap<EnvName, Id>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change")]
pub enum ResourceRepoChange {
    Version {
        resource: ResourceId,
        #[serde(flatten)]
        version: ResourceVersion,
    },
    Deployable {
        resource: ResourceId,
        env: EnvName,
        content_id: Id,
    },
    BaseData {
        resource: ResourceId,
        env: EnvName,
        content_id: Id,
    },
    VersionDeployed {
        resource: ResourceId,
        env: EnvName,
        version_id: Id,
    }, // TODO existing version deployed to new env (outside of transition)
       // TODO locks/unlocks
       // TODO schedule changes
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRepoCommit {
    pub id: Id,
    pub message: String,
    // TODO add time
    // TODO author
    // TODO transition info, if applicable
    pub changes: Vec<ResourceRepoChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VersionsAnalysis {
    pub resources: HashMap<ResourceId, ResourceStatus>,
    pub history: Vec<ResourceRepoCommit>,
}

impl VersionsAnalysis {
    fn get_resource(&mut self, resource_id: &ResourceId) -> &mut ResourceStatus {
        self.resources
            .entry(resource_id.clone())
            .or_insert_with(|| ResourceStatus {
                name: resource_id.0.clone(),
                versions: Default::default(),
                base_data: Default::default(),
                version_by_env: Default::default(),
            })
    }
    pub fn add_commit(&mut self, commit: ResourceRepoCommit) {
        use self::ResourceRepoChange::*;
        for change in &commit.changes {
            match change {
                Version {
                    resource: resource_id,
                    version,
                } => {
                    let resource = self.get_resource(resource_id);
                    resource
                        .versions
                        .insert(version.version_id, version.clone());
                }
                Deployable {
                    resource: resource_id,
                    env,
                    content_id,
                }
                | BaseData {
                    resource: resource_id,
                    env,
                    content_id,
                } => {
                    let resource = self.get_resource(resource_id);
                    resource.base_data.insert(env.clone(), *content_id);
                }
                VersionDeployed {
                    resource: resource_id,
                    env,
                    version_id,
                } => {
                    let resource = self.get_resource(resource_id);
                    resource.version_by_env.insert(env.clone(), *version_id);
                }
            }
        }
        self.history.push(commit);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FullStatus {
    pub counter: usize,
    #[serde(flatten)]
    pub deployers: AllDeployerStatus,
    pub transitions: AllTransitionStatus,
    #[serde(flatten)]
    pub analysis: VersionsAnalysis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    FullStatus(FullStatus),
    DeployerStatus {
        counter: usize,
        #[serde(flatten)]
        content: AllDeployerStatus,
    },
    TransitionStatus {
        counter: usize,
        transitions: AllTransitionStatus,
    },
    // TODO only send analyzed versions repo commits, and reconstruct full data client-side
    // -- except for the first message
    // TODO don't send full repo history unless needed
    Versions {
        counter: usize,
        #[serde(flatten)]
        analysis: VersionsAnalysis,
    },
}
