use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vsv2_common::{Source, TenantId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncPageMetric {
    pub run_id: Uuid,
    pub tenant: TenantId,
    pub endpoint: String,
    pub source: Source,
    pub partition_key: String,
    pub upstream_path: String,
    pub page_param: Option<String>,
    pub page_value: Option<i64>,
    pub page_size: Option<i64>,
    pub status: i32,
    pub elapsed_ms: i64,
    pub records: i64,
    pub total_count: Option<i64>,
    pub has_next: Option<bool>,
    pub rate_limit_limit: Option<i64>,
    pub rate_limit_remaining: Option<i64>,
    pub rate_limit_reset: Option<i64>,
    pub rate_limit_policy: Option<String>,
    pub retry_after_secs: Option<i64>,
    pub background_mode: Option<String>,
    pub fetched_at: DateTime<Utc>,
}
