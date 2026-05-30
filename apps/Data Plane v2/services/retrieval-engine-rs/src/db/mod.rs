use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn create_pool(database_url: &str) -> anyhow::Result<PgPool> {
    let max_conns: u32 = std::env::var("PG_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let min_conns: u32 = std::env::var("PG_MIN_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let acquire_timeout_secs: u64 = std::env::var("PG_ACQUIRE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    let pool = PgPoolOptions::new()
        .max_connections(max_conns)
        .min_connections(min_conns)
        .acquire_timeout(std::time::Duration::from_secs(acquire_timeout_secs))
        .connect(database_url)
        .await?;
    tracing::info!(max_conns, min_conns, "postgres pool connected");

    // §16.3.6 warmup — eagerly drive the pool up to `min_conns` so the first
    // user-facing queries don't pay the TCP/TLS connect cost after boot.
    // `connect()` only seeded one connection; we fan out parallel `SELECT 1`s
    // up to min_conns to force the pool to materialize the rest.
    warmup_pool(&pool, min_conns).await;

    // §16.2.7 pool-saturation gauges — start a background task that polls
    // sqlx's `pool.size()` / `pool.num_idle()` every 5s and publishes them as
    // Prometheus gauges. Alerts at 80% of max_conns fire from there.
    spawn_pool_metrics(pool.clone(), max_conns);

    Ok(pool)
}

async fn warmup_pool(pool: &PgPool, min_conns: u32) {
    if min_conns <= 1 {
        return;
    }
    let mut handles = Vec::with_capacity(min_conns as usize);
    for _ in 0..min_conns {
        let p = pool.clone();
        handles.push(tokio::spawn(async move {
            let _ = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&p).await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    tracing::info!(min_conns, "postgres pool warmed up");
}

fn spawn_pool_metrics(pool: PgPool, max_conns: u32) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            ticker.tick().await;
            let size = pool.size() as f64;
            let idle = pool.num_idle() as f64;
            let active = (size - idle).max(0.0);
            metrics::gauge!("dpv2_postgres_pool_size").set(size);
            metrics::gauge!("dpv2_postgres_pool_idle").set(idle);
            metrics::gauge!("dpv2_postgres_pool_active").set(active);
            metrics::gauge!("dpv2_postgres_pool_max").set(max_conns as f64);
            if max_conns > 0 {
                let saturation = active / max_conns as f64;
                metrics::gauge!("dpv2_postgres_pool_saturation").set(saturation);
            }
        }
    });
}
