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
//!
//! A row is written only while its source still waits. The write takes a share lock on that source row, which a
//! decision on it must wait for until the write commits, so a decision that lands between the sweep's read and
//! its write writes nothing ([`enqueue`]). A decision that commits after the write is announced as the item it
//! was, inside the outbox's delivery latency (DEC-F04-6).
//!
//! The sweep reads back from its durable mark (migration 0132), the start time of its last complete pass. A pass
//! after an outage reads the rows created while it was down, and announces each one once. The lookback is the
//! mark's age plus a minute, never under ten minutes and never over a day. A row created more than a day before a
//! pass is not announced: the cap bounds the work, and the burst, after a long outage.

use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{AttentionReason, AttentionState};
use mp_events::envelope::Envelope;
use mp_events::idempotency::derive_idempotency_hash;
use mp_events::subjects::run_event_subject;
use sqlx::{PgPool, Postgres, Transaction};

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

/// The shortest lookback of a pass. A sweep runs every few seconds, so a row is announced by the first sweep after
/// it is created; a pass reads further back only while its mark is older (see [`lookback_seconds`]).
const SWEEP_WINDOW_SECONDS: f64 = 600.0;

/// The overlap a pass reads back past its mark, so a row that committed after the mark was taken is still read.
const SWEEP_OVERLAP_SECONDS: f64 = 60.0;

/// The longest lookback a pass reads. A row created longer ago than this is not announced.
const MAX_CATCHUP_SECONDS: f64 = 86_400.0;

/// The name of this sweeper's row in `attention_sweep_marks`.
const SWEEPER: &str = "run_attention";

/// The origins whose waiting items are announced: the origins the attention list covers. A Space or a
/// support thread answers to its own surface. The matrix test pins this list equal to the list's. The
/// crash-park notice (O-02) reads this same list, so a park is announced exactly where the list names it.
pub(crate) const LISTED_ORIGINS: [&str; 2] = ["chat", "agent_run"];

/// Whether the sweep is switched on. Read once at boot.
pub(crate) fn enabled() -> bool {
    enabled_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The switch's value through the one strict parser of the durable-loop switches: only the exact
/// literal `true`, with no trimming and no case folding, and nothing else.
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

/// The lookback of one pass, in seconds: the time since the sweeper's mark plus the overlap, kept between the
/// window every pass reads and the catch-up cap. Without a mark it is the window. A mark ahead of the clock reads
/// as the window too, so clock skew cannot shrink a pass below it.
#[allow(clippy::cast_precision_loss)]
fn lookback_seconds(mark: Option<DateTime<Utc>>, now: DateTime<Utc>) -> f64 {
    let Some(mark) = mark else {
        return SWEEP_WINDOW_SECONDS;
    };
    let since_mark = (now - mark).num_milliseconds() as f64 / 1000.0;
    (since_mark + SWEEP_OVERLAP_SECONDS).clamp(SWEEP_WINDOW_SECONDS, MAX_CATCHUP_SECONDS)
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

impl ApprovalRow {
    /// The state this row puts its item in, or `None` when it no longer waits. The sweep and the confirm both read
    /// it through here, so the two cannot disagree about which approval waits.
    fn state(&self) -> Option<AttentionState> {
        approval_state(PendingApproval {
            kind: self.kind.clone(),
            run_status: self.run_status.clone(),
            plan_status: self.plan_status.clone(),
            requested_at: self.requested_at,
        })
    }
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
    window_seconds: f64,
    origins: &[String],
    withheld: &mut u64,
) -> Result<Vec<Waiting>, sqlx::Error> {
    if !attention::goals_enabled() {
        return Ok(Vec::new());
    }
    let rows: Vec<StoppedGoalRow> = sqlx::query_as(GOALS_SQL)
        .bind(window_seconds)
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

/// The sweeper's high-water mark: the start time of its last complete pass, or `None` before the first one.
async fn read_mark(pool: &PgPool, sweeper: &str) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT high_water FROM attention_sweep_marks WHERE sweeper = $1",
    )
    .bind(sweeper)
    .fetch_optional(pool)
    .await
}

/// Moves the mark forward to a complete pass's start time. It never moves back, so two passes that overlap cannot
/// undo each other's mark.
async fn advance_mark(
    pool: &PgPool,
    sweeper: &str,
    started: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO attention_sweep_marks (sweeper, high_water) VALUES ($1, $2)
         ON CONFLICT (sweeper) DO UPDATE
         SET high_water = GREATEST(attention_sweep_marks.high_water, EXCLUDED.high_water)",
    )
    .bind(sweeper)
    .bind(started)
    .execute(pool)
    .await?;
    Ok(())
}

/// One sweep: reads the pending items of the three tables and the stopped goals, gates them on their run's
/// posture, and writes one outbox row for each item that still waits and has none yet. Returns how many rows
/// this sweep wrote.
///
/// Each row is its own statement, so a refused row is counted, left for the next sweep, and never holds
/// back the others. A failed read ends the pass and leaves the mark where it was, so the next pass reads
/// from the same mark again.
pub(crate) async fn sweep_once(pool: &PgPool, now: DateTime<Utc>) -> Result<usize, sqlx::Error> {
    sweep_pass(pool, now, SWEEPER).await
}

/// One pass of the sweep for `sweeper`, which names the mark the pass reads from and advances.
async fn sweep_pass(
    pool: &PgPool,
    now: DateTime<Utc>,
    sweeper: &str,
) -> Result<usize, sqlx::Error> {
    let window = lookback_seconds(read_mark(pool, sweeper).await?, now);
    let origins: Vec<String> = LISTED_ORIGINS
        .iter()
        .map(|origin| (*origin).to_owned())
        .collect();
    let approvals: Vec<ApprovalRow> = sqlx::query_as(APPROVALS_SQL)
        .bind(window)
        .bind(&origins)
        .fetch_all(pool)
        .await?;
    let plans: Vec<WaitingRow> = sqlx::query_as(PLANS_SQL)
        .bind(window)
        .bind(&origins)
        .fetch_all(pool)
        .await?;
    let questions: Vec<WaitingRow> = sqlx::query_as(QUESTIONS_SQL)
        .bind(window)
        .bind(&origins)
        .fetch_all(pool)
        .await?;

    // Counted per sweep: a waiting row is seen by every sweep until it stops waiting.
    let mut withheld: u64 = 0;
    let mut items = Vec::new();
    for row in approvals {
        let Some(state) = row.state() else {
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
    items.extend(goal_items(pool, window, &origins, &mut withheld).await?);
    if withheld > 0 {
        metrics::counter!("mp_session_attention_events_skipped_zdr_total").increment(withheld);
    }

    let mut enqueued = 0;
    let mut failed = 0;
    for item in &items {
        match enqueue(pool, item, now).await {
            Ok(true) => {
                metrics::counter!("mp_session_attention_events_enqueued_total").increment(1);
                enqueued += 1;
            }
            Ok(false) => {}
            Err(error) => {
                failed += 1;
                metrics::counter!("mp_session_attention_events_dropped_total").increment(1);
                tracing::warn!(%error, run_id = %item.run_id, "RUN_ATTENTION event not enqueued; the next sweep retries it");
            }
        }
    }
    // The mark moves only when this pass wrote every row it found. A row a failed write left behind must still
    // fall inside the next pass's lookback, or no pass would announce it.
    if failed == 0 {
        advance_mark(pool, sweeper, now).await?;
    }
    Ok(enqueued)
}

/// The row an item waits on, which its write re-checks under a share lock (F-f04-20). A plan item names either its
/// proposed plan row or the plan approval that names it, so both are tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitsOn {
    Approval,
    Plan,
    Question,
    Goal,
    /// A crash park's notice (O-02), read by the confirm only. The sweep names no park, so [`waits_on`] never
    /// returns this row.
    CrashPark,
}

/// The rows an item may wait on, by what the item is. Every state is matched with no catch-all, so a new state
/// cannot be left without a row to re-check: the compiler stops at this match. A state that raises no item waits
/// on nothing, so an item of that state is never written.
fn waits_on(item: &Waiting) -> &'static [WaitsOn] {
    match &item.goal {
        Some(_) => &[WaitsOn::Goal],
        None => match item.state {
            AttentionState::NeedsApproval => &[WaitsOn::Approval],
            AttentionState::PlanReady => &[WaitsOn::Approval, WaitsOn::Plan],
            AttentionState::NeedsInput => &[WaitsOn::Question],
            AttentionState::Unspecified
            | AttentionState::Working
            | AttentionState::Completed
            | AttentionState::Failed
            | AttentionState::Archived
            | AttentionState::Idle => &[],
        },
    }
}

