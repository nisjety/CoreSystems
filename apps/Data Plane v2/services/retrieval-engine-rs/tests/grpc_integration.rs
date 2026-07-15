use std::net::SocketAddr;
use std::sync::Arc;

use sqlx::PgPool;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Server};
use tonic::{Request, Status};

use retrieval_engine::authz::{
    AuthContext, AuthMethod, EffectiveAcl, NoopPolicyClient, NoopVisibilityClient,
};
use retrieval_engine::grpc::document_svc::DocumentSvc;
use retrieval_engine::grpc::knowledge_svc::KnowledgeSvc;
use retrieval_engine::grpc::pb_documents;
use retrieval_engine::grpc::pb_documents::document_service_client::DocumentServiceClient;
use retrieval_engine::grpc::pb_documents::document_service_server::DocumentServiceServer;
use retrieval_engine::grpc::pb_knowledge;
use retrieval_engine::grpc::pb_knowledge::knowledge_service_client::KnowledgeServiceClient;
use retrieval_engine::grpc::pb_knowledge::knowledge_service_server::KnowledgeServiceServer;

const TEST_ORG: &str = "org-integration-test";

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

async fn setup_schema(pool: &PgPool) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS documents (
            document_id  TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
            org_id       TEXT NOT NULL,
            owner_id     TEXT NOT NULL DEFAULT 'test-user',
            visibility   TEXT NOT NULL DEFAULT 'org',
            source       TEXT NOT NULL DEFAULT '',
            type         TEXT NOT NULL DEFAULT '',
            title        TEXT NOT NULL DEFAULT '',
            content      TEXT NOT NULL DEFAULT '',
            status       TEXT NOT NULL DEFAULT 'pending',
            metadata     JSONB,
            zdr_classification TEXT NOT NULL DEFAULT 'internal',
            zdr_reason   TEXT,
            extraction_trace JSONB,
            created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            deleted_at   TIMESTAMPTZ
        )",
    )
    .execute(pool)
    .await
    .expect("create documents table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS knowledge_units (
            knowledge_id     TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
            document_id      TEXT NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
            org_id           TEXT NOT NULL,
            chunk_index      INTEGER NOT NULL DEFAULT 0,
            text             TEXT NOT NULL DEFAULT '',
            content_hash     TEXT NOT NULL DEFAULT '',
            embedding_status TEXT NOT NULL DEFAULT 'pending',
            metadata         JSONB,
            created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    .expect("create knowledge_units table");
}

async fn cleanup(pool: &PgPool) {
    sqlx::query("DELETE FROM knowledge_units WHERE org_id = $1")
        .bind(TEST_ORG)
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM documents WHERE org_id = $1")
        .bind(TEST_ORG)
        .execute(pool)
        .await
        .ok();
}

/// Test-fixture setup only. Production document writes must use
/// documents-api-go; direct SQL here keeps read-path integration tests
/// independent from the permanently disabled legacy gRPC mutations.
async fn insert_test_document(
    pool: &PgPool,
    org_id: &str,
    title: &str,
    content: &str,
    zdr_classification: &str,
) -> String {
    sqlx::query_as::<_, (String,)>(
        "INSERT INTO documents (org_id, source, type, title, content, zdr_classification)
         VALUES ($1, 'test-fixture', 'article', $2, $3, $4) RETURNING document_id",
    )
    .bind(org_id)
    .bind(title)
    .bind(content)
    .bind(zdr_classification)
    .fetch_one(pool)
    .await
    .expect("insert isolated test document")
    .0
}

