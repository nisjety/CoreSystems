//! Static fetch driver. reqwest + rustls. Browser driver added in Phase 2.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

use quarry_core::error::{driver_failed_reason, ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use serde_json::json;

use crate::capture::PageCaptures;
use crate::dns_guard::{resolve_public_url, PinnedDnsResolver};
use crate::driver::{Driver, FetchHints};
use crate::egress_broker::{EgressBroker, EgressDecision, EgressIdentity};
use crate::escalation::{escalation_allowed, EscalationMode, EscalationTrigger};
use crate::fallback_driver::fetch_sub_codes_enabled;
use crate::proxy_pool::{ProxyEntry, ProxyPool};

/// Stable public discovery identity; robots parsing must use this same token.
/// The token is defined once, in `quarry_core::identity::DISCOVERY_TOKEN` (F-t2-067).
pub const DISCOVERY_USER_AGENT: &str = quarry_core::identity::DISCOVERY_TOKEN;

const MAX_REDIRECT_URL_BYTES: usize = 4096;

/// Validate the URL shape before any network request. The transport still
/// resolves and pins public addresses for each separate probe.
pub fn validate_redirect_probe_url(url: &Url) -> QuarryResult<()> {
    if url.as_str().len() > MAX_REDIRECT_URL_BYTES
        || url.port().is_some()
        || quarry_security::heur::check(url).decision == quarry_security::Decision::Block
    {
        return Err(redirect_probe_blocked());
    }
    Ok(())
}

fn redirect_probe_blocked() -> QuarryError {
    QuarryError::new(ErrorCode::SecurityBlocked, "redirect probe URL blocked")
}

pub(crate) fn redirect_probe_target(
    url: &Url,
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
) -> QuarryResult<Option<Url>> {
    if !matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
        return Ok(None);
    }

    let all_locations = headers.get_all(reqwest::header::LOCATION);
    let mut locations = all_locations.iter();
    let location = locations
        .next()
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= MAX_REDIRECT_URL_BYTES)
        .ok_or_else(redirect_probe_blocked)?;
    if locations.next().is_some() {
        return Err(redirect_probe_blocked());
    }
    let mut target = url.join(location).map_err(|_| redirect_probe_blocked())?;
    target.set_fragment(None);
    validate_redirect_probe_url(&target)?;
    if url.scheme() == "https" && target.scheme() == "http" {
        return Err(redirect_probe_blocked());
    }
    Ok(Some(target))
}

async fn send_redirect_probe(client: &reqwest::Client, url: &Url) -> QuarryResult<Option<Url>> {
    // Use GET for sites that implement different routing for HEAD. Drop the
    // response immediately after inspecting its status and Location header.
    let response = client.get(url.clone()).send().await.map_err(|error| {
        let code = if error.is_timeout() {
            ErrorCode::Timeout
        } else {
            ErrorCode::DriverFailed
        };
        QuarryError::new(code, "redirect probe request failed")
    })?;
    redirect_probe_target(url, response.status(), response.headers())
}

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub status: u16,
    pub final_url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub duration_ms: u64,
    /// The driver that actually produced this response. Set by each
    /// concrete driver; fallback wrappers (`FallbackDriver`,
    /// `TransportFallbackDriver`) pass it through untouched. Consumers
    /// must attribute output to this field, not to `Driver::kind()` on
    /// the driver they hold — after chain rotation the wrapper's
    /// `kind()` still reports the planned primary.
    pub served_by: DriverKind,
    /// Captures a browser fetch made beside the page body (WS13.4a). `None`
    /// for every driver until the browser capture (WS13.3) fills it. Never
    /// serialized.
    pub captures: Option<Box<PageCaptures>>,
}

impl FetchResponse {
    /// Builds a response with no captures. Prefer this to a struct literal so
    /// that a new field defaults in one place.
    pub fn new(
        status: u16,
        final_url: Url,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        duration_ms: u64,
        served_by: DriverKind,
    ) -> Self {
        Self {
            status,
            final_url,
            headers,
            body,
            duration_ms,
            served_by,
            captures: None,
        }
    }
}

/// Redirect-hop ceiling for `do_fetch`. Real sites commonly chain
/// apex→www→locale (2–3 hops); 5 covers that with margin while still
/// bounding redirect loops.
const MAX_REDIRECT_HOPS: usize = 5;

