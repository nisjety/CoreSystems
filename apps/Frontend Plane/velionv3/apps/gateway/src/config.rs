use std::{env, time::Duration};

use anyhow::Result;
use axum::http::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    HeaderName, HeaderValue, Method as HttpMethod,
};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::audience_tokens::{new_audience_token_cache, AudienceTokenCache};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) client: reqwest::Client,
    /// Client for SSE/streaming upstreams — NO overall request timeout. The shared
    /// `client` has a 25s ceiling that would sever long-lived event streams (chat,
    /// ag-ui, search answer, crawl events) mid-stream and drop their terminal events.
    pub(crate) streaming_client: reqwest::Client,
    pub(crate) internal_api_key: String,
    pub(crate) auth_core_url: String,
    pub(crate) session_core_url: String,
    pub(crate) billing_core_url: String,
    pub(crate) org_core_url: String,
    pub(crate) integration_core_url: String,
    pub(crate) audit_core_url: String,
    pub(crate) insight_core_url: String,
    pub(crate) user_core_url: String,
    pub(crate) graph_index_url: String,
    pub(crate) quarry_edge_url: String,
    pub(crate) quarry_control_url: String,
    pub(crate) model_recommend_url: String,
    pub(crate) model_gateway_url: String,
    pub(crate) model_gateway_dev_bearer: String,
    pub(crate) inference_core_url: String,
    pub(crate) documents_api_url: String,
    pub(crate) retrieval_engine_url: String,
    pub(crate) wiki_store_url: String,
    pub(crate) embedding_engine_url: String,
    pub(crate) quickwit_adapter_url: String,
    pub(crate) qdrant_url: String,
    pub(crate) quickwit_url: String,
    pub(crate) finspo_core_url: String,
    pub(crate) imports_api_url: String,
    pub(crate) notification_core_url: String,
    pub(crate) information_core_url: String,
    pub(crate) conversation_core_url: String,
    pub(crate) social_core_url: String,
    pub(crate) searxng_url: String,
    pub(crate) autocomplete_core_url: String,
    pub(crate) autocomplete_token: String,
    pub(crate) zammad_api_url: String,
    pub(crate) zammad_api_token: String,
    pub(crate) audience_token_cache: AudienceTokenCache,
    pub(crate) browser_run_store: crate::domains::browser::BrowserRunStore,
    pub(crate) cache: crate::cache::ResultCache,
    pub(crate) chat_history_store: crate::domains::chat::history::ChatHistoryStore,
    pub(crate) studio_store: crate::domains::studio::StudioStore,
    pub(crate) allow_dev_actor_headers: bool,
    pub(crate) allow_dev_auth_bypass: bool,
    /// Stealth/proxy escalation for bot-walled sites (see domains/knowledge/enhanced_fetch).
    /// Empty provider/key ⇒ disabled (callers return a clear "site blocked" error).
    /// `scrapfly` (free tier) or `brightdata` (premium Web Unlocker).
    pub(crate) enhanced_scrape_provider: String,
    pub(crate) enhanced_scrape_api_key: String,
    /// Bright Data Web Unlocker zone name (only used by the `brightdata` provider).
    pub(crate) enhanced_scrape_zone: String,
    /// Optional ISO country for proxy egress (e.g. `no`); blank lets the provider choose.
    pub(crate) enhanced_scrape_country: String,
}

