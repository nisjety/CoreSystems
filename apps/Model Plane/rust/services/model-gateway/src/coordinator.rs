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
use tonic::Status;

use mp_contracts::model_plane::v1::{
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
}

impl PlanModeStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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

/// Exits plan mode for a run and emits a best-effort lifecycle event.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.run_id` is empty. Event-publish
/// failures are logged and non-fatal, not surfaced as an `Err`.
pub async fn handle_exit_plan_mode<P: EventPublisher>(
    store: &PlanModeStore,
    publisher: &P,
    req: ExitPlanModeRequest,
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

    let was_active = store.exit(&req.org_id, &req.run_id);
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
        )
        .await
        .expect("exit plan mode");
        let exited = publisher.drain();
        assert_eq!(exited.len(), 1);
        assert_eq!(exited[0].0, "mp.v1.run.run-1.event");
        assert_eq!(exited[0].1.event_type, "RUN_PLAN_MODE_EXITED");
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
}
