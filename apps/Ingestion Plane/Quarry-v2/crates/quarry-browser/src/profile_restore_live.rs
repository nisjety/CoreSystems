// WS17.C2: restore against a real Chromium. This file is `include!`d at the end of `mod tests`
// in chromiumoxide.rs, so it shares that module's helpers (`live_driver`, `live_enabled`,
// `make_lease_with_key`, `profile_v2_run`).
//
// Run (the two store-failure tests need no network; the others need the public network,
// because the navigation guard refuses loopback):
//   CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium \
//   cargo test -p quarry-browser --features chromiumoxide restore_ -- --test-threads=1 --ignored --nocapture
//
// NOT RUN when this was written (build-first rule). The evidence run is the final test phase
// of wave 2, in the rebuilt edge image's Chromium.

/// A store that fails its `load` with a chosen error and counts its `save` calls. Everything
/// else is an in-memory store.
struct FailingLoadStore {
    error: QuarryError,
    inner: crate::session::InMemoryProfileStore,
    saves: AtomicUsize,
}

impl FailingLoadStore {
    fn new(error: QuarryError) -> Self {
        Self {
            error,
            inner: crate::session::InMemoryProfileStore::new(),
            saves: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ProfileStore for FailingLoadStore {
    async fn save(
        &self,
        org_id: &str,
        profile_id: &quarry_core::ids::kinds::ProfileKind,
        snapshot: &SessionSnapshot,
        meta: &crate::session::SaveMeta,
    ) -> QuarryResult<crate::session::SaveOutcome> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        self.inner.save(org_id, profile_id, snapshot, meta).await
    }
    async fn load(
        &self,
        _org_id: &str,
        _profile_id: &quarry_core::ids::kinds::ProfileKind,
    ) -> QuarryResult<Option<SessionSnapshot>> {
        Err(self.error.clone())
    }
    async fn delete(
        &self,
        org_id: &str,
        profile_id: &quarry_core::ids::kinds::ProfileKind,
    ) -> QuarryResult<()> {
        self.inner.delete(org_id, profile_id).await
    }
    async fn list(&self, org_id: &str) -> QuarryResult<Vec<crate::session::ProfileSummary>> {
        self.inner.list(org_id).await
    }
    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &quarry_core::ids::kinds::ProfileKind,
        metadata: &crate::session::ProfileMetadata,
    ) -> QuarryResult<()> {
        self.inner.save_metadata(org_id, profile_id, metadata).await
    }
    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &quarry_core::ids::kinds::ProfileKind,
    ) -> QuarryResult<Option<crate::session::ProfileMetadata>> {
        self.inner.load_metadata(org_id, profile_id).await
    }
}

fn restore_cookie(name: &str, value: &str, domain: &str, days: i64) -> crate::session::Cookie {
    crate::session::Cookie {
        name: name.to_owned(),
        value: value.to_owned(),
        domain: domain.to_owned(),
        path: "/".to_owned(),
        expires: Some(chrono::Utc::now() + chrono::Duration::days(days)),
        ..Default::default()
    }
}

/// A persistent session over `store`, egress open to the example and httpbin origins.
async fn restore_session(
    driver: &ChromiumoxideDriver,
    key: &str,
    lease_profile: Option<quarry_core::ids::kinds::ProfileKind>,
) -> BrowserSession {
    let mut lease = make_lease_with_key(key);
    lease.persist_profile = true;
    if let Some(profile) = lease_profile {
        lease.profile_id = profile;
    }
    let session = driver.acquire(&lease).await.expect("acquire");
    driver
        .configure_egress_policy(
            &session,
            BrowserEgressPolicy::from_allowed_domains(&[
                "example.com".to_owned(),
                "example.org".to_owned(),
                "httpbin.org".to_owned(),
            ]),
        )
        .await
        .expect("install egress policy");
    session
}

