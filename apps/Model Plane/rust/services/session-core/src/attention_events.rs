//! Typed `RUN_ATTENTION` events from the outbox (build plan WS16 S3).
//!
//! A sweep writes one `RUN_ATTENTION` outbox row per pending item that waits on its owner: an
//! approval, a proposed plan or a run question. The row takes the path the run-lifecycle events
//! already take ([`crate::learning_events`]): `session_audit_outbox`, on the run-event subject
//! `mp.v1.run.<run_id>.event`, published by the drainer after its authority check
//! ([`crate::audit_publisher`]). The subject, the outbox CHECK and the broker grants already admit
//! it, so this slice changes none of them.
//!
//! The envelope names ids and closed tokens only, never text. A consumer builds whatever it shows from
//! the tokens, so no tool name, argument or free text reaches a notification through this event.
//!
//! A thread goal that stopped (EP-7 S5) is announced too, while thread goals are on. Its item also names
//! its reason and its goal status, since its state alone does not say what waits: NEEDS_INPUT is a
//! question's state as well. A pending row names neither.
//!
//! Only a persistent run is announced. A zero-retention run's waiting items, and a waived run's
//! (DEC-32), are never written, so their state stays on the client.
//!
//! The sweep is the producer, not the call sites: a new approval kind or a new waiting table joins
//! the events through the one derivation in [`crate::attention`], with no edit to the code that
//! creates the row. A row written twice is harmless. The event id is derived from the item, and
//! `ON CONFLICT (event_id) DO NOTHING` keeps one row per item.

use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{AttentionReason, AttentionState};
use mp_events::envelope::Envelope;
use mp_events::idempotency::derive_idempotency_hash;
use mp_events::subjects::run_event_subject;
use sqlx::PgPool;

use crate::attention::{self, GoalFact, Pending, PendingApproval, ThreadFacts};
use crate::learning_events::{run_resource_ref, PRODUCER};
use crate::retention_receipt::RetentionClass;

/// Envelope `event_type` of a waiting item. A consumer that does not map it ignores it, as the
/// run-watch, session-review and task consumers already ignore every run event they do not map.
pub(crate) const RUN_ATTENTION_EVENT_TYPE: &str = "RUN_ATTENTION";

/// The switch of the sweep. Off by default: `main` starts nothing unless it is the literal `true`.
pub(crate) const ENABLED_ENV: &str = "SESSION_CORE_ATTENTION_EVENTS_ENABLED";

/// The sweep cadence in seconds.
const SWEEP_SECONDS_ENV: &str = "SESSION_CORE_ATTENTION_SWEEP_SECONDS";

/// The cadence when the variable is unset or does not name a positive whole number of seconds.
const DEFAULT_SWEEP_SECONDS: u64 = 5;

/// A waiting row created longer ago than this is not read. A sweep runs every few seconds, so a row is
/// announced by the first sweep after it is created. The window bounds how many rows one sweep reads.
const SWEEP_WINDOW_SECONDS: f64 = 600.0;

/// The origins whose waiting items are announced: the origins the attention list covers. A Space or a
/// support thread answers to its own surface. The matrix test pins this list equal to the list's.
const LISTED_ORIGINS: [&str; 2] = ["chat", "agent_run"];

/// Whether the sweep is switched on. Read once at boot.
pub(crate) fn enabled() -> bool {
    enabled_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The switch's value through the one strict parser of the durable-loop switches: `true`, in any case
/// and with surrounding whitespace, and nothing else.
fn enabled_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// The sweep cadence. A value that is not a positive whole number of seconds reads as the default, so a
/// typo can slow the sweep but never stop it.
fn sweep_seconds_from(value: Option<&str>) -> u64 {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_SWEEP_SECONDS)
}

/// One item that waits on its owner, reduced to what its envelope names. The run and the owner are the
/// row's own, and the state is the one the row waits for. A goal item's row is the goal, and its run is
/// the goal's last settled turn's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Waiting {
    pub(crate) org_id: String,
    pub(crate) user_id: String,
    pub(crate) thread_id: String,
    pub(crate) run_id: String,
    pub(crate) state: AttentionState,
    /// The row that waits: the approval, the proposed plan, the question or the goal.
    pub(crate) source_id: String,
    /// What a goal item names beyond its ids and state. `None` for a pending row, whose state says it all.
    pub(crate) goal: Option<GoalStop>,
}

/// What a goal item names beyond its ids and state (EP-7 S5). NEEDS_INPUT is also a question's state and
/// COMPLETED a finished run's, so the reason and the closed goal status say which goal waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoalStop {
    /// Always `ATTENTION_REASON_GOAL`.
    pub(crate) reason: AttentionReason,
    /// The closed status that stopped the goal: `met`, `budget_reached`, `turns_reached` or `blocked`.
    pub(crate) status: String,
}

