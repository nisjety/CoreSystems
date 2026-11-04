//! `ArtifactRevisionService`: the durable revision store of thread artifacts (Wave 3 EP-3 S2).
//!
//! The gateway publishes each model revision of an artifact into the assistant message's
//! `metadata.artifacts` and counts versions in its own memory, so two replicas never agree on a
//! head and a user edit has no message to live in. This store gives every revision a row with its
//! own id, its editor and the message it came from, and its append is a compare-and-set on the
//! artifact's head inside one transaction, which holds across replicas. The metadata is still
//! written as before: the Space room aggregate and rehydration read it.
//!
//! Owner decisions taken by their recommendation (EP-3 section 3): the store is thread-owned in
//! session-core and the Data-owned Space canvas is a different object (OD-1, OD-2); the head is
//! thread-wide and a revision records the message it came from (OD-8); a Space thread takes only
//! model revisions (OD-9); 200 revisions per artifact and 20 MiB of revision text per thread
//! (OD-10); a thread that holds a waived run gets no row (OD-11, DEC-32).
//!
//! Refused: a zero-retention credential (every RPC: no ZDR content can be here, and saying so is
//! the contract), a waived credential or thread, another person's thread, a non-model editor on a
//! Space thread, a stale head (`aborted` `revision_mismatch`), a restore of a version that does not
//! exist (`failed_precondition` `revision_missing`) and an append past the caps
//! (`resource_exhausted` `artifact_revision_limit`). The switch is [`ENABLED_ENV`], default off;
//! while it is off every RPC answers `unavailable` `artifact_revisions_disabled`.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    self as pb, artifact_revision_service_server::ArtifactRevisionService,
};
use mp_ids::new_ulid;
use sha2::{Digest as _, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Instant;
use tonic::{Request, Response, Status};

use crate::auth::{
    authorize_operation, authorize_owner_row, authorize_thread_owner_row, identity, OwnerIntent,
    VerifiedIdentity,
};
use crate::grpc::{record_metrics, to_proto_timestamp};

/// The environment switch for the store. Default off, read by the strict parser the other switches
/// share, so only the literal `true` turns it on.
pub(crate) const ENABLED_ENV: &str = "SESSION_CORE_ARTIFACT_REVISIONS_ENABLED";

/// OD-10: the most revisions one artifact keeps. The next append is refused, not rotated: dropping
/// the oldest would silently rewrite the history the panel shows.
pub(crate) const MAX_REVISIONS_PER_ARTIFACT: i64 = 200;

/// OD-10: the most revision text one thread keeps, in bytes of stored content.
pub(crate) const MAX_THREAD_REVISION_BYTES: i64 = 20 * 1024 * 1024;

/// The largest revision text, in characters. Matches the gateway's persisted artifact cap
/// (`RecordedArtifact::MAX_PERSISTED_CHARS`) and the table's CHECK.
const MAX_CONTENT_CHARS: usize = 200_000;
/// The longest title, in characters (the table's CHECK).
const MAX_TITLE_CHARS: usize = 2_000;
/// The longest artifact id, in characters (the table's CHECK).
const MAX_ARTIFACT_ID_CHARS: usize = 256;
/// The longest idempotency key, in characters (the table's CHECK).
const MAX_IDEMPOTENCY_KEY_CHARS: usize = 128;
/// A listing's page when the caller names none, and the most one page returns.
const DEFAULT_LIST_LIMIT: u32 = 20;
const MAX_LIST_LIMIT: u32 = 50;

/// The refusal codes a caller can act on. Strings, as the other session-core refusals are.
pub(crate) const REVISION_MISMATCH: &str = "revision_mismatch";
pub(crate) const REVISION_MISSING: &str = "revision_missing";
pub(crate) const REVISION_LIMIT: &str = "artifact_revision_limit";
pub(crate) const SPACE_THREAD_EDIT: &str = "artifact_edit_space_thread";
pub(crate) const WAIVED_THREAD: &str = "artifact_revisions_waived_thread";
const DISABLED: &str = "artifact_revisions_disabled";

/// Read where it is used, as the fork switch is: the RPC holds no switch of its own.
pub(crate) fn artifact_revisions_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The revision RPCs. They hold the pool and nothing else; the rules are in this module.
pub struct ArtifactRevisionGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl ArtifactRevisionService for ArtifactRevisionGrpc {
    async fn append_artifact_revision(
        &self,
        request: Request<pb::AppendArtifactRevisionRequest>,
    ) -> Result<Response<pb::AppendArtifactRevisionResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            // `session:write` refuses a zero-retention credential before anything is read.
            authorize_operation(&caller, "session:write")?;
            refuse_retention_exempt(&caller)?;
            if !artifact_revisions_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            append_revision(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("append_artifact_revision", started, result.is_ok());
        result
    }

    async fn list_artifact_revisions(
        &self,
        request: Request<pb::ListArtifactRevisionsRequest>,
    ) -> Result<Response<pb::ListArtifactRevisionsResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            refuse_retention_exempt(&caller)?;
            authorize_operation(&caller, "session:read")?;
            if !artifact_revisions_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            list_revisions(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("list_artifact_revisions", started, result.is_ok());
        result
    }

    async fn get_artifact_revision(
        &self,
        request: Request<pb::GetArtifactRevisionRequest>,
    ) -> Result<Response<pb::GetArtifactRevisionResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            refuse_retention_exempt(&caller)?;
            authorize_operation(&caller, "session:read")?;
            if !artifact_revisions_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            get_revision(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("get_artifact_revision", started, result.is_ok());
        result
    }
}

/// A zero-retention or waived credential never reaches the store. `authorize_operation` lets a
/// ZDR credential read, and a waived bearer's `zdr` claim is false, so both are named here: the
/// store holds no ZDR content and no waived content, and the refusal keeps that explicit.
fn refuse_retention_exempt(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() || caller.zdr_waiver().is_some() {
        return Err(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    Ok(())
}

/// Who made a revision. The table's CHECK holds the same three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorKind {
    Model,
    User,
    Restore,
}

impl EditorKind {
    fn parse(raw: &str) -> Result<Self, Status> {
        match raw.trim() {
            "model" => Ok(Self::Model),
            "user" => Ok(Self::User),
            "restore" => Ok(Self::Restore),
            _ => Err(Status::invalid_argument(
                "editor_kind must be model, user or restore",
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::User => "user",
            Self::Restore => "restore",
        }
    }
}

/// An append request after its shape was checked. Every field is trimmed where a stray space
/// would make two names for one thing.
#[derive(Debug)]
struct CheckedAppend<'a> {
    org_id: &'a str,
    thread_id: &'a str,
    artifact_id: &'a str,
    kind: &'a str,
    title: &'a str,
    content: &'a str,
    editor: EditorKind,
    run_id: &'a str,
    source_message_id: &'a str,
    restored_from_version: u32,
    idempotency_key: &'a str,
    expected_version: u32,
    version: u32,
}

/// The kind is an open string (integration C-B8: a deck is an artifact too), bounded in shape so a
/// kind can never carry content: lowercase letters, digits, `_` and `-`, at most 64.
fn valid_kind(kind: &str) -> bool {
    let mut chars = kind.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && kind.len() <= 64
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Check an append's shape before anything is read. The table's CHECKs say the same, so a request
/// that passes here fails there only on a race the transaction resolves.
fn check_append(req: &pb::AppendArtifactRevisionRequest) -> Result<CheckedAppend<'_>, Status> {
    let checked = CheckedAppend {
        org_id: req.org_id.trim(),
        thread_id: req.thread_id.trim(),
        artifact_id: req.artifact_id.trim(),
        kind: req.kind.trim(),
        title: req.title.trim(),
        content: &req.content,
        editor: EditorKind::parse(&req.editor_kind)?,
        run_id: req.run_id.trim(),
        source_message_id: req.source_message_id.trim(),
        restored_from_version: req.restored_from_version,
        idempotency_key: req.idempotency_key.trim(),
        expected_version: req.expected_version,
        version: req.version,
    };
    if checked.org_id.is_empty() || checked.thread_id.is_empty() || checked.artifact_id.is_empty()
    {
        return Err(Status::invalid_argument(
            "org_id, thread_id and artifact_id are required",
        ));
    }
    if checked.artifact_id.chars().count() > MAX_ARTIFACT_ID_CHARS {
        return Err(Status::invalid_argument("artifact_id is too long"));
    }
    if !valid_kind(checked.kind) {
        return Err(Status::invalid_argument(
            "kind must be lowercase letters, digits, '_' or '-', at most 64",
        ));
    }
    if checked.title.chars().count() > MAX_TITLE_CHARS {
        return Err(Status::invalid_argument("title is too long"));
    }
    if checked.content.chars().count() > MAX_CONTENT_CHARS {
        return Err(Status::invalid_argument("content is too long"));
    }
    if checked.idempotency_key.chars().count() > MAX_IDEMPOTENCY_KEY_CHARS {
        return Err(Status::invalid_argument("idempotency_key is too long"));
    }
    // A restore names the version it restores, and nothing else names one: the table's CHECK.
    if (checked.editor == EditorKind::Restore) != (checked.restored_from_version > 0) {
        return Err(Status::invalid_argument(
            "restored_from_version is required for a restore and refused otherwise",
        ));
    }
    if checked.version > 0 && checked.version <= checked.expected_version {
        return Err(Status::invalid_argument(
            "version must be greater than expected_version",
        ));
    }
    if i32::try_from(checked.version.max(checked.expected_version)).is_err() {
        return Err(Status::invalid_argument("version out of range"));
    }
    Ok(checked)
}

/// The compare-and-set: the head the caller saw must be the head now.
fn check_head(head: u32, expected: u32) -> Result<(), Status> {
    if head != expected {
        return Err(Status::aborted(REVISION_MISMATCH));
    }
    Ok(())
}

/// OD-10: refuse the append that would pass either cap.
fn check_caps(artifact_revisions: i64, thread_bytes: i64, new_bytes: usize) -> Result<(), Status> {
    let new_bytes = i64::try_from(new_bytes).unwrap_or(i64::MAX);
    if artifact_revisions >= MAX_REVISIONS_PER_ARTIFACT
        || thread_bytes.saturating_add(new_bytes) > MAX_THREAD_REVISION_BYTES
    {
        return Err(Status::resource_exhausted(REVISION_LIMIT));
    }
    Ok(())
}

/// OD-9: a Space thread's audience is the room's, and member authority for edits needs a Control
/// decision, so only the model's own revisions are stored for it.
fn check_space_editor(in_space: bool, editor: EditorKind) -> Result<(), Status> {
    if in_space && editor != EditorKind::Model {
        return Err(Status::failed_precondition(SPACE_THREAD_EDIT));
    }
    Ok(())
}

/// The version a new row takes: the caller's (a model revision keeps the number the gateway
/// published, so metadata and table agree, with gaps where a turn wrote several versions), or the
/// next one.
fn next_version(head: u32, requested: u32) -> u32 {
    if requested > 0 {
        requested
    } else {
        head.saturating_add(1)
    }
}

fn content_sha256(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

/// The page a listing returns.
fn clamp_limit(limit: u32) -> u32 {
    match limit {
        0 => DEFAULT_LIST_LIMIT,
        n => n.min(MAX_LIST_LIMIT),
    }
}

/// The thread's owner and Space, read for an owner check.
struct ThreadOwner {
    org_id: String,
    user_id: String,
    space_id: Option<String>,
}

impl ThreadOwner {
    fn in_space(&self) -> bool {
        self.space_id
            .as_deref()
            .is_some_and(|space| !space.trim().is_empty())
    }
}

async fn read_thread<'e, E>(executor: E, thread_id: &str) -> Result<ThreadOwner, Status>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let row: Option<(String, String, Option<String>)> =
        sqlx::query_as("SELECT org_id, user_id, space_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(executor)
            .await
            .map_err(internal)?;
    let (org_id, user_id, space_id) = row.ok_or_else(|| Status::not_found("thread not found"))?;
    Ok(ThreadOwner {
        org_id,
        user_id,
        space_id,
    })
}

/// Append one revision. The switch and the session scope are the RPC's to check, so the append
/// itself can be tested without either.
pub(crate) async fn append_revision(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::AppendArtifactRevisionRequest,
) -> Result<pb::AppendArtifactRevisionResponse, Status> {
    let checked = check_append(&req)?;
    caller.authorize_org(checked.org_id)?;

    let mut tx = pool.begin().await.map_err(internal)?;
    // One writer per thread at a time: the head read, the caps (one of them thread-wide) and the
    // insert are one decision. An advisory lock rather than the thread row's, so a message append
    // on the same thread is never held behind a revision write.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('artifact_revisions:' || $1, 0))")
        .bind(checked.thread_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;

    let thread = read_thread(&mut *tx, checked.thread_id).await?;
    if thread.org_id != checked.org_id {
        return Err(Status::not_found("thread not found"));
    }
    authorize_owner_row(caller, &thread.org_id, &thread.user_id, OwnerIntent::Mutate)?;
    check_space_editor(thread.in_space(), checked.editor)?;
    if thread_is_waived(&mut tx, checked.thread_id).await? {
        return Err(Status::failed_precondition(WAIVED_THREAD));
    }

    if !checked.idempotency_key.is_empty() {
        if let Some(existing) =
            find_by_idempotency_key(&mut tx, checked.thread_id, checked.idempotency_key).await?
        {
            if existing.artifact_id != checked.artifact_id
                || existing.content_sha256 != content_sha256(checked.content)
            {
                return Err(Status::failed_precondition("idempotency_key_reused"));
            }
            tx.commit().await.map_err(internal)?;
            return Ok(pb::AppendArtifactRevisionResponse {
                revision: Some(existing.into_proto(true)),
                outcome: pb::AppendOutcome::Duplicate as i32,
            });
        }
    }

    let (head, revisions): (i32, i64) = sqlx::query_as(
        "SELECT COALESCE(MAX(version), 0), COUNT(*) FROM artifact_revisions
         WHERE thread_id = $1 AND artifact_id = $2",
    )
    .bind(checked.thread_id)
    .bind(checked.artifact_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    let head = u32::try_from(head).map_err(|_| Status::internal("head version out of range"))?;
    check_head(head, checked.expected_version)?;
    let thread_bytes: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(octet_length(content)), 0)::BIGINT FROM artifact_revisions
         WHERE thread_id = $1",
    )
    .bind(checked.thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    check_caps(revisions, thread_bytes, checked.content.len())?;

    if checked.editor == EditorKind::Restore {
        let restored: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM artifact_revisions
                            WHERE thread_id = $1 AND artifact_id = $2 AND version = $3)",
        )
        .bind(checked.thread_id)
        .bind(checked.artifact_id)
        .bind(to_i32(checked.restored_from_version)?)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
        if !restored {
            return Err(Status::failed_precondition(REVISION_MISSING));
        }
    }

    let source_message_id = source_message(&mut tx, &checked).await?;
    let row = RevisionRow {
        id: new_ulid(),
        thread_id: checked.thread_id.to_owned(),
        artifact_id: checked.artifact_id.to_owned(),
        version: to_i32(next_version(head, checked.version))?,
        kind: checked.kind.to_owned(),
        title: checked.title.to_owned(),
        content: checked.content.to_owned(),
        editor_kind: checked.editor.as_str().to_owned(),
        editor_subject_id: caller
            .user_id()
            .unwrap_or_else(|| caller.principal_id())
            .to_owned(),
        run_id: non_empty(checked.run_id),
        source_message_id,
        restored_from_version: (checked.restored_from_version > 0)
            .then(|| to_i32(checked.restored_from_version))
            .transpose()?,
        content_sha256: content_sha256(checked.content),
        created_at: Utc::now(),
    };
    insert_row(&mut tx, checked.org_id, non_empty(checked.idempotency_key), &row).await?;
    tx.commit().await.map_err(internal)?;
    Ok(pb::AppendArtifactRevisionResponse {
        revision: Some(row.into_proto(true)),
        outcome: pb::AppendOutcome::Appended as i32,
    })
}

/// The assistant message a model revision came from. A named message must be in the thread; a
/// named run must be the thread's, and when no message is named it is the answer that run's
/// completion saved (the gateway does not learn the message id from the completion call).
async fn source_message(
    tx: &mut Transaction<'_, Postgres>,
    checked: &CheckedAppend<'_>,
) -> Result<Option<String>, Status> {
    if !checked.run_id.is_empty() {
        let run_thread: Option<String> =
            sqlx::query_scalar("SELECT thread_id FROM runs WHERE id = $1")
                .bind(checked.run_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(internal)?;
        if run_thread.as_deref() != Some(checked.thread_id) {
            return Err(Status::invalid_argument(
                "run_id does not belong to the thread",
            ));
        }
    }
    if !checked.source_message_id.is_empty() {
        let in_thread: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM messages WHERE id = $1 AND thread_id = $2)",
        )
        .bind(checked.source_message_id)
        .bind(checked.thread_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(internal)?;
        if !in_thread {
            return Err(Status::invalid_argument(
                "source_message_id is not a message of the thread",
            ));
        }
        return Ok(Some(checked.source_message_id.to_owned()));
    }
    if checked.run_id.is_empty() {
        return Ok(None);
    }
    sqlx::query_scalar(
        "SELECT m.id FROM managed_run_terminalization_outbox o
         JOIN messages m ON m.thread_id = $2 AND m.sequence = o.completion_message_sequence
         WHERE o.run_id = $1",
    )
    .bind(checked.run_id)
    .bind(checked.thread_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)
}

/// Whether the thread holds a run whose content was waived from retention (DEC-32). The same
/// predicate as the fork's.
async fn thread_is_waived(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<bool, Status> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE thread_id = $1 AND retention_class = 'zdr_waived')",
    )
    .bind(thread_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)
}

/// List one artifact's revisions newest first, or the head of each artifact in the thread.
pub(crate) async fn list_revisions(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::ListArtifactRevisionsRequest,
) -> Result<pb::ListArtifactRevisionsResponse, Status> {
    let org_id = req.org_id.trim();
    let thread_id = req.thread_id.trim();
    let artifact_id = req.artifact_id.trim();
    if org_id.is_empty() || thread_id.is_empty() {
        return Err(Status::invalid_argument("org_id and thread_id are required"));
    }
    authorize_thread_read(pool, caller, org_id, thread_id).await?;
    let limit = clamp_limit(req.limit);
    // One row past the page says whether more exist without counting them.
    let fetch = i64::from(limit) + 1;
    let mut rows: Vec<RevisionRow> = if artifact_id.is_empty() {
        sqlx::query_as(&format!(
            "SELECT * FROM (
               SELECT DISTINCT ON (artifact_id) {REVISION_COLUMNS} FROM artifact_revisions
               WHERE thread_id = $1 ORDER BY artifact_id, version DESC
             ) heads ORDER BY created_at DESC, artifact_id LIMIT $2"
        ))
        .bind(thread_id)
        .bind(fetch)
        .fetch_all(pool)
        .await
        .map_err(internal)?
    } else {
        sqlx::query_as(&format!(
            "SELECT {REVISION_COLUMNS} FROM artifact_revisions
             WHERE thread_id = $1 AND artifact_id = $2 AND ($3 = 0 OR version < $3)
             ORDER BY version DESC LIMIT $4"
        ))
        .bind(thread_id)
        .bind(artifact_id)
        .bind(to_i32(req.before_version)?)
        .bind(fetch)
        .fetch_all(pool)
        .await
        .map_err(internal)?
    };
    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    Ok(pb::ListArtifactRevisionsResponse {
        revisions: rows
            .into_iter()
            .map(|row| row.into_proto(req.include_content))
            .collect(),
        has_more,
    })
}

