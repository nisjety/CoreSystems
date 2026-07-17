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
use tonic::{Request, Status};
use tracing::{info, warn};

use mp_contracts::model_plane::v1::{
    orchestration_core_service_client::OrchestrationCoreServiceClient, Approval, ApprovalKind,
    ApprovalState, ApproveApprovalRequest, ApproveApprovalResponse, CreateApprovalRequest,
    DecideApprovalRequest, DenyApprovalRequest, DenyApprovalResponse, GatewayApproval,
    GetApprovalRequest, ListPendingApprovalsRequest, ListPendingApprovalsResponse,
    OrgPendingApprovalsRequest, RequestApprovalRequest, RequestApprovalResponse,
};
use tonic::transport::Channel;

// ---------------------------------------------------------------------------
// Durable write-through (matrix §4.1).
//
// The in-memory ApprovalStore above is a run-loop latency cache; session-core's
// OrchestrationCoreService is the system of record. These map the gateway's
// approval to the durable RPCs before committing the local cache. A backend
// hiccup therefore blocks the approval gate rather than creating an
// in-memory-only authority record. The gateway's own approval_id is passed as
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
        // Cross-org IDOR fix (Phase 6): this gateway-side `GatewayApproval`
        // was itself org-scoped by `ApprovalStore::resolve` before this
        // function is ever reached, so asserting it here is a real,
        // already-verified ownership check, not a no-op.
        org_id: a.org_id.clone(),
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

/// Authenticated durable approval write. `bearer` must already have been
/// independently verified and bound to the gateway caller as
/// `aud=session-core`; session-core verifies it again.
///
/// # Errors
/// Returns an authentication, transport, or durable-store status when the
/// request cannot be recorded.
pub async fn persist_approval_request_authenticated(
    client: &mut OrchestrationCoreServiceClient<Channel>,
    approval: &GatewayApproval,
    bearer: &str,
) -> Result<Approval, Status> {
    let response = client
        .create_approval(authenticated_session_request(
            to_create_approval_request(approval),
            bearer,
        )?)
        .await
        .map_err(|error| {
            warn!(%error, approval_id = %approval.approval_id, "authenticated durable approval persist failed");
            Status::unavailable("durable approval store unavailable")
        })?
        .into_inner();
    let durable = response
        .approval
        .ok_or_else(|| Status::data_loss("durable approval store returned no approval"))?;
    if durable.id.trim().is_empty() {
        return Err(Status::data_loss(
            "durable approval store returned an empty approval id",
        ));
    }
    Ok(durable)
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

/// Authenticated durable approval decision using a separately verified
/// session-core audience credential.
///
/// # Errors
/// Returns an authentication, transport, or durable-store status when the
/// decision cannot be recorded.
pub async fn persist_approval_decision_authenticated(
    client: &mut OrchestrationCoreServiceClient<Channel>,
    approval: &GatewayApproval,
    bearer: &str,
) -> Result<(), Status> {
    client
        .decide_approval(authenticated_session_request(
            to_decide_approval_request(approval),
            bearer,
        )?)
        .await
        .map(|_| ())
        .map_err(|error| {
            warn!(%error, approval_id = %approval.approval_id, "authenticated durable approval decision failed");
            Status::unavailable("durable approval store unavailable")
        })
}

fn authenticated_session_request<T>(value: T, bearer: &str) -> Result<Request<T>, Status> {
    let mut request = Request::new(value);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {bearer}")
            .parse()
            .map_err(|_| Status::internal("verified session credential is not forwardable"))?,
    );
    Ok(request)
}

/// A durable approval grant is authority to *consider* the exact paused work,
/// not authority for Gateway to invoke generic `ResumeRun`. The current outbox
/// intentionally contains identifiers only; it cannot reconstruct the exited
/// agent loop or direct-step request, and Execution Core has no service-only
/// continuation receipt endpoint. Keep this boundary explicit until a
/// descriptor-backed dispatcher exists.
///
/// # Errors
/// Returns `unavailable` for a granted approval, after the durable decision
/// has been accepted, so callers cannot report a state-only resume as work.
#[allow(clippy::result_large_err)]
pub fn quarantine_granted_approval_continuation(approval: &GatewayApproval) -> Result<(), Status> {
    if approval.status != STATUS_APPROVED {
        return Ok(());
    }
    warn!(
        approval_id = %approval.approval_id,
        run_id = %approval.run_id,
        "approval grant accepted; durable continuation remains quarantined"
    );
    Err(Status::unavailable(
        "approval grant recorded; durable continuation delivery is not available",
    ))
}

