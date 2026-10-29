//! Opt-in OAuth bridge: Claude receives MCP-only tokens after Xero PKCE login.

use crate::config::AccessScope;
use crate::oauth_store::{
    AccessGrant, AuthorizationRequest, ConnectionBinding, OAuthStore, PendingAuthorization,
    TokenPair,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use axum::{
    extract::DefaultBodyLimit,
    extract::{Form, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use sha2::Digest;
use std::{collections::HashMap, sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use url::Url;
use uuid::Uuid;
use xero_store::{ConnectionStore, StoredConnection};

const CLAUDE_REDIRECT: &str = "https://claude.ai/api/mcp/auth_callback";
const READ_SCOPE: &str = "xero.read";

fn cookie_name(config: &OAuthConfig) -> &'static str {
    if config.issuer.starts_with("https://") {
        "__Host-xero_mcp_oauth_state"
    } else {
        "xero_mcp_oauth_state"
    }
}

fn valid_browser_state(jar: &CookieJar, config: &OAuthConfig, state: &str) -> bool {
    jar.get(cookie_name(config))
        .is_some_and(|cookie| bool::from(cookie.value().as_bytes().ct_eq(state.as_bytes())))
}

#[derive(Clone)]
pub struct OAuthConfig {
    pub issuer: String,
    pub client_id: String,
    pub xero_client_id: String,
    pub xero_client_secret: Option<String>,
    pub xero_redirect_uri: String,
    pub xero_scopes: String,
    pub access_policy: OAuthAccessPolicy,
}

/// Server-controlled capabilities for MCP tokens issued after a Xero sign-in.
/// Tenant access is always derived from the organizations returned by that sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthAccessPolicy {
    pub allow_extract: bool,
    pub allow_warehouse: bool,
    pub zero_data_retention: bool,
    pub write_actions: Vec<String>,
}

impl Default for OAuthAccessPolicy {
    fn default() -> Self {
        Self {
            allow_extract: false,
            allow_warehouse: false,
            zero_data_retention: true,
            write_actions: Vec::new(),
        }
    }
}

impl OAuthConfig {
    pub fn from_env() -> Result<Option<Self>> {
        Self::from_map(&std::env::vars().collect())
    }

    pub fn from_map(vars: &HashMap<String, String>) -> Result<Option<Self>> {
        let Some(issuer) = vars.get("XERO_MCP_OAUTH_ISSUER") else {
            anyhow::ensure!(
                !vars.keys().any(|key| key.starts_with("XERO_MCP_OAUTH_")),
                "XERO_MCP_OAUTH_ISSUER is required when MCP OAuth is configured"
            );
            return Ok(None);
        };
        let url = Url::parse(issuer).context("invalid MCP OAuth issuer URL")?;
        let local =
            url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
        anyhow::ensure!(
            url.scheme() == "https" || local,
            "MCP OAuth issuer requires HTTPS"
        );
        anyhow::ensure!(
            url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty()
                && url.password().is_none(),
            "MCP OAuth issuer must be an origin without path or credentials"
        );
        anyhow::ensure!(
            vars.get("XERO_AUTH_MODE").map(String::as_str) == Some("pkce"),
            "MCP OAuth requires XERO_AUTH_MODE=pkce"
        );
        let required = |name: &str| -> Result<String> {
            vars.get(name)
                .filter(|s| !s.trim().is_empty())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("{name} is required for MCP OAuth"))
        };
        let policy_flag = |name: &str, default: bool| -> Result<bool> {
            vars.get(name)
                .map(|value| {
                    value
                        .parse::<bool>()
                        .map_err(|_| anyhow::anyhow!("{name} must be true or false"))
                })
                .unwrap_or(Ok(default))
        };
        let policy_actions = |name: &str| -> Result<Vec<String>> {
            let actions = vars
                .get(name)
                .map(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            anyhow::ensure!(
                actions
                    .iter()
                    .all(|action| crate::writes::ACTIONS.contains(&action.as_str()))
                    && actions
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == actions.len(),
                "{name} contains an unknown or duplicate Xero write action"
            );
            Ok(actions)
        };
        let issuer = issuer.trim_end_matches('/').to_owned();
        let xero_redirect_uri = required("XERO_MCP_OAUTH_XERO_REDIRECT_URI")?;
        anyhow::ensure!(
            xero_redirect_uri == format!("{issuer}/oauth/xero/callback"),
            "Xero redirect URI must be the exact MCP OAuth callback"
        );
        let xero_scopes = required("XERO_MCP_OAUTH_SCOPES")?;
        let scope_set = xero_scopes.split_whitespace().collect::<Vec<_>>();
        anyhow::ensure!(
            scope_set.contains(&"openid") && scope_set.contains(&"offline_access"),
            "MCP OAuth Xero scopes require openid and offline_access"
        );
        let access_policy = OAuthAccessPolicy {
            allow_extract: policy_flag("XERO_MCP_OAUTH_ALLOW_EXTRACT", false)?,
            allow_warehouse: policy_flag("XERO_MCP_OAUTH_ALLOW_WAREHOUSE", false)?,
            zero_data_retention: policy_flag("XERO_MCP_OAUTH_ZERO_DATA_RETENTION", true)?,
            write_actions: policy_actions("XERO_MCP_OAUTH_WRITE_ACTIONS")?,
        };
        anyhow::ensure!(
            !access_policy.allow_extract || !access_policy.zero_data_retention,
            "XERO_MCP_OAUTH_ALLOW_EXTRACT requires XERO_MCP_OAUTH_ZERO_DATA_RETENTION=false"
        );
        anyhow::ensure!(
            access_policy.write_actions.is_empty() || !access_policy.zero_data_retention,
            "XERO_MCP_OAUTH_WRITE_ACTIONS requires XERO_MCP_OAUTH_ZERO_DATA_RETENTION=false"
        );
        Ok(Some(Self {
            issuer,
            client_id: required("XERO_MCP_OAUTH_CLIENT_ID")?,
            xero_client_id: required("XERO_PKCE_CLIENT_ID")?,
            xero_client_secret: vars
                .get("XERO_PKCE_CLIENT_SECRET")
                .filter(|s| !s.is_empty())
                .cloned(),
            xero_redirect_uri,
            xero_scopes,
            access_policy,
        }))
    }

    pub fn resource(&self) -> String {
        format!("{}/mcp", self.issuer)
    }

    pub fn metadata_url(&self) -> String {
        format!("{}/.well-known/oauth-protected-resource/mcp", self.issuer)
    }

    pub fn access_scope(&self, subject: &str, tenant_ids: Vec<String>) -> Result<AccessScope> {
        let digest = sha2::Sha256::digest(subject.as_bytes());
        let mut scope = AccessScope::new(
            &format!("xero-user-{digest:x}"),
            tenant_ids,
            self.access_policy.allow_extract,
            false,
            self.access_policy.zero_data_retention,
        )?;
        scope.allow_warehouse = self.access_policy.allow_warehouse;
        scope.with_write_actions(self.access_policy.write_actions.clone())
    }
}

#[derive(Clone)]
pub struct OAuthState {
    pub config: OAuthConfig,
    pub store: OAuthStore,
    xero_store: Arc<dyn ConnectionStore>,
    gateway: Arc<dyn XeroOAuthGateway>,
}

pub struct XeroLogin {
    pub subject: String,
    pub refresh_token: String,
    pub scopes: Vec<String>,
    pub connections: Vec<xero_auth::XeroConnection>,
}

#[async_trait]
pub trait XeroOAuthGateway: Send + Sync {
    async fn login(
        &self,
        config: &OAuthConfig,
        code: &str,
        verifier: &str,
        nonce: &str,
    ) -> Result<XeroLogin>;
}

struct LiveXeroGateway {
    http: reqwest::Client,
}

#[async_trait]
impl XeroOAuthGateway for LiveXeroGateway {
    async fn login(
        &self,
        config: &OAuthConfig,
        code: &str,
        verifier: &str,
        nonce: &str,
    ) -> Result<XeroLogin> {
        let token = xero_auth::exchange_code(
            &self.http,
            &config.xero_client_id,
            config.xero_client_secret.as_deref(),
            &config.xero_redirect_uri,
            code,
            verifier,
        )
        .await?;
        let id_token = token.id_token.as_deref().context("Xero ID token missing")?;
        let identity = xero_auth::identity::verify_xero_id_token(
            &self.http,
            id_token,
            &config.xero_client_id,
            nonce,
        )
        .await?;
        anyhow::ensure!(
            !token.refresh_token.is_empty(),
            "Xero refresh token missing"
        );
        let connections = xero_auth::fetch_connections(&self.http, &token.access_token).await?;
        Ok(XeroLogin {
            subject: identity.subject,
            refresh_token: token.refresh_token.clone(),
            scopes: token.scopes(),
            connections,
        })
    }
}

impl OAuthState {
    pub fn with_store(
        config: OAuthConfig,
        store: OAuthStore,
        xero_store: Arc<dyn ConnectionStore>,
    ) -> Self {
        Self::with_gateway(
            config,
            store,
            xero_store,
            Arc::new(LiveXeroGateway {
                http: reqwest::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .build()
                    .expect("valid fixed HTTP client configuration"),
            }),
        )
    }

    pub fn with_gateway(
        config: OAuthConfig,
        store: OAuthStore,
        xero_store: Arc<dyn ConnectionStore>,
        gateway: Arc<dyn XeroOAuthGateway>,
    ) -> Self {
        Self {
            config,
            store,
            xero_store,
            gateway,
        }
    }

    pub async fn grant_is_current(&self, grant: &AccessGrant) -> Result<bool> {
        if grant.tenant_bindings.is_empty() {
            return Ok(false);
        }
        let current = self.xero_store.list().await?;
        Ok(grant.tenant_bindings.iter().all(|binding| {
            current.iter().any(|connection| {
                connection.tenant_id == binding.tenant_id
                    && connection.connected_at == binding.connected_at
            })
        }))
    }
}

#[derive(Deserialize)]
struct AuthorizeQuery {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
    resource: String,
    scope: Option<String>,
}

#[derive(Deserialize)]
struct XeroCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    client_id: String,
    resource: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct RegistrationRequest {
    redirect_uris: Vec<String>,
    token_endpoint_auth_method: Option<String>,
    grant_types: Option<Vec<String>>,
    response_types: Option<Vec<String>>,
    scope: Option<String>,
}

#[derive(Deserialize)]
struct ApproveForm {
    state: String,
    decision: String,
}

fn invalid_request() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":"invalid_request"})),
    )
        .into_response()
}

