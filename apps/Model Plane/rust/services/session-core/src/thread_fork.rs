//! `ForkThread`: a new thread that starts with a copy of another thread's active path (build
//! plan WS17 S6).
//!
//! The copy runs from the source's first turn up to and including a chosen assistant message on
//! its active path. The new thread names its source and that message, and each copied turn names
//! the turn it was copied from, so the lineage is one query away. The source is not changed. A
//! copy is not a new conversation turn: it writes one `THREAD_FORKED` event and no
//! `MESSAGE_APPENDED`, and Dreaming never extracts from a copied turn, because the source already
//! produced what it says.
//!
//! Refused: a Space thread (its audience is not the owner's to copy), a thread that holds a waived
//! run (DEC-32: waived content is not copied into a new durable store), a point off the active
//! path, and a path longer than [`MAX_FORK_TURNS`]. A zero-retention credential is refused by the
//! `session:write` gate, and a service credential is refused because a fork is the owner's own
//! act. The copy is one transaction under the source's row lock, so an append cannot land between
//! the read and the copy. The switch is [`ENABLED_ENV`], default off; while it is off the RPC
//! answers `unavailable`.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{self as pb, thread_fork_service_server::ThreadForkService};
use mp_events::idempotency::derive_idempotency_hash;
use mp_ids::new_ulid;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Instant;
use tonic::{Request, Response, Status};
use tracing::info;

use crate::auth::{authorize_operation, identity, VerifiedIdentity};
use crate::grpc::record_metrics;

/// The environment switch for forking. Default off, read by the strict parser the other switches
/// share, so only the literal `true` turns it on.
pub(crate) const ENABLED_ENV: &str = "SESSION_CORE_THREAD_FORK_ENABLED";

/// The longest path a fork may copy. A longer path is refused rather than cut short: a copy that
/// stopped early would be a different conversation from the one the person chose.
pub(crate) const MAX_FORK_TURNS: usize = 5_000;

/// The type URL of the `THREAD_FORKED` event. Like `ThreadArchived`, it names no message type.
const THREAD_FORKED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.ThreadForked";

/// Read where it is used, as the message tree's switch is: the RPC holds no switch of its own.
pub(crate) fn thread_fork_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The fork RPC. It holds the pool and nothing else; the rules are in this module.
pub struct ThreadForkGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl ThreadForkService for ThreadForkGrpc {
    async fn fork_thread(
        &self,
        request: Request<pb::ForkThreadRequest>,
    ) -> Result<Response<pb::ForkThreadResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            if !thread_fork_enabled() {
                return Err(Status::unavailable("thread_fork_disabled"));
            }
            copy_thread(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("fork_thread", started, result.is_ok());
        result
    }
}

/// The source thread's settings, read under its row lock. The copy inherits the ones that decide
/// what the conversation may do: its source scope, whether persona applies and whether memory is
/// off for it.
struct SourceThread {
    org_id: String,
    user_id: String,
    space_id: Option<String>,
    source_scope: String,
    persona_enabled: bool,
    memory_mode: String,
}

/// One fork as it is being written: the new thread, the point it copies to and the settings it
/// inherits.
struct Fork<'a> {
    new_thread_id: String,
    org_id: &'a str,
    source_thread_id: &'a str,
    from_message_id: &'a str,
    source: SourceThread,
    title: &'a str,
    now: DateTime<Utc>,
}

