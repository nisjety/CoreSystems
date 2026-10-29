use std::sync::Arc;

use sqlx::PgPool;
use tonic::{Request, Response, Status};

use super::pb_documents::document_service_server::DocumentService;
use super::pb_documents::*;

pub struct DocumentSvc {
    pool: Arc<PgPool>,
    policy: Arc<dyn crate::authz::PolicyClient>,
    visibility: Arc<dyn crate::authz::VisibilityClient>,
}

impl DocumentSvc {
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

    // Keep the native tonic error type at this authorization boundary.
    #[allow(clippy::result_large_err)]
    fn require_write_scope(ctx: &crate::authz::AuthContext) -> Result<(), Status> {
        if ctx.scopes.iter().any(|scope| scope == "org:data:write_all") {
            Ok(())
        } else {
            Err(Status::permission_denied("document write scope required"))
        }
    }

    fn deprecated_write(operation: &str, canonical_route: &str) -> Status {
        Status::failed_precondition(format!(
            "gRPC DocumentService.{operation} is disabled; use documents-api-go {canonical_route}"
        ))
    }
}

#[tonic::async_trait]
impl DocumentService for DocumentSvc {
    async fn get_document(
        &self,
        request: Request<GetDocumentRequest>,
    ) -> Result<Response<GetDocumentResponse>, Status> {
        let ctx = self.authorize(&request, &request.get_ref().org_id).await?;
        let req = request.into_inner();
        let grants = self.grants(&ctx).await;
        // Phase 1 RLS: the org comes from the authorized `AuthContext`, so this
        // reads through an org-scoped transaction. The SQL still binds `org_id`
        // itself — the database policy is a backstop against that filter being
        // dropped or mis-edited later, not a replacement for it.
        let mut tx = pg_org_scope::begin_org_scoped(self.pool.as_ref(), &ctx.org_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                String,
            ),
        >(
            "SELECT document_id, org_id, source, type, title, content, status, zdr_classification
             FROM documents WHERE document_id = $1 AND org_id = $2 AND deleted_at IS NULL
               AND (owner_id = $3 OR visibility = 'org' OR document_id = ANY($4))",
        )
        .bind(&req.document_id)
        .bind(&ctx.org_id)
        .bind(ctx.user_id.as_deref())
        .bind(&grants)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        match row {
            Some((did, oid, source, dtype, title, content, status, zdr)) => {
                Ok(Response::new(GetDocumentResponse {
                    document: Some(Document {
                        document_id: did,
                        org_id: oid,
                        source,
                        r#type: dtype,
                        title,
                        content,
                        status,
                        metadata: None,
                        created_at: None,
                        updated_at: None,
                        deleted_at: None,
                        zdr_classification: zdr,
                        zdr_reason: None,
                    }),
                }))
            }
            None => Err(Status::not_found("document not found")),
        }
    }

