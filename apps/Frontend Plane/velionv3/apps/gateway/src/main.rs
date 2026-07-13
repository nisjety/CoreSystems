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
use rate_limit::rate_limit_middleware;

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
        .merge(domains::cost::router(state.clone()))
        .merge(domains::eval::router(state.clone()))
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
        .merge(domains::shipping::router(state.clone()))
        .merge(domains::social::router(state.clone()))
        .merge(domains::studio::router(state.clone()))
        .merge(domains::tickets::router(state.clone()))
        // Inbound rate limiting. Runs early — after identity-header stripping
        // (so the validated `AuthenticatedUser` extension, when a downstream
        // `require_session` route_layer has inserted it, is the key) and before
        // CORS/tracing, so throttled requests do the least work. The
        // `Extension(RateLimiter)` layer must sit OUTER of the middleware so the
        // limiter is in extensions by the time `rate_limit_middleware` reads it.
        // Distributed (Dragonfly-backed) when the cache is connected, with an
        // in-process fallback — see `rate_limit.rs`. Reuses the cache's shared
        // Dragonfly connection via `AppState.rate_limiter`.
        .layer(axum::middleware::from_fn(rate_limit_middleware))
        .layer(axum::Extension(state.rate_limiter.clone()))
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
            session_core_service_token: "0123456789abcdef0123456789abcdef".into(),
            user_core_service_token: "abcdef0123456789abcdef0123456789".into(),
            billing_core_url: "http://127.0.0.1:1".into(),
            cost_core_url: "http://127.0.0.1:1".into(),
            org_core_url: "http://127.0.0.1:1".into(),
            integration_core_url: "http://127.0.0.1:1".into(),
            audit_core_url: "http://127.0.0.1:1".into(),
            insight_core_url: "http://127.0.0.1:1".into(),
            leads_core_url: "http://127.0.0.1:1".into(),
            shipping_core_url: "http://127.0.0.1:1".into(),
            user_core_url: "http://127.0.0.1:1".into(),
            graph_index_url: "http://127.0.0.1:1".into(),
            quarry_edge_url: "http://127.0.0.1:1".into(),
            model_recommend_url: "http://127.0.0.1:1".into(),
            model_gateway_url: "http://127.0.0.1:1".into(),
            model_gateway_dev_bearer: String::new(),
            inference_core_url: "http://127.0.0.1:1".into(),
            documents_api_url: "http://127.0.0.1:1".into(),
            retrieval_engine_url: "http://127.0.0.1:1".into(),
            wiki_store_url: "http://127.0.0.1:1".into(),
            embedding_engine_url: "http://127.0.0.1:1".into(),
            quickwit_adapter_url: "http://127.0.0.1:1".into(),
            finspo_core_url: "http://127.0.0.1:1".into(),
            imports_api_url: "http://127.0.0.1:1".into(),
            notification_core_url: "http://127.0.0.1:1".into(),
            notification_core_service_token: "notification-test-secret-at-least-32-bytes".into(),
            conversation_core_service_token: "conversation-test-secret-at-least-32-bytes".into(),
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
            rate_limiter: crate::rate_limit::RateLimiter::from_cache(
                &crate::cache::ResultCache::disabled(),
            ),
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
        // Tenant identity now comes from the verified session/JWT, not the body,
        // so a legacy `orgId`/`org_id` field is accepted-and-ignored. Only `url`
        // and `maxPages`/`max_pages` are read.
        let camel = serde_json::from_value::<WebsiteIngestRequest>(json!({
            "orgId": "org_1",
            "url": "https://example.com",
            "maxPages": 4
        }))
        .unwrap();
        assert_eq!(camel.url, "https://example.com");
        assert_eq!(camel.max_pages, Some(4));

        let snake = serde_json::from_value::<WebsiteIngestRequest>(json!({
            "org_id": "org_2",
            "url": "https://example.com",
            "max_pages": 6
        }))
        .unwrap();
        assert_eq!(snake.url, "https://example.com");
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

    /// A validated session's `activeOrganizationId` is only a requested scope,
    /// not proof that the user is still a member. Every sensitive gateway domain
    /// must consult user-core's canonical membership decision before forwarding.
    #[tokio::test]
    async fn removed_active_org_member_is_blocked_before_sensitive_upstream() {
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
                "user": {
                    "id": "removed-user",
                    "email": "removed@example.com",
                    "emailVerified": true
                },
                "session": { "activeOrganizationId": "removed-org" }
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "removed-user",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;

        let sensitive_upstream = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/internal/v1/router-policy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&sensitive_upstream)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.inference_core_url = sensitive_upstream.uri();
        let app = crate::build_router(state);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/router-policy")
                    .header("cookie", "better-auth.session_token=removed")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            body.pointer("/error/code").and_then(Value::as_str),
            Some("organization_membership_required")
        );
        assert!(
            sensitive_upstream
                .received_requests()
                .await
                .unwrap()
                .is_empty(),
            "removed members must be rejected before the sensitive upstream"
        );

        let bootstrap = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/session/current")
                    .header("cookie", "better-auth.session_token=removed")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bootstrap.status(), StatusCode::OK);
        let bootstrap_body: Value =
            serde_json::from_slice(&bootstrap.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert!(
            bootstrap_body
                .pointer("/data/org")
                .is_some_and(Value::is_null),
            "bootstrap must remain available without exposing the removed active org"
        );
    }

    #[tokio::test]
    async fn membership_authority_redirect_is_not_followed_and_returns_sanitized_503() {
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
                "user": {
                    "id": "redirect-user",
                    "email": "redirect@example.com",
                    "emailVerified": true
                },
                "session": { "activeOrganizationId": "org-active" }
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(307).insert_header("location", "/redirect-target"))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/redirect-target"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "redirect-user",
                "orgId": "org-active",
                "role": "owner",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        let app = crate::build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/router-policy")
                    .header("cookie", "better-auth.session_token=redirect")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            body.pointer("/error/code").and_then(Value::as_str),
            Some("organization_membership_authority_unavailable")
        );
        let redirected = user_core
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == "/redirect-target")
            .count();
        assert_eq!(
            redirected, 0,
            "delegation credentials must not follow redirects"
        );
    }

    #[tokio::test]
    async fn mismatched_or_malformed_membership_success_returns_sanitized_503() {
        use axum::body::Body;
        use axum::http::Request;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        for (case, authority_body) in [
            (
                "mismatched user",
                json!({
                    "userId": "different-user",
                    "orgId": "org-active",
                    "role": "member",
                    "onboardingStatus": "COMPLETED"
                }),
            ),
            (
                "unknown field",
                json!({
                    "userId": "authority-user",
                    "orgId": "org-active",
                    "role": "member",
                    "onboardingStatus": "COMPLETED",
                    "unexpected": true
                }),
            ),
        ] {
            let auth = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/auth/get-session"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "user": { "id": "authority-user", "email": "authority@example.com", "emailVerified": true },
                    "session": { "activeOrganizationId": "org-active" }
                })))
                .mount(&auth)
                .await;

            let user_core = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/v1/me/session-context"))
                .respond_with(ResponseTemplate::new(200).set_body_json(authority_body))
                .mount(&user_core)
                .await;

            let mut state = test_state(false);
            state.auth_core_url = auth.uri();
            state.user_core_url = user_core.uri();
            let app = crate::build_router(state);

            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/v1/router-policy")
                        .header("cookie", "better-auth.session_token=authority")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{case}");
            let body: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(
                body.pointer("/error/code").and_then(Value::as_str),
                Some("organization_membership_authority_unavailable"),
                "{case}"
            );
        }
    }

    #[tokio::test]
    async fn active_member_cannot_read_a_different_organization_by_path() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": { "id": "org-a-user", "email": "a@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": "org-a" }
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "org-a-user",
                "orgId": "org-a",
                "role": "member",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;

        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "org-b" })))
            .mount(&org_core)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org_core.uri();
        let app = crate::build_router(state);

        for path in ["/api/v1/orgs/org-b", "/api/v1/orgs/org-b/entitlements"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(path)
                        .header("cookie", "better-auth.session_token=org-a")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        }
        assert!(
            org_core.received_requests().await.unwrap().is_empty(),
            "a mismatched path organization must be rejected before org-core"
        );
    }

    #[tokio::test]
    async fn onboarding_tenant_routes_reject_forged_org_before_upstream() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": { "id": "onboarding-user", "email": "onboarding@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": "org-a" }
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "onboarding-user",
                "orgId": "org-a",
                "role": "owner",
                "onboardingStatus": "PROFILE_READY"
            })))
            .mount(&user_core)
            .await;

        let integration = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "connections": [{ "id": "conn-b", "status": "active" }] }
            })))
            .mount(&integration)
            .await;
        let graph = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "nodes": [], "edges": [], "node_count": 0, "edge_count": 0
            })))
            .mount(&graph)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.integration_core_url = integration.uri();
        state.graph_index_url = graph.uri();
        let app = crate::build_router(state);

        let requests = [
            ("GET", "/api/v1/onboarding/graph-preview?org_id=org-b", None),
            (
                "POST",
                "/api/v1/onboarding/actions/start-connect-session",
                Some(json!({ "orgId": "org-b", "provider": "microsoft", "selectedSources": [] })),
            ),
            (
                "POST",
                "/api/v1/onboarding/actions/discover-source",
                Some(
                    json!({ "orgId": "org-b", "provider": "microsoft", "connectorId": "c", "sources": [] }),
                ),
            ),
            (
                "POST",
                "/api/v1/onboarding/actions/cleanup-source",
                Some(json!({ "orgId": "org-b", "sourceId": "source-b" })),
            ),
            (
                "POST",
                "/api/v1/onboarding/actions/warm-sharepoint-discovery",
                Some(json!({ "orgId": "org-b" })),
            ),
            (
                "POST",
                "/api/v1/onboarding/actions/start-integration-sync",
                Some(
                    json!({ "orgId": "org-b", "provider": "microsoft", "connectorId": "c", "sources": [] }),
                ),
            ),
        ];
        for (method, uri, body) in requests {
            let mut builder = Request::builder()
                .method(method)
                .uri(uri)
                .header("cookie", "better-auth.session_token=onboarding");
            let request_body = match body {
                Some(body) => {
                    builder = builder.header("content-type", "application/json");
                    Body::from(serde_json::to_vec(&body).unwrap())
                }
                None => Body::empty(),
            };
            let response = app
                .clone()
                .oneshot(builder.body(request_body).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {uri}");
        }
        assert!(
            integration.received_requests().await.unwrap().is_empty(),
            "forged onboarding connector scope must be rejected before integration-core"
        );
        assert!(
            graph.received_requests().await.unwrap().is_empty(),
            "forged onboarding graph scope must be rejected before graph-index"
        );
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

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": { "id": "user-attacker", "email": "attacker@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": "org-attacker" }
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-attacker",
                "orgId": "org-attacker",
                "role": "member",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;

        // Stand-in inference-core: records every inbound request, always 200.
        let upstream = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/internal/v1/router-policy"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&upstream)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.inference_core_url = upstream.uri();
        let app = crate::build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/router-policy")
                    .header("cookie", "better-auth.session_token=attacker")
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
            assert_eq!(forwarded, Some("org-attacker"));
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

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "real-user-ima",
                "orgId": "triodelab-org",
                "role": "owner",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
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
        state.user_core_url = user_core.uri();
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
        assert_eq!(
            req.headers.get("x-user-role").and_then(|v| v.to_str().ok()),
            Some("owner"),
            "tenant downstreams must receive the canonical membership role"
        );
    }

    #[tokio::test]
    async fn inbox_reuses_the_single_live_membership_decision() {
        use axum::body::Body;
        use axum::http::Request;
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        use wiremock::matchers::{header, method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": { "id": "inbox-user", "email": "inbox@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": "inbox-org" }
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "inbox-user",
                "orgId": "inbox-org",
                "role": "member",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;

        let conversation = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/conversations"))
            .and(header("x-org-id", "inbox-org"))
            .and(header("x-user-id", "inbox-user"))
            .and(header("x-user-role", "member"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [],
                "meta": { "total": 0 }
            })))
            .mount(&conversation)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.conversation_core_url = conversation.uri();
        let app = crate::build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/inbox/conversations")
                    .header("cookie", "better-auth.session_token=inbox")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let _ = response.into_body().collect().await.unwrap();

        let authority_calls = user_core
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == "/api/v1/me/session-context")
            .count();
        assert_eq!(
            authority_calls, 1,
            "Inbox must reuse middleware's live membership instead of looking it up twice"
        );
        assert_eq!(conversation.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn onboarding_retry_fails_closed_when_org_core_reconciliation_fails() {
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
                "user": {
                    "id": "owner-1",
                    "email": "owner@acme.example",
                    "emailVerified": true
                },
                "session": { "activeOrganizationId": null }
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/organization/list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
                "id": "org-acme",
                "name": "Acme",
                "slug": "acme",
                "metadata": { "plan": "trial" }
            }])))
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/organization/get-full-organization"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-acme",
                "members": [{ "userId": "owner-1", "role": "owner" }]
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/auth/organization/set-active"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&auth)
            .await;

        let org_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/orgs"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "error": "failed to add organization member"
            })))
            .mount(&org_core)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.org_core_url = org_core.uri();
        let app = crate::build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/onboarding/actions/create-organization")
                    .header("content-type", "application/json")
                    .header("cookie", "idknuten.sid=session")
                    .body(Body::from(
                        json!({ "name": "Acme", "plan": "trial" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            body.pointer("/error/code").and_then(Value::as_str),
            Some("organization_provisioning_failed")
        );
    }

    #[tokio::test]
    async fn organization_list_failure_never_creates_a_duplicate() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {
                    "id": "owner-1",
                    "email": "owner@acme.example",
                    "emailVerified": true
                },
                "session": { "activeOrganizationId": null }
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/organization/list"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&auth)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/auth/organization/create"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "id": "must-not-be-created"
            })))
            .mount(&auth)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        let app = crate::build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/onboarding/actions/create-organization")
                    .header("content-type", "application/json")
                    .header("cookie", "idknuten.sid=session")
                    .body(Body::from(json!({ "name": "Acme" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let received = auth.received_requests().await.unwrap();
        assert!(received.iter().all(|request| {
            !(request.method.as_str() == "POST"
                && request.url.path() == "/api/auth/organization/create")
        }));
    }

    #[tokio::test]
    async fn failed_onboarding_completion_preserves_recovery_state() {
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
                "user": {
                    "id": "owner-1",
                    "email": "owner@acme.example",
                    "emailVerified": true
                },
                "session": { "activeOrganizationId": "org-acme" }
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/organization/get-full-organization"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-acme",
                "members": [{ "userId": "owner-1", "role": "owner" }]
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "owner-1",
                "orgId": "org-acme",
                "role": "owner",
                "onboardingStatus": "PROFILE_READY"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/users/onboarding/complete"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "error": "write failed"
            })))
            .mount(&user_core)
            .await;

        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/orgs/org-acme"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-acme",
                "status": "active"
            })))
            .mount(&org_core)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org_core.uri();
        let app = crate::build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/onboarding/complete")
                    .header("content-type", "application/json")
                    .header("cookie", "idknuten.sid=session")
                    .body(Body::from(
                        json!({ "orgId": "org-acme", "plan": "trial" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            body.pointer("/error/code").and_then(Value::as_str),
            Some("onboarding_completion_failed")
        );

        let received = user_core.received_requests().await.unwrap();
        assert!(
            received
                .iter()
                .all(|request| request.method.as_str() != "PUT"),
            "a failed completion must not clear the saved onboarding state"
        );
    }

    #[tokio::test]
    async fn canonical_completion_is_not_reported_failed_for_projection_lag() {
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
                "user": { "id": "owner-1", "email": "owner@acme.example", "emailVerified": true },
                "session": { "activeOrganizationId": "org-acme" }
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/organization/get-full-organization"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-acme",
                "members": [{ "userId": "owner-1", "role": "owner" }]
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "owner-1",
                "orgId": "org-acme",
                "role": "owner",
                "onboardingStatus": "PROFILE_READY"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/users/onboarding/complete"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("PUT"))
            .and(wm_path("/api/v1/users/me/onboarding-state"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
            .mount(&user_core)
            .await;

        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/orgs/org-acme"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "org-acme" })))
            .mount(&org_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/internal/orgs/org-acme/onboarding/state"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&org_core)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org_core.uri();
        let app = crate::build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/onboarding/complete")
                    .header("content-type", "application/json")
                    .header("cookie", "idknuten.sid=session")
                    .body(Body::from(json!({ "orgId": "org-acme" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            body.pointer("/data/orgProjectionSynced")
                .and_then(Value::as_bool),
            Some(false)
        );
    }

    #[tokio::test]
    async fn checkout_is_blocked_when_active_org_membership_is_stale() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        use wiremock::matchers::{method as wm_method, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wiremock::matchers::path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {
                    "id": "owner-1",
                    "email": "owner@acme.example",
                    "emailVerified": true
                },
                "session": { "activeOrganizationId": "org-acme" }
            })))
            .mount(&auth)
            .await;

        let billing = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(path_regex("/api/v1/billing/orgs/.*/checkout-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "checkout_url": "https://billing.example/checkout"
            })))
            .mount(&billing)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wiremock::matchers::path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "owner-1",
                "onboardingStatus": "PROFILE_READY"
            })))
            .mount(&user_core)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.billing_core_url = billing.uri();
        state.user_core_url = user_core.uri();
        let app = crate::build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/onboarding/actions/start-checkout")
                    .header("content-type", "application/json")
                    .header("cookie", "idknuten.sid=session")
                    .body(Body::from(
                        json!({
                            "orgId": "org-acme",
                            "plan": "enterprise",
                            "successUrl": "http://localhost/success",
                            "cancelUrl": "http://localhost/cancel"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(billing.received_requests().await.unwrap().is_empty());
    }
}
