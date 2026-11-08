//! E3.1-h: the real [`OtpSource`], a client of integration-corev2's executor-only
//! `mail.otp.read` operation (E3.1-g, ADR-2a).
//!
//! The client calls `POST {integration-core}/api/v1/connections/{id}/actions` with
//! `{"operation": "mail.otp.read", "params": {"approvalId": .., "windowSeconds": ..}}`
//! and reads back `{"found": true, "code": ".."}` or `{"found": false}` from the
//! action envelope (`data.action.result`). Nothing else of the mailbox ever reaches
//! Quarry: no subject, no sender, no message id.
//!
//! Identity: its own. Every read carries a bearer minted by Auth Core for the run's
//! organization from the manifest row `mail-otp-read` (audience `ingestion`, scope
//! `integration:otp:read` alone, a ZDR lane in the registry, R-13 PR 1). It never
//! uses the shared integration-core internal key, and quarry-edge is not a token
//! lease consumer: integration-core refuses both for this operation.
//!
//! Per-use approval: the edge's single-use set (E3.1-ab,
//! `used_sensitive_approval_grants`) admits a `fill_otp_ref` step once per approval
//! grant. This client keeps its own spent set as well, so the same approval grant
//! can never buy a second read through this process even if a caller skips the
//! edge. The grant is spent before the read, so a failed read also uses it up
//! (fail closed); the grant id is sent as `approvalId` for integration-core's audit
//! event only.
//!
//! The code: read into an [`OtpCode`] (zeroized on drop, redacted `Debug`); the
//! response buffer is zeroized too. No log line, error message or error detail
//! carries the value or the response body. Timeouts bound every call; redirects are
//! never followed (a redirect could move the bearer).
//!
//! Flag: `QUARRY_EDGE__FILL_OTP`, default off. The client is built disabled and
//! refuses every read (`Unsupported`, no mint, no request) until the executor
//! wiring passes [`crate::otp_source::fill_otp_enabled`] to
//! [`IntegrationCoreOtpSource::with_enabled`].

use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use crate::otp_source::{OtpCode, OtpRequest, OtpSource};
use crate::service_tokens::{ServiceTokenRequest, SharedServiceTokenProvider};

/// The integration-corev2 operation this client calls.
pub const MAIL_OTP_READ_OPERATION: &str = "mail.otp.read";

/// The total budget of one call (connect, send, read the answer).
pub const DEFAULT_OTP_READ_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// The look-back window integration-core accepts (`windowSeconds`, 30 to 1800).
const MIN_WINDOW_SECONDS: u32 = 30;
const MAX_WINDOW_SECONDS: u32 = 1_800;

/// The answer is a small JSON envelope; anything larger is refused unread.
const MAX_RESPONSE_BYTES: usize = 16 * 1024;

/// Opaque ids: what integration-core accepts as `approvalId`, and a safe path
/// segment for the connection id.
const MAX_ID_LEN: usize = 128;

/// How many spent approval grants the client remembers. The oldest is forgotten
/// first; broker grants expire long before this many fills happen in one process.
const MAX_SPENT_GRANTS: usize = 4_096;

/// The [`OtpSource`] that reads from integration-corev2 with quarry-edge's own
/// `ingestion` identity.
pub struct IntegrationCoreOtpSource {
    http: Client,
    base_url: String,
    tokens: SharedServiceTokenProvider,
    enabled: bool,
    timeout: Duration,
    window_seconds: Option<u32>,
    spent: Mutex<SpentGrants>,
}

impl IntegrationCoreOtpSource {
    /// A disabled client for `base_url` (the integration-core origin, e.g.
    /// `http://integration-core:8080`). Call [`Self::with_enabled`] with the flag.
    pub fn new(
        base_url: impl Into<String>,
        tokens: SharedServiceTokenProvider,
    ) -> QuarryResult<Self> {
        let base_url = base_url.into().trim().trim_end_matches('/').to_owned();
        let parsed = reqwest::Url::parse(&base_url).map_err(|_| {
            QuarryError::new(
                ErrorCode::Internal,
                "the integration-core URL for one-time-code reads must be an absolute URL",
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(QuarryError::new(
                ErrorCode::Internal,
                "the integration-core URL for one-time-code reads must be a plain http(s) origin",
            ));
        }
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DEFAULT_OTP_READ_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                QuarryError::new(
                    ErrorCode::Internal,
                    format!("otp client: {}", error.without_url()),
                )
            })?;
        Ok(Self {
            http,
            base_url,
            tokens,
            enabled: false,
            timeout: DEFAULT_OTP_READ_TIMEOUT,
            window_seconds: None,
            spent: Mutex::new(SpentGrants::default()),
        })
    }

