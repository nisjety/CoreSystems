//! E3.5-6: the monitor tag on a create and the `?tag=` filter are dark behind
//! `QUARRY_EDGE__MONITOR_TAGS`, as the tag PATCH is. With the switch off, a `POST` that names a tag
//! and a list that filters by one answer 501 `UNSUPPORTED` before the edge calls Control Plane, so
//! the Control double records zero requests. A blank tag names no tag: it reaches Control as an
//! untagged create or an unfiltered list, as it always did.
//!
//! Real RS256 tokens through the real router, as `url_gate_resources.rs` does. Nothing in this
//! binary sets the switch, so it is read off, which is its default.

use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use quarry_edge::state::AppState;

mod support;
#[path = "support/zdr_harness.rs"]
mod zdr_harness;

use zdr_harness::{user_in_standard_org, Harness};

/// Control Plane as the edge reaches it for these creates and lists. Every request it gets is
/// recorded, so a refused request is asserted as zero recorded requests.
async fn control_double() -> MockServer {
    let control = MockServer::start().await;
    for route in ["/v1/sources", "/v1/schedules"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "data": { "id": "res_1" } })),
            )
            .mount(&control)
            .await;
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [] })))
            .mount(&control)
            .await;
    }
    control
}

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
async fn a_create_that_names_a_tag_is_501_while_dark_and_reaches_no_control() {
    let control = control_double().await;
    let harness = edge(&control).await;

    let schedule = harness
        .post(
            &user_in_standard_org(),
            "/v1/schedules",
            json!({ "preset": "daily", "target_ref": "https://ok.example.org/", "tag": "pricing" }),
        )
        .await;
    let (status, body) = body_of(schedule).await;
    assert_eq!(status, 501, "{body}");
    assert_eq!(body["error"]["code"], "UNSUPPORTED", "{body}");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("QUARRY_EDGE__MONITOR_TAGS"), "{body}");

    let source = harness
        .post(
            &user_in_standard_org(),
            "/v1/sources",
            json!({
                "name": "Pricing",
                "url": "https://ok.example.org/",
                "kind": "crawl",
                "tag": "pricing"
            }),
        )
        .await;
    let (status, body) = body_of(source).await;
    assert_eq!(status, 501, "{body}");
    assert_eq!(body["error"]["code"], "UNSUPPORTED", "{body}");

    assert_eq!(
        count(&control).await,
        0,
        "a dark tagged create never reaches Control Plane"
    );
}

#[tokio::test]
async fn a_list_filtered_by_a_tag_is_501_while_dark_and_reaches_no_control() {
    let control = control_double().await;
    let harness = edge(&control).await;

    for route in ["/v1/sources?tag=pricing", "/v1/schedules?tag=pricing"] {
        let (status, body) = body_of(harness.get(&user_in_standard_org(), route).await).await;
        assert_eq!(status, 501, "{route}: {body}");
        assert_eq!(body["error"]["code"], "UNSUPPORTED", "{route}: {body}");
    }

    assert_eq!(
        count(&control).await,
        0,
        "a dark tag filter never reaches Control Plane"
    );
}

#[tokio::test]
async fn a_blank_tag_names_no_tag_and_reaches_control_while_dark() {
    let control = control_double().await;
    let harness = edge(&control).await;

    let source = harness
        .post(
            &user_in_standard_org(),
            "/v1/sources",
            json!({
                "name": "Plain",
                "url": "https://ok.example.org/",
                "kind": "crawl",
                "tag": "  "
            }),
        )
        .await;
    let (status, body) = body_of(source).await;
    assert_eq!(status, 200, "{body}");

    let list = harness
        .get(&user_in_standard_org(), "/v1/sources?tag=")
        .await;
    let (status, body) = body_of(list).await;
    assert_eq!(status, 200, "{body}");

    assert_eq!(
        count(&control).await,
        2,
        "an untagged create and an unfiltered list still reach Control Plane"
    );
}
