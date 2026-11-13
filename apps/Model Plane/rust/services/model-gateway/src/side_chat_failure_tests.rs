//! V1 R8 follow-up (side chat on the thread's connection). Two points the verifier left open:
//!
//! 1. A side chat served on the thread's subscription connection ends a failed inference call on the subscription
//!    catalogue, as every other subscription call does (`/v1/invoke`, the direct inference run):
//!    `side_chat_inference_error` answers the typed code, or the generic route failure, with the catalogue's words,
//!    `retryable`, timing and status, and never the upstream message. The tests feed it the status inference-core
//!    sends when the user's subscription needs reconnecting, and assert the typed answer a client can act on. A
//!    platform or customer-gateway call keeps `grpc_status_to_http`, pinned below.
//! 2. The waiver branch of `connection_posture` cannot be reached from the route: `side_chat_with` decides the plain
//!    route first, and a waiver on the plain route is a route mismatch, so a waived caller is refused there, before
//!    the thread is read. Pinned here so the fail-closed order is visible.

use axum::http::StatusCode;
use mp_contracts::zdr_refusal::code;

use crate::auth::{Claims, ZdrWaiverClaim};
use crate::subscription_failure::{
    PROMPT_TOO_LONG, RESETS_AT_TRAILER, RETRY_AFTER_MS_TRAILER, SUBSCRIPTION_REAUTH_REQUIRED,
    SUBSCRIPTION_ROUTE_UNAVAILABLE, SUBSCRIPTION_USAGE_LIMIT,
};

fn subscription_status(kind: &str) -> tonic::Status {
    let mut status = tonic::Status::failed_precondition("upstream said: token expired for acct 42");
    status.metadata_mut().insert(
        crate::compaction::PROVIDER_ERROR_KIND_TRAILER,
        kind.parse().expect("ascii"),
    );
    status
}

/// The mapping `side_chat_with` applies to a failed `infer` on a subscription thread.
fn side_chat_inference_error(status: &tonic::Status) -> crate::http_routes::HttpJsonError {
    crate::side_chat_routes::side_chat_inference_error(status, true)
}