    /// Turn reads on or off: pass `fill_otp_enabled()` (`QUARRY_EDGE__FILL_OTP`).
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The budget of one call; it never exceeds [`DEFAULT_OTP_READ_TIMEOUT`].
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.min(DEFAULT_OTP_READ_TIMEOUT);
        self
    }

    /// How far back integration-core looks for the code, clamped to its 30..=1800
    /// seconds. Unset, integration-core's default (600) applies.
    pub fn with_window_seconds(mut self, seconds: u32) -> Self {
        self.window_seconds = Some(seconds.clamp(MIN_WINDOW_SECONDS, MAX_WINDOW_SECONDS));
        self
    }

    /// Whether this approval grant was already spent on a read here.
    pub fn is_spent(&self, approval_grant_id: &str) -> bool {
        self.spent
            .lock()
            .map(|spent| spent.contains(approval_grant_id))
            .unwrap_or(true)
    }

    fn spend(&self, approval_grant_id: &str) -> QuarryResult<()> {
        let mut spent = self.spent.lock().map_err(|_| {
            QuarryError::new(ErrorCode::Internal, "one-time-code approval set is unavailable")
        })?;
        if !spent.insert(approval_grant_id) {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "this fill_otp approval was already used; a new approval is required",
            )
            .with_details(serde_json::json!({ "reason": "otp_approval_spent" })));
        }
        Ok(())
    }

    fn url_for(&self, mail_connection_id: &str) -> String {
        format!(
            "{}/api/v1/connections/{}/actions",
            self.base_url, mail_connection_id
        )
    }

    async fn send(
        &self,
        url: &str,
        body: &ActionBody<'_>,
        bearer: &str,
    ) -> QuarryResult<reqwest::Response> {
        self.http
            .post(url)
            .timeout(self.timeout)
            .bearer_auth(bearer)
            .header("content-type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|error| {
                let code = if error.is_timeout() {
                    ErrorCode::Timeout
                } else if error.is_connect() {
                    ErrorCode::UpstreamBlocked
                } else {
                    ErrorCode::DriverFailed
                };
                QuarryError::new(
                    code,
                    format!("one-time-code read failed: {}", error.without_url()),
                )
            })
    }
}

#[async_trait]
impl OtpSource for IntegrationCoreOtpSource {
    async fn read_code(&self, request: &OtpRequest) -> QuarryResult<OtpCode> {
        if !self.enabled {
            return Err(QuarryError::new(
                ErrorCode::Unsupported,
                "fill_otp is not enabled on this deployment (QUARRY_EDGE__FILL_OTP is off)",
            ));
        }
        validate_request(request)?;
        // The token request is the manifest row; build it before spending the
        // approval so a broken manifest does not burn a grant.
        let token_request = ServiceTokenRequest::from_manifest("mail-otp-read")?;
        self.spend(&request.approval_grant_id)?;

        let url = self.url_for(&request.mail_connection_id);
        let body = ActionBody {
            operation: MAIL_OTP_READ_OPERATION,
            params: ActionParams {
                approval_id: &request.approval_grant_id,
                window_seconds: self.window_seconds,
            },
        };
        let bearer = self
            .tokens
            .token_for_org(&request.org_id, &token_request, false)
            .await?;
        let mut response = self.send(&url, &body, bearer.expose()).await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            // integration-core refused the bearer before any read: one retry with
            // a fresh token cannot read twice.
            self.tokens
                .invalidate(&request.org_id, &token_request)
                .await;
            let refreshed = self
                .tokens
                .token_for_org(&request.org_id, &token_request, true)
                .await?;
            response = self.send(&url, &body, refreshed.expose()).await?;
        }

