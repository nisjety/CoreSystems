//! Ingestion schedules — `GET/POST /api/ingestions/schedules`, the run history of
//! one schedule at `GET /api/ingestions/schedules/{id}/runs`, and the schedule
//! lifecycle actions at `POST /api/ingestions/actions`.
//!
//! Port of verevonv2's `app/api/ingestions/schedules/route.ts` +
//! `app/api/ingestions/actions/route.ts`. Schedules are normalized into the
//! SPA's `ScheduleItem`; actions proxy quarry-control's schedule control verbs.

use std::collections::HashMap;

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::{AppState, ScheduleCapabilities},
    envelope::{error, unwrap_data},
    middleware::AuthenticatedUser,
};

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
        "needsActivation": schedule.get("needs_activation").and_then(Value::as_bool).unwrap_or(false),
        "timezone": str_at(schedule, "timezone"),
        "overlapPolicy": str_at(schedule, "overlap_policy"),
        "skippedOverlapCount": schedule.get("skipped_overlap_count").and_then(Value::as_u64).unwrap_or(0),
        "target": target,
        "config": config,
    })
}

// ── Run history (WS15.S4e) ─────────────────────────────────────────────────

/// The runs a request gets when it names no `limit`. The SPA reads only the newest run,
/// so a short page is enough.
const RUN_PAGE: u32 = 10;
/// The most runs one request may ask the edge for.
const MAX_RUN_PAGE: u32 = 20;
/// The longest schedule id the gateway forwards. The edge mints `sch_` and a ULID, far shorter.
const MAX_SCHEDULE_ID_CHARS: usize = 80;
/// The terminal reason a skipped fire is registered with (control's `registerScheduledRun`).
const SCHEDULE_SKIPPED: &str = "schedule_skipped";

/// `GET /api/ingestions/schedules/{id}/runs?limit=` — the newest fires of one schedule, as the
/// SPA reads them. The edge takes the org from the verified token, so no scoping header is sent.
/// Each row is normalized to ids, status, reason and times: a fire's parameters (the schedule's
/// target among them) never leave the gateway.
pub(super) async fn list_schedule_runs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let Some(schedule_id) = valid_schedule_id(&id) else {
        return validation("A schedule id is required.");
    };
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;

    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &schedule_runs_path(&schedule_id, &params),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }

    let runs: Vec<Value> = array_at(&unwrap_data(&body), "items")
        .iter()
        .map(run_view)
        .collect();
    okay(Value::Array(runs))
}

/// The edge path of one schedule's run list. `limit` is clamped to 1..=20 (default 10), and the
/// id is percent-encoded so it cannot change the path. `cursor` is not forwarded: the SPA reads
/// one page.
fn schedule_runs_path(schedule_id: &str, params: &HashMap<String, String>) -> String {
    let limit = params
        .get("limit")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .map(|v| v.clamp(1, MAX_RUN_PAGE))
        .unwrap_or(RUN_PAGE);
    format!(
        "/v1/schedules/{}/runs?limit={limit}",
        urlencoding::encode(schedule_id)
    )
}

/// A schedule id as the edge mints it (`sch_` and a ULID): ASCII letters, digits and `_`. Anything
/// else is refused before a token is minted or a call is made.
fn valid_schedule_id(raw: &str) -> Option<String> {
    let id = raw.trim();
    let well_formed = !id.is_empty()
        && id.len() <= MAX_SCHEDULE_ID_CHARS
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    well_formed.then(|| id.to_owned())
}

