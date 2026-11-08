//! Graceful shutdown: stop taking new work, give in-flight work a bounded time
//! to finish, then release every live browser session instead of dying with
//! them.
//!
//! Before this, `docker stop` sent SIGTERM and axum's graceful shutdown waited
//! for every open connection. A single SSE stream or WebSocket never ends on
//! its own, so the process was killed at the platform's grace period (exit 137)
//! with its Chromium sessions still open. The sequence now is:
//!
//! 1. SIGTERM: [`begin`]. `/ready` answers 503 `draining`, and a request to
//!    start an agent run, a crawl or a batch gets 503 `RUNTIME_NOT_READY`
//!    (retry elsewhere or shortly), while steps on runs already live continue.
//! 2. Wait for in-flight requests up to [`deadline_from_env`] (default 30 s).
//!    Connections still open then (a stream with a viewer) are closed.
//! 3. Release every remaining browser session. A non-ZDR run keeps its
//!    checkpoint `Active`, so it can be resumed with `resume_run_id` after the
//!    restart; a ZDR run has none and is simply gone. Each released run emits its
//!    last usage segment and an `agent.expired` event with `reason: "shutdown"`
//!    (WS6.J), so a viewer sees why the run ended.
//! 4. Close the streams that are still open ([`close_streams`]): an event stream, a
//!    frame stream or a browser WebSocket ends with a `done` frame instead of
//!    holding axum's graceful shutdown open until the platform kills the process.
//! 5. Flush the usage meter, then exit.
//!
//! The flag is process-global, so only a test binary of its own may set it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use quarry_core::error::{ErrorCode, QuarryError};
use tokio::sync::Notify;

pub const ENV_DEADLINE: &str = "QUARRY_EDGE__DRAIN_DEADLINE_S";
pub const DEFAULT_DEADLINE_S: u64 = 30;
const MAX_DEADLINE_S: u64 = 600;

/// What a new request is told while draining.
pub const RETRY_AFTER_SECONDS: u64 = 5;

static DRAINING: AtomicBool = AtomicBool::new(false);
static STARTED: Notify = Notify::const_new();
static STREAMS_CLOSING: AtomicBool = AtomicBool::new(false);
static STREAMS_CLOSED: Notify = Notify::const_new();

/// How long to wait for in-flight requests before closing the rest. Unset is
/// [`DEFAULT_DEADLINE_S`]; anything that is not an integer from 1 to 600 is an
/// error naming the variable (and must stay under the platform's stop grace
/// period, which is 45 s in the shipped compose file).
pub fn deadline_from_env(get: impl Fn(&str) -> Option<String>) -> Result<Duration, String> {
    let Some(raw) = get(ENV_DEADLINE)
        .map(|value| value.trim().to_owned())
        .filter(|v| !v.is_empty())
    else {
        return Ok(Duration::from_secs(DEFAULT_DEADLINE_S));
    };
    match raw.parse::<u64>() {
        Ok(seconds) if (1..=MAX_DEADLINE_S).contains(&seconds) => Ok(Duration::from_secs(seconds)),
        _ => Err(format!(
            "{ENV_DEADLINE} must be an integer number of seconds from 1 to {MAX_DEADLINE_S}, got {raw:?}"
        )),
    }
}

/// Start draining. Idempotent.
pub fn begin() {
    if !DRAINING.swap(true, Ordering::SeqCst) {
        // WS6.I: visible at once, not at the sampler's next tick.
        metrics::gauge!("quarry_edge_draining").set(1.0);
        tracing::info!("drain: no longer accepting new runs, crawls or batches");
        STARTED.notify_waiters();
    }
}

pub fn is_draining() -> bool {
    DRAINING.load(Ordering::SeqCst)
}

/// Resolves once [`begin`] has been called.
pub async fn started() {
    loop {
        let notified = STARTED.notified();
        if is_draining() {
            return;
        }
        notified.await;
    }
}

/// Ask every open stream to end (WS6.J). Called once the runs have been released, so a
/// viewer has already received the `agent.expired` event. Idempotent. The flag is
/// process-global, like [`begin`].
pub fn close_streams() {
    if !STREAMS_CLOSING.swap(true, Ordering::SeqCst) {
        STREAMS_CLOSED.notify_waiters();
    }
}

/// Whether [`close_streams`] has been called.
pub fn streams_are_closing() -> bool {
    STREAMS_CLOSING.load(Ordering::SeqCst)
}

/// Resolves once [`close_streams`] has been called. A stream selects on this next to its
/// own wait, so it ends the moment shutdown asks it to.
pub async fn streams_closing() {
    loop {
        let notified = STREAMS_CLOSED.notified();
        if streams_are_closing() {
            return;
        }
        notified.await;
    }
}

/// Put both process-global flags back (and the gauge). For an integration-test binary that
/// runs the drain more than once; the edge itself never calls it.
#[doc(hidden)]
#[allow(dead_code)] // the binary compiles this module too and never calls it
pub fn reset_for_tests() {
    DRAINING.store(false, Ordering::SeqCst);
    STREAMS_CLOSING.store(false, Ordering::SeqCst);
    metrics::gauge!("quarry_edge_draining").set(0.0);
}

/// The refusal a request to start new work gets while draining, or `None` when
/// the edge is taking work.
pub fn refusal() -> Option<QuarryError> {
    refusal_if(is_draining())
}

pub fn refusal_if(draining: bool) -> Option<QuarryError> {
    draining.then(|| {
        let mut error = QuarryError::new(
            ErrorCode::RuntimeNotReady,
            "this instance is shutting down and is not taking new work; retry shortly",
        )
        .with_details(serde_json::json!({
            "reason": "draining",
            "retry_after_seconds": RETRY_AFTER_SECONDS,
        }));
        // RUNTIME_NOT_READY is not retryable in general (a missing dependency
        // does not fix itself); a draining instance is replaced within seconds.
        error.retryable = true;
        error
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| vars.get(name).cloned()
    }

    #[test]
    fn the_deadline_defaults_and_rejects_nonsense() {
        assert_eq!(deadline_from_env(env(&[])), Ok(Duration::from_secs(30)));
        assert_eq!(
            deadline_from_env(env(&[(ENV_DEADLINE, "")])),
            Ok(Duration::from_secs(30))
        );
        assert_eq!(
            deadline_from_env(env(&[(ENV_DEADLINE, " 12 ")])),
            Ok(Duration::from_secs(12))
        );
        for bad in ["0", "601", "-5", "soon", "1.5"] {
            let error = deadline_from_env(env(&[(ENV_DEADLINE, bad)])).unwrap_err();
            assert!(error.contains(ENV_DEADLINE), "{bad}: {error}");
        }
    }

    #[test]
    fn a_draining_edge_refuses_new_work_with_a_typed_retryable_503() {
        assert!(refusal_if(false).is_none());
        let error = refusal_if(true).expect("refused");
        assert_eq!(error.code, ErrorCode::RuntimeNotReady);
        assert_eq!(error.code.http_status(), 503);
        assert!(error.retryable);
        assert_eq!(error.details.as_ref().unwrap()["reason"], "draining");
        assert_eq!(error.details.as_ref().unwrap()["retry_after_seconds"], 5);
    }
}
