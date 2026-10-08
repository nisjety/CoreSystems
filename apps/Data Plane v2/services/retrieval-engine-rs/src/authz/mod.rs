//! Wave 3 — Control Plane authorization.
//!
//! `AuthContext` is the canonical "who is calling and what are they allowed
//! to see" struct, populated by the HTTP middleware / gRPC interceptor at
//! request entry and read by the retrieval pipeline before any data access.
//!
//! Three pieces:
//!   - [`AuthContext`]   — user_id, org_id, scopes, effective ACL.
//!   - [`Claims`]        — JWT claims decoded from the Bearer header.
//!   - [`PolicyClient`]  — abstraction over Control's versioned canonical
//!                          membership decision. Two impls:
//!         * [`NoopPolicyClient`]   — local dev: pass-through allow-all.
//!         * [`HttpPolicyClient`]   — production: HTTP calls to the
//!                                     Control Plane using an org-bound,
//!                                     scoped service bearer and a short
//!                                     positive-only TTL cache.
//!
//! The `CONTROL_PLANE_ENFORCEMENT` env var selects the impl:
//!   - `off`        → NoopPolicyClient only with the explicit insecure-dev gate
//!   - `strict`     → HttpPolicyClient, fail-closed if Control Plane is down
//!   - `permissive` → retained config alias for HttpPolicyClient; still fail-closed
//!
//! Two kinds of verified caller are admitted differently (QB-T03):
//!   - [`principal`]       — identity classification, the single admission
//!                           decision ([`principal::admit`]) and the HTTP guard.
//!                           Users: Control membership. Services: the default-
//!                           deny route allowlist.
//!   - [`service_routes`]  — that allowlist: which service may call which
//!                           `(method, route pattern)` with which scope.
//!   - [`jwt`]             — the real bearer validation, shared with the tests.
//!
//! Closes §15-A `AuthContext`, §15-B `MembershipCache`, §15-C
//! `PermissionResolver` foundation.

pub mod context;
pub mod jwks;
pub mod jwt;
pub mod policy;
pub mod principal;
pub mod service_routes;
pub mod taxonomy;
pub mod visibility;

#[allow(unused_imports)]
pub use context::{
    pin_org_from_ctx, AuthContext, AuthMethod, Claims, EffectiveAcl, OrgScopeMismatch,
};
pub use jwks::JwksCache;
#[allow(unused_imports)] // the verifier seam is consumed by the crate's integration tests
pub use jwt::{
    bearer_token, decode_verified_claims, strict_validation_for, ClaimsVerifier, EnvClaimsVerifier,
};
#[allow(unused_imports)]
// PolicyDecision is part of the public surface; consumers land in later wiring
pub use policy::{
    EnforcementMode, HttpPolicyClient, NoopPolicyClient, PolicyClient, PolicyDecision,
};
#[allow(unused_imports)] // the admission surface is consumed by the crate's integration tests
pub use principal::{
    admit, auth_context_from_verified_claims, classify_principal, guard_middleware,
    resolve_explicit_grants, Admission, AdmissionRequest, Admitted, AuthGuard, Denial, Principal,
    PrincipalShapeError,
};
#[allow(unused_imports)]
pub use visibility::{HttpVisibilityClient, NoopVisibilityClient, VisibilityClient};
