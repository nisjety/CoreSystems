//! Plain (non-JetStream) cross-plane consumer for Control Plane's
//! `controlplane.space.membership_changed` event, projected into the
//! durable `space_membership_revocations` table (migration 0032).
//!
//! Deliberately mirrors `retrieval-engine-rs`'s `spawn_grant_invalidator`
//! pattern (a plain `nats.subscribe`, not a JetStream durable pull consumer)
//! rather than `gdpr_nats.rs`'s: a JetStream durable consumer needs its
//! stream/consumer topology pre-provisioned outside this crate (see
//! `gdpr_nats.rs`'s own FOLLOW-UP — that provisioning still hasn't landed,
//! so that consumer cannot bind at all today) and this fix's own tolerance
//! for a missed event is far higher than GDPR erasure's: a dropped
//! revocation just means the gap `resolve_run_owner`/`owner_matches`
//! (`run_service_grpc.rs`) closes takes a little longer to close for that
//! one subject, never a regression below today's org+user-only check.
//! Reliable delivery is not worth taking on a second service's deployment
//! risk for that failure mode.
//!
//! Connects over the same dedicated shared-broker session as the GDPR
//! consumer (`NATS_SHARED_URL`/`NATS_SHARED_USER`/`NATS_SHARED_PASSWORD`,
//! [`crate::nats_connection::connect_shared`]) — a fresh, independent
//! connection, so a bug or outage in the (currently non-functional)
//! JetStream GDPR consumer cannot affect this one or vice versa.
//!
//! Ordering (W3-DEC-ORDER). Each fact states the Space's authority revision it was produced at (`space_seq`). A fact is
//! applied only when that revision is newer than the last one applied for the same (Space, subject), kept in
//! `space_membership_fact_order` (migration 0132). A stale fact, or a repeat, changes nothing, so a late `active=true`
//! cannot reopen a room that a later removal closed. The retraction consumer (`space_membership_retract_nats.rs`) reads
//! the same value under the same per-subject lock.

use futures::StreamExt;
use serde::Deserialize;
use sqlx::{Postgres, Transaction};
use tracing::{debug, info, warn};

use crate::store::Pool;

const SUBJECT: &str = "controlplane.space.membership_changed";

/// W3-DEC-ORDER: the (org, Space, subject) lock both consumers take before they read or advance the stored ordering (see
/// [`lock_fact_order`]). It is a different key from the declassify and retraction lock (`LOCK_MEMBER_SQL` in
/// `space/memory_declassify.rs`).
const LOCK_FACT_ORDER_SQL: &str =
    "SELECT pg_advisory_xact_lock(hashtextextended('space_membership_fact|' || $1::text || '|' || $2::text || '|' || $3::text, 0))";

/// `active` states the CURRENT fact (member or not), not an increment: a
/// redundant or reordered delivery of the same fact is a no-op, which two
/// separate revoke/restore event types would not tolerate as safely under
/// at-least-once, unordered delivery.
#[derive(Deserialize)]
struct MembershipChangedEvent {
    space_ref: String,
    org_id: String,
    subject_id: String,
    active: bool,
    /// The Space authority revision the fact was produced at (W3-DEC-ORDER). Absent only from an older producer, whose
    /// facts are applied without ordering, as before.
    #[serde(default)]
    space_seq: Option<i64>,
}

/// Parse one event payload. Exposed separately from [`run`] so the decode
/// contract is unit-testable without a live NATS connection.
fn parse_event(payload: &[u8]) -> Result<MembershipChangedEvent, serde_json::Error> {
    serde_json::from_slice(payload)
}

