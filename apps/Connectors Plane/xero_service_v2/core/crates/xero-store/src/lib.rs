//! `xero-store` — durable store for PKCE onboarding results: refresh tokens +
//! connected tenants. Shared by the onboarding BFF (writer) and the sync
//! backend (reader + refresh-token rotator).
//!
//! Local development uses an owner-only JSON file. Production uses a dedicated,
//! generation-guarded GCS object with a fenced cross-process refresh lease.

use std::{
    collections::HashMap,
    env,
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

mod gcs;
pub use gcs::GcsConnectionStore;

pub struct RefreshLock(pub(crate) RefreshLockKind);

pub(crate) enum RefreshLockKind {
    Local { file: File, owner: uuid::Uuid },
    Gcs(uuid::Uuid),
}

pub enum StoreConfig {
    File(PathBuf),
    Gcs {
        bucket: String,
        object: String,
        kms_key_name: Option<String>,
    },
}

impl StoreConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_map(&env::vars().collect())
    }

    fn from_map(vars: &HashMap<String, String>) -> anyhow::Result<Self> {
        match vars
            .get("XERO_CONNECTION_STORE_BACKEND")
            .map(String::as_str)
            .unwrap_or("file")
        {
            "file" => Ok(Self::File(PathBuf::from(
                vars.get("XERO_CONNECTION_STORE_PATH")
                    .map(String::as_str)
                    .unwrap_or("./data/connections.json"),
            ))),
            "gcs" => {
                let bucket = vars
                    .get("XERO_CONNECTION_STORE_GCS_BUCKET")
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| anyhow::anyhow!("XERO_CONNECTION_STORE_GCS_BUCKET is required"))?
                    .clone();
                for forbidden in ["GCS_BUCKET", "XERO_SENSITIVE_GCS_BUCKET"] {
                    anyhow::ensure!(
                        vars.get(forbidden) != Some(&bucket),
                        "token-store bucket must be separate from {forbidden}"
                    );
                }
                let object = vars
                    .get("XERO_CONNECTION_STORE_GCS_OBJECT")
                    .cloned()
                    .unwrap_or_else(|| "xero/pkce/connections.json".into());
                let kms_key_name = vars
                    .get("XERO_CONNECTION_STORE_KMS_KEY")
                    .filter(|value| !value.trim().is_empty())
                    .cloned();
                Ok(Self::Gcs {
                    bucket,
                    object,
                    kms_key_name,
                })
            }
            other => anyhow::bail!("unsupported XERO_CONNECTION_STORE_BACKEND: {other}"),
        }
    }
}

pub async fn open_from_env() -> anyhow::Result<Arc<dyn ConnectionStore>> {
    match StoreConfig::from_env()? {
        StoreConfig::File(path) => Ok(Arc::new(FileConnectionStore::load(path)?)),
        StoreConfig::Gcs {
            bucket,
            object,
            kms_key_name,
        } => Ok(Arc::new(
            GcsConnectionStore::new(bucket, object, kms_key_name).await?,
        )),
    }
}

/// A persisted connection: tenant metadata plus its refresh token.
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredConnection {
    pub tenant_id: String,
    pub tenant_name: String,
    pub tenant_type: String,
    pub connected_at: DateTime<Utc>,
    pub scopes: Vec<String>,
    /// Long-lived credential — persisted, but NEVER returned over the API.
    pub refresh_token: String,
}

#[async_trait]
pub trait ConnectionStore: Send + Sync {
    /// Insert or replace the record for a tenant (onboarding).
    async fn upsert(&self, conn: StoredConnection) -> anyhow::Result<()> {
        self.upsert_many(vec![conn]).await
    }
    /// Publish every tenant from one Xero authorization atomically.
    async fn upsert_many(&self, connections: Vec<StoredConnection>) -> anyhow::Result<()>;
    /// All stored connections (UI listing + the sync's tenant list).
    async fn list(&self) -> anyhow::Result<Vec<StoredConnection>>;
    /// The refresh token for a tenant, if connected (sync auth).
    async fn get_refresh_token(&self, tenant_id: &str) -> anyhow::Result<Option<String>>;
    /// Serialize refresh-token exchange across all workers sharing this store.
    async fn acquire_refresh_lock(&self) -> anyhow::Result<RefreshLock>;
    async fn release_refresh_lock(&self, lock: RefreshLock) -> anyhow::Result<()>;
    /// Rotate the whole authorization grant if the caller still owns its lease.
    async fn update_refresh_token(
        &self,
        lock: &RefreshLock,
        tenant_id: &str,
        previous_refresh_token: &str,
        refresh_token: &str,
    ) -> anyhow::Result<()>;
}