async fn start_server(pool: PgPool) -> (SocketAddr, PgPool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().unwrap();

    let policy = Arc::new(NoopPolicyClient);
    let visibility = Arc::new(NoopVisibilityClient);
    let doc_svc = DocumentSvc::new(Arc::new(pool.clone()), policy.clone(), visibility.clone());
    let know_svc = KnowledgeSvc::new(Arc::new(pool.clone()), policy, visibility);

    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    tokio::spawn(async move {
        Server::builder()
            .add_service(DocumentServiceServer::with_interceptor(
                doc_svc,
                test_principal,
            ))
            .add_service(KnowledgeServiceServer::with_interceptor(
                know_svc,
                test_principal,
            ))
            .serve_with_incoming(incoming)
            .await
            .expect("grpc test server");
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (addr, pool)
}

async fn connect(addr: SocketAddr) -> Channel {
    Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap()
}

#[allow(clippy::result_large_err)]
fn test_principal(mut request: Request<()>) -> Result<Request<()>, Status> {
    request
        .metadata()
        .get("x-test-org")
        .filter(|value| value.as_bytes() == TEST_ORG.as_bytes())
        .ok_or_else(|| Status::unauthenticated("missing test principal"))?;
    request.extensions_mut().insert(AuthContext {
        user_id: Some("test-user".into()),
        org_id: TEST_ORG.into(),
        auth_method: AuthMethod::Jwt,
        scopes: vec!["org:data:write_all".into()],
        zdr: false,
        acl: EffectiveAcl::allow_all(),
        request_id: "grpc-integration-test".into(),
        verified_bearer: None,
    });
    Ok(request)
}

fn authed<T>(inner: T) -> Request<T> {
    let mut request = Request::new(inner);
    request.metadata_mut().insert(
        "x-test-org",
        MetadataValue::try_from(TEST_ORG).expect("valid test org metadata"),
    );
    request
}

// ─── Document Service Tests ────────────────────────────────────────

#[tokio::test]
async fn test_deprecated_mutations_are_contained_without_a_database() {
    std::env::set_var("DPV2_ALLOW_GRPC_DOCUMENT_WRITES", "1");
    let pool = PgPool::connect_lazy("postgresql://unused:unused@127.0.0.1:1/unused")
        .expect("valid lazy test pool configuration");
    pool.close().await;

    let (addr, _pool) = start_server(pool).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    let create = pb_documents::CreateDocumentRequest {
        org_id: TEST_ORG.into(),
        source: "offline-regression".into(),
        r#type: "text".into(),
        title: "non-sensitive fixture".into(),
        content: "non-sensitive fixture".into(),
        zdr_classification: "restricted".into(),
        metadata: None,
        ingest_policy: Some(pb_documents::IngestPolicy {
            zdr_mode: "ephemeral".into(),
            index_schedule: "disabled".into(),
            ephemeral_only: true,
        }),
    };

    let create_error = client
        .create_document(authed(create.clone()))
        .await
        .expect_err("deprecated create must fail before database access");
    assert_eq!(create_error.code(), tonic::Code::FailedPrecondition);

    let delete_error = client
        .delete_document(authed(pb_documents::DeleteDocumentRequest {
            document_id: "fixture-document".into(),
            org_id: TEST_ORG.into(),
        }))
        .await
        .expect_err("deprecated delete must fail before database access");
    assert_eq!(delete_error.code(), tonic::Code::FailedPrecondition);

    let bulk_error = client
        .bulk_ingest(authed(pb_documents::BulkIngestRequest {
            org_id: TEST_ORG.into(),
            documents: vec![create],
            ingest_policy: Some(pb_documents::IngestPolicy {
                zdr_mode: "ephemeral".into(),
                index_schedule: "disabled".into(),
                ephemeral_only: true,
            }),
        }))
        .await
        .expect_err("deprecated bulk ingest must fail before database access");
    assert_eq!(bulk_error.code(), tonic::Code::FailedPrecondition);
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn test_create_containment_and_get_seeded_document() {
    let url = test_db_url().expect("TEST_DATABASE_URL is required for this ignored test");
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let (addr, pool) = start_server(pool).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    let create_error = client
        .create_document(authed(pb_documents::CreateDocumentRequest {
            org_id: TEST_ORG.into(),
            source: "test-source".into(),
            r#type: "article".into(),
            title: "Integration Test Doc".into(),
            content: "This is test content for integration testing.".into(),
            zdr_classification: "internal".into(),
            metadata: None,
            ingest_policy: None,
        }))
        .await
        .expect_err("legacy gRPC create must remain disabled");
    assert_eq!(create_error.code(), tonic::Code::FailedPrecondition);

    let document_id = insert_test_document(
        &pool,
        TEST_ORG,
        "Integration Test Doc",
        "This is test content for integration testing.",
        "internal",
    )
    .await;

    let get_resp = client
        .get_document(authed(pb_documents::GetDocumentRequest {
            document_id: document_id.clone(),
            org_id: TEST_ORG.into(),
        }))
        .await
        .expect("get document");

    let fetched = get_resp.into_inner().document.expect("document");
    assert_eq!(fetched.document_id, document_id);
    assert_eq!(
        fetched.content,
        "This is test content for integration testing."
    );

    cleanup(&pool).await;
}

#[tokio::test]
async fn test_list_documents() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let (addr, pool) = start_server(pool).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    for i in 0..3 {
        insert_test_document(
            &pool,
            TEST_ORG,
            &format!("Doc {i}"),
            &format!("Content {i}"),
            "internal",
        )
        .await;
    }

    let list_resp = client
        .list_documents(authed(pb_documents::ListDocumentsRequest {
            org_id: TEST_ORG.into(),
            limit: 10,
            offset: 0,
            r#type: String::new(),
        }))
        .await
        .unwrap();

    let inner = list_resp.into_inner();
    assert_eq!(inner.documents.len(), 3);
    assert_eq!(inner.total, 3);

    let filtered = client
        .list_documents(authed(pb_documents::ListDocumentsRequest {
            org_id: TEST_ORG.into(),
            limit: 10,
            offset: 0,
            r#type: "nonexistent".into(),
        }))
        .await
        .unwrap();

    assert_eq!(filtered.into_inner().documents.len(), 0);

    cleanup(&pool).await;
}

#[tokio::test]
async fn test_delete_document_is_permanently_disabled() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let (addr, pool) = start_server(pool).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    let document_id = insert_test_document(&pool, TEST_ORG, "to-delete", "gone", "internal").await;

    let delete_error = client
        .delete_document(authed(pb_documents::DeleteDocumentRequest {
            document_id: document_id.clone(),
            org_id: TEST_ORG.into(),
        }))
        .await
        .expect_err("legacy gRPC delete must remain disabled");
    assert_eq!(delete_error.code(), tonic::Code::FailedPrecondition);

    let still_present = client
        .get_document(authed(pb_documents::GetDocumentRequest {
            document_id,
            org_id: TEST_ORG.into(),
        }))
        .await
        .expect("disabled delete must not mutate the fixture");
    assert!(still_present.into_inner().document.is_some());

    cleanup(&pool).await;
}

#[tokio::test]
async fn test_bulk_ingest_is_permanently_disabled() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let (addr, pool) = start_server(pool).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    let docs: Vec<pb_documents::CreateDocumentRequest> = (0..5)
        .map(|i| pb_documents::CreateDocumentRequest {
            org_id: String::new(),
            source: "bulk".into(),
            r#type: "faq".into(),
            title: format!("Bulk doc {i}"),
            content: format!("Bulk content {i}"),
            zdr_classification: String::new(),
            metadata: None,
            ingest_policy: None,
        })
        .collect();

    let error = client
        .bulk_ingest(authed(pb_documents::BulkIngestRequest {
            org_id: TEST_ORG.into(),
            documents: docs,
            ingest_policy: None,
        }))
        .await
        .expect_err("legacy gRPC bulk ingest must remain disabled");
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);

    let persisted: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM documents WHERE org_id = $1")
        .bind(TEST_ORG)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(persisted.0, 0, "disabled bulk ingest must not persist rows");

    cleanup(&pool).await;
}

