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

use quarry_browser::session::{
    InMemoryProfileStore, ProfileMetadata, ProfileScope, ProfileStore, SessionSnapshot,
};
use quarry_core::ids::kinds::ProfileKind;
use quarry_edge::state::AppState;

use zdr_harness::{golden_token, user_in_standard_org, user_in_zdr_org, Harness};

const ZDR_ORG: &str = "org-zdr-golden";
const INTERNAL_TOKEN: &str = "refusal-modes-internal-token";
const CODE: &str = "zdr_durable_mutation_forbidden";
const SEEDED_NAME: &str = "seeded-profile-name";

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

/// Every test here shares the process-global capture, and the enforce tests all emit
/// `zdr.refused` for the same organization, so each test takes this lock for its whole body (the
/// observe test too, so its "no `zdr.would_refuse` yet" check cannot depend on a test that
/// later learns to emit one) and reads only the events appended after it started.
static CAPTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// How many `zdr.refused` events the process log holds now; pass it to [`refused_since`].
fn refused_mark() -> usize {
    events("zdr.refused").len()
}

/// The `zdr.refused` events appended since `mark`.
fn refused_since(mark: usize) -> Vec<Value> {
    events("zdr.refused").split_off(mark)
}

/// One `zdr.refused` event per refused request, naming exactly these rows and actions, the
/// organization id, and nothing else (no URL, host, title or name).
fn assert_refused_events(seen: &[Value], expected: &BTreeSet<(String, String)>) {
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
        &observed, expected,
        "one zdr.refused per refused durable write: {seen:?}"
    );
    assert_eq!(
        seen.len(),
        expected.len(),
        "exactly one event per refused request: {seen:?}"
    );
    for fields in seen {
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
}

/// The profiles the organization behind `bearer` holds, as (id, name) (a wind-down read).
async fn held_profiles(harness: &Harness, bearer: &str) -> Vec<(String, Option<String>)> {
    let listed = harness.get(bearer, "/v1/profiles").await;
    assert_eq!(listed.status().as_u16(), 200, "profile list");
    let listed: Value = listed.json().await.unwrap();
    listed["data"]["profiles"]
        .as_array()
        .expect("a profiles array")
        .iter()
        .filter_map(|profile| {
            profile["profile_id"].as_str().map(|id| {
                (
                    id.to_owned(),
                    profile["name"].as_str().map(str::to_owned),
                )
            })
        })
        .collect()
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
/// URL so a leak into the log is visible. `profile_id` is the profile the update renames: pass a
/// profile the organization holds to see whether a refused update changed it.
async fn durable_writes(
    harness: &Harness,
    token: &str,
    marker: &str,
    profile_id: &str,
) -> Vec<(&'static str, &'static str, reqwest::Response)> {
    let url = format!("https://page.test/{marker}");
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
/// The only test here that runs with the flag off; it holds the capture lock like the others, so
/// the "none yet" check below sees no other test's events.
#[tokio::test]
async fn observe_mode_serves_a_zdr_org_as_before_and_logs_one_content_free_event_per_action() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let profiles = Arc::new(InMemoryProfileStore::new());
    let held: ProfileKind = quarry_core::ids::Id::new();
    profiles
        .save(ZDR_ORG, &held, &SessionSnapshot::default())
        .await
        .expect("seed a profile the organization already holds");
    let harness = edge(control.uri(), false, profiles.clone()).await;
    let probe = marker();

    // A standard organization: served, and not observed.
    let unheld = ProfileKind::new().to_string();
    for (_row, action, response) in
        durable_writes(&harness, &user_in_standard_org(), &probe, &unheld).await
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
    for (row, action, response) in
        durable_writes(&harness, &user_in_zdr_org(), &probe, &held.to_string()).await
    {
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
        harness
            .post(
                &user_in_zdr_org(),
                &format!("/v1/profiles/{held}/restore_probe"),
                json!({ "url": format!("https://page.test/{probe}") }),
            )
            .await,
    ] {
        assert!(response.status().is_success(), "{}", response.status());
    }
    // `GET /v1/change/latest` is a read: 501 here (no baseline store), never observed.
    let latest = harness
        .get(
            &user_in_zdr_org(),
            &format!(
                "/v1/change/latest?url={}",
                urlencode(&format!("https://page.test/{probe}"))
            ),
        )
        .await;
    assert_eq!(latest.status().as_u16(), 501, "change latest is served as before");

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
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    // The organization already holds a named profile: a refused update must leave its name alone
    // (a profile id the store never held could not show a mutate-then-403 update).
    let profiles = Arc::new(InMemoryProfileStore::new());
    let held: ProfileKind = quarry_core::ids::Id::new();
    profiles
        .save(ZDR_ORG, &held, &SessionSnapshot::default())
        .await
        .expect("seed a profile the organization already holds");
    profiles
        .save_metadata(
            ZDR_ORG,
            &held,
            &ProfileMetadata {
                name: Some(SEEDED_NAME.to_owned()),
                scope: ProfileScope::UserPrivate,
            },
        )
        .await
        .expect("name the seeded profile");
    let harness = edge(control.uri(), true, profiles.clone()).await;
    let probe = marker();
    let mark = refused_mark();
    let mut expected: BTreeSet<(String, String)> = BTreeSet::new();

    for (row, action, response) in
        durable_writes(&harness, &user_in_zdr_org(), &probe, &held.to_string()).await
    {
        expected.insert((row.to_owned(), action.to_owned()));
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
    assert_refused_events(&refused_since(mark), &expected);
    assert!(
        !logged().contains(&probe),
        "a refused request put its URL or name in the log:\n{}",
        logged()
            .lines()
            .filter(|l| l.contains(&probe))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(
        held_profiles(&harness, &user_in_zdr_org()).await,
        vec![(held.to_string(), Some(SEEDED_NAME.to_owned()))],
        "a refused profile create, save or update stored nothing and renamed nothing"
    );
    let stored = profiles
        .load_metadata(ZDR_ORG, &held)
        .await
        .expect("load the seeded profile's metadata")
        .expect("the seeded profile keeps its metadata");
    assert_eq!(
        stored,
        ProfileMetadata {
            name: Some(SEEDED_NAME.to_owned()),
            scope: ProfileScope::UserPrivate,
        },
        "a refused update left the stored profile as it was"
    );
}

/// The orchestrator's internal change record: a body that declares `zdr: true` is refused before
/// the baseline store is touched; one that does not declare it is served as before (501 here: this
/// harness wires no baseline store). The shared token is still checked first.
#[tokio::test]
async fn enforced_internal_change_record_refuses_a_declared_zdr_body_only() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), true, empty_profiles()).await;
    let probe = marker();
    let mark = refused_mark();

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
    // Exactly the declared body was refused: the 401, the two served bodies left no event.
    assert_refused_events(
        &refused_since(mark),
        &BTreeSet::from([("change_record_internal".to_owned(), "record".to_owned())]),
    );
}

