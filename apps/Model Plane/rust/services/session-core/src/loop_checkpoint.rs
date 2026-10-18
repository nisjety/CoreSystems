//! The durable loop's fold store (build plan WS03 S4).
//!
//! A run that pauses for an approval, or parks for credentials, leaves its loop's
//! fold here: the messages, counters and pending call the next segment needs to
//! continue the same run. The fold holds action-bearing state, so it is sealed
//! under the descriptor key and bound to its row. A run whose retention is not
//! `persistent` never keeps one: zero-retention and waived runs are refused.
//!
//! Writes are compare-and-set on `seq`, so a stale writer is refused. A claim moves
//! a paused or parked row to `resuming` under a lease, so exactly one continuation
//! resumes a given pause. The whole store is off unless
//! `SESSION_CORE_LOOP_CHECKPOINT_ENABLED` is `true`.

use chrono::{DateTime, Utc};
use mp_contracts::loop_fold::{LoopFoldV1, ParkReason, FOLD_VERSION};
use mp_contracts::zdr_refusal::Refusal;
use mp_events::idempotency::derive_idempotency_hash;
use mp_ids::new_ulid;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use tonic::{Code, Status};

use crate::continuation_crypto::DescriptorCipher;

/// The plaintext cap for one fold. The stored ciphertext is bounded by the table's CHECK.
pub(crate) const MAX_FOLD_BYTES: usize = 4 * 1024 * 1024;

/// A claim holds a pause for this many seconds before another continuation may take it.
const CLAIM_LEASE_SECONDS: f64 = 120.0;

const ENABLED_ENV: &str = "SESSION_CORE_LOOP_CHECKPOINT_ENABLED";
const CHECKPOINT_RESTORED_TYPE_URL: &str = "type.googleapis.com/model_plane.v1.CheckpointRestored";

/// Whether the fold store is switched on. Off by default: a loop keeps its fold in
/// memory and a pause cannot be resumed from the store.
pub(crate) fn enabled() -> bool {
    std::env::var(ENABLED_ENV).is_ok_and(|value| value.trim().eq_ignore_ascii_case("true"))
}

/// The refusal while the switch is off.
pub(crate) fn disabled() -> Status {
    Status::failed_precondition("loop_checkpoint_disabled")
}

