//! WS4.H, the posture signal: the edge stamps the caller's Zero Data Retention posture on the
//! change-monitor schedule it forwards to Control Plane (`POST /v1/schedules` with a `preset`, and
//! `POST /v1/sources` with `monitor: true`), and the orchestrator later declares that stamp on
//! `/v1/internal/change/record`, where the edge refuses (flag on) or logs would-refuse (flag off).
//!
//! * the stamp is `Claims::effective_zdr` of the body's declared `zdr`, the one rule every ZDR
//!   decision follows: a body can raise the posture and never lower the floor of a user token.
//!   A service principal cannot reach these routes at all (`require_service_route_scope` fails
//!   unknown service routes closed), so there is no service-stamped schedule to be wrong;
//! * it is computed once, so the refusal and the stamp cannot disagree;
//! * the record route trusts the stamp it is given, not the org's posture today. That is the
//!   documented "org turned ZDR on later" gap, pinned here so it cannot change unnoticed
//!   (RUNBOOKS, "ZDR posture of a scheduled change monitor").
//!
//! Own binary: the capturing subscriber is process-global, and `zdr_refusal_modes.rs` counts
//! every `zdr.would_refuse` its process emits.

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

use quarry_edge::state::AppState;

use zdr_harness::{golden_token, user_in_standard_org, user_in_zdr_org, Harness};

const ZDR_ORG: &str = "org-zdr-golden";
const INTERNAL_TOKEN: &str = "posture-stamp-internal-token";
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

/// The `fields` objects of the log events whose message is `message`.
fn events(message: &str) -> Vec<Value> {
    String::from_utf8_lossy(&capture().0.lock().unwrap())
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|line| line.get("fields").cloned())
        .filter(|fields| fields["message"] == message)
        .collect()
}

/// The tests share the process-global capture and read only the events appended after they start.
static CAPTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Control Plane as the edge sees it: schedule and source creates answer 200.
async fn control_plane() -> MockServer {
    let control = MockServer::start().await;
    for route in ["/v1/schedules", "/v1/sources"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "stamp-1" })))
            .mount(&control)
            .await;
    }
    control
}

async fn edge(control_url: String, enforce: bool) -> Harness {
    std::env::set_var("QUARRY_EDGE_INTERNAL_TOKEN", INTERNAL_TOKEN);
    Harness::start_with(move |state| AppState {
        control_base_url: control_url,
        zdr_refuse_durable_resources: enforce,
        ..state
    })
    .await
}

/// The JSON bodies Control Plane received on `POST route`, oldest first.
async fn control_bodies(control: &MockServer, route: &str) -> Vec<Value> {
    control
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "POST" && r.url.path() == route)
        .map(|r| serde_json::from_slice(&r.body).expect("control receives JSON"))
        .collect()
}

fn monitor_schedule(url: &str) -> Value {
    json!({ "preset": "daily", "target_ref": url })
}

fn monitor_source(url: &str) -> Value {
    json!({ "name": "watch", "url": url, "kind": "scrape", "monitor": true, "preset": "daily" })
}

fn internal_record(zdr: Option<bool>) -> Value {
    let mut body = json!({
        "org_id": ZDR_ORG,
        "url": "https://page.test/stamp",
        "fresh_fingerprint": "posture-stamp-fingerprint",
        "run_id": "posture-stamp-run",
    });
    if let Some(zdr) = zdr {
        body["zdr"] = json!(zdr);
    }
    body
}

// -- schedules ---------------------------------------------------------------------------

/// Flag off (stage A): a ZDR organization's schedule is created exactly as before and now
/// carries `zdr: true`; a standard organization's carries an explicit `zdr: false`.
#[tokio::test]
async fn observe_mode_stamps_the_zdr_posture_on_the_forwarded_schedule() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), false).await;
    let seen_before = events("zdr.would_refuse").len();

    let zdr = harness
        .post(&user_in_zdr_org(), "/v1/schedules", monitor_schedule("https://page.test/a"))
        .await;
    assert_eq!(zdr.status().as_u16(), 200, "observe mode serves a ZDR org as before");
    let standard = harness
        .post(&user_in_standard_org(), "/v1/schedules", monitor_schedule("https://page.test/b"))
        .await;
    assert_eq!(standard.status().as_u16(), 200);

    let bodies = control_bodies(&control, "/v1/schedules").await;
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    let by_target = |target: &str| {
        bodies
            .iter()
            .find(|b| b["target_ref"] == target)
            .unwrap_or_else(|| panic!("no forward for {target}: {bodies:?}"))
            .clone()
    };
    let zdr_body = by_target("https://page.test/a");
    assert_eq!(zdr_body["zdr"], json!(true), "the ZDR org's stamp: {zdr_body}");
    assert_eq!(zdr_body["org_id"], ZDR_ORG);
    let standard_body = by_target("https://page.test/b");
    assert_eq!(
        standard_body["zdr"],
        json!(false),
        "a standard org is stamped false, not left unstamped: {standard_body}"
    );
    assert_eq!(
        events("zdr.would_refuse").len() - seen_before,
        1,
        "exactly the ZDR org's create was observed"
    );
}

