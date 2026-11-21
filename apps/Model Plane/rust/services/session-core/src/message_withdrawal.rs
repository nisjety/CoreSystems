//! Taking back a mid-run message that is already stored (build plan O-01, DEC-F01-03).
//!
//! A message the person sends while a run is working is stored under its `client_message_id` before
//! the run can read it (WS04 S6, the 2026-08-24 "persist before deliver" ledger entry). When the
//! gateway's queue still holds it, the person may take it back, and the gateway then asks this service
//! to withdraw the stored copy. The ledger entry is reversed for that one case: a message the run never
//! read is not a message the transcript keeps.
//!
//! # What a withdrawal does to the row
//!
//! One statement stamps `messages.withdrawn_at` (migration 0134), clears the text and the metadata (as
//! a deleted Space message is cleared), and takes the row off the active path. The last one is what
//! keeps every reader honest without touching each of them: the transcript, the prompts, learning,
//! Dreaming, search and export all filter on `on_active_path` whether or not message versions are on
//! (migration 0061), so a withdrawn turn leaves all of them at once, and the cleared text means a
//! reader that was missed still holds nothing to leak. The readers that do not filter on the flag
//! (the tree rules, the sibling count, the thread view) name `withdrawn_at` themselves.
//!
//! The thread view alone asks for the withdrawn rows (`ListConversationRequest.include_withdrawn_notes`)
//! and answers one text-free note, `SessionMessage.withdrawn`, in each one's place.
//!
//! # Tree threads
//!
//! With message versions on, the next turn of the run continues the withdrawn one (it was the leaf when
//! the run's answer was appended). The turns that continue it are therefore re-parented to its parent,
//! and the thread's leaf moves back to its parent when it was the leaf, so the chain from the first
//! turn to the leaf stays unbroken and never passes through a withdrawn turn.
//!
//! # Who may
//!
//! The thread owner, and only for a user message of their own: a message with an author on record
//! (a shared Space) must be the caller's. A Space thread is refused: its messages are removed through
//! a revision under a Control decision (WS12-S4), not here. A message an assistant answer already
//! follows is refused (`message_already_answered`): the run read it.
//!
//! Idempotent: withdrawing a withdrawn message answers `already_withdrawn` and changes nothing.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    self as pb, message_withdrawal_service_server::MessageWithdrawalService,
};
use mp_contracts::zdr_refusal::Refusal;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Instant;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, identity, OwnerIntent, VerifiedIdentity};
use crate::grpc::{authorize_thread_owner, record_metrics, to_proto_timestamp};

/// `MessageWithdrawalService`: the take-back of a stored mid-run message. Served beside `SessionCore`
/// behind the verifying interceptor, so that trait and its mocks are unchanged.
pub struct MessageWithdrawalGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl MessageWithdrawalService for MessageWithdrawalGrpc {
    async fn withdraw_message(
        &self,
        request: Request<pb::WithdrawMessageRequest>,
    ) -> Result<Response<pb::WithdrawMessageResponse>, Status> {
        let started = Instant::now();
        let result = self.withdraw_checked(request).await;
        record_metrics("withdraw_message", started, result.is_ok());
        result
    }
}

impl MessageWithdrawalGrpc {
    async fn withdraw_checked(
        &self,
        request: Request<pb::WithdrawMessageRequest>,
    ) -> Result<Response<pb::WithdrawMessageResponse>, Status> {
        let caller = identity(&request)?;
        // A zero-retention caller is refused typed here: it keeps nothing, so it has nothing to take back.
        authorize_operation(&caller, "session:write")?;
        refuse_waived(&caller)?;
        let req = request.into_inner();
        if req.org_id.is_empty() || req.thread_id.is_empty() || req.client_message_id.is_empty() {
            return Err(Status::invalid_argument(
                "org_id, thread_id and client_message_id are required",
            ));
        }
        if !valid_client_message_id(&req.client_message_id) {
            return Err(Status::invalid_argument(
                "client_message_id is not a valid message id",
            ));
        }
        caller.authorize_org(&req.org_id)?;
        authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Mutate).await?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let response = withdraw_in_tx(&mut tx, &caller, &req).await?;
        tx.commit().await.map_err(internal)?;
        Ok(Response::new(response))
    }
}

