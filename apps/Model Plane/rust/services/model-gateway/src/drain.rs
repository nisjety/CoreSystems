//! Graceful drain on shutdown, so a redeploy no longer kills chat turns in flight.
//!
//! Until this module, SIGTERM ended `main` at once and the runtime dropped every
//! producer task with it. A chat turn is produced by a spawned task that already
//! outlives a disconnected client (it keeps going to persist the answer and
//! terminalize the run), so the only thing that stopped it was the process
//! exiting underneath it. Any launcher redeploy of this container therefore cut
//! the turns of everyone using it, mid-answer, and left their runs non-terminal.
//!
//! The drain makes the exit wait for them:
//!
//! 1. `/readyz` starts answering 503, so an orchestrator or load balancer stops
//!    routing new traffic here ([`GrpcReadiness::begin_drain`]);
//! 2. the process waits until no stream is registered in the [`CancelRegistry`]
//!    (every chat turn registers on start and finishes on every exit path), or
//!    until the drain window runs out;
//! 3. only then does the HTTP server stop accepting and `main` return.
//!
//! The window is `MODEL_GATEWAY_DRAIN_SECS` (default 120, `0` restores the old
//! immediate exit). The container's `stop_grace_period` must be longer than the
//! window plus the short flush bound in `main`, or Docker kills the process
//! before it finishes draining; the compose file sets 150 s against 120 s.
//!
//! What it does not cover: a request that has been accepted but has not yet
//! registered its stream (the preflight, normally well under a second), unary
//! calls, the gRPC listener, and the persistence-free ZDR path, which holds no
//! registration. A turn started during the drain is waited for like any other
//! (the listener stays open until the drain ends, so resume, stop and queued
//! input keep working), but it can only extend the drain up to the window.

use std::future::Future;
use std::time::Duration;

use futures::future::Shared;
use futures::FutureExt as _;
use tokio::time::Instant;
use tracing::{info, warn};

use crate::{cancel_registry::CancelRegistry, readiness::GrpcReadiness};

/// Environment variable holding the drain window, in whole seconds.
pub const DRAIN_SECS_ENV: &str = "MODEL_GATEWAY_DRAIN_SECS";

/// How long a draining process waits for streams to finish. A long governed
/// answer (tool phase, source review, bounded repair) runs for a minute or two.
pub const DEFAULT_DRAIN_SECS: u64 = 120;

/// Ceiling for the configured window; a larger value is clamped, not trusted.
pub const MAX_DRAIN_SECS: u64 = 3_600;

/// How often the registry is looked at while draining.
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How a drain ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    /// No stream was left running.
    Idle,
    /// The window ran out with `still_active` streams still running; they are
    /// stopped by the exit that follows, exactly as before this module.
    TimedOut { still_active: usize },
}

/// The drain window from `MODEL_GATEWAY_DRAIN_SECS`.
///
/// Unset or empty gives the default. A value that is not a whole number gives
/// the default too, with a warning, and one above the ceiling is clamped to it.
/// `0` is a real value: it means do not wait.
#[must_use]
pub fn drain_window_from_env() -> Duration {
    parse_drain_window(std::env::var(DRAIN_SECS_ENV).ok().as_deref())
}

fn parse_drain_window(raw: Option<&str>) -> Duration {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Duration::from_secs(DEFAULT_DRAIN_SECS);
    };
    match raw.parse::<u64>() {
        Ok(secs) if secs > MAX_DRAIN_SECS => {
            warn!(
                configured = secs,
                clamped_to = MAX_DRAIN_SECS,
                "{DRAIN_SECS_ENV} is above the ceiling; using the ceiling"
            );
            Duration::from_secs(MAX_DRAIN_SECS)
        }
        Ok(secs) => Duration::from_secs(secs),
        Err(_) => {
            warn!(
                default_secs = DEFAULT_DRAIN_SECS,
                "{DRAIN_SECS_ENV} is not a whole number of seconds; using the default"
            );
            Duration::from_secs(DEFAULT_DRAIN_SECS)
        }
    }
}

/// Wait until `active()` reports no running work, or `window` has passed.
///
/// Takes the count as a closure so the waiter is tested without a registry and
/// without wall-clock time. It looks at the count before it sleeps, so an
/// already idle process returns at once, and a window of zero never sleeps.
pub async fn wait_until_idle<F>(active: F, window: Duration, poll: Duration) -> DrainOutcome
where
    F: Fn() -> usize,
{
    let deadline = Instant::now() + window;
    loop {
        let running = active();
        if running == 0 {
            return DrainOutcome::Idle;
        }
        let now = Instant::now();
        if now >= deadline {
            return DrainOutcome::TimedOut {
                still_active: running,
            };
        }
        tokio::time::sleep(poll.min(deadline - now)).await;
    }
}

