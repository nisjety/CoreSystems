//! One table of deadlines for everything the driver asks Chromium to do.
//!
//! A CDP call that never answers (a renderer stuck in a loop, a target that
//! closed mid-call, a browser that died) used to hold the run lock and the
//! driver's browser lock until the router's 60 s timeout dropped the whole
//! request, which reports nothing useful and leaves the state undefined. Every
//! call now has an upper bound from this table and a typed `Timeout` that names
//! the operation:
//!
//! - `details.op`: which operation (`navigate`, `screenshot`, ...);
//! - `details.deadline_ms`: the bound that expired;
//! - `details.effect`: `none` when the operation cannot have changed the page,
//!   `unknown` when it may have (a click, a navigation, a script). An effectful
//!   timeout is **not retryable**: repeating a click that may have landed is not
//!   safe, so the caller has to look at the page first.
//!
//! The details never carry a URL, selector or page text.
//!
//! Only the three read helpers retry, and only once, when Chromium says the
//! execution context went away under them (a navigation raced the read). Nothing
//! effectful is ever retried here.
//!
//! `QUARRY_BROWSER_DEADLINE_SCALE` (a number in (0, 2], default 1) multiplies the
//! whole table, for tests and for a host that is slower than the numbers assume.
//!
//! # What is not in the table (the named exceptions)
//!
//! Every CDP call in `chromiumoxide.rs` runs inside one of the wrappers above. The
//! `cdp_calls_are_bounded` test scans the production source call by call: every method
//! call on a `page`, an `element` or a `browser` is a CDP round trip unless the test's
//! `NOT_CDP` list names it (with the reason), so a bare call next to a wrapped one is
//! reported, a new CDP method nobody listed is reported, and the three functions whose
//! bound comes from their caller are checked at their call sites. It cannot see a call whose
//! future is built first and wrapped later; that is what the review of a diff is for.
//! These durations are written elsewhere on purpose:
//!
//! - `BROWSER_EXIT_WAIT` (5 s): how long a retired browser process gets to exit before
//!   its directories are deleted. It waits on a child process, not on a CDP call.
//! - The launch builder's `request_timeout` (40 s) and `launch_timeout` (20 s): the
//!   library's own backstop, above every entry here so a typed `Timeout` always wins;
//!   it bounds only what has no entry (the event listeners).
//! - The wait for a screencast frame: the caller's `timeout_ms`, clamped to 100 ms to
//!   5 s (`normalize_live_frame_options`). It is the caller's answer to "how long may I
//!   wait for a picture", and it expires as the typed `live_frame` `Timeout`.
//! - `wait_for`'s caller timeout (capped by `Op::WaitFor`): a selector that does not
//!   appear in the time the caller allowed is the caller's answer, so it stays a
//!   `DriverFailed` "wait_for selector timed out" and is not a stalled CDP call.
//! - The 500 ms the trusted-input path gives `Runtime.releaseObject` after a click
//!   (`Op::StablePoll` is the same length but a different thing): the release queues
//!   behind a blocked `alert`, and the click's own result is returned either way.

use std::future::Future;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use serde_json::json;

/// What the driver is asking Chromium to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Launch,
    TabAttach,
    Navigate,
    Content,
    AxSnapshot,
    Screenshot,
    Pdf,
    Evaluate,
    Input,
    NativeAction,
    WaitForPoll,
    WaitFor,
    PageClose,
    ContextClose,
    /// Reserved for the page-opening probe behind `/ready` (WS3.F).
    #[allow(dead_code)]
    HealthProbe,
    ProfileCapture,
    DownloadBegin,
    DownloadComplete,
    /// The whole wait for a ref target to become actionable (visible, enabled,
    /// not covered), and the bound of each CDP call inside it; polled every
    /// 100 ms (WS3.K). Nothing has been clicked yet, so it is effect-less.
    Actionability,
    /// The window in which a target's box has to hold still, sampled every
    /// 25 ms (WS3.K). Effect-less.
    StablePoll,
    /// A tab's URL read for the tab list and its hints (`page.url()`). Effect-less; a read
    /// that does not answer leaves the last known value in place.
    TabMetadata,
    /// The CDP calls around one live screencast frame (start, ack, stop). The wait for
    /// the frame itself takes the caller's `timeout_ms` instead (see the exceptions in the
    /// module docs). Effect-less.
    LiveFrame,
    /// Answering one paused request in the network guard (`Fetch.failRequest`,
    /// `Fetch.continueRequest`). It runs on the guard's own task, so the error is only
    /// logged; a stuck answer must not hold every later request of the page behind it for
    /// the library's 40 s.
    GuardResolve,
    /// `Performance.getMetrics` for the telemetry sample (`page.metrics()`). Effect-less;
    /// the sample is optional, so a timeout leaves the observation without renderer metrics.
    Metrics,
}

