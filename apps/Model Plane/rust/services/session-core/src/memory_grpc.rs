//! Durable `MemoryService` implementation backed by session-core `agent_memory`.

use crate::letta_adapter::LettaMemoryAdapter;
use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    memory_service_server::{MemoryService, MemoryServiceServer},
    IndexMemoryRequest, IndexMemoryResponse, MemoryEntry, MemoryHealthRequest,
    MemoryHealthResponse, SearchMemoryRequest, SearchMemoryResponse,
};
use sqlx::PgPool;
use tonic::{Request, Response, Status};

pub(crate) struct MemoryGrpc {
    pool: PgPool,
    letta: Option<LettaMemoryAdapter>,
}

impl MemoryGrpc {
    pub(crate) fn new(pool: PgPool, letta: Option<LettaMemoryAdapter>) -> Self {
        Self { pool, letta }
    }

    pub(crate) fn into_server(self) -> MemoryServiceServer<Self> {
        MemoryServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl MemoryService for MemoryGrpc {
    async fn search_memory(
        &self,
        request: Request<SearchMemoryRequest>,
    ) -> Result<Response<SearchMemoryResponse>, Status> {
        let req = request.into_inner();
        if req.org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if req.thread_id.trim().is_empty() {
            return Err(Status::invalid_argument("thread_id is required"));
        }

        let updated_after = req.updated_after.and_then(|ts| {
            u32::try_from(ts.nanos)
                .ok()
                .and_then(|nanos| DateTime::<Utc>::from_timestamp(ts.seconds, nanos))
        });

        let limit = if req.limit == 0 { 10 } else { req.limit };
        let topic_filter = req.topic_filter.clone();
        let rows = crate::dreaming::search_agent_memory(
            &self.pool,
            req.org_id.trim(),
            req.thread_id.trim(),
            &req.query,
            &topic_filter,
            limit,
            updated_after,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        let mut entries: Vec<MemoryEntry> = rows
            .into_iter()
            .map(|row| MemoryEntry {
                memory_id: row.id,
                thread_id: row.thread_id,
                topic: row.topic,
                content: row.content,
                score: row.score,
                updated_at: Some(prost_types::Timestamp {
                    seconds: row.updated_at.timestamp(),
                    nanos: i32::try_from(row.updated_at.timestamp_subsec_nanos())
                        .unwrap_or(i32::MAX),
                }),
            })
            .collect();

        if let Some(letta) = self.letta.as_ref() {
            let remaining = limit.saturating_sub(u32::try_from(entries.len()).unwrap_or(u32::MAX));
            if remaining > 0 {
                let letta_entries = letta
                    .search(
                        req.org_id.trim(),
                        req.thread_id.trim(),
                        &req.query,
                        &topic_filter,
                        remaining,
                    )
                    .await;
                for mut entry in letta_entries {
                    let content = entry.content.trim();
                    if content.is_empty()
                        || entries
                            .iter()
                            .any(|existing| existing.content.trim() == content)
                    {
                        continue;
                    }
                    entry.score *= 0.85;
                    entries.push(entry);
                    if entries.len() >= usize::try_from(limit).unwrap_or(usize::MAX) {
                        break;
                    }
                }
            }
        }

        Ok(Response::new(SearchMemoryResponse { entries }))
    }

    async fn index_memory(
        &self,
        request: Request<IndexMemoryRequest>,
    ) -> Result<Response<IndexMemoryResponse>, Status> {
        let req = request.into_inner();
        if req.org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if req.thread_id.trim().is_empty() {
            return Err(Status::invalid_argument("thread_id is required"));
        }
        if req.content.trim().is_empty() {
            return Err(Status::invalid_argument("content is required"));
        }

        let memory_id = crate::dreaming::index_agent_memory(
            &self.pool,
            req.org_id.trim(),
            req.thread_id.trim(),
            &req.topic,
            &req.content,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        if memory_id.is_empty() {
            return Err(Status::not_found("thread not found for org"));
        }

        if let Some(letta) = self.letta.as_ref() {
            letta
                .index(
                    req.org_id.trim(),
                    req.thread_id.trim(),
                    &req.topic,
                    &req.content,
                )
                .await;
        }

        Ok(Response::new(IndexMemoryResponse { memory_id }))
    }

    async fn health(
        &self,
        _request: Request<MemoryHealthRequest>,
    ) -> Result<Response<MemoryHealthResponse>, Status> {
        Ok(Response::new(MemoryHealthResponse {
            status: "ok".to_owned(),
        }))
    }
}
