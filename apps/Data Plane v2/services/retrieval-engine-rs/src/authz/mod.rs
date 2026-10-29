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
//! Closes §15-A `AuthContext`, §15-B `MembershipCache`, §15-C
//! `PermissionResolver` foundation.

pub mod context;
pub mod jwks;
pub mod policy;
pub mod taxonomy;
pub mod visibility;

#[allow(unused_imports)]
pub use context::{
    pin_org_from_ctx, AuthContext, AuthMethod, Claims, EffectiveAcl, OrgScopeMismatch,
};
pub use jwks::JwksCache;
#[allow(unused_imports)]
// PolicyDecision is part of the public surface; consumers land in later wiring
pub use policy::{
    EnforcementMode, HttpPolicyClient, NoopPolicyClient, PolicyClient, PolicyDecision,
};
#[allow(unused_imports)]
pub use visibility::{HttpVisibilityClient, NoopVisibilityClient, VisibilityClient};
