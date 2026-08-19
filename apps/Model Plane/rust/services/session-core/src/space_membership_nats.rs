//! Plain (non-JetStream) cross-plane consumer for Control Plane's
//! `aqencia.controlplane.space.membership_changed` event, projected into the
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

use futures::StreamExt;
use serde::Deserialize;
use tracing::{info, warn};

use crate::store::Pool;

const SUBJECT: &str = "aqencia.controlplane.space.membership_changed";

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
}

/// Parse one event payload. Exposed separately from [`run`] so the decode
/// contract is unit-testable without a live NATS connection.
fn parse_event(payload: &[u8]) -> Result<MembershipChangedEvent, serde_json::Error> {
    serde_json::from_slice(payload)
}

/// `active=false` upserts a revocation row (deny resource-scoped run/thread/
/// memory access to this Space until further notice). `active=true` deletes
/// any existing revocation for this exact (space_ref, subject_id) — a
/// legitimately rejoined member must not stay permanently denied merely
/// because Control Plane once revoked them.
async fn apply_membership_change(
    pool: &Pool,
    event: &MembershipChangedEvent,
) -> Result<(), sqlx::Error> {
    if event.active {
        sqlx::query(
            "DELETE FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2",
        )
        .bind(&event.space_ref)
        .bind(&event.subject_id)
        .execute(pool)
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
        .execute(pool)
        .await?;
    }
    Ok(())
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
