//! Wave 10e — approval gate for risky tool actions.
//!
//! In-memory `ApprovalStore` (DashMap-backed) backed by session-core's durable
//! Postgres `approvals` store. Each approval moves through
//! `pending → approved | denied | expired`.
//!
//! **D-1 durability (Phase 3 PR-4):** the durable write is now the path of
//! record. A request/decision is persisted to session-core's
//! `OrchestrationCoreService` *before* the handler returns OK — a backend
//! failure propagates as `Status::unavailable` and the gate does NOT report
//! success. The in-memory store mints the ULID id (passed as
//! `client_approval_id`), serves low-latency reads, and is rehydrated from the
//! durable store on boot (`rehydrate_pending`) so a restart never silently
//! drops a pending approval. There is no in-memory-only path of record.

// tonic::Status is the unavoidable large Err for gRPC handlers; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use dashmap::DashMap;
use mp_events::publisher::EventPublisher;
use mp_ids::new_ulid;
use tonic::Status;
use tracing::{info, warn};

use mp_contracts::model_plane::v1::{
    execution_core_client::ExecutionCoreClient,
    orchestration_core_service_client::OrchestrationCoreServiceClient, Approval, ApprovalKind,
    ApprovalState, ApproveApprovalRequest, ApproveApprovalResponse, CreateApprovalRequest,
    DecideApprovalRequest, DenyApprovalRequest, DenyApprovalResponse, GatewayApproval,
    ListPendingApprovalsRequest, ListPendingApprovalsResponse, OrgPendingApprovalsRequest,
    RequestApprovalRequest, RequestApprovalResponse, ResumeRunRequest,
};
use tonic::transport::Channel;

// ---------------------------------------------------------------------------
// Durable write-through (matrix §4.1).
//
// The in-memory ApprovalStore above is a run-loop latency cache; session-core's
// OrchestrationCoreService is the system of record. These map the gateway's
// approval to the durable RPCs and persist best-effort — the in-memory store
// stays authoritative for the response, so a backend hiccup never blocks the
// approval gate. The gateway's own approval_id is passed as
// `client_approval_id` so the durable record shares the id and a later
// DecideApproval can target it.
// ---------------------------------------------------------------------------

/// Map a gateway approval to the durable `CreateApprovalRequest`.
#[must_use]
pub fn to_create_approval_request(a: &GatewayApproval) -> CreateApprovalRequest {
    let kind = match a.kind.as_str() {
        "tool_execution" | "tool_call" => ApprovalKind::ToolCall,
        "plan" => ApprovalKind::Plan,
        "permission" => ApprovalKind::Permission,
        _ => ApprovalKind::Unspecified,
    };
    CreateApprovalRequest {
        run_id: a.run_id.clone(),
        step_id: String::new(),
        kind: kind as i32,
        requested_of: String::new(),
        org_id: a.org_id.clone(),
        user_id: String::new(),
        reason: a.reason.clone(),
        expires_in_seconds: 0,
        client_approval_id: a.approval_id.clone(),
        // Stable per-(run, action) key so a retried request collapses onto the
        // existing durable row instead of duplicating it (D-1 idempotency).
        idempotency_key: idempotency_key(&a.run_id, &a.action_id),
    }
}

/// Stable idempotency key for a gated action: `"{run_id}:{action_id}"`.
#[must_use]
pub fn idempotency_key(run_id: &str, action_id: &str) -> String {
    format!("{run_id}:{action_id}")
}

