//! F-f10-22: an `api_key` caller starts a managed run on its own thread, streams that run's events,
//! and is refused everywhere it would reach beyond its own rows.
//!
//! The refusals run over the handlers directly and need no database: each would fail on the first
//! query otherwise, so a pool that never answers proves the refusal came before any read. The start
//! on a key's own thread, the Space refusal and the stream on a key's own run are scratch-database
//! tests (`DATABASE_URL`, as the neighbouring tests are).

use super::*;
use crate::auth::VerifiedIdentity;
use crate::loop_checkpoint::test_support::scratch_pool;
use mp_contracts::model_plane::v1::orchestration_core_service_server::OrchestrationCoreService as _;
use sqlx::PgPool;
use std::time::Duration;

/// A managed-run service whose database never answers: a call that reaches it waits out the pool's
/// acquire timeout and fails. Keep the listener alive: it holds that socket.
fn silent_database_service() -> (std::net::TcpListener, ManagedRunLifecycleService) {
    let silent_database = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(250))
        .connect_lazy(&format!(
            "postgresql://session-core@{}/session_core",
            silent_database.local_addr().expect("address")
        ))
        .expect("lazy pool");
    (
        silent_database,
        ManagedRunLifecycleService {
            pool,
            letta_memory: None,
        },
    )
}

fn start_message(org: &str, thread: &str, start_key: &str) -> pb::StartManagedRunRequest {
    pb::StartManagedRunRequest {
        thread_id: thread.to_owned(),
        agent_id: "general-v1".to_owned(),
        goal: "answer the post".to_owned(),
        mode: "execute".to_owned(),
        org_id: org.to_owned(),
        start_key: start_key.to_owned(),
        terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
        ..Default::default()
    }
}

/// The verified identity the gRPC interceptor would have installed on `message`.
fn with_identity<T>(message: T, identity: VerifiedIdentity) -> Request<T> {
    let mut request = Request::new(message);
    request.extensions_mut().insert(identity);
    request
}

/// A personal thread of `owner` in `org`.
async fn seed_thread(pool: &PgPool, org: &str, thread: &str, owner: &str) {
    sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)")
        .bind(thread)
        .bind(org)
        .bind(owner)
        .execute(pool)
        .await
        .expect("thread seeded");
}

/// A thread of `owner` in a Space, with the Space columns a Space thread carries (migrations 0023,
/// 0024 and 0034).
async fn seed_space_thread(pool: &PgPool, org: &str, thread: &str, owner: &str) {
    sqlx::query(
        "INSERT INTO threads (id, session_key, org_id, user_id, origin, space_id,
                              space_decision_ref, recipient_audience_ref, recipient_audience_revision,
                              recipient_audience_hash, privacy_policy_ref, resource_authorization_ref,
                              authority_revision)
         VALUES ($1, $1, $2, $3, 'space', 'space-1', 'dec-1', 'aud-1', 1, 'sha256:aud-1',
                 'priv-1', 'res-1', 1)",
    )
    .bind(thread)
    .bind(org)
    .bind(owner)
    .execute(pool)
    .await
    .expect("space thread seeded");
}

/// A running run of `owner` on `thread`.
async fn seed_run(pool: &PgPool, org: &str, thread: &str, run: &str, owner: &str) {
    sqlx::query(
        "INSERT INTO runs (id, thread_id, goal, org_id, user_id, retention_class, status)
         VALUES ($1, $2, 'g', $3, $4, 'persistent', 'running')",
    )
    .bind(run)
    .bind(thread)
    .bind(org)
    .bind(owner)
    .execute(pool)
    .await
    .expect("run seeded");
}

async fn runs_on(pool: &PgPool, thread: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM runs WHERE thread_id = $1")
        .bind(thread)
        .fetch_one(pool)
        .await
        .expect("count runs")
}

#[tokio::test]
async fn an_api_key_may_not_start_a_wake() {
    let (_silent, service) = silent_database_service();
    let key = VerifiedIdentity::api_key_for_test("org-1", "key-1", "user-1", false);
    let mut message = start_message("org-1", "thread-1", "wake-key-1");
    message.wake = Some(pb::RunWakeContext::default());
    let status = service
        .start_managed_run(with_identity(message, key))
        .await
        .expect_err("a key may not wake");
    assert_eq!(status.code(), tonic::Code::PermissionDenied, "{status:?}");
    assert!(status.message().contains("API key"), "{status:?}");
}

/// A service has no owner, so it starts no managed run, whatever its scopes.
#[tokio::test]
async fn a_service_has_no_owner_and_starts_no_managed_run() {
    let (_silent, service) = silent_database_service();
    let caller = VerifiedIdentity::service_for_test("org-1", &["session:write"], false);
    let status = service
        .start_managed_run(with_identity(
            start_message("org-1", "thread-1", "service-start-1"),
            caller,
        ))
        .await
        .expect_err("a service owns no run");
    assert_eq!(status.code(), tonic::Code::PermissionDenied, "{status:?}");
    assert!(
        status
            .message()
            .contains("user-bound managed-run credential required"),
        "{status:?}"
    );
}

