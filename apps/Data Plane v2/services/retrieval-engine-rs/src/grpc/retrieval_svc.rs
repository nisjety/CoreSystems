use std::pin::Pin;
use std::sync::Arc;

use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::MetadataMap;
use tonic::{Request, Response, Status};

use crate::context_pack::pack_context;
use crate::pipeline::orchestrator::RetrievalPipeline;
use crate::pipeline::types::{RetrievalFiltersInput, RetrievalRequest as PipelineReq, ZdrMode};
use crate::trace;

use super::pb_retrieval::retrieval_service_server::RetrievalService;
use super::pb_retrieval::*;

type StreamResp<T> = Pin<Box<dyn futures::Stream<Item = Result<T, Status>> + Send + 'static>>;

/// Build `RetrieveResponse.retrieval_metadata` (proto field 10).
///
/// The pipeline computes two things that the gRPC surface used to throw away by
/// hardcoding `retrieval_metadata: None`:
///
/// * `suggested_next_tools` — the agent planner hints (which of the typed
///   retrieval endpoints are likely productive next, given this response's
///   signal). The HTTP surface serialises them; gRPC did not. gRPC is the path
///   the Model Plane's agent loop actually calls, so the hints were computed on
///   every request and delivered to nobody. That is the concrete reason 14
///   typed retrieval endpoints saw ~1 in use: the Data Plane never told its
///   primary consumer the others existed, let alone when to reach for them.
/// * `zdr_actions_applied` — which Zero Data Retention enforcement actions this
///   retrieval actually took. Also HTTP-only before this. A caller that cannot
///   observe enforcement cannot propagate it, and ZDR has to survive every
///   content-carrying boundary, so dropping it on the primary transport was the
///   more serious of the two omissions.
///
/// `google.protobuf.Struct` is used rather than new typed proto fields on
/// purpose: field 10 is already in the frozen contract, so this needs no
/// regeneration and no coordinated cross-plane roll. If these hints ever become
/// load-bearing rather than advisory, promote them to typed fields then.
///
/// Returns `None` — not an empty `Struct` — when there is nothing to report, so
/// a quiet response stays byte-identical on the wire to its old form.
fn retrieval_metadata(
    suggested_next_tools: &[String],
    zdr_actions_applied: &[String],
) -> Option<prost_types::Struct> {
    fn string_list(values: &[String]) -> prost_types::Value {
        prost_types::Value {
            kind: Some(prost_types::value::Kind::ListValue(
                prost_types::ListValue {
                    values: values
                        .iter()
                        .map(|v| prost_types::Value {
                            kind: Some(prost_types::value::Kind::StringValue(v.clone())),
                        })
                        .collect(),
                },
            )),
        }
    }

    let mut fields = std::collections::BTreeMap::new();
    if !suggested_next_tools.is_empty() {
        fields.insert(
            "suggested_next_tools".to_string(),
            string_list(suggested_next_tools),
        );
    }
    if !zdr_actions_applied.is_empty() {
        fields.insert(
            "zdr_actions_applied".to_string(),
            string_list(zdr_actions_applied),
        );
    }

    if fields.is_empty() {
        None
    } else {
        Some(prost_types::Struct { fields })
    }
}

pub struct RetrievalSvc {
    pipeline: Arc<RetrievalPipeline>,
}

impl RetrievalSvc {
    pub fn new(pipeline: Arc<RetrievalPipeline>) -> Self {
        Self { pipeline }
    }

    async fn authorize<T>(
        &self,
        request: &Request<T>,
        org_id: &str,
        user_id: Option<&str>,
    ) -> Result<crate::authz::AuthContext, Status> {
        super::interceptor::authorize_request(
            self.pipeline.policy.as_ref(),
            request,
            org_id,
            user_id,
        )
        .await
    }

    async fn grants(&self, ctx: &crate::authz::AuthContext) -> Vec<String> {
        match ctx.user_id.as_deref() {
            Some(user_id) => {
                self.pipeline
                    .visibility
                    .visible_documents(&ctx.org_id, user_id, ctx.verified_bearer.as_deref())
                    .await
            }
            None => Vec::new(),
        }
    }

