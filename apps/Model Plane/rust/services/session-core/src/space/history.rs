//! `SpaceHistoryService`: permission-checked message search and message address resolution for
//! one Space room (build plan WS13-S1).
//!
//! Authority, in order: a user principal (a service identity has no user id and is refused);
//! `session:read` (a ZDR credential may search: both RPCs are reads and persist, cache and log
//! nothing); the caller's org and user; `SPACE_MESSAGE_SEARCH_CORE`; request validation; the
//! Control read decision (`Some` = the room up to the decision's audience ceiling,
//! `coverage = "shared"`; `None` = the caller's own threads in the Space only,
//! `coverage = "own_threads_only"`); the membership revocation row; then one SQL statement under
//! a permit and a statement timeout. The index is never the access boundary: every row is still
//! checked against the Space, the org, the audience ceiling (thread AND message) and, without a
//! decision, the thread owner.
//!
//! Query text, snippets and hit ids never reach a log line. Snippets are returned as text
//! segments split on a per-request nonce, never as markup.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use mp_contracts::model_plane::v1 as pb;
use mp_contracts::model_plane::v1::space_history_service_server::SpaceHistoryService;
use sqlx::PgPool;
use std::time::Instant;
use tokio::sync::Semaphore;
use tonic::metadata::MetadataValue;
use tonic::{Request, Response, Status};
use tracing::warn;

use crate::auth::{authorize_operation, identity};
use crate::grpc::{
    compact_thread_text, record_metrics, to_proto_timestamp, verify_thread_read_space_decision,
    THREAD_TITLE_MAX_CHARS,
};
use crate::run_phase::ERROR_CODE_TRAILER;

const MAX_SPACE_ID_BYTES: usize = 128;
const MAX_ID_BYTES: usize = 128;
const MIN_QUERY_CHARS: usize = 2;
const MAX_QUERY_CHARS: usize = 200;
const MAX_QUERY_WORDS: usize = 8;
const MAX_WORD_CHARS: usize = 64;
const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 50;
/// Relevance order stops here: a ranked list nobody pages through 100 deep is cut and flagged.
const RELEVANCE_HORIZON: i64 = 100;
const MAX_CURSOR_BYTES: usize = 512;
/// Searches that may run at once in this process; the next one is told to retry.
const SEARCH_PERMITS: usize = 8;
const SEARCH_TIMEOUT_SQL: &str = "SET LOCAL statement_timeout = '2500ms'";
/// Postgres `query_canceled`, raised when the timeout above fires.
const QUERY_CANCELED: &str = "57014";

static PERMITS: Semaphore = Semaphore::const_new(SEARCH_PERMITS);

/// The indexed expression. It must match migration 0077's index definition character for
/// character (the planner matches expressions structurally), so it exists once, here.
pub(crate) const FTS_EXPR: &str = "to_tsvector('simple', left(m.content, 100000))";

/// Every closed word `excluded` may carry.
const EXCLUDED_ZDR_WAIVED: &str = "zdr_waived_threads";
const EXCLUDED_NO_TERMS: &str = "no_searchable_terms";

fn coded(mut status: Status, code: &'static str) -> Status {
    status
        .metadata_mut()
        .insert(ERROR_CODE_TRAILER, MetadataValue::from_static(code));
    status
}

/// Handler state for the Space history RPCs.
#[derive(Clone)]
pub(crate) struct SpaceHistoryGrpc {
    pool: PgPool,
    /// `SPACE_MESSAGE_SEARCH_CORE`, read once at boot. Off, every call is refused before a row
    /// is read. A rollout control, never an authorization input.
    enabled: bool,
}

impl SpaceHistoryGrpc {
    pub(crate) fn new(pool: PgPool, enabled: bool) -> Self {
        Self { pool, enabled }
    }
}

// ---------------------------------------------------------------------------------------------
// Query, cursor and snippet helpers (pure).
// ---------------------------------------------------------------------------------------------

