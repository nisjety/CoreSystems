//! The ZDR run tombstone (WS4.J): what it may carry, when it is sent, and that it can
//! never fail a request.
//!
//! OFF BY DEFAULT, and counsel review gates turning it on (see `src/zdr_tombstone.rs`).
//! These tests build the emitter by hand with a capturing sink; no env file, compose
//! file or default turns it on, and `the_flag_defaults_off` pins that.
//!
//! The allowlist is `crates/quarry-runtime/contracts/zdr-tombstone.schema.json`. The
//! tests compare the edge struct's keys, required set and vocabularies to it, and
//! validate every emitted body against it with a small checker driven by the schema
//! file (the workspace has no JSON Schema crate).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_edge::state::AppState;
use quarry_edge::zdr_tombstone::{
    emit_run_end, Kind, Outcome, Pending, RunEnd, SinkError, Tombstone, TombstoneEmitter,
    TombstoneSink,
};
use quarry_runtime::{fetch::FetchResponse, Driver, EventSink};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use url::Url;

#[cfg(feature = "browser-agent")]
#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

const SCHEMA: &str = include_str!("../../quarry-runtime/contracts/zdr-tombstone.schema.json");
const SCHEMA_PIN: &str = include_str!("../../quarry-runtime/contracts/zdr-tombstone.schema.json.sha256");
const BEARER: &str = "zdr-tombstone-dev-token";

// ── the capturing sink ──────────────────────────────────────────────────────

#[derive(Default)]
struct Capture {
    bodies: Mutex<Vec<Value>>,
}

impl Capture {
    fn all(&self) -> Vec<Value> {
        self.bodies.lock().unwrap().clone()
    }
}

struct CaptureSink(Arc<Capture>);

#[async_trait]
impl TombstoneSink for CaptureSink {
    async fn put(&self, tombstone: &Tombstone) -> Result<(), SinkError> {
        let body: Value = serde_json::from_slice(&tombstone.to_body()).expect("a tombstone body is JSON");
        self.0.bodies.lock().unwrap().push(body);
        Ok(())
    }
}

struct FailingSink;

#[async_trait]
impl TombstoneSink for FailingSink {
    async fn put(&self, _tombstone: &Tombstone) -> Result<(), SinkError> {
        Err(SinkError::Status(503))
    }
}

struct HangingSink;

#[async_trait]
impl TombstoneSink for HangingSink {
    async fn put(&self, _tombstone: &Tombstone) -> Result<(), SinkError> {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(())
    }
}

fn capturing() -> (Arc<Capture>, Arc<TombstoneEmitter>) {
    let capture = Arc::new(Capture::default());
    let emitter = Arc::new(TombstoneEmitter::new(Arc::new(CaptureSink(capture.clone()))));
    (capture, emitter)
}