/// JSON-file-backed store. Keyed by tenant id; `upsert` replaces.
pub struct FileConnectionStore {
    path: PathBuf,
    lock_path: PathBuf,
    owner: uuid::Uuid,
}

impl FileConnectionStore {
    /// Load an existing store file, or start empty if it does not exist.
    pub fn load(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        read_store(&path)?;
        let name = path
            .file_name()
            .context("connection store path has no file name")?;
        let mut lock_name = name.to_os_string();
        lock_name.push(".lock");
        Ok(Self {
            lock_path: path.with_file_name(lock_name),
            path,
            owner: uuid::Uuid::new_v4(),
        })
    }

    fn persist(&self, map: &HashMap<String, StoredConnection>) -> anyhow::Result<()> {
        ensure_private_parent(&self.path)?;
        validate_store_path(&self.path)?;
        let mut list: Vec<&StoredConnection> = map.values().collect();
        list.sort_by(|a, b| a.tenant_id.cmp(&b.tenant_id));
        let json = serde_json::to_vec_pretty(&list)?;

        let (mut file, mut tmp) = create_private_temp(&self.path)?;
        file.write_all(&json)?;
        file.sync_all()?;
        drop(file);
        validate_store_path(&self.path)?;
        fs::rename(&tmp.path, &self.path)?;
        tmp.published = true;
        Ok(())
    }
}

fn read_store(path: &Path) -> anyhow::Result<HashMap<String, StoredConnection>> {
    ensure_private_parent(path)?;
    let Some(mut file) = open_existing_store(path)? else {
        return Ok(HashMap::new());
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let list: Vec<StoredConnection> = serde_json::from_slice(&bytes)?;
    Ok(list.into_iter().map(|c| (c.tenant_id.clone(), c)).collect())
}

fn ensure_private_parent(path: &Path) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

        match fs::symlink_metadata(parent) {
            Ok(meta) => anyhow::ensure!(
                !meta.file_type().is_symlink(),
                "connection store parent is a symlink: {}",
                parent.display()
            ),
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        let before = fs::symlink_metadata(parent)?;
        anyhow::ensure!(
            before.is_dir() && !before.file_type().is_symlink(),
            "connection store parent is not a directory"
        );
        let dir = File::open(parent)?;
        let opened = dir.metadata()?;
        anyhow::ensure!(
            opened.dev() == before.dev() && opened.ino() == before.ino(),
            "connection store parent changed while opening"
        );
        if opened.permissions().mode() & 0o777 != 0o700 {
            dir.set_permissions(fs::Permissions::from_mode(0o700))?;
        }
        anyhow::ensure!(
            dir.metadata()?.permissions().mode() & 0o777 == 0o700,
            "connection store parent is not private"
        );
    }
    #[cfg(not(unix))]
    fs::create_dir_all(parent)?;
    Ok(())
}

fn validate_store_path(path: &Path) -> anyhow::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_file(),
                "connection store path must be a regular file"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                anyhow::ensure!(
                    meta.permissions().mode() & 0o077 == 0,
                    "connection store file is not owner-only"
                );
            }
            Ok(Some(meta))
        }
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn open_existing_store(path: &Path) -> anyhow::Result<Option<File>> {
    let Some(before) = validate_store_path(path)? else {
        return Ok(None);
    };
    let file = File::open(path)?;
    let opened = file.metadata()?;
    anyhow::ensure!(
        opened.is_file(),
        "connection store path changed while opening"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            opened.dev() == before.dev()
                && opened.ino() == before.ino()
                && opened.permissions().mode() & 0o077 == 0,
            "connection store file changed or is not owner-only"
        );
    }
    #[cfg(not(unix))]
    let _ = before;
    Ok(Some(file))
}

fn open_lock_file(path: &Path) -> anyhow::Result<File> {
    ensure_private_parent(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(file) => Ok(file),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            let before = validate_store_path(path)?.context("connection store lock disappeared")?;
            let file = OpenOptions::new().read(true).write(true).open(path)?;
            let opened = file.metadata()?;
            anyhow::ensure!(
                opened.is_file(),
                "connection store lock is not a regular file"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                anyhow::ensure!(
                    opened.dev() == before.dev()
                        && opened.ino() == before.ino()
                        && opened.permissions().mode() & 0o077 == 0,
                    "connection store lock changed or is not owner-only"
                );
            }
            #[cfg(not(unix))]
            let _ = before;
            Ok(file)
        }
        Err(err) => Err(err.into()),
    }
}

