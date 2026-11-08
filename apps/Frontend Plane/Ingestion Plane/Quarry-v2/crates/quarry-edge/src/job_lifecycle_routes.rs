//! Job lifecycle reads on the edge (WS15.L1): the status, errors and ongoing
//! views of a crawl, a batch or any other durable job.
//!
//! Every read is org-scoped by the verified token (never by a query parameter):
//! control answers a foreign id with 404, and this file passes that through.
//! Kind checks reject a crawl id on the batch path and the reverse, so a status
//! document always matches its path. A service principal cannot reach these
//! routes at all: `auth::require_service_route_scope` answers 403 for every path
//! it does not name, and these paths are not named.

use std::time::{Duration, Instant};

use axum::{
    extract::{Path, Query, RawQuery, State},
    http::{header, HeaderValue, StatusCode},
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

pub(crate) fn fail(request_id: &str, err: QuarryError) -> (StatusCode, Json<Envelope<()>>) {
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

/// Settles that the job exists for the caller's org and is of the kind the path names
/// (`None`: any kind). A foreign, unknown or mismatched id is the same 404.
pub(crate) async fn require_job_of_kind(
    state: &AppState,
    claims: &Claims,
    id: &str,
    kind: Option<&str>,
) -> Result<(), QuarryError> {
    let summary = load_summary(state, claims, id).await?;
    if kind.is_some_and(|kind| summary.job.kind != kind) {
        return Err(QuarryError::new(ErrorCode::NotFound, "not found"));
    }
    Ok(())
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

/// `GET /v1/jobs/:id`: the status of any job of the org, with the optional wait
/// of E3.7.3 (see `read_or_wait`).
pub async fn get_job(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Response {
    read_or_wait(&state, &claims, &id, None, raw.as_deref()).await
}

/// `GET /v1/crawl/:id`. `GET /v1/crawl/jobs` is the job list: the static-versus-
/// parameter match can route it here, so the list runs in that case.
pub async fn get_crawl(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<ListQuery>,
    RawQuery(raw): RawQuery,
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
    read_or_wait(&state, &claims, &id, Some("crawl"), raw.as_deref()).await
}

/// `GET /v1/batch/:id`, with the optional wait of E3.7.3.
pub async fn get_batch(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Response {
    read_or_wait(&state, &claims, &id, Some("batch"), raw.as_deref()).await
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The production half of this file: everything before the test module.
    fn production_source() -> &'static str {
        let source = include_str!("job_lifecycle_routes.rs");
        let cut = source.find("#[cfg(test)]").expect("test module marker");
        &source[..cut]
    }

    /// Every string literal that starts with `/v1/` in the production half.
    fn control_paths() -> Vec<String> {
        let mut found = Vec::new();
        for part in production_source().split('"') {
            if part.starts_with("/v1/") {
                found.push(part.to_string());
            }
        }
        found
    }

    /// D2-050: the edge signs its calls to control with the edge secret, and control
    /// verifies `/v1/internal/jobs/*` and `/v1/internal/schedules/*` with a different one,
    /// held by the orchestrator only. So no lifecycle call may target those prefixes:
    /// the edge has no business there, and a call that did would be refused with a 401
    /// that the edge maps to 403.
    #[test]
    fn lifecycle_calls_stay_out_of_the_orchestrator_only_group() {
        let paths = control_paths();
        assert!(
            paths.len() >= 4,
            "the scan found only {} control paths: {paths:?}",
            paths.len()
        );
        for path in &paths {
            assert!(
                !path.starts_with("/v1/internal/jobs/") && !path.starts_with("/v1/internal/schedules/"),
                "lifecycle call to an orchestrator-only control route: {path}"
            );
        }
    }

    /// The paths the lifecycle routes do call: org-scoped job resources, which control
    /// verifies with the edge secret.
    #[test]
    fn lifecycle_calls_use_the_org_scoped_job_resources() {
        let paths = control_paths();
        for expected in [
            "/v1/jobs/{id}/summary",
            "/v1/jobs/{id}/errors",
            "/v1/jobs/ongoing",
            "/v1/jobs/{id}/control",
        ] {
            assert!(
                paths.iter().any(|path| path == expected),
                "expected a call to {expected}; found {paths:?}"
            );
        }
    }

    /// A control refusal (the wrong secret answers 401, a role problem 403) reaches the
    /// caller as 403 Forbidden, never as the caller's own 401 and never with control's body.
    #[test]
    fn a_control_auth_refusal_is_a_forbidden_not_a_401() {
        for status in [reqwest::StatusCode::UNAUTHORIZED, reqwest::StatusCode::FORBIDDEN] {
            let err = control_client::map_status(status);
            assert_eq!(err.code, ErrorCode::Forbidden, "status {status}");
            assert_eq!(err.message, "control refused the request");
        }
    }

    /// The terminal statuses control can report, and only those, mark a status document terminal.
    #[test]
    fn terminal_statuses_are_the_five_the_wire_names() {
        for status in ["completed", "failed", "cancelled", "timed_out", "skipped"] {
            assert!(is_terminal(status), "{status}");
        }
        for status in ["accepted", "running", "paused"] {
            assert!(!is_terminal(status), "{status}");
        }
    }

    /// E3.7.3: the wait keys of a raw query. Other keys are ignored and `wait` is clamped.
    #[test]
    fn wait_query_reads_the_wait_and_clamps_it_to_the_cap() {
        assert_eq!(parse_wait_params(None).unwrap(), WaitParams::default());
        assert_eq!(parse_wait_params(Some("")).unwrap().wait_s, 0);
        assert_eq!(parse_wait_params(Some("wait=0")).unwrap().wait_s, 0);
        assert_eq!(parse_wait_params(Some("wait=20")).unwrap().wait_s, 20);
        assert_eq!(
            parse_wait_params(Some("wait=500")).unwrap().wait_s,
            WAIT_MAX_S
        );
        assert_eq!(parse_wait_params(Some("limit=5&wait=3")).unwrap().wait_s, 3);
        let params = parse_wait_params(Some("wait=4&resume=abc")).unwrap();
        assert_eq!(params.wait_s, 4);
        assert_eq!(params.resume.as_deref(), Some("abc"));
    }

    /// E3.7.3: a `wait` that is not a whole number, and an empty `resume`, are 400s.
    #[test]
    fn wait_query_refuses_a_value_that_is_not_a_number_and_an_empty_resume() {
        for raw in ["wait=", "wait=soon", "wait=-1", "resume="] {
            let err = parse_wait_params(Some(raw)).unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest, "{raw}");
        }
    }

    fn sample_marker(completed: u64) -> ProgressMarker {
        ProgressMarker {
            status: "running".to_string(),
            phase: "crawling".to_string(),
            completed,
            failed: 0,
            blocked: 0,
            retried: 0,
            desired_seq: 0,
            applied_seq: 0,
        }
    }

    /// E3.7.3: a resume token round-trips in URL-safe characters with no padding, and
    /// anything that is not a token is a 400.
    #[test]
    fn resume_token_round_trips_and_a_stranger_is_a_bad_request() {
        let token = ResumeToken {
            job_id: "job_1".to_string(),
            marker: sample_marker(4),
        };
        let raw = token.encode();
        assert!(
            raw.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{raw}"
        );
        assert_eq!(ResumeToken::decode(&raw).unwrap(), token);
        for bad in ["", "not a token!", "e30"] {
            let err = ResumeToken::decode(bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest, "{bad}");
        }
    }

    /// E3.7.3: a token names one job. A token for another job is a 400, and no token is no resume.
    #[test]
    fn a_resume_token_for_another_job_is_refused() {
        assert!(resume_for_job(None, "job_1").unwrap().is_none());
        let raw = ResumeToken {
            job_id: "job_1".to_string(),
            marker: sample_marker(0),
        }
        .encode();
        assert!(resume_for_job(Some(raw.as_str()), "job_1")
            .unwrap()
            .is_some());
        let err = resume_for_job(Some(raw.as_str()), "job_2").unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    /// E3.7.3: the progress marker is read field by field from control's summary.
    #[test]
    fn progress_marker_is_read_from_the_control_summary() {
        let summary: ControlSummary = serde_json::from_value(serde_json::json!({
            "job": {
                "id": "job_1",
                "kind": "crawl",
                "status": "running",
                "created_at": "2026-11-03T10:00:00Z",
                "desired_seq": 2,
                "applied_seq": 1
            },
            "phase": "crawling",
            "counts": { "completed": 4, "failed": 1, "blocked": 0, "retried": 2 },
            "total": 10,
            "total_kind": "budget"
        }))
        .unwrap();
        let marker = progress_marker(&summary);
        assert_eq!(marker.status, "running");
        assert_eq!(marker.phase, "crawling");
        assert_eq!((marker.completed, marker.failed, marker.retried), (4, 1, 2));
        assert_eq!((marker.desired_seq, marker.applied_seq), (2, 1));
    }

    /// E3.7.3: the 408 says whether the job moved since the caller's token; no token, no answer.
    #[test]
    fn progressed_says_whether_the_job_moved_since_the_token() {
        let marker = sample_marker(4);
        assert_eq!(progressed_since(None, &marker), None);
        let token = ResumeToken {
            job_id: "job_1".to_string(),
            marker: marker.clone(),
        };
        assert_eq!(progressed_since(Some(&token), &marker), Some(false));
        assert_eq!(
            progressed_since(Some(&token), &sample_marker(5)),
            Some(true)
        );
    }

    /// E3.7.3: a wait is refused to a ZDR caller with a typed 403; a ZDR-free caller is not refused.
    #[test]
    fn a_wait_under_zdr_is_forbidden() {
        assert!(refuse_wait_under_zdr(false).is_ok());
        let err = refuse_wait_under_zdr(true).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    /// E3.7.3: a job still processing when the wait runs out answers 408 with the
    /// processing_continues reason, a resume token for that job, and Retry-After.
    #[tokio::test]
    async fn processing_continues_is_a_408_with_a_resume_token_and_retry_after() {
        let view = JobStatusView {
            job_id: "job_1".to_string(),
            kind: "crawl".to_string(),
            status: "running".to_string(),
            phase: "crawling".to_string(),
            terminal: false,
            origin: None,
            created_at: Utc::now(),
            finished_at: None,
            deadline_at: None,
            counts: JobCounts {
                completed: 4,
                failed: 0,
                blocked: 0,
                retried: 0,
            },
            total: None,
            total_kind: None,
            warnings: Vec::new(),
            control: JobControlState::default(),
            schedule_id: None,
            run_id: None,
            credits: None,
        };
        let response = processing_continues(
            "req_1".to_string(),
            "job_1",
            view,
            sample_marker(4),
            Some(true),
            25,
        );
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(
            response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            Some("1")
        );
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "TIMEOUT");
        assert_eq!(body["error"]["details"]["reason"], "processing_continues");
        assert_eq!(body["error"]["details"]["progressed"], true);
        assert_eq!(body["error"]["details"]["waited_s"], 25);
        assert_eq!(body["error"]["details"]["counts"]["completed"], 4);
        let raw = body["error"]["details"]["resume_token"].as_str().unwrap();
        let token = ResumeToken::decode(raw).unwrap();
        assert_eq!(token.job_id, "job_1");
        assert_eq!(token.marker, sample_marker(4));
    }
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

// ---- E3.7.3: the wait on a job, and the 408 processing_continues envelope ----------
//
// `GET /v1/jobs/:id`, `GET /v1/crawl/:id` and `GET /v1/batch/:id` take an optional
// `wait` (whole seconds) and, after a 408, a `resume` token. The wait belongs to the
// job control surface: it is ignored unless QUARRY_EDGE__JOB_CONTROL is on, so with
// the flag off each read is the WS15.L1 read exactly. A ZDR caller is refused a wait
// (403), because the durable path a wait stands on is refused under ZDR (routes.rs).
// Cancel, pause and resume are the L2 verbs above and are not changed here.

/// The longest one read waits, in seconds. Below the router's 60 s path deadline, so
/// a wait ends in an answer rather than in the router's bare 408.
pub const WAIT_MAX_S: u64 = 25;

/// How often a waiting read asks control for the job. Matches the orchestrator's
/// control loop period (every 2 s), so a terminal state is seen at most 2 s late.
const WAIT_POLL: Duration = Duration::from_secs(2);

/// The `Retry-After` of the 408. A resume may follow at once, so one second is the
/// least a caller should wait.
const RETRY_AFTER_S: u64 = 1;

/// The wait keys of a status read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WaitParams {
    /// Whole seconds to wait for a terminal status; 0 is a plain read.
    pub wait_s: u64,
    /// The `resume_token` of an earlier 408, sent back exactly as it came.
    pub resume: Option<String>,
}

/// Reads `wait` and `resume` from a raw query string. Other keys are ignored, as the
/// list routes ignore keys they do not name. A `wait` above the cap is clamped to it.
pub fn parse_wait_params(raw: Option<&str>) -> Result<WaitParams, QuarryError> {
    let mut params = WaitParams::default();
    for pair in raw.unwrap_or("").split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "wait" => {
                let seconds: u64 = value.parse().map_err(|_| {
                    QuarryError::new(
                        ErrorCode::BadRequest,
                        "wait must be a whole number of seconds",
                    )
                })?;
                params.wait_s = seconds.min(WAIT_MAX_S);
            }
            "resume" => {
                if value.is_empty() {
                    return Err(QuarryError::new(
                        ErrorCode::BadRequest,
                        "resume must carry the resume_token of a 408",
                    ));
                }
                params.resume = Some(value.to_string());
            }
            _ => {}
        }
    }
    Ok(params)
}

/// A job's progress at one instant, as control's summary shows it. A resume token
/// carries it, so the next wait can say whether the job moved in between.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressMarker {
    pub status: String,
    pub phase: String,
    pub completed: u64,
    pub failed: u64,
    pub blocked: u64,
    pub retried: u64,
    pub desired_seq: i64,
    pub applied_seq: i64,
}

/// The token of a 408. It names the job it was issued for and carries the progress
/// marker. It is not a credential: it holds a job id and counts. The job id must match
/// the path, so a forged token can misreport only the `progressed` flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeToken {
    pub job_id: String,
    pub marker: ProgressMarker,
}

