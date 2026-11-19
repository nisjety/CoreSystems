//! `SpaceRoomReadService`: Session Core's read-side aggregates for the Space room
//! (build plan P-SC1, decision D3; first RPC WS07-S9).
//!
//! The service was registered empty so that WS07-S9, WS15-S6 and WS11-S2 each add their RPC
//! to `space_room.proto` and an impl method here without touching `main.rs` or the service
//! registration again. Every RPC here is owner-plane read-only: it reads the messages and
//! threads tables as they are, widens no owner-plane ACL, and persists nothing.
//!
//! `ListSpaceArtifacts` (WS07-S9) answers "which artifacts did the room's agents produce".
//! Authority, in order: a user principal (a service identity has no user id and is refused),
//! the caller's org, a Control Space read decision that must be PRESENT (an absent decision is
//! `permission_denied`, never an owner-bound fallback) and verified for exactly this org, this
//! user and this Space, then the audience ceiling the decision carries, applied to both the
//! thread and the message row so a newer-audience message changes neither the list nor any
//! count, and the `since_join` history floor (WS04-S11) applied to the message row the same way,
//! so a late joiner never lists an artifact stamped before the floor. A deleted message (the
//! WS12-S4 revision fold) lists none of its artifacts, so its versions count nowhere. A message
//! off the active path (a turn rewound or regenerated away, V3e-sws-1-rel1) is the same: none of
//! its artifacts is listed or counted. A ZDR caller reads no row and gets an empty page (a ZDR
//! turn produces no artifact).
//! Content is never returned: the summary carries `char_length` only, and the viewer reads the
//! thread through the existing transcript route under its own authority.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::space_room_read_service_server::SpaceRoomReadService;
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use tracing::warn;

use crate::auth::{authorize_operation, identity};
use crate::grpc::{verify_thread_read_space_decision, VerifiedSpaceRead};

const MAX_SPACE_ID_BYTES: usize = 128;
const DEFAULT_ARTIFACT_LIMIT: u32 = 50;
const MAX_ARTIFACT_LIMIT: u32 = 200;
const MAX_CURSOR_BYTES: usize = 512;
const MAX_TITLE_CHARS: i32 = 200;

/// Handler state for the Space room read RPCs.
#[derive(Clone)]
pub(crate) struct SpaceRoomReadGrpc {
    pool: PgPool,
    /// `SPACE_TIMELINE_CORE`, read once at boot (WS15-S6). A rollout control, never an
    /// authorization input.
    timeline_enabled: bool,
}

impl SpaceRoomReadGrpc {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self {
            pool,
            timeline_enabled: mp_contracts::space_timeline::core_enabled(),
        }
    }

    /// Overrides the timeline switch (tests and the boot wiring).
    #[allow(dead_code)]
    pub(crate) fn with_timeline_enabled(mut self, enabled: bool) -> Self {
        self.timeline_enabled = enabled;
        self
    }
}

/// The keyset position of the last row of a page: `(created_at, message_id, artifact_id)`.
/// `artifact_id` is part of the key because one message can carry several artifacts.
struct ArtifactCursor {
    created_at: DateTime<Utc>,
    message_id: String,
    artifact_id: String,
}

fn encode_artifact_cursor(cursor: &ArtifactCursor) -> String {
    let body = serde_json::json!({
        "t": cursor.created_at.to_rfc3339_opts(SecondsFormat::Micros, true),
        "m": cursor.message_id,
        "a": cursor.artifact_id,
    });
    URL_SAFE_NO_PAD.encode(body.to_string())
}

#[allow(clippy::result_large_err)]
fn decode_artifact_cursor(raw: &str) -> Result<ArtifactCursor, Status> {
    let invalid = || Status::invalid_argument("cursor is not a Space artifact cursor");
    if raw.len() > MAX_CURSOR_BYTES {
        return Err(invalid());
    }
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let created_at = value["t"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|parsed| parsed.with_timezone(&Utc))
        .ok_or_else(invalid)?;
    let text = |key: &str| {
        value[key]
            .as_str()
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
    };
    Ok(ArtifactCursor {
        created_at,
        message_id: text("m").ok_or_else(invalid)?,
        artifact_id: text("a").ok_or_else(invalid)?,
    })
}

fn clamp_artifact_limit(limit: u32) -> i64 {
    let limit = if limit == 0 {
        DEFAULT_ARTIFACT_LIMIT
    } else {
        limit.clamp(1, MAX_ARTIFACT_LIMIT)
    };
    i64::from(limit)
}

