//! `ExportThread`: the owner's bounded copy of one thread, to take off the platform (Wave 3 EP-12
//! S9, decisions D7 and D8).
//!
//! An export carries the turns of the thread's active branch with the latest edit or deletion
//! folded in, the persisted text of their artifacts, their citations, the rest of their metadata
//! and a summary of each run. Binaries are never part of it. The `REDACTED` profile is the replay
//! view's input: it keeps the turn text and the artifacts and drops tool arguments and results,
//! connector payloads, citation snippets and URL query strings, and replaces attachment names with
//! `attachment-N`.
//!
//! Whether the thread may leave at all is decided in the same `REPEATABLE READ, READ ONLY`
//! transaction as the read, so a run cannot land between the decision and the copy:
//!
//! * a zero-retention or waived caller is refused with `zdr_feature_not_supported`
//!   (`conversation_export`): `authorize_operation` lets such a caller read, and a waived
//!   bearer's `zdr` claim is false, so both are named here;
//! * a thread that belongs to a Space, or that holds any run whose retention class is not
//!   `persistent`, is refused with `thread_not_exportable` and its reason in the
//!   [`ERROR_REASON_TRAILER`]. A transcript missing a waived or zero-retention turn is never
//!   exported as if it were whole (DEC-40);
//! * another person's thread reads as not found, and a service principal is refused, because an
//!   export is the owner's own act.
//!
//! Past [`MAX_EXPORT_MESSAGES`] turns, [`MAX_EXPORT_RUNS`] runs or [`MAX_EXPORT_TEXT_BYTES`] of
//! text the export stops and says so with `truncated`. The switch is [`ENABLED_ENV`], default off;
//! while it is off the RPC answers `unavailable`.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    self as pb, thread_export_service_server::ThreadExportService,
};
use mp_contracts::run_origin::RunOrigin;
use mp_contracts::zdr_refusal::Refusal;
use serde_json::{Map, Value};
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Instant;
use tonic::metadata::MetadataValue;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, identity, VerifiedIdentity};
use crate::grpc::{record_metrics, to_proto_timestamp};
use crate::run_phase::ERROR_CODE_TRAILER;

/// The environment switch for the export. Default off, read by the strict parser the other
/// switches share, so only the literal `true` turns it on.
pub(crate) const ENABLED_ENV: &str = "SESSION_THREAD_EXPORT_ENABLED";

/// The most turns one export carries (D7).
pub(crate) const MAX_EXPORT_MESSAGES: usize = 2_000;

/// The most run summaries one export carries. A run per turn is the common shape, so the run cap
/// matches the turn cap.
pub(crate) const MAX_EXPORT_RUNS: usize = 2_000;

/// The most text one export carries, counted over each turn's content and its JSON fields (D7).
pub(crate) const MAX_EXPORT_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// The typed code of a thread that may not leave, on [`ERROR_CODE_TRAILER`].
pub(crate) const NOT_EXPORTABLE_CODE: &str = "thread_not_exportable";

/// gRPC metadata key carrying why a thread may not leave; the gateway puts it in the typed
/// envelope's `details.reason`.
pub(crate) const ERROR_REASON_TRAILER: &str = "x-mp-error-reason";

/// The fields of a stored artifact an export carries. Provenance, hashes and internal flags stay
/// in the plane.
const ARTIFACT_FIELDS: &[&str] = &["id", "kind", "title", "content", "version"];

/// The fields of a stored attachment an export carries. The body is a binary and never leaves.
const ATTACHMENT_FIELDS: &[&str] = &["name", "kind", "mime_type", "media_type", "size_bytes"];

/// The fields of a web or knowledge citation the redacted profile keeps; the URL loses its query
/// string and fragment. The snippet and passage are the source's own text and are dropped.
const REDACTED_CITATION_FIELDS: &[&str] = &[
    "id",
    "title",
    "url",
    "number",
    "origin",
    "source_group_id",
    "source_group_label",
];

