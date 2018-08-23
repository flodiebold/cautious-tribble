use std::collections::HashMap;

use indexmap::IndexMap;

use deployment::AllDeployerStatus;
use repo::Id;
use transitions::AllTransitionStatus;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceVersion {
    pub id: Id,
    pub introduced_in: Id,
    pub version: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceStatus {
    pub name: String,
    pub versions: IndexMap<Id, ResourceVersion>, // TODO serialize as array
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change")]
pub enum ResourceRepoChange {
    NewVersion {
        resource: ResourceId,
        #[serde(flatten)]
        version: ResourceVersion,
    }, // TODO new deployable resource
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRepoCommit {
    pub id: Id,
    pub changes: Vec<ResourceRepoChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VersionsAnalysis {
    pub resources: HashMap<ResourceId, ResourceStatus>,
    pub history: Vec<ResourceRepoCommit>,
}

impl VersionsAnalysis {
    pub fn add_commit(&mut self, commit: ResourceRepoCommit) {
        use self::ResourceRepoChange::*;
        for change in &commit.changes {
            match change {
                NewVersion {
                    resource: resource_id,
                    version,
                } => {
                    let resource = self
                        .resources
                        .entry(resource_id.clone())
                        .or_insert_with(|| ResourceStatus {
                            name: resource_id.0.clone(),
                            versions: Default::default(),
                        });
                    resource.versions.insert(version.id, version.clone());
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
        analysis: VersionsAnalysis,
    },
}