/// Read one revision, or the head when the version is 0.
pub(crate) async fn get_revision(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::GetArtifactRevisionRequest,
) -> Result<pb::GetArtifactRevisionResponse, Status> {
    let org_id = req.org_id.trim();
    let thread_id = req.thread_id.trim();
    let artifact_id = req.artifact_id.trim();
    if org_id.is_empty() || thread_id.is_empty() || artifact_id.is_empty() {
        return Err(Status::invalid_argument(
            "org_id, thread_id and artifact_id are required",
        ));
    }
    authorize_thread_read(pool, caller, org_id, thread_id).await?;
    let row: Option<RevisionRow> = sqlx::query_as(&format!(
        "SELECT {REVISION_COLUMNS} FROM artifact_revisions
         WHERE thread_id = $1 AND artifact_id = $2 AND ($3 = 0 OR version = $3)
         ORDER BY version DESC LIMIT 1"
    ))
    .bind(thread_id)
    .bind(artifact_id)
    .bind(to_i32(req.version)?)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let row = row.ok_or_else(|| Status::not_found(REVISION_MISSING))?;
    Ok(pb::GetArtifactRevisionResponse {
        revision: Some(row.into_proto(true)),
    })
}

/// The owner check of a read: the same rule as every other thread read, so a cross-org caller or
/// another person in the org learns nothing about the thread.
async fn authorize_thread_read(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    org_id: &str,
    thread_id: &str,
) -> Result<(), Status> {
    caller.authorize_org(org_id)?;
    let thread = read_thread(pool, thread_id).await?;
    if thread.org_id != org_id {
        return Err(Status::not_found("thread not found"));
    }
    authorize_thread_owner_row(
        caller,
        &thread.org_id,
        &thread.user_id,
        thread.space_id.as_deref().unwrap_or_default(),
        OwnerIntent::Read,
    )
}

