//! Typed `RUN_APPROVAL_DECIDED` events from the decide transaction (build plan WS13-D10, F-f10-34).
//!
//! A decided approval writes one content-free `RUN_APPROVAL_DECIDED` row on the run-event subject, so a run's log can
//! show each decision as it was made. The row is written inside the decide transaction, after the approval row and the
//! audit row (when audited) are written, so it exists exactly when the decision committed. A replay of a decision
//! changes nothing and writes nothing.
//!
//! Only a persistent run is announced. A zero-retention run's and a waived run's decisions are not written, and the
//! skip is counted. A class this build does not know is refused too, so the gate fails closed.
//!
//! The payload names ids and closed words only: the outcome, the scope, the decision source, the cause code, the
//! step-up provenance and the rule id. No reason, tool input, digest or other free text reaches it. The event rides the
//! run-event subject that `RUN_ATTENTION` already uses, so the broker grants and the outbox check admit it unchanged.
//! Consumers that do not map the type ignore it, as they ignore every run event they do not map.
//!
//! The switch is `SESSION_CORE_APPROVAL_EVENTS_ENABLED`, read by the decide handler. It is off by default, and only the
//! exact literal `true` turns it on. With it off the decide transaction writes exactly what it wrote before.

use chrono::{DateTime, Utc};
use mp_events::envelope::Envelope;
use mp_events::idempotency::derive_idempotency_hash;
use mp_events::subjects::run_event_subject;
use serde_json::json;
use sqlx::{Acquire, Postgres, Transaction};

use crate::learning_events::{run_resource_ref, PRODUCER};
use crate::retention_receipt::RetentionClass;

/// Envelope `event_type` of a decided approval.
pub(crate) const RUN_APPROVAL_DECIDED_EVENT_TYPE: &str = "RUN_APPROVAL_DECIDED";

/// The switch. Off by default; only the exact literal `true` turns it on.
pub(crate) const ENABLED_ENV: &str = "SESSION_CORE_APPROVAL_EVENTS_ENABLED";

/// The closed outcome words of the payload. The decide path writes the first three; `cancelled` is reserved in the
/// contract, and no approval path writes it today.
pub(crate) const OUTCOMES: [&str; 4] = ["approved", "denied", "expired", "cancelled"];

/// Whether the switch is on. Read by the decide handler on each decision.
pub(crate) fn enabled() -> bool {
    enabled_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The switch's value through the strict parser the durable-loop switches share: only the exact literal `true`, with no
/// trimming and no case folding.
fn enabled_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// The closed outcome word of a decision status the decide path writes, or `None` for any other status.
pub(crate) fn outcome_word(status: &str) -> Option<&'static str> {
    match status {
        "granted" => Some("approved"),
        "denied" => Some("denied"),
        "timed_out" => Some("expired"),
        _ => None,
    }
}

/// Whether a decided run's event may be written: only a persistent run. A zero-retention or waived class is refused, and
/// so is a missing or unknown class, so the gate fails closed.
pub(crate) fn enqueue_permitted(retention_class: Option<&str>) -> bool {
    retention_class.and_then(RetentionClass::from_storage) == Some(RetentionClass::Persistent)
}

/// The facts one decision's event names. Every word is a closed token and every id is opaque.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DecidedApproval<'a> {
    pub(crate) org_id: &'a str,
    /// The owner of the run. The event's user.
    pub(crate) owner_user_id: &'a str,
    pub(crate) run_id: &'a str,
    pub(crate) approval_id: &'a str,
    /// One of [`OUTCOMES`].
    pub(crate) outcome: &'a str,
    pub(crate) scope: &'a str,
    pub(crate) decision_source: &'a str,
    pub(crate) cause_code: &'a str,
    pub(crate) rule_id: &'a str,
    pub(crate) step_up_verified: bool,
}

/// The idempotency key of a decision's event: its approval and its outcome, on the run's resource.
fn idempotency_key(run_id: &str, approval_id: &str, outcome: &str) -> String {
    derive_idempotency_hash(
        PRODUCER,
        RUN_APPROVAL_DECIDED_EVENT_TYPE,
        &run_resource_ref(run_id),
        &format!("{approval_id}:{outcome}"),
    )
}

/// The outbox identity of a decision's event. It is derived from the approval and the outcome, so a replay of the same
/// decision names the same row and `ON CONFLICT (event_id) DO NOTHING` keeps one row.
pub(crate) fn decided_event_id(run_id: &str, approval_id: &str, outcome: &str) -> String {
    format!(
        "run_approval_decided:{PRODUCER}:{}",
        idempotency_key(run_id, approval_id, outcome)
    )
}

