//! WS4.D: what a request under Zero Data Retention leaves in the process log
//! and in usage-meter metadata.
//!
//! A log line is a durable copy. ZDR promises that the page an organization
//! fetched is not kept, so a default-level log line naming the URL or the page
//! title breaks the promise even though no database row was written. These
//! tests run the real router with real token verification, capture every
//! tracing event the process emits (JSON, `info` filter: what a deployed edge
//! logs by default) and look for the probe URL.
//!
//! Own binary on purpose: the capturing subscriber is process-global.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};
use tracing_subscriber::EnvFilter;

mod support;
#[path = "support/zdr_harness.rs"]
mod zdr_harness;

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
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(EnvFilter::new("info"))
            .with_writer(capture.clone())
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
async fn a_zdr_scrape_logs_no_url_and_no_title_at_the_default_level() {
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
