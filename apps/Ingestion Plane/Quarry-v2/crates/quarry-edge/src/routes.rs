use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use url::Url;

use quarry_core::cache::{CacheMode, CachePolicy};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::{RequestKind, RunKind};
use quarry_core::output::NormalizedOutput;
use quarry_core::privacy::PrivacyPolicy;
use quarry_core::zdr::ZdrMode;
use quarry_runtime::driver::RenderHints;
use quarry_runtime::driver_plan::{plan_from_signals, DriverSignals};
use quarry_runtime::pipeline::PageRunner;

use crate::cache::DEFAULT_TTL_SECS;
use crate::state::AppState;
use axum::response::sse::Event;
use axum::response::Sse;

/// Wave 7 — driver picker. When the request opts in and the edge has
/// HTTP/3 wired, wrap the normal plan in a transport fallback: try QUIC
/// first, then use the planned driver if the QUIC path is unavailable.
/// Browser plans always go through the registry — h3 has no rendering
/// primitive and there's no point routing a browser fetch through QUIC
/// for the navigation HTML.
fn pick_driver(
    state: &AppState,
    plan: &quarry_runtime::DriverPlan,
    prefer_http3: bool,
) -> Arc<dyn quarry_runtime::Driver> {
    let planned = state.drivers.build_driver(plan);
    if prefer_http3 && plan.driver != quarry_core::output::DriverKind::Browser {
        if let Some(h3) = state.http3.as_ref() {
            return Arc::new(quarry_runtime::TransportFallbackDriver::prefer_http3(
                h3.clone(),
                planned,
            ));
        }
        tracing::debug!("prefer_http3 requested but no HTTP/3 driver wired; using default plan");
    }
    planned
}

fn should_read(p: &Option<CachePolicy>) -> bool {
    match p {
        None => true,
        Some(p) => matches!(p.mode, CacheMode::ReadOnly | CacheMode::ReadWrite),
    }
}

fn should_write(p: &Option<CachePolicy>) -> bool {
    match p {
        None => true,
        Some(p) => matches!(p.mode, CacheMode::WriteOnly | CacheMode::ReadWrite),
    }
}

fn effective_ttl(p: &Option<CachePolicy>, default: u64) -> u64 {
    match p {
        Some(p) if p.max_age_s > 0 => p.max_age_s as u64,
        _ => default,
    }
}