    /// Apply a Space authority only after the gRPC caller's normal identity is
    /// verified. This mirrors the HTTP retrieval boundary: metadata can carry
    /// the signed decision, but it can never choose a workspace/collection or
    /// substitute for a verified subject.
    async fn apply_space_decision(
        &self,
        ctx: &crate::authz::AuthContext,
        token: Option<&str>,
        pipeline_req: &mut PipelineReq,
    ) -> Result<(), Status> {
        let Some(token) = token else {
            return Ok(());
        };
        let subject = ctx.user_id.as_deref().ok_or_else(|| {
            Status::permission_denied("user subject required for Space retrieval")
        })?;
        let keys = crate::space_scope::configured_retrieval_decision_keys()
            .map_err(|_| Status::permission_denied("Space decision verification is unavailable"))?;
        let authority = crate::space_scope::verify_retrieval_space_decision(
            token,
            &keys,
            &ctx.org_id,
            subject,
            chrono::Utc::now(),
        )
        .map_err(|_| Status::permission_denied("invalid Space retrieval decision"))?;
        pipeline_req.space_scope = Some(
            crate::space_scope::resolve_space_retrieval_scope(&self.pipeline.pool, authority)
                .await
                .map_err(|_| Status::permission_denied("Space retrieval binding is unavailable"))?,
        );
        Ok(())
    }
}

// `tonic::Status` is 176 bytes, over clippy's 128-byte `result_large_err`
// threshold. Every fallible function in this file returns `Result<_, Status>` —
// it is tonic's own error contract, not a choice we can make differently — and
// this is the only one the lint fires on, because it is the only one whose Ok
// variant (`Option<&str>`, 16 bytes) is small enough for the ratio to trip.
// Boxing the error here would make one function inconsistent with the trait
// impls around it and change nothing about the actual cost. Allowed rather than
// worked around; revisit if tonic ever shrinks `Status`.
#[allow(clippy::result_large_err)]
fn space_decision_from_metadata(metadata: &MetadataMap) -> Result<Option<&str>, Status> {
    metadata
        .get("x-space-decision")
        .map(|value| {
            value
                .to_str()
                .map_err(|_| Status::invalid_argument("invalid Space retrieval decision metadata"))
        })
        .transpose()
}

#[tonic::async_trait]
impl RetrievalService for RetrievalSvc {
    // §17.3.2 — server-streaming retrieval. We still run the same
    // pipeline (no real progressive results until the pipeline itself
    // is refactored), but we frame the response so clients can adopt
    // the streaming wire shape now and migrate the internals later
    // without a contract break.
    type RetrieveStreamStream = StreamResp<RetrievalChunk>;

