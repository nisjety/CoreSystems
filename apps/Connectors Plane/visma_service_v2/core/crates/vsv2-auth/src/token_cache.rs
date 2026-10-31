use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// In-memory + Redis-backed token cache. Phase 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedToken {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}

impl CachedToken {
    pub fn is_fresh(&self, skew_seconds: i64) -> bool {
        let now = Utc::now() + chrono::Duration::seconds(skew_seconds);
        now < self.expires_at
    }
}
