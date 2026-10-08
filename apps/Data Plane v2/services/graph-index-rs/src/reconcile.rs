//! Heals graph extraction that the event path could not deliver, by reading
//! database truth instead of replaying events.
//!
//! ## Why this exists rather than a re-announce job
//!
//! Graph extraction is one inference call PER CHUNK — tens of seconds per
//! document. Signed event envelopes carry a 120s TTL (300s ceiling). So the
//! queue depth times the per-document extraction time must stay under the TTL,
//! which is impossible for a consumer this slow: announce a dozen documents and
//! roughly the first four extract while the rest expire in the stream and are
//! dropped. Measured 2026-08-26: 12 announced, 2 extracted, 10 discarded.
//!
//! The three obvious fixes were each considered and rejected:
//!
//! * **Accept expired envelopes, trusting `jti` replay protection instead.**
//!   Unsound as the crate is written: the replay cache is in-memory,
//!   capacity-bounded, and pruned exactly at `exp`
//!   (`event-envelope-rs::consume_replay_id`). The replay window *is* `exp`, so
//!   they are not independent controls — honouring one without the other would
//!   require remembering every `jti` for the stream's full 7-day `max_age`,
//!   durably across restarts.
//! * **Mint a fresh envelope per chunk.** The TTL clock starts at *publish*; the
//!   expiry happens while messages wait in the stream behind slow work. Splitting
//!   one document event into N chunk events multiplies queue depth and extends no
//!   deadline.
//! * **Trust the broker's delivery timestamp.** Envelopes exist precisely so a
//!   compromised broker cannot forge events; trusting its clock reopens that.
//!
//! What actually resolves the tension: extraction needs nothing from the event.
//! `GraphExtractor::extract` takes `(text, org_id)`, both of which are
//! in Postgres. So envelopes keep authenticating the *event path* at a short
//! TTL, and recovery reads the database and never needs an envelope at all.
//!
//! ## What "not extracted yet" means (design task CR-T12)
//!
//! It used to mean "has no `graph_text_units` rows", which cannot tell a chunk nobody tried
//! from one that was tried and held nothing, nor from one that keeps failing. The loop
//! re-selected all three on every tick (measured: 63 provider calls per tick for nine documents
//! with no entities, about 18,000 a day, and one failing chunk retried at least ten times), and
//! its only memory of "tried, nothing there" was a set in the process that a restart emptied.
//! The answer now lives in `graph_extraction_state`: one row per chunk saying whether it was
//! extracted (`ready`), extracted and empty (`empty`, durable until the prompt version changes),
//! skipped and why, waiting out a backoff, or given up on after the poison cap. This module
//! only asks the `pipeline` for each organization's due work; the rules are there.
//!
//! ## Guardrail for future consumers
//!
//! Any new consumer whose per-message work is inference-bound inherits this
//! problem and must ship with a database-truth reconciler. Consumers whose work
//! is milliseconds (quickwit-adapter, meilisearch-adapter) do not: for them a
//! >120s backlog is a broker outage, not a design flaw.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;

use crate::pipeline::{self, Live, Ports};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileConfig {
    pub enabled: bool,
    /// How often to look for unextracted documents.
    pub interval: Duration,
    /// Documents attempted per tick. Small by default: each one costs an
    /// inference call per chunk, and a tick that runs for minutes would hold the
    /// loop past the next interval.
    pub batch_size: i64,
}

impl Default for ReconcileConfig {
    fn default() -> Self {
        Self {
            // On by default, same reasoning index-engine's reconciler documents:
            // a config-gated fix ships inert on a deployment whose compose file
            // and `.env` were never updated. The blast radius is bounded — at
            // most `batch_size` documents per tick, extraction is idempotent
            // (deterministic content-derived ids, `MERGE` on the shared PKs), and
            // a chunk that is extracted, empty at the current prompt version, given
            // up on or waiting out its backoff is never selected. Set
            // GRAPH_RECONCILE_ENABLED=0 to disable.
            enabled: true,
            // Deliberately slower than index-engine's 60s: a tick here can run
            // for minutes, so polling faster would only stack ticks.
            interval: Duration::from_secs(300),
            batch_size: 5,
        }
    }
}

impl ReconcileConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            enabled: !matches!(
                std::env::var("GRAPH_RECONCILE_ENABLED").as_deref(),
                Ok("0") | Ok("false")
            ),
            interval: env_secs("GRAPH_RECONCILE_INTERVAL_SECS", defaults.interval),
            batch_size: env_parse("GRAPH_RECONCILE_BATCH", defaults.batch_size).clamp(1, 100),
        }
    }
}

