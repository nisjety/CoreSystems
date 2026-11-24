//! Addressee routing, member decide, reroute and the room read of approvals (build plan WS10-S6).
//!
//! An approval raised in a Space run is addressed to someone (`approvals.addressee_kind`, migration
//! 0074). The run owner (the requester) decides it on the existing owner path. Any other member
//! decides it with a Control-signed `model.approval.decide` decision; Control has already checked
//! that the member is the named approver, or holds the editor or manager role the addressee asks
//! for, so Session Core verifies the signature and every binding (org, caller, Space, approval,
//! decision, scope) and then applies the rules it can see itself: the approval's run belongs to the
//! decision's Space, a named addressee is the caller, a requester-addressed approval is decided by
//! its requester only, and the caller has not been removed from the Space. The approved action
//! still runs as the requester: the outbox row copies the approval's own `user_id`.
//!
//! The room read returns a redacted view (no context, no decision reason) of the approvals of runs
//! in a Space the caller holds a verified `model.thread.read` decision for, under the decision's
//! recipient-audience ceiling. Nothing here widens an owner-plane ACL: it never opens a run, a
//! transcript or a continuation descriptor.
//!
//! Every content-persisting step fails closed under zero data retention: a zero-retention caller or
//! run is refused or invisible. The switch `SESSION_CORE_APPROVAL_MEMBER_DECIDE` is a rollout
//! control, never an authorization input.

use mp_contracts::model_plane::v1 as pb;
use mp_contracts::space_approvals as sa;
use sqlx::FromRow;
use tonic::Status;
use tracing::warn;

use crate::orchestration_store::ApprovalRow;
use crate::space::audience_window::floor_admits_at;
use crate::store::Pool;

/// Typed refusal words on the `x-mp-error-code` trailer.
pub(crate) const NOT_ADDRESSED: &str = "approval_not_addressed_to_you";
const ADDRESSEE_INVALID: &str = "approval_addressee_invalid";

fn coded(status: Status, code: &'static str) -> Status {
    crate::space::mentions::coded_status(status, code)
}

/// The refusal while `SESSION_CORE_APPROVAL_MEMBER_DECIDE` is off.
#[allow(clippy::result_large_err)]
pub(crate) fn require_enabled() -> Result<(), Status> {
    require_enabled_from(sa::member_decide_core_enabled())
}

/// The answer for a given switch state, so the refusal can be checked without the environment.
#[allow(clippy::result_large_err)]
fn require_enabled_from(enabled: bool) -> Result<(), Status> {
    if enabled {
        Ok(())
    } else {
        Err(coded(
            Status::failed_precondition(sa::MEMBER_DECIDE_DISABLED),
            sa::MEMBER_DECIDE_DISABLED,
        ))
    }
}

/// A stated addressee as stored: the kind word and, for a named one, the subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredAddressee {
    pub(crate) kind: &'static str,
    pub(crate) subject_id: Option<String>,
}

/// Validate a stated addressee. `None` when unset or UNSPECIFIED (the requester, as before).
#[allow(clippy::result_large_err)]
pub(crate) fn addressee_from_request(
    addressee: Option<&pb::Addressee>,
) -> Result<Option<StoredAddressee>, Status> {
    let Some(addressee) = addressee else {
        return Ok(None);
    };
    let kind = pb::AddresseeKind::try_from(addressee.kind)
        .map_err(|_| coded(Status::invalid_argument("addressee kind is invalid"), ADDRESSEE_INVALID))?;
    let (word, named) = match kind {
        pb::AddresseeKind::Unspecified => {
            if addressee.subject_id.is_empty() {
                return Ok(None);
            }
            return Err(coded(
                Status::invalid_argument("an unspecified addressee carries no subject"),
                ADDRESSEE_INVALID,
            ));
        }
        pb::AddresseeKind::Requester => (sa::KIND_REQUESTER, false),
        pb::AddresseeKind::Named => (sa::KIND_NAMED, true),
        pb::AddresseeKind::AnyEditor => (sa::KIND_ANY_EDITOR, false),
        pb::AddresseeKind::AnyManager => (sa::KIND_ANY_MANAGER, false),
    };
    let subject = addressee.subject_id.trim();
    if named {
        if !mp_contracts::space_mentions::valid_subject_id(subject) {
            return Err(coded(
                Status::invalid_argument("a named addressee needs a valid subject_id"),
                ADDRESSEE_INVALID,
            ));
        }
    } else if !subject.is_empty() {
        return Err(coded(
            Status::invalid_argument("only a named addressee carries a subject_id"),
            ADDRESSEE_INVALID,
        ));
    }
    Ok(Some(StoredAddressee {
        kind: word,
        subject_id: named.then(|| subject.to_owned()),
    }))
}

