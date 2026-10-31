use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateLimitSnapshot {
    pub limit: Option<u32>,
    pub remaining: Option<u32>,
    pub reset: Option<i64>,
    pub policy: Option<String>,
    pub retry_after_secs: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageFetchMetric {
    pub upstream_path: String,
    pub page_param: Option<String>,
    pub page_value: Option<i64>,
    pub page_size: Option<u32>,
    pub status: u16,
    pub elapsed_ms: i64,
    pub records: i64,
    pub total_count: Option<i64>,
    pub has_next: Option<bool>,
    pub rate_limit: RateLimitSnapshot,
    pub background_mode: Option<String>,
}
