//! `vsv2-state` — Postgres + Redis state store.
//!
//! **Separate database** from v1 per locked decision (`visma_v2`).

pub mod checkpoint;
pub mod endpoint_flags;
pub mod lease;
pub mod migrate;
pub mod monitor;
pub mod page_metric;
pub mod run_history;
pub mod scheduler_pass;
pub mod tenant_company;

pub use checkpoint::{Checkpoint, CheckpointKey, DEFAULT_PARTITION_KEY};
pub use lease::Lease;
pub use migrate::embedded as embedded_migrations;
pub use page_metric::SyncPageMetric;
pub use run_history::{RunStatus, SyncRun};
pub use tenant_company::{Registration, TenantCompanyMap};

use chrono::DateTime;
use chrono::Utc;
use deadpool_redis::{Config as RedisConfig, Pool as RedisPool, Runtime};
use serde_json::{Map, Value};
use sqlx::postgres::PgPool;
use sqlx::Row;
use std::collections::HashSet;
use tracing::debug;
use uuid::Uuid;

use vsv2_common::{Error, Result, Source, TenantConfig, TenantId};

/// A run still marked `running` after this long belonged to a process that died (the slowest
/// full sync measured takes minutes). See [`StateStore::abandon_stale_runs`].
pub const STALE_RUN_AFTER: chrono::Duration = chrono::Duration::hours(6);

/// Value migration 0001 seeds `tenant_company_map.company_id` with until discovery has run.
const COMPANY_ID_PLACEHOLDER: &str = "PENDING_PHASE_0_DISCOVERY";

/// Watermarks only ever move forward (`GREATEST` ignores NULLs). Runs are not
/// mutually exclusive — HTTP backfills take no lease — so a stale run that
/// finishes last, or a bounded backfill capped at an old `date_to`, must not
/// rewind the checkpoint and trigger a large re-fetch. Rewinding on purpose
/// means deleting the row.
const UPSERT_CHECKPOINT_SQL: &str = "INSERT INTO vsv2.sync_checkpoint \
     (tenant_id, endpoint, source, last_modified_watermark, last_updated_in_dm, \
      last_id, last_sync_at, records_seen, partition_key, updated_at) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW()) \
     ON CONFLICT (tenant_id, endpoint, source, partition_key) DO UPDATE SET \
     last_modified_watermark = GREATEST(vsv2.sync_checkpoint.last_modified_watermark, \
                                        EXCLUDED.last_modified_watermark), \
     last_updated_in_dm      = GREATEST(vsv2.sync_checkpoint.last_updated_in_dm, \
                                        EXCLUDED.last_updated_in_dm), \
     last_id                 = EXCLUDED.last_id, \
     last_sync_at            = EXCLUDED.last_sync_at, \
     records_seen            = EXCLUDED.records_seen, \
     updated_at              = NOW()";

/// Postgres + Redis pool handle.
#[derive(Debug, Clone)]
pub struct StateStore {
    pub pg: PgPool,
    pub redis: RedisPool,
}

impl StateStore {
    pub async fn connect(pg_dsn: &str, redis_url: &str) -> Result<Self> {
        let pg = sqlx::postgres::PgPoolOptions::new()
            .max_connections(8)
            .connect(pg_dsn)
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;

        let redis = RedisConfig::from_url(redis_url)
            .create_pool(Some(Runtime::Tokio1))
            .map_err(|e| Error::StateStore(e.to_string()))?;

        debug!("state store connected");
        Ok(Self { pg, redis })
    }

