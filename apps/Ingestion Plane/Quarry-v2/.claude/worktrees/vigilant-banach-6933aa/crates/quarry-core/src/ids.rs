//! Typed ULID IDs. Prefix = resource kind.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;
use ulid::Ulid;

use crate::error::{ErrorCode, QuarryError};

pub trait IdKind: 'static {
    const PREFIX: &'static str;
}

macro_rules! id_kinds {
    ($($name:ident => $prefix:literal),* $(,)?) => {
        $(
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub struct $name;
            impl IdKind for $name {
                const PREFIX: &'static str = $prefix;
            }
        )*
        pub mod kinds {
            $( pub type $name = super::Id<super::$name>; )*
        }
    };
}

id_kinds! {
    RunKind       => "run_",
    QueueKind     => "queue_",
    CheckpointKind=> "cp_",
    ScheduleKind  => "sch_",
    StoreKind     => "store_",
    SnapshotKind  => "snap_",
    SourceKind    => "src_",
    ArtifactKind  => "art_",
    LeaseKind     => "lease_",
    ProfileKind   => "prof_",
    JobKind       => "job_",
    EventKind     => "evt_",
    WebhookKind   => "whk_",
    RequestKind   => "req_",
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Id<K: IdKind> {
    ulid: Ulid,
    _kind: PhantomData<K>,
}

impl<K: IdKind> Id<K> {
    pub fn new() -> Self {
        Self {
            ulid: Ulid::new(),
            _kind: PhantomData,
        }
    }

    pub fn from_ulid(ulid: Ulid) -> Self {
        Self {
            ulid,
            _kind: PhantomData,
        }
    }

    pub fn ulid(&self) -> Ulid {
        self.ulid
    }
}

impl<K: IdKind> Default for Id<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: IdKind> fmt::Display for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", K::PREFIX, self.ulid)
    }
}

impl<K: IdKind> FromStr for Id<K> {
    type Err = QuarryError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let rest = s.strip_prefix(K::PREFIX).ok_or_else(|| {
            QuarryError::new(
                ErrorCode::BadRequest,
                format!("id missing prefix {}", K::PREFIX),
            )
        })?;
        let ulid = Ulid::from_string(rest)
            .map_err(|e| QuarryError::new(ErrorCode::BadRequest, format!("invalid ulid: {e}")))?;
        Ok(Self::from_ulid(ulid))
    }
}

impl<K: IdKind> Serialize for Id<K> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de, K: IdKind> Deserialize<'de> for Id<K> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Self::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_run_id() {
        let id: kinds::RunKind = Id::new();
        let s = id.to_string();
        assert!(s.starts_with("run_"));
        let back: kinds::RunKind = s.parse().unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn wrong_prefix_rejected() {
        let s = "queue_01J000000000000000000000";
        let parsed: Result<kinds::RunKind, _> = s.parse();
        assert!(parsed.is_err());
    }
}
