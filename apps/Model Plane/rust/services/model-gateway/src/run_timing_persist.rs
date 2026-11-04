//! A run's durable, content-free timing record: the gateway's writer (EP-14 S5).
//!
//! Two records reach session-core's `RunLedgerService.RecordRunTiming`, each as one
//! `RUN_TIMING_RECORDED` event of its run:
//!
//! - an Ask turn's `turn_timing` report (`turn-timing-v1`), the same serialised report the
//!   SSE frame carries. The turn is armed once its run exists ([`arm_turn`]), and the one
//!   place the report is built, `sse.rs`'s `emit_turn_timing`, hands it over ([`persist_turn`]);
//! - a Do run's `RunAgentResponse.round_timing` (`round-timing-v1`, [`persist_rounds`]).
//!
//! Both are best effort and after the frame: the write runs on its own task, and a failure
//! is a counter and a debug line, never a turn error. Nothing is written with
//! `MODEL_GATEWAY_RUN_TIMING_PERSIST` off, for a ZDR turn or for a waived turn (DEC-32, D13);
//! session-core refuses both again on its side. The write carries the user's session bearer,
//! so session-core attributes it to the run's owner, never to a service. The record holds ids,
//! milliseconds, counts and offered tool names (see `turn_timing`), and nothing of it is logged.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1::{RecordRunTimingRequest, RoundTiming, RunTimingRecorded};
use mp_contracts::run_timing::{SCHEMA_ROUND, SCHEMA_TURN};

use crate::auth::VerifiedSessionBearer;
use crate::state::AppState;
use crate::turn_timing::TurnTimingReport;

/// The writer switch. Only the literal `true` enables it.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_RUN_TIMING_PERSIST";

/// Most Ask turns armed at once. A turn whose stream never reports (a client that left
/// before the stream task started) is dropped after [`ARMED_TTL`], so this is only reached
/// under a burst; a turn that finds it full is not recorded, and counted.
const MAX_ARMED: usize = 4096;

/// How long an armed turn waits for its report. Longer than any streamed turn.
const ARMED_TTL: Duration = Duration::from_secs(30 * 60);

