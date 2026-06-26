use std::{env, net::SocketAddr};

use anyhow::Result;
use axum::{
    http::{header, HeaderValue},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use tower_http::{set_header::SetResponseHeaderLayer, trace::TraceLayer};
use tracing::info;

mod audience_tokens;
mod auth;
mod cache;
mod config;
mod contracts;
mod domains;
mod envelope;
mod middleware;
mod observability;
mod onboarding;
mod public_url;
mod rate_limit;
mod upstream;
mod utils;

use config::{build_cors_layer, build_state};
use middleware::strip_inbound_identity_headers;
use rate_limit::{rate_limit_middleware, RateLimiter};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    // Prometheus recorder for the /metrics endpoint (Phase 6 B13). Installed
    // once, globally, before the router so the request-tracking middleware and
    // the auth path can record into it.
    let prometheus_handle = observability::install_recorder()?;

    let state = build_state().await?;
    let app = build_router(state)
        .route(
            "/metrics",
            get(move || {
                let handle = prometheus_handle.clone();
                async move { handle.render() }
            }),
        )
        .layer(axum::middleware::from_fn(observability::track_metrics));

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
        .merge(domains::agents_runs::router(state.clone()))
        .merge(domains::ag_ui::router(state.clone()))
        .merge(domains::ai::router(state.clone()))
        .merge(domains::audit::router(state.clone()))
        .merge(domains::auth::router(state.clone()))
        .merge(domains::browser::router(state.clone()))
        .merge(domains::billing::router(state.clone()))
        .merge(domains::briefs::router(state.clone()))
        .merge(domains::chat::router(state.clone()))
        .merge(domains::finetune::router(state.clone()))
        .merge(domains::inbox::router(state.clone()))
        .merge(domains::information::router(state.clone()))
        .merge(domains::ingestions::router(state.clone()))
        .merge(domains::insights::router(state.clone()))
        .merge(domains::integrations::router(state.clone()))
        .merge(domains::knowledge::router(state.clone()))
        .merge(domains::leads::router(state.clone()))
        .merge(domains::mcp::router(state.clone()))
        .merge(domains::monitoring::router(state.clone()))
        .merge(domains::navbar::router(state.clone()))
        .merge(domains::notifications::router(state.clone()))
        .merge(domains::onboarding::router(state.clone()))
        .merge(domains::orchestration::router(state.clone()))
        .merge(domains::orgs::router(state.clone()))
        .merge(domains::ownership::router(state.clone()))
        .merge(domains::privacy::router(state.clone()))
        .merge(domains::router_policy::router(state.clone()))
        .merge(domains::search::router(state.clone()))
        .merge(domains::settings::router(state.clone()))
        .merge(domains::shares::router(state.clone()))
        .merge(domains::social::router(state.clone()))
        .merge(domains::studio::router(state.clone()))
        .merge(domains::tickets::router(state.clone()))
        // Inbound rate limiting. Runs early — after identity-header stripping
        // (so the validated `AuthenticatedUser` extension, when a downstream
        // `require_session` route_layer has inserted it, is the key) and before
        // CORS/tracing, so throttled requests do the least work. The
        // `Extension(RateLimiter)` layer must sit OUTER of the middleware so the
        // limiter is in extensions by the time `rate_limit_middleware` reads it.
        // Per-instance / not distributed — see `rate_limit.rs`.
        .layer(axum::middleware::from_fn(rate_limit_middleware))
        .layer(axum::Extension(RateLimiter::from_env()))
        .layer(axum::middleware::from_fn(strip_inbound_identity_headers))
        // Minimal security response headers on every API response. `if_not_present`
        // never clobbers a value an upstream already set, and these are passive
        // response headers — they do not touch CORS negotiation. The browser-facing
        // CSP / X-Frame-Options live at nginx (it serves the SPA HTML); these two
        // harden the JSON/SSE API surface the gateway owns directly.
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
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
            enforcement_mode: "off".to_string(),
            auth_core_url: "http://127.0.0.1:1".into(),
            session_core_url: "http://127.0.0.1:1".into(),
            billing_core_url: "http://127.0.0.1:1".into(),
            org_core_url: "http://127.0.0.1:1".into(),
            integration_core_url: "http://127.0.0.1:1".into(),
            audit_core_url: "http://127.0.0.1:1".into(),
            insight_core_url: "http://127.0.0.1:1".into(),
            leads_core_url: "http://127.0.0.1:1".into(),
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
            audience_token_cache: crate::audience_tokens::new_audience_token_cache(),
            browser_run_store: crate::domains::browser::new_browser_run_store(),
            cache: crate::cache::ResultCache::disabled(),
            chat_history_store: crate::domains::chat::history::ChatHistoryStore::new(),
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
    fn trusted_actor_headers_win_over_dev_body_actor() {
        let mut headers = HeaderMap::new();
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
            true,
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

    /// Tier-2 (cross-tenant IDOR): drive the *real* router end-to-end. An
    /// authenticated user forges `x-velion-org-id` for a victim org; the gateway
    /// must never forward that id to the upstream core as `x-org-id`. Fails on
    /// pre-fix code (handler read the client header), passes after the org is
    /// derived from the validated session + the header is stripped at ingress.
    #[tokio::test]
    async fn forged_org_header_never_reaches_upstream() {
        use axum::body::Body;
        use axum::http::Request;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        // Stand-in inference-core: records every inbound request, always 200.
        let upstream = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/internal/v1/router-policy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&upstream)
            .await;

        // Dev-bypass auth on; point router-policy's upstream at the mock. The
        // dev user has no active org and user-core is unreachable in tests, so
        // the authoritative org resolves to empty.
        let mut state = test_state(true);
        state.inference_core_url = upstream.uri();
        let app = crate::build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/router-policy")
                    .header("authorization", "Bearer dev-bypass")
                    .header("x-velion-org-id", "org-victim")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // The request must have been proxied (not rejected) so the assertion
        // below is meaningful.
        assert_eq!(response.status(), StatusCode::OK);
        let _ = response.into_body().collect().await.unwrap();

        // Negative control across EVERY recorded upstream request.
        let received = upstream.received_requests().await.unwrap();
        assert!(
            !received.is_empty(),
            "router-policy must proxy through to the upstream core"
        );
        for req in &received {
            let forwarded = req.headers.get("x-org-id").and_then(|v| v.to_str().ok());
            assert_ne!(
                forwarded,
                Some("org-victim"),
                "a client-forged x-velion-org-id must never be forwarded as x-org-id"
            );
        }
    }

    /// Regression (cross-tenant chat leak): a real validated Better Auth session
    /// MUST win over a `Bearer dev-bypass` header. If dev-bypass took precedence
    /// it would collapse every caller onto the shared dev identity, so distinct
    /// tenants would read each other's chat/data. Here a real Triodelab session
    /// is present alongside a dev-bypass header — the real identity must be used.
    #[tokio::test]
    async fn real_session_is_authoritative_over_dev_bypass() {
        use axum::body::Body;
        use axum::http::Request;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": { "id": "real-user-ima", "email": "ima@triodelab.no", "emailVerified": true },
                "session": { "activeOrganizationId": "triodelab-org" }
            })))
            .mount(&auth)
            .await;

        let upstream = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/internal/v1/router-policy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&upstream)
            .await;

        // Dev-bypass ENABLED, but a real session cookie is present.
        let mut state = test_state(true);
        state.auth_core_url = auth.uri();
        state.inference_core_url = upstream.uri();
        let app = crate::build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/router-policy")
                    .header("authorization", "Bearer dev-bypass")
                    .header("cookie", "better-auth.session_token=real")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let _ = response.into_body().collect().await.unwrap();

        let received = upstream.received_requests().await.unwrap();
        assert!(!received.is_empty(), "router-policy must proxy upstream");
        let req = &received[0];
        assert_eq!(
            req.headers.get("x-user-id").and_then(|v| v.to_str().ok()),
            Some("real-user-ima"),
            "the real session user must be used, not the shared dev-bypass identity"
        );
        assert_eq!(
            req.headers.get("x-org-id").and_then(|v| v.to_str().ok()),
            Some("triodelab-org"),
            "the real session's org must be forwarded, not the dev user's"
        );
    }
}
