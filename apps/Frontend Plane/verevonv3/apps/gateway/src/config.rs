use std::{env, time::Duration};

use anyhow::Result;
use axum::http::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    HeaderName, HeaderValue, Method as HttpMethod,
};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::audience_tokens::{new_audience_token_cache, AudienceTokenCache};

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) client: reqwest::Client,
    /// Client for SSE/streaming upstreams — NO overall request timeout. The shared
    /// `client` has a 25s ceiling that would sever long-lived event streams (chat,
    /// ag-ui, search answer, crawl events) mid-stream and drop their terminal events.
    pub(crate) streaming_client: reqwest::Client,
    pub(crate) internal_api_key: String,
    /// Ownership PR-4/PR-6 honesty gate. The SAME `CONTROL_PLANE_ENFORCEMENT`
    /// value retrieval-engine reads (off|permissive|strict). The frontend's
    /// privacy affordances (badges/ShareDialog) render ONLY when this is
    /// "strict" AND viewer identity is live — surfaced via GET /api/v1/ownership/status.
    pub(crate) enforcement_mode: String,
    pub(crate) auth_core_url: String,
    pub(crate) verevon_public_origin: String,
    pub(crate) session_core_url: String,
    pub(crate) session_core_service_token: String,
    pub(crate) user_core_service_token: String,
    pub(crate) billing_core_url: String,
    pub(crate) billing_core_service_token: String,
    pub(crate) org_core_url: String,
    pub(crate) org_core_service_token: String,
    pub(crate) integration_core_url: String,
    pub(crate) audit_core_url: String,
    pub(crate) audit_core_service_token: String,
    pub(crate) insight_core_url: String,
    pub(crate) leads_core_url: String,
    pub(crate) shipping_core_url: String,
    pub(crate) user_core_url: String,
    pub(crate) graph_index_url: String,
    pub(crate) quarry_edge_url: String,
    pub(crate) model_recommend_url: String,
    pub(crate) model_gateway_url: String,
    pub(crate) cost_core_url: String,
    pub(crate) model_gateway_dev_bearer: String,
    pub(crate) inference_core_url: String,
    pub(crate) documents_api_url: String,
    pub(crate) retrieval_engine_url: String,
    pub(crate) wiki_store_url: String,
    pub(crate) embedding_engine_url: String,
    pub(crate) quickwit_adapter_url: String,
    pub(crate) finspo_core_url: String,
    pub(crate) imports_api_url: String,
    pub(crate) notification_core_url: String,
    pub(crate) notification_core_service_token: String,
    pub(crate) information_core_url: String,
    pub(crate) conversation_core_url: String,
    pub(crate) conversation_core_service_token: String,
    pub(crate) social_core_url: String,
    pub(crate) searxng_url: String,
    pub(crate) autocomplete_core_url: String,
    pub(crate) autocomplete_token: String,
    pub(crate) zammad_api_url: String,
    pub(crate) zammad_api_token: String,
    pub(crate) audience_token_cache: AudienceTokenCache,
    /// Retained only for legacy browser-domain unit fixtures. Runtime browser
    /// authority and presentation state live in Quarry-v2.
    #[cfg(test)]
    pub(crate) browser_run_store: crate::domains::browser::BrowserRunStore,
    pub(crate) cache: crate::cache::ResultCache,
    /// Fleet-wide inbound rate limiter. Backed by the same Dragonfly connection
    /// as `cache` (distributed token buckets), degrading to an in-process bucket
    /// when the cache is disabled. Wired as an `Extension` layer in `main.rs`.
    pub(crate) rate_limiter: crate::rate_limit::RateLimiter,
    pub(crate) studio_store: crate::domains::studio::StudioStore,
    pub(crate) allow_dev_actor_headers: bool,
    pub(crate) allow_dev_auth_bypass: bool,
}