#[tokio::test]
async fn test_document_index_status() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let (addr, pool) = start_server(pool).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    let document_id =
        insert_test_document(&pool, TEST_ORG, "idx-test", "content", "internal").await;

    sqlx::query(
        "INSERT INTO knowledge_units (document_id, org_id, chunk_index, text, content_hash, embedding_status)
         VALUES ($1, $2, 0, 'chunk 0', 'hash0', 'completed'),
                ($1, $2, 1, 'chunk 1', 'hash1', 'completed'),
                ($1, $2, 2, 'chunk 2', 'hash2', 'failed')"
    )
    .bind(&document_id)
    .bind(TEST_ORG)
    .execute(&pool)
    .await
    .unwrap();

    let status = client
        .get_document_index_status(authed(pb_documents::GetDocumentIndexStatusRequest {
            document_id,
            org_id: TEST_ORG.into(),
        }))
        .await
        .unwrap()
        .into_inner()
        .status
        .unwrap();

    assert_eq!(status.chunk_count, 3);
    assert_eq!(status.chunk_status, "chunked");
    assert_eq!(status.embed_status, "failed");
    assert_eq!(status.embeddings_synced, 2);

    cleanup(&pool).await;
}

// ─── Auth Tests ────────────────────────────────────────────────────

#[tokio::test]
async fn test_missing_api_key_rejected() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;

    let (addr, _pool) = start_server(pool).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    let err = client
        .get_ingest_status(Request::new(pb_documents::IngestStatusRequest {
            org_id: TEST_ORG.into(),
        }))
        .await
        .unwrap_err();

    assert_eq!(err.code(), tonic::Code::Unauthenticated);
}

