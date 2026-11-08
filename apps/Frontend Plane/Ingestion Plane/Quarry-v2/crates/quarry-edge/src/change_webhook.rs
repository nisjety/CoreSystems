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

use quarry_core::change_history::{BaselineSnapshot, ChangeRecord, ChangeStatus, ParagraphDiff};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::RunKind;
use tracing::Instrument;

use crate::state::AppState;

/// Webhook event-type/subject for a detected content change. Pinned by
/// docs/CHANGE_TRACKING.md and by test — consumers filter on this string.
pub const CHANGE_DETECTED_SUBJECT: &str = "quarry.change.detected";

/// Control-plane route that accepts signed change webhooks.
pub const CHANGE_WEBHOOK_PATH: &str = "/v1/webhooks/change";

/// The `change_type` of a first capture (E3.5-7). The edge sends no webhook for a first capture
/// today (see `change_webhook_payload`), so no filter sees this value yet.
pub const CHANGE_TYPE_NEW: &str = "new";
/// The `change_type` of a content change: the kind every change webhook carries today.
pub const CHANGE_TYPE_CHANGED: &str = "changed";

/// The change type a record's status classifies as, or `None` when the status is no change
/// (unchanged, unreachable). `removed` has no producer: on this path a page that is gone is not
/// yet told apart from a failed fetch.
pub fn change_type_of(status: ChangeStatus) -> Option<&'static str> {
    match status {
        ChangeStatus::New => Some(CHANGE_TYPE_NEW),
        ChangeStatus::Changed => Some(CHANGE_TYPE_CHANGED),
        ChangeStatus::Unchanged | ChangeStatus::Unreachable => None,
    }
}

/// `QUARRY_EDGE__CHANGE_WEBHOOK_DELTA` (E3.5-7): the change webhook carries the paragraph delta
/// measured from the stored text. Off (the default), the body carries `paragraph_delta: null` and
/// no stored text is read for it.
pub const ENV_CHANGE_WEBHOOK_DELTA: &str = "QUARRY_EDGE__CHANGE_WEBHOOK_DELTA";

/// Whether the paragraph delta is measured. Read on each change emit, like the other change flags.
pub fn change_webhook_delta_enabled() -> bool {
    delta_flag_is_on(std::env::var(ENV_CHANGE_WEBHOOK_DELTA).ok().as_deref())
}

/// The flag grammar: `1`, `true`, `yes` or `on` in any case, surrounding space ignored.
pub fn delta_flag_is_on(value: Option<&str>) -> bool {
    let norm = value.map(|v| v.trim().to_ascii_lowercase());
    matches!(norm.as_deref(), Some("1" | "true" | "yes" | "on"))
}

/// Build the webhook body for a change event.
///
/// Cross-agent contract (orchestrator-pinned): the body IS the
/// `/v1/change/check` response shape (`ChangeRecord`, field names
/// identical to the Rust wire struct so control's Go receiver decodes it
/// directly) plus three top-level extras — `subject`
/// (`quarry.change.detected`) and `emitted_at` (RFC3339 emission stamp,
/// distinct from the record's `checked_at`), and `change_type` (see `change_type_of`).
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
    if let Some(kind) = change_type_of(record.status) {
        obj.insert("change_type".into(), serde_json::Value::String(kind.into()));
    }
    Some(body)
}

