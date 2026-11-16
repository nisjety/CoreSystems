//! Auto-retraction of a removed or demoted member's shared room copies (build plan W3 D1; WS17-S7).
//!
//! A person who shared a personal memory into a room (`declassify`) may retract it, and Session Core retracts
//! it for them when they stop being an editor of that room. Control publishes
//! `controlplane.space.membership_changed` from its durable outbox (user-core `membership_events_outbox.go`),
//! stating each membership fact with the subject's current role. A removal states `active=false`; a demotion
//! below editor states `active=true` with the lower role. For either, this consumer retracts the subject's live
//! declassified rows in that Space through `memory_declassify::retract_removed_subject`, which uses the owner's retract
//! statements limited to the subject's shares authorised below the change, without the owner's revocation check (a
//! removed person is refused by that check).
//!
//! Delivery. Like `space_membership_nats.rs`, this is a plain subscription, so a fact can be missed while the
//! consumer is down. Control re-announces every membership changed within 24 hours, every 10 minutes, which
//! redelivers a removal or a demotion inside that window. A periodic sweep covers a removal beyond it: a live
//! share whose subject holds a revocation row in its Space is retracted, because the revocation projection is
//! durable. A demotion older than the re-announce window whose event was missed is not covered (see the depth
//! file).
//!
//! Retraction is idempotent: a repeated fact or sweep finds no live row and changes nothing. A failed retraction is
//! retried here with backoff; the next re-announce or sweep retries it after that. Logs carry ids and counts only.
//!
//! Ordering (W3-DEC-ORDER, W3-DEC-ORDER2). A removal or demotion is never skipped for being older than the last fact the
//! membership consumer applied for its subject. It retracts the subject's shares authorised below its Space authority
//! revision (`memory_declassify::retract_removed_subject`), so a later re-add does not leave them behind, while a share
//! authorised after the re-add survives. Only a removal's revocation is ordered: it is written, and the revision recorded,
//! only when the removal is the newest fact applied for the subject, so a stale removal cannot revoke a re-added member.
//! Each attempt runs under the subject's ordering lock (`space_membership_nats.rs`), then the (org, Space, subject) lock
//! that declassify shares, so a newer fact cannot be applied between the check and the retraction.

use std::time::Duration;

use futures::StreamExt;
use serde::Deserialize;
use tracing::{info, warn};

use crate::space::memory_declassify::{retract_removed_subject, retraction_required, revoked_live_shares};
use crate::space_membership_nats::{advance_fact_order, lock_fact_order, stored_fact_seq};
use crate::store::Pool;

const SUBJECT: &str = "controlplane.space.membership_changed";

/// How often the revoked-subject sweep runs.
const SWEEP_EVERY: Duration = Duration::from_secs(600);

/// Shares swept per run. A larger backlog drains over several runs.
const SWEEP_LIMIT: i64 = 500;

/// Attempts per fact before the next re-announce or sweep takes over.
const RETRACT_ATTEMPTS: u32 = 4;

/// The fields of a membership fact this consumer reads. `role` is the subject's current role for an active member,
/// and empty for a removal (the outbox and re-announce payloads in `membership_events_outbox.go`).
#[derive(Deserialize)]
struct MembershipFact {
    space_ref: String,
    org_id: String,
    subject_id: String,
    active: bool,
    #[serde(default)]
    role: String,
    /// The Space authority revision the fact was produced at (W3-DEC-ORDER). Absent only from an older producer, whose
    /// facts are retracted without the ordering check, as before.
    #[serde(default)]
    space_seq: Option<i64>,
}

/// Parse one event payload. Exposed separately from [`run`] so the decode contract stays testable without a broker.
fn parse_fact(payload: &[u8]) -> Result<MembershipFact, serde_json::Error> {
    serde_json::from_slice(payload)
}

/// Run the retraction consumer until its subscription ends. The caller supervises and restarts it. The first sweep
/// runs at once, so a removal missed while the consumer was down is retracted at start.
///
/// # Errors
///
/// Returns an error if the shared-broker connection or the subscribe fails.
pub async fn run(pool: Pool, nats_url: String) -> anyhow::Result<()> {
    info!(%nats_url, subject = SUBJECT, "session-core Space membership retraction consumer connecting");
    let client = crate::nats_connection::connect_shared(&nats_url).await?;
    let mut subscription = client.subscribe(SUBJECT).await?;
    info!(subject = SUBJECT, "Space membership retraction consumer subscribed");
    let mut sweep = tokio::time::interval(SWEEP_EVERY);
    loop {
        tokio::select! {
            message = subscription.next() => {
                let Some(message) = message else { break };
                let fact = match parse_fact(&message.payload) {
                    Ok(fact) => fact,
                    Err(error) => {
                        warn!(?error, "Space membership retraction decode failed");
                        continue;
                    }
                };
                if retraction_required(fact.active, &fact.role) {
                    retract_with_retry(&pool, &fact).await;
                }
            }
            _ = sweep.tick() => sweep_revoked_shares(&pool).await,
        }
    }
    Ok(())
}

