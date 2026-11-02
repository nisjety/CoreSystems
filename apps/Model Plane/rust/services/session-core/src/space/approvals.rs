//! Typed approval card context and derived continuation state (build plan WS10-S1).
//!
//! Two things live here, both content-free:
//!
//! - The approval *card*: `tool_key`, `action_label` and `resume_class`, validated from the
//!   creator's `context_json` and stored under `approvals.metadata.card`. The resume class is the
//!   server's own truth, never the caller's claim: an approval that carries a continuation
//!   descriptor is `action_continuation`, one that does not can only be `live_poll` (a browser
//!   action polled inside the running loop) or `none`.
//! - The *continuation* of a granted approval, derived on every read from the delivery outbox, the
//!   start receipt and the outcome (migrations 0014, 0018, 0019, 0020). Nothing is stored for it
//!   and no client can state it, so the card can only claim what the rows prove.
//!
//! No migration: the card is a JSONB key on the existing `approvals.metadata`.

use std::collections::HashMap;

use mp_contracts::model_plane::v1 as pb;
use serde_json::{Map, Value as JsonValue};
use sqlx::FromRow;
use tonic::Status;
use tracing::warn;

use crate::store::Pool;

/// Largest accepted `CreateApprovalRequest.context_json`.
pub(crate) const MAX_CONTEXT_BYTES: usize = 4096;
/// Longest accepted value in the context object, in characters.
pub(crate) const MAX_CONTEXT_VALUE_CHARS: usize = 128;
/// The only keys a creator may state. Everything else is refused, so raw arguments and
/// credential-shaped keys can never enter a card.
const ALLOWED_CONTEXT_KEYS: &[&str] = &["tool_key", "action_label", "resume_class"];

/// Resume class stored when the approval carries a continuation descriptor.
pub(crate) const RESUME_ACTION_CONTINUATION: &str = "action_continuation";
/// Resume class of a browser live approval polled inside the running loop.
pub(crate) const RESUME_LIVE_POLL: &str = "live_poll";
/// Resume class of an approval that only lets the agent retry the exact action once.
pub(crate) const RESUME_NONE: &str = "none";

/// Validate the creator's `context_json` and build the `metadata.card` value.
///
/// Returns `Ok(None)` when the creator stated no context and there is no descriptor (the
/// approval is stored exactly as before and its continuation is derived without a resume class).
/// A zero-retention caller keeps its `tool_key` and resume class but never an `action_label`.
pub(crate) fn card_from_request(
    context_json: &str,
    has_descriptor: bool,
    zdr: bool,
) -> Result<Option<JsonValue>, Status> {
    let stated = parse_context(context_json)?;
    if stated.is_none() && !has_descriptor {
        return Ok(None);
    }
    let stated = stated.unwrap_or_default();
    let mut card = Map::new();
    if let Some(tool_key) = stated.get("tool_key") {
        card.insert("tool_key".to_owned(), JsonValue::String(tool_key.clone()));
    }
    if !zdr {
        if let Some(label) = stated.get("action_label") {
            card.insert("action_label".to_owned(), JsonValue::String(label.clone()));
        }
    }
    let resume = if has_descriptor {
        RESUME_ACTION_CONTINUATION
    } else if stated.get("resume_class").map(String::as_str) == Some(RESUME_LIVE_POLL) {
        RESUME_LIVE_POLL
    } else {
        RESUME_NONE
    };
    card.insert(
        "resume_class".to_owned(),
        JsonValue::String(resume.to_owned()),
    );
    Ok(Some(JsonValue::Object(card)))
}

/// Parse the allowlisted context object; empty input is "no context".
fn parse_context(context_json: &str) -> Result<Option<HashMap<String, String>>, Status> {
    if context_json.is_empty() {
        return Ok(None);
    }
    if context_json.len() > MAX_CONTEXT_BYTES {
        return Err(Status::invalid_argument("approval context is too large"));
    }
    let value: JsonValue = serde_json::from_str(context_json)
        .map_err(|_| Status::invalid_argument("approval context must be valid JSON"))?;
    let object = value
        .as_object()
        .ok_or_else(|| Status::invalid_argument("approval context must be an object"))?;
    let mut stated = HashMap::new();
    for (key, value) in object {
        if !ALLOWED_CONTEXT_KEYS.contains(&key.as_str()) {
            return Err(Status::invalid_argument("approval context key is not allowed"));
        }
        let text = value
            .as_str()
            .ok_or_else(|| Status::invalid_argument("approval context values must be strings"))?;
        if text.chars().count() > MAX_CONTEXT_VALUE_CHARS || text.chars().any(char::is_control) {
            return Err(Status::invalid_argument("approval context value is invalid"));
        }
        stated.insert(key.clone(), text.to_owned());
    }
    Ok(Some(stated))
}

/// A text field of the stored card; empty when the approval has none.
pub(crate) fn card_text(metadata: &JsonValue, key: &str) -> String {
    metadata["card"][key].as_str().unwrap_or_default().to_owned()
}

/// What the rows say about one approval's delivery, read in one join.
#[derive(FromRow, Debug, Clone)]
pub(crate) struct ContinuationFacts {
    pub approval_id: String,
    pub status: String,
    pub continuation_mode: String,
    pub resume_class: Option<String>,
    pub delivery_state: Option<String>,
    pub last_failure_code: Option<String>,
    pub has_receipt: bool,
    pub outcome: Option<String>,
    pub outcome_failure_code: Option<String>,
    pub provider_receipt_id: Option<String>,
    pub verification_status: Option<String>,
}

