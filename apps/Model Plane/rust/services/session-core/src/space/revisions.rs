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

    // ---- WS12-S4: who may revise what (pure) ------------------------------------------------

    /// One revision attempt as plain values; `owner` is the thread owner.
    fn attempt<'a>(
        is_delete: bool,
        target_role: &'a str,
        target_author: &'a str,
        caller: &'a str,
        thread_owner: &'a str,
        moderate_permission: bool,
    ) -> ClassFacts<'a> {
        ClassFacts {
            is_delete,
            target_role,
            target_author,
            caller,
            thread_owner,
            moderate_permission,
        }
    }

    /// The class table of [`revise_class`]: an author edits or deletes their own user message, the
    /// thread owner deletes an assistant answer, a moderator deletes (never edits), and everything
    /// else is `message_revise_forbidden` (`None`).
    #[test]
    fn revise_class_follows_the_table() {
        use ReviseClass::{AgentOwner, Author, Moderator};
        // (case, facts, expected class)
        let table: Vec<(&str, ClassFacts<'_>, Option<ReviseClass>)> = vec![
            // The author of a user message.
            ("author edits their own message", attempt(false, "user", "bob", "bob", "owner", false), Some(Author)),
            ("author deletes their own message", attempt(true, "user", "bob", "bob", "owner", false), Some(Author)),
            (
                "the author class wins over the moderator class on a delete",
                attempt(true, "user", "bob", "bob", "owner", true),
                Some(Author),
            ),
            (
                "the author class wins over the owner class when the owner wrote the message",
                attempt(true, "user", "owner", "owner", "owner", false),
                Some(Author),
            ),
            // The thread owner and an assistant answer.
            (
                "agent owner deletes an assistant answer",
                attempt(true, "assistant", "", "owner", "owner", false),
                Some(AgentOwner),
            ),
            (
                "agent owner deletes an assistant answer that names an agent author",
                attempt(true, "assistant", "agent-7", "owner", "owner", false),
                Some(AgentOwner),
            ),
            (
                "the owner class wins over the moderator class on an assistant delete",
                attempt(true, "assistant", "", "owner", "owner", true),
                Some(AgentOwner),
            ),
            (
                "a member who is not the owner does not delete an assistant answer",
                attempt(true, "assistant", "", "carol", "owner", false),
                None,
            ),
            // Nobody edits an assistant row.
            ("the owner cannot edit an assistant answer", attempt(false, "assistant", "", "owner", "owner", false), None),
            (
                "a moderator cannot edit an assistant answer",
                attempt(false, "assistant", "", "carol", "owner", true),
                None,
            ),
            (
                "an assistant row that names the caller as author is still not editable",
                attempt(false, "assistant", "carol", "carol", "owner", false),
                None,
            ),
            (
                "an assistant row that names the caller as author is not the author class on a delete",
                attempt(true, "assistant", "carol", "carol", "owner", false),
                None,
            ),
            // System and tool rows are never revisable.
            ("a system row is never edited", attempt(false, "system", "owner", "owner", "owner", true), None),
            ("a system row is never deleted", attempt(true, "system", "owner", "owner", "owner", true), None),
            ("a tool row is never edited", attempt(false, "tool", "owner", "owner", "owner", true), None),
            ("a tool row is never deleted", attempt(true, "tool", "owner", "owner", "owner", true), None),
            ("an unknown role is never revisable", attempt(true, "developer", "owner", "owner", "owner", true), None),
            ("an empty role is never revisable", attempt(true, "", "owner", "owner", "owner", true), None),
            // The moderator deletes, never edits.
            (
                "a moderator deletes another member's user message",
                attempt(true, "user", "bob", "carol", "owner", true),
                Some(Moderator),
            ),
            (
                "a moderator deletes an assistant answer they do not own",
                attempt(true, "assistant", "", "carol", "owner", true),
                Some(Moderator),
            ),
            (
                "a moderator cannot edit another member's message",
                attempt(false, "user", "bob", "carol", "owner", true),
                None,
            ),
            // A member against someone else's message.
            (
                "a member cannot edit another member's message",
                attempt(false, "user", "bob", "carol", "owner", false),
                None,
            ),
            (
                "a member cannot delete another member's message",
                attempt(true, "user", "bob", "carol", "owner", false),
                None,
            ),
            (
                "the thread owner is not a moderator of another member's user message",
                attempt(true, "user", "bob", "owner", "owner", false),
                None,
            ),
            // A row written before the author column existed reads as the thread owner's.
            (
                "a legacy row (author is the owner) is the owner's own message to edit",
                attempt(false, "user", "owner", "owner", "owner", false),
                Some(Author),
            ),
            (
                "a legacy row (author is the owner) is the owner's own message to delete",
                attempt(true, "user", "owner", "owner", "owner", false),
                Some(Author),
            ),
            (
                "a legacy row (author is the owner) is not another member's to edit",
                attempt(false, "user", "owner", "carol", "owner", false),
                None,
            ),
            (
                "a legacy row (author is the owner) is not another member's to delete without moderation",
                attempt(true, "user", "owner", "carol", "owner", false),
                None,
            ),
            // An empty identity never matches another empty identity.
            ("an empty author never matches an empty caller", attempt(true, "user", "", "", "owner", false), None),
            ("an empty author never matches an empty caller on an edit", attempt(false, "user", "", "", "", false), None),
            (
                "an empty owner never matches an empty caller on an assistant delete",
                attempt(true, "assistant", "", "", "", false),
                None,
            ),
        ];
        for (case, facts, expected) in table {
            assert_eq!(revise_class(&facts), expected, "{case}: {facts:?}");
        }
    }

    /// The class words are the closed `actor_class` values the revision row and the audit carry.
    #[test]
    fn revise_class_words_are_the_closed_actor_classes() {
        assert_eq!(ReviseClass::Author.as_str(), "author");
        assert_eq!(ReviseClass::AgentOwner.as_str(), "agent_owner");
        assert_eq!(ReviseClass::Moderator.as_str(), "moderator");
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

    // ---- WS12-S4: the request rules and the source scan (pure) ------------------------------

    fn edit_request(content: &str) -> pb::ReviseMessageRequest {
        pb::ReviseMessageRequest {
            org_id: ORG.to_owned(),
            thread_id: "thread-1".to_owned(),
            message_id: "message-1".to_owned(),
            action: rev::ACTION_EDIT.to_owned(),
            content: content.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn validate_request_holds_an_edit_to_text_and_a_delete_to_none() {
        validate_request(&edit_request("new words")).expect("a plain edit");
        let at_cap = "a".repeat(rev::MAX_EDIT_BYTES);
        validate_request(&edit_request(&at_cap)).expect("an edit at the byte cap");
        for (why, req) in [
            ("an empty edit", edit_request("")),
            ("a blank edit", edit_request("  \n ")),
            ("an edit over the byte cap", edit_request(&"a".repeat(rev::MAX_EDIT_BYTES + 1))),
        ] {
            let status = validate_request(&req).expect_err(why);
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{why}");
            assert_eq!(
                status.metadata().get(ERROR_CODE_TRAILER).and_then(|code| code.to_str().ok()),
                Some("invalid_message"),
                "{why}"
            );
        }
        let delete = pb::ReviseMessageRequest {
            action: rev::ACTION_DELETE.to_owned(),
            content: String::new(),
            ..edit_request("")
        };
        validate_request(&delete).expect("a delete without text");
        let delete_with_text = pb::ReviseMessageRequest { content: "text".to_owned(), ..delete.clone() };
        assert_eq!(validate_request(&delete_with_text).expect_err("a delete carries no text").code(), tonic::Code::InvalidArgument);
        let other = pb::ReviseMessageRequest { action: "restore".to_owned(), ..delete };
        assert_eq!(validate_request(&other).expect_err("an unknown action").code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn validate_request_needs_every_id_and_a_closed_moderation_reason() {
        for blank in ["org_id", "thread_id", "message_id"] {
            let mut req = edit_request("new words");
            match blank {
                "org_id" => req.org_id = "  ".to_owned(),
                "thread_id" => req.thread_id = String::new(),
                _ => req.message_id = String::new(),
            }
            assert_eq!(validate_request(&req).expect_err(blank).code(), tonic::Code::InvalidArgument, "{blank}");
        }
        for reason in rev::MODERATION_REASONS {
            let req = pb::ReviseMessageRequest { moderation_reason: reason.to_owned(), ..edit_request("new words") };
            validate_request(&req).unwrap_or_else(|status| panic!("{reason}: {status:?}"));
        }
        let unknown = pb::ReviseMessageRequest { moderation_reason: "because".to_owned(), ..edit_request("new words") };
        assert_eq!(validate_request(&unknown).expect_err("outside the closed set").code(), tonic::Code::InvalidArgument);
    }

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read the source directory") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    /// The only production write that changes what a message says is the purge in this file: every other
    /// `UPDATE messages` sets presentation, tree or provenance columns. The scan is scoped to writes that
    /// set `content` or `metadata`, in production code (test files and the part of a file after its first
    /// `#[cfg(test)]` are not production).
    #[test]
    fn update_messages_only_in_message_revision_rs() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        assert!(files.len() > 20, "the scan found the source tree");
        let mut writers = Vec::new();
        for path in files {
            let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
            if stem.ends_with("_tests") || stem == "tests" {
                continue;
            }
            let name = path.strip_prefix(&root).expect("under src").to_string_lossy().replace('\\', "/");
            let source = std::fs::read_to_string(&path).expect("read a source file");
            let mut rest = production_source(&source);
            while let Some(at) = rest.find("UPDATE messages") {
                let statement = &rest[at..];
                // The SQL string ends at the closing quote of its Rust literal.
                let statement = &statement[..statement.find('"').unwrap_or(statement.len())];
                let set_clause = statement.split("WHERE").next().unwrap_or(statement);
                if set_clause.contains("content =") || set_clause.contains("content=") || set_clause.contains("metadata =") {
                    writers.push(name.clone());
                    break;
                }
                rest = &rest[at + "UPDATE messages".len()..];
            }
        }
        assert_eq!(writers, ["space/revisions.rs"], "a content-blanking write on messages outside the revision path");
        // And it is the purge branch of `revise_message`, guarded by the retention rule.
        let revise = production_fn(production_source(include_str!("revisions.rs")), "\nasync fn revise_message(");
        let write = at(revise, "UPDATE messages SET content");
        assert!(at(revise, "if verified.purge_original {") < write, "the write sits inside the purge branch");
    }

    // ---- WS12-S4: the DB tier ---------------------------------------------------------------
    //
    // Every test is `#[ignore]`d. Run with `DATABASE_URL` pointing at a disposable Postgres whose database
    // name contains `test`: `cargo test -p session-core --bin session-core space::revisions -- --ignored
    // --test-threads=1`. They share one org, Space and person (the constants above), each with a thread
    // of its own, and remove what they wrote.

    mod db_tests {
        use super::*;
        use crate::grpc::space_w1_tests::{mint, signing_key, StreamKeysEnv, DECISION_REF, KEY_ID};
        use crate::space::test_support::{connect, seed_space_run, seed_space_thread};
        use mp_contracts::zdr_refusal::Refusal;

        const NEEDS_DB: &str = "requires DATABASE_URL to a disposable Postgres with session-core migrations";

        /// How one revision decision is signed (everything else is fixed, as in `sign_revise`).
        struct Spec<'a> {
            caller: &'a str,
            key: &'a str,
            audience_revision: u64,
            purge: bool,
            moderate: bool,
            moderation_reason: &'a str,
        }

        fn spec(key: &str) -> Spec<'_> {
            Spec {
                caller: PERSON,
                key,
                audience_revision: 4,
                purge: false,
                moderate: false,
                moderation_reason: "",
            }
        }

        /// The decision of an org whose retention rule purges the original text.
        fn purging(key: &str) -> Spec<'_> {
            Spec { purge: true, ..spec(key) }
        }

        /// Fills in and signs the Control `model.thread.revise` decision of `req` with the shared test key.
        fn signed(mut req: pb::ReviseMessageRequest, spec: &Spec<'_>) -> pb::ReviseMessageRequest {
            req.space_id = SPACE.to_owned();
            req.space_decision_ref = format!("revise-decision-{}", spec.key);
            req.recipient_audience_ref = "audience:space-1:4".to_owned();
            req.recipient_audience_hash = "sha256:revise-audience".to_owned();
            req.privacy_policy_ref = "privacy:org-1:5".to_owned();
            req.resource_authorization_ref = "control:space-1:thread-revise:7".to_owned();
            req.action_schema_hash = rev::DECISION_SCHEMA.to_owned();
            req.idempotency_key = spec.key.to_owned();
            req.authority_revision = 7;
            req.recipient_audience_revision = spec.audience_revision;
            req.moderation_reason = spec.moderation_reason.to_owned();
            req.payload_digest = thread_revise_payload_digest(&req, ORG, spec.caller);
            let mut permissions = vec![
                rev::PERMISSION_REVISE,
                if spec.purge { rev::PERMISSION_PURGE_ORIGINAL } else { rev::PERMISSION_RETAIN_ORIGINAL },
            ];
            if spec.moderate {
                permissions.push(rev::PERMISSION_MODERATE);
            }
            let now = Utc::now();
            let claims = serde_json::json!({
                "decision_ref": req.space_decision_ref,
                "org_id": ORG,
                "space_ref": SPACE,
                "subject_id": spec.caller,
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
                "nonce": format!("nonce-{}", spec.key),
                "authority_revision": req.authority_revision,
                "membership_revision": 4,
                "privacy_revision": 5,
                "entitlement_revision": 3,
                "permissions": permissions,
                "issued_at": now.to_rfc3339(),
                "expires_at": (now + chrono::Duration::minutes(1)).to_rfc3339(),
            });
            let signed = format!(
                "v2.{}.{}",
                URL_SAFE_NO_PAD.encode(KEY_ID.as_bytes()),
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims")),
            );
            let signature = signing_key().sign(signed.as_bytes());
            req.space_decision_token = format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()));
            req
        }

        /// A thread of the shared Space with its owner [`PERSON`], and the helpers every test shares.
        struct Room {
            pool: PgPool,
            thread: String,
            nonce: String,
        }

        type Revised = (pb::ReviseMessageResponse, Vec<crate::memory_erasure::ErasedMemory>);

        impl Room {
            async fn new() -> Option<Self> {
                let pool = connect().await?;
                let nonce = mp_ids::new_ulid();
                let thread = format!("revise-thread-{nonce}");
                sqlx::query("DELETE FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2")
                    .bind(SPACE)
                    .bind(PERSON)
                    .execute(&pool)
                    .await
                    .ok();
                seed_space_thread(&pool, ORG, SPACE, &thread, PERSON).await;
                Some(Self { pool, thread, nonce })
            }

            /// A message of the thread, stamped at audience `revision`, with a metadata marker.
            async fn message(&self, name: &str, role: &str, author: Option<&str>, content: &str, revision: i64) -> String {
                let id = format!("revise-{name}-{}", self.nonce);
                sqlx::query(
                    "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
                     recipient_audience_revision, recipient_audience_hash, authority_revision, \
                     resource_authorization_ref, author_subject_id, metadata) \
                     VALUES ($1, $2, $3, $4, $5, 'audience-1', $6, 'sha256:audience-1', 1, 'resource-1', $7, \
                             '{\"keep\": true}'::jsonb)",
                )
                .bind(&id)
                .bind(&self.thread)
                .bind(role)
                .bind(content)
                .bind(SPACE)
                .bind(revision)
                .bind(author)
                .execute(&self.pool)
                .await
                .expect("seed a message");
                id
            }

            fn request(&self, message: &str, action: &str, content: &str) -> pb::ReviseMessageRequest {
                pb::ReviseMessageRequest {
                    org_id: ORG.to_owned(),
                    thread_id: self.thread.clone(),
                    message_id: message.to_owned(),
                    action: action.to_owned(),
                    content: content.to_owned(),
                    ..Default::default()
                }
            }

            async fn revise(&self, req: &pb::ReviseMessageRequest, caller: &str) -> Result<Revised, Status> {
                revise_message(&self.pool, req, caller, &crate::gdpr::ErasureHoldGate::Off).await
            }

            async fn edit(&self, message: &str, content: &str, spec: &Spec<'_>) -> Revised {
                let req = signed(self.request(message, rev::ACTION_EDIT, content), spec);
                self.revise(&req, spec.caller).await.expect("an edit")
            }

            async fn delete(&self, message: &str, spec: &Spec<'_>) -> Revised {
                let req = signed(self.request(message, rev::ACTION_DELETE, ""), spec);
                self.revise(&req, spec.caller).await.expect("a delete")
            }

            /// `(revision_no, action, content, original_retention, actor_class)` of each revision, oldest first.
            async fn revisions(&self, message: &str) -> Vec<(i32, String, Option<String>, String, String)> {
                sqlx::query_as(
                    "SELECT revision_no, action, content, original_retention, actor_class \
                     FROM message_revisions WHERE message_id = $1 ORDER BY revision_no",
                )
                .bind(message)
                .fetch_all(&self.pool)
                .await
                .expect("read the revisions")
            }

            async fn content_of(&self, message: &str) -> String {
                sqlx::query_scalar("SELECT content FROM messages WHERE id = $1")
                    .bind(message)
                    .fetch_one(&self.pool)
                    .await
                    .expect("read the message")
            }

            async fn metadata_of(&self, message: &str) -> serde_json::Value {
                sqlx::query_scalar("SELECT metadata FROM messages WHERE id = $1")
                    .bind(message)
                    .fetch_one(&self.pool)
                    .await
                    .expect("read the metadata")
            }

            async fn xmin(&self, message: &str) -> String {
                sqlx::query_scalar("SELECT xmin::text FROM messages WHERE id = $1")
                    .bind(message)
                    .fetch_one(&self.pool)
                    .await
                    .expect("read xmin")
            }

            /// A live root run on the thread that holds it (a direct run, open and unexpired).
            async fn start_run(&self, name: &str) -> String {
                let run = format!("revise-run-{name}-{}", self.nonce);
                seed_space_run(&self.pool, ORG, SPACE, &self.thread, &run, "running", "persistent").await;
                sqlx::query(
                    "INSERT INTO managed_run_terminalization_outbox \
                     (run_id, org_id, user_id, start_key, configured_source, configured_terminal_step_id, zdr, deadline_at) \
                     VALUES ($1, $2, 'owner-1', $3, 'gateway_direct', 'model-gateway-direct-inference-final', false, \
                             now() + interval '15 minutes')",
                )
                .bind(&run)
                .bind(ORG)
                .bind(format!("sk-{run}"))
                .execute(&self.pool)
                .await
                .expect("seed the start obligation");
                run
            }

            async fn finish(&self) {
                for sql in [
                    "DELETE FROM agent_memory WHERE session_id = $1",
                    "DELETE FROM events WHERE run_id = $1",
                    "DELETE FROM session_audit_outbox WHERE payload->'details'->>'thread_id' = $1",
                    "DELETE FROM managed_run_terminalization_outbox WHERE run_id IN (SELECT id FROM runs WHERE thread_id = $1)",
                    "DELETE FROM runs WHERE thread_id = $1",
                    "DELETE FROM messages WHERE thread_id = $1",
                    "DELETE FROM threads WHERE id = $1",
                ] {
                    sqlx::query(sql).bind(&self.thread).execute(&self.pool).await.ok();
                }
                sqlx::query("DELETE FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2")
                    .bind(SPACE)
                    .bind(PERSON)
                    .execute(&self.pool)
                    .await
                    .ok();
            }
        }

        fn shown(rows: &[(i32, String, Option<String>, String, String)]) -> Vec<(i32, &str, Option<&str>, &str, &str)> {
            rows.iter()
                .map(|(no, action, content, retention, class)| {
                    (*no, action.as_str(), content.as_deref(), retention.as_str(), class.as_str())
                })
                .collect()
        }

        fn code_of(status: &Status) -> Option<&str> {
            status.metadata().get(ERROR_CODE_TRAILER).and_then(|code| code.to_str().ok())
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn revise_inserts_revision_and_never_updates_message_in_retain_mode() {
            let Some(room) = Room::new().await else { return };
            let _keys = StreamKeysEnv::publish();
            let message = room.message("m1", "user", Some(PERSON), "original words", 1).await;
            let before = room.xmin(&message).await;

            let (first, erased) = room.edit(&message, "first edit", &spec("k1")).await;
            assert_eq!(
                (first.revision_no, first.revision_state.as_str(), first.actor_class.as_str(), first.deduplicated),
                (1, "edited", "author", false),
                "{NEEDS_DB}"
            );
            assert!(erased.is_empty(), "a retained edit retires nothing");
            let (second, _) = room.edit(&message, "second edit", &spec("k2")).await;
            assert_eq!(second.revision_no, 2);

            let rows = room.revisions(&message).await;
            assert_eq!(
                shown(&rows),
                [(1, "edited", Some("first edit"), "retain", "author"), (2, "edited", Some("second edit"), "retain", "author")]
            );
            let prev: Vec<String> = sqlx::query_scalar("SELECT prev_content_digest FROM message_revisions WHERE message_id = $1 ORDER BY revision_no")
                .bind(&message)
                .fetch_all(&room.pool)
                .await
                .expect("read the digests");
            assert_eq!(prev, [digest_of("original words"), digest_of("first edit")], "each revision names the text it replaced");
            assert_eq!(room.content_of(&message).await, "original words", "retain mode keeps the original");
            assert_eq!(room.xmin(&message).await, before, "the message row was never updated");
            room.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn purge_mode_blanks_original_and_prior_revisions() {
            let Some(room) = Room::new().await else { return };
            let _keys = StreamKeysEnv::publish();
            let message = room.message("m1", "user", Some(PERSON), "original words", 1).await;
            room.edit(&message, "first edit", &spec("k1")).await;

            // An edit under purge blanks the original and the text of every older revision.
            let (second, _) = room.edit(&message, "second edit", &purging("k2")).await;
            assert_eq!(second.revision_no, 2, "{NEEDS_DB}");
            assert_eq!(room.content_of(&message).await, "", "the original text is gone");
            assert_eq!(room.metadata_of(&message).await, serde_json::json!({"keep": true}), "an edit leaves the metadata");
            let rows = room.revisions(&message).await;
            assert_eq!(
                shown(&rows),
                [(1, "edited", None, "retain", "author"), (2, "edited", Some("second edit"), "purge", "author")],
                "only the newest text survives"
            );

            // A delete under purge also clears the metadata and the newest edit's text.
            let (third, _) = room.delete(&message, &purging("k3")).await;
            assert_eq!((third.revision_no, third.revision_state.as_str()), (3, "deleted"));
            assert_eq!(room.content_of(&message).await, "");
            assert_eq!(room.metadata_of(&message).await, serde_json::json!({}), "a delete clears the metadata");
            let rows = room.revisions(&message).await;
            assert_eq!(
                shown(&rows),
                [(1, "edited", None, "retain", "author"), (2, "edited", None, "purge", "author"), (3, "deleted", None, "purge", "author")]
            );
            room.finish().await;
        }

        /// The fold of every read: the same fragments `ListConversation`, the model context and Dreaming splice in.
        async fn fold(room: &Room, ceiling: Option<i64>) -> Vec<(String, String, String, Option<serde_json::Value>, bool, bool)> {
            let sql = format!(
                "SELECT m.id, {content} AS content, {state} AS state, {metadata} AS metadata, \
                 {followed} AS followed_up, ({live}) AS live \
                 FROM messages m {join} WHERE m.thread_id = $1 ORDER BY m.sequence",
                content = FOLD_CONTENT_SQL,
                state = FOLD_STATE_SQL,
                metadata = FOLD_METADATA_SQL,
                followed = FOLD_FOLLOWED_UP_SQL,
                live = FOLD_NOT_DELETED_SQL,
                join = fold_join_sql("$2::BIGINT"),
            );
            sqlx::query_as(&sql)
                .bind(&room.thread)
                .bind(ceiling)
                .fetch_all(&room.pool)
                .await
                .expect("fold")
        }

        /// A revision row written directly, so a test controls its audience stamp.
        async fn put_revision(room: &Room, message: &str, no: i32, action: &str, content: Option<&str>, audience_revision: i64) {
            sqlx::query(
                "INSERT INTO message_revisions (revision_id, org_id, thread_id, message_id, revision_no, action, \
                 actor_subject_id, actor_class, content, content_digest, prev_content_digest, original_retention, \
                 idempotency_hash, space_id, recipient_audience_revision, authority_revision, resource_authorization_ref) \
                 VALUES ($1, $2, $3, $4, $5, $6, 'owner-1', 'author', $7, 'sha256:new', 'sha256:old', 'retain', $8, $9, $10, 1, 'resource-1')",
            )
            .bind(format!("rev-{message}-{no}"))
            .bind(ORG)
            .bind(&room.thread)
            .bind(message)
            .bind(no)
            .bind(action)
            .bind(content)
            .bind(format!("idem-{message}-{no}"))
            .bind(SPACE)
            .bind(audience_revision)
            .execute(&room.pool)
            .await
            .expect("seed a revision");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn fold_returns_effective_content_in_the_reads_that_splice_it() {
            let Some(room) = Room::new().await else { return };
            let plain = room.message("plain", "user", Some(PERSON), "plain words", 1).await;
            let edited = room.message("edited", "user", Some(PERSON), "old words", 1).await;
            let deleted = room.message("deleted", "user", Some(PERSON), "doomed words", 1).await;
            let answer = room.message("answer", "assistant", None, "an answer", 1).await;
            let late = room.message("late", "user", Some(PERSON), "late words", 1).await;
            put_revision(&room, &edited, 1, "edited", Some("new words"), 1).await;
            put_revision(&room, &deleted, 1, "deleted", None, 1).await;
            put_revision(&room, &late, 1, "deleted", None, 1).await;

            let marker = Some(serde_json::json!({"keep": true}));
            let rows = fold(&room, None).await;
            let by_id = |id: &str| rows.iter().find(|row| row.0 == id).unwrap_or_else(|| panic!("{id} is not read"));
            let row = by_id(&plain);
            assert_eq!((row.1.as_str(), row.2.as_str(), &row.3, row.4, row.5), ("plain words", "", &marker, false, true), "{NEEDS_DB}");
            let row = by_id(&edited);
            assert_eq!((row.1.as_str(), row.2.as_str(), &row.3, row.4, row.5), ("new words", "edited", &marker, false, true));
            let row = by_id(&deleted);
            assert_eq!((row.1.as_str(), row.2.as_str(), &row.3, row.4, row.5), ("", "deleted", &None, true, false), "an answer followed the deleted message");
            let row = by_id(&answer);
            assert_eq!((row.1.as_str(), row.2.as_str(), row.4, row.5), ("an answer", "", false, true));
            let row = by_id(&late);
            assert_eq!((row.1.as_str(), row.2.as_str(), row.4, row.5), ("", "deleted", false, false), "no answer followed the last message");
            room.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn deleted_applies_above_audience_ceiling() {
            let Some(room) = Room::new().await else { return };
            let message = room.message("m1", "user", Some(PERSON), "doomed words", 1).await;
            // The delete was made under audience revision 9; a reader at 4 must still not see the text.
            put_revision(&room, &message, 1, "deleted", None, 9).await;
            for ceiling in [Some(1), Some(4), Some(9), None] {
                let rows = fold(&room, ceiling).await;
                let row = &rows[0];
                assert_eq!((row.1.as_str(), row.2.as_str(), row.5), ("", "deleted", false), "ceiling {ceiling:?}, {NEEDS_DB}");
                assert_eq!(row.3, None, "a deleted message carries no metadata at ceiling {ceiling:?}");
            }
            room.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn edit_under_newer_audience_hidden_from_older_ceiling() {
            let Some(room) = Room::new().await else { return };
            let message = room.message("m1", "user", Some(PERSON), "original words", 1).await;
            put_revision(&room, &message, 1, "edited", Some("edit under two"), 2).await;
            put_revision(&room, &message, 2, "edited", Some("edit under nine"), 9).await;
            let text = |rows: &[(String, String, String, Option<serde_json::Value>, bool, bool)]| (rows[0].1.clone(), rows[0].2.clone());
            let own = |content: &str, state: &str| (content.to_owned(), state.to_owned());
            // The newest edit a reader's ceiling allows is the one it sees.
            assert_eq!(text(&fold(&room, None).await), own("edit under nine", "edited"), "{NEEDS_DB}");
            assert_eq!(text(&fold(&room, Some(9)).await), own("edit under nine", "edited"));
            assert_eq!(text(&fold(&room, Some(4)).await), own("edit under two", "edited"));
            assert_eq!(text(&fold(&room, Some(2)).await), own("edit under two", "edited"));
            // Below every edit the reader sees the original, as if it had never been edited.
            assert_eq!(text(&fold(&room, Some(1)).await), own("original words", ""));
            room.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn delete_retires_derived_memory_and_events_carry_thread_id() {
            let Some(room) = Room::new().await else { return };
            let _keys = StreamKeysEnv::publish();
            let doomed = room.message("doomed", "user", Some(PERSON), "doomed words", 1).await;
            let kept = room.message("kept", "user", Some(PERSON), "kept words", 1).await;
            let memory = |name: &str| format!("revise-memory-{name}-{}", room.nonce);
            for (name, message) in [("doomed", &doomed), ("kept", &kept)] {
                sqlx::query(
                    "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, owner, source_links) \
                     VALUES ($1, $2, $3, 'thread', $1, 'remembered', $4, $5)",
                )
                .bind(memory(name))
                .bind(ORG)
                .bind(&room.thread)
                .bind(PERSON)
                .bind(vec![format!("message:{message}")])
                .execute(&room.pool)
                .await
                .expect("seed a derived memory row");
            }

            let (response, erased) = room.delete(&doomed, &spec("k1")).await;
            assert_eq!(response.revision_state, "deleted", "{NEEDS_DB}");
            assert_eq!(
                erased,
                vec![crate::memory_erasure::ErasedMemory { memory_id: memory("doomed"), owner: PERSON.to_owned() }],
                "the rows that cite the deleted message are retired"
            );
            let remaining: Vec<String> = sqlx::query_scalar("SELECT id FROM agent_memory WHERE session_id = $1")
                .bind(&room.thread)
                .fetch_all(&room.pool)
                .await
                .expect("read memory");
            assert_eq!(remaining, [memory("kept")], "a row that cites another message stays");

            // The event is keyed by the thread, so deleting the thread erases it.
            let payload: serde_json::Value = sqlx::query_scalar(
                "SELECT payload FROM events WHERE run_id = $1 AND event_type = 'MESSAGE_REVISED'",
            )
            .bind(&room.thread)
            .fetch_one(&room.pool)
            .await
            .expect("the revision event, found by thread id");
            assert_eq!(payload["thread_id"], room.thread.as_str());
            assert_eq!(payload["message_id"], doomed.as_str());
            assert_eq!((payload["action"].as_str(), payload["revision_no"].as_i64()), (Some("deleted"), Some(1)));
            room.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn revision_dedupe_replay_and_conflict() {
            let Some(room) = Room::new().await else { return };
            let _keys = StreamKeysEnv::publish();
            let message = room.message("m1", "user", Some(PERSON), "original words", 1).await;
            let (first, _) = room.edit(&message, "first edit", &spec("k1")).await;
            assert!(!first.deduplicated, "{NEEDS_DB}");

            // The same key, action and text is answered with the stored row, never applied twice.
            let (replay, erased) = room.edit(&message, "first edit", &spec("k1")).await;
            assert!(replay.deduplicated);
            assert_eq!((replay.revision_no, replay.revision_state.as_str(), replay.actor_class.as_str()), (1, "edited", "author"));
            assert_eq!(
                replay.revised_at.map(|time| time.seconds),
                first.revised_at.map(|time| time.seconds),
                "the stored time, not a new one"
            );
            assert!(erased.is_empty());
            assert_eq!(room.revisions(&message).await.len(), 1);

            // The same key with other text, or another action, is a conflict.
            for (why, req) in [
                ("other text", signed(room.request(&message, rev::ACTION_EDIT, "another edit"), &spec("k1"))),
                ("another action", signed(room.request(&message, rev::ACTION_DELETE, ""), &spec("k1"))),
            ] {
                let status = room.revise(&req, PERSON).await.expect_err(why);
                assert_eq!(status.code(), tonic::Code::AlreadyExists, "{why}");
                assert_eq!(code_of(&status), Some("idempotency_key_conflict"), "{why}");
            }
            assert_eq!(room.revisions(&message).await.len(), 1, "a conflict writes nothing");
            // Another key is another revision.
            let (second, _) = room.edit(&message, "second edit", &spec("k2")).await;
            assert_eq!((second.revision_no, second.deduplicated), (2, false));
            // An edit that changes nothing is refused as a no-op.
            let same = signed(room.request(&message, rev::ACTION_EDIT, "second edit"), &spec("k3"));
            let status = room.revise(&same, PERSON).await.expect_err("nothing changes");
            assert_eq!(code_of(&status), Some("revision_noop"));
            room.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn revoked_member_cannot_revise() {
            let Some(room) = Room::new().await else { return };
            let _keys = StreamKeysEnv::publish();
            let message = room.message("m1", "user", Some(PERSON), "original words", 1).await;
            room.edit(&message, "while a member", &spec("k1")).await;
            sqlx::query("INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
                .bind(SPACE)
                .bind(ORG)
                .bind(PERSON)
                .execute(&room.pool)
                .await
                .expect("revoke the member");
            let req = signed(room.request(&message, rev::ACTION_EDIT, "after the revocation"), &spec("k2"));
            let status = room.revise(&req, PERSON).await.expect_err("revoked");
            assert_eq!(status.code(), tonic::Code::PermissionDenied, "{NEEDS_DB}");
            assert_eq!(room.revisions(&message).await.len(), 1, "the refused revision wrote nothing");
            room.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn stale_decision_rejected() {
            let Some(room) = Room::new().await else { return };
            let _keys = StreamKeysEnv::publish();
            let message = room.message("m1", "user", Some(PERSON), "original words", 1).await;
            sqlx::query("UPDATE threads SET recipient_audience_revision = 6 WHERE id = $1")
                .bind(&room.thread)
                .execute(&room.pool)
                .await
                .expect("the thread has seen audience 6");
            // A decision made against audience 4 predates a roster change the thread already reflects.
            let stale = signed(room.request(&message, rev::ACTION_EDIT, "stale edit"), &spec("k1"));
            let status = room.revise(&stale, PERSON).await.expect_err("stale");
            assert_eq!(status.code(), tonic::Code::Aborted, "{NEEDS_DB}");
            assert_eq!(code_of(&status), Some("space_authority_stale"));
            assert!(room.revisions(&message).await.is_empty());
            // The same revision under a decision of audience 6 is current.
            let current = Spec { audience_revision: 6, ..spec("k2") };
            let (response, _) = room.edit(&message, "current edit", &current).await;
            assert_eq!(response.revision_no, 1);
            room.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn revise_blocked_while_run_active() {
            let Some(room) = Room::new().await else { return };
            let _keys = StreamKeysEnv::publish();
            let older = room.message("older", "user", Some(PERSON), "an older question", 1).await;
            let latest = room.message("latest", "user", Some(PERSON), "the question being answered", 1).await;
            let run = room.start_run("live").await;

            // The message the live run is answering cannot change under it.
            let blocked = signed(room.request(&latest, rev::ACTION_EDIT, "changed under the run"), &spec("k1"));
            let status = room.revise(&blocked, PERSON).await.expect_err("a run is answering this message");
            assert_eq!(status.code(), tonic::Code::Aborted, "{NEEDS_DB}");
            assert_eq!(code_of(&status), Some(crate::run_phase::THREAD_RUN_ACTIVE_CODE));
            assert_eq!(
                status.metadata().get(crate::run_phase::ACTIVE_RUN_TRAILER).and_then(|id| id.to_str().ok()),
                Some(run.as_str()),
                "the refusal names the run"
            );
            assert!(room.revisions(&latest).await.is_empty());
            // An earlier question is not the one being answered.
            let (response, _) = room.edit(&older, "an older question, reworded", &spec("k2")).await;
            assert_eq!(response.revision_no, 1);
            // Once the run has ended the latest message can be revised.
            sqlx::query("UPDATE runs SET status = 'completed' WHERE id = $1")
                .bind(&run)
                .execute(&room.pool)
                .await
                .expect("end the run");
            let (response, _) = room.edit(&latest, "changed after the run", &spec("k3")).await;
            assert_eq!(response.revision_no, 1);
            room.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn message_revised_audit_body_has_no_content() {
            let Some(room) = Room::new().await else { return };
            let _keys = StreamKeysEnv::publish();
            let message = room.message("m1", "user", Some(PERSON), "CANARY-ORIGINAL-TEXT", 1).await;
            room.edit(&message, "CANARY-REPLACEMENT-TEXT", &spec("k1")).await;

            let audit: Vec<serde_json::Value> = sqlx::query_scalar(
                "SELECT payload FROM session_audit_outbox WHERE payload->'details'->>'thread_id' = $1",
            )
            .bind(&room.thread)
            .fetch_all(&room.pool)
            .await
            .expect("read the audit rows");
            assert_eq!(audit.len(), 1, "one audit row per revision, {NEEDS_DB}");
            let body = &audit[0];
            assert_eq!((body["event"].as_str(), body["outcome"].as_str(), body["plane"].as_str()), (Some("message_revised"), Some("success"), Some("model")));
            assert_eq!(body["resource_id"], message.as_str());
            let mut keys: Vec<&str> = body["details"].as_object().expect("details").keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                ["action", "actor_class", "moderation_reason", "original_retention", "revision_no", "space_id", "target_role", "thread_id"],
                "ids and enums only"
            );
            // Neither the audit row nor the event carries any text of the message.
            let event: serde_json::Value = sqlx::query_scalar("SELECT payload FROM events WHERE run_id = $1 AND event_type = 'MESSAGE_REVISED'")
                .bind(&room.thread)
                .fetch_one(&room.pool)
                .await
                .expect("the revision event");
            for (what, text) in [("audit", body.to_string()), ("event", event.to_string())] {
                assert!(!text.contains("CANARY"), "the {what} body carries message text: {text}");
                assert!(!text.contains("\"content\""), "the {what} body names a content field: {text}");
            }
            room.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn zdr_credential_cannot_revise() {
            let Some(room) = Room::new().await else { return };
            let _keys = StreamKeysEnv::publish();
            let message = room.message("m1", "user", Some(PERSON), "original words", 1).await;
            let req = signed(room.request(&message, rev::ACTION_EDIT, "an edit"), &spec("k1"));
            let mut request = Request::new(req);
            request.extensions_mut().insert(crate::auth::VerifiedIdentity::user_for_test_with_zdr(ORG, PERSON, true));
            let status = MessageRevisionGrpc::new(room.pool.clone(), None)
                .revise_message(request)
                .await
                .expect_err("a zero-retention credential writes nothing durable");
            assert_eq!(status.code(), tonic::Code::FailedPrecondition, "{NEEDS_DB}");
            assert_eq!(Refusal::from_status(&status), Some(Refusal::DURABLE_MUTATION_FORBIDDEN), "{status:?}");
            assert!(room.revisions(&message).await.is_empty(), "nothing was written");
            assert_eq!(room.content_of(&message).await, "original words");
            room.finish().await;
        }

        fn read_token(user: &str, revision: i64) -> String {
            mint(
                crate::grpc::CONTROL_SPACE_DECISION_AUDIENCE,
                &["thread:read"],
                false,
                |claims| {
                    claims["org_id"] = serde_json::json!(ORG);
                    claims["subject_id"] = serde_json::json!(user);
                    claims["space_ref"] = serde_json::json!(SPACE);
                    claims["recipient_audience_revision"] = serde_json::json!(revision);
                    claims["issued_at"] = serde_json::json!(Utc::now().to_rfc3339());
                    claims["expires_at"] = serde_json::json!((Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
                },
                false,
            )
        }

        async fn resolve(room: &Room, message: &str, ceiling: Option<i64>) -> Result<pb::ResolveMessageTargetResponse, Status> {
            let (decision_ref, token) = match ceiling {
                Some(revision) => (DECISION_REF.to_owned(), read_token(PERSON, revision)),
                None => (String::new(), String::new()),
            };
            let mut request = Request::new(pb::ResolveMessageTargetRequest {
                org_id: ORG.to_owned(),
                thread_id: room.thread.clone(),
                message_id: message.to_owned(),
                space_id: SPACE.to_owned(),
                space_read_decision_ref: decision_ref,
                space_read_decision_token: token,
                ..Default::default()
            });
            request.extensions_mut().insert(crate::auth::VerifiedIdentity::user_for_test(ORG, PERSON));
            MessageRevisionGrpc::new(room.pool.clone(), None)
                .resolve_message_target(request)
                .await
                .map(Response::into_inner)
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
        async fn resolve_message_target_respects_ceiling() {
            let Some(room) = Room::new().await else { return };
            let _keys = StreamKeysEnv::publish();
            let visible = room.message("visible", "user", Some(PERSON), "visible words", 1).await;
            let newer = room.message("newer", "user", Some(PERSON), "newer words", 6).await;
            let edited = room.message("edited", "user", Some(PERSON), "edited words", 1).await;
            let deleted = room.message("deleted", "assistant", None, "deleted words", 1).await;
            put_revision(&room, &edited, 1, "edited", Some("later edit"), 9).await;
            put_revision(&room, &deleted, 1, "deleted", None, 9).await;

            // A read decision must be present: there is no owner-bound fallback.
            let status = resolve(&room, &visible, None).await.expect_err("no decision");
            assert_eq!(status.code(), tonic::Code::PermissionDenied, "{NEEDS_DB}");

            let target = resolve(&room, &visible, Some(4)).await.expect("a message inside the ceiling");
            assert_eq!((target.role.as_str(), target.revision_state.as_str()), ("user", ""));
            // A message newer than the ceiling is not found, as is one that does not exist.
            for (why, id) in [("above the ceiling", newer.as_str()), ("missing", "revise-missing")] {
                let status = resolve(&room, id, Some(4)).await.expect_err(why);
                assert_eq!(status.code(), tonic::Code::NotFound, "{why}");
                assert_eq!(code_of(&status), Some("message_not_found"), "{why}");
            }
            // The same message is found by a reader whose ceiling reaches it.
            resolve(&room, &newer, Some(6)).await.expect("a ceiling that reaches the message");
            // An edit under a newer audience is not shown to an older reader; a deletion always applies.
            assert_eq!(resolve(&room, &edited, Some(4)).await.expect("edited").revision_state, "");
            assert_eq!(resolve(&room, &edited, Some(9)).await.expect("edited").revision_state, "edited");
            assert_eq!(resolve(&room, &deleted, Some(4)).await.expect("deleted").revision_state, "deleted");
            // A thread stamped above the ceiling hides every message in it.
            sqlx::query("UPDATE threads SET recipient_audience_revision = 8 WHERE id = $1")
                .bind(&room.thread)
                .execute(&room.pool)
                .await
                .expect("restamp the thread");
            let status = resolve(&room, &visible, Some(4)).await.expect_err("the thread is above the ceiling");
            assert_eq!(status.code(), tonic::Code::NotFound);
            room.finish().await;
        }
    }
}