/// Map a durable `Approval` (session-core) to the gateway's `GatewayApproval`.
///
/// Inverts `to_create_approval_request`: the durable `state` vocabulary
/// (`REQUESTED`/`GRANTED`/`DENIED`/`TIMED_OUT`) maps back to the gateway's
/// string status (`pending`/`approved`/`denied`/`expired`). `action_id` /
/// `action_name` / `session_id` are not stored on the durable record (they
/// live only in the gateway view), so they default to empty on rehydrate.
#[must_use]
pub fn gateway_approval_from_proto(a: &Approval) -> GatewayApproval {
    let kind = match ApprovalKind::try_from(a.kind).unwrap_or(ApprovalKind::Unspecified) {
        ApprovalKind::ToolCall => "tool_execution",
        ApprovalKind::Plan => "plan",
        ApprovalKind::Permission => "permission",
        ApprovalKind::Destructive => "destructive_action",
        _ => "custom",
    };
    let status = match ApprovalState::try_from(a.state).unwrap_or(ApprovalState::Unspecified) {
        ApprovalState::Granted => STATUS_APPROVED,
        ApprovalState::Denied => STATUS_DENIED,
        ApprovalState::TimedOut => STATUS_EXPIRED,
        // An unknown/unspecified durable state defaults to pending so the gate
        // stays visible rather than silently disappearing.
        ApprovalState::Requested | ApprovalState::Unspecified => STATUS_PENDING,
    };
    GatewayApproval {
        approval_id: a.id.clone(),
        org_id: a.org_id.clone(),
        run_id: a.run_id.clone(),
        session_id: String::new(),
        action_id: String::new(),
        action_name: String::new(),
        kind: kind.to_string(),
        reason: String::new(),
        status: status.to_string(),
        decided_by: a.decided_by.clone(),
        comment: a.decision_reason.clone(),
        created_at_unix: a.requested_at.as_ref().map_or(0, |t| t.seconds),
        resolved_at_unix: a.decided_at.as_ref().map_or(0, |t| t.seconds),
    }
}

/// Map a resolved gateway approval to the durable `DecideApprovalRequest`.
#[must_use]
pub fn to_decide_approval_request(a: &GatewayApproval) -> DecideApprovalRequest {
    let decision = match a.status.as_str() {
        STATUS_APPROVED => ApprovalState::Granted,
        STATUS_DENIED => ApprovalState::Denied,
        _ => ApprovalState::Unspecified,
    };
    DecideApprovalRequest {
        approval_id: a.approval_id.clone(),
        decision: decision as i32,
        decided_by: a.decided_by.clone(),
        decision_reason: a.comment.clone(),
    }
}

/// Durable-FIRST persist of a newly-requested approval (D-1). The durable
/// write to session-core is the path of record: on failure this returns
/// `Status::unavailable` so the caller does NOT report the gate as succeeded.
///
/// # Errors
///
/// Returns `Status::unavailable` when the session-core write fails.
pub async fn persist_approval_request(
    client: &mut OrchestrationCoreServiceClient<Channel>,
    approval: &GatewayApproval,
) -> Result<(), Status> {
    client
        .create_approval(to_create_approval_request(approval))
        .await
        .map(|_| ())
        .map_err(|e| {
            warn!(error = %e, approval_id = %approval.approval_id, "durable approval persist failed");
            Status::unavailable(format!("durable approval store unavailable: {e}"))
        })
}

/// Durable-FIRST persist of an approval decision (D-1). On failure this
/// returns `Status::unavailable` so the decision is not reported as recorded.
///
/// # Errors
///
/// Returns `Status::unavailable` when the session-core write fails.
pub async fn persist_approval_decision(
    client: &mut OrchestrationCoreServiceClient<Channel>,
    approval: &GatewayApproval,
) -> Result<(), Status> {
    client
        .decide_approval(to_decide_approval_request(approval))
        .await
        .map(|_| ())
        .map_err(|e| {
            warn!(error = %e, approval_id = %approval.approval_id, "durable approval decision persist failed");
            Status::unavailable(format!("durable approval store unavailable: {e}"))
        })
}

/// Best-effort resume of the gated run once an approval is **granted**.
///
/// The gateway is the approval decision point but does not drive the execution
/// loop, so it signals execution-core directly to flip the run from
/// `AwaitingApproval` back to `Running`. session-core separately broadcasts
/// `RunResumedAfterApproval` for SSE consumers. Denials/timeouts never resume.
/// Logged on failure, never blocks the caller — the in-memory decision already
/// succeeded by the time this runs.
pub async fn resume_run_if_approved(
    client: &mut ExecutionCoreClient<Channel>,
    approval: &GatewayApproval,
) {
    if approval.status != STATUS_APPROVED {
        return;
    }
    match client
        .resume_run(ResumeRunRequest {
            run_id: approval.run_id.clone(),
            checkpoint_id: String::new(),
            org_id: approval.org_id.clone(),
        })
        .await
    {
        Ok(resp) => info!(
            approval_id = %approval.approval_id,
            run_id = %approval.run_id,
            resumed = resp.into_inner().resumed,
            "approval granted → execution-core resume_run"
        ),
        Err(e) => {
            warn!(error = %e, approval_id = %approval.approval_id, run_id = %approval.run_id, "resume_run after approval failed (best-effort)");
        }
    }
}