fn registration_error(error: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error":error}))).into_response()
}

fn server_error() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error":"temporarily_unavailable"})),
    )
        .into_response()
}

fn redirect_client(
    pending: &PendingAuthorization,
    key: &str,
    value: &str,
    issuer: &str,
) -> Response {
    let Ok(mut url) = Url::parse(&pending.0.redirect_uri) else {
        return server_error();
    };
    url.query_pairs_mut()
        .append_pair(key, value)
        .append_pair("state", &pending.0.client_state)
        .append_pair("iss", issuer);
    Redirect::to(url.as_str()).into_response()
}

async fn authorize(
    State(state): State<OAuthState>,
    jar: CookieJar,
    Query(query): Query<AuthorizeQuery>,
) -> Response {
    let config = &state.config;
    if query.response_type != "code"
        || query.client_id != config.client_id
        || query.redirect_uri != CLAUDE_REDIRECT
        || query.code_challenge_method != "S256"
        || query.resource != config.resource()
        || query
            .scope
            .as_deref()
            .is_some_and(|scope| scope != READ_SCOPE)
    {
        return invalid_request();
    }
    let upstream_pkce = xero_auth::PkceChallenge::generate();
    let nonce = xero_auth::random_state();
    let request = AuthorizationRequest {
        client_id: query.client_id,
        resource: query.resource,
        redirect_uri: query.redirect_uri,
        client_state: query.state,
        code_challenge: query.code_challenge,
        upstream_verifier: upstream_pkce.verifier,
        nonce: nonce.clone(),
    };
    let upstream_state = match state.store.begin(request) {
        Ok(value) => value,
        Err(_) => return invalid_request(),
    };
    let html = format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Connect Xero to Claude</title><style>
body{{font-family:system-ui,sans-serif;max-width:38rem;margin:8vh auto;padding:0 1.5rem;color:#1b2731;line-height:1.5}}
h1{{font-size:1.65rem}}p{{margin:1rem 0}}.actions{{display:flex;gap:.75rem;margin-top:2rem}}
button{{border:1px solid #315667;border-radius:4px;padding:.7rem 1rem;background:#fff;color:#1b2731;font:inherit;cursor:pointer}}
button[value=approve]{{background:#176784;color:#fff;border-color:#176784}}
</style></head><body><main><h1>Connect Xero to Claude</h1>
<p>Continue to Xero to choose the organizations you want Claude to read through this integration.</p>
<p>Claude will receive a read-only MCP token for those organizations. Your Xero access and refresh tokens stay on this server. This access expires after 24 hours unless you sign in again.</p>
<p>After sign-in, you will return to <strong>claude.ai</strong>.</p>
<form method="post" action="/oauth/approve"><input type="hidden" name="state" value="{upstream_state}">
<div class="actions"><button name="decision" value="approve" type="submit">Continue to Xero</button>
<button name="decision" value="cancel" type="submit">Cancel</button></div></form></main></body></html>"##
    );
    let mut response = Html(html).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response.headers_mut().insert("content-security-policy", "default-src 'none'; style-src 'unsafe-inline'; form-action 'self' https://login.xero.com https://authorize.xero.com https://claude.ai; base-uri 'none'; frame-ancestors 'none'".parse().unwrap());
    let cookie = Cookie::build((cookie_name(config), upstream_state))
        .path("/")
        .http_only(true)
        .secure(config.issuer.starts_with("https://"))
        .same_site(SameSite::Lax)
        .build();
    (jar.add(cookie), response).into_response()
}

async fn approve(
    State(state): State<OAuthState>,
    jar: CookieJar,
    Form(form): Form<ApproveForm>,
) -> Response {
    if !valid_browser_state(&jar, &state.config, &form.state) {
        return invalid_request();
    }
    if form.decision == "cancel" {
        return match state.store.cancel(&form.state) {
            Ok(Some(pending)) => {
                redirect_client(&pending, "error", "access_denied", &state.config.issuer)
            }
            Ok(None) => invalid_request(),
            Err(_) => server_error(),
        };
    }
    if form.decision != "approve" {
        return invalid_request();
    }
    let pending = match state.store.approve(&form.state) {
        Ok(Some(value)) => value,
        Ok(None) => return invalid_request(),
        Err(_) => return server_error(),
    };
    let challenge = xero_auth::PkceChallenge::challenge_for(&pending.0.upstream_verifier);
    let url = xero_auth::authorize_url_with_nonce(
        &state.config.xero_client_id,
        &state.config.xero_redirect_uri,
        &state.config.xero_scopes,
        &form.state,
        &challenge,
        &pending.0.nonce,
    );
    Redirect::to(&url).into_response()
}

async fn xero_callback(
    State(state): State<OAuthState>,
    jar: CookieJar,
    Query(callback): Query<XeroCallback>,
) -> Response {
    let Some(upstream_state) = callback.state.as_deref() else {
        return invalid_request();
    };
    if !valid_browser_state(&jar, &state.config, upstream_state) {
        return invalid_request();
    }
    let pending = match state.store.finish(upstream_state) {
        Ok(Some(value)) => value,
        Ok(None) => return invalid_request(),
        Err(_) => return server_error(),
    };
    if callback.error.is_some() {
        return redirect_client(&pending, "error", "access_denied", &state.config.issuer);
    }
    let Some(code) = callback.code.as_deref() else {
        return redirect_client(&pending, "error", "invalid_request", &state.config.issuer);
    };
    let login = match state
        .gateway
        .login(
            &state.config,
            code,
            &pending.0.upstream_verifier,
            &pending.0.nonce,
        )
        .await
    {
        Ok(value) => value,
        Err(_) => return redirect_client(&pending, "error", "server_error", &state.config.issuer),
    };
    if login.subject.is_empty() || login.refresh_token.is_empty() {
        return redirect_client(&pending, "error", "server_error", &state.config.issuer);
    }
    let records = login
        .connections
        .into_iter()
        .filter(|connection| connection.tenant_type.eq_ignore_ascii_case("ORGANISATION"))
        .filter_map(|connection| {
            let tenant_id = Uuid::parse_str(&connection.tenant_id).ok()?.to_string();
            Some(StoredConnection {
                tenant_name: connection.tenant_name.unwrap_or_else(|| tenant_id.clone()),
                tenant_id,
                tenant_type: connection.tenant_type,
                connected_at: Utc::now(),
                scopes: login.scopes.clone(),
                refresh_token: login.refresh_token.clone(),
            })
        })
        .collect::<Vec<_>>();
    if records.is_empty() || state.xero_store.upsert_many(records.clone()).await.is_err() {
        return redirect_client(&pending, "error", "server_error", &state.config.issuer);
    }
    let bindings = records
        .iter()
        .map(|record| ConnectionBinding {
            tenant_id: record.tenant_id.clone(),
            connected_at: record.connected_at,
        })
        .collect::<Vec<_>>();
    match state
        .store
        .issue_code(pending.clone(), &login.subject, &bindings)
    {
        Ok(code) => redirect_client(&pending, "code", &code, &state.config.issuer),
        Err(_) => redirect_client(&pending, "error", "server_error", &state.config.issuer),
    }
}

async fn token(State(state): State<OAuthState>, Form(form): Form<TokenForm>) -> Response {
    if form.client_id != state.config.client_id
        || form.resource.as_deref() != Some(state.config.resource().as_str())
    {
        return invalid_request();
    }
    let result = match form.grant_type.as_str() {
        "authorization_code" => {
            let (Some(code), Some(redirect), Some(verifier)) = (
                form.code.as_deref(),
                form.redirect_uri.as_deref(),
                form.code_verifier.as_deref(),
            ) else {
                return invalid_request();
            };
            state
                .store
                .exchange_code(code, &form.client_id, redirect, verifier)
        }
        "refresh_token" => {
            let Some(refresh) = form.refresh_token.as_deref() else {
                return invalid_request();
            };
            state.store.refresh(refresh, &form.client_id)
        }
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"unsupported_grant_type"})),
            )
                .into_response()
        }
    };
    match result {
        Ok(Some(TokenPair {
            access_token,
            refresh_token,
            expires_in,
        })) => (
            StatusCode::OK,
            [
                (header::CACHE_CONTROL, "no-store"),
                (header::PRAGMA, "no-cache"),
            ],
            Json(
                json!({"access_token":access_token,"refresh_token":refresh_token,
                "token_type":"Bearer","expires_in":expires_in,"scope":READ_SCOPE}),
            ),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_grant"})),
        )
            .into_response(),
        Err(_) => server_error(),
    }
}

