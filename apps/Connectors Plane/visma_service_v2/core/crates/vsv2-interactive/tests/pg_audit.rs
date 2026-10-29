//! Needs `VSV2_TEST_DATABASE_URL` to point at a *scratch* database (it creates the `vsv2` schema and applies
//! migration 0012). `#[ignore]`d, so a plain `cargo test` stays hermetic and does not claim to have run it:
//! `VSV2_TEST_DATABASE_URL=... cargo test -p vsv2-interactive --test pg_audit -- --ignored`.

use chrono::Utc;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;
use vsv2_interactive::{AuditEntry, AuditSink, AuditStage, PgAuditSink};

fn entry(plan_id: Uuid, stage: AuditStage) -> AuditEntry {
    AuditEntry {
        audit_id: Uuid::new_v4(),
        plan_id: Some(plan_id),
        tenant_id: "t-test".into(),
        operation: "test.update".into(),
        caller: "pg-test".into(),
        stage,
        target: Some("salesorder".into()),
        method: Some("PUT".into()),
        path: Some("/v3/salesorder/SO1".into()),
        payload_hash: Some("abc".into()),
        payload: Some(json!({"qty": 3})),
        http_status: Some(200),
        detail: Some("ok".into()),
        created_at: Utc::now(),
    }
}

#[ignore = "needs a scratch Postgres: set VSV2_TEST_DATABASE_URL and run with --ignored"]
#[tokio::test]
async fn pg_sink_round_trips_and_the_table_is_append_only() {
    let url = std::env::var("VSV2_TEST_DATABASE_URL")
        .expect("VSV2_TEST_DATABASE_URL must point at a scratch Postgres for this test");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE SCHEMA IF NOT EXISTS vsv2")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../../../../migrations/0012_write_audit.sql"))
        .execute(&pool)
        .await
        .unwrap();
    // Migration must be re-runnable.
    sqlx::raw_sql(include_str!("../../../../migrations/0012_write_audit.sql"))
        .execute(&pool)
        .await
        .unwrap();

    let sink = PgAuditSink::new(pool.clone());
    let plan = Uuid::new_v4();

    assert!(!sink.was_attempted(plan).await.unwrap());
    sink.record(&entry(plan, AuditStage::DryRun)).await.unwrap();
    assert!(
        !sink.was_attempted(plan).await.unwrap(),
        "a dry run is not an attempt"
    );
    sink.record(&entry(plan, AuditStage::Executing))
        .await
        .unwrap();
    assert!(sink.was_attempted(plan).await.unwrap());
    sink.record(&entry(plan, AuditStage::Succeeded))
        .await
        .unwrap();

    // Entry without optional fields (a denial) also stores.
    let mut denied = entry(Uuid::new_v4(), AuditStage::Denied);
    denied.plan_id = None;
    denied.payload = None;
    denied.http_status = None;
    sink.record(&denied).await.unwrap();

    // The execution claim is atomic in the database: a second `executing` row for a plan is refused.
    let dup = sink.record(&entry(plan, AuditStage::Executing)).await;
    assert!(
        matches!(
            dup,
            Err(vsv2_interactive::InteractiveError::AlreadyExecuted(_))
        ),
        "{dup:?}"
    );
    // Outcome-unknown is a valid stage; TRUNCATE is blocked like UPDATE/DELETE.
    sink.record(&entry(Uuid::new_v4(), AuditStage::Unknown))
        .await
        .unwrap();
    assert!(sqlx::query("TRUNCATE vsv2.write_audit")
        .execute(&pool)
        .await
        .is_err());

    // Append-only: UPDATE and DELETE are rejected by the trigger.
    let upd = sqlx::query("UPDATE vsv2.write_audit SET detail = 'tampered' WHERE plan_id = $1")
        .bind(plan)
        .execute(&pool)
        .await;
    assert!(upd.is_err(), "UPDATE must be blocked");
    let del = sqlx::query("DELETE FROM vsv2.write_audit WHERE plan_id = $1")
        .bind(plan)
        .execute(&pool)
        .await;
    assert!(del.is_err(), "DELETE must be blocked");

    // Stage CHECK constraint rejects unknown stages.
    let bad = sqlx::query(
        "INSERT INTO vsv2.write_audit (audit_id, tenant_id, operation, caller, stage) \
         VALUES ($1,'t','o','c','bogus')",
    )
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await;
    assert!(bad.is_err());

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM vsv2.write_audit WHERE plan_id = $1")
        .bind(plan)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 3);
}