/// The addressee of a new approval (WS10-S7). A person's own (`direct`) turn keeps what was stated;
/// an `ambient` or `automation` turn has no watching requester, so an approval that states no
/// addressee goes to any manager of the Space instead of waiting on the requester.
pub(crate) fn default_addressee_for_origin(
    stated: Option<StoredAddressee>,
    turn_origin: Option<&str>,
) -> Option<StoredAddressee> {
    if stated.is_none() && !crate::space::approval_gate::origin_is_direct(turn_origin) {
        return Some(StoredAddressee {
            kind: sa::KIND_ANY_MANAGER,
            subject_id: None,
        });
    }
    stated
}

/// Record the stated addressee on a row this request created. A no-op while the switch is off, so
/// the approval stays requester-addressed.
pub(crate) async fn record_addressee(
    pool: &Pool,
    approval_id: &str,
    addressee: &StoredAddressee,
) -> anyhow::Result<()> {
    if !sa::member_decide_core_enabled() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE approvals SET addressee_kind = $2, addressee_subject_id = $3 WHERE id = $1",
    )
    .bind(approval_id)
    .bind(addressee.kind)
    .bind(addressee.subject_id.as_deref())
    .execute(pool)
    .await?;
    Ok(())
}

/// What a decision or reroute is checked against, read from the durable rows.
#[derive(Debug, FromRow)]
pub(crate) struct Addressing {
    /// The run owner and the approval's `user_id`: the subject the approved action runs as.
    pub(crate) user_id: String,
    pub(crate) addressee_kind: String,
    pub(crate) addressee_subject_id: Option<String>,
    /// The run's Space; empty for a run outside any Space.
    pub(crate) space_id: String,
    pub(crate) retention_class: String,
}

pub(crate) async fn load_addressing(
    pool: &Pool,
    approval_id: &str,
    org_id: &str,
) -> Result<Option<Addressing>, Status> {
    sqlx::query_as::<_, Addressing>(
        "SELECT a.user_id, a.addressee_kind, a.addressee_subject_id, \
                COALESCE(r.space_id, '') AS space_id, r.retention_class \
         FROM approvals a JOIN runs r ON r.id = a.run_id AND r.org_id = a.org_id \
         WHERE a.id = $1 AND a.org_id = $2",
    )
    .bind(approval_id)
    .bind(org_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "approval addressing lookup failed");
        Status::unavailable("approval addressing unavailable")
    })
}

/// The Session Core half of the addressee rule. `any_editor` and `any_manager` are decided by
/// Control (it signed the decision only for a member holding that role); the other two are
/// checked here against the durable row.
#[allow(clippy::result_large_err)]
pub(crate) fn check_addressed(addressing: &Addressing, caller_user_id: &str) -> Result<(), Status> {
    let addressed = match addressing.addressee_kind.as_str() {
        sa::KIND_REQUESTER => addressing.user_id == caller_user_id,
        sa::KIND_NAMED => addressing.addressee_subject_id.as_deref() == Some(caller_user_id),
        sa::KIND_ANY_EDITOR | sa::KIND_ANY_MANAGER => true,
        _ => false,
    };
    if addressed {
        Ok(())
    } else {
        Err(coded(Status::permission_denied(NOT_ADDRESSED), NOT_ADDRESSED))
    }
}