pub(crate) async fn build_state() -> Result<AppState> {
    let cache_url = env::var("GATEWAY_CACHE_REDIS_URL").ok();
    let cache = crate::cache::ResultCache::connect(cache_url.as_deref()).await;
    let internal_api_key = internal_api_key()?;
    let session_core_service_token = required_service_token("SESSION_CORE_SERVICE_TOKEN")?;
    let user_core_service_token = required_service_token("USER_CORE_SERVICE_TOKEN")?;
    let billing_core_service_token = required_service_token("BILLING_CORE_SERVICE_TOKEN")?;
    let org_core_service_token = required_service_token("ORG_CORE_SERVICE_TOKEN")?;
    let audit_core_service_token = required_service_token("AUDIT_CORE_SERVICE_TOKEN")?;
    validate_service_token_distinctness(
        &[
            ("SESSION_CORE_SERVICE_TOKEN", &session_core_service_token),
            ("USER_CORE_SERVICE_TOKEN", &user_core_service_token),
            ("BILLING_CORE_SERVICE_TOKEN", &billing_core_service_token),
            ("ORG_CORE_SERVICE_TOKEN", &org_core_service_token),
            ("AUDIT_CORE_SERVICE_TOKEN", &audit_core_service_token),
        ],
        &[("INTERNAL_API_KEY", &internal_api_key)],
    )?;
    Ok(AppState {
        client: standard_http_client()?,
        // Streaming client: same connection hygiene but NO overall `.timeout()`, so
        // long-lived SSE streams are not severed at 25s. Connect/idle/dead-socket
        // protection remains; stream lifetime is bounded by the client disconnecting.
        streaming_client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .pool_idle_timeout(Duration::from_secs(20))
            .tcp_keepalive(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        internal_api_key,
        // Same env var retrieval-engine reads; normalize + whitelist so an
        // unknown/unset value can never accidentally read as "strict".
        enforcement_mode: env::var("CONTROL_PLANE_ENFORCEMENT")
            .ok()
            .map(|v| v.trim().to_lowercase())
            .filter(|v| matches!(v.as_str(), "off" | "permissive" | "strict"))
            .unwrap_or_else(|| "off".to_string()),
        auth_core_url: env_url("AUTH_CORE_URL", "http://auth-core:3011"),
        verevon_public_origin: canonical_public_origin(
            &env::var("VEREVON_PUBLIC_ORIGIN")
                .unwrap_or_else(|_| "http://localhost:5173".to_owned()),
            !dev_flags_allowed(&env::var("APP_ENV").unwrap_or_default()),
        )?,
        session_core_url: env_url("SESSION_CORE_URL", "http://session-core:3017"),
        session_core_service_token,
        user_core_service_token,
        billing_core_url: env_url("BILLING_CORE_URL", "http://billing-core:3014"),
        billing_core_service_token,
        org_core_url: env_url("ORG_CORE_URL", "http://org-core:8080"),
        org_core_service_token,
        integration_core_url: env_url("INTEGRATION_CORE_URL", "http://integration-api:3026"),
        // audit-core (Control Plane) serves the audit read API the Trust Center
        // aggregates over. Its credential is audience-bound and cannot be reused
        // for Org or Billing authority.
        audit_core_url: env_url("AUDIT_CORE_URL", "http://audit-core:8187"),
        audit_core_service_token,
        // insight-core (Application Plane, registry-only) serves the connector
        // registry. Internal-key auth + x-org-id header, like the other cores.
        insight_core_url: env_url("INSIGHT_CORE_URL", "http://insight-core:3163"),
        leads_core_url: env_url("LEADS_CORE_URL", "http://leads-core:3164"),
        // shipping-core (Ingestion Plane) — freight aggregator; on the
        // inter-plane bus, so the in-network name resolves from the gateway.
        shipping_core_url: env_url("SHIPPING_CORE_URL", "http://shipping-core:8080"),
        user_core_url: env_url("USER_CORE_URL", "http://user-core:3012"),
        graph_index_url: env_url("GRAPH_INDEX_URL", "http://dpv2-graph-index:9203"),
        quarry_edge_url: env_url("QUARRY_EDGE_URL", "http://quarry-edge:8082"),
        model_recommend_url: env_url(
            "MODEL_PLANE_RECOMMEND_URL",
            "http://model-gateway:8080/v1/recommend/plan",
        ),
        model_gateway_url: env_url("MODEL_GATEWAY_URL", "http://model-gateway:8080"),
        // Phase 7 B5 — Model Plane cost ledger (cost-core HTTP API). Reachable
        // over the inter-plane-bus alias; backs the cost/usage dashboard.
        cost_core_url: env_url("COST_CORE_URL", "http://model-plane-cost-core-1:8089"),
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
        // Data Plane v2 diagnostics stay behind service contracts. Storage
        // backends (Qdrant, Quickwit, MinIO) remain private to dpv2-net.
        embedding_engine_url: env_url("EMBEDDING_ENGINE_URL", "http://dpv2-embedding-engine:9202"),
        quickwit_adapter_url: env_url("QUICKWIT_ADAPTER_URL", "http://dpv2-quickwit-adapter:9204"),
        // finspo-core (SharePoint/OneDrive storage analytics + duplicate management).
        finspo_core_url: env_url("FINSPO_CORE_URL", "http://finspo-api:3130"),
        imports_api_url: env_url("IMPORTS_API_URL", "http://imports-core:3025"),
        notification_core_url: env_url("NOTIFICATION_CORE_URL", "http://notification-core:3140"),
        notification_core_service_token: required_service_token("NOTIFICATION_CORE_SERVICE_TOKEN")?,
        information_core_url: env_url("INFORMATION_CORE_URL", "http://information-core:3190"),
        // Inbox / support conversations (Application Plane conversation-core-go).
        conversation_core_url: env_url("CONVERSATION_CORE_URL", "http://conversation-core-go:3160"),
        conversation_core_service_token: required_service_token("CONVERSATION_CORE_SERVICE_TOKEN")?,
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
        #[cfg(test)]
        browser_run_store: crate::domains::browser::new_browser_run_store(),
        // Reuse the cache's Dragonfly connection for fleet-wide rate limiting.
        // Built before `cache` is moved into the struct below (literal fields
        // evaluate top-to-bottom, so this borrow happens first).
        rate_limiter: crate::rate_limit::RateLimiter::from_cache(&cache),
        cache,
        studio_store: crate::domains::studio::StudioStore::new(),
        allow_dev_actor_headers: dev_only_flag("ALLOW_DEV_ACTOR_HEADERS"),
        allow_dev_auth_bypass: dev_only_flag("ALLOW_DEV_AUTH_BYPASS"),
    })
}

fn standard_http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        // Generous overall ceiling so browser-render scrapes (quarry /v1/scrape)
        // don't get cut off; most internal calls return in well under a second.
        .timeout(Duration::from_secs(25))
        // Never carry service credentials or signed delegation headers across
        // an upstream redirect, even when a compromised service returns 3xx.
        .redirect(reqwest::redirect::Policy::none())
        // Drop idle keep-alive connections before the upstream/bridge reaps them.
        .pool_idle_timeout(Duration::from_secs(20))
        .tcp_keepalive(Duration::from_secs(20))
        .build()?)
}

