//! Per-run results on the edge (INNH-12): the pages a crawl, a batch or any other durable
//! job fetched, the Apify dataset equivalent.
//!
//! A row is read from control's `page_fetched` events: the page URL, closed fetch metadata
//! and the ids of the stored artifacts. The content itself is not returned; a client reads
//! it through the existing org-bound `GET /v1/artifacts/{id}`.
//!
//! Dark by default: `QUARRY_EDGE__JOB_RESULTS` must be on, otherwise every route answers 501
//! before control is called. The rows carry page URLs, so a caller under Zero Data Retention
//! gets a 403 (the same floor as the errors routes), and a service principal is refused by
//! `auth::require_service_route_scope`, which does not name these paths. The caller's org
//! comes from the verified token; control answers a foreign or unknown id with 404, and a
//! kind mismatch (a batch id on the crawl path) is a 404 too.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Extension, Json,
};

use quarry_core::envelope::{Envelope, PageMeta};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::resources::JobResultsView;

use crate::auth::Claims;
use crate::control_client;
use crate::job_lifecycle_routes::{fail, require_job_of_kind};
use crate::state::AppState;

/// Turns the results routes on. Off (the default), every route answers 501.
pub const ENV_JOB_RESULTS: &str = "QUARRY_EDGE__JOB_RESULTS";

/// Default and largest page of results.
pub const DEFAULT_LIMIT: u32 = 100;
pub const MAX_LIMIT: u32 = 500;

/// Whether a flag value switches the feature on.
pub fn flag_is_on(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

/// Whether the results routes are enabled. Read per request, so a flip needs no restart.
pub fn job_results_enabled() -> bool {
    flag_is_on(std::env::var(ENV_JOB_RESULTS).ok().as_deref())
}

/// The checks every results read makes before control is asked: the flag first (a dark
/// feature answers 501 whoever asks), then the ZDR floor.
pub fn gate(enabled: bool, zdr_active: bool) -> Result<(), QuarryError> {
    if !enabled {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            format!(
                "job results are not enabled on this edge: set {ENV_JOB_RESULTS}=1 \
                 (see docs/RUNBOOKS.md, \"Job results\")"
            ),
        ));
    }
    if zdr_active {
        // A row is a page URL and its title; a ZDR caller gets neither.
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "job results are not available under zero data retention",
        )
        .with_details(serde_json::json!({ "reason": "job_result_urls" })));
    }
    Ok(())
}

/// The query of a results read, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultsParams {
    pub after_seq: Option<u64>,
    pub limit: u32,
    pub latest: bool,
}

fn bad(field: &str, message: &str) -> QuarryError {
    QuarryError::new(ErrorCode::BadRequest, message)
        .with_details(serde_json::json!({ "field": field }))
}

/// Parses `after_seq`, `limit` (1..=500, default 100) and `latest`. A bad value is a 400
/// that names the field, never a silent default.
pub fn parse_params(query: &HashMap<String, String>) -> Result<ResultsParams, QuarryError> {
    let after_seq = match query.get("after_seq") {
        None => None,
        Some(raw) => Some(
            raw.parse::<u64>()
                .map_err(|_| bad("after_seq", "after_seq must be a non-negative integer"))?,
        ),
    };
    let limit = match query.get("limit") {
        None => DEFAULT_LIMIT,
        Some(raw) => match raw.parse::<u32>() {
            Ok(n) if (1..=MAX_LIMIT).contains(&n) => n,
            _ => return Err(bad("limit", "limit must be an integer from 1 to 500")),
        },
    };
    let latest = match query.get("latest").map(String::as_str) {
        None | Some("false" | "0") => false,
        Some("true" | "1") => true,
        Some(_) => return Err(bad("latest", "latest must be true or false")),
    };
    Ok(ResultsParams {
        after_seq,
        limit,
        latest,
    })
}

