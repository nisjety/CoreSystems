//! `ProfileStore` with envelope encryption at rest (WS17.E1).
//!
//! Layering, outermost first: `EnvelopeProfileStore` (seal and open, DEK
//! selection, lazy re-seal) over `CachedBlobStore` (Redis, envelope bytes
//! only, when configured) over a `ProfileBlobStore` (Postgres in production,
//! in memory in tests). The ordering and tombstone rules of WS17.P1 stay in
//! the blob store, so every backend enforces them the same way.
//!
//! Modes (`QUARRY_EDGE__PROFILE_ENCRYPTION`):
//!   * `off`: this store is not used; the plaintext stores serve as before.
//!   * `compat`: reads envelope or plaintext; writes plaintext. Safe for a mixed fleet.
//!   * `on`: reads both; writes envelopes only, with `snapshot` NULL; re-seals
//!     plaintext rows and rows wrapped under a key other than the current one.
//!
//! A load that cannot get the DEK fails with `RuntimeNotReady` (or `Internal`
//! for a wrap that does not verify). It never reports the profile as absent,
//! because a caller that starts fresh would overwrite the stored session.

use std::sync::Arc;

use async_trait::async_trait;

use quarry_browser::session::{
    ProfileMetadata, ProfileStore, ProfileSummary, SaveMeta, SaveOutcome, SessionSnapshot,
    StampOutcome,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::ProfileKind;

use crate::cached_profile_store::CachedBlobStore;
use crate::profile_blob_store::{BlobPayload, BlobRow, ProfileBlobStore};
use crate::profile_crypto::{self, CryptoError, EnvelopeV1, ENC_V};
use crate::profile_keys::{KeyError, ProfileKeyProvider};

/// A new DEK on every stored save whose current `rev` is a multiple of this.
/// Bounds the number of messages sealed under one DEK.
pub const DEK_ROTATE_EVERY_REVS: i64 = 256;

/// Encryption mode of the profile store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionMode {
    Off,
    Compat,
    On,
}

impl EncryptionMode {
    /// Parse the env value. Empty means `off`. Anything unknown is `None`
    /// (boot refuses it rather than guessing).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "off" => Some(Self::Off),
            "compat" => Some(Self::Compat),
            "on" => Some(Self::On),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Compat => "compat",
            Self::On => "on",
        }
    }
}

fn key_err(e: KeyError) -> QuarryError {
    match e {
        KeyError::Unavailable | KeyError::Destroyed => QuarryError::new(
            ErrorCode::RuntimeNotReady,
            format!("profile key {}", e.class()),
        ),
        KeyError::Denied | KeyError::Invalid => {
            QuarryError::new(ErrorCode::Internal, format!("profile key {}", e.class()))
        }
    }
}

fn crypto_err(e: CryptoError) -> QuarryError {
    QuarryError::new(
        ErrorCode::Internal,
        format!("profile envelope {}", e.class()),
    )
}

fn decode_err(label: &str) -> QuarryError {
    QuarryError::new(ErrorCode::Internal, format!("profile {label} decode failed"))
}

/// Envelope-encrypting `ProfileStore`.
pub struct EnvelopeProfileStore {
    blobs: Arc<dyn ProfileBlobStore>,
    keys: Arc<dyn ProfileKeyProvider>,
    mode: EncryptionMode,
}

impl EnvelopeProfileStore {
    pub fn new(
        blobs: Arc<dyn ProfileBlobStore>,
        keys: Arc<dyn ProfileKeyProvider>,
        mode: EncryptionMode,
    ) -> Self {
        Self { blobs, keys, mode }
    }

    pub fn mode(&self) -> EncryptionMode {
        self.mode
    }

