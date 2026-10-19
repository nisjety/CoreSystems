//! The park write of the durable loop (build plan WS03 S8-A, step S8-5).
//!
//! A pinned, root Do loop that meets a typed, content-free, recoverable stop at a round
//! boundary used to end in a failed run and a terminal receipt. With parking on it saves a
//! fold of kind `park` through its fenced writer, returns `RunAgentResponse.status`
//! `parked` and leaves the run open: no assistant message, no receipt, no ledger
//! finalization and no lease release. session-core's recovery worker spares the run until
//! its park TTL; a resume (S8-6) continues it from the fold.
//!
//! **Parking upgrades a failure; it is never a new way to fail.** Unless a park-aware store
//! durably accepts the park and says so (`SaveLoopCheckpointResponse.park_accepted`), the run
//! takes exactly the outcome it had before this module existed, and a park that may have
//! been stored without being accepted is closed best effort so no stray resumable park
//! outlives the fallback.
//!
//! # What parks
//!
//! Only a fresh root request (or, from S8-6, a park resume) of a run that is persistent (not
//! zero-retention, not waived), has no Space, was offered a tool set a resume can derive
//! again (no client or MCP tools), holds no approval release, has an active fold writer and
//! is pinned to the subscription route with a connection and a model: [`run_parkable`], read
//! once when the loop starts, and [`parkable`] at each stop. The stops are the three the loop
//! already has ([`ParkStop`]): a run-wide refusal latched by any loop (site A), the credential
//! deadline (site B) and a failed inference round of the pinned route (site C). The closed
//! table is [`park_reason_for`]; everything else keeps the committed return.
//!
//! # Content rules
//!
//! The park fold is conversation content, written through the same gate and scrub as every
//! fold (never for a zero-retention, waived, Space or delegated run). The four extension
//! keys a park adds are identifiers and counts, never a credential. Logs and events are
//! content-free: ids, counts, closed tokens and timestamps. The park reason reaches only
//! the response and the store, never an event.

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use mp_contracts::loop_fold::{
    self as lf, LoopFoldV1, ParkReason, PauseKind, PendingPause, MAX_PARKS_PER_RUN,
};
use mp_contracts::model_plane::v1 as pb;
use tonic::transport::Channel;
use tracing::{info, warn};

use super::fold::{self, FoldWriter, ParkSaveError, SaveError, Snapshot};
use crate::run_budget::DeadlineCause;
use crate::subscription_refusal::{
    Refusal, RATE_LIMITED, SUBSCRIPTION_REAUTH_REQUIRED, SUBSCRIPTION_UPSTREAM_UNAVAILABLE,
    SUBSCRIPTION_USAGE_LIMIT,
};

/// `RunAgentResponse.status` of a run that parked.
pub(crate) const PARKED_STATUS: &str = "parked";

/// `AgentResume.mode` of a park resume (S8-6 builds the resume itself).
#[cfg_attr(not(test), allow(dead_code, reason = "read by the park resume (WS03 S8-6)"))]
pub(crate) const PARK_MODE: &str = "park";

/// The longest wait a provider's own hint may ask for, in milliseconds. session-core refuses
/// a park due beyond half its TTL (default 12 hours, at most 15 days), so a hint past this
/// is refused there and the run takes its committed outcome; the clamp only keeps the
/// arithmetic from overflowing on a nonsense number.
const MAX_HINT_MS: u64 = 30 * 24 * 60 * 60 * 1000;

// --- the switches ---------------------------------------------------------------------

#[cfg(test)]
thread_local! {
    /// A test's override of the parking switch. `#[tokio::test]` runs its future on the
    /// test's own thread, so a thread-local cannot leak into a parallel test the way a
    /// process-wide environment variable would.
    static TEST_PARKING: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// A test's override of [`PARK_RESUME_BUILT`], beside the switch's own.
    static TEST_RESUME_BUILT: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Holds the parking switch at a fixed value for the current test thread, then restores it.
/// A test that holds it exercises the park WRITER, so it also treats the resume half as built
/// ([`PARK_RESUME_BUILT`]); [`ResumeBuilt`] says otherwise for the one test of that gate.
#[cfg(test)]
pub(crate) struct ParkSwitch;

#[cfg(test)]
impl ParkSwitch {
    pub(crate) fn set(on: bool) -> Self {
        TEST_PARKING.with(|cell| cell.set(Some(on)));
        TEST_RESUME_BUILT.with(|cell| cell.set(Some(true)));
        Self
    }
}

#[cfg(test)]
impl Drop for ParkSwitch {
    fn drop(&mut self) {
        TEST_PARKING.with(|cell| cell.set(None));
        TEST_RESUME_BUILT.with(|cell| cell.set(None));
    }
}

/// Holds [`PARK_RESUME_BUILT`] at a fixed value for the current test thread, then puts back
/// what it found.
#[cfg(test)]
pub(crate) struct ResumeBuilt(Option<bool>);

#[cfg(test)]
impl ResumeBuilt {
    pub(crate) fn set(built: bool) -> Self {
        Self(TEST_RESUME_BUILT.with(|cell| cell.replace(Some(built))))
    }
}

#[cfg(test)]
impl Drop for ResumeBuilt {
    fn drop(&mut self) {
        TEST_RESUME_BUILT.with(|cell| cell.set(self.0));
    }
}

/// Whether the RESUME half of the park (WS03 S8-6) is built into this binary. It is not: the
/// executor still refuses `AgentResume.mode` `park` (`enforce_resume`), so a park written by
/// this binary would be held until its 24 hour park TTL with nothing able to continue it, a
/// run waiting for nothing. With this `false`, [`parking_active`] is false whatever the
/// switches say, so an image that has the write but not the resume cannot park a run by
/// configuration alone. S8-6 flips it to `true` together with the resume half, and the guard
/// of `durable_loop_contract.rs` (`PARK_RESUME_BUILT_EXPECTED`) in the same commit.
const PARK_RESUME_BUILT: bool = false;

/// [`PARK_RESUME_BUILT`], or a test's override of it.
fn park_resume_built() -> bool {
    #[cfg(test)]
    if let Some(forced) = TEST_RESUME_BUILT.with(std::cell::Cell::get) {
        return forced;
    }
    PARK_RESUME_BUILT
}

/// Whether the parking switch is on in this process (the one strict parser,
/// `mp_contracts::loop_fold::loop_parking_enabled`). Default off. Only the WRITE of a park
/// reads it here: a park RESUME is never gated by it, so with it off existing parks stay
/// resumable and no new one is written.
fn loop_parking_enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = TEST_PARKING.with(std::cell::Cell::get) {
        return forced;
    }
    lf::loop_parking_enabled()
}

