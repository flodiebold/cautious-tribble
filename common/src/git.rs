use std::ffi::OsStr;
use std::fmt;
use std::ops::Range;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use failure::{Error, ResultExt};
use git2::{Blob, Commit, ObjectType, Oid, Repository, Tree, TreeBuilder, TreeEntry};
use serde;

pub fn update(repo: &Repository, url: &str) -> Result<(), Error> {
    let mut remote = repo
        .remote_anonymous(url)
        .context("creating remote failed")?;

    remote
        .fetch(&["+refs/heads/master:refs/dm_head"], None, None)
        .context("fetch failed")?;

    Ok(())
}

pub fn push(repo: &Repository, url: &str) -> Result<(), Error> {
    let mut remote = repo
        .remote_anonymous(url)
        .context("creating remote failed")?;

    // TODO according to git2 documentation: Note that you'll likely want to use
    // RemoteCallbacks and set push_update_reference to test whether all the
    // references were pushed successfully.
    remote
        .push(&["+refs/dm_head:refs/heads/master"], None)
        .context("push failed")?;

    Ok(())
}

pub fn init_or_open(checkout_path: &str) -> Result<Repository, Error> {
    let repo = if Path::new(checkout_path).is_dir() {
        Repository::open(checkout_path).context("open failed")?
    } else {
        Repository::init_bare(checkout_path).context("init --bare failed")?
    };

    Ok(repo)
}

pub fn get_head_commit<'repo>(repo: &'repo Repository) -> Result<Commit<'repo>, Error> {
    let head = repo
        .find_reference("refs/dm_head")
        .context("refs/dm_head not found")?;
    Ok(head.peel_to_commit()?)
}

#[derive(Clone)]
pub struct TreeZipper<'repo> {
    repo: &'repo Repository,
    current: Option<Tree<'repo>>,
    stack: Vec<(Option<Tree<'repo>>, String)>,
}

impl<'repo> TreeZipper<'repo> {
    pub fn from(repo: &'repo Repository, tree: Tree<'repo>) -> TreeZipper<'repo> {
        TreeZipper {
            repo,
            current: Some(tree),
            stack: Vec::new(),
        }
    }

    pub fn new(repo: &'repo Repository) -> TreeZipper<'repo> {
        TreeZipper {
            repo,
            current: None,
            stack: Vec::new(),
        }
    }

    pub fn exists(&self) -> bool {
        self.current.is_some()
    }

    pub fn into_inner(self) -> Option<Tree<'repo>> {
        self.current
    }

    pub fn descend(&mut self, name: &str) -> Result<(), Error> {
        let next = if let Some(t) = self.current.as_ref().and_then(|t| t.get_name(name)) {
            let tree = t
                .to_object(self.repo)?
                .into_tree()
                .or_else(|_| Err(format_err!("expected tree in {}", name)))?;
            Some(tree)
        } else {
            None
        };

        let old = self.current.take();
        self.stack.push((old, name.to_owned()));
        self.current = next;
        Ok(())
    }

    pub fn rebuild<F: FnOnce(&mut TreeBuilder<'repo>) -> Result<(), Error>>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        let mut builder = self.repo.treebuilder(self.current.as_ref())?;
        f(&mut builder)?;
        let result = builder.write()?;
        self.current = Some(self.repo.find_tree(result)?);
        Ok(())
    }

    pub fn is_root(&self) -> bool {
        self.stack.is_empty()
    }

    pub fn ascend(&mut self) -> Result<(), Error> {
        let new_child = self.current.take();
        let (parent, name) = self.stack.pop().expect("ascend called while at the root");

        if let Some(parent) = parent {
            let cur = parent.get_name(&name).map(|e| e.id());
            if cur == new_child.as_ref().map(|t| t.id()) {
                self.current = Some(parent);
            } else {
                let mut builder = self.repo.treebuilder(Some(&parent))?;
                if let Some(t) = new_child {
                    builder.insert(&name, t.id(), 0o040000)?;
                } else {
                    // name must have existed before, otherwise we would not be here
                    builder.remove(&name)?;
                }
                let id = builder.write()?;
                let new_parent = self.repo.find_tree(id)?;
                self.current = Some(new_parent);
            }
        } else if let Some(child_tree) = new_child {
            let mut builder = self.repo.treebuilder(None)?;
            builder.insert(&name, child_tree.id(), 0o040000)?;
            let id = builder.write()?;
            let new_parent = self.repo.find_tree(id)?;
            self.current = Some(new_parent);
        }
        // else nothing to do, keep None as current

        Ok(())
    }

    pub fn get_blob(&self, name: &str) -> Result<Option<Blob<'repo>>, Error> {
        let t = if let Some(t) = self.current.as_ref() {
            t
        } else {
            return Ok(None);
        };

        let entry = if let Some(e) = t.get_name(name) {
            e
        } else {
            return Ok(None);
        };

        let obj = entry.to_object(self.repo)?;

        Ok(Some(obj.into_blob().or_else(|_| {
            Err(format_err!("expected blob in {}", name))
        })?))
    }

    pub fn walk(
        &self,
        include_dirs: bool,
    ) -> impl Iterator<Item = (PathBuf, TreeEntry<'static>)> + 'repo {
        if let Some(tree) = self.current.clone() {
            TreeWalk {
                path: PathBuf::new(),
                repo: self.repo,
                stack: vec![(0..tree.len(), tree)],
                include_dirs,
            }
        } else {
            TreeWalk {
                path: PathBuf::new(),
                repo: self.repo,
                stack: Vec::new(),
                include_dirs,
            }
        }
    }
}

