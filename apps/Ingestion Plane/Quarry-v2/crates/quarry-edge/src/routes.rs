use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    middleware,
    response::IntoResponse,
    routing::{delete, get, patch, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing::Instrument as _;
use url::Url;

use quarry_core::cache::{CacheMode, CachePolicy};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::RunKind;
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
        .route("/version", get(version))
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
        .route("/v1/internal/run_page", post(internal_run_page))
        // INNH-6a: sitemap discovery for a crawl. Same runtime token and org binding as
        // run_page; served only with QUARRY_EDGE__CRAWL_SITEMAP on (501 otherwise).
        .route(
            "/v1/internal/crawl/sitemap",
            post(crate::crawl_sitemap::internal_sitemap),
        )
        // Fire-time preflight of an executable scheduled job (WS15.S2). Runtime token
        // plus the schedule run binding; S2 always allows (S4 adds the checks).
        .route(
            "/v1/internal/schedules/preflight",
            post(crate::schedule_preflight::preflight),
        )
        // WS17.E3: org erasure of the profile families. Shared internal token, counts only.
        .route(
            "/v1/internal/orgs/:org/profiles",
            delete(crate::internal_routes::purge_org_profiles),
        )
        // W2-U49: user-level erasure of step receipts. Same token, counts only.
        .route(
            "/v1/internal/orgs/:org/users/:user/receipts",
            delete(crate::internal_routes::purge_user_receipts),
        )
        .merge(crate::queue_routes::internal_router());

    // WS17.P0 — the profile list and import. Their own router: the import body is a whole
    // snapshot, up to 4 MiB, past the 1 MB cap of the surface below. Both methods of the path
    // live here, so no path is split across two routers. The auth chain is the surface's.
    let profile_import = Router::new()
        .route("/v1/profiles", post(crate::profile_routes::save_profile))
        .route("/v1/profiles", get(crate::profile_routes::list_profiles))
        .layer(DefaultBodyLimit::max(crate::profile_routes::IMPORT_MAX_BYTES))
        .layer(RequestBodyLimitLayer::new(crate::profile_routes::IMPORT_MAX_BYTES))
        .layer(middleware::from_fn(crate::auth::require_service_route_scope))
        // INNH-10: an `api_key` principal is held to its route allow-list (dark behind
        // QUARRY_EDGE__API_KEY_AGENT_RUNS; user and service tokens pass through untouched).
        .layer(middleware::from_fn(crate::api_key::require_api_key_route_fence))
        .layer(middleware::from_fn(crate::zdr_log::mark_floor))
        .layer(middleware::from_fn(crate::auth::require_auth));

    // INNH-11 — POST /v1/parse. Its own router: the body is a whole document, up to 25 MiB,
    // past the 1 MB cap of the surface below. The handler enforces the limit itself, so a
    // refusal is the typed envelope, and it takes no AppState: it has nothing to store to.
    // The auth chain is the surface's. Served only with QUARRY_EDGE__PARSE_ROUTE on.
    let parse_upload = Router::new()
        .route("/v1/parse", post(crate::parse_routes::parse))
        .layer(middleware::from_fn(crate::auth::require_service_route_scope))
        // INNH-10 x INNH-11: an `api_key` principal is held to its allow-list here too; /v1/parse
        // is not on it, so a key is refused (403 `api_key_route_refused`). This router's layers
        // also wrap the fallback of the merged router, so the fence must be here for an unknown
        // path to be answered 403 to a key, as the other two routers do.
        .layer(middleware::from_fn(crate::api_key::require_api_key_route_fence))
        .layer(middleware::from_fn(crate::zdr_log::mark_floor))
        .layer(middleware::from_fn(crate::auth::require_auth));

    // Protected surface — every /v1/* route. The auth middleware
    // verifies an `Authorization: Bearer <jwt>` against the Control
    // Plane's `auth-core` JWKS (or dev-bypass), then inserts `Claims`
    // into request extensions. Handlers recover claims via
    // `Extension<Claims>` and use the verified `org_id` to scope all
    // data access — client-supplied `org_id` fields are overwritten.
    let protected = Router::new()
        .route("/v1/scrape", post(scrape))
        .route("/v1/scrape/stream", post(scrape_stream))
        .route("/v1/scrape/:id", get(crate::scrape_routes::get_scrape))
        .route("/v1/crawl", post(crawl_handoff))
        .route("/v1/batch", post(batch_handoff))
        // WS15.L1 — job lifecycle reads. `ongoing` is static, so it is matched
        // before the `:id` routes of the same kind.
        .route("/v1/crawl/ongoing", get(crate::job_lifecycle_routes::ongoing_crawl))
        .route("/v1/crawl/:id", get(crate::job_lifecycle_routes::get_crawl))
        .route("/v1/crawl/:id/errors", get(crate::job_lifecycle_routes::crawl_errors))
        // INNH-12: the pages a job fetched. Off unless QUARRY_EDGE__JOB_RESULTS is on.
        .route("/v1/crawl/:id/results", get(crate::job_results_routes::crawl_results))
        .route("/v1/batch/:id/results", get(crate::job_results_routes::batch_results))
        .route("/v1/jobs/:id/results", get(crate::job_results_routes::job_results))
        // INNH-5: org-scoped webhook subscriptions (control owns them; the edge passes the
        // verified org). Off unless QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS is on (501).
        .route(
            "/v1/webhook-subscriptions",
            get(crate::webhook_subscription_routes::list).post(crate::webhook_subscription_routes::create),
        )
        .route(
            "/v1/webhook-subscriptions/:id",
            get(crate::webhook_subscription_routes::get)
                .patch(crate::webhook_subscription_routes::patch)
                .delete(crate::webhook_subscription_routes::delete),
        )
        .route(
            "/v1/webhook-subscriptions/:id/rotate-secret",
            post(crate::webhook_subscription_routes::rotate_secret),
        )
        .route(
            "/v1/webhook-subscriptions/:id/deliveries",
            get(crate::webhook_subscription_routes::deliveries),
        )
        .route(
            "/v1/webhook-subscriptions/:id/deliveries/:did/retry",
            post(crate::webhook_subscription_routes::retry_delivery),
        )
        .route("/v1/batch/ongoing", get(crate::job_lifecycle_routes::ongoing_batch))
        .route("/v1/batch/:id", get(crate::job_lifecycle_routes::get_batch))
        .route("/v1/batch/:id/errors", get(crate::job_lifecycle_routes::batch_errors))
        // Job control (WS15.L2): cancel, pause and resume. Off unless QUARRY_EDGE__JOB_CONTROL is on.
        .route("/v1/crawl/:id", axum::routing::delete(crate::job_lifecycle_routes::cancel_crawl))
        .route("/v1/crawl/:id/pause", post(crate::job_lifecycle_routes::pause_crawl))
        .route("/v1/crawl/:id/resume", post(crate::job_lifecycle_routes::resume_crawl))
        .route("/v1/batch/:id", axum::routing::delete(crate::job_lifecycle_routes::cancel_batch))
        .route("/v1/batch/:id/pause", post(crate::job_lifecycle_routes::pause_batch))
        .route("/v1/batch/:id/resume", post(crate::job_lifecycle_routes::resume_batch))
        // WS15.L3: the job event tail by kind (`follow=true` on a flagged edge), same handler as /v1/jobs/:id/events.
        .route("/v1/crawl/:id/events", get(crate::resource_routes::list_crawl_events))
        .route("/v1/batch/:id/events", get(crate::resource_routes::list_batch_events))
        .route("/v1/jobs/:id", get(crate::job_lifecycle_routes::get_job))
        .route("/v1/jobs/:id/errors", get(crate::job_lifecycle_routes::job_errors))
        // Browser Workspace Phase 3 continuation — explicit named/scoped
        // profile create + rename/rescope, independent of the internal
        // snapshot-capture `save_profile` write path above.
        .route(
            "/v1/profiles/create",
            post(crate::profile_routes::create_profile),
        )
        .route("/v1/profiles/:id", get(crate::profile_routes::load_profile))
        .route(
            "/v1/profiles/:id",
            patch(crate::profile_routes::update_profile),
        )
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
        // The BFF has been calling `/api/v1/search/videos` since the VIDEOS tab
        // shipped; with no route here it fell straight through to SearXNG and
        // skipped org scoping, metering and the shared error envelope. Keep it
        // next to `images` — the two verticals share a handler shape and a
        // SearXNG dependency, and a future one belongs in the same block.
        .route("/v1/search/videos", post(crate::search_routes::videos))
        .route("/v1/search/similar", post(crate::search_routes::similar))
        .route("/v1/search/suggest", post(crate::search_routes::suggest))
        .route("/v1/map", post(crate::map_routes::map))
        .route("/v1/audit/page", post(crate::audit_page::capture))
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
        // Crawl 0-pages fix — durable read keyed by the handoff `job_id`
        // (a run_id is not assigned synchronously at handoff). Forwards to
        // control's `/v1/jobs/{id}/events`; the gateway re-emits the JSON
        // array as SSE for the SPA.
        .route(
            "/v1/jobs/:id/events",
            get(crate::resource_routes::list_job_events),
        )
        // C30.2 / cluster #9 — versioned change tracking.
        .route("/v1/change/check", post(crate::change_routes::check))
        .route("/v1/change/latest", get(crate::change_routes::latest))
        .route("/v1/change/history", get(crate::change_routes::history))
        // INNH-4: the two versions of a tracked page and the diff between them. 501 unless
        // QUARRY_EDGE__CHANGE_TEXT_DIFF is on.
        .route("/v1/change/diff", get(crate::change_routes::diff))
        // Parity finishers — PromoteTrackedResultToSnapshot +
        // ScheduleRefreshRun over the PostgresBaselineStore / durable
        // frontier. Both 501 with a hint when postgres-queue is off.
        .route(
            "/v1/change/snapshot",
            post(crate::change_routes::promote_snapshot),
        )
        .route(
            "/v1/change/refresh",
            post(crate::change_routes::schedule_refresh),
        )
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
        // W5 — fleet orchestration. Shares the same auth scope as agent
        // runs (create/list/get/member/budget all org-scoped via verified
        // JWT). No-op when the `browser-agent` feature is disabled so
        // default builds stay lean.
        .merge(crate::fleet_routes::fleet_router())
        .layer(middleware::from_fn(
            crate::auth::require_service_route_scope,
        ))
        // INNH-10: the API-key route fence, beside the service scope (see the profile router).
        .layer(middleware::from_fn(crate::api_key::require_api_key_route_fence))
        // After require_auth (which inserts the claims), before the handler: a
        // user of a ZDR org is marked ZDR for the log from the first line.
        .layer(middleware::from_fn(crate::zdr_log::mark_floor))
        .layer(middleware::from_fn(crate::auth::require_auth));

    // Cap request bodies at 1 MB. Largest legit payload is /v1/batch
    // with a few hundred URLs (~30 KB). Without this an attacker can
    // stream an arbitrarily large JSON body to /v1/answer or
    // /v1/scrape and pin a request thread in serde decoding. axum's
    // default body-limit middleware is permissive; this layer is
    // explicit and tunable.
    let body_limit = RequestBodyLimitLayer::new(1024 * 1024);
    // The 1 MB cap covers the surface; the profile import has its own cap (WS17.P0).
    let limited = Router::new()
        .merge(public)
        .merge(internal)
        .merge(protected)
        .layer(body_limit);

    Router::new()
        .merge(limited)
        .merge(profile_import)
        .merge(parse_upload)
        .layer(middleware::from_fn(crate::retry_after::retry_after_header))
        // Method and path only: tower-http's default span records the full URI,
        // query string included.
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<_>| {
                crate::request_id::make_request_span(request)
            }),
        )
        // WS13.5: path-aware deadline (60 s, or 125 s for POST /v1/scrape with actions on).
        .layer(middleware::from_fn(crate::scrape_actions::path_deadline))
        // WS10.S1: outermost, so the span above sees the id and the
        // x-request-id header is echoed on every answer (timeout and body-limit
        // answers included).
        .layer(middleware::from_fn(crate::request_id::layer))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
async fn ready(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    // Shutting down: take this instance out of rotation now, before it is gone.
    if crate::drain::is_draining() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "draining"})),
        );
    }
    if state.readiness.durable {
        // The local browsers (WS3.F). The edge runs two driver instances, each
        // with its own Chromium: agent runs' (`browser`) and the scrape
        // path's (`scrape_browser`). Each is reported when it runs a local
        // browser; 503 `browser_unhealthy` when either one is Unhealthy
        // (every launched browser of that driver unhealthy or dead). Drivers
        // without a local browser (NotApplicable) leave the body as it was.
        #[cfg(feature = "browser-agent")]
        {
            use quarry_browser::BrowserHealthState;
            let local = |health: quarry_browser::BrowserHealth| {
                (health.state != BrowserHealthState::NotApplicable).then_some(health)
            };
            let agent = local(state.agent_driver.health().await);
            let scrape = state.drivers.browser_health().await.and_then(local);
            if agent.is_some() || scrape.is_some() {
                let unhealthy = agent
                    .iter()
                    .chain(scrape.iter())
                    .any(|health| health.state == BrowserHealthState::Unhealthy);
                let mut body = serde_json::json!({
                    "status": if unhealthy { "not_ready" } else { "ready" },
                    "durable": true,
                });
                if unhealthy {
                    body["reason"] = serde_json::json!("browser_unhealthy");
                }
                if let Some(agent) = agent {
                    body["browser"] = serde_json::json!(agent);
                }
                if let Some(scrape) = scrape {
                    body["scrape_browser"] = serde_json::json!(scrape);
                }
                let status = if unhealthy {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::OK
                };
                return (status, Json(body));
            }
        }
        (
            StatusCode::OK,
            Json(serde_json::json!({"status": "ready", "durable": true})),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "not_ready",
                "durable": false,
                "reason": state.readiness.reason,
            })),
        )
    }
}

/// Reports what is actually running in this container: the git revision and
/// build timestamp baked into the image (see `Dockerfile.edge`'s
/// `SOURCE_REVISION` / `BUILD_DATE` build args, re-exposed as runtime `ENV`
/// so no Docker/registry access is needed to answer "what SHA is deployed
/// here"). Falls back to the same `unverified`/`unknown` defaults the image
/// LABELs use when unset.
async fn version() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "quarry-edge-rs",
        "revision": std::env::var("SOURCE_REVISION").unwrap_or_else(|_| "unverified".to_string()),
        "build_date": std::env::var("BUILD_DATE").unwrap_or_else(|_| "unknown".to_string()),
        "cargo_version": env!("CARGO_PKG_VERSION"),
    }))
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
    /// WS7.C: the request's web-policy ask. It can only tighten; an unknown
    /// capability or key is a 400 (see `quarry_core::web_policy::RequestAsk`).
    #[serde(default)]
    pub policy: Option<quarry_core::web_policy::RequestAsk>,
    /// WS13.4b: the raw `formats`. Read only by the stream route, which serves
    /// no captures and refuses a `screenshot` or `pdf` entry. `/v1/scrape`
    /// reads its formats through the contract parse, not through this field.
    #[serde(default)]
    pub formats: Option<serde_json::Value>,
    /// WS13.5: the raw `actions`. The contract parse checks them on `/v1/scrape`;
    /// the stream route refuses them.
    #[serde(default)]
    pub actions: Option<serde_json::Value>,
    /// WS13.5: a tighten-only hint. Only `"agent"` changes anything: it refuses
    /// `actions` whatever the token (`scrape_actions::check_principal`). Unknown
    /// values are ignored.
    #[serde(default)]
    pub initiated_by: Option<String>,
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
            // WS13.4b: captures are set by `scrape_actions::apply_capture`, never by the wire shape.
            ..RenderHints::default()
        }
    }
}

