//! Cross-plane GDPR organization-erasure purge.
//!
//! `org-core`'s `PublishGDPRErasureFanout` (both the explicit immediate
//! hard-delete path and the 30-day retention cron) and `user-core`'s
//! per-user erasure saga both publish onto the shared
//! `verevon.gdpr.erasure.requested` fan-out. This module decides which of
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
use std::{sync::OnceLock, time::Duration};
use tonic::Status;

use crate::store::Pool;

const MAX_ID_LEN: usize = 255;

/// Wire shape of one `verevon.gdpr.erasure.requested` message. Producers
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

/// Failure decoding or validating a `verevon.gdpr.erasure.requested`
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
    /// Semantic-tier outcome for the `agent_memory` rows this purge removed.
    ///
    /// Deliberately NOT folded into [`PurgeSummary::total`]: that total counts
    /// Postgres rows deleted, and adding a vector-store count to it would make
    /// one number mean two things. An erasure proof reads this field, and
    /// [`PurgeSummary::erasure_is_complete`] is the question it should ask.
    pub semantic_memory: crate::memory_erasure::SemanticErasure,
    pub checkpoints: u64,
    pub subagent_edges: u64,
    pub todos: u64,
    pub plan_steps: u64,
    pub approvals: u64,
    /// Standing approval grants erased with the organization (build plan WS13 S6).
    pub approval_scope_grants: u64,
    pub plans: u64,
    pub memory_index: u64,
    /// Artifact revision texts (EP-3 S2). They also cascade from `threads`; counted here so the
    /// erasure proof names the content table.
    pub artifact_revisions: u64,
    /// Thread goals, the owner's objectives and criteria (EP-7 S1). They also cascade from
    /// `threads`; counted here so the erasure proof names the content table.
    pub thread_goals: u64,
    /// Thread share snapshots, the owner's redacted copies of a thread (EP-12 S14). They also
    /// cascade from `threads`; counted here so the erasure proof names the content table.
    pub thread_share_snapshots: u64,
    pub messages: u64,
    pub task_events: u64,
    pub task_assignments: u64,
    pub task_dependencies: u64,
    pub cron_fires: u64,
    pub tasks: u64,
    pub cron_schedules: u64,
    /// Event trigger records (W4-F T1), the member's configuration of connector-event wakes. They name
    /// no thread or run; counted here so the erasure proof names the table.
    pub event_triggers: u64,
    /// Durable promote-on-use counters (W5-093-b): ids, a URL hash and a count. They hold no page
    /// address; counted here so the erasure proof names the table.
    pub promote_use_counts: u64,
    pub events: u64,
    pub run_trace_events: u64,
    /// Request snapshots and their system/tool blobs (EP-14 S6).
    pub run_request_snapshots: u64,
    pub run_request_snapshot_blobs: u64,
    pub runs: u64,
    pub threads: u64,
    pub hook_configs: u64,
    pub agent_skills: u64,
    pub skill_revisions: u64,
    /// Immutable skill versions and their bodies (EP-11 S2). A skill's delete also removes them;
    /// they are purged before the skills so the erasure proof counts them.
    pub agent_skill_versions: u64,
    pub agent_memory: u64,
    pub user_memory_controls: u64,
    /// Standing user profiles (EP-8 S4).
    pub user_profiles: u64,
    pub forgotten_memory_ids: u64,
    pub memory_control_events: u64,
    pub org_learning_policy_mirror: u64,
    /// The organisation's routing overlay (W4-BYO-3): configuration the organisation wrote, erased with it.
    pub org_routing_overlays: u64,
    pub finetune_jobs: u64,
    pub dream_runs: u64,
    pub session_audit_outbox: u64,
}

impl PurgeSummary {
    /// Total rows deleted across every table in one purge run.
    #[must_use]
    /// Whether this purge can honestly claim complete erasure.
    ///
    /// A DSAR response should not assert erasure on `total()` alone: the
    /// Postgres rows can all be gone while their semantic twins are
    /// unaccounted for. False means the proof is partial and the
    /// `semantic_memory.degradation_reason` says why.
    pub fn erasure_is_complete(&self) -> bool {
        self.semantic_memory.is_complete()
    }

    pub fn total(&self) -> u64 {
        self.checkpoints
            + self.subagent_edges
            + self.todos
            + self.plan_steps
            + self.approvals
            + self.approval_scope_grants
            + self.plans
            + self.memory_index
            + self.artifact_revisions
            + self.thread_goals
            + self.thread_share_snapshots
            + self.messages
            + self.task_events
            + self.task_assignments
            + self.task_dependencies
            + self.cron_fires
            + self.tasks
            + self.cron_schedules
            + self.event_triggers
            + self.promote_use_counts
            + self.events
            + self.run_trace_events
            + self.run_request_snapshots
            + self.run_request_snapshot_blobs
            + self.runs
            + self.threads
            + self.hook_configs
            + self.agent_skills
            + self.skill_revisions
            + self.agent_skill_versions
            + self.agent_memory
            + self.user_memory_controls
            + self.user_profiles
            + self.forgotten_memory_ids
            + self.memory_control_events
            + self.org_learning_policy_mirror
            + self.org_routing_overlays
            + self.finetune_jobs
            + self.dream_runs
            + self.session_audit_outbox
    }
}

// ---------------------------------------------------------------------------------------------
// Legal holds against erasure (EP-9 C4, decision D-C1).
//
// org-core is the hold record (EP-9 C1). Before a user deletes a thread, bulk-deletes their
// history, or an organisation erasure purges the org, session-core asks org-core's check route
// whether the organisation or any of the threads is held. A held scope is refused with
// `erasure_blocked_legal_hold`; the hold is released in Control, never overridden here. A check
// that cannot be answered refuses with the retryable `erasure_hold_check_unavailable`: a hold
// that cannot be ruled out is treated as present, because a wrong refusal is retried and a
// wrong erasure is not recoverable.
// ---------------------------------------------------------------------------------------------

/// The switch. Only the literal `true` turns the check on; off, no hold is asked and every
/// erasure runs exactly as before.
pub(crate) const ERASURE_HOLD_CHECK_ENV: &str = "ERASURE_HOLD_CHECK";

/// Typed refusal words on the `x-mp-error-code` trailer.
pub(crate) const ERASURE_BLOCKED_LEGAL_HOLD: &str = "erasure_blocked_legal_hold";
pub(crate) const ERASURE_HOLD_CHECK_UNAVAILABLE: &str = "erasure_hold_check_unavailable";

/// What a hold keeps when it blocks a thread erase: every content family `delete_thread_rows`
/// and the org purge remove, named so the refusal says what stays. Thread goals, artifact
/// revisions and share snapshots are thread-bound tables of their own (C-B9, EP-12 S14) and must
/// stay in this list.
pub(crate) const HELD_THREAD_CONTENT: &[&str] = &[
    "messages",
    "runs",
    "plans",
    "approvals",
    "tasks",
    "agent_memory",
    "thread_goals",
    "artifact_revisions",
    "thread_share_snapshots",
];

/// The most thread ids one check call may name (org-core `retention.MaxCheckThreadIDs`).
const CHECK_PAGE: usize = 500;
/// A full page of held ids is well under this; anything larger is not a check answer.
const MAX_CHECK_BODY_BYTES: usize = 128 * 1024;
const CHECK_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Above org-core's own 3 s bound on the check, so its typed answer arrives first.
const CHECK_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Why an erasure was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ErasureHoldError {
    /// An active org hold covers every thread of the organisation.
    #[error("erasure_blocked_legal_hold: an active legal hold covers the organisation")]
    OrgHeld,
    /// Active thread holds cover this many of the threads asked about.
    #[error("erasure_blocked_legal_hold: an active legal hold covers {0} of the threads")]
    ThreadsHeld(usize),
    /// No answer could be had, so no hold can be ruled out. The text is for logs only.
    #[error("erasure_hold_check_unavailable: {0}")]
    Unavailable(String),
}

