//! Ingestion runs — `GET/POST /api/ingestions/runs`.
//!
//! Port of verevonv2's `app/api/ingestions/runs/route.ts`. The list fans out
//! across quarry-control's per-kind job registries and normalizes each job into
//! the SPA's `RunItem`. Create dispatches by `kind` to the matching quarry
//! execution endpoint, SSRF-guarding every user URL, and returns `RunCreateResult`.

use axum::{
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use futures_util::future::join_all;
use reqwest::Method;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::{config::AppState, envelope::unwrap_data, middleware::AuthenticatedUser};

use super::shared::{
    array_at, cookie_header, created, date_millis, first_num, first_str, forward,
    normalize_batch_urls, normalize_target, obj_or_empty, okay, quarry_call, quarry_token, str_at,
    str_or_null, validation,
};

const RUN_KINDS: [&str; 5] = ["crawl", "extract", "search", "agent", "batch"];

// ── List ────────────────────────────────────────────────────────────────────

pub(super) async fn list_runs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;

    let limit: u32 = params
        .get("limit")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(20)
        .clamp(1, 50);
    let requested = params
        .get("kind")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let kinds: Vec<&str> = match requested {
        Some(kind) if RUN_KINDS.contains(&kind) => vec![kind],
        _ => RUN_KINDS.to_vec(),
    };

    let state_ref = &state;
    let token_ref = token.as_deref();
    let user_id = user.user_id.as_str();

    // Each kind's job registry is independent — fetch concurrently, like v2's
    // `Promise.all(kinds.map(loadJobs))`.
    let results = join_all(kinds.into_iter().map(|kind| {
        let path = format!("/v1/{kind}/jobs?limit={limit}");
        async move { quarry_call(state_ref, Method::GET, &path, None, token_ref, user_id).await }
    }))
    .await;

    let mut any_ok = false;
    let mut first_err: Option<(StatusCode, Value)> = None;
    let mut runs: Vec<Value> = Vec::new();
    for (status, body) in results {
        if status.is_success() {
            any_ok = true;
            for job in array_at(&unwrap_data(&body), "items") {
                runs.push(to_run_item(&job));
            }
        } else if first_err.is_none() {
            first_err = Some((status, body));
        }
    }

    // Only surface an error when *every* kind failed (total quarry outage);
    // otherwise a single missing registry shouldn't blank the whole list.
    if !any_ok {
        if let Some((status, body)) = first_err {
            return forward(status, body);
        }
    }

    runs.sort_by(|a, b| {
        let am = date_millis(a.get("createdAt").and_then(Value::as_str).unwrap_or(""));
        let bm = date_millis(b.get("createdAt").and_then(Value::as_str).unwrap_or(""));
        bm.cmp(&am)
    });

    okay(Value::Array(runs))
}

/// Normalize a quarry job into the SPA's `RunItem`.
fn to_run_item(job: &Value) -> Value {
    let stats = obj_or_empty(job, "stats");
    let kind = str_at(job, "kind");

    let completed = first_num(&stats, &["completed", "pages_completed", "pages"]);
    let total = first_num(&stats, &["total", "max_pages", "target_pages"]);
    let pages = first_num(&stats, &["pages", "page_count"]);
    let url_count = first_num(&stats, &["url_count", "urls"]);
    let query = first_str(&stats, &["query"]);

    let target = first_str(&stats, &["url", "seed_url", "query", "target"]).unwrap_or_else(|| {
        first_str(&stats, &["url", "seed_url", "query"]).unwrap_or_else(|| {
            if first_num(&stats, &["pages"]).is_some() {
                format!("{kind} target")
            } else {
                "Manual run".to_owned()
            }
        })
    });

    let mut progress = serde_json::Map::new();
    if let Some(v) = completed {
        progress.insert("completed".into(), v);
    }
    if let Some(v) = total {
        progress.insert("total".into(), v);
    }
    if let Some(v) = pages {
        progress.insert("pages".into(), v);
    }
    if let Some(v) = url_count {
        progress.insert("urlCount".into(), v);
    }
    progress.insert(
        "query".into(),
        query.map(Value::String).unwrap_or(Value::Null),
    );

    json!({
        "id": str_at(job, "job_id"),
        // The Temporal run id, stamped once the job starts. The evidence
        // endpoint (`/api/ingestions/evidence?runId=…`) proxies quarry-edge's
        // `/v1/runs/{id}/events`, which strictly parses a `run_…` id — passing
        // this job row's `id` (a `job_…` id) there is a guaranteed 400. The SPA
        // must prefer `runId` for evidence lookups; null until the job starts.
        "runId": str_or_null(job, "run_id"),
        "kind": kind,
        "status": str_at(job, "status"),
        "createdAt": str_at(job, "created_at"),
        "startedAt": str_or_null(job, "started_at"),
        "completedAt": str_or_null(job, "completed_at"),
        "target": target,
        "progress": Value::Object(progress),
        "stats": stats,
    })
}

