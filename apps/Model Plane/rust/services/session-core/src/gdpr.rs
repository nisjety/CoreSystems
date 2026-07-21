//! Cross-plane GDPR organization-erasure purge.
//!
//! `org-core`'s `PublishGDPRErasureFanout` (both the explicit immediate
//! hard-delete path and the 30-day retention cron) and `user-core`'s
//! per-user erasure saga both publish onto the shared
//! `velion.gdpr.erasure.requested` fan-out. This module decides which of
//! those events this crate must act on, and performs the actual hard-purge
//! of every org-scoped row session-core owns: threads/messages,
//! runs/checkpoints/subagent edges, plans/plan_steps/todos/approvals, the
//! append-only event log, tasks and their cron/dependency/artifact rows,
//! hook configs, agent skills/memory, fine-tune jobs, dream runs, and any
//! not-yet-delivered audit-outbox rows for the org.
//!
//! Scope note: session-core does NOT own Model Plane cost/usage records —
//! those live in the sibling `cost-core` Go service's own Postgres database
//! (`apps/Model Plane/go/services/cost-core`), unreachable from this crate.
//! `cost-core` has no subscriber on this subject today; purging an erased
//! org's cost/usage rows there is a follow-up outside this crate's reach.
//!
//! Safety: the fan-out subject also carries **per-user** erasure events
//! (`subject_type: "user"` / `"user_anonymize"`, see `user-core`'s
//! `erasureFanoutPayload`) whose `org_id` is simply the user's org — it is
//! NOT a request to erase that org. Only `subject_type == "organization"`
//! (org-core's `PublishGDPRErasureFanout` shape) triggers the purge in this
//! module; every other subject type is a deliberate no-op. Treating a
//! per-user erasure's `org_id` as "purge this org" would delete every other
//! member's threads/runs over one member's personal-data request.
//!
//! Idempotency: NATS is at-least-once delivery. Every statement below is a
//! `DELETE ... WHERE org_id = $1` (or a subquery scoped the same way), so a
//! redelivered event is a no-op the second time — matching zero rows is not
//! an error.

use serde::Deserialize;
use sqlx::{Postgres, Transaction};

use crate::store::Pool;

const MAX_ID_LEN: usize = 255;

/// Wire shape of one `velion.gdpr.erasure.requested` message. Producers
/// (`org-core`, `user-core`) do not emit the same optional field set, so
/// only the fields this module reads are required to be present at all —
/// everything else is optional-and-ignored rather than rejected.
#[derive(Debug, Clone, Deserialize)]
struct ErasureEventWire {
    subject_type: Option<String>,
    subject_id: Option<String>,
    org_id: Option<String>,
    requested_by: Option<String>,
}

/// A validated organization-scoped erasure request. The only way to obtain
/// one is through [`parse_erasure_event`], which rejects every other
/// `subject_type` before a value of this type can exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationErasure {
    pub org_id: String,
    pub requested_by: String,
}

/// Failure decoding or validating a `velion.gdpr.erasure.requested`
/// message. Every variant is a poison condition — the caller should route
/// the message to a dead-letter path rather than retry it forever.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ErasureEventError {
    #[error("invalid JSON payload: {0}")]
    Decode(String),
    #[error("missing or empty org_id")]
    MissingOrgId,
    #[error("org_id exceeds the maximum bound")]
    OrgIdTooLong,
    #[error("missing or empty subject_id")]
    MissingSubjectId,
    #[error("subject_id does not match org_id for an organization-scoped erasure")]
    SubjectOrgMismatch,
}

