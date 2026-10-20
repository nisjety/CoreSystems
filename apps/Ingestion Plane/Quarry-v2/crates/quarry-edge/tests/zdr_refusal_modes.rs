//! WS4.H: the durable-resource refusal for a Zero Data Retention organization, in its
//! two modes (`QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES`, `AppState::zdr_refuse_durable_resources`).
//!
//! * Observe (flag off, stage A, the default): a ZDR organization is served exactly as before
//!   and each request leaves one content-free `zdr.would_refuse` event in the process log,
//!   naming the matrix row, the action and the organization id, and no URL, host or title.
//!   `zdr_gap_rows.rs` pins the "served as before" half on every row; this file adds the log half.
//! * Enforce (flag on): the same requests are refused 403 with
//!   `error.details.zdr_refusal.code == "zdr_durable_mutation_forbidden"`, before the request
//!   reaches Control Plane or a store. Wind-down routes (list, pause, delete, read) still work,
//!   and a standard organization is unchanged.
//!
//! Own binary: the capturing subscriber is process-global. Real RS256 tokens and the golden Auth
//! Core claims through the real router, like `zdr_gap_rows.rs`.

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod support;
#[path = "support/zdr_harness.rs"]
mod zdr_harness;

use quarry_browser::session::{InMemoryProfileStore, ProfileStore, SessionSnapshot};
use quarry_core::ids::kinds::ProfileKind;
use quarry_edge::state::AppState;

use zdr_harness::{user_in_standard_org, user_in_zdr_org, Harness};

const ZDR_ORG: &str = "org-zdr-golden";
const INTERNAL_TOKEN: &str = "refusal-modes-internal-token";
const CODE: &str = "zdr_durable_mutation_forbidden";

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

/// Install the capturing subscriber once for the process (the stack the binary runs, at `debug`).
fn capture() -> &'static Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let capture = Capture::default();
        quarry_edge::zdr_log::production_subscriber(
            capture.clone(),
            quarry_edge::zdr_log::harden(EnvFilter::new("debug")),
        )
        .init();
        capture
    })
}

fn logged() -> String {
    String::from_utf8_lossy(&capture().0.lock().unwrap()).into_owned()
}

/// The JSON log lines whose message is `message`, as their `fields` objects.
fn events(message: &str) -> Vec<Value> {
    logged()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|line| line.get("fields").cloned())
        .filter(|fields| fields["message"] == message)
        .collect()
}

fn marker() -> String {
    format!("zdr-probe-{}", uuid::Uuid::new_v4().simple())
}

/// Control Plane as the edge sees it for the schedule routes: create and every lifecycle call
/// answer 2xx, the list answers an empty page. wiremock records every request it receives.
async fn control_plane() -> MockServer {
    let control = MockServer::start().await;
    for (verb, route) in [
        ("POST", "/v1/schedules"),
        ("POST", "/v1/schedules/sched-1/pause"),
        ("POST", "/v1/schedules/sched-1/unpause"),
        ("POST", "/v1/schedules/sched-1/trigger"),
        ("POST", "/v1/schedules/sched-1/backfill"),
        ("DELETE", "/v1/schedules/sched-1"),
    ] {
        Mock::given(method(verb))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "sched-1" })))
            .mount(&control)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/v1/schedules"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [] })))
        .mount(&control)
        .await;
    control
}

async fn edge(control_url: String, enforce: bool, profiles: Arc<dyn ProfileStore>) -> Harness {
    std::env::set_var("QUARRY_EDGE_INTERNAL_TOKEN", INTERNAL_TOKEN);
    Harness::start_with(move |state| AppState {
        control_base_url: control_url,
        zdr_refuse_durable_resources: enforce,
        profiles,
        ..state
    })
    .await
}

fn empty_profiles() -> Arc<dyn ProfileStore> {
    Arc::new(InMemoryProfileStore::new())
}

