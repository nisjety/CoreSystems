use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, Extension, Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::{
    auth::{
        authorize_approval, authorize_rebuild, AdminVerifier, AuthError, RebuildAuthorization,
        RebuildIntent,
    },
    jobs::{AdminJobStore, JobError, JobSpec, Submission},
    rebuild::RebuildContext,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RebuildRequest {
    org_id: Option<String>,
    #[serde(default)]
    global: bool,
    #[serde(default)]
    break_glass: bool,
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    clear: bool,
    approval_id: Option<String>,
    idempotency_key: Option<String>,
    reason: Option<String>,
}

#[derive(Clone)]
struct ApiState {
    rebuild: Arc<RebuildContext>,
    jobs: Arc<dyn AdminJobStore>,
}

pub fn router_with_store(
    ctx: Arc<RebuildContext>,
    verifier: Arc<AdminVerifier>,
    jobs: Arc<dyn AdminJobStore>,
) -> Router {
    let state = ApiState { rebuild: ctx, jobs };

    let admin = Router::new()
        .route("/admin/rebuild", post(rebuild_index))
        .route("/admin/rebuild/{job_id}", get(rebuild_status))
        .route("/admin/rebuild/{job_id}/approve", post(approve_rebuild))
        .route_layer(middleware::from_fn_with_state(verifier, require_admin_jwt))
        .layer(DefaultBodyLimit::max(16 * 1024));

    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readyz))
        .merge(admin)
        .with_state(state)
}

async fn require_admin_jwt(
    State(verifier): State<Arc<AdminVerifier>>,
    mut request: Request,
    next: Next,
) -> Response {
    let authorization = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let claims = match verifier.verify_authorization(authorization) {
        Ok(claims) => claims,
        Err(error) => return auth_error(error),
    };
    request.extensions_mut().insert(claims);
    next.run(request).await
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "quickwit-adapter-rs"}))
}

async fn readyz(State(state): State<ApiState>) -> impl axum::response::IntoResponse {
    // The GDPR erasure consumer is part of readiness because its failure was
    // otherwise invisible: the supervisor retries forever and only logs, so this
    // process reported ready while org erasure silently stopped being applied.
    let erasure = nats_connection::erasure_health::readiness();
    let ok = erasure.is_ready();
    (
        if ok {
            axum::http::StatusCode::OK
        } else {
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        },
        Json(serde_json::json!({
            "status": if ok { "ready" } else { "not_ready" },
            "service": "quickwit-adapter-rs",
            "index": state.rebuild.quickwit.index_id(),
            "checks": { "gdpr_erasure_consumer": erasure.as_str() }
        })),
    )
}