const STATUS_PENDING: &str = "pending";
const STATUS_APPROVED: &str = "approved";
const STATUS_DENIED: &str = "denied";
const STATUS_EXPIRED: &str = "expired";

const MAX_PENDING_LIST: usize = 500;
const MAX_CACHED_APPROVALS: usize = 2_000;

#[derive(Clone, Default, Debug)]
pub struct ApprovalStore {
    inner: Arc<DashMap<String, GatewayApproval>>,
    owners: Arc<DashMap<String, String>>,
}

impl ApprovalStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    fn create(&self, mut approval: GatewayApproval) -> GatewayApproval {
        approval.approval_id = new_ulid();
        approval.status = STATUS_PENDING.to_string();
        approval.created_at_unix = now_unix();
        approval.resolved_at_unix = 0;
        self.inner
            .insert(approval.approval_id.clone(), approval.clone());
        approval
    }

    #[cfg(test)]
    fn create_for_owner(&self, approval: GatewayApproval, owner_user_id: &str) -> GatewayApproval {
        let approval = self.create(approval);
        self.owners
            .insert(approval.approval_id.clone(), owner_user_id.to_owned());
        self.enforce_cache_bound();
        approval
    }

    fn insert_bounded_for_owner(&self, approval: GatewayApproval, owner_user_id: &str) -> bool {
        if approval.approval_id.is_empty() || owner_user_id.trim().is_empty() {
            return false;
        }
        let approval_id = approval.approval_id.clone();
        match self.inner.entry(approval_id.clone()) {
            dashmap::mapref::entry::Entry::Occupied(_) => {
                // A legacy/restart-rehydrated entry has no trustworthy owner.
                // Only a caller that has just received an authenticated,
                // user-filtered session-core acknowledgement reaches this
                // method, so it may bind that previously ownerless cache row.
                self.owners
                    .entry(approval_id)
                    .or_insert_with(|| owner_user_id.to_owned());
                return false;
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                self.owners.insert(approval_id, owner_user_id.to_owned());
                entry.insert(approval);
            }
        }
        self.enforce_cache_bound();
        true
    }

    fn enforce_cache_bound(&self) {
        while self.inner.len() > MAX_CACHED_APPROVALS {
            let oldest = self
                .inner
                .iter()
                .min_by(|left, right| {
                    left.created_at_unix
                        .cmp(&right.created_at_unix)
                        .then(left.approval_id.cmp(&right.approval_id))
                })
                .map(|entry| entry.approval_id.clone());
            let Some(approval_id) = oldest else {
                break;
            };
            self.inner.remove(&approval_id);
            self.owners.remove(&approval_id);
        }
    }

    fn owner_matches(&self, approval_id: &str, owner_user_id: &str) -> bool {
        !owner_user_id.trim().is_empty()
            && self
                .owners
                .get(approval_id)
                .is_some_and(|owner| owner.as_str() == owner_user_id)
    }

    /// Resolve a pending approval. Returns the updated record or an
    /// error indicating why the transition was rejected.
    pub(crate) fn preview_resolution(
        &self,
        approval_id: &str,
        org_id: &str,
        approved: bool,
        decided_by: String,
        comment: String,
    ) -> Result<GatewayApproval, ResolveError> {
        self.preview_resolution_with_transition(approval_id, org_id, approved, decided_by, comment)
            .map(|outcome| outcome.approval)
    }

    fn preview_resolution_with_transition(
        &self,
        approval_id: &str,
        org_id: &str,
        approved: bool,
        decided_by: String,
        comment: String,
    ) -> Result<ApprovalResolution, ResolveError> {
        let current = self.inner.get(approval_id).ok_or(ResolveError::NotFound)?;
        if current.org_id != org_id {
            return Err(ResolveError::NotFound);
        }
        let target_status = if approved {
            STATUS_APPROVED
        } else {
            STATUS_DENIED
        };
        if current.status != STATUS_PENDING {
            if !decided_by.is_empty()
                && current.status == target_status
                && current.decided_by == decided_by
                && current.comment == comment
            {
                return Ok(ApprovalResolution {
                    approval: current.clone(),
                    transitioned: false,
                });
            }
            return Err(ResolveError::AlreadyResolved(current.status.clone()));
        }
        let mut updated = current.clone();
        target_status.clone_into(&mut updated.status);
        updated.decided_by = decided_by;
        updated.comment = comment;
        updated.resolved_at_unix = now_unix();
        Ok(ApprovalResolution {
            approval: updated,
            transitioned: true,
        })
    }

    pub(crate) fn preview_resolution_for_owner(
        &self,
        approval_id: &str,
        org_id: &str,
        approved: bool,
        decided_by: String,
        comment: String,
    ) -> Result<GatewayApproval, ResolveError> {
        if !self.owner_matches(approval_id, &decided_by) {
            return Err(ResolveError::NotFound);
        }
        self.preview_resolution(approval_id, org_id, approved, decided_by, comment)
    }

    pub(crate) fn preview_resolution_for_owner_with_transition(
        &self,
        approval_id: &str,
        org_id: &str,
        approved: bool,
        decided_by: String,
        comment: String,
    ) -> Result<ApprovalResolution, ResolveError> {
        if !self.owner_matches(approval_id, &decided_by) {
            return Err(ResolveError::NotFound);
        }
        self.preview_resolution_with_transition(approval_id, org_id, approved, decided_by, comment)
    }

    fn resolve(
        &self,
        approval_id: &str,
        org_id: &str,
        approved: bool,
        decided_by: String,
        comment: String,
    ) -> Result<GatewayApproval, ResolveError> {
        self.resolve_with_transition(approval_id, org_id, approved, decided_by, comment)
            .map(|outcome| outcome.approval)
    }

    fn resolve_with_transition(
        &self,
        approval_id: &str,
        org_id: &str,
        approved: bool,
        decided_by: String,
        comment: String,
    ) -> Result<ApprovalResolution, ResolveError> {
        let updated = self.preview_resolution_with_transition(
            approval_id,
            org_id,
            approved,
            decided_by,
            comment,
        )?;
        let mut entry = self
            .inner
            .get_mut(approval_id)
            .ok_or(ResolveError::NotFound)?;
        if entry.org_id != org_id {
            return Err(ResolveError::NotFound);
        }
        if entry.status != STATUS_PENDING {
            if !updated.approval.decided_by.is_empty()
                && entry.status == updated.approval.status
                && entry.decided_by == updated.approval.decided_by
                && entry.comment == updated.approval.comment
            {
                return Ok(ApprovalResolution {
                    approval: entry.clone(),
                    transitioned: false,
                });
            }
            return Err(ResolveError::AlreadyResolved(entry.status.clone()));
        }
        *entry = updated.approval.clone();
        Ok(ApprovalResolution {
            approval: updated.approval,
            transitioned: true,
        })
    }

    fn resolve_for_owner(
        &self,
        approval_id: &str,
        org_id: &str,
        approved: bool,
        decided_by: String,
        comment: String,
    ) -> Result<GatewayApproval, ResolveError> {
        if !self.owner_matches(approval_id, &decided_by) {
            return Err(ResolveError::NotFound);
        }
        self.resolve(approval_id, org_id, approved, decided_by, comment)
    }

    fn resolve_for_owner_with_transition(
        &self,
        approval_id: &str,
        org_id: &str,
        approved: bool,
        decided_by: String,
        comment: String,
    ) -> Result<ApprovalResolution, ResolveError> {
        if !self.owner_matches(approval_id, &decided_by) {
            return Err(ResolveError::NotFound);
        }
        self.resolve_with_transition(approval_id, org_id, approved, decided_by, comment)
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

    fn list_pending_for_owner(
        &self,
        org_id: &str,
        owner_user_id: &str,
        run_id: &str,
    ) -> Vec<GatewayApproval> {
        self.list_pending(org_id, run_id)
            .into_iter()
            .filter(|approval| self.owner_matches(&approval.approval_id, owner_user_id))
            .collect()
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

    fn find_pending_by_action_for_owner(
        &self,
        org_id: &str,
        owner_user_id: &str,
        run_id: &str,
        action_id: &str,
    ) -> Option<GatewayApproval> {
        self.find_pending_by_action(org_id, run_id, action_id)
            .filter(|approval| self.owner_matches(&approval.approval_id, owner_user_id))
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
        self.enforce_cache_bound();
    }

    fn insert_existing_for_owner(&self, approval: GatewayApproval, owner_user_id: &str) {
        let _ = self.insert_bounded_for_owner(approval, owner_user_id);
    }

    /// Number of cached approvals. Test/diagnostic helper.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.len()
    }
}

