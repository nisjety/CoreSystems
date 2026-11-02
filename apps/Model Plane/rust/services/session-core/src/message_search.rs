//! Full-text search over the durable conversation record (build plan WS17 S3).
//!
//! `SearchMessages` answers two callers. UI serves the person at the keyboard and
//! searches every unscoped thread they own, waived ones included: it reads a record
//! the owner already keeps and persists nothing (DEC-32). RECALL serves the read-only
//! recall tool, which runs on a prompt the model writes. It searches only the owner's
//! unscoped threads, skips every thread that holds a zero-retention or waived run, and
//! is refused to a zero-retention credential. A recall result becomes model context, and a
//! member's room post (`post_kind = 'post'`) is never model context (WS01), so recall also
//! skips every post even where one sits in a thread the owner can reach; UI search shows them.
//!
//! Both purposes read the active path only (WS17 S8): a turn on an inactive branch is not
//! part of the conversation the owner sees, so search never returns one.
//!
//! `SESSION_CORE_MESSAGE_SEARCH_ENABLED` gates the query path only. Migration 0055
//! builds the index whether or not the switch is on, so enabling search needs no
//! migration.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{self as pb, message_search_server::MessageSearch};
use sqlx::PgPool;
use std::time::Instant;
use tonic::{Request, Response, Status};
use tracing::warn;

use crate::auth::{authorize_operation, identity};
use crate::grpc::{
    compact_thread_text, record_metrics, to_proto_timestamp, verify_thread_read_space_decision,
    THREAD_TITLE_MAX_CHARS,
};

/// The environment switch for the query path. Default off; read by the strict parser the
/// other durable-loop switches share, so only the literal `true` turns it on.
pub(crate) const ENABLED_ENV: &str = "SESSION_CORE_MESSAGE_SEARCH_ENABLED";

const DEFAULT_HIT_LIMIT: i64 = 20;
const MAX_HIT_LIMIT: i64 = 50;
/// Messages read before one hit is kept per thread. A common word matches many messages,
/// so the cap bounds one page. Every page of one query reads the same candidates, which
/// keeps paging free of gaps and repeats.
const CANDIDATE_LIMIT: i64 = 200;
/// Characters of the query read at all, and words kept from them. A search longer than
/// this is not a search anyone types.
const MAX_QUERY_CHARS: usize = 256;
const MAX_QUERY_WORDS: usize = 8;
/// Bounds one search so that a broad match cannot hold a connection.
const SEARCH_TIMEOUT_SQL: &str = "SET LOCAL statement_timeout = '2s'";
/// Postgres `query_canceled`, raised when the timeout above fires.
const QUERY_CANCELED: &str = "57014";

/// The page, every candidate message, the highlight and the caller's scope.
///
/// Parameters: $1 org, $2 owner, $3 prefix tsquery, $4 Space, $5 Space audience ceiling,
/// $6 origin, $7 thread filter, $8 excluded thread, $9 recall (no human posts, no zero-retention
/// threads), $10 archive state,
/// $11/$12 keyset position, $13 page rows + 1, $14 candidate cap.
///
/// The predicate on `to_tsvector('simple', left(m.content, 65536))` and the role filter
/// match migration 0055's index expression, so the planner can use it. `ts_headline` runs
/// on the page only, after the limit.
pub(crate) const SEARCH_SQL: &str = "
WITH candidates AS (
    SELECT m.id AS message_id, m.thread_id, m.role, m.content, m.created_at,
           t.session_key, t.presentation_title, t.space_id, t.archived_at
    FROM messages m
    JOIN threads t ON t.id = m.thread_id
    WHERE m.role IN ('user', 'assistant')
      AND m.on_active_path
      AND (NOT $9::BOOLEAN OR m.post_kind = 'turn')
      AND to_tsvector('simple', left(m.content, 65536)) @@ to_tsquery('simple', $3)
      AND t.org_id = $1
      AND (CASE WHEN $5::BIGINT IS NULL
                THEN t.user_id = $2 AND t.space_id IS NULL
                ELSE t.space_id = $4
                     AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $5::BIGINT)
                     AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $5::BIGINT)
           END)
      AND (NULLIF($6, '') IS NULL OR t.origin = $6)
      AND (NULLIF($7, '') IS NULL OR t.id = $7)
      AND (NULLIF($8, '') IS NULL OR t.id <> $8)
      AND ($10 = 'all' OR (($10 = 'archived') = (t.archived_at IS NOT NULL)))
      AND (NOT $9::BOOLEAN OR NOT EXISTS (
            SELECT 1 FROM runs r
            WHERE r.thread_id = t.id AND r.retention_class IN ('zdr', 'zdr_waived')))
    ORDER BY m.created_at DESC, m.id DESC
    LIMIT $14
),
ranked AS (
    SELECT c.*,
           ROW_NUMBER() OVER (PARTITION BY c.thread_id ORDER BY c.created_at DESC, c.message_id DESC) AS thread_rank
    FROM candidates c
),
hits AS (
    SELECT * FROM ranked
    WHERE NULLIF($7, '') IS NOT NULL OR thread_rank = 1
),
page AS (
    SELECT * FROM hits
    WHERE $11::TIMESTAMPTZ IS NULL
       OR (hits.created_at, hits.message_id) < ($11::TIMESTAMPTZ, $12::TEXT)
    ORDER BY hits.created_at DESC, hits.message_id DESC
    LIMIT $13
)
SELECT page.thread_id,
       page.message_id,
       page.role,
       page.created_at,
       page.session_key,
       page.presentation_title,
       page.space_id,
       (page.archived_at IS NOT NULL) AS archived,
       ts_headline(
           'simple',
           replace(replace(left(page.content, 65536), chr(57344), ''), chr(57345), ''),
           to_tsquery('simple', $3),
           'StartSel=' || chr(57344) || ', StopSel=' || chr(57345)
               || ', MaxFragments=1, MaxWords=24, MinWords=8'
       ) AS snippet
