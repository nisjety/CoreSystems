//! Tenant-bound, append-only action receipts.
//!
//! Receipts are the durable proof boundary for agent actions. The browser
//! session may be in-memory, but the action/outcome/observation record must
//! survive an edge restart so Model Plane can distinguish VERIFIED, FAILED,
//! and UNKNOWN outcomes. Every query includes the verified org id; a run id
//! alone is never a sufficient authorization boundary.

#![cfg(feature = "postgres-queue")]

use sqlx::postgres::PgPool;
use sqlx::Row;

use crate::step_receipts::{
    AgentRunCheckpoint, AgentRunStatus, BrowserTimelineEvent, StepReceipt, StepReceiptStore,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::RunKind;

fn db_err(label: &str, error: sqlx::Error) -> QuarryError {
    QuarryError::new(ErrorCode::Internal, format!("postgres {label}: {error}"))
}

#[derive(Clone)]
pub struct PostgresStepReceiptStore {
    pool: PgPool,
}

impl PostgresStepReceiptStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn decode(row: sqlx::postgres::PgRow) -> QuarryResult<StepReceipt> {
        let action: serde_json::Value = row
            .try_get("action")
            .map_err(|error| db_err("decode action", error))?;
        let outcome: serde_json::Value = row
            .try_get("outcome")
            .map_err(|error| db_err("decode outcome", error))?;
        let observation: Option<serde_json::Value> = row
            .try_get("observation")
            .map_err(|error| db_err("decode observation", error))?;
        Ok(StepReceipt {
            org_id: Some(
                row.try_get("org_id")
                    .map_err(|error| db_err("decode org_id", error))?,
            ),
            actor_id: row
                .try_get("actor_id")
                .map_err(|error| db_err("decode actor_id", error))?,
            receipt_id: row
                .try_get("receipt_id")
                .map_err(|error| db_err("decode receipt_id", error))?,
            run_id: row
                .try_get("run_id")
                .map_err(|error| db_err("decode run_id", error))?,
            step: row
                .try_get::<i32, _>("step")
                .map_err(|error| db_err("decode step", error))? as u32,
            started_at: row
                .try_get("started_at")
                .map_err(|error| db_err("decode started_at", error))?,
            finished_at: row
                .try_get("finished_at")
                .map_err(|error| db_err("decode finished_at", error))?,
            action: serde_json::from_value(action).map_err(|error| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("decode receipt action: {error}"),
                )
            })?,
            outcome: serde_json::from_value(outcome).map_err(|error| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("decode receipt outcome: {error}"),
                )
            })?,
            observation: observation
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| {
                    QuarryError::new(
                        ErrorCode::Internal,
                        format!("decode receipt observation: {error}"),
                    )
                })?,
            correction_of: row
                .try_get("correction_of")
                .map_err(|error| db_err("decode correction_of", error))?,
            cost_micro_usd: row
                .try_get::<i64, _>("cost_micro_usd")
                .map_err(|error| db_err("decode cost", error))?
                .max(0) as u64,
        })
    }
}