/// The query as a `tsquery`: words are the alphanumeric runs, each quoted, joined with AND. With
/// `prefix` the trailing word also matches as a prefix (the user is still typing it). Anything
/// that is not alphanumeric is a separator, so no operator or quote can come from the input.
/// `None` when the query holds no word.
pub(crate) fn build_tsquery(query: &str, prefix: bool) -> Option<String> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(MAX_QUERY_WORDS)
        .map(|word| word.chars().take(MAX_WORD_CHARS).collect())
        .collect();
    let last = words.len().checked_sub(1)?;
    let terms: Vec<String> = words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            if prefix && index == last {
                format!("'{word}':*")
            } else {
                format!("'{word}'")
            }
        })
        .collect();
    Some(terms.join(" & "))
}

fn clamp_limit(limit: u32) -> i64 {
    let limit = if limit == 0 { DEFAULT_LIMIT } else { limit.clamp(1, MAX_LIMIT) };
    i64::from(limit)
}

/// Where the previous page ended. `recent` pages by `(created_at, id)`; `relevance` pages by
/// offset into the ranked list (rank is not a stable key).
#[derive(Debug, PartialEq)]
enum SearchCursor {
    Recent { created_at: DateTime<Utc>, message_id: String },
    Relevance { offset: i64 },
}

fn encode_cursor(cursor: &SearchCursor) -> String {
    let body = match cursor {
        SearchCursor::Recent { created_at, message_id } => serde_json::json!({
            "s": "recent",
            "c": created_at.to_rfc3339_opts(SecondsFormat::Micros, true),
            "m": message_id,
        }),
        SearchCursor::Relevance { offset } => serde_json::json!({"s": "relevance", "o": offset}),
    };
    URL_SAFE_NO_PAD.encode(body.to_string())
}

/// A cursor carries a position only. Every request re-applies the audience, org, Space and
/// filter predicates, so a forged or replayed cursor can move the window but never widen it.
#[allow(clippy::result_large_err)]
fn decode_cursor(raw: &str) -> Result<SearchCursor, Status> {
    let invalid = || coded(Status::invalid_argument("cursor is not a Space search cursor"), "invalid_cursor");
    if raw.len() > MAX_CURSOR_BYTES {
        return Err(invalid());
    }
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    match value["s"].as_str() {
        Some("recent") => {
            let created_at = value["c"]
                .as_str()
                .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
                .map(|parsed| parsed.with_timezone(&Utc))
                .ok_or_else(invalid)?;
            let message_id = value["m"]
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= MAX_ID_BYTES)
                .ok_or_else(invalid)?
                .to_owned();
            Ok(SearchCursor::Recent { created_at, message_id })
        }
        Some("relevance") => {
            let offset = value["o"]
                .as_i64()
                .filter(|offset| (0..=RELEVANCE_HORIZON).contains(offset))
                .ok_or_else(invalid)?;
            Ok(SearchCursor::Relevance { offset })
        }
        _ => Err(invalid()),
    }
}

/// Splits `ts_headline` output on the per-request markers into text segments. A marker that is
/// unbalanced or repeated is dropped, so a message that contains marker-like text cannot turn
/// ordinary words into a highlight or leave a marker in the output.
pub(crate) fn parse_snippet(raw: &str, start: &str, stop: &str) -> Vec<pb::SnippetSegment> {
    let mut segments: Vec<pb::SnippetSegment> = Vec::new();
    let mut push = |text: &str, matched: bool| {
        if text.is_empty() {
            return;
        }
        match segments.last_mut() {
            Some(last) if last.matched == matched => last.text.push_str(text),
            _ => segments.push(pb::SnippetSegment { text: text.to_owned(), matched }),
        }
    };
    let mut rest = raw;
    while !rest.is_empty() {
        match rest.find(start) {
            Some(open) => {
                push(&rest[..open].replace(stop, ""), false);
                let after = &rest[open + start.len()..];
                match after.find(stop) {
                    Some(close) => {
                        push(&after[..close].replace(start, ""), true);
                        rest = &after[close + stop.len()..];
                    }
                    None => {
                        push(&after.replace(start, ""), false);
                        break;
                    }
                }
            }
            None => {
                push(&rest.replace(stop, ""), false);
                break;
            }
        }
    }
    segments
}

// ---------------------------------------------------------------------------------------------
// SQL.
// ---------------------------------------------------------------------------------------------

