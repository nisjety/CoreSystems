//! Verifies the Xero OIDC identity issued during a user-facing PKCE flow.

use std::time::Duration;

use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde_json::Value;
use xero_common::Error;

const ISSUER: &str = "https://identity.xero.com";
const JWKS_URL: &str = "https://identity.xero.com/.well-known/openid-configuration/jwks";
const MAX_JWKS_BYTES: usize = 128 * 1024;
const MAX_TOKEN_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XeroIdentity {
    pub subject: String,
    pub email: Option<String>,
}

fn invalid_identity() -> Error {
    Error::Auth("Xero identity could not be verified".into())
}

pub async fn verify_xero_id_token(
    http: &reqwest::Client,
    token: &str,
    client_id: &str,
    expected_nonce: &str,
) -> Result<XeroIdentity, Error> {
    let response = tokio::time::timeout(Duration::from_secs(5), http.get(JWKS_URL).send())
        .await
        .map_err(|_| invalid_identity())?
        .map_err(|_| invalid_identity())?;
    if !response.status().is_success() {
        return Err(invalid_identity());
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| invalid_identity())? {
        if bytes.len().saturating_add(chunk.len()) > MAX_JWKS_BYTES {
            return Err(invalid_identity());
        }
        bytes.extend_from_slice(&chunk);
    }
    let jwks: JwkSet = serde_json::from_slice(&bytes).map_err(|_| invalid_identity())?;
    verify_id_token_with_jwks(token, &jwks, client_id, expected_nonce)
}

pub fn verify_id_token_with_jwks(
    token: &str,
    jwks: &JwkSet,
    client_id: &str,
    expected_nonce: &str,
) -> Result<XeroIdentity, Error> {
    if token.len() > MAX_TOKEN_BYTES || client_id.is_empty() || expected_nonce.is_empty() {
        return Err(invalid_identity());
    }
    let header = decode_header(token).map_err(|_| invalid_identity())?;
    if header.alg != Algorithm::RS256 {
        return Err(invalid_identity());
    }
    let kid = header.kid.as_deref().ok_or_else(invalid_identity)?;
    let jwk = jwks.find(kid).ok_or_else(invalid_identity)?;
    let key = DecodingKey::from_jwk(jwk).map_err(|_| invalid_identity())?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id]);
    validation.set_issuer(&[ISSUER]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.leeway = 30;
    let claims = decode::<Value>(token, &key, &validation)
        .map_err(|_| invalid_identity())?
        .claims;
    if claims.get("aud").and_then(Value::as_str) != Some(client_id)
        || claims.get("nonce").and_then(Value::as_str) != Some(expected_nonce)
    {
        return Err(invalid_identity());
    }
    let subject = claims
        .get("sub")
        .and_then(Value::as_str)
        .filter(|subject| !subject.is_empty() && subject.len() <= 256)
        .ok_or_else(invalid_identity)?;
    let now = chrono::Utc::now().timestamp();
    if claims
        .get("iat")
        .and_then(Value::as_i64)
        .is_none_or(|issued| issued > now + 30)
    {
        return Err(invalid_identity());
    }
    Ok(XeroIdentity {
        subject: subject.to_owned(),
        email: claims
            .get("email")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}
