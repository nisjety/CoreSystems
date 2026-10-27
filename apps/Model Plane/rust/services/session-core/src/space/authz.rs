//! Space authorization seam for message appends (build plan P-SC1, WS01-S3, X01).
//!
//! `AppendAs` says on whose authority `append_message_in_tx_as` writes. The owner path is the one
//! that existed before this seam. The member path lets a Space member who does not own a thread
//! append a human `post` to it, and only when every condition in [`member_append_denial`] holds.
//! The gate is run twice: [`authorize_append`] picks the authority before the transaction, and
//! `space::append::enforce_member_append` re-runs it from the locked thread row inside the
//! transaction, so a revocation or a newer audience cannot slip in between.

use sqlx::PgPool;
use tonic::Status;

use crate::auth::{authorize_thread_owner_row, is_system_run_owner, OwnerIntent, VerifiedIdentity};
use mp_contracts::model_plane::v1 as pb;

/// Smallest and largest member post, in bytes of `content`.
pub(crate) const MEMBER_POST_MAX_BYTES: usize = 32_768;

/// The authority an append is made under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppendAs {
    /// The thread's owner, or a service acting for the owner: every caller of
    /// `append_message_in_tx` today.
    Owner,
    /// A Space member who does not own the thread. Reachable only through
    /// [`authorize_append`] with `SPACE_HUMAN_POSTS_CORE` on; the append core re-checks every
    /// condition under the thread lock.
    Member,
}

/// Everything the member gate looks at. Built from the request, the verified caller and the thread
/// row, so [`member_append_denial`] stays a pure function of plain values.
#[derive(Clone, Debug)]
pub(crate) struct MemberFacts {
    /// `space_human_posts_core` is on.
    pub flag_on: bool,
    /// The caller is a person (a user-bound credential), not a service principal.
    pub caller_is_human: bool,
    /// The caller's org equals the thread's org.
    pub org_matches: bool,
    /// `req.role == "user"`.
    pub role_is_user: bool,
    /// `req.post_kind == "post"`.
    pub post_kind_is_post: bool,
    /// The complete Control decision envelope is present.
    pub envelope_complete: bool,
    /// The envelope names the v2 thread-append schema.
    pub schema_is_v2: bool,
    /// The thread lives in a Space and it is the Space the request names.
    pub thread_space_matches: bool,
    /// The thread's owner is a system principal (scheduled and maintenance threads).
    pub owner_is_system: bool,
    /// `space_membership_revocations` holds a row for (Space, caller).
    pub author_revoked: bool,
    /// `req.content` length in bytes.
    pub content_bytes: usize,
    /// `req.content` is empty after trimming.
    pub content_blank: bool,
}

/// Why a member append is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemberDenial {
    FlagOff,
    ServiceCaller,
    WrongOrg,
    NotUserRole,
    NotPostKind,
    EnvelopeIncomplete,
    SchemaNotV2,
    OtherSpace,
    SystemOwner,
    Revoked,
    EmptyContent,
    OversizeContent,
}

impl MemberDenial {
    /// Denials that mean "this was not a member post attempt at all": the caller gets the owner
    /// refusal it always got, so a flag-off Session Core behaves exactly as before.
    pub(crate) fn is_passthrough(self) -> bool {
        matches!(
            self,
            Self::FlagOff
                | Self::ServiceCaller
                | Self::WrongOrg
                | Self::NotUserRole
                | Self::NotPostKind
        )
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::FlagOff => "member appends are not enabled on this Session Core",
            Self::ServiceCaller => "a member post needs a person",
            Self::WrongOrg => "caller org does not own this thread",
            Self::NotUserRole => "a member post is a user message",
            Self::NotPostKind => "a member may only append a post",
            Self::EnvelopeIncomplete => {
                "a member post needs a complete Control Space decision envelope"
            }
            Self::SchemaNotV2 => "a member post needs a v2 Control Space decision",
            Self::OtherSpace => "the thread does not belong to the Space the decision names",
            Self::SystemOwner => "a system-owned thread does not take member posts",
            Self::Revoked => "Space membership required for this thread",
            Self::EmptyContent => "a post cannot be empty",
            Self::OversizeContent => "a post is too long",
        }
    }
}

