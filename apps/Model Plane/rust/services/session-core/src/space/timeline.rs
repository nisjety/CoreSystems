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
//! verified for exactly this org, this user and this Space, then the audience ceiling it carries
//! and the `since_join` history floor it may carry (WS04-S11): a thread with no message stamped at
//! or after the floor is not listed, and its title, preview, reply count, turn count and authors
//! read only messages from the floor on.
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
use crate::space::audience_window::floor_gate_message_sql;

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
/// count. `$1` org, `$2` space, `$3` audience ceiling, `$4` history floor (`NULL` = none). A thread
/// with no message (no summary) is never listed, which also keeps an orphan thread's internal key
/// out of the room. Under a floor a thread needs a message the reader may see from the floor on (on
/// the active path, not deleted, within the ceiling and at or after the floor), as `ListThreads` and
/// the attention gate do (V3e-sws-2), so a thread that holds only earlier, or only deleted, messages
/// is not listed to a `since_join` reader.
fn eligible_sql() -> String {
    format!(
        "t.org_id = $1 AND t.space_id = $2 AND t.archived_at IS NULL
      AND t.last_message_seq IS NOT NULL
      AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $3)
      AND COALESCE(t.activity_max_audience_revision, t.recipient_audience_revision, 0) <= $3
      AND ($4::BIGINT IS NULL OR EXISTS (
            SELECT 1 FROM messages em
             WHERE em.thread_id = t.id
               AND em.on_active_path
               AND {gate}))",
        gate = floor_gate_message_sql("em", "$3", "$4"),
    )
}

/// The listing probe of one thread (R-sws-06): whether the floored timeline lists it to this reader. It is
/// [`eligible_sql`] for one thread id, with the same org and Space, the same ceiling on the thread and its
/// activity, and the same floor gate on a message. It leaves out the `archived_at` clause on purpose: an
/// archive event is the change the probe judges, so a thread the reader could list before its archive still
/// delivers that event. `$1` org, `$2` Space, `$3` audience ceiling, `$4` history floor (`NULL` = none), `$5`
/// thread id. The answer is one boolean; no content is read.
pub(crate) fn thread_listed_sql() -> String {
    format!(
        "SELECT EXISTS (
            SELECT 1 FROM threads t
             WHERE t.id = $5 AND t.org_id = $1 AND t.space_id = $2
               AND t.last_message_seq IS NOT NULL
               AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $3)
               AND COALESCE(t.activity_max_audience_revision, t.recipient_audience_revision, 0) <= $3
               AND ($4::BIGINT IS NULL OR EXISTS (
                     SELECT 1 FROM messages em
                      WHERE em.thread_id = t.id
                        AND em.on_active_path
                        AND {gate})))",
        gate = floor_gate_message_sql("em", "$3", "$4"),
    )
}

