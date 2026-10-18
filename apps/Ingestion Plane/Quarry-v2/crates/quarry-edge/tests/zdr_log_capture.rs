//! WS4.D: what a request under Zero Data Retention leaves in the process log
//! and in usage-meter metadata.
//!
//! A log line is a durable copy. ZDR promises that the page an organization
//! fetched is not kept, so a log line naming the URL or the page title breaks
//! the promise even though no database row was written. These tests run the
//! real router with real token verification and capture every tracing event the
//! process emits, at `debug` (the most an operator can turn it up to), through
//! the layer stack the binary builds: the ZDR mark layer and the sensitive-field
//! event filter. Then they look for the probe URL.
//!
//! Own binary on purpose: the capturing subscriber is process-global.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

mod support;
#[path = "support/zdr_harness.rs"]
mod zdr_harness;
// Only for its `RecordingDriver`: this binary's tokens come from zdr_harness, and
// agent_harness's own fixture() is never called here, so the two do not fight over
// the JWKS environment.
#[cfg(feature = "browser-agent")]
#[path = "support/agent_harness.rs"]
mod agent_harness;

use zdr_harness::{user_in_standard_org, user_in_zdr_org, Harness};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Install the capturing subscriber once for the process and return its buffer.
fn capture() -> &'static Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let capture = Capture::default();
        tracing_subscriber::registry()
            .with(quarry_edge::zdr_log::ZdrMarkLayer)
            .with(quarry_edge::zdr_log::harden(EnvFilter::new("debug")))
            .with(quarry_edge::zdr_log::build_json_layer(capture.clone()))
            .init();
        capture
    })
}

fn logged() -> String {
    String::from_utf8_lossy(&capture().0.lock().unwrap()).into_owned()
}

fn probe_url() -> (String, String) {
    let marker = format!("zdr-probe-{}", uuid::Uuid::new_v4().simple());
    (format!("https://example.com/{marker}"), marker)
}

#[tokio::test]
async fn a_zdr_scrape_logs_no_url_and_no_title_even_at_debug() {
    capture();
    let harness = Harness::start().await;
    let (url, marker) = probe_url();

    let response = harness
        .post(&user_in_zdr_org(), "/v1/scrape", json!({ "url": url }))
        .await;
    assert_eq!(response.status().as_u16(), 200);

    let log = logged();
    assert!(
        !log.contains(&marker),
        "a ZDR scrape put the page URL or title in a default-level log line:\n{}",
        log.lines()
            .filter(|l| l.contains(&marker))
            .collect::<Vec<_>>()
            .join("\n")
    );
    // The run did happen, and it still reports the shape of the page.
    assert!(
        log.lines()
            .any(|l| l.contains("page extracted") && l.contains("word_count")),
        "the ZDR branch must still log that a page was extracted"
    );
}

#[tokio::test]
async fn a_non_zdr_scrape_still_logs_page_extracted_with_the_url() {
    // Control: proves the capture sees this line, so the test above passing is
    // not just a subscriber that records nothing.
    capture();
    let harness = Harness::start().await;
    let (url, marker) = probe_url();

    let response = harness
        .post(&user_in_standard_org(), "/v1/scrape", json!({ "url": url }))
        .await;
    assert_eq!(response.status().as_u16(), 200);

    assert!(
        logged()
            .lines()
            .any(|l| l.contains("page extracted") && l.contains(&marker)),
        "a non-ZDR scrape logs the URL with `page extracted`"
    );
}

fn scrape_meter_metadata(harness: &Harness) -> Value {
    let events = harness.usage.events.lock().unwrap();
    let event = events
        .iter()
        .find(|e| e.metric == quarry_runtime::usage_metrics::SCRAPE_PAGE)
        .expect("a scrape.page usage event");
    event.metadata.clone()
}

