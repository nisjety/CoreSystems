use async_trait::async_trait;
use vsv2_common::{Error, PageSink, Result, TenantId};

use crate::adapter::{FetchAdapter, FetchPartitionResult, FetchWindow};
use crate::partition::PartitionPlan;
use crate::registry::EndpointDefinition;

/// Stand-in adapter for sources not yet implemented (expense API, Phase 7).
pub struct NoopAdapter {
    pub source_name: &'static str,
}

#[async_trait]
impl FetchAdapter for NoopAdapter {
    async fn fetch_partition(
        &self,
        _tenant: &TenantId,
        _def: &EndpointDefinition,
        _partition: &PartitionPlan,
        _window: FetchWindow,
        _sink: &mut dyn PageSink,
    ) -> Result<FetchPartitionResult> {
        Err(Error::Other(format!(
            "{} adapter not configured — set the required credentials env vars",
            self.source_name
        )))
    }
}
