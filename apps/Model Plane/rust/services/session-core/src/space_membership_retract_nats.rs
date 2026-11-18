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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::memory_declassify::{declassify_into_room, DeclassifyInput};
    use crate::space_membership_nats::{advance_fact_order, lock_fact_order};

    // ---- parse_fact (pure, no DB) ----------------------------------------------------------------

    #[test]
    fn parse_fact_decodes_the_outbox_payload_with_and_without_role() {
        let removal = parse_fact(
            br#"{"space_ref":"room-1","org_id":"org-1","subject_id":"user-1","active":false}"#,
        )
        .expect("a removal decodes with no role field");
        assert!(!removal.active);
        assert_eq!(removal.role, "", "role defaults to empty for a removal");

        let demotion = parse_fact(
            br#"{"space_ref":"room-1","org_id":"org-1","subject_id":"user-1","active":true,"role":"viewer"}"#,
        )
        .expect("an active fact with a role decodes");
        assert!(demotion.active);
        assert_eq!(demotion.role, "viewer");
    }

    #[test]
    fn parse_fact_reads_space_seq_as_absent_or_an_integer() {
        let unordered = parse_fact(
            br#"{"space_ref":"room-1","org_id":"org-1","subject_id":"user-1","active":false}"#,
        )
        .expect("a fact with no space_seq decodes");
        assert_eq!(unordered.space_seq, None);

        let ordered = parse_fact(
            br#"{"space_ref":"room-1","org_id":"org-1","subject_id":"user-1","active":false,"space_seq":7}"#,
        )
        .expect("a fact with space_seq decodes");
        assert_eq!(ordered.space_seq, Some(7));
    }

    #[test]
    fn retraction_required_table_matches_what_the_consumer_acts_on() {
        // The gate the consumer calls before retracting at all (re-exported from memory_declassify;
        // the table itself is proven in that module's own tests).
        assert!(retraction_required(false, ""));
        assert!(retraction_required(true, "viewer"));
        assert!(!retraction_required(true, "editor"));
    }

    // ---- DB-tier fixtures -------------------------------------------------------------------------

    async fn seed_personal_memory(pool: &Pool, org: &str, owner: &str, id: &str, key: &str) {
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, owner, scope, key, content, kind, confidence, review_state, is_latest) \
             VALUES ($1, $2, $3, 'user', $4, 'retract test content', 'fact', 1.0, 'accepted', true)",
        )
        .bind(id)
        .bind(org)
        .bind(owner)
        .bind(key)
        .execute(pool)
        .await
        .expect("seed personal memory");
    }

    fn digest(tag: u8) -> String {
        format!("sha256:{}", (0..32).map(|_| format!("{tag:02x}")).collect::<String>())
    }

    async fn share(pool: &Pool, org: &str, user: &str, space: &str, source_id: &str, tag: u8, authority_revision: i64) -> String {
        let decision_ref = format!("decision-{}", mp_ids::new_ulid());
        declassify_into_room(
            pool,
            DeclassifyInput {
                org_id: org,
                user_id: user,
                space_id: space,
                source_memory_id: source_id,
                reviewed_content: "reviewed text",
                content_digest: &digest(tag),
                decision_ref: &decision_ref,
                idempotency_key: &decision_ref,
                audience_hash: "sha256:test-audience",
                audience_revision: 1,
                authority_revision,
            },
        )
        .await
        .expect("share")
        .memory_id
    }

    async fn room_row_exists(pool: &Pool, memory_id: &str) -> bool {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM agent_memory WHERE id = $1")
            .bind(memory_id)
            .fetch_one(pool)
            .await
            .expect("count room row")
            > 0
    }

    async fn is_revoked(pool: &Pool, space: &str, subject: &str) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2)",
        )
        .bind(space)
        .bind(subject)
        .fetch_one(pool)
        .await
        .expect("read revocation row")
    }

    async fn clean_world(pool: &Pool, org: &str, space: &str) {
        for sql in [
            "DELETE FROM memory_declassifications WHERE org_id = $1 AND space_ref = $2",
            "DELETE FROM space_membership_retract_marks WHERE org_id = $1 AND space_ref = $2",
            "DELETE FROM space_membership_revocations WHERE space_ref = $2",
            "DELETE FROM space_membership_fact_order WHERE space_ref = $2",
            "DELETE FROM agent_memory WHERE org_id = $1",
        ] {
            sqlx::query(sql).bind(org).bind(space).execute(pool).await.ok();
        }
    }

    /// Seeds exactly what `space_membership_nats::apply_membership_change` would have committed
    /// for one fact, through the same `lock_fact_order`/`advance_fact_order` primitives, so these
    /// tests can set up "the membership consumer already applied this" without reaching into that
    /// module's private event type.
    async fn seed_membership_fact(
        pool: &Pool,
        space: &str,
        org: &str,
        subject: &str,
        active: bool,
        space_seq: Option<i64>,
    ) {
        let mut tx = pool.begin().await.expect("begin seed fact");
        lock_fact_order(&mut tx, org, space, subject).await.expect("lock fact order");
        if let Some(seq) = space_seq {
            advance_fact_order(&mut tx, org, space, subject, seq).await.expect("advance fact order");
        }
        if active {
            sqlx::query("DELETE FROM space_membership_revocations WHERE space_ref = $1 AND subject_id = $2")
                .bind(space)
                .bind(subject)
                .execute(&mut *tx)
                .await
                .expect("clear revocation");
        } else {
            sqlx::query(
                "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id) VALUES ($1, $2, $3) \
                 ON CONFLICT (space_ref, subject_id) DO UPDATE SET org_id = EXCLUDED.org_id, revoked_at = NOW()",
            )
            .bind(space)
            .bind(org)
            .bind(subject)
            .execute(&mut *tx)
            .await
            .expect("seed revocation");
        }
        tx.commit().await.expect("commit seed fact");
    }

    async fn connect() -> Option<Pool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        if !url.to_ascii_lowercase().contains("test") {
            eprintln!("skipping: DATABASE_URL must name a *test* database");
            return None;
        }
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations").run(&pool).await.expect("migrate");
        Some(pool)
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres with session-core migrations"]
    async fn retract_if_current_on_a_removal_below_the_stored_space_seq_does_nothing() {
        let Some(pool) = connect().await else { return };
        let org = format!("retract-org-{}", mp_ids::new_ulid());
        let space = format!("retract-space-{}", mp_ids::new_ulid());
        let subject = "member-1";
        let source = format!("mem-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, subject, &source, "pref-1").await;
        let room_id = share(&pool, &org, subject, &space, &source, 1, 1).await;

        // The membership consumer has already applied a newer fact (the re-add) at space_seq 9.
        seed_membership_fact(&pool, &space, &org, subject, true, Some(9)).await;

        // A removal fact at space_seq 3 (older than the stored 9) arrives late.
        let fact = MembershipFact {
            space_ref: space.clone(),
            org_id: org.clone(),
            subject_id: subject.to_owned(),
            active: false,
            role: String::new(),
            space_seq: Some(3),
        };
        let deleted = retract_if_current(&pool, &fact, true).await.expect("retract_if_current");
        // The removal is superseded for revocation purposes, but its retraction still runs against
        // shares authorised below its own revision (3): the share at revision 1 is below it.
        assert_eq!(deleted, 1);
        assert!(!room_row_exists(&pool, &room_id).await, "the pre-removal share is still retracted");
        assert!(!is_revoked(&pool, &space, subject).await, "a superseded removal must not write a revocation row");

        clean_world(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres with session-core migrations"]
    async fn a_removal_superseded_by_a_later_re_add_retracts_old_shares_and_spares_new_ones() {
        let Some(pool) = connect().await else { return };
        let org = format!("retract-order2-org-{}", mp_ids::new_ulid());
        let space = format!("retract-order2-space-{}", mp_ids::new_ulid());
        let subject = "member-2";
        let old_source = format!("mem-old-{}", mp_ids::new_ulid());
        let new_source = format!("mem-new-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, subject, &old_source, "pref-old").await;
        seed_personal_memory(&pool, &org, subject, &new_source, "pref-new").await;

        // A share made before the removal (authority revision 3).
        let old_room = share(&pool, &org, subject, &space, &old_source, 2, 3).await;

        // The membership consumer applies the removal (space_seq 5), then the re-add (space_seq
        // 10), exactly as it would if both were delivered before the retraction consumer got to
        // the removal fact at all.
        seed_membership_fact(&pool, &space, &org, subject, false, Some(5)).await;
        seed_membership_fact(&pool, &space, &org, subject, true, Some(10)).await;

        // A share made after the re-add (authority revision 10).
        let new_room = share(&pool, &org, subject, &space, &new_source, 3, 10).await;

        // The retraction consumer finally processes the original removal fact (space_seq 5),
        // delivered out of order relative to the re-add the membership consumer already applied.
        let fact = MembershipFact {
            space_ref: space.clone(),
            org_id: org.clone(),
            subject_id: subject.to_owned(),
            active: false,
            role: String::new(),
            space_seq: Some(5),
        };
        let deleted = retract_if_current(&pool, &fact, true).await.expect("retract_if_current");
        assert_eq!(deleted, 1, "only the pre-removal share (revision 3 < 5) is retracted");
        assert!(!room_row_exists(&pool, &old_room).await, "the old share is retracted");
        assert!(room_row_exists(&pool, &new_room).await, "the share made after the re-add survives");
        assert!(
            !is_revoked(&pool, &space, subject).await,
            "the re-add already cleared the revocation; the stale removal must not reinstate it"
        );

        clean_world(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres with session-core migrations"]
    async fn retract_while_revoked_is_a_noop_once_a_newer_fact_has_cleared_the_revocation() {
        let Some(pool) = connect().await else { return };
        let org = format!("retract-sweep-org-{}", mp_ids::new_ulid());
        let space = format!("retract-sweep-space-{}", mp_ids::new_ulid());
        let subject = "member-3";
        let source = format!("mem-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, subject, &source, "pref-1").await;
        let room_id = share(&pool, &org, subject, &space, &source, 4, 1).await;

        seed_membership_fact(&pool, &space, &org, subject, false, Some(1)).await;
        seed_membership_fact(&pool, &space, &org, subject, true, Some(2)).await;

        // The sweep catches up on this subject after the revocation has already been cleared.
        let outcome = retract_while_revoked(&pool, &org, &space, subject).await.expect("retract_while_revoked");
        assert_eq!(outcome, None, "a cleared revocation means the sweep does nothing for this subject");
        assert!(room_row_exists(&pool, &room_id).await, "the sweep must not touch a live share once cleared");

        clean_world(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres with session-core migrations"]
    async fn retract_while_revoked_deletes_the_live_shares_of_a_still_revoked_subject() {
        let Some(pool) = connect().await else { return };
        let org = format!("retract-sweep2-org-{}", mp_ids::new_ulid());
        let space = format!("retract-sweep2-space-{}", mp_ids::new_ulid());
        let subject = "member-4";
        let source = format!("mem-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, subject, &source, "pref-1").await;
        let room_id = share(&pool, &org, subject, &space, &source, 5, 1).await;

        seed_membership_fact(&pool, &space, &org, subject, false, Some(1)).await;

        let outcome = retract_while_revoked(&pool, &org, &space, subject).await.expect("retract_while_revoked");
        assert_eq!(outcome, Some(1), "a still-revoked subject's live share is retracted");
        assert!(!room_row_exists(&pool, &room_id).await);

        clean_world(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres with session-core migrations"]
    async fn a_demotion_retracts_live_shares_and_writes_no_revocation_row() {
        let Some(pool) = connect().await else { return };
        let org = format!("retract-demote-org-{}", mp_ids::new_ulid());
        let space = format!("retract-demote-space-{}", mp_ids::new_ulid());
        let subject = "member-5";
        let source = format!("mem-{}", mp_ids::new_ulid());
        seed_personal_memory(&pool, &org, subject, &source, "pref-1").await;
        let room_id = share(&pool, &org, subject, &space, &source, 6, 1).await;

        // A demotion: active=true with a role below editor, space_seq 4.
        let fact = MembershipFact {
            space_ref: space.clone(),
            org_id: org.clone(),
            subject_id: subject.to_owned(),
            active: true,
            role: "viewer".to_owned(),
            space_seq: Some(4),
        };
        assert!(retraction_required(fact.active, &fact.role), "a viewer must be retracted");
        let deleted = retract_if_current(&pool, &fact, false).await.expect("retract_if_current (demotion)");
        assert_eq!(deleted, 1);
        assert!(!room_row_exists(&pool, &room_id).await, "the demoted member's share is retracted");
        assert!(!is_revoked(&pool, &space, subject).await, "a demotion never writes a revocation row");

        clean_world(&pool, &org, &space).await;
    }

    /// W3-DEC-ORDER / W3-DEC-ORDER2: `sqlx::migrate!` reads only the top-level `migrations/` dir, so
    /// 0132's and 0133's down scripts (`migrations/revert/`) are never applied by it and must be run
    /// by hand; each states it is safe to run twice. Run inside an uncommitted transaction (as the
    /// 0045 revert test does, `grpc.rs`) so this never leaves the shared test database without the
    /// tables every other test in this file relies on.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres with session-core migrations"]
    async fn the_0132_and_0133_down_migrations_apply_and_are_idempotent() {
        let Some(pool) = connect().await else { return };
        let strip = |script: &str| {
            script
                .lines()
                .filter(|line| !matches!(line.trim().to_ascii_uppercase().as_str(), "BEGIN;" | "COMMIT;"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        // 0133 reverts before 0132 (the later migration first): 0133 drops a column 0132 does not touch.
        let revert_0133 = strip(include_str!(
            "../migrations/revert/0133_declassify_retraction_order.down.sql"
        ));
        let revert_0132 = strip(include_str!(
            "../migrations/revert/0132_space_membership_fact_order.down.sql"
        ));

        let mut tx = pool.begin().await.expect("begin");
        for _ in 0..2 {
            sqlx::raw_sql(&revert_0133).execute(&mut *tx).await.expect("0133 down applies, twice over");
        }
        for _ in 0..2 {
            sqlx::raw_sql(&revert_0132).execute(&mut *tx).await.expect("0132 down applies, twice over");
        }
        let marks_gone: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'space_membership_retract_marks')",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("check marks table");
        assert!(marks_gone, "0133's down drops space_membership_retract_marks");
        let column_gone: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS (SELECT 1 FROM information_schema.columns \
             WHERE table_name = 'memory_declassifications' AND column_name = 'authority_revision')",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("check authority_revision column");
        assert!(column_gone, "0133's down drops memory_declassifications.authority_revision");
        let fact_order_gone: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'space_membership_fact_order')",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("check fact order table");
        assert!(fact_order_gone, "0132's down drops space_membership_fact_order");
        tx.rollback().await.expect("rolled back: the shared test database keeps every table");

        let kept: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'space_membership_fact_order')",
        )
        .fetch_one(&pool)
        .await
        .expect("check fact order table after rollback");
        assert!(kept, "the rolled-back transaction leaves the table in place for every other test");
    }
}
