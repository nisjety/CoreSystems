//! Row-level storage for profile payloads (WS17.E1).
//!
//! `ProfileBlobStore` is the storage layer under `EnvelopeProfileStore`. It
//! holds either the legacy plaintext JSON (`BlobPayload::Legacy`, modes off and
//! compat) or an encrypted envelope (`BlobPayload::Envelope`, mode on). The
//! ordering rules of WS17.P1 (saved_at, rev, tombstones) are the same for both
//! payload kinds and are enforced by the store, not by the callers.
//!
//! `InMemoryBlobStore` is the dev and test implementation. The Postgres
//! implementation is `postgres_profile_store::PostgresProfileBlobStore`, and
//! `cached_profile_store::CachedBlobStore` wraps either one.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use quarry_browser::session::{
    clamp_saved_at, ProfileMetadata, ProfileSummary, SaveMeta, SaveOutcome,
};
use quarry_core::error::QuarryResult;
use quarry_core::ids::kinds::ProfileKind;

/// What a save writes.
#[derive(Debug, Clone, PartialEq)]
pub enum BlobPayload {
    /// Plaintext snapshot JSON, in the legacy `snapshot` column. Modes off and compat.
    Legacy { json: serde_json::Value },
    /// Envelope bytes (`EnvelopeV1` JSON) in the `envelope` column. The legacy
    /// column is written NULL. Mode on.
    Envelope {
        envelope: Vec<u8>,
        enc_v: i16,
        key_id: String,
    },
}

/// One stored row, as the envelope layer reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct BlobRow {
    pub envelope: Option<Vec<u8>>,
    pub legacy_json: Option<serde_json::Value>,
    pub saved_at: Option<DateTime<Utc>>,
    pub rev: i64,
    pub enc_v: Option<i16>,
    pub key_id: Option<String>,
}

impl BlobRow {
    /// Whether the payload is an envelope (true) or legacy plaintext (false).
    pub fn is_envelope(&self) -> bool {
        self.envelope.is_some()
    }
}

/// A plaintext (legacy) row as the WS17.E3 backfill reads it. Counts and ids only
/// leave the backfill; the JSON is sealed and never logged.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaintextRow {
    pub org_id: String,
    pub profile_id: ProfileKind,
    pub rev: i64,
    pub json: serde_json::Value,
}

/// Rows and tombstones removed by one org purge (WS17.E3 erasure).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BlobPurge {
    pub rows: u64,
    pub tombstones: u64,
}

/// Row storage for profile payloads, metadata and tombstones.
#[async_trait]
pub trait ProfileBlobStore: Send + Sync {
    /// Ordered conditional write. Same rules as `ProfileStore::save`: a save
    /// that is tombstoned or stale writes nothing and reports why.
    async fn put(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        payload: BlobPayload,
        meta: &SaveMeta,
    ) -> QuarryResult<SaveOutcome>;

    /// Replace the payload with an envelope, only if the row is still at
    /// `expected_rev`. Does not bump `rev` and does not move `saved_at`.
    /// Returns `false` when the row moved on (lost race) or is gone.
    async fn reseal(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        expected_rev: i64,
        envelope: Vec<u8>,
        enc_v: i16,
        key_id: &str,
    ) -> QuarryResult<bool>;

    /// The row, if any.
    async fn get(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<Option<BlobRow>>;

    /// Remove the row and write a tombstone.
    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()>;

    /// Whether a row exists.
    async fn exists(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<bool>;

    /// The org's rows with metadata attached.
    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>>;

    /// Upsert name, scope and owner. Never touches the payload.
    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
    ) -> QuarryResult<()>;

    /// Name, scope and owner of the row, if any.
    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>>;

    /// One page of plaintext rows (legacy JSON set, no envelope), ordered by
    /// (org_id, profile_id) strictly after after. WS17.E3 backfill only.
    async fn plaintext_page(
        &self,
        after: Option<&(String, String)>,
        limit: usize,
    ) -> QuarryResult<Vec<PlaintextRow>>;

    /// Delete every row and tombstone of one org. Idempotent; counts only.
    async fn purge_org(&self, org_id: &str) -> QuarryResult<BlobPurge>;
}

#[derive(Debug, Clone)]
struct MemRow {
    payload: BlobPayload,
    saved_at: Option<DateTime<Utc>>,
    rev: i64,
    metadata: ProfileMetadata,
}

#[derive(Debug, Default)]
struct MemState {
    rows: HashMap<String, MemRow>,
    tombstones: HashMap<String, DateTime<Utc>>,
}

/// In-memory `ProfileBlobStore`. Dev and tests only: nothing survives a restart,
/// and tombstones never expire.
#[derive(Debug, Default)]
pub struct InMemoryBlobStore {
    state: RwLock<MemState>,
}

impl InMemoryBlobStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(org_id: &str, profile_id: &ProfileKind) -> String {
        // Same NUL separator as InMemoryProfileStore: no cross-tenant collisions.
        format!("{org_id}\0{profile_id}")
    }
}