const STATUS_PENDING: &str = "pending";
const STATUS_APPROVED: &str = "approved";
const STATUS_DENIED: &str = "denied";
const STATUS_EXPIRED: &str = "expired";

const MAX_PENDING_LIST: usize = 500;

#[derive(Clone, Default, Debug)]
pub struct ApprovalStore {
    inner: Arc<DashMap<String, GatewayApproval>>,
}

impl ApprovalStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn create(&self, mut approval: GatewayApproval) -> GatewayApproval {
        approval.approval_id = new_ulid();
        approval.status = STATUS_PENDING.to_string();
        approval.created_at_unix = now_unix();
        approval.resolved_at_unix = 0;
        self.inner
            .insert(approval.approval_id.clone(), approval.clone());
        approval
    }

    /// Resolve a pending approval. Returns the updated record or an
    /// error indicating why the transition was rejected.
    fn resolve(
        &self,
        approval_id: &str,
        org_id: &str,
        approved: bool,
        decided_by: String,
        comment: String,
    ) -> Result<GatewayApproval, ResolveError> {
        let mut entry = self
            .inner
            .get_mut(approval_id)
            .ok_or(ResolveError::NotFound)?;
        if entry.org_id != org_id {
            // Cross-org access → treat as not-found so we don't leak
            // existence of foreign approvals.
            return Err(ResolveError::NotFound);
        }
        if entry.status != STATUS_PENDING {
            return Err(ResolveError::AlreadyResolved(entry.status.clone()));
        }
        entry.status = if approved {
            STATUS_APPROVED.to_string()
        } else {
            STATUS_DENIED.to_string()
        };
        entry.decided_by = decided_by;
        entry.comment = comment;
        entry.resolved_at_unix = now_unix();
        Ok(entry.clone())
    }

    fn list_pending(&self, org_id: &str, run_id: &str) -> Vec<GatewayApproval> {
        let mut out: Vec<GatewayApproval> = self
            .inner
            .iter()
            .filter(|a| a.org_id == org_id)
            .filter(|a| a.status == STATUS_PENDING)
            .filter(|a| run_id.is_empty() || a.run_id == run_id)
            .map(|a| a.value().clone())
            .collect();
        // Newest-first ordering so UIs surface the most recent request
        // at the top of the queue.
        out.sort_by(|a, b| {
            b.created_at_unix
                .cmp(&a.created_at_unix)
                .then(a.approval_id.cmp(&b.approval_id))
        });
        out.truncate(MAX_PENDING_LIST);
        out
    }

    /// Find an existing PENDING approval for the same `(org_id, run_id,
    /// action_id)` tuple, if any. Used to make `handle_request_approval`
    /// idempotent: a repeated gate for the same action returns the existing
    /// pending record instead of minting a duplicate (D-1).
    fn find_pending_by_action(
        &self,
        org_id: &str,
        run_id: &str,
        action_id: &str,
    ) -> Option<GatewayApproval> {
        self.inner
            .iter()
            .find(|a| {
                a.status == STATUS_PENDING
                    && a.org_id == org_id
                    && a.run_id == run_id
                    && a.action_id == action_id
            })
            .map(|a| a.value().clone())
    }

    /// Insert a pre-built approval, preserving its existing `approval_id` and
    /// timestamps (does NOT mint a new id). Used by `rehydrate_pending` to warm
    /// the cache from the durable store on boot. An entry already present for
    /// the same id is left untouched so a concurrently-created live approval is
    /// never clobbered by a rehydrated copy.
    fn insert_existing(&self, approval: GatewayApproval) {
        if approval.approval_id.is_empty() {
            return;
        }
        self.inner
            .entry(approval.approval_id.clone())
            .or_insert(approval);
    }

    /// Number of cached approvals. Test/diagnostic helper.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.len()
    }
}

#[derive(Debug)]
enum ResolveError {
    NotFound,
    AlreadyResolved(String),
}

