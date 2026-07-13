//! Secure-MVP compatibility contract for Model Plane gRPC surfaces.
//!
//! The legacy listeners are additive again, but every business RPC is behind a
//! dedicated exact-audience verifier. Health remains public and the ports are
//! internal-only in Compose until caller and rollback gates are complete.

const MODEL_MAIN: &str = include_str!("../src/main.rs");
const MODEL_LIB: &str = include_str!("../src/lib.rs");
const MODEL_GRPC: &str = include_str!("../src/grpc.rs");
const EXECUTION_MAIN: &str = include_str!("../../execution-core/src/main.rs");
const EXECUTION_LIB: &str = include_str!("../../execution-core/src/lib.rs");
const EXECUTION_GRPC: &str = include_str!("../../execution-core/src/grpc.rs");
const INFERENCE_MAIN: &str = include_str!("../../inference-core/src/main.rs");
const INFERENCE_LIB: &str = include_str!("../../inference-core/src/lib.rs");
const INFERENCE_GRPC: &str = include_str!("../../inference-core/src/grpc.rs");
const COMPOSE: &str = include_str!("../../../../deploy/docker-compose.yml");

#[test]
fn legacy_grpc_listeners_have_no_runtime_escape_hatch() {
    for (name, source) in [
        ("model-gateway", MODEL_MAIN),
        ("execution-core", EXECUTION_MAIN),
        ("inference-core", INFERENCE_MAIN),
    ] {
        assert!(
            !source.contains("ALLOW_UNAUTHENTICATED_GRPC")
                && !source.contains("ALLOW_UNVERIFIED_GRPC"),
            "{name} must not recognize an environment escape hatch"
        );
    }
}

#[test]
fn authenticated_server_contracts_are_additive_and_fail_closed() {
    assert!(MODEL_GRPC.contains("pub async fn serve("));
    assert!(MODEL_GRPC.contains("JwtVerifier::from_env().await?"));
    assert!(MODEL_GRPC.contains("ModelGatewayServer::with_interceptor"));

    assert!(EXECUTION_GRPC.contains("pub async fn serve("));
    assert!(EXECUTION_MAIN.contains("JwtVerifier::from_env().await?"));
    assert!(EXECUTION_GRPC.contains("ExecutionCoreServer::new"));
    assert!(EXECUTION_GRPC.contains("self.auth.authenticate(&request).await?"));

    assert!(INFERENCE_GRPC.contains("pub async fn serve_with_providers("));
    assert!(INFERENCE_MAIN.contains("JwtVerifier::from_env().await?"));
    assert!(INFERENCE_GRPC.contains("InferenceCoreServer::new"));

    for (name, source) in [
        ("model-gateway", MODEL_LIB),
        ("execution-core", EXECUTION_LIB),
        ("inference-core", INFERENCE_LIB),
    ] {
        assert!(
            source.contains("pub mod grpc;"),
            "{name} must export the additive compatibility service"
        );
    }
}

#[test]
fn disabled_grpc_ports_are_not_published_to_the_host() {
    for port in ["9090:9090", "9092:9092", "9093:9093"] {
        assert!(
            !COMPOSE.contains(port),
            "disabled gRPC port {port} must remain unpublished"
        );
    }
}