pub(crate) fn build_cors_layer() -> CorsLayer {
    let origins = env::var("VEREVON_ALLOWED_ORIGINS")
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
            HeaderName::from_static("x-verevon-org-id"),
        ])
        .allow_credentials(true)
}

fn internal_api_key() -> Result<String> {
    match env::var("INTERNAL_API_KEY") {
        Ok(value) => {
            let trimmed = value.trim();
            // Reject the ENTIRE `change-me*` placeholder family (case-insensitive),
            // not just the exact string "change-me". A value like
            // `change-me-internal-service-secret` authenticates to no core, so a
            // gateway that boots with it silently 401s every internal-key call
            // (Brreg, /me/session-context, audit, …). Treat any change-me* / empty
            // value as "not configured".
            let is_placeholder =
                trimmed.is_empty() || trimmed.to_ascii_lowercase().starts_with("change-me");
            if !is_placeholder {
                return Ok(trimmed.to_owned());
            }
            // A placeholder was EXPLICITLY set — this is the silent-401 trap.
            // Fail loudly unless the operator explicitly opts into insecure dev
            // defaults, so a misconfigured key surfaces at boot, not as a fleet of
            // mysterious 401s once requests start flowing.
            if !trimmed.is_empty() && env_bool("ALLOW_INSECURE_DEV_DEFAULTS", false) {
                eprintln!(
                    "WARN: INTERNAL_API_KEY is a `change-me*` placeholder; internal-key \
                     calls to cores WILL 401 (running anyway: ALLOW_INSECURE_DEV_DEFAULTS=1)"
                );
                return Ok(trimmed.to_owned());
            }
            anyhow::bail!(
                "INTERNAL_API_KEY is empty or a `change-me*` placeholder; set the real \
                 shared internal key (or ALLOW_INSECURE_DEV_DEFAULTS=1 to override)"
            );
        }
        Err(_) => {
            // No INTERNAL_API_KEY at all: keep prior dev ergonomics — a debug build
            // (or explicit opt-in) boots with the canonical fake key.
            if cfg!(debug_assertions) || env_bool("ALLOW_INSECURE_DEV_DEFAULTS", false) {
                return Ok("change-me".into());
            }
            anyhow::bail!("INTERNAL_API_KEY must be set and cannot be a change-me* placeholder");
        }
    }
}

fn required_service_token(name: &str) -> Result<String> {
    let value = env::var(name).unwrap_or_default();
    let trimmed = value.trim();
    if !service_token_is_secure(trimmed) {
        anyhow::bail!("{name} must be a non-placeholder secret of at least 32 characters");
    }
    Ok(trimmed.to_owned())
}

fn service_token_is_secure(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    value.len() >= 32
        && !lower.starts_with("test")
        && !lower.starts_with("placeholder")
        && !lower.starts_with("change-me")
        && !lower.starts_with("replace-with")
}

fn validate_service_token_distinctness(
    scoped: &[(&str, &str)],
    forbidden: &[(&str, &str)],
) -> Result<()> {
    for (index, (name, value)) in scoped.iter().enumerate() {
        if let Some((reused_name, _)) = forbidden
            .iter()
            .chain(scoped[..index].iter())
            .find(|(_, candidate)| candidate.trim() == value.trim())
        {
            anyhow::bail!("{name} must not reuse {reused_name}");
        }
    }
    Ok(())
}

