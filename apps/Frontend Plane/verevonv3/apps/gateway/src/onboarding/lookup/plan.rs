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
    onboarding::recommendation::fetch_remote_recommendation,
};

pub(crate) async fn recommend_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(input): Json<RecommendPlanRequest>,
) -> impl IntoResponse {
    // Plan recommendation is a Model Plane decision. The gateway may carry the
    // validated user/session context to that service but must not substitute a
    // local pricing heuristic when the owner is unavailable.
    let Some(recommendation) =
        fetch_remote_recommendation(&state, &user, &headers, &input.context).await
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "recommendation_unavailable",
                "Plan recommendation is temporarily unavailable. Choose a plan manually and retry later.",
            )),
        );
    };
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
    if !matches!(target_language, Some("en" | "nb")) {
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
    // /v1/ai/translate extracts a VerifiedInferenceBearer from
    // `x-inference-authorization` (separate aud=inference-core user token,
    // same as the chat path); without it model-gateway rejects the call.
    let inference_token = match shared::required_inference_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error_detail) => return shared::delegated_auth_unavailable(error_detail),
    };
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
        .header(
            "x-inference-authorization",
            format!("Bearer {inference_token}"),
        )
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
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(translation_failure("translation_unavailable")),
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
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(translation_failure("translation_invalid_response")),
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

fn normalize_locale(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "en" | "en-us" | "en-gb" => Some("en"),
        "nb" | "no" | "nb-no" | "no-no" => Some("nb"),
        _ => None,
    }
}

fn translation_failure(code: &'static str) -> serde_json::Value {
    error(code, "The translation service is unavailable.")
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

    use crate::contracts::RecommendationText;

    use super::{apply_translation, translation_failure, translation_items, translation_map};

    #[test]
    fn translation_operational_failures_are_bounded() {
        let unavailable = translation_failure("translation_unavailable");
        let invalid = translation_failure("translation_invalid_response");
        assert_eq!(
            unavailable["error"]["message"],
            "The translation service is unavailable."
        );
        assert_eq!(
            invalid["error"]["message"],
            "The translation service is unavailable."
        );
        assert!(!unavailable.to_string().contains("http://"));
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
}