/// Decode the fan-out payload and decide whether it is an
/// organization-scoped erasure this crate must act on.
///
/// Returns:
/// - `Ok(Some(erasure))` for a well-formed `subject_type: "organization"`
///   event — the caller should purge `erasure.org_id`.
/// - `Ok(None)` for any other well-formed `subject_type` (`"user"`,
///   `"user_anonymize"`, or anything else). See the module-level safety
///   note: those events are a deliberate no-op here.
/// - `Err(_)` for a malformed or self-contradictory payload.
///
/// # Errors
///
/// Returns [`ErasureEventError`] if the payload is not valid JSON, or if an
/// `organization`-typed event is missing `org_id`/`subject_id` or has a
/// `subject_id` that disagrees with `org_id` (the fixed contract is
/// `subject_id == org_id` for this subject type — a mismatch means the
/// event is malformed and must not be trusted for a destructive purge).
pub fn parse_erasure_event(
    payload: &[u8],
) -> Result<Option<OrganizationErasure>, ErasureEventError> {
    let wire: ErasureEventWire =
        serde_json::from_slice(payload).map_err(|e| ErasureEventError::Decode(e.to_string()))?;

    if wire.subject_type.as_deref().unwrap_or_default() != "organization" {
        return Ok(None);
    }

    let org_id = wire.org_id.unwrap_or_default().trim().to_owned();
    if org_id.is_empty() {
        return Err(ErasureEventError::MissingOrgId);
    }
    if org_id.len() > MAX_ID_LEN {
        return Err(ErasureEventError::OrgIdTooLong);
    }

    let subject_id = wire.subject_id.unwrap_or_default().trim().to_owned();
    if subject_id.is_empty() {
        return Err(ErasureEventError::MissingSubjectId);
    }
    if subject_id != org_id {
        return Err(ErasureEventError::SubjectOrgMismatch);
    }

    Ok(Some(OrganizationErasure {
        org_id,
        requested_by: wire.requested_by.unwrap_or_default(),
    }))
}

/// Per-table row counts from one purge run (logging/metrics/tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PurgeSummary {
    pub checkpoints: u64,
    pub subagent_edges: u64,
    pub todos: u64,
    pub plan_steps: u64,
    pub approvals: u64,
    pub plans: u64,
    pub memory_index: u64,
    pub messages: u64,
    pub task_events: u64,
    pub task_assignments: u64,
    pub task_dependencies: u64,
    pub cron_fires: u64,
    pub tasks: u64,
    pub cron_schedules: u64,
    pub events: u64,
    pub runs: u64,
    pub threads: u64,
    pub hook_configs: u64,
    pub agent_skills: u64,
    pub agent_memory: u64,
    pub finetune_jobs: u64,
    pub dream_runs: u64,
    pub session_audit_outbox: u64,
}

impl PurgeSummary {
    /// Total rows deleted across every table in one purge run.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.checkpoints
            + self.subagent_edges
            + self.todos
            + self.plan_steps
            + self.approvals
            + self.plans
            + self.memory_index
            + self.messages
            + self.task_events
            + self.task_assignments
            + self.task_dependencies
            + self.cron_fires
            + self.tasks
            + self.cron_schedules
            + self.events
            + self.runs
            + self.threads
            + self.hook_configs
            + self.agent_skills
            + self.agent_memory
            + self.finetune_jobs
            + self.dream_runs
            + self.session_audit_outbox
    }
}

