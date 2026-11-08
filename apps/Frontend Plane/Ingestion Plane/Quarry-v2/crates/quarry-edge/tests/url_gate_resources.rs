//! W2-U33b: the org web policy on the three resources that name a URL and fetch it later:
//! `POST /v1/audio` (`audio_url`), `POST /v1/sources` and `POST /v1/schedules`, plus the crawl
//! completion registrar that writes a source row. The routes were the gap in the policy: every
//! scrape-family route gated its URL, these three did not.
//!
//! Real RS256 tokens through the real router (the golden Auth Core claims). The operator
//! baseline stands in for the org layer, as in `web_policy_routes.rs`: both are layers of the
//! same lattice. Control Plane and the Model Plane are wiremock doubles that record every
//! request, so "refused before any I/O" is asserted as zero recorded requests.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::mpsc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use quarry_edge::policy_baseline::{PolicyBaseline, BASELINE_ENV};
use quarry_edge::source_registrar::EdgeSourceRegistrar;
use quarry_edge::state::AppState;
use quarry_runtime::source_registrar::SourceRegistrar;
use quarry_runtime::{fetch::StaticDriver, EventSink};

mod support;
#[path = "support/zdr_harness.rs"]
mod zdr_harness;

use zdr_harness::{user_in_standard_org, user_in_zdr_org, Harness};

/// The operator denies this host; everything else is allowed.
const DENIED: &str = "denied.example.net";
const ALLOWED: &str = "ok.example.org";

fn deny_baseline() -> PolicyBaseline {
    let json = format!(r#"{{"domains":{{"deny":["{DENIED}"]}}}}"#);
    PolicyBaseline::from_lookup(|name| (name == BASELINE_ENV).then(|| json.clone()))
        .expect("the test baseline is a valid layer")
}

/// Control Plane as the edge reaches it for these resources.
async fn control_double() -> MockServer {
    let control = MockServer::start().await;
    for route in ["/v1/sources", "/v1/schedules"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "id": "res_1" } })))
            .mount(&control)
            .await;
    }
    control
}

/// The Model Plane's transcription route; answers a transcript.
async fn model_plane_double() -> MockServer {
    let model_plane = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/ai/transcribe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "text": "hello" })))
        .mount(&model_plane)
        .await;
    model_plane
}

async fn edge(control: &MockServer, model_plane: &MockServer) -> Harness {
    let control_url = control.uri();
    let model_plane_url = model_plane.uri();
    Harness::start_with(move |state| AppState {
        control_base_url: control_url,
        model_plane_url: Some(model_plane_url),
        model_plane: quarry_edge::model_plane_gate::ModelPlaneFeatures {
            model_audio: true,
            ..Default::default()
        },
        policy: deny_baseline(),
        ..state
    })
    .await
}

async fn count(server: &MockServer) -> usize {
    server.received_requests().await.expect("requests are recorded").len()
}

async fn body_of(response: reqwest::Response) -> (u16, Value, String) {
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null), text)
}

// -- audio_url ---------------------------------------------------------------------------------

#[tokio::test]
async fn audio_url_on_a_denied_host_is_403_and_reaches_nothing() {
    let (control, model_plane) = (control_double().await, model_plane_double().await);
    let harness = edge(&control, &model_plane).await;

    let response = harness
        .post(
            &user_in_standard_org(),
            "/v1/audio",
            json!({ "audio_url": format!("https://{DENIED}/talk.mp3?sig=s3cret") }),
        )
        .await;
    let (status, body, text) = body_of(response).await;

    assert_eq!(status, 403, "{text}");
    assert_eq!(body["code"], "FORBIDDEN");
    assert_eq!(body["details"]["capability"], "domains.deny");
    assert!(!text.contains("s3cret"), "the query is echoed: {text}");
    assert!(!text.contains("talk.mp3"), "the path is echoed: {text}");
    assert_eq!(count(&model_plane).await, 0, "a denied audio URL never reaches the Model Plane");
}

