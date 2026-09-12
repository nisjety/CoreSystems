//! Data Plane v2 document promotion client — S3.3 step 5 (design doc §5,
//! §8 item 5). Distinct from step 4's `PromoteWorkspace` merge (§4): that
//! merges a run's CAS-backed workspace overlay into its Space, restart-durable
//! COMPUTE state; this promotes SPECIFIC workspace paths into durable,
//! RAG-searchable KNOWLEDGE (a Data Plane v2 document) — never every
//! workspace file (a build artifact belongs in the Space's CAS-backed
//! workspace, not as a document), and never an automatic side effect of the
//! merge. "A run or its caller" takes this action explicitly, per path.
//!
//! Mirrors model-gateway's own already-shipped
//! `dataplane.rs::create_document`/`bulk_ingest` client pattern directly —
//! the sanctioned path per `apps/CODEBASE_INFORMATION_SYSTEM.md`'s "Model
//! Plane calls Data Plane v2's API directly" rule — rather than inventing a
//! new one: same `CreateDocumentRequest` shape, same ZDR-gate shape, same
//! delegated-bearer forwarding this crate already uses for
//! [`DelegatedDataPlaneBearer`] (`auth::authenticate_delegated_data_plane`).
//!
//! **Not yet wired to a real caller**, matching this whole initiative's own
//! established "ship the primitive, wire the caller once a concrete
//! consumer exists" precedent (the CAS client, the sandbox lease RPCs, and
//! `PromoteWorkspace` were all in this exact position before their own
//! first real caller landed).

use mp_contracts::dataplane::documents_v2::{
    document_service_client::DocumentServiceClient, BulkIngestRequest, BulkIngestResponse,
    CreateDocumentRequest, CreateDocumentResponse,
};
use tonic::{transport::Channel, Request, Status};

use crate::auth::DelegatedDataPlaneBearer;

const RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Mirrors `model-gateway/src/dataplane.rs`'s own `document_zdr_requested`:
/// the document-side half of the ZDR check. A durable mutation is also
/// blocked when the CALLER's own verified retention posture is ZDR — that
/// half is `caller_zdr`, checked alongside this one in
/// [`reject_zdr_durable_mutation`], never a substitute for it.
fn document_zdr_requested(zdr_classification: &str) -> bool {
    matches!(
        zdr_classification.trim().to_ascii_lowercase().as_str(),
        "on" | "true" | "zdr" | "ephemeral" | "ephemeral-only" | "zero-retention"
    )
}

/// Rejects a promotion that would durably mutate Data Plane v2 state under
/// Zero Data Retention — checked BEFORE any network call, never downstream
/// of one. Either signal alone is enough; neither can downgrade the other.
///
/// # Errors
/// `failed_precondition` when `caller_zdr` or `zdr_classification` requests
/// a ZDR-shaped retention posture.
fn reject_zdr_durable_mutation(caller_zdr: bool, zdr_classification: &str) -> Result<(), Status> {
    if caller_zdr || document_zdr_requested(zdr_classification) {
        return Err(Status::failed_precondition(
            "Data Plane document promotion is unavailable under ZDR",
        ));
    }
    Ok(())
}

/// One workspace path a caller wants promoted into durable, RAG-searchable
/// knowledge — deliberately NOT every workspace file; this is an explicit,
/// per-path action (design doc §5), never an automatic side effect of
/// `PromoteWorkspace`'s own compute-state merge (§4).
pub struct PromotedDocument<'a> {
    pub source: &'a str,
    pub doc_type: &'a str,
    pub title: &'a str,
    pub content: &'a str,
    /// Empty means "not ZDR" — the same sentinel `zdr_classification`
    /// already uses on the wire everywhere else in this codebase.
    pub zdr_classification: &'a str,
}

#[derive(Clone)]
pub struct DataPlaneDocumentClient {
    channel: Channel,
}

impl DataPlaneDocumentClient {
    #[must_use]
    pub fn new(channel: Channel) -> Self {
        Self { channel }
    }