    /// Seal a snapshot. Reuses the stored DEK unless rotation or a key change
    /// applies. `current` is the row as it is now (None for a new profile).
    fn seal_snapshot(
        &self,
        org_id: &str,
        profile: &str,
        snapshot: &SessionSnapshot,
        current: Option<&BlobRow>,
    ) -> QuarryResult<BlobPayload> {
        let plain = serde_json::to_vec(snapshot).map_err(|_| decode_err("snapshot encode"))?;
        let reused = match current.and_then(|row| row.envelope.as_ref().map(|e| (row.rev, e))) {
            Some((rev, env_bytes)) if rev % DEK_ROTATE_EVERY_REVS != 0 => {
                let env: EnvelopeV1 =
                    serde_json::from_slice(env_bytes).map_err(|_| decode_err("envelope"))?;
                // Per-org current key (org-core provider): comparing with the
                // provider-wide current_key_id() would never match, so every save
                // would mint a fresh DEK.
                if self.keys.is_current_key(org_id, &env.key_id) {
                    let wrapped = env.wrapped_dek_bytes().map_err(crypto_err)?;
                    let dek = self
                        .keys
                        .unwrap(org_id, profile, &env.key_id, &wrapped)
                        .map_err(key_err)?;
                    Some((dek, wrapped, env.key_id))
                } else {
                    None
                }
            }
            _ => None,
        };
        let (dek, wrapped, key_id) = match reused {
            Some(parts) => parts,
            None => {
                let g = self.keys.generate(org_id, profile).map_err(key_err)?;
                (g.dek, g.wrapped, g.key_id)
            }
        };
        let env = profile_crypto::seal(&dek, org_id, profile, &plain, &key_id, &wrapped)
            .map_err(crypto_err)?;
        let bytes = serde_json::to_vec(&env).map_err(|_| decode_err("envelope encode"))?;
        Ok(BlobPayload::Envelope {
            envelope: bytes,
            enc_v: ENC_V as i16,
            key_id,
        })
    }

    /// Re-seal a plaintext row, or one wrapped under an old key, in the background.
    /// Compare-and-set on `rev`: a row that moved on is left alone.
    fn spawn_reseal(&self, org_id: &str, profile_id: &ProfileKind, rev: i64, snapshot: SessionSnapshot) {
        let blobs = self.blobs.clone();
        let keys = self.keys.clone();
        let org_id = org_id.to_string();
        let profile_id = profile_id.clone();
        tokio::spawn(async move {
            match reseal_once(blobs.as_ref(), keys.as_ref(), &org_id, &profile_id, rev, &snapshot).await
            {
                Ok(true) => tracing::debug!("profile re-sealed"),
                Ok(false) => tracing::debug!("profile re-seal skipped: row moved on"),
                Err(e) => tracing::warn!(
                    error_class = e.code.wire_name(),
                    "profile re-seal failed"
                ),
            }
        });
    }
}

/// One re-seal attempt under the current key with a fresh DEK. Returns whether
/// the compare-and-set applied.
async fn reseal_once(
    blobs: &dyn ProfileBlobStore,
    keys: &dyn ProfileKeyProvider,
    org_id: &str,
    profile_id: &ProfileKind,
    rev: i64,
    snapshot: &SessionSnapshot,
) -> QuarryResult<bool> {
    let profile = profile_id.to_string();
    let plain = serde_json::to_vec(snapshot).map_err(|_| decode_err("snapshot encode"))?;
    let g = keys.generate(org_id, &profile).map_err(key_err)?;
    let env = profile_crypto::seal(&g.dek, org_id, &profile, &plain, &g.key_id, &g.wrapped)
        .map_err(crypto_err)?;
    let bytes = serde_json::to_vec(&env).map_err(|_| decode_err("envelope encode"))?;
    blobs
        .reseal(org_id, profile_id, rev, bytes, ENC_V as i16, &g.key_id)
        .await
}

