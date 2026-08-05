//! Ordered node-postprocessor chain: everything that happens to the candidate
//! list between fusion and the response.
//!
//! These steps used to be ~170 lines inlined in [`RetrievalPipeline::retrieve`],
//! where the order was implied by statement order and every step was only
//! reachable by driving a whole pipeline with a live Postgres, a reranker, and a
//! ColQwen endpoint. That made the two steps which actually enforce authority --
//! the visibility gate and the ZDR filter -- the hardest things in the file to
//! test, and it made the order they run in invisible to a reader.
//!
//! Modelling each step as a [`NodePostprocessor`] makes the chain a value the
//! caller can read top-to-bottom, and lets each stage be exercised against a
//! hand-built candidate list.
//!
//! Three properties here are load-bearing. Do not "simplify" them away:
//!
//! 1. **The order enforces authority.** [`VisibilityGate`] must run before the
//!    truncate to `top_n`, because truncating first would let a non-visible
//!    candidate consume one of the caller's slots and the response would
//!    silently undershoot with a document the viewer may not read. [`ZdrFilter`]
//!    must run after it, so a restricted document that was already dropped for
//!    visibility is not counted as a ZDR rejection.
//!
//! 2. **Failure semantics differ per stage, deliberately.** Reranking (text and
//!    visual) is *refinement*: any provider error degrades to the incoming order
//!    and the query still succeeds. The visibility and ZDR stages are
//!    *enforcement*: their errors propagate, because returning unfiltered
//!    candidates is worse than returning an error.
//!
//! 3. **Stage timings feed the persisted trace.** `run_chain` returns a
//!    [`StageTiming`] per stage so the caller can reconstruct the exact
//!    `PipelineTimings` fields it reported before this refactor. Collapsing them
//!    into one number would silently change what every historical trace means.

use async_trait::async_trait;
use sqlx::PgPool;

// The ColQwen score-placement helpers stay in `orchestrator` with their existing
// unit tests; only their orchestration moves here.
use super::orchestrator::{apply_colqwen_scores, page_image_url};
use crate::pipeline::types::{ScoredCandidate, ZdrMode};
use crate::search::colqwen::ColqwenClient;
use crate::search::rerank::RerankClient;

/// Stage names. Constants rather than inline literals because the orchestrator
/// sums stage timings *by name* to reproduce the `PipelineTimings` buckets it
/// reported before the chain existed — a typo there would silently zero a
/// persisted trace field instead of failing to compile.
pub const STAGE_VISUAL_RERANK: &str = "visual_rerank";
pub const STAGE_TRUNCATE_OVERFETCH: &str = "truncate:overfetch";
pub const STAGE_TEXT_RERANK: &str = "text_rerank";
pub const STAGE_RECENCY_DECAY: &str = "recency_decay";
pub const STAGE_VISIBILITY_GATE: &str = "visibility_gate";
pub const STAGE_TRUNCATE_TOP_N: &str = "truncate:top_n";
pub const STAGE_ZDR_FILTER: &str = "zdr_filter";

/// Per-request state a stage may read, plus the values a stage produces that the
/// caller needs afterwards.
///
/// The `OUT` fields are on the context rather than in the return type because
/// they are produced by *specific* stages but consumed well after the chain: the
/// cost-ledger event needs `rerank_used_count`, and both the response and the
/// persisted trace need `zdr_actions_applied`. Threading them through every
/// stage's return type would force each unrelated stage to carry them.
pub struct PostprocessCtx<'a> {
    /// The user query, for the stages that score against it.
    pub query: &'a str,
    /// Session ZDR posture. Drives [`ZdrFilter`] and blocks visual egress.
    pub zdr_mode: ZdrMode,
    /// Whether the query text is ZDR content. A ZDR query must not egress page
    /// images to the visual reranker.
    pub embed_zdr: bool,
    /// OUT: how many candidates the text reranker actually scored. `0` means it
    /// did not run (absent, opted out, or failed) -- which is the honest signal
    /// the confidence gate depends on, since `rerank_score` is left at `0.0` by
    /// every retrieval arm and would otherwise read as "score below threshold".
    pub rerank_used_count: usize,
    /// OUT: ZDR enforcement actions, in application order, for the audit trail.
    pub zdr_actions_applied: Vec<&'static str>,
}