async fn scrape(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    body: axum::body::Bytes,
) -> Result<axum::response::Response, (StatusCode, axum::response::Response)> {
    scrape_with(
        state,
        claims,
        &body,
        crate::scrape_contract::ScrapeSettings::current(),
    )
    .await
}

/// A 400 for a request whose body or options are not acceptable: the canonical
/// envelope with `details.fields[] {path, code, message}`.
fn contract_error(
    request_id: &str,
    fields: Vec<quarry_core::scrape::FieldError>,
) -> (StatusCode, axum::response::Response) {
    let error = QuarryError::new(ErrorCode::BadRequest, "the request has invalid fields")
        .with_details(serde_json::json!({ "fields": fields }));
    (
        StatusCode::BAD_REQUEST,
        Json(Envelope::<()>::err(request_id, error)).into_response(),
    )
}

/// `/v1/scrape` with its settings passed in, so tests can pick strictness.
async fn scrape_with(
    state: AppState,
    claims: crate::auth::Claims,
    body: &[u8],
    settings: &crate::scrape_contract::ScrapeSettings,
) -> Result<axum::response::Response, (StatusCode, axum::response::Response)> {
    use quarry_core::scrape::{CacheState, EffectiveScrapeOptions, ScrapeFormat};

    let request_id = crate::request_id::current();
    let parsed = crate::scrape_contract::parse_body(body, settings, chrono::Utc::now())
        .map_err(|fields| contract_error(&request_id, fields))?;
    let req: ScrapeRequest = serde_json::from_value(parsed.value.clone()).map_err(|error| {
        contract_error(
            &request_id,
            vec![quarry_core::scrape::FieldError::new(
                "$",
                "invalid_request",
                error.to_string(),
            )],
        )
    })?;
    if req.ingest.unwrap_or(false) && claims.is_service() && !claims.has_scope("scrape:write") {
        let error = QuarryError::new(
            ErrorCode::Forbidden,
            "scrape ingest requires the scrape:write service scope",
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(Envelope::<()>::err(&request_id, error)).into_response(),
        ));
    }
    // Body flag raised (never lowered) by the token's ZDR floor: see
    // `Claims::zdr_floor`.
    let zdr = claims.effective_zdr_mode(req.zdr);
    let resolved = parsed
        .options
        .clone()
        .resolve(zdr.is_active(), &settings.limits())
        .map_err(|fields| contract_error(&request_id, fields))?;
    if !parsed.unknown_keys.is_empty() {
        // Names only: a key is part of the contract, a value is the caller's.
        tracing::warn!(
            target: "quarry.scrape.unknown_field",
            org_id = %claims.org_id,
            principal = %claims.actor_id(),
            fields = ?parsed.unknown_keys,
            "scrape request carried fields that are not part of the contract"
        );
    }
    // WS13.5: a request with actions needs a user, or a service principal holding
    // scrape:interact; a request labelled as a Model action is refused.
    if resolved.actions.is_some() {
        if let Err(error) = crate::scrape_actions::check_principal(
            &claims,
            req.initiated_by.as_deref(),
            crate::scrape_actions::actions_mode(),
        ) {
            return Err((
                StatusCode::FORBIDDEN,
                Json(Envelope::<()>::err(&request_id, error)).into_response(),
            ));
        }
    }
    // INNH-1: AI formats (json, summary, question, highlights). The flag is already
    // on when any is present (the resolve above refused them otherwise). Who may ask
    // and whether a model can be reached are decided here, before the page is
    // fetched: a refused request spends no fetch. Under ZDR the model is never
    // reached, so its availability is not asked.
    let ai_specs: Vec<quarry_core::ai_spec::AiSpec> =
        resolved.ai_specs().into_iter().cloned().collect();
    if !ai_specs.is_empty() {
        let refusal = crate::scrape_contract::ai_formats::check_principal(
            &claims,
            req.initiated_by.as_deref(),
            crate::scrape_contract::ai_formats::first_ai_index(parsed.value.get("formats")),
        )
        .and_then(|()| {
            if zdr.is_active() {
                Ok(())
            } else {
                crate::scrape_contract::ai_formats::availability(&state)
            }
        });
        if let Err(error) = refusal {
            return Err((
                StatusCode::from_u16(error.code.http_status())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                Json(Envelope::<()>::err(&request_id, error)).into_response(),
            ));
        }
    }
    let privacy = effective_privacy(req.privacy.clone(), zdr);
    // INNH-1: the same rule the page-title hop uses. The AI formats are skipped
    // (no model call) when the request's privacy block forbids a model hop.
    let ai_model_hop_allowed = quarry_runtime::page_extract::model_hop_allowed(zdr, &privacy);
    // An explicit durable-write request must not degrade into read-only
    // capture because this local runtime has no Data Plane adapter. Check
    // before cache lookup or fetching so neither can mask a missing write.
    if req.ingest.unwrap_or(false) {
        let error = if zdr.is_active() {
            Some(QuarryError::new(
                ErrorCode::BadRequest,
                "ingest cannot be combined with zero data retention",
            ))
        } else if state.ingest.is_none() {
            Some(QuarryError::new(
                ErrorCode::RuntimeNotReady,
                "durable ingestion is not configured for this runtime",
            ))
        } else {
            None
        };
        if let Some(error) = error {
            return Err((
                StatusCode::from_u16(error.code.http_status())
                    .unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
                Json(Envelope::<()>::err(&request_id, error)).into_response(),
            ));
        }
    }
    // Tenant isolation: ignore any client-supplied org_id and use the
    // verified JWT claim. The `ScrapeRequest.org_id` field is preserved
    // on the wire for backwards-compat but its value is never trusted.
    let _ = &req.org_id;
    let org_id = claims.org_id.clone();

    let url: Url = req.url.parse().map_err(|e| {
        let err = QuarryError::new(ErrorCode::BadRequest, format!("invalid url: {e}"));
        (
            StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::BAD_REQUEST),
            Json(Envelope::<()>::err(&request_id, err)).into_response(),
        )
    })?;

    // WS7.C: the web policy for this request, decided before any I/O, and the URL
    // gated before the cache is read, so a denied page is never served from it.
    let decision = crate::web_policy_edge::decide(
        &state.policy,
        &claims,
        zdr.is_active(),
        req.policy.as_ref(),
    )
    .map_err(|error| crate::web_policy_edge::error_response(&request_id, error))?;
    crate::web_policy_edge::gate_url(&decision, &req.url)
        .map_err(|error| crate::web_policy_edge::error_response(&request_id, error))?;

    let doc_routing = quarry_runtime::DocRouting::cached();
    let doc_routing_label = format!("{doc_routing:?}").to_ascii_lowercase();
    let effective = |cache_state: CacheState| {
        EffectiveScrapeOptions::new(
            &resolved,
            zdr.is_active(),
            cache_state,
            &doc_routing_label,
            parsed.strict,
            &parsed.unknown_keys,
        )
    };
    let finish = |data: quarry_core::scrape::ScrapeResponse| {
        let mut response = Json(Envelope::ok(request_id.clone(), data)).into_response();
        if !parsed.unknown_keys.is_empty() {
            for (name, value) in crate::scrape_contract::deprecation_headers(settings) {
                response.headers_mut().insert(name, value);
            }
        }
        response
    };
    let unknown_warnings = crate::scrape_contract::unknown_field_warnings(&parsed.unknown_keys);

    // The page cache holds the result of a default scrape: any other options
    // (formats, selectors, an extractor) change what a result is, so they neither
    // read nor write it. A request whose signals select the browser driver needs a
    // render the cache does not hold, so it neither reads nor writes it either. ZDR
    // never touches it (and, with the guard, cannot). The driver is chosen here, before
    // the cache is consulted, so the gate can see it.
    let plan = crate::scrape_actions::plan_for(&resolved, req.signals.unwrap_or_default());
    let cache_allowed = resolved.is_default()
        && crate::web_policy_edge::is_default(&decision)
        && plan.driver != quarry_core::output::DriverKind::Browser
        && quarry_core::zdr::guard(zdr, quarry_core::zdr::WriteKind::Cache).is_ok();
    let mut cache_state = CacheState::Bypass;
    if cache_allowed && should_read(&req.cache) {
        if let (Some(prev), Some(cache)) = (req.prev_fingerprint.as_ref(), state.cache.as_ref()) {
            cache_state = CacheState::Miss;
            if let Some(cached) = cache.get(&org_id, prev).await {
                let mut data = quarry_core::scrape::ScrapeResponse::from_output(cached);
                data.warnings = unknown_warnings;
                data.effective_options = Some(effective(CacheState::Hit));
                return Ok(finish(data));
            }
        }
    }

    // INNH-7: the URL result cache, behind QUARRY_EDGE__SCRAPE_MAX_AGE. Read when the
    // request carries `max_age`; written after every cacheable success, so a later
    // request can read it. The same gates the page cache has run first (the web
    // policy above, ZDR below); ZDR is answered before a key exists.
    let mut max_age_warnings: Vec<quarry_core::scrape::ScrapeWarning> = Vec::new();
    let inline_request = quarry_runtime::InlineRequest {
        markdown: resolved.wants(ScrapeFormat::Markdown),
        html: resolved.wants(ScrapeFormat::Html),
        raw_html: resolved.wants(ScrapeFormat::RawHtml),
        max_bytes: resolved.max_inline_bytes,
        clean: quarry_transform::clean_html::CleanOptions {
            only_main_content: resolved.only_main_content,
            include_tags: resolved.include_tags.clone(),
            exclude_tags: resolved.exclude_tags.clone(),
            remove_base64_images: resolved.remove_base64_images,
            extractor: resolved.extractor,
        },
        // INNH-1: the page text the AI formats read. One char more than the
        // model's input cap, so the engine does the cut and can tell a page that
        // is over the cap (`truncated_input`) from one of exactly the cap. Asked
        // for only when the request carries an AI format and a model hop is allowed.
        ai_source_max_chars: (!ai_specs.is_empty() && ai_model_hop_allowed).then(|| {
            crate::scrape_contract::ai_formats::settings()
                .max_input_chars
                .saturating_add(1)
        }),
    };
    let mut result_key: Option<String> = None;
    if settings.max_age_enabled {
        let eligibility = crate::scrape_cache::eligibility(&crate::scrape_cache::Request {
            resolved: &resolved,
            zdr_blocks_cache: quarry_core::zdr::guard(zdr, quarry_core::zdr::WriteKind::Cache)
                .is_err(),
            driver: plan.driver,
            default_web_policy: crate::web_policy_edge::is_default(&decision),
            privacy: &privacy,
            ingest: req.ingest.unwrap_or(false),
        });
        if eligibility == crate::scrape_cache::Eligibility::Eligible {
            result_key = state.cache.as_ref().and_then(|_| {
                crate::cache::PageCache::result_key(
                    &org_id,
                    url.as_str(),
                    &resolved.canonical_options_key(),
                )
            });
            if result_key.is_none() && resolved.max_age_ms.is_some() {
                max_age_warnings.push(crate::scrape_cache::unavailable_warning());
            }
        } else if resolved.max_age_ms.is_some() {
            max_age_warnings.extend(eligibility.warning());
        }
    }
    if let (Some(key), Some(max_age_ms), Some(cache)) =
        (result_key.as_ref(), resolved.max_age_ms, state.cache.as_ref())
    {
        if max_age_ms > 0 && should_read(&req.cache) {
            cache_state = CacheState::Miss;
            if let Some(cached) = cache.get_result(key).await {
                let now = chrono::Utc::now();
                let age_ms = crate::scrape_cache::age_ms(cached.output.fetched_at, now);
                if crate::scrape_cache::is_fresh(age_ms, max_age_ms) {
                    // A stored result whose artifacts are gone is a miss, not an error.
                    if let Some(parts) = crate::scrape_cache::rehydrate(
                        state.artifacts.as_ref(),
                        &org_id,
                        &cached,
                        &inline_request,
                    )
                    .await
                    {
                        let fetched_at = cached.output.fetched_at;
                        let mut data = quarry_core::scrape::ScrapeResponse::from_output(cached.output);
                        data.markdown = parts.markdown;
                        data.html = parts.html;
                        data.raw_html = parts.raw_html;
                        data.truncated = parts.truncated;
                        data.omitted = parts.omitted;
                        let mut warnings = unknown_warnings.clone();
                        warnings.extend(max_age_warnings);
                        warnings.extend(parts.warnings);
                        data.warnings = warnings;
                        // Not metered as a scrape.page (the page cache does not either) and
                        // no new record: `scrape_id` is absent on a hit.
                        data.effective_options =
                            Some(effective(CacheState::Hit).served_from_cache(fetched_at, age_ms));
                        return Ok(finish(data));
                    }
                }
            }
        }
    }

    // WS13.4b: a capture needs the browser. Without one, refuse with 501 rather
    // than hand the plan to the default driver, which would drop the capture.
    // WS13.5: actions need it for the same reason (the default driver would run the
    // page without them and answer 200).
    if (!resolved.capture.is_empty() || resolved.actions.is_some())
        && !state.drivers.has(quarry_core::output::DriverKind::Browser)
    {
        let error = crate::scrape_actions::browser_unavailable();
        return Err((
            StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::NOT_IMPLEMENTED),
            Json(Envelope::<()>::err(&request_id, error)).into_response(),
        ));
    }

    let driver = pick_driver(&state, &plan, req.prefer_http3);

    let ingest_client = if req.ingest.unwrap_or(false) {
        state.ingest.clone()
    } else {
        None
    };

    // WS13.5: a page an action changed is not evidence of its URL, so nothing that
    // indexes, titles, re-renders or ingests it runs for an actions request
    // (`scrape_actions::safe_mode_runner`, applied right after the runner is built).
    let runner = PageRunner {
        driver,
        security: crate::web_policy_edge::security_engine(&state.security, &state.policy, &decision),
        artifacts: state.artifacts.clone(),
        event_sink: state.event_sink.clone(),
        zdr,
        ingest: ingest_client,
        org_id: Some(org_id.clone()),
        user_id: Some(claims.actor_id().to_owned()),
        privacy: privacy.clone(),
        cancel_token: None,
        local_index: state.local_index.clone(),
        policy: quarry_runtime::RunPolicy::from(decision.policy()),
        scheduler: state.scheduler.clone(),
        autoscale: Some(quarry_runtime::global_autoscale()),
        render: {
            let mut render = req
                .render
                .clone()
                .map(RenderHints::from)
                .unwrap_or_default();
            // WS13.4b: captures ride the same render hints; a request without
            // them leaves the hints exactly as they were.
            crate::scrape_actions::apply_capture(&mut render, &resolved, settings.limits().captures);
            crate::scrape_actions::apply_actions(
                &mut render,
                &resolved,
                settings.limits().captures,
            );
            render
        },
        page_renderer: state.page_renderer.clone(),
        // Ad-hoc single-page scrape, not a crawl target — don't materialize
        // a "tracked website" row for it.
        source_registrar: None,
        title_enricher: state.page_title_enricher.clone(),
        doc_routing,
        robots: None,
    };
    let runner = crate::scrape_actions::safe_mode_runner(runner, resolved.actions.is_some());

    let run_id: RunKind = quarry_core::ids::Id::new();
    let prev_fp = req.prev_fingerprint.clone();
    let tombstone_started = chrono::Utc::now();
    // The caller's bound, never past the table's cap, so the typed Timeout
    // reaches the caller before the router's own limit drops the request.
    let run_deadline_ms =
        crate::scrape_actions::run_deadline_ms(resolved.actions.is_some(), resolved.timeout_ms);
    let run = tokio::time::timeout(
        Duration::from_millis(run_deadline_ms),
        runner.run_with_inline_request(&run_id, &url, prev_fp, &inline_request),
    )
    .await
    .unwrap_or_else(|_| {
        Err(
            QuarryError::new(ErrorCode::Timeout, "the scrape did not finish within timeout_ms")
                .with_details(crate::scrape_actions::run_timeout_details(
                    resolved.actions.is_some(),
                    run_deadline_ms,
                )),
        )
    });
    // WS4.J: a ZDR run's metadata-only tombstone (off unless the flag is on; fail open).
    let tombstone_run_id = run_id.to_string();
    crate::zdr_tombstone::emit_run_end(
        state.tombstones.as_ref(),
        zdr.is_active(),
        &match &run {
            Ok((out, _)) => crate::zdr_tombstone::RunEnd::new(
                &tombstone_run_id,
                &org_id,
                crate::zdr_tombstone::Kind::Scrape,
            )
            .started(tombstone_started)
            .driver(out.driver.kind),
            Err(err) => crate::zdr_tombstone::RunEnd::new(
                &tombstone_run_id,
                &org_id,
                crate::zdr_tombstone::Kind::Scrape,
            )
            .started(tombstone_started)
            .failed(err),
        },
    );
    match run {
        Ok((out, inline)) => {
            if cache_allowed && should_write(&req.cache) {
                if let Some(cache) = state.cache.as_ref() {
                    let ttl = effective_ttl(&req.cache, DEFAULT_TTL_SECS);
                    if let Err(e) = cache.put_with_ttl(&org_id, &out.fingerprint, &out, ttl).await {
                        // zdr-log-ok: reached only when the ZDR guard allows a cache write (cache_allowed, or !zdr.is_active()); the redis error text is what an operator needs
                        tracing::warn!(error = %e, "cache put failed");
                    }
                }
            }
            // INNH-7: the URL result cache. A key exists only for a non-ZDR, cacheable
            // request on a stack with a cache; the write honours `cache.mode` and its TTL.
            if let (Some(key), Some(cache)) = (result_key.as_ref(), state.cache.as_ref()) {
                if should_write(&req.cache) && crate::scrape_cache::is_storable(&out, &inline) {
                    let ttl = effective_ttl(&req.cache, DEFAULT_TTL_SECS);
                    let entry = crate::cache::CachedScrape {
                        output: out.clone(),
                        warnings: inline.warnings.clone(),
                    };
                    if let Err(e) = cache.put_result(key, &entry, ttl).await {
                        // zdr-log-ok: reached only with a result key, which a ZDR request never has (scrape_cache::eligibility); the redis error text is what an operator needs
                        tracing::warn!(error = %e, "scrape result cache put failed");
                    }
                }
            }
            crate::metrics::record_scrape("ok");
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
                    scrape_usage_metadata(&state.usage, &claims, &out, zdr, &privacy),
                ))
                .await;
            // INNH-1: the AI formats read the page text and answer inline only. The
            // result is never stored: not in the scrape record below, not in the
            // cache (these requests never touch it), not in an artifact or event.
            // A model failure leaves the scrape a 200 with the failure in `ai`.
            let (ai_fields, ai_report) = if ai_specs.is_empty() {
                (quarry_runtime::ai_scrape::AiFields::default(), None)
            } else {
                let (fields, report) = crate::scrape_contract::ai_formats::run_ai_formats(
                    &state,
                    &org_id,
                    &ai_specs,
                    inline.ai_source.as_deref().unwrap_or(""),
                    zdr.is_active(),
                    ai_model_hop_allowed,
                    &request_id,
                )
                .await;
                (fields, Some(report))
            };
            let mut warnings = unknown_warnings;
            warnings.extend(max_age_warnings);
            warnings.extend(inline.warnings);
            let effective_options = effective(cache_state);
            // One small record per non-ZDR scrape, so the result can be read
            // again by id. Never the inline text; never under ZDR.
            let mut scrape_id = None;
            if settings.records_enabled
                && quarry_core::zdr::guard(zdr, quarry_core::zdr::WriteKind::Artifact).is_ok()
            {
                let record = serde_json::json!({
                    "output": &out,
                    "effective_options": &effective_options,
                    "actions": &inline.actions,
                });
                let page_hash =
                    quarry_core::artifact::page_hash(&out.url.final_url, &out.fingerprint);
                let stored = match serde_json::to_vec(&record) {
                    Ok(bytes) => state
                        .artifacts
                        .put(&org_id, &run_id, &page_hash, "scrape_record", bytes)
                        .await
                        .map(|handle| handle.artifact_id.to_string()),
                    Err(error) => Err(QuarryError::new(ErrorCode::Internal, error.to_string())),
                };
                match stored {
                    Ok(id) => scrape_id = Some(id),
                    Err(_) => warnings.push(quarry_core::scrape::ScrapeWarning::new(
                        "scrape_record_unavailable",
                        "the result could not be stored for later reads; scrape_id is absent",
                    )),
                }
            }
            Ok(finish(quarry_core::scrape::ScrapeResponse {
                output: out,
                markdown: inline.markdown,
                html: inline.html,
                raw_html: inline.raw_html,
                truncated: inline.truncated,
                omitted: inline.omitted,
                warnings,
                effective_options: Some(effective_options),
                scrape_id,
                screenshot: inline.screenshot,
                pdf: inline.pdf,
                captures: inline.captures,
                actions: inline.actions,
                json: ai_fields.json,
                summary: ai_fields.summary,
                answer: ai_fields.answer,
                highlights: ai_fields.highlights,
                ai: ai_report,
            }))
        }
        Err(err) => {
            crate::metrics::record_scrape(&crate::metrics::error_label(err.code));
            // OSS-parity 3D — structured rate-limit envelope (Tavily-style)
            // on 429, matching /v1/search. Every other failure keeps the
            // canonical `{ data, meta, error }` envelope.
            if matches!(err.code, ErrorCode::RateLimited) {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(crate::api_error::ApiError::rate_limited(err.message, 60)).into_response(),
                ));
            }
            Err((
                StatusCode::from_u16(err.code.http_status())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                Json(Envelope::<()>::err(&request_id, err)).into_response(),
            ))
        }
    }
}

