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
//! checked against the Space, the org, the audience ceiling (thread AND message), the message's
//! history floors (WS04-S11 revision floor, D18 timestamp floor) and, without a decision, the thread
//! owner.
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
use crate::space::audience_window::{audience_window, AudienceWindow};

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
/// $7 author filter, $8 role filter, $9 since, $10 until, and the two history floors as the
/// placeholders `floor_rev` (the revision floor) and `floor_at` (the D18 timestamp floor), each
/// `NULL` when unset.
///
/// `m.space_id = $2` with the role list is the index's partial predicate; the Space equality is
/// stated on the thread and the message, the ceiling on both. The floors are the conversation
/// reads' predicates on the message: a NULL-stamped row passes the revision floor, and the
/// timestamp floor hides a row created before it. A message that has any revision
/// row is excluded (retain-mode originals of an edited or deleted message stay in
/// `messages.content`, so they must never be found). Threads that hold a `zdr_waived` run are
/// added by the page query and reported by [`WAIVED_EXISTS_SQL`].
fn base_predicates(floor_rev: &str, floor_at: &str) -> String {
    format!(
        "m.space_id = $2 AND m.space_id IS NOT NULL AND m.role IN ('user', 'assistant')
      AND {FTS_EXPR} @@ to_tsquery('simple', $5)
      AND t.org_id = $1 AND t.space_id = $2 AND t.archived_at IS NULL
      AND (CASE WHEN $4::BIGINT IS NULL
                THEN t.user_id = $3
                ELSE (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $4::BIGINT)
                     AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $4::BIGINT)
           END)
      AND ({floor_rev}::BIGINT IS NULL OR m.recipient_audience_revision IS NULL
           OR m.recipient_audience_revision >= {floor_rev}::BIGINT)
      AND ({floor_at}::TIMESTAMPTZ IS NULL OR m.created_at >= {floor_at}::TIMESTAMPTZ)
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
/// read, $15/$16 snippet markers, $17/$18 the history floors (under a floor the stored thread title
/// is not shown). `ts_headline` runs on the page only, after the limit.
fn search_sql(relevance: bool) -> String {
    let (score, order) = if relevance {
        (
            format!("ts_rank_cd({FTS_EXPR}, to_tsquery('simple', $5))"),
            "score DESC, m.created_at DESC, m.id DESC",
        )
    } else {
        ("0::FLOAT4".to_owned(), "m.created_at DESC, m.id DESC")
    };
    let base = base_predicates("$17", "$18");
    format!(
        "WITH page AS (
    SELECT m.id AS message_id, m.thread_id, m.role, m.author_subject_id, m.agent_name,
           m.created_at, left(m.content, 100000) AS body, {score} AS score,
           t.session_key,
           CASE WHEN $17::BIGINT IS NOT NULL OR $18::TIMESTAMPTZ IS NOT NULL
                THEN NULL ELSE t.presentation_title END AS presentation_title
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
/// Same $1..$10 as the page, then the two history floors as $11 and $12.
fn waived_exists_sql() -> String {
    let base = base_predicates("$11", "$12");
    format!(
        "SELECT EXISTS (
    SELECT 1 FROM messages m JOIN threads t ON t.id = m.thread_id
    WHERE {base} AND {WAIVED_RUN_SQL}
)"
    )
}

/// Message address: the thread of one message under the same Space, org, ceiling, floors and owner
/// predicates. A deleted message is not addressable; an edited one is. Parameters: $1 org,
/// $2 Space, $3 user, $4 ceiling (`NULL` = owner-only), $5 message id, $6 revision floor and $7
/// timestamp floor (`NULL` = none). A message a floor hides answers as a missing one does.
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
  AND ($6::BIGINT IS NULL OR m.recipient_audience_revision IS NULL
       OR m.recipient_audience_revision >= $6::BIGINT)
  AND ($7::TIMESTAMPTZ IS NULL OR m.created_at >= $7::TIMESTAMPTZ)
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
    /// The audience window of the verified decision: shared coverage up to its ceiling, with its
    /// history floors. An owner-only read (no decision) is unbounded.
    window: AudienceWindow,
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
        let window = audience_window(verify_thread_read_space_decision(
            decision_token,
            decision_ref,
            org_id,
            user_id,
            space_id,
        )?);
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
            window,
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
            coverage: if reach.window.ceiling.is_some() { "shared" } else { "own_threads_only" }.to_owned(),
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
            .bind(reach.window.ceiling)
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
            .bind(reach.window.floor)
            .bind(reach.window.floor_at)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_db_error)?;
        let (waived,): (bool,) = sqlx::query_as(&waived_exists_sql())
            .bind(&reach.org_id)
            .bind(&reach.space_id)
            .bind(&reach.user_id)
            .bind(reach.window.ceiling)
            .bind(&tsquery)
            .bind(&req.thread_id)
            .bind(&req.author_subject_id)
            .bind(&req.role)
            .bind(since)
            .bind(until)
            .bind(reach.window.floor)
            .bind(reach.window.floor_at)
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
            .bind(reach.window.ceiling)
            .bind(message_id)
            .bind(reach.window.floor)
            .bind(reach.window.floor_at)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    // ---- the query as a tsquery -------------------------------------------------------------

    #[test]
    fn build_tsquery_quotes_each_word_and_joins_them_with_and() {
        // (query, prefix, expected)
        let table: &[(&str, bool, Option<&str>)] = &[
            ("hello", false, Some("'hello'")),
            ("hello", true, Some("'hello':*")),
            ("hello world", false, Some("'hello' & 'world'")),
            // Only the last word is a prefix: the user is still typing it.
            ("hello world", true, Some("'hello' & 'world':*")),
            ("  hello   world  ", true, Some("'hello' & 'world':*")),
            // Case is kept; the `simple` configuration folds it on the database side.
            ("Hello WORLD", false, Some("'Hello' & 'WORLD'")),
            // Digits count as word characters, and so do non-ASCII letters.
            ("v2 release", false, Some("'v2' & 'release'")),
            ("na\u{ef}ve caf\u{e9}", false, Some("'na\u{ef}ve' & 'caf\u{e9}'")),
            // Everything that is not alphanumeric is a separator.
            ("release-2026", false, Some("'release' & '2026'")),
            ("snake_case", false, Some("'snake' & 'case'")),
            ("foo & bar | !baz", false, Some("'foo' & 'bar' & 'baz'")),
            ("(foo) <-> bar", false, Some("'foo' & 'bar'")),
            ("foo:*bar", false, Some("'foo' & 'bar'")),
            ("o'brien", false, Some("'o' & 'brien'")),
            ("a\\b", false, Some("'a' & 'b'")),
            // No word at all.
            ("", false, None),
            ("   ", true, None),
            ("&|!()", false, None),
            ("'''", true, None),
            ("-- :: <->", false, None),
        ];
        for (query, prefix, expected) in table {
            assert_eq!(
                build_tsquery(query, *prefix).as_deref(),
                *expected,
                "query {query:?}, prefix {prefix}"
            );
        }
    }

    #[test]
    fn build_tsquery_keeps_at_most_eight_words_of_sixty_four_characters() {
        let nine = (1..=9).map(|n| format!("w{n}")).collect::<Vec<_>>().join(" ");
        let capped = "'w1' & 'w2' & 'w3' & 'w4' & 'w5' & 'w6' & 'w7' & 'w8'";
        assert_eq!(build_tsquery(&nine, false).as_deref(), Some(capped));
        // The prefix marker lands on the last word that is kept, never on a dropped one.
        assert_eq!(
            build_tsquery(&nine, true).as_deref(),
            Some("'w1' & 'w2' & 'w3' & 'w4' & 'w5' & 'w6' & 'w7' & 'w8':*")
        );
        // A word is cut to 64 characters, counted as characters and not bytes.
        assert_eq!(
            build_tsquery(&"a".repeat(70), false),
            Some(format!("'{}'", "a".repeat(64)))
        );
        assert_eq!(
            build_tsquery(&"\u{e9}".repeat(70), true),
            Some(format!("'{}':*", "\u{e9}".repeat(64)))
        );
        assert_eq!(
            build_tsquery(&"b".repeat(64), false),
            Some(format!("'{}'", "b".repeat(64))),
            "exactly 64 characters is kept whole"
        );
    }

    #[test]
    fn build_tsquery_never_lets_input_become_query_syntax() {
        for hostile in [
            "'",
            "' | '",
            "foo' | 'bar",
            "a & !b",
            "a <-> b",
            "a:*",
            "\\",
            "x\u{0}y",
            "a\nb",
            "$1 $2",
            "100%",
            "{}[]",
            "a\"b",
            "'; DROP TABLE messages; --",
        ] {
            for prefix in [false, true] {
                let Some(query) = build_tsquery(hostile, prefix) else { continue };
                for term in query.split(" & ") {
                    let term = term.strip_suffix(":*").unwrap_or(term);
                    let inner = term
                        .strip_prefix('\'')
                        .and_then(|rest| rest.strip_suffix('\''))
                        .unwrap_or_else(|| panic!("{hostile:?} produced an unquoted term {term:?}"));
                    assert!(
                        !inner.is_empty() && inner.chars().all(char::is_alphanumeric),
                        "{hostile:?} produced a term with syntax in it: {term:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_page_limit_defaults_and_clamps() {
        assert_eq!(clamp_limit(0), i64::from(DEFAULT_LIMIT), "zero asks for the default");
        assert_eq!(DEFAULT_LIMIT, 20);
        assert_eq!(MAX_LIMIT, 50);
        assert_eq!(clamp_limit(1), 1);
        assert_eq!(clamp_limit(20), 20);
        assert_eq!(clamp_limit(MAX_LIMIT), i64::from(MAX_LIMIT));
        assert_eq!(clamp_limit(MAX_LIMIT + 1), i64::from(MAX_LIMIT));
        assert_eq!(clamp_limit(u32::MAX), i64::from(MAX_LIMIT));
    }

    #[test]
    fn the_search_expression_is_the_one_migration_0077_indexes() {
        // The planner matches the indexed expression structurally, so the query must say the same
        // thing the index does; the only difference is the table alias.
        let migration = include_str!("../../migrations/0077_space_message_search_index.sql");
        let indexed = FTS_EXPR.replace("m.content", "content");
        assert!(
            migration.contains(&format!("USING GIN ({indexed})")),
            "FTS_EXPR {FTS_EXPR:?} drifted from the migration's index"
        );
        assert!(migration.starts_with("-- no-transaction"), "CONCURRENTLY needs its own transaction");
        assert!(migration.contains("WHERE space_id IS NOT NULL AND role IN ('user', 'assistant')"));
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
    fn every_history_statement_names_exactly_the_binds_it_carries() {
        // The search page binds 18 (the scope, the query, the filters, the keyset, the page, the
        // markers and the two floors), the waived check 12 and the address 7. A bind that no clause
        // names, or a clause that names an unbound parameter, fails at run time.
        for (case, sql, binds) in [
            ("search, recent", search_sql(false), 18),
            ("search, relevance", search_sql(true), 18),
            ("waived check", waived_exists_sql(), 12),
            ("address", RESOLVE_SQL.to_owned(), 7),
        ] {
            assert_eq!(placeholders(&sql), (1..=binds).collect::<BTreeSet<usize>>(), "{case}");
        }
    }

    // ---- the cursor -------------------------------------------------------------------------

    fn encode_raw(value: serde_json::Value) -> String {
        URL_SAFE_NO_PAD.encode(value.to_string())
    }

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).expect("test time").with_timezone(&Utc)
    }

    fn error_code(status: &Status) -> Option<&str> {
        status.metadata().get(ERROR_CODE_TRAILER).and_then(|value| value.to_str().ok())
    }

    #[test]
    fn a_recent_cursor_round_trips_to_the_microsecond() {
        let cursor = SearchCursor::Recent {
            created_at: DateTime::from_timestamp(1_800_000_000, 123_456_000).expect("time"),
            message_id: "01HZMESSAGE".to_owned(),
        };
        assert_eq!(decode_cursor(&encode_cursor(&cursor)).expect("round trip"), cursor);
    }

    #[test]
    fn a_relevance_cursor_round_trips_up_to_the_horizon() {
        for offset in [0, 1, 20, RELEVANCE_HORIZON - 1, RELEVANCE_HORIZON] {
            let cursor = SearchCursor::Relevance { offset };
            assert_eq!(decode_cursor(&encode_cursor(&cursor)).expect("round trip"), cursor, "offset {offset}");
        }
    }

    #[test]
    fn a_cursor_is_url_safe_text_inside_the_byte_cap() {
        let encoded = encode_cursor(&SearchCursor::Recent {
            created_at: instant("2026-10-01T00:00:00Z"),
            message_id: "m".repeat(MAX_ID_BYTES),
        });
        assert!(encoded.len() <= MAX_CURSOR_BYTES, "the longest real cursor must still decode: {}", encoded.len());
        assert!(encoded.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{encoded}");
        decode_cursor(&encoded).expect("the longest real cursor decodes");
    }

    #[test]
    fn a_forged_cursor_is_refused_as_invalid_cursor() {
        let recent = |created: serde_json::Value, message: serde_json::Value| {
            encode_raw(json!({"s": "recent", "c": created, "m": message}))
        };
        let when = "2026-10-01T00:00:00Z";
        let cases: Vec<(&str, String)> = vec![
            ("not base64", "!!!".to_owned()),
            ("base64 of text that is not JSON", URL_SAFE_NO_PAD.encode("not json")),
            ("a JSON array", URL_SAFE_NO_PAD.encode("[]")),
            ("a JSON string", encode_raw(json!("recent"))),
            ("an empty object", encode_raw(json!({}))),
            ("an unknown sort word", encode_raw(json!({"s": "oldest", "o": 1}))),
            ("a numeric sort word", encode_raw(json!({"s": 1}))),
            ("recent without a time", encode_raw(json!({"s": "recent", "m": "msg-1"}))),
            ("recent with a bad time", recent(json!("yesterday"), json!("msg-1"))),
            ("recent with a numeric time", recent(json!(1_800_000_000), json!("msg-1"))),
            ("recent without an id", encode_raw(json!({"s": "recent", "c": when}))),
            ("recent with an empty id", recent(json!(when), json!(""))),
            ("recent with an oversize id", recent(json!(when), json!("m".repeat(MAX_ID_BYTES + 1)))),
            ("recent with a non-string id", recent(json!(when), json!(7))),
            ("relevance without an offset", encode_raw(json!({"s": "relevance"}))),
            ("relevance with a negative offset", encode_raw(json!({"s": "relevance", "o": -1}))),
            (
                "relevance past the horizon",
                encode_raw(json!({"s": "relevance", "o": RELEVANCE_HORIZON + 1})),
            ),
            ("relevance with a huge offset", encode_raw(json!({"s": "relevance", "o": i64::MAX}))),
            ("relevance with a fractional offset", encode_raw(json!({"s": "relevance", "o": 1.5}))),
            ("relevance with a text offset", encode_raw(json!({"s": "relevance", "o": "5"}))),
            ("a cursor over the byte cap", "a".repeat(MAX_CURSOR_BYTES + 1)),
        ];
        for (why, raw) in cases {
            let status = decode_cursor(&raw).expect_err(why);
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{why}");
            assert_eq!(error_code(&status), Some("invalid_cursor"), "{why}");
        }
    }

    #[test]
    fn a_cursor_carries_a_position_only_so_extra_keys_cannot_widen_a_search() {
        // Whatever else a forger packs next to the position, the decode keeps the position and
        // nothing else; the audience, org, Space and filter predicates are applied on every request.
        let forged_recent = encode_raw(json!({
            "s": "recent",
            "c": "2026-10-01T00:00:00Z",
            "m": "msg-1",
            "space_id": "space-other",
            "org_id": "org-other",
            "ceiling": 9_999,
            "role": "system",
            "thread_id": "thread-other",
        }));
        assert_eq!(
            decode_cursor(&forged_recent).expect("the position survives"),
            SearchCursor::Recent {
                created_at: instant("2026-10-01T00:00:00Z"),
                message_id: "msg-1".to_owned(),
            }
        );
        let forged_relevance =
            encode_raw(json!({"s": "relevance", "o": 20, "space_id": "space-other", "ceiling": 9_999}));
        assert_eq!(
            decode_cursor(&forged_relevance).expect("the offset survives"),
            SearchCursor::Relevance { offset: 20 }
        );
        // A time with an offset is the same instant in UTC.
        let zoned = encode_raw(json!({"s": "recent", "c": "2026-10-01T02:00:00+02:00", "m": "msg-1"}));
        assert_eq!(
            decode_cursor(&zoned).expect("zoned time"),
            SearchCursor::Recent {
                created_at: instant("2026-10-01T00:00:00Z"),
                message_id: "msg-1".to_owned(),
            }
        );
    }

    // ---- the snippet ------------------------------------------------------------------------

    const START: &str = "\u{E000}n0nce\u{E001}";
    const STOP: &str = "\u{E002}n0nce\u{E003}";

    fn snippet(raw: &str) -> Vec<(String, bool)> {
        let segments = parse_snippet(raw, START, STOP);
        for segment in &segments {
            assert!(
                !segment.text.contains(START) && !segment.text.contains(STOP),
                "a marker leaked into the output: {raw:?} -> {segments:?}"
            );
            assert!(!segment.text.is_empty(), "an empty segment was emitted: {raw:?}");
        }
        segments.into_iter().map(|segment| (segment.text, segment.matched)).collect()
    }

    fn plain(text: &str) -> (String, bool) {
        (text.to_owned(), false)
    }

    fn hit(text: &str) -> (String, bool) {
        (text.to_owned(), true)
    }

    #[test]
    fn a_snippet_splits_on_the_markers_into_text_segments() {
        assert_eq!(snippet(""), Vec::<(String, bool)>::new());
        assert_eq!(snippet("plain words"), vec![plain("plain words")]);
        assert_eq!(
            snippet(&format!("alpha {START}beta{STOP} gamma")),
            vec![plain("alpha "), hit("beta"), plain(" gamma")]
        );
        assert_eq!(snippet(&format!("{START}solo{STOP}")), vec![hit("solo")]);
        assert_eq!(
            snippet(&format!("{START}one{STOP} and {START}two{STOP}")),
            vec![hit("one"), plain(" and "), hit("two")]
        );
        assert_eq!(
            snippet(&format!("\u{e9}{START}\u{fc}{STOP}\u{f1}")),
            vec![plain("\u{e9}"), hit("\u{fc}"), plain("\u{f1}")],
            "multi-byte text next to multi-byte markers splits on character boundaries"
        );
    }

    #[test]
    fn adjacent_segments_of_one_kind_merge_and_empty_ones_vanish() {
        assert_eq!(snippet(&format!("{START}a{STOP}{START}b{STOP}")), vec![hit("ab")]);
        assert_eq!(snippet(&format!("x{START}{STOP}y")), vec![plain("xy")]);
        assert_eq!(snippet(&format!("{STOP}a")), vec![plain("a")]);
    }

    #[test]
    fn marker_like_text_cannot_forge_a_highlight_or_leave_a_marker_behind() {
        // An unbalanced start: the rest is ordinary text and the marker is gone.
        assert_eq!(snippet(&format!("tail {START}never closed")), vec![plain("tail never closed")]);
        assert_eq!(snippet(&format!("a{START}")), vec![plain("a")]);
        // A stop with no start is dropped from plain text, and cannot open anything.
        assert_eq!(snippet(&format!("a {STOP}b")), vec![plain("a b")]);
        assert_eq!(snippet(&format!("a{STOP}b {START}c{STOP}")), vec![plain("ab "), hit("c")]);
        // A repeated start inside a highlight is dropped, not nested.
        assert_eq!(snippet(&format!("{START}a{START}b{STOP}")), vec![hit("ab")]);
        assert_eq!(snippet(&format!("{START}a{START}b")), vec![plain("ab")]);
        // A stop inside the plain text before a real highlight cannot widen the highlight backwards.
        assert_eq!(
            snippet(&format!("one {STOP} two {START}three{STOP} four")),
            vec![plain("one  two "), hit("three"), plain(" four")]
        );
    }

    #[test]
    fn markers_of_another_request_are_ordinary_text() {
        // The nonce is per request, so another nonce's markers can never open a highlight here.
        let foreign = "see \u{E000}other\u{E001}fake\u{E002}other\u{E003} here";
        assert_eq!(snippet(foreign), vec![plain(foreign)]);
    }

    // ---- real Postgres (every test is `#[ignore]`d) -----------------------------------------
    //
    // `cargo test -p session-core --bin session-core space::history -- --ignored --test-threads=1`
    // with `DATABASE_URL` pointing at a disposable Postgres whose database name contains `test`.

    mod db_tests {
        use super::*;
        use crate::auth::VerifiedIdentity;
        use crate::grpc::space_w1_tests::{mint, StreamKeysEnv, DECISION_REF};
        use crate::space::test_support::{clean_space, connect, seed_space_run, seed_space_thread};
        use sqlx::Executor as _;

        const NEEDS_DB: &str = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)";
        /// Owns the threads of a test and, unless said otherwise, wrote every message.
        const OWNER: &str = "owner-1";
        /// A member of the Space who searches it.
        const MEMBER: &str = "member-1";
        /// The audience ceiling of the member's read decision.
        const CEILING: i64 = 3;

        fn read_token(org: &str, user: &str, space: &str, revision: i64, floor_at: Option<DateTime<Utc>>) -> String {
            mint(
                crate::grpc::CONTROL_SPACE_DECISION_AUDIENCE,
                &["thread:read"],
                false,
                |claims| {
                    claims["org_id"] = json!(org);
                    claims["subject_id"] = json!(user);
                    claims["space_ref"] = json!(space);
                    claims["recipient_audience_revision"] = json!(revision);
                    if let Some(at) = floor_at {
                        claims["history_floor_at"] = json!(at.to_rfc3339());
                    }
                    claims["issued_at"] = json!(Utc::now().to_rfc3339());
                    claims["expires_at"] = json!((Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
                },
                false,
            )
        }

        /// What the caller presents: a read decision up to a ceiling (`Since` also carries the D18
        /// timestamp floor, the member's join), or none (own threads only).
        #[derive(Clone, Copy)]
        enum Reach {
            Shared(i64),
            Since(i64, DateTime<Utc>),
            OwnThreads,
        }

        impl Reach {
            /// The decision ref and token this reach presents to the RPC.
            fn presents(self, org: &str, user: &str, space: &str) -> (String, String) {
                match self {
                    Reach::Shared(revision) => (DECISION_REF.to_owned(), read_token(org, user, space, revision, None)),
                    Reach::Since(revision, floor_at) => {
                        (DECISION_REF.to_owned(), read_token(org, user, space, revision, Some(floor_at)))
                    }
                    Reach::OwnThreads => (String::new(), String::new()),
                }
            }
        }

        /// One message to seed.
        struct Msg<'a> {
            thread: &'a str,
            name: &'a str,
            role: &'a str,
            author: Option<&'a str>,
            content: &'a str,
            revision: i64,
            age_secs: i32,
        }

        impl<'a> Msg<'a> {
            /// A user message by [`OWNER`], new, at audience revision 1.
            fn new(thread: &'a str, name: &'a str, content: &'a str) -> Self {
                Self { thread, name, role: "user", author: Some(OWNER), content, revision: 1, age_secs: 0 }
            }
        }

        struct World {
            pool: PgPool,
            org: String,
            space: String,
            suffix: String,
        }

        impl World {
            async fn new(prefix: &str) -> Option<Self> {
                let pool = connect().await?;
                let suffix = mp_ids::new_ulid();
                Some(Self {
                    pool,
                    org: format!("org-{prefix}-{suffix}"),
                    space: format!("space-{prefix}-{suffix}"),
                    suffix,
                })
            }

            async fn thread(&self, name: &str, owner: &str) -> String {
                let id = format!("thread-{name}-{}", self.suffix);
                seed_space_thread(&self.pool, &self.org, &self.space, &id, owner).await;
                id
            }

            /// Inserts `msg` stamped with `space` (a message of another Space for the isolation tests).
            async fn insert_in(&self, space: &str, msg: Msg<'_>) -> String {
                let id = format!("{}-{}", msg.name, self.suffix);
                sqlx::query(
                    "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
                     recipient_audience_revision, recipient_audience_hash, authority_revision, \
                     resource_authorization_ref, author_subject_id, created_at) \
                     VALUES ($1, $2, $3, $4, $5, 'audience-1', $6, 'sha256:audience-1', 1, 'resource-1', $7, \
                             now() - make_interval(secs => $8))",
                )
                .bind(&id)
                .bind(msg.thread)
                .bind(msg.role)
                .bind(msg.content)
                .bind(space)
                .bind(msg.revision)
                .bind(msg.author)
                .bind(f64::from(msg.age_secs))
                .execute(&self.pool)
                .await
                .expect("seed a message");
                id
            }

            async fn say(&self, msg: Msg<'_>) -> String {
                self.insert_in(&self.space, msg).await
            }

            /// Records a revision of `message` (`edited` or `deleted`), which takes it out of search.
            async fn revise(&self, thread: &str, message: &str, action: &str) {
                sqlx::query(
                    "INSERT INTO message_revisions (revision_id, org_id, thread_id, message_id, revision_no, action, \
                     actor_subject_id, actor_class, content, content_digest, prev_content_digest, original_retention, \
                     idempotency_hash, space_id, recipient_audience_revision, authority_revision, resource_authorization_ref) \
                     VALUES ($1, $2, $3, $4, 1, $5, 'owner-1', 'author', $6, 'sha256:new', 'sha256:old', 'retain', $7, $8, 1, 1, 'resource-1')",
                )
                .bind(format!("revision-{message}"))
                .bind(&self.org)
                .bind(thread)
                .bind(message)
                .bind(action)
                .bind((action == "edited").then_some("the replacement words"))
                .bind(format!("idem-{message}"))
                .bind(&self.space)
                .execute(&self.pool)
                .await
                .expect("seed a revision");
            }

            async fn search(
                &self,
                user: &str,
                reach: Reach,
                edit: impl FnOnce(&mut pb::SearchSpaceMessagesRequest),
            ) -> Result<pb::SearchSpaceMessagesResponse, Status> {
                let (decision_ref, token) = reach.presents(&self.org, user, &self.space);
                let mut message = pb::SearchSpaceMessagesRequest {
                    org_id: self.org.clone(),
                    user_id: user.to_owned(),
                    space_id: self.space.clone(),
                    space_read_decision_ref: decision_ref,
                    space_read_decision_token: token,
                    query: "zebra".to_owned(),
                    ..Default::default()
                };
                edit(&mut message);
                let mut request = Request::new(message);
                request.extensions_mut().insert(VerifiedIdentity::user_for_test(&self.org, user));
                SpaceHistoryGrpc::new(self.pool.clone(), true)
                    .search_messages(request)
                    .await
                    .map(Response::into_inner)
            }

            async fn resolve(&self, user: &str, reach: Reach, message_id: &str) -> Result<pb::ResolveSpaceMessageResponse, Status> {
                let (decision_ref, token) = reach.presents(&self.org, user, &self.space);
                let mut request = Request::new(pb::ResolveSpaceMessageRequest {
                    org_id: self.org.clone(),
                    user_id: user.to_owned(),
                    space_id: self.space.clone(),
                    space_read_decision_ref: decision_ref,
                    space_read_decision_token: token,
                    message_id: message_id.to_owned(),
                });
                request.extensions_mut().insert(VerifiedIdentity::user_for_test(&self.org, user));
                SpaceHistoryGrpc::new(self.pool.clone(), true)
                    .resolve_message(request)
                    .await
                    .map(Response::into_inner)
            }

            async fn finish(&self) {
                clean_space(&self.pool, &self.org, &self.space).await;
                sqlx::query("DELETE FROM space_membership_revocations WHERE org_id = $1")
                    .bind(&self.org)
                    .execute(&self.pool)
                    .await
                    .ok();
                sqlx::query("DELETE FROM threads WHERE org_id = $1").bind(&self.org).execute(&self.pool).await.ok();
            }
        }

        fn ids(answer: &pb::SearchSpaceMessagesResponse) -> Vec<&str> {
            answer.hits.iter().map(|hit| hit.message_id.as_str()).collect()
        }

        fn trailer(status: &Status) -> Option<&str> {
            status.metadata().get(ERROR_CODE_TRAILER).and_then(|value| value.to_str().ok())
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn search_returns_only_current_audience_rows() {
            let Some(world) = World::new("aud").await else { return };
            let _keys = StreamKeysEnv::publish();
            let thread = world.thread("room", OWNER).await;
            let newer_thread = world.thread("newer", OWNER).await;
            sqlx::query("UPDATE threads SET recipient_audience_revision = 5 WHERE id = $1")
                .bind(&newer_thread)
                .execute(&world.pool)
                .await
                .expect("restamp a thread above the ceiling");
            let m1 = world.say(Msg { revision: 1, age_secs: 40, ..Msg::new(&thread, "m1", "the zebra plan") }).await;
            let m2 = world.say(Msg { revision: 3, age_secs: 30, ..Msg::new(&thread, "m2", "zebra update") }).await;
            // Above the ceiling: a message stamped 5 and a message of a thread stamped 5.
            let m3 = world.say(Msg { revision: 5, age_secs: 20, ..Msg::new(&thread, "m3", "zebra future") }).await;
            let m4 = world.say(Msg { revision: 1, age_secs: 10, ..Msg::new(&newer_thread, "m4", "zebra hidden thread") }).await;

            let answer = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert_eq!(answer.coverage, "shared", "{NEEDS_DB}");
            assert_eq!(ids(&answer), [m2.as_str(), m1.as_str()], "newest first, nothing above the ceiling");
            let hit = &answer.hits[1];
            assert_eq!((hit.thread_id.as_str(), hit.role.as_str(), hit.space_id.as_str()), (thread.as_str(), "user", world.space.as_str()));
            assert!(hit.snippet.iter().any(|segment| segment.matched && segment.text.eq_ignore_ascii_case("zebra")), "{hit:?}");
            // The ceiling moves the answer, in both directions.
            let low = world.search(MEMBER, Reach::Shared(1), |_| {}).await.expect("search");
            assert_eq!(ids(&low), [m1.as_str()]);
            let high = world.search(MEMBER, Reach::Shared(5), |_| {}).await.expect("search");
            assert_eq!(ids(&high), [m4.as_str(), m3.as_str(), m2.as_str(), m1.as_str()]);
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn owner_only_path_returns_own_threads_and_labels_coverage() {
            let Some(world) = World::new("own").await else { return };
            let _keys = StreamKeysEnv::publish();
            let mine = world.thread("mine", MEMBER).await;
            let theirs = world.thread("theirs", OWNER).await;
            let mine_hit = world.say(Msg { author: Some(MEMBER), age_secs: 20, ..Msg::new(&mine, "mine", "zebra mine") }).await;
            let their_hit = world.say(Msg { age_secs: 10, ..Msg::new(&theirs, "theirs", "zebra theirs") }).await;

            // No decision: only the caller's own threads, and the answer says so.
            let own = world.search(MEMBER, Reach::OwnThreads, |_| {}).await.expect("search");
            assert_eq!(own.coverage, "own_threads_only", "{NEEDS_DB}");
            assert_eq!(ids(&own), [mine_hit.as_str()]);
            let owners = world.search(OWNER, Reach::OwnThreads, |_| {}).await.expect("search");
            assert_eq!(ids(&owners), [their_hit.as_str()], "each caller sees only their own threads");
            // With a decision, the room.
            let shared = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert_eq!(shared.coverage, "shared");
            assert_eq!(ids(&shared), [their_hit.as_str(), mine_hit.as_str()]);
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn other_space_and_other_org_rows_never_match() {
            let Some(world) = World::new("iso").await else { return };
            let _keys = StreamKeysEnv::publish();
            let own = world.thread("own", OWNER).await;
            let here = world.say(Msg::new(&own, "here", "zebra here")).await;
            // Another Space of the same org, and another org that wears the same Space id.
            let other_space = format!("{}-other", world.space);
            let other_org = format!("{}-other", world.org);
            let space_thread = format!("thread-os-{}", world.suffix);
            let org_thread = format!("thread-oo-{}", world.suffix);
            seed_space_thread(&world.pool, &world.org, &other_space, &space_thread, OWNER).await;
            seed_space_thread(&world.pool, &other_org, &world.space, &org_thread, OWNER).await;
            world.insert_in(&other_space, Msg::new(&space_thread, "os", "zebra elsewhere")).await;
            world.insert_in(&world.space, Msg::new(&org_thread, "oo", "zebra in another org")).await;

            let answer = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert_eq!(ids(&answer), [here.as_str()], "{NEEDS_DB}");
            let own_threads = world.search(OWNER, Reach::OwnThreads, |_| {}).await.expect("search");
            assert_eq!(ids(&own_threads), [here.as_str()]);
            // The same walls hold for resolving an address.
            for (why, id) in [("another Space", format!("os-{}", world.suffix)), ("another org", format!("oo-{}", world.suffix))] {
                let status = world.resolve(MEMBER, Reach::Shared(CEILING), &id).await.expect_err(why);
                assert_eq!(status.code(), tonic::Code::NotFound, "{why}");
            }
            world.finish().await;
            clean_space(&world.pool, &world.org, &other_space).await;
            clean_space(&world.pool, &other_org, &world.space).await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn revoked_member_is_denied() {
            let Some(world) = World::new("rev").await else { return };
            let _keys = StreamKeysEnv::publish();
            let thread = world.thread("room", OWNER).await;
            let hit = world.say(Msg::new(&thread, "m1", "zebra")).await;
            world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("a member searches");
            world.resolve(MEMBER, Reach::Shared(CEILING), &hit).await.expect("a member resolves");

            sqlx::query("INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3)")
                .bind(&world.space)
                .bind(&world.org)
                .bind(MEMBER)
                .execute(&world.pool)
                .await
                .expect("revoke the member");
            let searched = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect_err("revoked");
            assert_eq!(searched.code(), tonic::Code::PermissionDenied, "{NEEDS_DB}");
            assert_eq!(trailer(&searched), Some("active_membership_required"));
            let resolved = world.resolve(MEMBER, Reach::Shared(CEILING), &hit).await.expect_err("revoked");
            assert_eq!(resolved.code(), tonic::Code::PermissionDenied);
            assert_eq!(trailer(&resolved), Some("active_membership_required"));
            // The owner-only path is behind the same revocation check.
            let own = world.search(MEMBER, Reach::OwnThreads, |_| {}).await.expect_err("revoked");
            assert_eq!(own.code(), tonic::Code::PermissionDenied);
            // Another member is unaffected.
            world.search("member-2", Reach::Shared(CEILING), |_| {}).await.expect("another member searches");
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn archived_threads_are_skipped() {
            let Some(world) = World::new("arch").await else { return };
            let _keys = StreamKeysEnv::publish();
            let live = world.thread("live", OWNER).await;
            let gone = world.thread("gone", OWNER).await;
            let kept = world.say(Msg { age_secs: 20, ..Msg::new(&live, "kept", "zebra kept") }).await;
            world.say(Msg { age_secs: 10, ..Msg::new(&gone, "archived", "zebra archived") }).await;
            sqlx::query("UPDATE threads SET archived_at = now() WHERE id = $1")
                .bind(&gone)
                .execute(&world.pool)
                .await
                .expect("archive");
            let answer = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert_eq!(ids(&answer), [kept.as_str()], "{NEEDS_DB}");
            let own = world.search(OWNER, Reach::OwnThreads, |_| {}).await.expect("search");
            assert_eq!(ids(&own), [kept.as_str()]);
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn zdr_waived_threads_are_excluded_and_reported() {
            let Some(world) = World::new("zdr").await else { return };
            let _keys = StreamKeysEnv::publish();
            let live = world.thread("live", OWNER).await;
            let waived = world.thread("waived", OWNER).await;
            let live_hit = world.say(Msg { age_secs: 20, ..Msg::new(&live, "live", "zebra and unicorn live") }).await;
            world.say(Msg { age_secs: 10, ..Msg::new(&waived, "waived", "zebra waived") }).await;
            seed_space_run(&world.pool, &world.org, &world.space, &waived, &format!("run-{}", world.suffix), "completed", "zdr_waived").await;

            let answer = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert_eq!(ids(&answer), [live_hit.as_str()], "{NEEDS_DB}");
            assert_eq!(answer.excluded, [EXCLUDED_ZDR_WAIVED], "the exclusion is reported, not silent");
            // A query that only the live thread matches has nothing withheld to report.
            let unicorn = world.search(MEMBER, Reach::Shared(CEILING), |request| request.query = "unicorn".to_owned()).await.expect("search");
            assert_eq!(ids(&unicorn), [live_hit.as_str()]);
            assert!(unicorn.excluded.is_empty(), "{:?}", unicorn.excluded);
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn resolve_message_not_found_is_identical_for_hidden_missing_and_foreign() {
            let Some(world) = World::new("res").await else { return };
            let _keys = StreamKeysEnv::publish();
            let thread = world.thread("room", OWNER).await;
            let visible = world.say(Msg::new(&thread, "visible", "zebra")).await;
            let above = world.say(Msg { revision: 5, ..Msg::new(&thread, "above", "zebra") }).await;
            let deleted = world.say(Msg::new(&thread, "deleted", "zebra")).await;
            world.revise(&thread, &deleted, "deleted").await;
            let edited = world.say(Msg::new(&thread, "edited", "zebra")).await;
            world.revise(&thread, &edited, "edited").await;
            let other_space = format!("{}-other", world.space);
            let foreign_thread = format!("thread-os-{}", world.suffix);
            seed_space_thread(&world.pool, &world.org, &other_space, &foreign_thread, OWNER).await;
            let foreign = world.insert_in(&other_space, Msg::new(&foreign_thread, "foreign", "zebra")).await;

            let resolved = world.resolve(MEMBER, Reach::Shared(CEILING), &visible).await.expect("visible resolves");
            assert_eq!((resolved.message_id.as_str(), resolved.thread_id.as_str()), (visible.as_str(), thread.as_str()), "{NEEDS_DB}");
            assert_eq!(resolved.space_id, world.space);
            world.resolve(MEMBER, Reach::Shared(CEILING), &edited).await.expect("an edited message is still addressable");

            // Hidden by the ceiling, deleted, missing, foreign, blank and oversize all answer the same way.
            let cases = [
                ("above the ceiling", above),
                ("deleted", deleted),
                ("missing", "message-missing".to_owned()),
                ("in another Space", foreign),
                ("blank", String::new()),
                ("oversize", "m".repeat(MAX_ID_BYTES + 1)),
            ];
            for (why, id) in cases {
                let status = world.resolve(MEMBER, Reach::Shared(CEILING), &id).await.expect_err(why);
                assert_eq!(status.code(), tonic::Code::NotFound, "{why}");
                assert_eq!(status.message(), "entry not found", "{why}");
                assert_eq!(trailer(&status), Some("entry_not_found"), "{why}");
            }
            world.finish().await;
            clean_space(&world.pool, &world.org, &other_space).await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn post_kind_post_rows_are_searchable() {
            let Some(world) = World::new("post").await else { return };
            let _keys = StreamKeysEnv::publish();
            let thread = world.thread("room", OWNER).await;
            let post = world.say(Msg { age_secs: 30, ..Msg::new(&thread, "post", "zebra human post") }).await;
            sqlx::query("UPDATE messages SET post_kind = 'post' WHERE id = $1")
                .bind(&post)
                .execute(&world.pool)
                .await
                .expect("mark a human post");
            let turn = world.say(Msg { age_secs: 20, ..Msg::new(&thread, "turn", "zebra a turn") }).await;
            // Tool and system rows are never searched.
            world.say(Msg { role: "tool", author: None, age_secs: 10, ..Msg::new(&thread, "tool", "zebra tool output") }).await;
            world.say(Msg { role: "system", author: None, age_secs: 5, ..Msg::new(&thread, "system", "zebra system text") }).await;
            let answer = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert_eq!(ids(&answer), [turn.as_str(), post.as_str()], "{NEEDS_DB}");
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn edited_or_deleted_originals_are_not_searchable() {
            let Some(world) = World::new("rev2").await else { return };
            let _keys = StreamKeysEnv::publish();
            let thread = world.thread("room", OWNER).await;
            let kept = world.say(Msg { age_secs: 40, ..Msg::new(&thread, "kept", "zebra kept") }).await;
            let edited = world.say(Msg { age_secs: 30, ..Msg::new(&thread, "edited", "zebra original text") }).await;
            let deleted = world.say(Msg { age_secs: 20, ..Msg::new(&thread, "deleted", "zebra deleted text") }).await;
            world.revise(&thread, &edited, "edited").await;
            world.revise(&thread, &deleted, "deleted").await;
            let answer = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert_eq!(ids(&answer), [kept.as_str()], "retain-mode originals stay in messages but are never found, {NEEDS_DB}");
            // Nor does the replacement text of an edit match: it lives in the revision row, not the index.
            let replacement = world.search(MEMBER, Reach::Shared(CEILING), |request| request.query = "replacement".to_owned()).await.expect("search");
            assert!(replacement.hits.is_empty(), "{:?}", ids(&replacement));
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn messages_over_100k_chars_insert_and_search_head() {
            let Some(world) = World::new("big").await else { return };
            let _keys = StreamKeysEnv::publish();
            let thread = world.thread("room", OWNER).await;
            // 9 characters of head, 112,000 of filler, then a tail word that is past the indexed prefix.
            let content = format!("headword {}tailword", "filler ".repeat(16_000));
            assert!(content.chars().count() > 100_000);
            let big = world.say(Msg::new(&thread, "big", &content)).await;
            let head = world.search(MEMBER, Reach::Shared(CEILING), |request| request.query = "headword".to_owned()).await.expect("search");
            assert_eq!(ids(&head), [big.as_str()], "the head of a long message is searched, {NEEDS_DB}");
            let snippet: String = head.hits[0].snippet.iter().map(|segment| segment.text.as_str()).collect();
            assert!(snippet.chars().count() < 1_000, "the snippet is a window, not the message: {}", snippet.len());
            let tail = world.search(MEMBER, Reach::Shared(CEILING), |request| request.query = "tailword".to_owned()).await.expect("search");
            assert!(tail.hits.is_empty(), "the part past 100,000 characters is not indexed: {:?}", ids(&tail));
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn erasure_removes_hits() {
            let Some(world) = World::new("erase").await else { return };
            let _keys = StreamKeysEnv::publish();
            let thread = world.thread("room", OWNER).await;
            let first = world.say(Msg { age_secs: 20, ..Msg::new(&thread, "first", "zebra first") }).await;
            let second = world.say(Msg { age_secs: 10, ..Msg::new(&thread, "second", "zebra second") }).await;
            let both = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert_eq!(ids(&both), [second.as_str(), first.as_str()], "{NEEDS_DB}");
            // Erasure deletes the message rows; the index follows with no purge hook of its own.
            sqlx::query("DELETE FROM messages WHERE id = $1").bind(&first).execute(&world.pool).await.expect("erase a message");
            let one = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert_eq!(ids(&one), [second.as_str()]);
            sqlx::query("DELETE FROM messages WHERE thread_id = $1").bind(&thread).execute(&world.pool).await.expect("erase the thread's messages");
            let none = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert!(none.hits.is_empty() && none.next_cursor.is_empty());
            world.resolve(MEMBER, Reach::Shared(CEILING), &second).await.expect_err("an erased message no longer resolves");
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn pages_are_full_when_filters_discard_rows() {
            let Some(world) = World::new("page").await else { return };
            let _keys = StreamKeysEnv::publish();
            let thread = world.thread("room", OWNER).await;
            let other = world.thread("other", OWNER).await;
            // Six matching messages, oldest first, alternating role and author.
            let mut made = Vec::new();
            for (n, (role, author)) in [
                ("user", "alice"),
                ("assistant", "agent"),
                ("user", "bob"),
                ("assistant", "agent"),
                ("user", "alice"),
                ("assistant", "agent"),
            ]
            .into_iter()
            .enumerate()
            {
                let name = format!("m{}", n + 1);
                let age = i32::try_from(60 - 10 * n).expect("small");
                let id = world.say(Msg { role, author: Some(author), age_secs: age, ..Msg::new(&thread, &name, "zebra words") }).await;
                made.push(id);
            }
            let in_other = world.say(Msg { age_secs: 5, ..Msg::new(&other, "elsewhere", "zebra in another thread") }).await;
            let by = |ids: &[usize]| ids.iter().map(|n| made[*n - 1].as_str()).collect::<Vec<_>>();

            // Role filter: three assistant rows match, in pages of two that are both as full as they can be.
            let first = world.search(MEMBER, Reach::Shared(CEILING), |request| {
                request.role = "assistant".to_owned();
                request.limit = 2;
            })
            .await
            .expect("search");
            assert_eq!(ids(&first), by(&[6, 4]), "the page is full although user rows sit between, {NEEDS_DB}");
            assert!(!first.next_cursor.is_empty());
            let second = world.search(MEMBER, Reach::Shared(CEILING), |request| {
                request.role = "assistant".to_owned();
                request.limit = 2;
                request.cursor = first.next_cursor.clone();
            })
            .await
            .expect("search");
            assert_eq!(ids(&second), by(&[2]));
            assert!(second.next_cursor.is_empty(), "no further page");

            // Author filter.
            let alice = world.search(MEMBER, Reach::Shared(CEILING), |request| {
                request.author_subject_id = "alice".to_owned();
                request.limit = 1;
            })
            .await
            .expect("search");
            assert_eq!(ids(&alice), by(&[5]));
            assert!(!alice.next_cursor.is_empty());
            let alice_next = world.search(MEMBER, Reach::Shared(CEILING), |request| {
                request.author_subject_id = "alice".to_owned();
                request.limit = 1;
                request.cursor = alice.next_cursor.clone();
            })
            .await
            .expect("search");
            assert_eq!(ids(&alice_next), by(&[1]));
            assert!(alice_next.next_cursor.is_empty());

            // Thread filter: only the named thread's row.
            let only_other = world.search(MEMBER, Reach::Shared(CEILING), |request| request.thread_id = other.clone()).await.expect("search");
            assert_eq!(ids(&only_other), [in_other.as_str()]);
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn relevance_cap_sets_truncated() {
            let Some(world) = World::new("rel").await else { return };
            let _keys = StreamKeysEnv::publish();
            let thread = world.thread("room", OWNER).await;
            for n in 0..120 {
                let name = format!("m{n:03}");
                world.say(Msg { age_secs: 200 - n, ..Msg::new(&thread, &name, "zebra relevance") }).await;
            }
            let ask = |cursor: String, limit: u32| {
                world.search(MEMBER, Reach::Shared(CEILING), move |request| {
                    request.sort = "relevance".to_owned();
                    request.limit = limit;
                    request.cursor = cursor;
                })
            };
            let first = ask(String::new(), 50).await.expect("search");
            assert_eq!(first.hits.len(), 50, "{NEEDS_DB}");
            assert!(!first.truncated && !first.next_cursor.is_empty(), "the first 50 of the 100 horizon");
            // The page that reaches the horizon is cut and flagged; no cursor leads past it.
            let second = ask(first.next_cursor.clone(), 50).await.expect("search");
            assert_eq!(second.hits.len(), 50);
            assert!(second.truncated, "100 results is the relevance horizon");
            assert!(second.next_cursor.is_empty(), "no cursor past the horizon");
            // No row repeats across the two pages.
            let mut seen: Vec<&str> = ids(&first).into_iter().chain(ids(&second)).collect();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), 100);
            // A cursor placed at the horizon asks for nothing and is answered as cut.
            let at_horizon = encode_cursor(&SearchCursor::Relevance { offset: RELEVANCE_HORIZON });
            let cut = ask(at_horizon, 50).await.expect("search");
            assert!(cut.hits.is_empty() && cut.truncated);
            // A cursor of the other sort is refused.
            let recent = encode_cursor(&SearchCursor::Recent { created_at: Utc::now(), message_id: "m".to_owned() });
            let refused = ask(recent, 50).await.expect_err("a recent cursor on a relevance search");
            assert_eq!(refused.code(), tonic::Code::InvalidArgument);
            assert_eq!(trailer(&refused), Some("invalid_cursor"));
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn since_join_floor_hides_pre_join_hits_titles_and_addresses() {
            let Some(world) = World::new("floor").await else { return };
            let _keys = StreamKeysEnv::publish();
            let thread = world.thread("room", OWNER).await;
            // The pre-join message is two hours old; the member joined one hour ago.
            let before = world.say(Msg { age_secs: 7_200, ..Msg::new(&thread, "before", "zebra before the join") }).await;
            let after = world.say(Msg { age_secs: 600, ..Msg::new(&thread, "after", "zebra after the join") }).await;
            sqlx::query("UPDATE threads SET presentation_title = 'Stored title' WHERE id = $1")
                .bind(&thread)
                .execute(&world.pool)
                .await
                .expect("the thread carries a stored title");
            let member = Reach::Since(CEILING, Utc::now() - chrono::Duration::hours(1));

            // Searched and addressed through the floor, the pre-join message is not there.
            let answer = world.search(MEMBER, member, |_| {}).await.expect("search");
            assert_eq!(answer.coverage, "shared", "{NEEDS_DB}");
            assert_eq!(ids(&answer), [after.as_str()], "a pre-join message is not searched");
            assert_ne!(answer.hits[0].thread_title, "Stored title", "a stored title is not shown under a floor");
            let hidden = world.resolve(MEMBER, member, &before).await.expect_err("a pre-join message");
            let missing = world.resolve(MEMBER, member, "message-missing").await.expect_err("a missing message");
            assert_eq!((hidden.code(), hidden.message()), (missing.code(), missing.message()), "hidden answers as missing");
            assert_eq!(trailer(&hidden), Some("entry_not_found"));
            world.resolve(MEMBER, member, &after).await.expect("a post-join message resolves");

            // The same member with no floor reads the whole room, and the stored title as stored.
            let whole = world.search(MEMBER, Reach::Shared(CEILING), |_| {}).await.expect("search");
            assert_eq!(ids(&whole), [after.as_str(), before.as_str()]);
            assert_eq!(whole.hits[0].thread_title, "Stored title");
            world
                .resolve(MEMBER, Reach::Shared(CEILING), &before)
                .await
                .expect("no floor: the pre-join message resolves");
            world.finish().await;
        }

        // ---- plan shape and the index migration ----

        fn index_is_valid() -> &'static str {
            "SELECT COALESCE((SELECT i.indisvalid FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid \
              WHERE c.relname = 'idx_messages_space_fts'), false)"
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn plan_shape_names_idx_messages_space_fts_over_50000_messages() {
            let Some(world) = World::new("plan").await else { return };
            let thread = world.thread("bulk", OWNER).await;
            let mut tx = world.pool.begin().await.expect("begin");
            sqlx::query("SELECT set_config('space_event.suppress', 'on', true)").execute(&mut *tx).await.ok();
            sqlx::query(
                "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
                 recipient_audience_revision, recipient_audience_hash, authority_revision, resource_authorization_ref, \
                 author_subject_id) \
                 SELECT 'bulk-' || g || '-' || $3, $1, 'user', 'ordinary words number ' || g, $2, 'audience-1', 1, \
                        'sha256:audience-1', 1, 'resource-1', 'owner-1' \
                 FROM generate_series(1, 50000) AS g",
            )
            .bind(&thread)
            .bind(&world.space)
            .bind(&world.suffix)
            .execute(&mut *tx)
            .await
            .expect("seed 50,000 messages");
            tx.commit().await.expect("commit");
            world.say(Msg::new(&thread, "needle", "the one rare needleword here")).await;
            sqlx::query("ANALYZE messages").execute(&world.pool).await.expect("analyze");

            // The real page query, with the bindings `search` gives it.
            let query = build_tsquery("needleword", false).expect("a query");
            let none_time: Option<DateTime<Utc>> = None;
            let plan: Vec<String> = sqlx::query_scalar(&format!("EXPLAIN {}", search_sql(false)))
                .bind(&world.org)
                .bind(&world.space)
                .bind(OWNER)
                .bind(Some(CEILING))
                .bind(&query)
                .bind("")
                .bind("")
                .bind("")
                .bind(none_time)
                .bind(none_time)
                .bind(none_time)
                .bind(None::<String>)
                .bind(0_i64)
                .bind(21_i64)
                .bind("start-marker")
                .bind("stop-marker")
                .bind(None::<i64>)
                .bind(none_time)
                .fetch_all(&world.pool)
                .await
                .expect("explain");
            let plan = plan.join("\n");
            assert!(plan.contains("idx_messages_space_fts"), "the search must use the full-text index, {NEEDS_DB}:\n{plan}");
            world.finish().await;
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
        async fn migration_0077_applies_twice_with_a_concurrent_insert_and_the_index_stays_valid() {
            let Some(world) = World::new("mig").await else { return };
            let valid = || async { sqlx::query_scalar::<_, bool>(index_is_valid()).fetch_one(&world.pool).await.expect("index probe") };
            assert!(valid().await, "the migration built a valid index, {NEEDS_DB}");

            let up = include_str!("../../migrations/0077_space_message_search_index.sql");
            let down = include_str!("../../migrations/revert/0077_space_message_search_index.down.sql");
            assert!(up.starts_with("-- no-transaction") && down.starts_with("-- no-transaction"));
            world.pool.execute(down).await.expect("the revert runs outside a transaction");
            assert!(!valid().await, "the revert dropped the index");

            // Build it again while a writer keeps inserting; the build must not block or invalidate.
            let thread = world.thread("room", OWNER).await;
            let writer = async {
                for n in 0..40 {
                    let name = format!("w{n:02}");
                    world.say(Msg { age_secs: 100 - n, ..Msg::new(&thread, &name, "zebra written during the build") }).await;
                }
            };
            let (built, ()) = tokio::join!(world.pool.execute(up), writer);
            built.expect("the build ran while rows were written");
            assert!(valid().await, "the index is valid after a build with a concurrent insert");

            // A second application is a no-op and the index is still valid and finds every row.
            world.pool.execute(up).await.expect("re-apply");
            assert!(valid().await);
            let (found,): (i64,) = sqlx::query_as(&format!(
                "SELECT count(*) FROM messages m WHERE m.space_id = $1 AND {FTS_EXPR} @@ to_tsquery('simple', 'zebra')"
            ))
            .bind(&world.space)
            .fetch_one(&world.pool)
            .await
            .expect("count through the index expression");
            assert_eq!(found, 40);
            world.finish().await;
        }
    }
}
