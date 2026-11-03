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
    array_at, bad_request, cookie_header, created, date_millis, first_num, first_str, forward,
    normalize_batch_urls, normalize_target, obj_or_empty, okay, quarry_call, quarry_token, str_at,
    str_or_null, validation,
};

// `scrape` is listed since WS15.L5: a single-page fetch is a job of its own in
// quarry-control's registry (`/v1/scrape/jobs`) and carries the same origin and
// phase as the other kinds.
const RUN_KINDS: [&str; 6] = ["crawl", "extract", "search", "agent", "batch", "scrape"];

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

    let mut row = json!({
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
        // Who started the job (`{type, ref}`) and which schedule fired it,
        // so a run can say "manual" or "schedule X" instead of guessing. The
        // actor behind the origin is an internal identity and stays inside.
        "origin": origin_view(job),
        "scheduleId": str_or_null(job, "schedule_id"),
        "phase": str_or_null(job, "phase"),
        "stats": stats,
        // WS15.L5: what was last asked of the job (`pause`, `resume`, `cancel`).
        // Null when quarry does not send it (older rows, a flag that is off).
        "desiredState": desired_state(job),
    });
    if let Some(credits) = job.get("credits").and_then(to_credits) {
        row["credits"] = credits;
    }
    row
}

/// Reshapes Quarry's `JobCredits` (`used`, `unit`, `basis`, `price_per_page`,
/// `estimated`, `final`) into the SPA's camelCase. `None` when the edge sent no object.
/// Quarry sends the field only while QUARRY_CONTROL__JOB_CREDITS is on, so a row
/// without it stays as it was.
pub(super) fn to_credits(credits: &Value) -> Option<Value> {
    let object = credits.as_object()?;
    Some(json!({
        "used": object.get("used").cloned().unwrap_or(json!(0)),
        "unit": object.get("unit").cloned().unwrap_or(json!("credit")),
        "basis": object.get("basis").cloned().unwrap_or(json!("pages_fetched")),
        "pricePerPage": object.get("price_per_page").cloned().unwrap_or(json!(0)),
        "estimated": object.get("estimated").cloned().unwrap_or(json!(true)),
        "final": object.get("final").cloned().unwrap_or(json!(false)),
    }))
}

/// The extraction prompt of a run body, trimmed. Blank means none; a prompt over
/// 10,000 characters is refused here, the same limit quarry-edge applies.
fn extract_prompt(body: &Value) -> Result<Option<String>, String> {
    let Some(raw) = body.get("prompt").and_then(Value::as_str) else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > 10_000 {
        return Err("The extraction prompt must be at most 10,000 characters.".to_owned());
    }
    Ok(Some(trimmed.to_owned()))
}

/// The job's origin as the SPA reads it: `{ type, ref }`. Quarry sends
/// `{ type, actor?, ref? }` (or a bare type string); the actor is an internal
/// identity and does not cross to the browser. Null when there is no type.
pub(super) fn origin_view(job: &Value) -> Value {
    let (label, reference) = match job.get("origin") {
        Some(Value::String(s)) => (s.trim(), None),
        Some(Value::Object(o)) => (
            o.get("type").and_then(Value::as_str).unwrap_or("").trim(),
            o.get("ref").and_then(Value::as_str).map(str::trim).filter(|r| !r.is_empty()),
        ),
        _ => ("", None),
    };
    if label.is_empty() {
        return Value::Null;
    }
    json!({ "type": label, "ref": reference })
}

/// What was last asked of the job (`pause`, `resume`, `cancel`), from the job row or
/// from the lifecycle `control` block; null when nothing was asked.
fn desired_state(job: &Value) -> Value {
    ["desired_state", "desiredState"]
        .iter()
        .filter_map(|key| job.get(*key).and_then(Value::as_str))
        .chain(job.pointer("/control/desired").and_then(Value::as_str))
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(|s| Value::String(s.to_owned()))
        .unwrap_or(Value::Null)
}