/// The visible text of the current tab.
async fn restore_page_text(driver: &ChromiumoxideDriver, session: &BrowserSession) -> String {
    driver
        .evaluate(session, "document.body ? document.body.innerText : ''")
        .await
        .expect("page text")
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn key_unavailable_aborts_acquire_with_503() {
    if !live_enabled() {
        return;
    }
    let store = Arc::new(FailingLoadStore::new(QuarryError::new(
        ErrorCode::RuntimeNotReady,
        "profile key unavailable",
    )));
    let driver = live_driver().with_profile_store(store.clone());
    let session = restore_session(&driver, "ws17-c2-key-unavailable", None).await;
    let error = driver
        .new_tab(&session, None)
        .await
        .expect_err("the first tab must not start fresh over an unreadable profile");
    assert_eq!(error.code, ErrorCode::RuntimeNotReady);
    assert_eq!(error.code.http_status(), 503);
    // Nothing was saved, so the stored profile is untouched.
    let _ = driver.release(session).await;
    assert_eq!(store.saves.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn corrupt_profile_starts_fresh_and_does_not_overwrite() {
    if !live_enabled() {
        return;
    }
    let store = Arc::new(FailingLoadStore::new(QuarryError::new(
        ErrorCode::Internal,
        "profile snapshot decode failed",
    )));
    let driver = live_driver().with_profile_store(store.clone());
    let session = restore_session(&driver, "ws17-c2-corrupt", None).await;
    driver
        .new_tab(&session, None)
        .await
        .expect("a corrupt profile starts the session fresh");
    driver.release(session).await.expect("release");
    assert_eq!(
        store.saves.load(Ordering::SeqCst),
        0,
        "the release must not save over a profile that could not be read"
    );
}

#[tokio::test]
#[ignore = "needs a public origin: httpbin.org over the public network"]
async fn restore_applies_cookies_before_the_first_request() {
    if !live_enabled() {
        return;
    }
    let store = Arc::new(crate::session::InMemoryProfileStore::new());
    let profile = quarry_core::ids::kinds::ProfileKind::new();
    let snapshot = SessionSnapshot {
        cookies: vec![
            restore_cookie("quarry_e2e", "CANARY-d16-c2", "httpbin.org", 1),
            restore_cookie("quarry_old", "expired", "httpbin.org", -1),
        ],
        captured_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    store
        .save("test_org", &profile, &snapshot, &crate::session::SaveMeta::now(None))
        .await
        .expect("seed");
    let driver = live_driver().with_profile_store(store.clone());
    let session = restore_session(&driver, "ws17-c2-first-request", Some(profile)).await;
    driver
        .new_tab(&session, Some("https://httpbin.org/cookies"))
        .await
        .expect("first tab");
    let text = restore_page_text(&driver, &session).await;
    assert!(text.contains("quarry_e2e"), "the first request carried the cookie: {text}");
    assert!(!text.contains("quarry_old"), "an expired cookie is not restored");
    // One navigation request for the restore URL: no reload.
    let receipts = driver.egress_receipts(&session, 0, 200).await.expect("receipts");
    let document_requests = receipts
        .iter()
        .filter(|receipt| receipt.url.starts_with("https://httpbin.org/cookies"))
        .count();
    assert_eq!(document_requests, 1, "a single request for the restore URL");
    // The session reports what it restored: counts only.
    let telemetry = driver.telemetry(&session).await.expect("telemetry");
    let summary = telemetry.profile_restore.expect("a restore summary");
    assert_eq!(summary.cookies_total, 2);
    assert_eq!(summary.cookies_applied, 1);
    assert_eq!(summary.skipped_expired, 1);
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "needs a public origin: httpbin.org over the public network"]
async fn second_tab_does_not_rehydrate_stale_profile_cookies() {
    if !live_enabled() {
        return;
    }
    let store = Arc::new(crate::session::InMemoryProfileStore::new());
    let profile = quarry_core::ids::kinds::ProfileKind::new();
    let snapshot = SessionSnapshot {
        cookies: vec![restore_cookie("quarry_e2e", "OLD", "httpbin.org", 1)],
        captured_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    store
        .save("test_org", &profile, &snapshot, &crate::session::SaveMeta::now(None))
        .await
        .expect("seed");
    let driver = live_driver().with_profile_store(store.clone());
    let session = restore_session(&driver, "ws17-c2-second-tab", Some(profile)).await;
    driver
        .new_tab(&session, Some("https://httpbin.org/cookies"))
        .await
        .expect("first tab");
    // The live jar moves on: a newer value is set.
    profile_v2_run(&driver, &session, "document.cookie = 'quarry_e2e=NEW; path=/; max-age=86400'").await;
    driver
        .new_tab(&session, Some("https://httpbin.org/cookies"))
        .await
        .expect("second tab");
    let text = restore_page_text(&driver, &session).await;
    assert!(text.contains("NEW"), "the second tab sees the live value: {text}");
    assert!(!text.contains("OLD"), "the second tab did not write the saved copy back");
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "needs a public origin: example.com and example.org over the public network"]
async fn localstorage_of_second_origin_is_seeded_on_first_visit_only() {
    if !live_enabled() {
        return;
    }
    let store = Arc::new(crate::session::InMemoryProfileStore::new());
    let profile = quarry_core::ids::kinds::ProfileKind::new();
    let snapshot = SessionSnapshot {
        snapshot_version: 2,
        origins: vec![
            crate::profile_snapshot::OriginState {
                origin: "https://example.com".to_owned(),
                local_storage: vec![("a".to_owned(), "1".to_owned())],
                ..Default::default()
            },
            crate::profile_snapshot::OriginState {
                origin: "https://example.org".to_owned(),
                local_storage: vec![("b".to_owned(), "2".to_owned())],
                ..Default::default()
            },
        ],
        captured_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    store
        .save("test_org", &profile, &snapshot, &crate::session::SaveMeta::now(None))
        .await
        .expect("seed");
    let driver = live_driver().with_profile_store(store.clone());
    let session = restore_session(&driver, "ws17-c2-second-origin", Some(profile)).await;
    driver
        .new_tab(&session, Some("https://example.com/"))
        .await
        .expect("first origin");
    // The second origin is seeded when it is first visited, not before.
    driver
        .new_tab(&session, Some("https://example.org/"))
        .await
        .expect("second origin");
    let value = driver
        .evaluate(&session, "localStorage.getItem('b')")
        .await
        .expect("read");
    assert_eq!(value.as_str(), Some("2"));
    // Changed by the page, then revisited: the sentinel stops a second seeding.
    profile_v2_run(&driver, &session, "localStorage.setItem('b', 'changed')").await;
    driver
        .new_tab(&session, Some("https://example.org/"))
        .await
        .expect("revisit");
    let value = driver
        .evaluate(&session, "localStorage.getItem('b')")
        .await
        .expect("read");
    assert_eq!(value.as_str(), Some("changed"));
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "needs a public origin: example.com over the public network"]
async fn indexeddb_roundtrip_across_sessions() {
    if !live_enabled() {
        return;
    }
    std::env::set_var(crate::profile_snapshot::CAPTURE_V2_ENV, "1");
    let store = Arc::new(crate::session::InMemoryProfileStore::new());
    let profile = quarry_core::ids::kinds::ProfileKind::new();
    let driver = live_driver().with_profile_store(store.clone());
    // Session one writes a record and saves.
    let first = restore_session(&driver, "ws17-c2-idb-1", Some(profile.clone())).await;
    driver
        .new_tab(&first, Some("https://example.com/"))
        .await
        .expect("origin");
    profile_v2_run(
        &driver,
        &first,
        r#"(async () => {
          const db = await new Promise((res, rej) => {
            const r = indexedDB.open('c2-db', 1);
            r.onupgradeneeded = () => { r.result.createObjectStore('items', { keyPath: 'id' }); };
            r.onsuccess = () => res(r.result);
            r.onerror = () => rej(r.error);
          });
          await new Promise((res, rej) => {
            const tx = db.transaction('items', 'readwrite');
            tx.objectStore('items').put({ id: 7, when: new Date(1700000000000), bytes: new Uint8Array([9, 8]) });
            tx.oncomplete = () => res();
            tx.onerror = () => rej(tx.error);
          });
          db.close();
        })()"#,
    )
    .await;
    driver.release(first).await.expect("release one");
    // Session two reads it back on first visit.
    let second = restore_session(&driver, "ws17-c2-idb-2", Some(profile)).await;
    driver
        .new_tab(&second, Some("https://example.com/"))
        .await
        .expect("origin again");
    let read = driver
        .evaluate(
            &second,
            r#"(async () => {
              const db = await new Promise((res, rej) => {
                const r = indexedDB.open('c2-db');
                r.onsuccess = () => res(r.result);
                r.onerror = () => rej(r.error);
              });
              const rec = await new Promise((res, rej) => {
                const q = db.transaction('items').objectStore('items').get(7);
                q.onsuccess = () => res(q.result);
                q.onerror = () => rej(q.error);
              });
              db.close();
              return rec ? [rec.id, rec.when instanceof Date, rec.bytes.length] : null;
            })()"#,
        )
        .await
        .expect("read back");
    assert_eq!(read, serde_json::json!([7, true, 2]));
    driver.release(second).await.expect("release two");
}

#[tokio::test]
#[ignore = "needs a public origin: httpbin.org over the public network"]
async fn restore_works_for_a_profile_saved_by_v1() {
    if !live_enabled() {
        return;
    }
    // A snapshot as capture v1 wrote it: no version, no origins, flat web storage, a cookie
    // without SameSite or an expiry field beyond the date.
    let fixture = serde_json::json!({
        "cookies": [{
            "name": "quarry_v1", "value": "LEGACY", "domain": "httpbin.org", "path": "/",
            "secure": false, "http_only": false
        }],
        "local_storage": [["legacy_key", "legacy_value"]],
        "session_storage": []
    });
    let snapshot: SessionSnapshot = serde_json::from_value(fixture).expect("a v1 snapshot parses");
    assert_eq!(snapshot.snapshot_version, 0);
    let store = Arc::new(crate::session::InMemoryProfileStore::new());
    let profile = quarry_core::ids::kinds::ProfileKind::new();
    store
        .save("test_org", &profile, &snapshot, &crate::session::SaveMeta::now(None))
        .await
        .expect("seed");
    let driver = live_driver().with_profile_store(store.clone());
    let session = restore_session(&driver, "ws17-c2-v1-fixture", Some(profile)).await;
    driver
        .new_tab(&session, Some("https://httpbin.org/cookies"))
        .await
        .expect("first tab");
    let text = restore_page_text(&driver, &session).await;
    assert!(text.contains("quarry_v1"), "the legacy cookie came back: {text}");
    let value = driver
        .evaluate(&session, "localStorage.getItem('legacy_key')")
        .await
        .expect("read");
    assert_eq!(value.as_str(), Some("legacy_value"), "flat entries seed the initial origin");
    driver.release(session).await.expect("release");
}
