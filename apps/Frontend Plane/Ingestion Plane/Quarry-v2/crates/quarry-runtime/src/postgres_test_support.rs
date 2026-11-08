//! Test setup shared by the `postgres_*` stores (tests only).
//!
//! The stores' tables are unqualified (`quarry_profiles`, `quarry_baselines`,
//! `quarry_job_history`, `quarry_queue_items`, ...), so every test that used the
//! database's default schema shared one set of tables with every other test, and
//! each of them began with a `TRUNCATE`: tests of one store, and tests of different
//! stores, wiped each other's rows when libtest ran them in parallel.
//!
//! [`isolated_pool`] gives each test its own schema instead. The pool's connections
//! all run with `search_path` set to that schema, the migrations are applied into it
//! (sqlx puts `_sqlx_migrations` there too), and the stores' unqualified table names
//! resolve to the test's own tables. Nothing is shared, so the tests need no
//! `--test-threads=1`.
//!
//! Schemas are not dropped when a test ends (an async drop would need a runtime of
//! its own); each call instead drops the `qt_` schemas that are older than
//! [`STALE_AFTER_SECS`], whose creation time is in their name, so a disposable
//! database does not collect them.
//!
//! A job that provides a database sets `REQUIRE_CONTRACT=1`: then a missing or
//! unreachable `DATABASE_URL` fails the test instead of skipping it, and so does a
//! migration that does not apply.

use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool, Row};

const SCHEMA_PREFIX: &str = "qt_";
/// Older than this, a schema left by an earlier run is dropped.
const STALE_AFTER_SECS: u64 = 6 * 60 * 60;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A pool on a fresh schema with every migration applied, or `None` (the test
/// skips) when there is no `DATABASE_URL` and `REQUIRE_CONTRACT` is not set.
/// `what` names the tests in the failure message.
pub(crate) async fn isolated_pool(what: &str) -> Option<PgPool> {
    let required = std::env::var_os("REQUIRE_CONTRACT").is_some();
    let Some(url) = std::env::var("DATABASE_URL")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        assert!(
            !required,
            "REQUIRE_CONTRACT is set but DATABASE_URL is not: the {what} tests would silently skip"
        );
        return None;
    };
    let admin = match PgPool::connect(&url).await {
        Ok(pool) => pool,
        Err(err) => {
            assert!(
                !required,
                "REQUIRE_CONTRACT is set but DATABASE_URL is unreachable: {err}"
            );
            return None;
        }
    };

    drop_stale_schemas(&admin).await;
    let schema = format!(
        "{SCHEMA_PREFIX}{}_{}",
        now_secs(),
        uuid::Uuid::new_v4().simple()
    );
    admin
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .expect("a test schema can be created");
    admin.close().await;

    let set_search_path = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |connection, _meta| {
            let statement = set_search_path.clone();
            Box::pin(async move {
                connection.execute(statement.as_str()).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .expect("the test schema's pool connects");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations must apply");
    Some(pool)
}

/// Drop the test schemas of earlier runs. Best effort: another process may be
/// dropping the same ones, and a failure here must not fail the test.
async fn drop_stale_schemas(admin: &PgPool) {
    let Ok(rows) = sqlx::query("SELECT nspname FROM pg_namespace WHERE nspname LIKE 'qt\\_%'")
        .fetch_all(admin)
        .await
    else {
        return;
    };
    let oldest_kept = now_secs().saturating_sub(STALE_AFTER_SECS);
    for row in rows {
        let Ok(name) = row.try_get::<String, _>("nspname") else {
            continue;
        };
        let created = name
            .strip_prefix(SCHEMA_PREFIX)
            .and_then(|rest| rest.split('_').next())
            .and_then(|secs| secs.parse::<u64>().ok());
        // Only a name this module made (`qt_<secs>_<uuid>`) is ever dropped.
        if matches!(created, Some(secs) if secs < oldest_kept) {
            let _ = admin
                .execute(format!("DROP SCHEMA IF EXISTS {name} CASCADE").as_str())
                .await;
        }
    }
}
