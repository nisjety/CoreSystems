use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditStage {
    DryRun,
    Denied,
    /// Written *before* the request leaves; if the process dies mid-send this row is the trace.
    Executing,
    Succeeded,
    /// The write was not applied: the request never left (no token, no scope, rate limit) or Visma answered
    /// with an error that says so.
    Failed,
    /// The request left but the outcome is unclear (no answer, a timeout or reset, or a gateway error to a
    /// create): the write may or may not have applied.
    Unknown,
}

impl AuditStage {
    pub const fn as_str(&self) -> &'static str {
        match self {
            AuditStage::DryRun => "dry_run",
            AuditStage::Denied => "denied",
            AuditStage::Executing => "executing",
            AuditStage::Succeeded => "succeeded",
            AuditStage::Failed => "failed",
            AuditStage::Unknown => "unknown",
        }
    }
}

/// One append-only audit row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub audit_id: Uuid,
    pub plan_id: Option<Uuid>,
    pub tenant_id: String,
    pub operation: String,
    pub caller: String,
    pub stage: AuditStage,
    pub target: Option<String>,
    pub method: Option<String>,
    pub path: Option<String>,
    pub payload_hash: Option<String>,
    /// Full request body, kept on `dry_run` and `executing` rows for forensics.
    pub payload: Option<Value>,
    pub http_status: Option<u16>,
    pub detail: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait AuditSink: Send + Sync {
    /// Must be durable before returning. Execute aborts if this fails: no unaudited writes.
    /// For an `executing` row this is an atomic claim: a second row for the same plan must fail
    /// with `AlreadyExecuted`.
    async fn record(&self, entry: &AuditEntry) -> Result<()>;

    /// True if any `executing` row exists for the plan (a crashed attempt counts: the write may
    /// have landed, so a replay could double-post).
    async fn was_attempted(&self, plan_id: Uuid) -> Result<bool>;
}

/// In-process sink for tests and dry local runs.
#[derive(Debug, Default)]
pub struct MemoryAuditSink {
    entries: Mutex<Vec<AuditEntry>>,
}

impl MemoryAuditSink {
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.lock().expect("audit lock").clone()
    }

    pub fn stages(&self) -> Vec<AuditStage> {
        self.entries().iter().map(|e| e.stage).collect()
    }
}

#[async_trait]
impl AuditSink for MemoryAuditSink {
    async fn record(&self, entry: &AuditEntry) -> Result<()> {
        let mut entries = self.entries.lock().expect("audit lock");
        // Same guarantee as the unique index in Postgres: one `executing` claim per plan.
        if entry.stage == AuditStage::Executing {
            if let Some(id) = entry.plan_id {
                if entries
                    .iter()
                    .any(|e| e.plan_id == Some(id) && e.stage == AuditStage::Executing)
                {
                    return Err(crate::error::InteractiveError::AlreadyExecuted(
                        id.to_string(),
                    ));
                }
            }
        }
        entries.push(entry.clone());
        Ok(())
    }

    async fn was_attempted(&self, plan_id: Uuid) -> Result<bool> {
        Ok(self
            .entries()
            .iter()
            .any(|e| e.plan_id == Some(plan_id) && e.stage == AuditStage::Executing))
    }
}
