use std::sync::Arc;

use sqlx::PgPool;
use tonic::{Request, Response, Status};

use super::pb_knowledge::knowledge_service_server::KnowledgeService;
use super::pb_knowledge::*;

pub struct KnowledgeSvc {
    pool: Arc<PgPool>,
}

impl KnowledgeSvc {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[tonic::async_trait]
impl KnowledgeService for KnowledgeSvc {
    async fn check_permissions(
        &self,
        request: Request<CheckPermissionsRequest>,
    ) -> Result<Response<CheckPermissionsResponse>, Status> {
        let req = request.into_inner();

        let row = sqlx::query_as::<_, (String,)>(
            "SELECT zdr_classification FROM documents
             WHERE document_id = $1 AND org_id = $2 AND deleted_at IS NULL",
        )
        .bind(&req.document_id)
        .bind(&req.org_id)
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        match row {
            Some((classification,)) => {
                let allowed = classification != "restricted";
                let reason = if allowed {
                    String::new()
                } else {
                    "document classified as restricted".into()
                };
                Ok(Response::new(CheckPermissionsResponse { allowed, reason }))
            }
            None => Err(Status::not_found("document not found")),
        }
    }

    async fn get_knowledge_units(
        &self,
        request: Request<GetKnowledgeUnitsRequest>,
    ) -> Result<Response<GetKnowledgeUnitsResponse>, Status> {
        let req = request.into_inner();

        let rows = sqlx::query_as::<_, (String, String, String, i32, String, String, String)>(
            "SELECT ku.knowledge_id, ku.document_id, ku.org_id, ku.chunk_index,
                    ku.text, ku.embedding_status, ku.content_hash
             FROM knowledge_units ku
             JOIN documents d ON d.document_id = ku.document_id
             WHERE ku.document_id = $1 AND ku.org_id = $2 AND d.deleted_at IS NULL
             ORDER BY ku.chunk_index",
        )
        .bind(&req.document_id)
        .bind(&req.org_id)
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        let units: Vec<KnowledgeUnit> = rows
            .into_iter()
            .map(|(kid, did, oid, idx, text, status, hash)| KnowledgeUnit {
                knowledge_id: kid,
                document_id: did,
                org_id: oid,
                chunk_index: idx,
                text,
                embedding_status: status,
                content_hash: hash,
                chunk_version: String::new(),
                parent_chunk_id: None,
                metadata: None,
                created_at: None,
                updated_at: None,
            })
            .collect();

        Ok(Response::new(GetKnowledgeUnitsResponse { units }))
    }
}