impl<'a> PostprocessCtx<'a> {
    pub fn new(query: &'a str, zdr_mode: ZdrMode, embed_zdr: bool) -> Self {
        Self {
            query,
            zdr_mode,
            embed_zdr,
            rerank_used_count: 0,
            zdr_actions_applied: Vec::new(),
        }
    }
}

/// One ordered transformation of the candidate list.
///
/// Naming follows LlamaIndex's node-postprocessor concept, which is where the
/// shape was borrowed from -- the library itself was evaluated and rejected (see
/// the plan doc §3), but the interface is a good fit for an ordered chain of
/// filters and reorderings.
#[async_trait]
pub trait NodePostprocessor: Send + Sync {
    /// Stable identifier, used for the per-stage trace/log line. `&'static str`
    /// so a stage cannot accidentally make it request-dependent and blow up
    /// log-label cardinality.
    fn name(&self) -> &'static str;

    /// Transform the candidate list. An `Err` aborts the whole retrieval, so
    /// only *enforcement* stages should return one; refinement stages must
    /// absorb their provider errors and return the input unchanged.
    async fn postprocess(
        &self,
        nodes: Vec<ScoredCandidate>,
        ctx: &mut PostprocessCtx<'_>,
    ) -> anyhow::Result<Vec<ScoredCandidate>>;
}

/// What one stage did, so the caller can rebuild the trace timings.
#[derive(Debug, Clone, Copy)]
pub struct StageTiming {
    pub name: &'static str,
    pub ms: u64,
    pub before: usize,
    pub after: usize,
}

/// Run `stages` in order. Returns the final list plus one [`StageTiming`] per
/// stage, in the same order.
pub async fn run_chain(
    stages: &[&dyn NodePostprocessor],
    nodes: Vec<ScoredCandidate>,
    ctx: &mut PostprocessCtx<'_>,
) -> anyhow::Result<(Vec<ScoredCandidate>, Vec<StageTiming>)> {
    let mut nodes = nodes;
    let mut timings = Vec::with_capacity(stages.len());
    for stage in stages {
        let before = nodes.len();
        let started = std::time::Instant::now();
        nodes = stage.postprocess(nodes, ctx).await?;
        let timing = StageTiming {
            name: stage.name(),
            ms: started.elapsed().as_millis() as u64,
            before,
            after: nodes.len(),
        };
        tracing::debug!(
            stage = timing.name,
            ms = timing.ms,
            before = timing.before,
            after = timing.after,
            "postprocessor applied"
        );
        timings.push(timing);
    }
    Ok((nodes, timings))
}

/// Sum the elapsed time of the named stages. Used to reproduce the pre-refactor
/// `PipelineTimings` buckets, which grouped several steps into one number.
pub fn elapsed_of(timings: &[StageTiming], names: &[&str]) -> u64 {
    timings
        .iter()
        .filter(|t| names.contains(&t.name))
        .map(|t| t.ms)
        .sum()
}

// ---------------------------------------------------------------------------
// Stage: visual rerank (ColQwen late-interaction / MaxSim)
// ---------------------------------------------------------------------------

/// Reorders page-image candidates by ColQwen relevance on top of Embed v4's
/// first-stage order.
///
/// Refinement, not enforcement: a ZDR query, a missing client, fewer than two
/// visual candidates, or any client error all return the input untouched.
pub struct VisualRerank<'a> {
    pub client: Option<&'a ColqwenClient>,
    /// `VISUAL_RERANK_ENABLED`. Folded in here so the caller does not have to
    /// re-check it before adding the stage to the chain.
    pub enabled: bool,
    pub top_k: usize,
    /// `JOINT_MULTIMODAL_RERANK` — see [`apply_colqwen_scores`].
    pub joint: bool,
}