/// A body can raise the stamp and never lower the token's floor: a `zdr: false` in a ZDR
/// organization's body does not lower it, and a `zdr: true` in a standard organization's body
/// raises it.
#[tokio::test]
async fn a_client_body_can_raise_the_stamp_but_never_lower_it() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), false).await;

    let mut lowered = monitor_schedule("https://page.test/lower");
    lowered["zdr"] = json!(false);
    let mut raised = monitor_schedule("https://page.test/raise");
    raised["zdr"] = json!(true);
    assert_eq!(
        harness.post(&user_in_zdr_org(), "/v1/schedules", lowered).await.status().as_u16(),
        200
    );
    assert_eq!(
        harness.post(&user_in_standard_org(), "/v1/schedules", raised).await.status().as_u16(),
        200
    );

    let bodies = control_bodies(&control, "/v1/schedules").await;
    let stamp = |target: &str| {
        bodies
            .iter()
            .find(|b| b["target_ref"] == target)
            .map(|b| b["zdr"].clone())
            .unwrap_or_else(|| panic!("no forward for {target}: {bodies:?}"))
    };
    assert_eq!(stamp("https://page.test/lower"), json!(true), "a body cannot lower the stamp");
    assert_eq!(stamp("https://page.test/raise"), json!(true), "a body raises the stamp");
}

/// Flag off, the declared posture of a caller whose token has no floor: a standard organization's
/// user that says `zdr: true` gets a schedule stamped true (and the would-be refusal is logged);
/// without the declaration it is stamped false.
#[tokio::test]
async fn observe_mode_a_declared_zdr_body_is_stamped_for_a_standard_user() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), false).await;
    let seen_before = events("zdr.would_refuse").len();

    for (who, bearer) in [("standard", user_in_standard_org())] {
        let mut declared = monitor_schedule(&format!("https://page.test/declared-{who}"));
        declared["zdr"] = json!(true);
        let plain = monitor_schedule(&format!("https://page.test/plain-{who}"));
        for body in [declared, plain] {
            let created = harness.post(&bearer, "/v1/schedules", body).await;
            assert_eq!(created.status().as_u16(), 200, "{who}: flag off serves it");
        }
    }

    let bodies = control_bodies(&control, "/v1/schedules").await;
    let stamp = |target: &str| {
        bodies
            .iter()
            .find(|b| b["target_ref"] == target)
            .map(|b| b["zdr"].clone())
            .unwrap_or_else(|| panic!("no forward for {target}: {bodies:?}"))
    };
    for who in ["standard"] {
        assert_eq!(
            stamp(&format!("https://page.test/declared-{who}")),
            json!(true),
            "{who}: a declared zdr is stamped"
        );
        assert_eq!(
            stamp(&format!("https://page.test/plain-{who}")),
            json!(false),
            "{who}: no declaration, no stamp"
        );
    }
    assert_eq!(
        events("zdr.would_refuse").len() - seen_before,
        1,
        "the declared create was observed, the plain one was not"
    );
}

