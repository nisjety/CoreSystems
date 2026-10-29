//! `RUN_COMPLETED` run-lifecycle events for the skill-learning loop (§1.1).
//!
//! capability-core's `sessionreview` consumer subscribes the fixed core-NATS
//! subject `mp.v1.run.*.event` and turns every `RUN_COMPLETED` envelope into a
//! skill-learning review (fetch transcript -> LLM review -> `UpsertAgentSkill`),
//! and model-gateway injects the top learned skills into every chat turn. Both
//! ends were live; nothing published `RUN_COMPLETED` to that subject, so the
//! loop was dead in the middle: Session Core recorded the terminal event into
//! the Postgres `events` table only.
//!
//! # Delivery
//!
//! This opens no new NATS path. It reuses the existing durable
//! `session_audit_outbox` table plus the [`crate::audit_publisher`] drainer, so
//! inserting one row inside the terminalization transaction is sufficient to get
//! an at-least-once publish with retry/attempt tracking. The drainer routes the
//! row by its own `subject` (see `audit_publisher::classify_outbox_subject`).
//!
//! # Wire shape
//!
//! The message body is the canonical [`Envelope`], whose serde field names are
//! the same `snake_case` set as Go's `pkg/envelope.Envelope` json tags —
//! capability-core decodes with `json.Unmarshal(data, &env)` and then requires
//! `event_type == "RUN_COMPLETED"`, a non-empty `org_id`, and a run id parsed
//! out of `resource_ref` (`run/<id>` or `run:<id>`). A field-name drift here
//! fails *silently*, so the shared Rust type is used rather than a hand-rolled
//! `json!` literal: the two envelopes then stay in lockstep by construction.
//!
//! `Envelope` carries one field Go's struct does not (`zdr`); Go's
//! `json.Unmarshal` ignores unknown fields, so it is inert on the wire.
//!
//! # Never fail a user's turn
//!
//! The run is already durable before any of this runs. Enqueueing is therefore
//! wrapped in a SAVEPOINT and every failure is swallowed with a warning +
//! counter: the learning loop can lose an event, but a chat turn must never
//! fail, and terminalization must never roll back, because of it.

use chrono::{DateTime, Utc};
use mp_events::envelope::Envelope;
use mp_events::idempotency::derive_idempotency_hash;
use mp_events::subjects::run_event_subject;
use sqlx::{Acquire, Postgres, Transaction};

/// Envelope `event_type` capability-core's `sessionreview` trigger selects on.
pub(crate) const RUN_COMPLETED_EVENT_TYPE: &str = "RUN_COMPLETED";

/// Producer identity stamped on every Session Core event.
pub(crate) const PRODUCER: &str = "session-core";

/// Kill-switch env var. Reviewing every completed turn is the deliberate owner
/// choice (maximum learning surface); this is the cost lever that turns the
/// review spend off without a redeploy. Default ON.
pub(crate) const ENABLED_ENV: &str = "LEARNING_RUN_EVENTS_ENABLED";

/// `true` unless [`ENABLED_ENV`] is explicitly set to a falsey value.
///
/// Defaults ON so the loop is live by default; the operator lever is setting it
/// to `false`/`0`/`off`/`no`, which skips the outbox insert entirely (no row, no
/// publish, no review, no review spend).
pub(crate) fn learning_run_events_enabled() -> bool {
    enabled_from_env(std::env::var(ENABLED_ENV).ok())
}

/// Pure gate resolution so the default-ON contract is testable without mutating
/// process env (`unsafe_code` is forbidden workspace-wide). Mirrors the
/// `grpc::resolve_residency` pattern.
pub(crate) fn enabled_from_env(env_value: Option<String>) -> bool {
    !env_value.is_some_and(|value| is_falsey(&value))
}

/// Pure predicate for the kill-switch value, so the default-ON contract is
/// testable without mutating process env.
pub(crate) fn is_falsey(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "false" | "0" | "off" | "no"
    )
}

/// Canonical `resource_ref` for a run. capability-core's `ParseRunCompleted`
/// tolerates both `run/<id>` and `run:<id>`; `run:<id>` is what the rest of
/// Session Core already writes into the `events` table, so it is used here too.
pub(crate) fn run_resource_ref(run_id: &str) -> String {
    format!("run:{run_id}")
}

