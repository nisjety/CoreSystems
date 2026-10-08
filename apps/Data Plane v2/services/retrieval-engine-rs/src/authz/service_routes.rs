//! Default-deny route allowlist for SERVICE principals.
//!
//! A caller whose verified token says `principal_type=service` (Auth Core:
//! `sub == service_id == "service:<name>"`, no `user_id`) is not an organization
//! member, so it can never pass the membership decision that gates interactive
//! users: Auth Core's real answer for a service subject is `not_member`. That
//! mismatch is what answered 403 to every quarry-edge `POST /v1/retrieve`
//! (live, 2026-10-01).
//!
//! The fix is not to loosen membership. A service is admitted by NAMING: this
//! table lists, per service id, the exact `(method, matched route pattern)` it
//! may call and the scope the token must carry for that call. Everything not
//! listed is refused, so adding a route to the router can never silently expose
//! it to a service. The key is the axum `MatchedPath` pattern, never the raw
//! URI path, so percent-encoding, case, trailing-slash and `//` spellings
//! cannot reach a different decision than the route they resolve to.
//!
//! The table is policy and lives in code on purpose: it is reviewed with the
//! handler it admits, and `tests/service_principal_auth.rs` sweeps every route
//! the real router registers against it. Scopes are issued by Auth Core's
//! registry (`config/plane-service-principals.json`); this table only decides
//! which scope a route demands.
//!
//! Matrix exception (rule 9, "identity and membership are Control Plane-owned"):
//! admitting a service without a membership lookup is intentional and bounded
//! to the rows below. Tenancy for the service itself is Control's registry
//! (`orgIds` / `allowAnyOrg`) at token issuance; the engine pins every request
//! to the token's `org_id` exactly as it does for users.

/// Prefix every Auth Core service subject carries.
pub const SERVICE_SUBJECT_PREFIX: &str = "service:";

/// Read retrieval over the caller's organization.
pub const SCOPE_DATA_READ: &str = "data:read";
/// Rerank caller-supplied candidates (`POST /v1/rerank`).
pub const SCOPE_DATA_RERANK: &str = "data:rerank";

/// One service, one route, one scope. Exact match only: there are no wildcards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceRouteGrant {
    pub service_id: &'static str,
    pub method: &'static str,
    /// The router's route PATTERN (axum `MatchedPath`), e.g. `/v1/retrieve`.
    pub path: &'static str,
    /// Scope the token must carry for this call.
    pub scope: &'static str,
}

const fn grant(
    service_id: &'static str,
    method: &'static str,
    path: &'static str,
    scope: &'static str,
) -> ServiceRouteGrant {
    ServiceRouteGrant {
        service_id,
        method,
        path,
        scope,
    }
}

/// Everything a service may call. Anything absent is refused (403).
pub const SERVICE_ROUTE_GRANTS: &[ServiceRouteGrant] = &[
    // Quarry's retrieval over verified organization evidence.
    grant(
        "service:quarry-edge",
        "POST",
        "/v1/retrieve",
        SCOPE_DATA_READ,
    ),
    grant(
        "service:quarry-edge",
        "POST",
        "/v1/retrieve/hybrid",
        SCOPE_DATA_READ,
    ),
    grant(
        "service:quarry-edge",
        "POST",
        "/v1/knowledge/search",
        SCOPE_DATA_READ,
    ),
    // Option B: the Data Plane owns reranking of caller-supplied candidates
    // (the handler is registered by the rerank task; the decision for it lives
    // here so the route is never reachable by a service without its own scope).
    grant(
        "service:quarry-edge",
        "POST",
        "/v1/rerank",
        SCOPE_DATA_RERANK,
    ),
];

/// Routes that exist for service callers only. An interactive user is refused
/// here BEFORE any membership lookup: these routes carry no per-user visibility
/// semantics, so a user token reaching one would be an authority nobody granted.
pub const SERVICE_ONLY_PATHS: &[&str] = &["/v1/rerank"];

/// The only scopes a service's request context ever carries. Every other scope
/// in the token (notably the org-wide `org:data:read_all` / `org:data:write_all`
/// visibility scopes a registry ceiling may include) is dropped at admission, so
/// no downstream `scopes.contains(..)` check can widen a service's reach.
pub const SERVICE_CONTEXT_SCOPES: &[&str] = &[SCOPE_DATA_READ, SCOPE_DATA_RERANK];

/// What the table says about one `(service, method, route)` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceRouteDecision {
    Allowed,
    /// The service is not named for this route (or at all, or the route did not
    /// resolve): default deny.
    RouteNotAllowed,
    /// The route is granted to the service but the token lacks the scope.
    ScopeMissing,
}