pub fn router(state: AppState) -> Router {
    #[allow(deprecated)]
    let timeout = TimeoutLayer::new(Duration::from_secs(60));

    // Public surface — health/ready are intentionally unauthenticated so
    // load balancers and orchestrator probes can verify liveness without
    // bearer tokens. Everything else lives behind the auth middleware.
    //
    // Cycle 31 / cluster #11 — GraphQL introspection + playground are
    // public-readable. Schema introspection is needed by SDK generators
    // in CI; the playground is a dev convenience. Both can be disabled
    // in production via reverse-proxy ACL if the schema is sensitive.
    // `POST /graphql` itself is auth-gated below (protected layer).
    let public = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/graphql/schema", get(crate::graphql::introspection))
        .route("/graphql/playground", get(crate::graphql::playground));

    // Internal service surface — authenticated by a shared service token
    // (QUARRY_EDGE_INTERNAL_TOKEN), NOT a per-tenant JWT, so it lives
    // OUTSIDE require_auth. The W2 change-monitor activity (quarry-orchestrator,
    // Go) persists baselines/diffs here on behalf of MANY tenants; org_id
    // travels in the request body (verified at schedule-creation) rather than
    // a JWT claim. The handler enforces the token itself.
    let internal = Router::new()
        .route(
            "/v1/internal/change/record",
            post(crate::change_routes::record_internal),
        )
        // Phase-2 visual RAG — page-image serve for the embedding-engine consumer
        // (no JWT; the content-hash in the path is the capability on the trusted bus).
        .route(
            "/v1/internal/page-images/:org/:doc/:page/:hash",
            get(crate::resource_routes::get_page_image),
        )
        // Orchestrator page-execution activity. Authenticated by the shared
        // runtime service token (QUARRY_EDGE__RUNTIME_AUTH_TOKEN), NOT a
        // per-tenant JWT; the originating org travels in the request body
        // (stamped by the orchestrator from the schedule memo, verified at
        // schedule-creation). The handler enforces the token itself.
        .route("/v1/internal/run_page", post(internal_run_page));

    // Protected surface — every /v1/* route. The auth middleware
    // verifies an `Authorization: Bearer <jwt>` against the Control
    // Plane's `auth-core` JWKS (or dev-bypass), then inserts `Claims`
    // into request extensions. Handlers recover claims via
    // `Extension<Claims>` and use the verified `org_id` to scope all
    // data access — client-supplied `org_id` fields are overwritten.
    let protected = Router::new()
        .route("/v1/scrape", post(scrape))
        .route("/v1/scrape/stream", post(scrape_stream))
        .route("/v1/crawl", post(crawl_handoff))
        .route("/v1/batch", post(batch_handoff))
        .route("/v1/profiles", post(crate::profile_routes::save_profile))
        .route("/v1/profiles", get(crate::profile_routes::list_profiles))
        .route("/v1/profiles/:id", get(crate::profile_routes::load_profile))
        .route(
            "/v1/profiles/:id",
            delete(crate::profile_routes::delete_profile),
        )
        // Cycle 20 / cluster #13 — restore-probe validation.
        .route(
            "/v1/profiles/:id/restore_probe",
            post(crate::profile_routes::restore_probe),
        )
        .route("/v1/audio", post(crate::audio_routes::audio))
        .route("/v1/search", post(crate::search_routes::search))
        .route("/v1/search/images", post(crate::search_routes::images))
        .route("/v1/search/similar", post(crate::search_routes::similar))
        .route("/v1/search/suggest", post(crate::search_routes::suggest))
        .route("/v1/map", post(crate::map_routes::map))
        .route("/v1/extract", post(crate::extract_routes::extract))
        .route("/v1/answer", post(crate::answer_routes::answer))
        .route(
            "/v1/answer/stream",
            post(crate::answer_routes::answer_stream),
        )
        // Cycle 22 / cluster #4 part 1 — resource list endpoints.
        // /v1/artifacts is served locally; the rest forward to control plane.
        .route("/v1/artifacts", get(crate::resource_routes::list_artifacts))
        .route(
            "/v1/artifacts/:id",
            get(crate::resource_routes::get_artifact),
        )
        .route(
            "/v1/sources",
            get(crate::resource_routes::list_sources).post(crate::resource_routes::create_source),
        )
        .route(
            "/v1/sources/:id",
            delete(crate::resource_routes::delete_source),
        )
        .route("/v1/snapshots", get(crate::resource_routes::list_snapshots))
        .route("/v1/:kind/jobs", get(crate::resource_routes::list_jobs))
        // Cycle 23 / cluster #4 part 2 — request-queues, benchmarks, team/*
        .route(
            "/v1/request-queues",
            get(crate::resource_routes::list_request_queues),
        )
        .route(
            "/v1/benchmarks",
            get(crate::resource_routes::list_benchmarks),
        )
        .route(
            "/v1/team/credit-usage",
            get(crate::resource_routes::team_credit_usage),
        )
        .route(
            "/v1/team/token-usage",
            get(crate::resource_routes::team_token_usage),
        )
        .route(
            "/v1/team/concurrency",
            get(crate::resource_routes::team_concurrency),
        )
        .route(
            "/v1/team/queue-status",
            get(crate::resource_routes::team_queue_status),
        )
        .route(
            "/v1/team/activity",
            get(crate::resource_routes::team_activity),
        )
        // C30.1 / cluster #7 — durable job-history read.
        .route(
            "/v1/runs/:id/events",
            get(crate::resource_routes::list_run_events),
        )
        // C30.2 / cluster #9 — versioned change tracking.
        .route("/v1/change/check", post(crate::change_routes::check))
        .route("/v1/change/latest", get(crate::change_routes::latest))
        .route("/v1/change/history", get(crate::change_routes::history))
        // Cycle 31 / cluster #11 — GraphQL query endpoint (auth-gated).
        .route("/graphql", post(crate::graphql::graphql_handler))
        // Cycle 23 / cluster #5 — schedules list + lifecycle.
        .route(
            "/v1/schedules",
            get(crate::resource_routes::list_schedules)
                .post(crate::schedule_routes::create_schedule),
        )
        .route(
            "/v1/schedules/:id",
            delete(crate::schedule_routes::delete_schedule),
        )
        .route(
            "/v1/schedules/:id/pause",
            post(crate::schedule_routes::pause_schedule),
        )
        .route(
            "/v1/schedules/:id/unpause",
            post(crate::schedule_routes::unpause_schedule),
        )
        .route(
            "/v1/schedules/:id/trigger",
            post(crate::schedule_routes::trigger_schedule),
        )
        .route(
            "/v1/schedules/:id/backfill",
            post(crate::schedule_routes::backfill_schedule),
        )
        // P7 — agentic browser loop (real chromiumoxide). No-op router when
        // the `browser-agent` feature is disabled. Inherits `require_auth`.
        .merge(crate::agent_routes::agent_router())
        .layer(middleware::from_fn(crate::auth::require_auth));

    // Cap request bodies at 1 MB. Largest legit payload is /v1/batch
    // with a few hundred URLs (~30 KB). Without this an attacker can
    // stream an arbitrarily large JSON body to /v1/answer or
    // /v1/scrape and pin a request thread in serde decoding. axum's
    // default body-limit middleware is permissive; this layer is
    // explicit and tunable.
    let body_limit = RequestBodyLimitLayer::new(1024 * 1024);

    Router::new()
        .merge(public)
        .merge(internal)
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .layer(body_limit)
        .layer(timeout)
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
async fn ready() -> &'static str {
    "ready"
}