fn env_secs(key: &str, fallback: Duration) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(fallback)
}

fn env_parse<T: std::str::FromStr>(key: &str, fallback: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.parse::<T>().ok())
        .unwrap_or(fallback)
}

/// Orgs that have any org-visible document, so the loop does not need a caller
/// to tell it which tenants exist.
const ORGS_SQL: &str = "SELECT DISTINCT org_id FROM documents
     WHERE visibility = 'org' AND deleted_at IS NULL";

/// One tick: every organization's turn through the pipeline. Returns how many documents had
/// chunks that stored something. An organization's failure is logged and does not stop the
/// others.
pub(crate) async fn run_tick<P: Ports>(ports: &P, orgs: &[String], batch_size: i64) -> usize {
    let mut healed = 0usize;
    for org_id in orgs {
        match pipeline::run_org_pass(ports, org_id, batch_size).await {
            Ok(report) => healed += report.healed,
            Err(error) => {
                tracing::warn!(%error, org_id = %org_id, "graph reconcile tick failed for org")
            }
        }
    }
    healed
}

/// Poll every org for unextracted documents, forever.
pub async fn run(pool: PgPool, ports: Arc<Live>, config: ReconcileConfig) -> anyhow::Result<()> {
    if !config.enabled {
        tracing::warn!("graph extraction reconciler disabled by GRAPH_RECONCILE_ENABLED");
        return std::future::pending().await;
    }
    tracing::info!(
        interval_secs = config.interval.as_secs(),
        batch_size = config.batch_size,
        "graph extraction reconciler online (heals documents the envelope TTL dropped)"
    );

    loop {
        // Sleep FIRST: at startup the stream consumer is draining its own
        // backlog, and racing it would double-extract the same documents (and
        // the claim on each chunk would only make the second one a no-op).
        tokio::time::sleep(config.interval).await;

        match sqlx::query_scalar::<_, String>(ORGS_SQL)
            .fetch_all(&pool)
            .await
        {
            Ok(orgs) => {
                let healed = run_tick(ports.as_ref(), &orgs, config.batch_size).await;
                if healed > 0 {
                    tracing::info!(healed, "graph reconciliation tick");
                }
            }
            Err(error) => tracing::warn!(%error, "graph reconcile: org scan failed; retrying"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scan_of_organizations_is_the_visibility_authority_filter() {
        // The organizations to visit are the ones with a live, organization-visible document.
        // The chunk-level rules (due, visible, allowed to reach the model) are the state
        // store's own, tested there against the real schema.
        assert!(ORGS_SQL.contains("visibility = 'org'"));
        assert!(ORGS_SQL.contains("deleted_at IS NULL"));
    }

    #[test]
    fn defaults_are_slower_and_smaller_than_the_embedding_reconciler() {
        let c = ReconcileConfig::default();
        assert!(c.enabled, "must ship on, or the fix lands inert");
        // A tick can run for minutes (one inference call per chunk), so polling
        // must be slower than index-engine's 60s and batches much smaller.
        assert!(c.interval >= Duration::from_secs(300));
        assert!(c.batch_size <= 5);
    }

    /// The loop keeps no memory of its own any more: what it has done is in the database, so a
    /// restart cannot make it pay again. A test over the text is crude, and exactly as strong
    /// as the regression it guards: someone putting a set back.
    #[test]
    fn the_loop_keeps_no_memory_in_the_process() {
        let source = include_str!("reconcile.rs");
        let production: String = source
            .split("mod tests {")
            .next()
            .expect("production code")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in ["HashSet", "Suppressed", "suppressed", "graph_text_units"] {
            assert!(
                !production.contains(forbidden),
                "reconcile.rs mentions `{forbidden}`: the state of a chunk lives in graph_extraction_state"
            );
        }
    }

    #[test]
    fn batch_size_is_clamped_to_a_sane_range() {
        std::env::set_var("GRAPH_RECONCILE_BATCH", "100000");
        assert_eq!(ReconcileConfig::from_env().batch_size, 100);
        std::env::set_var("GRAPH_RECONCILE_BATCH", "0");
        assert_eq!(ReconcileConfig::from_env().batch_size, 1);
        std::env::remove_var("GRAPH_RECONCILE_BATCH");
    }
}