#[derive(Debug)]
pub(crate) enum ResolveError {
    NotFound,
    AlreadyResolved(String),
}

#[derive(Debug)]
pub(crate) struct ApprovalResolution {
    pub(crate) approval: GatewayApproval,
    pub(crate) transitioned: bool,
}

/// Require proof that this request created the one local transition whose
/// execution delivery is about to be attempted. An already-granted cache row
/// has no durable delivery receipt, so treating it as success would hide a
/// prior resume failure. It must remain explicitly retryable/unavailable until
/// a durable outbox or delivery identifier exists.
pub(crate) fn require_fresh_approval_delivery(transitioned: bool) -> Result<(), Status> {
    if transitioned {
        return Ok(());
    }
    Err(Status::unavailable(
        "approval is granted but execution delivery is unknown",
    ))
}

#[derive(Debug)]
pub(crate) struct PreparedApproval {
    request_id: String,
    approval: GatewayApproval,
}

impl PreparedApproval {
    pub(crate) fn approval(&self) -> &GatewayApproval {
        &self.approval
    }

    pub(crate) fn align_with_durable(mut self, durable: &Approval) -> Result<Self, Status> {
        if durable.id.trim().is_empty() || durable.run_id != self.approval.run_id {
            return Err(Status::data_loss(
                "durable approval identity did not match the prepared request",
            ));
        }
        self.approval.approval_id.clone_from(&durable.id);
        Ok(self)
    }
}

