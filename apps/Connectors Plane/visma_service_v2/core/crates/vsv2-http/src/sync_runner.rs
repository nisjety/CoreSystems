use async_trait::async_trait;
use chrono::{DateTime, Utc};
use vsv2_common::TenantId;
use vsv2_orchestrator::SyncExecutor;
pub use vsv2_orchestrator::{EndpointSyncFailure, SyncExecutionSummary};

#[async_trait]
pub trait SyncRunner: Send + Sync {
    async fn sync_all_window(
        &self,
        tenant: &TenantId,
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> anyhow::Result<SyncExecutionSummary>;

    async fn sync_names_window(
        &self,
        tenant: &TenantId,
        names: &[String],
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> anyhow::Result<SyncExecutionSummary>;
}

#[async_trait]
impl SyncRunner for SyncExecutor {
    async fn sync_all_window(
        &self,
        tenant: &TenantId,
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> anyhow::Result<SyncExecutionSummary> {
        SyncExecutor::sync_all_window_detailed(self, tenant, date_from, date_to)
            .await
            .map_err(anyhow::Error::from)
    }

    async fn sync_names_window(
        &self,
        tenant: &TenantId,
        names: &[String],
        date_from: Option<DateTime<Utc>>,
        date_to: Option<DateTime<Utc>>,
    ) -> anyhow::Result<SyncExecutionSummary> {
        SyncExecutor::sync_names_window_detailed(self, tenant, names, date_from, date_to)
            .await
            .map_err(anyhow::Error::from)
    }
}