// ── Create ────────────────────────────────────────────────────────────────

pub(super) async fn create_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let token = token.as_deref();
    let user_id = user.user_id.as_str();

    let kind = str_at(&body, "kind");
    if kind.is_empty() {
        return validation("Run kind is required.");
    }

    match kind.as_str() {
        "crawl" => create_crawl(&state, &body, token, user_id).await,
        "batch" => create_batch(&state, &body, token, user_id).await,
        "extract" => create_extract(&state, &body, token, user_id).await,
        _ => create_scrape(&state, &body, token, user_id).await,
    }
}

async fn create_crawl(
    state: &AppState,
    body: &Value,
    token: Option<&str>,
    user_id: &str,
) -> Response {
    let Some(raw) = first_str(body, &["url"]) else {
        return validation("A URL is required for crawl runs.");
    };
    let url = match normalize_target(&raw) {
        Ok(value) => value,
        Err(message) => return validation(&message),
    };
    let max_pages = body
        .get("maxPages")
        .or_else(|| body.get("max_pages"))
        .and_then(Value::as_u64)
        .unwrap_or(12)
        .clamp(1, 50);

    let request = json!({ "url": url, "max_pages": max_pages });
    let (status, resp) = quarry_call(
        state,
        Method::POST,
        "/v1/crawl",
        Some(request),
        token,
        user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, resp);
    }
    let data = unwrap_data(&resp);
    created(json!({
        "run": {
            "id": handoff_id(&data),
            "kind": "crawl",
            "status": "queued",
            "createdAt": handoff_accepted_at(&data),
            "target": url,
        }
    }))
}

async fn create_batch(
    state: &AppState,
    body: &Value,
    token: Option<&str>,
    user_id: &str,
) -> Response {
    let raw = body
        .get("urls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let urls = match normalize_batch_urls(&raw) {
        Ok(value) => value,
        Err(_) => return validation("At least one URL is required for batch runs."),
    };
    let count = urls.len();

    let request = json!({ "urls": urls });
    let (status, resp) = quarry_call(
        state,
        Method::POST,
        "/v1/batch",
        Some(request),
        token,
        user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, resp);
    }
    let data = unwrap_data(&resp);
    created(json!({
        "run": {
            "id": handoff_id(&data),
            "kind": "batch",
            "status": "queued",
            "createdAt": handoff_accepted_at(&data),
            "target": format!("{count} URLs"),
        }
    }))
}

async fn create_extract(
    state: &AppState,
    body: &Value,
    token: Option<&str>,
    user_id: &str,
) -> Response {
    let Some(raw) = first_str(body, &["url"]) else {
        return validation("A URL is required for this run.");
    };
    let url = match normalize_target(&raw) {
        Ok(value) => value,
        Err(message) => return validation(&message),
    };

    let mut request = json!({ "urls": [url.clone()] });
    if let Some(prompt) = first_str(body, &["prompt"]) {
        request["prompt"] = Value::String(prompt);
    }
    let (status, resp) = quarry_call(
        state,
        Method::POST,
        "/v1/extract",
        Some(request),
        token,
        user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, resp);
    }

    let data = unwrap_data(&resp);
    let results = array_at(&data, "results");
    let first = results.first().cloned().unwrap_or(Value::Null);
    let now = chrono::Utc::now().to_rfc3339();
    let first_url = first
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&url)
        .to_owned();
    let run_status = first
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("completed")
        .to_owned();

    okay(json!({
        "run": {
            "id": format!("extract:{first_url}"),
            "kind": "extract",
            "status": run_status,
            "createdAt": now.clone(),
            "target": url.clone(),
        },
        "evidence": {
            "kind": "extract",
            "targetUrl": url,
            "extractedAt": now,
            "result": first,
            "results": results,
        }
    }))
}

