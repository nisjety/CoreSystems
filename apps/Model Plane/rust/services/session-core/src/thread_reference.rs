//! `ReadThreadForReference`: an outline or a page of one of the owner's earlier chat threads, for
//! the gateway's read-only thread reference tools (Wave 3 EP-3 S7).
//!
//! The person attaches earlier threads to a turn; the model then browses them lazily through
//! `thread_outline`, `thread_search` and `thread_read` instead of having them injected. What this
//! RPC returns becomes model context, so it reads only what recall may read (OD-7):
//!
//! * the caller's own thread, never another person's (answered `not_found`, as a missing one is);
//! * an unscoped chat thread: a Space thread's audience is not the owner's alone, and another
//!   origin is not a conversation the person had in chat;
//! * not the turn's own thread, which the model already holds;
//! * not an archived thread (the D2-186 default: archived threads are not opened by reference);
//! * not a thread that holds a zero-retention or waived run (DEC-32), the predicate recall uses;
//! * the active path only, user and assistant turns only, never a member's room post;
//! * at most [`MAX_LIMIT`] turns a page and [`MAX_CONTENT_CHARS`] characters a turn.
//!
//! A zero-retention credential is refused before any read. The RPC writes nothing and holds no
//! switch: it is reachable only through the gateway's tools, which are off by default
//! (`MODEL_GATEWAY_THREAD_REFERENCES_ENABLED`).

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    self as pb, thread_reference_service_server::ThreadReferenceService,
};
use sqlx::PgPool;
use std::time::Instant;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, identity, VerifiedIdentity};
use crate::grpc::{
    compact_thread_text, record_metrics, to_proto_timestamp, THREAD_TITLE_MAX_CHARS,
};

/// Most turns one read returns, and the number used when the caller names none.
pub(crate) const MAX_LIMIT: u32 = 20;
/// The longest a turn's text may be in a page, in characters.
pub(crate) const MAX_CONTENT_CHARS: usize = 4_000;
/// The longest an outline line may be, in characters.
pub(crate) const MAX_FIRST_LINE_CHARS: usize = 160;
/// Characters of a turn read from the database for an outline. The first line is found in them;
/// a turn whose first 2,000 characters hold no line break shows them cut to the line cap.
const OUTLINE_READ_CHARS: i32 = 2_000;
/// The only origin a referenced thread may have.
const CHAT_ORIGIN: &str = "chat";

/// The thread reference RPC. It holds the pool and nothing else; the rules are in this module.
pub struct ThreadReferenceGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl ThreadReferenceService for ThreadReferenceGrpc {
    async fn read_thread_for_reference(
        &self,
        request: Request<pb::ReadThreadForReferenceRequest>,
    ) -> Result<Response<pb::ReadThreadForReferenceResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            // The read becomes model context, so a zero-retention credential is refused as a
            // recall search is: `session:read` alone would let it through.
            if caller.zdr() {
                return Err(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
                    .grpc_status(tonic::Code::PermissionDenied));
            }
            read_reference(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("read_thread_for_reference", started, result.is_ok());
        result
    }
}

/// The referenced thread's row, as the refusal rules read it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferencedThread {
    org_id: String,
    user_id: String,
    space_id: Option<String>,
    origin: String,
    archived: bool,
}

/// Why a thread may not be read by reference, as the failed_precondition message, or `None` when
/// it may. Ownership is checked before this and answers `not_found`; the order here decides which
/// reason a thread that breaks several rules is given.
fn reference_refusal(
    thread: &ReferencedThread,
    thread_id: &str,
    current_thread_id: &str,
    holds_retention_run: bool,
) -> Option<&'static str> {
    if thread_id == current_thread_id {
        return Some("thread_reference_current_thread");
    }
    let in_space = thread
        .space_id
        .as_deref()
        .is_some_and(|space| !space.trim().is_empty());
    if in_space || thread.origin != CHAT_ORIGIN {
        return Some("thread_reference_not_chat");
    }
    if thread.archived {
        return Some("thread_reference_archived");
    }
    if holds_retention_run {
        return Some("thread_reference_retention");
    }
    None
}

