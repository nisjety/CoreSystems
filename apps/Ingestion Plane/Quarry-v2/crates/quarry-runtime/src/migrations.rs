//! Runtime migration runner for the durable Postgres stores.
//!
//! Phase 1 Track C / cluster #9 fix. The `PostgresBaselineStore`,
//! `PostgresEventHistory`, and `PostgresProfileStore` wiring in
//! `quarry-edge/src/main.rs` only ever called `PgPool::connect` — the
//! `sqlx::migrate!` invocation lived exclusively in `#[cfg(test)]`
//! helpers. So a production edge wired with `QUARRY_EDGE__DATABASE_URL`
//! connected to an empty database, the `quarry_baselines` /
//! `quarry_change_diffs` tables were never created, and the first
//! `/v1/change/check` failed with a SQL "relation does not exist" error
//! instead of serving a real comparison.
//!
//! `run_migrations` applies the embedded `crates/quarry-runtime/migrations`
//! set (the same set the durable queue/profile/event-history/baseline
//! tables share) against a connected pool. The `migrate!` macro resolves
//! the path relative to *this* crate's manifest dir at compile time, so
//! it must live in `quarry-runtime` rather than `quarry-edge`.
//!
//! Idempotent: sqlx records applied versions in `_sqlx_migrations` and
//! skips anything already present, so re-running on boot is safe. Point
//! a *dedicated* database (e.g. `quarry_edge`) at it to avoid
//! `_sqlx_migrations` checksum collisions with any other service that
//! applies a different migration set to a shared database.

use sqlx::postgres::PgPool;

/// Apply the embedded migration set to `pool`. Surfaces the underlying
/// `sqlx::migrate::MigrateError` to the caller so boot code can log a
/// warning (and refuse to wire the durable store) rather than silently
/// serving against missing tables.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
