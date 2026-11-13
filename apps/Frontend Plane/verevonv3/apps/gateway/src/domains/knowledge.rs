mod diagnostics;
pub(crate) mod documents;
mod imports;
mod operating_map;
pub(crate) mod products;
pub(crate) mod quarry;
mod retrieval;
pub(crate) mod shared;
pub(crate) mod sync;
mod wiki;
mod workspace;

use std::sync::OnceLock;

use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::error_with_details,
    middleware::{require_session, AuthenticatedUser},
    rate_limit::{with_rate_class, RateClass},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    // Submitting a crawl is the expensive, queue-filling call: its own
    // per-organization bucket, so a burst of submissions cannot starve search or
    // scrape, and the other way round (see `rate_limit::RateClass`).
    let job_submit = Router::new()
        .route(
            "/api/v1/knowledge/crawl/discover",
            post(quarry::discover_crawl_pages),
        )
        // E3.7.9a: a whole-site submit passes the approval gate before the edge is called.
        .route("/api/v1/knowledge/crawl", post(start_crawl_gated));
    let search_scrape = Router::new()
        .route(
            "/api/v1/knowledge/scrape-preview",
            post(quarry::scrape_preview),
        )
        .route("/api/v1/knowledge/scrape", post(quarry::scrape))
        .route(
            "/api/v1/knowledge/scrape-artifacts/{artifact_id}",
            get(quarry::scrape_artifact),
        )
        .route(
            "/api/v1/knowledge/scrape/products",
            post(products::extract_products),
        )
        .route(
            "/api/v1/knowledge/scrape/products/summary",
            post(products::summarize_products),
        );
    Router::new()
        // Documents
        .route(
            "/api/v1/knowledge/documents",
            get(documents::list_documents).post(documents::create_document),
        )
        // DELETE was missing entirely, which made the knowledge base
        // append-only from the product's side: a chat attachment is ingested
        // here as a durable, org-wide document (see chat::documents), and
        // nothing in the SPA or this gateway could remove it again — a demo
        // source pack uploaded once kept surfacing in unrelated conversations'
        // knowledge search, with no way out but a direct Data Plane call.
        // documents-api has supported `DELETE /v1/documents/{id}` under
        // `documents:write` all along.
        .route(
            "/api/v1/knowledge/documents/{id}",
            get(documents::get_document).delete(documents::delete_document),
        )
        // Knowledge workspace: the rich aggregated payload the SPA renders (sources,
        // collections, graph, finspo storage analytics, integrations, files, diagnostics).
        // Fans out server-side across Data Plane v2 + integration-core + finspo + quarry.
        .route("/api/v1/knowledge/sources", get(workspace::load_workspace))
        // Flat document-summary list (documents-api passthrough) for pickers/typeaheads
        // that only need {id,title,source,type} rather than the full workspace payload.
        .route(
            "/api/v1/knowledge/source-list",
            get(documents::list_sources),
        )
        // Workspace mutations: re-sync all connected sources, register a SharePoint drive.
        .route("/api/v1/knowledge/sync", post(sync::sync_workspace))
        .route(
            "/api/v1/knowledge/sharepoint",
            post(sync::register_sharepoint),
        )
        // SharePoint browse: pick a site, then a document library, instead of
        // hand-entering raw Microsoft Graph ids on the register form.
        .route(
            "/api/v1/knowledge/sharepoint/sites",
            get(sync::list_sharepoint_sites),
        )
        .route(
            "/api/v1/knowledge/sharepoint/sites/{site_id}/drives",
            get(sync::list_sharepoint_drives),
        )
        .route(
            "/api/v1/knowledge/sharepoint/drives/{drive_id}/children",
            get(sync::list_sharepoint_folders),
        )
        // Retrieval
        .route(
            "/api/v1/knowledge/search",
            post(retrieval::search_knowledge),
        )
        .route(
            "/api/v1/knowledge/retrieval/{trace_id}",
            get(retrieval::get_retrieval_trace),
        )
        .route(
            "/api/v1/knowledge/retrieve/sources",
            post(retrieval::resolve_sources),
        )
        .route(
            "/api/v1/knowledge/retrieve/chunks",
            post(retrieval::expand_chunks),
        )
        .route(
            "/api/v1/knowledge/retrieve/graph",
            post(retrieval::graph_retrieve),
        )
        .route(
            "/api/v1/knowledge/retrieve/wiki",
            post(retrieval::wiki_retrieve),
        )
        // Operating Map — durable, Data Plane-owned AI rollout map surfaced inside Knowledge.
        .route(
            "/api/v1/knowledge/operating-map",
            get(operating_map::get_operating_map),
        )
        .route(
            "/api/v1/knowledge/operating-map/generate",
            post(operating_map::generate_operating_map),
        )
        .route(
            "/api/v1/knowledge/operating-map/runs/{run_id}/events",
            get(operating_map::operating_map_run_events),
        )
        .route(
            "/api/v1/knowledge/operating-map/proposals/{proposal_id}/review",
            post(operating_map::review_operating_map_proposal),
        )
        // Wiki — canonical knowledge namespace. Static "by-path" must come
        // before the :id param route.
        .route(
            "/api/v1/knowledge/wiki/pages/by-path",
            get(wiki::wiki_page_by_path),
        )
        .route("/api/v1/knowledge/wiki/pages", get(wiki::list_wiki_pages))
        .route(
            "/api/v1/knowledge/wiki/pages/{id}",
            get(wiki::get_wiki_page),
        )
        .route(
            "/api/v1/knowledge/wiki/pages/{id}/versions",
            get(wiki::wiki_page_versions),
        )
        .route(
            "/api/v1/knowledge/wiki/pages/{id}/diff",
            get(wiki::wiki_page_diff),
        )
        .route(
            "/api/v1/knowledge/wiki/pages/{id}/backlinks",
            get(wiki::wiki_page_backlinks),
        )
        // W5-094-b: the wiki proposals a person reviews. The list is read-only; the review writes only on accept.
        .route(
            "/api/v1/knowledge/wiki/proposals",
            get(wiki::list_wiki_proposals),
        )
        .route(
            "/api/v1/knowledge/wiki/proposals/{id}/review",
            post(wiki::review_wiki_proposal),
        )
        // Legacy aliases kept for existing callers.
        .route("/api/v1/wiki/pages/by-path", get(wiki::wiki_page_by_path))
        .route("/api/v1/wiki/pages", get(wiki::list_wiki_pages))
        .route("/api/v1/wiki/pages/{id}", get(wiki::get_wiki_page))
        .route(
            "/api/v1/wiki/pages/{id}/versions",
            get(wiki::wiki_page_versions),
        )
        .route("/api/v1/wiki/pages/{id}/diff", get(wiki::wiki_page_diff))
        .route(
            "/api/v1/wiki/pages/{id}/backlinks",
            get(wiki::wiki_page_backlinks),
        )
        // Imports
        .route(
            "/api/v1/knowledge/imports/upload",
            post(imports::import_upload),
        )
        .route(
            "/api/v1/knowledge/imports/source",
            post(imports::import_source),
        )
        .route(
            "/api/v1/knowledge/imports/{id}",
            get(imports::get_import_job),
        )
        .route(
            "/api/v1/knowledge/imports/{id}/events",
            get(imports::import_job_events),
        )
        // Quarry
        .route("/api/v1/knowledge/crawl/jobs", get(quarry::list_crawl_jobs))
        .route(
            "/api/v1/knowledge/runs/{id}/events",
            get(quarry::crawl_run_events),
        )
        // Crawl 0-pages fix — durable crawl event stream keyed off the
        // handoff job_id (the normalized handoff's `eventStream` points
        // here). The edge emits real SSE; this forwards it verbatim.
        .route(
            "/api/v1/knowledge/jobs/{id}/events",
            get(quarry::crawl_job_events),
        )
        // WS15.L5: status, errors, ongoing and the pause/resume/cancel verbs of one crawl.
        .merge(crawl_control_routes())
        .merge(with_rate_class(
            job_submit,
            &state.rate_limiter,
            RateClass::JobSubmit,
        ))
        .merge(with_rate_class(
            search_scrape,
            &state.rate_limiter,
            RateClass::SearchScrape,
        ))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// The read and control routes of one crawl (WS15.L5). `ongoing` is a static
