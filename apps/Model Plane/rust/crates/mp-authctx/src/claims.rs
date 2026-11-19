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
    /// True if `scope` is present in the claims. It checks the list only, not verification, so
    /// reach the claims through [`AuthCtx::verified_claims`] before any authorisation decision.
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

/// How far the claims on an [`AuthCtx`] can be trusted.
///
/// Authorisation reads claims only through [`AuthCtx::verified_claims`], which yields them for
/// [`Verification::Verified`] and nothing otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// The signature, audience, issuer and expiry passed JWKS validation.
    Verified,
    /// The token failed verification. Observe mode attaches the claims read from it without a
    /// check, for telemetry only. Nothing may authorise on them.
    Unverified,
}

/// Request-scoped wrapper attached to the axum request extensions.
///
/// The claims are private, so every reader meets the marker. [`AuthCtx::verified_claims`] returns
/// them only when [`AuthCtx::verification`] is [`Verification::Verified`]. [`AuthCtx::unverified_claims`]
/// returns them only for an observe-mode context. Exactly one of the two yields, so a handler that
/// asks for a verified user or org gets `None` from an unverified context instead of a forged value.
#[derive(Debug, Clone)]
pub struct AuthCtx {
    claims: Claims,
    verification: Verification,
}

impl AuthCtx {
    /// A context whose claims passed JWKS validation.
    #[must_use]
    pub const fn verified(claims: Claims) -> Self {
        Self {
            claims,
            verification: Verification::Verified,
        }
    }

    /// The observe-mode context for a token that failed verification. Its claims are unverified.
    #[must_use]
    pub const fn observe(claims: Claims) -> Self {
        Self {
            claims,
            verification: Verification::Unverified,
        }
    }

    /// Whether the claims passed JWKS validation.
    #[must_use]
    pub const fn verification(&self) -> Verification {
        self.verification
    }

    /// True only when the claims passed JWKS validation.
    #[must_use]
    pub const fn is_verified(&self) -> bool {
        matches!(self.verification, Verification::Verified)
    }

    /// The claims, only when they passed JWKS validation. Authorisation decisions read claims here.
    #[must_use]
    pub fn verified_claims(&self) -> Option<&Claims> {
        self.is_verified().then_some(&self.claims)
    }

    /// The claims read from a token that failed verification. Telemetry and drift logs only: never
    /// authorise on these. `None` for a verified context.
    #[must_use]
    pub fn unverified_claims(&self) -> Option<&Claims> {
        (!self.is_verified()).then_some(&self.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthCtx, Claims, Verification};

    fn fixture_claims() -> Claims {
        Claims {
            user_id: "user-fixture".to_owned(),
            org_id: "org-fixture".to_owned(),
            email: None,
            scopes: Vec::new(),
            iss: "https://auth.fixture.invalid/api/convex-auth".to_owned(),
            aud: "model-plane".to_owned(),
            sub: "user-fixture".to_owned(),
            iat: 1,
            nbf: None,
            exp: 4_102_444_800,
        }
    }

    #[test]
    fn a_verified_context_yields_its_claims_and_no_unverified_ones() {
        let ctx = AuthCtx::verified(fixture_claims());
        assert_eq!(ctx.verification(), Verification::Verified);
        assert!(ctx.is_verified());
        assert_eq!(
            ctx.verified_claims().map(|claims| claims.org_id.as_str()),
            Some("org-fixture")
        );
        assert!(ctx.unverified_claims().is_none());
    }

    #[test]
    fn an_observe_context_is_marked_unverified_and_withholds_verified_claims() {
        let ctx = AuthCtx::observe(fixture_claims());
        assert_eq!(ctx.verification(), Verification::Unverified);
        assert!(!ctx.is_verified());
        assert!(
            ctx.verified_claims().is_none(),
            "unverified claims must not read as verified"
        );
        assert_eq!(
            ctx.unverified_claims().map(|claims| claims.org_id.as_str()),
            Some("org-fixture")
        );
        assert!(
            format!("{ctx:?}").contains("Unverified"),
            "the debug form names the trust level plainly"
        );
    }
}
