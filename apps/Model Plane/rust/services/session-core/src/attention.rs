//! The closed attention state of a thread (build plan WS16 S1).
//!
//! A thread is in exactly one [`AttentionState`]: the most urgent state its facts support. The facts
//! are the thread list's own (archive, latest run, unread) and the rows that wait on a person
//! (approvals, proposed plans, run questions). Nothing here writes, and nothing reads message
//! content, so the state is the same answer on every posture.
//!
//! The unseen marker is the one unread definition of WS17 S7 (`threads.last_read_at`, DEC-11). This
//! module keeps no second seen marker.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{self as pb, AttentionReason, AttentionState};
use sqlx::PgPool;
use tonic::Status;

use crate::grpc::to_proto_timestamp;
use crate::retention_receipt::RetentionClass;

/// The environment switch of the attention surfaces. Only the literal `true` (any case, surrounding
/// whitespace allowed) turns it on, through the same parser as the other durable-store switches.
pub(crate) const ENABLED_ENV: &str = "SESSION_CORE_ATTENTION_ENABLED";

/// The thread goals switch of EP-7 S1. While it is off no goal is read here, so the derivation and
/// the listing are what they were before goals existed. Same literal-`true` parser.
pub(crate) const GOALS_ENABLED_ENV: &str = "SESSION_CORE_THREAD_GOALS_ENABLED";

/// The most threads one `ListAttention` page considers. Also the page a zero or oversized request
/// asks for.
pub(crate) const MAX_PAGE: i64 = 200;

/// The origins a `ListAttention` page covers. Space rooms, support, system and mail-task threads
/// answer to their own surfaces, not to this list.
const LISTED_ORIGINS: [&str; 2] = ["chat", "agent_run"];

/// Run statuses that are final. A tool or cost approval of such a run is moot. A plan approval or a
/// question can outlive the run that raised it, so those are not judged by the run.
const ENDED_RUN_STATUSES: [&str; 3] = ["completed", "failed", "cancelled"];

const IDLE: Derived = Derived {
    state: AttentionState::Idle,
    reason: AttentionReason::Unspecified,
    since: None,
};

/// Whether the attention surfaces are on. Read at each call; the value cannot change while a
/// process runs, so the answer is the same as reading it once at boot.
pub(crate) fn enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// Whether thread goals take part in attention (EP-7 S5). Read at each call, like [`enabled`].
pub(crate) fn goals_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(GOALS_ENABLED_ENV).ok().as_deref())
}

/// The goal statuses the listing reports. `cleared` is a goal the owner removed, so it reads as no
/// goal; a status this build does not know reads as no goal too.
const REPORTED_GOAL_STATUSES: [&str; 6] = [
    "active",
    "paused",
    "met",
    "budget_reached",
    "turns_reached",
    "blocked",
];

/// The state and reason a goal in this status raises, or `None` for a goal that does not wait on
/// anyone (EP-7 S5). A met goal reads as COMPLETED; a goal stopped by its budget, its turn cap or a
/// blocked verdict reads as NEEDS_INPUT, so no new state is needed.
pub(crate) fn goal_attention(status: &str) -> Option<(AttentionState, AttentionReason)> {
    match status {
        "met" => Some((AttentionState::Completed, AttentionReason::Goal)),
        "budget_reached" | "turns_reached" | "blocked" => {
            Some((AttentionState::NeedsInput, AttentionReason::Goal))
        }
        _ => None,
    }
}

/// How urgent a state is; a higher number wins. What waits for the owner outranks what runs, and
/// what runs outranks what has finished (build plan WS16 S1, DEC-11).
pub(crate) fn precedence(state: AttentionState) -> u8 {
    match state {
        AttentionState::NeedsApproval => 8,
        AttentionState::NeedsInput => 7,
        AttentionState::PlanReady => 6,
        AttentionState::Working => 5,
        AttentionState::Failed => 4,
        AttentionState::Completed => 3,
        AttentionState::Archived => 2,
        AttentionState::Idle => 1,
        AttentionState::Unspecified => 0,
    }
}

/// The state and reason a pending approval of this kind raises, or `None` for a kind that raises
/// nothing. WS14 takes no approval kind (questions are rows of their own), and the coordinated
/// Space/WS13 kind has no name yet, so an unnamed kind raises nothing.
pub(crate) fn kind_attention(kind: &str) -> Option<(AttentionState, AttentionReason)> {
    match kind {
        "plan" => Some((AttentionState::PlanReady, AttentionReason::Plan)),
        "tool_call" | "permission" | "destructive" | "cost" => {
            Some((AttentionState::NeedsApproval, AttentionReason::Approval))
        }
        _ => None,
    }
}