    async fn retrieve_stream(
        &self,
        request: Request<RetrieveRequest>,
    ) -> Result<Response<Self::RetrieveStreamStream>, Status> {
        let pipeline = self.pipeline.clone();
        let space_decision = space_decision_from_metadata(request.metadata())?.map(str::to_owned);
        let ctx = self
            .authorize(
                &request,
                &request.get_ref().org_id,
                request.get_ref().user_id.as_deref(),
            )
            .await?;
        let inner = request.into_inner();
        let mut pipeline_req = grpc_to_pipeline(inner).map_err(Status::invalid_argument)?;
        apply_verified_context(&ctx, &mut pipeline_req);
        self.apply_space_decision(&ctx, space_decision.as_deref(), &mut pipeline_req)
            .await?;

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<RetrievalChunk, Status>>(32);
        tokio::spawn(async move {
            let resp = match pipeline.retrieve(pipeline_req).await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(Status::internal(e.to_string()))).await;
                    return;
                }
            };
            for c in resp.candidates.iter() {
                let frame = RetrievalChunk {
                    payload: Some(retrieval_chunk::Payload::Candidate(Candidate {
                        knowledge_id: c.knowledge_id.clone(),
                        document_id: c.document_id.clone(),
                        text: c.text.clone(),
                        dense_score: c.dense_score,
                        sparse_score: c.sparse_score,
                        rerank_score: c.rerank_score,
                        final_score: c.final_score,
                        metadata: None,
                        chunk_index: c.chunk_index,
                        chunk_version: String::new(),
                    })),
                };
                if tx.send(Ok(frame)).await.is_err() {
                    // client disconnected; abort cleanly.
                    return;
                }
            }
            let trailer = RetrievalChunk {
                payload: Some(retrieval_chunk::Payload::Trailer(RetrievalTrailer {
                    trace_id: resp.trace_id,
                    index_version: resp.index_version,
                    zdr_mode: resp.zdr_mode,
                    low_confidence: resp.low_confidence,
                    candidate_count: resp.candidates.len() as i32,
                })),
            };
            let _ = tx.send(Ok(trailer)).await;
        });

        let stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as StreamResp<RetrievalChunk>))
    }

    async fn retrieve(
        &self,
        request: Request<RetrieveRequest>,
    ) -> Result<Response<RetrieveResponse>, Status> {
        let space_decision = space_decision_from_metadata(request.metadata())?.map(str::to_owned);
        let ctx = self
            .authorize(
                &request,
                &request.get_ref().org_id,
                request.get_ref().user_id.as_deref(),
            )
            .await?;
        let req = request.into_inner();

        // Was a second, hand-maintained copy of `grpc_to_pipeline`'s field-by-field
        // construction. The two drifted, and that is what took this RPC down: the
        // per-request sovereignty fix landed in `grpc_to_pipeline` (which serves
        // `RetrieveStream`) while this copy — the path the Model Plane's agent loop
        // actually calls — kept its own `sovereign_required: None`. The unit test
        // covering the mapping passed the whole time, because it tested the helper.
        //
        // Collapsing them is behaviour-preserving: this copy differed only by
        // pre-seeding `org_id`, `user_id`, `verified_bearer` and `admin_read_all`
        // from the context, and `apply_verified_context` overwrites all four
        // unconditionally on the next line. One construction, one test, no drift.
        let mut pipeline_req = grpc_to_pipeline(req).map_err(Status::invalid_argument)?;
        apply_verified_context(&ctx, &mut pipeline_req);
        self.apply_space_decision(&ctx, space_decision.as_deref(), &mut pipeline_req)
            .await?;

        let resp = self.pipeline.retrieve(pipeline_req).await.map_err(|e| {
            // The anyhow chain (e.g. "model-plane embedding failed: status:
            // Unauthenticated ...") is deliberately not echoed to the caller,
            // but must not be silently truncated to just the top-level
            // context either — `{:?}` prints the full "Caused by:" chain so
            // this is diagnosable from server logs, not just an opaque
            // "model-plane embedding failed".
            tracing::warn!(error = ?e, "retrieval pipeline failed");
            Status::internal(e.to_string())
        })?;

        let candidates: Vec<Candidate> = resp
            .candidates
            .iter()
            .map(|c| Candidate {
                knowledge_id: c.knowledge_id.clone(),
                document_id: c.document_id.clone(),
                text: c.text.clone(),
                dense_score: c.dense_score,
                sparse_score: c.sparse_score,
                rerank_score: c.rerank_score,
                final_score: c.final_score,
                metadata: None,
                chunk_index: c.chunk_index,
                chunk_version: String::new(),
            })
            .collect();

        let sources: Vec<Source> = resp
            .sources
            .iter()
            .map(|s| Source {
                document_id: s.document_id.clone(),
                title: s.title.clone(),
                source: s.source.clone(),
                r#type: s.r#type.clone(),
                created_at: None,
            })
            .collect();

        let context_pack = resp.context_pack.map(|cp| ContextPack {
            facts: cp
                .facts
                .iter()
                .map(|f| ContextFact {
                    knowledge_id: f.knowledge_id.clone(),
                    document_id: f.document_id.clone(),
                    text: f.text.clone(),
                    score: f.score,
                    source_title: f.source_title.clone(),
                    source_type: f.source_type.clone(),
                    estimated_tokens: f.estimated_tokens as i32,
                })
                .collect(),
            total_tokens: cp.total_tokens as i32,
            budget_tokens: cp.budget_tokens as i32,
            format: cp.format,
        });

        // §17.3.6 — backpressure trailers. We attach RFC-style headers
        // so smart Model Plane clients can pre-throttle before they hit
        // the per-org rate limit. Values come from the same Dragonfly-backed
        // limiter that gates HTTP (`PerOrgLimiter`, P2-6). When the limiter
        // is healthy we publish a rough remaining budget; on load-shed
        // we'd return Status::resource_exhausted instead of here.
        let mut response = Response::new(RetrieveResponse {
            candidates,
            sources,
            query: resp.query,
            org_id: resp.org_id,
            trace_id: resp.trace_id,
            index_version: resp.index_version,
            zdr_mode: resp.zdr_mode,
            low_confidence: resp.low_confidence,
            context_pack,
            retrieval_metadata: retrieval_metadata(
                &resp.suggested_next_tools,
                &resp.zdr_actions_applied,
            ),
        });
        let burst = std::env::var("DPV2_RATE_LIMIT_PER_ORG_BURST")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(40);
        let rps = std::env::var("DPV2_RATE_LIMIT_PER_ORG_RPS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(20);
        if let Ok(val) = tonic::metadata::AsciiMetadataValue::try_from(burst.to_string()) {
            response.metadata_mut().insert("x-ratelimit-limit", val);
        }
        if let Ok(val) = tonic::metadata::AsciiMetadataValue::try_from(rps.to_string()) {
            response.metadata_mut().insert("x-ratelimit-rps", val);
        }
        Ok(response)
    }

    async fn get_trace(
        &self,
        request: Request<GetTraceRequest>,
    ) -> Result<Response<GetTraceResponse>, Status> {
        let ctx = self
            .authorize(&request, &request.get_ref().org_id, None)
            .await?;
        let req = request.into_inner();

        let actor = if ctx.scopes.iter().any(|scope| scope == "org:data:read_all") {
            None
        } else {
            ctx.user_id.as_deref()
        };
        let detail = trace::get_trace(&self.pipeline.pool, &req.trace_id, &ctx.org_id, actor)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        match detail {
            Some(t) => {
                let run = &t.run;
                let trace = RetrievalTrace {
                    trace_id: run.trace_id.clone(),
                    org_id: run.org_id.clone(),
                    query: run.query.clone(),
                    query_embedding_model: run.query_embedding_model.clone().unwrap_or_default(),
                    index_version: run.index_version.clone().unwrap_or_default(),
                    filters_applied: None,
                    candidates: t
                        .candidates
                        .into_iter()
                        .map(|c| RetrievalCandidate {
                            rank: c.rank,
                            knowledge_id: c.knowledge_id.unwrap_or_default(),
                            document_id: c.document_id.unwrap_or_default(),
                            // Narrowed here, at the wire boundary, because the
                            // proto declares these `float`. The DB columns are
                            // `double precision` and `TraceCandidateRow` now
                            // decodes them as `f64` to match — reading them as
                            // `f32` made every trace fetch fail. Scores are
                            // ranking signals in [0,1]-ish ranges, so f32 on the
                            // wire loses nothing that a caller can act on.
                            dense_score: c.dense_score.unwrap_or(0.0) as f32,
                            sparse_score: c.sparse_score.unwrap_or(0.0) as f32,
                            rerank_score: c.rerank_score.unwrap_or(0.0) as f32,
                            source_chunk_ref: String::new(),
                        })
                        .collect(),
                    reranker_name: run.reranker_name.clone().unwrap_or_default(),
                    zdr_mode: run.zdr_mode.clone().unwrap_or_default(),
                    created_at: None,
                    dense_retrieval_time_ms: run.dense_retrieval_ms.unwrap_or(0),
                    sparse_retrieval_time_ms: run.sparse_retrieval_ms.unwrap_or(0),
                    rerank_time_ms: run.rerank_ms.unwrap_or(0),
                    total_time_ms: run.total_ms.unwrap_or(0),
                    candidate_count_dense: run.candidate_count_dense.unwrap_or(0),
                    candidate_count_sparse: run.candidate_count_sparse.unwrap_or(0),
                    candidate_count_after_fusion: run.candidate_count_fused.unwrap_or(0),
                    candidate_count_after_rerank: run.candidate_count_reranked.unwrap_or(0),
                };
                Ok(Response::new(GetTraceResponse { trace: Some(trace) }))
            }
            None => Err(Status::not_found("trace not found")),
        }
    }

    async fn get_sources(
        &self,
        request: Request<GetSourcesRequest>,
    ) -> Result<Response<GetSourcesResponse>, Status> {
        let ctx = self
            .authorize(&request, &request.get_ref().org_id, None)
            .await?;
        let req = request.into_inner();
        if req.document_ids.is_empty() {
            return Ok(Response::new(GetSourcesResponse { sources: vec![] }));
        }

        let grants = self.grants(&ctx).await;
        // Phase 1 RLS: the org comes from the authorized `AuthContext`, so this
        // reads through an org-scoped transaction. The SQL still binds `org_id`
        // itself — the database policy is a backstop against that filter being
        // dropped or mis-edited later, not a replacement for it.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pipeline.pool, &ctx.org_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT document_id, title, source, type FROM documents
             WHERE document_id = ANY($1) AND org_id = $2 AND deleted_at IS NULL
               AND (owner_id = $3 OR visibility = 'org' OR document_id = ANY($4))",
        )
        .bind(&req.document_ids)
        .bind(&ctx.org_id)
        .bind(ctx.user_id.as_deref())
        .bind(&grants)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let sources = rows
            .into_iter()
            .map(|(did, title, source, dtype)| Source {
                document_id: did,
                title,
                source,
                r#type: dtype,
                created_at: None,
            })
            .collect();

        Ok(Response::new(GetSourcesResponse { sources }))
    }

    async fn get_chunks(
        &self,
        request: Request<GetChunksRequest>,
    ) -> Result<Response<GetChunksResponse>, Status> {
        let ctx = self
            .authorize(&request, &request.get_ref().org_id, None)
            .await?;
        let req = request.into_inner();
        let grants = self.grants(&ctx).await;

        // Phase 1 RLS: both lookup variants below serve the same org (from the
        // authorized `AuthContext`), so they share ONE scoped transaction. The
        // SQL still binds `org_id` itself — the database policy is a backstop,
        // not a replacement. Note the `documents` join carries no org predicate
        // of its own; RLS now supplies one for that side too.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pipeline.pool, &ctx.org_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let chunks = if !req.knowledge_ids.is_empty() {
            sqlx::query_as::<_, (String, String, String, i32, String, String)>(
                "SELECT ku.knowledge_id, ku.document_id, ku.text, ku.chunk_index, ku.content_hash, ku.embedding_status
                 FROM knowledge_units ku
                 JOIN documents d ON d.document_id = ku.document_id
                 WHERE ku.knowledge_id = ANY($1) AND ku.org_id = $2 AND d.deleted_at IS NULL
                   AND (d.owner_id = $3 OR d.visibility = 'org' OR d.document_id = ANY($4))
                 ORDER BY ku.chunk_index"
            )
            .bind(&req.knowledge_ids)
            .bind(&ctx.org_id)
            .bind(ctx.user_id.as_deref())
            .bind(&grants)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        } else if let Some(doc_id) = &req.document_id {
            sqlx::query_as::<_, (String, String, String, i32, String, String)>(
                "SELECT ku.knowledge_id, ku.document_id, ku.text, ku.chunk_index, ku.content_hash, ku.embedding_status
                 FROM knowledge_units ku
                 JOIN documents d ON d.document_id = ku.document_id
                 WHERE ku.document_id = $1 AND ku.org_id = $2 AND d.deleted_at IS NULL
                   AND (d.owner_id = $3 OR d.visibility = 'org' OR d.document_id = ANY($4))
                 ORDER BY ku.chunk_index"
            )
            .bind(doc_id)
            .bind(&ctx.org_id)
            .bind(ctx.user_id.as_deref())
            .bind(&grants)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        } else {
            // Dropping `tx` here rolls the (read-only) transaction back.
            return Err(Status::invalid_argument(
                "provide knowledge_ids or document_id",
            ));
        };

        tx.commit()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let result: Vec<Candidate> = chunks
            .into_iter()
            .map(|(kid, did, text, idx, hash, _status)| Candidate {
                knowledge_id: kid,
                document_id: did,
                text,
                dense_score: 0.0,
                sparse_score: 0.0,
                rerank_score: 0.0,
                final_score: 0.0,
                metadata: None,
                chunk_index: idx,
                chunk_version: hash,
            })
            .collect();

        Ok(Response::new(GetChunksResponse { chunks: result }))
    }

    async fn pack_context(
        &self,
        request: Request<PackContextRequest>,
    ) -> Result<Response<PackContextResponse>, Status> {
        let ctx = self
            .authorize(&request, &request.get_ref().org_id, None)
            .await?;
        let req = request.into_inner();
        let grants = self.grants(&ctx).await;

        // Phase 1 RLS: single-org read, same rationale as `get_chunks` above.
        // Committed before `join_sources`, which opens its own scoped
        // transaction for the same org.
        let mut tx = pg_org_scope::begin_org_scoped(&self.pipeline.pool, &ctx.org_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let rows = sqlx::query_as::<_, (String, String, String, i32)>(
            "SELECT ku.knowledge_id, ku.document_id, ku.text, ku.chunk_index
             FROM knowledge_units ku
             JOIN documents d ON d.document_id = ku.document_id
             WHERE ku.knowledge_id = ANY($1) AND ku.org_id = $2 AND d.deleted_at IS NULL
               AND (d.owner_id = $3 OR d.visibility = 'org' OR d.document_id = ANY($4))",
        )
        .bind(&req.knowledge_ids)
        .bind(&ctx.org_id)
        .bind(ctx.user_id.as_deref())
        .bind(&grants)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let candidates: Vec<crate::pipeline::types::ScoredCandidate> = rows
            .into_iter()
            .map(
                |(kid, did, text, idx)| crate::pipeline::types::ScoredCandidate {
                    knowledge_id: kid,
                    document_id: did,
                    text,
                    dense_score: 1.0,
                    sparse_score: 0.0,
                    rerank_score: 1.0,
                    final_score: 1.0,
                    chunk_index: idx,
                    metadata: Default::default(),
                },
            )
            .collect();

        let sources = self
            .pipeline
            .join_sources(&ctx.org_id, &candidates)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let packed = pack_context(
            &candidates,
            &sources,
            req.budget_tokens as usize,
            &req.format,
        );

        let pack = ContextPack {
            facts: packed
                .facts
                .iter()
                .map(|f| ContextFact {
                    knowledge_id: f.knowledge_id.clone(),
                    document_id: f.document_id.clone(),
                    text: f.text.clone(),
                    score: f.score,
                    source_title: f.source_title.clone(),
                    source_type: f.source_type.clone(),
                    estimated_tokens: f.estimated_tokens as i32,
                })
                .collect(),
            total_tokens: packed.total_tokens as i32,
            budget_tokens: packed.budget_tokens as i32,
            format: packed.format,
        };

        Ok(Response::new(PackContextResponse { pack: Some(pack) }))
    }
}

