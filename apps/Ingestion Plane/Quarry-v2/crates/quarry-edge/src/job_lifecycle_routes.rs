//! Job lifecycle reads on the edge (WS15.L1): the status, errors and ongoing
//! views of a crawl, a batch or any other durable job.
//!
//! Every read is org-scoped by the verified token (never by a query parameter):
//! control answers a foreign id with 404, and this file passes that through.
//! Kind checks reject a crawl id on the batch path and the reverse, so a status
//! document always matches its path. A service principal cannot reach these
//! routes at all: `auth::require_service_route_scope` answers 403 for every path
//! it does not name, and these paths are not named.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::JobKind;
use quarry_core::pagination::Page;
use quarry_core::resources::{
    JobControlState, JobCounts, JobCredits, JobErrorsView, JobStatusView, JobSummary, JobWarning,
    RunOrigin,
};

use crate::auth::Claims;
use crate::control_client;
use crate::resource_routes::{list_jobs, ListQuery};
use crate::state::AppState;

/// Query of the errors routes: `after_seq` pages by event seq, `limit` is capped
/// at 500 (the control default is 100).
#[derive(Debug, Default, Deserialize)]
pub struct ErrorsQuery {
    #[serde(default)]
    pub after_seq: Option<u64>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Control's job record as the edge reads it. Only the fields the status view
/// needs; everything else control sends is ignored.
#[derive(Debug, Deserialize)]
struct ControlJob {
    id: String,
    kind: String,
    status: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    origin: Option<RunOrigin>,
    #[serde(default)]
    desired_state: Option<String>,
    #[serde(default)]
    desired_seq: i64,
    #[serde(default)]
    applied_seq: i64,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    schedule_id: Option<String>,
    #[serde(default)]
    deadline_at: Option<DateTime<Utc>>,
    #[serde(default)]
    finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default, Deserialize)]
struct ControlCounts {
    #[serde(default)]
    completed: u64,
    #[serde(default)]
    failed: u64,
    #[serde(default)]
    blocked: u64,
    #[serde(default)]
    retried: u64,
}

/// Control's `GET /v1/jobs/{id}/summary`.
#[derive(Debug, Deserialize)]
struct ControlSummary {
    job: ControlJob,
    phase: String,
    #[serde(default)]
    counts: ControlCounts,
    #[serde(default)]
    total: u64,
    #[serde(default)]
    total_kind: Option<String>,
    #[serde(default)]
    warnings: Vec<JobWarning>,
    /// Control's derived credit estimate (INNH-9); present only while control's
    /// `QUARRY_CONTROL__JOB_CREDITS` flag is on. Forwarded unchanged: the edge
    /// holds no price and no flag of its own.
    #[serde(default)]
    credits: Option<JobCredits>,
}

fn is_terminal(status: &str) -> bool {
    matches!(
        status,
        "completed" | "failed" | "cancelled" | "timed_out" | "skipped"
    )
}

/// The public status document of one job, from control's summary.
fn status_view(summary: ControlSummary) -> JobStatusView {
    let ControlSummary {
        job,
        phase,
        counts,
        total,
        total_kind,
        warnings,
        credits,
    } = summary;
    let terminal = is_terminal(&job.status);
    let control = JobControlState {
        desired: job.desired_state.filter(|d| !d.is_empty()),
        pending: job.desired_seq > job.applied_seq,
    };
    JobStatusView {
        job_id: job.id,
        kind: job.kind,
        status: job.status,
        phase,
        terminal,
        origin: job.origin,
        created_at: job.created_at,
        finished_at: job.finished_at,
        deadline_at: job.deadline_at,
        counts: JobCounts {
            completed: counts.completed,
            failed: counts.failed,
            blocked: counts.blocked,
            retried: counts.retried,
        },
        total: total_kind.as_ref().map(|_| total),
        total_kind,
        warnings,
        control,
        schedule_id: job.schedule_id,
        run_id: job.run_id,
        credits,
    }
}

fn fail(request_id: &str, err: QuarryError) -> (StatusCode, Json<Envelope<()>>) {
    (
        StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(Envelope::<()>::err(request_id, err)),
    )
}

/// Renders a handler result as the `{data}` envelope or the typed error.
fn respond<T: Serialize>(request_id: String, result: Result<T, QuarryError>) -> Response {
    match result {
        Ok(value) => Json(Envelope::ok(request_id, value)).into_response(),
        Err(err) => fail(&request_id, err).into_response(),
    }
}

/// Reads control's summary of one job. An id that is not a job id is a 404
/// without a control call, so the id never reaches control unparsed.
async fn load_summary(state: &AppState, claims: &Claims, id: &str) -> Result<ControlSummary, QuarryError> {
    if id.parse::<JobKind>().is_err() {
        return Err(QuarryError::new(ErrorCode::NotFound, "not found"));
    }
    let raw = control_client::get_json(
        state,
        &format!("/v1/jobs/{id}/summary"),
        &claims.org_id,
        &[],
    )
    .await?;
    serde_json::from_value(raw)
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "control summary unreadable"))
}