/// The state a latest run puts its thread in. A cancelled run and a status this code does not know
/// read as no state, so the thread is idle.
fn run_state(status: &str) -> Option<AttentionState> {
    match status {
        "queued" | "running" | "awaiting_approval" => Some(AttentionState::Working),
        "completed" => Some(AttentionState::Completed),
        "failed" => Some(AttentionState::Failed),
        _ => None,
    }
}

/// Whether a caller of this retention class may mark a thread read. Only a persistent caller may:
/// a zero-retention caller's read state stays on the client, and a waived run is not persistent
/// (DEC-32).
pub(crate) fn mark_seen_permitted(class: RetentionClass) -> bool {
    class == RetentionClass::Persistent
}

/// The page a `ListAttention` request asks for: one to [`MAX_PAGE`]. Zero, a negative number and an
/// oversized number all read as the maximum.
pub(crate) fn page_size(requested: i32) -> i64 {
    match i64::from(requested) {
        size @ 1..=MAX_PAGE => size,
        _ => MAX_PAGE,
    }
}

/// What the thread list already says about one thread.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ThreadFacts<'a> {
    /// When the owner archived the thread.
    pub(crate) archived_at: Option<DateTime<Utc>>,
    /// The latest run's status. Empty when the thread has no run.
    pub(crate) latest_run_status: &'a str,
    /// When the latest run last changed.
    pub(crate) latest_run_at: Option<DateTime<Utc>>,
}

/// One pending approval of a thread's runs, with what decides whether it still counts.
#[derive(Debug, Clone)]
pub(crate) struct PendingApproval {
    pub(crate) kind: String,
    /// The status of the run the approval belongs to.
    pub(crate) run_status: String,
    /// The status of the plan the approval names, when it names one.
    pub(crate) plan_status: Option<String>,
    pub(crate) requested_at: DateTime<Utc>,
}

/// The rows that wait on a person, for one thread. Each query only reads rows that are still open
/// (requested, proposed, pending and not expired), so nothing here is a settled row.
#[derive(Debug, Clone, Default)]
pub(crate) struct Pending {
    pub(crate) approvals: Vec<PendingApproval>,
    /// When each proposed plan was proposed.
    pub(crate) proposed_plans: Vec<DateTime<Utc>>,
    /// When each pending question was asked.
    pub(crate) questions: Vec<DateTime<Utc>>,
    /// The thread's latest goal (EP-7 S5). `None` while the goals switch is off.
    pub(crate) goal: Option<GoalFact>,
}

/// The thread's latest goal: its closed status and when that status was last written.
#[derive(Debug, Clone)]
pub(crate) struct GoalFact {
    pub(crate) status: String,
    pub(crate) since: DateTime<Utc>,
}

/// The attention of one thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Derived {
    pub(crate) state: AttentionState,
    pub(crate) reason: AttentionReason,
    /// When the state began. Only IDLE has none.
    pub(crate) since: Option<DateTime<Utc>>,
}

/// The most urgent candidate. Among equally urgent ones the oldest wins, so a waiting list reads
/// oldest first.
fn most_urgent(candidates: impl Iterator<Item = Derived>) -> Derived {
    candidates
        .max_by(|a, b| {
            precedence(a.state)
                .cmp(&precedence(b.state))
                .then_with(|| b.since.cmp(&a.since))
        })
        .unwrap_or(IDLE)
}

/// The attention of one thread from its facts and its pending rows. Pure: the database reads come
/// in through [`Pending`].
///
/// An approval counts only while it is live. A tool, permission, destructive or cost approval of an
/// ended run is moot. A plan approval counts only while its plan is still proposed, so the approval
/// of a superseded plan is ignored. A question counts while it is pending, whatever became of its
/// run.
pub(crate) fn derive(facts: &ThreadFacts<'_>, pending: &Pending) -> Derived {
    let approvals = pending.approvals.iter().filter_map(|approval| {
        let (state, reason) = kind_attention(&approval.kind)?;
        let live = match state {
            AttentionState::PlanReady => approval.plan_status.as_deref() == Some("proposed"),
            _ => !ENDED_RUN_STATUSES.contains(&approval.run_status.as_str()),
        };
        live.then_some(Derived {
            state,
            reason,
            since: Some(approval.requested_at),
        })
    });
    let plans = pending.proposed_plans.iter().map(|&at| Derived {
        state: AttentionState::PlanReady,
        reason: AttentionReason::Plan,
        since: Some(at),
    });
    let questions = pending.questions.iter().map(|&at| Derived {
        state: AttentionState::NeedsInput,
        reason: AttentionReason::Input,
        since: Some(at),
    });
    let run = run_state(facts.latest_run_status).map(|state| Derived {
        state,
        reason: AttentionReason::Unspecified,
        since: facts.latest_run_at,
    });
    // A stopped goal speaks for the thread only while no other row waits on the owner (EP-7 S5) and
    // no run is working, since a working run means someone already acted. It then stands in for the
    // latest run: a met goal only over a completed run (a later failed run stays FAILED), a goal
    // stopped by a cap or a blocked verdict over any finished run.
    let has_live_pending = approvals.clone().next().is_some()
        || !pending.proposed_plans.is_empty()
        || !pending.questions.is_empty();
    let goal = pending
        .goal
        .as_ref()
        .filter(|_| !has_live_pending)
        .and_then(|goal| {
            let (state, reason) = goal_attention(&goal.status)?;
            let replaces_run = match (state, run.map(|run| run.state)) {
                (_, Some(AttentionState::Working)) => false,
                (AttentionState::Completed, Some(AttentionState::Failed)) => false,
                _ => true,
            };
            replaces_run.then_some(Derived {
                state,
                reason,
                since: Some(goal.since),
            })
        });
    let run = if goal.is_some() { None } else { run };
    let archived = facts.archived_at.map(|at| Derived {
        state: AttentionState::Archived,
        reason: AttentionReason::Unspecified,
        since: Some(at),
    });
    most_urgent(
        approvals
            .chain(plans)
            .chain(questions)
            .chain(goal)
            .chain(run)
            .chain(archived)
            .chain(std::iter::once(IDLE)),
    )
}

