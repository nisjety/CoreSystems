//! `/ready` and the local browser (WS3.F): an unhealthy browser takes the edge
//! out of rotation, a healthy one is reported, a driver without a local browser
//! leaves the body exactly as it was, and `/health` never changes.
#![cfg(feature = "browser-agent")]

use std::{net::SocketAddr, sync::Arc, time::Duration};

use quarry_browser::{BrowserHealth, BrowserHealthState, BrowserSession, SlotHealth};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::lease::BrowserLease;
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::Value;
use serial_test::serial;
use tokio::sync::mpsc;

mod support;

/// A driver that only reports a fixed health.
struct FixedHealth(BrowserHealth);

fn no_browser() -> QuarryError {
    QuarryError::new(ErrorCode::DriverFailed, "test driver has no browser")
}

#[async_trait::async_trait]
impl quarry_browser::BrowserDriver for FixedHealth {
    async fn acquire(&self, _lease: &BrowserLease) -> QuarryResult<BrowserSession> {
        Err(no_browser())
    }
    async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
        Ok(())
    }
    async fn health(&self) -> BrowserHealth {
        self.0.clone()
    }
    async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
        Err(no_browser())
    }
    async fn content(&self, _session: &BrowserSession) -> QuarryResult<bytes::Bytes> {
        Err(no_browser())
    }
    async fn screenshot(
        &self,
        _session: &BrowserSession,
        _full_page: bool,
    ) -> QuarryResult<bytes::Bytes> {
        Err(no_browser())
    }
    async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<bytes::Bytes> {
        Err(no_browser())
    }
}

fn slot(status: &str, healthy: Option<bool>) -> SlotHealth {
    SlotHealth {
        slot: 0,
        status: status.to_owned(),
        generation: 1,
        sessions: 2,
        pages: 3,
        healthy,
        probe_failures: if healthy == Some(false) { 2 } else { 0 },
    }
}

async fn spawn_edge(agent_driver: Arc<dyn quarry_browser::BrowserDriver>) -> SocketAddr {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        agent_driver,
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn get_text(addr: SocketAddr, path: &str) -> (u16, String) {
    let response = reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .send()
        .await
        .unwrap();
    (response.status().as_u16(), response.text().await.unwrap())
}

async fn get_json(addr: SocketAddr, path: &str) -> (u16, Value) {
    let (status, text) = get_text(addr, path).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

#[tokio::test]
#[serial]
async fn unhealthy_browser_answers_503_browser_unhealthy() {
    let health = BrowserHealth::from_slots(40, 2, vec![slot("ready", Some(false))]);
    assert_eq!(health.state, BrowserHealthState::Unhealthy);
    let addr = spawn_edge(Arc::new(FixedHealth(health))).await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["status"], "not_ready");
    assert_eq!(body["reason"], "browser_unhealthy");
    assert_eq!(body["browser"]["state"], "unhealthy");
    assert_eq!(body["browser"]["sessions_open"], 2);
    assert_eq!(body["browser"]["slots"][0]["status"], "ready");
    assert_eq!(body["browser"]["slots"][0]["healthy"], false);
    assert_eq!(get_text(addr, "/health").await.0, 200);

    // A dead browser counts the same.
    let dead = BrowserHealth::from_slots(40, 0, vec![slot("dead", Some(false))]);
    let addr = spawn_edge(Arc::new(FixedHealth(dead))).await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["reason"], "browser_unhealthy");
    assert_eq!(get_text(addr, "/health").await.0, 200);
}

#[tokio::test]
#[serial]
async fn healthy_and_idle_browsers_answer_200_with_a_browser_field() {
    let healthy = BrowserHealth::from_slots(40, 2, vec![slot("ready", Some(true))]);
    assert_eq!(healthy.state, BrowserHealthState::Healthy);
    let addr = spawn_edge(Arc::new(FixedHealth(healthy))).await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "ready");
    assert_eq!(body["durable"], true);
    assert_eq!(body["browser"]["state"], "healthy");
    assert_eq!(body["browser"]["max_sessions"], 40);
    assert_eq!(body["browser"]["slots"][0]["pages"], 3);
    assert_eq!(get_text(addr, "/health").await.0, 200);

    // Never launched is ready: the first run launches the browser. This is
    // the real driver, constructed and never used.
    let addr = spawn_edge(Arc::new(
        quarry_browser::chromiumoxide::ChromiumoxideDriver::new(),
    ))
    .await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["browser"]["state"], "idle");
    assert_eq!(body["browser"]["sessions_open"], 0);
}