pub struct TreeWalk<'tree> {
    path: PathBuf,
    repo: &'tree Repository,
    stack: Vec<(Range<usize>, Tree<'tree>)>,
    include_dirs: bool,
}

impl<'tree> Iterator for TreeWalk<'tree> {
    type Item = (PathBuf, TreeEntry<'static>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.stack.is_empty() {
                return None;
            }

            let tree = {
                let &mut (ref mut range, ref tree) = self.stack.last_mut().unwrap();
                let i = range.next();
                match i.and_then(|i| tree.get(i)) {
                    None => {
                        self.path.pop();
                        None
                    }
                    Some(entry) => match entry.kind() {
                        Some(ObjectType::Tree) => {
                            self.path.push(OsStr::from_bytes(entry.name_bytes()));
                            let tree = entry
                                .to_object(self.repo)
                                .unwrap()
                                .into_tree()
                                .expect("checked object type");
                            Some((0..tree.len(), tree, entry.to_owned()))
                        }
                        _ => {
                            let mut path = self.path.clone();
                            path.push(OsStr::from_bytes(entry.name_bytes()));
                            return Some((path, entry.to_owned()));
                        }
                    },
                }
            };
            if let Some((range, tree, entry)) = tree {
                self.stack.push((range, tree));
                if self.include_dirs {
                    return Some((self.path.clone(), entry));
                }
            } else {
                self.stack.pop();
            }
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VersionHash(Oid);

impl VersionHash {
    pub fn from_bytes(bytes: &[u8]) -> Result<VersionHash, Error> {
        Ok(VersionHash(Oid::from_bytes(bytes)?))
    }
}

impl serde::Serialize for VersionHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}", self.0))
    }
}

impl<'de> serde::Deserialize<'de> for VersionHash {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_str(VersionHashVisitor)
    }
}

struct VersionHashVisitor;

impl<'de> serde::de::Visitor<'de> for VersionHashVisitor {
    type Value = VersionHash;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a hex-formatted git hash")
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        VersionHash::from_str(v).map_err(|e| E::custom(e))
    }

    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
        VersionHash::from_str(&v).map_err(|e| E::custom(e))
    }
}

impl From<Oid> for VersionHash {
    fn from(oid: Oid) -> VersionHash {
        VersionHash(oid)
    }
}

impl fmt::Display for VersionHash {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        self.0.fmt(fmt)
    }
}

impl FromStr for VersionHash {
    type Err = <Oid as FromStr>::Err;

    fn from_str(s: &str) -> Result<VersionHash, Self::Err> {
        Oid::from_str(s).map(VersionHash)
    }
}
