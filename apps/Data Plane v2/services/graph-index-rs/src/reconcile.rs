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
//! `GraphExtractor::extract` takes `(text, org_id, zdr)`, all three of which are
//! in Postgres. So envelopes keep authenticating the *event path* at a short
//! TTL, and recovery reads the database and never needs an envelope at all.
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

use crate::extractor::GraphExtractor;
use crate::neo4j::Neo4jClient;
use crate::store::GraphStore;

/// ZDR classifications whose content may be sent to the extraction model.
/// Anything else — or a missing row/classification — fails closed. Mirrors
/// `embedding-engine-rs::media_consumer::caption_allowed_for_document`, which is
/// the established idiom for this decision in the plane.
const EGRESS_ALLOWED_CLASSIFICATIONS: [&str; 3] = ["internal", "public", "sensitive"];

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
            // a document that already has graph rows is never re-selected. Set
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

/// Documents eligible for re-extraction: org-visible, live, chunked, and with no
/// `graph_text_units` rows yet.
///
/// `visibility = 'org'` is not an optimisation — it is the same authority filter
/// `load_org_visible_chunks` applies, restated here so the candidate scan cannot
/// select a document whose chunks would then be refused. Graph entities are
/// org-shared, so extracting a private document would leak its content org-wide
/// through the graph arm.
///
/// A document that legitimately yields no entities has no `graph_text_units`
/// rows, so this scan alone would re-select it forever. Measured on the eval
/// corpus: 9 such documents, 63 chunks — **63 inference calls every tick, about
/// 18,000 a day, indefinitely**. `Suppressed` (below) is what bounds that;
/// without it this loop is a cost leak, not a reconciler.
///
/// `ORDER BY d.document_id` keeps the scan deterministic so a document that
/// keeps failing cannot starve later ones within a batch.
const CANDIDATES_SQL: &str = "SELECT d.document_id, d.zdr_classification
     FROM documents d
     WHERE d.org_id = $1
       AND d.visibility = 'org'
       AND d.deleted_at IS NULL
       AND EXISTS (
             SELECT 1 FROM knowledge_units ku
              WHERE ku.document_id = d.document_id AND ku.org_id = d.org_id)
       AND NOT EXISTS (
             SELECT 1
               FROM graph_text_units g
               JOIN knowledge_units ku2 ON ku2.knowledge_id = g.knowledge_id
              WHERE ku2.document_id = d.document_id AND ku2.org_id = d.org_id)
     ORDER BY d.document_id
     LIMIT $2";

/// Orgs that have any org-visible document, so the loop does not need a caller
/// to tell it which tenants exist.
const ORGS_SQL: &str = "SELECT DISTINCT org_id FROM documents
     WHERE visibility = 'org' AND deleted_at IS NULL";

/// Documents this process has already extracted to completion with **no rows to
/// show for it** — content that genuinely holds no entities.
///
/// Needed because "has no `graph_text_units` rows" is the only durable signal of
/// "not yet extracted", and it cannot distinguish *never tried* from *tried,
/// nothing there*. Without this the loop re-extracts every empty document every
/// tick forever (measured: 63 inference calls per tick on the eval corpus).
///
/// Deliberately in-memory rather than a new column:
///
/// * A restart clears it, so every suppressed document gets exactly one more
///   attempt per process lifetime. That is a feature — it is how a document
///   suppressed after a transient model outage recovers, without an operator
///   knowing to intervene.
/// * The alternative is an attempt-count column on `documents`, a table this
///   service does not own. A migration for a cost optimisation is the wrong
///   trade when a process-lifetime set gets the same steady-state behaviour.
///
/// Only a CLEAN empty result suppresses. If any chunk errored, the document is
/// left eligible: a rate-limited or failing model must not be able to
/// permanently silence a document that does have entities.
type Suppressed = std::collections::HashSet<(String, String)>;

fn egress_allowed(classification: Option<&str>) -> bool {
    match classification {
        Some(c) => EGRESS_ALLOWED_CLASSIFICATIONS.contains(&c.trim().to_ascii_lowercase().as_str()),
        // Fail closed: a document with no classification is not assumed safe to
        // send to the extraction model.
        None => false,
    }
}