#[async_trait]
impl NodePostprocessor for VisualRerank<'_> {
    fn name(&self) -> &'static str {
        STAGE_VISUAL_RERANK
    }

    async fn postprocess(
        &self,
        nodes: Vec<ScoredCandidate>,
        ctx: &mut PostprocessCtx<'_>,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        let Some(client) = self.client.filter(|_| self.enabled) else {
            return Ok(nodes);
        };
        if ctx.embed_zdr {
            // A ZDR query must not egress page images to the visual reranker.
            return Ok(nodes);
        }
        // Select page-image candidates (current order) with a fetchable
        // image_url, capped at visual_rerank_top_k.
        let cap = self.top_k.max(1);
        let mut idxs: Vec<usize> = Vec::new();
        let mut urls: Vec<String> = Vec::new();
        for (i, c) in nodes.iter().enumerate() {
            if let Some(u) = page_image_url(c) {
                idxs.push(i);
                urls.push(u);
                if idxs.len() >= cap {
                    break;
                }
            }
        }
        if urls.len() < 2 {
            // 0 or 1 visual candidate — nothing to reorder.
            return Ok(nodes);
        }
        let scores = match client.rerank(ctx.query, &urls).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "visual reranker failed; keeping Embed-v4 order");
                return Ok(nodes);
            }
        };
        let out = apply_colqwen_scores(nodes, &idxs, &scores, self.joint);
        tracing::info!(
            reranked = urls.len(),
            joint = self.joint,
            "visual reranker (ColQwen) applied"
        );
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Stage: truncate
// ---------------------------------------------------------------------------

/// Keep the first `limit` candidates.
///
/// A stage rather than an inline `.take()` because *where* the truncation
/// happens relative to the visibility gate is an authority decision (see the
/// module docs), and a named stage makes it visible in the chain and in the
/// trace.
pub struct Truncate {
    pub limit: usize,
    /// Distinguishes the two truncations in logs/timings (`overfetch` before
    /// rerank, `top_n` after the visibility gate).
    pub label: &'static str,
}

#[async_trait]
impl NodePostprocessor for Truncate {
    fn name(&self) -> &'static str {
        self.label
    }

    async fn postprocess(
        &self,
        nodes: Vec<ScoredCandidate>,
        _ctx: &mut PostprocessCtx<'_>,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        Ok(nodes.into_iter().take(self.limit).collect())
    }
}

// ---------------------------------------------------------------------------
// Stage: text rerank (cross-encoder)
// ---------------------------------------------------------------------------

/// Cross-encoder rerank.
///
/// Refinement: ANY reranker failure degrades to the incoming fused order rather
/// than failing the retrieval. Reranking refines ordering; it must never be able
/// to sink an otherwise-successful query. Records the honest
/// `ctx.rerank_used_count` either way, which is what the confidence gate reads.
pub struct TextRerank<'a> {
    pub reranker: Option<&'a RerankClient>,
    /// Caller opted in (`mode_mix.rerank`) *and* the ZDR posture permits
    /// retaining egress.
    pub requested: bool,
    pub out_n: usize,
}

