//! Job detail, per-URL errors, job control and team usage for the Innhenting page.
//!
//! Every route proxies quarry-edge with the caller's bearer token, so the org
//! scope comes from the session (never a client header). Quarry's snake_case
//! views are reshaped into the SPA's camelCase, the same way `runs.rs` reshapes
//! job rows. Job control is kind-specific upstream (`/v1/crawl/{id}/pause`,
//! `/v1/batch/{id}/resume`, `DELETE /v1/batch/{id}`), so the handler reads the
//! job's kind first and never guesses it from the id.

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

use super::shared::{
    array_at, cookie_header, first_str, forward, obj_or_empty, okay, quarry_call, quarry_token,
    str_at, str_or_null, validation,
};

/// The job kinds Quarry exposes pause/resume/cancel for. Scrape, search, extract
/// and agent runs finish inside one request, so there is nothing to control.
const CONTROLLABLE_KINDS: [&str; 2] = ["crawl", "batch"];
const CONTROL_ACTIONS: [&str; 3] = ["pause", "resume", "cancel"];

/// `GET /api/ingestions/jobs/{id}` — the status of one durable job.
pub(super) async fn get_job(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let Some(job_id) = valid_job_id(&id) else {
        return validation("A job id is required.");
    };
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &format!("/v1/jobs/{job_id}"),
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }
    okay(to_job_detail(&crate::envelope::unwrap_data(&body)))
}

/// `GET /api/ingestions/jobs/{id}/errors` — the failed and blocked URLs of a job,
/// with `?afterSeq=` and `?limit=` for paging. Quarry answers 403 under ZDR.
pub(super) async fn get_job_errors(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let Some(job_id) = valid_job_id(&id) else {
        return validation("A job id is required.");
    };
    let mut query: Vec<String> = Vec::new();
    if let Some(after) = params.get("afterSeq").and_then(|v| v.parse::<u64>().ok()) {
        query.push(format!("after_seq={after}"));
    }
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(1, 500))
        .unwrap_or(100);
    query.push(format!("limit={limit}"));

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &format!("/v1/jobs/{job_id}/errors?{}", query.join("&")),
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }
    okay(to_errors_view(&crate::envelope::unwrap_data(&body)))
}

/// `POST /api/ingestions/jobs/{id}/control` with `{ "action": "pause" | "resume" | "cancel" }`.
pub(super) async fn job_control(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let action = first_str(&body, &["action"]).unwrap_or_default();
    control_job(&state, &user, &headers, &id, &action).await
}

/// `GET /api/ingestions/usage?period=7d|30d|today|YYYY-MM-DD` — the org's credit
/// use and ceiling for a period, straight from Quarry's team usage read.
pub(super) async fn get_usage(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let period = params
        .get("period")
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .unwrap_or("7d")
        .to_owned();
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let path = format!(
        "/v1/team/credit-usage?period={}",
        urlencoding::encode(&period)
    );
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &path,
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }
    okay(to_usage_view(&crate::envelope::unwrap_data(&body)))
}

/// Pause, resume or cancel a crawl or batch job. Shared by the REST route above
/// and the `ingestions.job_control` action dispatcher, so both answer the same way.
pub(crate) async fn control_job(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    id: &str,
    action: &str,
) -> Response {
    let Some(job_id) = valid_job_id(id) else {
        return validation("A job id is required.");
    };
    if !CONTROL_ACTIONS.contains(&action) {
        return validation("Action must be pause, resume or cancel.");
    }
    let cookie = cookie_header(headers);
    let token = quarry_token(state, user, &cookie).await;
    let token = token.as_deref();
    let user_id = user.user_id.as_str();

    let (status, job) = quarry_call(
        state,
        Method::GET,
        &format!("/v1/jobs/{job_id}"),
        None,
        token,
        user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, job);
    }
    let kind = str_at(&crate::envelope::unwrap_data(&job), "kind");
    if !CONTROLLABLE_KINDS.contains(&kind.as_str()) {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "not_controllable",
                format!("{kind} jobs run to completion and cannot be {action}ed."),
            )),
        )
            .into_response();
    }

    let (method, path) = match action {
        "cancel" => (Method::DELETE, format!("/v1/{kind}/{job_id}")),
        verb => (Method::POST, format!("/v1/{kind}/{job_id}/{verb}")),
    };
    let (status, body) = quarry_call(state, method, &path, None, token, user_id).await;
    if !status.is_success() {
        return forward(status, body);
    }
    let data = crate::envelope::unwrap_data(&body);
    let accepted = json!({
        "id": str_at(&data, "job_id"),
        "status": str_at(&data, "status"),
        "phase": str_at(&data, "phase"),
        "control": {
            "desired": data.pointer("/control/desired").cloned().unwrap_or(Value::Null),
            "pending": data.pointer("/control/pending").and_then(Value::as_bool).unwrap_or(false),
        },
    });
    // Quarry answers 202 while the orchestrator has yet to apply the request.
    let code = if status == StatusCode::ACCEPTED {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    (code, Json(crate::envelope::ok(accepted))).into_response()
}

/// Accepts the `job_<ulid>` ids Quarry mints; anything else never reaches the edge.
fn valid_job_id(raw: &str) -> Option<String> {
    let id = raw.trim();
    let well_formed = id.starts_with("job_")
        && id.len() <= 80
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    well_formed.then(|| id.to_owned())
}