/// The caller owns the thread: a person in the thread's org who is its owner. Anything else is
/// answered as a missing thread, so a reference cannot probe for another person's threads.
fn owned_by(thread: &ReferencedThread, org_id: &str, caller_user: Option<&str>) -> bool {
    thread.org_id == org_id && caller_user.is_some_and(|user| user == thread.user_id)
}

/// The page size: the server default for 0, otherwise at most [`MAX_LIMIT`].
fn clamp_limit(limit: u32) -> i64 {
    i64::from(if limit == 0 { MAX_LIMIT } else { limit.min(MAX_LIMIT) })
}

/// A turn's text cut to [`MAX_CONTENT_CHARS`], and whether it was cut.
fn capped_content(content: &str) -> (String, bool) {
    let mut chars = content.chars();
    let capped: String = chars.by_ref().take(MAX_CONTENT_CHARS).collect();
    (capped, chars.next().is_some())
}

/// A turn's first non-empty line, whitespace collapsed and cut to [`MAX_FIRST_LINE_CHARS`].
fn first_line(content: &str) -> String {
    let line = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    line.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_FIRST_LINE_CHARS)
        .collect()
}

/// Read one page of an attached thread. The credential checks are the RPC's, so the read itself
/// can be tested without the interceptor.
pub(crate) async fn read_reference(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::ReadThreadForReferenceRequest,
) -> Result<pb::ReadThreadForReferenceResponse, Status> {
    let org_id = req.org_id.trim();
    let user_id = req.user_id.trim();
    let thread_id = req.thread_id.trim();
    let current_thread_id = req.current_thread_id.trim();
    if thread_id.is_empty() || current_thread_id.is_empty() {
        return Err(Status::invalid_argument(
            "thread_id and current_thread_id are required",
        ));
    }
    caller.authorize_org(org_id)?;
    caller.authorize_user(user_id)?;
    let mode = match pb::ThreadReferenceMode::try_from(req.mode) {
        Ok(pb::ThreadReferenceMode::Outline) => pb::ThreadReferenceMode::Outline,
        Ok(pb::ThreadReferenceMode::Page) => pb::ThreadReferenceMode::Page,
        _ => {
            return Err(Status::invalid_argument(
                "mode must be THREAD_REFERENCE_MODE_OUTLINE or THREAD_REFERENCE_MODE_PAGE",
            ))
        }
    };

    // One read-only transaction, so the refusal checks and the page see the same thread.
    let mut tx = pool.begin().await.map_err(internal)?;
    let row: Option<(String, String, Option<String>, String, bool, Option<String>, String)> =
        sqlx::query_as(
            "SELECT org_id, user_id, space_id, origin, archived_at IS NOT NULL, presentation_title, session_key
             FROM threads WHERE id = $1",
        )
        .bind(thread_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
    let Some((row_org, row_user, space_id, origin, archived, title, session_key)) = row else {
        return Err(Status::not_found("thread not found"));
    };
    let thread = ReferencedThread {
        org_id: row_org,
        user_id: row_user,
        space_id,
        origin,
        archived,
    };
    if !owned_by(&thread, org_id, caller.user_id()) {
        return Err(Status::not_found("thread not found"));
    }
    let holds_retention_run: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE thread_id = $1 AND retention_class IN ('zdr', 'zdr_waived'))",
    )
    .bind(thread_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    if let Some(reason) =
        reference_refusal(&thread, thread_id, current_thread_id, holds_retention_run)
    {
        return Err(Status::failed_precondition(reason));
    }

    let before_sequence = match req.before_message_id.trim() {
        "" => None,
        before => Some(
            sqlx::query_scalar::<_, i64>(
                "SELECT sequence FROM messages WHERE id = $1 AND thread_id = $2 AND on_active_path",
            )
            .bind(before)
            .bind(thread_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal)?
            .ok_or_else(|| {
                Status::invalid_argument("before_message_id is not on this thread's active path")
            })?,
        ),
    };
    let limit = clamp_limit(req.limit);
    // Only as much of each turn is read as the mode can return, plus one character to tell a cut.
    let read_chars = match mode {
        pb::ThreadReferenceMode::Outline => OUTLINE_READ_CHARS,
        _ => i32::try_from(MAX_CONTENT_CHARS + 1).unwrap_or(i32::MAX),
    };
    // One row past the page tells whether older turns exist without a count query.
    let mut rows: Vec<(String, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, role, left(content, $4), created_at
         FROM messages
         WHERE thread_id = $1
           AND on_active_path
           AND role IN ('user', 'assistant')
           AND post_kind = 'turn'
           AND ($2::BIGINT IS NULL OR sequence < $2)
         ORDER BY sequence DESC
         LIMIT $3",
    )
    .bind(thread_id)
    .bind(before_sequence)
    .bind(limit + 1)
    .bind(read_chars)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    let page_len = usize::try_from(limit).unwrap_or(usize::MAX);
    let has_more = rows.len() > page_len;
    rows.truncate(page_len);
    rows.reverse();

    let fallback_title = if session_key.trim().is_empty() {
        "Verevon Chat"
    } else {
        session_key.as_str()
    };
    let mut response = pb::ReadThreadForReferenceResponse {
        thread_title: compact_thread_text(title, fallback_title, THREAD_TITLE_MAX_CHARS),
        has_more,
        ..Default::default()
    };
    for (message_id, role, content, created_at) in rows {
        if mode == pb::ThreadReferenceMode::Outline {
            response.outline.push(pb::ThreadReferenceOutlineItem {
                message_id,
                role,
                first_line: first_line(&content),
                created_at: Some(to_proto_timestamp(created_at)),
            });
        } else {
            let (content, truncated) = capped_content(&content);
            response.messages.push(pb::ThreadReferenceMessage {
                message_id,
                role,
                content,
                created_at: Some(to_proto_timestamp(created_at)),
                truncated,
            });
        }
    }
    Ok(response)
}

