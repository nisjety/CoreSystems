//! Fine-tune jobs domain — proxies the SPA's `/api/v1/finetune/*` surface to
//! model-gateway's `/v1/finetune/*` backend (the Azure OpenAI fine-tuning
//! lifecycle: create job, upload training data, poll status, cancel).
//!
//! model-gateway authenticates internal calls with the internal API key +
//! `x-org-id` + `x-user-*` actor headers (its `require_admin` reads
//! `x-user-role`) — exactly what `proxy_json` forwards, so the JSON endpoints
//! are thin proxies. The training-file endpoint is `multipart/form-data`, so it
//! is a raw passthrough that preserves the inbound Content-Type/boundary + body.

use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    middleware::{require_session, AuthenticatedUser},
    upstream::proxy_json,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/finetune/jobs", get(list_jobs).post(create_job))
        .route("/api/v1/finetune/jobs/upload", post(upload_training_file))
        .route(
            "/api/v1/finetune/jobs/:job_id",
            get(get_job).delete(cancel_job),
        )
        .route("/api/v1/finetune/jobs/:job_id/deploy", post(deploy_job))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn qs(uri: &Uri) -> String {
    uri.query()
        .filter(|q| !q.is_empty())
        .map(|q| format!("?{q}"))
        .unwrap_or_default()
}

// ── handlers ──────────────────────────────────────────────────────────────--

async fn list_jobs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> impl IntoResponse {
    let url = format!("{}/v1/finetune/jobs{}", state.model_gateway_url, qs(&uri));
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(org_id.as_str()),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn create_job(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let url = format!("{}/v1/finetune/jobs", state.model_gateway_url);
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(org_id.as_str()),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn get_job(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let url = format!(
        "{}/v1/finetune/jobs/{}",
        state.model_gateway_url,
        urlencoding::encode(&job_id)
    );
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(org_id.as_str()),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn cancel_job(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let url = format!(
        "{}/v1/finetune/jobs/{}",
        state.model_gateway_url,
        urlencoding::encode(&job_id)
    );
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    proxy_json(
        &state,
        Method::DELETE,
        &url,
        None,
        Some(org_id.as_str()),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// Promote a fine-tuned model to a deployment tier (e.g. `production`).
/// Forwards the JSON body (`{"tier": "production" | "developer"}`) verbatim
/// to model-gateway's `/v1/finetune/jobs/{job_id}/deploy`, mirroring the
/// other JSON handlers' org + actor header forwarding.
async fn deploy_job(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(job_id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let url = format!(
        "{}/v1/finetune/jobs/{}/deploy",
        state.model_gateway_url,
        urlencoding::encode(&job_id)
    );
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(org_id.as_str()),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// Raw multipart passthrough for the JSONL training-file upload. `proxy_json`
/// only handles JSON bodies, so we forward the body bytes verbatim with the
/// inbound `content-type` (which carries the multipart boundary) and the same
/// internal-auth/org/actor headers `proxy_json` injects.
async fn upload_training_file(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let url = format!("{}/v1/finetune/jobs/upload", state.model_gateway_url);
    let actor = actor_for(&user);

    let mut req = state
        .client
        .post(&url)
        .header("x-internal-api-key", &state.internal_api_key)
        .header("x-user-id", &actor.user_id);
    if !actor.user_email.is_empty() {
        req = req.header("x-user-email", &actor.user_email);
    }
    if !actor.user_name.is_empty() {
        req = req.header("x-user-name", &actor.user_name);
    }
    if !actor.user_role.is_empty() {
        req = req.header("x-user-role", &actor.user_role);
    }
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    if !org_id.is_empty() {
        req = req.header("x-org-id", org_id);
    }
    if let Some(content_type) = headers.get("content-type").and_then(|v| v.to_str().ok()) {
        req = req.header("content-type", content_type);
    }
    req = req.body(body);

    match req.send().await {
        Ok(response) => {
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let body = response.json::<Value>().await.unwrap_or(Value::Null);
            (status, Json(if body.is_null() { json!({}) } else { body }))
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        ),
    }
}