    /// Builds from `DATAPLANE_RETRIEVAL_URL`/`_ADDR` — the SAME address
    /// execution-core's own deployment config already resolves
    /// `DocumentService` through (`docker-compose.yml`'s `execution-core`
    /// block already sets exactly this), since `RetrievalService`/
    /// `DocumentService`/`KnowledgeService` are one physical Data Plane v2
    /// endpoint multiplexed over gRPC — the same variable model-gateway's
    /// own `document_client` construction already reads, for the identical
    /// reason. Connects lazily: a down or misconfigured Data Plane only
    /// fails the first real RPC, never startup.
    ///
    /// # Errors
    /// Returns an error only when the resolved URL is not a valid endpoint.
    pub fn from_env() -> Result<Self, String> {
        let url = std::env::var("DATAPLANE_RETRIEVAL_URL")
            .or_else(|_| std::env::var("DATAPLANE_RETRIEVAL_ADDR"))
            .unwrap_or_else(|_| "http://retrieval-engine:50052".to_owned());
        let channel = tonic::transport::Endpoint::from_shared(url)
            .map_err(|error| format!("Data Plane document endpoint is invalid: {error}"))?
            .connect_lazy();
        Ok(Self::new(channel))
    }

    fn client(&self) -> DocumentServiceClient<Channel> {
        DocumentServiceClient::new(self.channel.clone())
    }