/// A ZDR organization can still wind down: list schedules, pause and delete a schedule, list,
/// read and delete the profiles it already holds, and read the change history.
#[tokio::test]
async fn enforced_zdr_org_can_still_wind_down() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let profiles = Arc::new(InMemoryProfileStore::new());
    let held: ProfileKind = quarry_core::ids::Id::new();
    profiles
        .save(ZDR_ORG, &held, &SessionSnapshot::default())
        .await
        .expect("seed a profile the organization already holds");
    let harness = edge(control.uri(), true, profiles).await;
    let zdr = user_in_zdr_org();
    let mark = refused_mark();

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
    let probed = harness
        .post(
            &zdr,
            &format!("/v1/profiles/{held}/restore_probe"),
            json!({ "url": "https://page.test/restore" }),
        )
        .await;
    assert_eq!(probed.status().as_u16(), 200, "profile restore_probe");
    let probed: Value = probed.json().await.unwrap();
    assert!(
        probed["error"]["details"]["zdr_refusal"].is_null(),
        "{probed}"
    );
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
    let latest = harness
        .get(&zdr, "/v1/change/latest?url=https%3A%2F%2Fpage.test%2F")
        .await;
    assert_eq!(
        latest.status().as_u16(),
        501,
        "latest is a read: not refused (no store here: 501)"
    );
    let latest: Value = latest.json().await.unwrap_or(Value::Null);
    assert!(
        latest["error"]["details"]["zdr_refusal"].is_null(),
        "{latest}"
    );
    assert!(
        refused_since(mark).is_empty(),
        "no wind-down call left a zdr.refused event: {:?}",
        refused_since(mark)
    );
}

/// A standard organization is unchanged with the flag on: it creates a schedule and a profile
/// (200) and gets the same 501 for the change routes that it got before.
#[tokio::test]
async fn enforced_standard_org_is_unchanged() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), true, empty_profiles()).await;
    let standard = user_in_standard_org();
    let probe = marker();
    let mark = refused_mark();

    let unheld = ProfileKind::new().to_string();
    for (row, action, response) in durable_writes(&harness, &standard, &probe, &unheld).await {
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
    assert!(
        refused_since(mark).is_empty(),
        "a standard organization leaves no zdr.refused event: {:?}",
        refused_since(mark)
    );
    assert!(
        !held_profiles(&harness, &standard).await.is_empty(),
        "the standard organization's profile writes are stored (so the empty store above means refusal)"
    );
}

