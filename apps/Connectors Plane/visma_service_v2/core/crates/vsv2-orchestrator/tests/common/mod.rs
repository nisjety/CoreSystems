//! Shared scaffolding for the sync-semantics integration tests.
//!
//! `Harness` wires a real `SyncExecutor` (real registry loading, real
//! Postgres state store, local-bronze sink) to a scriptable `FakeAdapter`, so
//! tests exercise the production write/checkpoint path with no network access.

#![allow(dead_code)] // each test binary uses a different subset

pub mod fake_adapter;
pub mod test_db;

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;
use vsv2_common::{BackgroundMode, PaginationProfile, PrimaryKey, Result, Source, TenantId};
use vsv2_orchestrator::partition::{PartitionDimensionConfig, PartitioningConfig};
use vsv2_orchestrator::{
    DispatchTable, EndpointDefinition, EndpointRegistry, NoopAdapter, SyncExecutor,
};
use vsv2_state::{Checkpoint, CheckpointKey, DEFAULT_PARTITION_KEY};

use fake_adapter::{FakeAdapter, SharedAdapter};
use test_db::TestDb;

pub const TENANT_A: &str = "tenant-a";
pub const TENANT_B: &str = "tenant-b";

/// An ERP REST endpoint keyed on `accountCD`, incrementally synced on
/// `lastModifiedDateTime` (10-minute overlap, like `accounts` in production).
pub fn erp_def(endpoint: &str) -> EndpointDefinition {
    EndpointDefinition {
        endpoint_name: endpoint.into(),
        bigquery_table: format!("visma_{endpoint}"),
        primary_key: PrimaryKey::Single("accountCD".into()),
        source: Source::ErpRest,
        enabled: true,
        api_method: Some("get_accounts".into()),
        api_endpoint: Some("/v1/account".into()),
        fallback_api_method: None,
        pagination_profile: PaginationProfile::FinancePageNumber,
        page_size: None,
        background_mode: BackgroundMode::Off,
        background_poll_secs: None,
        background_timeout_secs: None,
        default_query: HashMap::new(),
        required_query: vec![],
        modified_field: Some("lastModifiedDateTime".into()),
        merge_type: Some("upsert".into()),
        sync_pattern: Some("incremental_upsert".into()),
        validation_fields: Some(vec!["accountCD".into()]),
        partitioning: None,
        period_scan: None,
        datamart: None,
        tier: None,
        open_set: false,
    }
}

/// Same endpoint, fanned out over ledgers. Records must carry `_partition_key`
/// (the fake adapter stamps it, as the real ERP adapter does).
pub fn partitioned_erp_def(endpoint: &str) -> EndpointDefinition {
    EndpointDefinition {
        partitioning: Some(PartitioningConfig {
            mode: "cartesian".into(),
            dimensions: vec![PartitionDimensionConfig {
                query_param: "ledger".into(),
                source_endpoint: "ledgers".into(),
                source_path: "ledgerId".into(),
                fallback_values: vec![],
                ..Default::default()
            }],
            max_partitions: 10,
            ..Default::default()
        }),
        ..erp_def(endpoint)
    }
}

/// One upstream account row.
pub fn account(code: &str, modified: &str) -> Value {
    json!({
        "accountCD": code,
        "description": format!("account {code}"),
        "lastModifiedDateTime": modified,
    })
}

pub fn utc(rfc3339: &str) -> DateTime<Utc> {
    rfc3339.parse().expect("valid RFC3339 timestamp")
}

#[derive(Debug, Clone)]
pub struct BronzeRow {
    pub record_key: String,
    pub payload: Value,
    pub first_seen_at: DateTime<Utc>,
    pub last_run_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct RunRow {
    pub status: String,
    pub records_in: i64,
    pub records_written: i64,
    pub error: Option<String>,
}

pub struct Harness {
    pub db: TestDb,
    pub fake: Arc<FakeAdapter>,
    pub executor: SyncExecutor,
    pub tenant: TenantId,
    pub endpoint: String,
}

impl Harness {
    pub async fn new(def: EndpointDefinition, fake: Arc<FakeAdapter>) -> Self {
        let db = TestDb::create().await;
        let endpoint = def.endpoint_name.clone();

        // Load through the real registry so the definition round-trips its
        // serde config format, exactly as `config/endpoints/*.json` does.
        let config_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            config_dir.path().join(format!("{endpoint}.json")),
            serde_json::to_string(&def).expect("serialize endpoint definition"),
        )
        .expect("write endpoint config");
        let registry = EndpointRegistry::load_dir(config_dir.path()).expect("load registry");