    fn authorize<T>(bearer: &DelegatedDataPlaneBearer, message: T) -> Result<Request<T>, Status> {
        let mut request = Request::new(message);
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", bearer.as_str())
                .parse()
                .map_err(|_| Status::internal("Data Plane bearer is not forwardable"))?,
        );
        Ok(request)
    }

    /// Promotes one workspace path as a durable Data Plane v2 document.
    ///
    /// # Errors
    /// `failed_precondition` when ZDR blocks durable mutation (checked
    /// before any network call); `invalid_argument` for a blank
    /// `title`/`content`; `deadline_exceeded` past [`RPC_TIMEOUT`];
    /// otherwise Data Plane v2's own status.
    pub async fn create_document(
        &self,
        bearer: &DelegatedDataPlaneBearer,
        org_id: &str,
        caller_zdr: bool,
        doc: &PromotedDocument<'_>,
    ) -> Result<CreateDocumentResponse, Status> {
        reject_zdr_durable_mutation(caller_zdr, doc.zdr_classification)?;
        if doc.title.trim().is_empty() || doc.content.trim().is_empty() {
            return Err(Status::invalid_argument("title and content are required"));
        }
        let message = CreateDocumentRequest {
            org_id: org_id.to_owned(),
            source: doc.source.to_owned(),
            r#type: doc.doc_type.to_owned(),
            title: doc.title.to_owned(),
            content: doc.content.to_owned(),
            metadata: None,
            zdr_classification: doc.zdr_classification.to_owned(),
            ingest_policy: None,
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().create_document(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("Data Plane CreateDocument timed out"))?;
        Ok(outcome?.into_inner())
    }

    /// Promotes multiple workspace paths as durable Data Plane v2 documents
    /// in one call.
    ///
    /// # Errors
    /// `failed_precondition` when ANY document in the batch would require a
    /// durable mutation under ZDR (checked before any network call, for the
    /// whole batch — never partially forwarded); `invalid_argument` when
    /// `docs` is empty; `deadline_exceeded` past [`RPC_TIMEOUT`]; otherwise
    /// Data Plane v2's own status.
    pub async fn bulk_ingest(
        &self,
        bearer: &DelegatedDataPlaneBearer,
        org_id: &str,
        caller_zdr: bool,
        docs: &[PromotedDocument<'_>],
    ) -> Result<BulkIngestResponse, Status> {
        if docs.is_empty() {
            return Err(Status::invalid_argument(
                "at least one document is required",
            ));
        }
        for doc in docs {
            reject_zdr_durable_mutation(caller_zdr, doc.zdr_classification)?;
        }
        let documents = docs
            .iter()
            .map(|doc| CreateDocumentRequest {
                org_id: org_id.to_owned(),
                source: doc.source.to_owned(),
                r#type: doc.doc_type.to_owned(),
                title: doc.title.to_owned(),
                content: doc.content.to_owned(),
                metadata: None,
                zdr_classification: doc.zdr_classification.to_owned(),
                ingest_policy: None,
            })
            .collect();
        let message = BulkIngestRequest {
            org_id: org_id.to_owned(),
            documents,
            ingest_policy: None,
        };
        let wire = Self::authorize(bearer, message)?;
        let outcome = tokio::time::timeout(RPC_TIMEOUT, self.client().bulk_ingest(wire))
            .await
            .map_err(|_| Status::deadline_exceeded("Data Plane BulkIngest timed out"))?;
        Ok(outcome?.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> DataPlaneDocumentClient {
        DataPlaneDocumentClient::new(
            tonic::transport::Endpoint::from_shared("http://127.0.0.1:1")
                .expect("valid endpoint")
                .connect_lazy(),
        )
    }

    fn doc<'a>(
        title: &'a str,
        content: &'a str,
        zdr_classification: &'a str,
    ) -> PromotedDocument<'a> {
        PromotedDocument {
            source: "code_interpreter",
            doc_type: "workspace_artifact",
            title,
            content,
            zdr_classification,
        }
    }

    #[test]
    fn document_zdr_requested_matches_every_restrictive_shape() {
        for value in [
            "on",
            "true",
            "zdr",
            "ephemeral",
            "ephemeral-only",
            "zero-retention",
        ] {
            assert!(document_zdr_requested(value), "{value}");
        }
        assert!(!document_zdr_requested(""));
        assert!(!document_zdr_requested("internal"));
    }

    #[test]
    fn reject_zdr_durable_mutation_checks_both_caller_and_document() {
        assert!(reject_zdr_durable_mutation(true, "").is_err());
        assert!(reject_zdr_durable_mutation(false, "zdr").is_err());
        assert!(reject_zdr_durable_mutation(false, "").is_ok());
    }

    #[tokio::test]
    async fn create_document_rejects_zdr_before_any_network_call() {
        let client = test_client();
        let error = client
            .create_document(
                &DelegatedDataPlaneBearer::for_test(),
                "org-a",
                true,
                &doc("title", "content", ""),
            )
            .await
            .expect_err("caller ZDR must fail closed before any RPC");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn create_document_rejects_a_zdr_classified_document_even_for_a_non_zdr_caller() {
        let client = test_client();
        let error = client
            .create_document(
                &DelegatedDataPlaneBearer::for_test(),
                "org-a",
                false,
                &doc("title", "content", "ephemeral"),
            )
            .await
            .expect_err("a ZDR-classified document must fail closed before any RPC");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn create_document_rejects_blank_title_or_content() {
        let client = test_client();
        assert_eq!(
            client
                .create_document(
                    &DelegatedDataPlaneBearer::for_test(),
                    "org-a",
                    false,
                    &doc("", "content", ""),
                )
                .await
                .expect_err("blank title must fail closed")
                .code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            client
                .create_document(
                    &DelegatedDataPlaneBearer::for_test(),
                    "org-a",
                    false,
                    &doc("title", "", ""),
                )
                .await
                .expect_err("blank content must fail closed")
                .code(),
            tonic::Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn bulk_ingest_rejects_an_empty_batch() {
        let client = test_client();
        let error = client
            .bulk_ingest(&DelegatedDataPlaneBearer::for_test(), "org-a", false, &[])
            .await
            .expect_err("an empty batch must fail closed before any RPC");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn bulk_ingest_rejects_the_whole_batch_if_any_document_is_zdr() {
        let client = test_client();
        let docs = [doc("a", "content-a", ""), doc("b", "content-b", "zdr")];
        let error = client
            .bulk_ingest(&DelegatedDataPlaneBearer::for_test(), "org-a", false, &docs)
            .await
            .expect_err("one ZDR document in the batch must reject the whole batch");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn bulk_ingest_rejects_the_whole_batch_when_the_caller_is_zdr() {
        let client = test_client();
        let docs = [doc("a", "content-a", "")];
        let error = client
            .bulk_ingest(&DelegatedDataPlaneBearer::for_test(), "org-a", true, &docs)
            .await
            .expect_err("caller ZDR must fail closed before any RPC");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    }

    // The tests above prove rejection never reaches the network (an
    // unroutable endpoint would hang/fail loudly if it were). This one
    // proves the OTHER half: a genuinely non-ZDR call really does reach a
    // real server, forwards the bearer as an authorization header, and
    // returns its response -- mirroring `dataplane.rs`'s own
    // `issuer_zdr_document_delete_is_rejected_before_data_plane_forwarding`
    // test's real-server shape for the success path.
    mod wire_round_trip {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        };

        use mp_contracts::dataplane::documents_v2::{
            document_service_server::{DocumentService, DocumentServiceServer},
            Document, DeleteDocumentRequest, DeleteDocumentResponse, GetDocumentIndexStatusRequest,
            GetDocumentIndexStatusResponse, GetDocumentRequest, GetDocumentResponse,
            IngestStatusRequest, IngestStatusResponse, ListDocumentsRequest, ListDocumentsResponse,
        };
        use tonic::{Response, Status as TonicStatus};

        use super::*;

        #[derive(Clone, Default)]
        struct RecordingDocumentService {
            create_calls: Arc<AtomicUsize>,
            last_authorization: Arc<Mutex<Option<String>>>,
            last_request: Arc<Mutex<Option<CreateDocumentRequest>>>,
        }

        #[tonic::async_trait]
        impl DocumentService for RecordingDocumentService {
            async fn get_document(
                &self,
                _request: tonic::Request<GetDocumentRequest>,
            ) -> Result<Response<GetDocumentResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
            async fn list_documents(
                &self,
                _request: tonic::Request<ListDocumentsRequest>,
            ) -> Result<Response<ListDocumentsResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
            async fn create_document(
                &self,
                request: tonic::Request<CreateDocumentRequest>,
            ) -> Result<Response<CreateDocumentResponse>, TonicStatus> {
                self.create_calls.fetch_add(1, Ordering::SeqCst);
                *self.last_authorization.lock().unwrap() = request
                    .metadata()
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let req = request.into_inner();
                *self.last_request.lock().unwrap() = Some(req.clone());
                Ok(Response::new(CreateDocumentResponse {
                    document: Some(Document {
                        document_id: "doc-1".to_owned(),
                        org_id: req.org_id,
                        source: req.source,
                        r#type: req.r#type,
                        title: req.title,
                        content: req.content,
                        status: "created".to_owned(),
                        metadata: None,
                        created_at: None,
                        updated_at: None,
                        deleted_at: None,
                        zdr_classification: req.zdr_classification,
                        zdr_reason: None,
                    }),
                }))
            }
            async fn delete_document(
                &self,
                _request: tonic::Request<DeleteDocumentRequest>,
            ) -> Result<Response<DeleteDocumentResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
            async fn bulk_ingest(
                &self,
                _request: tonic::Request<BulkIngestRequest>,
            ) -> Result<Response<BulkIngestResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
            async fn get_document_index_status(
                &self,
                _request: tonic::Request<GetDocumentIndexStatusRequest>,
            ) -> Result<Response<GetDocumentIndexStatusResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
            async fn get_ingest_status(
                &self,
                _request: tonic::Request<IngestStatusRequest>,
            ) -> Result<Response<IngestStatusResponse>, TonicStatus> {
                Err(TonicStatus::unimplemented("not used by this test"))
            }
        }

        #[tokio::test]
        async fn create_document_forwards_the_bearer_and_reaches_a_real_server() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind fake Data Plane server");
            let address = listener.local_addr().expect("listener address");
            let service = RecordingDocumentService::default();
            let server_service = service.clone();
            let handle = tokio::spawn(async move {
                let _ = tonic::transport::Server::builder()
                    .add_service(DocumentServiceServer::new(server_service))
                    .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                    .await;
            });

            let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
                .expect("valid endpoint")
                .connect()
                .await
                .expect("connect to fake Data Plane server");
            let client = DataPlaneDocumentClient::new(channel);

            let response = client
                .create_document(
                    &DelegatedDataPlaneBearer::for_test(),
                    "org-a",
                    false,
                    &doc("promoted title", "promoted content", ""),
                )
                .await
                .expect("non-ZDR create_document should reach the real server");

            assert_eq!(service.create_calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                service.last_authorization.lock().unwrap().as_deref(),
                Some("Bearer data-plane-test-bearer")
            );
            let sent = service.last_request.lock().unwrap().clone().expect("request recorded");
            assert_eq!(sent.org_id, "org-a");
            assert_eq!(sent.title, "promoted title");
            assert_eq!(sent.content, "promoted content");
            let document = response.document.expect("document in response");
            assert_eq!(document.document_id, "doc-1");
            assert_eq!(document.title, "promoted title");

            handle.abort();
        }
    }
}
