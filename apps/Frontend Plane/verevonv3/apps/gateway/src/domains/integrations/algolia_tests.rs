use super::algolia::*;
use crate::middleware::{AuthenticatedUser, AuthorizedMembership};
use axum::{
    body::{to_bytes, Bytes},
    http::{HeaderMap, StatusCode},
};
use base64::Engine;
use serde_json::json;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};
fn actor(org: &str, role: &str) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "user-one".into(),
        user_email: "user@example.test".into(),
        user_name: "Test".into(),
        user_image: None,
        email_verified: true,
        auth_role: Some(role.into()),
        active_org_id: Some(org.into()),
        authorized_membership: Some(AuthorizedMembership {
            organization_id: org.into(),
            role: role.into(),
        }),
    }
}
fn token(org: &str) -> String {
    format!(
        "e30.{}.sig",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json!({"org_id":org}).to_string())
    )
}
const ID: &str = "conn_11111111-1111-4111-8111-111111111111";
const SESSION: &str = "cs_11111111-1111-4111-8111-111111111111";
fn auth_url() -> String {
    "https://dashboard.algolia.com/2/oauth/authorize?response_type=code&client_id=public-client&redirect_uri=http%3A%2F%2Flocalhost%3A3026%2Fapi%2Fv1%2Fproviders%2Falgolia%2Fcallback&scope=public&state=opaque-state&code_challenge=abcdefghijklmnopqrstuvwxyz0123456789ABCDE12&code_challenge_method=S256&resource=https%3A%2F%2Fmcp.algolia.com%2Fmcp".into()
}
#[test]
fn algolia_metadata_projection_is_bounded_and_provider_specific() {
    let row = json!({"id":ID,"providerKey":"algolia","organizationId":"org-one","status":"active","displayName":"Account","providerContext":{"token":"private"},"accessToken":"private"});
    let projected = project(
        &Operation::Connections,
        &json!({"connections":[row.clone()]}),
        "org-one",
    )
    .unwrap();
    assert_eq!(
        projected,
        json!({"connections":[{"id":ID,"status":"active","displayName":"Account"}]})
    );
    assert!(!projected.to_string().contains("private"));
    assert!(project(
        &Operation::Connections,
        &json!({"connections":vec![row;65]}),
        "org-one"
    )
    .is_none());
    assert!(project(
        &Operation::Connections,
        &json!({"connections":[{"id":ID,"providerKey":"google","status":"active"}]}),
        "org-one"
    )
    .is_none());
    assert_eq!(
        project(
            &Operation::Applications(ID.into()),
            &json!({"applications":[{"id":"APP1","schema":"private"}]}),
            "org-one"
        ),
        Some(json!({"applications":[{"id":"APP1"}]}))
    );
    assert!(project(
        &Operation::Indices(ID.into(), "APP1".into()),
        &json!({"indices":[{"name":"bad index"}]}),
        "org-one"
    )
    .is_none());
    assert!(project(&Operation::Status(SESSION.into()),&json!({"id":SESSION,"providerKey":"google","status":"pending","expiresAt":"2026-10-23T16:00:00Z"}),"org-one").is_none());
}
#[test]
fn algolia_authorization_requires_fixed_readonly_public_client_contract() {
    assert!(authorization_url(&auth_url()));
    for value in [
        auth_url().replace("dashboard.algolia.com", "evil.example"),
        auth_url().replace("scope=public", "scope=admin"),
        auth_url() + "&scope=public",
        auth_url().replace("S256", "plain"),
        auth_url() + "#secret",
    ] {
        assert!(!authorization_url(&value));
    }
    let value = json!({"sessionToken":SESSION,"authMode":"direct-oauth","providerConfigKey":"algolia","authorizationUrl":auth_url(),"expiresAt":"2026-10-23T16:00:00Z","token":"private"});
    let result = project(&Operation::Connect, &value, "org-one").unwrap();
    assert_eq!(result["id"], SESSION);
    assert!(!result.to_string().contains("private"));
}
#[tokio::test]
async fn algolia_guard_rejects_forged_authority_before_mint_or_upstream() {
    let server = MockServer::start().await;
    let mut state = crate::tests::test_state(false);
    state.auth_core_url = server.uri();
    state.integration_core_url = server.uri();
    let mut headers = HeaderMap::new();
    headers.insert("origin", state.verevon_public_origin.parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    let body = Bytes::from(format!(
        r#"{{"returnUrl":"{}/studio/pulse"}}"#,
        state.verevon_public_origin
    ));
    assert_eq!(
        proxy(
            &state,
            &actor("org-one", "viewer"),
            &headers,
            Operation::Connect,
            body.clone()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    headers.insert("origin", "https://evil.example".parse().unwrap());
    assert_eq!(
        proxy(
            &state,
            &actor("org-one", "admin"),
            &headers,
            Operation::Connect,
            body
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        proxy(
            &state,
            &actor("org-one", "admin"),
            &headers,
            Operation::Applications("bad/id".into()),
            Bytes::new()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}
#[tokio::test]
async fn algolia_delegates_only_current_user_org_token_and_metadata() {
    let auth = MockServer::start().await;
    let integration = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/ingestion/token"))
        .and(header("cookie", "session=real-user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token":token("org-one")})))
        .expect(1)
        .mount(&auth)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/api/v1/providers/algolia/connections/{ID}/applications"
        )))
        .and(header(
            "authorization",
            format!("Bearer {}", token("org-one")),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"success":true,"data":{"applications":[{"id":"APP1","token":"private"}]}}),
        ))
        .expect(1)
        .mount(&integration)
        .await;
    let mut state = crate::tests::test_state(false);
    state.auth_core_url = auth.uri();
    state.integration_core_url = integration.uri();
    let mut headers = HeaderMap::new();
    headers.insert("cookie", "session=real-user".parse().unwrap());
    headers.insert("x-org-id", "forged".parse().unwrap());
    headers.insert("authorization", "Bearer forged-browser".parse().unwrap());
    let response = proxy(
        &state,
        &actor("org-one", "admin"),
        &headers,
        Operation::Applications(ID.into()),
        Bytes::new(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let raw = to_bytes(response.into_body(), 8192).await.unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("private"));
    for request in integration.received_requests().await.unwrap() {
        for name in ["cookie", "x-org-id", "x-user-id", "x-internal-api-key"] {
            assert!(!request.headers.contains_key(name));
        }
    }
}
#[tokio::test]
async fn algolia_wrong_org_minted_token_never_reaches_provider() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/ingestion/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token":token("other-org")})))
        .expect(1)
        .mount(&server)
        .await;
    let mut state = crate::tests::test_state(false);
    state.auth_core_url = server.uri();
    state.integration_core_url = server.uri();
    assert_eq!(
        proxy(
            &state,
            &actor("org-one", "admin"),
            &HeaderMap::new(),
            Operation::Connections,
            Bytes::new()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn algolia_generic_provider_aliases_cannot_bypass_admin_or_origin_guards() {
    use axum::{body::Body, http::Request, routing::post, Extension, Router};
    use tower::ServiceExt;
    let server = MockServer::start().await;
    let mut state = crate::tests::test_state(false);
    state.auth_core_url = server.uri();
    state.integration_core_url = server.uri();
    for (role, origin) in [
        ("member", state.verevon_public_origin.as_str()),
        ("admin", "https://other.example"),
    ] {
        let app = Router::new()
            .route(
                "/api/v1/integrations/providers/{provider}/connect-session",
                post(super::providers::start_connect_session),
            )
            .layer(Extension(actor("org-one", role)))
            .with_state(state.clone());
        for provider in ["algolia", "Algolia", "%20ALGOLIA%20", "%09algolia%09"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!(
                            "/api/v1/integrations/providers/{provider}/connect-session"
                        ))
                        .header("content-type", "application/json")
                        .header("origin", origin)
                        .body(Body::from(format!(
                            r#"{{"returnUrl":"{}/studio/pulse"}}"#,
                            state.verevon_public_origin
                        )))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "{provider} {role}"
            );
        }
        // Go's Unicode case mapping converts U+0130 to ASCII i. Reject this
        // alias rather than forwarding through the unguarded generic branch.
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/integrations/providers/ALGOL%C4%B0A/connect-session")
                    .header("content-type", "application/json")
                    .header("origin", origin)
                    .body(Body::from(format!(
                        r#"{{"returnUrl":"{}/studio/pulse"}}"#,
                        state.verevon_public_origin
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn algolia_connect_projects_authorization_but_no_provider_credentials() {
    let auth = MockServer::start().await;
    let integration = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/ingestion/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token":token("org-one")})))
        .mount(&auth)
        .await;
    Mock::given(method("POST")).and(path("/api/v1/providers/algolia/connect-session"))
        .and(wiremock::matchers::body_json(json!({"returnUrl":"http://localhost:5173/studio/pulse","capabilities":["search.analytics.read"]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true,"data":{"sessionToken":SESSION,"authMode":"direct-oauth","providerConfigKey":"algolia","authorizationUrl":auth_url(),"expiresAt":"2026-10-23T16:00:00Z","refreshToken":"private"}}))).expect(1).mount(&integration).await;
    let mut state = crate::tests::test_state(false);
    state.auth_core_url = auth.uri();
    state.integration_core_url = integration.uri();
    state.verevon_public_origin = "http://localhost:5173".into();
    let mut headers = HeaderMap::new();
    headers.insert("origin", state.verevon_public_origin.parse().unwrap());
    headers.insert("content-type", "application/json".parse().unwrap());
    let body = Bytes::from_static(br#"{"returnUrl":"http://localhost:5173/studio/pulse"}"#);
    let response = proxy(
        &state,
        &actor("org-one", "owner"),
        &headers,
        Operation::Connect,
        body,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let raw = to_bytes(response.into_body(), 8192).await.unwrap();
    let raw = String::from_utf8_lossy(&raw);
    assert!(!raw.contains("private"));
    assert!(!raw.contains("sessionToken"));
    assert!(raw.contains("authorizationUrl"));
}

#[test]
fn algolia_discovery_limits_are_explicit_and_never_silent_truncation() {
    let apps: Vec<_> = (0..128).map(|i| json!({"id":format!("APP{i}")})).collect();
    assert!(project(
        &Operation::Applications(ID.into()),
        &json!({"applications":apps}),
        "org-one"
    )
    .is_some());
    let indices: Vec<_> = (0..5000)
        .map(|i| json!({"name":format!("index_{i}")}))
        .collect();
    assert_eq!(
        project(
            &Operation::Indices(ID.into(), "APP1".into()),
            &json!({"indices":indices}),
            "org-one"
        )
        .unwrap()["indices"]
            .as_array()
            .unwrap()
            .len(),
        5000
    );
    assert!(project(
        &Operation::Applications(ID.into()),
        &json!({"applications":vec![json!({"id":"APP1"});129]}),
        "org-one"
    )
    .is_none());
    assert!(project(&Operation::Status(SESSION.into()),&json!({"id":SESSION,"providerKey":"algolia","status":"pending","expiresAt":"2000-01-01T00:00:00Z"}),"org-one").unwrap()["status"]=="expired");
}
