// WS17.C1: capture v2 against a real Chromium. This file is `include!`d at the end of
// `mod tests` in chromiumoxide.rs, so it shares that module's helpers (`live_driver`,
// `live_enabled`, `make_lease_with_key`).
//
// Run (public network needed; the navigation guard refuses loopback):
//   CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium \
//   cargo test -p quarry-browser --features chromiumoxide capture_ -- --test-threads=1 --ignored --nocapture
//
// NOT RUN when this was written (build-first rule). The evidence run is the final test
// phase of wave 2, in the rebuilt edge image's Chromium.
//
// Each test turns capture v2 on through the environment (the tests run one at a time) and
// calls `persist_profile_v2` directly, then reads the snapshot back from the store.

/// A profile-enabled session under an egress policy that allows the two example origins.
async fn profile_v2_session(
    driver: &ChromiumoxideDriver,
    key: &str,
) -> (BrowserSession, Arc<crate::session::InMemoryProfileStore>) {
    std::env::set_var(crate::profile_snapshot::CAPTURE_V2_ENV, "1");
    let mut lease = make_lease_with_key(key);
    lease.persist_profile = true;
    let session = driver.acquire(&lease).await.expect("acquire");
    driver
        .configure_egress_policy(
            &session,
            BrowserEgressPolicy::from_allowed_domains(&[
                "example.com".to_owned(),
                "example.org".to_owned(),
            ]),
        )
        .await
        .expect("install egress policy");
    let store = Arc::new(crate::session::InMemoryProfileStore::new());
    (session, store)
}

/// Run a script in the current tab; the promise it returns is awaited.
async fn profile_v2_run(driver: &ChromiumoxideDriver, session: &BrowserSession, js: &str) {
    let page = driver.current_page(session).await.expect("current page");
    let mut params = ::chromiumoxide::cdp::js_protocol::runtime::EvaluateParams::new(js);
    params.await_promise = Some(true);
    params.return_by_value = Some(true);
    page.execute(params).await.expect("script ran");
}

/// Capture now and read the saved snapshot back.
async fn profile_v2_saved(
    driver: &ChromiumoxideDriver,
    session: &BrowserSession,
    store: &Arc<crate::session::InMemoryProfileStore>,
) -> SessionSnapshot {
    let as_store: Arc<dyn ProfileStore> = store.clone();
    driver
        .persist_profile_v2(session, &as_store)
        .await
        .expect("capture v2");
    store
        .load(&session.lease.org_id, &session.lease.profile_id)
        .await
        .expect("load")
        .expect("a snapshot was saved")
}