/// Reshape a Quarry `JobStatusView` into the SPA's job detail.
pub(super) fn to_job_detail(job: &Value) -> Value {
    let counts = obj_or_empty(job, "counts");
    let warnings: Vec<Value> = array_at(job, "warnings")
        .into_iter()
        .map(|w| json!({ "code": str_at(&w, "code"), "message": str_at(&w, "message") }))
        .collect();
    json!({
        "id": str_at(job, "job_id"),
        "kind": str_at(job, "kind"),
        "status": str_at(job, "status"),
        "phase": str_at(job, "phase"),
        "terminal": job.get("terminal").and_then(Value::as_bool).unwrap_or(false),
        "origin": job.get("origin").cloned().unwrap_or(Value::Null),
        "createdAt": str_at(job, "created_at"),
        "finishedAt": str_or_null(job, "finished_at"),
        "deadlineAt": str_or_null(job, "deadline_at"),
        "counts": {
            "completed": counts.get("completed").cloned().unwrap_or(json!(0)),
            "failed": counts.get("failed").cloned().unwrap_or(json!(0)),
            "blocked": counts.get("blocked").cloned().unwrap_or(json!(0)),
            "retried": counts.get("retried").cloned().unwrap_or(json!(0)),
        },
        "total": job.get("total").cloned().unwrap_or(Value::Null),
        "totalKind": str_or_null(job, "total_kind"),
        "warnings": warnings,
        "control": {
            "desired": job.pointer("/control/desired").cloned().unwrap_or(Value::Null),
            "pending": job.pointer("/control/pending").and_then(Value::as_bool).unwrap_or(false),
        },
        "scheduleId": str_or_null(job, "schedule_id"),
        "runId": str_or_null(job, "run_id"),
        "controllable": CONTROLLABLE_KINDS.contains(&str_at(job, "kind").as_str()),
    })
}

/// Reshape a Quarry `JobErrorsView`: failed pages, blocked pages and pages
/// robots.txt kept out, each with its URL and the code Quarry recorded.
pub(super) fn to_errors_view(view: &Value) -> Value {
    let errors: Vec<Value> = array_at(view, "errors")
        .into_iter()
        .map(|e| {
            json!({
                "id": str_at(&e, "id"),
                "ts": str_at(&e, "ts"),
                "url": str_or_null(&e, "url"),
                "code": str_or_null(&e, "code"),
                "category": str_or_null(&e, "category"),
                "retryable": e.get("retryable").and_then(Value::as_bool).unwrap_or(false),
                "status": e.get("status").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    json!({
        "errors": errors,
        "blocked": blocked_rows(view, "blocked"),
        "robotsBlocked": blocked_rows(view, "robots_blocked"),
        "nextSeq": view.get("next_seq").cloned().unwrap_or(Value::Null),
    })
}

fn blocked_rows(view: &Value, key: &str) -> Vec<Value> {
    array_at(view, key)
        .into_iter()
        .map(|b| {
            json!({
                "url": str_or_null(&b, "url"),
                "ts": str_at(&b, "ts"),
                "reason": str_or_null(&b, "reason"),
            })
        })
        .collect()
}

/// Reshape Quarry's `TeamCreditUsage`.
pub(super) fn to_usage_view(usage: &Value) -> Value {
    json!({
        "period": usage.get("period").cloned().unwrap_or(Value::Null),
        "creditsUsed": usage.get("credits_used").cloned().unwrap_or(json!(0.0)),
        "creditsLimit": usage.get("credits_limit").cloned().unwrap_or(Value::Null),
        "utilizationPercent": usage.get("utilization_percent").cloned().unwrap_or(json!(0.0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_ids_must_be_quarry_job_ids() {
        assert_eq!(valid_job_id(" job_01HZX9 ").as_deref(), Some("job_01HZX9"));
        assert!(valid_job_id("run_01HZX9").is_none());
        assert!(valid_job_id("job_../../x").is_none());
        assert!(valid_job_id("").is_none());
    }

    #[test]
    fn job_detail_is_camel_cased_and_keeps_origin_and_schedule() {
        let job = json!({
            "job_id": "job_1",
            "kind": "crawl",
            "status": "running",
            "phase": "crawling",
            "terminal": false,
            "origin": { "type": "schedule", "ref": "sch_1" },
            "created_at": "2026-10-28T10:00:00Z",
            "counts": { "completed": 4, "failed": 1 },
            "total": 10,
            "total_kind": "budget",
            "warnings": [{ "code": "robots", "message": "robots.txt limited the crawl" }],
            "control": { "desired": "pause", "pending": true },
            "schedule_id": "sch_1"
        });
        let view = to_job_detail(&job);
        assert_eq!(view["id"], "job_1");
        assert_eq!(view["origin"]["type"], "schedule");
        assert_eq!(view["scheduleId"], "sch_1");
        assert_eq!(view["counts"]["blocked"], 0);
        assert_eq!(view["control"]["pending"], true);
        assert_eq!(view["warnings"][0]["code"], "robots");
        assert_eq!(view["controllable"], true);
    }

    #[test]
    fn errors_view_keeps_the_urls_quarry_recorded() {
        let raw = json!({
            "errors": [{ "id": "e1", "ts": "t", "url": "https://a.no/x", "code": "http_503", "category": "upstream", "retryable": true, "status": 503 }],
            "blocked": [{ "url": "https://a.no/login", "ts": "t", "reason": "auth" }],
            "robots_blocked": [{ "url": "https://a.no/private", "ts": "t" }],
            "next_seq": 7
        });
        let view = to_errors_view(&raw);
        assert_eq!(view["errors"][0]["status"], 503);
        assert_eq!(view["blocked"][0]["reason"], "auth");
        assert_eq!(view["robotsBlocked"][0]["url"], "https://a.no/private");
        assert_eq!(view["nextSeq"], 7);
    }

    #[test]
    fn only_crawl_and_batch_are_controllable() {
        assert!(CONTROLLABLE_KINDS.contains(&"crawl"));
        assert!(!CONTROLLABLE_KINDS.contains(&"scrape"));
    }
}
