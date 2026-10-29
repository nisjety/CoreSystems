//! Static fetch driver. reqwest + rustls. Browser driver added in Phase 2.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use serde_json::json;

use crate::dns_guard::{resolve_public_url, PinnedDnsResolver};
use crate::driver::{Driver, FetchHints};
use crate::egress_broker::{EgressBroker, EgressDecision, EgressIdentity};
use crate::proxy_pool::{ProxyEntry, ProxyPool};

/// Stable public discovery identity; robots parsing must use this same token.
pub const DISCOVERY_USER_AGENT: &str = "QuarryBot";

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub status: u16,
    pub final_url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub duration_ms: u64,
}

pub struct StaticDriver {
    /// The only direct-egress client. Its resolver has no DNS fallback --
    /// every direct request is pinned to a preflighted address, either one
    /// PageRunner already vetted (`hints.resolved_target`) or one this
    /// driver preflights itself in `client_for_decision`. There is
    /// deliberately no unpinned direct-egress client: that used to exist as
    /// a fallback for callers that didn't supply `hints.resolved_target`,
    /// which meant `Driver::fetch`/`fetch_conditional` calls without a
    /// pre-populated hint got no SSRF/DNS-rebinding protection at the
    /// connection level at all (EgressBroker::plan only selects a proxy; it
    /// never validates the URL).
    pinned_client: reqwest::Client,
    /// Public discovery never inherits the general fetch cookie jar or a
    /// configured/environment proxy. It uses the same pinned DNS boundary.
    bounded_client: reqwest::Client,
    pinned_resolver: Arc<PinnedDnsResolver>,
    /// Pool of proxies and pre-built clients. Empty when no
    /// `QUARRY_PROXY_POOL` is configured. Each entry is built once at
    /// driver construction; reqwest's connection pool inside the
    /// client handles per-request reuse.
    proxy_clients: HashMap<String, reqwest::Client>,
    /// Pinned resolvers for the subset of `proxy_clients` whose scheme
    /// resolves the target hostname on the client side (plain `socks4://` /
    /// `socks5://`; see `resolves_dns_locally`). No entry exists for
    /// `socks4a://` / `socks5h://` / `http://` / `https://` proxies -- those
    /// hand the raw hostname to the proxy for server-side resolution, so a
    /// client-side resolver would never be consulted. Closing the DNS-guard
    /// gap for those schemes is a proxy-trust-boundary question, not a
    /// client-side fix; see the "Provider gates" section of `docs/POLICY.md`.
    proxy_pin_resolvers: HashMap<String, Arc<PinnedDnsResolver>>,
    egress: EgressBroker,
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
        let pinned_resolver = Arc::new(PinnedDnsResolver::default());
        let pinned_client = build_client(
            timeout,
            user_agent,
            None,
            Some(Arc::clone(&pinned_resolver)),
        )?;
        let bounded_client = bounded_client_builder(timeout, DISCOVERY_USER_AGENT)
            .dns_resolver(Arc::clone(&pinned_resolver))
            .build()
            .map_err(|_| {
                QuarryError::new(ErrorCode::Internal, "bounded client construction failed")
            })?;
        let mut proxy_clients = HashMap::with_capacity(pool.len());
        let mut proxy_pin_resolvers = HashMap::new();
        for entry in pool.entries() {
            let resolver = if resolves_dns_locally(&entry.uri) {
                let resolver = Arc::new(PinnedDnsResolver::default());
                proxy_pin_resolvers.insert(entry.uri.clone(), Arc::clone(&resolver));
                Some(resolver)
            } else {
                None
            };
            let proxied = build_client(timeout, user_agent, Some(entry), resolver)?;
            proxy_clients.insert(entry.uri.clone(), proxied);
        }
        if !pool.is_empty() {
            tracing::info!(
                proxy_count = pool.len(),
                pinned_proxy_count = proxy_pin_resolvers.len(),
                "StaticDriver wired with proxy pool"
            );
        }
        Ok(Self {
            pinned_client,
            bounded_client,
            pinned_resolver,
            proxy_clients,
            proxy_pin_resolvers,
            egress: EgressBroker::new(pool, proxy_processor_id),
        })
    }

    /// Selects the client for this egress decision. Direct egress always
    /// resolves to the pinned client: when `hints.resolved_target` is
    /// missing (the caller didn't preflight), this preflights `url` itself
    /// via `resolve_public_url` before pinning, so there is no code path
    /// where a direct fetch reaches the network unpinned and unvalidated.
    ///
    /// Proxy egress pins the same way, but only for the subset of proxy
    /// schemes that resolve the target hostname client-side (see
    /// `resolves_dns_locally`); `proxy_pin_resolvers` has no entry for the
    /// rest, so nothing is pinned for them and the proxy server resolves the
    /// hostname itself, same as before this method existed.
    async fn client_for_decision(
        &self,
        decision: &EgressDecision,
        hints: &FetchHints,
        url: &Url,
    ) -> QuarryResult<&reqwest::Client> {
        match &decision.identity {
            EgressIdentity::Direct => {
                let target = match hints.resolved_target.as_ref() {
                    Some(target) => target.clone(),
                    None => resolve_public_url(url).await?,
                };
                self.pinned_resolver.pin(target)?;
                Ok(&self.pinned_client)
            }
            EgressIdentity::Proxy { uri, .. } => {
                if let Some(resolver) = self.proxy_pin_resolvers.get(uri) {
                    let target = match hints.resolved_target.as_ref() {
                        Some(target) => target.clone(),
                        None => resolve_public_url(url).await?,
                    };
                    resolver.pin(target)?;
                }
                self.proxy_clients.get(uri).ok_or_else(|| {
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
        self.do_fetch_with_limit(url, hints, None).await
    }

    async fn do_fetch_with_limit(
        &self,
        url: &Url,
        hints: &FetchHints,
        max_bytes: Option<usize>,
    ) -> QuarryResult<FetchResponse> {
        let host = url.host_str().unwrap_or("").to_string();
        let plan = self.egress.plan(hints, url)?;
        let mut attempts = Vec::with_capacity(plan.len());
        for (idx, decision) in plan.iter().enumerate() {
            let client = self.client_for_decision(decision, hints, url).await?;
            match self.send_once(client, url, hints, max_bytes).await {
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
        max_bytes: Option<usize>,
    ) -> QuarryResult<FetchResponse> {
        self.send_single_hop(client, url, hints, max_bytes, false)
            .await
    }

    async fn send_single_hop(
        &self,
        client: &reqwest::Client,
        url: &Url,
        hints: &FetchHints,
        max_bytes: Option<usize>,
        expose_redirect: bool,
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
        // Redirect targets have not passed Quarry's URL preflight or DNS guard.
        // Do not hand them to reqwest's implicit redirect engine: the target
        // could resolve to a private or metadata address after an otherwise
        // safe public URL was accepted. A caller can surface the original
        // redirect and submit a separately reviewed target later.
        if resp.status().is_redirection() && !expose_redirect {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "static fetch redirect blocked pending target validation",
            ));
        }
        let final_url = resp.url().clone();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        // 304 Not Modified bodies are empty by spec — don't try to
        // read them. Returning the empty body + 304 status lets the
        // caller detect cache-hit cheaply.
        let body = if (300..400).contains(&status) {
            Vec::new()
        } else if let Some(limit) = max_bytes {
            read_bounded_body(resp, limit).await?
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

/// Count decoded chunks, including responses without Content-Length and
/// compressed responses whose expanded body is larger than the wire length.
async fn read_bounded_body(mut response: reqwest::Response, limit: usize) -> QuarryResult<Vec<u8>> {
    let too_large = || {
        QuarryError::new(
            ErrorCode::BadRequest,
            "upstream body exceeds bounded fetch limit",
        )
    };
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| QuarryError::new(ErrorCode::DriverFailed, "bounded body read failed"))?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
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

    async fn fetch_bounded(&self, url: &Url, max_bytes: usize) -> QuarryResult<FetchResponse> {
        let target = resolve_public_url(url).await?;
        self.pinned_resolver.pin(target)?;
        self.send_once(
            &self.bounded_client,
            url,
            &FetchHints::default(),
            Some(max_bytes),
        )
        .await
    }

    async fn fetch_discovery_hop(
        &self,
        url: &Url,
        max_bytes: usize,
    ) -> QuarryResult<FetchResponse> {
        let target = resolve_public_url(url).await?;
        self.pinned_resolver.pin(target)?;
        self.send_single_hop(
            &self.bounded_client,
            url,
            &FetchHints::default(),
            Some(max_bytes),
            true,
        )
        .await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        self.do_fetch(url, hints).await
    }
}

fn bounded_client_builder(timeout: Duration, user_agent: &str) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(user_agent)
        .no_proxy()
        .cookie_store(false)
        .redirect(reqwest::redirect::Policy::none())
}

/// True for proxy URI schemes where reqwest resolves the destination
/// hostname itself, client-side, before it ever contacts the proxy: plain
/// `socks4://` / `socks5://`. False for `socks4a://` / `socks5h://` (the
/// raw hostname is sent to the proxy inside the SOCKS request, which
/// resolves it) and for `http://` / `https://` (an HTTP CONNECT tunnel
/// carries the hostname verbatim in the `CONNECT host:port` request line;
/// the proxy resolves it, reqwest's resolver is never consulted). A URI
/// that fails to parse returns false -- `reqwest::Proxy::all` in
/// `build_client` rejects it with a clearer error at construction time.
/// Confirmed against reqwest 0.13's `connect.rs` (`connect_socks` picks
/// `DnsResolve::Local` only for `socks4`/`socks5`) rather than assumed.
fn resolves_dns_locally(uri: &str) -> bool {
    Url::parse(uri)
        .map(|u| matches!(u.scheme(), "socks4" | "socks5"))
        .unwrap_or(false)
}

/// Shared client builder used for both the direct-egress and proxy
/// variants. Keeps the cookie store + decompression knobs identical
/// across every client so swapping proxies mid-crawl doesn't change
/// downstream parsing behaviour.
fn build_client(
    timeout: Duration,
    user_agent: &str,
    proxy: Option<&ProxyEntry>,
    pinned_resolver: Option<Arc<PinnedDnsResolver>>,
) -> QuarryResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(user_agent)
        // Any redirect target is a fresh SSRF boundary. The pipeline only
        // preflights the requested URL, so implicit following would bypass
        // target validation and DNS policy.
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true);
    if let Some(resolver) = pinned_resolver {
        builder = builder.dns_resolver(resolver);
    }
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
    async fn bounded_static_body_accepts_limit_and_rejects_oversize() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string("123456789"))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let response = client.get(server.uri()).send().await.unwrap();
        assert_eq!(read_bounded_body(response, 9).await.unwrap(), b"123456789");
        let response = client.get(server.uri()).send().await.unwrap();
        assert_eq!(
            read_bounded_body(response, 8).await.unwrap_err().code,
            ErrorCode::BadRequest
        );
    }

    #[tokio::test]
    async fn bounded_static_body_rejects_chunked_overflow_without_content_length() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 2048];
            socket.read(&mut buf).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\n1234\r\n5\r\n56789\r\n0\r\n\r\n").await.unwrap();
        });
        let response = reqwest::Client::new()
            .get(format!("http://{addr}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            read_bounded_body(response, 8).await.unwrap_err().code,
            ErrorCode::BadRequest
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn bounded_static_fetch_still_rejects_private_destinations() {
        let driver = StaticDriver::new(Duration::from_secs(1), "QuarryTest").unwrap();
        let error = driver
            .fetch_bounded(&Url::parse("http://127.0.0.1:9/").unwrap(), 1024)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::SecurityBlocked);
        let error = driver
            .fetch_discovery_hop(&Url::parse("http://127.0.0.1:9/").unwrap(), 1024)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn discovery_hop_returns_redirect_without_following_or_reading_body() {
        let target = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .mount(&target)
            .await;
        let source = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(308)
                    .insert_header("location", target.uri())
                    .insert_header("set-cookie", "private=value")
                    .set_body_string("body larger than the one-byte bound"),
            )
            .mount(&source)
            .await;
        let driver = StaticDriver::new(Duration::from_secs(1), "QuarryTest").unwrap();
        let client = bounded_client_builder(Duration::from_secs(1), DISCOVERY_USER_AGENT)
            .build()
            .unwrap();
        let response = driver
            .send_single_hop(
                &client,
                &Url::parse(&source.uri()).unwrap(),
                &FetchHints::default(),
                Some(1),
                true,
            )
            .await
            .unwrap();
        assert_eq!(response.status, 308);
        assert!(response.body.is_empty());
        assert_eq!(response.final_url.as_str(), format!("{}/", source.uri()));
        assert!(target.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn bounded_discovery_client_never_retains_or_sends_cookies() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("set-cookie", "session=private; Path=/")
                    .set_body_string("public"),
            )
            .mount(&server)
            .await;
        // Test only the shared builder's cookie policy; production additionally
        // installs the fail-closed public-address resolver.
        let client = bounded_client_builder(Duration::from_secs(1), DISCOVERY_USER_AGENT)
            .build()
            .unwrap();
        for _ in 0..2 {
            client.get(server.uri()).send().await.unwrap();
        }
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests
            .iter()
            .all(|request| !request.headers.contains_key("cookie")));
        assert!(requests.iter().all(|request| request
            .headers
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            == Some(DISCOVERY_USER_AGENT)));
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
    async fn static_driver_rejects_redirects_before_contacting_the_target() {
        let target = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string("target reached"))
            .mount(&target)
            .await;

        let redirector = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/metadata", target.uri())),
            )
            .mount(&redirector)
            .await;

        let driver = StaticDriver::new(Duration::from_secs(2), "QuarryTest/1.0").unwrap();
        let url: Url = format!("{}/redirect", redirector.uri()).parse().unwrap();

        let err = driver.fetch(&url).await.unwrap_err();

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
        assert!(target.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn without_a_resolved_target_no_pin_is_applied() {
        // Negative control for the pinning tests above. They assert the pinned
        // client is SELECTED and that the resolver hands back only preflighted
        // addresses; neither would notice if the pinned path were somehow taken
        // for a request that never preflighted, which is the case that would
        // let an unvetted host inherit another's pin.
        //
        // "…​.invalid" is reserved by RFC 2606 to never resolve, so the failure
        // must come from resolution or connect — never from the security guard,
        // because with no hint there is nothing for it to have vetted. Costs no
        // network: a resolver answers .invalid NXDOMAIN locally.
        let driver = StaticDriver::new(Duration::from_secs(2), "QuarryTest/1.0").unwrap();
        let url: Url = "http://quarry-pin-negative-control.invalid/"
            .parse()
            .unwrap();

        let error = driver
            .fetch_conditional(&url, &FetchHints::default())
            .await
            .expect_err("an unresolvable host cannot succeed");

        assert_ne!(
            error.code,
            ErrorCode::SecurityBlocked,
            "with no resolved_target there is nothing vetted, so this must fail \
             on resolution/connect rather than as a guard decision: {error:?}"
        );
    }

    #[tokio::test]
    async fn static_driver_selects_the_pinned_client_for_preflighted_dns() {
        let driver = StaticDriver::new(Duration::from_secs(2), "QuarryTest/1.0").unwrap();
        // 93.184.216.34, not an RFC 5737 TEST-NET literal: `resolve_guard`
        // (which `pin` runs through) now blocks the documentation ranges too,
        // so a 203.0.113.0/24 address is no longer "preflighted" here.
        let hints = FetchHints {
            resolved_target: Some(crate::dns_guard::ResolvedTarget {
                host: "rebind.example".to_string(),
                addresses: vec!["93.184.216.34:443".parse().unwrap()],
            }),
            ..FetchHints::default()
        };
        let decision = EgressDecision {
            identity: EgressIdentity::Direct,
            attempt: 0,
            reason: "direct egress".to_string(),
        };
        let url: Url = "https://rebind.example/".parse().unwrap();

        let client = driver
            .client_for_decision(&decision, &hints, &url)
            .await
            .unwrap();

        assert!(std::ptr::eq(client, &driver.pinned_client));
    }

    #[tokio::test]
    async fn direct_fetch_without_preflight_hints_still_blocks_private_targets() {
        // Before this fix, a Direct decision with no `hints.resolved_target`
        // fell back to an unpinned client with zero SSRF validation of its
        // own -- EgressBroker::plan only selects a proxy, it never checks
        // the URL. This proves client_for_decision now preflights itself
        // instead of trusting an absent hint.
        let driver = StaticDriver::new(Duration::from_secs(2), "QuarryTest/1.0").unwrap();
        let decision = EgressDecision {
            identity: EgressIdentity::Direct,
            attempt: 0,
            reason: "direct egress".to_string(),
        };
        let url: Url = "http://127.0.0.1:9/".parse().unwrap();

        let err = driver
            .client_for_decision(&decision, &FetchHints::default(), &url)
            .await
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn fetch_without_hints_blocks_private_targets_end_to_end() {
        // Same gap, exercised through the public Driver::fetch entry point
        // (FetchHints::default() -- exactly what a caller gets if it never
        // pre-populates resolved_target) rather than calling
        // client_for_decision directly.
        let driver = StaticDriver::new(Duration::from_secs(2), "QuarryTest/1.0").unwrap();
        let url: Url = "http://169.254.169.254/latest/meta-data/".parse().unwrap();

        let err = driver.fetch(&url).await.unwrap_err();

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[test]
    fn resolves_dns_locally_matches_socks_variant_not_the_h_suffix() {
        assert!(resolves_dns_locally("socks5://proxy.example:1080"));
        assert!(resolves_dns_locally("socks4://proxy.example:1080"));
        assert!(!resolves_dns_locally("socks5h://proxy.example:1080"));
        assert!(!resolves_dns_locally("socks4a://proxy.example:1080"));
        assert!(!resolves_dns_locally("http://proxy.example:8080"));
        assert!(!resolves_dns_locally("https://proxy.example:8443"));
        assert!(!resolves_dns_locally("not a uri"));
    }

    #[test]
    fn proxy_pool_only_pins_client_side_resolving_schemes() {
        let pool = ProxyPool::from_env_string(
            "socks5://p1.example:1080;socks5h://p2.example:1080;http://p3.example:8080",
        );
        let driver =
            StaticDriver::with_proxy_pool(Duration::from_secs(2), "QuarryTest/1.0", pool).unwrap();

        assert!(driver
            .proxy_pin_resolvers
            .contains_key("socks5://p1.example:1080"));
        assert!(!driver
            .proxy_pin_resolvers
            .contains_key("socks5h://p2.example:1080"));
        assert!(!driver
            .proxy_pin_resolvers
            .contains_key("http://p3.example:8080"));
    }

    #[tokio::test]
    async fn proxy_egress_blocks_private_targets_for_a_client_side_resolving_proxy() {
        // Mirrors direct_fetch_without_preflight_hints_still_blocks_private_targets:
        // a socks5:// proxy entry now pins the same way Direct egress does, so a
        // private/loopback target is rejected before it ever reaches the proxy.
        let pool = ProxyPool::from_env_string("socks5://proxy.example:1080");
        let driver =
            StaticDriver::with_proxy_pool(Duration::from_secs(2), "QuarryTest/1.0", pool).unwrap();
        let decision = EgressDecision {
            identity: EgressIdentity::Proxy {
                uri: "socks5://proxy.example:1080".to_string(),
                processor_id: Some("quarry_proxy_pool".to_string()),
            },
            attempt: 0,
            reason: "proxy egress".to_string(),
        };
        let url: Url = "http://127.0.0.1:9/".parse().unwrap();

        let err = driver
            .client_for_decision(&decision, &FetchHints::default(), &url)
            .await
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn proxy_egress_selects_the_matching_client_for_preflighted_dns() {
        let pool = ProxyPool::from_env_string("socks5://proxy.example:1080");
        let driver =
            StaticDriver::with_proxy_pool(Duration::from_secs(2), "QuarryTest/1.0", pool).unwrap();
        // 93.184.216.34, not an RFC 5737 TEST-NET literal -- see the sibling
        // static-driver test above for why 203.0.113.0/24 no longer works.
        let hints = FetchHints {
            resolved_target: Some(crate::dns_guard::ResolvedTarget {
                host: "rebind.example".to_string(),
                addresses: vec!["93.184.216.34:443".parse().unwrap()],
            }),
            ..FetchHints::default()
        };
        let decision = EgressDecision {
            identity: EgressIdentity::Proxy {
                uri: "socks5://proxy.example:1080".to_string(),
                processor_id: Some("quarry_proxy_pool".to_string()),
            },
            attempt: 0,
            reason: "proxy egress".to_string(),
        };
        let url: Url = "https://rebind.example/".parse().unwrap();

        let client = driver
            .client_for_decision(&decision, &hints, &url)
            .await
            .unwrap();

        assert!(std::ptr::eq(
            client,
            driver
                .proxy_clients
                .get("socks5://proxy.example:1080")
                .unwrap()
        ));
    }

    #[tokio::test]
    async fn proxy_egress_does_not_pin_for_server_side_resolving_schemes() {
        // socks5h:// hands the raw hostname to the proxy for resolution -- a
        // client-side pin would never be consulted, so client_for_decision must
        // not attempt one. Using a target that would fail preflight if it WERE
        // (incorrectly) resolved locally proves this path is skipped entirely.
        let pool = ProxyPool::from_env_string("socks5h://proxy.example:1080");
        let driver =
            StaticDriver::with_proxy_pool(Duration::from_secs(2), "QuarryTest/1.0", pool).unwrap();
        let decision = EgressDecision {
            identity: EgressIdentity::Proxy {
                uri: "socks5h://proxy.example:1080".to_string(),
                processor_id: Some("quarry_proxy_pool".to_string()),
            },
            attempt: 0,
            reason: "proxy egress".to_string(),
        };
        let url: Url = "http://127.0.0.1:9/".parse().unwrap();

        let client = driver
            .client_for_decision(&decision, &FetchHints::default(), &url)
            .await
            .unwrap();

        assert!(std::ptr::eq(
            client,
            driver
                .proxy_clients
                .get("socks5h://proxy.example:1080")
                .unwrap()
        ));
    }
}