/// Flag on: the same declared `zdr: true` is refused before Control Plane sees anything, for a
/// standard organization's user, on both the schedule and the monitored-source routes; the
/// undeclared create of the standard user is still served.
#[tokio::test]
async fn enforced_a_declared_zdr_body_is_refused_for_a_standard_user() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), true).await;

    for (who, bearer) in [("standard", user_in_standard_org())] {
        let mut schedule = monitor_schedule(&format!("https://page.test/refused-{who}"));
        schedule["zdr"] = json!(true);
        let mut source = monitor_source(&format!("https://page.test/refused-src-{who}"));
        source["zdr"] = json!(true);
        for (route, body) in [("/v1/schedules", schedule), ("/v1/sources", source)] {
            let refused = harness.post(&bearer, route, body).await;
            assert_eq!(refused.status().as_u16(), 403, "{who} {route}");
            let refused: Value = refused.json().await.unwrap();
            assert_eq!(
                refused["error"]["details"]["zdr_refusal"]["code"], CODE,
                "{who} {route}: {refused}"
            );
            assert_eq!(
                refused["error"]["details"]["zdr_refusal"]["action"], "create",
                "{who} {route}: {refused}"
            );
        }
    }
    assert!(control_bodies(&control, "/v1/schedules").await.is_empty());
    assert!(control_bodies(&control, "/v1/sources").await.is_empty());

    let served = harness
        .post(&user_in_standard_org(), "/v1/schedules", monitor_schedule("https://page.test/ok"))
        .await;
    assert_eq!(served.status().as_u16(), 200);
    assert_eq!(control_bodies(&control, "/v1/schedules").await[0]["zdr"], json!(false));
}

/// Flag on: the standard organization is unchanged (created, stamped false) and a ZDR one never
/// reaches Control Plane, so no `zdr: true` schedule can come from an enforcing edge.
#[tokio::test]
async fn enforced_schedule_create_is_stamped_false_for_standard_and_never_forwarded_for_zdr() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), true).await;

    let refused = harness
        .post(&user_in_zdr_org(), "/v1/schedules", monitor_schedule("https://page.test/z"))
        .await;
    assert_eq!(refused.status().as_u16(), 403);
    let refused: Value = refused.json().await.unwrap();
    assert_eq!(refused["error"]["details"]["zdr_refusal"]["code"], CODE, "{refused}");
    let created = harness
        .post(&user_in_standard_org(), "/v1/schedules", monitor_schedule("https://page.test/s"))
        .await;
    assert_eq!(created.status().as_u16(), 200);

    let bodies = control_bodies(&control, "/v1/schedules").await;
    assert_eq!(bodies.len(), 1, "only the standard org's create is forwarded: {bodies:?}");
    assert_eq!(bodies[0]["zdr"], json!(false));
}

/// The legacy non-preset create (cron or schedule_at, no change monitor) forwards the whole
/// `CreateScheduleRequest`. A client-declared `zdr` must not reach control on that body: it is
/// not a change monitor, nothing reads a stamp on it, and only the stamped monitor body carries
/// a posture. Pins `skip_serializing` on the field, in both flag modes.
#[tokio::test]
async fn a_legacy_cron_create_never_forwards_a_declared_zdr() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    for enforce in [false, true] {
        let control = control_plane().await;
        let harness = edge(control.uri(), enforce).await;
        let body = json!({ "name": "legacy", "kind": "scrape", "cron": "*/5 * * * *", "config": { "url": "https://page.test/legacy" }, "zdr": true });
        let resp = harness.post(&user_in_standard_org(), "/v1/schedules", body).await;
        // Enforced, the declaration makes it a ZDR create and the edge refuses it; observing,
        // it is created. Either way the forwarded body (if any) has no zdr key.
        assert_eq!(resp.status().as_u16(), if enforce { 403 } else { 200 }, "enforce={enforce}");
        let bodies = control_bodies(&control, "/v1/schedules").await;
        assert_eq!(bodies.len(), usize::from(!enforce), "enforce={enforce}: {bodies:?}");
        for forwarded in &bodies {
            assert_eq!(forwarded["cron"], "*/5 * * * *", "{forwarded}");
            assert!(
                forwarded.get("zdr").is_none(),
                "the legacy create must not forward a declared zdr: {forwarded}"
            );
        }
    }
}

// -- sources with monitor: true ------------------------------------------------------------