// -- the refusals that were enforced before WS4.H (V4) -------------------------------------
//
// Durable crawl and batch (row `durable_jobs`) and a persistent profile at run start (row
// `agent_persistent_profile`) are refused for a ZDR caller whatever the flag says: they are
// not behind `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES`. Their status, error code and
// message are unchanged; what V4 adds is `details.zdr_refusal { code, action }`, the same
// object the flag-gated refusals carry, so a client keys on one shape. (The resume refusal of a
// ZDR checkpoint needs a stored checkpoint and has its own test below.)

/// Both modes, `/v1/crawl` and `/v1/batch`: 403 `FORBIDDEN`, the unchanged ZDR message, and
/// `details.zdr_refusal` with the action `crawl` or `batch`. A body that asks for ZDR is refused
/// the same way, nothing reaches Control Plane, and a standard organization gets no such
/// details (its request is accepted).
#[tokio::test]
async fn durable_crawl_and_batch_refusals_carry_zdr_refusal_details_in_both_modes() {
    for enforce in [false, true] {
        let control = control_plane().await;
        let harness = edge(control.uri(), enforce, empty_profiles()).await;
        let probe = marker();

        for (route, body, action) in [
            (
                "/v1/crawl",
                json!({ "url": format!("https://page.test/{probe}"), "max_pages": 1 }),
                "crawl",
            ),
            (
                "/v1/batch",
                json!({ "urls": [format!("https://page.test/{probe}")] }),
                "batch",
            ),
        ] {
            for (who, bearer, flag) in [
                ("a ZDR organization's user", user_in_zdr_org(), None),
                ("a standard user asking for ZDR", user_in_standard_org(), Some(true)),
            ] {
                let mut request = body.clone();
                if let Some(flag) = flag {
                    request["zdr"] = json!(flag);
                }
                let refused = harness.post(&bearer, route, request).await;
                assert_eq!(
                    refused.status().as_u16(),
                    403,
                    "{route} (flag {enforce}), {who}"
                );
                let payload: Value = refused.json().await.unwrap();
                let context = format!("{route} (flag {enforce}), {who}: {payload}");
                assert_eq!(payload["error"]["code"], "FORBIDDEN", "{context}");
                assert_eq!(
                    payload["error"]["message"],
                    format!(
                        "ZDR active: durable {action} jobs are not available under zero data \
                         retention (the job record and per-page results are retained)"
                    ),
                    "the message is unchanged: {context}"
                );
                assert_eq!(
                    payload["error"]["details"]["zdr_refusal"]["code"], CODE,
                    "{context}"
                );
                assert_eq!(
                    payload["error"]["details"]["zdr_refusal"]["action"], action,
                    "{context}"
                );
                assert!(
                    payload["error"]["details"]["zdr_refusal"]
                        .as_object()
                        .is_some_and(|object| object.len() == 2),
                    "code and action only, no feature key: {context}"
                );
                assert!(!payload.to_string().contains(&probe), "{context}");
            }
        }
        assert!(
            control.received_requests().await.unwrap().is_empty(),
            "a refused durable job never reaches Control Plane (flag {enforce})"
        );

        // A standard organization is not refused and its answer has no zdr_refusal.
        let accepted = harness
            .post(
                &user_in_standard_org(),
                "/v1/crawl",
                json!({ "url": "https://page.test/", "max_pages": 1 }),
            )
            .await;
        assert_ne!(accepted.status().as_u16(), 403, "flag {enforce}");
        let accepted: Value = accepted.json().await.unwrap_or(Value::Null);
        assert!(
            accepted["error"]["details"]["zdr_refusal"].is_null(),
            "{accepted}"
        );
    }
}