/// segment, so it takes precedence over `{id}` (the same holds for `jobs` and
/// `discover`, which are registered beside it); the verbs reach quarry-edge's
/// `/v1/crawl/{id}` family, which keeps them behind its own `JOB_CONTROL` flag.
fn crawl_control_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/knowledge/crawl/ongoing", get(quarry::crawl_ongoing))
        .route(
            "/api/v1/knowledge/crawl/{id}",
            get(quarry::crawl_status).delete(quarry::crawl_cancel),
        )
        .route(
            "/api/v1/knowledge/crawl/{id}/errors",
            get(quarry::crawl_errors),
        )
        .route(
            "/api/v1/knowledge/crawl/{id}/pause",
            post(quarry::crawl_pause),
        )
        .route(
            "/api/v1/knowledge/crawl/{id}/resume",
            post(quarry::crawl_resume),
        )
}

/// E3.7.9a: a whole-site crawl above the approval threshold needs its page count confirmed
/// before it starts (owner decision 2, 2026-10-23). This is the default threshold; the setting
/// may only lower it.
pub(crate) const CRAWL_APPROVAL_PAGES_DEFAULT: u64 = 50;

/// The page ceiling the crawl normaliser applies (`normalize_crawl_body_for`). A crawl with no
/// bound of its own is estimated at it, because the gateway cannot see the edge's own default.
pub(crate) const CRAWL_MAX_PAGES_CEILING: u64 = 5_000;

