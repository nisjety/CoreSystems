//! SLO integration tests for execution-core.
//!
//! Wires the `mp-slo` harness against the real `runtime_loop::execute_step`
//! entrypoint and `crate::scrub` helpers to close two PR-8 gates:
//!
//!   - "Step throughput > 10 steps/second per run" (target p99 inter-step < 100 ms)
//!   - "Checkpoint recovery < 5 s" (target max recovery duration < 5 s)
//!
//! The third PR-8 gate (context assembly within token budget) lives in
//! `session-core/tests/slo.rs` because the pure `assemble_segments` function is
//! owned by that service.
//!
//! Design notes:
//!   - Tests run in-process against pure in-crate code — no DB, no gRPC, no
//!     network. `runtime_loop::execute_step` is sync and pure once the tool
//!     bridge fake returns; `scrub::scrub_json_value` is pure.
//!   - Scope is the **runtime loop itself**, not end-to-end latency including
//!     Postgres + session-core RPC. That end-to-end number is a separate SLO
//!     that depends on infra availability.

use std::time::{Duration, Instant};

use mp_slo::{defaults, harness};

use execution_core::runtime_loop;
use execution_core::scrub;

type BoxedErr = Box<dyn std::error::Error + Send + Sync>;

/// Minimal permission mode + hook context combo that steers the runtime loop
/// through its Allow → tool_bridge path without external dependencies.
fn bench_step() -> runtime_loop::StepOutcome {
    runtime_loop::execute_step(
        "noop", // unrecognized tool name → tool_bridge returns a default stub output
        "payload",
        "permissive",
        "", // empty hook context → not blocked
    )
}

// ---------------------------------------------------------------------------
// Gate: step throughput > 10 steps/second
// ---------------------------------------------------------------------------

/// Runs 200 steps through `runtime_loop::execute_step`; evaluates the p99
/// inter-step interval against the default throughput SLO (< 100 ms ⇒ > 10/s).
///
/// A breach here means either the runtime loop has regressed or the tool bridge
/// is unexpectedly slow for the default fixture.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn step_throughput_meets_slo() {
    let iterations = 200usize;

    let recorder = harness::step_throughput(iterations, |_| async {
        // Keep the work on-thread. Harness records per-call elapsed.
        let outcome = bench_step();
        // Light sanity — if the tool bridge started returning errors, surface
        // it as a harness failure rather than silently counting broken steps.
        if outcome.status == "failed" && !outcome.error.is_empty() {
            return Err::<(), BoxedErr>(outcome.error.into());
        }
        Ok(())
    })
    .await
    .expect("step harness ran");

    let sorted = recorder.sorted_durations();
    let verdict = defaults::step_throughput_p99_interval().evaluate(&sorted);
    assert!(!verdict.breached, "{}", verdict.summary());
    assert_eq!(verdict.sample_count, iterations, "all samples recorded");
}

// ---------------------------------------------------------------------------
// Gate: checkpoint recovery < 5 s
// ---------------------------------------------------------------------------

/// Fabricates a realistic checkpoint payload of a few hundred steps, serializes
/// it, scrubs it (the canonical pre-persist pass), and deserializes it back.
/// This mirrors the hot path a Temporal activity performs on restart.
///
/// The purpose is to prove the **CPU-bound** side of recovery stays within
/// budget; the network-bound half (Postgres fetch) is covered by a separate
/// SLO harness owned by session-core once that side is wired.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn checkpoint_recovery_cpu_path_meets_slo() {
    let elapsed = harness::checkpoint_recovery_ms(|| async {
        // Build a synthetic checkpoint that mirrors the shape emitted by
        // `grpc.rs::execute_step` — run-level state plus a step sub-object.
        // 512 steps is a generous fixture for a long-running run.
        let mut steps = Vec::with_capacity(512);
        for i in 0..512u32 {
            steps.push(serde_json::json!({
                "id": format!("step-{i:06}"),
                "status": "completed",
                "output": format!("step {} completed; secret=not-really api_key=sk-test", i),
                "error": "",
                "compaction_triggered": i % 128 == 127,
            }));
        }
        let mut value = serde_json::json!({
            "run_id": "run-bench-recovery",
            "step_index": 512,
            "status": "completed",
            "last_error": null,
            "steps": steps,
        });

        // Pre-persist scrub (mirrors `grpc.rs::execute_step` line 83).
        scrub::scrub_json_value(&mut value);

        // Serialize → deserialize round-trip; this is the CPU cost of replay.
        let bytes = serde_json::to_vec(&value)
            .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e.to_string()))?;
        let _restored: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e.to_string()))?;
        Ok::<(), BoxedErr>(())
    })
    .await
    .expect("recovery harness ran");

    let slo = defaults::checkpoint_recovery();
    assert!(
        elapsed < slo.target,
        "{}: observed {:?} target {:?}",
        slo.name,
        elapsed,
        slo.target
    );
}

// ---------------------------------------------------------------------------
// Sanity: a deliberately slow step surfaces as an SLO breach.
// ---------------------------------------------------------------------------

/// Regression guard: if someone accidentally removes the breach detection
/// (e.g. by inverting the comparison), this test would start passing silently.
/// Forcing a breach and asserting on it keeps the SLO gate honest.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn synthetic_slow_step_surfaces_breach() {
    let recorder = harness::step_throughput(5, |_| async {
        tokio::time::sleep(Duration::from_millis(150)).await; // > 100 ms target
        Ok::<(), BoxedErr>(())
    })
    .await
    .unwrap();

    let verdict = defaults::step_throughput_p99_interval().evaluate(&recorder.sorted_durations());
    assert!(verdict.breached, "expected breach: {}", verdict.summary());
}

// ---------------------------------------------------------------------------
// Smoke: wall-clock timing sanity around Instant usage inside the harness.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wall_clock_consistent_with_recorder() {
    let iterations = 10;
    let overall_start = Instant::now();

    let rec = harness::step_throughput(iterations, |_| async {
        let _ = bench_step();
        Ok::<(), BoxedErr>(())
    })
    .await
    .unwrap();
    let overall = overall_start.elapsed();

    // Sum of intervals should not exceed wall-clock elapsed by more than the
    // first-iteration's warm-up + scheduling slack.
    let sum_intervals: Duration = rec.samples().iter().map(|s| s.duration).sum();
    assert!(
        sum_intervals <= overall + Duration::from_millis(50),
        "intervals {sum_intervals:?} exceed wall-clock {overall:?}"
    );
}
