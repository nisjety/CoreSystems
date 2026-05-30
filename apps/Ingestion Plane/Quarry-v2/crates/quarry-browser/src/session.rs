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
    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileKind>>;
}

pub struct InMemoryProfileStore {
    /// Composite key `{org_id}\0{profile_id}` keeps tenants strictly
    /// partitioned in a single hashmap. The NUL separator prevents any
    /// accidental key collision between an org named "alpha" with profile
    /// "foo" and an org named "alphafoo" with no profile suffix.
    store: tokio::sync::RwLock<std::collections::HashMap<String, SessionSnapshot>>,
}

impl InMemoryProfileStore {
    pub fn new() -> Self {
        Self {
            store: tokio::sync::RwLock::new(std::collections::HashMap::new()),
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
        self.store
            .write()
            .await
            .remove(&Self::key(org_id, profile_id));
        Ok(())
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileKind>> {
        let prefix = format!("{org_id}\0");
        Ok(self
            .store
            .read()
            .await
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix).and_then(|s| s.parse().ok()))
            .collect())
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
        assert_eq!(mine[0].to_string(), pid.to_string());

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