/// What the gate does. `Observe` logs a crawl it would refuse and changes no response. It is
/// the default, so turning enforcement on is a separate, reviewed step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrawlApprovalMode {
    Off,
    Observe,
    Enforce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CrawlApprovalPolicy {
    pub(crate) mode: CrawlApprovalMode,
    pub(crate) threshold_pages: u64,
}

/// `CRAWL_APPROVAL_MODE`: `off`, `observe` or `enforce`. Unset or unrecognised means `observe`.
pub(crate) fn parse_crawl_approval_mode(raw: Option<&str>) -> CrawlApprovalMode {
    match raw
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("off") => CrawlApprovalMode::Off,
        Some("enforce") => CrawlApprovalMode::Enforce,
        _ => CrawlApprovalMode::Observe,
    }
}

/// `CRAWL_APPROVAL_PAGES`, tighten-only: a lower value lowers the threshold, a higher one is
/// held at the default, and anything unparsable is the default.
pub(crate) fn parse_crawl_approval_pages(raw: Option<&str>) -> u64 {
    raw.map(str::trim)
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(CRAWL_APPROVAL_PAGES_DEFAULT, |pages| {
            pages.min(CRAWL_APPROVAL_PAGES_DEFAULT)
        })
}

/// The gate's policy for this process, read from the environment once. The settings belong on
/// `AppState`; they are read here until `config.rs` is in scope, and the `OnceLock` keeps the
/// read out of request handling.
pub(crate) fn crawl_approval_policy() -> CrawlApprovalPolicy {
    static POLICY: OnceLock<CrawlApprovalPolicy> = OnceLock::new();
    *POLICY.get_or_init(|| CrawlApprovalPolicy {
        mode: parse_crawl_approval_mode(std::env::var("CRAWL_APPROVAL_MODE").ok().as_deref()),
        threshold_pages: parse_crawl_approval_pages(
            std::env::var("CRAWL_APPROVAL_PAGES").ok().as_deref(),
        ),
    })
}

/// The page count a crawl is estimated to fetch: its bound, held to the ceiling.
pub(crate) fn estimated_crawl_pages(input: &Value) -> u64 {
    requested_crawl_pages(input).map_or(CRAWL_MAX_PAGES_CEILING, |pages| {
        pages.min(CRAWL_MAX_PAGES_CEILING)
    })
}

fn requested_crawl_pages(input: &Value) -> Option<u64> {
    input
        .get("max_pages")
        .or_else(|| input.get("maxPages"))
        .and_then(Value::as_u64)
}

/// A curated batch (a non-empty `urls` list) is not a whole-site crawl, so it is not gated here.
fn is_whole_site_crawl(input: &Value) -> bool {
    !input
        .get("urls")
        .and_then(Value::as_array)
        .is_some_and(|urls| !urls.is_empty())
}