pub(crate) async fn build_state() -> Result<AppState> {
    let cache_url = env::var("GATEWAY_CACHE_REDIS_URL").ok();
    let cache = crate::cache::ResultCache::connect(cache_url.as_deref()).await;
    Ok(AppState {
        client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            // Generous overall ceiling so browser-render scrapes (quarry /v1/scrape)
            // don't get cut off; most internal calls return in well under a second.
            .timeout(Duration::from_secs(25))
            // Drop idle keep-alive connections well before the upstream (or the
            // Docker bridge) reaps them, so we never send on a dead socket — the
            // root cause of intermittent "error sending request" 502s.
            .pool_idle_timeout(Duration::from_secs(20))
            .tcp_keepalive(Duration::from_secs(20))
            .build()?,
        // Streaming client: same connection hygiene but NO overall `.timeout()`, so
        // long-lived SSE streams are not severed at 25s. Connect/idle/dead-socket
        // protection remains; stream lifetime is bounded by the client disconnecting.
        streaming_client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .pool_idle_timeout(Duration::from_secs(20))
            .tcp_keepalive(Duration::from_secs(20))
            .build()?,
        internal_api_key: internal_api_key()?,
        auth_core_url: env_url("AUTH_CORE_URL", "http://auth-core:3011"),
        session_core_url: env_url("SESSION_CORE_URL", "http://session-core:3013"),
        billing_core_url: env_url("BILLING_CORE_URL", "http://billing-core:3017"),
        org_core_url: env_url("ORG_CORE_URL", "http://org-core:8080"),
        integration_core_url: env_url("INTEGRATION_CORE_URL", "http://integration-api:3026"),
        // audit-core (Control Plane) serves the audit read API the Trust Center
        // aggregates over. Internal-key auth (X-Internal-Api-Key) like the other cores.
        audit_core_url: env_url("AUDIT_CORE_URL", "http://audit-core:8187"),
        // insight-core (Application Plane, registry-only) serves the connector
        // registry. Internal-key auth + x-org-id header, like the other cores.
        insight_core_url: env_url("INSIGHT_CORE_URL", "http://insight-core:3163"),
        user_core_url: env_url("USER_CORE_URL", "http://user-core:3012"),
        graph_index_url: env_url("GRAPH_INDEX_URL", "http://dpv2-graph-index:9203"),
        quarry_edge_url: env_url("QUARRY_EDGE_URL", "http://quarry-edge:8082"),
        quarry_control_url: env_url("QUARRY_CONTROL_URL", "http://quarry-control:8081"),
        model_recommend_url: env_url(
            "MODEL_PLANE_RECOMMEND_URL",
            "http://model-gateway:8080/v1/recommend/plan",
        ),
        model_gateway_url: env_url("MODEL_GATEWAY_URL", "http://model-gateway:8080"),
        model_gateway_dev_bearer: env::var("MODEL_GATEWAY_DEV_BEARER")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        // inference-core's internal HTTP (health server, :8082) serves the
        // router-policy GET/PUT used by the admin UI. Must be reachable from the
        // gateway over the inter-plane-bus (like model-gateway).
        inference_core_url: env_url("INFERENCE_CORE_URL", "http://inference-core:8082"),
        documents_api_url: env_url("DOCUMENTS_API_URL", "http://dpv2-documents-api:8010"),
        // retrieval-engine listens on container port 8004 (compose maps host 8014:8004);
        // gateway↔engine traffic rides the container port over inter-plane-bus.
        retrieval_engine_url: env_url("RETRIEVAL_ENGINE_URL", "http://dpv2-retrieval-engine:8004"),
        wiki_store_url: env_url("WIKI_STORE_URL", "http://dpv2-wiki-store:8011"),
        // Data Plane v2 diagnostics + analytics upstreams. embedding-engine, quickwit-adapter
        // and qdrant ride inter-plane-bus; minio is dpv2-net-only so storage health is derived
        // indirectly from quickwit index URIs rather than a direct call.
        embedding_engine_url: env_url("EMBEDDING_ENGINE_URL", "http://dpv2-embedding-engine:9202"),
        quickwit_adapter_url: env_url("QUICKWIT_ADAPTER_URL", "http://dpv2-quickwit-adapter:9204"),
        qdrant_url: env_url("QDRANT_URL", "http://dpv2-qdrant:6333"),
        quickwit_url: env_url("QUICKWIT_URL", "http://dpv2-quickwit:7280"),
        // finspo-core (SharePoint/OneDrive storage analytics + duplicate management).
        finspo_core_url: env_url("FINSPO_CORE_URL", "http://finspo-api:3130"),
        imports_api_url: env_url("IMPORTS_API_URL", "http://imports-core:3025"),
        notification_core_url: env_url("NOTIFICATION_CORE_URL", "http://notification-core:3140"),
        information_core_url: env_url("INFORMATION_CORE_URL", "http://information-core:3190"),
        // Inbox / support conversations (Application Plane conversation-core-go).
        conversation_core_url: env_url("CONVERSATION_CORE_URL", "http://conversation-core-go:3160"),
        // Social publishing and calendar persistence (Application Plane social-core).
        social_core_url: env_url("SOCIAL_CORE_URL", "http://social-core:3162"),
        // Search verticals: SearXNG powers the videos category; quarry-edge (above)
        // owns web/images/answer; autocomplete-core backs typeahead suggestions.
        searxng_url: env_url("SEARXNG_URL", "http://searxng:8080"),
        autocomplete_core_url: env_url("AUTOCOMPLETE_CORE_URL", "http://autocomplete-core:3219"),
        autocomplete_token: env::var("AUTOCOMPLETE_INTERNAL_TOKEN")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        zammad_api_url: env_url("ZAMMAD_API_URL", "http://zammad-railsserver:3000"),
        zammad_api_token: env::var("ZAMMAD_API_TOKEN")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        audience_token_cache: new_audience_token_cache(),
        browser_run_store: crate::domains::browser::new_browser_run_store(),
        cache,
        chat_history_store: crate::domains::chat::history::ChatHistoryStore::new(),
        studio_store: crate::domains::studio::StudioStore::new(),
        allow_dev_actor_headers: env_bool("ALLOW_DEV_ACTOR_HEADERS", false),
        allow_dev_auth_bypass: env_bool("ALLOW_DEV_AUTH_BYPASS", false),
        enhanced_scrape_provider: env::var("SCRAPE_ENHANCED_PROVIDER")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase(),
        enhanced_scrape_api_key: env::var("SCRAPE_ENHANCED_API_KEY")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        enhanced_scrape_zone: env::var("SCRAPE_ENHANCED_ZONE")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        enhanced_scrape_country: env::var("SCRAPE_ENHANCED_COUNTRY")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase(),
    })
}

