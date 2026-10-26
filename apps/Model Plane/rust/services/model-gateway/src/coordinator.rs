//! Wave 10b — plan mode + team-worker coordination.
//!
//! Two in-memory stores, both backed by `dashmap` for cheap concurrent
//! access:
//!
//! 1. `PlanModeStore` — per-run flags with a TTL. When a run is in
//!    plan mode, write-class tools (`file_edit`, bash, `remote_trigger`)
//!    are expected to gate via `is_plan_mode` before executing; read-class
//!    tools (Fetch, `WebSearch`, `ExtractStructured`) flow normally.
//!    `SendMessage` is independently quarantined and always fails closed.
//!
//! 2. `TeamWorkerStore` — coordinator-mode sub-task tracker. A worker
//!    is a (objective, context, `success_criteria`) triple with a state
//!    machine: pending → running → completed | failed. Used by the
//!    coordinator agent to fan out parallel sub-work.
//!
//! Both stores are intentionally in-memory:
//!  - Plan mode is short-lived (TTL default 1h) and doesn't need to
//!    survive a gateway restart — operators restart the gateway, the
//!    LLM call retries, plan mode gets re-entered.
//!  - Team workers are run-scoped; the coordinator's parent run owns
//!    them. If the gateway restarts mid-run the orchestrator restarts
//!    the run from its Temporal checkpoint, which re-creates them.
//!
//! Promote to postgres if either invariant ever stops holding.

// tonic::Status is the unavoidable large Err for gRPC handlers; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use dashmap::DashMap;
use mp_events::{publisher::EventPublisher, subjects};
use mp_ids::new_ulid;
use tonic::transport::{Channel, Endpoint};
use tonic::Status;

use mp_contracts::model_plane::v1::{
    thread_autonomy_client::ThreadAutonomyClient,
    EnterPlanModeRequest, EnterPlanModeResponse, ExitPlanModeRequest, ExitPlanModeResponse,
    IsPlanModeRequest, IsPlanModeResponse, TeamCreateRequest, TeamCreateResponse,
    TeamDeleteRequest, TeamDeleteResponse, TeamListRequest, TeamListResponse, TeamWorker,
};

/// Default plan-mode TTL when the caller doesn't supply one. 1 hour
/// matches the v2 implementation; long enough for a human review,
/// short enough that an abandoned run can't lock a session forever.
const DEFAULT_PLAN_TTL_SECS: i64 = 3600;

/// Hard cap on plan-mode TTL. Anything beyond a day is almost
/// certainly a bug.
const MAX_PLAN_TTL_SECS: i64 = 86_400;

/// Max workers returned per `TeamList` call.
const MAX_TEAM_LIST_LIMIT: i32 = 500;
const DEFAULT_TEAM_LIST_LIMIT: i32 = 100;

// ---------------- Plan mode ----------------

#[derive(Debug, Clone)]
struct PlanModeEntry {
    org_id: String,
    rationale: String,
    expires_at_unix: i64,
}

/// In-memory plan-mode store. Cheap clone (Arc<DashMap>).
#[derive(Clone, Default, Debug)]
pub struct PlanModeStore {
    inner: Arc<DashMap<String, PlanModeEntry>>,
    /// Autonomy grants from approved plans, keyed by THREAD — the follow-up
    /// run is a new run, so a run-keyed entry could never reach it. Written by
    /// `handle_exit_plan_mode`, read by the invoke path when constructing the
    /// next `RunAgentRequest`. Until this map existed, the grant was persisted
    /// to the plan run's metadata with a comment claiming "the next
    /// RunAgentRequest reads it back" — no such read existed anywhere, so the
    /// per-call rung gate could never refuse anything.
    ///
    /// In-memory, so a gateway restart drops the grant and the next run falls
    /// back to UNSPECIFIED (no graded constraint — today's behaviour for every
    /// run). That is fail-open for the graded ladder and is accepted ONLY
    /// because the durable posture gates (plan mode, risky-call approvals)
    /// are unaffected; the durable thread-scoped carrier belongs in
    /// session-core and is recorded as follow-up in the decisions ledger.
    grants: Arc<DashMap<String, GrantEntry>>,
    /// Whether an execute turn (`execute_granted`, WS14 S2a "run the approved plan") may
    /// run on this gateway. Decided once, when the store is built from the process
    /// environment ([`PlanModeStore::new`]); a store built any other way has it off.
    /// Held here rather than read per request so every handler agrees and a test can
    /// state it without touching the process environment.
    execute_enabled: bool,
    /// Whether a per-call approval of a paused run can resume it on this gateway: the
    /// approval-resume switch (`AGENT_CONTINUE_AFTER_APPROVAL_ENABLED`, read through
    /// `mp_contracts::loop_fold::continue_after_approval_enabled`, the same reader
    /// `continue_run` uses), decided once with the store. An execute turn is only ever run
    /// when this is on: the posture ratchet runs the widest rung under `ask`, and an `ask`
    /// run that pauses for an approval nothing can resume strands the run and spends the
    /// user's click. A store built any other way has it off.
    approval_resume_enabled: bool,
    /// Whether plan grants live in Session Core instead of this store's memory (build plan
    /// WS04 S4, [`DURABLE_PLAN_GRANT_ENV`]). Decided once, with the store, so the approval
    /// that writes a grant and the run start that reads it can never disagree about where
    /// it is. With it on this store's `grants` half is not read and not written; with it off
    /// (the default) every path is the in-memory one it always was.
    durable_grant_enabled: bool,
    /// The reader of the durable grants (`ThreadAutonomy` on Session Core's endpoint). Held
    /// here, beside the switch it serves, so the two travel together. `None` while the switch
    /// is off, or when the endpoint could not be built: a read then fails and a run start
    /// finds its grant unreadable: an execute turn is refused and any other run is held to
    /// READ_ONLY, rather than guessed at.
    durable_grant_client: Option<ThreadAutonomyClient<Channel>>,
}

/// The environment variable of the execute-turn kill switch. Named and read here and
/// nowhere else (`nothing_outside_the_one_parser_reads_the_execute_switch`).
///
/// Default off: with it off an execute turn is refused, typed (`plan_execute_disabled`,
/// not retryable), before any run exists, and every other turn is untouched. No other
/// service reads it, but it is coupled to the approval-resume switch of the WS03 flip
/// (`AGENT_CONTINUE_AFTER_APPROVAL_ENABLED`, [`PlanModeStore::approval_resume_enabled`]): an
/// execute turn is refused the same way while that one is off, because the widest rung runs
/// under `ask` and an approval nothing can resume strands the run. It deploys with the
/// gateway first, then the BFF, then the SPA, after the WS03 flip; turn it off before that
/// flip is rolled back.
pub const PLAN_EXECUTE_ENV: &str = "MODEL_GATEWAY_PLAN_EXECUTE_ENABLED";

/// The one strict parser of the execute switch: the shared durable-loop parser, `true`
/// in any case with surrounding whitespace and nothing else, so a typo or a
/// half-recognised value never enables execute turns.
#[must_use]
pub fn plan_execute_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// The environment variable of the durable plan grant switch (build plan WS04 S4). Named and
/// read here and nowhere else (`nothing_outside_the_one_parser_reads_the_durable_grant_switch`).
///
/// Default off: grants stay in the memory of the replica that took the approval, exactly as
/// before. On, the approval records the grant on its thread in Session Core, in the same
/// transaction as the run update, and every run start reads it from there, so a Do run
/// started on another replica, or after a restart, carries the rung. Turn it off to roll
/// back; a grant recorded durably is then simply no longer read.
pub const DURABLE_PLAN_GRANT_ENV: &str = "MODEL_GATEWAY_DURABLE_PLAN_GRANT";

