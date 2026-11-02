//! What the offline downstreams of the gateway's integration tests promise
//! (`tests/support/offline_downstreams.rs`): every gRPC client a test does not serve answers
//! from inside the process, at once, with what a refused connection gives, and the helper
//! cannot silently miss a client that `AppState` grows later.
//!
//! The socket-level proof (the three integration files pass with the dev stack's ports held
//! by a listener that accepts and never answers) is a run, not a test; its recipe is in the
//! commit that introduced the helper.

#[path = "support/offline_downstreams.rs"]
mod offline_downstreams;

use offline_downstreams::{offline_channel, offline_state};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tonic::{transport::Endpoint, Code, Request, Status};

/// What a call answers, as its status code: a downstream that answers `Ok` here is not offline.
fn code<T>(answer: Result<tonic::Response<T>, Status>) -> Code {
    match answer {
        Ok(_) => Code::Ok,
        Err(status) => status.code(),
    }
}

/// A channel at a port nothing listens on, which is what the dev stack's ports are on a clean
/// machine: the connect is refused.
async fn refused_channel() -> tonic::transport::Channel {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);
    Endpoint::from_shared(format!("http://{addr}"))
        .expect("endpoint")
        .connect_lazy()
}

/// One unary call on a gRPC client of the state, with an empty request, as `(client, code)`.
macro_rules! call {
    ($state:expr, $client:ident . $method:ident) => {
        (
            stringify!($client),
            code(
                $state
                    .$client
                    .clone()
                    .$method(Request::new(Default::default()))
                    .await,
            ),
        )
    };
}

/// One call on every gRPC client of `AppState`, in the order of the struct.
macro_rules! one_call_per_client {
    ($state:expr) => {{
        let state = $state;
        vec![
            call!(state, inference_client.infer),
            call!(state, session_client.create_thread),
            call!(state, message_search_client.search_messages),
            call!(state, message_tree_client.set_active_leaf),
            call!(state, learning_client.list_learning_ledger),
            call!(state, space_events_client.stream_space_events),
            call!(state, space_room_read_client.list_space_artifacts),
            call!(state, managed_run_client.start_managed_run),
            call!(state, run_client.get_run),
            call!(state, orchestration_client.list_plans),
            call!(state, consume_client.list_plans),
            call!(state, execution_client.resume_run),
            call!(state, sandbox_client.acquire_lease),
            call!(state, browser_client.health),
            call!(state, memory_client.health),
            call!(state, capability_client.list_capabilities),
            call!(state, retrieval_client.retrieve),
            call!(state, document_client.list_documents),
            call!(state, knowledge_client.check_permissions),
            call!(state, graph_client.get_entity),
            call!(state, finetune_jobs_client.list_jobs),
            call!(state, wiki_client.get_page),
        ]
    }};
}

/// Every client answers `Unavailable` without the test serving anything, and all of them
/// together answer well inside the bound: a client that waited on a socket would sit out the
/// bound (or, against a wedged engine, forever).
#[tokio::test]
async fn every_grpc_client_of_the_offline_state_is_unavailable_at_once() {
    let started = Instant::now();
    let answers = tokio::time::timeout(Duration::from_secs(10), async {
        one_call_per_client!(offline_state())
    })
    .await
    .expect("an offline downstream must answer at once, not wait on a socket");

    for (client, code) in &answers {
        assert_eq!(*code, Code::Unavailable, "{client} must answer Unavailable");
    }
    assert_eq!(answers.len(), 22, "one call per gRPC client of AppState");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "all twenty-one answered in {:?}",
        started.elapsed()
    );
}

/// Whether a failure carries the connect-phase refusal the gateway's executor dispatch reads
/// as "the request never left" (`dispatch_never_reached_execution_core` in `sse.rs`): an
/// `io::Error` of a refusal kind somewhere in the status's source chain.
fn carries_a_connect_refusal(status: &Status) -> bool {
    let mut source = std::error::Error::source(status);
    while let Some(error) = source {
        if let Some(io) = error.downcast_ref::<std::io::Error>() {
            if io.kind() == std::io::ErrorKind::ConnectionRefused {
                return true;
            }
        }
        source = error.source();
    }
    false
}

/// The offline double is not a different failure from the one the tests already tolerate on a
/// clean machine: a call over it answers the same code a refused connect gives, and the
/// failure carries the same connect-phase refusal, so a scenario that leaves the executor
/// offline is classified as "never delivered", as it is against a closed port.
#[tokio::test]
async fn an_offline_channel_fails_as_a_refused_connection_fails() {
    use mp_contracts::model_plane::v1::execution_core_client::ExecutionCoreClient;

    let offline = ExecutionCoreClient::new(offline_channel())
        .resume_run(Request::new(Default::default()))
        .await
        .expect_err("an offline downstream never answers Ok");
    let refused = ExecutionCoreClient::new(refused_channel().await)
        .resume_run(Request::new(Default::default()))
        .await
        .expect_err("a closed port never answers Ok");

    assert_eq!(offline.code(), Code::Unavailable);
    assert_eq!(offline.code(), refused.code());
    assert!(
        carries_a_connect_refusal(&refused),
        "the control: a closed port's failure carries the refusal ({refused:?})"
    );
    assert!(
        carries_a_connect_refusal(&offline),
        "an offline channel's failure must carry the same refusal ({offline:?})"
    );
}

/// `AppState` gains a gRPC client field and the helper does not name it: the test that uses
/// it would dial the dev stack's port for that client again, and nothing else would say so.
/// Reads both sources, so adding a client to `AppState` fails here until the helper replaces it.
#[test]
fn the_offline_state_replaces_every_grpc_client_field_of_app_state() {
    let state_source = include_str!("../src/state.rs");
    let helper_source = include_str!("support/offline_downstreams.rs");

    let struct_start = state_source
        .find("pub struct AppState {")
        .expect("AppState is declared in state.rs");
    let struct_body = &state_source[struct_start..];
    let struct_body = &struct_body[..struct_body.find("\n}").expect("the struct closes")];

    let client_fields: Vec<&str> = struct_body
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (name, ty) = line.strip_prefix("pub ")?.split_once(':')?;
            (ty.trim().ends_with("Client<Channel>,")).then_some(name.trim())
        })
        .collect();

    assert_eq!(
        client_fields.len(),
        22,
        "the gRPC client fields of AppState changed ({client_fields:?}): update this count, the \
         helper and `one_call_per_client!` together"
    );
    for field in client_fields {
        assert!(
            helper_source.contains(&format!("state.{field} = ")),
            "offline_state() does not replace AppState::{field}: a test that serves only some \
             clients would dial the dev stack's default port for it"
        );
    }
}
