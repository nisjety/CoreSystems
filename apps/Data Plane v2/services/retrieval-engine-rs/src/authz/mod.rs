//! Wave 3 — Control Plane authorization.
//!
//! `AuthContext` is the canonical "who is calling and what are they allowed
//! to see" struct, populated by the HTTP middleware / gRPC interceptor at
//! request entry and read by the retrieval pipeline before any data access.
//!
//! Three pieces:
//!   - [`AuthContext`]   — user_id, org_id, scopes, effective ACL.
//!   - [`Claims`]        — JWT claims decoded from the Bearer header.
//!   - [`PolicyClient`]  — abstraction over `user-service` (membership)
//!                          + `org-core` (permissions). Two impls:
//!         * [`NoopPolicyClient`]   — local dev: pass-through allow-all.
//!         * [`HttpPolicyClient`]   — production: HTTP calls to the
//!                                     Control Plane services with a
//!                                     Redis-compatible TTL cache.
//!
//! The `CONTROL_PLANE_ENFORCEMENT` env var selects the impl:
//!   - `off`        → NoopPolicyClient (default for v2.3; back-compat)
//!   - `strict`     → HttpPolicyClient, fail-closed if Control Plane down
//!   - `permissive` → HttpPolicyClient, fail-open on Control Plane errors
//!                    (degraded mode for the rollout window)
//!
//! Closes §15-A `AuthContext`, §15-B `MembershipCache`, §15-C
//! `PermissionResolver` foundation. The protobuf contracts for
//! `user-service.CheckMembership` and `org-core.GetUserPermissions` are
//! still pending — once they land, swap `HttpPolicyClient` for a
//! `GrpcPolicyClient` without changing the trait.

pub mod context;
pub mod jwks;
pub mod policy;
pub mod taxonomy;
pub mod visibility;

pub use context::{pin_org_from_ctx, AuthContext, AuthMethod, Claims, EffectiveAcl};
pub use jwks::JwksCache;
#[allow(unused_imports)]
// PolicyDecision is part of the public surface; consumers land in later wiring
pub use policy::{
    EnforcementMode, HttpPolicyClient, NoopPolicyClient, PolicyClient, PolicyDecision,
};
#[allow(unused_imports)]
pub use visibility::{HttpVisibilityClient, NoopVisibilityClient, VisibilityClient};
