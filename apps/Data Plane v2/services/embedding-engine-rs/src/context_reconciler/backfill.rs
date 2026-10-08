//! The operator's backfill: `context-backfill`'s dry run and its capped run (CR-T9a).
//!
//! The same passes as the service's, over one organization, with the caps the operator gave. It
//! is a way of asking the reconciler's own code, not a second implementation: a run here and the
//! service's loop lease the same units the same way (so one can run beside the other), run the
//! same pipeline, and write the same states.
//!
//! ## The caps
//!
//! `--max-units` is exact: no pass leases past it. `--max-usd` is held to an ESTIMATE, the dry
//! run's own arithmetic (see `estimate`): before each pass the units a generation would be asked
//! for are priced, the pass is cut to what fits in what is left, and every unit it leased for
//! generation is counted as spent whether or not the provider answered it, because a call that
//! failed was still a call. The estimate counts the answer at its cap, so the figure over-states
//! the bill and never under-states it; the re-embeds (the legacy verification, the repairs) cost no
//! generation and are not held to it. The embedding price is not in the repository, so unless the
//! operator gives one the cap is on generation spend only, and the report says so.

use std::collections::BTreeMap;

use anyhow::Context as _;
use sqlx::PgPool;

use super::estimate::{self, Prices};
use super::{pacing, select, Paused, Reconciler, Scope};
use crate::context::PromptVersion;

/// What the operator asked `context-backfill` to do.
#[derive(Clone, Debug)]
pub(crate) struct Backfill {
    pub org_id: String,
    pub max_units: Option<u64>,
    pub max_usd: Option<f64>,
    /// The prompt version a refresh moves to: it is also the version this run generates with
    /// (the caller sets it as the service's version for the run), so what is selected and what is
    /// generated agree. It makes the refresh's units count in the estimate.
    pub refresh_target: Option<PromptVersion>,
    pub prices: Prices,
}

/// What a backfill did.
#[derive(Clone, Debug, Default)]
pub(crate) struct BackfillReport {
    pub passes: usize,
    /// Units leased, by kind.
    pub leased: BTreeMap<&'static str, usize>,
    pub generated: usize,
    pub verified: usize,
    pub repaired: usize,
    pub unresolved: usize,
    pub announced: usize,
    pub given_up: i64,
    /// Why it stopped.
    pub stopped: &'static str,
    /// The estimated generation spend of the units this run asked a model about.
    pub estimated_llm_usd: f64,
}

/// The dry run: what there is to do for an organization (or all), counted and priced, one JSON
/// line each and a total. It reads counts and nothing else, so what it returns holds no content,
/// and it writes nothing: no lease, no status.
pub(crate) async fn dry_run(
    pool: &PgPool,
    max_attempts: i32,
    prompt_chars_cap: i64,
    org_id: Option<&str>,
    refresh_target: Option<PromptVersion>,
    prices: &Prices,
    max_output_tokens: i64,
) -> anyhow::Result<String> {
    let counts = select::counts(
        pool,
        org_id,
        max_attempts,
        prompt_chars_cap,
        refresh_target.map(PromptVersion::as_str),
    )
    .await
    .context("count the work")?;
    Ok(estimate::render(
        &counts,
        prices,
        max_output_tokens,
        refresh_target.map(PromptVersion::as_str),
    ))
}

impl Reconciler {
    #[cfg(test)]
    pub(crate) async fn dry_run(
        &self,
        org_id: Option<&str>,
        refresh_target: Option<PromptVersion>,
        prices: &Prices,
        max_output_tokens: i64,
    ) -> anyhow::Result<String> {
        dry_run(
            &self.deps.pool,
            self.settings.max_attempts,
            self.window_chars(),
            org_id,
            refresh_target,
            prices,
            max_output_tokens,
        )
        .await
    }