async fn register(
    State(state): State<OAuthState>,
    Json(request): Json<RegistrationRequest>,
) -> Response {
    let grants_valid = request.grant_types.as_ref().is_none_or(|grants| {
        grants.iter().any(|grant| grant == "authorization_code")
            && grants
                .iter()
                .all(|grant| grant == "authorization_code" || grant == "refresh_token")
    });
    let responses_valid = request
        .response_types
        .as_ref()
        .is_none_or(|responses| responses == &["code"]);
    if request.redirect_uris != [CLAUDE_REDIRECT] {
        return registration_error("invalid_redirect_uri");
    }
    if request
        .token_endpoint_auth_method
        .as_deref()
        .is_some_and(|method| method != "none")
        || !grants_valid
        || !responses_valid
        || request
            .scope
            .as_deref()
            .is_some_and(|scope| scope != READ_SCOPE)
    {
        return registration_error("invalid_client_metadata");
    }
    (
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "client_id": state.config.client_id,
            "redirect_uris": [CLAUDE_REDIRECT],
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "application_type": "web",
            "client_name": "Claude",
            "scope": READ_SCOPE
        })),
    )
        .into_response()
}

async fn resource_metadata(State(state): State<OAuthState>) -> impl IntoResponse {
    Json(json!({
        "resource":state.config.resource(),
        "authorization_servers":[state.config.issuer],
        "scopes_supported":[READ_SCOPE]
    }))
}

async fn server_metadata(State(state): State<OAuthState>) -> impl IntoResponse {
    let issuer = &state.config.issuer;
    Json(json!({
        "issuer":issuer,
        "authorization_endpoint":format!("{issuer}/oauth/authorize"),
        "token_endpoint":format!("{issuer}/oauth/token"),
        "registration_endpoint":format!("{issuer}/oauth/register"),
        "response_types_supported":["code"],
        "grant_types_supported":["authorization_code","refresh_token"],
        "code_challenge_methods_supported":["S256"],
        "token_endpoint_auth_methods_supported":["none"],
        "scopes_supported":[READ_SCOPE],
        "authorization_response_iss_parameter_supported":true
    }))
}

pub fn router(state: OAuthState) -> Router {
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(server_metadata),
        )
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/register", post(register))
        .route("/oauth/approve", post(approve))
        .route("/oauth/xero/callback", get(xero_callback))
        .route("/oauth/token", post(token))
        .layer(DefaultBodyLimit::max(8 * 1024))
        .with_state(state)
}