impl Op {
    #[cfg(test)]
    pub(crate) const ALL: [Op; 24] = [
        Op::Launch,
        Op::TabAttach,
        Op::Navigate,
        Op::Content,
        Op::AxSnapshot,
        Op::Screenshot,
        Op::Pdf,
        Op::Evaluate,
        Op::Input,
        Op::NativeAction,
        Op::WaitForPoll,
        Op::WaitFor,
        Op::PageClose,
        Op::ContextClose,
        Op::HealthProbe,
        Op::ProfileCapture,
        Op::DownloadBegin,
        Op::DownloadComplete,
        Op::Actionability,
        Op::StablePoll,
        Op::TabMetadata,
        Op::LiveFrame,
        Op::GuardResolve,
        Op::Metrics,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Op::Launch => "launch",
            Op::TabAttach => "tab_attach",
            Op::Navigate => "navigate",
            Op::Content => "content",
            Op::AxSnapshot => "ax_snapshot",
            Op::Screenshot => "screenshot",
            Op::Pdf => "pdf",
            Op::Evaluate => "evaluate",
            Op::Input => "input",
            Op::NativeAction => "native_action",
            Op::WaitForPoll => "wait_for_poll",
            Op::WaitFor => "wait_for",
            Op::PageClose => "page_close",
            Op::ContextClose => "context_close",
            Op::HealthProbe => "health_probe",
            Op::ProfileCapture => "profile_capture",
            Op::DownloadBegin => "download_begin",
            Op::DownloadComplete => "download_complete",
            Op::Actionability => "actionability",
            Op::StablePoll => "stable_poll",
            Op::TabMetadata => "tab_metadata",
            Op::LiveFrame => "live_frame",
            Op::GuardResolve => "guard_resolve",
            Op::Metrics => "metrics",
        }
    }

    /// Whether the operation may have changed the page by the time it timed
    /// out. Reads and teardown cannot; a navigation, an input or a script can.
    pub(crate) const fn effectful(self) -> bool {
        matches!(
            self,
            Op::Navigate | Op::Evaluate | Op::Input | Op::NativeAction
        )
    }

    /// The table, before scaling. `Launch` is the outer bound: the launch
    /// itself is also limited to 20 s inside chromiumoxide.
    const fn base_millis(self) -> u64 {
        match self {
            Op::Launch => 25_000,
            Op::TabAttach => 15_000,
            Op::Navigate => 30_000,
            Op::Content => 15_000,
            Op::AxSnapshot => 10_000,
            Op::Screenshot => 10_000,
            Op::Pdf => 30_000,
            Op::Evaluate => 15_000,
            Op::Input => 5_000,
            Op::NativeAction => 10_000,
            Op::WaitForPoll => 2_000,
            Op::WaitFor => 30_000,
            Op::PageClose => 5_000,
            Op::ContextClose => 5_000,
            Op::HealthProbe => 3_000,
            Op::ProfileCapture => 5_000,
            Op::DownloadBegin => 10_000,
            Op::DownloadComplete => 30_000,
            Op::Actionability => 2_000,
            Op::StablePoll => 500,
            Op::TabMetadata => 3_000,
            Op::LiveFrame => 5_000,
            Op::GuardResolve => 5_000,
            Op::Metrics => 3_000,
        }
    }
}

/// `QUARRY_BROWSER_DEADLINE_SCALE`, read once. Anything outside (0, 2] or not a
/// number is ignored (with a warning) and leaves the table as written.
fn scale() -> f64 {
    static SCALE: OnceLock<f64> = OnceLock::new();
    *SCALE.get_or_init(|| {
        parse_scale(
            std::env::var("QUARRY_BROWSER_DEADLINE_SCALE")
                .ok()
                .as_deref(),
        )
    })
}

pub(crate) fn parse_scale(raw: Option<&str>) -> f64 {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return 1.0;
    };
    match raw.parse::<f64>() {
        Ok(value) if value > 0.0 && value <= 2.0 => value,
        _ => {
            tracing::warn!("QUARRY_BROWSER_DEADLINE_SCALE must be a number in (0, 2]; using 1");
            1.0
        }
    }
}

pub(crate) fn deadline_scaled(op: Op, scale: f64) -> Duration {
    let millis = (op.base_millis() as f64 * scale).round().max(1.0);
    Duration::from_millis(millis as u64)
}

/// The bound for `op` on this process.
pub(crate) fn deadline(op: Op) -> Duration {
    deadline_scaled(op, scale())
}