    /// Work one organization's backlog to its end, or to the operator's caps.
    pub(crate) async fn backfill(
        &self,
        request: &Backfill,
        max_output_tokens: i64,
    ) -> anyhow::Result<BackfillReport> {
        let mut report = BackfillReport::default();
        let mut units_left = request.max_units;
        loop {
            if units_left == Some(0) {
                report.stopped = "unit cap reached";
                break;
            }
            // What a unit asked of a model is expected to cost, from what is left to do.
            let remaining = self
                .remaining(request, max_output_tokens)
                .await
                .context("count the work to be done")?;
            let per_unit = if remaining.llm_calls > 0 {
                remaining.llm_usd / remaining.llm_calls as f64
            } else {
                0.0
            };
            let mut pass_cap = self.settings.units_per_pass;
            if let Some(max_usd) = request.max_usd {
                if report.estimated_llm_usd >= max_usd {
                    report.stopped = "spend cap reached";
                    break;
                }
                if per_unit > 0.0 {
                    let fits = ((max_usd - report.estimated_llm_usd) / per_unit).floor();
                    // One unit at the least, so that a cap below one unit's estimate is a run of
                    // one and not a run that never starts: the estimate is an upper bound.
                    pass_cap = pass_cap.min(fits.max(1.0) as usize);
                }
            }
            if let Some(left) = units_left {
                pass_cap = pass_cap.min(usize::try_from(left).unwrap_or(usize::MAX));
            }
            let scope = Scope {
                orgs: Some(vec![request.org_id.clone()]),
                max_units: Some(pass_cap),
                eager: true,
            };
            let pass = self.pass_scoped(&scope).await;
            report.passes += 1;
            if let Some(paused) = pass.paused {
                report.stopped = match paused {
                    Paused::File => "paused: the pause file exists",
                    Paused::Breaker(_) => "paused: the circuit breaker is open",
                };
                break;
            }
            for (kind, count) in &pass.leased {
                *report.leased.entry(kind).or_default() += count;
            }
            // Every unit leased to ask a model about is a call that was made, answered or not.
            let asked: usize = ["generate", "stale", "refresh"]
                .iter()
                .map(|kind| pass.leased.get(kind).copied().unwrap_or(0))
                .sum();
            report.estimated_llm_usd += per_unit * asked as f64;
            report.generated += pass.generated;
            report.verified += pass.verified;
            report.repaired += pass.repaired;
            report.unresolved += pass.unresolved;
            report.announced += pass.announced;
            report.given_up += pass.given_up.iter().map(|(_, _, n)| n).sum::<i64>();
            if let Some(left) = units_left.as_mut() {
                *left = left.saturating_sub(pass.leased_total() as u64);
            }
            if pass.leased_total() == 0 && pass.zero_retention_released == 0 {
                // A used-up budget in `stop` mode is a reason, and the operator is told it, not
                // "nothing left to do": the units are there, and the gate is holding them.
                report.stopped = if pass.budget_held > 0 {
                    "stopped: the organization's background budget is used up and its budget mode is stop (the units are skipped_budget until it allows generation again)"
                } else {
                    "nothing left to do"
                };
                break;
            }
            if pass.errors > 0 && pass.documents == 0 {
                report.stopped = "stopped: the pass could not run (see the log)";
                break;
            }
            // A unit that could not be made right in a pass is not asked again in the same run
            // before its cooldown ends, so a run always ends; give a throttled provider room.
            if let Some(wait) = pass.retry_after {
                tokio::time::sleep(wait.min(pacing::MAX_WAIT)).await;
            }
        }
        Ok(report)
    }

    async fn remaining(
        &self,
        request: &Backfill,
        max_output_tokens: i64,
    ) -> anyhow::Result<estimate::Estimate> {
        let counts = select::counts(
            &self.deps.pool,
            Some(&request.org_id),
            self.settings.max_attempts,
            self.window_chars(),
            request.refresh_target.map(PromptVersion::as_str),
        )
        .await?;
        Ok(counts
            .iter()
            .map(|org| {
                estimate::estimate(
                    org,
                    request.refresh_target.is_some(),
                    &request.prices,
                    max_output_tokens,
                )
            })
            .fold(estimate::Estimate::default(), |sum, next| sum.plus(&next)))
    }
}
