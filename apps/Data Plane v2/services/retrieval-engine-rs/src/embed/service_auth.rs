//! Tenant-bound Auth Core service-token client for inference-core.
//!
//! Retrieval never authenticates to inference-core with a shared API key or
//! caller-selected identity headers. For every organization-scoped embedding
//! batch it mints one short-lived, audited `aud=inference-core` service JWT and
//! validates the bounded response before forwarding it as a gRPC bearer.

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
const TOKEN_REASON: &str = "embed retrieval query";
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
            service_api_key.len() >= 16 && service_api_key == service_api_key.trim(),
            "MODEL_PLANE_INFERENCE_SERVICE_API_KEY is missing or invalid"
        );

        let http = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            // Never forward the deployment credential to a redirected host.
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
        anyhow::ensure!(valid_org_id(org_id), "embedding tenant is invalid");

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

        // Inference Core is the cryptographic verifier. This local decode is a
        // fail-fast bounded-response check so a misrouted issuer response is
        // never forwarded as authority.
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
                && claims.scopes.len() == 1
                && claims.scopes[0] == INFERENCE_SCOPE
                && claims.reason == TOKEN_REASON
                // Query embeddings ride the same `persistent`-posture principal
                // as document embeddings (zdr:false). See embedding-engine's
                // inference_auth.rs for why requiring zdr:true here broke every
                // call once the registry posture was corrected.
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
    use std::sync::{Arc, Mutex, OnceLock};

    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    use jsonwebtoken::{encode, EncodingKey, Header};
    use rand::thread_rng;
    use rsa::{
        pkcs8::{EncodePrivateKey, LineEnding},
        RsaPrivateKey,
    };
    use serde_json::{json, Value};

    use super::*;

    const ISSUER: &str = "http://control.test/api/convex-auth";
    const SERVICE_ID: &str = "retrieval-engine";
    const SERVICE_CREDENTIAL: &str = "isolated-test-service-credential";

    #[derive(Clone, Copy)]
    enum TokenProblem {
        None,
        WrongAudience,
        WrongScope,
        WrongOrg,
        Retaining,
    }

    #[derive(Clone)]
    struct RecordedRequest {
        headers: HeaderMap,
        body: Value,
    }

    struct MockState {
        problem: TokenProblem,
        requests: Mutex<Vec<RecordedRequest>>,
    }

    #[derive(Serialize)]
    struct MockClaims {
        iss: &'static str,
        aud: String,
        sub: String,
        iat: i64,
        nbf: i64,
        exp: i64,
        org_id: String,
        principal_type: &'static str,
        service_id: String,
        scopes: Vec<String>,
        reason: &'static str,
        zdr: bool,
    }

    fn encoding_key() -> &'static EncodingKey {
        static KEY: OnceLock<EncodingKey> = OnceLock::new();
        KEY.get_or_init(|| {
            let private =
                RsaPrivateKey::new(&mut thread_rng(), 2048).expect("generate inference test key");
            let private_pem = private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("encode inference test key");
            EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("load inference test key")
        })
    }

    async fn mint_token(
        State(state): State<Arc<MockState>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        state
            .requests
            .lock()
            .expect("token request records lock")
            .push(RecordedRequest {
                headers,
                body: body.clone(),
            });

        let now = Utc::now().timestamp();
        let requested_org = body["orgId"].as_str().unwrap_or_default();
        let audience = if matches!(state.problem, TokenProblem::WrongAudience) {
            "control-policy"
        } else {
            INFERENCE_AUDIENCE
        };
        let org_id = if matches!(state.problem, TokenProblem::WrongOrg) {
            "other-org"
        } else {
            requested_org
        };
        let scopes = if matches!(state.problem, TokenProblem::WrongScope) {
            vec!["inference:admin".to_string()]
        } else {
            vec![INFERENCE_SCOPE.to_string()]
        };
        let claims = MockClaims {
            iss: ISSUER,
            aud: audience.to_string(),
            sub: format!("service:{SERVICE_ID}"),
            iat: now,
            nbf: now - 5,
            exp: now + 60,
            org_id: org_id.to_string(),
            principal_type: "service",
            service_id: format!("service:{SERVICE_ID}"),
            scopes,
            reason: TOKEN_REASON,
            zdr: !matches!(state.problem, TokenProblem::Retaining),
        };
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".to_string());
        let token = encode(&header, &claims, encoding_key()).expect("mint mock inference token");
        let expires_at = DateTime::from_timestamp(now + 60, 0)
            .expect("valid mock expiry")
            .to_rfc3339();
        (
            StatusCode::OK,
            Json(json!({
                "token": token,
                "expiresAt": expires_at,
                "expiresInSeconds": 60,
                "issuer": ISSUER,
                "audience": audience,
            })),
        )
    }

    async fn start_server(problem: TokenProblem) -> (String, Arc<MockState>) {
        // Keep key generation outside the production client's short timeout.
        let _ = encoding_key();
        let state = Arc::new(MockState {
            problem,
            requests: Mutex::new(Vec::new()),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind inference token test server");
        let address = listener.local_addr().expect("inference token test addr");
        let app = Router::new()
            .route("/api/inference-core/internal-token", post(mint_token))
            .with_state(state.clone());
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve inference token test endpoint");
        });
        (
            format!("http://{address}/api/inference-core/internal-token"),
            state,
        )
    }

    fn client(token_url: &str) -> InferenceTokenClient {
        InferenceTokenClient::new(token_url, ISSUER, SERVICE_ID, SERVICE_CREDENTIAL)
            .expect("inference token client")
    }

    #[tokio::test]
    async fn mints_exact_org_bound_inference_service_token() {
        let (token_url, state) = start_server(TokenProblem::None).await;
        let bearer = client(&token_url)
            .mint("org-a")
            .await
            .expect("bounded bearer");

        assert!(bearer.as_str().split('.').count() == 3);
        assert_eq!(format!("{bearer:?}"), "InferenceBearer([REDACTED])");
        let requests = state.requests.lock().expect("token request records lock");
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(
            request
                .headers
                .get("x-service-id")
                .and_then(|value| value.to_str().ok()),
            Some(SERVICE_ID)
        );
        assert_eq!(
            request
                .headers
                .get("x-service-api-key")
                .and_then(|value| value.to_str().ok()),
            Some(SERVICE_CREDENTIAL)
        );
        assert!(request.headers.get("authorization").is_none());
        assert_eq!(request.body["orgId"], "org-a");
        assert_eq!(request.body["scopes"], json!([INFERENCE_SCOPE]));
        assert_eq!(request.body["reason"], TOKEN_REASON);
    }

    #[tokio::test]
    async fn rejects_tokens_that_exceed_requested_inference_authority() {
        for problem in [
            TokenProblem::WrongAudience,
            TokenProblem::WrongScope,
            TokenProblem::WrongOrg,
            TokenProblem::Retaining,
        ] {
            let (token_url, _) = start_server(problem).await;
            assert!(client(&token_url).mint("org-a").await.is_err());
        }
    }

    #[test]
    fn rejects_missing_credentials_and_unsafe_token_urls() {
        assert!(InferenceTokenClient::new(
            "http://auth-core:3011/api/inference-core/internal-token",
            ISSUER,
            SERVICE_ID,
            ""
        )
        .is_err());
        assert!(InferenceTokenClient::new(
            "http://credential@auth-core:3011/api/inference-core/internal-token",
            ISSUER,
            SERVICE_ID,
            SERVICE_CREDENTIAL
        )
        .is_err());
    }
}
