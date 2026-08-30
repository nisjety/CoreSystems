//! Ingestion schedules — `GET/POST /api/ingestions/schedules` and the schedule
//! lifecycle actions at `POST /api/ingestions/actions`.
//!
//! Port of verevonv2's `app/api/ingestions/schedules/route.ts` +
//! `app/api/ingestions/actions/route.ts`. Schedules are normalized into the
//! SPA's `ScheduleItem`; actions proxy quarry-control's schedule control verbs.

use axum::{
    extract::{Extension, State},
    response::Response,
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{config::AppState, envelope::unwrap_data, middleware::AuthenticatedUser};

use super::shared::{
    array_at, cookie_header, created, date_millis, first_str, forward, normalize_batch_urls,
    normalize_target, obj_or_empty, okay, quarry_call, quarry_token, str_at, str_or_null,
    validation,
};

// ── List ────────────────────────────────────────────────────────────────────

pub(super) async fn list_schedules(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: axum::http::HeaderMap,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;

    let (status, body) = quarry_call(
        &state,
        Method::GET,
        "/v1/schedules?limit=100",
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }

    let mut schedules: Vec<Value> = array_at(&unwrap_data(&body), "items")
        .iter()
        .map(normalize_schedule)
        .collect();
    // Soonest-upcoming first, matching v2's nextRunAt descending sort.
    schedules.sort_by(|a, b| {
        let am = a
            .get("nextRunAt")
            .and_then(Value::as_str)
            .map(date_millis)
            .unwrap_or(0);
        let bm = b
            .get("nextRunAt")
            .and_then(Value::as_str)
            .map(date_millis)
            .unwrap_or(0);
        bm.cmp(&am)
    });

    okay(Value::Array(schedules))
}

/// Normalize a quarry schedule into the SPA's `ScheduleItem`.
fn normalize_schedule(schedule: &Value) -> Value {
    let config = obj_or_empty(schedule, "config");
    let target = first_str(&config, &["url"])
        .or_else(|| {
            config
                .get("urls")
                .and_then(Value::as_array)
                .map(|urls| format!("{} URLs", urls.len()))
        })
        .or_else(|| first_str(&config, &["query"]))
        .unwrap_or_else(|| "Scheduled ingestion".to_owned());

    json!({
        "id": str_at(schedule, "schedule_id"),
        "name": str_at(schedule, "name"),
        "kind": str_at(schedule, "kind"),
        "status": str_at(schedule, "status"),
        "cron": str_or_null(schedule, "cron"),
        "scheduleAt": str_or_null(schedule, "schedule_at"),
        "createdAt": str_at(schedule, "created_at"),
        "lastRunAt": str_or_null(schedule, "last_run_at"),
        "nextRunAt": str_or_null(schedule, "next_run_at"),
        "target": target,
        "config": config,
    })
}

// ── Create ────────────────────────────────────────────────────────────────

pub(crate) async fn create_schedule(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;

    let (Some(name), Some(kind)) = (first_str(&body, &["name"]), first_str(&body, &["kind"]))
    else {
        return validation("Schedule name and kind are required.");
    };
    let cron = first_str(&body, &["cron"]);
    let schedule_at = first_str(&body, &["scheduleAt", "schedule_at"]);
    if cron.is_none() && schedule_at.is_none() {
        return validation("Provide either a cron expression or a scheduled timestamp.");
    }

    let config = match build_schedule_config(&kind, &body) {
        Ok(config) => config,
        Err(message) => return validation(&message),
    };

    let mut request = serde_json::Map::new();
    request.insert("name".into(), Value::String(name));
    request.insert("kind".into(), Value::String(kind));
    if let Some(cron) = cron {
        request.insert("cron".into(), Value::String(cron));
    }
    if let Some(schedule_at) = schedule_at {
        request.insert("schedule_at".into(), Value::String(schedule_at));
    }
    request.insert(
        "overlap_policy".into(),
        Value::String(
            first_str(&body, &["overlapPolicy", "overlap_policy"])
                .unwrap_or_else(|| "skip".to_owned()),
        ),
    );
    request.insert("config".into(), config);

    let (status, resp) = quarry_call(
        &state,
        Method::POST,
        "/v1/schedules",
        Some(Value::Object(request)),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, resp);
    }

    created(normalize_schedule(&unwrap_data(&resp)))
}