#[async_trait]
impl ProfileStore for EnvelopeProfileStore {
    async fn save(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        snapshot: &SessionSnapshot,
        meta: &SaveMeta,
    ) -> QuarryResult<SaveOutcome> {
        match self.mode {
            EncryptionMode::Off => Err(QuarryError::new(
                ErrorCode::Internal,
                "envelope profile store built in mode off",
            )),
            EncryptionMode::Compat => {
                let json = serde_json::to_value(snapshot).map_err(|_| decode_err("snapshot encode"))?;
                self.blobs
                    .put(org_id, profile_id, BlobPayload::Legacy { json }, meta)
                    .await
            }
            EncryptionMode::On => {
                // The current row supplies the DEK to reuse and the rev that
                // decides rotation. Both may be stale by the time of the put;
                // that only moves rotation by one save.
                let current = self.blobs.get(org_id, profile_id).await?;
                let profile = profile_id.to_string();
                let payload = self.seal_snapshot(org_id, &profile, snapshot, current.as_ref())?;
                self.blobs.put(org_id, profile_id, payload, meta).await
            }
        }
    }

    async fn load(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<SessionSnapshot>> {
        let Some(row) = self.blobs.get(org_id, profile_id).await? else {
            return Ok(None);
        };
        let profile = profile_id.to_string();
        if let Some(env_bytes) = &row.envelope {
            let env: EnvelopeV1 =
                serde_json::from_slice(env_bytes).map_err(|_| decode_err("envelope"))?;
            let wrapped = env.wrapped_dek_bytes().map_err(crypto_err)?;
            let dek = self
                .keys
                .unwrap(org_id, &profile, &env.key_id, &wrapped)
                .map_err(key_err)?;
            let plain = profile_crypto::open(&dek, org_id, &profile, &env).map_err(crypto_err)?;
            let snap: SessionSnapshot =
                serde_json::from_slice(&plain).map_err(|_| decode_err("snapshot"))?;
            if self.mode == EncryptionMode::On && !self.keys.is_current_key(org_id, &env.key_id) {
                self.spawn_reseal(org_id, profile_id, row.rev, snap.clone());
            }
            return Ok(Some(snap));
        }
        if let Some(json) = row.legacy_json.clone() {
            let snap: SessionSnapshot =
                serde_json::from_value(json).map_err(|_| decode_err("snapshot"))?;
            if self.mode == EncryptionMode::On {
                self.spawn_reseal(org_id, profile_id, row.rev, snap.clone());
            }
            return Ok(Some(snap));
        }
        Err(QuarryError::new(
            ErrorCode::Internal,
            "profile row has no payload",
        ))
    }

    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
        self.blobs.delete(org_id, profile_id).await
    }

