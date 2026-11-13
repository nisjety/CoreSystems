//! Message revisions in Session Core (build plan WS12-S4): edit or delete ONE message of a Space
//! thread, and the fold every read applies.
//!
//! A revision is an append-only row in `message_revisions` keyed by the message it changes (one
//! message, no nesting, no supersede). `messages` rows are not modified except by the single
//! sanctioned purge write in [`revise_message`] (an org whose retention rule is `purge` blanks the
//! original text there). Everything else, `ListConversation`, the model's context, Dreaming,
//! reads the message through the fold in [`fold_join_sql`], so an edited message shows its
//! effective text and a deleted one shows nothing.
//!
//! Authority, in order: a zero-retention credential is refused first (`authorize_operation`);
//! `SPACE_MESSAGE_REVISE_CORE` must be on; the caller is a person in the thread's org; the thread
//! lives in the Space the decision names and has no revocation for the caller; the Control
//! `model.thread.revise` decision verifies for the CALLER (not the thread owner) and for this
//! exact message, action and text; the caller's class for this message is one of `author`,
//! `agent_owner` or `moderator` ([`revise_class`]). Content never reaches an event, an audit body
//! or a log: those carry ids, enums and digests.

use chrono::{DateTime, Utc};
use mp_contracts::message_revision as rev;
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::message_revision_service_server::MessageRevisionService;
use sha2::{Digest as _, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use tonic::metadata::MetadataValue;
use tonic::{Request, Response, Status};
use tracing::warn;

use crate::auth::{authorize_operation, identity, is_system_run_owner};
use crate::grpc::{
    record_metrics, to_proto_timestamp, verify_revise_space_decision, verify_thread_read_space_decision,
    VerifiedRevise,
};
use crate::letta_adapter::LettaMemoryAdapter;
use crate::run_phase::ERROR_CODE_TRAILER;

/// Event type of a recorded revision in the `events` table (ids and enums only).
const MESSAGE_REVISED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.MessageRevised";
/// Flat audit event name of a recorded revision.
const EVENT_MESSAGE_REVISED: &str = "message_revised";

// ---------------------------------------------------------------------------------------------
// The fold. Every transcript, context and Dreaming read joins `rv` and selects these.
// ---------------------------------------------------------------------------------------------

/// The latest revision of message alias `m` that `ceiling` (a SQL expression for the reader's
/// recipient-audience revision, `NULL::BIGINT` for none) allows. A deletion applies whatever the
/// ceiling (fail closed); an edit made under a newer audience is not shown to an older reader.
#[must_use]
pub(crate) fn fold_join_sql(ceiling: &str) -> String {
    format!(
        "LEFT JOIN LATERAL (
            SELECT r.action, r.content, r.actor_subject_id, r.revision_no, r.created_at
            FROM message_revisions r
            WHERE r.message_id = m.id
              AND (r.action = 'deleted' OR {ceiling} IS NULL OR r.recipient_audience_revision <= {ceiling})
            ORDER BY r.revision_no DESC
            LIMIT 1
         ) rv ON TRUE"
    )
}

/// Effective content: empty for a deleted message, the edit for an edited one.
pub(crate) const FOLD_CONTENT_SQL: &str = "CASE rv.action WHEN 'deleted' THEN '' WHEN 'edited' THEN COALESCE(rv.content, '') ELSE m.content END";
/// A deleted message carries no metadata (artifacts, citations, evidence).
pub(crate) const FOLD_METADATA_SQL: &str = "CASE WHEN rv.action = 'deleted' THEN NULL ELSE m.metadata END";
/// `''`, `'edited'` or `'deleted'`.
pub(crate) const FOLD_STATE_SQL: &str = "COALESCE(rv.action, '')";
/// Dreaming and other derivation loaders skip a deleted message.
pub(crate) const FOLD_NOT_DELETED_SQL: &str = "COALESCE(rv.action, '') <> 'deleted'";
/// A deleted user message that an assistant answer already followed: the answer is not retracted.
pub(crate) const FOLD_FOLLOWED_UP_SQL: &str = "COALESCE(rv.action = 'deleted' AND m.role = 'user' AND EXISTS (SELECT 1 FROM messages fa WHERE fa.thread_id = m.thread_id AND fa.role = 'assistant' AND fa.sequence > m.sequence), FALSE)";

// ---------------------------------------------------------------------------------------------
// Who may revise what.
// ---------------------------------------------------------------------------------------------

/// The class a revision is made under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReviseClass {
    Author,
    AgentOwner,
    Moderator,
}

impl ReviseClass {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Author => "author",
            Self::AgentOwner => "agent_owner",
            Self::Moderator => "moderator",
        }
    }
}

/// Everything [`revise_class`] looks at, as plain values.
#[derive(Clone, Debug)]
pub(crate) struct ClassFacts<'a> {
    pub is_delete: bool,
    pub target_role: &'a str,
    /// `author_subject_id`, or the thread owner for a row written before the column existed.
    pub target_author: &'a str,
    pub caller: &'a str,
    pub thread_owner: &'a str,
    /// The verified decision carries `thread:moderate`.
    pub moderate_permission: bool,
}

