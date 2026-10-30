//! `xero-bff` — onboarding gateway for the SolidJS SPA.
//!
//! Runs the Authorization Code + PKCE flow against Xero on the browser's
//! behalf: the SPA never sees the client credential or the refresh token. On a
//! successful connect it persists the refresh token + connected tenants via a
//! [`ConnectionStore`] so the sync backend can pick them up.

mod config;
mod handlers;
mod sync;

use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use axum::{
    extract::{FromRef, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use axum_extra::extract::cookie::Key;
use base64::{engine::general_purpose::STANDARD, Engine};
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

use xero_store::ConnectionStore;

use crate::config::BffConfig;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<BffConfig>,
    pub http: reqwest::Client,
    pub store: Arc<dyn ConnectionStore>,
    pub key: Key,
    pub sync_state: sync::SharedSyncState,
}

// Lets `PrivateCookieJar` pull the encryption key out of the shared state.
impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.key.clone()
    }
}

/// SPA fallback: return index.html with a 200 for any non-API, non-asset path so
/// the client router can handle deep links (/connected, /auth/error).
async fn spa_index(State(state): State<AppState>) -> Response {
    let Some(dir) = state.cfg.static_dir.as_deref() else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let index = format!("{}/index.html", dir.trim_end_matches('/'));
    match tokio::fs::read(&index).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], bytes).into_response(),
        Err(e) => {
            tracing::error!(error = %e, path = %index, "failed to read SPA index");
            (StatusCode::INTERNAL_SERVER_ERROR, "spa index unavailable").into_response()
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,xero_bff=info")),
        )
        .init();

    let cfg = BffConfig::from_env()?;

    let key_bytes = STANDARD
        .decode(cfg.cookie_key_b64.as_bytes())
        .context("XERO_BFF_COOKIE_KEY must be valid base64")?;
    let key = Key::try_from(key_bytes.as_slice())
        .map_err(|_| anyhow::anyhow!("XERO_BFF_COOKIE_KEY must decode to >= 64 bytes"))?;

    let http = reqwest::Client::builder()
        .user_agent("xero-bff/0.1")
        .build()
        .context("failed to build HTTP client")?;

    let store: Arc<dyn ConnectionStore> = xero_store::open_from_env()
        .await
        .context("failed to open PKCE connection store")?;

    let bind_addr = cfg.bind_addr.clone();
    let state = AppState {
        cfg: Arc::new(cfg),
        http,
        store,
        key,
        sync_state: Arc::new(std::sync::Mutex::new(sync::SyncState::default())),
    };

    tracing::info!(
        redirect_uri = %state.cfg.redirect_uri,
        scopes = %state.cfg.scopes,
        confidential = state.cfg.client_secret.is_some(),
        "xero-bff config loaded"
    );

    let mut router = Router::new()
        .route("/api/health", get(handlers::health))
        .route("/api/auth/login", get(handlers::login))
        .route("/api/auth/callback", get(handlers::callback))
        .route("/api/xero/callback", get(handlers::callback))
        .route("/api/connections", get(handlers::connections))
        .route("/api/auth/logout", post(handlers::logout))
        .route("/api/sync", post(sync::start_sync))
        .route("/api/backfill", post(sync::start_backfill))
        .route("/api/sync/status", get(sync::sync_status));

    // Single-origin deploy: serve the built SPA. Real files under /assets/*;
    // every other non-API path returns index.html with 200 so client-side
    // routes (/connected, /auth/error) deep-link correctly. (ServeDir's
    // not_found_service preserved the 404 status for SPA routes — wrong.)
    if let Some(dir) = state.cfg.static_dir.as_deref() {
        let assets = format!("{}/assets", dir.trim_end_matches('/'));
        router = router
            .nest_service("/assets", ServeDir::new(assets))
            .fallback(spa_index);
        tracing::info!(static_dir = dir, "serving SPA static assets");
    }

    let app = router.layer(TraceLayer::new_for_http()).with_state(state);

    let addr: SocketAddr = bind_addr
        .parse()
        .with_context(|| format!("XERO_BFF_BIND must be host:port, got: {bind_addr}"))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, "xero-bff listening");

    axum::serve(listener, app).await.context("server error")?;
    Ok(())
}
