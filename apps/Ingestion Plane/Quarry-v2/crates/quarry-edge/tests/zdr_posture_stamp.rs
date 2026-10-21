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
        let body = json!({ "name": "legacy", "kind": "scrape", "cron": "*/5 * * * *", "zdr": true });
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
