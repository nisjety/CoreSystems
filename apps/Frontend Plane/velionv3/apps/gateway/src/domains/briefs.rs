//! Org brief assembler (W3).
//!
//! Fans in insight-core's real per-org metric rollups and presents them as a
//! brief. The org is resolved server-side from the validated session (never
//! client-supplied); insight-core requires the `x-org-id` header.
//!
//! HONESTY GATE: a brief is labelled `live` only when enough real events have
//! accrued (>= [`BRIEF_MIN_EVENTS`]); below that it is labelled `preview` with a
//! disclosure. Either way it shows ONLY the real counts insight-core recorded —
//! it never infers a trend/forecast over empty or near-empty data. An empty org
//! returns a preview over an empty overview, never fabricated numbers.
//!
//! Scope: this MVP assembles the insight-rollup leg. The Quarry-change and
//! model-gateway-summary (with citations) legs are additive enrichment layered
//! on the same envelope in a follow-up; the Preview gate already protects honesty.

use axum::{
    extract::{Extension, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    middleware::{require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json},
};

/// Minimum real metric events before a brief is `live` rather than `preview`.
const BRIEF_MIN_EVENTS: u64 = 5;

const PREVIEW_DISCLOSURE: &str = "Preview — not enough recorded activity to brief on yet. Showing the real counts collected so far; no trends are inferred until more events accrue.";

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/briefs", get(daily_brief))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Assemble a brief from an insight-core overview body (`data` payload). Sums the
/// real per-surface `total_events`; below the threshold the brief is `preview`
/// with a disclosure. Surfaces/scorecards are passed through verbatim — never a
/// fabricated trend. Pure, so the gate is unit-testable.
fn assemble_brief(overview: &Value) -> Value {
    let surfaces = overview
        .get("surfaces")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total_events: u64 = surfaces
        .iter()
        .filter_map(|s| s.get("total_events").and_then(Value::as_u64))
        .sum();
    let scorecards = overview
        .get("scorecards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let generated_at = overview
        .get("generated_at")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let live = total_events >= BRIEF_MIN_EVENTS;
    let mut brief = json!({
        "state": if live { "live" } else { "preview" },
        "generated_at": generated_at,
        "total_events": total_events,
        "threshold": BRIEF_MIN_EVENTS,
        "surfaces": surfaces,
        "scorecards": scorecards,
    });
    if !live {
        brief["disclosure"] = json!(PREVIEW_DISCLOSURE);
    }
    brief
}

async fn daily_brief(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;

    // No authorized org → a preview over an empty overview (honest empty-state,
    // never fabricated numbers).
    if org_id.trim().is_empty() {
        return Json(json!({ "data": assemble_brief(&json!({})), "error": null })).into_response();
    }

    let url = format!("{}/api/v1/insights/overview", state.insight_core_url);
    let (status, Json(body)) =
        proxy_json(&state, Method::GET, &url, None, Some(&org_id), None, None).await;
    if !status.is_success() {
        return (status, Json(body)).into_response();
    }

    let overview = body.get("data").cloned().unwrap_or_else(|| json!({}));
    Json(json!({ "data": assemble_brief(&overview), "error": null })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_threshold_is_preview_with_disclosure() {
        let overview = json!({
            "generated_at": "2026-06-20T10:00:00Z",
            "surfaces": [{ "surface": "inbox", "total_events": 2, "metrics": [] }],
            "scorecards": [],
        });
        let brief = assemble_brief(&overview);
        assert_eq!(brief["state"], "preview");
        assert_eq!(brief["total_events"], 2);
        assert!(
            brief.get("disclosure").is_some(),
            "preview must carry a disclosure"
        );
    }

    #[test]
    fn at_threshold_is_live_without_disclosure() {
        let overview = json!({
            "surfaces": [
                { "surface": "inbox", "total_events": 4 },
                { "surface": "agents", "total_events": 1 },
            ],
            "scorecards": [],
        });
        let brief = assemble_brief(&overview);
        assert_eq!(brief["state"], "live");
        assert_eq!(brief["total_events"], 5);
        assert!(
            brief.get("disclosure").is_none(),
            "live brief must not carry the preview disclosure"
        );
    }

    #[test]
    fn empty_overview_is_preview_never_fabricates() {
        // The empty-org / no-data path must refuse to invent numbers.
        let brief = assemble_brief(&json!({}));
        assert_eq!(brief["state"], "preview");
        assert_eq!(brief["total_events"], 0);
        assert_eq!(brief["surfaces"], json!([]));
        assert_eq!(brief["scorecards"], json!([]));
        assert!(brief.get("disclosure").is_some());
    }
}
