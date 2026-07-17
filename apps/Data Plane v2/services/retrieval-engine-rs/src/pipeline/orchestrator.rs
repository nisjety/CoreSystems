use std::time::Instant;

use event_envelope_rs::EventSigner;
use qdrant_client::qdrant::Condition;
use qdrant_client::Qdrant;
use sqlx::PgPool;

use crate::cache::CacheLayer;
use crate::config::Config;
use crate::context_pack::pack_context;
use crate::embed::EmbeddingClient;
use crate::pipeline::types::*;
use crate::search::dense::vector_search;
use crate::search::filters::RetrievalFilters;
use crate::search::fusion::reciprocal_rank_fusion;
use crate::search::rerank::RerankClient;
use crate::search::sparse::DynSparseSearchBackend;
use crate::trace::persist_trace;

/// Qdrant collection the embedding-engine writes wiki page embeddings to.
/// Must match `embedding_engine_rs::wiki_consumer::WIKI_COLLECTION`.
const WIKI_COLLECTION: &str = "wiki_block_embeddings";

/// Any restrictive posture blocks retaining egress. This covers both the
/// ephemeral no-persistence posture and reject mode: neither may silently send
/// query text or candidates to Cohere/Azure/text rerank providers.
fn embed_zdr_for_mode(zdr_mode: ZdrMode) -> bool {
    zdr_mode.restricts_egress()
}

fn embedding_cache_allowed(zdr: bool) -> bool {
    !zdr
}

fn text_rerank_allowed(zdr_mode: ZdrMode, requested: bool) -> bool {
    requested && !zdr_mode.restricts_egress()
}

fn encode_cost_event(
    signer: &EventSigner,
    org_id: &str,
    user_id: Option<&str>,
    zdr_mode: ZdrMode,
    model: &str,
    count: usize,
) -> anyhow::Result<Option<Vec<u8>>> {
    if zdr_mode.restricts_egress() {
        return Ok(None);
    }
    let event = serde_json::json!({
        "event_type": "rerank",
        "model": model,
        "count": count,
        "estimated_tokens": count as i64 * 32,
        "org_id": org_id,
        "user_id": user_id,
        "zdr": false,
        "idempotency_key": format!("rerank:{org_id}:{}", uuid::Uuid::new_v4()),
    });
    let raw = serde_json::to_vec(&event)?;
    Ok(Some(signer.sign(
        "dataplane.cost.ledger",
        org_id,
        user_id,
        false,
        &raw,
    )?))
}

pub struct RetrievalPipeline {
    pub pool: PgPool,
    pub qdrant: Qdrant,
    pub embedder: EmbeddingClient,
    pub reranker: Option<RerankClient>,
    pub cache: Option<CacheLayer>,
    pub config: Config,
    /// Wave 3 §15-B/C — `user-service.CheckMembership` +
    /// `org-core.GetUserPermissions`, abstracted so local dev can use
    /// `NoopPolicyClient` while prod uses `HttpPolicyClient`.
    pub policy: std::sync::Arc<dyn crate::authz::PolicyClient>,
    /// Per-User Data Ownership & Sharing — resolves a viewer's explicit document
    /// grants from user-core's resource_grants. ALWAYS-ON when a user_id is
    /// present, decoupled from `CONTROL_PLANE_ENFORCEMENT` (which gates only the
    /// coarse org axis via `policy`). The post-filter unions this with the
    /// owner_id/visibility columns on `documents`.
    pub visibility: std::sync::Arc<dyn crate::authz::VisibilityClient>,
    /// Wave 3.1 §15-G — publish `dataplane.cost.ledger` events on rerank
    /// (embedding worker handles embeds). Optional: degrades silently when
    /// NATS isn't reachable.
    pub nats: Option<async_nats::Client>,
    /// Producer-local signer for retrieval cost events. When absent, the
    /// durable ledger publication is disabled; raw tenant-selected JSON is
    /// never emitted as a fallback.
    pub event_signer: Option<std::sync::Arc<EventSigner>>,
    /// Sparse lexical search backend. Postgres remains the canonical fallback;
    /// Quickwit is a rebuildable read model when enabled.
    pub sparse_backend: DynSparseSearchBackend,
    /// Visual RAG arm query embedder (Cohere Embed v4). `None` when the visual
    /// arm is not configured (text-only deployment); when present and `w_visual`
    /// > 0 the orchestrator embeds the query into Embed v4's multimodal space and
    ///
    /// fuses page-image hits from `qdrant_visual_collection`.
    pub visual_embedder: Option<crate::embed::visual::VisualQueryEmbedder>,
    /// ColQwen visual reranker client. `Some` only when `VISUAL_RERANK_ENABLED`
    /// and `COLQWEN_ENDPOINT_URL` are set. When present, the orchestrator reorders
    /// Embed-v4's page-image candidates by ColQwen late-interaction (MaxSim)
    /// relevance; any failure degrades to the Embed-v4 order (non-fatal).
    pub colqwen: Option<crate::search::colqwen::ColqwenClient>,
    /// Deep multi-hop graph traversal client (graph-index's `/v1/graph/traverse`,
    /// Neo4j read-model). `None` when `GRAPH_INDEX_URL` is empty. Used by the
    /// fused graph arm only when the request carries a verified bearer to
    /// forward; otherwise the arm's in-process 1-hop grounding serves alone.
    pub graph_remote: Option<crate::search::graph_remote::GraphTraverseClient>,
}

/// A page-image candidate's fetchable `image_url`, read from its raw Qdrant
/// payload metadata — or `None` if it isn't a page-image candidate / has no URL.
fn page_image_url(c: &ScoredCandidate) -> Option<String> {
    use qdrant_client::qdrant::value::Kind;
    let kind = |k: &str| c.metadata.get(k).and_then(|v| v.kind.as_ref());
    match kind("source_type") {
        Some(Kind::StringValue(s)) if s == "page_image" => {}
        _ => return None,
    }
    match kind("image_url") {
        Some(Kind::StringValue(u)) if !u.is_empty() => Some(u.clone()),
        _ => None,
    }
}