/// Stable idempotency key for a run's single learning event, using the same
/// `derive_idempotency_hash(producer, event_type, resource_ref, key)` convention
/// as every other Session Core event.
pub(crate) fn run_completed_idempotency_key(run_id: &str) -> String {
    derive_idempotency_hash(
        PRODUCER,
        RUN_COMPLETED_EVENT_TYPE,
        &run_resource_ref(run_id),
        &format!("{run_id}:learning"),
    )
}

/// Deterministic outbox/event identity for a run's learning event.
///
/// Deterministic on purpose: the outbox primary key is `event_id`, so
/// `ON CONFLICT (event_id) DO NOTHING` makes a replayed terminalization a no-op
/// instead of a second review. It is also published as `Nats-Msg-Id`, giving
/// JetStream-level dedup on the same identity.
pub(crate) fn run_completed_event_id(run_id: &str) -> String {
    format!(
        "run_completed:{PRODUCER}:{}",
        run_completed_idempotency_key(run_id)
    )
}

/// Run identity the envelope needs. Content-free: ids only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunCompletedContext {
    pub run_id: String,
    pub org_id: String,
    pub user_id: String,
    /// Thread whose transcript the reviewer replays. Best-effort for the
    /// consumer, but without it there is nothing to review.
    pub thread_id: String,
    /// The `RUN_COMPLETED` event row that this projection reports.
    pub causation_event_id: String,
    pub ts: DateTime<Utc>,
}

/// Build the `RUN_COMPLETED` envelope. Pure, so the wire shape is unit-testable
/// against capability-core's decode contract.
pub(crate) fn build_run_completed_envelope(context: &RunCompletedContext) -> Envelope {
    Envelope {
        event_id: run_completed_event_id(&context.run_id),
        event_type: RUN_COMPLETED_EVENT_TYPE.to_owned(),
        schema_version: 1,
        ts: context.ts,
        producer: PRODUCER.to_owned(),
        // The events table already uses run_id as the run's correlation id.
        correlation_id: context.run_id.clone(),
        causation_id: context.causation_event_id.clone(),
        idempotency_key: run_completed_idempotency_key(&context.run_id),
        org_id: context.org_id.clone(),
        user_id: context.user_id.clone(),
        resource_ref: run_resource_ref(&context.run_id),
        // Ids only. The envelope carries no transcript content; the consumer
        // fetches the transcript itself over gRPC under its own authority.
        payload: serde_json::json!({ "thread_id": context.thread_id }),
        zdr: false,
    }
}

/// Run identity + retention posture, read inside the terminalization tx.
#[derive(Debug, Clone, sqlx::FromRow)]
struct RunLearningIdentity {
    org_id: String,
    user_id: String,
    thread_id: Option<String>,
    /// Zero Data Retention. Only managed runs carry the flag, so an unmanaged
    /// legacy run coalesces to `false`.
    zdr: bool,
}