// ── Create ────────────────────────────────────────────────────────────────

pub(crate) async fn create_run(
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

/// The typed code for a structured-extraction refusal, read from the `error` text that
/// quarry-edge sets. Quarry gives these refusals no `details.reason`, so the text is the only
/// signal it sends. A text that matches none of the four keeps Quarry's own `UNSUPPORTED`.
fn extract_refusal_code(text: &str) -> Option<&'static str> {
    if text.contains("QUARRY_EDGE__AI_FORMATS is off") {
        Some("extract_ai_formats_off")
    } else if text.contains("QUARRY_EDGE__MODEL_PLANE_URL is not set") {
        Some("extract_model_plane_not_configured")
    } else if text.contains("holds no model credential") {
        Some("extract_model_lane_not_granted")
    } else if text.contains("refused this service") {
        Some("extract_model_plane_refused")
    } else {
        None
    }
}

/// The 501 body with the typed code at its top level, where the SPA reads it.
fn with_extract_refusal_code(status: StatusCode, mut body: Value) -> Value {
    if status != StatusCode::NOT_IMPLEMENTED {
        return body;
    }
    let text = body.get("error").and_then(Value::as_str).unwrap_or_default();
    if let Some(code) = extract_refusal_code(text) {
        body["code"] = json!(code);
    }
    body
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

    // quarry-edge's `/v1/extract` reads both: a `schema` makes the extraction
    // structured, and a `prompt` steers it (inside the schema) or asks for one
    // free-form JSON object when it stands alone. With neither, the sources come
    // back as markdown.
    let mut request = json!({ "urls": [url.clone()] });
    match extract_schema(body) {
        Ok(Some(schema)) => request["schema"] = schema,
        Ok(None) => {}
        // Present but not a JSON object: malformed, so a 400, and nothing reaches quarry-edge.
        Err(message) => return bad_request("invalid_schema", &message),
    }
    match extract_prompt(body) {
        Ok(Some(prompt)) => request["prompt"] = Value::String(prompt),
        Ok(None) => {}
        Err(message) => return validation(&message),
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
        return forward(status, with_extract_refusal_code(status, resp));
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
    // quarry-edge reports per source: "ok" (structured), "fetched" (markdown only)
    // or "error". A failed source is a failed run; the other two are completed.
    let run_status = match first.get("status").and_then(Value::as_str).map(str::trim) {
        Some("error") => "failed",
        _ => "completed",
    };

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

/// The extraction schema of a run body: a JSON object, or the same document sent
/// as text. `Ok(None)` when none was sent or the text is blank. A schema that is not a
/// JSON object is an `Err`, which `create_extract` answers with a 400.
fn extract_schema(body: &Value) -> Result<Option<Value>, String> {
    match body.get("schema") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.trim().is_empty() => Ok(None),
        Some(Value::String(text)) => serde_json::from_str::<Value>(text)
            .ok()
            .filter(Value::is_object)
            .map(Some)
            .ok_or_else(|| "The extraction schema must be a JSON object.".to_owned()),
        Some(value) if value.is_object() => Ok(Some(value.clone())),
        Some(_) => Err("The extraction schema must be a JSON object.".to_owned()),
    }
}

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

    /// INNH-1 / INNH-9: the job's credits pass through in camelCase, and a row without
    /// them (the flag is off) has no `credits` key at all, not a null one.
    #[test]
    fn run_rows_carry_credits_only_when_quarry_sends_them() {
        let with = json!({
            "job_id": "job-2", "kind": "crawl", "status": "running", "created_at": "t",
            "credits": { "used": 12.0, "unit": "credit", "basis": "pages_fetched",
                         "price_per_page": 1.0, "estimated": true, "final": false }
        });
        let row = to_run_item(&with);
        assert_eq!(row["credits"]["used"], 12.0);
        assert_eq!(row["credits"]["pricePerPage"], 1.0);
        assert_eq!(row["credits"]["final"], false);
        let without = json!({ "job_id": "job-3", "kind": "crawl", "status": "running", "created_at": "t" });
        assert!(to_run_item(&without).get("credits").is_none());
    }

    /// INNH-2: a prompt is trimmed; blank means none; over 10,000 characters is refused.
    #[test]
    fn extract_prompt_is_trimmed_and_bounded() {
        assert_eq!(extract_prompt(&json!({ "prompt": "  Find the price  " })).unwrap().as_deref(), Some("Find the price"));
        assert_eq!(extract_prompt(&json!({ "prompt": "   " })).unwrap(), None);
        assert_eq!(extract_prompt(&json!({})).unwrap(), None);
        let long = "x".repeat(10_001);
        assert!(extract_prompt(&json!({ "prompt": long })).is_err());
        assert!(extract_prompt(&json!({ "prompt": "y".repeat(10_000) })).is_ok());
    }

    /// WS16.S7: a schema is a JSON object, or the same object as text. Blank text means none;
    /// anything else is refused before it can reach quarry-edge.
    #[test]
    fn extract_schema_accepts_an_object_or_its_text_and_refuses_the_rest() {
        let object = json!({ "type": "object" });
        assert_eq!(extract_schema(&json!({})).unwrap(), None);
        assert_eq!(extract_schema(&json!({ "schema": null })).unwrap(), None);
        assert_eq!(extract_schema(&json!({ "schema": "  " })).unwrap(), None);
        assert_eq!(
            extract_schema(&json!({ "schema": object.clone() })).unwrap(),
            Some(object.clone())
        );
        assert_eq!(
            extract_schema(&json!({ "schema": "{\"type\":\"object\"}" })).unwrap(),
            Some(object)
        );
        for malformed in [
            json!("not json"),
            json!("[1, 2]"),
            json!("\"text\""),
            json!(["type"]),
            json!(42),
            json!(true),
        ] {
            let refused = extract_schema(&json!({ "schema": malformed.clone() }));
            assert!(refused.is_err(), "{malformed}");
        }
    }

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

    #[test]
    fn each_of_the_four_extract_refusals_gets_its_own_code() {
        let texts = [
            (
                "structured extraction needs the Model Plane, and QUARRY_EDGE__MODEL_PLANE_URL is not set on this deployment",
                "extract_model_plane_not_configured",
            ),
            (
                "structured extraction needs a granted Model Plane lane, and this deployment holds no model credential",
                "extract_model_lane_not_granted",
            ),
            (
                "structured extraction needs a granted Model Plane lane, and the Model Plane refused this service",
                "extract_model_plane_refused",
            ),
            (
                "structured extraction is not available on this edge: QUARRY_EDGE__AI_FORMATS is off",
                "extract_ai_formats_off",
            ),
        ];
        for (text, code) in texts {
            assert_eq!(extract_refusal_code(text), Some(code), "{text}");
        }
    }

    #[test]
    fn an_unknown_extract_refusal_keeps_quarry_s_own_code() {
        assert_eq!(extract_refusal_code("a refusal this gateway has not seen"), None);
        let body = json!({ "error": "a refusal this gateway has not seen", "code": "UNSUPPORTED" });
        let kept = with_extract_refusal_code(StatusCode::NOT_IMPLEMENTED, body);
        assert_eq!(kept["code"], "UNSUPPORTED");
    }

    #[test]
    fn the_code_is_set_only_on_a_501_refusal() {
        let body = json!({ "error": "QUARRY_EDGE__AI_FORMATS is off", "code": "UNSUPPORTED" });
        let other_status = with_extract_refusal_code(StatusCode::BAD_REQUEST, body.clone());
        assert_eq!(other_status["code"], "UNSUPPORTED");
        let refused = with_extract_refusal_code(StatusCode::NOT_IMPLEMENTED, body);
        assert_eq!(refused["code"], "extract_ai_formats_off");
    }
}

