//! Insights (Market Intelligence) read surface.
//!
//! The gateway authenticates the browser and resolves its active organization,
//! then preserves Insight Core's connector and overview contracts. It may
//! validate safe reporting filters, but it never selects a tenant, calculates
//! metrics, or invents an empty rollup.

use axum::{
    extract::{Extension, Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde_json::json;
use std::collections::HashMap;

use crate::{
    config::AppState,
    contracts::ActionActor,
    middleware::{require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/insights/connectors", get(list_connectors))
        .route("/api/v1/insights/overview", get(overview))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

fn insight_actor(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn organization_required() -> impl IntoResponse {
    (
        axum::http::StatusCode::CONFLICT,
        Json(json!({
            "error": {
                "code": "organization_required",
                "message": "Select an organization before viewing Insights."
            }
        })),
    )
}

async fn list_connectors(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return organization_required().into_response();
    }

    let actor = insight_actor(&user);
    let (status, Json(body)) = proxy_json(
        &state,
        Method::GET,
        &format!("{}/api/v1/insights/connectors", state.insight_core_url),
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;

    (status, Json(body)).into_response()
}

async fn overview(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;
    if org_id.trim().is_empty() {
        return organization_required().into_response();
    }

    let url = match overview_url(&state.insight_core_url, &params) {
        Ok(url) => url,
        Err(message) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({ "error": { "code": "invalid_insights_query", "message": message } })),
            )
                .into_response();
        }
    };
    let actor = insight_actor(&user);
    let (status, Json(body)) = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;

    (status, Json(body)).into_response()
}

// The browser may narrow an Insights read to a known reporting surface and
// time window, but must never select an organization or relay arbitrary query
// fields. Insight Core validates the actual date values.
fn overview_url(base: &str, params: &HashMap<String, String>) -> Result<String, &'static str> {
    const SURFACES: [&str; 8] = [
        "social",
        "inbox",
        "agents",
        "chat",
        "knowledge",
        "ingestion",
        "campaigns",
        "external_analytics",
    ];

    let mut query = Vec::new();
    if let Some(surface) = params
        .get("surface")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        if !SURFACES.contains(&surface) {
            return Err("surface must be a supported Insights reporting surface");
        }
        query.push(format!("surface={}", urlencoding::encode(surface)));
    }
    for key in ["from", "to"] {
        if let Some(value) = params
            .get(key)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            query.push(format!("{key}={}", urlencoding::encode(value)));
        }
    }
    if let Some(scope) = params
        .get("scope")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        if !matches!(scope, "organization" | "me") {
            return Err("scope must be organization or me");
        }
        if scope == "me" {
            query.push("scope=me".to_string());
        }
    }

    let path = format!("{}/api/v1/insights/overview", base.trim_end_matches('/'));
    Ok(if query.is_empty() {
        path
    } else {
        format!("{}?{}", path, query.join("&"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_url_forwards_only_allowlisted_measurement_filters() {
        let params = HashMap::from([
            ("surface".to_string(), "inbox".to_string()),
            ("from".to_string(), "2026-08-01T00:00:00Z".to_string()),
            ("to".to_string(), "2026-08-05T00:00:00Z".to_string()),
            ("org_id".to_string(), "forged-org".to_string()),
        ]);

        assert_eq!(
            overview_url("http://insight-core:3163/", &params),
            Ok("http://insight-core:3163/api/v1/insights/overview?surface=inbox&from=2026-08-01T00%3A00%3A00Z&to=2026-08-05T00%3A00%3A00Z".to_string())
        );
    }

    #[test]
    fn overview_url_allows_only_the_verified_user_scope_selector() {
        let params = HashMap::from([
            ("surface".to_string(), "chat".to_string()),
            ("scope".to_string(), "me".to_string()),
            ("user_id".to_string(), "forged-user".to_string()),
        ]);

        assert_eq!(
            overview_url("http://insight-core:3163", &params),
            Ok(
                "http://insight-core:3163/api/v1/insights/overview?surface=chat&scope=me"
                    .to_string()
            )
        );
    }

    #[test]
    fn overview_url_rejects_unknown_scope_and_surface() {
        assert_eq!(
            overview_url(
                "http://insight-core:3163",
                &HashMap::from([("scope".to_string(), "user-2".to_string())]),
            ),
            Err("scope must be organization or me")
        );
        assert_eq!(
            overview_url(
                "http://insight-core:3163",
                &HashMap::from([("surface".to_string(), "experiments".to_string())]),
            ),
            Err("surface must be a supported Insights reporting surface")
        );
    }
}
