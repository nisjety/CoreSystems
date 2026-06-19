use axum::{
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    audience_tokens::get_audience_token, config::AppState, envelope::error,
    middleware::AuthenticatedUser,
};

pub(super) fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

pub(crate) fn zdr_flag(headers: &HeaderMap) -> bool {
    headers
        .get("x-zdr")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
}

pub(crate) async fn model_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Option<String> {
    if let Some(token) =
        get_audience_token(state, &user.user_id, &cookie_header(headers), "model-plane").await
    {
        return Some(token);
    }

    if !state.model_gateway_dev_bearer.is_empty() {
        return Some(state.model_gateway_dev_bearer.clone());
    }

    dev_bypass_model_token(state, headers)
}

fn dev_bypass_model_token(state: &AppState, headers: &HeaderMap) -> Option<String> {
    if !state.allow_dev_auth_bypass {
        return None;
    }

    let raw = headers.get(AUTHORIZATION)?.to_str().ok()?.trim();
    raw.strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| *token == "dev-bypass")
        .map(str::to_owned)
}

pub(crate) async fn proxy_model_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let mut req = state
        .client
        .request(method, url)
        .header("x-user-id", &user.user_id);
    if !org_id.trim().is_empty() {
        req = req.header("x-org-id", org_id);
    }

    if let Some(token) = bearer_token {
        req = req.bearer_auth(token);
    }

    if let Some(b) = body {
        req = req.json(&b);
    }

    match req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let b = resp.json::<Value>().await.unwrap_or_else(|_| json!({}));
            (status, Json(b))
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(error("upstream_unavailable", e.to_string())),
        ),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use crate::{audience_tokens::new_audience_token_cache, cache::ResultCache, config::AppState};

    use super::dev_bypass_model_token;

    fn test_state(allow_dev_auth_bypass: bool) -> AppState {
        AppState {
            client: reqwest::Client::new(),
            streaming_client: reqwest::Client::new(),
            internal_api_key: "test-key".into(),
            auth_core_url: "http://127.0.0.1:1".into(),
            session_core_url: "http://127.0.0.1:1".into(),
            billing_core_url: "http://127.0.0.1:1".into(),
            org_core_url: "http://127.0.0.1:1".into(),
            integration_core_url: "http://127.0.0.1:1".into(),
            audit_core_url: "http://127.0.0.1:1".into(),
            user_core_url: "http://127.0.0.1:1".into(),
            graph_index_url: "http://127.0.0.1:1".into(),
            quarry_edge_url: "http://127.0.0.1:1".into(),
            quarry_control_url: "http://127.0.0.1:1".into(),
            model_recommend_url: "http://127.0.0.1:1".into(),
            model_gateway_url: "http://127.0.0.1:1".into(),
            model_gateway_dev_bearer: String::new(),
            inference_core_url: "http://127.0.0.1:1".into(),
            documents_api_url: "http://127.0.0.1:1".into(),
            retrieval_engine_url: "http://127.0.0.1:1".into(),
            wiki_store_url: "http://127.0.0.1:1".into(),
            embedding_engine_url: "http://127.0.0.1:1".into(),
            quickwit_adapter_url: "http://127.0.0.1:1".into(),
            qdrant_url: "http://127.0.0.1:1".into(),
            quickwit_url: "http://127.0.0.1:1".into(),
            finspo_core_url: "http://127.0.0.1:1".into(),
            imports_api_url: "http://127.0.0.1:1".into(),
            notification_core_url: "http://127.0.0.1:1".into(),
            information_core_url: "http://127.0.0.1:1".into(),
            conversation_core_url: "http://127.0.0.1:1".into(),
            social_core_url: "http://127.0.0.1:1".into(),
            searxng_url: "http://127.0.0.1:1".into(),
            autocomplete_core_url: "http://127.0.0.1:1".into(),
            autocomplete_token: String::new(),
            zammad_api_url: "http://127.0.0.1:1".into(),
            zammad_api_token: String::new(),
            audience_token_cache: new_audience_token_cache(),
            cache: ResultCache::disabled(),
            chat_history_store: crate::domains::chat::history::ChatHistoryStore::new(),
            studio_store: crate::domains::studio::StudioStore::new(),
            allow_dev_actor_headers: false,
            allow_dev_auth_bypass,
            enhanced_scrape_provider: String::new(),
            enhanced_scrape_api_key: String::new(),
            enhanced_scrape_zone: String::new(),
            enhanced_scrape_country: String::new(),
        }
    }

    #[test]
    fn dev_bypass_model_token_requires_explicit_gate_and_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer dev-bypass"),
        );

        assert_eq!(
            dev_bypass_model_token(&test_state(true), &headers),
            Some("dev-bypass".to_owned())
        );
        assert_eq!(dev_bypass_model_token(&test_state(false), &headers), None);

        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer other-token"),
        );
        assert_eq!(dev_bypass_model_token(&test_state(true), &headers), None);
    }
}