/// The fields of a connector citation the redacted profile keeps. Its URL and id name an item in
/// the owner's connected account, which is a connector payload, so only the label survives.
const REDACTED_CONNECTOR_CITATION_FIELDS: &[&str] =
    &["title", "number", "origin", "source_group_label"];

/// Read where it is used, as the fork's switch is: the RPC holds no switch of its own.
pub(crate) fn thread_export_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The export RPC. It holds the pool and nothing else; the rules are in this module.
pub struct ThreadExportGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl ThreadExportService for ThreadExportGrpc {
    async fn export_thread(
        &self,
        request: Request<pb::ExportThreadRequest>,
    ) -> Result<Response<pb::ExportThreadResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            if !thread_export_enabled() {
                return Err(Status::unavailable("thread_export_disabled"));
            }
            export_thread(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("export_thread", started, result.is_ok());
        result
    }
}

/// Which copy of the thread the caller asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    Full,
    Redacted,
}

/// A caller always chooses: an unset profile is refused rather than read as either one.
fn profile_from(raw: i32) -> Result<Profile, Status> {
    match pb::ExportProfile::try_from(raw) {
        Ok(pb::ExportProfile::Full) => Ok(Profile::Full),
        Ok(pb::ExportProfile::Redacted) => Ok(Profile::Redacted),
        _ => Err(Status::invalid_argument(
            "profile must be EXPORT_PROFILE_FULL or EXPORT_PROFILE_REDACTED",
        )),
    }
}

/// Why a thread may not leave the platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotExportable {
    /// A run of the thread was zero-retention or waived, so its transcript is not whole.
    NonPersistentRun,
    /// The thread belongs to a Space, whose audience and history have their own authority.
    SpaceThread,
}

impl NotExportable {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NonPersistentRun => "non_persistent_run",
            Self::SpaceThread => "space_thread",
        }
    }

    /// The typed refusal: `failed_precondition`, the code and the reason on their trailers.
    pub(crate) fn status(self) -> Status {
        let mut status = Status::failed_precondition(NOT_EXPORTABLE_CODE);
        status.metadata_mut().insert(
            ERROR_CODE_TRAILER,
            MetadataValue::from_static(NOT_EXPORTABLE_CODE),
        );
        status.metadata_mut().insert(
            ERROR_REASON_TRAILER,
            MetadataValue::from_static(self.as_str()),
        );
        status
    }
}

/// The thread's row as the export reads it.
struct ExportSource {
    org_id: String,
    user_id: String,
    space_id: Option<String>,
    title: Option<String>,
    created_at: DateTime<Utc>,
}

/// One stored turn of the active branch, its revision already folded in.
#[derive(sqlx::FromRow)]
struct MessageRow {
    id: String,
    role: String,
    content: String,
    metadata: Option<Value>,
    created_at: DateTime<Utc>,
}

/// One run of the thread, as the summary reads it.
#[derive(sqlx::FromRow)]
struct RunRow {
    id: String,
    status: String,
    metadata: Value,
    created_at: DateTime<Utc>,
}

