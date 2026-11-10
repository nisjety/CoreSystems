//! The one HTTP path every Data Plane call of Quarry takes: the vector leg (`/v1/retrieve`) and the
//! rerank (`/v1/rerank`). Each caller used to re-derive these rules, so they live here once.
//!
//! What this module owns:
//! * a bounded client with no redirects, built with the caller's own timeout. A redirect would
//!   carry the bearer to an address Auth Core never named;
//! * the credential: a service token minted for the call's manifest row (see
//!   [`crate::service_tokens`]), or the development bypass key where the edge's bypass is on;
//! * one forced refresh when the Data Plane answers 401, because a token can expire at the Data
//!   Plane before Quarry's own cache knows. A second 401 is the caller's answer to read.
//!
//! What it does not own: status codes. The same status means different things on different
//! routes, so each caller classifies its own answer.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use crate::service_tokens::{ServiceTokenRequest, SharedServiceTokenProvider};

/// The window in which one route writes at most one ERROR line. Its refusals (401, 403, 404, 422) and its
/// token refusals share it, so a misconfigured principal logs one ERROR a minute, not one per call.
pub const ERROR_LOG_WINDOW: Duration = Duration::from_secs(60);

/// Why a Data Plane call produced no answer to read. The caller decides what each class means.
#[derive(Debug)]
pub enum DataPlaneCallError {
    /// Auth Core refused the token request, or could not be reached. The class is already logged.
    Token(QuarryError),
    /// The Data Plane did not answer before the client's timeout.
    Timeout(reqwest::Error),
    /// The Data Plane could not be reached, or the connection dropped.
    Unreachable(reqwest::Error),
}

/// Admits one ERROR line per [`ERROR_LOG_WINDOW`].
#[derive(Default)]
struct ErrorThrottle {
    last: Mutex<Option<Instant>>,
}

impl ErrorThrottle {
    fn admit(&self, now: Instant) -> bool {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        match *last {
            Some(previous) if now.duration_since(previous) < ERROR_LOG_WINDOW => false,
            _ => {
                *last = Some(now);
                true
            }
        }
    }
}

/// One Data Plane route and the credential its calls carry.
pub struct DataPlaneHttp {
    client: Client,
    url: String,
    tokens: Option<SharedServiceTokenProvider>,
    api_key: Option<String>,
    timeout: Duration,
    error_log: ErrorThrottle,
}

impl DataPlaneHttp {
    /// `url` is the full route, for example `http://dpv2-retrieval-engine:8080/v1/rerank`.
    pub fn new(url: impl Into<String>, timeout: Duration) -> QuarryResult<Self> {
        Ok(Self {
            client: build_client(timeout)?,
            url: url.into(),
            tokens: None,
            api_key: None,
            timeout,
            error_log: ErrorThrottle::default(),
        })
    }

    /// Whether this route may write an ERROR line now. At most one per [`ERROR_LOG_WINDOW`]: a caller that
    /// is about to log a refusal asks first, and writes its line only on a `true`.
    pub(crate) fn admit_error_log(&self) -> bool {
        self.error_log.admit(Instant::now())
    }

    /// Mints the call's token from Auth Core. Production always sets one.
    pub fn with_token_provider(mut self, provider: SharedServiceTokenProvider) -> Self {
        self.tokens = Some(provider);
        self
    }

    /// The development bypass key. Set only where the edge's cross-plane bypass is on.
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Rebuilds the client with another whole-call timeout, keeping the credential.
    pub fn with_timeout(mut self, timeout: Duration) -> QuarryResult<Self> {
        self.client = build_client(timeout)?;
        self.timeout = timeout;
        Ok(self)
    }

    /// The whole-call timeout this route was built with.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Sends one JSON body for `org_id` under the manifest row `request`, and returns the Data
    /// Plane's answer unread. A 401 is retried once with a freshly minted token.
    pub async fn post_json<B>(
        &self,
        request: &ServiceTokenRequest,
        org_id: &str,
        body: &B,
    ) -> Result<Response, DataPlaneCallError>
    where
        B: Serialize + ?Sized,
    {
        self.post_json_for(Some(request), org_id, body).await
    }

