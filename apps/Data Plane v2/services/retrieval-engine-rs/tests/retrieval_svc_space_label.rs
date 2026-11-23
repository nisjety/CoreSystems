//! W3-FIX-3 (WS17-S9): the Space label on gRPC candidates, end to end.
//!
//! `RetrievalSvc::retrieve` and `retrieve_stream` run the real handler, the real verified-decision path
//! (`apply_space_decision` and `resolve_space_retrieval_scope`) and the real pipeline. Three dependencies are stubbed at
//! the transport, so the real clients still run:
//!
//! * Postgres: `FakePg` answers the Space binding, the room's documents and the live-document gate from a script.
//! * Qdrant: a gRPC `Points` server that answers `search` with fixed points and records each request.
//! * The query embedding: the production `deterministic_test` provider, which the environment enables with the two
//!   isolation flags it requires.
//!
//! The caller's identity is injected the way the JWT interceptor injects it, and the Control decision is signed here
//! with a test key the process trusts. Nothing needs a database, Qdrant or a model plane, so a default `cargo test`
//! runs these.

#[allow(dead_code)]
mod common;

use std::sync::{Arc, Mutex, Once};

use async_trait::async_trait;
use common::fake_pg::{self, FakePg, Reply};
use ed25519_dalek::{Signer as _, SigningKey};
use futures::StreamExt as _;
use qdrant_client::qdrant::{
    self,
    points_server::{Points, PointsServer},
};
use retrieval_engine::authz::{
    AuthContext, AuthMethod, EffectiveAcl, NoopPolicyClient, Principal, VisibilityClient,
};
use retrieval_engine::config::Config;
use retrieval_engine::embed::media::MediaQueryEmbedder;
use retrieval_engine::embed::visual::VisualQueryEmbedder;
use retrieval_engine::embed::EmbeddingClient;
use retrieval_engine::grpc::pb_retrieval::retrieval_service_server::RetrievalService;
use retrieval_engine::grpc::pb_retrieval::{
    retrieval_chunk::Payload, Candidate, RetrievalChunk, RetrieveRequest,
};
use retrieval_engine::grpc::retrieval_svc::RetrievalSvc;
use retrieval_engine::pipeline::orchestrator::RetrievalPipeline;
use retrieval_engine::search::graph_remote::GraphTraverseClient;
use retrieval_engine::search::keyword::MeilisearchQueryClient;
use retrieval_engine::search::sparse::{DynSparseSearchBackend, PostgresSparseBackend};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::{Code, Request, Response, Status};

const ORG: &str = "org-1";
const USER: &str = "user-1";
const SPACE: &str = "space:org-1:room-7";
const KEY_ID: &str = "space-label-e2e";
/// The audience a retrieval decision must name. The crate keeps its copy private, so the literal is repeated here.
const RETRIEVAL_AUDIENCE: &str = "data-plane-retrieval";
const ROOM_DOCUMENTS: [&str; 2] = ["doc-1", "doc-2"];
const EMBEDDING_DIMENSION: &str = "16";

/// The process-wide settings the stack reads: the Control key the decision verifier trusts, and the two isolation
/// flags the deterministic embedding provider requires. Every test in this binary sets the same values, so setting
/// them once is enough, and they are set before any pipeline is built.
fn configure_process() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let public = b64(&signing_key().verifying_key().to_bytes());
        std::env::set_var(
            "CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON",
            format!("{{\"{KEY_ID}\":\"{public}\"}}"),
        );
        std::env::set_var("ALLOW_INSECURE_DEV_DEFAULTS", "1");
        std::env::set_var("ISOLATED_E2E", "1");
    });
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[43; 32])
}