/// Decide and read one owner's export. The switch and the session scope are the RPC's to check,
/// so the export itself can be tested without either.
pub(crate) async fn export_thread(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::ExportThreadRequest,
) -> Result<pb::ExportThreadResponse, Status> {
    refuse_retention_exempt(caller)?;
    let owner = caller
        .user_id()
        .ok_or_else(|| Status::permission_denied("an export is made by the thread's owner"))?;
    let thread_id = req.thread_id.trim();
    if thread_id.is_empty() {
        return Err(Status::invalid_argument("thread_id is required"));
    }
    let profile = profile_from(req.profile)?;

    let mut tx = pool.begin().await.map_err(internal)?;
    // The decision and the read see one snapshot: a run that starts while the export reads is
    // either in both or in neither.
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let source = read_source(&mut tx, thread_id).await?;
    if source.org_id != caller.org_id() || source.user_id != owner {
        return Err(Status::not_found("thread not found"));
    }
    let in_space = source
        .space_id
        .as_deref()
        .is_some_and(|space| !space.trim().is_empty());
    if in_space {
        return Err(NotExportable::SpaceThread.status());
    }
    if holds_non_persistent_run(&mut tx, thread_id).await? {
        return Err(NotExportable::NonPersistentRun.status());
    }

    let message_rows = active_branch(&mut tx, thread_id).await?;
    let run_rows = run_summaries(&mut tx, thread_id).await?;
    tx.commit().await.map_err(internal)?;

    let more_turns = message_rows.len() > MAX_EXPORT_MESSAGES;
    let mut attachments = 0_usize;
    let messages: Vec<pb::ExportedMessage> = message_rows
        .into_iter()
        .take(MAX_EXPORT_MESSAGES)
        .map(|row| exported_message(row, profile, &mut attachments))
        .collect();
    let (messages, cut_by_text) = cap_text(messages);
    let more_runs = run_rows.len() > MAX_EXPORT_RUNS;
    let runs = run_rows
        .into_iter()
        .take(MAX_EXPORT_RUNS)
        .map(exported_run)
        .collect();
    Ok(pb::ExportThreadResponse {
        thread_id: thread_id.to_owned(),
        title: source.title.unwrap_or_default(),
        created_at: Some(to_proto_timestamp(source.created_at)),
        messages,
        runs,
        truncated: more_turns || cut_by_text || more_runs,
    })
}