impl ErasureHoldError {
    /// The gRPC refusal. Held is `FailedPrecondition` (retrying changes nothing until the hold is
    /// released); unavailable is `Unavailable` (a retry may succeed). The message never carries
    /// the internal cause, which can hold org-core's address.
    pub(crate) fn into_status(self) -> Status {
        let held = |scope: &str| {
            Status::failed_precondition(format!(
                "a legal hold covers {scope}, so nothing was deleted; its {} are kept until the hold is released",
                HELD_THREAD_CONTENT.join(", ")
            ))
        };
        match self {
            Self::OrgHeld => crate::space::mentions::coded_status(
                held("this organisation"),
                ERASURE_BLOCKED_LEGAL_HOLD,
            ),
            Self::ThreadsHeld(count) => crate::space::mentions::coded_status(
                held(&format!("{count} of these conversations")),
                ERASURE_BLOCKED_LEGAL_HOLD,
            ),
            Self::Unavailable(cause) => {
                tracing::warn!(%cause, "legal hold check unavailable; erasure refused");
                crate::space::mentions::coded_status(
                    Status::unavailable(
                        "legal holds could not be checked, so nothing was deleted; try again shortly",
                    ),
                    ERASURE_HOLD_CHECK_UNAVAILABLE,
                )
            }
        }
    }
}

/// org-core's check answer, read strictly: both fields are required. The window and revision
/// it also carries are the sweeper's, not this gate's.
#[derive(Deserialize)]
struct CheckWire {
    org_held: bool,
    held_thread_ids: Vec<String>,
}

/// Asks org-core's check route on behalf of session-core. Holds a service credential, so it has
/// no `Debug`.
pub(crate) struct HoldChecker {
    base_url: String,
    token: String,
    client: reqwest::Client,
}

impl HoldChecker {
    /// `None` when org-core's address or the service token is unset. It reuses the org policy
    /// poller's credentials: one session-core principal at org-core, holding the check scope.
    fn from_lookup(get: &impl Fn(&str) -> Option<String>) -> Option<Self> {
        let base_url = get(crate::org_policy::ORG_CORE_URL_ENV)?.trim().to_owned();
        let token = get(crate::org_policy::SERVICE_TOKEN_ENV)?.trim().to_owned();
        if base_url.is_empty() || token.is_empty() {
            return None;
        }
        let client = reqwest::Client::builder()
            .connect_timeout(CHECK_CONNECT_TIMEOUT)
            .timeout(CHECK_REQUEST_TIMEOUT)
            .build()
            .ok()?;
        Some(Self { base_url, token, client })
    }

    fn check_url(&self, org_id: &str) -> Result<reqwest::Url, ErasureHoldError> {
        let mut url = reqwest::Url::parse(&self.base_url)
            .map_err(|_| ErasureHoldError::Unavailable("ORG_CORE_URL is not a URL".to_owned()))?;
        url.path_segments_mut()
            .map_err(|()| ErasureHoldError::Unavailable("ORG_CORE_URL cannot carry a path".to_owned()))?
            .pop_if_empty()
            .extend(["internal", "orgs", org_id, "retention", "check"]);
        Ok(url)
    }

    /// One page. `None` is org-core's definitive 404: the organisation is not live, and its
    /// holds went with it (`org_legal_holds` cascades from the organisation).
    async fn ask_page(&self, org_id: &str, page: &[String]) -> Result<Option<CheckWire>, ErasureHoldError> {
        let unavailable = ErasureHoldError::Unavailable;
        let response = self
            .client
            .post(self.check_url(org_id)?)
            .header("x-service-id", "session-core")
            .header("x-service-token", &self.token)
            .json(&serde_json::json!({ "thread_ids": page }))
            .send()
            .await
            .map_err(|error| unavailable(format!("check request failed: {error}")))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(unavailable(format!("check answered {status}")));
        }
        let body = response
            .bytes()
            .await
            .map_err(|error| unavailable(format!("check body unreadable: {error}")))?;
        if body.len() > MAX_CHECK_BODY_BYTES {
            return Err(unavailable("check body too large".to_owned()));
        }
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|error| unavailable(format!("check body is not a check answer: {error}")))
    }

    /// Whether the organisation or any of `thread_ids` is held, paging the ids. An org hold
    /// answers at once; thread holds are counted over every page.
    async fn ensure_not_held(&self, org_id: &str, thread_ids: &[String]) -> Result<(), ErasureHoldError> {
        let empty: [String; 0] = [];
        let pages: Vec<&[String]> = if thread_ids.is_empty() {
            vec![&empty[..]]
        } else {
            thread_ids.chunks(CHECK_PAGE).collect()
        };
        let mut held = 0usize;
        for page in pages {
            let Some(answer) = self.ask_page(org_id, page).await? else {
                return Ok(());
            };
            if answer.org_held {
                return Err(ErasureHoldError::OrgHeld);
            }
            held += answer.held_thread_ids.len();
        }
        if held > 0 {
            return Err(ErasureHoldError::ThreadsHeld(held));
        }
        Ok(())
    }
}

/// The gate every erasure path asks.
pub(crate) enum ErasureHoldGate {
    /// The switch is off: nothing is asked.
    Off,
    /// The switch is on but org-core cannot be addressed: every erasure is refused as
    /// unavailable, since no hold can be ruled out.
    Unconfigured,
    On(HoldChecker),
}

impl ErasureHoldGate {
    pub(crate) fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        if get(ERASURE_HOLD_CHECK_ENV).as_deref() != Some("true") {
            return Self::Off;
        }
        HoldChecker::from_lookup(&get).map_or(Self::Unconfigured, Self::On)
    }

    /// Refuses when the organisation or any of `thread_ids` (empty: the organisation alone) is
    /// under an active legal hold, or when that cannot be ruled out.
    ///
    /// # Errors
    ///
    /// [`ErasureHoldError`] names the refusal.
    pub(crate) async fn ensure_not_held(&self, org_id: &str, thread_ids: &[String]) -> Result<(), ErasureHoldError> {
        match self {
            Self::Off => Ok(()),
            Self::Unconfigured => Err(ErasureHoldError::Unavailable(format!(
                "{ERASURE_HOLD_CHECK_ENV} is true but {} or {} is unset",
                crate::org_policy::ORG_CORE_URL_ENV,
                crate::org_policy::SERVICE_TOKEN_ENV
            ))),
            Self::On(checker) => checker.ensure_not_held(org_id, thread_ids).await,
        }
    }
}