/// Hard-purge every org-scoped row this crate owns for `org_id`, in one
/// transaction. Deletion order respects every FK with no `ON DELETE
/// CASCADE` (children before parents); `approval_delivery_outbox`,
/// `session_tool_audit_intents`, and `managed_run_terminalization_outbox`
/// cascade automatically from `approvals`/`runs` and need no statement here.
/// `routing_policy` is a global singleton (not org data) and is
/// intentionally excluded.
///
/// # Errors
///
/// Returns an error if the transaction fails to begin, any statement fails,
/// or the commit fails. On error nothing is purged — the transaction rolls
/// back, so a NAK'd redelivery retries the whole purge cleanly.
// Fields are populated one statement at a time (not as a single struct
// literal) because the comments between them document real FK-ordering
// dependencies between the awaited statements — collapsing this into one
// expression would obscure that ordering.
#[allow(clippy::field_reassign_with_default)]
pub async fn purge_organization_data(pool: &Pool, org_id: &str) -> anyhow::Result<PurgeSummary> {
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;
    let mut summary = PurgeSummary::default();

    // `runs` children with no cascade — must go before `runs`.
    summary.checkpoints = sqlx::query(
        "DELETE FROM checkpoints WHERE run_id IN (SELECT id FROM runs WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    summary.subagent_edges = sqlx::query(
        "DELETE FROM subagent_edges
         WHERE parent_run_id IN (SELECT id FROM runs WHERE org_id = $1)
            OR child_run_id  IN (SELECT id FROM runs WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // `plans` children with no cascade — must go before `plans`.
    summary.todos =
        sqlx::query("DELETE FROM todos WHERE plan_id IN (SELECT id FROM plans WHERE org_id = $1)")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

    summary.plan_steps = sqlx::query(
        "DELETE FROM plan_steps WHERE plan_id IN (SELECT id FROM plans WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // `approvals` references both `plans` and `runs` — delete before either.
    // `approval_delivery_outbox` cascades from `approvals` automatically.
    summary.approvals = sqlx::query("DELETE FROM approvals WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.plans = sqlx::query("DELETE FROM plans WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.memory_index = sqlx::query("DELETE FROM memory_index WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.messages = sqlx::query(
        "DELETE FROM messages WHERE thread_id IN (SELECT id FROM threads WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // Task family — children before `tasks` / `cron_schedules`.
    summary.task_events = sqlx::query(
        "DELETE FROM task_events WHERE task_id IN (SELECT id FROM tasks WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    summary.task_assignments = sqlx::query(
        "DELETE FROM task_assignments WHERE task_id IN (SELECT id FROM tasks WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    summary.task_dependencies = sqlx::query(
        "DELETE FROM task_dependencies
         WHERE task_id       IN (SELECT id FROM tasks WHERE org_id = $1)
            OR depends_on_id IN (SELECT id FROM tasks WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    summary.cron_fires = sqlx::query(
        "DELETE FROM cron_fires
         WHERE task_id     IN (SELECT id FROM tasks WHERE org_id = $1)
            OR schedule_id IN (SELECT id FROM cron_schedules WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    summary.tasks = sqlx::query("DELETE FROM tasks WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.cron_schedules = sqlx::query("DELETE FROM cron_schedules WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // Append-only event log: scoped by its own `org_id` column, plus (for
    // any pre-2 row where that column is still the empty-string default) by
    // the `run_id` of a run this org owns. Must run before `runs` below —
    // the `run_id` branch needs the parent rows to still exist.
    summary.events = sqlx::query(
        "DELETE FROM events
         WHERE org_id = $1
            OR run_id IN (SELECT id FROM runs WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // `runs` — cascades `session_tool_audit_intents` and
    // `managed_run_terminalization_outbox` (ON DELETE CASCADE).
    summary.runs = sqlx::query("DELETE FROM runs WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.threads = sqlx::query("DELETE FROM threads WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // Independent org-scoped tables — no FK ordering constraint on any of
    // these, so their position here is arbitrary.
    summary.hook_configs = sqlx::query("DELETE FROM hook_configs WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.agent_skills = sqlx::query("DELETE FROM agent_skills WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.agent_memory = sqlx::query("DELETE FROM agent_memory WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.finetune_jobs = sqlx::query("DELETE FROM finetune_jobs WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    summary.dream_runs = sqlx::query("DELETE FROM dream_runs WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // Not-yet-delivered audit-outbox rows carry `org_id` inside the JSONB
    // payload, not as a column (see `audit_publisher::build_tool_action_body`).
    summary.session_audit_outbox =
        sqlx::query("DELETE FROM session_audit_outbox WHERE payload ->> 'org_id' = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

    tx.commit().await?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn organization_payload(org_id: &str, subject_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "subject_type": "organization",
            "subject_id": subject_id,
            "org_id": org_id,
            "requested_by": "user_admin_1",
            "ts": "2026-07-20T00:00:00.000000000Z",
        }))
        .unwrap()
    }

    #[test]
    fn organization_erasure_is_parsed() {
        let payload = organization_payload("org_1", "org_1");
        let erasure = parse_erasure_event(&payload)
            .expect("decodes")
            .expect("organization events purge");
        assert_eq!(erasure.org_id, "org_1");
        assert_eq!(erasure.requested_by, "user_admin_1");
    }

    /// Safety-critical: a per-user erasure fan-out carries `org_id` for
    /// routing, not as a purge target. Treating it as one would destroy
    /// every other org member's data over a single user's request.
    #[test]
    fn user_subject_type_is_skipped_not_purged() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "subject_type": "user",
            "subject_id": "user_1",
            "org_id": "org_1",
            "requested_by": "user_1",
            "mode": "delete",
        }))
        .unwrap();
        assert_eq!(parse_erasure_event(&payload).unwrap(), None);
    }

    #[test]
    fn user_anonymize_subject_type_is_skipped_not_purged() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "subject_type": "user_anonymize",
            "subject_id": "user_1",
            "org_id": "org_1",
        }))
        .unwrap();
        assert_eq!(parse_erasure_event(&payload).unwrap(), None);
    }

    #[test]
    fn unknown_subject_type_is_skipped_not_purged() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "subject_type": "something_new",
            "subject_id": "x",
            "org_id": "org_1",
        }))
        .unwrap();
        assert_eq!(parse_erasure_event(&payload).unwrap(), None);
    }

    #[test]
    fn missing_subject_type_is_skipped_not_purged() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "org_id": "org_1",
        }))
        .unwrap();
        assert_eq!(parse_erasure_event(&payload).unwrap(), None);
    }

    #[test]
    fn missing_org_id_is_rejected() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "subject_type": "organization",
            "subject_id": "org_1",
        }))
        .unwrap();
        assert_eq!(
            parse_erasure_event(&payload).unwrap_err(),
            ErasureEventError::MissingOrgId
        );
    }

    #[test]
    fn empty_org_id_is_rejected() {
        let payload = organization_payload("   ", "   ");
        assert_eq!(
            parse_erasure_event(&payload).unwrap_err(),
            ErasureEventError::MissingOrgId
        );
    }

    #[test]
    fn oversized_org_id_is_rejected() {
        let huge = "o".repeat(MAX_ID_LEN + 1);
        let payload = organization_payload(&huge, &huge);
        assert_eq!(
            parse_erasure_event(&payload).unwrap_err(),
            ErasureEventError::OrgIdTooLong
        );
    }

    #[test]
    fn missing_subject_id_is_rejected() {
        let payload = serde_json::to_vec(&serde_json::json!({
            "subject_type": "organization",
            "org_id": "org_1",
        }))
        .unwrap();
        assert_eq!(
            parse_erasure_event(&payload).unwrap_err(),
            ErasureEventError::MissingSubjectId
        );
    }

    /// Defense in depth: the fixed contract is `subject_id == org_id` for
    /// organization-typed events. A mismatch means the event is malformed
    /// (or forged) and must never be trusted to select a purge target.
    #[test]
    fn subject_id_org_id_mismatch_is_rejected() {
        let payload = organization_payload("org_1", "org_2");
        assert_eq!(
            parse_erasure_event(&payload).unwrap_err(),
            ErasureEventError::SubjectOrgMismatch
        );
    }

    #[test]
    fn malformed_json_is_rejected() {
        assert!(matches!(
            parse_erasure_event(b"not-json"),
            Err(ErasureEventError::Decode(_))
        ));
    }

    #[test]
    fn purge_summary_total_sums_every_field() {
        let summary = PurgeSummary {
            checkpoints: 1,
            subagent_edges: 1,
            todos: 1,
            plan_steps: 1,
            approvals: 1,
            plans: 1,
            memory_index: 1,
            messages: 1,
            task_events: 1,
            task_assignments: 1,
            task_dependencies: 1,
            cron_fires: 1,
            tasks: 1,
            cron_schedules: 1,
            events: 1,
            runs: 1,
            threads: 1,
            hook_configs: 1,
            agent_skills: 1,
            agent_memory: 1,
            finetune_jobs: 1,
            dream_runs: 1,
            session_audit_outbox: 1,
        };
        assert_eq!(summary.total(), 23);
    }

    /// Static-analysis guard on the purge SQL itself: every `DELETE`
    /// statement must be parameterized (never string-interpolate `org_id`
    /// into a query — see the crate's org-isolation safety requirement),
    /// and the FK-dependency ordering this module's docs promise must hold
    /// in source order (children's `DELETE` textually precede their
    /// parent's).
    #[test]
    fn purge_statements_are_parameterized_and_correctly_ordered() {
        let source = include_str!("gdpr.rs");

        // No query ever builds its WHERE clause via string formatting of
        // the caller-supplied org_id — every statement binds `$1`.
        assert!(
            !source.contains("format!(\"DELETE"),
            "a DELETE must never be built with format!/string interpolation"
        );

        let pos = |needle: &str| {
            source
                .find(needle)
                .unwrap_or_else(|| panic!("expected to find {needle:?} in gdpr.rs"))
        };

        let checkpoints = pos("DELETE FROM checkpoints");
        let subagent_edges = pos("DELETE FROM subagent_edges");
        let todos = pos("DELETE FROM todos");
        let plan_steps = pos("DELETE FROM plan_steps");
        let approvals = pos("DELETE FROM approvals WHERE org_id = $1");
        let plans = pos("DELETE FROM plans WHERE org_id = $1");
        let messages = pos("DELETE FROM messages");
        let task_events = pos("DELETE FROM task_events");
        let task_assignments = pos("DELETE FROM task_assignments");
        let task_dependencies = pos("DELETE FROM task_dependencies");
        let cron_fires = pos("DELETE FROM cron_fires");
        let tasks = pos("DELETE FROM tasks WHERE org_id = $1");
        let cron_schedules = pos("DELETE FROM cron_schedules WHERE org_id = $1");
        let events = pos("DELETE FROM events");
        let runs = pos("DELETE FROM runs WHERE org_id = $1");
        let threads = pos("DELETE FROM threads WHERE org_id = $1");

        assert!(checkpoints < runs, "checkpoints must be purged before runs");
        assert!(
            subagent_edges < runs,
            "subagent_edges must be purged before runs"
        );
        assert!(todos < plans, "todos must be purged before plans");
        assert!(plan_steps < plans, "plan_steps must be purged before plans");
        assert!(
            approvals < plans,
            "approvals (references plans) must be purged before plans"
        );
        assert!(
            approvals < runs,
            "approvals (references runs) must be purged before runs"
        );
        assert!(
            plans < runs,
            "plans (references runs) must be purged before runs"
        );
        assert!(messages < threads, "messages must be purged before threads");
        assert!(
            task_events < tasks,
            "task_events must be purged before tasks"
        );
        assert!(
            task_assignments < tasks,
            "task_assignments must be purged before tasks"
        );
        assert!(
            task_dependencies < tasks,
            "task_dependencies must be purged before tasks"
        );
        assert!(
            cron_fires < tasks,
            "cron_fires (references tasks) must be purged before tasks"
        );
        assert!(
            cron_fires < cron_schedules,
            "cron_fires (references cron_schedules) must be purged before cron_schedules"
        );
        assert!(
            tasks < runs,
            "tasks (references runs) must be purged before runs"
        );
        assert!(
            events < runs,
            "events' run_id sweep must run while runs still exist"
        );
        assert!(runs < threads, "runs must be purged before threads");
    }
}