#[async_trait::async_trait]
impl StepReceiptStore for PostgresStepReceiptStore {
    async fn append(&self, receipt: StepReceipt) -> QuarryResult<()> {
        let Some(org_id) = receipt.org_id.as_deref().filter(|value| !value.is_empty()) else {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "durable receipt requires a non-empty org_id",
            ));
        };
        let Some(actor_id) = receipt
            .actor_id
            .as_deref()
            .filter(|value| !value.is_empty())
        else {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "durable receipt requires a non-empty actor_id",
            ));
        };
        let action = serde_json::to_value(&receipt.action).map_err(|error| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("encode receipt action: {error}"),
            )
        })?;
        let outcome = serde_json::to_value(&receipt.outcome).map_err(|error| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("encode receipt outcome: {error}"),
            )
        })?;
        let observation = receipt
            .observation
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("encode receipt observation: {error}"),
                )
            })?;

        sqlx::query(
            "INSERT INTO quarry_step_receipts
             (receipt_id, run_id, org_id, actor_id, step, started_at, finished_at, action,
              outcome, observation, correction_of, cost_micro_usd)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
             ON CONFLICT (receipt_id) DO NOTHING",
        )
        .bind(&receipt.receipt_id)
        .bind(&receipt.run_id)
        .bind(org_id)
        .bind(actor_id)
        .bind(receipt.step as i32)
        .bind(receipt.started_at)
        .bind(receipt.finished_at)
        .bind(action)
        .bind(outcome)
        .bind(observation)
        .bind(&receipt.correction_of)
        .bind(receipt.cost_micro_usd as i64)
        .execute(&self.pool)
        .await
        .map_err(|error| db_err("append receipt", error))?;
        Ok(())
    }

    async fn list(&self, run_id: &RunKind) -> QuarryResult<Vec<StepReceipt>> {
        Err(QuarryError::new(
            ErrorCode::Forbidden,
            format!("org-scoped receipt listing required for run {run_id}"),
        ))
    }

    async fn list_for_org(&self, org_id: &str, run_id: &RunKind) -> QuarryResult<Vec<StepReceipt>> {
        if org_id.trim().is_empty() {
            return Err(QuarryError::new(ErrorCode::Forbidden, "org_id is required"));
        }
        let rows = sqlx::query(
            "SELECT receipt_id, run_id, org_id, actor_id, step, started_at, finished_at,
                    action, outcome, observation, correction_of, cost_micro_usd
             FROM quarry_step_receipts
             WHERE org_id = $1 AND run_id = $2
             ORDER BY step ASC, finished_at ASC",
        )
        .bind(org_id)
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| db_err("list receipts", error))?;
        rows.into_iter().map(Self::decode).collect()
    }

    async fn list_for_actor(
        &self,
        org_id: &str,
        actor_id: &str,
        run_id: &RunKind,
    ) -> QuarryResult<Vec<StepReceipt>> {
        if org_id.trim().is_empty() || actor_id.trim().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "org_id and actor_id are required",
            ));
        }
        let rows = sqlx::query(
            "SELECT receipt_id, run_id, org_id, actor_id, step, started_at, finished_at,
                    action, outcome, observation, correction_of, cost_micro_usd
             FROM quarry_step_receipts
             WHERE org_id = $1 AND actor_id = $2 AND run_id = $3
             ORDER BY step ASC, finished_at ASC",
        )
        .bind(org_id)
        .bind(actor_id)
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| db_err("list actor receipts", error))?;
        rows.into_iter().map(Self::decode).collect()
    }

    async fn get(&self, receipt_id: &str) -> QuarryResult<Option<StepReceipt>> {
        let row = sqlx::query(
            "SELECT receipt_id, run_id, org_id, actor_id, step, started_at, finished_at,
                    action, outcome, observation, correction_of, cost_micro_usd
             FROM quarry_step_receipts WHERE receipt_id = $1",
        )
        .bind(receipt_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| db_err("get receipt", error))?;
        row.map(Self::decode).transpose()
    }

    async fn append_browser_timeline_event(&self, event: BrowserTimelineEvent) -> QuarryResult<()> {
        if event.org_id.trim().is_empty() || event.actor_id.trim().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "browser timeline event requires org_id and actor_id",
            ));
        }
        let event_body = serde_json::to_value(&event.event).map_err(|error| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("encode browser timeline event: {error}"),
            )
        })?;
        sqlx::query(
            "INSERT INTO quarry_browser_timeline_events
             (event_id, org_id, actor_id, run_id, occurred_at, event)
             VALUES ($1,$2,$3,$4,$5,$6)
             ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(&event.event_id)
        .bind(&event.org_id)
        .bind(&event.actor_id)
        .bind(&event.run_id)
        .bind(event.occurred_at)
        .bind(event_body)
        .execute(&self.pool)
        .await
        .map_err(|error| db_err("append browser timeline event", error))?;
        Ok(())
    }

    async fn list_browser_timeline_events(
        &self,
        org_id: &str,
        actor_id: &str,
        run_id: &RunKind,
    ) -> QuarryResult<Vec<BrowserTimelineEvent>> {
        if org_id.trim().is_empty() || actor_id.trim().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "org_id and actor_id are required",
            ));
        }
        let rows = sqlx::query(
            "SELECT event_id, org_id, actor_id, run_id, occurred_at, event
             FROM quarry_browser_timeline_events
             WHERE org_id = $1 AND actor_id = $2 AND run_id = $3
             ORDER BY occurred_at ASC, event_id ASC",
        )
        .bind(org_id)
        .bind(actor_id)
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| db_err("list browser timeline events", error))?;
        rows.into_iter()
            .map(|row| {
                let event_body: serde_json::Value = row
                    .try_get("event")
                    .map_err(|error| db_err("decode browser timeline event", error))?;
                Ok(BrowserTimelineEvent {
                    event_id: row
                        .try_get("event_id")
                        .map_err(|error| db_err("decode browser timeline event id", error))?,
                    org_id: row
                        .try_get("org_id")
                        .map_err(|error| db_err("decode browser timeline org", error))?,
                    actor_id: row
                        .try_get("actor_id")
                        .map_err(|error| db_err("decode browser timeline actor", error))?,
                    run_id: row
                        .try_get("run_id")
                        .map_err(|error| db_err("decode browser timeline run", error))?,
                    occurred_at: row
                        .try_get("occurred_at")
                        .map_err(|error| db_err("decode browser timeline timestamp", error))?,
                    event: serde_json::from_value(event_body).map_err(|error| {
                        QuarryError::new(
                            ErrorCode::Internal,
                            format!("decode browser timeline event: {error}"),
                        )
                    })?,
                })
            })
            .collect()
    }

    async fn save_run_checkpoint(&self, checkpoint: AgentRunCheckpoint) -> QuarryResult<()> {
        let state = serde_json::to_value(&checkpoint).map_err(|error| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("encode agent checkpoint: {error}"),
            )
        })?;
        sqlx::query(
            "INSERT INTO quarry_agent_run_checkpoints
             (org_id, run_id, profile_id, step, current_url, page_hash, state, status, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,NOW())
             ON CONFLICT (org_id, run_id) DO UPDATE SET
               profile_id = EXCLUDED.profile_id,
               step = EXCLUDED.step,
               current_url = EXCLUDED.current_url,
               page_hash = EXCLUDED.page_hash,
               state = EXCLUDED.state,
               status = EXCLUDED.status,
               updated_at = NOW()",
        )
        .bind(&checkpoint.org_id)
        .bind(&checkpoint.run_id)
        .bind(&checkpoint.profile_id)
        .bind(checkpoint.step as i32)
        .bind(&checkpoint.current_url)
        .bind(&checkpoint.page_hash)
        .bind(state)
        .bind(match checkpoint.status {
            AgentRunStatus::Active => "active",
            AgentRunStatus::Closed => "closed",
        })
        .execute(&self.pool)
        .await
        .map_err(|error| db_err("save agent checkpoint", error))?;
        Ok(())
    }

    async fn load_run_checkpoint(
        &self,
        org_id: &str,
        run_id: &RunKind,
    ) -> QuarryResult<Option<AgentRunCheckpoint>> {
        let row = sqlx::query(
            "SELECT state FROM quarry_agent_run_checkpoints
             WHERE org_id = $1 AND run_id = $2",
        )
        .bind(org_id)
        .bind(run_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| db_err("load agent checkpoint", error))?;
        row.map(|row| {
            let state: serde_json::Value = row
                .try_get("state")
                .map_err(|error| db_err("decode agent checkpoint", error))?;
            serde_json::from_value(state).map_err(|error| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("decode agent checkpoint: {error}"),
                )
            })
        })
        .transpose()
    }

    async fn close_run_checkpoint(&self, org_id: &str, run_id: &RunKind) -> QuarryResult<()> {
        sqlx::query(
            "UPDATE quarry_agent_run_checkpoints
             SET status = 'closed', updated_at = NOW()
             WHERE org_id = $1 AND run_id = $2",
        )
        .bind(org_id)
        .bind(run_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|error| db_err("close agent checkpoint", error))?;
        Ok(())
    }
}
