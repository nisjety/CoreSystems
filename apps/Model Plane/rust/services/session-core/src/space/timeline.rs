//! `SpaceRoomReadService.ListSpaceTimeline` (build plan WS15-S6): one window of the room's posts.
//!
//! The window is read from the thread activity summary (migration 0078, WS15-S5): the sort and the
//! keyset are the persisted `last_message_seq`, served by `idx_threads_space_activity`, so only the
//! at most `limit + 1` page rows ever pay for the lateral joins (title, preview, latest run, reply
//! count). Bump-on-reply order, as today: a post that gains a reply moves to the newer end, which
//! can cause a duplicate for a reader paging backwards (the client dedupes by `thread_id`), never a
//! skip.
//!
//! Authority, in order: a user principal (`session:read`, which writes nothing, so a ZDR
//! credential passes), the caller's org, the rollout switch, a Control Space read decision that
//! must be PRESENT (an absent decision is `permission_denied`, never an owner-bound fallback) and
//! verified for exactly this org, this user and this Space, then the audience ceiling it carries.
//! A thread is eligible only when its newest audience stamp (`activity_max_audience_revision`,
//! else the thread's own revision, else 0) is at or below the ceiling, and the thread's own stamp
//! is too: fail closed, so a count or a recent-author list never reveals a row the reader could
//! not open. Title and preview are read under the same ceiling and the revision fold (a deleted
//! message reads as empty). Nothing is written. A ZDR caller reads no row and gets an empty window.
//! Until the summary backfill has finished the RPC refuses with
//! `failed_precondition("summary_backfill_pending")` and the BFF serves the legacy listing.

use mp_contracts::model_plane::v1 as pb;
use mp_contracts::space_timeline::{DEFAULT_LIMIT, MAX_LIMIT, MAX_PINNED, MAX_REFRESH_IDS};
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use tracing::warn;

use crate::auth::{authorize_operation, identity};
use crate::grpc::{
    compact_thread_text, to_proto_timestamp, verify_thread_read_space_decision,
    THREAD_PREVIEW_MAX_CHARS, THREAD_TITLE_MAX_CHARS,
};

const MAX_SPACE_ID_BYTES: usize = 128;
const MAX_THREAD_ID_BYTES: usize = 128;

/// The refusal word a reader sees while the summary backfill is unfinished.
pub(crate) const BACKFILL_PENDING: &str = mp_contracts::space_timeline::BACKFILL_PENDING_MESSAGE;

/// One thread row of the window, before it becomes a `ThreadSummary`.
#[derive(sqlx::FromRow)]
struct TimelineRow {
    id: String,
    created_at: chrono::DateTime<chrono::Utc>,
    title: Option<String>,
    preview: Option<String>,
    pinned_at: Option<chrono::DateTime<chrono::Utc>>,
    last_activity_at: chrono::DateTime<chrono::Utc>,
    last_message_seq: i64,
    turn_count: i32,
    last_role: Option<String>,
    last_author_subject_id: Option<String>,
    recent_author_subject_ids: Vec<String>,
    owner_subject_id: String,
    origin: String,
    persona_enabled: bool,
    latest_run_id: Option<String>,
    latest_run_status: Option<String>,
    latest_run_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    reply_count: Option<i64>,
}

fn summary_from_row(row: TimelineRow, space_id: &str) -> pb::ThreadSummary {
    pb::ThreadSummary {
        thread_id: row.id,
        // The internal session key is never a title and never leaves the plane here.
        session_key: String::new(),
        title: compact_thread_text(row.title, "Verevon Chat", THREAD_TITLE_MAX_CHARS),
        preview: compact_thread_text(row.preview, "", THREAD_PREVIEW_MAX_CHARS),
        created_at: Some(to_proto_timestamp(row.created_at)),
        updated_at: Some(to_proto_timestamp(row.last_activity_at)),
        pinned: row.pinned_at.is_some(),
        space_id: space_id.to_owned(),
        latest_run_id: row.latest_run_id.unwrap_or_default(),
        latest_run_status: row.latest_run_status.unwrap_or_default(),
        latest_run_updated_at: row.latest_run_updated_at.map(to_proto_timestamp),
        origin: row.origin,
        owner_subject_id: row.owner_subject_id,
        persona_enabled: row.persona_enabled,
        reply_count: row
            .reply_count
            .and_then(|count| u32::try_from(count).ok())
            .unwrap_or_default(),
        last_author_subject_id: row.last_author_subject_id.unwrap_or_default(),
        last_message_seq: u64::try_from(row.last_message_seq).unwrap_or_default(),
        recent_author_subject_ids: row.recent_author_subject_ids,
        last_role: row.last_role.unwrap_or_default(),
        turn_count: u32::try_from(row.turn_count).unwrap_or_default(),
        ..Default::default()
    }
}

