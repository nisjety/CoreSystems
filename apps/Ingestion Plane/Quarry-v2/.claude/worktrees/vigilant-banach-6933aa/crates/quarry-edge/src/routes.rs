use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    middleware,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use url::Url;

use quarry_core::cache::{CacheMode, CachePolicy};
use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds::{RequestKind, RunKind};
use quarry_core::output::NormalizedOutput;
use quarry_core::zdr::ZdrMode;
use quarry_runtime::driver_plan::{DriverSignals, plan_from_signals};
use quarry_runtime::pipeline::PageRunner;

use crate::cache::DEFAULT_TTL_SECS;
use crate::state::AppState;
use axum::response::sse::Event;
use axum::response::Sse;

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
        // Internal plane: orchestrator activities call here. Not exposed
        // publicly in prod; same binary today — later a separate
        // `quarry-runtime-svc` crate.
        .route("/v1/internal/run_page", post(internal_run_page))
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
        .route("/v1/answer", post(crate::answer_routes::answer))
        // Cycle 22 / cluster #4 part 1 — resource list endpoints.
        // /v1/artifacts is served locally; the rest forward to control plane.
        .route("/v1/artifacts", get(crate::resource_routes::list_artifacts))
        .route("/v1/sources", get(crate::resource_routes::list_sources))
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
        .layer(middleware::from_fn(crate::auth::require_auth));

    Router::new()
        .merge(public)
        .merge(protected)
        .layer(TraceLayer::new_for_http())
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
    pub signals: Option<DriverSignals>,
    #[serde(default)]
    pub ingest: Option<bool>,
    #[serde(default)]
    pub org_id: Option<String>,
}

async fn scrape(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<ScrapeRequest>,
) -> Result<Json<Envelope<NormalizedOutput>>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let zdr = ZdrMode::from(req.zdr.unwrap_or(false));
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
    let driver = state.drivers.build_driver(&plan);

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
        cancel_token: None,
        local_index: state.local_index.clone(),
        policy: quarry_runtime::RunPolicy::default(),
        scheduler: state.scheduler.clone(),
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
    pub signals: Option<DriverSignals>,
    #[serde(default)]
    pub ingest: Option<bool>,
    #[serde(default)]
    pub org_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InternalRunPageResult {
    pub run_id: String,
    pub status: u16,
    pub fingerprint: String,
    pub links: Vec<String>,
}

async fn internal_run_page(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<InternalRunPage>,
) -> Result<Json<InternalRunPageResult>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = RequestKind::new().to_string();
    let zdr = ZdrMode::from(req.zdr.unwrap_or(false));
    // Same tenant enforcement as the public /v1/scrape: ignore client
    // org_id, use the verified JWT claim. /v1/internal/run_page is
    // typically called by the orchestrator on behalf of a tenant, so the
    // service token used must carry the originating org's claim.
    let _ = &req.org_id;
    let org_id = claims.org_id.clone();

    let url: Url = req.url.parse().map_err(|e| {
        let err = QuarryError::new(ErrorCode::BadRequest, format!("invalid url: {e}"));
        (
            StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::BAD_REQUEST),
            Json(Envelope::<()>::err(&request_id, err)),
        )
    })?;

    let plan = plan_from_signals(req.signals.unwrap_or_default());
    let driver = state.drivers.build_driver(&plan);

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
        cancel_token: None,
        local_index: state.local_index.clone(),
        policy: quarry_runtime::RunPolicy::default(),
        scheduler: state.scheduler.clone(),
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
        let driver = state.drivers.build_driver(&plan);

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
        cancel_token: None,
        local_index: state.local_index.clone(),
        policy: quarry_runtime::RunPolicy::default(),
        scheduler: state.scheduler.clone(),
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
