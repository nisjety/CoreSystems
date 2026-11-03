//! INNH-11: `POST /v1/parse` with its flag off, the default.
//!
//! `QUARRY_EDGE__PARSE_ROUTE` is removed from the environment before the first request. The
//! route stays registered (the route-coverage guards need it) and answers 501 naming the
//! variable, after the auth chain: an unauthenticated caller still gets 401.

mod support;

#[path = "support/zdr_harness.rs"]
mod zdr_harness;

use serde_json::Value;
use serial_test::serial;
use zdr_harness::{golden_token, user_in_standard_org, Harness};

const PDF: &[u8] =
    include_bytes!("../../quarry-transform/tests/fixtures/documents/no/no-01-vedtak-bostotte.pdf");

async fn post(harness: &Harness, bearer: Option<&str>, body: Vec<u8>) -> (u16, Value) {
    let mut request = reqwest::Client::new()
        .post(harness.url("/v1/parse"))
        .header("content-type", "application/pdf");
    if let Some(bearer) = bearer {
        request = request.bearer_auth(bearer);
    }
    let response = request.body(body).send().await.expect("the edge answers");
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

#[tokio::test]
#[serial]
async fn off_by_default_the_route_is_a_501_that_names_the_variable() {
    std::env::remove_var("QUARRY_EDGE__PARSE_ROUTE");
    // Allowed or not, in-process parsing is not what answers when the route is off.
    std::env::set_var("QUARRY_EDGE__PARSE_ALLOW_INPROCESS", "1");
    let harness = Harness::start().await;

    let (status, body) = post(&harness, Some(&user_in_standard_org()), PDF.to_vec()).await;
    assert_eq!(status, 501, "{body}");
    assert_eq!(body["error"]["code"], "UNSUPPORTED");
    assert_eq!(body["error"]["details"]["reason"], "parse_route_disabled");
    assert_eq!(body["error"]["details"]["env"], "QUARRY_EDGE__PARSE_ROUTE");
    assert!(body["data"].is_null());

    // Anything that is not on, 1, true or yes is off.
    for value in ["", "off", "0", "false", "no", "maybe"] {
        std::env::set_var("QUARRY_EDGE__PARSE_ROUTE", value);
        let (status, body) = post(&harness, Some(&user_in_standard_org()), PDF.to_vec()).await;
        assert_eq!(status, 501, "{value:?}: {body}");
    }
    std::env::remove_var("QUARRY_EDGE__PARSE_ROUTE");
}

#[tokio::test]
#[serial]
async fn off_the_auth_chain_still_runs_first() {
    std::env::remove_var("QUARRY_EDGE__PARSE_ROUTE");
    let harness = Harness::start().await;
    let (status, _) = post(&harness, None, PDF.to_vec()).await;
    assert_eq!(status, 401);
    // A service principal with no parse scope is a 403, not a 501 that confirms the route.
    let (status, _) = post(
        &harness,
        Some(&golden_token("service_execution_core")),
        PDF.to_vec(),
    )
    .await;
    assert_eq!(status, 403);
}