/// A member removed from the Space after Control signed the decision decides nothing.
#[allow(clippy::result_large_err)]
async fn refuse_if_revoked(pool: &Pool, user_id: &str, space_id: &str) -> Result<(), Status> {
    let (revoked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM space_membership_revocations \
         WHERE space_ref = $1 AND subject_id = $2)",
    )
    .bind(space_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "approval membership revocation lookup failed");
        Status::unavailable("approval authority unavailable")
    })?;
    if revoked {
        Err(coded(
            Status::permission_denied("space_authority_stale"),
            "space_authority_stale",
        ))
    } else {
        Ok(())
    }
}

/// The whole member-decide gate. Returns the Space whose membership the decision proved, or
/// `None` when the request carries no Space fields (the owner path, unchanged).
///
/// A partial set of Space fields is refused. The ZDR refusal of the caller runs before this in the
/// handler; a run whose stored posture is not persistent is refused here as well.
#[allow(clippy::result_large_err)]
pub(crate) async fn authorize_member_decide(
    pool: &Pool,
    caller_user_id: Option<&str>,
    req: &pb::DecideApprovalRequest,
) -> Result<Option<String>, Status> {
    let stated = [&req.space_id, &req.space_decision_ref, &req.space_decision_token]
        .iter()
        .filter(|value| !value.trim().is_empty())
        .count();
    if stated == 0 {
        return Ok(None);
    }
    require_enabled()?;
    if stated != 3 {
        return Err(Status::invalid_argument(
            "a member decision requires space_id, decision ref, and token together",
        ));
    }
    let user_id = caller_user_id
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("a member decision requires a user principal"))?;
    let status = crate::orchestration_grpc::approval_state_to_str(req.decision)
        .filter(|status| matches!(*status, "granted" | "denied"))
        .ok_or_else(|| Status::invalid_argument("a member decision is granted or denied"))?;
    let scope = crate::orchestration_grpc::approval_scope_to_str(req.scope)
        .ok_or_else(|| Status::invalid_argument("invalid decision scope"))?;
    crate::grpc::verify_approval_space_decision(&crate::grpc::ApprovalDecisionExpect {
        action_id: sa::DECIDE_ACTION_ID,
        schema: sa::DECIDE_SCHEMA,
        permission: sa::PERMISSION_DECIDE,
        payload_digest: sa::decide_payload_digest(
            &req.org_id,
            user_id,
            &req.approval_id,
            &req.space_id,
            &req.space_decision_ref,
            status,
            scope,
        ),
        space_id: &req.space_id,
        decision_ref: &req.space_decision_ref,
        token: &req.space_decision_token,
        org_id: &req.org_id,
        user_id,
        metric_action: "approval_decide",
    })?;
    // From here on an approval outside the decision's Space is "not found": no oracle.
    let not_found = || Status::not_found("approval not found");
    let addressing = load_addressing(pool, &req.approval_id, &req.org_id)
        .await?
        .filter(|addressing| !addressing.space_id.is_empty() && addressing.space_id == req.space_id)
        .ok_or_else(not_found)?;
    if addressing.retention_class != "persistent" {
        return Err(not_found());
    }
    check_addressed(&addressing, user_id)?;
    refuse_if_revoked(pool, user_id, &req.space_id).await?;
    Ok(Some(req.space_id.clone()))
}

