use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, envelope::error, middleware::AuthenticatedUser, upstream::proxy_json,
};

use super::shared::{actor_for, require_org_admin, GatewayJsonResponse};

/// PATCH /api/v1/orgs/:id/settings — org-admin-gated organization settings.
///
/// Currently carries the interactive Zero-Data-Retention posture. The value is
/// persisted as durable organization intent in org-core; live token-issue
/// enforcement remains with auth-core's managed, attested retention policy.
/// This route deliberately never becomes a per-request ZDR override: it only
/// records the org's chosen posture, gated to org owners/admins.
pub(crate) async fn update_org_settings(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    let zdr = body.get("zeroDataRetention").and_then(Value::as_bool);
    let support_ai_mode = body.get("supportAiMode").and_then(Value::as_str);
    if (zdr.is_some() && support_ai_mode.is_some()) || (zdr.is_none() && support_ai_mode.is_none())
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "validation_error",
                "provide exactly one of zeroDataRetention or supportAiMode.",
            )),
        );
    }
    if let Some(mode) = support_ai_mode {
        if !matches!(mode, "off" | "assist" | "review") {
            return (
                StatusCode::BAD_REQUEST,
                Json(error(
                    "validation_error",
                    "supportAiMode must be off, assist, or review.",
                )),
            );
        }
    }
    let url = format!(
        "{}/api/v1/organizations/{}/settings",
        state.org_core_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::PATCH,
        &url,
        Some(match (zdr, support_ai_mode) {
            (Some(zdr), None) => json!({ "zeroDataRetention": zdr }),
            (None, Some(mode)) => json!({ "supportAiMode": mode }),
            _ => unreachable!(),
        }),
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}
