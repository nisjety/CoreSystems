//! Wave 10e — approval gate for risky tool actions.
//!
//! In-memory `ApprovalStore` (DashMap-backed). Each approval moves
//! through `pending → approved | denied | expired`. The store is the
//! source of truth; resolution events are best-effort published to
//! NATS so external UIs / pollers can react.
//!
//! For durable persistence promote this to postgres later — the
//! orchestrator already has an `Approval` proto + storage (see
//! `orchestration.proto`). This gateway-scoped store is intentionally
//! lighter weight: ephemeral, run-loop-blocking, no plan-step linkage.

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
    orchestration_core_service_client::OrchestrationCoreServiceClient, ApprovalKind, ApprovalState,
    ApproveApprovalRequest, ApproveApprovalResponse, CreateApprovalRequest, DecideApprovalRequest,
    DenyApprovalRequest, DenyApprovalResponse, GatewayApproval, ListPendingApprovalsRequest,
    ListPendingApprovalsResponse, RequestApprovalRequest, RequestApprovalResponse,
    ResumeRunRequest,
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

/// Best-effort durable persist of a newly-requested approval. Logged on
/// failure, never blocks the caller.
pub async fn persist_approval_request(
    client: &mut OrchestrationCoreServiceClient<Channel>,
    approval: &GatewayApproval,
) {
    if let Err(e) = client
        .create_approval(to_create_approval_request(approval))
        .await
    {
        warn!(error = %e, approval_id = %approval.approval_id, "durable approval persist failed (best-effort)");
    }
}

/// Best-effort durable persist of an approval decision.
pub async fn persist_approval_decision(
    client: &mut OrchestrationCoreServiceClient<Channel>,
    approval: &GatewayApproval,
) {
    if let Err(e) = client
        .decide_approval(to_decide_approval_request(approval))
        .await
    {
        warn!(error = %e, approval_id = %approval.approval_id, "durable approval decision persist failed (best-effort)");
    }
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
            warn!(error = %e, approval_id = %approval.approval_id, run_id = %approval.run_id, "resume_run after approval failed (best-effort)")
        }
    }
}

const STATUS_PENDING: &str = "pending";
const STATUS_APPROVED: &str = "approved";
const STATUS_DENIED: &str = "denied";

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
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.org_id` is empty.
pub fn handle_list_pending_approvals(
    store: &ApprovalStore,
    req: ListPendingApprovalsRequest,
) -> Result<ListPendingApprovalsResponse, Status> {
    if req.org_id.is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }
    let approvals = store.list_pending(&req.org_id, &req.run_id);
    Ok(ListPendingApprovalsResponse {
        request_id: req.request_id,
        approvals,
    })
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
}