/// The one strict parser of the durable grant switch: the shared durable-loop parser, `true`
/// in any case with surrounding whitespace and nothing else.
#[must_use]
pub fn durable_plan_grant_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// How long the lazy channel to Session Core waits to connect. Matches the other clients of
/// that endpoint (`state.rs`).
const DURABLE_GRANT_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The `ThreadAutonomy` client on the Session Core endpoint the other session clients use
/// (`SESSION_CORE_URL`, else `SESSION_CORE_ADDR`, else the local default). `None` when the
/// endpoint is not a valid URI or no runtime is running to own the channel; either is logged.
fn durable_grant_client_from_env() -> Option<ThreadAutonomyClient<Channel>> {
    let endpoint = std::env::var("SESSION_CORE_URL")
        .or_else(|_| std::env::var("SESSION_CORE_ADDR"))
        .unwrap_or_else(|_| "http://localhost:9091".to_owned());
    let normalized = if endpoint.contains("://") {
        endpoint
    } else {
        format!("http://{endpoint}")
    };
    if tokio::runtime::Handle::try_current().is_err() {
        tracing::error!("durable plan grant switch is on but no runtime is running to own its channel");
        return None;
    }
    match Endpoint::from_shared(normalized) {
        Ok(endpoint) => Some(ThreadAutonomyClient::new(
            endpoint
                .connect_timeout(DURABLE_GRANT_CONNECT_TIMEOUT)
                .connect_lazy(),
        )),
        Err(error) => {
            tracing::error!(%error, "durable plan grant switch is on but the Session Core endpoint is invalid");
            None
        }
    }
}

#[derive(Debug)]
struct GrantEntry {
    org_id: String,
    rung: mp_contracts::model_plane::v1::AutonomyRung,
    expires_at_unix: i64,
    /// The plan-gate plan this grant was approved TOGETHER with, until the
    /// next run takes it (`take_approved_plan`). `None` for a grant made
    /// without a structured plan, and once the plan has been handed to a run:
    /// the rung outlives the plan's consumption (it covers the thread until
    /// it expires), the plan does not.
    approved_plan_id: Option<String>,
}

impl PlanModeStore {
    /// The store the gateway runs with: empty, with the execute switch read ONCE from the
    /// process environment ([`PLAN_EXECUTE_ENV`], through [`plan_execute_from`]) and the
    /// durable grant switch likewise ([`DURABLE_PLAN_GRANT_ENV`]).
    #[must_use]
    pub fn new() -> Self {
        let durable_grant_enabled =
            durable_plan_grant_from(std::env::var(DURABLE_PLAN_GRANT_ENV).ok().as_deref());
        Self {
            execute_enabled: plan_execute_from(std::env::var(PLAN_EXECUTE_ENV).ok().as_deref()),
            approval_resume_enabled: mp_contracts::loop_fold::continue_after_approval_enabled(),
            durable_grant_enabled,
            durable_grant_client: durable_grant_enabled
                .then(durable_grant_client_from_env)
                .flatten(),
            ..Self::default()
        }
    }

    /// Whether plan grants live in Session Core (the durable grant switch). Off by default.
    #[must_use]
    pub fn durable_grant_enabled(&self) -> bool {
        self.durable_grant_enabled
    }

    /// This store with the durable grant switch set to `enabled`; the grants stay shared with
    /// every clone. For a test that states the switch without the process environment.
    #[must_use]
    pub fn with_durable_grant_enabled(mut self, enabled: bool) -> Self {
        self.durable_grant_enabled = enabled;
        self
    }

    /// This store reading durable grants through `client`, for a test that points the read at
    /// a Session Core it controls.
    #[must_use]
    pub fn with_durable_grant_client(mut self, client: ThreadAutonomyClient<Channel>) -> Self {
        self.durable_grant_client = Some(client);
        self
    }

    /// The reader of the durable grants, or `None` when there is none to read from.
    #[must_use]
    pub fn durable_grant_client(&self) -> Option<ThreadAutonomyClient<Channel>> {
        self.durable_grant_client.clone()
    }

    /// Whether a paused run's approval can be resumed on this gateway (the approval-resume
    /// switch the `/continue` route is gated by). Off by default. An execute turn needs it.
    #[must_use]
    pub fn approval_resume_enabled(&self) -> bool {
        self.approval_resume_enabled
    }

    /// This store with the approval-resume switch set to `enabled`; the grants stay shared
    /// with every clone. For a test that states the switch without the process environment.
    #[must_use]
    pub fn with_approval_resume_enabled(mut self, enabled: bool) -> Self {
        self.approval_resume_enabled = enabled;
        self
    }

    /// Whether an execute turn may run on this gateway (the kill switch). Off by default.
    #[must_use]
    pub fn execute_enabled(&self) -> bool {
        self.execute_enabled
    }

    /// This store with the execute switch set to `enabled`; the grants stay shared with
    /// every clone. For a test that states the switch without the process environment.
    #[must_use]
    pub fn with_execute_enabled(mut self, enabled: bool) -> Self {
        self.execute_enabled = enabled;
        self
    }

    /// Set or refresh the plan-mode flag for `run_id`. Returns the new
    /// expiry as a unix epoch second.
    fn enter(&self, org_id: &str, run_id: &str, rationale: String, ttl_secs: i64) -> i64 {
        let expires_at = now_unix() + ttl_secs.clamp(1, MAX_PLAN_TTL_SECS);
        self.inner.insert(
            run_id.to_owned(),
            PlanModeEntry {
                org_id: org_id.to_owned(),
                rationale,
                expires_at_unix: expires_at,
            },
        );
        expires_at
    }

    /// Clear the plan-mode flag. Returns true when there was an entry
    /// to clear; false when the run wasn't in plan mode.
    fn exit(&self, org_id: &str, run_id: &str) -> bool {
        let owned = self
            .inner
            .get(run_id)
            .is_some_and(|entry| entry.org_id == org_id);
        owned && self.inner.remove(run_id).is_some()
    }

    /// True when the run is currently in plan mode and the entry
    /// hasn't expired. Sweeps the entry if it has expired so the
    /// store doesn't accumulate dead state.
    pub fn is_plan_mode(&self, org_id: &str, run_id: &str) -> (bool, String, i64) {
        let now = now_unix();
        if let Some(entry) = self.inner.get(run_id) {
            if entry.org_id != org_id {
                return (false, String::new(), 0);
            }
            if entry.expires_at_unix > now {
                return (true, entry.rationale.clone(), entry.expires_at_unix);
            }
            // Lazily evict the expired entry. Drop the read guard
            // first so the writer can take the slot.
            let key = run_id.to_owned();
            drop(entry);
            self.inner.remove(&key);
        }
        (false, String::new(), 0)
    }

    /// Record the rung an approved plan granted for this thread.
    ///
    /// Same TTL ceiling as plan entries: a grant is an approval artifact, not a
    /// standing policy, and an unbounded one would quietly outlive the plan it
    /// authorized.
    ///
    /// `approved_plan_id` is the plan-gate plan approved in the same decision
    /// (empty when there is none). Remembering it here — rather than having
    /// the next run go looking for "the thread's latest approved plan" — is
    /// what binds the plan a run is told to follow to the approval that
    /// granted it, and it keeps that lookup off the hot path: the alternative
    /// listed every plan of the thread, with all their steps, at the start of
    /// every run for the 24 hours a grant lasts.
    pub fn record_grant(
        &self,
        org_id: &str,
        thread_id: &str,
        rung: mp_contracts::model_plane::v1::AutonomyRung,
        approved_plan_id: &str,
    ) {
        if thread_id.trim().is_empty()
            || rung == mp_contracts::model_plane::v1::AutonomyRung::Unspecified
        {
            return;
        }
        self.grants.insert(
            thread_id.to_owned(),
            GrantEntry {
                org_id: org_id.to_owned(),
                rung,
                expires_at_unix: now_unix() + MAX_PLAN_TTL_SECS,
                approved_plan_id: (!approved_plan_id.is_empty()).then(|| approved_plan_id.to_owned()),
            },
        );
    }