/// Fail closed: `/v1/crawl` and `/v1/batch` are durable by construction. The
/// job record lives in the control plane, the Temporal history carries the URLs
/// and the per-page runs go through `/v1/internal/run_page`, which the
/// orchestrator calls with no `zdr` field at all, so every page runs
/// non-ZDR (artifacts, durable events, cache, optional ingest). Running such a
/// job for a ZDR caller would silently retain what the org said must not be
/// retained, so it is refused with the reason. Lifting this needs the
/// orchestrator to carry the posture from the handoff to `run_page`.
fn refuse_durable_job_under_zdr(
    claims: &crate::auth::Claims,
    requested: Option<bool>,
    request_id: &str,
    kind: &str,
) -> Result<(), (StatusCode, Json<Envelope<()>>)> {
    if !claims.effective_zdr(requested.unwrap_or(false)) {
        return Ok(());
    }
    tracing::info!(
        org_id = %claims.org_id,
        kind,
        "durable job refused: zdr active and the durable path has no zdr support"
    );
    // Always enforced (it was before WS4.H, and is not behind
    // `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES`): the same 403, the same message,
    // plus the shared `details.zdr_refusal` with `action` = `kind` (`crawl`, `batch`).
    Err(crate::zdr_refusal::respond(
        request_id,
        crate::zdr_refusal::forbidden_with_details(
            format!(
                "ZDR active: durable {kind} jobs are not available under zero data retention \
                 (the job record and per-page results are retained)"
            ),
            kind,
        ),
    ))
}

/// A crawl or batch is not accepted while this instance is draining: the job
/// would be handed off from a process that is about to exit.
fn refuse_while_draining(request_id: &str) -> Result<(), (StatusCode, Json<Envelope<()>>)> {
    match crate::drain::refusal() {
        Some(error) => Err((
            StatusCode::from_u16(error.code.http_status())
                .unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
            Json(Envelope::<()>::err(request_id, error)),
        )),
        None => Ok(()),
    }
}

/// WS6.D-s: a refusal from the limits resolver (a spent allowance, or an unreadable one
/// under fail mode `closed`) as the response of a handoff. Nothing was created yet.
fn limits_refusal(
    request_id: &str,
    error: QuarryError,
) -> (StatusCode, Json<Envelope<()>>) {
    (
        StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
        Json(Envelope::<()>::err(request_id, error)),
    )
}

/// WS14.S6: a refused crawl request as the canonical envelope. A 400 for fields that cannot
/// be accepted; a 403 `policy_denied` for an option this request may not use. Nothing was created.
fn crawl_request_refusal(
    request_id: &str,
    refusal: crate::crawl_request::Refusal,
) -> (StatusCode, Json<Envelope<()>>) {
    let fields = crate::crawl_request::fields_json(refusal.fields());
    let (status, error) = match refusal {
        crate::crawl_request::Refusal::Invalid(_) => (
            StatusCode::BAD_REQUEST,
            QuarryError::new(ErrorCode::BadRequest, "the crawl request has invalid fields")
                .with_details(serde_json::json!({ "fields": fields })),
        ),
        crate::crawl_request::Refusal::PolicyDenied(_) => (
            StatusCode::FORBIDDEN,
            QuarryError::new(
                ErrorCode::Forbidden,
                "the crawl request asks for an option this request may not use",
            )
            .with_details(serde_json::json!({ "reason": "policy_denied", "fields": fields })),
        ),
    };
    (status, Json(Envelope::<()>::err(request_id, error)))
}

