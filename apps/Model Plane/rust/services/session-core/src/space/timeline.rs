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
//! message reads as empty). The verified read's history floors (WS04-S11 revision floor, D18
//! timestamp floor; see `space::audience_window`) narrow the same message rows: under a floor a
//! thread is listed only when it holds a message the reader may see, and no stored title or preview
//! is shown, only the text of the visible messages. Nothing is written. A ZDR caller reads no row
//! and gets an empty window.
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
use crate::space::audience_window::{audience_window, AudienceWindow};

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
    // WS18-S3: the provenance words of the title and the pin, read as stored (W3-FIX-16).
    title_source: String,
    pinned_source: String,
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
        title_source: row.title_source,
        pinned_source: row.pinned_source,
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

/// The two history floors of one page query, as the placeholders it names them by (the revision
/// floor, then the D18 timestamp floor). A query binds its own parameters first and the floors
/// after them: none (`$4`, `$5`), the two cursors (`$6`, `$7`), or the ids (`$5`, `$6`).
const FLOOR_NO_EXTRA: (&str, &str) = ("$4", "$5");
const FLOOR_AFTER_CURSORS: (&str, &str) = ("$6", "$7");
const FLOOR_AFTER_IDS: (&str, &str) = ("$5", "$6");

/// Whether either floor is set. A floored read shows no stored title or preview.
fn has_floor_sql((floor_rev, floor_at): (&str, &str)) -> String {
    format!("({floor_rev}::BIGINT IS NOT NULL OR {floor_at}::TIMESTAMPTZ IS NOT NULL)")
}

/// The floors on one message row `m`, the predicates the conversation reads apply (WS04-S11, D18):
/// a NULL-stamped row passes the revision floor, and the timestamp floor hides a row created before
/// the instant.
fn message_floor_sql((floor_rev, floor_at): (&str, &str)) -> String {
    format!(
        "({floor_rev}::BIGINT IS NULL OR m.recipient_audience_revision IS NULL
                 OR m.recipient_audience_revision >= {floor_rev}::BIGINT)
         AND ({floor_at}::TIMESTAMPTZ IS NULL OR m.created_at >= {floor_at}::TIMESTAMPTZ)"
    )
}

/// Under a floor a thread is listed only when it holds a message the reader may see (the ceiling and
/// the floors, on the active path). A thread that predates the floor then shows neither its text
/// nor its owner, authors or activity. Without a floor the EXISTS is never read.
fn thread_floor_sql(floor: (&str, &str)) -> String {
    format!(
        "AND (NOT {has_floor} OR EXISTS (
            SELECT 1 FROM messages m
            WHERE m.thread_id = t.id AND m.on_active_path
              AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
              AND {visible}))",
        has_floor = has_floor_sql(floor),
        visible = message_floor_sql(floor),
    )
}