/// §17.3.2 — shared proto → pipeline mapper. The streaming and unary
/// handlers both call this so the wire contract stays identical
/// regardless of frame shape.
fn parse_zdr_mode(mode: Option<String>) -> Result<Option<ZdrMode>, &'static str> {
    mode.map(|value| value.parse::<ZdrMode>()).transpose()
}

/// Apply only verified boundary authority to the pipeline request. Keeping the
/// unary and streaming gRPC handlers on this shared path prevents either wire
/// shape from downgrading signed ZDR or caller identity.
fn apply_verified_context(ctx: &crate::authz::AuthContext, req: &mut PipelineReq) {
    ctx.apply_to_request(req);
}

fn grpc_to_pipeline(req: RetrieveRequest) -> Result<PipelineReq, &'static str> {
    let filters = req.filters.unwrap_or_default();
    let zdr_mode = parse_zdr_mode(req.zdr_mode)?;
    Ok(PipelineReq {
        org_id: req.org_id,
        query: req.query,
        top_k: if req.top_k > 0 {
            Some(req.top_k as usize)
        } else {
            None
        },
        top_n: req.top_k_before_rerank.map(|v| v as usize),
        filters: RetrievalFiltersInput {
            document_types: filters.document_types,
            departments: filters.departments,
            languages: filters.languages,
            document_ids: filters.document_ids,
            sources: filters.sources,
            region: if filters.region.is_empty() {
                None
            } else {
                Some(filters.region)
            },
            workspaces: filters.workspace_ids,
            collections: filters.collection_ids,
            acl_tags: vec![],
        },
        user_id: req.user_id,
        verified_bearer: None,
        query_expansion: req.query_expansion,
        reranker_model: req.reranker_model,
        zdr_mode,
        // Proto field 14. Was hardcoded `None`, which the orchestrator resolves
        // to the fail-closed `true` — unsatisfiable by the configured embedding
        // provider, so it took every gRPC dense retrieval down. `None` still
        // means "caller said nothing" and still fails closed; the point is that
        // a caller can now say something, as it always could over HTTP.
        sovereign_required: req.sovereign_required,
        mode_mix: None,
        context_budget_tokens: req.context_budget_tokens.map(|v| v as usize),
        context_format: req.context_format,
        agent_id: req.agent_id,
        admin_read_all: false,
        space_scope: None,
    })
}

