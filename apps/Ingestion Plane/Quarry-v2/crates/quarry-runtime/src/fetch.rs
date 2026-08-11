//! Static fetch driver. reqwest + rustls. Browser driver added in Phase 2.

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;
use url::Url;

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use serde_json::json;

use crate::dns_guard::ResolvedTarget;
use crate::driver::{Driver, FetchHints};
use crate::egress_broker::{EgressBroker, EgressDecision, EgressIdentity};
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
    egress: EgressBroker,
    /// Retained (alongside `user_agent` below) so a pinned, single-purpose
    /// client can be built on demand for direct-egress requests carrying a
    /// `resolved_target` hint — see `client_for_decision`.
    timeout: Duration,
    user_agent: String,
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
        let processor_id = if pool.is_empty() {
            None
        } else {
            Some("quarry_proxy_pool".to_string())
        };
        Self::with_proxy_pool_and_processor(timeout, user_agent, pool, processor_id)
    }

    pub fn with_proxy_pool_and_processor(
        timeout: Duration,
        user_agent: &str,
        pool: ProxyPool,
        proxy_processor_id: Option<String>,
    ) -> QuarryResult<Self> {
        let client = build_client(timeout, user_agent, None, None)?;
        let mut proxy_clients = HashMap::with_capacity(pool.len());
        for entry in pool.entries() {
            let proxied = build_client(timeout, user_agent, Some(entry), None)?;
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
            egress: EgressBroker::new(pool, proxy_processor_id),
            timeout,
            user_agent: user_agent.to_string(),
        })
    }

    /// Resolve which client should carry this request. Direct egress with a
    /// `resolved_target` hint gets a fresh, single-purpose client pinned to
    /// those exact addresses via `resolve_to_addrs` — reqwest has no API to
    /// add a resolve override to an already-built `Client`, so the pooled
    /// `self.client` can't be reused for this case. Proxied egress is never
    /// pinned: with a forwarding proxy, the target host is resolved by the
    /// proxy server, not locally, so a local resolve override would have no
    /// effect on where the connection actually lands.
    fn client_for_decision(
        &self,
        decision: &EgressDecision,
        resolved_target: Option<&ResolvedTarget>,
    ) -> QuarryResult<reqwest::Client> {
        match (&decision.identity, resolved_target) {
            (EgressIdentity::Direct, Some(target)) => {
                build_client(self.timeout, &self.user_agent, None, Some(target))
            }
            (EgressIdentity::Direct, None) => Ok(self.client.clone()),
            (EgressIdentity::Proxy { uri, .. }, _) => {
                self.proxy_clients.get(uri).cloned().ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::Internal,
                        format!("egress proxy client missing for configured proxy: {uri}"),
                    )
                })
            }
        }
    }

    /// Shared fetch path used by both `fetch` and `fetch_conditional`.
    /// When `hints` carries an `if_none_match` / `if_modified_since`
    /// validator we send the matching request header; the server is
    /// then free to respond `304 Not Modified` with an empty body and
    /// we propagate that status up to the caller (which can short-
    /// circuit by re-using the previously stored artifact).
    async fn do_fetch(&self, url: &Url, hints: &FetchHints) -> QuarryResult<FetchResponse> {
        let host = url.host_str().unwrap_or("").to_string();
        let plan = self.egress.plan(hints, url)?;
        let mut attempts = Vec::with_capacity(plan.len());
        for (idx, decision) in plan.iter().enumerate() {
            let client = self.client_for_decision(decision, hints.resolved_target.as_ref())?;
            match self.send_once(&client, url, hints).await {
                Ok(resp) => {
                    self.egress
                        .mark_http_status(&host, &decision.identity, resp.status);
                    if crate::fingerprint_rotation::is_block_status(resp.status)
                        && matches!(decision.identity, EgressIdentity::Proxy { .. })
                    {
                        attempts.push(attempt_record(
                            decision,
                            Some(resp.status),
                            None,
                            "block_status",
                        ));
                        if idx + 1 < plan.len() {
                            tracing::warn!(
                                host = %host,
                                status = resp.status,
                                attempt = idx,
                                egress = decision.identity.label(),
                                "egress blocked; rotating to next approved proxy"
                            );
                            continue;
                        }
                        return Err(blocked_error(resp.status, attempts));
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    self.egress.mark_transport_error(&host, &decision.identity);
                    attempts.push(attempt_record(
                        decision,
                        None,
                        Some(&err),
                        "transport_error",
                    ));
                    if is_retryable_egress_error(err.code) && idx + 1 < plan.len() {
                        tracing::warn!(
                            host = %host,
                            error = %err,
                            attempt = idx,
                            egress = decision.identity.label(),
                            "egress transport failed; rotating to next approved proxy"
                        );
                        continue;
                    }
                    return Err(err.with_details(json!({ "egress_attempts": attempts })));
                }
            }
        }

        Err(QuarryError::new(
            ErrorCode::UpstreamBlocked,
            "no egress identities available",
        ))
    }

    async fn send_once(
        &self,
        client: &reqwest::Client,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        let start = std::time::Instant::now();
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

fn is_retryable_egress_error(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::Timeout
            | ErrorCode::UpstreamBlocked
            | ErrorCode::DriverFailed
            | ErrorCode::RateLimited
    )
}

fn attempt_record(
    decision: &EgressDecision,
    status: Option<u16>,
    error: Option<&QuarryError>,
    reason: &str,
) -> serde_json::Value {
    let (egress, proxy_uri, processor_id) = match &decision.identity {
        EgressIdentity::Direct => ("direct", None, None),
        EgressIdentity::Proxy { uri, processor_id } => {
            ("proxy", Some(uri.as_str()), processor_id.as_deref())
        }
    };
    json!({
        "attempt": decision.attempt,
        "egress": egress,
        "proxy_uri": proxy_uri,
        "processor_id": processor_id,
        "reason": reason,
        "decision_reason": decision.reason,
        "status": status,
        "error_code": error.map(|e| format!("{:?}", e.code)),
        "error": error.map(|e| e.message.clone()),
    })
}

fn blocked_error(status: u16, attempts: Vec<serde_json::Value>) -> QuarryError {
    let code = if status == 429 {
        ErrorCode::RateLimited
    } else {
        ErrorCode::UpstreamBlocked
    };
    QuarryError::new(
        code,
        format!("all approved egress identities blocked; last status={status}"),
    )
    .with_details(json!({ "egress_attempts": attempts }))
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
    resolve_override: Option<&ResolvedTarget>,
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
    if let Some(target) = resolve_override {
        // Pin this client's connection for `target.host` to the exact
        // addresses the SSRF guard already validated, instead of letting
        // reqwest perform its own independent DNS lookup at connect time.
        builder = builder.resolve_to_addrs(&target.host, &target.addrs);
    }
    builder
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::error::ErrorCode;
    use quarry_core::privacy::PrivacyPolicy;
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn approved_hints_for_org(org_id: String) -> FetchHints {
        FetchHints {
            org_id,
            privacy: PrivacyPolicy {
                allow_third_party_processing: true,
                processor_id: Some("quarry_proxy_pool".into()),
                ..PrivacyPolicy::default()
            },
            ..FetchHints::default()
        }
    }

    #[tokio::test]
    async fn proxy_pool_denies_default_private_policy_before_network() {
        let pool = ProxyPool::from_env_string("http://127.0.0.1:9");
        let driver =
            StaticDriver::with_proxy_pool(Duration::from_millis(50), "QuarryTest/1.0", pool)
                .unwrap();
        let url: Url = "https://example.com/".parse().unwrap();

        let err = driver
            .fetch_conditional(&url, &FetchHints::default())
            .await
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    #[tokio::test]
    async fn proxy_pool_rotates_to_next_proxy_on_429() {
        let blocked_proxy = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(429).set_body_string("blocked"))
            .mount(&blocked_proxy)
            .await;
        let ok_proxy = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&ok_proxy)
            .await;

        let blocked_uri = format!("http://{}", blocked_proxy.address());
        let ok_uri = format!("http://{}", ok_proxy.address());
        let pool = ProxyPool::from_env_string(&format!("{blocked_uri};{ok_uri}"));
        let host = "example.com";
        let org_id = (0..100)
            .map(|i| format!("org_{i}"))
            .find(|org| pool.pick(org, host).is_some_and(|p| p.uri == blocked_uri))
            .expect("test should find an org that maps to the blocked proxy");

        let driver = StaticDriver::with_proxy_pool_and_processor(
            Duration::from_secs(5),
            "QuarryTest/1.0",
            pool,
            Some("quarry_proxy_pool".into()),
        )
        .unwrap();
        let url: Url = format!("http://{host}/").parse().unwrap();
        let hints = approved_hints_for_org(org_id);

        let resp = driver.fetch_conditional(&url, &hints).await.unwrap();

        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"ok");
    }

    #[tokio::test]
    async fn proxy_pool_returns_rate_limited_when_all_approved_proxies_block() {
        let p1 = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(429).set_body_string("blocked"))
            .mount(&p1)
            .await;
        let p2 = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(429).set_body_string("blocked"))
            .mount(&p2)
            .await;

        let pool =
            ProxyPool::from_env_string(&format!("http://{};http://{}", p1.address(), p2.address()));
        let driver = StaticDriver::with_proxy_pool_and_processor(
            Duration::from_secs(5),
            "QuarryTest/1.0",
            pool,
            Some("quarry_proxy_pool".into()),
        )
        .unwrap();
        let url: Url = "http://example.com/".parse().unwrap();
        let hints = approved_hints_for_org("org_a".into());

        let err = driver.fetch_conditional(&url, &hints).await.unwrap_err();

        assert_eq!(err.code, ErrorCode::RateLimited);
        assert!(err
            .details
            .as_ref()
            .and_then(|v| v.get("egress_attempts"))
            .is_some());
    }

    #[tokio::test]
    async fn resolved_target_pins_direct_egress_to_the_exact_resolved_address() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string("pinned"))
            .mount(&server)
            .await;

        // ".invalid" is reserved by RFC 2606 to never resolve via real DNS —
        // if this fetch reaches the mock server anyway, the only possible
        // explanation is that `resolved_target` pinned the connection to it,
        // since ordinary DNS resolution of this host cannot succeed.
        let host = "quarry-dns-pin-test.invalid";
        let url: Url = format!("http://{host}:{}/", server.address().port())
            .parse()
            .unwrap();

        let driver = StaticDriver::new(Duration::from_secs(5), "QuarryTest/1.0").unwrap();
        let hints = FetchHints {
            resolved_target: Some(ResolvedTarget {
                host: host.to_string(),
                addrs: vec![*server.address()],
            }),
            ..FetchHints::default()
        };

        let resp = driver.fetch_conditional(&url, &hints).await.unwrap();

        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"pinned");
    }

    #[tokio::test]
    async fn no_resolved_target_falls_back_to_pooled_client_unpinned() {
        // Without a resolved_target hint, an unresolvable ".invalid" host
        // must fail with a real DNS/connect error — proving the previous
        // test's success came from pinning, not from some other bypass.
        let host = "quarry-dns-pin-test-unpinned.invalid";
        let url: Url = format!("http://{host}/").parse().unwrap();

        let driver = StaticDriver::new(Duration::from_secs(5), "QuarryTest/1.0").unwrap();
        let err = driver
            .fetch_conditional(&url, &FetchHints::default())
            .await
            .unwrap_err();

        assert_ne!(err.code, ErrorCode::SecurityBlocked);
    }
}
