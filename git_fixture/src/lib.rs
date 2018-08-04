extern crate git2;
#[macro_use]
extern crate failure;
extern crate serde;
extern crate tempfile;
#[macro_use]
extern crate serde_derive;
extern crate serde_yaml;

extern crate common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use common::git::TreeZipper;
use failure::Error;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepoTemplate {
    commits: Vec<Commit>,
}

impl RepoTemplate {
    pub fn from_string(s: &str) -> Result<RepoTemplate, Error> {
        Ok(serde_yaml::from_str(s)?)
    }
    pub fn create(&self) -> Result<RepoFixture, Error> {
        let dir = tempfile::tempdir()?;
        let mut repo = git2::Repository::init_bare(dir.path())?;
        let mut commits = HashMap::with_capacity(self.commits.len());
        let mut last_commit = None;
        for commit in &self.commits {
            last_commit = Some(commit.create(&mut repo, &mut commits, last_commit)?);
        }
        Ok(RepoFixture {
            dir: Some(dir),
            repo,
            commits,
            template: self.clone(),
        })
    }

    pub fn create_in(&self, path: &Path) -> Result<RepoFixture, Error> {
        let mut repo = git2::Repository::init_bare(path)?;
        let mut commits = HashMap::with_capacity(self.commits.len());
        let mut last_commit = None;
        for commit in &self.commits {
            last_commit = Some(commit.create(&mut repo, &mut commits, last_commit)?);
        }
        Ok(RepoFixture {
            dir: None,
            repo,
            commits,
            template: self.clone(),
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Commit {
    files: HashMap<PathBuf, String>,
    name: Option<String>,
    parent: Option<String>,
}

impl Commit {
    fn create_with_parent(
        &self,
        repo: &git2::Repository,
        parent: Option<git2::Oid>,
        base_tree: Option<git2::Tree>,
    ) -> Result<git2::Oid, Error> {
        let mut files: Vec<&Path> = self.files.keys().map(Path::new).collect();
        files.sort_unstable();
        let tree = {
            let mut zipper = if let Some(t) = base_tree {
                TreeZipper::from(repo, t)
            } else {
                TreeZipper::new(repo)
            };
            let mut current_prefix = PathBuf::new();
            for file in files.iter().rev() {
                let file_dir = if let Some(p) = file.parent() {
                    p
                } else {
                    bail!("empty file in files");
                };
                let file_name = if let Some(f) = file.file_name() {
                    f
                } else {
                    bail!("bad file name: {:?}", file);
                };
                while !file_dir.starts_with(&current_prefix) {
                    current_prefix.pop();
                    zipper.ascend()?;
                }
                let missing = file_dir
                    .strip_prefix(&current_prefix)
                    .expect("strip_prefix failed even though starts_with returned true")
                    .to_owned();
                current_prefix.extend(missing.components());
                for component in missing.components() {
                    match component {
                        ::std::path::Component::Normal(name) => {
                            if let Some(name) = name.to_str() {
                                zipper.descend(name)?;
                            } else {
                                bail!("non-utf8 component in file name: {:?}", name);
                            }
                        }
                        _ => {
                            bail!("unsupported path component in file name: {:?}", component);
                        }
                    }
                }
                let content = self.files.get(*file).expect("file not in files");
                let blob_oid = repo.blob(content.as_bytes())?;
                zipper.rebuild(|b| {
                    b.insert(file_name, blob_oid, 0o100644)?;
                    Ok(())
                })?;
            }
            while !zipper.is_root() {
                zipper.ascend()?;
            }
            zipper.into_inner()
        };
        let tree = if let Some(t) = tree {
            t
        } else {
            empty_tree(repo)?
        };

        let signature = git2::Signature::now("Git Fixture", "n/a")?;

        let parent_commit = parent
            .map(|oid| repo.find_commit(oid))
            .map_or(Ok(None), |r| r.map(Some))?;
        let parent_commits = if let Some(c) = parent_commit.as_ref() {
            vec![c]
        } else {
            vec![]
        };
        let commit = repo.commit(
            None,
            &signature,
            &signature,
            &format!(
                "Commit {}",
                self.name.as_ref().map(|s| &**s).unwrap_or("(unnamed)")
            ),
            &tree,
            &parent_commits,
        )?;
        Ok(commit)
    }
    fn create(
        &self,
        repo: &mut git2::Repository,
        commits: &mut HashMap<String, git2::Oid>,
        last_commit: Option<git2::Oid>,
    ) -> Result<git2::Oid, Error> {
        let parent_commit = self
            .parent
            .as_ref()
            .and_then(|name| commits.get(name))
            .cloned()
            .or(last_commit);
        let commit = self.create_with_parent(repo, parent_commit, None)?;
        if let Some(name) = self.name.as_ref() {
            commits.insert(name.to_string(), commit);
        }
        Ok(commit)
    }
}

fn empty_tree(repo: &git2::Repository) -> Result<git2::Tree, Error> {
    let builder = repo.treebuilder(None)?;
    Ok(repo.find_tree(builder.write()?)?)
}

#[test]
pub fn test_path_order() {
    let mut paths = vec![
        PathBuf::from("a/b"),
        PathBuf::from("a"),
        PathBuf::from("a/z"),
    ];
    paths.sort_unstable();
    assert_eq!(paths, &[Path::new("a"), Path::new("a/b"), Path::new("a/z")]);
}

pub struct RepoFixture {
    #[allow(dead_code)] // it's there to keep the temp dir alive
    dir: Option<tempfile::TempDir>,
    template: RepoTemplate,
    pub repo: git2::Repository,
    pub commits: HashMap<String, git2::Oid>,
}

impl RepoFixture {
    pub fn from_str(s: &str) -> Result<RepoFixture, Error> {
        RepoTemplate::from_string(s)?.create()
    }
    pub fn set_ref(&self, ref_name: &str, commit_name: &str) -> Result<(), Error> {
        let id = self.get_commit(commit_name)?;
        self.repo.reference(ref_name, id, true, "")?;
        Ok(())
    }
    pub fn apply(&self, ref_name: &str, commit_name: &str) -> Result<(), Error> {
        let reference = self.repo.find_reference(ref_name)?;
        let parent = reference.peel_to_commit()?;
        let tree = reference.peel_to_tree()?;
        let commit = self
            .template
            .commits
            .iter()
            .find(|c| c.name.as_ref().map(|s| s == commit_name).unwrap_or(false))
            .ok_or_else(|| format_err!("named commit not found: {}", commit_name))?
            .create_with_parent(&self.repo, Some(parent.id()), Some(tree))?;
        self.repo.reference(ref_name, commit, true, "")?;
        Ok(())
    }
    pub fn get_commit(&self, commit_name: &str) -> Result<git2::Oid, Error> {
        Ok(*self
            .commits
            .get(commit_name)
            .ok_or_else(|| format_err!("named commit not found: {}", commit_name))?)
    }

    pub fn assert_ref_matches(&self, ref_name: &str, commit_name: &str) {
        use std::process::Command;
        let commit_id = self.repo.refname_to_id(ref_name).unwrap();
        let actual_commit = self.repo.find_commit(commit_id).unwrap();
        let expected_commit = self
            .repo
            .find_commit(self.get_commit(commit_name).unwrap())
            .unwrap();

        let actual_parent_ids = actual_commit.parent_ids().collect::<Vec<_>>();
        let expected_parent_ids = expected_commit.parent_ids().collect::<Vec<_>>();
        assert_eq!(
            actual_parent_ids, expected_parent_ids,
            "parent ids did not match"
        );

        let actual_tree = actual_commit.tree_id();
        let expected_tree = expected_commit.tree_id();
        if actual_tree != expected_tree {
            let diff = Command::new("git")
                .args(&[
                    "diff",
                    &format!("{}", expected_tree),
                    &format!("{}", actual_tree),
                ]).current_dir(self.repo.path())
                .output()
                .unwrap()
                .stdout;
            panic!("Trees differed: {}", String::from_utf8_lossy(&diff));
        }
    }
}