/// Whether a park is resumable in this process: the in-place resume, the continue switch,
/// the fold writer and the effect ledger are all on. A park the executor cannot resume
/// again would be a run nothing can finish, so none is written without all four.
pub(crate) fn park_resume_active() -> bool {
    super::resume::loop_resume_enabled()
        && super::resume::continue_enabled()
        && fold::checkpoint_enabled()
        && super::effect::ledger_enabled()
}

/// Whether this process writes parks: the parking switch, a resume half that is built
/// ([`PARK_RESUME_BUILT`]), and everything a park needs to be resumed ([`park_resume_active`]).
pub(crate) fn parking_active() -> bool {
    loop_parking_enabled() && park_resume_built() && park_resume_active()
}

// --- what may park ---------------------------------------------------------------------

/// What a round loop starts from, as the park decision reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeedKind {
    /// A fresh root request: no seed.
    Fresh,
    /// A stage-1 approval continuation.
    Segment,
    /// The in-place resume of an approval pause.
    InPlace,
    /// A park resume (WS03 S8-6): the loop continues from a claimed park fold.
    #[cfg_attr(not(test), allow(dead_code, reason = "built by the park resume (WS03 S8-6)"))]
    Parked,
}

/// The facts of a run that never change while it runs. Read once, when the loop starts.
// Each flag is an independent fact about the run, read once: a state machine would only
// rename them.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct StaticFacts {
    /// Nesting depth: only the root loop (0) parks.
    pub depth: u32,
    /// The run is zero-retention.
    pub zdr: bool,
    /// The run is a waived run (zero-retention org, one connection with ZDR off).
    pub waived: bool,
    /// The run belongs to a Space.
    pub space: bool,
    /// The offered tool set is one a resume derives again, by name and digest: no client or
    /// MCP tools, and the server-resolved view was readable.
    pub tools_derivable: bool,
    /// The request holds an approved release (an approval continuation).
    pub holds_release: bool,
    /// What the loop starts from.
    pub seed: SeedKind,
    /// The run is pinned to the subscription route.
    pub pinned: bool,
    /// The route names a connection (the park fold records it, and a resume rebuilds it).
    pub connection_id_present: bool,
    /// The route names a model (the same).
    pub model_present: bool,
}

/// Whether the run is one that may park at all: every fact that is true of the whole run.
/// The switch is read here, so a run that starts with parking off never parks later.
pub(crate) fn run_parkable(facts: &StaticFacts) -> bool {
    parking_active()
        && facts.depth == 0
        && !facts.zdr
        && !facts.waived
        && !facts.space
        && facts.tools_derivable
        && !facts.holds_release
        && matches!(facts.seed, SeedKind::Fresh | SeedKind::Parked)
        && facts.pinned
        && facts.connection_id_present
        && facts.model_present
}

/// The facts of one stop.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ParkFacts {
    /// [`run_parkable`], read when the loop started.
    pub run_parkable: bool,
    /// The fold writer is saving folds (not off, not failed).
    pub writer_active: bool,
    /// Rounds charged to the run: its own and its delegates', the failed round included.
    pub rounds_used: u32,
    /// The run's round budget.
    pub max_rounds: u32,
    /// Parks the run has taken before this one.
    pub parks_so_far: u32,
}

/// Whether the run may park at this stop: it may park at all, its writer can save the fold,
/// a resume would have a round left to run, and the run is under its park cap.
pub(crate) fn parkable(facts: &ParkFacts) -> bool {
    facts.run_parkable
        && facts.writer_active
        && facts.rounds_used < facts.max_rounds
        && facts.parks_so_far < MAX_PARKS_PER_RUN
}

// --- why it parks ----------------------------------------------------------------------