/// The predicates every hit must satisfy. Parameters: $1 org, $2 Space, $3 user (owner path),
/// $4 audience ceiling (`NULL` = no decision, owner-only), $5 tsquery, $6 thread filter,
/// $7 author filter, $8 role filter, $9 since, $10 until.
///
/// `m.space_id = $2` with the role list is the index's partial predicate; the Space equality is
/// stated on the thread and the message, the ceiling on both. A message that has any revision
/// row is excluded (retain-mode originals of an edited or deleted message stay in
/// `messages.content`, so they must never be found). Threads that hold a `zdr_waived` run are
/// added by the page query and reported by [`WAIVED_EXISTS_SQL`].
fn base_predicates() -> String {
    format!(
        "m.space_id = $2 AND m.space_id IS NOT NULL AND m.role IN ('user', 'assistant')
      AND {FTS_EXPR} @@ to_tsquery('simple', $5)
      AND t.org_id = $1 AND t.space_id = $2 AND t.archived_at IS NULL
      AND (CASE WHEN $4::BIGINT IS NULL
                THEN t.user_id = $3
                ELSE (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4::BIGINT)
                     AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $4::BIGINT)
           END)
      AND (NULLIF($6, '') IS NULL OR t.id = $6)
      AND (NULLIF($7, '') IS NULL OR m.author_subject_id = $7)
      AND (NULLIF($8, '') IS NULL OR m.role = $8)
      AND ($9::TIMESTAMPTZ IS NULL OR m.created_at >= $9::TIMESTAMPTZ)
      AND ($10::TIMESTAMPTZ IS NULL OR m.created_at < $10::TIMESTAMPTZ)
      AND NOT EXISTS (SELECT 1 FROM message_revisions r WHERE r.message_id = m.id)"
    )
}

const WAIVED_RUN_SQL: &str =
    "EXISTS (SELECT 1 FROM runs wr WHERE wr.thread_id = t.id AND wr.retention_class = 'zdr_waived')";

/// The page. Extra parameters: $11/$12 keyset position, $13 offset (relevance), $14 rows to
/// read, $15/$16 snippet markers. `ts_headline` runs on the page only, after the limit.
fn search_sql(relevance: bool) -> String {
    let (score, order) = if relevance {
        (
            format!("ts_rank_cd({FTS_EXPR}, to_tsquery('simple', $5))"),
            "score DESC, m.created_at DESC, m.id DESC",
        )
    } else {
        ("0::FLOAT4".to_owned(), "m.created_at DESC, m.id DESC")
    };
    let base = base_predicates();
    format!(
        "WITH page AS (
    SELECT m.id AS message_id, m.thread_id, m.role, m.author_subject_id, m.agent_name,
           m.created_at, left(m.content, 100000) AS body, {score} AS score,
           t.session_key, t.presentation_title
    FROM messages m
    JOIN threads t ON t.id = m.thread_id
    WHERE {base}
      AND NOT {WAIVED_RUN_SQL}
      AND ($11::TIMESTAMPTZ IS NULL OR (m.created_at, m.id) < ($11::TIMESTAMPTZ, $12::TEXT))
    ORDER BY {order}
    LIMIT $14 OFFSET $13
)
SELECT p.message_id, p.thread_id, p.role, p.author_subject_id, p.agent_name, p.created_at,
       p.session_key, p.presentation_title,
       ts_headline(
           'simple',
           replace(replace(replace(replace(p.body, chr(57344), ''), chr(57345), ''), chr(57346), ''), chr(57347), ''),
           to_tsquery('simple', $5),
           'StartSel=' || $15::TEXT || ', StopSel=' || $16::TEXT
               || ', MaxFragments=1, MaxWords=24, MinWords=8'
       ) AS snippet
FROM page p
ORDER BY p.score DESC, p.created_at DESC, p.message_id DESC"
    )
}

/// Whether any matching message sits in a thread withheld because it holds a `zdr_waived` run.
/// Same $1..$10 as the page.
fn waived_exists_sql() -> String {
    let base = base_predicates();
    format!(
        "SELECT EXISTS (
    SELECT 1 FROM messages m JOIN threads t ON t.id = m.thread_id
    WHERE {base} AND {WAIVED_RUN_SQL}
)"
    )
}