async fn create_scrape(
    state: &AppState,
    body: &Value,
    token: Option<&str>,
    user_id: &str,
) -> Response {
    let Some(raw) = first_str(body, &["url"]) else {
        return validation("A URL is required for this run.");
    };
    let url = match normalize_target(&raw) {
        Ok(value) => value,
        Err(message) => return validation(&message),
    };

    let request = json!({ "url": url });
    let (status, resp) = quarry_call(
        state,
        Method::POST,
        "/v1/scrape",
        Some(request),
        token,
        user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, resp);
    }

    let data = unwrap_data(&resp);
    let status_code = first_num(&data, &["status"])
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let final_url = data
        .get("url")
        .and_then(|u| {
            u.get("final_url")
                .and_then(Value::as_str)
                .or_else(|| u.get("requested").and_then(Value::as_str))
                .or_else(|| u.as_str())
        })
        .map(str::to_owned)
        .unwrap_or_else(|| url.clone());
    let run_status = if (200..400).contains(&status_code) {
        "completed"
    } else {
        "failed"
    };
    let fetched_at = first_str(&data, &["fetched_at"]).unwrap_or_default();

    okay(json!({
        "run": {
            "id": first_str(&data, &["run_id"]).unwrap_or_default(),
            "kind": "scrape",
            "status": run_status,
            "createdAt": fetched_at.clone(),
            "target": final_url.clone(),
        },
        "evidence": {
            "kind": "scrape",
            "targetUrl": final_url,
            "extractedAt": fetched_at,
            "fingerprint": first_str(&data, &["fingerprint"]).unwrap_or_default(),
            "statusCode": status_code,
            "metadata": obj_or_empty(&data, "metadata"),
            "driver": obj_or_empty(&data, "driver"),
            "formats": obj_or_empty(&data, "formats"),
            "sourceTrace": data.get("source_trace").cloned().unwrap_or(Value::Null),
        }
    }))
}

// ── Small helpers ───────────────────────────────────────────────────────────

fn handoff_id(data: &Value) -> String {
    first_str(data, &["job_id", "jobId", "id", "run_id", "runId"]).unwrap_or_default()
}

fn handoff_accepted_at(data: &Value) -> String {
    first_str(
        data,
        &["accepted_at", "acceptedAt", "created_at", "createdAt"],
    )
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_run_item_derives_target_and_progress() {
        let job = json!({
            "job_id": "job-1",
            "kind": "crawl",
            "status": "running",
            "created_at": "2026-06-14T10:00:00Z",
            "started_at": "2026-06-14T10:00:01Z",
            "completed_at": null,
            "stats": { "url": "https://vg.no", "completed": 3, "max_pages": 12, "pages": 3 }
        });
        let item = to_run_item(&job);
        assert_eq!(item["id"], "job-1");
        assert_eq!(item["target"], "https://vg.no");
        assert_eq!(item["startedAt"], "2026-06-14T10:00:01Z");
        assert_eq!(item["completedAt"], Value::Null);
        assert_eq!(item["progress"]["completed"], 3);
        assert_eq!(item["progress"]["total"], 12);
        assert_eq!(item["progress"]["query"], Value::Null);
    }

    #[test]
    fn to_run_item_falls_back_to_manual_run_without_target_hints() {
        let job = json!({ "job_id": "j", "kind": "agent", "status": "done", "created_at": "x" });
        let item = to_run_item(&job);
        assert_eq!(item["target"], "Manual run");
        // No numeric progress fields, query present as null.
        assert!(item["progress"].get("completed").is_none());
        assert_eq!(item["progress"]["query"], Value::Null);
    }

    #[test]
    fn to_run_item_uses_kind_target_when_only_pages_present() {
        let job = json!({ "job_id": "j", "kind": "crawl", "status": "x", "created_at": "x", "stats": { "pages": 5 } });
        let item = to_run_item(&job);
        assert_eq!(item["target"], "crawl target");
        assert_eq!(item["progress"]["pages"], 5);
    }
}
