use chrono::Utc;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::{PlanRecommendation, RecommendContext},
};

use super::{normalize::string_list, plan_id};

pub(crate) async fn fetch_remote_recommendation(
    state: &AppState,
    context: &RecommendContext,
) -> Option<PlanRecommendation> {
    let response = state
        .client
        .post(&state.model_recommend_url)
        .header("x-internal-api-key", &state.internal_api_key)
        .json(&json!({
            "context": context,
            "locale": context.locale.clone().unwrap_or_else(|| "en".into())
        }))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.json::<Value>().await.ok()?;
    let raw = body.get("recommendation").unwrap_or(&body);
    Some(PlanRecommendation {
        plan_id: plan_id(raw.get("planId").and_then(Value::as_str).unwrap_or("trial")),
        reason: raw.get("reason").and_then(Value::as_str)?.to_owned(),
        summary: raw.get("summary").and_then(Value::as_str)?.to_owned(),
        proof_points: string_list(raw.get("proofPoints")),
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
