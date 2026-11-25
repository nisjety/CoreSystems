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
use mp_contracts::loop_fold::{
    child_ceiling_in, park_ref, parks_in, route_connection_in, route_model_in, LoopFoldV1,
    ParkReason, PauseKind, PendingPause, EXT_EFFECT_LEDGER, EXT_PARKS, FOLD_VERSION,
    MAX_PARKS_PER_RUN,
};
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

/// How long a parked run waits for its continuation, in seconds, when the process names no
/// other (build plan WS03 S8-4): one day.
const PARK_TTL_DEFAULT_SECONDS: i64 = 86_400;
/// The shortest and the longest wait the process may name: a minute and thirty days.
const PARK_TTL_MIN_SECONDS: i64 = 60;
const PARK_TTL_MAX_SECONDS: i64 = 2_592_000;
const PARK_TTL_ENV: &str = "SESSION_CORE_PARK_TTL_SECONDS";

/// How long a crash park waits before a continuation may claim it, and how far every heartbeat
/// of a run that proves to be alive pushes it on (build plan WS03 S8-B): the recovery worker's
/// own deadline period, so a loop that is silent for less than two periods is never continued
/// under itself. Counted on the database clock only.
pub(crate) const CRASH_PARK_GRACE_SECONDS: f64 = 900.0;

/// The extension key of the thread position (the highest message sequence of the run's thread)
/// a round fold was saved at. [`save`] stamps it while crash parking is on, and the recovery
/// worker and the claim of a crash park compare it with the thread as it stands: a message
/// above it means the run's own answer, or another turn, already reached the thread, and a
/// continuation would answer it a second time. A fold without it is never crash parked. It is
/// a number, never content.
const THREAD_WATERMARK_KEY: &str = "thread_watermark";

/// The round budget a fold's authority resolves to, as execution-core resolves it
/// (`resume::resolved_max_rounds`): zero means the default, and nothing exceeds the ceiling.
/// **KEEP IN SYNC with `runtime_loop::agent::{DEFAULT_MAX_ROUNDS, MAX_ROUNDS_CEILING}`.** A drift
/// can only make a park the executor then refuses to resume, never a wrong resume.
const DEFAULT_ROUND_BUDGET: u32 = 12;
const ROUND_BUDGET_CEILING: u32 = 32;

/// The wait in `value`, a whole number of seconds clamped to
/// `[PARK_TTL_MIN_SECONDS, PARK_TTL_MAX_SECONDS]`; the default when it is absent or is not one.
fn park_ttl_from(value: Option<&str>) -> i64 {
    value
        .and_then(|text| text.trim().parse::<i64>().ok())
        .map_or(PARK_TTL_DEFAULT_SECONDS, |seconds| {
            seconds.clamp(PARK_TTL_MIN_SECONDS, PARK_TTL_MAX_SECONDS)
        })
}

/// How long a parked run waits for a continuation before the recovery worker ends it
/// (`outcome_unknown`), counted from the park save: `SESSION_CORE_PARK_TTL_SECONDS`, one day
/// by default, clamped to a minute..thirty days. Only read while park recovery is active.
pub(crate) fn park_ttl_seconds() -> i64 {
    park_ttl_from(std::env::var(PARK_TTL_ENV).ok().as_deref())
}

/// Whether the fold store is switched on. Off by default: a loop keeps its fold in
/// memory and a pause cannot be resumed from the store. Only the exact literal `true`
/// enables it, with no trimming and no case folding, as every durable-loop switch reads.
pub(crate) fn enabled() -> bool {
    // A test that serves an RPC through its trait method forces the answer for its own thread,
    // so it does not race the tests that set the process environment. Compiled out of the binary.
    #[cfg(test)]
    if let Some(forced) = test_switch::forced() {
        return forced;
    }
    std::env::var(ENABLED_ENV).ok().as_deref() == Some("true")
}

/// A per-thread override of [`enabled`] for tests. It holds only for a test that runs its call
/// on its own thread (a `#[tokio::test]` is single-threaded), and restores the earlier value
/// when the guard drops.
#[cfg(test)]
pub(crate) mod test_switch {
    use std::cell::Cell;

    thread_local! {
        static FORCED: Cell<Option<bool>> = const { Cell::new(None) };
    }

    pub(crate) fn forced() -> Option<bool> {
        FORCED.with(Cell::get)
    }

    pub(crate) struct Forced(Option<bool>);

    impl Forced {
        pub(crate) fn set(on: bool) -> Self {
            Self(FORCED.with(|cell| cell.replace(Some(on))))
        }
    }

    impl Drop for Forced {
        fn drop(&mut self) {
            FORCED.with(|cell| cell.set(self.0));
        }
    }
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
    // lapsed lease can be claimed again at the same seq: its key carries a fresh ULID, unique
    // per successful claim and independent of any clock, so a reclaim is a new event, never a
    // duplicate. (An earlier version keyed on the lease expiry; that depends on the database
    // clock moving between two claims, which a disturbed VM clock does not guarantee.)
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

/// What a save stored: the seq the fold was stored under, and whether the store took it as a
/// park under the park rules (build plan WS03 S8-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Saved {
    pub seq: i64,
    /// True for exactly one path: a `park` fold saved while [`park_recovery_active`], after every
    /// park rule held. False for every other save, including a park stored under the committed
    /// rules while park recovery is off. The executor parks a run only on a true echo, so a store
    /// that does not know the park rules (or has them off) leaves the run its committed outcome.
    pub park_accepted: bool,
}

/// The park rules of a save, checked before the store is touched (build plan WS03 S8-4).
///
/// `until` is the park's resume time, `now` and `ttl` (seconds) the clock and the park wait. A
/// park is a fold the loop wrote at a round boundary (no call in flight, a pending park), under
/// the reference of the run's own next park, within the park cap and the wait. `crash_interrupted`
/// is reserved: no executor writes it, so a store that takes it would be taking a forgery.
fn validate_park_save(
    run_id: &str,
    fold: &LoopFoldV1,
    input: &SaveInput<'_>,
    reason: ParkReason,
    until: DateTime<Utc>,
    now: DateTime<Utc>,
    ttl: i64,
) -> Result<(), Status> {
    if reason == ParkReason::CrashInterrupted {
        return Err(Status::invalid_argument("park_reason_reserved"));
    }
    let at_a_round_boundary = fold.round_in_flight.is_none()
        && fold.pending.as_ref().is_some_and(|pending| {
            pending.kind == PauseKind::Park && pending.pause_ref == input.pause_ref
        });
    if !at_a_round_boundary {
        return Err(Status::invalid_argument("park_not_at_round_boundary"));
    }
    let parks = parks_in(fold);
    if parks > MAX_PARKS_PER_RUN {
        return Err(Status::failed_precondition("park_limit_reached"));
    }
    if parks == 0 || input.pause_ref != park_ref(run_id, parks) {
        return Err(Status::invalid_argument("park_ref_mismatch"));
    }
    // A wait longer than half the park's own life would have the worker end the run before it
    // could be continued, so such a park is refused and the run takes its committed outcome.
    if until > now + chrono::Duration::seconds(ttl / 2) {
        return Err(Status::failed_precondition("park_until_beyond_ttl"));
    }
    Ok(())
}

/// `fold` with the thread position stamped into its extensions, or `None` when it cannot be (the
/// fold is not an object with an object `extensions`, or the stamped fold would pass the size
/// cap). Such a fold is stored as it came and is never crash parked.
fn stamp_thread_watermark(fold: &str, position: i64) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(fold).ok()?;
    value
        .get_mut("extensions")?
        .as_object_mut()?
        .insert(THREAD_WATERMARK_KEY.to_owned(), json!(position));
    let stamped = serde_json::to_string(&value).ok()?;
    (stamped.len() <= MAX_FOLD_BYTES).then_some(stamped)
}

/// The highest message sequence of the run's thread, read under the thread's row lock so an
/// append in flight is either counted or waited for. `None` when an append (or any other holder)
/// has the thread locked: it is never waited for here, because the answer-append path takes the
/// thread lock before the run's and a caller holding the run's lock would deadlock with it. A
/// thread with no messages reads 0.
async fn thread_position_locked(
    tx: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<Option<i64>, Status> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT (SELECT COALESCE(MAX(m.sequence), 0) FROM messages AS m
                  WHERE m.thread_id = t.id)::bigint
           FROM threads AS t JOIN runs AS r ON r.thread_id = t.id
          WHERE r.id = $1
            FOR UPDATE OF t SKIP LOCKED",
    )
    .bind(run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(row.map(|(position,)| position))
}

/// Whether a save is an input pause: a `pause` fold under the reference of a question or an
/// elicitation the run waits on, not an approval's `{run}:{step}` reference.
fn is_input_pause_save(kind: &str, pause_ref: &str) -> bool {
    kind == "pause" && mp_contracts::user_question::is_input_pause_ref(pause_ref)
}

/// Where a thread stands against the position a fold was stamped at.
#[derive(Debug, PartialEq, Eq)]
enum ThreadSince {
    /// Nothing was appended above the stamp.
    Unmoved,
    /// A message sits above the stamp, or the stamp is not an integer (fail closed).
    Moved,
    /// An append holds the thread; the caller rolls back and the fold stays as it was.
    Busy,
}

/// Compare the stamp in a fold's extensions with the thread as it stands, under the thread's lock
/// (see [`thread_position_locked`]). A stamp that is not an integer counts as moved.
async fn thread_since(
    tx: &mut Transaction<'_, Postgres>,
    run_id: &str,
    stamp: &serde_json::Value,
) -> Result<ThreadSince, Status> {
    let Some(position) = thread_position_locked(tx, run_id).await? else {
        return Ok(ThreadSince::Busy);
    };
    Ok(
        if stamp.as_i64().is_none_or(|watermark| position > watermark) {
            ThreadSince::Moved
        } else {
            ThreadSince::Unmoved
        },
    )
}

