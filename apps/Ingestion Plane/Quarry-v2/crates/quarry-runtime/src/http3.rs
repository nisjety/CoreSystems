//! HTTP/3 (QUIC) driver. Wave 7.
//!
//! Built on the workspace Reqwest client. The `http3` Cargo feature on
//! `quarry-runtime` enables this
//! module; the matching `.cargo/config.toml` sets
//! `--cfg=reqwest_unstable` so reqwest's http3 surface is visible.
//!
//! **TLS fingerprint caveat.** This driver uses rustls under quinn,
//! which means we get a standard rustls ClientHello on the h3 path —
//! *not* the browser-emulating JA3/JA4 fingerprint that `wreq` gives
//! us on the HTTP/2 path. No public Rust HTTP/3 client currently
//! supports BoringSSL-based fingerprint emulation (wreq's TLS layer
//! doesn't speak QUIC, and the rustls-on-quinn stack is what
//! `h3-quinn` is built on). The driver is therefore most useful for
//! tail latency on CDN-fronted origins (Cloudflare, Fastly, Google,
//! Cloudfront) where TLS fingerprint matters less than the
//! 1-RTT-instead-of-3-RTT handshake.
//!
//! Wire flow on `prefer_http3`:
//!   1. Edge / orchestrator passes `prefer_http3=true` on the request.
//!   2. Edge selects the startup-wired `Http3Driver`.
//!   3. The first request to a new origin opens a QUIC connection
//!      (cached by reqwest). Subsequent requests reuse 0-RTT.
//!   4. If QUIC fails (UDP blocked, server refuses), `do_fetch`
//!      surfaces a retryable driver error. Edge wraps this driver in
//!      [`crate::transport_fallback_driver::TransportFallbackDriver`]
//!      for `prefer_http3`, so requests retry against the planned
//!      Static/TLS fallback instead of returning a local Docker-only 502.
//!
//! Conditional GET (`If-None-Match`, `If-Modified-Since`) is honoured
//! identically to [`crate::fetch::StaticDriver`]. Render hints are
//! ignored — h3 has no rendering primitive.

use async_trait::async_trait;
use std::time::Duration;
use url::Url;

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;

use crate::driver::{Driver, FetchHints};
use crate::fetch::FetchResponse;

/// HTTP/3 driver backed by reqwest 0.13 + h3 + h3-quinn + quinn.
#[derive(Debug)]
pub struct Http3Driver {
    client: reqwest::Client,
}

impl Http3Driver {
    /// Build an HTTP/3 client. Falls back to HTTP/2 on hosts that
    /// don't advertise QUIC via Alt-Svc — that's reqwest's own
    /// negotiation, not ours.
    pub fn new(timeout: Duration, user_agent: &str) -> QuarryResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(user_agent)
            .redirect(reqwest::redirect::Policy::limited(5))
            .cookie_store(true)
            // Prefer h3 when the server advertises it; reqwest still
            // races h2 to hide the cold-start cost on first request
            // to a new origin.
            .http3_prior_knowledge()
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http3 client: {e:?}")))?;
        Ok(Self { client })
    }

    /// Shared fetch path. Identical conditional-GET semantics to
    /// `StaticDriver::do_fetch` — caller passes `FetchHints` with
    /// optional `If-None-Match` / `If-Modified-Since`; we propagate
    /// and treat `304 Not Modified` as an empty body.
    async fn do_fetch(&self, url: &Url, hints: &FetchHints) -> QuarryResult<FetchResponse> {
        let start = std::time::Instant::now();
        // Force HTTP/3 for this request. If the upstream doesn't speak
        // it we want a hard error, not a silent fallback, because the
        // caller asked for QUIC and may have measurement / privacy
        // assumptions that hinge on it.
        let mut req = self
            .client
            .get(url.clone())
            .version(reqwest::Version::HTTP_3);
        if let Some(etag) = hints.if_none_match.as_deref() {
            req = req.header(reqwest::header::IF_NONE_MATCH, etag);
        } else if let Some(lm) = hints.if_modified_since.as_deref() {
            req = req.header(reqwest::header::IF_MODIFIED_SINCE, lm);
        }
        let resp = req.send().await.map_err(|e| {
            let code = if e.is_timeout() {
                ErrorCode::Timeout
            } else if e.is_connect() {
                ErrorCode::UpstreamBlocked
            } else {
                ErrorCode::DriverFailed
            };
            QuarryError::new(code, format!("http3 fetch: {e}"))
        })?;
        let status = resp.status().as_u16();
        let final_url = resp.url().clone();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
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
impl Driver for Http3Driver {
    fn kind(&self) -> DriverKind {
        // No dedicated `Http3` variant on DriverKind yet — surface as
        // Static so existing dashboards / billing don't break. A
        // future variant is fine but adds a workspace-wide enum change.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Constructor smoke test. Requires a live tokio runtime because
    /// reqwest 0.13's http3 builder peeks at the current runtime to
    /// initialise the quinn endpoint — synchronous `#[test]` panics
    /// with "no async runtime found". We don't have a public QUIC
    /// test endpoint in CI so live-traffic tests are skipped — those
    /// live in `tests/integration/http3_live.rs` and are run on
    /// demand.
    #[tokio::test]
    async fn build_succeeds_with_sane_defaults() {
        let driver = Http3Driver::new(Duration::from_secs(10), "quarry-test/0");
        assert!(driver.is_ok(), "Http3Driver::new should build: {driver:?}");
    }

    #[tokio::test]
    async fn driver_kind_is_static() {
        let driver = Http3Driver::new(Duration::from_secs(10), "quarry-test/0").unwrap();
        assert_eq!(driver.kind(), DriverKind::Static);
    }
}
