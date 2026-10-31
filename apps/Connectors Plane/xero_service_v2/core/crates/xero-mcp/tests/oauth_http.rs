use std::collections::HashMap;
use xero_mcp::oauth::OAuthConfig;

#[test]
fn oauth_is_opt_in_and_requires_a_pkce_source_and_exact_redirects() {
    assert!(OAuthConfig::from_map(&HashMap::new()).unwrap().is_none());
    let mut vars = HashMap::from([
        (
            "XERO_MCP_OAUTH_ISSUER".into(),
            "https://tools.aquatiq.com".into(),
        ),
        ("XERO_MCP_OAUTH_CLIENT_ID".into(), "claude-xero".into()),
        ("XERO_PKCE_CLIENT_ID".into(), "xero-app".into()),
        (
            "XERO_MCP_OAUTH_XERO_REDIRECT_URI".into(),
            "https://tools.aquatiq.com/oauth/xero/callback".into(),
        ),
        (
            "XERO_MCP_OAUTH_SCOPES".into(),
            "openid profile email offline_access accounting.transactions.read".into(),
        ),
        ("XERO_AUTH_MODE".into(), "pkce".into()),
    ]);
    let config = OAuthConfig::from_map(&vars).unwrap().unwrap();
    assert_eq!(config.resource(), "https://tools.aquatiq.com/mcp");
    assert!(!config.access_policy.allow_extract);
    assert!(!config.access_policy.allow_warehouse);
    assert!(config.access_policy.zero_data_retention);
    assert!(config.access_policy.write_actions.is_empty());
    vars.insert("XERO_MCP_OAUTH_ALLOW_EXTRACT".into(), "true".into());
    assert!(OAuthConfig::from_map(&vars).is_err());
    vars.insert("XERO_MCP_OAUTH_ZERO_DATA_RETENTION".into(), "false".into());
    vars.insert("XERO_MCP_OAUTH_ALLOW_WAREHOUSE".into(), "true".into());
    let configured = OAuthConfig::from_map(&vars).unwrap().unwrap();
    let scope = configured
        .access_scope(
            "xero-user",
            vec!["5f3c9a2e-0000-0000-0000-000000000000".into()],
        )
        .unwrap();
    assert!(scope.allow_extract);
    assert!(scope.allow_warehouse);
    assert!(!scope.zero_data_retention);
    vars.insert(
        "XERO_MCP_OAUTH_WRITE_ACTIONS".into(),
        "create_contact,update_invoice".into(),
    );
    let writer = OAuthConfig::from_map(&vars).unwrap().unwrap();
    let writer_scope = writer
        .access_scope(
            "xero-user",
            vec!["5f3c9a2e-0000-0000-0000-000000000000".into()],
        )
        .unwrap();
    assert_eq!(
        writer_scope.write_actions,
        ["create_contact", "update_invoice"]
    );
    vars.insert("XERO_MCP_OAUTH_ZERO_DATA_RETENTION".into(), "true".into());
    assert!(OAuthConfig::from_map(&vars).is_err());
    vars.insert("XERO_AUTH_MODE".into(), "custom".into());
    assert!(OAuthConfig::from_map(&vars).is_err());
    vars.insert("XERO_AUTH_MODE".into(), "pkce".into());
    vars.insert(
        "XERO_MCP_OAUTH_XERO_REDIRECT_URI".into(),
        "https://evil.example/callback".into(),
    );
    assert!(OAuthConfig::from_map(&vars).is_err());
}