#[tokio::test]
#[serial]
async fn not_applicable_leaves_the_ready_body_byte_identical() {
    let addr = spawn_edge(Arc::new(FixedHealth(BrowserHealth::not_applicable()))).await;
    let (status, text) = get_text(addr, "/ready").await;
    assert_eq!(status, 200);
    assert_eq!(text, r#"{"durable":true,"status":"ready"}"#);
    assert_eq!(get_text(addr, "/health").await.0, 200);
}

/// An edge whose registry also carries a scrape-path browser driver, as
/// main.rs builds it (a second local browser behind `BrowserDriverAdapter`).
async fn spawn_edge_with_scrape_browser(
    agent_driver: Arc<dyn quarry_browser::BrowserDriver>,
    scrape_driver: Arc<dyn quarry_browser::BrowserDriver + Send + Sync>,
) -> SocketAddr {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let base = support::base_state(driver, EventSink::new(tx));
    let mut drivers = base.drivers.clone();
    drivers.register(Arc::new(quarry_runtime::BrowserDriverAdapter::new(
        scrape_driver,
        Arc::new(quarry_runtime::lease_pool::RuntimeLeasePool::new(2)),
    )));
    let state = AppState {
        agent_driver,
        drivers,
        ..base
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

#[tokio::test]
#[serial]
async fn an_unhealthy_scrape_browser_answers_503_beside_a_healthy_agent_browser() {
    let healthy = BrowserHealth::from_slots(40, 1, vec![slot("ready", Some(true))]);
    let unhealthy = BrowserHealth::from_slots(40, 0, vec![slot("dead", Some(false))]);
    let addr = spawn_edge_with_scrape_browser(
        Arc::new(FixedHealth(healthy.clone())),
        Arc::new(FixedHealth(unhealthy.clone())),
    )
    .await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["reason"], "browser_unhealthy");
    assert_eq!(body["browser"]["state"], "healthy");
    assert_eq!(body["scrape_browser"]["state"], "unhealthy");
    assert_eq!(get_text(addr, "/health").await.0, 200);

    // Both healthy: 200 with both objects.
    let addr = spawn_edge_with_scrape_browser(
        Arc::new(FixedHealth(healthy.clone())),
        Arc::new(FixedHealth(healthy)),
    )
    .await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["scrape_browser"]["state"], "healthy");

    // A scrape driver without a local browser (a remote provider) is left out.
    let addr = spawn_edge_with_scrape_browser(
        Arc::new(FixedHealth(unhealthy)),
        Arc::new(FixedHealth(BrowserHealth::not_applicable())),
    )
    .await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 503, "{body}");
    assert!(body.get("scrape_browser").is_none(), "{body}");
}

#[tokio::test]
#[serial]
async fn a_degraded_browser_is_ready() {
    let degraded = BrowserHealth::from_slots(
        40,
        1,
        vec![
            slot("ready", Some(true)),
            SlotHealth {
                slot: 1,
                ..slot("dead", Some(false))
            },
        ],
    );
    assert_eq!(degraded.state, BrowserHealthState::Degraded);
    let addr = spawn_edge(Arc::new(FixedHealth(degraded))).await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["browser"]["state"], "degraded");
}

/// Puts the process-global drain flag back when a test ends, whether it passed or not.
struct DrainGuard;

impl Drop for DrainGuard {
    fn drop(&mut self) {
        quarry_edge::drain::reset_for_tests();
    }
}

/// A draining edge answers `draining` before it looks at the browsers: an unhealthy browser
/// does not change the answer, and the body names no browser.
#[tokio::test]
#[serial]
async fn draining_takes_precedence_over_an_unhealthy_browser() {
    let dead = BrowserHealth::from_slots(40, 0, vec![slot("dead", Some(false))]);
    let addr = spawn_edge(Arc::new(FixedHealth(dead))).await;
    let _guard = DrainGuard;
    quarry_edge::drain::begin();
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body, serde_json::json!({ "status": "draining" }));
    assert_eq!(get_text(addr, "/health").await.0, 200);
}

/// docs/openapi.yaml's `/ready` schemas, read from the file with the same
/// indentation scan the other openapi parity tests use (no YAML crate). The
/// real response bodies are checked against what they say, both ways: every
/// key sent is documented, every required key is sent, every enum value sent
/// is documented, and every documented property and enum value that the edge
/// can produce is produced by one of the bodies below.
mod openapi {
    use std::collections::BTreeSet;

