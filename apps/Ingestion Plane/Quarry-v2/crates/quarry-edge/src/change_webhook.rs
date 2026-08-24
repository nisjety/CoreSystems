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

    #[test]
    fn signing_rejects_empty_org_fail_closed() {
        let signer = crate::internal_auth::InternalSigner::new("0123456789abcdef0123456789abcdef")
            .unwrap();
        let err = sign_change_webhook(&signer, "  ", &serde_json::json!({})).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}