/// The AAD scope of one stored fold. It names the run, its owner and the seq, so a
/// ciphertext moved to another row, owner or write does not open.
fn scope_aad(run_id: &str, org_id: &str, user_id: &str, seq: i64) -> String {
    format!("loopfold:{run_id}:org:{org_id}:user:{user_id}:seq:{seq}")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn internal(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

/// Parse a fold and check that it belongs to `run_id` at this version.
pub(crate) fn check_fold(run_id: &str, bytes: &[u8]) -> Result<LoopFoldV1, Status> {
    if bytes.is_empty() || bytes.len() > MAX_FOLD_BYTES {
        return Err(Status::invalid_argument(
            "loop fold is empty or over the size cap",
        ));
    }
    let fold: LoopFoldV1 = serde_json::from_slice(bytes)
        .map_err(|_| Status::invalid_argument("loop fold must be a version 1 JSON object"))?;
    if fold.v != FOLD_VERSION || fold.run_id != run_id {
        return Err(Status::invalid_argument(
            "loop fold belongs to another run or version",
        ));
    }
    Ok(fold)
}

/// The run a fold belongs to, as read under the run row's lock.
struct RunOwner {
    org_id: String,
    user_id: String,
    status: String,
}

/// Whether a run status can no longer change. A run in one of these never resumes.
fn ended(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

/// The refusal for a write or claim on a run that has ended. Closed token: the
/// loop that receives it must stop writing, not retry.
fn run_ended() -> Status {
    Status::failed_precondition("loop_checkpoint_run_ended")
}

/// Lock the run and return its owner. A run that is not `persistent` never keeps a
/// fold, so its refusal is the durable-mutation refusal the rest of session-core uses.
///
/// The `FOR UPDATE` on the runs row serialises this against every terminal status
/// write, and the lock order (runs row, then fold row) matches the purge order.
async fn run_owner_tx(
    tx: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<RunOwner, Status> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT org_id, user_id, retention_class, status FROM runs WHERE id = $1 FOR UPDATE",
    )
    .bind(run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;
    let (org_id, user_id, retention, status) =
        row.ok_or_else(|| Status::not_found("run not found"))?;
    if retention != "persistent" {
        return Err(Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(Code::FailedPrecondition));
    }
    Ok(RunOwner {
        org_id,
        user_id,
        status,
    })
}

/// Write a content-free audit event for a fold, in the caller's transaction.
async fn record_event(
    tx: &mut Transaction<'_, Postgres>,
    run_id: &str,
    org_id: &str,
    user_id: &str,
    event_type: &str,
    type_url: &str,
    seq: i64,
    discriminator: &str,
    kind: &str,
) -> Result<(), Status> {
    let resource = format!("run:{run_id}:loop_checkpoint:{seq}:{event_type}");
    // A save is unique per (run, seq). A claim is a compare-and-set on the same row, so a
    // lapsed lease can be claimed again at the same seq: its key carries the lease expiry,
    // which is unique per successful claim, so a reclaim is a new event, not a duplicate.
    let key = if discriminator.is_empty() {
        format!("{run_id}:{seq}:{event_type}")
    } else {
        format!("{run_id}:{seq}:{event_type}:{discriminator}")
    };
    let idempotency = derive_idempotency_hash("session-core", event_type, &resource, &key);
    sqlx::query(
        "INSERT INTO events (id, event_type, run_id, payload, ts, org_id, user_id, correlation_id, causation_id, idempotency_key, resource_ref, type_url, producer, schema_version)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $3, '', $8, $9, $10, 'session-core', 1)",
    )
    .bind(new_ulid())
    .bind(event_type)
    .bind(run_id)
    .bind(json!({ "seq": seq, "kind": kind }))
    .bind(Utc::now())
    .bind(org_id)
    .bind(user_id)
    .bind(&idempotency)
    .bind(&resource)
    .bind(type_url)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

/// What a save writes. The fold is the LoopFoldV1 JSON the writer built.
pub(crate) struct SaveInput<'a> {
    pub run_id: &'a str,
    pub expected_seq: i64,
    pub kind: &'a str,
    pub fold: &'a [u8],
    pub authority_digest: &'a str,
    pub pause_ref: &'a str,
    pub park_reason: &'a str,
    pub park_until: Option<DateTime<Utc>>,
}

/// Store a run's fold, latest wins. Returns the seq the fold was stored under.
pub(crate) async fn save(
    pool: &PgPool,
    cipher: &DescriptorCipher,
    input: SaveInput<'_>,
) -> Result<i64, Status> {
    let fold = check_fold(input.run_id, input.fold)?;
    if input.authority_digest.trim().is_empty() {
        return Err(Status::invalid_argument("authority digest is required"));
    }
    let segment_no = i32::try_from(fold.segment_no)
        .map_err(|_| Status::invalid_argument("loop fold segment is out of range"))?;
    let plaintext = std::str::from_utf8(input.fold)
        .map_err(|_| Status::invalid_argument("loop fold must be UTF-8 JSON"))?;

    // The state and the pause fields follow from the kind. A round or pre-effect
    // fold holds no pause, so its pause fields are cleared.
    let (state, pause_ref, park_reason, park_until) = match input.kind {
        "round" | "pre_effect" => ("active", None, None, None),
        "pause" => {
            if input.pause_ref.trim().is_empty() {
                return Err(Status::invalid_argument("a pause needs its approval reference"));
            }
            ("paused", Some(input.pause_ref), None, None)
        }
        "park" => {
            if input.pause_ref.trim().is_empty()
                || input.park_reason.trim().is_empty()
                || input.park_until.is_none()
            {
                return Err(Status::invalid_argument(
                    "a park needs its reference, reason and resume time",
                ));
            }
            // The reason is one closed token, never free text: it reaches the
            // run's read model and must not carry content.
            let reason = ParkReason::parse(input.park_reason)
                .ok_or_else(|| Status::invalid_argument("park reason is not recognised"))?;
            (
                "parked",
                Some(input.pause_ref),
                Some(reason.as_str()),
                input.park_until,
            )
        }
        _ => return Err(Status::invalid_argument("loop checkpoint kind is not recognised")),
    };

    let mut tx = pool.begin().await.map_err(internal)?;
    let owner = run_owner_tx(&mut tx, input.run_id).await?;
    if ended(&owner.status) {
        return Err(run_ended());
    }
    let (org_id, user_id) = (owner.org_id, owner.user_id);
    let current: Option<(i64, String)> = sqlx::query_as(
        "SELECT seq, state FROM run_loop_checkpoints WHERE run_id = $1 FOR UPDATE",
    )
    .bind(input.run_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let next_seq = match current {
        None if input.expected_seq == 0 => 1,
        None => return Err(Status::aborted("loop_checkpoint_stale")),
        Some((_, state)) if state == "closed" => {
            return Err(Status::failed_precondition("loop_checkpoint_closed"));
        }
        Some((seq, _)) if seq != input.expected_seq => {
            return Err(Status::aborted("loop_checkpoint_stale"));
        }
        Some((seq, _)) => seq + 1,
    };

    let ciphertext = cipher
        .seal(
            &scope_aad(input.run_id, &org_id, &user_id, next_seq),
            plaintext,
        )
        .map_err(|_| Status::failed_precondition("loop checkpoint key unavailable"))?;
    sqlx::query(
        "INSERT INTO run_loop_checkpoints
            (run_id, org_id, user_id, seq, kind, state, pause_ref, park_reason, park_until,
             authority_digest, segment_no, ciphertext, fold_digest)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
         ON CONFLICT (run_id) DO UPDATE SET
            seq = EXCLUDED.seq, kind = EXCLUDED.kind, state = EXCLUDED.state,
            pause_ref = EXCLUDED.pause_ref, park_reason = EXCLUDED.park_reason,
            park_until = EXCLUDED.park_until, authority_digest = EXCLUDED.authority_digest,
            segment_no = EXCLUDED.segment_no, ciphertext = EXCLUDED.ciphertext,
            fold_digest = EXCLUDED.fold_digest, claim_token_hash = NULL,
            claim_expires_at = NULL, updated_at = now()",
    )
    .bind(input.run_id)
    .bind(&org_id)
    .bind(&user_id)
    .bind(next_seq)
    .bind(input.kind)
    .bind(state)
    .bind(pause_ref)
    .bind(park_reason)
    .bind(park_until)
    .bind(input.authority_digest)
    .bind(segment_no)
    .bind(&ciphertext)
    .bind(sha256_hex(input.fold))
    .execute(&mut *tx)
    .await
    .map_err(internal)?;

    if matches!(input.kind, "pause" | "park") {
        record_event(
            &mut tx,
            input.run_id,
            &org_id,
            &user_id,
            "CHECKPOINT_SAVED",
            crate::grpc::CHECKPOINT_SAVED_TYPE_URL,
            next_seq,
            "",
            input.kind,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(next_seq)
}

/// A fold a continuation has claimed.
#[derive(Debug)]
pub(crate) struct Claimed {
    pub seq: i64,
    pub fold: Vec<u8>,
    pub segment_no: i32,
    pub authority_digest: String,
    pub kind: String,
}

/// Claim a paused or parked run's fold. Exactly one caller wins a given pause: the
/// claim is a conditional update, and a live lease blocks a second claim.
pub(crate) async fn claim(
    pool: &PgPool,
    cipher: &DescriptorCipher,
    run_id: &str,
    pause_ref: &str,
    claim_token: &str,
) -> Result<Claimed, Status> {
    if claim_token.trim().is_empty() {
        return Err(Status::invalid_argument("claim token is required"));
    }
    let token_hash = sha256_hex(claim_token.as_bytes());
    let mut tx = pool.begin().await.map_err(internal)?;
    let owner = run_owner_tx(&mut tx, run_id).await?;
    if ended(&owner.status) {
        return Err(run_ended());
    }
    let (org_id, user_id) = (owner.org_id, owner.user_id);

    // Claimable: a paused or parked fold, or a resuming fold whose lease lapsed.
    // An `active` fold is never claimable: its loop may still be running, and only
    // a worker that parks it after a crash makes it resumable.
    let row: Option<(i64, String, i32, String, String, String)> = sqlx::query_as(
        "UPDATE run_loop_checkpoints
            SET state = 'resuming',
                claim_token_hash = $2,
                claim_expires_at = now() + make_interval(secs => $3),
                updated_at = now()
          WHERE run_id = $1
            AND pause_ref = $4
            AND (state IN ('paused', 'parked')
                 OR (state = 'resuming' AND claim_expires_at < now()))
          RETURNING seq, ciphertext, segment_no, authority_digest, kind, claim_expires_at::text",
    )
    .bind(run_id)
    .bind(&token_hash)
    .bind(CLAIM_LEASE_SECONDS)
    .bind(pause_ref)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let Some((seq, ciphertext, segment_no, authority_digest, kind, lease)) = row else {
        // The same continuation asking again while its own lease is live is told
        // so, which is not the same as losing the pause to someone else.
        let held: Option<(bool,)> = sqlx::query_as(
            "SELECT COALESCE(state = 'resuming' AND claim_token_hash = $2
                             AND claim_expires_at > now(), false)
               FROM run_loop_checkpoints WHERE run_id = $1",
        )
        .bind(run_id)
        .bind(&token_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        return Err(if held.is_some_and(|(held,)| held) {
            Status::aborted("loop_checkpoint_claim_held")
        } else {
            Status::failed_precondition("loop_checkpoint_not_claimable")
        });
    };

    // Open and check the fold before the claim commits, so a fold that does not
    // authenticate leaves the pause where it was.
    let plaintext = cipher
        .open(&scope_aad(run_id, &org_id, &user_id, seq), &ciphertext)
        .map_err(|_| Status::failed_precondition("loop checkpoint did not authenticate"))?;
    check_fold(run_id, plaintext.as_bytes())?;

    record_event(
        &mut tx,
        run_id,
        &org_id,
        &user_id,
        "CHECKPOINT_RESTORED",
        CHECKPOINT_RESTORED_TYPE_URL,
        seq,
        &lease,
        &kind,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Claimed {
        seq,
        fold: plaintext.into_bytes(),
        segment_no,
        authority_digest,
        kind,
    })
}

/// Close a run's fold once its loop has finished. The ciphertext is dropped at
/// once; the row remains so a late writer is refused rather than resurrecting it.
pub(crate) async fn close(
    pool: &PgPool,
    run_id: &str,
    seq: i64,
    reason: &str,
) -> Result<(), Status> {
    let mut tx = pool.begin().await.map_err(internal)?;
    run_owner_tx(&mut tx, run_id).await?;
    let current: Option<(i64, String)> = sqlx::query_as(
        "SELECT seq, state FROM run_loop_checkpoints WHERE run_id = $1 FOR UPDATE",
    )
    .bind(run_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    match current {
        // Nothing was retained, so there is nothing to close.
        None => {}
        Some((_, state)) if state == "closed" => {}
        Some((current_seq, _)) if current_seq != seq => {
            return Err(Status::aborted("loop_checkpoint_stale"));
        }
        Some(_) => {
            sqlx::query(
                "UPDATE run_loop_checkpoints
                    SET state = 'closed', ciphertext = '', claim_token_hash = NULL,
                        claim_expires_at = NULL, park_until = NULL, updated_at = now()
                  WHERE run_id = $1",
            )
            .bind(run_id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        }
    }
    tracing::info!(run_id, seq, reason, "loop checkpoint closed");
    tx.commit().await.map_err(internal)?;
    Ok(())
}

/// Remove a run's fold inside the caller's transaction: at terminalization, erasure
/// and thread delete. Returns the number of rows removed.
pub(crate) async fn purge_run(
    tx: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<u64, sqlx::Error> {
    let removed = sqlx::query("DELETE FROM run_loop_checkpoints WHERE run_id = $1")
        .bind(run_id)
        .execute(&mut **tx)
        .await?;
    Ok(removed.rows_affected())
}

/// Helpers for tests, here and in the run service, that need a scratch database
/// and a seeded fold. Every database helper refuses a database whose name does
/// not say it is a test database.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use serde_json::Value;
    use sqlx::postgres::PgPoolOptions;

    /// A real LoopFoldV1 golden fixture from mp-contracts.
    pub(crate) const FIXTURE: &str =
        include_str!("../../../crates/mp-contracts/tests/fixtures/loop-fold-v1.json");

    pub(crate) async fn scratch_pool() -> Option<PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return None;
        };
        assert!(url.contains("test"), "the scratch database name must say test");
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(&url)
            .await
            .expect("connect pg");
        sqlx::migrate!("./migrations").run(&pool).await.expect("migrate");
        Some(pool)
    }

    pub(crate) fn cipher() -> DescriptorCipher {
        DescriptorCipher::from_key_bytes_for_test(&[7_u8; 32]).expect("test key")
    }

    /// Seed a thread and a run with the given retention class; returns the run
    /// id, the org and the user. The org is unique per call.
    pub(crate) async fn seed_run(pool: &PgPool, retention: &str) -> (String, String, String) {
        let sfx = mp_ids::new_ulid();
        let (thread_id, run_id, org, user) = (
            format!("lc-t-{sfx}"),
            format!("lc-r-{sfx}"),
            format!("lc-org-{sfx}"),
            "u1".to_owned(),
        );
        sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,$3)")
            .bind(&thread_id)
            .bind(&org)
            .bind(&user)
            .execute(pool)
            .await
            .expect("seed thread");
        // A waived run must name its acknowledgement and connection, and no other class may.
        let (waiver_ack, waiver_connection) = if retention == "zdr_waived" {
            (Some("ack-1"), Some("conn-1"))
        } else {
            (None, None)
        };
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id,retention_class,waiver_ack_id,waiver_connection_id)
             VALUES ($1,$2,'g',$3,$4,$5,$6,$7)",
        )
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org)
        .bind(&user)
        .bind(retention)
        .bind(waiver_ack)
        .bind(waiver_connection)
        .execute(pool)
        .await
        .expect("seed run");
        (run_id, org, user)
    }

    /// The golden fold, re-addressed to `run_id`.
    pub(crate) fn fold_for(run_id: &str) -> Vec<u8> {
        let mut value: Value = serde_json::from_str(FIXTURE).expect("fixture json");
        value["run_id"] = json!(run_id);
        serde_json::to_vec(&value).expect("fold json")
    }

    /// Store a fold through the real `save`, the way a loop does. `kind` is
    /// `round`, `pre_effect`, `pause` or `park`; the pause fields follow from it.
    pub(crate) async fn seed_fold(
        pool: &PgPool,
        run_id: &str,
        kind: &str,
        pause_ref: &str,
        park_reason: &str,
        park_until: Option<DateTime<Utc>>,
    ) -> Result<i64, Status> {
        let fold = fold_for(run_id);
        save(
            pool,
            &cipher(),
            SaveInput {
                run_id,
                expected_seq: 0,
                kind,
                fold: &fold,
                authority_digest: "auth-1",
                pause_ref,
                park_reason,
                park_until,
            },
        )
        .await
    }

    /// End a run with a raw write that does not purge its fold, the way a row that
    /// outlived its run (written before the end-of-run guard) came to exist. Kept
    /// here so the source scan that pairs every status write with a purge can
    /// leave this file out.
    pub(crate) async fn end_run_without_purge(pool: &PgPool, run_id: &str) {
        sqlx::query(
            "UPDATE runs SET status = 'cancelled', ended_at = now(), updated_at = now() WHERE id = $1",
        )
        .bind(run_id)
        .execute(pool)
        .await
        .expect("end the run without a purge");
    }

    /// How many fold rows a run has (0 or 1).
    pub(crate) async fn fold_rows(pool: &PgPool, run_id: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM run_loop_checkpoints WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("count fold rows")
    }

    /// The fold's state, or `None` when the run has no fold.
    pub(crate) async fn fold_state(pool: &PgPool, run_id: &str) -> Option<String> {
        sqlx::query_scalar("SELECT state FROM run_loop_checkpoints WHERE run_id = $1")
            .bind(run_id)
            .fetch_optional(pool)
            .await
            .expect("read fold state")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::FIXTURE;
    use super::*;
    use serde_json::Value;

    fn fixture_run_id() -> String {
        let value: Value = serde_json::from_str(FIXTURE).expect("fixture json");
        value["run_id"].as_str().expect("fixture run_id").to_owned()
    }

    #[test]
    fn a_fold_is_accepted_only_for_its_own_run_and_version() {
        let run_id = fixture_run_id();
        assert!(check_fold(&run_id, FIXTURE.as_bytes()).is_ok());
        assert_eq!(
            check_fold("another-run", FIXTURE.as_bytes())
                .expect_err("wrong run")
                .code(),
            tonic::Code::InvalidArgument
        );
        let wrong_version = FIXTURE.replacen("\"v\": 1", "\"v\": 2", 1);
        assert!(check_fold(&run_id, wrong_version.as_bytes()).is_err());
    }

    #[test]
    fn an_empty_or_oversized_fold_is_refused_before_it_is_parsed() {
        assert!(check_fold("run-1", b"").is_err());
        let oversized = vec![b' '; MAX_FOLD_BYTES + 1];
        assert_eq!(
            check_fold("run-1", &oversized)
                .expect_err("over the cap")
                .message(),
            "loop fold is empty or over the size cap"
        );
    }

    #[test]
    fn the_scope_names_the_run_owner_and_seq() {
        assert_eq!(
            scope_aad("run-1", "org-1", "user-1", 4),
            "loopfold:run-1:org:org-1:user:user-1:seq:4"
        );
    }

    #[test]
    fn the_store_is_off_unless_the_switch_is_true() {
        // Read through the same accessor the handlers use; only "true" enables it.
        std::env::remove_var(ENABLED_ENV);
        assert!(!enabled());
        std::env::set_var(ENABLED_ENV, "false");
        assert!(!enabled());
        std::env::set_var(ENABLED_ENV, " TRUE ");
        assert!(enabled());
        std::env::remove_var(ENABLED_ENV);
    }

    #[test]
    fn ended_is_exactly_the_three_terminal_run_statuses() {
        for status in ["completed", "failed", "cancelled"] {
            assert!(ended(status), "{status}");
        }
        for status in ["queued", "running", "awaiting_approval", "", "Completed"] {
            assert!(!ended(status), "{status}");
        }
        let refusal = run_ended();
        assert_eq!(refusal.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refusal.message(), "loop_checkpoint_run_ended");
    }

    #[tokio::test]
    async fn park_reason_accepts_only_the_closed_tokens() {
        use super::test_support::fold_for;
        // The valid tokens are the closed set.
        for token in [
            "credential_window",
            "provider_exhausted",
            "upstream_unavailable",
            "crash_interrupted",
            "reauth_required",
        ] {
            assert!(ParkReason::parse(token).is_some(), "{token}");
        }
        // Anything else is refused before the database is touched, so a lazy pool
        // that cannot connect is enough to prove the refusal.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool");
        let fold = fold_for(&fixture_run_id());
        for bad in ["free text about a user", "CREDENTIAL_WINDOW", "credential_window ", "other"] {
            let refused = save(
                &pool,
                &DescriptorCipher::from_key_bytes_for_test(&[7_u8; 32]).expect("test key"),
                SaveInput {
                    run_id: &fixture_run_id(),
                    expected_seq: 0,
                    kind: "park",
                    fold: &fold,
                    authority_digest: "auth-1",
                    pause_ref: "appr_1",
                    park_reason: bad,
                    park_until: Some(Utc::now()),
                },
            )
            .await
            .expect_err("an unknown park reason is refused");
            assert_eq!(refusal_pair(&refused), ("invalid_argument", "park reason is not recognised"), "{bad:?}");
        }
    }

    fn refusal_pair(status: &Status) -> (&'static str, &str) {
        let code = match status.code() {
            tonic::Code::InvalidArgument => "invalid_argument",
            tonic::Code::FailedPrecondition => "failed_precondition",
            tonic::Code::Aborted => "aborted",
            _ => "other",
        };
        (code, status.message())
    }

    /// A run that ends leaves no fold behind, and that holds only if every place
    /// that writes a terminal status also calls `purge_run`. This is a tripwire,
    /// not a proof: it counts the two in the source of every other file (test code
    /// included), so a new terminal writer that forgets the purge breaks it.
    #[test]
    fn every_run_status_write_outside_the_fold_module_is_paired_with_a_purge() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let (mut status_writes, mut purges) = (0_usize, 0_usize);
        for entry in std::fs::read_dir(&dir).expect("read src") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs")
                || path.file_name().and_then(|name| name.to_str()) == Some("loop_checkpoint.rs")
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read source");
            let normalised = source.split_whitespace().collect::<Vec<_>>().join(" ");
            status_writes += normalised.matches("UPDATE runs SET status").count();
            purges += normalised.matches("purge_run(").count();
        }
        assert!(status_writes >= 3, "the scan found {status_writes} status writes; it is not reading the sources");
        assert_eq!(
            status_writes, purges,
            "every `UPDATE runs SET status` must be paired with a `purge_run(` in the same transaction"
        );
    }

    /// The database-backed tests need a scratch database. Run them with
    /// `DATABASE_URL=... cargo test -p session-core --bin session-core loop_checkpoint -- --ignored --test-threads=1`
    /// against a scratch Postgres (the database name must say it is a test database).
    mod db {
        use super::super::test_support::{
            cipher, fold_for, fold_rows, fold_state, scratch_pool, seed_fold, seed_run,
        };
        use super::*;

        async fn save_round(pool: &PgPool, run_id: &str, expected_seq: i64) -> Result<i64, Status> {
            let fold = fold_for(run_id);
            save(
                pool,
                &cipher(),
                SaveInput {
                    run_id,
                    expected_seq,
                    kind: "round",
                    fold: &fold,
                    authority_digest: "auth-1",
                    pause_ref: "",
                    park_reason: "",
                    park_until: None,
                },
            )
            .await
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn saves_are_compare_and_set_and_a_stale_writer_is_refused() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;

            assert_eq!(save_round(&pool, &run_id, 0).await.expect("first save"), 1);
            assert_eq!(
                save_round(&pool, &run_id, 0)
                    .await
                    .expect_err("a writer that still expects 0 is stale")
                    .code(),
                tonic::Code::Aborted
            );
            assert_eq!(save_round(&pool, &run_id, 1).await.expect("second save"), 2);

            // The stored row is ciphertext: neither the run id nor the fold's
            // plaintext appears in the table.
            let stored: String =
                sqlx::query_scalar("SELECT ciphertext FROM run_loop_checkpoints WHERE run_id = $1")
                    .bind(&run_id)
                    .fetch_one(&pool)
                    .await
                    .expect("stored row");
            assert!(stored.starts_with("v1:"));
            assert!(!stored.contains(&run_id));
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn one_claim_wins_a_pause_and_the_fold_opens_under_its_scope() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            let fold = fold_for(&run_id);
            save(
                &pool,
                &cipher(),
                SaveInput {
                    run_id: &run_id,
                    expected_seq: 0,
                    kind: "pause",
                    fold: &fold,
                    authority_digest: "auth-1",
                    pause_ref: "appr_1",
                    park_reason: "",
                    park_until: None,
                },
            )
            .await
            .expect("pause save");

            let claimed = claim(&pool, &cipher(), &run_id, "appr_1", "token-a")
                .await
                .expect("first claim wins");
            assert_eq!(claimed.seq, 1);
            assert_eq!(claimed.kind, "pause");
            assert_eq!(claimed.segment_no, 1);
            let restored: Value = serde_json::from_slice(&claimed.fold).expect("restored fold");
            let original: Value = serde_json::from_slice(&fold).expect("original fold");
            assert_eq!(restored, original);

            // A second continuation, or one aimed at another pause, finds nothing to claim.
            assert_eq!(
                claim(&pool, &cipher(), &run_id, "appr_1", "token-b")
                    .await
                    .expect_err("second claim loses")
                    .message(),
                "loop_checkpoint_not_claimable"
            );
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_zdr_or_waived_run_never_keeps_a_fold() {
            let Some(pool) = scratch_pool().await else { return };
            for retention in ["zdr", "zdr_waived"] {
                let (run_id, _org, _user) = seed_run(&pool, retention).await;
                let refused = save_round(&pool, &run_id, 0)
                    .await
                    .expect_err("a non-persistent run keeps no fold");
                assert_eq!(refused.code(), tonic::Code::FailedPrecondition, "{retention}");
                let rows: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM run_loop_checkpoints WHERE run_id = $1")
                        .bind(&run_id)
                        .fetch_one(&pool)
                        .await
                        .expect("count");
                assert_eq!(rows, 0, "{retention}");
            }
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn close_refuses_late_writers_and_purge_removes_the_fold() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            assert_eq!(save_round(&pool, &run_id, 0).await.expect("save"), 1);

            close(&pool, &run_id, 1, "run finished").await.expect("close");
            let state: String =
                sqlx::query_scalar("SELECT state FROM run_loop_checkpoints WHERE run_id = $1")
                    .bind(&run_id)
                    .fetch_one(&pool)
                    .await
                    .expect("state");
            assert_eq!(state, "closed");
            assert_eq!(
                save_round(&pool, &run_id, 1)
                    .await
                    .expect_err("a closed fold takes no writes")
                    .message(),
                "loop_checkpoint_closed"
            );

            let mut tx = pool.begin().await.expect("begin");
            assert_eq!(purge_run(&mut tx, &run_id).await.expect("purge"), 1);
            tx.commit().await.expect("commit");
            let rows: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM run_loop_checkpoints WHERE run_id = $1")
                    .bind(&run_id)
                    .fetch_one(&pool)
                    .await
                    .expect("count");
            assert_eq!(rows, 0);
        }

        // -- Purge on every run-end path, and the ended-run guard (WS03 S4) ------------
        //
        // Every test below is written for a scratch database and is `#[ignore]`d. The
        // names share the `loop_checkpoint_` prefix so one filter runs them all.

        use mp_contracts::model_plane::v1 as pb;
        use mp_contracts::model_plane::v1::run_service_server::RunService as _;

        fn as_caller<T>(message: T, org_id: &str, user_id: &str) -> tonic::Request<T> {
            let mut request = tonic::Request::new(message);
            request
                .extensions_mut()
                .insert(crate::auth::VerifiedIdentity::user_for_test(org_id, user_id));
            request
        }

        /// A thread and a persistent managed run (queued) whose terminal source is
        /// `source`; returns the run id, the org and the user.
        async fn seed_managed_run(
            pool: &PgPool,
            source: pb::ManagedRunSource,
        ) -> (String, String, String) {
            let sfx = mp_ids::new_ulid();
            let (thread_id, org, user) = (
                format!("lc-mt-{sfx}"),
                format!("lc-morg-{sfx}"),
                "u1".to_owned(),
            );
            sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,$3)")
                .bind(&thread_id)
                .bind(&org)
                .bind(&user)
                .execute(pool)
                .await
                .expect("seed thread");
            let started = crate::terminalization::start_managed_run_inner(
                pool,
                pb::StartManagedRunRequest {
                    thread_id,
                    agent_id: "managed-agent".to_owned(),
                    goal: "g".to_owned(),
                    mode: "execute".to_owned(),
                    org_id: org.clone(),
                    user_id: user.clone(),
                    start_key: format!("lc-start-{sfx}"),
                    terminal_source: source as i32,
                    ..Default::default()
                },
                false,
            )
            .await
            .expect("start managed run");
            (started.run_id, org, user)
        }

        /// End a run through the production `CancelRun`.
        async fn cancel_through_the_service(pool: &PgPool, run_id: &str, org: &str, user: &str) {
            let cancelled = crate::run_service_grpc::RunServiceImpl::new(pool.clone())
                .cancel_run(as_caller(
                    pb::CancelRunRequest {
                        run_id: run_id.to_owned(),
                        reason: "test".to_owned(),
                    },
                    org,
                    user,
                ))
                .await
                .expect("cancel run")
                .into_inner()
                .cancelled;
            assert!(cancelled, "the run was live, so the cancel took effect");
        }

        /// End a managed run through `RecordTerminalOutcome` as its execution agent.
        async fn complete_through_the_receipt(pool: &PgPool, run_id: &str) {
            crate::terminalization::record_terminal_outcome_inner(
                pool,
                pb::RecordTerminalOutcomeRequest {
                    run_id: run_id.to_owned(),
                    source: pb::ManagedRunSource::ExecutionAgent as i32,
                    outcome: pb::TerminalOutcome::Completed as i32,
                    failure_code: String::new(),
                },
            )
            .await
            .expect("record terminal outcome");
        }

        async fn run_status(pool: &PgPool, run_id: &str) -> String {
            sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_one(pool)
                .await
                .expect("run status")
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_cancel_run_removes_the_fold() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, org, user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &run_id, "round", "", "", None)
                .await
                .expect("seed fold");
            assert_eq!(fold_rows(&pool, &run_id).await, 1);

            cancel_through_the_service(&pool, &run_id, &org, &user).await;

            assert_eq!(run_status(&pool, &run_id).await, "cancelled");
            assert_eq!(fold_rows(&pool, &run_id).await, 0);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_recorded_terminal_outcome_removes_the_fold() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, _org, _user) =
                seed_managed_run(&pool, pb::ManagedRunSource::ExecutionAgent).await;
            seed_fold(&pool, &run_id, "round", "", "", None)
                .await
                .expect("seed fold");
            assert_eq!(fold_rows(&pool, &run_id).await, 1);

            complete_through_the_receipt(&pool, &run_id).await;

            assert_eq!(run_status(&pool, &run_id).await, "completed");
            assert_eq!(fold_rows(&pool, &run_id).await, 0);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_recovery_terminalization_removes_the_fold() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, _org, _user) =
                seed_managed_run(&pool, pb::ManagedRunSource::ExecutionAgent).await;
            seed_fold(&pool, &run_id, "round", "", "", None)
                .await
                .expect("seed fold");
            let state: String = sqlx::query_scalar(
                "SELECT state FROM managed_run_terminalization_outbox WHERE run_id = $1",
            )
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("obligation state");
            assert_eq!(state, "open", "the obligation is waiting for its producer");

            // The producer is gone: its deadline and retry time are in the past.
            sqlx::query(
                "UPDATE managed_run_terminalization_outbox
                    SET deadline_at = now() - interval '1 hour',
                        next_attempt_at = now() - interval '1 hour'
                  WHERE run_id = $1",
            )
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("age the obligation");

            // The recovery worker claims any due obligation in the database, so this
            // asserts on this run rather than on the count it recovered.
            let recovered = crate::terminalization::recover_due_terminalizations(&pool)
                .await
                .expect("recover");
            assert!(recovered >= 1);

            assert_eq!(run_status(&pool, &run_id).await, "failed");
            assert_eq!(fold_rows(&pool, &run_id).await, 0);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_org_erasure_removes_the_fold() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, org, _user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &run_id, "round", "", "", None)
                .await
                .expect("seed fold");
            assert_eq!(fold_rows(&pool, &run_id).await, 1);

            crate::gdpr::purge_organization_data(&pool, &org, None)
                .await
                .expect("erase the org");

            assert_eq!(fold_rows(&pool, &run_id).await, 0);
            let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE id = $1")
                .bind(&run_id)
                .fetch_one(&pool)
                .await
                .expect("count runs");
            assert_eq!(runs, 0, "the run goes with the fold");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_save_for_an_ended_run_is_refused_and_writes_nothing() {
            let Some(pool) = scratch_pool().await else { return };

            // Path 1: the production cancel. Path 2: a recorded terminal outcome.
            for path in ["cancel", "terminal_outcome"] {
                let (run_id, org, user) = if path == "cancel" {
                    seed_run(&pool, "persistent").await
                } else {
                    seed_managed_run(&pool, pb::ManagedRunSource::ExecutionAgent).await
                };
                assert_eq!(save_round(&pool, &run_id, 0).await.expect("first save"), 1);
                if path == "cancel" {
                    cancel_through_the_service(&pool, &run_id, &org, &user).await;
                } else {
                    complete_through_the_receipt(&pool, &run_id).await;
                }
                assert_eq!(fold_rows(&pool, &run_id).await, 0, "{path}: the end purged the fold");

                // A late writer that starts over (seq 0) must not recreate the fold,
                // and one that still holds the old seq (1) is refused the same way.
                for expected_seq in [0, 1] {
                    let refused = save_round(&pool, &run_id, expected_seq)
                        .await
                        .expect_err("an ended run takes no fold");
                    assert_eq!(refused.code(), tonic::Code::FailedPrecondition, "{path}");
                    assert_eq!(refused.message(), "loop_checkpoint_run_ended", "{path}");
                }
                assert_eq!(fold_rows(&pool, &run_id).await, 0, "{path}: nothing was written");
            }
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_claim_on_an_ended_run_is_refused() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &run_id, "pause", "appr_1", "", None)
                .await
                .expect("pause fold");
            // A row that outlived its run, as one written before the guard would:
            // the run is ended by a raw write that does not purge.
            super::super::test_support::end_run_without_purge(&pool, &run_id).await;

            let refused = claim(&pool, &cipher(), &run_id, "appr_1", "token-a")
                .await
                .expect_err("an ended run's fold is not claimable");
            assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
            assert_eq!(refused.message(), "loop_checkpoint_run_ended");

            // The row is untouched: still paused, no claim recorded.
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("paused"));
            let claimed: Option<String> = sqlx::query_scalar(
                "SELECT claim_token_hash FROM run_loop_checkpoints WHERE run_id = $1",
            )
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("claim hash");
            assert_eq!(claimed, None);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_a_stale_active_row_is_not_claimable() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            assert_eq!(save_round(&pool, &run_id, 0).await.expect("active fold"), 1);
            // Not written for far longer than the old 900 second window.
            sqlx::query(
                "UPDATE run_loop_checkpoints SET updated_at = now() - interval '1 hour' WHERE run_id = $1",
            )
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("age the row");

            // Whatever pause reference the caller names, an active fold is not
            // claimable: only a worker that parks it after a crash makes it so.
            for pause_ref in ["", "appr_1", "anything"] {
                let refused = claim(&pool, &cipher(), &run_id, pause_ref, "token-a")
                    .await
                    .expect_err("a stale active fold is not a pause");
                assert_eq!(refused.code(), tonic::Code::FailedPrecondition, "{pause_ref:?}");
                assert_eq!(refused.message(), "loop_checkpoint_not_claimable", "{pause_ref:?}");
            }
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("active"));
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_claim_held_is_told_apart_from_losing_the_pause() {
            let Some(pool) = scratch_pool().await else { return };
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &run_id, "pause", "appr_1", "", None)
                .await
                .expect("pause fold");
            claim(&pool, &cipher(), &run_id, "appr_1", "token-a")
                .await
                .expect("first claim wins");

            // The same continuation asking again while its lease is live: held.
            let held = claim(&pool, &cipher(), &run_id, "appr_1", "token-a")
                .await
                .expect_err("the lease is live");
            assert_eq!(held.code(), tonic::Code::Aborted);
            assert_eq!(held.message(), "loop_checkpoint_claim_held");

            // Another continuation lost the pause: not claimable.
            let lost = claim(&pool, &cipher(), &run_id, "appr_1", "token-b")
                .await
                .expect_err("another claimant lost");
            assert_eq!(lost.code(), tonic::Code::FailedPrecondition);
            assert_eq!(lost.message(), "loop_checkpoint_not_claimable");

            // Once the lease lapses the pause is claimable again, by anyone.
            sqlx::query(
                "UPDATE run_loop_checkpoints SET claim_expires_at = now() - interval '1 second' WHERE run_id = $1",
            )
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("lapse the lease");
            let reclaimed = claim(&pool, &cipher(), &run_id, "appr_1", "token-b")
                .await
                .expect("a lapsed lease is claimable");
            assert_eq!(reclaimed.kind, "pause");
            // And the previous holder, whose lease lapsed, is no longer "held".
            let stale_holder = claim(&pool, &cipher(), &run_id, "appr_1", "token-a")
                .await
                .expect_err("token-b holds it now");
            assert_eq!(stale_holder.message(), "loop_checkpoint_not_claimable");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_a_park_stores_only_a_closed_reason() {
            let Some(pool) = scratch_pool().await else { return };
            let until = Utc::now() + chrono::Duration::hours(1);

            let (bad_run, _org, _user) = seed_run(&pool, "persistent").await;
            let refused = seed_fold(&pool, &bad_run, "park", "appr_1", "free text", Some(until))
                .await
                .expect_err("free text is not a park reason");
            assert_eq!(refused.code(), tonic::Code::InvalidArgument);
            assert_eq!(fold_rows(&pool, &bad_run).await, 0, "nothing was written");

            for reason in mp_contracts::loop_fold::ParkReason::ALL {
                let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
                seed_fold(&pool, &run_id, "park", "appr_1", reason.as_str(), Some(until))
                    .await
                    .expect("a closed token is stored");
                let stored: Option<String> = sqlx::query_scalar(
                    "SELECT park_reason FROM run_loop_checkpoints WHERE run_id = $1",
                )
                .bind(&run_id)
                .fetch_one(&pool)
                .await
                .expect("stored reason");
                assert_eq!(stored.as_deref(), Some(reason.as_str()));
                assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("parked"));
            }
        }
    }
}