/// The key a waiting item's event is deduplicated by: its run, its state and the row that waits.
pub(crate) fn run_attention_idempotency_key(item: &Waiting) -> String {
    derive_idempotency_hash(
        PRODUCER,
        RUN_ATTENTION_EVENT_TYPE,
        &run_resource_ref(&item.run_id),
        &format!(
            "{}:{}:{}",
            item.run_id,
            item.state.as_str_name(),
            item.source_id
        ),
    )
}

/// The outbox identity of a waiting item's event. It is derived from the item, so a later sweep that
/// finds the same row writes nothing.
pub(crate) fn run_attention_event_id(item: &Waiting) -> String {
    format!(
        "run_attention:{PRODUCER}:{}",
        run_attention_idempotency_key(item)
    )
}

/// Build the `RUN_ATTENTION` envelope. Pure, so the wire shape is pinned by the golden test.
pub(crate) fn build_run_attention_envelope(item: &Waiting, ts: DateTime<Utc>) -> Envelope {
    Envelope {
        event_id: run_attention_event_id(item),
        event_type: RUN_ATTENTION_EVENT_TYPE.to_owned(),
        schema_version: 1,
        ts,
        producer: PRODUCER.to_owned(),
        correlation_id: item.run_id.clone(),
        // The row that waits is what caused the event.
        causation_id: item.source_id.clone(),
        idempotency_key: run_attention_idempotency_key(item),
        org_id: item.org_id.clone(),
        user_id: item.user_id.clone(),
        resource_ref: run_resource_ref(&item.run_id),
        // Ids and closed tokens only.
        payload: payload_of(item),
        zdr: false,
        ..Default::default()
    }
}

/// The payload of a waiting item. A goal item adds its reason and its goal status, and a pending row keeps
/// the four keys it has always had. The JSON map keeps its keys sorted, so the golden text does not depend
/// on the order they are inserted in.
fn payload_of(item: &Waiting) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "run_id": item.run_id,
        "source_id": item.source_id,
        "state": item.state.as_str_name(),
        "thread_id": item.thread_id,
    });
    if let Some(goal) = &item.goal {
        payload["goal_status"] = goal.status.as_str().into();
        payload["reason"] = goal.reason.as_str_name().into();
    }
    payload
}

/// Whether a run's waiting items may be announced. Only a persistent run is: a zero-retention run's state
/// stays on the client, a waived run is not persistent (DEC-32), and a class this build does not know is
/// refused, so the gate fails closed. The run's zero-retention flag refuses on its own.
pub(crate) fn enqueue_permitted(zdr: bool, retention_class: &str) -> bool {
    !zdr && RetentionClass::from_storage(retention_class) == Some(RetentionClass::Persistent)
}

/// The state a pending approval puts its thread in, by the derivation the attention list uses (WS16 S1),
/// so the two cannot disagree about which approval waits. `None` when the approval no longer waits: its
/// kind raises nothing, or its run or its plan has moved on.
pub(crate) fn approval_state(approval: PendingApproval) -> Option<AttentionState> {
    let pending = Pending {
        approvals: vec![approval],
        ..Pending::default()
    };
    let facts = ThreadFacts {
        archived_at: None,
        latest_run_status: "",
        latest_run_at: None,
    };
    let derived = attention::derive(&facts, &pending);
    (derived.state != AttentionState::Idle).then_some(derived.state)
}

/// A pending approval row, with the facts its liveness reads.
#[derive(Debug, sqlx::FromRow)]
struct ApprovalRow {
    org_id: String,
    user_id: String,
    thread_id: String,
    run_id: String,
    source_id: String,
    kind: String,
    run_status: String,
    plan_status: Option<String>,
    requested_at: DateTime<Utc>,
    retention_class: String,
    zdr: bool,
}

/// A pending plan or run question row.
#[derive(Debug, sqlx::FromRow)]
struct WaitingRow {
    org_id: String,
    user_id: String,
    thread_id: String,
    run_id: String,
    source_id: String,
    retention_class: String,
    zdr: bool,
}

/// Approvals still requested and unexpired, of a listed thread, created inside the window. Whether each
/// one still waits is decided by [`approval_state`], the derivation the attention list uses.
const APPROVALS_SQL: &str =
    "SELECT r.org_id, r.user_id, r.thread_id, r.id AS run_id, a.id AS source_id,
        a.kind, r.status AS run_status, p.status AS plan_status, a.requested_at,
        r.retention_class, COALESCE(o.zdr, false) AS zdr
     FROM approvals a
     JOIN runs r ON r.id = a.run_id
     JOIN threads t ON t.id = r.thread_id
     LEFT JOIN plans p ON p.id = a.plan_id
     LEFT JOIN managed_run_terminalization_outbox o ON o.run_id = r.id
     WHERE a.status = 'requested'
       AND (a.expires_at IS NULL OR a.expires_at > now())
       AND a.requested_at > now() - make_interval(secs => $1)
       AND t.origin = ANY($2)";