/// The columns a revision is read with, in [`RevisionRow`]'s order.
const REVISION_COLUMNS: &str = "id, thread_id, artifact_id, version, kind, title, content, \
     editor_kind, editor_subject_id, run_id, source_message_id, restored_from_version, \
     content_sha256, created_at";

#[derive(Debug, sqlx::FromRow)]
struct RevisionRow {
    id: String,
    thread_id: String,
    artifact_id: String,
    version: i32,
    kind: String,
    title: String,
    content: String,
    editor_kind: String,
    editor_subject_id: String,
    run_id: Option<String>,
    source_message_id: Option<String>,
    restored_from_version: Option<i32>,
    content_sha256: String,
    created_at: DateTime<Utc>,
}

impl RevisionRow {
    fn into_proto(self, include_content: bool) -> pb::ArtifactRevision {
        pb::ArtifactRevision {
            id: self.id,
            thread_id: self.thread_id,
            artifact_id: self.artifact_id,
            version: u32::try_from(self.version).unwrap_or_default(),
            kind: self.kind,
            title: self.title,
            content: if include_content {
                self.content
            } else {
                String::new()
            },
            editor_kind: self.editor_kind,
            editor_subject_id: self.editor_subject_id,
            run_id: self.run_id.unwrap_or_default(),
            source_message_id: self.source_message_id.unwrap_or_default(),
            restored_from_version: self
                .restored_from_version
                .and_then(|version| u32::try_from(version).ok())
                .unwrap_or_default(),
            content_sha256: self.content_sha256,
            created_at: Some(to_proto_timestamp(self.created_at)),
        }
    }
}