fn b64(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A retrieval decision for `subject` on `space_ref`, signed the way Control signs one.
fn signed_decision(space_ref: &str, subject: &str) -> String {
    let now = chrono::Utc::now();
    let claims = serde_json::json!({
        "decision_ref": "decision-1",
        "org_id": ORG,
        "space_ref": space_ref,
        "subject_id": subject,
        "service_audience": RETRIEVAL_AUDIENCE,
        "recipient_audience_ref": "audience:user-1",
        "recipient_audience_hash": "audience-hash:user-1",
        "recipient_audience_revision": 1,
        "privacy_policy_ref": "privacy:org-1",
        "resource_authorization_ref": "resource:1",
        "authority_revision": 1,
        "permissions": ["retrieval:read"],
        "purpose": "assistant_collaboration",
        "lawful_basis": "contract",
        "privacy_class": "internal",
        "retention_class": "standard",
        "residency": "swedencentral",
        "deletion_scope": "space",
        "nonce": "nonce-1",
        "issued_at": now.to_rfc3339(),
        "expires_at": (now + chrono::Duration::minutes(5)).to_rfc3339(),
    });
    let head = format!(
        "v2.{}.{}",
        b64(KEY_ID.as_bytes()),
        b64(&serde_json::to_vec(&claims).expect("the claims serialize")),
    );
    let signature = signing_key().sign(head.as_bytes());
    format!("{head}.{}", b64(&signature.to_bytes()))
}

/// The decision for `SPACE` with its claims rewritten to `other_space` and its signature kept. The payload is still
/// valid base64url and JSON, so the refusal can only come from the signature no longer covering the claims.
fn decision_rewritten_to(decision: &str, other_space: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let parts: Vec<&str> = decision.split('.').collect();
    assert_eq!(parts.len(), 4, "a v2 decision has four segments");
    let mut claims: serde_json::Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(parts[2])
            .expect("the payload is base64url"),
    )
    .expect("the payload is JSON");
    claims["space_ref"] = serde_json::Value::String(other_space.to_string());
    format!(
        "{}.{}.{}.{}",
        parts[0],
        parts[1],
        b64(&serde_json::to_vec(&claims).expect("the claims serialize")),
        parts[3]
    )
}

/// The room as Postgres would answer it: one active Space binding, two documents imported under the Space, and both
/// live. `FakePg` answers by statement text, so these scripts are what the handler's queries receive.
fn script_room(db: &FakePg) {
    db.answer(
        "FROM space_retrieval_bindings",
        Reply {
            columns: vec![
                ("workspace_id", fake_pg::OID_TEXT),
                ("collection_id", fake_pg::OID_TEXT),
                ("owner_resource_ref", fake_pg::OID_TEXT),
            ],
            rows: vec![vec![fake_pg::text("ws-1"), None, fake_pg::text("owner:1")]],
        },
    );
    // The room's document set, which `resolve_space_retrieval_scope` reads once per decision.
    db.answer(
        "FROM documents WHERE org_id = $1 AND space_ref = $2",
        document_ids(&ROOM_DOCUMENTS),
    );
    // The live-document gate of the post-filter, which keeps a candidate only when Postgres lists it.
    db.answer(
        "AND ($2::text IS NULL OR owner_id = $2 OR visibility = 'org'",
        document_ids(&ROOM_DOCUMENTS),
    );
}

fn document_ids(ids: &[&str]) -> Reply {
    Reply {
        columns: vec![("document_id", fake_pg::OID_TEXT)],
        rows: ids.iter().map(|id| vec![fake_pg::text(id)]).collect(),
    }
}

type Served<T> = Result<Response<T>, Status>;

/// A Qdrant `Points` server. `search` answers the room's points and records every request it receives. The other
/// calls are not made by the retrieval path, so they are refused.
#[derive(Clone)]
struct FakeQdrant {
    searches: Arc<Mutex<Vec<qdrant::SearchPoints>>>,
}

// tonic's `Status` is the error type of the generated `Points` trait, so the signature cannot box it.
#[allow(clippy::result_large_err)]
fn refused<T>(method: &str) -> Served<T> {
    Err(Status::unimplemented(format!(
        "the fake Qdrant does not serve {method}"
    )))
}