/// Build a pending approval without mutating the cache or publishing an event.
/// The caller must first persist it through session-core, which independently
/// validates run ownership, and only then call `commit_persisted_approval`.
pub(crate) fn prepare_request_approval(
    store: &ApprovalStore,
    req: RequestApprovalRequest,
    owner_user_id: &str,
) -> Result<PreparedApproval, Status> {
    if req.run_id.is_empty() {
        return Err(Status::invalid_argument("run_id is required"));
    }
    if req.action_id.is_empty() {
        return Err(Status::invalid_argument("action_id is required"));
    }
    if owner_user_id.trim().is_empty() {
        return Err(Status::permission_denied(
            "a verified user must own an approval request",
        ));
    }
    // Idempotency (D-1): if a pending approval already exists for this exact
    // (org, user, run, action), return it instead of minting a duplicate. The caller
    // (grpc.rs) re-persists it durably; the durable store collapses the retry
    // via its (org_id, user_id, idempotency_key) ON CONFLICT guard, so this stays a
    // no-op of record rather than a second gate.
    if let Some(existing) = store.find_pending_by_action_for_owner(
        &req.org_id,
        owner_user_id,
        &req.run_id,
        &req.action_id,
    ) {
        return Ok(PreparedApproval {
            request_id: req.request_id,
            approval: existing,
        });
    }

    let kind = if req.kind.is_empty() {
        "tool_execution".to_string()
    } else {
        req.kind.clone()
    };

    let approval = GatewayApproval {
        approval_id: new_ulid(),
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
        created_at_unix: now_unix(),
        resolved_at_unix: 0,
    };

    Ok(PreparedApproval {
        request_id: req.request_id,
        approval,
    })
}

/// Commit a session-core-validated approval into the bounded, owner-scoped
/// cache and publish its lifecycle event. Duplicate durable retries converge
/// on the same approval id and therefore do not publish twice.
pub(crate) async fn commit_persisted_approval<P: EventPublisher>(
    store: &ApprovalStore,
    publisher: &P,
    prepared: PreparedApproval,
    owner_user_id: &str,
) -> Result<RequestApprovalResponse, Status> {
    if !store.insert_bounded_for_owner(prepared.approval.clone(), owner_user_id) {
        return Ok(RequestApprovalResponse {
            request_id: prepared.request_id,
            approval: Some(prepared.approval),
        });
    }
    publish_event(
        publisher,
        "agents.approval.requested",
        "APPROVAL_REQUESTED",
        &prepared.request_id,
        &prepared.approval,
    )
    .await;

    Ok(RequestApprovalResponse {
        request_id: prepared.request_id,
        approval: Some(prepared.approval),
    })
}

/// Result of a local approval resolution. `transitioned` is true exactly once
/// and is the only signal that permits the caller to resume execution.
pub(crate) struct ApprovalDecisionOutcome {
    pub(crate) response: ApproveApprovalResponse,
    pub(crate) transitioned: bool,
}

