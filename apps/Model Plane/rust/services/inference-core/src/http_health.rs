//! HTTP health/metrics endpoints for inference-core on :8082.
//!
//! Also hosts the internal runtime routing-policy admin surface:
//! `GET /internal/v1/router-policy` returns the currently-loaded effective
//! [`RoutingPolicy`]; `PUT /internal/v1/router-policy` persists a new one to
//! session-core and hot-swaps the live copy immediately. With
//! `ROUTING_POLICY_PROMOTION_REQUIRED=true` the PUT instead records a candidate
//! (answering 202 with its offline diff) and
//! `POST /internal/v1/router-policy/candidates/:id/promote` is the only way a
//! candidate becomes the live policy (EP-14 S10).

use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::{
    extract::{Path, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use metrics_exporter_prometheus::PrometheusHandle;
use tracing::info;

use crate::auth::JwtVerifier;
use crate::provider::fallback::FallbackChain;
use crate::provider::policy_client::{CandidateError, PolicyClient, PolicyWriteError};
use crate::provider::routing_diff::{self, RouteFacts};
use crate::provider::routing_policy::RoutingPolicy;
use crate::provider::{ModelInfo, PrivacyTier};
use crate::readiness::GrpcReadiness;

/// Shared state for the routing-policy admin routes.
#[derive(Clone)]
pub struct PolicyState {
    /// Auth Core verifier shared with the gRPC boundary. Policy routes require
    /// a verified `inference-core` audience plus a dedicated admin scope.
    pub auth: JwtVerifier,
    /// Live policy, hot-swapped by the refresh loop and the PUT write-through.
    pub policy: Arc<ArcSwap<RoutingPolicy>>,
    /// session-core client for the write path. `None` → PUT returns 503.
    pub client: Option<Arc<PolicyClient>>,
    /// The chain the policy steers. A role table must fit its chat catalogue (EP-13 S4).
    pub chain: FallbackChain,
}

/// Start the HTTP health server on :8082. `metrics` is the handle of the
/// process-global recorder ([`crate::metrics::install_recorder`]) that
/// `/metrics` renders.
///
/// # Errors
///
/// Returns an error if the server fails to bind or serve.
pub async fn serve(
    state: PolicyState,
    grpc_readiness: GrpcReadiness,
    metrics: PrometheusHandle,
) -> anyhow::Result<()> {
    let app = router(state, grpc_readiness, metrics);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8082").await?;
    info!("HTTP health listening on :8082");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Every route of the :8082 server. Split from [`serve`] so a test can mount it
/// on an ephemeral port.
pub fn router(
    state: PolicyState,
    grpc_readiness: GrpcReadiness,
    metrics: PrometheusHandle,
) -> Router {
    let policy_routes = Router::new()
        .route(
            "/internal/v1/router-policy",
            get(get_policy).put(put_policy),
        )
        .route(
            "/internal/v1/router-policy/candidates/:id/promote",
            post(promote_policy_candidate),
        )
        .with_state(state);

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/version", get(version))
        .merge(metrics_route(metrics))
        .merge(policy_routes)
        .layer(Extension(grpc_readiness))
}

/// `GET /metrics`: the Prometheus text exposition of the recorder behind `handle`.
pub fn metrics_route(handle: PrometheusHandle) -> Router {
    Router::new().route("/metrics", get(metrics_handler).with_state(handle))
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(Extension(grpc_readiness): Extension<GrpcReadiness>) -> Response {
    if grpc_readiness.is_bound() {
        (StatusCode::OK, "ok").into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "grpc listener not bound").into_response()
    }
}

/// Prometheus text exposition format, version 0.0.4.
const PROMETHEUS_TEXT: &str = "text/plain; version=0.0.4; charset=utf-8";

async fn metrics_handler(State(handle): State<PrometheusHandle>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(CONTENT_TYPE, PROMETHEUS_TEXT)],
        handle.render(),
    )
}

/// Reports what is actually running in this container: the git revision and
/// build timestamp baked into the image (see `Dockerfile`'s `SOURCE_REVISION`
/// / `BUILD_DATE` build args, re-exposed as runtime `ENV` so no Docker/registry
/// access is needed to answer "what SHA is deployed here"). Falls back to the
/// same `unverified`/`unknown` defaults the image LABELs use when unset.
async fn version() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "inference-core",
        "revision": std::env::var("SOURCE_REVISION").unwrap_or_else(|_| "unverified".to_string()),
        "build_date": std::env::var("BUILD_DATE").unwrap_or_else(|_| "unknown".to_string()),
        "cargo_version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Return the currently-loaded effective routing policy.
async fn get_policy(State(state): State<PolicyState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize_policy_admin(&state.auth, &headers).await {
        return response;
    }
    Json(RoutingPolicy::clone(&state.policy.load_full())).into_response()
}

/// Persist a new routing policy to session-core, then hot-swap the live copy.
/// Returns the stored policy (as session-core echoes it back). 503 when no
/// session-core store is configured. 409 when the live version is no longer the
/// `expected_version` the PUT carries (W4-BYO-3).
async fn put_policy(
    State(state): State<PolicyState>,
    headers: HeaderMap,
    Json(policy): Json<RoutingPolicy>,
) -> impl IntoResponse {
    if let Err(response) = authorize_policy_admin(&state.auth, &headers).await {
        return response;
    }
    let catalogue = state.chain.list_models("chat", "");
    if let Some(response) = role_table_refusal(&policy, &catalogue) {
        return response;
    }
    if let Some(response) = catalogue_overlay_refusal(&policy, &catalogue) {
        return response;
    }
    let Some(client) = state.client.clone() else {
        return policy_store_unconfigured();
    };
    if promotion_required() {
        // With promotion required a PUT never takes a policy live: it records a candidate and answers 202.
        return propose_policy_candidate(&state, &client, &policy).await;
    }
    match client
        .set(&policy, policy.expected_version, "inference-core-admin")
        .await
    {
        Ok(stored) => {
            state.policy.store(Arc::new(stored.clone()));
            (StatusCode::OK, Json(stored)).into_response()
        }
        Err(PolicyWriteError::VersionConflict) => {
            policy_version_conflict(client.live_version().await)
        }
        Err(PolicyWriteError::Failed(message)) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": { "code": "policy_store_write_failed", "message": message }
            })),
        )
            .into_response(),
    }
}