    /// Hand the plan approved with this thread's grant to the run that is
    /// starting — ONCE. The id is removed as it is returned, so of two runs
    /// starting together only one is told to follow the plan (session-core's
    /// compare-and-swap on the plan itself makes the same true across
    /// gateway replicas). Org-checked and expiry-checked like
    /// [`granted_rung`](Self::granted_rung); `None` when there is nothing
    /// pending.
    #[must_use]
    pub fn take_approved_plan(&self, org_id: &str, thread_id: &str) -> Option<String> {
        let mut entry = self.grants.get_mut(thread_id)?;
        if entry.org_id != org_id || entry.expires_at_unix <= now_unix() {
            return None;
        }
        entry.approved_plan_id.take()
    }

    /// The plan approved with this thread's grant, WITHOUT handing it to a run: the
    /// non-consuming twin of [`take_approved_plan`](Self::take_approved_plan), org-checked and
    /// expiry-checked the same way. For the durable grant switch, where the plan's own
    /// compare-and-swap in Session Core (not this memory) is the single-use guard.
    #[must_use]
    pub fn peek_approved_plan(&self, org_id: &str, thread_id: &str) -> Option<String> {
        let entry = self.grants.get(thread_id)?;
        if entry.org_id != org_id || entry.expires_at_unix <= now_unix() {
            return None;
        }
        entry.approved_plan_id.clone()
    }

    /// The rung a prior approved plan granted this thread, if any and unexpired.
    /// Org-checked like every other read here; an expired entry is swept.
    #[must_use]
    pub fn granted_rung(
        &self,
        org_id: &str,
        thread_id: &str,
    ) -> Option<mp_contracts::model_plane::v1::AutonomyRung> {
        let now = now_unix();
        if let Some(entry) = self.grants.get(thread_id) {
            if entry.org_id != org_id {
                return None;
            }
            if entry.expires_at_unix > now {
                return Some(entry.rung);
            }
            let key = thread_id.to_owned();
            drop(entry);
            // Remove only the entry that was seen expired: a grant recorded for the thread
            // in the meantime (a fresh approval) must survive this sweep.
            self.grants.remove_if(&key, |_, held| held.expires_at_unix <= now);
        }
        None
    }

    /// Plant a grant with an explicit expiry, for a test that needs a lapsed one
    /// (`record_grant` always stamps the full TTL from now).
    #[cfg(test)]
    pub(crate) fn plant_grant_for_test(
        &self,
        org_id: &str,
        thread_id: &str,
        rung: mp_contracts::model_plane::v1::AutonomyRung,
        expires_at_unix: i64,
    ) {
        self.grants.insert(
            thread_id.to_owned(),
            GrantEntry {
                org_id: org_id.to_owned(),
                rung,
                expires_at_unix,
                approved_plan_id: None,
            },
        );
    }

    /// The unix second the thread's live grant lapses, if there is one: the same
    /// org-check and expiry-check as [`granted_rung`](Self::granted_rung) (an expired entry
    /// is swept and reports nothing, another org's reports nothing). Read-only: it neither
    /// spends nor extends the grant. `plan_approval` states it so the client knows how long
    /// the rung it just granted lasts.
    #[must_use]
    pub fn grant_expires_at(&self, org_id: &str, thread_id: &str) -> Option<i64> {
        let now = now_unix();
        if let Some(entry) = self.grants.get(thread_id) {
            if entry.org_id != org_id {
                return None;
            }
            if entry.expires_at_unix > now {
                return Some(entry.expires_at_unix);
            }
            let key = thread_id.to_owned();
            drop(entry);
            // Remove only the entry that was seen expired: a grant recorded for the thread
            // in the meantime (a fresh approval) must survive this sweep.
            self.grants.remove_if(&key, |_, held| held.expires_at_unix <= now);
        }
        None
    }

    /// Forget this thread's grant in memory, but only the one that belongs to `org_id`. Called once
    /// an approval is durably recorded in Session Core: a copy kept here is only the fallback for an
    /// approval whose write failed, and an older copy must not keep answering for the thread
    /// (`with_local_fallback` honours the narrower of the two). Another org's entry is left alone.
    pub fn clear_grant(&self, org_id: &str, thread_id: &str) {
        self.grants.remove_if(thread_id, |_, held| held.org_id == org_id);
    }
}

/// Enters plan mode for a run and emits a best-effort lifecycle event.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.run_id` is empty. Event-publish
/// failures are logged and non-fatal, not surfaced as an `Err`.
pub async fn handle_enter_plan_mode<P: EventPublisher>(
    store: &PlanModeStore,
    publisher: &P,
    req: EnterPlanModeRequest,
) -> Result<EnterPlanModeResponse, Status> {
    if req.run_id.is_empty() {
        return Err(Status::invalid_argument("run_id is required"));
    }
    let ttl = if req.ttl_seconds <= 0 {
        DEFAULT_PLAN_TTL_SECS
    } else {
        i64::from(req.ttl_seconds)
    };
    let expires = store.enter(&req.org_id, &req.run_id, req.rationale.clone(), ttl);

    // Best-effort lifecycle event so audit + observability sees the
    // mode transition. Publish failure is non-fatal — the in-memory
    // store is the authoritative state for the flag itself.
    let envelope = mp_events::envelope::Envelope {
        event_id: new_ulid(),
        event_type: "RUN_PLAN_MODE_ENTERED".to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: req.request_id.clone(),
        causation_id: String::new(),
        idempotency_key: format!("plan-enter-{}", req.run_id),
        org_id: req.org_id.clone(),
        user_id: "agent".to_owned(),
        resource_ref: format!("run/{}", req.run_id),
        payload: serde_json::json!({
            "run_id": req.run_id,
            "session_id": req.session_id,
            "rationale": req.rationale,
            "ttl_seconds": ttl,
        }),
        zdr: false,
    };
    if let Err(e) = publisher
        .publish(&subjects::run_event_subject(&req.run_id), &envelope)
        .await
    {
        tracing::warn!(error = %e, "failed to publish RUN_PLAN_MODE_ENTERED");
    }

    Ok(EnterPlanModeResponse {
        request_id: req.request_id,
        expires_at_unix: expires,
    })
}

/// Whether a failed `SetRunMode` (the durable write of an approval) is a TRANSIENT failure of
/// Session Core, the only kind after which this replica keeps the grant in memory (WS04 S4).
/// A refusal (`FailedPrecondition`, `PermissionDenied`, `Unauthenticated`, `InvalidArgument`,
/// `NotFound`, anything else) is Session Core saying no: a grant it refused must not be honoured
/// from memory, or the refusal would be a way to be handed what it denied.
#[must_use]
pub fn set_run_mode_failure_is_transient(code: tonic::Code) -> bool {
    matches!(
        code,
        tonic::Code::Unavailable
            | tonic::Code::DeadlineExceeded
            | tonic::Code::Aborted
            | tonic::Code::ResourceExhausted
            | tonic::Code::Internal
    )
}

