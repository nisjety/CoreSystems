use std::{env, net::SocketAddr};

use anyhow::Result;
use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use tower_http::trace::TraceLayer;
use tracing::info;

mod audience_tokens;
mod auth;
mod cache;
mod config;
mod contracts;
mod domains;
mod envelope;
mod middleware;
mod onboarding;
mod public_url;
mod upstream;
mod utils;

use config::{build_cors_layer, build_state};
use middleware::strip_inbound_identity_headers;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let state = build_state().await?;
    let app = build_router(state);

    let port = env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3185);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!(%addr, "velion-gateway-rs listening");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}

/// Assemble the full gateway router. Extracted from `main` so tests can build it
/// and catch overlapping-route panics (axum panics at construction time when two
/// merged routers define the same method+path).
fn build_router(state: config::AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .merge(domains::actions::router(state.clone()))
        .merge(domains::agent_actions::router(state.clone()))
        .merge(domains::agents::router(state.clone()))
        .merge(domains::ag_ui::router(state.clone()))
        .merge(domains::ai::router(state.clone()))
        .merge(domains::auth::router(state.clone()))
        .merge(domains::billing::router(state.clone()))
        .merge(domains::chat::router(state.clone()))
        .merge(domains::finetune::router(state.clone()))
        .merge(domains::inbox::router(state.clone()))
        .merge(domains::information::router(state.clone()))
        .merge(domains::ingestions::router(state.clone()))
        .merge(domains::integrations::router(state.clone()))
        .merge(domains::knowledge::router(state.clone()))
        .merge(domains::navbar::router(state.clone()))
        .merge(domains::notifications::router(state.clone()))
        .merge(domains::onboarding::router(state.clone()))
        .merge(domains::orchestration::router(state.clone()))
        .merge(domains::orgs::router(state.clone()))
        .merge(domains::router_policy::router(state.clone()))
        .merge(domains::search::router(state.clone()))
        .merge(domains::settings::router(state.clone()))
        .merge(domains::social::router(state.clone()))
        .merge(domains::studio::router(state.clone()))
        .layer(axum::middleware::from_fn(strip_inbound_identity_headers))
        .layer(build_cors_layer())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "velion-gateway-rs" }))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use axum::Json;
    use reqwest::Method;
    use serde_json::{json, Value};

    use crate::{
        auth::actor_from_request,
        config::AppState,
        contracts::{ActionActor, WebsiteIngestRequest},
        envelope::{error, ok},
        onboarding::recommendation::plan_id,
        public_url::normalize_public_http_url,
        upstream::proxy_json,
    };

    fn test_state(allow_dev_actor_headers: bool) -> AppState {
        AppState {
            client: reqwest::Client::new(),
            streaming_client: reqwest::Client::new(),
            internal_api_key: "test-key".into(),
            auth_core_url: "http://127.0.0.1:1".into(),
            session_core_url: "http://127.0.0.1:1".into(),
            billing_core_url: "http://127.0.0.1:1".into(),
            org_core_url: "http://127.0.0.1:1".into(),
            integration_core_url: "http://127.0.0.1:1".into(),
            user_core_url: "http://127.0.0.1:1".into(),
            graph_index_url: "http://127.0.0.1:1".into(),
            quarry_edge_url: "http://127.0.0.1:1".into(),
            quarry_control_url: "http://127.0.0.1:1".into(),
            model_recommend_url: "http://127.0.0.1:1".into(),
            model_gateway_url: "http://127.0.0.1:1".into(),
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
            audience_token_cache: crate::audience_tokens::new_audience_token_cache(),
            cache: crate::cache::ResultCache::disabled(),
            social_store: crate::domains::social::SocialStore::new(),
            studio_store: crate::domains::studio::StudioStore::new(),
            allow_dev_actor_headers,
            allow_dev_auth_bypass: allow_dev_actor_headers,
            enhanced_scrape_provider: String::new(),
            enhanced_scrape_api_key: String::new(),
            enhanced_scrape_zone: String::new(),
            enhanced_scrape_country: String::new(),
        }
    }

    #[test]
    fn url_validation_allows_public_http_urls() {
        assert_eq!(
            normalize_public_http_url(" https://example.com/path?q=1 ").unwrap(),
            "https://example.com/path?q=1"
        );
    }

    #[test]
    fn url_validation_blocks_private_loopback_and_credential_urls() {
        for input in [
            "http://localhost",
            "http://127.0.0.1",
            "http://10.0.0.1",
            "http://172.16.0.1",
            "http://172.31.0.1",
            "http://192.168.1.1",
            "http://169.254.1.1",
            "http://[::1]",
            "http://user:pass@example.com",
            "ftp://example.com",
        ] {
            assert!(
                normalize_public_http_url(input).is_err(),
                "{input} should be blocked"
            );
        }
    }

    #[test]
    fn trusted_actor_headers_win_without_dev_actor_mode() {
        let mut headers = HeaderMap::new();
        headers.insert("x-user-id", HeaderValue::from_static("client-controlled"));
        headers.insert(
            "x-session-user-id",
            HeaderValue::from_static("trusted-user"),
        );
        headers.insert(
            "x-session-user-email",
            HeaderValue::from_static("trusted@example.com"),
        );

        let actor = actor_from_request(
            Some(&ActionActor {
                user_id: "body-user".into(),
                user_email: "body@example.com".into(),
                user_name: "Body User".into(),
                user_role: String::new(),
            }),
            Some(&headers),
            false,
        );

        assert_eq!(actor.user_id, "trusted-user");
        assert_eq!(actor.user_email, "trusted@example.com");
    }

    #[test]
    fn client_actor_is_ignored_unless_dev_actor_mode_is_explicit() {
        let mut headers = HeaderMap::new();
        headers.insert("x-user-id", HeaderValue::from_static("client-controlled"));

        let actor = actor_from_request(None, Some(&headers), false);
        assert_eq!(actor.user_id, "velion-v3-local-user");

        let dev_actor = actor_from_request(None, Some(&headers), true);
        assert_eq!(dev_actor.user_id, "client-controlled");
    }

    #[test]
    fn onboarding_payloads_accept_camel_and_snake_case_fields() {
        let camel = serde_json::from_value::<WebsiteIngestRequest>(json!({
            "orgId": "org_1",
            "url": "https://example.com",
            "maxPages": 4
        }))
        .unwrap();
        assert_eq!(camel.org_id, "org_1");
        assert_eq!(camel.max_pages, Some(4));

        let snake = serde_json::from_value::<WebsiteIngestRequest>(json!({
            "org_id": "org_2",
            "url": "https://example.com",
            "max_pages": 6
        }))
        .unwrap();
        assert_eq!(snake.org_id, "org_2");
        assert_eq!(snake.max_pages, Some(6));
    }

    #[test]
    fn envelopes_use_consistent_success_and_error_shapes() {
        assert_eq!(ok(json!({ "value": 1 })), json!({ "data": { "value": 1 } }));
        assert_eq!(
            error("invalid_input", "Nope"),
            json!({ "error": { "code": "invalid_input", "message": "Nope" } })
        );
    }

    #[tokio::test]
    async fn upstream_transport_errors_map_to_bad_gateway_envelopes() {
        let state = test_state(false);
        let (status, Json(body)) = proxy_json(
            &state,
            Method::GET,
            "http://127.0.0.1:1/unavailable",
            None,
            None,
            None,
            None,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(
            body.pointer("/error/code").and_then(Value::as_str),
            Some("upstream_unavailable")
        );
    }

    #[test]
    fn plan_ids_are_normalized_to_supported_values() {
        assert_eq!(plan_id("standard"), "standard");
        assert_eq!(plan_id("unknown"), "trial");
    }

    #[test]
    fn full_router_builds_without_overlapping_routes() {
        // axum panics at construction time when two merged routers register the
        // same method+path. Building the real router here guards against that
        // boot-time panic (e.g. a duplicated `/api/v1/me` across domains).
        let _ = crate::build_router(test_state(false));
    }
}
