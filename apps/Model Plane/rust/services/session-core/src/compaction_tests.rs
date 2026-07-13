use super::*;
use metrics::{
    Counter, CounterFn, Gauge, Histogram, HistogramFn, Key, KeyName, Metadata, Recorder,
    SharedString, Unit,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Debug, PartialEq)]
struct RecordedMetric {
    name: String,
    labels: Vec<(String, String)>,
    value: f64,
}

#[derive(Default)]
struct TestRecorder {
    counters: Arc<Mutex<Vec<RecordedMetric>>>,
    histograms: Arc<Mutex<Vec<RecordedMetric>>>,
}

struct TestCounter {
    key: Key,
    values: Arc<Mutex<Vec<RecordedMetric>>>,
}

impl CounterFn for TestCounter {
    #[allow(clippy::cast_precision_loss)]
    fn increment(&self, value: u64) {
        self.values
            .lock()
            .expect("counter lock")
            .push(metric(&self.key, value as f64));
    }

    fn absolute(&self, value: u64) {
        self.increment(value);
    }
}

struct TestHistogram {
    key: Key,
    values: Arc<Mutex<Vec<RecordedMetric>>>,
}

impl HistogramFn for TestHistogram {
    fn record(&self, value: f64) {
        self.values
            .lock()
            .expect("histogram lock")
            .push(metric(&self.key, value));
    }
}

