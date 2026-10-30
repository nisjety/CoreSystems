//! The tenant lease can be renewed by its holder, and frees itself when nobody renews it.
//!
//! Requires the local compose Postgres and Redis (`make up`); the Redis used is
//! `VSV2_TEST_REDIS_URL` (default the compose port). Each test uses a tenant id of its own.

use std::time::Duration;

use uuid::Uuid;
use vsv2_common::TenantId;
use vsv2_state::StateStore;

async fn store() -> StateStore {
    let pg = std::env::var("VSV2_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://visma_v2:visma_v2@127.0.0.1:55433/postgres".to_string());
    let redis = std::env::var("VSV2_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:56380/0".to_string());
    StateStore::connect(&pg, &redis)
        .await
        .unwrap_or_else(|e| panic!("cannot reach the test Postgres/Redis (`make up`): {e}"))
}

fn tenant() -> TenantId {
    TenantId::new(format!("lease-test-{}", Uuid::new_v4().simple()))
}

#[tokio::test]
async fn the_holder_can_renew_and_another_owner_cannot() {
    let state = store().await;
    let tenant = tenant();
    assert!(state
        .try_acquire_lease(&tenant, "a", 2)
        .await
        .expect("acquire"));

    assert!(!state
        .try_acquire_lease(&tenant, "b", 2)
        .await
        .expect("second"));
    assert!(!state
        .renew_lease(&tenant, "b", 60)
        .await
        .expect("renew by b"));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(state
        .renew_lease(&tenant, "a", 2)
        .await
        .expect("renew by a"));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        !state
            .try_acquire_lease(&tenant, "b", 2)
            .await
            .expect("still held"),
        "renewed, so 3 s later it is still a's"
    );
    state.release_lease(&tenant).await.expect("release");
}

#[tokio::test]
async fn a_lease_nobody_renews_frees_itself() {
    let state = store().await;
    let tenant = tenant();
    assert!(state
        .try_acquire_lease(&tenant, "dead", 1)
        .await
        .expect("acquire"));

    tokio::time::sleep(Duration::from_millis(1500)).await;

    assert!(
        state
            .try_acquire_lease(&tenant, "next", 60)
            .await
            .expect("take over"),
        "the holder stopped renewing; the next one takes over"
    );
    assert!(!state
        .renew_lease(&tenant, "dead", 60)
        .await
        .expect("late renew"));
    state.release_lease(&tenant).await.expect("release");
}