/// The still-waiting check of each row. Each reads its row under `FOR SHARE`, which a decision on that row waits for
/// until the write commits, so a decision cannot interleave between the check and the write.
const APPROVAL_WAITS_SQL: &str = "SELECT 1 FROM approvals
     WHERE id = $1 AND status = 'requested' AND (expires_at IS NULL OR expires_at > now())
     FOR SHARE";
const PLAN_WAITS_SQL: &str = "SELECT 1 FROM plans WHERE id = $1 AND status = 'proposed' FOR SHARE";
const QUESTION_WAITS_SQL: &str = "SELECT 1 FROM run_questions
     WHERE id = $1 AND state = 'requested' AND expires_at > now()
     FOR SHARE";
const GOAL_WAITS_SQL: &str = "SELECT 1 FROM thread_goals
     WHERE id = $1 AND status = $2 AND status IN ('met', 'budget_reached', 'turns_reached', 'blocked')
     FOR SHARE";

/// Whether an item's row still waits, read under the share lock the write keeps until it commits.
async fn still_waits(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    item: &Waiting,
) -> Result<bool, sqlx::Error> {
    for row in waits_on(item) {
        let found: Option<i32> = match row {
            WaitsOn::Approval => {
                sqlx::query_scalar::<_, i32>(APPROVAL_WAITS_SQL)
                    .bind(&item.source_id)
                    .fetch_optional(&mut **tx)
                    .await?
            }
            WaitsOn::Plan => {
                sqlx::query_scalar::<_, i32>(PLAN_WAITS_SQL)
                    .bind(&item.source_id)
                    .fetch_optional(&mut **tx)
                    .await?
            }
            WaitsOn::Question => {
                sqlx::query_scalar::<_, i32>(QUESTION_WAITS_SQL)
                    .bind(&item.source_id)
                    .fetch_optional(&mut **tx)
                    .await?
            }
            WaitsOn::Goal => {
                // The goal must still hold the status the item names, so a goal that moved on is not announced.
                let status = item.goal.as_ref().map_or("", |goal| goal.status.as_str());
                sqlx::query_scalar::<_, i32>(GOAL_WAITS_SQL)
                    .bind(&item.source_id)
                    .bind(status)
                    .fetch_optional(&mut **tx)
                    .await?
            }
            // `waits_on` never names a crash park, so no sweep item is checked against one. It answers no, so nothing
            // is written for a park; the confirm reads a park by [`confirm_crash_park`].
            WaitsOn::CrashPark => None,
        };
        if found.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Writes one waiting item's row, if its source still waits (F-f04-20). Returns `false` when the row is already
/// there, which is what a later sweep finds. It also returns `false`, and writes nothing, when the source was
/// decided, expired or moved on since the sweep read it: that is counted as stale.
async fn enqueue(pool: &PgPool, item: &Waiting, now: DateTime<Utc>) -> Result<bool, sqlx::Error> {
    let envelope = build_run_attention_envelope(item, now);
    let payload =
        serde_json::to_value(&envelope).map_err(|error| sqlx::Error::Encode(error.into()))?;
    let mut tx = pool.begin().await?;
    if !still_waits(&mut tx, item).await? {
        tx.rollback().await?;
        metrics::counter!("mp_session_attention_events_stale_total").increment(1);
        return Ok(false);
    }
    let inserted = sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(&envelope.event_id)
    .bind(run_event_subject(&item.run_id))
    .bind(&payload)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(inserted == 1)
}

/// What a service names to ask whether one announced item still waits (F-f05-03). The ids are the ones the item was
/// announced with, and the state is the one it was announced in. Every id is matched against the row, so a row of
/// another org, owner, thread or run answers not waiting.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConfirmQuery<'a> {
    pub(crate) org_id: &'a str,
    pub(crate) user_id: &'a str,
    pub(crate) thread_id: &'a str,
    pub(crate) run_id: &'a str,
    pub(crate) source_id: &'a str,
    pub(crate) state: AttentionState,
}

/// The rows an announced state can wait on, for a confirm. NEEDS_INPUT is a question's state, a stopped goal's or a
/// crash park's (O-02), so it reads all three rows; COMPLETED is a met goal alone. Every state is named, so a new one
/// cannot slip past the match. A state that raises no item waits on nothing.
fn confirm_rows(state: AttentionState) -> &'static [WaitsOn] {
    match state {
        AttentionState::NeedsApproval => &[WaitsOn::Approval],
        AttentionState::PlanReady => &[WaitsOn::Approval, WaitsOn::Plan],
        AttentionState::NeedsInput => &[WaitsOn::Question, WaitsOn::Goal, WaitsOn::CrashPark],
        AttentionState::Completed => &[WaitsOn::Goal],
        AttentionState::Unspecified
        | AttentionState::Working
        | AttentionState::Failed
        | AttentionState::Archived
        | AttentionState::Idle => &[],
    }
}

/// The approval a confirm names, read by its id and matched to the owner, thread and run it was announced for. It
/// keeps the sweep's liveness predicates (requested and unexpired) and leaves out the sweep's window and origin.
const CONFIRM_APPROVAL_SQL: &str =
    "SELECT r.org_id, r.user_id, r.thread_id, r.id AS run_id, a.id AS source_id,
        a.kind, r.status AS run_status, p.status AS plan_status, a.requested_at,
        r.retention_class, COALESCE(o.zdr, false) AS zdr
     FROM approvals a
     JOIN runs r ON r.id = a.run_id
     LEFT JOIN plans p ON p.id = a.plan_id
     LEFT JOIN managed_run_terminalization_outbox o ON o.run_id = r.id
     WHERE a.id = $1
       AND r.org_id = $2
       AND r.user_id = $3
       AND r.thread_id = $4
       AND r.id = $5
       AND a.status = 'requested'
       AND (a.expires_at IS NULL OR a.expires_at > now())";