async fn find_by_idempotency_key(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
    key: &str,
) -> Result<Option<RevisionRow>, Status> {
    sqlx::query_as(&format!(
        "SELECT {REVISION_COLUMNS} FROM artifact_revisions
         WHERE thread_id = $1 AND idempotency_key = $2"
    ))
    .bind(thread_id)
    .bind(key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)
}

async fn insert_row(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    idempotency_key: Option<String>,
    row: &RevisionRow,
) -> Result<(), Status> {
    sqlx::query(
        "INSERT INTO artifact_revisions
         (id, org_id, thread_id, artifact_id, version, kind, title, content, editor_kind,
          editor_subject_id, run_id, source_message_id, restored_from_version, content_sha256,
          idempotency_key, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
    )
    .bind(&row.id)
    .bind(org_id)
    .bind(&row.thread_id)
    .bind(&row.artifact_id)
    .bind(row.version)
    .bind(&row.kind)
    .bind(&row.title)
    .bind(&row.content)
    .bind(&row.editor_kind)
    .bind(&row.editor_subject_id)
    .bind(&row.run_id)
    .bind(&row.source_message_id)
    .bind(row.restored_from_version)
    .bind(&row.content_sha256)
    .bind(idempotency_key)
    .bind(row.created_at)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn to_i32(version: u32) -> Result<i32, Status> {
    i32::try_from(version).map_err(|_| Status::invalid_argument("version out of range"))
}

fn internal(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORG: &str = "org-artifact-revisions";
    const OWNER: &str = "user-artifact-owner";

    fn append_request(thread: &str, expected: u32) -> pb::AppendArtifactRevisionRequest {
        pb::AppendArtifactRevisionRequest {
            org_id: ORG.to_owned(),
            thread_id: thread.to_owned(),
            artifact_id: "report".to_owned(),
            expected_version: expected,
            kind: "document".to_owned(),
            title: "Report".to_owned(),
            content: format!("text {expected}"),
            editor_kind: "model".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn kind_is_an_open_string_bounded_in_shape() {
        for kind in ["document", "code", "html", "deck", "slide_v2", "x-1"] {
            assert!(valid_kind(kind), "{kind}");
        }
        for kind in ["", "Document", "-deck", "deck kind", "a/b", &"k".repeat(65)] {
            assert!(!valid_kind(kind), "{kind}");
        }
    }

    #[test]
    fn append_shape_is_checked_before_anything_is_read() {
        assert!(check_append(&append_request("t", 0)).is_ok());

        let mut missing = append_request("", 0);
        missing.artifact_id.clear();
        assert_eq!(
            check_append(&missing).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );

        let mut editor = append_request("t", 0);
        editor.editor_kind = "admin".to_owned();
        assert!(check_append(&editor).is_err());

        let mut long = append_request("t", 0);
        long.content = "x".repeat(MAX_CONTENT_CHARS + 1);
        assert!(check_append(&long).is_err());
        long.content = "x".repeat(MAX_CONTENT_CHARS);
        assert!(check_append(&long).is_ok());
    }

    #[test]
    fn only_a_restore_names_the_version_it_restored() {
        let mut restore = append_request("t", 3);
        restore.editor_kind = "restore".to_owned();
        assert!(check_append(&restore).is_err(), "a restore without a source");
        restore.restored_from_version = 1;
        assert!(check_append(&restore).is_ok());

        let mut model = append_request("t", 3);
        model.restored_from_version = 1;
        assert!(check_append(&model).is_err(), "a model revision restores nothing");
    }

    #[test]
    fn a_published_version_must_move_past_the_expected_head() {
        let mut request = append_request("t", 2);
        request.version = 2;
        assert!(check_append(&request).is_err());
        request.version = 4;
        assert!(check_append(&request).is_ok(), "gaps are allowed (OD-10)");
        assert_eq!(next_version(2, 4), 4);
        assert_eq!(next_version(2, 0), 3);
    }

    #[test]
    fn stale_expected_version_is_revision_mismatch() {
        assert!(check_head(3, 3).is_ok());
        let stale = check_head(4, 3).unwrap_err();
        assert_eq!(stale.code(), tonic::Code::Aborted);
        assert_eq!(stale.message(), REVISION_MISMATCH);
        assert_eq!(check_head(1, 0).unwrap_err().message(), REVISION_MISMATCH);
    }

    #[test]
    fn caps_refuse_the_201st_revision_and_the_thread_byte_budget() {
        assert!(check_caps(MAX_REVISIONS_PER_ARTIFACT - 1, 0, 10).is_ok());
        let refused = check_caps(MAX_REVISIONS_PER_ARTIFACT, 0, 10).unwrap_err();
        assert_eq!(refused.code(), tonic::Code::ResourceExhausted);
        assert_eq!(refused.message(), REVISION_LIMIT);
        assert!(check_caps(0, MAX_THREAD_REVISION_BYTES - 10, 10).is_ok());
        assert_eq!(
            check_caps(0, MAX_THREAD_REVISION_BYTES - 10, 11)
                .unwrap_err()
                .message(),
            REVISION_LIMIT
        );
    }

    #[test]
    fn space_thread_refuses_user_editor() {
        assert!(check_space_editor(true, EditorKind::Model).is_ok());
        assert_eq!(
            check_space_editor(true, EditorKind::User)
                .unwrap_err()
                .message(),
            SPACE_THREAD_EDIT
        );
        assert!(check_space_editor(true, EditorKind::Restore).is_err());
        assert!(check_space_editor(false, EditorKind::User).is_ok());
    }

    #[test]
    fn zdr_and_waived_credentials_are_refused_on_every_rpc() {
        assert!(refuse_retention_exempt(&VerifiedIdentity::user_for_test(ORG, OWNER)).is_ok());
        let zdr = VerifiedIdentity::user_for_test_with_zdr(ORG, OWNER, true);
        assert_eq!(
            refuse_retention_exempt(&zdr).unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
    }

    #[test]
    fn list_limit_defaults_and_clamps() {
        assert_eq!(clamp_limit(0), DEFAULT_LIST_LIMIT);
        assert_eq!(clamp_limit(5), 5);
        assert_eq!(clamp_limit(500), MAX_LIST_LIMIT);
    }

    #[test]
    fn content_hash_is_lowercase_hex_sha256() {
        let hash = content_sha256("abc");
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_switch_is_off_unless_it_reads_true() {
        assert!(!mp_contracts::loop_fold::switch_from(None));
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("on")));
    }

    // The database tests below run with `--ignored` against a disposable Postgres, as the other
    // session-core database tests do. They call the store functions directly, so the switch is not
    // needed; `rpc_is_refused_while_the_switch_is_off` tests the switch.

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_thread(pool: &PgPool) -> String {
        let thread = new_ulid();
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(&thread)
        .bind(ORG)
        .bind(OWNER)
        .execute(pool)
        .await
        .unwrap();
        thread
    }

    async fn rows(pool: &PgPool, thread: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM artifact_revisions WHERE thread_id = $1")
            .bind(thread)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn owner() -> VerifiedIdentity {
        VerifiedIdentity::user_for_test(ORG, OWNER)
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn append_assigns_next_version_under_cas() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        let first = append_revision(&pool, &owner(), append_request(&thread, 0))
            .await
            .unwrap();
        assert_eq!(first.outcome, pb::AppendOutcome::Appended as i32);
        let first = first.revision.unwrap();
        assert_eq!(first.version, 1);
        assert_eq!(first.editor_kind, "model");
        assert_eq!(first.editor_subject_id, OWNER);
        let second = append_revision(&pool, &owner(), append_request(&thread, 1))
            .await
            .unwrap()
            .revision
            .unwrap();
        assert_eq!(second.version, 2);
        let mut published = append_request(&thread, 2);
        published.version = 5;
        let gap = append_revision(&pool, &owner(), published)
            .await
            .unwrap()
            .revision
            .unwrap();
        assert_eq!(gap.version, 5, "the gateway's published number is kept");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn stale_expected_version_is_revision_mismatch_in_the_store() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        append_revision(&pool, &owner(), append_request(&thread, 0))
            .await
            .unwrap();
        let stale = append_revision(&pool, &owner(), append_request(&thread, 0))
            .await
            .unwrap_err();
        assert_eq!(stale.code(), tonic::Code::Aborted);
        assert_eq!(stale.message(), REVISION_MISMATCH);
        assert_eq!(rows(&pool, &thread).await, 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn idempotent_append_is_duplicate() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        let mut request = append_request(&thread, 0);
        request.idempotency_key = "req-1".to_owned();
        let first = append_revision(&pool, &owner(), request.clone()).await.unwrap();
        let again = append_revision(&pool, &owner(), request).await.unwrap();
        assert_eq!(again.outcome, pb::AppendOutcome::Duplicate as i32);
        assert_eq!(again.revision.unwrap().id, first.revision.unwrap().id);
        assert_eq!(rows(&pool, &thread).await, 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn concurrent_appends_one_wins() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        let caller = owner();
        let (a, b) = tokio::join!(
            append_revision(&pool, &caller, append_request(&thread, 0)),
            append_revision(&pool, &caller, append_request(&thread, 0)),
        );
        assert_eq!(
            usize::from(a.is_ok()) + usize::from(b.is_ok()),
            1,
            "exactly one append wins the head"
        );
        let loser = a.err().or(b.err()).unwrap();
        assert_eq!(loser.message(), REVISION_MISMATCH);
        assert_eq!(rows(&pool, &thread).await, 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn zdr_credential_refused() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        let mut request = Request::new(append_request(&thread, 0));
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test_with_zdr(ORG, OWNER, true));
        let refused = ArtifactRevisionGrpc { pool: pool.clone() }
            .append_artifact_revision(request)
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(rows(&pool, &thread).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn waived_thread_refused() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, metadata,
                               retention_class, waiver_ack_id, waiver_connection_id)
             VALUES ($1, $2, 'g', $3, $4, 'completed', '{}'::jsonb,
                     'zdr_waived', 'ack-1', 'conn-1')",
        )
        .bind(new_ulid())
        .bind(&thread)
        .bind(ORG)
        .bind(OWNER)
        .execute(&pool)
        .await
        .unwrap();
        let refused = append_revision(&pool, &owner(), append_request(&thread, 0))
            .await
            .unwrap_err();
        assert_eq!(refused.message(), WAIVED_THREAD);
        assert_eq!(rows(&pool, &thread).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn space_thread_refuses_user_editor_in_the_store() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        sqlx::query(
            "UPDATE threads SET space_id = 'space-art', origin = 'space', space_decision_ref = 'dec-t', recipient_audience_ref = 'aud-t', privacy_policy_ref = 'pp-t', resource_authorization_ref = 'ra-t', authority_revision = 1, recipient_audience_revision = 1, recipient_audience_hash = 'test-audience' WHERE id = $1",
        )
        .bind(&thread)
        .execute(&pool)
        .await
        .unwrap();
        let mut edit = append_request(&thread, 0);
        edit.editor_kind = "user".to_owned();
        let refused = append_revision(&pool, &owner(), edit).await.unwrap_err();
        assert_eq!(refused.message(), SPACE_THREAD_EDIT);
        append_revision(&pool, &owner(), append_request(&thread, 0))
            .await
            .expect("a model revision is stored for a Space thread");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn caps_refuse_201st_revision() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        sqlx::query(
            "INSERT INTO artifact_revisions
             (id, org_id, thread_id, artifact_id, version, kind, title, content, editor_kind,
              editor_subject_id, content_sha256)
             SELECT $1 || '-' || g::text, $2, $1, 'report', g, 'document', 'Report', 'x', 'model',
                    $3, repeat('0', 64)
             FROM generate_series(1, $4) AS g",
        )
        .bind(&thread)
        .bind(ORG)
        .bind(OWNER)
        .bind(i32::try_from(MAX_REVISIONS_PER_ARTIFACT).unwrap())
        .execute(&pool)
        .await
        .unwrap();
        let refused = append_revision(
            &pool,
            &owner(),
            append_request(&thread, u32::try_from(MAX_REVISIONS_PER_ARTIFACT).unwrap()),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.message(), REVISION_LIMIT);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn restore_of_a_missing_version_is_revision_missing() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        append_revision(&pool, &owner(), append_request(&thread, 0))
            .await
            .unwrap();
        let mut restore = append_request(&thread, 1);
        restore.editor_kind = "restore".to_owned();
        restore.restored_from_version = 7;
        let refused = append_revision(&pool, &owner(), restore).await.unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), REVISION_MISSING);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn another_person_may_not_append_or_read() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        let other = VerifiedIdentity::user_for_test(ORG, "user-artifact-other");
        assert!(append_revision(&pool, &other, append_request(&thread, 0))
            .await
            .is_err());
        let list = pb::ListArtifactRevisionsRequest {
            org_id: ORG.to_owned(),
            thread_id: thread.clone(),
            ..Default::default()
        };
        assert!(list_revisions(&pool, &other, list).await.is_err());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn list_pages_newest_first_and_heads_cover_every_artifact() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        for expected in 0..3 {
            append_revision(&pool, &owner(), append_request(&thread, expected))
                .await
                .unwrap();
        }
        let mut other = append_request(&thread, 0);
        other.artifact_id = "notes".to_owned();
        append_revision(&pool, &owner(), other).await.unwrap();

        let page = list_revisions(
            &pool,
            &owner(),
            pb::ListArtifactRevisionsRequest {
                org_id: ORG.to_owned(),
                thread_id: thread.clone(),
                artifact_id: "report".to_owned(),
                limit: 2,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let versions: Vec<u32> = page.revisions.iter().map(|r| r.version).collect();
        assert_eq!(versions, [3, 2]);
        assert!(page.has_more);
        assert!(page.revisions.iter().all(|r| r.content.is_empty()));

        let heads = list_revisions(
            &pool,
            &owner(),
            pb::ListArtifactRevisionsRequest {
                org_id: ORG.to_owned(),
                thread_id: thread.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(heads.revisions.len(), 2);
        assert!(!heads.has_more);

        let head = get_revision(
            &pool,
            &owner(),
            pb::GetArtifactRevisionRequest {
                org_id: ORG.to_owned(),
                thread_id: thread,
                artifact_id: "report".to_owned(),
                version: 0,
            },
        )
        .await
        .unwrap()
        .revision
        .unwrap();
        assert_eq!(head.version, 3);
        assert_eq!(head.content, "text 2");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn delete_thread_erases_revisions() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        append_revision(&pool, &owner(), append_request(&thread, 0))
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        crate::grpc::delete_thread_rows(&mut tx, &thread).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(rows(&pool, &thread).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn org_erase_erases_revisions() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        append_revision(&pool, &owner(), append_request(&thread, 0))
            .await
            .unwrap();
        // The rows go with their threads (ON DELETE CASCADE); the purge summary has no count for them.
        crate::gdpr::purge_organization_data(&pool, ORG, None)
            .await
            .unwrap();
        assert_eq!(rows(&pool, &thread).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn rpc_is_refused_while_the_switch_is_off() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        // The suite runs with the switch unset, which is the default this test proves.
        let mut request = Request::new(append_request(&thread, 0));
        request.extensions_mut().insert(owner());
        let refused = ArtifactRevisionGrpc { pool: pool.clone() }
            .append_artifact_revision(request)
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::Unavailable);
        assert_eq!(refused.message(), DISABLED);
    }
}
