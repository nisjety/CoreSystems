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
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::{
    config::AppState,
    domains::browser::{is_valid_artifact_id, BrowserArtifactAbsence, MAX_BROWSER_ARTIFACT_BYTES},
    envelope::error,
    middleware::AuthenticatedUser,
};

use super::shared::{
    array_at, cookie_header, first_str, forward, obj_or_empty, okay, quarry_call, quarry_token,
    str_at, str_or_null, validation,
};

/// The job kinds Quarry exposes pause/resume/cancel for. Scrape, search, extract
/// and agent runs finish inside one request, so there is nothing to control.
const CONTROLLABLE_KINDS: [&str; 2] = ["crawl", "batch"];
const CONTROL_ACTIONS: [&str; 3] = ["pause", "resume", "cancel"];
/// The most text `GET /api/ingestions/artifacts/{id}` returns; a longer artifact is cut.
const MAX_TEXT_ARTIFACT_BYTES: usize = 1024 * 1024;

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

/// `GET /api/ingestions/jobs/{id}/results` — the pages a crawl, batch or job fetched, in
/// fetch order. Query: `afterSeq` (cursor), `limit` (1..500, default 100) and `latest=true`
/// (only the newest fetch of each URL). Quarry answers 501 while `QUARRY_EDGE__JOB_RESULTS`
/// is off and 403 under ZDR; both pass through unchanged.
pub(super) async fn get_job_results(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let Some(job_id) = valid_job_id(&id) else {
        return validation("A job id is required.");
    };
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &results_path(&job_id, &params),
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }
    okay(to_results_view(&crate::envelope::unwrap_data(&body)))
}

/// `GET /api/ingestions/artifacts/{id}` — the text of one artifact of the caller's org,
/// such as a page's markdown (`markdownArtifactId` in a results row). Only `text/*` and
/// `application/json` are served, cut to 1 MiB with `truncated` set. A ZDR org gets the
/// same refusal as a scrape artifact, because Quarry stores nothing for it.
pub(super) async fn get_artifact_text(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !is_valid_artifact_id(&id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_artifact", "The artifact id is invalid.")),
        )
            .into_response();
    }
    if crate::zdr::org_zdr_enabled(&state, &user).await {
        return BrowserArtifactAbsence::Withheld.into_response();
    }

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let url = format!(
        "{}/v1/artifacts/{}",
        state.quarry_edge_url,
        urlencoding::encode(&id)
    );
    let mut req =
        crate::request_id::apply(state.client.get(&url)).header("x-user-id", &user.user_id);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let upstream = match req.send().await {
        Ok(upstream) => upstream,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(crate::envelope::upstream_unavailable()),
            )
                .into_response()
        }
    };
    let status = upstream.status();
    if matches!(status.as_u16(), 404 | 410) {
        return (
            StatusCode::NOT_FOUND,
            Json(error("artifact_not_found", "The artifact is not available.")),
        )
            .into_response();
    }
    if !status.is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        )
            .into_response();
    }
    let upstream_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let Some(content_type) = text_artifact_content_type(upstream_type.as_deref()) else {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(error(
                "artifact_unsupported",
                "Only text artifacts can be read here.",
            )),
        )
            .into_response();
    };
    if upstream
        .content_length()
        .is_some_and(|length| length > MAX_BROWSER_ARTIFACT_BYTES)
    {
        return artifact_too_large();
    }
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(crate::envelope::upstream_unavailable()),
            )
                .into_response()
        }
    };
    if bytes.len() as u64 > MAX_BROWSER_ARTIFACT_BYTES {
        return artifact_too_large();
    }
    let text = String::from_utf8_lossy(&bytes);
    let (content, truncated) = cap_text(&text, MAX_TEXT_ARTIFACT_BYTES);
    okay(json!({
        "id": id,
        "contentType": content_type,
        "content": content,
        "bytes": bytes.len(),
        "truncated": truncated,
    }))
}

fn artifact_too_large() -> Response {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(error(
            "artifact_too_large",
            "The artifact is too large to read here.",
        )),
    )
        .into_response()
}

/// The `essence` of a text or JSON content type (`text/markdown; charset=utf-8` →
/// `text/markdown`), or `None` for anything else.
fn text_artifact_content_type(value: Option<&str>) -> Option<String> {
    let essence = value
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    (essence.starts_with("text/") || essence == "application/json").then_some(essence)
}

/// Cuts `text` to at most `max` bytes without splitting a character. Returns the kept
/// part and whether anything was cut.
fn cap_text(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
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

/// The Quarry path of a results read. `limit` is clamped to 1..=500 (default 100);
/// `afterSeq` and `latest=true` are forwarded only when they parse.
fn results_path(job_id: &str, params: &HashMap<String, String>) -> String {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(1, 500))
        .unwrap_or(100);
    let mut query = vec![format!("limit={limit}")];
    if let Some(after) = params.get("afterSeq").and_then(|v| v.parse::<u64>().ok()) {
        query.push(format!("after_seq={after}"));
    }
    if params.get("latest").map(|v| v.trim()) == Some("true") {
        query.push("latest=true".to_owned());
    }
    format!("/v1/jobs/{job_id}/results?{}", query.join("&"))
}