    /// As [`Self::post_json`], for a route whose manifest row may not exist yet (E3.8.8). With a
    /// token provider and no row the call is refused before any request: a route never goes out
    /// without its credential because its row is missing. Without a provider the development
    /// bypass key is sent, as in [`Self::post_json`].
    pub async fn post_json_for<B>(
        &self,
        request: Option<&ServiceTokenRequest>,
        org_id: &str,
        body: &B,
    ) -> Result<Response, DataPlaneCallError>
    where
        B: Serialize + ?Sized,
    {
        let bearer = match (&self.tokens, request) {
            (Some(provider), Some(request)) => Some(
                provider
                    .token_for_org(org_id, request, false)
                    .await
                    .map_err(|error| {
                        DataPlaneCallError::Token(self.log_token_failure(error, request))
                    })?
                    .expose()
                    .to_owned(),
            ),
            (Some(_), None) => {
                return Err(DataPlaneCallError::Token(QuarryError::new(
                    ErrorCode::Internal,
                    "data plane call: this route has no token-request manifest row yet",
                )))
            }
            (None, _) => self.api_key.clone(),
        };
        let post = |bearer: Option<&str>| {
            let mut builder = self
                .client
                .post(&self.url)
                .header("x-org-id", org_id)
                .json(body);
            if let Some(token) = bearer {
                builder = builder.bearer_auth(token);
            }
            builder.send()
        };
        let mut response = post(bearer.as_deref()).await;
        if matches!(&response, Ok(resp) if resp.status() == StatusCode::UNAUTHORIZED) {
            if let (Some(provider), Some(request)) = (&self.tokens, request) {
                // The cached token may have expired at the Data Plane: mint once more.
                response = match provider.token_for_org(org_id, request, true).await {
                    Ok(token) => post(Some(token.expose())).await,
                    Err(error) => {
                        return Err(DataPlaneCallError::Token(
                            self.log_token_failure(error, request),
                        ))
                    }
                };
            }
        }
        response.map_err(|error| {
            if error.is_timeout() {
                DataPlaneCallError::Timeout(error)
            } else {
                DataPlaneCallError::Unreachable(error)
            }
        })
    }
}

fn build_client(timeout: Duration) -> QuarryResult<Client> {
    Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| {
            QuarryError::new(ErrorCode::Internal, format!("data plane client: {error}"))
        })
}

impl DataPlaneHttp {
    /// A failed token exchange, logged by its class. A 401 or 403 from Auth Core is an error (the registry
    /// row is wrong), and at most one such line is written per [`ERROR_LOG_WINDOW`] for this route. An
    /// Auth Core that cannot be reached is a warning.
    pub(crate) fn log_token_failure(
        &self,
        error: QuarryError,
        request: &ServiceTokenRequest,
    ) -> QuarryError {
        match error.code {
            ErrorCode::Unauthorized | ErrorCode::Forbidden => {
                if self.admit_error_log() {
                    // zdr-log-ok: only the rerank and vector legs call this, and both return before any token request for a ZDR caller; the text is Auth Core's refusal, not caller data
                    tracing::error!(
                        reason = "dp_token_refused",
                        purpose = request.reason(),
                        error = %error,
                        "data plane call: Auth Core refused the service token; check the service principal's registry row"
                    );
                }
            }
            _ => {
                // zdr-log-ok: only the rerank and vector legs call this, and both return before any token request for a ZDR caller; the text is the token provider's failure, not caller data
                tracing::warn!(
                    reason = "dp_token_unavailable",
                    purpose = request.reason(),
                    error = %error,
                    "data plane call: no service token for this call"
                );
            }
        }
        error
    }
}

// -- E3.8.8: the Data Plane artifacts read (no_egress mode) -------------------------------------
//
// Built against the proposed contract below, with a fake on the Quarry side: the Data Plane route
// does not exist yet (Data Plane ask in the unit's status file). The reader owns the wire and its
// status classes; the policy (the flag, ZDR, the typed miss) belongs to `crate::fetch::NoEgressDriver`.