/// Move a pending approval to another addressee. Returns false when no pending approval of this
/// Space matched (decided meanwhile, or not in this Space).
pub(crate) async fn reroute(
    pool: &Pool,
    approval_id: &str,
    org_id: &str,
    space_id: &str,
    addressee: &StoredAddressee,
) -> anyhow::Result<bool> {
    let row: Option<(String,)> = sqlx::query_as(
        "UPDATE approvals SET addressee_kind = $4, addressee_subject_id = $5 \
         WHERE id = $1 AND org_id = $2 AND status = 'requested' \
           AND run_id IN (SELECT id FROM runs WHERE org_id = $2 AND space_id = $3 \
                          AND retention_class = 'persistent') \
         RETURNING id",
    )
    .bind(approval_id)
    .bind(org_id)
    .bind(space_id)
    .bind(addressee.kind)
    .bind(addressee.subject_id.as_deref())
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// Verify a reroute decision and the caller's standing. Returns the validated new addressee.
#[allow(clippy::result_large_err)]
pub(crate) async fn authorize_reroute(
    pool: &Pool,
    caller_user_id: Option<&str>,
    req: &pb::RerouteApprovalRequest,
) -> Result<StoredAddressee, Status> {
    require_enabled()?;
    if req.space_id.trim().is_empty()
        || req.space_decision_ref.trim().is_empty()
        || req.space_decision_token.trim().is_empty()
        || req.approval_id.trim().is_empty()
    {
        return Err(Status::invalid_argument(
            "a reroute requires approval_id, space_id, decision ref, and token",
        ));
    }
    let user_id = caller_user_id
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("a reroute requires a user principal"))?;
    let addressee = addressee_from_request(req.addressee.as_ref())?
        .ok_or_else(|| coded(Status::invalid_argument("an addressee is required"), ADDRESSEE_INVALID))?;
    crate::grpc::verify_approval_space_decision(&crate::grpc::ApprovalDecisionExpect {
        action_id: sa::REROUTE_ACTION_ID,
        schema: sa::REROUTE_SCHEMA,
        permission: sa::PERMISSION_REROUTE,
        payload_digest: sa::reroute_payload_digest(
            &req.org_id,
            user_id,
            &req.approval_id,
            &req.space_id,
            &req.space_decision_ref,
            addressee.kind,
            addressee.subject_id.as_deref().unwrap_or(""),
        ),
        space_id: &req.space_id,
        decision_ref: &req.space_decision_ref,
        token: &req.space_decision_token,
        org_id: &req.org_id,
        user_id,
        metric_action: "approval_reroute",
    })?;
    refuse_if_revoked(pool, user_id, &req.space_id).await?;
    Ok(addressee)
}

type AddressingRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// The addressing columns of a set of approvals and their run's agent. The run join is tenant
/// scoped like the approval read.
const ADDRESSING_SQL: &str =
    "SELECT a.id, a.addressee_kind, a.addressee_subject_id, a.grant_ref, a.turn_origin, \
            r.agent_subject_id \
       FROM approvals AS a \
       LEFT JOIN runs AS r ON r.id = a.run_id AND r.org_id = a.org_id \
      WHERE a.org_id = $1 AND a.id = ANY($2)";

/// Fill the S6 projection on approvals read from rows: addressee, grant, turn origin, and the
/// run's agent. The requester is set from the row itself in `approval_from_row`. A failed read
/// leaves the approvals without the projection (they read as requester-addressed), never
/// failing the call.
pub(crate) async fn attach_addressing(pool: &Pool, org_id: &str, approvals: &mut [pb::Approval]) {
    if approvals.is_empty() {
        return;
    }
    let ids: Vec<String> = approvals.iter().map(|approval| approval.id.clone()).collect();
    // The run's agent (runs.agent_subject_id, set only by a verified wake) rides along so an
    // "always" decision can name the agent its standing rule binds (DEPTH-WS10-S4-1).
    let rows: Result<Vec<AddressingRow>, _> = sqlx::query_as(ADDRESSING_SQL)
        .bind(org_id)
        .bind(&ids)
        .fetch_all(pool)
        .await;
    match rows {
        Ok(rows) => {
            for (id, kind, subject, grant_ref, turn_origin, agent) in rows {
                if let Some(approval) = approvals.iter_mut().find(|approval| approval.id == id) {
                    approval.addressee = Some(pb::Addressee {
                        kind: addressee_kind_from_word(&kind) as i32,
                        subject_id: subject.unwrap_or_default(),
                    });
                    approval.grant_ref = grant_ref.unwrap_or_default();
                    approval.turn_origin = turn_origin.unwrap_or_default();
                    approval.agent_subject_id = agent.unwrap_or_default().trim().to_owned();
                }
            }
        }
        Err(error) => warn!(error = %error, "approval addressing projection unavailable"),
    }
}