    pub fn indent(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    /// The direct child `key:` of the mapping on line `parent`, if any.
    pub fn try_child(lines: &[&str], parent: usize, key: &str) -> Option<usize> {
        let base = indent(lines[parent]);
        let prefix = format!("{key}:");
        for (i, line) in lines.iter().enumerate().skip(parent + 1) {
            if line.trim().is_empty() {
                continue;
            }
            let at = indent(line);
            if at <= base {
                return None;
            }
            if at == base + 2 && line.trim().starts_with(&prefix) {
                return Some(i);
            }
        }
        None
    }

    pub fn child(lines: &[&str], parent: usize, key: &str) -> usize {
        try_child(lines, parent, key)
            .unwrap_or_else(|| panic!("docs/openapi.yaml /ready: no `{key}` under line {parent}"))
    }

    pub fn nav(lines: &[&str], mut at: usize, path: &[&str]) -> usize {
        for key in path {
            at = child(lines, at, key);
        }
        at
    }

    /// The keys of the mapping on line `parent`.
    pub fn keys(lines: &[&str], parent: usize) -> BTreeSet<String> {
        let base = indent(lines[parent]);
        let mut out = BTreeSet::new();
        for line in lines.iter().skip(parent + 1) {
            if line.trim().is_empty() {
                continue;
            }
            let at = indent(line);
            if at <= base {
                break;
            }
            if at == base + 2 {
                let key = line.trim().split(':').next().unwrap_or_default();
                out.insert(key.trim_matches('\'').to_owned());
            }
        }
        out
    }

    /// An inline list `key: [a, b]` that is a child of `parent`.
    pub fn list(lines: &[&str], parent: usize, key: &str) -> Option<BTreeSet<String>> {
        let line = lines[try_child(lines, parent, key)?];
        let inner = line.split_once('[')?.1.rsplit_once(']')?.0;
        Some(
            inner
                .split(',')
                .map(|item| item.trim().trim_matches('\'').to_owned())
                .collect(),
        )
    }

    /// An object schema: its properties and its required keys.
    pub struct Object {
        pub properties: BTreeSet<String>,
        pub required: BTreeSet<String>,
        pub at: usize,
    }

    pub fn object(lines: &[&str], at: usize) -> Object {
        let additional = lines[child(lines, at, "additionalProperties")];
        assert!(
            additional.trim() == "additionalProperties: false",
            "docs/openapi.yaml /ready: an object that allows undocumented keys: {additional}"
        );
        let properties = child(lines, at, "properties");
        Object {
            properties: keys(lines, properties),
            required: list(lines, at, "required").unwrap_or_default(),
            at,
        }
    }

    pub fn enum_of(lines: &[&str], property: usize) -> BTreeSet<String> {
        list(lines, property, "enum")
            .unwrap_or_else(|| panic!("docs/openapi.yaml /ready: no inline enum at line {property}"))
    }
}

/// What the edge sent across the bodies checked: for each schema level, the
/// keys and the enum values that were seen.
#[derive(Default)]
struct Seen {
    ready: std::collections::BTreeSet<String>,
    browser: std::collections::BTreeSet<String>,
    slot: std::collections::BTreeSet<String>,
    ready_status: std::collections::BTreeSet<String>,
    state: std::collections::BTreeSet<String>,
    slot_status: std::collections::BTreeSet<String>,
}

fn check_object(
    body: &Value,
    schema: &openapi::Object,
    what: &str,
    seen: &mut std::collections::BTreeSet<String>,
) {
    let map = body
        .as_object()
        .unwrap_or_else(|| panic!("{what} is not an object: {body}"));
    for key in map.keys() {
        assert!(
            schema.properties.contains(key),
            "{what}: the edge sends `{key}`, docs/openapi.yaml /ready does not document it"
        );
        seen.insert(key.clone());
    }
    for key in &schema.required {
        assert!(
            map.contains_key(key),
            "{what}: docs/openapi.yaml /ready requires `{key}`, the edge left it out: {body}"
        );
    }
}

fn in_enum(value: &Value, allowed: &std::collections::BTreeSet<String>, what: &str) -> String {
    let value = value
        .as_str()
        .unwrap_or_else(|| panic!("{what} is not a string: {value}"))
        .to_owned();
    assert!(
        allowed.contains(&value),
        "{what}: `{value}` is not one of the documented values {allowed:?}"
    );
    value
}

#[allow(clippy::too_many_arguments)]
fn check_ready_body(
    status: u16,
    body: &Value,
    ok_schema: &openapi::Object,
    err_schema: &openapi::Object,
    lines: &[&str],
    browser_schema: &openapi::Object,
    slot_schema: &openapi::Object,
    seen: &mut Seen,
) {
    let (schema, what) = match status {
        200 => (ok_schema, "200 body"),
        503 => (err_schema, "503 body"),
        other => panic!("/ready answered {other}, which docs/openapi.yaml does not document"),
    };
    check_object(body, schema, what, &mut seen.ready);
    let status_prop = openapi::nav(lines, schema.at, &["properties", "status"]);
    let allowed = openapi::enum_of(lines, status_prop);
    let value = in_enum(&body["status"], &allowed, what);
    seen.ready_status.insert(value);
    if let Some(durable) = body.get("durable") {
        assert!(durable.is_boolean(), "{what}: durable is not a boolean");
    }
    for key in ["browser", "scrape_browser"] {
        let Some(browser) = body.get(key) else {
            continue;
        };
        let what = format!("{what}.{key}");
        check_object(browser, browser_schema, &what, &mut seen.browser);
        let state_prop = openapi::nav(lines, browser_schema.at, &["properties", "state"]);
        let state = in_enum(
            &browser["state"],
            &openapi::enum_of(lines, state_prop),
            &format!("{what}.state"),
        );
        seen.state.insert(state);
        assert!(browser["max_sessions"].is_u64() && browser["sessions_open"].is_u64());
        let slots = browser["slots"].as_array().expect("slots is an array");
        for slot in slots {
            let what = format!("{what}.slots[]");
            check_object(slot, slot_schema, &what, &mut seen.slot);
            let status_prop = openapi::nav(lines, slot_schema.at, &["properties", "status"]);
            let value = in_enum(
                &slot["status"],
                &openapi::enum_of(lines, status_prop),
                &format!("{what}.status"),
            );
            seen.slot_status.insert(value);
            for key in ["slot", "generation", "sessions", "pages", "probe_failures"] {
                assert!(slot[key].is_u64(), "{what}.{key} is not an integer: {slot}");
            }
            assert!(
                slot["healthy"].is_boolean() || slot["healthy"].is_null(),
                "{what}.healthy is neither a boolean nor null: {slot}"
            );
        }
    }
}

async fn spawn_edge_not_durable() -> SocketAddr {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let base = support::base_state(driver, EventSink::new(tx));
    let state = AppState {
        readiness: quarry_edge::state::ReadinessState {
            durable: false,
            reason: Some("event store is in memory".to_owned()),
        },
        ..base
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

/// The `/ready` bodies the edge really sends, checked against the schemas of
/// docs/openapi.yaml in both directions (see `mod openapi`). The `draining`
/// body is a process-wide switch that would take the other tests of this
/// binary out of rotation, so it is only checked as a documented value.
#[tokio::test]
#[serial]
async fn ready_bodies_match_the_documented_schemas_both_ways() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/openapi.yaml"
    ))
    .expect("docs/openapi.yaml")
    .replace("\r\n", "\n");
    let lines: Vec<&str> = text.lines().collect();
    let ready = lines
        .iter()
        .position(|line| *line == "  /ready:")
        .expect("the /ready path");
    let lines = &lines[ready..];
    let end = lines[1..]
        .iter()
        .position(|line| indent_of(line) <= 2 && !line.trim().is_empty())
        .map_or(lines.len(), |i| i + 1);
    let lines = &lines[..end];
    assert!(
        !lines.iter().any(|line| line.contains("text/plain")),
        "/ready is documented as JSON"
    );

