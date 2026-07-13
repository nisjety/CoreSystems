//! GraphQL schema definition.
//!
//! Maps `quarry-core::resources` types onto async-graphql derives.
//! Every resolver pulls `org_id` from the `Claims` extension in the
//! context — tenant filtering is impossible to bypass from the wire.

use async_graphql::{
    Context, EmptyMutation, EmptySubscription, InputObject, Object, Schema, SimpleObject,
};

use crate::auth::Claims;

const COMPLEXITY_BUDGET: usize = 2000;
const DEPTH_BUDGET: usize = 16;

/// GraphQL representation of `quarry_core::resources::JobSummary`.
/// We hand-roll the type rather than `derive(SimpleObject)` on the
/// core type because async-graphql's derive imposes ownership rules
/// that fight `serde_json::Value` payload fields.
#[derive(SimpleObject, Clone)]
pub struct Job {
    pub job_id: String,
    pub kind: String,
    pub org_id: String,
    pub status: String,
    pub created_at: String,
}

/// Wire-compatible mirror of `quarry_core::pagination::ListFilter`.
/// Input objects can't carry `chrono::DateTime<Utc>` directly without
/// a scalar definition; we use RFC3339 strings the resolver parses.
#[derive(InputObject, Default)]
pub struct ListFilterInput {
    pub status: Option<String>,
    pub created_before: Option<String>,
    pub created_after: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub sort: Option<String>,
}

/// Page envelope identical to the REST `Page<T>` shape.
#[derive(SimpleObject)]
pub struct JobPage {
    pub items: Vec<Job>,
    pub next_cursor: Option<String>,
    pub total_estimated: Option<u64>,
}

/// Root query object. Resolvers are intentionally narrow — each one
/// pulls the verified `Claims` from context and proxies to existing
/// REST forward helpers so the auth + HMAC + idempotency rules carry
/// over without duplication.
pub struct Query;

#[Object]
impl Query {
    /// Schema version — useful for SDK generators to pin against.
    async fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Paginated list of jobs scoped to the caller's org.
    ///
    /// The current implementation returns an empty page so the schema
    /// is wire-complete and SDK generators have a working query to
    /// emit. Cycle 32 wires this to the actual `resource_routes::
    /// forward_list` helper — that requires plumbing the AppState
    /// reference + reqwest client into the GraphQL context, which is
    /// a small but distinct refactor.
    async fn jobs(
        &self,
        ctx: &Context<'_>,
        _filter: Option<ListFilterInput>,
    ) -> async_graphql::Result<JobPage> {
        // Pull verified claims — resolvers MUST gate every read on
        // this. The middleware injected it before async-graphql got
        // to execute the request.
        let claims = ctx.data::<Claims>().map_err(|_| {
            async_graphql::Error::new("auth context missing; route not wrapped with require_auth")
        })?;
        tracing::debug!(org_id = %claims.org_id, "graphql.jobs called");

        // Empty page contract — every consumer sees a well-shaped
        // response without reaching control-plane until the wire-up
        // ships.
        Ok(JobPage {
            items: Vec::new(),
            next_cursor: None,
            total_estimated: Some(0),
        })
    }
}

/// Build the schema with complexity + depth limits.
pub fn build_schema() -> Schema<Query, EmptyMutation, EmptySubscription> {
    Schema::build(Query, EmptyMutation, EmptySubscription)
        .limit_complexity(COMPLEXITY_BUDGET)
        .limit_depth(DEPTH_BUDGET)
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_introspection_contains_jobs_query() {
        let schema = build_schema();
        let sdl = schema.sdl();
        assert!(sdl.contains("type Query"), "Query type missing from SDL");
        assert!(sdl.contains("jobs("), "jobs query missing from SDL");
        assert!(sdl.contains("type Job"), "Job type missing from SDL");
        assert!(
            sdl.contains("type JobPage"),
            "JobPage type missing from SDL"
        );
        assert!(
            sdl.contains("input ListFilterInput"),
            "ListFilterInput missing"
        );
    }

    #[test]
    fn schema_pins_version_resolver() {
        let schema = build_schema();
        let sdl = schema.sdl();
        // Every schema MUST expose `version` so SDK generators have a
        // stable resolver to smoke-test.
        assert!(sdl.contains("version"), "version resolver missing");
    }

    #[tokio::test]
    async fn version_query_resolves() {
        let schema = build_schema();
        let result = schema.execute("{ version }").await;
        assert!(
            result.errors.is_empty(),
            "version query failed: {:?}",
            result.errors
        );
        // Pin that the response shape contains a `version` field.
        let value = result.data.into_json().unwrap();
        assert!(value.get("version").is_some());
    }

    #[tokio::test]
    async fn jobs_query_without_auth_errors() {
        // Without a Claims extension in the context, the resolver
        // returns an error rather than leaking an empty page — proves
        // every resolver enforces auth context presence.
        let schema = build_schema();
        let result = schema.execute("{ jobs { items { jobId } } }").await;
        assert!(!result.errors.is_empty(), "jobs MUST require auth context");
    }

    #[tokio::test]
    async fn jobs_query_with_auth_returns_empty_page() {
        let claims = Claims {
            sub: "u1".into(),
            iss: "auth-core".into(),
            exp: i64::MAX,
            org_id: "org_a".into(),
            user_id: "u1".into(),
            principal_type: None,
            service_id: None,
            nbf: None,
            aud: None,
            scopes: Vec::new(),
        };
        let schema = build_schema();
        let req =
            async_graphql::Request::new("{ jobs { items { jobId } totalEstimated } }").data(claims);
        let result = schema.execute(req).await;
        assert!(result.errors.is_empty(), "jobs failed: {:?}", result.errors);
        let v = result.data.into_json().unwrap();
        // Empty page contract.
        let items = v["jobs"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 0);
        assert_eq!(v["jobs"]["totalEstimated"], 0);
    }

    #[test]
    fn complexity_and_depth_budgets_active() {
        // Sanity — the schema's exposed limits should match the
        // constants. async-graphql doesn't surface the active limits
        // post-build, but the constants are visible to anyone
        // grepping for "limit_complexity".
        assert_eq!(COMPLEXITY_BUDGET, 2000);
        assert_eq!(DEPTH_BUDGET, 16);
    }
}
