use std::path::Path;

use git2::{Blob, Commit, Repository, Tree, TreeBuilder};
use failure::{Error, ResultExt};

pub fn update(repo: &Repository, url: &str) -> Result<(), Error> {
    let mut remote = repo.remote_anonymous(url)
        .context("creating remote failed")?;

    remote
        .fetch(&["+refs/heads/master:refs/dm_head"], None, None)
        .context("fetch failed")?;

    Ok(())
}

pub fn push(repo: &Repository, url: &str) -> Result<(), Error> {
    let mut remote = repo.remote_anonymous(url)
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
    let head = repo.find_reference("refs/dm_head")
        .context("refs/dm_head not found")?;
    Ok(head.peel_to_commit()?)
}

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

    pub fn into_inner(self) -> Option<Tree<'repo>> {
        self.current
    }

    pub fn descend(&mut self, name: &str) -> Result<(), Error> {
        let next = if let Some(t) = self.current.as_ref().and_then(|t| t.get_name(name)) {
            let tree = t.to_object(self.repo)?
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
}