#[cfg(test)]
mod job_control_field_tests {
    use super::*;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn a_run_item_carries_origin_phase_and_desired_state() {
        let item = to_run_item(&json!({
            "job_id": "job-1", "kind": "crawl", "status": "running", "created_at": "x",
            "origin": { "type": "knowledge", "actor": "user-9", "ref": "r1" },
            "phase": "fetching", "desired_state": "pause"
        }));
        assert_eq!(item["origin"]["type"], "knowledge");
        assert_eq!(item["origin"]["ref"], "r1");
        assert_eq!(item["phase"], "fetching");
        assert_eq!(item["desiredState"], "pause");
        // The actor behind the origin stays inside the platform.
        assert!(!item.to_string().contains("user-9"));
    }

    #[test]
    fn the_new_fields_are_null_when_quarry_sends_none() {
        let item = to_run_item(&json!({ "job_id": "j", "kind": "scrape", "status": "completed", "created_at": "x" }));
        assert_eq!(item["origin"], Value::Null);
        assert_eq!(item["phase"], Value::Null);
        assert_eq!(item["desiredState"], Value::Null);
        let blank = to_run_item(&json!({
            "job_id": "j", "kind": "crawl", "status": "x", "created_at": "x",
            "origin": { "type": "  " }, "phase": 7, "desired_state": ""
        }));
        assert_eq!(blank["origin"], Value::Null);
        assert_eq!(blank["phase"], Value::Null);
        assert_eq!(blank["desiredState"], Value::Null);
    }