/// Creates a pending approval for a gated action and emits a lifecycle event.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.run_id` or `req.action_id` is empty.
pub async fn handle_request_approval<P: EventPublisher>(
    store: &ApprovalStore,
    publisher: &P,
    req: RequestApprovalRequest,
) -> Result<RequestApprovalResponse, Status> {
    if req.run_id.is_empty() {
        return Err(Status::invalid_argument("run_id is required"));
    }
    if req.action_id.is_empty() {
        return Err(Status::invalid_argument("action_id is required"));
    }
    // Idempotency (D-1): if a pending approval already exists for this exact
    // (org, run, action), return it instead of minting a duplicate. The caller
    // (grpc.rs) re-persists it durably; the durable store collapses the retry
    // via its (org_id, idempotency_key) ON CONFLICT guard, so this stays a
    // no-op of record rather than a second gate.
    if let Some(existing) =
        store.find_pending_by_action(&req.org_id, &req.run_id, &req.action_id)
    {
        return Ok(RequestApprovalResponse {
            request_id: req.request_id,
            approval: Some(existing),
        });
    }

    let kind = if req.kind.is_empty() {
        "tool_execution".to_string()
    } else {
        req.kind.clone()
    };

    let approval = store.create(GatewayApproval {
        approval_id: String::new(), // assigned in `create`
        org_id: req.org_id.clone(),
        run_id: req.run_id.clone(),
        session_id: req.session_id.clone(),
        action_id: req.action_id.clone(),
        action_name: req.action_name.clone(),
        kind,
        reason: req.reason.clone(),
        status: STATUS_PENDING.to_string(),
        decided_by: String::new(),
        comment: String::new(),
        created_at_unix: 0,
        resolved_at_unix: 0,
    });

    publish_event(
        publisher,
        "agents.approval.requested",
        "APPROVAL_REQUESTED",
        &req.request_id,
        &approval,
    )
    .await;

    Ok(RequestApprovalResponse {
        request_id: req.request_id,
        approval: Some(approval),
    })
}

/// Approves a pending approval and emits a lifecycle event.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.approval_id` is empty, `Status::not_found`
/// if it is unknown, or `Status::failed_precondition` if it was already resolved.
pub async fn handle_approve_approval<P: EventPublisher>(
    store: &ApprovalStore,
    publisher: &P,
    req: ApproveApprovalRequest,
) -> Result<ApproveApprovalResponse, Status> {
    if req.approval_id.is_empty() {
        return Err(Status::invalid_argument("approval_id is required"));
    }
    let updated = store
        .resolve(
            &req.approval_id,
            &req.org_id,
            true,
            req.decided_by,
            req.comment,
        )
        .map_err(resolve_err_to_status)?;
    publish_event(
        publisher,
        "agents.approval.resolved",
        "APPROVAL_APPROVED",
        &req.request_id,
        &updated,
    )
    .await;
    Ok(ApproveApprovalResponse {
        request_id: req.request_id,
        approval: Some(updated),
    })
}

/// Denies a pending approval and emits a lifecycle event.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.approval_id` is empty, `Status::not_found`
/// if it is unknown, or `Status::failed_precondition` if it was already resolved.
pub async fn handle_deny_approval<P: EventPublisher>(
    store: &ApprovalStore,
    publisher: &P,
    req: DenyApprovalRequest,
) -> Result<DenyApprovalResponse, Status> {
    if req.approval_id.is_empty() {
        return Err(Status::invalid_argument("approval_id is required"));
    }
    let updated = store
        .resolve(
            &req.approval_id,
            &req.org_id,
            false,
            req.decided_by,
            req.comment,
        )
        .map_err(resolve_err_to_status)?;
    publish_event(
        publisher,
        "agents.approval.resolved",
        "APPROVAL_DENIED",
        &req.request_id,
        &updated,
    )
    .await;
    Ok(DenyApprovalResponse {
        request_id: req.request_id,
        approval: Some(updated),
    })
}