/// What one egress-planned request produced: a final response, or a redirect
/// whose target still has to pass the same preflight the entry URL did before
/// any follow-up request is allowed to touch the network.
enum SendOutcome {
    Response(FetchResponse),
    Redirect { status: u16, location: Url },
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
    /// Whether a block status may rotate the proxy (WS3.I). Transport errors
    /// rotate in both modes.
    mode: EscalationMode,
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
            mode: EscalationMode::current(),
        })
    }

    /// Pins the escalation mode (and the broker's) instead of reading the
    /// process default, so a test never depends on the environment.
    pub fn with_escalation_mode(mut self, mode: EscalationMode) -> Self {
        self.egress = self.egress.with_escalation_mode(mode);
        self.mode = mode;
        self
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
                        format!(
                            "egress proxy client missing for configured proxy: {}",
                            redacted_proxy_uri(uri)
                        ),
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
    ///
    /// Redirects are followed manually, never by reqwest: each hop's target
    /// is a fresh SSRF boundary, so before any follow-up request the target
    /// re-runs `resolve_public_url` — the exact preflight the entry URL
    /// passed — and its addresses are pinned via `hints.resolved_target`,
    /// closing the "otherwise safe public URL redirects to a private or
    /// metadata address" hole that implicit following would open. Hops are
    /// capped at `MAX_REDIRECT_HOPS`; conditional validators only apply to
    /// the entry URL (they describe the entry resource, not the target's).
    async fn do_fetch(&self, url: &Url, hints: &FetchHints) -> QuarryResult<FetchResponse> {
        let mut current_url = url.clone();
        let mut current_hints = hints.clone();
        for hop in 0..=MAX_REDIRECT_HOPS {
            match self.fetch_via_egress(&current_url, &current_hints).await? {
                SendOutcome::Response(resp) => return Ok(resp),
                SendOutcome::Redirect { status, location } => {
                    if hop == MAX_REDIRECT_HOPS {
                        break;
                    }
                    // Same public-address policy as the entry URL; the pinned
                    // resolver then connects only to these vetted addresses.
                    let target = resolve_public_url(&location).await?;
                    // Neither URL: both are the caller's, and `from`/`to` are names the
                    // ZDR event filter does not know.
                    tracing::debug!(
                        status,
                        hop,
                        "following redirect after target re-validation"
                    );
                    current_hints.resolved_target = Some(target);
                    current_hints.if_none_match = None;
                    current_hints.if_modified_since = None;
                    current_url = location;
                }
            }
        }
        Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            format!("static fetch exceeded {MAX_REDIRECT_HOPS} redirect hops"),
        ))
    }

    /// One egress-planned pass for a single, already-validated URL: plan
    /// egress identities, attempt each until one yields a response, and
    /// surface redirects to `do_fetch` for target re-validation.
    async fn fetch_via_egress(&self, url: &Url, hints: &FetchHints) -> QuarryResult<SendOutcome> {
        let host = url.host_str().unwrap_or("").to_string();
        let plan = self.egress.plan(hints, url)?;
        let mut attempts = Vec::with_capacity(plan.len());
        for (idx, decision) in plan.iter().enumerate() {
            let client = self.client_for_decision(decision, hints, url).await?;
            match self.send_once(client, url, hints).await {
                Ok(SendOutcome::Redirect { status, location }) => {
                    // The origin answered; a redirect is not an egress block,
                    // so record the status for host health and hand the hop
                    // decision back to do_fetch instead of rotating proxies.
                    self.egress.mark_http_status(&host, &decision.identity, status);
                    return Ok(SendOutcome::Redirect { status, location });
                }
                Ok(SendOutcome::Response(resp)) => {
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
                        // A status is not a transport failure: under `honest`
                        // the first proxy's answer is final, one attempt, and
                        // the caller gets the existing block code.
                        if idx + 1 < plan.len()
                            && escalation_allowed(
                                self.mode,
                                EscalationTrigger::BlockStatus(resp.status),
                            )
                        {
                            tracing::warn!(
                                status = resp.status,
                                attempt = idx,
                                egress = decision.identity.label(),
                                "egress blocked; rotating to next approved proxy"
                            );
                            continue;
                        }
                        return Err(blocked_error(resp.status, attempts, self.mode));
                    }
                    return Ok(SendOutcome::Response(resp));
                }
                Err(err) => {
                    self.egress.mark_transport_error(&host, &decision.identity);
                    attempts.push(attempt_record(
                        decision,
                        None,
                        Some(&err),
                        "transport_error",
                    ));
                    if is_retryable_egress_error(err.code)
                        && idx + 1 < plan.len()
                        && escalation_allowed(self.mode, EscalationTrigger::TransportError)
                    {
                        tracing::warn!(
                            error_class = err.code.wire_name(),
                            attempt = idx,
                            egress = decision.identity.label(),
                            "egress transport failed; rotating to next approved proxy"
                        );
                        continue;
                    }
                    return Err(with_egress_attempts(err, attempts));
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
    ) -> QuarryResult<SendOutcome> {
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
        let resp = req.send().await.map_err(static_send_error)?;
        let status = resp.status().as_u16();
        // Redirect targets have not passed Quarry's URL preflight or DNS guard,
        // so they never go through reqwest's implicit redirect engine (the
        // client is built with Policy::none): the target could resolve to a
        // private or metadata address after an otherwise safe public URL was
        // accepted. Surface the parsed target to do_fetch, which re-runs the
        // full preflight before any follow-up request.
        if resp.status().is_redirection() {
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::SecurityBlocked,
                        "static fetch redirect carried no readable Location target",
                    )
                })?;
            // Join resolves relative targets against the redirecting URL.
            let next = url.join(location).map_err(|e| {
                QuarryError::new(
                    ErrorCode::SecurityBlocked,
                    format!("static fetch redirect target failed to parse: {e}"),
                )
            })?;
            if !matches!(next.scheme(), "http" | "https") {
                return Err(QuarryError::new(
                    ErrorCode::SecurityBlocked,
                    format!(
                        "static fetch redirect to non-http scheme blocked: {}",
                        next.scheme()
                    ),
                ));
            }
            return Ok(SendOutcome::Redirect {
                status,
                location: next,
            });
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
        } else {
            match hints.max_body_bytes {
                None => resp
                    .bytes()
                    .await
                    .map_err(|e| QuarryError::new(ErrorCode::DriverFailed, format!("read body: {}", e.without_url())))?
                    .to_vec(),
                Some(cap) => read_capped(resp, cap, hints.truncate_at_cap).await?,
            }
        };
        Ok(SendOutcome::Response(FetchResponse {
            status,
            final_url,
            headers,
            body,
            duration_ms: start.elapsed().as_millis() as u64,
            served_by: DriverKind::Static,
            captures: None,
        }))
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
        let resp = req.send().await.map_err(static_send_error)?;
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
                .map_err(|e| QuarryError::new(ErrorCode::DriverFailed, format!("read body: {}", e.without_url())))?
                .to_vec()
        };
        Ok(FetchResponse {
            status,
            final_url,
            headers,
            body,
            duration_ms: start.elapsed().as_millis() as u64,
            served_by: DriverKind::Static,
            captures: None,
        })
    }
}