/// Decide a service call. `matched_path` is the router's pattern; `None` (the
/// request resolved to no route pattern) is refused.
pub fn decide(
    service_id: &str,
    method: &str,
    matched_path: Option<&str>,
    token_scopes: &[String],
) -> ServiceRouteDecision {
    let Some(path) = matched_path else {
        return ServiceRouteDecision::RouteNotAllowed;
    };
    let Some(grant) = SERVICE_ROUTE_GRANTS
        .iter()
        .find(|g| g.service_id == service_id && g.method == method && g.path == path)
    else {
        return ServiceRouteDecision::RouteNotAllowed;
    };
    if token_scopes.iter().any(|scope| scope == grant.scope) {
        ServiceRouteDecision::Allowed
    } else {
        ServiceRouteDecision::ScopeMissing
    }
}

/// True for a route interactive users may not call.
pub fn is_service_only(matched_path: Option<&str>) -> bool {
    matched_path.is_some_and(|path| SERVICE_ONLY_PATHS.contains(&path))
}

/// The subset of a token's scopes a service context may carry.
pub fn service_context_scopes(token_scopes: &[String]) -> Vec<String> {
    token_scopes
        .iter()
        .filter(|scope| SERVICE_CONTEXT_SCOPES.contains(&scope.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scopes(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn the_table_is_well_formed_and_has_no_duplicate_keys() {
        let mut seen = std::collections::HashSet::new();
        for g in SERVICE_ROUTE_GRANTS {
            assert!(g.service_id.starts_with(SERVICE_SUBJECT_PREFIX), "{g:?}");
            assert!(g.path.starts_with('/'), "{g:?}");
            assert!(!g.path.contains('*'), "no wildcard grants: {g:?}");
            assert!(g.method.chars().all(|c| c.is_ascii_uppercase()), "{g:?}");
            assert!(
                SERVICE_CONTEXT_SCOPES.contains(&g.scope),
                "a granted scope must be one a service context carries: {g:?}"
            );
            assert!(
                seen.insert((g.service_id, g.method, g.path)),
                "duplicate key would make the scope check ambiguous: {g:?}"
            );
        }
    }

    #[test]
    fn every_service_only_route_is_granted_to_some_service() {
        for path in SERVICE_ONLY_PATHS {
            assert!(
                SERVICE_ROUTE_GRANTS.iter().any(|g| g.path == *path),
                "{path} is service-only but no service may call it"
            );
        }
    }

    #[test]
    fn decide_is_exact_match_and_default_deny() {
        let read = scopes(&["data:read"]);
        assert_eq!(
            decide("service:quarry-edge", "POST", Some("/v1/retrieve"), &read),
            ServiceRouteDecision::Allowed
        );
        // Wrong method, wrong route, unknown service, unresolved route.
        for (service, method, path) in [
            ("service:quarry-edge", "GET", Some("/v1/retrieve")),
            ("service:quarry-edge", "POST", Some("/v1/retrieve/graph")),
            ("service:quarry-edge", "POST", Some("/v1/retrieve/")),
            ("service:quarry-edge", "POST", Some("/V1/retrieve")),
            ("service:quarry-edge", "POST", None),
            ("service:other", "POST", Some("/v1/retrieve")),
        ] {
            assert_eq!(
                decide(service, method, path, &read),
                ServiceRouteDecision::RouteNotAllowed,
                "{service} {method} {path:?}"
            );
        }
    }

    #[test]
    fn decide_demands_the_scope_the_route_names() {
        assert_eq!(
            decide(
                "service:quarry-edge",
                "POST",
                Some("/v1/retrieve"),
                &scopes(&["data:rerank"])
            ),
            ServiceRouteDecision::ScopeMissing
        );
        assert_eq!(
            decide(
                "service:quarry-edge",
                "POST",
                Some("/v1/rerank"),
                &scopes(&["data:read"])
            ),
            ServiceRouteDecision::ScopeMissing
        );
        assert_eq!(
            decide("service:quarry-edge", "POST", Some("/v1/rerank"), &[]),
            ServiceRouteDecision::ScopeMissing
        );
        assert_eq!(
            decide(
                "service:quarry-edge",
                "POST",
                Some("/v1/rerank"),
                &scopes(&["data:rerank"])
            ),
            ServiceRouteDecision::Allowed
        );
    }

    #[test]
    fn service_context_keeps_only_the_scopes_a_service_may_hold() {
        let kept = service_context_scopes(&scopes(&[
            "data:read",
            "documents:write",
            "org:data:read_all",
            "org:data:write_all",
            "data:rerank",
            "admin",
        ]));
        assert_eq!(kept, scopes(&["data:read", "data:rerank"]));
    }

    #[test]
    fn service_only_paths_match_exactly() {
        assert!(is_service_only(Some("/v1/rerank")));
        assert!(!is_service_only(Some("/v1/rerank/")));
        assert!(!is_service_only(Some("/v1/retrieve")));
        assert!(!is_service_only(None));
    }
}
