use serde::{Deserialize, Serialize};

/// Canonical claim shape the Model Plane services read out of the
/// auth-core JWT. Matches the issuance side
/// (`apps/Control Plane/auth-core/src/auth/convex-token.service.ts ::
/// issuePlaneToken`) plus the existing model-gateway `Claims` struct
/// (which already expects `org_id` + `user_id` `snake_case` top-level
/// claims).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Claims {
    #[serde(rename = "user_id")]
    pub user_id: String,

    #[serde(rename = "org_id")]
    pub org_id: String,

    pub email: Option<String>,

    #[serde(default)]
    pub scopes: Vec<String>,

    pub iss: String,

    pub aud: String,

    pub sub: String,

    pub iat: i64,

    #[serde(default)]
    pub nbf: Option<i64>,

    pub exp: i64,
}

impl Claims {
    /// True if `scope` is present in the claims. Always false on
    /// unverified claims — callers must consult [`AuthCtx::verified`]
    /// before calling this.
    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// True when `exp` lies in the past with a small clock-skew grace
    /// window. Matches the verevon mint-side 30-second refresh-safety so
    /// both sides treat the boundary consistently.
    #[must_use]
    pub fn is_expired(&self, now_secs: i64) -> bool {
        now_secs > self.exp + 30
    }
}

/// Request-scoped wrapper attached to the axum request extensions.
/// `verified` is `true` only when the signature passed JWKS validation;
/// observe-mode middleware sets it to `false` so handlers can refuse
/// to make authorisation decisions on the data.
#[derive(Debug, Clone)]
pub struct AuthCtx {
    pub claims: Claims,
    pub verified: bool,
}

impl AuthCtx {
    #[must_use]
    pub const fn verified(claims: Claims) -> Self {
        Self {
            claims,
            verified: true,
        }
    }

    #[must_use]
    pub const fn observe(claims: Claims) -> Self {
        Self {
            claims,
            verified: false,
        }
    }
}
