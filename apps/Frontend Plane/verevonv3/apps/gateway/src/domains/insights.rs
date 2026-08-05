//! Insights (Market Intelligence) read surface.
//!
//! Proxies insight-core's connector registry so the SPA can show which sources
//! could feed market-intelligence metrics. The org is resolved server-side from
//! the validated session (never client-supplied); insight-core requires the
//! `x-org-id` header, so an empty org returns an empty success envelope.
//!
//! The connector registry is normalized to the SPA's compact
//! `{id, kind, label, status}` shape, and the per-org metric overview to a
//! compact `{scorecards, surfaces, generated_at}` shape. Both routes resolve the
//! org from the session and NEVER trust a client-supplied org (IDOR-clean).
//!
//! HONESTY: the overview proxy passes through ONLY the real values insight-core
//! recorded — it never fabricates a metric. When the org has no recorded metrics
//! the upstream returns zero scorecards, and the SPA maps that to an honest empty
//! / `not_connected` state. The gateway does not invent a `live` label.

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

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/insights/connectors", get(list_connectors))
        .route("/api/v1/insights/overview", get(overview))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Normalize an insight-core `ConnectorSlot` to the SPA's compact shape. Missing
/// fields collapse to empty strings — never a fabricated label/status/kind.
fn normalize_connector(slot: &Value) -> Value {
    let field = |key: &str| {
        slot.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    json!({
        "id": field("type"),
        "kind": field("surface"),
        "label": field("display_name"),
        "status": field("status"),
    })
}

async fn list_connectors(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;

    // insight-core requires an x-org-id; without an authorized org there is
    // nothing to show, so return an empty success envelope rather than a 400.
    if org_id.trim().is_empty() {
        return Json(json!({ "data": [], "meta": { "count": 0 }, "error": null })).into_response();
    }

    let url = format!("{}/api/v1/insights/connectors", state.insight_core_url);
    // proxy_json injects the shared internal-api-key + the server-set x-org-id.
    let (status, Json(body)) =
        proxy_json(&state, Method::GET, &url, None, Some(&org_id), None, None).await;
    if !status.is_success() {
        return (status, Json(body)).into_response();
    }

    let connectors: Vec<Value> = body
        .get("data")
        .and_then(Value::as_array)
        .map(|slots| slots.iter().map(normalize_connector).collect())
        .unwrap_or_default();
    let count = connectors.len();

    Json(json!({ "data": connectors, "meta": { "count": count }, "error": null })).into_response()
}

/// The honest empty overview: no recorded metrics for this org. Carries empty
/// arrays and a zero source count so the SPA renders an explicit empty /
/// `not_connected` state — NEVER a fabricated metric value.
fn empty_overview() -> Value {
    json!({
        "scorecards": [],
        "surfaces": [],
        "generated_at": "",
        "source_count": 0,
    })
}

/// Normalize an insight-core `Scorecard` to the SPA's compact shape. Numeric and
/// citation fields are passed through verbatim from the real rollup — missing
/// fields collapse to empty/zero, never a fabricated value.
fn normalize_scorecard(card: &Value) -> Value {
    let str_field = |key: &str| {
        card.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    json!({
        "id": str_field("id"),
        "label": str_field("label"),
        "surface": str_field("surface"),
        "metric": str_field("metric"),
        "value": card.get("value").and_then(Value::as_f64).unwrap_or(0.0),
        "unit": str_field("unit"),
        // Real producer citation threaded from insight-core; empty when the
        // metric was unattributed — never fabricated.
        "source": str_field("source"),
    })
}

/// Normalize an insight-core `Overview` (`data` payload) to the SPA's compact
/// metric shape. Only the real scorecards/surfaces insight-core produced are
/// passed through; `source_count` is the count of REAL scorecards so the SPA can
/// decide `live` vs `empty` from produced rows alone.
fn normalize_overview(overview: &Value) -> Value {
    let scorecards: Vec<Value> = overview
        .get("scorecards")
        .and_then(Value::as_array)
        .map(|cards| cards.iter().map(normalize_scorecard).collect())
        .unwrap_or_default();
    let surfaces = overview
        .get("surfaces")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let generated_at = overview
        .get("generated_at")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source_count = scorecards.len();
    json!({
        "scorecards": scorecards,
        "surfaces": surfaces,
        "generated_at": generated_at,
        "source_count": source_count,
    })
}

async fn overview(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;

    // No authorized org → an honest empty overview (the SPA renders
    // `not_connected`), never fabricated metrics and never a 400.
    if org_id.trim().is_empty() {
        return Json(json!({ "data": empty_overview(), "error": null })).into_response();
    }

    let url = format!("{}/api/v1/insights/overview", state.insight_core_url);
    // proxy_json injects the shared internal-api-key + the server-set x-org-id.
    let (status, Json(body)) =
        proxy_json(&state, Method::GET, &url, None, Some(&org_id), None, None).await;
    if !status.is_success() {
        return (status, Json(body)).into_response();
    }

    let normalized = body
        .get("data")
        .map(normalize_overview)
        .unwrap_or_else(empty_overview);

    Json(json!({ "data": normalized, "error": null })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_connector_maps_slot_fields_to_spa_shape() {
        let slot = json!({
            "type": "social-core",
            "display_name": "Verevon Social",
            "surface": "social",
            "status": "native",
            "authorization": "internal_application_plane_event",
            "contracts": [],
        });
        assert_eq!(
            normalize_connector(&slot),
            json!({ "id": "social-core", "kind": "social", "label": "Verevon Social", "status": "native" })
        );
    }

    #[test]
    fn normalize_connector_collapses_missing_fields_to_empty_strings() {
        // Never fabricate a label/status/kind the upstream did not provide.
        assert_eq!(
            normalize_connector(&json!({ "type": "x" })),
            json!({ "id": "x", "kind": "", "label": "", "status": "" })
        );
    }

    #[test]
    fn normalize_scorecard_passes_through_real_values_and_citation() {
        let card = json!({
            "id": "inbox.ai_actions_executed",
            "label": "inbox ai actions executed",
            "surface": "inbox",
            "metric": "ai_actions_executed",
            "value": 7.0,
            "unit": "count",
            "source": "conversation-core",
        });
        assert_eq!(
            normalize_scorecard(&card),
            json!({
                "id": "inbox.ai_actions_executed",
                "label": "inbox ai actions executed",
                "surface": "inbox",
                "metric": "ai_actions_executed",
                "value": 7.0,
                "unit": "count",
                "source": "conversation-core",
            })
        );
    }

    #[test]
    fn normalize_scorecard_collapses_missing_value_and_source_to_zero_and_empty() {
        // An unattributed / valueless scorecard must NOT gain a fabricated value
        // or producer citation.
        assert_eq!(
            normalize_scorecard(&json!({ "id": "x.y", "metric": "y", "surface": "x" })),
            json!({
                "id": "x.y",
                "label": "",
                "surface": "x",
                "metric": "y",
                "value": 0.0,
                "unit": "",
                "source": "",
            })
        );
    }

    #[test]
    fn normalize_overview_counts_real_scorecards_only() {
        let overview = json!({
            "generated_at": "2026-06-20T10:00:00Z",
            "surfaces": [{ "surface": "inbox", "total_events": 7 }],
            "scorecards": [
                { "id": "inbox.a", "surface": "inbox", "metric": "a", "value": 4.0, "source": "conversation-core" },
                { "id": "inbox.b", "surface": "inbox", "metric": "b", "value": 3.0 },
            ],
        });
        let normalized = normalize_overview(&overview);
        assert_eq!(normalized["source_count"], json!(2));
        assert_eq!(normalized["generated_at"], json!("2026-06-20T10:00:00Z"));
        assert_eq!(normalized["scorecards"].as_array().unwrap().len(), 2);
        // Citation threaded; the unattributed scorecard stays empty.
        assert_eq!(
            normalized["scorecards"][0]["source"],
            json!("conversation-core")
        );
        assert_eq!(normalized["scorecards"][1]["source"], json!(""));
    }

    #[test]
    fn normalize_overview_on_empty_org_has_zero_sources_and_no_fabricated_metrics() {
        // An overview with no recorded scorecards must yield zero real metrics —
        // the honest empty state the SPA renders as `not_connected`/empty.
        let normalized = normalize_overview(&json!({ "surfaces": [], "scorecards": [] }));
        assert_eq!(normalized["source_count"], json!(0));
        assert_eq!(normalized["scorecards"], json!([]));
        // empty_overview is the same honest shape for the no-org path.
        assert_eq!(empty_overview()["source_count"], json!(0));
        assert_eq!(empty_overview()["scorecards"], json!([]));
    }
}
