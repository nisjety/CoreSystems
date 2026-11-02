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

use axum::{
    routing::{get, post},
    Router,
};

use crate::{
    config::AppState,
    middleware::require_session,
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
        .route("/api/v1/knowledge/crawl", post(quarry::start_crawl));
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