/// The page count the person confirmed, sent as `approvedMaxPages`. It must cover the estimate.
fn confirmed_crawl_pages(input: &Value) -> Option<u64> {
    input
        .get("approvedMaxPages")
        .or_else(|| input.get("approved_max_pages"))
        .and_then(Value::as_u64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrawlApprovalDecision {
    /// The crawl may start: no approval is needed, or the page count is confirmed.
    Proceed,
    /// Above the threshold in observe mode: the crawl starts and the event is logged.
    Observed { estimated_pages: u64 },
    /// Above the threshold in enforce mode, with no confirmation that covers the estimate.
    Refuse { estimated_pages: u64 },
}

/// Whether a whole-site crawl may start. Only a request from a person (`Surface::Ui`) can
/// confirm its own page count; a Model proposal (`Surface::Model`) never can.
pub(crate) fn decide_crawl_approval(
    input: &Value,
    policy: CrawlApprovalPolicy,
    surface: quarry::Surface,
) -> CrawlApprovalDecision {
    if policy.mode == CrawlApprovalMode::Off || !is_whole_site_crawl(input) {
        return CrawlApprovalDecision::Proceed;
    }
    let estimated_pages = estimated_crawl_pages(input);
    if estimated_pages <= policy.threshold_pages {
        return CrawlApprovalDecision::Proceed;
    }
    let confirmed = surface == quarry::Surface::Ui
        && confirmed_crawl_pages(input).is_some_and(|pages| pages >= estimated_pages);
    if confirmed {
        return CrawlApprovalDecision::Proceed;
    }
    match policy.mode {
        CrawlApprovalMode::Enforce => CrawlApprovalDecision::Refuse { estimated_pages },
        CrawlApprovalMode::Observe | CrawlApprovalMode::Off => {
            CrawlApprovalDecision::Observed { estimated_pages }
        }
    }
}

/// The typed refusal: 409 `approval_required`, carrying the estimate and the threshold the
/// server applied, so the client can show the person what they are asked to approve.
pub(crate) fn approval_required_response(estimated_pages: u64, threshold_pages: u64) -> Response {
    (
        StatusCode::CONFLICT,
        Json(error_with_details(
            "approval_required",
            "This crawl covers more pages than can start without approval. Confirm its page count and start it again.",
            json!({ "estimatedPages": estimated_pages, "thresholdPages": threshold_pages }),
        )),
    )
        .into_response()
}

/// `POST /api/v1/knowledge/crawl`: the approval gate in front of the crawl submit.
async fn start_crawl_gated(
    state: State<AppState>,
    user: Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Response {
    start_crawl_gated_with(crawl_approval_policy(), state, user, headers, body).await
}

async fn start_crawl_gated_with(
    policy: CrawlApprovalPolicy,
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    // A body the normaliser refuses is not a crawl yet: start_crawl answers it with the 400.
    let decision = if quarry::normalize_crawl_body(&body).is_ok() {
        decide_crawl_approval(&body, policy, quarry::Surface::Ui)
    } else {
        CrawlApprovalDecision::Proceed
    };
    match decision {
        CrawlApprovalDecision::Proceed => {}
        CrawlApprovalDecision::Observed { estimated_pages } => {
            // The page counts only: no URL, no organization and no user go into the log.
            tracing::info!(
                estimated_pages,
                threshold_pages = policy.threshold_pages,
                "crawl above the approval threshold (observe mode, not refused)"
            );
        }
        CrawlApprovalDecision::Refuse { estimated_pages } => {
            return approval_required_response(estimated_pages, policy.threshold_pages);
        }
    }
    quarry::start_crawl(State(state), Extension(user), headers, Json(body)).await
}

#[cfg(test)]
mod crawl_approval_tests {
    use super::quarry::Surface;
    use super::*;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ENFORCE: CrawlApprovalPolicy = CrawlApprovalPolicy {
        mode: CrawlApprovalMode::Enforce,
        threshold_pages: CRAWL_APPROVAL_PAGES_DEFAULT,
    };

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    #[test]
    fn the_threshold_setting_can_only_tighten() {
        assert_eq!(parse_crawl_approval_pages(None), 50);
        assert_eq!(parse_crawl_approval_pages(Some("20")), 20);
        assert_eq!(parse_crawl_approval_pages(Some(" 0 ")), 0);
        assert_eq!(parse_crawl_approval_pages(Some("50")), 50);
        assert_eq!(parse_crawl_approval_pages(Some("500")), 50);
        assert_eq!(parse_crawl_approval_pages(Some("-1")), 50);
        assert_eq!(parse_crawl_approval_pages(Some("many")), 50);
    }

    #[test]
    fn the_mode_is_observe_unless_set() {
        assert_eq!(parse_crawl_approval_mode(None), CrawlApprovalMode::Observe);
        assert_eq!(
            parse_crawl_approval_mode(Some(" Enforce ")),
            CrawlApprovalMode::Enforce
        );
        assert_eq!(
            parse_crawl_approval_mode(Some("off")),
            CrawlApprovalMode::Off
        );
        assert_eq!(
            parse_crawl_approval_mode(Some("on")),
            CrawlApprovalMode::Observe
        );
    }

    #[test]
    fn the_estimate_is_the_bound_held_to_the_ceiling() {
        let bounded = json!({ "url": "https://a.example/", "maxPages": 60 });
        let over_ceiling = json!({ "url": "https://a.example/", "max_pages": 999_999 });
        let unbounded = json!({ "url": "https://a.example/" });
        assert_eq!(estimated_crawl_pages(&bounded), 60);
        assert_eq!(estimated_crawl_pages(&over_ceiling), 5_000);
        assert_eq!(estimated_crawl_pages(&unbounded), 5_000);
    }

    #[test]
    fn a_crawl_at_the_threshold_proceeds_and_one_above_it_is_refused() {
        let at = json!({ "url": "https://a.example/", "maxPages": 50 });
        let above = json!({ "url": "https://a.example/", "maxPages": 51 });
        assert_eq!(
            decide_crawl_approval(&at, ENFORCE, Surface::Ui),
            CrawlApprovalDecision::Proceed
        );
        assert_eq!(
            decide_crawl_approval(&above, ENFORCE, Surface::Ui),
            CrawlApprovalDecision::Refuse {
                estimated_pages: 51
            }
        );
    }

    #[test]
    fn a_confirmation_must_cover_the_estimate() {
        let short = json!({ "url": "https://a.example/", "maxPages": 60, "approvedMaxPages": 50 });
        let covered =
            json!({ "url": "https://a.example/", "maxPages": 60, "approvedMaxPages": 60 });
        let wider =
            json!({ "url": "https://a.example/", "maxPages": 60, "approved_max_pages": 80 });
        assert!(matches!(
            decide_crawl_approval(&short, ENFORCE, Surface::Ui),
            CrawlApprovalDecision::Refuse { .. }
        ));
        assert_eq!(
            decide_crawl_approval(&covered, ENFORCE, Surface::Ui),
            CrawlApprovalDecision::Proceed
        );
        assert_eq!(
            decide_crawl_approval(&wider, ENFORCE, Surface::Ui),
            CrawlApprovalDecision::Proceed
        );
    }

    #[test]
    fn a_model_proposal_cannot_confirm_its_own_crawl() {
        let input = json!({ "url": "https://a.example/", "maxPages": 60, "approvedMaxPages": 60 });
        assert!(matches!(
            decide_crawl_approval(&input, ENFORCE, Surface::Model),
            CrawlApprovalDecision::Refuse { .. }
        ));
    }

    #[test]
    fn observe_logs_and_off_skips_but_neither_refuses() {
        let big = json!({ "url": "https://a.example/", "maxPages": 60 });
        let observe = CrawlApprovalPolicy {
            mode: CrawlApprovalMode::Observe,
            threshold_pages: 50,
        };
        let off = CrawlApprovalPolicy {
            mode: CrawlApprovalMode::Off,
            threshold_pages: 50,
        };
        assert_eq!(
            decide_crawl_approval(&big, observe, Surface::Ui),
            CrawlApprovalDecision::Observed {
                estimated_pages: 60
            }
        );
        assert_eq!(
            decide_crawl_approval(&big, off, Surface::Ui),
            CrawlApprovalDecision::Proceed
        );
    }

    #[test]
    fn a_curated_batch_is_not_a_whole_site_crawl() {
        let batch = json!({ "urls": ["https://a.example/1"], "maxPages": 500 });
        assert_eq!(
            decide_crawl_approval(&batch, ENFORCE, Surface::Ui),
            CrawlApprovalDecision::Proceed
        );
    }

    #[tokio::test]
    async fn an_enforced_crawl_above_the_threshold_never_reaches_the_edge() {
        let quarry = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        let response = start_crawl_gated_with(
            ENFORCE,
            State(state),
            Extension(user()),
            HeaderMap::new(),
            Json(json!({ "url": "https://example.com/", "maxPages": 60 })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "approval_required");
        assert_eq!(body["error"]["details"]["estimatedPages"], json!(60));
        assert_eq!(body["error"]["details"]["thresholdPages"], json!(50));
        assert!(quarry.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_confirmed_crawl_reaches_the_edge_without_the_confirmation_key() {
        let quarry = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/crawl"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "job_id": "job_1" })))
            .expect(1)
            .mount(&quarry)
            .await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        let response = start_crawl_gated_with(
            ENFORCE,
            State(state),
            Extension(user()),
            HeaderMap::new(),
            Json(json!({ "url": "https://example.com/", "maxPages": 60, "approvedMaxPages": 60 })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let received = quarry.received_requests().await.unwrap();
        let forwarded: Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(forwarded["max_pages"], json!(60));
        assert!(forwarded.get("approvedMaxPages").is_none());
        assert!(forwarded.get("approved_max_pages").is_none());
    }
}
#[cfg(test)]
mod crawl_control_route_tests {
    use super::*;
    use axum::{
        body::Body,
        extract::Extension,
        http::{Method as HttpMethod, Request, StatusCode},
    };
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::middleware::AuthenticatedUser;

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    /// The control routes plus the static routes that sit beside `{id}`, with the
    /// session user injected (the real stack adds it through `require_session`).
    fn app(state: AppState) -> Router {
        Router::new()
            .merge(crawl_control_routes())
            .route("/api/v1/knowledge/crawl/jobs", get(quarry::list_crawl_jobs))
            .route(
                "/api/v1/knowledge/crawl/discover",
                post(quarry::discover_crawl_pages),
            )
            .layer(Extension(user()))
            .with_state(state)
    }

    async fn call(app: Router, method: HttpMethod, uri: &str) -> StatusCode {
        app.oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
    }

    async fn mount(quarry: &MockServer, verb: &str, route: &str, status: u16) {
        Mock::given(wm_method(verb))
            .and(wm_path(route))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(serde_json::json!({ "data": {} })),
            )
            .expect(1)
            .mount(quarry)
            .await;
    }

    #[test]
    fn the_real_router_builds_with_the_control_routes_beside_the_static_ones() {
        // A conflicting route panics when the router is built.
        let _ = router(crate::tests::test_state(false));
    }

    #[tokio::test]
    async fn each_route_reaches_its_edge_path_and_ongoing_wins_over_the_id() {
        let quarry = MockServer::start().await;
        mount(&quarry, "GET", "/v1/crawl/ongoing", 200).await;
        mount(&quarry, "GET", "/v1/crawl/job_1", 200).await;
        mount(&quarry, "GET", "/v1/crawl/job_1/errors", 200).await;
        mount(&quarry, "POST", "/v1/crawl/job_1/pause", 200).await;
        mount(&quarry, "POST", "/v1/crawl/job_1/resume", 200).await;
        mount(&quarry, "DELETE", "/v1/crawl/job_1", 200).await;
        mount(&quarry, "GET", "/v1/crawl/jobs", 200).await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();

        for (method, uri) in [
            (HttpMethod::GET, "/api/v1/knowledge/crawl/ongoing"),
            (HttpMethod::GET, "/api/v1/knowledge/crawl/job_1"),
            (HttpMethod::GET, "/api/v1/knowledge/crawl/job_1/errors"),
            (HttpMethod::POST, "/api/v1/knowledge/crawl/job_1/pause"),
            (HttpMethod::POST, "/api/v1/knowledge/crawl/job_1/resume"),
            (HttpMethod::DELETE, "/api/v1/knowledge/crawl/job_1"),
            (HttpMethod::GET, "/api/v1/knowledge/crawl/jobs"),
        ] {
            assert_eq!(
                call(app(state.clone()), method.clone(), uri).await,
                StatusCode::OK,
                "{method} {uri}"
            );
        }
        // `.expect(1)` on every mock is checked when the server drops.
    }

    #[tokio::test]
    async fn a_verb_on_the_wrong_method_is_refused_without_an_upstream_call() {
        let quarry = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.quarry_edge_url = quarry.uri();
        for (method, uri) in [
            (HttpMethod::GET, "/api/v1/knowledge/crawl/job_1/pause"),
            (HttpMethod::POST, "/api/v1/knowledge/crawl/job_1"),
            (HttpMethod::DELETE, "/api/v1/knowledge/crawl/ongoing"),
            (HttpMethod::POST, "/api/v1/knowledge/crawl/job_1/errors"),
        ] {
            assert_eq!(
                call(app(state.clone()), method.clone(), uri).await,
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {uri}"
            );
        }
        assert!(quarry.received_requests().await.unwrap().is_empty());
    }
}
