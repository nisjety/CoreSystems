use async_trait::async_trait;
use vsv2_common::{Error, PageSink, Result, TenantId};
use vsv2_expense::{paginate_with_metrics, ExpenseClient, ExpensePageOpts};

use crate::adapter::{FetchAdapter, FetchPartitionResult, FetchWindow};
use crate::partition::PartitionPlan;
use crate::registry::EndpointDefinition;

pub struct ExpenseAdapter {
    pub client: ExpenseClient,
    pub page_size: u32,
}

#[async_trait]
impl FetchAdapter for ExpenseAdapter {
    async fn fetch_partition(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
        _partition: &PartitionPlan,
        window: FetchWindow,
        sink: &mut dyn PageSink,
    ) -> Result<FetchPartitionResult> {
        let path = def.api_method.as_deref().ok_or_else(|| {
            Error::Other(format!(
                "endpoint '{}' has no api_method",
                def.endpoint_name
            ))
        })?;
        let opts = ExpensePageOpts {
            page_size: self.page_size,
            approval_from: window.watermark,
            approval_to: window.window_end,
            ..Default::default()
        };
        // The expense API is walked whole, so its records are handed over as one page.
        let batch = paginate_with_metrics(&self.client, tenant, path, &opts).await?;
        sink.accept(batch.records)?;
        Ok(FetchPartitionResult {
            page_metrics: batch.page_metrics,
        })
    }
}