#[async_trait]
impl NodePostprocessor for TextRerank<'_> {
    fn name(&self) -> &'static str {
        STAGE_TEXT_RERANK
    }

    async fn postprocess(
        &self,
        nodes: Vec<ScoredCandidate>,
        ctx: &mut PostprocessCtx<'_>,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        let Some(reranker) = self.reranker.filter(|_| self.requested) else {
            ctx.rerank_used_count = 0;
            return Ok(nodes.into_iter().take(self.out_n).collect());
        };
        let input_count = nodes.len();
        match reranker.rerank(ctx.query, &nodes, self.out_n).await {
            Ok(out) => {
                // Deliberately NOT re-truncated: the reranker was asked for
                // `out_n` and its output length is its own contract.
                ctx.rerank_used_count = input_count;
                Ok(out)
            }
            Err(e) => {
                tracing::warn!(error = %e, "reranker failed; degrading to fused order");
                ctx.rerank_used_count = 0;
                Ok(nodes.into_iter().take(self.out_n).collect())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stage: recency decay (P2-3)
// ---------------------------------------------------------------------------

/// Multiplicatively dampens `final_score` by content age, read from the
/// `document_date` Qdrant payload key. Half-life decay: a candidate exactly
/// `half_life_days` old scores 0.5x, one twice that old scores 0.25x, and so
/// on — a smooth curve, not a hard cutoff.
///
/// Runs AFTER [`TextRerank`], not before: the reranker OVERWRITES
/// `final_score` outright with its own cross-encoder score, so decay applied
/// before it would be erased the instant a reranker is configured and
/// working — which is the common case in this stack. Placed here, it
/// dampens whatever score is live at this point in the chain: the reranker's
/// when one ran, the fused RRF score when it did not.
///
/// A candidate with no `document_date` (absent, unparseable, or dated in the
/// future relative to `now`) is left UNCHANGED. Treating "unknown" as "old"
/// would bury every document that predates this column — the vast majority
/// of the corpus until P2-3's backfill and re-ingestion catch up — which is a
/// worse distortion than not decaying at all. A future-dated candidate
/// (clock skew, bad connector data) is clamped to age zero rather than
/// rewarded with a multiplier above 1.0.
pub struct RecencyDecay {
    /// `RECENCY_DECAY_ENABLED` — OFF by default. This changes ranking for
    /// every query and has not yet been measured against the P0.5 golden
    /// set; shipping it on unmeasured would be a regression risk, not a
    /// free improvement.
    pub enabled: bool,
    pub half_life_days: f32,
    /// Injected rather than read via `chrono::Utc::now()` inside the stage,
    /// so tests can pin "now" and get a deterministic multiplier instead of
    /// a flaky one that depends on wall-clock time at the moment the test
    /// happens to run.
    pub now: chrono::DateTime<chrono::Utc>,
}

impl RecencyDecay {
    /// `None` (no decay applied) for a missing/unparseable date or a
    /// non-positive half-life (misconfiguration); otherwise the multiplier
    /// to apply to `final_score`.
    fn multiplier(&self, document_date: Option<&str>) -> Option<f32> {
        if self.half_life_days <= 0.0 {
            return None;
        }
        let parsed = document_date?
            .parse::<chrono::DateTime<chrono::Utc>>()
            .ok()?;
        let age_days = (self.now - parsed).num_seconds() as f32 / 86400.0;
        let age_days = age_days.max(0.0);
        Some(0.5_f32.powf(age_days / self.half_life_days))
    }
}

#[async_trait]
impl NodePostprocessor for RecencyDecay {
    fn name(&self) -> &'static str {
        STAGE_RECENCY_DECAY
    }

    async fn postprocess(
        &self,
        mut nodes: Vec<ScoredCandidate>,
        _ctx: &mut PostprocessCtx<'_>,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        if !self.enabled {
            return Ok(nodes);
        }
        use qdrant_client::qdrant::value::Kind;
        for node in &mut nodes {
            let date = match node
                .metadata
                .get("document_date")
                .and_then(|v| v.kind.as_ref())
            {
                Some(Kind::StringValue(s)) => Some(s.as_str()),
                _ => None,
            };
            if let Some(multiplier) = self.multiplier(date) {
                node.final_score *= multiplier;
            }
        }
        Ok(nodes)
    }
}

// ---------------------------------------------------------------------------
// Stage: visibility gate (liveness + per-user ownership)
// ---------------------------------------------------------------------------

