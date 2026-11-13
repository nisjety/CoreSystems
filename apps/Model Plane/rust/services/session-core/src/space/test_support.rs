//! Shared helpers for the Space DB-tier tests (test-only). Every test that uses them is
//! `#[ignore]`d and needs `DATABASE_URL` to a disposable Postgres whose database name contains
//! `test`: the helpers refuse to touch any other database.

use sqlx::PgPool;

/// Connects to the disposable test database and applies the session-core migrations. `None`
/// (with a note on stderr) when `DATABASE_URL` is unset or does not name a `*test*` database.
pub(crate) async fn connect() -> Option<PgPool> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL unset");
        return None;
    };
    let database = url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !database.contains("test") {
        eprintln!("skipping: the database name must contain \"test\"");
        return None;
    }
    let pool = PgPool::connect(&url).await.expect("connect pg");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");
    Some(pool)
}

/// A Space thread with the full authority context the schema requires.
pub(crate) async fn seed_space_thread(pool: &PgPool, org: &str, space: &str, thread_id: &str, owner: &str) {
    sqlx::query(
        "INSERT INTO threads (id, session_key, org_id, user_id, origin, space_id, space_decision_ref, \
         recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, \
         privacy_policy_ref, resource_authorization_ref, authority_revision) \
         VALUES ($1, $1, $2, $3, 'space', $4, 'decision-1', 'audience-1', 1, 'sha256:audience-1', \
         'privacy-1', 'resource-1', 1)",
    )
    .bind(thread_id)
    .bind(org)
    .bind(owner)
    .bind(space)
    .execute(pool)
    .await
    .expect("seed space thread");
}

/// One Space message row (author optional), with the audience context the schema requires.
pub(crate) async fn seed_space_message(
    pool: &PgPool,
    space: &str,
    thread_id: &str,
    message_id: &str,
    role: &str,
    author: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
         recipient_audience_revision, recipient_audience_hash, authority_revision, \
         resource_authorization_ref, author_subject_id) \
         VALUES ($1, $2, $3, 'canary body', $4, 'audience-1', 1, 'sha256:audience-1', 1, 'resource-1', $5)",
    )
    .bind(message_id)
    .bind(thread_id)
    .bind(role)
    .bind(space)
    .bind(author)
    .execute(pool)
    .await
    .expect("seed space message");
}

/// A Space run in `status` under `retention_class` (`persistent`, `zdr` or `zdr_waived`). A waived
/// run names its acknowledgement and connection, and no other class does, as production writes its
/// receipt (`runs_retention_waiver_consistent`, migration 0044).
pub(crate) async fn seed_space_run(
    pool: &PgPool,
    org: &str,
    space: &str,
    thread_id: &str,
    run_id: &str,
    status: &str,
    retention_class: &str,
) {
    let (waiver_ack_id, waiver_connection_id) = match retention_class {
        "zdr_waived" => (Some("ack-1"), Some("conn-1")),
        _ => (None, None),
    };
    sqlx::query(
        "INSERT INTO runs (id, thread_id, goal, status, org_id, user_id, space_id, space_decision_ref, \
         recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, \
         privacy_policy_ref, resource_authorization_ref, authority_revision, retention_class, \
         waiver_ack_id, waiver_connection_id) \
         VALUES ($1, $2, 'goal', $3, $4, 'owner-1', $5, 'decision-1', 'audience-1', 1, \
         'sha256:audience-1', 'privacy-1', 'resource-1', 1, $6, $7, $8)",
    )
    .bind(run_id)
    .bind(thread_id)
    .bind(status)
    .bind(org)
    .bind(space)
    .bind(retention_class)
    .bind(waiver_ack_id)
    .bind(waiver_connection_id)
    .execute(pool)
    .await
    .expect("seed space run");
}

/// Turns the event log on for the test database (the settings row is a singleton).
pub(crate) async fn set_event_log(pool: &PgPool, enabled: bool) {
    sqlx::query("UPDATE space_event_settings SET enabled = $1 WHERE singleton")
        .bind(enabled)
        .execute(pool)
        .await
        .expect("set space_event_settings");
}

/// Removes everything a test created for one Space, bypassing the event triggers.
pub(crate) async fn clean_space(pool: &PgPool, org: &str, space: &str) {
    let mut tx = pool.begin().await.expect("begin cleanup");
    sqlx::query("SELECT set_config('space_event.suppress', 'on', true)")
        .execute(&mut *tx)
        .await
        .ok();
    for sql in [
        "DELETE FROM messages WHERE thread_id IN (SELECT id FROM threads WHERE org_id = $1 AND space_id = $2)",
        "DELETE FROM runs WHERE org_id = $1 AND space_id = $2",
        "DELETE FROM threads WHERE org_id = $1 AND space_id = $2",
        "DELETE FROM space_event_log WHERE org_id = $1 AND space_ref = $2",
        "DELETE FROM space_event_heads WHERE org_id = $1 AND space_ref = $2",
    ] {
        sqlx::query(sql).bind(org).bind(space).execute(&mut *tx).await.ok();
    }
    tx.commit().await.ok();
}