async fn wait_for(capture: &Capture, count: usize) {
    for _ in 0..100 {
        if capture.all().len() >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

/// Long enough that a second, unwanted emission would have landed.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

// ── a mini JSON Schema checker, driven by the schema file ───────────────────

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("the schema is JSON")
}

fn id_ok(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn validate(body: &Value) -> Result<(), String> {
    let schema = schema();
    let object = body.as_object().ok_or("the body is not an object")?;
    let properties = schema["properties"].as_object().expect("properties");
    if schema["additionalProperties"] != json!(false) {
        return Err("the schema must forbid additional properties".into());
    }
    for key in object.keys() {
        if !properties.contains_key(key) {
            return Err(format!("`{key}` is outside the allowlist"));
        }
    }
    for required in schema["required"].as_array().expect("required") {
        let name = required.as_str().unwrap();
        if !object.contains_key(name) {
            return Err(format!("`{name}` is required"));
        }
    }
    for (key, value) in object {
        let rule = &properties[key];
        if let Some(allowed) = rule.get("enum") {
            if !allowed.as_array().unwrap().contains(value) {
                return Err(format!("`{key}` is outside its vocabulary"));
            }
        }
        if let Some(constant) = rule.get("const") {
            if constant != value {
                return Err(format!("`{key}` must be {constant}"));
            }
        }
        if let Some(pattern) = rule.get("pattern") {
            assert_eq!(pattern, "^[A-Za-z0-9_-]{1,64}$", "the checker knows only the id pattern");
            if !value.as_str().is_some_and(id_ok) {
                return Err(format!("`{key}` is not id-shaped"));
            }
        }
        match rule.get("type").and_then(Value::as_str) {
            Some("string") if !value.is_string() => return Err(format!("`{key}` must be a string")),
            Some("integer") if !(value.is_u64() || value.is_i64()) => {
                return Err(format!("`{key}` must be an integer"))
            }
            Some("number") if !value.is_number() => return Err(format!("`{key}` must be a number")),
            _ => {}
        }
        if let Some(minimum) = rule.get("minimum").and_then(Value::as_f64) {
            if value.as_f64().is_some_and(|v| v < minimum) {
                return Err(format!("`{key}` is below its minimum"));
            }
        }
        if rule.get("format").and_then(Value::as_str) == Some("date-time")
            && chrono::DateTime::parse_from_rfc3339(value.as_str().unwrap_or("")).is_err()
        {
            return Err(format!("`{key}` is not an RFC 3339 timestamp"));
        }
    }
    Ok(())
}

fn schema_keys() -> BTreeSet<String> {
    schema()["properties"].as_object().unwrap().keys().cloned().collect()
}

fn enum_of(key: &str) -> BTreeSet<String> {
    schema()["properties"][key]["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("`{key}` has no enum"))
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

fn full_tombstone() -> Tombstone {
    Tombstone::new("run_01HZZZZZZZZZZZZZZZZZZZZZZZ", "org_alpha", Kind::Scrape, Outcome::Timeout, Utc::now())
        .expect("id-shaped")
        .with_started_at(Utc::now() - chrono::Duration::seconds(2))
        .with_driver(Some(DriverKind::Browser))
        .with_step_count(4)
        .with_cost_units(0.25)
        .with_error(&QuarryError::new(ErrorCode::Timeout, "secret page text must never travel"))
}

// ── the allowlist, struct against schema ────────────────────────────────────

#[test]
fn the_schema_is_the_one_its_pin_names() {
    use sha2::{Digest, Sha256};
    let digest = format!("{:x}", Sha256::digest(SCHEMA.as_bytes()));
    assert_eq!(digest, SCHEMA_PIN.split_whitespace().next().unwrap());
}

/// The test the plan asks for: a field outside the allowlist fails. A fully populated
/// tombstone carries exactly the schema's keys, so a field added to the struct (and not
/// to the schema) shows up here as a key the schema does not name.
#[test]
fn the_edge_struct_names_exactly_the_schema_keys_and_nothing_outside_them() {
    let body: Value = serde_json::from_slice(&full_tombstone().to_body()).unwrap();
    let keys: BTreeSet<String> = body.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, schema_keys(), "the emitted keys differ from the allowlist");
    validate(&body).expect("a full tombstone validates");
    // The minimal one carries exactly the required keys.
    let minimal: Value = serde_json::from_slice(
        &Tombstone::new("run_1", "org_1", Kind::Search, Outcome::Ok, Utc::now())
            .unwrap()
            .to_body(),
    )
    .unwrap();
    let required: BTreeSet<String> = schema()["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    let minimal_keys: BTreeSet<String> = minimal.as_object().unwrap().keys().cloned().collect();
    assert_eq!(minimal_keys, required);
    validate(&minimal).expect("a minimal tombstone validates");
}

#[test]
fn the_checker_itself_refuses_a_field_outside_the_allowlist() {
    let mut body: Value = serde_json::from_slice(&full_tombstone().to_body()).unwrap();
    for extra in ["url", "title", "query", "user_id", "text", "digest", "fingerprint", "error", "zdr"] {
        body[extra] = json!("x");
        assert!(validate(&body).is_err(), "`{extra}` must be refused");
        body.as_object_mut().unwrap().remove(extra);
    }
    body["outcome"] = json!("fine");
    assert!(validate(&body).is_err(), "a value outside the vocabulary must be refused");
}

#[test]
fn the_vocabularies_are_the_schemas() {
    // Every variant of the closed enums, serialized, is in the schema's list, and the
    // lists are exactly those variants.
    let kinds: BTreeSet<String> = [Kind::Scrape, Kind::Search, Kind::Answer, Kind::Extract, Kind::AgentRun]
        .iter()
        .map(|k| serde_json::to_value(k).unwrap().as_str().unwrap().to_owned())
        .collect();
    assert_eq!(kinds, enum_of("kind"));
    let outcomes: BTreeSet<String> = [Outcome::Ok, Outcome::Error, Outcome::Refused, Outcome::Cancelled, Outcome::Timeout]
        .iter()
        .map(|o| serde_json::to_value(o).unwrap().as_str().unwrap().to_owned())
        .collect();
    assert_eq!(outcomes, enum_of("outcome"));
    let drivers: BTreeSet<String> = [DriverKind::Static, DriverKind::Browser, DriverKind::Tls]
        .iter()
        .map(|d| serde_json::to_value(d).unwrap().as_str().unwrap().to_owned())
        .collect();
    assert_eq!(drivers, enum_of("driver"));

    // Every ErrorCode wire name is a valid class (the match is exhaustive, so a new
    // code must be added here and to the schema), plus the transport classes.
    fn every_code() -> Vec<ErrorCode> {
        let all = [
            ErrorCode::BadRequest, ErrorCode::Unauthorized, ErrorCode::Forbidden, ErrorCode::NotFound,
            ErrorCode::Conflict, ErrorCode::RateLimited, ErrorCode::Timeout, ErrorCode::SecurityBlocked,
            ErrorCode::ActionUnknown, ErrorCode::ChallengeDetected, ErrorCode::RuntimeNotReady,
            ErrorCode::CheckpointLost, ErrorCode::TargetRepairRequired, ErrorCode::DriverFailed,
            ErrorCode::UpstreamBlocked, ErrorCode::Unsupported, ErrorCode::UnsupportedMedia,
            ErrorCode::BudgetExhausted, ErrorCode::UserTookOver, ErrorCode::Internal,
        ];
        for code in all {
            match code {
                ErrorCode::BadRequest | ErrorCode::Unauthorized | ErrorCode::Forbidden | ErrorCode::NotFound
                | ErrorCode::Conflict | ErrorCode::RateLimited | ErrorCode::Timeout | ErrorCode::SecurityBlocked
                | ErrorCode::ActionUnknown | ErrorCode::ChallengeDetected | ErrorCode::RuntimeNotReady
                | ErrorCode::CheckpointLost | ErrorCode::TargetRepairRequired | ErrorCode::DriverFailed
                | ErrorCode::UpstreamBlocked | ErrorCode::Unsupported | ErrorCode::UnsupportedMedia
                | ErrorCode::BudgetExhausted | ErrorCode::UserTookOver | ErrorCode::Internal => {}
            }
        }
        all.to_vec()
    }
    let mut classes: BTreeSet<String> = every_code().iter().map(|c| c.wire_name().to_owned()).collect();
    assert_eq!(classes.len(), 20);
    for transport in ["timeout", "connect", "status", "decode", "request"] {
        classes.insert(transport.to_owned());
    }
    assert_eq!(classes, enum_of("error_class"));

    // The status table only ever yields a class in the vocabulary.
    let allowed = enum_of("error_class");
    for status in 100u16..=599 {
        if let (_, Some(class)) = quarry_edge::zdr_tombstone::outcome_of_status(status) {
            assert!(allowed.contains(class), "status {status} maps to `{class}`");
        }
    }
}

#[test]
fn an_error_travels_as_its_class_never_its_text() {
    let text = "https://secret.example/page?q=canary-4711 refused";
    let body = String::from_utf8(
        Tombstone::new("run_1", "org_1", Kind::Scrape, Outcome::Error, Utc::now())
            .unwrap()
            .with_error(&QuarryError::new(ErrorCode::DriverFailed, text))
            .to_body(),
    )
    .unwrap();
    assert!(body.contains("DRIVER_FAILED"));
    for leak in ["secret.example", "canary", "://", "refused"] {
        assert!(!body.contains(leak), "the tombstone leaks {leak:?}: {body}");
    }
}

#[test]
fn an_id_outside_the_pattern_gives_no_tombstone() {
    let now = Utc::now();
    for bad in ["", "https://example.com/x", "run 1", "org:1", &"a".repeat(65), "run/1"] {
        assert!(Tombstone::new(bad, "org_1", Kind::Scrape, Outcome::Ok, now).is_none(), "run id {bad:?}");
        assert!(Tombstone::new("run_1", bad, Kind::Scrape, Outcome::Ok, now).is_none(), "org id {bad:?}");
    }
    assert!(Tombstone::new(&"a".repeat(64), "org_1", Kind::Scrape, Outcome::Ok, now).is_some());
}

#[test]
fn a_bad_cost_is_dropped_not_sent() {
    for bad in [f64::NAN, f64::INFINITY, -1.0, 1e12] {
        let body: Value = serde_json::from_slice(
            &Tombstone::new("run_1", "org_1", Kind::Scrape, Outcome::Ok, Utc::now())
                .unwrap()
                .with_cost_units(bad)
                .to_body(),
        )
        .unwrap();
        assert!(body.get("cost_units").is_none(), "cost {bad} was sent");
    }
}

// ── the flag is off ─────────────────────────────────────────────────────────

#[test]
fn the_flag_defaults_off() {
    // Nothing in the environment of this test process sets it; with the variable absent the config is off.
    std::env::remove_var("QUARRY_EDGE__ZDR_TOMBSTONE");
    let from_env = quarry_edge::config::EdgeConfig::from_env().expect("config from env");
    assert!(!from_env.zdr_tombstone);
}

/// No env file, compose file, Dockerfile or workflow in this workspace sets the flag.
#[test]
fn no_file_in_the_workspace_turns_the_flag_on() {
    fn scan(dir: &std::path::Path, hits: &mut Vec<String>, depth: usize) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if depth < 6 && !matches!(name.as_str(), "target" | ".git" | "node_modules") {
                    scan(&path, hits, depth + 1);
                }
                continue;
            }
            let interesting = name.ends_with(".yml")
                || name.ends_with(".yaml")
                || name.ends_with(".env")
                || name.starts_with(".env")
                || name.starts_with("Dockerfile");
            if !interesting {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('#') || !line.contains("ZDR_TOMBSTONE") {
                    continue;
                }
                let value = line
                    .rsplit(|c| c == '=' || c == ':')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_ascii_lowercase();
                if ["1", "true", "on", "yes"].contains(&value.as_str()) {
                    hits.push(format!("{}: {line}", path.display()));
                }
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut hits = Vec::new();
    scan(&root, &mut hits, 0);
    assert!(hits.is_empty(), "a file turns the tombstone on: {hits:?}");
}

// ── emission ────────────────────────────────────────────────────────────────

struct StubPageDriver;

#[async_trait]
impl Driver for StubPageDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> quarry_core::error::QuarryResult<FetchResponse> {
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![("content-type".to_string(), "text/html".to_string())],
            body: b"<html><head><title>Hidden title</title></head><body><main><p>Hidden page text that is long enough to read as content.</p></main></body></html>".to_vec(),
            duration_ms: 1,
            served_by: DriverKind::Static,
            captures: None,
        })
    }
}

async fn spawn_edge(tombstones: Option<Arc<TombstoneEmitter>>) -> std::net::SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(1024);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let state = AppState {
        tombstones,
        ..support::base_state(Arc::new(StubPageDriver), EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn post(addr: std::net::SocketAddr, route: &str, body: Value) -> (u16, String) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}{route}"))
        .bearer_auth(BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    (response.status().as_u16(), response.text().await.unwrap())
}

const SECRET_URL: &str = "https://stub.example/private/path-canary-9981?token=canary-9981";

#[tokio::test]
async fn a_zdr_scrape_emits_exactly_one_tombstone_without_any_url() {
    let (capture, emitter) = capturing();
    let addr = spawn_edge(Some(emitter.clone())).await;

    let (status, _) = post(addr, "/v1/scrape", json!({ "url": SECRET_URL, "zdr": true })).await;
    assert_eq!(status, 200);
    wait_for(&capture, 1).await;
    settle().await;

    let bodies = capture.all();
    assert_eq!(bodies.len(), 1, "exactly one tombstone: {bodies:?}");
    let body = &bodies[0];
    validate(body).expect("the emitted body validates against the schema");
    assert_eq!(body["kind"], "scrape");
    assert_eq!(body["retention_class"], "zdr");
    assert_eq!(body["outcome"], "ok");
    assert_eq!(body["driver"], "static");
    assert_eq!(body["org_id"], "org_placeholder");
    assert!(body["run_id"].as_str().unwrap().starts_with("run_"));
    let text = body.to_string();
    for leak in ["stub.example", "canary", "path", "token", "://", "Hidden", "title"] {
        assert!(!text.contains(leak), "the tombstone leaks {leak:?}: {text}");
    }
    assert_eq!(emitter.counters().sent, 1);
}

#[tokio::test]
async fn a_standard_scrape_emits_none() {
    let (capture, emitter) = capturing();
    let addr = spawn_edge(Some(emitter.clone())).await;
    let (status, _) = post(addr, "/v1/scrape", json!({ "url": SECRET_URL })).await;
    assert_eq!(status, 200);
    settle().await;
    assert!(capture.all().is_empty(), "a standard run emitted: {:?}", capture.all());
    assert_eq!(emitter.counters(), Default::default());
}

#[tokio::test]
async fn with_no_emitter_a_zdr_scrape_runs_and_nothing_is_sent() {
    let addr = spawn_edge(None).await;
    let (status, _) = post(addr, "/v1/scrape", json!({ "url": SECRET_URL, "zdr": true })).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn a_zdr_streamed_scrape_emits_one_tombstone() {
    let (capture, emitter) = capturing();
    let addr = spawn_edge(Some(emitter)).await;
    let (status, body) = post(addr, "/v1/scrape/stream", json!({ "url": SECRET_URL, "zdr": true })).await;
    assert_eq!(status, 200);
    assert!(body.contains("done"), "the stream finished: {body}");
    wait_for(&capture, 1).await;
    settle().await;
    let bodies = capture.all();
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    validate(&bodies[0]).unwrap();
    assert_eq!(bodies[0]["kind"], "scrape");
    assert!(!bodies[0].to_string().contains("stub.example"));
}

#[tokio::test]
async fn a_tombstone_failure_never_fails_the_request() {
    for sink in [Arc::new(FailingSink) as Arc<dyn TombstoneSink>, Arc::new(HangingSink)] {
        let emitter = Arc::new(TombstoneEmitter::new(sink));
        let addr = spawn_edge(Some(emitter.clone())).await;
        let started = std::time::Instant::now();
        let (status, body) = post(addr, "/v1/scrape", json!({ "url": SECRET_URL, "zdr": true })).await;
        assert_eq!(status, 200, "a sink failure failed the request: {body}");
        assert!(started.elapsed() < Duration::from_secs(1), "the request waited on the sink");
    }
    // A failing sink is counted, not raised.
    let emitter = Arc::new(TombstoneEmitter::new(Arc::new(FailingSink)));
    let addr = spawn_edge(Some(emitter.clone())).await;
    let (status, _) = post(addr, "/v1/scrape", json!({ "url": SECRET_URL, "zdr": true })).await;
    assert_eq!(status, 200);
    for _ in 0..50 {
        if emitter.counters().failed == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(emitter.counters().failed, 1);
    assert_eq!(emitter.counters().sent, 0);
}

#[tokio::test]
async fn a_full_in_flight_cap_drops_and_counts_without_waiting() {
    let emitter = Arc::new(TombstoneEmitter::with_capacity(Arc::new(HangingSink), 2));
    let started = std::time::Instant::now();
    for index in 0..5 {
        emit_run_end(
            Some(&emitter),
            true,
            &RunEnd::new(&format!("run_{index}"), "org_1", Kind::Scrape).started(Utc::now()),
        );
    }
    assert!(started.elapsed() < Duration::from_millis(500), "emission waited");
    assert_eq!(emitter.counters().dropped, 3, "two in flight, three dropped");
}

/// The 2 s `SEND_TIMEOUT` is a field of the emitter, so a test can shorten it. A
/// delivery that hangs is abandoned, counted as failed, and its permit comes back:
/// with the timeout wrapper removed the first emit would hold the only permit for
/// 60 s and the third emit below would be dropped.
#[tokio::test]
async fn a_hung_delivery_times_out_is_counted_failed_and_frees_its_permit() {
    let emitter = Arc::new(TombstoneEmitter::with_limits(
        Arc::new(HangingSink),
        1,
        Duration::from_millis(100),
    ));
    emit_run_end(Some(&emitter), true, &RunEnd::new("run_1", "org_1", Kind::Scrape).started(Utc::now()));
    // The only permit is held while the delivery hangs.
    emit_run_end(Some(&emitter), true, &RunEnd::new("run_2", "org_1", Kind::Scrape).started(Utc::now()));
    assert_eq!(emitter.counters().dropped, 1, "the second emit found no permit");
    for _ in 0..100 {
        if emitter.counters().failed == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(emitter.counters().failed, 1, "the abandoned delivery is counted as failed");
    assert_eq!(emitter.counters().sent, 0);
    // The permit is back: a third emit is accepted, not dropped.
    emit_run_end(Some(&emitter), true, &RunEnd::new("run_3", "org_1", Kind::Scrape).started(Utc::now()));
    assert_eq!(emitter.counters().dropped, 1, "the permit was not released after the timeout");
}

// ── HttpSink on the wire ────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct Seen {
    requests: Arc<Mutex<Vec<(String, String, Vec<(String, String)>, Vec<u8>)>>>,
}

async fn stub_control(status: u16) -> (std::net::SocketAddr, Seen) {
    use axum::extract::State;
    use axum::http::{HeaderMap, Method, StatusCode, Uri};

    async fn handle(
        State((seen, status)): State<(Seen, u16)>,
        method: Method,
        uri: Uri,
        headers: HeaderMap,
        body: axum::body::Bytes,
    ) -> StatusCode {
        let headers = headers
            .iter()
            .map(|(name, value)| (name.as_str().to_ascii_lowercase(), value.to_str().unwrap_or("").to_owned()))
            .collect();
        seen.requests
            .lock()
            .unwrap()
            .push((method.to_string(), uri.to_string(), headers, body.to_vec()));
        StatusCode::from_u16(status).unwrap()
    }

    let seen = Seen::default();
    let app = axum::Router::new().fallback(handle).with_state((seen.clone(), status));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, seen)
}

fn signer() -> Arc<quarry_edge::internal_auth::InternalSigner> {
    Arc::new(quarry_edge::internal_auth::InternalSigner::new("0123456789abcdef0123456789abcdef").unwrap())
}

/// The only path that sends data off the edge: a POST to quarry-control's receiver,
/// the body exactly `to_body()`, and the three signature headers verifying over the
/// same canonical string the Go verifier rebuilds.
#[tokio::test]
async fn the_http_sink_posts_the_signed_allowlisted_body_to_the_receiver_path() {
    use quarry_edge::internal_auth::{InternalSigner, HEADER_NONCE, HEADER_SIG, HEADER_TS};
    use quarry_edge::zdr_tombstone::{HttpSink, PATH};

    let (addr, seen) = stub_control(204).await;
    let signer = signer();
    // A trailing slash on the base URL must not double the slash in the path.
    let sink = HttpSink::new(&format!("http://{addr}/"), signer.clone()).expect("client builds");
    let tombstone = full_tombstone();
    sink.put(&tombstone).await.expect("a 204 is a delivery");

    let requests = seen.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let (method, uri, headers, body) = &requests[0];
    assert_eq!(method, "POST");
    assert_eq!(uri, PATH);
    assert_eq!(PATH, "/v1/internal/zdr-tombstones");
    assert_eq!(body, &tombstone.to_body(), "the wire body is exactly the allowlisted struct");
    let header = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key == &name.to_ascii_lowercase())
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("header {name} missing in {headers:?}"))
    };
    assert_eq!(header("content-type"), "application/json");
    let timestamp: i64 = header(HEADER_TS).parse().expect("a unix timestamp");
    let nonce = header(HEADER_NONCE);
    assert_eq!(nonce.len(), 32);
    let expected = InternalSigner::header_value(&signer.sign_raw(
        "POST",
        PATH,
        &InternalSigner::body_digest(body),
        timestamp,
        &nonce,
    ));
    assert_eq!(header(HEADER_SIG), expected, "the signature does not verify over method, path, body digest, ts and nonce");
    // The body carries no URL even though the stub saw the real wire bytes.
    assert!(!String::from_utf8_lossy(body).contains("://"));
}