        let outcome = decode_response(response).await;
        match &outcome {
            Ok(_) => tracing::info!(
                outcome = "otp_read",
                org_id = %request.org_id,
                mail_connection_id = %request.mail_connection_id,
                approval_grant_id = %request.approval_grant_id,
                "one-time code read from integration-core"
            ),
            Err(error) => tracing::warn!(
                outcome = "otp_read_failed",
                error_code = ?error.code,
                org_id = %request.org_id,
                mail_connection_id = %request.mail_connection_id,
                approval_grant_id = %request.approval_grant_id,
                "one-time-code read from integration-core failed"
            ),
        }
        outcome
    }
}

/// Refuse ids that are empty, too long, or not plain opaque ids. Never echoes them.
fn validate_request(request: &OtpRequest) -> QuarryResult<()> {
    let bad = |field: &str| {
        QuarryError::new(
            ErrorCode::BadRequest,
            format!("one-time-code read: {field} is not a valid id"),
        )
        .with_details(serde_json::json!({ "reason": "otp_request_id", "field": field }))
    };
    if request.org_id.trim().is_empty() || request.org_id.len() > MAX_ID_LEN {
        return Err(bad("org_id"));
    }
    if !is_opaque_id(&request.mail_connection_id) {
        return Err(bad("mail_connection_id"));
    }
    if !is_opaque_id(&request.approval_grant_id) {
        return Err(bad("approval_grant_id"));
    }
    Ok(())
}

/// `^[A-Za-z0-9._:-]{1,128}$`, and not a dot path segment: integration-core's
/// `approvalId` rule, and safe as a URL path segment.
fn is_opaque_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// Map the answer to a code. The body is read into a zeroizing buffer, bounded, and
/// never copied into an error.
async fn decode_response(response: reqwest::Response) -> QuarryResult<OtpCode> {
    let status = response.status();
    if !status.is_success() {
        let code = match status.as_u16() {
            401 | 403 => ErrorCode::Forbidden,
            404 => ErrorCode::NotFound,
            408 | 504 => ErrorCode::Timeout,
            429 => ErrorCode::RateLimited,
            400 | 409 | 422 => ErrorCode::BadRequest,
            501 => ErrorCode::Unsupported,
            _ => ErrorCode::DriverFailed,
        };
        let upstream = upstream_error_code(response).await;
        let mut error = QuarryError::new(
            code,
            format!(
                "integration-core refused the one-time-code read (status {})",
                status.as_u16()
            ),
        );
        if let Some(upstream) = upstream {
            error = error.with_details(serde_json::json!({ "upstream_code": upstream }));
        }
        return Err(error);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(too_large());
    }
    let bytes = Zeroizing::new(
        response
            .bytes()
            .await
            .map_err(|error| {
                let code = if error.is_timeout() {
                    ErrorCode::Timeout
                } else {
                    ErrorCode::DriverFailed
                };
                QuarryError::new(
                    code,
                    format!(
                        "one-time-code answer could not be read: {}",
                        error.without_url()
                    ),
                )
            })?
            .to_vec(),
    );
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(too_large());
    }
    let mut envelope: ActionEnvelope = serde_json::from_slice(&bytes).map_err(|_| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            "integration-core answered the one-time-code read with an unreadable body",
        )
    })?;
    let action = &mut envelope.data.action;
    if action.operation != MAIL_OTP_READ_OPERATION {
        return Err(QuarryError::new(
            ErrorCode::DriverFailed,
            "integration-core answered with another operation than mail.otp.read",
        ));
    }
    if !action.result.found {
        return Err(QuarryError::new(
            ErrorCode::NotFound,
            "no one-time code arrived in the mailbox within the window",
        )
        .with_details(serde_json::json!({ "reason": "otp_not_found" })));
    }
    match action.result.code.take() {
        // Moved, not copied: OtpCode wraps this buffer and zeroes it on drop.
        Some(code) => OtpCode::new(code),
        None => Err(QuarryError::new(
            ErrorCode::DriverFailed,
            "integration-core said a code was found but sent none",
        )),
    }
}

fn too_large() -> QuarryError {
    QuarryError::new(
        ErrorCode::DriverFailed,
        "integration-core's one-time-code answer is larger than allowed",
    )
}