/// Copy the active path of one owned thread into a new thread. The switch and the session scope
/// are the RPC's to check, so the copy itself can be tested without either.
pub(crate) async fn copy_thread(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::ForkThreadRequest,
) -> Result<pb::ForkThreadResponse, Status> {
    let org_id = req.org_id.trim();
    let thread_id = req.thread_id.trim();
    let from_message_id = req.from_message_id.trim();
    if org_id.is_empty() || thread_id.is_empty() || from_message_id.is_empty() {
        return Err(Status::invalid_argument(
            "org_id, thread_id and from_message_id are required",
        ));
    }
    caller.authorize_org(org_id)?;

    let mut tx = pool.begin().await.map_err(internal)?;
    let source = lock_source(&mut tx, thread_id).await?;
    if source.org_id != org_id {
        return Err(Status::not_found("thread not found"));
    }
    if !owner_may_fork(caller.user_id(), &source.user_id) {
        return Err(Status::permission_denied(
            "only the thread owner may fork it",
        ));
    }
    let in_space = source
        .space_id
        .as_deref()
        .is_some_and(|space| !space.trim().is_empty());
    if in_space || thread_is_waived(&mut tx, thread_id).await? {
        return Err(Status::failed_precondition("fork_source_not_forkable"));
    }

    let point: Option<(String, bool, i64)> = sqlx::query_as(
        "SELECT role, on_active_path, sequence FROM messages WHERE id = $1 AND thread_id = $2",
    )
    .bind(from_message_id)
    .bind(thread_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let (role, on_active_path, through) =
        point.ok_or_else(|| Status::not_found("message not found in this thread"))?;
    check_fork_point(&role, on_active_path)?;

    let source_ids = active_path_through(&mut tx, thread_id, through).await?;
    let new_ids: Vec<String> = source_ids.iter().map(|_| new_ulid()).collect();
    let fork = Fork {
        new_thread_id: new_ulid(),
        org_id,
        source_thread_id: thread_id,
        from_message_id,
        source,
        title: req.title.trim(),
        now: Utc::now(),
    };
    insert_fork_thread(&mut tx, &fork).await?;
    let copied = copy_turns(&mut tx, &fork.new_thread_id, thread_id, new_ids, source_ids).await?;
    insert_forked_event(&mut tx, &fork, copied).await?;
    tx.commit().await.map_err(internal)?;
    info!(new_thread_id = %fork.new_thread_id, copied, "thread forked");
    Ok(pb::ForkThreadResponse {
        new_thread_id: fork.new_thread_id,
        copied_message_count: u32::try_from(copied)
            .map_err(|_| Status::internal("copied turn count out of range"))?,
    })
}

/// Lock the source thread's row for the copy. An append takes the same lock, so the path read
/// after it is the path the copy takes, and no turn lands between the two.
async fn lock_source(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<SourceThread, Status> {
    let row: Option<(String, String, Option<String>, String, bool, String)> = sqlx::query_as(
        "SELECT org_id, user_id, space_id, source_scope, persona_enabled, memory_mode
         FROM threads WHERE id = $1 FOR UPDATE",
    )
    .bind(thread_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;
    let (org_id, user_id, space_id, source_scope, persona_enabled, memory_mode) =
        row.ok_or_else(|| Status::not_found("thread not found"))?;
    Ok(SourceThread {
        org_id,
        user_id,
        space_id,
        source_scope,
        persona_enabled,
        memory_mode,
    })
}

/// Whether the thread holds a run whose content was waived from retention (DEC-32).
async fn thread_is_waived(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<bool, Status> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE thread_id = $1 AND retention_class = 'zdr_waived')",
    )
    .bind(thread_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)
}

/// A fork starts from an assistant turn that the thread's active path holds. A user turn is
/// the question, not the answer a copy should end on, and a turn on an inactive branch is not
/// part of the conversation the person sees.
fn check_fork_point(role: &str, on_active_path: bool) -> Result<(), Status> {
    if role != "assistant" {
        return Err(Status::invalid_argument(
            "a fork starts from an assistant message",
        ));
    }
    if !on_active_path {
        return Err(Status::failed_precondition("fork_point_not_on_active_path"));
    }
    Ok(())
}

/// A fork is the owner's own act. The caller must be the person who owns the thread: a service
/// credential holds no user and so never forks, and nor does another person in the same org.
fn owner_may_fork(caller_user: Option<&str>, owner: &str) -> bool {
    caller_user.is_some_and(|user| user == owner)
}

/// The turns of the active path from its first turn through `through_sequence`, oldest first.
/// One row past the cap is read so that a path over it is recognised without reading the rest.
async fn active_path_through(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
    through_sequence: i64,
) -> Result<Vec<String>, Status> {
    let limit =
        i64::try_from(MAX_FORK_TURNS + 1).map_err(|_| Status::internal("fork cap out of range"))?;
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM messages
         WHERE thread_id = $1 AND on_active_path AND sequence <= $2
         ORDER BY sequence
         LIMIT $3",
    )
    .bind(thread_id)
    .bind(through_sequence)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal)?;
    check_path_length(ids.len())?;
    Ok(ids)
}