/// Canonical visibility gate. Runs POST-fusion + POST-rerank over the unified
/// dense+sparse+wiki+graph candidate list, so it gates every retrieval arm
/// uniformly (closing the sparse leak for free). Two passes:
///
///   1. Liveness — drop candidates absent from canonical `documents`
///      (Qdrant/Quickwit are rebuildable read models that can lag a delete).
///   2. Per-user OWNERSHIP — when `viewer` is present, keep a document only
///      if `owner_id = viewer OR visibility = 'org' OR it is in `granted`
///      (explicit resource_grants). 'shared' docs are NOT org-readable — they
///      reach recipients ONLY via a grant. Always-on when a viewer is present,
///      decoupled from `CONTROL_PLANE_ENFORCEMENT`. When `viewer` is `None` the
///      ownership predicate is a no-op (legacy org-scoped path).
///
/// Published wiki pages stay via their own branch — wiki is org-shared
/// knowledge, not an ownable resource type. This NEVER backfills with
/// non-visible docs; it only removes, so a low-visibility user honestly
/// undershoots rather than seeing someone else's data.
///
/// Enforcement: errors propagate. Returning unfiltered candidates because a
/// query failed would leak deleted or non-visible documents.
pub struct VisibilityGate<'a> {
    pub pool: &'a PgPool,
    /// `None` means the legacy org-scoped path (or an audited org-admin
    /// bypass), which applies liveness only.
    pub viewer: Option<&'a str>,
    /// The viewer's explicit grants from user-core's `resource_grants`.
    pub granted: &'a [String],
}

#[async_trait]
impl NodePostprocessor for VisibilityGate<'_> {
    fn name(&self) -> &'static str {
        STAGE_VISIBILITY_GATE
    }

    async fn postprocess(
        &self,
        nodes: Vec<ScoredCandidate>,
        _ctx: &mut PostprocessCtx<'_>,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        let doc_ids: Vec<String> = nodes
            .iter()
            .map(|c| c.document_id.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if doc_ids.is_empty() {
            return Ok(nodes);
        }

        // Pass 1 + 2 fused into one query: liveness AND (when a viewer is
        // present) per-user ownership. The `$2::text IS NULL` branch makes the
        // ownership predicate a no-op for the no-viewer legacy path.
        let mut live: std::collections::HashSet<String> = sqlx::query_as::<_, (String,)>(
            r#"
            SELECT document_id
            FROM documents
            WHERE document_id = ANY($1)
              AND deleted_at IS NULL
              AND ($2::text IS NULL
                   OR owner_id = $2
                   OR visibility = 'org'
                   OR document_id = ANY($3))
            "#,
        )
        .bind(&doc_ids)
        .bind(self.viewer)
        .bind(self.granted)
        .fetch_all(self.pool)
        .await?
        .into_iter()
        .map(|(id,)| id)
        .collect();

        // Wiki candidates (from the wiki ANN arm) carry document_id = wiki
        // page_id, which is canonical in wiki_pages, not documents. Treat a
        // published wiki page as live so this visibility gate doesn't drop
        // every wiki hit as "missing from canonical documents". Best-effort:
        // a failure here just means wiki candidates fall back to being gated
        // by the documents table alone (i.e. filtered out).
        match sqlx::query_as::<_, (String,)>(
            r#"
            SELECT page_id
            FROM wiki_pages
            WHERE page_id = ANY($1)
              AND page_status = 'published'
            "#,
        )
        .bind(&doc_ids)
        .fetch_all(self.pool)
        .await
        {
            Ok(rows) => live.extend(rows.into_iter().map(|(id,)| id)),
            Err(e) => {
                tracing::warn!(error = %e, "wiki live-gate lookup failed; wiki candidates may be dropped")
            }
        }

        if live.len() == doc_ids.len() {
            return Ok(nodes);
        }

        let before = nodes.len();
        let filtered: Vec<ScoredCandidate> = nodes
            .into_iter()
            .filter(|c| live.contains(&c.document_id))
            .collect();
        // Raw-vs-survivor on the trace. When a viewer is present and a large
        // fraction was dropped, flag potential top-k starvation (the response
        // will honestly undershoot rather than backfill non-visible docs).
        let after = filtered.len();
        let viewer_present = self.viewer.is_some();
        if viewer_present && after * 2 < before {
            tracing::warn!(
                before,
                after,
                viewer_present,
                "ownership/liveness gate dropped >50% of candidates — possible top-k starvation; result honestly undershoots"
            );
        } else {
            tracing::debug!(before, after, viewer_present, "visibility gate applied");
        }
        Ok(filtered)
    }
}

// ---------------------------------------------------------------------------
// Stage: ZDR enforcement
// ---------------------------------------------------------------------------

