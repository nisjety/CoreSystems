use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use vsv2_common::TenantId;

/// Tenant-level Redis lease — only one runner may sync a tenant at a time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    pub tenant: TenantId,
    pub owner_id: String,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl Lease {
    pub fn redis_key(tenant: &TenantId) -> String {
        format!("vsv2:lease:tenant:{tenant}")
    }

    pub const GLOBAL_KEY: &'static str = "vsv2:lease:global";
}