/// `author` edits or deletes their own user message; `agent_owner` (the thread owner, who is the
/// run initiator) deletes an assistant answer; `moderator` deletes a user or assistant message
/// under `thread:moderate`. A `system` or `tool` row is never revisable and nobody edits an
/// assistant row. `None` is `message_revise_forbidden`.
#[must_use]
pub(crate) fn revise_class(facts: &ClassFacts<'_>) -> Option<ReviseClass> {
    if !matches!(facts.target_role, "user" | "assistant") {
        return None;
    }
    if facts.target_role == "user" && !facts.target_author.is_empty() && facts.target_author == facts.caller {
        return Some(ReviseClass::Author);
    }
    if !facts.is_delete {
        return None;
    }
    if facts.target_role == "assistant" && !facts.thread_owner.is_empty() && facts.thread_owner == facts.caller {
        return Some(ReviseClass::AgentOwner);
    }
    if facts.moderate_permission {
        return Some(ReviseClass::Moderator);
    }
    None
}

fn coded(mut status: Status, code: &'static str) -> Status {
    status
        .metadata_mut()
        .insert(ERROR_CODE_TRAILER, MetadataValue::from_static(code));
    status
}

fn digest_of(text: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

/// The idempotency hash of one revision: the raw key is never stored.
fn idempotency_hash(org_id: &str, thread_id: &str, message_id: &str, actor: &str, key: &str) -> String {
    let mut hasher = Sha256::new();
    for part in [org_id, thread_id, message_id, actor, key] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

// ---------------------------------------------------------------------------------------------
// The service.
// ---------------------------------------------------------------------------------------------

/// Handler state for the message revision RPCs.
#[derive(Clone)]
pub(crate) struct MessageRevisionGrpc {
    pool: PgPool,
    letta: Option<LettaMemoryAdapter>,
}

impl MessageRevisionGrpc {
    pub(crate) fn new(pool: PgPool, letta: Option<LettaMemoryAdapter>) -> Self {
        Self { pool, letta }
    }
}

#[tonic::async_trait]
impl MessageRevisionService for MessageRevisionGrpc {
    async fn revise_message(
        &self,
        request: Request<pb::ReviseMessageRequest>,
    ) -> Result<Response<pb::ReviseMessageResponse>, Status> {
        let started = std::time::Instant::now();
        let result = async {
            let caller = identity(&request)?;
            // A zero-retention credential is refused first, with the typed catalogue refusal.
            authorize_operation(&caller, "session:write")?;
            if !mp_contracts::space_message_lifecycle::core_enabled() {
                return Err(coded(
                    Status::failed_precondition("message revisions are not enabled"),
                    "revise_disabled",
                ));
            }
            let req = request.into_inner();
            if caller.is_service() {
                return Err(Status::permission_denied("a message revision needs a person"));
            }
            let user_id = caller
                .user_id()
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| Status::permission_denied("a message revision needs a person"))?
                .to_owned();
            caller.authorize_org(&req.org_id)?;
            validate_request(&req)?;
            let (response, erased) =
                revise_message(&self.pool, &req, &user_id, crate::gdpr::erasure_hold_gate()).await?;
            if !erased.is_empty() {
                let outcome =
                    crate::memory_erasure::erase_semantic_copies(self.letta.as_ref(), &req.org_id, &erased)
                        .await;
                if !outcome.is_complete() {
                    metrics::counter!(
                        "mp_session_semantic_erasure_unconfirmed_total",
                        "operation" => "revise_message"
                    )
                    .increment(outcome.unconfirmed);
                    warn!(
                        operation = "revise_message",
                        unconfirmed = outcome.unconfirmed,
                        "semantic memory retirement incomplete: durable rows are gone but their semantic twins could not be confirmed erased"
                    );
                }
            }
            Ok(Response::new(response))
        }
        .await;
        record_metrics("revise_message", started, result.is_ok());
        result
    }

    async fn resolve_message_target(
        &self,
        request: Request<pb::ResolveMessageTargetRequest>,
    ) -> Result<Response<pb::ResolveMessageTargetResponse>, Status> {
        let started = std::time::Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.org_id.is_empty() || req.thread_id.is_empty() || req.message_id.is_empty() {
                return Err(Status::invalid_argument(
                    "org_id, thread_id and message_id are required",
                ));
            }
            caller.authorize_org(&req.org_id)?;
            // The read decision must be PRESENT: there is no owner-bound fallback for a Space
            // message the caller does not own.
            let read = verify_thread_read_space_decision(
                &req.space_read_decision_token,
                &req.space_read_decision_ref,
                &req.org_id,
                caller.user_id().unwrap_or_default(),
                &req.space_id,
            )?
            .ok_or_else(|| Status::permission_denied("a Space read decision is required"))?;
            let ceiling = read.recipient_audience_revision;
            let sql = format!(
                "SELECT m.role, {state} AS revision_state
                 FROM messages m
                 JOIN threads t ON t.id = m.thread_id
                 {join}
                 WHERE m.id = $1 AND m.thread_id = $2 AND t.org_id = $3 AND t.space_id = $4
                   AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $5)
                   AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $5)",
                state = FOLD_STATE_SQL,
                join = fold_join_sql("$5::BIGINT"),
            );
            let row: Option<(String, String)> = sqlx::query_as(&sql)
                .bind(&req.message_id)
                .bind(&req.thread_id)
                .bind(&req.org_id)
                .bind(&req.space_id)
                .bind(ceiling)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            let (role, revision_state) =
                row.ok_or_else(|| coded(Status::not_found("message not found"), "message_not_found"))?;
            Ok(Response::new(pb::ResolveMessageTargetResponse { role, revision_state }))
        }
        .await;
        record_metrics("resolve_message_target", started, result.is_ok());
        result
    }
}