/// One request per refused action: `(row, action, response)`. Each carries `marker` in its
/// URL so a leak into the log is visible.
async fn durable_writes(
    harness: &Harness,
    token: &str,
    marker: &str,
) -> Vec<(&'static str, &'static str, reqwest::Response)> {
    let url = format!("https://page.test/{marker}");
    let profile_id = ProfileKind::new().to_string();
    let backfill = json!({ "start_at": "2026-01-01T00:00:00Z", "end_at": "2026-01-02T00:00:00Z" });
    vec![
        (
            "schedules",
            "create",
            harness
                .post(token, "/v1/schedules", json!({ "preset": "hourly", "target_ref": url }))
                .await,
        ),
        (
            "schedules",
            "unpause",
            harness.post(token, "/v1/schedules/sched-1/unpause", json!({})).await,
        ),
        (
            "schedules",
            "trigger",
            harness.post(token, "/v1/schedules/sched-1/trigger", json!({})).await,
        ),
        (
            "schedules",
            "backfill",
            harness.post(token, "/v1/schedules/sched-1/backfill", backfill).await,
        ),
        (
            "profiles",
            "create",
            harness
                .post(token, "/v1/profiles/create", json!({ "name": marker, "scope": "user_private" }))
                .await,
        ),
        (
            "profiles",
            "save",
            harness
                .post(
                    token,
                    "/v1/profiles",
                    json!({ "snapshot": { "cookies": [], "local_storage": [], "session_storage": [] } }),
                )
                .await,
        ),
        (
            "profiles",
            "update",
            harness
                .patch(token, &format!("/v1/profiles/{profile_id}"), json!({ "name": marker }))
                .await,
        ),
        (
            "change_history",
            "check",
            harness.post(token, "/v1/change/check", json!({ "url": url })).await,
        ),
        (
            "change_snapshot",
            "snapshot",
            harness
                .post(token, &format!("/v1/change/snapshot?url={}", urlencode(&url)), json!({}))
                .await,
        ),
        (
            "change_snapshot",
            "refresh",
            harness.post(token, "/v1/change/refresh", json!({ "url": url })).await,
        ),
    ]
}

fn urlencode(raw: &str) -> String {
    raw.replace(':', "%3A").replace('/', "%2F")
}

fn internal_record(zdr: Option<bool>, marker: &str) -> Value {
    let mut body = json!({
        "org_id": ZDR_ORG,
        "url": format!("https://page.test/{marker}"),
        "fresh_fingerprint": "refusal-modes-fingerprint",
        "run_id": "refusal-modes-run",
    });
    if let Some(zdr) = zdr {
        body["zdr"] = json!(zdr);
    }
    body
}

// ── observe mode (flag off, the default) ─────────────────────────────────────