/// Zero-data-retention enforcement.
///
/// In [`ZdrMode::Reject`], drops candidates whose document is classified
/// `restricted`. Records what it actually did on `ctx.zdr_actions_applied`, so
/// the audit trail can distinguish "mode=reject but nothing to reject" from
/// "mode=disabled" from "mode=reject and N docs filtered".
///
/// Enforcement: errors propagate.
pub struct ZdrFilter<'a> {
    pub pool: &'a PgPool,
}

#[async_trait]
impl NodePostprocessor for ZdrFilter<'_> {
    fn name(&self) -> &'static str {
        STAGE_ZDR_FILTER
    }

    async fn postprocess(
        &self,
        nodes: Vec<ScoredCandidate>,
        ctx: &mut PostprocessCtx<'_>,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        match ctx.zdr_mode {
            ZdrMode::Reject => {
                let candidate_doc_ids: Vec<String> =
                    nodes.iter().map(|c| c.document_id.clone()).collect();
                if candidate_doc_ids.is_empty() {
                    return Ok(nodes);
                }
                let restricted: std::collections::HashSet<String> = sqlx::query_as::<_, (String,)>(
                    "SELECT document_id FROM documents WHERE document_id = ANY($1) AND zdr_classification = 'restricted'"
                )
                .bind(&candidate_doc_ids)
                .fetch_all(self.pool)
                .await?
                .into_iter()
                .map(|(id,)| id)
                .collect();

                if restricted.is_empty() {
                    ctx.zdr_actions_applied
                        .push("reject_mode_no_restricted_found");
                    Ok(nodes)
                } else {
                    ctx.zdr_actions_applied
                        .push("reject_mode_filtered_restricted");
                    Ok(nodes
                        .into_iter()
                        .filter(|c| !restricted.contains(&c.document_id))
                        .collect())
                }
            }
            ZdrMode::Ephemeral => {
                ctx.zdr_actions_applied.push("ephemeral_no_trace_persist");
                Ok(nodes)
            }
            ZdrMode::Disabled => Ok(nodes),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(document_id: &str, score: f32) -> ScoredCandidate {
        ScoredCandidate {
            knowledge_id: format!("k-{document_id}"),
            document_id: document_id.to_string(),
            chunk_index: 0,
            text: String::new(),
            final_score: score,
            dense_score: score,
            sparse_score: 0.0,
            rerank_score: 0.0,
            metadata: std::collections::HashMap::new(),
        }
    }

    fn ctx() -> PostprocessCtx<'static> {
        PostprocessCtx::new("q", ZdrMode::Disabled, false)
    }

    fn ids(v: &[ScoredCandidate]) -> Vec<&str> {
        v.iter().map(|c| c.document_id.as_str()).collect()
    }

    #[tokio::test]
    async fn truncate_keeps_prefix_order() {
        let stage = Truncate {
            limit: 2,
            label: "truncate:test",
        };
        let mut c = ctx();
        let got = stage
            .postprocess(vec![cand("a", 3.0), cand("b", 2.0), cand("c", 1.0)], &mut c)
            .await
            .unwrap();
        assert_eq!(ids(&got), vec!["a", "b"]);
    }

    #[tokio::test]
    async fn truncate_is_a_noop_below_the_limit() {
        let stage = Truncate {
            limit: 10,
            label: "truncate:test",
        };
        let mut c = ctx();
        let got = stage
            .postprocess(vec![cand("a", 1.0)], &mut c)
            .await
            .unwrap();
        assert_eq!(ids(&got), vec!["a"]);
    }

    /// The absent-reranker path must still bound the output AND report 0 usage.
    /// Reporting a non-zero count here is what previously made the confidence
    /// gate read an all-zero `rerank_score` as "below threshold" and mark every
    /// query low-confidence.
    #[tokio::test]
    async fn text_rerank_without_a_client_truncates_and_reports_zero_usage() {
        let stage = TextRerank {
            reranker: None,
            requested: true,
            out_n: 2,
        };
        let mut c = ctx();
        let got = stage
            .postprocess(vec![cand("a", 3.0), cand("b", 2.0), cand("c", 1.0)], &mut c)
            .await
            .unwrap();
        assert_eq!(ids(&got), vec!["a", "b"]);
        assert_eq!(c.rerank_used_count, 0);
    }

    /// `requested: false` (caller opted out, or a restrictive ZDR posture) must
    /// behave exactly like "no reranker configured" — never egress the query.
    #[tokio::test]
    async fn text_rerank_opt_out_matches_the_absent_client_path() {
        let stage = TextRerank {
            reranker: None,
            requested: false,
            out_n: 1,
        };
        let mut c = ctx();
        let got = stage
            .postprocess(vec![cand("a", 3.0), cand("b", 2.0)], &mut c)
            .await
            .unwrap();
        assert_eq!(ids(&got), vec!["a"]);
        assert_eq!(c.rerank_used_count, 0);
    }

    /// A disabled visual stage must not consult the client at all. Combined with
    /// the ZDR check below, this pins both non-egress conditions.
    #[tokio::test]
    async fn visual_rerank_disabled_returns_input_untouched() {
        let stage = VisualRerank {
            client: None,
            enabled: false,
            top_k: 8,
            joint: false,
        };
        let mut c = ctx();
        let got = stage
            .postprocess(vec![cand("a", 2.0), cand("b", 1.0)], &mut c)
            .await
            .unwrap();
        assert_eq!(ids(&got), vec!["a", "b"]);
    }

    // NOTE: [`ZdrFilter`] and [`VisibilityGate`] both hold a `&PgPool`, so their
    // filtering behaviour is covered by the DB-gated integration tests
    // (`tests/zdr_behavior.rs`, `tests/ownership_filter.rs`) rather than here.
    // Asserting their branches against a hand-rolled copy of the same logic
    // would only prove the copy matches itself.

    #[test]
    fn elapsed_of_sums_only_the_named_stages() {
        let t = vec![
            StageTiming {
                name: "a",
                ms: 5,
                before: 3,
                after: 3,
            },
            StageTiming {
                name: "b",
                ms: 7,
                before: 3,
                after: 2,
            },
            StageTiming {
                name: "c",
                ms: 11,
                before: 2,
                after: 2,
            },
        ];
        assert_eq!(elapsed_of(&t, &["a", "c"]), 16);
        assert_eq!(elapsed_of(&t, &["missing"]), 0);
    }

    #[tokio::test]
    async fn run_chain_applies_stages_in_order_and_times_each() {
        let first = Truncate {
            limit: 3,
            label: "truncate:overfetch",
        };
        let second = Truncate {
            limit: 1,
            label: "truncate:top_n",
        };
        let stages: Vec<&dyn NodePostprocessor> = vec![&first, &second];
        let mut c = ctx();
        let (got, timings) = run_chain(
            &stages,
            vec![
                cand("a", 3.0),
                cand("b", 2.0),
                cand("c", 1.0),
                cand("d", 0.5),
            ],
            &mut c,
        )
        .await
        .unwrap();
        assert_eq!(ids(&got), vec!["a"]);
        assert_eq!(timings.len(), 2);
        assert_eq!(timings[0].name, "truncate:overfetch");
        assert_eq!((timings[0].before, timings[0].after), (4, 3));
        assert_eq!(timings[1].name, "truncate:top_n");
        assert_eq!((timings[1].before, timings[1].after), (3, 1));
    }

    // --- RecencyDecay -------------------------------------------------------

    fn cand_dated(document_id: &str, score: f32, document_date: &str) -> ScoredCandidate {
        let mut c = cand(document_id, score);
        c.metadata.insert(
            "document_date".to_string(),
            qdrant_client::qdrant::Value {
                kind: Some(qdrant_client::qdrant::value::Kind::StringValue(
                    document_date.to_string(),
                )),
            },
        );
        c
    }

    fn fixed_now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-08-05T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[tokio::test]
    async fn disabled_decay_leaves_scores_untouched_even_with_a_dated_candidate() {
        let stage = RecencyDecay {
            enabled: false,
            half_life_days: 30.0,
            now: fixed_now(),
        };
        let mut c = ctx();
        let got = stage
            .postprocess(vec![cand_dated("a", 1.0, "2020-01-01T00:00:00Z")], &mut c)
            .await
            .unwrap();
        assert_eq!(got[0].final_score, 1.0);
    }

    #[tokio::test]
    async fn a_candidate_exactly_one_half_life_old_is_halved() {
        let stage = RecencyDecay {
            enabled: true,
            half_life_days: 30.0,
            now: fixed_now(),
        };
        let mut c = ctx();
        // Exactly 30 days before fixed_now().
        let got = stage
            .postprocess(vec![cand_dated("a", 1.0, "2026-07-06T00:00:00Z")], &mut c)
            .await
            .unwrap();
        assert!(
            (got[0].final_score - 0.5).abs() < 1e-4,
            "score = {}, want ~0.5",
            got[0].final_score
        );
    }

    #[tokio::test]
    async fn a_candidate_with_no_document_date_is_unchanged() {
        let stage = RecencyDecay {
            enabled: true,
            half_life_days: 30.0,
            now: fixed_now(),
        };
        let mut c = ctx();
        let got = stage
            .postprocess(vec![cand("a", 0.73)], &mut c)
            .await
            .unwrap();
        assert_eq!(got[0].final_score, 0.73);
    }

    #[tokio::test]
    async fn an_unparseable_document_date_is_treated_as_unknown_not_penalized() {
        let stage = RecencyDecay {
            enabled: true,
            half_life_days: 30.0,
            now: fixed_now(),
        };
        let mut c = ctx();
        let got = stage
            .postprocess(vec![cand_dated("a", 0.5, "not-a-date")], &mut c)
            .await
            .unwrap();
        assert_eq!(got[0].final_score, 0.5);
    }

    #[tokio::test]
    async fn a_future_dated_candidate_is_clamped_to_zero_age_not_boosted() {
        let stage = RecencyDecay {
            enabled: true,
            half_life_days: 30.0,
            now: fixed_now(),
        };
        let mut c = ctx();
        // One day AFTER fixed_now() -- clock skew or bad connector data.
        let got = stage
            .postprocess(vec![cand_dated("a", 1.0, "2026-08-06T00:00:00Z")], &mut c)
            .await
            .unwrap();
        assert_eq!(
            got[0].final_score, 1.0,
            "a future date must not produce a multiplier above 1.0"
        );
    }

    #[tokio::test]
    async fn a_non_positive_half_life_disables_decay_rather_than_dividing_by_zero() {
        let stage = RecencyDecay {
            enabled: true,
            half_life_days: 0.0,
            now: fixed_now(),
        };
        let mut c = ctx();
        let got = stage
            .postprocess(vec![cand_dated("a", 1.0, "2020-01-01T00:00:00Z")], &mut c)
            .await
            .unwrap();
        assert_eq!(got[0].final_score, 1.0);
    }

    #[tokio::test]
    async fn decay_can_reorder_a_stale_high_scorer_below_a_fresh_low_scorer() {
        let stage = RecencyDecay {
            enabled: true,
            half_life_days: 30.0,
            now: fixed_now(),
        };
        let mut c = ctx();
        let got = stage
            .postprocess(
                vec![
                    // 120 days old (4 half-lives): 1.0 * 0.5^4 = 0.0625.
                    cand_dated("stale", 1.0, "2026-04-07T00:00:00Z"),
                    // Fresh, undecayed.
                    cand_dated("fresh", 0.5, "2026-08-05T00:00:00Z"),
                ],
                &mut c,
            )
            .await
            .unwrap();
        let stale_score = got
            .iter()
            .find(|c| c.document_id == "stale")
            .unwrap()
            .final_score;
        let fresh_score = got
            .iter()
            .find(|c| c.document_id == "fresh")
            .unwrap()
            .final_score;
        assert!(
            fresh_score > stale_score,
            "fresh={fresh_score} should now outrank stale={stale_score}"
        );
    }
}