/// Message address: the thread of one message under the same Space, org, ceiling and owner
/// predicates. A deleted message is not addressable; an edited one is. Parameters: $1 org,
/// $2 Space, $3 user, $4 ceiling (`NULL` = owner-only), $5 message id.
const RESOLVE_SQL: &str = "
SELECT m.thread_id
FROM messages m
JOIN threads t ON t.id = m.thread_id
WHERE m.id = $5
  AND m.space_id = $2
  AND m.role IN ('user', 'assistant')
  AND t.org_id = $1
  AND t.space_id = $2
  AND (CASE WHEN $4::BIGINT IS NULL
            THEN t.user_id = $3
            ELSE (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4::BIGINT)
                 AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $4::BIGINT)
       END)
  AND NOT EXISTS (SELECT 1 FROM message_revisions r WHERE r.message_id = m.id AND r.action = 'deleted')";

type SearchRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    DateTime<Utc>,
    String,
    Option<String>,
    Option<String>,
);

fn map_db_error(error: sqlx::Error) -> Status {
    let timed_out = error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code == QUERY_CANCELED);
    if timed_out {
        return coded(Status::deadline_exceeded("search timed out"), "search_timeout");
    }
    // The error text never carries the query (parameters are bound), but keep the log line to
    // the kind of failure only.
    warn!(error = %error, "space history query failed");
    coded(Status::unavailable("search is unavailable"), "search_unavailable")
}

/// What the caller is allowed to read, resolved once per request.
struct Reach {
    org_id: String,
    user_id: String,
    space_id: String,
    /// `Some` = a verified decision (shared coverage up to this revision); `None` = owner-only.
    ceiling: Option<i64>,
}

impl SpaceHistoryGrpc {
    fn require_enabled(&self) -> Result<(), Status> {
        if self.enabled {
            Ok(())
        } else {
            Err(coded(Status::unavailable("Space message search is disabled"), "space_search_disabled"))
        }
    }