/// Approves a pending approval and emits a lifecycle event exactly once.
///
/// # Errors
///
/// Returns `Status::invalid_argument` if `req.approval_id` is empty, `Status::not_found`
/// if it is unknown, or `Status::failed_precondition` if it was already resolved.
pub(crate) async fn handle_approve_approval<P: EventPublisher>(
    store: &ApprovalStore,
    publisher: &P,
    req: ApproveApprovalRequest,
) -> Result<ApprovalDecisionOutcome, Status> {
    if req.approval_id.is_empty() {
        return Err(Status::invalid_argument("approval_id is required"));
    }
    let resolution = store
        .resolve_for_owner_with_transition(
            &req.approval_id,
            &req.org_id,
            true,
            req.decided_by,
            req.comment,
        )
        .map_err(resolve_err_to_status)?;
    if resolution.transitioned {
        publish_event(
            publisher,
            "agents.approval.resolved",
            "APPROVAL_APPROVED",
            &req.request_id,
            &resolution.approval,
        )
        .await;
    }
    Ok(ApprovalDecisionOutcome {
        response: ApproveApprovalResponse {
            request_id: req.request_id,
            approval: Some(resolution.approval),
        },
        transitioned: resolution.transitioned,
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
        .resolve_for_owner(
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

fn cache_durable_approval_for_owner(
    store: &ApprovalStore,
    durable: &Approval,
    expected_approval_id: &str,
    expected_org_id: &str,
    owner_user_id: &str,
) -> Result<GatewayApproval, Status> {
    if owner_user_id.trim().is_empty() {
        return Err(Status::permission_denied(
            "a verified user must own an approval decision",
        ));
    }
    if durable.id != expected_approval_id || durable.org_id != expected_org_id {
        return Err(Status::data_loss(
            "durable approval response did not match the authenticated scope",
        ));
    }

    let approval = gateway_approval_from_proto(durable);
    store.insert_existing_for_owner(approval.clone(), owner_user_id);
    if !store.owner_matches(&approval.approval_id, owner_user_id) {
        return Err(Status::not_found("approval not found"));
    }
    Ok(approval)
}

/// Hydrate one approval into a fresh replica's owner-bound cache through an
/// authenticated, tenant-scoped Session Core read. Unlike the pending-list
/// cache fallback, decision paths fail explicitly when durable state is
/// unavailable because they must not turn stale local state into authority.
pub(crate) async fn read_through_approval_for_owner_authenticated(
    store: &ApprovalStore,
    client: &mut OrchestrationCoreServiceClient<Channel>,
    approval_id: &str,
    org_id: &str,
    bearer: &str,
    owner_user_id: &str,
) -> Result<GatewayApproval, Status> {
    if approval_id.trim().is_empty() {
        return Err(Status::invalid_argument("approval_id is required"));
    }
    if org_id.trim().is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }
    if bearer.trim().is_empty() {
        return Err(Status::unauthenticated(
            "verified session credential is required",
        ));
    }
    if owner_user_id.trim().is_empty() {
        return Err(Status::permission_denied(
            "a verified user must own an approval decision",
        ));
    }

    let response = client
        .get_approval(authenticated_session_request(
            GetApprovalRequest {
                approval_id: approval_id.to_owned(),
                org_id: org_id.to_owned(),
            },
            bearer,
        )?)
        .await
        .map_err(|error| {
            warn!(%error, approval_id, org_id, "durable approval read-through failed");
            error
        })?
        .into_inner();
    let durable = response
        .approval
        .ok_or_else(|| Status::not_found("approval not found"))?;
    cache_durable_approval_for_owner(store, &durable, approval_id, org_id, owner_user_id)
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
    handle_list_pending_approvals_inner(store, client, req, None).await
}

/// Authenticated durable read-through using a boundary-verified
/// `aud=session-core` credential.
///
/// # Errors
/// Returns an authentication, validation, or durable-store status when the
/// scoped read cannot be completed.
pub async fn handle_list_pending_approvals_authenticated(
    store: &ApprovalStore,
    client: &mut OrchestrationCoreServiceClient<Channel>,
    req: ListPendingApprovalsRequest,
    bearer: &str,
    owner_user_id: &str,
) -> Result<ListPendingApprovalsResponse, Status> {
    if owner_user_id.trim().is_empty() {
        return Err(Status::permission_denied(
            "a verified user must list pending approvals",
        ));
    }
    handle_list_pending_approvals_inner(store, client, req, Some((bearer, owner_user_id))).await
}

async fn handle_list_pending_approvals_inner(
    store: &ApprovalStore,
    client: &mut OrchestrationCoreServiceClient<Channel>,
    req: ListPendingApprovalsRequest,
    authenticated: Option<(&str, &str)>,
) -> Result<ListPendingApprovalsResponse, Status> {
    if req.org_id.is_empty() {
        return Err(Status::invalid_argument("org_id is required"));
    }

    // Start from the warm in-memory cache, keyed by approval_id for dedupe.
    let cached = authenticated.map_or_else(
        || store.list_pending(&req.org_id, &req.run_id),
        |(_, owner_user_id)| store.list_pending_for_owner(&req.org_id, owner_user_id, &req.run_id),
    );
    let mut by_id: std::collections::HashMap<String, GatewayApproval> = cached
        .into_iter()
        .map(|a| (a.approval_id.clone(), a))
        .collect();

    // Merge a fresh durable read-through, org-scoped (IDOR-clean). The in-memory
    // copy wins on conflict — it carries gateway-only fields (action_id,
    // session_id) the durable record does not.
    let durable_request = OrgPendingApprovalsRequest {
        org_id: req.org_id.clone(),
    };
    let durable = if let Some((bearer, _)) = authenticated {
        client
            .list_pending_approvals(authenticated_session_request(durable_request, bearer)?)
            .await
    } else {
        client.list_pending_approvals(durable_request).await
    };
    match durable {
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
                if let Some((_, owner_user_id)) = authenticated {
                    store.insert_existing_for_owner(gw.clone(), owner_user_id);
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

/// Rehydrate one organization's in-memory approval view from session-core.
/// This helper deliberately has no all-tenant mode; callers must supply a
/// validated tenant and session-core must independently pin it to identity.
///
/// Returns the number of approvals rehydrated (0 on any failure).
pub async fn rehydrate_pending_for_org(
    store: &ApprovalStore,
    client: &mut OrchestrationCoreServiceClient<Channel>,
    org_id: &str,
) -> usize {
    let org_id = org_id.trim();
    if org_id.is_empty() {
        warn!("pending-approval rehydrate rejected without tenant scope");
        return 0;
    }
    match client
        .list_pending_approvals(OrgPendingApprovalsRequest {
            org_id: org_id.to_owned(),
        })
        .await
    {
        Ok(resp) => {
            let mut n = 0usize;
            for proto in resp.into_inner().approvals {
                let gw = gateway_approval_from_proto(&proto);
                if gw.org_id == org_id && gw.status == STATUS_PENDING && !gw.approval_id.is_empty()
                {
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

pub(crate) fn resolve_err_to_status(err: ResolveError) -> Status {
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
    fn identical_approval_replay_is_not_a_new_resume_transition() {
        let store = ApprovalStore::new();
        let pending = store.create_for_owner(
            GatewayApproval {
                org_id: "org1".into(),
                run_id: "run1".into(),
                ..Default::default()
            },
            "alice",
        );

        let first = store
            .resolve_for_owner_with_transition(
                &pending.approval_id,
                "org1",
                true,
                "alice".into(),
                "approved once".into(),
            )
            .expect("first approval decision");
        assert!(
            first.transitioned,
            "the first decision must resume exactly once"
        );

        let replay = store
            .resolve_for_owner_with_transition(
                &pending.approval_id,
                "org1",
                true,
                "alice".into(),
                "approved once".into(),
            )
            .expect("identical retry is idempotent");
        assert!(
            !replay.transitioned,
            "an identical retry must not resume again"
        );
        assert_eq!(replay.approval.status, STATUS_APPROVED);
    }

    #[tokio::test]
    async fn identical_approval_replay_does_not_republish_resolution() {
        let store = ApprovalStore::new();
        let pending = store.create_for_owner(
            GatewayApproval {
                org_id: "org1".into(),
                run_id: "run1".into(),
                ..Default::default()
            },
            "alice",
        );
        let publisher = mp_events::publisher::InMemoryPublisher::new();
        let request = || ApproveApprovalRequest {
            request_id: "request1".into(),
            approval_id: pending.approval_id.clone(),
            org_id: "org1".into(),
            decided_by: "alice".into(),
            comment: "approved once".into(),
        };

        let first = handle_approve_approval(&store, &publisher, request())
            .await
            .expect("first approval decision");
        assert!(first.transitioned);
        assert_eq!(publisher.drain().len(), 1);

        let replay = handle_approve_approval(&store, &publisher, request())
            .await
            .expect("identical retry");
        assert!(!replay.transitioned);
        assert!(publisher.drain().is_empty());
    }

    #[test]
    fn granted_retry_after_unknown_resume_delivery_fails_closed() {
        let store = ApprovalStore::new();
        let pending = store.create_for_owner(
            GatewayApproval {
                org_id: "org1".into(),
                run_id: "run1".into(),
                ..Default::default()
            },
            "alice",
        );
        store
            .resolve_for_owner_with_transition(
                &pending.approval_id,
                "org1",
                true,
                "alice".into(),
                "approved once".into(),
            )
            .expect("durable decision succeeded before resume delivery failed");

        let retry = store
            .preview_resolution_for_owner_with_transition(
                &pending.approval_id,
                "org1",
                true,
                "alice".into(),
                "approved once".into(),
            )
            .expect("identical retry is recognized");
        let error = require_fresh_approval_delivery(retry.transitioned)
            .expect_err("unknown execution delivery must not return success");
        assert_eq!(error.code(), tonic::Code::Unavailable);
    }

    #[test]
    fn preview_resolution_does_not_mutate_the_pending_store() {
        let store = ApprovalStore::new();
        let pending = store.create(GatewayApproval {
            org_id: "org1".into(),
            run_id: "run1".into(),
            ..Default::default()
        });

        let preview = store
            .preview_resolution(
                &pending.approval_id,
                "org1",
                true,
                "user1".into(),
                "approved".into(),
            )
            .expect("preview pending decision");
        assert_eq!(preview.status, STATUS_APPROVED);
        assert_eq!(store.list_pending("org1", "run1").len(), 1);
    }

    #[test]
    fn exact_authenticated_decision_retry_is_idempotent() {
        let store = ApprovalStore::new();
        let pending = store.create(GatewayApproval {
            org_id: "org1".into(),
            run_id: "run1".into(),
            ..Default::default()
        });
        let first = store
            .resolve(
                &pending.approval_id,
                "org1",
                true,
                "user1".into(),
                "approved".into(),
            )
            .expect("first decision");
        let retry = store
            .preview_resolution(
                &pending.approval_id,
                "org1",
                true,
                "user1".into(),
                "approved".into(),
            )
            .expect("same authenticated decision retry");
        assert_eq!(retry, first);
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

    #[test]
    fn granted_approval_continuation_is_quarantined_without_a_descriptor() {
        let approval = GatewayApproval {
            approval_id: "appr_1".to_owned(),
            org_id: "org_1".to_owned(),
            run_id: "run_1".to_owned(),
            status: STATUS_APPROVED.to_owned(),
            ..Default::default()
        };

        let error = quarantine_granted_approval_continuation(&approval)
            .expect_err("a grant alone cannot authorize generic ResumeRun");
        assert_eq!(error.code(), tonic::Code::Unavailable);

        let denied = GatewayApproval {
            status: STATUS_DENIED.to_owned(),
            ..approval
        };
        assert!(quarantine_granted_approval_continuation(&denied).is_ok());
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

    /// (3) RESTART-SURVIVAL: pending approvals persisted durably are recovered
    /// by tenant-scoped read-through insertion into a FRESH `ApprovalStore`
    /// (the kill-mid-pending case). Proves the cache reconstructs from the
    /// durable path of record after a process restart — no in-memory state was
    /// carried across the "restart".
    #[test]
    fn rehydrate_recovers_pending_approval_into_fresh_store() {
        // Simulate results from two independently authorized tenant reads.
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
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
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

        let first = commit_persisted_approval(
            &store,
            &publisher,
            prepare_request_approval(&store, make_req("run_1", "act_1"), "user-a").unwrap(),
            "user-a",
        )
        .await
        .unwrap()
        .approval
        .unwrap();
        let second = commit_persisted_approval(
            &store,
            &publisher,
            prepare_request_approval(&store, make_req("run_1", "act_1"), "user-a").unwrap(),
            "user-a",
        )
        .await
        .unwrap()
        .approval
        .unwrap();

        assert_eq!(first.approval_id, second.approval_id, "no duplicate minted");
        assert_eq!(store.list_pending_for_owner("org1", "user-a", "").len(), 1);
    }

    #[test]
    fn same_org_cache_never_exposes_another_users_pending_approval() {
        let store = ApprovalStore::new();
        store.create_for_owner(
            GatewayApproval {
                org_id: "org1".into(),
                run_id: "run_user_a".into(),
                ..Default::default()
            },
            "user-a",
        );

        assert_eq!(store.list_pending_for_owner("org1", "user-a", "").len(), 1);
        assert!(store
            .list_pending_for_owner("org1", "user-b", "")
            .is_empty());
    }

    #[test]
    fn same_org_user_cannot_preview_another_users_decision() {
        let store = ApprovalStore::new();
        let approval = store.create_for_owner(
            GatewayApproval {
                org_id: "org1".into(),
                run_id: "run_user_a".into(),
                ..Default::default()
            },
            "user-a",
        );
        let error = store
            .preview_resolution_for_owner(
                &approval.approval_id,
                "org1",
                true,
                "user-b".into(),
                String::new(),
            )
            .expect_err("same-org non-owner must not decide");
        assert!(matches!(error, ResolveError::NotFound));
    }

    #[test]
    fn fresh_replica_durable_approval_is_owner_bound_before_decision() {
        let store = ApprovalStore::new();
        let durable = durable_pending("appr-durable", "org1", "run1");

        let cached =
            cache_durable_approval_for_owner(&store, &durable, "appr-durable", "org1", "user-a")
                .expect("authenticated durable approval should hydrate a fresh replica");

        assert_eq!(cached.approval_id, "appr-durable");
        assert!(store
            .preview_resolution_for_owner(
                "appr-durable",
                "org1",
                true,
                "user-a".into(),
                String::new(),
            )
            .is_ok());
        assert!(matches!(
            store.preview_resolution_for_owner(
                "appr-durable",
                "org1",
                true,
                "user-b".into(),
                String::new(),
            ),
            Err(ResolveError::NotFound)
        ));
    }

    #[test]
    fn durable_read_through_rejects_mismatched_scope_without_cache_side_effect() {
        let store = ApprovalStore::new();
        let durable = durable_pending("appr-durable", "org-other", "run1");

        let error =
            cache_durable_approval_for_owner(&store, &durable, "appr-durable", "org1", "user-a")
                .expect_err("a mismatched durable tenant must fail closed");

        assert_eq!(error.code(), tonic::Code::DataLoss);
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn durable_read_through_requires_verified_owner_before_cache_side_effect() {
        let store = ApprovalStore::new();
        let durable = durable_pending("appr-durable", "org1", "run1");

        let error =
            cache_durable_approval_for_owner(&store, &durable, "appr-durable", "org1", "   ")
                .expect_err("an unbound durable response must not enter the cache");

        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(store.len(), 0);
    }

    #[tokio::test]
    async fn durable_read_failure_serves_only_the_authenticated_users_cache() {
        let store = ApprovalStore::new();
        for (owner, run_id) in [("user-a", "run-a"), ("user-b", "run-b")] {
            store.create_for_owner(
                GatewayApproval {
                    org_id: "org1".into(),
                    run_id: run_id.into(),
                    ..Default::default()
                },
                owner,
            );
        }
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let mut client = OrchestrationCoreServiceClient::new(channel);
        let response = handle_list_pending_approvals_authenticated(
            &store,
            &mut client,
            ListPendingApprovalsRequest {
                org_id: "org1".into(),
                ..Default::default()
            },
            "signed-session-token",
            "user-b",
        )
        .await
        .expect("cache-only degradation remains available");
        assert_eq!(response.approvals.len(), 1);
        assert_eq!(response.approvals[0].run_id, "run-b");
    }

    #[test]
    fn approval_cache_is_bounded_and_owner_index_is_evicted_with_entries() {
        let store = ApprovalStore::new();
        for index in 0..=MAX_CACHED_APPROVALS {
            store.create_for_owner(
                GatewayApproval {
                    org_id: "org1".into(),
                    run_id: format!("run_{index}"),
                    ..Default::default()
                },
                "user-a",
            );
        }
        assert_eq!(store.inner.len(), MAX_CACHED_APPROVALS);
        assert_eq!(store.owners.len(), MAX_CACHED_APPROVALS);
    }

    #[tokio::test]
    async fn prepared_request_does_not_touch_cache_or_publish_before_commit() {
        let store = ApprovalStore::new();
        let publisher = mp_events::publisher::InMemoryPublisher::new();
        let prepared = prepare_request_approval(&store, make_req("run_1", "act_1"), "user-a")
            .expect("valid request");

        assert_eq!(store.len(), 0);
        assert!(publisher.drain().is_empty());

        commit_persisted_approval(&store, &publisher, prepared, "user-a")
            .await
            .expect("commit durable result into bounded cache");
        assert_eq!(store.len(), 1);
        assert_eq!(publisher.drain().len(), 1);
    }
}