/// integration-core's own error code (`error.code`), if it is a short snake_case
/// token. Never the message: only the code is safe to carry.
async fn upstream_error_code(response: reqwest::Response) -> Option<String> {
    let bytes = Zeroizing::new(response.bytes().await.ok()?.to_vec());
    if bytes.len() > MAX_RESPONSE_BYTES {
        return None;
    }
    let parsed: ErrorEnvelope = serde_json::from_slice(&bytes).ok()?;
    let code = parsed.error?.code?;
    let safe = !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    safe.then_some(code)
}

#[derive(Serialize)]
struct ActionBody<'a> {
    operation: &'static str,
    params: ActionParams<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionParams<'a> {
    approval_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_seconds: Option<u32>,
}

#[derive(Deserialize)]
struct ActionEnvelope {
    data: ActionData,
}

#[derive(Deserialize)]
struct ActionData {
    action: ActionResult,
}

#[derive(Deserialize)]
struct ActionResult {
    operation: String,
    result: OtpResult,
}

#[derive(Deserialize)]
struct OtpResult {
    found: bool,
    #[serde(default)]
    code: Option<String>,
}

impl Drop for OtpResult {
    fn drop(&mut self) {
        if let Some(code) = self.code.take() {
            drop(Zeroizing::new(code));
        }
    }
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    #[serde(default)]
    error: Option<ErrorBody>,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    code: Option<String>,
}

/// The approval grants already spent on a read, bounded, oldest forgotten first.
#[derive(Default)]
struct SpentGrants {
    set: HashSet<String>,
    order: VecDeque<String>,
}

impl SpentGrants {
    fn contains(&self, id: &str) -> bool {
        self.set.contains(id)
    }

    /// False when `id` was already spent.
    fn insert(&mut self, id: &str) -> bool {
        if self.set.contains(id) {
            return false;
        }
        if self.order.len() >= MAX_SPENT_GRANTS {
            if let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            }
        }
        self.set.insert(id.to_owned());
        self.order.push_back(id.to_owned());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::service_tokens::{ServiceBearer, ServiceTokenProvider};

    /// The fixture code (the `/otp-login` page's value; not a secret).
    const CODE: &str = "482915";
    const ACTIONS_PATH: &str = "/api/v1/connections/conn-mail-1/actions";

    /// One mint: the org, the request (audience, scopes, reason) and the refresh bit.
    type Mint = (String, ServiceTokenRequest, bool);

    #[derive(Default)]
    struct RecordingTokens {
        calls: Mutex<Vec<Mint>>,
        invalidated: Mutex<usize>,
    }

    #[async_trait]
    impl ServiceTokenProvider for RecordingTokens {
        async fn token_for_org(
            &self,
            org_id: &str,
            request: &ServiceTokenRequest,
            force_refresh: bool,
        ) -> QuarryResult<ServiceBearer> {
            let mut calls = self.calls.lock().unwrap();
            calls.push((org_id.to_owned(), request.clone(), force_refresh));
            let token = if force_refresh {
                "refreshed.ingestion.token"
            } else {
                "first.ingestion.token"
            };
            Ok(ServiceBearer::from_test_token(token))
        }

        async fn invalidate(&self, _org_id: &str, _request: &ServiceTokenRequest) {
            *self.invalidated.lock().unwrap() += 1;
        }
    }

    fn request() -> OtpRequest {
        OtpRequest {
            org_id: "org-1".into(),
            mail_connection_id: "conn-mail-1".into(),
            approval_grant_id: "grant-1".into(),
            origin: "https://login.example.test".into(),
        }
    }

    fn found(code: &str) -> serde_json::Value {
        json!({
            "success": true,
            "data": { "action": {
                "providerKey": "microsoft",
                "operation": "mail.otp.read",
                "result": { "found": true, "code": code }
            }}
        })
    }

    fn client(server: &MockServer, tokens: Arc<RecordingTokens>) -> IntegrationCoreOtpSource {
        IntegrationCoreOtpSource::new(server.uri(), tokens)
            .expect("client")
            .with_enabled(true)
    }