/// The proposed Data Plane route a no_egress scrape reads stored artifacts from. The URL travels in
/// the JSON body, never in the path or the query string, so it never lands in an access log.
pub const ARTIFACT_LOOKUP_PATH: &str = "/v1/artifacts/lookup";

/// The `error.code` the Data Plane answers a 404 with when it holds no artifact of the org for the
/// URL. A 404 without it is a route that does not exist, which is not a miss.
pub const ARTIFACT_MISS_CODE: &str = "artifact_not_found";

/// The largest answer body the reader takes. The body is base64 inside JSON, so this is above the
/// 16 MiB page bodies Quarry stores.
pub const MAX_ARTIFACT_ANSWER_BYTES: usize = 24 * 1024 * 1024;

/// One stored capture of a page, as the Data Plane holds it for the org.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredArtifact {
    /// The status the page answered with when it was captured.
    pub status: u16,
    /// The URL the capture ended on, after redirects.
    pub final_url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// When the capture was taken (RFC 3339), if the Data Plane says.
    pub captured_at: Option<String>,
}

/// What a lookup found. A miss is an answer, not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactLookup {
    Hit(StoredArtifact),
    Miss,
}

/// The read side of the Data Plane artifacts contract. The production reader is
/// [`DataPlaneArtifactReader`]; tests use a fake.
#[async_trait]
pub trait ArtifactReader: Send + Sync {
    /// Looks up the stored artifact of `org_id` for `url`. `max_bytes` is the caller's read cap,
    /// passed on so the Data Plane may refuse a larger body early; the caller enforces it anyway.
    async fn lookup(
        &self,
        org_id: &str,
        url: &str,
        max_bytes: Option<usize>,
    ) -> QuarryResult<ArtifactLookup>;
}

#[derive(Serialize)]
struct ArtifactLookupBody<'a> {
    url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_bytes: Option<usize>,
}

#[derive(Deserialize)]
struct ArtifactLookupEnvelope {
    data: StoredArtifactWire,
}

#[derive(Deserialize)]
struct StoredArtifactWire {
    status: u16,
    final_url: String,
    #[serde(default)]
    headers: Vec<(String, String)>,
    body_base64: String,
    #[serde(default)]
    captured_at: Option<String>,
}

#[derive(Deserialize)]
struct ArtifactErrorEnvelope {
    error: ArtifactErrorBody,
}

#[derive(Deserialize)]
struct ArtifactErrorBody {
    code: String,
}

/// Reads stored artifacts from the Data Plane over [`ARTIFACT_LOOKUP_PATH`]. No error, message or
/// log line it writes carries the looked-up URL.
pub struct DataPlaneArtifactReader {
    http: DataPlaneHttp,
    token_request: Option<ServiceTokenRequest>,
}

impl DataPlaneArtifactReader {
    /// `http` is built on the full route, for example
    /// `http://dpv2-documents-api:8080/v1/artifacts/lookup`.
    pub fn new(http: DataPlaneHttp) -> Self {
        Self {
            http,
            token_request: None,
        }
    }

    /// The manifest row the call's service token is minted for. A reader with a token provider and
    /// no row refuses every lookup before any request (see [`DataPlaneHttp::post_json_for`]).
    pub fn with_token_request(mut self, request: ServiceTokenRequest) -> Self {
        self.token_request = Some(request);
        self
    }

    /// The production row: `artifacts-read` (audience `data-plane`, scope `data:read`), read from
    /// the token-request manifest and never built by hand. A no_egress run is never a ZDR run, so
    /// this token is never requested for one.
    pub fn with_manifest_token_request(self) -> QuarryResult<Self> {
        Ok(self.with_token_request(ServiceTokenRequest::from_manifest("artifacts-read")?))
    }