/// Close a claimed fold whose thread moved: the content is dropped and the row can no longer be
/// claimed. The caller commits.
async fn close_moved_fold(tx: &mut Transaction<'_, Postgres>, run_id: &str) -> Result<(), Status> {
    sqlx::query(
        "UPDATE run_loop_checkpoints
            SET state = 'closed', ciphertext = '', claim_token_hash = NULL,
                claim_expires_at = NULL, park_until = NULL, updated_at = now()
          WHERE run_id = $1",
    )
    .bind(run_id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

/// Store a run's fold, latest wins. Returns the seq the fold was stored under and whether it was
/// accepted as a park.
pub(crate) async fn save(
    pool: &PgPool,
    cipher: &DescriptorCipher,
    input: SaveInput<'_>,
) -> Result<Saved, Status> {
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
    let mut park_accepted = false;
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
            // The park rules apply only while park recovery is on, and they run before the run
            // row is locked or anything is written. With them off a park is stored exactly as it
            // always was, and the store does not say it accepted it.
            if crate::terminalization::park_recovery_active() {
                if let Some(until) = input.park_until {
                    validate_park_save(
                        input.run_id,
                        &fold,
                        &input,
                        reason,
                        until,
                        Utc::now(),
                        park_ttl_seconds(),
                    )?;
                    park_accepted = true;
                }
            }
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

    // While crash parking is on, a round fold also records how far its thread had got. An unlocked
    // read is enough: a message that lands after it only makes the later comparison refuse.
    // An input-pause fold (a question or elicitation the run waits on) is stamped the same way
    // whatever the crash-park switch says: its claim refuses the answer once the thread moved.
    let stamped = if (input.kind == "round" && crate::terminalization::crash_park_active())
        || is_input_pause_save(input.kind, input.pause_ref)
    {
        let position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(m.sequence), 0)::bigint FROM messages AS m
              WHERE m.thread_id = (SELECT thread_id FROM runs WHERE id = $1)",
        )
        .bind(input.run_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
        stamp_thread_watermark(plaintext, position)
    } else {
        None
    };
    let plaintext = stamped.as_deref().unwrap_or(plaintext);

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
    Ok(Saved {
        seq: next_seq,
        park_accepted,
    })
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

/// The claim of a run's CURRENT parked fold, by an empty pause reference (build plan WS03 S8-4,
/// only while [`crate::terminalization::park_recovery_active`]). It takes a `kind = 'park'` row
/// only, in state `parked` or `resuming` under a lapsed lease, and only once `park_until` has
/// come: a paused fold is never claimed by it. It deliberately does NOT set `updated_at`: that
/// column is the park's age, the anchor the recovery worker counts the park's wait from, and a
/// claim is not a new park.
const CLAIM_PARK_SQL: &str = "UPDATE run_loop_checkpoints
            SET state = 'resuming',
                claim_token_hash = $2,
                claim_expires_at = now() + make_interval(secs => $3)
          WHERE run_id = $1
            AND kind = 'park'
            AND (state = 'parked'
                 OR (state = 'resuming' AND claim_expires_at < now()))
            AND park_until <= now()
          RETURNING seq, ciphertext, segment_no, authority_digest, kind";

/// Why an empty-reference claim found nothing to take, as the one diagnostic read after it:
/// `park_not_due` for a park that is claimable but whose `park_until` has not come, a held lease
/// (ANY token's: an empty reference names no pause to compare a token against) for a park that
/// is being resumed, and the committed `not_claimable` for everything else (no fold, a paused or
/// active or closed one). Nothing is written, so a refused claim leaves the park exactly as it was.
async fn park_claim_refusal(
    tx: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<Status, Status> {
    let row: Option<(bool, bool)> = sqlx::query_as(
        "SELECT COALESCE(kind = 'park' AND park_until > now()
                         AND (state = 'parked'
                              OR (state = 'resuming' AND claim_expires_at < now())), false),
                COALESCE(kind = 'park' AND state = 'resuming' AND claim_expires_at > now(), false)
           FROM run_loop_checkpoints WHERE run_id = $1",
    )
    .bind(run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(match row {
        Some((true, _)) => Status::failed_precondition("park_not_due"),
        Some((_, true)) => Status::aborted("loop_checkpoint_claim_held"),
        _ => Status::failed_precondition("loop_checkpoint_not_claimable"),
    })
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

    // An empty reference names no pause: under the park rules it asks for the run's current
    // parked fold (build plan WS03 S8-4). With them off it is the committed statement below,
    // which an empty reference matches no row of.
    let park_claim = pause_ref.is_empty() && crate::terminalization::park_recovery_active();
    // A named reference that is a question's or elicitation's: it gets the thread checks below.
    let input_pause_claim =
        !park_claim && mp_contracts::user_question::is_input_pause_ref(pause_ref);

    // Under the park rules a park is claimed by the empty reference and by nothing else. Its own
    // reference (`park:{run_id}:{n}`) is computable by anyone from the run id, and the committed
    // statement below ignores `park_until` and sets `updated_at`, the anchor of the park's TTL: a
    // caller holding this RPC could re-claim whenever the lease lapses, move the anchor and read
    // the fold each time, and the park would never reach its TTL. So a named reference against a
    // run whose current fold is a park is refused as not claimable. The kind is read under the
    // run row's lock (taken above, and by every save), so it cannot change before the statement.
    // A pause fold, which an approval resume claims by its reference, is untouched, and with the
    // park rules off nothing here runs.
    if !park_claim && !pause_ref.is_empty() && crate::terminalization::park_recovery_active() {
        let current: Option<(String,)> =
            sqlx::query_as("SELECT kind FROM run_loop_checkpoints WHERE run_id = $1")
                .bind(run_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(internal)?;
        if current.is_some_and(|(kind,)| kind == "park") {
            return Err(Status::failed_precondition("loop_checkpoint_not_claimable"));
        }
    }

    // Claimable: a paused or parked fold, or a resuming fold whose lease lapsed.
    // An `active` fold is never claimable: its loop may still be running, and only
    // a worker that parks it after a crash makes it resumable.
    let row: Option<(i64, String, i32, String, String)> = if park_claim {
        sqlx::query_as(CLAIM_PARK_SQL)
            .bind(run_id)
            .bind(&token_hash)
            .bind(CLAIM_LEASE_SECONDS)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal)?
    } else {
        sqlx::query_as(
        "UPDATE run_loop_checkpoints
            SET state = 'resuming',
                claim_token_hash = $2,
                claim_expires_at = now() + make_interval(secs => $3),
                updated_at = now()
          WHERE run_id = $1
            AND pause_ref = $4
            AND (state IN ('paused', 'parked')
                 OR (state = 'resuming' AND claim_expires_at < now()))
          RETURNING seq, ciphertext, segment_no, authority_digest, kind",
        )
        .bind(run_id)
        .bind(&token_hash)
        .bind(CLAIM_LEASE_SECONDS)
        .bind(pause_ref)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?
    };
    let Some((seq, ciphertext, segment_no, authority_digest, kind)) = row else {
        if park_claim {
            return Err(park_claim_refusal(&mut tx, run_id).await?);
        }
        // A different continuation holding a live lease on this same input pause: the caller
        // learns another continuation is applying the answer, which is not a gone pause.
        if input_pause_claim {
            let elsewhere: Option<(bool,)> = sqlx::query_as(
                "SELECT COALESCE(state = 'resuming' AND claim_expires_at > now()
                                 AND pause_ref = $3 AND claim_token_hash IS DISTINCT FROM $2,
                                 false)
                   FROM run_loop_checkpoints WHERE run_id = $1",
            )
            .bind(run_id)
            .bind(&token_hash)
            .bind(pause_ref)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal)?;
            if elsewhere.is_some_and(|(elsewhere,)| elsewhere) {
                return Err(Status::aborted("loop_checkpoint_claimed_elsewhere"));
            }
        }
        // The same continuation asking again, for the same pause, while its own lease
        // is live is told so, which is not the same as losing the pause to someone
        // else. The pause reference is part of the question: a holder of the token who
        // names another pause is not holding that one.
        let held: Option<(bool,)> = sqlx::query_as(
            "SELECT COALESCE(state = 'resuming' AND claim_token_hash = $2
                             AND pause_ref = $3 AND claim_expires_at > now(), false)
               FROM run_loop_checkpoints WHERE run_id = $1",
        )
        .bind(run_id)
        .bind(&token_hash)
        .bind(pause_ref)
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
    let claimed_fold = check_fold(run_id, plaintext.as_bytes())?;

    // A crash park (it carries the thread position its round fold was saved at) is continued only
    // while its thread has not moved: a message above that position is the run's own answer, or
    // another turn, and a continuation would answer it again. The check runs under the thread's
    // lock, so an append in flight is counted. It is keyed on the stamp, not on the crash-park
    // switch: a park written before the switch went off stays checked. A thread held by an
    // append is not waited for (see `thread_position_locked`): the claim is refused and rolls
    // back, and the park stays as it was.
    if park_claim {
        if let Some(stamp) = claimed_fold.extensions.get(THREAD_WATERMARK_KEY) {
            match thread_since(&mut tx, run_id, stamp).await? {
                ThreadSince::Busy => return Err(Status::aborted("loop_checkpoint_claim_held")),
                ThreadSince::Moved => {
                    close_moved_fold(&mut tx, run_id).await?;
                    tx.commit().await.map_err(internal)?;
                    return Err(Status::failed_precondition("loop_checkpoint_not_claimable"));
                }
                ThreadSince::Unmoved => {}
            }
        }
    }

    // An input pause (a question the run waits on) is answered only while its thread has not
    // moved: a newer turn leaves the old run's context stale. The stamp is written by `save`; a
    // fold without one, or with one that is not an integer, counts as moved. A thread held by an
    // append is not waited for: the claim rolls back and the fold stays paused. Approval and park
    // claims never reach this branch.
    if input_pause_claim {
        let moved = match claimed_fold.extensions.get(THREAD_WATERMARK_KEY) {
            None => true,
            Some(stamp) => match thread_since(&mut tx, run_id, stamp).await? {
                ThreadSince::Busy => return Err(Status::aborted("loop_checkpoint_thread_busy")),
                ThreadSince::Moved => true,
                ThreadSince::Unmoved => false,
            },
        };
        if moved {
            close_moved_fold(&mut tx, run_id).await?;
            tx.commit().await.map_err(internal)?;
            return Err(Status::failed_precondition("loop_checkpoint_thread_moved"));
        }
    }

    record_event(
        &mut tx,
        run_id,
        &org_id,
        &user_id,
        "CHECKPOINT_RESTORED",
        CHECKPOINT_RESTORED_TYPE_URL,
        seq,
        &new_ulid(),
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

/// What the recovery worker found when it asked to park a crashed run's fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CrashPark {
    /// The fold was re-sealed as a `crash_interrupted` park, stored under this seq.
    Parked(i64),
    /// The fold is not parkable, for this closed reason. Nothing was written; the caller takes
    /// the committed path.
    Refused(&'static str),
    /// The run's thread is locked by a writer, so the run's producer is alive and finishing. Nothing
    /// was written; the caller leaves the run alone for now.
    Busy,
}

/// The first reason a stored round fold may not be crash parked, or `None` when it may. A crash
/// park is held for the park wait, so it is made only when a continuation could actually resume
/// it: every rule here is one `park::check_park_fold` applies to the fold on resume (or a fact it
/// cannot see), and a fold that fails one would be held for the wait and then refused.
///
/// `thread_position` is the highest message sequence of the run's thread now. The fold carries
/// the executor's markers only on a run that was parkable when it started (a fresh root request
/// with a derivable tool set, never an approval continuation): the route keys, the park count
/// and the sub-agent ceiling, written on its round folds.
fn crash_park_refusal(
    fold: &LoopFoldV1,
    stored_authority_digest: &str,
    thread_position: i64,
) -> Option<&'static str> {
    if !fold.is_resumable() {
        return Some("not_resumable");
    }
    if fold.extensions.get(EXT_EFFECT_LEDGER) != Some(&serde_json::Value::Bool(true)) {
        return Some("ledger_off_in_fold");
    }
    if fold.messages.first().map(|message| message.role.as_str()) != Some("system") {
        return Some("history_invalid");
    }
    if stored_authority_digest != fold.authority.digest() {
        return Some("authority_digest_mismatch");
    }
    if fold.round_in_flight.is_some() {
        return Some("round_open");
    }
    if fold.pending.is_some() {
        return Some("pending_pause");
    }
    if !fold.authority.space_id.is_empty() {
        return Some("space_scope");
    }
    if parks_in(fold) >= MAX_PARKS_PER_RUN {
        return Some("park_limit");
    }
    let budget = match fold.authority.max_rounds {
        0 => DEFAULT_ROUND_BUDGET,
        raw => raw.min(ROUND_BUDGET_CEILING),
    };
    if fold.rounds_executed.saturating_add(fold.delegated_rounds) >= budget {
        return Some("budget_spent");
    }
    if fold.authority.provider_hint.is_empty()
        || route_model_in(fold).is_none()
        || route_connection_in(fold).is_none()
        || child_ceiling_in(fold).is_none_or(|ceiling| ceiling == 0)
    {
        return Some("route_unrecorded");
    }
    match fold
        .extensions
        .get(THREAD_WATERMARK_KEY)
        .and_then(serde_json::Value::as_i64)
    {
        None => Some("no_watermark"),
        Some(watermark) if thread_position > watermark => Some("thread_moved"),
        Some(_) => None,
    }
}

/// The park fold of a crashed round fold: the same fold, pending the run's next park, with the
/// park count it records. Its shape is the one the executor's own park fold has, so the resume's
/// structural rules read it the same way.
fn crash_park_fold(run_id: &str, fold: &LoopFoldV1) -> LoopFoldV1 {
    let count = parks_in(fold).saturating_add(1);
    let mut parked = fold.clone();
    parked.pending = Some(PendingPause {
        kind: PauseKind::Park,
        pause_ref: park_ref(run_id, count),
        step_id: String::new(),
        call: String::new(),
        action_fingerprint: String::new(),
    });
    parked
        .extensions
        .insert(EXT_PARKS.to_owned(), serde_json::Value::from(count));
    parked
}

/// Park the fold a crashed run left active (build plan WS03 S8-B), inside the recovery worker's
/// transaction, which already holds the run's row and its obligation.
///
/// Only an `active` fold of kind `round` is a candidate: a `pre_effect` fold has an effect in
/// flight, a `pause` is the approval path's, and a park is already one. The run must be
/// persistent (a zero-retention or waived run never keeps a fold, and is refused again here), no
/// approval may be open, and the effect ledger must hold no open reservation and no row newer
/// than the fold (an effect the fold does not know of would run twice on a resume). The thread
/// is read under its lock, so an answer append in flight is counted.
///
/// The fold is re-sealed at `seq + 1` as a park with `park_reason` `crash_interrupted`, due after
/// [`CRASH_PARK_GRACE_SECONDS`]. `updated_at` becomes now: the park's age, which its TTL counts
/// from. A loop that is still alive finds its next fold save stale.
///
/// # Errors
///
/// A database failure. A fold that cannot be opened or read is [`CrashPark::Refused`]: the run
/// takes the committed outcome.
pub(crate) async fn park_crashed(
    tx: &mut Transaction<'_, Postgres>,
    cipher: &DescriptorCipher,
    run_id: &str,
) -> Result<CrashPark, Status> {
    let stored: Option<(i64, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT seq, kind, state, ciphertext, authority_digest, org_id, user_id
           FROM run_loop_checkpoints WHERE run_id = $1 FOR UPDATE",
    )
    .bind(run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;
    let Some((seq, kind, state, ciphertext, authority_digest, org_id, user_id)) = stored else {
        return Ok(CrashPark::Refused("no_fold"));
    };
    if kind != "round" || state != "active" {
        return Ok(CrashPark::Refused("not_an_active_round_fold"));
    }
    let retention: Option<String> =
        sqlx::query_scalar("SELECT retention_class FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(internal)?;
    if retention.as_deref() != Some("persistent") {
        return Ok(CrashPark::Refused("not_persistent"));
    }
    let open_facts: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM approvals
                         WHERE run_id = $1 AND consumed_at IS NULL
                           AND status IN ('requested', 'granted'))
             OR EXISTS (SELECT 1 FROM session_tool_audit_intents AS i
                         WHERE i.run_id = $1
                           AND (i.status = 'reserved'
                                OR i.reserved_at >= (SELECT updated_at FROM run_loop_checkpoints
                                                      WHERE run_id = $1)))",
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)?;
    if open_facts {
        return Ok(CrashPark::Refused("open_approval_or_effect"));
    }
    let Some(position) = thread_position_locked(tx, run_id).await? else {
        return Ok(CrashPark::Busy);
    };
    let Ok(plaintext) = cipher.open(&scope_aad(run_id, &org_id, &user_id, seq), &ciphertext)
    else {
        return Ok(CrashPark::Refused("fold_unreadable"));
    };
    let Ok(fold) = check_fold(run_id, plaintext.as_bytes()) else {
        return Ok(CrashPark::Refused("fold_invalid"));
    };
    if let Some(reason) = crash_park_refusal(&fold, &authority_digest, position) {
        return Ok(CrashPark::Refused(reason));
    }
    let Ok(parked) = crash_park_fold(run_id, &fold).to_bytes() else {
        return Ok(CrashPark::Refused("fold_unwritable"));
    };
    let next_seq = seq + 1;
    let Ok(sealed) = cipher.seal(
        &scope_aad(run_id, &org_id, &user_id, next_seq),
        std::str::from_utf8(&parked).map_err(internal)?,
    ) else {
        return Ok(CrashPark::Refused("key_unavailable"));
    };
    let count = parks_in(&fold).saturating_add(1);
    // A new park is a new notice (O-02): the mark of an earlier park is cleared with the park's own write, so this
    // park's notice is written once, under its own park reference.
    let updated = sqlx::query(
        "UPDATE run_loop_checkpoints
            SET seq = $2, kind = 'park', state = 'parked', pause_ref = $3,
                park_reason = $4, park_until = now() + make_interval(secs => $5),
                ciphertext = $6, fold_digest = $7, claim_token_hash = NULL,
                claim_expires_at = NULL, updated_at = now(), park_notice_sent_at = NULL
          WHERE run_id = $1 AND seq = $8 AND kind = 'round' AND state = 'active'",
    )
    .bind(run_id)
    .bind(next_seq)
    .bind(park_ref(run_id, count))
    .bind(ParkReason::CrashInterrupted.as_str())
    .bind(CRASH_PARK_GRACE_SECONDS)
    .bind(&sealed)
    .bind(sha256_hex(&parked))
    .bind(seq)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    if updated.rows_affected() != 1 {
        return Ok(CrashPark::Refused("fold_moved"));
    }
    record_event(
        tx,
        run_id,
        &org_id,
        &user_id,
        "CHECKPOINT_SAVED",
        crate::grpc::CHECKPOINT_SAVED_TYPE_URL,
        next_seq,
        "",
        "park",
    )
    .await?;
    Ok(CrashPark::Parked(next_seq))
}

/// A heartbeat of a run whose fold is a crash park proves its loop is alive: the park is pushed
/// a full grace period on, and is not claimable until that has passed without another heartbeat.
/// Only a crash park that is still parked moves (a claimed one is a continuation's), `park_until`
/// never moves back, and `updated_at`, the park's age, is left alone. Returns the rows moved.
pub(crate) async fn push_crash_park(
    tx: &mut Transaction<'_, Postgres>,
    run_id: &str,
) -> Result<u64, sqlx::Error> {
    let pushed = sqlx::query(
        "UPDATE run_loop_checkpoints
            SET park_until = GREATEST(park_until, now() + make_interval(secs => $2))
          WHERE run_id = $1 AND kind = 'park' AND state = 'parked' AND park_reason = $3",
    )
    .bind(run_id)
    .bind(CRASH_PARK_GRACE_SECONDS)
    .bind(ParkReason::CrashInterrupted.as_str())
    .execute(&mut **tx)
    .await?;
    Ok(pushed.rows_affected())
}

/// The switch of the crash-park notice (O-02). Off by default; only the exact literal `true` turns it on. It needs crash
/// parking and the attention events on as well, and [`crash_park_notice_active`] says so.
pub(crate) const CRASH_PARK_NOTICE_ENV: &str = "SESSION_CORE_CRASH_PARK_NOTICE_ENABLED";
/// The notice time in seconds (O-02), read through [`crash_park_notice_seconds`].
const CRASH_PARK_NOTICE_SECONDS_ENV: &str = "SESSION_CORE_CRASH_PARK_NOTICE_SECONDS";
/// Four hours: how long a crash park waits before the person is told it waits for them.
const CRASH_PARK_NOTICE_DEFAULT_SECONDS: i64 = 14_400;
/// The most notices one pass writes. A backlog after an outage drains over several passes.
const CRASH_PARK_NOTICE_BATCH: i64 = 100;

/// The crash parks a notice is due for (O-02), as the notice, the attention list and the confirm all read them. Placeholders:
/// `$1` the park reason, `$2` the notice time in seconds, `$3` the park TTL in seconds. A park is due once it has waited
/// the notice time and has not run out. Only a persistent run is named: a zero-retention or waived run never keeps a
/// fold, and its ZDR flag is checked too. A park is also held while its thread has an owner row to answer: a requested
/// approval that still counts, a proposed plan, or an unexpired requested question. Those are the rows
/// `attention::derive` lets suppress a park, read the way `attention::fetch_pending` reads them. A held park is neither
/// listed nor announced, and it is not stamped, so the first pass after the row is gone announces it once. The readers
/// splice this one text in, so they name exactly the same parks. Keep it single-spaced: the query tests compare it as text.
pub(crate) const DUE_CRASH_PARK_PREDICATE: &str = "c.kind = 'park' AND c.state = 'parked' AND c.park_reason = $1 \
    AND c.updated_at <= now() - make_interval(secs => $2) \
    AND c.updated_at > now() - make_interval(secs => $3) \
    AND r.retention_class = 'persistent' \
    AND NOT COALESCE((SELECT o.zdr FROM managed_run_terminalization_outbox AS o WHERE o.run_id = c.run_id), false) \
    AND NOT EXISTS (SELECT 1 FROM approvals AS wa JOIN runs AS wr ON wr.id = wa.run_id \
            LEFT JOIN plans AS wp ON wp.id = wa.plan_id \
            WHERE wa.org_id = c.org_id AND wr.thread_id = r.thread_id AND wa.status = 'requested' \
              AND (wa.expires_at IS NULL OR wa.expires_at > now()) \
              AND ((wa.kind = 'plan' AND wp.status = 'proposed') \
                   OR (wa.kind IN ('tool_call', 'permission', 'destructive', 'cost') \
                       AND wr.status NOT IN ('completed', 'failed', 'cancelled')))) \
    AND NOT EXISTS (SELECT 1 FROM plans AS wpl \
            WHERE wpl.org_id = c.org_id AND wpl.thread_id = r.thread_id AND wpl.status = 'proposed') \
    AND NOT EXISTS (SELECT 1 FROM run_questions AS wq \
            WHERE wq.org_id = c.org_id AND wq.thread_id = r.thread_id \
              AND wq.state = 'requested' AND wq.expires_at > now())";

/// Whether the crash-park notice is switched on: only the exact literal `true`, through the parser the durable-loop
/// switches share.
pub(crate) fn crash_park_notice_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(CRASH_PARK_NOTICE_ENV).ok().as_deref())
}

/// Whether the notice goes out at all: crash parking is on, the attention events that carry the notice are on, and the
/// notice switch is the literal `true`. The attention list names a park as NEEDS_INPUT only while this holds, so the
/// list and the notice agree.
pub(crate) fn crash_park_notice_active() -> bool {
    crate::terminalization::crash_park_active()
        && crate::attention_events::enabled()
        && crash_park_notice_enabled()
}

/// The notice time in seconds from the variable's value: a positive whole number of seconds is used as it is, and
/// anything else (unset, not a whole number, zero or negative) keeps four hours. It is capped one second under the park
/// TTL, so the notice falls inside the park's life. At the TTL itself the due window of [`DUE_CRASH_PARK_PREDICATE`]
/// would be empty, and no park would ever be announced. At the cap the window is one second wide.
pub(crate) fn crash_park_notice_seconds_from(value: Option<&str>, ttl_seconds: i64) -> i64 {
    let seconds = value
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(CRASH_PARK_NOTICE_DEFAULT_SECONDS);
    seconds.min(ttl_seconds.saturating_sub(1))
}

/// The notice time in seconds, read at each pass.
pub(crate) fn crash_park_notice_seconds() -> i64 {
    crash_park_notice_seconds_from(
        std::env::var(CRASH_PARK_NOTICE_SECONDS_ENV).ok().as_deref(),
        park_ttl_seconds(),
    )
}

/// The recovery tick's crash-park notice (O-02): writes one `RUN_ATTENTION` row for each due crash park, and stamps the
/// park in the same transaction, so the person is told once. It writes nothing while its switches are off. Returns the
/// number of notices written.
pub(crate) async fn notify_waiting_crash_parks(pool: &PgPool) -> Result<u64, sqlx::Error> {
    if !crash_park_notice_active() {
        return Ok(0);
    }
    notify_due_crash_parks(
        pool,
        crash_park_notice_seconds(),
        park_ttl_seconds(),
        Utc::now(),
    )
    .await
}

/// The crash parks the notice writes for, read in one query (O-02). Placeholders: `$1` to `$3` as
/// [`DUE_CRASH_PARK_PREDICATE`] names them, `$4` the batch and `$5` the origins the attention list covers.
///
/// Two scopes narrow the due predicate to the parks the attention list names, so the list and the notice name the
/// same thing. The thread's origin must be one the list covers: a Space, support or system thread answers to its own
/// surface, and a scheduled run lives in a Space thread. The park's run must be its thread's latest run, the one the
/// list reads, so a park that a newer run has overtaken is never announced. The third condition, not stamped yet, is
/// what keeps the notice to once per park. A park that a pending row holds is not selected at all, so it is not stamped
/// either, and the first pass after that row is gone announces it.
fn due_crash_park_notice_sql() -> String {
    format!(
        "SELECT c.run_id, c.org_id, c.user_id, r.thread_id, COALESCE(c.pause_ref, c.run_id) AS source_id
           FROM run_loop_checkpoints AS c
           JOIN runs AS r ON r.id = c.run_id
           JOIN threads AS t ON t.id = r.thread_id
          WHERE {DUE_CRASH_PARK_PREDICATE} AND c.park_notice_sent_at IS NULL
            AND t.origin = ANY($5)
            AND NOT EXISTS (
                SELECT 1 FROM runs AS newer
                 WHERE newer.thread_id = r.thread_id
                   AND (newer.updated_at, newer.created_at, newer.id) > (r.updated_at, r.created_at, r.id)
            )
          ORDER BY c.updated_at, c.run_id
          LIMIT $4
            FOR UPDATE OF c SKIP LOCKED"
    )
}