/// The proposed plan a confirm names: still proposed, with no live plan approval that would announce it instead. The
/// window and the origin are left out, as in the approval's read.
const CONFIRM_PLAN_SQL: &str =
    "SELECT r.org_id, r.user_id, p.thread_id, r.id AS run_id, p.id AS source_id,
        r.retention_class, COALESCE(o.zdr, false) AS zdr
     FROM plans p
     JOIN runs r ON r.id = p.run_id
     LEFT JOIN managed_run_terminalization_outbox o ON o.run_id = r.id
     WHERE p.id = $1
       AND r.org_id = $2
       AND r.user_id = $3
       AND p.thread_id = $4
       AND r.id = $5
       AND p.status = 'proposed'
       AND NOT EXISTS (
           SELECT 1 FROM approvals a
           WHERE a.plan_id = p.id
             AND a.kind = 'plan'
             AND a.status = 'requested'
             AND (a.expires_at IS NULL OR a.expires_at > now())
       )";

/// The run question a confirm names: still requested and unexpired. The window and the origin are left out.
const CONFIRM_QUESTION_SQL: &str =
    "SELECT r.org_id, r.user_id, q.thread_id, r.id AS run_id, q.id AS source_id,
        r.retention_class, COALESCE(o.zdr, false) AS zdr
     FROM run_questions q
     JOIN runs r ON r.id = q.run_id
     LEFT JOIN managed_run_terminalization_outbox o ON o.run_id = r.id
     WHERE q.id = $1
       AND r.org_id = $2
       AND r.user_id = $3
       AND q.thread_id = $4
       AND r.id = $5
       AND q.state = 'requested'
       AND q.expires_at > now()";

/// The stopped goal a confirm names, read as [`GOALS_SQL`] reads it: the thread's latest goal, a stopped status and
/// the run of its last settled turn. Its NOT EXISTS clauses are GOALS_SQL's own text, which a test pins equal, so no
/// other row of the thread may wait. The window and the origin are left out.
const CONFIRM_GOAL_SQL: &str = "SELECT g.org_id, g.user_id, g.thread_id, last_turn.run_id,
        g.id AS source_id, g.status, COALESCE(g.closed_at, g.updated_at) AS since,
        latest.status AS latest_run_status, latest.updated_at AS latest_run_at,
        r.retention_class, COALESCE(o.zdr, false) AS zdr
     FROM thread_goals g
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
     WHERE g.id = $1
       AND g.org_id = $2
       AND g.user_id = $3
       AND g.thread_id = $4
       AND last_turn.run_id = $5
       AND g.status IN ('met', 'budget_reached', 'turns_reached', 'blocked')
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

/// Whether the item a confirm names still waits on its owner, in the state it was announced in (F-f05-03). Each row
/// the state can wait on is read by its id and matched to the item's owner, org, thread and run; the first that still
/// waits answers yes. The derivation is the sweep's: an approval through [`ApprovalRow::state`], a goal through
/// [`goal_stop`], and every row through [`enqueue_permitted`], so the confirm and the sweep cannot disagree. A
/// zero-retention or waived run answers no, and so does a row that was decided, answered, expired or moved on. A
/// crash park answers while the attention list names it (O-02). Reads only: nothing is written.
pub(crate) async fn still_waiting(
    pool: &PgPool,
    query: &ConfirmQuery<'_>,
) -> Result<bool, sqlx::Error> {
    still_waiting_with(
        pool,
        query,
        attention::goals_enabled(),
        crate::loop_checkpoint::crash_park_notice_active(),
    )
    .await
}

