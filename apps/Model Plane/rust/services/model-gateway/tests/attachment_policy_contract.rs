//! The server attachment contract's wiring (WS08-S2): the body limit the chat
//! stream route carries, where the policy runs in the stream, and what
//! `/v1/models` declares.
//!
//! The stream handler needs a full set of signed bearers before it reads its
//! body, so the limit is shown on a route layered the way `build_router` layers
//! the real one, and the real route is pinned to that layering by reading its
//! source, the same way `zdr_feature_table.rs` pins the ZDR branch.

use axum::{
    body::Body,
    extract::Request,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use model_gateway::attachment_policy::{
    stream_body_limit_bytes, AttachmentCode, AttachmentPolicy, DEFAULT_STREAM_BODY_LIMIT_BYTES,
};
use tower::ServiceExt;

const MIB: usize = 1024 * 1024;

fn stream_route() -> Router {
    async fn echo(Json(body): Json<serde_json::Value>) -> StatusCode {
        assert!(body.get("content").is_some());
        StatusCode::OK
    }
    Router::new().route(
        "/v1/invoke/stream",
        post(echo).layer(axum::extract::DefaultBodyLimit::max(stream_body_limit_bytes())),
    )
}

fn body_of(bytes: usize) -> Request {
    let padding = "A".repeat(bytes);
    Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header("content-type", "application/json")
        .body(Body::from(format!(r#"{{"content":"x","pad":"{padding}"}}"#)))
        .expect("a request")
}

#[tokio::test]
#[serial_test::serial]
async fn a_5_mib_body_passes_and_a_13_mib_body_gets_413() {
    std::env::remove_var("CHAT_STREAM_BODY_LIMIT_BYTES");
    assert_eq!(stream_body_limit_bytes(), 12 * MIB);
    assert_eq!(DEFAULT_STREAM_BODY_LIMIT_BYTES, 12 * MIB);

    let passed = stream_route().oneshot(body_of(5 * MIB)).await.expect("served");
    assert_eq!(passed.status(), StatusCode::OK);

    let refused = stream_route().oneshot(body_of(13 * MIB)).await.expect("served");
    assert_eq!(refused.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn the_real_stream_route_carries_the_body_limit() {
    let routes = include_str!("../src/http_routes.rs");
    let route = routes
        .find("\"/v1/invoke/stream\"")
        .expect("the stream route is registered");
    let layer = routes[route..]
        .find("DefaultBodyLimit::max(")
        .expect("the stream route is layered with a body limit");
    assert!(
        layer < 200,
        "the body limit must be on the stream route itself, not a later route"
    );
    assert!(
        routes[route..route + 400].contains("attachment_policy::stream_body_limit_bytes()"),
        "the stream route's limit is the one the attachment policy declares"
    );
}

/// Posture first (a ZDR or waiver refusal keeps its catalogue code), the attachment
/// contract next, and only then normalisation, which refuses a subscription turn
/// that carries attachments in a generic code that would hide the typed one.
#[test]
fn the_stream_validates_after_the_posture_and_before_normalisation() {
    let sse = include_str!("../src/sse.rs");
    let stream = sse
        .find("pub async fn invoke_stream_sse(")
        .expect("the stream handler");
    let body = &sse[stream..];
    let position = |needle: &str| {
        body.find(needle)
            .unwrap_or_else(|| panic!("invoke_stream_sse no longer contains {needle}"))
    };
    let posture = position("zdr_posture::decide_for(");
    let waiver = position("WAIVER_ROUTE_MISMATCH");
    let policy = position("attachment_policy.validate(");
    let normalised = position("normalize::normalize_stream(");
    let zdr_branch = position("zdr_features::refusal(");
    assert!(posture < waiver && waiver < policy, "the waiver refusal keeps priority");
    assert!(policy < normalised, "the typed refusal precedes normalisation");
    assert!(normalised < zdr_branch);
    assert!(
        body[..policy].contains("!turn_posture.zdr()"),
        "a ZDR turn must be left to zdr_features::refusal, which owns its attachment refusal"
    );
}

#[test]
fn models_declares_the_policy_beside_the_catalogue() {
    let routes = include_str!("../src/http_routes.rs");
    assert!(routes.contains("attachment_policy: Option<crate::attachment_policy::DeclaredAttachmentPolicy>"));
    let declared = AttachmentPolicy::default().declared(true).expect("on by default");
    let json = serde_json::to_value(declared).expect("serialises");
    for key in [
        "image_mimes",
        "max_images",
        "max_image_bytes",
        "max_files",
        "max_file_bytes",
        "max_total_bytes",
        "modes",
    ] {
        assert!(json.get(key).is_some(), "{key} is declared");
    }
}

#[test]
fn every_refusal_code_is_a_stable_attachment_code() {
    for code in AttachmentCode::ALL {
        assert!(code.as_str().starts_with("attachment_"), "{}", code.as_str());
    }
}