/// Dev auth escape hatches must never activate in a production deploy, even if
/// the env var leaks into the profile: `APP_ENV=production` (or `prod`) wins
/// over the flag.
fn dev_only_flag(key: &str) -> bool {
    let enabled = env_bool(key, false);
    let app_env = env::var("APP_ENV").unwrap_or_default();
    if enabled && !dev_flags_allowed(&app_env) {
        eprintln!("refusing {key}=true because APP_ENV={app_env}; dev bypass is disabled");
        return false;
    }
    enabled
}

fn dev_flags_allowed(app_env: &str) -> bool {
    !matches!(app_env.to_ascii_lowercase().as_str(), "production" | "prod")
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

fn canonical_public_origin(value: &str, production_like: bool) -> Result<String> {
    let parsed = url::Url::parse(value.trim())?;
    let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    let allowed_scheme =
        parsed.scheme() == "https" || (!production_like && parsed.scheme() == "http" && loopback);
    if !allowed_scheme
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        anyhow::bail!("VEREVON_PUBLIC_ORIGIN must be a canonical HTTPS origin");
    }
    Ok(parsed.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_public_origin, dev_flags_allowed, service_token_is_secure, standard_http_client,
        validate_service_token_distinctness,
    };

    #[tokio::test]
    async fn standard_upstream_client_never_follows_redirects() {
        use axum::{response::Redirect, routing::get, Router};

        let app = Router::new()
            .route(
                "/redirect",
                get(|| async { Redirect::temporary("/target") }),
            )
            .route("/target", get(|| async { "scoped token must not arrive" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind redirect fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let response = standard_http_client()
            .expect("client")
            .get(format!("http://{address}/redirect"))
            .header("x-service-token", "scoped-token")
            .send()
            .await
            .expect("redirect response");
        server.abort();
        assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
    }

    #[test]
    fn public_origin_requires_https_in_production_and_loopback_http_in_development() {
        assert_eq!(
            canonical_public_origin("https://verevon.example/", true).unwrap(),
            "https://verevon.example"
        );
        assert_eq!(
            canonical_public_origin("http://localhost:5173", false).unwrap(),
            "http://localhost:5173"
        );
        for value in [
            "http://verevon.example",
            "https://user:pass@verevon.example",
            "https://verevon.example/path",
            "https://verevon.example?tenant=acme",
            "https://verevon.example/#fragment",
        ] {
            assert!(canonical_public_origin(value, true).is_err(), "{value}");
        }
        assert!(canonical_public_origin("http://localhost:5173", true).is_err());
        assert!(canonical_public_origin("http://verevon.example", false).is_err());
    }

    #[test]
    fn service_tokens_reject_public_placeholder_families() {
        assert!(!service_token_is_secure(
            "test-generated-dedicated-random-32-byte-minimum-key"
        ));
        assert!(!service_token_is_secure(
            "placeholder-dedicated-random-32-byte-minimum-key"
        ));
        assert!(!service_token_is_secure(
            "replace-with-dedicated-random-32-byte-minimum-key"
        ));
        assert!(!service_token_is_secure(
            "change-me-notification-gateway-secret-32-bytes"
        ));
        assert!(service_token_is_secure(
            "generated-secret-value-with-at-least-32-bytes"
        ));
    }

    #[test]
    fn service_tokens_must_be_distinct_from_each_other_and_legacy_keys() {
        assert!(validate_service_token_distinctness(
            &[
                ("ORG_CORE_SERVICE_TOKEN", "org-token"),
                ("BILLING_CORE_SERVICE_TOKEN", "billing-token"),
            ],
            &[("INTERNAL_API_KEY", "legacy-key")],
        )
        .is_ok());
        assert!(validate_service_token_distinctness(
            &[
                ("ORG_CORE_SERVICE_TOKEN", "same-token"),
                ("BILLING_CORE_SERVICE_TOKEN", "same-token"),
            ],
            &[],
        )
        .is_err());
        assert!(validate_service_token_distinctness(
            &[("ORG_CORE_SERVICE_TOKEN", "legacy-key")],
            &[("INTERNAL_API_KEY", "legacy-key")],
        )
        .is_err());
    }

    #[test]
    fn dev_flags_refused_in_production_profiles() {
        assert!(!dev_flags_allowed("production"));
        assert!(!dev_flags_allowed("Production"));
        assert!(!dev_flags_allowed("prod"));
    }

    #[test]
    fn dev_flags_allowed_outside_production() {
        assert!(dev_flags_allowed(""));
        assert!(dev_flags_allowed("development"));
        assert!(dev_flags_allowed("staging"));
    }
}