/// Flag off: a ZDR organization's monitored source is registered as before and its forwarded
/// body carries the stamp; a standard organization's carries `false`; a plain source (no
/// monitor) carries no stamp at all, whatever the client sent; a client-declared `zdr: true` raises a
/// standard organization's monitor to true and no declaration lowers a ZDR organization's.
#[tokio::test]
async fn observe_mode_stamps_a_source_monitor() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), false).await;
    let seen_before = events("zdr.would_refuse").len();

    let mut lowered = monitor_source("https://page.test/src-zdr");
    lowered["zdr"] = json!(false);
    assert_eq!(
        harness.post(&user_in_zdr_org(), "/v1/sources", lowered).await.status().as_u16(),
        200
    );
    assert_eq!(
        harness
            .post(&user_in_standard_org(), "/v1/sources", monitor_source("https://page.test/src-std"))
            .await
            .status()
            .as_u16(),
        200
    );
    let mut declared = monitor_source("https://page.test/src-declared");
    declared["zdr"] = json!(true);
    assert_eq!(
        harness.post(&user_in_standard_org(), "/v1/sources", declared).await.status().as_u16(),
        200
    );
    let mut plain = json!({ "name": "plain", "url": "https://page.test/src-plain", "kind": "scrape" });
    plain["zdr"] = json!(true);
    assert_eq!(
        harness.post(&user_in_zdr_org(), "/v1/sources", plain).await.status().as_u16(),
        200,
        "a plain source is a record only and is served"
    );

    let bodies = control_bodies(&control, "/v1/sources").await;
    assert_eq!(bodies.len(), 4, "{bodies:?}");
    let body = |url: &str| {
        bodies
            .iter()
            .find(|b| b["url"] == url)
            .unwrap_or_else(|| panic!("no forward for {url}: {bodies:?}"))
            .clone()
    };
    assert_eq!(body("https://page.test/src-zdr")["zdr"], json!(true), "a body cannot lower it");
    assert_eq!(body("https://page.test/src-std")["zdr"], json!(false));
    assert_eq!(body("https://page.test/src-declared")["zdr"], json!(true), "a body raises it");
    assert!(
        body("https://page.test/src-plain").get("zdr").is_none(),
        "a plain source carries no stamp, whatever the client sent: {}",
        body("https://page.test/src-plain")
    );
    assert_eq!(
        events("zdr.would_refuse").len() - seen_before,
        2,
        "the ZDR org's and the declared monitored source were observed (a plain source is not a schedule)"
    );
}

/// Flag on: a ZDR organization's monitored source is refused before Control Plane sees it, a plain
/// source of the same organization is served, and a standard organization's monitor is registered.
#[tokio::test]
async fn enforced_source_monitor_is_refused_but_a_plain_source_is_not() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), true).await;

    let refused = harness
        .post(&user_in_zdr_org(), "/v1/sources", monitor_source("https://page.test/m"))
        .await;
    assert_eq!(refused.status().as_u16(), 403);
    let refused: Value = refused.json().await.unwrap();
    assert_eq!(refused["error"]["details"]["zdr_refusal"]["code"], CODE, "{refused}");
    assert_eq!(refused["error"]["details"]["zdr_refusal"]["action"], "create", "{refused}");
    assert!(!refused.to_string().contains("https://page.test/m"), "no echo: {refused}");
    assert!(
        control_bodies(&control, "/v1/sources").await.is_empty(),
        "a refused monitor never reaches Control Plane"
    );

    let plain = harness
        .post(
            &user_in_zdr_org(),
            "/v1/sources",
            json!({ "name": "plain", "url": "https://page.test/p", "kind": "scrape" }),
        )
        .await;
    assert_eq!(plain.status().as_u16(), 200, "a plain source is not a durable schedule");
    let standard = harness
        .post(&user_in_standard_org(), "/v1/sources", monitor_source("https://page.test/s"))
        .await;
    assert_eq!(standard.status().as_u16(), 200);
    let bodies = control_bodies(&control, "/v1/sources").await;
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    let monitored = bodies.iter().find(|b| b["monitor"] == json!(true)).expect("the standard monitor");
    assert_eq!(monitored["zdr"], json!(false));
}

// -- the record route ----------------------------------------------------------------------

