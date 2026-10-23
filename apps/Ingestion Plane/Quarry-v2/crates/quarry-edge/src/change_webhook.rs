//! Change-webhook emission for the change-tracking surface.
//!
//! Closes docs/CHANGE_TRACKING.md §"Webhook emission": `status=Changed`
//! events emit a webhook with subject `quarry.change.detected` whose
//! payload mirrors the `/v1/change/check` response (a
//! [`quarry_core::change_history::ChangeRecord`]).
//!
//! Delivery rides the existing cross-plane HMAC dispatcher
//! ([`crate::internal_auth::apply_to_request`]) as a signed edge → control
//! `POST /v1/webhooks/change`, so control's webhook fan-out (which owns
//! per-subscriber delivery) receives an authenticated, replay-resistant
//! event. Emission is best-effort: a control outage never fails the
//! caller's check, it only warn-logs.

use quarry_core::change_history::{ChangeRecord, ChangeStatus};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::RunKind;
use tracing::Instrument;

use crate::state::AppState;

/// Webhook event-type/subject for a detected content change. Pinned by
/// docs/CHANGE_TRACKING.md and by test — consumers filter on this string.
pub const CHANGE_DETECTED_SUBJECT: &str = "quarry.change.detected";

/// Control-plane route that accepts signed change webhooks.
pub const CHANGE_WEBHOOK_PATH: &str = "/v1/webhooks/change";

/// Build the webhook body for a change event.
///
/// Cross-agent contract (orchestrator-pinned): the body IS the
/// `/v1/change/check` response shape (`ChangeRecord`, field names
/// identical to the Rust wire struct so control's Go receiver decodes it
/// directly) plus two top-level extras — `subject`
/// (`quarry.change.detected`) and `emitted_at` (RFC3339 emission stamp,
/// distinct from the record's `checked_at`).
///
/// Returns `None` when the record is not a `Changed` result — only real
/// changes produce webhooks (New/Unchanged/Unreachable stay silent).
pub fn change_webhook_payload(record: &ChangeRecord) -> Option<serde_json::Value> {
    if record.status != ChangeStatus::Changed {
        return None;
    }
    let mut body = serde_json::to_value(record)
        .expect("ChangeRecord must serialize; all fields are serde-derive");
    let obj = body.as_object_mut().expect("ChangeRecord serializes to an object");
    obj.insert(
        "subject".into(),
        serde_json::Value::String(CHANGE_DETECTED_SUBJECT.into()),
    );
    obj.insert("emitted_at".into(), serde_json::json!(chrono::Utc::now()));
    Some(body)
}

/// Serialize + HMAC-sign a change webhook for delivery to control.
/// Returns `(path_with_query, body_bytes, headers)` ready for the
/// transport layer. Fails closed on serialization errors rather than
/// sending unsigned bytes.
pub fn sign_change_webhook(
    signer: &crate::internal_auth::InternalSigner,
    org_id: &str,
    payload: &serde_json::Value,
) -> Result<
    (
        String,
        Vec<u8>,
        crate::internal_auth::SignedHeaders,
    ),
    QuarryError,
> {
    if org_id.trim().is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "change webhook missing org_id",
        ));
    }
    let body =
        serde_json::to_vec(payload).map_err(|e| QuarryError::new(ErrorCode::Internal, format!("encode change webhook: {e}")))?;
    let path_q = format!("{CHANGE_WEBHOOK_PATH}?org_id={org_id}");
    let headers = crate::internal_auth::apply_to_request(signer, "POST", &path_q, &body);
    Ok((path_q, body, headers))
}

