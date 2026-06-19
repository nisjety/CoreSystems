//! Insights (Market Intelligence) read surface.
//!
//! Proxies insight-core's connector registry so the SPA can show which sources
//! could feed market-intelligence metrics. The org is resolved server-side from
//! the validated session (never client-supplied); insight-core requires the
//! `x-org-id` header, so an empty org returns an empty success envelope.
//!
//! insight-core is registry-only today — it has NO metric producers — so this
//! domain exposes only the connector registry, normalized to the SPA's compact
//! `{id, kind, label, status}` shape. The SPA renders connectors but treats
//! metrics as an explicit "not yet reporting" empty state.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_connector_maps_slot_fields_to_spa_shape() {
        let slot = json!({
            "type": "social-core",
            "display_name": "Velion Social",
            "surface": "social",
            "status": "native",
            "authorization": "internal_application_plane_event",
            "contracts": [],
        });
        assert_eq!(
            normalize_connector(&slot),
            json!({ "id": "social-core", "kind": "social", "label": "Velion Social", "status": "native" })
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
}