/// Reshape Quarry's `JobResultsView`: one row per fetched page, in fetch order, with the
/// artifact ids that hold its markdown and source. `nextSeq` is null on the last page.
pub(super) fn to_results_view(view: &Value) -> Value {
    let items: Vec<Value> = array_at(view, "items")
        .into_iter()
        .map(|item| {
            json!({
                "seq": item.get("seq").cloned().unwrap_or(Value::Null),
                "url": str_or_null(&item, "url"),
                "status": item.get("status").cloned().unwrap_or(Value::Null),
                "contentType": str_or_null(&item, "content_type"),
                "title": str_or_null(&item, "title"),
                "fetchedAt": str_or_null(&item, "fetched_at"),
                "wordCount": item.get("word_count").cloned().unwrap_or(Value::Null),
                "driver": str_or_null(&item, "driver"),
                "markdownArtifactId": str_or_null(&item, "markdown_artifact_id"),
                "sourceArtifactId": str_or_null(&item, "source_artifact_id"),
                "bytes": item.get("bytes").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    json!({
        "items": items,
        "nextSeq": view.get("next_seq").cloned().unwrap_or(Value::Null),
        "scanCapped": view.get("scan_capped").and_then(Value::as_bool).unwrap_or(false),
    })
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
    let mut detail = json!({
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
    });
    // INNH-9: credits are present only while Quarry's job-credits flag is on.
    if let Some(credits) = job.get("credits").and_then(super::runs::to_credits) {
        detail["credits"] = credits;
    }
    detail
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

    /// INNH-9: credits are passed through when Quarry sends them, and absent otherwise.
    #[test]
    fn job_detail_forwards_credits_only_when_present() {
        let with = json!({
            "job_id": "job_c", "kind": "batch", "status": "completed", "created_at": "t",
            "credits": { "used": 3.0, "unit": "credit", "basis": "pages_fetched",
                         "price_per_page": 1.0, "estimated": true, "final": true }
        });
        let view = to_job_detail(&with);
        assert_eq!(view["credits"]["used"], 3.0);
        assert_eq!(view["credits"]["final"], true);
        let without = json!({ "job_id": "job_d", "kind": "batch", "status": "completed", "created_at": "t" });
        assert!(to_job_detail(&without).get("credits").is_none());
    }

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
    fn results_path_clamps_the_limit_and_forwards_only_known_parameters() {
        let none = HashMap::new();
        assert_eq!(results_path("job_1", &none), "/v1/jobs/job_1/results?limit=100");

        let mut params = HashMap::new();
        params.insert("limit".to_owned(), "9000".to_owned());
        params.insert("afterSeq".to_owned(), "12".to_owned());
        params.insert("latest".to_owned(), "true".to_owned());
        assert_eq!(
            results_path("job_1", &params),
            "/v1/jobs/job_1/results?limit=500&after_seq=12&latest=true"
        );

        let mut junk = HashMap::new();
        junk.insert("limit".to_owned(), "0".to_owned());
        junk.insert("afterSeq".to_owned(), "x".to_owned());
        junk.insert("latest".to_owned(), "yes".to_owned());
        assert_eq!(results_path("job_1", &junk), "/v1/jobs/job_1/results?limit=1");
    }

    #[test]
    fn results_view_keeps_artifact_ids_and_leaves_missing_fields_null() {
        let raw = json!({
            "items": [
                {
                    "seq": 1, "url": "https://a.no/", "status": 200, "content_type": "text/html",
                    "title": "A", "fetched_at": "2026-10-31T10:00:00Z", "word_count": 120,
                    "driver": "static", "markdown_artifact_id": "art_md1",
                    "source_artifact_id": "art_src1", "bytes": 900
                },
                { "seq": 2, "url": "https://a.no/x", "status": 404, "fetched_at": "2026-10-31T10:00:02Z" }
            ],
            "next_seq": 2
        });
        let view = to_results_view(&raw);
        assert_eq!(view["items"][0]["markdownArtifactId"], "art_md1");
        assert_eq!(view["items"][0]["sourceArtifactId"], "art_src1");
        assert_eq!(view["items"][0]["wordCount"], 120);
        assert_eq!(view["items"][1]["status"], 404);
        assert!(view["items"][1]["title"].is_null());
        assert!(view["items"][1]["markdownArtifactId"].is_null());
        assert!(view["items"][1]["bytes"].is_null());
        assert_eq!(view["nextSeq"], 2);
        assert_eq!(view["scanCapped"], false);

        let last = to_results_view(&json!({ "items": [], "scan_capped": true }));
        assert!(last["nextSeq"].is_null());
        assert_eq!(last["scanCapped"], true);
    }

    #[test]
    fn artifact_text_is_cut_on_a_character_boundary() {
        let text = "é".repeat(10);
        let (kept, truncated) = cap_text(&text, 5);
        assert!(truncated);
        assert_eq!(kept, "éé");
        assert_eq!(cap_text("abc", 5), ("abc", false));
    }

    #[test]
    fn only_text_and_json_artifacts_are_served() {
        assert_eq!(
            text_artifact_content_type(Some("text/markdown; charset=utf-8")).as_deref(),
            Some("text/markdown")
        );
        assert_eq!(
            text_artifact_content_type(Some("Application/JSON")).as_deref(),
            Some("application/json")
        );
        assert!(text_artifact_content_type(Some("image/png")).is_none());
        assert!(text_artifact_content_type(Some("application/pdf")).is_none());
        assert!(text_artifact_content_type(None).is_none());
    }

    #[test]
    fn artifact_ids_must_be_quarry_artifact_ids() {
        assert!(is_valid_artifact_id("art_01HZX9"));
        assert!(!is_valid_artifact_id("job_01HZX9"));
        assert!(!is_valid_artifact_id("art_"));
        assert!(!is_valid_artifact_id("art_../../x"));
    }

    #[test]
    fn only_crawl_and_batch_are_controllable() {
        assert!(CONTROLLABLE_KINDS.contains(&"crawl"));
        assert!(!CONTROLLABLE_KINDS.contains(&"scrape"));
    }
}