/// The page query: `extra` narrows the eligible set, `order` is the inner (indexed) order, `limit`
/// an integer literal, `outer_order` the order of the answer. Lateral joins run on page rows only.
/// Under a floor the turn count, the last author and the recent authors count and name only the turns
/// from the floor on, and only those on the active path (V3e-sws-2, V3e-sws-1-rel1); with no floor they
/// are the stored summary, as before.
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
                page.pinned_at, page.last_activity_at, page.last_message_seq,
                CASE WHEN $4::BIGINT IS NULL THEN page.turn_count
                     ELSE (SELECT count(*)::INTEGER FROM messages tm
                            WHERE tm.thread_id = page.id AND tm.role IN ('user', 'assistant')
                              AND tm.on_active_path
                              AND (tm.recipient_audience_revision IS NULL
                                   OR tm.recipient_audience_revision >= $4::BIGINT))
                END AS turn_count,
                page.last_role,
                CASE WHEN $4::BIGINT IS NULL THEN page.last_author_subject_id
                     ELSE (SELECT tm.author_subject_id FROM messages tm
                            WHERE tm.thread_id = page.id AND tm.role = 'user'
                              AND tm.on_active_path
                              AND tm.author_subject_id IS NOT NULL
                              AND (tm.recipient_audience_revision IS NULL
                                   OR tm.recipient_audience_revision >= $4::BIGINT)
                            ORDER BY tm.sequence DESC
                            LIMIT 1)
                END AS last_author_subject_id,
                CASE WHEN $4::BIGINT IS NULL THEN page.recent_author_subject_ids
                     ELSE COALESCE((SELECT array_agg(ra.author_subject_id ORDER BY ra.newest DESC)
                                      FROM (SELECT tm.author_subject_id, max(tm.sequence) AS newest
                                              FROM messages tm
                                             WHERE tm.thread_id = page.id AND tm.role = 'user'
                                               AND tm.author_subject_id IS NOT NULL
                                               AND (tm.recipient_audience_revision IS NULL
                                                    OR tm.recipient_audience_revision >= $4::BIGINT)
                                               AND tm.on_active_path
                                             GROUP BY tm.author_subject_id
                                             ORDER BY max(tm.sequence) DESC
                                             LIMIT 5) ra), ARRAY[]::TEXT[])
                END AS recent_author_subject_ids,
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
              AND ($4::BIGINT IS NULL OR m.recipient_audience_revision IS NULL OR m.recipient_audience_revision >= $4)
            ORDER BY m.sequence ASC
            LIMIT 1
         ) fu ON TRUE
         LEFT JOIN LATERAL (
            SELECT {fold_content} AS content
            FROM messages m
            {fold_join}
            WHERE m.thread_id = page.id AND m.on_active_path
              AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
              AND ($4::BIGINT IS NULL OR m.recipient_audience_revision IS NULL OR m.recipient_audience_revision >= $4)
            ORDER BY m.sequence DESC
            LIMIT 1
         ) lm ON TRUE
         LEFT JOIN LATERAL (
            SELECT r.id, {run_phase} AS status, r.updated_at
            FROM runs r
            WHERE r.thread_id = page.id
              AND ($4::BIGINT IS NULL OR r.recipient_audience_revision IS NULL
                   OR r.recipient_audience_revision >= $4::BIGINT)
            ORDER BY r.updated_at DESC, r.created_at DESC, r.id DESC
            LIMIT 1
         ) latest_run ON TRUE
         LEFT JOIN LATERAL (
            SELECT GREATEST(COUNT(*) FILTER (WHERE m.role = 'user') - 1, 0) AS reply_count
            FROM messages m
            WHERE m.thread_id = page.id AND m.on_active_path
              AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
              AND ($4::BIGINT IS NULL OR m.recipient_audience_revision IS NULL OR m.recipient_audience_revision >= $4)
         ) rc ON TRUE
         ORDER BY {outer_order}",
        eligible = eligible_sql(),
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