    #[test]
    fn desired_state_reads_the_control_block_and_prefers_the_row() {
        assert_eq!(desired_state(&json!({ "control": { "desired": "resume", "pending": true } })), "resume");
        assert_eq!(
            desired_state(&json!({ "desired_state": "cancel", "control": { "desired": "resume" } })),
            "cancel"
        );
        assert_eq!(desired_state(&json!({ "desiredState": "pause" })), "pause");
        assert_eq!(origin_view(&json!({ "origin": "schedule" }))["type"], "schedule");
    }

    #[test]
    fn scrape_is_a_listed_run_kind() {
        assert!(RUN_KINDS.contains(&"scrape"));
        assert_eq!(RUN_KINDS.len(), 6);
    }

    #[tokio::test]
    async fn the_list_reads_the_scrape_registry_and_keeps_its_rows() {
        let quarry = MockServer::start().await;
        for kind in ["crawl", "extract", "search", "agent", "batch"] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/v1/{kind}/jobs")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "items": [] } })))
                .expect(1)
                .mount(&quarry)
                .await;
        }
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/scrape/jobs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "items": [{
                    "job_id": "job_s1", "kind": "scrape", "status": "completed",
                    "created_at": "2026-11-01T10:00:00Z", "stats": { "url": "https://example.com/a" },
                    "origin": { "type": "api" }, "phase": "completed"
                }] }
            })))
            .expect(1)
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        let user = AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        };

        let response = list_runs(
            State(state),
            Extension(user),
            HeaderMap::new(),
            Query(HashMap::new()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let rows = body["data"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], "job_s1");
        assert_eq!(rows[0]["kind"], "scrape");
        assert_eq!(rows[0]["origin"]["type"], "api");
        assert_eq!(rows[0]["phase"], "completed");
    }

    #[tokio::test]
    async fn a_kind_filter_of_scrape_reads_only_that_registry() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/scrape/jobs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "items": [] } })))
            .expect(1)
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        let user = AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        };
        let mut params = HashMap::new();
        params.insert("kind".to_owned(), "scrape".to_owned());
        let response = list_runs(State(state), Extension(user), HeaderMap::new(), Query(params)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(quarry.received_requests().await.unwrap().len(), 1);
    }
}