    /// Identity, scope, org/user binding, decision and revocation: the shared front half of both
    /// RPCs. Nothing is read from `messages` before this returns.
    #[allow(clippy::too_many_arguments)]
    async fn authorize<T>(
        &self,
        request: &Request<T>,
        org_id: &str,
        user_id: &str,
        space_id: &str,
        decision_ref: &str,
        decision_token: &str,
    ) -> Result<Reach, Status> {
        let caller = identity(request)?;
        authorize_operation(&caller, "session:read")?;
        caller.authorize_org(org_id)?;
        // A user principal only: the decision's subject is this very user and a service
        // identity has no user id to bind it to.
        let caller_user = caller
            .user_id()
            .map(str::to_owned)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| Status::permission_denied("Space history requires a user principal"))?;
        if user_id.trim().is_empty() || user_id != caller_user {
            return Err(Status::permission_denied("user_id does not match the caller"));
        }
        self.require_enabled()?;
        let space_id = space_id.trim();
        if space_id.is_empty() || space_id.len() > MAX_SPACE_ID_BYTES {
            return Err(Status::invalid_argument("space_id is required"));
        }
        let ceiling = verify_thread_read_space_decision(
            decision_token,
            decision_ref,
            org_id,
            user_id,
            space_id,
        )?
        .map(|read| read.recipient_audience_revision);
        let revoked: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2)",
        )
        .bind(space_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)?;
        if revoked.0 {
            return Err(coded(
                Status::permission_denied("Space membership required"),
                "active_membership_required",
            ));
        }
        Ok(Reach {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            space_id: space_id.to_owned(),
            ceiling,
        })
    }

    async fn search(
        &self,
        request: Request<pb::SearchSpaceMessagesRequest>,
    ) -> Result<Response<pb::SearchSpaceMessagesResponse>, Status> {
        let caller_view = (
            request.get_ref().org_id.clone(),
            request.get_ref().user_id.clone(),
            request.get_ref().space_id.clone(),
            request.get_ref().space_read_decision_ref.clone(),
            request.get_ref().space_read_decision_token.clone(),
        );
        let reach = self
            .authorize(&request, &caller_view.0, &caller_view.1, &caller_view.2, &caller_view.3, &caller_view.4)
            .await?;
        let req = request.into_inner();

        // Validation: query, filters, sort, window, cursor.
        let query = req.query.trim();
        let query_chars = query.chars().count();
        if !(MIN_QUERY_CHARS..=MAX_QUERY_CHARS).contains(&query_chars) || query.chars().any(char::is_control) {
            return Err(coded(
                Status::invalid_argument("query must be 2 to 200 characters"),
                "invalid_query",
            ));
        }
        let invalid_filter = |what: &'static str| coded(Status::invalid_argument(what), "invalid_filter");
        if !matches!(req.role.as_str(), "" | "user" | "assistant") {
            return Err(invalid_filter("role must be user or assistant"));
        }
        let relevance = match req.sort.as_str() {
            "" | "recent" => false,
            "relevance" => true,
            _ => return Err(invalid_filter("sort must be recent or relevance")),
        };
        if req.thread_id.len() > MAX_ID_BYTES || req.author_subject_id.len() > MAX_ID_BYTES {
            return Err(invalid_filter("filter id is too long"));
        }
        let to_time = |ts: &Option<prost_types::Timestamp>| -> Result<Option<DateTime<Utc>>, Status> {
            match ts {
                None => Ok(None),
                Some(ts) => DateTime::<Utc>::from_timestamp(ts.seconds, u32::try_from(ts.nanos).unwrap_or(0))
                    .map(Some)
                    .ok_or_else(|| invalid_filter("since or until is out of range")),
            }
        };
        let since = to_time(&req.since)?;
        let until = to_time(&req.until)?;
        if let (Some(since), Some(until)) = (since, until) {
            if since >= until {
                return Err(invalid_filter("since must be before until"));
            }
        }
        let cursor = if req.cursor.is_empty() { None } else { Some(decode_cursor(&req.cursor)?) };
        let (key_at, key_id, offset) = match (&cursor, relevance) {
            (None, _) => (None, None, 0),
            (Some(SearchCursor::Recent { created_at, message_id }), false) => {
                (Some(*created_at), Some(message_id.clone()), 0)
            }
            (Some(SearchCursor::Relevance { offset }), true) => (None, None, *offset),
            _ => {
                return Err(coded(
                    Status::invalid_argument("cursor does not belong to this sort"),
                    "invalid_cursor",
                ))
            }
        };

        let mut response = pb::SearchSpaceMessagesResponse {
            coverage: if reach.ceiling.is_some() { "shared" } else { "own_threads_only" }.to_owned(),
            ..Default::default()
        };
        let Some(tsquery) = build_tsquery(query, req.prefix) else {
            response.excluded.push(EXCLUDED_NO_TERMS.to_owned());
            return Ok(Response::new(response));
        };

        let limit = clamp_limit(req.limit);
        // Relevance stops at the horizon; a page never reads past it.
        let page_size = if relevance { limit.min(RELEVANCE_HORIZON - offset).max(0) } else { limit };
        if page_size == 0 {
            response.truncated = true;
            return Ok(Response::new(response));
        }

        let _permit = PERMITS
            .try_acquire()
            .map_err(|_| coded(Status::resource_exhausted("search is busy"), "search_busy"))?;

        // Per-request markers: a random nonce keeps message text from forging a highlight.
        let nonce = mp_ids::new_ulid();
        let start_marker = format!("\u{E000}{nonce}\u{E001}");
        let stop_marker = format!("\u{E002}{nonce}\u{E003}");

        let mut tx = self.pool.begin().await.map_err(map_db_error)?;
        sqlx::query(SEARCH_TIMEOUT_SQL).execute(&mut *tx).await.map_err(map_db_error)?;
        let mut rows: Vec<SearchRow> = sqlx::query_as(&search_sql(relevance))
            .bind(&reach.org_id)
            .bind(&reach.space_id)
            .bind(&reach.user_id)
            .bind(reach.ceiling)
            .bind(&tsquery)
            .bind(&req.thread_id)
            .bind(&req.author_subject_id)
            .bind(&req.role)
            .bind(since)
            .bind(until)
            .bind(key_at)
            .bind(key_id)
            .bind(offset)
            .bind(page_size + 1)
            .bind(&start_marker)
            .bind(&stop_marker)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_db_error)?;
        let (waived,): (bool,) = sqlx::query_as(&waived_exists_sql())
            .bind(&reach.org_id)
            .bind(&reach.space_id)
            .bind(&reach.user_id)
            .bind(reach.ceiling)
            .bind(&tsquery)
            .bind(&req.thread_id)
            .bind(&req.author_subject_id)
            .bind(&req.role)
            .bind(since)
            .bind(until)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_db_error)?;
        tx.commit().await.map_err(map_db_error)?;

        let page_len = usize::try_from(page_size).unwrap_or(usize::MAX);
        let has_more = rows.len() > page_len;
        if has_more {
            let last = &rows[page_len - 1];
            if relevance {
                if offset + page_size >= RELEVANCE_HORIZON {
                    response.truncated = true;
                } else {
                    response.next_cursor = encode_cursor(&SearchCursor::Relevance { offset: offset + page_size });
                }
            } else {
                response.next_cursor = encode_cursor(&SearchCursor::Recent {
                    created_at: last.5,
                    message_id: last.0.clone(),
                });
            }
        }
        rows.truncate(page_len);

        response.hits = rows
            .into_iter()
            .map(|(message_id, thread_id, role, author, agent, created_at, session_key, title, snippet)| {
                let fallback = if session_key.trim().is_empty() { "Verevon Chat" } else { session_key.as_str() };
                pb::SpaceMessageHit {
                    message_id,
                    thread_id,
                    space_id: reach.space_id.clone(),
                    role,
                    author_subject_id: author.unwrap_or_default(),
                    agent_name: agent.unwrap_or_default(),
                    created_at: Some(to_proto_timestamp(created_at)),
                    snippet: parse_snippet(&snippet.unwrap_or_default(), &start_marker, &stop_marker),
                    thread_title: compact_thread_text(title, fallback, THREAD_TITLE_MAX_CHARS),
                }
            })
            .collect();
        if waived {
            response.excluded.push(EXCLUDED_ZDR_WAIVED.to_owned());
        }
        Ok(Response::new(response))
    }

    async fn resolve(
        &self,
        request: Request<pb::ResolveSpaceMessageRequest>,
    ) -> Result<Response<pb::ResolveSpaceMessageResponse>, Status> {
        let view = (
            request.get_ref().org_id.clone(),
            request.get_ref().user_id.clone(),
            request.get_ref().space_id.clone(),
            request.get_ref().space_read_decision_ref.clone(),
            request.get_ref().space_read_decision_token.clone(),
        );
        let reach = self.authorize(&request, &view.0, &view.1, &view.2, &view.3, &view.4).await?;
        let message_id = request.into_inner().message_id;
        let message_id = message_id.trim();
        // One answer for every way a message can be unreachable.
        let not_found = || coded(Status::not_found("entry not found"), "entry_not_found");
        if message_id.is_empty() || message_id.len() > MAX_ID_BYTES {
            return Err(not_found());
        }
        let _permit = PERMITS
            .try_acquire()
            .map_err(|_| coded(Status::resource_exhausted("search is busy"), "search_busy"))?;
        let mut tx = self.pool.begin().await.map_err(map_db_error)?;
        sqlx::query(SEARCH_TIMEOUT_SQL).execute(&mut *tx).await.map_err(map_db_error)?;
        let row: Option<(String,)> = sqlx::query_as(RESOLVE_SQL)
            .bind(&reach.org_id)
            .bind(&reach.space_id)
            .bind(&reach.user_id)
            .bind(reach.ceiling)
            .bind(message_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_db_error)?;
        tx.commit().await.map_err(map_db_error)?;
        let (thread_id,) = row.ok_or_else(not_found)?;
        Ok(Response::new(pb::ResolveSpaceMessageResponse {
            message_id: message_id.to_owned(),
            thread_id,
            space_id: reach.space_id,
        }))
    }
}

#[tonic::async_trait]
impl SpaceHistoryService for SpaceHistoryGrpc {
    async fn search_messages(
        &self,
        request: Request<pb::SearchSpaceMessagesRequest>,
    ) -> Result<Response<pb::SearchSpaceMessagesResponse>, Status> {
        let started = Instant::now();
        let result = self.search(request).await;
        record_metrics("space_search_messages", started, result.is_ok());
        result
    }

    async fn resolve_message(
        &self,
        request: Request<pb::ResolveSpaceMessageRequest>,
    ) -> Result<Response<pb::ResolveSpaceMessageResponse>, Status> {
        let started = Instant::now();
        let result = self.resolve(request).await;
        record_metrics("space_resolve_message", started, result.is_ok());
        result
    }
}