/// W3-DEC-ORDER: take the (org, Space, subject) ordering lock for the rest of the transaction. Both consumers take it
/// before they read or advance the stored ordering, so one subject's facts apply one at a time. It is distinct from the
/// declassify and retraction lock, so a transaction holding this one may run `retract_removed_subject` on another
/// connection without waiting on itself.
pub(crate) async fn lock_fact_order(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    space_ref: &str,
    subject_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(LOCK_FACT_ORDER_SQL)
        .bind(org_id)
        .bind(space_ref)
        .bind(subject_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// The Space authority revision of the last fact applied for this subject, or `None` when none has been applied.
pub(crate) async fn stored_fact_seq(
    tx: &mut Transaction<'_, Postgres>,
    space_ref: &str,
    subject_id: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT space_seq FROM space_membership_fact_order WHERE space_ref = $1 AND subject_id = $2",
    )
    .bind(space_ref)
    .bind(subject_id)
    .fetch_optional(&mut **tx)
    .await
}

/// Record `space_seq` as the last applied revision for this subject, if it is newer than the stored one. Returns false,
/// and writes nothing, for a fact no newer than the stored one.
pub(crate) async fn advance_fact_order(
    tx: &mut Transaction<'_, Postgres>,
    org_id: &str,
    space_ref: &str,
    subject_id: &str,
    space_seq: i64,
) -> Result<bool, sqlx::Error> {
    let advanced: Option<i64> = sqlx::query_scalar(
        "INSERT INTO space_membership_fact_order (space_ref, subject_id, org_id, space_seq) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (space_ref, subject_id) DO UPDATE \
         SET org_id = EXCLUDED.org_id, space_seq = EXCLUDED.space_seq, applied_at = NOW() \
         WHERE space_membership_fact_order.space_seq < EXCLUDED.space_seq \
         RETURNING space_seq",
    )
    .bind(space_ref)
    .bind(subject_id)
    .bind(org_id)
    .bind(space_seq)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(advanced.is_some())
}

/// `active=false` upserts a revocation row (deny resource-scoped run/thread/
/// memory access to this Space until further notice). `active=true` deletes
/// any existing revocation for this exact (space_ref, subject_id) — a
/// legitimately rejoined member must not stay permanently denied merely
/// because Control Plane once revoked them.
///
/// W3-DEC-ORDER: a fact no newer than the stored ordering for its subject is ignored, clearing and setting alike. The
/// stored ordering and the change commit together, under the subject's lock.
async fn apply_membership_change(
    pool: &Pool,
    event: &MembershipChangedEvent,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    lock_fact_order(&mut tx, &event.org_id, &event.space_ref, &event.subject_id).await?;
    if let Some(space_seq) = event.space_seq {
        if !advance_fact_order(&mut tx, &event.org_id, &event.space_ref, &event.subject_id, space_seq).await? {
            debug!(
                space_ref = %event.space_ref,
                subject_id = %event.subject_id,
                space_seq,
                "stale Space membership fact ignored"
            );
            tx.rollback().await?;
            return Ok(());
        }
    }
    if event.active {
        sqlx::query(
            "DELETE FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2",
        )
        .bind(&event.space_ref)
        .bind(&event.subject_id)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (space_ref, subject_id) DO UPDATE SET org_id = EXCLUDED.org_id, revoked_at = NOW()",
        )
        .bind(&event.space_ref)
        .bind(&event.org_id)
        .bind(&event.subject_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Run the Space-membership-change consumer until the connection is lost.
///
/// `nats_url` is the shared cross-plane broker (`control-shared-nats`), not
/// session-core's own Model-Plane-local `NATS_URL` — that broker never
/// carries this subject. The caller in `main.rs` passes `NATS_SHARED_URL`.
///
/// # Errors
///
/// Returns an error if the initial shared-broker connection or subscribe
/// fails; the caller's supervised background loop retries.
pub async fn run(pool: Pool, nats_url: String) -> anyhow::Result<()> {
    info!(%nats_url, subject = SUBJECT, "session-core Space membership change consumer connecting");

    let client = crate::nats_connection::connect_shared(&nats_url).await?;
    let mut subscription = client.subscribe(SUBJECT).await?;
    info!(
        subject = SUBJECT,
        "Space membership change consumer subscribed"
    );

    while let Some(message) = subscription.next().await {
        let event = match parse_event(&message.payload) {
            Ok(event) => event,
            Err(error) => {
                warn!(?error, "Space membership change decode failed");
                continue;
            }
        };
        if let Err(error) = apply_membership_change(&pool, &event).await {
            warn!(
                ?error,
                space_ref = %event.space_ref,
                subject_id = %event.subject_id,
                active = event.active,
                "Space membership change persist failed"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_control_plane_event_shape() {
        let event = parse_event(
            br#"{"space_ref":"room-1","org_id":"org-1","subject_id":"user-1","active":false,"_source":"user-core","_published_at":"2026-08-19T00:00:00Z"}"#,
        )
        .expect("valid event decodes");
        assert_eq!(event.space_ref, "room-1");
        assert_eq!(event.org_id, "org-1");
        assert_eq!(event.subject_id, "user-1");
        assert!(!event.active);
    }

    #[test]
    fn rejects_a_payload_missing_a_required_field() {
        assert!(parse_event(br#"{"space_ref":"room-1","org_id":"org-1","active":false}"#).is_err());
    }

    // WS04-S10: the outbox and the re-announce sweep publish the same subject
    // with additive ledger fields. The consumer still decodes the current fact
    // and ignores every field it does not read, so an older consumer and a newer
    // producer keep working together.
    #[test]
    fn decodes_the_extended_ledger_payload_and_ignores_unknown_fields() {
        let event = parse_event(
            br#"{"space_ref":"room-1","org_id":"org-1","subject_id":"user-1","active":true,
                 "event_id":"sme_0b1c","space_seq":42,"action":"member_added","role":"editor",
                 "actor_kind":"user","reannounce":false,"a_future_field":{"nested":[1,2]},
                 "_source":"user-core","_published_at":"2026-11-02T00:00:00Z"}"#,
        )
        .expect("the extended payload decodes");
        assert_eq!(event.space_ref, "room-1");
        assert_eq!(event.org_id, "org-1");
        assert_eq!(event.subject_id, "user-1");
        assert!(event.active);
    }

    #[test]
    fn decodes_a_reannounce_of_an_inactive_member() {
        // The sweep states the current fact with no role for an inactive member.
        let event = parse_event(
            br#"{"space_ref":"room-2","org_id":"org-1","subject_id":"user-9","active":false,
                 "event_id":"sma_9f","space_seq":0,"action":"reannounce","role":"","actor_kind":"service"}"#,
        )
        .expect("a re-announcement decodes");
        assert_eq!(event.space_ref, "room-2");
        assert!(!event.active);
    }

    #[test]
    fn the_subject_is_the_one_user_core_publishes_on() {
        assert_eq!(SUBJECT, "controlplane.space.membership_changed");
    }

    #[test]
    fn rejects_a_non_boolean_active_even_with_the_extended_fields() {
        assert!(parse_event(
            br#"{"space_ref":"room-1","org_id":"org-1","subject_id":"user-1","active":"yes","event_id":"sme_1"}"#
        )
        .is_err());
    }

    // -- apply_membership_change (real Postgres) ------------------------------
    //
    // #[ignore]d so plain `cargo test` (no DB) skips this; run with a DB:
    //   DATABASE_URL=… cargo test --bin session-core -- --ignored apply_membership_change

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn apply_membership_change_revokes_then_clears_on_reactivation() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let sfx = std::process::id();
        let (space_ref, org_id, subject_id) = (
            format!("smn-space-{sfx}"),
            format!("smn-org-{sfx}"),
            format!("smn-subject-{sfx}"),
        );

        apply_membership_change(
            &pool,
            &MembershipChangedEvent {
                space_ref: space_ref.clone(),
                org_id: org_id.clone(),
                subject_id: subject_id.clone(),
                active: false,
                space_seq: Some(1),
            },
        )
        .await
        .expect("revoke");

        let (revoked,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2)",
        )
        .bind(&space_ref)
        .bind(&subject_id)
        .fetch_one(&pool)
        .await
        .expect("read revocation row");
        assert!(revoked, "revoke event must insert a revocation row");

        apply_membership_change(
            &pool,
            &MembershipChangedEvent {
                space_ref: space_ref.clone(),
                org_id: org_id.clone(),
                subject_id: subject_id.clone(),
                active: true,
                space_seq: Some(2),
            },
        )
        .await
        .expect("reactivate");

        let (still_revoked,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2)",
        )
        .bind(&space_ref)
        .bind(&subject_id)
        .fetch_one(&pool)
        .await
        .expect("read revocation row after reactivation");
        assert!(
            !still_revoked,
            "an active=true event must clear the revocation row, not leave it in place"
        );
    }
}
