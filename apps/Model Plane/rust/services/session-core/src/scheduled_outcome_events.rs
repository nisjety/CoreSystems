//! The `SCHEDULED_RUN_OUTCOME` notice of a finished scheduled run (F-f05-05).
//!
//! capability-core's run watch turns this envelope into one notice to the person a schedule belongs to, the
//! schedule's `creator_subject_id`, so a finished scheduled run reaches its owner without anyone opening the chat.
//! The envelope names ids and one closed outcome word, never the answer. It is written in the same transaction as
//! the receipt and the fire row it describes, so the three agree or none of them is written.
//!
//! The row takes the path the run-lifecycle events take ([`crate::learning_events`]): `session_audit_outbox`, on the
//! run-event subject `mp.v1.run.<run_id>.event`, published by the drainer after its authority check
//! ([`crate::audit_publisher`]). The subject, the outbox CHECK and the broker grants already admit it, so this module
//! changes none of them.
//!
//! Only a persistent run is announced. A zero-retention or waived run is refused before any answer is stored, and
//! [`crate::attention_events::enqueue_permitted`] refuses it again here. A replayed result writes nothing: the write is
//! reached only from the claimed state, and the event id is derived from the run and the outcome, so
//! `ON CONFLICT (event_id) DO NOTHING` keeps one row.
//!
//! The emitter is off by default. Only the exact literal `true` in `SESSION_CORE_SCHEDULED_OUTCOME_EVENTS_ENABLED`
//! turns it on, and off, a result writes what it wrote before.
//!
//! An enqueue failure never fails the result. The insert runs in a SAVEPOINT, so a failure leaves the outer
//! transaction usable, and the failure is counted and logged with the run id only.

use chrono::{DateTime, Utc};
use mp_events::envelope::Envelope;
use mp_events::idempotency::derive_idempotency_hash;
use mp_events::subjects::run_event_subject;
use sqlx::{Acquire, Postgres, Transaction};

use crate::attention_events::enqueue_permitted;
use crate::learning_events::{run_resource_ref, PRODUCER};

/// Envelope `event_type` of a finished scheduled run. capability-core's run watch selects on it.
pub(crate) const SCHEDULED_RUN_OUTCOME_EVENT_TYPE: &str = "SCHEDULED_RUN_OUTCOME";

/// The switch of the emitter. Off by default: a result writes no outcome row unless this is the literal `true`.
pub(crate) const ENABLED_ENV: &str = "SESSION_CORE_SCHEDULED_OUTCOME_EVENTS_ENABLED";

/// Whether a finished scheduled run writes its outcome notice. Read once per result.
pub(crate) fn enabled() -> bool {
    enabled_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The switch's value through the one strict parser of the durable-loop switches: only the exact literal `true`,
/// with no trimming and no case folding.
fn enabled_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// The closed outcome word a run outcome announces, or `None` for an outcome that announces nothing here. The run
/// outcome is the one the result records: a completed step that withheld effects is `needs_review`, and an unknown
/// outcome reads as failed. The schedule-level pauses (`paused_expired`, `paused_failures`) are written by
/// capability-core's sweeper, not by a result, so they are not words here.
pub(crate) fn outcome_word(run_outcome: &str) -> Option<&'static str> {
    match run_outcome {
        "completed" => Some("completed"),
        "needs_review" => Some("needs_review"),
        "failed" | "unknown_outcome" => Some("failed"),
        _ => None,
    }
}

/// One finished scheduled run, as its notice names it: ids and the closed outcome word, never the answer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FinishedRun<'a> {
    pub(crate) org_id: &'a str,
    pub(crate) run_id: &'a str,
    /// The scheduled-step receipt that settled the run. It is what caused the notice.
    pub(crate) receipt_id: &'a str,
    pub(crate) thread_id: &'a str,
    pub(crate) schedule_id: &'a str,
    /// The run's owner: the system run owner every scheduled run carries.
    pub(crate) user_id: &'a str,
    /// The run's retention class, as stored (`persistent`, `zdr` or `zdr_waived`).
    pub(crate) retention_class: &'a str,
    /// The closed word from [`outcome_word`].
    pub(crate) outcome: &'static str,
    pub(crate) ts: DateTime<Utc>,
}

