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

/// Retention posture this deployment expects on an issued inference token.
///
/// Mirrors auth-core's service-principal registry vocabulary — the
/// `retentionByAudience` map takes exactly `"zdr"` or `"persistent"` per
/// audience. The issuer alone selects the posture (auth-core rejects any
/// caller-supplied `x-zdr` header or `zdr` body field), so this is an assertion
/// that the token matches the principal we are registered as, never a request.
///
/// Keep this aligned with the registry entry for this service's
/// `inference-core` audience. A mismatch fails closed on every mint, which is
/// how embedding silently died when the registry moved to `persistent` while
/// the clients still demanded `zdr` — see commit 3666503f.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RetentionPosture {
    /// Registry `"zdr"`: the token must carry `zdr:true`.
    ZeroRetention,
    /// Registry `"persistent"`: the token must carry `zdr:false`.
    Persistent,
}

impl RetentionPosture {
    pub(super) fn parse(value: &str) -> anyhow::Result<Self> {
        match value.trim() {
            "zdr" => Ok(Self::ZeroRetention),
            "persistent" => Ok(Self::Persistent),
            other => anyhow::bail!(
                "MODEL_PLANE_INFERENCE_RETENTION_POSTURE must be `zdr` or `persistent`, got `{other}`"
            ),
        }
    }

    /// The `zdr` claim value a correctly issued token carries under this posture.
    const fn expected_zdr(self) -> bool {
        matches!(self, Self::ZeroRetention)
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::ZeroRetention => "zdr",
            Self::Persistent => "persistent",
        }
    }
}

/// Renders an observed `zdr` claim in the registry's vocabulary so a mismatch
/// error names both sides in the same terms the operator configures.
const fn observed_posture(zdr: bool) -> &'static str {
    if zdr {
        "zdr"
    } else {
        "persistent"
    }
}

#[derive(Clone)]
pub(super) struct InferenceTokenClient {
    http: Client,
    token_url: Url,
    expected_issuer: Arc<str>,
    service_id: Arc<str>,
    service_api_key: Arc<str>,
    retention_posture: RetentionPosture,
}

impl fmt::Debug for InferenceTokenClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InferenceTokenClient")
            .field("token_url", &self.token_url)
            .field("expected_issuer", &self.expected_issuer)
            .field("service_id", &self.service_id)
            .field("service_api_key", &"[REDACTED]")
            .field("retention_posture", &self.retention_posture)
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
        retention_posture: RetentionPosture,
    ) -> anyhow::Result<Self> {
        Self::build(
            token_url,
            expected_issuer,
            service_id,
            service_api_key,
            retention_posture,
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
        retention_posture: RetentionPosture,
    ) -> anyhow::Result<Self> {
        Self::build(
            token_url,
            expected_issuer,
            service_id,
            service_api_key,
            retention_posture,
            true,
        )
    }

    fn build(
        token_url: &str,
        expected_issuer: &str,
        service_id: &str,
        service_api_key: &str,
        retention_posture: RetentionPosture,
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
            retention_posture,
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
                && claims.reason == TOKEN_REASON,
            "inference service-token claims exceeded requested authority"
        );

        // Embeddings of durable documents ARE persisted (Qdrant), so the registry
        // provisions this principal with a `persistent` posture by default.
        // Checked separately from the authority bounds above so a posture drift
        // between this deployment and auth-core's registry names both sides
        // instead of hiding inside a nine-clause assertion.
        anyhow::ensure!(
            claims.zdr == self.retention_posture.expected_zdr(),
            "inference service-token carried the `{}` retention posture but this deployment \
             expects `{}`; align MODEL_PLANE_INFERENCE_RETENTION_POSTURE with auth-core's \
             retentionByAudience entry for this principal's `{INFERENCE_AUDIENCE}` audience",
            observed_posture(claims.zdr),
            self.retention_posture.as_str(),
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
            RetentionPosture::Persistent,
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
            RetentionPosture::Persistent,
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
            RetentionPosture::Persistent,
        )
        .expect("standalone startup may bind without the external principal");
        assert!(client.service_api_key.is_empty());
    }
}
