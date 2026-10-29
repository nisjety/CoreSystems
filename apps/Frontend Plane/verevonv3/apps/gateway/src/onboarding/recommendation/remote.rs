use axum::http::HeaderMap;
use chrono::Utc;
use serde_json::{json, Value};

use crate::{
    audience_tokens::get_audience_token,
    config::AppState,
    contracts::{PlanRecommendation, RecommendContext},
    middleware::AuthenticatedUser,
};

use super::{
    normalize::{align_source_proof_points, string_list},
    plan_id,
};

pub(crate) async fn fetch_remote_recommendation(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    context: &RecommendContext,
) -> Option<PlanRecommendation> {
    // Authenticate to model-gateway exactly like the chat path: a short-lived
    // `model-plane` audience JWT minted from the caller's session cookie, plus
    // the verified user id/role headers. Without the Bearer token,
    // model-gateway's `require_auth` rejects the call ("missing or invalid
    // Authorization header") and the recommendation silently degrades to the
    // local heuristic — i.e. system-based, not AI-based.
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let token = match get_audience_token(state, &user.user_id, cookie, "model-plane").await {
        Some(token) => Some(token),
        None if !state.model_gateway_dev_bearer.is_empty() => {
            Some(state.model_gateway_dev_bearer.clone())
        }
        None => None,
    };
    let user_role = user
        .auth_role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or("member");

    let mut request = state
        .client
        .post(&state.model_recommend_url)
        .header("x-internal-api-key", &state.internal_api_key)
        .header("x-user-id", &user.user_id)
        .header("x-user-role", user_role)
        .json(&json!({
            "context": context,
            "locale": context.locale.clone().unwrap_or_else(|| "en".into())
        }));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.json::<Value>().await.ok()?;
    let raw = body.get("recommendation").unwrap_or(&body);
    let locale = context.locale.as_deref().unwrap_or("en");
    Some(PlanRecommendation {
        plan_id: plan_id(raw.get("planId").and_then(Value::as_str).unwrap_or("trial")),
        reason: raw.get("reason").and_then(Value::as_str)?.to_owned(),
        summary: raw.get("summary").and_then(Value::as_str)?.to_owned(),
        proof_points: align_source_proof_points(
            string_list(raw.get("proofPoints")),
            context,
            locale,
        ),
        scope_signals: string_list(raw.get("scopeSignals")),
        opportunities: string_list(raw.get("opportunities")),
        generated_at: raw
            .get("generatedAt")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Utc::now().to_rfc3339()),
        source: "model",
    })
}