/// The idempotency key of a `ChangeDetected` event: one per `(org, url, new baseline)`. A record
/// that names no new baseline (the recording path before INNH-4) ends in an empty id.
pub fn change_detected_idempotency_key(record: &ChangeRecord) -> String {
    format!(
        "change_detected:{}:{}:{}",
        record.org_id,
        record.source_url,
        record
            .new_baseline
            .as_ref()
            .map(|b| b.baseline_id.clone())
            .unwrap_or_default()
    )
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

/// The paragraph delta of a change: the paragraphs its diff added plus the ones it removed. A moved
/// paragraph is not new text, and a markup-only change counts zero. `None` is unknown, not zero: the
/// diff was not available (no stored text, or the text diff is off).
pub fn delta_of(diff: Option<&ParagraphDiff>) -> Option<u64> {
    diff.map(|d| d.added.saturating_add(d.removed) as u64)
}

/// The two baselines whose stored text a paragraph delta is read from. A record names both only when
/// the text path stored them (`QUARRY_EDGE__CHANGE_TEXT_DIFF`, and not zero data retention).
pub fn delta_pair(record: &ChangeRecord) -> Option<(&BaselineSnapshot, &BaselineSnapshot)> {
    Some((record.prev_baseline.as_ref()?, record.new_baseline.as_ref()?))
}

/// The paragraph delta of a changed record, measured from the stored markdown of its two baselines
/// (E3.5-7, behind `QUARRY_EDGE__CHANGE_WEBHOOK_DELTA`). The recorder computed this diff already; it
/// is computed again here so the recorder keeps its signature (see the depth file).
#[cfg(feature = "postgres-queue")]
async fn paragraph_delta_of(state: &AppState, record: &ChangeRecord) -> Option<u64> {
    if !change_webhook_delta_enabled() {
        return None;
    }
    let (from, to) = delta_pair(record)?;
    let outcome =
        crate::change_diff::compute_outcome(state.artifacts.as_ref(), &record.org_id, from, to)
            .await;
    delta_of(outcome.diff.as_ref())
}

/// Without the postgres-queue baseline store there is no stored text, so no delta is known.
#[cfg(not(feature = "postgres-queue"))]
async fn paragraph_delta_of(_state: &AppState, _record: &ChangeRecord) -> Option<u64> {
    None
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
    let idem = change_detected_idempotency_key(record);
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
            // E3.5-7: the paragraph delta rides the body, null when unknown, before it is signed.
            let mut payload = payload;
            let delta = paragraph_delta_of(state, record).await;
            payload["paragraph_delta"] = delta.map_or(serde_json::Value::Null, |n| serde_json::json!(n));
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
    fn the_idempotency_key_ends_in_the_new_baseline_id() {
        let record = changed_record();
        assert_eq!(
            change_detected_idempotency_key(&record),
            "change_detected:org_a:https://example.com/pricing:bln_new"
        );
        // Two changes of one url have different keys once the baseline is named.
        let mut next = changed_record();
        next.new_baseline.as_mut().unwrap().baseline_id = "bln_next".into();
        assert_ne!(
            change_detected_idempotency_key(&record),
            change_detected_idempotency_key(&next)
        );
        // The record the recorder passed before INNH-4 named none: the key ends empty.
        let mut unnamed = changed_record();
        unnamed.new_baseline = None;
        assert!(change_detected_idempotency_key(&unnamed).ends_with(':'));
    }

    #[test]
    fn the_payload_names_the_new_baseline_and_the_diff() {
        let body = change_webhook_payload(&changed_record()).unwrap();
        assert_eq!(body["new_baseline"]["baseline_id"], "bln_new");
        assert_eq!(body["diff_id"], "diff_1");
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

    #[test]
    fn the_payload_names_the_change_type() {
        let body = change_webhook_payload(&changed_record()).unwrap();
        assert_eq!(body["change_type"], CHANGE_TYPE_CHANGED);
    }

    #[test]
    fn change_type_follows_the_status() {
        assert_eq!(change_type_of(ChangeStatus::New), Some("new"));
        assert_eq!(change_type_of(ChangeStatus::Changed), Some("changed"));
        assert_eq!(change_type_of(ChangeStatus::Unchanged), None);
        assert_eq!(change_type_of(ChangeStatus::Unreachable), None);
    }

    #[test]
    fn the_delta_counts_added_and_removed_paragraphs_and_not_moves() {
        let diff = ParagraphDiff {
            format: "paragraph".into(),
            real_delta: true,
            added: 2,
            removed: 1,
            moved: 4,
            unchanged: 9,
            ops: Vec::new(),
            truncated: false,
        };
        assert_eq!(delta_of(Some(&diff)), Some(3));
        let markup_only = ParagraphDiff {
            real_delta: false,
            added: 0,
            removed: 0,
            moved: 0,
            ..diff.clone()
        };
        assert_eq!(delta_of(Some(&markup_only)), Some(0));
        assert_eq!(delta_of(None), None, "no diff is unknown, not zero");
    }

    #[test]
    fn a_delta_needs_the_stored_text_of_both_baselines() {
        let mut record = changed_record();
        assert!(delta_pair(&record).is_none(), "the fixture names no previous baseline");
        record.prev_baseline = record.new_baseline.clone();
        assert!(delta_pair(&record).is_some());
        record.new_baseline = None;
        assert!(delta_pair(&record).is_none());
    }

    #[test]
    fn the_delta_flag_is_on_only_when_named() {
        assert!(!delta_flag_is_on(None));
        assert!(!delta_flag_is_on(Some("")));
        assert!(!delta_flag_is_on(Some("off")));
        assert!(delta_flag_is_on(Some("1")));
        assert!(delta_flag_is_on(Some(" TRUE ")));
        assert!(delta_flag_is_on(Some("on")));
    }
}