/// The 409 for a write whose `expected_version` is no longer the live version (W4-BYO-3). The body carries the live
/// version number and nothing else of the policy, so the admin reloads it and retries against that version.
fn policy_version_conflict(live_version: Option<i64>) -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": {
                "code": "policy_version_conflict",
                "message": "the routing policy changed since the version you read; reload it and retry",
                "details": { "live_version": live_version },
            }
        })),
    )
        .into_response()
}

/// The 503 for an admin write or promotion when no session-core store is configured.
fn policy_store_unconfigured() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "error": {
                "code": "policy_store_unconfigured",
                "message": "SESSION_CORE_URL is not set; the routing policy store is unavailable"
            }
        })),
    )
        .into_response()
}

/// A typed `{ error: { code, message } }` envelope.
fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

/// What the offline diff needs to know about the models this process can serve. inference-core holds no price table
/// of its own, so a model counts as priced when the chat catalogue serves it; a model the catalogue does not serve is
/// reported in the diff's `unknown_models`. A model has a ZDR route when its provider's privacy tier is contractual
/// ZDR or stronger.
fn route_facts(catalogue: &[ModelInfo]) -> RouteFacts {
    let served = catalogue.iter().map(|model| model.id.as_str());
    let zdr_routed = catalogue
        .iter()
        .filter(|model| model.privacy_tier >= PrivacyTier::ZdrContractual)
        .map(|model| model.id.as_str());
    RouteFacts::new(served, zdr_routed)
}

/// Record `policy` as a candidate and answer 202 with its id and offline diff (EP-14 S10). The live policy is never
/// written. The diff replays the checked-in synthetic corpus, so it holds no org or user content.
async fn propose_policy_candidate(
    state: &PolicyState,
    client: &PolicyClient,
    policy: &RoutingPolicy,
) -> Response {
    let corpus = match routing_diff::embedded_corpus() {
        Ok(corpus) => corpus,
        Err(message) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "routing_corpus_unavailable",
                &message,
            );
        }
    };
    let facts = route_facts(&state.chain.list_models("chat", ""));
    let live = state.policy.load_full();
    let report = routing_diff::diff(&live, policy, &corpus, &facts);
    let diff = match serde_json::to_value(&report) {
        Ok(diff) => diff,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "routing_diff_unserializable",
                &format!("serialize routing diff: {error}"),
            );
        }
    };
    match client
        .propose_candidate(policy, "inference-core-admin", diff.to_string())
        .await
    {
        Ok(candidate) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "data": { "candidate_id": candidate.id, "diff": diff }
            })),
        )
            .into_response(),
        Err(error) => candidate_error_response(&error),
    }
}

