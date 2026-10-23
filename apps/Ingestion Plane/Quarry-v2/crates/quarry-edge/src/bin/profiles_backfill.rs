//! Operator backfill for profile envelopes (WS17.E3).
//!
//! Seals every plaintext profile row under the current master key, then unlinks
//! the legacy plaintext cache keys. Run it once per environment, after
//! `QUARRY_EDGE__PROFILE_ENCRYPTION=on` is live fleet-wide and before the
//! release that sets `QUARRY_EDGE__PROFILE_ENCRYPTION_REQUIRED=1`. It is safe
//! while the fleet is live and can be repeated.
//!
//! Environment (the same names the edge reads):
//!   QUARRY_EDGE__DATABASE_URL          Postgres DSN of the profile table (required)
//!   QUARRY_PROFILE_MASTER_KEYS         JSON object of base64 32-byte master keys (required)
//!   QUARRY_PROFILE_MASTER_KEY_ACTIVE   id of the master key used for new wraps (required)
//!   QUARRY_EDGE__REDIS_URL             Redis or Dragonfly, for the legacy key unlink (optional)
//!
//! Flags: `--batch N` (default 200), `--pause-ms N` (default 50), `--max-rows N`.
//!
//! Output: one JSON line of counts. It never prints a key, an id, a URL or a
//! payload, and an error prints its class only.

#[cfg(feature = "postgres-queue")]
fn main() -> std::process::ExitCode {
    use std::time::Duration;

    use quarry_runtime::postgres_profile_store::{PostgresProfileBlobStore, PostgresProfileStore};
    use quarry_runtime::profile_backfill::{run_backfill, BackfillOptions};
    use quarry_runtime::profile_keys::EnvKeyProvider;

    fn usage() -> std::process::ExitCode {
        eprintln!("usage: profiles_backfill [--batch N] [--pause-ms N] [--max-rows N]");
        std::process::ExitCode::from(2)
    }

    let mut opts = BackfillOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let Some(value) = args.next() else {
            return usage();
        };
        match flag.as_str() {
            "--batch" => match value.parse::<usize>() {
                Ok(n) => opts.batch = n,
                Err(_) => return usage(),
            },
            "--pause-ms" => match value.parse::<u64>() {
                Ok(ms) => opts.pause_between_batches = Duration::from_millis(ms),
                Err(_) => return usage(),
            },
            "--max-rows" => match value.parse::<u64>() {
                Ok(n) => opts.max_rows = Some(n),
                Err(_) => return usage(),
            },
            _ => return usage(),
        }
    }

    let Some(dsn) = std::env::var("QUARRY_EDGE__DATABASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
    else {
        eprintln!("QUARRY_EDGE__DATABASE_URL is required");
        return std::process::ExitCode::from(2);
    };
    let master = std::env::var("QUARRY_PROFILE_MASTER_KEYS").unwrap_or_default();
    let active = std::env::var("QUARRY_PROFILE_MASTER_KEY_ACTIVE").unwrap_or_default();
    let keys = match EnvKeyProvider::from_config(&master, &active) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("master key configuration refused: {}", e.class());
            return std::process::ExitCode::from(2);
        }
    };
    let redis_url = std::env::var("QUARRY_EDGE__REDIS_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => {
            eprintln!("could not start the async runtime");
            return std::process::ExitCode::FAILURE;
        }
    };
    runtime.block_on(async move {
        let pool = match sqlx::postgres::PgPool::connect(&dsn).await {
            Ok(pool) => pool,
            Err(_) => {
                eprintln!("database connection failed");
                return std::process::ExitCode::FAILURE;
            }
        };
        let blobs = PostgresProfileBlobStore::new(PostgresProfileStore::new(pool));
        let redis = match redis_url {
            None => None,
            Some(url) => {
                let connected = match redis::Client::open(url.as_str()) {
                    Ok(client) => redis::aio::ConnectionManager::new(client).await.ok(),
                    Err(_) => None,
                };
                if connected.is_none() {
                    eprintln!("redis connection failed; the legacy key unlink is skipped");
                }
                connected
            }
        };
        match run_backfill(&blobs, &keys, redis, opts).await {
            Ok(report) => {
                println!(
                    "{}",
                    serde_json::json!({
                        "scanned": report.scanned,
                        "sealed": report.sealed,
                        "moved_on": report.moved_on,
                        "failed": report.failed,
                        "legacy_cache_keys_unlinked": report.legacy_cache_keys_unlinked,
                    })
                );
                if report.failed == 0 {
                    std::process::ExitCode::SUCCESS
                } else {
                    std::process::ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("backfill stopped: {}", e.code.wire_name());
                std::process::ExitCode::FAILURE
            }
        }
    })
}

#[cfg(not(feature = "postgres-queue"))]
fn main() -> std::process::ExitCode {
    eprintln!("profiles_backfill needs a build with --features postgres-queue");
    std::process::ExitCode::from(2)
}