/// The status of a job, checked against the kind the path names.
async fn status_for(
    state: &AppState,
    claims: &Claims,
    id: &str,
    kind: Option<&str>,
) -> Result<JobStatusView, QuarryError> {
    let summary = load_summary(state, claims, id).await?;
    if let Some(kind) = kind {
        if summary.job.kind != kind {
            return Err(QuarryError::new(ErrorCode::NotFound, "not found"));
        }
    }
    Ok(status_view(summary))
}

/// The errors of a job, checked against the kind the path names. Under the
/// token's ZDR floor the URLs of failed pages are not returned at all.
async fn errors_for(
    state: &AppState,
    claims: &Claims,
    id: &str,
    kind: Option<&str>,
    query: ErrorsQuery,
) -> Result<JobErrorsView, QuarryError> {
    if claims.effective_zdr(false) {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "error URLs are not available under zero data retention",
        ));
    }
    // The summary check settles existence and kind before the errors are read.
    load_summary(state, claims, id).await.and_then(|summary| match kind {
        Some(kind) if summary.job.kind != kind => Err(QuarryError::new(ErrorCode::NotFound, "not found")),
        _ => Ok(()),
    })?;
    let mut params: Vec<(&str, String)> = Vec::new();
    if let Some(after) = query.after_seq {
        params.push(("after_seq", after.to_string()));
    }
    if let Some(limit) = query.limit {
        params.push(("limit", limit.clamp(1, 500).to_string()));
    }
    let raw = control_client::get_json(
        state,
        &format!("/v1/jobs/{id}/errors"),
        &claims.org_id,
        &params,
    )
    .await?;
    serde_json::from_value(raw)
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "control errors unreadable"))
}

/// The org's jobs still in flight, of one kind, oldest first.
async fn ongoing_for(state: &AppState, claims: &Claims, kind: &str) -> Result<Page<JobSummary>, QuarryError> {
    let raw = control_client::get_json(state, "/v1/jobs/ongoing", &claims.org_id, &[]).await?;
    let items: Vec<JobSummary> = serde_json::from_value(raw)
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "control ongoing list unreadable"))?;
    let items = items
        .into_iter()
        .filter(|job| job.kind.as_str() == kind)
        .collect();
    Ok(Page {
        items,
        next_cursor: None,
        total_estimated: None,
    })
}

/// `GET /v1/jobs/:id`: the status of any job of the org.
pub async fn get_job(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    let request_id = crate::request_id::current();
    respond(request_id, status_for(&state, &claims, &id, None).await)
}

/// `GET /v1/crawl/:id`. `GET /v1/crawl/jobs` is the job list: the static-versus-
/// parameter match can route it here, so the list runs in that case.
pub async fn get_crawl(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Response {
    if id == "jobs" {
        return match list_jobs(
            State(state),
            Extension(claims),
            Path("crawl".to_string()),
            Query(query),
        )
        .await
        {
            Ok(list) => list.into_response(),
            Err(err) => err.into_response(),
        };
    }
    let request_id = crate::request_id::current();
    respond(request_id, status_for(&state, &claims, &id, Some("crawl")).await)
}

/// `GET /v1/batch/:id`.
pub async fn get_batch(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    let request_id = crate::request_id::current();
    respond(request_id, status_for(&state, &claims, &id, Some("batch")).await)
}

/// `GET /v1/jobs/:id/errors`.
pub async fn job_errors(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<ErrorsQuery>,
) -> Response {
    let request_id = crate::request_id::current();
    respond(request_id, errors_for(&state, &claims, &id, None, query).await)
}

/// `GET /v1/crawl/:id/errors`.
pub async fn crawl_errors(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<ErrorsQuery>,
) -> Response {
    let request_id = crate::request_id::current();
    respond(request_id, errors_for(&state, &claims, &id, Some("crawl"), query).await)
}

/// `GET /v1/batch/:id/errors`.
pub async fn batch_errors(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<ErrorsQuery>,
) -> Response {
    let request_id = crate::request_id::current();
    respond(request_id, errors_for(&state, &claims, &id, Some("batch"), query).await)
}

/// `GET /v1/crawl/ongoing`: the org's crawls still in flight.
pub async fn ongoing_crawl(State(state): State<AppState>, Extension(claims): Extension<Claims>) -> Response {
    let request_id = crate::request_id::current();
    respond(request_id, ongoing_for(&state, &claims, "crawl").await)
}

/// `GET /v1/batch/ongoing`: the org's batches still in flight.
pub async fn ongoing_batch(State(state): State<AppState>, Extension(claims): Extension<Claims>) -> Response {
    let request_id = crate::request_id::current();
    respond(request_id, ongoing_for(&state, &claims, "batch").await)
}

// ---- Job control (WS15.L2) ----------------------------------------------------

/// Turns the control verbs on. Off, every verb answers 501 before control is called.
pub const ENV_JOB_CONTROL: &str = "QUARRY_EDGE__JOB_CONTROL";

/// Whether the control verbs are enabled. Read per request: it is one environment
/// variable and a control request is rare, so the read costs nothing.
pub fn job_control_enabled() -> bool {
    std::env::var(ENV_JOB_CONTROL)
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

/// A control verb as the caller names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlVerb {
    Pause,
    Resume,
    Cancel,
}

impl ControlVerb {
    /// The action word control records.
    fn action(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Cancel => "cancel",
        }
    }

    /// The phase a caller sees while the request is pending.
    fn pending_phase(self) -> &'static str {
        match self {
            Self::Pause => "pausing",
            Self::Resume => "resuming",
            Self::Cancel => "cancelling",
        }
    }
}