/// Writes the notices due at the given notice time and park TTL. The parks are locked with `SKIP LOCKED`, so two replicas
/// never take the same park, and the notice's event id names the run, so a park is written once even if a lock is lost.
/// Returns the number of rows written.
pub(crate) async fn notify_due_crash_parks(
    pool: &PgPool,
    notice_seconds: i64,
    ttl_seconds: i64,
    now: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let origins: Vec<String> = crate::attention_events::LISTED_ORIGINS
        .iter()
        .map(|origin| (*origin).to_owned())
        .collect();
    let sql = due_crash_park_notice_sql();
    let mut tx = pool.begin().await?;
    let due: Vec<(String, String, String, String, String)> = sqlx::query_as(&sql)
        .bind(mp_contracts::loop_fold::ParkReason::CrashInterrupted.as_str())
        .bind(notice_seconds as f64)
        .bind(ttl_seconds as f64)
        .bind(CRASH_PARK_NOTICE_BATCH)
        .bind(origins)
        .fetch_all(&mut *tx)
        .await?;
    let mut written = 0_u64;
    for (run_id, org_id, user_id, thread_id, source_id) in due {
        let notice = crate::attention_events::CrashParkNotice {
            org_id,
            user_id,
            thread_id,
            run_id,
            source_id,
        };
        if crate::attention_events::write_crash_park_notice(&mut tx, &notice, now).await? {
            written += 1;
        }
        sqlx::query(
            "UPDATE run_loop_checkpoints SET park_notice_sent_at = now()
              WHERE run_id = $1 AND park_notice_sent_at IS NULL",
        )
        .bind(&notice.run_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    if written > 0 {
        metrics::counter!("mp_session_crash_park_notices_total").increment(written);
    }
    Ok(written)
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

    /// The golden fold turned into a park fold the way the executor writes one (WS03 S8-4): at a
    /// round boundary (no call in flight), pending a `park` under `pause_ref`, and carrying the
    /// run's park count `parks`. Its reference satisfies the park rules when `pause_ref` is
    /// `park_ref(run_id, parks)`.
    pub(crate) fn park_fold_for(run_id: &str, pause_ref: &str, parks: u32) -> Vec<u8> {
        let mut value: Value = serde_json::from_str(FIXTURE).expect("fixture json");
        value["run_id"] = json!(run_id);
        value["round_in_flight"] = Value::Null;
        value["pending"] = json!({
            "kind": "park",
            "pause_ref": pause_ref,
            "step_id": "",
            "call": "",
            "action_fingerprint": ""
        });
        value["extensions"]["parks"] = json!(parks);
        serde_json::to_vec(&value).expect("fold json")
    }

    /// The golden fold turned into the round fold a crash-parkable run leaves (WS03 S8-B): at a
    /// round boundary, nothing pending, with the system message first, the ledger flag and the
    /// executor's route keys, and a pinned provider. The thread stamp is NOT here: the store
    /// writes it on save while crash parking is on.
    pub(crate) fn crash_parkable_value(run_id: &str) -> Value {
        let mut value: Value = serde_json::from_str(FIXTURE).expect("fixture json");
        value["run_id"] = json!(run_id);
        value["round_in_flight"] = Value::Null;
        value["pending"] = Value::Null;
        value["messages"] = json!([
            { "role": "system", "content": "system prompt" },
            { "role": "user", "content": "do the work" },
            { "role": "assistant", "content": "working on it" }
        ]);
        value["authority"]["provider_hint"] = json!("openai-codex-subscription");
        value["extensions"]["effect_ledger"] = json!(true);
        value["extensions"]["route_model"] = json!("model-x");
        value["extensions"]["route_connection_id"] = json!("conn-1");
        value["extensions"]["child_ceiling"] = json!(2);
        value
    }

    /// The bytes of [`crash_parkable_value`] and the authority digest its fold computes, which
    /// is what the store keeps beside it.
    pub(crate) fn crash_parkable_fold(run_id: &str) -> (Vec<u8>, String) {
        digest_and_bytes(&crash_parkable_value(run_id))
    }

    /// Serialize a fold value and compute its authority digest.
    pub(crate) fn digest_and_bytes(value: &Value) -> (Vec<u8>, String) {
        let fold: LoopFoldV1 = serde_json::from_value(value.clone()).expect("a fold");
        (serde_json::to_vec(value).expect("fold json"), fold.authority.digest())
    }

    /// Store a crash-parkable round fold through the real `save`. The thread stamp is written
    /// only while crash parking is on, so the caller sets the switches first.
    pub(crate) async fn seed_crash_fold(pool: &PgPool, run_id: &str, kind: &str) -> i64 {
        let (fold, digest) = crash_parkable_fold(run_id);
        save(
            pool,
            &cipher(),
            SaveInput {
                run_id,
                expected_seq: 0,
                kind,
                fold: &fold,
                authority_digest: &digest,
                pause_ref: "",
                park_reason: "",
                park_until: None,
            },
        )
        .await
        .expect("the round fold is stored")
        .seq
    }

    /// Write a fold row straight into the table, sealed as the store seals it, for a run the
    /// store would refuse (a zero-retention or waived run): the premise of a test that the
    /// crash park refuses such a row even if one existed.
    pub(crate) async fn plant_round_fold(pool: &PgPool, run_id: &str, fold: &[u8], digest: &str) {
        let (org, user): (String, String) =
            sqlx::query_as("SELECT org_id, user_id FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_one(pool)
                .await
                .expect("the run");
        let sealed = cipher()
            .seal(
                &scope_aad(run_id, &org, &user, 1),
                std::str::from_utf8(fold).expect("utf8"),
            )
            .expect("seal");
        sqlx::query(
            "INSERT INTO run_loop_checkpoints
                (run_id, org_id, user_id, seq, kind, state, authority_digest, segment_no,
                 ciphertext, fold_digest)
             VALUES ($1, $2, $3, 1, 'round', 'active', $4, 1, $5, $6)",
        )
        .bind(run_id)
        .bind(&org)
        .bind(&user)
        .bind(digest)
        .bind(&sealed)
        .bind(sha256_hex(fold))
        .execute(pool)
        .await
        .expect("plant the fold");
    }

    /// Store a fold through the real `save`, the way a loop does. `kind` is
    /// `round`, `pre_effect`, `pause` or `park`; the pause fields follow from it. A `park` is a
    /// park-shaped fold (see [`park_fold_for`]) naming `pause_ref` and a park count of one, so it
    /// stands under the park rules too when `pause_ref` is `park_ref(run_id, 1)`; with park
    /// recovery off it is stored as it always was, whatever the reference.
    pub(crate) async fn seed_fold(
        pool: &PgPool,
        run_id: &str,
        kind: &str,
        pause_ref: &str,
        park_reason: &str,
        park_until: Option<DateTime<Utc>>,
    ) -> Result<i64, Status> {
        let fold = if kind == "park" {
            park_fold_for(run_id, pause_ref, 1)
        } else {
            fold_for(run_id)
        };
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
        .map(|saved| saved.seq)
    }

    /// Place a run inside a Space, with the authority context the table requires.
    pub(crate) async fn put_run_in_space(pool: &PgPool, run_id: &str, space_ref: &str) {
        sqlx::query(
            "UPDATE runs SET space_id = $2, space_decision_ref = 'decision-1',
                    recipient_audience_ref = 'audience-1', privacy_policy_ref = 'policy-1',
                    resource_authorization_ref = 'resource-1', authority_revision = 1,
                    recipient_audience_revision = 1, recipient_audience_hash = 'audience-hash-1'
              WHERE id = $1",
        )
        .bind(run_id)
        .bind(space_ref)
        .execute(pool)
        .await
        .expect("put the run in a Space");
    }

    /// Remove the run's owner from `space_ref`, the way the membership consumer records it.
    pub(crate) async fn remove_owner_from_space(pool: &PgPool, run_id: &str, space_ref: &str) {
        sqlx::query(
            "INSERT INTO space_membership_revocations (space_ref, org_id, subject_id)
             SELECT $2, org_id, user_id FROM runs WHERE id = $1
             ON CONFLICT (space_ref, subject_id) DO UPDATE SET revoked_at = NOW()",
        )
        .bind(run_id)
        .bind(space_ref)
        .execute(pool)
        .await
        .expect("remove the owner from the Space");
    }

    /// The owner joins `space_ref` again: the revocation is cleared.
    pub(crate) async fn rejoin_owner_to_space(pool: &PgPool, run_id: &str, space_ref: &str) {
        sqlx::query(
            "DELETE FROM space_membership_revocations
              WHERE space_ref = $2 AND subject_id = (SELECT user_id FROM runs WHERE id = $1)",
        )
        .bind(run_id)
        .bind(space_ref)
        .execute(pool)
        .await
        .expect("rejoin the owner to the Space");
    }

    /// End a run with a raw write that does not purge its fold, the way a row that
    /// outlived its run (written before the end-of-run guard) came to exist. It is
    /// test-only, so the source scan that pairs every status write with a purge
    /// skips it with the rest of the `#[cfg(test)]` items.
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
        assert!(!enabled(), "a padded or cased true leaves the store off");
        std::env::set_var(ENABLED_ENV, "true");
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
    async fn park_reason_refuses_free_text_and_takes_only_the_closed_tokens() {
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

    // -- Build plan WS03 S8-4: the park rules of a save, the park claim and the park wait ----

    #[test]
    fn the_park_wait_is_a_day_by_default_and_is_clamped() {
        assert_eq!(PARK_TTL_DEFAULT_SECONDS, 86_400);
        for unset in [None, Some(""), Some("soon"), Some("1.5"), Some("99999999999999999999")] {
            assert_eq!(park_ttl_from(unset), 86_400, "{unset:?}");
        }
        for (value, want) in [
            ("-5", 60),
            ("0", 60),
            ("59", 60),
            ("60", 60),
            (" 3600 ", 3600),
            ("2592000", 2_592_000),
            ("2592001", 2_592_000),
        ] {
            assert_eq!(park_ttl_from(Some(value)), want, "{value:?}");
        }
        if std::env::var_os(PARK_TTL_ENV).is_none() {
            assert_eq!(park_ttl_seconds(), 86_400, "the process default");
        }
    }

    #[test]
    fn the_crash_park_notice_waits_four_hours_and_stays_inside_the_park_ttl() {
        // Unset, not a whole number of seconds, or too large to read as one: four hours, as park_ttl_from reads them.
        for unset in [
            None,
            Some(""),
            Some("soon"),
            Some("1.5"),
            Some("99999999999999999999"),
        ] {
            assert_eq!(
                crash_park_notice_seconds_from(unset, 86_400),
                14_400,
                "{unset:?}"
            );
        }
        // A positive whole number is used as it is; zero and negative numbers keep the default.
        for (value, want) in [
            ("1", 1),
            ("599", 599),
            ("600", 600),
            ("0", 14_400),
            ("-5", 14_400),
            (" 3600 ", 3_600),
            ("86399", 86_399),
            ("86400", 86_399),
            ("86401", 86_399),
        ] {
            assert_eq!(
                crash_park_notice_seconds_from(Some(value), 86_400),
                want,
                "{value:?}"
            );
        }
        // A notice time at or past the park TTL is capped one second under it, so the notice still falls inside the
        // park's life. The cap is never the TTL itself, whose due window would be empty.
        assert_eq!(crash_park_notice_seconds_from(Some("9999"), 60), 59);
        assert_eq!(crash_park_notice_seconds_from(None, 60), 59);
        assert_eq!(crash_park_notice_seconds_from(Some("60"), 60), 59);
        assert_eq!(crash_park_notice_seconds_from(Some("59"), 60), 59);
    }

    #[test]
    fn a_new_crash_park_clears_the_notice_mark_of_an_earlier_park() {
        // The park write is the one place a crash park begins, so its mark is cleared there. Without that, a second
        // park of the same run would never be announced, because the first park's mark would still be set.
        let source = include_str!("loop_checkpoint.rs").replace("\r\n", "\n");
        let production = &source[..source
            .find("\n#[cfg(test)]\nmod tests {")
            .expect("tests marker")];
        assert!(
            production.contains(
                "claim_expires_at = NULL, updated_at = now(), park_notice_sent_at = NULL"
            ),
            "the crash park's write clears the mark"
        );
    }

    #[test]
    fn the_notice_is_never_due_for_a_zero_retention_or_waived_run() {
        // A zero-retention or waived run keeps no fold, so none of its parks is ever due. The predicate names a
        // persistent run and refuses one the terminalization outbox marks zero-retention, and it waits for the notice
        // time and not past the park TTL.
        assert!(
            DUE_CRASH_PARK_PREDICATE.contains("r.retention_class = 'persistent'"),
            "{DUE_CRASH_PARK_PREDICATE}"
        );
        assert!(
            DUE_CRASH_PARK_PREDICATE.contains("NOT COALESCE((SELECT o.zdr FROM managed_run_terminalization_outbox AS o WHERE o.run_id = c.run_id), false)"),
            "{DUE_CRASH_PARK_PREDICATE}"
        );
        assert!(
            DUE_CRASH_PARK_PREDICATE.contains("c.updated_at <= now() - make_interval(secs => $2)")
        );
        assert!(
            DUE_CRASH_PARK_PREDICATE.contains("c.updated_at > now() - make_interval(secs => $3)")
        );
    }

    #[test]
    fn the_notice_switch_reads_only_the_exact_literal() {
        // The parser the durable-loop switches share: `true` and nothing else.
        for (value, on) in [
            (Some("true"), true),
            (Some("TRUE"), false),
            (Some(" true"), false),
            (Some("1"), false),
            (None, false),
        ] {
            assert_eq!(mp_contracts::loop_fold::switch_from(value), on, "{value:?}");
        }
        assert_eq!(
            CRASH_PARK_NOTICE_ENV,
            "SESSION_CORE_CRASH_PARK_NOTICE_ENABLED"
        );
    }

    /// Collapses runs of whitespace, so a query is compared by its tokens and not by how its lines are laid out.
    fn squeezed_sql(sql: &str) -> String {
        sql.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn the_notice_names_only_the_listed_origins_and_the_threads_latest_run() {
        // The notice is read through a database, so this pins its text. The origin filter binds the attention list's
        // origins, and a park is announced only while its run is the newest run of its thread, the run the list reads.
        let sql = squeezed_sql(&due_crash_park_notice_sql());
        assert!(
            sql.contains(DUE_CRASH_PARK_PREDICATE),
            "the notice reads the shared predicate"
        );
        assert!(
            sql.contains("JOIN threads AS t ON t.id = r.thread_id"),
            "{sql}"
        );
        assert!(sql.contains("AND t.origin = ANY($5)"), "{sql}");
        assert!(
            sql.contains(
                "NOT EXISTS ( SELECT 1 FROM runs AS newer WHERE newer.thread_id = r.thread_id"
            ),
            "{sql}"
        );
        assert!(
            sql.contains("(newer.updated_at, newer.created_at, newer.id) > (r.updated_at, r.created_at, r.id)"),
            "{sql}"
        );
        assert!(sql.contains("AND c.park_notice_sent_at IS NULL"), "{sql}");
        assert!(
            sql.contains("LIMIT $4 FOR UPDATE OF c SKIP LOCKED"),
            "{sql}"
        );
    }

    /// A park fold of `run_id` as the executor writes one, parsed, after `edit` changed its JSON.
    fn park_fold_edited(
        run_id: &str,
        pause_ref: &str,
        parks: u32,
        edit: impl FnOnce(&mut Value),
    ) -> (Vec<u8>, LoopFoldV1) {
        let mut value: Value =
            serde_json::from_slice(&super::test_support::park_fold_for(run_id, pause_ref, parks))
                .expect("park fold json");
        edit(&mut value);
        let bytes = serde_json::to_vec(&value).expect("fold json");
        let fold = check_fold(run_id, &bytes).expect("a fold of this run");
        (bytes, fold)
    }

    fn park_save_input<'a>(
        run_id: &'a str,
        fold: &'a [u8],
        pause_ref: &'a str,
        reason: &'a str,
    ) -> SaveInput<'a> {
        SaveInput {
            run_id,
            expected_seq: 0,
            kind: "park",
            fold,
            authority_digest: "auth-1",
            pause_ref,
            park_reason: reason,
            park_until: Some(Utc::now()),
        }
    }

    /// The park rules for one fold, the ref the fold was built with named by the save.
    fn park_rules(
        run_id: &str,
        parks: u32,
        pause_ref: &str,
        reason: ParkReason,
        until_after_secs: i64,
        edit: impl FnOnce(&mut Value),
    ) -> Result<(), Status> {
        let (bytes, fold) = park_fold_edited(run_id, pause_ref, parks, edit);
        let now = Utc::now();
        validate_park_save(
            run_id,
            &fold,
            &park_save_input(run_id, &bytes, pause_ref, reason.as_str()),
            reason,
            now + chrono::Duration::seconds(until_after_secs),
            now,
            86_400,
        )
    }

    #[test]
    fn a_park_that_obeys_the_park_rules_is_accepted_and_each_rule_refuses_with_its_own_token() {
        let run = "run-park-rules";
        let ref_n = |n: u32| park_ref(run, n);
        let keep = |_: &mut Value| {};
        let reason = ParkReason::ProviderExhausted;

        // The control: a park at a round boundary, under the run's own reference, within the cap
        // and the wait, passes, for every reason an executor may write.
        for reason in ParkReason::ALL {
            if reason == ParkReason::CrashInterrupted {
                continue;
            }
            assert!(park_rules(run, 1, &ref_n(1), reason, 3600, keep).is_ok(), "{reason:?}");
        }
        // The fifth park is the last one the cap allows.
        assert!(park_rules(run, 5, &ref_n(5), reason, 3600, keep).is_ok());

        // Reserved: no executor writes it.
        assert_eq!(
            refusal_pair(
                &park_rules(run, 1, &ref_n(1), ParkReason::CrashInterrupted, 3600, keep)
                    .expect_err("crash_interrupted is reserved")
            ),
            ("invalid_argument", "park_reason_reserved")
        );

        // Not at a round boundary: a call in flight, no pending pause, a pause of another kind,
        // or a pending reference other than the one the save names.
        let boundary = ("invalid_argument", "park_not_at_round_boundary");
        let in_flight = |value: &mut Value| {
            value["round_in_flight"] = json!({
                "assistant_content": "x", "calls": [], "completed": []
            });
        };
        let no_pending = |value: &mut Value| value["pending"] = Value::Null;
        let approval = |value: &mut Value| value["pending"]["kind"] = json!("approval");
        let other_ref = |value: &mut Value| value["pending"]["pause_ref"] = json!("park:other:1");
        for (what, edit) in [
            ("a call in flight", &in_flight as &dyn Fn(&mut Value)),
            ("no pending pause", &no_pending),
            ("a pending approval", &approval),
            ("another pending reference", &other_ref),
        ] {
            assert_eq!(
                refusal_pair(
                    &park_rules(run, 1, &ref_n(1), reason, 3600, edit)
                        .expect_err(what)
                ),
                boundary,
                "{what}"
            );
        }

        // The cap: a sixth park, or a count too large to read, is refused as a precondition.
        let limit = ("failed_precondition", "park_limit_reached");
        assert_eq!(
            refusal_pair(&park_rules(run, 6, &ref_n(6), reason, 3600, keep).expect_err("six")),
            limit
        );
        assert_eq!(
            refusal_pair(
                &park_rules(run, 1, &ref_n(1), reason, 3600, |value| {
                    value["extensions"]["parks"] = json!(u64::MAX);
                })
                .expect_err("a huge count")
            ),
            limit
        );

        // The reference: no count at all, or a reference that is not the run's own `n`th.
        let mismatch = ("invalid_argument", "park_ref_mismatch");
        assert_eq!(
            refusal_pair(
                &park_rules(run, 1, &ref_n(1), reason, 3600, |value| {
                    value["extensions"].as_object_mut().expect("extensions").remove("parks");
                })
                .expect_err("no count")
            ),
            mismatch
        );
        assert_eq!(
            refusal_pair(&park_rules(run, 2, &ref_n(1), reason, 3600, keep).expect_err("stale ref")),
            mismatch
        );
        assert_eq!(
            refusal_pair(
                &park_rules(run, 1, &park_ref("another-run", 1), reason, 3600, keep)
                    .expect_err("another run's reference")
            ),
            mismatch
        );

        // The wait: up to half the park's own life is fine; one second more is refused.
        assert!(park_rules(run, 1, &ref_n(1), reason, 43_200 - 1, keep).is_ok());
        assert_eq!(
            refusal_pair(
                &park_rules(run, 1, &ref_n(1), reason, 43_200 + 60, keep).expect_err("too long")
            ),
            ("failed_precondition", "park_until_beyond_ttl")
        );

        // The order the rules are asked in: reserved, then the boundary, then the cap, then the
        // reference, then the wait.
        assert_eq!(
            park_rules(run, 9, &ref_n(1), ParkReason::CrashInterrupted, 999_999, in_flight)
                .expect_err("first")
                .message(),
            "park_reason_reserved"
        );
        assert_eq!(
            park_rules(run, 9, &ref_n(1), reason, 999_999, in_flight)
                .expect_err("second")
                .message(),
            "park_not_at_round_boundary"
        );
        assert_eq!(
            park_rules(run, 9, &ref_n(9), reason, 999_999, keep)
                .expect_err("third")
                .message(),
            "park_limit_reached"
        );
        assert_eq!(
            park_rules(run, 2, &ref_n(1), reason, 999_999, keep)
                .expect_err("fourth")
                .message(),
            "park_ref_mismatch"
        );
    }

    /// The park rules are asked by `save` itself, before it touches the store, and only while
    /// park recovery is active: a lazy pool that cannot connect proves each refusal came first.
    #[tokio::test]
    async fn save_asks_the_park_rules_before_the_store_and_only_under_park_recovery() {
        use crate::terminalization::park_test_switch;
        let run = fixture_run_id();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://unreachable.invalid/session")
            .expect("lazy pool");
        let key = DescriptorCipher::from_key_bytes_for_test(&[7_u8; 32]).expect("test key");
        let attempt = |pause_ref: String, parks: u32, reason: &'static str, edit: fn(&mut Value)| {
            let (bytes, _fold) = park_fold_edited(&run, &pause_ref, parks, edit);
            (bytes, pause_ref, reason)
        };
        let cases = [
            (
                "a reserved reason",
                attempt(park_ref(&run, 1), 1, "crash_interrupted", |_| {}),
                "park_reason_reserved",
            ),
            (
                "a call in flight",
                attempt(park_ref(&run, 1), 1, "provider_exhausted", |value| {
                    value["round_in_flight"] =
                        json!({"assistant_content": "x", "calls": [], "completed": []});
                }),
                "park_not_at_round_boundary",
            ),
            (
                "a sixth park",
                attempt(park_ref(&run, 6), 6, "provider_exhausted", |_| {}),
                "park_limit_reached",
            ),
            (
                "another run's reference",
                attempt(park_ref("another-run", 1), 1, "provider_exhausted", |_| {}),
                "park_ref_mismatch",
            ),
        ];
        let _store = test_switch::Forced::set(true);
        let _park = park_test_switch::Forced::set(true);
        for (what, (bytes, pause_ref, reason), token) in cases {
            let refused = save(
                &pool,
                &key,
                SaveInput {
                    park_until: Some(Utc::now() + chrono::Duration::seconds(3600)),
                    ..park_save_input(&run, &bytes, &pause_ref, reason)
                },
            )
            .await
            .expect_err(what);
            assert_eq!(refused.message(), token, "{what}");
        }
        // A wait past half the park's life, through `save` and the real clock.
        let (bytes, _fold) = park_fold_edited(&run, &park_ref(&run, 1), 1, |_| {});
        let refused = save(
            &pool,
            &key,
            SaveInput {
                park_until: Some(Utc::now() + chrono::Duration::seconds(park_ttl_seconds())),
                ..park_save_input(&run, &bytes, &park_ref(&run, 1), "provider_exhausted")
            },
        )
        .await
        .expect_err("a wait as long as the park's whole life");
        assert_eq!(refused.message(), "park_until_beyond_ttl");

        // Off: with either switch off the rules are not asked (`park_recovery_active` is false),
        // so the store is reached. That is proven by the database tests, which store the park as
        // committed with `park_accepted` false; here only the gate is read.
        drop(_park);
        let _park_off = park_test_switch::Forced::set(false);
        assert!(!crate::terminalization::park_recovery_active());
        drop(_park_off);
        let _park_on = park_test_switch::Forced::set(true);
        let _store_off = test_switch::Forced::set(false);
        assert!(!crate::terminalization::park_recovery_active());
    }

    fn crash_fold(edit: impl FnOnce(&mut Value)) -> (LoopFoldV1, String) {
        let mut value = test_support::crash_parkable_value("run-crash");
        value["extensions"]["thread_watermark"] = serde_json::json!(7);
        edit(&mut value);
        let (bytes, digest) = test_support::digest_and_bytes(&value);
        (check_fold("run-crash", &bytes).expect("a fold"), digest)
    }

    #[test]
    fn a_crash_parkable_round_fold_is_accepted_and_each_rule_refuses_with_its_own_token() {
        let (fold, digest) = crash_fold(|_| {});
        assert_eq!(crash_park_refusal(&fold, &digest, 7), None, "the thread is where the fold left it");
        assert_eq!(crash_park_refusal(&fold, &digest, 3), None, "a thread below the stamp is no move");

        type Edit = fn(&mut Value);
        let cases: &[(&str, &str, Edit)] = &[
            ("not_resumable", "a tombstone", |v| v["extensions"]["not_resumable"] = "oversize".into()),
            ("ledger_off_in_fold", "no ledger flag", |v| { v["extensions"].as_object_mut().unwrap().remove("effect_ledger"); }),
            ("ledger_off_in_fold", "a ledger flag that is not true", |v| v["extensions"]["effect_ledger"] = "true".into()),
            ("history_invalid", "no system message first", |v| v["messages"][0]["role"] = "user".into()),
            ("round_open", "a round in flight", |v| v["round_in_flight"] = serde_json::json!({"assistant_content": "", "calls": [], "completed": []})),
            ("pending_pause", "a pending pause", |v| v["pending"] = serde_json::json!({"kind": "approval", "pause_ref": "a"})),
            ("space_scope", "a Space run", |v| v["authority"]["space_id"] = "space-1".into()),
            ("park_limit", "the park cap spent", |v| v["extensions"]["parks"] = 5.into()),
            ("budget_spent", "the round budget spent", |v| v["rounds_executed"] = 8.into()),
            ("budget_spent", "delegated rounds count", |v| v["delegated_rounds"] = 6.into()),
            ("route_unrecorded", "no pinned provider", |v| v["authority"]["provider_hint"] = "".into()),
            ("route_unrecorded", "no route model", |v| { v["extensions"].as_object_mut().unwrap().remove("route_model"); }),
            ("route_unrecorded", "no route connection", |v| v["extensions"]["route_connection_id"] = "".into()),
            ("route_unrecorded", "no child ceiling", |v| { v["extensions"].as_object_mut().unwrap().remove("child_ceiling"); }),
            ("route_unrecorded", "a zero child ceiling", |v| v["extensions"]["child_ceiling"] = 0.into()),
            ("no_watermark", "no thread stamp", |v| { v["extensions"].as_object_mut().unwrap().remove("thread_watermark"); }),
            ("no_watermark", "a stamp that is not a number", |v| v["extensions"]["thread_watermark"] = "7".into()),
        ];
        for (token, what, edit) in cases {
            let (fold, digest) = crash_fold(edit);
            assert_eq!(crash_park_refusal(&fold, &digest, 7), Some(*token), "{what}");
        }

        let (fold, digest) = crash_fold(|_| {});
        assert_eq!(crash_park_refusal(&fold, &digest, 8), Some("thread_moved"), "a message above the stamp");
        assert_eq!(
            crash_park_refusal(&fold, "another-digest", 7),
            Some("authority_digest_mismatch"),
            "the stored digest must be the fold's own"
        );
    }

    #[test]
    fn the_round_budget_mirrors_the_executors_and_a_default_budget_is_charged() {
        let (spent_by_default, digest) = crash_fold(|v| {
            v["authority"]["max_rounds"] = 0.into();
            v["rounds_executed"] = 12.into();
        });
        assert_eq!(crash_park_refusal(&spent_by_default, &digest, 7), Some("budget_spent"));
        let (left_by_default, digest) = crash_fold(|v| {
            v["authority"]["max_rounds"] = 0.into();
            v["rounds_executed"] = 11.into();
        });
        assert_eq!(crash_park_refusal(&left_by_default, &digest, 7), None);
        let (clamped, digest) = crash_fold(|v| {
            v["authority"]["max_rounds"] = 500.into();
            v["rounds_executed"] = 32.into();
        });
        assert_eq!(crash_park_refusal(&clamped, &digest, 7), Some("budget_spent"), "the ceiling applies");
    }

    #[test]
    fn a_crash_park_fold_has_the_shape_of_the_executors_park_fold() {
        let (fold, _) = crash_fold(|v| v["extensions"]["parks"] = 2.into());
        let parked = crash_park_fold("run-crash", &fold);
        let pending = parked.pending.as_ref().expect("a pending park");
        assert_eq!(pending.kind, PauseKind::Park);
        assert_eq!(pending.pause_ref, park_ref("run-crash", 3));
        assert!(pending.step_id.is_empty() && pending.call.is_empty() && pending.action_fingerprint.is_empty());
        assert_eq!(parks_in(&parked), 3, "the count includes this park");
        assert!(parked.round_in_flight.is_none());
        assert_eq!(parked.messages, fold.messages, "the history is untouched");
        assert_eq!(parked.authority.digest(), fold.authority.digest());
        assert_eq!(route_model_in(&parked), route_model_in(&fold));
        assert_eq!(
            parked.extensions.get(THREAD_WATERMARK_KEY),
            fold.extensions.get(THREAD_WATERMARK_KEY),
            "the stamp rides along, so the claim can check the thread again"
        );
        let bytes = parked.to_bytes().expect("it serializes");
        assert!(check_fold("run-crash", &bytes).is_ok());
    }

    #[test]
    fn the_thread_stamp_is_added_to_the_extensions_and_nowhere_else() {
        let (bytes, _) = test_support::crash_parkable_fold("run-crash");
        let plain = std::str::from_utf8(&bytes).expect("utf8");
        let stamped = stamp_thread_watermark(plain, 42).expect("stamped");
        let (before, after): (Value, Value) = (
            serde_json::from_str(plain).expect("json"),
            serde_json::from_str(&stamped).expect("json"),
        );
        assert_eq!(after["extensions"][THREAD_WATERMARK_KEY], 42);
        let mut without = after.clone();
        without["extensions"].as_object_mut().expect("object").remove(THREAD_WATERMARK_KEY);
        assert_eq!(without, before, "nothing else changed");
        assert_eq!(stamp_thread_watermark("[]", 1), None, "not an object");
        assert_eq!(stamp_thread_watermark("{\"v\":1}", 1), None, "no extensions to stamp");
        assert_eq!(stamp_thread_watermark("not json", 1), None);
        let mut huge = before;
        huge["messages"][0]["content"] = "x".repeat(MAX_FOLD_BYTES - 40).into();
        assert_eq!(
            stamp_thread_watermark(&serde_json::to_string(&huge).expect("json"), 1_000_000_000),
            None,
            "a stamp that would pass the size cap is not written"
        );
    }

    #[test]
    fn crash_parking_is_off_unless_both_switches_are_true() {
        use crate::terminalization::{crash_park_active, crash_test_switch, park_test_switch};
        let _store = test_switch::Forced::set(true);
        for (park, crash, expected) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, true),
        ] {
            let _park = park_test_switch::Forced::set(park);
            let _crash = crash_test_switch::Forced::set(crash);
            assert_eq!(crash_park_active(), expected, "park recovery {park}, crash park {crash}");
        }
        let _park = park_test_switch::Forced::set(true);
        let _crash = crash_test_switch::Forced::set(true);
        let _store_off = test_switch::Forced::set(false);
        assert!(!crash_park_active(), "the fold store is a precondition too");
    }

    #[test]
    fn the_crash_park_grace_is_the_workers_deadline_period() {
        assert!((CRASH_PARK_GRACE_SECONDS - 900.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_park_claim_statement_never_touches_the_parks_age() {
        let sql = CLAIM_PARK_SQL.split_whitespace().collect::<Vec<_>>().join(" ");
        // The park's age is the anchor of the worker's wait: a claim is not a new park.
        assert!(!sql.contains("updated_at"), "{sql}");
        // It takes a park row only, in a claimable state, and only once the park is due.
        for needed in [
            "WHERE run_id = $1 AND kind = 'park'",
            "(state = 'parked' OR (state = 'resuming' AND claim_expires_at < now()))",
            "AND park_until <= now()",
            "SET state = 'resuming'",
            "RETURNING seq, ciphertext, segment_no, authority_digest, kind",
        ] {
            assert!(sql.contains(needed), "{needed}: {sql}");
        }
        // It names no reference: an empty reference is how a caller asks for it.
        assert!(!sql.contains("pause_ref"), "{sql}");

        // The committed claim statement is still in the source, untouched, and the committed
        // save's upsert still sets the age on every write.
        let source = include_str!("loop_checkpoint.rs").replace("\r\n", "\n");
        let production = &source[..source
            .find("\n#[cfg(test)]\npub(crate) mod test_support")
            .expect("test support")];
        let flat: String = production.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains(
                "SET state = 'resuming', claim_token_hash = $2, claim_expires_at = now() + make_interval(secs => $3), updated_at = now() WHERE run_id = $1 AND pause_ref = $4 AND (state IN ('paused', 'parked') OR (state = 'resuming' AND claim_expires_at < now())) RETURNING seq, ciphertext, segment_no, authority_digest, kind"
            ),
            "the committed claim statement changed"
        );
        assert!(
            flat.contains("claim_token_hash = NULL, claim_expires_at = NULL, updated_at = now()"),
            "the committed save's upsert changed"
        );
        // The empty-reference claim is taken only under the park rules.
        assert!(
            flat.contains("let park_claim = pause_ref.is_empty() && crate::terminalization::park_recovery_active();"),
            "the park claim is not gated on park recovery"
        );
    }

    /// Under park recovery a park is claimed by the empty reference only: its own reference is
    /// computable from the run id, and the committed statement would take it before `park_until`
    /// and move `updated_at`, the anchor of its TTL. The refusal sits behind the park-recovery
    /// gate (so with the rules off the committed claim is untouched), before the committed
    /// statement, and reads the kind of the run's current fold.
    #[test]
    fn a_named_reference_never_claims_a_park_under_park_recovery() {
        let source = include_str!("loop_checkpoint.rs").replace("\r\n", "\n");
        let production = &source[..source
            .find("\n#[cfg(test)]\npub(crate) mod test_support")
            .expect("test support")];
        let flat: String = production.split_whitespace().collect::<Vec<_>>().join(" ");
        let gate = "if !park_claim && !pause_ref.is_empty() && crate::terminalization::park_recovery_active() {";
        let read = "SELECT kind FROM run_loop_checkpoints WHERE run_id = $1";
        let refusal = "if current.is_some_and(|(kind,)| kind == \"park\") { return Err(Status::failed_precondition(\"loop_checkpoint_not_claimable\")); }";
        let committed = "SET state = 'resuming', claim_token_hash = $2, claim_expires_at = now() + make_interval(secs => $3), updated_at = now() WHERE run_id = $1 AND pause_ref = $4";
        for needle in [gate, read, refusal, committed] {
            assert_eq!(flat.matches(needle).count(), 1, "{needle}");
        }
        let (gate_at, read_at, refusal_at, committed_at) = (
            flat.find(gate).expect("gate"),
            flat.find(read).expect("read"),
            flat.find(refusal).expect("refusal"),
            flat.find(committed).expect("committed"),
        );
        // The read and the refusal are inside the gate, and the gate is before the statement.
        assert!(gate_at < read_at && read_at < refusal_at, "the refusal is not behind its gate");
        assert!(refusal_at < committed_at, "the refusal is not before the committed claim");
        // Inside the gate means before its closing brace: the refusal ends the gate's body.
        let after_refusal = &flat[refusal_at + refusal.len()..];
        assert!(after_refusal.trim_start().starts_with('}'), "the gate holds more than the refusal");
        // The gate's body is the one read and the refusal, nothing that writes.
        let body = &flat[gate_at + gate.len()..refusal_at + refusal.len()];
        for write in ["UPDATE", "INSERT", "DELETE"] {
            assert!(!body.contains(write), "{write} inside the park-reference refusal");
        }
    }

    /// The save RPC echoes the store's own answer for `park_accepted`, never a literal: with a
    /// literal an old or switched-off store would tell the executor a park was accepted.
    #[test]
    fn the_save_rpc_echoes_the_stores_park_answer_and_never_a_literal() {
        let source = include_str!("grpc.rs").replace("\r\n", "\n");
        let start = source
            .find("pub(crate) async fn save_loop_checkpoint_inner(")
            .expect("the save handler");
        let body = &source[start..start + source[start..].find("\n}\n").expect("its end")];
        assert!(body.contains("park_accepted: saved.park_accepted,"), "{body}");
        for literal in ["park_accepted: true", "park_accepted: false"] {
            assert!(!body.contains(literal), "{literal}");
        }
    }

    #[test]
    fn a_park_seed_fold_is_park_shaped_and_obeys_the_park_rules_for_its_own_reference() {
        let run = "run-park-seed";
        let bytes = super::test_support::park_fold_for(run, &park_ref(run, 1), 1);
        let fold = check_fold(run, &bytes).expect("a fold of this run");
        assert!(fold.round_in_flight.is_none());
        let pending = fold.pending.as_ref().expect("a pending pause");
        assert_eq!(pending.kind, PauseKind::Park);
        assert_eq!(pending.pause_ref, park_ref(run, 1));
        assert_eq!(parks_in(&fold), 1);
        assert!(park_rules(run, 1, &park_ref(run, 1), ParkReason::ReauthRequired, 60, |_| {}).is_ok());
        // The committed park seeds name `appr_1`: judged by the park rules they are refused, so
        // they are stored only while park recovery is off, as they always were.
        assert_eq!(
            park_rules(run, 1, "appr_1", ParkReason::ReauthRequired, 60, |_| {})
                .expect_err("a seed reference is not the run's park reference")
                .message(),
            "park_ref_mismatch"
        );
    }

    /// The source scan the tripwire below is built on. It is a text heuristic over
    /// the production code of `src/`, not a parser: see the tripwire's comment.
    mod status_write_scan {
        pub(super) struct Scan {
            pub(super) status_writes: usize,
            pub(super) purge_calls: usize,
        }

        pub(super) fn scan(source: &str) -> Scan {
            let code = production_code(source);
            Scan {
                status_writes: status_writes(&code),
                purge_calls: purge_calls(&code),
            }
        }

        /// A comment, or a string or char literal, and the index just past it.
        enum Lexeme {
            Comment(usize),
            Literal(usize),
        }

        fn is_ident(c: char) -> bool {
            c.is_alphanumeric() || c == '_'
        }

        /// The comment or literal that starts at `i`, if one does. Strings and char
        /// literals matter because a `{`, `}` or `//` inside one must not be read as
        /// code; comments because a statement quoted in one is not a write.
        fn lexeme_at(chars: &[char], i: usize) -> Option<Lexeme> {
            let next = chars.get(i + 1).copied();
            match chars[i] {
                '/' if next == Some('/') => {
                    let end = (i..chars.len()).find(|&j| chars[j] == '\n');
                    Some(Lexeme::Comment(end.unwrap_or(chars.len())))
                }
                '/' if next == Some('*') => {
                    let (mut j, mut depth) = (i + 2, 1_usize);
                    while j < chars.len() && depth > 0 {
                        match (chars[j], chars.get(j + 1).copied()) {
                            ('/', Some('*')) => {
                                depth += 1;
                                j += 2;
                            }
                            ('*', Some('/')) => {
                                depth -= 1;
                                j += 2;
                            }
                            _ => j += 1,
                        }
                    }
                    Some(Lexeme::Comment(j))
                }
                'r' | 'b' if i == 0 || !is_ident(chars[i - 1]) => prefixed_string(chars, i),
                '"' => Some(Lexeme::Literal(string_end(chars, i))),
                '\'' => char_literal_end(chars, i).map(Lexeme::Literal),
                _ => None,
            }
        }

        /// The end of the plain string whose opening quote is at `open`.
        fn string_end(chars: &[char], open: usize) -> usize {
            let mut j = open + 1;
            while j < chars.len() {
                match chars[j] {
                    '\\' => j += 2,
                    '"' => return j + 1,
                    _ => j += 1,
                }
            }
            chars.len()
        }

        /// A raw string (`r"..."`, `r#"..."#`, `br"..."`) or a byte string (`b"..."`).
        fn prefixed_string(chars: &[char], i: usize) -> Option<Lexeme> {
            let mut j = i + usize::from(chars[i] == 'b');
            if chars.get(j) != Some(&'r') {
                let byte_string = chars[i] == 'b' && chars.get(i + 1) == Some(&'"');
                return byte_string.then(|| Lexeme::Literal(string_end(chars, i + 1)));
            }
            j += 1;
            let hashes = chars[j..].iter().take_while(|&&c| c == '#').count();
            j += hashes;
            if chars.get(j) != Some(&'"') {
                return None;
            }
            let closes = |k: usize| {
                chars[k] == '"' && (1..=hashes).all(|n| chars.get(k + n) == Some(&'#'))
            };
            let end = (j + 1..chars.len()).find(|&k| closes(k)).map(|k| k + 1 + hashes);
            Some(Lexeme::Literal(end.unwrap_or(chars.len())))
        }

        /// The end of the char literal that starts at `i`, or `None` for a lifetime.
        fn char_literal_end(chars: &[char], i: usize) -> Option<usize> {
            match (chars.get(i + 1), chars.get(i + 2)) {
                (Some('\\'), _) => {
                    let close = (i + 3..chars.len()).find(|&j| chars[j] == '\'');
                    Some(close.map_or(chars.len(), |j| j + 1))
                }
                (Some(c), Some('\'')) if *c != '\'' => Some(i + 3),
                _ => None,
            }
        }

        /// The index just past the `{ ... }` block that opens at `open`.
        fn skip_braces(chars: &[char], open: usize) -> usize {
            let (mut i, mut depth) = (open, 0_usize);
            while i < chars.len() {
                if let Some(lexeme) = lexeme_at(chars, i) {
                    i = match lexeme {
                        Lexeme::Comment(end) | Lexeme::Literal(end) => end,
                    };
                    continue;
                }
                match chars[i] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return i + 1;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            chars.len()
        }

        /// The index just past the item that follows a `#[cfg(test)]` attribute at
        /// `i`: up to its closing brace, or to its `;` when it has no body. Brackets
        /// and parentheses are tracked so the `;` in `[u8; 4]` is not an item end.
        fn skip_item(chars: &[char], mut i: usize) -> usize {
            let mut nesting = 0_i32;
            while i < chars.len() {
                if let Some(lexeme) = lexeme_at(chars, i) {
                    i = match lexeme {
                        Lexeme::Comment(end) | Lexeme::Literal(end) => end,
                    };
                    continue;
                }
                match chars[i] {
                    '(' | '[' => nesting += 1,
                    ')' | ']' => nesting -= 1,
                    ';' if nesting <= 0 => return i + 1,
                    '{' if nesting <= 0 => return skip_braces(chars, i),
                    _ => {}
                }
                i += 1;
            }
            chars.len()
        }

        /// The source with its comments blanked and every `#[cfg(test)]` item removed
        /// (a file that opens with `#![cfg(test)]` has no production code at all).
        /// Strings are kept: the SQL is in them.
        fn production_code(source: &str) -> String {
            const TEST_ONLY: &str = "#[cfg(test)]";
            if source.lines().any(|line| line.trim() == "#![cfg(test)]") {
                return String::new();
            }
            let chars: Vec<char> = source.chars().collect();
            let marker: Vec<char> = TEST_ONLY.chars().collect();
            let mut out = String::with_capacity(source.len());
            let mut i = 0;
            while i < chars.len() {
                match lexeme_at(&chars, i) {
                    Some(Lexeme::Comment(end)) => {
                        out.push(' ');
                        i = end;
                    }
                    Some(Lexeme::Literal(end)) => {
                        out.extend(&chars[i..end]);
                        i = end;
                    }
                    None if chars[i..].starts_with(&marker) => {
                        out.push(' ');
                        i = skip_item(&chars, i + marker.len());
                    }
                    None => {
                        out.push(chars[i]);
                        i += 1;
                    }
                }
            }
            out
        }

        /// The code with whitespace collapsed and lower-cased, so a statement split
        /// over lines, or written in lower case, reads the same.
        fn flatten(code: &str) -> String {
            code.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase()
        }

        /// How many `UPDATE runs` statements assign the `status` column.
        fn status_writes(code: &str) -> usize {
            let flat = flatten(code);
            let (mut rest, mut count) = (flat.as_str(), 0);
            while let Some(at) = rest.find("update ") {
                let standalone = !rest[..at].chars().next_back().is_some_and(is_ident);
                rest = &rest[at + "update ".len()..];
                if standalone && assigns_status_of_runs(rest) {
                    count += 1;
                }
            }
            count
        }

        /// Whether the statement that begins at `statement` (just past `UPDATE `)
        /// updates `runs` and names `status` among the columns it sets.
        fn assigns_status_of_runs(statement: &str) -> bool {
            let end = statement.find([';', '"']).unwrap_or(statement.len());
            let statement = &statement[..end];
            let statement = statement.strip_prefix("only ").unwrap_or(statement);
            let table: String = statement
                .chars()
                .take_while(|&c| is_ident(c) || c == '.')
                .collect();
            if table.rsplit('.').next() != Some("runs") {
                return false;
            }
            let Some(set_at) = statement.find(" set ") else {
                return false;
            };
            let list = &statement[set_at + " set ".len()..];
            let list_end = [" where ", " returning ", " from "]
                .iter()
                .filter_map(|keyword| list.find(keyword))
                .min()
                .unwrap_or(list.len());
            set_targets(&list[..list_end]).iter().any(|column| column == "status")
        }

        /// The column names a SET list assigns: `a = 1, b = f(x, y)` gives `a`, `b`,
        /// and `(a, b) = (1, 2)` gives both. Commas inside parentheses or quotes do
        /// not split.
        fn set_targets(list: &str) -> Vec<String> {
            let (mut pieces, mut start) = (Vec::new(), 0);
            let (mut depth, mut quoted) = (0_i32, false);
            for (at, c) in list.char_indices() {
                match c {
                    '\'' => quoted = !quoted,
                    '(' if !quoted => depth += 1,
                    ')' if !quoted => depth -= 1,
                    ',' if !quoted && depth == 0 => {
                        pieces.push(&list[start..at]);
                        start = at + 1;
                    }
                    _ => {}
                }
            }
            pieces.push(&list[start..]);

            let mut targets = Vec::new();
            for piece in pieces {
                let piece = piece.trim();
                if let Some(group) = piece.strip_prefix('(') {
                    let names = group.split(')').next().unwrap_or_default();
                    targets.extend(names.split(',').map(|name| name.trim().to_owned()));
                    continue;
                }
                let name: String = piece.chars().take_while(|&c| is_ident(c)).collect();
                if piece[name.len()..].trim_start().starts_with('=') {
                    targets.push(name);
                }
            }
            targets
        }

        /// How many calls to `purge_run` the code makes; its definition is not one.
        fn purge_calls(code: &str) -> usize {
            let flat = flatten(code);
            flat.match_indices("purge_run(")
                .filter(|(at, _)| !flat[..*at].ends_with("fn "))
                .count()
        }
    }

    fn writes(source: &str) -> usize {
        status_write_scan::scan(source).status_writes
    }

    #[test]
    fn a_status_assignment_is_found_wherever_it_sits_in_the_statement() {
        let found = [
            r#"q("UPDATE runs SET status = 'x' WHERE id = $1");"#,
            r#"q("UPDATE runs SET ended_at = now(), status = $2 WHERE id = $1");"#,
            r#"q("UPDATE runs SET updated_at = now(), status = 'awaiting_approval' WHERE id = $1");"#,
            "q(\"UPDATE runs\n   SET mode = $2,\n       status = $3\n WHERE id = $1\");",
            r#"q("update runs set status = 'x'");"#,
            r#"q("UPDATE runs AS r SET status = 'x' WHERE r.id = $1");"#,
            r#"q("UPDATE ONLY runs SET status = 'x'");"#,
            r#"q("UPDATE public.runs SET status = 'x'");"#,
            r#"q("UPDATE runs SET (status, ended_at) = ('x', now())");"#,
            r#"q("UPDATE runs SET status='x'");"#,
        ];
        for source in found {
            assert_eq!(writes(source), 1, "{source}");
        }
        assert_eq!(writes(&found[..2].join("\n")), 2, "two statements count twice");
    }

    #[test]
    fn an_update_that_does_not_assign_the_status_column_is_not_a_status_write() {
        let not_found = [
            r#"q("UPDATE runs SET metadata = COALESCE(metadata, '{}'::jsonb) || $4::jsonb, updated_at = now() WHERE id = $1");"#,
            r#"q("UPDATE runs SET mode = $2, metadata = metadata || $4::jsonb WHERE id = $1 RETURNING id, mode");"#,
            r#"q("UPDATE runs SET final_output = $2, updated_at = now() WHERE id = $1");"#,
            r#"q("UPDATE runs SET retention_class = 'persistent', waiver_ack_id = NULL WHERE id = $1");"#,
            r#"q("UPDATE runs SET goal = CASE WHEN status = 'failed' THEN 'a' ELSE goal END WHERE id = $1");"#,
            r#"q("UPDATE runs SET ended_status = 'x' WHERE id = $1");"#,
            r#"q("UPDATE runs SET updated_at = now() WHERE id = $1 AND status = 'running'");"#,
            r#"q("UPDATE runs_archive SET status = 'x'");"#,
            r#"q("UPDATE plans SET status = $1 WHERE id = $3 AND status IN ('draft')");"#,
            r#"q("SELECT status FROM runs WHERE id = $1");"#,
        ];
        for source in not_found {
            assert_eq!(writes(source), 0, "{source}");
        }
    }

    #[test]
    fn comments_and_test_only_items_are_not_production_code() {
        let write = r#"q("UPDATE runs SET status = 'x'");"#;
        assert_eq!(writes(&format!("// {write}")), 0);
        assert_eq!(writes(&format!("/// {write}")), 0);
        assert_eq!(writes(&format!("/* {write} */")), 0);
        assert_eq!(writes(&format!("/* outer /* nested */ {write} */ fn f() {{}}")), 0);

        // The test module holds braces in a string, a raw string and two char
        // literals, and a lifetime precedes it; the production write after it counts.
        let template = r##"
fn before<'a>(x: &'a str) -> &'a str { x }
#[cfg(test)]
mod tests {
    fn f() { let _ = "}"; let _ = '}'; let _ = '{'; let _ = r#"}"#; WRITE }
}
fn after() { WRITE }
"##;
        assert_eq!(writes(&template.replace("WRITE", write)), 1);

        // A test-only fn with an array type in its signature, a use, and a module
        // declared in another file: each is skipped, the write after them counts.
        let template = r##"
#[cfg(test)]
fn helper(x: [u8; 4]) -> u8 { WRITE x[0] }
#[cfg(test)]
use std::fmt;
#[cfg(test)]
#[allow(dead_code)]
mod seeded;
fn after() { WRITE }
"##;
        assert_eq!(writes(&template.replace("WRITE", write)), 1);

        // `cfg(not(test))` is production; a raw string is code too.
        assert_eq!(writes(&format!("#[cfg(not(test))]\nfn f() {{ {write} }}")), 1);
        assert_eq!(writes("let s = r#\"UPDATE runs SET status = 'x'\"#;"), 1);
        // A file that is test-only as a whole has no production code.
        assert_eq!(writes(&format!("#![cfg(test)]\nfn f() {{ {write} }}")), 0);
    }

    #[test]
    fn a_purge_call_is_a_call_in_production_code_not_the_definition() {
        let purges = |source: &str| status_write_scan::scan(source).purge_calls;
        assert_eq!(purges("fn a() { crate::loop_checkpoint::purge_run(tx, id).await; }"), 1);
        assert_eq!(purges("pub(crate) async fn purge_run(tx: &mut T) {}"), 0);
        assert_eq!(purges("// purge_run(tx)\nfn a() {}"), 0);
        assert_eq!(purges("#[cfg(test)] mod t { fn a() { purge_run(tx); } }"), 0);
    }

    /// Source files that carry a status write which is not a product terminal write
    /// and are not already skipped as `#[cfg(test)]` (for instance a module declared
    /// `#[cfg(test)] mod x;` whose whole file seeds test data). Path under `src/`,
    /// with the reason. Empty today: every test-seeding write sits in a
    /// `#[cfg(test)]` module of its file, which the scan leaves out. An entry that
    /// no longer matches a write fails the tripwire, so the list cannot go stale.
    const TEST_SEEDING_WRITES: &[(&str, &str)] = &[(
        "run_phase_tests.rs",
        "WS09-S1 DB tests seed a terminal status to prove the thread run guard releases",
    ), (
        "grpc_space_mentions_tests.rs",
        "WS08-S2 DB tests seed a run status to prove the mention wake fold; test-only",
    )];

    fn rust_sources(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read a source directory") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                rust_sources(&path, found);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                found.push(path);
            }
        }
    }

    /// A run that ends leaves no fold behind, and that should hold only if every
    /// place that writes a run's status also calls `purge_run`. This is a HEURISTIC
    /// TRIPWIRE, not a proof. It reads the production code of every file under
    /// `src/` as text (comments and `#[cfg(test)]` items left out), finds each
    /// `UPDATE runs` statement whose SET list assigns `status` in any order, and
    /// requires the same file to call `purge_run` at least as often. It cannot see
    /// that the purge is in the same transaction, in the same branch, or reached at
    /// all, and it cannot see a status written through some other statement shape.
    /// The proof is the per-path database tests in this module, which end a run
    /// through each production path and read the fold table back.
    #[test]
    fn every_run_status_write_is_paired_with_a_purge() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_sources(&src, &mut files);
        files.sort();
        let mut with_writes = 0_usize;
        let (mut unpaired, mut allowlist_used) = (Vec::new(), Vec::new());
        for path in &files {
            let relative = path
                .strip_prefix(&src)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            let source = std::fs::read_to_string(path).expect("read source");
            let found = status_write_scan::scan(&source);
            if found.status_writes == 0 {
                continue;
            }
            if let Some((entry, _reason)) =
                TEST_SEEDING_WRITES.iter().find(|(file, _)| *file == relative)
            {
                allowlist_used.push(*entry);
                continue;
            }
            with_writes += 1;
            if found.purge_calls < found.status_writes {
                unpaired.push(format!(
                    "{relative}: {} status write(s), {} purge_run call(s)",
                    found.status_writes, found.purge_calls
                ));
            }
        }
        assert!(
            with_writes >= 3,
            "the scan found status writes in {with_writes} files; it is not reading the sources"
        );
        for (file, reason) in TEST_SEEDING_WRITES {
            assert!(
                allowlist_used.contains(file),
                "the allowlist entry {file} ({reason}) matches no status write; remove it"
            );
        }
        assert!(
            unpaired.is_empty(),
            "every `UPDATE runs` that assigns status must be paired with a `purge_run(` call in \
             the same file (add the purge in the same transaction, or allowlist a test-only \
             file): {unpaired:#?}"
        );
    }

    /// The database-backed tests need a scratch database. Run them with
    /// `DATABASE_URL=... cargo test -p session-core --bin session-core loop_checkpoint -- --ignored --test-threads=1`
    /// against a scratch Postgres (the database name must say it is a test database).
    mod db {
        use super::super::test_support::{
            cipher, fold_for, fold_rows, fold_state, park_fold_for, plant_round_fold,
            scratch_pool, seed_fold, seed_run,
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
            .map(|saved| saved.seq)
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

            // The recovery worker claims any due obligation in the database, and one
            // pass takes at most a batch, so in a shared database this run may not be
            // in the first batch. Pass again, bounded, until this run is terminal, and
            // assert on this run rather than on the count recovered.
            for _ in 0..100 {
                crate::terminalization::recover_due_terminalizations(&pool)
                    .await
                    .expect("recover");
                if run_status(&pool, &run_id).await == "failed" {
                    break;
                }
                // Not terminal yet: this run was past the batch, or another worker
                // holds its lease. Give that a moment instead of spinning.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }

            assert_eq!(run_status(&pool, &run_id).await, "failed");
            assert_eq!(fold_rows(&pool, &run_id).await, 0);
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_org_erasure_removes_the_fold() {
            // This proves the OUTCOME: after an org is erased no fold is left. It does
            // not prove which statement removed it, because the foreign key from
            // `run_loop_checkpoints` to `runs` cascades and covers the fold too.
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

            // The same token naming a DIFFERENT pause is not holding that pause: the
            // probe asks about the pause reference too, so this is not claimable, not
            // held. The live lease on the real pause is left as it was.
            let other_pause = claim(&pool, &cipher(), &run_id, "appr_2", "token-a")
                .await
                .expect_err("the token holds appr_1, not appr_2");
            assert_eq!(other_pause.code(), tonic::Code::FailedPrecondition);
            assert_eq!(other_pause.message(), "loop_checkpoint_not_claimable");
            let still_held = claim(&pool, &cipher(), &run_id, "appr_1", "token-a")
                .await
                .expect_err("the lease on appr_1 is still live");
            assert_eq!(still_held.message(), "loop_checkpoint_claim_held");

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

        /// A `pre_effect` save, the kind an in-place resume writes as its fence.
        async fn save_fence_at(pool: &PgPool, run_id: &str, expected_seq: i64) -> Result<i64, Status> {
            let fold = fold_for(run_id);
            save(
                pool,
                &cipher(),
                SaveInput {
                    run_id,
                    expected_seq,
                    kind: "pre_effect",
                    fold: &fold,
                    authority_digest: "auth-1",
                    pause_ref: "",
                    park_reason: "",
                    park_until: None,
                },
            )
            .await
            .map(|saved| saved.seq)
        }

        async fn claim_columns(pool: &PgPool, run_id: &str) -> (Option<String>, bool) {
            sqlx::query_as::<_, (Option<String>, bool)>(
                "SELECT claim_token_hash, claim_expires_at IS NULL
                   FROM run_loop_checkpoints WHERE run_id = $1",
            )
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("claim columns")
        }

        /// The fencing argument of the in-place resume (WS03 S7b). `claim` does not bump the
        /// seq, so two claimants can hold the SAME seq (a claim whose lease lapsed, or a
        /// replay). The executor's fence is a `pre_effect` save at the claimed seq before it
        /// dispatches anything, and `save` is a compare-and-set on that seq under the row lock,
        /// so of any number of fences at one seq exactly one wins. The row then becomes
        /// `active`, which is never claimable, so a later claimant gets `not_claimable`.
        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_a_fence_at_the_claimed_seq_is_won_exactly_once() {
            let Some(pool) = scratch_pool().await else { return };

            // One claimant: the fence at the claimed seq wins once, and only once.
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &run_id, "pause", "appr_1", "", None)
                .await
                .expect("pause fold");
            let claimed = claim(&pool, &cipher(), &run_id, "appr_1", "token-a")
                .await
                .expect("the claim wins");
            assert_eq!(claimed.seq, 1, "the claim does not bump the seq");
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("resuming"));
            assert_eq!(
                save_fence_at(&pool, &run_id, claimed.seq)
                    .await
                    .expect("the fence at the claimed seq wins"),
                2
            );
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("active"));
            assert_eq!(
                claim_columns(&pool, &run_id).await,
                (None, true),
                "the fence clears the claim"
            );
            let again = save_fence_at(&pool, &run_id, claimed.seq)
                .await
                .expect_err("a second fence at the claimed seq loses");
            assert_eq!(again.code(), tonic::Code::Aborted);
            assert_eq!(again.message(), "loop_checkpoint_stale");
            let reclaim = claim(&pool, &cipher(), &run_id, "appr_1", "token-b")
                .await
                .expect_err("an active row is never claimable");
            assert_eq!(reclaim.code(), tonic::Code::FailedPrecondition);
            assert_eq!(reclaim.message(), "loop_checkpoint_not_claimable");

            // Two claimants at the same seq: A claims, A's lease lapses, B claims the same
            // pause. Both hold seq 1; A's and B's fences race, and exactly one wins.
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &run_id, "pause", "appr_1", "", None)
                .await
                .expect("pause fold");
            let a = claim(&pool, &cipher(), &run_id, "appr_1", "token-a").await.expect("A claims");
            sqlx::query(
                "UPDATE run_loop_checkpoints SET claim_expires_at = now() - interval '1 second' WHERE run_id = $1",
            )
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("lapse A's lease");
            let b = claim(&pool, &cipher(), &run_id, "appr_1", "token-b")
                .await
                .expect("B claims after the lapse");
            assert_eq!((a.seq, b.seq), (1, 1), "two claimants hold one seq");
            let (fence_a, fence_b) = tokio::join!(
                save_fence_at(&pool, &run_id, a.seq),
                save_fence_at(&pool, &run_id, b.seq),
            );
            let wins = [&fence_a, &fence_b].iter().filter(|fence| fence.is_ok()).count();
            assert_eq!(wins, 1, "exactly one fence wins: {fence_a:?} / {fence_b:?}");
            let loser = [fence_a, fence_b]
                .into_iter()
                .find_map(Result::err)
                .expect("one fence lost");
            assert_eq!(loser.code(), tonic::Code::Aborted);
            assert_eq!(loser.message(), "loop_checkpoint_stale");
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("active"));
            let stored: i64 = sqlx::query_scalar("SELECT seq FROM run_loop_checkpoints WHERE run_id = $1")
                .bind(&run_id)
                .fetch_one(&pool)
                .await
                .expect("read the fold seq");
            assert_eq!(stored, 2, "one fence moved the seq, once");
            let late = claim(&pool, &cipher(), &run_id, "appr_1", "token-c")
                .await
                .expect_err("after the fence the pause is not claimable");
            assert_eq!(late.message(), "loop_checkpoint_not_claimable");
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

        // -- Build plan WS03 S8-4: the park rules at the store ----------------------------------
        //
        // WRITTEN, NOT RUN by the author: each needs the disposable-Postgres harness
        // (scripts/tests/session-core-db-tests.sh), which the lead runs. The park switches are
        // forced for the test's own thread (`ParkRules`), so no test sets the process
        // environment, and the park wait is read from `park_ttl_seconds()` rather than assumed.

        use crate::terminalization::park_test_switch;

        /// The fold store and park recovery forced on or off for this thread, restored on drop.
        struct ParkRules {
            _store: test_switch::Forced,
            _park: park_test_switch::Forced,
        }

        fn forced_rules(on: bool) -> ParkRules {
            ParkRules {
                _store: test_switch::Forced::set(true),
                _park: park_test_switch::Forced::set(on),
            }
        }

        async fn save_park_fold(
            pool: &PgPool,
            run_id: &str,
            expected_seq: i64,
            fold: &[u8],
            pause_ref: &str,
            reason: &str,
            until: DateTime<Utc>,
        ) -> Result<Saved, Status> {
            save(
                pool,
                &cipher(),
                SaveInput {
                    run_id,
                    expected_seq,
                    kind: "park",
                    fold,
                    authority_digest: "auth-1",
                    pause_ref,
                    park_reason: reason,
                    park_until: Some(until),
                },
            )
            .await
        }

        /// A save of any kind of the golden fold (not a park-shaped one).
        async fn save_kind(
            pool: &PgPool,
            run_id: &str,
            expected_seq: i64,
            kind: &str,
            pause_ref: &str,
        ) -> Result<Saved, Status> {
            let fold = fold_for(run_id);
            save(
                pool,
                &cipher(),
                SaveInput {
                    run_id,
                    expected_seq,
                    kind,
                    fold: &fold,
                    authority_digest: "auth-1",
                    pause_ref,
                    park_reason: "",
                    park_until: None,
                },
            )
            .await
        }

        async fn updated_at(pool: &PgPool, run_id: &str) -> DateTime<Utc> {
            sqlx::query_scalar("SELECT updated_at FROM run_loop_checkpoints WHERE run_id = $1")
                .bind(run_id)
                .fetch_one(pool)
                .await
                .expect("read the fold's age")
        }

        async fn events_of(pool: &PgPool, run_id: &str, event_type: &str) -> i64 {
            sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE run_id = $1 AND event_type = $2")
                .bind(run_id)
                .bind(event_type)
                .fetch_one(pool)
                .await
                .expect("count events")
        }

        /// Make the park due: its `park_until` moved into the past.
        async fn make_park_due(pool: &PgPool, run_id: &str) {
            sqlx::query(
                "UPDATE run_loop_checkpoints SET park_until = now() - interval '1 second' WHERE run_id = $1",
            )
            .bind(run_id)
            .execute(pool)
            .await
            .expect("make the park due");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_a_park_save_is_validated_and_echoed_only_under_park_recovery() {
            let Some(pool) = scratch_pool().await else { return };
            let soon = Utc::now() + chrono::Duration::seconds(3600);

            // Park recovery OFF: a park is stored exactly as it always was, whatever its fold and
            // reference say, and the store does not say it accepted it.
            {
                let _off = forced_rules(false);
                let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
                let golden = fold_for(&run_id);
                let saved =
                    save_park_fold(&pool, &run_id, 0, &golden, "appr_1", "provider_exhausted", soon)
                        .await
                        .expect("committed: a park with a call in flight is stored");
                assert_eq!(saved, Saved { seq: 1, park_accepted: false });
                assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("parked"));
                // Not even a reserved reason or a wait past the cap is refused.
                let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
                let far = Utc::now() + chrono::Duration::seconds(park_ttl_seconds() * 2);
                let saved = save_park_fold(
                    &pool,
                    &run_id,
                    0,
                    &fold_for(&run_id),
                    "appr_1",
                    "crash_interrupted",
                    far,
                )
                .await
                .expect("committed: no park rule is asked");
                assert!(!saved.park_accepted);
            }

            let _on = forced_rules(true);
            let wait_cap = park_ttl_seconds();

            // Each rule refuses with its own token, and a refused park writes nothing.
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            let good_ref = park_ref(&run_id, 1);
            let good = park_fold_for(&run_id, &good_ref, 1);
            let cases: Vec<(&str, Vec<u8>, String, &str, i64, (&str, &str))> = vec![
                (
                    "a reserved reason",
                    good.clone(),
                    good_ref.clone(),
                    "crash_interrupted",
                    3600,
                    ("invalid_argument", "park_reason_reserved"),
                ),
                (
                    "a call in flight and an approval pending",
                    fold_for(&run_id),
                    good_ref.clone(),
                    "provider_exhausted",
                    3600,
                    ("invalid_argument", "park_not_at_round_boundary"),
                ),
                (
                    "a sixth park",
                    park_fold_for(&run_id, &park_ref(&run_id, 6), 6),
                    park_ref(&run_id, 6),
                    "provider_exhausted",
                    3600,
                    ("failed_precondition", "park_limit_reached"),
                ),
                (
                    "a reference that is not the run's own park",
                    park_fold_for(&run_id, "appr_1", 1),
                    "appr_1".to_owned(),
                    "provider_exhausted",
                    3600,
                    ("invalid_argument", "park_ref_mismatch"),
                ),
                (
                    "a wait past half the park's life",
                    good.clone(),
                    good_ref.clone(),
                    "provider_exhausted",
                    wait_cap,
                    ("failed_precondition", "park_until_beyond_ttl"),
                ),
            ];
            for (what, fold, pause_ref, reason, wait_secs, want) in cases {
                let until = Utc::now() + chrono::Duration::seconds(wait_secs);
                let refused = save_park_fold(&pool, &run_id, 0, &fold, &pause_ref, reason, until)
                    .await
                    .expect_err(what);
                assert_eq!(refusal_pair(&refused), want, "{what}");
                assert_eq!(fold_rows(&pool, &run_id).await, 0, "{what}: nothing was written");
            }

            // A park that obeys them is stored, and the store says so.
            let saved = save_park_fold(&pool, &run_id, 0, &good, &good_ref, "provider_exhausted", soon)
                .await
                .expect("a park at a round boundary under the run's own reference");
            assert_eq!(saved, Saved { seq: 1, park_accepted: true });
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("parked"));
            let stored: (Option<String>, Option<String>) = sqlx::query_as(
                "SELECT pause_ref, park_reason FROM run_loop_checkpoints WHERE run_id = $1",
            )
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("stored park fields");
            assert_eq!(
                stored,
                (Some(good_ref.clone()), Some("provider_exhausted".to_owned()))
            );
            assert_eq!(events_of(&pool, &run_id, "CHECKPOINT_SAVED").await, 1);

            // The compare-and-set is unchanged: a stale writer is refused even with a good park.
            let stale = save_park_fold(&pool, &run_id, 0, &good, &good_ref, "provider_exhausted", soon)
                .await
                .expect_err("a writer that still expects 0 is stale");
            assert_eq!(refusal_pair(&stale), ("aborted", "loop_checkpoint_stale"));

            // The second park of the run takes the next reference and replaces the first.
            let second_ref = park_ref(&run_id, 2);
            let second = park_fold_for(&run_id, &second_ref, 2);
            let saved = save_park_fold(&pool, &run_id, 1, &second, &second_ref, "upstream_unavailable", soon)
                .await
                .expect("the run's second park");
            assert_eq!(saved, Saved { seq: 2, park_accepted: true });
            // The count and the reference must agree: a fold that counts the run's second park
            // under the first park's reference is refused.
            let reused = save_park_fold(
                &pool,
                &run_id,
                2,
                &park_fold_for(&run_id, &good_ref, 2),
                &good_ref,
                "provider_exhausted",
                soon,
            )
            .await
            .expect_err("the count says the second park, the reference says the first");
            assert_eq!(refusal_pair(&reused), ("invalid_argument", "park_ref_mismatch"));

            // Every other save is never accepted as a park, park recovery on or off.
            for rules_on in [true, false] {
                let _rules = forced_rules(rules_on);
                let (other, _org, _user) = seed_run(&pool, "persistent").await;
                let mut seq = 0;
                for kind in ["round", "pre_effect", "pause"] {
                    let pause_ref = if kind == "pause" { "appr_1" } else { "" };
                    let saved = save_kind(&pool, &other, seq, kind, pause_ref)
                        .await
                        .expect("a non-park save");
                    assert!(!saved.park_accepted, "{kind} with park recovery {rules_on}");
                    seq = saved.seq;
                }
            }
        }

        /// A zero-retention or waived run never keeps a park, under the park rules or without
        /// them: the refusal is the one `run_owner_tx` gives every durable fold. (Park StoreProof.)
        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_a_park_save_is_refused_for_a_zdr_or_waived_run() {
            let Some(pool) = scratch_pool().await else { return };
            let soon = Utc::now() + chrono::Duration::seconds(3600);
            let refusal = Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(Code::FailedPrecondition);
            for on in [true, false] {
                let _rules = forced_rules(on);
                for retention in ["zdr", "zdr_waived"] {
                    let (run_id, _org, _user) = seed_run(&pool, retention).await;
                    let pause_ref = park_ref(&run_id, 1);
                    let fold = park_fold_for(&run_id, &pause_ref, 1);
                    let refused =
                        save_park_fold(&pool, &run_id, 0, &fold, &pause_ref, "provider_exhausted", soon)
                            .await
                            .expect_err("a non-persistent run keeps no park");
                    assert_eq!(refused.code(), tonic::Code::FailedPrecondition, "{retention} {on}");
                    assert_eq!(refused.message(), refusal.message(), "{retention} {on}");
                    assert_eq!(fold_rows(&pool, &run_id).await, 0, "{retention} {on}: nothing stored");
                }
            }
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_an_empty_ref_claims_only_a_due_park() {
            let Some(pool) = scratch_pool().await else { return };
            let _on = forced_rules(true);
            let not_claimable = ("failed_precondition", "loop_checkpoint_not_claimable");
            let far = Utc::now() + chrono::Duration::seconds(3600);

            // A pause fold, an active fold and a closed one are never claimed by an empty ref.
            let (paused, _org, _user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &paused, "pause", "appr_1", "", None).await.expect("pause fold");
            let (active, _org, _user) = seed_run(&pool, "persistent").await;
            save_kind(&pool, &active, 0, "round", "").await.expect("active fold");
            let (closed, _org, _user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &closed, "park", &park_ref(&closed, 1), "provider_exhausted", Some(far))
                .await
                .expect("a park");
            make_park_due(&pool, &closed).await;
            close(&pool, &closed, 1, "park_not_taken").await.expect("close");
            for (what, run_id, state) in [
                ("a pause fold", &paused, "paused"),
                ("an active fold", &active, "active"),
                ("a closed park", &closed, "closed"),
            ] {
                let refused = claim(&pool, &cipher(), run_id, "", "token-a")
                    .await
                    .expect_err(what);
                assert_eq!(refusal_pair(&refused), not_claimable, "{what}");
                assert_eq!(fold_state(&pool, run_id).await.as_deref(), Some(state), "{what}");
            }
            // No fold at all.
            let (bare, _org, _user) = seed_run(&pool, "persistent").await;
            assert_eq!(
                refusal_pair(&claim(&pool, &cipher(), &bare, "", "token-a").await.expect_err("no fold")),
                not_claimable
            );

            // A park whose time has not come: park_not_due, and NOTHING is written.
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            let park = park_ref(&run_id, 1);
            seed_fold(&pool, &run_id, "park", &park, "provider_exhausted", Some(far))
                .await
                .expect("a park");
            // Age the park: its age is the anchor of the worker's wait, and a claim must not move it.
            sqlx::query(
                "UPDATE run_loop_checkpoints SET updated_at = now() - interval '10 minutes' WHERE run_id = $1",
            )
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("age the park");
            let aged = updated_at(&pool, &run_id).await;
            let not_due = claim(&pool, &cipher(), &run_id, "", "token-a")
                .await
                .expect_err("the park is not due");
            assert_eq!(refusal_pair(&not_due), ("failed_precondition", "park_not_due"));
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("parked"));
            assert_eq!(claim_columns(&pool, &run_id).await, (None, true), "no lease taken");
            assert_eq!(updated_at(&pool, &run_id).await, aged);
            assert_eq!(events_of(&pool, &run_id, "CHECKPOINT_RESTORED").await, 0);

            // Due: the claim wins, returns the park fold, and leaves the park's age alone.
            make_park_due(&pool, &run_id).await;
            let claimed = claim(&pool, &cipher(), &run_id, "", "token-a")
                .await
                .expect("a due park is claimed by an empty ref");
            assert_eq!((claimed.seq, claimed.kind.as_str(), claimed.segment_no), (1, "park", 1));
            let restored: Value = serde_json::from_slice(&claimed.fold).expect("the park fold");
            assert_eq!(restored["pending"]["kind"], "park");
            assert_eq!(restored["pending"]["pause_ref"], park.as_str());
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("resuming"));
            assert_eq!(updated_at(&pool, &run_id).await, aged, "a claim is not a new park");
            assert_eq!(events_of(&pool, &run_id, "CHECKPOINT_RESTORED").await, 1);

            // A live lease gives claim_held, for the same token and for another one.
            for token in ["token-a", "token-b"] {
                let held = claim(&pool, &cipher(), &run_id, "", token)
                    .await
                    .expect_err("the lease is live");
                assert_eq!(refusal_pair(&held), ("aborted", "loop_checkpoint_claim_held"), "{token}");
            }
            assert_eq!(events_of(&pool, &run_id, "CHECKPOINT_RESTORED").await, 1);

            // A lapsed lease is claimed again, by anyone, at the same seq.
            sqlx::query(
                "UPDATE run_loop_checkpoints SET claim_expires_at = now() - interval '1 second' WHERE run_id = $1",
            )
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("lapse the lease");
            let again = claim(&pool, &cipher(), &run_id, "", "token-b")
                .await
                .expect("a lapsed resume lease of a park is re-claimable by an empty ref");
            assert_eq!(again.seq, 1);
            assert_eq!(updated_at(&pool, &run_id).await, aged, "still the park's own age");
            let held = claim(&pool, &cipher(), &run_id, "", "token-a")
                .await
                .expect_err("token-b holds it now");
            assert_eq!(refusal_pair(&held), ("aborted", "loop_checkpoint_claim_held"));

            // A fence at the claimed seq wins once and makes the row active: no longer a park.
            assert_eq!(save_fence_at(&pool, &run_id, again.seq).await.expect("the fence"), 2);
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("active"));
            let late = claim(&pool, &cipher(), &run_id, "", "token-c")
                .await
                .expect_err("after the fence there is no park");
            assert_eq!(refusal_pair(&late), not_claimable);

            // An ended run is refused as it is for every claim.
            let (ended, _org, _user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &ended, "park", &park_ref(&ended, 1), "provider_exhausted", Some(far))
                .await
                .expect("a park");
            make_park_due(&pool, &ended).await;
            super::super::test_support::end_run_without_purge(&pool, &ended).await;
            let refused = claim(&pool, &cipher(), &ended, "", "token-a")
                .await
                .expect_err("an ended run");
            assert_eq!(refusal_pair(&refused), ("failed_precondition", "loop_checkpoint_run_ended"));

            // Park recovery OFF: the empty reference is the committed statement, which it
            // matches no row of, so even a due park is not claimable.
            drop(_on);
            let _off = forced_rules(false);
            let (off_run, _org, _user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &off_run, "park", "appr_1", "provider_exhausted", Some(far))
                .await
                .expect("committed park");
            make_park_due(&pool, &off_run).await;
            let refused = claim(&pool, &cipher(), &off_run, "", "token-a")
                .await
                .expect_err("off: an empty ref names nothing");
            assert_eq!(refusal_pair(&refused), not_claimable);
            assert_eq!(fold_state(&pool, &off_run).await.as_deref(), Some("parked"));
        }

        /// A claim by a NON-empty reference. With park recovery OFF it is the committed statement:
        /// it does not wait for `park_until`, it moves the row's age, and the claim_held probe
        /// compares the token and the reference. With park recovery ON the park's own reference
        /// (`park:{run_id}:{n}`, which anyone can compute) claims NOTHING: the committed statement
        /// would take the park before `park_until` and set `updated_at`, the anchor of its TTL, so
        /// a caller could re-claim each time the lease lapses and the park would never expire. A
        /// pause fold is claimed by its reference either way.
        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_a_non_empty_ref_claim_is_the_committed_statement() {
            let Some(pool) = scratch_pool().await else { return };
            let far = Utc::now() + chrono::Duration::seconds(3600);
            let not_claimable = ("failed_precondition", "loop_checkpoint_not_claimable");

            // OFF: the committed statement, exactly as it was.
            {
                let _off = forced_rules(false);
                let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
                let park = park_ref(&run_id, 1);
                seed_fold(&pool, &run_id, "park", &park, "provider_exhausted", Some(far))
                    .await
                    .expect("a park that is not due");
                sqlx::query(
                    "UPDATE run_loop_checkpoints SET updated_at = now() - interval '10 minutes' WHERE run_id = $1",
                )
                .bind(&run_id)
                .execute(&pool)
                .await
                .expect("age the park");
                let aged = updated_at(&pool, &run_id).await;

                // The wrong reference matches nothing; the right one claims the park at once, due or not.
                let wrong = claim(&pool, &cipher(), &run_id, "park:other:1", "token-a")
                    .await
                    .expect_err("another reference");
                assert_eq!(wrong.message(), "loop_checkpoint_not_claimable");
                let claimed = claim(&pool, &cipher(), &run_id, &park, "token-a")
                    .await
                    .expect("the committed statement does not read park_until");
                assert_eq!((claimed.seq, claimed.kind.as_str()), (1, "park"));
                assert!(
                    updated_at(&pool, &run_id).await > aged,
                    "the committed claim sets the row's age"
                );

                // The committed held probe: the same token and reference is held, another is lost.
                let held = claim(&pool, &cipher(), &run_id, &park, "token-a")
                    .await
                    .expect_err("held");
                assert_eq!(held.message(), "loop_checkpoint_claim_held");
                let lost = claim(&pool, &cipher(), &run_id, &park, "token-b")
                    .await
                    .expect_err("lost");
                assert_eq!(lost.message(), "loop_checkpoint_not_claimable");
            }

            // ON: a pause fold is claimed by its reference as it always was ...
            let _on = forced_rules(true);
            let (paused, _org, _user) = seed_run(&pool, "persistent").await;
            seed_fold(&pool, &paused, "pause", "appr_1", "", None).await.expect("pause fold");
            assert_eq!(
                claim(&pool, &cipher(), &paused, "appr_1", "token-a").await.expect("pause claim").kind,
                "pause"
            );

            // ... and a park is not claimed by its own reference, due or not, parked or with a
            // lapsed lease, and the refusal writes nothing: the park keeps its age and its lease.
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            let park = park_ref(&run_id, 1);
            seed_fold(&pool, &run_id, "park", &park, "provider_exhausted", Some(far))
                .await
                .expect("a park that is not due");
            sqlx::query(
                "UPDATE run_loop_checkpoints SET updated_at = now() - interval '10 minutes' WHERE run_id = $1",
            )
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("age the park");
            let aged = updated_at(&pool, &run_id).await;
            for (what, named) in [("the park's own reference", park.as_str()), ("another reference", "park:other:1")] {
                let refused = claim(&pool, &cipher(), &run_id, named, "token-a")
                    .await
                    .expect_err(what);
                assert_eq!(refusal_pair(&refused), not_claimable, "{what}");
            }
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("parked"));
            assert_eq!(claim_columns(&pool, &run_id).await, (None, true), "no lease taken");
            assert_eq!(updated_at(&pool, &run_id).await, aged, "the refusal did not move the TTL anchor");
            assert_eq!(events_of(&pool, &run_id, "CHECKPOINT_RESTORED").await, 0);

            // Due, and then resumed by the empty reference (the legitimate claim): a named
            // reference cannot re-claim it once the lease has lapsed, and the anchor stays put.
            make_park_due(&pool, &run_id).await;
            let refused = claim(&pool, &cipher(), &run_id, &park, "token-a")
                .await
                .expect_err("a due park is still not claimed by its reference");
            assert_eq!(refusal_pair(&refused), not_claimable);
            assert_eq!(updated_at(&pool, &run_id).await, aged);
            let claimed = claim(&pool, &cipher(), &run_id, "", "token-a")
                .await
                .expect("the empty reference claims the due park");
            assert_eq!((claimed.seq, claimed.kind.as_str()), (1, "park"));
            sqlx::query(
                "UPDATE run_loop_checkpoints SET claim_expires_at = now() - interval '1 second' WHERE run_id = $1",
            )
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("lapse the lease");
            for token in ["token-a", "token-b"] {
                let refused = claim(&pool, &cipher(), &run_id, &park, token)
                    .await
                    .expect_err("a lapsed lease is not re-claimed through the park's reference");
                assert_eq!(refusal_pair(&refused), not_claimable, "{token}");
            }
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("resuming"));
            assert_eq!(updated_at(&pool, &run_id).await, aged, "the park's TTL anchor never moved");
            assert_eq!(events_of(&pool, &run_id, "CHECKPOINT_RESTORED").await, 1);
            // The empty reference still takes it.
            let again = claim(&pool, &cipher(), &run_id, "", "token-b")
                .await
                .expect("the lapsed lease is claimed by the empty reference");
            assert_eq!(again.seq, 1);
            assert_eq!(updated_at(&pool, &run_id).await, aged);
        }

        /// `SaveLoopCheckpoint` served through its handler function: the response carries the
        /// store's own `park_accepted`.
        async fn save_rpc_response(
            pool: &PgPool,
            owner: &crate::auth::VerifiedIdentity,
            run_id: &str,
            expected_seq: u64,
            kind: &str,
            fold: Vec<u8>,
            pause_ref: &str,
            park: Option<(&str, DateTime<Utc>)>,
        ) -> Result<pb::SaveLoopCheckpointResponse, Status> {
            crate::grpc::save_loop_checkpoint_inner(
                pool,
                owner,
                pb::SaveLoopCheckpointRequest {
                    run_id: run_id.to_owned(),
                    expected_seq,
                    kind: kind.to_owned(),
                    fold,
                    authority_digest: "auth-1".to_owned(),
                    pause_ref: pause_ref.to_owned(),
                    park_reason: park.map_or_else(String::new, |(reason, _)| reason.to_owned()),
                    park_until: park.map(|(_, until)| prost_types::Timestamp {
                        seconds: until.timestamp(),
                        nanos: i32::try_from(until.timestamp_subsec_nanos()).expect("nanos"),
                    }),
                },
                true,
                || Ok(cipher()),
            )
            .await
            .map(tonic::Response::into_inner)
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_the_save_rpc_echoes_park_accepted_from_the_store() {
            let Some(pool) = scratch_pool().await else { return };
            let soon = Utc::now() + chrono::Duration::seconds(3600);

            // Park recovery on: a park under the park rules is echoed true; every other save
            // (a round, a pause) false; a refused park is an error and no echo at all.
            {
                let _on = forced_rules(true);
                let (run_id, org, _user) = seed_run(&pool, "persistent").await;
                let owner = crate::auth::VerifiedIdentity::user_for_test(&org, "u1");
                let refused = save_rpc_response(
                    &pool,
                    &owner,
                    &run_id,
                    0,
                    "park",
                    fold_for(&run_id),
                    "appr_1",
                    Some(("provider_exhausted", soon)),
                )
                .await
                .expect_err("not a park under the run's reference");
                assert_eq!(refused.message(), "park_not_at_round_boundary");
                let pause_ref = park_ref(&run_id, 1);
                let parked = save_rpc_response(
                    &pool,
                    &owner,
                    &run_id,
                    0,
                    "park",
                    park_fold_for(&run_id, &pause_ref, 1),
                    &pause_ref,
                    Some(("provider_exhausted", soon)),
                )
                .await
                .expect("a park under the park rules");
                assert_eq!((parked.seq, parked.park_accepted), (1, true));
                let round = save_rpc_response(&pool, &owner, &run_id, 1, "round", fold_for(&run_id), "", None)
                    .await
                    .expect("a round");
                assert_eq!((round.seq, round.park_accepted), (2, false));
                let pause = save_rpc_response(&pool, &owner, &run_id, 2, "pause", fold_for(&run_id), "appr_1", None)
                    .await
                    .expect("a pause");
                assert_eq!((pause.seq, pause.park_accepted), (3, false));
            }

            // Park recovery off: the committed store takes the same park and does not echo it.
            {
                let _off = forced_rules(false);
                let (run_id, org, _user) = seed_run(&pool, "persistent").await;
                let owner = crate::auth::VerifiedIdentity::user_for_test(&org, "u1");
                let pause_ref = park_ref(&run_id, 1);
                let parked = save_rpc_response(
                    &pool,
                    &owner,
                    &run_id,
                    0,
                    "park",
                    park_fold_for(&run_id, &pause_ref, 1),
                    &pause_ref,
                    Some(("provider_exhausted", soon)),
                )
                .await
                .expect("a park stored as committed");
                assert_eq!((parked.seq, parked.park_accepted), (1, false));
            }
        }

        /// The committed park-seeding tests name the reference `appr_1` and use the golden fold's
        /// shape. They run with park recovery off (the harness scrubs the switch), where the park
        /// rules are not asked; `seed_fold` now writes a park-SHAPED fold, so the same helper also
        /// stands under the park rules when it is given the run's own reference.
        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn loop_checkpoint_the_committed_park_seeding_tests_pass_through_the_amended_seed_fold() {
            let Some(pool) = scratch_pool().await else { return };
            let until = Utc::now() + chrono::Duration::seconds(3600);

            {
                let _off = forced_rules(false);
                for reason in mp_contracts::loop_fold::ParkReason::ALL {
                    let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
                    seed_fold(&pool, &run_id, "park", "appr_1", reason.as_str(), Some(until))
                        .await
                        .expect("committed: a park seeded under `appr_1`");
                    assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("parked"));
                    // It opens and parses, and it is a park fold under the seeded reference.
                    let claimed = claim(&pool, &cipher(), &run_id, "appr_1", "token-a")
                        .await
                        .expect("the committed claim by reference");
                    let fold = check_fold(&run_id, &claimed.fold).expect("a fold of this run");
                    let pending = fold.pending.expect("a pending park");
                    assert_eq!((pending.kind, pending.pause_ref.as_str()), (PauseKind::Park, "appr_1"));
                    assert!(fold.round_in_flight.is_none());
                }
            }

            let _on = forced_rules(true);
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            let refused = seed_fold(&pool, &run_id, "park", "appr_1", "provider_exhausted", Some(until))
                .await
                .expect_err("under the park rules `appr_1` is not the run's park reference");
            assert_eq!(refused.message(), "park_ref_mismatch");
            seed_fold(&pool, &run_id, "park", &park_ref(&run_id, 1), "provider_exhausted", Some(until))
                .await
                .expect("the run's own reference stands under the park rules");
            assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("parked"));
        }

        /// Plant a crash park the way the recovery worker leaves one, `age_seconds` old: a round fold turned into a park
        /// (`kind` `park`, `state` `parked`, reason `crash_interrupted`) whose `updated_at` is that far in the past.
        async fn plant_crash_park(pool: &PgPool, run_id: &str, age_seconds: f64) {
            plant_round_fold(pool, run_id, &fold_for(run_id), "auth-1").await;
            sqlx::query(
                "UPDATE run_loop_checkpoints
                    SET kind = 'park', state = 'parked', pause_ref = $2, park_reason = 'crash_interrupted',
                        park_until = now() + interval '1 hour', updated_at = now() - make_interval(secs => $3)
                  WHERE run_id = $1",
            )
            .bind(run_id)
            .bind(format!("park-{run_id}"))
            .bind(age_seconds)
            .execute(pool)
            .await
            .expect("plant a crash park");
        }

        /// How many notices the outbox holds for a run, and whether the park carries its notice mark.
        async fn notice_state(pool: &PgPool, run_id: &str) -> (i64, bool) {
            let park = format!("park-{run_id}");
            let rows: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM session_audit_outbox WHERE event_id = $1")
                    .bind(crate::attention_events::crash_park_notice_event_id(
                        run_id, &park,
                    ))
                    .fetch_one(pool)
                    .await
                    .expect("count notices");
            let stamped: Option<bool> = sqlx::query_scalar(
                "SELECT park_notice_sent_at IS NOT NULL FROM run_loop_checkpoints WHERE run_id = $1",
            )
            .bind(run_id)
            .fetch_optional(pool)
            .await
            .expect("read the notice mark");
            (rows, stamped.unwrap_or(false))
        }

        /// The notice time and the park TTL the tests run under: four hours and one day.
        const NOTICE_SECONDS: i64 = 14_400;
        const TTL_SECONDS: i64 = 86_400;

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_due_crash_park_is_announced_once_and_stamped() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &run_id, 5.0 * 3600.0).await;

            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass");
            assert_eq!(
                notice_state(&pool, &run_id).await,
                (1, true),
                "one notice, and the park is stamped"
            );

            // The payload names ids and the closed tokens only, and it is a persistent run's notice.
            let payload: serde_json::Value =
                sqlx::query_scalar("SELECT payload FROM session_audit_outbox WHERE event_id = $1")
                    .bind(crate::attention_events::crash_park_notice_event_id(
                        &run_id,
                        &format!("park-{run_id}"),
                    ))
                    .fetch_one(&pool)
                    .await
                    .expect("read the notice");
            assert_eq!(payload["payload"]["state"], "ATTENTION_STATE_NEEDS_INPUT");
            assert_eq!(payload["payload"]["reason"], "ATTENTION_REASON_PARKED");
            assert_eq!(payload["payload"]["run_id"], run_id.as_str());
            assert_eq!(payload["zdr"], false);

            // A later pass finds the park stamped, and writes nothing more for it.
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the second pass");
            assert_eq!(
                notice_state(&pool, &run_id).await,
                (1, true),
                "still one notice"
            );
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_park_that_is_young_claimed_resumed_or_expired_is_not_announced() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            // Young: an hour old, before the notice time.
            let (young, _, _) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &young, 3600.0).await;
            // Claimed: a continuation holds the park.
            let (claimed, _, _) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &claimed, 5.0 * 3600.0).await;
            sqlx::query("UPDATE run_loop_checkpoints SET state = 'resuming' WHERE run_id = $1")
                .bind(&claimed)
                .execute(&pool)
                .await
                .expect("claim the park");
            // Resumed: the run is no longer a park.
            let (resumed, _, _) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &resumed, 5.0 * 3600.0).await;
            sqlx::query(
                "UPDATE run_loop_checkpoints SET kind = 'round', state = 'active' WHERE run_id = $1",
            )
            .bind(&resumed)
            .execute(&pool)
            .await
            .expect("resume the run");
            // Expired: older than the park TTL, so the worker ends the run instead.
            let (expired, _, _) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &expired, 2.0 * 86_400.0).await;

            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass");
            for run in [&young, &claimed, &resumed, &expired] {
                assert_eq!(
                    notice_state(&pool, run).await,
                    (0, false),
                    "{run} is not announced"
                );
            }
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_zero_retention_or_waived_run_is_never_announced() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            for retention in ["zdr", "zdr_waived"] {
                let (run_id, _org, _user) = seed_run(&pool, retention).await;
                plant_crash_park(&pool, &run_id, 5.0 * 3600.0).await;
                notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                    .await
                    .expect("the notice pass");
                assert_eq!(
                    notice_state(&pool, &run_id).await,
                    (0, false),
                    "a {retention} run is not announced"
                );
            }
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn two_replicas_announce_a_crash_park_once() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &run_id, 5.0 * 3600.0).await;

            let (first, second) = tokio::join!(
                notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now()),
                notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now()),
            );
            first.expect("replica one");
            second.expect("replica two");
            assert_eq!(
                notice_state(&pool, &run_id).await,
                (1, true),
                "one row between two replicas"
            );
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn with_the_notice_switched_off_nothing_is_announced() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            if crash_park_notice_active() {
                eprintln!("skipping: the crash-park notice is on in this environment");
                return;
            }
            let (run_id, _org, _user) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &run_id, 5.0 * 3600.0).await;
            assert_eq!(
                notify_waiting_crash_parks(&pool)
                    .await
                    .expect("the recovery tick's pass"),
                0,
                "a switched-off notice writes nothing"
            );
            assert_eq!(notice_state(&pool, &run_id).await, (0, false));
        }

        /// Moves a run's thread to another origin the attention list does not cover (`system`, `support`, ...).
        async fn put_thread_on_origin(pool: &PgPool, run_id: &str, origin: &str) {
            sqlx::query(
                "UPDATE threads SET origin = $2 WHERE id = (SELECT thread_id FROM runs WHERE id = $1)",
            )
            .bind(run_id)
            .bind(origin)
            .execute(pool)
            .await
            .expect("move the thread to its origin");
        }

        /// Makes a run a scheduled run's, as `start_scheduled_run` leaves it: its thread lives in a Space (origin `space`,
        /// with the Space context the threads table requires) and the system principal owns the thread and the run. Call
        /// it before the run's fold is planted, since the fold takes its owner from the run.
        async fn put_scheduled_run_in_space(pool: &PgPool, run_id: &str, space_ref: &str) {
            sqlx::query(
                "UPDATE threads
                    SET origin = 'space', space_id = $2, user_id = 'service:orchestrator-core',
                        space_decision_ref = 'decision-1', recipient_audience_ref = 'audience-1',
                        privacy_policy_ref = 'policy-1', resource_authorization_ref = 'resource-1',
                        authority_revision = 1, recipient_audience_revision = 1,
                        recipient_audience_hash = 'audience-hash-1'
                  WHERE id = (SELECT thread_id FROM runs WHERE id = $1)",
            )
            .bind(run_id)
            .bind(space_ref)
            .execute(pool)
            .await
            .expect("move the thread into a Space");
            sqlx::query("UPDATE runs SET user_id = 'service:orchestrator-core' WHERE id = $1")
                .bind(run_id)
                .execute(pool)
                .await
                .expect("the system principal owns the run");
        }

        /// Starts a newer run in the thread of `run_id`, so the thread's latest run is the newer one from now on.
        async fn start_newer_run(pool: &PgPool, run_id: &str) {
            sqlx::query(
                "INSERT INTO runs (id, thread_id, goal, org_id, user_id, retention_class, updated_at)
                 SELECT $2, thread_id, 'g', org_id, user_id, 'persistent', now() + interval '1 second'
                   FROM runs WHERE id = $1",
            )
            .bind(run_id)
            .bind(format!("{run_id}-newer"))
            .execute(pool)
            .await
            .expect("start a newer run");
        }

        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_crash_park_the_list_does_not_name_is_not_announced() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            // Announced: a due park on a chat thread, and one on an agent-run thread, each on its thread's latest run.
            let (chat, _, _) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &chat, 5.0 * 3600.0).await;
            let (agent_run, _, _) = seed_run(&pool, "persistent").await;
            put_thread_on_origin(&pool, &agent_run, "agent_run").await;
            plant_crash_park(&pool, &agent_run, 5.0 * 3600.0).await;
            // Not announced: a scheduled run's park, in a Space and owned by the system principal.
            let (scheduled, _, _) = seed_run(&pool, "persistent").await;
            put_scheduled_run_in_space(&pool, &scheduled, "space-1").await;
            plant_crash_park(&pool, &scheduled, 5.0 * 3600.0).await;
            // Not announced: a system thread's park.
            let (system, _, _) = seed_run(&pool, "persistent").await;
            put_thread_on_origin(&pool, &system, "system").await;
            plant_crash_park(&pool, &system, 5.0 * 3600.0).await;
            // Not announced: a park that a newer run of its thread has overtaken. The list reads the latest run only.
            let (overtaken, _, _) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &overtaken, 5.0 * 3600.0).await;
            start_newer_run(&pool, &overtaken).await;

            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass");
            assert_eq!(
                notice_state(&pool, &chat).await,
                (1, true),
                "a chat park is announced"
            );
            assert_eq!(
                notice_state(&pool, &agent_run).await,
                (1, true),
                "an agent-run park is announced"
            );
            for run in [&scheduled, &system, &overtaken] {
                assert_eq!(
                    notice_state(&pool, run).await,
                    (0, false),
                    "{run} is neither announced nor stamped"
                );
            }
        }

        /// A requested question on the run's thread, as the run's own ask leaves one, open for an hour. Its id is the
        /// run's id with a `q_` prefix, so each run has its own.
        async fn put_requested_question(pool: &PgPool, run_id: &str) -> String {
            let question = format!("q_{run_id}");
            sqlx::query(
                "INSERT INTO run_questions (id, run_id, thread_id, org_id, user_id, spec, expires_at)
                 SELECT $2, id, thread_id, org_id, user_id, '{}'::jsonb, now() + interval '1 hour'
                   FROM runs WHERE id = $1",
            )
            .bind(run_id)
            .bind(&question)
            .execute(pool)
            .await
            .expect("put a question on the run's thread");
            question
        }

        /// A proposed plan on the run's thread, as the run's planning step leaves one.
        async fn put_proposed_plan(pool: &PgPool, run_id: &str) -> String {
            let plan = format!("pln_{run_id}");
            sqlx::query(
                "INSERT INTO plans (id, thread_id, run_id, status, org_id, user_id)
                 SELECT $2, thread_id, id, 'proposed', org_id, user_id FROM runs WHERE id = $1",
            )
            .bind(run_id)
            .bind(&plan)
            .execute(pool)
            .await
            .expect("put a proposed plan on the run's thread");
            plan
        }

        /// A requested approval of `kind` on the run, as the run's gate leaves one.
        async fn put_requested_approval(pool: &PgPool, run_id: &str, kind: &str) -> String {
            let approval = format!("apr_{run_id}_{kind}");
            sqlx::query(
                "INSERT INTO approvals (id, run_id, kind, status, org_id, user_id)
                 SELECT $2, id, $3, 'requested', org_id, user_id FROM runs WHERE id = $1",
            )
            .bind(run_id)
            .bind(&approval)
            .bind(kind)
            .execute(pool)
            .await
            .expect("put a requested approval on the run");
            approval
        }

        /// A run of the same thread that ended an hour ago, so the parked run stays the thread's latest run. Returns its
        /// id.
        async fn put_ended_run_before(pool: &PgPool, run_id: &str) -> String {
            let ended = format!("{run_id}-ended");
            sqlx::query(
                "INSERT INTO runs (id, thread_id, goal, org_id, user_id, retention_class, status, updated_at)
                 SELECT $2, thread_id, 'g', org_id, user_id, 'persistent', 'completed', now() - interval '1 hour'
                   FROM runs WHERE id = $1",
            )
            .bind(run_id)
            .bind(&ended)
            .execute(pool)
            .await
            .expect("start an ended run on the thread");
            ended
        }

        /// A requested question holds a due park (O-02) until it is answered or expires. Only then is the park announced,
        /// once, and the list names it at the same moment, since both read the same rule.
        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_due_park_waits_while_a_question_is_requested() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            // Answered: the open question holds the park, and the answer releases it.
            let (run, _org, _user) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &run, 5.0 * 3600.0).await;
            let question = put_requested_question(&pool, &run).await;
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass");
            assert_eq!(
                notice_state(&pool, &run).await,
                (0, false),
                "held by the open question: neither announced nor stamped"
            );
            sqlx::query(
                "UPDATE run_questions
                    SET state = 'answered', answered_at = now(), answers = '[]'::jsonb,
                        answers_digest = repeat('a', 64)
                  WHERE id = $1",
            )
            .bind(&question)
            .execute(&pool)
            .await
            .expect("the owner answers the question");
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass after the answer");
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("a second pass");
            assert_eq!(
                notice_state(&pool, &run).await,
                (1, true),
                "one notice once the question is answered, and none after it"
            );

            // Expired: a question that runs out unanswered releases the park the same way.
            let (lapsed, _org, _user) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &lapsed, 5.0 * 3600.0).await;
            let lapsed_question = put_requested_question(&pool, &lapsed).await;
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass while the question is open");
            assert_eq!(
                notice_state(&pool, &lapsed).await,
                (0, false),
                "held while the question is open"
            );
            sqlx::query("UPDATE run_questions SET state = 'expired' WHERE id = $1")
                .bind(&lapsed_question)
                .execute(&pool)
                .await
                .expect("the question expires");
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass after the expiry");
            assert_eq!(
                notice_state(&pool, &lapsed).await,
                (1, true),
                "one notice once the question has expired"
            );
        }

        /// A proposed plan holds a due park (O-02) until the plan is no longer proposed, and then one notice follows.
        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_due_park_waits_while_a_plan_is_proposed() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let (run, _org, _user) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &run, 5.0 * 3600.0).await;
            let plan = put_proposed_plan(&pool, &run).await;
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass");
            assert_eq!(
                notice_state(&pool, &run).await,
                (0, false),
                "held by the proposed plan: neither announced nor stamped"
            );
            sqlx::query("UPDATE plans SET status = 'superseded' WHERE id = $1")
                .bind(&plan)
                .execute(&pool)
                .await
                .expect("the plan is superseded");
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass after the plan");
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("a second pass");
            assert_eq!(
                notice_state(&pool, &run).await,
                (1, true),
                "one notice once the plan is no longer proposed"
            );
        }

        /// A live approval holds a due park (O-02) until it is decided. An approval that waits on a run which has ended
        /// holds nothing, because the attention list reads it as moot, and the notice must agree with the list.
        #[tokio::test]
        #[ignore = "requires DATABASE_URL to a scratch Postgres"]
        async fn a_due_park_waits_while_a_live_approval_waits_but_an_ended_runs_does_not_hold_it() {
            let Some(pool) = scratch_pool().await else {
                return;
            };
            let (live, _org, _user) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &live, 5.0 * 3600.0).await;
            let approval = put_requested_approval(&pool, &live, "tool_call").await;
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass");
            assert_eq!(
                notice_state(&pool, &live).await,
                (0, false),
                "held by the live approval: neither announced nor stamped"
            );
            sqlx::query(
                "UPDATE approvals SET status = 'approved', decided_at = now() WHERE id = $1",
            )
            .bind(&approval)
            .execute(&pool)
            .await
            .expect("the owner decides the approval");
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass after the decision");
            assert_eq!(
                notice_state(&pool, &live).await,
                (1, true),
                "one notice once the approval is decided"
            );

            // An approval on a run that has ended holds nothing.
            let (parked, _org, _user) = seed_run(&pool, "persistent").await;
            plant_crash_park(&pool, &parked, 5.0 * 3600.0).await;
            let ended = put_ended_run_before(&pool, &parked).await;
            put_requested_approval(&pool, &ended, "tool_call").await;
            notify_due_crash_parks(&pool, NOTICE_SECONDS, TTL_SECONDS, Utc::now())
                .await
                .expect("the notice pass with an ended run's approval");
            assert_eq!(
                notice_state(&pool, &parked).await,
                (1, true),
                "an approval on an ended run does not hold the park"
            );
        }
    }
}