const CONTINUATION_FACTS_SQL: &str = "SELECT a.id AS approval_id, a.status AS status, \
        a.continuation_mode AS continuation_mode, \
        a.metadata #>> '{card,resume_class}' AS resume_class, \
        d.state AS delivery_state, d.last_failure_code AS last_failure_code, \
        (r.receipt_id IS NOT NULL) AS has_receipt, \
        o.outcome AS outcome, o.failure_code AS outcome_failure_code, \
        o.provider_receipt_id AS provider_receipt_id, \
        o.verification_status AS verification_status \
    FROM approvals AS a \
    LEFT JOIN approval_delivery_outbox AS d ON d.approval_id = a.id \
    LEFT JOIN approval_continuation_receipts AS r ON r.delivery_id = d.delivery_id \
    LEFT JOIN approval_continuation_outcomes AS o ON o.receipt_id = r.receipt_id \
    WHERE a.org_id = $1 AND a.id = ANY($2)";

fn verification_to_proto(status: &str) -> i32 {
    match status {
        "verified_success" => pb::VerificationStatus::VerifiedSuccess as i32,
        "verified_failure" => pb::VerificationStatus::VerifiedFailure as i32,
        "partially_verified" => pb::VerificationStatus::PartiallyVerified as i32,
        _ => pb::VerificationStatus::Unknown as i32,
    }
}

/// Derive the continuation of one approval from its rows. `None` while the approval is not
/// granted: there is nothing to continue yet, and nothing is claimed.
///
/// Order matters: a recorded outcome beats a start receipt, which beats the approval's class,
/// which beats the outbox state. A row the creator never classified (created before this
/// slice) is read from the outbox alone, and a terminal `invalid_continuation` there means the
/// worker found nothing it could run, i.e. the approval is not resumable.
pub(crate) fn derive_continuation(facts: &ContinuationFacts) -> Option<pb::ApprovalContinuation> {
    use pb::ContinuationState as State;
    if facts.status != "granted" {
        return None;
    }
    let make = |state: State, failure_code: &str| pb::ApprovalContinuation {
        state: state as i32,
        failure_code: failure_code.to_owned(),
        verification: facts
            .verification_status
            .as_deref()
            .map_or(pb::VerificationStatus::Unspecified as i32, verification_to_proto),
        provider_receipt_id: facts.provider_receipt_id.clone().unwrap_or_default(),
    };
    if !facts.continuation_mode.is_empty() {
        return Some(make(State::AwaitingContinue, ""));
    }
    if let Some(outcome) = facts.outcome.as_deref() {
        return Some(if outcome == "completed" {
            make(State::Completed, "")
        } else {
            let code = facts
                .outcome_failure_code
                .as_deref()
                .unwrap_or(if outcome == "cancelled" {
                    "cancelled"
                } else {
                    "unknown_outcome"
                });
            make(State::Failed, code)
        });
    }
    if facts.has_receipt {
        return Some(make(State::Running, ""));
    }
    match facts.resume_class.as_deref() {
        Some(RESUME_LIVE_POLL) => return Some(make(State::Live, "")),
        Some(RESUME_NONE) => return Some(make(State::NotResumable, "")),
        _ => {}
    }
    match facts.delivery_state.as_deref() {
        Some("terminal") => {
            let code = facts.last_failure_code.as_deref().unwrap_or("unknown_outcome");
            Some(if code == "invalid_continuation" && facts.resume_class.is_none() {
                make(State::NotResumable, "")
            } else {
                make(State::Failed, code)
            })
        }
        Some("pending" | "processing") => Some(make(State::Queued, "")),
        _ => None,
    }
}

/// Read the facts of several approvals of one org.
pub(crate) async fn load_continuation_facts(
    pool: &Pool,
    org_id: &str,
    approval_ids: &[String],
) -> anyhow::Result<HashMap<String, ContinuationFacts>> {
    let rows = sqlx::query_as::<_, ContinuationFacts>(CONTINUATION_FACTS_SQL)
        .bind(org_id)
        .bind(approval_ids)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.approval_id.clone(), row))
        .collect())
}

/// Fill `continuation` on every granted approval of `approvals` (all of one org). A failed read
/// leaves the field unset and is logged: the card then states nothing about the continuation
/// instead of guessing, and the decision it follows is never failed for it.
pub(crate) async fn attach_continuations(
    pool: &Pool,
    org_id: &str,
    approvals: &mut [pb::Approval],
) {
    let granted = pb::ApprovalState::Granted as i32;
    let ids: Vec<String> = approvals
        .iter()
        .filter(|approval| approval.state == granted)
        .map(|approval| approval.id.clone())
        .collect();
    if ids.is_empty() {
        return;
    }
    match load_continuation_facts(pool, org_id, &ids).await {
        Ok(facts) => {
            for approval in approvals.iter_mut() {
                if let Some(facts) = facts.get(&approval.id) {
                    approval.continuation = derive_continuation(facts);
                }
            }
        }
        Err(error) => {
            warn!(error = %error, "approval continuation state unavailable");
        }
    }
}
