//! WS17.S0 CDP capability spike (plan `docs/plans/quarry-browser-build-plan-2026-10-14.md`,
//! slice WS17.S0, "CDP capability spike").
//!
//! Replaces three `[UNVERIFIED]` statements (D16-12) with recorded facts on the
//! pinned Chromium before WS17.C1 fixes a profile-persistence architecture
//! ("frame-less CDP" or "journal/flush"). It runs five probes:
//!
//! - P1 (a, b, c, and latency P5): `Storage.getCookies { browserContextId }` across
//!   two origins in one context, expires/session/sameSite/partition key as reported;
//!   `Storage.setCookies` on `about:blank` before navigation in a second context,
//!   including a host-only `__Host-` cookie restored by `url` and a `__Host-`
//!   cookie sent with `domain` (expected: rejected).
//! - P2 (a, b): `DOMStorage.getDOMStorageItems` and `setDOMStorageItem` for an
//!   origin with no live frame in the target (expected: no).
//! - P3 (a, b, c): `IndexedDB.requestDatabaseNames` and `requestData` for an origin
//!   with no live frame (expected: yes), how `RemoteObject` values come back, and
//!   the latency of a 5 MB value read.
//! - P4 (a, b, c): `Page.addScriptToEvaluateOnNewDocument` seeding before page
//!   scripts and in a same-origin iframe; `Runtime.addBinding` plus a `pagehide`
//!   listener, counting binding calls at navigation and at `page.close()`.
//!
//! The probes record facts; they do not assert the expectations above. A row is
//! `probe | outcome`. The table printed by the run goes into WS17.C1's design
//! notes before C1 starts.
//!
//! Run (live Chromium lane, through the docker lock):
//! `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 cargo test -p quarry-browser --features chromiumoxide --test profile_cdp_spike -- --ignored --nocapture`
//!
//! Without `CHROMIUMOXIDE_TEST=1` the test returns at once. With it, a missing
//! Chromium FAILS the run (see `quarry_browser::live_gate`).
//!
//! Deviations from the plan text, on purpose:
//! - The raw CDP calls run on a `chromiumoxide::Browser` owned by this test. The
//!   driver does not expose its `Browser`, and WS3.B's `live_driver()` helper is
//!   private to the crate's unit tests, so an integration test cannot reach it.
//!   The launch flags mirror `ChromiumoxideDriver`'s launch (`--disable-quic`,
//!   `--disable-dev-shm-usage`, no browser-wide viewport, `QUARRY_BROWSER_NO_SANDBOX`).
//! - No pinned egress proxy is installed. The probes use the public origin
//!   `https://example.com` (as the plan says) and a loopback HTTP origin served by
//!   this test, so the multi-origin cookie check does not depend on a second host.
//!   Run offline, the `https://example.com` probes record `ERROR` rows.
//!
//! Not run yet: this file was compile-checked only (WS17.S0 build). Its first live
//! run is in the wave's final test phase.

#![cfg(feature = "chromiumoxide")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chromiumoxide::cdp::browser_protocol::browser::BrowserContextId;
use chromiumoxide::cdp::browser_protocol::dom_storage::{
    EnableParams as DomStorageEnableParams, GetDomStorageItemsParams, SetDomStorageItemParams,
    StorageId,
};
use chromiumoxide::cdp::browser_protocol::indexed_db::{
    RequestDataParams, RequestDatabaseNamesParams,
};
use chromiumoxide::cdp::browser_protocol::network::{Cookie, CookieParam};
use chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams;
use chromiumoxide::cdp::browser_protocol::storage::{GetCookiesParams, SetCookiesParams};
use chromiumoxide::cdp::browser_protocol::target::{
    CreateBrowserContextParams, CreateTargetParams,
};
use chromiumoxide::cdp::js_protocol::runtime::{
    AddBindingParams, EvaluateParams, EventBindingCalled,
};
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const EXAMPLE_URL: &str = "https://example.com/";
const EXAMPLE_ORIGIN: &str = "https://example.com";
const IDB_NAME: &str = "spike-idb";
const SEED_SCRIPT: &str = "window.__seeded = 'seed:' + location.origin;";

