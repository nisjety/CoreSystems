//! DNS-pinned HTTP/CONNECT egress for Chromium browser sessions.
//!
//! CDP Fetch interception can decide whether a browser request is allowed,
//! but Chromium would otherwise resolve the approved hostname again when it
//! opens a socket. This local proxy is the transport half of that boundary:
//! it resolves a destination once with Quarry's SSRF guard and connects to
//! the exact vetted IP address. Chromium keeps the original hostname for HTTP
//! `Host` and TLS SNI, while no untrusted hostname reaches `TcpStream::connect`.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use async_trait::async_trait;
use quarry_browser::{BrowserEgressPolicy, BrowserEgressProxyProvider, PinnedEgressProxyEndpoint};
use quarry_core::contracts::{BrowserEgressDecision, BrowserEgressReceipt};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use crate::dns_guard::{resolve_public_url, ResolvedTarget};

const MAX_PROXY_HEADER_BYTES: usize = 32 * 1024;
const RECEIPT_LIMIT: usize = 2_048;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PROXY_HEADER_TIMEOUT: Duration = Duration::from_secs(10);

/// Process-local egress authority for a Chromium instance. Its endpoint is
/// intentionally loopback-only: it is a browser transport boundary, never a
/// tenant-facing proxy service.
#[derive(Clone)]
pub struct PinnedBrowserEgressProxy {
    inner: Arc<PinnedBrowserEgressProxyInner>,
}

struct PinnedBrowserEgressProxyInner {
    sessions: Mutex<HashMap<String, PinnedBrowserEgressSession>>,
    shutdown: CancellationToken,
}

struct PinnedBrowserEgressSession {
    endpoint: PinnedEgressProxyEndpoint,
    receipts: Arc<Mutex<Vec<BrowserEgressReceipt>>>,
    policy: Arc<Mutex<BrowserEgressPolicy>>,
    shutdown: CancellationToken,
    accept_abort: AbortHandle,
}

impl Drop for PinnedBrowserEgressProxyInner {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Ok(sessions) = self.sessions.try_lock() {
            for session in sessions.values() {
                session.shutdown.cancel();
                session.accept_abort.abort();
            }
        }
    }
}

impl PinnedBrowserEgressProxy {
    /// Create the egress authority. A listener is bound lazily for each
    /// browser lease, giving every session a separate local port and receipt
    /// stream without exposing a run identifier to any destination server.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(PinnedBrowserEgressProxyInner {
                sessions: Mutex::new(HashMap::new()),
                shutdown: CancellationToken::new(),
            }),
        }
    }

    async fn bind_session(&self, session_key: &str) -> QuarryResult<PinnedEgressProxyEndpoint> {
        let mut sessions = self.inner.sessions.lock().await;
        if let Some(session) = sessions.get(session_key) {
            return Ok(session.endpoint.clone());
        }
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .map_err(|error| {
                QuarryError::new(ErrorCode::DriverFailed, "bind browser egress proxy failed")
                    .with_details(serde_json::json!({ "error": error.to_string() }))
            })?;
        let address = listener.local_addr().map_err(|error| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "read browser egress proxy address failed",
            )
            .with_details(serde_json::json!({ "error": error.to_string() }))
        })?;
        let endpoint = PinnedEgressProxyEndpoint::parse(format!("http://{address}"))?;
        let receipts = Arc::new(Mutex::new(Vec::new()));
        let sequence = Arc::new(Mutex::new(0));
        // A listener exists before the CDP context points at it. Until the
        // caller installs the run-derived policy, reject every destination:
        // an absent policy is never equivalent to an unconstrained grant.
        let policy = Arc::new(Mutex::new(BrowserEgressPolicy::deny_all()));
        let shutdown = self.inner.shutdown.child_token();
        let accept_shutdown = shutdown.clone();
        let accept_receipts = receipts.clone();
        let accept_sequence = sequence.clone();
        let accept_policy = policy.clone();
        let accept_task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = accept_shutdown.cancelled() => break,
                    accepted = listener.accept() => accepted,
                };
                let (stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        tracing::warn!(error = %error, "browser egress proxy accept failed");
                        continue;
                    }
                };
                let receipts = accept_receipts.clone();
                let sequence = accept_sequence.clone();
                let policy = accept_policy.clone();
                let connection_shutdown = accept_shutdown.clone();
                tokio::spawn(async move {
                    if let Err(error) =
                        serve_connection(stream, receipts, sequence, policy, connection_shutdown)
                            .await
                    {
                        tracing::debug!(error = %error, "browser egress proxy request refused");
                    }
                });
            }
        });
        sessions.insert(
            session_key.to_owned(),
            PinnedBrowserEgressSession {
                endpoint: endpoint.clone(),
                receipts,
                policy,
                shutdown,
                accept_abort: accept_task.abort_handle(),
            },
        );
        Ok(endpoint)
    }
}

