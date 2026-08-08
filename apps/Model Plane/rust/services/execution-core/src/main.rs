//! execution-core — runtime loop ownership for agent execution.
//!
//! gRPC on :9093, HTTP health/metrics on :8083.

use anyhow::Result;
use execution_core::{auth::JwtVerifier, http_health::Readiness, state::StateStore};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let _otel_guard = mp_telemetry::init("execution-core")?;
    info!("execution-core starting");

    // Probe the OS sandbox ONCE, at boot, on the record. A capability problem in
    // the container (bubblewrap installed but unable to build a namespace) used to
    // surface as a generic per-call tool failure; now it is one line in the
    // startup log, before any tool has run. Result is cached for the process.
    execution_core::sandbox::log_support();

    // Attest what the sandbox probe just measured to capability-core, then keep
    // attesting: capability-core denies dispatch for any capability whose runtime
    // health has never been attested (or whose attestation has expired), and
    // nothing else in the system reports it. Detached and infallible on purpose —
    // a service that cannot report its health must still serve.
    execution_core::health_attest::spawn_heartbeat();

    // Post-approval continuation dispatcher: claims durable approval-delivery
    // leases and resumes the one action kind (execute_provider_action) this
    // codebase can currently re-attest and re-execute safely. See
    // approval_delivery_worker's module doc for scope and protocol. Detached,
    // same as the heartbeat above — idles quietly until configured.
    execution_core::approval_delivery_worker::spawn();

    let readiness = Readiness::new();
    let state = StateStore::new();
    let auth = JwtVerifier::from_env().await?;
    let grpc_handle = tokio::spawn(execution_core::grpc::serve(state, readiness.clone(), auth));
    let http_handle = tokio::spawn(execution_core::http_health::serve(readiness));

    let shutdown = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
        info!("SIGTERM received, shutting down");
    };

    tokio::select! {
        result = grpc_handle => result??,
        result = http_handle => result??,
        () = shutdown => {},
    }

    info!("execution-core stopped");
    Ok(())
}