/// Proposed plans of a listed thread, proposed inside the window by a run, that no live plan approval
/// already announces. When one does, the approval's own row is the announcement.
const PLANS_SQL: &str =
    "SELECT r.org_id, r.user_id, p.thread_id, r.id AS run_id, p.id AS source_id,
        r.retention_class, COALESCE(o.zdr, false) AS zdr
     FROM plans p
     JOIN runs r ON r.id = p.run_id
     JOIN threads t ON t.id = p.thread_id
     LEFT JOIN managed_run_terminalization_outbox o ON o.run_id = r.id
     WHERE p.status = 'proposed'
       AND p.updated_at > now() - make_interval(secs => $1)
       AND t.origin = ANY($2)
       AND NOT EXISTS (
           SELECT 1 FROM approvals a
           WHERE a.plan_id = p.id
             AND a.kind = 'plan'
             AND a.status = 'requested'
             AND (a.expires_at IS NULL OR a.expires_at > now())
             AND a.requested_at > now() - make_interval(secs => $1)
       )";

/// Pending run questions of a listed thread, asked inside the window and still unexpired.
const QUESTIONS_SQL: &str =
    "SELECT r.org_id, r.user_id, q.thread_id, r.id AS run_id, q.id AS source_id,
        r.retention_class, COALESCE(o.zdr, false) AS zdr
     FROM run_questions q
     JOIN runs r ON r.id = q.run_id
     JOIN threads t ON t.id = q.thread_id
     LEFT JOIN managed_run_terminalization_outbox o ON o.run_id = r.id
     WHERE q.state = 'requested'
       AND q.expires_at > now()
       AND q.requested_at > now() - make_interval(secs => $1)
       AND t.origin = ANY($2)";

/// A stopped goal of a listed thread, with the latest run of its thread and the run of its last settled turn.
#[derive(Debug, sqlx::FromRow)]
struct StoppedGoalRow {
    org_id: String,
    user_id: String,
    thread_id: String,
    run_id: String,
    source_id: String,
    status: String,
    since: DateTime<Utc>,
    latest_run_status: Option<String>,
    latest_run_at: Option<DateTime<Utc>>,
    retention_class: String,
    zdr: bool,
}

/// Goals of a listed thread that stopped inside the window (met, a cap reached or blocked). Only the thread's
/// latest goal counts, the one the listing reads, and a goal is left out while another row of its thread
/// waits, since the listing shows that row. Its run is the goal's last settled turn that started one, so a
/// goal with no such turn names no run and is not announced.
const GOALS_SQL: &str = "SELECT g.org_id, g.user_id, g.thread_id, last_turn.run_id,
        g.id AS source_id, g.status, COALESCE(g.closed_at, g.updated_at) AS since,
        latest.status AS latest_run_status, latest.updated_at AS latest_run_at,
        r.retention_class, COALESCE(o.zdr, false) AS zdr
     FROM thread_goals g
     JOIN threads t ON t.id = g.thread_id
     JOIN LATERAL (
        SELECT s.run_id
        FROM thread_goal_turns s
        WHERE s.goal_id = g.id AND s.settled_at IS NOT NULL AND s.run_id IS NOT NULL
        ORDER BY s.turn_no DESC
        LIMIT 1
     ) last_turn ON TRUE
     JOIN runs r ON r.id = last_turn.run_id
     LEFT JOIN managed_run_terminalization_outbox o ON o.run_id = r.id
     LEFT JOIN LATERAL (
        SELECT status, updated_at
        FROM runs
        WHERE thread_id = g.thread_id
        ORDER BY updated_at DESC, created_at DESC, id DESC
        LIMIT 1
     ) latest ON TRUE
     WHERE g.status IN ('met', 'budget_reached', 'turns_reached', 'blocked')
       AND GREATEST(g.closed_at, g.updated_at) > now() - make_interval(secs => $1)
       AND t.origin = ANY($2)
       AND NOT EXISTS (
           SELECT 1 FROM thread_goals newer
           WHERE newer.thread_id = g.thread_id
             AND (newer.updated_at, newer.created_at, newer.id) > (g.updated_at, g.created_at, g.id)
       )
       AND NOT EXISTS (
           SELECT 1 FROM approvals a
           JOIN runs ar ON ar.id = a.run_id
           LEFT JOIN plans ap ON ap.id = a.plan_id
           WHERE ar.thread_id = g.thread_id
             AND a.status = 'requested'
             AND (a.expires_at IS NULL OR a.expires_at > now())
             AND ((a.kind = 'plan' AND ap.status = 'proposed')
                  OR (a.kind IN ('tool_call', 'permission', 'destructive', 'cost')
                      AND ar.status NOT IN ('completed', 'failed', 'cancelled')))
       )
       AND NOT EXISTS (
           SELECT 1 FROM plans p
           WHERE p.thread_id = g.thread_id AND p.status = 'proposed'
       )
       AND NOT EXISTS (
           SELECT 1 FROM run_questions q
           WHERE q.thread_id = g.thread_id AND q.state = 'requested' AND q.expires_at > now()
       )";

