use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::body;
use vsv2_auth::{Scope, Tokens};
use vsv2_common::{BackgroundMode, Error, RateLimitSnapshot, Result, Source, TenantId};
use vsv2_ratelimit::Limiter;

/// Longest wait for one response. Deep pages of the expand-heavy endpoints are slow: pages of
/// `customer_invoices` have taken up to 99 s, and page 22 of Hygiene Systems' walk exceeded 60 s on
/// all three attempts. A response that is merely slow is better than a failed sync of the endpoint.
const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(180);
const DEFAULT_BACKGROUND_POLL: Duration = Duration::from_secs(5);
const DEFAULT_BACKGROUND_TIMEOUT: Duration = Duration::from_secs(900);
const TRANSPORT_RETRY_DELAYS: [Duration; 2] = [Duration::from_millis(250), Duration::from_secs(1)];
/// Waits before retrying a 502/503/504. Visma's gateway times out under load (a page with
/// `expandApplications` takes ~20s), and without a retry one such answer 100 pages into a
/// 35-minute walk discards everything fetched so far.
const GATEWAY_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(10),
    Duration::from_secs(30),
];
const VISMA_APPLICATION_TYPE: &str = "Visma.net Financials";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackgroundOpts {
    pub mode: BackgroundMode,
    pub poll_secs: u64,
    pub timeout_secs: u64,
}