#[allow(clippy::result_large_err)]
fn validate_request(req: &pb::ReviseMessageRequest) -> Result<(), Status> {
    if req.org_id.trim().is_empty() || req.thread_id.trim().is_empty() || req.message_id.trim().is_empty() {
        return Err(Status::invalid_argument("org_id, thread_id and message_id are required"));
    }
    match req.action.as_str() {
        rev::ACTION_EDIT => {
            if req.content.trim().is_empty() || req.content.len() > rev::MAX_EDIT_BYTES {
                return Err(coded(
                    Status::invalid_argument("an edit needs 1 to 32768 bytes of text"),
                    "invalid_message",
                ));
            }
        }
        rev::ACTION_DELETE => {
            if !req.content.is_empty() {
                return Err(coded(
                    Status::invalid_argument("a delete carries no text"),
                    "invalid_message",
                ));
            }
        }
        _ => return Err(Status::invalid_argument("action must be edit or delete")),
    }
    if !req.moderation_reason.is_empty() && !rev::MODERATION_REASONS.contains(&req.moderation_reason.as_str()) {
        return Err(Status::invalid_argument("moderation_reason is not in the closed set"));
    }
    Ok(())
}

/// The threads the hold gate is asked about for a destructive revision: the message's own thread, always, and every
/// thread the memory rows it retires cite. Sorted, each named once.
fn revision_hold_threads(thread_id: &str, cited: Vec<String>) -> Vec<String> {
    let mut threads = cited;
    threads.push(thread_id.to_owned());
    threads.sort();
    threads.dedup();
    threads
}