/// Who may control a job. One function, so a later rule (the creator only, or a
/// role) changes one place. Org-wide today. A service principal is refused here as
/// well as by the route scope in `auth::require_service_route_scope`.
pub fn authorize_control(claims: &Claims) -> Result<(), QuarryError> {
    if claims.is_service() {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "service principals cannot control jobs",
        ));
    }
    Ok(())
}

/// The answer to a control request: the phase after it and what was asked.
/// `control.pending` stays true until the orchestrator applies the request.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JobControlAccepted {
    pub job_id: String,
    pub status: String,
    pub phase: String,
    pub control: JobControlState,
}

/// The answer built from control's job record after a request was written.
fn accepted_view(job: ControlJob, verb: ControlVerb) -> JobControlAccepted {
    let pending = job.desired_seq > job.applied_seq;
    let phase = if is_terminal(&job.status) {
        job.status.clone()
    } else {
        verb.pending_phase().to_string()
    };
    JobControlAccepted {
        job_id: job.id,
        status: job.status,
        phase,
        control: JobControlState {
            desired: Some(verb.action().to_string()),
            pending,
        },
    }
}

/// Writes one control request to control. The job must exist for the org and be of
/// the kind the path names; control decides whether the state allows the verb.
async fn control_request(
    state: &AppState,
    claims: &Claims,
    id: &str,
    kind: &str,
    verb: ControlVerb,
) -> Result<JobControlAccepted, QuarryError> {
    if !job_control_enabled() {
        return Err(QuarryError::new(ErrorCode::Unsupported, "job control is not enabled"));
    }
    authorize_control(claims)?;
    let summary = load_summary(state, claims, id).await?;
    if summary.job.kind != kind {
        return Err(QuarryError::new(ErrorCode::NotFound, "not found"));
    }
    let body = serde_json::json!({
        "action": verb.action(),
        "actor": claims.actor_id(),
        "actor_type": if claims.is_service() { "service" } else { "user" },
    });
    let raw = control_client::send_json(
        state,
        reqwest::Method::POST,
        &format!("/v1/jobs/{id}/control"),
        &claims.org_id,
        &body,
        None,
    )
    .await?;
    let job: ControlJob = serde_json::from_value(raw)
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "control answer unreadable"))?;
    Ok(accepted_view(job, verb))
}

/// Renders a control request: 202 while the orchestrator has yet to apply it, 200
/// once the job's state already reflects it.
async fn control_verb(
    state: &AppState,
    claims: &Claims,
    id: &str,
    kind: &str,
    verb: ControlVerb,
) -> Response {
    let request_id = crate::request_id::current();
    match control_request(state, claims, id, kind, verb).await {
        Ok(view) => {
            let code = if view.control.pending {
                StatusCode::ACCEPTED
            } else {
                StatusCode::OK
            };
            (code, Json(Envelope::ok(request_id, view))).into_response()
        }
        Err(err) => fail(&request_id, err).into_response(),
    }
}

/// `DELETE /v1/crawl/:id`: cancel a crawl. An accepted crawl is cancelled at once;
/// a running one is cancelled within a page and the control loop's period.
pub async fn cancel_crawl(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    control_verb(&state, &claims, &id, "crawl", ControlVerb::Cancel).await
}

/// `POST /v1/crawl/:id/pause`.
pub async fn pause_crawl(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    control_verb(&state, &claims, &id, "crawl", ControlVerb::Pause).await
}

/// `POST /v1/crawl/:id/resume`.
pub async fn resume_crawl(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    control_verb(&state, &claims, &id, "crawl", ControlVerb::Resume).await
}

/// `DELETE /v1/batch/:id`: cancel a batch (the batch alias of `cancel_crawl`).
pub async fn cancel_batch(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    control_verb(&state, &claims, &id, "batch", ControlVerb::Cancel).await
}

/// `POST /v1/batch/:id/pause`.
pub async fn pause_batch(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    control_verb(&state, &claims, &id, "batch", ControlVerb::Pause).await
}

/// `POST /v1/batch/:id/resume`.
pub async fn resume_batch(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Response {
    control_verb(&state, &claims, &id, "batch", ControlVerb::Resume).await
}
