//! Auto-retraction of a removed or demoted member's shared room copies (build plan W3 D1; WS17-S7).
//!
//! A person who shared a personal memory into a room (`declassify`) may retract it, and Session Core retracts
//! it for them when they stop being an editor of that room. Control publishes
//! `controlplane.space.membership_changed` from its durable outbox (user-core `membership_events_outbox.go`),
//! stating each membership fact with the subject's current role. A removal states `active=false`; a demotion
//! below editor states `active=true` with the lower role. For either, this consumer retracts the subject's live
//! declassified rows in that Space through `memory_declassify::retract_removed_subject`, the owner's own delete
//! path, without the owner's revocation check (a removed person is refused by that check).
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

use std::time::Duration;

use futures::StreamExt;
use serde::Deserialize;
use tracing::{info, warn};

use crate::space::memory_declassify::{retract_removed_subject, retraction_required, revoked_live_shares};
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
        match retract_removed_subject(pool, &fact.org_id, &fact.space_ref, &fact.subject_id, revoke).await {
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

/// Retract the live shares whose subject already holds a revocation in their Space. Each one is idempotent.
async fn sweep_revoked_shares(pool: &Pool) {
    let shares = match revoked_live_shares(pool, SWEEP_LIMIT).await {
        Ok(shares) => shares,
        Err(error) => {
            warn!(?error, "declassified share sweep could not read the ledger");
            return;
        }
    };
    for (org_id, space_id, subject_id) in shares {
        match retract_removed_subject(pool, &org_id, &space_id, &subject_id, false).await {
            Ok(deleted) => record_retraction(deleted, "sweep", &org_id, &space_id, &subject_id),
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

/// Count and log one retraction that deleted rows. A repeat that deletes nothing records nothing.
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
