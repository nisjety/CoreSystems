use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use vsv2_auth::{OAuthClient, Scope};
use vsv2_common::{Error, RateLimitSnapshot, Result, TenantId};
use vsv2_ratelimit::Limiter;

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct DatamartClient {
    pub graphql_url: String,
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

#[derive(Debug, Serialize)]
struct GraphQlRequest<'a> {
    query: &'a str,
    variables: &'a serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse {
    data: Option<serde_json::Value>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
    #[allow(dead_code)]
    path: Option<Vec<serde_json::Value>>,
}

impl DatamartClient {
    pub fn new(graphql_url: impl Into<String>, oauth: OAuthClient, limiter: Limiter) -> Self {
        Self {
            graphql_url: graphql_url.into(),
            oauth,
            limiter,
            http: reqwest::Client::builder()
                .timeout(DEFAULT_HTTP_TIMEOUT)
                .gzip(true)
                .build()
                .expect("reqwest client builds"),
        }
    }

    /// Execute a single GraphQL query. Honours rate limits, retries 429
    /// once with `Retry-After`, returns `data` payload on success.
    pub async fn execute(
        &self,
        tenant: &TenantId,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value> {
        Ok(self.execute_with_meta(tenant, query, variables).await?.body)
    }

    pub async fn execute_with_meta(
        &self,
        tenant: &TenantId,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<JsonResponse> {
        self.limiter.acquire().await?;
        let started = Instant::now();

        let token = self.oauth.token_for(tenant, Scope::Datamart).await?;
        let body = GraphQlRequest {
            query,
            variables: &variables,
        };

        debug!(tenant = %tenant, "datamart: POST {}", self.graphql_url);
        let resp = self
            .http
            .post(&self.graphql_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Http(format!("datamart POST: {e}")))?;

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
            warn!(retry_after = retry, "datamart 429");
            return Err(Error::RateLimited {
                kind: vsv2_common::Source::DatamartGraphql.as_str(),
                until,
            });
        }

        let text = resp
            .text()
            .await
            .map_err(|e| Error::Http(format!("read datamart body: {e}")))?;

        if !status.is_success() {
            return Err(Error::UpstreamStatus {
                status: status.as_u16(),
                body: text,
            });
        }

        let parsed: GraphQlResponse = serde_json::from_str(&text)
            .map_err(|e| Error::GraphQl(format!("parse: {e}; body: {text}")))?;

        if let Some(errors) = parsed.errors {
            let joined = errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(Error::GraphQl(joined));
        }

        let body = parsed
            .data
            .ok_or_else(|| Error::GraphQl("missing `data` field".into()))?;

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
