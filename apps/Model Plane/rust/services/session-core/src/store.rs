//! Postgres connection and migration runner.

use anyhow::Result;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use std::time::Instant;
use tracing::{info, warn};

pub type Pool = sqlx::PgPool;

/// Lets the migrator boot against a database that has applied migrations this
/// binary does not know (a rolled-back image over a newer schema). Off by
/// default: strict mode is the drift detector. Only the rollback overlay sets
/// it, and `scripts/tests/migration-expand-contract-test.sh` keeps the newer
/// schema safe for the older binary.
const IGNORE_MISSING_ENV: &str = "SESSION_CORE_MIGRATION_IGNORE_MISSING";

/// Builds the message-tree indexes of migration 0061 CONCURRENTLY before the migrator runs it on a populated
/// database. The migration builds them plainly, which holds a SHARE lock on `messages` until each build ends.
/// Read once at boot, by [`run_migrations`]. Only the literal `true` turns it on; off by default.
pub(crate) const PREBUILD_MESSAGE_INDEXES_ENV: &str = "SESSION_CORE_PREBUILD_MESSAGE_INDEXES";

/// Migration 0061, which adds the message-tree columns and their two indexes.
const MESSAGE_TREE_MIGRATION: i64 = 61;

/// Migration 0061's column statement, as the migration writes it. The test `prebuild_statements_match_migration_0061`
/// keeps this text equal to the migration's own, so the two cannot drift.
const MESSAGE_TREE_COLUMNS_SQL: &str = "ALTER TABLE messages \
    ADD COLUMN IF NOT EXISTS parent_message_id TEXT, \
    ADD COLUMN IF NOT EXISTS sibling_ordinal INTEGER NOT NULL DEFAULT 1, \
    ADD COLUMN IF NOT EXISTS on_active_path BOOLEAN NOT NULL DEFAULT TRUE";

/// The two indexes of migration 0061, each named and defined as the migration names it, with CONCURRENTLY added.
const PARENT_INDEX: &str = "idx_messages_parent";
const PARENT_INDEX_SQL: &str = "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_messages_parent \
    ON messages (thread_id, parent_message_id) WHERE parent_message_id IS NOT NULL";
const ACTIVE_INDEX: &str = "idx_messages_thread_active";
const ACTIVE_INDEX_SQL: &str =
    "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_messages_thread_active \
     ON messages (thread_id, sequence) WHERE on_active_path";

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

/// Only the literal `true` enables the prebuild; `1`, `TRUE` and `yes` do not.
fn prebuild_enabled_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// The SQLSTATE of a database error, the only detail a prebuild log carries. The message is left out, since a
/// database message can quote a value.
fn sqlstate(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map_or_else(|| "none".to_owned(), |code| code.into_owned())
}

/// Runs one statement on a plain pool connection. Never inside a transaction: PostgreSQL refuses CREATE INDEX
/// CONCURRENTLY and DROP INDEX CONCURRENTLY in a transaction block.
async fn run_statement(pool: &Pool, sql: &str) -> Result<(), sqlx::Error> {
    sqlx::query(sql).execute(pool).await.map(|_| ())
}

/// Whether the index `name` on `messages` is INVALID, as a failed concurrent build leaves it.
async fn index_is_invalid(pool: &Pool, name: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM pg_index AS i
             JOIN pg_class AS c ON c.oid = i.indexrelid
             WHERE i.indrelid = to_regclass('messages')::oid
               AND c.relname::text = $1 AND NOT i.indisvalid
         )",
    )
    .bind(name)
    .fetch_one(pool)
    .await
}

/// Drops the index `name` if a failed concurrent build left it INVALID. A valid index is kept. The name is always one of
/// this module's constants, never input.
async fn drop_invalid_index(pool: &Pool, name: &str) -> Result<(), sqlx::Error> {
    if index_is_invalid(pool, name).await? {
        run_statement(pool, &format!("DROP INDEX CONCURRENTLY IF EXISTS {name}")).await?;
    }
    Ok(())
}

/// Builds one of the two indexes ahead of migration 0061. An INVALID index of the same name, left by a failed earlier
/// build, is dropped first. A failed build is logged, and the INVALID index it may leave is dropped as well: the
/// migration's `IF NOT EXISTS` would otherwise keep that index, and the migration would then build nothing.
async fn build_index(pool: &Pool, name: &str, sql: &str) {
    let started = Instant::now();
    let outcome: Result<(), sqlx::Error> = async {
        drop_invalid_index(pool, name).await?;
        run_statement(pool, sql).await
    }
    .await;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match outcome {
        Ok(()) => info!(
            index = name,
            elapsed_ms, "message-tree index ready ahead of migration 0061"
        ),
        Err(error) => {
            warn!(
                index = name,
                elapsed_ms,
                sqlstate = %sqlstate(&error),
                "message-tree index prebuild failed; migration 0061 builds it instead"
            );
            if let Err(error) = drop_invalid_index(pool, name).await {
                warn!(
                    index = name,
                    sqlstate = %sqlstate(&error),
                    "an INVALID index left by the failed build could not be dropped"
                );
            }
        }
    }
}