    let responses = openapi::nav(lines, 0, &["get", "responses"]);
    let ok = openapi::nav(
        lines,
        responses,
        &["'200'", "content", "application/json", "schema"],
    );
    let err = openapi::nav(
        lines,
        responses,
        &["'503'", "content", "application/json", "schema"],
    );
    let ok_schema = openapi::object(lines, ok);
    let err_schema = openapi::object(lines, err);
    let browser = openapi::nav(lines, err, &["properties", "browser"]);
    let browser_schema = openapi::object(lines, browser);
    let slot_item = openapi::nav(lines, browser, &["properties", "slots", "items"]);
    let slot_schema = openapi::object(lines, slot_item);

    // The 200 body is the 503 body's `ready` shape: its browser objects are
    // the same schema, by reference.
    for key in ["browser", "scrape_browser"] {
        assert!(ok_schema.properties.contains(key));
        let at = openapi::nav(lines, ok, &["properties", key]);
        assert!(
            lines[openapi::child(lines, at, "$ref")].contains("browser"),
            "{key} of the 200 body must reference the browser schema"
        );
    }

    // Bodies, built from the real code paths.
    let mut bodies: Vec<(u16, Value)> = Vec::new();
    let healthy = BrowserHealth::from_slots(40, 2, vec![slot("ready", Some(true))]);
    let unhealthy = BrowserHealth::from_slots(40, 2, vec![slot("ready", Some(false))]);
    // One browser with a slot in each status, one never probed (`healthy: null`).
    let mut one_of_each = vec![
        slot("starting", None),
        slot("ready", Some(true)),
        slot("retiring", Some(true)),
        slot("dead", Some(false)),
    ];
    for (i, one) in one_of_each.iter_mut().enumerate() {
        one.slot = i;
    }
    let degraded = BrowserHealth::from_slots(40, 1, one_of_each);
    assert_eq!(degraded.state, BrowserHealthState::Degraded);