        let executor = SyncExecutor {
            registry,
            dispatch: DispatchTable {
                erp: Box::new(SharedAdapter(Arc::clone(&fake))),
                datamart: Box::new(NoopAdapter {
                    source_name: "datamart",
                }),
                expense: Box::new(NoopAdapter {
                    source_name: "expense",
                }),
            },
            state: db.state.clone(),
            bq: None,
            local_bronze_enabled: true,
        };

        Self {
            db,
            fake,
            executor,
            tenant: TenantId::new(TENANT_A),
            endpoint,
        }
    }

    pub async fn sync(&self) -> Result<u64> {
        self.executor.sync_one(&self.tenant, &self.endpoint).await
    }

    pub async fn sync_tenant(&self, tenant: &str) -> Result<u64> {
        self.executor
            .sync_one(&TenantId::new(tenant), &self.endpoint)
            .await
    }

    pub async fn sync_window(
        &self,
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> Result<u64> {
        self.executor
            .sync_one_window(&self.tenant, &self.endpoint, date_from, date_to)
            .await
    }

    /// Bronze rows for `tenant`, ordered by key.
    pub async fn bronze_for(&self, tenant: &str) -> Vec<BronzeRow> {
        sqlx::query(
            "SELECT record_key, payload, first_seen_at, last_run_id \
             FROM vsv2.local_bronze_record \
             WHERE tenant_id = $1 AND endpoint = $2 ORDER BY record_key",
        )
        .bind(tenant)
        .bind(&self.endpoint)
        .fetch_all(&self.db.state.pg)
        .await
        .expect("query bronze")
        .into_iter()
        .map(|row| BronzeRow {
            record_key: row.get("record_key"),
            payload: row.get("payload"),
            first_seen_at: row.get("first_seen_at"),
            last_run_id: row.get("last_run_id"),
        })
        .collect()
    }

    pub async fn bronze(&self) -> Vec<BronzeRow> {
        self.bronze_for(TENANT_A).await
    }

    /// Runs for the default tenant, oldest first.
    pub async fn runs(&self) -> Vec<RunRow> {
        sqlx::query(
            "SELECT status, records_in, records_written, error \
             FROM vsv2.sync_run \
             WHERE tenant_id = $1 AND endpoint = $2 ORDER BY started_at, created_at",
        )
        .bind(TENANT_A)
        .bind(&self.endpoint)
        .fetch_all(&self.db.state.pg)
        .await
        .expect("query runs")
        .into_iter()
        .map(|row| RunRow {
            status: row.get("status"),
            records_in: row.get("records_in"),
            records_written: row.get("records_written"),
            error: row.get("error"),
        })
        .collect()
    }

    pub async fn checkpoint_for(&self, tenant: &str, partition: &str) -> Option<Checkpoint> {
        let key = CheckpointKey::for_partition(
            TenantId::new(tenant),
            self.endpoint.clone(),
            Source::ErpRest,
            partition.to_string(),
        );
        self.db
            .state
            .get_checkpoint(&key)
            .await
            .expect("read checkpoint")
    }

    pub async fn checkpoint(&self) -> Option<Checkpoint> {
        self.checkpoint_for(TENANT_A, DEFAULT_PARTITION_KEY).await
    }

    pub async fn checkpoint_partition(&self, partition: &str) -> Option<Checkpoint> {
        self.checkpoint_for(TENANT_A, partition).await
    }

    pub async fn checkpoint_row_count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM vsv2.sync_checkpoint WHERE endpoint = $1")
            .bind(&self.endpoint)
            .fetch_one(&self.db.state.pg)
            .await
            .expect("count checkpoints")
    }

    /// Page-metric rows recorded for failed fetches: `(status, retry_after_secs)`.
    pub async fn page_metrics(&self) -> Vec<(i32, Option<i64>)> {
        sqlx::query(
            "SELECT status, retry_after_secs FROM vsv2.sync_page_metric \
             WHERE endpoint = $1 ORDER BY metric_id",
        )
        .bind(&self.endpoint)
        .fetch_all(&self.db.state.pg)
        .await
        .expect("query page metrics")
        .into_iter()
        .map(|row| (row.get("status"), row.get("retry_after_secs")))
        .collect()
    }
}

/// A checkpoint as comparable JSON, for "did anything change?" assertions.
pub fn snapshot(checkpoint: &Option<Checkpoint>) -> Value {
    serde_json::to_value(checkpoint).expect("serialize checkpoint")
}
