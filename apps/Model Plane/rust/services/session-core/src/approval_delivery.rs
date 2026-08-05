//! Durable, identifier-only approval-delivery outbox primitives.
//!
//! This module deliberately has no execution dispatcher. A granted approval
//! may be leased and retried, but it cannot be marked delivered or emit a
//! resumed event until Execution Core has a durable continuation descriptor
//! and can prove that it started the exact suspended work.

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use mp_ids::new_ulid;
use sqlx::FromRow;
use uuid::Uuid;

use crate::store::Pool;

/// Maximum number of durable delivery leases before a poison delivery becomes
/// terminal and requires an operator-visible remediation path.
pub const MAX_APPROVAL_DELIVERY_ATTEMPTS: u32 = 8;

/// The largest number of deliveries a single authenticated worker may claim.
pub const MAX_APPROVAL_DELIVERY_BATCH: u32 = 32;

/// Fixed server-owned lease duration. Workers cannot choose arbitrarily long
/// leases that would delay restart recovery.
const APPROVAL_DELIVERY_LEASE_SECS: i64 = 60;
const RETRY_BASE_SECS: u64 = 5;
const RETRY_MAX_SECS: u64 = 300;
const LEASE_TOKEN_HEX_LEN: usize = 32;

/// Delivery failure classifications are deliberately finite. Free-form errors
/// frequently include provider responses, arguments, or other retained data.
const ALLOWED_FAILURE_CODES: &[&str] = &[
    "continuation_unavailable",
    "transient_dependency",
    "invalid_continuation",
    "run_not_resumable",
    "approval_not_granted",
    "max_attempts_exhausted",
    "cancelled",
];

/// Verified Outcome Foundation (verevon-roadmap.md §3b) status strings, kept
/// in exact sync with migration `0020_verification_result.sql`'s CHECK
/// constraint and `model_plane.v1.VerificationStatus`'s non-`UNSPECIFIED`
/// variants.
const ALLOWED_VERIFICATION_STATUSES: &[&str] = &[
    "unknown",
    "verified_success",
    "verified_failure",
    "partially_verified",
];

/// The exact record returned to an authenticated worker after it claims a
/// lease. It has identifiers and a one-time opaque capability only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedApprovalDelivery {
    pub delivery_id: String,
    pub approval_id: String,
    pub run_id: String,
    pub org_id: String,
    pub user_id: String,
    pub attempt: u32,
    pub lease_token: String,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcknowledgedApprovalDelivery {
    pub attempt: u32,
    pub terminal: bool,
    pub settled: bool,
    pub next_attempt_at: Option<DateTime<Utc>>,
}

/// A durable, immutable record that a worker began one exact approved action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedContinuationReceipt {
    pub receipt_id: String,
    pub already_started: bool,
}

/// The only terminal outcome facts permitted for a started continuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationOutcome {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationOutcomeWrite {
    pub recorded: bool,
    pub already_finalized: bool,
    /// Populated only when `recorded` — the run this outcome belongs to, so a
    /// caller can broadcast an `ApprovalContinuationVerified` event without a
    /// second round trip. Empty on the `already_finalized` (no-op replay)
    /// path, since nothing new happened worth broadcasting.
    pub run_id: String,
}

/// Verified Outcome Foundation (verevon-roadmap.md §3b): an optional
/// independent judgment on whether a completed/failed continuation's claimed
/// outcome actually happened, distinct from `ContinuationOutcome` itself (a
/// workflow-state concept — did the dispatcher consider this delivery done).
/// Absent for a caller that doesn't yet produce one — stored as all-NULL,
/// never inferred from the outcome.
#[derive(Debug, Clone, Copy)]
pub struct VerificationFields<'a> {
    /// One of `ALLOWED_VERIFICATION_STATUSES`.
    pub status: &'a str,
    pub method: &'a str,
    pub reason: &'a str,
}

/// The only acknowledgements currently accepted by the outbox. There is no
/// `Delivered` state transition because no component can yet prove a durable
/// continuation actually started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryAcknowledgement {
    Retry,
    Terminal,
    Settled,
}