/// The eligible-thread predicate shared by the window, the pinned shelf, the refresh read and the
/// count. `$1` org, `$2` space, `$3` audience ceiling. A thread with no message (no summary) is
/// never listed, which also keeps an orphan thread's internal key out of the room.
const ELIGIBLE_SQL: &str = "t.org_id = $1 AND t.space_id = $2 AND t.archived_at IS NULL
      AND t.last_message_seq IS NOT NULL
      AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $3)
      AND COALESCE(t.activity_max_audience_revision, t.recipient_audience_revision, 0) <= $3";

/// The page query: `extra` narrows the eligible set, `order` is the inner (indexed) order, `limit`
/// an integer literal, `outer_order` the order of the answer. Lateral joins run on page rows only.
fn page_sql(extra: &str, order: &str, limit: i64, outer_order: &str) -> String {
    format!(
        "WITH page AS (
            SELECT t.id, t.created_at, t.presentation_title, t.presentation_preview, t.pinned_at,
                   t.last_activity_at, t.last_message_seq, t.turn_count, t.last_role,
                   t.last_author_subject_id, t.recent_author_subject_ids, t.user_id, t.origin,
                   t.persona_enabled
            FROM threads t
            WHERE {eligible} {extra}
            ORDER BY {order}
            LIMIT {limit}
         )
         SELECT page.id, page.created_at,
                COALESCE(page.presentation_title, fu.content) AS title,
                COALESCE(page.presentation_preview, lm.content) AS preview,
                page.pinned_at, page.last_activity_at, page.last_message_seq, page.turn_count,
                page.last_role, page.last_author_subject_id, page.recent_author_subject_ids,
                page.user_id AS owner_subject_id, page.origin, page.persona_enabled,
                latest_run.id AS latest_run_id, latest_run.status AS latest_run_status,
                latest_run.updated_at AS latest_run_updated_at, rc.reply_count
         FROM page
         LEFT JOIN LATERAL (
            SELECT {fold_content} AS content
            FROM messages m
            {fold_join}
            WHERE m.thread_id = page.id AND m.role = 'user' AND m.on_active_path
              AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
            ORDER BY m.sequence ASC
            LIMIT 1
         ) fu ON TRUE
         LEFT JOIN LATERAL (
            SELECT {fold_content} AS content
            FROM messages m
            {fold_join}
            WHERE m.thread_id = page.id AND m.on_active_path
              AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
            ORDER BY m.sequence DESC
            LIMIT 1
         ) lm ON TRUE
         LEFT JOIN LATERAL (
            SELECT r.id, {run_phase} AS status, r.updated_at
            FROM runs r
            WHERE r.thread_id = page.id
            ORDER BY r.updated_at DESC, r.created_at DESC, r.id DESC
            LIMIT 1
         ) latest_run ON TRUE
         LEFT JOIN LATERAL (
            SELECT GREATEST(COUNT(*) FILTER (WHERE m.role = 'user') - 1, 0) AS reply_count
            FROM messages m
            WHERE m.thread_id = page.id AND m.on_active_path
              AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
         ) rc ON TRUE
         ORDER BY {outer_order}",
        eligible = ELIGIBLE_SQL,
        fold_content = crate::space::revisions::FOLD_CONTENT_SQL,
        fold_join = crate::space::revisions::fold_join_sql("$3::BIGINT"),
        run_phase = crate::run_phase::run_phase_sql!("r"),
    )
}

/// The window's validated paging request.
#[derive(Debug, PartialEq, Eq)]
struct Window {
    limit: i64,
    before: Option<i64>,
    after: Option<i64>,
}