/// Take this process out of rotation and wait for its streams to finish.
///
/// Raises the draining flag first (so `/readyz` answers 503 before any waiting
/// starts), then waits on the registry. Returns how it ended; the caller exits
/// either way.
pub async fn drain(
    readiness: &GrpcReadiness,
    cancels: &CancelRegistry,
    window: Duration,
) -> DrainOutcome {
    readiness.begin_drain();
    let active = cancels.active();
    if window.is_zero() {
        info!(
            active,
            "draining disabled ({DRAIN_SECS_ENV}=0): stopping without waiting for streams"
        );
    } else {
        info!(
            active,
            window_secs = window.as_secs(),
            "draining: /readyz now answers 503, waiting for in-flight streams to finish"
        );
    }
    let started = Instant::now();
    let outcome = wait_until_idle(|| cancels.active(), window, POLL_INTERVAL).await;
    match outcome {
        DrainOutcome::Idle => info!(
            waited_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "drained: no stream left running"
        ),
        DrainOutcome::TimedOut { still_active } => warn!(
            still_active,
            window_secs = window.as_secs(),
            "drain window elapsed with streams still running; they stop with the process"
        ),
    }
    outcome
}

/// The trigger the HTTP server stops on and `main` waits on: `signal`, then
/// [`drain`]. It resolves only once the process may exit.
///
/// Built here rather than in `main` so the tests run the composition production
/// runs. Shared because two tasks wait on it (the server's graceful shutdown and
/// `main`) and the drain must run once.
pub fn shutdown_trigger(
    signal: impl Future<Output = ()> + Send + 'static,
    readiness: GrpcReadiness,
    cancels: CancelRegistry,
    window: Duration,
) -> Shared<impl Future<Output = ()>> {
    async move {
        signal.await;
        drain(&readiness, &cancels, window).await;
    }
    .shared()
}