fn text_value(value: &str) -> qdrant::Value {
    qdrant::Value {
        kind: Some(qdrant::value::Kind::StringValue(value.to_string())),
    }
}

fn integer_value(value: i64) -> qdrant::Value {
    qdrant::Value {
        kind: Some(qdrant::value::Kind::IntegerValue(value)),
    }
}

/// One scored point per room document, best first, with the payload the dense arm reads.
fn room_points() -> Vec<qdrant::ScoredPoint> {
    ROOM_DOCUMENTS
        .iter()
        .enumerate()
        .map(|(rank, document)| qdrant::ScoredPoint {
            score: 0.9 - 0.1 * rank as f32,
            payload: [
                ("knowledge_id", text_value(&format!("k-{document}"))),
                ("document_id", text_value(document)),
                ("text", text_value(&format!("the text of {document}"))),
                ("chunk_index", integer_value(0)),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
            ..Default::default()
        })
        .collect()
}

#[tonic::async_trait]
impl Points for FakeQdrant {
    async fn search(
        &self,
        request: Request<qdrant::SearchPoints>,
    ) -> Served<qdrant::SearchResponse> {
        self.searches
            .lock()
            .expect("the search log")
            .push(request.into_inner());
        Ok(Response::new(qdrant::SearchResponse {
            result: room_points(),
            ..Default::default()
        }))
    }

    async fn upsert(
        &self,
        _: Request<qdrant::UpsertPoints>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("upsert")
    }
    async fn delete(
        &self,
        _: Request<qdrant::DeletePoints>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("delete")
    }
    async fn get(&self, _: Request<qdrant::GetPoints>) -> Served<qdrant::GetResponse> {
        refused("get")
    }
    async fn update_vectors(
        &self,
        _: Request<qdrant::UpdatePointVectors>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("update_vectors")
    }
    async fn delete_vectors(
        &self,
        _: Request<qdrant::DeletePointVectors>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("delete_vectors")
    }
    async fn set_payload(
        &self,
        _: Request<qdrant::SetPayloadPoints>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("set_payload")
    }
    async fn overwrite_payload(
        &self,
        _: Request<qdrant::SetPayloadPoints>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("overwrite_payload")
    }
    async fn delete_payload(
        &self,
        _: Request<qdrant::DeletePayloadPoints>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("delete_payload")
    }
    async fn clear_payload(
        &self,
        _: Request<qdrant::ClearPayloadPoints>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("clear_payload")
    }
    async fn create_field_index(
        &self,
        _: Request<qdrant::CreateFieldIndexCollection>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("create_field_index")
    }
    async fn delete_field_index(
        &self,
        _: Request<qdrant::DeleteFieldIndexCollection>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("delete_field_index")
    }
    async fn create_vector_name(
        &self,
        _: Request<qdrant::CreateVectorNameRequest>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("create_vector_name")
    }
    async fn delete_vector_name(
        &self,
        _: Request<qdrant::DeleteVectorNameRequest>,
    ) -> Served<qdrant::PointsOperationResponse> {
        refused("delete_vector_name")
    }
    async fn search_batch(
        &self,
        _: Request<qdrant::SearchBatchPoints>,
    ) -> Served<qdrant::SearchBatchResponse> {
        refused("search_batch")
    }
    async fn search_groups(
        &self,
        _: Request<qdrant::SearchPointGroups>,
    ) -> Served<qdrant::SearchGroupsResponse> {
        refused("search_groups")
    }
    async fn scroll(&self, _: Request<qdrant::ScrollPoints>) -> Served<qdrant::ScrollResponse> {
        refused("scroll")
    }
    async fn recommend(
        &self,
        _: Request<qdrant::RecommendPoints>,
    ) -> Served<qdrant::RecommendResponse> {
        refused("recommend")
    }
    async fn recommend_batch(
        &self,
        _: Request<qdrant::RecommendBatchPoints>,
    ) -> Served<qdrant::RecommendBatchResponse> {
        refused("recommend_batch")
    }
    async fn recommend_groups(
        &self,
        _: Request<qdrant::RecommendPointGroups>,
    ) -> Served<qdrant::RecommendGroupsResponse> {
        refused("recommend_groups")
    }
    async fn discover(
        &self,
        _: Request<qdrant::DiscoverPoints>,
    ) -> Served<qdrant::DiscoverResponse> {
        refused("discover")
    }
    async fn discover_batch(
        &self,
        _: Request<qdrant::DiscoverBatchPoints>,
    ) -> Served<qdrant::DiscoverBatchResponse> {
        refused("discover_batch")
    }
    async fn count(&self, _: Request<qdrant::CountPoints>) -> Served<qdrant::CountResponse> {
        refused("count")
    }
    async fn update_batch(
        &self,
        _: Request<qdrant::UpdateBatchPoints>,
    ) -> Served<qdrant::UpdateBatchResponse> {
        refused("update_batch")
    }
    async fn query(&self, _: Request<qdrant::QueryPoints>) -> Served<qdrant::QueryResponse> {
        refused("query")
    }
    async fn query_batch(
        &self,
        _: Request<qdrant::QueryBatchPoints>,
    ) -> Served<qdrant::QueryBatchResponse> {
        refused("query_batch")
    }
    async fn query_groups(
        &self,
        _: Request<qdrant::QueryPointGroups>,
    ) -> Served<qdrant::QueryGroupsResponse> {
        refused("query_groups")
    }
    async fn facet(&self, _: Request<qdrant::FacetCounts>) -> Served<qdrant::FacetResponse> {
        refused("facet")
    }
    async fn search_matrix_pairs(
        &self,
        _: Request<qdrant::SearchMatrixPoints>,
    ) -> Served<qdrant::SearchMatrixPairsResponse> {
        refused("search_matrix_pairs")
    }
    async fn search_matrix_offsets(
        &self,
        _: Request<qdrant::SearchMatrixPoints>,
    ) -> Served<qdrant::SearchMatrixOffsetsResponse> {
        refused("search_matrix_offsets")
    }
}

/// Starts the fake Qdrant on a free local port. Returns its URL and the log of the searches it served.
async fn serve_qdrant() -> (String, Arc<Mutex<Vec<qdrant::SearchPoints>>>) {
    let searches = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free local port");
    let address = listener.local_addr().expect("the listener's address");
    let incoming = Box::pin(futures::stream::unfold(listener, |listener| async move {
        let accepted = listener.accept().await.map(|(socket, _)| socket);
        Some((accepted, listener))
    }));
    let service = FakeQdrant {
        searches: searches.clone(),
    };
    tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(PointsServer::new(service))
            .serve_with_incoming(incoming)
            .await;
    });
    (format!("http://{address}"), searches)
}