/// Reads the pending rows of a set of threads in one org, in three batched queries.
async fn fetch_pending(
    pool: &PgPool,
    org_id: &str,
    thread_ids: &[String],
) -> Result<HashMap<String, Pending>, Status> {
    let approvals: Vec<(String, String, String, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT r.thread_id, a.kind, r.status, p.status, a.requested_at
             FROM approvals a
             JOIN runs r ON r.id = a.run_id
             LEFT JOIN plans p ON p.id = a.plan_id
             WHERE a.org_id = $1
               AND r.thread_id = ANY($2)
               AND a.status = 'requested'
               AND (a.expires_at IS NULL OR a.expires_at > now())",
    )
    .bind(org_id)
    .bind(thread_ids.to_vec())
    .fetch_all(pool)
    .await
    .map_err(read_error)?;
    let plans: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT thread_id, updated_at
         FROM plans
         WHERE org_id = $1 AND thread_id = ANY($2) AND status = 'proposed'",
    )
    .bind(org_id)
    .bind(thread_ids.to_vec())
    .fetch_all(pool)
    .await
    .map_err(read_error)?;
    let questions: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT thread_id, requested_at
         FROM run_questions
         WHERE org_id = $1
           AND thread_id = ANY($2)
           AND state = 'requested'
           AND expires_at > now()",
    )
    .bind(org_id)
    .bind(thread_ids.to_vec())
    .fetch_all(pool)
    .await
    .map_err(read_error)?;

    let mut pending: HashMap<String, Pending> = HashMap::new();
    for (thread_id, kind, run_status, plan_status, requested_at) in approvals {
        pending
            .entry(thread_id)
            .or_default()
            .approvals
            .push(PendingApproval {
                kind,
                run_status,
                plan_status,
                requested_at,
            });
    }
    for (thread_id, at) in plans {
        pending
            .entry(thread_id)
            .or_default()
            .proposed_plans
            .push(at);
    }
    for (thread_id, at) in questions {
        pending.entry(thread_id).or_default().questions.push(at);
    }
    if goals_enabled() {
        for (thread_id, goal) in fetch_goals(pool, org_id, thread_ids).await? {
            pending.entry(thread_id).or_default().goal = Some(goal);
        }
    }
    Ok(pending)
}

/// Reads the latest goal of each thread in one query (EP-7 S5): its status and when that status was
/// last written. A cleared goal or an unknown status reads as no goal. Status tokens and times only;
/// the objective is never read here.
async fn fetch_goals(
    pool: &PgPool,
    org_id: &str,
    thread_ids: &[String],
) -> Result<Vec<(String, GoalFact)>, Status> {
    let rows: Vec<(String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT DISTINCT ON (thread_id) thread_id, status, COALESCE(closed_at, updated_at)
         FROM thread_goals
         WHERE org_id = $1 AND thread_id = ANY($2)
         ORDER BY thread_id, updated_at DESC, created_at DESC, id DESC",
    )
    .bind(org_id)
    .bind(thread_ids.to_vec())
    .fetch_all(pool)
    .await
    .map_err(read_error)?;
    Ok(rows
        .into_iter()
        .filter(|(_, status, _)| REPORTED_GOAL_STATUSES.contains(&status.as_str()))
        .map(|(thread_id, status, since)| (thread_id, GoalFact { status, since }))
        .collect())
}

fn read_error(error: sqlx::Error) -> Status {
    tracing::warn!(%error, "attention read failed");
    Status::internal(error.to_string())
}