/// Build the `RUN_APPROVAL_DECIDED` envelope. Pure, so the wire shape is pinned by the golden test.
pub(crate) fn build_run_approval_decided_envelope(
    decided: &DecidedApproval<'_>,
    ts: DateTime<Utc>,
) -> Envelope {
    debug_assert!(OUTCOMES.contains(&decided.outcome), "a closed outcome word");
    Envelope {
        event_id: decided_event_id(decided.run_id, decided.approval_id, decided.outcome),
        event_type: RUN_APPROVAL_DECIDED_EVENT_TYPE.to_owned(),
        schema_version: 1,
        ts,
        producer: PRODUCER.to_owned(),
        correlation_id: decided.run_id.to_owned(),
        // The decision is what caused the event.
        causation_id: decided.approval_id.to_owned(),
        idempotency_key: idempotency_key(decided.run_id, decided.approval_id, decided.outcome),
        org_id: decided.org_id.to_owned(),
        user_id: decided.owner_user_id.to_owned(),
        resource_ref: run_resource_ref(decided.run_id),
        // Ids and closed tokens only.
        payload: json!({
            "run_id": decided.run_id,
            "approval_id": decided.approval_id,
            "outcome": decided.outcome,
            "scope": decided.scope,
            "decision_source": decided.decision_source,
            "cause_code": decided.cause_code,
            "step_up_verified": decided.step_up_verified,
            "rule_id": decided.rule_id,
        }),
        zdr: false,
        ..Default::default()
    }
}