/// The typed timeout for `op`.
pub(crate) fn timeout_error(op: Op, limit: Duration) -> QuarryError {
    let effectful = op.effectful();
    let mut error = QuarryError::new(
        ErrorCode::Timeout,
        format!("browser operation {} timed out", op.name()),
    )
    .with_details(json!({
        "op": op.name(),
        "deadline_ms": u64::try_from(limit.as_millis()).unwrap_or(u64::MAX),
        "effect": if effectful { "unknown" } else { "none" },
    }));
    // A read can be repeated. A click that timed out may have landed.
    error.retryable = !effectful;
    error
}

/// Run `fut` under `op`'s deadline. On expiry the future is dropped and the
/// typed timeout comes back. Logs how long the call took at debug level, the
/// calibration signal for the table.
pub(crate) async fn with_deadline<T, F>(op: Op, fut: F) -> QuarryResult<T>
where
    F: Future<Output = QuarryResult<T>>,
{
    let limit = deadline(op);
    let started = Instant::now();
    let outcome = tokio::time::timeout(limit, fut).await;
    tracing::debug!(
        op = op.name(),
        ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "browser_op_duration_ms"
    );
    match outcome {
        Ok(result) => result,
        Err(_) => Err(timeout_error(op, limit)),
    }
}

/// Teardown (closing a page or disposing a context) under its deadline, where a
/// failure is only worth a log line: the caller gets the reason as text, never a
/// typed error to propagate.
pub(crate) async fn best_effort<T, E, F>(op: Op, fut: F) -> Result<T, String>
where
    E: std::fmt::Display,
    F: Future<Output = Result<T, E>>,
{
    let outcome = with_deadline(op, async {
        fut.await
            .map_err(|error| QuarryError::new(ErrorCode::DriverFailed, error.to_string()))
    })
    .await;
    outcome.map_err(|error| error.message)
}

/// A closed-vocabulary label for a CDP failure that is only logged: `timeout` when
/// our own per-op deadline fired (see [`timeout_error`]), `cdp` for anything the
/// protocol or the connection said. The text itself is not logged: a CDP error can
/// quote the page it was about, and the event filter drops a ZDR caller's line for
/// carrying a field named `error`.
#[cfg(feature = "chromiumoxide")]
pub(crate) fn cdp_class(text: &impl std::fmt::Display) -> &'static str {
    if text.to_string().contains(" timed out") {
        "timeout"
    } else {
        "cdp"
    }
}

/// Signatures of a CDP call that lost its execution context to a navigation.
const DESTROYED_CONTEXT_SIGNATURES: [&str; 3] = [
    "Cannot find context with specified id",
    "Execution context was destroyed",
    "Inspected target navigated or closed",
];

pub(crate) fn is_context_destroyed(text: &str) -> bool {
    DESTROYED_CONTEXT_SIGNATURES
        .iter()
        .any(|signature| text.contains(signature))
}

fn error_is_context_destroyed(error: &QuarryError) -> bool {
    is_context_destroyed(&error.message)
        || error
            .details
            .as_ref()
            .is_some_and(|details| is_context_destroyed(&details.to_string()))
}

