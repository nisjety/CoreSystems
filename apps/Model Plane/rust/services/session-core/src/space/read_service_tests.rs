//! Tests of `SpaceRoomReadService::ListSpaceArtifacts` (WS07-S9), a child module of
//! `read_service`.
//!
//! The first half is pure: the keyset cursor, the limit clamp and every refusal that is
//! answered before a row is read (the pool is lazy and never connects). The second half
//! (`db_tests`) is `#[ignore]`d: run it with
//! `cargo test -p session-core --bin session-core space_room_read -- --ignored --test-threads=1`
//! and `DATABASE_URL` pointing at a disposable Postgres whose database name contains `test`.

use super::*;
use crate::auth::VerifiedIdentity;
use crate::grpc::space_w1_tests::{mint, StreamKeysEnv, DECISION_REF};
use serde_json::json;

const MODEL_PLANE_AUDIENCE: &str = "model-plane";

fn lazy_pool() -> PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgresql://unreachable.invalid/session")
        .expect("lazy pool")
}

/// A read decision for the room, valid at the wall clock now. `edit` runs before the digest.
fn read_token(
    org: &str,
    user: &str,
    space: &str,
    revision: i64,
    zdr: bool,
    edit: impl FnOnce(&mut serde_json::Value),
) -> String {
    mint(
        MODEL_PLANE_AUDIENCE,
        &["thread:read"],
        zdr,
        |claims| {
            claims["org_id"] = json!(org);
            claims["subject_id"] = json!(user);
            claims["space_ref"] = json!(space);
            claims["recipient_audience_revision"] = json!(revision);
            claims["issued_at"] = json!(Utc::now().to_rfc3339());
            claims["expires_at"] = json!((Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
            edit(claims);
        },
        false,
    )
}

fn list_request(token: &str, decision_ref: &str) -> pb::ListSpaceArtifactsRequest {
    pb::ListSpaceArtifactsRequest {
        org_id: "org-1".to_owned(),
        space_id: "space-room".to_owned(),
        space_read_decision_ref: decision_ref.to_owned(),
        space_read_decision_token: token.to_owned(),
        ..Default::default()
    }
}

async fn call(
    pool: PgPool,
    identity: VerifiedIdentity,
    req: pb::ListSpaceArtifactsRequest,
) -> Result<pb::ListSpaceArtifactsResponse, Status> {
    let mut request = Request::new(req);
    request.extensions_mut().insert(identity);
    SpaceRoomReadGrpc::new(pool)
        .list_space_artifacts(request)
        .await
        .map(Response::into_inner)
}

fn member() -> VerifiedIdentity {
    VerifiedIdentity::user_for_test("org-1", "user-1")
}

// ---- cursor and limit ----------------------------------------------------------------------

#[test]
fn artifact_cursor_round_trips_to_the_microsecond() {
    let cursor = ArtifactCursor {
        created_at: DateTime::from_timestamp(1_800_000_000, 123_456_000).expect("time"),
        message_id: "01HZMESSAGE".to_owned(),
        artifact_id: "art-1".to_owned(),
    };
    let decoded = decode_artifact_cursor(&encode_artifact_cursor(&cursor)).expect("round trip");
    assert_eq!(decoded.created_at, cursor.created_at);
    assert_eq!(decoded.message_id, cursor.message_id);
    assert_eq!(decoded.artifact_id, cursor.artifact_id);
}

#[test]
fn artifact_cursor_rejects_anything_that_is_not_a_cursor() {
    let encode = |value: serde_json::Value| URL_SAFE_NO_PAD.encode(value.to_string());
    for raw in [
        "!!!".to_owned(),
        URL_SAFE_NO_PAD.encode("[]"),
        encode(json!({"t": "not-a-time", "m": "x", "a": "y"})),
        encode(json!({"t": "2026-10-01T00:00:00Z", "m": "", "a": "y"})),
        encode(json!({"t": "2026-10-01T00:00:00Z", "m": "x"})),
        encode(json!({"t": "2026-10-01T00:00:00Z", "m": "x", "a": 7})),
        "a".repeat(MAX_CURSOR_BYTES + 1),
    ] {
        let status = decode_artifact_cursor(&raw).err().expect("not a cursor");
        assert_eq!(status.code(), tonic::Code::InvalidArgument, "{raw}");
    }
}

#[test]
fn artifact_limit_defaults_and_clamps() {
    assert_eq!(clamp_artifact_limit(0), i64::from(DEFAULT_ARTIFACT_LIMIT));
    assert_eq!(clamp_artifact_limit(1), 1);
    assert_eq!(clamp_artifact_limit(75), 75);
    assert_eq!(clamp_artifact_limit(10_000), i64::from(MAX_ARTIFACT_LIMIT));
}

#[test]
fn artifact_query_reads_only_the_indexed_predicate_and_never_content() {
    // The literal predicate of the partial index, so the planner can use it.
    assert!(LIST_SPACE_ARTIFACTS_SQL.contains("jsonb_exists(m.metadata, 'artifacts')"));
    assert!(LIST_SPACE_ARTIFACTS_SQL.contains("m.role = 'assistant'"));
    assert!(LIST_SPACE_ARTIFACTS_SQL.contains("t.archived_at IS NULL"));
    // The audience ceiling applies to the thread and the message row.
    assert!(LIST_SPACE_ARTIFACTS_SQL.contains("t.recipient_audience_revision <= $3"));
    assert!(LIST_SPACE_ARTIFACTS_SQL.contains("m.recipient_audience_revision <= $3"));
    // Content is measured, never selected.
    assert!(LIST_SPACE_ARTIFACTS_SQL.contains("char_length(COALESCE(a.elem->>'content', ''))"));
    assert!(!LIST_SPACE_ARTIFACTS_SQL.contains("m.content"));
}

// ---- refusals before any row is read ----------------------------------------------------

#[tokio::test]
async fn an_absent_decision_is_permission_denied_not_an_owner_fallback() {
    let status = call(lazy_pool(), member(), list_request("", ""))
        .await
        .expect_err("a decision is required");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
}

#[tokio::test]
async fn a_half_supplied_decision_is_a_caller_error() {
    for (token, decision_ref) in [("v2.k.p.s", ""), ("", DECISION_REF)] {
        let status = call(lazy_pool(), member(), list_request(token, decision_ref))
            .await
            .expect_err("half a decision");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }
}

#[tokio::test]
async fn a_service_identity_and_a_foreign_org_are_refused() {
    let _keys = StreamKeysEnv::publish();
    let token = read_token("org-1", "user-1", "space-room", 4, false, |_| {});
    let service = VerifiedIdentity::service_for_test_as(
        "org-1",
        "service:model-gateway",
        &["session:read"],
        false,
    );
    let status = call(lazy_pool(), service, list_request(&token, DECISION_REF))
        .await
        .expect_err("a service has no user id to bind the decision to");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);

    let foreign = VerifiedIdentity::user_for_test("org-2", "user-1");
    let status = call(lazy_pool(), foreign, list_request(&token, DECISION_REF))
        .await
        .expect_err("another tenant");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
}

#[tokio::test]
async fn a_decision_for_another_space_org_user_or_window_is_refused() {
    let _keys = StreamKeysEnv::publish();
    type Edit = fn(&mut serde_json::Value);
    let cases: [(&str, String); 5] = [
        (
            "another Space",
            read_token("org-1", "user-1", "space-other", 4, false, |_| {}),
        ),
        (
            "a foreign org",
            read_token("org-9", "user-1", "space-room", 4, false, |_| {}),
        ),
        (
            "a user mismatch",
            read_token("org-1", "user-2", "space-room", 4, false, |_| {}),
        ),
        (
            "an expired token",
            read_token("org-1", "user-1", "space-room", 4, false, (|claims| {
                claims["issued_at"] =
                    json!((Utc::now() - chrono::Duration::minutes(10)).to_rfc3339());
                claims["expires_at"] =
                    json!((Utc::now() - chrono::Duration::minutes(5)).to_rfc3339());
            }) as Edit),
        ),
        (
            "a write permission",
            mint(
                MODEL_PLANE_AUDIENCE,
                &["thread:read", "thread:append"],
                false,
                |claims| {
                    claims["issued_at"] = json!(Utc::now().to_rfc3339());
                    claims["expires_at"] =
                        json!((Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
                },
                false,
            ),
        ),
    ];
    for (why, token) in cases {
        let status = call(lazy_pool(), member(), list_request(&token, DECISION_REF))
            .await
            .expect_err(why);
        assert_eq!(status.code(), tonic::Code::PermissionDenied, "{why}");
    }
    // A stream decision (a different audience) never lists artifacts either.
    let stream = mint(
        crate::grpc::CONTROL_SPACE_STREAM_AUDIENCE,
        &["thread:read"],
        false,
        |claims| {
            claims["issued_at"] = json!(Utc::now().to_rfc3339());
            claims["expires_at"] = json!((Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
        },
        false,
    );
    let status = call(lazy_pool(), member(), list_request(&stream, DECISION_REF))
        .await
        .expect_err("a stream decision is the wrong audience");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
}

#[tokio::test]
async fn a_blank_space_and_a_bad_cursor_are_caller_errors() {
    let _keys = StreamKeysEnv::publish();
    let token = read_token("org-1", "user-1", "space-room", 4, false, |_| {});
    let mut blank = list_request(&token, DECISION_REF);
    blank.space_id = "   ".to_owned();
    let status = call(lazy_pool(), member(), blank).await.expect_err("blank space");
    assert_eq!(status.code(), tonic::Code::InvalidArgument);

    let mut bad = list_request(&token, DECISION_REF);
    bad.cursor = "not-a-cursor".to_owned();
    let status = call(lazy_pool(), member(), bad).await.expect_err("bad cursor");
    assert_eq!(status.code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn a_zdr_caller_gets_an_empty_page_and_no_row_is_read() {
    let _keys = StreamKeysEnv::publish();
    let token = read_token("org-1", "user-1", "space-room", 4, false, |_| {});
    let pool = lazy_pool();
    let page = call(
        pool.clone(),
        VerifiedIdentity::user_for_test_with_zdr("org-1", "user-1", true),
        list_request(&token, DECISION_REF),
    )
    .await
    .expect("a ZDR caller is answered");
    assert!(page.artifacts.is_empty());
    assert_eq!(page.next_cursor, "");
    assert_eq!(pool.size(), 0, "the pool never connected: no row was read");
}

// ---- real Postgres ----------------------------------------------------------------------

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::space::test_support::{clean_space, connect, seed_space_thread};
    use sqlx::Executor as _;

    const NEEDS_DB: &str = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)";

    fn ids(prefix: &str) -> (String, String) {
        let suffix = mp_ids::new_ulid();
        (format!("org-{prefix}-{suffix}"), format!("space-{prefix}-{suffix}"))
    }

    /// An assistant message with `metadata`, `age_secs` old, at audience `revision`.
    async fn seed(
        pool: &PgPool,
        space: &str,
        thread: &str,
        message: &str,
        revision: i64,
        age_secs: i64,
        metadata: serde_json::Value,
    ) {
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
             recipient_audience_revision, recipient_audience_hash, authority_revision, \
             resource_authorization_ref, agent_name, metadata, created_at) \
             VALUES ($1, $2, 'assistant', 'canary body', $3, 'audience-1', $4, 'sha256:audience-1', 1, \
             'resource-1', 'Researcher', $5, now() - make_interval(secs => $6))",
        )
        .bind(message)
        .bind(thread)
        .bind(space)
        .bind(revision)
        .bind(metadata)
        .bind(age_secs as f64)
        .execute(pool)
        .await
        .expect("seed artifact message");
    }

    fn artifact(id: &str, kind: &str, version: i64, content: &str) -> serde_json::Value {
        json!({"id": id, "kind": kind, "title": format!("Title {id}"), "version": version, "content": content})
    }

    async fn list(
        pool: &PgPool,
        org: &str,
        space: &str,
        ceiling: i64,
        limit: u32,
        cursor: &str,
    ) -> pb::ListSpaceArtifactsResponse {
        let token = read_token(org, "user-1", space, ceiling, false, |_| {});
        call(
            pool.clone(),
            VerifiedIdentity::user_for_test(org, "user-1"),
            pb::ListSpaceArtifactsRequest {
                org_id: org.to_owned(),
                space_id: space.to_owned(),
                limit,
                cursor: cursor.to_owned(),
                space_read_decision_ref: DECISION_REF.to_owned(),
                space_read_decision_token: token,
            },
        )
        .await
        .expect("the list answers")
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_room_read_latest_version_wins_and_counts_versions() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space) = ids("latest");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        seed(&pool, &space, &thread, &format!("m1-{space}"), 1, 300, json!({"artifacts": [artifact("a1", "document", 1, "first")]})).await;
        seed(&pool, &space, &thread, &format!("m2-{space}"), 1, 200, json!({"artifacts": [artifact("a1", "document", 2, "second draft")]})).await;
        let page = list(&pool, &org, &space, 4, 0, "").await;
        assert_eq!(page.artifacts.len(), 1, "{NEEDS_DB}");
        let summary = &page.artifacts[0];
        assert_eq!((summary.version, summary.version_count), (2, 2));
        assert_eq!(summary.size_chars, "second draft".chars().count() as i64);
        assert_eq!((summary.kind.as_str(), summary.agent_name.as_str()), ("document", "Researcher"));
        assert_eq!(summary.message_id, format!("m2-{space}"));
        assert!(!format!("{summary:?}").contains("second draft"), "content is never returned");
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_room_read_excludes_other_kinds_and_malformed_elements() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space) = ids("kinds");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        seed(
            &pool,
            &space,
            &thread,
            &format!("m1-{space}"),
            1,
            100,
            json!({"artifacts": [
                artifact("doc", "document", 1, "d"),
                artifact("code", "code", 1, "c"),
                artifact("html", "html", 1, "h"),
                artifact("img", "image", 1, "i"),
                artifact("svg", "svg", 1, "s"),
                {"kind": "document", "title": "no id"},
                "not an object",
                7
            ]}),
        )
        .await;
        seed(&pool, &space, &thread, &format!("m2-{space}"), 1, 90, json!({"artifacts": "not an array"})).await;
        let page = list(&pool, &org, &space, 4, 0, "").await;
        let mut kinds: Vec<&str> = page.artifacts.iter().map(|a| a.kind.as_str()).collect();
        kinds.sort_unstable();
        assert_eq!(kinds, ["code", "document", "html"], "{NEEDS_DB}");
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_room_read_hides_archived_threads() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space) = ids("archived");
        let (live, gone) = (format!("live-{space}"), format!("gone-{space}"));
        seed_space_thread(&pool, &org, &space, &live, "owner-1").await;
        seed_space_thread(&pool, &org, &space, &gone, "owner-1").await;
        seed(&pool, &space, &live, &format!("m1-{space}"), 1, 100, json!({"artifacts": [artifact("kept", "document", 1, "k")]})).await;
        seed(&pool, &space, &gone, &format!("m2-{space}"), 1, 90, json!({"artifacts": [artifact("hidden", "document", 1, "h")]})).await;
        sqlx::query("UPDATE threads SET archived_at = now() WHERE id = $1")
            .bind(&gone)
            .execute(&pool)
            .await
            .expect("archive");
        let page = list(&pool, &org, &space, 4, 0, "").await;
        assert_eq!(page.artifacts.iter().map(|a| a.artifact_id.as_str()).collect::<Vec<_>>(), ["kept"], "{NEEDS_DB}");
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_room_read_newer_audience_message_is_hidden_with_no_count_oracle() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space) = ids("audience");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        seed(&pool, &space, &thread, &format!("m1-{space}"), 1, 200, json!({"artifacts": [artifact("a1", "document", 1, "v1")]})).await;
        seed(&pool, &space, &thread, &format!("m2-{space}"), 2, 100, json!({"artifacts": [artifact("a1", "document", 2, "v2 newer audience"), artifact("a2", "code", 1, "only newer")]})).await;
        let old = list(&pool, &org, &space, 1, 0, "").await;
        assert_eq!(old.artifacts.len(), 1, "{NEEDS_DB}");
        assert_eq!(
            (old.artifacts[0].version, old.artifacts[0].version_count),
            (1, 1),
            "the newer-audience message changes neither the version nor the count"
        );
        let current = list(&pool, &org, &space, 2, 0, "").await;
        assert_eq!(current.artifacts.len(), 2);
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_room_read_cursor_is_stable_across_an_insert_and_two_artifacts_in_one_message() {
        let Some(pool) = connect().await else { return };
        let _keys = StreamKeysEnv::publish();
        let (org, space) = ids("cursor");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        seed(&pool, &space, &thread, &format!("m1-{space}"), 1, 100, json!({"artifacts": [artifact("a1", "document", 1, "x"), artifact("a2", "code", 1, "y")]})).await;
        seed(&pool, &space, &thread, &format!("m0-{space}"), 1, 200, json!({"artifacts": [artifact("a0", "html", 1, "z")]})).await;
        let first = list(&pool, &org, &space, 4, 1, "").await;
        assert_eq!(first.artifacts.len(), 1, "{NEEDS_DB}");
        assert!(!first.next_cursor.is_empty());
        // A newer message lands between the pages: it sorts before the cursor and is not
        // delivered twice, and no artifact of the original set is skipped.
        seed(&pool, &space, &thread, &format!("m9-{space}"), 1, 1, json!({"artifacts": [artifact("a9", "document", 1, "new")]})).await;
        let mut seen = vec![first.artifacts[0].artifact_id.clone()];
        let mut cursor = first.next_cursor;
        while !cursor.is_empty() {
            let page = list(&pool, &org, &space, 4, 1, &cursor).await;
            seen.extend(page.artifacts.iter().map(|a| a.artifact_id.clone()));
            cursor = page.next_cursor;
        }
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), seen.len(), "no artifact twice: {seen:?}");
        for original in ["a0", "a1", "a2"] {
            assert!(seen.iter().any(|id| id == original), "{original} missing from {seen:?}");
        }
        assert!(!seen.iter().any(|id| id == "a9"), "the newer message sorts before the cursor");
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_room_read_explain_uses_idx_messages_space_artifacts() {
        let Some(pool) = connect().await else { return };
        let (_, space) = ids("explain");
        let mut tx = pool.begin().await.expect("begin");
        tx.execute("SET LOCAL enable_seqscan = off").await.expect("planner switch");
        let plan: Vec<String> = sqlx::query_scalar(&format!(
            "EXPLAIN SELECT id FROM messages WHERE space_id = '{space}' AND role = 'assistant' \
             AND jsonb_exists(metadata, 'artifacts') ORDER BY created_at DESC, id DESC"
        ))
        .fetch_all(&mut *tx)
        .await
        .expect("explain");
        tx.rollback().await.ok();
        assert!(
            plan.join("\n").contains("idx_messages_space_artifacts"),
            "the artifact index is not used, {NEEDS_DB}: {plan:#?}"
        );
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn migration_0085_applies_without_a_transaction_and_the_revert_drops_it() {
        let Some(pool) = connect().await else { return };
        let valid = |pool: PgPool| async move {
            sqlx::query_scalar::<_, bool>(
                "SELECT COALESCE((SELECT i.indisvalid FROM pg_index i \
                   JOIN pg_class c ON c.oid = i.indexrelid \
                  WHERE c.relname = 'idx_messages_space_artifacts'), false)",
            )
            .fetch_one(&pool)
            .await
            .expect("index probe")
        };
        assert!(valid(pool.clone()).await, "the migration built a valid index, {NEEDS_DB}");

        let up = include_str!("../../migrations/0085_messages_space_artifact_index.sql");
        let down = include_str!("../../migrations/revert/0085_messages_space_artifact_index.down.sql");
        assert!(up.starts_with("-- no-transaction") && down.starts_with("-- no-transaction"));
        pool.execute(down).await.expect("the revert runs outside a transaction");
        assert!(!valid(pool.clone()).await, "the revert dropped the index");
        // Put it back, and prove the up file is idempotent (IF NOT EXISTS).
        pool.execute(up).await.expect("re-apply");
        pool.execute(up).await.expect("re-apply again");
        assert!(valid(pool.clone()).await);
    }
}
