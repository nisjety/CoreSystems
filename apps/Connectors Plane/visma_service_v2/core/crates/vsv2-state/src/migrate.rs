//! The schema of the state database, applied from the `migrations/` folder.
//!
//! The SQL files are embedded when the crate is built, so an image carries the schema its own code
//! expects and `vsv2 schedule` creates or updates the database before it reads a checkpoint. The
//! files are idempotent, so this is also safe on a database that was migrated by hand (`make
//! migrate`): the first run applies every file again and records it in `_sqlx_migrations`, later
//! runs apply only what is new. A recorded file that was edited afterwards is refused (its checksum
//! no longer matches): a changed schema is a new file. Runs take an advisory lock, so two processes
//! that start together (the scheduler and the HTTP service) do not race.

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPool;

use crate::StateStore;
use vsv2_common::{Error, Result};

static MIGRATOR: Migrator = sqlx::migrate!("../../../migrations");

/// Version and description of every embedded migration, in the order they are applied.
pub fn embedded() -> Vec<(i64, String)> {
    MIGRATOR
        .iter()
        .map(|m| (m.version, m.description.to_string()))
        .collect()
}

fn state_error(context: &str, error: impl std::fmt::Display) -> Error {
    Error::StateStore(format!("{context}: {error}"))
}

/// The versions recorded as applied. `_sqlx_migrations` does not exist before the first run.
async fn recorded_versions(pool: &PgPool) -> Result<Vec<i64>> {
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
        .fetch_one(pool)
        .await
        .map_err(|e| state_error("check the migration table", e))?;
    if !exists {
        return Ok(Vec::new());
    }
    sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success ORDER BY version")
        .fetch_all(pool)
        .await
        .map_err(|e| state_error("read the applied migrations", e))
}

impl StateStore {
    /// Apply the embedded migrations that are not recorded yet; returns how many were applied.
    pub async fn migrate(&self) -> Result<usize> {
        let before = recorded_versions(&self.pg).await?;
        MIGRATOR
            .run(&self.pg)
            .await
            .map_err(|e| state_error("apply migrations", e))?;
        let after = recorded_versions(&self.pg).await?;
        Ok(after.len().saturating_sub(before.len()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::embedded;

    fn sql_files() -> Vec<String> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../migrations");
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".sql"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn every_file_of_the_migrations_folder_is_embedded_in_order() {
        let files = sql_files();
        let versions: Vec<i64> = embedded().iter().map(|(version, _)| *version).collect();

        assert_eq!(versions.len(), files.len(), "{files:?}");
        assert_eq!(versions, (1..=files.len() as i64).collect::<Vec<_>>());
    }

    #[test]
    fn descriptions_come_from_the_file_names() {
        let (version, description) = embedded().remove(0);

        assert_eq!((version, description.as_str()), (1, "init"));
    }
}
