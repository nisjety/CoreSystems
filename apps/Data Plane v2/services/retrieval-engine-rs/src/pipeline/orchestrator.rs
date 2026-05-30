use std::time::Instant;

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
    /// Wave 3.1 §15-G — publish `dataplane.cost.ledger` events on rerank
    /// (embedding worker handles embeds). Optional: degrades silently when
    /// NATS isn't reachable.
    pub nats: Option<async_nats::Client>,
    /// Sparse lexical search backend. Postgres remains the canonical fallback;
    /// Quickwit is a rebuildable read model when enabled.
    pub sparse_backend: DynSparseSearchBackend,
}

impl RetrievalPipeline {
    #[tracing::instrument(
        name = "retrieval.pipeline",
        skip(self),
        fields(
            org_id = req.org_id.as_str(),
            query_len = req.query.len(),
            zdr_mode = req.zdr_mode.as_deref().unwrap_or("disabled"),
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
        let zdr_mode = req
            .zdr_mode
            .clone()
            .unwrap_or_else(|| "disabled".to_string());

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
            let vec = if let Some(ref cache) = self.cache {
                if let Some(cached) = cache.get_embedding(&model_ver, &query_hash).await {
                    tracing::debug!("embed cache hit");
                    crate::metrics::record_embed_cache_hit();
                    cached
                } else {
                    crate::metrics::record_embed_request();
                    let vec = self.embedder.embed_query(&req.org_id, &req.query).await?;
                    cache.set_embedding(&model_ver, &query_hash, &vec).await;
                    vec
                }
            } else {
                crate::metrics::record_embed_request();
                self.embedder.embed_query(&req.org_id, &req.query).await?
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

        // 3. Dense vector search (only when routed).
        let dense_start = Instant::now();
        let dense_candidates = if let Some(query_vector) = query_vector {
            vector_search(
                &self.qdrant,
                &self.config.qdrant_collection,
                query_vector,
                &req.org_id,
                conditions,
                top_k,
            )
            .await?
        } else {
            Vec::new()
        };
        let dense_ms = dense_start.elapsed().as_millis() as u64;
        let candidate_count_dense = dense_candidates.len();

        // 4. Sparse/BM25 search + fusion (only when routed).
        // Sparse weight passed to RRF is the captured `w_bm25` from the
        // resolved mix (renormalized over dense+bm25). w_graph/w_wiki are
        // captured for the trace but not yet folded into this RRF pass —
        // their scoring lives in dedicated `/v1/retrieve/graph` and
        // `/v1/retrieve/wiki` endpoints. Wave-3 will merge them into a
        // single 4-way score after wiki_block_embeddings ANN is populated.
        let sparse_start = Instant::now();
        let (fused_candidates, candidate_count_sparse) = if route.sparse {
            let sparse_candidates = self
                .sparse_backend
                .search(&req.query, &req.org_id, top_k)
                .await?;
            let sparse_count = sparse_candidates.len();
            if route.dense {
                // Renormalize dense+bm25 sub-mix so RRF gets a [0,1] weight on
                // the BM25 list (the legacy `bm25_weight` config remains as the
                // backwards-compat fallback when callers don't send mode_mix).
                let bm25_share = {
                    let d = mix_for_scoring.w_dense;
                    let b = mix_for_scoring.w_bm25;
                    let sum = (d + b).max(f32::EPSILON);
                    b / sum
                };
                let fused =
                    reciprocal_rank_fusion(&dense_candidates, &sparse_candidates, 60.0, bm25_share);
                (fused, sparse_count)
            } else {
                // Sparse-only route: no dense list to fuse against.
                (sparse_candidates, sparse_count)
            }
        } else {
            (dense_candidates, 0)
        };
        let sparse_ms = sparse_start.elapsed().as_millis() as u64;
        let candidate_count_fused = fused_candidates.len();

        // Trim to top_k before rerank
        let pre_rerank: Vec<ScoredCandidate> = fused_candidates.into_iter().take(top_k).collect();

        // 5. Rerank
        let rerank_start = Instant::now();
        let (reranked, rerank_used_count) = if let Some(ref reranker) = self.reranker {
            let input_count = pre_rerank.len();
            let out = reranker.rerank(&req.query, &pre_rerank, top_n).await?;
            (out, input_count)
        } else {
            (pre_rerank.into_iter().take(top_n).collect(), 0usize)
        };
        let rerank_ms = rerank_start.elapsed().as_millis() as u64;

        // Wave 3.1 §15-G — publish per-query rerank cost event. Best-effort;
        // NATS unavailable does not fail the request.
        if rerank_used_count > 0 {
            if let (Some(nats), Some(reranker)) = (self.nats.as_ref(), self.reranker.as_ref()) {
                let model = reranker.model_name().to_string();
                let cost_event = serde_json::json!({
                    "event_type": "rerank",
                    "model": model,
                    "count": rerank_used_count,
                    "estimated_tokens": rerank_used_count as i64 * 32,
                    "org_ids": [req.org_id.clone()],
                    "user_id": req.user_id.clone().unwrap_or_default(),
                    "idempotency_key": format!("rerank:{}:{}", req.org_id, uuid::Uuid::new_v4()),
                });
                if let Ok(payload) = serde_json::to_vec(&cost_event) {
                    // §17.3.3 — named subject, lint-checked.
                    const SUBJECT_COST_LEDGER: &str = "dataplane.cost.ledger";
                    let _ = nats.publish(SUBJECT_COST_LEDGER, payload.into()).await;
                }
                crate::metrics::record_rerank_request();
            }
        }

        // 6. Canonical visibility gate. Quickwit and Qdrant are rebuildable
        // read models, so stale hits can exist briefly after a Postgres
        // tombstone. Filter through canonical Postgres before applying ZDR
        // and joining sources.
        let reranked = self.filter_live_candidates(reranked).await?;

        // 7. ZDR enforcement — filter out restricted documents.
        // §16.1.3 — also record what we actually did, so the audit trail can
        // distinguish "mode=reject but nothing to reject" from "mode=disabled"
        // from "mode=reject and 4 docs filtered".
        let mut zdr_actions_applied: Vec<&'static str> = Vec::new();
        let reranked = if zdr_mode == "reject" {
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
        } else if zdr_mode == "ephemeral" {
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
        let trace_id = if zdr_mode == "ephemeral" {
            format!("ephemeral-{}", uuid::Uuid::new_v4())
        } else {
            match persist_trace(
                &self.pool,
                &req,
                &reranked,
                &timings,
                self.reranker.as_ref().map(|r| r.model_name()),
                &zdr_mode,
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
            zdr_mode,
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

        let rows = sqlx::query_as::<_, SourceRow>(
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

    async fn filter_live_candidates(
        &self,
        candidates: Vec<ScoredCandidate>,
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

        let live: std::collections::HashSet<String> = sqlx::query_as::<_, (String,)>(
            r#"
            SELECT document_id
            FROM documents
            WHERE document_id = ANY($1)
              AND deleted_at IS NULL
            "#,
        )
        .bind(&doc_ids)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(id,)| id)
        .collect();

        if live.len() == doc_ids.len() {
            return Ok(candidates);
        }

        let before = candidates.len();
        let filtered: Vec<ScoredCandidate> = candidates
            .into_iter()
            .filter(|c| live.contains(&c.document_id))
            .collect();
        tracing::warn!(
            before,
            after = filtered.len(),
            "filtered retrieval candidates missing from canonical live documents"
        );
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