    pub async fn get_checkpoint(&self, key: &CheckpointKey) -> Result<Option<Checkpoint>> {
        let row = sqlx::query(
            "SELECT last_modified_watermark, last_updated_in_dm, last_id, \
             last_sync_at, records_seen \
             FROM vsv2.sync_checkpoint \
             WHERE tenant_id = $1 AND endpoint = $2 AND source = $3 AND partition_key = $4",
        )
        .bind(key.tenant.as_str())
        .bind(&key.endpoint)
        .bind(key.source.as_str())
        .bind(&key.partition_key)
        .fetch_optional(&self.pg)
        .await
        .map_err(|e| Error::StateStore(e.to_string()))?;

        Ok(row.map(|r| Checkpoint {
            key: key.clone(),
            last_modified_watermark: r
                .try_get::<Option<DateTime<Utc>>, _>("last_modified_watermark")
                .ok()
                .flatten(),
            last_updated_in_dm: r
                .try_get::<Option<DateTime<Utc>>, _>("last_updated_in_dm")
                .ok()
                .flatten(),
            last_id: r.try_get::<Option<String>, _>("last_id").ok().flatten(),
            last_sync_at: r
                .try_get::<Option<DateTime<Utc>>, _>("last_sync_at")
                .ok()
                .flatten(),
            records_seen: r.try_get::<i64, _>("records_seen").unwrap_or(0),
        }))
    }

    pub async fn upsert_checkpoint(&self, cp: &Checkpoint) -> Result<()> {
        sqlx::query(UPSERT_CHECKPOINT_SQL)
            .bind(cp.key.tenant.as_str())
            .bind(&cp.key.endpoint)
            .bind(cp.key.source.as_str())
            .bind(cp.last_modified_watermark)
            .bind(cp.last_updated_in_dm)
            .bind(&cp.last_id)
            .bind(cp.last_sync_at)
            .bind(cp.records_seen)
            .bind(&cp.key.partition_key)
            .execute(&self.pg)
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;
        Ok(())
    }