#[tokio::test]
async fn test_wrong_api_key_rejected() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;

    let (addr, _pool) = start_server(pool).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    let mut req = Request::new(pb_documents::IngestStatusRequest {
        org_id: TEST_ORG.into(),
    });
    req.metadata_mut()
        .insert("x-api-key", MetadataValue::try_from("wrong-key").unwrap());

    let err = client.get_ingest_status(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
}

// ─── Knowledge Service Tests ───────────────────────────────────────

#[tokio::test]
async fn test_check_permissions_allowed() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let (addr, pool) = start_server(pool).await;
    let ch = connect(addr).await;

    let mut know_client = KnowledgeServiceClient::new(ch);

    let document_id =
        insert_test_document(&pool, TEST_ORG, "internal doc", "safe", "internal").await;

    let resp = know_client
        .check_permissions(authed(pb_knowledge::CheckPermissionsRequest {
            org_id: TEST_ORG.into(),
            document_id,
            user_id: "user-1".into(),
        }))
        .await
        .unwrap()
        .into_inner();

    assert!(resp.allowed);
    assert!(resp.reason.is_empty());

    cleanup(&pool).await;
}

#[tokio::test]
async fn test_check_permissions_restricted() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let doc_id: (String,) = sqlx::query_as(
        "INSERT INTO documents (org_id, source, type, title, content, zdr_classification)
         VALUES ($1, 's', 't', 'restricted doc', 'secret', 'restricted') RETURNING document_id",
    )
    .bind(TEST_ORG)
    .fetch_one(&pool)
    .await
    .unwrap();

    let (addr, pool) = start_server(pool).await;
    let ch = connect(addr).await;
    let mut know_client = KnowledgeServiceClient::new(ch);

    let resp = know_client
        .check_permissions(authed(pb_knowledge::CheckPermissionsRequest {
            org_id: TEST_ORG.into(),
            document_id: doc_id.0,
            user_id: "user-1".into(),
        }))
        .await
        .unwrap()
        .into_inner();

    assert!(!resp.allowed);
    assert!(resp.reason.contains("restricted"));

    cleanup(&pool).await;
}

#[tokio::test]
async fn test_get_knowledge_units() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let (addr, pool) = start_server(pool).await;
    let ch = connect(addr).await;

    let mut know_client = KnowledgeServiceClient::new(ch);

    let document_id =
        insert_test_document(&pool, TEST_ORG, "chunked doc", "full content", "internal").await;

    for i in 0..3 {
        sqlx::query(
            "INSERT INTO knowledge_units (document_id, org_id, chunk_index, text, content_hash, embedding_status)
             VALUES ($1, $2, $3, $4, $5, 'completed')"
        )
        .bind(&document_id)
        .bind(TEST_ORG)
        .bind(i)
        .bind(format!("chunk text {i}"))
        .bind(format!("hash-{i}"))
        .execute(&pool)
        .await
        .unwrap();
    }

    let resp = know_client
        .get_knowledge_units(authed(pb_knowledge::GetKnowledgeUnitsRequest {
            document_id,
            org_id: TEST_ORG.into(),
        }))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(resp.units.len(), 3);
    assert_eq!(resp.units[0].chunk_index, 0);
    assert_eq!(resp.units[1].text, "chunk text 1");
    assert_eq!(resp.units[2].embedding_status, "completed");

    cleanup(&pool).await;
}