/// The key a run's outcome notice is deduplicated by: its run and its outcome word. A replay derives the same key.
pub(crate) fn idempotency_key(run_id: &str, outcome: &str) -> String {
    derive_idempotency_hash(
        PRODUCER,
        SCHEDULED_RUN_OUTCOME_EVENT_TYPE,
        &run_resource_ref(run_id),
        &format!("{run_id}:{outcome}"),
    )
}

/// The outbox identity of a run's outcome notice, derived from the run and the outcome, so a replay writes nothing.
pub(crate) fn event_id(run_id: &str, outcome: &str) -> String {
    format!(
        "scheduled_outcome:{PRODUCER}:{}",
        idempotency_key(run_id, outcome)
    )
}

/// Build the `SCHEDULED_RUN_OUTCOME` envelope. Pure, so the wire shape is pinned by tests. The payload carries the six
/// ids and closed tokens capability-core reads, and nothing else. The notice goes to the subject the payload names,
/// never to the envelope's user, which is the run's service owner.
pub(crate) fn build_envelope(run: &FinishedRun<'_>, subject_id: &str) -> Envelope {
    Envelope {
        event_id: event_id(run.run_id, run.outcome),
        event_type: SCHEDULED_RUN_OUTCOME_EVENT_TYPE.to_owned(),
        schema_version: 1,
        ts: run.ts,
        producer: PRODUCER.to_owned(),
        correlation_id: run.run_id.to_owned(),
        causation_id: run.receipt_id.to_owned(),
        idempotency_key: idempotency_key(run.run_id, run.outcome),
        org_id: run.org_id.to_owned(),
        user_id: run.user_id.to_owned(),
        resource_ref: run_resource_ref(run.run_id),
        payload: serde_json::json!({
            "org_id": run.org_id,
            "run_id": run.run_id,
            "thread_id": run.thread_id,
            "schedule_id": run.schedule_id,
            "subject_id": subject_id,
            "outcome": run.outcome,
        }),
        zdr: false,
        ..Default::default()
    }
}

/// How one run's outcome notice was settled. Only `Inserted` wrote a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Enqueued {
    Inserted,
    /// The row was already there: a replay of the same outcome.
    Replayed,
    /// The run is not persistent, so its notice is withheld.
    Withheld,
    /// The schedule is gone, or it names no subject to notify.
    NoSubject,
}

/// Writes the run's outcome notice on the caller's open transaction, inside a SAVEPOINT, when the run is persistent
/// and its schedule names a subject. The subject and the run's outbox posture are read in that same transaction, so
/// the notice names the subject the schedule has when the run finishes.
pub(crate) async fn enqueue_in_tx(
    transaction: &mut Transaction<'_, Postgres>,
    run: &FinishedRun<'_>,
) -> Result<Enqueued, sqlx::Error> {
    let mut savepoint = transaction.begin().await?;
    match enqueue_row(&mut savepoint, run).await {
        Ok(settled) => {
            savepoint.commit().await?;
            Ok(settled)
        }
        Err(error) => {
            // Release the savepoint so the outer transaction is usable again. The result still commits.
            drop(savepoint.rollback().await);
            Err(error)
        }
    }
}