impl Window {
    /// First page: neither cursor. Pinned posts and the total are answered only then.
    fn is_first_page(&self) -> bool {
        self.before.is_none() && self.after.is_none()
    }
}

#[allow(clippy::result_large_err)]
fn window_from(req: &pb::ListSpaceTimelineRequest) -> Result<Window, Status> {
    let limit = if req.limit == 0 {
        DEFAULT_LIMIT
    } else {
        req.limit.clamp(1, MAX_LIMIT)
    };
    if req.before_seq > 0 && req.after_seq > 0 {
        return Err(Status::invalid_argument(
            "before_seq and after_seq cannot be combined",
        ));
    }
    let seq = |value: u64| {
        if value == 0 {
            Ok(None)
        } else {
            i64::try_from(value)
                .map(Some)
                .map_err(|_| Status::invalid_argument("a timeline cursor is out of range"))
        }
    };
    Ok(Window {
        limit: i64::from(limit),
        before: seq(req.before_seq)?,
        after: seq(req.after_seq)?,
    })
}

/// The caller-named thread ids of a refresh: non-empty, bounded, deduplicated, at most 30.
#[allow(clippy::result_large_err)]
fn refresh_ids(ids: &[String]) -> Result<Vec<String>, Status> {
    let mut out: Vec<String> = Vec::with_capacity(ids.len());
    for id in ids {
        if id.is_empty() || id.len() > MAX_THREAD_ID_BYTES {
            return Err(Status::invalid_argument(
                "refresh_thread_ids contains an invalid id",
            ));
        }
        if !out.contains(id) {
            out.push(id.clone());
        }
    }
    if out.len() > MAX_REFRESH_IDS {
        return Err(Status::invalid_argument("at most 30 refresh_thread_ids"));
    }
    Ok(out)
}

/// What a page query binds after `$1..$3` (org, space, ceiling).
enum Extra<'a> {
    None,
    Cursors(Option<i64>, Option<i64>),
    Ids(&'a [String]),
}

async fn fetch_rows(
    pool: &PgPool,
    sql: &str,
    org_id: &str,
    space_id: &str,
    ceiling: i64,
    extra: Extra<'_>,
) -> Result<Vec<TimelineRow>, Status> {
    let query = sqlx::query_as::<_, TimelineRow>(sql)
        .bind(org_id)
        .bind(space_id)
        .bind(ceiling);
    let query = match extra {
        Extra::None => query,
        Extra::Cursors(before, after) => query.bind(before).bind(after),
        Extra::Ids(ids) => query.bind(ids.to_vec()),
    };
    query.fetch_all(pool).await.map_err(|error| {
        warn!(error = %error, "list_space_timeline failed");
        Status::unavailable("Space timeline is unavailable")
    })
}