/// The results of a job, checked against the kind the path names (`None`: any kind).
async fn results_for(
    state: &AppState,
    claims: &Claims,
    id: &str,
    kind: Option<&str>,
    query: &HashMap<String, String>,
) -> Result<(JobResultsView, u32), QuarryError> {
    gate(job_results_enabled(), claims.effective_zdr(false))?;
    let params = parse_params(query)?;
    // The summary settles existence, org and kind before the rows are read.
    require_job_of_kind(state, claims, id, kind).await?;
    let mut forward: Vec<(&str, String)> = vec![("limit", params.limit.to_string())];
    if let Some(after) = params.after_seq {
        forward.push(("after_seq", after.to_string()));
    }
    if params.latest {
        forward.push(("latest", "true".to_string()));
    }
    let raw = control_client::get_json(
        state,
        &format!("/v1/jobs/{id}/results"),
        &claims.org_id,
        &forward,
    )
    .await?;
    let view: JobResultsView = serde_json::from_value(raw)
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "control results unreadable"))?;
    Ok((view, params.limit))
}

/// Renders the view as `{data: {items, next_seq?}, meta: {request_id, page: {next?, limit}}}`.
fn render(request_id: String, result: Result<(JobResultsView, u32), QuarryError>) -> Response {
    match result {
        Ok((view, limit)) => {
            let next = view.next_seq.map(|seq| seq.to_string());
            let mut envelope = Envelope::ok(request_id, view);
            envelope.meta.page = Some(PageMeta {
                cursor: None,
                next,
                limit,
            });
            Json(envelope).into_response()
        }
        Err(err) => fail(&request_id, err).into_response(),
    }
}

/// `GET /v1/jobs/:id/results`.
pub async fn job_results(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let request_id = crate::request_id::current();
    render(
        request_id,
        results_for(&state, &claims, &id, None, &query).await,
    )
}

/// `GET /v1/crawl/:id/results`.
pub async fn crawl_results(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let request_id = crate::request_id::current();
    render(
        request_id,
        results_for(&state, &claims, &id, Some("crawl"), &query).await,
    )
}

/// `GET /v1/batch/:id/results`.
pub async fn batch_results(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let request_id = crate::request_id::current();
    render(
        request_id,
        results_for(&state, &claims, &id, Some("batch"), &query).await,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn defaults_are_100_rows_from_the_start_with_every_fetch() {
        let p = parse_params(&q(&[])).unwrap();
        assert_eq!(
            p,
            ResultsParams {
                after_seq: None,
                limit: 100,
                latest: false
            }
        );
    }

    #[test]
    fn limit_must_be_1_to_500() {
        assert_eq!(parse_params(&q(&[("limit", "1")])).unwrap().limit, 1);
        assert_eq!(parse_params(&q(&[("limit", "500")])).unwrap().limit, 500);
        for bad_limit in ["0", "501", "-1", "abc", ""] {
            let err = parse_params(&q(&[("limit", bad_limit)])).unwrap_err();
            assert_eq!(err.code, ErrorCode::BadRequest, "limit={bad_limit:?}");
            assert_eq!(err.details.as_ref().unwrap()["field"], "limit");
        }
    }

    #[test]
    fn after_seq_and_latest_are_checked() {
        let p = parse_params(&q(&[("after_seq", "42"), ("latest", "true")])).unwrap();
        assert_eq!(p.after_seq, Some(42));
        assert!(p.latest);
        assert!(!parse_params(&q(&[("latest", "0")])).unwrap().latest);
        assert_eq!(
            parse_params(&q(&[("after_seq", "x")])).unwrap_err().code,
            ErrorCode::BadRequest
        );
        assert_eq!(
            parse_params(&q(&[("latest", "maybe")])).unwrap_err().code,
            ErrorCode::BadRequest
        );
    }

    #[test]
    fn the_flag_is_off_unless_set_to_a_true_word() {
        assert!(!flag_is_on(None));
        assert!(!flag_is_on(Some("")));
        assert!(!flag_is_on(Some("0")));
        assert!(flag_is_on(Some("1")));
        assert!(flag_is_on(Some(" true ")));
    }

    #[test]
    fn off_is_501_and_wins_over_the_zdr_refusal() {
        let err = gate(false, true).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unsupported);
        assert_eq!(err.code.http_status(), 501);
    }

    #[test]
    fn zdr_is_403_with_the_reason() {
        let err = gate(true, true).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert_eq!(err.details.as_ref().unwrap()["reason"], "job_result_urls");
        assert!(gate(true, false).is_ok());
    }
}
