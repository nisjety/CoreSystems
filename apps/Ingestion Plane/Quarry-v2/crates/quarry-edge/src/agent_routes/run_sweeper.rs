use super::*;

/// What one pass of the sweeper did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SweepReport {
    /// Runs released.
    pub expired: usize,
    /// Runs it would have released (dry run).
    pub would_expire: usize,
}

/// Start the periodic sweep. It does nothing when the sweeper is switched off
/// (`QUARRY_EDGE__RUN_SWEEPER=0`). Abort the returned handle on shutdown.
pub fn spawn_run_sweeper(state: AppState) -> tokio::task::JoinHandle<()> {
    let limits = state.agent_runs.limits().clone();
    // zdr-log-ok: a periodic background sweep started at boot; no request is in flight, and its events carry run ids, classes and numbers only
    tokio::spawn(async move {
        if !limits.sweeper {
            tracing::info!("run sweeper: off");
            return;
        }
        tracing::info!(
            interval_s = limits.sweep_interval_s,
            dry_run = limits.sweeper_dry_run,
            "run sweeper: on"
        );
        let mut ticker = tokio::time::interval(Duration::from_secs(limits.sweep_interval_s));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // the first tick is immediate; nothing is old yet
        loop {
            ticker.tick().await;
            sweep_once(&state, now_ms()).await;
        }
    })
}

/// One pass at `now_ms`: release every run whose verdict says it has expired.
/// In a dry run it logs `would_expire` (run id and numbers only, never a URL) and
/// releases nothing, which is how the first deploy runs so a person's session is
/// never reaped by a number nobody has checked.
pub async fn sweep_once(state: &AppState, now_ms: u64) -> SweepReport {
    let limits = state.agent_runs.limits().clone();
    let runs = state
        .agent_runs
        .lock()
        .expect("agent_runs mutex poisoned")
        .snapshot();
    let mut report = SweepReport::default();
    for (run_id, handle) in runs {
        let Some(verdict) = handle.live.verdict(now_ms, &limits) else {
            continue;
        };
        let class = handle.live.class().label();
        if limits.sweeper_dry_run {
            report.would_expire += 1;
            tracing::warn!(
                %run_id,
                class,
                reason = verdict.reason.as_str(),
                idle_s = verdict.idle_s,
                age_s = verdict.age_s,
                "would_expire: run sweeper is in dry run"
            );
            continue;
        }
        tracing::info!(
            %run_id,
            class,
            reason = verdict.reason.as_str(),
            idle_s = verdict.idle_s,
            age_s = verdict.age_s,
            "run expired"
        );
        match finish_run(state, &run_id, handle, CloseReason::Expired(verdict)).await {
            Ok(()) => report.expired += 1,
            Err(error) => tracing::warn!(
                %run_id,
                error_class = crate::zdr_log::class(&error),
                "run sweeper could not release a run"
            ),
        }
    }
    report
}