/// Where the loop stopped, in the words the park decision reads.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ParkStop<'a> {
    /// Site A, a loop top: a run-wide refusal some loop of the run latched.
    LatchedRefusal(&'a Refusal),
    /// Site B, a loop top: the run's time budget is spent.
    Deadline(DeadlineCause),
    /// Site C, the failed round of a pinned run (the refusal the run reports for it). The
    /// failed round stays charged.
    FailedRound(&'a Refusal),
}

/// The closed decision table: why a stop parks, or `None` and the committed return.
///
/// * a latched usage limit is [`ParkReason::ProviderExhausted`], an expired sign-in
///   [`ParkReason::ReauthRequired`]; any other latched code does not park;
/// * the credential deadline is [`ParkReason::CredentialWindow`]; the operator's wall clock
///   never parks (a longer wait would not change it);
/// * a failed round parks for a usage limit, a throttle that says how long to wait, a
///   transient upstream failure and an expired sign-in; a throttle with no wait, a rejected
///   request, an overlong prompt and every untyped failure keep the committed return.
///
/// [`ParkReason::CrashInterrupted`] is never produced here: only a crash is, and the
/// executor never writes one.
pub(crate) fn park_reason_for(stop: &ParkStop<'_>) -> Option<ParkReason> {
    match stop {
        ParkStop::LatchedRefusal(refusal) => match refusal.code {
            SUBSCRIPTION_USAGE_LIMIT => Some(ParkReason::ProviderExhausted),
            SUBSCRIPTION_REAUTH_REQUIRED => Some(ParkReason::ReauthRequired),
            _ => None,
        },
        ParkStop::Deadline(DeadlineCause::Credential) => Some(ParkReason::CredentialWindow),
        ParkStop::Deadline(DeadlineCause::WallClock) => None,
        ParkStop::FailedRound(refusal) => match refusal.code {
            SUBSCRIPTION_USAGE_LIMIT => Some(ParkReason::ProviderExhausted),
            RATE_LIMITED => refusal.retry_after_ms.map(|_| ParkReason::ProviderExhausted),
            SUBSCRIPTION_UPSTREAM_UNAVAILABLE => Some(ParkReason::UpstreamUnavailable),
            SUBSCRIPTION_REAUTH_REQUIRED => Some(ParkReason::ReauthRequired),
            _ => None,
        },
    }
}

/// `now` plus a provider's wait hint, or `None` when it is absent.
fn after_hint(now: DateTime<Utc>, retry_after_ms: Option<u64>) -> Option<DateTime<Utc>> {
    let ms = i64::try_from(retry_after_ms?.min(MAX_HINT_MS)).ok()?;
    now.checked_add_signed(TimeDelta::try_milliseconds(ms)?)
}

/// `now` plus `secs`.
fn after_secs(now: DateTime<Utc>, secs: i64) -> DateTime<Utc> {
    TimeDelta::try_seconds(secs)
        .and_then(|delta| now.checked_add_signed(delta))
        .unwrap_or(now)
}

/// When a resume of the park is first due.
///
/// * [`ParkReason::ProviderExhausted`]: the provider's reset time, else `now` plus its retry
///   hint, else `now` plus [`lf::PARK_DEFAULT_WAIT_SECS`];
/// * [`ParkReason::UpstreamUnavailable`]: `now` plus the hint, else `now` plus
///   [`lf::PARK_SHORT_WAIT_SECS`];
/// * the credential window and an expired sign-in: `now`, because the resume carries fresh
///   credentials and there is nothing to wait for.
///
/// A reset time in the past is due at once. A wait past half the store's TTL is refused by
/// the store, which is the committed outcome.
pub(crate) fn park_until_for(
    reason: ParkReason,
    retry_after_ms: Option<u64>,
    resets_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    match reason {
        ParkReason::ProviderExhausted => resets_at
            .or_else(|| after_hint(now, retry_after_ms))
            .unwrap_or_else(|| after_secs(now, lf::PARK_DEFAULT_WAIT_SECS)),
        ParkReason::UpstreamUnavailable => after_hint(now, retry_after_ms)
            .unwrap_or_else(|| after_secs(now, lf::PARK_SHORT_WAIT_SECS)),
        ParkReason::CredentialWindow | ParkReason::ReauthRequired | ParkReason::CrashInterrupted => {
            now
        }
    }
}

// --- the park itself -------------------------------------------------------------------

/// What `try_park` reads of the run it parks: identifiers and the session-core channel.
/// `LoopContext` is private to the loop, so the loop hands over exactly this.
pub(crate) struct ParkSite<'a> {
    pub run_id: &'a str,
    pub session_channel: &'a Channel,
    pub session_bearer: Option<&'a str>,
    /// The model every round of the run asks for (the route's, never a default).
    pub route_model: &'a str,
    /// The connection the run is pinned through: an identifier, never its secret.
    pub route_connection_id: &'a str,
    /// The run's resolved sub-agent ceiling.
    pub child_ceiling: u32,
}

/// A park the store accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParkedRun {
    pub reason: ParkReason,
    pub park_ref: String,
    pub seq: u64,
    pub park_until: DateTime<Utc>,
}

/// The park fold: the round-boundary fold of the loop's locals, naming its park, with the
/// four keys a resume needs to rebuild the run (the count of parks this one included, the
/// route's model and connection id, and the sub-agent ceiling). They are written here and
/// nowhere else, and only on a park fold.
pub(crate) fn park_fold(snapshot: &Snapshot<'_>, site: &ParkSite<'_>, n: u32) -> LoopFoldV1 {
    let mut fold = fold::build_fold(
        snapshot,
        None,
        Some(PendingPause {
            kind: PauseKind::Park,
            pause_ref: lf::park_ref(site.run_id, n),
            step_id: String::new(),
            call: String::new(),
            action_fingerprint: String::new(),
        }),
    );
    fold.extensions
        .insert(lf::EXT_PARKS.to_owned(), serde_json::Value::from(n));
    fold.extensions.insert(
        lf::EXT_ROUTE_MODEL.to_owned(),
        serde_json::Value::from(site.route_model),
    );
    fold.extensions.insert(
        lf::EXT_ROUTE_CONNECTION.to_owned(),
        serde_json::Value::from(site.route_connection_id),
    );
    fold.extensions.insert(
        lf::EXT_CHILD_CEILING.to_owned(),
        serde_json::Value::from(site.child_ceiling),
    );
    fold
}