    let addr = spawn_edge_with_scrape_browser(
        Arc::new(FixedHealth(healthy.clone())),
        Arc::new(FixedHealth(degraded)),
    )
    .await;
    bodies.push(get_json(addr, "/ready").await);
    let addr = spawn_edge_with_scrape_browser(
        Arc::new(FixedHealth(unhealthy)),
        Arc::new(quarry_browser::chromiumoxide::ChromiumoxideDriver::new()),
    )
    .await;
    bodies.push(get_json(addr, "/ready").await);
    let addr = spawn_edge(Arc::new(FixedHealth(BrowserHealth::not_applicable()))).await;
    bodies.push(get_json(addr, "/ready").await);
    bodies.push(get_json(spawn_edge_not_durable().await, "/ready").await);
    let _ = healthy;

    let mut seen = Seen::default();
    for (status, body) in &bodies {
        check_ready_body(
            *status,
            body,
            &ok_schema,
            &err_schema,
            lines,
            &browser_schema,
            &slot_schema,
            &mut seen,
        );
    }
    assert_eq!(
        bodies.iter().map(|(status, _)| *status).collect::<Vec<_>>(),
        vec![200, 503, 200, 503]
    );

    // Everything documented that the edge can send was sent by one of them.
    assert_eq!(seen.ready, err_schema.properties, "/ready keys sent vs documented");
    assert_eq!(seen.browser, browser_schema.properties, "browser keys sent vs documented");
    assert_eq!(seen.slot, slot_schema.properties, "slot keys sent vs documented");
    let set = |items: &[&str]| -> std::collections::BTreeSet<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    };
    let state_prop = openapi::nav(lines, browser, &["properties", "state"]);
    let slot_status_prop = openapi::nav(lines, slot_item, &["properties", "status"]);
    // The documented enums are the code's, variant for variant: the match
    // has no wildcard, so a new variant stops this test from compiling.
    let state_name = |state: BrowserHealthState| match state {
        BrowserHealthState::NotApplicable => None,
        BrowserHealthState::Idle => Some("idle"),
        BrowserHealthState::Healthy => Some("healthy"),
        BrowserHealthState::Degraded => Some("degraded"),
        BrowserHealthState::Unhealthy => Some("unhealthy"),
    };
    let code_states: std::collections::BTreeSet<String> = [
        BrowserHealthState::NotApplicable,
        BrowserHealthState::Idle,
        BrowserHealthState::Healthy,
        BrowserHealthState::Degraded,
        BrowserHealthState::Unhealthy,
    ]
    .into_iter()
    .filter_map(state_name)
    .map(str::to_owned)
    .collect();
    assert_eq!(openapi::enum_of(lines, state_prop), code_states);
    assert_eq!(seen.state, code_states, "every state was sent");
    let slot_statuses = |status: quarry_browser::slots::SlotStatus| {
        use quarry_browser::slots::SlotStatus::*;
        match status {
            Starting | Ready | Retiring | Dead => status.as_str().to_owned(),
        }
    };
    let code_slot_statuses: std::collections::BTreeSet<String> = {
        use quarry_browser::slots::SlotStatus::*;
        [Starting, Ready, Retiring, Dead]
            .into_iter()
            .map(slot_statuses)
            .collect()
    };
    assert_eq!(openapi::enum_of(lines, slot_status_prop), code_slot_statuses);
    assert_eq!(seen.slot_status, code_slot_statuses, "every slot status was sent");
    assert_eq!(seen.ready_status, set(&["ready", "not_ready"]));
    // `draining` is documented for the 503 body but not sent here.
    let err_status = openapi::nav(lines, err, &["properties", "status"]);
    assert_eq!(
        openapi::enum_of(lines, err_status),
        set(&["draining", "not_ready"])
    );
    assert_eq!(err_schema.required, set(&["status"]));
    assert_eq!(ok_schema.required, set(&["status", "durable"]));
    assert_eq!(
        browser_schema.required,
        set(&["state", "max_sessions", "sessions_open", "slots"])
    );
    assert_eq!(
        slot_schema.required,
        set(&["slot", "status", "generation", "sessions", "pages", "healthy", "probe_failures"])
    );
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}
