//! Real-Postgres proof of `SpaceEvents.StreamSpaceEventsStream` (WS02-S3). Every test is
//! `#[ignore]`d: run them with `--ignored --test-threads=1` and `DATABASE_URL` pointing at a
//! disposable Postgres whose database name contains `test` (the helpers refuse any other).
//!
//! Each test uses its own org and Space, switches the event log on for the test database and
//! cleans up after itself. Decisions are minted with the test key and verified through the
//! real handler, so the whole path (identity, decision, log read, gate, frames) is exercised.

use super::*;
use crate::auth::VerifiedIdentity;
use crate::grpc::space_w1_tests::{live_stream_token, StreamKeysEnv, DECISION_REF};
use crate::space::test_support::{
    clean_space, connect, seed_space_message, seed_space_thread, set_event_log,
};
use futures::StreamExt as _;

const NEEDS_DB: &str = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)";
const MEMBER: &str = "member-1";

type Frames = ReceiverStream<Result<pb::SpaceEventFrame, Status>>;

fn ids(prefix: &str) -> (String, String) {
    let suffix = mp_ids::new_ulid();
    (format!("org-{prefix}-{suffix}"), format!("space-{prefix}-{suffix}"))
}

struct Resume {
    generation: i64,
    after_seq: i64,
}

async fn open(
    pool: &PgPool,
    org: &str,
    space: &str,
    ceiling: i64,
    resume: Option<Resume>,
) -> Frames {
    let service = SpaceEventsGrpc::new(pool.clone(), SpaceWakes::new());
    let mut request = Request::new(pb::StreamSpaceEventsRequest {
        org_id: org.to_owned(),
        user_id: MEMBER.to_owned(),
        space_id: space.to_owned(),
        decision_ref: DECISION_REF.to_owned(),
        decision_token: live_stream_token(org, MEMBER, space, ceiling, false, |_| {}),
        resume: resume.is_some(),
        generation: resume.as_ref().map_or(0, |value| value.generation),
        after_seq: resume.as_ref().map_or(0, |value| value.after_seq),
    });
    request
        .extensions_mut()
        .insert(VerifiedIdentity::user_for_test(org, MEMBER));
    service
        .stream_space_events(request)
        .await
        .expect("the stream opens")
        .into_inner()
}

/// One word per frame: `connected`, `event:<kind>:<seq>`, `heartbeat`, `too_old:<reason>`,
/// `closing:<code>`.
fn word(frame: &pb::SpaceEventFrame) -> String {
    match frame.frame.as_ref().expect("a frame body") {
        Frame::Connected(_) => "connected".to_owned(),
        Frame::Event(record) => format!("event:{}:{}", record.kind, record.seq),
        Frame::Heartbeat(_) => "heartbeat".to_owned(),
        Frame::CursorTooOld(old) => format!("too_old:{}", old.reason),
        Frame::Closing(closing) => format!("closing:{}", closing.code),
    }
}

/// Reads a stream that ends by itself (a close, a too-old answer) to its end.
async fn until_end(mut stream: Frames) -> Vec<pb::SpaceEventFrame> {
    let mut frames = Vec::new();
    while let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(30), stream.next()).await {
        frames.push(item.expect("an ok frame"));
    }
    frames
}

async fn next_non_heartbeat(stream: &mut Frames) -> pb::SpaceEventFrame {
    loop {
        let item = tokio::time::timeout(Duration::from_secs(30), stream.next())
            .await
            .expect("a frame within 30 s")
            .expect("the stream is open")
            .expect("an ok frame");
        if !matches!(item.frame, Some(Frame::Heartbeat(_))) {
            return item;
        }
    }
}