    fn refusal(&self, status: StatusCode) -> QuarryError {
        let code = match status {
            StatusCode::UNAUTHORIZED => ErrorCode::Unauthorized,
            _ => ErrorCode::Forbidden,
        };
        if self.http.admit_error_log() {
            // zdr-log-ok: a no_egress lookup never runs for a ZDR caller (NoEgressDriver refuses first); the line carries the status only, no URL and no body
            tracing::error!(
                reason = "dp_artifacts_refused",
                status = status.as_u16(),
                "data plane artifacts: the Data Plane refused the lookup; check the service principal's registry row"
            );
        }
        QuarryError::new(code, "data plane artifacts: the lookup was refused")
    }

    fn route_missing(&self) -> QuarryError {
        if self.http.admit_error_log() {
            // zdr-log-ok: a no_egress lookup never runs for a ZDR caller (NoEgressDriver refuses first); the line names no URL
            tracing::error!(
                reason = "dp_artifacts_route_missing",
                "data plane artifacts: 404 without the artifact_not_found code; the artifacts route is not deployed"
            );
        }
        QuarryError::new(
            ErrorCode::RuntimeNotReady,
            "data plane artifacts: the artifacts route is not available",
        )
    }
}

fn unreadable_artifact_answer() -> QuarryError {
    QuarryError::new(
        ErrorCode::Internal,
        "data plane artifacts: the answer could not be read",
    )
}

