//! Shared scaffolding for the `vsv2 monitor` integration tests.
//!
//! One throwaway Postgres database per test (`vsv2_monitor_test_<uuid>` on the local compose
//! Postgres, the real migrations applied with `StateStore::migrate()`, force-dropped on `Drop`),
//! rows seeded through the state crate's own functions and then moved back in time, and the real
//! `vsv2` binary run against it. The live databases are never touched.

#![allow(dead_code)] // each test binary uses a different subset

use std::path::{Path, PathBuf};
use std::process::Stdio;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;
use tempfile::TempDir;
use uuid::Uuid;
use vsv2_common::{Source, TenantId};
use vsv2_orchestrator::open_set::run_label;
use vsv2_state::scheduler_pass::{self, PassStatus};
use vsv2_state::{RunStatus, StateStore, SyncRun};

const DEFAULT_ADMIN_URL: &str = "postgres://visma_v2:visma_v2@127.0.0.1:55433/postgres";
/// Redis is never contacted; its pool is created lazily.
const UNUSED_REDIS_URL: &str = "redis://127.0.0.1:6379";

pub const ACTIVE: &str = "ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd";
pub const OTHER: &str = "ffa4a061-7717-11ee-970e-025417856183";
pub const HISTORICAL: &str = "9b1fa69b-5568-11ee-960c-025417856183";

pub const OPEN_SET: [&str; 2] = ["customer_invoices", "supplier_invoices"];

pub struct TestDb {
    pub state: StateStore,
    /// The URL of the throwaway database, password included.
    pub url: String,
    name: String,
    admin_url: String,
}

impl TestDb {
    /// A database with the whole schema and no rows.
    pub async fn create() -> Self {
        let db = Self::create_unmigrated().await;
        db.state.migrate().await.expect("apply the real migrations");
        db
    }