/// The three fold RPCs follow `GetRun` and `ListToolActions` on a run whose owner was removed
/// from its Space. Every case is served through the same handler functions the RPCs call, with
/// the switch and the key passed in, so no process environment is set.
#[cfg(test)]
mod revocation_tests {
    use super::test_support::{
        cipher, fold_for, fold_state, put_run_in_space, rejoin_owner_to_space,
        remove_owner_from_space, scratch_pool, seed_run,
    };
    use super::*;
    use crate::auth::VerifiedIdentity;
    use crate::grpc::{
        claim_loop_checkpoint_inner, close_loop_checkpoint_inner, save_loop_checkpoint_inner,
    };
    use mp_contracts::model_plane::v1 as pb;
    use tonic::Response;

    /// The refusal `GetRun` and `ListToolActions` give a run whose owner was removed.
    const REFUSAL: &str = "Space membership required for this run";

    fn key() -> Result<DescriptorCipher, Status> {
        Ok(cipher())
    }

    async fn save_rpc(
        pool: &PgPool,
        caller: &VerifiedIdentity,
        run_id: &str,
        expected_seq: u64,
        kind: &str,
        pause_ref: &str,
    ) -> Result<u64, Status> {
        save_loop_checkpoint_inner(
            pool,
            caller,
            pb::SaveLoopCheckpointRequest {
                run_id: run_id.to_owned(),
                expected_seq,
                kind: kind.to_owned(),
                fold: fold_for(run_id),
                authority_digest: "auth-1".to_owned(),
                pause_ref: pause_ref.to_owned(),
                ..Default::default()
            },
            true,
            key,
        )
        .await
        .map(|response| response.into_inner().seq)
    }