/// Retract one subject's shares for a fact, retrying with backoff. A removal also writes the revocation row, as the
/// membership consumer does, inside the same transaction as the retraction.
async fn retract_with_retry(pool: &Pool, fact: &MembershipFact) {
    let revoke = !fact.active;
    let reason = if revoke { "removed" } else { "below_editor" };
    let mut delay = Duration::from_secs(1);
    for attempt in 1..=RETRACT_ATTEMPTS {
        match retract_if_current(pool, fact, revoke).await {
            Ok(deleted) => {
                record_retraction(deleted, reason, &fact.org_id, &fact.space_ref, &fact.subject_id);
                return;
            }
            Err(error) => {
                warn!(
                    ?error,
                    attempt,
                    org_id = %fact.org_id,
                    space_id = %fact.space_ref,
                    subject_id = %fact.subject_id,
                    "declassified memory retraction failed; retrying"
                );
                if attempt < RETRACT_ATTEMPTS {
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
            }
        }
    }
    metrics::counter!("mp_session_space_memory_declassified_total", "op" => "auto_retract_failed").increment(1);
    warn!(
        org_id = %fact.org_id,
        space_id = %fact.space_ref,
        subject_id = %fact.subject_id,
        "declassified memory retraction gave up; the next re-announce or sweep retries it"
    );
}

/// One attempt at a fact's retraction (W3-DEC-ORDER2). The fact is retracted whatever the stored ordering: its shares
/// authorised below its revision go, and a share made after a later re-add does not. Only a removal's revocation is
/// ordered. It is written, and the revision recorded, only when the removal is newer than the stored value, so a stale
/// removal cannot revoke a member who was re-added. A stored value equal to the fact is not stale: the membership
/// consumer may have applied this same fact first, and the retraction must still run. A demotion records nothing, since
/// it changes no revocation. The transaction holds the subject's ordering lock throughout, then takes the (org, Space,
/// subject) lock inside `retract_removed_subject`, so a newer fact cannot be applied between the check and the retraction.
async fn retract_if_current(pool: &Pool, fact: &MembershipFact, revoke: bool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    lock_fact_order(&mut tx, &fact.org_id, &fact.space_ref, &fact.subject_id).await?;
    let stored = stored_fact_seq(&mut tx, &fact.space_ref, &fact.subject_id).await?;
    let superseded = match (fact.space_seq, stored) {
        (Some(space_seq), Some(stored)) => stored > space_seq,
        _ => false,
    };
    let revoke_now = revoke && !superseded;
    if revoke_now {
        if let Some(space_seq) = fact.space_seq {
            advance_fact_order(&mut tx, &fact.org_id, &fact.space_ref, &fact.subject_id, space_seq).await?;
        }
    }
    let deleted = retract_removed_subject(
        &mut tx,
        &fact.org_id,
        &fact.space_ref,
        &fact.subject_id,
        revoke_now,
        fact.space_seq,
    )
    .await?;
    tx.commit().await?;
    Ok(deleted)
}

/// Retract the live shares whose subject already holds a revocation in their Space. Each one is idempotent, and each is
/// checked under the subject's ordering lock (W3-DEC-ORDER), so a fact that clears the revocation cannot land between the
/// check and the retraction.
async fn sweep_revoked_shares(pool: &Pool) {
    let shares = match revoked_live_shares(pool, SWEEP_LIMIT).await {
        Ok(shares) => shares,
        Err(error) => {
            warn!(?error, "declassified share sweep could not read the ledger");
            return;
        }
    };
    for (org_id, space_id, subject_id) in shares {
        match retract_while_revoked(pool, &org_id, &space_id, &subject_id).await {
            Ok(Some(deleted)) => record_retraction(deleted, "sweep", &org_id, &space_id, &subject_id),
            Ok(None) => {}
            Err(error) => warn!(
                ?error,
                org_id = %org_id,
                space_id = %space_id,
                subject_id = %subject_id,
                "declassified share sweep retraction failed"
            ),
        }
    }
}

/// The sweep's attempt (W3-DEC-ORDER, W3-DEC-ORDER2): retract only while the subject's revocation still stands, checked
/// under the subject's ordering lock. `None` when the revocation has since been cleared by a newer fact. A revocation
/// stands for the removal the stored ordering names (a removal writes both in one transaction, and a later fact clears
/// the revocation), so the stored revision is the bound: only shares authorised below that removal are retracted.
async fn retract_while_revoked(
    pool: &Pool,
    org_id: &str,
    space_id: &str,
    subject_id: &str,
) -> Result<Option<u64>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    lock_fact_order(&mut tx, org_id, space_id, subject_id).await?;
    let revoked: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2)",
    )
    .bind(space_id)
    .bind(subject_id)
    .fetch_one(&mut *tx)
    .await?;
    if !revoked {
        tx.rollback().await?;
        return Ok(None);
    }
    let before = stored_fact_seq(&mut tx, space_id, subject_id).await?;
    let deleted = retract_removed_subject(&mut tx, org_id, space_id, subject_id, false, before).await?;
    tx.commit().await?;
    Ok(Some(deleted))
}

/// Count and log one retraction that retracted shares. A repeat that retracts nothing records nothing.
fn record_retraction(deleted: u64, reason: &'static str, org_id: &str, space_id: &str, subject_id: &str) {
    if deleted == 0 {
        return;
    }
    metrics::counter!(
        "mp_session_space_memory_declassified_total",
        "op" => "auto_retract",
        "reason" => reason
    )
    .increment(deleted);
    info!(
        org_id,
        space_id,
        subject_id,
        rows = deleted,
        reason,
        "declassified memory retracted for a removed or demoted member"
    );
}