/// A waived caller keeps no stored copy of anything new (a waived run counts as zero-retention), so it
/// has no queued message to take back; refused with the catalogue code for a waiver on a route that
/// does not honour it.
#[allow(clippy::result_large_err)]
fn refuse_waived(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr_waiver().is_some() {
        return Err(Refusal::WAIVER_ROUTE_MISMATCH.grpc_status(tonic::Code::FailedPrecondition));
    }
    Ok(())
}

/// The shape `messages_client_message_id_check` allows, so a malformed id is a bad argument here and
/// not a database error.
fn valid_client_message_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && id.len() <= 128
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn internal(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

/// What the stored message is, read under the thread lock.
#[derive(sqlx::FromRow)]
struct StoredMessage {
    id: String,
    role: String,
    sequence: i64,
    author_subject_id: Option<String>,
    parent_message_id: Option<String>,
    withdrawn_at: Option<DateTime<Utc>>,
}

/// Why a stored message cannot be withdrawn by `caller`, decided before any row changes. `None` when it
/// can. A message of another author answers like a missing one, so an id is no oracle.
fn refusal_for(message: &StoredMessage, caller_user: Option<&str>) -> Option<Status> {
    if message.role != "user" {
        return Some(Status::not_found("message not found"));
    }
    match (message.author_subject_id.as_deref(), caller_user) {
        (Some(author), Some(user)) if author == user => None,
        (Some(_), _) => Some(Status::not_found("message not found")),
        (None, _) => None,
    }
}

/// The withdrawal itself, under the thread lock: every refusal is decided before any row changes.
async fn withdraw_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    caller: &VerifiedIdentity,
    req: &pb::WithdrawMessageRequest,
) -> Result<pb::WithdrawMessageResponse, Status> {
    let (org_id, space_id): (String, Option<String>) =
        sqlx::query_as("SELECT org_id, space_id FROM threads WHERE id = $1 FOR UPDATE")
            .bind(&req.thread_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(internal)?
            .ok_or_else(|| Status::not_found("thread not found"))?;
    if org_id != req.org_id {
        return Err(Status::not_found("thread not found"));
    }
    if space_id.is_some() {
        return Err(Status::failed_precondition(
            "a Space thread removes a message through a revision",
        ));
    }
    let message: StoredMessage = sqlx::query_as(
        "SELECT id, role, sequence, author_subject_id, parent_message_id, withdrawn_at
         FROM messages
         WHERE thread_id = $1 AND client_message_id = $2
         FOR UPDATE",
    )
    .bind(&req.thread_id)
    .bind(&req.client_message_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?
    .ok_or_else(|| Status::not_found("message not found"))?;
    if let Some(refusal) = refusal_for(&message, caller.user_id()) {
        return Err(refusal);
    }
    if let Some(withdrawn_at) = message.withdrawn_at {
        return Ok(pb::WithdrawMessageResponse {
            message_id: message.id,
            already_withdrawn: true,
            withdrawn_at: Some(to_proto_timestamp(withdrawn_at)),
        });
    }
    // The run read it once an assistant answer follows it. The gateway's queue decides the race
    // between a delivery and a take-back; this is the stored record's own backstop.
    let answered: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM messages
             WHERE thread_id = $1 AND role = 'assistant' AND sequence > $2 AND withdrawn_at IS NULL
         )",
    )
    .bind(&req.thread_id)
    .bind(message.sequence)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)?;
    if answered {
        return Err(Status::failed_precondition("message_already_answered"));
    }
    // The turns that continued it continue its parent, and the leaf steps back when it was the leaf,
    // so a versioned thread's chain never passes through a withdrawn turn. No rows match in a thread
    // that keeps no tree.
    sqlx::query(
        "UPDATE messages SET parent_message_id = $3::TEXT
         WHERE thread_id = $1 AND parent_message_id = $2",
    )
    .bind(&req.thread_id)
    .bind(&message.id)
    .bind(message.parent_message_id.as_deref())
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    sqlx::query(
        "UPDATE threads SET active_leaf_message_id = $3::TEXT
         WHERE id = $1 AND active_leaf_message_id = $2",
    )
    .bind(&req.thread_id)
    .bind(&message.id)
    .bind(message.parent_message_id.as_deref())
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    let withdrawn_at: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE messages
         SET withdrawn_at = now(), content = '', metadata = '{}'::jsonb, on_active_path = FALSE
         WHERE id = $1
         RETURNING withdrawn_at",
    )
    .bind(&message.id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(pb::WithdrawMessageResponse {
        message_id: message.id,
        already_withdrawn: false,
        withdrawn_at: Some(to_proto_timestamp(withdrawn_at)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(role: &str, author: Option<&str>) -> StoredMessage {
        StoredMessage {
            id: "m-1".into(),
            role: role.into(),
            sequence: 3,
            author_subject_id: author.map(str::to_owned),
            parent_message_id: None,
            withdrawn_at: None,
        }
    }

    #[test]
    fn a_client_message_id_has_the_shape_the_messages_check_allows() {
        for good in ["a", "0", "msg_01-X", &"a".repeat(128)] {
            assert!(valid_client_message_id(good), "{good}");
        }
        for bad in [
            "",
            "-a",
            "_a",
            "a b",
            "a.b",
            "a/b",
            "é",
            "a\n",
            &"a".repeat(129),
        ] {
            assert!(!valid_client_message_id(bad), "{bad:?}");
        }
    }

    /// Only a user message of the caller's own is withdrawn. A message of another author, and one that
    /// is no user message, answer exactly like a missing one.
    #[test]
    fn only_a_user_message_of_the_callers_own_can_be_withdrawn() {
        assert!(
            refusal_for(&stored("user", None), Some("u1")).is_none(),
            "an ordinary thread names no author"
        );
        assert!(refusal_for(&stored("user", Some("u1")), Some("u1")).is_none());
        for refused in [
            refusal_for(&stored("user", Some("u2")), Some("u1")),
            refusal_for(&stored("user", Some("u1")), None),
            refusal_for(&stored("assistant", None), Some("u1")),
            refusal_for(&stored("tool", Some("u1")), Some("u1")),
        ] {
            let refused = refused.expect("refused");
            assert_eq!(refused.code(), tonic::Code::NotFound);
            assert_eq!(refused.message(), "message not found");
        }
    }

    /// The readers that do not lean on the active-path flag name `withdrawn_at` themselves: the search
    /// candidates and the tree rules (the turn list a move reads, the sibling counts and the leaf's
    /// staleness check). A statement that dropped the predicate would count or return a withdrawn turn.
    #[test]
    fn the_readers_that_do_not_filter_on_the_active_path_name_withdrawn_at() {
        let search = crate::message_search::search_sql();
        assert!(
            search.contains("m.on_active_path"),
            "search reads the active path"
        );
        assert!(
            search.contains("m.withdrawn_at IS NULL"),
            "and says so for a withdrawn turn too"
        );

        let tree = include_str!("message_tree.rs");
        assert!(
            tree.matches("withdrawn_at IS NULL").count() >= 4,
            "load_turns, the sibling ordinal, the link_chain ranking and the stale-leaf check each leave a withdrawn turn out"
        );
        let conversation = include_str!("grpc.rs");
        assert!(
            conversation.contains("AND s.withdrawn_at IS NULL"),
            "the sibling count leaves a withdrawn turn out"
        );
        assert!(
            conversation.contains(
                "CASE WHEN m.withdrawn_at IS NULL THEN ($6 OR m.on_active_path) ELSE $9 END"
            ),
            "the conversation read shows a withdrawn turn only to a read that asked for the notes"
        );
    }

    #[tokio::test]
    async fn a_zero_retention_or_waived_caller_is_refused_before_any_thread_lookup() {
        let service = MessageWithdrawalGrpc {
            pool: sqlx::postgres::PgPoolOptions::new()
                .acquire_timeout(std::time::Duration::from_millis(250))
                .connect_lazy("postgresql://session-core@127.0.0.1:1/session_core")
                .expect("lazy pool"),
        };
        let request = |identity: VerifiedIdentity| {
            let mut request = Request::new(pb::WithdrawMessageRequest {
                org_id: "org-1".into(),
                thread_id: "thread-1".into(),
                client_message_id: "msg-1".into(),
            });
            request.extensions_mut().insert(identity);
            request
        };
        let zdr = service
            .withdraw_message(request(VerifiedIdentity::user_for_test_with_zdr(
                "org-1", "u1", true,
            )))
            .await
            .expect_err("a zero-retention credential keeps nothing to take back");
        assert_eq!(zdr.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            Refusal::from_status(&zdr),
            Some(Refusal::DURABLE_MUTATION_FORBIDDEN),
            "the typed catalogue refusal, not a bare status"
        );
        let waived = service
            .withdraw_message(request(VerifiedIdentity::user_for_test_with_waiver(
                "org-1", "u1", "conn-1", "ack-1",
            )))
            .await
            .expect_err("a waived caller is refused too");
        assert_eq!(waived.code(), tonic::Code::FailedPrecondition);
        assert_eq!(
            Refusal::from_status(&waived),
            Some(Refusal::WAIVER_ROUTE_MISMATCH)
        );
    }

    #[tokio::test]
    async fn the_request_must_name_the_thread_and_a_valid_message_id() {
        let service = MessageWithdrawalGrpc {
            pool: sqlx::postgres::PgPoolOptions::new()
                .acquire_timeout(std::time::Duration::from_millis(250))
                .connect_lazy("postgresql://session-core@127.0.0.1:1/session_core")
                .expect("lazy pool"),
        };
        for (thread, id) in [("", "msg-1"), ("thread-1", ""), ("thread-1", "bad id")] {
            let mut request = Request::new(pb::WithdrawMessageRequest {
                org_id: "org-1".into(),
                thread_id: thread.into(),
                client_message_id: id.into(),
            });
            request
                .extensions_mut()
                .insert(VerifiedIdentity::user_for_test("org-1", "u1"));
            let refused = service
                .withdraw_message(request)
                .await
                .expect_err("refused before any lookup");
            assert_eq!(
                refused.code(),
                tonic::Code::InvalidArgument,
                "{thread:?}/{id:?}"
            );
        }
    }

    // --- Database tests: run by the harness against a disposable Postgres. ---

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    /// A thread of `org` owned by "test-user" with one user message stored under `client_id`, and the
    /// message ids of everything in it in order.
    async fn seed(pool: &PgPool, org: &str, thread: &str, rows: &[(&str, &str, Option<&str>)]) {
        sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, 'test-user')")
            .bind(thread)
            .bind(org)
            .execute(pool)
            .await
            .unwrap();
        for (id, role, client_id) in rows {
            sqlx::query(
                "INSERT INTO messages (id, thread_id, role, content, client_message_id)
                 VALUES ($1, $2, $3, 'secret words', $4)",
            )
            .bind(id)
            .bind(thread)
            .bind(role)
            .bind(client_id)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    fn withdraw_request(
        org: &str,
        thread: &str,
        client_id: &str,
    ) -> Request<pb::WithdrawMessageRequest> {
        let mut request = Request::new(pb::WithdrawMessageRequest {
            org_id: org.into(),
            thread_id: thread.into(),
            client_message_id: client_id.into(),
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test(org, "test-user"));
        request
    }

    /// The plan's live proof, as a test: the stored row is withdrawn with its text gone and off the
    /// active path, a repeat changes nothing, the thread view alone reads a text-free note in its place,
    /// and a message the run already answered is refused.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_withdrawn_message_leaves_every_reader_and_the_thread_view_keeps_a_note() {
        let pool = connect().await;
        let suffix = mp_ids::new_ulid();
        let (org, thread) = (
            format!("withdraw-org-{suffix}"),
            format!("withdraw-thread-{suffix}"),
        );
        let (first, queued) = (format!("m-first-{suffix}"), format!("m-queued-{suffix}"));
        let client_id = format!("msg-{suffix}");
        seed(
            &pool,
            &org,
            &thread,
            &[(&first, "user", None), (&queued, "user", Some(&client_id))],
        )
        .await;
        let service = MessageWithdrawalGrpc { pool: pool.clone() };

        let done = service
            .withdraw_message(withdraw_request(&org, &thread, &client_id))
            .await
            .expect("withdrawn")
            .into_inner();
        assert_eq!(
            (done.message_id.as_str(), done.already_withdrawn),
            (queued.as_str(), false)
        );
        let (content, on_path, withdrawn): (String, bool, bool) = sqlx::query_as(
            "SELECT content, on_active_path, withdrawn_at IS NOT NULL FROM messages WHERE id = $1",
        )
        .bind(&queued)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            (content.as_str(), on_path, withdrawn),
            ("", false, true),
            "withdrawn, text gone, off the active path"
        );

        let again = service
            .withdraw_message(withdraw_request(&org, &thread, &client_id))
            .await
            .expect("idempotent")
            .into_inner();
        assert!(again.already_withdrawn);
        assert_eq!(
            again.withdrawn_at, done.withdrawn_at,
            "a repeat changes nothing"
        );

        // The prompt, learning and export readers read the active path: the withdrawn row is not on it.
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE thread_id = $1 AND on_active_path",
        )
        .bind(&thread)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(active, 1);

        // Another caller's take-back is not found, and so is an id the thread never held.
        for id in ["msg-unknown"] {
            let refused = service
                .withdraw_message(withdraw_request(&org, &thread, id))
                .await
                .expect_err("not found");
            assert_eq!(refused.code(), tonic::Code::NotFound);
        }
    }

    /// An assistant answer after the message means the run read it: the stored record refuses.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_message_the_run_already_answered_is_not_withdrawn() {
        let pool = connect().await;
        let suffix = mp_ids::new_ulid();
        let (org, thread) = (
            format!("withdraw-org-{suffix}"),
            format!("withdraw-thread-{suffix}"),
        );
        let (queued, answer) = (format!("m-queued-{suffix}"), format!("m-answer-{suffix}"));
        let client_id = format!("msg-{suffix}");
        seed(
            &pool,
            &org,
            &thread,
            &[
                (&queued, "user", Some(&client_id)),
                (&answer, "assistant", None),
            ],
        )
        .await;
        let service = MessageWithdrawalGrpc { pool: pool.clone() };
        let refused = service
            .withdraw_message(withdraw_request(&org, &thread, &client_id))
            .await
            .expect_err("answered");
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), "message_already_answered");
        let withdrawn: bool =
            sqlx::query_scalar("SELECT withdrawn_at IS NOT NULL FROM messages WHERE id = $1")
                .bind(&queued)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!withdrawn);
    }

    /// With message versions on, the turn that continued the withdrawn one continues its parent and
    /// the leaf steps back, so the chain never passes through the withdrawn turn.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres"]
    async fn a_versioned_thread_keeps_an_unbroken_chain_around_the_withdrawn_turn() {
        let pool = connect().await;
        let suffix = mp_ids::new_ulid();
        let (org, thread) = (
            format!("withdraw-org-{suffix}"),
            format!("withdraw-thread-{suffix}"),
        );
        let (first, queued) = (format!("m-first-{suffix}"), format!("m-queued-{suffix}"));
        let client_id = format!("msg-{suffix}");
        seed(
            &pool,
            &org,
            &thread,
            &[(&first, "user", None), (&queued, "user", Some(&client_id))],
        )
        .await;
        sqlx::query("UPDATE messages SET parent_message_id = $2 WHERE id = $1")
            .bind(&queued)
            .bind(&first)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE threads SET active_leaf_message_id = $2 WHERE id = $1")
            .bind(&thread)
            .bind(&queued)
            .execute(&pool)
            .await
            .unwrap();
        MessageWithdrawalGrpc { pool: pool.clone() }
            .withdraw_message(withdraw_request(&org, &thread, &client_id))
            .await
            .expect("withdrawn");
        let leaf: Option<String> =
            sqlx::query_scalar("SELECT active_leaf_message_id FROM threads WHERE id = $1")
                .bind(&thread)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            leaf.as_deref(),
            Some(first.as_str()),
            "the leaf steps back to the withdrawn turn's parent"
        );
    }
}