#[tokio::test]
async fn audio_url_on_an_allowed_host_is_forwarded() {
    let (control, model_plane) = (control_double().await, model_plane_double().await);
    let harness = edge(&control, &model_plane).await;

    let response = harness
        .post(
            &user_in_standard_org(),
            "/v1/audio",
            json!({ "audio_url": format!("https://{ALLOWED}/talk.mp3") }),
        )
        .await;
    let (status, body, text) = body_of(response).await;

    assert_eq!(status, 200, "{text}");
    assert_eq!(body["text"], "hello");
    let forwarded = model_plane.received_requests().await.unwrap();
    assert_eq!(forwarded.len(), 1);
    let sent: Value = serde_json::from_slice(&forwarded[0].body).unwrap();
    assert_eq!(sent["audio_url"], format!("https://{ALLOWED}/talk.mp3"));
}

#[tokio::test]
async fn audio_url_refusal_under_zdr_names_no_host_and_an_allowed_url_is_floored() {
    let (control, model_plane) = (control_double().await, model_plane_double().await);
    let harness = edge(&control, &model_plane).await;

    // The ZDR floor: the denial carries no host, and the Model Plane sees nothing.
    let response = harness
        .post(
            &user_in_zdr_org(),
            "/v1/audio",
            json!({ "audio_url": format!("https://{DENIED}/talk.mp3") }),
        )
        .await;
    let (status, body, text) = body_of(response).await;
    assert_eq!(status, 403, "{text}");
    assert_eq!(body["details"]["capability"], "domains.deny");
    assert!(!text.contains(DENIED), "a ZDR refusal names the host: {text}");
    assert!(!text.contains("example.net"), "a ZDR refusal names the host: {text}");
    assert_eq!(count(&model_plane).await, 0);

    // An allowed URL still goes through, and the Model Plane is told the request is ZDR.
    let response = harness
        .post(
            &user_in_zdr_org(),
            "/v1/audio",
            json!({ "audio_url": format!("https://{ALLOWED}/talk.mp3"), "zdr": false }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 200);
    let forwarded = model_plane.received_requests().await.unwrap();
    assert_eq!(forwarded.len(), 1);
    let sent: Value = serde_json::from_slice(&forwarded[0].body).unwrap();
    assert_eq!(sent["zdr"], true, "the floor raises the forwarded posture");
}

// -- sources -----------------------------------------------------------------------------------

fn source(url: &str) -> Value {
    json!({ "name": "Docs", "url": url, "kind": "crawl" })
}

#[tokio::test]
async fn source_create_on_a_denied_host_is_403_and_never_reaches_control() {
    let (control, model_plane) = (control_double().await, model_plane_double().await);
    let harness = edge(&control, &model_plane).await;

    let response = harness
        .post(&user_in_standard_org(), "/v1/sources", source(&format!("https://{DENIED}/docs?k=s3cret")))
        .await;
    let (status, body, text) = body_of(response).await;

    assert_eq!(status, 403, "{text}");
    assert_eq!(body["error"]["details"]["capability"], "domains.deny");
    assert!(!text.contains("s3cret"), "the query is echoed: {text}");
    assert_eq!(count(&control).await, 0, "a denied source is refused before the forward");
}

#[tokio::test]
async fn source_create_on_an_allowed_host_passes() {
    let (control, model_plane) = (control_double().await, model_plane_double().await);
    let harness = edge(&control, &model_plane).await;

    let response = harness
        .post(&user_in_standard_org(), "/v1/sources", source(&format!("https://{ALLOWED}/docs")))
        .await;

    assert_eq!(response.status().as_u16(), 200);
    let forwarded = control.received_requests().await.unwrap();
    assert_eq!(forwarded.len(), 1);
    let sent: Value = serde_json::from_slice(&forwarded[0].body).unwrap();
    assert_eq!(sent["url"], format!("https://{ALLOWED}/docs"));
}

#[tokio::test]
async fn source_create_refusal_under_zdr_names_no_host() {
    let (control, model_plane) = (control_double().await, model_plane_double().await);
    let harness = edge(&control, &model_plane).await;

    let response = harness
        .post(&user_in_zdr_org(), "/v1/sources", source(&format!("https://{DENIED}/docs")))
        .await;
    let (status, _, text) = body_of(response).await;

    assert_eq!(status, 403, "{text}");
    assert!(!text.contains("example.net"), "a ZDR refusal names the host: {text}");
    assert_eq!(count(&control).await, 0);
}

#[tokio::test]
async fn crawl_completion_registrar_never_writes_a_source_for_a_denied_host() {
    let control = control_double().await;
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        control_base_url: control.uri(),
        policy: deny_baseline(),
        ..support::base_state(driver, EventSink::new(tx))
    };
    let registrar = EdgeSourceRegistrar { state };

    let denied = registrar
        .register_source("org_a", "Denied", &format!("https://{DENIED}/"), "crawl")
        .await;
    assert!(denied.is_err(), "the registrar refuses a denied URL");
    assert_eq!(count(&control).await, 0);

    registrar
        .register_source("org_a", "Allowed", &format!("https://{ALLOWED}/"), "crawl")
        .await
        .expect("an allowed URL registers");
    assert_eq!(count(&control).await, 1);
}

