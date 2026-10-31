//! HTTP handlers for the onboarding BFF.
//!
//! Flow: `/api/auth/login` → Xero → `/api/auth/callback` → persist tenants +
//! refresh token → redirect to the SPA. `/api/connections` lists what's stored.

use axum::{
    extract::{Query, State},
    http::{header, uri::Authority, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use axum_extra::extract::cookie::{Cookie, PrivateCookieJar, SameSite};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use xero_store::StoredConnection;

use crate::{config::BffConfig, AppState};

const PKCE_COOKIE: &str = "xero_pkce";
pub(crate) const SESSION_COOKIE: &str = "xero_session";

/// Liveness probe.
pub async fn health() -> &'static str {
    "ok"
}

/// Start the OAuth2 + PKCE authorization: mint state + verifier, stash them in
/// an encrypted cookie, and 302 the browser to Xero.
pub async fn login(
    State(st): State<AppState>,
    headers: HeaderMap,
    jar: PrivateCookieJar,
) -> Response {
    let request_host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    if let Some(url) = canonical_local_login_url(&st.cfg.redirect_uri, request_host) {
        return Redirect::to(&url).into_response();
    }

    let pkce = xero_auth::PkceChallenge::generate();
    let state = xero_auth::random_state();

    let pending = serde_json::json!({ "state": state, "verifier": pkce.verifier }).to_string();
    // 30-minute window: first-time consent (Xero login + 2FA + reviewing many
    // scopes across several orgs) routinely takes longer than 10 min, and if the
    // cookie expires mid-flow the callback fails with `missing_state_cookie`.
    let jar = jar.add(build_cookie(
        PKCE_COOKIE,
        pending,
        st.cfg.secure_cookies,
        30,
    ));

    let url = xero_auth::authorize_url(
        &st.cfg.client_id,
        &st.cfg.redirect_uri,
        &st.cfg.scopes,
        &state,
        &pkce.challenge,
    );
    (jar, Redirect::to(&url)).into_response()
}

fn canonical_local_login_url(redirect_uri: &str, request_host: Option<&str>) -> Option<String> {
    let callback = reqwest::Url::parse(redirect_uri).ok()?;
    if callback.host_str()? != "localhost" {
        return None;
    }

    let incoming = request_host?.parse::<Authority>().ok()?;
    let expected_port = callback.port_or_known_default()?;
    let default_port = match callback.scheme() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let incoming_port = incoming.port_u16().unwrap_or(default_port);
    if incoming.host().eq_ignore_ascii_case("localhost") && incoming_port == expected_port {
        return None;
    }

    callback
        .join("/api/auth/login")
        .ok()
        .map(|url| url.to_string())
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct Pending {
    state: String,
    verifier: String,
}

/// OAuth2 redirect target: validate state, exchange the code (with the PKCE
/// verifier) for tokens, record the connected tenants, and bounce to the SPA.
pub async fn callback(
    State(st): State<AppState>,
    jar: PrivateCookieJar,
    Query(params): Query<CallbackParams>,
) -> Response {
    if let Some(err) = params.error.as_deref() {
        tracing::warn!(error = err, desc = ?params.error_description, "xero authorization error");
        return spa_error(&st.cfg, jar, "access_denied").into_response();
    }

    let (Some(code), Some(state)) = (params.code.as_deref(), params.state.as_deref()) else {
        return spa_error(&st.cfg, jar, "missing_code").into_response();
    };

    let Some(pending_raw) = jar.get(PKCE_COOKIE).map(|c| c.value().to_owned()) else {
        return spa_error(&st.cfg, jar, "missing_state_cookie").into_response();
    };
    let Ok(pending) = serde_json::from_str::<Pending>(&pending_raw) else {
        return spa_error(&st.cfg, jar, "bad_state_cookie").into_response();
    };
    if pending.state != state {
        tracing::warn!("oauth state mismatch");
        return spa_error(&st.cfg, jar, "state_mismatch").into_response();
    }

    let token = match xero_auth::exchange_code(
        &st.http,
        &st.cfg.client_id,
        st.cfg.client_secret.as_deref(),
        &st.cfg.redirect_uri,
        code,
        &pending.verifier,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "token exchange failed");
            return spa_error(&st.cfg, jar, "token_exchange_failed").into_response();
        }
    };

    if token.refresh_token.is_empty() {
        tracing::error!("no refresh_token returned — is offline_access in the scopes?");
        return spa_error(&st.cfg, jar, "no_refresh_token").into_response();
    }

    let connections = match xero_auth::fetch_connections(&st.http, &token.access_token).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "connections fetch failed");
            return spa_error(&st.cfg, jar, "connections_failed").into_response();
        }
    };
    if connections.is_empty() {
        return spa_error(&st.cfg, jar, "no_connections").into_response();
    }

    // Xero issues one refresh token per authorization, valid for every tenant
    // granted in this flow. Store the same token against each connected tenant.
    let scopes = token.scopes();
    let records = connections
        .iter()
        .map(|conn| StoredConnection {
            tenant_id: conn.tenant_id.clone(),
            tenant_name: conn
                .tenant_name
                .clone()
                .unwrap_or_else(|| conn.tenant_id.clone()),
            tenant_type: conn.tenant_type.clone(),
            connected_at: Utc::now(),
            scopes: scopes.clone(),
            refresh_token: token.refresh_token.clone(),
        })
        .collect();
    if let Err(e) = st.store.upsert_many(records).await {
        tracing::error!(error = %e, "failed to persist connection grant");
        return spa_error(&st.cfg, jar, "store_failed").into_response();
    }
    tracing::info!(count = connections.len(), "onboarded xero connection(s)");

    let jar = clear_cookie(jar, PKCE_COOKIE, st.cfg.secure_cookies);
    let jar = jar.add(build_cookie(
        SESSION_COOKIE,
        xero_auth::random_state(),
        st.cfg.secure_cookies,
        60 * 24,
    ));
    (jar, Redirect::to(&st.cfg.spa_connected_path)).into_response()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionDto {
    tenant_id: String,
    tenant_name: String,
    tenant_type: String,
    connected_at: String,
    scopes: Vec<String>,
}