impl Default for PinnedBrowserEgressProxy {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BrowserEgressProxyProvider for PinnedBrowserEgressProxy {
    async fn endpoint_for_session(
        &self,
        session_key: &str,
    ) -> QuarryResult<PinnedEgressProxyEndpoint> {
        self.bind_session(session_key).await
    }

    async fn receipts_after(
        &self,
        session_key: &str,
        after_sequence: u64,
        limit: usize,
    ) -> QuarryResult<Vec<BrowserEgressReceipt>> {
        let sessions = self.inner.sessions.lock().await;
        let receipts = sessions
            .get(session_key)
            .map(|session| session.receipts.clone())
            .unwrap_or_else(|| Arc::new(Mutex::new(Vec::new())));
        drop(sessions);
        let receipts = receipts.lock().await;
        if let Some(first_pending) = receipts
            .iter()
            .find(|receipt| receipt.sequence > after_sequence)
        {
            let expected = after_sequence.saturating_add(1);
            if first_pending.sequence != expected {
                return Err(QuarryError::new(
                    ErrorCode::Conflict,
                    "browser egress receipt continuity was lost; stop and re-observe",
                )
                .with_details(serde_json::json!({
                    "after_sequence": after_sequence,
                    "first_available_sequence": first_pending.sequence,
                })));
            }
        }
        Ok(receipts
            .iter()
            .filter(|receipt| receipt.sequence > after_sequence)
            .take(limit.clamp(1, RECEIPT_LIMIT))
            .cloned()
            .collect())
    }

    async fn configure_policy(
        &self,
        session_key: &str,
        policy: BrowserEgressPolicy,
    ) -> QuarryResult<()> {
        // Binding here ensures the policy exists before Chromium creates the
        // CDP context that points at this endpoint.
        self.bind_session(session_key).await?;
        let sessions = self.inner.sessions.lock().await;
        let policy_slot = sessions
            .get(session_key)
            .map(|session| session.policy.clone())
            .ok_or_else(|| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    "browser egress proxy session disappeared while configuring policy",
                )
            })?;
        drop(sessions);
        *policy_slot.lock().await = policy;
        Ok(())
    }

    async fn release_session(&self, session_key: &str) {
        if let Some(session) = self.inner.sessions.lock().await.remove(session_key) {
            session.shutdown.cancel();
            session.accept_abort.abort();
        }
    }
}

async fn serve_connection(
    client: TcpStream,
    receipts: Arc<Mutex<Vec<BrowserEgressReceipt>>>,
    sequence: Arc<Mutex<u64>>,
    policy: Arc<Mutex<BrowserEgressPolicy>>,
    shutdown: CancellationToken,
) -> QuarryResult<()> {
    tokio::select! {
        result = serve_connection_until_cancelled(client, receipts, sequence, policy) => result,
        _ = shutdown.cancelled() => Ok(()),
    }
}