async fn setup(prefix: &str, messages: usize) -> Option<(PgPool, String, String, String, StreamKeysEnv)> {
    let pool = connect().await?;
    set_event_log(&pool, true).await;
    let (org, space) = ids(prefix);
    let thread = format!("thread-{space}");
    seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
    for n in 0..messages {
        seed_space_message(&pool, &space, &thread, &format!("m{n}-{space}"), "user", Some("member-2"))
            .await;
    }
    Some((pool, org, space, thread, StreamKeysEnv::publish()))
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn stream_live_tail_starts_at_the_head_and_delivers_a_new_row() {
    let Some((pool, org, space, thread, _keys)) = setup("tail", 1).await else { return };
    let mut stream = open(&pool, &org, &space, 4, None).await;
    let connected = next_non_heartbeat(&mut stream).await;
    let Some(Frame::Connected(connected)) = connected.frame else {
        panic!("connected first, {NEEDS_DB}");
    };
    // The thread.created and the first message are before the head: not replayed.
    assert_eq!((connected.mode.as_str(), connected.head, connected.resumed), ("live", 2, false));
    seed_space_message(&pool, &space, &thread, &format!("late-{space}"), "user", Some("member-2")).await;
    let event = next_non_heartbeat(&mut stream).await;
    assert_eq!(word(&event), "event:message.appended:3");
    let Some(Frame::Event(record)) = event.frame else { unreachable!() };
    assert_eq!(record.actor_id, "member-2");
    assert!(!format!("{record:?}").contains("canary body"), "ids and closed words only");
    drop(stream);
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn stream_resume_honours_the_cursor() {
    let Some((pool, org, space, _thread, _keys)) = setup("resume", 3).await else { return };
    // Rows 1..=4 exist (thread.created plus three messages); resume after 2.
    let mut stream = open(&pool, &org, &space, 4, Some(Resume { generation: 1, after_seq: 2 })).await;
    let connected = next_non_heartbeat(&mut stream).await;
    let Some(Frame::Connected(connected)) = connected.frame else { panic!("connected first") };
    assert!(connected.resumed);
    assert_eq!(connected.cursor, 2);
    assert_eq!(word(&next_non_heartbeat(&mut stream).await), "event:message.appended:3");
    assert_eq!(word(&next_non_heartbeat(&mut stream).await), "event:message.appended:4");
    drop(stream);
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn stream_cursor_too_old_for_ahead_of_head_generation_changed_and_retention() {
    let Some((pool, org, space, _thread, _keys)) = setup("old", 3).await else { return };
    let ahead = until_end(open(&pool, &org, &space, 4, Some(Resume { generation: 1, after_seq: 99 })).await).await;
    assert_eq!(ahead.iter().map(word).collect::<Vec<_>>(), ["too_old:ahead_of_head"]);
    let Some(Frame::CursorTooOld(old)) = &ahead[0].frame else { unreachable!() };
    assert_eq!((old.head, old.generation), (4, 1));

    let changed = until_end(open(&pool, &org, &space, 4, Some(Resume { generation: 99, after_seq: 1 })).await).await;
    assert_eq!(changed.iter().map(word).collect::<Vec<_>>(), ["too_old:generation_changed"]);

    // Retention moved min_seq past the cursor.
    sqlx::query("UPDATE space_event_heads SET min_seq = 3 WHERE org_id = $1 AND space_ref = $2")
        .bind(&org)
        .bind(&space)
        .execute(&pool)
        .await
        .expect("move min_seq");
    let swept = until_end(open(&pool, &org, &space, 4, Some(Resume { generation: 1, after_seq: 0 })).await).await;
    assert_eq!(swept.iter().map(word).collect::<Vec<_>>(), ["too_old:retention"]);
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn stream_a_seq_hole_is_cursor_too_old_after_the_rows_before_it() {
    let Some((pool, org, space, _thread, _keys)) = setup("hole", 2).await else { return };
    sqlx::query("DELETE FROM space_event_log WHERE org_id = $1 AND space_ref = $2 AND seq = 2")
        .bind(&org)
        .bind(&space)
        .execute(&pool)
        .await
        .expect("punch a hole");
    let frames = until_end(open(&pool, &org, &space, 4, Some(Resume { generation: 1, after_seq: 0 })).await).await;
    assert_eq!(
        frames.iter().map(word).collect::<Vec<_>>(),
        ["connected", "event:thread.created:1", "too_old:retention"]
    );
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn stream_revocation_row_closes_membership_revoked_without_advancing() {
    let Some((pool, org, space, _thread, _keys)) = setup("revoke", 1).await else { return };
    sqlx::query(
        "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3)",
    )
    .bind(&space)
    .bind(&org)
    .bind(MEMBER)
    .execute(&pool)
    .await
    .expect("revoke the member");
    let frames = until_end(open(&pool, &org, &space, 4, Some(Resume { generation: 1, after_seq: 0 })).await).await;
    assert_eq!(frames.iter().map(word).collect::<Vec<_>>(), ["connected", "closing:membership_revoked"]);
    let Some(Frame::Closing(closing)) = &frames[1].frame else { unreachable!() };
    assert!(!closing.reconnect);
    assert_eq!(closing.cursor, 0, "no row was delivered to a revoked member");
    sqlx::query("DELETE FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2")
        .bind(&space)
        .bind(MEMBER)
        .execute(&pool)
        .await
        .ok();
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn stream_stale_audience_revision_closes_reauthorize_with_the_cursor_not_advanced() {
    let Some((pool, org, space, thread, _keys)) = setup("stale", 0).await else { return };
    // A message written at audience revision 2, after the decision's ceiling of 1.
    sqlx::query(
        "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
         recipient_audience_revision, recipient_audience_hash, authority_revision, \
         resource_authorization_ref, author_subject_id) \
         VALUES ($1, $2, 'user', 'canary body', $3, 'audience-1', 2, 'sha256:audience-2', 1, 'resource-1', 'member-2')",
    )
    .bind(format!("m-new-{space}"))
    .bind(&thread)
    .bind(&space)
    .execute(&pool)
    .await
    .expect("insert a newer-audience message");
    let frames = until_end(open(&pool, &org, &space, 1, Some(Resume { generation: 1, after_seq: 0 })).await).await;
    assert_eq!(
        frames.iter().map(word).collect::<Vec<_>>(),
        ["connected", "event:thread.created:1", "closing:reauthorize"]
    );
    let Some(Frame::Closing(closing)) = &frames[2].frame else { unreachable!() };
    assert!(closing.reconnect);
    assert_eq!(closing.cursor, 1, "the newer row was not delivered, so the cursor stays at 1");
    clean_space(&pool, &org, &space).await;
}

#[tokio::test]
#[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
async fn stream_thread_deleted_replay_reads_redacted() {
    let Some((pool, org, space, thread, _keys)) = setup("erase", 1).await else { return };
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("DELETE FROM messages WHERE thread_id = $1")
        .bind(&thread)
        .execute(&mut *tx)
        .await
        .expect("delete messages");
    sqlx::query("DELETE FROM threads WHERE id = $1")
        .bind(&thread)
        .execute(&mut *tx)
        .await
        .expect("delete thread");
    tx.commit().await.expect("commit");
    let mut stream = open(&pool, &org, &space, 4, Some(Resume { generation: 1, after_seq: 0 })).await;
    let mut seen = Vec::new();
    while seen.len() < 4 {
        let frame = next_non_heartbeat(&mut stream).await;
        let text = word(&frame);
        if let Some(Frame::Event(record)) = &frame.frame {
            if record.kind == "redacted" {
                assert_eq!(record.actor_id, "", "the erased row keeps no actor id");
                assert_eq!(record.thread_id, "");
                assert_eq!(record.message_id, "");
            }
        }
        seen.push(text);
    }
    assert_eq!(
        seen,
        [
            "connected",
            "event:redacted:1",
            "event:redacted:2",
            "event:thread.deleted:3"
        ]
    );
    drop(stream);
    clean_space(&pool, &org, &space).await;
}
