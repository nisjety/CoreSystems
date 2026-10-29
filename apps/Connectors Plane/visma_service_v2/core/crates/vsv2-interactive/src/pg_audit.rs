use async_trait::async_trait;
use sqlx::postgres::PgPool;
use sqlx::Row;
use uuid::Uuid;

use crate::audit::{AuditEntry, AuditSink};
use crate::error::{InteractiveError, Result};

/// Append-only audit in `vsv2.write_audit` (migration 0012; UPDATE/DELETE are blocked by trigger).
#[derive(Debug, Clone)]
pub struct PgAuditSink {
    pool: PgPool,
}

impl PgAuditSink {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn audit_err(e: sqlx::Error) -> InteractiveError {
    InteractiveError::Audit(e.to_string())
}

#[async_trait]
impl AuditSink for PgAuditSink {
    async fn record(&self, e: &AuditEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO vsv2.write_audit \
             (audit_id, plan_id, tenant_id, operation, caller, stage, target, method, path, \
              payload_hash, payload, http_status, detail, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(e.audit_id)
        .bind(e.plan_id)
        .bind(&e.tenant_id)
        .bind(&e.operation)
        .bind(&e.caller)
        .bind(e.stage.as_str())
        .bind(&e.target)
        .bind(&e.method)
        .bind(&e.path)
        .bind(&e.payload_hash)
        .bind(e.payload.as_ref().map(|v| sqlx::types::Json(v.clone())))
        .bind(e.http_status.map(i32::from))
        .bind(&e.detail)
        .bind(e.created_at)
        .execute(&self.pool)
        .await
        .map_err(|err| {
            // Unique index on (plan_id) WHERE stage = 'executing': the claim was already taken.
            let dup = err.as_database_error().and_then(|d| d.code()).as_deref() == Some("23505");
            match (dup, e.plan_id) {
                (true, Some(id)) => InteractiveError::AlreadyExecuted(id.to_string()),
                _ => audit_err(err),
            }
        })?;
        Ok(())
    }

    async fn was_attempted(&self, plan_id: Uuid) -> Result<bool> {
        let row = sqlx::query(
            "SELECT EXISTS (SELECT 1 FROM vsv2.write_audit WHERE plan_id = $1 AND stage = 'executing') AS hit",
        )
        .bind(plan_id)
        .fetch_one(&self.pool)
        .await
        .map_err(audit_err)?;
        Ok(row.get::<bool, _>("hit"))
    }
}