    async fn list_documents(
        &self,
        request: Request<ListDocumentsRequest>,
    ) -> Result<Response<ListDocumentsResponse>, Status> {
        let ctx = self.authorize(&request, &request.get_ref().org_id).await?;
        let req = request.into_inner();
        let grants = self.grants(&ctx).await;
        let limit = if req.limit > 0 { req.limit } else { 50 };
        let offset = req.offset;

        // Phase 1 RLS: all three statements below (the page, either variant, and
        // the total count) serve the same org, so they share ONE scoped
        // transaction — which also makes the count consistent with the page.
        let mut tx = pg_org_scope::begin_org_scoped(self.pool.as_ref(), &ctx.org_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let rows = if req.r#type.is_empty() {
            sqlx::query_as::<
                _,
                (
                    String,
                    String,
                    String,
                    String,
                    String,
                    String,
                    String,
                    String,
                ),
            >(
                "SELECT document_id, org_id, source, type, title, '', status, zdr_classification
                 FROM documents WHERE org_id = $1 AND deleted_at IS NULL
                   AND (owner_id = $4 OR visibility = 'org' OR document_id = ANY($5))
                 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(&ctx.org_id)
            .bind(limit)
            .bind(offset)
            .bind(ctx.user_id.as_deref())
            .bind(&grants)
            .fetch_all(&mut *tx)
            .await
        } else {
            sqlx::query_as::<
                _,
                (
                    String,
                    String,
                    String,
                    String,
                    String,
                    String,
                    String,
                    String,
                ),
            >(
                "SELECT document_id, org_id, source, type, title, '', status, zdr_classification
                 FROM documents WHERE org_id = $1 AND type = $4 AND deleted_at IS NULL
                   AND (owner_id = $5 OR visibility = 'org' OR document_id = ANY($6))
                 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(&ctx.org_id)
            .bind(limit)
            .bind(offset)
            .bind(&req.r#type)
            .bind(ctx.user_id.as_deref())
            .bind(&grants)
            .fetch_all(&mut *tx)
            .await
        }
        .map_err(|e| Status::internal(e.to_string()))?;

        let total_row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM documents WHERE org_id = $1 AND deleted_at IS NULL
               AND (owner_id = $2 OR visibility = 'org' OR document_id = ANY($3))",
        )
        .bind(&ctx.org_id)
        .bind(ctx.user_id.as_deref())
        .bind(&grants)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let documents: Vec<Document> = rows
            .into_iter()
            .map(
                |(did, oid, source, dtype, title, content, status, zdr)| Document {
                    document_id: did,
                    org_id: oid,
                    source,
                    r#type: dtype,
                    title,
                    content,
                    status,
                    metadata: None,
                    created_at: None,
                    updated_at: None,
                    deleted_at: None,
                    zdr_classification: zdr,
                    zdr_reason: None,
                },
            )
            .collect();

        Ok(Response::new(ListDocumentsResponse {
            documents,
            total: total_row.0 as i32,
        }))
    }

    async fn create_document(
        &self,
        request: Request<CreateDocumentRequest>,
    ) -> Result<Response<CreateDocumentResponse>, Status> {
        let ctx = self.authorize(&request, &request.get_ref().org_id).await?;
        Self::require_write_scope(&ctx)?;
        Err(Self::deprecated_write(
            "CreateDocument",
            "POST /v1/documents instead",
        ))
    }

    async fn delete_document(
        &self,
        request: Request<DeleteDocumentRequest>,
    ) -> Result<Response<DeleteDocumentResponse>, Status> {
        let ctx = self.authorize(&request, &request.get_ref().org_id).await?;
        Self::require_write_scope(&ctx)?;
        Err(Self::deprecated_write(
            "DeleteDocument",
            "DELETE /v1/documents/{id} instead",
        ))
    }

    async fn bulk_ingest(
        &self,
        request: Request<BulkIngestRequest>,
    ) -> Result<Response<BulkIngestResponse>, Status> {
        let ctx = self.authorize(&request, &request.get_ref().org_id).await?;
        Self::require_write_scope(&ctx)?;
        Err(Self::deprecated_write(
            "BulkIngest",
            "POST /v1/documents/bulk instead",
        ))
    }

    async fn get_document_index_status(
        &self,
        request: Request<GetDocumentIndexStatusRequest>,
    ) -> Result<Response<GetDocumentIndexStatusResponse>, Status> {
        let ctx = self.authorize(&request, &request.get_ref().org_id).await?;
        let req = request.into_inner();
        let grants = self.grants(&ctx).await;

        // Phase 1 RLS: single-org read, same rationale as `get_document` above.
        // Note the `documents` join carries no org predicate of its own — RLS
        // now supplies one for that side too.
        let mut tx = pg_org_scope::begin_org_scoped(self.pool.as_ref(), &ctx.org_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let chunk_row = sqlx::query_as::<_, (i64, i64, i64)>(
            // embedding-engine writes status 'done' (batch/mod.rs mark_units_done);
            // the prior 'completed' literal never matched, so the embedded count
            // was always 0. Align to 'done'.
            "SELECT COUNT(*),
                    COUNT(*) FILTER (WHERE embedding_status = 'done'),
                    COUNT(*) FILTER (WHERE embedding_status = 'failed')
             FROM knowledge_units ku
             JOIN documents d ON d.document_id = ku.document_id
             WHERE ku.document_id = $1 AND ku.org_id = $2
               AND d.deleted_at IS NULL
               AND (d.owner_id = $3 OR d.visibility = 'org' OR d.document_id = ANY($4))",
        )
        .bind(&req.document_id)
        .bind(&ctx.org_id)
        .bind(ctx.user_id.as_deref())
        .bind(&grants)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let (total, synced, failed) = chunk_row;
        let embed_status = if failed > 0 {
            "failed"
        } else if synced == total && total > 0 {
            "completed"
        } else {
            "pending"
        };
        let chunk_status = if total > 0 { "chunked" } else { "pending" };

        Ok(Response::new(GetDocumentIndexStatusResponse {
            status: Some(DocumentIndexStatus {
                document_id: req.document_id,
                org_id: ctx.org_id,
                chunk_count: total as i32,
                chunk_status: chunk_status.into(),
                embed_status: embed_status.into(),
                embeddings_synced: synced as i32,
                vector_status: embed_status.into(),
                last_indexed_at: None,
                error_message: None,
            }),
        }))
    }

    async fn get_ingest_status(
        &self,
        request: Request<IngestStatusRequest>,
    ) -> Result<Response<IngestStatusResponse>, Status> {
        self.authorize(&request, &request.get_ref().org_id).await?;
        Ok(Response::new(IngestStatusResponse {
            policy: Some(IngestPolicy {
                zdr_mode: "disabled".into(),
                index_schedule: "realtime".into(),
                ephemeral_only: false,
            }),
            is_active: true,
            next_batch_window: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sqlx::PgPool;
    use tonic::{Code, Request};

    use super::*;
    use crate::authz::{
        AuthContext, AuthMethod, EffectiveAcl, NoopPolicyClient, NoopVisibilityClient,
    };
    use crate::grpc::pb_documents::document_service_server::DocumentService;

    async fn closed_pool_service() -> DocumentSvc {
        let pool = PgPool::connect_lazy("postgresql://unused:unused@127.0.0.1:1/unused")
            .expect("valid lazy test pool configuration");
        pool.close().await;
        DocumentSvc::new(
            Arc::new(pool),
            Arc::new(NoopPolicyClient),
            Arc::new(NoopVisibilityClient),
        )
    }

    fn authenticated<T>(message: T) -> Request<T> {
        let mut request = Request::new(message);
        request.extensions_mut().insert(AuthContext {
            user_id: Some("user-test".into()),
            org_id: "org-test".into(),
            auth_method: AuthMethod::Jwt,
            scopes: vec!["org:data:write_all".into()],
            zdr: false,
            acl: EffectiveAcl::allow_all(),
            request_id: "grpc-document-write-regression".into(),
            verified_bearer: None,
        });
        request
    }

    fn restrictive_policy() -> IngestPolicy {
        IngestPolicy {
            zdr_mode: "ephemeral".into(),
            index_schedule: "disabled".into(),
            ephemeral_only: true,
        }
    }

    fn create_request(ingest_policy: Option<IngestPolicy>) -> CreateDocumentRequest {
        CreateDocumentRequest {
            org_id: "org-test".into(),
            source: "regression".into(),
            r#type: "text".into(),
            title: "non-sensitive fixture".into(),
            content: "non-sensitive fixture".into(),
            metadata: None,
            zdr_classification: "restricted".into(),
            ingest_policy,
        }
    }

    #[tokio::test]
    async fn legacy_environment_gate_alone_cannot_enable_create() {
        std::env::set_var("DPV2_ALLOW_GRPC_DOCUMENT_WRITES", "1");
        let service = closed_pool_service().await;

        let error = service
            .create_document(authenticated(create_request(None)))
            .await
            .expect_err("deprecated gRPC create must remain disabled");

        assert_eq!(error.code(), Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn restrictive_zdr_create_is_rejected_before_persistence() {
        std::env::set_var("DPV2_ALLOW_GRPC_DOCUMENT_WRITES", "1");
        let service = closed_pool_service().await;

        let error = service
            .create_document(authenticated(create_request(Some(restrictive_policy()))))
            .await
            .expect_err("restrictive ZDR create must never reach storage");

        assert_eq!(error.code(), Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn restrictive_zdr_bulk_is_rejected_before_persistence() {
        std::env::set_var("DPV2_ALLOW_GRPC_DOCUMENT_WRITES", "1");
        let service = closed_pool_service().await;
        let request = BulkIngestRequest {
            org_id: "org-test".into(),
            documents: vec![create_request(Some(restrictive_policy()))],
            ingest_policy: Some(restrictive_policy()),
        };

        let error = service
            .bulk_ingest(authenticated(request))
            .await
            .expect_err("restrictive ZDR bulk must never reach storage");

        assert_eq!(error.code(), Code::FailedPrecondition);
    }
}