/// A status that carries the given trailers, as inference-core declares them.
fn trailered(
    grpc_code: tonic::Code,
    message: &str,
    trailers: &[(&'static str, &str)],
) -> tonic::Status {
    let mut metadata = tonic::metadata::MetadataMap::new();
    for (key, value) in trailers {
        metadata.insert(*key, value.parse().expect("ascii"));
    }
    tonic::Status::with_metadata(grpc_code, message, metadata)
}

#[test]
fn a_subscription_failure_on_a_side_chat_keeps_its_catalogue_code() {
    for (kind, want) in [
        (
            SUBSCRIPTION_REAUTH_REQUIRED,
            crate::subscription_failure::http_status(SUBSCRIPTION_REAUTH_REQUIRED),
        ),
        (
            SUBSCRIPTION_USAGE_LIMIT,
            crate::subscription_failure::http_status(SUBSCRIPTION_USAGE_LIMIT),
        ),
    ] {
        let (status, axum::Json(body)) = side_chat_inference_error(&subscription_status(kind));
        assert_eq!(
            body["code"].as_str(),
            Some(kind),
            "the side chat drops the subscription failure's code: {status} {body}"
        );
        assert_eq!(status, want, "{body}");
        assert!(
            !body.to_string().contains("acct 42"),
            "the upstream message is relayed verbatim instead of the catalogue's words: {body}"
        );
    }
}

#[test]
fn a_failure_with_no_typed_reason_is_the_generic_route_failure() {
    let status = tonic::Status::unavailable("upstream said: token expired for acct 42");
    let (http_status, axum::Json(body)) = side_chat_inference_error(&status);
    assert_eq!(http_status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(
        body["code"].as_str(),
        Some(SUBSCRIPTION_ROUTE_UNAVAILABLE),
        "{body}"
    );
    assert_eq!(body["retryable"].as_bool(), Some(true), "{body}");
    assert!(!body.to_string().contains("acct 42"), "{body}");
}

#[test]
fn a_side_chat_usage_limit_carries_its_reset_and_wait() {
    let trailers = [
        (
            crate::compaction::PROVIDER_ERROR_KIND_TRAILER,
            SUBSCRIPTION_USAGE_LIMIT,
        ),
        (RETRY_AFTER_MS_TRAILER, "52260000"),
        (RESETS_AT_TRAILER, "2026-09-26T01:21:00Z"),
    ];
    let status = trailered(
        tonic::Code::ResourceExhausted,
        "upstream said: quota for acct 42",
        &trailers,
    );
    let (http_status, axum::Json(body)) = side_chat_inference_error(&status);
    assert_eq!(http_status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(
        body["code"].as_str(),
        Some(SUBSCRIPTION_USAGE_LIMIT),
        "{body}"
    );
    assert_eq!(body["retryable"].as_bool(), Some(false), "{body}");
    assert_eq!(body["retry_after_ms"].as_u64(), Some(52_260_000), "{body}");
    assert_eq!(
        body["resets_at"].as_str(),
        Some("2026-09-26T01:21:00Z"),
        "{body}"
    );
    assert_eq!(
        body["retry_at"].as_str(),
        Some("2026-09-26T01:21:00Z"),
        "{body}"
    );
    let words = body["error"].as_str().unwrap_or_default();
    assert!(words.contains("26.09.2026 kl. 01:21 UTC"), "{body}");
    assert!(!body.to_string().contains("acct 42"), "{body}");
}

#[test]
fn an_overlong_side_chat_on_the_subscription_route_is_prompt_too_long() {
    let trailers = [(crate::compaction::PROVIDER_ERROR_KIND_TRAILER, "too_long")];
    let status = trailered(
        tonic::Code::InvalidArgument,
        "upstream said: token expired for acct 42",
        &trailers,
    );
    let (http_status, axum::Json(body)) = side_chat_inference_error(&status);
    assert_eq!(http_status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(body["code"].as_str(), Some(PROMPT_TOO_LONG), "{body}");
    assert_eq!(body["retryable"].as_bool(), Some(false), "{body}");
    assert!(!body.to_string().contains("acct 42"), "{body}");
}

#[test]
fn a_typed_refusal_keeps_its_own_body_on_the_subscription_route() {
    let zdr = mp_contracts::zdr_refusal::Refusal::CONNECTION_NOT_ACKNOWLEDGED
        .grpc_status(tonic::Code::FailedPrecondition);
    let entitlement = mp_contracts::model_entitlement::Refusal::not_entitled(
        "gpt-x",
        mp_contracts::model_entitlement::Reason::NotInAllowList,
    )
    .grpc_status();
    for status in [zdr, entitlement] {
        let (flat_status, axum::Json(flat)) = crate::http_routes::grpc_status_to_http(&status);
        let (http_status, axum::Json(body)) = side_chat_inference_error(&status);
        assert_eq!((http_status, &body), (flat_status, &flat), "{body}");
        assert!(
            body["code"].is_null(),
            "a typed refusal is not a subscription failure: {body}"
        );
    }
}

#[test]
fn a_platform_or_customer_gateway_side_chat_keeps_grpc_status_to_http() {
    let status = subscription_status(SUBSCRIPTION_REAUTH_REQUIRED);
    let (flat_status, axum::Json(flat)) = crate::http_routes::grpc_status_to_http(&status);
    let (http_status, axum::Json(body)) =
        crate::side_chat_routes::side_chat_inference_error(&status, false);
    assert_eq!((http_status, &body), (flat_status, &flat), "{body}");
    assert!(body["code"].is_null(), "{body}");
}

fn waived_claims(connection: &str) -> Claims {
    Claims {
        key_id: None,
        acting_user_id: None,
        sub: "user-a".to_owned(),
        iss: "test-issuer".to_owned(),
        exp: i64::MAX,
        org_id: "org-a".to_owned(),
        user_id: "user-a".to_owned(),
        nbf: None,
        aud: Some("model-gateway".to_owned()),
        scopes: Vec::new(),
        zdr: false,
        sovereign: None,
        org_zdr: Some(true),
        zdr_waiver: Some(ZdrWaiverClaim {
            version: 1,
            provider: mp_contracts::agentic_subscription::PROVIDER_ID.to_owned(),
            connection: connection.to_owned(),
        }),
        principal_type: Some("user".to_owned()),
        service_id: None,
        reason: None,
    }
}

#[test]
fn a_waived_caller_is_refused_on_the_plain_route_before_the_thread_is_read() {
    // `side_chat_with` asks this first, before `refuse_unavailable_thread` reads anything.
    let (_status, axum::Json(body)) = crate::zdr_posture::zdr_or_refuse(
        &waived_claims("conn_sub"),
        &crate::zdr_posture::Route::plain(false),
    )
    .expect_err("a waiver binds one subscription connection, not the plain route");
    assert_eq!(body["error"]["code"], code::WAIVER_ROUTE_MISMATCH, "{body}");
}