#[tokio::test]
async fn oauth_metadata_and_bearer_tokens_protect_mcp() {
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::{path::Path, sync::Arc};
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;
    use xero_mcp::{
        backend::{Backend, ConnectionInfo, Failure, Query, Response},
        config::Settings,
        jobs::{Destination, ExtractionPlan},
        oauth::{OAuthState, XeroLogin, XeroOAuthGateway},
        oauth_store::OAuthStore,
        server::XeroMcp,
        transport,
    };
    use xero_store::{ConnectionStore, StoredConnection};

    struct Fixture;
    #[async_trait]
    impl Backend for Fixture {
        async fn connections(&self) -> Result<Vec<ConnectionInfo>, Failure> {
            Ok(Vec::new())
        }
        async fn fetch(&self, _: &Query) -> Result<Response, Failure> {
            Err(Failure::invalid("unused"))
        }
        async fn extract(
            &self,
            _: Uuid,
            _: &str,
            _: &ExtractionPlan,
            _: Destination,
            _: &Path,
        ) -> Result<Value, Failure> {
            Ok(json!({}))
        }
    }

    struct FakeGateway;
    #[async_trait]
    impl XeroOAuthGateway for FakeGateway {
        async fn login(
            &self,
            _: &OAuthConfig,
            code: &str,
            verifier: &str,
            nonce: &str,
        ) -> anyhow::Result<XeroLogin> {
            assert_eq!(code, "fixture-code");
            assert!(!verifier.is_empty() && !nonce.is_empty());
            Ok(XeroLogin {
                subject: "xero-user".into(),
                refresh_token: "private-xero-refresh".into(),
                scopes: vec!["accounting.transactions.read".into()],
                connections: vec![xero_auth::XeroConnection {
                    id: "connection-id".into(),
                    tenant_id: "5f3c9a2e-0000-0000-0000-000000000000".into(),
                    tenant_type: "ORGANISATION".into(),
                    tenant_name: Some("Fixture org".into()),
                }],
            })
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let tenant = "5f3c9a2e-0000-0000-0000-000000000000";
    let issuer = "http://127.0.0.1:5003";
    let settings = Settings::from_map(&HashMap::from([(
        "XERO_MCP_OAUTH_ISSUER".into(),
        issuer.into(),
    )]))
    .unwrap();
    let config = OAuthConfig::from_map(&HashMap::from([
        ("XERO_MCP_OAUTH_ISSUER".into(), issuer.into()),
        ("XERO_MCP_OAUTH_CLIENT_ID".into(), "claude-xero".into()),
        ("XERO_PKCE_CLIENT_ID".into(), "xero-app".into()),
        (
            "XERO_MCP_OAUTH_XERO_REDIRECT_URI".into(),
            format!("{issuer}/oauth/xero/callback"),
        ),
        (
            "XERO_MCP_OAUTH_SCOPES".into(),
            "openid offline_access accounting.transactions.read".into(),
        ),
        ("XERO_MCP_OAUTH_ALLOW_EXTRACT".into(), "true".into()),
        ("XERO_MCP_OAUTH_ALLOW_WAREHOUSE".into(), "true".into()),
        ("XERO_MCP_OAUTH_ZERO_DATA_RETENTION".into(), "false".into()),
        ("XERO_AUTH_MODE".into(), "pkce".into()),
    ]))
    .unwrap()
    .unwrap();
    let store = OAuthStore::open(directory.path().join("oauth.sqlite")).unwrap();
    let source_store = Arc::new(
        xero_store::FileConnectionStore::load(directory.path().join("connections.json")).unwrap(),
    );
    let state = OAuthState::with_gateway(
        config.clone(),
        store.clone(),
        source_store.clone(),
        Arc::new(FakeGateway),
    );
    let server = XeroMcp::new(
        Arc::new(Fixture),
        settings.local_scope.clone(),
        directory.path().into(),
    );
    let router =
        transport::router_with_oauth(server, &settings, CancellationToken::new(), Some(state))
            .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = reqwest::Client::new();
    let metadata: serde_json::Value = client
        .get(format!("{base}/.well-known/oauth-protected-resource/mcp"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(metadata["resource"], format!("{issuer}/mcp"));
    let authorization_server: serde_json::Value = client
        .get(format!("{base}/.well-known/oauth-authorization-server"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        authorization_server["registration_endpoint"],
        format!("{issuer}/oauth/register")
    );
    let invalid_registration = client
        .post(format!("{base}/oauth/register"))
        .json(&json!({
            "redirect_uris": ["https://attacker.example/callback"],
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid_registration.status(), 400);
    let invalid_registration_body: serde_json::Value = invalid_registration.json().await.unwrap();
    assert_eq!(invalid_registration_body["error"], "invalid_redirect_uri");
    let loopback_registration = client
        .post(format!("{base}/oauth/register"))
        .json(&json!({
            "application_type": "native",
            "redirect_uris": ["http://127.0.0.1:3118/callback"],
            "token_endpoint_auth_method": "none"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(loopback_registration.status(), 400);
    let confidential_registration = client
        .post(format!("{base}/oauth/register"))
        .json(&json!({
            "redirect_uris": ["https://claude.ai/api/mcp/auth_callback"],
            "token_endpoint_auth_method": "client_secret_basic"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(confidential_registration.status(), 400);
    let confidential_registration_body: serde_json::Value =
        confidential_registration.json().await.unwrap();
    assert_eq!(
        confidential_registration_body["error"],
        "invalid_client_metadata"
    );
    let write_registration = client
        .post(format!("{base}/oauth/register"))
        .json(&json!({
            "redirect_uris": ["https://claude.ai/api/mcp/auth_callback"],
            "token_endpoint_auth_method": "none",
            "grant_types": ["client_credentials"],
            "scope": "xero.write"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(write_registration.status(), 400);
    let registration = client
        .post(format!("{base}/oauth/register"))
        .json(&json!({
            "client_name": "Claude",
            "redirect_uris": ["https://claude.ai/api/mcp/auth_callback"],
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "scope": "xero.read"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(registration.status(), 201);
    assert_eq!(registration.headers()["cache-control"], "no-store");
    let registered: serde_json::Value = registration.json().await.unwrap();
    assert_eq!(registered["client_id"], "claude-xero");
    assert_eq!(registered["token_endpoint_auth_method"], "none");
    assert_eq!(registered["application_type"], "web");
    assert_eq!(
        registered["redirect_uris"],
        json!(["https://claude.ai/api/mcp/auth_callback"])
    );
    assert!(registered.get("client_secret").is_none());
    let repeated: serde_json::Value = client
        .post(format!("{base}/oauth/register"))
        .json(&json!({
            "application_type": "web",
            "redirect_uris": ["https://claude.ai/api/mcp/auth_callback"]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(repeated["client_id"], registered["client_id"]);
    assert_eq!(repeated["token_endpoint_auth_method"], "none");
    let unauthorized = client.post(format!("{base}/mcp")).send().await.unwrap();
    assert_eq!(unauthorized.status(), 401);
    assert!(unauthorized.headers()["www-authenticate"]
        .to_str()
        .unwrap()
        .contains("resource_metadata="));

    let verifier = "v".repeat(64);
    let challenge = xero_auth::PkceChallenge::challenge_for(&verifier);
    let approval = client
        .get(format!("{base}/oauth/authorize"))
        .query(&[
            ("response_type", "code"),
            ("client_id", "claude-xero"),
            ("redirect_uri", "https://claude.ai/api/mcp/auth_callback"),
            ("state", "claude-state"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("resource", "http://127.0.0.1:5003/mcp"),
            ("scope", "xero.read"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(approval.status(), 200);
    let policy = approval.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    let form_action = policy
        .split(';')
        .map(str::trim)
        .find(|directive| directive.starts_with("form-action "))
        .unwrap();
    assert_eq!(
        form_action,
        "form-action 'self' https://login.xero.com https://authorize.xero.com https://claude.ai"
    );
    let browser_cookie = approval.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body = approval.text().await.unwrap();
    assert!(body.contains("claude.ai"));
    assert!(body.contains("/oauth/approve"));
    let state = body
        .split("name=\"state\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let no_redirects = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let csrf = no_redirects
        .post(format!("{base}/oauth/approve"))
        .form(&[("state", state), ("decision", "approve")])
        .send()
        .await
        .unwrap();
    assert_eq!(csrf.status(), 400);
    let xero_redirect = no_redirects
        .post(format!("{base}/oauth/approve"))
        .header("cookie", browser_cookie.clone())
        .form(&[("state", state), ("decision", "approve")])
        .send()
        .await
        .unwrap();
    assert_eq!(xero_redirect.status(), 303);
    assert!(xero_redirect.headers()["location"]
        .to_str()
        .unwrap()
        .starts_with("https://login.xero.com/identity/connect/authorize?"));
    let callback = format!("{base}/oauth/xero/callback?state={state}&code=fixture-code");
    assert_eq!(
        no_redirects.get(&callback).send().await.unwrap().status(),
        400
    );
    let completed = no_redirects
        .get(&callback)
        .header("cookie", browser_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(completed.status(), 303);
    let redirect = url::Url::parse(completed.headers()["location"].to_str().unwrap()).unwrap();
    assert_eq!(redirect.origin().ascii_serialization(), "https://claude.ai");
    let code = redirect
        .query_pairs()
        .find(|(key, _)| key == "code")
        .unwrap()
        .1
        .into_owned();
    let connected = source_store.list().await.unwrap();
    assert_eq!(connected.len(), 1);
    assert_eq!(connected[0].tenant_id, tenant);
    assert_eq!(connected[0].refresh_token, "private-xero-refresh");
    let wrong_resource = client
        .post(format!("{base}/oauth/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", "claude-xero"),
            ("resource", "https://wrong.example/mcp"),
            ("code", code.as_str()),
            ("redirect_uri", "https://claude.ai/api/mcp/auth_callback"),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_resource.status(), 400);
    let issued: serde_json::Value = client
        .post(format!("{base}/oauth/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", "claude-xero"),
            ("resource", "http://127.0.0.1:5003/mcp"),
            ("code", code.as_str()),
            ("redirect_uri", "https://claude.ai/api/mcp/auth_callback"),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let access_token = issued["access_token"].as_str().unwrap();
    let refresh_token = issued["refresh_token"].as_str().unwrap();
    assert!(!issued.to_string().contains("private-xero-refresh"));
    let grant = store.access(access_token).unwrap().unwrap();
    assert_eq!(grant.user_sub, "xero-user");
    assert_eq!(grant.tenant_ids, vec![tenant]);
    let issued_scope = config
        .access_scope(&grant.user_sub, grant.tenant_ids.clone())
        .unwrap();
    assert!(issued_scope.allow_extract);
    assert!(issued_scope.allow_warehouse);
    assert!(!issued_scope.zero_data_retention);
    let authorized = client.post(format!("{base}/mcp"))
        .bearer_auth(access_token)
        .header("Accept", "application/json, text/event-stream")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"claude","version":"test"}}}))
        .send().await.unwrap();
    assert!(
        authorized.status().is_success(),
        "HTTP {}",
        authorized.status()
    );

    let mut changed_config = config;
    changed_config.issuer = "http://127.0.0.1:5004".into();
    let changed_state = OAuthState::with_gateway(
        changed_config,
        store.clone(),
        source_store.clone(),
        Arc::new(FakeGateway),
    );
    let changed_server = XeroMcp::new(
        Arc::new(Fixture),
        settings.local_scope.clone(),
        directory.path().into(),
    );
    let changed_router = transport::router_with_oauth(
        changed_server,
        &settings,
        CancellationToken::new(),
        Some(changed_state),
    )
    .unwrap();
    let changed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let changed_base = format!("http://{}", changed_listener.local_addr().unwrap());
    let changed_task = tokio::spawn(async move {
        axum::serve(changed_listener, changed_router).await.unwrap();
    });
    let wrong_audience = client
        .post(format!("{changed_base}/mcp"))
        .bearer_auth(access_token)
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_audience.status(), 401);
    changed_task.abort();

    source_store
        .upsert_many(vec![StoredConnection {
            tenant_id: tenant.into(),
            tenant_name: "Fixture org".into(),
            tenant_type: "ORGANISATION".into(),
            connected_at: chrono::Utc::now() + chrono::Duration::seconds(1),
            scopes: vec!["accounting.transactions.read".into()],
            refresh_token: "another-user-refresh".into(),
        }])
        .await
        .unwrap();
    let stale = client.post(format!("{base}/mcp"))
        .bearer_auth(access_token)
        .header("Accept", "application/json, text/event-stream")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"claude","version":"test"}}}))
        .send().await.unwrap();
    assert_eq!(stale.status(), 401);

    let refreshed: serde_json::Value = client
        .post(format!("{base}/oauth/token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", "claude-xero"),
            ("resource", "http://127.0.0.1:5003/mcp"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_ne!(refreshed["refresh_token"].as_str().unwrap(), refresh_token);
    let replay = client
        .post(format!("{base}/oauth/token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", "claude-xero"),
            ("resource", "http://127.0.0.1:5003/mcp"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), 400);
    let cancel_approval = client
        .get(format!("{base}/oauth/authorize"))
        .query(&[
            ("response_type", "code"),
            ("client_id", "claude-xero"),
            ("redirect_uri", "https://claude.ai/api/mcp/auth_callback"),
            ("state", "cancel-state"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("resource", "http://127.0.0.1:5003/mcp"),
            ("scope", "xero.read"),
        ])
        .send()
        .await
        .unwrap();
    let cancel_cookie = cancel_approval.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let cancel_body = cancel_approval.text().await.unwrap();
    let cancel_state = cancel_body
        .split("name=\"state\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let cancelled = no_redirects
        .post(format!("{base}/oauth/approve"))
        .header("cookie", cancel_cookie)
        .form(&[("state", cancel_state), ("decision", "cancel")])
        .send()
        .await
        .unwrap();
    assert_eq!(cancelled.status(), 303);
    let cancel_location =
        url::Url::parse(cancelled.headers()["location"].to_str().unwrap()).unwrap();
    assert_eq!(
        cancel_location.origin().ascii_serialization(),
        "https://claude.ai"
    );
    assert!(cancel_location
        .query_pairs()
        .any(|(key, value)| key == "error" && value == "access_denied"));
    task.abort();
}