/// An export is a copy that leaves the platform, which a zero-retention or waived credential may
/// never make (DEC-32: a waived run counts as zero-retention for every new copy).
fn refuse_retention_exempt(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() || caller.zdr_waiver().is_some() {
        return Err(Refusal::feature_not_supported("conversation_export")
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    Ok(())
}

async fn read_source(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<ExportSource, Status> {
    let row: Option<(
        String,
        String,
        Option<String>,
        Option<String>,
        DateTime<Utc>,
    )> = sqlx::query_as(
        "SELECT org_id, user_id, space_id, presentation_title, created_at
             FROM threads WHERE id = $1",
    )
    .bind(thread_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;
    let (org_id, user_id, space_id, title, created_at) =
        row.ok_or_else(|| Status::not_found("thread not found"))?;
    Ok(ExportSource {
        org_id,
        user_id,
        space_id,
        title,
        created_at,
    })
}

/// Whether any run of the thread was not persistent: zero-retention or waived (D8).
async fn holds_non_persistent_run(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<bool, Status> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE thread_id = $1 AND retention_class <> 'persistent')",
    )
    .bind(thread_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)
}

/// The active branch, oldest first, with the latest revision folded in as the transcript read
/// folds it. A deleted turn has nothing left to export and is skipped. One row past the cap is
/// read so that a longer branch is recognised without reading the rest.
async fn active_branch(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<Vec<MessageRow>, Status> {
    let limit = i64::try_from(MAX_EXPORT_MESSAGES + 1)
        .map_err(|_| Status::internal("export cap out of range"))?;
    // No audience ceiling: the owner of a thread outside a Space reads every revision.
    let sql = format!(
        "SELECT m.id::text AS id, m.role, {fold_content} AS content,
                {fold_metadata} AS metadata, m.created_at
         FROM messages m
         {fold_join}
         WHERE m.thread_id = $1 AND m.on_active_path AND m.post_kind <> 'post'
           AND {not_deleted}
         ORDER BY m.sequence
         LIMIT $2",
        fold_content = crate::space::revisions::FOLD_CONTENT_SQL,
        fold_metadata = crate::space::revisions::FOLD_METADATA_SQL,
        fold_join = crate::space::revisions::fold_join_sql("NULL::BIGINT"),
        not_deleted = crate::space::revisions::FOLD_NOT_DELETED_SQL,
    );
    sqlx::query_as::<_, MessageRow>(&sql)
        .bind(thread_id)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
        .map_err(internal)
}

/// Every run of the thread, oldest first, one past the cap.
async fn run_summaries(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<Vec<RunRow>, Status> {
    let limit = i64::try_from(MAX_EXPORT_RUNS + 1)
        .map_err(|_| Status::internal("export cap out of range"))?;
    sqlx::query_as::<_, RunRow>(
        "SELECT id, status, COALESCE(metadata, '{}'::jsonb) AS metadata, created_at
         FROM runs WHERE thread_id = $1
         ORDER BY created_at, id
         LIMIT $2",
    )
    .bind(thread_id)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal)
}

/// One turn as the profile shapes it. `attachments` numbers the attachments across the whole
/// export, so a redacted name is unique within it.
fn exported_message(
    row: MessageRow,
    profile: Profile,
    attachments: &mut usize,
) -> pb::ExportedMessage {
    let mut metadata = match row.metadata {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    let artifacts = metadata
        .remove("artifacts")
        .map(|value| text_artifacts(&value));
    let citations = metadata
        .remove("citations")
        .map(|value| shaped_citations(&value, profile));
    let rest = shaped_metadata(metadata, profile, attachments);
    pb::ExportedMessage {
        message_id: row.id,
        role: row.role,
        content: row.content,
        created_at: Some(to_proto_timestamp(row.created_at)),
        artifacts_json: json_array_or_empty(artifacts),
        citations_json: json_array_or_empty(citations),
        metadata_json: json_or_empty((!rest.is_empty()).then_some(Value::Object(rest))),
    }
}

fn exported_run(row: RunRow) -> pb::ExportedRun {
    pb::ExportedRun {
        run_id: row.id,
        status: row.status,
        // An origin outside the closed vocabulary is not a label to repeat: it reads as empty.
        origin: RunOrigin::from_metadata(&row.metadata)
            .map(|origin| origin.as_str().to_owned())
            .unwrap_or_default(),
        model: row
            .metadata
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        input_tokens: 0,
        output_tokens: 0,
        created_at: Some(to_proto_timestamp(row.created_at)),
    }
}

/// The artifacts that carry persisted text, cut to [`ARTIFACT_FIELDS`]. An artifact without text
/// content is a binary or a reference, which an export never carries.
fn text_artifacts(value: &Value) -> Vec<Value> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(Value::as_object)
        .filter(|artifact| artifact.get("content").is_some_and(Value::is_string))
        .map(|artifact| Value::Object(pick(artifact, ARTIFACT_FIELDS)))
        .collect()
}

/// The citations as the profile shapes them: as stored under `FULL`, reduced under `REDACTED`.
fn shaped_citations(value: &Value, profile: Profile) -> Vec<Value> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(Value::as_object)
        .map(|citation| match profile {
            Profile::Full => Value::Object(citation.clone()),
            Profile::Redacted => Value::Object(redacted_citation(citation)),
        })
        .collect()
}

fn redacted_citation(citation: &Map<String, Value>) -> Map<String, Value> {
    let connector = citation.get("origin").and_then(Value::as_str) == Some("connector");
    if connector {
        return pick(citation, REDACTED_CONNECTOR_CITATION_FIELDS);
    }
    let mut kept = pick(citation, REDACTED_CITATION_FIELDS);
    if let Some(Value::String(url)) = kept.get_mut("url") {
        *url = without_query(url).to_owned();
    }
    kept
}

/// A URL without its query string and fragment, where tokens and search terms ride.
fn without_query(url: &str) -> &str {
    url.find(['?', '#']).map_or(url, |cut| &url[..cut])
}

/// The rest of a turn's metadata. `FULL` keeps every key, with attachments cut to
/// [`ATTACHMENT_FIELDS`]; `REDACTED` keeps the attachments alone, renamed, so no tool argument,
/// tool result, evidence snippet or connector payload survives.
fn shaped_metadata(
    mut metadata: Map<String, Value>,
    profile: Profile,
    counter: &mut usize,
) -> Map<String, Value> {
    let attachments = metadata
        .remove("attachments")
        .map(|value| shaped_attachments(&value, profile, counter))
        .filter(|items| !items.is_empty());
    let mut shaped = match profile {
        Profile::Full => metadata,
        Profile::Redacted => Map::new(),
    };
    if let Some(items) = attachments {
        shaped.insert("attachments".to_owned(), Value::Array(items));
    }
    shaped
}

fn shaped_attachments(value: &Value, profile: Profile, counter: &mut usize) -> Vec<Value> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(Value::as_object)
        .map(|attachment| {
            let mut kept = pick(attachment, ATTACHMENT_FIELDS);
            if profile == Profile::Redacted {
                *counter += 1;
                kept.insert(
                    "name".to_owned(),
                    Value::String(format!("attachment-{counter}")),
                );
            }
            Value::Object(kept)
        })
        .collect()
}

/// The named fields of `source`, in the order named.
fn pick(source: &Map<String, Value>, fields: &[&str]) -> Map<String, Value> {
    fields
        .iter()
        .filter_map(|field| {
            source
                .get(*field)
                .map(|value| ((*field).to_owned(), value.clone()))
        })
        .collect()
}

fn json_or_empty(value: Option<Value>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

/// A list as JSON, or empty when there is nothing in it.
fn json_array_or_empty(items: Option<Vec<Value>>) -> String {
    json_or_empty(items.filter(|items| !items.is_empty()).map(Value::Array))
}

/// The oldest turns that fit in [`MAX_EXPORT_TEXT_BYTES`], and whether any turn was cut. The
/// export stops at the first turn that does not fit rather than skipping it, so what is returned
/// is always an unbroken start of the conversation.
fn cap_text(messages: Vec<pb::ExportedMessage>) -> (Vec<pb::ExportedMessage>, bool) {
    let total = messages.len();
    let mut used = 0_usize;
    let kept: Vec<pb::ExportedMessage> = messages
        .into_iter()
        .take_while(|message| {
            used = used.saturating_add(text_bytes(message));
            used <= MAX_EXPORT_TEXT_BYTES
        })
        .collect();
    let cut = kept.len() < total;
    (kept, cut)
}

fn text_bytes(message: &pb::ExportedMessage) -> usize {
    message.content.len()
        + message.artifacts_json.len()
        + message.citations_json.len()
        + message.metadata_json.len()
}

fn internal(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_ids::new_ulid;
    use serde_json::json;

    const ORG: &str = "org-export";
    const OWNER: &str = "user-export-owner";
    const OTHER: &str = "user-export-other";

    fn row(content: &str, metadata: Value) -> MessageRow {
        MessageRow {
            id: "m1".to_owned(),
            role: "assistant".to_owned(),
            content: content.to_owned(),
            metadata: Some(metadata),
            created_at: Utc::now(),
        }
    }

    fn stored_metadata() -> Value {
        json!({
            "tools": [{"name": "search", "arguments": {"q": "secret plan"}, "result": "tool output"}],
            "evidence": [{"snippet": "connector body"}],
            "artifacts": [
                {"id": "a1", "kind": "document", "title": "Plan", "content": "text", "version": 2, "provenance": "x"},
                {"id": "a2", "kind": "image", "title": "Chart", "blob_ref": "s3://bucket/key"}
            ],
            "citations": [
                {"id": "c1", "title": "Web", "url": "https://example.test/page?token=abc#frag", "snippet": "quoted", "origin": "web", "number": 1},
                {"id": "conn-1", "title": "Invoice 42", "url": "https://outlook.office.com/mail/id/m1", "snippet": "Ada", "origin": "connector", "source_group_label": "Microsoft 365"}
            ],
            "attachments": [{"name": "salary.xlsx", "mime_type": "application/vnd.ms-excel", "body": "QUJD"}]
        })
    }

    fn parsed(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn redacted_profile_drops_tool_payloads() {
        let mut counter = 0;
        let stored = row("the answer", stored_metadata());
        let message = exported_message(stored, Profile::Redacted, &mut counter);
        assert_eq!(message.content, "the answer", "the turn text is kept");

        let metadata = parsed(&message.metadata_json);
        assert_eq!(
            metadata,
            json!({"attachments": [{"name": "attachment-1", "mime_type": "application/vnd.ms-excel"}]}),
            "tool arguments, results and evidence are dropped and the attachment is renamed"
        );
        for leaked in [
            "secret plan",
            "tool output",
            "connector body",
            "salary.xlsx",
            "QUJD",
        ] {
            let fields = [
                &message.metadata_json,
                &message.citations_json,
                &message.artifacts_json,
            ];
            for field in fields {
                assert!(
                    !field.contains(leaked),
                    "{leaked} must not survive the redacted profile"
                );
            }
        }

        let citations = parsed(&message.citations_json);
        assert_eq!(citations[0]["url"], "https://example.test/page");
        assert!(citations[0].get("snippet").is_none());
        assert_eq!(
            citations[1],
            json!({"title": "Invoice 42", "origin": "connector", "source_group_label": "Microsoft 365"}),
            "a connector citation keeps its label only"
        );

        let artifacts = parsed(&message.artifacts_json);
        assert_eq!(
            artifacts,
            json!([{"id": "a1", "kind": "document", "title": "Plan", "content": "text", "version": 2}]),
            "text artifacts are kept, binaries and provenance are not"
        );
    }

    #[test]
    fn full_profile_keeps_tool_steps_but_never_an_attachment_body() {
        let mut counter = 0;
        let stored = row("the answer", stored_metadata());
        let message = exported_message(stored, Profile::Full, &mut counter);
        let metadata = parsed(&message.metadata_json);
        assert_eq!(metadata["tools"][0]["arguments"]["q"], "secret plan");
        assert_eq!(metadata["attachments"][0]["name"], "salary.xlsx");
        assert!(
            !message.metadata_json.contains("QUJD"),
            "a binary body never leaves"
        );
        let citations = parsed(&message.citations_json);
        assert_eq!(
            citations[0]["url"],
            "https://example.test/page?token=abc#frag"
        );
        assert_eq!(counter, 0, "the full profile renames nothing");
    }

    #[test]
    fn redacted_attachment_names_are_numbered_across_the_export() {
        let mut counter = 0;
        let one = json!({"attachments": [{"name": "a.pdf"}, {"name": "b.pdf"}]});
        let two = json!({"attachments": [{"name": "c.pdf"}]});
        exported_message(row("one", one), Profile::Redacted, &mut counter);
        let second = exported_message(row("two", two), Profile::Redacted, &mut counter);
        assert_eq!(
            parsed(&second.metadata_json)["attachments"][0]["name"],
            "attachment-3"
        );
    }

    #[test]
    fn a_turn_without_metadata_exports_empty_json_fields() {
        let mut counter = 0;
        let message = exported_message(row("hi", json!({})), Profile::Full, &mut counter);
        assert!(message.artifacts_json.is_empty());
        assert!(message.citations_json.is_empty());
        assert!(message.metadata_json.is_empty());
    }

    #[test]
    fn caps_mark_truncated() {
        let turn = |bytes: usize| pb::ExportedMessage {
            content: "x".repeat(bytes),
            ..Default::default()
        };
        let (kept, cut) = cap_text(vec![turn(10), turn(20)]);
        assert_eq!((kept.len(), cut), (2, false));

        let half = MAX_EXPORT_TEXT_BYTES / 2;
        let (kept, cut) = cap_text(vec![turn(half), turn(half), turn(1), turn(1)]);
        assert_eq!(
            kept.len(),
            2,
            "the export stops at the first turn that does not fit"
        );
        assert!(cut);
    }

    #[test]
    fn an_unset_profile_is_refused() {
        assert_eq!(
            profile_from(0).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(profile_from(1).unwrap(), Profile::Full);
        assert_eq!(profile_from(2).unwrap(), Profile::Redacted);
        assert!(profile_from(99).is_err());
    }

    #[test]
    fn a_refused_thread_names_its_reason_on_the_trailers() {
        let status = NotExportable::NonPersistentRun.status();
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(status.message(), NOT_EXPORTABLE_CODE);
        let trailer = |key: &str| {
            status
                .metadata()
                .get(key)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        assert_eq!(
            trailer(ERROR_CODE_TRAILER).as_deref(),
            Some(NOT_EXPORTABLE_CODE)
        );
        assert_eq!(
            trailer(ERROR_REASON_TRAILER).as_deref(),
            Some("non_persistent_run")
        );
        assert_eq!(NotExportable::SpaceThread.as_str(), "space_thread");
    }

    #[test]
    fn zero_retention_and_waived_callers_are_refused_with_the_export_key() {
        for caller in [
            VerifiedIdentity::user_for_test_with_zdr(ORG, OWNER, true),
            VerifiedIdentity::user_for_test_with_waiver(ORG, OWNER, "conn-1", "ack-1"),
        ] {
            let refused = refuse_retention_exempt(&caller).unwrap_err();
            assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
            assert_eq!(
                Refusal::from_status(&refused),
                Some(Refusal::feature_not_supported("conversation_export"))
            );
        }
        assert!(refuse_retention_exempt(&VerifiedIdentity::user_for_test(ORG, OWNER)).is_ok());
    }

    #[test]
    fn an_origin_outside_the_vocabulary_reads_as_empty() {
        let run = |metadata: Value| RunRow {
            id: "r1".to_owned(),
            status: "completed".to_owned(),
            metadata,
            created_at: Utc::now(),
        };
        assert_eq!(exported_run(run(json!({}))).origin, "chat");
        assert_eq!(
            exported_run(run(json!({"origin": "api", "model": "m-1"}))).model,
            "m-1"
        );
        assert_eq!(exported_run(run(json!({"origin": "Scheduled"}))).origin, "");
    }

    #[test]
    fn the_switch_is_off_unless_it_reads_true() {
        assert!(!mp_contracts::loop_fold::switch_from(None));
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("on")));
    }

    // The database tests below run with `--ignored` against a disposable Postgres, as the other
    // session-core database tests do. They call `export_thread` directly, so the switch is not
    // needed; the switch is tested by `export_is_refused_while_the_switch_is_off`.

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_thread(pool: &PgPool, thread: &str) {
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(thread)
        .bind(ORG)
        .bind(OWNER)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn append(pool: &PgPool, thread: &str, role: &str, content: &str) -> String {
        let mut tx = pool.begin().await.unwrap();
        let request = pb::AppendMessageRequest {
            thread_id: thread.to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            ..Default::default()
        };
        let appended = crate::grpc::append_message_in_tx(&mut tx, &request, OWNER)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        appended.message_id
    }

    async fn seed_run(pool: &PgPool, thread: &str, retention: &str, metadata: Value) {
        let waived = retention == "zdr_waived";
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, metadata,
                               retention_class, waiver_ack_id, waiver_connection_id)
             VALUES ($1, $2, 'g', $3, $4, 'completed', $5, $6, $7, $8)",
        )
        .bind(new_ulid())
        .bind(thread)
        .bind(ORG)
        .bind(OWNER)
        .bind(metadata)
        .bind(retention)
        .bind(waived.then_some("ack-1"))
        .bind(waived.then_some("conn-1"))
        .execute(pool)
        .await
        .unwrap();
    }

    fn export_request(thread: &str) -> pb::ExportThreadRequest {
        pb::ExportThreadRequest {
            thread_id: thread.to_owned(),
            profile: pb::ExportProfile::Full as i32,
        }
    }

    fn reason(status: &Status) -> Option<String> {
        status
            .metadata()
            .get(ERROR_REASON_TRAILER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn thread_with_zdr_run_is_not_exportable() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        append(&pool, &thread, "user", "question").await;
        seed_run(&pool, &thread, "zdr", json!({})).await;

        let owner = VerifiedIdentity::user_for_test(ORG, OWNER);
        let refused = export_thread(&pool, &owner, export_request(&thread))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(reason(&refused).as_deref(), Some("non_persistent_run"));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn thread_with_waived_run_is_not_exportable() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        append(&pool, &thread, "user", "question").await;
        seed_run(&pool, &thread, "persistent", json!({})).await;
        seed_run(&pool, &thread, "zdr_waived", json!({})).await;

        let owner = VerifiedIdentity::user_for_test(ORG, OWNER);
        let refused = export_thread(&pool, &owner, export_request(&thread))
            .await
            .unwrap_err();
        assert_eq!(refused.message(), NOT_EXPORTABLE_CODE);
        assert_eq!(reason(&refused).as_deref(), Some("non_persistent_run"));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn space_thread_is_not_exportable() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        sqlx::query(
            "UPDATE threads SET space_id = 'space-export', origin = 'space', space_decision_ref = 'dec-t', recipient_audience_ref = 'aud-t', privacy_policy_ref = 'pp-t', resource_authorization_ref = 'ra-t', authority_revision = 1, recipient_audience_revision = 1, recipient_audience_hash = 'test-audience' WHERE id = $1",
        )
        .bind(&thread)
        .execute(&pool)
        .await
        .unwrap();

        let owner = VerifiedIdentity::user_for_test(ORG, OWNER);
        let refused = export_thread(&pool, &owner, export_request(&thread))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(reason(&refused).as_deref(), Some("space_thread"));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn other_owner_is_not_found() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        append(&pool, &thread, "user", "question").await;

        let other = VerifiedIdentity::user_for_test(ORG, OTHER);
        let refused = export_thread(&pool, &other, export_request(&thread))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::NotFound);

        let foreign = VerifiedIdentity::user_for_test("org-elsewhere", OWNER);
        let refused = export_thread(&pool, &foreign, export_request(&thread))
            .await
            .unwrap_err();
        assert_eq!(
            refused.code(),
            tonic::Code::NotFound,
            "another org's thread is not confirmed"
        );

        let service = VerifiedIdentity::service_for_test(ORG, &["session:read"], false);
        let refused = export_thread(&pool, &service, export_request(&thread))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn export_lists_active_branch_only() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        append(&pool, &thread, "user", "question").await;
        let abandoned = append(&pool, &thread, "assistant", "abandoned answer").await;
        append(&pool, &thread, "assistant", "kept answer").await;
        sqlx::query("UPDATE messages SET on_active_path = FALSE WHERE id = $1")
            .bind(&abandoned)
            .execute(&pool)
            .await
            .unwrap();

        let owner = VerifiedIdentity::user_for_test(ORG, OWNER);
        let export = export_thread(&pool, &owner, export_request(&thread))
            .await
            .unwrap();
        let contents: Vec<&str> = export.messages.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(contents, ["question", "kept answer"]);
        assert_eq!(export.thread_id, thread);
        assert!(!export.truncated);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn run_summary_carries_origin() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;
        append(&pool, &thread, "user", "question").await;
        seed_run(&pool, &thread, "persistent", json!({"origin": "schedule"})).await;

        let owner = VerifiedIdentity::user_for_test(ORG, OWNER);
        let export = export_thread(&pool, &owner, export_request(&thread))
            .await
            .unwrap();
        assert_eq!(export.runs.len(), 1);
        assert_eq!(export.runs[0].origin, "schedule");
        assert_eq!(export.runs[0].status, "completed");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn export_is_refused_while_the_switch_is_off() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread).await;

        // The switch is read from the environment; the suite runs with it unset, which is the
        // default this test proves.
        let mut request = Request::new(export_request(&thread));
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test(ORG, OWNER));
        let refused = ThreadExportGrpc { pool: pool.clone() }
            .export_thread(request)
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::Unavailable);
        assert_eq!(refused.message(), "thread_export_disabled");
    }
}
