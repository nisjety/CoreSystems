use std::sync::Arc;

use sqlx::PgPool;
use tonic::{Request, Response, Status};

use super::pb_knowledge::knowledge_service_server::KnowledgeService;
use super::pb_knowledge::*;

pub struct KnowledgeSvc {
    pool: Arc<PgPool>,
    policy: Arc<dyn crate::authz::PolicyClient>,
    visibility: Arc<dyn crate::authz::VisibilityClient>,
}

impl KnowledgeSvc {
    pub fn new(
        pool: Arc<PgPool>,
        policy: Arc<dyn crate::authz::PolicyClient>,
        visibility: Arc<dyn crate::authz::VisibilityClient>,
    ) -> Self {
        Self {
            pool,
            policy,
            visibility,
        }
    }

    async fn authorize<T>(
        &self,
        request: &Request<T>,
        org_id: &str,
    ) -> Result<crate::authz::AuthContext, Status> {
        super::interceptor::authorize_request(self.policy.as_ref(), request, org_id, None).await
    }

    async fn grants(&self, ctx: &crate::authz::AuthContext) -> Vec<String> {
        match ctx.user_id.as_deref() {
            Some(user_id) => {
                self.visibility
                    .visible_documents(&ctx.org_id, user_id, ctx.verified_bearer.as_deref())
                    .await
            }
            None => Vec::new(),
        }
    }
}

#[tonic::async_trait]
impl KnowledgeService for KnowledgeSvc {
    async fn check_permissions(
        &self,
        request: Request<CheckPermissionsRequest>,
    ) -> Result<Response<CheckPermissionsResponse>, Status> {
        let ctx = self.authorize(&request, &request.get_ref().org_id).await?;
        let req = request.into_inner();
        let grants = self.grants(&ctx).await;

        let row = sqlx::query_as::<_, (String,)>(
            "SELECT zdr_classification FROM documents
             WHERE document_id = $1 AND org_id = $2 AND deleted_at IS NULL
               AND (owner_id = $3 OR visibility = 'org' OR document_id = ANY($4))",
        )
        .bind(&req.document_id)
        .bind(&ctx.org_id)
        .bind(ctx.user_id.as_deref())
        .bind(&grants)
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
        let ctx = self.authorize(&request, &request.get_ref().org_id).await?;
        let req = request.into_inner();
        let grants = self.grants(&ctx).await;

        let rows = sqlx::query_as::<_, (String, String, String, i32, String, String, String)>(
            "SELECT ku.knowledge_id, ku.document_id, ku.org_id, ku.chunk_index,
                    ku.text, ku.embedding_status, ku.content_hash
             FROM knowledge_units ku
             JOIN documents d ON d.document_id = ku.document_id
             WHERE ku.document_id = $1 AND ku.org_id = $2 AND d.deleted_at IS NULL
               AND (d.owner_id = $3 OR d.visibility = 'org' OR d.document_id = ANY($4))
             ORDER BY ku.chunk_index",
        )
        .bind(&req.document_id)
        .bind(&ctx.org_id)
        .bind(ctx.user_id.as_deref())
        .bind(&grants)
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
