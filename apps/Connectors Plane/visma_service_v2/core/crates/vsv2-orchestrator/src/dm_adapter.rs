use async_trait::async_trait;
use vsv2_common::{Error, PageSink, Result, TenantId};
use vsv2_datamart::{paginate_all_stream, CursorPaginator, DatamartClient};
use vsv2_state::StateStore;

use crate::adapter::{FetchAdapter, FetchPartitionResult, FetchWindow};
use crate::partition::PartitionPlan;
use crate::registry::EndpointDefinition;

pub struct DatamartAdapter {
    pub client: DatamartClient,
    pub state: StateStore,
    pub page_size: u32,
}

#[async_trait]
impl FetchAdapter for DatamartAdapter {
    async fn fetch_partition(
        &self,
        tenant: &TenantId,
        def: &EndpointDefinition,
        _partition: &PartitionPlan,
        window: FetchWindow,
        sink: &mut dyn PageSink,
    ) -> Result<FetchPartitionResult> {
        let dm_cfg = def.datamart.as_ref().ok_or_else(|| {
            Error::Other(format!(
                "endpoint '{}' has no datamart config",
                def.endpoint_name
            ))
        })?;

        let company_id: Option<String> = if dm_cfg.company_filter {
            Some(self.state.get_company_id(tenant).await?.ok_or_else(|| {
                Error::Other(format!(
                    "no company_id for tenant {tenant} — run `vsv2 dm-companies` \
                     and set VSV2_AQUATIQ_DATAMART_COMPANY_ID"
                ))
            })?)
        } else {
            None
        };

        let query_file = dm_cfg.query_file.as_deref().ok_or_else(|| {
            Error::Other(format!(
                "endpoint '{}' is routed to datamart but has no datamart.query_file",
                def.endpoint_name
            ))
        })?;
        let root_field = dm_cfg.root_field.as_deref().unwrap_or(&def.endpoint_name);
        let query = std::fs::read_to_string(query_file)?;
        let mut pager = CursorPaginator {
            page_size: self.page_size,
            last_id: window.cursor,
            updated_since: window.watermark,
            ..Default::default()
        };

        let walk = paginate_all_stream(
            &self.client,
            tenant,
            company_id.as_deref(),
            &query,
            root_field,
            &mut pager,
            sink,
        )
        .await?;
        Ok(FetchPartitionResult {
            page_metrics: walk.page_metrics,
        })
    }
}