/// One fire, normalized to the five fields the SPA reads. The specific skip code is recorded on the
/// `schedule_skipped` event, not on the run row, so a skipped fire reads its registration reason.
/// Quarry's job insert writes neither `terminal_reason` nor `finished_at`, so a skipped row that
/// lacks them takes the reason and its finish time (`created_at`, the registration instant) from its
/// own registration. `startedAt` is `created_at` too: a run row has no start time of its own.
fn run_view(run: &Value) -> Value {
    let status = str_at(run, "status");
    let mut reason = first_str(run, &["terminal_reason"]);
    let mut finished_at = first_str(run, &["finished_at"]);
    if status == "skipped" {
        reason = reason.or_else(|| Some(SCHEDULE_SKIPPED.to_owned()));
        finished_at = finished_at.or_else(|| first_str(run, &["created_at"]));
    }
    json!({
        "id": str_at(run, "id"),
        "status": status,
        "reason": reason,
        "startedAt": str_or_null(run, "created_at"),
        "finishedAt": finished_at,
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

    let (Some(name), Some(kind)) = (first_str(&body, &["name"]), first_str(&body, &["kind"]))
    else {
        return validation("Schedule name and kind are required.");
    };
    if let Some(rejected) = kind_rejection(&state.schedule_capabilities, &kind) {
        return rejected;
    }
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
    if state.schedule_capabilities.overlap {
        request.insert(
            "overlap_policy".into(),
            Value::String(
                first_str(&body, &["overlapPolicy", "overlap_policy"])
                    .unwrap_or_else(|| "skip".to_owned()),
            ),
        );
    }
    if let Some(timezone) = first_str(&body, &["timezone"]) {
        request.insert("timezone".into(), Value::String(timezone));
    }
    request.insert("config".into(), config);

    let token = quarry_token(&state, &user, &cookie).await;
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
            if !state.schedule_capabilities.trigger {
                return action_unavailable();
            }
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
            if !state.schedule_capabilities.backfill {
                return action_unavailable();
            }
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

// ── Capabilities (WS15.S0) ───────────────────────────────────────────────────

/// `GET /api/ingestions/schedules/capabilities`: what this installation can
/// honour. The SPA hides every control whose flag is off, so no control can
/// produce a 501 or a kinds 400.
pub(super) async fn schedule_capabilities(State(state): State<AppState>) -> Response {
    let caps = &state.schedule_capabilities;
    okay(json!({
        "trigger": caps.trigger,
        "backfill": caps.backfill,
        "overlap": caps.overlap,
        "kinds": caps.kinds,
        "needsActivation": caps.needs_activation,
        "jobControl": caps.job_control,
        "runs": caps.runs,
    }))
}

/// `Some(400)` unless `kind` is on the enabled list. It is checked before any
/// token mint or upstream call, so a refused kind never reaches quarry.
fn kind_rejection(caps: &ScheduleCapabilities, kind: &str) -> Option<Response> {
    if caps.kinds.iter().any(|enabled| enabled == kind) {
        return None;
    }
    Some(
        (
            StatusCode::BAD_REQUEST,
            Json(error(
                "unsupported_schedule_kind",
                "This schedule kind is not enabled on this installation.",
            )),
        )
            .into_response(),
    )
}

/// Typed 501 for a control verb whose backend is not enabled on this installation.
fn action_unavailable() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(error(
            "schedule_action_unavailable",
            "This schedule action is not enabled on this installation.",
        )),
    )
        .into_response()
}

#[cfg(test)]
mod capability_tests {
    use axum::{
        extract::{Extension, State},
        http::{HeaderMap, StatusCode},
        response::Response,
        Json,
    };
    use serde_json::{json, Value};

    use super::{create_schedule, kind_rejection, schedule_actions, schedule_capabilities};
    use crate::{
        config::{schedule_capabilities_from_env, ScheduleCapabilities},
        middleware::AuthenticatedUser,
    };

    fn caller() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-ws15-s0".into(),
            user_email: "ws15-s0@example.test".into(),
            user_name: "WS15 S0".into(),
            user_image: None,
            email_verified: true,
            auth_role: None,
            active_org_id: Some("org-ws15-s0".into()),
            authorized_membership: None,
        }
    }

    async fn body_json(response: Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[test]
    fn schedule_capabilities_default_is_all_false() {
        let defaults = ScheduleCapabilities::default();
        assert!(!defaults.trigger);
        assert!(!defaults.backfill);
        assert!(!defaults.overlap);
        assert!(!defaults.needs_activation);
        assert!(!defaults.runs);
        assert!(defaults.kinds.is_empty());

        assert_eq!(schedule_capabilities_from_env(None), defaults);
        assert_eq!(schedule_capabilities_from_env(Some("   ")), defaults);
        assert_eq!(schedule_capabilities_from_env(Some("{not json")), defaults);

        let crawl_only = schedule_capabilities_from_env(Some(r#"{"kinds":["crawl"]}"#));
        assert_eq!(crawl_only.kinds, vec!["crawl".to_owned()]);
        assert!(!crawl_only.trigger);
        assert!(!crawl_only.overlap);
        assert!(!crawl_only.runs);
        assert!(schedule_capabilities_from_env(Some(r#"{"runs":true}"#)).runs);
    }

    #[test]
    fn kind_rejection_follows_the_enabled_list() {
        let caps = ScheduleCapabilities {
            kinds: vec!["crawl".into()],
            ..Default::default()
        };
        assert!(kind_rejection(&caps, "crawl").is_none());
        assert!(kind_rejection(&caps, "batch").is_some());
        assert!(kind_rejection(&ScheduleCapabilities::default(), "crawl").is_some());
    }

    #[tokio::test]
    async fn create_schedule_rejects_kind_outside_capabilities() {
        let mut state = crate::tests::test_state(false);
        state.schedule_capabilities = ScheduleCapabilities {
            kinds: vec!["crawl".into()],
            ..Default::default()
        };
        let response = create_schedule(
            State(state),
            Extension(caller()),
            HeaderMap::new(),
            Json(json!({
                "name": "Nightly",
                "kind": "batch",
                "cron": "0 2 * * *",
                "urls": ["https://vg.no/a"],
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = body_json(response).await;
        assert_eq!(body["error"]["code"], "unsupported_schedule_kind");
    }

    #[tokio::test]
    async fn trigger_is_501_typed_when_disabled() {
        let state = crate::tests::test_state(false);
        assert!(!state.schedule_capabilities.trigger);
        assert!(!state.schedule_capabilities.backfill);

        let trigger = schedule_actions(
            State(state.clone()),
            Extension(caller()),
            HeaderMap::new(),
            Json(json!({ "action": "trigger_schedule", "scheduleId": "sched_1" })),
        )
        .await;
        assert_eq!(trigger.status(), StatusCode::NOT_IMPLEMENTED);
        let body = body_json(trigger).await;
        assert_eq!(body["error"]["code"], "schedule_action_unavailable");

        let backfill = schedule_actions(
            State(state),
            Extension(caller()),
            HeaderMap::new(),
            Json(json!({
                "action": "backfill_schedule",
                "scheduleId": "sched_1",
                "startAt": "2026-10-01T00:00:00Z",
                "endAt": "2026-10-02T00:00:00Z",
            })),
        )
        .await;
        assert_eq!(backfill.status(), StatusCode::NOT_IMPLEMENTED);
        let body = body_json(backfill).await;
        assert_eq!(body["error"]["code"], "schedule_action_unavailable");
    }

    #[tokio::test]
    async fn capabilities_endpoint_reports_the_configured_flags() {
        let mut state = crate::tests::test_state(false);
        state.schedule_capabilities = ScheduleCapabilities {
            kinds: vec!["crawl".into()],
            needs_activation: true,
            runs: true,
            ..Default::default()
        };
        let response = schedule_capabilities(State(state)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["data"]["trigger"], false);
        assert_eq!(body["data"]["backfill"], false);
        assert_eq!(body["data"]["overlap"], false);
        assert_eq!(body["data"]["kinds"], json!(["crawl"]));
        assert_eq!(body["data"]["needsActivation"], true);
        assert_eq!(body["data"]["jobControl"], false);
        assert_eq!(body["data"]["runs"], true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_schedule_carries_activation_timezone_and_overlap() {
        let item = normalize_schedule(&json!({
            "schedule_id": "sch_1",
            "name": "Nightly crawl",
            "kind": "crawl",
            "status": "paused",
            "cron": "0 7 * * *",
            "created_at": "2026-10-23T07:00:00Z",
            "timezone": "Europe/Oslo",
            "overlap_policy": "cancel",
            "needs_activation": true,
            "config": { "url": "https://example.com/docs" }
        }));
        assert_eq!(item["needsActivation"], true);
        assert_eq!(item["timezone"], "Europe/Oslo");
        assert_eq!(item["overlapPolicy"], "cancel");
        assert_eq!(item["status"], "paused");
        assert_eq!(item["target"], "https://example.com/docs");
    }

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
