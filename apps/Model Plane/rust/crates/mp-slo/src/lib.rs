//! mp-slo — Service Level Objective harness for the Model Plane.
//!
//! Closes the remaining **PR-8** performance verification gates:
//!
//! - Streaming first-token latency p95 < 200 ms (`inference-core`)
//! - Step throughput > 10 steps/second (`execution-core`)
//! - Checkpoint recovery < 5 s (`execution-core` + `orchestrator-core`)
//! - Context assembly within token budget (`session-core`)
//!
//! Design rules:
//! - **Measurement ≠ production path.** The harness wraps `Future`s / streams from
//!   the real services under test. It never owns business logic.
//! - **Thresholds are declarative.** A SLO is `Slo { name, target }` and verdicts
//!   are `Verdict { observed, target, breached }`. Thresholds live in code, not
//!   in config, so a perf regression surfaces as a typed failure, not a YAML diff.
//! - **Deterministic on small N.** Percentile computation uses nearest-rank on a
//!   sorted `Vec<Duration>` — simple, exact, good enough for N ≤ 10k samples.
//! - **No I/O in the crate.** All harnesses take an `async fn(...) -> Result<...>`
//!   supplied by the caller (integration test, bench, or service-internal probe).

#![deny(missing_docs)]

pub mod harness;
pub mod histogram;
pub mod percentile;
pub mod recorder;
pub mod slo;

pub use harness::{checkpoint_recovery_ms, context_assembly, first_token_latency, step_throughput};
pub use histogram::LatencyHistogram;
pub use percentile::Percentile;
pub use recorder::{Recorder, Sample};
pub use slo::{Slo, SloKind, Verdict};

/// Default SLO catalog matching `VERIFICATION.md` §Performance Tests.
pub mod defaults {
    use std::time::Duration;

    use crate::slo::{Slo, SloKind};

    /// Streaming: first-token p95 < 200 ms.
    #[must_use]
    pub fn streaming_first_token() -> Slo {
        Slo {
            name: "streaming.first_token.p95",
            kind: SloKind::LatencyP95,
            target: Duration::from_millis(200),
        }
    }

    /// Execution: step throughput > 10 steps/second per run (i.e. p99 interval < 100 ms).
    #[must_use]
    pub fn step_throughput_p99_interval() -> Slo {
        Slo {
            name: "execution.step.p99_interval",
            kind: SloKind::LatencyP99,
            target: Duration::from_millis(100),
        }
    }

    /// Checkpoint recovery time budget.
    #[must_use]
    pub fn checkpoint_recovery() -> Slo {
        Slo {
            name: "execution.checkpoint.recovery",
            kind: SloKind::LatencyMax,
            target: Duration::from_secs(5),
        }
    }

    /// Context assembly deadline (implementation-defined but ≤ 2s is current target).
    #[must_use]
    pub fn context_assembly_latency() -> Slo {
        Slo {
            name: "session.context_assembly.p95",
            kind: SloKind::LatencyP95,
            target: Duration::from_millis(2_000),
        }
    }
}