fn internal(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_ids::new_ulid;

    const ORG: &str = "org-thread-ref";
    const OWNER: &str = "user-thread-ref-owner";
    const OTHER: &str = "user-thread-ref-other";

    fn chat_thread() -> ReferencedThread {
        ReferencedThread {
            org_id: ORG.to_owned(),
            user_id: OWNER.to_owned(),
            space_id: None,
            origin: CHAT_ORIGIN.to_owned(),
            archived: false,
        }
    }

    #[test]
    fn an_owned_unscoped_chat_thread_is_readable() {
        assert_eq!(reference_refusal(&chat_thread(), "t-a", "t-now", false), None);
    }

    #[test]
    fn each_rule_refuses_with_its_own_reason() {
        assert_eq!(
            reference_refusal(&chat_thread(), "t-now", "t-now", false),
            Some("thread_reference_current_thread")
        );
        let space = ReferencedThread {
            space_id: Some("space-1".to_owned()),
            ..chat_thread()
        };
        assert_eq!(
            reference_refusal(&space, "t-a", "t-now", false),
            Some("thread_reference_not_chat")
        );
        let other_origin = ReferencedThread {
            origin: "mail".to_owned(),
            ..chat_thread()
        };
        assert_eq!(
            reference_refusal(&other_origin, "t-a", "t-now", false),
            Some("thread_reference_not_chat")
        );
        let archived = ReferencedThread {
            archived: true,
            ..chat_thread()
        };
        assert_eq!(
            reference_refusal(&archived, "t-a", "t-now", false),
            Some("thread_reference_archived")
        );
        assert_eq!(
            reference_refusal(&chat_thread(), "t-a", "t-now", true),
            Some("thread_reference_retention")
        );
    }

    #[test]
    fn only_the_owner_in_the_threads_org_reads_it() {
        assert!(owned_by(&chat_thread(), ORG, Some(OWNER)));
        assert!(!owned_by(&chat_thread(), ORG, Some(OTHER)));
        assert!(!owned_by(&chat_thread(), "org-other", Some(OWNER)));
        assert!(!owned_by(&chat_thread(), ORG, None), "a service credential holds no user");
    }

    #[test]
    fn the_limit_defaults_and_is_capped_at_twenty() {
        assert_eq!(clamp_limit(0), 20);
        assert_eq!(clamp_limit(5), 5);
        assert_eq!(clamp_limit(500), 20);
    }

    #[test]
    fn content_is_capped_at_4000_chars_and_says_so() {
        let (short, cut) = capped_content("kort");
        assert_eq!((short.as_str(), cut), ("kort", false));
        let exact = "ø".repeat(MAX_CONTENT_CHARS);
        assert_eq!(capped_content(&exact), (exact.clone(), false));
        let (long, cut) = capped_content(&"ø".repeat(MAX_CONTENT_CHARS + 1));
        assert!(cut);
        assert_eq!(long.chars().count(), MAX_CONTENT_CHARS);
    }

    #[test]
    fn the_outline_keeps_the_first_non_empty_line_only() {
        assert_eq!(first_line("\n\n  Budget   for Q3 \nsecond line"), "Budget for Q3");
        assert_eq!(first_line(""), "");
        assert_eq!(
            first_line(&"x".repeat(MAX_FIRST_LINE_CHARS + 40)).chars().count(),
            MAX_FIRST_LINE_CHARS
        );
    }

    /// A zero-retention credential is refused before any row is read, so no database is needed.
    #[tokio::test]
    async fn zdr_credential_is_refused_before_any_read() {
        use mp_contracts::zdr_refusal::Refusal;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool");
        let mut request = Request::new(page_request("t-a", "t-now"));
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test_with_zdr(ORG, OWNER, true));
        let status = ThreadReferenceGrpc { pool }
            .read_thread_for_reference(request)
            .await
            .expect_err("a ZDR credential is refused");
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
        assert_eq!(
            Refusal::from_status(&status).map(|refusal| refusal.code()),
            Some(mp_contracts::zdr_refusal::code::DURABLE_MUTATION_FORBIDDEN)
        );
    }

    fn page_request(thread: &str, current: &str) -> pb::ReadThreadForReferenceRequest {
        pb::ReadThreadForReferenceRequest {
            org_id: ORG.to_owned(),
            user_id: OWNER.to_owned(),
            thread_id: thread.to_owned(),
            mode: pb::ThreadReferenceMode::Page as i32,
            current_thread_id: current.to_owned(),
            ..Default::default()
        }
    }

    // The database tests below run with `--ignored` against a disposable Postgres, as the other
    // session-core database tests do. They call `read_reference` directly; the ZDR gate is the
    // RPC's and is tested above.

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_thread(pool: &PgPool, thread: &str, owner: &str) {
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(thread)
        .bind(ORG)
        .bind(owner)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn append(pool: &PgPool, thread: &str, owner: &str, role: &str, content: &str) -> String {
        let mut tx = pool.begin().await.unwrap();
        let request = pb::AppendMessageRequest {
            thread_id: thread.to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            ..Default::default()
        };
        let appended = crate::grpc::append_message_in_tx(&mut tx, &request, owner)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        appended.message_id
    }

    async fn seed_run(pool: &PgPool, thread: &str, retention_class: &str) {
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, metadata,
                               retention_class, waiver_ack_id, waiver_connection_id)
             VALUES ($1, $2, 'g', $3, $4, 'completed', '{}'::jsonb, $5,
                     CASE WHEN $5 = 'zdr_waived' THEN 'ack-1' END,
                     CASE WHEN $5 = 'zdr_waived' THEN 'conn-1' END)",
        )
        .bind(new_ulid())
        .bind(thread)
        .bind(ORG)
        .bind(OWNER)
        .bind(retention_class)
        .execute(pool)
        .await
        .unwrap();
    }

    fn owner() -> VerifiedIdentity {
        VerifiedIdentity::user_for_test(ORG, OWNER)
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn reads_owner_chat_thread_active_path() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread, OWNER).await;
        append(&pool, &thread, OWNER, "user", "What is the budget?").await;
        let answer = append(&pool, &thread, OWNER, "assistant", "The budget is 40k.").await;
        // A turn moved off the active path is not part of the conversation the owner sees.
        let hidden = append(&pool, &thread, OWNER, "user", "hidden branch").await;
        sqlx::query("UPDATE messages SET on_active_path = false WHERE id = $1")
            .bind(&hidden)
            .execute(&pool)
            .await
            .unwrap();

        let page = read_reference(&pool, &owner(), page_request(&thread, "t-now"))
            .await
            .unwrap();
        let contents: Vec<&str> = page.messages.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(contents, ["What is the budget?", "The budget is 40k."]);
        assert!(!page.has_more);
        assert!(page.outline.is_empty());

        // The cursor reads only the turns before it.
        let mut older = page_request(&thread, "t-now");
        older.before_message_id = answer;
        let older = read_reference(&pool, &owner(), older).await.unwrap();
        assert_eq!(older.messages.len(), 1);
        assert_eq!(older.messages[0].role, "user");

        let mut paged = page_request(&thread, "t-now");
        paged.limit = 1;
        let paged = read_reference(&pool, &owner(), paged).await.unwrap();
        assert_eq!(paged.messages.len(), 1);
        assert!(paged.has_more);
        assert_eq!(paged.messages[0].content, "The budget is 40k.");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn outline_lists_first_lines_only() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread, OWNER).await;
        append(&pool, &thread, OWNER, "user", "Plan the offsite\nwith details nobody needs").await;
        append(&pool, &thread, OWNER, "assistant", "\nHere is a plan\n1. Book rooms").await;

        let mut request = page_request(&thread, "t-now");
        request.mode = pb::ThreadReferenceMode::Outline as i32;
        let outline = read_reference(&pool, &owner(), request).await.unwrap();
        let lines: Vec<&str> = outline.outline.iter().map(|item| item.first_line.as_str()).collect();
        assert_eq!(lines, ["Plan the offsite", "Here is a plan"]);
        assert!(outline.messages.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn refuses_other_users_thread() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread, OTHER).await;
        append(&pool, &thread, OTHER, "user", "not yours").await;
        let refused = read_reference(&pool, &owner(), page_request(&thread, "t-now"))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn refuses_space_thread() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread, OWNER).await;
        sqlx::query(
            "UPDATE threads SET space_id = 'space-ref', origin = 'space', space_decision_ref = 'dec-t', recipient_audience_ref = 'aud-t', privacy_policy_ref = 'pp-t', resource_authorization_ref = 'ra-t', authority_revision = 1, recipient_audience_revision = 1, recipient_audience_hash = 'test-audience' WHERE id = $1",
        )
        .bind(&thread)
        .execute(&pool)
        .await
        .unwrap();
        let refused = read_reference(&pool, &owner(), page_request(&thread, "t-now"))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), "thread_reference_not_chat");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn refuses_waived_thread() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread, OWNER).await;
        append(&pool, &thread, OWNER, "user", "waived turn").await;
        seed_run(&pool, &thread, "zdr_waived").await;
        let refused = read_reference(&pool, &owner(), page_request(&thread, "t-now"))
            .await
            .unwrap_err();
        assert_eq!(refused.message(), "thread_reference_retention");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn refuses_thread_with_zdr_run() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread, OWNER).await;
        seed_run(&pool, &thread, "zdr").await;
        let refused = read_reference(&pool, &owner(), page_request(&thread, "t-now"))
            .await
            .unwrap_err();
        assert_eq!(refused.message(), "thread_reference_retention");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn refuses_current_thread() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread, OWNER).await;
        let refused = read_reference(&pool, &owner(), page_request(&thread, &thread))
            .await
            .unwrap_err();
        assert_eq!(refused.message(), "thread_reference_current_thread");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn refuses_archived_thread() {
        let pool = connect().await;
        let thread = new_ulid();
        seed_thread(&pool, &thread, OWNER).await;
        sqlx::query("UPDATE threads SET archived_at = now() WHERE id = $1")
            .bind(&thread)
            .execute(&pool)
            .await
            .unwrap();
        let refused = read_reference(&pool, &owner(), page_request(&thread, "t-now"))
            .await
            .unwrap_err();
        assert_eq!(refused.message(), "thread_reference_archived");
    }
}