/// The production configuration, from a minimal environment. The blend is the dense vertical alone, because the
/// Space path refuses every hybrid arm, and the embedding is the deterministic provider.
fn config(qdrant_url: &str) -> Config {
    let pairs = [
        (
            "DATABASE_URL",
            "postgres://unused:unused@127.0.0.1:1/unused",
        ),
        ("QDRANT_URL", qdrant_url),
        ("EMBEDDING_PROVIDER", "deterministic_test"),
        ("EMBEDDING_DIMENSION", EMBEDDING_DIMENSION),
        ("SMART_HYBRID_ENABLED", "false"),
        ("W_DENSE", "1.0"),
        ("W_BM25", "0.0"),
        ("W_GRAPH", "0.0"),
        ("W_WIKI", "0.0"),
        ("W_VISUAL", "0.0"),
        ("W_KEYWORD", "0.0"),
    ];
    envy::from_iter(pairs.map(|(key, value)| (key.to_string(), value.to_string())))
        .expect("the minimal environment deserializes with the production defaults")
}

/// The viewer holds no explicit document grant. The owner's Space grant attaches one room document to the viewer.
struct RoomVisibility;

#[async_trait]
impl VisibilityClient for RoomVisibility {
    async fn visible_documents(
        &self,
        _org_id: &str,
        _user_id: &str,
        _verified_bearer: Option<&str>,
    ) -> Vec<String> {
        Vec::new()
    }

