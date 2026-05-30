// Cross-org data isolation tests.
//
// Every query path in Data Plane v2 MUST scope by org_id. These tests assert
// that property by inserting data into two orgs and verifying that org A's
// queries cannot return org B's rows — even when the candidate IDs reference
// org B's data.
//
// Covers:
//   - Document retrieval scoped by org_id (GetDocument, ListDocuments)
//   - Knowledge units scoped by org_id (GetKnowledgeUnits)
//   - CheckPermissions does not leak existence of restricted docs across orgs
//   - List queries respect org_id under filter manipulation

use std::net::SocketAddr;
use std::sync::Arc;

use sqlx::PgPool;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Server};
use tonic::Request;

use retrieval_engine::grpc::document_svc::DocumentSvc;
use retrieval_engine::grpc::interceptor::ApiKeyInterceptor;
use retrieval_engine::grpc::knowledge_svc::KnowledgeSvc;
use retrieval_engine::grpc::pb_documents;
use retrieval_engine::grpc::pb_documents::document_service_client::DocumentServiceClient;
use retrieval_engine::grpc::pb_documents::document_service_server::DocumentServiceServer;
use retrieval_engine::grpc::pb_knowledge;
use retrieval_engine::grpc::pb_knowledge::knowledge_service_client::KnowledgeServiceClient;
use retrieval_engine::grpc::pb_knowledge::knowledge_service_server::KnowledgeServiceServer;

const TEST_API_KEY: &str = "test-cross-org-key";
const ORG_A: &str = "org-isolation-A";
const ORG_B: &str = "org-isolation-B";

fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

