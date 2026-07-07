use axum::{
    extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Extension, Json,
};
use serde_json::json;
use std::collections::HashMap;

use crate::{
    config::AppState,
    contracts::{RecommendPlanRequest, RecommendationText, TranslateRecommendationRequest},
    domains::chat::shared,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    onboarding::recommendation::{build_local_recommendation, fetch_remote_recommendation},
};

pub(crate) async fn recommend_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<RecommendPlanRequest>,
) -> impl IntoResponse {
    // The onboarding router is behind `require_session`, so the caller's
    // identity + cookie are available here — pass them to the remote call so it
    // can mint a model-plane token and reach the AI recommender (the previous
    // signature had no auth, so the model call 401'd and always fell back to
    // the local heuristic).
    let local = build_local_recommendation(&input.context);
    let remote = fetch_remote_recommendation(&state, &user, &headers, &input.context).await;
    let recommendation = select_recommendation(local, remote);
    (
        StatusCode::OK,
        Json(ok(json!({ "recommendation": recommendation }))),
    )
}

pub(crate) async fn translate_recommendation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<TranslateRecommendationRequest>,
) -> impl IntoResponse {
    let target_language = normalize_locale(&input.target_language);
    if !matches!(target_language.as_deref(), Some("en" | "nb")) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_target_language",
                "targetLanguage must be either en or nb.",
            )),
        );
    }

    let items = translation_items(&input.recommendation);
    if items.is_empty() {
        return (
            StatusCode::OK,
            Json(ok(json!({ "translation": input.recommendation }))),
        );
    }

    let token = shared::model_token(&state, &user, &headers).await;
    let url = format!("{}/v1/ai/translate", state.model_gateway_url);
    let user_role = user
        .auth_role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or("member");
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let mut request = state
        .client
        .post(url)
        .header("x-user-id", &user.user_id)
        .header("x-user-role", user_role)
        .json(&json!({
            "operation": "batch",
            "source_language": input.source_language.as_deref().and_then(normalize_locale).unwrap_or_default(),
            "target_language": target_language.unwrap_or_default(),
            "items": items,
        }));
    if !org_id.trim().is_empty() {
        request = request.header("x-org-id", org_id);
    }
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(reason) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(error("translation_unavailable", reason.to_string())),
            )
        }
    };
    if !response.status().is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "translation_failed",
                format!(
                    "Model Plane translation failed with status {}.",
                    response.status()
                ),
            )),
        );
    }

    let body = match response.json::<serde_json::Value>().await {
        Ok(body) => body,
        Err(reason) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(error("translation_invalid_response", reason.to_string())),
            )
        }
    };
    let translations = translation_map(&body);
    let translated = apply_translation(input.recommendation, &translations);

    (
        StatusCode::OK,
        Json(ok(json!({
            "translation": translated,
            "modelUsed": body.get("model_used").and_then(serde_json::Value::as_str).unwrap_or_default(),
            "providerUsed": body.get("provider_used").and_then(serde_json::Value::as_str).unwrap_or_default(),
        }))),
    )
}

fn select_recommendation(
    local: crate::contracts::PlanRecommendation,
    remote: Option<crate::contracts::PlanRecommendation>,
) -> crate::contracts::PlanRecommendation {
    match remote {
        Some(remote) if plan_rank(remote.plan_id) >= plan_rank(local.plan_id) => remote,
        _ => local,
    }
}

fn plan_rank(plan_id: &str) -> u8 {
    match plan_id {
        "trial" => 0,
        "hobby" => 1,
        "standard" => 2,
        "pro" => 3,
        "enterprise" => 4,
        _ => 0,
    }
}

fn normalize_locale(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "en" | "en-us" | "en-gb" => Some("en"),
        "nb" | "no" | "nb-no" | "no-no" => Some("nb"),
        _ => None,
    }
}

fn translation_items(input: &RecommendationText) -> Vec<serde_json::Value> {
    let mut items = Vec::new();
    push_translation_item(&mut items, "reason", &input.reason);
    push_translation_item(&mut items, "summary", &input.summary);
    for (index, item) in input.proof_points.iter().enumerate() {
        push_translation_item(&mut items, &format!("proofPoints:{index}"), item);
    }
    for (index, item) in input.scope_signals.iter().enumerate() {
        push_translation_item(&mut items, &format!("scopeSignals:{index}"), item);
    }
    for (index, item) in input.opportunities.iter().enumerate() {
        push_translation_item(&mut items, &format!("opportunities:{index}"), item);
    }
    items
}