/// The state and reason a stopped goal raises, or `None` when the listing would not show it. The thread's
/// latest run must not be working, and a met goal must not sit behind a failed run. The derivation makes
/// that call, so the sweep and the listing agree on which goal stands.
fn goal_stop(row: &StoppedGoalRow) -> Option<(AttentionState, AttentionReason)> {
    let (state, reason) = attention::goal_attention(&row.status)?;
    let pending = Pending {
        goal: Some(GoalFact {
            status: row.status.clone(),
            since: row.since,
        }),
        ..Pending::default()
    };
    let facts = ThreadFacts {
        archived_at: None,
        latest_run_status: row.latest_run_status.as_deref().unwrap_or(""),
        latest_run_at: row.latest_run_at,
    };
    let derived = attention::derive(&facts, &pending);
    (derived.state == state && derived.reason == reason).then_some((state, reason))
}

/// The stopped goals of the listed threads as waiting items, while thread goals are on (EP-7 S5). A goal the
/// zero-retention posture refuses is counted into `withheld`, as a refused pending row is.
async fn goal_items(
    pool: &PgPool,
    origins: &[String],
    withheld: &mut u64,
) -> Result<Vec<Waiting>, sqlx::Error> {
    if !attention::goals_enabled() {
        return Ok(Vec::new());
    }
    let rows: Vec<StoppedGoalRow> = sqlx::query_as(GOALS_SQL)
        .bind(SWEEP_WINDOW_SECONDS)
        .bind(origins)
        .fetch_all(pool)
        .await?;
    let mut items = Vec::new();
    for row in rows {
        let Some((state, reason)) = goal_stop(&row) else {
            continue;
        };
        if !enqueue_permitted(row.zdr, &row.retention_class) {
            *withheld += 1;
            continue;
        }
        items.push(Waiting {
            org_id: row.org_id,
            user_id: row.user_id,
            thread_id: row.thread_id,
            run_id: row.run_id,
            state,
            source_id: row.source_id,
            goal: Some(GoalStop {
                reason,
                status: row.status,
            }),
        });
    }
    Ok(items)
}

/// One sweep: reads the pending items of the three tables and the stopped goals, gates them on their run's
/// posture, and writes one outbox row for each item that has none yet. Returns how many rows this sweep
/// wrote.
///
/// Each row is its own statement, so a refused row is counted, left for the next sweep, and never holds
/// back the others. A failed read ends the sweep, and the next one starts again from the tables.
pub(crate) async fn sweep_once(pool: &PgPool, now: DateTime<Utc>) -> Result<usize, sqlx::Error> {
    let origins: Vec<String> = LISTED_ORIGINS
        .iter()
        .map(|origin| (*origin).to_owned())
        .collect();
    let approvals: Vec<ApprovalRow> = sqlx::query_as(APPROVALS_SQL)
        .bind(SWEEP_WINDOW_SECONDS)
        .bind(&origins)
        .fetch_all(pool)
        .await?;
    let plans: Vec<WaitingRow> = sqlx::query_as(PLANS_SQL)
        .bind(SWEEP_WINDOW_SECONDS)
        .bind(&origins)
        .fetch_all(pool)
        .await?;
    let questions: Vec<WaitingRow> = sqlx::query_as(QUESTIONS_SQL)
        .bind(SWEEP_WINDOW_SECONDS)
        .bind(&origins)
        .fetch_all(pool)
        .await?;

    // Counted per sweep: a waiting row is seen by every sweep until it stops waiting.
    let mut withheld: u64 = 0;
    let mut items = Vec::new();
    for row in approvals {
        let Some(state) = approval_state(PendingApproval {
            kind: row.kind,
            run_status: row.run_status,
            plan_status: row.plan_status,
            requested_at: row.requested_at,
        }) else {
            continue;
        };
        // Only a row that waits is withheld, so the count is of waiting items and not of every approval.
        if !enqueue_permitted(row.zdr, &row.retention_class) {
            withheld += 1;
            continue;
        }
        items.push(Waiting {
            org_id: row.org_id,
            user_id: row.user_id,
            thread_id: row.thread_id,
            run_id: row.run_id,
            state,
            source_id: row.source_id,
            goal: None,
        });
    }
    for (rows, state) in [
        (plans, AttentionState::PlanReady),
        (questions, AttentionState::NeedsInput),
    ] {
        for row in rows {
            if !enqueue_permitted(row.zdr, &row.retention_class) {
                withheld += 1;
                continue;
            }
            items.push(Waiting {
                org_id: row.org_id,
                user_id: row.user_id,
                thread_id: row.thread_id,
                run_id: row.run_id,
                state,
                source_id: row.source_id,
                goal: None,
            });
        }
    }
    items.extend(goal_items(pool, &origins, &mut withheld).await?);
    if withheld > 0 {
        metrics::counter!("mp_session_attention_events_skipped_zdr_total").increment(withheld);
    }

    let mut enqueued = 0;
    for item in &items {
        match enqueue(pool, item, now).await {
            Ok(true) => {
                metrics::counter!("mp_session_attention_events_enqueued_total").increment(1);
                enqueued += 1;
            }
            Ok(false) => {}
            Err(error) => {
                metrics::counter!("mp_session_attention_events_dropped_total").increment(1);
                tracing::warn!(%error, run_id = %item.run_id, "RUN_ATTENTION event not enqueued; the next sweep retries it");
            }
        }
    }
    Ok(enqueued)
}

