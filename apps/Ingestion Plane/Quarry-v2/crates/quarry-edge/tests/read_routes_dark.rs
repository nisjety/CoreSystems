//! The two tenant reads a dark edge must not serve. `GET /v1/sources/{id}` answers only while
//! `QUARRY_EDGE__SAVED_JOBS` or `QUARRY_EDGE__MONITOR_TAGS` is on, and `GET
//! /v1/schedules/{id}/requests/{rid}` only while `QUARRY_EDGE__SCHEDULE_RUNS` is on. With the
//! switches off (their default), each answers 501 `UNSUPPORTED`, naming the switch, before the
//! ids are parsed: a malformed id gets the same 501. Control Plane receives zero requests.
//!
//! Real RS256 tokens through the real router, as `monitor_tag_dark.rs` does. Nothing in this
//! binary sets a switch, so each is read off, which is its default.

use serde_json::Value;
use wiremock::MockServer;

use quarry_core::ids::kinds::{RequestKind, ScheduleKind, SourceKind};
use quarry_core::ids::Id;
use quarry_edge::state::AppState;

mod support;
#[path = "support/zdr_harness.rs"]
mod zdr_harness;

use zdr_harness::{user_in_standard_org, Harness};

async fn edge(control: &MockServer) -> Harness {
    let control_url = control.uri();
    Harness::start_with(move |state| AppState {
        control_base_url: control_url,
        ..state
    })
    .await
}

async fn count(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .expect("requests are recorded")
        .len()
}

async fn body_of(response: reqwest::Response) -> (u16, Value) {
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

#[tokio::test]
async fn a_source_read_is_501_while_both_switches_are_off_and_reaches_no_control() {
    // Nothing is mounted: a read that got past the gate would reach Control and be recorded.
    let control = MockServer::start().await;
    let harness = edge(&control).await;
    let source: SourceKind = Id::new();

    // The gate comes before the id is parsed, so a malformed id gets the same 501.
    for id in [source.to_string(), "not-a-source-id".to_owned()] {
        let route = format!("/v1/sources/{id}");
        let (status, body) = body_of(harness.get(&user_in_standard_org(), &route).await).await;
        assert_eq!(status, 501, "{route}: {body}");
        assert_eq!(body["error"]["code"], "UNSUPPORTED", "{route}: {body}");
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains("QUARRY_EDGE__SAVED_JOBS"), "{body}");
        assert!(message.contains("QUARRY_EDGE__MONITOR_TAGS"), "{body}");
    }

    assert_eq!(
        count(&control).await,
        0,
        "a dark source read never reaches Control Plane"
    );
}

#[tokio::test]
async fn a_request_row_read_is_501_while_its_switch_is_off_and_reaches_no_control() {
    let control = MockServer::start().await;
    let harness = edge(&control).await;
    let schedule: ScheduleKind = Id::new();
    let request: RequestKind = Id::new();

    // The switch is read before either id is parsed, so malformed ids get the same 501.
    for route in [
        format!("/v1/schedules/{schedule}/requests/{request}"),
        "/v1/schedules/not-a-schedule/requests/not-a-request".to_owned(),
    ] {
        let (status, body) = body_of(harness.get(&user_in_standard_org(), &route).await).await;
        assert_eq!(status, 501, "{route}: {body}");
        assert_eq!(body["error"]["code"], "UNSUPPORTED", "{route}: {body}");
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains("QUARRY_EDGE__SCHEDULE_RUNS"), "{body}");
    }

    assert_eq!(
        count(&control).await,
        0,
        "a dark request-row read never reaches Control Plane"
    );
}
