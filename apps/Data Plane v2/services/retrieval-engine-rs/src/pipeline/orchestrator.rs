use std::time::Instant;

use event_envelope_rs::EventSigner;
use qdrant_client::qdrant::Condition;
use qdrant_client::Qdrant;
use sqlx::PgPool;

use crate::cache::CacheLayer;
use crate::config::Config;
use crate::context_pack::pack_context_with_pins;
use crate::embed::EmbeddingClient;
use crate::pipeline::postprocess;
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

/// Whether a retrieval *result* may be written to / served from the shared
/// Dragonfly tier.
///
/// Deliberately the same rule as [`embedding_cache_allowed`] — any restrictive
/// posture disqualifies the query — rather than the narrower "ephemeral only"
/// rule that governs trace persistence. Two reasons the stricter line is the
/// right one here:
///
/// * A cached retrieval result is a durable copy of *document text* (candidate
///   `text`, the packed context) living outside Postgres for the TTL. Trace rows
///   store scores and ids; this stores content. Reject mode exists precisely
///   because the caller's posture says restricted content must not be handled
///   loosely, so materializing it into a second store is the wrong default.
/// * A Reject-mode result is a *filtered* view. Sharing entries between
///   postures is already prevented by `zdr_mode` being in the cache key, but
///   keeping restrictive postures out of the tier entirely means there is no
///   filtered-vs-unfiltered payload adjacency to reason about at all.
///
/// The cost is that restrictive-posture queries always pay full pipeline price.
/// That is the intended trade: correctness and a small audit surface over hit
/// rate on the minority of queries that asked for stricter handling.
fn retrieval_cache_allowed(zdr_mode: ZdrMode) -> bool {
    !zdr_mode.restricts_egress()
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
    /// Audio/video RAG arm query embedder (self-hosted `media-embedder`:
    /// LAION-CLAP + SigLIP 2). `None` when `MEDIA_EMBEDDER_ENDPOINT` is
    /// unset. When present and `w_audio`/`w_video` > 0 the orchestrator embeds
    /// the query into the matching shared space and fuses segment hits from
    /// `qdrant_audio_collection` / `qdrant_video_collection`.
    pub media_embedder: Option<crate::embed::media::MediaQueryEmbedder>,
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
    /// Keyword arm query client (Meilisearch, typo-tolerant exact-ID/code
    /// lookup). `None` when `MEILISEARCH_URL`/`MEILISEARCH_API_KEY` aren't
    /// both configured — text-only and pre-Meilisearch deployments are
    /// unaffected, exactly like `visual_embedder`'s `None` path.
    pub keyword_client: Option<crate::search::keyword::MeilisearchQueryClient>,
}