impl ResumeToken {
    /// URL-safe base64 of the token's JSON, without padding, so it is a query value as is.
    pub fn encode(&self) -> String {
        use base64::Engine as _;
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    /// Reads a token back. Anything that is not a token is a 400.
    pub fn decode(raw: &str) -> Result<Self, QuarryError> {
        use base64::Engine as _;
        let invalid = || QuarryError::new(ErrorCode::BadRequest, "resume token is not valid");
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(raw)
            .map_err(|_| invalid())?;
        serde_json::from_slice::<Self>(&bytes).map_err(|_| invalid())
    }
}

/// The token of a resume, checked against the job the path names.
fn resume_for_job(raw: Option<&str>, job_id: &str) -> Result<Option<ResumeToken>, QuarryError> {
    match raw {
        None => Ok(None),
        Some(raw) => {
            let token = ResumeToken::decode(raw)?;
            if token.job_id != job_id {
                return Err(QuarryError::new(
                    ErrorCode::BadRequest,
                    "resume token is for another job",
                ));
            }
            Ok(Some(token))
        }
    }
}

/// Whether the job moved since the caller's token was issued. `None` without a token.
fn progressed_since(resume: Option<&ResumeToken>, marker: &ProgressMarker) -> Option<bool> {
    resume.map(|token| token.marker != *marker)
}

/// A job's progress marker, read from control's summary.
fn progress_marker(summary: &ControlSummary) -> ProgressMarker {
    ProgressMarker {
        status: summary.job.status.clone(),
        phase: summary.phase.clone(),
        completed: summary.counts.completed,
        failed: summary.counts.failed,
        blocked: summary.counts.blocked,
        retried: summary.counts.retried,
        desired_seq: summary.job.desired_seq,
        applied_seq: summary.job.applied_seq,
    }
}

/// A wait stands on the durable-job path, which is refused under ZDR. A ZDR caller is
/// refused the wait the way the error reads are refused (403, typed, no content).
fn refuse_wait_under_zdr(zdr: bool) -> Result<(), QuarryError> {
    if zdr {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "waiting on a job is not available under zero data retention",
        ));
    }
    Ok(())
}