    async fn visible_space_documents(
        &self,
        _org_id: &str,
        _user_id: &str,
        _space_ref: &str,
        _verified_bearer: Option<&str>,
    ) -> Vec<String> {
        vec!["doc-1".to_string()]
    }
}

/// The pipeline over the stubs: the FakePg pool, the fake Qdrant, the deterministic embedding, and the production
/// policy and visibility seams with their stub answers.
fn pipeline(db: &FakePg, config: Config) -> RetrievalPipeline {
    let pool = db.pool();
    let qdrant = qdrant_client::Qdrant::from_url(&config.qdrant_url)
        .skip_compatibility_check()
        .build()
        .expect("the Qdrant client builds without connecting");
    let embedder =
        EmbeddingClient::from_config(&config).expect("the deterministic embedding provider builds");
    let sparse_backend: DynSparseSearchBackend = Arc::new(PostgresSparseBackend::new(pool.clone()));
    RetrievalPipeline {
        pool,
        qdrant,
        embedder,
        reranker: None,
        cache: None,
        policy: Arc::new(NoopPolicyClient),
        visibility: Arc::new(RoomVisibility),
        nats: None,
        event_signer: None,
        sparse_backend,
        visual_embedder: VisualQueryEmbedder::from_config(&config)
            .expect("the visual embedder config is valid"),
        media_embedder: MediaQueryEmbedder::from_config(&config),
        colqwen: None,
        graph_remote: GraphTraverseClient::from_config(
            &config.graph_index_url,
            config.graph_remote_timeout_ms,
        ),
        keyword_client: MeilisearchQueryClient::from_config(
            &config.meilisearch_url,
            &config.meilisearch_api_key,
            &config.meilisearch_index_uid,
        ),
        config,
    }
}

/// One deployment: the gRPC service over the pipeline, with the stubs behind it.
struct Room {
    service: RetrievalSvc,
    searches: Arc<Mutex<Vec<qdrant::SearchPoints>>>,
    /// Kept alive for the test: dropping it stops the fake database.
    _db: FakePg,
}

impl Room {
    async fn start() -> Self {
        configure_process();
        let db = FakePg::start().await;
        script_room(&db);
        let (qdrant_url, searches) = serve_qdrant().await;
        let pipeline = pipeline(&db, config(&qdrant_url));
        Self {
            service: RetrievalSvc::new(Arc::new(pipeline)),
            searches,
            _db: db,
        }
    }
}

/// The caller the JWT interceptor would have verified: an interactive member of the org.
fn verified_member() -> AuthContext {
    AuthContext {
        user_id: Some(USER.to_string()),
        org_id: ORG.to_string(),
        auth_method: AuthMethod::Jwt,
        scopes: vec!["retrieval:read".to_string()],
        zdr: false,
        sovereign: None,
        acl: EffectiveAcl::default(),
        request_id: "request-1".to_string(),
        verified_bearer: Some("bearer-1".to_string()),
        principal: Principal::User,
    }
}

fn retrieve_request() -> RetrieveRequest {
    RetrieveRequest {
        org_id: ORG.to_string(),
        query: "what does the room say about pump seals".to_string(),
        top_k: 5,
        user_id: Some(USER.to_string()),
        // Set explicitly, because an unset value is the fail-closed default the configured embedding route cannot meet.
        sovereign_required: Some(false),
        ..Default::default()
    }
}

