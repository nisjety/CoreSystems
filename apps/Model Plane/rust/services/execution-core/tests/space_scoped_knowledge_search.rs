//! The Space retrieval decision on the Data Plane wire (build plan WS18-S5).
//!
//! A Space-scoped Do run searches under the Control decision its run carries, and an organisation's
//! own search carries none. The decision rides as `x-space-decision` metadata on the one `Retrieve`
//! it scopes, beside the user's own credential. A decision that cannot be sent as metadata fails the
//! search; it is never sent without the decision, because that would search the whole organisation.

use execution_core::knowledge_tools::KnowledgeClient;
use mp_contracts::dataplane::retrieval_v2::{
    retrieval_service_server::{RetrievalService, RetrievalServiceServer},
    GetChunksRequest, GetChunksResponse, GetSourcesRequest, GetSourcesResponse, GetTraceRequest,
    GetTraceResponse, PackContextRequest, PackContextResponse, RetrievalChunk, RetrieveRequest,
    RetrieveResponse,
};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{transport::Server, Request, Response, Status};

/// What the stub Data Plane saw of one `Retrieve`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Seen {
    org_id: String,
    authorization: Option<String>,
    space_decision: Option<String>,
}

struct StubRetrieval {
    seen: Arc<Mutex<Vec<Seen>>>,
}

type NoStream =
    Pin<Box<dyn futures::Stream<Item = Result<RetrievalChunk, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl RetrievalService for StubRetrieval {
    async fn retrieve(
        &self,
        request: Request<RetrieveRequest>,
    ) -> Result<Response<RetrieveResponse>, Status> {
        let metadata = request.metadata();
        let text = |key: &str| {
            metadata
                .get(key)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        self.seen.lock().expect("seen lock").push(Seen {
            org_id: request.get_ref().org_id.clone(),
            authorization: text("authorization"),
            space_decision: text("x-space-decision"),
        });
        Ok(Response::new(RetrieveResponse::default()))
    }

    type RetrieveStreamStream = NoStream;
    async fn retrieve_stream(
        &self,
        _: Request<RetrieveRequest>,
    ) -> Result<Response<Self::RetrieveStreamStream>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn get_trace(
        &self,
        _: Request<GetTraceRequest>,
    ) -> Result<Response<GetTraceResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn get_sources(
        &self,
        _: Request<GetSourcesRequest>,
    ) -> Result<Response<GetSourcesResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn get_chunks(
        &self,
        _: Request<GetChunksRequest>,
    ) -> Result<Response<GetChunksResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
    async fn pack_context(
        &self,
        _: Request<PackContextRequest>,
    ) -> Result<Response<PackContextResponse>, Status> {
        Err(Status::unimplemented("not needed in this test"))
    }
}

async fn spawn_stub() -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
    let address = listener.local_addr().expect("stub address");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let stub = StubRetrieval { seen: seen.clone() };
    tokio::spawn(async move {
        Server::builder()
            .add_service(RetrievalServiceServer::new(stub))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    (format!("http://{address}"), seen)
}

/// One test drives the whole path, because `KnowledgeClient::from_env` reads the process
/// environment: two tests setting the endpoint in parallel could each dial the other's stub.
#[tokio::test]
async fn a_space_decision_rides_only_on_the_search_it_scopes() {
    let (endpoint, seen) = spawn_stub().await;
    std::env::set_var("DATAPLANE_RETRIEVAL_URL", &endpoint);
    let client = KnowledgeClient::from_env().expect("the retrieval endpoint is configured");

    client
        .search(
            "org-a",
            "user-a",
            "quarterly revenue",
            5,
            false,
            false,
            0,
            "user-token",
            Some("decision-1"),
        )
        .await
        .expect("the stub answers the scoped search");
    client
        .search(
            "org-a",
            "user-a",
            "quarterly revenue",
            5,
            false,
            false,
            0,
            "user-token",
            None,
        )
        .await
        .expect("the stub answers the organisation's own search");
    assert!(
        client
            .search(
                "org-a",
                "user-a",
                "quarterly revenue",
                5,
                false,
                false,
                0,
                "user-token",
                Some("bad\ndecision"),
            )
            .await
            .is_err(),
        "a decision that is not valid metadata fails the search"
    );

    let seen = seen.lock().expect("seen lock").clone();
    assert_eq!(
        seen.len(),
        2,
        "the unsendable decision reached no Retrieve: {seen:?}"
    );
    assert_eq!(
        seen[0],
        Seen {
            org_id: "org-a".to_owned(),
            authorization: Some("Bearer user-token".to_owned()),
            space_decision: Some("decision-1".to_owned()),
        },
        "the scoped search carries its decision beside the user's credential"
    );
    assert_eq!(
        seen[1],
        Seen {
            org_id: "org-a".to_owned(),
            authorization: Some("Bearer user-token".to_owned()),
            space_decision: None,
        },
        "the organisation's own search carries no decision"
    );
}