/// What a wait came back with: the job reached a terminal status, or the wait ran out.
enum Awaited {
    Settled(JobStatusView),
    Processing {
        view: JobStatusView,
        marker: ProgressMarker,
        progressed: Option<bool>,
    },
}

/// Polls control until the job is terminal or `wait_s` seconds have passed. The kind
/// check is the one `status_for` makes, and a foreign or unknown id is a 404 at every
/// poll, so a wait cannot be used to probe another organization's job.
async fn await_job(
    state: &AppState,
    claims: &Claims,
    id: &str,
    kind: Option<&str>,
    wait_s: u64,
    resume: Option<&ResumeToken>,
) -> Result<Awaited, QuarryError> {
    let deadline = Instant::now() + Duration::from_secs(wait_s);
    loop {
        let summary = load_summary(state, claims, id).await?;
        if kind.is_some_and(|kind| summary.job.kind != kind) {
            return Err(QuarryError::new(ErrorCode::NotFound, "not found"));
        }
        let marker = progress_marker(&summary);
        let view = status_view(summary);
        if view.terminal {
            return Ok(Awaited::Settled(view));
        }
        let now = Instant::now();
        if now >= deadline {
            let progressed = progressed_since(resume, &marker);
            return Ok(Awaited::Processing {
                view,
                marker,
                progressed,
            });
        }
        tokio::time::sleep(WAIT_POLL.min(deadline - now)).await;
    }
}