impl Recorder for TestRecorder {
    fn describe_counter(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

    fn describe_gauge(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

    fn describe_histogram(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

    fn register_counter(&self, key: &Key, _metadata: &Metadata<'_>) -> Counter {
        Counter::from_arc(Arc::new(TestCounter {
            key: key.clone(),
            values: Arc::clone(&self.counters),
        }))
    }

    fn register_gauge(&self, _key: &Key, _metadata: &Metadata<'_>) -> Gauge {
        Gauge::noop()
    }

    fn register_histogram(&self, key: &Key, _metadata: &Metadata<'_>) -> Histogram {
        Histogram::from_arc(Arc::new(TestHistogram {
            key: key.clone(),
            values: Arc::clone(&self.histograms),
        }))
    }
}

fn metric(key: &Key, value: f64) -> RecordedMetric {
    RecordedMetric {
        name: key.name().to_owned(),
        labels: key
            .labels()
            .map(|label| (label.key().to_owned(), label.value().to_owned()))
            .collect(),
        value,
    }
}

#[test]
fn compaction_only_checkpoints_durable_runs() {
    assert!(COMPACT_SQL.contains("JOIN runs r ON r.id = e.run_id"));
}

#[test]
fn compaction_uses_a_deterministic_step_watermark() {
    assert!(COMPACT_SQL.contains("auto-compact-v1:"));
    assert!(COMPACT_SQL.contains("sha256(convert_to"));
    assert!(COMPACT_SQL.contains("c.max_step"));
    assert!(!COMPACT_SQL.contains("e2.ts > lc.created_at"));
}

#[test]
fn compaction_is_conflict_safe_under_concurrent_cycles() {
    assert!(COMPACT_SQL.contains("ON CONFLICT (id) DO UPDATE"));
    assert!(COMPACT_SQL.contains("checkpoints.run_id = EXCLUDED.run_id"));
    assert!(COMPACT_SQL.contains("checkpoints.state = EXCLUDED.state"));
    assert!(!COMPACT_SQL.contains("ON CONFLICT DO NOTHING"));
    let migration = include_str!("../migrations/0002_events_and_ordinals.sql");
    assert!(migration.contains("idx_checkpoints_run_ordinal"));
}

#[test]
fn rollout_is_bounded_and_recognizes_legacy_watermark_state() {
    assert_eq!(MAX_CHECKPOINTS_PER_CYCLE, 100);
    assert!(COMPACT_SQL.contains("LIMIT $1"));
    assert!(COMPACT_SQL.contains("ck.state = i.legacy_state"));
    assert!(COMPACT_SQL.contains("ck.state = i.expected_state"));
}

#[test]
fn long_run_ids_are_hashed_to_a_fixed_width_checkpoint_id() {
    let long_run_id = "r".repeat(1_000_000);
    assert!(long_run_id.len() > MAX_CHECKPOINT_ID_LEN);
    assert_eq!(MAX_CHECKPOINT_ID_LEN, "auto-compact-v1:".len() + 64);
    assert!(COMPACT_SQL.contains("sha256(convert_to"));
    assert!(!COMPACT_SQL.contains("encode(convert_to(c.run_id, 'UTF8'), 'hex')"));
}

#[test]
fn semantic_preclaim_is_isolated_without_rolling_back_other_tenants() {
    let result = validate_compaction_result(2, 1, 1).expect("conflict is isolated");
    assert_eq!(result.inserted, 1);
    assert_eq!(result.semantic_conflicts, 1);

    let clean = validate_compaction_result(2, 2, 1).expect("valid");
    assert_eq!(clean.inserted, 1);
    assert_eq!(clean.semantic_conflicts, 0);
}

#[test]
fn statement_timeout_is_transaction_local() {
    assert!(SET_STATEMENT_TIMEOUT_SQL.contains("set_config"));
    assert!(SET_STATEMENT_TIMEOUT_SQL.contains("true"));
    assert_eq!(STATEMENT_TIMEOUT, Duration::from_secs(5));
}

#[test]
fn poison_ordinals_and_zdr_runs_are_excluded() {
    assert!(COMPACT_SQL.contains("e.step_ordinal > 0"));
    assert!(COMPACT_SQL.contains("r.metadata"));
    assert!(COMPACT_SQL.contains(r#"'{"zdr": true}'::jsonb"#));
}

#[test]
fn synthesized_checkpoint_never_copies_event_or_run_content() {
    assert!(!COMPACT_SQL.contains("e.payload"));
    assert!(!COMPACT_SQL.contains("r.goal"));
    assert!(COMPACT_SQL.contains("jsonb_build_object('max_step', c.max_step)"));
}

#[test]
fn retry_backoff_is_bounded_and_exponential() {
    let policy = RetryPolicy {
        max_attempts: 5,
        base_delay: Duration::from_millis(25),
        max_delay: Duration::from_millis(70),
    };
    assert_eq!(policy.delay_after(1, 0), Duration::from_millis(25));
    assert_eq!(policy.delay_after(2, 0), Duration::from_millis(50));
    let jittered = policy.delay_after(1, u64::MAX);
    assert!((Duration::from_millis(25)..=Duration::from_millis(30)).contains(&jittered));
    assert_eq!(policy.delay_after(3, u64::MAX), Duration::from_millis(70));
    assert_eq!(policy.delay_after(4, u64::MAX), Duration::from_millis(70));
}

#[test]
fn postgres_class_08_and_targeted_ordinal_conflicts_are_retryable() {
    assert_eq!(postgres_retry_reason("08006", None), Some("connection"));
    assert_eq!(postgres_retry_reason("08003", None), Some("connection"));
    assert_eq!(
        postgres_retry_reason("23505", Some("idx_checkpoints_run_ordinal")),
        Some("ordinal_contention")
    );
    assert_eq!(
        postgres_retry_reason("23505", Some("checkpoints_pkey")),
        None
    );
    assert_eq!(postgres_retry_reason("22021", None), None);
}

#[test]
fn invalid_or_zero_interval_fails_safe_to_default() {
    assert_eq!(parse_interval_secs(None), DEFAULT_INTERVAL_SECS);
    assert_eq!(
        parse_interval_secs(Some("not-a-number")),
        DEFAULT_INTERVAL_SECS
    );
    assert_eq!(parse_interval_secs(Some("0")), DEFAULT_INTERVAL_SECS);
    assert_eq!(parse_interval_secs(Some("86401")), DEFAULT_INTERVAL_SECS);
    assert_eq!(parse_interval_secs(Some("1")), 1);
    assert_eq!(parse_interval_secs(Some("86400")), MAX_INTERVAL_SECS);
}

#[tokio::test(start_paused = true)]
async fn transient_failure_retries_then_succeeds() {
    let recorder = TestRecorder::default();
    let _recorder_guard = metrics::set_default_local_recorder(&recorder);
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&attempts);
    let result = retry_operation(
        move || {
            let attempt = observed.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt < 2 {
                    Err(anyhow::Error::new(sqlx::Error::PoolTimedOut))
                } else {
                    Ok(7_i64)
                }
            }
        },
        RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(20),
        },
    )
    .await;

    assert_eq!(result.expect("third attempt succeeds"), 7);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    let counters = recorder.counters.lock().expect("counter lock");
    assert_eq!(
        counters
            .iter()
            .filter(|metric| metric.name == "mp_session_compaction_retries_total")
            .count(),
        2
    );
    let histograms = recorder.histograms.lock().expect("histogram lock");
    assert_eq!(
        histograms
            .iter()
            .filter(|metric| metric.name == "mp_session_compaction_retry_delay_seconds")
            .count(),
        2
    );
}

#[tokio::test(start_paused = true)]
async fn poison_failure_is_not_retried() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&attempts);
    let result: Result<i64> = retry_operation(
        move || {
            observed.fetch_add(1, Ordering::SeqCst);
            async { Err(anyhow::anyhow!("invalid durable checkpoint state")) }
        },
        RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(20),
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn transient_failure_stops_at_attempt_limit() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&attempts);
    let result: Result<i64> = retry_operation(
        move || {
            observed.fetch_add(1, Ordering::SeqCst);
            async { Err(anyhow::Error::new(sqlx::Error::PoolTimedOut)) }
        },
        RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(20),
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn closed_pool_fails_without_network_or_retry() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy_with(sqlx::postgres::PgConnectOptions::new());
    pool.close().await;

    let error = compact_with_retry(&pool, RetryPolicy::default())
        .await
        .expect_err("closed pool must fail");
    assert!(matches!(
        error.downcast_ref::<sqlx::Error>(),
        Some(sqlx::Error::PoolClosed)
    ));
}

#[test]
fn cycle_metrics_distinguish_success_and_failure() {
    let recorder = TestRecorder::default();
    metrics::with_local_recorder(&recorder, || {
        record_cycle_metrics("ok", Duration::from_millis(15));
        record_cycle_metrics("error", Duration::from_millis(25));
    });

    let counters = recorder.counters.lock().expect("counter lock");
    assert_eq!(counters.len(), 2);
    assert!(counters.iter().any(|metric| {
        metric.name == "mp_session_compaction_runs_total"
            && metric.labels == [("status".to_owned(), "ok".to_owned())]
            && (metric.value - 1.0).abs() < f64::EPSILON
    }));
    assert!(counters.iter().any(|metric| {
        metric.name == "mp_session_compaction_runs_total"
            && metric.labels == [("status".to_owned(), "error".to_owned())]
            && (metric.value - 1.0).abs() < f64::EPSILON
    }));

    let histograms = recorder.histograms.lock().expect("histogram lock");
    assert_eq!(histograms.len(), 2);
    assert!(histograms.iter().all(|metric| {
        metric.name == "mp_session_compaction_duration_seconds" && metric.value > 0.0
    }));
}

// PG-gated regression guard for the GOAL invariant "compaction never
// replaces source": `compact_once` must only ADD a checkpoint and leave
// every source event intact. The query is INSERT-only today (provable by
// reading it), so this test LOCKS that in — a future change that adds an
// event cleanup (e.g. "prune events older than the last checkpoint") would
// fail here, catching the data-loss regression before it ships.
//
// #[ignore]d so plain `cargo test` (no DB) skips it; run with a DB:
//   DATABASE_URL=… cargo test -p session-core --bin session-core \
//     compaction_is_additive_never_deletes_events -- --ignored
#[tokio::test]
#[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
#[allow(clippy::too_many_lines)]
async fn compaction_is_additive_never_deletes_events() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL unset");
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");