#[tokio::test]
async fn the_http_sink_maps_a_refusal_to_a_status_and_a_dead_peer_to_a_class() {
    use quarry_edge::zdr_tombstone::HttpSink;

    let (addr, _seen) = stub_control(503).await;
    let sink = HttpSink::new(&format!("http://{addr}"), signer()).unwrap();
    assert_eq!(sink.put(&full_tombstone()).await, Err(SinkError::Status(503)));

    // A port nothing listens on: a connect failure, reported as a class.
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let sink = HttpSink::new(&format!("http://{dead}"), signer()).unwrap();
    assert_eq!(sink.put(&full_tombstone()).await, Err(SinkError::Transport("connect")));

    // And through an emitter a refusal is counted, not raised.
    let (addr, _seen) = stub_control(500).await;
    let emitter = Arc::new(TombstoneEmitter::new(Arc::new(
        HttpSink::new(&format!("http://{addr}"), signer()).unwrap(),
    )));
    emit_run_end(Some(&emitter), true, &RunEnd::new("run_x", "org_1", Kind::Scrape).started(Utc::now()));
    for _ in 0..100 {
        if emitter.counters().failed == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(emitter.counters().failed, 1);
    assert_eq!(emitter.counters().sent, 0);
}

#[tokio::test]
async fn emit_run_end_sends_nothing_for_a_standard_run() {
    let (capture, emitter) = capturing();
    emit_run_end(Some(&emitter), false, &RunEnd::new("run_1", "org_1", Kind::Scrape));
    emit_run_end(None, true, &RunEnd::new("run_1", "org_1", Kind::Scrape));
    assert!(Pending::begin(Some(&emitter), false, "org_1", Kind::Search).is_none());
    assert!(Pending::begin(None, true, "org_1", Kind::Search).is_none());
    settle().await;
    assert!(capture.all().is_empty());
}

#[tokio::test]
async fn a_failed_zdr_run_carries_its_class_and_outcome() {
    let (capture, emitter) = capturing();
    let error = QuarryError::new(ErrorCode::Timeout, "page text that must not travel");
    emit_run_end(
        Some(&emitter),
        true,
        &RunEnd::new("run_1", "org_1", Kind::Scrape).started(Utc::now()).failed(&error),
    );
    wait_for(&capture, 1).await;
    let body = &capture.all()[0];
    validate(body).unwrap();
    assert_eq!(body["outcome"], "timeout");
    assert_eq!(body["error_class"], "TIMEOUT");
    assert!(!body.to_string().contains("page text"));
}

// ── search, answer, extract (the response-status seam) ──────────────────────

#[tokio::test]
async fn a_zdr_search_with_no_provider_is_a_refused_tombstone_and_a_standard_one_is_none() {
    let (capture, emitter) = capturing();
    let addr = spawn_edge(Some(emitter)).await;
    let (status, _) = post(addr, "/v1/search", json!({ "query": "private canary query", "zdr": true })).await;
    assert_eq!(status, 501, "no provider is configured in this harness");
    wait_for(&capture, 1).await;
    settle().await;
    let bodies = capture.all();
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    validate(&bodies[0]).unwrap();
    assert_eq!(bodies[0]["kind"], "search");
    assert_eq!(bodies[0]["outcome"], "refused");
    assert_eq!(bodies[0]["error_class"], "UNSUPPORTED");
    assert!(!bodies[0].to_string().contains("canary"));

    // A standard search adds none.
    let (status, _) = post(addr, "/v1/search", json!({ "query": "plain query" })).await;
    assert_eq!(status, 501);
    // A request refused before any work (an empty query) is not a run.
    let (status, _) = post(addr, "/v1/search", json!({ "query": "  ", "zdr": true })).await;
    assert_eq!(status, 400);
    settle().await;
    assert_eq!(capture.all().len(), 1, "only the ZDR search that ran: {:?}", capture.all());
}

#[tokio::test]
async fn a_zdr_answer_and_a_zdr_extract_each_emit_one() {
    let (capture, emitter) = capturing();
    let addr = spawn_edge(Some(emitter)).await;
    let (status, _) = post(addr, "/v1/answer", json!({ "query": "private canary", "zdr": true })).await;
    assert_eq!(status, 403, "INNH-8: a ZDR answer is refused (ADR-4), whatever the deployment's gate says");
    let (status, _) = post(addr, "/v1/extract", json!({ "urls": [SECRET_URL], "zdr": true })).await;
    assert_eq!(status, 200);
    wait_for(&capture, 2).await;
    settle().await;
    let bodies = capture.all();
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    let mut kinds: Vec<String> = bodies.iter().map(|b| b["kind"].as_str().unwrap().to_owned()).collect();
    kinds.sort();
    assert_eq!(kinds, ["answer", "extract"]);
    for body in &bodies {
        validate(body).unwrap();
        let text = body.to_string();
        for leak in ["canary", "stub.example", "://"] {
            assert!(!text.contains(leak), "{text}");
        }
    }
    let extract = bodies.iter().find(|b| b["kind"] == "extract").unwrap();
    assert_eq!(extract["outcome"], "ok");
}

// ── /v1/answer/stream (D1-003): the tombstone comes from the stream's end ─────

/// What the search step of the answer pipeline does in a stream test.
#[derive(Clone, Copy)]
enum SearchBehaviour {
    /// No results: the stream ends at once with an empty `done` (no model call).
    Empty,
    /// The provider fails with this code.
    Fails(ErrorCode),
    /// The provider never answers (a stream still running when the client leaves).
    Hangs,
}

struct ScriptedSearch(SearchBehaviour);

#[async_trait]
impl quarry_runtime::serp::SearchProvider for ScriptedSearch {
    async fn search(
        &self,
        _query: &str,
        _opts: &quarry_runtime::serp::SearchOptions,
    ) -> quarry_core::QuarryResult<Vec<quarry_runtime::serp::SearchResult>> {
        match self.0 {
            SearchBehaviour::Empty => Ok(Vec::new()),
            SearchBehaviour::Fails(code) => Err(QuarryError::new(code, "provider text that must not travel")),
            SearchBehaviour::Hangs => {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(Vec::new())
            }
        }
    }

    fn name(&self) -> &str {
        "scripted"
    }
}

/// An edge with an answer pipeline whose search step does `behaviour`.
async fn spawn_answer_edge(tombstones: Option<Arc<TombstoneEmitter>>, behaviour: SearchBehaviour) -> std::net::SocketAddr {
    use quarry_runtime::answer::{AnswerPipeline, SimpleHttpMarkdownFetcher};
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(1024);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let formats = quarry_runtime::ai_formats::AiFormatRunner::new(Arc::new(
        quarry_runtime::mp_client::ModelPlaneClient::new("http://127.0.0.1:9").expect("model plane client"),
    ));
    let pipeline = AnswerPipeline::new(
        Arc::new(ScriptedSearch(behaviour)),
        Arc::new(SimpleHttpMarkdownFetcher::new()),
        formats,
    );
    let state = AppState {
        tombstones,
        answer_pipeline: Some(Arc::new(pipeline)),
        ..support::base_state(Arc::new(StubPageDriver), EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

/// INNH-8 (ADR-4): a ZDR answer stream is refused before it starts, so it leaves one `refused`
/// tombstone (class `FORBIDDEN`, no query in it) and never an `ok` one; a standard stream leaves
/// none; a request refused before any work (an empty query) is not a run.
#[tokio::test]
async fn a_zdr_answer_stream_is_refused_before_it_starts_and_leaves_one_refused_tombstone() {
    let (capture, emitter) = capturing();
    let addr = spawn_answer_edge(Some(emitter), SearchBehaviour::Empty).await;

    let (status, body) = post(addr, "/v1/answer/stream", json!({ "query": "private canary stream", "zdr": true })).await;
    assert_eq!(status, 403, "{body}");
    assert!(!body.contains("event:"), "a refusal is JSON, not a stream: {body}");
    assert!(body.contains("zdr_unavailable"), "{body}");
    wait_for(&capture, 1).await;
    settle().await;
    let bodies = capture.all();
    assert_eq!(bodies.len(), 1, "exactly one tombstone for the one refused run: {bodies:?}");
    validate(&bodies[0]).unwrap();
    assert_eq!(bodies[0]["kind"], "answer");
    assert_eq!(bodies[0]["outcome"], "refused");
    assert_eq!(bodies[0]["error_class"], "FORBIDDEN");
    assert_eq!(bodies[0]["retention_class"], "zdr");
    assert!(!bodies[0].to_string().contains("canary"));

    let (status, _) = post(addr, "/v1/answer/stream", json!({ "query": "plain stream" })).await;
    assert_eq!(status, 200);
    let (status, _) = post(addr, "/v1/answer/stream", json!({ "query": "  ", "zdr": true })).await;
    assert_eq!(status, 400);
    settle().await;
    assert_eq!(capture.all().len(), 1, "a standard stream and a refused request add none: {:?}", capture.all());
}

/// INNH-8: whatever the pipeline behind it would have done (finish, fail, hang, or not exist), a
/// ZDR answer stream is the same prompt 403 with the same refused tombstone: the refusal comes
/// before the pipeline and before the deployment's gate, so no provider is ever asked.
#[tokio::test]
async fn a_zdr_answer_stream_never_reaches_the_pipeline_whatever_it_would_do() {
    let (capture, emitter) = capturing();
    let mut expected = 0;
    for behaviour in [
        SearchBehaviour::Empty,
        SearchBehaviour::Fails(ErrorCode::Timeout),
        SearchBehaviour::Hangs,
    ] {
        let addr = spawn_answer_edge(Some(emitter.clone()), behaviour).await;
        let started = std::time::Instant::now();
        let (status, body) = post(addr, "/v1/answer/stream", json!({ "query": "private canary", "zdr": true })).await;
        assert_eq!(status, 403, "{body}");
        assert!(started.elapsed() < Duration::from_secs(20), "a hanging provider was waited for");
        assert!(!body.contains("must not travel"), "{body}");
        expected += 1;
    }
    // No pipeline configured: the same 403, not the 501 a standard request gets.
    let unconfigured = spawn_edge(Some(emitter)).await;
    let (status, _) = post(unconfigured, "/v1/answer/stream", json!({ "query": "private canary", "zdr": true })).await;
    assert_eq!(status, 403);
    let (status, _) = post(unconfigured, "/v1/answer/stream", json!({ "query": "private canary" })).await;
    assert_eq!(status, 501);
    expected += 1;
    wait_for(&capture, expected).await;
    settle().await;
    let bodies = capture.all();
    assert_eq!(bodies.len(), expected, "{bodies:?}");
    for body in &bodies {
        validate(body).unwrap();
        assert_eq!(body["kind"], "answer");
        assert_eq!(body["outcome"], "refused");
        assert_eq!(body["error_class"], "FORBIDDEN");
        assert!(!body.to_string().contains("canary"), "{body:?}");
    }
}

/// The guard itself, without a server: a drop with nothing recorded is `cancelled`; a
/// recorded outcome wins; a guard over no pending run (standard run, flag off) sends nothing.
#[tokio::test]
async fn the_stream_guard_sends_once_on_drop_and_nothing_for_a_standard_run() {
    use quarry_edge::zdr_tombstone::StreamEnd;
    let (capture, emitter) = capturing();

    drop(StreamEnd::new(Pending::begin(Some(&emitter), true, "org_1", Kind::Answer)));
    let mut finished = StreamEnd::new(Pending::begin(Some(&emitter), true, "org_1", Kind::Answer));
    finished.ok();
    drop(finished);
    let mut failed = StreamEnd::new(Pending::begin(Some(&emitter), true, "org_1", Kind::Answer));
    failed.failed(&QuarryError::new(ErrorCode::Forbidden, "text that must not travel"));
    drop(failed);
    let mut no_run = StreamEnd::new(Pending::begin(Some(&emitter), true, "org_1", Kind::Answer));
    no_run.status(400);
    drop(no_run);
    drop(StreamEnd::new(Pending::begin(Some(&emitter), false, "org_1", Kind::Answer)));
    drop(StreamEnd::new(Pending::begin(None, true, "org_1", Kind::Answer)));

    wait_for(&capture, 3).await;
    settle().await;
    let bodies = capture.all();
    assert_eq!(bodies.len(), 3, "{bodies:?}");
    let mut outcomes: Vec<(String, Option<String>)> = bodies
        .iter()
        .map(|b| {
            validate(b).unwrap();
            (
                b["outcome"].as_str().unwrap().to_owned(),
                b.get("error_class").and_then(Value::as_str).map(str::to_owned),
            )
        })
        .collect();
    outcomes.sort();
    assert_eq!(
        outcomes,
        [
            ("cancelled".to_owned(), None),
            ("ok".to_owned(), None),
            ("refused".to_owned(), Some("FORBIDDEN".to_owned())),
        ]
    );
}

// ── an agent run ────────────────────────────────────────────────────────────

#[cfg(feature = "browser-agent")]
mod agent {
    use super::*;
    use agent_harness::*;
    use quarry_edge::agent_routes::sweep_once;
    use quarry_edge::run_limits::RunLimits;

    const ZDR_ORG: &str = "org-zdr-golden";
    const MEMBER_ORG: &str = "org-standard-golden";

    fn limits() -> RunLimits {
        RunLimits {
            interactive_idle_ttl_s: 60,
            agent_idle_ttl_s: 60,
            max_concurrent_runs: 0,
            max_runs_per_org: 0,
            max_interactive_runs_per_org: 0,
            ..RunLimits::default()
        }
    }

    async fn edge_with(emitter: Arc<TombstoneEmitter>) -> Edge {
        Edge::start_with(limits(), move |state| AppState { tombstones: Some(emitter), ..state }).await
    }

    #[tokio::test]
    async fn a_closed_agent_run_emits_once() {
        let (capture, emitter) = capturing();
        let edge = edge_with(emitter).await;
        let person = zdr_member_token();
        let run = edge.start_run(&person, json!({})).await;
        let step = edge.step(&person, &run, wait_action(), None).await;
        assert_eq!(step.status, 200, "{}", step.body);

        let closed = edge.delete(&person, &format!("/v1/agent/runs/{run}")).await;
        assert_eq!(closed.status, 200, "{}", closed.body);
        // A second close is the plain 404 and sends nothing.
        let again = edge.delete(&person, &format!("/v1/agent/runs/{run}")).await;
        assert_eq!(again.status, 404);
        wait_for(&capture, 1).await;
        settle().await;

        let bodies = capture.all();
        assert_eq!(bodies.len(), 1, "{bodies:?}");
        let body = &bodies[0];
        validate(body).unwrap();
        assert_eq!(body["kind"], "agent_run");
        assert_eq!(body["outcome"], "ok");
        assert_eq!(body["run_id"], run.as_str());
        assert_eq!(body["org_id"], ZDR_ORG);
        assert!(!body.to_string().contains("://"));
    }

    #[tokio::test]
    async fn an_expired_agent_run_and_a_racing_close_emit_once() {
        let (capture, emitter) = capturing();
        let edge = edge_with(emitter).await;
        let person = zdr_member_token();
        let run = edge.start_run(&person, json!({})).await;

        // The sweeper and the owner's DELETE race for the same run.
        let route = format!("/v1/agent/runs/{run}");
        let sweep = sweep_once(&edge.state, later(120_000));
        let close = edge.delete(&person, &route);
        let (report, _closed) = tokio::join!(sweep, close);
        let _ = report;
        wait_for(&capture, 1).await;
        settle().await;
        assert_eq!(capture.all().len(), 1, "one run, one tombstone: {:?}", capture.all());
        validate(&capture.all()[0]).unwrap();
    }

    #[tokio::test]
    async fn a_standard_agent_run_emits_none() {
        let (capture, emitter) = capturing();
        let edge = edge_with(emitter).await;
        let person = member_token();
        let run = edge.start_run(&person, json!({})).await;
        let closed = edge.delete(&person, &format!("/v1/agent/runs/{run}")).await;
        assert_eq!(closed.status, 200, "{}", closed.body);
        settle().await;
        assert!(capture.all().is_empty(), "a standard run emitted ({MEMBER_ORG}): {:?}", capture.all());
    }
}