async fn crawl_handoff(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<Envelope<HandoffAck>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    let req = crate::crawl_request::parse_body(
        &body,
        crate::crawl_request::CrawlRequestSettings::current(),
    )
    .map_err(|refusal| crawl_request_refusal(&request_id, refusal))?;
    // WS14.S7a: `ignore_robots_txt: true` is decided here, where the caller's claims are, and
    // before anything else, so a refused ask is the same 403 `policy_denied` it was when the
    // body check refused it. Only a person whose org's signed policy raises the robots ceiling
    // to `ignore`, with web policy and the claim both enforced, on an org the operators
    // granted, gets it. Never a service principal (the Model) or an API key.
    let robots_override = crate::crawl_request::robots_override(
        &req,
        &claims,
        &state.policy,
        crate::crawl_request::RobotsOverrideGrants::current(),
    )
    .map_err(|refusal| crawl_request_refusal(&request_id, refusal))?;
    refuse_durable_job_under_zdr(&claims, req.zdr, &request_id, "crawl")?;
    refuse_while_draining(&request_id)?;
    // INNH-6a: a sitemap is seeded through the crawl scope. A crawl that runs with the scope
    // switch off is a legacy one, with no scope to check the sitemap's URLs against.
    if req.sitemap != crate::crawl_request::SitemapMode::Skip
        && matches!(
            crate::crawl_policy::CrawlScopeSwitch::current(),
            crate::crawl_policy::CrawlScopeSwitch::Off
        )
    {
        return Err(crawl_request_refusal(
            &request_id,
            crate::crawl_request::Refusal::Invalid(vec![crate::crawl_request::RequestField {
                path: "sitemap".to_owned(),
                code: "unsupported_option",
                message: "sitemap include and only need the crawl scope (QUARRY_EDGE__CRAWL_SCOPE_MODE observe or enforce)"
                    .to_owned(),
            }]),
        ));
    }
    // WS7.E: the seed is gated by the web policy before the handoff. A denial is a 403 and
    // the orchestrator is never called. The durable refusal above already decided ZDR, so
    // the decision is taken for a non-ZDR run.
    let decision = crate::web_policy_edge::decide(
        &state.policy,
        &claims,
        false,
        None,
    )
    .map_err(|error| crate::web_policy_edge::envelope_error(&request_id, error))?;
    crate::web_policy_edge::gate_url(&decision, &req.url)
        .map_err(|error| crate::web_policy_edge::envelope_error(&request_id, error))?;
    let org_id = claims.org_id.clone();

    // Every crawl is bounded: an absent or zero page limit takes the default
    // (zero used to mean unlimited) and a request above the ceiling is clamped.
    // The resolved values are what the orchestrator receives and what the
    // response reports, so a clamp is never silent.
    let limits = crate::limits::crawl_limits().resolve(req.max_pages, req.max_depth);
    // WS6.D-s: and never past what the organization has left of its configured page allowance.
    // A spent allowance is a 402 `org_allowance` before any job exists; an organization with
    // no allowance policy keeps the floor (the static source is always this case).
    let org_limits = state
        .limits
        .org_limits(&org_id)
        .await
        .map_err(|error| limits_refusal(&request_id, error))?;
    let limits = limits
        .clamp_pages_to(org_limits.crawl_pages)
        .map_err(|exhausted| limits_refusal(&request_id, exhausted.into_error()))?;

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
                        "max_pages": limits.max_pages.effective,
                        "org_id": org_id,
                        "user_id": claims.user_id,
                    }),
                    idem,
                )
                .await;
        }
    }

    // Phase 3+: POST to orchestrator with verified tenant identity. The job
    // carries its origin (WS15.L1) and the caller's Idempotency-Key, if sent.
    // WS14.S3: the scope this crawl runs with, stamped into the job and echoed in the ack.
    // The switch is read once at boot; `off` stamps `legacy` so the orchestrator runs it as before.
    let mut scope = crate::crawl_policy::handoff_scope(
        crate::crawl_policy::CrawlScopeSwitch::current(),
        req.scope.allow_subdomains,
        req.scope.allow_external_links,
    )
    .with_request_scope(&req.scope);
    // WS14.S7a: an allowed override runs the crawl with robots ignored (`effective.robots`).
    if robots_override.is_some() {
        scope.robots = quarry_core::crawl_scope::RobotsMode::Ignore;
    }
    let origin = crate::handoff::run_origin_from_request(&claims, &headers);
    let idempotency_key = crate::handoff::idempotency_key_from_request(&headers);
    let mut job_params = serde_json::json!({
        "kind": "crawl",
        "url": req.url,
        "scope": scope.scope_value(),
        "scope_mode": scope.scope_mode_label(),
        "robots": scope.robots_label(),
        "max_pages": limits.max_pages.effective,
        "max_depth": limits.max_depth.effective,
        "max_pages_requested": req.max_pages,
        "max_depth_requested": req.max_depth,
        "org_id": org_id,
        "user_id": claims.user_id,
        "ingest": req.ingest,
    });
    // INNH-6: stamped only when asked for, so a job without them is byte-identical to before.
    if let Some(params) = job_params.as_object_mut() {
        if req.sitemap != crate::crawl_request::SitemapMode::Skip {
            params.insert("sitemap".to_owned(), serde_json::json!(req.sitemap.wire()));
        }
        if let Some(options) = req.scrape_options.as_ref() {
            params.insert("scrape_options".to_owned(), options.to_value());
        }
        // WS14.S7a: an override job's `robots` is the object quarry-control audits before it
        // acknowledges the job (`crawl.robots_ignored`, WS14.S7b): mode, the authority and the
        // org policy rev that allowed it. `user_id` (stamped above) is the person it is
        // attributed to. Every other job keeps the `"respect"` string, byte-identical.
        if let Some(allowed) = robots_override.as_ref() {
            params.insert("robots".to_owned(), allowed.params_value());
        }
    }
    // WS7.G 6b: the policy this crawl was admitted under rides with the job to every page.
    stamp_policy_snapshot(&mut job_params, &state, &decision);
    let ack = crate::handoff::forward_job_signed(
        &state.control_base_url,
        &org_id,
        "crawl",
        crate::handoff::with_admission(job_params, &org_limits),
        &origin,
        idempotency_key.as_deref(),
        state.internal_signer.as_deref(),
    )
    .await;

    // P3 / cluster #billing — meter a crawl.seed event. The handoff
    // request is the billable unit at this surface (the orchestrator
    // independently meters per-page scrapes via /v1/internal/run_page,
    // so we avoid double-billing the page-level work here). The event is keyed by
    // Control's job id (decision 3.11), so only a job this call created is billed: a
    // refused or failed handoff (queue full included) created no job, and a replay of
    // an Idempotency-Key (Control answered 200) created nothing new.
    let billed_job = crate::handoff::billable_job_id(&ack);
    if crate::usage_attrs::corrected_rules(&*state.usage) {
        if let Some(job_id) = billed_job {
            let mut attrs = crate::usage_attrs::UsageAttrs::new(
                crate::usage_attrs::Outcome::Ok,
                crate::usage_attrs::surface_for(&claims, None),
            );
            attrs.user_id = Some(claims.user_id.clone());
            attrs.counts = Some(serde_json::json!({ "max_pages": limits.max_pages.effective }));
            crate::usage_attrs::emit(
                &*state.usage,
                &[job_id],
                &org_id,
                quarry_runtime::usage_metrics::CRAWL_SEED,
                1.0,
                &attrs,
                false,
            )
            .await;
        }
    } else if let Some(job_id) = billed_job {
        state
            .usage
            .meter(quarry_runtime::UsageEvent::new(
                job_id,
                org_id.clone(),
                quarry_runtime::usage_metrics::CRAWL_SEED,
                1.0,
                serde_json::json!({
                    "user_id": claims.user_id,
                    "url": req.url,
                    "max_pages": limits.max_pages.effective,
                }),
            ))
            .await;
    }
    match ack {
        Ok(mut v) => {
            let mut effective =
                scope.effective(limits.max_pages.effective, limits.max_depth.effective);
            if let Some(object) = effective.as_object_mut() {
                if req.sitemap != crate::crawl_request::SitemapMode::Skip {
                    object.insert("sitemap".to_owned(), serde_json::json!(req.sitemap.wire()));
                }
                if let Some(options) = req.scrape_options.as_ref() {
                    object.insert("scrape_options".to_owned(), options.to_value());
                }
                // WS14.S7a: `robots` is `"ignore"` (from the scope); the authority and rev
                // that allowed it ride beside it.
                if let Some(allowed) = robots_override.as_ref() {
                    object.insert("robots_override".to_owned(), allowed.effective_value());
                }
                object.insert(
                    "request_warnings".to_owned(),
                    serde_json::Value::Array(
                        req.warnings
                            .iter()
                            .map(|w| serde_json::json!({ "code": w.code, "field": w.field }))
                            .collect(),
                    ),
                );
            }
            v.effective = Some(effective);
            v.limits = Some(limits.into());
            Ok(Json(Envelope::ok(request_id, v)))
        }
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
    /// See [`crate::crawl_request::ParsedCrawl::zdr`].
    #[serde(default)]
    pub zdr: Option<bool>,
}