    let sfx = std::process::id();
    let (thread_id, run_id, org) = (
        format!("ct-t-{sfx}"),
        format!("ct-r-{sfx}"),
        format!("ct-org-{sfx}"),
    );
    sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
        .bind(&thread_id)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed thread");
    sqlx::query("INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ($1,$2,'g',$3,'u1')")
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed run");

    // Seed source events; the trigger assigns step_ordinal for STEP_COMPLETED.
    for i in 0..3 {
        sqlx::query("INSERT INTO events (id,event_type,run_id) VALUES ($1,'STEP_COMPLETED',$2)")
            .bind(format!("ct-e-{sfx}-{i}"))
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("seed event");
    }
    let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE run_id=$1")
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("count before");
    assert_eq!(count_before, 3);

    // Compact: must add exactly one checkpoint for this fresh run...
    let rolled = compact_once(&pool).await.expect("compact");
    assert!(rolled >= 1, "expected >=1 checkpoint rolled, got {rolled}");
    let ck_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM checkpoints WHERE run_id=$1")
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("ck count");
    assert_eq!(ck_count, 1, "compaction must add exactly one checkpoint");
    let (checkpoint_id, state): (String, Vec<u8>) =
        sqlx::query_as("SELECT id,state FROM checkpoints WHERE run_id=$1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("checkpoint state");
    assert!(checkpoint_id.starts_with("auto-compact-v1:"));
    assert_eq!(checkpoint_id.len(), MAX_CHECKPOINT_ID_LEN);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&state).expect("checkpoint JSON"),
        serde_json::json!({
            "kind": "auto_compact_v1",
            "max_step": 3,
            "run_id": &run_id,
        })
    );

    // ...and must NOT have touched any source event (the invariant).
    let count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE run_id=$1")
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("count after");
    assert_eq!(
        count_after, count_before,
        "compaction must never delete source events"
    );

    // Idempotent: no new events since the checkpoint -> no new checkpoint,
    // events still intact.
    let rolled2 = compact_once(&pool).await.expect("compact2");
    assert_eq!(rolled2, 0, "no new events -> no new checkpoint");
    let count_final: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE run_id=$1")
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("count final");
    assert_eq!(
        count_final, count_before,
        "events still intact after re-compaction"
    );

    // cleanup (checkpoints/events first — checkpoints FK-references runs).
    for q in [
        "DELETE FROM checkpoints WHERE run_id=$1",
        "DELETE FROM events WHERE run_id=$1",
    ] {
        sqlx::query(q).bind(&run_id).execute(&pool).await.ok();
    }
    for q in [
        "DELETE FROM runs WHERE org_id=$1",
        "DELETE FROM threads WHERE org_id=$1",
    ] {
        sqlx::query(q).bind(&org).execute(&pool).await.ok();
    }
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
async fn concurrent_compaction_creates_one_checkpoint_per_watermark() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL unset");
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let thread_id = format!("concurrent-t-{suffix}");
    let run_id = format!("concurrent-r-{suffix}");
    let org = format!("concurrent-org-{suffix}");
    sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
        .bind(&thread_id)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed thread");
    sqlx::query("INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ($1,$2,'g',$3,'u1')")
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed run");
    sqlx::query("INSERT INTO events (id,event_type,run_id) VALUES ($1,'STEP_COMPLETED',$2)")
        .bind(format!("concurrent-e-{suffix}"))
        .bind(&run_id)
        .execute(&pool)
        .await
        .expect("seed event");

    let (left, right) = tokio::join!(compact_once(&pool), compact_once(&pool));
    left.expect("left compaction");
    right.expect("right compaction");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM checkpoints WHERE run_id=$1")
        .bind(&run_id)
        .fetch_one(&pool)
        .await
        .expect("checkpoint count");
    assert_eq!(count, 1, "one checkpoint for a shared step watermark");

    for query in [
        "DELETE FROM checkpoints WHERE run_id=$1",
        "DELETE FROM events WHERE run_id=$1",
    ] {
        sqlx::query(query).bind(&run_id).execute(&pool).await.ok();
    }
    for query in [
        "DELETE FROM runs WHERE org_id=$1",
        "DELETE FROM threads WHERE org_id=$1",
    ] {
        sqlx::query(query).bind(&org).execute(&pool).await.ok();
    }
}