/// A path past the cap is refused, not cut short.
fn check_path_length(turns: usize) -> Result<(), Status> {
    if turns > MAX_FORK_TURNS {
        return Err(Status::failed_precondition("fork_source_too_large"));
    }
    Ok(())
}

/// Insert the new thread with its lineage. Its session key is its own, so it never meets the
/// source's idempotency key, and its origin is the chat surface, the only one a fork is made from.
async fn insert_fork_thread(
    tx: &mut Transaction<'_, Postgres>,
    fork: &Fork<'_>,
) -> Result<(), Status> {
    sqlx::query(
        "INSERT INTO threads
         (id, session_key, org_id, user_id, created_at, origin, source_scope, persona_enabled, memory_mode,
          presentation_title, forked_from_thread_id, forked_from_message_id)
         VALUES ($1, $2, $3, $4, $5, 'chat', $6, $7, $8, NULLIF($9, ''), $10, $11)",
    )
    .bind(&fork.new_thread_id)
    .bind(format!("fork:{}", fork.new_thread_id))
    .bind(fork.org_id)
    .bind(&fork.source.user_id)
    .bind(fork.now)
    .bind(&fork.source.source_scope)
    .bind(fork.source.persona_enabled)
    .bind(&fork.source.memory_mode)
    .bind(fork.title)
    .bind(fork.source_thread_id)
    .bind(fork.from_message_id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

/// Copy the turns in one statement. The new ids and the source ids are paired by position, and
/// the ordinal keeps the insert in path order, so the copy's sequence reads as the source's did.
/// Only the content and the attribution are carried over: the tree columns are left to their
/// defaults, which make the copy a plain legacy chain, and the copy writes no tree of its own.
async fn copy_turns(
    tx: &mut Transaction<'_, Postgres>,
    new_thread_id: &str,
    source_thread_id: &str,
    new_ids: Vec<String>,
    source_ids: Vec<String>,
) -> Result<usize, Status> {
    let expected = source_ids.len();
    let copied = sqlx::query(
        "INSERT INTO messages
         (id, thread_id, role, content, created_at, agent_name, metadata, author_subject_id, copied_from_message_id)
         SELECT c.new_id, $1, m.role, m.content, m.created_at, m.agent_name, m.metadata, m.author_subject_id, m.id
         FROM unnest($2::text[], $3::text[]) WITH ORDINALITY AS c(new_id, source_id, pos)
         JOIN messages m ON m.id = c.source_id AND m.thread_id = $4
         ORDER BY c.pos",
    )
    .bind(new_thread_id)
    .bind(new_ids)
    .bind(source_ids)
    .bind(source_thread_id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?
    .rows_affected();
    let copied =
        usize::try_from(copied).map_err(|_| Status::internal("copied turn count out of range"))?;
    if copied != expected {
        return Err(Status::internal(
            "the fork copied fewer turns than the active path holds",
        ));
    }
    Ok(copied)
}

/// Record the fork as one `THREAD_FORKED` event on the new thread, in the same transaction as the
/// copy. It is the only event a fork writes: no `MESSAGE_APPENDED`, and so no memory extraction.
async fn insert_forked_event(
    tx: &mut Transaction<'_, Postgres>,
    fork: &Fork<'_>,
    copied: usize,
) -> Result<(), Status> {
    let resource = format!("thread:{}", fork.new_thread_id);
    let idempotency = derive_idempotency_hash(
        "session-core",
        "THREAD_FORKED",
        &resource,
        &format!("{}:forked", fork.new_thread_id),
    );
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         VALUES ($1, 'THREAD_FORKED', $2, $3, $4, $5, $6, $2, '', $7, $8, $9, 'session-core', 1)",
    )
    .bind(new_ulid())
    .bind(&fork.new_thread_id)
    .bind(serde_json::json!({
        "thread_id": &fork.new_thread_id,
        "forked_from_thread_id": fork.source_thread_id,
        "forked_from_message_id": fork.from_message_id,
        "copied_message_count": copied,
    }))
    .bind(fork.now)
    .bind(fork.org_id)
    .bind(&fork.source.user_id)
    .bind(idempotency)
    .bind(&resource)
    .bind(THREAD_FORKED_TYPE_URL)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

fn internal(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORG: &str = "org-fork";
    const OWNER: &str = "user-fork-owner";
    const OTHER: &str = "user-fork-other";

    #[test]
    fn a_fork_starts_only_from_an_assistant_turn_on_the_active_path() {
        assert!(check_fork_point("assistant", true).is_ok());
        assert_eq!(
            check_fork_point("user", true).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            check_fork_point("assistant", false).unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
    }

    #[test]
    fn only_the_person_who_owns_a_thread_may_fork_it() {
        assert!(owner_may_fork(Some(OWNER), OWNER));
        assert!(!owner_may_fork(Some(OTHER), OWNER));
        assert!(
            !owner_may_fork(None, OWNER),
            "a service credential holds no user"
        );
    }

    #[test]
    fn a_path_past_the_cap_is_refused_rather_than_cut_short() {
        assert!(check_path_length(MAX_FORK_TURNS).is_ok());
        assert_eq!(
            check_path_length(MAX_FORK_TURNS + 1).unwrap_err().message(),
            "fork_source_too_large"
        );
    }

    #[test]
    fn the_switch_is_off_unless_it_reads_true() {
        assert!(!mp_contracts::loop_fold::switch_from(None));
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("yes")));
    }

    // The database tests below run with `--ignored` against a disposable Postgres, as the other
    // session-core database tests do. They call `copy_thread` directly, so the switch is not
    // needed; the switch is tested by `fork_is_refused_while_the_switch_is_off`.

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

    fn fork_request(thread: &str, from: &str) -> pb::ForkThreadRequest {
        pb::ForkThreadRequest {
            org_id: ORG.to_owned(),
            thread_id: thread.to_owned(),
            from_message_id: from.to_owned(),
            title: String::new(),
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn fork_copies_active_path_up_to_message() {
        let pool = connect().await;
        let source = new_ulid();
        seed_thread(&pool, &source).await;
        append(&pool, &source, "user", "first question").await;
        append(&pool, &source, "assistant", "first answer").await;
        append(&pool, &source, "user", "second question").await;
        let answer = append(&pool, &source, "assistant", "second answer").await;
        append(&pool, &source, "user", "third question").await;
        append(&pool, &source, "assistant", "third answer").await;

        let owner = VerifiedIdentity::user_for_test(ORG, OWNER);
        let response = copy_thread(&pool, &owner, fork_request(&source, &answer))
            .await
            .unwrap();
        assert_eq!(response.copied_message_count, 4);

        let copied: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT content, copied_from_message_id FROM messages WHERE thread_id = $1 ORDER BY sequence",
        )
        .bind(&response.new_thread_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        let contents: Vec<&str> = copied.iter().map(|(content, _)| content.as_str()).collect();
        assert_eq!(
            contents,
            [
                "first question",
                "first answer",
                "second question",
                "second answer"
            ]
        );
        assert!(copied.iter().all(|(_, from)| from.is_some()));

        let lineage: (Option<String>, Option<String>, String) = sqlx::query_as(
            "SELECT forked_from_thread_id, forked_from_message_id, origin FROM threads WHERE id = $1",
        )
        .bind(&response.new_thread_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            lineage,
            (
                Some(source.clone()),
                Some(answer.clone()),
                "chat".to_owned()
            )
        );

        let source_turns: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE thread_id = $1")
                .bind(&source)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(source_turns, 6, "the source keeps every turn");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn fork_emits_one_thread_forked_event_and_no_message_appended() {
        let pool = connect().await;
        let source = new_ulid();
        seed_thread(&pool, &source).await;
        append(&pool, &source, "user", "question").await;
        let answer = append(&pool, &source, "assistant", "answer").await;

        let owner = VerifiedIdentity::user_for_test(ORG, OWNER);
        let response = copy_thread(&pool, &owner, fork_request(&source, &answer))
            .await
            .unwrap();
        let forked: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE run_id = $1 AND event_type = 'THREAD_FORKED'",
        )
        .bind(&response.new_thread_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(forked, 1);
        let appended: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE run_id = $1 AND event_type = 'MESSAGE_APPENDED'",
        )
        .bind(&response.new_thread_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(appended, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn fork_is_owner_bound() {
        let pool = connect().await;
        let source = new_ulid();
        seed_thread(&pool, &source).await;
        let answer = append(&pool, &source, "assistant", "answer").await;

        let other = VerifiedIdentity::user_for_test(ORG, OTHER);
        let refused = copy_thread(&pool, &other, fork_request(&source, &answer))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn fork_refuses_space_waived_and_zdr() {
        let pool = connect().await;
        let source = new_ulid();
        seed_thread(&pool, &source).await;
        let answer = append(&pool, &source, "assistant", "answer").await;
        let owner = VerifiedIdentity::user_for_test(ORG, OWNER);

        sqlx::query(
            "UPDATE threads SET space_id = 'space-fork', origin = 'space', space_decision_ref = 'dec-t', recipient_audience_ref = 'aud-t', privacy_policy_ref = 'pp-t', resource_authorization_ref = 'ra-t', authority_revision = 1, recipient_audience_revision = 1, recipient_audience_hash = 'test-audience' WHERE id = $1",
        )
            .bind(&source)
            .execute(&pool)
            .await
            .unwrap();
        let in_space = copy_thread(&pool, &owner, fork_request(&source, &answer))
            .await
            .unwrap_err();
        assert_eq!(in_space.message(), "fork_source_not_forkable");
        sqlx::query(
            "UPDATE threads SET space_id = NULL, origin = 'chat', space_decision_ref = NULL, recipient_audience_ref = NULL, privacy_policy_ref = NULL, resource_authorization_ref = NULL, authority_revision = NULL, recipient_audience_revision = NULL, recipient_audience_hash = NULL WHERE id = $1",
        )
            .bind(&source)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, metadata,
                               retention_class, waiver_ack_id, waiver_connection_id)
             VALUES ($1, $2, 'g', $3, $4, 'completed', '{}'::jsonb,
                     'zdr_waived', 'ack-1', 'conn-1')",
        )
        .bind(new_ulid())
        .bind(&source)
        .bind(ORG)
        .bind(OWNER)
        .execute(&pool)
        .await
        .unwrap();
        let waived = copy_thread(&pool, &owner, fork_request(&source, &answer))
            .await
            .unwrap_err();
        assert_eq!(waived.message(), "fork_source_not_forkable");

        // A zero-retention credential is refused at the session:write gate, before the switch
        // and before any read, so the RPC needs no switch to refuse it.
        let mut request = Request::new(fork_request(&source, &answer));
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test_with_zdr(ORG, OWNER, true));
        let zdr = ThreadForkGrpc { pool: pool.clone() }
            .fork_thread(request)
            .await
            .unwrap_err();
        assert_eq!(zdr.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn fork_of_a_path_past_the_cap_is_refused() {
        let pool = connect().await;
        let source = new_ulid();
        seed_thread(&pool, &source).await;
        // One more turn than the cap, ending on an assistant turn (an even position).
        let turns = i64::try_from(MAX_FORK_TURNS + 2).unwrap();
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content)
             SELECT $1::text || '-' || g::text, $1,
                    CASE WHEN g % 2 = 0 THEN 'assistant' ELSE 'user' END, 'turn'
             FROM generate_series(1, $2) AS g",
        )
        .bind(&source)
        .bind(turns)
        .execute(&pool)
        .await
        .unwrap();

        let last = format!("{source}-{turns}");
        let owner = VerifiedIdentity::user_for_test(ORG, OWNER);
        let refused = copy_thread(&pool, &owner, fork_request(&source, &last))
            .await
            .unwrap_err();
        assert_eq!(refused.message(), "fork_source_too_large");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn fork_is_refused_while_the_switch_is_off() {
        let pool = connect().await;
        let source = new_ulid();
        seed_thread(&pool, &source).await;
        let answer = append(&pool, &source, "assistant", "answer").await;

        // The switch is read from the environment; the suite runs with it unset, which is the
        // default this test proves.
        let mut request = Request::new(fork_request(&source, &answer));
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test(ORG, OWNER));
        let refused = ThreadForkGrpc { pool: pool.clone() }
            .fork_thread(request)
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::Unavailable);
        assert_eq!(refused.message(), "thread_fork_disabled");
    }
}