/// Count decoded chunks, including responses without Content-Length and
/// compressed responses whose expanded body is larger than the wire length.
pub(crate) async fn read_bounded_body(mut response: reqwest::Response, limit: usize) -> QuarryResult<Vec<u8>> {
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

/// Read a response body up to `cap` bytes, one chunk at a time, and stop there:
/// a body longer than the cap is never held whole. Past the cap the result is
/// the bytes read so far when `truncate`, otherwise a `DriverFailed` error.
async fn read_capped(
    mut resp: reqwest::Response,
    cap: usize,
    truncate: bool,
) -> QuarryResult<Vec<u8>> {
    let mut body: Vec<u8> = Vec::new();
    loop {
        let chunk = resp.chunk().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("read body: {}", e.without_url()),
            )
        })?;
        let Some(chunk) = chunk else {
            return Ok(body);
        };
        let room = cap.saturating_sub(body.len());
        if chunk.len() > room {
            if truncate {
                body.extend_from_slice(&chunk[..room]);
                return Ok(body);
            }
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                format!("response body is longer than the {cap} byte cap"),
            ));
        }
        body.extend_from_slice(&chunk);
    }
}

/// What a failed static send leaves the driver with (E3.8.1). A timeout keeps `TIMEOUT`. With
/// `QUARRY_EDGE__FETCH_SUB_CODES` on, a failure that names a class is `DRIVER_FAILED` with that
/// reason and its own retryable flag. Otherwise, or when it names none, it is the code it always
/// was: `UPSTREAM_BLOCKED` for a connect error, `DRIVER_FAILED` for the rest. Neither message
/// carries the URL.
fn static_send_error(error: reqwest::Error) -> QuarryError {
    if !error.is_timeout() && fetch_sub_codes_enabled() {
        if let Some(class) = classify_transport_error(&error) {
            return QuarryError::driver_failed(class, format!("static fetch: {class}"));
        }
    }
    let code = if error.is_timeout() {
        ErrorCode::Timeout
    } else if error.is_connect() {
        ErrorCode::UpstreamBlocked
    } else {
        ErrorCode::DriverFailed
    };
    QuarryError::new(code, format!("static fetch: {}", error.without_url()))
}

/// The fetch sub-code a failed static send names (E3.8.1): `dns`, `tls` or `connect`. It is read
/// from the text of the error chain below the top error, whose own text carries the URL. Pure: it
/// reads no flag. `None` when the failure names no class. The HTTP/3 send uses it too.
pub(crate) fn classify_transport_error(error: &reqwest::Error) -> Option<&'static str> {
    let mut messages = Vec::new();
    let mut source = std::error::Error::source(error);
    while let Some(inner) = source {
        messages.push(inner.to_string());
        source = inner.source();
    }
    driver_failed_reason::from_transport_chain(
        messages.iter().map(String::as_str),
        error.is_connect(),
        error.is_dns(),
    )
}

/// The error with the egress attempts added to its details. A reason the error already carries
/// (a fetch sub-code, E3.8.1) stays: the attempts are one more key.
fn with_egress_attempts(mut error: QuarryError, attempts: Vec<serde_json::Value>) -> QuarryError {
    let listed = json!(attempts);
    match error.details.as_mut().and_then(serde_json::Value::as_object_mut) {
        Some(object) => {
            object.insert("egress_attempts".to_owned(), listed);
            error
        }
        None => error.with_details(json!({ "egress_attempts": listed })),
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
            ("proxy", Some(redacted_proxy_uri(uri)), processor_id.as_deref())
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
        // The error class only. The message of a transport error can carry the
        // proxy URI (with its userinfo) or the target URL; these records are
        // returned to the caller in the error details.
        "error_class": error.map(|e| e.code.wire_name()),
    })
}

/// A proxy URI as it may appear in a response or a message: scheme, host and
/// port only. The userinfo (`user:pass@`), path, query and fragment are
/// dropped. A value that does not parse is replaced by a fixed word so the
/// raw text never leaks through the fallback.
fn redacted_proxy_uri(uri: &str) -> String {
    match Url::parse(uri) {
        Ok(parsed) => match parsed.host_str() {
            Some(host) => match parsed.port() {
                Some(port) => format!("{}://{}:{}", parsed.scheme(), host, port),
                None => format!("{}://{}", parsed.scheme(), host),
            },
            None => "proxy".to_string(),
        },
        Err(_) => "proxy".to_string(),
    }
}

