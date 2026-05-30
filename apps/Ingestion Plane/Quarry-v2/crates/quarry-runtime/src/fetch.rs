//! Static fetch driver. reqwest + rustls. Browser driver added in Phase 2.

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;
use url::Url;

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;

use crate::driver::{Driver, FetchHints};
use crate::proxy_pool::{ProxyEntry, ProxyPool};

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub status: u16,
    pub final_url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub duration_ms: u64,
}

pub struct StaticDriver {
    /// Direct-egress client. Used when no proxy pool is configured,
    /// when the pool is empty, or when the (org, host) hash misses
    /// every pool entry (it currently can't, but we keep this as the
    /// definitional fallback).
    client: reqwest::Client,
    /// Pool of proxies and pre-built clients. Empty when no
    /// `QUARRY_PROXY_POOL` is configured. Each entry is built once at
    /// driver construction; reqwest's connection pool inside the
    /// client handles per-request reuse.
    proxy_clients: HashMap<String, reqwest::Client>,
    pool: ProxyPool,
}

impl StaticDriver {
    pub fn new(timeout: Duration, user_agent: &str) -> QuarryResult<Self> {
        Self::with_proxy_pool(timeout, user_agent, ProxyPool::empty())
    }

    /// Build a static driver with a sticky proxy pool. Pass
    /// `ProxyPool::empty()` for direct-egress-only.
    pub fn with_proxy_pool(
        timeout: Duration,
        user_agent: &str,
        pool: ProxyPool,
    ) -> QuarryResult<Self> {
        let client = build_client(timeout, user_agent, None)?;
        let mut proxy_clients = HashMap::with_capacity(pool.len());
        for entry in pool.entries() {
            let proxied = build_client(timeout, user_agent, Some(entry))?;
            proxy_clients.insert(entry.uri.clone(), proxied);
        }
        if !pool.is_empty() {
            tracing::info!(
                proxy_count = pool.len(),
                "StaticDriver wired with proxy pool"
            );
        }
        Ok(Self {
            client,
            proxy_clients,
            pool,
        })
    }

    /// Pick the client that should service this (org, host) tuple.
    /// Falls back to the direct-egress client when:
    ///   - the pool is empty
    ///   - the pool entry's client was somehow evicted (cannot
    ///     currently happen)
    fn pick_client(&self, hints: &FetchHints, url: &Url) -> &reqwest::Client {
        let host = url.host_str().unwrap_or("");
        if let Some(entry) = self.pool.pick(&hints.org_id, host) {
            if let Some(c) = self.proxy_clients.get(&entry.uri) {
                return c;
            }
        }
        &self.client
    }

    /// Shared fetch path used by both `fetch` and `fetch_conditional`.
    /// When `hints` carries an `if_none_match` / `if_modified_since`
    /// validator we send the matching request header; the server is
    /// then free to respond `304 Not Modified` with an empty body and
    /// we propagate that status up to the caller (which can short-
    /// circuit by re-using the previously stored artifact).
    async fn do_fetch(&self, url: &Url, hints: &FetchHints) -> QuarryResult<FetchResponse> {
        let start = std::time::Instant::now();
        let client = self.pick_client(hints, url);
        let mut req = client.get(url.clone());
        if let Some(etag) = hints.if_none_match.as_deref() {
            // ETag values arrive with quotes already in place per
            // RFC 9110 — pass through verbatim.
            req = req.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        if hints.if_none_match.is_none() {
            // Only send If-Modified-Since when we DON'T have an ETag.
            // Servers must honor If-None-Match in preference per the
            // RFC; sending both is legal but redundant and some
            // misbehaving CDNs respond inconsistently to the pair.
            if let Some(lm) = hints.if_modified_since.as_deref() {
                req = req.header(reqwest::header::IF_MODIFIED_SINCE, lm);
            }
        }
        let resp = req.send().await.map_err(|e| {
            let code = if e.is_timeout() {
                ErrorCode::Timeout
            } else if e.is_connect() {
                ErrorCode::UpstreamBlocked
            } else {
                ErrorCode::DriverFailed
            };
            QuarryError::new(code, format!("static fetch: {e}"))
        })?;
        let status = resp.status().as_u16();
        let final_url = resp.url().clone();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        // 304 Not Modified bodies are empty by spec — don't try to
        // read them. Returning the empty body + 304 status lets the
        // caller detect cache-hit cheaply.
        let body = if status == 304 {
            Vec::new()
        } else {
            resp.bytes()
                .await
                .map_err(|e| QuarryError::new(ErrorCode::DriverFailed, format!("read body: {e}")))?
                .to_vec()
        };
        Ok(FetchResponse {
            status,
            final_url,
            headers,
            body,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

#[async_trait]
impl Driver for StaticDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.do_fetch(url, &FetchHints::default()).await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        self.do_fetch(url, hints).await
    }
}

/// Shared client builder used for both the direct-egress and proxy
/// variants. Keeps the cookie store + decompression knobs identical
/// across every client so swapping proxies mid-crawl doesn't change
/// downstream parsing behaviour.
fn build_client(
    timeout: Duration,
    user_agent: &str,
    proxy: Option<&ProxyEntry>,
) -> QuarryResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(user_agent)
        .redirect(reqwest::redirect::Policy::limited(5))
        .cookie_store(true);
    if let Some(p) = proxy {
        // `Proxy::all` routes every scheme (http + https) through the
        // proxy. Reqwest parses socks5/socks5h/http/https from the URI
        // scheme; on parse failure we surface a clear error so misC-
        // configured pools fail fast at startup.
        let parsed = reqwest::Proxy::all(&p.uri).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("invalid proxy URI {}: {e}", p.uri),
            )
        })?;
        builder = builder.proxy(parsed);
    }
    builder
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {e}")))
}