fn payload_to_row(row: &MemRow) -> BlobRow {
    match &row.payload {
        BlobPayload::Legacy { json } => BlobRow {
            envelope: None,
            legacy_json: Some(json.clone()),
            saved_at: row.saved_at,
            rev: row.rev,
            enc_v: None,
            key_id: None,
        },
        BlobPayload::Envelope {
            envelope,
            enc_v,
            key_id,
        } => BlobRow {
            envelope: Some(envelope.clone()),
            legacy_json: None,
            saved_at: row.saved_at,
            rev: row.rev,
            enc_v: Some(*enc_v),
            key_id: Some(key_id.clone()),
        },
    }
}

#[async_trait]
impl ProfileBlobStore for InMemoryBlobStore {
    async fn put(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        payload: BlobPayload,
        meta: &SaveMeta,
    ) -> QuarryResult<SaveOutcome> {
        let key = Self::key(org_id, profile_id);
        let saved_at = clamp_saved_at(meta.saved_at, Utc::now());
        let mut st = self.state.write().await;
        if st
            .tombstones
            .get(&key)
            .is_some_and(|deleted_at| *deleted_at >= saved_at)
        {
            return Ok(SaveOutcome::IgnoredByTombstone);
        }
        match st.rows.get_mut(&key) {
            Some(row) => {
                if row.saved_at.is_some_and(|s| s >= saved_at) {
                    return Ok(SaveOutcome::IgnoredStale);
                }
                row.payload = payload;
                row.saved_at = Some(saved_at);
                row.rev += 1;
                Ok(SaveOutcome::Stored { rev: row.rev })
            }
            None => {
                st.rows.insert(
                    key,
                    MemRow {
                        payload,
                        saved_at: Some(saved_at),
                        rev: 1,
                        metadata: ProfileMetadata {
                            owner: meta.owner.clone(),
                            ..ProfileMetadata::default()
                        },
                    },
                );
                Ok(SaveOutcome::Stored { rev: 1 })
            }
        }
    }