/// Resolves `bound` after `shutdown` has. Once the drain is over the HTTP server
/// stops accepting and waits for the connections still open, so a streamed
/// answer flushes its last frames and the client sees its `done`; normally the
/// server returns within milliseconds and wins the race in `main`. This is the
/// bound for the connection that never closes: it must not hold the exit.
pub async fn stop_after(shutdown: impl Future<Output = ()>, bound: Duration) {
    shutdown.await;
    tokio::time::sleep(bound).await;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;

    const POLL: Duration = Duration::from_millis(50);

    #[test]
    fn window_defaults_when_unset_or_blank_and_zero_is_a_real_value() {
        assert_eq!(parse_drain_window(None), Duration::from_secs(120));
        assert_eq!(parse_drain_window(Some("")), Duration::from_secs(120));
        assert_eq!(parse_drain_window(Some("   ")), Duration::from_secs(120));
        assert_eq!(parse_drain_window(Some("45")), Duration::from_secs(45));
        assert_eq!(parse_drain_window(Some(" 90 ")), Duration::from_secs(90));
        assert_eq!(
            parse_drain_window(Some("0")),
            Duration::ZERO,
            "0 is the explicit way back to the old immediate exit"
        );
    }

    #[test]
    fn window_ignores_garbage_and_clamps_an_absurd_value() {
        for bad in ["soon", "-5", "1.5", "120s", "0x10"] {
            assert_eq!(
                parse_drain_window(Some(bad)),
                Duration::from_secs(120),
                "{bad:?} is not a whole number of seconds"
            );
        }
        assert_eq!(
            parse_drain_window(Some("999999")),
            Duration::from_secs(MAX_DRAIN_SECS)
        );
        assert_eq!(
            parse_drain_window(Some("3600")),
            Duration::from_secs(3_600),
            "the ceiling itself is allowed"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_idle_process_returns_at_once_without_sleeping() {
        let before = Instant::now();
        let outcome = wait_until_idle(|| 0, Duration::from_secs(120), POLL).await;
        assert_eq!(outcome, DrainOutcome::Idle);
        assert_eq!(
            before.elapsed(),
            Duration::ZERO,
            "nothing to wait for, so the waiter never sleeps"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn it_returns_as_soon_as_the_last_stream_finishes() {
        let fake = Arc::new(AtomicUsize::new(2));
        let finishing = fake.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            finishing.store(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_secs(4)).await;
            finishing.store(0, Ordering::SeqCst);
        });

        let before = Instant::now();
        let outcome = wait_until_idle(
            {
                let fake = fake.clone();
                move || fake.load(Ordering::SeqCst)
            },
            Duration::from_secs(120),
            POLL,
        )
        .await;

        assert_eq!(outcome, DrainOutcome::Idle);
        let waited = before.elapsed();
        assert!(
            waited >= Duration::from_secs(7) && waited < Duration::from_secs(8),
            "returned when the last stream ended (7 s), not at the deadline: {waited:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn it_gives_up_at_the_deadline_and_says_how_many_were_left() {
        let fake = Arc::new(AtomicUsize::new(3));
        let before = Instant::now();
        let outcome = wait_until_idle(
            {
                let fake = fake.clone();
                move || fake.load(Ordering::SeqCst)
            },
            Duration::from_secs(10),
            // A poll interval that does not divide the window: the last sleep
            // must be cut short so the deadline is not overshot.
            Duration::from_millis(300),
        )
        .await;

        assert_eq!(outcome, DrainOutcome::TimedOut { still_active: 3 });
        assert_eq!(
            before.elapsed(),
            Duration::from_secs(10),
            "waits exactly the window, never past it"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_zero_window_with_streams_running_returns_without_waiting() {
        let before = Instant::now();
        let outcome = wait_until_idle(|| 4, Duration::ZERO, POLL).await;
        assert_eq!(outcome, DrainOutcome::TimedOut { still_active: 4 });
        assert_eq!(before.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn drain_raises_the_flag_before_it_waits_on_the_real_registry() {
        let readiness = GrpcReadiness::new();
        readiness.mark_bound();
        let cancels = CancelRegistry::new();
        let _flag = cancels.register("req-in-flight", "org-a", "user-a");
        assert!(!readiness.is_draining(), "not draining until asked to");

        let finisher = cancels.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            finisher.finish("req-in-flight");
        });

        let waiting = tokio::spawn({
            let readiness = readiness.clone();
            let cancels = cancels.clone();
            async move { drain(&readiness, &cancels, Duration::from_secs(120)).await }
        });
        // Let the drain task run up to its first sleep.
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(
            readiness.is_draining(),
            "the instance is out of rotation while it is still waiting"
        );
        assert!(!waiting.is_finished(), "a stream is still running");

        assert_eq!(waiting.await.unwrap(), DrainOutcome::Idle);
        assert_eq!(cancels.active(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn drain_with_a_stream_that_never_ends_stops_at_the_window() {
        let readiness = GrpcReadiness::new();
        let cancels = CancelRegistry::new();
        let _flag = cancels.register("req-stuck", "org-a", "user-a");

        let before = Instant::now();
        let outcome = drain(&readiness, &cancels, Duration::from_secs(120)).await;

        assert_eq!(outcome, DrainOutcome::TimedOut { still_active: 1 });
        assert_eq!(before.elapsed(), Duration::from_secs(120));
        assert!(readiness.is_draining());
    }

    #[tokio::test(start_paused = true)]
    async fn the_trigger_does_nothing_until_the_signal_then_drains_once_for_every_waiter() {
        let readiness = GrpcReadiness::new();
        let cancels = CancelRegistry::new();
        let _flag = cancels.register("req-in-flight", "org-a", "user-a");
        let (signal, signalled) = tokio::sync::oneshot::channel::<()>();
        let trigger = shutdown_trigger(
            async move {
                let _ = signalled.await;
            },
            readiness.clone(),
            cancels.clone(),
            Duration::from_secs(30),
        );
        // The HTTP server's graceful shutdown and `main` both wait on it.
        let server_side = tokio::spawn(trigger.clone());
        let main_side = tokio::spawn(trigger);

        tokio::time::sleep(Duration::from_secs(10)).await;
        assert!(!readiness.is_draining(), "no signal, no drain");

        signal.send(()).unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(readiness.is_draining(), "the signal starts the drain");
        assert!(
            !server_side.is_finished() && !main_side.is_finished(),
            "neither waiter is released while a stream is running"
        );

        cancels.finish("req-in-flight");
        server_side.await.unwrap();
        main_side.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn stop_after_adds_the_bound_once_the_trigger_has_resolved() {
        let before = Instant::now();
        stop_after(
            async { tokio::time::sleep(Duration::from_secs(7)).await },
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(
            before.elapsed(),
            Duration::from_secs(12),
            "the bound starts when the drain ends, not at the signal"
        );
    }
}