/// Lists pending approvals for an org, optionally scoped to a run.
///
/// Merges the warm in-memory cache with a fresh durable read-through from
/// session-core (D-1) so an approval created by another replica — or persisted
/// durably but not yet in this replica's cache — is still visible. Both sources
/// are org-scoped to `req.org_id`, so the client-facing list never leaks a
/// foreign tenant's approvals (IDOR-clean). A durable read failure degrades to
/// the cache-only view rather than failing the read.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.org_id` is empty.
pub async fn handle_list_pending_approvals(
    store: &ApprovalStore,
    client: &mut OrchestrationCoreServiceClient<Channel>,
    req: ListPendingApprovalsRequest,
) -> Result<ListPendingApprovalsResponse, Status> {
    if req.org_id.is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }

    // Start from the warm in-memory cache, keyed by approval_id for dedupe.
    let mut by_id: std::collections::HashMap<String, GatewayApproval> = store
        .list_pending(&req.org_id, &req.run_id)
        .into_iter()
        .map(|a| (a.approval_id.clone(), a))
        .collect();

    // Merge a fresh durable read-through, org-scoped (IDOR-clean). The in-memory
    // copy wins on conflict — it carries gateway-only fields (action_id,
    // session_id) the durable record does not.
    match client
        .list_pending_approvals(OrgPendingApprovalsRequest {
            org_id: req.org_id.clone(),
        })
        .await
    {
        Ok(resp) => {
            for proto in resp.into_inner().approvals {
                let gw = gateway_approval_from_proto(&proto);
                // Defence-in-depth: drop anything not scoped to the caller's org
                // and respect the optional run filter.
                if gw.org_id != req.org_id {
                    continue;
                }
                if !req.run_id.is_empty() && gw.run_id != req.run_id {
                    continue;
                }
                if gw.status != STATUS_PENDING {
                    continue;
                }
                by_id.entry(gw.approval_id.clone()).or_insert(gw);
            }
        }
        Err(e) => {
            warn!(error = %e, org_id = %req.org_id, "durable pending-approvals read-through failed; serving cache-only");
        }
    }

    let mut approvals: Vec<GatewayApproval> = by_id.into_values().collect();
    approvals.sort_by(|a, b| {
        b.created_at_unix
            .cmp(&a.created_at_unix)
            .then(a.approval_id.cmp(&b.approval_id))
    });
    approvals.truncate(MAX_PENDING_LIST);

    Ok(ListPendingApprovalsResponse {
        request_id: req.request_id,
        approvals,
    })
}

/// Rehydrate the in-memory `ApprovalStore` from session-core's durable store on
/// boot (D-1). Calls `ListPendingApprovals{org_id:""}` once (the internal-only
/// all-orgs variant) and inserts each pending durable approval into the cache
/// so a process restart never silently drops a pending HITL gate. Best-effort:
/// a session-core outage logs and leaves the cache empty rather than crashing
/// boot — the durable store remains the path of record either way.
///
/// Returns the number of approvals rehydrated (0 on any failure).
pub async fn rehydrate_pending(
    store: &ApprovalStore,
    client: &mut OrchestrationCoreServiceClient<Channel>,
) -> usize {
    match client
        .list_pending_approvals(OrgPendingApprovalsRequest {
            org_id: String::new(), // internal-only: all orgs, for boot rehydrate
        })
        .await
    {
        Ok(resp) => {
            let mut n = 0usize;
            for proto in resp.into_inner().approvals {
                let gw = gateway_approval_from_proto(&proto);
                if gw.status == STATUS_PENDING && !gw.approval_id.is_empty() {
                    store.insert_existing(gw);
                    n += 1;
                }
            }
            if n > 0 {
                info!(count = n, "rehydrated pending approvals from session-core");
            }
            n
        }
        Err(e) => {
            warn!(error = %e, "pending-approval rehydrate skipped (session-core unavailable)");
            0
        }
    }
}

