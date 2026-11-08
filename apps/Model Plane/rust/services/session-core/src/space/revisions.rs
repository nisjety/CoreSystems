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
use sqlx::PgPool;
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
            let (response, erased) = revise_message(&self.pool, &req, &user_id).await?;
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

/// Records one revision in a single transaction under the thread's row lock and returns the
/// response plus the memory rows it retired (their semantic twins are erased after commit).
#[allow(clippy::too_many_lines)]
async fn revise_message(
    pool: &PgPool,
    req: &pb::ReviseMessageRequest,
    caller_id: &str,
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
    //     under purge retire the rows linked to this message.
    let erased: Vec<crate::memory_erasure::ErasedMemory> = if is_delete || verified.purge_original {
        sqlx::query_as::<_, (String, String)>(
            "DELETE FROM agent_memory WHERE session_id = $1 AND ('message:' || $2) = ANY(source_links)
             RETURNING id, owner",
        )
        .bind(&req.thread_id)
        .bind(&req.message_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(internal)?
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