impl Default for BackgroundOpts {
    fn default() -> Self {
        Self {
            mode: BackgroundMode::Off,
            poll_secs: DEFAULT_BACKGROUND_POLL.as_secs(),
            timeout_secs: DEFAULT_BACKGROUND_TIMEOUT.as_secs(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ErpRestClient {
    pub base_url: String,
    pub oauth: Tokens,
    pub limiter: Limiter,
    http: reqwest::Client,
    gateway_retry_delays: Vec<Duration>,
}

#[derive(Debug, Clone)]
pub struct JsonResponse {
    pub body: serde_json::Value,
    pub status: u16,
    pub elapsed_ms: i64,
    pub rate_limit: RateLimitSnapshot,
}

impl ErpRestClient {
    /// `oauth` is the service's client credentials (an `OAuthClient`) or a signed-in person's tokens.
    pub fn new(base_url: impl Into<String>, oauth: impl Into<Tokens>, limiter: Limiter) -> Self {
        let mut url: String = base_url.into();
        if !url.ends_with('/') {
            url.push('/');
        }
        Self {
            base_url: url,
            oauth: oauth.into(),
            limiter,
            http: reqwest::Client::builder()
                .timeout(DEFAULT_HTTP_TIMEOUT)
                .gzip(true)
                .build()
                .expect("reqwest client builds"),
            gateway_retry_delays: GATEWAY_RETRY_DELAYS.to_vec(),
        }
    }

    /// Single GET. Caller composes pagination on top via [`paginate`].
    pub async fn get_json(
        &self,
        tenant: &TenantId,
        path: &str,
        query: &[(String, String)],
        background: &BackgroundOpts,
    ) -> Result<serde_json::Value> {
        Ok(self
            .get_json_with_meta(tenant, path, query, background)
            .await?
            .body)
    }

    pub async fn get_json_with_meta(
        &self,
        tenant: &TenantId,
        path: &str,
        query: &[(String, String)],
        background: &BackgroundOpts,
    ) -> Result<JsonResponse> {
        self.limiter.acquire().await?;
        let started = Instant::now();

        let url = self.url_for(path)?;
        let scope = self.scope_for_url(&url);
        let token = self.oauth.token_for(tenant, scope).await?;
        debug!(tenant = %tenant, %url, "erp_rest: GET");

        let mut resp = self
            .send_get_with_retry(
                tenant,
                &token,
                &url,
                query,
                background_header(background.mode),
            )
            .await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            resp = self
                .retry_after_401(
                    tenant,
                    scope,
                    &url,
                    query,
                    background_header(background.mode),
                )
                .await?;
        }
        let status = resp.status();
        let headers = resp.headers().clone();
        let rate_limit = rate_limit_snapshot(&headers);
        self.observe_rate_limit_headers(&headers).await?;

        if status.as_u16() == 429 {
            let retry = headers
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(60);
            let until = chrono::Utc::now() + chrono::Duration::seconds(retry);
            self.limiter.engage_cooldown_until(until).await?;
            warn!(retry_after = retry, %url, "erp_rest 429");
            return Err(Error::RateLimited {
                kind: Source::ErpRest.as_str(),
                until,
            });
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::Http(format!("read body: {e}")))?;
        let text = if status.is_success() {
            body::body_text(&bytes)?
        } else {
            body::error_text(&bytes)
        };
        drop(bytes);

        if status.as_u16() == 202 {
            let accepted = parse_background_accepted(&text)?;
            return self
                .poll_background(tenant, &accepted, background, started)
                .await;
        }

        if !status.is_success() {
            return Err(Error::UpstreamStatus {
                status: status.as_u16(),
                body: text,
            });
        }

        let body = serde_json::from_str(&text).map_err(|e| {
            Error::Http(format!(
                "parse erp_rest body: {e}; head: {}",
                vsv2_common::text::printable(&text, 256)
            ))
        })?;

        Ok(JsonResponse {
            body,
            status: status.as_u16(),
            elapsed_ms: elapsed_ms(started),
            rate_limit,
        })
    }

    async fn get_json_direct(
        &self,
        tenant: &TenantId,
        path: &str,
        query: &[(String, String)],
    ) -> Result<serde_json::Value> {
        Ok(self
            .get_json_direct_with_meta(tenant, path, query)
            .await?
            .body)
    }

    async fn get_json_direct_with_meta(
        &self,
        tenant: &TenantId,
        path: &str,
        query: &[(String, String)],
    ) -> Result<JsonResponse> {
        self.limiter.acquire().await?;
        let started = Instant::now();

        let url = self.url_for(path)?;
        let scope = self.scope_for_url(&url);
        let token = self.oauth.token_for(tenant, scope).await?;
        debug!(tenant = %tenant, %url, "erp_rest: background GET");

        let mut resp = self
            .send_get_with_retry(tenant, &token, &url, query, None)
            .await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            resp = self
                .retry_after_401(tenant, scope, &url, query, None)
                .await?;
        }
        let status = resp.status();
        let headers = resp.headers().clone();
        let rate_limit = rate_limit_snapshot(&headers);
        self.observe_rate_limit_headers(&headers).await?;

        if status.as_u16() == 429 {
            let retry = headers
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(60);
            let until = chrono::Utc::now() + chrono::Duration::seconds(retry);
            self.limiter.engage_cooldown_until(until).await?;
            return Err(Error::RateLimited {
                kind: Source::ErpRest.as_str(),
                until,
            });
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::Http(format!("read body: {e}")))?;
        let text = if status.is_success() {
            body::body_text(&bytes)?
        } else {
            body::error_text(&bytes)
        };
        drop(bytes);

        if !status.is_success() {
            return Err(Error::UpstreamStatus {
                status: status.as_u16(),
                body: text,
            });
        }

        let body = serde_json::from_str(&text).map_err(|e| {
            Error::Http(format!(
                "parse erp_rest body: {e}; head: {}",
                vsv2_common::text::printable(&text, 256)
            ))
        })?;

        Ok(JsonResponse {
            body,
            status: status.as_u16(),
            elapsed_ms: elapsed_ms(started),
            rate_limit,
        })
    }

    fn build_get_request(
        &self,
        tenant: &TenantId,
        token: &str,
        url: &str,
        query: &[(String, String)],
        background_header_value: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .get(url)
            .bearer_auth(token)
            .header("ipp-company-id", tenant.as_str())
            .header("ipp-application-type", VISMA_APPLICATION_TYPE);
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(header_value) = background_header_value {
            request = request.header("erp-api-background", header_value);
        }
        request
    }

    /// The request was answered 401. Try once with a new token for the same scope, then, if that
    /// is refused as well, once with the other scope.
    ///
    /// A cached token can be rejected without being wrong for the scope: it expired while the
    /// request was in flight after a long pause. Asking for the other scope first would hide that
    /// behind a second 401.
    async fn retry_after_401(
        &self,
        tenant: &TenantId,
        scope: Scope,
        url: &str,
        query: &[(String, String)],
        background_header_value: Option<&str>,
    ) -> Result<reqwest::Response> {
        debug!(tenant = %tenant, %url, ?scope, "erp_rest: 401, retrying with a new token");
        self.oauth.forget_token(tenant, scope).await;
        let fresh = self.oauth.token_for(tenant, scope).await?;
        let resp = self
            .send_get_with_retry(tenant, &fresh, url, query, background_header_value)
            .await?;
        if resp.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }
        // A signed-in person has one token per company for every scope, so "the other scope" is the same token.
        let other = fallback_scope(scope).filter(|_| self.oauth.token_depends_on_scope());
        let Some(other) = other else {
            return Ok(resp);
        };
        debug!(
            tenant = %tenant,
            %url,
            initial_scope = ?scope,
            fallback_scope = ?other,
            "erp_rest: retrying GET with fallback OAuth scope after a second 401"
        );
        let other_token = self.oauth.token_for(tenant, other).await?;
        self.send_get_with_retry(tenant, &other_token, url, query, background_header_value)
            .await
    }

    async fn send_get_with_retry(
        &self,
        tenant: &TenantId,
        token: &str,
        url: &str,
        query: &[(String, String)],
        background_header_value: Option<&str>,
    ) -> Result<reqwest::Response> {
        let mut attempt = 0usize;
        let mut gateway_attempt = 0usize;
        loop {
            let request =
                self.build_get_request(tenant, token, url, query, background_header_value);
            match request.send().await {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if matches!(status, 502..=504) {
                        if let Some(delay) = self.gateway_retry_delays.get(gateway_attempt) {
                            warn!(
                                tenant = %tenant,
                                %url,
                                status,
                                attempt = gateway_attempt + 1,
                                delay_ms = delay.as_millis() as u64,
                                "erp_rest gateway error, retrying"
                            );
                            tokio::time::sleep(*delay).await;
                            gateway_attempt += 1;
                            continue;
                        }
                    }
                    return Ok(response);
                }
                Err(error) => {
                    let Some(delay) = TRANSPORT_RETRY_DELAYS.get(attempt).copied() else {
                        return Err(Error::Http(format!("erp_rest GET {url}: {error}")));
                    };
                    warn!(
                        tenant = %tenant,
                        %url,
                        attempt = attempt + 1,
                        delay_ms = delay.as_millis() as u64,
                        error = %error,
                        "erp_rest transport failure, retrying"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn poll_background(
        &self,
        tenant: &TenantId,
        accepted: &BackgroundAccepted,
        opts: &BackgroundOpts,
        request_started: Instant,
    ) -> Result<JsonResponse> {
        let started = request_started;
        let timeout = Duration::from_secs(opts.timeout_secs.max(1));
        let poll = Duration::from_secs(opts.poll_secs.max(1));
        let state_path = accepted
            .state_location
            .as_deref()
            .unwrap_or(accepted.state_path.as_str());

        loop {
            if started.elapsed() >= timeout {
                return Err(Error::Http(format!(
                    "background operation {} timed out after {}s",
                    accepted.id, opts.timeout_secs
                )));
            }

            tokio::time::sleep(poll).await;
            let state = self.get_json_direct(tenant, state_path, &[]).await?;
            let status = state
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if status.eq_ignore_ascii_case("Finished") {
                let status_code = state
                    .get("statusCode")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(200) as u16;
                if !(200..300).contains(&status_code) {
                    return Err(Error::UpstreamStatus {
                        status: status_code,
                        body: state.to_string(),
                    });
                }

                let content_path = state
                    .get("contentLocation")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("/v1/background/{}/content", accepted.id));
                let mut response = self
                    .get_json_direct_with_meta(tenant, &content_path, &[])
                    .await?;
                response.elapsed_ms = elapsed_ms(started);
                return Ok(response);
            }

            if status.eq_ignore_ascii_case("Failed") {
                let status_code = state
                    .get("statusCode")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(500) as u16;
                return Err(Error::UpstreamStatus {
                    status: status_code,
                    body: state.to_string(),
                });
            }
        }
    }

    async fn observe_rate_limit_headers(&self, headers: &reqwest::header::HeaderMap) -> Result<()> {
        let limit = header_u32(headers, "x-ratelimit-limit");
        let remaining = header_u32(headers, "x-ratelimit-remaining");
        let reset = header_i64(headers, "x-ratelimit-reset");
        let policy = header_str(headers, "x-ratelimit-policy");
        self.limiter
            .observe_rate_limit(limit, remaining, reset, policy.as_deref())
            .await
    }

    fn url_for(&self, path: &str) -> Result<String> {
        if let Ok(url) = url::Url::parse(path) {
            if matches!(url.scheme(), "http" | "https") {
                return Ok(url.to_string());
            }
        }

        let base = url::Url::parse(&self.base_url)
            .map_err(|e| Error::Config(format!("invalid ERP REST base URL: {e}")))?;
        base.join(path.trim_start_matches('/'))
            .map(|u| u.to_string())
            .map_err(|e| Error::Config(format!("invalid ERP REST path {path}: {e}")))
    }

    fn scope_for_url(&self, url: &str) -> Scope {
        match url::Url::parse(url) {
            Ok(parsed) if parsed.host_str() == Some("salesorder.visma.net") => Scope::SalesOrder,
            Ok(parsed)
                if parsed.path() == "/api/v3/SalesOrders"
                    || parsed.path().starts_with("/api/v3/SalesOrders/") =>
            {
                Scope::SalesOrder
            }
            _ if url == "/api/v3/SalesOrders"
                || url.starts_with("/api/v3/SalesOrders/")
                || url.starts_with("/api/v3/SalesOrders?") =>
            {
                Scope::SalesOrder
            }
            _ => Scope::Erp,
        }
    }
}

fn fallback_scope(scope: Scope) -> Option<Scope> {
    match scope {
        Scope::SalesOrder => Some(Scope::Erp),
        Scope::Erp => Some(Scope::SalesOrder),
        Scope::Datamart | Scope::Expense | Scope::ErpWrite | Scope::SalesOrderWrite => None,
    }
}

#[derive(Debug)]
struct BackgroundAccepted {
    id: String,
    state_location: Option<String>,
    state_path: String,
}

fn background_header(mode: BackgroundMode) -> Option<&'static str> {
    match mode {
        BackgroundMode::Off => None,
        BackgroundMode::None => Some("none"),
        BackgroundMode::Subscription => Some("subscription"),
    }
}

fn parse_background_accepted(text: &str) -> Result<BackgroundAccepted> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        Error::Http(format!(
            "parse background accepted body: {e}; head: {}",
            vsv2_common::text::printable(text, 256)
        ))
    })?;
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Http(format!("background accepted response missing id: {value}")))?
        .to_string();
    let state_location = value
        .get("stateLocation")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Ok(BackgroundAccepted {
        state_path: format!("/v1/background/{id}"),
        id,
        state_location,
    })
}