#[tokio::test]
async fn scrape_meter_metadata_under_zdr_has_no_url_or_fingerprint() {
    let harness = Harness::start().await;
    let (url, marker) = probe_url();

    let response = harness
        .post(&user_in_zdr_org(), "/v1/scrape", json!({ "url": url }))
        .await;
    assert_eq!(response.status().as_u16(), 200);

    let meta = scrape_meter_metadata(&harness);
    assert_eq!(meta["zdr"], json!(true));
    assert_eq!(meta["status"], json!(200));
    assert!(
        meta.get("url").is_none(),
        "ZDR meter metadata names the page: {meta}"
    );
    assert!(
        meta.get("fingerprint").is_none(),
        "ZDR meter metadata carries the content fingerprint: {meta}"
    );
    assert!(!meta.to_string().contains(&marker), "{meta}");
}

#[tokio::test]
async fn scrape_meter_metadata_outside_zdr_keeps_url_and_fingerprint() {
    let harness = Harness::start().await;
    let (url, _marker) = probe_url();

    let response = harness
        .post(&user_in_standard_org(), "/v1/scrape", json!({ "url": url }))
        .await;
    assert_eq!(response.status().as_u16(), 200);

    let meta = scrape_meter_metadata(&harness);
    assert_eq!(meta["zdr"], json!(false));
    assert_eq!(meta["url"], json!(url));
    assert!(meta["fingerprint"].is_string(), "{meta}");
}