/// Writes one waiting item's row. Returns `false` when the row is already there, which is what a later
/// sweep finds.
async fn enqueue(pool: &PgPool, item: &Waiting, now: DateTime<Utc>) -> Result<bool, sqlx::Error> {
    let envelope = build_run_attention_envelope(item, now);
    let payload =
        serde_json::to_value(&envelope).map_err(|error| sqlx::Error::Encode(error.into()))?;
    let inserted = sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(&envelope.event_id)
    .bind(run_event_subject(&item.run_id))
    .bind(&payload)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(inserted == 1)
}

/// The sweep loop. It runs under `supervise_background`, and a failed pass is logged before the next one.
pub(crate) async fn run(pool: PgPool) -> Result<()> {
    let interval = Duration::from_secs(sweep_seconds_from(
        std::env::var(SWEEP_SECONDS_ENV).ok().as_deref(),
    ));
    loop {
        tokio::time::sleep(interval).await;
        match sweep_once(&pool, Utc::now()).await {
            Ok(0) => {}
            Ok(enqueued) => tracing::info!(enqueued, "enqueued RUN_ATTENTION events"),
            Err(error) => tracing::warn!(%error, "attention event sweep failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit_publisher::{classify_outbox_subject, OutboxRoute};

    const GOLDEN: &str = include_str!("../../../../api/contracts/run-attention-envelope.json");
    const GOLDEN_KEY_PLACEHOLDER: &str = "<idempotency hash: the hex tail of event_id>";

    fn at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-29T10:11:12Z")
            .expect("static timestamp")
            .with_timezone(&Utc)
    }

    fn waiting(state: AttentionState, source: &str) -> Waiting {
        Waiting {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            thread_id: "thread-9".to_owned(),
            run_id: "run-abc".to_owned(),
            state,
            source_id: source.to_owned(),
            goal: None,
        }
    }

    fn approval(kind: &str, run_status: &str, plan_status: Option<&str>) -> PendingApproval {
        PendingApproval {
            kind: kind.to_owned(),
            run_status: run_status.to_owned(),
            plan_status: plan_status.map(str::to_owned),
            requested_at: at(),
        }
    }

    /// The golden wire body, compared as pretty JSON text. A drift in a field name or value fails silently
    /// at the consumer, which decodes into zero values, so the whole text is pinned. CRLF is normalised,
    /// because the file may be checked out with either line ending. The golden carries a placeholder for
    /// idempotency_key (a bare 64-hex hash reads as a secret to the secret scan); the hash itself is pinned
    /// through event_id, and the key must equal event_id's hex tail.
    #[test]
    fn golden_run_attention_wire_body() {
        let envelope =
            build_run_attention_envelope(&waiting(AttentionState::NeedsApproval, "apr-1"), at());
        let key = envelope.idempotency_key.clone();
        assert_eq!(key.len(), 64, "the key is a sha256 hex digest");
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
    fn the_envelope_names_ids_and_one_closed_state_and_no_text() {
        let envelope =
            build_run_attention_envelope(&waiting(AttentionState::NeedsInput, "q_1"), at());
        assert!(envelope.validate().is_ok(), "the envelope must validate");
        assert_eq!(envelope.event_type, "RUN_ATTENTION");
        assert_eq!(envelope.resource_ref, "run:run-abc");
        assert!(!envelope.zdr);
        let keys: Vec<&str> = envelope
            .payload
            .as_object()
            .expect("the payload is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["run_id", "source_id", "state", "thread_id"]);
    }

    #[test]
    fn the_envelope_decodes_from_its_own_wire_bytes() {
        let envelope =
            build_run_attention_envelope(&waiting(AttentionState::PlanReady, "plan_1"), at());
        let bytes = envelope.to_json_bytes().expect("serializes");
        assert_eq!(
            Envelope::from_json_bytes(&bytes).expect("decodes"),
            envelope
        );
    }

    #[test]
    fn the_event_id_is_derived_from_run_state_and_row_so_a_later_sweep_writes_nothing() {
        let approval = waiting(AttentionState::NeedsApproval, "apr-1");
        let id = run_attention_event_id(&approval);
        assert_eq!(id, run_attention_event_id(&approval));
        assert_ne!(
            id,
            run_attention_event_id(&waiting(AttentionState::NeedsApproval, "apr-2"))
        );
        assert_ne!(
            id,
            run_attention_event_id(&waiting(AttentionState::NeedsInput, "apr-1"))
        );
        assert!(id.starts_with("run_attention:session-core:"));
        assert!(
            id.len() <= 128,
            "the outbox event id is at most 128 characters"
        );
    }

    #[test]
    fn a_tool_or_cost_approval_waits_as_needs_approval_only_while_its_run_is_live() {
        for kind in ["tool_call", "permission", "destructive", "cost"] {
            for live in ["running", "queued", "awaiting_approval"] {
                assert_eq!(
                    approval_state(approval(kind, live, None)),
                    Some(AttentionState::NeedsApproval),
                    "{kind} of a {live} run waits"
                );
            }
            for ended in ["completed", "failed", "cancelled"] {
                assert_eq!(
                    approval_state(approval(kind, ended, None)),
                    None,
                    "{kind} of a {ended} run is moot"
                );
            }
        }
    }

    #[test]
    fn a_plan_approval_waits_as_plan_ready_only_while_its_plan_is_proposed() {
        assert_eq!(
            approval_state(approval("plan", "running", Some("proposed"))),
            Some(AttentionState::PlanReady)
        );
        // A plan outlives the run that proposed it.
        assert_eq!(
            approval_state(approval("plan", "completed", Some("proposed"))),
            Some(AttentionState::PlanReady)
        );
        assert_eq!(
            approval_state(approval("plan", "running", Some("superseded"))),
            None
        );
        assert_eq!(approval_state(approval("plan", "running", None)), None);
    }

    #[test]
    fn an_approval_kind_the_derivation_does_not_name_raises_no_item() {
        assert_eq!(approval_state(approval("mystery", "running", None)), None);
    }

    #[test]
    fn only_a_persistent_run_is_announced() {
        assert!(enqueue_permitted(false, "persistent"));
        assert!(
            !enqueue_permitted(true, "persistent"),
            "the zero-retention flag refuses on its own"
        );
        assert!(!enqueue_permitted(false, "zdr"));
        assert!(!enqueue_permitted(false, "zdr_waived"));
        assert!(!enqueue_permitted(false, "waived"));
        assert!(!enqueue_permitted(false, ""));
    }

    #[test]
    fn the_switch_reads_only_the_literal_true() {
        assert!(enabled_from(Some("true")));
        assert!(enabled_from(Some(" TRUE ")));
        for off in [None, Some("1"), Some("on"), Some("yes"), Some("")] {
            assert!(!enabled_from(off), "{off:?} must not start the sweep");
        }
    }

    #[test]
    fn the_cadence_is_five_seconds_unless_told_otherwise_and_junk_cannot_stop_it() {
        assert_eq!(sweep_seconds_from(None), 5);
        assert_eq!(sweep_seconds_from(Some("2")), 2);
        assert_eq!(sweep_seconds_from(Some(" 30 ")), 30);
        for junk in ["", "0", "-3", "soon", "1.5"] {
            assert_eq!(sweep_seconds_from(Some(junk)), 5, "{junk:?}");
        }
    }

    #[test]
    fn the_switch_and_the_cadence_are_named_for_this_slice() {
        assert_eq!(ENABLED_ENV, "SESSION_CORE_ATTENTION_EVENTS_ENABLED");
        assert_eq!(SWEEP_SECONDS_ENV, "SESSION_CORE_ATTENTION_SWEEP_SECONDS");
        assert_eq!(RUN_ATTENTION_EVENT_TYPE, "RUN_ATTENTION");
    }

    /// The drainer's route check is private to `audit_publisher`, so this pins its conditions: the subject
    /// classifies as a run event of this run, and the envelope names the same run, the same row, the
    /// producer and a non-empty type and tenant. A row that meets them passes `check_outbox_authority`.
    #[test]
    fn the_row_takes_the_run_event_route_and_meets_the_drainer_conditions() {
        let item = waiting(AttentionState::PlanReady, "plan_1");
        let envelope = build_run_attention_envelope(&item, at());
        let subject = run_event_subject(&item.run_id);
        assert_eq!(subject, "mp.v1.run.run-abc.event");
        assert_eq!(
            classify_outbox_subject(&subject),
            Some(OutboxRoute::RunEvent { run_id: "run-abc" })
        );
        let wire = serde_json::to_value(&envelope).expect("serializes");
        assert_eq!(wire["event_id"].as_str(), Some(envelope.event_id.as_str()));
        assert_eq!(wire["producer"].as_str(), Some("session-core"));
        assert_eq!(wire["resource_ref"].as_str(), Some("run:run-abc"));
        assert!(!wire["event_type"].as_str().unwrap_or_default().is_empty());
        assert!(!wire["org_id"].as_str().unwrap_or_default().is_empty());
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::loop_checkpoint::test_support::scratch_pool;
    use mp_ids::new_ulid;

    async fn seed_thread(pool: &PgPool, org: &str, user: &str, thread: &str) {
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(thread)
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .expect("seed thread");
    }

    async fn seed_run(
        pool: &PgPool,
        org: &str,
        user: &str,
        thread: &str,
        run: &str,
        retention: &str,
    ) {
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, retention_class)
             VALUES ($1, $2, 'g', $3, $4, 'running', $5)",
        )
        .bind(run)
        .bind(thread)
        .bind(org)
        .bind(user)
        .bind(retention)
        .execute(pool)
        .await
        .expect("seed run");
    }

    async fn seed_approval(
        pool: &PgPool,
        org: &str,
        user: &str,
        run: &str,
        id: &str,
        kind: &str,
        plan: Option<&str>,
        status: &str,
    ) {
        sqlx::query(
            "INSERT INTO approvals (id, run_id, plan_id, kind, status, org_id, user_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id)
        .bind(run)
        .bind(plan)
        .bind(kind)
        .bind(status)
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .expect("seed approval");
    }

    async fn seed_plan(pool: &PgPool, org: &str, user: &str, thread: &str, run: &str, id: &str) {
        sqlx::query(
            "INSERT INTO plans (id, thread_id, run_id, status, org_id, user_id)
             VALUES ($1, $2, $3, 'proposed', $4, $5)",
        )
        .bind(id)
        .bind(thread)
        .bind(run)
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .expect("seed plan");
    }

    async fn seed_question(
        pool: &PgPool,
        org: &str,
        user: &str,
        thread: &str,
        run: &str,
        id: &str,
    ) {
        sqlx::query(
            "INSERT INTO run_questions (id, run_id, thread_id, org_id, user_id, spec, expires_at)
             VALUES ($1, $2, $3, $4, $5, '{}'::jsonb, now() + interval '1 hour')",
        )
        .bind(id)
        .bind(run)
        .bind(thread)
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .expect("seed question");
    }

    /// The envelopes announced for one run, decoded as a consumer decodes them.
    async fn announced(pool: &PgPool, run: &str) -> Vec<Envelope> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT payload::text FROM session_audit_outbox WHERE subject = $1 ORDER BY event_id",
        )
        .bind(run_event_subject(run))
        .fetch_all(pool)
        .await
        .expect("read the outbox");
        rows.iter()
            .map(|(text,)| {
                Envelope::from_json_bytes(text.as_bytes()).expect("a row decodes as an envelope")
            })
            .collect()
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_pending_approval_is_announced_once_across_two_sweeps() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("ev-{suffix}"), "ev-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        let approval = format!("apr_{suffix}");
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "persistent").await;
        seed_approval(
            &pool,
            &org,
            user,
            &run,
            &approval,
            "tool_call",
            None,
            "requested",
        )
        .await;

        sweep_once(&pool, Utc::now()).await.expect("first sweep");
        sweep_once(&pool, Utc::now()).await.expect("second sweep");

        let rows = announced(&pool, &run).await;
        assert_eq!(
            rows.len(),
            1,
            "one row per pending approval, however many sweeps find it"
        );
        assert_eq!(rows[0].event_type, RUN_ATTENTION_EVENT_TYPE);
        assert_eq!(rows[0].payload["state"], "ATTENTION_STATE_NEEDS_APPROVAL");
        assert_eq!(rows[0].payload["source_id"], approval.as_str());
        assert_eq!(rows[0].payload["thread_id"], thread.as_str());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_decided_approval_is_not_announced() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("ev-{suffix}"), "ev-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "persistent").await;
        seed_approval(
            &pool,
            &org,
            user,
            &run,
            &format!("apr_{suffix}"),
            "tool_call",
            None,
            "approved",
        )
        .await;

        sweep_once(&pool, Utc::now()).await.expect("sweep");

        assert!(announced(&pool, &run).await.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_zero_retention_runs_pending_approval_is_not_announced() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("ev-{suffix}"), "ev-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "zdr").await;
        seed_approval(
            &pool,
            &org,
            user,
            &run,
            &format!("apr_{suffix}"),
            "tool_call",
            None,
            "requested",
        )
        .await;

        sweep_once(&pool, Utc::now()).await.expect("sweep");

        assert!(announced(&pool, &run).await.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_pending_question_is_announced_as_needs_input() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("ev-{suffix}"), "ev-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        let question = format!("q_{suffix}");
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "persistent").await;
        seed_question(&pool, &org, user, &thread, &run, &question).await;

        sweep_once(&pool, Utc::now()).await.expect("sweep");

        let rows = announced(&pool, &run).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].payload["state"], "ATTENTION_STATE_NEEDS_INPUT");
        assert_eq!(rows[0].payload["source_id"], question.as_str());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_proposed_plan_without_an_approval_is_announced_as_plan_ready() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("ev-{suffix}"), "ev-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        let plan = format!("plan_{suffix}");
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "persistent").await;
        seed_plan(&pool, &org, user, &thread, &run, &plan).await;

        sweep_once(&pool, Utc::now()).await.expect("sweep");

        let rows = announced(&pool, &run).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].payload["state"], "ATTENTION_STATE_PLAN_READY");
        assert_eq!(rows[0].payload["source_id"], plan.as_str());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_proposed_plan_with_its_live_plan_approval_is_announced_once_by_the_approval() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("ev-{suffix}"), "ev-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        let (plan, approval) = (format!("plan_{suffix}"), format!("apr_{suffix}"));
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "persistent").await;
        seed_plan(&pool, &org, user, &thread, &run, &plan).await;
        seed_approval(
            &pool,
            &org,
            user,
            &run,
            &approval,
            "plan",
            Some(plan.as_str()),
            "requested",
        )
        .await;

        sweep_once(&pool, Utc::now()).await.expect("sweep");

        let rows = announced(&pool, &run).await;
        assert_eq!(
            rows.len(),
            1,
            "the plan and its approval are one waiting item"
        );
        assert_eq!(rows[0].payload["state"], "ATTENTION_STATE_PLAN_READY");
        assert_eq!(rows[0].payload["source_id"], approval.as_str());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn each_announcement_names_its_own_run_owner_and_tenant() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org_a, user_a) = (format!("ev-a-{suffix}"), "ev-user-a");
        let (org_b, user_b) = (format!("ev-b-{suffix}"), "ev-user-b");
        let (thread_a, run_a) = (format!("thr_a_{suffix}"), format!("run_a_{suffix}"));
        let (thread_b, run_b) = (format!("thr_b_{suffix}"), format!("run_b_{suffix}"));
        seed_thread(&pool, &org_a, user_a, &thread_a).await;
        seed_run(&pool, &org_a, user_a, &thread_a, &run_a, "persistent").await;
        seed_approval(
            &pool,
            &org_a,
            user_a,
            &run_a,
            &format!("apr_a_{suffix}"),
            "cost",
            None,
            "requested",
        )
        .await;
        seed_thread(&pool, &org_b, user_b, &thread_b).await;
        seed_run(&pool, &org_b, user_b, &thread_b, &run_b, "persistent").await;
        seed_approval(
            &pool,
            &org_b,
            user_b,
            &run_b,
            &format!("apr_b_{suffix}"),
            "cost",
            None,
            "requested",
        )
        .await;

        sweep_once(&pool, Utc::now()).await.expect("sweep");

        let a = announced(&pool, &run_a).await;
        let b = announced(&pool, &run_b).await;
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].org_id, org_a);
        assert_eq!(a[0].user_id, user_a);
        assert_eq!(b[0].org_id, org_b);
        assert_eq!(b[0].user_id, user_b);
    }
}

/// EP-7 S5: the goal item build.
#[cfg(test)]
#[path = "attention_events_goal_tests.rs"]
mod goal_tests;