/// The instant a protobuf timestamp names, or `None` for one this build cannot represent.
fn datetime_of(timestamp: &prost_types::Timestamp) -> Option<DateTime<Utc>> {
    u32::try_from(timestamp.nanos)
        .ok()
        .and_then(|nanos| DateTime::<Utc>::from_timestamp(timestamp.seconds, nanos))
}

/// Sets the attention fields of a thread listing. One batched read of the pending rows for the page;
/// the listing query itself does not change. Called only while the switch is on.
pub(crate) async fn enrich(
    pool: &PgPool,
    org_id: &str,
    threads: &mut [pb::ThreadSummary],
) -> Result<(), Status> {
    if threads.is_empty() {
        return Ok(());
    }
    let thread_ids: Vec<String> = threads
        .iter()
        .map(|thread| thread.thread_id.clone())
        .collect();
    let mut pending = fetch_pending(pool, org_id, &thread_ids).await?;
    for thread in threads.iter_mut() {
        let rows = pending.remove(&thread.thread_id).unwrap_or_default();
        thread.goal_status = rows
            .goal
            .as_ref()
            .map(|goal| goal.status.clone())
            .unwrap_or_default();
        let facts = ThreadFacts {
            archived_at: thread.archived_at.as_ref().and_then(datetime_of),
            latest_run_status: &thread.latest_run_status,
            latest_run_at: thread.latest_run_updated_at.as_ref().and_then(datetime_of),
        };
        let derived = derive(&facts, &rows);
        thread.attention_state = derived.state as i32;
        thread.attention_since = derived.since.map(to_proto_timestamp);
        thread.attention_unseen = thread.unread;
    }
    Ok(())
}