/// The process gate, read once.
pub(crate) fn erasure_hold_gate() -> &'static ErasureHoldGate {
    static GATE: OnceLock<ErasureHoldGate> = OnceLock::new();
    GATE.get_or_init(|| {
        let gate = ErasureHoldGate::from_lookup(|name| std::env::var(name).ok());
        if matches!(gate, ErasureHoldGate::Unconfigured) {
            tracing::error!(
                "{ERASURE_HOLD_CHECK_ENV} is true but org-core credentials are unset: every thread delete and org erasure is refused"
            );
        }
        gate
    })
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
/// back, so a NAK'd redelivery retries the whole purge cleanly. With
/// `ERASURE_HOLD_CHECK` on it also returns an [`ErasureHoldError`] before any
/// statement when the organisation is held or its holds cannot be checked.
// Fields are populated one statement at a time (not as a single struct
// literal) because the comments between them document real FK-ordering
// dependencies between the awaited statements — collapsing this into one
// expression would obscure that ordering.
#[allow(clippy::field_reassign_with_default)]
pub async fn purge_organization_data(
    pool: &Pool,
    org_id: &str,
    letta: Option<&crate::letta_adapter::LettaMemoryAdapter>,
) -> anyhow::Result<PurgeSummary> {
    // EP-9 C4: a held organisation is not purged. Asked before the transaction opens, so a
    // refusal touches nothing and the consumer's redelivery retries it after the release.
    erasure_hold_gate().ensure_not_held(org_id, &[]).await?;

    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;
    let mut summary = PurgeSummary::default();

    // Space event log: switch the deferred event triggers off for this transaction FIRST.
    // They fire at commit, and without this the thread/run deletes below would write new
    // `thread.deleted` rows for an org that was just erased.
    crate::space::events::suppress(&mut tx).await?;

    // The loop's sealed folds are erased with the org's runs, before them.
    sqlx::query(
        "DELETE FROM run_loop_checkpoints WHERE run_id IN (SELECT id FROM runs WHERE org_id = $1)",
    )
    .bind(org_id)
    .execute(&mut *tx)
    .await?;

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

    // Standing grants reference their approval, run and thread, so they go before any of them.
    summary.approval_scope_grants =
        sqlx::query("DELETE FROM approval_scope_grants WHERE org_id = $1")
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

    // Artifact revisions (EP-3 S2) go before their messages and threads. The thread FK cascades
    // too; the explicit statement keeps the count and survives a dropped cascade.
    summary.artifact_revisions = sqlx::query("DELETE FROM artifact_revisions WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // Thread goals (EP-7 S1) go before their threads, for the same reasons.
    summary.thread_goals = sqlx::query("DELETE FROM thread_goals WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // Thread share snapshots (EP-12 S14) go before their threads, for the same reasons. With the grants
    // switch on, their ids are read first, so the Control grants left behind can be named after the commit.
    let snapshot_ids: Vec<String> = if crate::thread_shares::snapshot_grants_enabled() {
        sqlx::query_scalar("SELECT id FROM thread_share_snapshots WHERE org_id = $1")
            .bind(org_id)
            .fetch_all(&mut *tx)
            .await?
    } else {
        Vec::new()
    };
    summary.thread_share_snapshots =
        sqlx::query("DELETE FROM thread_share_snapshots WHERE org_id = $1")
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

    // Event trigger records (W4-F T1): the org's rows, deleted before runs and threads so a later
    // reference from a run or thread cannot strand one. Nothing else in the purge reads them.
    summary.event_triggers = sqlx::query("DELETE FROM event_triggers WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // Durable promote-on-use counters (W5-093-b): the org's rows. Nothing else in the purge reads them.
    summary.promote_use_counts = sqlx::query("DELETE FROM promote_use_counts WHERE org_id = $1")
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

    // Space event log (ids and kinds only): the org's rows and per-Space heads go with its
    // events. Not counted in the summary (the erasure proof counts content tables).
    sqlx::query("DELETE FROM space_event_log WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM space_event_heads WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?;

    // The run trace (WS10.S6): scoped by its own `org_id`, and removed explicitly before `runs`
    // so the count is the purge's own rather than the cascade's.
    summary.run_trace_events = sqlx::query("DELETE FROM run_trace_events WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // Request snapshots (EP-14 S6): they would cascade from `runs`, but are removed explicitly by their own
    // `org_id` so the purge counts them, as the run trace is.
    summary.run_request_snapshot_blobs =
        sqlx::query("DELETE FROM run_request_snapshot_blobs WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    summary.run_request_snapshots = sqlx::query("DELETE FROM run_request_snapshots WHERE org_id = $1")
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

    // Before the skills, so the count is the purge's own rather than the skill delete trigger's.
    // A version row refuses any delete outside erasure (migration 0108); the setting is put back
    // at once so nothing else in this transaction deletes a version through it.
    sqlx::query("SELECT set_config('mp.version_erasure', 'on', true)")
        .execute(&mut *tx)
        .await?;
    summary.agent_skill_versions =
        sqlx::query("DELETE FROM agent_skill_versions WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    sqlx::query("SELECT set_config('mp.version_erasure', '', true)")
        .execute(&mut *tx)
        .await?;

    summary.agent_skills = sqlx::query("DELETE FROM agent_skills WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // After the skills, not before: deleting a skill row writes a `deleted`
    // revision holding its text (agent_skills_revision_trg), and that revision
    // must be erased with the rest.
    summary.skill_revisions = sqlx::query("DELETE FROM skill_revisions WHERE org_id = $1")
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    // RETURNING, not just a row count: each row has a semantic twin on
    // letta-bridge keyed by the same id, and once this statement commits there
    // is nothing left in Postgres to find that twin by. `owner` is the user_id
    // the semantic copy was tagged with (memory_grpc::index_memory).
    let mut erased_memories: Vec<crate::memory_erasure::ErasedMemory> =
        sqlx::query_as::<_, (String, String)>(
            "DELETE FROM agent_memory WHERE org_id = $1 RETURNING id, owner",
        )
        .bind(org_id)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|(memory_id, owner)| crate::memory_erasure::ErasedMemory { memory_id, owner })
        .collect();
    summary.agent_memory = erased_memories.len() as u64;

    let forgotten: Vec<(String, String)> = sqlx::query_as("SELECT memory_id, user_id FROM forgotten_memory_ids WHERE org_id = $1")
        .bind(org_id).fetch_all(&mut *tx).await?;
    for (memory_id, owner) in forgotten {
        if !erased_memories.iter().any(|entry| entry.memory_id == memory_id && entry.owner == owner) {
            erased_memories.push(crate::memory_erasure::ErasedMemory { memory_id, owner });
        }
    }

    summary.forgotten_memory_ids = sqlx::query("DELETE FROM forgotten_memory_ids WHERE org_id = $1")
        .bind(org_id).execute(&mut *tx).await?.rows_affected();
    summary.user_memory_controls = sqlx::query("DELETE FROM user_memory_controls WHERE org_id = $1")
        .bind(org_id).execute(&mut *tx).await?.rows_affected();
    // The profile is derived from the agent_memory rows erased above, or is the person's own text.
    summary.user_profiles = sqlx::query("DELETE FROM user_profiles WHERE org_id = $1")
        .bind(org_id).execute(&mut *tx).await?.rows_affected();
    summary.memory_control_events = sqlx::query("DELETE FROM memory_control_events WHERE org_id = $1")
        .bind(org_id).execute(&mut *tx).await?.rows_affected();
    summary.org_learning_policy_mirror = sqlx::query("DELETE FROM org_learning_policy_mirror WHERE org_id = $1")
        .bind(org_id).execute(&mut *tx).await?.rows_affected();
    // The organisation's routing overlay (W4-BYO-3), whatever SESSION_ORG_ROUTING_OVERLAY says.
    summary.org_routing_overlays = sqlx::query("DELETE FROM org_routing_overlays WHERE org_id = $1")
        .bind(org_id).execute(&mut *tx).await?.rows_affected();

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

    // The purge holds no owner credential, and Control grants a view only to a verified end user, so
    // the grants of these snapshots stay in Control. Each is named by id and code, never content.
    crate::thread_shares::log_snapshot_grants_not_removed(
        &snapshot_ids,
        crate::thread_shares::GRANTS_NO_CREDENTIAL_CODE,
    );

    // After the commit, deliberately. The durable rows are the source of truth
    // for existence; a degraded semantic tier must not roll back an erasure
    // that Postgres has already honoured. What it must not do either is let the
    // summary imply the vector-store copies went with them.
    summary.semantic_memory =
        crate::memory_erasure::erase_semantic_copies(letta, org_id, &erased_memories).await;
    if !summary.erasure_is_complete() {
        tracing::warn!(
            org_id,
            considered = summary.semantic_memory.considered(),
            confirmed = summary.semantic_memory.confirmed,
            unconfirmed = summary.semantic_memory.unconfirmed,
            degradation = ?summary.semantic_memory.degradation_reason,
            "GDPR erasure incomplete: agent_memory rows purged but semantic twins unconfirmed"
        );
        metrics::counter!(
            "mp_session_semantic_erasure_unconfirmed_total",
            "operation" => "gdpr_purge"
        )
        .increment(summary.semantic_memory.unconfirmed);
    }
    Ok(summary)
}

/// A validated per-user erasure (`user-core`'s saga, `subject_type == "user"`), read only for what
/// session-core erases per user: that person's part of Space memory (EP-8 S7), and their standing
/// profile and own personal memory (EP-8 S4). Everything else the module note says about per-user
/// events still holds: the org is not purged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserErasure {
    pub org_id: String,
    pub user_id: String,
    /// Which `subject_type` the event carried.
    pub kind: UserErasureKind,
}

/// Which per-user `subject_type` an erasure event carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserErasureKind {
    /// `subject_type: "user"`: the person's Space memory sources, standing profile and personal
    /// memory are erased.
    Erase,
    /// `subject_type: "user_anonymize"`: the records are kept and the identity removed. Session-core
    /// keeps the Space memory rows, but the standing profile and the person's own personal memory
    /// are content about the person, so they are removed as for [`UserErasureKind::Erase`].
    Anonymize,
}

impl UserErasureKind {
    /// The stable label written to the content-free audit row.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Erase => "user",
            Self::Anonymize => "user_anonymize",
        }
    }
}

/// Decode the fan-out payload as a per-user erasure. `Ok(None)` for every other subject type.
/// `user_anonymize` is a user erasure here ([`UserErasureKind::Anonymize`]): the caller erases the
/// standing profile and personal memory for it, and leaves the Space rows alone.
///
/// # Errors
///
/// Returns [`ErasureEventError`] for a payload that is not JSON, or a `user` event without a
/// bounded org or subject.
pub fn parse_user_erasure_event(payload: &[u8]) -> Result<Option<UserErasure>, ErasureEventError> {
    let wire: ErasureEventWire =
        serde_json::from_slice(payload).map_err(|e| ErasureEventError::Decode(e.to_string()))?;
    let kind = match wire.subject_type.as_deref().unwrap_or_default() {
        "user" => UserErasureKind::Erase,
        "user_anonymize" => UserErasureKind::Anonymize,
        _ => return Ok(None),
    };
    let org_id = wire.org_id.unwrap_or_default().trim().to_owned();
    if org_id.is_empty() {
        return Err(ErasureEventError::MissingOrgId);
    }
    if org_id.len() > MAX_ID_LEN {
        return Err(ErasureEventError::OrgIdTooLong);
    }
    let user_id = wire.subject_id.unwrap_or_default().trim().to_owned();
    if user_id.is_empty() || user_id.len() > MAX_ID_LEN {
        return Err(ErasureEventError::MissingSubjectId);
    }
    Ok(Some(UserErasure { org_id, user_id, kind }))
}

/// Asks the legal-hold gate about the organization, before a per-user erasure touches any store. A
/// hold that covers the organization, or a check that cannot be answered, refuses with
/// [`ErasureHoldError`], so a held organization erases nothing: not the Space rows, not the profile,
/// not the personal memory. The consumer runs this first and naks with a delay on a refusal.
///
/// # Errors
///
/// Returns [`ErasureHoldError`] when a hold covers the organization or cannot be ruled out.
pub async fn ensure_user_erasure_not_held(org_id: &str) -> Result<(), ErasureHoldError> {
    erasure_hold_gate().ensure_not_held(org_id, &[]).await
}

/// What erasing one person does to one Space row, pure. `links` are the row's `source_links`;
/// `attributed` names the person each attributable link came from: a `message:<id>` link its
/// author, the `tool:save_memory` link of an explicit save the owner of the thread it was saved in.
/// A link nobody is attributed to (a `thread:` link, a message already gone) supports nobody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceSourceErasure {
    /// The person is not a source of this row.
    Untouched,
    /// The person was the row's only source: the row goes (EP-8 S7, DPO rule assumed).
    Delete,
    /// Others are sources too: the row stays, without the person's links.
    Keep(Vec<String>),
}

pub fn space_source_erasure(
    links: &[String],
    attributed: &std::collections::HashMap<String, String>,
    user_id: &str,
) -> SpaceSourceErasure {
    let is_users = |link: &String| attributed.get(link).is_some_and(|subject| subject == user_id);
    if !links.iter().any(is_users) {
        return SpaceSourceErasure::Untouched;
    }
    let others = links
        .iter()
        .any(|link| attributed.get(link).is_some_and(|subject| subject != user_id));
    if !others {
        return SpaceSourceErasure::Delete;
    }
    SpaceSourceErasure::Keep(links.iter().filter(|link| !is_users(link)).cloned().collect())
}

/// Counts of one person's Space-memory erasure.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SpaceSourceSummary {
    pub deleted: u64,
    pub narrowed: u64,
}

/// Drops one person's part of every Space memory row of their org, in one transaction (EP-8 S7):
/// a row only they were the source of is deleted, a row with other members' sources keeps those
/// and loses theirs. Space rows have no Letta copy (EP-8 S6), so this is the whole erasure.
/// Idempotent: a redelivered event finds nothing of the person left.
///
/// # Errors
///
/// Returns an error if any statement or the commit fails; nothing is changed then.
pub async fn erase_user_space_memory_sources(
    pool: &Pool,
    org_id: &str,
    user_id: &str,
) -> anyhow::Result<SpaceSourceSummary> {
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;
    // The rows the person may be a source of, locked: a message they wrote, or an explicit save in
    // a thread they own.
    let rows: Vec<(String, Vec<String>, Option<String>)> = sqlx::query_as(
        "SELECT am.id, am.source_links, am.source_label FROM agent_memory am \
         WHERE am.org_id = $1 AND am.scope = 'space' AND ( \
           EXISTS (SELECT 1 FROM unnest(am.source_links) AS sl(link) \
                   JOIN messages m ON m.id = substr(sl.link, 9) \
                   JOIN threads t ON t.id = m.thread_id \
                   WHERE sl.link LIKE 'message:%' AND t.org_id = $1 \
                     AND COALESCE(m.author_subject_id, t.user_id) = $2) \
           OR (am.source_label = 'member_save' AND EXISTS ( \
                   SELECT 1 FROM unnest(am.source_links) AS sl(link) \
                   JOIN threads t ON t.id = substr(sl.link, 8) \
                   WHERE sl.link LIKE 'thread:%' AND t.org_id = $1 AND t.user_id = $2))) \
         ORDER BY am.id FOR UPDATE OF am",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;

    let mut summary = SpaceSourceSummary::default();
    for (memory_id, links, source_label) in rows {
        let mut attributed: std::collections::HashMap<String, String> = sqlx::query_as::<_, (String, String)>(
            "SELECT 'message:' || m.id, COALESCE(m.author_subject_id, t.user_id) \
             FROM messages m JOIN threads t ON t.id = m.thread_id \
             WHERE t.org_id = $1 AND ('message:' || m.id) = ANY($2)",
        )
        .bind(org_id)
        .bind(&links)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
        // An explicit save is the saving thread owner's statement.
        if source_label.as_deref() == Some("member_save") {
            let saver: Option<String> = sqlx::query_scalar(
                "SELECT t.user_id FROM threads t \
                 WHERE t.org_id = $1 AND ('thread:' || t.id) = ANY($2) ORDER BY t.id LIMIT 1",
            )
            .bind(org_id)
            .bind(&links)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(saver) = saver {
                attributed.insert("tool:save_memory".to_owned(), saver);
            }
        }
        match space_source_erasure(&links, &attributed, user_id) {
            SpaceSourceErasure::Untouched => {}
            SpaceSourceErasure::Delete => {
                summary.deleted += sqlx::query("DELETE FROM agent_memory WHERE id = $1 AND org_id = $2")
                    .bind(&memory_id)
                    .bind(org_id)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
            }
            SpaceSourceErasure::Keep(remaining) => {
                summary.narrowed += sqlx::query(
                    "UPDATE agent_memory SET source_links = $3, updated_at = now() WHERE id = $1 AND org_id = $2",
                )
                .bind(&memory_id)
                .bind(org_id)
                .bind(&remaining)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            }
        }
    }
    tx.commit().await?;
    if summary.deleted > 0 {
        metrics::counter!("mp_session_space_memory_erased_total", "op" => "gdpr_user")
            .increment(summary.deleted);
    }
    Ok(summary)
}

/// Counts of one person's standing-profile and personal-memory erasure (EP-8 S4).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UserErasureSummary {
    /// Standing profile rows deleted: one, or none when the person had no profile.
    pub user_profiles: u64,
    /// The person's own durable memory rows (`scope = 'user'`) deleted.
    pub personal_memory: u64,
    /// Semantic-tier outcome for the copies of those rows, and of any copy the listing found that no
    /// durable row still holds.
    pub semantic_memory: crate::memory_erasure::SemanticErasure,
    /// Why the semantic listing could not be read in full. The copies it would have named are then
    /// unknown, so the erasure cannot claim completeness.
    pub listing_degradation: Option<&'static str>,
}

impl UserErasureSummary {
    /// Whether this erasure can honestly claim completeness: every semantic copy accounted for, and
    /// the listing that finds orphaned copies read in full.
    pub fn erasure_is_complete(&self) -> bool {
        self.semantic_memory.is_complete() && self.listing_degradation.is_none()
    }
}

/// Erases one person's standing profile and their own personal memory, in one transaction, then the
/// semantic copies of that memory (EP-8 S4). It is the per-user counterpart of the organization purge
/// for a `subject_type: "user"` or `"user_anonymize"` event, called after
/// [`ensure_user_erasure_not_held`] has cleared the organization (and, for `user`, after
/// [`erase_user_space_memory_sources`]). It never touches another member's rows or the
/// organization's. It writes one content-free `memory_control_events` row (`event = 'erased'`).
///
/// Scope: the person's `scope = 'user'` rows that have no session and are owned by them. Org-owned
/// rows and the thread-bound rows of their threads are not erased here.
///
/// Legal holds are asked before anything is touched, as the organization purge asks them. A hold that
/// covers the organization, or a check that cannot be answered, refuses with [`ErasureHoldError`] and
/// the consumer naks with a delay.
///
/// The person's control row is locked first, as `reset_user_memory` locks it, so an extraction in
/// flight finishes before this erase or is refused after it. `extract_after` moves to now, so the
/// messages said before the erase are not mined back into memory by a later Dreaming pass. That is
/// why the control row is kept. Each erased id is recorded as forgotten, so a late semantic mirror of
/// one is removed rather than recalled.
///
/// The semantic tier is propagated after the commit, which a degraded tier never rolls back. With a
/// Letta adapter, each erased copy is deleted by its owner, and the per-user listing finds the copies
/// that no durable row of the organization still holds (what a degraded earlier delete leaves behind).
/// A copy that a remaining durable row names stays. Without an adapter nothing is called, and the
/// erased copies are reported unconfirmed, as the organization purge reports them, never as erased.
///
/// Idempotent: a redelivered event finds no durable row of the person left, so only the semantic tier
/// can still have work to do.
///
/// # Errors
///
/// Returns [`ErasureHoldError`] when a hold covers the organization or cannot be ruled out. Returns an
/// error when a statement or the commit fails, and nothing of that transaction is kept. An error from
/// the orphan lookup after the commit makes the redelivery finish the semantic tier.
pub async fn erase_user_personal_data(
    pool: &Pool,
    org_id: &str,
    user_id: &str,
    letta: Option<&crate::letta_adapter::LettaMemoryAdapter>,
) -> anyhow::Result<UserErasureSummary> {
    erase_user_personal_data_for(pool, org_id, user_id, UserErasureKind::Erase, letta).await
}

/// [`erase_user_personal_data`] for either per-user event kind. `user_anonymize` removes the same two
/// stores (the standing profile and the person's own personal memory) as `user`. Each call writes one
/// content-free `memory_control_events` row in the erasing transaction: the kind, the number of
/// profile rows and personal memories deleted, never any content or id list.
///
/// # Errors
///
/// As [`erase_user_personal_data`].
pub async fn erase_user_personal_data_for(
    pool: &Pool,
    org_id: &str,
    user_id: &str,
    kind: UserErasureKind,
    letta: Option<&crate::letta_adapter::LettaMemoryAdapter>,
) -> anyhow::Result<UserErasureSummary> {
    // EP-9 C4: asked before the transaction opens, so a refusal touches nothing and the redelivery
    // retries it after the release.
    erasure_hold_gate().ensure_not_held(org_id, &[]).await?;

    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;
    crate::memory_control::lock_user(&mut tx, org_id, user_id).await?;
    let mut erased: Vec<String> = sqlx::query_scalar(
        "DELETE FROM agent_memory \
         WHERE org_id = $1 AND session_id IS NULL AND scope = 'user' AND owner = $2 \
         RETURNING id",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    erased.sort();
    sqlx::query(
        "INSERT INTO forgotten_memory_ids (org_id, user_id, memory_id) \
         SELECT $1, $2, id FROM unnest($3::text[]) AS erased(id) ON CONFLICT DO NOTHING",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(&erased)
    .execute(&mut *tx)
    .await?;
    // clock_timestamp is measured after the lock, not when a waiting tx began.
    sqlx::query(
        "UPDATE user_memory_controls SET extract_after = clock_timestamp() \
         WHERE org_id = $1 AND user_id = $2",
    )
    .bind(org_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    let user_profiles = crate::user_profile::delete_in_tx(&mut tx, org_id, user_id).await?;
    // The audit record of this erasure: counts only, never the erased text or an id list. Written in
    // the erasing transaction, so a rolled-back erasure leaves no row claiming it happened.
    sqlx::query(
        "INSERT INTO memory_control_events (id, org_id, user_id, event, actor, detail) \
         VALUES ($1, $2, $3, 'erased', $4, jsonb_build_object( \
           'kind', $5::text, 'user_profiles', $6::bigint, 'personal_memory', $7::bigint))",
    )
    .bind(mp_ids::new_ulid())
    .bind(org_id)
    .bind(user_id)
    .bind("gdpr-erasure")
    .bind(kind.as_str())
    .bind(i64::try_from(user_profiles).unwrap_or(i64::MAX))
    .bind(i64::try_from(erased.len()).unwrap_or(i64::MAX))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // After the commit, deliberately: the durable rows are the source of truth for existence, and a
    // degraded semantic tier must not roll back an erasure Postgres has already honoured.
    let personal_memory = erased.len() as u64;
    let mut targets: Vec<crate::memory_erasure::ErasedMemory> = erased
        .iter()
        .map(|memory_id| crate::memory_erasure::ErasedMemory {
            memory_id: memory_id.clone(),
            owner: user_id.to_owned(),
        })
        .collect();
    let mut listing_degradation = None;
    if let Some(letta) = letta {
        let listed = letta
            .list_detailed(
                org_id,
                user_id,
                crate::memory_erasure::MAX_SEMANTIC_DELETES as u32,
            )
            .await;
        listing_degradation = listed.degradation_reason.or_else(|| {
            (listed.entries.len() >= crate::memory_erasure::MAX_SEMANTIC_DELETES)
                .then_some(crate::memory_erasure::SEMANTIC_ERASURE_TRUNCATED)
        });
        // The listed copies this erase did not already name. An empty id cannot be addressed, and a
        // repeated id names one memory.
        let mut seen: std::collections::HashSet<&str> = erased.iter().map(String::as_str).collect();
        let mut listed_ids: Vec<String> = Vec::new();
        for entry in &listed.entries {
            if entry.memory_id.trim().is_empty() || !seen.insert(entry.memory_id.as_str()) {
                continue;
            }
            listed_ids.push(entry.memory_id.clone());
        }
        if !listed_ids.is_empty() {
            // A copy whose durable row still exists is a thread memory this erase does not name, so it
            // stays. Only the copies no durable row holds are this person's alone to erase.
            let durable: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM agent_memory WHERE org_id = $1 AND id = ANY($2)",
            )
            .bind(org_id)
            .bind(&listed_ids)
            .fetch_all(pool)
            .await?;
            let durable: std::collections::HashSet<String> = durable.into_iter().collect();
            let orphans: Vec<String> = listed_ids
                .into_iter()
                .filter(|memory_id| !durable.contains(memory_id))
                .collect();
            if !orphans.is_empty() {
                // Recall is suppressed before the semantic delete, so a copy whose delete degrades is
                // still filtered from every read, as ResetMemory does.
                sqlx::query(
                    "INSERT INTO forgotten_memory_ids (org_id, user_id, memory_id) \
                     SELECT $1, $2, id FROM unnest($3::text[]) AS orphaned(id) ON CONFLICT DO NOTHING",
                )
                .bind(org_id)
                .bind(user_id)
                .bind(&orphans)
                .execute(pool)
                .await?;
                targets.extend(orphans.into_iter().map(|memory_id| {
                    crate::memory_erasure::ErasedMemory {
                        memory_id,
                        owner: user_id.to_owned(),
                    }
                }));
            }
        }
    }
    let semantic = crate::memory_erasure::erase_semantic_copies(letta, org_id, &targets).await;

    let summary = UserErasureSummary {
        user_profiles,
        personal_memory,
        semantic_memory: semantic,
        listing_degradation,
    };
    if !summary.erasure_is_complete() {
        tracing::warn!(
            org_id,
            considered = semantic.considered(),
            confirmed = semantic.confirmed,
            unconfirmed = semantic.unconfirmed,
            listing_degradation = ?listing_degradation,
            degradation = ?semantic.degradation_reason,
            "GDPR user erasure incomplete: not every personal memory copy is confirmed erased"
        );
        metrics::counter!(
            "mp_session_semantic_erasure_unconfirmed_total",
            "operation" => "gdpr_user"
        )
        .increment(semantic.unconfirmed);
    }
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

    // WS10.S6: the run trace goes with its org, and only with its org.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn gdpr_org_purge_removes_trace_events() {
        use crate::loop_checkpoint::test_support::{scratch_pool, seed_run};
        let Some(pool) = scratch_pool().await else {
            return;
        };
        let (run, org, _) = seed_run(&pool, "persistent").await;
        let (other_run, other_org, _) = seed_run(&pool, "persistent").await;
        for (run_id, org_id) in [(&run, &org), (&other_run, &other_org)] {
            for seq in 1..=3_i64 {
                sqlx::query(
                    "INSERT INTO run_trace_events (event_id, run_id, org_id, producer, producer_seq,                      event_type, occurred_at, resource_ref)                      VALUES ($1, $2, $3, 'quarry-edge', $4, 'BROWSER_SESSION_OPENED', now(), $5)",
                )
                .bind(format!("gdpr-{run_id}-{seq}"))
                .bind(run_id)
                .bind(org_id)
                .bind(seq)
                .bind(format!("run/{run_id}"))
                .execute(&pool)
                .await
                .expect("seed trace row");
            }
        }
        let summary = purge_organization_data(&pool, &org, None).await.expect("purge");
        assert_eq!(summary.run_trace_events, 3);
        let left = |run_id: String| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>(
                    "SELECT count(*) FROM run_trace_events WHERE run_id = $1",
                )
                .bind(run_id)
                .fetch_one(&pool)
                .await
                .expect("count")
            }
        };
        assert_eq!(left(run).await, 0, "the erased org's trace is gone");
        assert_eq!(left(other_run).await, 3, "another org's trace is untouched");
        purge_organization_data(&pool, &other_org, None).await.expect("cleanup");
    }

    #[test]
    fn purge_summary_total_sums_every_field() {
        let summary = PurgeSummary {
            checkpoints: 1,
            subagent_edges: 1,
            todos: 1,
            plan_steps: 1,
            approvals: 1,
            approval_scope_grants: 1,
            plans: 1,
            memory_index: 1,
            artifact_revisions: 1,
            thread_goals: 1,
            thread_share_snapshots: 1,
            messages: 1,
            task_events: 1,
            task_assignments: 1,
            task_dependencies: 1,
            cron_fires: 1,
            tasks: 1,
            cron_schedules: 1,
            event_triggers: 1,
            promote_use_counts: 1,
            events: 1,
            run_trace_events: 1,
            run_request_snapshots: 1,
            run_request_snapshot_blobs: 1,
            runs: 1,
            threads: 1,
            hook_configs: 1,
            agent_skills: 1,
            skill_revisions: 1,
            agent_skill_versions: 1,
            agent_memory: 1,
            user_memory_controls: 1,
            user_profiles: 1,
            forgotten_memory_ids: 1,
            memory_control_events: 1,
            org_learning_policy_mirror: 1,
            org_routing_overlays: 1,
            finetune_jobs: 1,
            dream_runs: 1,
            session_audit_outbox: 1,
            // Deliberately non-zero: `total()` counts Postgres rows deleted,
            // and the semantic tier is a different store. If someone folds
            // this into total() the assertion below breaks, which is the point
            // — the exclusion is a decision, not an omission.
            semantic_memory: crate::memory_erasure::SemanticErasure {
                confirmed: 5,
                unconfirmed: 7,
                degradation_reason: Some("DEGRADED_TEST"),
            },
        };
        assert_eq!(
            summary.total(),
            40,
            "total() must count only Postgres rows; semantic-tier counts belong \
             to erasure_is_complete(), not to this sum"
        );
        assert!(
            !summary.erasure_is_complete(),
            "an unconfirmed semantic count must make the erasure proof partial"
        );
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
        // The test module below formats table names into its own coverage checks (the held-scope
        // test), so only the code above it is scanned.
        let production = &source[..source.find("#[cfg(test)]").expect("the test module")];
        assert!(
            !production.contains("format!(\"DELETE"),
            "a DELETE must never be built with format!/string interpolation"
        );

        let pos = |needle: &str| {
            source
                .find(needle)
                .unwrap_or_else(|| panic!("expected to find {needle:?} in gdpr.rs"))
        };

        let checkpoints = pos("DELETE FROM checkpoints");
        let loop_checkpoints = pos("DELETE FROM run_loop_checkpoints");
        let subagent_edges = pos("DELETE FROM subagent_edges");
        let todos = pos("DELETE FROM todos");
        let plan_steps = pos("DELETE FROM plan_steps");
        let approvals = pos("DELETE FROM approvals WHERE org_id = $1");
        let plans = pos("DELETE FROM plans WHERE org_id = $1");
        let artifact_revisions = pos("DELETE FROM artifact_revisions WHERE org_id = $1");
        let thread_goals = pos("DELETE FROM thread_goals WHERE org_id = $1");
        let thread_share_snapshots = pos("DELETE FROM thread_share_snapshots WHERE org_id = $1");
        let messages = pos("DELETE FROM messages");
        let task_events = pos("DELETE FROM task_events");
        let task_assignments = pos("DELETE FROM task_assignments");
        let task_dependencies = pos("DELETE FROM task_dependencies");
        let cron_fires = pos("DELETE FROM cron_fires");
        let tasks = pos("DELETE FROM tasks WHERE org_id = $1");
        let cron_schedules = pos("DELETE FROM cron_schedules WHERE org_id = $1");
        let event_triggers = pos("DELETE FROM event_triggers WHERE org_id = $1");
        let events = pos("DELETE FROM events");
        let trace_events = pos("DELETE FROM run_trace_events WHERE org_id = $1");
        let snapshot_blobs = pos("DELETE FROM run_request_snapshot_blobs WHERE org_id = $1");
        let snapshots = pos("DELETE FROM run_request_snapshots WHERE org_id = $1");
        let runs = pos("DELETE FROM runs WHERE org_id = $1");
        let threads = pos("DELETE FROM threads WHERE org_id = $1");

        assert!(loop_checkpoints < runs, "loop folds must be purged before runs");
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
            artifact_revisions < messages && artifact_revisions < threads,
            "artifact revisions (EP-3 S2) must be purged before their messages and threads"
        );
        assert!(
            thread_goals < threads,
            "thread goals (EP-7 S1) must be purged before their threads"
        );
        assert!(
            thread_share_snapshots < threads,
            "thread share snapshots (EP-12 S14) must be purged before their threads"
        );
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
        assert!(
            trace_events < runs,
            "the run trace must be purged before runs (its rows reference them)"
        );
        assert!(
            snapshot_blobs < runs && snapshots < runs,
            "request snapshots must be purged before runs (their rows reference them)"
        );
        assert!(runs < threads, "runs must be purged before threads");
        assert!(
            event_triggers < runs && event_triggers < threads,
            "event trigger records (W4-F T1) must be purged before runs and threads"
        );
        assert!(
            pos("DELETE FROM agent_skills WHERE org_id = $1")
                < pos("DELETE FROM skill_revisions WHERE org_id = $1"),
            "deleting a skill writes a revision holding its text, so the revisions are purged after the skills"
        );
        // EP-11 S2: the versions are purged under the erasure setting, before the skills, and the
        // setting is put back before anything else runs.
        let erasure_on = pos("set_config('mp.version_erasure', 'on', true)");
        let versions = pos("DELETE FROM agent_skill_versions WHERE org_id = $1");
        let erasure_off = pos("set_config('mp.version_erasure', '', true)");
        assert!(
            erasure_on < versions && versions < erasure_off,
            "the version delete runs inside the erasure setting"
        );
        assert!(
            erasure_off < pos("DELETE FROM agent_skills WHERE org_id = $1"),
            "skill versions are purged before the skills"
        );
    }

    /// WS02-S1: the org erasure switches the Space event triggers off before any delete that
    /// could fire them, and purges the Space log beside the org's events, before runs and
    /// threads. Without the suppression the deferred delete triggers would write fresh
    /// tombstones for an org that was just erased.
    #[test]
    fn purge_suppresses_space_events_before_deletes_and_purges_the_log() {
        let source = include_str!("gdpr.rs");
        let pos = |needle: &str| {
            source
                .find(needle)
                .unwrap_or_else(|| panic!("expected to find {needle:?} in gdpr.rs"))
        };
        let suppress = pos("space::events::suppress(");
        let first_delete = pos("DELETE FROM run_loop_checkpoints");
        let events = pos("DELETE FROM events");
        let log = pos("DELETE FROM space_event_log WHERE org_id = $1");
        let heads = pos("DELETE FROM space_event_heads WHERE org_id = $1");
        let runs = pos("DELETE FROM runs WHERE org_id = $1");
        let messages = pos("DELETE FROM messages");
        let threads = pos("DELETE FROM threads WHERE org_id = $1");
        assert!(
            suppress < first_delete,
            "suppress must precede the first DELETE of the transaction"
        );
        assert!(suppress < messages && suppress < runs && suppress < threads);
        assert!(
            events < log && log < heads && heads < runs,
            "the Space log is purged beside the org's events, and its heads after it"
        );
        assert!(log < threads, "the log is purged before the threads whose delete triggers fire");
    }

    #[test]
    fn user_erasure_is_parsed_and_every_other_subject_is_not() {
        let user = serde_json::to_vec(&serde_json::json!({
            "subject_type": "user", "subject_id": "user_1", "org_id": "org_1",
        }))
        .unwrap();
        assert_eq!(
            parse_user_erasure_event(&user).unwrap(),
            Some(UserErasure {
                org_id: "org_1".to_owned(),
                user_id: "user_1".to_owned(),
                kind: UserErasureKind::Erase,
            })
        );
        let anonymize = serde_json::to_vec(&serde_json::json!({
            "subject_type": "user_anonymize", "subject_id": "user_1", "org_id": "org_1",
        }))
        .unwrap();
        assert_eq!(
            parse_user_erasure_event(&anonymize).unwrap(),
            Some(UserErasure {
                org_id: "org_1".to_owned(),
                user_id: "user_1".to_owned(),
                kind: UserErasureKind::Anonymize,
            })
        );
        for subject_type in ["organization", "other"] {
            let payload = serde_json::to_vec(&serde_json::json!({
                "subject_type": subject_type, "subject_id": "org_1", "org_id": "org_1",
            }))
            .unwrap();
            assert_eq!(parse_user_erasure_event(&payload).unwrap(), None, "{subject_type}");
        }
        let no_user = serde_json::to_vec(&serde_json::json!({"subject_type": "user", "org_id": "org_1"})).unwrap();
        assert_eq!(parse_user_erasure_event(&no_user), Err(ErasureEventError::MissingSubjectId));
        assert!(parse_erasure_event(&user).unwrap().is_none(), "a user erasure never purges the org");
    }

    #[test]
    fn space_source_erasure_deletes_sole_rows_and_narrows_shared_ones() {
        let attributed: std::collections::HashMap<String, String> = [
            ("message:m1", "user_1"),
            ("message:m2", "user_2"),
            ("tool:save_memory", "user_1"),
        ]
        .into_iter()
        .map(|(link, subject)| (link.to_owned(), subject.to_owned()))
        .collect();
        let links = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            space_source_erasure(&links(&["thread:t1", "message:m1"]), &attributed, "user_1"),
            SpaceSourceErasure::Delete
        );
        assert_eq!(
            space_source_erasure(&links(&["thread:t1", "tool:save_memory"]), &attributed, "user_1"),
            SpaceSourceErasure::Delete,
            "an explicit save of theirs alone"
        );
        assert_eq!(
            space_source_erasure(&links(&["thread:t1", "message:m1", "message:m2"]), &attributed, "user_1"),
            SpaceSourceErasure::Keep(links(&["thread:t1", "message:m2"]))
        );
        assert_eq!(
            space_source_erasure(&links(&["thread:t1", "message:m2"]), &attributed, "user_1"),
            SpaceSourceErasure::Untouched
        );
        assert_eq!(
            space_source_erasure(&links(&["thread:t1", "message:gone", "message:m1"]), &attributed, "user_1"),
            SpaceSourceErasure::Delete,
            "a link nobody is attributed to supports nobody"
        );
    }

    /// EP-8 S7: erasing a person deletes the Space rows only they were the source of and drops
    /// their message from rows other members also stated; other rows are untouched.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres whose name contains \"test\""]
    async fn gdpr_erasure_drops_user_sources() {
        use crate::space::test_support;
        let Some(pool) = test_support::connect().await else { return };
        let nonce = mp_ids::new_ulid();
        let org = format!("gdpr-space-org-{nonce}");
        let space = format!("gdpr-space-{nonce}");
        let thread = format!("gdpr-space-thread-{nonce}");
        let (mine, theirs) = (format!("gdpr-msg-a-{nonce}"), format!("gdpr-msg-b-{nonce}"));
        test_support::seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        test_support::seed_space_message(&pool, &space, &thread, &mine, "user", Some("erased-user")).await;
        test_support::seed_space_message(&pool, &space, &thread, &theirs, "user", Some("other-user")).await;
        let insert = |key: &'static str, links: Vec<String>| {
            let (pool, org, space) = (pool.clone(), org.clone(), space.clone());
            async move {
                let id = mp_ids::new_ulid();
                sqlx::query(
                    "INSERT INTO agent_memory (id, org_id, scope, scope_id, key, content, kind, confidence, owner, \
                     source_links, review_state, audience_hash, audience_revision, source_label) \
                     VALUES ($1, $2, 'space', $3, $4, 'c', 'fact', 0.9, '', $5, 'accepted', 'sha256:audience-1', 1, \
                     'room_statement')",
                )
                .bind(&id)
                .bind(&org)
                .bind(&space)
                .bind(format!("{space}/{key}"))
                .bind(links)
                .execute(&pool)
                .await
                .expect("seed space row");
                id
            }
        };
        let thread_link = format!("thread:{thread}");
        let sole = insert("sole", vec![thread_link.clone(), format!("message:{mine}")]).await;
        let shared = insert("shared", vec![thread_link.clone(), format!("message:{mine}"), format!("message:{theirs}")]).await;
        let other = insert("other", vec![thread_link.clone(), format!("message:{theirs}")]).await;

        let summary = erase_user_space_memory_sources(&pool, &org, "erased-user").await.expect("erasure");
        assert_eq!(summary, SpaceSourceSummary { deleted: 1, narrowed: 1 });
        let links_of = |id: String| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, Vec<String>>("SELECT source_links FROM agent_memory WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&pool)
                    .await
                    .expect("read row")
            }
        };
        assert_eq!(links_of(sole).await, None, "a row only the person stated is gone");
        assert_eq!(links_of(shared).await, Some(vec![thread_link.clone(), format!("message:{theirs}")]));
        assert_eq!(links_of(other).await, Some(vec![thread_link, format!("message:{theirs}")]));
        assert_eq!(
            erase_user_space_memory_sources(&pool, &org, "erased-user").await.expect("redelivery"),
            SpaceSourceSummary::default(),
            "a redelivered event finds nothing left"
        );
        sqlx::query("DELETE FROM agent_memory WHERE org_id = $1").bind(&org).execute(&pool).await.ok();
        test_support::clean_space(&pool, &org, &space).await;
    }

    // EP-9 C4: legal holds against erasure.

    fn hold_env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    fn hold_gate(url: &str) -> ErasureHoldGate {
        ErasureHoldGate::from_lookup(hold_env(&[
            (ERASURE_HOLD_CHECK_ENV, "true"),
            (crate::org_policy::ORG_CORE_URL_ENV, url),
            (crate::org_policy::SERVICE_TOKEN_ENV, "session-token"),
        ]))
    }

    fn check_answer(org_held: bool, held: &[&str]) -> wiremock::ResponseTemplate {
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "organization_id": "org_1",
            "window_days": null,
            "revision": 0,
            "org_held": org_held,
            "held_thread_ids": held,
        }))
    }

    async fn mount_check(server: &wiremock::MockServer, answer: wiremock::ResponseTemplate) {
        use wiremock::matchers::{header, method, path};
        server.reset().await;
        wiremock::Mock::given(method("POST"))
            .and(path("/internal/orgs/org_1/retention/check"))
            .and(header("x-service-id", "session-core"))
            .and(header("x-service-token", "session-token"))
            .respond_with(answer)
            .mount(server)
            .await;
    }

    fn trailer(status: &Status) -> Option<String> {
        status
            .metadata()
            .get(crate::run_phase::ERROR_CODE_TRAILER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    #[test]
    fn erasure_hold_switch_is_strict() {
        for value in ["", "1", "TRUE", "True", "yes", " true"] {
            let gate = ErasureHoldGate::from_lookup(hold_env(&[
                (ERASURE_HOLD_CHECK_ENV, value),
                (crate::org_policy::ORG_CORE_URL_ENV, "http://org-core:8080"),
                (crate::org_policy::SERVICE_TOKEN_ENV, "session-token"),
            ]));
            assert!(matches!(gate, ErasureHoldGate::Off), "{value:?} must leave the check off");
        }
        assert!(matches!(ErasureHoldGate::from_lookup(hold_env(&[])), ErasureHoldGate::Off));
        assert!(matches!(hold_gate("http://org-core:8080"), ErasureHoldGate::On(_)));
        assert!(matches!(
            ErasureHoldGate::from_lookup(hold_env(&[(ERASURE_HOLD_CHECK_ENV, "true")])),
            ErasureHoldGate::Unconfigured
        ));
    }

    #[tokio::test]
    async fn erasure_hold_gate_off_asks_nothing() {
        // No server at all: an Off gate must not try to reach one.
        let gate = ErasureHoldGate::from_lookup(hold_env(&[]));
        assert_eq!(gate.ensure_not_held("org_1", &["thr_1".to_owned()]).await, Ok(()));
        assert_eq!(gate.ensure_not_held("org_1", &[]).await, Ok(()));
    }

    #[tokio::test]
    async fn erasure_hold_unconfigured_refuses_as_unavailable() {
        let gate = ErasureHoldGate::from_lookup(hold_env(&[(ERASURE_HOLD_CHECK_ENV, "true")]));
        let refusal = gate.ensure_not_held("org_1", &["thr_1".to_owned()]).await.unwrap_err();
        assert!(matches!(refusal, ErasureHoldError::Unavailable(_)));
        let status = refusal.into_status();
        assert_eq!(status.code(), tonic::Code::Unavailable);
        assert_eq!(trailer(&status).as_deref(), Some(ERASURE_HOLD_CHECK_UNAVAILABLE));
    }

    /// A held thread is refused, and the same delete passes once the hold is released.
    #[tokio::test]
    async fn thread_hold_refuses_until_released() {
        let server = wiremock::MockServer::start().await;
        let gate = hold_gate(&server.uri());
        let thread = ["thr_1".to_owned()];

        mount_check(&server, check_answer(false, &["thr_1"])).await;
        let refusal = gate.ensure_not_held("org_1", &thread).await.unwrap_err();
        assert_eq!(refusal, ErasureHoldError::ThreadsHeld(1));
        let status = refusal.into_status();
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(trailer(&status).as_deref(), Some(ERASURE_BLOCKED_LEGAL_HOLD));
        assert!(
            status.message().contains("thread_goals") && status.message().contains("artifact_revisions"),
            "the refusal names goals and revisions among what is kept: {}",
            status.message()
        );

        mount_check(&server, check_answer(false, &[])).await;
        assert_eq!(gate.ensure_not_held("org_1", &thread).await, Ok(()));
    }

    /// An org hold blocks a thread delete and the org purge alike, until released.
    #[tokio::test]
    async fn org_hold_refuses_until_released() {
        let server = wiremock::MockServer::start().await;
        let gate = hold_gate(&server.uri());

        mount_check(&server, check_answer(true, &[])).await;
        assert_eq!(
            gate.ensure_not_held("org_1", &["thr_free".to_owned()]).await,
            Err(ErasureHoldError::OrgHeld)
        );
        assert_eq!(gate.ensure_not_held("org_1", &[]).await, Err(ErasureHoldError::OrgHeld));

        mount_check(&server, check_answer(false, &[])).await;
        assert_eq!(gate.ensure_not_held("org_1", &[]).await, Ok(()));
    }

    /// An outage, an unexpected status or a body that is not a check answer refuses with the
    /// retryable code; org-core's definitive 404 (organisation gone, holds with it) does not.
    #[tokio::test]
    async fn hold_check_failures_refuse_and_a_gone_org_does_not() {
        let server = wiremock::MockServer::start().await;
        let gate = hold_gate(&server.uri());
        let thread = ["thr_1".to_owned()];

        for answer in [
            wiremock::ResponseTemplate::new(500),
            wiremock::ResponseTemplate::new(503),
            wiremock::ResponseTemplate::new(403),
            wiremock::ResponseTemplate::new(200).set_body_string("not json"),
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"held_thread_ids": []})),
        ] {
            mount_check(&server, answer).await;
            assert!(matches!(
                gate.ensure_not_held("org_1", &thread).await,
                Err(ErasureHoldError::Unavailable(_))
            ));
        }

        mount_check(&server, wiremock::ResponseTemplate::new(404)).await;
        assert_eq!(gate.ensure_not_held("org_1", &thread).await, Ok(()));

        // Unreachable org-core: nothing listens on the dropped server's port.
        let gone = hold_gate("http://127.0.0.1:9");
        assert!(matches!(
            gone.ensure_not_held("org_1", &thread).await,
            Err(ErasureHoldError::Unavailable(_))
        ));
    }

    /// A bulk erase of more threads than one check may name is asked page by page, and a hold
    /// on the last page still refuses it.
    #[tokio::test]
    async fn hold_check_pages_large_erases() {
        use wiremock::matchers::{body_partial_json, method, path};
        let server = wiremock::MockServer::start().await;
        let gate = hold_gate(&server.uri());
        let threads: Vec<String> = (0..(CHECK_PAGE + 3)).map(|i| format!("thr_{i:04}")).collect();

        wiremock::Mock::given(method("POST"))
            .and(path("/internal/orgs/org_1/retention/check"))
            .and(body_partial_json(serde_json::json!({"thread_ids": [format!("thr_{:04}", CHECK_PAGE)]})))
            .respond_with(check_answer(false, &["thr_0502"]))
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        wiremock::Mock::given(method("POST"))
            .and(path("/internal/orgs/org_1/retention/check"))
            .respond_with(check_answer(false, &[]))
            .with_priority(2)
            .expect(1)
            .mount(&server)
            .await;

        assert_eq!(
            gate.ensure_not_held("org_1", &threads).await,
            Err(ErasureHoldError::ThreadsHeld(1))
        );
        server.verify().await;
    }

    /// C-B9 and EP-12 S14: the held scope names thread goals, artifact revisions and share
    /// snapshots, and both erase paths still remove them, so the list and the deletes cannot
    /// drift apart unnoticed.
    #[test]
    fn held_scope_names_goals_and_revisions_that_erasure_removes() {
        for table in ["thread_goals", "artifact_revisions", "thread_share_snapshots"] {
            assert!(HELD_THREAD_CONTENT.contains(&table), "{table} must be in the held scope");
            assert!(
                include_str!("grpc.rs").contains(&format!("DELETE FROM {table} WHERE thread_id = $1")),
                "delete_thread_rows must remove {table}"
            );
            assert!(
                include_str!("gdpr.rs").contains(&format!("DELETE FROM {table} WHERE org_id = $1")),
                "the org purge must remove {table}"
            );
        }
    }

    /// The org purge asks the gate before it opens its transaction.
    #[test]
    fn org_purge_checks_holds_before_any_delete() {
        let source = include_str!("gdpr.rs");
        let check = source
            .find("erasure_hold_gate().ensure_not_held(org_id, &[])")
            .expect("the purge asks the hold gate");
        let begin = source.find("let mut tx: Transaction<'_, Postgres> = pool.begin()").expect("purge transaction");
        assert!(check < begin, "the hold check must precede the purge transaction");
    }
}