#[derive(Debug, Deserialize)]
pub struct ScrapeRequest {
    pub url: String,
    #[serde(default)]
    pub prev_fingerprint: Option<String>,
    #[serde(default)]
    pub cache: Option<CachePolicy>,
    #[serde(default)]
    pub zdr: Option<bool>,
    #[serde(default)]
    pub privacy: Option<PrivacyPolicy>,
    #[serde(default)]
    pub signals: Option<DriverSignals>,
    #[serde(default)]
    pub ingest: Option<bool>,
    #[serde(default)]
    pub org_id: Option<String>,
    /// Render hints. Browser-driver only — static / TLS-profile fetches
    /// ignore these. Omitted = no render-time waits.
    #[serde(default)]
    pub render: Option<RenderHintsRequest>,
    /// Wave 7 — prefer HTTP/3 (QUIC). Honoured only when the edge
    /// was built with `--features http3` AND `QUARRY_EDGE__HTTP3_ENABLED`
    /// is set; otherwise the request silently runs over HTTP/1.1+2.
    /// If QUIC is unavailable at runtime (for example Docker Desktop on
    /// macOS blocking outbound UDP), the planned driver is retried.
    /// Trades JA3/JA4 TLS fingerprinting for 1-RTT QUIC handshakes on
    /// CDN-fronted origins.
    #[serde(default)]
    pub prefer_http3: bool,
}

/// Wire-level shape for [`RenderHints`]. Kept separate from the runtime
/// struct so the API surface (`waitForSelector` / `waitForTimeoutMs`) can
/// evolve independently of the internal representation.
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RenderHintsRequest {
    #[serde(default)]
    pub wait_for_selector: Option<String>,
    #[serde(default)]
    pub wait_for_timeout_ms: Option<u32>,
}

impl From<RenderHintsRequest> for RenderHints {
    fn from(r: RenderHintsRequest) -> Self {
        RenderHints {
            wait_for_selector: r.wait_for_selector,
            wait_for_timeout_ms: r.wait_for_timeout_ms,
        }
    }
}