/// One `ListAttention` page: the owner's unarchived threads in the listed origins, the state each is
/// in, and the counts over them. A thread that is idle is counted but is not an item.
pub(crate) async fn list(
    pool: &PgPool,
    org_id: &str,
    user_id: &str,
    limit: i64,
) -> Result<pb::ListAttentionResponse, Status> {
    #[derive(sqlx::FromRow)]
    struct ThreadRow {
        id: String,
        unread: bool,
        latest_run_id: Option<String>,
        latest_run_status: Option<String>,
        latest_run_updated_at: Option<DateTime<Utc>>,
    }

    let origins: Vec<String> = LISTED_ORIGINS
        .iter()
        .map(|origin| (*origin).to_owned())
        .collect();
    let rows: Vec<ThreadRow> = sqlx::query_as(
        "SELECT t.id,
                (t.last_assistant_at IS NOT NULL AND t.last_assistant_at > t.last_read_at) AS unread,
                latest.id AS latest_run_id,
                latest.status AS latest_run_status,
                latest.updated_at AS latest_run_updated_at
         FROM threads t
         LEFT JOIN LATERAL (
            SELECT id, status, updated_at
            FROM runs
            WHERE thread_id = t.id
            ORDER BY updated_at DESC, created_at DESC, id DESC
            LIMIT 1
         ) latest ON TRUE
         WHERE t.org_id = $1
           AND t.user_id = $2
           AND t.origin = ANY($3)
           AND t.archived_at IS NULL
         ORDER BY t.last_activity_at DESC, t.id DESC
         LIMIT $4",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(origins)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(read_error)?;

    let thread_ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let mut pending = fetch_pending(pool, org_id, &thread_ids).await?;
    let mut counts = pb::AttentionCounts::default();
    let mut listed: Vec<(Derived, pb::AttentionItem)> = Vec::new();
    for row in rows {
        let rows_pending = pending.remove(&row.id).unwrap_or_default();
        let latest_run_status = row.latest_run_status.unwrap_or_default();
        let facts = ThreadFacts {
            archived_at: None,
            latest_run_status: &latest_run_status,
            latest_run_at: row.latest_run_updated_at,
        };
        let derived = derive(&facts, &rows_pending);
        if row.unread {
            counts.unseen += 1;
        }
        match derived.state {
            AttentionState::NeedsApproval
            | AttentionState::NeedsInput
            | AttentionState::PlanReady => counts.needs_you += 1,
            AttentionState::Working => counts.working += 1,
            _ => {}
        }
        if derived.state != AttentionState::Idle {
            let item = pb::AttentionItem {
                thread_id: row.id,
                state: derived.state as i32,
                reason: derived.reason as i32,
                since: derived.since.map(to_proto_timestamp),
                unseen: row.unread,
                run_id: row.latest_run_id.unwrap_or_default(),
            };
            listed.push((derived, item));
        }
    }
    // Most urgent first; within a state, oldest waiting first, then by id so a page is stable.
    listed.sort_by(|(a, x), (b, y)| {
        precedence(b.state)
            .cmp(&precedence(a.state))
            .then_with(|| a.since.cmp(&b.since))
            .then_with(|| x.thread_id.cmp(&y.thread_id))
    });
    Ok(pb::ListAttentionResponse {
        enabled: true,
        items: listed.into_iter().map(|(_, item)| item).collect(),
        counts: Some(counts),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const ALL_STATES: [AttentionState; 9] = [
        AttentionState::Unspecified,
        AttentionState::NeedsApproval,
        AttentionState::NeedsInput,
        AttentionState::PlanReady,
        AttentionState::Working,
        AttentionState::Completed,
        AttentionState::Failed,
        AttentionState::Archived,
        AttentionState::Idle,
    ];

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).unwrap()
    }

    fn running_facts(status: &str) -> ThreadFacts<'_> {
        ThreadFacts {
            archived_at: None,
            latest_run_status: status,
            latest_run_at: Some(at(100)),
        }
    }

    fn approval(
        kind: &str,
        run_status: &str,
        plan_status: Option<&str>,
        requested: i64,
    ) -> PendingApproval {
        PendingApproval {
            kind: kind.to_owned(),
            run_status: run_status.to_owned(),
            plan_status: plan_status.map(str::to_owned),
            requested_at: at(requested),
        }
    }

    #[test]
    fn the_state_numbers_are_the_ones_the_contract_names() {
        assert_eq!(AttentionState::Unspecified as i32, 0);
        assert_eq!(AttentionState::NeedsApproval as i32, 1);
        assert_eq!(AttentionState::NeedsInput as i32, 2);
        assert_eq!(AttentionState::PlanReady as i32, 3);
        assert_eq!(AttentionState::Working as i32, 4);
        assert_eq!(AttentionState::Completed as i32, 5);
        assert_eq!(AttentionState::Failed as i32, 6);
        assert_eq!(AttentionState::Archived as i32, 7);
        assert_eq!(AttentionState::Idle as i32, 8);
    }

    #[test]
    fn every_state_survives_the_wire_and_an_unknown_number_is_refused() {
        for state in ALL_STATES {
            assert_eq!(AttentionState::try_from(state as i32), Ok(state));
        }
        assert!(AttentionState::try_from(9).is_err());
    }

    #[test]
    fn precedence_is_the_plan_order_from_the_most_urgent_down() {
        let order = [
            AttentionState::NeedsApproval,
            AttentionState::NeedsInput,
            AttentionState::PlanReady,
            AttentionState::Working,
            AttentionState::Failed,
            AttentionState::Completed,
            AttentionState::Archived,
            AttentionState::Idle,
        ];
        for pair in order.windows(2) {
            assert!(
                precedence(pair[0]) > precedence(pair[1]),
                "{:?} must outrank {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn every_pair_of_states_resolves_to_the_more_urgent_one() {
        for a in ALL_STATES {
            for b in ALL_STATES {
                let candidate = |state| Derived {
                    state,
                    reason: AttentionReason::Unspecified,
                    since: Some(at(5)),
                };
                let won = most_urgent([candidate(a), candidate(b)].into_iter()).state;
                let expected = if precedence(a) >= precedence(b) { a } else { b };
                assert_eq!(won, expected, "{a:?} against {b:?}");
            }
        }
    }

    #[test]
    fn among_equally_urgent_candidates_the_oldest_waits_first() {
        let newer = Derived {
            state: AttentionState::NeedsApproval,
            reason: AttentionReason::Approval,
            since: Some(at(200)),
        };
        let older = Derived {
            since: Some(at(150)),
            ..newer
        };
        assert_eq!(most_urgent([newer, older].into_iter()).since, Some(at(150)));
    }

    #[test]
    fn approval_kinds_map_to_their_states_and_an_unnamed_kind_raises_nothing() {
        assert_eq!(
            kind_attention("plan"),
            Some((AttentionState::PlanReady, AttentionReason::Plan))
        );
        for kind in ["tool_call", "permission", "destructive", "cost"] {
            assert_eq!(
                kind_attention(kind),
                Some((AttentionState::NeedsApproval, AttentionReason::Approval)),
                "{kind}"
            );
        }
        // The coordinated Space/WS13 kind (4.3 gives it number 6) has no name yet, and WS14 takes
        // no approval kind, so neither raises a state here.
        assert_eq!(kind_attention("quarantine"), None);
        assert_eq!(kind_attention(""), None);
    }

    #[test]
    fn a_cancelled_latest_run_is_idle() {
        let derived = derive(&running_facts("cancelled"), &Pending::default());
        assert_eq!(derived.state, AttentionState::Idle);
        assert_eq!(derived.since, None);
    }

    #[test]
    fn a_pending_approval_of_a_cancelled_run_does_not_wake_the_thread() {
        let pending = Pending {
            approvals: vec![approval("tool_call", "cancelled", None, 120)],
            ..Pending::default()
        };
        assert_eq!(
            derive(&running_facts("cancelled"), &pending).state,
            AttentionState::Idle
        );
    }

    #[test]
    fn a_pending_tool_approval_of_a_running_run_needs_approval_oldest_first() {
        let pending = Pending {
            approvals: vec![
                approval("tool_call", "running", None, 200),
                approval("cost", "running", None, 150),
            ],
            ..Pending::default()
        };
        let derived = derive(&running_facts("running"), &pending);
        assert_eq!(derived.state, AttentionState::NeedsApproval);
        assert_eq!(derived.reason, AttentionReason::Approval);
        assert_eq!(derived.since, Some(at(150)));
    }

    #[test]
    fn a_superseded_plan_approval_is_ignored() {
        let pending = Pending {
            approvals: vec![approval("plan", "completed", Some("superseded"), 120)],
            ..Pending::default()
        };
        assert_eq!(
            derive(&running_facts("completed"), &pending).state,
            AttentionState::Completed
        );
    }

    #[test]
    fn a_proposed_plan_is_plan_ready_even_though_its_run_ended() {
        let pending = Pending {
            proposed_plans: vec![at(120)],
            ..Pending::default()
        };
        let derived = derive(&running_facts("completed"), &pending);
        assert_eq!(derived.state, AttentionState::PlanReady);
        assert_eq!(derived.reason, AttentionReason::Plan);
        assert_eq!(derived.since, Some(at(120)));
    }

    #[test]
    fn a_pending_question_is_needs_input_even_though_its_run_ended() {
        let pending = Pending {
            questions: vec![at(130)],
            ..Pending::default()
        };
        let derived = derive(&running_facts("completed"), &pending);
        assert_eq!(derived.state, AttentionState::NeedsInput);
        assert_eq!(derived.reason, AttentionReason::Input);
    }

    #[test]
    fn a_waiting_state_outranks_a_running_run_and_an_archive() {
        let pending = Pending {
            approvals: vec![approval("tool_call", "running", None, 100)],
            ..Pending::default()
        };
        let facts = ThreadFacts {
            archived_at: Some(at(90)),
            ..running_facts("running")
        };
        assert_eq!(
            derive(&facts, &pending).state,
            AttentionState::NeedsApproval
        );
    }

    #[test]
    fn a_run_reads_as_working_failed_or_completed_by_its_status() {
        assert_eq!(
            derive(&running_facts("queued"), &Pending::default()).state,
            AttentionState::Working
        );
        assert_eq!(
            derive(&running_facts("running"), &Pending::default()).state,
            AttentionState::Working
        );
        assert_eq!(
            derive(&running_facts("failed"), &Pending::default()).state,
            AttentionState::Failed
        );
        assert_eq!(
            derive(&running_facts("completed"), &Pending::default()).state,
            AttentionState::Completed
        );
    }

    #[test]
    fn an_unknown_run_status_reads_as_no_state() {
        assert_eq!(
            derive(&running_facts("paused_somehow"), &Pending::default()).state,
            AttentionState::Idle
        );
    }

    #[test]
    fn an_archived_thread_with_no_run_is_archived_and_a_thread_with_nothing_is_idle() {
        let archived = ThreadFacts {
            archived_at: Some(at(90)),
            latest_run_status: "",
            latest_run_at: None,
        };
        let derived = derive(&archived, &Pending::default());
        assert_eq!(derived.state, AttentionState::Archived);
        assert_eq!(derived.since, Some(at(90)));
        let empty = ThreadFacts {
            archived_at: None,
            latest_run_status: "",
            latest_run_at: None,
        };
        assert_eq!(
            derive(&empty, &Pending::default()).state,
            AttentionState::Idle
        );
    }

    #[test]
    fn mark_seen_is_permitted_for_a_persistent_caller_only() {
        assert!(mark_seen_permitted(RetentionClass::Persistent));
        assert!(!mark_seen_permitted(RetentionClass::Zdr));
        assert!(!mark_seen_permitted(RetentionClass::ZdrWaived));
    }

    #[test]
    fn the_page_is_one_to_two_hundred_and_anything_else_reads_as_the_maximum() {
        assert_eq!(page_size(1), 1);
        assert_eq!(page_size(200), 200);
        assert_eq!(page_size(0), MAX_PAGE);
        assert_eq!(page_size(-5), MAX_PAGE);
        assert_eq!(page_size(201), MAX_PAGE);
    }

    #[test]
    fn the_switch_is_named_for_this_slice() {
        assert_eq!(ENABLED_ENV, "SESSION_CORE_ATTENTION_ENABLED");
    }

    fn goal(status: &str, seconds: i64) -> Pending {
        Pending {
            goal: Some(GoalFact {
                status: status.to_owned(),
                since: at(seconds),
            }),
            ..Pending::default()
        }
    }

    #[test]
    fn attention_goal_reason_takes_the_number_the_contract_names() {
        assert_eq!(AttentionReason::Goal as i32, 5);
        assert_eq!(
            AttentionReason::try_from(5),
            Ok(AttentionReason::Goal),
            "4 stays reserved for the WS03 parked loop"
        );
        assert!(AttentionReason::try_from(4).is_err());
    }

    #[test]
    fn attention_goal_met_is_completed_goal() {
        let derived = derive(&running_facts("completed"), &goal("met", 110));
        assert_eq!(derived.state, AttentionState::Completed);
        assert_eq!(derived.reason, AttentionReason::Goal);
        assert_eq!(derived.since, Some(at(110)));
    }

    #[test]
    fn attention_goal_budget_reached_needs_input() {
        for status in ["budget_reached", "turns_reached", "blocked"] {
            let derived = derive(&running_facts("completed"), &goal(status, 110));
            assert_eq!(derived.state, AttentionState::NeedsInput, "{status}");
            assert_eq!(derived.reason, AttentionReason::Goal, "{status}");
            assert_eq!(derived.since, Some(at(110)), "{status}");
        }
    }

    #[test]
    fn attention_goal_stopped_outranks_a_failed_run_and_needs_no_run() {
        let derived = derive(&running_facts("failed"), &goal("turns_reached", 110));
        assert_eq!(derived.state, AttentionState::NeedsInput);
        let no_run = ThreadFacts {
            archived_at: None,
            latest_run_status: "",
            latest_run_at: None,
        };
        let derived = derive(&no_run, &goal("blocked", 110));
        assert_eq!(derived.reason, AttentionReason::Goal);
    }

    #[test]
    fn attention_goal_met_does_not_hide_a_later_failed_run() {
        let derived = derive(&running_facts("failed"), &goal("met", 90));
        assert_eq!(derived.state, AttentionState::Failed);
        assert_eq!(derived.reason, AttentionReason::Unspecified);
    }

    #[test]
    fn attention_goal_yields_to_a_working_run() {
        for status in ["met", "budget_reached", "blocked"] {
            let derived = derive(&running_facts("running"), &goal(status, 110));
            assert_eq!(derived.state, AttentionState::Working, "{status}");
            assert_eq!(derived.reason, AttentionReason::Unspecified, "{status}");
        }
    }

    #[test]
    fn attention_goal_yields_to_any_other_pending_row() {
        let mut pending = goal("budget_reached", 50);
        pending.questions = vec![at(130)];
        let derived = derive(&running_facts("completed"), &pending);
        assert_eq!(derived.state, AttentionState::NeedsInput);
        assert_eq!(derived.reason, AttentionReason::Input);

        let mut pending = goal("met", 50);
        pending.proposed_plans = vec![at(130)];
        assert_eq!(
            derive(&running_facts("completed"), &pending).reason,
            AttentionReason::Plan
        );

        let mut pending = goal("blocked", 50);
        pending.approvals = vec![approval("tool_call", "awaiting_approval", None, 130)];
        assert_eq!(
            derive(&running_facts("awaiting_approval"), &pending).reason,
            AttentionReason::Approval
        );
    }

    #[test]
    fn attention_goal_open_or_unknown_status_raises_nothing() {
        for status in ["active", "paused", "cleared", "judged", ""] {
            assert_eq!(goal_attention(status), None, "{status}");
            let derived = derive(&running_facts("completed"), &goal(status, 110));
            assert_eq!(derived.state, AttentionState::Completed, "{status}");
            assert_eq!(derived.reason, AttentionReason::Unspecified, "{status}");
        }
    }

    #[test]
    fn attention_goal_absent_derives_exactly_as_before() {
        for status in ["queued", "running", "completed", "failed", "cancelled", ""] {
            let facts = running_facts(status);
            let derived = derive(&facts, &Pending::default());
            let expected = match run_state(status) {
                Some(state) => Derived {
                    state,
                    reason: AttentionReason::Unspecified,
                    since: Some(at(100)),
                },
                None => IDLE,
            };
            assert_eq!(derived, expected, "{status}");
        }
    }

    #[test]
    fn attention_goal_reported_statuses_exclude_cleared() {
        assert!(!REPORTED_GOAL_STATUSES.contains(&"cleared"));
        for status in ["met", "budget_reached", "turns_reached", "blocked"] {
            assert!(REPORTED_GOAL_STATUSES.contains(&status), "{status}");
        }
    }

    #[test]
    fn attention_goal_switch_is_the_thread_goals_switch_and_reads_literal_true_only() {
        assert_eq!(GOALS_ENABLED_ENV, "SESSION_CORE_THREAD_GOALS_ENABLED");
        let parse = |value: Option<&str>| mp_contracts::loop_fold::switch_from(value);
        assert!(parse(Some("true")));
        for value in [None, Some(""), Some("1"), Some("yes"), Some("on"), Some("false")] {
            assert!(!parse(value), "{value:?}");
        }
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

    async fn seed_run(pool: &PgPool, org: &str, user: &str, thread: &str, run: &str, status: &str) {
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status) VALUES ($1, $2, 'g', $3, $4, $5)",
        )
        .bind(run)
        .bind(thread)
        .bind(org)
        .bind(user)
        .bind(status)
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
        expires_at: Option<DateTime<Utc>>,
    ) {
        sqlx::query(
            "INSERT INTO approvals (id, run_id, plan_id, kind, status, org_id, user_id, requested_at, expires_at)
             VALUES ($1, $2, $3, $4, 'requested', $5, $6, now() - interval '5 minutes', $7)",
        )
        .bind(id)
        .bind(run)
        .bind(plan)
        .bind(kind)
        .bind(org)
        .bind(user)
        .bind(expires_at)
        .execute(pool)
        .await
        .expect("seed approval");
    }

    fn listing_row(thread: &str, latest_run_status: &str) -> pb::ThreadSummary {
        pb::ThreadSummary {
            thread_id: thread.to_owned(),
            latest_run_status: latest_run_status.to_owned(),
            ..Default::default()
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_pending_approval_on_a_running_run_is_needs_approval() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("attn-{suffix}"), "attn-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "running").await;
        seed_approval(
            &pool,
            &org,
            user,
            &run,
            &format!("apr_{suffix}"),
            "tool_call",
            None,
            None,
        )
        .await;

        let mut threads = vec![listing_row(&thread, "running")];
        enrich(&pool, &org, &mut threads).await.expect("enrich");
        assert_eq!(
            threads[0].attention_state,
            AttentionState::NeedsApproval as i32
        );
        assert!(!threads[0].attention_unseen);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn an_expired_approval_is_ignored() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("attn-{suffix}"), "attn-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "running").await;
        let past = Utc::now() - chrono::Duration::hours(1);
        seed_approval(
            &pool,
            &org,
            user,
            &run,
            &format!("apr_{suffix}"),
            "tool_call",
            None,
            Some(past),
        )
        .await;

        let mut threads = vec![listing_row(&thread, "running")];
        enrich(&pool, &org, &mut threads).await.expect("enrich");
        assert_eq!(threads[0].attention_state, AttentionState::Working as i32);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_superseded_plan_is_ignored() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("attn-{suffix}"), "attn-user");
        let (thread, run, plan) = (
            format!("thr_{suffix}"),
            format!("run_{suffix}"),
            format!("pln_{suffix}"),
        );
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "completed").await;
        sqlx::query("INSERT INTO plans (id, thread_id, status, org_id, user_id) VALUES ($1, $2, 'superseded', $3, $4)")
            .bind(&plan)
            .bind(&thread)
            .bind(&org)
            .bind(user)
            .execute(&pool)
            .await
            .expect("seed plan");
        seed_approval(
            &pool,
            &org,
            user,
            &run,
            &format!("apr_{suffix}"),
            "plan",
            Some(plan.as_str()),
            None,
        )
        .await;

        let mut threads = vec![listing_row(&thread, "completed")];
        enrich(&pool, &org, &mut threads).await.expect("enrich");
        assert_eq!(threads[0].attention_state, AttentionState::Completed as i32);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn another_tenants_approval_does_not_reach_this_thread() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, other_org, user) = (
            format!("attn-{suffix}"),
            format!("attn-other-{suffix}"),
            "attn-user",
        );
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "running").await;
        seed_approval(
            &pool,
            &org,
            user,
            &run,
            &format!("apr_{suffix}"),
            "tool_call",
            None,
            None,
        )
        .await;

        let mut threads = vec![listing_row(&thread, "running")];
        enrich(&pool, &other_org, &mut threads)
            .await
            .expect("enrich");
        assert_eq!(threads[0].attention_state, AttentionState::Working as i32);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn the_list_marks_an_unread_reply_and_counts_it() {
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let suffix = new_ulid();
        let (org, user) = (format!("attn-{suffix}"), "attn-user");
        let (thread, run) = (format!("thr_{suffix}"), format!("run_{suffix}"));
        seed_thread(&pool, &org, user, &thread).await;
        seed_run(&pool, &org, user, &thread, &run, "completed").await;
        sqlx::query(
            "UPDATE threads SET last_assistant_at = now(), last_read_at = now() - interval '1 hour' WHERE id = $1",
        )
        .bind(&thread)
        .execute(&pool)
        .await
        .expect("mark unread");

        let page = list(&pool, &org, user, MAX_PAGE).await.expect("list");
        let counts = page.counts.expect("counts");
        assert_eq!(counts.unseen, 1);
        let item = page
            .items
            .iter()
            .find(|item| item.thread_id == thread)
            .expect("item");
        assert!(item.unseen);
        assert_eq!(item.state, AttentionState::Completed as i32);
    }
}
