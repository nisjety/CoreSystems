//! One throwaway Postgres database per test.
//!
//! Each test creates `vsv2_test_<uuid>` on the local compose Postgres, applies
//! the real `migrations/*.sql`, and force-drops the database again on `Drop`.
//! The live `visma_v2` database is never touched, so these tests are safe to
//! run next to the local dev stack.

use std::path::PathBuf;

use sqlx::postgres::PgPool;
use uuid::Uuid;
use vsv2_state::StateStore;

const DEFAULT_ADMIN_URL: &str = "postgres://visma_v2:visma_v2@127.0.0.1:55433/postgres";
/// Redis is never contacted by the sync path; the pool is created lazily.
const UNUSED_REDIS_URL: &str = "redis://127.0.0.1:6379";

pub struct TestDb {
    pub state: StateStore,
    name: String,
    admin_url: String,
}

impl TestDb {
    /// A database whose migrations were applied by hand, as `make migrate` does.
    pub async fn create() -> Self {
        let db = Self::create_empty().await;
        apply_migrations(&db.state.pg).await;
        db
    }

    /// A database with nothing in it: the state of a fresh Postgres volume without the init scripts.
    pub async fn create_empty() -> Self {
        let admin_url = std::env::var("VSV2_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| DEFAULT_ADMIN_URL.to_string());
        let name = format!("vsv2_test_{}", Uuid::new_v4().simple());

        let admin = PgPool::connect(&admin_url).await.unwrap_or_else(|e| {
            panic!(
                "cannot reach the test Postgres at {admin_url}: {e}\n\
                 Start it with `make up`, or point VSV2_TEST_ADMIN_DATABASE_URL at a \
                 Postgres where the user may CREATE DATABASE."
            )
        });
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .unwrap_or_else(|e| panic!("create test database {name}: {e}"));
        admin.close().await;

        let test_url = with_database(&admin_url, &name);
        let state = StateStore::connect(&test_url, UNUSED_REDIS_URL)
            .await
            .unwrap_or_else(|e| panic!("connect to test database {name}: {e}"));

        Self {
            state,
            name,
            admin_url,
        }
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

async fn apply_migrations(pool: &PgPool) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../migrations");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read migrations dir {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("sql"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no migrations found in {}",
        dir.display()
    );

    for file in files {
        let sql = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("read migration {}: {e}", file.display()));
        sqlx::raw_sql(&sql)
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("apply migration {}: {e}", file.display()));
    }
}