/// The page query: `extra` narrows the eligible set, `order` is the inner (indexed) order, `limit`
/// an integer literal, `outer_order` the order of the answer, and `floor` the placeholders of the
/// two history floors. Lateral joins run on page rows only.
fn page_sql(extra: &str, order: &str, limit: i64, outer_order: &str, floor: (&str, &str)) -> String {
    format!(
        "WITH page AS (
            SELECT t.id, t.created_at, t.presentation_title, t.presentation_preview, t.pinned_at,
                   t.last_activity_at, t.last_message_seq, t.turn_count, t.last_role,
                   t.last_author_subject_id, t.recent_author_subject_ids, t.user_id, t.origin,
                   t.persona_enabled, t.title_source, t.pinned_source
            FROM threads t
            WHERE {eligible} {extra} {thread_floor}
            ORDER BY {order}
            LIMIT {limit}
         )
         SELECT page.id, page.created_at,
                COALESCE(CASE WHEN {has_floor} THEN NULL ELSE page.presentation_title END, fu.content) AS title,
                COALESCE(CASE WHEN {has_floor} THEN NULL ELSE page.presentation_preview END, lm.content) AS preview,
                page.pinned_at, page.last_activity_at, page.last_message_seq, page.turn_count,
                page.last_role, page.last_author_subject_id, page.recent_author_subject_ids,
                page.user_id AS owner_subject_id, page.origin, page.persona_enabled,
                page.title_source, page.pinned_source,
                latest_run.id AS latest_run_id, latest_run.status AS latest_run_status,
                latest_run.updated_at AS latest_run_updated_at, rc.reply_count
         FROM page
         LEFT JOIN LATERAL (
            SELECT {fold_content} AS content
            FROM messages m
            {fold_join}
            WHERE m.thread_id = page.id AND m.role = 'user' AND m.on_active_path
              AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
              AND {visible}
            ORDER BY m.sequence ASC
            LIMIT 1
         ) fu ON TRUE
         LEFT JOIN LATERAL (
            SELECT {fold_content} AS content
            FROM messages m
            {fold_join}
            WHERE m.thread_id = page.id AND m.on_active_path
              AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $3)
              AND {visible}
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
              AND {visible}
         ) rc ON TRUE
         ORDER BY {outer_order}",
        eligible = ELIGIBLE_SQL,
        thread_floor = thread_floor_sql(floor),
        has_floor = has_floor_sql(floor),
        visible = message_floor_sql(floor),
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
    audience: AudienceWindow,
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
    // The two history floors come last, in the order `page_sql` names them.
    query
        .bind(audience.floor)
        .bind(audience.floor_at)
        .fetch_all(pool)
        .await
        .map_err(|error| {
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
    let audience = audience_window(Some(read));
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
            FLOOR_AFTER_CURSORS,
        )
    } else if window.before.is_some() {
        page_sql(
            "AND ($4::BIGINT IS NULL OR t.last_message_seq < $4)
             AND ($5::BIGINT IS NULL OR t.last_message_seq > $5)",
            "t.last_message_seq DESC",
            window.limit + 1,
            "page.last_message_seq DESC",
            FLOOR_AFTER_CURSORS,
        )
    } else {
        page_sql(
            "",
            "t.last_message_seq DESC",
            window.limit + 1,
            "page.last_message_seq DESC",
            FLOOR_NO_EXTRA,
        )
    };
    let extra = if window.is_first_page() {
        Extra::None
    } else {
        Extra::Cursors(window.before, window.after)
    };
    let mut rows = fetch_rows(pool, &sql, &req.org_id, &space_id, ceiling, extra, audience).await?;
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
                FLOOR_NO_EXTRA,
            );
            pinned = fetch_rows(pool, &sql, &req.org_id, &space_id, ceiling, Extra::None, audience)
                .await?
                .into_iter()
                .map(|row| summary_from_row(row, &space_id))
                .collect();
        }
        let thread_floor = thread_floor_sql(FLOOR_NO_EXTRA);
        let sql = format!("SELECT count(*) FROM threads t WHERE {ELIGIBLE_SQL} {thread_floor}");
        let total: i64 = sqlx::query_scalar(&sql)
            .bind(&req.org_id)
            .bind(&space_id)
            .bind(ceiling)
            .bind(audience.floor)
            .bind(audience.floor_at)
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
            FLOOR_AFTER_IDS,
        );
        fetch_rows(pool, &sql, &req.org_id, &space_id, ceiling, Extra::Ids(&ids), audience)
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
    use crate::auth::VerifiedIdentity;
    use crate::grpc::space_w1_tests::{mint, StreamKeysEnv, DECISION_REF};
    use crate::space::test_support::{clean_space, connect, seed_space_message, seed_space_thread};
    use chrono::TimeZone as _;
    use serde_json::json;
    use std::collections::BTreeSet;

    // ---- pure ---------------------------------------------------------------------------------

    fn row(title_source: &str, pinned_source: &str) -> TimelineRow {
        TimelineRow {
            id: "thread-1".to_owned(),
            created_at: chrono::Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).single().expect("time"),
            title: Some("A post".to_owned()),
            preview: Some("The newest words".to_owned()),
            pinned_at: None,
            last_activity_at: chrono::Utc.with_ymd_and_hms(2026, 11, 1, 12, 5, 0).single().expect("time"),
            last_message_seq: 42,
            turn_count: 3,
            last_role: Some("assistant".to_owned()),
            last_author_subject_id: Some("user-2".to_owned()),
            recent_author_subject_ids: vec!["user-2".to_owned(), "user-1".to_owned()],
            owner_subject_id: "user-1".to_owned(),
            origin: "space".to_owned(),
            persona_enabled: true,
            latest_run_id: Some("run-1".to_owned()),
            latest_run_status: Some("completed".to_owned()),
            latest_run_updated_at: None,
            reply_count: Some(2),
            title_source: title_source.to_owned(),
            pinned_source: pinned_source.to_owned(),
        }
    }

    #[test]
    fn a_summary_carries_the_title_and_pin_provenance_as_stored() {
        for (title_source, pinned_source) in [("owner", "owner"), ("agent", "owner"), ("owner", "agent"), ("agent", "agent")] {
            let summary = summary_from_row(row(title_source, pinned_source), "space-1");
            assert_eq!(
                (summary.title_source.as_str(), summary.pinned_source.as_str()),
                (title_source, pinned_source)
            );
        }
        // The rest of the row maps across, and the internal session key never does.
        let summary = summary_from_row(row("owner", "owner"), "space-1");
        assert_eq!(summary.thread_id, "thread-1");
        assert_eq!(summary.session_key, "");
        assert_eq!(summary.space_id, "space-1");
        assert_eq!(summary.last_message_seq, 42);
        assert_eq!(summary.turn_count, 3);
        assert_eq!(summary.reply_count, 2);
        assert_eq!(summary.last_role, "assistant");
        assert_eq!(summary.recent_author_subject_ids, ["user-2", "user-1"]);
        assert!(!summary.pinned, "no pinned_at, not pinned");
        let mut pinned = row("owner", "agent");
        pinned.pinned_at = Some(chrono::Utc::now());
        assert!(summary_from_row(pinned, "space-1").pinned);
    }

    fn request(limit: u32, before_seq: u64, after_seq: u64) -> pb::ListSpaceTimelineRequest {
        pb::ListSpaceTimelineRequest { limit, before_seq, after_seq, ..Default::default() }
    }

    #[test]
    fn the_window_defaults_clamps_and_refuses_combined_or_out_of_range_cursors() {
        let default = window_from(&request(0, 0, 0)).expect("defaults");
        assert_eq!(default, Window { limit: i64::from(DEFAULT_LIMIT), before: None, after: None });
        assert!(default.is_first_page());
        assert_eq!(window_from(&request(1, 0, 0)).expect("one").limit, 1);
        assert_eq!(window_from(&request(MAX_LIMIT, 0, 0)).expect("the most").limit, i64::from(MAX_LIMIT));
        assert_eq!(window_from(&request(u32::MAX, 0, 0)).expect("clamped").limit, i64::from(MAX_LIMIT));
        let before = window_from(&request(5, 7, 0)).expect("before");
        assert_eq!((before.before, before.after, before.is_first_page()), (Some(7), None, false));
        let after = window_from(&request(5, 0, 9)).expect("after");
        assert_eq!((after.before, after.after, after.is_first_page()), (None, Some(9), false));
        for (why, bad) in [
            ("both cursors", request(5, 7, 9)),
            ("a before cursor beyond i64", request(5, u64::MAX, 0)),
            ("an after cursor beyond i64", request(5, 0, u64::MAX)),
        ] {
            let status = window_from(&bad).expect_err(why);
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{why}");
        }
    }

    #[test]
    fn refresh_ids_are_non_empty_bounded_deduplicated_and_at_most_thirty() {
        assert_eq!(refresh_ids(&[]).expect("none"), Vec::<String>::new());
        let named = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
        assert_eq!(refresh_ids(&named(&["a", "b", "a", "c", "b"])).expect("deduplicated"), named(&["a", "b", "c"]));
        for bad in [named(&["a", ""]), vec!["x".repeat(MAX_THREAD_ID_BYTES + 1)]] {
            assert_eq!(refresh_ids(&bad).expect_err("an invalid id").code(), tonic::Code::InvalidArgument);
        }
        assert_eq!(refresh_ids(&["x".repeat(MAX_THREAD_ID_BYTES)]).expect("at the id cap").len(), 1);
        let many = |count: usize| (0..count).map(|n| format!("thread-{n}")).collect::<Vec<_>>();
        assert_eq!(refresh_ids(&many(MAX_REFRESH_IDS)).expect("at the cap").len(), MAX_REFRESH_IDS);
        assert_eq!(refresh_ids(&many(MAX_REFRESH_IDS + 1)).expect_err("over the cap").code(), tonic::Code::InvalidArgument);
        // Duplicates are collapsed before the count, so thirty distinct ids may be sent twice over.
        let doubled: Vec<String> = many(MAX_REFRESH_IDS).into_iter().chain(many(MAX_REFRESH_IDS)).collect();
        assert_eq!(refresh_ids(&doubled).expect("deduplicated first").len(), MAX_REFRESH_IDS);
    }

    /// The `$n` placeholders a statement names, as numbers.
    fn placeholders(sql: &str) -> BTreeSet<usize> {
        let mut found = BTreeSet::new();
        let mut rest = sql;
        while let Some(at) = rest.find('$') {
            rest = &rest[at + 1..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(number) = digits.parse::<usize>() {
                found.insert(number);
            }
            rest = &rest[digits.len()..];
        }
        found
    }

    #[test]
    fn every_page_query_names_exactly_the_binds_it_carries() {
        // The org, space and ceiling, the query's own binds, then the two floors: a bind that no
        // clause names, or a clause that names an unbound parameter, fails at run time.
        let cursors = page_sql(
            "AND ($4::BIGINT IS NULL OR t.last_message_seq < $4) AND ($5::BIGINT IS NULL OR t.last_message_seq > $5)",
            "t.last_message_seq ASC",
            3,
            "page.last_message_seq ASC",
            FLOOR_AFTER_CURSORS,
        );
        let first = page_sql("", "t.last_message_seq DESC", 3, "page.last_message_seq DESC", FLOOR_NO_EXTRA);
        let refresh = page_sql(
            "AND t.id = ANY($4::TEXT[])",
            "t.last_message_seq DESC",
            30,
            "page.last_message_seq DESC",
            FLOOR_AFTER_IDS,
        );
        let count = format!("SELECT count(*) FROM threads t WHERE {ELIGIBLE_SQL} {}", thread_floor_sql(FLOOR_NO_EXTRA));
        for (case, sql, binds) in [
            ("cursors", cursors, 7),
            ("first page", first, 5),
            ("refresh", refresh, 6),
            ("count", count, 5),
        ] {
            assert_eq!(placeholders(&sql), (1..=binds).collect::<BTreeSet<usize>>(), "{case}");
        }
    }

    // ---- real Postgres (every test is `#[ignore]`d) -----------------------------------------
    //
    // `cargo test -p session-core --bin session-core space::timeline -- --ignored --test-threads=1`
    // with `DATABASE_URL` pointing at a disposable Postgres whose database name contains `test`.

    const NEEDS_DB: &str = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)";
    const OWNER: &str = "owner-1";
    const VIEWER: &str = "viewer-1";
    /// The decision's audience ceiling; every seeded thread and message is stamped at revision 1.
    const CEILING: i64 = 4;

    fn read_token(org: &str, user: &str, space: &str, floor_at: Option<chrono::DateTime<chrono::Utc>>) -> String {
        mint(
            crate::grpc::CONTROL_SPACE_DECISION_AUDIENCE,
            &["thread:read"],
            false,
            |claims| {
                claims["org_id"] = json!(org);
                claims["subject_id"] = json!(user);
                claims["space_ref"] = json!(space);
                claims["recipient_audience_revision"] = json!(CEILING);
                if let Some(at) = floor_at {
                    claims["history_floor_at"] = json!(at.to_rfc3339());
                }
                claims["issued_at"] = json!(chrono::Utc::now().to_rfc3339());
                claims["expires_at"] = json!((chrono::Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
            },
            false,
        )
    }

    async fn timeline(
        pool: &PgPool,
        org: &str,
        space: &str,
        edit: impl FnOnce(&mut pb::ListSpaceTimelineRequest),
    ) -> pb::ListSpaceTimelineResponse {
        timeline_as(pool, org, space, None, edit).await
    }

    /// The same read by a member whose decision carries the D18 timestamp floor `floor_at`.
    async fn timeline_as(
        pool: &PgPool,
        org: &str,
        space: &str,
        floor_at: Option<chrono::DateTime<chrono::Utc>>,
        edit: impl FnOnce(&mut pb::ListSpaceTimelineRequest),
    ) -> pb::ListSpaceTimelineResponse {
        let mut message = pb::ListSpaceTimelineRequest {
            org_id: org.to_owned(),
            space_id: space.to_owned(),
            space_read_decision_ref: DECISION_REF.to_owned(),
            space_read_decision_token: read_token(org, VIEWER, space, floor_at),
            ..Default::default()
        };
        edit(&mut message);
        let mut request = Request::new(message);
        request.extensions_mut().insert(VerifiedIdentity::user_for_test(org, VIEWER));
        list_space_timeline(pool, true, request).await.expect("the timeline answers").into_inner()
    }

    /// A Space thread with one user message, so it has a summary and is listed.
    async fn post(pool: &PgPool, org: &str, space: &str, thread: &str) {
        seed_space_thread(pool, org, space, thread, OWNER).await;
        seed_space_message(pool, space, thread, &format!("first-{thread}"), "user", Some(OWNER)).await;
    }

    fn ids_of(answer_rows: &[pb::ThreadSummary]) -> Vec<&str> {
        answer_rows.iter().map(|summary| summary.thread_id.as_str()).collect()
    }

    fn sources(rows: &[pb::ThreadSummary], thread: &str) -> (String, String) {
        let summary = rows
            .iter()
            .find(|summary| summary.thread_id == thread)
            .unwrap_or_else(|| panic!("{thread} is not among {:?}", ids_of(rows)));
        (summary.title_source.clone(), summary.pinned_source.clone())
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn list_space_timeline_returns_title_source_and_pinned_source_on_posts_pinned_and_refreshed_rows() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let suffix = mp_ids::new_ulid();
        let (org, space) = (format!("org-ts-{suffix}"), format!("space-ts-{suffix}"));
        let untouched = format!("thread-untouched-{suffix}");
        let renamed = format!("thread-renamed-{suffix}");
        let agent_pinned = format!("thread-agent-pinned-{suffix}");
        let owner_pinned = format!("thread-owner-pinned-{suffix}");
        for thread in [&untouched, &renamed, &agent_pinned, &owner_pinned] {
            post(&pool, &org, &space, thread).await;
        }
        sqlx::query("UPDATE threads SET presentation_title = 'Agent title', title_source = 'agent' WHERE id = $1")
            .bind(&renamed)
            .execute(&pool)
            .await
            .expect("an agent renames a thread");
        sqlx::query("UPDATE threads SET pinned_at = now(), pinned_source = 'agent' WHERE id = $1")
            .bind(&agent_pinned)
            .execute(&pool)
            .await
            .expect("an agent pins a thread");
        sqlx::query("UPDATE threads SET pinned_at = now() WHERE id = $1")
            .bind(&owner_pinned)
            .execute(&pool)
            .await
            .expect("the owner pins a thread");

        let answer = timeline(&pool, &org, &space, |request| {
            request.include_pinned = true;
            request.refresh_thread_ids = vec![renamed.clone(), agent_pinned.clone()];
        })
        .await;

        // The window: an untouched thread reads owner for both, a renamed one agent for the title only.
        let owner = || "owner".to_owned();
        let agent = || "agent".to_owned();
        assert_eq!(sources(&answer.posts, &untouched), (owner(), owner()), "{NEEDS_DB}");
        assert_eq!(sources(&answer.posts, &renamed), (agent(), owner()));
        assert_eq!(sources(&answer.posts, &agent_pinned), (owner(), agent()));
        assert_eq!(sources(&answer.posts, &owner_pinned), (owner(), owner()));
        // The pinned shelf carries the same provenance.
        assert_eq!(answer.pinned.len(), 2, "{:?}", ids_of(&answer.pinned));
        assert_eq!(sources(&answer.pinned, &agent_pinned), (owner(), agent()));
        assert_eq!(sources(&answer.pinned, &owner_pinned), (owner(), owner()));
        // So do the refreshed rows.
        assert_eq!(answer.refreshed.len(), 2, "{:?}", ids_of(&answer.refreshed));
        assert_eq!(sources(&answer.refreshed, &renamed), (agent(), owner()));
        assert_eq!(sources(&answer.refreshed, &agent_pinned), (owner(), agent()));
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn window_orders_by_last_message_seq_and_has_more_probe() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let suffix = mp_ids::new_ulid();
        let (org, space) = (format!("org-tw-{suffix}"), format!("space-tw-{suffix}"));
        let threads: Vec<String> = (1..=5).map(|n| format!("thread-{n}-{suffix}")).collect();
        for thread in &threads {
            post(&pool, &org, &space, thread).await;
        }
        // A reply bumps the first post to the newest end (bump-on-reply).
        seed_space_message(&pool, &space, &threads[0], &format!("reply-{suffix}"), "assistant", None).await;
        let (t1, t2, t3, t4, t5) = (&threads[0], &threads[1], &threads[2], &threads[3], &threads[4]);

        // The newest page: newest last_message_seq first, with the one-row probe saying more exists.
        let first = timeline(&pool, &org, &space, |request| request.limit = 2).await;
        assert_eq!(ids_of(&first.posts), [t1.as_str(), t5.as_str()], "{NEEDS_DB}");
        assert!(first.has_more, "a third row exists past the page of two");
        assert_eq!(first.total_posts, 5, "the count is answered on the first page");
        assert_eq!(first.newest_seq, first.posts[0].last_message_seq);
        assert_eq!(first.oldest_seq, first.posts[1].last_message_seq);
        assert!(first.newest_seq > first.oldest_seq);

        // before_seq keyset: strictly older than the cursor, same order, no overlap and no skip.
        let second = timeline(&pool, &org, &space, |request| {
            request.limit = 2;
            request.before_seq = first.oldest_seq;
        })
        .await;
        assert_eq!(ids_of(&second.posts), [t4.as_str(), t3.as_str()]);
        assert!(second.has_more);
        assert_eq!(second.total_posts, 0, "the count is first page only");
        let third = timeline(&pool, &org, &space, |request| {
            request.limit = 2;
            request.before_seq = second.oldest_seq;
        })
        .await;
        assert_eq!(ids_of(&third.posts), [t2.as_str()]);
        assert!(!third.has_more, "the last page has no probe row");

        // A page exactly as long as the list has no more.
        let all = timeline(&pool, &org, &space, |request| request.limit = 5).await;
        assert_eq!(ids_of(&all.posts), [t1.as_str(), t5.as_str(), t4.as_str(), t3.as_str(), t2.as_str()]);
        assert!(!all.has_more);

        // after_seq: the nearest newer rows are read oldest first and answered newest first.
        let t3_seq = second.posts[1].last_message_seq;
        let newer = timeline(&pool, &org, &space, |request| {
            request.limit = 2;
            request.after_seq = t3_seq;
        })
        .await;
        assert_eq!(ids_of(&newer.posts), [t5.as_str(), t4.as_str()], "the two nearest newer, newest first");
        assert!(newer.has_more, "t1 is newer still");
        assert_eq!(newer.total_posts, 0);
        let t4_seq = second.posts[0].last_message_seq;
        let newest = timeline(&pool, &org, &space, |request| {
            request.limit = 10;
            request.after_seq = t4_seq;
        })
        .await;
        assert_eq!(ids_of(&newest.posts), [t1.as_str(), t5.as_str()]);
        assert!(!newest.has_more);
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    async fn since_join_floor_hides_pre_join_posts_titles_previews_pins_and_counts() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let suffix = mp_ids::new_ulid();
        let (org, space) = (format!("org-fl-{suffix}"), format!("space-fl-{suffix}"));
        let before_join = format!("thread-before-{suffix}");
        let after_join = format!("thread-after-{suffix}");
        post(&pool, &org, &space, &before_join).await;
        post(&pool, &org, &space, &after_join).await;
        // The pre-join post: its message predates the join, and it carries a stored title, a stored
        // preview and a pin. The post-join post carries a stored title too.
        sqlx::query("UPDATE messages SET created_at = now() - interval '2 hours' WHERE thread_id = $1")
            .bind(&before_join)
            .execute(&pool)
            .await
            .expect("age the pre-join message");
        sqlx::query(
            "UPDATE threads SET presentation_title = 'Pre-join title', presentation_preview = 'Pre-join preview', \
             pinned_at = now() WHERE id = $1",
        )
        .bind(&before_join)
        .execute(&pool)
        .await
        .expect("the pre-join post carries stored text and a pin");
        sqlx::query("UPDATE threads SET presentation_title = 'Stored title' WHERE id = $1")
            .bind(&after_join)
            .execute(&pool)
            .await
            .expect("the post-join post carries a stored title");
        let join = chrono::Utc::now() - chrono::Duration::hours(1);

        // A member who joined an hour ago sees the post-join post, and nothing of the other.
        let answer = timeline_as(&pool, &org, &space, Some(join), |request| request.include_pinned = true).await;
        assert_eq!(ids_of(&answer.posts), [after_join.as_str()], "{NEEDS_DB}");
        assert_eq!(answer.posts[0].title, "canary body", "the visible text, never the stored title");
        assert_eq!(answer.posts[0].preview, "canary body");
        assert!(answer.pinned.is_empty(), "a pin on a thread that predates the join is not shown");
        assert_eq!(answer.total_posts, 1, "the count is of the posts this reader can see");

        // Without a floor the room is whole, and the stored text reads as before.
        let whole = timeline(&pool, &org, &space, |request| request.include_pinned = true).await;
        assert_eq!(whole.total_posts, 2);
        assert_eq!(whole.pinned.len(), 1);
        let older = whole
            .posts
            .iter()
            .find(|summary| summary.thread_id == before_join)
            .expect("the pre-join post");
        assert_eq!(older.title, "Pre-join title");
        clean_space(&pool, &org, &space).await;
    }
}
