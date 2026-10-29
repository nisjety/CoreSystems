//! Session restore/capture shape and profile store.
//!
//! `SessionSnapshot` carries all browser state needed to resume a session:
//! cookies, web storage, viewport, locale, timezone, and user-agent.
//!
//! `ProfileStore` is the persistence trait — implementations range from
//! in-memory (tests) to S3 (production).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use quarry_core::ids::kinds::ProfileKind;
use quarry_core::QuarryResult;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub cookies: Vec<Cookie>,
    pub local_storage: Vec<(String, String)>,
    pub session_storage: Vec<(String, String)>,
    /// Cycle 20 / cluster #13 — IndexedDB capture. Per-origin entries
    /// keyed `"<origin>::<db>::<store>::<key>"` so a snapshot can
    /// faithfully restore object-store records that auth-gated SPAs
    /// (Notion, Linear, Figma, etc.) require. Empty for back-compat
    /// when the driver couldn't or didn't capture IndexedDB state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexed_db: Vec<IndexedDbEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// One row inside an IndexedDB object store. `origin`/`database`/`store`
/// identify the bucket; `key` is the IDB key (string-form). `value` is
/// the JSON-serialized record body. Drivers that don't support IDB
/// capture (Browserless static, Kernel without explicit IDB hook) emit
/// no entries — the empty vec round-trips cleanly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedDbEntry {
    pub origin: String,
    pub database: String,
    pub store: String,
    pub key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub device_scale_factor: f64,
    #[serde(default)]
    pub is_mobile: bool,
}

/// Profile scope — mirrors the SPA's `BrowserProfileScope` union
/// (`ephemeral | user_private | org_shared | run_scoped`). Phase 3
/// continuation: promotes scope from an inferred gateway-side boolean
/// into first-class, queryable profile metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileScope {
    /// No persistence intended. A *session* can be ephemeral without any
    /// profile at all. This is the in-language default for "no scope was
    /// specified" (e.g. `ProfileMetadata::default()` when a store has no
    /// metadata row at all) — but a persisted `ProfileMetadata` row must
    /// never actually carry this value: `quarry-edge`'s create/update
    /// handlers reject `scope: ephemeral` outright, and the Postgres
    /// migration backfills/defaults un-metadata'd rows to `UserPrivate`,
    /// not this variant, precisely because a row that exists in storage
    /// was, by construction, deliberately persisted.
    #[default]
    Ephemeral,
    /// Persists for one signed-in user only.
    UserPrivate,
    /// Persists and is usable by any member of the org.
    OrgShared,
    /// Persists only for the lifetime of a single agent run/grant.
    RunScoped,
}

impl ProfileScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::UserPrivate => "user_private",
            Self::OrgShared => "org_shared",
            Self::RunScoped => "run_scoped",
        }
    }
}

impl std::fmt::Display for ProfileScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ProfileScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ephemeral" => Ok(Self::Ephemeral),
            "user_private" => Ok(Self::UserPrivate),
            "org_shared" => Ok(Self::OrgShared),
            "run_scoped" => Ok(Self::RunScoped),
            other => Err(format!("unknown profile scope: {other}")),
        }
    }
}

/// First-class profile metadata, additive alongside `SessionSnapshot`.
/// `name` is a caller-chosen display label; `scope` mirrors the SPA's
/// `BrowserProfileScope`. Stored/loaded independently of the snapshot so
/// naming/rescoping a profile never touches its captured cookies/storage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub scope: ProfileScope,
}

/// One row of `ProfileStore::list` — enough to render a profile picker
/// (name + scope) without a follow-up load per entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub profile_id: ProfileKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub scope: ProfileScope,
}

/// Tenant-scoped profile storage. Every method takes the verified
/// `org_id` so the store can never return another tenant's session state
/// (cookies, storage, viewport). The trait is intentionally a leaky
/// abstraction here — the org_id parameter forces every backend to
/// enforce isolation, rather than relying on callers to remember.
#[async_trait]
pub trait ProfileStore: Send + Sync {
    async fn save(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        snapshot: &SessionSnapshot,
    ) -> QuarryResult<()>;
    async fn load(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<SessionSnapshot>>;
    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()>;
    /// Enumerate this org's profiles with name/scope metadata attached.
    /// Phase 3 continuation: previously returned bare `Vec<ProfileKind>`.
    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>>;

    /// Persist/overwrite a profile's name/scope. Additive: never touches
    /// the `SessionSnapshot` written by `save`. Implementations must not
    /// require a prior `save` call — metadata can be written for a
    /// freshly "created" named profile that has no browsing history yet.
    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
    ) -> QuarryResult<()>;
    /// Load a profile's metadata. `Ok(None)` means no metadata (and, per
    /// `list`'s contract, no snapshot either) exists at all — callers
    /// that already know the profile exists should treat `None` here as
    /// `ProfileMetadata::default()` (scope `Ephemeral`, no name).
    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>>;
}