/// Both modes, `POST /v1/agent/runs` with a persistent profile: 400 `BAD_REQUEST` (the status
/// and code this refusal has always had), the unchanged message, and `details.zdr_refusal`
/// with the action `profile`, for a ZDR organization's user and for a service whose body says
/// `zdr: true`. A standard organization's request carries no such details.
#[cfg(feature = "browser-agent")]
#[tokio::test]
async fn the_persistent_profile_refusal_at_run_start_carries_zdr_refusal_details_in_both_modes() {
    for enforce in [false, true] {
        let control = control_plane().await;
        let harness = edge(control.uri(), enforce, empty_profiles()).await;

        for (who, bearer, body) in [
            (
                "a ZDR organization's user",
                user_in_zdr_org(),
                json!({ "zdr": false, "persist_profile": true }),
            ),
            (
                "a service with a ZDR body",
                golden_token("service_execution_core"),
                json!({ "zdr": true, "persist_profile": true }),
            ),
        ] {
            let refused = harness.post(&bearer, "/v1/agent/runs", body).await;
            assert_eq!(refused.status().as_u16(), 400, "{who} (flag {enforce})");
            let payload: Value = refused.json().await.unwrap();
            let context = format!("{who} (flag {enforce}): {payload}");
            assert_eq!(payload["error"]["code"], "BAD_REQUEST", "{context}");
            // The full text is pinned: it is the message this refusal has always carried.
            assert_eq!(
                payload["error"]["message"],
                "zdr_persistent_profile_forbidden: a ZDR run cannot request a persistent profile or profile_id",
                "{context}"
            );
            assert_eq!(
                payload["error"]["details"]["zdr_refusal"]["code"], CODE,
                "{context}"
            );
            assert_eq!(
                payload["error"]["details"]["zdr_refusal"]["action"], "profile",
                "{context}"
            );
            // The token this refusal has always carried in its message is also a field.
            assert_eq!(
                payload["error"]["details"]["legacy_code"], "zdr_persistent_profile_forbidden",
                "{context}"
            );
        }

        let standard = harness
            .post(
                &user_in_standard_org(),
                "/v1/agent/runs",
                json!({ "zdr": false, "persist_profile": true }),
            )
            .await;
        let standard: Value = standard.json().await.unwrap_or(Value::Null);
        assert!(
            standard["error"]["details"]["zdr_refusal"].is_null(),
            "a standard organization gets no ZDR refusal details: {standard}"
        );
    }
}

/// Both modes, `POST /v1/agent/runs` with `resume_run_id` naming a ZDR checkpoint: 403 (with the
/// `BAD_REQUEST` error code this refusal has always had), the unchanged message, and
/// `details.zdr_refusal` with the action `resume`. A real edge never writes such a checkpoint
/// (a ZDR run writes none), so the test stores one for the golden ZDR user; a checkpoint that is
/// not ZDR is not refused for ZDR.
#[cfg(feature = "browser-agent")]
#[tokio::test]
async fn resuming_a_zdr_checkpoint_is_refused_with_zdr_refusal_details_in_both_modes() {
    use quarry_core::ids::kinds::RunKind;
    use quarry_runtime::step_receipts::StepReceiptStore;
    use quarry_runtime::AgentRunCheckpoint;

    let checkpoint = |run: &str, zdr: &str| -> AgentRunCheckpoint {
        serde_json::from_value(json!({
            "org_id": ZDR_ORG,
            "actor_id": "user-golden",
            "run_id": run,
            "profile_id": ProfileKind::new().to_string(),
            "step": 1,
            "current_url": "https://example.com/",
            "constraints": { "max_steps": 5, "allowed_domains": ["example.com"], "max_runtime_s": 60 },
            "zdr": zdr,
            "status": "active",
        }))
        .expect("a checkpoint")
    };

    for enforce in [false, true] {
        let zdr_run = RunKind::new().to_string();
        let plain_run = RunKind::new().to_string();
        let receipts = Arc::new(quarry_runtime::InMemoryStepReceiptStore::new());
        for (run, zdr) in [(&zdr_run, "on"), (&plain_run, "off")] {
            receipts
                .save_run_checkpoint(checkpoint(run, zdr))
                .await
                .unwrap();
        }
        std::env::set_var("QUARRY_EDGE_INTERNAL_TOKEN", INTERNAL_TOKEN);
        let control = control_plane().await;
        let control_url = control.uri();
        let harness = Harness::start_with(move |state| AppState {
            control_base_url: control_url,
            zdr_refuse_durable_resources: enforce,
            receipts,
            ..state
        })
        .await;

        let refused = harness
            .post(
                &user_in_zdr_org(),
                "/v1/agent/runs",
                json!({ "resume_run_id": zdr_run }),
            )
            .await;
        assert_eq!(refused.status().as_u16(), 403, "flag {enforce}");
        let payload: Value = refused.json().await.unwrap();
        let context = format!("flag {enforce}: {payload}");
        assert_eq!(payload["error"]["code"], "BAD_REQUEST", "{context}");
        assert_eq!(
            payload["error"]["message"], "ZDR runs cannot be resumed from durable state",
            "{context}"
        );
        assert_eq!(
            payload["error"]["details"],
            json!({ "zdr_refusal": { "code": CODE, "action": "resume" } }),
            "{context}"
        );

        // A checkpoint that is not ZDR is not refused for ZDR (the user's own org floor still
        // refuses the persistent profile it needs, which is the `profile` action, not `resume`).
        let other = harness
            .post(
                &user_in_zdr_org(),
                "/v1/agent/runs",
                json!({ "resume_run_id": plain_run }),
            )
            .await;
        let other: Value = other.json().await.unwrap_or(Value::Null);
        assert_ne!(
            other["error"]["details"]["zdr_refusal"]["action"], "resume",
            "{other}"
        );
    }
}