    async fn exists(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<bool> {
        self.blobs.exists(org_id, profile_id).await
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>> {
        self.blobs.list(org_id).await
    }

    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
    ) -> QuarryResult<()> {
        self.blobs.save_metadata(org_id, profile_id, metadata).await
    }

    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>> {
        self.blobs.load_metadata(org_id, profile_id).await
    }

    async fn stamp_owner(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
        saved_at: chrono::DateTime<chrono::Utc>,
    ) -> QuarryResult<StampOutcome> {
        // The blob store keeps the tombstones; the placeholder row holds no secret payload.
        self.blobs
            .stamp_owner(org_id, profile_id, metadata, saved_at)
            .await
    }
}

/// Build the envelope store for the edge. `blobs` is the durable blob store.
/// The Redis envelope cache is added when `redis_url` is set and connects;
/// otherwise the store runs without it. Boot checks in quarry-edge run first.
pub async fn wire(
    mode: EncryptionMode,
    blobs: Arc<dyn ProfileBlobStore>,
    keys: Arc<dyn ProfileKeyProvider>,
    redis_url: Option<&str>,
) -> Arc<dyn ProfileStore> {
    let blobs: Arc<dyn ProfileBlobStore> = match redis_url.filter(|s| !s.is_empty()) {
        None => blobs,
        Some(url) => match redis::Client::open(url) {
            Ok(client) => match redis::aio::ConnectionManager::new(client).await {
                Ok(manager) => Arc::new(CachedBlobStore::new(blobs.clone(), manager)),
                Err(e) => {
                    // zdr-log-ok: boot-time Redis connection error, no request span exists
                    tracing::warn!(error = %e, "Redis ConnectionManager failed; envelope cache disabled");
                    blobs
                }
            },
            Err(e) => {
                // zdr-log-ok: boot-time Redis configuration error, no request span exists
                tracing::warn!(error = %e, "Redis client init failed; envelope cache disabled");
                blobs
            }
        },
    };
    tracing::info!(mode = mode.as_str(), "ProfileStore: envelope layer attached");
    Arc::new(EnvelopeProfileStore::new(blobs, keys, mode))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_blob_store::InMemoryBlobStore;
    use crate::profile_keys::EnvKeyProvider;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine as _;

    const CANARY: &str = "CANARY-E1-7f3a";

    fn keys() -> Arc<dyn ProfileKeyProvider> {
        let json = format!(r#"{{"v1":"{}"}}"#, B64.encode([3u8; 32]));
        Arc::new(EnvKeyProvider::from_config(&json, "v1").expect("keys"))
    }

    /// SessionSnapshot has no PartialEq; compare the wire form.
    fn json_of(s: Option<SessionSnapshot>) -> serde_json::Value {
        serde_json::to_value(s).unwrap()
    }

    fn snapshot_with(value: &str) -> SessionSnapshot {
        SessionSnapshot {
            session_storage: vec![("k".to_string(), value.to_string())],
            ..SessionSnapshot::default()
        }
    }

    fn store(mode: EncryptionMode, blobs: Arc<dyn ProfileBlobStore>) -> EnvelopeProfileStore {
        EnvelopeProfileStore::new(blobs, keys(), mode)
    }

    async fn settle() {
        // The re-seal runs on a spawned task; give it a few turns.
        for _ in 0..50 {
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    #[test]
    fn mode_parse_refuses_unknown_values() {
        assert_eq!(EncryptionMode::parse("off"), Some(EncryptionMode::Off));
        assert_eq!(EncryptionMode::parse("compat"), Some(EncryptionMode::Compat));
        assert_eq!(EncryptionMode::parse("ON"), Some(EncryptionMode::On));
        assert_eq!(EncryptionMode::parse("required"), None);
    }

    #[test]
    fn blob_cache_keys_are_namespaced_away_from_legacy_keys() {
        let p = ProfileKind::new();
        let k = crate::cached_profile_store::blob_cache_key("org_a", &p);
        assert!(k.starts_with("quarry:profile:v2:org_a:"));
        assert_ne!(k, format!("quarry:profile:org_a:{p}"));
    }

    #[tokio::test]
    async fn compat_mode_writes_plaintext() {
        let blobs: Arc<dyn ProfileBlobStore> = Arc::new(InMemoryBlobStore::new());
        let s = store(EncryptionMode::Compat, blobs.clone());
        let p = ProfileKind::new();
        s.save("org_a", &p, &snapshot_with(CANARY), &SaveMeta::now(None))
            .await
            .unwrap();
        let row = blobs.get("org_a", &p).await.unwrap().unwrap();
        assert!(row.legacy_json.is_some());
        assert!(row.envelope.is_none());
        assert_eq!(json_of(s.load("org_a", &p).await.unwrap()), json_of(Some(snapshot_with(CANARY))));
    }

    #[tokio::test]
    async fn on_mode_nulls_plaintext_and_holds_no_canary() {
        let blobs: Arc<dyn ProfileBlobStore> = Arc::new(InMemoryBlobStore::new());
        let s = store(EncryptionMode::On, blobs.clone());
        let p = ProfileKind::new();
        s.save("org_a", &p, &snapshot_with(CANARY), &SaveMeta::now(None))
            .await
            .unwrap();
        let row = blobs.get("org_a", &p).await.unwrap().unwrap();
        assert!(row.legacy_json.is_none(), "plaintext column is nulled");
        let env = row.envelope.expect("envelope written");
        assert!(!env.windows(CANARY.len()).any(|w| w == CANARY.as_bytes()));
        assert_eq!(row.key_id.as_deref(), Some("env:v1"));
        assert_eq!(row.enc_v, Some(1));
        assert_eq!(json_of(s.load("org_a", &p).await.unwrap()), json_of(Some(snapshot_with(CANARY))));
    }

    #[tokio::test]
    async fn plaintext_row_is_resealed_lazily_with_cas() {
        let blobs: Arc<dyn ProfileBlobStore> = Arc::new(InMemoryBlobStore::new());
        let p = ProfileKind::new();
        blobs
            .put(
                "org_a",
                &p,
                BlobPayload::Legacy {
                    json: serde_json::to_value(snapshot_with(CANARY)).unwrap(),
                },
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        let s = store(EncryptionMode::On, blobs.clone());
        assert_eq!(json_of(s.load("org_a", &p).await.unwrap()), json_of(Some(snapshot_with(CANARY))));
        settle().await;
        let row = blobs.get("org_a", &p).await.unwrap().unwrap();
        assert!(row.envelope.is_some(), "plaintext row was re-sealed");
        assert!(row.legacy_json.is_none());
        assert_eq!(json_of(s.load("org_a", &p).await.unwrap()), json_of(Some(snapshot_with(CANARY))));
    }

    #[tokio::test]
    async fn key_unavailable_is_not_start_fresh() {
        let blobs: Arc<dyn ProfileBlobStore> = Arc::new(InMemoryBlobStore::new());
        let writer = store(EncryptionMode::On, blobs.clone());
        let p = ProfileKind::new();
        writer
            .save("org_a", &p, &snapshot_with(CANARY), &SaveMeta::now(None))
            .await
            .unwrap();
        let before = blobs.get("org_a", &p).await.unwrap().unwrap();
        // A provider that only knows master v2: the stored wrap names v1.
        let other_json = format!(r#"{{"v2":"{}"}}"#, B64.encode([4u8; 32]));
        let other: Arc<dyn ProfileKeyProvider> =
            Arc::new(EnvKeyProvider::from_config(&other_json, "v2").unwrap());
        let reader = EnvelopeProfileStore::new(blobs.clone(), other, EncryptionMode::On);
        let err = reader.load("org_a", &p).await.err().expect("load fails");
        assert_eq!(err.code, ErrorCode::RuntimeNotReady);
        assert_eq!(blobs.get("org_a", &p).await.unwrap().unwrap(), before);
    }

    #[tokio::test]
    async fn delete_then_exists_is_false_and_tombstone_refuses_older_save() {
        let blobs: Arc<dyn ProfileBlobStore> = Arc::new(InMemoryBlobStore::new());
        let s = store(EncryptionMode::On, blobs);
        let p = ProfileKind::new();
        let t0 = SaveMeta::now(None);
        s.save("org_a", &p, &snapshot_with("a"), &t0).await.unwrap();
        s.delete("org_a", &p).await.unwrap();
        assert!(!s.exists("org_a", &p).await.unwrap());
        let older = SaveMeta {
            saved_at: t0.saved_at,
            owner: None,
        };
        assert_eq!(
            s.save("org_a", &p, &snapshot_with("b"), &older).await.unwrap(),
            SaveOutcome::IgnoredByTombstone
        );
    }

    /// WS17.E1 source scan: the plaintext S3 archive store is not constructed by any
    /// module of this crate except its own file.
    #[test]
    fn s3_profile_store_is_not_constructed_in_production_code() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(&dir).expect("src dir") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("s3_profile_store.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read source");
            assert!(
                // concat! keeps this file from matching its own needles.
                !text.contains(concat!("S3Profile", "Store::new"))
                    && !text.contains(concat!("S3Profile", "Store::from_parts")),
                "S3ProfileStore is plaintext and must not be constructed: {}",
                path.display()
            );
        }
    }
}