/// The memory rows a destructive revision retires (step 10), checked against the legal-hold gate (EP-9 C4, V3-sc-1).
/// A delete, and an edit under purge, delete the thread's memory rows that cite the message, and a purge blanks its
/// text; a hold keeps both. So the rows are locked here, the threads they cite are read after the lock, and the gate
/// is asked about the message's thread and those threads before any write of the revision. A refusal drops the
/// transaction with nothing changed. Switched off, nothing is read or asked, and `None` sends step 10 down the one
/// statement it always was.
///
/// # Errors
///
/// The typed hold refusal when a hold covers the organization or one of the threads, or cannot be ruled out;
/// `internal` for a failed statement. The caller's transaction is then dropped.
#[allow(clippy::result_large_err)]
async fn check_retiring_memory_against_holds(
    tx: &mut Transaction<'_, Postgres>,
    hold_gate: &crate::gdpr::ErasureHoldGate,
    org_id: &str,
    req: &pb::ReviseMessageRequest,
) -> Result<Option<Vec<String>>, Status> {
    if matches!(hold_gate, crate::gdpr::ErasureHoldGate::Off) {
        return Ok(None);
    }
    let rows: Vec<(String, Vec<String>)> = sqlx::query_as(
        "SELECT id, source_links FROM agent_memory
         WHERE session_id = $1 AND ('message:' || $2) = ANY(source_links)
         ORDER BY id FOR UPDATE",
    )
    .bind(&req.thread_id)
    .bind(&req.message_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    let links: Vec<String> = rows
        .iter()
        .flat_map(|(_, row_links)| row_links.iter().cloned())
        .collect();
    let cited = crate::gdpr::cited_thread_ids_in_tx(tx, org_id, &links)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    hold_gate
        .ensure_not_held(org_id, &revision_hold_threads(&req.thread_id, cited))
        .await
        .map_err(crate::gdpr::ErasureHoldError::into_status)?;
    Ok(Some(rows.into_iter().map(|(id, _)| id).collect()))
}

/// Records one revision in a single transaction under the thread's row lock and returns the
/// response plus the memory rows it retired (their semantic twins are erased after commit). A
/// destructive revision is checked against the legal-hold gate before its first write (step 7b).
#[allow(clippy::too_many_lines)]
async fn revise_message(
    pool: &PgPool,
    req: &pb::ReviseMessageRequest,
    caller_id: &str,
    hold_gate: &crate::gdpr::ErasureHoldGate,
) -> Result<(pb::ReviseMessageResponse, Vec<crate::memory_erasure::ErasedMemory>), Status> {
    let internal = |e: sqlx::Error| Status::internal(e.to_string());
    let is_delete = req.action == rev::ACTION_DELETE;
    let mut tx = pool.begin().await.map_err(internal)?;

    // 1. The thread, locked. A thread outside the org or outside the named Space is not found.
    let thread: Option<(String, String, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT org_id, user_id, space_id, recipient_audience_revision FROM threads WHERE id = $1 FOR UPDATE",
    )
    .bind(&req.thread_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let (org_id, owner_id, space_id, thread_revision) = thread
        .filter(|(org, _, space, _)| *org == req.org_id && space.as_deref() == Some(req.space_id.as_str()))
        .ok_or_else(|| coded(Status::not_found("thread not found"), "thread_not_found"))?;
    let space_id = space_id.unwrap_or_default();
    if is_system_run_owner(&owner_id) {
        return Err(Status::permission_denied("a system-owned thread does not take revisions"));
    }
    let (revoked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2)",
    )
    .bind(&space_id)
    .bind(caller_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    if revoked {
        return Err(Status::permission_denied("Space membership required for this thread"));
    }
    let request_revision = i64::try_from(req.recipient_audience_revision)
        .map_err(|_| Status::invalid_argument("recipient audience revision out of range"))?;
    let authority_revision = i64::try_from(req.authority_revision)
        .map_err(|_| Status::invalid_argument("authority revision out of range"))?;
    // A decision made against an older audience than the thread has seen predates a roster
    // change the thread already reflects; the BFF re-mints once on this code.
    if thread_revision.is_some_and(|stored| request_revision < stored) {
        return Err(coded(Status::aborted("space_authority_stale"), "space_authority_stale"));
    }

    // 2. The decision, verified for the CALLER.
    let verified: VerifiedRevise = verify_revise_space_decision(req, &org_id, caller_id)?;

    // 3. The target message, locked. A message newer than the decision's audience is not found.
    let target: Option<(String, String, String, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT role, content, post_kind, author_subject_id, recipient_audience_revision
         FROM messages WHERE id = $1 AND thread_id = $2 FOR UPDATE",
    )
    .bind(&req.message_id)
    .bind(&req.thread_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let (target_role, original_content, post_kind, author, _) = target
        .filter(|(_, _, _, _, rev_no)| rev_no.is_none_or(|stored| stored <= request_revision))
        .ok_or_else(|| coded(Status::not_found("message not found"), "message_not_found"))?;

    // 4. A repeat of the same revision is answered with the stored row; the same key with other
    //    content or another action is a conflict. Held to every check above, none below.
    let idem = idempotency_hash(&org_id, &req.thread_id, &req.message_id, caller_id, &req.idempotency_key);
    let content_digest = if is_delete { digest_of("") } else { digest_of(&req.content) };
    let existing: Option<(i32, String, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT revision_no, action, content_digest, actor_class, created_at
         FROM message_revisions WHERE message_id = $1 AND idempotency_hash = $2",
    )
    .bind(&req.message_id)
    .bind(&idem)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let wanted_action = if is_delete { "deleted" } else { "edited" };
    if let Some((revision_no, action, stored_digest, actor_class, created_at)) = existing {
        if action != wanted_action || stored_digest != content_digest {
            return Err(coded(
                Status::already_exists("idempotency key already names a different revision"),
                "idempotency_key_conflict",
            ));
        }
        return Ok((
            pb::ReviseMessageResponse {
                message_id: req.message_id.clone(),
                revision_no: u32::try_from(revision_no).unwrap_or(0),
                revision_state: action,
                revised_at: Some(to_proto_timestamp(created_at)),
                deduplicated: true,
                actor_class,
            },
            Vec::new(),
        ));
    }

    // 5. The latest revision and the text the message reads as now.
    let last: Option<(i32, String, Option<String>)> = sqlx::query_as(
        "SELECT revision_no, action, content FROM message_revisions
         WHERE message_id = $1 ORDER BY revision_no DESC LIMIT 1",
    )
    .bind(&req.message_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let (last_no, effective) = match &last {
        Some((_, action, _)) if action == "deleted" => {
            return Err(coded(
                Status::failed_precondition("message is already deleted"),
                "message_already_deleted",
            ));
        }
        Some((no, _, content)) => (*no, content.clone().unwrap_or_default()),
        None => (0, original_content.clone()),
    };
    if last_no >= rev::MAX_REVISIONS_PER_MESSAGE {
        return Err(coded(
            Status::failed_precondition("message has reached its revision limit"),
            "revision_limit",
        ));
    }

    // 6. Who may do this.
    let target_author = author.as_deref().unwrap_or(owner_id.as_str());
    let class = revise_class(&ClassFacts {
        is_delete,
        target_role: &target_role,
        target_author,
        caller: caller_id,
        thread_owner: &owner_id,
        moderate_permission: verified.moderate,
    })
    .ok_or_else(|| {
        coded(
            Status::permission_denied("this caller may not revise this message"),
            "message_revise_forbidden",
        )
    })?;
    if !is_delete && req.content == effective {
        return Err(coded(Status::invalid_argument("the edit changes nothing"), "revision_noop"));
    }

    // 7. Not while a run is answering this very message.
    if target_role == "user" && post_kind == "turn" && crate::run_phase::thread_run_guard_enabled() {
        let latest: Option<String> = sqlx::query_scalar(
            "SELECT id FROM messages WHERE thread_id = $1 AND role = 'user' AND post_kind = 'turn'
               AND on_active_path ORDER BY sequence DESC LIMIT 1",
        )
        .bind(&req.thread_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        if latest.as_deref() == Some(req.message_id.as_str()) {
            let active: Option<String> = sqlx::query_scalar(crate::run_phase::ACTIVE_ROOT_RUN_SQL)
                .bind(&req.thread_id)
                .bind(&org_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(internal)?;
            if let Some(run_id) = active {
                return Err(crate::run_phase::thread_run_active_status(&run_id));
            }
        }
    }

    // 7b. Held content (EP-9 C4, V3-sc-1). A delete, and an edit under purge, blank or delete what a legal hold keeps:
    //     the message's text and the memory rows that cite it. The hold is asked here, before the first write below, so
    //     a refusal changes nothing. A retained edit removes nothing and asks nothing.
    let retiring = if is_delete || verified.purge_original {
        check_retiring_memory_against_holds(&mut tx, hold_gate, &org_id, req).await?
    } else {
        None
    };

    // 8. The revision row. `content` is the new text of an edit (a ZDR credential never gets
    //    here); a delete stores none.
    let revision_no = last_no + 1;
    let revision_id = mp_ids::new_ulid();
    let reason = if class == ReviseClass::Moderator { req.moderation_reason.as_str() } else { "" };
    let retention = if verified.purge_original { "purge" } else { "retain" };
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO message_revisions
         (revision_id, org_id, thread_id, message_id, revision_no, action, actor_subject_id, actor_class,
          moderation_reason, content, content_digest, prev_content_digest, original_retention,
          idempotency_hash, space_id, recipient_audience_revision, authority_revision,
          resource_authorization_ref, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)",
    )
    .bind(&revision_id)
    .bind(&org_id)
    .bind(&req.thread_id)
    .bind(&req.message_id)
    .bind(revision_no)
    .bind(wanted_action)
    .bind(caller_id)
    .bind(class.as_str())
    .bind(reason)
    .bind(if is_delete { None } else { Some(req.content.as_str()) })
    .bind(&content_digest)
    .bind(digest_of(&effective))
    .bind(retention)
    .bind(&idem)
    .bind(&space_id)
    .bind(request_revision)
    .bind(authority_revision)
    .bind(&req.resource_authorization_ref)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;

    // 9. Purge mode: the one sanctioned write to `messages` that changes content. Blank the
    //    original (and its metadata on a delete), and the text of every older revision.
    if verified.purge_original {
        let purge = if is_delete {
            "UPDATE messages SET content = '', metadata = '{}'::jsonb WHERE id = $1"
        } else {
            "UPDATE messages SET content = '' WHERE id = $1"
        };
        sqlx::query(purge).bind(&req.message_id).execute(&mut *tx).await.map_err(internal)?;
        sqlx::query("UPDATE message_revisions SET content = NULL WHERE message_id = $1 AND revision_no < $2")
            .bind(&req.message_id)
            .bind(revision_no)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
    }

    // 10. Derived data: memory extracted from the old text. A delete (either mode) and an edit
    //     under purge retire the rows linked to this message. Behind the hold switch those are
    //     exactly the rows the hold check locked (7b).
    let erased: Vec<crate::memory_erasure::ErasedMemory> = if is_delete || verified.purge_original {
        let removed: Vec<(String, String)> = match &retiring {
            Some(ids) => sqlx::query_as::<_, (String, String)>(
                "DELETE FROM agent_memory WHERE session_id = $1 AND id = ANY($2) RETURNING id, owner",
            )
            .bind(&req.thread_id)
            .bind(ids)
            .fetch_all(&mut *tx)
            .await
            .map_err(internal)?,
            None => sqlx::query_as::<_, (String, String)>(
                "DELETE FROM agent_memory WHERE session_id = $1 AND ('message:' || $2) = ANY(source_links)
             RETURNING id, owner",
            )
            .bind(&req.thread_id)
            .bind(&req.message_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(internal)?,
        };
        removed
            .into_iter()
            .map(|(memory_id, owner)| crate::memory_erasure::ErasedMemory { memory_id, owner })
            .collect()
    } else {
        Vec::new()
    };

    // 11. The event and the audit row, ids and enums only. The event carries `thread_id` so a
    //     thread delete erases it; the audit row is kept for the audit trail.
    let resource = format!("message:{}", req.message_id);
    let event_idem = mp_events::idempotency::derive_idempotency_hash(
        "session-core",
        "MESSAGE_REVISED",
        &resource,
        &format!("{}:{}", req.thread_id, revision_no),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         SELECT $1, 'MESSAGE_REVISED', $2, $3, $4, t.org_id, t.user_id, $5, '', $6, $7, $8, 'session-core', 1
         FROM threads t WHERE t.id = $2",
    )
    .bind(mp_ids::new_ulid())
    .bind(&req.thread_id)
    .bind(serde_json::json!({
        "thread_id": &req.thread_id,
        "message_id": &req.message_id,
        "revision_no": revision_no,
        "action": wanted_action,
        "actor_class": class.as_str(),
        "actor_subject_id": caller_id,
        "space_id": &space_id,
        "recipient_audience_ref": &req.recipient_audience_ref,
        "recipient_audience_revision": request_revision,
        "recipient_audience_hash": &req.recipient_audience_hash,
        "authority_revision": authority_revision,
        "resource_authorization_ref": &req.resource_authorization_ref,
    }))
    .bind(now)
    .bind(&req.thread_id)
    .bind(&event_idem)
    .bind(&resource)
    .bind(MESSAGE_REVISED_TYPE_URL)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;

    let audit_event_id = format!("message_revised:session-core:{revision_id}");
    let audit_body = serde_json::json!({
        "event_id": &audit_event_id,
        "occurred_at": now.to_rfc3339(),
        "org_id": &org_id,
        "user_id": caller_id,
        "plane": "model",
        "producer": "session-core",
        "event": EVENT_MESSAGE_REVISED,
        "subject": &req.message_id,
        "resource_id": &req.message_id,
        "outcome": "success",
        "details": {
            "thread_id": &req.thread_id,
            "space_id": &space_id,
            "revision_no": revision_no,
            "action": wanted_action,
            "actor_class": class.as_str(),
            "moderation_reason": reason,
            "target_role": &target_role,
            "original_retention": retention,
        },
    });
    let audit_subject = crate::audit_publisher::audit_subject_for_event(EVENT_MESSAGE_REVISED)
        .ok_or_else(|| Status::internal("audit event name is not a subject token"))?;
    sqlx::query(
        "INSERT INTO session_audit_outbox (event_id, subject, payload) VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(&audit_event_id)
    .bind(audit_subject)
    .bind(audit_body)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;

    tx.commit().await.map_err(internal)?;
    metrics::counter!(
        "mp_session_message_revisions_total",
        "action" => wanted_action,
        "actor_class" => class.as_str()
    )
    .increment(1);
    Ok((
        pb::ReviseMessageResponse {
            message_id: req.message_id.clone(),
            revision_no: u32::try_from(revision_no).unwrap_or(0),
            revision_state: wanted_action.to_owned(),
            revised_at: Some(to_proto_timestamp(now)),
            deduplicated: false,
            actor_class: class.as_str().to_owned(),
        },
        erased,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::thread_revise_payload_digest;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use ed25519_dalek::{Signer as _, SigningKey};

    /// The file before its test module: the production code the source-order pins read.
    fn production_source(source: &str) -> &str {
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    /// The text of one production function: from its signature to its closing brace in column 0.
    fn production_fn<'a>(text: &'a str, signature: &str) -> &'a str {
        let start = text
            .find(signature)
            .unwrap_or_else(|| panic!("expected {signature:?}"));
        let rest = &text[start..];
        let end = rest.find("\n}\n").expect("the function's closing brace");
        &rest[..end + 2]
    }

    /// The position of `needle` in `text`, which must hold it.
    fn at(text: &str, needle: &str) -> usize {
        text.find(needle)
            .unwrap_or_else(|| panic!("expected {needle:?} in the revision path"))
    }

    /// V3-sc-1 (rel1): a delete, and an edit under purge, asks the legal-hold gate before its first write, about the
    /// message's own thread and the threads its memory rows cite. The RPC passes in the process gate. Switched off,
    /// the one statement that removes the memory rows is the one it always was.
    #[test]
    fn a_destructive_revision_asks_the_hold_gate_before_any_write() {
        let source = production_source(include_str!("revisions.rs"));
        assert!(
            source.contains(
                "revise_message(&self.pool, &req, &user_id, crate::gdpr::erasure_hold_gate())"
            ),
            "the RPC passes the process hold gate"
        );
        let revise = production_fn(source, "\nasync fn revise_message(");
        let check = production_fn(source, "\nasync fn check_retiring_memory_against_holds(");

        let condition = at(
            revise,
            "let retiring = if is_delete || verified.purge_original {",
        );
        let asked = at(revise, "check_retiring_memory_against_holds(&mut tx");
        let first_write = at(revise, "INSERT INTO message_revisions");
        let purge = at(revise, "let purge = if is_delete");
        let removed = at(
            revise,
            "DELETE FROM agent_memory WHERE session_id = $1 AND id = ANY($2)",
        );
        assert!(
            condition < asked && asked < first_write && first_write < purge && purge < removed,
            "the hold is asked before the revision row, the purge and the memory delete"
        );
        assert!(
            revise.contains("\"DELETE FROM agent_memory WHERE session_id = $1 AND ('message:' || $2) = ANY(source_links)"),
            "switched off, the one statement that removes the memory rows is unchanged"
        );

        let switch = at(
            check,
            "matches!(hold_gate, crate::gdpr::ErasureHoldGate::Off)",
        );
        let lock = at(check, "FOR UPDATE");
        let read = at(
            check,
            "crate::gdpr::cited_thread_ids_in_tx(tx, org_id, &links)",
        );
        let ask = at(
            check,
            ".ensure_not_held(org_id, &revision_hold_threads(&req.thread_id, cited))",
        );
        assert!(
            switch < lock && lock < read && read < ask,
            "switched off returns first; otherwise the rows are locked, the cited threads read, then the gate is asked"
        );
    }

    /// The hold check names the message's thread, and each thread its memory rows cite, once, in order.
    #[test]
    fn the_hold_check_names_the_thread_and_each_cited_thread_once() {
        assert_eq!(
            revision_hold_threads("thread-b", Vec::new()),
            vec!["thread-b".to_owned()]
        );
        assert_eq!(
            revision_hold_threads(
                "thread-b",
                vec!["thread-a".to_owned(), "thread-b".to_owned()]
            ),
            vec!["thread-a".to_owned(), "thread-b".to_owned()],
        );
    }

    // ---- DB tier: `#[ignore]`d, needs DATABASE_URL to a disposable `*test*` Postgres ----------

    /// The org, the Space, and the one person who owns the threads and wrote the message.
    const ORG: &str = "org-1";
    const SPACE: &str = "space-1";
    const PERSON: &str = "user-1";
    /// The text `seed_space_message` gives the message.
    const SEEDED_TEXT: &str = "canary body";

    /// Fills in the signed Control `model.thread.revise` decision of `req` the way Control mints one: every field the
    /// decision binds first, then the payload digest over them, then the envelope signed under `key_id`.
    fn sign_revise(
        req: &mut pb::ReviseMessageRequest,
        signing_key: &SigningKey,
        key_id: &str,
        purge: bool,
    ) {
        req.space_id = SPACE.to_owned();
        req.space_decision_ref = "revise-decision-1".to_owned();
        req.recipient_audience_ref = "audience:space-1:4".to_owned();
        req.recipient_audience_hash = "sha256:revise-audience".to_owned();
        req.privacy_policy_ref = "privacy:org-1:5".to_owned();
        req.resource_authorization_ref = "control:space-1:thread-revise:7".to_owned();
        req.action_schema_hash = rev::DECISION_SCHEMA.to_owned();
        req.idempotency_key = "revise-1".to_owned();
        req.authority_revision = 7;
        req.recipient_audience_revision = 4;
        let digest = thread_revise_payload_digest(req, ORG, PERSON);
        req.payload_digest = digest;
        let retention = if purge {
            rev::PERMISSION_PURGE_ORIGINAL
        } else {
            rev::PERMISSION_RETAIN_ORIGINAL
        };
        let now = Utc::now();
        let claims = serde_json::json!({
            "decision_ref": req.space_decision_ref,
            "org_id": ORG,
            "space_ref": SPACE,
            "subject_id": PERSON,
            "service_audience": "model-plane",
            "action_id": rev::DECISION_ACTION_ID,
            "action_schema_hash": rev::DECISION_SCHEMA,
            "payload_digest": req.payload_digest,
            "idempotency_key": req.idempotency_key,
            "recipient_audience_ref": req.recipient_audience_ref,
            "recipient_audience_revision": req.recipient_audience_revision,
            "recipient_audience_hash": req.recipient_audience_hash,
            "privacy_policy_ref": req.privacy_policy_ref,
            "resource_authorization_ref": req.resource_authorization_ref,
            "purpose": "assistant_collaboration",
            "lawful_basis": "contract",
            "privacy_class": "internal",
            "third_party_processing_allowed": false,
            "retention_class": "standard",
            "residency": "swedencentral",
            "deletion_scope": "space",
            "zero_data_retention": false,
            "nonce": "nonce-1",
            "authority_revision": req.authority_revision,
            "membership_revision": 4,
            "privacy_revision": 5,
            "entitlement_revision": 3,
            "permissions": [rev::PERMISSION_REVISE, retention],
            "issued_at": now.to_rfc3339(),
            "expires_at": (now + chrono::Duration::minutes(1)).to_rfc3339(),
        });
        let signed = format!(
            "v2.{}.{}",
            URL_SAFE_NO_PAD.encode(key_id.as_bytes()),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims")),
        );
        let signature = signing_key.sign(signed.as_bytes());
        req.space_decision_token =
            format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()));
    }

    /// The hold gate with the switch on, org-core answering from `url`, and session-core's credentials.
    fn hold_gate(url: &str) -> crate::gdpr::ErasureHoldGate {
        crate::gdpr::ErasureHoldGate::from_lookup(|name| match name {
            crate::gdpr::ERASURE_HOLD_CHECK_ENV => Some("true".to_owned()),
            crate::org_policy::ORG_CORE_URL_ENV => Some(url.to_owned()),
            crate::org_policy::SERVICE_TOKEN_ENV => Some("session-token".to_owned()),
            _ => None,
        })
    }

    /// Answers org-core's check the way the route does: of the threads asked about, only the held ones come back.
    struct HoldAnswer {
        held: Vec<String>,
    }

    impl wiremock::Respond for HoldAnswer {
        fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
            let asked: Vec<String> = serde_json::from_slice::<serde_json::Value>(&request.body)
                .ok()
                .and_then(|body| body["thread_ids"].as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect();
            let held_asked: Vec<String> = asked
                .into_iter()
                .filter(|id| self.held.contains(id))
                .collect();
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "organization_id": ORG,
                "window_days": null,
                "revision": 0,
                "org_held": false,
                "held_thread_ids": held_asked,
            }))
        }
    }

    /// Mounts [`HoldAnswer`] on the check route of [`ORG`], with session-core's credentials required.
    async fn mount_hold_answer(server: &wiremock::MockServer, held: Vec<String>) {
        use wiremock::matchers::{header, method, path};
        server.reset().await;
        wiremock::Mock::given(method("POST"))
            .and(path(format!("/internal/orgs/{ORG}/retention/check")))
            .and(header("x-service-id", "session-core"))
            .and(header("x-service-token", "session-token"))
            .respond_with(HoldAnswer { held })
            .mount(server)
            .await;
    }

    /// The refusal is the typed hold refusal: a failed precondition carrying `erasure_blocked_legal_hold`.
    fn assert_refused_by_hold(refusal: &Status) {
        assert_eq!(refusal.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            refusal
                .metadata()
                .get(ERROR_CODE_TRAILER)
                .and_then(|code| code.to_str().ok()),
            Some("erasure_blocked_legal_hold"),
        );
    }

    /// Nothing was retired: the message still reads as seeded, its memory row is there, and no revision was recorded.
    async fn assert_nothing_retired(pool: &PgPool, message: &str, memory: &str) {
        let text: String = sqlx::query_scalar("SELECT content FROM messages WHERE id = $1")
            .bind(message)
            .fetch_one(pool)
            .await
            .expect("read the message");
        assert_eq!(text, SEEDED_TEXT);
        let memories: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_memory WHERE id = $1")
            .bind(memory)
            .fetch_one(pool)
            .await
            .expect("count the memory row");
        assert_eq!(memories, 1, "the memory row survives the refusal");
        let revisions: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM message_revisions WHERE message_id = $1")
                .bind(message)
                .fetch_one(pool)
                .await
                .expect("count the revisions");
        assert_eq!(revisions, 0, "a refused revision records nothing");
    }

    /// A held thread refuses the revisions that would retire its content: a purge-mode edit of its message, and a
    /// delete whose memory row cites a held thread. Switched off, the same delete runs as it always did.
    /// Run with `DATABASE_URL` set to a disposable `*test*` database and `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn a_held_thread_refuses_the_revisions_that_would_retire_its_content() {
        let Some(pool) = crate::space::test_support::connect().await else {
            return;
        };
        let nonce = mp_ids::new_ulid();
        let thread = format!("revise-thread-{nonce}");
        let cited = format!("revise-cited-{nonce}");
        let message = format!("revise-message-{nonce}");
        let memory = format!("revise-memory-{nonce}");
        crate::space::test_support::seed_space_thread(&pool, ORG, SPACE, &thread, PERSON).await;
        crate::space::test_support::seed_space_thread(&pool, ORG, SPACE, &cited, PERSON).await;
        crate::space::test_support::seed_space_message(
            &pool,
            SPACE,
            &thread,
            &message,
            "user",
            Some(PERSON),
        )
        .await;
        // A memory row of the thread that cites the message, and a message of the other thread.
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, owner, source_links) \
             VALUES ($1, $2, $3, 'thread', $4, 'remembered', $5, $6)",
        )
        .bind(&memory)
        .bind(ORG)
        .bind(&thread)
        .bind(format!("revise-key-{nonce}"))
        .bind(PERSON)
        .bind(vec![format!("message:{message}"), format!("thread:{cited}")])
        .execute(&pool)
        .await
        .expect("seed the thread memory");

        let signing_key = SigningKey::from_bytes(&[61_u8; 32]);
        let key_id = "control-revise-test-key";
        std::env::remove_var("CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON");
        std::env::set_var("CONTROL_SPACE_DECISION_KEY_ID", key_id);
        std::env::set_var(
            "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64",
            URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes()),
        );
        let server = wiremock::MockServer::start().await;

        let mut purge_edit = pb::ReviseMessageRequest {
            org_id: ORG.to_owned(),
            thread_id: thread.clone(),
            message_id: message.clone(),
            action: rev::ACTION_EDIT.to_owned(),
            content: "Edited under purge".to_owned(),
            ..Default::default()
        };
        sign_revise(&mut purge_edit, &signing_key, key_id, true);
        let mut delete = purge_edit.clone();
        delete.action = rev::ACTION_DELETE.to_owned();
        delete.content = String::new();
        sign_revise(&mut delete, &signing_key, key_id, false);

        // A purge-mode edit of a held thread: refused before the text is blanked or the memory row removed.
        mount_hold_answer(&server, vec![thread.clone()]).await;
        let Err(refusal) =
            revise_message(&pool, &purge_edit, PERSON, &hold_gate(&server.uri())).await
        else {
            panic!("a held thread refuses a purge-mode edit");
        };
        assert_refused_by_hold(&refusal);
        assert_nothing_retired(&pool, &message, &memory).await;

        // A delete in a thread that is not held, whose memory row cites a message of a held thread: refused too.
        mount_hold_answer(&server, vec![cited.clone()]).await;
        let Err(refusal) = revise_message(&pool, &delete, PERSON, &hold_gate(&server.uri())).await
        else {
            panic!("a memory row that cites a held thread keeps its delete refused");
        };
        assert_refused_by_hold(&refusal);
        assert_nothing_retired(&pool, &message, &memory).await;

        // Switched off, the same delete runs as it always did: the memory row goes, the text stays.
        let (response, erased) =
            revise_message(&pool, &delete, PERSON, &crate::gdpr::ErasureHoldGate::Off)
                .await
                .expect("switched off, the delete runs");
        assert_eq!(response.revision_state, "deleted");
        assert_eq!(
            erased,
            vec![crate::memory_erasure::ErasedMemory {
                memory_id: memory.clone(),
                owner: PERSON.to_owned()
            }],
        );
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_memory WHERE id = $1")
            .bind(&memory)
            .fetch_one(&pool)
            .await
            .expect("count the memory row");
        assert_eq!(remaining, 0);
        let text: String = sqlx::query_scalar("SELECT content FROM messages WHERE id = $1")
            .bind(&message)
            .fetch_one(&pool)
            .await
            .expect("read the message");
        assert_eq!(
            text, SEEDED_TEXT,
            "a retained delete keeps the original text"
        );
    }
}