fn header_str(headers: &reqwest::header::HeaderMap, key: &'static str) -> Option<String> {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn header_u32(headers: &reqwest::header::HeaderMap, key: &'static str) -> Option<u32> {
    header_str(headers, key).and_then(|s| s.parse().ok())
}

fn header_i64(headers: &reqwest::header::HeaderMap, key: &'static str) -> Option<i64> {
    header_str(headers, key).and_then(|s| s.parse().ok())
}

fn rate_limit_snapshot(headers: &reqwest::header::HeaderMap) -> RateLimitSnapshot {
    RateLimitSnapshot {
        limit: header_u32(headers, "x-ratelimit-limit"),
        remaining: header_u32(headers, "x-ratelimit-remaining"),
        reset: header_i64(headers, "x-ratelimit-reset"),
        policy: header_str(headers, "x-ratelimit-policy"),
        retry_after_secs: header_i64(headers, "retry-after"),
    }
}

fn elapsed_ms(started: Instant) -> i64 {
    started.elapsed().as_millis().min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use vsv2_auth::Scope;

    #[test]
    fn background_mode_maps_to_visma_header_values() {
        assert_eq!(background_header(BackgroundMode::Off), None);
        assert_eq!(background_header(BackgroundMode::None), Some("none"));
        assert_eq!(
            background_header(BackgroundMode::Subscription),
            Some("subscription")
        );
    }

    #[test]
    fn parses_background_accepted_response() {
        let accepted = parse_background_accepted(
            r#"{"id":"2f9dbb8a-d3ac-41db-a693-9f7b594ea0a4","stateLocation":"/v1/background/2f9dbb8a-d3ac-41db-a693-9f7b594ea0a4"}"#,
        )
        .expect("accepted response");

        assert_eq!(accepted.id, "2f9dbb8a-d3ac-41db-a693-9f7b594ea0a4");
        assert_eq!(
            accepted.state_location.as_deref(),
            Some("/v1/background/2f9dbb8a-d3ac-41db-a693-9f7b594ea0a4")
        );
    }

    #[test]
    fn salesorder_urls_use_salesorder_scope() {
        let oauth = vsv2_auth::OAuthClient::new(
            "https://connect.visma.com/connect/token",
            "client-id",
            "client-secret",
        )
        .with_scope(Scope::Erp, "financials:read")
        .with_scope(Scope::SalesOrder, "visma.net.erp.salesorder:read");
        let client = ErpRestClient::new(
            "https://api.finance.visma.net",
            oauth,
            Limiter::new(Source::ErpRest, 1_000, 100_000),
        );

        assert_eq!(
            client.scope_for_url("https://salesorder.visma.net/api/v3/SalesOrders"),
            Scope::SalesOrder
        );
        assert_eq!(
            client.scope_for_url("/api/v3/SalesOrders?pageIndex=0"),
            Scope::SalesOrder
        );
        assert_eq!(client.scope_for_url("/v1/account"), Scope::Erp);
    }

    #[test]
    fn fallback_scope_maps_erp_and_salesorder_only() {
        assert_eq!(fallback_scope(Scope::SalesOrder), Some(Scope::Erp));
        assert_eq!(fallback_scope(Scope::Erp), Some(Scope::SalesOrder));
        assert_eq!(fallback_scope(Scope::Datamart), None);
        assert_eq!(fallback_scope(Scope::Expense), None);
    }

    #[tokio::test]
    async fn a_401_on_a_cached_token_is_retried_once_with_a_fresh_token() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("server addr");
        let token_requests = Arc::new(AtomicUsize::new(0));
        let token_requests_task = token_requests.clone();

        let server = tokio::spawn(async move {
            // token, api (rejected), token again, api (accepted)
            for _ in 0..4 {
                let (mut socket, _) = listener.accept().await.expect("accept connection");
                let mut buffer = [0_u8; 2048];
                let bytes_read = socket.read(&mut buffer).await.expect("read request");
                let request = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();

                let (status, body) = if request.starts_with("POST /token ") {
                    let n = token_requests_task.fetch_add(1, Ordering::SeqCst);
                    (
                        200,
                        format!(r#"{{"access_token":"token-{n}","expires_in":3600}}"#),
                    )
                } else if request.contains("authorization: Bearer token-0")
                    || request.contains("Authorization: Bearer token-0")
                {
                    (401, r#"{"error":"invalid_token"}"#.to_string())
                } else {
                    (200, r#"[{"number":"C0001"}]"#.to_string())
                };
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write response");
            }
        });

        let oauth = vsv2_auth::OAuthClient::new(
            format!("http://{address}/token"),
            "client-id",
            "client-secret",
        )
        .with_scope(Scope::Erp, "financials:read");
        let client = ErpRestClient::new(
            format!("http://{address}"),
            oauth,
            Limiter::new(Source::ErpRest, 1_000, 100_000),
        );
        let tenant = TenantId::new("33f319b9-4d57-11ee-960c-025417856183");

        let response = client
            .get_json_with_meta(&tenant, "/v1/customer", &[], &BackgroundOpts::default())
            .await
            .expect("a new token should get through");

        assert_eq!(response.status, 200);
        assert_eq!(response.body[0]["number"], "C0001");
        assert_eq!(
            token_requests.load(Ordering::SeqCst),
            2,
            "one new token, not more"
        );
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn get_json_with_meta_retries_dropped_transport_connection() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("server addr");
        let api_attempts = Arc::new(AtomicUsize::new(0));
        let api_attempts_task = api_attempts.clone();

        let server = tokio::spawn(async move {
            for _ in 0..3 {
                let (mut socket, _) = listener.accept().await.expect("accept connection");
                let mut buffer = [0_u8; 2048];
                let bytes_read = socket.read(&mut buffer).await.expect("read request");
                let request = String::from_utf8_lossy(&buffer[..bytes_read]);

                if request.starts_with("POST /token ") {
                    let body = r#"{"access_token":"test-token","expires_in":3600}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    socket
                        .write_all(response.as_bytes())
                        .await
                        .expect("write token response");
                    continue;
                }

                if request.starts_with("GET /v1/customer ") {
                    let attempt = api_attempts_task.fetch_add(1, Ordering::SeqCst);
                    if attempt == 0 {
                        continue;
                    }

                    let body = r#"[{"number":"C0001"}]"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    socket
                        .write_all(response.as_bytes())
                        .await
                        .expect("write api response");
                    continue;
                }

                panic!("unexpected request: {request}");
            }
        });

        let oauth = vsv2_auth::OAuthClient::new(
            format!("http://{address}/token"),
            "client-id",
            "client-secret",
        )
        .with_scope(Scope::Erp, "financials:read");
        let client = ErpRestClient::new(
            format!("http://{address}"),
            oauth,
            Limiter::new(Source::ErpRest, 1_000, 100_000),
        );
        let tenant = TenantId::new("33f319b9-4d57-11ee-960c-025417856183");

        let response = client
            .get_json_with_meta(&tenant, "/v1/customer", &[], &BackgroundOpts::default())
            .await
            .expect("retry should recover dropped transport connection");

        assert_eq!(response.status, 200);
        assert_eq!(response.body[0]["number"], "C0001");
        assert_eq!(api_attempts.load(Ordering::SeqCst), 2);

        server.await.expect("server completed");
    }

    /// A client wired to a one-shot local server: it answers the token request, then serves
    /// `body` (verbatim, with no `Content-Encoding` header) for the first API request.
    async fn client_serving(
        body: Vec<u8>,
    ) -> (ErpRestClient, tokio::task::JoinHandle<()>, TenantId) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("server addr");

        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.expect("accept connection");
                let mut buffer = [0_u8; 2048];
                let bytes_read = socket.read(&mut buffer).await.expect("read request");
                let request = String::from_utf8_lossy(&buffer[..bytes_read]);

                let payload = if request.starts_with("POST /token ") {
                    br#"{"access_token":"test-token","expires_in":3600}"#.to_vec()
                } else {
                    body.clone()
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    payload.len()
                );
                socket.write_all(head.as_bytes()).await.expect("write head");
                socket.write_all(&payload).await.expect("write body");
            }
        });

        let oauth = vsv2_auth::OAuthClient::new(
            format!("http://{address}/token"),
            "client-id",
            "client-secret",
        )
        .with_scope(Scope::Erp, "financials:read");
        let client = ErpRestClient::new(
            format!("http://{address}"),
            oauth,
            Limiter::new(Source::ErpRest, 1_000, 100_000),
        );
        (
            client,
            server,
            TenantId::new("33f319b9-4d57-11ee-960c-025417856183"),
        )
    }

    #[tokio::test]
    async fn get_json_inflates_a_gzip_body_that_does_not_declare_its_encoding() {
        use std::io::Write as _;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(br#"[{"internalId":1},{"internalId":2}]"#)
            .expect("compress");
        let (client, server, tenant) = client_serving(encoder.finish().expect("finish")).await;

        let response = client
            .get_json_with_meta(
                &tenant,
                "/v1/expenseReceipt",
                &[],
                &BackgroundOpts::default(),
            )
            .await
            .expect("a gzip body without Content-Encoding must still parse");

        assert_eq!(response.body.as_array().map(Vec::len), Some(2));
        assert_eq!(response.body[1]["internalId"], 2);
        server.await.expect("server completed");
    }

    #[tokio::test]
    async fn parse_failure_message_never_carries_a_nul_byte() {
        let (client, server, tenant) = client_serving(b"\0\0\x01binary, not json\0".to_vec()).await;

        let err = client
            .get_json_with_meta(
                &tenant,
                "/v1/expenseReceipt",
                &[],
                &BackgroundOpts::default(),
            )
            .await
            .expect_err("binary junk is not JSON");

        let message = err.to_string();
        assert!(message.contains("parse erp_rest body"), "{message}");
        assert!(
            !message.contains('\0'),
            "NUL would make Postgres reject the run row"
        );
        server.await.expect("server completed");
    }

    /// A client wired to a local server that answers the token request and then replies to API
    /// requests from `script` in order (the last entry repeats). Returns the client, the server
    /// task and a counter of API requests served.
    async fn client_scripted(
        script: Vec<(u16, &'static str)>,
    ) -> (
        ErpRestClient,
        tokio::task::JoinHandle<()>,
        Arc<AtomicUsize>,
        usize,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("server addr");
        let served = Arc::new(AtomicUsize::new(0));
        let served_task = served.clone();
        let expected_api_calls = script.len();

        let server = tokio::spawn(async move {
            // One token request plus every scripted API call.
            for _ in 0..=expected_api_calls {
                let (mut socket, _) = listener.accept().await.expect("accept connection");
                let mut buffer = [0_u8; 2048];
                let bytes_read = socket.read(&mut buffer).await.expect("read request");
                let request = String::from_utf8_lossy(&buffer[..bytes_read]);

                let (status, payload) = if request.starts_with("POST /token ") {
                    (200, r#"{"access_token":"test-token","expires_in":3600}"#)
                } else {
                    let n = served_task.fetch_add(1, Ordering::SeqCst);
                    script[n.min(script.len() - 1)]
                };
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write response");
            }
        });

        let oauth = vsv2_auth::OAuthClient::new(
            format!("http://{address}/token"),
            "client-id",
            "client-secret",
        )
        .with_scope(Scope::Erp, "financials:read");
        let mut client = ErpRestClient::new(
            format!("http://{address}"),
            oauth,
            Limiter::new(Source::ErpRest, 1_000, 100_000),
        );
        client.gateway_retry_delays = vec![Duration::from_millis(5); 2];
        (client, server, served, expected_api_calls)
    }

    #[tokio::test]
    async fn gateway_errors_are_retried_until_the_upstream_recovers() {
        let (client, server, served, _) = client_scripted(vec![
            (502, "<html>502 Bad Gateway</html>"),
            (504, "<html>504</html>"),
            (200, r#"[{"referenceNbr":"1"}]"#),
        ])
        .await;
        let tenant = TenantId::new("33f319b9-4d57-11ee-960c-025417856183");

        let response = client
            .get_json_with_meta(
                &tenant,
                "/v1/customerinvoice",
                &[],
                &BackgroundOpts::default(),
            )
            .await
            .expect("two gateway errors must be absorbed");

        assert_eq!(response.status, 200);
        assert_eq!(response.body[0]["referenceNbr"], "1");
        assert_eq!(served.load(Ordering::SeqCst), 3);
        server.await.expect("server completed");
    }

    #[tokio::test]
    async fn persistent_gateway_errors_surface_after_the_bounded_retries() {
        let (client, server, served, _) =
            client_scripted(vec![(502, "<html>502 Bad Gateway</html>"); 3]).await;
        let tenant = TenantId::new("33f319b9-4d57-11ee-960c-025417856183");

        let err = client
            .get_json_with_meta(
                &tenant,
                "/v1/customerinvoice",
                &[],
                &BackgroundOpts::default(),
            )
            .await
            .expect_err("a permanently failing gateway must not be retried forever");

        assert!(
            matches!(err, Error::UpstreamStatus { status: 502, .. }),
            "unexpected error: {err}"
        );
        assert_eq!(served.load(Ordering::SeqCst), 3, "1 try + 2 retries");
        server.await.expect("server completed");
    }

    #[tokio::test]
    async fn client_errors_are_not_retried() {
        let (client, server, served, _) =
            client_scripted(vec![(400, r#"{"message":"bad request"}"#)]).await;
        let tenant = TenantId::new("33f319b9-4d57-11ee-960c-025417856183");

        let err = client
            .get_json_with_meta(
                &tenant,
                "/v1/customerinvoice",
                &[],
                &BackgroundOpts::default(),
            )
            .await
            .expect_err("400 is the caller's problem");

        assert!(matches!(err, Error::UpstreamStatus { status: 400, .. }));
        assert_eq!(served.load(Ordering::SeqCst), 1);
        server.abort();
    }
}