/// A page-image candidate's fetchable `image_url`, read from its raw Qdrant
/// payload metadata — or `None` if it isn't a page-image candidate / has no URL.
pub(super) fn page_image_url(c: &ScoredCandidate) -> Option<String> {
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

/// Pure placement of ColQwen (late-interaction MaxSim) scores into the fused
/// candidate list — kept separate from the `postprocess::VisualRerank` stage
/// that calls it, so both modes are unit-testable without a live ColQwen server.
///
/// - **Band mode** (`joint = false`, the historical behavior): the visual
///   candidates are reordered *among themselves* by ColQwen relevance but keep
///   the fused-score band they already occupy — a visual hit can never
///   leapfrog a text hit.
/// - **Joint mode** (`joint = true`, default): ColQwen scores are min-max
///   mapped onto the fused list's global score range and the whole list is
///   re-sorted — true joint text-vs-image ordering, where a strongly relevant
///   page image CAN outrank weaker text candidates (and a weak one can sink).
///   Degenerate spreads (all ColQwen scores equal, or a flat fused range)
///   fall back to band mode rather than fabricate an ordering.
///
/// `idxs[k]` is the fused-list slot of the visual candidate scored `scores[k]`.
pub(super) fn apply_colqwen_scores(
    mut fused: Vec<ScoredCandidate>,
    idxs: &[usize],
    scores: &[f32],
    joint: bool,
) -> Vec<ScoredCandidate> {
    if idxs.is_empty() || idxs.len() != scores.len() {
        return fused;
    }

    let cq_min = scores.iter().copied().fold(f32::INFINITY, f32::min);
    let cq_max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let g_min = fused
        .iter()
        .map(|c| c.final_score)
        .fold(f32::INFINITY, f32::min);
    let g_max = fused
        .iter()
        .map(|c| c.final_score)
        .fold(f32::NEG_INFINITY, f32::max);

    if joint && cq_max > cq_min && g_max > g_min {
        // Joint text-vs-image: map each ColQwen score onto the global fused
        // score range, then re-sort the whole list.
        for (k, &slot) in idxs.iter().enumerate() {
            let mapped = g_min + (scores[k] - cq_min) / (cq_max - cq_min) * (g_max - g_min);
            fused[slot].rerank_score = scores[k];
            fused[slot].final_score = mapped;
        }
        fused.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        return fused;
    }

    // Band mode: pair each visual candidate with its ColQwen score, sort
    // best-first, and write them back into the visual slots by descending band
    // score — only the order among the visual candidates changes.
    let mut ranked: Vec<(ScoredCandidate, f32)> = idxs
        .iter()
        .enumerate()
        .map(|(k, &i)| (fused[i].clone(), scores[k]))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut band: Vec<f32> = idxs.iter().map(|&i| fused[i].final_score).collect();
    band.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    for (rank, &slot) in idxs.iter().enumerate() {
        let (mut cand, cq) = ranked[rank].clone();
        cand.rerank_score = cq;
        cand.final_score = band[rank];
        fused[slot] = cand;
    }
    fused
}

/// Weighted average of two same-length embedding vectors (P2-7 HyDE blend).
/// Cosine distance — the metric on every DPv2 Qdrant collection — is
/// scale-invariant, so the blended vector needs no renormalization before
/// the ANN search that follows.
///
/// `query`/`expansion` are always the same length in practice: both come
/// from `embed_text_cached`, which validates every vector it returns against
/// `config.embedding_dimension` before this function ever sees one. Zips
/// rather than asserts, so a hypothetical future caller that violates that
/// invariant gets a silently-truncated blend instead of a panic — a length
/// mismatch here would be a bug elsewhere, not a condition worth crashing
/// the request over.
fn blend_vectors(query: &[f32], expansion: &[f32], weight_expansion: f32) -> Vec<f32> {
    let weight_query = 1.0 - weight_expansion;
    query
        .iter()
        .zip(expansion)
        .map(|(q, e)| q * weight_query + e * weight_expansion)
        .collect()
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
///   5. keyword RRF (k=60, `w_keyword`) when `w_keyword > 0` and the Meilisearch
///      keyword arm returned hits. Last in fusion order because it is the
///      newest, least-proven arm — same reasoning `w_visual` landed last
///      before it, kept consistent by adding new arms at the end rather than
///      reordering the established chain.
///
/// Returns the fused list plus the sparse candidate count for the trace. The
/// `Option` on `sparse` is load-bearing: `None` (route off) yields the raw dense
/// list, whereas `Some(empty)` (route on, zero hits) still runs RRF and thus
/// re-weights the dense scores — the two are NOT interchangeable.
#[allow(clippy::too_many_arguments)] // one Vec per fused arm; a struct would just move the same count elsewhere
fn fuse_arms(
    dense: Vec<ScoredCandidate>,
    sparse: Option<Vec<ScoredCandidate>>,
    graph: Vec<ScoredCandidate>,
    wiki: Vec<ScoredCandidate>,
    visual: Vec<ScoredCandidate>,
    keyword: Vec<ScoredCandidate>,
    mix: &ResolvedWeights,
    route_dense: bool,
) -> (Vec<ScoredCandidate>, usize) {
    // P2-5: `k` now travels on the resolved mix (and into the trace) instead of
    // being a literal at each fusion call.
    let rrf_k = mix.rrf_k;

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
                    reciprocal_rank_fusion(&dense, &sparse_candidates, rrf_k, bm25_share),
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
        fused = reciprocal_rank_fusion(&fused, &graph, rrf_k, mix.w_graph);
    }

    // Step 5 — wiki 4-way merge by its w_wiki share.
    if mix.w_wiki > 0.0 && !wiki.is_empty() {
        fused = reciprocal_rank_fusion(&fused, &wiki, rrf_k, mix.w_wiki);
    }

    // Visual arm — layer Embed v4 page-image hits by w_visual (purely additive).
    if mix.w_visual > 0.0 && !visual.is_empty() {
        fused = reciprocal_rank_fusion(&fused, &visual, rrf_k, mix.w_visual);
    }

    // Keyword arm — layer Meilisearch typo-tolerant hits by w_keyword (purely
    // additive, same peer position as graph/wiki/visual).
    if mix.w_keyword > 0.0 && !keyword.is_empty() {
        fused = reciprocal_rank_fusion(&fused, &keyword, rrf_k, mix.w_keyword);
    }

    (fused, sparse_count)
}

impl RetrievalPipeline {
    /// Embeds `text` with cache + ZDR-admission handling, then validates the
    /// response shape. Shared by the raw query embed and the P2-7 query
    /// expansion embed in `retrieve()` — both need identical cache/ZDR/
    /// dimension-validation behavior, just applied to different text, and
    /// this is the single place that behavior lives.
    async fn embed_text_cached(
        &self,
        org_id: &str,
        text: &str,
        embed_zdr: bool,
    ) -> anyhow::Result<Vec<f32>> {
        let text_hash = crate::cache::hash_text(text);
        let model_ver = self.embedder.cache_namespace();
        let vec = if embedding_cache_allowed(embed_zdr) {
            if let Some(ref cache) = self.cache {
                if let Some(cached) = cache.get_embedding(&model_ver, &text_hash).await {
                    tracing::debug!("embed cache hit");
                    crate::metrics::record_embed_cache_hit();
                    cached
                } else {
                    crate::metrics::record_embed_request();
                    let vec = self.embedder.embed_query(org_id, text, embed_zdr).await?;
                    cache.set_embedding(&model_ver, &text_hash, &vec).await;
                    vec
                }
            } else {
                crate::metrics::record_embed_request();
                self.embedder.embed_query(org_id, text, embed_zdr).await?
            }
        } else {
            // ZDR is a cache-admission decision, not just an embedding-provider
            // flag. Skip both reads and writes so an ephemeral request never
            // touches durable Dragonfly state (including a pre-existing key).
            crate::metrics::record_embed_request();
            self.embedder.embed_query(org_id, text, embed_zdr).await?
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
        Ok(vec)
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

    /// Media arm — self-hosted CLAP/SigLIP 2 query embedding + the matching
    /// segment-collection ANN. Gated on the arm's weight AND a configured
    /// `media_embedder`. NON-FATAL throughout, exactly like `arm_visual`: a
    /// skip, dim mismatch, embed error, Qdrant error or empty result all yield an
    /// empty list rather than failing the query.
    ///
    /// Unlike `arm_visual` there is no ZDR gate — the embedder is local, so
    /// nothing egresses and a restricted query is safe to embed here.
    async fn arm_media(
        &self,
        query: &str,
        org_id: &str,
        space: crate::embed::media::MediaSpace,
        weight: f32,
        top_k: usize,
    ) -> Vec<ScoredCandidate> {
        if weight <= 0.0 {
            return Vec::new();
        }
        let Some(ref me) = self.media_embedder else {
            return Vec::new();
        };
        use crate::embed::media::MediaSpace;
        let (collection, want_dim) = match space {
            MediaSpace::Audio => (
                &self.config.qdrant_audio_collection,
                self.config.audio_embedding_dimension,
            ),
            MediaSpace::Video => (
                &self.config.qdrant_video_collection,
                self.config.video_embedding_dimension,
            ),
        };
        let arm = match space {
            MediaSpace::Audio => "audio",
            MediaSpace::Video => "video",
        };
        match me.embed_query(query, space).await {
            // The tower's dim must match the collection's, or every candidate
            // would error individually; skip the arm cleanly instead. This also
            // catches a tower swap (e.g. CLAP -> GLAP) that outgrew its
            // collection.
            Ok(vec_q) if vec_q.len() == want_dim => {
                match vector_search(&self.qdrant, collection, vec_q, org_id, Vec::new(), top_k)
                    .await
                {
                    Ok(hits) if !hits.is_empty() => hits,
                    Ok(_) => Vec::new(),
                    Err(e) => {
                        tracing::warn!(error = %e, arm, "media ANN arm failed; skipping");
                        Vec::new()
                    }
                }
            }
            Ok(vec_q) => {
                tracing::warn!(
                    got = vec_q.len(),
                    want = want_dim,
                    arm,
                    "media query embedding dim mismatch; skipping media arm"
                );
                Vec::new()
            }
            Err(e) => {
                tracing::warn!(error = %e, arm, "media query embed failed; skipping media arm");
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

    /// Keyword arm — Meilisearch typo-tolerant exact-ID/code lookup over the
    /// query TEXT (no embedding), so it runs regardless of the dense route —
    /// same shape as the graph arm. Gated on `w_keyword > 0` AND a configured
    /// `keyword_client`. NON-FATAL: on skip, an HTTP error, or an empty result
    /// it returns an empty list, exactly like the wiki/visual arms — a
    /// Meilisearch outage degrades the blend, it never fails the request.
    async fn arm_keyword(
        &self,
        query: &str,
        org_id: &str,
        w_keyword: f32,
        top_k: usize,
    ) -> Vec<ScoredCandidate> {
        if w_keyword <= 0.0 {
            return Vec::new();
        }
        let Some(ref client) = self.keyword_client else {
            return Vec::new();
        };
        match crate::search::keyword::keyword_arm_candidates(client, query, org_id, top_k as i64)
            .await
        {
            Ok(keyword) => keyword,
            Err(e) => {
                tracing::warn!(error = %e, "keyword arm (Meilisearch) failed; skipping");
                Vec::new()
            }
        }
    }

    // `cached` and `mix` are SKIPPED, not just `self`/`req`.
    //
    // `#[tracing::instrument]` records every non-skipped argument via `Debug`,
    // and `CachedRetrieval` holds each candidate's full `text` plus the packed
    // context — so omitting it from `skip` emitted retrieved DOCUMENT CONTENT
    // into the application log on every cache hit, at INFO, in a shared stdout
    // stream. That is the same leak `sanitized_provider_status_error` and the
    // `query_len`-not-`query` choice on this very attribute exist to prevent.
    // Restrictive ZDR postures never populate this tier (`retrieval_cache_allowed`),
    // so the exposure was non-ZDR org content only — still org-scoped material
    // in an operator-readable stream, which is not something to log by accident.
    //
    // The safe summary below is recorded instead; anything added to this
    // signature later must be skipped or proven non-sensitive.
    #[tracing::instrument(
        name = "retrieval.pipeline",
        skip(self, req, cached, mix),
        fields(
            org_id = req.org_id.as_str(),
            query_len = req.query.len(),
            zdr_mode = req.zdr_mode.unwrap_or_default().as_str(),
            cached_candidates = cached.candidates.len(),
            cached_low_confidence = cached.low_confidence,
        ),
    )]
    /// Rebuilds a full [`RetrievalResponse`] from a cache hit.
    ///
    /// A hit is NOT simply the stored payload handed back: this request gets its
    /// own freshly persisted trace. That matters for two reasons — the caller
    /// must never receive an earlier caller's `trace_id` (see
    /// [`crate::cache::CachedRetrieval`]), and a served retrieval that left no
    /// audit row would make the cache a hole in the audit trail, where the more
    /// cache-effective a deployment got the less of its retrieval history was
    /// recorded.
    ///
    /// No cost event is emitted, because no provider was called. The reranker
    /// bill was paid by the request that populated the entry; charging again for
    /// work nobody performed would be double billing.
    async fn respond_from_cache(
        &self,
        req: &RetrievalRequest,
        cached: crate::cache::CachedRetrieval,
        zdr_mode: ZdrMode,
        mix: &ResolvedWeights,
    ) -> anyhow::Result<RetrievalResponse> {
        // Timings are genuinely near-zero on this path and are reported as
        // such rather than replaying the original request's durations, which
        // would make cache hits invisible in latency telemetry.
        let timings = PipelineTimings::default();
        let zdr_action_refs: Vec<&str> = cached
            .zdr_actions_applied
            .iter()
            .map(String::as_str)
            .collect();
        let trace_id = match persist_trace(
            &self.pool,
            req,
            &cached.candidates,
            &timings,
            self.reranker.as_ref().map(|r| r.model_name()),
            zdr_mode.as_str(),
            Some(mix),
            &zdr_action_refs,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => {
                tracing::error!(error = %e, org_id = %req.org_id, "trace persistence failed on cache hit; returning unpersisted trace_id");
                crate::metrics::record_trace_persist_failure(&req.org_id);
                format!("unpersisted-{}", uuid::Uuid::new_v4())
            }
        };

        let candidate_count = cached.candidates.len();
        crate::metrics::record_retrieval(0.0, candidate_count, &req.org_id);
        if candidate_count == 0 {
            crate::metrics::record_zero_results(&req.org_id);
        }
        tracing::info!(
            trace_id = %trace_id,
            candidates = candidate_count,
            low_confidence = cached.low_confidence,
            cache = "hit",
            "retrieval.complete"
        );

        Ok(RetrievalResponse {
            candidates: cached.candidates,
            sources: cached.sources,
            query: req.query.clone(),
            org_id: req.org_id.clone(),
            trace_id,
            index_version: cached.index_version,
            zdr_mode: zdr_mode.as_str().to_owned(),
            zdr_actions_applied: cached.zdr_actions_applied,
            low_confidence: cached.low_confidence,
            context_pack: cached.context_pack,
            suggested_next_tools: cached.suggested_next_tools,
        })
    }

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
        if req
            .query_expansion
            .as_ref()
            .is_some_and(|e| e.len() > MAX_QUERY_LEN)
        {
            anyhow::bail!("query_expansion exceeds max length of {MAX_QUERY_LEN} bytes");
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
        // Sovereignty. New field, no legacy caller to preserve a permissive
        // default for — unlike `zdr_mode.unwrap_or_default()` above (which
        // resolves to `Disabled` but relies on the auth layer always setting
        // it on every real HTTP path), this defaults to the STRICT posture on
        // an absent value, deliberately: a missing claim means the request
        // never went through `AuthContext::apply_to_request`, and treating
        // that as "sovereignty not required" would be exactly the
        // absence-of-proof-as-proof-of-safety mistake this system's own
        // `Residency::classify` doctrine exists to prevent.
        let sovereign_required = req.sovereign_required.unwrap_or(true);
        // Azure-hosted providers (Cohere Embed v4 dense/visual embedding,
        // Cohere rerank v4) can never satisfy a sovereignty requirement —
        // Microsoft operates the infrastructure, and no Azure region is
        // Norwegian-operated regardless of AZURE_*_REGION. So
        // `sovereign_required` alone restricts these two cloud calls exactly
        // like ZDR already does, even when `zdr_mode` itself would otherwise
        // allow them. Deliberately NOT folded into `embed_zdr` itself:
        // `embed_zdr` also gates the self-hosted ColQwen visual-rerank stage
        // (`postprocess::VisualRerank`, via `PostprocessCtx`) — conflating the
        // two would incorrectly extend a cloud-only restriction to a call that
        // never egresses at all.
        let cloud_egress_restricted = embed_zdr || sovereign_required;
        // Self-hosted media (audio/video via `media-embedder`) never egresses,
        // so — like ColQwen — it needs no ZDR check at all; only sovereignty
        // is a real question, and only because self-hosted doesn't
        // automatically mean Norwegian-operated. Gated on an explicit,
        // conservative-by-default config declaration — see
        // `Config::media_embedder_sovereign`.
        let media_sovereign_ok = !sovereign_required || self.config.media_embedder_sovereign;
        let effective_w_audio = if media_sovereign_ok {
            self.config.w_audio
        } else {
            0.0
        };
        let effective_w_video = if media_sovereign_ok {
            self.config.w_video
        } else {
            0.0
        };

        // Resolve the mode-mix weights FIRST so they can drive engine routing
        // (D4+D5 spec §7) and so captured-on-trace == used-for-scoring.
        //
        // §16.1.4 — agent_retrieval_configs lookup. If the caller passed
        // `agent_id` AND a row exists for (org_id, agent_id), the row's
        // `weights` becomes the default; any explicit per-request
        // `mode_mix` still overrides. Cohort precedence:
        //   per-request mode_mix > agent default > smart hybrid > global config
        let agent_default = if let Some(agent_id) = req.agent_id.as_deref() {
            crate::agent_config::lookup(&self.pool, &req.org_id, agent_id).await
        } else {
            None
        };
        let starting_mix = req
            .mode_mix
            .clone()
            .or_else(|| {
                agent_default
                    .as_ref()
                    .and_then(|c| serde_json::from_value::<ModeMixWeights>(c.weights.clone()).ok())
            })
            .or_else(|| {
                // Smart hybrid — query-adaptive weight suggestion. Only when
                // neither the caller nor the agent config expressed a blend;
                // a neutral query yields all-None (static defaults apply).
                // The resolved mix lands on the trace either way (auditable).
                if self.config.smart_hybrid_enabled {
                    let smart = crate::pipeline::smart_mix::smart_mode_mix(&req.query);
                    tracing::debug!(
                        w_dense = ?smart.w_dense,
                        w_bm25 = ?smart.w_bm25,
                        w_graph = ?smart.w_graph,
                        w_visual = ?smart.w_visual,
                        w_keyword = ?smart.w_keyword,
                        "smart hybrid mode-mix suggestion"
                    );
                    Some(smart)
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let mix_for_scoring = starting_mix.resolve(
            self.config.w_dense,
            self.config.w_bm25,
            self.config.w_graph,
            self.config.w_wiki,
            self.config.w_visual,
            self.config.w_keyword,
            self.config.rrf_k,
        );

        // Best-tool routing: run only the engines the blend actually weights.
        // A purely lexical blend (w_dense≈0) skips the embedding call + Qdrant
        // entirely; a purely semantic blend skips the sparse scan. Reuses the
        // existing dense/sparse backends — no parallel retrieval path.
        let route = EngineRoute::from_weights(&mix_for_scoring, self.config.hybrid_enabled);

        // Per-user document grants. Resolved HERE, before any retrieval work,
        // rather than just before the visibility gate where it used to sit: the
        // grant set is half of the retrieval cache's partition key
        // (`viewer_scope_token`), so it has to be known before the cache is
        // consulted or the cache could only be read after paying for the very
        // pipeline it exists to skip. Hoisting is sound because this read was
        // always independent of the candidate list — it depends only on the
        // request — and it is the same single call either way, just earlier.
        //
        // Fail-open to empty so owner + org/shared visibility still apply.
        //
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

        // §16.2.2 — retrieval-result cache read. Everything that can change the
        // served result is either in `cache_key` (query, blend, limits, filters,
        // postures) or in the key's own prefix (`org_id`, `org_version`,
        // `scope`), so a hit is only ever reused for a request that would have
        // computed the identical answer.
        //
        // `admin_read_all` is in the key even though it also collapses the scope
        // to `org-shared`: an admin and an anonymous org-wide caller both scope
        // to `org-shared`, but the admin bypassed the ownership predicate and so
        // legitimately saw MORE. Without this component the admin's wider result
        // set would be served to the narrower caller.
        let cache_scope = crate::cache::viewer_scope_token(effective_viewer, &granted_docs);
        let cache_key = crate::cache::compose_cache_key(&[
            &req.query,
            req.query_expansion.as_deref().unwrap_or(""),
            &top_k.to_string(),
            &top_n.to_string(),
            zdr_mode.as_str(),
            if sovereign_required { "sov=1" } else { "sov=0" },
            if req.admin_read_all {
                "admin=1"
            } else {
                "admin=0"
            },
            // The resolved blend, not the requested one: two different requests
            // (explicit mode_mix vs. agent default vs. smart hybrid) that
            // resolve to the same weights genuinely produce the same result and
            // should share an entry.
            &serde_json::to_string(&mix_for_scoring).unwrap_or_default(),
            &serde_json::to_string(&req.filters).unwrap_or_default(),
        ]);
        let cache_readable = retrieval_cache_allowed(zdr_mode);
        let org_version = if cache_readable && self.cache.is_some() {
            crate::cache::org_version::current(&self.pool, &req.org_id).await
        } else {
            0
        };
        if cache_readable {
            if let Some(ref cache) = self.cache {
                if let Some(raw) = cache
                    .get_retrieval(&req.org_id, org_version, &cache_scope, &cache_key)
                    .await
                {
                    match serde_json::from_str::<crate::cache::CachedRetrieval>(&raw) {
                        Ok(cached) => {
                            crate::metrics::record_retrieval_cache_hit(&req.org_id);
                            return self
                                .respond_from_cache(&req, cached, zdr_mode, &mix_for_scoring)
                                .await;
                        }
                        Err(e) => {
                            // A shape change (new field, renamed field) after a
                            // deploy makes old entries unreadable. Treat that as
                            // a miss and recompute rather than failing the
                            // query — the entry ages out on its own TTL.
                            tracing::warn!(error = %e, "retrieval cache entry unreadable; recomputing");
                        }
                    }
                }
                crate::metrics::record_retrieval_cache_miss(&req.org_id);
            }
        }

        // §16.4.7 PII redaction — strip email/phone/cardlike before emitting.
        tracing::info!(
            org_id = %req.org_id,
            query = %crate::redact::redact_query(&req.query, 60),
            top_k,
            top_n,
            hybrid = self.config.hybrid_enabled,
            route_dense = route.dense,
            route_sparse = route.sparse,
            zdr_mode = zdr_mode.as_str(),
            sovereign_required,
            // Not a full response-field explanation of WHY an arm was
            // skipped (that's a real, separate addition — extending
            // `RetrievalResponse`/the persisted trace with a per-arm skip
            // reason) — this is the minimal v1: enough to answer "was this
            // request's media arm policy-restricted" from logs alone,
            // without yet surfacing it to the caller.
            media_sovereign_ok,
            "retrieval.start"
        );

        // 1. Embed query (only when dense retrieval is routed). Cached, with the
        // key namespaced by embedding route/model so a model rotation doesn't
        // serve stale vectors for the remaining TTL (§16.2.8).
        let embed_start = Instant::now();
        let query_vector = if route.dense {
            let vec = self
                .embed_text_cached(&req.org_id, &req.query, cloud_egress_restricted)
                .await?;

            // P2-7 — HyDE / query-expansion blend. `query_expansion` is dead
            // plumbing until a caller actually populates it (see the config
            // field's doc comment for why DPv2 does not generate this text
            // itself); a populated field is itself the opt-in, so this runs
            // unconditionally on that presence check, no separate flag.
            let vec = match req
                .query_expansion
                .as_deref()
                .map(str::trim)
                .filter(|e| !e.is_empty())
            {
                Some(expansion) => {
                    let expansion_vec = self
                        .embed_text_cached(&req.org_id, expansion, cloud_egress_restricted)
                        .await?;
                    let weight = self.config.query_expansion_blend_weight.clamp(0.0, 1.0);
                    tracing::debug!(
                        blend_weight = weight,
                        "blending query embedding with caller-supplied expansion"
                    );
                    blend_vectors(&vec, &expansion_vec, weight)
                }
                None => vec,
            };
            Some(vec)
        } else {
            tracing::debug!("dense retrieval disabled by mode_mix; skipping embed + vector search");
            None
        };
        let embed_ms = embed_start.elapsed().as_millis() as u64;

        // 2. Build filters
        let mut request_filters = req.filters.clone();
        if let Some(scope) = req.space_scope.as_ref() {
            // The other hybrid arms do not yet carry an owner-resource Space
            // predicate. Refuse a scoped request rather than letting a sparse,
            // graph, wiki, visual, or keyword result escape the exact binding.
            if route.sparse
                || mix_for_scoring.w_graph > 0.0
                || mix_for_scoring.w_wiki > 0.0
                || mix_for_scoring.w_visual > 0.0
                || mix_for_scoring.w_keyword > 0.0
            {
                anyhow::bail!(
                    "Space-scoped retrieval requires the dense-only vertical until every hybrid arm enforces the binding"
                );
            }
            scope.apply_to_filters(&mut request_filters)?;
            // The binding's workspace/collection are wiki-only payload keys.
            // Pin the document vertical to the Space's own documents as well,
            // or the dense arm filters on a key its points do not carry and
            // answers every Space query with zero candidates.
            scope.apply_document_scope_to_filters(&mut request_filters)?;
        }
        let filters = RetrievalFilters {
            document_types: request_filters.document_types,
            departments: request_filters.departments,
            languages: request_filters.languages,
            document_ids: request_filters.document_ids,
            sources: request_filters.sources,
            region: request_filters.region,
            workspaces: request_filters.workspaces,
            collections: request_filters.collections,
            acl_tags: request_filters.acl_tags,
        };
        let conditions = filters.to_qdrant_conditions();

        // 3-4. Independent search arms run CONCURRENTLY, then fuse sequentially.
        // Dense (main collection), sparse (BM25/lexical), wiki ANN, visual
        // (Embed v4 query + ANN), and the audio/video media arms (self-hosted
        // CLAP/SigLIP 2 query + ANN) are mutually independent I/O.
        // `tokio::join!` schedules them on one task so their round-trips
        // overlap; ONLY the scheduling changes. Gating, error semantics, and
        // the fusion order are identical to the prior sequential path:
        //   - dense + sparse errors are FATAL (propagated via `?`);
        //   - wiki, visual, audio, and video are NON-FATAL (any error/mismatch
        //     logs a `warn!` and yields an empty arm, skipped in fusion).
        // Audio/video cost nothing extra to include unconditionally: `arm_media`
        // early-returns before any I/O when its weight is <= 0.0 (both default
        // to 0.0 — shadow, unweighted) or when no `media_embedder` is
        // configured, exactly like `arm_visual` does for `w_visual`.
        // `conditions` moves into the dense arm; `query_vector` is cloned per
        // arm exactly as the sequential code did with `qv.clone()`. The sparse
        // weight for RRF is the captured `w_bm25` (renormalized over dense+bm25).
        let arms_start = Instant::now();
        let (
            dense_res,
            sparse_res,
            graph_candidates,
            wiki_candidates,
            visual_candidates,
            keyword_candidates,
            audio_candidates,
            video_candidates,
        ) = tokio::join!(
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
                cloud_egress_restricted,
                top_k,
            ),
            self.arm_keyword(&req.query, &req.org_id, mix_for_scoring.w_keyword, top_k),
            self.arm_media(
                &req.query,
                &req.org_id,
                crate::embed::media::MediaSpace::Audio,
                effective_w_audio,
                top_k,
            ),
            self.arm_media(
                &req.query,
                &req.org_id,
                crate::embed::media::MediaSpace::Video,
                effective_w_video,
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
        // RRF(_, visual) by w_visual, then RRF(_, keyword) by w_keyword.
        let fusion_start = Instant::now();
        let (fused_candidates, candidate_count_sparse) = fuse_arms(
            dense_candidates,
            sparse_opt,
            graph_candidates,
            wiki_candidates,
            visual_candidates,
            keyword_candidates,
            &mix_for_scoring,
            route.dense,
        );

        // Audio/video arms — already resolved above (they ran concurrently with
        // dense/sparse/graph/wiki/visual/keyword in the `tokio::join!`), so
        // folding them in here is pure RRF math, no I/O. Kept out of
        // `fuse_arms` itself only because that function's positional signature
        // is shared with two test call sites; the weight check still comes
        // straight from config, since neither arm has a per-request override
        // yet nor has been validated against the golden eval set.
        let mut fused_candidates = fused_candidates;
        for (weight, hits) in [
            (effective_w_audio, audio_candidates),
            (effective_w_video, video_candidates),
        ] {
            if weight > 0.0 && !hits.is_empty() {
                fused_candidates = crate::search::fusion::reciprocal_rank_fusion(
                    &fused_candidates,
                    &hits,
                    mix_for_scoring.rrf_k,
                    weight,
                );
            }
        }

        // Fusion time on its own. The visual-rerank stage's elapsed time is added
        // back into `sparse_ms` once the postprocessor chain has run, so that
        // trace field keeps meaning exactly what it meant before the chain
        // existed (fusion + visual rerank).
        let fuse_only_ms = fusion_start.elapsed().as_millis() as u64;
        // Visual rerank reorders and rescores candidates but never adds or drops
        // one, so the fused count is already final here.
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

        // Chain input: may the cross-encoder run at all? Honors an explicit
        // `mode_mix.rerank = false` (caller opts out), any restrictive ZDR
        // posture (which forbids retaining egress of the query text), and
        // sovereignty (Cohere rerank v4 is Azure-hosted and can never satisfy
        // it — same reasoning as `cloud_egress_restricted` above, kept as a
        // separate `&&` rather than passed into `text_rerank_allowed` itself
        // so that function's existing zdr-only contract and tests stay
        // unchanged). Reranker *failure* is handled inside the stage and is
        // non-fatal by design — reranking refines ordering and must never
        // sink a successful retrieval.
        let rerank_requested = text_rerank_allowed(
            zdr_mode,
            req.mode_mix.as_ref().and_then(|m| m.rerank).unwrap_or(true),
        ) && !sovereign_required;
        // Chain input: resolve the viewer's explicit grants for the visibility
        // 5-7. The post-fusion postprocessor chain. The ORDER below is the
        // policy, not a convenience: see `pipeline::postprocess` for why the
        // visibility gate must precede the truncation to `top_n`, and why ZDR
        // must follow it.
        //
        // Everything reported downstream — candidate counts, scores, the packed
        // context, the persisted trace — derives from this chain's OUTPUT only,
        // never from a pre-filter total and never backfilled with a candidate a
        // gate removed.
        let visual_stage = postprocess::VisualRerank {
            client: self.colqwen.as_ref(),
            enabled: self.config.visual_rerank_enabled,
            top_k: self.config.visual_rerank_top_k,
            joint: self.config.joint_multimodal_rerank,
        };
        let overfetch_trim = postprocess::Truncate {
            limit: fetch_k,
            label: postprocess::STAGE_TRUNCATE_OVERFETCH,
        };
        let text_stage = postprocess::TextRerank {
            reranker: self.reranker.as_ref(),
            requested: rerank_requested,
            out_n: rerank_out_n,
            window: self.config.rerank_top_k,
        };
        // P2-3, OFF by default. Runs AFTER text_stage (dampens whatever score
        // is live at this point — the reranker's or the fused RRF's — rather
        // than before it, where the reranker would overwrite final_score and
        // erase the effect) and BEFORE the visibility gate (an authority
        // filter, not a scoring step; order relative to it doesn't matter,
        // but it must still precede top_n_trim like every scoring stage does).
        let recency_stage = postprocess::RecencyDecay {
            enabled: self.config.recency_decay_enabled,
            half_life_days: self.config.recency_decay_half_life_days,
            now: chrono::Utc::now(),
        };
        let visibility_stage = postprocess::VisibilityGate {
            pool: &self.pool,
            org_id: &req.org_id,
            viewer: effective_viewer,
            granted: &granted_docs,
        };
        let top_n_trim = postprocess::Truncate {
            limit: top_n,
            label: postprocess::STAGE_TRUNCATE_TOP_N,
        };
        let zdr_stage = postprocess::ZdrFilter {
            pool: &self.pool,
            org_id: &req.org_id,
        };
        let chain: [&dyn postprocess::NodePostprocessor; 7] = [
            &visual_stage,
            &overfetch_trim,
            &text_stage,
            &recency_stage,
            &visibility_stage,
            &top_n_trim,
            &zdr_stage,
        ];

        let mut pp_ctx = postprocess::PostprocessCtx::new(&req.query, zdr_mode, embed_zdr);
        let (reranked, stage_timings) =
            postprocess::run_chain(&chain, fused_candidates, &mut pp_ctx).await?;
        let rerank_used_count = pp_ctx.rerank_used_count;
        let zdr_actions_applied = std::mem::take(&mut pp_ctx.zdr_actions_applied);
        // `candidate_count_reranked` is declared once, further down, AFTER the
        // similarity-floor truncation — nothing between here and there reads
        // the pre-truncation count.

        // Rebuild the pre-chain timing buckets from the named stages, so these
        // trace fields stay comparable with rows written before the refactor:
        // `sparse_ms` was fusion + visual rerank, `rerank_ms` was the trim +
        // cross-encoder call.
        let sparse_ms = fuse_only_ms
            + postprocess::elapsed_of(&stage_timings, &[postprocess::STAGE_VISUAL_RERANK]);
        let rerank_ms = postprocess::elapsed_of(
            &stage_timings,
            &[
                postprocess::STAGE_TRUNCATE_OVERFETCH,
                postprocess::STAGE_TEXT_RERANK,
            ],
        );

        // Wave 3.1 §15-G — publish per-query rerank cost event. Best-effort;
        // NATS unavailable does not fail the request.
        //
        // Emitted after the chain rather than immediately after reranking,
        // because `rerank_used_count` is now a chain output. The payload is
        // unchanged: the later stages only drop candidates, and cannot change how
        // many the reranker scored.
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

        // 8. Confidence gate.
        //
        // This is a RERANKER-based gate: `confidence_threshold` (0.35) is
        // calibrated against cross-encoder scores, and `rerank_score` is left at
        // 0.0 by every retrieval arm -- dense, sparse and graph all initialise it
        // to zero, and RRF writes only `final_score`. So reading it when no
        // reranker ran compared 0.0 against 0.35 and marked EVERY query
        // low-confidence. Because rerank failure is deliberately non-fatal
        // (above), an unset `COHERE_API_KEY` or any provider outage silently
        // turned that into a blanket low-confidence verdict on every answer.
        //
        // `rerank_used_count` is the honest signal: `input_count` when the
        // reranker actually produced this order, 0 when it failed or was opted
        // out of. With no reranker we have no calibrated score, so we decline to
        // judge rather than fabricate a verdict -- note RRF `final_score` is NOT
        // a substitute, since a rank-1 fused score is ~0.03 and would trip the
        // same threshold for the opposite reason.
        //
        // An empty result set is genuinely low-confidence either way.
        let low_confidence = if reranked.is_empty() {
            true
        } else if rerank_used_count > 0 {
            reranked
                .first()
                .is_some_and(|c| c.rerank_score < self.config.confidence_threshold)
        } else {
            tracing::debug!(
                candidates = reranked.len(),
                "confidence gate inactive: no reranker scored this query"
            );
            false
        };

        // No similarity floor, closed here. `low_confidence` was previously a
        // label only: every consumer downstream — source join, context pack,
        // the trace, the served response — still received the full candidate
        // list even when the top score never cleared `confidence_threshold`.
        // The agent-hint logic below already tells the caller "try more
        // tools, this wasn't confident" while handing over the same
        // unconfident results as if they were ordinary ones — self-
        // contradictory. A query with no relevant match now genuinely
        // returns nothing, matching what `low_confidence` already claims,
        // rather than "something, plus a warning nobody is required to
        // check." This intentionally empties the SERVED list only, not a
        // per-candidate score floor further down the ranking — that would be
        // a different, more aggressive change this fix doesn't make.
        let reranked = if low_confidence { Vec::new() } else { reranked };
        let candidate_count_reranked = reranked.len();

        // 9. Source join from Postgres
        let source_start = Instant::now();
        let sources = self.join_sources(&req.org_id, &reranked).await?;
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

        // 10. Context packing (if budget requested). CAG: the org's pinned
        // permanent-memory facts pack FIRST (priority order), retrieval
        // candidates fill the remaining budget. Pins are org-shared reads —
        // fine under ZDR (no persistence, no egress) — and best-effort: a pin
        // lookup failure degrades to a retrieval-only pack.
        let context_pack = if let Some(budget) = req.context_budget_tokens {
            let format = req
                .context_format
                .clone()
                .unwrap_or_else(|| "json".to_string());
            let pins = match crate::context_pins::list_pins(
                &self.pool,
                &req.org_id,
                crate::context_pins::MAX_PINS,
            )
            .await
            {
                Ok(pins) => pins,
                Err(e) => {
                    tracing::warn!(error = %e, "context pins lookup failed; packing retrieval-only");
                    Vec::new()
                }
            };
            Some(pack_context_with_pins(
                &pins, &reranked, &sources, budget, &format,
            ))
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
        // The conditions above overlap — the confidence gate emptying the served
        // list makes both `low_confidence` and `reranked.is_empty()` true, which
        // suggested `/v1/retrieve/wiki` twice (observed live). A duplicated hint
        // reads as a weighting signal to an agent planner when it is only a
        // bookkeeping artifact; keep first occurrence, preserve order.
        let mut seen = std::collections::HashSet::new();
        suggested_next_tools.retain(|tool| seen.insert(tool.clone()));

        let zdr_actions_applied: Vec<String> = zdr_actions_applied
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        // §16.2.2 — populate the retrieval-result cache. Written from the FINAL
        // post-chain values, so an entry can only ever contain candidates that
        // already survived the visibility gate, the top_n trim and the ZDR
        // filter for this exact scope. Best-effort: a cache write failure is
        // invisible to the caller.
        //
        // `trace_id` is not stored — see `CachedRetrieval`. The write is skipped
        // under a restrictive posture by the same rule that skipped the read, so
        // a restrictive query can neither read nor seed the tier.
        if cache_readable {
            if let Some(ref cache) = self.cache {
                let payload = crate::cache::CachedRetrieval {
                    candidates: reranked.clone(),
                    sources: sources.clone(),
                    index_version: index_version.clone(),
                    zdr_actions_applied: zdr_actions_applied.clone(),
                    low_confidence,
                    context_pack: context_pack.clone(),
                    suggested_next_tools: suggested_next_tools.clone(),
                };
                match serde_json::to_string(&payload) {
                    Ok(json) => {
                        cache
                            .set_retrieval(
                                &req.org_id,
                                org_version,
                                &cache_scope,
                                &cache_key,
                                &json,
                            )
                            .await;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "retrieval result not cacheable; skipping write");
                    }
                }
            }
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
            zdr_actions_applied,
            low_confidence,
            context_pack,
            suggested_next_tools,
        })
    }

    /// Resolves candidate `document_id`s to citation metadata.
    ///
    /// `org_id` is Phase 1 RLS: neither query below has ever carried an org
    /// predicate of its own — they were safe only because the candidate list
    /// arrives pre-filtered by org from the retrieval arms. Running them in an
    /// org-scoped transaction makes the database supply that missing predicate,
    /// so a `document_id` that somehow leaked in from another tenant resolves to
    /// nothing instead of to a real title.
    #[tracing::instrument(
        name = "postgres.join_sources",
        skip_all,
        fields(otel.kind = "client", db.system = "postgresql", candidate_count = candidates.len()),
    )]
    pub async fn join_sources(
        &self,
        org_id: &str,
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

        let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
        let mut rows = sqlx::query_as::<_, SourceRow>(
            r#"
            SELECT document_id, title, source, type
            FROM documents
            WHERE document_id = ANY($1)
              AND deleted_at IS NULL
            "#,
        )
        .bind(&doc_ids)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;

        // Wiki candidates (from the wiki ANN arm) carry document_id = wiki
        // page_id, which lives in wiki_pages, not documents. Look those up too
        // so wiki citations get a real title + path + type="wiki" instead of a
        // bare id. Best-effort: a failure here just omits the wiki title.
        //
        // Scoped on its OWN transaction, deliberately: in Postgres a failed
        // statement aborts the entire transaction, so sharing one with the
        // query above would turn this tolerated failure into a hard error at
        // COMMIT — exactly what this `match` exists to avoid.
        match async {
            let mut tx = pg_org_scope::begin_org_scoped(&self.pool, org_id).await?;
            let wiki_rows = sqlx::query_as::<_, SourceRow>(
                r#"
            SELECT page_id AS document_id, title, path AS source, 'wiki' AS type
            FROM wiki_pages
            WHERE page_id = ANY($1)
            "#,
            )
            .bind(&doc_ids)
            .fetch_all(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok::<_, anyhow::Error>(wiki_rows)
        }
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
        reciprocal_rank_fusion, retrieval_cache_allowed, text_rerank_allowed, ResolvedWeights,
        ScoredCandidate, ZdrMode,
    };
    // P2-5: tests bind to the same constant the config defaults to, so a change
    // to the default is caught here rather than silently reordering results.
    use crate::search::fusion::DEFAULT_RRF_K;
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

    /// A cached retrieval result is a copy of document text living outside
    /// Postgres for the TTL, so every restrictive posture is excluded from the
    /// tier — read AND write, which is what keeps a restrictive query from
    /// seeding an entry a later non-restrictive query could read.
    #[test]
    fn restrictive_modes_never_touch_the_retrieval_result_cache() {
        assert!(!retrieval_cache_allowed(ZdrMode::Reject));
        assert!(!retrieval_cache_allowed(ZdrMode::Ephemeral));
        assert!(retrieval_cache_allowed(ZdrMode::Disabled));
        // Same line as the sibling embedding tier, deliberately — the two
        // caches must not disagree about which postures are cacheable.
        for mode in [ZdrMode::Disabled, ZdrMode::Reject, ZdrMode::Ephemeral] {
            assert_eq!(
                retrieval_cache_allowed(mode),
                embedding_cache_allowed(embed_zdr_for_mode(mode)),
                "retrieval and embedding cache admission diverged for {mode:?}"
            );
        }
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
        mix_keyword(w_dense, w_bm25, w_wiki, w_visual, 0.0)
    }

    fn mix_keyword(
        w_dense: f32,
        w_bm25: f32,
        w_wiki: f32,
        w_visual: f32,
        w_keyword: f32,
    ) -> ResolvedWeights {
        ResolvedWeights {
            w_dense,
            w_bm25,
            w_graph: 0.0,
            w_wiki,
            w_visual,
            w_keyword,
            rerank: true,
            rrf_k: crate::search::fusion::DEFAULT_RRF_K,
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
        let expected = reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&m));

        let (got, sparse_count) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
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

        let base = reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&m));
        let expected = reciprocal_rank_fusion(&base, &wiki, DEFAULT_RRF_K, m.w_wiki);

        let (got, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            wiki.clone(),
            vec![],
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

        let base = reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&m));
        let with_wiki = reciprocal_rank_fusion(&base, &wiki, DEFAULT_RRF_K, m.w_wiki);
        let expected = reciprocal_rank_fusion(&with_wiki, &visual, DEFAULT_RRF_K, m.w_visual);

        let (got, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            wiki.clone(),
            visual.clone(),
            vec![],
            &m,
            true,
        );

        assert_eq!(project(&got), project(&expected));
    }

    #[test]
    fn fuse_layers_keyword_after_visual() {
        let dense = dense_fixture();
        let sparse = sparse_fixture();
        let wiki = vec![cand("k-w1", "doc-w1", 0.8)];
        let visual = vec![cand("k-v1", "doc-v1", 0.6)];
        let keyword = vec![
            cand("k-kw1", "doc-kw1", 0.95),
            cand("k-shared", "doc-shared", 0.4),
        ];
        let m = mix_keyword(0.5, 0.2, 0.2, 0.1, 0.4);

        // Oracle: dense+sparse RRF, then wiki, then visual, THEN keyword —
        // the newest arm fuses last (see `fuse_arms`'s doc comment).
        let base = reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&m));
        let with_wiki = reciprocal_rank_fusion(&base, &wiki, DEFAULT_RRF_K, m.w_wiki);
        let with_visual = reciprocal_rank_fusion(&with_wiki, &visual, DEFAULT_RRF_K, m.w_visual);
        let expected = reciprocal_rank_fusion(&with_visual, &keyword, DEFAULT_RRF_K, m.w_keyword);

        let (got, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            wiki.clone(),
            visual.clone(),
            keyword.clone(),
            &m,
            true,
        );

        assert_eq!(project(&got), project(&expected));
    }

    #[test]
    fn fuse_skips_keyword_when_weight_zero_or_empty() {
        let dense = dense_fixture();
        let sparse = sparse_fixture();

        // w_keyword = 0 but a keyword list IS supplied → it must be ignored.
        let base_mix = mix(0.7, 0.3, 0.0, 0.0);
        let expected =
            reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&base_mix));
        let (got_weight_off, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            vec![],
            vec![],
            vec![cand("k-kw1", "doc-kw1", 0.9)],
            &base_mix,
            true,
        );
        assert_eq!(project(&got_weight_off), project(&expected));

        // w_keyword > 0 but the arm returned nothing → skipped.
        let m = mix_keyword(0.6, 0.2, 0.0, 0.0, 0.4);
        let expected_empty = reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&m));
        let (got_empty, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            vec![],
            vec![],
            vec![],
            &m,
            true,
        );
        assert_eq!(project(&got_empty), project(&expected_empty));
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
        let (got, sparse_count) = fuse_arms(
            dense.clone(),
            None,
            vec![],
            vec![],
            vec![],
            vec![],
            &m,
            true,
        );
        assert_eq!(sparse_count, 0);
        assert_eq!(project(&got), project(&dense));

        // Contrast: sparse = Some(empty) (route on, zero hits) STILL runs RRF,
        // re-weighting dense — proving the Option distinction is load-bearing.
        let m2 = mix(0.7, 0.3, 0.0, 0.0);
        let expected_empty_rrf =
            reciprocal_rank_fusion(&dense, &[], DEFAULT_RRF_K, bm25_share(&m2));
        let (got_empty, count_empty) = fuse_arms(
            dense.clone(),
            Some(vec![]),
            vec![],
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
        let expected =
            reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&base_mix));

        // w_wiki = 0 but a wiki list IS supplied → wiki must be ignored.
        let (got_weight_off, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            vec![cand("k-w1", "doc-w1", 0.9)],
            vec![],
            vec![],
            &base_mix,
            true,
        );
        assert_eq!(project(&got_weight_off), project(&expected));

        // w_wiki/w_visual > 0 but the arms returned nothing → both skipped.
        let m = mix(0.6, 0.2, 0.5, 0.3);
        let expected_empty_arms =
            reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&m));
        let (got_empty_arms, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
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
            w_keyword: 0.0,
            rerank: true,
            rrf_k: crate::search::fusion::DEFAULT_RRF_K,
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
        let base = reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&m));
        let expected = reciprocal_rank_fusion(&base, &graph, DEFAULT_RRF_K, m.w_graph);

        let (got, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            graph.clone(),
            vec![],
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
        let expected =
            reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&base_mix));
        let (got_weight_off, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![cand("k-g1", "doc-g1", 0.9)],
            vec![],
            vec![],
            vec![],
            &base_mix,
            true,
        );
        assert_eq!(project(&got_weight_off), project(&expected));

        // w_graph > 0 but the graph arm returned nothing → skipped.
        let m = mix_graph(0.6, 0.2, 0.4);
        let expected_empty = reciprocal_rank_fusion(&dense, &sparse, DEFAULT_RRF_K, bm25_share(&m));
        let (got_empty, _) = fuse_arms(
            dense.clone(),
            Some(sparse.clone()),
            vec![],
            vec![],
            vec![],
            vec![],
            &m,
            true,
        );
        assert_eq!(project(&got_empty), project(&expected_empty));
    }

    // --- apply_colqwen_scores: joint (text-vs-image) vs band-preserving modes.

    use super::apply_colqwen_scores;

    /// fused = [text 0.9, visual 0.5, text 0.4, visual 0.2] (already desc).
    fn multimodal_fixture() -> Vec<ScoredCandidate> {
        vec![
            cand("t1", "doc-t1", 0.9),
            cand("v1", "doc-v1", 0.5),
            cand("t2", "doc-t2", 0.4),
            cand("v2", "doc-v2", 0.2),
        ]
    }

    fn ids(v: &[ScoredCandidate]) -> Vec<&str> {
        v.iter().map(|c| c.knowledge_id.as_str()).collect()
    }

    #[test]
    fn joint_mode_lets_strong_visual_leapfrog_text() {
        // v2 (slot 3, weakest fused score) gets the strongest ColQwen score:
        // in joint mode it maps to the global max (0.9) and must leapfrog t2.
        let got = apply_colqwen_scores(multimodal_fixture(), &[1, 3], &[2.0, 10.0], true);
        assert_eq!(ids(&got), vec!["t1", "v2", "t2", "v1"]);
        // Raw ColQwen scores are preserved on rerank_score for the trace.
        let v2 = got.iter().find(|c| c.knowledge_id == "v2").unwrap();
        assert_eq!(v2.rerank_score, 10.0);
        assert!((v2.final_score - 0.9).abs() < 1e-6);
        let v1 = got.iter().find(|c| c.knowledge_id == "v1").unwrap();
        assert!((v1.final_score - 0.2).abs() < 1e-6);
    }

    #[test]
    fn band_mode_reorders_visuals_within_their_band_only() {
        // Same ColQwen scores, band mode: v2 takes v1's band slot (0.5) and
        // vice versa — but neither crosses a text candidate.
        let got = apply_colqwen_scores(multimodal_fixture(), &[1, 3], &[2.0, 10.0], false);
        assert_eq!(ids(&got), vec!["t1", "v2", "t2", "v1"]);
        let scores: Vec<f32> = got.iter().map(|c| c.final_score).collect();
        assert_eq!(scores, vec![0.9, 0.5, 0.4, 0.2]);
    }

    #[test]
    fn joint_mode_degenerate_scores_fall_back_to_band() {
        // All ColQwen scores equal → no honest joint ordering exists; keep the
        // band behavior instead of fabricating one.
        let got = apply_colqwen_scores(multimodal_fixture(), &[1, 3], &[5.0, 5.0], true);
        let scores: Vec<f32> = got.iter().map(|c| c.final_score).collect();
        assert_eq!(scores, vec![0.9, 0.5, 0.4, 0.2]);
    }

    #[test]
    fn colqwen_placement_ignores_mismatched_inputs() {
        let fused = multimodal_fixture();
        let got = apply_colqwen_scores(fused.clone(), &[1, 3], &[1.0], true);
        assert_eq!(project(&got), project(&fused));
        let got_empty = apply_colqwen_scores(fused.clone(), &[], &[], true);
        assert_eq!(project(&got_empty), project(&fused));
    }

    // --- blend_vectors: P2-7 HyDE / query-expansion blend weight.

    use super::blend_vectors;

    #[test]
    fn weight_zero_returns_the_query_vector_unchanged() {
        let query = vec![1.0, 2.0, 3.0];
        let expansion = vec![10.0, 20.0, 30.0];
        assert_eq!(blend_vectors(&query, &expansion, 0.0), query);
    }

    #[test]
    fn weight_one_returns_the_expansion_vector_unchanged() {
        let query = vec![1.0, 2.0, 3.0];
        let expansion = vec![10.0, 20.0, 30.0];
        assert_eq!(blend_vectors(&query, &expansion, 1.0), expansion);
    }

    #[test]
    fn weight_half_is_the_midpoint_of_each_dimension() {
        let query = vec![0.0, 2.0, -4.0];
        let expansion = vec![4.0, 6.0, 0.0];
        assert_eq!(blend_vectors(&query, &expansion, 0.5), vec![2.0, 4.0, -2.0]);
    }

    #[test]
    fn a_mismatched_length_truncates_to_the_shorter_input_rather_than_panicking() {
        let query = vec![1.0, 1.0, 1.0];
        let expansion = vec![5.0, 5.0];
        assert_eq!(blend_vectors(&query, &expansion, 0.5), vec![3.0, 3.0]);
    }
}