/// [`still_waiting`] with the thread goals switch and the crash-park switch passed in. The sweep reads no goal while
/// the goals switch is off, so a confirm of a goal answers no then. The crash-park switch is the one the list reads a
/// park by, so a confirm of a park answers no while it is off, as the list names none. Both are parameters so a test
/// can read either with its switch on or off.
async fn still_waiting_with(
    pool: &PgPool,
    query: &ConfirmQuery<'_>,
    goals_on: bool,
    parks_on: bool,
) -> Result<bool, sqlx::Error> {
    for &row in confirm_rows(query.state) {
        let waiting = match row {
            WaitsOn::Approval => confirm_approval(pool, query).await?,
            WaitsOn::Plan => confirm_plan(pool, query).await?,
            WaitsOn::Question => confirm_question(pool, query).await?,
            WaitsOn::Goal if !goals_on => false,
            WaitsOn::Goal => confirm_goal(pool, query).await?,
            WaitsOn::CrashPark if !parks_on => false,
            WaitsOn::CrashPark => confirm_crash_park(pool, query).await?,
        };
        if waiting {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the approval a confirm names still waits in the state it was announced in.
async fn confirm_approval(pool: &PgPool, query: &ConfirmQuery<'_>) -> Result<bool, sqlx::Error> {
    let row: Option<ApprovalRow> = sqlx::query_as(CONFIRM_APPROVAL_SQL)
        .bind(query.source_id)
        .bind(query.org_id)
        .bind(query.user_id)
        .bind(query.thread_id)
        .bind(query.run_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some_and(|row| {
        enqueue_permitted(row.zdr, &row.retention_class) && row.state() == Some(query.state)
    }))
}

/// Whether the proposed plan a confirm names still waits. A plan is ready only by its own row, so the state needs no
/// further check here: [`confirm_rows`] reads this row only for PLAN_READY.
async fn confirm_plan(pool: &PgPool, query: &ConfirmQuery<'_>) -> Result<bool, sqlx::Error> {
    let row: Option<WaitingRow> = sqlx::query_as(CONFIRM_PLAN_SQL)
        .bind(query.source_id)
        .bind(query.org_id)
        .bind(query.user_id)
        .bind(query.thread_id)
        .bind(query.run_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some_and(|row| enqueue_permitted(row.zdr, &row.retention_class)))
}

/// Whether the run question a confirm names still waits. A question is needs-input only by its own row, as in
/// [`confirm_plan`].
async fn confirm_question(pool: &PgPool, query: &ConfirmQuery<'_>) -> Result<bool, sqlx::Error> {
    let row: Option<WaitingRow> = sqlx::query_as(CONFIRM_QUESTION_SQL)
        .bind(query.source_id)
        .bind(query.org_id)
        .bind(query.user_id)
        .bind(query.thread_id)
        .bind(query.run_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some_and(|row| enqueue_permitted(row.zdr, &row.retention_class)))
}

/// Whether the stopped goal a confirm names still waits in the state it was announced in. The caller has checked the
/// thread goals switch, which the sweep reads too.
async fn confirm_goal(pool: &PgPool, query: &ConfirmQuery<'_>) -> Result<bool, sqlx::Error> {
    let row: Option<StoppedGoalRow> = sqlx::query_as(CONFIRM_GOAL_SQL)
        .bind(query.source_id)
        .bind(query.org_id)
        .bind(query.user_id)
        .bind(query.thread_id)
        .bind(query.run_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some_and(|row| {
        enqueue_permitted(row.zdr, &row.retention_class)
            && goal_stop(&row).is_some_and(|(state, _)| state == query.state)
    }))
}

/// The crash park a confirm names (O-02), read as the attention list reads it: the due predicate the notice and the
/// list share, the source matched to the item, and the owner, org, thread and run matched too. The source is the
/// park's ref (its `pause_ref`), or the run when the park has none, which is the source the notice names. The run must
/// still be its thread's latest, as the list requires. The notice's own stamp is left out, as the list leaves it out,
/// so a due park whose notice is not yet written still confirms. The window and the origin are left out, as in the
/// other confirm reads. Placeholders: `$1` the park reason, `$2` the notice time and `$3` the park TTL, as the
/// predicate names them; `$4` the source, `$5` the org, `$6` the user, `$7` the thread and `$8` the run.
fn confirm_crash_park_sql() -> String {
    format!(
        "SELECT r.org_id, r.user_id, r.thread_id, r.id AS run_id,
                COALESCE(c.pause_ref, c.run_id) AS source_id,
                r.retention_class, COALESCE(o.zdr, false) AS zdr
           FROM run_loop_checkpoints AS c
           JOIN runs AS r ON r.id = c.run_id
           LEFT JOIN managed_run_terminalization_outbox AS o ON o.run_id = r.id
          WHERE {DUE}
            AND COALESCE(c.pause_ref, c.run_id) = $4
            AND c.org_id = $5
            AND c.user_id = $6
            AND r.thread_id = $7
            AND c.run_id = $8
            AND NOT EXISTS (
                SELECT 1 FROM runs AS newer
                 WHERE newer.thread_id = r.thread_id
                   AND (newer.updated_at, newer.created_at, newer.id) > (r.updated_at, r.created_at, r.id)
            )",
        DUE = crate::loop_checkpoint::DUE_CRASH_PARK_PREDICATE
    )
}

/// Whether the crash park a confirm names still waits (O-02). It reads the notice time and park TTL the list reads, and
/// no switch: the caller passes the list's switch in ([`still_waiting_with`]).
async fn confirm_crash_park(pool: &PgPool, query: &ConfirmQuery<'_>) -> Result<bool, sqlx::Error> {
    let sql = confirm_crash_park_sql();
    let row: Option<WaitingRow> = sqlx::query_as(&sql)
        .bind(mp_contracts::loop_fold::ParkReason::CrashInterrupted.as_str())
        .bind(crate::loop_checkpoint::crash_park_notice_seconds() as f64)
        .bind(crate::loop_checkpoint::park_ttl_seconds() as f64)
        .bind(query.source_id)
        .bind(query.org_id)
        .bind(query.user_id)
        .bind(query.thread_id)
        .bind(query.run_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some_and(|row| enqueue_permitted(row.zdr, &row.retention_class)))
}

/// One crash park whose notice is due (O-02): the ids the notice names, read from the park's row. Nothing of the
/// run's content is in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CrashParkNotice {
    pub(crate) org_id: String,
    pub(crate) user_id: String,
    pub(crate) thread_id: String,
    pub(crate) run_id: String,
    /// The park's own reference: the row the notice is about, or the run when the park has none.
    pub(crate) source_id: String,
}

/// The event id of a crash park's notice. It names the run only, so a run's park is announced once: a second
/// replica, a later pass or a redelivery that finds the same run writes nothing.
pub(crate) fn crash_park_notice_event_id(run_id: &str) -> String {
    format!("crash-park-notice:{run_id}")
}

/// Build the `RUN_ATTENTION` envelope of a crash park's notice (O-02). Its state and reason are the ones the
/// attention list derives for the same park, so the notice and the list name the same thing. Pure, so the wire
/// shape is pinned by a test.
pub(crate) fn build_crash_park_notice_envelope(
    notice: &CrashParkNotice,
    ts: DateTime<Utc>,
) -> Envelope {
    let event_id = crash_park_notice_event_id(&notice.run_id);
    Envelope {
        event_id: event_id.clone(),
        event_type: RUN_ATTENTION_EVENT_TYPE.to_owned(),
        schema_version: 1,
        ts,
        producer: PRODUCER.to_owned(),
        correlation_id: notice.run_id.clone(),
        // The parked row is what caused the event.
        causation_id: notice.source_id.clone(),
        idempotency_key: event_id,
        org_id: notice.org_id.clone(),
        user_id: notice.user_id.clone(),
        resource_ref: run_resource_ref(&notice.run_id),
        // Ids and closed tokens only.
        payload: serde_json::json!({
            "run_id": notice.run_id,
            "source_id": notice.source_id,
            "state": AttentionState::NeedsInput.as_str_name(),
            "thread_id": notice.thread_id,
            "reason": AttentionReason::Parked.as_str_name(),
        }),
        zdr: false,
        ..Default::default()
    }
}

/// Writes one crash park's notice row inside the caller's transaction (O-02), on the run-event subject the sweep
/// writes to. `Ok(false)` when the row is already there. The caller has checked that the run is persistent.
pub(crate) async fn write_crash_park_notice(
    tx: &mut Transaction<'_, Postgres>,
    notice: &CrashParkNotice,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let envelope = build_crash_park_notice_envelope(notice, now);
    let payload =
        serde_json::to_value(&envelope).map_err(|error| sqlx::Error::Encode(error.into()))?;
    let inserted = sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(&envelope.event_id)
    .bind(run_event_subject(&notice.run_id))
    .bind(&payload)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(inserted == 1)
}

#[cfg(test)]
mod crash_park_notice_tests {
    use super::*;
    use chrono::TimeZone;

    fn ts() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    fn notice() -> CrashParkNotice {
        CrashParkNotice {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            run_id: "run-1".to_owned(),
            source_id: "park-ref-1".to_owned(),
        }
    }

    #[test]
    fn the_notice_is_keyed_by_its_run_so_a_park_is_announced_once() {
        assert_eq!(
            crash_park_notice_event_id("run-1"),
            "crash-park-notice:run-1"
        );
        let envelope = build_crash_park_notice_envelope(&notice(), ts());
        assert_eq!(envelope.event_id, "crash-park-notice:run-1");
        assert_eq!(envelope.idempotency_key, "crash-park-notice:run-1");
        assert_eq!(envelope.event_type, RUN_ATTENTION_EVENT_TYPE);
        assert!(
            !envelope.zdr,
            "a notice is written only for a persistent run"
        );
    }

    #[test]
    fn the_notice_payload_carries_ids_and_closed_tokens_only() {
        let envelope = build_crash_park_notice_envelope(&notice(), ts());
        let mut keys: Vec<&str> = envelope
            .payload
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["reason", "run_id", "source_id", "state", "thread_id"]
        );
        assert_eq!(envelope.payload["state"], "ATTENTION_STATE_NEEDS_INPUT");
        assert_eq!(envelope.payload["reason"], "ATTENTION_REASON_PARKED");
    }

    #[test]
    fn the_notice_names_the_state_and_reason_the_list_derives_for_the_same_park() {
        let derived = attention::derive(
            &ThreadFacts {
                archived_at: None,
                latest_run_status: "running",
                latest_run_at: None,
            },
            &Pending {
                crash_park: Some(ts()),
                ..Pending::default()
            },
        );
        let envelope = build_crash_park_notice_envelope(&notice(), ts());
        assert_eq!(envelope.payload["state"], derived.state.as_str_name());
        assert_eq!(envelope.payload["reason"], derived.reason.as_str_name());
    }
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
        assert!(!enabled_from(Some(" TRUE ")));
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

    #[test]
    fn the_lookback_is_the_window_until_a_mark_ages_and_never_passes_the_cap() {
        let now = at();
        assert_eq!(
            lookback_seconds(None, now),
            SWEEP_WINDOW_SECONDS,
            "without a mark, the window"
        );
        assert_eq!(
            lookback_seconds(Some(now - chrono::Duration::seconds(5)), now),
            SWEEP_WINDOW_SECONDS,
            "a fresh mark never shrinks the window"
        );
        assert_eq!(
            lookback_seconds(Some(now - chrono::Duration::minutes(30)), now),
            1860.0,
            "thirty minutes down, plus the overlap"
        );
        assert_eq!(
            lookback_seconds(Some(now - chrono::Duration::days(2)), now),
            MAX_CATCHUP_SECONDS,
            "a lapse past the cap reads the cap"
        );
        assert_eq!(
            lookback_seconds(Some(now + chrono::Duration::hours(1)), now),
            SWEEP_WINDOW_SECONDS,
            "a mark ahead of the clock reads the window"
        );
    }

    #[test]
    fn each_waiting_state_names_the_row_it_waits_on_and_no_other_state_waits() {
        assert_eq!(
            waits_on(&waiting(AttentionState::NeedsApproval, "apr-1")),
            &[WaitsOn::Approval]
        );
        assert_eq!(
            waits_on(&waiting(AttentionState::PlanReady, "plan-1")),
            &[WaitsOn::Approval, WaitsOn::Plan],
            "a plan is ready by its approval or by its plan row"
        );
        assert_eq!(
            waits_on(&waiting(AttentionState::NeedsInput, "q-1")),
            &[WaitsOn::Question]
        );
        let goal = Waiting {
            goal: Some(GoalStop {
                reason: AttentionReason::Goal,
                status: "met".to_owned(),
            }),
            ..waiting(AttentionState::Completed, "goal-1")
        };
        assert_eq!(waits_on(&goal), &[WaitsOn::Goal]);
        for state in [
            AttentionState::Unspecified,
            AttentionState::Working,
            AttentionState::Completed,
            AttentionState::Failed,
            AttentionState::Archived,
            AttentionState::Idle,
        ] {
            assert!(
                waits_on(&waiting(state, "x")).is_empty(),
                "{state:?} raises no item, so it waits on no row"
            );
        }
    }

    /// Whitespace collapsed, so a SQL text is compared by its tokens and not by how its lines are laid out.
    fn squeezed(sql: &str) -> String {
        sql.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Each `NOT EXISTS (...)` clause of a query, collapsed, in the order the query names them.
    fn not_exists_clauses(sql: &str) -> Vec<String> {
        let text = squeezed(sql);
        let mut clauses = Vec::new();
        let mut from = 0;
        while let Some(offset) = text[from..].find("NOT EXISTS (") {
            let open = from + offset + "NOT EXISTS ".len();
            let mut depth = 0_i32;
            let mut close = text.len();
            for (index, byte) in text[open..].bytes().enumerate() {
                match byte {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            close = open + index + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            clauses.push(text[open..close].to_owned());
            from = close;
        }
        clauses
    }

    /// The confirm reads keep the sweep's liveness predicates and leave out its window and its origin, so a row the
    /// sweep would have announced and still waits is confirmed however old it is.
    #[test]
    fn the_confirm_reads_keep_the_sweeps_liveness_and_drop_its_window_and_origin() {
        let approval_live =
            "a.status = 'requested' AND (a.expires_at IS NULL OR a.expires_at > now())";
        assert!(squeezed(APPROVALS_SQL).contains(approval_live));
        let approval = squeezed(CONFIRM_APPROVAL_SQL);
        assert!(approval.contains(approval_live));
        assert!(!approval.contains("make_interval") && !approval.contains("origin"));

        let plan_sweep = not_exists_clauses(PLANS_SQL);
        assert_eq!(
            plan_sweep.len(),
            1,
            "the plan pass has one live-approval clause"
        );
        let plan_without_window = plan_sweep[0].replace(
            " AND a.requested_at > now() - make_interval(secs => $1)",
            "",
        );
        assert!(squeezed(CONFIRM_PLAN_SQL).contains(&plan_without_window));
        assert!(!squeezed(CONFIRM_PLAN_SQL).contains("make_interval"));

        let question_live = "q.state = 'requested' AND q.expires_at > now()";
        assert!(squeezed(QUESTIONS_SQL).contains(question_live));
        assert!(squeezed(CONFIRM_QUESTION_SQL).contains(question_live));
        assert!(!squeezed(CONFIRM_QUESTION_SQL).contains("make_interval"));

        let goal_sweep = not_exists_clauses(GOALS_SQL);
        assert_eq!(
            goal_sweep.len(),
            4,
            "the goal pass names four liveness clauses"
        );
        let confirm_goal = squeezed(CONFIRM_GOAL_SQL);
        for clause in &goal_sweep {
            assert!(
                confirm_goal.contains(clause),
                "the goal confirm carries the sweep's clause: {clause}"
            );
        }
        assert!(!confirm_goal.contains("make_interval") && !confirm_goal.contains("origin"));
        for sql in [
            CONFIRM_APPROVAL_SQL,
            CONFIRM_PLAN_SQL,
            CONFIRM_QUESTION_SQL,
            CONFIRM_GOAL_SQL,
        ] {
            assert!(
                squeezed(sql).contains("COALESCE(o.zdr, false) AS zdr"),
                "every confirm read carries the posture the sweep gates on"
            );
        }
    }

    #[test]
    fn a_confirm_reads_the_rows_each_announced_state_can_wait_on() {
        assert_eq!(
            confirm_rows(AttentionState::NeedsApproval),
            &[WaitsOn::Approval]
        );
        assert_eq!(
            confirm_rows(AttentionState::PlanReady),
            &[WaitsOn::Approval, WaitsOn::Plan]
        );
        assert_eq!(
            confirm_rows(AttentionState::NeedsInput),
            &[WaitsOn::Question, WaitsOn::Goal, WaitsOn::CrashPark],
            "NEEDS_INPUT is a question's, a stopped goal's or a crash park's"
        );
        assert_eq!(confirm_rows(AttentionState::Completed), &[WaitsOn::Goal]);
        for state in [
            AttentionState::Unspecified,
            AttentionState::Working,
            AttentionState::Failed,
            AttentionState::Archived,
            AttentionState::Idle,
        ] {
            assert!(confirm_rows(state).is_empty(), "{state:?} waits on no row");
        }
    }

    /// O-02: a crash park is confirmed by the source the notice names, and the notice's stamp is not part of the read,
    /// so a due park whose notice is not yet written still confirms. The read carries the list's due predicate, the
    /// latest-run check and the posture the notice gates on, and `still_waiting` passes the list's switch in.
    #[test]
    fn a_crash_park_confirm_reads_the_park_the_notice_names_as_the_list_does() {
        let sql = squeezed(&confirm_crash_park_sql());
        assert!(
            sql.contains(&squeezed(crate::loop_checkpoint::DUE_CRASH_PARK_PREDICATE)),
            "the confirm reads the due predicate the notice and the list share"
        );
        assert!(
            sql.contains("COALESCE(c.pause_ref, c.run_id) = $4"),
            "the source is the park's ref, or its run when the park has none"
        );
        assert!(
            !sql.contains("park_notice_sent_at"),
            "the notice's own stamp is not part of the read"
        );
        assert!(
            sql.contains(
                "(newer.updated_at, newer.created_at, newer.id) > (r.updated_at, r.created_at, r.id)"
            ),
            "a newer run of the thread overtakes the park"
        );
        assert!(
            sql.contains("COALESCE(o.zdr, false) AS zdr"),
            "the read carries the posture the notice gates on"
        );
        let source = include_str!("attention_events.rs").replace("\r\n", "\n");
        let production = &source[..source
            .find("\n#[cfg(test)]\nmod tests {")
            .expect("tests marker")];
        assert!(
            production.contains("crate::loop_checkpoint::crash_park_notice_active()"),
            "still_waiting reads the list's crash-park switch"
        );
    }

    /// The approval state of a confirm is the sweep's, through the one row helper: a row that raises no state
    /// answers none, and a plan approval answers only while its plan is proposed.
    #[test]
    fn a_confirmed_approval_is_judged_by_the_sweeps_row_helper() {
        let row = |kind: &str, run_status: &str, plan_status: Option<&str>| ApprovalRow {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            thread_id: "thread-9".to_owned(),
            run_id: "run-abc".to_owned(),
            source_id: "apr-1".to_owned(),
            kind: kind.to_owned(),
            run_status: run_status.to_owned(),
            plan_status: plan_status.map(str::to_owned),
            requested_at: at(),
            retention_class: "persistent".to_owned(),
            zdr: false,
        };
        assert_eq!(
            row("tool_call", "running", None).state(),
            Some(AttentionState::NeedsApproval)
        );
        assert_eq!(row("tool_call", "completed", None).state(), None);
        assert_eq!(
            row("plan", "running", Some("proposed")).state(),
            Some(AttentionState::PlanReady)
        );
        assert_eq!(row("plan", "running", Some("superseded")).state(), None);
        assert_eq!(row("mystery", "running", None).state(), None);
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::loop_checkpoint::test_support::{fold_for, plant_round_fold, scratch_pool};
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

    /// Sets a sweeper's mark, as a pass that completed at `at` would have left it.
    async fn set_mark(pool: &PgPool, sweeper: &str, at: DateTime<Utc>) {
        sqlx::query(
            "INSERT INTO attention_sweep_marks (sweeper, high_water) VALUES ($1, $2)
             ON CONFLICT (sweeper) DO UPDATE SET high_water = EXCLUDED.high_water",
        )
        .bind(sweeper)
        .bind(at)
        .execute(pool)
        .await
        .expect("set the mark");
    }

    /// F-f04-20: a decision that lands after the sweep read an approval as waiting, and before the sweep writes
    /// its row, leaves no row. The write re-checks the approval under a share lock.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_decision_between_the_read_and_the_write_writes_no_row() {
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
        // The sweep read the approval as waiting. Its owner decides it before the sweep writes.
        sqlx::query("UPDATE approvals SET status = 'approved', decided_at = now() WHERE id = $1")
            .bind(&approval)
            .execute(&pool)
            .await
            .expect("decide the approval");
        let item = Waiting {
            org_id: org,
            user_id: user.to_owned(),
            thread_id: thread,
            run_id: run.clone(),
            state: AttentionState::NeedsApproval,
            source_id: approval,
            goal: None,
        };

        let written = enqueue(&pool, &item, Utc::now()).await.expect("the write");

        assert!(!written, "a decided approval writes no row");
        assert!(announced(&pool, &run).await.is_empty());
    }

    /// F-f04-20: a waiting approval is announced once after the sweeper was down for thirty minutes. The pass
    /// after the outage reads back to the mark, so the approval created during the outage falls inside it.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_waiting_approval_is_announced_once_after_a_thirty_minute_outage() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("ev-{suffix}"), "ev-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        let approval = format!("apr_{suffix}");
        let sweeper = format!("run_attention_test_{suffix}");
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
        sqlx::query(
            "UPDATE approvals SET requested_at = now() - interval '30 minutes' WHERE id = $1",
        )
        .bind(&approval)
        .execute(&pool)
        .await
        .expect("age the approval to the outage");
        // The sweeper last completed a pass thirty minutes ago.
        let now = Utc::now();
        let last_pass = now - chrono::Duration::minutes(30);
        set_mark(&pool, &sweeper, last_pass).await;

        sweep_pass(&pool, now, &sweeper)
            .await
            .expect("the first pass after the outage");
        // A crash after the row is written and before the mark moves leaves the mark where it was, so the next
        // pass reads the same window again. The outbox id keeps the one row.
        set_mark(&pool, &sweeper, last_pass).await;
        sweep_pass(&pool, Utc::now(), &sweeper)
            .await
            .expect("the pass that reads the same window again");

        let rows = announced(&pool, &run).await;
        assert_eq!(
            rows.len(),
            1,
            "announced once, though a later pass reads it again"
        );
        assert_eq!(rows[0].payload["source_id"], approval.as_str());
    }

    /// F-f04-20: a row created more than a day before the pass is not announced, however long the sweeper was
    /// down. The lookback stops at the catch-up cap.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_row_older_than_the_catch_up_cap_is_not_announced() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("ev-{suffix}"), "ev-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        let approval = format!("apr_{suffix}");
        let sweeper = format!("run_attention_test_{suffix}");
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
        sqlx::query("UPDATE approvals SET requested_at = now() - interval '2 days' WHERE id = $1")
            .bind(&approval)
            .execute(&pool)
            .await
            .expect("age the approval past the cap");
        let now = Utc::now();
        set_mark(&pool, &sweeper, now - chrono::Duration::days(2)).await;

        sweep_pass(&pool, now, &sweeper).await.expect("the pass");

        assert!(
            announced(&pool, &run).await.is_empty(),
            "a row older than the cap is not announced"
        );
    }

    /// F-f04-20: a pass that completes moves its mark to its start time, and a later pass that started earlier
    /// never moves the mark back.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_completed_pass_moves_its_mark_to_its_start_and_never_back() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let sweeper = format!("run_attention_test_{}", new_ulid());
        let now = Utc::now();
        assert_eq!(
            read_mark(&pool, &sweeper).await.expect("read the mark"),
            None,
            "a sweeper with no pass has no mark"
        );

        sweep_pass(&pool, now, &sweeper)
            .await
            .expect("a pass with nothing to announce");
        let mark = read_mark(&pool, &sweeper)
            .await
            .expect("read the mark")
            .expect("the pass set its mark");
        assert_eq!(mark.timestamp_micros(), now.timestamp_micros());

        sweep_pass(&pool, now - chrono::Duration::minutes(5), &sweeper)
            .await
            .expect("an earlier pass");
        assert_eq!(
            read_mark(&pool, &sweeper).await.expect("read the mark"),
            Some(mark),
            "the mark never moves back"
        );
    }

    /// The query a service names for one announced item.
    fn confirm<'a>(
        org: &'a str,
        user: &'a str,
        thread: &'a str,
        run: &'a str,
        source: &'a str,
        state: AttentionState,
    ) -> ConfirmQuery<'a> {
        ConfirmQuery {
            org_id: org,
            user_id: user,
            thread_id: thread,
            run_id: run,
            source_id: source,
            state,
        }
    }

    /// One thread of `retention` with a requested tool approval on its run. Returns the org, the owner, the thread,
    /// the run and the approval, in that order.
    async fn seeded_requested_approval(
        pool: &PgPool,
        suffix: &str,
        retention: &str,
    ) -> [String; 5] {
        let (org, user) = (format!("cf-{suffix}"), "cf-user".to_owned());
        let (thread, run, approval) = (
            format!("thr_{suffix}"),
            format!("run_{suffix}"),
            format!("apr_{suffix}"),
        );
        seed_thread(pool, &org, &user, &thread).await;
        seed_run(pool, &org, &user, &thread, &run, retention).await;
        seed_approval(
            pool,
            &org,
            &user,
            &run,
            &approval,
            "tool_call",
            None,
            "requested",
        )
        .await;
        [org, user, thread, run, approval]
    }

    /// F-f05-03: a requested approval of a persistent run still waits in the state it was announced in, and it is not
    /// confirmed as any other state.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_requested_approval_still_waits_as_needs_approval() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let [org, user, thread, run, approval] =
            seeded_requested_approval(&pool, &suffix, "persistent").await;

        let announced_as = confirm(
            &org,
            &user,
            &thread,
            &run,
            &approval,
            AttentionState::NeedsApproval,
        );
        assert!(still_waiting(&pool, &announced_as)
            .await
            .expect("confirm the approval"));
        let as_plan = confirm(
            &org,
            &user,
            &thread,
            &run,
            &approval,
            AttentionState::PlanReady,
        );
        assert!(
            !still_waiting(&pool, &as_plan)
                .await
                .expect("confirm as a plan"),
            "a tool approval is not a plan"
        );
    }

    /// F-f05-03: a decided approval no longer waits, so its confirm answers no.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_decided_approval_does_not_wait() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let [org, user, thread, run, approval] =
            seeded_requested_approval(&pool, &suffix, "persistent").await;
        sqlx::query("UPDATE approvals SET status = 'approved', decided_at = now() WHERE id = $1")
            .bind(&approval)
            .execute(&pool)
            .await
            .expect("decide the approval");

        let query = confirm(
            &org,
            &user,
            &thread,
            &run,
            &approval,
            AttentionState::NeedsApproval,
        );
        assert!(!still_waiting(&pool, &query).await.expect("confirm"));
    }

    /// F-f05-03: an approval past its expiry no longer waits, so its confirm answers no.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn an_expired_approval_does_not_wait() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let [org, user, thread, run, approval] =
            seeded_requested_approval(&pool, &suffix, "persistent").await;
        sqlx::query("UPDATE approvals SET expires_at = now() - interval '1 minute' WHERE id = $1")
            .bind(&approval)
            .execute(&pool)
            .await
            .expect("expire the approval");

        let query = confirm(
            &org,
            &user,
            &thread,
            &run,
            &approval,
            AttentionState::NeedsApproval,
        );
        assert!(!still_waiting(&pool, &query).await.expect("confirm"));
    }

    /// F-f05-03: a row of another owner, another org or another run never confirms the item, however live it is.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn another_owners_orgs_or_runs_row_does_not_confirm() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let [org, user, thread, run, approval] =
            seeded_requested_approval(&pool, &suffix, "persistent").await;
        let state = AttentionState::NeedsApproval;
        for query in [
            confirm(&org, "someone-else", &thread, &run, &approval, state),
            confirm("other-org", &user, &thread, &run, &approval, state),
            confirm(&org, &user, &thread, "run-of-no-thread", &approval, state),
        ] {
            assert!(!still_waiting(&pool, &query).await.expect("confirm"));
        }
    }

    /// F-f05-03: a zero-retention or waived run never confirms, however live its approval.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_zero_retention_or_waived_run_never_confirms() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        for retention in ["zdr", "zdr_waived"] {
            let suffix = new_ulid();
            let [org, user, thread, run, approval] =
                seeded_requested_approval(&pool, &suffix, retention).await;
            let query = confirm(
                &org,
                &user,
                &thread,
                &run,
                &approval,
                AttentionState::NeedsApproval,
            );
            assert!(
                !still_waiting(&pool, &query).await.expect("confirm"),
                "{retention} never confirms"
            );
        }
    }

    /// F-f05-03: a proposed plan waits as PLAN_READY until it is decided, and a decided plan answers no.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_proposed_plan_waits_and_a_decided_plan_does_not() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("cf-{suffix}"), "cf-user");
        let (thread, run, plan) = (
            format!("thr_{suffix}"),
            format!("run_{suffix}"),
            format!("plan_{suffix}"),
        );
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "persistent").await;
        seed_plan(&pool, &org, user, &thread, &run, &plan).await;
        let query = confirm(&org, user, &thread, &run, &plan, AttentionState::PlanReady);
        assert!(still_waiting(&pool, &query)
            .await
            .expect("confirm the plan"));

        sqlx::query("UPDATE plans SET status = 'superseded' WHERE id = $1")
            .bind(&plan)
            .execute(&pool)
            .await
            .expect("supersede the plan");
        assert!(!still_waiting(&pool, &query)
            .await
            .expect("confirm the plan"));
    }

    /// F-f05-03: a run question waits as NEEDS_INPUT until it is answered, and an answered question answers no.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_requested_question_waits_and_an_answered_question_does_not() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("cf-{suffix}"), "cf-user");
        let (thread, run, question) = (
            format!("thr_{suffix}"),
            format!("run_{suffix}"),
            format!("q_{suffix}"),
        );
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "persistent").await;
        seed_question(&pool, &org, user, &thread, &run, &question).await;
        let query = confirm(
            &org,
            user,
            &thread,
            &run,
            &question,
            AttentionState::NeedsInput,
        );
        assert!(still_waiting(&pool, &query)
            .await
            .expect("confirm the question"));

        sqlx::query(
            "UPDATE run_questions
             SET state = 'answered', answers = '{}'::jsonb, answers_digest = $2, answered_at = now()
             WHERE id = $1",
        )
        .bind(&question)
        .bind("a".repeat(64))
        .execute(&pool)
        .await
        .expect("answer the question");
        assert!(!still_waiting(&pool, &query)
            .await
            .expect("confirm the question"));
    }

    /// F-f05-03: a stopped goal confirms as COMPLETED while its settled run is the thread's latest run. It is read only
    /// while the thread goals switch is on, and it stops standing once a later run of the thread fails.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_met_goal_confirms_while_its_settled_run_is_the_threads_latest() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("cf-{suffix}"), "cf-user");
        let (thread, run, goal) = (
            format!("thr_{suffix}"),
            format!("run_{suffix}"),
            format!("goal_{suffix}"),
        );
        seed_thread(&pool, &org, user, &thread).await;
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, retention_class)
             VALUES ($1, $2, 'g', $3, $4, 'completed', 'persistent')",
        )
        .bind(&run)
        .bind(&thread)
        .bind(&org)
        .bind(user)
        .execute(&pool)
        .await
        .expect("seed the settled run");
        sqlx::query(
            "INSERT INTO thread_goals
             (id, thread_id, org_id, user_id, objective, max_turns, status, closed_at)
             VALUES ($1, $2, $3, $4, 'collect the notes', 10, 'met', now())",
        )
        .bind(&goal)
        .bind(&thread)
        .bind(&org)
        .bind(user)
        .execute(&pool)
        .await
        .expect("seed the met goal");
        sqlx::query(
            "INSERT INTO thread_goal_turns
             (goal_id, turn_no, run_id, org_id, claim_expires_at, settled_at, outcome, counted)
             VALUES ($1, 1, $2, $3, now() + interval '30 minutes', now(), 'completed', true)",
        )
        .bind(&goal)
        .bind(&run)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("settle the goal's turn");

        let query = confirm(&org, user, &thread, &run, &goal, AttentionState::Completed);
        assert!(still_waiting_with(&pool, &query, true, false)
            .await
            .expect("confirm the goal"));
        assert!(
            !still_waiting_with(&pool, &query, false, false)
                .await
                .expect("confirm the goal with the switch off"),
            "the sweep reads no goal while its switch is off"
        );

        let later = format!("later_{suffix}");
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, retention_class)
             VALUES ($1, $2, 'g', $3, $4, 'failed', 'persistent')",
        )
        .bind(&later)
        .bind(&thread)
        .bind(&org)
        .bind(user)
        .execute(&pool)
        .await
        .expect("seed a later failed run");
        assert!(
            !still_waiting_with(&pool, &query, true, false)
                .await
                .expect("confirm after a later failure"),
            "a met goal does not stand over a later failed run"
        );
    }

    /// Seconds in five hours: due under the four-hour notice, and inside the one-day park TTL.
    const FIVE_HOURS: f64 = 5.0 * 3600.0;

    /// A crash park as the recovery worker leaves one (O-02): the run's own round fold turned into a park, aged
    /// `age_seconds`, under `pause_ref` (none for a park that has no ref). Its wait runs an hour on from the park.
    async fn plant_park(pool: &PgPool, run: &str, pause_ref: Option<&str>, age_seconds: f64) {
        plant_round_fold(pool, run, &fold_for(run), "auth-1").await;
        sqlx::query(
            "UPDATE run_loop_checkpoints
                SET kind = 'park', state = 'parked', pause_ref = $2, park_reason = 'crash_interrupted',
                    park_until = now() + interval '1 hour', updated_at = now() - make_interval(secs => $3)
              WHERE run_id = $1",
        )
        .bind(run)
        .bind(pause_ref)
        .bind(age_seconds)
        .execute(pool)
        .await
        .expect("plant a crash park");
    }

    /// Sets how long a planted park has waited. Its `updated_at` is its age, which the notice and TTL both count from.
    async fn set_park_age(pool: &PgPool, run: &str, age_seconds: f64) {
        sqlx::query(
            "UPDATE run_loop_checkpoints SET updated_at = now() - make_interval(secs => $2) WHERE run_id = $1",
        )
        .bind(run)
        .bind(age_seconds)
        .execute(pool)
        .await
        .expect("age the park");
    }

    /// Whether a park confirms as NEEDS_INPUT, with the crash-park switch on.
    async fn park_waits(pool: &PgPool, query: &ConfirmQuery<'_>) -> bool {
        still_waiting_with(pool, query, false, true)
            .await
            .expect("confirm the park")
    }

    /// O-02: a due crash park confirms as NEEDS_INPUT by the source the notice names. That is its park ref, or the run
    /// when the park has no ref, and never the other one. Its owner's confirm answers yes and another owner's answers
    /// no, and the list names no park while its switch is off, so the confirm answers no then.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_due_crash_park_confirms_by_the_source_the_notice_names() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("cp-{suffix}"), "cp-user");
        let (thread, run, park_ref) = (
            format!("thr_{suffix}"),
            format!("run_{suffix}"),
            format!("park_{suffix}"),
        );
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "persistent").await;
        plant_park(&pool, &run, Some(park_ref.as_str()), FIVE_HOURS).await;

        let by_ref = confirm(
            &org,
            user,
            &thread,
            &run,
            &park_ref,
            AttentionState::NeedsInput,
        );
        assert!(
            park_waits(&pool, &by_ref).await,
            "a due park confirms by its ref"
        );
        assert!(
            !still_waiting_with(&pool, &by_ref, false, false)
                .await
                .expect("confirm with the switch off"),
            "the list names no park while its switch is off"
        );
        let by_run = confirm(&org, user, &thread, &run, &run, AttentionState::NeedsInput);
        assert!(
            !park_waits(&pool, &by_run).await,
            "a park with a ref is named by the ref, not the run"
        );
        let other_owner = confirm(
            &org,
            "cp-other",
            &thread,
            &run,
            &park_ref,
            AttentionState::NeedsInput,
        );
        assert!(
            !park_waits(&pool, &other_owner).await,
            "another owner's confirm of the park answers no"
        );

        // A park that has no ref is named by its run.
        let (bare_thread, bare_run) = (format!("thr_b_{suffix}"), format!("run_b_{suffix}"));
        seed_thread(&pool, &org, user, &bare_thread).await;
        seed_run(&pool, &org, user, &bare_thread, &bare_run, "persistent").await;
        plant_park(&pool, &bare_run, None, FIVE_HOURS).await;
        let bare = confirm(
            &org,
            user,
            &bare_thread,
            &bare_run,
            &bare_run,
            AttentionState::NeedsInput,
        );
        assert!(
            park_waits(&pool, &bare).await,
            "a due park with no ref confirms by its run"
        );
    }

    /// O-02: a crash park stops confirming while it is not yet due, once a continuation claims it, once its wait has
    /// run out, and once a newer run of its thread overtakes it, which is when the list stops naming it.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_crash_park_stops_confirming_when_not_due_claimed_run_out_or_overtaken() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("cp-{suffix}"), "cp-user");
        let (thread, run, park_ref) = (
            format!("thr_{suffix}"),
            format!("run_{suffix}"),
            format!("park_{suffix}"),
        );
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "persistent").await;
        plant_park(&pool, &run, Some(park_ref.as_str()), FIVE_HOURS).await;
        let query = confirm(
            &org,
            user,
            &thread,
            &run,
            &park_ref,
            AttentionState::NeedsInput,
        );
        assert!(park_waits(&pool, &query).await, "a due park waits");

        set_park_age(&pool, &run, 3600.0).await;
        assert!(
            !park_waits(&pool, &query).await,
            "a park that has waited an hour is not yet due"
        );
        set_park_age(&pool, &run, FIVE_HOURS).await;

        sqlx::query("UPDATE run_loop_checkpoints SET state = 'resuming' WHERE run_id = $1")
            .bind(&run)
            .execute(&pool)
            .await
            .expect("a continuation claims the park");
        assert!(
            !park_waits(&pool, &query).await,
            "a claimed park no longer waits"
        );
        sqlx::query("UPDATE run_loop_checkpoints SET state = 'parked' WHERE run_id = $1")
            .bind(&run)
            .execute(&pool)
            .await
            .expect("the claim is released");

        set_park_age(&pool, &run, 25.0 * 3600.0).await;
        assert!(
            !park_waits(&pool, &query).await,
            "a park past its one-day TTL has run out"
        );
        set_park_age(&pool, &run, FIVE_HOURS).await;

        let newer = format!("run_new_{suffix}");
        seed_run(&pool, &org, user, &thread, &newer, "persistent").await;
        sqlx::query("UPDATE runs SET updated_at = now() + interval '1 second' WHERE id = $1")
            .bind(&newer)
            .execute(&pool)
            .await
            .expect("the newer run is the thread's latest");
        assert!(
            !park_waits(&pool, &query).await,
            "a park a newer run has overtaken no longer waits"
        );
    }
}

/// EP-7 S5: the goal item build.
#[cfg(test)]
#[path = "attention_events_goal_tests.rs"]
mod goal_tests;
