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
//! Both purposes also apply the WS12-S4 revision fold, as every product read does: a deleted
//! message is never a hit, and an edited one shows its effective text in the snippet. The original
//! text of a `retain` edit or delete stays in `messages.content`, so it is never a hit or a snippet
//! on its own. An edited message is a hit only when its effective text matches as well (see
//! [`search_sql`] for what the index prefilter does not find).
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
/// $11/$12 keyset position, $13 page rows + 1, $14 candidate cap, $15 Space history floor
/// (`NULL` = none; a message stamped before it is not a hit, WS04-S11).
///
/// Built at run time because the WS12-S4 revision fold is
/// [`crate::space::revisions::fold_join_sql`]. Its ceiling is `$5` too: `NULL` for an owner read,
/// which admits every revision. A deleted message is not a candidate (`FOLD_NOT_DELETED_SQL`), and
/// the snippet reads the effective text (`FOLD_CONTENT_SQL`), the same fold that ListConversation
/// and the Space timeline apply. A `retain` edit or delete leaves the original text in
/// `messages.content` (migration 0076), so that text is never a hit or a snippet on its own.
///
/// The predicate on `to_tsvector('simple', left(m.content, 65536))` and the role filter match
/// migration 0055's index expression, so the planner can use it. It is the prefilter, and it reads
/// the original text: an edited message is a candidate only when its original also matches, and its
/// effective text must match too. So a word that only the edit introduced is not found, and neither
/// is an edited message under `purge` (which blanks the original), until fold-aware indexing lands
/// (a later decision in `docs/plans/space-hybrid-build-plan-2026-10-14.md`). `ts_headline` runs on
/// the page only, after the limit.
pub(crate) fn search_sql() -> String {
    format!(
        "
WITH candidates AS (
    SELECT m.id AS message_id, m.thread_id, m.role, {fold_content} AS content, m.created_at,
           t.session_key, t.presentation_title, t.space_id, t.archived_at
    FROM messages m
    JOIN threads t ON t.id = m.thread_id
    {fold_join}
    WHERE m.role IN ('user', 'assistant')
      AND m.on_active_path
      AND (m.copied_from_message_id IS NULL OR NULLIF($7, '') IS NOT NULL)
      AND (NOT $9::BOOLEAN OR m.post_kind = 'turn')
      AND to_tsvector('simple', left(m.content, 65536)) @@ to_tsquery('simple', $3)
      AND {not_deleted}
      AND (COALESCE(rv.action, '') <> 'edited'
           OR to_tsvector('simple', left(COALESCE(rv.content, ''), 65536)) @@ to_tsquery('simple', $3))
      AND t.org_id = $1
      AND (CASE WHEN $5::BIGINT IS NULL
                THEN t.user_id = $2 AND t.space_id IS NULL
                ELSE t.space_id = $4
                     AND (t.recipient_audience_revision IS NULL OR t.recipient_audience_revision <= $5::BIGINT)
                     AND (m.recipient_audience_revision IS NULL OR m.recipient_audience_revision <= $5::BIGINT)
           END)
      AND ($15::BIGINT IS NULL
           OR m.recipient_audience_revision IS NULL
           OR m.recipient_audience_revision >= $15::BIGINT)
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
ORDER BY page.created_at DESC, page.message_id DESC",
        fold_content = crate::space::revisions::FOLD_CONTENT_SQL,
        fold_join = crate::space::revisions::fold_join_sql("$5::BIGINT"),
        not_deleted = crate::space::revisions::FOLD_NOT_DELETED_SQL,
    )
}

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
        // The window rides the same verified decision as the ceiling. The floor (WS04-S11) is
        // `None` for an owner read and whenever SESSION_CORE_SPACE_HISTORY_FLOOR is off.
        let audience = crate::space::audience_window::audience_window(space_read);
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
        let sql = search_sql();
        let mut rows: Vec<SearchRow> = sqlx::query_as(&sql)
            .bind(&req.org_id)
            .bind(&req.user_id)
            .bind(&tsquery)
            .bind(&req.space_id)
            .bind(audience.ceiling)
            .bind(&req.origin)
            .bind(&req.thread_id)
            .bind(&req.exclude_thread_id)
            .bind(recall)
            .bind(archive_state)
            .bind(cursor.as_ref().map(|position| position.created_at))
            .bind(cursor.as_ref().map(|position| position.message_id.clone()))
            .bind(page_size + 1)
            .bind(CANDIDATE_LIMIT)
            .bind(audience.floor)
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
    use crate::space::audience_window::{history_floor_enabled_from, history_floor_from_claim};
    use crate::space::test_support::{
        clean_space, seed_space_message_at_revision, seed_space_thread,
    };
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
        let sql = search_sql();
        assert!(sql
            .contains("to_tsvector('simple', left(m.content, 65536)) @@ to_tsquery('simple', $3)"));
        assert!(sql.contains("m.role IN ('user', 'assistant')"));
    }

    #[test]
    fn the_highlight_markers_are_stripped_from_content_then_added_back() {
        // A snippet wraps a match in U+E000 and U+E001, and both are removed from stored
        // content first, so a marker in a snippet always means a match.
        let sql = search_sql();
        assert!(sql.contains(
            "replace(replace(left(page.content, 65536), chr(57344), ''), chr(57345), '')"
        ));
        assert!(sql.contains("'StartSel=' || chr(57344) || ', StopSel=' || chr(57345)"));
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

    /// WS04-S11: the history floor is bound last ($15) and, as in the conversation read, admits a
    /// message stamped with no revision. Checked on the statement text (no database needed).
    #[test]
    fn the_space_history_floor_is_bound_last_and_admits_an_unstamped_message() {
        let sql = search_sql();
        assert!(sql.contains("$15::BIGINT IS NULL"));
        assert!(sql.contains("m.recipient_audience_revision >= $15::BIGINT"));
        assert!(
            !sql.contains("$16"),
            "fifteen binds: the floor is the last one"
        );
    }

    /// WS04-S11: a `since_join` reader's search returns no message stamped before the floor. The
    /// floor is an SQL predicate, so this runs the statement with the binds `search()` makes and the
    /// floor the signed claim yields. The switch off, and a zero claim, give full history.
    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_search_hides_messages_before_the_history_floor() {
        let Some(pool) = crate::space::test_support::connect().await else {
            return;
        };
        let suffix = new_ulid();
        let org = format!("search-floor-{suffix}");
        let space = format!("space-search-floor-{suffix}");
        // One thread per message. Search keeps only the newest hit of each thread unless a thread
        // filter is set, so two messages in one thread would show one of them whatever the floor.
        let before_thread = format!("{suffix}-before-thread");
        let after_thread = format!("{suffix}-after-thread");
        seed_space_thread(&pool, &org, &space, &before_thread, "owner-a").await;
        seed_space_thread(&pool, &org, &space, &after_thread, "owner-a").await;
        let before = format!("{suffix}-before");
        let after = format!("{suffix}-after");
        seed_space_message_at_revision(
            &pool,
            &space,
            &before_thread,
            &before,
            "budget before the join",
            1,
        )
        .await;
        seed_space_message_at_revision(
            &pool,
            &space,
            &after_thread,
            &after,
            "budget after the join",
            4,
        )
        .await;
        let mut full_history = vec![before.clone(), after.clone()];
        full_history.sort();

        // A member who joined at revision 3 reads from revision 3 on; the decision is at revision 5.
        let on = history_floor_enabled_from(Some("true"));
        let floored = space_search_ids(
            &pool,
            &org,
            &space,
            Some(5),
            history_floor_from_claim(3, on),
            "budget",
        )
        .await;
        assert_eq!(
            floored,
            vec![after.clone()],
            "a message stamped before the floor is not returned"
        );

        // The switch off: the same signed claim is ignored and the earlier message shows.
        let off = history_floor_from_claim(3, history_floor_enabled_from(Some("1")));
        let unfloored = space_search_ids(&pool, &org, &space, Some(5), off, "budget").await;
        assert_eq!(
            unfloored, full_history,
            "floor off returns the earlier message"
        );

        // The switch on with a zero claim: no floor is signed, so everything is returned.
        let zero = history_floor_from_claim(0, on);
        let zero_claim = space_search_ids(&pool, &org, &space, Some(5), zero, "budget").await;
        assert_eq!(zero_claim, full_history, "a zero claim is no floor");

        clean_space(&pool, &org, &space).await;
    }

    /// Records one revision the way `revise_message` stores it: an edit carries its new text, a delete
    /// none. Stamped at audience revision 1, so a reader at any higher ceiling sees it.
    async fn record_revision(
        pool: &PgPool,
        org: &str,
        space: &str,
        thread: &str,
        message: &str,
        action: &str,
        content: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO message_revisions (revision_id, org_id, thread_id, message_id, revision_no, \
             action, actor_subject_id, actor_class, moderation_reason, content, content_digest, \
             prev_content_digest, original_retention, idempotency_hash, space_id, \
             recipient_audience_revision, authority_revision, resource_authorization_ref) \
             VALUES ($1, $2, $3, $4, 1, $5, 'owner-a', 'author', '', $6, $7, $8, 'retain', $9, $10, \
             1, 1, 'resource-1')",
        )
        .bind(format!("rev-{message}"))
        .bind(org)
        .bind(thread)
        .bind(message)
        .bind(action)
        .bind(content)
        .bind(format!("sha256:{message}-now"))
        .bind(format!("sha256:{message}-before"))
        .bind(format!("idem-{message}"))
        .bind(space)
        .execute(pool)
        .await
        .expect("record the revision");
    }

    /// The hits of one owner search for `query`: a UI search, or a recall search when `recall` is set.
    async fn owner_search(
        pool: &PgPool,
        org: &str,
        owner: &str,
        query: &str,
        recall: bool,
    ) -> Vec<pb::SearchHit> {
        let mut req = request(org, owner, query);
        if recall {
            req.get_mut().purpose = pb::SearchPurpose::Recall as i32;
        }
        service(pool.clone())
            .search_messages(req)
            .await
            .expect("owner search")
            .into_inner()
            .hits
    }

    /// WS12-S4 against Postgres: a deleted message is not a hit on either purpose. An edited message that
    /// keeps the searched word is found and shown by its effective text, and its original text is never a
    /// hit or a snippet. An unrevised message is found as it was. The Space read at audience revision 5
    /// gives the same for its deleted and edited messages.
    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn search_hides_the_original_text_of_a_deleted_or_edited_message() {
        let Some(pool) = crate::space::test_support::connect().await else {
            return;
        };
        let suffix = new_ulid();
        let org = format!("search-revise-{suffix}");
        let owner = "reviser";
        // One owner thread per message, so each message is the hit of its own thread.
        let deleted_thread = format!("{suffix}-deleted");
        let edited_thread = format!("{suffix}-edited");
        let plain_thread = format!("{suffix}-plain");
        for thread in [&deleted_thread, &edited_thread, &plain_thread] {
            seed_thread(&pool, &org, owner, thread).await;
        }
        let deleted = format!("{suffix}-deleted-m");
        let edited = format!("{suffix}-edited-m");
        let plain = format!("{suffix}-plain-m");
        seed_message(&pool, &deleted_thread, &deleted, "retired marker text").await;
        seed_message(&pool, &edited_thread, &edited, "original wording stays").await;
        seed_message(&pool, &plain_thread, &plain, "plain wording stays").await;
        record_revision(&pool, &org, "", &deleted_thread, &deleted, "deleted", None).await;
        record_revision(
            &pool,
            &org,
            "",
            &edited_thread,
            &edited,
            "edited",
            Some("revised wording stays"),
        )
        .await;

        // The deleted message's words, and the edited message's original-only word, find nothing.
        for query in ["retired", "marker"] {
            let ui = owner_search(&pool, &org, owner, query, false).await;
            assert!(ui.is_empty(), "UI finds {query}");
            let recalled = owner_search(&pool, &org, owner, query, true).await;
            assert!(recalled.is_empty(), "recall finds {query}");
        }
        let original = owner_search(&pool, &org, owner, "original", false).await;
        assert!(original.is_empty(), "an edit's original is found");

        // A word both texts keep finds the edited message by its effective text, and the plain one too.
        let hits = owner_search(&pool, &org, owner, "wording", false).await;
        let mut threads: Vec<&str> = hits.iter().map(|hit| hit.thread_id.as_str()).collect();
        threads.sort_unstable();
        let mut expected = vec![edited_thread.as_str(), plain_thread.as_str()];
        expected.sort_unstable();
        assert_eq!(threads, expected);
        let edited_hit = hits
            .iter()
            .find(|hit| hit.thread_id == edited_thread)
            .expect("the edited message is a hit");
        let edited_snippet = edited_hit.snippet.as_str();
        assert!(edited_snippet.contains("revised"), "{edited_snippet}");
        assert!(!edited_snippet.contains("original"), "{edited_snippet}");
        assert!(edited_snippet.contains('\u{E000}'), "{edited_snippet}");
        let plain_hit = hits
            .iter()
            .find(|hit| hit.thread_id == plain_thread)
            .expect("the plain message is a hit");
        let plain_snippet = plain_hit.snippet.as_str();
        assert!(plain_snippet.contains('\u{E000}'), "{plain_snippet}");
        let recalled = owner_search(&pool, &org, owner, "wording", true).await;
        assert_eq!(recalled.len(), 2, "recall keeps both turns");

        // The same on a Space read at audience revision 5: one deleted and one edited Space message.
        let space = format!("space-revise-{suffix}");
        let space_deleted_thread = format!("{suffix}-sd");
        let space_edited_thread = format!("{suffix}-se");
        seed_space_thread(&pool, &org, &space, &space_deleted_thread, owner).await;
        seed_space_thread(&pool, &org, &space, &space_edited_thread, owner).await;
        let space_deleted = format!("{suffix}-sd-m");
        let space_edited = format!("{suffix}-se-m");
        seed_space_message_at_revision(
            &pool,
            &space,
            &space_deleted_thread,
            &space_deleted,
            "space marker text",
            1,
        )
        .await;
        seed_space_message_at_revision(
            &pool,
            &space,
            &space_edited_thread,
            &space_edited,
            "space original wording",
            1,
        )
        .await;
        record_revision(
            &pool,
            &org,
            &space,
            &space_deleted_thread,
            &space_deleted,
            "deleted",
            None,
        )
        .await;
        record_revision(
            &pool,
            &org,
            &space,
            &space_edited_thread,
            &space_edited,
            "edited",
            Some("space revised wording"),
        )
        .await;
        let marker = space_search_rows(&pool, &org, &space, Some(5), None, "marker").await;
        assert!(marker.is_empty(), "a deleted Space message is not a hit");
        let original = space_search_rows(&pool, &org, &space, Some(5), None, "original").await;
        assert!(original.is_empty(), "a Space edit's original is found");
        let rows = space_search_rows(&pool, &org, &space, Some(5), None, "wording").await;
        assert_eq!(rows.len(), 1, "only the edited Space message is a hit");
        assert_eq!(rows[0].1, space_edited);
        let snippet = rows[0].8.clone().unwrap_or_default();
        assert!(snippet.contains("revised"), "{snippet}");
        assert!(!snippet.contains("original"), "{snippet}");
        clean_space(&pool, &org, &space).await;
    }

    /// The rows one Space search returns, bound exactly as `search()` binds them.
    async fn space_search_rows(
        pool: &PgPool,
        org: &str,
        space: &str,
        ceiling: Option<i64>,
        floor: Option<i64>,
        query: &str,
    ) -> Vec<SearchRow> {
        let tsquery = build_prefix_tsquery(query).expect("a word");
        let sql = search_sql();
        let rows: Vec<SearchRow> = sqlx::query_as(&sql)
            .bind(org)
            .bind("owner-a")
            .bind(&tsquery)
            .bind(space)
            .bind(ceiling)
            .bind("")
            .bind("")
            .bind("")
            .bind(false)
            .bind("active")
            .bind(None::<DateTime<Utc>>)
            .bind(None::<String>)
            .bind(DEFAULT_HIT_LIMIT + 1)
            .bind(CANDIDATE_LIMIT)
            .bind(floor)
            .fetch_all(pool)
            .await
            .expect("space search");
        rows
    }

    /// The message ids one Space search returns, sorted.
    async fn space_search_ids(
        pool: &PgPool,
        org: &str,
        space: &str,
        ceiling: Option<i64>,
        floor: Option<i64>,
        query: &str,
    ) -> Vec<String> {
        let mut ids: Vec<String> = space_search_rows(pool, org, space, ceiling, floor, query)
            .await
            .into_iter()
            .map(|row| row.1)
            .collect();
        ids.sort();
        ids
    }

    /// WS12-S4: the search applies the fold every product read applies, so a deleted message is not a
    /// candidate, the snippet reads the effective text, and an edited candidate's match reads the edit
    /// too. Checked on the statement text (no database needed); the DB tier below proves the rows. For
    /// a message with no revision row the fold is a no-op: the effective text is `messages.content`,
    /// nothing is deleted, and the message matches on its own text, so such a search returns what it
    /// always did.
    #[test]
    fn the_search_reads_through_the_revision_fold() {
        let sql = search_sql();
        // The fold itself, with the reader's ceiling `$5` (`NULL` for an owner read: every revision).
        let fold = crate::space::revisions::fold_join_sql("$5::BIGINT");
        assert!(sql.contains(fold.as_str()), "the fold join");
        // A deleted message is never a candidate.
        assert!(sql.contains(crate::space::revisions::FOLD_NOT_DELETED_SQL));
        // The candidate's text is the effective text, which the match and the snippet read.
        let content = format!("{} AS content", crate::space::revisions::FOLD_CONTENT_SQL);
        assert!(sql.contains(content.as_str()));
        assert!(!sql.contains("replace(replace(left(m.content"));
        // An edited message is matched on its effective text as well as its original.
        assert!(sql.contains("COALESCE(rv.action, '') <> 'edited'"));
        assert!(sql.contains(
            "to_tsvector('simple', left(COALESCE(rv.content, ''), 65536)) @@ to_tsquery('simple', $3)"
        ));
        // The fold sits before the deleted-message filter, and both sit before the ranking.
        let fold_at = sql.find("LEFT JOIN LATERAL").expect("the fold join");
        let deleted_at = sql
            .find(crate::space::revisions::FOLD_NOT_DELETED_SQL)
            .expect("the deleted-message filter");
        let ranked_at = sql.find("ranked AS").expect("the ranking CTE");
        assert!(fold_at < deleted_at && deleted_at < ranked_at);
    }

    /// The post predicate rides on the recall flag alone: it must stay in the statement and stay
    /// off for UI search (no database needed).
    #[test]
    fn the_post_predicate_is_gated_on_the_recall_flag() {
        assert!(search_sql().contains("(NOT $9::BOOLEAN OR m.post_kind = 'turn')"));
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