/// [`with_deadline`] for a read, retried once after 100 ms when the context went
/// away mid-call. A second failure, or any other failure, is returned as it is.
pub(crate) async fn read_with_retry<T, F, Fut>(op: Op, mut attempt: F) -> QuarryResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = QuarryResult<T>>,
{
    debug_assert!(!op.effectful(), "only reads are retried");
    match with_deadline(op, attempt()).await {
        Err(error) if error_is_context_destroyed(&error) => {
            tokio::time::sleep(Duration::from_millis(100)).await;
            with_deadline(op, attempt()).await
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn every_op_has_a_positive_deadline_below_the_router_cap() {
        // The edge router drops a request after 60 s; a typed timeout has to win.
        for op in Op::ALL {
            let limit = deadline_scaled(op, 1.0);
            assert!(limit > Duration::ZERO, "{}", op.name());
            assert!(
                limit < Duration::from_secs(60),
                "{} is {limit:?}",
                op.name()
            );
        }
        // Even at the largest scale.
        for op in Op::ALL {
            assert!(
                deadline_scaled(op, 2.0) <= Duration::from_secs(60),
                "{}",
                op.name()
            );
        }
    }

    #[test]
    fn op_names_are_unique_and_snake_case() {
        let mut names: Vec<&str> = Op::ALL.iter().map(|op| op.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Op::ALL.len());
        assert!(names
            .iter()
            .all(|name| name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')));
    }

    #[cfg(feature = "chromiumoxide")]
    #[test]
    fn cdp_class_is_a_closed_label_and_never_the_text() {
        let deadline = timeout_error(Op::PageClose, Duration::from_millis(50)).message;
        assert_eq!(cdp_class(&deadline), "timeout");
        // Anything else the protocol says, including text that quotes a page, is `cdp`.
        assert_eq!(cdp_class(&"No target with given id found"), "cdp");
        assert_eq!(cdp_class(&"navigation to https://example.test/secret failed"), "cdp");
    }

    #[test]
    fn timeout_error_names_the_op_and_is_not_retryable_for_effectful_ops() {
        for op in [Op::Navigate, Op::Evaluate, Op::Input, Op::NativeAction] {
            let error = timeout_error(op, Duration::from_millis(1500));
            assert_eq!(error.code, ErrorCode::Timeout);
            assert!(!error.retryable, "{}", op.name());
            let details = error.details.expect("details");
            assert_eq!(details["op"], op.name());
            assert_eq!(details["deadline_ms"], 1500);
            assert_eq!(details["effect"], "unknown");
        }
    }

    #[test]
    fn actionability_ops_are_effect_less_and_retryable() {
        // Nothing has been clicked while a target is still being checked, so a
        // timeout there says the page was not touched and may be retried.
        for op in [Op::Actionability, Op::StablePoll] {
            assert!(!op.effectful(), "{}", op.name());
            let error = timeout_error(op, Duration::from_millis(500));
            assert!(error.retryable, "{}", op.name());
            assert_eq!(error.details.unwrap()["effect"], "none");
        }
        assert_eq!(
            deadline_scaled(Op::Actionability, 1.0),
            Duration::from_secs(2)
        );
        assert_eq!(
            deadline_scaled(Op::StablePoll, 1.0),
            Duration::from_millis(500)
        );
    }

    #[test]
    fn read_ops_timeout_is_retryable() {
        for op in [
            Op::Content,
            Op::Screenshot,
            Op::AxSnapshot,
            Op::Pdf,
            Op::PageClose,
        ] {
            let error = timeout_error(op, Duration::from_secs(1));
            assert!(error.retryable, "{}", op.name());
            assert_eq!(error.details.unwrap()["effect"], "none");
        }
    }

    #[test]
    fn timeout_details_never_carry_a_url_or_selector() {
        let rendered = timeout_error(Op::Navigate, Duration::from_secs(1))
            .details
            .unwrap()
            .to_string();
        assert!(
            !rendered.contains("url") && !rendered.contains("selector"),
            "{rendered}"
        );
    }

    #[test]
    fn scale_parses_only_numbers_in_range() {
        assert_eq!(parse_scale(None), 1.0);
        assert_eq!(parse_scale(Some("")), 1.0);
        assert_eq!(parse_scale(Some("0.5")), 0.5);
        assert_eq!(parse_scale(Some(" 2 ")), 2.0);
        for bad in ["0", "-1", "2.5", "fast", "NaN"] {
            assert_eq!(parse_scale(Some(bad)), 1.0, "{bad:?}");
        }
        assert_eq!(
            deadline_scaled(Op::Navigate, 0.02),
            Duration::from_millis(600)
        );
    }

    #[test]
    fn context_destroyed_matcher_accepts_known_signatures_only() {
        assert!(is_context_destroyed(
            "Cannot find context with specified id"
        ));
        assert!(is_context_destroyed(
            "x: Execution context was destroyed, most likely because of a navigation"
        ));
        assert!(is_context_destroyed("Inspected target navigated or closed"));
        assert!(!is_context_destroyed("net::ERR_CONNECTION_REFUSED"));
        assert!(!is_context_destroyed("timed out"));
        assert!(!is_context_destroyed(""));
    }

    #[tokio::test(start_paused = true)]
    async fn with_deadline_uses_tokio_paused_time() {
        let started = tokio::time::Instant::now();
        let error = with_deadline(Op::Screenshot, async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok::<_, QuarryError>(())
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
        assert_eq!(error.details.as_ref().unwrap()["op"], "screenshot");
        // The paused clock advanced by the deadline, not by an hour.
        assert!(started.elapsed() < Duration::from_secs(60));
        // A fast call is returned untouched.
        let value = with_deadline(Op::Content, async { Ok::<_, QuarryError>(7) })
            .await
            .unwrap();
        assert_eq!(value, 7);
    }

    fn destroyed() -> QuarryError {
        QuarryError::new(ErrorCode::DriverFailed, "read failed")
            .with_details(json!({ "error": "Cannot find context with specified id" }))
    }

    #[tokio::test(start_paused = true)]
    async fn read_with_retry_retries_once_then_surfaces() {
        // Fails once with a destroyed context, then succeeds: one retry.
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let value = read_with_retry(Op::Content, move || {
            let seen = seen.clone();
            async move {
                if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(destroyed())
                } else {
                    Ok("page")
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(value, "page");
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // Fails twice: the second failure surfaces, no third attempt.
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let error = read_with_retry(Op::Content, move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(destroyed())
            }
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::DriverFailed);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // Any other error is not retried.
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let _ = read_with_retry(Op::Content, move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(QuarryError::new(
                    ErrorCode::DriverFailed,
                    "net::ERR_ABORTED",
                ))
            }
        })
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
