//! Postgres-backed `ProfileStore`.
//!
//! Cycle 24 / cluster #6.
//!
//! Why this exists: the existing `InMemoryProfileStore` and
//! `S3ProfileStore` cover dev + bulk archival, but the production
//! hot-path needs:
//!   1. **Restart-safe** session restore (in-memory loses everything
//!      on rolling deploy).
//!   2. **Multi-instance** consistency (S3 list is eventually-consistent
//!      — two edges can see different profile sets for tens of
//!      seconds after a write).
//!   3. **Per-tenant isolation** that's enforced by the schema, not
//!      by hopeful code paths.
//!
//! Postgres gives us all three. The schema lives at
//! `crates/quarry-runtime/migrations/0002_profiles.sql`.
//!
//! Gated behind the `postgres-queue` feature so default builds don't
//! pull `sqlx` / `libpq`.

#![cfg(feature = "postgres-queue")]

use async_trait::async_trait;
use sqlx::postgres::PgPool;
use sqlx::Row;

use quarry_browser::session::{
    ProfileMetadata, ProfileScope, ProfileStore, ProfileSummary, SaveMeta, SaveOutcome,
    SessionSnapshot,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::ProfileKind;

use crate::profile_blob_store::{BlobPayload, BlobRow, ProfileBlobStore};

fn db_err(label: &str, e: sqlx::Error) -> QuarryError {
    QuarryError::new(ErrorCode::Internal, format!("postgres {label}: {e}"))
}

/// Placeholder snapshot for a freshly `save_metadata`'d profile that has
/// no browsing history yet (e.g. an explicitly "created" named profile).
/// Serializes cleanly and deserializes back into `SessionSnapshot`
/// unlike a bare `{}` — `cookies`/`local_storage`/`session_storage` have
/// no per-field `#[serde(default)]`, so an empty JSON object would fail
/// to deserialize.
fn empty_snapshot_json() -> serde_json::Value {
    serde_json::to_value(SessionSnapshot::default())
        .expect("SessionSnapshot::default always serializes")
}

/// Durable, multi-instance-safe `ProfileStore`.
///
/// All methods take an explicit `org_id` so the schema's PK
/// `(org_id, profile_id)` is hit on every operation. The verified JWT
/// org from the edge handler is the only value ever passed in
/// production (P0 / cluster #auth+tenancy enforces this upstream).
#[derive(Clone)]
pub struct PostgresProfileStore {
    pool: PgPool,
}

/// Tombstones outlive the delete by this long (WS17.P1). A capture that began
/// within this window before a delete is still refused.
pub const TOMBSTONE_TTL_DAYS: i32 = 7;

/// How often the sweeper drops expired tombstones.
pub const TOMBSTONE_SWEEP_INTERVAL_S: u64 = 3600;

impl PostgresProfileStore {
    /// Construct from an existing pool. Migrations are applied
    /// idempotently by the surrounding runtime — we don't run them
    /// inline so multiple components sharing the pool don't race on
    /// the migration table.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Drop tombstones whose TTL has passed. Returns how many were removed.
    pub async fn purge_expired_tombstones(&self) -> QuarryResult<u64> {
        let done = sqlx::query("DELETE FROM quarry_profile_tombstones WHERE expires_at < NOW()")
            .execute(&self.pool)
            .await
            .map_err(|e| db_err("purge_tombstones", e))?;
        Ok(done.rows_affected())
    }
}

impl PostgresProfileStore {
    /// Ordered upsert of one payload (WS17.P1 rules, WS17.E1 columns). Shared by
    /// the plaintext `ProfileStore::save` (legacy payload) and the envelope blob
    /// store. One statement decides: the clamp, the tombstone check, the insert
    /// and the ordered upsert share one `saved_at`. A stale or tombstoned save
    /// writes nothing. Owner is stamped on INSERT only.
    pub(crate) async fn upsert_payload(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        payload: &BlobPayload,
        meta: &SaveMeta,
    ) -> QuarryResult<SaveOutcome> {
        let (json, envelope, enc_v, key_id): (
            Option<serde_json::Value>,
            Option<Vec<u8>>,
            Option<i16>,
            Option<String>,
        ) = match payload {
            BlobPayload::Legacy { json } => (Some(json.clone()), None, None, None),
            BlobPayload::Envelope {
                envelope,
                enc_v,
                key_id,
            } => (None, Some(envelope.clone()), Some(*enc_v), Some(key_id.clone())),
        };
        let stored: Option<(i64,)> = sqlx::query_as(
            "WITH s AS ( \
                 SELECT LEAST($3::timestamptz, NOW() + INTERVAL '5 seconds') AS saved_at \
             ) \
             INSERT INTO quarry_profiles \
                 (org_id, profile_id, snapshot, envelope, enc_v, key_id, saved_at, rev, owner_user_id, created_at, updated_at) \
             SELECT $1::text, $2::text, $4::jsonb, $6::bytea, $7::smallint, $8::text, s.saved_at, 1, $5::text, NOW(), NOW() FROM s \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM quarry_profile_tombstones t \
                 WHERE t.org_id = $1::text AND t.profile_id = $2::text \
                   AND t.deleted_at >= s.saved_at \
             ) \
             ON CONFLICT (org_id, profile_id) DO UPDATE SET \
                 snapshot   = EXCLUDED.snapshot, \
                 envelope   = EXCLUDED.envelope, \
                 enc_v      = EXCLUDED.enc_v, \
                 key_id     = EXCLUDED.key_id, \
                 saved_at   = EXCLUDED.saved_at, \
                 rev        = quarry_profiles.rev + 1, \
                 updated_at = NOW() \
             WHERE quarry_profiles.saved_at IS NULL \
                OR quarry_profiles.saved_at < EXCLUDED.saved_at \
             RETURNING rev",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .bind(meta.saved_at)
        .bind(json)
        .bind(meta.owner.as_deref())
        .bind(envelope)
        .bind(enc_v)
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_err("save", e))?;

        if let Some((rev,)) = stored {
            return Ok(SaveOutcome::Stored { rev });
        }
        // Nothing was written. Diagnose only the reason, for the log class.
        let (tombstoned,): (bool,) = sqlx::query_as(
            "SELECT EXISTS ( \
                 SELECT 1 FROM quarry_profile_tombstones \
                 WHERE org_id = $1 AND profile_id = $2 \
                   AND deleted_at >= LEAST($3::timestamptz, NOW() + INTERVAL '5 seconds') \
             )",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .bind(meta.saved_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| db_err("save:diagnose", e))?;
        Ok(if tombstoned {
            SaveOutcome::IgnoredByTombstone
        } else {
            SaveOutcome::IgnoredStale
        })
    }

    /// The row's payload columns and ordering state (WS17.E1 blob read).
    pub(crate) async fn fetch_blob_row(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<BlobRow>> {
        let row: Option<(
            Option<serde_json::Value>,
            Option<Vec<u8>>,
            Option<chrono::DateTime<chrono::Utc>>,
            i64,
            Option<i16>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT snapshot, envelope, saved_at, rev, enc_v, key_id \
             FROM quarry_profiles WHERE org_id = $1 AND profile_id = $2",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_err("blob_get", e))?;
        Ok(row.map(
            |(legacy_json, envelope, saved_at, rev, enc_v, key_id)| BlobRow {
                envelope,
                legacy_json,
                saved_at,
                rev,
                enc_v,
                key_id,
            },
        ))
    }

    /// Compare-and-set re-seal: the payload becomes the envelope only while the
    /// row is still at `expected_rev`. `rev` and `saved_at` do not move.
    pub(crate) async fn reseal_row(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        expected_rev: i64,
        envelope: Vec<u8>,
        enc_v: i16,
        key_id: &str,
    ) -> QuarryResult<bool> {
        let done = sqlx::query(
            "UPDATE quarry_profiles \
             SET envelope = $3, enc_v = $4, key_id = $5, snapshot = NULL, updated_at = NOW() \
             WHERE org_id = $1 AND profile_id = $2 AND rev = $6",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .bind(envelope)
        .bind(enc_v)
        .bind(key_id)
        .bind(expected_rev)
        .execute(&self.pool)
        .await
        .map_err(|e| db_err("blob_reseal", e))?;
        Ok(done.rows_affected() == 1)
    }
}

/// Start the hourly tombstone sweep. The edge calls this once at boot, next
/// to the store it builds (see `main.rs`). The first tick runs immediately.
pub fn spawn_tombstone_sweeper(store: PostgresProfileStore) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick =
            tokio::time::interval(std::time::Duration::from_secs(TOMBSTONE_SWEEP_INTERVAL_S));
        loop {
            tick.tick().await;
            match store.purge_expired_tombstones().await {
                Ok(0) => {}
                Ok(purged) => tracing::debug!(purged, "profile tombstones swept"),
                Err(e) => tracing::warn!(
                    error_class = e.code.wire_name(),
                    "profile tombstone sweep failed"
                ),
            }
        }
    })
}

#[async_trait]
impl ProfileStore for PostgresProfileStore {
    async fn save(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        snapshot: &SessionSnapshot,
        meta: &SaveMeta,
    ) -> QuarryResult<SaveOutcome> {
        let json = serde_json::to_value(snapshot).map_err(|e| {
            QuarryError::new(ErrorCode::Internal, format!("snapshot encode failed: {e}"))
        })?;
        self.upsert_payload(org_id, profile_id, &BlobPayload::Legacy { json }, meta)
            .await
    }

    async fn load(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<SessionSnapshot>> {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT snapshot FROM quarry_profiles \
             WHERE org_id = $1 AND profile_id = $2",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_err("load", e))?;

        match row {
            None => Ok(None),
            Some((json,)) => {
                let snap: SessionSnapshot = serde_json::from_value(json).map_err(|e| {
                    QuarryError::new(ErrorCode::Internal, format!("snapshot decode failed: {e}"))
                })?;
                Ok(Some(snap))
            }
        }
    }

    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
        // The tombstone and the row removal share one transaction, so a save
        // whose capture started before this delete cannot land after it.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| db_err("delete:begin", e))?;
        sqlx::query(
            "INSERT INTO quarry_profile_tombstones (org_id, profile_id, deleted_at, expires_at) \
             VALUES ($1, $2, NOW(), NOW() + make_interval(days => $3)) \
             ON CONFLICT (org_id, profile_id) DO UPDATE SET \
                 deleted_at = GREATEST(quarry_profile_tombstones.deleted_at, EXCLUDED.deleted_at), \
                 expires_at = GREATEST(quarry_profile_tombstones.expires_at, EXCLUDED.expires_at)",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .bind(TOMBSTONE_TTL_DAYS)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err("delete:tombstone", e))?;
        sqlx::query("DELETE FROM quarry_profiles WHERE org_id = $1 AND profile_id = $2")
            .bind(org_id)
            .bind(profile_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| db_err("delete", e))?;
        tx.commit().await.map_err(|e| db_err("delete:commit", e))?;
        Ok(())
    }

    async fn exists(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<bool> {
        let (found,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM quarry_profiles WHERE org_id = $1 AND profile_id = $2)",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| db_err("exists", e))?;
        Ok(found)
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>> {
        // ORDER BY updated_at DESC uses the partial index defined in
        // 0002_profiles.sql — single index seek + range scan, no full
        // table walk even on a multi-tenant DB.
        let rows = sqlx::query(
            "SELECT profile_id, name, scope, owner_user_id FROM quarry_profiles \
             WHERE org_id = $1 \
             ORDER BY updated_at DESC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| db_err("list", e))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let id_str: String = row.try_get(0).map_err(|e| db_err("list:row", e))?;
            let name: Option<String> = row.try_get(1).map_err(|e| db_err("list:row", e))?;
            let scope_str: String = row.try_get(2).map_err(|e| db_err("list:row", e))?;
            let owner: Option<String> = row.try_get(3).map_err(|e| db_err("list:row", e))?;
            if let Ok(profile_id) = id_str.parse::<ProfileKind>() {
                out.push(ProfileSummary {
                    profile_id,
                    name,
                    scope: scope_str.parse::<ProfileScope>().unwrap_or_default(),
                    owner,
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
        // ON CONFLICT only touches name/scope/updated_at — an existing
        // row's snapshot (real captured cookies/storage) is never
        // clobbered by a rename/rescope. The INSERT branch only fires
        // for a brand-new, snapshot-less "created" profile, so its
        // placeholder snapshot is safe.
        sqlx::query(
            "INSERT INTO quarry_profiles (org_id, profile_id, snapshot, name, scope, owner_user_id, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW()) \
             ON CONFLICT (org_id, profile_id) DO UPDATE SET \
                 name       = EXCLUDED.name, \
                 scope      = EXCLUDED.scope, \
                 updated_at = NOW()",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .bind(empty_snapshot_json())
        .bind(&metadata.name)
        .bind(metadata.scope.as_str())
        .bind(&metadata.owner)
        .execute(&self.pool)
        .await
        .map_err(|e| db_err("save_metadata", e))?;
        Ok(())
    }

    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>> {
        let row: Option<(Option<String>, String, Option<String>)> = sqlx::query_as(
            "SELECT name, scope, owner_user_id FROM quarry_profiles \
             WHERE org_id = $1 AND profile_id = $2",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_err("load_metadata", e))?;

        Ok(row.map(|(name, scope, owner)| ProfileMetadata {
            name,
            scope: scope.parse::<ProfileScope>().unwrap_or_default(),
            owner,
        }))
    }
}

/// Postgres `ProfileBlobStore` (WS17.E1, renamed from the plan's
/// `PostgresProfileBlobStore` split): the same table and ordering rules as
/// `PostgresProfileStore`, with the envelope columns. Wrapped in
/// `CachedBlobStore` when Redis is configured.
#[derive(Clone)]
pub struct PostgresProfileBlobStore {
    inner: PostgresProfileStore,
}

impl PostgresProfileBlobStore {
    pub fn new(store: PostgresProfileStore) -> Self {
        Self { inner: store }
    }
}

#[async_trait]
impl ProfileBlobStore for PostgresProfileBlobStore {
    async fn put(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        payload: BlobPayload,
        meta: &SaveMeta,
    ) -> QuarryResult<SaveOutcome> {
        self.inner
            .upsert_payload(org_id, profile_id, &payload, meta)
            .await
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
        self.inner
            .reseal_row(org_id, profile_id, expected_rev, envelope, enc_v, key_id)
            .await
    }

    async fn get(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<Option<BlobRow>> {
        self.inner.fetch_blob_row(org_id, profile_id).await
    }

    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
        ProfileStore::delete(&self.inner, org_id, profile_id).await
    }

    async fn exists(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<bool> {
        ProfileStore::exists(&self.inner, org_id, profile_id).await
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>> {
        ProfileStore::list(&self.inner, org_id).await
    }

    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
    ) -> QuarryResult<()> {
        ProfileStore::save_metadata(&self.inner, org_id, profile_id, metadata).await
    }

    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>> {
        ProfileStore::load_metadata(&self.inner, org_id, profile_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pool on this test's own schema (`crate::postgres_test_support`): no table is
    /// shared with another test, so these run in parallel. Skips without a
    /// `DATABASE_URL`; fails instead when `REQUIRE_CONTRACT` is set.
    async fn fresh_pool() -> Option<PgPool> {
        crate::postgres_test_support::isolated_pool("profile store").await
    }

    async fn clean(pool: &PgPool) {
        let _ = sqlx::query("TRUNCATE TABLE quarry_profiles")
            .execute(pool)
            .await;
    }

    fn snap() -> SessionSnapshot {
        SessionSnapshot {
            cookies: vec![],
            local_storage: vec![("k".into(), "v".into())],
            session_storage: vec![],
            indexed_db: vec![],
            user_agent: Some("test/1.0".into()),
            viewport: None,
            locale: Some("en-US".into()),
            timezone: None,
        }
    }

    #[tokio::test]
    async fn save_load_roundtrip() {
        let Some(pool) = fresh_pool().await else {
            eprintln!("skipping: DATABASE_URL not set");
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();
        store
            .save("org_a", &id, &snap(), &SaveMeta::now(None))
            .await
            .unwrap();

        let loaded = store.load("org_a", &id).await.unwrap().unwrap();
        assert_eq!(loaded.user_agent.as_deref(), Some("test/1.0"));
        assert_eq!(loaded.local_storage.len(), 1);
    }

    #[tokio::test]
    async fn save_is_upsert_on_repeated_id() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();

        store
            .save("org_a", &id, &snap(), &SaveMeta::now(None))
            .await
            .unwrap();
        // Second save with different content should overwrite, not insert.
        let mut other = snap();
        other.user_agent = Some("test/2.0".into());
        store
            .save("org_a", &id, &other, &SaveMeta::now(None))
            .await
            .unwrap();

        let loaded = store.load("org_a", &id).await.unwrap().unwrap();
        assert_eq!(loaded.user_agent.as_deref(), Some("test/2.0"));
        // list MUST return exactly 1 row (upsert, not duplicate).
        let ids = store.list("org_a").await.unwrap();
        assert_eq!(ids.len(), 1);
    }

    #[tokio::test]
    async fn tenant_isolation_cross_org_load_returns_none() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();

        store
            .save("org_a", &id, &snap(), &SaveMeta::now(None))
            .await
            .unwrap();

        // The same profile_id under a DIFFERENT org MUST be invisible
        // (it's actually inaccessible because the PK is composite).
        let none = store.load("org_b", &id).await.unwrap();
        assert!(none.is_none(), "cross-org load leaked");

        let none_list = store.list("org_b").await.unwrap();
        assert!(none_list.is_empty(), "cross-org list leaked");

        // Cross-org delete must not affect org_a's data.
        store.delete("org_b", &id).await.unwrap();
        let still_there = store.load("org_a", &id).await.unwrap();
        assert!(still_there.is_some(), "cross-org delete corrupted org_a");
    }

    fn meta_at(saved_at: chrono::DateTime<chrono::Utc>) -> SaveMeta {
        SaveMeta {
            saved_at,
            owner: None,
        }
    }

    #[tokio::test]
    async fn save_with_older_saved_at_is_ignored() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();
        let now = chrono::Utc::now();
        store
            .save("org_a", &id, &snap(), &meta_at(now))
            .await
            .unwrap();
        let mut older = snap();
        older.user_agent = Some("older".into());
        let outcome = store
            .save(
                "org_a",
                &id,
                &older,
                &meta_at(now - chrono::Duration::seconds(10)),
            )
            .await
            .unwrap();
        assert_eq!(outcome, SaveOutcome::IgnoredStale);
        let loaded = store.load("org_a", &id).await.unwrap().unwrap();
        assert_eq!(loaded.user_agent.as_deref(), Some("test/1.0"));
    }

    #[tokio::test]
    async fn save_after_delete_with_older_saved_at_is_ignored_by_tombstone() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();
        let capture_started = chrono::Utc::now();
        store
            .save("org_a", &id, &snap(), &meta_at(capture_started))
            .await
            .unwrap();
        store.delete("org_a", &id).await.unwrap();
        let outcome = store
            .save("org_a", &id, &snap(), &meta_at(capture_started))
            .await
            .unwrap();
        assert_eq!(outcome, SaveOutcome::IgnoredByTombstone);
        assert!(store.load("org_a", &id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn save_after_delete_with_newer_saved_at_creates_a_new_row() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();
        store
            .save("org_a", &id, &snap(), &SaveMeta::now(None))
            .await
            .unwrap();
        store.delete("org_a", &id).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let outcome = store
            .save("org_a", &id, &snap(), &SaveMeta::now(None))
            .await
            .unwrap();
        assert!(outcome.is_stored());
        assert!(store.load("org_a", &id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn saved_at_is_clamped_to_now_plus_5s() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();
        let far_future = chrono::Utc::now() + chrono::Duration::hours(1);
        assert!(store
            .save("org_a", &id, &snap(), &meta_at(far_future))
            .await
            .unwrap()
            .is_stored());
        // Stored as now + 5 s, so a save stamped now + 1 s is older.
        let just_ahead = chrono::Utc::now() + chrono::Duration::seconds(1);
        let outcome = store
            .save("org_a", &id, &snap(), &meta_at(just_ahead))
            .await
            .unwrap();
        assert_eq!(outcome, SaveOutcome::IgnoredStale);
    }

    #[tokio::test]
    async fn rev_increments_only_on_stored() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();
        let now = chrono::Utc::now();
        assert_eq!(
            store
                .save("org_a", &id, &snap(), &meta_at(now))
                .await
                .unwrap(),
            SaveOutcome::Stored { rev: 1 }
        );
        let stale = store
            .save(
                "org_a",
                &id,
                &snap(),
                &meta_at(now - chrono::Duration::seconds(10)),
            )
            .await
            .unwrap();
        assert_eq!(stale, SaveOutcome::IgnoredStale);
        assert_eq!(
            store
                .save(
                    "org_a",
                    &id,
                    &snap(),
                    &meta_at(now + chrono::Duration::seconds(1))
                )
                .await
                .unwrap(),
            SaveOutcome::Stored { rev: 2 }
        );
    }

    #[tokio::test]
    async fn delete_writes_tombstone_in_same_transaction() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let _ = sqlx::query("TRUNCATE TABLE quarry_profile_tombstones")
            .execute(&pool)
            .await;
        let store = PostgresProfileStore::new(pool.clone());
        let id: ProfileKind = quarry_core::ids::Id::new();
        store
            .save("org_a", &id, &snap(), &SaveMeta::now(None))
            .await
            .unwrap();
        store.delete("org_a", &id).await.unwrap();
        let (rows,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM quarry_profiles WHERE org_id = $1 AND profile_id = $2",
        )
        .bind("org_a")
        .bind(id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 0);
        let (tombs,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM quarry_profile_tombstones WHERE org_id = $1 AND profile_id = $2",
        )
        .bind("org_a")
        .bind(id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(tombs, 1);
        assert!(!store.exists("org_a", &id).await.unwrap());
    }

    #[tokio::test]
    async fn expired_tombstones_are_swept() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let _ = sqlx::query("TRUNCATE TABLE quarry_profile_tombstones")
            .execute(&pool)
            .await;
        let store = PostgresProfileStore::new(pool.clone());
        let expired: ProfileKind = quarry_core::ids::Id::new();
        let live: ProfileKind = quarry_core::ids::Id::new();
        sqlx::query(
            "INSERT INTO quarry_profile_tombstones (org_id, profile_id, deleted_at, expires_at) \
             VALUES ('org_a', $1, NOW() - INTERVAL '8 days', NOW() - INTERVAL '1 day'), \
                    ('org_a', $2, NOW(), NOW() + INTERVAL '7 days')",
        )
        .bind(expired.to_string())
        .bind(live.to_string())
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(store.purge_expired_tombstones().await.unwrap(), 1);
        let (left,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM quarry_profile_tombstones")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 1);
    }

    #[tokio::test]
    async fn exists_matches_load() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();
        assert!(!store.exists("org_a", &id).await.unwrap());
        store
            .save("org_a", &id, &snap(), &SaveMeta::now(None))
            .await
            .unwrap();
        assert!(store.exists("org_a", &id).await.unwrap());
        assert!(!store.exists("org_b", &id).await.unwrap());
    }

    #[tokio::test]
    async fn list_orders_by_recent_update() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);

        let a: ProfileKind = quarry_core::ids::Id::new();
        let b: ProfileKind = quarry_core::ids::Id::new();
        store
            .save("org_a", &a, &snap(), &SaveMeta::now(None))
            .await
            .unwrap();
        // Tick to ensure distinct updated_at values.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        store
            .save("org_a", &b, &snap(), &SaveMeta::now(None))
            .await
            .unwrap();

        let ids = store.list("org_a").await.unwrap();
        assert_eq!(ids.len(), 2);
        // Most-recent first → b before a.
        assert_eq!(ids[0].profile_id.to_string(), b.to_string());
        assert_eq!(ids[1].profile_id.to_string(), a.to_string());
    }
}

#[cfg(test)]
mod blob_tests {
    use super::*;

    /// Envelope rows on Postgres (WS17.E1): NULL snapshot, envelope bytes kept
    /// as written, and the re-seal compare-and-set on rev. Skips without a
    /// DATABASE_URL, like the other store tests.
    #[tokio::test]
    async fn envelope_row_roundtrip_and_reseal_cas() {
        let Some(pool) = crate::postgres_test_support::isolated_pool("profile blob store").await
        else {
            return;
        };
        let _ = sqlx::query("TRUNCATE TABLE quarry_profiles")
            .execute(&pool)
            .await;
        let store = PostgresProfileBlobStore::new(PostgresProfileStore::new(pool));
        let id: ProfileKind = quarry_core::ids::Id::new();
        let envelope = br#"{"enc_v":1}"#.to_vec();
        let out = store
            .put(
                "org_a",
                &id,
                BlobPayload::Envelope {
                    envelope: envelope.clone(),
                    enc_v: 1,
                    key_id: "env:v1".into(),
                },
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        assert!(out.is_stored());
        let row = store.get("org_a", &id).await.unwrap().expect("row");
        assert!(row.legacy_json.is_none(), "snapshot column is NULL for envelope rows");
        assert_eq!(row.envelope.as_deref(), Some(envelope.as_slice()));
        assert_eq!(row.key_id.as_deref(), Some("env:v1"));
        assert!(
            !store
                .reseal("org_a", &id, row.rev + 1, b"x".to_vec(), 1, "env:v2")
                .await
                .unwrap(),
            "a stale rev loses the compare-and-set"
        );
        assert!(store
            .reseal("org_a", &id, row.rev, b"x".to_vec(), 1, "env:v2")
            .await
            .unwrap());
        assert_eq!(
            store.get("org_a", &id).await.unwrap().unwrap().rev,
            row.rev,
            "reseal does not bump rev"
        );
    }
}
