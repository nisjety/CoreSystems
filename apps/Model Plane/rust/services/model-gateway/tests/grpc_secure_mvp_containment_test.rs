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

const COMPOSE_PRODUCTION: &str = include_str!("../../../../deploy/docker-compose.production.yml");

/// The gRPC compatibility ports may be probed locally, but must never be
/// reachable from off the machine.
///
/// This used to demand they be unpublished outright. They have been published
/// on loopback for local probing since the secure-MVP gates landed, so the old
/// assertion had drifted into asserting something the deployment stopped doing
/// — which is worse than no assertion, because a stale red test gets muted
/// rather than read.
///
/// The two halves below are the invariant that actually matters:
///   * dev may publish them, but only bound to 127.0.0.1. A bare `9090:9090`
///     binds 0.0.0.0 and puts a business gRPC surface on the network.
///   * production must publish nothing, so a loopback exception in dev cannot
///     ride along into a deployed environment.
#[test]
fn grpc_compatibility_ports_are_loopback_only_and_absent_in_production() {
    for port in ["9090:9090", "9092:9092", "9093:9093"] {
        for line in COMPOSE.lines() {
            let line = line.trim();
            if !line.starts_with('-') || !line.contains(port) {
                continue;
            }
            assert!(
                line.contains(&format!("127.0.0.1:{port}")),
                "gRPC port {port} must be bound to loopback, not published on every \
                 interface: {line}"
            );
        }
    }

    for service in ["model-gateway", "inference-core", "execution-core"] {
        let rest = COMPOSE_PRODUCTION
            .split(&format!("\n  {service}:\n"))
            .nth(1)
            .unwrap_or_else(|| panic!("{service} must be present in the production overlay"));
        // Stop at the next service key — a line indented by exactly two spaces.
        // Splitting on the literal "\n  " would cut at the block's own deeper
        // indentation and make this assert nothing.
        let resets = rest
            .lines()
            .take_while(|line| line.starts_with("    ") || line.trim().is_empty())
            .any(|line| line.trim() == "ports: !reset []");
        assert!(resets, "{service} must publish no host ports in production");
    }
}