    async fn claim_rpc(
        pool: &PgPool,
        caller: &VerifiedIdentity,
        run_id: &str,
        pause_ref: &str,
    ) -> Result<pb::ClaimLoopCheckpointResponse, Status> {
        claim_loop_checkpoint_inner(
            pool,
            caller,
            pb::ClaimLoopCheckpointRequest {
                run_id: run_id.to_owned(),
                pause_ref: pause_ref.to_owned(),
                claim_token: "token-1".to_owned(),
            },
            true,
            key,
        )
        .await
        .map(Response::into_inner)
    }

    async fn close_rpc(
        pool: &PgPool,
        caller: &VerifiedIdentity,
        run_id: &str,
        seq: u64,
    ) -> Result<(), Status> {
        close_loop_checkpoint_inner(
            pool,
            caller,
            pb::CloseLoopCheckpointRequest {
                run_id: run_id.to_owned(),
                seq,
                reason: "loop_finished".to_owned(),
            },
        )
        .await
        .map(|_| ())
    }

    fn assert_refused(refused: &Status, who: &str, rpc: &str) {
        assert_eq!(refused.code(), tonic::Code::PermissionDenied, "{who} {rpc}");
        assert_eq!(refused.message(), REFUSAL, "{who} {rpc}");
    }

    async fn stored_seq(pool: &PgPool, run_id: &str) -> i64 {
        sqlx::query_scalar("SELECT seq FROM run_loop_checkpoints WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await
            .expect("read the fold seq")
    }

    async fn restored_events(pool: &PgPool, run_id: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE run_id = $1 AND event_type = 'CHECKPOINT_RESTORED'",
        )
        .bind(run_id)
        .fetch_one(pool)
        .await
        .expect("count restore events")
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_member_removed_from_the_runs_space_cannot_save_claim_or_close_a_fold() {
        let Some(pool) = scratch_pool().await else { return };
        let (run_id, org, _user) = seed_run(&pool, "persistent").await;
        let space_ref = format!("lc-space-{}", mp_ids::new_ulid());
        put_run_in_space(&pool, &run_id, &space_ref).await;
        let owner = VerifiedIdentity::user_for_test(&org, "u1");
        let service = VerifiedIdentity::service_for_test(&org, &["session:write"], false);

        // While the owner is a member the run's fold is theirs to write: the control case, so
        // the refusals below are about the removal and nothing else about the run.
        assert_eq!(
            save_rpc(&pool, &owner, &run_id, 0, "pause", "appr_1").await.expect("a member saves"),
            1
        );
        assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("paused"));

        remove_owner_from_space(&pool, &run_id, &space_ref).await;

        // The owner and a service in the org are both refused, with the refusal GetRun gives,
        // on each of the three RPCs.
        for (who, caller) in [("owner", &owner), ("service", &service)] {
            let refused = save_rpc(&pool, caller, &run_id, 1, "round", "")
                .await
                .expect_err("a removed member's run takes no save");
            assert_refused(&refused, who, "save");
            let refused = claim_rpc(&pool, caller, &run_id, "appr_1")
                .await
                .expect_err("a removed member's run cannot be claimed");
            assert_refused(&refused, who, "claim");
            let refused = close_rpc(&pool, caller, &run_id, 1)
                .await
                .expect_err("a removed member's run cannot be closed");
            assert_refused(&refused, who, "close");
        }
        // Nothing moved: the fold is the paused seq 1 it was, unclaimed, and no restore was recorded.
        assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("paused"));
        assert_eq!(stored_seq(&pool, &run_id).await, 1);
        assert_eq!(restored_events(&pool, &run_id).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_rejoin_restores_the_save_claim_and_close_of_a_fold() {
        let Some(pool) = scratch_pool().await else { return };
        let (run_id, org, _user) = seed_run(&pool, "persistent").await;
        let space_ref = format!("lc-space-{}", mp_ids::new_ulid());
        put_run_in_space(&pool, &run_id, &space_ref).await;
        let owner = VerifiedIdentity::user_for_test(&org, "u1");
        let service = VerifiedIdentity::service_for_test(&org, &["session:write"], false);
        assert_eq!(
            save_rpc(&pool, &owner, &run_id, 0, "pause", "appr_1").await.expect("save"),
            1
        );
        remove_owner_from_space(&pool, &run_id, &space_ref).await;
        assert_refused(
            &claim_rpc(&pool, &service, &run_id, "appr_1").await.expect_err("removed"),
            "service",
            "claim",
        );

        rejoin_owner_to_space(&pool, &run_id, &space_ref).await;

        // The service claims the pause, the loop saves its next round under the claimed seq and
        // the owner closes the fold: each RPC is open again, and the fold the removal left in
        // place is the one the claim returns.
        let claimed = claim_rpc(&pool, &service, &run_id, "appr_1")
            .await
            .expect("a rejoined member's fold is claimable");
        assert_eq!(claimed.seq, 1);
        assert_eq!(claimed.kind, "pause");
        assert_eq!(
            save_rpc(&pool, &service, &run_id, 1, "round", "").await.expect("save after rejoin"),
            2
        );
        close_rpc(&pool, &owner, &run_id, 2).await.expect("close after rejoin");
        assert_eq!(fold_state(&pool, &run_id).await.as_deref(), Some("closed"));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a scratch Postgres"]
    async fn a_run_outside_any_space_is_unaffected_by_a_revocation_row_for_its_owner() {
        let Some(pool) = scratch_pool().await else { return };
        let (run_id, org, _user) = seed_run(&pool, "persistent").await;
        let owner = VerifiedIdentity::user_for_test(&org, "u1");
        // A revocation names a Space; a run with no Space never matches it, even for the same user.
        let other_space = format!("lc-space-{}", mp_ids::new_ulid());
        remove_owner_from_space(&pool, &run_id, &other_space).await;

        assert_eq!(
            save_rpc(&pool, &owner, &run_id, 0, "pause", "appr_1").await.expect("unscoped save"),
            1
        );
        assert_eq!(claim_rpc(&pool, &owner, &run_id, "appr_1").await.expect("unscoped claim").seq, 1);
        close_rpc(&pool, &owner, &run_id, 1).await.expect("unscoped close");
    }
}

/// Input-pause fold tests (build plan WS14 S5 b3).
#[cfg(test)]
#[path = "loop_checkpoint_input_pause_tests.rs"]
mod input_pause_tests;