#[tokio::test]
async fn a_zdr_search_with_a_failing_provider_logs_no_query() {
    capture();
    let harness = Harness::start().await;
    let secret = format!("SECRET-QUERY-{}", uuid::Uuid::new_v4().simple());

    let response = harness
        .post(&user_in_zdr_org(), "/v1/search", json!({ "query": secret }))
        .await;
    assert!(!response.status().is_success(), "the provider fails");

    let log = logged();
    assert!(
        log.lines().any(|l| l.contains("search.upstream")),
        "the failure is still logged, as a class"
    );
    assert!(
        !log.contains(&secret),
        "a ZDR search put the query in a log line:\n{}",
        log.lines()
            .filter(|l| l.contains(&secret))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[tokio::test]
async fn the_request_span_never_logs_the_query_string() {
    // The access log line tower-http writes per request must not carry the
    // query string, which can hold the URL a ZDR caller asked about. The probe
    // rides in the query of a route that does not read it.
    capture();
    let harness = Harness::start().await;
    let marker = format!("zdr-query-probe-{}", uuid::Uuid::new_v4().simple());

    let response = harness
        .post(
            &user_in_zdr_org(),
            &format!("/v1/search?url=https://{marker}.test/"),
            json!({ "query": "anything" }),
        )
        .await;
    drop(response);

    assert!(
        !logged().contains(&marker),
        "a request span recorded the query string"
    );
}

// ---- agent runs ------------------------------------------------------------

/// The recording browser double of `support/agent_harness.rs`, plus what a real
/// driver does when it opens a page: it says so, with the address, at debug.
/// Nothing else on the agent path logs the address of the page a run is asked to
/// open, so without this line the capture below would pass whatever the filter
/// did; with it, the test fails if a ZDR run's step ever runs outside the ZDR span
/// (a spawned task that lost it, a layer that reordered the mark).
#[cfg(feature = "browser-agent")]
struct UrlLoggingDriver(agent_harness::RecordingDriver);

#[cfg(feature = "browser-agent")]
#[async_trait::async_trait]
impl quarry_browser::BrowserDriver for UrlLoggingDriver {
    fn capabilities(&self) -> quarry_browser::BrowserDriverCapabilities {
        self.0.capabilities()
    }

    async fn acquire(
        &self,
        lease: &quarry_core::lease::BrowserLease,
    ) -> quarry_core::QuarryResult<quarry_browser::BrowserSession> {
        self.0.acquire(lease).await
    }

    async fn release(
        &self,
        session: quarry_browser::BrowserSession,
    ) -> quarry_core::QuarryResult<()> {
        self.0.release(session).await
    }

    async fn configure_egress_policy(
        &self,
        session: &quarry_browser::BrowserSession,
        policy: quarry_browser::BrowserEgressPolicy,
    ) -> quarry_core::QuarryResult<()> {
        self.0.configure_egress_policy(session, policy).await
    }

    async fn goto(
        &self,
        session: &quarry_browser::BrowserSession,
        url: &str,
    ) -> quarry_core::QuarryResult<()> {
        tracing::debug!(url, "agent driver opening a page");
        self.0.goto(session, url).await
    }

    async fn content(
        &self,
        session: &quarry_browser::BrowserSession,
    ) -> quarry_core::QuarryResult<bytes::Bytes> {
        self.0.content(session).await
    }

    async fn screenshot(
        &self,
        session: &quarry_browser::BrowserSession,
        full_page: bool,
    ) -> quarry_core::QuarryResult<bytes::Bytes> {
        self.0.screenshot(session, full_page).await
    }

    async fn live_frame(
        &self,
        session: &quarry_browser::BrowserSession,
        options: quarry_browser::LiveFrameOptions,
    ) -> quarry_core::QuarryResult<quarry_browser::LiveFrame> {
        self.0.live_frame(session, options).await
    }

    async fn pdf(
        &self,
        session: &quarry_browser::BrowserSession,
    ) -> quarry_core::QuarryResult<bytes::Bytes> {
        self.0.pdf(session).await
    }
}

/// Start an agent run for `bearer`, ask it to open `url`, and return the run id.
#[cfg(feature = "browser-agent")]
async fn start_a_run_and_open(bearer: &str, url: &str) -> String {
    let harness = Harness::start_with(|state| quarry_edge::state::AppState {
        agent_driver: Arc::new(UrlLoggingDriver(agent_harness::RecordingDriver::default())),
        ..state
    })
    .await;

    let started = harness.post(bearer, "/v1/agent/runs", json!({})).await;
    assert_eq!(started.status().as_u16(), 200, "the run must start");
    let body: Value = started.json().await.expect("start answers JSON");
    let run_id = body["data"]["run_id"]
        .as_str()
        .expect("start answers a run_id")
        .to_owned();

    let step = harness
        .post(
            bearer,
            &format!("/v1/agent/runs/{run_id}/step"),
            json!({ "action": { "type": "navigate", "url": url } }),
        )
        .await;
    assert_eq!(
        step.status().as_u16(),
        200,
        "the navigate step must run for the capture to mean anything: {}",
        step.text().await.unwrap_or_default()
    );
    run_id
}

#[cfg(feature = "browser-agent")]
#[tokio::test]
async fn a_zdr_agent_start_logs_no_url() {
    capture();
    let (url, marker) = probe_url();

    let run_id = start_a_run_and_open(&user_in_zdr_org(), &url).await;

    let log = logged();
    assert!(
        !log.contains(&marker),
        "a ZDR agent run put the page URL in a log line:\n{}",
        log.lines()
            .filter(|l| l.contains(&marker))
            .collect::<Vec<_>>()
            .join("\n")
    );
    // The run did happen and the capture saw it: its own requests are logged,
    // marked ZDR, without a URL.
    assert!(
        log.lines()
            .any(|l| l.contains(&run_id) && l.contains("\"zdr\":true")),
        "the ZDR run's requests must still be logged, marked zdr"
    );
}

#[cfg(feature = "browser-agent")]
#[tokio::test]
async fn a_non_zdr_agent_run_logs_the_url_the_driver_is_asked_to_open() {
    // Control: the line the test above keeps out of a ZDR log is really emitted on
    // this path and really captured, so that test passing is not a quiet subscriber.
    capture();
    let (url, marker) = probe_url();

    start_a_run_and_open(&user_in_standard_org(), &url).await;

    assert!(
        logged()
            .lines()
            .any(|l| l.contains("agent driver opening a page") && l.contains(&marker)),
        "a non-ZDR agent run logs the URL at debug"
    );
}