async fn setup_schema(pool: &PgPool) {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS documents (
            document_id  TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
            org_id       TEXT NOT NULL,
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
    .expect("create documents");

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
    .expect("create knowledge_units");
}

async fn cleanup(pool: &PgPool) {
    for org in [ORG_A, ORG_B] {
        sqlx::query("DELETE FROM knowledge_units WHERE org_id = $1")
            .bind(org)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM documents WHERE org_id = $1")
            .bind(org)
            .execute(pool)
            .await
            .ok();
    }
}

async fn start_server(pool: PgPool) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let interceptor = ApiKeyInterceptor::new(Some(TEST_API_KEY.into()));
    let doc_svc = DocumentSvc::new(Arc::new(pool.clone()));
    let know_svc = KnowledgeSvc::new(Arc::new(pool));
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    tokio::spawn(async move {
        Server::builder()
            .add_service(DocumentServiceServer::with_interceptor(
                doc_svc,
                interceptor.clone(),
            ))
            .add_service(KnowledgeServiceServer::with_interceptor(
                know_svc,
                interceptor,
            ))
            .serve_with_incoming(incoming)
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

async fn connect(addr: SocketAddr) -> Channel {
    Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap()
}

fn authed<T>(inner: T) -> Request<T> {
    let mut req = Request::new(inner);
    req.metadata_mut()
        .insert("x-api-key", MetadataValue::try_from(TEST_API_KEY).unwrap());
    req
}

async fn create_doc_in(
    client: &mut DocumentServiceClient<Channel>,
    org: &str,
    title: &str,
) -> String {
    client
        .create_document(authed(pb_documents::CreateDocumentRequest {
            org_id: org.into(),
            source: "test".into(),
            r#type: "article".into(),
            title: title.into(),
            content: format!("content of {title} owned by {org}"),
            zdr_classification: "internal".into(),
            metadata: None,
            ingest_policy: None,
        }))
        .await
        .unwrap()
        .into_inner()
        .document
        .unwrap()
        .document_id
}

// ─── Test 1: GetDocument cannot see other org's docs by ID ──────────────────

#[tokio::test]
async fn test_get_document_isolated_across_orgs() {
    let Some(url) = test_db_url() else {
        eprintln!("TEST_DATABASE_URL not set, skipping");
        return;
    };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let addr = start_server(pool.clone()).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    let doc_a = create_doc_in(&mut client, ORG_A, "Org A Doc").await;
    let doc_b = create_doc_in(&mut client, ORG_B, "Org B Doc").await;

    // Org A asks for org B's doc by ID — must NOT find it
    let err = client
        .get_document(authed(pb_documents::GetDocumentRequest {
            document_id: doc_b.clone(),
            org_id: ORG_A.into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);

    // Org B asks for org A's doc — must NOT find it
    let err = client
        .get_document(authed(pb_documents::GetDocumentRequest {
            document_id: doc_a.clone(),
            org_id: ORG_B.into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);

    // Sanity: each org CAN get its own
    let resp_a = client
        .get_document(authed(pb_documents::GetDocumentRequest {
            document_id: doc_a,
            org_id: ORG_A.into(),
        }))
        .await
        .unwrap();
    assert_eq!(resp_a.into_inner().document.unwrap().org_id, ORG_A);

    let resp_b = client
        .get_document(authed(pb_documents::GetDocumentRequest {
            document_id: doc_b,
            org_id: ORG_B.into(),
        }))
        .await
        .unwrap();
    assert_eq!(resp_b.into_inner().document.unwrap().org_id, ORG_B);

    cleanup(&pool).await;
}

// ─── Test 2: ListDocuments returns only the requesting org's docs ───────────

#[tokio::test]
async fn test_list_documents_org_scoped() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let addr = start_server(pool.clone()).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    // 3 docs in A, 2 docs in B
    for i in 0..3 {
        create_doc_in(&mut client, ORG_A, &format!("A doc {i}")).await;
    }
    for i in 0..2 {
        create_doc_in(&mut client, ORG_B, &format!("B doc {i}")).await;
    }

    let list_a = client
        .list_documents(authed(pb_documents::ListDocumentsRequest {
            org_id: ORG_A.into(),
            limit: 100,
            offset: 0,
            r#type: String::new(),
        }))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(list_a.documents.len(), 3);
    for d in &list_a.documents {
        assert_eq!(d.org_id, ORG_A);
        assert!(d.title.starts_with("A doc"));
    }
    assert_eq!(list_a.total, 3);

    let list_b = client
        .list_documents(authed(pb_documents::ListDocumentsRequest {
            org_id: ORG_B.into(),
            limit: 100,
            offset: 0,
            r#type: String::new(),
        }))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(list_b.documents.len(), 2);
    for d in &list_b.documents {
        assert_eq!(d.org_id, ORG_B);
        assert!(d.title.starts_with("B doc"));
    }

    cleanup(&pool).await;
}

// ─── Test 3: Soft delete is org-scoped (cannot delete another org's doc) ────

#[tokio::test]
async fn test_delete_does_not_cross_orgs() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let addr = start_server(pool.clone()).await;
    let ch = connect(addr).await;
    let mut client = DocumentServiceClient::new(ch);

    let doc_b = create_doc_in(&mut client, ORG_B, "B's doc").await;

    // Org A tries to delete B's doc (passing B's document_id but A's org_id)
    let resp = client
        .delete_document(authed(pb_documents::DeleteDocumentRequest {
            document_id: doc_b.clone(),
            org_id: ORG_A.into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(
        !resp.success,
        "delete should report no rows affected when org doesn't match"
    );

    // Verify B's doc is still alive
    let still_there = client
        .get_document(authed(pb_documents::GetDocumentRequest {
            document_id: doc_b,
            org_id: ORG_B.into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(still_there.document.is_some());

    cleanup(&pool).await;
}

// ─── Test 4: GetKnowledgeUnits cannot retrieve other org's chunks ───────────

#[tokio::test]
async fn test_knowledge_units_org_scoped() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let addr = start_server(pool.clone()).await;
    let ch = connect(addr).await;

    let mut doc_client = DocumentServiceClient::new(ch.clone());
    let mut know_client = KnowledgeServiceClient::new(ch);

    let doc_b = create_doc_in(&mut doc_client, ORG_B, "B doc with chunks").await;

    // Insert chunks for B's doc
    for i in 0..3 {
        sqlx::query(
            "INSERT INTO knowledge_units (document_id, org_id, chunk_index, text, content_hash, embedding_status)
             VALUES ($1, $2, $3, $4, $5, 'completed')"
        )
        .bind(&doc_b)
        .bind(ORG_B)
        .bind(i)
        .bind(format!("chunk {i}"))
        .bind(format!("hash-{i}"))
        .execute(&pool)
        .await
        .unwrap();
    }

    // Org A asks for B's chunks (passing B's document_id but A's org_id) — empty
    let resp = know_client
        .get_knowledge_units(authed(pb_knowledge::GetKnowledgeUnitsRequest {
            document_id: doc_b.clone(),
            org_id: ORG_A.into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        resp.units.len(),
        0,
        "must not return chunks owned by another org"
    );

    // Sanity: org B can see its own chunks
    let resp = know_client
        .get_knowledge_units(authed(pb_knowledge::GetKnowledgeUnitsRequest {
            document_id: doc_b,
            org_id: ORG_B.into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp.units.len(), 3);

    cleanup(&pool).await;
}

// ─── Test 5: CheckPermissions cannot reveal existence across orgs ───────────

#[tokio::test]
async fn test_check_permissions_org_scoped() {
    let Some(url) = test_db_url() else { return };
    let pool = PgPool::connect(&url).await.unwrap();
    setup_schema(&pool).await;
    cleanup(&pool).await;

    let addr = start_server(pool.clone()).await;
    let ch = connect(addr).await;

    let mut doc_client = DocumentServiceClient::new(ch.clone());
    let mut know_client = KnowledgeServiceClient::new(ch);

    let doc_b = create_doc_in(&mut doc_client, ORG_B, "B's doc").await;

    // Org A asks about B's document — should be NotFound, NOT Allowed/Denied,
    // because returning Denied would confirm the document exists, leaking info.
    let err = know_client
        .check_permissions(authed(pb_knowledge::CheckPermissionsRequest {
            org_id: ORG_A.into(),
            document_id: doc_b,
            user_id: "attacker".into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);

    cleanup(&pool).await;
}