    async fn reseal(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        expected_rev: i64,
        envelope: Vec<u8>,
        enc_v: i16,
        key_id: &str,
    ) -> QuarryResult<bool> {
        let key = Self::key(org_id, profile_id);
        let mut st = self.state.write().await;
        match st.rows.get_mut(&key) {
            Some(row) if row.rev == expected_rev => {
                row.payload = BlobPayload::Envelope {
                    envelope,
                    enc_v,
                    key_id: key_id.to_string(),
                };
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn get(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<Option<BlobRow>> {
        let st = self.state.read().await;
        Ok(st
            .rows
            .get(&Self::key(org_id, profile_id))
            .map(payload_to_row))
    }

    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
        let key = Self::key(org_id, profile_id);
        let mut st = self.state.write().await;
        st.rows.remove(&key);
        let now = Utc::now();
        let entry = st.tombstones.entry(key).or_insert(now);
        if *entry < now {
            *entry = now;
        }
        Ok(())
    }

    async fn exists(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<bool> {
        let st = self.state.read().await;
        Ok(st.rows.contains_key(&Self::key(org_id, profile_id)))
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>> {
        let prefix = format!("{org_id}\0");
        let st = self.state.read().await;
        let mut out = Vec::new();
        for (key, row) in st.rows.iter() {
            let Some(id) = key.strip_prefix(&prefix) else {
                continue;
            };
            if let Ok(profile_id) = id.parse::<ProfileKind>() {
                out.push(ProfileSummary {
                    profile_id,
                    name: row.metadata.name.clone(),
                    scope: row.metadata.scope,
                    owner: row.metadata.owner.clone(),
                });
            }
        }
        Ok(out)
    }

    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
    ) -> QuarryResult<()> {
        let key = Self::key(org_id, profile_id);
        let mut st = self.state.write().await;
        match st.rows.get_mut(&key) {
            Some(row) => {
                row.metadata.name = metadata.name.clone();
                row.metadata.scope = metadata.scope;
            }
            None => {
                st.rows.insert(
                    key,
                    MemRow {
                        payload: BlobPayload::Legacy {
                            json: serde_json::to_value(
                                quarry_browser::session::SessionSnapshot::default(),
                            )
                            .unwrap_or(serde_json::Value::Null),
                        },
                        saved_at: None,
                        rev: 0,
                        metadata: metadata.clone(),
                    },
                );
            }
        }
        Ok(())
    }

    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>> {
        let st = self.state.read().await;
        Ok(st
            .rows
            .get(&Self::key(org_id, profile_id))
            .map(|r| r.metadata.clone()))
    }

    async fn plaintext_page(
        &self,
        after: Option<&(String, String)>,
        limit: usize,
    ) -> QuarryResult<Vec<PlaintextRow>> {
        let st = self.state.read().await;
        let mut found: Vec<(String, String, PlaintextRow)> = Vec::new();
        for (key, row) in st.rows.iter() {
            let BlobPayload::Legacy { json } = &row.payload else {
                continue;
            };
            let Some((org, prof)) = key.split_once('\0') else {
                continue;
            };
            let Ok(profile_id) = prof.parse::<ProfileKind>() else {
                continue;
            };
            found.push((
                org.to_string(),
                prof.to_string(),
                PlaintextRow {
                    org_id: org.to_string(),
                    profile_id,
                    rev: row.rev,
                    json: json.clone(),
                },
            ));
        }
        found.sort_by(|a, b| (a.0.as_str(), a.1.as_str()).cmp(&(b.0.as_str(), b.1.as_str())));
        Ok(found
            .into_iter()
            .filter(|(o, p, _)| match after {
                None => true,
                Some((ao, ap)) => (o.as_str(), p.as_str()) > (ao.as_str(), ap.as_str()),
            })
            .take(limit)
            .map(|(_, _, r)| r)
            .collect())
    }

    async fn purge_org(&self, org_id: &str) -> QuarryResult<BlobPurge> {
        let prefix = format!("{org_id}\0");
        let mut st = self.state.write().await;
        let rows_before = st.rows.len();
        st.rows.retain(|k, _| !k.starts_with(&prefix));
        let rows = (rows_before - st.rows.len()) as u64;
        let tomb_before = st.tombstones.len();
        st.tombstones.retain(|k, _| !k.starts_with(&prefix));
        let tombstones = (tomb_before - st.tombstones.len()) as u64;
        Ok(BlobPurge { rows, tombstones })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn pid(_tag: &str) -> ProfileKind {
        ProfileKind::new()
    }

    fn meta_at(t: DateTime<Utc>) -> SaveMeta {
        SaveMeta {
            saved_at: t,
            owner: None,
        }
    }

    fn legacy(v: i64) -> BlobPayload {
        BlobPayload::Legacy {
            json: serde_json::json!({ "v": v }),
        }
    }

    #[tokio::test]
    async fn put_ignores_older_and_tombstoned_saves() {
        let s = InMemoryBlobStore::new();
        let p = pid("prof_put_1");
        let now = Utc::now();
        assert_eq!(
            s.put("org_a", &p, legacy(1), &meta_at(now)).await.unwrap(),
            SaveOutcome::Stored { rev: 1 }
        );
        assert_eq!(
            s.put("org_a", &p, legacy(0), &meta_at(now - Duration::seconds(10)))
                .await
                .unwrap(),
            SaveOutcome::IgnoredStale
        );
        s.delete("org_a", &p).await.unwrap();
        assert_eq!(
            s.put("org_a", &p, legacy(2), &meta_at(now - Duration::seconds(1)))
                .await
                .unwrap(),
            SaveOutcome::IgnoredByTombstone
        );
    }

    #[tokio::test]
    async fn reseal_is_cas_on_rev_and_clears_legacy_on_get() {
        let s = InMemoryBlobStore::new();
        let p = pid("prof_reseal_1");
        s.put("org_a", &p, legacy(1), &meta_at(Utc::now()))
            .await
            .unwrap();
        // A stale expected rev loses.
        assert!(!s.reseal("org_a", &p, 7, vec![1], 1, "env:v1").await.unwrap());
        assert!(s.reseal("org_a", &p, 1, vec![1, 2], 1, "env:v1").await.unwrap());
        let row = s.get("org_a", &p).await.unwrap().expect("row");
        assert!(row.is_envelope());
        assert_eq!(row.legacy_json, None);
        assert_eq!(row.rev, 1, "reseal does not bump rev");
        assert_eq!(row.key_id.as_deref(), Some("env:v1"));
    }

    #[tokio::test]
    async fn metadata_and_list_do_not_touch_payload() {
        let s = InMemoryBlobStore::new();
        let p = pid("prof_meta_1");
        s.put("org_a", &p, legacy(1), &meta_at(Utc::now()))
            .await
            .unwrap();
        let md = ProfileMetadata {
            name: Some("work".to_string()),
            ..ProfileMetadata::default()
        };
        s.save_metadata("org_a", &p, &md).await.unwrap();
        let row = s.get("org_a", &p).await.unwrap().expect("row");
        assert!(row.legacy_json.is_some());
        assert_eq!(
            s.load_metadata("org_a", &p).await.unwrap().unwrap().name.as_deref(),
            Some("work")
        );
        assert_eq!(s.list("org_a").await.unwrap().len(), 1);
        assert!(s.list("org_b").await.unwrap().is_empty());
    }
}
