use std::collections::HashMap;

use chrono::{DateTime, Utc};
use failure::{Error, ResultExt};
use git2::{Repository, TreeBuilder};
use serde_derive::{Deserialize, Serialize};

use common::git::{self, TreeZipper};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionStates(pub HashMap<String, TransitionState>);

impl Default for TransitionStates {
    fn default() -> TransitionStates {
        TransitionStates(HashMap::new())
    }
}

impl TransitionStates {
    pub fn load(repo: &Repository) -> Result<TransitionStates, Error> {
        let head = git::get_head_commit(repo)?;
        let tree = head.tree()?;

        let zipper = TreeZipper::from(repo, tree);

        let locks_blob = if let Some(blob) = zipper.get_blob("transitions.yaml")? {
            blob
        } else {
            return Ok(TransitionStates::default());
        };

        let result = serde_yaml::from_slice(locks_blob.content())
            .context("deserializing transitions.yaml failed")?;

        Ok(result)
    }

    pub fn save<'repo>(
        &self,
        repo: &'repo Repository,
        tree_builder: &mut TreeBuilder<'repo>,
    ) -> Result<(), Error> {
        if self.0.is_empty() {
            if tree_builder.get("transitions.yaml")?.is_some() {
                tree_builder.remove("transitions.yaml")?;
            }
        } else {
            let mut serialized =
                serde_yaml::to_vec(self).context("serializing transitions.yaml failed")?;
            serialized.extend("\n".as_bytes());

            let blob = repo.blob(&serialized).context("writing blob failed")?;

            tree_builder
                .insert("transitions.yaml", blob, 0o100644)
                .context("updating transitions.yaml failed")?;
        }
        Ok(())
    }

    pub fn insert(&mut self, name: &str, state: TransitionState) {
        if state.is_empty() {
            self.0.remove(name);
        } else {
            self.0.insert(name.to_owned(), state);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionState {
    pub scheduled: Option<DateTime<Utc>>,
}

impl TransitionState {
    fn is_empty(&self) -> bool {
        self.scheduled.is_none()
    }
}

impl Default for TransitionState {
    fn default() -> TransitionState {
        TransitionState { scheduled: None }
    }
}