#[derive(Serialize)]
struct ConnectionsResponse {
    connections: Vec<ConnectionDto>,
}

/// List the connected tenants (session required). Refresh tokens are omitted.
pub async fn connections(State(st): State<AppState>, jar: PrivateCookieJar) -> Response {
    if jar.get(SESSION_COOKIE).is_none() {
        return (StatusCode::UNAUTHORIZED, "not authenticated").into_response();
    }

    match st.store.list().await {
        Ok(list) => {
            let connections = list
                .into_iter()
                .map(|c| ConnectionDto {
                    tenant_id: c.tenant_id,
                    tenant_name: c.tenant_name,
                    tenant_type: c.tenant_type,
                    connected_at: c.connected_at.to_rfc3339(),
                    scopes: c.scopes,
                })
                .collect();
            Json(ConnectionsResponse { connections }).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "list connections failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

/// Clear the session cookie.
pub async fn logout(State(st): State<AppState>, jar: PrivateCookieJar) -> impl IntoResponse {
    let jar = clear_cookie(jar, SESSION_COOKIE, st.cfg.secure_cookies);
    (jar, StatusCode::NO_CONTENT)
}

// ── cookie helpers ──────────────────────────────────────────────────────────

fn build_cookie(name: &'static str, value: String, secure: bool, minutes: i64) -> Cookie<'static> {
    Cookie::build((name, value))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(secure)
        .max_age(time::Duration::minutes(minutes))
        .build()
}

fn clear_cookie(jar: PrivateCookieJar, name: &'static str, secure: bool) -> PrivateCookieJar {
    jar.remove(Cookie::build((name, "")).path("/").secure(secure).build())
}

/// Redirect to the SPA error page with a short reason slug, clearing any pending
/// PKCE cookie. `reason` must be URL-safe (no spaces) — keep it a slug.
fn spa_error(cfg: &BffConfig, jar: PrivateCookieJar, reason: &str) -> (PrivateCookieJar, Redirect) {
    let jar = clear_cookie(jar, PKCE_COOKIE, cfg.secure_cookies);
    let url = format!("{}?reason={reason}", cfg.spa_error_path);
    (jar, Redirect::to(&url))
}

#[cfg(test)]
mod tests {
    use super::canonical_local_login_url;

    #[test]
    fn local_login_switches_to_the_registered_callback_host() {
        let callback = "http://localhost:5002/api/xero/callback";
        assert_eq!(
            canonical_local_login_url(callback, Some("127.0.0.1:5002")),
            Some("http://localhost:5002/api/auth/login".into())
        );
        assert_eq!(
            canonical_local_login_url(callback, Some("localhost:5002")),
            None
        );
    }

    #[test]
    fn production_login_is_not_rewritten_by_local_host_rule() {
        assert_eq!(
            canonical_local_login_url(
                "https://tools.aquatiq.com/api/auth/callback",
                Some("tools.aquatiq.com")
            ),
            None
        );
    }

    #[test]
    fn https_localhost_without_explicit_port_does_not_loop() {
        assert_eq!(
            canonical_local_login_url("https://localhost/api/xero/callback", Some("localhost")),
            None
        );
    }
}