/// Writes one decision's event on the decide transaction (F-f10-34). The insert runs inside a SAVEPOINT. A failure is
/// the insert's own, and it still propagates: the decide transaction then rolls back with it, so the row exists exactly
/// when the decision committed. Returns `Ok(true)` when the row was written and `Ok(false)` when it was already there.
pub(crate) async fn write_run_approval_decided(
    transaction: &mut Transaction<'_, Postgres>,
    decided: &DecidedApproval<'_>,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let envelope = build_run_approval_decided_envelope(decided, now);
    let payload =
        serde_json::to_value(&envelope).map_err(|error| sqlx::Error::Encode(error.into()))?;
    let mut savepoint = transaction.begin().await?;
    let inserted = sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(&envelope.event_id)
    .bind(run_event_subject(decided.run_id))
    .bind(&payload)
    .execute(&mut *savepoint)
    .await?
    .rows_affected();
    savepoint.commit().await?;
    Ok(inserted == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN: &str =
        include_str!("../../../../api/contracts/run-approval-decided-envelope.json");
    const GOLDEN_KEY_PLACEHOLDER: &str = "<idempotency hash: the hex tail of event_id>";

    fn at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-29T10:11:12Z")
            .expect("static timestamp")
            .with_timezone(&Utc)
    }

    fn decided(outcome: &'static str) -> DecidedApproval<'static> {
        DecidedApproval {
            org_id: "org-1",
            owner_user_id: "user-1",
            run_id: "run-abc",
            approval_id: "apr-1",
            outcome,
            scope: "once",
            decision_source: "owner",
            cause_code: "tool_permission",
            rule_id: "",
            step_up_verified: false,
        }
    }

    /// The golden wire body, compared as pretty JSON text. CRLF is normalised, because the file may be checked out with
    /// either line ending. The golden carries a placeholder for idempotency_key, because a bare 64-hex hash reads as a
    /// secret to the secret scan. The hash stays pinned through event_id, and the key must equal event_id's hex tail.
    #[test]
    fn golden_run_approval_decided_wire_body() {
        let envelope = build_run_approval_decided_envelope(&decided("approved"), at());
        let key = envelope.idempotency_key.clone();
        assert_eq!(key.len(), 64, "the key is a blake3 hex digest");
        assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(
            envelope.event_id.ends_with(&format!(":{key}")),
            "event_id must end with the idempotency key"
        );
        let body = serde_json::to_string_pretty(&envelope)
            .expect("envelope serializes")
            .replace(
                &format!("\"idempotency_key\": \"{key}\""),
                &format!("\"idempotency_key\": \"{GOLDEN_KEY_PLACEHOLDER}\""),
            );
        assert_eq!(body, GOLDEN.replace("\r\n", "\n").trim_end());
    }

    #[test]
    fn the_switch_is_the_exact_literal_true() {
        assert!(enabled_from(Some("true")));
        for value in [
            None,
            Some(""),
            Some("TRUE"),
            Some(" true"),
            Some("1"),
            Some("yes"),
            Some("false"),
        ] {
            assert!(!enabled_from(value), "{value:?} must not switch it on");
        }
    }

    #[test]
    fn a_decision_status_maps_to_one_closed_outcome_word() {
        assert_eq!(outcome_word("granted"), Some("approved"));
        assert_eq!(outcome_word("denied"), Some("denied"));
        assert_eq!(outcome_word("timed_out"), Some("expired"));
        assert_eq!(outcome_word("requested"), None);
        assert_eq!(outcome_word("cancelled"), None);
        for word in ["approved", "denied", "expired"] {
            assert!(OUTCOMES.contains(&word), "{word} is a closed outcome");
        }
    }

    #[test]
    fn only_a_persistent_run_is_announced() {
        assert!(enqueue_permitted(Some("persistent")));
        for class in [
            Some("zdr"),
            Some("zdr_waived"),
            Some(""),
            Some("PERSISTENT"),
            None,
        ] {
            assert!(!enqueue_permitted(class), "{class:?} must write nothing");
        }
    }

    #[test]
    fn the_event_id_is_stable_per_approval_and_outcome() {
        let first = build_run_approval_decided_envelope(&decided("approved"), at());
        let again = build_run_approval_decided_envelope(&decided("approved"), at());
        assert_eq!(
            first.event_id, again.event_id,
            "a replay names the same row"
        );
        assert!(first
            .event_id
            .starts_with("run_approval_decided:session-core:"));
        assert_ne!(
            first.event_id,
            build_run_approval_decided_envelope(&decided("denied"), at()).event_id
        );
        let mut other = decided("approved");
        other.approval_id = "apr-2";
        assert_ne!(
            first.event_id,
            build_run_approval_decided_envelope(&other, at()).event_id
        );
    }

    #[test]
    fn the_envelope_rides_the_run_subject_and_names_ids_and_closed_words_only() {
        let envelope = build_run_approval_decided_envelope(&decided("approved"), at());
        assert_eq!(envelope.event_type, "RUN_APPROVAL_DECIDED");
        assert_eq!(envelope.resource_ref, "run:run-abc");
        assert!(!envelope.zdr);
        assert_eq!(run_event_subject("run-abc"), "mp.v1.run.run-abc.event");
        let keys: Vec<&str> = envelope
            .payload
            .as_object()
            .expect("the payload is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "approval_id",
                "cause_code",
                "decision_source",
                "outcome",
                "rule_id",
                "run_id",
                "scope",
                "step_up_verified",
            ]
        );
    }

    #[test]
    fn the_run_event_reads_the_decision_the_projection_reads_back() {
        let metadata = serde_json::json!({
            "cause": {"code": "tool_permission", "rule_id": "rule-1"},
            "decision": {"source": "user", "scope": "thread", "step_up_verified": true},
        });
        let stored = crate::orchestration_store::stored_decision(&metadata);
        assert_eq!(stored.cause_code, "tool_permission");
        assert_eq!(stored.rule_id, "rule-1");
        assert_eq!(stored.scope, "thread");
        assert_eq!(stored.source, "user");
        assert!(stored.step_up_verified);
        // A row decided before the record existed reads as the one scope that existed, with no source and no step-up.
        let legacy = crate::orchestration_store::stored_decision(&serde_json::json!({}));
        assert_eq!(legacy.scope, "once");
        assert_eq!(legacy.source, "");
        assert!(!legacy.step_up_verified);
        assert_eq!(legacy.cause_code, "");
    }

    /// The row is event metadata of the run, like RUN_ATTENTION, so a thread erase must remove it. Its envelope nests
    /// run_id under payload and names no thread, so delete_thread_rows needs the nested run_id clause, and that delete
    /// must run while the thread's runs still exist.
    #[test]
    fn a_thread_erase_removes_the_run_decision_events() {
        let source = include_str!("grpc.rs");
        let body = &source[source
            .find("pub(crate) async fn delete_thread_rows(")
            .expect("delete_thread_rows")..];
        let outbox = body
            .find("\"DELETE FROM session_audit_outbox")
            .expect("the outbox delete");
        let clause = body
            .find("OR payload -> 'payload' ->> 'run_id' IN (SELECT id FROM runs WHERE thread_id = $1)")
            .expect("the nested run_id clause");
        let runs = body
            .find("\"DELETE FROM runs WHERE thread_id = $1\"")
            .expect("the runs delete");
        let next_query = outbox + body[outbox + 1..].find('"').expect("the outbox query ends") + 1;
        assert!(
            outbox < clause && clause < next_query,
            "the clause belongs to the outbox delete"
        );
        assert!(
            next_query < runs,
            "the outbox delete runs before the runs are gone"
        );
    }
}
