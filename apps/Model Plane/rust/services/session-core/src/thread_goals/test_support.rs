//! Seeds the goal judge's DB-tier tests share (`judge_read_tests`, `verdict_tests`), a test-only
//! child module of `thread_goals`.

use crate::retention_receipt::RunRetention;
use mp_ids::new_ulid;
use sqlx::PgPool;

/// The two receipts a held thread can carry: zero retention, and a waiver on one connection.
pub(crate) fn held_receipts() -> [RunRetention; 2] {
    [RunRetention::zdr(), RunRetention::waived("ack-1", "conn-1")]
}

/// Insert a completed run on `thread`, for `org` and `user`, that carries `retention` as its
/// receipt. The columns are written the way production writes a receipt: the class, and the
/// acknowledgement and connection of a waived run only (`runs_retention_waiver_consistent`,
/// migration 0044).
pub(crate) async fn insert_completed_run(
    pool: &PgPool,
    org: &str,
    user: &str,
    thread: &str,
    retention: &RunRetention,
) {
    sqlx::query(
        "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, retention_class,
                           waiver_ack_id, waiver_connection_id)
         VALUES ($1, $2, 'g', $3, $4, 'completed', $5, $6, $7)",
    )
    .bind(format!("test-run-{}", new_ulid()))
    .bind(thread)
    .bind(org)
    .bind(user)
    .bind(retention.class.as_str())
    .bind(retention.ack_id())
    .bind(retention.connection_id())
    .execute(pool)
    .await
    .unwrap();
}