fn addressee_kind_from_word(word: &str) -> pb::AddresseeKind {
    match word {
        sa::KIND_NAMED => pb::AddresseeKind::Named,
        sa::KIND_ANY_EDITOR => pb::AddresseeKind::AnyEditor,
        sa::KIND_ANY_MANAGER => pb::AddresseeKind::AnyManager,
        _ => pb::AddresseeKind::Requester,
    }
}

/// The redacted room form of an approval: what is asked and where it stands, never the stored
/// context or the decider's reason.
pub(crate) fn redact_for_room(approval: &mut pb::Approval) {
    approval.context = None;
    approval.decision_reason.clear();
}

/// What a verified Space read bounds a room approval read by: the audience ceiling and the member's own
/// history floors (WS04-S11 revision floor, D18 timestamp floor; `None` for full history).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoomReadBounds {
    pub(crate) ceiling: i64,
    pub(crate) floor: Option<i64>,
    pub(crate) floor_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// The caller's Space read, verified: the audience ceiling, the member's history floors and the membership standing.
#[allow(clippy::result_large_err)]
pub(crate) async fn verify_room_read(
    pool: &Pool,
    org_id: &str,
    user_id: Option<&str>,
    space_id: &str,
    decision_ref: &str,
    token: &str,
) -> Result<Option<RoomReadBounds>, Status> {
    if space_id.trim().is_empty() && decision_ref.trim().is_empty() && token.trim().is_empty() {
        return Ok(None);
    }
    let user_id = user_id
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("a Space read requires a user principal"))?;
    let read = crate::grpc::verify_thread_read_space_decision(token, decision_ref, org_id, user_id, space_id)?
        .ok_or_else(|| Status::invalid_argument("a Space read decision is required"))?;
    refuse_if_revoked(pool, user_id, space_id).await?;
    Ok(Some(RoomReadBounds {
        ceiling: read.recipient_audience_revision,
        floor: read.history_floor,
        floor_at: read.history_floor_at,
    }))
}

const ROOM_APPROVAL_COLUMNS: &str = "a.id, a.run_id, a.plan_id, a.kind, a.status, a.requested_by, \
     a.decided_by, a.decision_reason, a.org_id, a.user_id, a.idempotency_key, a.metadata, \
     a.requested_at, a.decided_at, a.expires_at, a.continuation_mode, a.consumed_at";

/// Approvals of persistent runs in a Space, inside the audience ceiling and the member's history floors. An approval
/// requested before the member's timestamp floor, or of a run that started before it, is not returned (D18).
/// `run_id` and `approval_id` narrow the read (empty keeps all); `status` keeps one state (empty keeps all).
/// Newest first, at most [`sa::MAX_SPACE_APPROVALS`].
pub(crate) async fn list_room_approvals(
    pool: &Pool,
    org_id: &str,
    space_id: &str,
    bounds: RoomReadBounds,
    run_id: &str,
    approval_id: &str,
    status: &str,
) -> Result<Vec<ApprovalRow>, Status> {
    let sql = format!(
        "SELECT {ROOM_APPROVAL_COLUMNS} \
         FROM approvals a \
         JOIN runs r ON r.id = a.run_id AND r.org_id = a.org_id \
         JOIN threads t ON t.id = r.thread_id AND t.org_id = r.org_id \
         WHERE a.org_id = $1 AND r.space_id = $2 AND t.space_id = $2 \
           AND t.archived_at IS NULL \
           AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $3) \
           AND r.retention_class = 'persistent' \
           AND ($8::BIGINT IS NULL OR r.recipient_audience_revision IS NULL \
                OR r.recipient_audience_revision >= $8::BIGINT) \
           AND ($9::TIMESTAMPTZ IS NULL \
                OR (r.created_at >= $9::TIMESTAMPTZ AND a.requested_at >= $9::TIMESTAMPTZ)) \
           AND ($4 = '' OR a.run_id = $4) AND ($5 = '' OR a.id = $5) AND ($6 = '' OR a.status = $6) \
         ORDER BY a.requested_at DESC, a.id DESC LIMIT $7"
    );
    let mut rows = sqlx::query_as::<_, ApprovalRow>(&sql)
        .bind(org_id)
        .bind(space_id)
        .bind(bounds.ceiling)
        .bind(run_id)
        .bind(approval_id)
        .bind(status)
        .bind(sa::MAX_SPACE_APPROVALS)
        .bind(bounds.floor)
        .bind(bounds.floor_at)
        .fetch_all(pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Space approvals read failed");
            Status::unavailable("Space approvals unavailable")
        })?;
    // D18: the same rule on the rows read, so an approval requested before the floor is never returned.
    rows.retain(|row| floor_admits_at(row.requested_at, bounds.floor_at));
    Ok(rows)
}