/// Flag off: a record that declares `zdr: true` is served as before (501: this harness has no
/// baseline store, which is past the refusal check, so the flag-off path did not refuse) and
/// leaves exactly one content-free `zdr.would_refuse` event.
#[tokio::test]
async fn observe_internal_record_with_declared_zdr_logs_would_refuse_and_is_not_403() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), false).await;
    let before = events("zdr.would_refuse").len();

    let declared = harness
        .post(INTERNAL_TOKEN, "/v1/internal/change/record", internal_record(Some(true)))
        .await;
    assert_eq!(declared.status().as_u16(), 501, "served as before, not refused");
    let seen = events("zdr.would_refuse").split_off(before);
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0]["row"], "change_record_internal");
    assert_eq!(seen[0]["action"], "record");
    assert_eq!(seen[0]["org_id"], ZDR_ORG);
    assert!(!seen[0].to_string().contains("page.test"), "no URL in the event: {}", seen[0]);

    let undeclared = harness
        .post(INTERNAL_TOKEN, "/v1/internal/change/record", internal_record(None))
        .await;
    assert_eq!(undeclared.status().as_u16(), 501);
    assert_eq!(
        events("zdr.would_refuse").len() - before,
        1,
        "an unstamped (legacy) schedule's record leaves no event"
    );
}

/// KNOWN GAP, NOT DESIRED BEHAVIOUR: the record route trusts the stamp it is given, not the
/// organization's posture today. `org-zdr-golden` is a ZDR organization right now, and a record
/// that declares `zdr: false` (a schedule created before the org turned ZDR on) or declares
/// nothing (a legacy row) is served with the flag on. The operator remedy is
/// `UPDATE schedules SET zdr = true WHERE org_id = ...` in quarry-control. Closing the gap (a
/// fire-time posture lookup, plan X-9) must flip this test in the same commit.
#[tokio::test]
async fn record_trusts_the_stamp_not_the_current_posture() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let control = control_plane().await;
    let harness = edge(control.uri(), true).await;
    let before = events("zdr.refused").len();

    for (what, body) in [
        ("stamped standard", internal_record(Some(false))),
        ("unstamped legacy", internal_record(None)),
    ] {
        let served = harness
            .post(INTERNAL_TOKEN, "/v1/internal/change/record", body)
            .await;
        assert_eq!(
            served.status().as_u16(),
            501,
            "{what}: not refused although the org is ZDR today (501 = no store in this harness)"
        );
    }
    assert_eq!(events("zdr.refused").len(), before, "nothing was refused");

    let refused = harness
        .post(INTERNAL_TOKEN, "/v1/internal/change/record", internal_record(Some(true)))
        .await;
    assert_eq!(refused.status().as_u16(), 403, "a ZDR stamp is refused with the flag on");
}

/// A service principal never reaches the change-monitor create routes, in either flag mode, with
/// or without a declared posture: the route scope middleware answers 403 before the handler, so
/// no schedule can be stamped by (or for) a service. Pins why the stamp has no service case.
#[tokio::test]
async fn a_service_principal_cannot_create_a_monitor_in_either_mode() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    for enforce in [false, true] {
        let control = control_plane().await;
        let harness = edge(control.uri(), enforce).await;
        for case in ["service_model_gateway", "service_execution_core"] {
            let mut schedule = monitor_schedule("https://page.test/svc");
            schedule["zdr"] = json!(true);
            let mut source = monitor_source("https://page.test/svc-src");
            source["zdr"] = json!(true);
            for (route, body) in [("/v1/schedules", schedule), ("/v1/sources", source)] {
                let denied = harness.post(&golden_token(case), route, body).await;
                assert_eq!(denied.status().as_u16(), 403, "{case} {route} (flag {enforce})");
            }
        }
        assert!(control_bodies(&control, "/v1/schedules").await.is_empty());
        assert!(control_bodies(&control, "/v1/sources").await.is_empty());
    }
}

// -- the orchestrator's two other calls: what a declared stamp suppresses ---------------------
//
// Round 7. The record tests above stop at 501 (this harness has no Postgres baseline store), so
// they never reach the fan-out after the baseline write, and nothing sent `zdr` to
// `/v1/internal/run_page`. The two tests below drive both effects directly.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::Utc;
use hmac::{Hmac, Mac};
use quarry_core::change_history::{BaselineSnapshot, ChangeRecord, ChangeStatus};
use quarry_core::output::DriverKind;
use quarry_core::zdr::ZdrMode;
use quarry_core::QuarryResult;
use quarry_edge::internal_auth::InternalSigner;
use quarry_runtime::artifact_store::FilesystemStore;
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::local_index::TantivyLocalIndex;
use quarry_runtime::EventSink;
use tokio::sync::mpsc;
use url::Url;

