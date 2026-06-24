use std::sync::Arc;

use sqlx::PgPool;
use tonic::{Request, Response, Status};

use super::pb_documents::document_service_server::DocumentService;
use super::pb_documents::*;

pub struct DocumentSvc {
    pool: Arc<PgPool>,
}

impl DocumentSvc {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[tonic::async_trait]
impl DocumentService for DocumentSvc {
    async fn get_document(
        &self,
        request: Request<GetDocumentRequest>,
    ) -> Result<Response<GetDocumentResponse>, Status> {
        let req = request.into_inner();
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
             FROM documents WHERE document_id = $1 AND org_id = $2 AND deleted_at IS NULL",
        )
        .bind(&req.document_id)
        .bind(&req.org_id)
        .fetch_optional(self.pool.as_ref())
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
        let req = request.into_inner();
        let limit = if req.limit > 0 { req.limit } else { 50 };
        let offset = req.offset;

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
                 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(&req.org_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(self.pool.as_ref())
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
                 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(&req.org_id)
            .bind(limit)
            .bind(offset)
            .bind(&req.r#type)
            .fetch_all(self.pool.as_ref())
            .await
        }
        .map_err(|e| Status::internal(e.to_string()))?;

        let total_row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM documents WHERE org_id = $1 AND deleted_at IS NULL",
        )
        .bind(&req.org_id)
        .fetch_one(self.pool.as_ref())
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
        // §17.3.4 — gRPC `DocumentService.CreateDocument` is deprecated.
        // The canonical write path is `documents-api-go POST /v1/documents`
        // (and `/v1/documents/bulk`), which carries validation,
        // idempotency, outbox publish, org_version bump. This gRPC path
        // bypasses all of that and only writes the Postgres row. We
        // refuse new writes here and tell callers where to migrate.
        // Until every Model Plane caller is on HTTP we keep the route
        // discoverable but no-op-with-error.
        if std::env::var("DPV2_ALLOW_GRPC_DOCUMENT_WRITES").as_deref() != Ok("1") {
            return Err(Status::failed_precondition(
                "gRPC DocumentService.CreateDocument is deprecated (§17.3.4); \
                 use documents-api-go POST /v1/documents instead. \
                 Set DPV2_ALLOW_GRPC_DOCUMENT_WRITES=1 to opt back in for migration.",
            ));
        }
        tracing::warn!("deprecated gRPC create_document called; migrate to documents-api-go HTTP");
        let req = request.into_inner();
        let zdr = if req.zdr_classification.is_empty() {
            "internal".to_string()
        } else {
            req.zdr_classification
        };

        let row = sqlx::query_as::<_, (String,)>(
            "INSERT INTO documents (org_id, source, type, title, content, zdr_classification)
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING document_id",
        )
        .bind(&req.org_id)
        .bind(&req.source)
        .bind(&req.r#type)
        .bind(&req.title)
        .bind(&req.content)
        .bind(&zdr)
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(CreateDocumentResponse {
            document: Some(Document {
                document_id: row.0,
                org_id: req.org_id,
                source: req.source,
                r#type: req.r#type,
                title: req.title,
                content: req.content,
                status: "pending".into(),
                metadata: None,
                created_at: None,
                updated_at: None,
                deleted_at: None,
                zdr_classification: zdr,
                zdr_reason: None,
            }),
        }))
    }

    async fn delete_document(
        &self,
        request: Request<DeleteDocumentRequest>,
    ) -> Result<Response<DeleteDocumentResponse>, Status> {
        // §17.3.4 — same deprecation gate as `create_document`. Soft-delete
        // bypasses org_version bump + outbox publish on this path.
        if std::env::var("DPV2_ALLOW_GRPC_DOCUMENT_WRITES").as_deref() != Ok("1") {
            return Err(Status::failed_precondition(
                "gRPC DocumentService.DeleteDocument is deprecated (§17.3.4); \
                 use documents-api-go DELETE /v1/documents/{id} instead.",
            ));
        }
        tracing::warn!("deprecated gRPC delete_document called; migrate to documents-api-go HTTP");
        let req = request.into_inner();
        let result = sqlx::query(
            "UPDATE documents SET deleted_at = NOW() WHERE document_id = $1 AND org_id = $2 AND deleted_at IS NULL"
        )
        .bind(&req.document_id)
        .bind(&req.org_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(DeleteDocumentResponse {
            success: result.rows_affected() > 0,
        }))
    }

    async fn bulk_ingest(
        &self,
        request: Request<BulkIngestRequest>,
    ) -> Result<Response<BulkIngestResponse>, Status> {
        // §17.3.4 — deprecated. See `create_document` for migration path.
        if std::env::var("DPV2_ALLOW_GRPC_DOCUMENT_WRITES").as_deref() != Ok("1") {
            return Err(Status::failed_precondition(
                "gRPC DocumentService.BulkIngest is deprecated (§17.3.4); \
                 use documents-api-go POST /v1/documents/bulk instead.",
            ));
        }
        tracing::warn!("deprecated gRPC bulk_ingest called; migrate to documents-api-go HTTP");
        let req = request.into_inner();
        let mut accepted = 0i32;
        let mut rejected = 0i32;
        let mut doc_ids = Vec::new();

        for doc in &req.documents {
            let zdr = if doc.zdr_classification.is_empty() {
                "internal"
            } else {
                &doc.zdr_classification
            };
            match sqlx::query_as::<_, (String,)>(
                "INSERT INTO documents (org_id, source, type, title, content, zdr_classification)
                 VALUES ($1, $2, $3, $4, $5, $6) RETURNING document_id",
            )
            .bind(&req.org_id)
            .bind(&doc.source)
            .bind(&doc.r#type)
            .bind(&doc.title)
            .bind(&doc.content)
            .bind(zdr)
            .fetch_one(self.pool.as_ref())
            .await
            {
                Ok((id,)) => {
                    accepted += 1;
                    doc_ids.push(id);
                }
                Err(_) => {
                    rejected += 1;
                }
            }
        }

        Ok(Response::new(BulkIngestResponse {
            accepted,
            rejected,
            document_ids: doc_ids,
        }))
    }

    async fn get_document_index_status(
        &self,
        request: Request<GetDocumentIndexStatusRequest>,
    ) -> Result<Response<GetDocumentIndexStatusResponse>, Status> {
        let req = request.into_inner();

        let chunk_row = sqlx::query_as::<_, (i64, i64, i64)>(
            // embedding-engine writes status 'done' (batch/mod.rs mark_units_done);
            // the prior 'completed' literal never matched, so the embedded count
            // was always 0. Align to 'done'.
            "SELECT COUNT(*),
                    COUNT(*) FILTER (WHERE embedding_status = 'done'),
                    COUNT(*) FILTER (WHERE embedding_status = 'failed')
             FROM knowledge_units WHERE document_id = $1 AND org_id = $2",
        )
        .bind(&req.document_id)
        .bind(&req.org_id)
        .fetch_one(self.pool.as_ref())
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
                org_id: req.org_id,
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
        let _req = request.into_inner();
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