#[cfg(test)]
mod retrieval_metadata_tests {
    use super::retrieval_metadata;

    /// Reads a `ListValue` of strings back out of the Struct, so the assertions
    /// below check what a client would actually decode rather than just that
    /// some key is present.
    fn list(md: &prost_types::Struct, key: &str) -> Vec<String> {
        let Some(prost_types::Value {
            kind: Some(prost_types::value::Kind::ListValue(l)),
        }) = md.fields.get(key)
        else {
            panic!("{key} missing or not a list: {:?}", md.fields.get(key));
        };
        l.values
            .iter()
            .map(|v| match &v.kind {
                Some(prost_types::value::Kind::StringValue(s)) => s.clone(),
                other => panic!("non-string list entry: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn planner_hints_reach_the_grpc_caller() {
        let md = retrieval_metadata(
            &[
                "/v1/retrieve/graph".to_string(),
                "/v1/retrieve/wiki".to_string(),
            ],
            &[],
        )
        .expect("hints present, so metadata must be populated");
        assert_eq!(
            list(&md, "suggested_next_tools"),
            vec!["/v1/retrieve/graph", "/v1/retrieve/wiki"]
        );
        // Absent rather than an empty list: nothing was enforced, and an empty
        // list would read as "enforcement ran and did nothing".
        assert!(!md.fields.contains_key("zdr_actions_applied"));
    }

    #[test]
    fn zdr_enforcement_reaches_the_grpc_caller() {
        // The regression this guards: ZDR actions were HTTP-only, so a gRPC
        // caller could not observe — and therefore could not propagate —
        // enforcement that had actually been applied to its own results.
        let md = retrieval_metadata(&[], &["reject_mode_filtered_restricted".to_string()])
            .expect("zdr actions present, so metadata must be populated");
        assert_eq!(
            list(&md, "zdr_actions_applied"),
            vec!["reject_mode_filtered_restricted"]
        );
    }

    #[test]
    fn hint_order_is_preserved() {
        // Order is the pipeline's confidence ordering — most-productive first.
        // A BTreeMap keys the Struct, but the LIST inside a key must not be
        // reordered, or the top hint stops being the top hint.
        let hints: Vec<String> = [
            "/v1/retrieve/wiki",
            "/v1/knowledge/search",
            "/v1/retrieve/graph",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let md = retrieval_metadata(&hints, &[]).expect("populated");
        assert_eq!(list(&md, "suggested_next_tools"), hints);
    }

    #[test]
    fn a_quiet_response_stays_off_the_wire() {
        // Not `Some(empty Struct)`: a confident retrieval with ZDR off must
        // encode exactly as it did before this field was populated.
        assert!(retrieval_metadata(&[], &[]).is_none());
    }
}

#[cfg(test)]
mod zdr_boundary_tests {
    use super::{
        apply_verified_context, grpc_to_pipeline, parse_zdr_mode, space_decision_from_metadata,
        RetrieveRequest, ZdrMode,
    };
    use crate::authz::{AuthContext, AuthMethod, EffectiveAcl};
    use tonic::metadata::{MetadataMap, MetadataValue};
    use tonic::{Code, Status};

    fn ctx(zdr: bool) -> AuthContext {
        AuthContext {
            user_id: Some("verified-user".into()),
            org_id: "verified-org".into(),
            auth_method: AuthMethod::Jwt,
            scopes: vec![],
            zdr,
            // This helper is specifically the ZDR boundary test; sovereignty
            // is orthogonal, so it stays at the permissive `false` here
            // rather than gaining its own parameter nobody exercises yet.
            sovereign: Some(false),
            acl: EffectiveAcl::allow_all(),
            request_id: "grpc-zdr-boundary".into(),
            verified_bearer: None,
        }
    }

    fn request(zdr_mode: Option<&str>) -> RetrieveRequest {
        RetrieveRequest {
            org_id: "verified-org".into(),
            query: "synthetic boundary query".into(),
            zdr_mode: zdr_mode.map(str::to_owned),
            ..Default::default()
        }
    }

    /// A gRPC caller can state its sovereignty posture at all.
    ///
    /// This was hardcoded `None`, and the orchestrator resolves `None` to the
    /// fail-closed `true`, which the configured embedding provider cannot
    /// satisfy — so every gRPC dense retrieval returned an error, while the same
    /// query over HTTP (where the field has always existed) returned results.
    /// The test is about the field being *carried*, not about which default is
    /// right: `None` still means "said nothing" and still fails closed.
    #[test]
    fn sovereignty_posture_survives_the_grpc_boundary() {
        for requested in [None, Some(false), Some(true)] {
            let req = RetrieveRequest {
                sovereign_required: requested,
                ..request(Some("disabled"))
            };
            let pipeline = grpc_to_pipeline(req).expect("valid request");
            assert_eq!(
                pipeline.sovereign_required, requested,
                "gRPC must forward the caller's sovereignty posture verbatim, \
                 including the absence of one"
            );
        }
    }

    #[test]
    fn grpc_space_decision_is_an_explicit_metadata_authority() {
        let mut metadata = MetadataMap::new();
        assert_eq!(
            space_decision_from_metadata(&metadata).expect("no metadata is legacy"),
            None
        );
        metadata.insert(
            "x-space-decision",
            MetadataValue::try_from("signed-control-decision").expect("valid ascii metadata"),
        );
        assert_eq!(
            space_decision_from_metadata(&metadata).expect("metadata parses"),
            Some("signed-control-decision")
        );
    }

    #[test]
    fn grpc_signed_zdr_forces_ephemeral_and_preserves_reject() {
        for requested in [None, Some("disabled"), Some("ephemeral")] {
            let mut pipeline = grpc_to_pipeline(request(requested)).expect("valid gRPC request");
            apply_verified_context(&ctx(true), &mut pipeline);
            assert_eq!(pipeline.zdr_mode, Some(ZdrMode::Ephemeral));
        }

        let mut reject = grpc_to_pipeline(request(Some("reject"))).expect("valid gRPC request");
        apply_verified_context(&ctx(true), &mut reject);
        assert_eq!(reject.zdr_mode, Some(ZdrMode::Reject));
    }

    #[test]
    fn grpc_signed_non_zdr_preserves_stricter_requested_posture() {
        for requested in [None, Some("disabled"), Some("reject"), Some("ephemeral")] {
            let mut pipeline = grpc_to_pipeline(request(requested)).expect("valid gRPC request");
            let original = pipeline.zdr_mode;
            apply_verified_context(&ctx(false), &mut pipeline);
            assert_eq!(pipeline.zdr_mode, original);
        }
    }

    #[test]
    fn grpc_rejects_unknown_and_case_variant_zdr_modes() {
        for mode in ["Ephemeral", "unknown", ""] {
            let message = parse_zdr_mode(Some(mode.to_owned()))
                .expect_err("free-form mode must fail closed at gRPC boundary");
            assert_eq!(
                Status::invalid_argument(message).code(),
                Code::InvalidArgument
            );
        }
    }

    #[test]
    fn grpc_accepts_only_exact_supported_zdr_modes() {
        assert_eq!(
            parse_zdr_mode(Some("ephemeral".to_owned())).expect("exact mode"),
            Some(ZdrMode::Ephemeral)
        );
        assert_eq!(parse_zdr_mode(None).expect("absent mode"), None);
    }
}