async fn scrape(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<ScrapeRequest>,
) -> Result<Json<Envelope<NormalizedOutput>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let zdr = ZdrMode::from(req.zdr.unwrap_or(false));
    let privacy = effective_privacy(req.privacy.clone(), zdr);
    // Tenant isolation: ignore any client-supplied org_id and use the
    // verified JWT claim. The `ScrapeRequest.org_id` field is preserved
    // on the wire for backwards-compat but its value is never trusted.
    let _ = &req.org_id;
    let org_id = claims.org_id.clone();

    let url: Url = req.url.parse().map_err(|e| {
        let err = QuarryError::new(ErrorCode::BadRequest, format!("invalid url: {e}"));
        (
            StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::BAD_REQUEST),
            Json(Envelope::<()>::err(&request_id, err)),
        )
    })?;

    if !zdr.is_active() && should_read(&req.cache) {
        if let (Some(prev), Some(cache)) = (req.prev_fingerprint.as_ref(), state.cache.as_ref()) {
            if let Some(cached) = cache.get(prev).await {
                return Ok(Json(Envelope::ok(request_id, cached)));
            }
        }
    }

    let plan = plan_from_signals(req.signals.unwrap_or_default());
    let driver = pick_driver(&state, &plan, req.prefer_http3);

    let ingest_client = if req.ingest.unwrap_or(false) {
        state.ingest.clone()
    } else {
        None
    };

    let runner = PageRunner {
        driver,
        security: state.security.clone(),
        artifacts: state.artifacts.clone(),
        event_sink: state.event_sink.clone(),
        zdr,
        ingest: ingest_client,
        org_id: Some(org_id.clone()),
        user_id: Some(claims.user_id.clone()),
        privacy: privacy.clone(),
        cancel_token: None,
        local_index: state.local_index.clone(),
        policy: quarry_runtime::RunPolicy::default(),
        scheduler: state.scheduler.clone(),
        autoscale: Some(quarry_runtime::global_autoscale()),
        render: req
            .render
            .clone()
            .map(RenderHints::from)
            .unwrap_or_default(),
        page_renderer: state.page_renderer.clone(),
    };

    let run_id: RunKind = quarry_core::ids::Id::new();
    let prev_fp = req.prev_fingerprint.clone();
    match runner.run(&run_id, &url, prev_fp).await {
        Ok(out) => {
            if !zdr.is_active() && should_write(&req.cache) {
                if let Some(cache) = state.cache.as_ref() {
                    let ttl = effective_ttl(&req.cache, DEFAULT_TTL_SECS);
                    if let Err(e) = cache.put_with_ttl(&out.fingerprint, &out, ttl).await {
                        tracing::warn!(error = %e, "cache put failed");
                    }
                }
            }
            // P3 / cluster #billing — meter a scrape.page event on
            // success. Skipped on error so unsuccessful fetches don't
            // bill the org. ZDR mode is recorded in metadata for audit.
            state
                .usage
                .meter(quarry_runtime::UsageEvent::new(
                    run_id.to_string(),
                    org_id.clone(),
                    quarry_runtime::usage_metrics::SCRAPE_PAGE,
                    1.0,
                    serde_json::json!({
                        "user_id": claims.user_id,
                        "url": out.url.final_url,
                        "status": out.status,
                        "zdr": zdr.is_active(),
                        "purpose_id": privacy.purpose_id,
                        "privacy_classification": privacy.privacy_classification,
                        "allow_third_party_processing": privacy.allow_third_party_processing,
                        "fingerprint": out.fingerprint,
                    }),
                ))
                .await;
            Ok(Json(Envelope::ok(request_id, out)))
        }
        Err(err) => Err((
            StatusCode::from_u16(err.code.http_status())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(Envelope::<()>::err(&request_id, err)),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct CrawlRequest {
    pub url: String,
    #[serde(default)]
    pub max_pages: Option<u32>,
    /// Selective ingest (Phase 2): true = successful crawled pages are durably
    /// persisted+embedded into the Data Plane (owner=initiator/private via
    /// Phase 1). Absent/false = working-set only (default NEVER). The gateway
    /// sets this from the user's crawl_ingest_mode (auto|never|prompt).
    #[serde(default)]
    pub ingest: Option<bool>,
}

async fn crawl_handoff(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<CrawlRequest>,
) -> Result<Json<Envelope<HandoffAck>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let org_id = claims.org_id.clone();

    // Cycle 19 / cluster #19: emit HostDiscovered for autocomplete-core
    // consumption when this seed URL introduces a brand-new host. The
    // event carries the verified org_id so downstream consumers can
    // partition by tenant.
    if let Ok(parsed) = url::Url::parse(&req.url) {
        if let Some(host) = parsed.host_str() {
            let run_id: RunKind = quarry_core::ids::Id::new();
            let idem = format!("host_discovered:{}:{}", run_id, host);
            state
                .event_sink
                .emit(
                    run_id,
                    quarry_core::event::EventType::HostDiscovered,
                    serde_json::json!({
                        "host": host,
                        "seed_url": req.url,
                        "max_pages": req.max_pages,
                        "org_id": org_id,
                        "user_id": claims.user_id,
                    }),
                    idem,
                )
                .await;
        }
    }

    // Phase 3+: POST to orchestrator with verified tenant identity.
    let ack = crate::handoff::forward_to_orchestrator(
        &state.control_base_url,
        &request_id,
        serde_json::json!({
            "kind": "crawl",
            "url": req.url,
            "max_pages": req.max_pages,
            "org_id": org_id,
            "user_id": claims.user_id,
            "ingest": req.ingest,
        }),
    )
    .await;

    // P3 / cluster #billing — meter a crawl.seed event. The handoff
    // request is the billable unit at this surface (the orchestrator
    // independently meters per-page scrapes via /v1/internal/run_page,
    // so we avoid double-billing the page-level work here).
    state
        .usage
        .meter(quarry_runtime::UsageEvent::new(
            request_id.clone(),
            org_id.clone(),
            quarry_runtime::usage_metrics::CRAWL_SEED,
            1.0,
            serde_json::json!({
                "user_id": claims.user_id,
                "url": req.url,
                "max_pages": req.max_pages,
            }),
        ))
        .await;
    match ack {
        Ok(v) => Ok(Json(Envelope::ok(request_id, v))),
        Err(err) => Err((
            StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(Envelope::<()>::err(&request_id, err)),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct BatchRequest {
    pub urls: Vec<String>,
    /// Selective ingest (Phase 2): true = persist+embed crawled pages; absent
    /// = working-set only (default NEVER). Gateway sets from crawl_ingest_mode.
    #[serde(default)]
    pub ingest: Option<bool>,
}

async fn batch_handoff(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<BatchRequest>,
) -> Result<Json<Envelope<HandoffAck>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let ack = crate::handoff::forward_to_orchestrator(
        &state.control_base_url,
        &request_id,
        serde_json::json!({
            "kind": "batch",
            "urls": req.urls,
            "org_id": claims.org_id,
            "user_id": claims.user_id,
            "ingest": req.ingest,
        }),
    )
    .await;

    // P3 / cluster #billing — one batch.url event with quantity = url
    // count. Cheaper than emitting N individual events; billing-core can
    // still derive per-URL pricing via the float quantity.
    state
        .usage
        .meter(quarry_runtime::UsageEvent::new(
            request_id.clone(),
            claims.org_id.clone(),
            quarry_runtime::usage_metrics::BATCH_URL,
            req.urls.len() as f64,
            serde_json::json!({
                "user_id": claims.user_id,
                "url_count": req.urls.len(),
            }),
        ))
        .await;
    match ack {
        Ok(v) => Ok(Json(Envelope::ok(request_id, v))),
        Err(err) => Err((
            StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(Envelope::<()>::err(&request_id, err)),
        )),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HandoffAck {
    pub job_id: String,
    pub accepted_at: chrono::DateTime<chrono::Utc>,
}

/// Internal endpoint called by orchestrator activities.
/// Returns a compact result shape matching activities.RunPageResult in Go.
#[derive(Debug, Deserialize)]
pub struct InternalRunPage {
    pub url: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub prev_fingerprint: Option<String>,
    #[serde(default)]
    pub cache: Option<CachePolicy>,
    #[serde(default)]
    pub zdr: Option<bool>,
    #[serde(default)]
    pub privacy: Option<PrivacyPolicy>,
    #[serde(default)]
    pub signals: Option<DriverSignals>,
    #[serde(default)]
    pub ingest: Option<bool>,
    #[serde(default)]
    pub org_id: Option<String>,
    /// Initiating user id, set by the orchestrator from the verified Edge JWT.
    /// Threaded into PageRunner so durable ingests are owner-stamped private.
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub render: Option<RenderHintsRequest>,
    #[serde(default)]
    pub prefer_http3: bool,
}

#[derive(Debug, Serialize)]
pub struct InternalRunPageResult {
    pub run_id: String,
    pub status: u16,
    pub fingerprint: String,
    pub links: Vec<String>,
    /// MIME from the upstream Content-Type header (best-effort).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// <title> when parseable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Output of `quarry_transform::branding_rendered::extract`.
    /// Forwarded by the orchestrator as a `branding_extracted` event so
    /// consumers polling `/v1/jobs/{id}/events` (e.g. velion's
    /// onboarding wizard) see real brand signals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branding: Option<serde_json::Value>,
}

/// The shared runtime service token, latched once at startup (mirrors
/// `auth::dev_bypass_enabled`): re-reading per request is a syscall on the hot
/// path and would let a post-boot env mutation silently swap the secret. Test
/// builds skip the latch so each test can set the env independently.
fn runtime_token() -> String {
    #[cfg(test)]
    {
        read_runtime_token_from_env()
    }
    #[cfg(not(test))]
    {
        use std::sync::OnceLock;
        static CACHED: OnceLock<String> = OnceLock::new();
        CACHED.get_or_init(read_runtime_token_from_env).clone()
    }
}

fn read_runtime_token_from_env() -> String {
    std::env::var("QUARRY_EDGE__RUNTIME_AUTH_TOKEN")
        .or_else(|_| std::env::var("RUNTIME_AUTH_TOKEN"))
        .unwrap_or_default()
}

/// Verify the shared runtime service token the orchestrator presents on the
/// internal execution route. Fail-closed: an unset token refuses the call.
/// Service credential (NOT a per-tenant JWT); constant-time compared.
fn verify_runtime_token(headers: &HeaderMap) -> Result<(), QuarryError> {
    let expected = runtime_token();
    if expected.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "run_page not configured (set QUARRY_EDGE__RUNTIME_AUTH_TOKEN)",
        ));
    }
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    if !provided.is_empty()
        && crate::change_routes::ct_eq(provided.as_bytes(), expected.as_bytes())
    {
        Ok(())
    } else {
        tracing::warn!(
            had_token = !provided.is_empty(),
            "run_page runtime-token auth failed (possible probe)"
        );
        Err(QuarryError::new(
            ErrorCode::Unauthorized,
            "invalid runtime token",
        ))
    }
}

async fn internal_run_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<InternalRunPage>,
) -> Result<Json<InternalRunPageResult>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    // Internal orchestrator route (NOT a per-tenant JWT surface): enforce the
    // shared runtime service token. The originating org travels in the request
    // body, stamped by the orchestrator from the schedule memo (verified at
    // schedule-creation), so we use it directly rather than a JWT claim.
    verify_runtime_token(&headers).map_err(|e| {
        (
            StatusCode::from_u16(e.code.http_status()).unwrap_or(StatusCode::UNAUTHORIZED),
            Json(Envelope::<()>::err(&request_id, e)),
        )
    })?;
    let org_id = req.org_id.clone().unwrap_or_default();
    if org_id.trim().is_empty() {
        let err =
            QuarryError::new(ErrorCode::BadRequest, "run_page requires org_id in the body");
        return Err((
            StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::BAD_REQUEST),
            Json(Envelope::<()>::err(&request_id, err)),
        ));
    }

    // HMAC org-binding. The runtime bearer token (above) authenticates the
    // CALLER; this signature binds the TENANT identity so a leaked bearer
    // can't be replayed against an arbitrary org_id. We sign over the body's
    // raw `org_id` + "\n" + raw `url` (BEFORE URL-parsing, so the bytes the
    // orchestrator signed are exactly the bytes we verify). When the edge
    // has no shared secret configured we skip the check (runtime token only)
    // and warn that the binding is unenforced.
    if let Some(signer) = state.internal_signer.as_ref() {
        let sig = headers
            .get("x-quarry-run-sig")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !signer.verify_run_binding(&org_id, &req.url, sig) {
            tracing::warn!(had_sig = !sig.is_empty(), "run_page HMAC org-binding failed");
            let err =
                QuarryError::new(ErrorCode::Unauthorized, "run_page org binding invalid");
            return Err((
                StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::UNAUTHORIZED),
                Json(Envelope::<()>::err(&request_id, err)),
            ));
        }
    } else {
        // Fail CLOSED: run_page is a tenant-scoped execution surface, so a
        // missing shared secret must refuse the request rather than silently
        // degrade to runtime-token-only (which would re-open org-forgery).
        // The edge needs QUARRY_EDGE__INTERNAL_SECRET for its control-plane
        // HMAC peer anyway, so this is always configured in a real deployment.
        tracing::error!(
            "run_page rejected: QUARRY_EDGE__INTERNAL_SECRET not configured; cannot verify org binding"
        );
        let err = QuarryError::new(
            ErrorCode::Unsupported,
            "run_page org binding not configured (set QUARRY_EDGE__INTERNAL_SECRET)",
        );
        return Err((
            StatusCode::from_u16(err.code.http_status())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(Envelope::<()>::err(&request_id, err)),
        ));
    }

    let zdr = ZdrMode::from(req.zdr.unwrap_or(false));
    let privacy = effective_privacy(req.privacy.clone(), zdr);

    let url: Url = req.url.parse().map_err(|e| {
        let err = QuarryError::new(ErrorCode::BadRequest, format!("invalid url: {e}"));
        (
            StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::BAD_REQUEST),
            Json(Envelope::<()>::err(&request_id, err)),
        )
    })?;

    let plan = plan_from_signals(req.signals.unwrap_or_default());
    let driver = pick_driver(&state, &plan, req.prefer_http3);

    let ingest_client = if req.ingest.unwrap_or(false) {
        state.ingest.clone()
    } else {
        None
    };

    let runner = PageRunner {
        driver,
        security: state.security.clone(),
        artifacts: state.artifacts.clone(),
        event_sink: state.event_sink.clone(),
        zdr,
        ingest: ingest_client,
        org_id: Some(org_id),
        user_id: req.user_id.clone(),
        privacy,
        cancel_token: None,
        local_index: state.local_index.clone(),
        policy: quarry_runtime::RunPolicy::default(),
        scheduler: state.scheduler.clone(),
        autoscale: Some(quarry_runtime::global_autoscale()),
        render: req
            .render
            .clone()
            .map(RenderHints::from)
            .unwrap_or_default(),
        page_renderer: state.page_renderer.clone(),
    };

    let run_id: RunKind = match req.run_id.as_deref() {
        Some(s) => s.parse().unwrap_or_else(|_| quarry_core::ids::Id::new()),
        None => quarry_core::ids::Id::new(),
    };

    if !zdr.is_active() && should_read(&req.cache) {
        if let (Some(prev), Some(cache)) = (req.prev_fingerprint.as_ref(), state.cache.as_ref()) {
            if let Some(cached) = cache.get(prev).await {
                return Ok(Json(InternalRunPageResult {
                    run_id: run_id.to_string(),
                    status: cached.status,
                    fingerprint: cached.fingerprint,
                    links: cached
                        .formats
                        .links
                        .iter()
                        .map(|l| l.href.clone())
                        .collect(),
                    content_type: cached.metadata.content_type.clone(),
                    title: cached.metadata.title.clone(),
                    branding: cached.branding.clone(),
                }));
            }
        }
    }

    let prev_fp = req.prev_fingerprint.clone();
    match runner.run(&run_id, &url, prev_fp).await {
        Ok(out) => {
            if !zdr.is_active() && should_write(&req.cache) {
                if let Some(cache) = state.cache.as_ref() {
                    let ttl = effective_ttl(&req.cache, DEFAULT_TTL_SECS);
                    if let Err(e) = cache.put_with_ttl(&out.fingerprint, &out, ttl).await {
                        tracing::warn!(error = %e, "cache put failed");
                    }
                }
            }
            Ok(Json(InternalRunPageResult {
                run_id: run_id.to_string(),
                status: out.status,
                fingerprint: out.fingerprint,
                links: out.formats.links.iter().map(|l| l.href.clone()).collect(),
                content_type: out.metadata.content_type.clone(),
                title: out.metadata.title.clone(),
                branding: out.branding.clone(),
            }))
        }
        Err(err) => Err((
            StatusCode::from_u16(err.code.http_status())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(Envelope::<()>::err(&request_id, err)),
        )),
    }
}