// -- schedules ---------------------------------------------------------------------------------

#[tokio::test]
async fn change_monitor_schedule_on_a_denied_host_is_403_and_never_reaches_control() {
    let (control, model_plane) = (control_double().await, model_plane_double().await);
    let harness = edge(&control, &model_plane).await;

    let response = harness
        .post(
            &user_in_standard_org(),
            "/v1/schedules",
            json!({ "name": "watch", "preset": "daily", "target_ref": format!("https://{DENIED}/pricing") }),
        )
        .await;
    let (status, body, text) = body_of(response).await;

    assert_eq!(status, 403, "{text}");
    assert_eq!(body["error"]["details"]["capability"], "domains.deny");
    assert_eq!(count(&control).await, 0);
}

#[tokio::test]
async fn executable_schedule_is_refused_when_any_batch_url_is_denied() {
    let (control, model_plane) = (control_double().await, model_plane_double().await);
    let harness = edge(&control, &model_plane).await;

    // The first URL is fine; the second is denied. The whole create is refused, and the
    // refusal counts the denied URLs without naming one.
    let response = harness
        .post(
            &user_in_standard_org(),
            "/v1/schedules",
            json!({
                "name": "nightly",
                "kind": "batch",
                "cron": "0 3 * * *",
                "config": { "urls": [format!("https://{ALLOWED}/a"), format!("https://{DENIED}/b")] },
            }),
        )
        .await;
    let (status, body, text) = body_of(response).await;

    assert_eq!(status, 403, "{text}");
    assert_eq!(body["error"]["details"]["denied_count"], 1);
    assert!(!text.contains(DENIED), "the batch refusal names a host: {text}");
    assert_eq!(count(&control).await, 0);

    // A crawl whose one URL is denied is refused the same way.
    let response = harness
        .post(
            &user_in_standard_org(),
            "/v1/schedules",
            json!({
                "name": "crawl",
                "kind": "crawl",
                "cron": "0 3 * * *",
                "config": { "url": format!("https://{DENIED}/") },
            }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(count(&control).await, 0);
}

#[tokio::test]
async fn schedules_on_allowed_hosts_pass() {
    let (control, model_plane) = (control_double().await, model_plane_double().await);
    let harness = edge(&control, &model_plane).await;

    let monitor = harness
        .post(
            &user_in_standard_org(),
            "/v1/schedules",
            json!({ "name": "watch", "preset": "daily", "target_ref": format!("https://{ALLOWED}/pricing") }),
        )
        .await;
    assert_eq!(monitor.status().as_u16(), 200);

    let batch = harness
        .post(
            &user_in_standard_org(),
            "/v1/schedules",
            json!({
                "name": "nightly",
                "kind": "batch",
                "cron": "0 3 * * *",
                "config": { "urls": [format!("https://{ALLOWED}/a"), format!("https://{ALLOWED}/b")] },
            }),
        )
        .await;
    assert_eq!(batch.status().as_u16(), 200);
    assert_eq!(count(&control).await, 2, "both allowed creates reach Control Plane");
}

#[tokio::test]
async fn schedule_refusal_under_zdr_names_no_host() {
    let (control, model_plane) = (control_double().await, model_plane_double().await);
    let harness = edge(&control, &model_plane).await;

    // The durable-resource refusal is off here (stage A), so the request reaches the URL
    // gate, which is what carries the ZDR floor: no host in the refusal.
    let response = harness
        .post(
            &user_in_zdr_org(),
            "/v1/schedules",
            json!({ "name": "watch", "preset": "daily", "target_ref": format!("https://{DENIED}/pricing") }),
        )
        .await;
    let (status, _, text) = body_of(response).await;

    assert_eq!(status, 403, "{text}");
    assert!(!text.contains("example.net"), "a ZDR refusal names the host: {text}");
    assert_eq!(count(&control).await, 0);
}