pub(crate) fn build_cors_layer() -> CorsLayer {
    let origins = env::var("VELION_ALLOWED_ORIGINS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|origin| HeaderValue::from_str(origin.trim()).ok())
                .collect::<Vec<_>>()
        })
        .filter(|origins| !origins.is_empty())
        .unwrap_or_else(|| {
            vec![
                HeaderValue::from_static("http://localhost:5173"),
                HeaderValue::from_static("http://127.0.0.1:5173"),
            ]
        });

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            HttpMethod::GET,
            HttpMethod::POST,
            HttpMethod::PUT,
            HttpMethod::PATCH,
            HttpMethod::DELETE,
            HttpMethod::OPTIONS,
        ])
        .allow_headers([
            ACCEPT,
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("x-velion-org-id"),
        ])
        .allow_credentials(true)
}

fn internal_api_key() -> Result<String> {
    if let Ok(value) = env::var("INTERNAL_API_KEY") {
        let trimmed = value.trim();
        if !trimmed.is_empty() && trimmed != "change-me" {
            return Ok(trimmed.to_owned());
        }
    }

    if cfg!(debug_assertions) || env_bool("ALLOW_INSECURE_DEV_DEFAULTS", false) {
        return Ok("change-me".into());
    }

    anyhow::bail!("INTERNAL_API_KEY must be set and cannot be change-me");
}

fn env_bool(key: &str, fallback: bool) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(fallback)
}

fn env_url(key: &str, fallback: &str) -> String {
    env::var(key)
        .unwrap_or_else(|_| fallback.into())
        .trim_end_matches('/')
        .to_owned()
}
