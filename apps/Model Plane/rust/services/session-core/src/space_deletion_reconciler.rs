//! Leased reconciliation for exact, correlated external semantic-memory erases.
//!
//! A Space deletion commits canonical thread/memory deletion first.  Its
//! external Letta cleanup is therefore intentionally a separate, durable
//! uncertainty: no bridge timeout may resurrect local state or be reported as
//! success.  This worker retries only receipt-ledger IDs captured at deletion
//! time; it never discovers or bulk-deletes user/org semantic memory.

use crate::{
    letta_adapter::{LettaDeleteOutcome, LettaMemoryAdapter},
    store::Pool,
};
use anyhow::Result;
use sqlx::FromRow;
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

const DEFAULT_INTERVAL_SECS: u64 = 60;
const MAX_INTERVAL_SECS: u64 = 86_400;
const LEASE_SECS: i64 = 60;
const BATCH_SIZE: i64 = 32;

#[derive(FromRow)]
struct ClaimedReceipt {
    deletion_request_id: String,
    org_id: String,
    owner_principal_id: String,
    memory_id: String,
    lease_token: String,
}

fn interval_secs(value: Option<&str>) -> u64 {
    value
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_INTERVAL_SECS)
        .min(MAX_INTERVAL_SECS)
}

fn receipt_settlement(outcome: &LettaDeleteOutcome) -> (&'static str, String) {
    if outcome.deleted && outcome.degradation_reason.is_none() {
        ("confirmed", String::new())
    } else {
        (
            "unconfirmed",
            outcome
                .degradation_reason
                .unwrap_or("semantic_delete_not_confirmed")
                .to_owned(),
        )
    }
}

async fn claim_due(pool: &Pool, limit: i64) -> Result<Vec<ClaimedReceipt>> {
    let lease_token = Uuid::new_v4().to_string();
    let rows = sqlx::query_as::<_, ClaimedReceipt>(
        "WITH due AS (
             SELECT deletion_request_id, memory_id
             FROM space_deletion_semantic_memory_receipts
             WHERE status <> 'confirmed'
               AND (lease_expires_at IS NULL OR lease_expires_at <= now())
             ORDER BY updated_at, deletion_request_id, memory_id
             FOR UPDATE SKIP LOCKED
             LIMIT $1
         )
         UPDATE space_deletion_semantic_memory_receipts AS receipt
         SET lease_token = $2,
             lease_expires_at = now() + ($3 * interval '1 second'),
             updated_at = now()
         FROM due
         WHERE receipt.deletion_request_id = due.deletion_request_id
           AND receipt.memory_id = due.memory_id
         RETURNING receipt.deletion_request_id, receipt.org_id,
                   receipt.owner_principal_id, receipt.memory_id,
                   receipt.lease_token",
    )
    .bind(limit)
    .bind(&lease_token)
    .bind(LEASE_SECS)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn settle_claim(
    pool: &Pool,
    claim: &ClaimedReceipt,
    outcome: &LettaDeleteOutcome,
) -> Result<bool> {
    let (status, error) = receipt_settlement(outcome);
    let result = sqlx::query(
        "UPDATE space_deletion_semantic_memory_receipts
         SET status = $1, attempts = attempts + 1, last_error = $2,
             lease_token = NULL, lease_expires_at = NULL, updated_at = now()
         WHERE deletion_request_id = $3 AND memory_id = $4 AND lease_token = $5",
    )
    .bind(status)
    .bind(error)
    .bind(&claim.deletion_request_id)
    .bind(&claim.memory_id)
    .bind(&claim.lease_token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn reconcile_once(pool: &Pool, letta: &LettaMemoryAdapter) -> Result<i64> {
    let claims = claim_due(pool, BATCH_SIZE).await?;
    let mut settled = 0_i64;
    for claim in claims {
        let outcome = letta
            .delete_detailed(&claim.org_id, &claim.owner_principal_id, &claim.memory_id)
            .await;
        if settle_claim(pool, &claim, &outcome).await? {
            settled += 1;
            if !outcome.deleted || outcome.degradation_reason.is_some() {
                warn!(
                    deletion_request_id = %claim.deletion_request_id,
                    org_id = %claim.org_id,
                    memory_id = %claim.memory_id,
                    degradation = ?outcome.degradation_reason,
                    "Space deletion semantic-memory reconciliation remains unconfirmed"
                );
            }
        }
    }
    Ok(settled)
}

pub(crate) async fn run(pool: Pool, letta: LettaMemoryAdapter) -> Result<()> {
    let interval_secs = interval_secs(
        std::env::var("SPACE_DELETION_SEMANTIC_RETRY_INTERVAL_SECS")
            .ok()
            .as_deref(),
    );
    let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    info!(
        interval_secs,
        "Space deletion semantic-memory reconciler started"
    );
    loop {
        tick.tick().await;
        match reconcile_once(&pool, &letta).await {
            Ok(count) if count > 0 => {
                info!(count, "Space deletion semantic-memory receipts reconciled")
            }
            Ok(_) => {}
            Err(error) => {
                warn!(%error, "Space deletion semantic-memory reconciliation cycle failed")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{interval_secs, receipt_settlement, DEFAULT_INTERVAL_SECS, MAX_INTERVAL_SECS};
    use crate::letta_adapter::LettaDeleteOutcome;

    #[test]
    fn ambiguous_or_degraded_delete_never_becomes_confirmed() {
        for outcome in [
            LettaDeleteOutcome {
                deleted: false,
                degradation_reason: None,
            },
            LettaDeleteOutcome {
                deleted: true,
                degradation_reason: Some("DEGRADED_LETTA_TIMEOUT"),
            },
        ] {
            let (status, error) = receipt_settlement(&outcome);
            assert_eq!(status, "unconfirmed");
            assert!(!error.is_empty());
        }
        let (status, error) = receipt_settlement(&LettaDeleteOutcome {
            deleted: true,
            degradation_reason: None,
        });
        assert_eq!(status, "confirmed");
        assert!(error.is_empty());
    }

    #[test]
    fn retry_interval_is_positive_bounded_and_fail_safe() {
        assert_eq!(interval_secs(None), DEFAULT_INTERVAL_SECS);
        assert_eq!(interval_secs(Some("0")), DEFAULT_INTERVAL_SECS);
        assert_eq!(interval_secs(Some("invalid")), DEFAULT_INTERVAL_SECS);
        assert_eq!(interval_secs(Some("999999")), MAX_INTERVAL_SECS);
    }

    #[test]
    fn receipt_lease_migration_has_no_implicit_success_path() {
        let migration =
            include_str!("../migrations/0027_space_deletion_semantic_memory_leases.sql");
        assert!(migration.contains("lease_token TEXT"));
        assert!(migration.contains("lease_expires_at TIMESTAMPTZ"));
        assert!(!migration.contains("DEFAULT 'confirmed'"));
    }
}