/// Make an evaluated candidate the live policy and hot-swap the live copy (EP-14 S10). Only available while
/// promotion is required, so a deployment that never turns the switch on has no way to reach it.
async fn promote_policy_candidate(
    State(state): State<PolicyState>,
    Path(candidate_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = authorize_policy_admin(&state.auth, &headers).await {
        return response;
    }
    if !promotion_required() {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "candidate_store_disabled",
            "ROUTING_POLICY_PROMOTION_REQUIRED is off, so there is no candidate to promote",
        );
    }
    let Some(client) = state.client.clone() else {
        return policy_store_unconfigured();
    };
    match client
        .promote_candidate(&candidate_id, "inference-core-admin")
        .await
    {
        Ok(promoted) => {
            state.policy.store(Arc::new(promoted.clone()));
            (
                StatusCode::OK,
                Json(serde_json::json!({ "data": promoted })),
            )
                .into_response()
        }
        Err(error) => candidate_error_response(&error),
    }
}

/// Map a session-core candidate failure to the HTTP refusal the admin sees: a superseded candidate is a conflict, an
/// unevaluated one is unprocessable, a switched-off store is unavailable and an unknown id is not found. Anything
/// else is a failed write at the store.
fn candidate_error_response(error: &CandidateError) -> Response {
    let (status, code) = match error.code {
        tonic::Code::Aborted => (StatusCode::CONFLICT, "candidate_superseded"),
        tonic::Code::FailedPrecondition if error.message.contains(CANDIDATES_SWITCH) => {
            (StatusCode::SERVICE_UNAVAILABLE, "candidate_store_disabled")
        }
        tonic::Code::FailedPrecondition => {
            (StatusCode::UNPROCESSABLE_ENTITY, "candidate_not_evaluated")
        }
        tonic::Code::NotFound => (StatusCode::NOT_FOUND, "candidate_not_found"),
        tonic::Code::InvalidArgument => (StatusCode::BAD_REQUEST, "candidate_invalid"),
        _ => (StatusCode::BAD_GATEWAY, "policy_store_write_failed"),
    };
    error_response(status, code, &error.message)
}

/// The session-core switch whose name its own refusal message carries; that is how a switched-off store is told from
/// an unevaluated candidate, as both answer `FAILED_PRECONDITION`.
const CANDIDATES_SWITCH: &str = "SESSION_CORE_ROUTING_CANDIDATES_ENABLED";

/// The refusal for a role table the chat catalogue does not support (EP-13 S4), or `None` when the table
/// is valid. The table is platform-wide, so a bad entry would reach every org's side tasks. It is refused
/// here rather than skipped at every call.
fn role_table_refusal(policy: &RoutingPolicy, catalogue: &[ModelInfo]) -> Option<Response> {
    let errors = policy.validate_roles(catalogue).err()?;
    Some(
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "code": "invalid_role_table",
                    "message": "the role table does not fit the chat catalogue",
                    "details": { "issues": errors },
                }
            })),
        )
            .into_response(),
    )
}

/// The refusal for a catalogue overlay the chat catalogue does not support (F-t3-065), or `None` when the overlay is
/// valid. The overlay is platform-wide, so a bad alias would reach every org's listing and every pinned request. It is
/// refused here, on every write whether or not `INFERENCE_CATALOGUE_OVERLAY` is on, rather than skipped at every use.
fn catalogue_overlay_refusal(policy: &RoutingPolicy, catalogue: &[ModelInfo]) -> Option<Response> {
    let errors = policy.catalogue_overlay.validate(catalogue).err()?;
    Some(
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "code": "invalid_catalogue_overlay",
                    "message": "the catalogue overlay is not valid against the chat catalogue",
                    "details": { "issues": errors },
                }
            })),
        )
            .into_response(),
    )
}

/// The switch that makes a policy change a candidate (EP-14 S10). Read on each write; only the literal `true`
/// enables it.
const PROMOTION_REQUIRED_ENV: &str = "ROUTING_POLICY_PROMOTION_REQUIRED";

