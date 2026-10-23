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

// ── WS12.S3: receipts, typed text, checkpoints ──────────────────────────────

/// The standard-org member (`user-golden` in `org-standard-golden`).
const ORG: &str = "org-standard-golden";
const ACTOR: &str = "user-golden";

/// A receipt written the way a release before WS12.S3 wrote it: raw typed text and no
/// rule version.
fn legacy_receipt(run: &str, step: u32, text: &str) -> quarry_runtime::StepReceipt {
    quarry_runtime::ReceiptBuilder::start(run.to_owned(), step)
        .org_id(ORG)
        .actor_id(ACTOR)
        .redaction_mode(quarry_core::redaction::Mode::Off)
        .complete(
            quarry_core::contracts::AgentAction::TypeRef {
                snapshot_id: "snap_old".to_owned(),
                generation: 1,
                ref_id: "@e1".to_owned(),
                text: text.to_owned(),
            },
            None,
            0.0,
        )
}

/// `GET .../receipts` returns typed text masked: a password field's text by its kind, an
/// ordinary field's text kept under the default policy, and a legacy row that holds raw
/// text masked on read (the stored row is not rewritten).
#[tokio::test]
async fn receipts_endpoint_returns_masked_typed_text() {
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
            json!({ "type": "type", "selector": "#q", "text": "wireless headphones" }),
            None,
        )
        .await;
    edge.state
        .receipts
        .append(legacy_receipt(&run, 90, &secret))
        .await
        .expect("the legacy row is stored");

    let receipts = edge.receipts(&person, &run).await;
    let rendered = serde_json::to_string(&receipts).unwrap();
    assert!(!rendered.contains(&secret), "a receipt carries the typed text: {rendered}");

    let password_step = receipts
        .iter()
        .find(|receipt| receipt["action"]["selector"] == "input#password")
        .expect("the password step has a receipt");
    assert_eq!(password_step["action"]["text"], "[redacted:password]");

    let search_step = receipts
        .iter()
        .find(|receipt| receipt["action"]["selector"] == "#q")
        .expect("the search step has a receipt");
    assert_eq!(search_step["action"]["text"], "wireless headphones");

    let legacy = receipts
        .iter()
        .find(|receipt| receipt["step"] == 90)
        .expect("the legacy row is listed");
    assert_eq!(legacy["action"]["text"], "[redacted:secret]");
    for receipt in &receipts {
        assert_eq!(
            receipt["redaction_rule_version"],
            quarry_core::redaction::RULE_VERSION,
            "every returned receipt is stamped: {receipt}"
        );
    }

    // The stored row still holds what was written: masking is a read-time act.
    let stored = edge
        .state
        .receipts
        .list_for_actor(ORG, ACTOR, &kind(&run))
        .await
        .unwrap();
    let stored_legacy = stored.iter().find(|receipt| receipt.step == 90).unwrap();
    assert_eq!(stored_legacy.redaction_rule_version, None);
}

/// The run checkpoint's URL carries no secret query value.
#[tokio::test]
async fn checkpoint_current_url_has_no_query_token() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;

    let _ = edge
        .step(
            &person,
            &run,
            json!({
                "type": "navigate",
                "url": format!("https://example.test/cb?access_token={}&page=2", token_shape()),
            }),
            None,
        )
        .await;
    let checkpoint = edge
        .state
        .receipts
        .load_run_checkpoint(ORG, &kind(&run))
        .await
        .unwrap()
        .expect("a step on a non-ZDR run saves a checkpoint");
    assert!(
        !checkpoint.current_url.contains(&token_shape()),
        "the checkpoint keeps a token: {}",
        checkpoint.current_url
    );
    assert!(checkpoint.current_url.contains("example.test/cb"));
}

/// An action sent over the browser socket is recorded as masked as one sent over REST.
#[tokio::test]
async fn ws_action_receipt_is_masked() {
    let edge = Edge::start(RunLimits::default()).await;
    let person = member_token();
    let run = edge.start_run(&person, json!({})).await;
    let secret = typed_secret();

    let mut socket =
        TestSocket::connect(&edge, &person, &format!("/v1/agent/runs/{run}/frames/ws")).await;
    socket
        .send_text(
            &json!({
                "type": "action",
                "action": { "type": "type", "selector": "input#password", "text": secret },
            })
            .to_string(),
        )
        .await;
    let _ = socket
        .next_matching(|message| message["type"] == "observation" || message["type"] == "error")
        .await;
    drop(socket);

    let receipts = edge.receipts(&person, &run).await;
    assert!(!receipts.is_empty(), "the socket action has a receipt");
    let rendered = serde_json::to_string(&receipts).unwrap();
    assert!(!rendered.contains(&secret), "{rendered}");
    assert_eq!(receipts[0]["action"]["text"], "[redacted:password]");
}

/// The receipt text policy boot values.
#[test]
fn the_receipt_text_policy_boot_values() {
    use quarry_core::redaction::action::RecordPolicy;
    use quarry_edge::redaction_boot::decide_record_policy;

    assert_eq!(decide_record_policy(None), Ok(RecordPolicy::SensitiveOnly));
    assert_eq!(
        decide_record_policy(Some("all")),
        Ok(RecordPolicy::AllTyped)
    );
    assert_eq!(
        decide_record_policy(Some("everything")),
        Err(RedactionBootError::ReceiptTextUnparsable)
    );
}