#[derive(Debug, FromRow)]
struct AuthorityRow {
    run_id: String,
    org_id: String,
    requester: String,
    thread_id: String,
    space_id: String,
    status: String,
    kind: String,
    addressee_kind: String,
    addressee_subject_id: Option<String>,
    turn_origin: Option<String>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    recipient_audience_ref: Option<String>,
    recipient_audience_revision: Option<i64>,
    recipient_audience_hash: Option<String>,
    privacy_policy_ref: Option<String>,
    thread_resource_authorization_ref: Option<String>,
    authority_revision: Option<i64>,
}

fn unresolved() -> pb::ResolveApprovalAuthorityResponse {
    pb::ResolveApprovalAuthorityResponse::default()
}

/// The content-free bindings Control needs to sign a member decision for one approval. A
/// zero-retention run, a run outside a Space, a service-owned run and a run with an incomplete
/// authority projection all answer `resolved=false`.
pub(crate) async fn resolve_authority(
    pool: &Pool,
    approval_id: &str,
    org_id: &str,
) -> Result<pb::ResolveApprovalAuthorityResponse, Status> {
    let row: Option<AuthorityRow> = sqlx::query_as(
        "SELECT a.run_id, a.org_id, r.user_id AS requester, r.thread_id, \
                COALESCE(r.space_id, '') AS space_id, a.status, a.kind, a.addressee_kind, \
                a.addressee_subject_id, a.turn_origin, a.expires_at, \
                r.recipient_audience_ref, r.recipient_audience_revision, \
                r.recipient_audience_hash, r.privacy_policy_ref, \
                r.resource_authorization_ref AS thread_resource_authorization_ref, \
                r.authority_revision \
         FROM approvals a JOIN runs r ON r.id = a.run_id AND r.org_id = a.org_id \
         WHERE a.id = $1 AND a.org_id = $2 AND r.retention_class = 'persistent'",
    )
    .bind(approval_id)
    .bind(org_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::error!(%error, "approval authority lookup failed");
        Status::unavailable("approval authority unavailable")
    })?;
    let Some(row) = row else {
        return Ok(unresolved());
    };
    Ok(project_authority(approval_id, row).unwrap_or_else(unresolved))
}

