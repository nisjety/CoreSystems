use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vsv2_common::{Source, TenantId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    PartialSuccess,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Pending => "pending",
            RunStatus::Running => "running",
            RunStatus::Succeeded => "succeeded",
            RunStatus::Failed => "failed",
            RunStatus::PartialSuccess => "partial_success",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRun {
    pub run_id: Uuid,
    pub tenant: TenantId,
    pub endpoint: String,
    pub source: Source,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub records_in: i64,
    pub records_written: i64,
    pub error: Option<String>,
}