    pub async fn upsert_checkpoints(&self, checkpoints: &[Checkpoint]) -> Result<()> {
        if checkpoints.is_empty() {
            return Ok(());
        }

        let mut tx = self
            .pg
            .begin()
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;

        for cp in checkpoints {
            sqlx::query(UPSERT_CHECKPOINT_SQL)
                .bind(cp.key.tenant.as_str())
                .bind(&cp.key.endpoint)
                .bind(cp.key.source.as_str())
                .bind(cp.last_modified_watermark)
                .bind(cp.last_updated_in_dm)
                .bind(&cp.last_id)
                .bind(cp.last_sync_at)
                .bind(cp.records_seen)
                .bind(&cp.key.partition_key)
                .execute(&mut *tx)
                .await
                .map_err(|e| Error::StateStore(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;
        Ok(())
    }

    pub async fn insert_page_metrics(&self, metrics: &[SyncPageMetric]) -> Result<()> {
        if metrics.is_empty() {
            return Ok(());
        }

        let mut tx = self
            .pg
            .begin()
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;

        for metric in metrics {
            sqlx::query(
                "INSERT INTO vsv2.sync_page_metric \
                 (run_id, tenant_id, endpoint, source, partition_key, upstream_path, \
                  page_param, page_value, page_size, status, elapsed_ms, records, \
                  total_count, has_next, rate_limit_limit, rate_limit_remaining, \
                  rate_limit_reset, rate_limit_policy, retry_after_secs, background_mode, fetched_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, \
                         $13, $14, $15, $16, $17, $18, $19, $20, $21)",
            )
            .bind(metric.run_id)
            .bind(metric.tenant.as_str())
            .bind(&metric.endpoint)
            .bind(metric.source.as_str())
            .bind(&metric.partition_key)
            .bind(&metric.upstream_path)
            .bind(&metric.page_param)
            .bind(metric.page_value)
            .bind(metric.page_size)
            .bind(metric.status)
            .bind(metric.elapsed_ms)
            .bind(metric.records)
            .bind(metric.total_count)
            .bind(metric.has_next)
            .bind(metric.rate_limit_limit)
            .bind(metric.rate_limit_remaining)
            .bind(metric.rate_limit_reset)
            .bind(&metric.rate_limit_policy)
            .bind(metric.retry_after_secs)
            .bind(&metric.background_mode)
            .bind(metric.fetched_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;
        Ok(())
    }

    pub async fn upsert_company_id(&self, tenant: &TenantId, company_id: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO vsv2.tenant_company_map (tenant_id, company_id, updated_at) \
             VALUES ($1, $2, NOW()) \
             ON CONFLICT (tenant_id) DO UPDATE SET \
             company_id = EXCLUDED.company_id, updated_at = NOW()",
        )
        .bind(tenant.as_str())
        .bind(company_id)
        .execute(&self.pg)
        .await
        .map_err(|e| Error::StateStore(e.to_string()))?;
        Ok(())
    }

    /// What registering `tenant` would do, without doing it.
    pub async fn plan_tenant(&self, tenant: &TenantConfig) -> Result<Registration> {
        let Some(company_id) = &tenant.datamart_company_id else {
            return Ok(Registration::SkippedNoCompanyId);
        };
        let row = sqlx::query(
            "SELECT company_id, company_name FROM vsv2.tenant_company_map WHERE tenant_id = $1",
        )
        .bind(&tenant.tenant_id)
        .fetch_optional(&self.pg)
        .await
        .map_err(|e| Error::StateStore(e.to_string()))?;
        Ok(match row {
            None => Registration::Created,
            Some(row) => {
                let company: String = row.try_get("company_id").unwrap_or_default();
                let name: Option<String> = row.try_get("company_name").ok().flatten();
                if &company == company_id && name.as_deref() == Some(tenant.tenant_name.as_str()) {
                    Registration::Unchanged
                } else {
                    Registration::Updated
                }
            }
        })
    }

    /// Record `tenant`'s DataMart company so a sync of that tenant can query it.
    ///
    /// Idempotent. A tenant whose config has no company id yet is left alone: overwriting its row
    /// (the pilot's placeholder) with nothing would only lose information.
    pub async fn register_tenant(&self, tenant: &TenantConfig) -> Result<Registration> {
        let outcome = self.plan_tenant(tenant).await?;
        if let (Registration::Created | Registration::Updated, Some(company_id)) =
            (outcome, &tenant.datamart_company_id)
        {
            sqlx::query(
                "INSERT INTO vsv2.tenant_company_map (tenant_id, company_id, company_name, updated_at) \
                 VALUES ($1, $2, $3, NOW()) \
                 ON CONFLICT (tenant_id) DO UPDATE SET \
                 company_id = EXCLUDED.company_id, company_name = EXCLUDED.company_name, \
                 updated_at = NOW()",
            )
            .bind(&tenant.tenant_id)
            .bind(company_id)
            .bind(&tenant.tenant_name)
            .execute(&self.pg)
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;
        }
        Ok(outcome)
    }

    /// Every row of the tenant-to-company map, by name.
    pub async fn list_tenant_companies(&self) -> Result<Vec<TenantCompanyMap>> {
        let rows = sqlx::query(
            "SELECT tenant_id, company_id, company_name FROM vsv2.tenant_company_map \
             ORDER BY company_name NULLS LAST, tenant_id",
        )
        .fetch_all(&self.pg)
        .await
        .map_err(|e| Error::StateStore(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| TenantCompanyMap {
                tenant: TenantId::new(row.try_get::<String, _>("tenant_id").unwrap_or_default()),
                company_id: row.try_get("company_id").unwrap_or_default(),
                company_name: row.try_get("company_name").ok().flatten(),
            })
            .collect())
    }

    /// The DataMart company id for `tenant`, or `None` if none has been configured.
    ///
    /// Migration 0001 seeds the pilot tenant with [`COMPANY_ID_PLACEHOLDER`]. That is "not
    /// configured yet", not an id, so it is reported as absent: the caller then fails with
    /// its "set VSV2_AQUATIQ_DATAMART_COMPANY_ID" message instead of querying DataMart for a
    /// company that does not exist.
    pub async fn get_company_id(&self, tenant: &TenantId) -> Result<Option<String>> {
        let row =
            sqlx::query("SELECT company_id FROM vsv2.tenant_company_map WHERE tenant_id = $1")
                .bind(tenant.as_str())
                .fetch_optional(&self.pg)
                .await
                .map_err(|e| Error::StateStore(e.to_string()))?;

        Ok(row
            .and_then(|r| r.try_get::<String, _>("company_id").ok())
            .filter(|id| id != COMPANY_ID_PLACEHOLDER))
    }

    pub async fn insert_run(&self, run: &SyncRun) -> Result<()> {
        sqlx::query(
            "INSERT INTO vsv2.sync_run \
             (run_id, tenant_id, endpoint, source, status, started_at, \
              records_in, records_written, error) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(run.run_id)
        .bind(run.tenant.as_str())
        .bind(&run.endpoint)
        .bind(run.source.as_str())
        .bind(run.status.as_str())
        .bind(run.started_at)
        .bind(run.records_in)
        .bind(run.records_written)
        .bind(run.error.as_deref().map(vsv2_common::text::storable_text))
        .execute(&self.pg)
        .await
        .map_err(|e| Error::StateStore(e.to_string()))?;
        Ok(())
    }

    pub async fn finish_run(
        &self,
        run_id: Uuid,
        status: RunStatus,
        records_in: i64,
        records_written: i64,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE vsv2.sync_run \
             SET status = $2, finished_at = NOW(), records_in = $3, \
                 records_written = $4, error = $5 \
             WHERE run_id = $1",
        )
        .bind(run_id)
        .bind(status.as_str())
        .bind(records_in)
        .bind(records_written)
        .bind(error.map(vsv2_common::text::storable_text))
        .execute(&self.pg)
        .await
        .map_err(|e| Error::StateStore(e.to_string()))?;
        Ok(())
    }

    /// Mark `running` runs older than `older_than` as failed and return how many there were.
    ///
    /// Only the process that owns a run can finish it, so a `kill -9`, an OOM kill or a crashed
    /// container leaves the row `running` for good. Call this at process start with a threshold
    /// longer than any legitimate run; runs that are recent, or already finished, are untouched.
    pub async fn abandon_stale_runs(&self, older_than: chrono::Duration) -> Result<u64> {
        let result = sqlx::query(
            "UPDATE vsv2.sync_run \
             SET status = 'failed', finished_at = NOW(), \
                 error = COALESCE(error || '; ', '') || \
                         'abandoned: the process ended without finishing this run \
                          (still marked running after ' || $1::bigint || 's)' \
             WHERE status = 'running' \
               AND started_at < NOW() - make_interval(secs => $1::double precision)",
        )
        .bind(older_than.num_seconds())
        .execute(&self.pg)
        .await
        .map_err(|e| Error::StateStore(e.to_string()))?;
        Ok(result.rows_affected())
    }

    /// Upsert endpoint payloads into local Postgres bronze storage.
    ///
    /// This is intended for local dry-run validation when BigQuery is not
    /// configured. One row per logical key is kept and refreshed on re-sync.
    pub async fn upsert_local_bronze_records(
        &self,
        run_id: Uuid,
        tenant: &TenantId,
        endpoint: &str,
        source: Source,
        records: &[Value],
        primary_key: &str,
    ) -> Result<u64> {
        if records.is_empty() {
            return Ok(0);
        }

        let pk_fields = parse_primary_key_fields(primary_key)?;
        let seen_at = Utc::now();
        let mut seen_keys = HashSet::with_capacity(records.len());
        let mut tx = self
            .pg
            .begin()
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;

        for record in records {
            let (record_key, record_key_json) = record_key_from_record(record, &pk_fields)
                .map_err(|msg| Error::Config(format!("{endpoint}: {msg}")))?;

            if !seen_keys.insert(record_key.clone()) {
                return Err(Error::Config(format!(
                    "{endpoint}: duplicate primary key within sync batch: {record_key}"
                )));
            }

            sqlx::query(
                "INSERT INTO vsv2.local_bronze_record \
                 (tenant_id, endpoint, source, record_key, record_key_json, payload, \
                  first_seen_at, last_seen_at, last_run_id, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $7, $8, NOW()) \
                 ON CONFLICT (tenant_id, endpoint, source, record_key) DO UPDATE SET \
                 record_key_json = EXCLUDED.record_key_json, \
                 payload = EXCLUDED.payload, \
                 last_seen_at = EXCLUDED.last_seen_at, \
                 last_run_id = EXCLUDED.last_run_id, \
                 updated_at = NOW()",
            )
            .bind(tenant.as_str())
            .bind(endpoint)
            .bind(source.as_str())
            .bind(&record_key)
            .bind(&*vsv2_common::text::storable_json(&record_key_json))
            .bind(&*vsv2_common::text::storable_json(record))
            .bind(seen_at)
            .bind(run_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;
        Ok(records.len() as u64)
    }

    /// Acquire a Redis-based per-tenant mutex. Returns `true` if the lock was
    /// obtained, `false` if another owner already holds it.
    ///
    /// The lock expires automatically after `ttl_secs` even if `release_lease`
    /// is never called (e.g. process crash).
    pub async fn try_acquire_lease(
        &self,
        tenant: &TenantId,
        owner_id: &str,
        ttl_secs: u64,
    ) -> Result<bool> {
        use deadpool_redis::redis::cmd;
        let key = Lease::redis_key(tenant);
        let mut conn = self
            .redis
            .get()
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;
        let result: Option<String> = cmd("SET")
            .arg(&key)
            .arg(owner_id)
            .arg("NX")
            .arg("PX")
            .arg(ttl_secs * 1_000)
            .query_async(&mut *conn)
            .await
            .map_err(|e: deadpool_redis::redis::RedisError| Error::StateStore(e.to_string()))?;
        Ok(result.is_some())
    }

    /// Push the expiry of a lease this owner holds out to `ttl_secs` from now. `false` when the
    /// lease is gone or held by someone else (nothing is changed then).
    ///
    /// A holder renews while it works and keeps the TTL short, so a process that dies frees its
    /// tenant within minutes instead of holding it for the whole TTL.
    pub async fn renew_lease(
        &self,
        tenant: &TenantId,
        owner_id: &str,
        ttl_secs: u64,
    ) -> Result<bool> {
        use deadpool_redis::redis::cmd;
        let mut conn = self
            .redis
            .get()
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;
        let renewed: i64 = cmd("EVAL")
            .arg(
                "if redis.call('GET', KEYS[1]) == ARGV[1] \
                 then return redis.call('PEXPIRE', KEYS[1], ARGV[2]) else return 0 end",
            )
            .arg(1)
            .arg(Lease::redis_key(tenant))
            .arg(owner_id)
            .arg(ttl_secs * 1_000)
            .query_async(&mut *conn)
            .await
            .map_err(|e: deadpool_redis::redis::RedisError| Error::StateStore(e.to_string()))?;
        Ok(renewed == 1)
    }

    /// Release the per-tenant mutex. Best-effort — errors are logged but not
    /// propagated so the caller's main result is unaffected.
    pub async fn release_lease(&self, tenant: &TenantId) -> Result<()> {
        use deadpool_redis::redis::cmd;
        let key = Lease::redis_key(tenant);
        let mut conn = self
            .redis
            .get()
            .await
            .map_err(|e| Error::StateStore(e.to_string()))?;
        let _: () = cmd("DEL")
            .arg(&key)
            .query_async(&mut *conn)
            .await
            .map_err(|e: deadpool_redis::redis::RedisError| Error::StateStore(e.to_string()))?;
        Ok(())
    }
}

fn parse_primary_key_fields(primary_key: &str) -> Result<Vec<String>> {
    let fields = primary_key
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if fields.is_empty() {
        return Err(Error::Config(
            "primary key is empty; cannot write local bronze".into(),
        ));
    }

    Ok(fields)
}

fn record_key_from_record(
    record: &Value,
    fields: &[String],
) -> std::result::Result<(String, Value), String> {
    record
        .as_object()
        .ok_or_else(|| "record payload is not a JSON object".to_string())?;
    let mut key_obj = Map::with_capacity(fields.len());

    for field in fields {
        let Some(value) = get_nested_value(record, field) else {
            return Err(format!("missing primary key field '{field}'"));
        };
        if value.is_null() {
            return Err(format!("primary key field '{field}' is null"));
        }
        key_obj.insert(field.clone(), value.clone());
    }

    let key_json = Value::Object(key_obj);
    let key_text = serde_json::to_string(&key_json)
        .map_err(|e| format!("failed to serialize primary key object: {e}"))?;
    Ok((key_text, key_json))
}

fn get_nested_value<'a>(record: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = record;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

/// Connect a bare Postgres pool for ad-hoc CLI use (Phase 2 flag command).
pub async fn connect_pool(dsn: &str) -> Result<PgPool> {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(dsn)
        .await
        .map_err(|e| Error::StateStore(e.to_string()))
}