fn project_authority(approval_id: &str, row: AuthorityRow) -> Option<pb::ResolveApprovalAuthorityResponse> {
    if row.space_id.trim().is_empty() || row.requester.starts_with("service:") {
        return None;
    }
    let text = |value: Option<String>| value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let recipient_audience_ref = text(row.recipient_audience_ref)?;
    let recipient_audience_hash = text(row.recipient_audience_hash)?;
    let privacy_policy_ref = text(row.privacy_policy_ref)?;
    let thread_resource_authorization_ref = text(row.thread_resource_authorization_ref)?;
    let recipient_audience_revision = u64::try_from(row.recipient_audience_revision?).ok()?;
    let authority_revision = u64::try_from(row.authority_revision?).ok()?;
    if recipient_audience_revision == 0 || authority_revision == 0 {
        return None;
    }
    Some(pb::ResolveApprovalAuthorityResponse {
        resolved: true,
        approval_id: approval_id.to_owned(),
        run_id: row.run_id,
        org_id: row.org_id,
        requester_subject_id: row.requester,
        thread_id: row.thread_id,
        space_id: row.space_id,
        approval_status: row.status,
        approval_kind: row.kind,
        addressee_kind: row.addressee_kind,
        addressee_subject_id: row.addressee_subject_id.unwrap_or_default(),
        turn_origin: row.turn_origin.unwrap_or_default(),
        expires_at: row
            .expires_at
            .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            .unwrap_or_default(),
        recipient_audience_ref,
        recipient_audience_revision,
        recipient_audience_hash,
        privacy_policy_ref,
        thread_resource_authorization_ref,
        authority_revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_phase::ERROR_CODE_TRAILER;
    use mp_contracts::model_plane::v1::AddresseeKind as Kind;

    fn stated(kind: Kind, subject: &str) -> pb::Addressee {
        pb::Addressee {
            kind: kind as i32,
            subject_id: subject.to_owned(),
        }
    }

    fn stored(kind: &'static str, subject: Option<&str>) -> Option<StoredAddressee> {
        Some(StoredAddressee {
            kind,
            subject_id: subject.map(str::to_owned),
        })
    }

    fn trailer(status: &Status) -> Option<&str> {
        status
            .metadata()
            .get(ERROR_CODE_TRAILER)
            .and_then(|value| value.to_str().ok())
    }

    #[test]
    fn an_unstated_or_unspecified_addressee_is_the_requester_as_before() {
        assert_eq!(addressee_from_request(None).expect("none"), None);
        assert_eq!(
            addressee_from_request(Some(&stated(Kind::Unspecified, ""))).expect("unspecified"),
            None
        );
    }

    #[test]
    fn a_stated_addressee_is_validated_and_stored_as_a_kind_word() {
        let ok = |kind: Kind, subject: &str| {
            addressee_from_request(Some(&stated(kind, subject))).expect("a valid addressee")
        };
        assert_eq!(ok(Kind::Requester, ""), stored("requester", None));
        assert_eq!(ok(Kind::AnyEditor, ""), stored("any_editor", None));
        assert_eq!(ok(Kind::AnyManager, ""), stored("any_manager", None));
        assert_eq!(ok(Kind::Named, "user-2"), stored("named", Some("user-2")));
        assert_eq!(
            ok(Kind::Named, "  user-2  "),
            stored("named", Some("user-2")),
            "the subject is stored trimmed"
        );
        let longest = "a".repeat(128);
        assert_eq!(ok(Kind::Named, &longest), stored("named", Some(&longest)), "128 characters is allowed");
    }

    #[test]
    fn a_named_addressee_needs_a_valid_subject_and_no_other_kind_carries_one() {
        let refused = |kind: i32, subject: &str| {
            addressee_from_request(Some(&pb::Addressee {
                kind,
                subject_id: subject.to_owned(),
            }))
            .expect_err("the addressee is refused")
        };
        let too_long = "a".repeat(129);
        for (name, kind, subject) in [
            ("named without a subject", Kind::Named as i32, ""),
            ("named with a blank subject", Kind::Named as i32, "   "),
            ("named with a space in it", Kind::Named as i32, "has space"),
            ("named with a leading dash", Kind::Named as i32, "-lead"),
            ("named with 129 characters", Kind::Named as i32, too_long.as_str()),
            ("requester with a subject", Kind::Requester as i32, "user-2"),
            ("any editor with a subject", Kind::AnyEditor as i32, "user-2"),
            ("any manager with a subject", Kind::AnyManager as i32, "user-2"),
            ("unspecified with a subject", Kind::Unspecified as i32, "user-2"),
            ("a kind nobody defined", 99, ""),
            ("a negative kind", -1, ""),
        ] {
            let status = refused(kind, subject);
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{name}");
            assert_eq!(trailer(&status), Some("approval_addressee_invalid"), "{name}");
        }
    }

    fn addressing(kind: &str, requester: &str, named: Option<&str>) -> Addressing {
        Addressing {
            user_id: requester.to_owned(),
            addressee_kind: kind.to_owned(),
            addressee_subject_id: named.map(str::to_owned),
            space_id: "space-1".to_owned(),
            retention_class: "persistent".to_owned(),
        }
    }

    #[test]
    fn check_addressed_table() {
        // (addressee kind, run owner, named subject, caller, addressed)
        let cases = [
            ("requester", "owner-1", None, "owner-1", true),
            ("requester", "owner-1", None, "member-2", false),
            // A stale subject on a requester row names nobody.
            ("requester", "owner-1", Some("member-2"), "member-2", false),
            ("named", "owner-1", Some("member-2"), "member-2", true),
            ("named", "owner-1", Some("member-2"), "owner-1", false),
            ("named", "owner-1", Some("member-2"), "member-3", false),
            ("named", "owner-1", None, "member-2", false),
            // Control decided the role of an editor or a manager; nothing here narrows it.
            ("any_editor", "owner-1", None, "member-2", true),
            ("any_manager", "owner-1", None, "member-2", true),
            ("any_editor", "owner-1", None, "owner-1", true),
            // A kind this code does not know fails closed.
            ("owner", "owner-1", None, "owner-1", false),
            ("", "owner-1", None, "owner-1", false),
        ];
        for (kind, requester, named, caller, addressed) in cases {
            let result = check_addressed(&addressing(kind, requester, named), caller);
            assert_eq!(result.is_ok(), addressed, "{kind:?} for {caller}");
            if let Err(status) = result {
                assert_eq!(status.code(), tonic::Code::PermissionDenied, "{kind:?} for {caller}");
                assert_eq!(status.message(), NOT_ADDRESSED);
                assert_eq!(trailer(&status), Some(NOT_ADDRESSED));
            }
        }
    }

    #[test]
    fn a_turn_nobody_watched_defaults_its_addressee_to_any_manager() {
        let manager = stored("any_manager", None);
        for origin in [Some("ambient"), Some("automation"), Some(" ambient ")] {
            assert_eq!(default_addressee_for_origin(None, origin), manager, "{origin:?}");
        }
        // A person's own turn, or a run with no claim, stays with the requester as before.
        for origin in [None, Some(""), Some("direct")] {
            assert_eq!(default_addressee_for_origin(None, origin), None, "{origin:?}");
        }
        // What the creator stated is kept on every origin.
        for origin in [None, Some("direct"), Some("ambient"), Some("automation")] {
            assert_eq!(
                default_addressee_for_origin(stored("named", Some("member-2")), origin),
                stored("named", Some("member-2")),
                "{origin:?}"
            );
            assert_eq!(
                default_addressee_for_origin(stored("requester", None), origin),
                stored("requester", None),
                "{origin:?}"
            );
        }
    }

    #[test]
    fn member_decide_answers_approval_member_decide_disabled_while_the_switch_is_off() {
        let refusal = require_enabled_from(false).expect_err("the switch is off");
        assert_eq!(refusal.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refusal.message(), "approval_member_decide_disabled");
        assert_eq!(refusal.message(), sa::MEMBER_DECIDE_DISABLED);
        assert_eq!(trailer(&refusal), Some("approval_member_decide_disabled"));
        assert!(require_enabled_from(true).is_ok());
        // Only an on-word turns it on.
        assert!(!sa::enabled_from(None));
        assert!(!sa::enabled_from(Some("0")));
        assert!(sa::enabled_from(Some("true")));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn a_revoked_member_is_refused_space_authority_stale() {
        let Some(pool) = crate::space::test_support::connect().await else {
            return;
        };
        let sfx = mp_ids::new_ulid();
        let (org, space, elsewhere) = (
            format!("org-rev-{sfx}"),
            format!("space-rev-{sfx}"),
            format!("space-rev-other-{sfx}"),
        );
        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, 'member-2')",
        )
        .bind(&space)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed revocation");
        refuse_if_revoked(&pool, "member-3", &space)
            .await
            .expect("a member nobody removed");
        refuse_if_revoked(&pool, "member-2", &elsewhere)
            .await
            .expect("removed from another Space only");
        let refusal = refuse_if_revoked(&pool, "member-2", &space)
            .await
            .expect_err("removed from this Space");
        assert_eq!(refusal.code(), tonic::Code::PermissionDenied);
        assert_eq!(refusal.message(), "space_authority_stale");
        assert_eq!(trailer(&refusal), Some("space_authority_stale"));
        sqlx::query("DELETE FROM space_membership_revocations WHERE space_ref = $1")
            .bind(&space)
            .execute(&pool)
            .await
            .ok();
    }
}
