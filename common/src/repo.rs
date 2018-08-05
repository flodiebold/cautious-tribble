use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use failure::Error;
use serde;

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Id(pub [u8; 20]);

impl FromStr for Id {
    type Err = Error;

    fn from_str(s: &str) -> Result<Id, Self::Err> {
        let oid = Oid::from_str(s)?;
        Ok(oid_to_id(oid))
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        for b in &self.0 {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

impl serde::Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Id {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_str(IdVisitor)
    }
}

struct IdVisitor;

impl<'de> serde::de::Visitor<'de> for IdVisitor {
    type Value = Id;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a hex-formatted git hash")
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Id::from_str(v).map_err(E::custom)
    }

    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
        Id::from_str(&v).map_err(E::custom)
    }
}

#[derive(Clone, Debug)]
pub struct ResourceRepoEntry {
    pub path: PathBuf,
    pub content: Vec<u8>,
    pub last_change: Id,
    pub change_message: String,
}

pub trait ResourceRepo {
    fn update(&mut self) -> Result<(), Error>;
    fn version(&self) -> Id;
    fn get(&self, path: &Path) -> Result<Option<Vec<u8>>, Error>;
    fn walk<F: FnMut(ResourceRepoEntry) -> Result<(), Error>>(
        &self,
        path: &Path,
        f: F,
    ) -> Result<(), Error>;
}

use super::git;
use git2::{Commit, ErrorCode, Oid, Repository};

pub struct GitResourceRepo {
    repo: Repository,
    head: Oid,
    remote_url: String,
}

impl GitResourceRepo {
    pub fn open(checkout_path: &str, remote_url: String) -> Result<GitResourceRepo, Error> {
        let repo = git::init_or_open(checkout_path)?;
        git::update(&repo, &remote_url)?;
        let head = git::get_head_commit(&repo)?.id();
        Ok(GitResourceRepo {
            repo,
            head,
            remote_url,
        })
    }

    pub fn from_repo(repo: Repository, head: Oid, remote_url: String) -> GitResourceRepo {
        GitResourceRepo {
            repo,
            head,
            remote_url,
        }
    }
}

impl ResourceRepo for GitResourceRepo {
    fn update(&mut self) -> Result<(), Error> {
        git::update(&self.repo, &self.remote_url)?;
        self.head = git::get_head_commit(&self.repo)?.id();
        Ok(())
    }

    fn version(&self) -> Id {
        oid_to_id(self.head)
    }

    fn get(&self, path: &Path) -> Result<Option<Vec<u8>>, Error> {
        let tree = self.repo.find_commit(self.head)?.tree()?;

        let entry = match tree.get_path(path) {
            Ok(entry) => entry,
            Err(ref e) if e.code() == ErrorCode::NotFound => {
                return Ok(None);
            }
            Err(e) => bail!(e),
        };

        let obj = entry.to_object(&self.repo)?;

        if let Some(blob) = obj.as_blob() {
            Ok(Some(blob.content().to_vec()))
        } else {
            Ok(None)
        }
    }

    fn walk<F: FnMut(ResourceRepoEntry) -> Result<(), Error>>(
        &self,
        base_path: &Path,
        mut f: F,
    ) -> Result<(), Error> {
        let tree = self.repo.find_commit(self.head)?.tree()?;

        let mut zipper = git::TreeZipper::from(&self.repo, tree);
        for component in base_path {
            zipper.descend(
                component
                    .to_str()
                    .ok_or_else(|| format_err!("invalid utf8 in path"))?,
            )?;
        }

        for (path, entry) in zipper.walk(false) {
            let entry = entry?;

            let obj = entry.to_object(&self.repo)?;

            let content = if let Some(blob) = obj.as_blob() {
                blob.content().to_vec()
            } else {
                continue;
            };

            let full_path = base_path.join(&path);

            let last_change_commit = determine_last_change(&self.repo, self.head, &full_path)?;

            let last_change = oid_to_id(last_change_commit.id());
            let change_message = last_change_commit
                .message()
                .unwrap_or("[invalid utf8]")
                .to_string();

            let repo_entry = ResourceRepoEntry {
                path,
                content,
                last_change,
                change_message,
            };

            f(repo_entry)?;
        }

        Ok(())
    }
}

fn determine_last_change<'repo>(
    repo: &'repo Repository,
    commit: Oid,
    path: &Path,
) -> Result<Commit<'repo>, Error> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push(commit)?;

    let mut last = None;

    for rev_result in revwalk {
        let commit = repo.find_commit(rev_result?)?;
        let tree = commit.tree()?;
        let oid = match tree.get_path(path) {
            Ok(entry) => entry.id(),
            Err(ref e) if e.code() == ErrorCode::NotFound => {
                return last
                    .map(|(_, commit)| commit)
                    .ok_or_else(|| format_err!("file not found: {:?}", path));
            }
            Err(e) => bail!(e),
        };

        if let Some((last_id, last_commit)) = last {
            if last_id != oid {
                return Ok(last_commit);
            }
        }

        last = Some((oid, commit));
    }

    last.map(|(_, commit)| commit)
        .ok_or_else(|| format_err!("no commits found"))
}