/// Exits plan mode for a run and emits a best-effort lifecycle event.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.run_id` is empty. Event-publish
/// failures are logged and non-fatal, not surfaced as an `Err`.
///
/// `plan_id` is the durable receipt/lineage binding plan-gate adds (§2.3):
/// the id of the `Plan` row this grant was approved against, or empty for a
/// caller with no plan-gate plan (the pre-existing gRPC `ExitPlanMode` path,
/// and coordinator's own tests). It is a plain Rust parameter, not a proto
/// field — `sessions.proto`'s `ExitPlanModeRequest` stays untouched.
pub async fn handle_exit_plan_mode<P: EventPublisher>(
    store: &PlanModeStore,
    publisher: &P,
    req: ExitPlanModeRequest,
    plan_id: &str,
) -> Result<ExitPlanModeResponse, Status> {
    exit_plan_mode(store, publisher, req, plan_id, None).await
}

/// [`handle_exit_plan_mode`] for a gateway whose grants are durable (build plan WS04 S4).
///
/// The grant is NOT recorded in this store: Session Core records it with the approval
/// (`SetRunMode`), and every replica reads it from there. And whether the run was in plan
/// mode is the caller's answer from the durable run row (`was_in_plan_mode`) OR this
/// replica's own flag: the flag was set by whichever replica started the run, so this one's
/// silence proves nothing, but the durable write that marks the run is best-effort, so this
/// replica's flag set proves the run was in plan mode even when the row says otherwise. The
/// lifecycle event is published whenever either says so, on any replica.
///
/// # Errors
///
/// As [`handle_exit_plan_mode`].
pub async fn handle_exit_plan_mode_durable<P: EventPublisher>(
    store: &PlanModeStore,
    publisher: &P,
    req: ExitPlanModeRequest,
    plan_id: &str,
    was_in_plan_mode: bool,
) -> Result<ExitPlanModeResponse, Status> {
    exit_plan_mode(store, publisher, req, plan_id, Some(was_in_plan_mode)).await
}

/// The body of both exits. `durable` is `None` for the in-memory grant (record it here, ask
/// this store whether the run was in plan mode) and `Some(was_in_plan_mode)` for the durable
/// one (record nothing here, take the caller's durable answer).
async fn exit_plan_mode<P: EventPublisher>(
    store: &PlanModeStore,
    publisher: &P,
    req: ExitPlanModeRequest,
    plan_id: &str,
    durable: Option<bool>,
) -> Result<ExitPlanModeResponse, Status> {
    if req.run_id.is_empty() {
        return Err(Status::invalid_argument("run_id is required"));
    }

    // Leaving plan mode IS the grant, so it must say what is granted and why.
    //
    // Before this, the request carried neither: any caller with write access
    // could flip a run out of plan mode, and nothing recorded what authority it
    // gained or on what grounds. That is exactly the shape DeepSeek calls a
    // malformed ask — "an approval prompt without a reason, or a reason driving
    // nothing". The run is leaving READ_ONLY by definition (plan mode is
    // investigate-only), so the escalation is validated against that floor.
    let requested = mp_contracts::model_plane::v1::AutonomyRung::try_from(req.granted_rung)
        .unwrap_or(mp_contracts::model_plane::v1::AutonomyRung::Unspecified);
    let escalation = mp_contracts::autonomy::AutonomyEscalation::request(
        mp_contracts::model_plane::v1::AutonomyRung::ReadOnly,
        requested,
        &req.justification,
    )
    .map_err(|refusal| Status::invalid_argument(refusal.message()))?;

    // The grant must reach the NEXT run, which is a different run on the same
    // thread — record it thread-keyed before anything else can fail. A durable grant is
    // Session Core's to record, and this memory is then not a second copy of it.
    if durable.is_none() {
        store.record_grant(&req.org_id, &req.session_id, escalation.to(), plan_id);
    }
    // The local flag is cleared either way. The run was in plan mode when EITHER this replica
    // held the flag OR the durable run row says so: `mark_run_plan_mode`'s durable write is
    // best-effort, so a run that entered plan mode here with that write failed has `runs.mode`
    // still `execute` and only this replica's flag. Following the durable answer alone would
    // then write a durable grant and publish nothing, and RUN_PLAN_MODE_EXITED is the only
    // event that carries the rung, the justification and the plan id.
    let was_local = store.exit(&req.org_id, &req.run_id);
    let was_active = durable.map_or(was_local, |was_durable| was_durable || was_local);
    if was_active {
        let envelope = mp_events::envelope::Envelope {
            event_id: new_ulid(),
            event_type: "RUN_PLAN_MODE_EXITED".to_owned(),
            schema_version: 1,
            ts: Utc::now(),
            producer: "model-gateway".to_owned(),
            correlation_id: req.request_id.clone(),
            causation_id: String::new(),
            idempotency_key: format!("plan-exit-{}", req.run_id),
            org_id: req.org_id.clone(),
            user_id: "agent".to_owned(),
            resource_ref: format!("run/{}", req.run_id),
            payload: serde_json::json!({
                "run_id": req.run_id,
                "session_id": req.session_id,
                // Both, together. A grant with no reason is unreviewable after
                // the fact, and a reason with no grant does not say what
                // changed.
                "granted_rung": mp_contracts::autonomy::label(escalation.to()),
                "justification": escalation.justification(),
                // plan-gate §2.3: the pointer from this durable, correlation-
                // id-bearing grant event to the exact `Plan` row (with its
                // edited `PlanStep`s) it was approved against — the
                // rung-grant's receipt/lineage binding. Omitted (not merely
                // empty-stringed) when there is no plan, so an older reader
                // doing `payload.get("plan_id")` sees an honest absence.
                "plan_id": (!plan_id.is_empty()).then(|| plan_id.to_owned()),
            }),
            zdr: false,
        };
        if let Err(e) = publisher
            .publish(&subjects::run_event_subject(&req.run_id), &envelope)
            .await
        {
            tracing::warn!(error = %e, "failed to publish RUN_PLAN_MODE_EXITED");
        }
    }
    Ok(ExitPlanModeResponse {
        request_id: req.request_id,
        was_active,
    })
}