/// A request as the interceptor hands it over: the verified caller in its extensions, and the Space decision in its
/// metadata when the caller asks for a Space.
fn request_with(decision: Option<&str>) -> Request<RetrieveRequest> {
    let mut request = Request::new(retrieve_request());
    request.extensions_mut().insert(verified_member());
    if let Some(decision) = decision {
        request.metadata_mut().insert(
            "x-space-decision",
            MetadataValue::<Ascii>::try_from(decision).expect("an ASCII decision"),
        );
    }
    request
}

/// The Space a candidate is labelled with, as the wire carries it.
fn space_label(candidate: &Candidate) -> Option<String> {
    let fields = &candidate.metadata.as_ref()?.fields;
    match fields.get("space_ref")?.kind.as_ref()? {
        prost_types::value::Kind::StringValue(space) => Some(space.clone()),
        _ => None,
    }
}

#[tokio::test]
async fn a_space_scoped_unary_retrieve_labels_every_candidate_with_the_verified_space() {
    let room = Room::start().await;
    let decision = signed_decision(SPACE, USER);

    let response = room
        .service
        .retrieve(request_with(Some(decision.as_str())))
        .await
        .expect("the verified Space retrieval answers")
        .into_inner();

    assert!(
        !response.candidates.is_empty(),
        "the room's documents are retrieved"
    );
    for candidate in &response.candidates {
        assert_eq!(
            space_label(candidate).as_deref(),
            Some(SPACE),
            "candidate {} carries the verified Space",
            candidate.document_id
        );
    }
}

#[tokio::test]
async fn a_space_scoped_stream_labels_every_candidate_and_ends_with_its_trailer() {
    let room = Room::start().await;
    let decision = signed_decision(SPACE, USER);

    let frames: Vec<RetrievalChunk> = room
        .service
        .retrieve_stream(request_with(Some(decision.as_str())))
        .await
        .expect("the verified Space stream opens")
        .into_inner()
        .map(|frame| frame.expect("no frame of the stream is an error"))
        .collect()
        .await;

    let candidates: Vec<&Candidate> = frames
        .iter()
        .filter_map(|frame| match &frame.payload {
            Some(Payload::Candidate(candidate)) => Some(candidate),
            _ => None,
        })
        .collect();
    assert!(!candidates.is_empty(), "the room's documents are streamed");
    for candidate in &candidates {
        assert_eq!(
            space_label(candidate).as_deref(),
            Some(SPACE),
            "streamed candidate {} carries the verified Space",
            candidate.document_id
        );
    }
    match frames.last().and_then(|frame| frame.payload.as_ref()) {
        Some(Payload::Trailer(trailer)) => {
            assert_eq!(trailer.candidate_count as usize, candidates.len());
        }
        other => panic!("the stream ends with its trailer, found {other:?}"),
    }
}

#[tokio::test]
async fn an_unscoped_retrieve_labels_no_candidate() {
    let room = Room::start().await;

    let response = room
        .service
        .retrieve(request_with(None))
        .await
        .expect("the unscoped retrieval answers")
        .into_inner();

    assert!(
        !response.candidates.is_empty(),
        "the unscoped answer is retrieved"
    );
    for candidate in &response.candidates {
        assert!(
            candidate.metadata.is_none(),
            "an unscoped candidate carries no Space label, found {:?}",
            candidate.metadata
        );
    }
}

#[tokio::test]
async fn a_forged_space_decision_is_refused_before_any_search_so_no_label_can_be_set() {
    let room = Room::start().await;
    let forged = decision_rewritten_to(&signed_decision(SPACE, USER), "space:org-1:room-8");

    let status = match room
        .service
        .retrieve(request_with(Some(forged.as_str())))
        .await
    {
        Ok(_) => panic!("a forged Space decision is refused"),
        Err(status) => status,
    };

    assert_eq!(status.code(), Code::PermissionDenied);
    assert!(
        room.searches.lock().expect("the search log").is_empty(),
        "no search runs for a refused decision"
    );
}