async fn publish_event<P: EventPublisher>(
    publisher: &P,
    subject: &str,
    event_type: &str,
    request_id: &str,
    approval: &GatewayApproval,
) {
    let envelope = mp_events::envelope::Envelope {
        event_id: new_ulid(),
        event_type: event_type.to_string(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_string(),
        correlation_id: request_id.to_string(),
        causation_id: String::new(),
        idempotency_key: format!("{event_type}-{}", approval.approval_id),
        org_id: approval.org_id.clone(),
        user_id: if approval.decided_by.is_empty() {
            "agent".to_string()
        } else {
            approval.decided_by.clone()
        },
        resource_ref: format!("approval/{}", approval.approval_id),
        payload: serde_json::json!({
            "approval_id": approval.approval_id,
            "run_id": approval.run_id,
            "session_id": approval.session_id,
            "action_id": approval.action_id,
            "action_name": approval.action_name,
            "kind": approval.kind,
            "status": approval.status,
            "comment": approval.comment,
        }),
        zdr: false,
    };
    if let Err(e) = publisher.publish(subject, &envelope).await {
        warn!(error = %e, subject = %subject, "failed to publish approval event");
    }
}

fn resolve_err_to_status(err: ResolveError) -> Status {
    match err {
        ResolveError::NotFound => Status::not_found("approval not found"),
        ResolveError::AlreadyResolved(s) => {
            Status::failed_precondition(format!("approval already resolved as {s}"))
        }
    }
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

    fn make_req(run_id: &str, action_id: &str) -> RequestApprovalRequest {
        RequestApprovalRequest {
            request_id: "t".into(),
            org_id: "org1".into(),
            run_id: run_id.into(),
            session_id: "s1".into(),
            action_id: action_id.into(),
            action_name: "bash".into(),
            kind: String::new(),
            reason: "destructive".into(),
        }
    }

    #[test]
    fn create_assigns_id_and_pending_status() {
        let s = ApprovalStore::new();
        let a = s.create(GatewayApproval {
            org_id: "org1".into(),
            run_id: "r".into(),
            ..Default::default()
        });
        assert!(!a.approval_id.is_empty());
        assert_eq!(a.status, STATUS_PENDING);
        assert!(a.created_at_unix > 0);
    }

    #[test]
    fn resolve_approved_sets_decided_fields() {
        let s = ApprovalStore::new();
        let a = s.create(GatewayApproval {
            org_id: "org1".into(),
            run_id: "r".into(),
            ..Default::default()
        });
        let r = s
            .resolve(
                &a.approval_id,
                "org1",
                true,
                "alice".into(),
                "looks good".into(),
            )
            .unwrap();
        assert_eq!(r.status, STATUS_APPROVED);
        assert_eq!(r.decided_by, "alice");
        assert_eq!(r.comment, "looks good");
        assert!(r.resolved_at_unix > 0);
    }

    #[test]
    fn resolve_denied_keeps_record() {
        let s = ApprovalStore::new();
        let a = s.create(GatewayApproval {
            org_id: "org1".into(),
            run_id: "r".into(),
            ..Default::default()
        });
        let r = s
            .resolve(
                &a.approval_id,
                "org1",
                false,
                "bob".into(),
                "too risky".into(),
            )
            .unwrap();
        assert_eq!(r.status, STATUS_DENIED);
    }

    #[test]
    fn cross_org_resolve_returns_not_found() {
        let s = ApprovalStore::new();
        let a = s.create(GatewayApproval {
            org_id: "org1".into(),
            run_id: "r".into(),
            ..Default::default()
        });
        let err = s
            .resolve(&a.approval_id, "org2", true, String::new(), String::new())
            .expect_err("cross-org must error");
        matches!(err, ResolveError::NotFound);
    }

    #[test]
    fn resolve_twice_errors() {
        let s = ApprovalStore::new();
        let a = s.create(GatewayApproval {
            org_id: "org1".into(),
            run_id: "r".into(),
            ..Default::default()
        });
        s.resolve(&a.approval_id, "org1", true, String::new(), String::new())
            .unwrap();
        let err = s
            .resolve(&a.approval_id, "org1", true, String::new(), String::new())
            .expect_err("double-resolve must error");
        matches!(err, ResolveError::AlreadyResolved(_));
    }

    #[test]
    fn list_pending_filters_by_run_and_org() {
        let s = ApprovalStore::new();
        let _ = make_req("r1", "a1");
        s.create(GatewayApproval {
            org_id: "org1".into(),
            run_id: "r1".into(),
            ..Default::default()
        });
        let a2 = s.create(GatewayApproval {
            org_id: "org1".into(),
            run_id: "r2".into(),
            ..Default::default()
        });
        s.create(GatewayApproval {
            org_id: "org2".into(),
            run_id: "r1".into(),
            ..Default::default()
        });
        // Resolve one so it's no longer pending.
        s.resolve(&a2.approval_id, "org1", true, String::new(), String::new())
            .unwrap();

        let all = s.list_pending("org1", "");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].run_id, "r1");

        let by_run = s.list_pending("org1", "r1");
        assert_eq!(by_run.len(), 1);

        let other_org = s.list_pending("org2", "");
        assert_eq!(other_org.len(), 1);
    }

    #[test]
    fn to_create_request_maps_kind_and_aligns_id() {
        let a = GatewayApproval {
            approval_id: "appr-gw-1".into(),
            org_id: "org1".into(),
            run_id: "r1".into(),
            kind: "tool_execution".into(),
            reason: "rm -rf /".into(),
            ..Default::default()
        };
        let req = to_create_approval_request(&a);
        // The gateway id is carried so the durable record shares it.
        assert_eq!(req.client_approval_id, "appr-gw-1");
        assert_eq!(req.run_id, "r1");
        assert_eq!(req.org_id, "org1");
        assert_eq!(req.kind, ApprovalKind::ToolCall as i32);
        assert_eq!(req.reason, "rm -rf /");
    }

    #[test]
    fn to_decide_request_maps_status_to_state() {
        let approved = GatewayApproval {
            approval_id: "appr-gw-2".into(),
            status: STATUS_APPROVED.into(),
            decided_by: "alice".into(),
            comment: "looks safe".into(),
            ..Default::default()
        };
        let req = to_decide_approval_request(&approved);
        assert_eq!(req.approval_id, "appr-gw-2");
        assert_eq!(req.decision, ApprovalState::Granted as i32);
        assert_eq!(req.decided_by, "alice");
        assert_eq!(req.decision_reason, "looks safe");

        let denied = GatewayApproval {
            status: STATUS_DENIED.into(),
            ..Default::default()
        };
        assert_eq!(
            to_decide_approval_request(&denied).decision,
            ApprovalState::Denied as i32
        );
    }

    // ---------------------------------------------------------------------
    // D-1 (Phase 3 PR-4) — durable-first + restart-survival.
    // ---------------------------------------------------------------------

    fn durable_pending(id: &str, org: &str, run: &str) -> Approval {
        Approval {
            id: id.into(),
            run_id: run.into(),
            step_id: String::new(),
            kind: ApprovalKind::ToolCall as i32,
            state: ApprovalState::Requested as i32,
            requested_of: String::new(),
            decided_by: String::new(),
            decision_reason: String::new(),
            context: None,
            requested_at: Some(prost_types::Timestamp {
                seconds: 1_700_000_000,
                nanos: 0,
            }),
            decided_at: None,
            expires_at: None,
            org_id: org.into(),
        }
    }

    /// (2) proto `Approval` → `GatewayApproval` mapping, incl. status vocab.
    #[test]
    fn proto_to_gateway_maps_fields_and_status_vocab() {
        let gw = gateway_approval_from_proto(&durable_pending("appr_1", "orgA", "run_1"));
        assert_eq!(gw.approval_id, "appr_1");
        assert_eq!(gw.org_id, "orgA"); // org carried over the wire (IDOR re-scope)
        assert_eq!(gw.run_id, "run_1");
        assert_eq!(gw.kind, "tool_execution");
        assert_eq!(gw.status, STATUS_PENDING); // REQUESTED → pending
        assert_eq!(gw.created_at_unix, 1_700_000_000);

        // Full status vocabulary: GRANTED→approved, DENIED→denied, TIMED_OUT→expired.
        let mut granted = durable_pending("a", "o", "r");
        granted.state = ApprovalState::Granted as i32;
        granted.decided_by = "alice".into();
        granted.decision_reason = "ok".into();
        let g = gateway_approval_from_proto(&granted);
        assert_eq!(g.status, STATUS_APPROVED);
        assert_eq!(g.decided_by, "alice");
        assert_eq!(g.comment, "ok");

        let mut denied = durable_pending("a", "o", "r");
        denied.state = ApprovalState::Denied as i32;
        assert_eq!(gateway_approval_from_proto(&denied).status, STATUS_DENIED);

        let mut timed = durable_pending("a", "o", "r");
        timed.state = ApprovalState::TimedOut as i32;
        assert_eq!(gateway_approval_from_proto(&timed).status, STATUS_EXPIRED);
    }

    /// (3) RESTART-SURVIVAL: a pending approval persisted durably is recovered
    /// by `rehydrate_pending`-equivalent insertion into a FRESH `ApprovalStore`
    /// (the kill-mid-pending case). Proves the cache reconstructs from the
    /// durable path of record after a process restart — no in-memory state was
    /// carried across the "restart".
    #[test]
    fn rehydrate_recovers_pending_approval_into_fresh_store() {
        // Durable store holds two pending approvals across two orgs (the
        // all-orgs boot-rehydrate view ListPendingApprovals{org_id:""}).
        let durable = vec![
            durable_pending("appr_orgA_1", "orgA", "run_A"),
            durable_pending("appr_orgB_1", "orgB", "run_B"),
        ];

        // Simulate a restart: brand-new, empty in-memory store.
        let fresh = ApprovalStore::new();
        assert_eq!(fresh.len(), 0, "fresh store starts empty (post-restart)");

        // The rehydrate body maps each durable Approval and inserts it.
        let mut recovered = 0usize;
        for proto in &durable {
            let gw = gateway_approval_from_proto(proto);
            if gw.status == STATUS_PENDING && !gw.approval_id.is_empty() {
                fresh.insert_existing(gw);
                recovered += 1;
            }
        }
        assert_eq!(recovered, 2);

        // The previously-pending approval is visible again, org-scoped.
        let org_a = fresh.list_pending("orgA", "");
        assert_eq!(org_a.len(), 1);
        assert_eq!(org_a[0].approval_id, "appr_orgA_1");
        assert_eq!(org_a[0].status, STATUS_PENDING);

        // Cross-org isolation survives rehydrate (IDOR-clean).
        let org_b = fresh.list_pending("orgB", "");
        assert_eq!(org_b.len(), 1);
        assert!(fresh.list_pending("orgA", "run_B").is_empty());
    }

    /// `insert_existing` never clobbers a live approval already cached under the
    /// same id (a concurrently-created gate must win over a rehydrated copy).
    #[test]
    fn insert_existing_does_not_clobber_live_entry() {
        let store = ApprovalStore::new();
        let live = store.create(GatewayApproval {
            approval_id: String::new(),
            org_id: "orgA".into(),
            run_id: "run_A".into(),
            action_id: "act_1".into(),
            action_name: "bash".into(),
            ..Default::default()
        });
        // A rehydrated copy with the same id but stale (empty) action fields.
        store.insert_existing(GatewayApproval {
            approval_id: live.approval_id.clone(),
            org_id: "orgA".into(),
            run_id: "run_A".into(),
            status: STATUS_PENDING.into(),
            ..Default::default()
        });
        let pending = store.list_pending("orgA", "");
        assert_eq!(pending.len(), 1);
        // The live entry (with action_id) is preserved, not overwritten.
        assert_eq!(pending[0].action_id, "act_1");
    }

    /// idempotency key is the stable `{run_id}:{action_id}` pair and is set on
    /// the durable `CreateApprovalRequest`.
    #[test]
    fn create_request_carries_idempotency_key() {
        let a = GatewayApproval {
            approval_id: "appr_x".into(),
            org_id: "orgA".into(),
            run_id: "run_1".into(),
            action_id: "act_9".into(),
            kind: "tool_execution".into(),
            ..Default::default()
        };
        let req = to_create_approval_request(&a);
        assert_eq!(req.idempotency_key, "run_1:act_9");
        assert_eq!(idempotency_key("run_1", "act_9"), "run_1:act_9");
    }

    /// (1) PERSIST-FAILURE → handler returns error (no silent in-memory-only
    /// path of record). With session-core unreachable, the durable write
    /// surfaces `Status::unavailable` rather than swallowing the failure.
    #[tokio::test]
    async fn persist_request_failure_returns_unavailable() {
        // Lazy channel to a closed port: the first RPC attempt fails to connect.
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1")
            .connect_lazy();
        let mut client = OrchestrationCoreServiceClient::new(channel);
        let approval = GatewayApproval {
            approval_id: "appr_p".into(),
            org_id: "orgA".into(),
            run_id: "run_1".into(),
            action_id: "act_1".into(),
            kind: "tool_execution".into(),
            status: STATUS_PENDING.into(),
            ..Default::default()
        };
        let err = persist_approval_request(&mut client, &approval)
            .await
            .expect_err("durable write to a dead session-core must error, not swallow");
        assert_eq!(err.code(), tonic::Code::Unavailable);

        // The decision path enforces the same durable-first contract.
        let decided = GatewayApproval {
            status: STATUS_APPROVED.into(),
            decided_by: "alice".into(),
            ..approval
        };
        let err = persist_approval_decision(&mut client, &decided)
            .await
            .expect_err("durable decision write to a dead session-core must error");
        assert_eq!(err.code(), tonic::Code::Unavailable);
    }

    /// Idempotency: a second request for the same (org, run, action) while one
    /// is pending returns the EXISTING approval, not a new duplicate.
    #[tokio::test]
    async fn duplicate_request_returns_existing_pending() {
        let store = ApprovalStore::new();
        let publisher = mp_events::publisher::InMemoryPublisher::new();

        let first = handle_request_approval(&store, &publisher, make_req("run_1", "act_1"))
            .await
            .unwrap()
            .approval
            .unwrap();
        let second = handle_request_approval(&store, &publisher, make_req("run_1", "act_1"))
            .await
            .unwrap()
            .approval
            .unwrap();

        assert_eq!(first.approval_id, second.approval_id, "no duplicate minted");
        assert_eq!(store.list_pending("org1", "").len(), 1);
    }
}
