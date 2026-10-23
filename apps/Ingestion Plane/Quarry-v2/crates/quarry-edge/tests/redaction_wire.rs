//! WS12.S2: what leaves the edge over the wire carries no secret.
//!
//! Through the real router and the harness in `support/agent_harness.rs`:
//!
//! - an error body from a driver or target failure (`driver_err`) has no secret
//!   substring, whatever the failed action carried (Flow C of the slice: a stale
//!   `type_ref` with the secret as its text);
//! - the durable events a run emits (`ActionStarted`, `ObservationReady`) carry
//!   no typed secret;
//! - `QuarryError::redacted` masks a token a driver message echoed, and leaves
//!   the code, the status and the retryability alone;
//! - the boot policy refuses `shadow` and `off` in production.
//!
//! Nothing here needs Chromium. The secret is assembled at run time so no
//! credential-shaped literal exists in the repository.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use agent_harness::*;
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_edge::redaction_boot::{decide, RedactionBootError};
use quarry_edge::run_limits::RunLimits;
use serde_json::json;

fn typed_secret() -> String {
    ["S3cret", "P@ss-0451"].concat()
}

fn token_shape() -> String {
    format!(
        "{}.{}.{}",
        ["eyJ", "hbGciOiJIUzI1NiJ9"].concat(),
        ["eyJ", "zdWIiOiIxMjM0NTY3ODkwIn0"].concat(),
        ["SflKxwRJSM", "eKKF2QT4fwpMeJf36POk6yJV_adQssw5c"].concat()
    )
}

/// Flow C: a `type_ref` against a snapshot that does not exist answers 409 and the
/// body does not hold the text that was to be typed.
#[tokio::test]
async fn a_stale_type_ref_error_body_has_no_secret() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let secret = typed_secret();

    let reply = edge
        .step(
            &person,
            &run,
            json!({
                "type": "type_ref",
                "snapshot_id": "snap_stale",
                "generation": 1,
                "ref_id": "@e1",
                "text": secret,
            }),
            None,
        )
        .await;
    assert!(
        reply.status == 409 || reply.status == 400,
        "a stale ref is a client condition: {} {}",
        reply.status,
        reply.body
    );
    assert!(
        !reply.body.to_string().contains(&secret),
        "the error body echoes the typed text: {}",
        reply.body
    );
}

/// The events a run emits carry no typed secret, whatever the step's outcome.
#[tokio::test]
async fn durable_events_carry_no_typed_secret() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let secret = typed_secret();

    let _ = edge
        .step(
            &person,
            &run,
            json!({ "type": "type", "selector": "input#password", "text": secret }),
            None,
        )
        .await;
    let _ = edge
        .step(
            &person,
            &run,
            json!({
                "type": "navigate",
                "url": format!("https://example.test/cb?access_token={}", token_shape()),
            }),
            None,
        )
        .await;
    for (event_type, payload) in edge.durable_events().await {
        let rendered = payload.to_string();
        assert!(
            !rendered.contains(&secret),
            "{event_type:?} carries the typed text: {rendered}"
        );
        assert!(
            !rendered.contains(&token_shape()),
            "{event_type:?} carries a token from a URL: {rendered}"
        );
    }
}

/// The envelope shape is unchanged; only its content is masked.
#[test]
fn a_redacted_error_keeps_its_code_and_masks_its_text() {
    let token = token_shape();
    let error = QuarryError::new(ErrorCode::DriverFailed, format!("driver said {token}"))
        .with_details(json!({ "echo": token, "count": 3 }))
        .redacted();
    assert_eq!(error.code, ErrorCode::DriverFailed);
    assert!(error.retryable == ErrorCode::DriverFailed.retryable());
    let body = serde_json::to_string(&Envelope::<()>::err("req_1", error)).unwrap();
    assert!(!body.contains(&token), "{body}");
    assert!(body.contains("\"count\":3"));
    assert!(body.contains("DRIVER_FAILED"));
}

#[test]
fn production_refuses_shadow_and_off() {
    assert!(decide(Some("enforce"), true).is_ok());
    assert_eq!(
        decide(Some("shadow"), true),
        Err(RedactionBootError::NotPermittedInProduction(
            quarry_core::redaction::Mode::Shadow
        ))
    );
    assert!(decide(Some("off"), true).is_err());
    assert!(decide(Some("shadow"), false).is_ok());
}