    /// A database with nothing in it: no `vsv2` schema.
    pub async fn create_unmigrated() -> Self {
        let admin_url = std::env::var("VSV2_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| DEFAULT_ADMIN_URL.to_string());
        let name = format!("vsv2_monitor_test_{}", Uuid::new_v4().simple());

        let admin = PgPool::connect(&admin_url).await.unwrap_or_else(|e| {
            panic!(
                "cannot reach the test Postgres: {e}\n\
                 Start it with `make up`, or point VSV2_TEST_ADMIN_DATABASE_URL at a \
                 Postgres where the user may CREATE DATABASE."
            )
        });
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .unwrap_or_else(|e| panic!("create test database {name}: {e}"));
        admin.close().await;

        let url = with_database(&admin_url, &name);
        let state = StateStore::connect(&url, UNUSED_REDIS_URL)
            .await
            .unwrap_or_else(|e| panic!("connect to test database {name}: {e}"));
        Self {
            state,
            url,
            name,
            admin_url,
        }
    }

    /// A pass of `tier` that began `started_ago` ago and ended as `status` (still running for
    /// `Running`). `failed` are the endpoint names a failed pass lists.
    pub async fn pass(
        &self,
        tenant: &str,
        tier: &str,
        status: PassStatus,
        started_ago: Duration,
        failed: &[&str],
    ) -> Uuid {
        let pass_id = Uuid::new_v4();
        let pool = &self.state.pg;
        scheduler_pass::start_pass(pool, pass_id, tier, &TenantId::new(tenant), None)
            .await
            .expect("start a pass");
        if status != PassStatus::Running {
            let names: Vec<String> = failed.iter().map(|name| name.to_string()).collect();
            scheduler_pass::finish_pass(pool, pass_id, status, 15, &names, 0, None)
                .await
                .expect("finish a pass");
        }
        // The state functions stamp the database's clock; move the pass back in time.
        sqlx::query(
            "UPDATE vsv2.scheduler_pass \
             SET started_at = $2, heartbeat_at = $2, \
                 finished_at = CASE WHEN status = 'running' THEN NULL \
                                    ELSE $2 + interval '2 minutes' END \
             WHERE pass_id = $1",
        )
        .bind(pass_id)
        .bind(Utc::now() - started_ago)
        .execute(pool)
        .await
        .expect("move the pass back in time");
        pass_id
    }

    pub async fn succeeded(&self, tenant: &str, tier: &str, started_ago: Duration) -> Uuid {
        self.pass(tenant, tier, PassStatus::Succeeded, started_ago, &[])
            .await
    }

    pub async fn failed(
        &self,
        tenant: &str,
        tier: &str,
        started_ago: Duration,
        names: &[&str],
    ) -> Uuid {
        self.pass(tenant, tier, PassStatus::Failed, started_ago, names)
            .await
    }

    /// A pass still `running` that began `started_ago` ago and last beat `heartbeat_ago` ago.
    pub async fn running(
        &self,
        tenant: &str,
        tier: &str,
        started_ago: Duration,
        heartbeat_ago: Duration,
    ) -> Uuid {
        let pass_id = self
            .pass(tenant, tier, PassStatus::Running, started_ago, &[])
            .await;
        sqlx::query("UPDATE vsv2.scheduler_pass SET heartbeat_at = $2 WHERE pass_id = $1")
            .bind(pass_id)
            .bind(Utc::now() - heartbeat_ago)
            .execute(&self.state.pg)
            .await
            .expect("move the heartbeat back in time");
        pass_id
    }

    /// An open-set run of `endpoint` that began `started_ago` ago and ended as `status`.
    pub async fn open_set_run(
        &self,
        tenant: &str,
        endpoint: &str,
        status: RunStatus,
        started_ago: Duration,
    ) -> Uuid {
        let run_id = Uuid::new_v4();
        self.state
            .insert_run(&SyncRun {
                run_id,
                tenant: TenantId::new(tenant),
                endpoint: run_label(endpoint),
                source: Source::ErpRest,
                status: RunStatus::Running,
                started_at: Utc::now() - started_ago,
                finished_at: None,
                records_in: 0,
                records_written: 0,
                error: None,
            })
            .await
            .expect("insert a run");
        if status != RunStatus::Running {
            let error = (status == RunStatus::Failed).then_some("boom");
            self.state
                .finish_run(run_id, status, 0, 0, error)
                .await
                .expect("finish a run");
        }
        run_id
    }

    /// A tenant that is up to date everywhere: all three tiers, and both open-set endpoints.
    pub async fn healthy(&self, tenant: &str) {
        self.succeeded(tenant, "hourly", Duration::minutes(20))
            .await;
        self.succeeded(tenant, "daily", Duration::hours(3)).await;
        self.succeeded(tenant, "full", Duration::days(2)).await;
        for endpoint in OPEN_SET {
            self.open_set_run(tenant, endpoint, RunStatus::Succeeded, Duration::hours(5))
                .await;
        }
    }

    /// A tenant that only runs the weekly full pass, and ran it.
    pub async fn healthy_historical(&self, tenant: &str) {
        self.succeeded(tenant, "full", Duration::days(3)).await;
    }

    pub async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM vsv2.{table}"))
            .fetch_one(&self.state.pg)
            .await
            .unwrap_or_else(|e| panic!("count {table}: {e}"))
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let admin_url = self.admin_url.clone();
        let name = self.name.clone();
        // Drop runs inside a tokio runtime, so the cleanup gets its own thread.
        let _ = std::thread::spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(async {
                if let Ok(admin) = PgPool::connect(&admin_url).await {
                    let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
                        .execute(&admin)
                        .await;
                    admin.close().await;
                }
            });
        })
        .join();
    }
}

/// Swap the database name at the end of a `postgres://…/db` URL (no query string).
fn with_database(url: &str, database: &str) -> String {
    let (base, _) = url
        .rsplit_once('/')
        .expect("admin database URL must end in /<database>");
    format!("{base}/{database}")
}

/// A directory of tenant files that is also the working directory of the monitor under test, so
/// the repository's `.env` is never read.
pub struct Fixture {
    dir: TempDir,
}

impl Fixture {
    pub fn new(tenants: &[(&str, &str, bool)]) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let tenants_dir = dir.path().join("tenants");
        std::fs::create_dir(&tenants_dir).expect("create the tenants directory");
        for (id, name, historical) in tenants {
            let status = if *historical {
                r#""status": "historical", "valid_to": "2025-12-31","#
            } else {
                ""
            };
            let json = format!(r#"{{ {status} "tenant_id": "{id}", "tenant_name": "{name}" }}"#);
            std::fs::write(tenants_dir.join(format!("{id}.json")), json)
                .expect("write a tenant file");
        }
        Self { dir }
    }

    /// Aqua Pharma AS (active) and Consult AS (historical).
    pub fn standard() -> Self {
        Self::new(&[
            (ACTIVE, "Aqua Pharma AS", false),
            (HISTORICAL, "Consult AS", true),
        ])
    }