/// The whole `ListSpaceTimeline` read; the service impl delegates here.
pub(crate) async fn list_space_timeline(
    pool: &PgPool,
    enabled: bool,
    request: Request<pb::ListSpaceTimelineRequest>,
) -> Result<Response<pb::ListSpaceTimelineResponse>, Status> {
    let caller = identity(&request)?;
    authorize_operation(&caller, "session:read")?;
    let req = request.into_inner();
    caller.authorize_org(&req.org_id)?;
    let user_id = caller
        .user_id()
        .map(str::to_owned)
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Status::permission_denied("a Space room read requires a user principal"))?;
    if !enabled {
        return Err(Status::unimplemented("the Space timeline is not enabled"));
    }
    let space_id = req.space_id.trim().to_owned();
    if space_id.is_empty() || space_id.len() > MAX_SPACE_ID_BYTES {
        return Err(Status::invalid_argument("space_id is required"));
    }
    let read = verify_thread_read_space_decision(
        &req.space_read_decision_token,
        &req.space_read_decision_ref,
        &req.org_id,
        &user_id,
        &space_id,
    )?
    .ok_or_else(|| Status::permission_denied("a Space read decision is required"))?;
    let ceiling = read.recipient_audience_revision;
    let window = window_from(&req)?;
    let ids = refresh_ids(&req.refresh_thread_ids)?;

    // Nothing of a ZDR turn is stored, so a ZDR caller reads no row.
    if caller.zdr() {
        return Ok(Response::new(pb::ListSpaceTimelineResponse::default()));
    }
    match crate::space::thread_activity::backfill_pending(pool).await {
        Ok(false) => {}
        Ok(true) => return Err(Status::failed_precondition(BACKFILL_PENDING)),
        Err(error) => {
            warn!(error = %error, "list_space_timeline backfill probe failed");
            return Err(Status::unavailable("Space timeline is unavailable"));
        }
    }

    // The window. Newest first by default and with `before_seq`; with `after_seq` the nearest
    // newer rows are read oldest first (that is what the index walks) and put back newest first.
    let sql = if window.after.is_some() {
        page_sql(
            "AND ($4::BIGINT IS NULL OR t.last_message_seq < $4)
             AND ($5::BIGINT IS NULL OR t.last_message_seq > $5)",
            "t.last_message_seq ASC",
            window.limit + 1,
            "page.last_message_seq ASC",
        )
    } else if window.before.is_some() {
        page_sql(
            "AND ($4::BIGINT IS NULL OR t.last_message_seq < $4)
             AND ($5::BIGINT IS NULL OR t.last_message_seq > $5)",
            "t.last_message_seq DESC",
            window.limit + 1,
            "page.last_message_seq DESC",
        )
    } else {
        page_sql(
            "",
            "t.last_message_seq DESC",
            window.limit + 1,
            "page.last_message_seq DESC",
        )
    };
    let extra = if window.is_first_page() {
        Extra::None
    } else {
        Extra::Cursors(window.before, window.after)
    };
    let mut rows = fetch_rows(pool, &sql, &req.org_id, &space_id, ceiling, extra).await?;
    let page_size = usize::try_from(window.limit).unwrap_or(usize::MAX);
    let has_more = rows.len() > page_size;
    rows.truncate(page_size);
    if window.after.is_some() {
        rows.reverse();
    }
    let posts: Vec<pb::ThreadSummary> = rows
        .into_iter()
        .map(|row| summary_from_row(row, &space_id))
        .collect();
    let newest_seq = posts.iter().map(|p| p.last_message_seq).max().unwrap_or(0);
    let oldest_seq = posts.iter().map(|p| p.last_message_seq).min().unwrap_or(0);

    let mut pinned = Vec::new();
    let mut total_posts = 0u32;
    if window.is_first_page() {
        if req.include_pinned {
            let sql = page_sql(
                "AND t.pinned_at IS NOT NULL",
                "t.pinned_at DESC, t.id DESC",
                i64::try_from(MAX_PINNED).unwrap_or(10),
                "page.pinned_at DESC, page.id DESC",
            );
            pinned = fetch_rows(pool, &sql, &req.org_id, &space_id, ceiling, Extra::None)
                .await?
                .into_iter()
                .map(|row| summary_from_row(row, &space_id))
                .collect();
        }
        let sql = format!("SELECT count(*) FROM threads t WHERE {ELIGIBLE_SQL}");
        let total: i64 = sqlx::query_scalar(&sql)
            .bind(&req.org_id)
            .bind(&space_id)
            .bind(ceiling)
            .fetch_one(pool)
            .await
            .map_err(|error| {
                warn!(error = %error, "list_space_timeline count failed");
                Status::unavailable("Space timeline is unavailable")
            })?;
        total_posts = u32::try_from(total).unwrap_or(u32::MAX);
    }

    // Space-scoped by the same predicate: an id outside the Space or the ceiling is simply absent.
    let refreshed = if ids.is_empty() {
        Vec::new()
    } else {
        let sql = page_sql(
            "AND t.id = ANY($4::TEXT[])",
            "t.last_message_seq DESC",
            i64::try_from(MAX_REFRESH_IDS).unwrap_or(30),
            "page.last_message_seq DESC",
        );
        fetch_rows(pool, &sql, &req.org_id, &space_id, ceiling, Extra::Ids(&ids))
            .await?
            .into_iter()
            .map(|row| summary_from_row(row, &space_id))
            .collect()
    };

    Ok(Response::new(pb::ListSpaceTimelineResponse {
        posts,
        pinned,
        refreshed,
        has_more,
        oldest_seq,
        newest_seq,
        total_posts,
    }))
}