/// Reports whether a run is currently in plan mode (and its TTL).
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.run_id` is empty.
pub fn handle_is_plan_mode(
    store: &PlanModeStore,
    req: IsPlanModeRequest,
) -> Result<IsPlanModeResponse, Status> {
    if req.run_id.is_empty() {
        return Err(Status::invalid_argument("run_id is required"));
    }
    let (in_plan_mode, rationale, expires_at_unix) = store.is_plan_mode(&req.org_id, &req.run_id);
    Ok(IsPlanModeResponse {
        request_id: req.request_id,
        in_plan_mode,
        rationale,
        expires_at_unix,
    })
}

// ---------------- Team workers ----------------

const STATE_PENDING: &str = "pending";
const STATE_COMPLETED: &str = "completed";
const STATE_FAILED: &str = "failed";

#[derive(Clone, Default, Debug)]
pub struct TeamWorkerStore {
    // Keyed by worker_id. Org isolation is enforced at the handler
    // layer; cross-org workers can't share an id because new_ulid is
    // collision-resistant.
    inner: Arc<DashMap<String, TeamWorker>>,
}

impl TeamWorkerStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn create(
        &self,
        org_id: String,
        objective: String,
        context: String,
        success_criteria: String,
    ) -> TeamWorker {
        let now = now_unix();
        let worker = TeamWorker {
            worker_id: new_ulid(),
            org_id,
            objective,
            context,
            success_criteria,
            state: STATE_PENDING.to_owned(),
            summary: String::new(),
            error: String::new(),
            created_at_unix: now,
            updated_at_unix: now,
        };
        self.inner.insert(worker.worker_id.clone(), worker.clone());
        worker
    }

    /// Mark a worker as completed or failed. Returns the updated
    /// worker, or `Err(NotFound)` when the `worker_id` is unknown or
    /// belongs to a different org.
    fn finish(
        &self,
        worker_id: &str,
        org_id: &str,
        outcome: &str,
        summary: String,
    ) -> Result<TeamWorker, FinishError> {
        let mut entry = self.inner.get_mut(worker_id).ok_or(FinishError::NotFound)?;
        if entry.org_id != org_id {
            // Tenant isolation: treat cross-org access as not-found so
            // we don't leak the existence of foreign workers.
            return Err(FinishError::NotFound);
        }
        match outcome {
            "completed" => {
                STATE_COMPLETED.clone_into(&mut entry.state);
                entry.summary = summary;
                entry.error = String::new();
            }
            "failed" => {
                STATE_FAILED.clone_into(&mut entry.state);
                entry.error = if summary.is_empty() {
                    "worker failed".to_owned()
                } else {
                    summary
                };
                entry.summary = String::new();
            }
            other => return Err(FinishError::BadOutcome(other.to_owned())),
        }
        entry.updated_at_unix = now_unix();
        Ok(entry.clone())
    }

    fn list(&self, org_id: &str, state_filter: &str, limit: usize) -> (Vec<TeamWorker>, i32) {
        let mut matching: Vec<TeamWorker> = self
            .inner
            .iter()
            .filter(|w| w.org_id == org_id)
            .filter(|w| state_filter.is_empty() || w.state == state_filter)
            .map(|w| w.value().clone())
            .collect();
        // Newest-first; deterministic ordering for callers that
        // paginate. updated_at desc, then created_at desc, then id.
        matching.sort_by(|a, b| {
            b.updated_at_unix
                .cmp(&a.updated_at_unix)
                .then(b.created_at_unix.cmp(&a.created_at_unix))
                .then(a.worker_id.cmp(&b.worker_id))
        });
        let total = i32::try_from(matching.len()).unwrap_or(i32::MAX);
        matching.truncate(limit);
        (matching, total)
    }
}

#[derive(Debug)]
enum FinishError {
    NotFound,
    BadOutcome(String),
}

/// Creates a team worker for coordinator-mode sub-task tracking.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.objective` is empty or whitespace.
pub fn handle_team_create(
    store: &TeamWorkerStore,
    req: TeamCreateRequest,
) -> Result<TeamCreateResponse, Status> {
    if req.objective.trim().is_empty() {
        return Err(Status::invalid_argument(
            "objective is required and must be non-empty",
        ));
    }
    let worker = store.create(
        req.org_id.clone(),
        req.objective,
        req.context,
        req.success_criteria,
    );
    Ok(TeamCreateResponse {
        request_id: req.request_id,
        worker: Some(worker),
    })
}

/// Finishes (completes or fails) a team worker.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `worker_id` is empty or the outcome is
/// invalid, or `Status::not_found` if the worker is unknown or owned by another org.
pub fn handle_team_delete(
    store: &TeamWorkerStore,
    req: TeamDeleteRequest,
) -> Result<TeamDeleteResponse, Status> {
    if req.worker_id.is_empty() {
        return Err(Status::invalid_argument("worker_id is required"));
    }
    let outcome = if req.outcome.is_empty() {
        "completed".to_owned()
    } else {
        req.outcome
    };
    let updated = store
        .finish(&req.worker_id, &req.org_id, &outcome, req.summary)
        .map_err(|e| match e {
            FinishError::NotFound => {
                Status::not_found(format!("worker not found: {}", req.worker_id))
            }
            FinishError::BadOutcome(o) => Status::invalid_argument(format!(
                "outcome must be 'completed' or 'failed', got {o}"
            )),
        })?;
    Ok(TeamDeleteResponse {
        request_id: req.request_id,
        worker: Some(updated),
    })
}

