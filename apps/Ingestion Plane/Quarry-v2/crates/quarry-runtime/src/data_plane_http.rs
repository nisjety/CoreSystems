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

use reqwest::{Client, Response, StatusCode};
use serde::Serialize;

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
        let bearer = match &self.tokens {
            Some(provider) => Some(
                provider
                    .token_for_org(org_id, request, false)
                    .await
                    .map_err(|error| {
                        DataPlaneCallError::Token(self.log_token_failure(error, request))
                    })?
                    .expose()
                    .to_owned(),
            ),
            None => self.api_key.clone(),
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
            if let Some(provider) = &self.tokens {
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