#[async_trait]
impl ArtifactReader for DataPlaneArtifactReader {
    async fn lookup(
        &self,
        org_id: &str,
        url: &str,
        max_bytes: Option<usize>,
    ) -> QuarryResult<ArtifactLookup> {
        let body = ArtifactLookupBody { url, max_bytes };
        let response = match self
            .http
            .post_json_for(self.token_request.as_ref(), org_id, &body)
            .await
        {
            Ok(response) => response,
            Err(DataPlaneCallError::Token(error)) => return Err(error),
            Err(DataPlaneCallError::Timeout(_)) => {
                return Err(QuarryError::new(
                    ErrorCode::Timeout,
                    "data plane artifacts: the lookup timed out",
                ))
            }
            Err(DataPlaneCallError::Unreachable(_)) => {
                return Err(QuarryError::new(
                    ErrorCode::Internal,
                    "data plane artifacts: the Data Plane could not be reached",
                ))
            }
        };
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ARTIFACT_ANSWER_BYTES as u64)
        {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "data plane artifacts: the answer is larger than the reader takes",
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| unreadable_artifact_answer())?;
        if bytes.len() > MAX_ARTIFACT_ANSWER_BYTES {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "data plane artifacts: the answer is larger than the reader takes",
            ));
        }
        match status {
            StatusCode::OK => {
                let envelope: ArtifactLookupEnvelope =
                    serde_json::from_slice(&bytes).map_err(|_| unreadable_artifact_answer())?;
                let wire = envelope.data;
                let body = B64
                    .decode(wire.body_base64.as_bytes())
                    .map_err(|_| unreadable_artifact_answer())?;
                Ok(ArtifactLookup::Hit(StoredArtifact {
                    status: wire.status,
                    final_url: wire.final_url,
                    headers: wire.headers,
                    body,
                    captured_at: wire.captured_at,
                }))
            }
            StatusCode::NOT_FOUND => match serde_json::from_slice::<ArtifactErrorEnvelope>(&bytes) {
                Ok(envelope) if envelope.error.code == ARTIFACT_MISS_CODE => Ok(ArtifactLookup::Miss),
                _ => Err(self.route_missing()),
            },
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(self.refusal(status)),
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => Err(QuarryError::new(
                ErrorCode::BadRequest,
                "data plane artifacts: the Data Plane rejected the lookup",
            )),
            StatusCode::TOO_MANY_REQUESTS => Err(QuarryError::new(
                ErrorCode::RateLimited,
                "data plane artifacts: the Data Plane is rate limiting lookups",
            )),
            other => Err(QuarryError::new(
                ErrorCode::Internal,
                format!("data plane artifacts: unexpected status {}", other.as_u16()),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_line_is_admitted_once_per_window() {
        let throttle = ErrorThrottle::default();
        let t0 = Instant::now();
        assert!(throttle.admit(t0));
        assert!(!throttle.admit(t0 + Duration::from_secs(1)));
        assert!(!throttle.admit(t0 + ERROR_LOG_WINDOW - Duration::from_millis(1)));
        assert!(throttle.admit(t0 + ERROR_LOG_WINDOW));
    }

    #[test]
    fn a_route_shares_one_error_window_across_its_refusals() {
        let http = DataPlaneHttp::new("http://127.0.0.1:9/v1/rerank", Duration::from_secs(1))
            .expect("a client");
        assert!(http.admit_error_log(), "the first refusal may log");
        assert!(
            !http.admit_error_log(),
            "the next one in the window may not"
        );
    }
}

#[cfg(test)]
mod artifact_reader_tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PAGE: &str = "https://shop.example/private/basket?session=abc";

    fn reader(server: &MockServer) -> DataPlaneArtifactReader {
        let http = DataPlaneHttp::new(
            format!("{}{ARTIFACT_LOOKUP_PATH}", server.uri()),
            Duration::from_secs(5),
        )
        .expect("a client")
        .with_api_key("dev-bypass");
        DataPlaneArtifactReader::new(http)
    }

    #[tokio::test]
    async fn a_stored_artifact_is_read_and_the_url_travels_in_the_body_only() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ARTIFACT_LOOKUP_PATH))
            .and(header("x-org-id", "org-a"))
            .and(body_partial_json(json!({ "url": PAGE, "max_bytes": 1024 })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "status": 200,
                    "final_url": "https://shop.example/private/basket",
                    "headers": [["content-type", "text/html"]],
                    "body_base64": "aGk=",
                    "captured_at": "2026-11-01T10:00:00Z"
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let found = reader(&server)
            .lookup("org-a", PAGE, Some(1024))
            .await
            .expect("a hit");
        let ArtifactLookup::Hit(artifact) = found else {
            panic!("expected a hit");
        };
        assert_eq!(artifact.status, 200);
        assert_eq!(artifact.body, b"hi".to_vec());
        assert_eq!(artifact.final_url, "https://shop.example/private/basket");
        assert_eq!(
            artifact.headers,
            vec![("content-type".to_string(), "text/html".to_string())]
        );

        let requests = server.received_requests().await.expect("recorded");
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0].url.query().is_none(),
            "the looked-up URL never travels in the query string"
        );
        assert!(!requests[0].url.path().contains("shop.example"));
    }

    #[tokio::test]
    async fn a_typed_404_is_a_miss() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ARTIFACT_LOOKUP_PATH))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": ARTIFACT_MISS_CODE, "message": "no artifact" }
            })))
            .mount(&server)
            .await;

        let found = reader(&server).lookup("org-a", PAGE, None).await.expect("a miss");
        assert_eq!(found, ArtifactLookup::Miss);
    }

    #[tokio::test]
    async fn a_bare_404_is_a_missing_route_not_a_miss() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let error = reader(&server)
            .lookup("org-a", PAGE, None)
            .await
            .expect_err("not a miss");
        assert_eq!(error.code, ErrorCode::RuntimeNotReady);
        assert!(!error.message.contains("shop.example"));
    }

    #[tokio::test]
    async fn a_refusal_is_typed_and_names_no_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let error = reader(&server)
            .lookup("org-a", PAGE, None)
            .await
            .expect_err("refused");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert!(!error.message.contains("shop.example"));
        assert!(error.details.is_none());
    }

    #[tokio::test]
    async fn an_unreadable_body_is_an_internal_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "status": 200, "final_url": "x", "body_base64": "!!not base64!!" }
            })))
            .mount(&server)
            .await;

        let error = reader(&server)
            .lookup("org-a", PAGE, None)
            .await
            .expect_err("unreadable");
        assert_eq!(error.code, ErrorCode::Internal);
    }
}