/// Extract one org's eligible documents. Returns how many produced graph rows.
pub async fn run_once(
    pool: &PgPool,
    store: &GraphStore,
    extractor: &GraphExtractor,
    neo4j: Option<&Arc<Neo4jClient>>,
    org_id: &str,
    batch_size: i64,
    suppressed: &mut Suppressed,
) -> anyhow::Result<usize> {
    // Over-fetch, then drop the already-known-empty documents, so suppression
    // does not shrink the effective batch: otherwise a handful of empty
    // documents at the front of the deterministic ordering would consume every
    // slot and starve real work behind them.
    let candidates: Vec<(String, Option<String>)> = sqlx::query_as(CANDIDATES_SQL)
        .bind(org_id)
        .bind(batch_size.saturating_mul(8).min(400))
        .fetch_all(pool)
        .await?;
    let candidates: Vec<(String, Option<String>)> = candidates
        .into_iter()
        .filter(|(doc, _)| !suppressed.contains(&(org_id.to_string(), doc.clone())))
        .take(batch_size as usize)
        .collect();
    if candidates.is_empty() {
        return Ok(0);
    }

    let mut healed = 0usize;
    for (document_id, classification) in candidates {
        if !egress_allowed(classification.as_deref()) {
            tracing::info!(
                document_id = %document_id,
                org_id,
                classification = %classification.as_deref().unwrap_or("<none>"),
                "graph reconcile skipped: restricted document must not egress to the extraction model"
            );
            continue;
        }

        let chunks = store.load_org_visible_chunks(org_id, &document_id).await?;
        if chunks.is_empty() {
            // The candidate scan and this loader disagree only if the document
            // changed underneath us (deleted, or visibility revoked) between the
            // two reads. Skip rather than treat it as an extraction failure.
            continue;
        }

        // ZDR is `false` here by construction: `egress_allowed` already refused
        // every classification that must not reach the model, so this path never
        // carries restricted content. Passing the flag through keeps the
        // extractor's own contract explicit rather than implied.
        let mut produced = 0usize;
        let mut errored = false;
        for (knowledge_id, text) in &chunks {
            match extractor.extract(text, org_id, false).await {
                Ok(result) => match store
                    .persist_extraction(org_id, knowledge_id, &result)
                    .await
                {
                    Ok(persisted) => {
                        if let Err(e) = store
                            .persist_text_unit_mappings(
                                org_id,
                                knowledge_id,
                                &persisted.entity_ids,
                                &persisted.rel_ids,
                                &persisted.claim_ids,
                            )
                            .await
                        {
                            tracing::error!(err = %e, knowledge_id, "graph reconcile: persist mappings failed");
                            continue;
                        }
                        // Best-effort Neo4j mirror, exactly as the stream path
                        // treats it: Postgres is canonical and the read-model is
                        // rebuildable, so a mirror failure must not fail the heal.
                        if let Some(neo4j) = neo4j {
                            if let Err(e) = neo4j
                                .merge_extraction(
                                    org_id,
                                    &persisted.mirror_entities,
                                    &persisted.mirror_relationships,
                                )
                                .await
                            {
                                tracing::warn!(err = %e, knowledge_id, "graph reconcile: neo4j mirror failed (non-fatal)");
                            }
                        }
                        produced += persisted.entity_ids.len()
                            + persisted.rel_ids.len()
                            + persisted.claim_ids.len();
                    }
                    Err(e) => {
                        errored = true;
                        tracing::error!(err = %e, knowledge_id, "graph reconcile: persist extraction failed")
                    }
                },
                Err(e) => {
                    errored = true;
                    tracing::warn!(err = %e, knowledge_id, "graph reconcile: extraction failed")
                }
            }
        }

        match classify_outcome(produced, errored) {
            DocumentOutcome::Healed => {
                healed += 1;
                tracing::info!(
                    document_id = %document_id,
                    org_id,
                    chunks = chunks.len(),
                    produced,
                    "graph reconcile: document healed"
                );
            }
            DocumentOutcome::Failed => {
                // Stay eligible: a rate-limited or failing model must not be able
                // to permanently silence a document that does have entities.
                tracing::info!(
                    document_id = %document_id,
                    org_id,
                    chunks = chunks.len(),
                    "graph reconcile: extraction failed for this document; stays eligible for retry"
                );
            }
            DocumentOutcome::EmptyClean => {
                // Clean pass, nothing found. Suppress for this process lifetime
                // so the loop stops re-paying for it every tick; a restart grants
                // one more attempt. See `Suppressed`.
                suppressed.insert((org_id.to_string(), document_id.clone()));
                tracing::info!(
                    document_id = %document_id,
                    org_id,
                    chunks = chunks.len(),
                    suppressed = suppressed.len(),
                    "graph reconcile: extraction completed with no entities; suppressed until restart"
                );
            }
        }
    }
    Ok(healed)
}

