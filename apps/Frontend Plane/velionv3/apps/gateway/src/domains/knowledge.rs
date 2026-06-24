mod diagnostics;
mod documents;
mod enhanced_fetch;
mod imports;
mod operating_map;
mod products;
mod quarry;
mod retrieval;
mod shared;
mod sync;
mod wiki;
mod workspace;

use axum::{
    routing::{get, post},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        // Documents
        .route(
            "/api/v1/knowledge/documents",
            get(documents::list_documents).post(documents::create_document),
        )
        .route(
            "/api/v1/knowledge/documents/:id",
            get(documents::get_document),
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
        // Retrieval
        .route(
            "/api/v1/knowledge/search",
            post(retrieval::search_knowledge),
        )
        .route(
            "/api/v1/knowledge/retrieval/:trace_id",
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
            "/api/v1/knowledge/operating-map/runs/:run_id/events",
            get(operating_map::operating_map_run_events),
        )
        .route(
            "/api/v1/knowledge/operating-map/proposals/:proposal_id/review",
            post(operating_map::review_operating_map_proposal),
        )
        // Wiki — canonical knowledge namespace. Static "by-path" must come
        // before the :id param route.
        .route(
            "/api/v1/knowledge/wiki/pages/by-path",
            get(wiki::wiki_page_by_path),
        )
        .route("/api/v1/knowledge/wiki/pages", get(wiki::list_wiki_pages))
        .route("/api/v1/knowledge/wiki/pages/:id", get(wiki::get_wiki_page))
        .route(
            "/api/v1/knowledge/wiki/pages/:id/versions",
            get(wiki::wiki_page_versions),
        )
        .route(
            "/api/v1/knowledge/wiki/pages/:id/diff",
            get(wiki::wiki_page_diff),
        )
        .route(
            "/api/v1/knowledge/wiki/pages/:id/backlinks",
            get(wiki::wiki_page_backlinks),
        )
        // Legacy aliases kept for existing callers.
        .route("/api/v1/wiki/pages/by-path", get(wiki::wiki_page_by_path))
        .route("/api/v1/wiki/pages", get(wiki::list_wiki_pages))
        .route("/api/v1/wiki/pages/:id", get(wiki::get_wiki_page))
        .route(
            "/api/v1/wiki/pages/:id/versions",
            get(wiki::wiki_page_versions),
        )
        .route("/api/v1/wiki/pages/:id/diff", get(wiki::wiki_page_diff))
        .route(
            "/api/v1/wiki/pages/:id/backlinks",
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
            "/api/v1/knowledge/imports/:id",
            get(imports::get_import_job),
        )
        .route(
            "/api/v1/knowledge/imports/:id/events",
            get(imports::import_job_events),
        )
        // Quarry
        .route(
            "/api/v1/knowledge/scrape-preview",
            post(quarry::scrape_preview),
        )
        .route("/api/v1/knowledge/scrape", post(quarry::scrape))
        .route(
            "/api/v1/knowledge/scrape/products",
            post(products::extract_products),
        )
        .route(
            "/api/v1/knowledge/scrape/products/summary",
            post(products::summarize_products),
        )
        .route(
            "/api/v1/knowledge/crawl/discover",
            post(quarry::discover_crawl_pages),
        )
        .route("/api/v1/knowledge/crawl", post(quarry::start_crawl))
        .route("/api/v1/knowledge/crawl/jobs", get(quarry::list_crawl_jobs))
        .route(
            "/api/v1/knowledge/runs/:id/events",
            get(quarry::crawl_run_events),
        )
        // Crawl 0-pages fix — durable crawl event stream keyed off the
        // handoff job_id (the normalized handoff's `eventStream` points
        // here). The edge emits real SSE; this forwards it verbatim.
        .route(
            "/api/v1/knowledge/jobs/:id/events",
            get(quarry::crawl_job_events),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
