use std::pin::Pin;
use std::sync::Arc;

use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::context_pack::pack_context;
use crate::pipeline::orchestrator::RetrievalPipeline;
use crate::pipeline::types::{RetrievalFiltersInput, RetrievalRequest as PipelineReq};
use crate::trace;

use super::pb_retrieval::retrieval_service_server::RetrievalService;
use super::pb_retrieval::*;

type StreamResp<T> = Pin<Box<dyn futures::Stream<Item = Result<T, Status>> + Send + 'static>>;

pub struct RetrievalSvc {
    pipeline: Arc<RetrievalPipeline>,
}

impl RetrievalSvc {
    pub fn new(pipeline: Arc<RetrievalPipeline>) -> Self {
        Self { pipeline }
    }
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
        let pipeline_req = grpc_to_pipeline(request.into_inner());

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
        let req = request.into_inner();

        let filters = req.filters.clone().unwrap_or_default();
        let pipeline_req = PipelineReq {
            org_id: req.org_id.clone(),
            query: req.query.clone(),
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
            user_id: req.user_id.clone(),
            query_expansion: req.query_expansion,
            reranker_model: req.reranker_model,
            zdr_mode: req.zdr_mode,
            // gRPC clients don't send per-request mode_mix yet — config defaults apply.
            mode_mix: None,
            context_budget_tokens: req.context_budget_tokens.map(|v| v as usize),
            context_format: req.context_format,
            // §16.1.4 — agent_id now on the proto contract (field 13).
            agent_id: req.agent_id,
        };

        let resp = self
            .pipeline
            .retrieve(pipeline_req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

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
        // the per-org rate limit. Values come from the same governor
        // limiter that gates HTTP (`PerOrgLimiter`). When the limiter
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
            retrieval_metadata: None,
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
        let req = request.into_inner();

        let detail = trace::get_trace(&self.pipeline.pool, &req.trace_id, &req.org_id)
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
                            dense_score: c.dense_score.unwrap_or(0.0),
                            sparse_score: c.sparse_score.unwrap_or(0.0),
                            rerank_score: c.rerank_score.unwrap_or(0.0),
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
        let req = request.into_inner();
        if req.document_ids.is_empty() {
            return Ok(Response::new(GetSourcesResponse { sources: vec![] }));
        }

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT document_id, title, source, type FROM documents
             WHERE document_id = ANY($1) AND org_id = $2 AND deleted_at IS NULL",
        )
        .bind(&req.document_ids)
        .bind(&req.org_id)
        .fetch_all(&self.pipeline.pool)
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
        let req = request.into_inner();

        let chunks = if !req.knowledge_ids.is_empty() {
            sqlx::query_as::<_, (String, String, String, i32, String, String)>(
                "SELECT ku.knowledge_id, ku.document_id, ku.text, ku.chunk_index, ku.content_hash, ku.embedding_status
                 FROM knowledge_units ku
                 JOIN documents d ON d.document_id = ku.document_id
                 WHERE ku.knowledge_id = ANY($1) AND ku.org_id = $2 AND d.deleted_at IS NULL
                 ORDER BY ku.chunk_index"
            )
            .bind(&req.knowledge_ids)
            .bind(&req.org_id)
            .fetch_all(&self.pipeline.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        } else if let Some(doc_id) = &req.document_id {
            sqlx::query_as::<_, (String, String, String, i32, String, String)>(
                "SELECT ku.knowledge_id, ku.document_id, ku.text, ku.chunk_index, ku.content_hash, ku.embedding_status
                 FROM knowledge_units ku
                 JOIN documents d ON d.document_id = ku.document_id
                 WHERE ku.document_id = $1 AND ku.org_id = $2 AND d.deleted_at IS NULL
                 ORDER BY ku.chunk_index"
            )
            .bind(doc_id)
            .bind(&req.org_id)
            .fetch_all(&self.pipeline.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        } else {
            return Err(Status::invalid_argument(
                "provide knowledge_ids or document_id",
            ));
        };

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
        let req = request.into_inner();

        let rows = sqlx::query_as::<_, (String, String, String, i32)>(
            "SELECT ku.knowledge_id, ku.document_id, ku.text, ku.chunk_index
             FROM knowledge_units ku
             JOIN documents d ON d.document_id = ku.document_id
             WHERE ku.knowledge_id = ANY($1) AND ku.org_id = $2 AND d.deleted_at IS NULL",
        )
        .bind(&req.knowledge_ids)
        .bind(&req.org_id)
        .fetch_all(&self.pipeline.pool)
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
            .join_sources(&candidates)
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
fn grpc_to_pipeline(req: RetrieveRequest) -> PipelineReq {
    let filters = req.filters.unwrap_or_default();
    PipelineReq {
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
        query_expansion: req.query_expansion,
        reranker_model: req.reranker_model,
        zdr_mode: req.zdr_mode,
        mode_mix: None,
        context_budget_tokens: req.context_budget_tokens.map(|v| v as usize),
        context_format: req.context_format,
        agent_id: req.agent_id,
    }
}
