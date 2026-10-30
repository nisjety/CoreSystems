//! Shadow-mode writer — writes DataMart records to `<table>_dm_shadow`
//! for 7-day diff validation before flipping the live flag.

use vsv2_common::{Result, TenantId};

use crate::bq_writer::BqWriter;

pub struct ShadowWriter {
    pub bq: BqWriter,
}

impl ShadowWriter {
    pub async fn write(
        &self,
        _tenant: &TenantId,
        table: &str,
        records: &[serde_json::Value],
        normalizer: Option<&str>,
    ) -> Result<u64> {
        self.bq.write_shadow(table, records, normalizer).await
    }
}