async fn enqueue_row(
    savepoint: &mut Transaction<'_, Postgres>,
    run: &FinishedRun<'_>,
) -> Result<Enqueued, sqlx::Error> {
    // The posture gate. The result refuses a non-persistent run before this point, so this is the belt-and-braces
    // check, and it reads the run's outbox flag as well as its class.
    let zdr: Option<bool> =
        sqlx::query_scalar("SELECT zdr FROM managed_run_terminalization_outbox WHERE run_id = $1")
            .bind(run.run_id)
            .fetch_optional(&mut **savepoint)
            .await?;
    if !enqueue_permitted(zdr.unwrap_or(false), run.retention_class) {
        return Ok(Enqueued::Withheld);
    }
    let subject: Option<String> = sqlx::query_scalar(
        "SELECT creator_subject_id FROM cron_schedules WHERE id = $1 AND org_id = $2",
    )
    .bind(run.schedule_id)
    .bind(run.org_id)
    .fetch_optional(&mut **savepoint)
    .await?;
    let Some(subject_id) = subject.filter(|subject| !subject.trim().is_empty()) else {
        return Ok(Enqueued::NoSubject);
    };
    let envelope = build_envelope(run, &subject_id);
    let payload =
        serde_json::to_value(&envelope).map_err(|error| sqlx::Error::Encode(error.into()))?;
    let inserted = sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(&envelope.event_id)
    .bind(run_event_subject(run.run_id))
    .bind(&payload)
    .execute(&mut **savepoint)
    .await?
    .rows_affected();
    Ok(if inserted == 1 {
        Enqueued::Inserted
    } else {
        Enqueued::Replayed
    })
}