FROM page
ORDER BY page.created_at DESC, page.message_id DESC";

/// Read once, at boot, when the service is built: the switch is a deploy setting, and a
/// restart is how a deploy changes it.
pub(crate) fn message_search_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The query as a prefix `tsquery`: each word is alphanumeric, quoted, and matched as a
/// prefix, so the search as typed finds the words as they are being typed. Anything else
/// in the query is a separator. `None` when the query holds no word at all.
pub(crate) fn build_prefix_tsquery(query: &str) -> Option<String> {
    let bounded: String = query.chars().take(MAX_QUERY_CHARS).collect();
    let words: Vec<String> = bounded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(MAX_QUERY_WORDS)
        .map(|word| format!("'{word}':*"))
        .collect();
    (!words.is_empty()).then(|| words.join(" & "))
}

fn clamp_hit_limit(limit: u32) -> i64 {
    if limit == 0 {
        return DEFAULT_HIT_LIMIT;
    }
    i64::from(limit).clamp(1, MAX_HIT_LIMIT)
}

/// The sort position a SearchMessages page ended on. Hits sort newest first, then by
/// message id, so these two values identify the row and the next page starts strictly after.
#[derive(Debug, PartialEq)]
struct HitCursor {
    created_at: DateTime<Utc>,
    message_id: String,
}

fn encode_hit_cursor(cursor: &HitCursor) -> String {
    // One-letter keys keep the cursor short when it travels in a query string.
    let json = serde_json::json!({
        "c": cursor.created_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
        "m": cursor.message_id,
    });
    URL_SAFE_NO_PAD.encode(json.to_string())
}

fn decode_hit_cursor(raw: &str) -> Result<HitCursor, Status> {
    let invalid = || Status::invalid_argument("cursor is not a message search cursor");
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let created_at = value["c"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|parsed| parsed.with_timezone(&Utc))
        .ok_or_else(invalid)?;
    let message_id = value["m"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(invalid)?
        .to_owned();
    Ok(HitCursor {
        created_at,
        message_id,
    })
}

type SearchRow = (
    String,
    String,
    String,
    DateTime<Utc>,
    String,
    Option<String>,
    Option<String>,
    bool,
    Option<String>,
);

/// Postgres errors from the search, as typed statuses. A timeout is retryable and says so.
fn map_search_error(error: sqlx::Error) -> Status {
    let timed_out = error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code == QUERY_CANCELED);
    if timed_out {
        return Status::unavailable("message_search_timeout");
    }
    warn!(error = %error, "search_messages failed");
    Status::internal(error.to_string())
}