async fn serve_connection_until_cancelled(
    mut client: TcpStream,
    receipts: Arc<Mutex<Vec<BrowserEgressReceipt>>>,
    sequence: Arc<Mutex<u64>>,
    policy: Arc<Mutex<BrowserEgressPolicy>>,
) -> QuarryResult<()> {
    // Chromium is the only intended peer, but this listener is still a
    // process-local network service. A partial request line must not retain a
    // task/socket forever and starve later governed browser work.
    let (head, remainder) =
        tokio::time::timeout(PROXY_HEADER_TIMEOUT, read_proxy_head(&mut client))
            .await
            .map_err(|_| {
                QuarryError::new(
                    ErrorCode::BadRequest,
                    "browser proxy request headers timed out",
                )
            })??;
    let request = parse_proxy_request(&head)?;
    if !policy.lock().await.allows_url(request.url.as_str()) {
        record_receipt(
            &receipts,
            &sequence,
            &request.method,
            &request.url,
            BrowserEgressDecision::Block,
            "domain_grant_blocked",
        )
        .await;
        let _ = write_proxy_error(&mut client, 403, "destination blocked").await;
        return Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            "browser proxy request is outside the run domain policy",
        ));
    }
    let target = match resolve_public_url(&request.url).await {
        Ok(target) => target,
        Err(error) => {
            record_receipt(
                &receipts,
                &sequence,
                &request.method,
                &request.url,
                BrowserEgressDecision::Block,
                "dns_or_ssrf_blocked",
            )
            .await;
            let _ = write_proxy_error(&mut client, 403, "destination blocked").await;
            return Err(error);
        }
    };
    let mut upstream = match connect_pinned(&target).await {
        Ok(stream) => stream,
        Err(error) => {
            record_receipt(
                &receipts,
                &sequence,
                &request.method,
                &request.url,
                BrowserEgressDecision::Block,
                "pinned_connect_failed",
            )
            .await;
            let _ = write_proxy_error(&mut client, 502, "destination unavailable").await;
            return Err(error);
        }
    };
    record_receipt(
        &receipts,
        &sequence,
        &request.method,
        &request.url,
        BrowserEgressDecision::Allow,
        "dns_pinned_proxy",
    )
    .await;

    match request.kind {
        ProxyRequestKind::Connect => {
            client
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .map_err(proxy_transport_error)?;
            if !remainder.is_empty() {
                upstream
                    .write_all(&remainder)
                    .await
                    .map_err(proxy_transport_error)?;
            }
        }
        ProxyRequestKind::Http { rewritten_head } => {
            upstream
                .write_all(&rewritten_head)
                .await
                .map_err(proxy_transport_error)?;
            if !remainder.is_empty() {
                upstream
                    .write_all(&remainder)
                    .await
                    .map_err(proxy_transport_error)?;
            }
        }
    }
    tokio::io::copy_bidirectional(&mut client, &mut upstream)
        .await
        .map_err(proxy_transport_error)?;
    Ok(())
}

struct ProxyRequest {
    method: String,
    url: url::Url,
    kind: ProxyRequestKind,
}

enum ProxyRequestKind {
    Connect,
    Http { rewritten_head: Vec<u8> },
}

async fn read_proxy_head(stream: &mut TcpStream) -> QuarryResult<(Vec<u8>, Vec<u8>)> {
    let mut bytes = Vec::with_capacity(2048);
    loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let end = end + 4;
            return Ok((bytes[..end].to_vec(), bytes[end..].to_vec()));
        }
        if bytes.len() >= MAX_PROXY_HEADER_BYTES {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "browser proxy request headers exceed the configured limit",
            ));
        }
        let mut chunk = [0_u8; 4096];
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(proxy_transport_error)?;
        if read == 0 {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "browser proxy connection closed before request headers",
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

fn parse_proxy_request(head: &[u8]) -> QuarryResult<ProxyRequest> {
    let text = std::str::from_utf8(head).map_err(|_| {
        QuarryError::new(
            ErrorCode::BadRequest,
            "browser proxy headers must be valid ASCII",
        )
    })?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if method.is_empty()
        || target.is_empty()
        || !version.starts_with("HTTP/")
        || parts.next().is_some()
    {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "invalid browser proxy request line",
        ));
    }
    if method.eq_ignore_ascii_case("CONNECT") {
        let url = connect_target_url(target)?;
        return Ok(ProxyRequest {
            method: "CONNECT".to_owned(),
            url,
            kind: ProxyRequestKind::Connect,
        });
    }
    let url = url::Url::parse(target).map_err(|_| {
        QuarryError::new(
            ErrorCode::BadRequest,
            "browser proxy HTTP request must use an absolute URL",
        )
    })?;
    if url.scheme() != "http" || url.host_str().is_none() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "browser proxy permits only HTTP absolute requests or HTTPS CONNECT tunnels",
        ));
    }
    let path_and_query = &url[url::Position::BeforePath..];
    let mut rewritten = format!("{method} {path_and_query} {version}\r\n").into_bytes();
    for header in lines {
        if header.is_empty() {
            break;
        }
        let header_name = header.split_once(':').map(|(name, _)| name.trim());
        if matches!(header_name, Some(name) if name.eq_ignore_ascii_case("proxy-connection") || name.eq_ignore_ascii_case("proxy-authorization"))
        {
            continue;
        }
        rewritten.extend_from_slice(header.as_bytes());
        rewritten.extend_from_slice(b"\r\n");
    }
    rewritten.extend_from_slice(b"\r\n");
    Ok(ProxyRequest {
        method: method.to_ascii_uppercase(),
        url,
        kind: ProxyRequestKind::Http {
            rewritten_head: rewritten,
        },
    })
}

