use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use chrono::Utc;
use xero_store::{ConnectionStore, FileConnectionStore, StoredConnection};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("xero-store-concurrency-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn store_path(&self) -> PathBuf {
        self.0.join("connections.json")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn connection(tenant_id: &str, refresh_token: &str) -> StoredConnection {
    StoredConnection {
        tenant_id: tenant_id.into(),
        tenant_name: tenant_id.into(),
        tenant_type: "ORGANISATION".into(),
        connected_at: Utc::now(),
        scopes: vec!["offline_access".into()],
        refresh_token: refresh_token.into(),
    }
}

#[tokio::test]
async fn independently_loaded_stores_see_rotations_and_reject_stale_tokens() {
    let dir = TestDir::new();
    let first = FileConnectionStore::load(dir.store_path()).unwrap();
    first
        .upsert_many(vec![connection("a", "shared"), connection("b", "shared")])
        .await
        .unwrap();
    let second = FileConnectionStore::load(dir.store_path()).unwrap();

    let lock = first.acquire_refresh_lock().await.unwrap();
    first
        .update_refresh_token(&lock, "a", "shared", "rotated")
        .await
        .unwrap();
    first.release_refresh_lock(lock).await.unwrap();

    assert_eq!(
        second.get_refresh_token("a").await.unwrap().as_deref(),
        Some("rotated")
    );
    assert_eq!(
        second.get_refresh_token("b").await.unwrap().as_deref(),
        Some("rotated")
    );
    let lock = second.acquire_refresh_lock().await.unwrap();
    assert!(second
        .update_refresh_token(&lock, "a", "shared", "stale")
        .await
        .is_err());
    second.release_refresh_lock(lock).await.unwrap();
}

#[tokio::test]
async fn stale_instance_upsert_cannot_overwrite_a_rotation() {
    let dir = TestDir::new();
    let first = FileConnectionStore::load(dir.store_path()).unwrap();
    first.upsert(connection("a", "original")).await.unwrap();
    let second = FileConnectionStore::load(dir.store_path()).unwrap();

    let lock = first.acquire_refresh_lock().await.unwrap();
    first
        .update_refresh_token(&lock, "a", "original", "rotated")
        .await
        .unwrap();
    first.release_refresh_lock(lock).await.unwrap();
    second.upsert(connection("b", "other-grant")).await.unwrap();

    let reloaded = FileConnectionStore::load(dir.store_path()).unwrap();
    assert_eq!(
        reloaded.get_refresh_token("a").await.unwrap().as_deref(),
        Some("rotated")
    );
    assert_eq!(
        reloaded.get_refresh_token("b").await.unwrap().as_deref(),
        Some("other-grant")
    );
}

#[tokio::test]
async fn independent_stores_serialize_refresh_and_onboarding() {
    let dir = TestDir::new();
    let first = FileConnectionStore::load(dir.store_path()).unwrap();
    first.upsert(connection("a", "original")).await.unwrap();
    let second = Arc::new(FileConnectionStore::load(dir.store_path()).unwrap());

    let lock = first.acquire_refresh_lock().await.unwrap();
    let pending_store = Arc::clone(&second);
    let pending = tokio::spawn(async move { pending_store.upsert(connection("b", "other")).await });
    assert!(tokio::time::timeout(Duration::from_millis(100), async {
        while !pending.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .is_err());

    first
        .update_refresh_token(&lock, "a", "original", "rotated")
        .await
        .unwrap();
    first.release_refresh_lock(lock).await.unwrap();
    pending.await.unwrap().unwrap();
    assert_eq!(
        second.get_refresh_token("a").await.unwrap().as_deref(),
        Some("rotated")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn lock_file_remains_stable_when_store_file_is_replaced() {
    use std::os::unix::fs::MetadataExt;

    let dir = TestDir::new();
    let path = dir.store_path();
    let lock_path = dir.0.join("connections.json.lock");
    let store = FileConnectionStore::load(&path).unwrap();
    store.upsert(connection("a", "original")).await.unwrap();
    let store_inode = fs::metadata(&path).unwrap().ino();
    let lock_inode = fs::metadata(&lock_path).unwrap().ino();

    let lock = store.acquire_refresh_lock().await.unwrap();
    store
        .update_refresh_token(&lock, "a", "original", "rotated")
        .await
        .unwrap();
    store.release_refresh_lock(lock).await.unwrap();

    assert_ne!(fs::metadata(&path).unwrap().ino(), store_inode);
    assert_eq!(fs::metadata(&lock_path).unwrap().ino(), lock_inode);
}
