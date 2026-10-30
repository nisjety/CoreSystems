use async_trait::async_trait;
use chrono::{DateTime, Utc};
use vsv2_common::{PageFetchMetric, PageSink, Result, TenantId};

use crate::open_set::OpenSetSource;
use crate::partition::PartitionPlan;
use crate::registry::EndpointDefinition;

/// Where a partition's fetch starts and stops, as the checkpoint and the run's window set it.
#[derive(Debug, Clone, Default)]
pub struct FetchWindow {
    /// Only records modified at or after this instant.
    pub watermark: Option<DateTime<Utc>>,
    /// Only records modified at or before this instant (the run's explicit window end).
    pub window_end: Option<DateTime<Utc>>,
    /// Where a cursor-paginated source resumes.
    pub cursor: Option<String>,
}

/// What a partition's fetch leaves behind once its records have gone to the sink.
#[derive(Debug, Clone, Default)]
pub struct FetchPartitionResult {
    pub page_metrics: Vec<PageFetchMetric>,
}

/// Source-agnostic fetch contract. Each client crate implements one adapter.
#[async_trait]
pub trait FetchAdapter: Send + Sync {
    /// Runtime fetch partitions for this endpoint. Most sources have a single
    /// default partition; ERP REST may fan out across tenant-specific query
    /// dimensions such as ledger, branch, and financial year.
    async fn plan_partitions(
        &self,
        _tenant: &TenantId,
        _def: &EndpointDefinition,
    ) -> Result<Vec<PartitionPlan>> {
        Ok(vec![PartitionPlan::default_partition()])
    }

    /// Single endpoint run. Each page of records goes to `sink` as it is fetched, so a run never
    /// holds the whole endpoint in memory.
    async fn fetch_partition(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
        partition: &PartitionPlan,
        window: FetchWindow,
        sink: &mut dyn PageSink,
    ) -> Result<FetchPartitionResult>;

    /// What this source offers for open-set reconciliation (`crate::open_set`): Visma's list of
    /// open documents and a document by key. `None` for a source that has neither.
    fn open_set_source(&self) -> Option<&dyn OpenSetSource> {
        None
    }
}