/// Builds the message-tree indexes of migration 0061 CONCURRENTLY, before the migrator runs it on a database that
/// already holds messages. Migration 0061 then finds the columns and both indexes present, and its IF NOT EXISTS
/// statements do nothing. It reads no turn and writes nothing beyond the schema.
///
/// Skipped on a fresh database (no `messages` table, or no migration history yet) and once migration 0061 is
/// recorded. A failure is logged with the index name, the duration and the SQLSTATE, and never stops boot: the
/// migration then builds plain indexes, as it did before this existed.
async fn prebuild_message_tree_indexes(pool: &Pool) {
    let state: Result<(bool, bool), sqlx::Error> = sqlx::query_as(
        "SELECT to_regclass('messages') IS NOT NULL, to_regclass('_sqlx_migrations') IS NOT NULL",
    )
    .fetch_one(pool)
    .await;
    let (has_messages, has_history) = match state {
        Ok(state) => state,
        Err(error) => {
            warn!(sqlstate = %sqlstate(&error), "message-tree index prebuild skipped: the schema could not be read");
            return;
        }
    };
    if !has_messages || !has_history {
        return;
    }
    let recorded: Result<bool, sqlx::Error> =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = $1)")
            .bind(MESSAGE_TREE_MIGRATION)
            .fetch_one(pool)
            .await;
    match recorded {
        Ok(false) => {}
        Ok(true) => return,
        Err(error) => {
            warn!(sqlstate = %sqlstate(&error), "message-tree index prebuild skipped: the migration history could not be read");
            return;
        }
    }
    if let Err(error) = run_statement(pool, MESSAGE_TREE_COLUMNS_SQL).await {
        warn!(sqlstate = %sqlstate(&error), "message-tree index prebuild stopped: the columns could not be added");
        return;
    }
    build_index(pool, PARENT_INDEX, PARENT_INDEX_SQL).await;
    build_index(pool, ACTIVE_INDEX, ACTIVE_INDEX_SQL).await;
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
    // Read once, at boot: a literal `true` builds the message-tree indexes before the migrator runs (WS17 S8).
    if prebuild_enabled_from(std::env::var(PREBUILD_MESSAGE_INDEXES_ENV).ok().as_deref()) {
        prebuild_message_tree_indexes(pool).await;
    }
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

    #[test]
    fn the_prebuild_switch_is_off_unless_the_literal_true() {
        assert!(!prebuild_enabled_from(None));
        for value in ["", "false", "1", "TRUE", "True", "yes", " true"] {
            assert!(
                !prebuild_enabled_from(Some(value)),
                "{value:?} must not enable"
            );
        }
        assert!(prebuild_enabled_from(Some("true")));
        assert_eq!(
            PREBUILD_MESSAGE_INDEXES_ENV,
            "SESSION_CORE_PREBUILD_MESSAGE_INDEXES"
        );
    }

    /// The text with every run of whitespace reduced to one space, so a line break in either file does not matter.
    fn squeezed(sql: &str) -> String {
        sql.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The prebuild runs migration 0061's own statements, so they cannot drift from it: each one, with CONCURRENTLY
    /// taken out, appears in the migration as written. The index names the statements create are the ones the
    /// prebuild checks and drops.
    #[test]
    fn prebuild_statements_match_migration_0061() {
        let migration = squeezed(include_str!("../migrations/0061_message_versions.sql"));
        for sql in [MESSAGE_TREE_COLUMNS_SQL, PARENT_INDEX_SQL, ACTIVE_INDEX_SQL] {
            let as_migration = squeezed(&sql.replacen("CONCURRENTLY ", "", 1));
            assert!(
                migration.contains(&as_migration),
                "migration 0061 has no statement {as_migration:?}"
            );
        }
        assert!(PARENT_INDEX_SQL.contains(&format!(" {PARENT_INDEX} ")));
        assert!(ACTIVE_INDEX_SQL.contains(&format!(" {ACTIVE_INDEX} ")));
    }

    /// Migrations up to and including `newest`, as a migrator for a scratch schema.
    fn migrations_up_to(newest: i64) -> Migrator {
        let mut migrator = sqlx::migrate!("./migrations");
        let kept: Vec<_> = migrator
            .iter()
            .filter(|migration| migration.version <= newest)
            .cloned()
            .collect();
        migrator.migrations = Cow::Owned(kept);
        migrator
    }

    /// A pool whose connections search only `schema`, so a test's tables and migrations stay in it.
    async fn scratch_pool(url: &str, schema: &str) -> sqlx::PgPool {
        let search_path = format!("SET search_path TO {schema}, public");
        PgPoolOptions::new()
            .max_connections(2)
            .after_connect(move |conn, _meta| {
                let search_path = search_path.clone();
                Box::pin(async move {
                    sqlx::query(&search_path).execute(conn).await?;
                    Ok(())
                })
            })
            .connect(url)
            .await
            .expect("connect scratch pool")
    }

    /// Migrated to 0060 in `schema`, with one thread of `turns` messages: a transcript as it stands before 0061.
    /// The schema must already exist.
    async fn populated_before_0061(url: &str, schema: &str, turns: usize) -> sqlx::PgPool {
        let pool = scratch_pool(url, schema).await;
        apply(&pool, migrations_up_to(60), false)
            .await
            .expect("apply migrations 0001-0060");
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id)
             VALUES ('thread-1', 'thread-1', 'org-1', 'user-1')",
        )
        .execute(&pool)
        .await
        .expect("insert a thread");
        for n in 0..turns {
            sqlx::query(
                "INSERT INTO messages (id, thread_id, role, content)
                 VALUES ($1, 'thread-1', 'user', 'turn')",
            )
            .bind(format!("message-{n}"))
            .execute(&pool)
            .await
            .expect("insert a turn");
        }
        pool
    }

    /// The two message-tree indexes as the catalogue holds them: name, oid, validity and definition.
    async fn message_indexes(pool: &sqlx::PgPool) -> Vec<(String, String, bool, String)> {
        sqlx::query_as(
            "SELECT c.relname::text, c.oid::text, i.indisvalid, pg_get_indexdef(c.oid)
             FROM pg_index AS i
             JOIN pg_class AS c ON c.oid = i.indexrelid
             WHERE i.indrelid = to_regclass('messages')::oid
               AND c.relname::text IN ('idx_messages_parent', 'idx_messages_thread_active')
             ORDER BY c.relname::text",
        )
        .fetch_all(pool)
        .await
        .expect("read the message-tree indexes")
    }

    // Needs a Postgres, like the test above: run with
    //   DATABASE_URL=... cargo test -p session-core --bin session-core \
    //     prebuild_builds_valid_indexes -- --ignored
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres"]
    async fn prebuild_builds_valid_indexes_that_migration_0061_then_keeps() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let schema = format!("prebuild_populated_{}", std::process::id());
        let admin = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("create scratch schema");
        let pool = populated_before_0061(&url, &schema, 200).await;

        prebuild_message_tree_indexes(&pool).await;
        let prebuilt = message_indexes(&pool).await;
        let migrated = apply(&pool, migrations_up_to(61), false).await;
        let after = message_indexes(&pool).await;

        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("drop scratch schema");
        assert_eq!(
            prebuilt.len(),
            2,
            "both indexes are built ahead of the migration: {prebuilt:?}"
        );
        assert!(
            prebuilt.iter().all(|index| index.2),
            "both are valid: {prebuilt:?}"
        );
        assert!(
            prebuilt[0].3.contains("parent_message_id"),
            "{:?}",
            prebuilt[0]
        );
        assert!(
            prebuilt[1].3.contains("on_active_path"),
            "{:?}",
            prebuilt[1]
        );
        migrated.expect("migration 0061 runs over the prebuilt indexes");
        assert_eq!(
            after, prebuilt,
            "the migration keeps the prebuilt indexes and builds nothing again"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres"]
    async fn prebuild_replaces_an_invalid_index_that_a_failed_build_left() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let schema = format!("prebuild_invalid_{}", std::process::id());
        let admin = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("create scratch schema");
        let pool = populated_before_0061(&url, &schema, 2).await;

        // Two turns of one thread break a unique index on the thread alone: the build fails, and it leaves the
        // name the migration uses taken by an INVALID index.
        let failed = sqlx::query(
            "CREATE UNIQUE INDEX CONCURRENTLY idx_messages_parent ON messages (thread_id)",
        )
        .execute(&pool)
        .await;
        let left = message_indexes(&pool).await;
        prebuild_message_tree_indexes(&pool).await;
        let repaired = message_indexes(&pool).await;
        let migrated = apply(&pool, migrations_up_to(61), false).await;
        let after = message_indexes(&pool).await;

        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("drop scratch schema");
        assert!(failed.is_err(), "the second turn breaks the unique index");
        assert!(
            left.iter().any(|index| index.0 == PARENT_INDEX && !index.2),
            "the failed build left an INVALID index: {left:?}"
        );
        assert!(
            repaired.iter().all(|index| index.2),
            "both are valid: {repaired:?}"
        );
        assert!(
            repaired[0].3.contains("parent_message_id"),
            "the repaired index has the migration's definition: {repaired:?}"
        );
        migrated.expect("migration 0061 runs over the repaired indexes");
        assert_eq!(after, repaired, "the migration keeps the repaired indexes");
    }
}