// The fan-out lives here, not in change_routes.rs, so the library build (which has this module
// ungated) compiles it in every feature set: `tests/zdr_posture_stamp.rs` drives the ZDR
// suppression directly, without a Postgres baseline store. The bin target declares this module
// only under `postgres-queue`, where `change_routes::record_check` is its one caller.
/// Post-persistence fan-out for a completed check: a durable
/// `ChangeDetected` event on the event sink (mirrors PageRunner's
/// ChangeDetected emissions so `/v1/runs/:id/events` consumers see the
/// change even though this path never ran a page pipeline) and, on real
/// changes, a signed webhook to control's delivery pipeline (subject
/// `quarry.change.detected`). Best-effort: a control outage warn-logs and
/// never fails the caller's check.
///
/// Fail closed under ZDR: when the declaring schedule runs
/// zero-data-retention, NEITHER side effect fires — the durable event log
/// is content-bearing and the webhook would push the same payload
/// off-process.
#[doc(hidden)]
pub async fn emit_change_side_effects(
    state: &AppState,
    record: &ChangeRecord,
    run_kind: Option<RunKind>,
    zdr: quarry_core::zdr::ZdrMode,
) {
    use quarry_core::change_history::ChangeStatus;
    use quarry_core::event::EventType;

    if zdr.is_active() {
        tracing::debug!(
            org_id = %record.org_id,
            "zdr active: change event + webhook suppressed"
        );
        return;
    }
    if record.status != ChangeStatus::Changed {
        return;
    }

    // Durable event log entry.
    let run_id = run_kind.unwrap_or_default();
    let idem = format!(
        "change_detected:{}:{}:{}",
        record.org_id,
        record.source_url,
        record
            .new_baseline
            .as_ref()
            .map(|b| b.baseline_id.clone())
            .unwrap_or_default()
    );
    let payload = serde_json::to_value(record).unwrap_or(serde_json::Value::Null);
    state
        .event_sink
        .emit(run_id, EventType::ChangeDetected, payload, idem)
        .await;

    // Signed webhook → control (subject quarry.change.detected).
    // Fire-and-forget transport: subscribers poll /v1/change/latest
    // anyway, so a missed push is availability loss, not correctness loss.
    if let Some(payload) = crate::change_webhook::change_webhook_payload(record) {
        if state.control_base_url.is_empty() {
            tracing::warn!("change webhook not sent: control base url empty");
        } else if let Some(signer) = state.internal_signer.as_ref() {
            match crate::change_webhook::sign_change_webhook(signer, &record.org_id, &payload) {
                Ok((path_q, body, headers)) => {
                    let base = state.control_base_url.trim_end_matches('/').to_string();
                    tokio::spawn(async move {
                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(10))
                            .build();
                        let Ok(client) = client else {
                            tracing::warn!("change webhook client build failed");
                            return;
                        };
                        let res = client
                            .post(format!("{base}{path_q}"))
                            .header(crate::internal_auth::HEADER_SIG, headers.signature)
                            .header(crate::internal_auth::HEADER_TS, headers.timestamp)
                            .header(crate::internal_auth::HEADER_NONCE, headers.nonce)
                            .header("content-type", "application/json")
                            .body(body)
                            .send()
                            .await;
                        match res {
                            Ok(r) if r.status().is_success() => {}
                            Ok(r) => {
                                tracing::warn!(status = %r.status(), "change webhook delivery failed")
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error_class = crate::zdr_log::class_reqwest(&e),
                                    "change webhook transport failed"
                                )
                            }
                        }
                    }.in_current_span());
                }
                Err(e) => tracing::warn!(error_class = crate::zdr_log::class(&e), "change webhook signing failed"),
            }
        } else {
            tracing::warn!("change webhook skipped: QUARRY_EDGE__INTERNAL_SECRET not configured (unsigned delivery refused, fail closed)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use quarry_core::change_history::BaselineSnapshot;

    fn changed_record() -> ChangeRecord {
        let now = Utc::now();
        ChangeRecord {
            source_url: "https://example.com/pricing".into(),
            org_id: "org_a".into(),
            status: ChangeStatus::Changed,
            new_baseline: Some(BaselineSnapshot {
                baseline_id: "bln_new".into(),
                org_id: "org_a".into(),
                source_url: "https://example.com/pricing".into(),
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

    #[test]
    fn payload_subject_is_pinned_constant() {
        let body = change_webhook_payload(&changed_record()).expect("changed yields webhook");
        assert_eq!(body["subject"], CHANGE_DETECTED_SUBJECT);
        assert_eq!(CHANGE_DETECTED_SUBJECT, "quarry.change.detected");
    }

    #[test]
    fn body_is_change_record_plus_subject_and_emitted_at() {
        let record = changed_record();
        let body = change_webhook_payload(&record).unwrap();
        // Orchestrator-pinned cross-agent contract: the Go receiver decodes
        // the body DIRECTLY as the ChangeRecord wire shape (+ subject,
        // emitted_at). Field names must be identical to the Rust struct.
        let inner: ChangeRecord = serde_json::from_value(body.clone())
            .expect("body decodes directly as ChangeRecord");
        assert_eq!(inner.status, ChangeStatus::Changed);
        assert_eq!(inner.diff_id.as_deref(), Some("diff_1"));
        assert_eq!(inner.source_url, record.source_url);
        assert_eq!(body["subject"], "quarry.change.detected");
        assert!(body.get("emitted_at").is_some());
    }

    #[test]
    fn non_changed_statuses_emit_nothing() {
        let mut status_rec = changed_record();
        status_rec.status = ChangeStatus::Unchanged;
        assert!(change_webhook_payload(&status_rec).is_none());
        status_rec.status = ChangeStatus::New;
        assert!(change_webhook_payload(&status_rec).is_none());
        status_rec.status = ChangeStatus::Unreachable;
        assert!(change_webhook_payload(&status_rec).is_none());
    }

    #[test]
    fn signed_webhook_covers_body_and_carries_headers() {
        let signer = crate::internal_auth::InternalSigner::new("0123456789abcdef0123456789abcdef")
            .expect("valid secret");
        let body_json = change_webhook_payload(&changed_record()).unwrap();
        let (path_q, bytes, headers) = sign_change_webhook(&signer, "org_a", &body_json).unwrap();

        assert_eq!(path_q, "/v1/webhooks/change?org_id=org_a");
        assert!(headers.signature.starts_with("sig_v1="));
        assert!(!bytes.is_empty());

        // Signature verifies over exactly these bytes.
        let ts: i64 = headers.timestamp.parse().expect("unix secs");
        let nonce = headers.nonce.clone();
        let digest = crate::internal_auth::InternalSigner::body_digest(&bytes);
        let expected = signer.sign_raw("POST", &path_q, &digest, ts, &nonce);
        assert_eq!(
            crate::internal_auth::InternalSigner::header_value(&expected),
            headers.signature,
            "HMAC must verify over the exact wire bytes"
        );
    }

    /// Control's receiver REQUIRES body.org_id == query org_id. The body is
    /// the full ChangeRecord whose `org_id` is a non-skipped field, so this
    /// holds by construction — pinned here so a future serde skip cannot
    /// silently break the cross-plane handshake.
    #[test]
    fn webhook_body_carries_org_id_matching_query() {
        let record = changed_record();
        assert_eq!(record.org_id, "org_a");
        let body = change_webhook_payload(&record).unwrap();
        assert_eq!(body["org_id"], serde_json::json!("org_a"));
        let signer = crate::internal_auth::InternalSigner::new("0123456789abcdef0123456789abcdef")
            .expect("valid secret");
        let (path_q, bytes, _) = sign_change_webhook(&signer, "org_a", &body).unwrap();
        assert!(path_q.starts_with("/v1/webhooks/change?org_id=org_a"));
        let decoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded["org_id"], serde_json::json!("org_a"));
    }

    #[test]
    fn signing_rejects_empty_org_fail_closed() {
        let signer = crate::internal_auth::InternalSigner::new("0123456789abcdef0123456789abcdef")
            .unwrap();
        let err = sign_change_webhook(&signer, "  ", &serde_json::json!({})).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}