const MAIN_HTML: &str = "<!doctype html><html><head><title>spike</title></head><body>\
<script>window.__atLoad = window.__seeded || 'missing';</script>\
<iframe src=\"/frame\"></iframe></body></html>";
const FRAME_HTML: &str = "<!doctype html><html><body>\
<script>window.__frameAtLoad = window.__seeded || 'missing';</script></body></html>";

/// Creates the database and writes a small record and a 5 MB record, run inside
/// the live example.com frame before the frame is torn down.
const IDB_SETUP_JS: &str = r#"(async () => {
  const db = await new Promise((resolve, reject) => {
    const req = indexedDB.open('spike-idb', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('items', { keyPath: 'id' });
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
  await new Promise((resolve, reject) => {
    const tx = db.transaction('items', 'readwrite');
    tx.objectStore('items').put({ id: 1, small: 'x' });
    tx.objectStore('items').put({ id: 2, big: 'y'.repeat(5 * 1024 * 1024) });
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
  db.close();
  return 'stored';
})()"#;

/// Result rows, printed at the end of the run.
#[derive(Default)]
struct Report {
    rows: Vec<(String, String)>,
}

impl Report {
    fn record(&mut self, probe: &str, outcome: impl Into<String>) {
        self.rows.push((probe.to_string(), outcome.into()));
    }

    fn render(&self) -> String {
        let mut out = String::from("WS17.S0 CDP spike results (chromium pinned by WS3)\n");
        for (probe, outcome) in &self.rows {
            out.push_str(&format!("{probe} | {outcome}\n"));
        }
        out
    }
}

/// A loopback HTTP origin: `/frame` gets the frame page, everything else the main
/// page. Answers each connection once and closes it.
async fn serve_loopback() -> (u16, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback origin");
    let port = listener.local_addr().expect("loopback address").port();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let body = if request.starts_with("GET /frame") {
                    FRAME_HTML
                } else {
                    MAIN_HTML
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (port, task)
}

/// Launches the pinned Chromium with the same flags the driver uses, and drains
/// the CDP handler on a background task.
async fn launch(profile: &std::path::Path) -> anyhow::Result<Browser> {
    let mut builder = BrowserConfig::builder()
        .request_timeout(Duration::from_secs(40))
        .launch_timeout(Duration::from_secs(20))
        .user_data_dir(profile)
        .arg("--disable-dev-shm-usage")
        .arg("--disable-quic")
        .viewport(None::<Viewport>);
    if std::env::var("QUARRY_BROWSER_NO_SANDBOX").ok().as_deref() == Some("1") {
        builder = builder.no_sandbox();
    }
    let config = builder
        .build()
        .map_err(|e| anyhow::anyhow!("browser config: {e}"))?;
    let (browser, mut handler) = Browser::launch(config).await?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });
    Ok(browser)
}

async fn new_context(browser: &Browser) -> anyhow::Result<BrowserContextId> {
    Ok(browser
        .create_browser_context(CreateBrowserContextParams::default())
        .await?)
}

async fn new_page(browser: &Browser, context: Option<&BrowserContextId>) -> anyhow::Result<Page> {
    let mut params = CreateTargetParams::new("about:blank");
    params.browser_context_id = context.cloned();
    Ok(browser.new_page(params).await?)
}

async fn context_cookies(
    browser: &Browser,
    context: &BrowserContextId,
) -> anyhow::Result<Vec<Cookie>> {
    let response = browser
        .execute(GetCookiesParams {
            browser_context_id: Some(context.clone()),
        })
        .await?;
    Ok(response.result.cookies)
}

/// Rebuilds a cookie for `Storage.setCookies` from what `getCookies` reported,
/// using `url` (host-only), the way a restore from a journal would.
fn cookie_as_param(cookie: &Cookie) -> CookieParam {
    let scheme = if cookie.secure { "https" } else { "http" };
    let host = cookie.domain.trim_start_matches('.');
    let path = if cookie.path.is_empty() {
        "/"
    } else {
        cookie.path.as_str()
    };
    let mut param = CookieParam::new(cookie.name.clone(), cookie.value.clone());
    param.url = Some(format!("{scheme}://{host}{path}"));
    param.secure = Some(cookie.secure);
    param.http_only = Some(cookie.http_only);
    param.same_site = cookie.same_site.clone();
    param
}

fn names_of(cookies: &[Cookie]) -> String {
    let names: Vec<String> = cookies
        .iter()
        .map(|c| format!("{}@{}", c.name, c.domain))
        .collect();
    format!("{} cookies: [{}]", names.len(), names.join(", "))
}

/// P1: cookies across origins in one context, restore on `about:blank`, the
/// `__Host-` rejection, and the latency of `Storage.getCookies`.
async fn probe_cookies(browser: &Browser, port: u16, report: &mut Report) -> anyhow::Result<()> {
    let ctx = new_context(browser).await?;
    let page = new_page(browser, Some(&ctx)).await?;
    page.goto(EXAMPLE_URL).await?;
    page.evaluate(
        "document.cookie='ex_plain=1; path=/'; \
         document.cookie='ex_secure=1; Secure; path=/'; \
         document.cookie='__Host-ex=1; Secure; path=/'; 'ok'",
    )
    .await?;
    let loopback = format!("http://127.0.0.1:{port}/");
    page.goto(loopback.as_str()).await?;
    page.evaluate("document.cookie='lb_plain=1; path=/'; 'ok'")
        .await?;

    let before = context_cookies(browser, &ctx).await?;
    report.record("P1a getCookies(ctx) across origins", names_of(&before));
    report.record(
        "P1a detail (expires, session, sameSite, partitionKey)",
        format!("{before:?}"),
    );

    // Restore into a second context, on about:blank, before any navigation.
    let restore_ctx = new_context(browser).await?;
    let _restore_page = new_page(browser, Some(&restore_ctx)).await?;
    let mut set = SetCookiesParams::new(before.iter().map(cookie_as_param).collect());
    set.browser_context_id = Some(restore_ctx.clone());
    match browser.execute(set).await {
        Ok(_) => report.record("P1b setCookies on about:blank, restore by url", "ok"),
        Err(e) => report.record(
            "P1b setCookies on about:blank, restore by url",
            format!("ERR {e}"),
        ),
    }
    let restored = context_cookies(browser, &restore_ctx).await?;
    report.record("P1b restored cookies", names_of(&restored));

    // A __Host- cookie sent with an explicit domain must be refused.
    let mut host_with_domain = CookieParam::new("__Host-ex", "1");
    host_with_domain.domain = Some("example.com".to_string());
    host_with_domain.path = Some("/".to_string());
    host_with_domain.secure = Some(true);
    let mut refused = SetCookiesParams::new(vec![host_with_domain]);
    refused.browser_context_id = Some(restore_ctx.clone());
    let outcome = match browser.execute(refused).await {
        Ok(_) => "accepted".to_string(),
        Err(e) => format!("rejected: {e}"),
    };
    let after_refused = context_cookies(browser, &restore_ctx).await?;
    let host_count = after_refused
        .iter()
        .filter(|c| c.name.starts_with("__Host-"))
        .count();
    report.record(
        "P1c __Host- sent with domain",
        format!("{outcome}; __Host- cookies in restore context: {host_count}"),
    );

    // P5: latency of Storage.getCookies over 50 calls.
    let mut samples = Vec::with_capacity(50);
    for _ in 0..50 {
        let started = Instant::now();
        let _ = context_cookies(browser, &ctx).await?;
        samples.push(started.elapsed());
    }
    samples.sort();
    report.record(
        "P5 getCookies latency (50 calls)",
        format!(
            "min {:?}, median {:?}, max {:?}",
            samples[0],
            samples[samples.len() / 2],
            samples[samples.len() - 1]
        ),
    );
    Ok(())
}

/// P2: DOMStorage for an origin whose frame has been torn down.
async fn probe_dom_storage(browser: &Browser, report: &mut Report) -> anyhow::Result<()> {
    let ctx = new_context(browser).await?;
    let page = new_page(browser, Some(&ctx)).await?;
    page.execute(DomStorageEnableParams::default()).await?;
    page.goto(EXAMPLE_URL).await?;
    page.evaluate("localStorage.setItem('spike', 'v1'); 'ok'")
        .await?;
    page.goto("about:blank").await?;

    let storage_id = StorageId {
        security_origin: Some(EXAMPLE_ORIGIN.to_string()),
        storage_key: None,
        is_local_storage: true,
    };
    let read = match page
        .execute(GetDomStorageItemsParams::new(storage_id.clone()))
        .await
    {
        Ok(r) => format!("ok {:?}", r.result.entries),
        Err(e) => format!("ERR {e}"),
    };
    report.record("P2a getDOMStorageItems, no live frame", read);

    let write = match page
        .execute(SetDomStorageItemParams {
            storage_id,
            key: "spike2".to_string(),
            value: "v2".to_string(),
        })
        .await
    {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("ERR {e}"),
    };
    report.record("P2b setDOMStorageItem, no live frame", write);
    Ok(())
}

/// P3: IndexedDB for an origin whose frame has been torn down, and the value shape.
async fn probe_indexed_db(browser: &Browser, report: &mut Report) -> anyhow::Result<()> {
    let ctx = new_context(browser).await?;
    let page = new_page(browser, Some(&ctx)).await?;
    page.goto(EXAMPLE_URL).await?;
    let mut setup = EvaluateParams::new(IDB_SETUP_JS);
    setup.await_promise = Some(true);
    setup.return_by_value = Some(true);
    let stored = match page.execute(setup).await {
        Ok(r) => format!("ok value={:?}", r.result.result.value),
        Err(e) => format!("ERR {e}"),
    };
    report.record("P3 setup in live frame", stored);
    page.goto("about:blank").await?;

    let names = match page
        .execute(RequestDatabaseNamesParams {
            security_origin: Some(EXAMPLE_ORIGIN.to_string()),
            storage_key: None,
            storage_bucket: None,
        })
        .await
    {
        Ok(r) => format!("{:?}", r.result.database_names),
        Err(e) => format!("ERR {e}"),
    };
    report.record("P3a requestDatabaseNames, no live frame", names);

    let started = Instant::now();
    let data = page
        .execute(RequestDataParams {
            security_origin: Some(EXAMPLE_ORIGIN.to_string()),
            storage_key: None,
            storage_bucket: None,
            database_name: IDB_NAME.to_string(),
            object_store_name: "items".to_string(),
            index_name: None,
            skip_count: 0,
            page_size: 10,
            key_range: None,
        })
        .await;
    let elapsed = started.elapsed();
    match data {
        Ok(r) => {
            for entry in &r.result.object_store_data_entries {
                let value_bytes = entry
                    .value
                    .value
                    .as_ref()
                    .map(|v| v.to_string().len())
                    .unwrap_or(0);
                report.record(
                    "P3b requestData entry (RemoteObject shape)",
                    format!(
                        "key type {:?} by value {:?}; value type {:?}, value.value present {}, JSON bytes {}",
                        entry.key.r#type,
                        entry.key.value,
                        entry.value.r#type,
                        entry.value.value.is_some(),
                        value_bytes
                    ),
                );
            }
            report.record(
                "P3c requestData latency (pageSize 10, includes the 5 MB value)",
                format!("{elapsed:?}"),
            );
        }
        Err(e) => report.record("P3b requestData", format!("ERR {e}")),
    }
    Ok(())
}

/// P4: seeding before page scripts and in a same-origin iframe, and the
/// `pagehide` flush through `Runtime.addBinding` at navigation and at close.
async fn probe_seed_and_flush(
    browser: &Browser,
    port: u16,
    report: &mut Report,
) -> anyhow::Result<()> {
    let ctx = new_context(browser).await?;
    let page = new_page(browser, Some(&ctx)).await?;
    page.evaluate_on_new_document(AddScriptToEvaluateOnNewDocumentParams::new(SEED_SCRIPT))
        .await?;
    let main_url = format!("http://127.0.0.1:{port}/");
    page.goto(main_url.as_str()).await?;
    let at_load = page
        .evaluate("window.__atLoad")
        .await?
        .into_value::<String>()?;
    let frame_at_load = page
        .evaluate("document.querySelector('iframe').contentWindow.__frameAtLoad")
        .await?
        .into_value::<String>()?;
    report.record(
        "P4a seed before page scripts",
        format!("main page {at_load}; same-origin iframe {frame_at_load}"),
    );

    page.execute(AddBindingParams::new("spikeFlush")).await?;
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mut events = page.event_listener::<EventBindingCalled>().await?;
    let sink = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Some(event) = events.next().await {
            if event.name == "spikeFlush" {
                sink.lock().expect("flush log").push(event.payload.clone());
            }
        }
    });

    page.evaluate("window.addEventListener('pagehide', () => spikeFlush('pagehide')); 'ok'")
        .await?;
    page.goto("about:blank").await?;
    let at_goto_return = seen.lock().expect("flush log").len();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let after_settle = seen.lock().expect("flush log").len();
    report.record(
        "P4b pagehide binding on navigation",
        format!("received when goto returned {at_goto_return}; after 1 s {after_settle}"),
    );

    page.goto(main_url.as_str()).await?;
    page.evaluate("window.addEventListener('pagehide', () => spikeFlush('pagehide-close')); 'ok'")
        .await?;
    let before_close = seen.lock().expect("flush log").len();
    page.close().await?;
    let at_close_return = seen.lock().expect("flush log").len();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let after_close = seen.lock().expect("flush log").len();
    report.record(
        "P4c pagehide binding on page.close()",
        format!(
            "before close {before_close}; when close returned {at_close_return}; after 1 s {after_close}"
        ),
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live Chromium spike: run with CHROMIUMOXIDE_TEST=1 and --ignored, see module docs"]
async fn cdp_capability_spike() {
    if !quarry_browser::live_gate::live_enabled() {
        return;
    }
    let (port, server) = serve_loopback().await;
    let profile = tempfile::tempdir().expect("profile directory");
    let browser = launch(profile.path())
        .await
        .expect("launch the pinned Chromium");

    let mut report = Report::default();
    if let Err(e) = probe_cookies(&browser, port, &mut report).await {
        report.record("P1 cookies", format!("ERROR {e:#}"));
    }
    if let Err(e) = probe_dom_storage(&browser, &mut report).await {
        report.record("P2 DOMStorage", format!("ERROR {e:#}"));
    }
    if let Err(e) = probe_indexed_db(&browser, &mut report).await {
        report.record("P3 IndexedDB", format!("ERROR {e:#}"));
    }
    if let Err(e) = probe_seed_and_flush(&browser, port, &mut report).await {
        report.record("P4 seed and flush", format!("ERROR {e:#}"));
    }

    println!("{}", report.render());
    server.abort();
    // The probes must each leave at least one row, even when a probe fails.
    assert!(
        report.rows.len() >= 4,
        "every probe records a row, got {}",
        report.rows.len()
    );
}