/// `MessageSearch`: the read half of conversation search, served behind the verifying
/// interceptor beside `SessionCore`, so that trait and its mocks are unchanged.
pub struct MessageSearchGrpc {
    pub(crate) pool: PgPool,
    /// Whether the query path answers. Off, every search is refused before it reads a row.
    pub(crate) enabled: bool,
}

#[tonic::async_trait]
impl MessageSearch for MessageSearchGrpc {
    async fn search_messages(
        &self,
        request: Request<pb::SearchMessagesRequest>,
    ) -> Result<Response<pb::SearchMessagesResponse>, Status> {
        let started = Instant::now();
        let result = self.search(request).await;
        record_metrics("search_messages", started, result.is_ok());
        result
    }
}

impl MessageSearchGrpc {
    async fn search(
        &self,
        request: Request<pb::SearchMessagesRequest>,
    ) -> Result<Response<pb::SearchMessagesResponse>, Status> {
        let caller = identity(&request)?;
        authorize_operation(&caller, "session:read")?;
        let req = request.into_inner();
        if req.org_id.is_empty() || req.user_id.is_empty() {
            return Err(Status::invalid_argument("org_id and user_id are required"));
        }
        caller.authorize_org(&req.org_id)?;
        caller.authorize_user(&req.user_id)?;
        if !self.enabled {
            return Err(Status::unavailable("message_search_disabled"));
        }
        let purpose = pb::SearchPurpose::try_from(req.purpose).map_err(|_| {
            Status::invalid_argument("purpose must be SEARCH_PURPOSE_UI or SEARCH_PURPOSE_RECALL")
        })?;
        let recall = purpose == pb::SearchPurpose::Recall;
        if recall {
            if caller.zdr() {
                return Err(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
                    .grpc_status(tonic::Code::PermissionDenied));
            }
            // Recall reads the owner's own unscoped threads. A Space read is never
            // widened to it, so a decision or a Space id here is a caller error.
            if !req.space_id.is_empty()
                || !req.space_read_decision_ref.is_empty()
                || !req.space_read_decision_token.is_empty()
            {
                return Err(Status::invalid_argument(
                    "recall searches the owner's own unscoped threads only",
                ));
            }
        }
        let space_read = verify_thread_read_space_decision(
            &req.space_read_decision_token,
            &req.space_read_decision_ref,
            &req.org_id,
            &req.user_id,
            &req.space_id,
        )?;
        let audience_ceiling = space_read.map(|read| read.recipient_audience_revision);
        let archive_state = match pb::ArchiveFilter::try_from(req.archived) {
            Ok(pb::ArchiveFilter::Unspecified) => "active",
            Ok(pb::ArchiveFilter::Archived) => "archived",
            Ok(pb::ArchiveFilter::All) => "all",
            Err(_) => {
                return Err(Status::invalid_argument(
                    "archived must be ARCHIVE_FILTER_UNSPECIFIED, ARCHIVE_FILTER_ARCHIVED or ARCHIVE_FILTER_ALL",
                ))
            }
        };
        let Some(tsquery) = build_prefix_tsquery(&req.query) else {
            return Ok(Response::new(pb::SearchMessagesResponse::default()));
        };
        let cursor = if req.cursor.is_empty() {
            None
        } else {
            Some(decode_hit_cursor(&req.cursor)?)
        };
        let page_size = clamp_hit_limit(req.limit);

        let mut tx = self.pool.begin().await.map_err(map_search_error)?;
        sqlx::query(SEARCH_TIMEOUT_SQL)
            .execute(&mut *tx)
            .await
            .map_err(map_search_error)?;
        // One extra row tells the caller whether another page exists without a count query.
        let mut rows: Vec<SearchRow> = sqlx::query_as(SEARCH_SQL)
            .bind(&req.org_id)
            .bind(&req.user_id)
            .bind(&tsquery)
            .bind(&req.space_id)
            .bind(audience_ceiling)
            .bind(&req.origin)
            .bind(&req.thread_id)
            .bind(&req.exclude_thread_id)
            .bind(recall)
            .bind(archive_state)
            .bind(cursor.as_ref().map(|position| position.created_at))
            .bind(cursor.as_ref().map(|position| position.message_id.clone()))
            .bind(page_size + 1)
            .bind(CANDIDATE_LIMIT)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_search_error)?;
        tx.commit().await.map_err(map_search_error)?;