#[tokio::test]
#[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
async fn zdr_and_poison_runs_do_not_create_checkpoints() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL unset");
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let org = format!("guard-org-{suffix}");
    let mut run_ids = Vec::new();
    for kind in ["zdr", "poison"] {
        let thread_id = format!("guard-t-{kind}-{suffix}");
        let run_id = format!("guard-r-{kind}-{suffix}");
        sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
            .bind(&thread_id)
            .bind(&org)
            .execute(&pool)
            .await
            .expect("seed thread");
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id,metadata) \
                 VALUES ($1,$2,'g',$3,'u1',$4)",
        )
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org)
        .bind(if kind == "zdr" {
            serde_json::json!({"zdr": true})
        } else {
            serde_json::json!({})
        })
        .execute(&pool)
        .await
        .expect("seed run");
        sqlx::query(
            "INSERT INTO events (id,event_type,run_id,step_ordinal) \
                 VALUES ($1,'STEP_COMPLETED',$2,$3)",
        )
        .bind(format!("guard-e-{kind}-{suffix}"))
        .bind(&run_id)
        .bind(if kind == "poison" { -1_i64 } else { 1_i64 })
        .execute(&pool)
        .await
        .expect("seed event");
        run_ids.push(run_id);
    }

    compact_once(&pool).await.expect("guarded compaction");
    for run_id in &run_ids {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM checkpoints WHERE run_id=$1")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .expect("checkpoint count");
        assert_eq!(count, 0, "guarded run {run_id} must not be checkpointed");
    }

    for run_id in &run_ids {
        sqlx::query("DELETE FROM events WHERE run_id=$1")
            .bind(run_id)
            .execute(&pool)
            .await
            .ok();
    }
    for query in [
        "DELETE FROM runs WHERE org_id=$1",
        "DELETE FROM threads WHERE org_id=$1",
    ] {
        sqlx::query(query).bind(&org).execute(&pool).await.ok();
    }
}