/// Pure fusion chain extracted from `retrieve()` so it is unit-testable and so
/// the concurrently-gathered arms fuse in EXACTLY the original sequential order:
///
///   1. dense + sparse RRF (k=60) with the renormalized `bm25_share` when the
///      sparse arm ran (`sparse = Some`) AND dense is routed; sparse-only when
///      the sparse arm ran but dense isn't routed; dense-only with NO RRF when
///      the sparse arm didn't run (`sparse = None`).
///   2. graph RRF (k=60, `w_graph`) when `w_graph > 0` and the graph arm returned
///      hits. This is the signal §16.1.1 flagged as recorded-but-never-scored —
///      it now drives fusion, so the `mode_mix` trace no longer lies.
///   3. wiki RRF (k=60, `w_wiki`) when `w_wiki > 0` and the wiki arm returned hits.
///   4. visual RRF (k=60, `w_visual`) when `w_visual > 0` and the visual arm
///      returned hits.
///
/// Returns the fused list plus the sparse candidate count for the trace. The
/// `Option` on `sparse` is load-bearing: `None` (route off) yields the raw dense
/// list, whereas `Some(empty)` (route on, zero hits) still runs RRF and thus
/// re-weights the dense scores — the two are NOT interchangeable.
fn fuse_arms(
    dense: Vec<ScoredCandidate>,
    sparse: Option<Vec<ScoredCandidate>>,
    graph: Vec<ScoredCandidate>,
    wiki: Vec<ScoredCandidate>,
    visual: Vec<ScoredCandidate>,
    mix: &ResolvedWeights,
    route_dense: bool,
) -> (Vec<ScoredCandidate>, usize) {
    // Step 4 — dense + sparse.
    let (mut fused, sparse_count) = match sparse {
        Some(sparse_candidates) => {
            let sparse_count = sparse_candidates.len();
            if route_dense {
                // Renormalize dense+bm25 sub-mix so RRF gets a [0,1] weight on
                // the BM25 list.
                let bm25_share = {
                    let d = mix.w_dense;
                    let b = mix.w_bm25;
                    let sum = (d + b).max(f32::EPSILON);
                    b / sum
                };
                (
                    reciprocal_rank_fusion(&dense, &sparse_candidates, 60.0, bm25_share),
                    sparse_count,
                )
            } else {
                // Sparse-only route: no dense list to fuse against.
                (sparse_candidates, sparse_count)
            }
        }
        None => (dense, 0),
    };

    // Graph arm — fold graph-grounded candidates by w_graph (peer of wiki/visual,
    // additive RRF). Closes §16.1.1: w_graph now affects scoring, not just the
    // trace.
    if mix.w_graph > 0.0 && !graph.is_empty() {
        fused = reciprocal_rank_fusion(&fused, &graph, 60.0, mix.w_graph);
    }

    // Step 5 — wiki 4-way merge by its w_wiki share.
    if mix.w_wiki > 0.0 && !wiki.is_empty() {
        fused = reciprocal_rank_fusion(&fused, &wiki, 60.0, mix.w_wiki);
    }

    // Visual arm — layer Embed v4 page-image hits by w_visual (purely additive).
    if mix.w_visual > 0.0 && !visual.is_empty() {
        fused = reciprocal_rank_fusion(&fused, &visual, 60.0, mix.w_visual);
    }

    (fused, sparse_count)
}