/// One row per (thread, artifact) at its latest version. Built at run time because the WS12-S4
/// fold is [`crate::space::revisions::fold_join_sql`]; every other fragment is a literal.
///
/// `art` explodes `metadata.artifacts` of the Space's assistant messages under the org join,
/// the unarchived-thread filter, the audience ceiling (thread AND message) and the `since_join`
/// history floor (message, WS04-S11: `$9`, `NULL` when the read carries none). The revision fold
/// (`rv`, the join ListConversation and the Space timeline use) drops a deleted message's rows
/// from `art`: a deletion is terminal (`revise_message` refuses a further revision) and always
/// applies, whatever the ceiling, so none of its artifacts is listed and none of its versions is
/// counted. The active-path filter (`m.on_active_path`, V3e-sws-1-rel1) drops a turn rewound or
/// regenerated away the same way: the transcript does not show that turn, and every history reader
/// filters on the flag whatever the message-versions switch says (the switch gates the writes only).
/// The window count therefore only ever counts rows the caller may see. The artifact
/// kinds are the ones the room can render (`document`, `code`, `html`); a non-object element, a
/// missing id or any other kind is dropped. `version` falls back to 1 when it is not a small
/// non-negative integer. The keyset filter applies after the latest-version pick so a page
/// boundary never splits an artifact from its newest version. `jsonb_exists(m.metadata,
/// 'artifacts')` is the literal predicate of the partial index `idx_messages_space_artifacts`
/// (migration 0085), kept on the raw column so the index still serves the scan.
fn list_space_artifacts_sql() -> String {
    format!(
        "
WITH art AS (
    SELECT m.thread_id,
           m.id AS message_id,
           a.elem->>'id' AS artifact_id,
           a.elem->>'kind' AS kind,
           left(COALESCE(a.elem->>'title', ''), $8) AS title,
           CASE WHEN (a.elem->>'version') ~ '^[0-9]{{1,9}}$'
                THEN (a.elem->>'version')::BIGINT ELSE 1 END AS version,
           char_length(COALESCE(a.elem->>'content', ''))::BIGINT AS size_chars,
           m.agent_name,
           m.created_at,
           count(*) OVER (PARTITION BY m.thread_id, a.elem->>'id') AS version_count
    FROM messages m
    JOIN threads t ON t.id = m.thread_id
    {fold_join}
    CROSS JOIN LATERAL jsonb_array_elements(
        CASE WHEN jsonb_typeof(m.metadata->'artifacts') = 'array'
             THEN m.metadata->'artifacts' ELSE '[]'::jsonb END
    ) AS a(elem)
    WHERE m.space_id = $2
      AND m.role = 'assistant'
      AND jsonb_exists(m.metadata, 'artifacts')
      AND {not_deleted}
      AND m.on_active_path
      AND t.org_id = $1
      AND t.space_id = $2
      AND t.archived_at IS NULL
      AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $3)
      AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
      AND ($9::BIGINT IS NULL OR m.recipient_audience_revision IS NULL
           OR m.recipient_audience_revision >= $9::BIGINT)
      AND jsonb_typeof(a.elem) = 'object'
      AND NULLIF(a.elem->>'id', '') IS NOT NULL
      AND a.elem->>'kind' IN ('document', 'code', 'html')
),
latest AS (
    SELECT DISTINCT ON (thread_id, artifact_id)
           thread_id, message_id, artifact_id, kind, title, version, size_chars,
           agent_name, created_at, version_count
    FROM art
    ORDER BY thread_id, artifact_id, version DESC, created_at DESC, message_id DESC
)
SELECT thread_id, message_id, artifact_id, kind, title, version, version_count,
       size_chars, agent_name, created_at
FROM latest
WHERE $5::TIMESTAMPTZ IS NULL
   OR (created_at, message_id, artifact_id) < ($5::TIMESTAMPTZ, $6::TEXT, $7::TEXT)
ORDER BY created_at DESC, message_id DESC, artifact_id DESC
LIMIT $4",
        fold_join = crate::space::revisions::fold_join_sql("$3::BIGINT"),
        not_deleted = crate::space::revisions::FOLD_NOT_DELETED_SQL,
    )
}

type ArtifactRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    Option<String>,
    DateTime<Utc>,
);