fn push_translation_item(items: &mut Vec<serde_json::Value>, id: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    items.push(json!({ "id": id, "text": text }));
}

fn translation_map(body: &serde_json::Value) -> HashMap<String, String> {
    body.get("translations")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("id").and_then(serde_json::Value::as_str)?;
                    let text = item
                        .get("translated_text")
                        .and_then(serde_json::Value::as_str)?
                        .trim();
                    if text.is_empty() {
                        return None;
                    }
                    Some((id.to_owned(), text.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn apply_translation(
    input: RecommendationText,
    translations: &HashMap<String, String>,
) -> RecommendationText {
    RecommendationText {
        reason: translated_scalar(translations, "reason", input.reason),
        summary: translated_scalar(translations, "summary", input.summary),
        proof_points: translated_list(translations, "proofPoints", input.proof_points),
        scope_signals: translated_list(translations, "scopeSignals", input.scope_signals),
        opportunities: translated_list(translations, "opportunities", input.opportunities),
    }
}

fn translated_scalar(
    translations: &HashMap<String, String>,
    key: &str,
    fallback: String,
) -> String {
    translations.get(key).cloned().unwrap_or(fallback)
}

fn translated_list(
    translations: &HashMap<String, String>,
    prefix: &str,
    fallbacks: Vec<String>,
) -> Vec<String> {
    fallbacks
        .into_iter()
        .enumerate()
        .map(|(index, fallback)| {
            translations
                .get(&format!("{prefix}:{index}"))
                .cloned()
                .unwrap_or(fallback)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::contracts::{PlanRecommendation, RecommendationText};

    use super::{apply_translation, select_recommendation, translation_items, translation_map};

    #[test]
    fn deterministic_plan_is_a_floor_for_model_recommendations() {
        let selected = select_recommendation(
            recommendation("pro", "local"),
            Some(recommendation("standard", "model")),
        );

        assert_eq!(selected.plan_id, "pro");
        assert_eq!(selected.source, "local");
    }

    #[test]
    fn model_can_choose_same_or_higher_plan_than_the_floor() {
        let selected = select_recommendation(
            recommendation("standard", "local"),
            Some(recommendation("pro", "model")),
        );

        assert_eq!(selected.plan_id, "pro");
        assert_eq!(selected.source, "model");
    }

    #[test]
    fn recommendation_translation_items_have_stable_field_ids() {
        let items = translation_items(&RecommendationText {
            reason: "reason".into(),
            summary: "summary".into(),
            proof_points: vec!["proof".into()],
            scope_signals: vec!["scope".into()],
            opportunities: vec!["opportunity".into()],
        });

        let ids = items
            .iter()
            .filter_map(|item| item.get("id").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "reason",
                "summary",
                "proofPoints:0",
                "scopeSignals:0",
                "opportunities:0"
            ]
        );
    }

    #[test]
    fn applies_translations_by_id_and_keeps_missing_originals() {
        let body = json!({
            "translations": [
                { "id": "summary", "translated_text": "translated summary" },
                { "id": "proofPoints:0", "translated_text": "translated proof" }
            ]
        });
        let translated = apply_translation(
            RecommendationText {
                reason: "original reason".into(),
                summary: "original summary".into(),
                proof_points: vec!["original proof".into()],
                scope_signals: vec!["original scope".into()],
                opportunities: vec![],
            },
            &translation_map(&body),
        );

        assert_eq!(translated.reason, "original reason");
        assert_eq!(translated.summary, "translated summary");
        assert_eq!(translated.proof_points, vec!["translated proof"]);
        assert_eq!(translated.scope_signals, vec!["original scope"]);
    }

    fn recommendation(plan_id: &'static str, source: &'static str) -> PlanRecommendation {
        PlanRecommendation {
            plan_id,
            reason: "reason".to_owned(),
            summary: "summary".to_owned(),
            proof_points: vec![],
            scope_signals: vec![],
            opportunities: vec![],
            generated_at: "2026-07-05T00:00:00Z".to_owned(),
            source,
        }
    }
}