#[tokio::test]
#[ignore = "needs a public origin: example.com and example.org over the public network"]
async fn capture_includes_cookies_of_a_previously_visited_origin() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let (session, store) = profile_v2_session(&driver, "ws17-c1-two-origins").await;
    let first = driver
        .new_tab(&session, Some("https://example.com/"))
        .await
        .expect("first origin");
    profile_v2_run(&driver, &session, "document.cookie = 'first=1; max-age=86400; path=/'").await;
    driver
        .new_tab(&session, Some("https://example.org/"))
        .await
        .expect("second origin");
    profile_v2_run(&driver, &session, "document.cookie = 'second=1; max-age=86400; path=/'").await;
    // The first origin's tab is gone before the release.
    driver.close_tab(&session, &first.tab_id).await.expect("close first tab");

    let saved = profile_v2_saved(&driver, &session, &store).await;
    let names: Vec<&str> = saved.cookies.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"first"), "the closed tab's origin kept its cookie: {names:?}");
    assert!(names.contains(&"second"), "{names:?}");
    assert_eq!(saved.snapshot_version, 2);
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "needs a public origin: example.com over the public network"]
async fn capture_records_expiry_and_samesite() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let (session, store) = profile_v2_session(&driver, "ws17-c1-expiry-samesite").await;
    driver
        .new_tab(&session, Some("https://example.com/"))
        .await
        .expect("origin");
    profile_v2_run(
        &driver,
        &session,
        "document.cookie = 'p=1; max-age=86400; path=/; SameSite=Strict'; document.cookie = 's=1; path=/'",
    )
    .await;
    let saved = profile_v2_saved(&driver, &session, &store).await;
    let persistent = saved.cookies.iter().find(|c| c.name == "p").expect("persistent cookie");
    let expires = persistent.expires.expect("a persistent cookie has an expiry");
    let ahead = (expires - chrono::Utc::now()).num_seconds();
    assert!((80_000..=90_000).contains(&ahead), "about 24 h ahead: {ahead}");
    assert_eq!(persistent.same_site, Some(crate::session::SameSite::Strict));
    let session_cookie = saved.cookies.iter().find(|c| c.name == "s").expect("session cookie");
    assert!(session_cookie.session && session_cookie.expires.is_none());
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "needs a public origin: example.com and example.org over the public network"]
async fn capture_includes_localstorage_of_a_closed_tab_origin() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let (session, store) = profile_v2_session(&driver, "ws17-c1-closed-tab-storage").await;
    let first = driver
        .new_tab(&session, Some("https://example.com/"))
        .await
        .expect("first origin");
    profile_v2_run(&driver, &session, "localStorage.setItem('kept', 'from-closed-tab')").await;
    driver
        .new_tab(&session, Some("https://example.org/"))
        .await
        .expect("second origin");
    // Closing the tab fires `pagehide` there: the flush reaches the journal.
    driver.close_tab(&session, &first.tab_id).await.expect("close first tab");
    tokio::time::sleep(Duration::from_millis(500)).await;

    let saved = profile_v2_saved(&driver, &session, &store).await;
    let origin = saved
        .origins
        .iter()
        .find(|state| state.origin == "https://example.com")
        .expect("the closed tab's origin is in the snapshot");
    assert!(
        origin.local_storage.iter().any(|(k, v)| k == "kept" && v == "from-closed-tab"),
        "flushed localStorage survived the tab"
    );
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "needs a public origin: example.com over the public network"]
async fn capture_includes_indexeddb_records() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let (session, store) = profile_v2_session(&driver, "ws17-c1-indexeddb").await;
    driver
        .new_tab(&session, Some("https://example.com/"))
        .await
        .expect("origin");
    profile_v2_run(
        &driver,
        &session,
        r#"(async () => {
          const db = await new Promise((res, rej) => {
            const r = indexedDB.open('c1-db', 2);
            r.onupgradeneeded = () => {
              const s = r.result.createObjectStore('items', { keyPath: 'id' });
              s.createIndex('by_name', 'name', { unique: false });
            };
            r.onsuccess = () => res(r.result);
            r.onerror = () => rej(r.error);
          });
          await new Promise((res, rej) => {
            const tx = db.transaction('items', 'readwrite');
            tx.objectStore('items').put({
              id: 1, name: 'one', when: new Date(1700000000000), bytes: new Uint8Array([1, 2, 3]),
              tags: new Set(['a']), big: 12345678901234567890n,
            });
            tx.oncomplete = () => res();
            tx.onerror = () => rej(tx.error);
          });
          db.close();
        })()"#,
    )
    .await;
    let saved = profile_v2_saved(&driver, &session, &store).await;
    let origin = saved
        .origins
        .iter()
        .find(|state| state.origin == "https://example.com")
        .expect("origin captured");
    let database = origin
        .indexed_db
        .iter()
        .find(|db| db.name == "c1-db")
        .expect("database captured");
    assert_eq!(database.version, 2);
    let store_state = database.stores.iter().find(|s| s.name == "items").expect("store");
    assert_eq!(store_state.records.len(), 1);
    assert!(store_state.indexes.iter().any(|ix| ix.name == "by_name"));
    // The record decodes through the tagged encoding, with its Date, bytes, Set and BigInt.
    let decoded = crate::profile_snapshot::IdbValue::from_json(&store_state.records[0].value)
        .expect("tagged value decodes");
    let crate::profile_snapshot::IdbValue::Object(fields) = decoded else {
        panic!("a record is an object");
    };
    assert!(matches!(fields.get("when"), Some(crate::profile_snapshot::IdbValue::Date(Some(_)))));
    assert!(matches!(fields.get("bytes"), Some(crate::profile_snapshot::IdbValue::Bytes { .. })));
    assert!(matches!(fields.get("tags"), Some(crate::profile_snapshot::IdbValue::Set(_))));
    assert!(matches!(fields.get("big"), Some(crate::profile_snapshot::IdbValue::BigInt(_))));
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "needs a public origin: example.com over the public network"]
async fn capture_with_slow_indexeddb_still_saves_cookies() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let (session, store) = profile_v2_session(&driver, "ws17-c1-slow-idb").await;
    driver
        .new_tab(&session, Some("https://example.com/"))
        .await
        .expect("origin");
    profile_v2_run(&driver, &session, "document.cookie = 'auth=1; max-age=86400; path=/'").await;
    // An IndexedDB that never answers: the serializer's own wait cannot end, so the
    // driver's 4 s IndexedDB budget has to.
    profile_v2_run(
        &driver,
        &session,
        "indexedDB.databases = () => new Promise(() => {}); localStorage.setItem('ls', '1')",
    )
    .await;
    let started = Instant::now();
    let saved = profile_v2_saved(&driver, &session, &store).await;
    assert!(started.elapsed() < Duration::from_secs(10), "inside the 10 s budget");
    assert!(saved.cookies.iter().any(|c| c.name == "auth"), "authentication was kept");
    let origin = saved
        .origins
        .iter()
        .find(|state| state.origin == "https://example.com")
        .expect("origin captured");
    assert!(origin.local_storage.iter().any(|(k, _)| k == "ls"), "web storage was kept");
    assert!(origin.indexed_db.is_empty() && origin.truncated, "IndexedDB gave out, flagged");
    driver.release(session).await.expect("release");
}