const SIGNING_SECRET: &str = "posture-stamp-signing-secret-0123456789abcdef";
const RUNTIME_TOKEN: &str = "posture-stamp-runtime-token";
const PAGE: &str = "<html><head><title>Stamp page</title></head><body><main><p>Page text for the posture stamp test, long enough to read as page content and to be indexed.</p></main></body></html>";

/// Serves every URL with `PAGE`. No network.
struct FixedPage;

#[async_trait]
impl Driver for FixedPage {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
            body: PAGE.as_bytes().to_vec(),
            duration_ms: 1,
            served_by: DriverKind::Static,
        })
    }
}

/// An event sink whose durable channel is collected, so a test can count what reached it.
/// `emit_ephemeral` (the ZDR path) never reaches the channel.
fn observing_sink() -> (EventSink, Arc<Mutex<Vec<quarry_core::event::Event>>>) {
    let (tx, mut rx) = mpsc::channel::<quarry_core::event::Event>(1024);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let collected = seen.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            collected.lock().unwrap().push(event);
        }
    });
    (EventSink::new(tx), seen)
}

/// Long enough for a fire-and-forget task or the collector to have run; used only before a
/// "nothing happened" assertion that a positive control in the same test makes non-vacuous.
async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
}

/// Polls (up to 15 s) until `done` holds, so a positive control never depends on the runner being
/// fast: only the "nothing happened" side of a test waits a fixed `settle()`.
async fn wait_for(mut done: impl FnMut() -> bool) {
    for _ in 0..150 {
        if done() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn changed_record() -> ChangeRecord {
    let now = Utc::now();
    ChangeRecord {
        source_url: "https://page.test/changed".into(),
        org_id: ZDR_ORG.into(),
        status: ChangeStatus::Changed,
        new_baseline: Some(BaselineSnapshot {
            baseline_id: "bln_new".into(),
            org_id: ZDR_ORG.into(),
            source_url: "https://page.test/changed".into(),
            fingerprint: "blake3:new".into(),
            artifact_id: None,
            prev_baseline_id: Some("bln_old".into()),
            captured_at: now,
            run_id: None,
        }),
        prev_baseline: None,
        diff_id: Some("diff_1".into()),
        checked_at: now,
    }
}

/// A `Changed` record fanned out with the given posture: the durable `ChangeDetected` events
/// that reached the sink and the webhook calls that reached Control Plane.
async fn fan_out(zdr: bool) -> (usize, usize) {
    let control = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/webhooks/change"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&control)
        .await;
    let (sink, seen) = observing_sink();
    let state = AppState {
        control_base_url: control.uri(),
        internal_signer: Some(Arc::new(InternalSigner::new(SIGNING_SECRET).unwrap())),
        ..support::base_state(Arc::new(FixedPage), sink)
    };

    // The same call `record_check` makes after the baseline is written, with the posture it
    // derives from the body (`ZdrMode::from(req.zdr.unwrap_or(false))`).
    quarry_edge::change_webhook::emit_change_side_effects(
        &state,
        &changed_record(),
        None,
        ZdrMode::from(zdr),
    )
    .await;

    // Positive controls wait for the asynchronous webhook; the ZDR case waits the settle time.
    if zdr {
        settle().await;
    } else {
        for _ in 0..50 {
            if !control.received_requests().await.unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        settle().await;
    }
    let events = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.event_type == quarry_core::event::EventType::ChangeDetected)
        .count();
    let webhooks = control
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/v1/webhooks/change")
        .count();
    (events, webhooks)
}

/// A change record that declares `zdr: true` suppresses the durable `ChangeDetected` event
/// (and so its NATS fan-out) and the signed change webhook; the same record with `zdr` false
/// produces both. The false case is the control that proves the harness can see them.
#[tokio::test]
async fn a_declared_zdr_change_record_suppresses_the_change_event_and_the_webhook() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();

    let (events, webhooks) = fan_out(false).await;
    assert_eq!(
        (events, webhooks),
        (1, 1),
        "control: a standard record emits one durable event and one webhook"
    );

    let (events, webhooks) = fan_out(true).await;
    assert_eq!(events, 0, "a ZDR record emits no durable change event");
    assert_eq!(webhooks, 0, "a ZDR record sends no change webhook");
}

struct RunPageEdge {
    addr: std::net::SocketAddr,
    client: reqwest::Client,
    root: PathBuf,
    events: Arc<Mutex<Vec<quarry_core::event::Event>>>,
    index: TantivyLocalIndex,
}

impl RunPageEdge {
    async fn start(root: &Path) -> Self {
        // `verify_runtime_token` latches this value once per process; every test here uses the same one.
        std::env::set_var("QUARRY_EDGE__RUNTIME_AUTH_TOKEN", RUNTIME_TOKEN);
        let (sink, events) = observing_sink();
        let index = TantivyLocalIndex::in_memory().expect("in-memory local index");
        let state = AppState {
            artifacts: Arc::new(FilesystemStore::new(root.to_path_buf()).expect("artifact store")),
            internal_signer: Some(Arc::new(InternalSigner::new(SIGNING_SECRET).unwrap())),
            local_index: Some(index.clone()),
            ..support::base_state(Arc::new(FixedPage), sink)
        };
        let app = quarry_edge::routes::router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            addr,
            client: reqwest::Client::new(),
            root: root.to_path_buf(),
            events,
            index,
        }
    }

    /// `POST /v1/internal/run_page` the way the orchestrator does: the runtime bearer token and
    /// the HMAC org binding over `org_id \n url`.
    async fn run_page(&self, body: Value) -> reqwest::Response {
        let org = body["org_id"].as_str().unwrap().to_owned();
        let url = body["url"].as_str().unwrap().to_owned();
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(SIGNING_SECRET.as_bytes()).unwrap();
        mac.update(format!("{org}\n{url}").as_bytes());
        let signature = format!("{:x}", mac.finalize().into_bytes());
        self.client
            .post(format!("http://{}/v1/internal/run_page", self.addr))
            .bearer_auth(RUNTIME_TOKEN)
            .header("x-quarry-run-sig", signature)
            .json(&body)
            .send()
            .await
            .expect("request reaches the edge")
    }

    /// (files in the artifact store, durable events, index documents written or pending).
    fn persisted(&self) -> (usize, usize, u64) {
        (
            files_under(&self.root).len(),
            self.events.lock().unwrap().len(),
            self.index.doc_count() + self.index.pending_writes(),
        )
    }
}

