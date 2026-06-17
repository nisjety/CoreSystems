//! Evidence timeline — `GET /api/ingestions/evidence?runId=…`.
//!
//! Port of velionv2's `app/api/ingestions/evidence/route.ts`: stream a run's
//! job-history events from quarry-control and derive a warnings list, returning
//! the SPA's `EvidenceTimeline`. The per-event objects are passed through
//! verbatim (their snake_case fields match the SPA's timeline shape).

use std::collections::HashMap;

use axum::{
    extract::{Extension, Query, State},
    http::HeaderMap,
    response::Response,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{config::AppState, envelope::unwrap_data, middleware::AuthenticatedUser};

use super::shared::{
    array_at, cookie_header, forward, okay, quarry_call, quarry_token, str_at, validation,
};

pub(super) async fn get_evidence(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let Some(run_id) = params
        .get("runId")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    else {
        return validation("runId is required.");
    };

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;

    let path = format!("/v1/runs/{}/events?limit=250", urlencoding::encode(run_id));
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &path,
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }

    let timeline = array_at(&unwrap_data(&body), "items");
    let warnings: Vec<Value> = timeline.iter().filter_map(to_warning).collect();

    okay(json!({
        "runId": run_id,
        "timeline": timeline,
        "warnings": warnings,
    }))
}

/// A timeline event becomes a warning when it reports a warn status or any
/// blocked URLs. Summary prefers the event payload's `error`/`message`.
fn to_warning(event: &Value) -> Option<Value> {
    let is_warn = event.get("status").and_then(Value::as_str) == Some("warn");
    let blocks = event.get("blocks").and_then(Value::as_i64).unwrap_or(0);
    if !is_warn && blocks <= 0 {
        return None;
    }

    let stage = str_at(event, "stage");
    let seq = event.get("seq").and_then(Value::as_i64).unwrap_or(0);
    let summary = event
        .get("payload")
        .and_then(|p| {
            p.get("error")
                .and_then(Value::as_str)
                .or_else(|| p.get("message").and_then(Value::as_str))
        })
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Warning during {stage}"));

    Some(json!({
        "id": format!("{}:{}", str_at(event, "run_id"), seq),
        "stage": stage,
        "summary": summary,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_warning_keeps_warn_and_blocked_events_only() {
        let ok_event =
            json!({ "run_id": "r", "stage": "fetch", "status": "ok", "seq": 1, "blocks": 0 });
        assert!(to_warning(&ok_event).is_none());

        let warn = json!({
            "run_id": "r", "stage": "fetch", "status": "warn", "seq": 2, "blocks": 0,
            "payload": { "error": "robots blocked" }
        });
        let w = to_warning(&warn).unwrap();
        assert_eq!(w["id"], "r:2");
        assert_eq!(w["stage"], "fetch");
        assert_eq!(w["summary"], "robots blocked");

        let blocked =
            json!({ "run_id": "r", "stage": "queue", "status": "ok", "seq": 3, "blocks": 2 });
        let b = to_warning(&blocked).unwrap();
        assert_eq!(b["summary"], "Warning during queue");
    }
}