    #[tokio::test]
    async fn reads_the_code_with_the_ingestion_token_of_the_manifest_row() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ACTIONS_PATH))
            .and(header("authorization", "Bearer first.ingestion.token"))
            .and(body_json(json!({
                "operation": "mail.otp.read",
                "params": { "approvalId": "grant-1", "windowSeconds": 300 }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(found(CODE)))
            .expect(1)
            .mount(&server)
            .await;
        let tokens = Arc::new(RecordingTokens::default());
        let source = client(&server, tokens.clone()).with_window_seconds(300);

        let code = source.read_code(&request()).await.expect("code");
        assert_eq!(code.expose_for_insert(), CODE);
        assert_eq!(format!("{code:?}"), "[redacted:otp]");

        // The mint is exactly the manifest row: audience ingestion, the code-read
        // scope alone (service_tokens' EXPECTED_REQUESTS pins the row itself).
        let row = ServiceTokenRequest::from_manifest("mail-otp-read").expect("manifest row");
        assert_eq!(row.scopes(), ["integration:otp:read".to_owned()].as_slice());
        let calls = tokens.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (org, minted, force) = &calls[0];
        assert_eq!(org, "org-1");
        assert_eq!(minted, &row);
        assert!(!force);
    }

    #[tokio::test]
    async fn the_same_approval_never_buys_a_second_read() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ACTIONS_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(found(CODE)))
            .expect(1)
            .mount(&server)
            .await;
        let source = client(&server, Arc::new(RecordingTokens::default()));

        source.read_code(&request()).await.expect("first read");
        assert!(source.is_spent("grant-1"));
        let error = source.read_code(&request()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(
            error.details.as_ref().and_then(|d| d["reason"].as_str()),
            Some("otp_approval_spent")
        );
    }

    #[tokio::test]
    async fn a_failed_read_still_spends_the_approval() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ACTIONS_PATH))
            .respond_with(ResponseTemplate::new(502))
            .expect(1)
            .mount(&server)
            .await;
        let source = client(&server, Arc::new(RecordingTokens::default()));

        let first = source.read_code(&request()).await.unwrap_err();
        assert_eq!(first.code, ErrorCode::DriverFailed);
        let second = source.read_code(&request()).await.unwrap_err();
        assert_eq!(second.code, ErrorCode::Forbidden);
    }

    #[tokio::test]
    async fn disabled_by_default_reads_nothing_and_mints_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(found(CODE)))
            .expect(0)
            .mount(&server)
            .await;
        let tokens = Arc::new(RecordingTokens::default());
        let source = IntegrationCoreOtpSource::new(server.uri(), tokens.clone()).unwrap();

        let error = source.read_code(&request()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(tokens.calls.lock().unwrap().is_empty());
        assert!(!source.is_spent("grant-1"));
    }

    #[tokio::test]
    async fn a_refused_bearer_is_refreshed_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(ACTIONS_PATH))
            .and(header("authorization", "Bearer first.ingestion.token"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(ACTIONS_PATH))
            .and(header("authorization", "Bearer refreshed.ingestion.token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(found(CODE)))
            .expect(1)
            .mount(&server)
            .await;
        let tokens = Arc::new(RecordingTokens::default());
        let source = client(&server, tokens.clone());

        let code = source.read_code(&request()).await.expect("code");
        assert_eq!(code.expose_for_insert(), CODE);
        assert_eq!(*tokens.invalidated.lock().unwrap(), 1);
        let forced: Vec<bool> = tokens.calls.lock().unwrap().iter().map(|c| c.2).collect();
        assert_eq!(forced, vec![false, true]);
    }

    #[tokio::test]
    async fn no_code_in_the_window_is_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "data": { "action": {
                    "providerKey": "google",
                    "operation": "mail.otp.read",
                    "result": { "found": false }
                }}
            })))
            .mount(&server)
            .await;
        let source = client(&server, Arc::new(RecordingTokens::default()));

        let error = source.read_code(&request()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn refusals_carry_the_upstream_code_and_never_the_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "success": false,
                "error": { "code": "otp_caller_not_allowed", "message": "body text 482915" }
            })))
            .mount(&server)
            .await;
        let source = client(&server, Arc::new(RecordingTokens::default()));

        let error = source.read_code(&request()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(
            error.details.as_ref().and_then(|d| d["upstream_code"].as_str()),
            Some("otp_caller_not_allowed")
        );
        let rendered = format!("{error:?} {}", serde_json::to_string(&error).unwrap());
        assert!(!rendered.contains(CODE), "the body leaked into the error");
        assert!(!rendered.contains("body text"));
    }

    #[tokio::test]
    async fn a_code_of_the_wrong_shape_is_refused_without_its_value() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(found("12 34<script>")))
            .mount(&server)
            .await;
        let source = client(&server, Arc::new(RecordingTokens::default()));

        let error = source.read_code(&request()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::BadRequest);
        let rendered = format!("{error:?}");
        assert!(!rendered.contains("12 34"));
        assert!(!rendered.contains("script"));
    }

    #[tokio::test]
    async fn another_operation_in_the_answer_is_refused() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "data": { "action": {
                    "providerKey": "microsoft",
                    "operation": "mail.messages",
                    "result": { "found": true, "code": CODE }
                }}
            })))
            .mount(&server)
            .await;
        let source = client(&server, Arc::new(RecordingTokens::default()));

        let error = source.read_code(&request()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DriverFailed);
        assert!(!format!("{error:?}").contains(CODE));
    }

    #[tokio::test]
    async fn a_slow_answer_times_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(found(CODE))
                    .set_delay(Duration::from_millis(500)),
            )
            .mount(&server)
            .await;
        let source = client(&server, Arc::new(RecordingTokens::default()))
            .with_timeout(Duration::from_millis(50));

        let error = source.read_code(&request()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
    }

    #[tokio::test]
    async fn an_oversized_answer_is_refused() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(MAX_RESPONSE_BYTES + 1)))
            .mount(&server)
            .await;
        let source = client(&server, Arc::new(RecordingTokens::default()));

        let error = source.read_code(&request()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::DriverFailed);
    }

    #[tokio::test]
    async fn ids_that_are_not_opaque_are_refused_before_any_call() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(found(CODE)))
            .expect(0)
            .mount(&server)
            .await;
        let tokens = Arc::new(RecordingTokens::default());
        let source = client(&server, tokens.clone());

        for (connection, grant) in [
            ("../admin", "grant-2"),
            ("conn/1", "grant-3"),
            ("..", "grant-4"),
            ("conn-mail-1", "grant with space"),
            ("conn-mail-1", ""),
        ] {
            let mut bad = request();
            bad.mail_connection_id = connection.into();
            bad.approval_grant_id = grant.into();
            let error = source.read_code(&bad).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::BadRequest, "{connection:?} {grant:?}");
        }
        assert!(tokens.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn only_an_http_origin_is_accepted() {
        let tokens: SharedServiceTokenProvider = Arc::new(RecordingTokens::default());
        for bad in ["", "integration-core", "ftp://ic", "http://ic/?q=1", "http://ic/#f"] {
            assert!(
                IntegrationCoreOtpSource::new(bad, tokens.clone()).is_err(),
                "{bad:?}"
            );
        }
        assert!(IntegrationCoreOtpSource::new("http://integration-core:8080/", tokens).is_ok());
    }

    #[test]
    fn the_window_is_clamped_to_what_integration_core_accepts() {
        let tokens: SharedServiceTokenProvider = Arc::new(RecordingTokens::default());
        let low = IntegrationCoreOtpSource::new("http://ic", tokens.clone())
            .unwrap()
            .with_window_seconds(1);
        assert_eq!(low.window_seconds, Some(MIN_WINDOW_SECONDS));
        let high = IntegrationCoreOtpSource::new("http://ic", tokens)
            .unwrap()
            .with_window_seconds(86_400);
        assert_eq!(high.window_seconds, Some(MAX_WINDOW_SECONDS));
    }

    #[test]
    fn the_spent_set_is_bounded_and_forgets_the_oldest_first() {
        let mut spent = SpentGrants::default();
        for i in 0..MAX_SPENT_GRANTS {
            assert!(spent.insert(&format!("g{i}")));
        }
        assert!(!spent.insert("g0"));
        assert!(spent.insert("one-more"));
        assert!(!spent.contains("g0"));
        assert!(spent.contains("g1"));
        assert_eq!(spent.set.len(), MAX_SPENT_GRANTS);
    }
}
