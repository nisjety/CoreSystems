#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
};

use chrono::Utc;
use xero_store::{ConnectionStore, FileConnectionStore, StoredConnection};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("xero-store-security-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn connection() -> StoredConnection {
    StoredConnection {
        tenant_id: "tenant".into(),
        tenant_name: "Tenant".into(),
        tenant_type: "ORGANISATION".into(),
        connected_at: Utc::now(),
        scopes: vec!["offline_access".into()],
        refresh_token: "test-token".into(),
    }
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn startup_makes_the_parent_private_before_the_first_write() {
    let dir = TestDir::new();
    let parent = dir.path().join("mcp");
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
    let path = parent.join("connections.json");

    let store = FileConnectionStore::load(&path).unwrap();
    assert_eq!(mode(&parent), 0o700);
    store.upsert(connection()).await.unwrap();
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&parent.join("connections.json.lock")), 0o600);
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 2);
}

#[test]
fn startup_creates_a_private_parent() {
    let dir = TestDir::new();
    let parent = dir.path().join("mcp");
    FileConnectionStore::load(parent.join("connections.json")).unwrap();
    assert_eq!(mode(&parent), 0o700);
}

#[test]
fn load_rejects_symlinks_and_loose_mode_files() {
    let dir = TestDir::new();
    let path = dir.path().join("connections.json");
    let source = dir.path().join("source.json");
    fs::write(&source, "[]").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&source, &path).unwrap();
    assert!(FileConnectionStore::load(&path).is_err());

    fs::remove_file(&path).unwrap();
    fs::write(&path, "[]").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(FileConnectionStore::load(&path).is_err());
}

#[test]
fn load_rejects_a_symlinked_parent() {
    let dir = TestDir::new();
    let actual = dir.path().join("actual");
    fs::create_dir(&actual).unwrap();
    let link = dir.path().join("mcp");
    symlink(&actual, &link).unwrap();
    assert!(FileConnectionStore::load(link.join("connections.json")).is_err());
}

#[tokio::test]
async fn predictable_temp_symlink_is_not_followed_or_removed() {
    let dir = TestDir::new();
    let path = dir.path().join("connections.json");
    let victim = dir.path().join("victim");
    fs::write(&victim, "unchanged").unwrap();
    let trap = path.with_extension("json.tmp");
    symlink(&victim, &trap).unwrap();

    let store = FileConnectionStore::load(&path).unwrap();
    store.upsert(connection()).await.unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "unchanged");
    assert!(fs::symlink_metadata(&trap)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(mode(&path), 0o600);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 4);
}

#[tokio::test]
async fn failed_publish_cleans_up_its_unique_temporary_file() {
    let dir = TestDir::new();
    let path = dir.path().join("connections.json");
    let store = FileConnectionStore::load(&path).unwrap();
    fs::create_dir(&path).unwrap();

    assert!(store.upsert(connection()).await.is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    assert!(store.list().await.is_err());
}

#[tokio::test]
async fn lock_file_rejects_symlinks_and_loose_permissions() {
    let dir = TestDir::new();
    let path = dir.path().join("connections.json");
    let lock_path = dir.path().join("connections.json.lock");
    let victim = dir.path().join("victim");
    fs::write(&victim, "unchanged").unwrap();
    symlink(&victim, &lock_path).unwrap();

    let store = FileConnectionStore::load(&path).unwrap();
    assert!(store.upsert(connection()).await.is_err());
    assert_eq!(fs::read_to_string(&victim).unwrap(), "unchanged");
    assert!(!path.exists());

    fs::remove_file(&lock_path).unwrap();
    fs::write(&lock_path, "").unwrap();
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.acquire_refresh_lock().await.is_err());
    assert_eq!(mode(&lock_path), 0o644);
}