        let page_len = usize::try_from(page_size).unwrap_or(usize::MAX);
        let has_more = rows.len() > page_len;
        let next_cursor = match rows.get(page_len - 1) {
            Some(last) if has_more => encode_hit_cursor(&HitCursor {
                created_at: last.3,
                message_id: last.1.clone(),
            }),
            _ => String::new(),
        };
        rows.truncate(page_len);

        let hits = rows
            .into_iter()
            .map(
                |(
                    thread_id,
                    message_id,
                    role,
                    created_at,
                    session_key,
                    title,
                    space_id,
                    archived,
                    snippet,
                )| {
                    let fallback_title = if session_key.trim().is_empty() {
                        "Verevon Chat"
                    } else {
                        session_key.as_str()
                    };
                    pb::SearchHit {
                        thread_id,
                        message_id,
                        role,
                        snippet: snippet.unwrap_or_default(),
                        created_at: Some(to_proto_timestamp(created_at)),
                        thread_title: compact_thread_text(
                            title,
                            fallback_title,
                            THREAD_TITLE_MAX_CHARS,
                        ),
                        space_id: space_id.unwrap_or_default(),
                        archived,
                    }
                },
            )
            .collect();
        Ok(Response::new(pb::SearchMessagesResponse {
            hits,
            next_cursor,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use mp_ids::new_ulid;

    #[test]
    fn prefix_query_quotes_each_word_and_matches_it_as_a_prefix() {
        assert_eq!(
            build_prefix_tsquery("Quarterly budget").as_deref(),
            Some("'Quarterly':* & 'budget':*")
        );
        assert_eq!(
            build_prefix_tsquery("  rust-lang, 2026!  ").as_deref(),
            Some("'rust':* & 'lang':* & '2026':*")
        );
    }

    #[test]
    fn prefix_query_is_empty_without_a_word_and_capped_at_eight_words() {
        assert_eq!(build_prefix_tsquery(""), None);
        assert_eq!(build_prefix_tsquery(" -- ?? "), None);
        let nine = (1..=9)
            .map(|n| format!("w{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let query = build_prefix_tsquery(&nine).unwrap();
        assert_eq!(query.matches(":*").count(), MAX_QUERY_WORDS);
        assert!(!query.contains("w9"));
    }

    #[test]
    fn prefix_query_reads_at_most_256_characters() {
        let long = format!("{} tail", "a".repeat(MAX_QUERY_CHARS));
        assert_eq!(
            build_prefix_tsquery(&long),
            Some(format!("'{}':*", "a".repeat(MAX_QUERY_CHARS)))
        );
    }

    #[test]
    fn prefix_query_never_emits_a_quote_or_operator_from_the_input() {
        let query = build_prefix_tsquery("it's a & b | !c").unwrap();
        assert_eq!(query.matches('\'').count(), 2 * query.matches(":*").count());
        assert!(!query.contains('|') && !query.contains('!'));
    }

    #[test]
    fn hit_limit_defaults_and_clamps() {
        assert_eq!(clamp_hit_limit(0), DEFAULT_HIT_LIMIT);
        assert_eq!(clamp_hit_limit(7), 7);
        assert_eq!(clamp_hit_limit(10_000), MAX_HIT_LIMIT);
    }

    #[test]
    fn hit_cursor_round_trips_to_the_microsecond() {
        let cursor = HitCursor {
            created_at: Utc.with_ymd_and_hms(2026, 10, 1, 12, 30, 15).unwrap()
                + Duration::microseconds(123_456),
            message_id: "01HZMESSAGE".to_owned(),
        };
        let raw = encode_hit_cursor(&cursor);
        assert_eq!(decode_hit_cursor(&raw).unwrap(), cursor);
    }

    #[test]
    fn hit_cursor_rejects_anything_that_is_not_a_search_position() {
        let cases = vec![
            "!!!".to_owned(),
            URL_SAFE_NO_PAD.encode("[]"),
            URL_SAFE_NO_PAD.encode(r#"{"c":"not-a-time","m":"x"}"#),
            URL_SAFE_NO_PAD.encode(r#"{"c":"2026-10-01T00:00:00Z","m":""}"#),
        ];
        for raw in cases {
            let error = decode_hit_cursor(&raw).expect_err(&raw);
            assert_eq!(error.code(), tonic::Code::InvalidArgument, "{raw}");
        }
    }

    #[test]
    fn the_switch_is_on_only_for_the_literal_true() {
        assert!(!mp_contracts::loop_fold::switch_from(None));
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("yes")));
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
    }

    #[test]
    fn the_query_matches_the_index_the_migration_builds() {
        // The planner uses the GIN index only when the predicate is the indexed expression
        // with the indexed partial filter. A change to either side must change both.
        assert!(SEARCH_SQL
            .contains("to_tsvector('simple', left(m.content, 65536)) @@ to_tsquery('simple', $3)"));
        assert!(SEARCH_SQL.contains("m.role IN ('user', 'assistant')"));
    }

    #[test]
    fn the_highlight_markers_are_stripped_from_content_then_added_back() {
        // A snippet wraps a match in U+E000 and U+E001, and both are removed from stored
        // content first, so a marker in a snippet always means a match.
        assert!(SEARCH_SQL.contains(
            "replace(replace(left(page.content, 65536), chr(57344), ''), chr(57345), '')"
        ));
        assert!(SEARCH_SQL.contains("'StartSel=' || chr(57344) || ', StopSel=' || chr(57345)"));
    }

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_thread(pool: &PgPool, org: &str, user: &str, id: &str) {
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(id)
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_message(pool: &PgPool, thread: &str, id: &str, content: &str) {
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content) VALUES ($1, $2, 'user', $3)",
        )
        .bind(id)
        .bind(thread)
        .bind(content)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_waived_run(pool: &PgPool, thread: &str, org: &str, user: &str) {
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, metadata,
                               retention_class, waiver_ack_id, waiver_connection_id)
             VALUES ($1, $2, 'g', $3, $4, 'completed', '{}'::jsonb,
                     'zdr_waived', 'ack-1', 'conn-1')",
        )
        .bind(new_ulid())
        .bind(thread)
        .bind(org)
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_post(pool: &PgPool, thread: &str, id: &str, content: &str) {
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content, post_kind)
             VALUES ($1, $2, 'user', $3, 'post')",
        )
        .bind(id)
        .bind(thread)
        .bind(content)
        .execute(pool)
        .await
        .unwrap();
    }

    fn request(org: &str, user: &str, query: &str) -> Request<pb::SearchMessagesRequest> {
        let mut request = Request::new(pb::SearchMessagesRequest {
            org_id: org.to_owned(),
            user_id: user.to_owned(),
            query: query.to_owned(),
            origin: "chat".to_owned(),
            ..Default::default()
        });
        request
            .extensions_mut()
            .insert(crate::auth::VerifiedIdentity::user_for_test(org, user));
        request
    }

    fn service(pool: PgPool) -> MessageSearchGrpc {
        MessageSearchGrpc {
            pool,
            enabled: true,
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn search_returns_only_the_owners_threads() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("search-owner-{suffix}");
        let mine = format!("{suffix}-mine");
        let theirs = format!("{suffix}-theirs");
        seed_thread(&pool, &org, "owner-a", &mine).await;
        seed_thread(&pool, &org, "owner-b", &theirs).await;
        seed_message(
            &pool,
            &mine,
            &format!("{suffix}-m1"),
            "the quarterly budget review",
        )
        .await;
        seed_message(
            &pool,
            &theirs,
            &format!("{suffix}-m2"),
            "a budget for the trip",
        )
        .await;

        let response = service(pool.clone())
            .search_messages(request(&org, "owner-a", "budget"))
            .await
            .expect("search")
            .into_inner();
        let thread_ids: Vec<&str> = response
            .hits
            .iter()
            .map(|hit| hit.thread_id.as_str())
            .collect();
        assert_eq!(thread_ids, vec![mine.as_str()]);
        assert!(response.hits[0].snippet.contains('\u{E000}'));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn search_never_returns_another_orgs_rows() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("search-org-a-{suffix}");
        let other_org = format!("search-org-b-{suffix}");
        let thread = format!("{suffix}-t");
        seed_thread(&pool, &other_org, "same-user", &thread).await;
        seed_message(
            &pool,
            &thread,
            &format!("{suffix}-m"),
            "a shared phrase here",
        )
        .await;

        let response = service(pool.clone())
            .search_messages(request(&org, "same-user", "shared phrase"))
            .await
            .expect("search")
            .into_inner();
        assert!(
            response.hits.is_empty(),
            "another org's message must not appear"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn search_recall_excludes_threads_with_waived_runs() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("search-recall-{suffix}");
        let waived = format!("{suffix}-waived");
        let plain = format!("{suffix}-plain");
        seed_thread(&pool, &org, "recaller", &waived).await;
        seed_thread(&pool, &org, "recaller", &plain).await;
        seed_message(
            &pool,
            &waived,
            &format!("{suffix}-w"),
            "waived recall marker",
        )
        .await;
        seed_message(&pool, &plain, &format!("{suffix}-p"), "plain recall marker").await;
        seed_waived_run(&pool, &waived, &org, "recaller").await;

        let mut recall = request(&org, "recaller", "recall marker");
        recall.get_mut().purpose = pb::SearchPurpose::Recall as i32;
        let response = service(pool.clone())
            .search_messages(recall)
            .await
            .expect("recall search")
            .into_inner();
        let thread_ids: Vec<&str> = response
            .hits
            .iter()
            .map(|hit| hit.thread_id.as_str())
            .collect();
        assert_eq!(thread_ids, vec![plain.as_str()]);

        // The same words in UI search keep the waived thread: the owner reads their own record.
        let ui = service(pool.clone())
            .search_messages(request(&org, "recaller", "recall marker"))
            .await
            .expect("ui search")
            .into_inner();
        assert_eq!(ui.hits.len(), 2);
    }

    /// WS01 depth: a recall result is model context and a member's post never is, so recall skips
    /// `post_kind = 'post'` rows while the person's own UI search still finds them.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn search_recall_excludes_human_posts_but_ui_search_keeps_them() {
        let pool = connect().await;
        let suffix = new_ulid();
        let org = format!("search-posts-{suffix}");
        let turn = format!("{suffix}-turn");
        let post = format!("{suffix}-post");
        seed_thread(&pool, &org, "poster", &turn).await;
        seed_thread(&pool, &org, "poster", &post).await;
        seed_message(&pool, &turn, &format!("{suffix}-t"), "kvartal marker as a turn").await;
        seed_post(&pool, &post, &format!("{suffix}-p"), "kvartal marker as a post").await;

        let mut recall = request(&org, "poster", "kvartal marker");
        recall.get_mut().purpose = pb::SearchPurpose::Recall as i32;
        let recalled = service(pool.clone())
            .search_messages(recall)
            .await
            .expect("recall search")
            .into_inner();
        let recalled_threads: Vec<&str> =
            recalled.hits.iter().map(|hit| hit.thread_id.as_str()).collect();
        assert_eq!(recalled_threads, vec![turn.as_str()], "recall never returns a post");

        let ui = service(pool.clone())
            .search_messages(request(&org, "poster", "kvartal marker"))
            .await
            .expect("ui search")
            .into_inner();
        let mut ui_threads: Vec<&str> = ui.hits.iter().map(|hit| hit.thread_id.as_str()).collect();
        ui_threads.sort_unstable();
        let mut expected = vec![turn.as_str(), post.as_str()];
        expected.sort_unstable();
        assert_eq!(ui_threads, expected, "UI search keeps the post");
    }

    /// The post predicate rides on the recall flag alone: it must stay in the statement and stay
    /// off for UI search (no database needed).
    #[test]
    fn the_post_predicate_is_gated_on_the_recall_flag() {
        assert!(SEARCH_SQL.contains("(NOT $9::BOOLEAN OR m.post_kind = 'turn')"));
    }

    /// WS06-S1: a ZDR credential's recall search is the typed durable-mutation refusal
    /// (answered before any row is read, so no database is needed).
    #[tokio::test]
    async fn zdr_refusal_recall_search_is_durable_mutation_forbidden() {
        use mp_contracts::zdr_refusal::Refusal;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool");
        let service = MessageSearchGrpc { pool, enabled: true };
        let mut request = Request::new(pb::SearchMessagesRequest {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            purpose: pb::SearchPurpose::Recall as i32,
            query: "budget".to_owned(),
            ..Default::default()
        });
        request
            .extensions_mut()
            .insert(crate::auth::VerifiedIdentity::user_for_test_with_zdr("org-1", "user-1", true));
        let status = service.search(request).await.expect_err("ZDR recall is refused");
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
        assert_eq!(
            Refusal::from_status(&status).map(|refusal| refusal.code()),
            Some(mp_contracts::zdr_refusal::code::DURABLE_MUTATION_FORBIDDEN)
        );
    }
}