/// Build quarry's schedule `config`: a batch schedule carries normalized `urls`;
/// every other kind carries an optional SSRF-guarded `url` plus passthrough
/// `max_pages` / `schema`. Mirrors v2's `config` branch.
fn build_schedule_config(kind: &str, body: &Value) -> Result<Value, String> {
    if kind == "batch" {
        let raw = body
            .get("urls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let urls = normalize_batch_urls(&raw)
            .map_err(|_| "At least one URL is required for batch schedules.".to_owned())?;
        return Ok(json!({ "urls": urls }));
    }

    let mut config = serde_json::Map::new();
    if let Some(raw) = first_str(body, &["targetUrl", "target_url"]) {
        config.insert("url".into(), Value::String(normalize_target(&raw)?));
    }
    if let Some(max_pages) = body
        .get("maxPages")
        .or_else(|| body.get("max_pages"))
        .and_then(Value::as_u64)
    {
        config.insert("max_pages".into(), json!(max_pages));
    }
    if let Some(schema) = body.get("schema").filter(|v| !v.is_null()) {
        config.insert("schema".into(), schema.clone());
    }
    Ok(Value::Object(config))
}

// ── Schedule lifecycle actions ───────────────────────────────────────────────

pub(crate) async fn schedule_actions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let token = token.as_deref();
    let user_id = user.user_id.as_str();

    let (Some(action), Some(schedule_id)) = (
        first_str(&body, &["action"]),
        first_str(&body, &["scheduleId", "schedule_id"]),
    ) else {
        return validation("Action and scheduleId are required.");
    };
    let encoded = urlencoding::encode(&schedule_id);

    match action.as_str() {
        "pause_schedule" => {
            action_post(
                &state,
                &format!("/v1/schedules/{encoded}/pause"),
                None,
                token,
                user_id,
            )
            .await
        }
        "unpause_schedule" => {
            action_post(
                &state,
                &format!("/v1/schedules/{encoded}/unpause"),
                None,
                token,
                user_id,
            )
            .await
        }
        "trigger_schedule" => {
            action_post(
                &state,
                &format!("/v1/schedules/{encoded}/trigger"),
                None,
                token,
                user_id,
            )
            .await
        }
        "backfill_schedule" => {
            let (Some(start), Some(end)) = (
                first_str(&body, &["startAt", "start_at"]),
                first_str(&body, &["endAt", "end_at"]),
            ) else {
                return validation("Backfill requires startAt and endAt.");
            };
            action_post(
                &state,
                &format!("/v1/schedules/{encoded}/backfill"),
                Some(json!({ "start_at": start, "end_at": end, "overlap_policy": "allow" })),
                token,
                user_id,
            )
            .await
        }
        "delete_schedule" => {
            let (status, resp) = quarry_call(
                &state,
                Method::DELETE,
                &format!("/v1/schedules/{encoded}"),
                None,
                token,
                user_id,
            )
            .await;
            if !status.is_success() {
                return forward(status, resp);
            }
            okay(json!({ "deleted": true, "scheduleId": schedule_id }))
        }
        _ => validation("Unsupported ingestion action."),
    }
}

/// POST a schedule control verb and return the (unwrapped) updated schedule.
async fn action_post(
    state: &AppState,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
    user_id: &str,
) -> Response {
    let (status, resp) = quarry_call(state, Method::POST, path, body, token, user_id).await;
    if !status.is_success() {
        return forward(status, resp);
    }
    okay(unwrap_data(&resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_schedule_targets_url_then_urls_then_query() {
        let from_url = normalize_schedule(&json!({
            "schedule_id": "s1", "name": "Nightly", "kind": "crawl", "status": "active",
            "cron": "0 2 * * *", "created_at": "x", "config": { "url": "https://vg.no" }
        }));
        assert_eq!(from_url["id"], "s1");
        assert_eq!(from_url["target"], "https://vg.no");
        assert_eq!(from_url["cron"], "0 2 * * *");
        assert_eq!(from_url["nextRunAt"], Value::Null);

        let from_urls = normalize_schedule(&json!({
            "schedule_id": "s2", "name": "Batch", "kind": "batch", "status": "active",
            "created_at": "x", "config": { "urls": ["https://a", "https://b", "https://c"] }
        }));
        assert_eq!(from_urls["target"], "3 URLs");

        let fallback = normalize_schedule(&json!({
            "schedule_id": "s3", "name": "X", "kind": "search", "status": "active",
            "created_at": "x", "config": {}
        }));
        assert_eq!(fallback["target"], "Scheduled ingestion");
    }

    #[test]
    fn build_schedule_config_batch_requires_valid_urls() {
        assert!(build_schedule_config("batch", &json!({ "urls": [] })).is_err());
        let config =
            build_schedule_config("batch", &json!({ "urls": ["https://vg.no/a"] })).unwrap();
        assert_eq!(config["urls"][0], "https://vg.no/a");
    }

    #[test]
    fn build_schedule_config_non_batch_normalizes_target_url() {
        let config = build_schedule_config(
            "crawl",
            &json!({ "targetUrl": "https://vg.no", "maxPages": 7 }),
        )
        .unwrap();
        assert_eq!(config["url"], "https://vg.no/");
        assert_eq!(config["max_pages"], 7);

        assert!(
            build_schedule_config("crawl", &json!({ "targetUrl": "http://localhost" })).is_err()
        );
    }
}
