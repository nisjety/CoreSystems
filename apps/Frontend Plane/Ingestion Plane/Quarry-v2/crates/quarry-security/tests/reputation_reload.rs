//! W3-B3-01 (E3.8.6): reputation snapshot hot reload, dot-suffix matching.
//! Compile-checked only in this wave; run with `cargo test -p quarry-security`.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use quarry_security::preflight::{spawn_snapshot_watch, DefaultEngine, ListCounts};
use quarry_security::{Decision, SecurityEngine};
use url::Url;

fn u(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn temp_snapshot(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("quarry_reputation_{tag}_{nanos}.json"))
}

fn write_snapshot(path: &PathBuf, blocklist: &[&str], allowlist: &[&str]) {
    let body = serde_json::json!({ "blocklist": blocklist, "allowlist": allowlist });
    std::fs::write(path, serde_json::to_vec(&body).unwrap()).unwrap();
}

#[tokio::test]
async fn reload_replaces_both_lists_and_reports_counts() {
    let path = temp_snapshot("replace");
    write_snapshot(&path, &["bad.example"], &[]);
    let engine = DefaultEngine::new()
        .with_persistence(path.clone())
        .await
        .unwrap();
    assert_eq!(
        engine.preflight(&u("https://bad.example/")).await.decision,
        Decision::Block
    );

    write_snapshot(&path, &["worse.example", "worst.example"], &["ok.example"]);
    let counts = engine.reload_snapshot(&path).await.unwrap();
    assert_eq!(
        counts,
        ListCounts {
            blocklist: 2,
            allowlist: 1
        }
    );
    assert_eq!(engine.list_counts().await, counts);
    assert_ne!(
        engine.preflight(&u("https://bad.example/")).await.decision,
        Decision::Block
    );
    assert_eq!(
        engine
            .preflight(&u("https://worse.example/"))
            .await
            .decision,
        Decision::Block
    );

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn failed_reload_keeps_previous_lists() {
    let path = temp_snapshot("bad_json");
    write_snapshot(&path, &["bad.example"], &[]);
    let engine = DefaultEngine::new()
        .with_persistence(path.clone())
        .await
        .unwrap();

    std::fs::write(&path, b"{ not json").unwrap();
    let err = engine.reload_snapshot(&path).await.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(
        engine.preflight(&u("https://bad.example/")).await.decision,
        Decision::Block
    );

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn subdomain_matching_is_off_by_default() {
    let path = temp_snapshot("sub_off");
    write_snapshot(&path, &["evil.example"], &[]);
    let engine = DefaultEngine::new()
        .with_persistence(path.clone())
        .await
        .unwrap();
    assert_eq!(
        engine
            .preflight(&u("https://a.b.evil.example/"))
            .await
            .decision,
        Decision::Allow
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn subdomain_matching_blocks_children_on_a_dot_boundary() {
    let path = temp_snapshot("sub_on");
    write_snapshot(&path, &["evil.example"], &[]);
    let engine = DefaultEngine::new()
        .with_persistence(path.clone())
        .await
        .unwrap()
        .with_subdomain_matching(true);
    assert_eq!(
        engine
            .preflight(&u("https://a.b.evil.example/"))
            .await
            .decision,
        Decision::Block
    );
    assert_eq!(
        engine.preflight(&u("https://evil.example/")).await.decision,
        Decision::Block
    );
    // A label boundary is required: notevil.example is a different domain.
    assert_eq!(
        engine
            .preflight(&u("https://notevil.example/"))
            .await
            .decision,
        Decision::Allow
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn watcher_reloads_after_the_file_changes() {
    let path = temp_snapshot("watch");
    write_snapshot(&path, &["first.example"], &[]);
    let engine = DefaultEngine::new()
        .with_persistence(path.clone())
        .await
        .unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = spawn_snapshot_watch(
        &engine,
        path.clone(),
        Duration::from_secs(1),
        move |result| {
            let _ = tx.send(result.map_err(|kind| format!("{kind:?}")));
        },
    );

    // Sleep past the mtime granularity so the rewrite gets a new timestamp.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    write_snapshot(&path, &["second.example"], &["ok.example"]);

    let reported = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("watcher reported a reload within 5s")
        .expect("channel open");
    assert_eq!(
        reported,
        Ok(ListCounts {
            blocklist: 1,
            allowlist: 1
        })
    );
    assert_eq!(
        engine
            .preflight(&u("https://second.example/"))
            .await
            .decision,
        Decision::Block
    );
    assert_ne!(
        engine
            .preflight(&u("https://first.example/"))
            .await
            .decision,
        Decision::Block
    );

    handle.abort();
    let _ = std::fs::remove_file(&path);
}