impl RetrievalPipeline {
    /// ColQwen visual reranker: reorder the page-image candidates in `fused` by
    /// ColQwen late-interaction (MaxSim) relevance to `query`. Only the order
    /// *among the visual candidates* changes — they keep the score band they
    /// already occupy, so they don't leapfrog text candidates. Non-fatal: a ZDR
    /// query or any client error returns `fused` unchanged (Embed-v4 order).
    async fn visual_rerank(
        &self,
        query: &str,
        mut fused: Vec<ScoredCandidate>,
        embed_zdr: bool,
    ) -> Vec<ScoredCandidate> {
        let Some(ref client) = self.colqwen else {
            return fused;
        };
        if embed_zdr {
            // A ZDR query must not egress page images to the visual reranker.
            return fused;
        }
        // Select page-image candidates (current order) with a fetchable image_url,
        // capped at visual_rerank_top_k.
        let cap = self.config.visual_rerank_top_k.max(1);
        let mut idxs: Vec<usize> = Vec::new();
        let mut urls: Vec<String> = Vec::new();
        for (i, c) in fused.iter().enumerate() {
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
            return fused;
        }
        let scores = match client.rerank(query, &urls).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "visual reranker failed; keeping Embed-v4 order");
                return fused;
            }
        };
        // Pair each visual candidate with its ColQwen score and sort best-first.
        let mut ranked: Vec<(ScoredCandidate, f32)> = idxs
            .iter()
            .enumerate()
            .map(|(k, &i)| (fused[i].clone(), scores[k]))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        // The score band the visual candidates currently occupy (desc), reused so
        // the reordered subset interleaves with text candidates exactly as before.
        let mut band: Vec<f32> = idxs.iter().map(|&i| fused[i].final_score).collect();
        band.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        // Write the best ColQwen candidate into the earliest visual slot with the
        // highest band score, and so on — reorders the subset in place.
        for (rank, &slot) in idxs.iter().enumerate() {
            let (mut cand, cq) = ranked[rank].clone();
            cand.rerank_score = cq;
            cand.final_score = band[rank];
            fused[slot] = cand;
        }
        tracing::info!(reranked = urls.len(), "visual reranker (ColQwen) applied");
        fused
    }

    /// Dense arm — main-collection ANN over the query embedding. Gated on the
    /// dense route (a present `query_vector`); returns an empty list when dense
    /// wasn't routed (embed skipped). FATAL: a Qdrant error propagates via `?`.
    async fn arm_dense(
        &self,
        query_vector: &Option<Vec<f32>>,
        org_id: &str,
        conditions: Vec<Condition>,
        top_k: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        if let Some(ref qv) = query_vector {
            vector_search(
                &self.qdrant,
                &self.config.qdrant_collection,
                qv.clone(),
                org_id,
                conditions,
                top_k,
            )
            .await
        } else {
            Ok(Vec::new())
        }
    }

    /// Sparse arm — lexical/BM25 search. Gated on `routed` (`route.sparse`):
    /// `Ok(None)` when sparse isn't routed (so no scan runs), `Ok(Some(..))`
    /// when it ran. FATAL: a backend error propagates via `?`. The `Option`
    /// preserves the route-off vs ran-empty distinction that fusion depends on.
    async fn arm_sparse(
        &self,
        query: &str,
        org_id: &str,
        top_k: usize,
        routed: bool,
    ) -> anyhow::Result<Option<Vec<ScoredCandidate>>> {
        if routed {
            Ok(Some(
                self.sparse_backend.search(query, org_id, top_k).await?,
            ))
        } else {
            Ok(None)
        }
    }

    /// Wiki ANN arm — WIKI_COLLECTION ANN over the (reused) query embedding.
    /// Gated on `w_wiki > 0` AND a present query vector. NON-FATAL: on skip, a
    /// Qdrant error, or an empty result it returns an empty list (the request
    /// never fails). Emits the same `warn!` as the sequential path on error.
    async fn arm_wiki(
        &self,
        query_vector: &Option<Vec<f32>>,
        org_id: &str,
        w_wiki: f32,
        top_k: usize,
    ) -> Vec<ScoredCandidate> {
        if w_wiki <= 0.0 {
            return Vec::new();
        }
        let Some(ref qv) = query_vector else {
            return Vec::new();
        };
        match vector_search(
            &self.qdrant,
            WIKI_COLLECTION,
            qv.clone(),
            org_id,
            Vec::new(),
            top_k,
        )
        .await
        {
            Ok(wiki) if !wiki.is_empty() => wiki,
            Ok(_) => Vec::new(),
            Err(e) => {
                tracing::warn!(error = %e, "wiki ANN arm failed; skipping");
                Vec::new()
            }
        }
    }

    /// Graph arm — org-scoped graph-neighbourhood grounding, in two tiers:
    ///
    ///   1. **Deep multi-hop (remote):** when `graph_remote` is configured AND
    ///      the request carries a verified bearer, resolve seed entities from
    ///      the query text and call graph-index's `POST /v1/graph/traverse`
    ///      (Neo4j read-model, server-side Postgres fallback) forwarding that
    ///      bearer — graph-index independently re-verifies it and pins the org.
    ///      The traversed `(entity, hop)` set is grounded back to org-visible
    ///      chunks nearest-hop-first (the "entities → candidates" adapter).
    ///   2. **In-process 1-hop (fallback):** the local SQL grounding (entities
    ///      matching the query + their 1-hop neighbours). Also the only tier on
    ///      bearer-less internal calls.
    ///
    /// Gated on `w_graph > 0`. Uses the query TEXT (no embedding), so it runs
    /// regardless of the dense route. NON-FATAL at every step: remote errors
    /// degrade to tier 2; tier-2 errors yield an empty arm, exactly like the
    /// wiki/visual arms. The graph is built only from `visibility = 'org'`
    /// provenance and the step-6 canonical gate still re-filters, so this arm
    /// cannot leak.
    async fn arm_graph(
        &self,
        query: &str,
        org_id: &str,
        bearer: Option<&str>,
        w_graph: f32,
        top_k: usize,
    ) -> Vec<ScoredCandidate> {
        if w_graph <= 0.0 {
            return Vec::new();
        }

        // Tier 1 — deep multi-hop via graph-index's traverse endpoint.
        if let (Some(remote), Some(bearer)) = (self.graph_remote.as_ref(), bearer) {
            match crate::search::graph::seed_entities_for_query(&self.pool, query, org_id, 8).await
            {
                Ok(seeds) if !seeds.is_empty() => {
                    match remote
                        .traverse(
                            bearer,
                            org_id,
                            &seeds,
                            self.config.graph_remote_max_hops,
                            top_k as u32,
                        )
                        .await
                    {
                        Ok(reached) => {
                            // Seeds are the direct query matches (hop 0); the
                            // traversal contributes the connected facts.
                            let mut entities: Vec<(String, u8)> =
                                seeds.into_iter().map(|id| (id, 0)).collect();
                            entities.extend(reached);
                            match crate::search::graph::chunks_for_entities(
                                &self.pool,
                                org_id,
                                &entities,
                                top_k as i64,
                            )
                            .await
                            {
                                Ok(candidates) => return candidates,
                                Err(e) => {
                                    tracing::warn!(error = %e, "graph arm chunk grounding failed; falling back to 1-hop")
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "graph traverse (remote) failed; falling back to 1-hop")
                        }
                    }
                }
                Ok(_) => {
                    // No seed entities match the query — the 1-hop SQL would
                    // find nothing either (same match predicate); skip cleanly.
                    return Vec::new();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "graph seed resolution failed; falling back to 1-hop")
                }
            }
        }

        // Tier 2 — in-process 1-hop grounding.
        match crate::search::graph::graph_arm_candidates(&self.pool, query, org_id, top_k as i64)
            .await
        {
            Ok(candidates) => candidates,
            Err(e) => {
                tracing::warn!(error = %e, "graph arm failed; skipping");
                Vec::new()
            }
        }
    }

    /// Visual arm — Cohere Embed v4 query embedding + visual-collection ANN.
    /// Gated on `w_visual > 0` AND a configured `visual_embedder`. NON-FATAL: on
    /// skip, a dim mismatch, an embed error, a Qdrant error, or an empty result
    /// it returns an empty list. Emits the same `warn!`s as the sequential path.
    async fn arm_visual(
        &self,
        query: &str,
        org_id: &str,
        w_visual: f32,
        embed_zdr: bool,
        top_k: usize,
    ) -> Vec<ScoredCandidate> {
        if w_visual <= 0.0 {
            return Vec::new();
        }
        let Some(ref ve) = self.visual_embedder else {
            return Vec::new();
        };
        match ve.embed_query(query, embed_zdr).await {
            // Guard the visual query vector against the configured visual
            // dimension — a misconfigured Embed v4 deployment returning a
            // different dim than the collection would otherwise error per
            // candidate; skip the arm cleanly instead.
            Ok(visual_vec) if visual_vec.len() == self.config.visual_embedding_dimension => {
                match vector_search(
                    &self.qdrant,
                    &self.config.qdrant_visual_collection,
                    visual_vec,
                    org_id,
                    Vec::new(),
                    top_k,
                )
                .await
                {
                    Ok(visual) if !visual.is_empty() => visual,
                    Ok(_) => Vec::new(),
                    Err(e) => {
                        tracing::warn!(error = %e, "visual ANN arm failed; skipping");
                        Vec::new()
                    }
                }
            }
            Ok(visual_vec) => {
                tracing::warn!(
                    got = visual_vec.len(),
                    want = self.config.visual_embedding_dimension,
                    "visual query embedding dim mismatch; skipping visual arm"
                );
                Vec::new()
            }
            Err(e) => {
                tracing::warn!(error = %e, "visual query embed skipped; skipping visual arm");
                Vec::new()
            }
        }
    }

    #[tracing::instrument(
        name = "retrieval.pipeline",
        skip(self, req),
        fields(
            org_id = req.org_id.as_str(),
            query_len = req.query.len(),
            zdr_mode = req.zdr_mode.unwrap_or_default().as_str(),
        ),
    )]
    pub async fn retrieve(&self, req: RetrievalRequest) -> anyhow::Result<RetrievalResponse> {
        // Input bounds: refuse pathological inputs at the door rather than
        // letting them propagate into Qdrant filters and Postgres ANY($1)
        // queries (where they cause memory/CPU spikes).
        const MAX_QUERY_LEN: usize = 8 * 1024;
        const MAX_FILTER_IDS: usize = 1000;
        if req.org_id.is_empty() {
            anyhow::bail!("org_id is required");
        }
        if req.query.len() > MAX_QUERY_LEN {
            anyhow::bail!("query exceeds max length of {MAX_QUERY_LEN} bytes");
        }
        if req.filters.document_ids.len() > MAX_FILTER_IDS {
            anyhow::bail!("filters.document_ids exceeds max of {MAX_FILTER_IDS}");
        }

        let pipeline_start = Instant::now();
        let top_k = req.top_k.unwrap_or(self.config.retrieval_top_k);
        let top_n = req.top_n.unwrap_or(self.config.retrieval_top_n);
        let zdr_mode = req.zdr_mode.unwrap_or_default();
        // ephemeral = the zero-data-retention session mode. The query text is
        // ZDR content, so its embedding must not egress to a retaining provider:
        // this drives the embed-path egress guard (the direct-Azure backend
        // fails closed when true).
        let embed_zdr = embed_zdr_for_mode(zdr_mode);

        // Resolve the mode-mix weights FIRST so they can drive engine routing
        // (D4+D5 spec §7) and so captured-on-trace == used-for-scoring.
        //
        // §16.1.4 — agent_retrieval_configs lookup. If the caller passed
        // `agent_id` AND a row exists for (org_id, agent_id), the row's
        // `weights` becomes the default; any explicit per-request
        // `mode_mix` still overrides. Cohort precedence:
        //   per-request mode_mix > agent default > global config
        let agent_default = if let Some(agent_id) = req.agent_id.as_deref() {
            crate::agent_config::lookup(&self.pool, &req.org_id, agent_id).await
        } else {
            None
        };
        let starting_mix = req.mode_mix.clone().unwrap_or_else(|| {
            agent_default
                .as_ref()
                .and_then(|c| serde_json::from_value::<ModeMixWeights>(c.weights.clone()).ok())
                .unwrap_or_default()
        });
        let mix_for_scoring = starting_mix.resolve(
            self.config.w_dense,
            self.config.w_bm25,
            self.config.w_graph,
            self.config.w_wiki,
            self.config.w_visual,
        );

        // Best-tool routing: run only the engines the blend actually weights.
        // A purely lexical blend (w_dense≈0) skips the embedding call + Qdrant
        // entirely; a purely semantic blend skips the sparse scan. Reuses the
        // existing dense/sparse backends — no parallel retrieval path.
        let route = EngineRoute::from_weights(&mix_for_scoring, self.config.hybrid_enabled);

        // §16.4.7 PII redaction — strip email/phone/cardlike before emitting.
        tracing::info!(
            org_id = %req.org_id,
            query = %crate::redact::redact_query(&req.query, 60),
            top_k,
            top_n,
            hybrid = self.config.hybrid_enabled,
            route_dense = route.dense,
            route_sparse = route.sparse,
            "retrieval.start"
        );

        // 1. Embed query (only when dense retrieval is routed). Cached, with the
        // key namespaced by embedding route/model so a model rotation doesn't
        // serve stale vectors for the remaining TTL (§16.2.8).
        let embed_start = Instant::now();
        let query_vector = if route.dense {
            let query_hash = crate::cache::hash_text(&req.query);
            let model_ver = self.embedder.cache_namespace();
            let vec = if embedding_cache_allowed(embed_zdr) {
                if let Some(ref cache) = self.cache {
                    if let Some(cached) = cache.get_embedding(&model_ver, &query_hash).await {
                        tracing::debug!("embed cache hit");
                        crate::metrics::record_embed_cache_hit();
                        cached
                    } else {
                        crate::metrics::record_embed_request();
                        let vec = self
                            .embedder
                            .embed_query(&req.org_id, &req.query, embed_zdr)
                            .await?;
                        cache.set_embedding(&model_ver, &query_hash, &vec).await;
                        vec
                    }
                } else {
                    crate::metrics::record_embed_request();
                    self.embedder
                        .embed_query(&req.org_id, &req.query, embed_zdr)
                        .await?
                }
            } else {
                // ZDR is a cache-admission decision, not just an embedding-provider
                // flag. Skip both reads and writes so an ephemeral request never
                // touches durable Dragonfly state (including a pre-existing key).
                crate::metrics::record_embed_request();
                self.embedder
                    .embed_query(&req.org_id, &req.query, embed_zdr)
                    .await?
            };

            // Validate embedding response: empty or wrong-dimension vectors
            // would cause Qdrant to return InvalidArgument with a confusing
            // message; catch them here with a clear error.
            if vec.is_empty() {
                anyhow::bail!("embedding provider returned empty vector");
            }
            if vec.len() != self.config.embedding_dimension {
                anyhow::bail!(
                    "embedding dimension mismatch: got {}, expected {}",
                    vec.len(),
                    self.config.embedding_dimension
                );
            }
            Some(vec)
        } else {
            tracing::debug!("dense retrieval disabled by mode_mix; skipping embed + vector search");
            None
        };
        let embed_ms = embed_start.elapsed().as_millis() as u64;

        // 2. Build filters
        let filters = RetrievalFilters {
            document_types: req.filters.document_types.clone(),
            departments: req.filters.departments.clone(),
            languages: req.filters.languages.clone(),
            document_ids: req.filters.document_ids.clone(),
            sources: req.filters.sources.clone(),
            region: req.filters.region.clone(),
            workspaces: req.filters.workspaces.clone(),
            collections: req.filters.collections.clone(),
            acl_tags: req.filters.acl_tags.clone(),
        };
        let conditions = filters.to_qdrant_conditions();

        // 3-4. Independent search arms run CONCURRENTLY, then fuse sequentially.
        // Dense (main collection), sparse (BM25/lexical), wiki ANN, and visual
        // (Embed v4 query + ANN) are mutually independent I/O. `tokio::join!`
        // schedules them on one task so their round-trips overlap; ONLY the
        // scheduling changes. Gating, error semantics, and the fusion order are
        // identical to the prior sequential path:
        //   - dense + sparse errors are FATAL (propagated via `?`);
        //   - wiki + visual are NON-FATAL (any error/mismatch logs the same
        //     `warn!` and yields an empty arm, skipped in fusion).
        // `conditions` moves into the dense arm; `query_vector` is cloned per
        // arm exactly as the sequential code did with `qv.clone()`. The sparse
        // weight for RRF is the captured `w_bm25` (renormalized over dense+bm25).
        let arms_start = Instant::now();
        let (dense_res, sparse_res, graph_candidates, wiki_candidates, visual_candidates) = tokio::join!(
            self.arm_dense(&query_vector, &req.org_id, conditions, top_k),
            self.arm_sparse(&req.query, &req.org_id, top_k, route.sparse),
            self.arm_graph(
                &req.query,
                &req.org_id,
                req.verified_bearer.as_deref(),
                mix_for_scoring.w_graph,
                top_k,
            ),
            self.arm_wiki(&query_vector, &req.org_id, mix_for_scoring.w_wiki, top_k),
            self.arm_visual(
                &req.query,
                &req.org_id,
                mix_for_scoring.w_visual,
                embed_zdr,
                top_k,
            ),
        );
        // Dense + sparse are fatal — surface their errors just as the sequential
        // `?` did. `sparse_res` is `Ok(None)` when the sparse route was off and
        // `Ok(Some(..))` when it ran; that distinction drives fusion.
        let dense_candidates = dense_res?;
        let candidate_count_dense = dense_candidates.len();
        let sparse_opt = sparse_res?;
        // `dense_ms` now measures the concurrent arm phase (all four overlap).
        let dense_ms = arms_start.elapsed().as_millis() as u64;

        // Fuse the gathered arms sequentially: RRF(dense, sparse) with
        // renormalized bm25_share (or sparse-only / dense-only per route), then
        // RRF(_, graph) by w_graph, then RRF(_, wiki) by w_wiki, then
        // RRF(_, visual) by w_visual.
        let fusion_start = Instant::now();
        let (fused_candidates, candidate_count_sparse) = fuse_arms(
            dense_candidates,
            sparse_opt,
            graph_candidates,
            wiki_candidates,
            visual_candidates,
            &mix_for_scoring,
            route.dense,
        );

        // Visual rerank (ColQwen late-interaction / MaxSim). Reorders the
        // page-image candidates among themselves by ColQwen relevance, on top of
        // Embed-v4's first-stage order. Gated by VISUAL_RERANK_ENABLED; entirely
        // non-fatal (any failure or ZDR leaves the Embed-v4 order intact).
        let fused_candidates = if self.config.visual_rerank_enabled && self.colqwen.is_some() {
            self.visual_rerank(&req.query, fused_candidates, embed_zdr)
                .await
        } else {
            fused_candidates
        };
        // `sparse_ms` now measures the sequential fusion + visual-rerank phase.
        let sparse_ms = fusion_start.elapsed().as_millis() as u64;
        let candidate_count_fused = fused_candidates.len();

        // Over-fetch when a viewer is present so the step-6 ownership gate has
        // headroom and the response doesn't silently undershoot top_n after
        // filtering. Bounded at 1000 (`k_fetch = min(top_k*4, 1000)`). The
        // no-viewer (legacy org-scoped) path keeps its exact prior sizing.
        let overfetch = req.user_id.is_some();
        let fetch_k = if overfetch {
            (top_k.saturating_mul(4)).min(1000).max(top_k)
        } else {
            top_k
        };
        let rerank_out_n = if overfetch {
            (top_n.saturating_mul(4)).min(1000).max(top_n)
        } else {
            top_n
        };

        // Trim to the (possibly over-fetched) candidate pool before rerank
        let pre_rerank: Vec<ScoredCandidate> = fused_candidates.into_iter().take(fetch_k).collect();

        // 5. Rerank. Honor an explicit `mode_mix.rerank = false` (caller opts
        // out of the cross-encoder), and treat ANY reranker failure as
        // NON-FATAL: degrade to the fused RRF order rather than 500-ing the
        // whole retrieve when the rerank provider is unavailable or
        // misconfigured. Reranking refines ordering; it must never be able to
        // sink an otherwise-successful retrieval.
        let rerank_requested = text_rerank_allowed(
            zdr_mode,
            req.mode_mix.as_ref().and_then(|m| m.rerank).unwrap_or(true),
        );
        let rerank_start = Instant::now();
        let (reranked, rerank_used_count) = match self.reranker {
            Some(ref reranker) if rerank_requested => {
                let input_count = pre_rerank.len();
                match reranker.rerank(&req.query, &pre_rerank, rerank_out_n).await {
                    Ok(out) => (out, input_count),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "reranker failed; degrading to fused order"
                        );
                        (pre_rerank.into_iter().take(rerank_out_n).collect(), 0usize)
                    }
                }
            }
            _ => (pre_rerank.into_iter().take(rerank_out_n).collect(), 0usize),
        };
        let rerank_ms = rerank_start.elapsed().as_millis() as u64;

        // Wave 3.1 §15-G — publish per-query rerank cost event. Best-effort;
        // NATS unavailable does not fail the request.
        if rerank_used_count > 0 {
            if let (Some(nats), Some(reranker), Some(signer)) = (
                self.nats.as_ref(),
                self.reranker.as_ref(),
                self.event_signer.as_deref(),
            ) {
                let model = reranker.model_name().to_string();
                if let Ok(Some(payload)) = encode_cost_event(
                    signer,
                    &req.org_id,
                    req.user_id.as_deref(),
                    zdr_mode,
                    &model,
                    rerank_used_count,
                ) {
                    // §17.3.3 — named subject, lint-checked.
                    const SUBJECT_COST_LEDGER: &str = "dataplane.cost.ledger";
                    let _ = nats.publish(SUBJECT_COST_LEDGER, payload.into()).await;
                }
                crate::metrics::record_rerank_request();
            }
        }

        // 6. Canonical visibility gate (liveness + per-user ownership). Quickwit
        // and Qdrant are rebuildable read models, so stale hits can exist briefly
        // after a Postgres tombstone. Filter through canonical Postgres — and,
        // when a viewer is present, through the ownership predicate — before
        // applying ZDR and joining sources.
        //
        // Resolve the viewer's explicit grants first (always-on when a user_id is
        // present, decoupled from CONTROL_PLANE_ENFORCEMENT; fail-open to empty so
        // owner + org/shared visibility still apply). Then truncate the
        // over-fetched pool to top_n — counts/scores derive from this post-filter
        // set only, never from a pre-filter total or a non-visible backfill.
        // Org-admin super-visibility: when the verified `org:data:read_all` scope
        // is present, bypass the ownership predicate org-wide (still org-scoped,
        // never cross-org) and audit it. This is reached only on the human HTTP
        // path — the agent/api-key path never sets admin_read_all — so admin
        // bypass is EXCLUDED from agent grounding by construction.
        let (effective_viewer, granted_docs): (Option<&str>, Vec<String>) = if req.admin_read_all {
            tracing::warn!(
                org_id = %req.org_id,
                actor = req.user_id.as_deref().unwrap_or("unknown"),
                reason = "admin_bypass:read_all",
                "org-admin super-visibility: ownership post-filter bypassed (org-scoped, audited)"
            );
            (None, Vec::new())
        } else {
            let granted = match req.user_id.as_deref() {
                Some(uid) => {
                    self.visibility
                        .visible_documents(&req.org_id, uid, req.verified_bearer.as_deref())
                        .await
                }
                None => Vec::new(),
            };
            (req.user_id.as_deref(), granted)
        };
        let reranked = self
            .filter_live_candidates(reranked, effective_viewer, &granted_docs)
            .await?;
        let reranked: Vec<ScoredCandidate> = reranked.into_iter().take(top_n).collect();

        // 7. ZDR enforcement — filter out restricted documents.
        // §16.1.3 — also record what we actually did, so the audit trail can
        // distinguish "mode=reject but nothing to reject" from "mode=disabled"
        // from "mode=reject and 4 docs filtered".
        let mut zdr_actions_applied: Vec<&'static str> = Vec::new();
        let reranked = if zdr_mode == ZdrMode::Reject {
            let candidate_doc_ids: Vec<String> =
                reranked.iter().map(|c| c.document_id.clone()).collect();
            if !candidate_doc_ids.is_empty() {
                let restricted: std::collections::HashSet<String> = sqlx::query_as::<_, (String,)>(
                    "SELECT document_id FROM documents WHERE document_id = ANY($1) AND zdr_classification = 'restricted'"
                )
                .bind(&candidate_doc_ids)
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .map(|(id,)| id)
                .collect();

                if restricted.is_empty() {
                    zdr_actions_applied.push("reject_mode_no_restricted_found");
                    reranked
                } else {
                    zdr_actions_applied.push("reject_mode_filtered_restricted");
                    reranked
                        .into_iter()
                        .filter(|c| !restricted.contains(&c.document_id))
                        .collect()
                }
            } else {
                reranked
            }
        } else if zdr_mode == ZdrMode::Ephemeral {
            zdr_actions_applied.push("ephemeral_no_trace_persist");
            reranked
        } else {
            reranked
        };
        let candidate_count_reranked = reranked.len();

        // 8. Confidence gate
        let low_confidence = reranked
            .first()
            .map(|c| c.rerank_score < self.config.confidence_threshold)
            .unwrap_or(true);

        // 9. Source join from Postgres
        let source_start = Instant::now();
        let sources = self.join_sources(&reranked).await?;
        let source_join_ms = source_start.elapsed().as_millis() as u64;

        let total_ms = pipeline_start.elapsed().as_millis() as u64;

        let timings = PipelineTimings {
            embed_ms,
            dense_ms,
            sparse_ms,
            fusion_ms: 0,
            rerank_ms,
            source_join_ms,
            total_ms,
            candidate_count_dense,
            candidate_count_sparse,
            candidate_count_fused,
            candidate_count_reranked,
        };

        // 10. Context packing (if budget requested)
        let context_pack = if let Some(budget) = req.context_budget_tokens {
            let format = req
                .context_format
                .clone()
                .unwrap_or_else(|| "json".to_string());
            Some(pack_context(&reranked, &sources, budget, &format))
        } else {
            None
        };

        // Reuse the mix resolved before fusion so scoring and trace agree.
        let resolved_mix = mix_for_scoring;

        // 11. Persist retrieval trace (skip in ephemeral ZDR mode).
        // Best-effort: a trace persistence failure must NOT fail the retrieval —
        // the user gets candidates, we log the trace error and synthesize a
        // fallback trace_id so downstream "/retrieval/{trace_id}" gracefully
        // returns NotFound rather than the user's query failing entirely.
        let trace_id = if zdr_mode.is_ephemeral() {
            format!("ephemeral-{}", uuid::Uuid::new_v4())
        } else {
            match persist_trace(
                &self.pool,
                &req,
                &reranked,
                &timings,
                self.reranker.as_ref().map(|r| r.model_name()),
                zdr_mode.as_str(),
                Some(&resolved_mix),
                &zdr_actions_applied,
            )
            .await
            {
                Ok(id) => id,
                Err(e) => {
                    tracing::error!(error = %e, org_id = %req.org_id, "trace persistence failed; returning unpersisted trace_id");
                    crate::metrics::record_trace_persist_failure(&req.org_id);
                    format!("unpersisted-{}", uuid::Uuid::new_v4())
                }
            }
        };

        let index_version = "v2-current".to_string();

        crate::metrics::record_retrieval(
            total_ms as f64 / 1000.0,
            candidate_count_reranked,
            &req.org_id,
        );
        if candidate_count_reranked == 0 {
            crate::metrics::record_zero_results(&req.org_id);
        }

        tracing::info!(
            trace_id = %trace_id,
            total_ms,
            candidates = candidate_count_reranked,
            low_confidence,
            "retrieval.complete"
        );

        // Agent retrieval planner hints (D4+D5 spec §3). Heuristic-only —
        // we surface honest follow-up suggestions based on signal in the
        // current response, never synthesized.
        let mut suggested_next_tools: Vec<String> = Vec::new();
        if low_confidence {
            // Low rerank confidence → try graph expansion + wiki to cover
            // semantically adjacent material.
            suggested_next_tools.push("/v1/retrieve/graph".into());
            suggested_next_tools.push("/v1/retrieve/wiki".into());
        }
        if sources.len() >= 3 {
            // Multiple sources → worth checking for cross-source disagreement.
            suggested_next_tools.push("/v1/retrieve/contradictions".into());
        }
        if reranked.is_empty() {
            // No candidates at all → broaden via wiki/global search.
            suggested_next_tools.push("/v1/retrieve/wiki".into());
            suggested_next_tools.push("/v1/knowledge/search".into());
        }

        Ok(RetrievalResponse {
            candidates: reranked,
            sources,
            query: req.query,
            org_id: req.org_id,
            trace_id,
            index_version,
            zdr_mode: zdr_mode.as_str().to_owned(),
            // Surface the SAME enforcement actions already persisted on the
            // trace (§16.1.3) — the real computed value, not a synthesized one.
            zdr_actions_applied: zdr_actions_applied
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            low_confidence,
            context_pack,
            suggested_next_tools,
        })
    }

    #[tracing::instrument(
        name = "postgres.join_sources",
        skip_all,
        fields(otel.kind = "client", db.system = "postgresql", candidate_count = candidates.len()),
    )]
    pub async fn join_sources(
        &self,
        candidates: &[ScoredCandidate],
    ) -> anyhow::Result<Vec<SourceRef>> {
        let doc_ids: Vec<String> = candidates
            .iter()
            .map(|c| c.document_id.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if doc_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut rows = sqlx::query_as::<_, SourceRow>(
            r#"
            SELECT document_id, title, source, type
            FROM documents
            WHERE document_id = ANY($1)
              AND deleted_at IS NULL
            "#,
        )
        .bind(&doc_ids)
        .fetch_all(&self.pool)
        .await?;

        // Wiki candidates (from the wiki ANN arm) carry document_id = wiki
        // page_id, which lives in wiki_pages, not documents. Look those up too
        // so wiki citations get a real title + path + type="wiki" instead of a
        // bare id. Best-effort: a failure here just omits the wiki title.
        match sqlx::query_as::<_, SourceRow>(
            r#"
            SELECT page_id AS document_id, title, path AS source, 'wiki' AS type
            FROM wiki_pages
            WHERE page_id = ANY($1)
            "#,
        )
        .bind(&doc_ids)
        .fetch_all(&self.pool)
        .await
        {
            Ok(wiki_rows) => rows.extend(wiki_rows),
            Err(e) => tracing::warn!(error = %e, "wiki source join failed; omitting wiki titles"),
        }

        Ok(rows
            .into_iter()
            .map(|r| SourceRef {
                document_id: r.document_id,
                title: r.title,
                source: r.source,
                r#type: r.r#type,
            })
            .collect())
    }

    /// Step-6 canonical visibility gate. Runs POST-fusion + POST-rerank over the
    /// unified dense+sparse+wiki candidate list, so it gates every retrieval arm
    /// uniformly (closing the sparse leak for free). Two passes:
    ///
    ///   1. Liveness — drop candidates absent from canonical `documents`
    ///      (Qdrant/Quickwit are rebuildable read models that can lag a delete).
    ///   2. Per-user OWNERSHIP — when `viewer` is present, keep a document only
    ///      if `owner_id = viewer OR visibility = 'org' OR it is in
    ///      `granted_ids` (explicit resource_grants). 'shared' docs are NOT
    ///      org-readable — they reach recipients ONLY via a grant. Always-on when a viewer is
    ///      present, decoupled from `CONTROL_PLANE_ENFORCEMENT`. When `viewer` is
    ///      `None` the ownership predicate is a no-op (legacy org-scoped path).
    ///
    /// Published wiki pages stay via their own branch — wiki is org-shared
    /// knowledge, not an ownable resource type. This NEVER backfills with
    /// non-visible docs; it only removes, so a low-visibility user honestly
    /// undershoots rather than seeing someone else's data.
    async fn filter_live_candidates(
        &self,
        candidates: Vec<ScoredCandidate>,
        viewer: Option<&str>,
        granted_ids: &[String],
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        let doc_ids: Vec<String> = candidates
            .iter()
            .map(|c| c.document_id.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if doc_ids.is_empty() {
            return Ok(candidates);
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
        .bind(viewer)
        .bind(granted_ids)
        .fetch_all(&self.pool)
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
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => live.extend(rows.into_iter().map(|(id,)| id)),
            Err(e) => {
                tracing::warn!(error = %e, "wiki live-gate lookup failed; wiki candidates may be dropped")
            }
        }

        if live.len() == doc_ids.len() {
            return Ok(candidates);
        }

        let before = candidates.len();
        let filtered: Vec<ScoredCandidate> = candidates
            .into_iter()
            .filter(|c| live.contains(&c.document_id))
            .collect();
        // Raw-vs-survivor on the trace. When a viewer is present and a large
        // fraction was dropped, flag potential top-k starvation (the response
        // will honestly undershoot rather than backfill non-visible docs).
        let after = filtered.len();
        let viewer_present = viewer.is_some();
        if viewer_present && after * 2 < before {
            tracing::warn!(
                before,
                after,
                viewer_present,
                "ownership/liveness gate dropped >50% of candidates — possible top-k starvation; result honestly undershoots"
            );
        } else {
            tracing::debug!(
                before,
                after,
                viewer_present,
                "step-6 visibility gate applied"
            );
        }
        Ok(filtered)
    }
}

#[derive(sqlx::FromRow)]
struct SourceRow {
    document_id: String,
    title: String,
    source: String,
    r#type: String,
}

#[cfg(test)]
mod tests {
    use super::{
        embed_zdr_for_mode, embedding_cache_allowed, encode_cost_event, fuse_arms,
        reciprocal_rank_fusion, text_rerank_allowed, ResolvedWeights, ScoredCandidate, ZdrMode,
    };
    use event_envelope_rs::{EventSigner, EventVerifier};
    use rsa::{
        pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey},
        rand_core::OsRng,
        RsaPrivateKey, RsaPublicKey,
    };

    #[test]
    fn ephemeral_mode_drives_embed_zdr_true() {
        assert!(embed_zdr_for_mode(ZdrMode::Ephemeral));
    }

    #[test]
    fn restrictive_modes_drive_retaining_egress_guard() {
        assert!(embed_zdr_for_mode(ZdrMode::Reject));
        assert!(embed_zdr_for_mode(ZdrMode::Ephemeral));
        assert!(!embed_zdr_for_mode(ZdrMode::Disabled));
    }

    #[test]
    fn restrictive_modes_never_invoke_text_reranker() {
        assert!(!text_rerank_allowed(ZdrMode::Reject, true));
        assert!(!text_rerank_allowed(ZdrMode::Ephemeral, true));
        assert!(text_rerank_allowed(ZdrMode::Disabled, true));
        assert!(!text_rerank_allowed(ZdrMode::Disabled, false));
    }

    #[test]
    fn ephemeral_queries_bypass_embedding_cache_reads_and_writes() {
        assert!(!embedding_cache_allowed(true));
        assert!(embedding_cache_allowed(false));
    }

    #[test]
    fn cost_events_are_signed_per_tenant_and_suppressed_for_restrictive_zdr() {
        let private = RsaPrivateKey::new(&mut OsRng, 2048).expect("test RSA key");
        let public = RsaPublicKey::from(&private);
        let private_pem = private
            .to_pkcs1_pem(Default::default())
            .expect("private PEM");
        let public_pem = public.to_pkcs1_pem(Default::default()).expect("public PEM");
        let signer = EventSigner::from_rsa_pem(
            private_pem.as_bytes(),
            "service:retrieval-engine-rs",
            "retrieval-events-v1",
            "dataplane-events",
            "events:retrieval:publish",
        )
        .expect("signer");
        let verifier = EventVerifier::from_rsa_pem(
            public_pem.as_bytes(),
            "service:retrieval-engine-rs",
            "retrieval-events-v1",
            "dataplane-events",
            "events:retrieval:publish",
            16,
        )
        .expect("verifier");

        let envelope = encode_cost_event(
            &signer,
            "org-test",
            Some("user-test"),
            ZdrMode::Disabled,
            "rerank-test",
            2,
        )
        .expect("cost event")
        .expect("durable posture emits event");
        let verified = verifier
            .verify("dataplane.cost.ledger", &envelope)
            .expect("verified cost event");
        let payload: serde_json::Value =
            serde_json::from_slice(&verified.payload).expect("payload JSON");
        assert_eq!(payload["org_id"], "org-test");
        assert_eq!(payload["zdr"], false);
        assert!(payload.get("org_ids").is_none());

        assert!(encode_cost_event(
            &signer,
            "org-test",
            Some("user-test"),
            ZdrMode::Ephemeral,
            "rerank-test",
            2,
        )
        .expect("restricted event result")
        .is_none());
    }

    // --- fuse_arms: proves the concurrent arms fuse in the same order/gating
    // as the original sequential chain. Fixtures are built directly; the
    // "oracle" is the same `reciprocal_rank_fusion` composed by hand in order.

    fn cand(knowledge_id: &str, document_id: &str, score: f32) -> ScoredCandidate {
        ScoredCandidate {
            knowledge_id: knowledge_id.to_string(),
            document_id: document_id.to_string(),
            text: String::new(),
            dense_score: score,
            sparse_score: 0.0,
            rerank_score: 0.0,
            final_score: score,
            chunk_index: 0,
            metadata: std::collections::HashMap::new(),
        }
    }

    fn mix(w_dense: f32, w_bm25: f32, w_wiki: f32, w_visual: f32) -> ResolvedWeights {
        ResolvedWeights {
            w_dense,
            w_bm25,
            w_graph: 0.0,
            w_wiki,
            w_visual,
            rerank: true,
        }
    }

    /// Compare two candidate lists by the fields fusion actually determines:
    /// identity order and final score. (ScoredCandidate has no PartialEq and
    /// carries an opaque metadata map, so we project to a comparable shape.)
    fn project(v: &[ScoredCandidate]) -> Vec<(String, f32)> {
        v.iter()
            .map(|c| (c.knowledge_id.clone(), c.final_score))
            .collect()
    }

    fn bm25_share(m: &ResolvedWeights) -> f32 {
        let d = m.w_dense;
        let b = m.w_bm25;
        let sum = (d + b).max(f32::EPSILON);
        b / sum
    }

    fn dense_fixture() -> Vec<ScoredCandidate> {
        vec![
            cand("k-d1", "doc-d1", 0.90),
            cand("k-d2", "doc-d2", 0.50),
            cand("k-shared", "doc-shared", 0.30),
        ]
    }

    fn sparse_fixture() -> Vec<ScoredCandidate> {
        vec![
            cand("k-s1", "doc-s1", 4.0),
            cand("k-shared", "doc-shared", 3.0),
            cand("k-s2", "doc-s2", 2.0),
        ]
    }

    #[test]
    fn fuse_dense_and_sparse_matches_sequential_rrf() {
        let dense = dense_fixture();
        let sparse = sparse_fixture();
        let m = mix(0.7, 0.3, 0.0, 0.0);
        let expected = reciprocal_rank_fusion(&dense, &sparse, 60.0, bm25_share(&m));

        let (got, sparse_count) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            vec![],
            vec![],
            &m,
            true,
        );

        assert_eq!(sparse_count, sparse.len());
        assert_eq!(project(&got), project(&expected));
    }

    #[test]
    fn fuse_layers_wiki_after_dense_sparse() {
        let dense = dense_fixture();
        let sparse = sparse_fixture();
        let wiki = vec![
            cand("k-w1", "doc-w1", 0.8),
            cand("k-shared", "doc-shared", 0.7),
        ];
        let m = mix(0.7, 0.3, 0.5, 0.0);

        let base = reciprocal_rank_fusion(&dense, &sparse, 60.0, bm25_share(&m));
        let expected = reciprocal_rank_fusion(&base, &wiki, 60.0, m.w_wiki);

        let (got, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            wiki.clone(),
            vec![],
            &m,
            true,
        );

        assert_eq!(project(&got), project(&expected));
    }

    #[test]
    fn fuse_layers_visual_after_wiki() {
        let dense = dense_fixture();
        let sparse = sparse_fixture();
        let wiki = vec![cand("k-w1", "doc-w1", 0.8)];
        let visual = vec![cand("k-v1", "doc-v1", 0.6)];
        let m = mix(0.6, 0.2, 0.4, 0.3);

        let base = reciprocal_rank_fusion(&dense, &sparse, 60.0, bm25_share(&m));
        let with_wiki = reciprocal_rank_fusion(&base, &wiki, 60.0, m.w_wiki);
        let expected = reciprocal_rank_fusion(&with_wiki, &visual, 60.0, m.w_visual);

        let (got, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            wiki.clone(),
            visual.clone(),
            &m,
            true,
        );

        assert_eq!(project(&got), project(&expected));
    }

    #[test]
    fn fuse_sparse_only_route_returns_sparse_unchanged() {
        let dense = dense_fixture();
        let sparse = sparse_fixture();
        // Sparse-only route (route_dense = false): sparse list is returned raw,
        // with no RRF re-weighting.
        let m = mix(0.0, 1.0, 0.0, 0.0);

        let (got, sparse_count) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            vec![],
            vec![],
            &m,
            false,
        );

        assert_eq!(sparse_count, sparse.len());
        assert_eq!(project(&got), project(&sparse));
    }

    #[test]
    fn fuse_dense_only_when_sparse_arm_did_not_run() {
        let dense = dense_fixture();
        let m = mix(1.0, 0.0, 0.0, 0.0);

        // sparse = None (route off): dense returned raw, count 0, no RRF.
        let (got, sparse_count) = fuse_arms(dense.clone(), None, vec![], vec![], vec![], &m, true);
        assert_eq!(sparse_count, 0);
        assert_eq!(project(&got), project(&dense));

        // Contrast: sparse = Some(empty) (route on, zero hits) STILL runs RRF,
        // re-weighting dense — proving the Option distinction is load-bearing.
        let m2 = mix(0.7, 0.3, 0.0, 0.0);
        let expected_empty_rrf = reciprocal_rank_fusion(&dense, &[], 60.0, bm25_share(&m2));
        let (got_empty, count_empty) = fuse_arms(
            dense.clone(),
            Some(vec![]),
            vec![],
            vec![],
            vec![],
            &m2,
            true,
        );
        assert_eq!(count_empty, 0);
        assert_eq!(project(&got_empty), project(&expected_empty_rrf));
        assert_ne!(project(&got_empty), project(&dense));
    }

    #[test]
    fn fuse_skips_wiki_and_visual_when_gated_off_or_empty() {
        let dense = dense_fixture();
        let sparse = sparse_fixture();
        let base_mix = mix(0.7, 0.3, 0.0, 0.0);
        let expected = reciprocal_rank_fusion(&dense, &sparse, 60.0, bm25_share(&base_mix));

        // w_wiki = 0 but a wiki list IS supplied → wiki must be ignored.
        let (got_weight_off, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            vec![cand("k-w1", "doc-w1", 0.9)],
            vec![],
            &base_mix,
            true,
        );
        assert_eq!(project(&got_weight_off), project(&expected));

        // w_wiki/w_visual > 0 but the arms returned nothing → both skipped.
        let m = mix(0.6, 0.2, 0.5, 0.3);
        let expected_empty_arms = reciprocal_rank_fusion(&dense, &sparse, 60.0, bm25_share(&m));
        let (got_empty_arms, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            vec![],
            vec![],
            &m,
            true,
        );
        assert_eq!(project(&got_empty_arms), project(&expected_empty_arms));
    }

    fn mix_graph(w_dense: f32, w_bm25: f32, w_graph: f32) -> ResolvedWeights {
        ResolvedWeights {
            w_dense,
            w_bm25,
            w_graph,
            w_wiki: 0.0,
            w_visual: 0.0,
            rerank: true,
        }
    }

    #[test]
    fn fuse_layers_graph_after_dense_sparse() {
        let dense = dense_fixture();
        let sparse = sparse_fixture();
        let graph = vec![
            cand("k-g1", "doc-g1", 0.9),
            cand("k-shared", "doc-shared", 0.5),
        ];
        let m = mix_graph(0.6, 0.2, 0.4);

        // Oracle: dense+sparse RRF, THEN graph RRF by w_graph (peer position,
        // before the empty wiki/visual arms).
        let base = reciprocal_rank_fusion(&dense, &sparse, 60.0, bm25_share(&m));
        let expected = reciprocal_rank_fusion(&base, &graph, 60.0, m.w_graph);

        let (got, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            graph.clone(),
            vec![],
            vec![],
            &m,
            true,
        );

        assert_eq!(project(&got), project(&expected));
    }

    #[test]
    fn fuse_skips_graph_when_weight_zero_or_empty() {
        let dense = dense_fixture();
        let sparse = sparse_fixture();

        // w_graph = 0 but a graph list IS supplied → graph must be ignored.
        let base_mix = mix(0.7, 0.3, 0.0, 0.0);
        let expected = reciprocal_rank_fusion(&dense, &sparse, 60.0, bm25_share(&base_mix));
        let (got_weight_off, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![cand("k-g1", "doc-g1", 0.9)],
            vec![],
            vec![],
            &base_mix,
            true,
        );
        assert_eq!(project(&got_weight_off), project(&expected));

        // w_graph > 0 but the graph arm returned nothing → skipped.
        let m = mix_graph(0.6, 0.2, 0.4);
        let expected_empty = reciprocal_rank_fusion(&dense, &sparse, 60.0, bm25_share(&m));
        let (got_empty, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            vec![],
            vec![],
            &m,
            true,
        );
        assert_eq!(project(&got_empty), project(&expected_empty));
    }
}