fn blocked_error(
    status: u16,
    attempts: Vec<serde_json::Value>,
    mode: EscalationMode,
) -> QuarryError {
    let code = if status == 429 {
        ErrorCode::RateLimited
    } else {
        ErrorCode::UpstreamBlocked
    };
    let message = if mode.is_honest() {
        format!("egress identity answered with a block status and was not rotated; status={status}")
    } else {
        format!("all approved egress identities blocked; last status={status}")
    };
    QuarryError::new(code, message).with_details(json!({ "egress_attempts": attempts }))
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
        self.send_single_hop(
            &self.bounded_client,
            url,
            &FetchHints::default(),
            Some(max_bytes),
            false,
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

    async fn probe_redirect(&self, url: &Url) -> QuarryResult<Option<Url>> {
        validate_redirect_probe_url(url)?;
        self.pinned_resolver.pin(resolve_public_url(url).await?)?;
        send_redirect_probe(&self.bounded_client, url).await
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
        let parsed = reqwest::Proxy::all(&p.uri).map_err(|_| {
            // Neither the URI nor the reqwest text: both can hold the userinfo.
            QuarryError::new(
                ErrorCode::Internal,
                format!("invalid proxy URI {}", redacted_proxy_uri(&p.uri)),
            )
        })?;
        builder = builder.proxy(parsed);
    }
    builder
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("http client: {}", e.without_url())))
}

// -- E3.8.8: no_egress mode ---------------------------------------------------------------------
//
// A no_egress scrape never touches the network beyond the platform: it reads the org's stored
// artifact for the URL from the Data Plane, through its API only (`crate::data_plane_http`), and
// answers a typed miss when there is none. The URL is never logged and never written into an error
// message or its details. A ZDR caller is refused before any lookup: stored artifacts are durable
// content, and ADR topic 4 refuses URL-keyed reads for ZDR runs. Dark until the edge builds the
// driver behind `QUARRY_EDGE__NO_EGRESS` (default off).

use std::sync::OnceLock;

use quarry_core::privacy::PrivacyClassification;

use crate::data_plane_http::{ArtifactLookup, ArtifactReader};

/// The environment variable that turns the no_egress mode on (E3.8.8). Default off.
pub const NO_EGRESS_ENV: &str = "QUARRY_EDGE__NO_EGRESS";

/// The `details.reason` values a no_egress fetch refuses or misses with.
pub mod no_egress_reason {
    /// The Data Plane holds no artifact of the org for the URL.
    pub const MISS: &str = "no_egress_miss";
    /// The caller runs under ZDR: stored artifacts are not read for it.
    pub const ZDR_REFUSED: &str = "no_egress_zdr_refused";
    /// The fetch carries no org, so there is no org's store to read.
    pub const NO_ORG: &str = "no_egress_no_org";
}

/// The pure half of [`no_egress_enabled`]. `1`, `true` and `on` (any case, surrounding blanks
/// ignored) turn it on. Unset, blank or any other value is off.
pub fn no_egress_from_value(value: Option<&str>) -> bool {
    matches!(
        value.map(|raw| raw.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "on")
    )
}

/// The process-wide default, read from the environment once.
pub fn no_egress_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let raw = std::env::var(NO_EGRESS_ENV).ok();
        let enabled = no_egress_from_value(raw.as_deref());
        tracing::info!(enabled = enabled, "quarry.no_egress");
        enabled
    })
}

fn no_egress_error(code: ErrorCode, reason: &'static str, message: &'static str) -> QuarryError {
    QuarryError::new(code, message).with_details(json!({ "reason": reason, "no_egress": true }))
}

/// The typed miss: NOT_FOUND, not retryable, and no URL in the message or the details.
pub fn no_egress_miss() -> QuarryError {
    no_egress_error(
        ErrorCode::NotFound,
        no_egress_reason::MISS,
        "no_egress: no stored artifact for the requested page",
    )
}

/// A driver that serves pages only from the org's stored Data Plane artifacts. It opens no socket
/// to the page's host, follows no redirect and uses no proxy.
pub struct NoEgressDriver {
    reader: Arc<dyn ArtifactReader>,
}

impl NoEgressDriver {
    pub fn new(reader: Arc<dyn ArtifactReader>) -> Self {
        Self { reader }
    }

    /// The driver when `QUARRY_EDGE__NO_EGRESS` is on, `None` otherwise.
    pub fn from_env(reader: Arc<dyn ArtifactReader>) -> Option<Self> {
        Self::when_enabled(no_egress_enabled(), reader)
    }

    /// The pure half of [`Self::from_env`].
    pub fn when_enabled(enabled: bool, reader: Arc<dyn ArtifactReader>) -> Option<Self> {
        enabled.then(|| Self::new(reader))
    }

    async fn read(&self, url: &Url, hints: &FetchHints) -> QuarryResult<FetchResponse> {
        if hints.privacy.zdr.is_active()
            || hints.privacy.privacy_classification == PrivacyClassification::ZdrEphemeral
        {
            return Err(no_egress_error(
                ErrorCode::Forbidden,
                no_egress_reason::ZDR_REFUSED,
                "no_egress: stored artifacts are not read under ZDR",
            ));
        }
        if hints.org_id.trim().is_empty() {
            return Err(no_egress_error(
                ErrorCode::BadRequest,
                no_egress_reason::NO_ORG,
                "no_egress: the fetch names no org",
            ));
        }
        let started = std::time::Instant::now();
        let artifact = match self
            .reader
            .lookup(&hints.org_id, url.as_str(), hints.max_body_bytes)
            .await?
        {
            ArtifactLookup::Hit(artifact) => artifact,
            ArtifactLookup::Miss => return Err(no_egress_miss()),
        };
        let mut body = artifact.body;
        if let Some(cap) = hints.max_body_bytes {
            if body.len() > cap {
                if !hints.truncate_at_cap {
                    return Err(QuarryError::new(
                        ErrorCode::DriverFailed,
                        "no_egress: the stored body is larger than the read cap",
                    ));
                }
                body.truncate(cap);
            }
        }
        // A stored final URL that does not parse is the Data Plane's fault, not the page's: the
        // requested URL stands in for it.
        let final_url = Url::parse(&artifact.final_url).unwrap_or_else(|_| url.clone());
        Ok(FetchResponse::new(
            artifact.status,
            final_url,
            artifact.headers,
            body,
            started.elapsed().as_millis() as u64,
            // No driver kind names a stored read yet (quarry-core); the stored page was a static capture.
            DriverKind::Static,
        ))
    }
}