#[allow(dead_code)]
fn _keep(_a: Arc<()>) {}

async fn scrape_stream(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<ScrapeRequest>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let zdr = ZdrMode::from(req.zdr.unwrap_or(false));
    let privacy = effective_privacy(req.privacy.clone(), zdr);
    // Tenant enforcement: claim wins over any client-supplied org_id.
    let _ = &req.org_id;
    let org_id = claims.org_id.clone();
    let stream = async_stream::stream! {
        let url = match req.url.parse::<Url>() {
            Ok(u) => u,
            Err(e) => {
                yield Ok(Event::default().event("error").data(e.to_string()));
                return;
            }
        };
        if !zdr.is_active() && should_read(&req.cache) {
            if let (Some(prev), Some(cache)) = (req.prev_fingerprint.as_ref(), state.cache.as_ref()) {
                if let Some(cached) = cache.get(prev).await {
                    let data = serde_json::to_string(&cached).unwrap_or_default();
                    yield Ok(Event::default().event("result").data(data));
                    yield Ok(Event::default().data("done"));
                    return;
                }
            }
        }
        let plan = plan_from_signals(req.signals.clone().unwrap_or_default());
        let driver = pick_driver(&state, &plan, req.prefer_http3);

        let ingest_client = if req.ingest.unwrap_or(false) {
            state.ingest.clone()
        } else {
            None
        };

        let runner = PageRunner {
            driver,
            security: state.security.clone(),
            artifacts: state.artifacts.clone(),
            event_sink: state.event_sink.clone(),
            zdr,
            ingest: ingest_client,
            org_id: Some(org_id.clone()),
            user_id: Some(claims.user_id.clone()),
            privacy: privacy.clone(),
            cancel_token: None,
            local_index: state.local_index.clone(),
            policy: quarry_runtime::RunPolicy::default(),
            scheduler: state.scheduler.clone(),
            autoscale: Some(quarry_runtime::global_autoscale()),
            render: req.render.clone().map(RenderHints::from).unwrap_or_default(),
            page_renderer: state.page_renderer.clone(),
        };
        let run_id = RunKind::new();
        let mut rx = state.event_sink.subscribe(&run_id);

        // Spawn the runner; forward broadcast events as they arrive.
        let runner_run_id = run_id.clone();
        let runner_url = url.clone();
        let prev_fp = req.prev_fingerprint.clone();
        let cache = state.cache.clone();
        let policy = req.cache.clone();
        let runner_handle = tokio::spawn(async move {
            let res = runner.run(&runner_run_id, &runner_url, prev_fp).await;
            if let Ok(ref out) = res {
                if !zdr.is_active() && should_write(&policy) {
                    if let Some(c) = cache.as_ref() {
                        let ttl = effective_ttl(&policy, DEFAULT_TTL_SECS);
                        if let Err(e) = c.put_with_ttl(&out.fingerprint, out, ttl).await {
                            tracing::warn!(error = %e, "cache put failed");
                        }
                    }
                }
            }
            res
        });

        // Drain broadcast events until the runner finishes and the channel drains.
        loop {
            let recv = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
            match recv {
                Ok(Ok(env)) => {
                    let ev_name = serde_json::to_value(env.event_type)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_else(|| "event".into());
                    if let Ok(ev) = Event::default().event(ev_name).json_data(&env) {
                        yield Ok(ev);
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                Err(_) => {
                    if runner_handle.is_finished() {
                        while let Ok(env) = rx.try_recv() {
                            let ev_name = serde_json::to_value(env.event_type)
                                .ok()
                                .and_then(|v| v.as_str().map(str::to_owned))
                                .unwrap_or_else(|| "event".into());
                            if let Ok(ev) = Event::default().event(ev_name).json_data(&env) {
                                yield Ok(ev);
                            }
                        }
                        break;
                    }
                }
            }
        }

        match runner_handle.await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                yield Ok(Event::default().event("error").data(e.to_string()));
            }
            Err(e) => {
                yield Ok(Event::default().event("error").data(format!("join error: {e}")));
            }
        }
        yield Ok(Event::default().data("done"));
    };
    Sse::new(stream)
}