fn promotion_required_from(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

fn promotion_required() -> bool {
    promotion_required_from(std::env::var(PROMOTION_REQUIRED_ENV).ok().as_deref())
}

async fn authorize_policy_admin(
    verifier: &JwtVerifier,
    headers: &HeaderMap,
) -> Result<(), Response> {
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| auth_error(StatusCode::UNAUTHORIZED, "verified credential required"))?;
    let metadata = authorization
        .parse()
        .map_err(|_| auth_error(StatusCode::UNAUTHORIZED, "invalid credential"))?;
    let mut request = tonic::Request::new(());
    request.metadata_mut().insert("authorization", metadata);
    let principal = verifier
        .authenticate(&request)
        .await
        .map_err(|status| map_auth_status(&status))?;
    principal
        .authorize_policy_admin()
        .map_err(|status| map_auth_status(&status))
}

fn map_auth_status(status: &tonic::Status) -> Response {
    let http_status = match status.code() {
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::UNAUTHORIZED,
    };
    auth_error(http_status, status.message())
}

fn auth_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": { "code": "authorization_failed", "message": message }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::routing_policy::{RoleEntry, RolePurpose};
    use crate::readiness::GrpcReadiness;
    use axum::Extension;

    #[tokio::test]
    async fn put_policy_refuses_unknown_model_in_roles() {
        let catalogue = vec![ModelInfo {
            id: "gpt-4o-mini".to_owned(),
            modality: "chat".to_owned(),
            ..Default::default()
        }];
        let mut policy = RoutingPolicy::default();
        policy.roles.insert(
            RolePurpose::Title,
            RoleEntry {
                candidates: vec!["no-such-model".to_owned()],
                effort: None,
            },
        );

        let refusal = role_table_refusal(&policy, &catalogue).expect("an unknown model is refused");
        assert_eq!(refusal.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(refusal.into_body(), usize::MAX)
            .await
            .expect("the refusal body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["error"]["code"], "invalid_role_table");
        assert_eq!(value["error"]["details"]["issues"][0]["reason"], "unknown_model");

        assert!(role_table_refusal(&RoutingPolicy::default(), &catalogue).is_none());
    }

    #[test]
    fn promotion_switch_reads_only_the_literal_true() {
        assert!(promotion_required_from(Some("true")));
        assert!(promotion_required_from(Some(" true ")));
        for raw in [None, Some(""), Some("1"), Some("TRUE"), Some("yes"), Some("false")] {
            assert!(!promotion_required_from(raw), "{raw:?}");
        }
    }

    #[test]
    fn put_with_switch_off_unchanged() {
        // Off (unset), the write takes the path it always took: the direct store write.
        assert!(!promotion_required_from(None));
    }

    #[tokio::test]
    async fn put_with_switch_on_maps_a_switched_off_store_to_503() {
        let refusal = candidate_error_response(&CandidateError {
            code: tonic::Code::FailedPrecondition,
            message: format!("routing policy candidates are not enabled ({CANDIDATES_SWITCH})"),
        });
        assert_eq!(refusal.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(refusal.into_body(), usize::MAX)
            .await
            .expect("the refusal body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["error"]["code"], "candidate_store_disabled");
    }

    #[tokio::test]
    async fn readyz_fails_until_grpc_listener_is_bound() {
        let readiness = GrpcReadiness::new();

        let before_bind = readyz(Extension(readiness.clone())).await.into_response();
        assert_eq!(before_bind.status(), StatusCode::SERVICE_UNAVAILABLE);

        readiness.mark_bound();
        let after_bind = readyz(Extension(readiness)).await.into_response();
        assert_eq!(after_bind.status(), StatusCode::OK);
    }

    /// `/metrics` is what the recorder rendered, over a real HTTP round trip,
    /// not the fixed 37-byte line it used to answer with whatever had happened.
    #[tokio::test]
    async fn metrics_serves_the_recorders_exposition_over_http() {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        ::metrics::with_local_recorder(&recorder, || {
            crate::metrics::record_request(
                "infer",
                "service:data-orchestrator",
                "gpt-4o-mini",
                "azure-openai",
                "zdr_unavailable",
            );
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral port");
        let address = listener.local_addr().expect("the bound address");
        tokio::spawn(async move { axum::serve(listener, metrics_route(handle)).await });

        let response = reqwest::get(format!("http://{address}/metrics"))
            .await
            .expect("the scrape");

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some(PROMETHEUS_TEXT)
        );
        let body = response.text().await.expect("the body");
        assert!(body.len() > 37, "more than the old stub: {body}");
        assert!(
            body.contains("inference_requests_total{")
                && body.contains("service=\"service:data-orchestrator\"")
                && body.contains("outcome=\"zdr_unavailable\""),
            "{body}"
        );
    }
}