/// What a page query binds after `$1..$4` (org, space, ceiling, history floor).
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
    floor: Option<i64>,
    extra: Extra<'_>,
) -> Result<Vec<TimelineRow>, Status> {
    let query = sqlx::query_as::<_, TimelineRow>(sql)
        .bind(org_id)
        .bind(space_id)
        .bind(ceiling)
        .bind(floor);
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
    // WS04-S11: `None` when the switch is off or the decision signs no floor (full history).
    let floor = read.history_floor;
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
            "AND ($5::BIGINT IS NULL OR t.last_message_seq < $5)
             AND ($6::BIGINT IS NULL OR t.last_message_seq > $6)",
            "t.last_message_seq ASC",
            window.limit + 1,
            "page.last_message_seq ASC",
        )
    } else if window.before.is_some() {
        page_sql(
            "AND ($5::BIGINT IS NULL OR t.last_message_seq < $5)
             AND ($6::BIGINT IS NULL OR t.last_message_seq > $6)",
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
    let mut rows = fetch_rows(pool, &sql, &req.org_id, &space_id, ceiling, floor, extra).await?;
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
            pinned = fetch_rows(
                pool,
                &sql,
                &req.org_id,
                &space_id,
                ceiling,
                floor,
                Extra::None,
            )
            .await?
            .into_iter()
            .map(|row| summary_from_row(row, &space_id))
            .collect();
        }
        let sql = format!("SELECT count(*) FROM threads t WHERE {}", eligible_sql());
        let total: i64 = sqlx::query_scalar(&sql)
            .bind(&req.org_id)
            .bind(&space_id)
            .bind(ceiling)
            .bind(floor)
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
            "AND t.id = ANY($5::TEXT[])",
            "t.last_message_seq DESC",
            i64::try_from(MAX_REFRESH_IDS).unwrap_or(30),
            "page.last_message_seq DESC",
        );
        fetch_rows(
            pool,
            &sql,
            &req.org_id,
            &space_id,
            ceiling,
            floor,
            Extra::Ids(&ids),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::audience_window::{history_floor_enabled_from, history_floor_from_claim};
    use crate::space::test_support::{
        clean_space, connect, seed_space_message_at_revision, seed_space_run, seed_space_thread,
    };
    use mp_ids::new_ulid;

    /// The floor is `$4`: in the eligible set and in the three message joins (title, preview and
    /// reply count), while the cursors and the refresh ids moved to `$5` and `$6`. Checked on the
    /// statement text, so no database is needed.
    #[test]
    fn the_history_floor_is_bound_fourth_and_every_message_join_applies_it() {
        assert!(eligible_sql().contains("$4::BIGINT IS NULL"));
        let page = page_sql(
            "",
            "t.last_message_seq DESC",
            10,
            "page.last_message_seq DESC",
        );
        // Three lateral joins, the eligible EXISTS, the three floored thread columns (turn count, last
        // author and recent authors) and the latest run (a run stamped before the floor is not shown)
        // each read under the floor.
        assert_eq!(page.matches("recipient_audience_revision >= $4").count(), 8);
        assert!(page.contains("r.recipient_audience_revision >= $4::BIGINT"));
        assert!(page.contains("CASE WHEN $4::BIGINT IS NULL THEN page.turn_count"));
        assert!(page.contains("CASE WHEN $4::BIGINT IS NULL THEN page.last_author_subject_id"));
        assert!(page.contains("CASE WHEN $4::BIGINT IS NULL THEN page.recent_author_subject_ids"));
        // V3e-sws-2: the eligible gate states the ceiling, the active path and the deletion check.
        let eligible = eligible_sql();
        assert!(eligible.contains("em.on_active_path"));
        assert!(eligible.contains("em.recipient_audience_revision <= $3::BIGINT"));
        assert!(eligible.contains("floor_gate_rev.action = 'deleted'"));
        assert!(!page.contains("$5") && !page.contains("$6"));
        let window = page_sql(
            "AND ($5::BIGINT IS NULL OR t.last_message_seq < $5)
             AND ($6::BIGINT IS NULL OR t.last_message_seq > $6)",
            "t.last_message_seq DESC",
            10,
            "page.last_message_seq DESC",
        );
        assert!(window.contains("$5") && window.contains("$6"));
    }

    /// V3e-sws-1-rel1: under a floor the turn count, the last author and the recent authors read only the
    /// turns on the active path, each subquery stating it, as the three message laterals already do. The
    /// eligible gate's EXISTS states it in its own text as well. Checked on the statement text.
    #[test]
    fn the_floored_thread_columns_and_the_eligible_gate_read_only_the_active_path() {
        let page = page_sql(
            "",
            "t.last_message_seq DESC",
            10,
            "page.last_message_seq DESC",
        );
        assert_eq!(
            page.matches("tm.on_active_path").count(),
            3,
            "turn count, last author and recent authors each state it: {page}"
        );
        assert!(eligible_sql().contains("AND em.on_active_path\n"));
        assert!(thread_listed_sql().contains("AND em.on_active_path\n"));
    }

    /// R-sws-06: the stream's listing probe states the timeline's own predicates for one thread: the org and
    /// the Space, the thread's ceiling and its activity ceiling, and the floor gate on a message, bound on `$3`
    /// and `$4`, with the thread id on `$5`. It leaves out the archived clause on purpose. Checked on the
    /// statement text, so no database is needed.
    #[test]
    fn the_thread_listing_probe_states_the_timeline_predicates_for_one_thread() {
        let sql = thread_listed_sql();
        for part in [
            "t.id = $5",
            "t.org_id = $1",
            "t.space_id = $2",
            "t.recipient_audience_revision <= $3",
            "COALESCE(t.activity_max_audience_revision, t.recipient_audience_revision, 0) <= $3",
            "$4::BIGINT IS NULL OR EXISTS",
            "em.on_active_path",
            "em.recipient_audience_revision <= $3::BIGINT",
            "em.recipient_audience_revision >= $4::BIGINT",
            "floor_gate_rev.action = 'deleted'",
        ] {
            assert!(sql.contains(part), "the probe states {part}: {sql}");
        }
        assert!(
            !sql.contains("archived_at"),
            "an archive event is judged, not the archive state: {sql}"
        );
        assert!(!sql.contains("$6"), "five placeholders only: {sql}");
    }

    /// WS04-S11 against Postgres: under a `since_join` floor, a thread with no message from the floor
    /// on is not listed, and a listed thread's title, preview and reply count read only messages from
    /// the floor on. The switch off, or a zero claim, lists both threads with the earlier turn in view.
    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn timeline_under_a_history_floor_lists_and_previews_only_what_the_reader_may_see() {
        let Some(pool) = connect().await else {
            return;
        };
        let suffix = new_ulid();
        let org = format!("timeline-floor-{suffix}");
        let space = format!("space-timeline-floor-{suffix}");
        let earlier_only = format!("{suffix}-earlier-only");
        let mixed = format!("{suffix}-mixed");
        seed_space_thread(&pool, &org, &space, &earlier_only, "owner-a").await;
        seed_space_thread(&pool, &org, &space, &mixed, "owner-a").await;
        seed_space_message_at_revision(
            &pool,
            &space,
            &earlier_only,
            &format!("{suffix}-e1"),
            "earlier only",
            1,
        )
        .await;
        seed_space_message_at_revision(
            &pool,
            &space,
            &mixed,
            &format!("{suffix}-m1"),
            "first before the join",
            1,
        )
        .await;
        seed_space_message_at_revision(
            &pool,
            &space,
            &mixed,
            &format!("{suffix}-m2"),
            "second after the join",
            4,
        )
        .await;

        let sql = page_sql(
            "",
            "t.last_message_seq DESC",
            10,
            "page.last_message_seq DESC",
        );
        // A member who joined at revision 3; the decision is at revision 5.
        let on = history_floor_enabled_from(Some("true"));
        let floored = fetch_rows(
            &pool,
            &sql,
            &org,
            &space,
            5,
            history_floor_from_claim(3, on),
            Extra::None,
        )
        .await
        .expect("floored page");
        let listed: Vec<&str> = floored.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            listed,
            vec![mixed.as_str()],
            "a thread with nothing from the floor on is not listed"
        );
        let shown = &floored[0];
        assert_eq!(shown.title.as_deref(), Some("second after the join"));
        assert_eq!(shown.preview.as_deref(), Some("second after the join"));
        assert_eq!(
            shown.reply_count,
            Some(0),
            "the earlier turn is not a reply here"
        );

        // The switch off: the same claim is ignored, both threads list and the earlier turn shows.
        let off = history_floor_from_claim(3, history_floor_enabled_from(Some("1")));
        let all = fetch_rows(&pool, &sql, &org, &space, 5, off, Extra::None)
            .await
            .expect("unfloored page");
        assert_eq!(all.len(), 2, "floor off lists both threads");
        let full = all
            .iter()
            .find(|row| row.id == mixed)
            .expect("the mixed thread");
        assert_eq!(full.title.as_deref(), Some("first before the join"));
        assert_eq!(full.reply_count, Some(1));

        // The switch on with a zero claim: no floor is signed, so the full view again.
        let zero = history_floor_from_claim(0, on);
        let zero_claim = fetch_rows(&pool, &sql, &org, &space, 5, zero, Extra::None)
            .await
            .expect("zero-claim page");
        assert_eq!(zero_claim.len(), 2, "a zero claim is no floor");

        clean_space(&pool, &org, &space).await;
    }

    /// V3e-sws-2 against Postgres: under a `since_join` floor a listed thread counts only the turns the
    /// reader may see and names only the authors of those turns, its last author being the author of the
    /// newest user turn from the floor on (none when there is none). The switch off keeps the stored
    /// summary: every turn, and every author of the thread's history, newest first.
    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn timeline_under_a_floor_counts_and_names_only_the_turns_the_reader_may_see() {
        let Some(pool) = connect().await else {
            return;
        };
        let suffix = new_ulid();
        let org = format!("timeline-turns-floor-{suffix}");
        let space = format!("space-timeline-turns-floor-{suffix}");
        let thread = format!("{suffix}-thread");
        seed_space_thread(&pool, &org, &space, &thread, "owner-a").await;
        // The earlier turn is written by another author, so the stored summary names both.
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
             recipient_audience_revision, recipient_audience_hash, authority_revision, \
             resource_authorization_ref, author_subject_id) \
             VALUES ($1, $2, 'user', 'before the join', $3, 'audience-1', 1, 'sha256:audience-1', 1, \
             'resource-1', 'author-old')",
        )
        .bind(format!("{suffix}-before"))
        .bind(&thread)
        .bind(&space)
        .execute(&pool)
        .await
        .expect("seed the earlier turn");
        seed_space_message_at_revision(
            &pool,
            &space,
            &thread,
            &format!("{suffix}-after"),
            "after the join",
            4,
        )
        .await;
        // Two runs of the thread: the one stamped at the floor, and an earlier-stamped one updated last.
        let late_run = format!("{suffix}-late-run");
        let early_run = format!("{suffix}-early-run");
        seed_space_run(
            &pool,
            &org,
            &space,
            &thread,
            &late_run,
            "completed",
            "persistent",
        )
        .await;
        seed_space_run(
            &pool,
            &org,
            &space,
            &thread,
            &early_run,
            "completed",
            "persistent",
        )
        .await;
        for (run, revision) in [(&late_run, 4_i64), (&early_run, 2_i64)] {
            sqlx::query("UPDATE runs SET recipient_audience_revision = $1 WHERE id = $2")
                .bind(revision)
                .bind(run)
                .execute(&pool)
                .await
                .expect("stamp the run");
        }
        sqlx::query("UPDATE runs SET updated_at = now() + interval '1 minute' WHERE id = $1")
            .bind(&early_run)
            .execute(&pool)
            .await
            .expect("the pre-join run is the most recently updated");

        // A thread whose only message from the floor on is an assistant reply. Its earlier user turn is
        // by 'author-old', so under the floor no user turn is in view and no author is named.
        let reply_only = format!("{suffix}-reply-only");
        seed_space_thread(&pool, &org, &space, &reply_only, "owner-a").await;
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
             recipient_audience_revision, recipient_audience_hash, authority_revision, \
             resource_authorization_ref, author_subject_id) \
             VALUES ($1, $2, 'user', 'asked before the join', $3, 'audience-1', 1, 'sha256:audience-1', 1, \
             'resource-1', 'author-old')",
        )
        .bind(format!("{suffix}-reply-ask"))
        .bind(&reply_only)
        .bind(&space)
        .execute(&pool)
        .await
        .expect("seed the earlier user turn");
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
             recipient_audience_revision, recipient_audience_hash, authority_revision, \
             resource_authorization_ref, author_subject_id) \
             VALUES ($1, $2, 'assistant', 'answered after the join', $3, 'audience-1', 4, \
             'sha256:audience-1', 1, 'resource-1', NULL)",
        )
        .bind(format!("{suffix}-reply-answer"))
        .bind(&reply_only)
        .bind(&space)
        .execute(&pool)
        .await
        .expect("seed the reply from the floor on");

        let sql = page_sql(
            "",
            "t.last_message_seq DESC",
            10,
            "page.last_message_seq DESC",
        );
        let on = history_floor_enabled_from(Some("true"));
        let floored = fetch_rows(
            &pool,
            &sql,
            &org,
            &space,
            5,
            history_floor_from_claim(3, on),
            Extra::None,
        )
        .await
        .expect("floored page");
        let shown = floored
            .iter()
            .find(|row| row.id == thread)
            .expect("the thread has a turn from the floor on, so it is listed");
        assert_eq!(
            shown.turn_count, 1,
            "only the turn from the floor on is counted"
        );
        assert_eq!(
            shown.recent_author_subject_ids,
            vec!["member-2".to_owned()],
            "only the author of that turn is named"
        );
        assert_eq!(
            shown.last_author_subject_id.as_deref(),
            Some("member-2"),
            "the last author is the author of the newest user turn from the floor on"
        );
        let reply = floored
            .iter()
            .find(|row| row.id == reply_only)
            .expect("the thread has a reply from the floor on, so it is listed");
        assert_eq!(
            reply.turn_count, 1,
            "only the reply from the floor on is counted"
        );
        assert_eq!(
            reply.last_author_subject_id.clone().unwrap_or_default(),
            "",
            "no user turn from the floor on, so no last author is named"
        );
        assert!(
            reply.recent_author_subject_ids.is_empty(),
            "no author of an earlier turn is named"
        );

        // The switch off: the stored summary, every turn and every author.
        let all = fetch_rows(&pool, &sql, &org, &space, 5, None, Extra::None)
            .await
            .expect("unfloored page");
        let whole = all
            .iter()
            .find(|row| row.id == thread)
            .expect("the thread is listed");
        assert_eq!(whole.turn_count, 2, "the switch off counts every turn");
        assert_eq!(
            whole.recent_author_subject_ids,
            vec!["member-2".to_owned(), "author-old".to_owned()],
            "the switch off names every author, newest first"
        );
        let whole_reply = all
            .iter()
            .find(|row| row.id == reply_only)
            .expect("the switch off lists the reply thread");
        assert_eq!(
            whole_reply.last_author_subject_id.as_deref(),
            Some("author-old"),
            "the switch off names the stored last author, the author of the earlier turn"
        );
        assert_eq!(
            whole_reply.recent_author_subject_ids,
            vec!["author-old".to_owned()],
            "the switch off names the earlier turn's author"
        );
        assert_eq!(
            shown.latest_run_id.as_deref(),
            Some(late_run.as_str()),
            "the latest run is the one stamped from the floor on"
        );
        assert_eq!(
            whole.latest_run_id.as_deref(),
            Some(early_run.as_str()),
            "the switch off shows the most recently updated run"
        );

        clean_space(&pool, &org, &space).await;
    }
}