fn connect_target_url(authority: &str) -> QuarryResult<url::Url> {
    if authority.contains('/') || authority.contains('@') || authority.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "invalid CONNECT authority",
        ));
    }
    let url = url::Url::parse(&format!("https://{authority}/"))
        .map_err(|_| QuarryError::new(ErrorCode::BadRequest, "invalid CONNECT destination"))?;
    if url.host_str().is_none() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "CONNECT destination is missing a host",
        ));
    }
    Ok(url)
}

async fn connect_pinned(target: &ResolvedTarget) -> QuarryResult<TcpStream> {
    let mut last_error = None;
    for address in &target.addresses {
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => last_error = Some(error.to_string()),
            Err(_) => last_error = Some("connection timed out".to_owned()),
        }
    }
    Err(QuarryError::new(
        ErrorCode::DriverFailed,
        "browser egress proxy could not connect to any vetted destination",
    )
    .with_details(serde_json::json!({ "error": last_error })))
}

async fn write_proxy_error(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
) -> std::io::Result<()> {
    let response =
        format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(response.as_bytes()).await
}

fn proxy_transport_error(error: std::io::Error) -> QuarryError {
    QuarryError::new(
        ErrorCode::DriverFailed,
        "browser egress proxy transport failed",
    )
    .with_details(serde_json::json!({ "error": error.to_string() }))
}

async fn record_receipt(
    receipts: &Arc<Mutex<Vec<BrowserEgressReceipt>>>,
    sequence: &Arc<Mutex<u64>>,
    method: &str,
    url: &url::Url,
    decision: BrowserEgressDecision,
    policy: &str,
) {
    let mut sequence_guard = sequence.lock().await;
    *sequence_guard = sequence_guard.saturating_add(1);
    let receipt = BrowserEgressReceipt {
        sequence: *sequence_guard,
        tab_id: None,
        method: method.to_owned(),
        url: redacted_url(url),
        decision,
        policy: policy.to_owned(),
        timestamp_ms: now_ms(),
    };
    drop(sequence_guard);
    let mut receipts = receipts.lock().await;
    receipts.push(receipt);
    if receipts.len() > RECEIPT_LIMIT {
        let excess = receipts.len() - RECEIPT_LIMIT;
        receipts.drain(0..excess);
    }
}

