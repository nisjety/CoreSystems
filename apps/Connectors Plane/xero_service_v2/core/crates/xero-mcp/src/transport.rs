use crate::{
    config::{ClientCredential, Settings},
    oauth::{self, OAuthConfig, OAuthState},
    server::XeroMcp,
};
use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use rmcp::{
    transport::{
        stdio as stdio_transport,
        streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
    },
    ServiceExt,
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub async fn stdio(server: XeroMcp) -> anyhow::Result<()> {
    server.serve(stdio_transport()).await?.waiting().await?;
    Ok(())
}

#[derive(Clone)]
struct Guard {
    clients: Vec<ClientCredential>,
    oauth: Option<OAuthState>,
    rates: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
}

fn unauthorized(guard: &Guard) -> Response {
    let challenge = guard.oauth.as_ref().map_or_else(
        || "Bearer realm=\"xero-mcp\"".to_owned(),
        |state| {
            format!(
                "Bearer realm=\"xero-mcp\", resource_metadata=\"{}\", scope=\"xero.read\"",
                state.config.metadata_url()
            )
        },
    );
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error":{"code":"unauthorized","message":"MCP sign-in is required."}})),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&challenge) {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

async fn authenticate(State(guard): State<Guard>, mut request: Request, next: Next) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .filter(|s| s.len() <= 1024);
    let scope = if let Some(client) =
        token.and_then(|token| guard.clients.iter().find(|client| client.matches(token)))
    {
        Some(client.scope.clone())
    } else if let (Some(token), Some(oauth)) = (token, guard.oauth.as_ref()) {
        match oauth.store.access(token) {
            Ok(Some(grant)) => {
                if grant.resource != oauth.config.resource() {
                    return unauthorized(&guard);
                }
                match oauth.grant_is_current(&grant).await {
                    Ok(true) => {}
                    Ok(false) => return unauthorized(&guard),
                    Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
                }
                oauth
                    .config
                    .access_scope(&grant.user_sub, grant.tenant_ids)
                    .ok()
            }
            Ok(None) => None,
            Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        }
    } else {
        None
    };
    let Some(scope) = scope else {
        return unauthorized(&guard);
    };
    let mut rates = guard.rates.lock().await;
    let entry = rates
        .entry(scope.client_id.clone())
        .or_insert((Instant::now(), 0));
    if entry.0.elapsed() >= Duration::from_secs(60) {
        *entry = (Instant::now(), 0);
    }
    if entry.1 >= 120 {
        return (StatusCode::TOO_MANY_REQUESTS,[(header::RETRY_AFTER,"60")],Json(json!({"error":{"code":"rate_limited","message":"MCP request rate exceeded; retry after 60 seconds."}}))).into_response();
    }
    entry.1 += 1;
    drop(rates);
    request.extensions_mut().insert(scope);
    let request_id = uuid::Uuid::new_v4();
    let mut response = next.run(request).await;
    if let Ok(value) = request_id.to_string().parse() {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

pub fn router(
    server: XeroMcp,
    settings: &Settings,
    cancellation: CancellationToken,
) -> anyhow::Result<Router> {
    router_with_oauth(server, settings, cancellation, None)
}

pub fn router_with_oauth(
    server: XeroMcp,
    settings: &Settings,
    cancellation: CancellationToken,
    oauth: Option<OAuthState>,
) -> anyhow::Result<Router> {
    anyhow::ensure!(
        !settings.clients.is_empty() || oauth.is_some(),
        "HTTP MCP requires a configured client token or MCP OAuth"
    );
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(settings.allowed_hosts.clone())
        .with_allowed_origins(settings.allowed_origins.clone())
        .enforce_origin_validation()
        .with_max_request_body_bytes(64 * 1024)
        .with_cancellation_token(cancellation);
    let manifest = server.manifest();
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let guard = Guard {
        clients: settings.clients.clone(),
        oauth: oauth.clone(),
        rates: Arc::new(Mutex::new(HashMap::new())),
    };
    let protected = Router::new()
        .nest_service("/mcp", service)
        .route(
            "/capabilities",
            get(move || {
                let manifest = manifest.clone();
                async move { Json(manifest) }
            }),
        )
        .layer(middleware::from_fn_with_state(guard, authenticate));
    let public = Router::new().merge(protected).route(
        "/healthz",
        get(|| async {
            Json(json!({"status":"ok","service":"xero-mcp","version":env!("CARGO_PKG_VERSION")}))
        }),
    );
    Ok(if let Some(oauth) = oauth {
        public.merge(oauth::router(oauth))
    } else {
        public
    })
}

pub async fn http(server: XeroMcp, settings: Settings) -> anyhow::Result<()> {
    anyhow::ensure!(
        OAuthConfig::from_env()?.is_none(),
        "MCP OAuth requires a shared Xero connection store; use http_with_oauth"
    );
    http_with_oauth(server, settings, None).await
}

pub async fn http_with_oauth(
    server: XeroMcp,
    settings: Settings,
    oauth: Option<OAuthState>,
) -> anyhow::Result<()> {
    let cancellation = CancellationToken::new();
    let router = router_with_oauth(server, &settings, cancellation.clone(), oauth)?;
    let listener = tokio::net::TcpListener::bind(settings.bind).await?;
    tracing::info!(address=%listener.local_addr()?,"Xero MCP Streamable HTTP listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                cancellation.cancel();
            }
        })
        .await?;
    Ok(())
}
