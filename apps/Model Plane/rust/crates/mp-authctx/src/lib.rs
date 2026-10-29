//! Phase A · A1.3 — shared auth context for Model Plane Rust services.
//!
//! Mirrors `apps/Data Plane v2/services/documents-api-go/pkg/authctx`.
//! Validates the auth-core JWT carried in `Authorization: Bearer <jwt>`
//! against the JWKS published at
//! `${AUTH_CORE_JWKS_URL:-http://auth-core:3011/api/convex-auth/jwks}`,
//! checks audience + issuer + expiry, and stuffs the verified [`Claims`]
//! struct into the request extensions so downstream handlers can read
//! `org_id` / `user_id` / scopes without ever trusting a client header.
//!
//! Two modes, controlled via [`Config::enforce`] (or the
//! `AUTHCTX_ENFORCE` env var):
//!
//! - **Observe** (default): pass-through. Decode + (best-effort) verify
//!   the JWT if present, attach claims to extensions, log drift between
//!   the legacy `X-Org-ID` header and the JWT `org_id` claim. Requests
//!   without a JWT are forwarded unchanged so the legacy header path
//!   keeps working.
//!
//! - **Enforce**: every request must carry a JWT that verifies against
//!   the JWKS with the configured audience. Missing or invalid token →
//!   `401`. Verified `org_id` that disagrees with `X-Org-ID` → `403`.
//!
//! The JWKS is cached in-process with a TTL (default 15 min) and lazily
//! refreshed on cache miss.

#![allow(clippy::module_name_repetitions, clippy::items_after_statements)]

mod claims;
mod jwks;
mod layer;

pub use claims::{AuthCtx, Claims};
pub use jwks::JwksCache;
pub use layer::{AuthCtxLayer, AuthCtxMiddleware, Config};

/// Errors surfaced by the middleware. Most are mapped to HTTP status
/// codes via [`AuthCtxError::status`]; `Internal` is used when the JWKS
/// fetch fails in enforce mode (returns 503 so a wedged auth-core does
/// not cascade into a misleading 401).
#[derive(Debug, thiserror::Error)]
pub enum AuthCtxError {
    #[error("missing Authorization header")]
    MissingHeader,

    #[error("malformed Authorization header")]
    MalformedHeader,

    #[error("JWT decode failed: {0}")]
    Decode(String),

    #[error("JWT signature verification failed: {0}")]
    Signature(String),

    #[error("JWT audience mismatch (expected {expected:?}, got {got:?})")]
    AudienceMismatch { expected: String, got: String },

    #[error("JWT issuer mismatch (expected {expected:?}, got {got:?})")]
    IssuerMismatch { expected: String, got: String },

    #[error("JWT expired")]
    Expired,

    #[error("JWKS unavailable: {0}")]
    JwksUnavailable(String),

    #[error("X-Org-ID header ({header:?}) disagrees with JWT org_id ({jwt:?})")]
    OrgIdMismatch { header: String, jwt: String },
}

impl AuthCtxError {
    /// HTTP status code this error maps to in enforce mode.
    #[must_use]
    pub fn status(&self) -> http::StatusCode {
        match self {
            Self::MissingHeader
            | Self::MalformedHeader
            | Self::Decode(_)
            | Self::Signature(_)
            | Self::AudienceMismatch { .. }
            | Self::IssuerMismatch { .. }
            | Self::Expired => http::StatusCode::UNAUTHORIZED,
            Self::OrgIdMismatch { .. } => http::StatusCode::FORBIDDEN,
            Self::JwksUnavailable(_) => http::StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}
