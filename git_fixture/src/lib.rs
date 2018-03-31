extern crate git2;
#[macro_use]
extern crate failure;
extern crate tempfile;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_yaml;

extern crate common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use failure::Error;
use common::git::TreeZipper;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RepoTemplate {
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
        for commit in self.commits.iter() {
            last_commit = Some(commit.create(&mut repo, &mut commits, last_commit)?);
        }
        // TODO
        Ok(RepoFixture { dir, repo, commits })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Commit {
    files: HashMap<PathBuf, String>,
    name: Option<String>,
    parent: Option<String>,
}

impl Commit {
    fn create(
        &self,
        repo: &mut git2::Repository,
        commits: &mut HashMap<String, git2::Oid>,
        last_commit: Option<git2::Oid>,
    ) -> Result<git2::Oid, Error> {
        let mut files: Vec<&Path> = self.files.keys().map(Path::new).collect();
        files.sort_unstable();
        let tree = {
            let mut zipper = TreeZipper::new(repo);
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

        let parent_commit = self.parent
            .as_ref()
            .and_then(|name| commits.get(name))
            .cloned()
            .or(last_commit)
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
    dir: tempfile::TempDir,
    pub repo: git2::Repository,
    pub commits: HashMap<String, git2::Oid>,
}

impl RepoFixture {
    pub fn from_string(s: &str) -> Result<RepoFixture, Error> {
        RepoTemplate::from_string(s)?.create()
    }
    pub fn set_ref(&self, ref_name: &str, commit_name: &str) -> Result<(), Error> {
        let id = self.get_commit(commit_name)?;
        self.repo.reference(ref_name, id, true, "")?;
        Ok(())
    }
    pub fn get_commit(&self, commit_name: &str) -> Result<git2::Oid, Error> {
        Ok(self.commits
            .get(commit_name)
            .ok_or_else(|| format_err!("named commit not found: {}", commit_name))?
            .clone())
    }
}