/// Stage A. A standard organization leaves no `zdr.would_refuse` event. A ZDR organization is
/// served as before (never a refusal naming ZDR) and leaves one event per action, with the row,
/// the action and the org id, and nothing the caller sent.
///
/// One test on purpose: the capture is process-global and this is the only test here that runs
/// with the flag off, so the "none yet" check below cannot see another test's events.
#[tokio::test]
async fn observe_mode_serves_a_zdr_org_as_before_and_logs_one_content_free_event_per_action() {
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), false, empty_profiles()).await;
    let probe = marker();

    // A standard organization: served, and not observed.
    for (_row, action, response) in durable_writes(&harness, &user_in_standard_org(), &probe).await
    {
        assert_ne!(
            response.status().as_u16(),
            403,
            "{action}: a standard organization is not refused"
        );
    }
    let declared_false = harness
        .post(
            INTERNAL_TOKEN,
            "/v1/internal/change/record",
            internal_record(Some(false), &probe),
        )
        .await;
    assert_eq!(
        declared_false.status().as_u16(),
        501,
        "no baseline store in this harness"
    );
    assert!(
        events("zdr.would_refuse").is_empty(),
        "a standard organization must not be observed: {:?}",
        events("zdr.would_refuse")
    );

    // A ZDR organization: served as before.
    let mut expected: BTreeSet<(String, String)> = BTreeSet::new();
    for (row, action, response) in durable_writes(&harness, &user_in_zdr_org(), &probe).await {
        let status = response.status().as_u16();
        let payload: Value = response.json().await.unwrap_or(Value::Null);
        assert!(
            payload["error"]["details"]["zdr_refusal"].is_null(),
            "{row}/{action}: observe mode must not refuse: {payload}"
        );
        assert!(
            !payload["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("ZDR"),
            "{row}/{action}: observe mode must not name a ZDR refusal ({status}): {payload}"
        );
        expected.insert((row.to_owned(), action.to_owned()));
    }
    let declared = harness
        .post(
            INTERNAL_TOKEN,
            "/v1/internal/change/record",
            internal_record(Some(true), &probe),
        )
        .await;
    assert_eq!(
        declared.status().as_u16(),
        501,
        "a declared ZDR record is served as before (no store: 501), not refused"
    );
    expected.insert(("change_record_internal".to_owned(), "record".to_owned()));

    // Wind-down reads and writes are never observed.
    for response in [
        harness.get(&user_in_zdr_org(), "/v1/schedules").await,
        harness.get(&user_in_zdr_org(), "/v1/profiles").await,
        harness
            .post(&user_in_zdr_org(), "/v1/schedules/sched-1/pause", json!({}))
            .await,
        harness
            .delete(&user_in_zdr_org(), "/v1/schedules/sched-1")
            .await,
    ] {
        assert!(response.status().is_success(), "{}", response.status());
    }

    let seen = events("zdr.would_refuse");
    let observed: BTreeSet<(String, String)> = seen
        .iter()
        .map(|fields| {
            (
                fields["row"].as_str().unwrap_or_default().to_owned(),
                fields["action"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        observed, expected,
        "one would_refuse per observed action, and none for the wind-down routes: {seen:?}"
    );
    assert_eq!(
        seen.len(),
        expected.len(),
        "exactly one event per request: {seen:?}"
    );
    for fields in &seen {
        assert_eq!(fields["org_id"], ZDR_ORG, "{fields}");
        let keys: BTreeSet<&str> = fields
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from(["action", "message", "org_id", "row"]),
            "the event carries the row, the action and the org id and nothing else: {fields}"
        );
    }
    assert!(
        !logged().contains(&probe),
        "an observed request put its URL or name in the log:\n{}",
        logged()
            .lines()
            .filter(|l| l.contains(&probe))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ── enforce mode (flag on) ───────────────────────────────────────────────────

/// A ZDR organization's schedule create, monitor check, profile writes, snapshot and refresh are
/// refused 403 with the catalogue code, and not one of them reaches Control Plane.
#[tokio::test]
async fn enforced_zdr_org_is_refused_every_durable_write_and_control_plane_sees_nothing() {
    let control = control_plane().await;
    let harness = edge(control.uri(), true, empty_profiles()).await;
    let probe = marker();

    for (row, action, response) in durable_writes(&harness, &user_in_zdr_org(), &probe).await {
        assert_eq!(response.status().as_u16(), 403, "{row}/{action}");
        let payload: Value = response.json().await.unwrap();
        assert_eq!(
            payload["error"]["details"]["zdr_refusal"]["code"], CODE,
            "{row}/{action}: {payload}"
        );
        assert_eq!(
            payload["error"]["details"]["zdr_refusal"]["action"], action,
            "{row}/{action}: {payload}"
        );
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("ZDR"),
            "{row}/{action}: the refusal says why: {payload}"
        );
        assert!(
            !payload.to_string().contains(&probe),
            "{row}/{action}: the refusal echoes caller data: {payload}"
        );
    }
    assert!(
        control.received_requests().await.unwrap().is_empty(),
        "a refused request never reaches Control Plane"
    );
}

/// The orchestrator's internal change record: a body that declares `zdr: true` is refused before
/// the baseline store is touched; one that does not declare it is served as before (501 here: this
/// harness wires no baseline store). The shared token is still checked first.
#[tokio::test]
async fn enforced_internal_change_record_refuses_a_declared_zdr_body_only() {
    let control = control_plane().await;
    let harness = edge(control.uri(), true, empty_profiles()).await;
    let probe = marker();

    let wrong_token = harness
        .post(
            "not-the-internal-token",
            "/v1/internal/change/record",
            internal_record(Some(true), &probe),
        )
        .await;
    assert_eq!(
        wrong_token.status().as_u16(),
        401,
        "the token is checked before the posture"
    );

    let declared = harness
        .post(
            INTERNAL_TOKEN,
            "/v1/internal/change/record",
            internal_record(Some(true), &probe),
        )
        .await;
    assert_eq!(declared.status().as_u16(), 403);
    let declared: Value = declared.json().await.unwrap();
    assert_eq!(
        declared["error"]["details"]["zdr_refusal"]["code"], CODE,
        "{declared}"
    );
    assert_eq!(
        declared["error"]["details"]["zdr_refusal"]["action"], "record",
        "{declared}"
    );

    for body in [
        internal_record(Some(false), &probe),
        internal_record(None, &probe),
    ] {
        let served = harness
            .post(INTERNAL_TOKEN, "/v1/internal/change/record", body)
            .await;
        assert_eq!(
            served.status().as_u16(),
            501,
            "an undeclared run is not refused"
        );
    }
}

/// A ZDR organization can still wind down: list schedules, pause and delete a schedule, list,
/// read and delete the profiles it already holds, and read the change history.
#[tokio::test]
async fn enforced_zdr_org_can_still_wind_down() {
    let control = control_plane().await;
    let profiles = Arc::new(InMemoryProfileStore::new());
    let held: ProfileKind = quarry_core::ids::Id::new();
    profiles
        .save(ZDR_ORG, &held, &SessionSnapshot::default())
        .await
        .expect("seed a profile the organization already holds");
    let harness = edge(control.uri(), true, profiles).await;
    let zdr = user_in_zdr_org();

    let listed = harness.get(&zdr, "/v1/schedules").await;
    assert_eq!(listed.status().as_u16(), 200, "schedule list");
    let paused = harness
        .post(&zdr, "/v1/schedules/sched-1/pause", json!({}))
        .await;
    assert_eq!(paused.status().as_u16(), 200, "schedule pause");
    let deleted = harness.delete(&zdr, "/v1/schedules/sched-1").await;
    assert_eq!(deleted.status().as_u16(), 204, "schedule delete");

    let profile_list = harness.get(&zdr, "/v1/profiles").await;
    assert_eq!(profile_list.status().as_u16(), 200, "profile list");
    let profile_list: Value = profile_list.json().await.unwrap();
    let ids: Vec<&str> = profile_list["data"]["profiles"]
        .as_array()
        .expect("a profiles array")
        .iter()
        .filter_map(|profile| profile["profile_id"].as_str())
        .collect();
    assert_eq!(
        ids,
        vec![held.to_string().as_str()],
        "the held profile is listed"
    );
    let read = harness.get(&zdr, &format!("/v1/profiles/{held}")).await;
    assert_eq!(read.status().as_u16(), 200, "profile read");
    let erased = harness.delete(&zdr, &format!("/v1/profiles/{held}")).await;
    assert_eq!(
        erased.status().as_u16(),
        204,
        "profile delete erases what the org holds"
    );
    let after = harness.get(&zdr, "/v1/profiles").await;
    let after: Value = after.json().await.unwrap();
    assert!(
        after["data"]["profiles"].as_array().unwrap().is_empty(),
        "the profile is gone: {after}"
    );

    let history = harness
        .get(&zdr, "/v1/change/history?url=https%3A%2F%2Fpage.test%2F")
        .await;
    assert_eq!(
        history.status().as_u16(),
        501,
        "history is a read: not refused (no store here: 501)"
    );
    let history: Value = history.json().await.unwrap_or(Value::Null);
    assert!(
        history["error"]["details"]["zdr_refusal"].is_null(),
        "{history}"
    );
}

/// A standard organization is unchanged with the flag on: it creates a schedule and a profile
/// (200) and gets the same 501 for the change routes that it got before.
#[tokio::test]
async fn enforced_standard_org_is_unchanged() {
    let control = control_plane().await;
    let harness = edge(control.uri(), true, empty_profiles()).await;
    let standard = user_in_standard_org();
    let probe = marker();

    for (row, action, response) in durable_writes(&harness, &standard, &probe).await {
        let status = response.status().as_u16();
        let payload: Value = response.json().await.unwrap_or(Value::Null);
        assert!(
            payload["error"]["details"]["zdr_refusal"].is_null(),
            "{row}/{action}: a standard organization is never refused for ZDR: {payload}"
        );
        match (row, action) {
            ("schedules", "create") | ("profiles", "create" | "save") => {
                assert_eq!(status, 200, "{row}/{action}: {payload}");
            }
            ("change_history" | "change_snapshot", _) => {
                assert_eq!(
                    status, 501,
                    "{row}/{action}: no store in this harness: {payload}"
                );
            }
            _ => {}
        }
    }
    let created = control.received_requests().await.unwrap();
    assert!(
        created
            .iter()
            .any(|r| r.method.as_str() == "POST" && r.url.path() == "/v1/schedules"),
        "the standard organization's schedule reached Control Plane"
    );
}
