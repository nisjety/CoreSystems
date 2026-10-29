use std::time::{Duration, Instant};

use tracing::{debug, warn};

use vsv2_auth::{OAuthClient, Scope};
use vsv2_common::{Error, RateLimitSnapshot, Result, Source, TenantId};
use vsv2_ratelimit::Limiter;

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct ExpenseClient {
    pub base_url: String,
    pub oauth: OAuthClient,
    pub limiter: Limiter,
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct JsonResponse {
    pub body: serde_json::Value,
    pub status: u16,
    pub elapsed_ms: i64,
    pub rate_limit: RateLimitSnapshot,
}

impl ExpenseClient {
    pub fn new(base_url: impl Into<String>, oauth: OAuthClient, limiter: Limiter) -> Self {
        let mut url: String = base_url.into();
        if !url.ends_with('/') {
            url.push('/');
        }
        Self {
            base_url: url,
            oauth,
            limiter,
            http: reqwest::Client::builder()
                .timeout(DEFAULT_HTTP_TIMEOUT)
                .build()
                .expect("reqwest client builds"),
        }
    }

    pub async fn get_json(
        &self,
        tenant: &TenantId,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<serde_json::Value> {
        Ok(self.get_json_with_meta(tenant, path, query).await?.body)
    }

    pub async fn get_json_with_meta(
        &self,
        tenant: &TenantId,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<JsonResponse> {
        self.limiter.acquire().await?;
        let started = Instant::now();
        let token = self.oauth.token_for(tenant, Scope::Expense).await?;

        let url = format!("{}{}", self.base_url, path.trim_start_matches('/'));
        debug!(tenant = %tenant, %url, "expense: GET");

        let mut req = self.http.get(&url).bearer_auth(&token);
        if !query.is_empty() {
            req = req.query(query);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| Error::Http(format!("expense GET {url}: {e}")))?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let rate_limit = rate_limit_snapshot(&headers);
        observe_rate_limit_headers(&self.limiter, &headers).await?;

        if status.as_u16() == 429 {
            let retry = headers
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(60);
            let until = chrono::Utc::now() + chrono::Duration::seconds(retry);
            self.limiter.engage_cooldown_until(until).await?;
            warn!(retry_after = retry, %url, "expense 429");
            return Err(Error::RateLimited {
                kind: Source::ExpenseApi.as_str(),
                until,
            });
        }

        let text = resp
            .text()
            .await
            .map_err(|e| Error::Http(format!("read body: {e}")))?;

        if !status.is_success() {
            warn!(%status, %url, "expense: non-success");
            return Err(Error::UpstreamStatus {
                status: status.as_u16(),
                body: text,
            });
        }

        let body = serde_json::from_str(&text).map_err(|e| {
            Error::Http(format!(
                "parse expense body: {e}; head: {}",
                &text.chars().take(256).collect::<String>(),
            ))
        })?;

        Ok(JsonResponse {
            body,
            status: status.as_u16(),
            elapsed_ms: elapsed_ms(started),
            rate_limit,
        })
    }
}

async fn observe_rate_limit_headers(
    limiter: &Limiter,
    headers: &reqwest::header::HeaderMap,
) -> Result<()> {
    let limit = header_u32(headers, "x-ratelimit-limit");
    let remaining = header_u32(headers, "x-ratelimit-remaining");
    let reset = header_i64(headers, "x-ratelimit-reset");
    let policy = header_str(headers, "x-ratelimit-policy");
    limiter
        .observe_rate_limit(limit, remaining, reset, policy.as_deref())
        .await
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