/// Lists team workers for an org, optionally filtered by state.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `org_id` is empty or `state_filter`
/// is not one of "", `pending`, `running`, `completed`, `failed`.
pub fn handle_team_list(
    store: &TeamWorkerStore,
    req: TeamListRequest,
) -> Result<TeamListResponse, Status> {
    if req.org_id.is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }
    if !matches!(
        req.state_filter.as_str(),
        "" | "pending" | "running" | "completed" | "failed"
    ) {
        return Err(Status::invalid_argument(format!(
            "invalid state_filter: {}",
            req.state_filter
        )));
    }
    let limit = usize::try_from(if req.limit <= 0 {
        DEFAULT_TEAM_LIST_LIMIT
    } else {
        req.limit.min(MAX_TEAM_LIST_LIMIT)
    })
    .unwrap_or(0);
    let (workers, total) = store.list(&req.org_id, &req.state_filter, limit);
    Ok(TeamListResponse {
        request_id: req.request_id,
        workers,
        total,
    })
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_events::publisher::InMemoryPublisher;

    #[test]
    fn plan_mode_enter_and_check() {
        let s = PlanModeStore::new();
        s.enter("org-1", "run-1", "drafting".to_owned(), 60);
        let (active, rationale, exp) = s.is_plan_mode("org-1", "run-1");
        assert!(active);
        assert_eq!(rationale, "drafting");
        assert!(exp > now_unix());
    }

    #[test]
    fn plan_mode_exit_returns_was_active() {
        let s = PlanModeStore::new();
        assert!(!s.exit("org-1", "missing"));
        s.enter("org-1", "run-1", String::new(), 60);
        assert!(!s.exit("org-other", "run-1"));
        assert!(s.is_plan_mode("org-1", "run-1").0);
        assert!(s.exit("org-1", "run-1"));
        assert!(!s.is_plan_mode("org-1", "run-1").0);
    }

    #[test]
    fn plan_mode_expired_entries_are_swept() {
        let s = PlanModeStore::new();
        // Manually plant an already-expired entry.
        s.inner.insert(
            "run-1".to_owned(),
            PlanModeEntry {
                org_id: "org-1".to_owned(),
                rationale: "old".to_owned(),
                expires_at_unix: now_unix() - 10,
            },
        );
        let (active, _, _) = s.is_plan_mode("org-1", "run-1");
        assert!(!active);
        // Sweep should have removed it.
        assert!(s.inner.get("run-1").is_none());
    }

    #[test]
    fn plan_mode_ttl_capped_at_max() {
        let s = PlanModeStore::new();
        let exp = s.enter("org-1", "run-1", String::new(), 999_999_999);
        // 999M secs → caps to MAX_PLAN_TTL_SECS=86_400.
        assert!(exp <= now_unix() + MAX_PLAN_TTL_SECS + 5);
    }

    /// The grant reports the instant it lapses, for the org and thread that own it and
    /// for nobody else: the plan-approval answer states it, so the client knows how long
    /// the rung lasts without a second read.
    #[test]
    fn a_grant_reports_when_it_expires() {
        use mp_contracts::model_plane::v1::AutonomyRung;
        let s = PlanModeStore::new();
        assert_eq!(s.grant_expires_at("org-1", "thread-1"), None, "no grant yet");
        let before = now_unix();
        s.record_grant("org-1", "thread-1", AutonomyRung::WorkspaceWrite, "");
        let after = now_unix();
        let expires = s
            .grant_expires_at("org-1", "thread-1")
            .expect("the grant just recorded reports its expiry");
        assert!(
            (before + MAX_PLAN_TTL_SECS..=after + MAX_PLAN_TTL_SECS).contains(&expires),
            "the expiry is the grant's hard TTL from now: {expires}"
        );
        assert_eq!(
            s.grant_expires_at("org-2", "thread-1"),
            None,
            "another org never learns a grant exists"
        );
        assert_eq!(
            s.grant_expires_at("org-1", "thread-2"),
            None,
            "another thread has no grant"
        );
        // An expired entry reports nothing (and is swept, exactly like `granted_rung`).
        s.grants.insert(
            "thread-old".to_owned(),
            GrantEntry {
                org_id: "org-1".to_owned(),
                rung: AutonomyRung::WorkspaceWrite,
                expires_at_unix: now_unix() - 1,
                approved_plan_id: None,
            },
        );
        assert_eq!(s.grant_expires_at("org-1", "thread-old"), None);
        assert!(s.grants.get("thread-old").is_none(), "the expired entry is swept");
        // Reading the expiry never spends or alters the grant.
        assert_eq!(
            s.granted_rung("org-1", "thread-1"),
            Some(AutonomyRung::WorkspaceWrite)
        );
        assert_eq!(s.grant_expires_at("org-1", "thread-1"), Some(expires));
    }

    /// The execute switch is the shared strict parser: `true` in any case with
    /// surrounding whitespace, and nothing else. A store built any other way than
    /// from the environment is off.
    #[test]
    fn the_plan_execute_switch_is_strict_and_defaults_off() {
        assert!(plan_execute_from(Some("true")));
        assert!(plan_execute_from(Some("  TRUE ")));
        assert!(plan_execute_from(Some("True")));
        for not_on in [
            None,
            Some(""),
            Some("false"),
            Some("0"),
            Some("1"),
            Some("on"),
            Some("yes"),
            Some("enabled"),
            Some("truee"),
        ] {
            assert!(!plan_execute_from(not_on), "{not_on:?} must stay off");
        }
        assert!(
            !PlanModeStore::default().execute_enabled(),
            "a store nobody configured has execute turns off"
        );
        let on = PlanModeStore::default().with_execute_enabled(true);
        assert!(on.execute_enabled());
        assert!(
            on.clone().execute_enabled(),
            "a clone (the store AppState hands every handler) keeps the switch"
        );
        assert!(!on.with_execute_enabled(false).execute_enabled());
    }

    /// The approval-resume switch the execute switch is coupled to: off for a store nobody
    /// configured, kept by every clone, independent of the execute switch, and read in
    /// production code once, in `new()`, through the one reader `/continue` also uses.
    #[test]
    fn the_approval_resume_switch_defaults_off_and_is_read_once_by_the_store() {
        assert!(!PlanModeStore::default().approval_resume_enabled());
        let on = PlanModeStore::default().with_approval_resume_enabled(true);
        assert!(on.approval_resume_enabled());
        assert!(on.clone().approval_resume_enabled(), "a clone keeps it");
        assert!(!on.execute_enabled(), "the two switches are separate");
        assert!(!on.with_approval_resume_enabled(false).approval_resume_enabled());

        let text = include_str!("coordinator.rs").replace("\r\n", "\n");
        let production = text.split("#[cfg(test)]").next().expect("split yields a first part");
        let code: String = production
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            code.matches("mp_contracts::loop_fold::continue_after_approval_enabled()").count(),
            1,
            "read once, through the shared reader"
        );
        assert!(
            !code.contains("AGENT_CONTINUE_AFTER_APPROVAL_ENABLED")
                && !code.contains("CONTINUE_AFTER_APPROVAL_ENV"),
            "the variable is the shared reader's, not named here"
        );
    }

    /// The execute switch's variable is named and read in this file alone, once, through
    /// the one parser: no handler reads it, so no code path can disagree with another
    /// about whether execute turns are on, and a typo'd spelling is not a second reader.
    #[test]
    fn nothing_outside_the_one_parser_reads_the_execute_switch() {
        // The names are built from parts so this test is not itself a reader of them.
        let variable = ["MODEL_GATEWAY", "PLAN_EXECUTE", "ENABLED"].join("_");
        let constant = ["PLAN_EXECUTE", "ENV"].join("_");
        let code_of = |text: &str| -> String {
            text.lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut scanned = 0;
        for entry in std::fs::read_dir(&src).expect("the crate's src directory is readable") {
            let path = entry.expect("a directory entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            scanned += 1;
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let text = std::fs::read_to_string(&path).expect("a source file is readable");
            if name == "coordinator.rs" {
                // Production code (above the test module): the variable is named once, as the
                // constant, and the constant is read once, by the one function that parses it.
                // Everything above the test MODULE (line-ending independent): a `#[cfg(test)]`
                // helper inside an impl (plant_grant_for_test) stays in scope.
                let production = text
                    .split("mod tests {")
                    .next()
                    .expect("split yields a first part");
                let code = code_of(production);
                assert_eq!(code.matches(&variable).count(), 1, "the variable is named once");
                assert_eq!(
                    code.matches(&format!("std::env::var({constant})")).count(),
                    1,
                    "the constant is read once"
                );
                assert_eq!(
                    code.matches("switch_from(").count(),
                    2,
                    "through the shared strict parser alone: one call per switch (execute, durable grant)"
                );
            } else {
                let code = code_of(&text);
                assert!(
                    !code.contains(&variable) && !code.contains(&constant),
                    "{name} reads or names the execute switch's variable; only coordinator.rs may"
                );
            }
        }
        assert!(scanned > 20, "the scan saw the crate's sources ({scanned} files)");
    }

    #[tokio::test]
    async fn plan_mode_lifecycle_events_use_the_fixed_run_event_subject() {
        let store = PlanModeStore::new();
        let publisher = InMemoryPublisher::new();

        handle_enter_plan_mode(
            &store,
            &publisher,
            EnterPlanModeRequest {
                request_id: "request-enter".to_owned(),
                org_id: "org-1".to_owned(),
                run_id: "run-1".to_owned(),
                session_id: "session-1".to_owned(),
                rationale: "review first".to_owned(),
                ttl_seconds: 60,
            },
        )
        .await
        .expect("enter plan mode");
        let entered = publisher.drain();
        assert_eq!(entered.len(), 1);
        assert_eq!(entered[0].0, "mp.v1.run.run-1.event");
        assert_eq!(entered[0].1.event_type, "RUN_PLAN_MODE_ENTERED");

        handle_exit_plan_mode(
            &store,
            &publisher,
            ExitPlanModeRequest {
                request_id: "request-exit".to_owned(),
                org_id: "org-1".to_owned(),
                run_id: "run-1".to_owned(),
                session_id: "session-1".to_owned(),
                granted_rung: mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite as i32,
                justification: "the approved plan writes its report into the workspace".to_owned(),
            },
            "plan_abc123",
        )
        .await
        .expect("exit plan mode");
        let exited = publisher.drain();
        assert_eq!(exited.len(), 1);
        assert_eq!(exited[0].0, "mp.v1.run.run-1.event");
        assert_eq!(exited[0].1.event_type, "RUN_PLAN_MODE_EXITED");
        assert_eq!(exited[0].1.payload["plan_id"], "plan_abc123");
        // The same decision that emitted the receipt queued the plan for the
        // next run on the thread, so the plan a run is told to follow is the
        // one approved together with its grant.
        assert_eq!(
            store.take_approved_plan("org-1", "session-1"),
            Some("plan_abc123".to_owned())
        );
    }

    #[test]
    fn team_create_assigns_id_and_pending_state() {
        let s = TeamWorkerStore::new();
        let w = s.create(
            "org1".into(),
            "do X".into(),
            "ctx".into(),
            "criteria".into(),
        );
        assert!(!w.worker_id.is_empty());
        assert_eq!(w.state, STATE_PENDING);
        assert_eq!(w.org_id, "org1");
    }

    #[test]
    fn team_finish_completed_sets_summary() {
        let s = TeamWorkerStore::new();
        let w = s.create("org1".into(), "do X".into(), String::new(), String::new());
        let updated = s
            .finish(&w.worker_id, "org1", "completed", "all good".into())
            .unwrap();
        assert_eq!(updated.state, STATE_COMPLETED);
        assert_eq!(updated.summary, "all good");
        assert!(updated.error.is_empty());
    }

    #[test]
    fn team_finish_failed_sets_error() {
        let s = TeamWorkerStore::new();
        let w = s.create("org1".into(), "do X".into(), String::new(), String::new());
        let updated = s
            .finish(&w.worker_id, "org1", "failed", "boom".into())
            .unwrap();
        assert_eq!(updated.state, STATE_FAILED);
        assert_eq!(updated.error, "boom");
    }

    #[test]
    fn team_finish_other_org_returns_not_found() {
        let s = TeamWorkerStore::new();
        let w = s.create("org1".into(), "do X".into(), String::new(), String::new());
        let err = s
            .finish(&w.worker_id, "org2", "completed", String::new())
            .expect_err("cross-org must error");
        matches!(err, FinishError::NotFound);
    }

    #[test]
    fn team_finish_bad_outcome_rejects() {
        let s = TeamWorkerStore::new();
        let w = s.create("org1".into(), "do X".into(), String::new(), String::new());
        let err = s
            .finish(&w.worker_id, "org1", "weird", String::new())
            .expect_err("bad outcome must error");
        matches!(err, FinishError::BadOutcome(_));
    }

    #[test]
    fn team_list_filters_by_org_and_state() {
        let s = TeamWorkerStore::new();
        s.create("org1".into(), "A".into(), String::new(), String::new());
        let w2 = s.create("org1".into(), "B".into(), String::new(), String::new());
        s.create("org2".into(), "C".into(), String::new(), String::new());
        s.finish(&w2.worker_id, "org1", "completed", "ok".into())
            .unwrap();

        let (list, total) = s.list("org1", "", 10);
        assert_eq!(list.len(), 2);
        assert_eq!(total, 2);

        let (only_pending, _) = s.list("org1", "pending", 10);
        assert_eq!(only_pending.len(), 1);
        assert_eq!(only_pending[0].objective, "A");

        // org2 isolated.
        let (org2, _) = s.list("org2", "", 10);
        assert_eq!(org2.len(), 1);
    }

    /// The grant must survive the run it was granted on, because the work it
    /// authorizes happens on the NEXT run of the same thread.
    ///
    /// This test exists because the first version of this feature keyed the
    /// grant on a field the production caller left EMPTY (`session_id`), so
    /// `record_grant` refused every grant and the whole path was a no-op that
    /// no test noticed. Asserting the round-trip — write with the thread the
    /// approval resolved, read with the thread the next run carries — is the
    /// only shape that catches that.
    #[test]
    fn a_grant_is_readable_by_the_next_run_on_the_same_thread() {
        let store = PlanModeStore::new();
        store.record_grant(
            "org-1",
            "thread-1",
            mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite,
            "",
        );
        assert_eq!(
            store.granted_rung("org-1", "thread-1"),
            Some(mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite),
        );
        // Another org must never read it, and an unrelated thread must not
        // inherit it.
        assert_eq!(store.granted_rung("org-2", "thread-1"), None);
        assert_eq!(store.granted_rung("org-1", "thread-2"), None);
    }

    /// plan-gate §2.4: the plan approved WITH a grant is handed to the next run
    /// once, and only to a caller in the grant's own org and thread.
    #[test]
    fn the_plan_approved_with_a_grant_is_handed_over_exactly_once() {
        let store = PlanModeStore::new();
        store.record_grant(
            "org-1",
            "thread-1",
            mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite,
            "plan_7",
        );
        // A foreign org or an unrelated thread neither sees it nor spends it.
        assert_eq!(store.take_approved_plan("org-2", "thread-1"), None);
        assert_eq!(store.take_approved_plan("org-1", "thread-2"), None);
        // The first run to start takes it …
        assert_eq!(
            store.take_approved_plan("org-1", "thread-1"),
            Some("plan_7".to_owned())
        );
        // … the second gets nothing: an approval is spent once.
        assert_eq!(store.take_approved_plan("org-1", "thread-1"), None);
        // The RUNG is not spent with the plan — it covers the thread until it
        // expires.
        assert_eq!(
            store.granted_rung("org-1", "thread-1"),
            Some(mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite),
        );
    }

    #[test]
    fn a_grant_made_without_a_plan_hands_over_no_plan() {
        let store = PlanModeStore::new();
        store.record_grant(
            "org-1",
            "thread-1",
            mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite,
            "",
        );
        assert_eq!(store.take_approved_plan("org-1", "thread-1"), None);
    }

    #[test]
    fn a_newer_grant_replaces_the_pending_plan_of_an_older_one() {
        // Two approvals on one thread before any run starts: only the latest
        // decision stands, so the earlier plan is never handed over.
        let store = PlanModeStore::new();
        let rung = mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite;
        store.record_grant("org-1", "thread-1", rung, "plan_old");
        store.record_grant("org-1", "thread-1", rung, "plan_new");
        assert_eq!(
            store.take_approved_plan("org-1", "thread-1"),
            Some("plan_new".to_owned())
        );
        assert_eq!(store.take_approved_plan("org-1", "thread-1"), None);
    }

    /// An empty thread is refused rather than stored under "" — where every
    /// caller would collide and read each other's grants.
    #[test]
    fn an_empty_thread_or_unspecified_rung_records_nothing() {
        let store = PlanModeStore::new();
        store.record_grant(
            "org-1",
            "",
            mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite,
            "plan_7",
        );
        assert_eq!(store.granted_rung("org-1", ""), None);
        assert_eq!(store.take_approved_plan("org-1", ""), None);
        store.record_grant(
            "org-1",
            "thread-1",
            mp_contracts::model_plane::v1::AutonomyRung::Unspecified,
            "plan_7",
        );
        assert_eq!(store.granted_rung("org-1", "thread-1"), None);
    }

    /// `clear_grant` forgets this org's memory copy of the thread's grant and leaves another org's
    /// entry for the same thread id alone, so the durable approval clears only its own copy.
    #[test]
    fn clear_grant_forgets_only_its_own_orgs_copy() {
        use mp_contracts::model_plane::v1::AutonomyRung;
        let store = PlanModeStore::new();
        store.record_grant("org-1", "thread-1", AutonomyRung::WorkspaceWrite, "plan-7");
        store.clear_grant("org-2", "thread-1");
        assert_eq!(
            store.granted_rung("org-1", "thread-1"),
            Some(AutonomyRung::WorkspaceWrite),
            "another org's clear leaves the copy"
        );
        store.clear_grant("org-1", "thread-1");
        assert_eq!(store.granted_rung("org-1", "thread-1"), None);
        assert_eq!(store.peek_approved_plan("org-1", "thread-1"), None, "its plan goes with it");
        // A thread that holds nothing is a no-op.
        store.clear_grant("org-1", "thread-2");
    }

    /// The durable grant switch (WS04 S4): strict like the execute switch (only `true`, in any
    /// case, around whitespace), off for a store nobody configured, kept by every clone.
    #[test]
    fn the_durable_grant_switch_is_strict_and_defaults_off() {
        assert!(!durable_plan_grant_from(None));
        for value in ["", "false", "0", "1", "yes", "on", "enabled", "tru", "truee", "t"] {
            assert!(!durable_plan_grant_from(Some(value)), "{value:?} must not enable it");
        }
        for value in ["true", "TRUE", "True", " true ", "\ttrue\n"] {
            assert!(durable_plan_grant_from(Some(value)), "{value:?} enables it");
        }
        let store = PlanModeStore::default();
        assert!(!store.durable_grant_enabled());
        assert!(store.durable_grant_client().is_none(), "no reader while the switch is off");
        let on = store.with_durable_grant_enabled(true);
        assert!(on.durable_grant_enabled());
        assert!(on.clone().durable_grant_enabled(), "a clone keeps the switch");
        assert!(!on.execute_enabled(), "separate from the execute switch");
        assert!(!on.with_durable_grant_enabled(false).durable_grant_enabled());
    }

    /// The durable grant switch's variable is named and read in this file alone, once, through
    /// the one parser, like the execute switch's.
    #[test]
    fn nothing_outside_the_one_parser_reads_the_durable_grant_switch() {
        // Built from parts so this test is not itself a reader of the names.
        let variable = ["MODEL_GATEWAY", "DURABLE_PLAN_GRANT"].join("_");
        let constant = ["DURABLE_PLAN_GRANT", "ENV"].join("_");
        let code_of = |text: &str| -> String {
            text.lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut scanned = 0;
        for entry in std::fs::read_dir(&src).expect("the crate's src directory is readable") {
            let path = entry.expect("a directory entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            scanned += 1;
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let text = std::fs::read_to_string(&path).expect("a source file is readable");
            if name == "coordinator.rs" {
                let production = text
                    .split("mod tests {")
                    .next()
                    .expect("split yields a first part");
                let code = code_of(production);
                assert_eq!(code.matches(&variable).count(), 1, "the variable is named once");
                assert_eq!(
                    code.matches(&format!("std::env::var({constant})")).count(),
                    1,
                    "the constant is read once"
                );
            } else {
                let code = code_of(&text);
                assert!(
                    !code.contains(&variable) && !code.contains(&constant),
                    "{name} reads or names the durable grant switch's variable; only coordinator.rs may"
                );
            }
        }
        assert!(scanned > 20, "the scan saw the crate's sources ({scanned} files)");
    }

    /// A durable exit records NO grant in this replica's memory (Session Core records it) and
    /// follows the caller's durable answer to "was the run in plan mode", not this replica's
    /// flag: the run may have entered plan mode on another replica.
    #[tokio::test]
    async fn a_durable_exit_records_no_grant_here_and_follows_the_durable_plan_mode_answer() {
        let rung = mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite as i32;
        let request = || ExitPlanModeRequest {
            request_id: "req-1".to_owned(),
            org_id: "org-1".to_owned(),
            run_id: "run-1".to_owned(),
            session_id: "thread-1".to_owned(),
            granted_rung: rung,
            justification: "the plan only writes a report".to_owned(),
        };
        // This replica never saw the run enter plan mode.
        let store = PlanModeStore::new();
        let publisher = InMemoryPublisher::new();
        let exited = handle_exit_plan_mode_durable(&store, &publisher, request(), "plan-1", true)
            .await
            .expect("a justified escalation from READ_ONLY is valid");
        assert!(exited.was_active, "the durable run row said it was in plan mode");
        assert_eq!(store.granted_rung("org-1", "thread-1"), None, "no copy of the grant here");
        assert_eq!(store.take_approved_plan("org-1", "thread-1"), None);
        let events = publisher.drain();
        assert_eq!(events.len(), 1, "the lifecycle event is published on any replica");
        assert_eq!(events[0].1.event_type, "RUN_PLAN_MODE_EXITED");

        // The durable row says the run was not in plan mode and this replica holds no flag
        // either (already approved once): no event.
        let publisher = InMemoryPublisher::new();
        let exited = handle_exit_plan_mode_durable(&store, &publisher, request(), "", false)
            .await
            .expect("valid");
        assert!(!exited.was_active);
        assert!(publisher.drain().is_empty());

        // WS04 S4 review defect 3a: the durable row says `execute` because the run's best-effort
        // durable SetRunMode(plan) failed, but THIS replica holds the plan-mode flag. The run was
        // in plan mode: the exit event, the only one carrying the rung, is published, and the
        // local flag is cleared.
        let publisher = InMemoryPublisher::new();
        store.enter("org-1", "run-1", "entered here, durable write failed".to_owned(), 600);
        let exited = handle_exit_plan_mode_durable(&store, &publisher, request(), "plan-1", false)
            .await
            .expect("valid");
        assert!(exited.was_active, "a local flag alone is enough: the durable write was best-effort");
        let events = publisher.drain();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].1.event_type, "RUN_PLAN_MODE_EXITED");
        assert_eq!(events[0].1.payload["plan_id"], "plan-1");
        assert_eq!(events[0].1.payload["granted_rung"], "workspace_write");
        assert!(!store.is_plan_mode("org-1", "run-1").0, "the local flag is cleared");

        // The refusals are the in-memory path's: a malformed grant is refused before anything.
        let mut unjustified = request();
        unjustified.justification = String::new();
        assert!(handle_exit_plan_mode_durable(&store, &publisher, unjustified, "", true)
            .await
            .is_err());
    }

    /// WS04 S4 second review, defect 0: only a transient failure of the durable write lets the
    /// approving replica keep the grant in memory; a refusal never does.
    #[test]
    fn only_a_transient_set_run_mode_failure_keeps_the_grant_in_memory() {
        for code in [
            tonic::Code::Unavailable,
            tonic::Code::DeadlineExceeded,
            tonic::Code::Aborted,
            tonic::Code::ResourceExhausted,
            tonic::Code::Internal,
        ] {
            assert!(set_run_mode_failure_is_transient(code), "{code:?}");
        }
        for code in [
            tonic::Code::FailedPrecondition,
            tonic::Code::PermissionDenied,
            tonic::Code::Unauthenticated,
            tonic::Code::InvalidArgument,
            tonic::Code::NotFound,
            tonic::Code::Unimplemented,
            tonic::Code::Unknown,
            tonic::Code::Cancelled,
            tonic::Code::AlreadyExists,
            tonic::Code::OutOfRange,
            tonic::Code::DataLoss,
        ] {
            assert!(!set_run_mode_failure_is_transient(code), "{code:?}");
        }
    }

    /// Exiting plan mode IS the grant, so the handler must record it — with the
    /// thread the caller resolved, not with the run id.
    #[tokio::test]
    async fn exiting_plan_mode_records_the_grant_for_the_thread() {
        let store = PlanModeStore::new();
        let publisher = InMemoryPublisher::new();
        store.enter("org-1", "run-1", "investigating".to_owned(), 600);
        let resp = handle_exit_plan_mode(
            &store,
            &publisher,
            ExitPlanModeRequest {
                request_id: "req-1".to_owned(),
                org_id: "org-1".to_owned(),
                run_id: "run-1".to_owned(),
                session_id: "thread-1".to_owned(),
                granted_rung: mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite as i32,
                justification: "the plan only writes a report".to_owned(),
            },
            "",
        )
        .await
        .expect("a justified escalation from READ_ONLY is valid");
        assert!(resp.was_active);
        assert_eq!(
            store.granted_rung("org-1", "thread-1"),
            Some(mp_contracts::model_plane::v1::AutonomyRung::WorkspaceWrite),
            "the next run on this thread must see the rung the human granted"
        );
        // An empty plan_id (a caller with no plan-gate plan, e.g. the
        // pre-existing gRPC ExitPlanMode path) is OMITTED, not empty-stringed
        // — a reader doing payload.get("plan_id") sees an honest absence
        // rather than a value that looks like a real, empty plan id.
        let exited = publisher.drain();
        assert_eq!(exited.len(), 1);
        assert!(
            exited[0]
                .1
                .payload
                .get("plan_id")
                .map_or(true, |v| v.is_null()),
            "empty plan_id must not appear as a truthy/present field: {:?}",
            exited[0].1.payload
        );
    }
}