#[derive(FromRow)]
struct ClaimedApprovalDeliveryRow {
    delivery_id: String,
    approval_id: String,
    run_id: String,
    org_id: String,
    user_id: String,
    attempts: i32,
    lease_expires_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct AcknowledgedApprovalDeliveryRow {
    attempts: i32,
    state: String,
    next_attempt_at: DateTime<Utc>,
}

/// Atomically selects exactly one eligible, granted delivery with `SKIP
/// LOCKED`, so competing workers neither receive the same lease nor block one
/// another. The approval and run joins re-check the durable tenant/user/run
/// binding at dispatch time; the outbox columns alone are never trusted.
const CLAIM_APPROVAL_DELIVERY_SQL: &str = "WITH candidate AS ( \
    SELECT d.delivery_id \
    FROM approval_delivery_outbox AS d \
    JOIN approvals AS a ON a.id = d.approval_id \
      AND a.run_id = d.run_id \
      AND a.org_id = d.org_id \
      AND a.user_id = d.user_id \
    JOIN runs AS r ON r.id = d.run_id \
      AND r.org_id = d.org_id \
      AND r.user_id = d.user_id \
    WHERE d.org_id = $1 \
      AND a.status = 'granted' \
      AND r.status NOT IN ('completed', 'failed', 'cancelled') \
      AND d.attempts < $2 \
      AND ( \
        (d.state = 'pending' AND d.next_attempt_at <= now()) \
        OR (d.state = 'processing' AND d.lease_expires_at <= now()) \
      ) \
    ORDER BY d.next_attempt_at ASC, d.created_at ASC \
    FOR UPDATE SKIP LOCKED \
    LIMIT 1 \
) \
UPDATE approval_delivery_outbox AS d \
SET state = 'processing', \
    attempts = d.attempts + 1, \
    processing_at = now(), \
    lease_owner = $4, \
    lease_token_hash = $5, \
    lease_expires_at = now() + ($3::bigint * interval '1 second'), \
    last_failure_code = NULL \
FROM candidate \
WHERE d.delivery_id = candidate.delivery_id \
RETURNING d.delivery_id, d.approval_id, d.run_id, d.org_id, d.user_id, \
          d.attempts, d.lease_expires_at";

/// Safely terminates irrecoverable rows before workers claim more work. A
/// worker crash after its final lease reaches this query after the lease TTL;
/// it cannot be retried indefinitely.
const REAP_UNDELIVERABLE_APPROVALS_SQL: &str = "UPDATE approval_delivery_outbox AS d \
SET state = 'terminal', \
    terminal_at = now(), \
    last_failure_code = CASE \
      WHEN r.status IN ('completed', 'failed', 'cancelled') THEN 'run_not_resumable' \
      WHEN a.status <> 'granted' THEN 'approval_not_granted' \
      ELSE 'max_attempts_exhausted' \
    END, \
    lease_owner = NULL, \
    lease_token_hash = NULL, \
    lease_expires_at = NULL \
FROM approvals AS a, runs AS r \
WHERE d.approval_id = a.id \
  AND a.run_id = d.run_id \
  AND a.org_id = d.org_id \
  AND a.user_id = d.user_id \
  AND r.id = d.run_id \
  AND r.org_id = d.org_id \
  AND r.user_id = d.user_id \
  AND d.org_id = $1 \
  AND ( \
    r.status IN ('completed', 'failed', 'cancelled') \
    OR a.status <> 'granted' \
    OR (d.state = 'processing' AND d.attempts >= $2 AND d.lease_expires_at <= now()) \
  )";

/// Retry settlement is compare-and-set on the exact authenticated worker and
/// opaque lease hash. It clears the lease before another worker can claim it.
const ACK_RETRY_APPROVAL_DELIVERY_SQL: &str = "UPDATE approval_delivery_outbox AS d \
SET state = CASE WHEN d.attempts >= $5 THEN 'terminal' ELSE 'pending' END, \
    next_attempt_at = CASE \
      WHEN d.attempts >= $5 THEN d.next_attempt_at \
      ELSE now() + ($6::bigint * interval '1 second') \
    END, \
    terminal_at = CASE WHEN d.attempts >= $5 THEN now() ELSE d.terminal_at END, \
    last_failure_code = CASE WHEN d.attempts >= $5 THEN 'max_attempts_exhausted' ELSE $7 END, \
    lease_owner = NULL, \
    lease_token_hash = NULL, \
    lease_expires_at = NULL \
WHERE d.org_id = $1 \
  AND d.lease_owner = $2 \
  AND d.lease_token_hash = $3 \
  AND d.delivery_id = $4 \
  AND d.state = 'processing' \
  AND d.lease_expires_at > now() \
RETURNING d.attempts, d.state, d.next_attempt_at";

/// Locks the exact active lease before calculating retry backoff. The lock and
/// following update run in one transaction, so a late worker cannot settle a
/// lease that another worker has recovered after expiry.
const LOCK_ACTIVE_APPROVAL_DELIVERY_SQL: &str = "SELECT d.attempts \
FROM approval_delivery_outbox AS d \
WHERE d.org_id = $1 \
  AND d.lease_owner = $2 \
  AND d.lease_token_hash = $3 \
  AND d.delivery_id = $4 \
  AND d.state = 'processing' \
  AND d.lease_expires_at > now() \
FOR UPDATE";

/// A descriptor can be read only through the same live worker lease that will
/// later consume it. Normal approval APIs deliberately never project this
/// retained action input.
const LOAD_ACTIVE_CONTINUATION_DESCRIPTOR_SQL: &str =
    "SELECT a.metadata -> 'continuation_descriptor' \
FROM approval_delivery_outbox AS d \
JOIN approvals AS a ON a.id = d.approval_id \
  AND a.run_id = d.run_id \
  AND a.org_id = d.org_id \
  AND a.user_id = d.user_id \
WHERE d.org_id = $1 \
  AND d.lease_owner = $2 \
  AND d.lease_token_hash = $3 \
  AND d.delivery_id = $4 \
  AND d.approval_id = $5 \
  AND d.state = 'processing' \
  AND d.lease_expires_at > now() \
  AND a.status = 'granted' \
  AND a.metadata ? 'continuation_descriptor'";

/// The descriptor's version and action fingerprint come exclusively from the
/// persisted descriptor. A worker cannot choose either. The receipt ID is
/// unique to the delivery so a retry must reuse proof of the first start.
const INSERT_CONTINUATION_START_RECEIPT_SQL: &str = "INSERT INTO approval_continuation_receipts \
    (receipt_id, delivery_id, approval_id, run_id, org_id, user_id, descriptor_version, \
     action_fingerprint, execution_service_id) \
SELECT $6, d.delivery_id, d.approval_id, d.run_id, d.org_id, d.user_id, \
       (a.metadata -> 'continuation_descriptor' ->> 'version')::smallint, \
       a.metadata -> 'continuation_descriptor' ->> 'action_fingerprint', \
       $2 \
FROM approval_delivery_outbox AS d \
JOIN approvals AS a ON a.id = d.approval_id \
  AND a.run_id = d.run_id \
  AND a.org_id = d.org_id \
  AND a.user_id = d.user_id \
WHERE d.org_id = $1 \
  AND d.lease_owner = $2 \
  AND d.lease_token_hash = $3 \
  AND d.delivery_id = $4 \
  AND d.approval_id = $5 \
  AND d.state = 'processing' \
  AND d.lease_expires_at > now() \
  AND a.status = 'granted' \
  AND a.metadata ? 'continuation_descriptor' \
  AND a.metadata -> 'continuation_descriptor' ->> 'run_id' = d.run_id \
  AND a.metadata -> 'continuation_descriptor' ->> 'org_id' = d.org_id \
  AND a.metadata -> 'continuation_descriptor' ->> 'user_id' = d.user_id \
ON CONFLICT (delivery_id) DO NOTHING \
RETURNING receipt_id";

const GET_CONTINUATION_START_RECEIPT_SQL: &str = "SELECT receipt_id \
FROM approval_continuation_receipts \
WHERE delivery_id = $1";

const INSERT_CONTINUATION_OUTCOME_SQL: &str = "INSERT INTO approval_continuation_outcomes \
    (receipt_id, outcome, provider_receipt_id, failure_code, \
     verification_status, verification_method, verification_reason) \
SELECT r.receipt_id, $6, $7, $8, $10, $11, $12 \
FROM approval_continuation_receipts AS r \
JOIN approval_delivery_outbox AS d ON d.delivery_id = r.delivery_id \
WHERE d.org_id = $1 \
  AND d.lease_owner = $2 \
  AND d.lease_token_hash = $3 \
  AND d.delivery_id = $4 \
  AND d.approval_id = $5 \
  AND r.receipt_id = $9 \
  AND d.state = 'processing' \
  AND d.lease_expires_at > now() \
ON CONFLICT (receipt_id) DO NOTHING \
RETURNING receipt_id";

const GET_CONTINUATION_OUTCOME_SQL: &str = "SELECT receipt_id \
FROM approval_continuation_outcomes \
WHERE receipt_id = $1";

/// `receipt_id` is `approval_continuation_receipts`' primary key, so this is
/// a single indexed lookup — used only to attach `run_id` to a fresh outcome
/// write for event broadcast (see `record_continuation_outcome`).
const GET_CONTINUATION_RECEIPT_RUN_ID_SQL: &str = "SELECT run_id \
FROM approval_continuation_receipts \
WHERE receipt_id = $1";

/// Terminal settlement is also lease-bound. It never records a successful
/// continuation and therefore cannot cause a `RunResumedAfterApproval` event.
const ACK_TERMINAL_APPROVAL_DELIVERY_SQL: &str = "UPDATE approval_delivery_outbox AS d \
SET state = 'terminal', \
    terminal_at = now(), \
    last_failure_code = $5, \
    lease_owner = NULL, \
    lease_token_hash = NULL, \
    lease_expires_at = NULL \
WHERE d.org_id = $1 \
  AND d.lease_owner = $2 \
  AND d.lease_token_hash = $3 \
  AND d.delivery_id = $4 \
  AND d.state = 'processing' \
  AND d.lease_expires_at > now() \
RETURNING d.attempts, d.state, d.next_attempt_at";

/// Settlement is possible only after the same active lease recorded a
/// completed immutable continuation outcome with a non-empty provider receipt.
const ACK_SETTLED_APPROVAL_DELIVERY_SQL: &str = "UPDATE approval_delivery_outbox AS d \
SET state = 'settled', \
    settled_at = now(), \
    lease_owner = NULL, \
    lease_token_hash = NULL, \
    lease_expires_at = NULL, \
    last_failure_code = NULL \
FROM approval_continuation_receipts AS r \
JOIN approval_continuation_outcomes AS o ON o.receipt_id = r.receipt_id \
WHERE d.org_id = $1 \
  AND d.lease_owner = $2 \
  AND d.lease_token_hash = $3 \
  AND d.delivery_id = $4 \
  AND r.receipt_id = $5 \
  AND r.delivery_id = d.delivery_id \
  AND r.approval_id = d.approval_id \
  AND d.state = 'processing' \
  AND d.lease_expires_at > now() \
  AND o.outcome = 'completed' \
  AND o.provider_receipt_id IS NOT NULL \
RETURNING d.attempts, d.state, d.next_attempt_at";

/// Deterministic capped exponential backoff. Attempt 1 waits 5 seconds,
/// attempt 2 waits 10, etc.; the server, not a worker, owns the schedule.
#[must_use]
pub fn retry_delay_seconds(attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(6);
    RETRY_BASE_SECS
        .saturating_mul(1_u64 << exponent)
        .min(RETRY_MAX_SECS)
}

fn validate_nonempty_bounded(value: &str, label: &str, maximum: usize) -> Result<()> {
    if value.trim().is_empty()
        || value != value.trim()
        || value.len() > maximum
        || value.chars().any(char::is_control)
    {
        bail!("invalid {label}");
    }
    Ok(())
}

fn validate_delivery_scope(org_id: &str, worker_id: &str) -> Result<()> {
    validate_nonempty_bounded(org_id, "org_id", 128)?;
    validate_nonempty_bounded(worker_id, "worker_id", 256)
}

/// Validate a stable category without accepting free-form dependency output.
pub fn validate_failure_code(code: &str) -> Result<()> {
    if ALLOWED_FAILURE_CODES.contains(&code) {
        Ok(())
    } else {
        bail!("unsupported approval delivery failure code")
    }
}

fn new_lease_token() -> String {
    Uuid::new_v4().simple().to_string()
}

fn validate_lease_token(token: &str) -> Result<()> {
    if token.len() != LEASE_TOKEN_HEX_LEN || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid approval delivery lease token");
    }
    Ok(())
}