/// Park the run at this stop, or return `None` and let the caller take its committed return.
///
/// `None` unless [`parkable`] and the stop has a reason. It saves the park fold through the
/// run's one fenced writer ([`FoldWriter::save_park`]: one RPC, no probe, no tombstone) and
/// succeeds only when the store says it durably accepted it. Every other outcome is `None`:
///
/// * the store stored the fold without accepting it as a park: the row is closed best effort
///   (`park_not_taken`) so a stray resumable park never outlives the fallback;
/// * the save may have committed (a timeout or a transport failure): the row it would sit at,
///   `expected + 1`, is closed best effort the same way;
/// * any other failure committed nothing.
///
/// This is the only caller of [`FoldWriter::save_park`].
pub(crate) async fn try_park(
    site: &ParkSite<'_>,
    folds: &mut FoldWriter<'_>,
    snapshot: &Snapshot<'_>,
    facts: &ParkFacts,
    stop: &ParkStop<'_>,
) -> Option<ParkedRun> {
    if !parkable(facts) {
        return None;
    }
    let reason = park_reason_for(stop)?;
    let (retry_after_ms, resets_at) = match stop {
        ParkStop::LatchedRefusal(refusal) | ParkStop::FailedRound(refusal) => {
            (refusal.retry_after_ms, refusal.resets_at)
        }
        ParkStop::Deadline(_) => (None, None),
    };
    let park_until = park_until_for(reason, retry_after_ms, resets_at, Utc::now());
    let n = facts.parks_so_far.saturating_add(1);
    let park_ref = lf::park_ref(site.run_id, n);
    let fold = park_fold(snapshot, site, n);
    let expected = folds.expected_seq();
    match folds.save_park(fold, reason, park_until).await {
        Ok(seq) => {
            info!(
                run_id = %site.run_id,
                park_reason = reason.as_str(),
                park_ref = %park_ref,
                seq,
                parks = n,
                park_until = %park_until.to_rfc3339_opts(SecondsFormat::Secs, true),
                "run_agent: the run is parked at a round boundary"
            );
            publish_run_parked(site.session_channel, site.session_bearer, site.run_id).await;
            Some(ParkedRun {
                reason,
                park_ref,
                seq,
                park_until,
            })
        }
        Err(ParkSaveError::NotAccepted { seq }) => {
            warn!(
                run_id = %site.run_id,
                park_reason = reason.as_str(),
                seq,
                "run_agent: the store did not accept the park; closing its row and taking the \
                 committed outcome"
            );
            close_stray_park(site, seq).await;
            None
        }
        Err(ParkSaveError::Save(SaveError::Unavailable { ambiguous: true })) => {
            warn!(
                run_id = %site.run_id,
                park_reason = reason.as_str(),
                "run_agent: the park save may have committed; closing its row and taking the \
                 committed outcome"
            );
            close_stray_park(site, expected.saturating_add(1)).await;
            None
        }
        Err(ParkSaveError::Save(error)) => {
            info!(
                run_id = %site.run_id,
                park_reason = reason.as_str(),
                outcome = error.as_str(),
                cause = error.cause(),
                "run_agent: the run could not be parked; taking the committed outcome"
            );
            None
        }
    }
}

/// Close the row a park that was not taken left (or may have left) behind. Best effort, like
/// every close: a failure is logged with a closed token by `close_rpc` and changes nothing.
async fn close_stray_park(site: &ParkSite<'_>, seq: u64) {
    let Some(bearer) = site.session_bearer else {
        return;
    };
    fold::close_rpc(
        site.session_channel,
        bearer,
        site.run_id,
        seq,
        "park_not_taken",
        fold::GATED_SAVE_TIMEOUT,
    )
    .await;
}

/// The response of a run that parked: `parked`, the reason, and when a resume is first due.
/// No answer, no failure code and no retry hint: the run is not finished and not failed.
pub(crate) fn parked_response(
    parked: &ParkedRun,
    rounds_executed: u32,
    grounded: bool,
    compaction_triggered: bool,
) -> pb::RunAgentResponse {
    pb::RunAgentResponse {
        status: PARKED_STATUS.to_owned(),
        final_output: String::new(),
        rounds_executed,
        grounded,
        compaction_triggered,
        failure_code: String::new(),
        retry_after_ms: None,
        resets_at: parked
            .park_until
            .to_rfc3339_opts(SecondsFormat::Secs, true),
        park_reason: parked.reason.as_str().to_owned(),
    }
}

/// Publish `RunParked` on session-core's orchestration broadcast (the same
/// `RecordOrchestrationEvent` path the plan transition uses). Best effort: a failure is
/// logged and changes nothing. Ids only: the park reason is never broadcast.
async fn publish_run_parked(channel: &Channel, bearer: Option<&str>, run_id: &str) {
    let request = pb::RecordOrchestrationEventRequest {
        event: Some(pb::OrchestrationEvent {
            // session-core assigns event_id and at.
            event_id: String::new(),
            at: None,
            event: Some(pb::orchestration_event::Event::RunParked(
                pb::orchestration_event::RunParked {
                    run_id: run_id.to_owned(),
                },
            )),
        }),
    };
    let result = match authenticated(request, bearer) {
        Some(request) => {
            pb::orchestration_core_service_client::OrchestrationCoreServiceClient::new(
                channel.clone(),
            )
            .record_orchestration_event(request)
            .await
            .map(|_| ())
        }
        None => Err(tonic::Status::unauthenticated(
            "verified session credential required",
        )),
    };
    if let Err(status) = result {
        warn!(
            run_id = %run_id,
            grpc_code = ?status.code(),
            "run_agent: failed to publish RunParked (best-effort)"
        );
    }
}