pub struct InMemoryProfileStore {
    /// Composite key `{org_id}\0{profile_id}` keeps tenants strictly
    /// partitioned in a single hashmap. The NUL separator prevents any
    /// accidental key collision between an org named "alpha" with profile
    /// "foo" and an org named "alphafoo" with no profile suffix.
    store: tokio::sync::RwLock<std::collections::HashMap<String, SessionSnapshot>>,
    /// Same composite-key scheme, kept as an independent map so naming
    /// or rescoping a profile never touches its captured snapshot.
    metadata: tokio::sync::RwLock<std::collections::HashMap<String, ProfileMetadata>>,
}

impl InMemoryProfileStore {
    pub fn new() -> Self {
        Self {
            store: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            metadata: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    fn key(org_id: &str, profile_id: &ProfileKind) -> String {
        format!("{org_id}\0{profile_id}")
    }
}

impl Default for InMemoryProfileStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProfileStore for InMemoryProfileStore {
    async fn save(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        snapshot: &SessionSnapshot,
    ) -> QuarryResult<()> {
        self.store
            .write()
            .await
            .insert(Self::key(org_id, profile_id), snapshot.clone());
        Ok(())
    }

    async fn load(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<SessionSnapshot>> {
        Ok(self
            .store
            .read()
            .await
            .get(&Self::key(org_id, profile_id))
            .cloned())
    }

    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
        let key = Self::key(org_id, profile_id);
        self.store.write().await.remove(&key);
        self.metadata.write().await.remove(&key);
        Ok(())
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>> {
        let prefix = format!("{org_id}\0");
        let metadata = self.metadata.read().await;
        Ok(self
            .store
            .read()
            .await
            .keys()
            .filter_map(|k| {
                let suffix = k.strip_prefix(&prefix)?;
                let profile_id: ProfileKind = suffix.parse().ok()?;
                let found = metadata.get(k).cloned().unwrap_or_default();
                Some(ProfileSummary {
                    profile_id,
                    name: found.name,
                    scope: found.scope,
                })
            })
            .collect())
    }

    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
    ) -> QuarryResult<()> {
        let key = Self::key(org_id, profile_id);
        // Mirror Postgres/S3: a profile "created" via metadata alone (no
        // browsing history yet) must still surface from `list()`, whose
        // InMemory implementation enumerates `store`'s keys. Seed a
        // placeholder snapshot only if one doesn't already exist so a
        // rename/rescope of a profile with real captured cookies never
        // clobbers them.
        self.store
            .write()
            .await
            .entry(key.clone())
            .or_insert_with(SessionSnapshot::default);
        self.metadata.write().await.insert(key, metadata.clone());
        Ok(())
    }

    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>> {
        Ok(self
            .metadata
            .read()
            .await
            .get(&Self::key(org_id, profile_id))
            .cloned())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<chrono::DateTime<chrono::Utc>>,
}

// reason: the items below the test module are `#[cfg(feature = "chromiumoxide")]`-gated
// imports/types used by `BrowserSession`; relocating the test module past them is awkward.
#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::ids::Id;

    #[tokio::test]
    async fn in_memory_store_save_load_roundtrip() {
        let store = InMemoryProfileStore::new();
        let profile_id: ProfileKind = Id::new();

        let snapshot = SessionSnapshot {
            cookies: vec![Cookie {
                name: "session".into(),
                value: "abc123".into(),
                domain: "example.com".into(),
                path: "/".into(),
                secure: true,
                http_only: true,
                expires: None,
            }],
            local_storage: vec![("theme".into(), "dark".into())],
            session_storage: vec![],
            indexed_db: vec![],
            user_agent: Some("Quarry/1.0".into()),
            viewport: Some(Viewport {
                width: 1920,
                height: 1080,
                device_scale_factor: 1.0,
                is_mobile: false,
            }),
            locale: Some("en-US".into()),
            timezone: Some("America/New_York".into()),
        };

        store.save("org_a", &profile_id, &snapshot).await.unwrap();
        let loaded = store.load("org_a", &profile_id).await.unwrap().unwrap();

        assert_eq!(loaded.cookies.len(), 1);
        assert_eq!(loaded.cookies[0].name, "session");
        assert_eq!(loaded.local_storage.len(), 1);
        assert_eq!(loaded.user_agent.as_deref(), Some("Quarry/1.0"));
        assert_eq!(loaded.viewport.as_ref().unwrap().width, 1920);
        assert_eq!(loaded.locale.as_deref(), Some("en-US"));
        assert_eq!(loaded.timezone.as_deref(), Some("America/New_York"));
    }

    #[tokio::test]
    async fn in_memory_store_load_missing_returns_none() {
        let store = InMemoryProfileStore::new();
        let profile_id: ProfileKind = Id::new();
        assert!(store.load("org_a", &profile_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_store_isolates_orgs() {
        // org_a saves a profile; org_b must NOT see it via load or list,
        // even when the profile_id ULID is identical (which it cannot be
        // in practice, but the test pins the contract).
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        let snapshot = SessionSnapshot {
            cookies: vec![],
            local_storage: vec![],
            session_storage: vec![],
            indexed_db: vec![],
            user_agent: None,
            viewport: None,
            locale: None,
            timezone: None,
        };
        store.save("org_a", &pid, &snapshot).await.unwrap();

        // Cross-tenant load: must miss.
        assert!(store.load("org_b", &pid).await.unwrap().is_none());

        // Cross-tenant list: must be empty for org_b.
        let other = store.list("org_b").await.unwrap();
        assert!(other.is_empty(), "org_b leaked org_a's profile");

        // Own-tenant list: must contain it.
        let mine = store.list("org_a").await.unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].profile_id.to_string(), pid.to_string());

        // Cross-tenant delete: must NOT remove org_a's profile.
        store.delete("org_b", &pid).await.unwrap();
        assert!(store.load("org_a", &pid).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn in_memory_store_delete_removes_entry() {
        let store = InMemoryProfileStore::new();
        let profile_id: ProfileKind = Id::new();
        let snapshot = SessionSnapshot::default();

        store.save("org_a", &profile_id, &snapshot).await.unwrap();
        assert!(store.load("org_a", &profile_id).await.unwrap().is_some());

        store.delete("org_a", &profile_id).await.unwrap();
        assert!(store.load("org_a", &profile_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_store_list_returns_all_profiles() {
        let store = InMemoryProfileStore::new();
        let p1: ProfileKind = Id::new();
        let p2: ProfileKind = Id::new();

        store
            .save("org_a", &p1, &SessionSnapshot::default())
            .await
            .unwrap();
        store
            .save("org_a", &p2, &SessionSnapshot::default())
            .await
            .unwrap();

        let list = store.list("org_a").await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn metadata_save_load_roundtrip() {
        let store = InMemoryProfileStore::new();
        let profile_id: ProfileKind = Id::new();
        store
            .save("org_a", &profile_id, &SessionSnapshot::default())
            .await
            .unwrap();

        assert!(store
            .load_metadata("org_a", &profile_id)
            .await
            .unwrap()
            .is_none());

        let metadata = ProfileMetadata {
            name: Some("Work Gmail".into()),
            scope: ProfileScope::UserPrivate,
        };
        store
            .save_metadata("org_a", &profile_id, &metadata)
            .await
            .unwrap();

        let loaded = store
            .load_metadata("org_a", &profile_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Work Gmail"));
        assert_eq!(loaded.scope, ProfileScope::UserPrivate);
    }

    #[tokio::test]
    async fn list_carries_name_and_scope_from_metadata() {
        let store = InMemoryProfileStore::new();
        let named: ProfileKind = Id::new();
        let unnamed: ProfileKind = Id::new();
        store
            .save("org_a", &named, &SessionSnapshot::default())
            .await
            .unwrap();
        store
            .save("org_a", &unnamed, &SessionSnapshot::default())
            .await
            .unwrap();
        store
            .save_metadata(
                "org_a",
                &named,
                &ProfileMetadata {
                    name: Some("Norwegian bank".into()),
                    scope: ProfileScope::OrgShared,
                },
            )
            .await
            .unwrap();

        let list = store.list("org_a").await.unwrap();
        let named_summary = list
            .iter()
            .find(|s| s.profile_id == named)
            .expect("named profile missing from list");
        assert_eq!(named_summary.name.as_deref(), Some("Norwegian bank"));
        assert_eq!(named_summary.scope, ProfileScope::OrgShared);

        // No metadata ever saved for `unnamed` — must default, not error.
        let unnamed_summary = list
            .iter()
            .find(|s| s.profile_id == unnamed)
            .expect("unnamed profile missing from list");
        assert!(unnamed_summary.name.is_none());
        assert_eq!(unnamed_summary.scope, ProfileScope::Ephemeral);
    }

    #[tokio::test]
    async fn metadata_isolates_orgs() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        store
            .save("org_a", &pid, &SessionSnapshot::default())
            .await
            .unwrap();
        store
            .save_metadata(
                "org_a",
                &pid,
                &ProfileMetadata {
                    name: Some("secret".into()),
                    scope: ProfileScope::UserPrivate,
                },
            )
            .await
            .unwrap();

        assert!(store.load_metadata("org_b", &pid).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_removes_metadata_too() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        store
            .save("org_a", &pid, &SessionSnapshot::default())
            .await
            .unwrap();
        store
            .save_metadata(
                "org_a",
                &pid,
                &ProfileMetadata {
                    name: Some("temp".into()),
                    scope: ProfileScope::RunScoped,
                },
            )
            .await
            .unwrap();

        store.delete("org_a", &pid).await.unwrap();
        assert!(store.load("org_a", &pid).await.unwrap().is_none());
        assert!(store.load_metadata("org_a", &pid).await.unwrap().is_none());
    }

    #[test]
    fn profile_scope_string_roundtrip() {
        for scope in [
            ProfileScope::Ephemeral,
            ProfileScope::UserPrivate,
            ProfileScope::OrgShared,
            ProfileScope::RunScoped,
        ] {
            let s = scope.to_string();
            let parsed: ProfileScope = s.parse().unwrap();
            assert_eq!(parsed, scope);
        }
    }

    #[test]
    fn profile_scope_default_is_ephemeral() {
        assert_eq!(ProfileScope::default(), ProfileScope::Ephemeral);
    }

    #[test]
    fn snapshot_serde_roundtrip() {
        let snapshot = SessionSnapshot {
            cookies: vec![Cookie {
                name: "sid".into(),
                value: "xyz".into(),
                domain: ".example.com".into(),
                path: "/app".into(),
                secure: true,
                http_only: false,
                expires: None,
            }],
            local_storage: vec![("k".into(), "v".into())],
            session_storage: vec![],
            indexed_db: vec![],
            user_agent: None,
            viewport: Some(Viewport {
                width: 375,
                height: 812,
                device_scale_factor: 3.0,
                is_mobile: true,
            }),
            locale: None,
            timezone: None,
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: SessionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.cookies[0].name, "sid");
        assert!(restored.viewport.unwrap().is_mobile);
    }
}

#[cfg(feature = "chromiumoxide")]
use anyhow::Result;
#[cfg(feature = "chromiumoxide")]
use chromiumoxide::Page;
#[cfg(feature = "chromiumoxide")]
use std::sync::Arc;

#[cfg(feature = "chromiumoxide")]
pub struct BrowserSession {
    pub page: Arc<Page>,
}

#[cfg(feature = "chromiumoxide")]
impl BrowserSession {
    pub async fn capture(&self) -> Result<SessionSnapshot> {
        let raw = self.page.get_cookies().await?;
        let cookies = raw
            .into_iter()
            .map(|c| Cookie {
                name: c.name,
                value: c.value,
                domain: c.domain,
                path: c.path,
                secure: c.secure,
                http_only: c.http_only,
                expires: None,
            })
            .collect();
        Ok(SessionSnapshot {
            cookies,
            local_storage: vec![],
            session_storage: vec![],
            indexed_db: vec![],
            user_agent: None,
            viewport: None,
            locale: None,
            timezone: None,
        })
    }

    pub async fn hydrate(&self, snapshot: &SessionSnapshot) -> Result<()> {
        let cookies: Vec<_> = snapshot
            .cookies
            .iter()
            .map(|c| {
                chromiumoxide::cdp::browser_protocol::network::CookieParam::builder()
                    .name(c.name.clone())
                    .value(c.value.clone())
                    .domain(c.domain.clone())
                    .path(c.path.clone())
                    .secure(c.secure)
                    .http_only(c.http_only)
                    .build()
                    .unwrap()
            })
            .collect();
        self.page.set_cookies(cookies).await?;
        Ok(())
    }
}
