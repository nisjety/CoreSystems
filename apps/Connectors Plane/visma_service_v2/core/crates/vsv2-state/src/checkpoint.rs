use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use vsv2_common::{Source, TenantId};

pub const DEFAULT_PARTITION_KEY: &str = "default";

fn default_partition_key() -> String {
    DEFAULT_PARTITION_KEY.to_string()
}

/// Composite primary key — tenant + endpoint + source + partition.
/// **Critical:** `source` lets REST and DataMart run side-by-side without
/// colliding on watermark state, and `partition_key` lets high-volume ERP REST
/// slices resume independently.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CheckpointKey {
    pub tenant: TenantId,
    pub endpoint: String,
    pub source: Source,
    #[serde(default = "default_partition_key")]
    pub partition_key: String,
}

impl CheckpointKey {
    pub fn default_partition(tenant: TenantId, endpoint: String, source: Source) -> Self {
        Self {
            tenant,
            endpoint,
            source,
            partition_key: default_partition_key(),
        }
    }

    pub fn for_partition(
        tenant: TenantId,
        endpoint: String,
        source: Source,
        partition_key: String,
    ) -> Self {
        Self {
            tenant,
            endpoint,
            source,
            partition_key,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub key: CheckpointKey,

    /// REST watermark (`lastModifiedDateTime`). Null when source != `ErpRest`.
    pub last_modified_watermark: Option<DateTime<Utc>>,

    /// DataMart watermark — DM ingestion timestamp.
    pub last_updated_in_dm: Option<DateTime<Utc>>,

    /// DataMart cursor — last `id` consumed (for resume after partial run).
    pub last_id: Option<String>,

    pub last_sync_at: Option<DateTime<Utc>>,
    pub records_seen: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_checkpoint_key_uses_default_partition() {
        let key = CheckpointKey::default_partition(
            TenantId::new("tenant-a"),
            "accounts".into(),
            Source::ErpRest,
        );

        assert_eq!(key.partition_key, DEFAULT_PARTITION_KEY);
    }

    #[test]
    fn partition_checkpoint_key_keeps_partition_dimension() {
        let key = CheckpointKey::for_partition(
            TenantId::new("tenant-a"),
            "general_ledger_transactions".into(),
            Source::ErpRest,
            "ledger=ACTUAL".into(),
        );

        assert_eq!(key.partition_key, "ledger=ACTUAL");
    }
}
