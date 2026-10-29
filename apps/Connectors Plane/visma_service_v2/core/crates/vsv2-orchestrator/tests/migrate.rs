//! The embedded migration runner against a real Postgres.
//!
//! Each test gets its own throwaway database (see `common/test_db.rs`). What they guard: a fresh
//! database gets the whole schema and every file is recorded once, and a database that was migrated
//! by hand (the state of every database before the runner existed) is accepted without losing a row.

mod common;

use chrono::{TimeZone, Utc};
use common::test_db::TestDb;
use vsv2_common::{Source, TenantId};
use vsv2_state::{Checkpoint, CheckpointKey};

async fn recorded(db: &TestDb) -> Vec<i64> {
    sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success ORDER BY version")
        .fetch_all(&db.state.pg)
        .await
        .expect("read _sqlx_migrations")
}

#[tokio::test]
async fn a_fresh_database_gets_every_migration_once() {
    let db = TestDb::create_empty().await;
    let embedded = vsv2_state::embedded_migrations();

    let first = db.state.migrate().await.expect("first run");
    let second = db.state.migrate().await.expect("second run");

    assert_eq!(first, embedded.len());
    assert_eq!(second, 0, "a second run has nothing to apply");
    assert_eq!(
        recorded(&db).await,
        embedded.iter().map(|(v, _)| *v).collect::<Vec<_>>()
    );
    // the schema the scheduler needs is there
    for table in [
        "sync_checkpoint",
        "sync_run",
        "scheduler_pass",
        "write_audit",
    ] {
        let found: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("vsv2.{table}"))
            .fetch_one(&db.state.pg)
            .await
            .expect("to_regclass");
        assert!(found, "vsv2.{table} should exist after migrate");
    }
}

#[tokio::test]
async fn a_database_migrated_by_hand_is_accepted_and_keeps_its_rows() {
    let db = TestDb::create().await;
    let key = CheckpointKey::default_partition(
        TenantId::new("tenant-a".to_string()),
        "accounts".to_string(),
        Source::ErpRest,
    );
    let watermark = Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
    db.state
        .upsert_checkpoint(&Checkpoint {
            key: key.clone(),
            last_modified_watermark: Some(watermark),
            last_updated_in_dm: None,
            last_id: None,
            last_sync_at: Some(watermark),
            records_seen: 42,
        })
        .await
        .expect("seed a checkpoint");

    let applied = db
        .state
        .migrate()
        .await
        .expect("migrate over hand-applied files");
    let again = db.state.migrate().await.expect("second run");

    assert_eq!(applied, vsv2_state::embedded_migrations().len());
    assert_eq!(again, 0);
    let kept = db
        .state
        .get_checkpoint(&key)
        .await
        .expect("read the checkpoint back")
        .expect("the checkpoint is still there");
    assert_eq!(kept.records_seen, 42);
    assert_eq!(kept.last_modified_watermark, Some(watermark));
}

#[tokio::test]
async fn an_edited_migration_that_was_recorded_is_refused() {
    let db = TestDb::create_empty().await;
    db.state.migrate().await.expect("migrate");
    // someone edits migration 1 after it was applied: its recorded checksum no longer matches
    sqlx::query("UPDATE _sqlx_migrations SET checksum = '\\x00'::bytea WHERE version = 1")
        .execute(&db.state.pg)
        .await
        .expect("tamper with the recorded checksum");

    let result = db.state.migrate().await;

    let message = result
        .expect_err("a changed migration must be refused")
        .to_string();
    assert!(message.contains("apply migrations"), "{message}");
}
