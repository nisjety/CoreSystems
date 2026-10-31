//! Postgres connection and migration runner.

use anyhow::Result;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use tracing::{info, warn};

pub type Pool = sqlx::PgPool;

/// Lets the migrator boot against a database that has applied migrations this
/// binary does not know (a rolled-back image over a newer schema). Off by
/// default: strict mode is the drift detector. Only the rollback overlay sets
/// it, and `scripts/tests/migration-expand-contract-test.sh` keeps the newer
/// schema safe for the older binary.
const IGNORE_MISSING_ENV: &str = "SESSION_CORE_MIGRATION_IGNORE_MISSING";

/// Connect to Postgres using `DATABASE_URL` env var.
///
/// # Errors
///
/// Returns an error if the connection cannot be established.
pub async fn connect_postgres() -> Result<Pool> {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5432/session_core".to_owned()
    });

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&url)
        .await?;

    info!("postgres connected");
    Ok(pool)
}

/// Only the literal `true` enables the switch; `1`, `TRUE` and `yes` do not.
fn ignore_missing_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// Applied versions the binary has no migration for, in ascending order.
fn unknown_applied_versions(known: &[i64], applied: &[i64]) -> Vec<i64> {
    let mut unknown: Vec<i64> = applied
        .iter()
        .copied()
        .filter(|version| !known.contains(version))
        .collect();
    unknown.sort_unstable();
    unknown
}

/// Runs `migrator` against `pool`. With `ignore_missing` the unknown applied
/// versions are logged first, because sqlx no longer reports them.
async fn apply(pool: &Pool, mut migrator: Migrator, ignore_missing: bool) -> Result<()> {
    if ignore_missing {
        migrator.set_ignore_missing(true);
        // The table does not exist on a first boot, so nothing is unknown then.
        if let Ok(applied) =
            sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations")
                .fetch_all(pool)
                .await
        {
            let known: Vec<i64> = migrator.iter().map(|migration| migration.version).collect();
            let unknown = unknown_applied_versions(&known, &applied);
            if !unknown.is_empty() {
                warn!(
                    ?unknown,
                    "database has applied migrations this binary does not know; ignoring them"
                );
            }
        }
    }
    migrator.run(pool).await?;
    Ok(())
}

/// Run embedded migrations.
///
/// # Errors
///
/// Returns an error if any migration fails, or if the database has applied a
/// migration this binary does not know and `SESSION_CORE_MIGRATION_IGNORE_MISSING`
/// is not `true`.
pub async fn run_migrations(pool: &Pool) -> Result<()> {
    let ignore_missing = ignore_missing_from(std::env::var(IGNORE_MISSING_ENV).ok().as_deref());
    apply(pool, sqlx::migrate!("./migrations"), ignore_missing).await?;
    info!("migrations applied");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn ignore_missing_is_off_unless_the_literal_true() {
        assert!(!ignore_missing_from(None));
        for value in ["", "false", "1", "TRUE", "True", "yes", " true"] {
            assert!(!ignore_missing_from(Some(value)), "{value:?} must not enable");
        }
        assert!(ignore_missing_from(Some("true")));
    }

    #[test]
    fn unknown_applied_versions_lists_only_what_the_binary_lacks() {
        assert_eq!(unknown_applied_versions(&[1, 2, 3], &[1, 2, 3]), Vec::<i64>::new());
        assert_eq!(unknown_applied_versions(&[1, 2], &[2, 5, 1, 4]), vec![4, 5]);
        assert_eq!(unknown_applied_versions(&[], &[]), Vec::<i64>::new());
    }

    // A migrator that knows fewer versions than the database has applied: a
    // rolled-back image over a newer schema. Runs in a scratch schema so it
    // never touches migrations already applied to the shared database.
    //
    // #[ignore]d so plain `cargo test` (no DB) skips it; run with a DB:
    //   DATABASE_URL=... cargo test -p session-core --bin session-core \
    //     older_migrator_boots_only_with_ignore_missing -- --ignored
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres"]
    async fn older_migrator_boots_only_with_ignore_missing() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let schema = format!("migration_policy_{}", std::process::id());
        let admin = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("create scratch schema");

        let search_path = format!("SET search_path TO {schema}, public");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .after_connect(move |conn, _meta| {
                let search_path = search_path.clone();
                Box::pin(async move {
                    sqlx::query(&search_path).execute(conn).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .expect("connect scratch pool");

        let prefix = |newest: i64| {
            let mut migrator = sqlx::migrate!("./migrations");
            let kept: Vec<_> = migrator
                .iter()
                .filter(|migration| migration.version <= newest)
                .cloned()
                .collect();
            migrator.migrations = Cow::Owned(kept);
            migrator
        };

        apply(&pool, prefix(43), false)
            .await
            .expect("apply migrations 0001-0043");
        let strict = apply(&pool, prefix(42), false).await;
        let lenient = apply(&pool, prefix(42), true).await;

        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("drop scratch schema");

        assert!(
            strict.is_err(),
            "a strict migrator must refuse a database that applied version 43"
        );
        lenient.expect("ignore_missing must boot over an unknown applied version");
    }
}