async fn rebuild_index(
    State(state): State<ApiState>,
    Extension(claims): Extension<crate::auth::AdminClaims>,
    Json(req): Json<RebuildRequest>,
) -> Response {
    let intent = RebuildIntent {
        requested_org_id: req.org_id,
        global: req.global,
        break_glass: req.break_glass,
        dry_run: req.dry_run,
        clear: req.clear,
        approval_id: req.approval_id.clone(),
        idempotency_key: req.idempotency_key.clone(),
        reason: req.reason.clone(),
    };
    let authorization = match authorize_rebuild(&claims, &intent) {
        Ok(authorization) => authorization,
        Err(error) => return auth_error(error),
    };

    let (org_id, global, dry_run, clear) = match authorization {
        RebuildAuthorization::Org {
            org_id,
            dry_run,
            clear,
        } => (Some(org_id), false, dry_run, clear),
        RebuildAuthorization::Global { dry_run, clear } => (None, true, dry_run, clear),
    };

    // Defence in depth: the authorization policy currently returns only
    // previews, but the production handler also refuses to execute a mutation.
    // Durable job state, audit, rate limiting and safe Quickwit delete-task
    // completion must exist before this branch can ever become executable.
    if dry_run {
        tracing::info!(operator = %claims.subject, org_scoped = !global, clear, "Quickwit rebuild preview authorized");
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "accepted": false, "dry_run": true,
                "scope": if global { "global" } else { "tenant" }, "clear": clear,
            })),
        )
            .into_response();
    }

    let spec = JobSpec {
        org_id,
        global,
        clear,
        requested_by: claims.subject,
        approval_id: req.approval_id.unwrap_or_default(),
        idempotency_key: req.idempotency_key.unwrap_or_default(),
        reason: req.reason.unwrap_or_default(),
    };
    match state.jobs.submit(spec).await {
        Ok(Submission::Created(job)) => job_response(StatusCode::ACCEPTED, &job, false),
        Ok(Submission::Existing(job)) => job_response(StatusCode::OK, &job, true),
        Err(error) => job_error(error),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalRequest {
    #[serde(default)]
    break_glass: bool,
    approval_id: String,
}

async fn approve_rebuild(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
    Extension(claims): Extension<crate::auth::AdminClaims>,
    Json(req): Json<ApprovalRequest>,
) -> Response {
    if !claims.has_scope(crate::auth::APPROVE_REBUILD_SCOPE) {
        return auth_error(AuthError::Forbidden);
    }
    let job = match state.jobs.get(&job_id).await {
        Ok(Some(job)) => job,
        Ok(None) | Err(JobError::NotFound) => return job_error(JobError::NotFound),
        Err(error) => return job_error(error),
    };
    if req.approval_id != job.approval_id {
        return auth_error(AuthError::Forbidden);
    }
    if !job.global && job.org_id.as_deref() != Some(claims.org_id.as_str()) {
        return job_error(JobError::NotFound);
    }
    if authorize_approval(&claims, job.org_id.as_deref(), job.global, req.break_glass).is_err() {
        return auth_error(AuthError::Forbidden);
    }
    match state.jobs.approve(&job_id, &claims.subject).await {
        Ok(job) => job_response(StatusCode::OK, &job, false),
        Err(error) => job_error(error),
    }
}

async fn rebuild_status(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
    Extension(claims): Extension<crate::auth::AdminClaims>,
) -> Response {
    if !claims.has_scope(crate::auth::REBUILD_SCOPE) {
        return auth_error(AuthError::Forbidden);
    }
    let job = match state.jobs.get(&job_id).await {
        Ok(Some(job)) => job,
        Ok(None) | Err(JobError::NotFound) => return job_error(JobError::NotFound),
        Err(error) => return job_error(error),
    };
    if (!job.global && job.org_id.as_deref() != Some(claims.org_id.as_str()))
        || (job.global
            && !claims
                .scopes
                .iter()
                .any(|scope| scope == crate::auth::GLOBAL_REBUILD_SCOPE))
    {
        return job_error(JobError::NotFound);
    }
    job_response(StatusCode::OK, &job, false)
}

fn job_response(status: StatusCode, job: &crate::jobs::AdminJob, replay: bool) -> Response {
    (
        status,
        Json(serde_json::json!({
            "job_id": job.job_id,
            "status": format!("{:?}", job.status).to_lowercase(),
            "scope": if job.global { "global" } else { "tenant" },
            "idempotent_replay": replay,
        })),
    )
        .into_response()
}

fn job_error(error: JobError) -> Response {
    match error {
        JobError::NotFound => {
            error_response(StatusCode::NOT_FOUND, "not_found", "Admin job not found")
        }
        JobError::IdempotencyConflict => error_response(
            StatusCode::CONFLICT,
            "idempotency_conflict",
            "Idempotency key is already bound to another request",
        ),
        JobError::RateLimited => error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Rebuild request rate limit exceeded",
        ),
        JobError::SeparationOfDuties => error_response(
            StatusCode::FORBIDDEN,
            "separation_of_duties",
            "Requester cannot approve this job",
        ),
        JobError::UnsafeClear => error_response(
            StatusCode::NOT_IMPLEMENTED,
            "clear_unavailable",
            "Clear remains disabled without verifiable Quickwit task completion",
        ),
        JobError::InvalidState => error_response(
            StatusCode::CONFLICT,
            "invalid_job_state",
            "Admin job state transition rejected",
        ),
        JobError::Storage | JobError::Execution => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "admin_job_unavailable",
            "Admin job service unavailable",
        ),
    }
}

fn auth_error(error: AuthError) -> Response {
    match error {
        AuthError::Unauthorized => error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Verified bearer credential required",
        ),
        AuthError::Forbidden => error_response(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Credential is not authorized for this operation",
        ),
        AuthError::BadRequest => error_response(
            StatusCode::BAD_REQUEST,
            "invalid_admin_request",
            "Approval, idempotency, and reason are required for mutations",
        ),
    }
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": {"code": code, "message": message}
        })),
    )
        .into_response()
}