/// `value` as a request under the run owner's bearer, or `None` when there is none to send.
fn authenticated<T>(value: T, bearer: Option<&str>) -> Option<tonic::Request<T>> {
    let header = format!("Bearer {}", bearer?).parse().ok()?;
    let mut request = tonic::Request::new(value);
    request.metadata_mut().insert("authorization", header);
    Some(request)
}

/// Whether `req` is a park resume (WS03 S8-6): its resume names the `park` mode. A park
/// resume has no approval to consume and no release to hold.
#[cfg_attr(not(test), allow(dead_code, reason = "read by the park resume (WS03 S8-6)"))]
pub(crate) fn is_park_resume(req: &pb::RunAgentRequest) -> bool {
    req.resume
        .as_ref()
        .is_some_and(|resume| resume.mode == PARK_MODE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_budget::DeadlineCause;
    use crate::subscription_refusal::{
        PROMPT_TOO_LONG, SUBSCRIPTION_BAD_REQUEST, SUBSCRIPTION_ROUTE_UNAVAILABLE,
    };
    use std::collections::BTreeSet;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("a timestamp")
            .with_timezone(&Utc)
    }

    fn refusal(code: &'static str) -> Refusal {
        Refusal::bare(code)
    }

    /// Every code a refusal can carry, so a table that forgets one is caught.
    const ALL_CODES: [&str; 7] = [
        SUBSCRIPTION_USAGE_LIMIT,
        SUBSCRIPTION_REAUTH_REQUIRED,
        SUBSCRIPTION_UPSTREAM_UNAVAILABLE,
        SUBSCRIPTION_BAD_REQUEST,
        RATE_LIMITED,
        SUBSCRIPTION_ROUTE_UNAVAILABLE,
        PROMPT_TOO_LONG,
    ];

    /// The decision table, whole: each stop against each code. Anything the table does not
    /// name keeps the committed return, and a crash is never a reason the executor writes.
    #[test]
    fn the_decision_table_is_closed() {
        use ParkReason::{CredentialWindow, ProviderExhausted, ReauthRequired, UpstreamUnavailable};
        // Site A: only the two run-wide refusals, which are the ones that latch.
        for code in ALL_CODES {
            let expected = match code {
                SUBSCRIPTION_USAGE_LIMIT => Some(ProviderExhausted),
                SUBSCRIPTION_REAUTH_REQUIRED => Some(ReauthRequired),
                _ => None,
            };
            let latched = refusal(code);
            assert_eq!(
                park_reason_for(&ParkStop::LatchedRefusal(&latched)),
                expected,
                "latched {code}"
            );
            assert_eq!(
                latched.latches(),
                expected.is_some(),
                "site A parks exactly what latches: {code}"
            );
        }
        // Site B: the credential deadline parks, the operator's wall clock never.
        assert_eq!(
            park_reason_for(&ParkStop::Deadline(DeadlineCause::Credential)),
            Some(CredentialWindow)
        );
        assert_eq!(park_reason_for(&ParkStop::Deadline(DeadlineCause::WallClock)), None);
        // Site C.
        for code in ALL_CODES {
            let expected = match code {
                SUBSCRIPTION_USAGE_LIMIT => Some(ProviderExhausted),
                SUBSCRIPTION_UPSTREAM_UNAVAILABLE => Some(UpstreamUnavailable),
                SUBSCRIPTION_REAUTH_REQUIRED => Some(ReauthRequired),
                // A throttle parks only when the provider said how long to wait (checked
                // below), and nothing else parks.
                _ => None,
            };
            assert_eq!(
                park_reason_for(&ParkStop::FailedRound(&refusal(code))),
                expected,
                "failed round {code}"
            );
        }
        let throttle_with_wait = Refusal {
            retry_after_ms: Some(30_000),
            ..refusal(RATE_LIMITED)
        };
        assert_eq!(
            park_reason_for(&ParkStop::FailedRound(&throttle_with_wait)),
            Some(ProviderExhausted),
            "a throttle that names its wait parks"
        );
        let throttle_with_zero = Refusal {
            retry_after_ms: Some(0),
            ..refusal(RATE_LIMITED)
        };
        assert_eq!(
            park_reason_for(&ParkStop::FailedRound(&throttle_with_zero)),
            Some(ProviderExhausted),
            "a stated zero is a stated wait (an unset hint is not zero)"
        );
        // The four reasons of the table, and never the crash.
        let produced: BTreeSet<&str> = [
            park_reason_for(&ParkStop::Deadline(DeadlineCause::Credential)),
            park_reason_for(&ParkStop::FailedRound(&refusal(SUBSCRIPTION_USAGE_LIMIT))),
            park_reason_for(&ParkStop::FailedRound(&refusal(SUBSCRIPTION_UPSTREAM_UNAVAILABLE))),
            park_reason_for(&ParkStop::FailedRound(&refusal(SUBSCRIPTION_REAUTH_REQUIRED))),
        ]
        .into_iter()
        .flatten()
        .map(ParkReason::as_str)
        .collect();
        assert_eq!(
            produced,
            BTreeSet::from([
                "credential_window",
                "provider_exhausted",
                "upstream_unavailable",
                "reauth_required"
            ])
        );
    }

    #[test]
    fn park_until_defaults() {
        let now = at("2026-10-19T12:00:00Z");
        // A provider's own reset wins, in the past too.
        let reset = at("2026-10-20T01:21:00Z");
        assert_eq!(
            park_until_for(ParkReason::ProviderExhausted, Some(5_000), Some(reset), now),
            reset
        );
        let past = at("2026-10-19T11:00:00Z");
        assert_eq!(
            park_until_for(ParkReason::ProviderExhausted, None, Some(past), now),
            past,
            "a reset in the past is due at once, and the store decides what that means"
        );
        // Then its retry hint, then the default wait.
        assert_eq!(
            park_until_for(ParkReason::ProviderExhausted, Some(90_500), None, now),
            now + TimeDelta::milliseconds(90_500)
        );
        assert_eq!(
            park_until_for(ParkReason::ProviderExhausted, None, None, now),
            now + TimeDelta::seconds(lf::PARK_DEFAULT_WAIT_SECS)
        );
        assert_eq!(lf::PARK_DEFAULT_WAIT_SECS, 300);
        // An upstream failure waits for its hint, else the short default; it has no reset.
        assert_eq!(
            park_until_for(ParkReason::UpstreamUnavailable, Some(2_000), Some(reset), now),
            now + TimeDelta::seconds(2)
        );
        assert_eq!(
            park_until_for(ParkReason::UpstreamUnavailable, None, Some(reset), now),
            now + TimeDelta::seconds(lf::PARK_SHORT_WAIT_SECS)
        );
        assert_eq!(lf::PARK_SHORT_WAIT_SECS, 60);
        // The credential window and an expired sign-in are due now, whatever they carry.
        for reason in [
            ParkReason::CredentialWindow,
            ParkReason::ReauthRequired,
            ParkReason::CrashInterrupted,
        ] {
            assert_eq!(
                park_until_for(reason, Some(5_000), Some(reset), now),
                now,
                "{reason:?}"
            );
        }
        // A nonsense hint cannot overflow the arithmetic: it is clamped, and the store refuses
        // a wait that long.
        let huge = park_until_for(ParkReason::ProviderExhausted, Some(u64::MAX), None, now);
        let clamp = i64::try_from(MAX_HINT_MS).expect("the clamp fits");
        assert_eq!(huge, now + TimeDelta::milliseconds(clamp));
    }

    #[test]
    fn parking_active_needs_the_resume_switches() {
        use crate::runtime_loop::effect::LedgerSwitch;
        use crate::runtime_loop::fold::CheckpointSwitch;
        use crate::runtime_loop::resume::{ContinueSwitch, LoopResumeSwitch};

        // Every switch on: the one combination that parks.
        {
            let _park = ParkSwitch::set(true);
            let _resume = LoopResumeSwitch::set(true);
            let _continue = ContinueSwitch::set(true);
            let _checkpoint = CheckpointSwitch::set(true);
            let _ledger = LedgerSwitch::set(true);
            assert!(park_resume_active());
            assert!(parking_active());
        }
        // Each one off, alone, turns parking off. The parking switch off leaves the resume
        // half alone: parks stay resumable and no new one is written.
        for off in 0..5 {
            let _park = ParkSwitch::set(off != 0);
            let _resume = LoopResumeSwitch::set(off != 1);
            let _continue = ContinueSwitch::set(off != 2);
            let _checkpoint = CheckpointSwitch::set(off != 3);
            let _ledger = LedgerSwitch::set(off != 4);
            assert!(!parking_active(), "switch {off} off");
            assert_eq!(
                park_resume_active(),
                off == 0,
                "the parking switch is not part of what a park needs to be resumed (switch {off})"
            );
        }
    }

    /// A binary that has the park write but not the park resume never parks, whatever its
    /// switches say: a park nothing can resume is a run held for the whole park TTL.
    #[test]
    fn parking_needs_the_resume_half_to_be_built() {
        use crate::runtime_loop::effect::LedgerSwitch;
        use crate::runtime_loop::fold::CheckpointSwitch;
        use crate::runtime_loop::resume::{ContinueSwitch, LoopResumeSwitch};

        let _park = ParkSwitch::set(true);
        let _resume = LoopResumeSwitch::set(true);
        let _continue = ContinueSwitch::set(true);
        let _checkpoint = CheckpointSwitch::set(true);
        let _ledger = LedgerSwitch::set(true);
        assert!(parking_active(), "the writer tests hold the resume half as built");
        {
            let _unbuilt = ResumeBuilt::set(false);
            assert!(!park_resume_built());
            assert!(!parking_active(), "every switch on and no resume half: no park");
            // The decision of a whole run reads the same gate.
            let run = StaticFacts {
                depth: 0,
                zdr: false,
                waived: false,
                space: false,
                tools_derivable: true,
                holds_release: false,
                seed: SeedKind::Fresh,
                pinned: true,
                connection_id_present: true,
                model_present: true,
            };
            assert!(!run_parkable(&run));
        }
        assert!(parking_active(), "the override is put back");
        // Without any override it is the production constant, which S8-6 flips.
        drop(_park);
        assert_eq!(park_resume_built(), PARK_RESUME_BUILT);
    }

    fn all_on() -> (
        ParkSwitch,
        crate::runtime_loop::resume::LoopResumeSwitch,
        crate::runtime_loop::resume::ContinueSwitch,
        fold::CheckpointSwitch,
        crate::runtime_loop::effect::LedgerSwitch,
    ) {
        (
            ParkSwitch::set(true),
            crate::runtime_loop::resume::LoopResumeSwitch::set(true),
            crate::runtime_loop::resume::ContinueSwitch::set(true),
            fold::CheckpointSwitch::set(true),
            crate::runtime_loop::effect::LedgerSwitch::set(true),
        )
    }

    fn parkable_run() -> StaticFacts {
        StaticFacts {
            depth: 0,
            zdr: false,
            waived: false,
            space: false,
            tools_derivable: true,
            holds_release: false,
            seed: SeedKind::Fresh,
            pinned: true,
            connection_id_present: true,
            model_present: true,
        }
    }

    /// The control and then each exclusion, alone: a run is parkable only when every fact is
    /// right, and each one that is wrong is enough to keep today's outcome.
    #[test]
    fn run_parkable_refuses_every_excluded_run() {
        let _on = all_on();
        assert!(run_parkable(&parkable_run()), "the control: a fresh pinned persistent root run");
        assert!(
            run_parkable(&StaticFacts {
                seed: SeedKind::Parked,
                ..parkable_run()
            }),
            "a park resume may park again"
        );
        let excluded = [
            (StaticFacts { depth: 1, ..parkable_run() }, "a delegated loop"),
            (StaticFacts { zdr: true, ..parkable_run() }, "a zero-retention run"),
            (StaticFacts { waived: true, ..parkable_run() }, "a waived run"),
            (StaticFacts { space: true, ..parkable_run() }, "a Space run"),
            (
                StaticFacts { tools_derivable: false, ..parkable_run() },
                "a run with client or MCP tools, or an approval continuation's tools",
            ),
            (StaticFacts { holds_release: true, ..parkable_run() }, "a release holder"),
            (
                StaticFacts { seed: SeedKind::Segment, ..parkable_run() },
                "a stage-1 continuation",
            ),
            (
                StaticFacts { seed: SeedKind::InPlace, ..parkable_run() },
                "an in-place approval resume",
            ),
            (StaticFacts { pinned: false, ..parkable_run() }, "an unpinned run"),
            (
                StaticFacts { connection_id_present: false, ..parkable_run() },
                "a route with no connection",
            ),
            (StaticFacts { model_present: false, ..parkable_run() }, "a route with no model"),
        ];
        for (facts, why) in excluded {
            assert!(!run_parkable(&facts), "{why} must keep today's outcome: {facts:?}");
        }
        // The switches are part of the decision.
        let off = ParkSwitch::set(false);
        assert!(!run_parkable(&parkable_run()), "parking off");
        drop(off);
        let _no_resume = crate::runtime_loop::resume::LoopResumeSwitch::set(false);
        assert!(!run_parkable(&parkable_run()), "a park that cannot be resumed is not written");
    }

    #[test]
    fn parkable_needs_a_writer_a_round_left_and_a_park_to_spare() {
        let facts = ParkFacts {
            run_parkable: true,
            writer_active: true,
            rounds_used: 3,
            max_rounds: 12,
            parks_so_far: 0,
        };
        assert!(parkable(&facts));
        assert!(!parkable(&ParkFacts { run_parkable: false, ..facts }));
        assert!(!parkable(&ParkFacts { writer_active: false, ..facts }), "no fold, no park");
        assert!(
            !parkable(&ParkFacts { rounds_used: 12, ..facts }),
            "a resume with no round left would only end the run"
        );
        assert!(parkable(&ParkFacts { rounds_used: 11, ..facts }));
        assert!(parkable(&ParkFacts { parks_so_far: MAX_PARKS_PER_RUN - 1, ..facts }));
        assert!(
            !parkable(&ParkFacts { parks_so_far: MAX_PARKS_PER_RUN, ..facts }),
            "the sixth park is refused"
        );
        assert_eq!(MAX_PARKS_PER_RUN, 5);
    }

    fn authority() -> lf::Authority {
        lf::Authority {
            mode: "ask".to_owned(),
            plan_mode: true,
            autonomy_rung: "read_only".to_owned(),
            space_id: String::new(),
            min_privacy_tier: 0,
            provider_hint: "openai-codex-subscription".to_owned(),
            tool_names: vec!["knowledge_search".to_owned()],
            max_rounds: 12,
        }
    }

    fn snapshot<'a>(
        retrievals: &'a BTreeSet<String>,
        messages: &'a [pb::ChatMessage],
    ) -> Snapshot<'a> {
        Snapshot {
            run_id: "run-1",
            segment_no: 1,
            rounds_executed: 3,
            delegated_rounds: 1,
            step_seq: 3,
            children_started: 1,
            grounded: true,
            compaction_triggered: false,
            attempted_retrievals: retrievals,
            messages,
            authority: authority(),
            prompt_digest: "digest",
            effect_ledger: true,
        }
    }

    fn chat(role: &str, content: &str) -> pb::ChatMessage {
        pb::ChatMessage {
            compaction_summary: String::new(),
            role: role.to_owned(),
            content: content.to_owned(),
            name: String::new(),
        }
    }

    fn site(channel: &Channel) -> ParkSite<'_> {
        ParkSite {
            run_id: "run-1",
            session_channel: channel,
            session_bearer: Some("session-token"),
            route_model: "gpt-5.6-terra",
            route_connection_id: "conn_1",
            child_ceiling: 4,
        }
    }

    /// The park fold is the round-boundary fold plus the four keys, its pending names the
    /// park of this count, and nothing else about it differs from a round fold.
    #[tokio::test]
    async fn the_park_fold_is_a_round_boundary_fold_naming_its_park_and_route() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let retrievals = BTreeSet::new();
        let messages = vec![chat("system", "p"), chat("user", "g")];
        let snapshot = snapshot(&retrievals, &messages);
        let round_fold = fold::build_fold(&snapshot, None, None);
        for n in 1..=MAX_PARKS_PER_RUN {
            let fold = park_fold(&snapshot, &site(&channel), n);
            assert!(fold.round_in_flight.is_none(), "a park is at a round boundary");
            let pending = fold.pending.as_ref().expect("a park names itself");
            assert_eq!(pending.kind, PauseKind::Park);
            assert_eq!(pending.pause_ref, format!("park:run-1:{n}"));
            assert_eq!(pending.pause_ref, lf::park_ref("run-1", n));
            assert!(
                pending.step_id.is_empty()
                    && pending.call.is_empty()
                    && pending.action_fingerprint.is_empty(),
                "a park concerns no step and no call"
            );
            assert_eq!(lf::parks_in(&fold), n, "the count rides in the fold, this park included");
            assert_eq!(lf::route_model_in(&fold), Some("gpt-5.6-terra"));
            assert_eq!(lf::route_connection_in(&fold), Some("conn_1"));
            assert_eq!(lf::child_ceiling_in(&fold), Some(4));
            // Everything but the pending and the four keys is the round fold's.
            let mut stripped = fold.clone();
            stripped.pending = None;
            for key in [
                lf::EXT_PARKS,
                lf::EXT_ROUTE_MODEL,
                lf::EXT_ROUTE_CONNECTION,
                lf::EXT_CHILD_CEILING,
            ] {
                stripped.extensions.remove(key);
            }
            assert_eq!(stripped, round_fold, "n={n}");
        }
        // A round fold never carries the keys: they are written only on a park.
        assert_eq!(lf::parks_in(&round_fold), 0);
        assert!(lf::route_model_in(&round_fold).is_none());
    }

    #[test]
    fn a_parked_response_is_parked_with_its_reason_and_due_time_and_nothing_else() {
        let parked = ParkedRun {
            reason: ParkReason::ProviderExhausted,
            park_ref: "park:run-1:2".to_owned(),
            seq: 9,
            park_until: at("2026-10-20T01:21:00Z"),
        };
        let response = parked_response(&parked, 5, true, true);
        assert_eq!(
            response,
            pb::RunAgentResponse {
                status: "parked".to_owned(),
                final_output: String::new(),
                rounds_executed: 5,
                grounded: true,
                compaction_triggered: true,
                failure_code: String::new(),
                retry_after_ms: None,
                resets_at: "2026-10-20T01:21:00Z".to_owned(),
                park_reason: "provider_exhausted".to_owned(),
            }
        );
        // Whole seconds, UTC, whatever the sub-second part.
        let fractional = ParkedRun {
            park_until: at("2026-10-20T01:21:00.987Z"),
            ..parked
        };
        assert_eq!(
            parked_response(&fractional, 1, false, false).resets_at,
            "2026-10-20T01:21:00Z"
        );
    }

    #[test]
    fn only_the_park_mode_is_a_park_resume() {
        let with_mode = |mode: &str| pb::RunAgentRequest {
            resume: Some(pb::AgentResume {
                mode: mode.to_owned(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(is_park_resume(&with_mode("park")));
        for other in ["", "segment", "loop", "PARK", " park"] {
            assert!(!is_park_resume(&with_mode(other)), "{other:?}");
        }
        assert!(!is_park_resume(&pb::RunAgentRequest::default()), "no resume at all");
    }

    /// A stop that may not park makes no call of any kind: no save, no close, no event. (A
    /// listener nothing serves stands in for session-core: a connection to it is an RPC.)
    #[tokio::test]
    async fn a_stop_that_may_not_park_makes_no_call() {
        let _on = all_on();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("non-blocking");
        let channel = tonic::transport::Endpoint::from_shared(format!(
            "http://{}",
            listener.local_addr().expect("addr")
        ))
        .expect("endpoint")
        .connect_lazy();
        let gate = fold::WriterGate {
            switch: true,
            ledger: true,
            zdr: false,
            waived: false,
            space: false,
            depth: 0,
            resumed: false,
        };
        let mut folds = FoldWriter::new(&channel, Some("session-token"), "run-1", gate);
        assert!(folds.is_active());
        let retrievals = BTreeSet::new();
        let messages = vec![chat("system", "p"), chat("user", "g")];
        let snapshot = snapshot(&retrievals, &messages);
        let usage = refusal(SUBSCRIPTION_USAGE_LIMIT);
        let facts = ParkFacts {
            run_parkable: true,
            writer_active: true,
            rounds_used: 3,
            max_rounds: 12,
            parks_so_far: 0,
        };
        let refused = [
            (ParkFacts { run_parkable: false, ..facts }, ParkStop::LatchedRefusal(&usage)),
            (ParkFacts { writer_active: false, ..facts }, ParkStop::LatchedRefusal(&usage)),
            (ParkFacts { rounds_used: 12, ..facts }, ParkStop::LatchedRefusal(&usage)),
            (
                ParkFacts { parks_so_far: MAX_PARKS_PER_RUN, ..facts },
                ParkStop::LatchedRefusal(&usage),
            ),
            (facts, ParkStop::Deadline(DeadlineCause::WallClock)),
        ];
        for (facts, stop) in refused {
            let parked =
                try_park(&site(&channel), &mut folds, &snapshot, &facts, &stop).await;
            assert_eq!(parked, None, "{facts:?} {stop:?}");
        }
        let mut connections = 0;
        while listener.accept().is_ok() {
            connections += 1;
        }
        assert_eq!(connections, 0, "nothing was sent to session-core");
        assert_eq!(folds.expected_seq(), 0, "the writer did not move");
    }
}
