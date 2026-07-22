//! Tenant-bound Auth Core service-token client for inference-core.
//!
//! The embedding worker is a background caller, so it cannot forward a user
//! bearer or rely on a shared internal key. It mints a short-lived, audited
//! `aud=inference-core` service JWT for the organization whose content is being
//! embedded and forwards only that bearer over gRPC.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};

const INFERENCE_AUDIENCE: &str = "inference-core";
const INFERENCE_SCOPE: &str = "inference:invoke";
const TOKEN_REASON: &str = "embed document content";
const MAX_TOKEN_TTL_SECONDS: i64 = 300;
const MAX_TOKEN_RESPONSE_BYTES: usize = 64 * 1024;
const CLOCK_LEEWAY_SECONDS: i64 = 30;

#[derive(Clone)]
pub(super) struct InferenceTokenClient {
    http: Client,
    token_url: Url,
    expected_issuer: Arc<str>,
    service_id: Arc<str>,
    service_api_key: Arc<str>,
}

impl fmt::Debug for InferenceTokenClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InferenceTokenClient")
            .field("token_url", &self.token_url)
            .field("expected_issuer", &self.expected_issuer)
            .field("service_id", &self.service_id)
            .field("service_api_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
pub(super) struct InferenceBearer(Arc<str>);

impl InferenceBearer {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for InferenceBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InferenceBearer([REDACTED])")
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenRequest<'a> {
    org_id: &'a str,
    scopes: [&'static str; 1],
    reason: &'static str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TokenResponse {
    token: String,
    expires_at: String,
    expires_in_seconds: i64,
    issuer: String,
    audience: String,
}

#[derive(Deserialize)]
struct TokenClaims {
    iss: String,
    aud: String,
    sub: String,
    iat: i64,
    nbf: i64,
    exp: i64,
    org_id: String,
    principal_type: String,
    service_id: String,
    scopes: Vec<String>,
    reason: String,
    zdr: bool,
}

impl InferenceTokenClient {
    pub(super) fn new(
        token_url: &str,
        expected_issuer: &str,
        service_id: &str,
        service_api_key: &str,
    ) -> anyhow::Result<Self> {
        Self::build(
            token_url,
            expected_issuer,
            service_id,
            service_api_key,
            false,
        )
    }

    /// Standalone startup may bind without Auth Core's registered principal.
    /// The client remains unusable until a real credential is supplied, so a
    /// content request still fails closed rather than making an unauthenticated
    /// inference call.
    pub(super) fn new_allow_unconfigured(
        token_url: &str,
        expected_issuer: &str,
        service_id: &str,
        service_api_key: &str,
    ) -> anyhow::Result<Self> {
        Self::build(
            token_url,
            expected_issuer,
            service_id,
            service_api_key,
            true,
        )
    }

    fn build(
        token_url: &str,
        expected_issuer: &str,
        service_id: &str,
        service_api_key: &str,
        allow_unconfigured: bool,
    ) -> anyhow::Result<Self> {
        let token_url = Url::parse(token_url.trim())
            .context("MODEL_PLANE_INFERENCE_TOKEN_URL must be an absolute HTTP(S) URL")?;
        anyhow::ensure!(
            matches!(token_url.scheme(), "http" | "https")
                && token_url.username().is_empty()
                && token_url.password().is_none()
                && token_url.query().is_none()
                && token_url.fragment().is_none(),
            "MODEL_PLANE_INFERENCE_TOKEN_URL must be an uncredentialed HTTP(S) URL without query or fragment"
        );
        anyhow::ensure!(
            !expected_issuer.trim().is_empty() && expected_issuer == expected_issuer.trim(),
            "MODEL_PLANE_INFERENCE_TOKEN_ISSUER is required"
        );
        anyhow::ensure!(
            valid_service_id(service_id),
            "MODEL_PLANE_INFERENCE_SERVICE_ID is invalid"
        );
        anyhow::ensure!(
            allow_unconfigured
                || (service_api_key.len() >= 16 && service_api_key == service_api_key.trim()),
            "MODEL_PLANE_INFERENCE_SERVICE_API_KEY is missing or invalid"
        );

        let http = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("build inference service-token HTTP client")?;
        Ok(Self {
            http,
            token_url,
            expected_issuer: Arc::from(expected_issuer),
            service_id: Arc::from(service_id),
            service_api_key: Arc::from(service_api_key),
        })
    }

    pub(super) async fn mint(&self, org_id: &str) -> anyhow::Result<InferenceBearer> {
        anyhow::ensure!(valid_org_id(org_id), "inference tenant is invalid");
        anyhow::ensure!(
            self.service_api_key.len() >= 16,
            "inference service principal credential is unavailable"
        );

        let response = self
            .http
            .post(self.token_url.clone())
            .header("x-service-id", self.service_id.as_ref())
            .header("x-service-api-key", self.service_api_key.as_ref())
            .json(&TokenRequest {
                org_id,
                scopes: [INFERENCE_SCOPE],
                reason: TOKEN_REASON,
            })
            .send()
            .await
            .context("inference service-token endpoint unavailable")?;
        anyhow::ensure!(
            response.status().is_success(),
            "inference service-token issuance failed with HTTP status {}",
            response.status().as_u16()
        );
        let body = response
            .bytes()
            .await
            .context("read inference service-token response")?;
        anyhow::ensure!(
            body.len() <= MAX_TOKEN_RESPONSE_BYTES,
            "inference service-token response exceeded size limit"
        );
        let bundle: TokenResponse = serde_json::from_slice(&body)
            .context("inference service-token response was malformed")?;
        self.validate_bundle(bundle, org_id)
    }

    fn validate_bundle(
        &self,
        bundle: TokenResponse,
        org_id: &str,
    ) -> anyhow::Result<InferenceBearer> {
        anyhow::ensure!(
            bundle.audience == INFERENCE_AUDIENCE
                && bundle.issuer == self.expected_issuer.as_ref()
                && bundle.expires_in_seconds > 0
                && bundle.expires_in_seconds <= MAX_TOKEN_TTL_SECONDS
                && !bundle.token.is_empty()
                && bundle.token.len() <= 8192
                && !bundle.token.chars().any(char::is_whitespace),
            "inference service-token response violated the bounded-token contract"
        );

        let header = decode_header(&bundle.token)
            .context("inference service-token was not a compact JWT")?;
        anyhow::ensure!(
            header.alg == Algorithm::RS256 && header.typ.as_deref() == Some("JWT"),
            "inference service-token used an unsupported JWT profile"
        );

        // Inference Core remains the cryptographic verifier. This local decode
        // only checks the bounded response profile before forwarding it.
        let mut validation = Validation::new(Algorithm::RS256);
        validation.insecure_disable_signature_validation();
        validation.set_audience(&[INFERENCE_AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "iat", "nbf", "aud", "iss", "sub"]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = CLOCK_LEEWAY_SECONDS as u64;
        let claims =
            decode::<TokenClaims>(&bundle.token, &DecodingKey::from_secret(&[]), &validation)
                .context("inference service-token claims were invalid")?
                .claims;

        let expected_subject = format!("service:{}", self.service_id);
        anyhow::ensure!(
            claims.iss == self.expected_issuer.as_ref()
                && claims.aud == INFERENCE_AUDIENCE
                && claims.org_id == org_id
                && claims.principal_type == "service"
                && claims.sub == expected_subject
                && claims.service_id == expected_subject
                && claims.scopes.as_slice() == [INFERENCE_SCOPE]
                && claims.reason == TOKEN_REASON
                // Embeddings of durable documents ARE persisted (Qdrant), so the
                // registry provisions this principal with a `persistent` retention
                // posture — the token must carry zdr:false. Requiring zdr:true
                // here (the old registry posture) broke every embedding call once
                // the registry was corrected: inference-core's fail-closed ZDR
                // chain has no verified-ZDR provider, and this local check
                // rejected the honest zdr:false token before even sending it.
                && !claims.zdr,
            "inference service-token claims exceeded requested authority"
        );

        let now = Utc::now();
        let expires_at = DateTime::parse_from_rfc3339(&bundle.expires_at)
            .context("inference service-token expiry was malformed")?
            .with_timezone(&Utc);
        let remaining_claim_seconds = claims.exp - now.timestamp();
        let remaining_bundle_seconds = (expires_at - now).num_seconds();
        anyhow::ensure!(
            claims.iat <= now.timestamp() + CLOCK_LEEWAY_SECONDS
                && claims.nbf <= claims.exp
                && claims.iat <= claims.exp
                && remaining_claim_seconds > 0
                && remaining_claim_seconds <= MAX_TOKEN_TTL_SECONDS
                && remaining_bundle_seconds > 0
                && remaining_bundle_seconds <= MAX_TOKEN_TTL_SECONDS
                && (claims.exp - expires_at.timestamp()).abs() <= 5
                && (bundle.expires_in_seconds - remaining_claim_seconds).abs() <= 5,
            "inference service-token was expired or exceeded its maximum lifetime"
        );

        Ok(InferenceBearer(Arc::from(bundle.token)))
    }
}

fn valid_service_id(value: &str) -> bool {
    let mut chars = value.chars();
    value.len() <= 128
        && chars
            .next()
            .is_some_and(|value| value.is_ascii_alphanumeric())
        && chars.all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-'))
}

fn valid_org_id(value: &str) -> bool {
    let mut chars = value.chars();
    value.len() <= 128
        && chars
            .next()
            .is_some_and(|value| value.is_ascii_alphanumeric())
        && chars
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | ':' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_or_short_service_credentials() {
        let missing = InferenceTokenClient::new(
            "http://auth-core:3011/api/inference-core/internal-token",
            "http://auth-core:3011/api/convex-auth",
            "embedding-engine",
            "",
        )
        .expect_err("empty credential must fail closed");
        assert!(missing
            .to_string()
            .contains("MODEL_PLANE_INFERENCE_SERVICE_API_KEY"));

        let short = InferenceTokenClient::new(
            "http://auth-core:3011/api/inference-core/internal-token",
            "http://auth-core:3011/api/convex-auth",
            "embedding-engine",
            "too-short",
        )
        .expect_err("short credential must fail closed");
        assert!(short
            .to_string()
            .contains("MODEL_PLANE_INFERENCE_SERVICE_API_KEY"));
    }

    #[test]
    fn standalone_constructor_allows_bind_but_keeps_inference_unconfigured() {
        let client = InferenceTokenClient::new_allow_unconfigured(
            "http://auth-core:3011/api/inference-core/internal-token",
            "http://auth-core:3011/api/convex-auth",
            "embedding-engine",
            "",
        )
        .expect("standalone startup may bind without the external principal");
        assert!(client.service_api_key.is_empty());
    }
}
