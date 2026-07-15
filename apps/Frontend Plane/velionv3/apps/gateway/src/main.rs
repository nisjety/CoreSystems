use std::{env, net::SocketAddr};

use anyhow::Result;
use axum::{
    body::Body,
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
        .layer(TraceLayer::new_for_http().make_span_with(make_http_trace_span))
        .with_state(state)
}

fn request_target_for_log(uri: &axum::http::Uri) -> String {
    const SENSITIVE_CALLBACK_PREFIXES: [&str; 3] = [
        "/api/auth/callback/",
        "/api/auth/sso/callback/",
        "/api/auth/sso/saml2/callback/",
    ];

    if SENSITIVE_CALLBACK_PREFIXES
        .iter()
        .any(|prefix| uri.path().starts_with(prefix))
        || uri.path() == "/reset-password"
    {
        return uri.path().to_owned();
    }

    if uri.path().starts_with("/accept-invitation/") {
        return "/accept-invitation/:invitationId".to_owned();
    }

    if uri.path().starts_with("/api/v1/orgs/invitations/") {
        return "/api/v1/orgs/invitations/:invitationId/accept".to_owned();
    }

    uri.path().to_owned()
}

fn make_http_trace_span(request: &axum::http::Request<Body>) -> tracing::Span {
    let request_target = request_target_for_log(request.uri());
    tracing::info_span!(
        "http_request",
        method = %request.method(),
        uri = %request_target,
        version = ?request.version(),
    )
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

    #[test]
    fn callback_trace_target_redacts_oauth_oidc_and_saml_secrets() {
        use axum::http::Uri;

        for raw_uri in [
            "/api/auth/callback/google?code=oauth-secret&state=oauth-state",
            "/api/auth/sso/callback/acme?code=oidc-secret&state=oidc-state",
            "/api/auth/sso/saml2/callback/acme?SAMLResponse=saml-secret&RelayState=relay-secret",
            "/reset-password?token=password-reset-secret",
        ] {
            let uri: Uri = raw_uri.parse().expect("valid test URI");
            let logged = super::request_target_for_log(&uri);
            assert_eq!(logged, uri.path());
            assert!(!logged.contains("secret"));
            assert!(!logged.contains("state="));
        }

        let invitation: Uri = "/accept-invitation/invitation-secret"
            .parse()
            .expect("valid invitation URI");
        assert_eq!(
            super::request_target_for_log(&invitation),
            "/accept-invitation/:invitationId"
        );

        let invitation_api: Uri = "/api/v1/orgs/invitations/invitation-secret/accept"
            .parse()
            .expect("valid invitation API URI");
        assert_eq!(
            super::request_target_for_log(&invitation_api),
            "/api/v1/orgs/invitations/:invitationId/accept"
        );

        let ordinary: Uri = "/api/v1/audit?limit=25".parse().expect("valid test URI");
        assert_eq!(super::request_target_for_log(&ordinary), "/api/v1/audit");
    }

    #[test]
    fn nginx_access_log_uses_a_query_redacting_callback_target() {
        let nginx = include_str!("../../../nginx.conf");
        assert!(nginx.contains("map $uri $velion_log_uri"));
        assert!(nginx.contains("~^/api/auth/(callback|sso/callback|sso/saml2/callback)/ $uri;"));
        assert!(nginx.contains("/reset-password $uri;"));
        assert!(nginx.contains("~^/accept-invitation/ /accept-invitation/:invitationId;"));
        assert!(nginx.contains(
            "~^/api/v1/orgs/invitations/ /api/v1/orgs/invitations/:invitationId/accept;"
        ));
        assert!(nginx.contains("default $uri;"));
        assert!(!nginx.contains("default $request_uri;"));
        assert!(nginx.contains("log_format velion_safe"));
        assert!(nginx.contains("access_log /var/log/nginx/access.log velion_safe;"));

        let safe_format = nginx
            .split("log_format velion_safe")
            .nth(1)
            .and_then(|suffix| suffix.split(';').next())
            .expect("velion_safe log format");
        assert!(safe_format.contains("$velion_log_uri"));
        assert!(!safe_format.contains("$request "));
        assert!(!safe_format.contains("$request_uri"));
    }

    #[test]
    fn compose_defaults_and_production_override_fail_closed() {
        let base = include_str!("../../../docker-compose.yml");
        let production = include_str!("../../../docker-compose.production.yml");

        assert!(base.contains("CONTROL_PLANE_ENFORCEMENT: ${CONTROL_PLANE_ENFORCEMENT:-strict}"));
        assert!(production.contains("CONTROL_PLANE_ENFORCEMENT: strict"));
        assert!(production.contains("ALLOW_DEV_AUTH_BYPASS: \"0\""));
        assert!(production.contains("ALLOW_INSECURE_DEV_DEFAULTS: \"0\""));
    }

    pub(crate) fn test_state(allow_dev_actor_headers: bool) -> AppState {
        AppState {
            client: reqwest::Client::new(),
            streaming_client: reqwest::Client::new(),
            internal_api_key: "test-key".into(),
            enforcement_mode: "off".to_string(),
            auth_core_url: "http://127.0.0.1:1".into(),
            velion_public_origin: "http://localhost:5173".into(),
            session_core_url: "http://127.0.0.1:1".into(),
            session_core_service_token: "0123456789abcdef0123456789abcdef".into(),
            user_core_service_token: "abcdef0123456789abcdef0123456789".into(),
            billing_core_url: "http://127.0.0.1:1".into(),
            billing_core_service_token: "billing-test-secret-at-least-32-bytes".into(),
            cost_core_url: "http://127.0.0.1:1".into(),
            org_core_url: "http://127.0.0.1:1".into(),
            org_core_service_token: "org-test-secret-at-least-32-bytes".into(),
            integration_core_url: "http://127.0.0.1:1".into(),
            audit_core_url: "http://127.0.0.1:1".into(),
            audit_core_service_token: "audit-test-secret-at-least-32-bytes".into(),
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
        assert_eq!(
            body.pointer("/error/message").and_then(Value::as_str),
            Some("The upstream service is unavailable.")
        );
        assert!(!body.to_string().contains("127.0.0.1"));
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

    #[tokio::test]
    async fn membership_retry_requests_return_successful_noops() {
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
                    "id": "owner-user",
                    "email": "owner@example.com",
                    "emailVerified": true
                },
                "session": { "activeOrganizationId": "org-active" }
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/auth/organization/update-member-role"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "member-record-current-role",
                "userId": "same-role-user",
                "organizationId": "org-active",
                "role": "admin"
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/auth/organization/invite-member"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "code": "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION",
                "message": "User is already invited to this organization"
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/organization/list-members"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "members": [{
                    "id": "member-record-current-role",
                    "userId": "same-role-user",
                    "role": "admin"
                }]
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "owner-user",
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

        let invite = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/orgs/org-active/members/invite")
                    .header("cookie", "better-auth.session_token=owner")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": " Invitee@Example.com ",
                            "role": "member"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invite.status(), StatusCode::OK);
        let invite_body: Value =
            serde_json::from_slice(&invite.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            invite_body,
            json!({
                "data": {
                    "email": "invitee@example.com",
                    "role": "member",
                    "invitation_created": false
                }
            })
        );

        let remove = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/orgs/org-active/members/already-removed")
                    .header("cookie", "better-auth.session_token=owner")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(remove.status(), StatusCode::OK);
        let remove_body: Value =
            serde_json::from_slice(&remove.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            remove_body,
            json!({
                "data": {
                    "user_id": "already-removed",
                    "removed": false
                }
            })
        );
        let remove_mutations = auth
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == "/api/auth/organization/remove-member")
            .count();
        assert_eq!(remove_mutations, 0, "a no-op must not reach Auth mutation");

        let same_role = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/v1/orgs/org-active/members/same-role-user/role")
                    .header("cookie", "better-auth.session_token=owner")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "role": "admin" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(same_role.status(), StatusCode::OK);
        let same_role_body: Value =
            serde_json::from_slice(&same_role.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(
            same_role_body,
            json!({
                "id": "member-record-current-role",
                "userId": "same-role-user",
                "organizationId": "org-active",
                "role": "admin"
            })
        );
        let role_mutations = auth
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == "/api/auth/organization/update-member-role")
            .count();
        assert_eq!(
            role_mutations, 1,
            "Auth must serialize even an apparent same-role request"
        );
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

    #[tokio::test]
    async fn oauth_callback_preserves_redirect_and_cookies_without_forwarding_authority_headers() {
        use axum::body::Body;
        use axum::http::{header, Request};
        use tower::ServiceExt;
        use wiremock::matchers::{method as wm_method, path as wm_path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/callback/google"))
            .and(query_param("code", "oauth-code"))
            .and(query_param("state", "opaque-state"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", "http://localhost:5173/onboarding")
                    .append_header(
                        "set-cookie",
                        "idknuten.session_token=session; Path=/; HttpOnly; SameSite=Lax",
                    )
                    .append_header(
                        "set-cookie",
                        "idknuten.session_data=context; Path=/; HttpOnly; SameSite=Lax",
                    ),
            )
            .expect(1)
            .mount(&auth)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        let app = crate::build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/auth/callback/google?code=oauth-code&state=opaque-state")
                    .header("cookie", "idknuten.state=state-cookie")
                    .header("x-user-id", "forged-user")
                    .header("x-org-id", "forged-org")
                    .header("x-user-role", "admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("http://localhost:5173/onboarding")
        );
        assert_eq!(
            response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .count(),
            2
        );

        let received = auth.received_requests().await.unwrap();
        let callback = received.first().expect("callback request");
        assert_eq!(
            callback
                .headers
                .get("cookie")
                .and_then(|value| value.to_str().ok()),
            Some("idknuten.state=state-cookie")
        );
        for forbidden in ["x-user-id", "x-org-id", "x-user-role", "x-internal-api-key"] {
            assert!(
                callback.headers.get(forbidden).is_none(),
                "callback must not forward {forbidden}"
            );
        }
    }

    #[tokio::test]
    async fn sso_callbacks_preserve_oidc_query_and_saml_form_body() {
        use axum::body::Body;
        use axum::http::{header, Request};
        use tower::ServiceExt;
        use wiremock::matchers::{
            body_string, header as wm_header, method as wm_method, path as wm_path, query_param,
        };
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/sso/callback/acme"))
            .and(query_param("code", "oidc-code"))
            .and(query_param("state", "oidc-state"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", "http://localhost:5173/dashboard")
                    .append_header(
                        "set-cookie",
                        "idknuten.session_token=oidc; Path=/; HttpOnly",
                    ),
            )
            .expect(1)
            .mount(&auth)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/auth/sso/saml2/callback/acme"))
            .and(wm_header(
                "content-type",
                "application/x-www-form-urlencoded",
            ))
            .and(body_string("SAMLResponse=signed-response&RelayState=relay"))
            .respond_with(
                ResponseTemplate::new(303)
                    .insert_header("location", "http://localhost:5173/dashboard")
                    .append_header(
                        "set-cookie",
                        "idknuten.session_token=saml; Path=/; HttpOnly",
                    ),
            )
            .expect(1)
            .mount(&auth)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        let app = crate::build_router(state);

        let oidc = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/auth/sso/callback/acme?code=oidc-code&state=oidc-state")
                    .header("cookie", "idknuten.state=oidc-state-cookie")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oidc.status(), StatusCode::FOUND);
        assert_eq!(
            oidc.headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("http://localhost:5173/dashboard")
        );

        let saml = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/sso/saml2/callback/acme")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header("cookie", "idknuten.state=saml-state-cookie")
                    .body(Body::from("SAMLResponse=signed-response&RelayState=relay"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(saml.status(), StatusCode::SEE_OTHER);
        assert_eq!(saml.headers().get_all(header::SET_COOKIE).iter().count(), 1);
    }

    #[tokio::test]
    async fn auth_callbacks_reject_invalid_provider_ids_and_oversized_saml_bodies() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        use wiremock::MockServer;

        let auth = MockServer::start().await;
        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        let app = crate::build_router(state);

        let invalid_provider = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/auth/callback/%25invalid?code=oauth-code&state=opaque-state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_provider.status(), StatusCode::BAD_REQUEST);

        let oversized_saml = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/sso/saml2/callback/acme")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(vec![b'a'; 64 * 1024 + 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized_saml.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(auth.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn organization_list_is_session_scoped_and_proxies_canonical_auth_authority() {
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
                "user": { "id": "user-1", "email": "user@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": null }
            })))
            .expect(1)
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/organization/list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                { "id": "org-1", "name": "Acme", "slug": "acme", "metadata": { "plan": "trial" } }
            ])))
            .expect(1)
            .mount(&auth)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        let app = crate::build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/orgs")
                    .header("cookie", "idknuten.sid=session")
                    .header("x-user-role", "admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body[0]["id"], "org-1");

        let requests = auth.received_requests().await.unwrap();
        let list = requests
            .iter()
            .find(|request| request.url.path() == "/api/auth/organization/list")
            .expect("organization list request");
        assert!(list.headers.get("x-user-role").is_none());
        assert!(list.headers.get("x-internal-api-key").is_none());
    }

    #[tokio::test]
    async fn organization_self_read_sends_org_bound_control_delegation() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        use wiremock::matchers::{method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": { "id": "user-1", "email": "user@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": "org-1" }
            })))
            .mount(&auth)
            .await;

        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "org-1", "name": "Acme"
            })))
            .mount(&org)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1",
                "orgId": "org-1",
                "role": "owner",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.org_core_url = org.uri();
        state.user_core_url = user_core.uri();
        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/orgs/org-1")
                    .header("cookie", "idknuten.sid=session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let requests = org.received_requests().await.unwrap();
        let request = requests.first().expect("Org Core request");
        assert_eq!(
            request.headers.get("x-service-id").unwrap(),
            "velion-gateway"
        );
        assert_eq!(request.headers.get("x-org-id").unwrap(), "org-1");
        assert_eq!(request.headers.get("x-delegation-version").unwrap(), "v3");
        assert!(request.headers.get("x-delegation-signature").is_some());
        assert!(request.headers.get("x-internal-api-key").is_none());
    }

    #[tokio::test]
    async fn knowledge_routes_forward_session_minted_data_plane_bearer() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        use wiremock::matchers::{header as wm_header, method as wm_method, path as wm_path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": { "id": "user-1", "email": "user@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": "org-1" }
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/data-plane/token"))
            .and(wm_header("cookie", "idknuten.sid=session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "signed-data-plane-token",
                "expiresInSeconds": 300
            })))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1",
                "orgId": "org-1",
                "role": "member",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;

        let documents = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/documents"))
            .and(wm_header("authorization", "Bearer signed-data-plane-token"))
            .and(wm_header("x-org-id", "org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "documents": [] })))
            .expect(2)
            .mount(&documents)
            .await;

        let retrieval = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/retrieve/chunks"))
            .and(wm_header("authorization", "Bearer signed-data-plane-token"))
            .and(wm_header("x-org-id", "org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "chunks": [] })))
            .expect(1)
            .mount(&retrieval)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/knowledge/search"))
            .and(wm_header("authorization", "Bearer signed-data-plane-token"))
            .and(wm_header("x-org-id", "org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "candidates": [{
                    "knowledge_id": "chunk-1",
                    "document_id": "doc-1",
                    "text": "Budget evidence"
                }],
                "sources": [{
                    "document_id": "doc-1",
                    "title": "Budget plan",
                    "source": "sharepoint"
                }]
            })))
            .expect(1)
            .mount(&retrieval)
            .await;

        let wiki = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/wiki/pages"))
            .and(wm_header("authorization", "Bearer signed-data-plane-token"))
            .and(wm_header("x-org-id", "org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "pages": [] })))
            .expect(1)
            .mount(&wiki)
            .await;

        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.documents_api_url = documents.uri();
        state.retrieval_engine_url = retrieval.uri();
        state.wiki_store_url = wiki.uri();
        let app = crate::build_router(state);

        for (method, uri, body) in [
            ("GET", "/api/v1/knowledge/documents", None),
            ("GET", "/api/v1/knowledge/source-list", None),
            (
                "POST",
                "/api/v1/knowledge/retrieve/chunks",
                Some(json!({ "document_id": "doc-1" })),
            ),
            ("GET", "/api/v1/knowledge/wiki/pages", None),
        ] {
            let mut builder = Request::builder()
                .method(method)
                .uri(uri)
                .header("cookie", "idknuten.sid=session");
            let request_body = match body {
                Some(value) => {
                    builder = builder.header("content-type", "application/json");
                    Body::from(serde_json::to_vec(&value).unwrap())
                }
                None => Body::empty(),
            };
            let response = app
                .clone()
                .oneshot(builder.body(request_body).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{method} {uri}");
        }

        let navbar_search = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/navbar/search?q=budget")
                    .header("cookie", "idknuten.sid=session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(navbar_search.status(), StatusCode::OK);
        let navbar_body: Value = serde_json::from_slice(
            &http_body_util::BodyExt::collect(navbar_search.into_body())
                .await
                .unwrap()
                .to_bytes(),
        )
        .unwrap();
        assert_eq!(navbar_body["data"]["results"][0]["id"], "chunk-1");
        assert_eq!(navbar_body["data"]["results"][0]["label"], "Budget plan");
        assert_eq!(
            navbar_body["data"]["results"][0]["excerpt"],
            "Budget evidence"
        );

        let retrieval_requests = retrieval.received_requests().await.unwrap();
        let search_request = retrieval_requests
            .iter()
            .find(|request| request.url.path() == "/v1/knowledge/search")
            .expect("navbar retrieval request");
        let search_body: Value = serde_json::from_slice(&search_request.body).unwrap();
        assert_eq!(search_body["org_id"], "org-1");
        assert_eq!(search_body["top_k"], 6);
        assert!(search_body["filters"].is_object());

        for upstream in [&documents, &retrieval, &wiki] {
            for request in upstream.received_requests().await.unwrap() {
                assert!(
                    request.headers.get("x-internal-api-key").is_none(),
                    "interactive Data Plane calls must not fall back to a shared internal key"
                );
            }
        }

        let token_requests = auth
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.url.path() == "/api/data-plane/token")
            .count();
        assert_eq!(token_requests, 5);
    }

    #[tokio::test]
    async fn knowledge_routes_fail_closed_when_data_plane_token_cannot_be_minted() {
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
                "user": { "id": "user-1", "email": "user@example.com", "emailVerified": true },
                "session": { "activeOrganizationId": "org-1" }
            })))
            .mount(&auth)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/data-plane/token"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&auth)
            .await;

        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1",
                "orgId": "org-1",
                "role": "member",
                "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;

        let documents = MockServer::start().await;
        let mut state = test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.documents_api_url = documents.uri();

        let response = crate::build_router(state)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/knowledge/documents")
                    .header("cookie", "idknuten.sid=session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["error"]["code"], "delegated_auth_unavailable");
        assert!(documents.received_requests().await.unwrap().is_empty());
    }
}