fn lock_file(path: &Path) -> anyhow::Result<File> {
    let file = open_lock_file(path)?;
    file.lock()?;
    ensure_private_parent(path)?;
    let current = validate_store_path(path)?.context("connection store lock disappeared")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata()?;
        anyhow::ensure!(
            opened.dev() == current.dev() && opened.ino() == current.ino(),
            "connection store lock changed while waiting"
        );
    }
    #[cfg(not(unix))]
    let _ = current;
    Ok(file)
}

struct PendingTemp {
    path: PathBuf,
    published: bool,
}

impl Drop for PendingTemp {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn create_private_temp(path: &Path) -> anyhow::Result<(File, PendingTemp)> {
    let name = path
        .file_name()
        .context("connection store path has no file name")?;
    let mut temp_name = name.to_os_string();
    temp_name.push(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let temp_path = path.with_file_name(temp_name);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&temp_path)?;
    Ok((
        file,
        PendingTemp {
            path: temp_path,
            published: false,
        },
    ))
}

#[async_trait]
impl ConnectionStore for FileConnectionStore {
    async fn upsert_many(&self, connections: Vec<StoredConnection>) -> anyhow::Result<()> {
        anyhow::ensure!(
            connections
                .iter()
                .all(|conn| !conn.refresh_token.is_empty()),
            "refresh token must not be empty"
        );
        let _refresh_lock = self.acquire_refresh_lock().await?;
        let mut next = read_store(&self.path)?;
        for conn in connections {
            next.insert(conn.tenant_id.clone(), conn);
        }
        self.persist(&next)?;
        Ok(())
    }

    async fn list(&self) -> anyhow::Result<Vec<StoredConnection>> {
        let map = read_store(&self.path)?;
        let mut list: Vec<StoredConnection> = map.values().cloned().collect();
        list.sort_by(|a, b| a.tenant_name.cmp(&b.tenant_name));
        Ok(list)
    }

    async fn get_refresh_token(&self, tenant_id: &str) -> anyhow::Result<Option<String>> {
        let map = read_store(&self.path)?;
        Ok(map.get(tenant_id).map(|c| c.refresh_token.clone()))
    }

    async fn acquire_refresh_lock(&self) -> anyhow::Result<RefreshLock> {
        let path = self.lock_path.clone();
        let file = tokio::task::spawn_blocking(move || lock_file(&path)).await??;
        Ok(RefreshLock(RefreshLockKind::Local {
            file,
            owner: self.owner,
        }))
    }

    async fn release_refresh_lock(&self, lock: RefreshLock) -> anyhow::Result<()> {
        match lock.0 {
            RefreshLockKind::Local { file, owner } => {
                anyhow::ensure!(owner == self.owner, "lock backend mismatch");
                drop(file);
                Ok(())
            }
            RefreshLockKind::Gcs(_) => anyhow::bail!("lock backend mismatch"),
        }
    }

    async fn update_refresh_token(
        &self,
        lock: &RefreshLock,
        tenant_id: &str,
        previous_refresh_token: &str,
        refresh_token: &str,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(lock.0, RefreshLockKind::Local { owner, .. } if owner == self.owner),
            "lock backend mismatch"
        );
        anyhow::ensure!(!refresh_token.is_empty(), "refresh token must not be empty");
        let map = read_store(&self.path)?;
        let conn = map
            .get(tenant_id)
            .ok_or_else(|| anyhow::anyhow!("tenant not connected"))?;
        anyhow::ensure!(
            conn.refresh_token == previous_refresh_token,
            "stale refresh token"
        );
        let next = map
            .iter()
            .map(|(id, conn)| {
                let updated = if conn.refresh_token == previous_refresh_token {
                    StoredConnection {
                        refresh_token: refresh_token.to_owned(),
                        ..conn.clone()
                    }
                } else {
                    conn.clone()
                };
                (id.clone(), updated)
            })
            .collect();
        self.persist(&next)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn temporary_file_is_owner_only_at_creation_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("xstore-temp-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let (file, pending) = create_private_temp(&dir.join("connections.json")).unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        assert!(pending.path.exists());
        let path = pending.path.clone();
        drop(file);
        drop(pending);
        assert!(!path.exists());
        fs::remove_dir(&dir).unwrap();
    }