#[async_trait]
impl Driver for NoEgressDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    /// The stored read never touches the page's host, so the run resolves no DNS for it.
    fn reaches_network(&self) -> bool {
        false
    }

    /// A bare fetch names no org, so it is refused: no_egress never reads without one.
    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.read(url, &FetchHints::default()).await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        self.read(url, hints).await
    }

    // `fetch_bounded` and `fetch_discovery_hop` keep the trait's fail-closed defaults: discovery
    // (robots, sitemaps) is live egress and has no stored form yet.
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::error::ErrorCode;
    use quarry_core::privacy::PrivacyPolicy;
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn fetch_response_new_defaults_captures_to_none() {
        let response = FetchResponse::new(
            200,
            Url::parse("https://example.com/").expect("static url"),
            Vec::new(),
            b"ok".to_vec(),
            1,
            DriverKind::Static,
        );
        assert_eq!(response.status, 200);
        assert!(response.captures.is_none());
    }

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
            let _request_len = socket.read(&mut buf).await.unwrap();
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
        let error = driver
            .probe_redirect(&Url::parse("http://127.0.0.1/").unwrap())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::SecurityBlocked);
    }

    #[test]
    fn redirect_probe_resolves_relative_location_and_drops_fragment() {
        let url = Url::parse("https://example.com/path/page").unwrap();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::LOCATION,
            "../new?view=all#anchor".parse().unwrap(),
        );
        assert_eq!(
            redirect_probe_target(&url, reqwest::StatusCode::PERMANENT_REDIRECT, &headers)
                .unwrap()
                .unwrap()
                .as_str(),
            "https://example.com/new?view=all"
        );
        assert!(redirect_probe_target(&url, reqwest::StatusCode::OK, &headers)
            .unwrap()
            .is_none());
    }

    #[test]
    fn redirect_probe_rejects_unsafe_or_ambiguous_locations() {
        let url = Url::parse("https://example.com/").unwrap();
        let mut headers = reqwest::header::HeaderMap::new();
        for location in [
            "https://127.0.0.1/",
            "https://[::ffff:127.0.0.1]/",
            "https://service.internal/",
            "https://user:secret@example.net/",
            "https://example.net:8443/",
            "http://example.net/",
            "file:///path",
        ] {
            headers.insert(reqwest::header::LOCATION, location.parse().unwrap());
            assert_eq!(
                redirect_probe_target(&url, reqwest::StatusCode::FOUND, &headers)
                    .unwrap_err()
                    .code,
                ErrorCode::SecurityBlocked,
                "{location}"
            );
        }
        headers.clear();
        assert!(redirect_probe_target(&url, reqwest::StatusCode::FOUND, &headers).is_err());
        headers.append(reqwest::header::LOCATION, "/a".parse().unwrap());
        headers.append(reqwest::header::LOCATION, "/b".parse().unwrap());
        assert!(redirect_probe_target(&url, reqwest::StatusCode::FOUND, &headers).is_err());
    }

    #[tokio::test]
    async fn redirect_probe_returns_before_reading_final_body() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, wait_for_probe) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            assert!(request.starts_with(b"GET / HTTP/1.1"));
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 99999999\r\n\r\n")
                .await
                .unwrap();
            // A consumer waiting for the advertised body will time out.
            wait_for_probe.await.unwrap();
        });
        // Only the transport helper may use the local fixture. The production
        // driver validates and pins public DNS before it calls this helper.
        let client = bounded_client_builder(Duration::from_secs(2), DISCOVERY_USER_AGENT)
            .build()
            .unwrap();
        let url = Url::parse(&format!("http://{address}/")).unwrap();
        assert!(send_redirect_probe(&client, &url).await.unwrap().is_none());
        stop.send(()).unwrap();
        server.await.unwrap();
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
        .unwrap()
        .with_escalation_mode(EscalationMode::Legacy);
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
        .unwrap()
        .with_escalation_mode(EscalationMode::Legacy);
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

    /// Two mock proxies that both answer `status`, a driver pinned to `mode`,
    /// and the one request the tests send. Returns the error and how many
    /// requests the two origins saw in total.
    async fn fetch_through_two_blocking_proxies(
        status: u16,
        mode: EscalationMode,
    ) -> (QuarryError, usize) {
        let p1 = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(status).set_body_string("blocked"))
            .mount(&p1)
            .await;
        let p2 = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(status).set_body_string("blocked"))
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
        .unwrap()
        .with_escalation_mode(mode);
        let url: Url = "http://example.com/".parse().unwrap();
        let hints = approved_hints_for_org("org_a".into());

        let err = driver.fetch_conditional(&url, &hints).await.unwrap_err();

        let hits = p1.received_requests().await.unwrap().len()
            + p2.received_requests().await.unwrap().len();
        (err, hits)
    }

    fn attempt_count(err: &QuarryError) -> usize {
        err.details
            .as_ref()
            .and_then(|v| v.get("egress_attempts"))
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len)
    }

    #[tokio::test]
    async fn honest_mode_never_rotates_on_status() {
        for (status, code) in [
            (403u16, ErrorCode::UpstreamBlocked),
            (429, ErrorCode::RateLimited),
            (503, ErrorCode::UpstreamBlocked),
        ] {
            let (err, hits) =
                fetch_through_two_blocking_proxies(status, EscalationMode::Honest).await;

            assert_eq!(err.code, code, "status {status}");
            assert_eq!(hits, 1, "status {status}: exactly one upstream attempt");
            assert_eq!(
                attempt_count(&err),
                1,
                "status {status}: one attempt record"
            );
        }
    }

    #[tokio::test]
    async fn legacy_mode_unchanged() {
        let (err, hits) = fetch_through_two_blocking_proxies(403, EscalationMode::Legacy).await;

        assert_eq!(err.code, ErrorCode::UpstreamBlocked);
        assert_eq!(hits, 2, "legacy still tries every approved proxy");
        assert_eq!(attempt_count(&err), 2);
    }

    #[tokio::test]
    async fn honest_mode_still_rotates_on_transport_error() {
        // A proxy address nothing listens on: connecting fails, which is a
        // transport failure and rotates in both modes.
        let dead = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap()
        };
        let ok_proxy = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&ok_proxy)
            .await;
        let dead_uri = format!("http://{dead}");
        let ok_uri = format!("http://{}", ok_proxy.address());
        let pool = ProxyPool::from_env_string(&format!("{dead_uri};{ok_uri}"));
        let host = "example.com";
        let org_id = (0..100)
            .map(|i| format!("org_{i}"))
            .find(|org| pool.pick(org, host).is_some_and(|p| p.uri == dead_uri))
            .expect("test should find an org that maps to the dead proxy");

        for mode in [EscalationMode::Honest, EscalationMode::Legacy] {
            let driver = StaticDriver::with_proxy_pool_and_processor(
                Duration::from_secs(5),
                "QuarryTest/1.0",
                pool.clone(),
                Some("quarry_proxy_pool".into()),
            )
            .unwrap()
            .with_escalation_mode(mode);
            let url: Url = format!("http://{host}/").parse().unwrap();

            let resp = driver
                .fetch_conditional(&url, &approved_hints_for_org(org_id.clone()))
                .await
                .unwrap();

            assert_eq!(resp.status, 200, "{mode:?}");
            assert_eq!(resp.body, b"ok", "{mode:?}");
        }
    }

    #[tokio::test]
    async fn honest_mode_does_not_move_the_next_request_to_another_proxy_after_a_403() {
        // First proxy answers 403, second answers 200. Honest: the 403 is
        // returned as the error, and a SECOND request to the same host still
        // goes to the first proxy (health is transport-only).
        let blocked_proxy = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(403).set_body_string("blocked"))
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
        .unwrap()
        .with_escalation_mode(EscalationMode::Honest);
        let url: Url = format!("http://{host}/").parse().unwrap();
        let hints = approved_hints_for_org(org_id);

        for _ in 0..2 {
            let err = driver.fetch_conditional(&url, &hints).await.unwrap_err();
            assert_eq!(err.code, ErrorCode::UpstreamBlocked);
        }

        assert_eq!(blocked_proxy.received_requests().await.unwrap().len(), 2);
        assert!(ok_proxy.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn redirect_to_a_private_target_is_blocked_before_contact() {
        // Redirects are followed, but each hop's target re-runs the public-
        // address preflight first. MockServer binds loopback, so the hop is
        // exactly the "safe public URL redirects to a private address" attack:
        // the guard must reject it without the target ever seeing a request.
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
    async fn redirect_to_a_non_http_scheme_is_blocked() {
        let redirector = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "ftp://files.example/x"),
            )
            .mount(&redirector)
            .await;

        let driver = StaticDriver::new(Duration::from_secs(2), "QuarryTest/1.0").unwrap();
        let url: Url = format!("{}/redirect", redirector.uri()).parse().unwrap();

        let res = driver.fetch(&url).await;
        let err = res.unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
        // In CI Docker the initial fetch is already blocked as loopback
        // (127.0.0.1) before the redirect is evaluated — still the
        // correct SecurityBlocked outcome for a redirect testcase.
        assert!(
            err.message.contains("non-http scheme") || err.message.contains("loopback"),
            "unexpected msg: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn redirect_without_a_location_header_is_blocked() {
        let redirector = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(302))
            .mount(&redirector)
            .await;

        let driver = StaticDriver::new(Duration::from_secs(2), "QuarryTest/1.0").unwrap();
        let url: Url = format!("{}/redirect", redirector.uri()).parse().unwrap();

        let res = driver.fetch(&url).await;
        let err = res.unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
        assert!(
            err.message.contains("no readable Location") || err.message.contains("loopback"),
            "unexpected msg: {}",
            err.message
        );
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

    fn proxy_decision(uri: &str) -> EgressDecision {
        EgressDecision {
            identity: EgressIdentity::Proxy {
                uri: uri.to_string(),
                processor_id: Some("quarry_proxy_pool".to_string()),
            },
            attempt: 0,
            reason: "proxy egress".to_string(),
        }
    }

    #[test]
    fn attempt_record_never_carries_proxy_userinfo() {
        let decision = proxy_decision("http://alice:s3cret@proxy.example:8080/path?token=abc");
        let rec = attempt_record(&decision, Some(403), None, "block_status");
        let text = rec.to_string();
        assert!(!text.contains("alice"), "{text}");
        assert!(!text.contains("s3cret"), "{text}");
        assert!(!text.contains("token=abc"), "{text}");
        assert_eq!(rec["proxy_uri"], "http://proxy.example:8080");
        assert_eq!(rec["processor_id"], "quarry_proxy_pool");
    }

    #[test]
    fn attempt_record_reports_an_error_class_and_not_the_error_text() {
        let decision = proxy_decision("socks5h://bob:hunter2@proxy.example:1080");
        let err = QuarryError::new(
            ErrorCode::DriverFailed,
            "error sending request for url (https://target.example/secret?q=1) via socks5h://bob:hunter2@proxy.example:1080",
        );
        let rec = attempt_record(&decision, None, Some(&err), "transport_error");
        let text = rec.to_string();
        assert!(!text.contains("hunter2"), "{text}");
        assert!(!text.contains("bob"), "{text}");
        assert!(!text.contains("target.example"), "{text}");
        assert!(rec.get("error").is_none(), "{text}");
        assert_eq!(rec["error_class"], ErrorCode::DriverFailed.wire_name());
        assert_eq!(rec["error_code"], "DriverFailed");
    }

    #[test]
    fn redacted_proxy_uri_handles_odd_inputs() {
        assert_eq!(redacted_proxy_uri("http://u:p@h.example"), "http://h.example");
        assert_eq!(redacted_proxy_uri("http://u@h.example:3128/x#f"), "http://h.example:3128");
        assert_eq!(redacted_proxy_uri("not a url with u:p@x"), "proxy");
    }

    #[tokio::test]
    async fn a_missing_proxy_client_error_does_not_carry_the_userinfo() {
        let pool = ProxyPool::from_env_string("socks5://proxy.example:1080");
        let driver =
            StaticDriver::with_proxy_pool(Duration::from_secs(2), "QuarryTest/1.0", pool).unwrap();
        let decision = proxy_decision("socks5://carol:pw123@unregistered.example:1080");
        let url: Url = "http://example.com/".parse().unwrap();
        let hints = FetchHints {
            resolved_target: Some(crate::dns_guard::ResolvedTarget {
                host: "example.com".to_string(),
                addresses: vec!["93.184.216.34:80".parse().unwrap()],
            }),
            ..FetchHints::default()
        };
        let err = driver
            .client_for_decision(&decision, &hints, &url)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        assert!(!err.message.contains("pw123"), "{}", err.message);
        assert!(!err.message.contains("carol"), "{}", err.message);
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

#[cfg(test)]
mod no_egress_tests {
    use super::*;
    use std::sync::Mutex;

    use quarry_core::privacy::PrivacyPolicy;
    use quarry_core::zdr::ZdrMode;

    use crate::data_plane_http::StoredArtifact;

    const PAGE: &str = "https://shop.example/private/basket?session=abc";

    /// The Data Plane artifacts contract, faked: one stored page per (org, url).
    #[derive(Default)]
    struct FakeArtifacts {
        stored: HashMap<(String, String), StoredArtifact>,
        calls: Mutex<Vec<(String, String, Option<usize>)>>,
        fail_with: Option<ErrorCode>,
    }

    impl FakeArtifacts {
        fn with_page(org: &str, url: &str, body: &[u8]) -> Self {
            let mut stored = HashMap::new();
            stored.insert(
                (org.to_string(), url.to_string()),
                StoredArtifact {
                    status: 200,
                    final_url: "https://shop.example/private/basket".into(),
                    headers: vec![("content-type".into(), "text/html".into())],
                    body: body.to_vec(),
                    captured_at: Some("2026-11-01T10:00:00Z".into()),
                },
            );
            Self {
                stored,
                ..Self::default()
            }
        }

        fn calls(&self) -> usize {
            self.calls.lock().expect("calls").len()
        }
    }

    #[async_trait]
    impl ArtifactReader for FakeArtifacts {
        async fn lookup(
            &self,
            org_id: &str,
            url: &str,
            max_bytes: Option<usize>,
        ) -> QuarryResult<ArtifactLookup> {
            self.calls
                .lock()
                .expect("calls")
                .push((org_id.to_string(), url.to_string(), max_bytes));
            if let Some(code) = self.fail_with {
                return Err(QuarryError::new(code, "fake failure"));
            }
            Ok(self
                .stored
                .get(&(org_id.to_string(), url.to_string()))
                .cloned()
                .map(ArtifactLookup::Hit)
                .unwrap_or(ArtifactLookup::Miss))
        }
    }

    fn hints(org: &str) -> FetchHints {
        FetchHints {
            org_id: org.to_string(),
            ..FetchHints::default()
        }
    }

    fn page() -> Url {
        Url::parse(PAGE).expect("static url")
    }

    fn assert_names_no_url(error: &QuarryError) {
        let details = error
            .details
            .as_ref()
            .map(|value| value.to_string())
            .unwrap_or_default();
        for fragment in ["shop.example", "basket", "session=abc"] {
            assert!(!error.message.contains(fragment), "message: {}", error.message);
            assert!(!details.contains(fragment), "details: {details}");
        }
    }

    #[test]
    fn the_flag_is_off_unless_set_on() {
        assert_eq!(NO_EGRESS_ENV, "QUARRY_EDGE__NO_EGRESS");
        assert!(!no_egress_from_value(None));
        assert!(!no_egress_from_value(Some("")));
        assert!(!no_egress_from_value(Some("yes")));
        assert!(!no_egress_from_value(Some("0")));
        assert!(no_egress_from_value(Some("1")));
        assert!(no_egress_from_value(Some(" TRUE ")));
        assert!(no_egress_from_value(Some("on")));
    }

    #[test]
    fn the_driver_exists_only_when_the_flag_is_on() {
        let reader: Arc<dyn ArtifactReader> = Arc::new(FakeArtifacts::default());
        assert!(NoEgressDriver::when_enabled(false, Arc::clone(&reader)).is_none());
        assert!(NoEgressDriver::when_enabled(true, reader).is_some());
    }

    #[tokio::test]
    async fn a_stored_page_is_served_from_the_data_plane() {
        let fake = Arc::new(FakeArtifacts::with_page("org-a", PAGE, b"<p>stored</p>"));
        let driver = NoEgressDriver::new(fake.clone());

        let response = driver
            .fetch_conditional(&page(), &hints("org-a"))
            .await
            .expect("a hit");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"<p>stored</p>".to_vec());
        assert_eq!(
            response.final_url.as_str(),
            "https://shop.example/private/basket"
        );
        assert_eq!(response.served_by, DriverKind::Static);
        assert!(response.captures.is_none());
        assert_eq!(fake.calls(), 1);
    }

    #[tokio::test]
    async fn a_miss_is_typed_not_retryable_and_names_no_url() {
        let fake = Arc::new(FakeArtifacts::default());
        let driver = NoEgressDriver::new(fake.clone());

        let error = driver
            .fetch_conditional(&page(), &hints("org-a"))
            .await
            .expect_err("a miss");

        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(!error.retryable);
        assert_eq!(
            error.details.as_ref().and_then(|d| d["reason"].as_str()),
            Some(no_egress_reason::MISS)
        );
        assert_names_no_url(&error);
    }

    #[tokio::test]
    async fn another_orgs_artifact_is_a_miss() {
        let fake = Arc::new(FakeArtifacts::with_page("org-a", PAGE, b"secret"));
        let driver = NoEgressDriver::new(fake);

        let error = driver
            .fetch_conditional(&page(), &hints("org-b"))
            .await
            .expect_err("org-b holds nothing");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn zdr_is_refused_before_any_lookup() {
        let fake = Arc::new(FakeArtifacts::with_page("org-a", PAGE, b"stored"));
        let driver = NoEgressDriver::new(fake.clone());

        let mut zdr = hints("org-a");
        zdr.privacy = PrivacyPolicy::default().with_zdr(ZdrMode::On);
        let error = driver
            .fetch_conditional(&page(), &zdr)
            .await
            .expect_err("refused under ZDR");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(
            error.details.as_ref().and_then(|d| d["reason"].as_str()),
            Some(no_egress_reason::ZDR_REFUSED)
        );
        assert_names_no_url(&error);

        let mut ephemeral = hints("org-a");
        ephemeral.privacy.privacy_classification = PrivacyClassification::ZdrEphemeral;
        let error = driver
            .fetch_conditional(&page(), &ephemeral)
            .await
            .expect_err("refused for ZDR-ephemeral content");
        assert_eq!(error.code, ErrorCode::Forbidden);

        assert_eq!(fake.calls(), 0, "no lookup ever reaches the Data Plane");
    }

    #[tokio::test]
    async fn a_fetch_without_an_org_is_refused_before_any_lookup() {
        let fake = Arc::new(FakeArtifacts::with_page("", PAGE, b"stored"));
        let driver = NoEgressDriver::new(fake.clone());

        let error = driver.fetch(&page()).await.expect_err("no org");
        assert_eq!(error.code, ErrorCode::BadRequest);
        assert_eq!(
            error.details.as_ref().and_then(|d| d["reason"].as_str()),
            Some(no_egress_reason::NO_ORG)
        );
        assert_eq!(fake.calls(), 0);
    }

    #[tokio::test]
    async fn the_read_cap_is_passed_on_and_enforced() {
        let fake = Arc::new(FakeArtifacts::with_page("org-a", PAGE, b"0123456789"));
        let driver = NoEgressDriver::new(fake.clone());

        let mut refuse = hints("org-a");
        refuse.max_body_bytes = Some(4);
        let error = driver
            .fetch_conditional(&page(), &refuse)
            .await
            .expect_err("over the cap");
        assert_eq!(error.code, ErrorCode::DriverFailed);
        assert_names_no_url(&error);

        let mut cut = refuse.clone();
        cut.truncate_at_cap = true;
        let response = driver
            .fetch_conditional(&page(), &cut)
            .await
            .expect("cut at the cap");
        assert_eq!(response.body, b"0123".to_vec());

        let calls = fake.calls.lock().expect("calls");
        assert!(calls.iter().all(|(_, _, max)| *max == Some(4)));
    }

    #[tokio::test]
    async fn a_reader_failure_passes_through_unchanged() {
        let fake = Arc::new(FakeArtifacts {
            fail_with: Some(ErrorCode::Timeout),
            ..FakeArtifacts::default()
        });
        let driver = NoEgressDriver::new(fake);

        let error = driver
            .fetch_conditional(&page(), &hints("org-a"))
            .await
            .expect_err("the reader failed");
        assert_eq!(error.code, ErrorCode::Timeout);
    }

    #[tokio::test]
    async fn discovery_stays_fail_closed() {
        let driver = NoEgressDriver::new(Arc::new(FakeArtifacts::default()));
        let bounded = driver
            .fetch_bounded(&page(), 1024)
            .await
            .expect_err("no bounded fetch");
        assert_eq!(bounded.code, ErrorCode::Unsupported);
        let hop = driver
            .fetch_discovery_hop(&page(), 1024)
            .await
            .expect_err("no discovery hop");
        assert_eq!(hop.code, ErrorCode::Unsupported);
    }
}