/// What one document's extraction attempt amounted to.
#[derive(Debug, PartialEq, Eq)]
enum DocumentOutcome {
    /// Produced graph rows.
    Healed,
    /// At least one chunk errored, so "no rows" is not evidence of "no entities".
    Failed,
    /// Every chunk succeeded and there was genuinely nothing to extract.
    EmptyClean,
}

/// Decide a document's outcome from its chunk results.
///
/// A pure function on purpose. The suppression decision is the one piece of this
/// module whose logic can be wrong without any test noticing — an earlier
/// revision threaded the `Suppressed` set through every signature but never
/// called `insert`, so suppression was a silent no-op while the type-level tests
/// all passed and the deployed loop kept re-paying for empty documents. Keeping
/// the decision out of the async DB path is what makes it directly testable.
///
/// The `errored` guard is the important half: without it a provider outage would
/// mark every document "empty" and suppress the entire corpus.
fn classify_outcome(produced: usize, errored: bool) -> DocumentOutcome {
    match (produced, errored) {
        (p, _) if p > 0 => DocumentOutcome::Healed,
        (_, true) => DocumentOutcome::Failed,
        (_, false) => DocumentOutcome::EmptyClean,
    }
}

/// Poll every org for unextracted documents, forever.
pub async fn run(
    pool: PgPool,
    store: Arc<GraphStore>,
    extractor: Arc<GraphExtractor>,
    neo4j: Option<Arc<Neo4jClient>>,
    config: ReconcileConfig,
) -> anyhow::Result<()> {
    if !config.enabled {
        tracing::warn!("graph extraction reconciler disabled by GRAPH_RECONCILE_ENABLED");
        return std::future::pending().await;
    }
    tracing::info!(
        interval_secs = config.interval.as_secs(),
        batch_size = config.batch_size,
        "graph extraction reconciler online (heals documents the envelope TTL dropped)"
    );

    let mut suppressed: Suppressed = Suppressed::new();

    loop {
        // Sleep FIRST: at startup the stream consumer is draining its own
        // backlog, and racing it would double-extract the same documents.
        tokio::time::sleep(config.interval).await;

        match sqlx::query_scalar::<_, String>(ORGS_SQL)
            .fetch_all(&pool)
            .await
        {
            Ok(orgs) => {
                let mut total = 0usize;
                for org_id in orgs {
                    match run_once(
                        &pool,
                        store.as_ref(),
                        extractor.as_ref(),
                        neo4j.as_ref(),
                        &org_id,
                        config.batch_size,
                        &mut suppressed,
                    )
                    .await
                    {
                        Ok(n) => total += n,
                        Err(error) => {
                            tracing::warn!(%error, org_id = %org_id, "graph reconcile tick failed for org")
                        }
                    }
                }
                if total > 0 {
                    tracing::info!(healed = total, "graph reconciliation tick");
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
    fn restricted_and_unclassified_documents_never_reach_the_model() {
        // Allowed: the same three classifications the media/caption path allows.
        for c in ["internal", "public", "sensitive", "INTERNAL", " internal "] {
            assert!(egress_allowed(Some(c)), "{c} should be allowed");
        }
        // Refused, including the fail-closed cases.
        for c in ["restricted", "zdr", "confidential", "", "unknown"] {
            assert!(!egress_allowed(Some(c)), "{c} must be refused");
        }
        assert!(
            !egress_allowed(None),
            "a document with no classification must fail closed"
        );
    }

    #[test]
    fn candidate_scan_restates_the_visibility_authority_filter() {
        // The scan must not be able to select a document whose chunks
        // `load_org_visible_chunks` would then refuse — graph entities are
        // org-shared, so a private document must never be extracted.
        assert!(CANDIDATES_SQL.contains("d.visibility = 'org'"));
        assert!(CANDIDATES_SQL.contains("d.deleted_at IS NULL"));
        assert!(ORGS_SQL.contains("visibility = 'org'"));
        assert!(ORGS_SQL.contains("deleted_at IS NULL"));
        // And it must be org-bound, never a cross-tenant scan.
        assert!(CANDIDATES_SQL.contains("d.org_id = $1"));
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

    /// Only a CLEAN empty result may suppress — the guard that stops a provider
    /// outage from silencing the whole corpus.
    #[test]
    fn only_a_clean_empty_result_suppresses() {
        assert_eq!(classify_outcome(0, false), DocumentOutcome::EmptyClean);
        assert_eq!(
            classify_outcome(0, true),
            DocumentOutcome::Failed,
            "a failed chunk means 'no rows' is not evidence of 'no entities'"
        );
        // Rows found always wins, even if some chunk also errored: partial
        // success is real progress and the document is no longer a candidate.
        assert_eq!(classify_outcome(7, false), DocumentOutcome::Healed);
        assert_eq!(classify_outcome(7, true), DocumentOutcome::Healed);
    }

    /// An outage must not suppress anything, at any scale.
    #[test]
    fn a_total_provider_outage_suppresses_nothing() {
        let outcomes: Vec<DocumentOutcome> = (0..50).map(|_| classify_outcome(0, true)).collect();
        assert!(
            outcomes.iter().all(|o| *o == DocumentOutcome::Failed),
            "every document must stay eligible while the model is failing"
        );
    }

    /// Suppression must key on (org, document) so one tenant's empty document
    /// cannot silence the same document id in another tenant.
    #[test]
    fn suppression_is_tenant_scoped() {
        let mut s: Suppressed = Suppressed::new();
        s.insert(("org-a".into(), "doc-1".into()));
        assert!(s.contains(&("org-a".to_string(), "doc-1".to_string())));
        assert!(
            !s.contains(&("org-b".to_string(), "doc-1".to_string())),
            "another tenant's identically-named document must stay eligible"
        );
    }

    /// The over-fetch exists so suppressed documents cannot consume the batch.
    ///
    /// Without it, `LIMIT batch_size` returns the same empty documents at the
    /// front of the deterministic ordering every tick, they all get filtered,
    /// and real work behind them never runs — the loop would look busy and heal
    /// nothing.
    #[test]
    fn suppressed_documents_do_not_starve_the_batch() {
        let batch: i64 = 5;
        let fetched = batch.saturating_mul(8).min(400);
        assert_eq!(fetched, 40, "must over-fetch beyond the batch");

        // 9 suppressed (the measured eval-corpus count) then real candidates.
        let mut suppressed: Suppressed = Suppressed::new();
        let mut candidates: Vec<String> = Vec::new();
        for i in 0..9 {
            let doc = format!("empty-{i}");
            suppressed.insert(("org".into(), doc.clone()));
            candidates.push(doc);
        }
        for i in 0..10 {
            candidates.push(format!("real-{i}"));
        }

        let selected: Vec<String> = candidates
            .into_iter()
            .take(fetched as usize)
            .filter(|d| !suppressed.contains(&("org".to_string(), d.clone())))
            .take(batch as usize)
            .collect();

        assert_eq!(selected.len(), batch as usize, "batch must still be filled");
        assert!(
            selected.iter().all(|d| d.starts_with("real-")),
            "only real work selected, got {selected:?}"
        );
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