pub fn oid_to_id(oid: Oid) -> Id {
    let mut id = Id([0; 20]);
    id.0.copy_from_slice(oid.as_bytes());
    id
}

/// A helper to keep a temp dir alive as long as a GitResourceRepo using it.
pub struct GitResourceRepoWithTempDir {
    pub inner: GitResourceRepo,
    pub tempdir: Option<::tempfile::TempDir>,
}

impl ResourceRepo for GitResourceRepoWithTempDir {
    fn update(&mut self) -> Result<(), Error> {
        self.inner.update()
    }
    fn version(&self) -> Id {
        self.inner.version()
    }
    fn get(&self, path: &Path) -> Result<Option<Vec<u8>>, Error> {
        self.inner.get(path)
    }
    fn walk<F: FnMut(ResourceRepoEntry) -> Result<(), Error>>(
        &self,
        path: &Path,
        f: F,
    ) -> Result<(), Error> {
        self.inner.walk(path, f)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use git_fixture;

    fn make_resource_repo(
        git_fixture: git_fixture::RepoFixture,
        head: &str,
    ) -> GitResourceRepoWithTempDir {
        let head = git_fixture.get_commit(head).unwrap();
        let (repo, tempdir) = git_fixture.into_inner();
        let inner = GitResourceRepo::from_repo(repo, head, String::new());
        GitResourceRepoWithTempDir { inner, tempdir }
    }

    #[test]
    fn test_get_resource() {
        let fixture =
            git_fixture::RepoFixture::from_str(include_str!("./fixtures/test_repo.yaml")).unwrap();
        let repo = make_resource_repo(fixture, "head");

        let s1 = String::from_utf8(
            repo.get(Path::new("a/b/1"))
                .expect("no error")
                .expect("file exists"),
        ).unwrap();
        assert_eq!(s1, "yy");

        let s2 = String::from_utf8(
            repo.get(Path::new("a/b/2"))
                .expect("no error")
                .expect("file exists"),
        ).unwrap();
        assert_eq!(s2, "blubb");
    }

    #[test]
    fn test_walk_root() {
        let fixture =
            git_fixture::RepoFixture::from_str(include_str!("./fixtures/test_repo.yaml")).unwrap();
        let first = fixture.get_commit("first").unwrap();
        let head = fixture.get_commit("head").unwrap();
        let repo = make_resource_repo(fixture, "head");

        let mut found = Vec::new();

        repo.walk(Path::new(""), |e| {
            found.push(e);
            Ok(())
        }).unwrap();
        found.sort_by_key(|e| e.path.clone());
        assert_eq!(found.len(), 4);

        assert_eq!(found[0].path, Path::new("a/b/1"));
        assert_eq!(found[0].content, "yy".as_bytes());
        assert_eq!(found[0].last_change, oid_to_id(head));
        assert_eq!(found[0].change_message, "Commit head");

        assert_eq!(found[1].path, Path::new("a/b/2"));
        assert_eq!(found[1].content, "blubb".as_bytes());
        assert_eq!(found[1].last_change, oid_to_id(first));
        assert_eq!(found[1].change_message, "Commit first");

        assert_eq!(found[2].path, Path::new("a/c"));
        assert_eq!(found[2].content, "c".as_bytes());
        assert_eq!(found[2].last_change, oid_to_id(head));

        assert_eq!(found[3].path, Path::new("x/y"));
    }

    #[test]
    fn test_walk_subdir() {
        let fixture =
            git_fixture::RepoFixture::from_str(include_str!("./fixtures/test_repo.yaml")).unwrap();
        let repo = make_resource_repo(fixture, "head");

        let mut found = Vec::new();

        repo.walk(Path::new("a"), |e| {
            found.push(e);
            Ok(())
        }).unwrap();
        found.sort_by_key(|e| e.path.clone());
        assert_eq!(found.len(), 3);

        assert_eq!(found[0].path, Path::new("b/1"));
        assert_eq!(found[1].path, Path::new("b/2"));
        assert_eq!(found[2].path, Path::new("c"));

        found.clear();

        repo.walk(Path::new("a/b"), |e| {
            found.push(e);
            Ok(())
        }).unwrap();
        found.sort_by_key(|e| e.path.clone());
        assert_eq!(found.len(), 2);

        assert_eq!(found[0].path, Path::new("1"));
        assert_eq!(found[1].path, Path::new("2"));
    }
}