async fn batch_handoff(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    headers: HeaderMap,
    Json(req): Json<BatchRequest>,
) -> Result<Json<Envelope<HandoffAck>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    refuse_durable_job_under_zdr(&claims, req.zdr, &request_id, "batch")?;
    refuse_while_draining(&request_id)?;
    // WS14.S5: every URL is checked statically before anything is created (no network work).
    // When none can be crawled the batch is a 400 with `details.rejected`; otherwise the
    // accepted URLs go on and the refused ones come back in `HandoffAck.rejected`. The policy
    // gate below sees only the accepted URLs.
    let mut req = req;
    let check = crate::batch_check::validate_batch_urls(
        &req.urls,
        crate::batch_check::batch_max_urls(),
    );
    if check.accepted.is_empty() && !check.rejected.is_empty() {
        let err = QuarryError::new(ErrorCode::BadRequest, "no URL in the batch can be crawled")
            .with_details(serde_json::json!({ "rejected": check.rejected }));
        return Err((
            StatusCode::BAD_REQUEST,
            Json(Envelope::<()>::err(&request_id, err)),
        ));
    }
    let rejected = check.rejected;
    req.urls = check.accepted;
    // WS7.E: every URL of the batch is gated before the handoff. One denied URL refuses
    // the whole batch; the details carry a count, never a URL or a host.
    let decision = crate::web_policy_edge::decide(
        &state.policy,
        &claims,
        false,
        None,
    )
    .map_err(|error| crate::web_policy_edge::envelope_error(&request_id, error))?;
    crate::web_policy_edge::gate_urls(&decision, req.urls.iter().map(String::as_str))
        .map_err(|error| crate::web_policy_edge::envelope_error(&request_id, error))?;
    // WS6.D-s: the batch keeps the first URLs that fit the organization's remaining
    // `quarry.batch.url` allowance (all of them when none is configured); a spent allowance
    // is a 402 `org_allowance` before any job exists. The ack reports counts only.
    let org_limits = state
        .limits
        .org_limits(&claims.org_id)
        .await
        .map_err(|error| limits_refusal(&request_id, error))?;
    let urls_bound = crate::limits::clamp_batch_urls(req.urls.len(), org_limits.batch_urls)
        .map_err(|exhausted| limits_refusal(&request_id, exhausted.into_error()))?;
    req.urls.truncate(urls_bound.accepted);
    let origin = crate::handoff::run_origin_from_request(&claims, &headers);
    let idempotency_key = crate::handoff::idempotency_key_from_request(&headers);
    let mut batch_params = serde_json::json!({
        "kind": "batch",
        "urls": req.urls,
        "robots": "respect",
        "org_id": claims.org_id,
        "user_id": claims.user_id,
        "ingest": req.ingest,
    });
    // WS7.G 6b: the policy this batch was admitted under rides with the job to every page.
    stamp_policy_snapshot(&mut batch_params, &state, &decision);
    let ack = crate::handoff::forward_job_signed(
        &state.control_base_url,
        &claims.org_id,
        "batch",
        crate::handoff::with_admission(batch_params, &org_limits),
        &origin,
        idempotency_key.as_deref(),
        state.internal_signer.as_deref(),
    )
    .await;

    // P3 / cluster #billing — one batch.url event with quantity = url
    // count. Cheaper than emitting N individual events; billing-core can
    // still derive per-URL pricing via the float quantity. A submission
    // refused because the organization's queue is full created no job, so it
    // is not billed. The event is keyed by Control's job id (decision 3.11): a failed
    // handoff created no job, and a replay of an Idempotency-Key (Control answered 200)
    // created nothing new, so neither is billed.
    let billed_job = crate::handoff::billable_job_id(&ack);
    if crate::usage_attrs::corrected_rules(&*state.usage) {
        if let Some(job_id) = billed_job {
            let mut attrs = crate::usage_attrs::UsageAttrs::new(
                crate::usage_attrs::Outcome::Ok,
                crate::usage_attrs::surface_for(&claims, None),
            );
            attrs.user_id = Some(claims.user_id.clone());
            attrs.counts = Some(serde_json::json!({ "url_count": req.urls.len() }));
            crate::usage_attrs::emit(
                &*state.usage,
                &[job_id],
                &claims.org_id,
                quarry_runtime::usage_metrics::BATCH_URL,
                req.urls.len() as f64,
                &attrs,
                false,
            )
            .await;
        }
    } else if let Some(job_id) = billed_job {
        state
            .usage
            .meter(quarry_runtime::UsageEvent::new(
                job_id,
                claims.org_id.clone(),
                quarry_runtime::usage_metrics::BATCH_URL,
                req.urls.len() as f64,
                serde_json::json!({
                    "user_id": claims.user_id,
                    "url_count": req.urls.len(),
                }),
            ))
            .await;
    }
    match ack {
        Ok(mut v) => {
            v.limits = Some(urls_bound.into());
            v.rejected = rejected;
            Ok(Json(Envelope::ok(request_id, v)))
        }
        Err(err) => Err((
            StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(Envelope::<()>::err(&request_id, err)),
        )),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HandoffAck {
    pub job_id: String,
    /// Temporal workflow run id, present once the orchestrator's jobs
    /// dispatcher has flipped the job accepted → running and stamped it
    /// (see control migration 006_jobs_run_id). Absent at first handoff —
    /// the gateway therefore keys the SSE event stream off `job_id`, which
    /// control indexes events by regardless of run_id assignment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub accepted_at: chrono::DateTime<chrono::Utc>,
    /// The page and depth bounds this crawl will actually run with, and why they
    /// differ from the request (`/v1/crawl`), or the URLs a batch accepted and dropped
    /// (`/v1/batch`, WS6.D-s). Absent for other kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<crate::limits::HandoffLimits>,
    /// WS14.S3: the scope and bounds a crawl runs with (`crawl` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective: Option<serde_json::Value>,
    /// WS14.S5: the URLs of a batch that the static check refused (`/v1/batch`). Absent
    /// when none was refused. The URLs are the caller's own; no host or reason detail is added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected: Vec<crate::batch_check::BatchRejection>,
    /// True when Control answered a repeated `Idempotency-Key` with the job it already held
    /// (HTTP 200 instead of 201). Not on the wire: it only decides whether the call is billed.
    #[serde(skip)]
    pub replayed: bool,
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
    /// WS14.S2: the crawl scope for this page's links. Absent (or null) leaves the
    /// response unchanged. Kept as a raw JSON value so a malformed object is a 400
    /// that names its fields, not a rejection of the whole body.
    #[serde(default)]
    pub crawl: Option<serde_json::Value>,
    /// WS6.C-e: the job kind the orchestrator fetched this page for (`crawl`, `batch`, or absent).
    /// Only a crawl page is metered (`quarry.crawl.page`).
    #[serde(default)]
    pub kind: Option<String>,
    /// INNH-6b: the crawl's checked `scrape_options` (`crawl_scrape::CrawlScrapeOptions`).
    /// Raw JSON, re-validated here. Absent (or null) leaves the page run and the response
    /// exactly as they were. Honoured only with `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS` on; with
    /// the flag off the field is ignored and the response lacks `scrape_applied`.
    #[serde(default)]
    pub scrape: Option<serde_json::Value>,
    /// WS7.G 6b: the web policy the handoff resolved for the durable job
    /// (`ResolvedPolicy::snapshot_json`), stamped on the job's params and carried by every page.
    /// Raw JSON, re-validated here. Its sha256 is part of the signed run-binding string, so it
    /// cannot be added, swapped or stripped without the shared secret. Honoured only with
    /// `QUARRY_EDGE__WEB_POLICY_SNAPSHOT` at `enforce` (counted at `observe`); with the flag off
    /// the field is ignored. Absent (or null) leaves the page run exactly as it was.
    #[serde(default)]
    pub policy_snapshot: Option<serde_json::Value>,
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
    /// consumers polling `/v1/jobs/{id}/events` (e.g. verevon's
    /// onboarding wizard) see real brand signals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branding: Option<serde_json::Value>,
    /// Display title after provenance resolution (`html` → cleaned
    /// `<title>`, `model` → Model Plane proposal, `host` → host label).
    /// `title` above stays the raw HTML title for backward compatibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
    /// `html` | `model` | `host` — see `quarry_core::output::TitleSource`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_source: Option<String>,
    /// ~300-char plain-text excerpt of the readable markdown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    /// One-sentence model summary (only with `title_source == model`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub word_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Driver that served the fetch: `static` | `tls` | `browser`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    /// WS14.S2: set only when the request carried `crawl`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crawl: Option<quarry_core::crawl_scope::CrawlReport>,
    /// INNH-4: id of the stored markdown artifact of this page (org-bound, readable through
    /// `GET /v1/artifacts/{id}`). General, not change-specific: the change monitor keeps it
    /// on a baseline, and the per-run results of a crawl or batch can list it (INNH-12).
    /// Absent when nothing was stored, always so under ZDR. An older runtime omits it, and
    /// so does every response when no page artifact was written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown_artifact_id: Option<String>,
    /// INNH-12: id of the stored source of this page (`html`, or `raw` for a document), read
    /// through the same org-bound `GET /v1/artifacts/{id}`. Absent whenever the markdown id is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_artifact_id: Option<String>,
    /// INNH-12: size in bytes of the stored markdown. Absent whenever the markdown id is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// INNH-6b: `true` when the request's `scrape` options were applied to this page. Absent
    /// when the request carried none. An orchestrator that sent options and does not see this
    /// ends the run (an edge that ignores the field would crawl with the default cleaning).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrape_applied: Option<bool>,
}

impl InternalRunPageResult {
    /// Project a `NormalizedOutput` onto the orchestrator wire shape. The
    /// orchestrator forwards `title`/`branding` as `page_fetched` /
    /// `branding_extracted` and the extraction fields as `page_extracted`
    /// into control's per-job event log.
    pub fn from_output(run_id: String, out: &NormalizedOutput) -> Self {
        let ex = out.extraction.as_ref();
        Self {
            run_id,
            status: out.status,
            fingerprint: out.fingerprint.clone(),
            links: out.formats.links.iter().map(|l| l.href.clone()).collect(),
            content_type: out.metadata.content_type.clone(),
            title: out.metadata.title.clone(),
            branding: out.branding.clone(),
            display_title: ex.map(|e| e.title.clone()),
            title_source: ex.map(|e| e.title_source.as_str().to_string()),
            excerpt: ex.map(|e| e.excerpt.clone()),
            summary: ex.and_then(|e| e.summary.clone()),
            word_count: ex.map(|e| e.word_count),
            lang: ex.and_then(|e| e.lang.clone()),
            driver: ex.map(|e| {
                serde_json::to_value(e.driver)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default()
            }),
            crawl: None,
            markdown_artifact_id: out
                .formats
                .markdown
                .as_ref()
                .map(|format| format.artifact_id.to_string()),
            source_artifact_id: out
                .formats
                .html
                .as_ref()
                .or(out.formats.raw.as_ref())
                .map(|format| format.artifact_id.to_string()),
            bytes: out.formats.markdown.as_ref().map(|format| format.bytes),
            scrape_applied: None,
        }
    }
}

/// WS14.S2: the `crawl` object, checked and compiled before anything is fetched.
/// A malformed one is a 400 whose `details.fields` names each rejected path, the
/// same shape as `contract_error`. Absent or null means no scope.
fn compile_crawl(
    raw: Option<serde_json::Value>,
    request_id: &str,
) -> Result<Option<quarry_runtime::crawl_filter::CompiledScope>, (StatusCode, Json<Envelope<()>>)>
{
    let Some(raw) = raw.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let compiled = quarry_core::crawl_scope::CrawlContext::from_json(raw).and_then(|ctx| {
        quarry_runtime::crawl_filter::CompiledScope::compile(&ctx)
    });
    match compiled {
        Ok(scope) => Ok(Some(scope)),
        Err(errs) => {
            let fields: Vec<serde_json::Value> = errs
                .iter()
                .map(|e| {
                    serde_json::json!({ "path": e.field, "code": "invalid", "message": e.message })
                })
                .collect();
            let err = QuarryError::new(ErrorCode::BadRequest, "the crawl scope is invalid")
                .with_details(serde_json::json!({ "fields": fields }));
            Err((
                StatusCode::BAD_REQUEST,
                Json(Envelope::<()>::err(request_id, err)),
            ))
        }
    }
}

/// INNH-6b: mark whether the request's `scrape` options were applied to this page.
fn with_scrape_applied(
    mut result: InternalRunPageResult,
    applied: Option<bool>,
) -> InternalRunPageResult {
    result.scrape_applied = applied;
    result
}

/// WS14.S2: when the request carried `crawl`, replace the page's links with the
/// scope's allowed links and attach the report. Without a scope the result is
/// returned exactly as built, so the response is unchanged for existing callers.
fn with_crawl(
    mut result: InternalRunPageResult,
    final_url: &str,
    scope: Option<&quarry_runtime::crawl_filter::CompiledScope>,
    robots: Option<&quarry_runtime::RobotsGate>,
) -> InternalRunPageResult {
    let (Some(scope), Ok(page)) = (scope, Url::parse(final_url)) else {
        return result;
    };
    // WS14.S4: cached robots rules filter the page's links, never a network call, and
    // only when the gate enforces; in observe mode no link is dropped on robots.txt's
    // word. The page's `Crawl-delay` is reported in every mode but off.
    let peek = |link: &Url| robots.map_or(quarry_runtime::crawl_filter::RobotsPeek::Unknown, |g| g.peek(link));
    let lookup: Option<quarry_runtime::crawl_filter::RobotsLookup<'_>> = robots
        .filter(|g| g.mode() == quarry_runtime::RobotsEnforceMode::Enforce)
        .map(|_| &peek as quarry_runtime::crawl_filter::RobotsLookup<'_>);
    let mut outcome = scope.evaluate(&page, &result.links, lookup);
    if let Some(gate) = robots {
        outcome.report.crawl_delay_ms = gate.crawl_delay_ms(&page);
        // WS14.S4, D2-091: the links the robots decision removed, counted only when the gate
        // enforces. Observe and off remove no link, so they count none.
        if gate.mode() == quarry_runtime::RobotsEnforceMode::Enforce {
            crate::metrics::record_robots_link_refusals(&outcome.report);
        }
    }
    result.links = outcome.links;
    result.crawl = Some(outcome.report);
    result
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
pub(crate) fn verify_runtime_token(headers: &HeaderMap) -> Result<(), QuarryError> {
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
    if !provided.is_empty() && crate::change_routes::ct_eq(provided.as_bytes(), expected.as_bytes())
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

/// WS7.G 6b: stamp the policy a durable job's handoff resolved onto the job's params, for the
/// orchestrator to carry to every page run. Stamped only with `QUARRY_EDGE__WEB_POLICY_SNAPSHOT`
/// at `observe` or `enforce`, and only when the policy differs from the catalogue default (a
/// default policy carries nothing a page does not already apply), so a job without a tightening
/// is byte-identical to one built before the snapshot existed.
///
/// The policy stamped is the one `enforce` would apply (`shadow` under `observe`): the page's
/// own web-policy enforcement mode decides whether it binds.
fn stamp_policy_snapshot(
    params: &mut serde_json::Value,
    state: &AppState,
    decision: &crate::policy_baseline::Decision,
) {
    use crate::policy_baseline::Enforcement;
    if state.policy.snapshot_mode() == Enforcement::Off {
        return;
    }
    let policy = decision.shadow().unwrap_or_else(|| decision.policy());
    if quarry_runtime::RunPolicy::from(policy).web_policy.is_none() {
        return;
    }
    if let Some(object) = params.as_object_mut() {
        object.insert("policy_snapshot".to_owned(), policy.snapshot_json());
    }
}

/// WS7.G 6b: meet the page's decision with the job's policy snapshot. Returns the decision the
/// page runs under and whether the snapshot bound it.
///
/// * flag `off`, or no snapshot: the decision is returned unchanged;
/// * `observe`: the decision is returned unchanged; pages the snapshot would tighten, and pages
///   it would refuse, are counted (`web_policy_snapshot_tighter_total`,
///   `web_policy_snapshot_would_deny_total`; no label, no URL);
/// * `enforce`: the decision is resolved again with the snapshot as the request layer, so the
///   page can only be as restrictive as, or more than, the handoff's policy.
///
/// A snapshot the current version cannot read is a 400 in `observe` and `enforce`, never a
/// silent run under a looser policy.
fn apply_policy_snapshot(
    state: &AppState,
    decision: crate::policy_baseline::Decision,
    snapshot: Option<&serde_json::Value>,
    zdr: bool,
    url: &str,
    request_id: &str,
) -> Result<(crate::policy_baseline::Decision, bool), (StatusCode, Json<Envelope<()>>)> {
    use crate::policy_baseline::Enforcement;
    use quarry_core::web_policy::{OrgInput, PolicyLayer, Principal};
    let Some(raw) = snapshot.filter(|value| !value.is_null()) else {
        return Ok((decision, false));
    };
    let mode = state.policy.snapshot_mode();
    if mode == Enforcement::Off {
        tracing::warn!(
            target: "quarry.web_policy",
            "run_page policy snapshot ignored: QUARRY_EDGE__WEB_POLICY_SNAPSHOT is off"
        );
        return Ok((decision, false));
    }
    let layer = PolicyLayer::from_snapshot_json(raw).map_err(|_| {
        let err = QuarryError::new(ErrorCode::BadRequest, "the policy snapshot is invalid")
            .with_details(serde_json::json!({ "reason": "invalid_policy_snapshot", "retryable": false }));
        (
            StatusCode::BAD_REQUEST,
            Json(Envelope::<()>::err(request_id, err)),
        )
    })?;
    let met = state
        .policy
        .decide_with_org(Principal::Autonomous, zdr, OrgInput::Absent, Some(layer))
        .map_err(|denial| {
            crate::web_policy_edge::record_denial(&denial, true);
            crate::web_policy_edge::envelope_error(request_id, crate::web_policy_edge::quarry_error(&denial))
        })?;
    if mode == Enforcement::Enforce {
        return Ok((met, true));
    }
    // observe: what `enforce` would have decided, beside what the page decided without it.
    let with_snapshot = met.shadow().unwrap_or_else(|| met.policy());
    let without = decision.shadow().unwrap_or_else(|| decision.policy());
    if with_snapshot.digest() != without.digest() {
        metrics::counter!("web_policy_snapshot_tighter_total").increment(1);
        if with_snapshot.check_url(url).is_err() && without.check_url(url).is_ok() {
            metrics::counter!("web_policy_snapshot_would_deny_total").increment(1);
            tracing::info!(target: "quarry.web_policy", "policy.snapshot_would_deny");
        }
    }
    Ok((decision, false))
}

async fn internal_run_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<InternalRunPage>,
) -> Result<Json<InternalRunPageResult>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
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
        let err = QuarryError::new(
            ErrorCode::BadRequest,
            "run_page requires org_id in the body",
        );
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
        // WS7.G 6b: a page that carries a policy snapshot signs its hash too, whatever the
        // snapshot flag says, so the binding holds before anything decides to use it.
        // WS14.S7a: and a page whose crawl context does not respect robots.txt signs the hash of
        // its `crawl.robots`, so a captured respecting page cannot be replayed as ignoring.
        let extras = crate::internal_auth::RunBindingExtras::for_page(
            req.policy_snapshot.as_ref(),
            req.crawl.as_ref(),
        );
        if !signer.verify_run_binding_with(&org_id, &req.url, &extras, sig) {
            tracing::warn!(
                had_sig = !sig.is_empty(),
                "run_page HMAC org-binding failed"
            );
            let err = QuarryError::new(ErrorCode::Unauthorized, "run_page org binding invalid");
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

    // WS14.S2: a malformed `crawl` is refused before anything is fetched.
    let crawl_scope = compile_crawl(req.crawl.clone(), &request_id)?;

    // INNH-6b: the crawl's per-page options, re-checked here with the rules the handoff used.
    // Off (the default), the field is ignored and the response says nothing of it, so an
    // orchestrator that sent options sees they were not applied and ends the run.
    let scrape_options = match req.scrape.as_ref().filter(|value| !value.is_null()) {
        Some(raw) if crate::crawl_policy::CrawlFeatures::current().scrape_options => {
            Some(
                crate::crawl_scrape::CrawlScrapeOptions::from_stamped(raw).map_err(|errors| {
                    let fields: Vec<serde_json::Value> = errors
                        .iter()
                        .map(|e| {
                            serde_json::json!({
                                "path": format!("scrape.{}", e.path),
                                "code": e.code,
                                "message": e.message,
                            })
                        })
                        .collect();
                    let err = QuarryError::new(ErrorCode::BadRequest, "the scrape options are invalid")
                        .with_details(serde_json::json!({ "fields": fields }));
                    (
                        StatusCode::BAD_REQUEST,
                        Json(Envelope::<()>::err(&request_id, err)),
                    )
                })?,
            )
        }
        Some(_) => {
            tracing::warn!(
                "run_page scrape options ignored: QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS is off"
            );
            None
        }
        None => None,
    };
    let scrape_applied = scrape_options.as_ref().map(|_| true);
    // The page cache is keyed by the page's fingerprint alone: a page cleaned differently from
    // the default must neither read it nor write it.
    let scrape_cache_ok = scrape_options
        .as_ref()
        .map_or(true, crate::crawl_scrape::CrawlScrapeOptions::cache_admissible);

    // WS7.C: no JWT on this route. The caller is the orchestrator, an autonomous
    // principal, so the decision is the baseline and the invariants (no ask).
    let decision = crate::web_policy_edge::decide_for_principal(
        &state.policy,
        quarry_core::web_policy::Principal::Autonomous,
        zdr.is_active(),
        None,
    )
    .map_err(|error| crate::web_policy_edge::envelope_error(&request_id, error))?;
    // WS7.G 6b: the policy the handoff resolved meets this page's own (tighten-only). Under
    // `observe` the page runs as before and the tightening is counted; under `enforce` the
    // page runs under the met policy. A tightened page is not a default page, so it neither
    // reads nor writes the page cache.
    let (decision, snapshot_tightened) =
        apply_policy_snapshot(&state, decision, req.policy_snapshot.as_ref(), zdr.is_active(), &req.url, &request_id)?;
    crate::web_policy_edge::gate_url(&decision, &req.url)
        .map_err(|error| crate::web_policy_edge::envelope_error(&request_id, error))?;

    let plan = plan_from_signals(req.signals.unwrap_or_default());
    let driver = pick_driver(&state, &plan, req.prefer_http3);

    let ingest_client = if req.ingest.unwrap_or(false) {
        state.ingest.clone()
    } else {
        None
    };

    let runner = PageRunner {
        driver,
        security: crate::web_policy_edge::security_engine(&state.security, &state.policy, &decision),
        artifacts: state.artifacts.clone(),
        event_sink: state.event_sink.clone(),
        zdr,
        ingest: ingest_client,
        org_id: Some(org_id.clone()),
        user_id: req.user_id.clone(),
        privacy,
        cancel_token: None,
        local_index: state.local_index.clone(),
        policy: quarry_runtime::RunPolicy::from(decision.policy()),
        scheduler: state.scheduler.clone(),
        autoscale: Some(quarry_runtime::global_autoscale()),
        render: req
            .render
            .clone()
            .map(RenderHints::from)
            .unwrap_or_default(),
        page_renderer: state.page_renderer.clone(),
        // P0 fix — Aquatiq crawl-to-KB pipeline (2026-07-20): this is the
        // orchestrator-driven per-page route used for durable crawl/batch
        // execution (quarry-orchestrator calls it once per page). Wiring the
        // registrar HERE — and only here, not on the ad-hoc /v1/scrape
        // paths — is what makes crawl-originated ingests materialize a
        // "tracked website" Source row.
        source_registrar: Some(std::sync::Arc::new(
            crate::source_registrar::EdgeSourceRegistrar {
                state: state.clone(),
            },
        )),
        title_enricher: state.page_title_enricher.clone(),
        doc_routing: quarry_runtime::DocRouting::cached(),
        // WS14.S4: a crawl or batch page is gated on robots.txt when its context says
        // `robots: respect`, the gate exists and the mode is not off.
        robots: match (&state.robots, crawl_scope.as_ref()) {
            (Some(gate), Some(scope)) if scope.respects_robots() => Some(
                quarry_runtime::RobotsEnforcement::new(gate.clone(), gate.mode()),
            ),
            _ => None,
        },
    };

    let run_id: RunKind = match req.run_id.as_deref() {
        Some(s) => s.parse().unwrap_or_else(|_| quarry_core::ids::Id::new()),
        None => quarry_core::ids::Id::new(),
    };

    let snapshot_cache_ok = !(snapshot_tightened && !crate::web_policy_edge::is_default(&decision));
    if !zdr.is_active() && scrape_cache_ok && snapshot_cache_ok && should_read(&req.cache) {
        if let (Some(prev), Some(cache)) = (req.prev_fingerprint.as_ref(), state.cache.as_ref()) {
            if let Some(cached) = cache.get(&org_id, prev).await {
                // WS14.S4, D2-161: a cached page gets the robots.txt decision that a fetched
                // page gets, before it is served or metered. A refusal is the fetch refusal.
                if let Err(err) = runner.precheck_robots(&run_id, &url).await {
                    return Err((
                        StatusCode::from_u16(err.code.http_status())
                            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        Json(Envelope::<()>::err(&request_id, err)),
                    ));
                }
                meter_crawl_page(&state, req.kind.as_deref(), &req.url, req.user_id.as_deref(), &run_id, &org_id, zdr).await;
                return Ok(Json(with_crawl(
                    with_scrape_applied(
                        InternalRunPageResult::from_output(run_id.to_string(), &cached),
                        scrape_applied,
                    ),
                    &cached.url.final_url,
                    crawl_scope.as_ref(),
                    state.robots.as_deref(),
                )));
            }
        }
    }

    let prev_fp = req.prev_fingerprint.clone();
    // INNH-6b: with options, the page is cleaned with them (the stored markdown and what an
    // ingest sends on are the cleaned text) and bounded by `timeout_ms` when one was given.
    let run_result = match scrape_options.as_ref() {
        None => runner.run(&run_id, &url, prev_fp).await,
        Some(options) => {
            let inline_request = quarry_runtime::InlineRequest {
                clean: options.clean_options(),
                ..quarry_runtime::InlineRequest::default_for(zdr)
            };
            let run = runner.run_with_inline_request(&run_id, &url, prev_fp, &inline_request);
            let finished = match options.timeout_ms {
                Some(ms) => tokio::time::timeout(Duration::from_millis(ms), run)
                    .await
                    .unwrap_or_else(|_| {
                        Err(QuarryError::new(
                            ErrorCode::Timeout,
                            "the page did not finish within scrape_options.timeout_ms",
                        ))
                    }),
                None => run.await,
            };
            finished.map(|(output, _inline)| output)
        }
    };
    match run_result {
        Ok(out) => {
            if !zdr.is_active()
                && scrape_cache_ok
                && crate::web_policy_edge::is_default(&decision)
                && should_write(&req.cache)
            {
                if let Some(cache) = state.cache.as_ref() {
                    let ttl = effective_ttl(&req.cache, DEFAULT_TTL_SECS);
                    if let Err(e) = cache.put_with_ttl(&org_id, &out.fingerprint, &out, ttl).await {
                        // zdr-log-ok: reached only when the ZDR guard allows a cache write (cache_allowed, or !zdr.is_active()); the redis error text is what an operator needs
                        tracing::warn!(error = %e, "cache put failed");
                    }
                }
            }
            meter_crawl_page(&state, req.kind.as_deref(), &req.url, req.user_id.as_deref(), &run_id, &org_id, zdr).await;
            Ok(Json(with_crawl(
                with_scrape_applied(
                    InternalRunPageResult::from_output(run_id.to_string(), &out),
                    scrape_applied,
                ),
                &out.url.final_url,
                crawl_scope.as_ref(),
                state.robots.as_deref(),
            )))
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

/// WS6.C-e: one `quarry.crawl.page` event for a page the orchestrator fetched for a crawl, off
/// ZDR. The id is keyed by the run and a short hash of the page URL, so a retried page activity
/// adds no second row. A batch page and a single scrape are not metered here.
async fn meter_crawl_page(
    state: &AppState,
    kind: Option<&str>,
    url: &str,
    user_id: Option<&str>,
    run_id: &RunKind,
    org_id: &str,
    zdr: ZdrMode,
) {
    use sha2::{Digest, Sha256};
    if zdr.is_active() || kind != Some("crawl") {
        return;
    }
    let digest = format!("{:x}", Sha256::digest(url.as_bytes()));
    let page = &digest[..16];
    let run = run_id.to_string();
    let mut attrs = crate::usage_attrs::UsageAttrs::new(
        crate::usage_attrs::Outcome::Ok,
        crate::usage_attrs::Surface::Internal,
    );
    attrs.user_id = user_id.map(str::to_owned);
    crate::usage_attrs::emit(
        &*state.usage,
        &[run.as_str(), page],
        org_id,
        quarry_runtime::usage_metrics::CRAWL_PAGE,
        1.0,
        &attrs,
        false,
    )
    .await;
}

async fn scrape_stream(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<ScrapeRequest>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let zdr = claims.effective_zdr_mode(req.zdr);
    let privacy = effective_privacy(req.privacy.clone(), zdr);
    // Tenant enforcement: claim wins over any client-supplied org_id.
    let _ = &req.org_id;
    let org_id = claims.org_id.clone();
    // The request span, taken here and handed to the runner's task below:
    // `tokio::spawn` does not inherit the caller's span, so without this the
    // runner would log without the ZDR mark and the event filter could not see it.
    let request_span = tracing::Span::current();
    // WS7.C: the policy is decided before the stream starts. A refusal is the
    // stream's first event (an `error` event carrying the 403 or 400 envelope).
    let request_id = crate::request_id::current();
    let decision = crate::web_policy_edge::decide(
        &state.policy,
        &claims,
        zdr.is_active(),
        req.policy.as_ref(),
    );
    let stream = async_stream::stream! {
        let url = match req.url.parse::<Url>() {
            Ok(u) => u,
            Err(e) => {
                yield Ok(Event::default().event("error").data(e.to_string()));
                return;
            }
        };
        // WS13.5: the stream runs no actions and keeps no run. Refused, not dropped.
        if req.actions.as_ref().is_some_and(|value| !value.is_null()) {
            let error = QuarryError::new(
                ErrorCode::BadRequest,
                "scrape actions are not served on the stream route",
            )
            .with_details(serde_json::json!({ "reason": "unsupported_in_stream" }));
            yield Ok(Event::default().event("error").data(serde_json::to_string(&error).unwrap_or_default()));
            return;
        }
        // WS13.4b: the stream serves no captures. Refused, not dropped.
        if crate::scrape_actions::requests_capture(req.formats.as_ref()) {
            let error = QuarryError::new(
                ErrorCode::BadRequest,
                "screenshot and pdf formats are not served on the stream route",
            )
            .with_details(serde_json::json!({ "reason": "unsupported_in_stream" }));
            yield Ok(Event::default().event("error").data(serde_json::to_string(&error).unwrap_or_default()));
            return;
        }
        // INNH-1: the stream serves no AI formats. Refused whatever the switch says,
        // before any fetch, so a caller learns it is the route and not the flag.
        if crate::scrape_contract::ai_formats::requests_ai_format(req.formats.as_ref()) {
            let error = QuarryError::new(
                ErrorCode::BadRequest,
                "AI formats are not served on the stream route",
            )
            .with_details(serde_json::json!({ "reason": "unsupported_in_stream" }));
            yield Ok(Event::default().event("error").data(serde_json::to_string(&error).unwrap_or_default()));
            return;
        }
        let decision = match decision {
            Ok(decision) => decision,
            Err(error) => {
                yield Ok(Event::default().event("error").data(crate::web_policy_edge::stream_error_data(&request_id, error)));
                return;
            }
        };
        if let Err(error) = crate::web_policy_edge::gate_url(&decision, req.url.as_str()) {
            yield Ok(Event::default().event("error").data(crate::web_policy_edge::stream_error_data(&request_id, error)));
            return;
        }
        let cache_default = crate::web_policy_edge::is_default(&decision);
        if !zdr.is_active() && cache_default && should_read(&req.cache) {
            if let (Some(prev), Some(cache)) = (req.prev_fingerprint.as_ref(), state.cache.as_ref()) {
                if let Some(cached) = cache.get(&org_id, prev).await {
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
            security: crate::web_policy_edge::security_engine(&state.security, &state.policy, &decision),
            artifacts: state.artifacts.clone(),
            event_sink: state.event_sink.clone(),
            zdr,
            ingest: ingest_client,
            org_id: Some(org_id.clone()),
            user_id: Some(claims.user_id.clone()),
            privacy: privacy.clone(),
            cancel_token: None,
            local_index: state.local_index.clone(),
            policy: quarry_runtime::RunPolicy::from(decision.policy()),
            scheduler: state.scheduler.clone(),
            autoscale: Some(quarry_runtime::global_autoscale()),
            render: req.render.clone().map(RenderHints::from).unwrap_or_default(),
            page_renderer: state.page_renderer.clone(),
            // Ad-hoc streamed single-page scrape, not a crawl target — don't
            // materialize a "tracked website" row for it.
            source_registrar: None,
            title_enricher: state.page_title_enricher.clone(),
            doc_routing: quarry_runtime::DocRouting::cached(),
            robots: None,
        };
        let run_id = RunKind::new();
        let mut rx = state.event_sink.subscribe(&run_id);

        // Spawn the runner; forward broadcast events as they arrive.
        let runner_run_id = run_id.clone();
        let runner_url = url.clone();
        let prev_fp = req.prev_fingerprint.clone();
        let cache = state.cache.clone();
        let cache_org_id = org_id.clone();
        let policy = req.cache.clone();
        let tombstones = state.tombstones.clone();
        let tombstone_started = chrono::Utc::now();
        let runner_handle = tokio::spawn(async move {
            let res = runner.run(&runner_run_id, &runner_url, prev_fp).await;
            // WS4.J: a ZDR run's metadata-only tombstone (off unless the flag is on; fail open).
            let tombstone_run_id = runner_run_id.to_string();
            crate::zdr_tombstone::emit_run_end(
                tombstones.as_ref(),
                zdr.is_active(),
                &match &res {
                    Ok(out) => crate::zdr_tombstone::RunEnd::new(
                        &tombstone_run_id,
                        &cache_org_id,
                        crate::zdr_tombstone::Kind::Scrape,
                    )
                    .started(tombstone_started)
                    .driver(out.driver.kind),
                    Err(err) => crate::zdr_tombstone::RunEnd::new(
                        &tombstone_run_id,
                        &cache_org_id,
                        crate::zdr_tombstone::Kind::Scrape,
                    )
                    .started(tombstone_started)
                    .failed(err),
                },
            );
            if let Ok(ref out) = res {
                if !zdr.is_active() && cache_default && should_write(&policy) {
                    if let Some(c) = cache.as_ref() {
                        let ttl = effective_ttl(&policy, DEFAULT_TTL_SECS);
                        if let Err(e) = c.put_with_ttl(&cache_org_id, &out.fingerprint, out, ttl).await {
                            // zdr-log-ok: reached only under !zdr.is_active(); the redis error text is what an operator needs
                            tracing::warn!(error = %e, "cache put failed");
                        }
                    }
                }
            }
            res
        }.instrument(request_span));

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

/// Metadata stored with a `scrape.page` usage event. Under ZDR the event
/// still bills and audits (status, `zdr: true`, privacy posture) but names
/// neither the page nor its content fingerprint: billing-core persists
/// metadata, so a URL here is a durable copy of what ZDR forbids keeping.
/// The scrape event's metadata. Off `on` it is the legacy shape, unchanged. From `on` it is the
/// closed set of `usage_attrs` (no URL, fingerprint or privacy fields, under ZDR or not).
fn scrape_usage_metadata(
    meter: &Arc<dyn quarry_runtime::UsageMeter>,
    claims: &crate::auth::Claims,
    out: &NormalizedOutput,
    zdr: ZdrMode,
    privacy: &PrivacyPolicy,
) -> serde_json::Value {
    if !crate::usage_attrs::corrected_rules(&**meter) {
        return scrape_meter_metadata(&claims.user_id, out, zdr, privacy);
    }
    let mut attrs = crate::usage_attrs::UsageAttrs::new(
        crate::usage_attrs::Outcome::Ok,
        crate::usage_attrs::surface_for(claims, None),
    );
    attrs.user_id = Some(claims.user_id.clone());
    crate::usage_attrs::metadata(&attrs, zdr.is_active())
}

fn scrape_meter_metadata(
    user_id: &str,
    out: &NormalizedOutput,
    zdr: ZdrMode,
    privacy: &PrivacyPolicy,
) -> serde_json::Value {
    let mut meta = serde_json::json!({
        "user_id": user_id,
        "status": out.status,
        "zdr": zdr.is_active(),
        "purpose_id": privacy.purpose_id,
        "privacy_classification": privacy.privacy_classification,
        "allow_third_party_processing": privacy.allow_third_party_processing,
    });
    if !zdr.is_active() {
        meta["url"] = serde_json::json!(out.url.final_url);
        meta["fingerprint"] = serde_json::json!(out.fingerprint);
    }
    meta
}

fn effective_privacy(privacy: Option<PrivacyPolicy>, zdr: ZdrMode) -> PrivacyPolicy {
    privacy.unwrap_or_default().with_zdr(zdr)
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::response::IntoResponse;

    use async_trait::async_trait;
    use quarry_core::error::{ErrorCode, QuarryError};
    use quarry_core::output::DriverKind;
    use quarry_core::QuarryResult;
    use quarry_runtime::artifact_store::{ArtifactStore, InMemoryStore};
    use quarry_runtime::driver::{Driver, FetchHints};
    use quarry_runtime::driver_registry::DriverRegistry;
    use quarry_runtime::fetch::FetchResponse;
    use quarry_runtime::{EventSink, NoopUsageMeter};
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
                    served_by: self.kind,
                    captures: None,
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
            readiness: crate::state::ReadinessState {
                durable: true,
                reason: None,
            },
            receipts: Arc::new(quarry_runtime::InMemoryStepReceiptStore::new()),
            grant_validator: Arc::new(quarry_runtime::NoopGrantValidator),
            require_browser_grants: false,
            zdr_refuse_durable_resources: false,
            profile_owner_enforce: crate::profile_guard::OwnerEnforce::Observe,
            org_erasables: Vec::new(),
            driver: static_driver,
            drivers,
            http3,
            security: Arc::new(quarry_security::preflight::DefaultEngine::new()),
            artifacts,
            proxy_pool_name: String::new(),
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
            page_title_enricher: None,
            service_token_provider: None,
            answer_pipeline: None,
            answer_status: crate::answer_status::AnswerPipelineStatus::SynthesisDisabled,
            model_plane: Default::default(),
            local_index: None,
            usage: Arc::new(NoopUsageMeter),
            limits: Arc::new(crate::limits::StaticFloor),
            policy: crate::policy_baseline::PolicyBaseline::default(),
            robots: None,
            scheduler: None,
            internal_signer: None,
            tombstones: None,
            page_renderer: None,
            visual_processor: None,
            #[cfg(feature = "postgres-queue")]
            event_history: None,
            #[cfg(feature = "postgres-queue")]
            queue_pool: None,
            #[cfg(feature = "postgres-queue")]
            baseline_store: None,
            #[cfg(feature = "browser-agent")]
            agent_driver: Arc::new(quarry_browser::chromiumoxide::ChromiumoxideDriver::new()),
            #[cfg(feature = "browser-agent")]
            browser_egress_proxy: None,
            #[cfg(feature = "browser-agent")]
            agent_runs: crate::agent_routes::new_runs(),
            #[cfg(feature = "browser-agent")]
            fleets: crate::fleet_routes::new_fleet_state(),
        }
    }

    fn claims_for_org(org_id: &str) -> crate::auth::Claims {
        crate::auth::Claims {
            web_policy: None,
            sub: "u1".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: org_id.into(),
            user_id: "u1".into(),
            principal_type: None,
            service_id: None,
            nbf: None,
            aud: None,
            scopes: Vec::new(),
            zdr: None,
            key_id: None,
            acting_user_id: None,
        }
    }

    async fn stored_screenshot(
        artifacts: &InMemoryStore,
        org_id: &str,
    ) -> quarry_runtime::artifact_store::ArtifactHandle {
        artifacts
            .put(
                org_id,
                &RunKind::new(),
                "blake3:browser-frame",
                "screenshot",
                b"png bytes".to_vec(),
            )
            .await
            .expect("stored artifact")
    }

    #[tokio::test]
    async fn artifact_route_returns_stored_bytes() {
        let calls = Arc::new(AtomicU32::new(0));
        let static_driver = Arc::new(TestDriver::ok(DriverKind::Static, calls, b"unused"));
        let drivers = DriverRegistry::new(DriverKind::Static);
        let artifacts = Arc::new(InMemoryStore::new());
        let handle = stored_screenshot(&artifacts, "org_alpha").await;
        let state = test_state_with_artifacts(static_driver, drivers, None, artifacts);

        let response = crate::resource_routes::get_artifact(
            State(state),
            Extension(claims_for_org("org_alpha")),
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
    async fn artifact_route_refuses_another_tenants_artifact_id() {
        // Artifact ids are ULIDs, but obscurity is not isolation: a caller that
        // learns an id from another org must still get nothing back.
        let calls = Arc::new(AtomicU32::new(0));
        let static_driver = Arc::new(TestDriver::ok(DriverKind::Static, calls, b"unused"));
        let drivers = DriverRegistry::new(DriverKind::Static);
        let artifacts = Arc::new(InMemoryStore::new());
        let handle = stored_screenshot(&artifacts, "org_alpha").await;
        let state = test_state_with_artifacts(static_driver, drivers, None, artifacts);

        let response = crate::resource_routes::get_artifact(
            State(state),
            Extension(claims_for_org("org_beta")),
            axum::extract::Path(handle.artifact_id.to_string()),
        )
        .await;

        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "cross-tenant read must not serve bytes, and must not confirm the id exists"
        );
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

    async fn call_scrape(
        state: AppState,
        url: &str,
    ) -> axum::response::Response {
        call_scrape_with(state, serde_json::json!({ "url": url })).await
    }

    async fn call_scrape_with(
        state: AppState,
        body: serde_json::Value,
    ) -> axum::response::Response {
        call_scrape_settings(state, body, &crate::scrape_contract::ScrapeSettings::default()).await
    }

    async fn call_scrape_settings(
        state: AppState,
        body: serde_json::Value,
        settings: &crate::scrape_contract::ScrapeSettings,
    ) -> axum::response::Response {
        let body = serde_json::to_vec(&body).expect("request body");
        match scrape_with(state, claims_for_org("org_alpha"), &body, settings).await {
            Ok(response) => response,
            Err((status, response)) => (status, response).into_response(),
        }
    }

    async fn ingest_request_response(service: bool, zdr: bool) -> (axum::response::Response, u32) {
        let driver = crate::test_support::StubDriver::ok();
        let state = crate::test_support::test_state(driver.clone());
        let mut claims = crate::test_support::claims_for_org("org_alpha");
        if service {
            claims.sub = "service:marketing-core".into();
            claims.user_id = claims.sub.clone();
            claims.service_id = Some(claims.sub.clone());
            claims.principal_type = Some("service".into());
            claims.scopes = vec!["scrape:write".into()];
        }
        // Real Axum JSON/route and service-scope middleware, with already
        // verified claims injected as the auth boundary's test fixture.
        let app = Router::new()
            .route("/v1/scrape", post(scrape))
            .layer(middleware::from_fn(
                crate::auth::require_service_route_scope,
            ))
            .layer(Extension(claims))
            .with_state(state);
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/scrape")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({"url":"https://example.com/", "ingest":true, "zdr":zdr})
                    .to_string(),
            ))
            .unwrap();
        let response = tower::ServiceExt::oneshot(app, request).await.unwrap();
        (response, driver.calls.load(Ordering::Relaxed))
    }

    #[tokio::test]
    async fn ingest_without_data_plane_fails_closed_before_fetch_for_users_and_services() {
        for service in [false, true] {
            let (response, calls) = ingest_request_response(service, false).await;
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body = crate::test_support::response_json(response).await;
            assert_eq!(body["error"]["code"], "RUNTIME_NOT_READY");
            assert_eq!(body["data"], serde_json::Value::Null);
            assert_eq!(calls, 0);
        }
    }

    #[tokio::test]
    async fn ingest_with_zdr_is_invalid_before_fetch_for_users_and_services() {
        for service in [false, true] {
            let (response, calls) = ingest_request_response(service, true).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = crate::test_support::response_json(response).await;
            assert_eq!(body["error"]["code"], "BAD_REQUEST");
            assert_eq!(calls, 0);
        }
    }

    #[tokio::test]
    async fn rate_limited_scrape_yields_api_error_envelope() {
        // The runtime pipeline surfaces a driver RateLimited as-is (no retry
        // loop for single-page runs), so this exercises the handler's 429 path.
        // The driver must be REGISTERED — /v1/scrape builds through the
        // registry, not the bare static driver.
        let err_driver = Arc::new(TestDriver::err(
            DriverKind::Static,
            Arc::new(AtomicU32::new(0)),
            ErrorCode::RateLimited,
        ));
        let mut drivers = DriverRegistry::new(DriverKind::Static);
        drivers.register(err_driver.clone());
        let state =
            test_state_with_artifacts(err_driver, drivers, None, Arc::new(InMemoryStore::new()));
        let response = call_scrape(state, "https://example.com/page").await;

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = crate::test_support::response_json(response).await;
        assert_eq!(body["code"], "RATE_LIMITED");
        assert_eq!(body["retry_after_seconds"], 60);
        // Full ApiError envelope — no request_id/meta wrapper on error paths.
        assert_eq!(
            crate::test_support::json_keys(&body),
            vec![
                "code",
                "error",
                "hint",
                "next_actions",
                "retry_after_seconds",
                "window"
            ]
        );
    }

    #[tokio::test]
    async fn bad_url_scrape_keeps_envelope_error_shape() {
        // Non-429 failures keep the canonical { data, meta, error } envelope:
        // the ApiError adoption is scoped to rate-limit paths only.
        let calls = Arc::new(AtomicU32::new(0));
        let static_driver = Arc::new(TestDriver::ok(DriverKind::Static, calls, b"unused"));
        let state = test_state(static_driver, DriverRegistry::new(DriverKind::Static), None);
        let response = call_scrape(state, "not a url").await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = crate::test_support::response_json(response).await;
        assert_eq!(
            crate::test_support::json_keys(&body),
            vec!["data", "error", "meta"]
        );
        assert_eq!(body["data"], serde_json::Value::Null);
        assert!(body["meta"]["request_id"]
            .as_str()
            .expect("request id")
            .starts_with("req_"));
        assert!(body["error"]["message"]
            .as_str()
            .expect("envelope error message")
            .starts_with("invalid url:"));
    }
    // ---- WS5.D: the request contract -------------------------------------

    const PAGE: &[u8] = b"<html><head><title>Options page</title></head><body><main><p>Hello options world, long enough to read as content for the extractor.</p></main></body></html>";

    fn page_state() -> AppState {
        let calls = Arc::new(AtomicU32::new(0));
        let driver = Arc::new(TestDriver::ok(DriverKind::Static, calls, PAGE));
        let mut drivers = DriverRegistry::new(DriverKind::Static);
        drivers.register(driver.clone());
        test_state(driver, drivers, None)
    }

    struct SlowDriver;

    #[async_trait]
    impl Driver for SlowDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }

        async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Err(QuarryError::new(ErrorCode::DriverFailed, "unreachable"))
        }
    }

    fn strict_settings() -> crate::scrape_contract::ScrapeSettings {
        crate::scrape_contract::ScrapeSettings {
            strict_validation: true,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_default_request_gets_no_inline_parts_and_reports_its_options() {
        let response = call_scrape(page_state(), "https://example.com/a").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("deprecation").is_none());
        let body = crate::test_support::response_json(response).await;
        let data = &body["data"];
        for absent in ["markdown", "html", "raw_html", "truncated", "omitted", "warnings"] {
            assert!(data.get(absent).is_none(), "{absent} must be absent on a default request");
        }
        // Still the same artifact references as before.
        assert!(data["formats"]["markdown"]["artifact_id"].is_string());
        let effective = &data["effective_options"];
        assert_eq!(effective["formats"], serde_json::json!([]));
        assert_eq!(effective["cache_state"], "bypass");
        assert_eq!(effective["zdr"], false);
        assert_eq!(effective["strict"], false);
        assert_eq!(effective["extractor"], "readability");
        assert_eq!(effective["timeout_ms"], 55_000);
    }

    #[tokio::test]
    async fn formats_markdown_returns_top_level_markdown_and_keeps_the_refs() {
        let response = call_scrape_with(
            page_state(),
            serde_json::json!({ "url": "https://example.com/a", "formats": ["markdown"] }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = crate::test_support::response_json(response).await;
        let data = &body["data"];
        assert!(data["markdown"].as_str().unwrap().contains("Hello options world"));
        assert!(data["formats"]["markdown"]["artifact_id"].is_string(), "refs are unchanged");
        assert!(data.get("html").is_none() && data.get("raw_html").is_none());
        assert_eq!(data["effective_options"]["formats"], serde_json::json!(["markdown"]));
        // Non-default options never touch the page cache.
        assert_eq!(data["effective_options"]["cache_state"], "bypass");
    }

    #[tokio::test]
    async fn html_and_raw_html_return_the_fetched_document_inline() {
        let response = call_scrape_with(
            page_state(),
            serde_json::json!({ "url": "https://example.com/a", "formats": ["html", "rawHtml"] }),
        )
        .await;
        let body = crate::test_support::response_json(response).await;
        let data = &body["data"];
        // raw_html is the document as fetched; html is what the markdown was made from.
        assert!(data["raw_html"].as_str().unwrap().contains("<title>Options page</title>"));
        assert!(data["html"].as_str().unwrap().contains("Hello options world"));
        assert!(!data["html"].as_str().unwrap().contains("<title>"));
        assert!(data.get("markdown").is_none(), "markdown only when asked for");
    }

    #[tokio::test]
    async fn an_over_cap_field_is_omitted_outside_zdr_and_truncated_under_it() {
        // Not ZDR: the field is left out and named; the stored artifact has it.
        let response = call_scrape_with(
            page_state(),
            serde_json::json!({ "url": "https://example.com/a", "formats": ["markdown"], "max_inline_bytes": 10 }),
        )
        .await;
        let body = crate::test_support::response_json(response).await;
        let data = &body["data"];
        assert!(data.get("markdown").is_none());
        assert_eq!(data["omitted"], serde_json::json!(["markdown"]));
        assert!(data["formats"]["markdown"]["artifact_id"].is_string());

        // ZDR: nothing is stored, so the text is cut instead, on a character boundary.
        let response = call_scrape_with(
            page_state(),
            serde_json::json!({ "url": "https://example.com/a", "zdr": true, "max_inline_bytes": 10 }),
        )
        .await;
        let body = crate::test_support::response_json(response).await;
        let data = &body["data"];
        assert!(data["markdown"].as_str().unwrap().len() <= 10);
        assert_eq!(data["truncated"], serde_json::json!(["markdown"]));
        assert!(data.get("omitted").is_none());
        assert!(data["formats"]["markdown"].is_null(), "ZDR stores no artifact");
        assert_eq!(data["effective_options"]["zdr"], true);
    }

    #[tokio::test]
    async fn zdr_with_no_formats_defaults_to_inline_markdown() {
        let response = call_scrape_with(
            page_state(),
            serde_json::json!({ "url": "https://example.com/a", "zdr": true }),
        )
        .await;
        let body = crate::test_support::response_json(response).await;
        assert!(body["data"]["markdown"].as_str().unwrap().contains("Hello options world"));
        assert_eq!(body["data"]["effective_options"]["formats"], serde_json::json!(["markdown"]));
    }

    #[tokio::test]
    async fn lenient_mode_warns_about_unknown_fields_and_says_so_in_headers() {
        let settings = crate::scrape_contract::ScrapeSettings {
            strict_after: Some(chrono::DateTime::parse_from_rfc3339("2099-01-01T00:00:00Z").unwrap().into()),
            ..Default::default()
        };
        let response = call_scrape_settings(
            page_state(),
            serde_json::json!({ "url": "https://example.com/a", "formats": ["markdown"], "maxPages": 1 }),
            &settings,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("deprecation").unwrap(), "true");
        assert_eq!(response.headers().get("sunset").unwrap(), "Thu, 01 Jan 2099 00:00:00 GMT");
        let body = crate::test_support::response_json(response).await;
        let data = &body["data"];
        assert!(data["markdown"].is_string(), "the known option still worked");
        assert_eq!(data["warnings"][0]["code"], "unknown_field");
        assert_eq!(data["warnings"][0]["field"], "maxPages");
        assert_eq!(data["effective_options"]["ignored_fields"], serde_json::json!(["maxPages"]));
    }

    #[tokio::test]
    async fn strict_mode_rejects_unknown_fields_with_their_paths() {
        let response = call_scrape_settings(
            page_state(),
            serde_json::json!({ "url": "https://example.com/a", "maxPages": 1 }),
            &strict_settings(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = crate::test_support::response_json(response).await;
        assert_eq!(body["error"]["code"], "BAD_REQUEST");
        assert_eq!(body["error"]["details"]["fields"][0]["path"], "maxPages");
        assert_eq!(body["error"]["details"]["fields"][0]["code"], "unknown_field");
    }

    #[tokio::test]
    async fn a_reserved_field_is_rejected_in_lenient_mode_too() {
        for key in ["skipTlsVerification", "proxy", "headers", "stealth"] {
            let response = call_scrape_with(
                page_state(),
                serde_json::json!({ "url": "https://example.com/a", key: true }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{key}");
            let body = crate::test_support::response_json(response).await;
            assert_eq!(body["error"]["details"]["fields"][0]["code"], "reserved_field", "{key}");
        }
    }

    #[tokio::test]
    async fn a_malformed_body_and_a_bad_option_return_the_envelope_with_field_paths() {
        let calls = Arc::new(AtomicU32::new(0));
        let driver = Arc::new(TestDriver::ok(DriverKind::Static, calls, PAGE));
        let state = test_state(driver, DriverRegistry::new(DriverKind::Static), None);

        let response = scrape_with(
            state.clone(),
            claims_for_org("org_alpha"),
            b"{not json",
            &crate::scrape_contract::ScrapeSettings::default(),
        )
        .await
        .map(|response| response)
        .unwrap_or_else(|(status, response)| (status, response).into_response());
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = crate::test_support::response_json(response).await;
        assert_eq!(body["error"]["details"]["fields"][0]["code"], "invalid_json");
        assert_eq!(body["data"], serde_json::Value::Null);

        let response = call_scrape_with(
            state.clone(),
            serde_json::json!({ "url": "https://example.com/a", "formats": ["markdown", "docx"], "timeout_ms": 5 }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = crate::test_support::response_json(response).await;
        let fields = body["error"]["details"]["fields"].as_array().unwrap();
        assert_eq!(fields[0]["path"], "formats[1]");
        assert_eq!(fields[0]["code"], "unsupported_format");

        // A missing url is a field error too, not axum's plain rejection.
        let response = call_scrape_with(state, serde_json::json!({ "formats": ["markdown"] })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = crate::test_support::response_json(response).await;
        assert_eq!(body["error"]["details"]["fields"][0]["code"], "invalid_request");
    }

    #[tokio::test]
    async fn timeout_ms_expiry_is_a_typed_504() {
        let slow = Arc::new(SlowDriver);
        let mut drivers = DriverRegistry::new(DriverKind::Static);
        drivers.register(slow.clone());
        let state = test_state(slow, drivers, None);
        let started = std::time::Instant::now();
        let response = call_scrape_with(
            state,
            serde_json::json!({ "url": "https://example.com/slow", "timeout_ms": 1000 }),
        )
        .await;
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = crate::test_support::response_json(response).await;
        assert_eq!(body["error"]["code"], "TIMEOUT");
        assert_eq!(body["error"]["details"]["op"], "scrape");
        assert_eq!(body["error"]["details"]["deadline_ms"], 1000);
    }

    const NOISY: &[u8] = b"<html><head><title>Noisy</title></head><body><nav>Site navigation links</nav><article><h1>The headline</h1><p>Body text of the article that is long enough to be the main content of the page.</p><aside class=\"related\">Related stories box</aside></article><footer>Footer small print</footer></body></html>";

    fn noisy_state() -> AppState {
        let calls = Arc::new(AtomicU32::new(0));
        let driver = Arc::new(TestDriver::ok(DriverKind::Static, calls, NOISY));
        let mut drivers = DriverRegistry::new(DriverKind::Static);
        drivers.register(driver.clone());
        test_state(driver, drivers, None)
    }

    async fn markdown_of(body: serde_json::Value) -> (String, serde_json::Value) {
        let response = call_scrape_with(noisy_state(), body).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = crate::test_support::response_json(response).await;
        (body["data"]["markdown"].as_str().unwrap_or_default().to_owned(), body)
    }

    #[tokio::test]
    async fn include_and_exclude_tags_shape_the_markdown_and_nothing_is_ignored() {
        let (default, _) = markdown_of(serde_json::json!({ "url": "https://example.com/a", "formats": ["markdown"] })).await;
        let (subset, body) = markdown_of(serde_json::json!({
            "url": "https://example.com/a", "formats": ["markdown"],
            "include_tags": ["article"], "exclude_tags": ["aside", ".related"]
        }))
        .await;
        assert!(subset.contains("The headline"), "{subset}");
        assert!(!subset.contains("Related stories"), "{subset}");
        assert!(!subset.contains("Site navigation"), "{subset}");
        assert!(subset.len() <= default.len() + 20, "a strict subset in effect: {subset:?} vs {default:?}");
        assert_eq!(body["data"]["effective_options"]["ignored_fields"], serde_json::json!([]));
        assert_eq!(body["data"]["effective_options"]["cache_state"], "bypass");
    }

    #[tokio::test]
    async fn only_main_content_false_keeps_the_navigation() {
        let (full, body) = markdown_of(serde_json::json!({
            "url": "https://example.com/a", "formats": ["markdown"], "only_main_content": false
        }))
        .await;
        assert!(full.contains("Site navigation"), "{full}");
        assert!(full.contains("Footer small print"), "{full}");
        assert_eq!(body["data"]["effective_options"]["extractor"], "clean_full");
    }

    #[tokio::test]
    async fn an_include_selector_that_matches_nothing_is_empty_with_a_warning() {
        let (markdown, body) = markdown_of(serde_json::json!({
            "url": "https://example.com/a", "formats": ["markdown"], "include_tags": ["#nope"]
        }))
        .await;
        assert_eq!(markdown, "");
        assert_eq!(body["data"]["warnings"][0]["code"], "include_tags_matched_nothing");
    }

    #[tokio::test]
    async fn an_invalid_selector_and_a_conflicting_extractor_are_field_errors() {
        let response = call_scrape_with(
            noisy_state(),
            serde_json::json!({ "url": "https://example.com/a", "include_tags": ["article", "div["], "exclude_tags": ["ok", "<<<"] }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = crate::test_support::response_json(response).await;
        let fields = body["error"]["details"]["fields"].as_array().unwrap();
        let paths: Vec<&str> = fields.iter().map(|f| f["path"].as_str().unwrap()).collect();
        assert_eq!(paths, vec!["include_tags[1]", "exclude_tags[1]"]);
        assert!(fields.iter().all(|f| f["code"] == "invalid_selector"));

        let response = call_scrape_with(
            noisy_state(),
            serde_json::json!({ "url": "https://example.com/a", "only_main_content": true, "extractor": "raw" }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = crate::test_support::response_json(response).await;
        assert_eq!(body["error"]["details"]["fields"][0]["path"], "extractor");
    }

    // ---- WS5.F: the scrape record ---------------------------------------

    async fn scrape_id_of(response: axum::response::Response) -> (String, serde_json::Value) {
        assert_eq!(response.status(), StatusCode::OK);
        let body = crate::test_support::response_json(response).await;
        let id = body["data"]["scrape_id"].as_str().expect("scrape_id").to_owned();
        (id, body)
    }

    async fn read_scrape(state: AppState, claims: crate::auth::Claims, id: &str) -> axum::response::Response {
        crate::scrape_routes::get_scrape(State(state), Extension(claims), axum::extract::Path(id.to_owned())).await
    }

    #[tokio::test]
    async fn a_scrape_can_be_read_back_by_id_for_the_same_org_only() {
        let state = page_state();
        let (id, original) = scrape_id_of(
            call_scrape_with(
                state.clone(),
                serde_json::json!({ "url": "https://example.com/a", "formats": ["markdown"] }),
            )
            .await,
        )
        .await;

        let response = read_scrape(state.clone(), claims_for_org("org_alpha"), &id).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("cache-control").unwrap(), "private, max-age=30");
        let body = crate::test_support::response_json(response).await;
        let data = &body["data"];
        assert_eq!(data["fingerprint"], original["data"]["fingerprint"]);
        assert_eq!(data["scrape_id"], id.as_str());
        assert_eq!(data["effective_options"]["formats"], serde_json::json!(["markdown"]));
        // Inline text was never stored, so it is not on the read-back.
        for absent in ["markdown", "html", "raw_html"] {
            assert!(data.get(absent).is_none(), "{absent}");
        }

        // Another organization's token: not found, never forbidden.
        let response = read_scrape(state, claims_for_org("org_beta"), &id).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_id_that_is_not_a_scrape_record_reads_as_not_found() {
        let state = page_state();
        // A screenshot artifact of the same org.
        let handle = state
            .artifacts
            .put("org_alpha", &RunKind::new(), "blake3:x", "screenshot", b"png".to_vec())
            .await
            .unwrap();
        let response =
            read_scrape(state.clone(), claims_for_org("org_alpha"), &handle.artifact_id.to_string()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = read_scrape(state, claims_for_org("org_alpha"), "not-an-id").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_zdr_scrape_has_no_scrape_id_and_stores_no_artifact() {
        let artifacts = Arc::new(InMemoryStore::new());
        let calls = Arc::new(AtomicU32::new(0));
        let driver = Arc::new(TestDriver::ok(DriverKind::Static, calls, PAGE));
        let mut drivers = DriverRegistry::new(DriverKind::Static);
        drivers.register(driver.clone());
        let state = test_state_with_artifacts(driver, drivers, None, artifacts.clone());
        let response = call_scrape_with(
            state,
            serde_json::json!({ "url": "https://example.com/a", "zdr": true }),
        )
        .await;
        let body = crate::test_support::response_json(response).await;
        assert!(body["data"].get("scrape_id").is_none());
        let listed = artifacts
            .list("org_alpha", &quarry_core::pagination::ListFilter::default())
            .await
            .unwrap();
        assert!(listed.items.is_empty(), "ZDR must write no artifact");
    }

    #[tokio::test]
    async fn a_zdr_floored_reader_is_refused_even_for_an_earlier_record() {
        let state = page_state();
        let (id, _) = scrape_id_of(call_scrape(state.clone(), "https://example.com/a").await).await;
        let mut claims = claims_for_org("org_alpha");
        claims.zdr = Some(true);
        let response = read_scrape(state, claims, &id).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn records_can_be_switched_off() {
        let settings = crate::scrape_contract::ScrapeSettings {
            records_enabled: false,
            ..Default::default()
        };
        let response = call_scrape_settings(
            page_state(),
            serde_json::json!({ "url": "https://example.com/a" }),
            &settings,
        )
        .await;
        let body = crate::test_support::response_json(response).await;
        assert!(body["data"].get("scrape_id").is_none());
    }

    // ---- INNH-4: run_page hands back the id of the stored markdown ----

    fn output_with_markdown(markdown: Option<&str>) -> NormalizedOutput {
        let mut formats = serde_json::json!({});
        if let Some(id) = markdown {
            formats = serde_json::json!({ "markdown": { "artifact_id": id, "bytes": 12 } });
        }
        serde_json::from_value(serde_json::json!({
            "run_id": quarry_core::ids::kinds::RunKind::new(),
            "url": { "requested": "https://example.com/p", "final_url": "https://example.com/p" },
            "status": 200,
            "fetched_at": "2026-01-01T00:00:00Z",
            "fingerprint": "blake3:fp",
            "formats": formats,
            "change": { "status": "new" },
            "metadata": {},
            "driver": { "kind": "static", "duration_ms": 1 }
        }))
        .expect("a minimal NormalizedOutput")
    }

    #[test]
    fn run_page_result_carries_the_markdown_artifact_id() {
        let id = quarry_core::ids::kinds::ArtifactKind::new().to_string();
        let out = output_with_markdown(Some(&id));
        let result = InternalRunPageResult::from_output("run_1".into(), &out);
        assert_eq!(result.markdown_artifact_id.as_deref(), Some(id.as_str()));
        let wire = serde_json::to_value(&result).unwrap();
        assert_eq!(wire["markdown_artifact_id"], id);
    }

    #[test]
    fn run_page_result_omits_the_markdown_artifact_id_when_nothing_was_stored() {
        // ZDR (and a failed write) leave `formats.markdown` empty: the wire carries no key,
        // so the orchestrator forwards nothing and no hash-only stand-in exists.
        let out = output_with_markdown(None);
        let result = InternalRunPageResult::from_output("run_1".into(), &out);
        assert!(result.markdown_artifact_id.is_none());
        let wire = serde_json::to_value(&result).unwrap();
        assert!(wire.get("markdown_artifact_id").is_none(), "{wire}");
        assert!(wire.get("source_artifact_id").is_none(), "{wire}");
        assert!(wire.get("bytes").is_none(), "{wire}");
    }

    fn output_with_formats(formats: serde_json::Value) -> NormalizedOutput {
        let mut out = output_with_markdown(None);
        out.formats = serde_json::from_value(formats).expect("formats");
        out
    }

    // ---- INNH-12: the per-run results read these fields off page_fetched ----

    #[test]
    fn run_page_result_carries_the_source_artifact_and_markdown_bytes() {
        let md = quarry_core::ids::kinds::ArtifactKind::new().to_string();
        let html = quarry_core::ids::kinds::ArtifactKind::new().to_string();
        let out = output_with_formats(serde_json::json!({
            "markdown": { "artifact_id": md, "bytes": 321 },
            "html": { "artifact_id": html, "bytes": 9000 }
        }));
        let wire = serde_json::to_value(InternalRunPageResult::from_output("run_1".into(), &out))
            .unwrap();
        assert_eq!(wire["markdown_artifact_id"], md);
        assert_eq!(wire["source_artifact_id"], html);
        assert_eq!(wire["bytes"], 321, "bytes is the markdown size, not the source size");
    }

    #[test]
    fn run_page_result_source_is_the_raw_artifact_for_a_document() {
        let md = quarry_core::ids::kinds::ArtifactKind::new().to_string();
        let raw = quarry_core::ids::kinds::ArtifactKind::new().to_string();
        let out = output_with_formats(serde_json::json!({
            "markdown": { "artifact_id": md, "bytes": 5 },
            "raw": { "artifact_id": raw, "bytes": 77 }
        }));
        let result = InternalRunPageResult::from_output("run_1".into(), &out);
        assert_eq!(result.source_artifact_id.as_deref(), Some(raw.as_str()));
    }

    // ---- WS14.S4, D2-091: a robots link refusal is counted only when the gate enforces ----

    /// Answers every robots.txt with the same body.
    struct FixedRobots(&'static str);

    #[async_trait]
    impl quarry_runtime::RobotsFetcher for FixedRobots {
        async fn fetch(&self, _origin: &str, _org_id: &str) -> quarry_runtime::RobotsFetch {
            quarry_runtime::RobotsFetch::Found(self.0.to_owned())
        }
    }

    /// A gate in `mode` whose cache already holds the page's origin, so the link filter reads
    /// the rules from the cache as it does on a live crawl.
    async fn gate_with_cached_rules(
        mode: quarry_runtime::RobotsEnforceMode,
        page: &Url,
    ) -> quarry_runtime::RobotsGate {
        let gate = quarry_runtime::RobotsGate::with_fetcher(
            Arc::new(FixedRobots("User-agent: *\nDisallow: /private/\n")),
            quarry_runtime::RobotsGateConfig {
                mode,
                ..quarry_runtime::RobotsGateConfig::default()
            },
        );
        let _ = gate.rules_for_origin(page, "org_test", false).await;
        gate
    }

    #[tokio::test]
    async fn robots_link_refusals_are_counted_only_when_the_gate_enforces() {
        let page = "https://example.test/docs/landing";
        let page_url: Url = page.parse().unwrap();
        let scope = quarry_runtime::crawl_filter::CompiledScope::compile(
            &quarry_core::crawl_scope::CrawlContext::default(),
        )
        .expect("the default scope compiles");
        let page_links = || {
            let mut result =
                InternalRunPageResult::from_output("run_1".into(), &output_with_markdown(None));
            result.links = vec![
                "https://example.test/private/x".to_owned(),
                "https://example.test/docs/a".to_owned(),
            ];
            result
        };
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();

        let observe =
            gate_with_cached_rules(quarry_runtime::RobotsEnforceMode::Observe, &page_url).await;
        let kept = metrics::with_local_recorder(&recorder, || {
            with_crawl(page_links(), page, Some(&scope), Some(&observe))
        });
        assert_eq!(kept.links.len(), 2, "observe removes no link");
        assert!(
            !handle
                .render()
                .contains("quarry_robots_link_refusals_total"),
            "observe counts no refusal: {}",
            handle.render()
        );

        let enforce =
            gate_with_cached_rules(quarry_runtime::RobotsEnforceMode::Enforce, &page_url).await;
        let kept = metrics::with_local_recorder(&recorder, || {
            with_crawl(page_links(), page, Some(&scope), Some(&enforce))
        });
        assert_eq!(kept.links, vec!["https://example.test/docs/a".to_owned()]);
        assert!(
            handle
                .render()
                .contains(r#"quarry_robots_link_refusals_total{outcome="denied"} 1"#),
            "{}",
            handle.render()
        );
    }
}