/// Writes the run's outcome notice and counts and logs what became of it. It never fails the result: the receipt and
/// the fire row are the authority, and a notice that could not be written is counted and logged with the run id only.
pub(crate) async fn record_outcome(
    transaction: &mut Transaction<'_, Postgres>,
    run: &FinishedRun<'_>,
) {
    match enqueue_in_tx(transaction, run).await {
        Ok(Enqueued::Inserted) => {
            metrics::counter!("mp_session_scheduled_outcome_events_enqueued_total").increment(1);
        }
        Ok(Enqueued::Withheld) => {
            metrics::counter!(
                "mp_session_scheduled_outcome_events_skipped_total",
                "reason" => "retention"
            )
            .increment(1);
        }
        Ok(Enqueued::NoSubject) => {
            metrics::counter!(
                "mp_session_scheduled_outcome_events_skipped_total",
                "reason" => "no_subject"
            )
            .increment(1);
        }
        Ok(Enqueued::Replayed) => {}
        Err(_) => {
            metrics::counter!("mp_session_scheduled_outcome_events_dropped_total").increment(1);
            tracing::warn!(
                run_id = %run.run_id,
                "SCHEDULED_RUN_OUTCOME event not enqueued; the scheduled result stands"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit_publisher::{classify_outbox_subject, OutboxRoute};

    fn at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-19T09:30:00Z")
            .expect("static timestamp")
            .with_timezone(&Utc)
    }

    fn finished(outcome: &'static str) -> FinishedRun<'static> {
        FinishedRun {
            org_id: "org-1",
            run_id: "run-abc",
            receipt_id: "receipt-1",
            thread_id: "thread-9",
            schedule_id: "schedule-7",
            user_id: "service:orchestrator-core",
            retention_class: "persistent",
            outcome,
            ts: at(),
        }
    }

    #[test]
    fn the_outcome_word_names_the_run_outcomes_a_result_records_and_nothing_else() {
        assert_eq!(outcome_word("completed"), Some("completed"));
        assert_eq!(outcome_word("needs_review"), Some("needs_review"));
        assert_eq!(outcome_word("failed"), Some("failed"));
        assert_eq!(
            outcome_word("unknown_outcome"),
            Some("failed"),
            "an unknown outcome reads as failed"
        );
        for other in [
            "paused_expired",
            "paused_failures",
            "",
            "COMPLETED",
            "running",
        ] {
            assert_eq!(
                outcome_word(other),
                None,
                "{other:?} announces nothing here"
            );
        }
    }

    #[test]
    fn the_switch_reads_only_the_literal_true() {
        assert!(enabled_from(Some("true")));
        for off in [
            None,
            Some(""),
            Some("1"),
            Some("on"),
            Some(" TRUE "),
            Some("yes"),
        ] {
            assert!(!enabled_from(off), "{off:?} must not start the emitter");
        }
        assert_eq!(ENABLED_ENV, "SESSION_CORE_SCHEDULED_OUTCOME_EVENTS_ENABLED");
    }

    #[test]
    fn the_envelope_names_ids_and_one_closed_token_and_carries_no_content() {
        let envelope = build_envelope(&finished("needs_review"), "subject-42");
        assert!(envelope.validate().is_ok(), "the envelope must validate");
        assert_eq!(envelope.event_type, "SCHEDULED_RUN_OUTCOME");
        assert_eq!(envelope.producer, "session-core");
        assert_eq!(envelope.resource_ref, "run:run-abc");
        assert_eq!(envelope.correlation_id, "run-abc");
        assert_eq!(envelope.causation_id, "receipt-1", "the receipt caused it");
        assert_eq!(envelope.user_id, "service:orchestrator-core");
        assert!(!envelope.zdr);
        let mut keys: Vec<&str> = envelope
            .payload
            .as_object()
            .expect("the payload is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "org_id",
                "outcome",
                "run_id",
                "schedule_id",
                "subject_id",
                "thread_id"
            ],
            "the payload carries ids and the closed outcome only"
        );
        assert_eq!(envelope.payload["outcome"], "needs_review");
        assert_eq!(envelope.payload["subject_id"], "subject-42");
        assert_eq!(envelope.payload["org_id"], "org-1");
        assert_eq!(envelope.payload["run_id"], "run-abc");
    }

    /// capability-core decodes the body with `json.Unmarshal` and reads the posture from the field, so `zdr` must be
    /// on the wire as an explicit false, never left out.
    #[test]
    fn the_wire_carries_an_explicit_persistent_posture() {
        let wire = serde_json::to_value(build_envelope(&finished("completed"), "subject-42"))
            .expect("the envelope serializes");
        assert_eq!(wire["zdr"], serde_json::Value::Bool(false));
        assert_eq!(wire["event_type"], "SCHEDULED_RUN_OUTCOME");
    }

    #[test]
    fn the_event_id_is_stable_per_run_and_outcome_and_ignores_the_clock() {
        let first = build_envelope(&finished("completed"), "subject-42");
        let mut later = finished("completed");
        later.ts += chrono::Duration::seconds(90);
        let replay = build_envelope(&later, "subject-42");
        assert_eq!(
            first.event_id, replay.event_id,
            "a replay keeps one identity"
        );
        assert_eq!(first.idempotency_key, replay.idempotency_key);
        assert!(first
            .event_id
            .starts_with("scheduled_outcome:session-core:"));
        assert!(
            first.event_id.len() <= 128,
            "the outbox event id is at most 128 characters"
        );
        assert_ne!(
            first.event_id,
            build_envelope(&finished("failed"), "subject-42").event_id,
            "another outcome of the same run is another notice"
        );
        assert_ne!(
            first.event_id,
            event_id("run-other", "completed"),
            "another run is another notice"
        );
    }

    #[test]
    fn the_notice_routes_on_the_run_event_subject_like_the_run_lifecycle_events() {
        let subject = run_event_subject("run-abc");
        assert_eq!(subject, "mp.v1.run.run-abc.event");
        assert_eq!(
            classify_outbox_subject(&subject),
            Some(OutboxRoute::RunEvent { run_id: "run-abc" })
        );
    }

    #[test]
    fn a_zero_retention_or_waived_run_is_withheld() {
        assert!(enqueue_permitted(false, "persistent"));
        assert!(!enqueue_permitted(true, "persistent"));
        assert!(!enqueue_permitted(false, "zdr"));
        assert!(!enqueue_permitted(false, "zdr_waived"));
    }
}
