//! HTTP health/metrics endpoints for execution-core on :8083.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use tracing::info;

use crate::capability_client::{CapabilityClient, CapabilityDecisionRequest};

/// Process readiness shared by the gRPC and HTTP servers.
///
/// Liveness means the process can answer HTTP. Readiness is stricter: it is
/// true only while Execution Core's required gRPC listener is accepting work.
#[derive(Clone, Default)]
pub struct Readiness(Arc<AtomicBool>);

impl Readiness {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_grpc_ready(&self, ready: bool) {
        self.0.store(ready, Ordering::Release);
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// The stable identifier this process presents when requesting a signed
/// Space capability decision, and that a lease request must later present
/// back to sandbox-manager for the backend-pinning check to accept it. Reads
/// `EXECUTION_CORE_BACKEND_ID` first (an operator-assigned, stable value);
/// falls back to `HOSTNAME` (typically the container ID in this stack) so a
/// deployment that never set the explicit variable still gets *some* stable,
/// non-empty identifier rather than an empty string a pinning check could
/// mishandle. Never fabricated as random per-call data: the whole point is
/// that the SAME value is presented on every request from this process.
pub(crate) fn resolve_backend_id() -> String {
    for var in ["EXECUTION_CORE_BACKEND_ID", "HOSTNAME"] {
        if let Ok(value) = std::env::var(var) {
            let value = value.trim();
            if !value.is_empty() {
                return value.to_owned();
            }
        }
    }
    "execution-core-unknown-backend".to_owned()
}

/// Combined Axum state for the :8083 router. `Readiness` is extracted
/// unchanged by existing handlers via `FromRef` below; only
/// `capability_profile` needs the rest.
#[derive(Clone)]
struct HttpState {
    readiness: Readiness,
    capability_client: std::sync::Arc<Option<CapabilityClient>>,
    backend_id: std::sync::Arc<str>,
}

impl axum::extract::FromRef<HttpState> for Readiness {
    fn from_ref(state: &HttpState) -> Self {
        state.readiness.clone()
    }
}

/// Start the HTTP health server on :8083.
///
/// # Errors
///
/// Returns an error if the server fails to bind or serve, or if
/// `CapabilityClient::from_env` finds a present-but-malformed configuration
/// (never for absent configuration, which is a valid disabled state).
pub async fn serve(readiness: Readiness) -> anyhow::Result<()> {
    let capability_client = CapabilityClient::from_env()
        .map_err(|error| anyhow::anyhow!("sandbox capability client configuration: {error}"))?;
    let state = HttpState {
        readiness,
        capability_client: std::sync::Arc::new(capability_client),
        backend_id: resolve_backend_id().into(),
    };
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/capability-profile", get(capability_profile))
        .route("/metrics", get(metrics_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8083").await?;
    info!("HTTP health listening on :8083");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

fn ready_status(readiness: &Readiness) -> StatusCode {
    if readiness.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn readyz(State(readiness): State<Readiness>) -> impl IntoResponse {
    let status = ready_status(&readiness);
    let body = if status == StatusCode::OK {
        "ok"
    } else {
        "gRPC unavailable"
    };
    (status, body)
}

/// Optional Space context for requesting a signed capability decision
/// alongside the measured profile. All fields must be present together (any
/// one missing means "just report the plain measurement") — this endpoint
/// never partially authenticates a decision request.
#[derive(serde::Deserialize, Default)]
struct CapabilityProfileQuery {
    org_id: Option<String>,
    space_ref: Option<String>,
    subject_id: Option<String>,
    idempotency_key: Option<String>,
}

#[derive(serde::Serialize)]
struct CapabilityProfileResponse {
    profile: crate::sandbox::SandboxCapabilityProfile,
    backend_id: String,
    /// A signed `model.space.capability_profile` decision token from Control
    /// Plane, or `None` when no Control client is configured, the local
    /// isolation backend is unavailable, no Space context was supplied, or
    /// Control declined the request (missing entitlement, wrong role, etc.).
    /// Never fabricated: an absent decision here must read as "not
    /// authorized yet," not as a transport hiccup masquerading as success.
    decision: Option<String>,
}

/// Measured substrate facts, intended for a Model-owned Space lease resolver.
/// Reporting the plain measurement never requires a Space context and never
/// exposes a credential. Attaching a signed decision additionally requires a
/// configured Control client, a working local isolation backend, and a full
/// Space context in the query string — Control still independently checks
/// entitlement and role before it will sign anything.
async fn capability_profile(
    State(state): State<HttpState>,
    Query(query): Query<CapabilityProfileQuery>,
) -> Json<CapabilityProfileResponse> {
    let profile = crate::sandbox::capability_profile();
    let backend_id = state.backend_id.to_string();
    let decision = match (
        state.capability_client.as_ref(),
        query.org_id,
        query.space_ref,
        query.subject_id,
        query.idempotency_key,
    ) {
        (Some(client), Some(org_id), Some(space_ref), Some(subject_id), Some(idempotency_key))
            if profile.local_isolation_available =>
        {
            let request = CapabilityDecisionRequest {
                org_id: &org_id,
                space_ref: &space_ref,
                subject_id: &subject_id,
                backend_id: &backend_id,
                profile: &profile,
                idempotency_key: &idempotency_key,
            };
            match client.request_decision(&request).await {
                Ok((token, _claims)) => Some(token),
                Err(error) => {
                    tracing::warn!(error = %error, "sandbox capability decision request failed");
                    None
                }
            }
        }
        _ => None,
    };
    Json(CapabilityProfileResponse {
        profile,
        backend_id,
        decision,
    })
}

async fn metrics_handler() -> impl IntoResponse {
    (StatusCode::OK, "# HELP execution_core_steps_total\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_is_false_until_grpc_listener_is_bound() {
        let readiness = Readiness::new();

        assert!(!readiness.is_ready());
        assert_eq!(ready_status(&readiness), StatusCode::SERVICE_UNAVAILABLE);

        readiness.set_grpc_ready(true);
        assert!(readiness.is_ready());
        assert_eq!(ready_status(&readiness), StatusCode::OK);

        readiness.set_grpc_ready(false);
        assert!(!readiness.is_ready());
        assert_eq!(ready_status(&readiness), StatusCode::SERVICE_UNAVAILABLE);
    }

    fn test_state(capability_client: Option<CapabilityClient>) -> HttpState {
        HttpState {
            readiness: Readiness::new(),
            capability_client: std::sync::Arc::new(capability_client),
            backend_id: "test-backend".into(),
        }
    }

    #[tokio::test]
    async fn capability_profile_endpoint_is_explicitly_ephemeral_and_credential_free() {
        let Json(response) = capability_profile(
            State(test_state(None)),
            Query(CapabilityProfileQuery::default()),
        )
        .await;
        assert_eq!(response.profile.persistence, "ephemeral");
        assert!(!response.profile.backup);
        assert_eq!(response.profile.credential_mode, "credential_free");
        assert_eq!(response.profile.egress, "disabled_by_default");
        assert_eq!(response.backend_id, "test-backend");
    }

    #[tokio::test]
    async fn decision_is_none_without_control_client() {
        // No client configured at all.
        let Json(response) = capability_profile(
            State(test_state(None)),
            Query(CapabilityProfileQuery {
                org_id: Some("org-1".to_owned()),
                space_ref: Some("space-1".to_owned()),
                subject_id: Some("user-1".to_owned()),
                idempotency_key: Some("key-1".to_owned()),
            }),
        )
        .await;
        assert!(response.decision.is_none());
    }

    #[tokio::test]
    async fn decision_is_none_without_full_space_context() {
        // A client IS configured, but the query is missing subject_id — the
        // whole point of "all fields present together" is that a partial
        // context must not silently proceed with an empty/forged subject.
        let client = CapabilityClient::from_env().ok().flatten();
        let Json(response) = capability_profile(
            State(test_state(client)),
            Query(CapabilityProfileQuery {
                org_id: Some("org-1".to_owned()),
                space_ref: Some("space-1".to_owned()),
                subject_id: None,
                idempotency_key: Some("key-1".to_owned()),
            }),
        )
        .await;
        assert!(response.decision.is_none());
    }
}
