use std::str::FromStr;

use serde;
use git2;
use failure::Error;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VersionHash(git2::Oid);

impl VersionHash {
    pub fn from_bytes(bytes: &[u8]) -> Result<VersionHash, Error> {
        Ok(VersionHash(git2::Oid::from_bytes(bytes)?))
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

impl From<git2::Oid> for VersionHash {
    fn from(oid: git2::Oid) -> VersionHash {
        VersionHash(oid)
    }
}

impl ::std::fmt::Display for VersionHash {
    fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> Result<(), ::std::fmt::Error> {
        self.0.fmt(fmt)
    }
}

impl FromStr for VersionHash {
    type Err = <git2::Oid as FromStr>::Err;

    fn from_str(s: &str) -> Result<VersionHash, Self::Err> {
        git2::Oid::from_str(s).map(VersionHash)
    }
}
