//! Member append rules inside the append transaction (build plan WS01-S3).
//!
//! `append_message_in_tx_as` calls [`enforce_member_append`] under the thread's row lock when the
//! authority is `AppendAs::Member`. It re-runs the pure member gate from the locked row (so a
//! revocation or a newer audience that landed after the handler's pre-check still refuses), verifies
//! the Control decision with the CALLER as its subject (the owner path binds the owner), and rejects
//! a decision made against an older recipient audience than the thread has seen.

use mp_contracts::model_plane::v1 as pb;
use sqlx::{Postgres, Transaction};
use tonic::Status;

use super::authz::{member_append_denial, request_facts, MemberFacts};
use crate::auth::is_system_run_owner;

/// The locked `threads` row's fields the member rules read.
pub(crate) struct LockedThread<'a> {
    pub org_id: &'a str,
    pub owner_id: &'a str,
    pub space_id: Option<&'a str>,
    pub audience_revision: Option<i64>,
}

/// Refuses a member append unless it is a valid, current, authorized `post`. `author_subject_id`
/// is the verified caller. The handler has already checked that the caller is a person in the
/// thread's org.
pub(crate) async fn enforce_member_append(
    tx: &mut Transaction<'_, Postgres>,
    req: &pb::AppendMessageRequest,
    author_subject_id: &str,
    thread: &LockedThread<'_>,
) -> Result<(), Status> {
    let space_id = thread.space_id.unwrap_or_default();
    let (author_revoked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM space_membership_revocations \
         WHERE space_ref = $1 AND subject_id = $2)",
    )
    .bind(space_id)
    .bind(author_subject_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    let (role_is_user, post_kind_is_post, envelope_complete, schema_is_v2, content_bytes, content_blank) =
        request_facts(req);
    let facts = MemberFacts {
        flag_on: mp_contracts::space_human_posts::core_enabled(),
        caller_is_human: !author_subject_id.trim().is_empty(),
        org_matches: true,
        role_is_user,
        post_kind_is_post,
        envelope_complete,
        schema_is_v2,
        thread_space_matches: !space_id.is_empty() && space_id == req.space_id,
        owner_is_system: is_system_run_owner(thread.owner_id),
        author_revoked,
        content_bytes,
        content_blank,
    };
    if let Some(denial) = member_append_denial(&facts) {
        return Err(Status::permission_denied(denial.message()));
    }
    // Subject = the caller: Control signs a member decision for the member, never the owner.
    crate::grpc::verify_append_space_decision(req, thread.org_id, author_subject_id)?;
    // A decision made against an older recipient audience than the thread has seen predates a
    // roster change the thread already reflects. The BFF re-mints once on this code.
    let request_revision = i64::try_from(req.recipient_audience_revision)
        .map_err(|_| Status::invalid_argument("recipient audience revision out of range"))?;
    if thread
        .audience_revision
        .is_some_and(|stored| request_revision < stored)
    {
        return Err(Status::aborted("space_authority_stale"));
    }
    Ok(())
}