/// Every regular file under `dir`, at any depth.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

fn run_page_body(url: &str, zdr: Option<bool>) -> Value {
    let mut body = json!({
        "url": url,
        "org_id": ZDR_ORG,
        "run_id": "posture-stamp-run-page",
        "ingest": false,
    });
    if let Some(zdr) = zdr {
        body["zdr"] = json!(zdr);
    }
    body
}

/// The orchestrator's page fetch for a stamped-ZDR monitor persists nothing: no artifact file,
/// no durable event, no local-index write. The same call without the stamp (and with an explicit
/// `zdr: false`) persists all three, which is the control that makes the zero meaningful.
#[tokio::test]
async fn run_page_with_a_zdr_stamp_persists_no_artifact_event_or_index_write() {
    let _serial = CAPTURE_LOCK.lock().await;
    capture();
    let root = std::env::temp_dir().join(format!("quarry-posture-run-page-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let edge = RunPageEdge::start(&root).await;

    // Controls: unstamped (a legacy schedule) and stamped false both persist.
    for (n, zdr) in [(1, None), (2, Some(false))] {
        let before = edge.persisted();
        let served = edge
            .run_page(run_page_body(&format!("https://page.test/control-{n}"), zdr))
            .await;
        assert_eq!(served.status().as_u16(), 200, "control {n}: {:?}", served.text().await);
        wait_for(|| {
            let now = edge.persisted();
            now.0 > before.0 && now.1 > before.1 && now.2 > before.2
        })
        .await;
        let after = edge.persisted();
        assert!(after.0 > before.0, "control {n}: an artifact file is written {before:?} -> {after:?}");
        assert!(after.1 > before.1, "control {n}: a durable event is emitted {before:?} -> {after:?}");
        assert!(after.2 > before.2, "control {n}: the page is written to the local index {before:?} -> {after:?}");
    }

    // The stamped ZDR fetch leaves all three counters where they were.
    let before = edge.persisted();
    let served = edge
        .run_page(run_page_body("https://page.test/stamped", Some(true)))
        .await;
    assert_eq!(served.status().as_u16(), 200, "{:?}", served.text().await);
    settle().await;
    assert_eq!(edge.persisted(), before, "a stamped-ZDR run_page persists nothing");

    let _ = std::fs::remove_dir_all(&root);
}