/// The page statement for one verified read. `read` supplies the audience ceiling and the history
/// floor, so the handler and the DB tests bind the same values in the same order. It reads
/// `limit + 1` rows so the caller can tell whether a next page exists.
async fn artifact_page_rows(
    pool: &PgPool,
    org_id: &str,
    space_id: &str,
    read: VerifiedSpaceRead,
    cursor: Option<&ArtifactCursor>,
    limit: i64,
) -> Result<Vec<ArtifactRow>, sqlx::Error> {
    let sql = list_space_artifacts_sql();
    sqlx::query_as::<_, ArtifactRow>(&sql)
        .bind(org_id)
        .bind(space_id)
        .bind(read.recipient_audience_revision)
        .bind(limit + 1)
        .bind(cursor.map(|c| c.created_at))
        .bind(cursor.map(|c| c.message_id.clone()))
        .bind(cursor.map(|c| c.artifact_id.clone()))
        .bind(MAX_TITLE_CHARS)
        .bind(read.history_floor)
        .fetch_all(pool)
        .await
}

#[tonic::async_trait]
impl SpaceRoomReadService for SpaceRoomReadGrpc {
    async fn list_space_timeline(
        &self,
        request: Request<pb::ListSpaceTimelineRequest>,
    ) -> Result<Response<pb::ListSpaceTimelineResponse>, Status> {
        crate::space::timeline::list_space_timeline(&self.pool, self.timeline_enabled, request)
            .await
    }

    async fn list_space_artifacts(
        &self,
        request: Request<pb::ListSpaceArtifactsRequest>,
    ) -> Result<Response<pb::ListSpaceArtifactsResponse>, Status> {
        let caller = identity(&request)?;
        authorize_operation(&caller, "session:read")?;
        let req = request.into_inner();
        caller.authorize_org(&req.org_id)?;
        // A user principal only: the decision's subject must be this very user, and a service
        // identity has no user id to bind it to.
        let user_id = caller
            .user_id()
            .map(str::to_owned)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                Status::permission_denied("a Space room read requires a user principal")
            })?;
        let space_id = req.space_id.trim().to_owned();
        if space_id.is_empty() || space_id.len() > MAX_SPACE_ID_BYTES {
            return Err(Status::invalid_argument("space_id is required"));
        }
        // The decision must be present: `Ok(None)` (no decision offered) is a refusal here,
        // because this aggregate has no owner-bound reading.
        let read = verify_thread_read_space_decision(
            &req.space_read_decision_token,
            &req.space_read_decision_ref,
            &req.org_id,
            &user_id,
            &space_id,
        )?
        .ok_or_else(|| Status::permission_denied("a Space read decision is required"))?;
        let cursor = if req.cursor.is_empty() {
            None
        } else {
            Some(decode_artifact_cursor(&req.cursor)?)
        };
        let limit = clamp_artifact_limit(req.limit);

        // A ZDR caller reads no row: nothing of a ZDR turn is stored, so the list is empty.
        if caller.zdr() {
            return Ok(Response::new(pb::ListSpaceArtifactsResponse {
                artifacts: Vec::new(),
                next_cursor: String::new(),
            }));
        }

        let rows: Vec<ArtifactRow> = artifact_page_rows(
            &self.pool,
            &req.org_id,
            &space_id,
            read,
            cursor.as_ref(),
            limit,
        )
        .await
        .map_err(|error| {
            warn!(error = %error, "list_space_artifacts failed");
            Status::unavailable("Space artifacts are unavailable")
        })?;

        let page_size = usize::try_from(limit).unwrap_or(usize::MAX);
        let has_more = rows.len() > page_size;
        let mut artifacts: Vec<pb::SpaceArtifactSummary> = Vec::with_capacity(rows.len());
        let mut next_cursor = String::new();
        for (index, row) in rows.into_iter().take(page_size).enumerate() {
            let (thread_id, message_id, artifact_id, kind, title, version, version_count, size, agent, created_at) =
                row;
            if has_more && index + 1 == page_size {
                next_cursor = encode_artifact_cursor(&ArtifactCursor {
                    created_at,
                    message_id: message_id.clone(),
                    artifact_id: artifact_id.clone(),
                });
            }
            artifacts.push(pb::SpaceArtifactSummary {
                thread_id,
                message_id,
                artifact_id,
                kind,
                title,
                version,
                version_count,
                size_chars: size,
                agent_name: agent.unwrap_or_default(),
                created_at: created_at.to_rfc3339_opts(SecondsFormat::Micros, true),
                space_id: space_id.clone(),
            });
        }
        Ok(Response::new(pb::ListSpaceArtifactsResponse {
            artifacts,
            next_cursor,
        }))
    }
}

#[cfg(test)]
#[path = "read_service_tests.rs"]
mod tests;