    fn rec(tenant: &str, token: &str) -> StoredConnection {
        StoredConnection {
            tenant_id: tenant.to_owned(),
            tenant_name: format!("Org {tenant}"),
            tenant_type: "ORGANISATION".to_owned(),
            connected_at: Utc::now(),
            scopes: vec!["offline_access".to_owned()],
            refresh_token: token.to_owned(),
        }
    }

    #[tokio::test]
    async fn upsert_rotate_and_reload() {
        let dir = std::env::temp_dir().join(format!("xstore-{}", std::process::id()));
        let path = dir.join("connections.json");
        let store = FileConnectionStore::load(&path).unwrap();

        store.upsert(rec("t1", "r1")).await.unwrap();
        store.upsert(rec("t2", "r3")).await.unwrap();
        let lock = store.acquire_refresh_lock().await.unwrap();
        store
            .update_refresh_token(&lock, "t1", "r1", "r2")
            .await
            .unwrap();
        store.release_refresh_lock(lock).await.unwrap();

        let reloaded = FileConnectionStore::load(&path).unwrap();
        assert_eq!(reloaded.list().await.unwrap().len(), 2);
        assert_eq!(
            reloaded.get_refresh_token("t1").await.unwrap().as_deref(),
            Some("r2")
        );
        assert_eq!(reloaded.get_refresh_token("absent").await.unwrap(), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn refresh_rotation_updates_every_tenant_on_the_same_grant() {
        let dir = std::env::temp_dir().join(format!(
            "xstore-grant-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let path = dir.join("connections.json");
        let store = FileConnectionStore::load(&path).unwrap();
        store
            .upsert_many(vec![rec("t1", "shared"), rec("t2", "shared")])
            .await
            .unwrap();

        let lock = store.acquire_refresh_lock().await.unwrap();
        store
            .update_refresh_token(&lock, "t1", "shared", "rotated")
            .await
            .unwrap();
        assert_eq!(
            store.get_refresh_token("t2").await.unwrap().as_deref(),
            Some("rotated")
        );
        assert!(store
            .update_refresh_token(&lock, "t1", "shared", "stale")
            .await
            .is_err());
        assert_eq!(
            store.get_refresh_token("t1").await.unwrap().as_deref(),
            Some("rotated")
        );
        store.release_refresh_lock(lock).await.unwrap();

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn onboarding_waits_for_an_active_refresh() {
        let dir = std::env::temp_dir().join(format!(
            "xstore-concurrent-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let store = Arc::new(FileConnectionStore::load(dir.join("connections.json")).unwrap());
        store.upsert(rec("t1", "old")).await.unwrap();
        let lock = store.acquire_refresh_lock().await.unwrap();

        let pending_store = Arc::clone(&store);
        let pending =
            tokio::spawn(async move { pending_store.upsert(rec("t1", "new-grant")).await });
        tokio::task::yield_now().await;
        assert!(!pending.is_finished(), "onboarding must wait for refresh");

        store
            .update_refresh_token(&lock, "t1", "old", "rotated")
            .await
            .unwrap();
        store.release_refresh_lock(lock).await.unwrap();
        pending.await.unwrap().unwrap();
        assert_eq!(
            store.get_refresh_token("t1").await.unwrap().as_deref(),
            Some("new-grant")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn production_store_requires_a_dedicated_bucket() {
        let vars = HashMap::from([
            ("XERO_CONNECTION_STORE_BACKEND".into(), "gcs".into()),
            ("XERO_CONNECTION_STORE_GCS_BUCKET".into(), "raw-data".into()),
            ("GCS_BUCKET".into(), "raw-data".into()),
        ]);
        assert!(StoreConfig::from_map(&vars).is_err());
        let vars = HashMap::from([
            ("XERO_CONNECTION_STORE_BACKEND".into(), "gcs".into()),
            (
                "XERO_CONNECTION_STORE_GCS_BUCKET".into(),
                "private-tokens".into(),
            ),
            ("GCS_BUCKET".into(), "raw-data".into()),
        ]);
        assert!(matches!(
            StoreConfig::from_map(&vars).unwrap(),
            StoreConfig::Gcs { .. }
        ));
    }
}
