//! Service-agnostic harness functions.
//!
//! Each harness takes a user-supplied async operation, drives it repeatedly or
//! observes its events, and returns a `Recorder` of timed samples.
//!
//! The caller then evaluates one or more `Slo`s against the recorder's sorted
//! durations. This keeps the harness independent of any particular service
//! crate and avoids circular workspace deps.

use std::future::Future;
use std::time::{Duration, Instant};

use crate::recorder::Recorder;

/// Measure **first-token latency** for a streaming operation.
///
/// `op(i)` must return a future that yields something (i.e. the *first event*)
/// of an async stream. The harness records time-to-first-event, **not** total
/// stream completion time.
///
/// # Errors
///
/// Propagates the first error from `op` via the returned `Result`. The recorder
/// up to that point is returned via the `recorder` out-parameter pattern — not
/// used here to keep the API minimal. Callers typically retry on error.
pub async fn first_token_latency<F, Fut, T>(
    iterations: usize,
    mut op: F,
) -> Result<Recorder, Box<dyn std::error::Error + Send + Sync>>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
{
    let mut recorder = Recorder::new();
    for i in 0..iterations {
        let start = Instant::now();
        let _first = op(i).await?;
        recorder.push(format!("req-{i}"), start.elapsed());
    }
    Ok(recorder)
}

/// Measure **inter-step interval** for a step-execution loop.
///
/// Record every call to `step()`; report inter-arrival durations. An SLO of
/// "throughput > 10 steps/s" translates to "p99 interval < 100 ms".
///
/// # Errors
///
/// Propagates the first error from `step`.
pub async fn step_throughput<F, Fut>(
    iterations: usize,
    mut step: F,
) -> Result<Recorder, Box<dyn std::error::Error + Send + Sync>>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>>,
{
    let mut recorder = Recorder::new();
    let mut last = Instant::now();
    for i in 0..iterations {
        step(i).await?;
        let now = Instant::now();
        recorder.push(format!("step-{i}"), now.duration_since(last));
        last = now;
    }
    Ok(recorder)
}

/// Measure **checkpoint recovery** latency by timing a single restore invocation.
///
/// Returns the observed recovery duration in milliseconds.
///
/// # Errors
///
/// Propagates any error from `restore`.
pub async fn checkpoint_recovery_ms<F, Fut>(
    restore: F,
) -> Result<Duration, Box<dyn std::error::Error + Send + Sync>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>>,
{
    let start = Instant::now();
    restore().await?;
    Ok(start.elapsed())
}

/// Measure **context-assembly** latency per call.
///
/// `assemble(i)` runs one assembly; the harness times each call and records the
/// resulting token count via `token_count_of` so the caller can assert on both
/// latency (SLO) and token-budget headroom.
///
/// Returns `(latency_recorder, token_counts)`.
///
/// # Errors
///
/// Propagates the first error from `assemble`.
pub async fn context_assembly<F, Fut>(
    iterations: usize,
    mut assemble: F,
) -> Result<(Recorder, Vec<usize>), Box<dyn std::error::Error + Send + Sync>>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Result<usize, Box<dyn std::error::Error + Send + Sync>>>,
{
    let mut recorder = Recorder::new();
    let mut tokens = Vec::with_capacity(iterations);
    for i in 0..iterations {
        let start = Instant::now();
        let n = assemble(i).await?;
        recorder.push(format!("assemble-{i}"), start.elapsed());
        tokens.push(n);
    }
    Ok((recorder, tokens))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defaults;

    type BoxedErr = Box<dyn std::error::Error + Send + Sync>;

    #[tokio::test]
    async fn first_token_meets_streaming_slo() {
        let rec = first_token_latency(20, |_| async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            Ok::<_, BoxedErr>(())
        })
        .await
        .unwrap();

        let sorted = rec.sorted_durations();
        let verdict = defaults::streaming_first_token().evaluate(&sorted);
        assert!(!verdict.breached, "{}", verdict.summary());
    }

    #[tokio::test]
    async fn step_throughput_above_target() {
        // 20 zero-work steps; intervals should be well under 100ms p99.
        let rec = step_throughput(20, |_| async { Ok::<_, BoxedErr>(()) })
            .await
            .unwrap();
        let sorted = rec.sorted_durations();
        let verdict = defaults::step_throughput_p99_interval().evaluate(&sorted);
        assert!(!verdict.breached, "{}", verdict.summary());
    }

    #[tokio::test]
    async fn checkpoint_recovery_meets_slo() {
        let elapsed = checkpoint_recovery_ms(|| async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, BoxedErr>(())
        })
        .await
        .unwrap();

        let slo = defaults::checkpoint_recovery();
        assert!(
            elapsed < slo.target,
            "recovery {elapsed:?} ≥ target {:?}",
            slo.target
        );
    }

    #[tokio::test]
    async fn context_assembly_within_budget() {
        let (rec, tokens) = context_assembly(10, |_| async {
            tokio::time::sleep(Duration::from_millis(2)).await;
            Ok::<_, BoxedErr>(1024usize)
        })
        .await
        .unwrap();

        let verdict = defaults::context_assembly_latency().evaluate(&rec.sorted_durations());
        assert!(!verdict.breached, "{}", verdict.summary());
        assert!(
            tokens.iter().all(|&t| t <= 128 * 1024),
            "token budget breach"
        );
    }

    #[tokio::test]
    async fn slo_surfaces_synthetic_breach() {
        // Deliberately slow: 250ms per op; default p95 target is 200ms ⇒ breach.
        let rec = first_token_latency(5, |_| async {
            tokio::time::sleep(Duration::from_millis(250)).await;
            Ok::<_, BoxedErr>(())
        })
        .await
        .unwrap();
        let verdict = defaults::streaming_first_token().evaluate(&rec.sorted_durations());
        assert!(
            verdict.breached,
            "expected breach; got {}",
            verdict.summary()
        );
    }
}