#[cfg(test)]
thread_local! {
    /// A test's override of the switch, so tests never touch the process environment.
    static ENABLED_OVERRIDE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether timing records are written.
#[must_use]
pub fn enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = ENABLED_OVERRIDE.with(std::cell::Cell::get) {
        return forced;
    }
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// Whether a run of this posture may keep a timing record: the switch is on, and the turn is
/// neither ZDR nor waived (D13).
#[must_use]
pub const fn permits(enabled: bool, zdr: bool, waived: bool) -> bool {
    enabled && !zdr && !waived
}

/// Where an armed Ask turn's report goes.
struct Target {
    state: AppState,
    bearer: VerifiedSessionBearer,
    run_id: String,
    armed_at: Instant,
}

fn armed() -> std::sync::MutexGuard<'static, HashMap<String, Target>> {
    static ARMED: OnceLock<Mutex<HashMap<String, Target>>> = OnceLock::new();
    ARMED
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn count(outcome: &'static str) {
    metrics::counter!("mp_gateway_run_timing_persist_total", "outcome" => outcome).increment(1);
}

/// Arm an Ask turn: its report, when `emit_turn_timing` builds it, is written for `run_id`.
/// Returns whether the turn was armed; a ZDR or waived turn, or the switch off, arms nothing.
pub fn arm_turn(
    state: &AppState,
    request_id: &str,
    run_id: &str,
    bearer: &VerifiedSessionBearer,
    zdr: bool,
    waived: bool,
) -> bool {
    if !permits(enabled(), zdr, waived) || request_id.is_empty() || run_id.is_empty() {
        return false;
    }
    let mut targets = armed();
    targets.retain(|_, target| target.armed_at.elapsed() < ARMED_TTL);
    if targets.len() >= MAX_ARMED {
        count("not_armed_full");
        return false;
    }
    targets.insert(
        request_id.to_owned(),
        Target {
            state: state.clone(),
            bearer: bearer.clone(),
            run_id: run_id.to_owned(),
            armed_at: Instant::now(),
        },
    );
    true
}

/// Write an armed turn's report. Does nothing for a turn that was not armed. Returns whether
/// a write was started.
pub fn persist_turn(report: &TurnTimingReport) -> bool {
    let Some(target) = armed().remove(&report.request_id) else {
        return false;
    };
    let Ok(report_json) = serde_json::to_string(report) else {
        count("not_serialised");
        return false;
    };
    spawn_write(
        target.state,
        target.bearer,
        RunTimingRecorded {
            run_id: target.run_id,
            request_id: report.request_id.clone(),
            schema: SCHEMA_TURN.to_owned(),
            report_json,
            phases_dropped: report.phases_dropped,
            recorded_at: None,
        },
    );
    true
}

/// Write a Do run's round timing. Does nothing with the switch off, for a ZDR or waived turn,
/// or when execution-core sent no rounds (its own switch off). Returns whether a write was
/// started.
pub fn persist_rounds(
    state: &AppState,
    run_id: &str,
    request_id: &str,
    bearer: &VerifiedSessionBearer,
    zdr: bool,
    waived: bool,
    rounds: &[RoundTiming],
) -> bool {
    if !permits(enabled(), zdr, waived) || rounds.is_empty() || run_id.is_empty() {
        return false;
    }
    spawn_write(
        state.clone(),
        bearer.clone(),
        RunTimingRecorded {
            run_id: run_id.to_owned(),
            request_id: request_id.to_owned(),
            schema: SCHEMA_ROUND.to_owned(),
            report_json: mp_contracts::run_timing::round_report(rounds).to_string(),
            phases_dropped: 0,
            recorded_at: None,
        },
    );
    true
}

fn spawn_write(state: AppState, bearer: VerifiedSessionBearer, timing: RunTimingRecorded) {
    tokio::spawn(async move {
        match write(&state, &bearer, timing).await {
            Ok(true) => count("stored"),
            Ok(false) => count("repeated"),
            Err(status) => {
                // The code only: a status message is not repeated into the log.
                tracing::debug!(code = ?status.code(), "run timing record not stored");
                count("failed");
            }
        }
    });
}

/// One `RecordRunTiming` call with the user's session bearer. `Ok(false)` is a repeat.
async fn write(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    timing: RunTimingRecorded,
) -> Result<bool, tonic::Status> {
    let request = crate::http_routes::authenticated_session_request(
        RecordRunTimingRequest {
            timing: Some(timing),
        },
        bearer,
    )
    .map_err(|_| tonic::Status::internal("the session credential is not forwardable"))?;
    let response = state
        .run_ledger_client
        .clone()
        .record_run_timing(request)
        .await?;
    Ok(response.into_inner().stored)
}

#[cfg(test)]
struct EnabledSwitch;

#[cfg(test)]
impl EnabledSwitch {
    fn set(on: bool) -> Self {
        ENABLED_OVERRIDE.with(|cell| cell.set(Some(on)));
        Self
    }
}

#[cfg(test)]
impl Drop for EnabledSwitch {
    fn drop(&mut self) {
        ENABLED_OVERRIDE.with(|cell| cell.set(None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn_timing::{Outcome, TurnTiming};

    fn bearer() -> VerifiedSessionBearer {
        VerifiedSessionBearer::for_test("session-token")
    }

    fn round() -> RoundTiming {
        RoundTiming {
            round: 1,
            inference_start_ms: 3,
            inference_end_ms: 400,
            dispatch_end_ms: 900,
            tool_count: 1,
        }
    }

    fn is_armed(request_id: &str) -> bool {
        armed().contains_key(request_id)
    }

    #[test]
    fn only_a_persistent_turn_with_the_switch_on_is_permitted() {
        assert!(permits(true, false, false));
        assert!(!permits(false, false, false));
        assert!(!permits(true, true, false));
        assert!(!permits(true, false, true));
    }

    #[test]
    fn the_switch_takes_only_the_literal_true() {
        for raw in [None, Some(""), Some("1"), Some("yes"), Some("on")] {
            assert!(!mp_contracts::loop_fold::switch_from(raw), "{raw:?}");
        }
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
    }

    #[tokio::test]
    async fn run_timing_persist_off_writes_nothing() {
        let _off = EnabledSwitch::set(false);
        let state = AppState::new();
        assert!(!arm_turn(&state, "req-off", "run-1", &bearer(), false, false));
        assert!(!is_armed("req-off"));
        assert!(!persist_rounds(&state, "run-1", "req-off", &bearer(), false, false, &[round()]));
        let timing = TurnTiming::start("req-off");
        let report = timing.report(Outcome::Completed).expect("report");
        assert!(!persist_turn(&report), "an unarmed turn writes nothing");
    }

    #[tokio::test]
    async fn run_timing_persist_skips_zdr_turn() {
        let _on = EnabledSwitch::set(true);
        let state = AppState::new();
        assert!(!arm_turn(&state, "req-zdr", "run-1", &bearer(), true, false));
        assert!(!is_armed("req-zdr"));
        assert!(!persist_rounds(&state, "run-1", "req-zdr", &bearer(), true, false, &[round()]));
    }

    #[tokio::test]
    async fn run_timing_persist_skips_waived_turn() {
        let _on = EnabledSwitch::set(true);
        let state = AppState::new();
        assert!(!arm_turn(&state, "req-waived", "run-1", &bearer(), false, true));
        assert!(!is_armed("req-waived"));
        assert!(!persist_rounds(&state, "run-1", "req-waived", &bearer(), false, true, &[round()]));
    }

    #[tokio::test]
    async fn run_timing_persist_without_rounds_writes_nothing() {
        let _on = EnabledSwitch::set(true);
        let state = AppState::new();
        assert!(!persist_rounds(&state, "run-1", "req-empty", &bearer(), false, false, &[]));
    }

    #[tokio::test]
    async fn run_timing_persist_failure_does_not_fail_turn() {
        let _on = EnabledSwitch::set(true);
        // The test state's session channel points at nothing, so the write fails.
        let state = AppState::new();
        assert!(arm_turn(&state, "req-fail", "run-1", &bearer(), false, false));
        let timing = TurnTiming::start("req-fail");
        let report = timing.report(Outcome::Completed).expect("report");
        assert!(persist_turn(&report), "the write is started");
        assert!(!is_armed("req-fail"), "a turn is written once");
        assert!(!persist_turn(&report), "a second report finds nothing armed");
        let record = RunTimingRecorded {
            run_id: "run-1".to_owned(),
            request_id: "req-fail".to_owned(),
            schema: SCHEMA_TURN.to_owned(),
            report_json: "{}".to_owned(),
            phases_dropped: 0,
            recorded_at: None,
        };
        assert!(write(&state, &bearer(), record).await.is_err(), "the failure is the writer's alone");
    }

    #[tokio::test]
    async fn the_turn_report_serialises_without_content() {
        let timing = TurnTiming::start("req-shape");
        let report = timing.report(Outcome::Completed).expect("report");
        let json: serde_json::Value =
            serde_json::to_value(&report).expect("the report serialises");
        assert_eq!(json["request_id"], "req-shape");
        let record = RunTimingRecorded {
            run_id: "run-1".to_owned(),
            request_id: "req-shape".to_owned(),
            schema: SCHEMA_TURN.to_owned(),
            report_json: json.to_string(),
            phases_dropped: report.phases_dropped,
            recorded_at: None,
        };
        assert!(mp_contracts::run_timing::validate(&record).is_ok());
    }
}