/// Validate fields that can be rejected before touching Postgres. The actual
/// lease comparison remains in the atomic database transaction.
pub fn validate_acknowledgement_input(delivery_id: &str, lease_token: &str) -> Result<()> {
    validate_nonempty_bounded(delivery_id, "delivery_id", 128)?;
    validate_lease_token(lease_token)
}

fn hash_lease_token(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn row_to_claimed(
    row: ClaimedApprovalDeliveryRow,
    lease_token: String,
) -> Result<ClaimedApprovalDelivery> {
    let attempt = u32::try_from(row.attempts)
        .map_err(|_| anyhow::anyhow!("negative approval delivery attempt"))?;
    Ok(ClaimedApprovalDelivery {
        delivery_id: row.delivery_id,
        approval_id: row.approval_id,
        run_id: row.run_id,
        org_id: row.org_id,
        user_id: row.user_id,
        attempt,
        lease_token,
        lease_expires_at: row.lease_expires_at,
    })
}

fn row_to_acknowledged(
    row: &AcknowledgedApprovalDeliveryRow,
) -> Result<AcknowledgedApprovalDelivery> {
    let attempt = u32::try_from(row.attempts)
        .map_err(|_| anyhow::anyhow!("negative approval delivery attempt"))?;
    let terminal = match row.state.as_str() {
        "terminal" => true,
        "pending" => false,
        "settled" => false,
        _ => bail!("unexpected approval delivery acknowledgement state"),
    };
    Ok(AcknowledgedApprovalDelivery {
        attempt,
        terminal,
        settled: row.state == "settled",
        next_attempt_at: (!terminal).then_some(row.next_attempt_at),
    })
}

/// Claim a bounded batch of deliveries for one authenticated service identity.
/// Exact concurrent callers use Postgres row locks rather than an in-process
/// mutex, so a session-core restart cannot duplicate a valid lease.
pub async fn claim_due_deliveries(
    pool: &Pool,
    org_id: &str,
    worker_id: &str,
    requested_max: u32,
) -> Result<Vec<ClaimedApprovalDelivery>> {
    validate_delivery_scope(org_id, worker_id)?;
    let max = requested_max.clamp(1, MAX_APPROVAL_DELIVERY_BATCH);
    reap_undeliverable_deliveries(pool, org_id).await?;

    let mut claimed = Vec::with_capacity(usize::try_from(max).unwrap_or(0));
    for _ in 0..max {
        let lease_token = new_lease_token();
        let token_hash = hash_lease_token(&lease_token);
        let row = sqlx::query_as::<_, ClaimedApprovalDeliveryRow>(CLAIM_APPROVAL_DELIVERY_SQL)
            .bind(org_id)
            .bind(i32::try_from(MAX_APPROVAL_DELIVERY_ATTEMPTS).unwrap_or(i32::MAX))
            .bind(APPROVAL_DELIVERY_LEASE_SECS)
            .bind(worker_id)
            .bind(token_hash)
            .fetch_optional(pool)
            .await?;
        let Some(row) = row else {
            break;
        };
        claimed.push(row_to_claimed(row, lease_token)?);
    }
    Ok(claimed)
}

/// Reap expired poison leases and records whose approval/run can no longer be
/// resumed. No run state or event is changed here.
pub async fn reap_undeliverable_deliveries(pool: &Pool, org_id: &str) -> Result<u64> {
    validate_nonempty_bounded(org_id, "org_id", 128)?;
    let result = sqlx::query(REAP_UNDELIVERABLE_APPROVALS_SQL)
        .bind(org_id)
        .bind(i32::try_from(MAX_APPROVAL_DELIVERY_ATTEMPTS).unwrap_or(i32::MAX))
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Atomically settle an active lease. A replay after a successful settlement
/// returns `Ok(None)` and cannot modify a later worker's new lease.
pub async fn acknowledge_delivery(
    pool: &Pool,
    org_id: &str,
    worker_id: &str,
    delivery_id: &str,
    lease_token: &str,
    acknowledgement: DeliveryAcknowledgement,
    failure_code: &str,
    continuation_receipt_id: &str,
) -> Result<Option<AcknowledgedApprovalDelivery>> {
    validate_delivery_scope(org_id, worker_id)?;
    validate_acknowledgement_input(delivery_id, lease_token)?;
    match acknowledgement {
        DeliveryAcknowledgement::Retry | DeliveryAcknowledgement::Terminal => {
            validate_failure_code(failure_code)?;
            if !continuation_receipt_id.is_empty() {
                bail!("failure acknowledgement must not include a continuation receipt");
            }
        }
        DeliveryAcknowledgement::Settled => {
            validate_nonempty_bounded(continuation_receipt_id, "continuation_receipt_id", 128)?;
            if !failure_code.is_empty() {
                bail!("settled acknowledgement must not include a failure code");
            }
        }
    }
    let token_hash = hash_lease_token(lease_token);
    let max_attempts = i32::try_from(MAX_APPROVAL_DELIVERY_ATTEMPTS).unwrap_or(i32::MAX);

    let mut transaction = pool.begin().await?;
    let active_attempt: Option<i32> = sqlx::query_scalar(LOCK_ACTIVE_APPROVAL_DELIVERY_SQL)
        .bind(org_id)
        .bind(worker_id)
        .bind(&token_hash)
        .bind(delivery_id)
        .fetch_optional(&mut *transaction)
        .await?;
    let Some(active_attempt) = active_attempt else {
        transaction.commit().await?;
        return Ok(None);
    };
    let active_attempt = u32::try_from(active_attempt)
        .map_err(|_| anyhow::anyhow!("negative approval delivery attempt"))?;

    let row = match acknowledgement {
        DeliveryAcknowledgement::Retry => {
            let delay = i64::try_from(retry_delay_seconds(active_attempt)).unwrap_or(i64::MAX);
            sqlx::query_as::<_, AcknowledgedApprovalDeliveryRow>(ACK_RETRY_APPROVAL_DELIVERY_SQL)
                .bind(org_id)
                .bind(worker_id)
                .bind(&token_hash)
                .bind(delivery_id)
                .bind(max_attempts)
                .bind(delay)
                .bind(failure_code)
                .fetch_optional(&mut *transaction)
                .await?
        }
        DeliveryAcknowledgement::Terminal => {
            sqlx::query_as::<_, AcknowledgedApprovalDeliveryRow>(ACK_TERMINAL_APPROVAL_DELIVERY_SQL)
                .bind(org_id)
                .bind(worker_id)
                .bind(&token_hash)
                .bind(delivery_id)
                .bind(failure_code)
                .fetch_optional(&mut *transaction)
                .await?
        }
        DeliveryAcknowledgement::Settled => {
            sqlx::query_as::<_, AcknowledgedApprovalDeliveryRow>(ACK_SETTLED_APPROVAL_DELIVERY_SQL)
                .bind(org_id)
                .bind(worker_id)
                .bind(&token_hash)
                .bind(delivery_id)
                .bind(continuation_receipt_id)
                .fetch_optional(&mut *transaction)
                .await?
        }
    };
    let acknowledged = row.as_ref().map(row_to_acknowledged).transpose()?;
    transaction.commit().await?;
    Ok(acknowledged)
}

/// Return the immutable descriptor only to the worker holding the exact
/// active lease. Missing, stale, foreign, and descriptor-free records are
/// intentionally indistinguishable.
pub async fn load_active_continuation_descriptor(
    pool: &Pool,
    org_id: &str,
    worker_id: &str,
    delivery_id: &str,
    approval_id: &str,
    lease_token: &str,
) -> Result<Option<serde_json::Value>> {
    validate_delivery_scope(org_id, worker_id)?;
    validate_acknowledgement_input(delivery_id, lease_token)?;
    validate_nonempty_bounded(approval_id, "approval_id", 128)?;
    let token_hash = hash_lease_token(lease_token);
    sqlx::query_scalar(LOAD_ACTIVE_CONTINUATION_DESCRIPTOR_SQL)
        .bind(org_id)
        .bind(worker_id)
        .bind(token_hash)
        .bind(delivery_id)
        .bind(approval_id)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
}

/// Record exactly one immutable start receipt for an active lease. A duplicate
/// result tells a recovered worker not to execute the action a second time.
pub async fn record_continuation_started(
    pool: &Pool,
    org_id: &str,
    worker_id: &str,
    delivery_id: &str,
    approval_id: &str,
    lease_token: &str,
) -> Result<Option<StartedContinuationReceipt>> {
    validate_delivery_scope(org_id, worker_id)?;
    validate_acknowledgement_input(delivery_id, lease_token)?;
    validate_nonempty_bounded(approval_id, "approval_id", 128)?;
    let token_hash = hash_lease_token(lease_token);
    let receipt_id = format!("continuation_receipt_{}", new_ulid());
    let inserted: Option<String> = sqlx::query_scalar(INSERT_CONTINUATION_START_RECEIPT_SQL)
        .bind(org_id)
        .bind(worker_id)
        .bind(token_hash)
        .bind(delivery_id)
        .bind(approval_id)
        .bind(&receipt_id)
        .fetch_optional(pool)
        .await?;
    if let Some(receipt_id) = inserted {
        return Ok(Some(StartedContinuationReceipt {
            receipt_id,
            already_started: false,
        }));
    }
    let existing: Option<String> = sqlx::query_scalar(GET_CONTINUATION_START_RECEIPT_SQL)
        .bind(delivery_id)
        .fetch_optional(pool)
        .await?;
    Ok(existing.map(|receipt_id| StartedContinuationReceipt {
        receipt_id,
        already_started: true,
    }))
}

fn validate_continuation_outcome(
    outcome: ContinuationOutcome,
    provider_receipt_id: &str,
    failure_code: &str,
) -> Result<()> {
    match outcome {
        ContinuationOutcome::Completed => {
            validate_nonempty_bounded(provider_receipt_id, "provider_receipt_id", 256)?;
            if !failure_code.is_empty() {
                bail!("completed continuation must not have a failure code");
            }
        }
        ContinuationOutcome::Failed | ContinuationOutcome::Cancelled => {
            if !provider_receipt_id.is_empty() {
                bail!("failed continuation must not have a provider receipt");
            }
            validate_failure_code(failure_code)?;
        }
    }
    Ok(())
}

fn validate_verification_fields(verification: Option<VerificationFields<'_>>) -> Result<()> {
    let Some(verification) = verification else {
        return Ok(());
    };
    if !ALLOWED_VERIFICATION_STATUSES.contains(&verification.status) {
        bail!("unsupported verification status");
    }
    validate_nonempty_bounded(verification.method, "verification_method", 64)?;
    if verification.reason.len() > 512 || verification.reason.chars().any(char::is_control) {
        bail!("invalid verification_reason");
    }
    Ok(())
}

/// Append the only terminal outcome for a started continuation. The active
/// lease and receipt binding make a stale worker incapable of finalizing a
/// later worker's attempt.
#[allow(clippy::too_many_arguments)]
pub async fn record_continuation_outcome(
    pool: &Pool,
    org_id: &str,
    worker_id: &str,
    delivery_id: &str,
    approval_id: &str,
    receipt_id: &str,
    lease_token: &str,
    outcome: ContinuationOutcome,
    provider_receipt_id: &str,
    failure_code: &str,
    verification: Option<VerificationFields<'_>>,
) -> Result<Option<ContinuationOutcomeWrite>> {
    validate_delivery_scope(org_id, worker_id)?;
    validate_acknowledgement_input(delivery_id, lease_token)?;
    validate_nonempty_bounded(approval_id, "approval_id", 128)?;
    validate_nonempty_bounded(receipt_id, "receipt_id", 128)?;
    validate_continuation_outcome(outcome, provider_receipt_id, failure_code)?;
    validate_verification_fields(verification)?;
    let token_hash = hash_lease_token(lease_token);
    let outcome = match outcome {
        ContinuationOutcome::Completed => "completed",
        ContinuationOutcome::Failed => "failed",
        ContinuationOutcome::Cancelled => "cancelled",
    };
    let inserted: Option<String> = sqlx::query_scalar(INSERT_CONTINUATION_OUTCOME_SQL)
        .bind(org_id)
        .bind(worker_id)
        .bind(token_hash)
        .bind(delivery_id)
        .bind(approval_id)
        .bind(outcome)
        .bind((!provider_receipt_id.is_empty()).then_some(provider_receipt_id))
        .bind((!failure_code.is_empty()).then_some(failure_code))
        .bind(receipt_id)
        .bind(verification.map(|v| v.status))
        .bind(verification.map(|v| v.method))
        .bind(verification.map(|v| v.reason))
        .fetch_optional(pool)
        .await?;
    if inserted.is_some() {
        // The INSERT...SELECT...RETURNING above can only return
        // approval_continuation_outcomes' own columns, not the joined
        // receipt's run_id — a second, indexed (PK) lookup is simplest.
        let run_id: Option<String> = sqlx::query_scalar(GET_CONTINUATION_RECEIPT_RUN_ID_SQL)
            .bind(receipt_id)
            .fetch_optional(pool)
            .await?;
        return Ok(Some(ContinuationOutcomeWrite {
            recorded: true,
            already_finalized: false,
            run_id: run_id.unwrap_or_default(),
        }));
    }
    let existing: Option<String> = sqlx::query_scalar(GET_CONTINUATION_OUTCOME_SQL)
        .bind(receipt_id)
        .fetch_optional(pool)
        .await?;
    Ok(existing.map(|_| ContinuationOutcomeWrite {
        recorded: false,
        already_finalized: true,
        run_id: String::new(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_policy_is_bounded_and_monotonic_for_poison_protection() {
        assert_eq!(retry_delay_seconds(1), 5);
        assert_eq!(retry_delay_seconds(2), 10);
        assert_eq!(retry_delay_seconds(3), 20);
        assert_eq!(retry_delay_seconds(MAX_APPROVAL_DELIVERY_ATTEMPTS), 300);
        assert_eq!(retry_delay_seconds(u32::MAX), 300);
    }

    #[test]
    fn lease_claim_query_is_tenant_run_and_approval_bound_and_skips_locked_rows() {
        for required in [
            "d.org_id = $1",
            "a.status = 'granted'",
            "a.run_id = d.run_id",
            "a.org_id = d.org_id",
            "a.user_id = d.user_id",
            "r.org_id = d.org_id",
            "r.user_id = d.user_id",
            "FOR UPDATE SKIP LOCKED",
            "lease_token_hash = $5",
        ] {
            assert!(CLAIM_APPROVAL_DELIVERY_SQL.contains(required));
        }
    }

    #[test]
    fn descriptor_lookup_is_lease_bound_and_never_uses_general_approval_reads() {
        for required in [
            "d.lease_owner = $2",
            "d.lease_token_hash = $3",
            "d.delivery_id = $4",
            "d.approval_id = $5",
            "d.lease_expires_at > now()",
            "a.metadata ? 'continuation_descriptor'",
        ] {
            assert!(
                LOAD_ACTIVE_CONTINUATION_DESCRIPTOR_SQL.contains(required),
                "descriptor lookup must require {required}"
            );
        }
        assert!(!LOAD_ACTIVE_CONTINUATION_DESCRIPTOR_SQL.contains("SELECT a.metadata FROM"));
    }

    #[test]
    fn continuation_receipts_are_append_only_and_distinct_from_delivery_leases() {
        let migration = include_str!("../migrations/0018_approval_continuation_receipts.sql");
        for required in [
            "CREATE TABLE IF NOT EXISTS approval_continuation_receipts",
            "delivery_id TEXT NOT NULL UNIQUE",
            "action_fingerprint TEXT NOT NULL",
            "CREATE TABLE IF NOT EXISTS approval_continuation_outcomes",
            "outcome IN ('completed', 'failed', 'cancelled')",
        ] {
            assert!(
                migration.contains(required),
                "receipt migration requires {required}"
            );
        }
        assert!(!migration.contains("UPDATE approval_continuation_receipts"));
        assert!(!migration.contains("provider_response"));
        assert!(!migration.contains("tool_input"));
    }

    #[test]
    fn receipt_start_derives_scope_and_fingerprint_from_the_live_descriptor() {
        for required in [
            "d.lease_owner = $2",
            "d.lease_token_hash = $3",
            "d.delivery_id = $4",
            "d.approval_id = $5",
            "a.metadata -> 'continuation_descriptor' ->> 'action_fingerprint'",
            "a.metadata -> 'continuation_descriptor' ->> 'run_id' = d.run_id",
            "ON CONFLICT (delivery_id) DO NOTHING",
        ] {
            assert!(
                INSERT_CONTINUATION_START_RECEIPT_SQL.contains(required),
                "start receipt must require {required}"
            );
        }
        assert!(!INSERT_CONTINUATION_START_RECEIPT_SQL.contains("tool_input"));
        assert!(!INSERT_CONTINUATION_START_RECEIPT_SQL.contains("provider_response"));
    }

    #[test]
    fn continuation_outcome_requires_live_lease_and_authoritative_completion_receipt() {
        for required in [
            "d.lease_owner = $2",
            "d.lease_token_hash = $3",
            "d.delivery_id = $4",
            "d.approval_id = $5",
            "r.receipt_id = $9",
            "verification_status, verification_method, verification_reason",
            "ON CONFLICT (receipt_id) DO NOTHING",
        ] {
            assert!(
                INSERT_CONTINUATION_OUTCOME_SQL.contains(required),
                "outcome write must require {required}"
            );
        }
        assert!(validate_continuation_outcome(
            ContinuationOutcome::Completed,
            "provider-receipt-1",
            ""
        )
        .is_ok());
        assert!(validate_continuation_outcome(ContinuationOutcome::Completed, "", "").is_err());
        assert!(validate_continuation_outcome(
            ContinuationOutcome::Failed,
            "provider-receipt-1",
            "invalid_continuation"
        )
        .is_err());
        assert!(validate_continuation_outcome(
            ContinuationOutcome::Failed,
            "",
            "invalid_continuation"
        )
        .is_ok());
    }

    #[test]
    fn verification_fields_are_optional_and_validated_against_the_fixed_allowlist() {
        assert!(validate_verification_fields(None).is_ok());
        assert!(validate_verification_fields(Some(VerificationFields {
            status: "verified_success",
            method: "structural",
            reason: "provider returned an authoritative receipt id",
        }))
        .is_ok());
        assert!(validate_verification_fields(Some(VerificationFields {
            status: "made_up_status",
            method: "structural",
            reason: "",
        }))
        .is_err());
        assert!(validate_verification_fields(Some(VerificationFields {
            status: "verified_success",
            method: "",
            reason: "",
        }))
        .is_err());
        // An empty reason is allowed (not every judgment needs a justification
        // string) — only a control character or an over-long one is rejected.
        assert!(validate_verification_fields(Some(VerificationFields {
            status: "unknown",
            method: "structural",
            reason: "",
        }))
        .is_ok());
        assert!(validate_verification_fields(Some(VerificationFields {
            status: "unknown",
            method: "structural",
            reason: "contains a \0 control byte",
        }))
        .is_err());
    }

    #[test]
    fn only_bounded_failure_classifications_are_storable() {
        for accepted in [
            "continuation_unavailable",
            "transient_dependency",
            "invalid_continuation",
            "run_not_resumable",
            "max_attempts_exhausted",
            "cancelled",
        ] {
            assert!(validate_failure_code(accepted).is_ok());
        }
        for rejected in ["provider said: secret", "", "arbitrary_free_form"] {
            assert!(validate_failure_code(rejected).is_err());
        }
    }

    #[test]
    fn lease_tokens_are_opaque_fixed_width_and_only_hashes_are_persisted() {
        let token = new_lease_token();
        assert_eq!(token.len(), LEASE_TOKEN_HEX_LEN);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(hash_lease_token(&token).len(), 64);
        assert!(validate_lease_token(&token).is_ok());
        assert!(validate_lease_token("not-a-lease").is_err());
    }

    #[test]
    fn settlement_sql_requires_exact_worker_and_lease_hash_and_never_marks_delivered() {
        for query in [
            ACK_RETRY_APPROVAL_DELIVERY_SQL,
            ACK_TERMINAL_APPROVAL_DELIVERY_SQL,
        ] {
            assert!(query.contains("d.org_id = $1"));
            assert!(query.contains("d.lease_owner = $2"));
            assert!(query.contains("d.lease_token_hash = $3"));
            assert!(query.contains("d.state = 'processing'"));
            assert!(!query.contains("'delivered'"));
        }
    }

    #[test]
    fn settlement_requires_a_completed_receipt_and_never_claims_customer_delivery() {
        for required in [
            "d.lease_owner = $2",
            "d.lease_token_hash = $3",
            "r.receipt_id = $5",
            "r.delivery_id = d.delivery_id",
            "o.outcome = 'completed'",
            "o.provider_receipt_id IS NOT NULL",
            "state = 'settled'",
        ] {
            assert!(
                ACK_SETTLED_APPROVAL_DELIVERY_SQL.contains(required),
                "settlement must require {required}"
            );
        }
        assert!(!ACK_SETTLED_APPROVAL_DELIVERY_SQL.contains("'delivered'"));
        assert!(!ACK_SETTLED_APPROVAL_DELIVERY_SQL.contains("customer"));
    }

    #[test]
    fn settlement_migration_names_internal_settlement_without_customer_delivery_claims() {
        let migration = include_str!("../migrations/0019_approval_delivery_settlement.sql");
        assert!(migration.contains("state IN ('pending', 'processing', 'terminal', 'settled')"));
        assert!(migration.contains("ADD COLUMN IF NOT EXISTS settled_at"));
        assert!(!migration.contains("customer_delivered"));
        assert!(!migration.contains("provider_response"));
    }
}