async fn load_run_identity(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<Option<RunLearningIdentity>, sqlx::Error> {
    sqlx::query_as::<_, RunLearningIdentity>(
        "SELECT r.org_id, r.user_id, r.thread_id,
                COALESCE(o.zdr, false) AS zdr
         FROM runs AS r
         LEFT JOIN managed_run_terminalization_outbox AS o ON o.run_id = r.id
         WHERE r.id = $1",
    )
    .bind(run_id)
    .fetch_optional(&mut **transaction)
    .await
}

/// Enqueue the run's `RUN_COMPLETED` learning event on the caller's open
/// transaction. Call this only for a genuinely completed run.
///
/// Infallible by construction: the run is already durable, so nothing here may
/// surface an error to the user or roll back terminalization. The insert runs
/// inside a SAVEPOINT so that even a constraint or serialization failure is
/// contained instead of poisoning the outer transaction, and the failure is
/// reported as a warning + `mp_session_learning_run_events_dropped_total`.
pub(crate) async fn enqueue_run_completed(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    causation_event_id: &str,
    now: DateTime<Utc>,
) {
    if !learning_run_events_enabled() {
        return;
    }
    match enqueue_in_savepoint(transaction, run_id, causation_event_id, now).await {
        Ok(Enqueued::Inserted) => {
            metrics::counter!("mp_session_learning_run_events_enqueued_total").increment(1);
        }
        Ok(Enqueued::SkippedZdr) => {
            metrics::counter!("mp_session_learning_run_events_skipped_zdr_total").increment(1);
            tracing::debug!(
                run_id,
                "ZDR run: RUN_COMPLETED learning event withheld (transcript must not be reviewed)"
            );
        }
        Ok(Enqueued::Replayed | Enqueued::NoRun) => {}
        Err(error) => {
            // The run is already terminal and durable. Learning is strictly
            // downstream, so it never gets to fail the user's turn.
            metrics::counter!("mp_session_learning_run_events_dropped_total").increment(1);
            tracing::warn!(
                %error,
                run_id,
                "RUN_COMPLETED learning event not enqueued; run remains terminal"
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Enqueued {
    Inserted,
    Replayed,
    SkippedZdr,
    NoRun,
}

async fn enqueue_in_savepoint(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    causation_event_id: &str,
    now: DateTime<Utc>,
) -> Result<Enqueued, sqlx::Error> {
    let mut savepoint = transaction.begin().await?;
    match enqueue_row(&mut savepoint, run_id, causation_event_id, now).await {
        Ok(outcome) => {
            savepoint.commit().await?;
            Ok(outcome)
        }
        Err(error) => {
            // Release the savepoint so the outer terminalization transaction is
            // usable again; the original cause is what gets reported.
            drop(savepoint.rollback().await);
            Err(error)
        }
    }
}

async fn enqueue_row(
    savepoint: &mut Transaction<'_, Postgres>,
    run_id: &str,
    causation_event_id: &str,
    now: DateTime<Utc>,
) -> Result<Enqueued, sqlx::Error> {
    let Some(identity) = load_run_identity(savepoint, run_id).await? else {
        return Ok(Enqueued::NoRun);
    };
    // ZDR COMPLIANCE: the envelope itself is ids-only, but emitting it causes
    // the consumer to replay the transcript, LLM-review it, and persist durable
    // skills derived from it. A Zero Data Retention run must not have its
    // transcript reviewed or distilled into a retained artifact, so it is never
    // announced. (ZDR runs also only ever have a redacted metadata thread, so
    // there is nothing to learn from in the first place.)
    if identity.zdr {
        return Ok(Enqueued::SkippedZdr);
    }
    let context = RunCompletedContext {
        run_id: run_id.to_owned(),
        org_id: identity.org_id,
        user_id: identity.user_id,
        thread_id: identity.thread_id.unwrap_or_default(),
        causation_event_id: causation_event_id.to_owned(),
        ts: now,
    };
    let envelope = build_run_completed_envelope(&context);
    let payload =
        serde_json::to_value(&envelope).map_err(|error| sqlx::Error::Encode(error.into()))?;
    let inserted = sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(&envelope.event_id)
    .bind(run_event_subject(run_id))
    .bind(&payload)
    .execute(&mut **savepoint)
    .await?
    .rows_affected();
    if inserted == 1 {
        Ok(Enqueued::Inserted)
    } else {
        Ok(Enqueued::Replayed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> RunCompletedContext {
        RunCompletedContext {
            run_id: "run-abc".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            thread_id: "thread-9".to_owned(),
            causation_event_id: "evt-terminal".to_owned(),
            ts: DateTime::parse_from_rfc3339("2026-07-29T10:11:12Z")
                .expect("static timestamp")
                .with_timezone(&Utc),
        }
    }

    /// Golden wire shape. Pinned in full because a drift in ANY field name or
    /// value fails silently: capability-core just decodes into a zero-valued
    /// field and drops the event, and nothing reports a skipped review.
    #[test]
    fn golden_run_completed_wire_body() {
        let body = serde_json::to_value(build_run_completed_envelope(&context()))
            .expect("envelope serializes");
        assert_eq!(
            body,
            serde_json::json!({
                "event_id": "run_completed:session-core:f4c3eade36ac92c46dafc7cd687621dce86e45f417ff4a30a69de553ae931137",
                "event_type": "RUN_COMPLETED",
                "schema_version": 1,
                "ts": "2026-07-29T10:11:12Z",
                "producer": "session-core",
                "correlation_id": "run-abc",
                "causation_id": "evt-terminal",
                "idempotency_key": "f4c3eade36ac92c46dafc7cd687621dce86e45f417ff4a30a69de553ae931137",
                "org_id": "org-1",
                "user_id": "user-1",
                "resource_ref": "run:run-abc",
                "payload": { "thread_id": "thread-9" },
                "zdr": false,
            })
        );
    }

    /// The envelope's json field names ARE the contract with Go's
    /// `pkg/envelope.Envelope` json tags. capability-core decodes with
    /// `json.Unmarshal`, so a rename here fails silently at runtime.
    #[test]
    fn envelope_field_names_match_go_envelope_json_tags() {
        let body = serde_json::to_value(build_run_completed_envelope(&context()))
            .expect("envelope serializes");
        let object = body.as_object().expect("envelope is a json object");
        for field in [
            "event_id",
            "event_type",
            "schema_version",
            "ts",
            "producer",
            "correlation_id",
            "causation_id",
            "idempotency_key",
            "org_id",
            "user_id",
            "resource_ref",
            "payload",
        ] {
            assert!(
                object.contains_key(field),
                "envelope is missing Go json tag {field}"
            );
        }
    }

    /// Every field Go's `Envelope.Validate()` requires must be non-empty, or a
    /// validating subscriber drops the event.
    #[test]
    fn envelope_satisfies_go_validate_required_fields() {
        let envelope = build_run_completed_envelope(&context());
        assert!(envelope.validate().is_ok(), "envelope must validate");
        assert_eq!(envelope.schema_version, 1);
        assert_eq!(envelope.producer, "session-core");
        assert!(!envelope.user_id.is_empty());
        assert!(!envelope.correlation_id.is_empty());
        assert!(!envelope.idempotency_key.is_empty());
    }

    /// The four things `ParseRunCompleted` actually reads.
    #[test]
    fn envelope_satisfies_capability_core_parse_run_completed() {
        let envelope = build_run_completed_envelope(&context());
        assert_eq!(envelope.event_type, "RUN_COMPLETED");
        assert!(!envelope.org_id.is_empty());
        assert_eq!(envelope.resource_ref, "run:run-abc");
        assert!(
            envelope
                .resource_ref
                .strip_prefix("run:")
                .is_some_and(|id| id == "run-abc"),
            "resource_ref must yield the run id"
        );
        assert_eq!(envelope.payload["thread_id"], serde_json::json!("thread-9"));
    }

    /// A `request/<id>` style ref (what model-gateway's stream envelope uses)
    /// yields no run id, so the consumer silently drops it. Guard against
    /// regressing into that shape.
    #[test]
    fn resource_ref_is_not_a_request_ref() {
        let envelope = build_run_completed_envelope(&context());
        assert!(!envelope.resource_ref.starts_with("request/"));
    }

    /// The payload must stay ids-only: no prompt, answer, or goal text.
    #[test]
    fn envelope_payload_carries_thread_id_only() {
        let envelope = build_run_completed_envelope(&context());
        let payload = envelope.payload.as_object().expect("payload is an object");
        assert_eq!(payload.len(), 1, "payload must carry thread_id only");
        assert!(payload.contains_key("thread_id"));
    }

    #[test]
    fn subject_is_the_fixed_run_event_subject() {
        assert_eq!(run_event_subject("run-abc"), "mp.v1.run.run-abc.event");
    }

    /// A replayed terminalization must derive the same identity so the outbox's
    /// `ON CONFLICT (event_id) DO NOTHING` collapses it to one publish.
    #[test]
    fn event_identity_is_deterministic_per_run() {
        assert_eq!(
            run_completed_event_id("run-abc"),
            run_completed_event_id("run-abc")
        );
        assert_eq!(
            build_run_completed_envelope(&context()).event_id,
            run_completed_event_id("run-abc")
        );
    }

    #[test]
    fn event_identity_is_distinct_per_run() {
        assert_ne!(
            run_completed_event_id("run-abc"),
            run_completed_event_id("run-abd")
        );
    }

    /// A second envelope built for the same run at a different time keeps the
    /// same dedup identity (only `ts` moves), so a retry cannot double-review.
    #[test]
    fn replayed_envelope_keeps_one_dedup_identity() {
        let first = build_run_completed_envelope(&context());
        let mut later = context();
        later.ts += chrono::Duration::seconds(90);
        later.causation_event_id = "evt-terminal-replay".to_owned();
        let second = build_run_completed_envelope(&later);
        assert_eq!(first.event_id, second.event_id);
        assert_eq!(first.idempotency_key, second.idempotency_key);
    }

    #[test]
    fn kill_switch_recognizes_falsey_values() {
        for value in ["false", "FALSE", " 0 ", "off", "No"] {
            assert!(is_falsey(value), "{value} must disable the learning loop");
        }
    }

    #[test]
    fn kill_switch_defaults_on() {
        for value in ["true", "1", "", "  ", "anything-else"] {
            assert!(
                !is_falsey(value),
                "{value} must leave the learning loop enabled"
            );
        }
    }

    /// Default ON: an unset (or blank) `LEARNING_RUN_EVENTS_ENABLED` must leave
    /// the learning loop live, and only an explicit falsey value disables it.
    #[test]
    fn kill_switch_gate_defaults_on_and_honours_explicit_off() {
        assert!(enabled_from_env(None), "unset must default to enabled");
        assert!(
            enabled_from_env(Some(String::new())),
            "blank must default to enabled"
        );
        assert!(enabled_from_env(Some("true".to_owned())));
        assert!(!enabled_from_env(Some("false".to_owned())));
        assert!(!enabled_from_env(Some("0".to_owned())));
        assert!(!enabled_from_env(Some("off".to_owned())));
    }

    /// The kill-switch must skip the outbox insert ENTIRELY, i.e. the gate is
    /// consulted before any SQL runs — otherwise "disabled" would still cost a
    /// query per completed turn. Asserted at source level, in the same style as
    /// `audit_publisher`'s transaction-ordering contract test.
    #[test]
    fn kill_switch_short_circuits_before_any_sql() {
        let source = include_str!("learning_events.rs");
        let gate = source
            .find("if !learning_run_events_enabled() {")
            .expect("enqueue_run_completed must consult the kill-switch");
        let enqueue = source
            .find("pub(crate) async fn enqueue_run_completed(")
            .expect("enqueue entry point must exist");
        let savepoint = source
            .find("match enqueue_in_savepoint(")
            .expect("enqueue must reach the savepoint path");
        assert!(
            enqueue < gate && gate < savepoint,
            "the kill-switch must be checked on entry, before the savepoint/SQL"
        );
    }

    /// ZDR runs must never be announced: the consumer would replay and
    /// LLM-review the transcript and persist durable skills from it.
    #[test]
    fn zdr_runs_are_never_announced() {
        let source = include_str!("learning_events.rs");
        let zdr_guard = source
            .find("if identity.zdr {")
            .expect("enqueue must skip ZDR runs");
        let insert = source
            .find("INSERT INTO session_audit_outbox")
            .expect("enqueue must insert an outbox row");
        assert!(
            zdr_guard < insert,
            "the ZDR check must precede the outbox insert"
        );
    }

    /// Both terminal paths must emit, or the high-volume surface goes unlearned:
    /// a plain chat turn terminalizes through the MANAGED path
    /// (`terminalization::apply_managed_terminal_outcome`, reached from
    /// model-gateway's `GatewayDirect` -> `RecordTerminalOutcome`), while legacy
    /// unmanaged runs terminalize through `grpc::record_run_terminal`.
    #[test]
    fn both_terminal_paths_enqueue_the_learning_event() {
        let managed = include_str!("terminalization.rs");
        assert!(
            managed.contains("learning_events::enqueue_run_completed("),
            "managed terminalization (the chat path) must announce RUN_COMPLETED"
        );
        assert!(
            managed.contains(
                "matches!(outcome, ManagedOutcome::Completed) && !reconciliation_required"
            ),
            "only a genuine, non-reconciled completion may be announced"
        );
        let legacy = include_str!("grpc.rs");
        assert!(
            legacy.contains("learning_events::enqueue_run_completed("),
            "legacy CompleteStep terminalization must announce RUN_COMPLETED"
        );
        assert!(
            legacy.contains("if terminal_event_type == \"RUN_COMPLETED\""),
            "the legacy path must not announce a failed run"
        );
    }
}