/// The member gate, in the order a refusal is most informative. `None` means every condition
/// holds; it is necessary, not sufficient: the signed decision is verified separately.
pub(crate) fn member_append_denial(facts: &MemberFacts) -> Option<MemberDenial> {
    if !facts.flag_on {
        return Some(MemberDenial::FlagOff);
    }
    if !facts.caller_is_human {
        return Some(MemberDenial::ServiceCaller);
    }
    if !facts.org_matches {
        return Some(MemberDenial::WrongOrg);
    }
    if !facts.role_is_user {
        return Some(MemberDenial::NotUserRole);
    }
    if !facts.post_kind_is_post {
        return Some(MemberDenial::NotPostKind);
    }
    if !facts.envelope_complete {
        return Some(MemberDenial::EnvelopeIncomplete);
    }
    if !facts.schema_is_v2 {
        return Some(MemberDenial::SchemaNotV2);
    }
    if !facts.thread_space_matches {
        return Some(MemberDenial::OtherSpace);
    }
    if facts.owner_is_system {
        return Some(MemberDenial::SystemOwner);
    }
    if facts.author_revoked {
        return Some(MemberDenial::Revoked);
    }
    if facts.content_blank {
        return Some(MemberDenial::EmptyContent);
    }
    if facts.content_bytes > MEMBER_POST_MAX_BYTES {
        return Some(MemberDenial::OversizeContent);
    }
    None
}

/// Facts that come from the request alone.
pub(crate) fn request_facts(req: &pb::AppendMessageRequest) -> (bool, bool, bool, bool, usize, bool) {
    let envelope_complete = crate::grpc::append_space_context_is_complete(req);
    (
        req.role.trim() == "user",
        matches!(req.post_kind.trim(), "post"),
        envelope_complete,
        req.action_schema_hash.trim() == crate::grpc::CONTROL_THREAD_APPEND_SCHEMA_V2,
        req.content.len(),
        req.content.trim().is_empty(),
    )
}

/// Decides the authority of an `AppendMessage` call. The thread's owner (or a service that was
/// always allowed) keeps the owner path unchanged. Anyone else is a member candidate: with the
/// flag on and every [`member_append_denial`] condition met they append as `Member`; otherwise they
/// get the owner refusal (flag off, not a post, not a person) or a specific permission denial.
pub(crate) async fn authorize_append(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: &pb::AppendMessageRequest,
) -> Result<AppendAs, Status> {
    let caller_user = caller.user_id().unwrap_or_default().to_owned();
    let row: Option<(String, String, String, bool)> = sqlx::query_as(
        "SELECT t.org_id, t.user_id, COALESCE(t.space_id, ''), \
         EXISTS(SELECT 1 FROM space_membership_revocations rev \
                WHERE rev.space_ref = t.space_id AND rev.subject_id = $2) \
         FROM threads t WHERE t.id = $1",
    )
    .bind(&req.thread_id)
    .bind(&caller_user)
    .fetch_optional(pool)
    .await
    .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, owner_id, space_id, author_revoked) =
        row.ok_or_else(|| Status::not_found("thread not found"))?;
    let owner_denied =
        match authorize_thread_owner_row(caller, &org_id, &owner_id, &space_id, OwnerIntent::Mutate)
        {
            Ok(()) => return Ok(AppendAs::Owner),
            Err(denied) => denied,
        };
    let (role_is_user, post_kind_is_post, envelope_complete, schema_is_v2, content_bytes, content_blank) =
        request_facts(req);
    let facts = MemberFacts {
        flag_on: mp_contracts::space_human_posts::core_enabled(),
        caller_is_human: !caller.is_service() && !caller_user.is_empty(),
        org_matches: caller.authorize_org(&org_id).is_ok(),
        role_is_user,
        post_kind_is_post,
        envelope_complete,
        schema_is_v2,
        thread_space_matches: !space_id.is_empty() && space_id == req.space_id,
        owner_is_system: is_system_run_owner(&owner_id),
        author_revoked,
        content_bytes,
        content_blank,
    };
    match member_append_denial(&facts) {
        None => Ok(AppendAs::Member),
        Some(denial) if denial.is_passthrough() => Err(owner_denied),
        Some(denial) => Err(Status::permission_denied(denial.message())),
    }
}