fn effective_privacy(privacy: Option<PrivacyPolicy>, zdr: ZdrMode) -> PrivacyPolicy {
    privacy.unwrap_or_default().with_zdr(zdr)
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use quarry_core::error::{ErrorCode, QuarryError};
    use quarry_core::output::DriverKind;
    use quarry_core::QuarryResult;
    use quarry_runtime::artifact_store::{ArtifactStore, InMemoryStore};
    use quarry_runtime::driver::{Driver, FetchHints};
    use quarry_runtime::driver_registry::DriverRegistry;
    use quarry_runtime::fetch::FetchResponse;
    use quarry_runtime::{EventSink, NoopUsageMeter, RunPolicy};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use tokio::sync::mpsc;

    struct TestDriver {
        kind: DriverKind,
        calls: Arc<AtomicU32>,
        result: TestResult,
    }

    enum TestResult {
        Ok(&'static [u8]),
        Err(ErrorCode),
    }

    impl TestDriver {
        fn ok(kind: DriverKind, calls: Arc<AtomicU32>, body: &'static [u8]) -> Self {
            Self {
                kind,
                calls,
                result: TestResult::Ok(body),
            }
        }

        fn err(kind: DriverKind, calls: Arc<AtomicU32>, code: ErrorCode) -> Self {
            Self {
                kind,
                calls,
                result: TestResult::Err(code),
            }
        }
    }

    #[async_trait]
    impl Driver for TestDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }

        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            self.fetch_conditional(url, &FetchHints::default()).await
        }

        async fn fetch_conditional(
            &self,
            url: &Url,
            _hints: &FetchHints,
        ) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            match self.result {
                TestResult::Ok(body) => Ok(FetchResponse {
                    status: 200,
                    final_url: url.clone(),
                    headers: vec![],
                    body: body.to_vec(),
                    duration_ms: 1,
                }),
                TestResult::Err(code) => Err(QuarryError::new(code, "simulated transport failure")),
            }
        }
    }

    #[allow(unexpected_cfgs)]
    fn test_state(
        static_driver: Arc<dyn Driver>,
        drivers: DriverRegistry,
        http3: Option<Arc<dyn Driver>>,
    ) -> AppState {
        test_state_with_artifacts(
            static_driver,
            drivers,
            http3,
            Arc::new(InMemoryStore::new()),
        )
    }

    #[allow(unexpected_cfgs)]
    fn test_state_with_artifacts(
        static_driver: Arc<dyn Driver>,
        drivers: DriverRegistry,
        http3: Option<Arc<dyn Driver>>,
        artifacts: Arc<dyn ArtifactStore>,
    ) -> AppState {
        let (tx, mut rx) = mpsc::channel(8);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        AppState {
            driver: static_driver,
            drivers,
            http3,
            security: Arc::new(quarry_security::preflight::DefaultEngine::new()),
            artifacts,
            control_base_url: String::new(),
            redis: None,
            cache: None,
            event_sink: EventSink::new(tx),
            ingest: None,
            profiles: Arc::new(quarry_browser::session::InMemoryProfileStore::new()),
            search: None,
            vector_index: None,
            searxng_url: None,
            model_plane_url: None,
            model_plane_token: None,
            answer_pipeline: None,
            local_index: None,
            usage: Arc::new(NoopUsageMeter),
            policy: RunPolicy::default(),
            scheduler: None,
            internal_signer: None,
            page_renderer: None,
            #[cfg(feature = "postgres-queue")]
            event_history: None,
            #[cfg(feature = "postgres-queue")]
            baseline_store: None,
            #[cfg(feature = "browser-agent")]
            agent_driver: Arc::new(quarry_browser::chromiumoxide::ChromiumoxideDriver::new()),
            #[cfg(feature = "browser-agent")]
            agent_runs: crate::agent_routes::new_runs(),
        }
    }

    #[tokio::test]
    async fn artifact_route_returns_stored_bytes() {
        let calls = Arc::new(AtomicU32::new(0));
        let static_driver = Arc::new(TestDriver::ok(DriverKind::Static, calls, b"unused"));
        let drivers = DriverRegistry::new(DriverKind::Static);
        let artifacts = Arc::new(InMemoryStore::new());
        let handle = artifacts
            .put(
                &RunKind::new(),
                "blake3:browser-frame",
                "screenshot",
                b"png bytes".to_vec(),
            )
            .await
            .expect("stored artifact");
        let state = test_state_with_artifacts(static_driver, drivers, None, artifacts);

        let response = crate::resource_routes::get_artifact(
            State(state),
            axum::extract::Path(handle.artifact_id.to_string()),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap(),
            "application/octet-stream"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert_eq!(&body[..], b"png bytes");
    }

    #[tokio::test]
    async fn prefer_http3_falls_back_to_planned_driver_on_transport_failure() {
        let h3_calls = Arc::new(AtomicU32::new(0));
        let static_calls = Arc::new(AtomicU32::new(0));
        let h3 = Arc::new(TestDriver::err(
            DriverKind::Static,
            h3_calls.clone(),
            ErrorCode::DriverFailed,
        ));
        let static_driver = Arc::new(TestDriver::ok(
            DriverKind::Static,
            static_calls.clone(),
            b"planned",
        ));
        let mut drivers = DriverRegistry::new(DriverKind::Static);
        drivers.register(static_driver.clone());
        let state = test_state(static_driver, drivers, Some(h3));
        let plan = quarry_runtime::DriverPlan::static_fetch("test");
        let url: Url = "https://example.com".parse().unwrap();

        let driver = pick_driver(&state, &plan, true);
        let resp = driver.fetch(&url).await.unwrap();

        assert_eq!(resp.body, b"planned");
        assert_eq!(h3_calls.load(Ordering::Relaxed), 1);
        assert_eq!(static_calls.load(Ordering::Relaxed), 1);
    }
}