    /// Aqua Pharma AS and Aqua Pharma Group AS, both active.
    pub fn two_active() -> Self {
        Self::new(&[
            (ACTIVE, "Aqua Pharma AS", false),
            (OTHER, "Aqua Pharma Group AS", false),
        ])
    }

    pub fn tenants_dir(&self) -> PathBuf {
        self.dir.path().join("tenants")
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

/// What one run of `vsv2 monitor` did.
#[derive(Debug)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    /// stdout as JSON: fails the test when anything but the report is on it.
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout is not the JSON report ({e}):\n{}", self.stdout))
    }

    /// Everything the process printed.
    pub fn everything(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

/// How to run the monitor: against which database, with which extra flags and environment.
pub struct Run<'a> {
    pub fixture: &'a Fixture,
    pub database_url: Option<&'a str>,
    /// Replaces the fixture's tenant directory.
    pub tenants_dir: Option<&'a str>,
    pub flags: Vec<&'a str>,
    pub env: Vec<(&'a str, &'a str)>,
}

impl<'a> Run<'a> {
    pub fn new(fixture: &'a Fixture, database_url: &'a str) -> Self {
        Self {
            fixture,
            database_url: Some(database_url),
            tenants_dir: None,
            flags: vec!["--format", "json"],
            env: Vec::new(),
        }
    }

    pub fn flags(mut self, flags: &[&'a str]) -> Self {
        self.flags.extend_from_slice(flags);
        self
    }

    pub fn env(mut self, name: &'a str, value: &'a str) -> Self {
        self.env.push((name, value));
        self
    }

    pub fn without_database(mut self) -> Self {
        self.database_url = None;
        self
    }

    pub fn tenants_dir(mut self, dir: &'a str) -> Self {
        self.tenants_dir = Some(dir);
        self
    }

    /// The real `vsv2` binary, with `--tenants-dir` pointing at the fixture.
    pub async fn go(self) -> Outcome {
        let tenants_dir = self
            .tenants_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| self.fixture.tenants_dir());
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_vsv2"));
        command
            .arg("monitor")
            .arg("--tenants-dir")
            .arg(&tenants_dir)
            .args(&self.flags)
            .current_dir(self.fixture.path())
            .env_remove("VSV2_DATABASE_URL")
            .env_remove("VSV2_MONITOR_WEBHOOK_URL")
            .env_remove("RUST_LOG")
            .stdin(Stdio::null());
        if let Some(url) = self.database_url {
            command.env("VSV2_DATABASE_URL", url);
        }
        for (name, value) in &self.env {
            command.env(name, value);
        }
        let output = command.output().await.expect("run vsv2 monitor");
        Outcome {
            code: output
                .status
                .code()
                .expect("vsv2 monitor ended with an exit code"),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

/// `(tenant_id or "-", check)` of every finding of `severity`, in report order.
pub fn with_severity(report: &Value, severity: &str) -> Vec<(String, String)> {
    report["findings"]
        .as_array()
        .expect("findings is an array")
        .iter()
        .filter(|finding| finding["severity"] == severity)
        .map(|finding| {
            (
                finding["tenant_id"].as_str().unwrap_or("-").to_string(),
                finding["check"].as_str().expect("a check").to_string(),
            )
        })
        .collect()
}

pub fn finding<'a>(report: &'a Value, tenant: &str, check: &str) -> &'a Value {
    report["findings"]
        .as_array()
        .expect("findings is an array")
        .iter()
        .find(|finding| finding["tenant_id"] == tenant && finding["check"] == check)
        .unwrap_or_else(|| panic!("no {check} finding for {tenant} in {report:#}"))
}

pub fn instant(report: &Value, pointer: &str) -> DateTime<Utc> {
    report
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no timestamp at {pointer} in {report:#}"))
        .parse()
        .expect("an RFC 3339 timestamp")
}

/// A pipeline with exactly one problem: Aqua Pharma AS' hourly tier is five hours old.
pub async fn pipeline_with_one_problem() -> (TestDb, Fixture) {
    let db = TestDb::create().await;
    db.succeeded(ACTIVE, "hourly", Duration::hours(5)).await;
    db.succeeded(ACTIVE, "daily", Duration::hours(3)).await;
    db.succeeded(ACTIVE, "full", Duration::days(2)).await;
    for endpoint in OPEN_SET {
        db.open_set_run(ACTIVE, endpoint, RunStatus::Succeeded, Duration::hours(5))
            .await;
    }
    db.healthy_historical(HISTORICAL).await;
    (db, Fixture::standard())
}