/// The 408 envelope: the job is still processing when the wait runs out. The body
/// carries the token to send back with `resume`, and the `Retry-After` header says when.
fn processing_continues(
    request_id: String,
    job_id: &str,
    view: JobStatusView,
    marker: ProgressMarker,
    progressed: Option<bool>,
    waited_s: u64,
) -> Response {
    let token = ResumeToken {
        job_id: job_id.to_string(),
        marker,
    }
    .encode();
    let details = serde_json::json!({
        "reason": "processing_continues",
        "job_id": view.job_id,
        "status": view.status,
        "phase": view.phase,
        "counts": view.counts,
        "progressed": progressed,
        "waited_s": waited_s,
        "retry_after_seconds": RETRY_AFTER_S,
        "resume_token": token,
    });
    let err = QuarryError::new(
        ErrorCode::Timeout,
        "the job is still processing; send resume_token back with resume to keep waiting",
    )
    .with_details(details);
    let mut response = (
        StatusCode::REQUEST_TIMEOUT,
        Json(Envelope::<()>::err(request_id, err)),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&RETRY_AFTER_S.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

/// The status read of the three job GET routes, with the wait. With the flag off the
/// wait keys are ignored and this is the WS15.L1 read.
async fn read_or_wait(
    state: &AppState,
    claims: &Claims,
    id: &str,
    kind: Option<&str>,
    raw_query: Option<&str>,
) -> Response {
    let request_id = crate::request_id::current();
    if !job_control_enabled() {
        return respond(request_id, status_for(state, claims, id, kind).await);
    }
    let params = match parse_wait_params(raw_query) {
        Ok(params) => params,
        Err(err) => return fail(&request_id, err).into_response(),
    };
    if params.wait_s == 0 {
        if params.resume.is_some() {
            let err = QuarryError::new(
                ErrorCode::BadRequest,
                "resume needs a wait of at least 1 second",
            );
            return fail(&request_id, err).into_response();
        }
        return respond(request_id, status_for(state, claims, id, kind).await);
    }
    if let Err(err) = refuse_wait_under_zdr(claims.effective_zdr(false)) {
        return fail(&request_id, err).into_response();
    }
    let resume = match resume_for_job(params.resume.as_deref(), id) {
        Ok(resume) => resume,
        Err(err) => return fail(&request_id, err).into_response(),
    };
    match await_job(state, claims, id, kind, params.wait_s, resume.as_ref()).await {
        Ok(Awaited::Settled(view)) => respond(request_id, Ok(view)),
        Ok(Awaited::Processing {
            view,
            marker,
            progressed,
        }) => processing_continues(request_id, id, view, marker, progressed, params.wait_s),
        Err(err) => fail(&request_id, err).into_response(),
    }
}
