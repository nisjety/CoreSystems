//! Bearer-token verification for the HTTP surface.
//!
//! Moved here from `api/mod.rs` UNCHANGED so the same validator that runs in
//! production is reachable from the crate's integration tests: `exp`, `nbf`,
//! `aud`, `iss`, `sub` and the signed `zdr` posture are all required, RS256 is
//! the only algorithm, and identities must be canonical. The tests sign tokens
//! with their own key and run them through [`decode_verified_claims`]; only the
//! key source differs from production ([`EnvClaimsVerifier`]).

use axum::http::{HeaderMap, StatusCode};

use super::context::Claims;

/// Verifies a bearer token and returns its claims. The seam exists so a test can
/// supply its own verification KEY while still running the real validation
/// rules; it is not a place to relax them.
#[async_trait::async_trait]
pub trait ClaimsVerifier: Send + Sync {
    async fn verify(&self, token: &str) -> Result<Claims, StatusCode>;
}

/// Production verifier: `JWT_REQUIRED_ISSUER` / `JWT_REQUIRED_AUDIENCE`, a JWKS
/// key by `kid` when one is cached, otherwise the pinned `JWT_PUBLIC_KEY_PEM`.
pub struct EnvClaimsVerifier;

#[async_trait::async_trait]
impl ClaimsVerifier for EnvClaimsVerifier {
    async fn verify(&self, token: &str) -> Result<Claims, StatusCode> {
        verify_jwt(token).await
    }
}

/// `Authorization: Bearer <token>` -> the token. Case-sensitive scheme, as before.
pub fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get("authorization").and_then(|v| v.to_str().ok())?;
    value.strip_prefix("Bearer ").map(|s| s.to_string())
}

fn strict_validation_from_env() -> Result<jsonwebtoken::Validation, StatusCode> {
    let issuer = std::env::var("JWT_REQUIRED_ISSUER")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let audience = std::env::var("JWT_REQUIRED_AUDIENCE")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    strict_validation_for(&issuer, &audience)
}

pub fn strict_validation_for(
    issuer: &str,
    audience: &str,
) -> Result<jsonwebtoken::Validation, StatusCode> {
    if issuer.trim().is_empty()
        || issuer != issuer.trim()
        || audience.trim().is_empty()
        || audience != audience.trim()
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    // "sovereign" deliberately NOT required here — see `Claims::sovereign`'s
    // doc comment: auth-core does not emit it yet, and requiring it would
    // reject every real token in production.
    validation.set_required_spec_claims(&["exp", "nbf", "aud", "iss", "sub", "zdr"]);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.leeway = 30;
    Ok(validation)
}

pub fn decode_verified_claims(
    token: &str,
    key: &jsonwebtoken::DecodingKey,
    validation: &jsonwebtoken::Validation,
) -> Result<Claims, StatusCode> {
    let header = jsonwebtoken::decode_header(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
    if header.alg != jsonwebtoken::Algorithm::RS256 {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let claims = jsonwebtoken::decode::<Claims>(token, key, validation)
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .claims;
    let org_id = claims
        .org_id
        .as_ref()
        .filter(|org| !org.trim().is_empty() && org.trim() == org.as_str())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if claims.sub.trim().is_empty() || claims.sub.trim() != claims.sub {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if org_id.is_empty() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(claims)
}

/// Verify a JWT against `JWT_PUBLIC_KEY_PEM` (RS256). Returns the decoded
/// claims so the caller can build `AuthContext` from them. JWKS fetching is
/// the wave-3.1 follow-up — most internal deployments pin a single key.
async fn verify_jwt(token: &str) -> Result<Claims, StatusCode> {
    // §16.5.5 — if a JWKS cache is initialized and the token's `kid`
    // resolves there, use that key. Otherwise fall through to the static
    // `JWT_PUBLIC_KEY_PEM` path (pre-3.4 behavior).
    let validation = strict_validation_from_env()?;

    if let Some(jwks) = super::JwksCache::global() {
        if let Ok(header) = jsonwebtoken::decode_header(token) {
            if let Some(kid) = header.kid {
                if let Some(key) = jwks.key_for_kid(&kid).await {
                    return decode_verified_claims(token, &key, &validation).inspect_err(|error| {
                        tracing::debug!(?error, kid = %kid, "jwks jwt verify failed");
                    });
                }
            }
        }
    }

    let pem = std::env::var("JWT_PUBLIC_KEY_PEM").ok();
    let pem = pem
        .filter(|p| !p.is_empty())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let key = jsonwebtoken::DecodingKey::from_rsa_pem(pem.as_bytes())
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    decode_verified_claims(token, &key, &validation).inspect_err(|error| {
        tracing::debug!(?error, "jwt verification failed");
    })
}