fn redacted_url(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or("unparseable");
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    format!("{}://{host}{port}{}", url.scheme(), url.path())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_browser::BrowserEgressProxyProvider;

    async fn send_proxy_request(endpoint: &PinnedEgressProxyEndpoint, request: &str) -> String {
        let endpoint = url::Url::parse(endpoint.as_str()).expect("valid proxy endpoint");
        let address = SocketAddr::new(
            endpoint
                .host_str()
                .expect("proxy host")
                .parse()
                .expect("literal loopback proxy host"),
            endpoint.port().expect("proxy port"),
        );
        let mut stream = TcpStream::connect(address).await.expect("connect proxy");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("write proxy request");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .expect("read proxy response");
        String::from_utf8_lossy(&response).into_owned()
    }

    #[tokio::test]
    async fn unconfigured_session_is_deny_all_and_emits_a_receipt() {
        let proxy = PinnedBrowserEgressProxy::new();
        let endpoint = proxy
            .endpoint_for_session("deny-all")
            .await
            .expect("bind session proxy");
        let response = send_proxy_request(
            &endpoint,
            "GET http://example.com/private?token=secret HTTP/1.1\r\nHost: example.com\r\n\r\n",
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 403"));
        let receipts = proxy
            .receipts_after("deny-all", 0, 10)
            .await
            .expect("read receipts");
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].sequence, 1);
        assert_eq!(receipts[0].decision, BrowserEgressDecision::Block);
        assert_eq!(receipts[0].policy, "domain_grant_blocked");
        assert_eq!(receipts[0].url, "http://example.com/private");
    }

    #[tokio::test]
    async fn allowed_loopback_and_metadata_destinations_are_blocked_after_resolution() {
        let proxy = PinnedBrowserEgressProxy::new();
        let session = "resolved-private";
        proxy
            .configure_policy(
                session,
                BrowserEgressPolicy::from_allowed_domains(&[
                    "127.0.0.1".to_owned(),
                    "169.254.169.254".to_owned(),
                ]),
            )
            .await
            .expect("configure transport policy");
        let endpoint = proxy
            .endpoint_for_session(session)
            .await
            .expect("session endpoint");

        let loopback = send_proxy_request(
            &endpoint,
            "GET http://127.0.0.1:9/a HTTP/1.1\r\nHost: 127.0.0.1:9\r\n\r\n",
        )
        .await;
        let metadata = send_proxy_request(
            &endpoint,
            "CONNECT 169.254.169.254:80 HTTP/1.1\r\nHost: 169.254.169.254:80\r\n\r\n",
        )
        .await;

        assert!(loopback.starts_with("HTTP/1.1 403"));
        assert!(metadata.starts_with("HTTP/1.1 403"));
        let receipts = proxy
            .receipts_after(session, 0, 10)
            .await
            .expect("ordered receipts");
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0].sequence, 1);
        assert_eq!(receipts[1].sequence, 2);
        assert!(receipts
            .iter()
            .all(|receipt| receipt.policy == "dns_or_ssrf_blocked"));
        assert!(receipts
            .iter()
            .all(|receipt| receipt.decision == BrowserEgressDecision::Block));
    }

    #[tokio::test]
    async fn releasing_a_session_removes_its_endpoint_and_receipts() {
        let proxy = PinnedBrowserEgressProxy::new();
        let original = proxy
            .endpoint_for_session("released")
            .await
            .expect("bind original endpoint");
        proxy.release_session("released").await;
        assert!(proxy
            .receipts_after("released", 0, 10)
            .await
            .expect("released receipt read")
            .is_empty());
        let replacement = proxy
            .endpoint_for_session("released")
            .await
            .expect("bind replacement endpoint");
        assert_ne!(original, replacement);
    }

    #[cfg(feature = "chromiumoxide")]
    mod chromium_proof {
        use super::*;
        use std::sync::Arc;
        use std::time::Duration;

        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;
        use quarry_core::ids::kinds;
        use quarry_core::lease::{BrowserLease, Capability, ProxyAffinity};
        use wiremock::MockServer;

        fn lease(key: &str) -> BrowserLease {
            BrowserLease {
                lease_id: kinds::LeaseKind::new(),
                profile_id: kinds::ProfileKind::new(),
                session_affinity_key: key.to_owned(),
                proxy_affinity: ProxyAffinity {
                    pool: "default".to_owned(),
                    sticky_key: None,
                },
                ttl_s: 60,
                capabilities: vec![Capability::Screenshots],
                artifact_bucket: "test".to_owned(),
                persist_profile: false,
                viewport: None,
                org_id: "test-org".to_owned(),
            }
        }

        async fn governed_driver(
            key: &str,
            domains: &[String],
        ) -> (ChromiumoxideDriver, quarry_browser::BrowserSession) {
            let proxy = Arc::new(PinnedBrowserEgressProxy::new());
            let driver = ChromiumoxideDriver::new().with_pinned_egress_proxy(proxy);
            let session = driver.acquire(&lease(key)).await.expect("launch browser");
            driver
                .configure_egress_policy(
                    &session,
                    BrowserEgressPolicy::from_allowed_domains(domains),
                )
                .await
                .expect("configure browser egress");
            driver
                .new_tab(&session, None)
                .await
                .expect("open governed blank tab");
            (driver, session)
        }

        #[tokio::test]
        async fn browser_blocks_iframe_xhr_fetch_subresource_and_script_navigation() {
            let target = MockServer::start().await;
            let host = "127.0.0.1".to_owned();
            let (driver, session) = governed_driver("browser-requests", &[host]).await;
            let capabilities = driver.capabilities();
            assert!(capabilities.isolated_egress);
            assert!(capabilities.security_evidence);
            let script = format!(
                r#"(() => {{
                    const base = '{base}';
                    fetch(base + '/fetch').catch(() => {{}});
                    const xhr = new XMLHttpRequest();
                    xhr.open('GET', base + '/xhr');
                    xhr.send();
                    const image = new Image();
                    image.src = base + '/subresource.png';
                    document.body.appendChild(image);
                    const frame = document.createElement('iframe');
                    frame.src = base + '/iframe';
                    document.body.appendChild(frame);
                    setTimeout(() => window.location.assign(base + '/script-navigation'), 50);
                    return true;
                }})()"#,
                base = target.uri()
            );
            driver
                .evaluate(&session, &script)
                .await
                .expect("initiate page requests");
            tokio::time::sleep(Duration::from_millis(750)).await;

            assert!(target
                .received_requests()
                .await
                .expect("target request log")
                .is_empty());
            let receipts = driver
                .egress_receipts(&session, 0, 100)
                .await
                .expect("browser egress receipts");
            for path in [
                "/fetch",
                "/xhr",
                "/subresource.png",
                "/iframe",
                "/script-navigation",
            ] {
                assert!(
                    receipts.iter().any(|receipt| {
                        receipt.url.ends_with(path)
                            && receipt.decision == BrowserEgressDecision::Block
                    }),
                    "missing blocked receipt for {path}: {receipts:?}"
                );
            }
            let telemetry = driver
                .telemetry(&session)
                .await
                .expect("browser telemetry sample");
            assert_eq!(
                telemetry.verified_action_cost_micro_usd, None,
                "local Chromium must not fabricate an external provider cost"
            );
            driver.release(session).await.expect("release browser");
        }

        #[tokio::test]
        #[ignore = "requires public network access to exercise a real HTTP redirect"]
        async fn public_redirect_to_ungranted_target_is_blocked_before_target_connection() {
            let domains = vec!["httpbingo.org".to_owned()];
            let (driver, session) = governed_driver("browser-redirect", &domains).await;
            let redirect =
                "https://httpbingo.org/redirect-to?url=https%3A%2F%2Fexample.com%2Fredirected";
            let _ = driver.goto(&session, &redirect).await;
            tokio::time::sleep(Duration::from_millis(500)).await;

            let receipts = driver
                .egress_receipts(&session, 0, 100)
                .await
                .expect("redirect receipts");
            assert!(
                receipts.iter().any(|receipt| {
                    receipt.url == "https://example.com/redirected"
                        && receipt.decision == BrowserEgressDecision::Block
                }),
                "missing redirected target receipt: {receipts:?}"
            );
            assert!(
                !receipts.iter().any(|receipt| {
                    receipt.tab_id.is_none()
                        && receipt.url.starts_with("https://example.com")
                        && receipt.decision == BrowserEgressDecision::Allow
                }),
                "redirect target must not reach the transport: {receipts:?}"
            );
            driver.release(session).await.expect("release browser");
        }
    }
}