/// A key starts a managed run on its own thread. The run is the key's, stamped `api` with the key and
/// its owner, and another key's thread is refused with nothing written to it.
#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn an_api_key_starts_a_managed_run_on_its_own_thread_only() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let org = format!("akr-org-{}", new_ulid());
    let own = format!("akr-own-{}", new_ulid());
    let other = format!("akr-other-{}", new_ulid());
    seed_thread(&pool, &org, &own, "apikey:key-1").await;
    seed_thread(&pool, &org, &other, "apikey:key-2").await;
    let service = ManagedRunLifecycleService {
        pool: pool.clone(),
        letta_memory: None,
    };
    let key = VerifiedIdentity::api_key_for_test(&org, "key-1", "user-1", false);

    let started = service
        .start_managed_run(with_identity(
            start_message(&org, &own, "key-start-own"),
            key.clone(),
        ))
        .await
        .expect("a key starts a run on its own thread")
        .into_inner();
    let (owner, metadata): (String, serde_json::Value) =
        sqlx::query_as("SELECT user_id, metadata FROM runs WHERE id = $1")
            .bind(&started.run_id)
            .fetch_one(&pool)
            .await
            .expect("the run row");
    assert_eq!(owner, "apikey:key-1", "the key owns its run");
    assert_eq!(metadata["origin"], "api");
    assert_eq!(metadata["api_key_id"], "key-1");
    assert_eq!(
        metadata["api_key_owner"], "user-1",
        "the person is attribution only"
    );

    let status = service
        .start_managed_run(with_identity(
            start_message(&org, &other, "key-start-other"),
            key,
        ))
        .await
        .expect_err("another key's thread is refused");
    assert_eq!(status.code(), tonic::Code::PermissionDenied, "{status:?}");
    assert_eq!(
        runs_on(&pool, &other).await,
        0,
        "nothing lands on another key's thread"
    );
}

/// A Space refuses a key's run: the thread's Space is read on the insert, and the refusal leaves no
/// run behind.
#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn an_api_key_may_not_start_a_run_in_a_space() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let org = format!("akr-org-{}", new_ulid());
    let room = format!("akr-room-{}", new_ulid());
    seed_space_thread(&pool, &org, &room, "apikey:key-1").await;
    let service = ManagedRunLifecycleService {
        pool: pool.clone(),
        letta_memory: None,
    };
    let key = VerifiedIdentity::api_key_for_test(&org, "key-1", "user-1", false);
    let status = service
        .start_managed_run(with_identity(
            start_message(&org, &room, "key-start-space"),
            key,
        ))
        .await
        .expect_err("a key may not start a run in a Space");
    assert_eq!(status.code(), tonic::Code::PermissionDenied, "{status:?}");
    assert!(status.message().contains("Space"), "{status:?}");
    assert_eq!(runs_on(&pool, &room).await, 0, "the refusal leaves no run");
}

/// A key streams the events of its own run and is refused on another key's run.
#[tokio::test]
#[ignore = "requires DATABASE_URL to a scratch Postgres"]
async fn an_api_key_streams_only_its_own_runs_events() {
    let Some(pool) = scratch_pool().await else {
        return;
    };
    let org = format!("akr-org-{}", new_ulid());
    let own_thread = format!("akr-own-{}", new_ulid());
    let other_thread = format!("akr-other-{}", new_ulid());
    let own_run = format!("akr-run-own-{}", new_ulid());
    let other_run = format!("akr-run-other-{}", new_ulid());
    seed_thread(&pool, &org, &own_thread, "apikey:key-1").await;
    seed_thread(&pool, &org, &other_thread, "apikey:key-2").await;
    seed_run(&pool, &org, &own_thread, &own_run, "apikey:key-1").await;
    seed_run(&pool, &org, &other_thread, &other_run, "apikey:key-2").await;
    let (events_tx, _events_rx) = tokio::sync::broadcast::channel(8);
    let service = crate::orchestration_grpc::OrchestrationGrpc::new(pool.clone(), events_tx);
    let key = VerifiedIdentity::api_key_for_test(&org, "key-1", "user-1", false);

    assert!(
        service
            .stream_run_events(with_identity(
                pb::StreamRunEventsRequest {
                    run_id: own_run,
                    after_event_id: String::new(),
                },
                key.clone(),
            ))
            .await
            .is_ok(),
        "a key streams its own run"
    );
    let status = service
        .stream_run_events(with_identity(
            pb::StreamRunEventsRequest {
                run_id: other_run,
                after_event_id: String::new(),
            },
            key,
        ))
        .await
        .err()
        .expect("another key's run is refused");
    assert_eq!(status.code(), tonic::Code::PermissionDenied, "{status:?}");
}
