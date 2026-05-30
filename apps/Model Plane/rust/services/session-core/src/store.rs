//! Postgres connection and migration runner.

use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use tracing::info;

pub type Pool = sqlx::PgPool;

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

/// Run embedded migrations.
///
/// # Errors
///
/// Returns an error if any migration fails.
pub async fn run_migrations(pool: &Pool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    info!("migrations applied");
    Ok(())
}
