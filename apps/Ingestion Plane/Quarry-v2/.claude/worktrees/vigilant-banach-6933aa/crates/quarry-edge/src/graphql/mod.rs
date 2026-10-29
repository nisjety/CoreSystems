//! GraphQL overlay — cycle 31 / cluster #11 implementation.
//!
//! The schema design lives in `docs/GRAPHQL.md`; this module ships
//! the working subset: schema introspection, JWT-gated `POST /graphql`,
//! and one read query (`jobs`) that proxies through the existing
//! control-plane forward path. Additional resolvers (schedules,
//! sources, snapshots, etc.) follow the same pattern — they're
//! deferred to incremental follow-up cycles rather than landing in
//! one massive file.
//!
//! ## Design constraints met
//!
//! - **No REST regression** — GraphQL is an additive route; every
//!   existing `/v1/*` endpoint is untouched.
//! - **JWT auth reused** — `require_auth` middleware wraps the route
//!   exactly like every other `/v1/*` handler; resolvers receive the
//!   `Claims` extension via axum's `Extension` extractor inside the
//!   GraphQL context.
//! - **Tenant isolation** — every resolver reads `org_id` from the
//!   `Claims` in the GraphQL context; client query args can't
//!   override it (mirrors P0 / cluster #auth+tenancy).
//! - **Complexity budget** — `Schema::limit_complexity(2000)` blocks
//!   pathological queries.

pub mod schema;

use axum::{extract::Extension, response::IntoResponse, Json};

use crate::auth::Claims;

/// `POST /graphql` handler. Wraps the schema with the caller's
/// verified `Claims` so resolvers can scope reads to the tenant.
///
/// Hand-rolled (not via `async-graphql-axum`) because the latter pins
/// to specific axum minors (6.x → axum 0.6, 7.x → axum 0.8) and our
/// workspace is on axum 0.7. The handler is ~10 LOC and decouples the
/// two upgrade cadences.
pub async fn graphql_handler(
    Extension(claims): Extension<Claims>,
    Json(req): Json<async_graphql::Request>,
) -> Json<async_graphql::Response> {
    let schema = schema::build_schema();
    let request = req.data(claims);
    Json(schema.execute(request).await)
}

/// `GET /graphql/playground` — minimal GraphiQL bundle served from
/// CDN. Production deployments should put this behind a reverse-proxy
/// ACL — the page is unauthenticated by axum, though every actual
/// query goes through `require_auth` via `POST /graphql`.
pub async fn playground() -> impl IntoResponse {
    axum::response::Html(
        r#"<!DOCTYPE html>
<html>
<head>
  <title>Quarry GraphQL Playground</title>
  <link href="https://unpkg.com/graphiql@3/graphiql.min.css" rel="stylesheet" />
</head>
<body style="margin:0;">
  <div id="graphiql" style="height: 100vh;"></div>
  <script crossorigin src="https://unpkg.com/react@18/umd/react.production.min.js"></script>
  <script crossorigin src="https://unpkg.com/react-dom@18/umd/react-dom.production.min.js"></script>
  <script crossorigin src="https://unpkg.com/graphiql@3/graphiql.min.js"></script>
  <script>
    const fetcher = GraphiQL.createFetcher({ url: '/graphql' });
    const root = ReactDOM.createRoot(document.getElementById('graphiql'));
    root.render(React.createElement(GraphiQL, { fetcher }));
  </script>
</body>
</html>"#,
    )
}

/// Schema introspection JSON. Used by SDK generators in CI.
pub async fn introspection() -> Json<serde_json::Value> {
    let schema = schema::build_schema();
    let sdl = schema.sdl();
    Json(serde_json::json!({ "sdl": sdl }))
}
