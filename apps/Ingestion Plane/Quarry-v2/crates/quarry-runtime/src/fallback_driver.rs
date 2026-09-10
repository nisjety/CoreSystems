//! Fallback driver — tries drivers in order until one succeeds.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use quarry_core::error::ErrorCode;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_tls::TlsProfile;
use tracing::{info, warn};
use url::Url;

use crate::driver::{Driver, FetchHints};
use crate::fetch::FetchResponse;

pub struct FallbackDriver {
    primary: DriverKind,
    chain: Vec<DriverKind>,
    drivers: HashMap<DriverKind, Arc<dyn Driver>>,
}

impl FallbackDriver {
    pub fn new(
        primary: DriverKind,
        chain: Vec<DriverKind>,
        drivers: HashMap<DriverKind, Arc<dyn Driver>>,
    ) -> Self {
        Self {
            primary,
            chain,
            drivers,
        }
    }

    pub fn from_plan(
        plan: &crate::driver_plan::DriverPlan,
        drivers: HashMap<DriverKind, Arc<dyn Driver>>,
    ) -> Self {
        Self {
            primary: plan.driver,
            chain: plan.fallback_chain.clone(),
            drivers,
        }
    }

    fn attempt_order(&self) -> Vec<DriverKind> {
        let mut order = vec![self.primary];
        for k in &self.chain {
            if !order.contains(k) {
                order.push(*k);
            }
        }
        order
    }

    async fn fetch_inner(
        &self,
        url: &Url,
        hints: Option<&FetchHints>,
    ) -> QuarryResult<FetchResponse> {
        let order = self.attempt_order();
        // Aggregate per-driver attempts so when the entire chain fails the
        // operator sees `[static: timeout (5s), tls: ja3-mismatch, browser:
        // 502]` instead of a single mystery error from the last attempt.
        let mut attempts: Vec<(DriverKind, String)> = Vec::with_capacity(order.len());
        let mut last_err: Option<quarry_core::QuarryError> = None;
        // 2D — on a block STATUS (403/429/503/…), fall through to the next
        // driver (a different TLS fingerprint / transport) instead of
        // returning the block. Bounded naturally by the chain length.
        let mut best_block: Option<FetchResponse> = None;
        // JS-shell rescue: keep the best (most readable text) shell response
        // we saw from a non-browser driver. If the browser then fails, or
        // renders nothing better than the shell, we return this instead of
        // erroring out / returning an emptier body — the page still has a
        // `<title>`, links and metadata worth extracting. Previously a
        // browser failure after a shell detection surfaced as a hard error
        // and a silent empty render was returned as-is, which is exactly
        // how aquatiq.com produced snippets with no text.
        let mut best_shell: Option<FetchResponse> = None;

        for (idx, kind) in order.iter().enumerate() {
            let driver = match self.drivers.get(kind) {
                Some(d) => d,
                None => {
                    attempts.push((*kind, "driver not registered".into()));
                    continue;
                }
            };
            let result = match hints {
                Some(hints) => driver.fetch_conditional(url, hints).await,
                None => driver.fetch(url).await,
            };
            match result {
                Ok(resp) => {
                    if crate::fingerprint_rotation::is_block_status(resp.status)
                        && idx < order.len() - 1
                    {
                        attempts.push((*kind, format!("blocked: HTTP {}", resp.status)));
                        warn!(
                            driver = ?kind,
                            status = resp.status,
                            next = ?order.get(idx + 1),
                            "block status — rotating fingerprint/driver"
                        );
                        best_block = Some(resp);
                        continue;
                    }
                    // Firecrawl parity: detect JS-shell pages (Next.js etc) that
                    // need browser rendering. Static fetch returns 200 with shell
                    // HTML (__NEXT_DATA__) but readability yields <1k chars of
                    // real content. Fall through to browser for full render.
                    if is_js_shell_needing_browser(&resp.body, resp.status)
                        && idx < order.len() - 1
                        && *kind != DriverKind::Browser
                    {
                        // Find next browser driver in chain, skip Tls if it's also static
                        let has_browser_ahead = order[idx + 1..].contains(&DriverKind::Browser);
                        if has_browser_ahead {
                            attempts.push((*kind, "js-shell detected — needs browser rendering".into()));
                            warn!(
                                driver = ?kind,
                                status = resp.status,
                                body_bytes = resp.body.len(),
                                text_chars = visible_text_len(&resp.body),
                                next = ?order.get(idx + 1),
                                "JS shell detected — falling back to browser"
                            );
                            let better = best_shell
                                .as_ref()
                                .map(|b| visible_text_len(&resp.body) > visible_text_len(&b.body))
                                .unwrap_or(true);
                            if better {
                                best_shell = Some(resp);
                            }
                            continue;
                        }
                    }
                    // A non-browser fallback that came back with an
                    // unfollowed redirect (the TLS-profile driver never
                    // follows redirects: each hop needs its own SSRF/DNS
                    // preflight, and only the static driver does that) or
                    // with LESS readable text than the shell we already hold
                    // is not an improvement — keep rotating towards the
                    // browser instead of returning it. Observed live on
                    // aquatiq.com: static followed `aquatiq.com →
                    // www.aquatiq.com` and got the 220 KB Next.js shell, TLS
                    // returned a 308 with a 44-char "Redirecting" body, and
                    // that body became the page's whole excerpt.
                    if *kind != DriverKind::Browser && idx < order.len() - 1 {
                        let is_redirect = (300..400).contains(&resp.status);
                        let thinner = best_shell
                            .as_ref()
                            .map(|b| visible_text_len(&resp.body) <= visible_text_len(&b.body))
                            .unwrap_or(false);
                        if is_redirect || thinner {
                            attempts.push((
                                *kind,
                                if is_redirect {
                                    format!("unfollowed redirect: HTTP {}", resp.status)
                                } else {
                                    "no more readable text than the JS shell".to_string()
                                },
                            ));
                            warn!(
                                driver = ?kind,
                                status = resp.status,
                                body_bytes = resp.body.len(),
                                text_chars = visible_text_len(&resp.body),
                                next = ?order.get(idx + 1),
                                "fallback response is not an improvement — rotating to next driver"
                            );
                            if best_shell.is_none() && !is_redirect {
                                best_shell = Some(resp);
                            }
                            continue;
                        }
                    }
                    if *kind == DriverKind::Browser {
                        if let Some(shell) = best_shell.take() {
                            let rendered = visible_text_len(&resp.body);
                            let shell_text = visible_text_len(&shell.body);
                            if rendered <= shell_text {
                                warn!(
                                    url = %url,
                                    rendered_text_chars = rendered,
                                    shell_text_chars = shell_text,
                                    rendered_bytes = resp.body.len(),
                                    "browser fallback rendered no more readable text than the static shell; keeping the static response"
                                );
                                return Ok(shell);
                            }
                            info!(
                                url = %url,
                                served_by = ?resp.served_by,
                                rendered_text_chars = rendered,
                                shell_text_chars = shell_text,
                                rendered_bytes = resp.body.len(),
                                "browser fallback rendered the JS shell"
                            );
                        }
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    let is_retryable = is_retryable_driver_error(e.code);
                    attempts.push((
                        *kind,
                        format!("{}: {}", error_code_short(e.code), e.message),
                    ));
                    if is_retryable && idx < order.len() - 1 {
                        warn!(
                            driver = ?kind,
                            next = ?order.get(idx + 1),
                            error = %e,
                            "driver failed, falling back"
                        );
                        last_err = Some(e);
                        continue;
                    }
                    if let Some(shell) = best_shell.take() {
                        warn!(
                            driver = ?kind,
                            error = %e,
                            url = %url,
                            shell_text_chars = visible_text_len(&shell.body),
                            "browser fallback failed after JS-shell detection; returning the static shell response"
                        );
                        return Ok(shell);
                    }
                    return Err(e.with_details(serde_json::json!({
                        "fallback_attempts": attempts
                            .iter()
                            .map(|(k, msg)| serde_json::json!({"driver": format!("{k:?}"), "error": msg}))
                            .collect::<Vec<_>>(),
                    })));
                }
            }
        }

        // All drivers rotated through; if every one was blocked, surface the
        // last block response (the caller's scheduler reads the status) rather
        // than a generic error.
        if let Some(resp) = best_block {
            return Ok(resp);
        }
        if let Some(shell) = best_shell {
            warn!(
                url = %url,
                attempts = ?attempts,
                "no driver improved on the JS shell; returning the static shell response"
            );
            return Ok(shell);
        }

        let aggregate_msg = attempts
            .iter()
            .map(|(k, e)| format!("{k:?}={e}"))
            .collect::<Vec<_>>()
            .join("; ");

        Err(last_err
            .map(|e| {
                e.with_details(serde_json::json!({
                    "fallback_attempts": attempts
                        .iter()
                        .map(|(k, msg)| serde_json::json!({"driver": format!("{k:?}"), "error": msg}))
                        .collect::<Vec<_>>(),
                }))
            })
            .unwrap_or_else(|| {
                quarry_core::QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("no drivers succeeded in fallback chain: {aggregate_msg}"),
                )
            }))
    }
}

#[async_trait]
impl Driver for FallbackDriver {
    fn kind(&self) -> DriverKind {
        self.primary
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.fetch_inner(url, None).await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        self.fetch_inner(url, Some(hints)).await
    }

    fn tls_profile(&self) -> Option<TlsProfile> {
        self.drivers
            .get(&self.primary)
            .and_then(|d: &Arc<dyn Driver>| d.tls_profile())
    }
}

/// Above this many visible text characters a page is NOT a content-less
/// shell, however many hydration markers and scripts it carries.
pub(crate) const SHELL_MAX_TEXT_CHARS: usize = 2_000;

/// Detect JS-shell pages (Next.js, SPA) that need browser rendering.
/// Static fetch returns 200 with shell HTML containing __NEXT_DATA__ but
/// readability yields truncated content. Trigger browser fallback.
pub(crate) fn is_js_shell_needing_browser(body: &[u8], status: u16) -> bool {
    if status != 200 {
        return false;
    }
    // Fast path: check for Next.js / SPA markers without full parse
    // 16KB shell is typical; 97808 for aquatiq. Small bodies are not JS shells.
    if body.len() < 8000 {
        return false;
    }
    // Look for JS framework shell markers
    let markers: &[&[u8]] = &[
        b"__NEXT_DATA__",
        b"data-next-head",
        b"_next/static",
        b"__NUXT__",
        b"id=\"__next\"",
        b"id=\"root\"", // generic SPA root
    ];
    let is_shell = markers.iter().any(|m| {
        // Simple substring search
        body.windows(m.len()).any(|w| w == *m)
    });
    if !is_shell {
        return false;
    }
    // Confirm shell by checking high script-to-content ratio:
    // Shells have huge <script> blocks and tiny visible text.
    // Heuristic: if body has >3 script tags and >20000 bytes, it's a shell
    let script_count = body.windows(b"<script".len()).filter(|w| w == b"<script").count();
    if script_count < 3 || body.len() <= 20000 {
        return false;
    }
    // The premise of every check above is that the shell carries almost no
    // readable content. Plenty of framework sites — aquatiq.com among them —
    // server-render the whole page AND ship the hydration bundle, so they
    // trip every marker while already exposing thousands of characters of
    // real text. Escalating those costs a browser session plus a hydration
    // settle per page and returns nothing extra (the live logs said so:
    // "browser fallback rendered no more readable text than the static
    // shell"), which pushed a 4-page crawl past the onboarding preview's
    // event-poll ceiling so only the seed page ever reached the wizard.
    // Only a genuinely content-less shell needs a browser.
    visible_text_len(body) < SHELL_MAX_TEXT_CHARS
}

/// Rough count of user-visible text characters in an HTML body: everything
/// outside `<script>`, `<style>`, `<noscript>`, `<template>` blocks and
/// outside tags, whitespace excluded. Cheap (single byte pass, no DOM) so it
/// can run on every fallback decision and every hydration poll.
pub(crate) fn visible_text_len(body: &[u8]) -> usize {
    const SKIP: &[&[u8]] = &[b"script", b"style", b"noscript", b"template", b"svg"];
    let mut i = 0;
    let mut count = 0usize;
    let n = body.len();
    while i < n {
        if body[i] == b'<' {
            // Comment
            if body[i..].starts_with(b"<!--") {
                match find(body, i + 4, b"-->") {
                    Some(end) => {
                        i = end + 3;
                        continue;
                    }
                    None => break,
                }
            }
            // Tag name
            let mut j = i + 1;
            while j < n && body[j] == b'/' {
                j += 1;
            }
            let name_start = j;
            while j < n && body[j].is_ascii_alphanumeric() {
                j += 1;
            }
            let name = &body[name_start..j];
            let is_open = body.get(i + 1) != Some(&b'/');
            let close = match find(body, j, b">") {
                Some(c) => c,
                None => break,
            };
            i = close + 1;
            if is_open && !name.is_empty() {
                let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
                if SKIP.iter().any(|s| *s == lower.as_slice()) {
                    // Skip to the matching close tag (no nesting for these).
                    let mut needle = b"</".to_vec();
                    needle.extend_from_slice(&lower);
                    match find_ci(body, i, &needle) {
                        Some(end) => match find(body, end, b">") {
                            Some(c) => i = c + 1,
                            None => break,
                        },
                        None => break,
                    }
                }
            }
            continue;
        }
        if !body[i].is_ascii_whitespace() {
            count += 1;
        }
        i += 1;
    }
    count
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn find_ci(hay: &[u8], from: usize, needle_lower: &[u8]) -> Option<usize> {
    if from >= hay.len() || needle_lower.is_empty() {
        return None;
    }
    hay[from..]
        .windows(needle_lower.len())
        .position(|w| w.iter().zip(needle_lower).all(|(a, b)| a.to_ascii_lowercase() == *b))
        .map(|p| p + from)
}

pub(crate) fn is_retryable_driver_error(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::Timeout | ErrorCode::UpstreamBlocked | ErrorCode::DriverFailed
    )
}

pub(crate) fn error_code_short(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::BadRequest => "bad_request",
        ErrorCode::Unauthorized => "unauthorized",
        ErrorCode::Forbidden => "forbidden",
        ErrorCode::NotFound => "not_found",
        ErrorCode::Conflict => "conflict",
        ErrorCode::RateLimited => "rate_limited",
        ErrorCode::Timeout => "timeout",
        ErrorCode::SecurityBlocked => "security_blocked",
        ErrorCode::ActionUnknown => "action_unknown",
        ErrorCode::ChallengeDetected => "challenge_detected",
        ErrorCode::RuntimeNotReady => "runtime_not_ready",
        ErrorCode::CheckpointLost => "checkpoint_lost",
        ErrorCode::TargetRepairRequired => "target_repair_required",
        ErrorCode::DriverFailed => "driver_failed",
        ErrorCode::UpstreamBlocked => "upstream_blocked",
        ErrorCode::Unsupported => "unsupported",
        ErrorCode::Internal => "internal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::QuarryError;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct OkDriver {
        kind: DriverKind,
        calls: AtomicU32,
    }

    impl OkDriver {
        fn new(kind: DriverKind) -> Self {
            Self {
                kind,
                calls: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl Driver for OkDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }

        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![],
                body: b"ok".to_vec(),
                duration_ms: 1,
                served_by: self.kind,
            })
        }
    }

    struct FailDriver {
        kind: DriverKind,
        calls: AtomicU32,
    }

    impl FailDriver {
        fn new(kind: DriverKind) -> Self {
            Self {
                kind,
                calls: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl Driver for FailDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }

        async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "simulated failure",
            ))
        }
    }

    struct BlockDriver {
        kind: DriverKind,
        calls: AtomicU32,
    }

    impl BlockDriver {
        fn new(kind: DriverKind) -> Self {
            Self {
                kind,
                calls: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl Driver for BlockDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(FetchResponse {
                status: 403,
                final_url: url.clone(),
                headers: vec![],
                body: b"blocked".to_vec(),
                duration_ms: 1,
                served_by: self.kind,
            })
        }
    }

    #[tokio::test]
    async fn block_status_rotates_to_next_fingerprint() {
        // primary returns a 403 block → fall through to the next (different
        // fingerprint) driver, which succeeds.
        let primary = Arc::new(BlockDriver::new(DriverKind::Static));
        let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());
        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.served_by, DriverKind::Tls);
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn all_blocked_returns_last_block_response() {
        let primary = Arc::new(BlockDriver::new(DriverKind::Static));
        let fallback = Arc::new(BlockDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());
        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status, 403); // exhausted rotation → surface the block
        assert_eq!(resp.served_by, DriverKind::Tls); // …attributed to the driver that produced it
    }

    #[tokio::test]
    async fn primary_success_skips_fallback() {
        let primary = Arc::new(OkDriver::new(DriverKind::Static));
        let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());

        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let url: Url = "https://example.com".parse().unwrap();
        let resp = fb.fetch(&url).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.served_by, DriverKind::Static);
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn falls_back_on_primary_failure() {
        let primary = Arc::new(FailDriver::new(DriverKind::Static));
        let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());

        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let url: Url = "https://example.com".parse().unwrap();
        let resp = fb.fetch(&url).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn rotated_fetch_reports_the_serving_drivers_kind_not_the_primarys() {
        // The wrapper's kind() intentionally reports the planned primary;
        // the RESPONSE must carry the driver that actually served. This is
        // what run output (DriverInfo.kind, meta.json) is stamped from — a
        // Static-primary run served by Browser must not claim "Static".
        let primary = Arc::new(FailDriver::new(DriverKind::Static));
        let fallback = Arc::new(OkDriver::new(DriverKind::Browser));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Browser, fallback.clone());

        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Browser], drivers);
        let url: Url = "https://example.com".parse().unwrap();
        let resp = fb.fetch(&url).await.unwrap();

        assert_eq!(fb.kind(), DriverKind::Static);
        assert_eq!(resp.served_by, DriverKind::Browser);
    }

    #[tokio::test]
    async fn all_fail_returns_last_error() {
        let primary = Arc::new(FailDriver::new(DriverKind::Static));
        let fallback = Arc::new(FailDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());

        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let url: Url = "https://example.com".parse().unwrap();
        let err = fb.fetch(&url).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
    }

    #[tokio::test]
    async fn all_fail_aggregates_per_driver_errors_in_details() {
        let primary = Arc::new(FailDriver::new(DriverKind::Static));
        let mid = Arc::new(FailDriver::new(DriverKind::Tls));
        let last = Arc::new(FailDriver::new(DriverKind::Browser));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, mid.clone());
        drivers.insert(DriverKind::Browser, last.clone());

        let fb = FallbackDriver::new(
            DriverKind::Static,
            vec![DriverKind::Tls, DriverKind::Browser],
            drivers,
        );
        let url: Url = "https://example.com".parse().unwrap();
        let err = fb.fetch(&url).await.unwrap_err();
        let details = err.details.as_ref().expect("details should be set");
        let attempts = details
            .get("fallback_attempts")
            .and_then(|v| v.as_array())
            .expect("fallback_attempts array");
        assert_eq!(attempts.len(), 3, "all three drivers should have entries");

        // Each driver tried.
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(mid.calls.load(Ordering::Relaxed), 1);
        assert_eq!(last.calls.load(Ordering::Relaxed), 1);
    }

    /// Big enough (>20 KB, ≥3 scripts, Next marker) to trip the JS-shell
    /// heuristic, with almost no visible text.
    fn shell_body() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"<html><head><title>aquatiq.com</title></head><body><div id=\"__next\"></div>");
        for _ in 0..3 {
            b.extend_from_slice(b"<script>");
            b.extend_from_slice(&vec![b'x'; 8_000]);
            b.extend_from_slice(b"</script>");
        }
        b.extend_from_slice(b"<script id=\"__NEXT_DATA__\">{}</script></body></html>");
        b
    }

    struct BodyDriver {
        kind: DriverKind,
        body: Vec<u8>,
        calls: AtomicU32,
    }

    #[async_trait]
    impl Driver for BodyDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![],
                body: self.body.clone(),
                duration_ms: 1,
                served_by: self.kind,
            })
        }
    }

    /// Regression: a server-rendered page that also ships a hydration
    /// bundle must NOT be escalated to the browser. Live aquatiq.com is
    /// 219 KB with `__NEXT_DATA__`, dozens of scripts and ~4.8k characters
    /// of real text; the browser round trip returned no more text than the
    /// static body and cost seconds per page.
    #[test]
    fn server_rendered_pages_with_real_text_are_not_treated_as_shells() {
        let mut body = shell_body();
        body.extend_from_slice(
            "<main><h1>Hygiene for matindustrien</h1><p>Vi leverer kompetanse og systemer. </p></main>"
                .repeat(60)
                .as_bytes(),
        );
        assert!(visible_text_len(&body) > SHELL_MAX_TEXT_CHARS);
        assert!(
            !is_js_shell_needing_browser(&body, 200),
            "a page with real server-rendered text must not need the browser"
        );
        // The content-less shell still escalates.
        assert!(is_js_shell_needing_browser(&shell_body(), 200));
    }

    #[test]
    fn visible_text_len_ignores_scripts_styles_and_tags() {
        let html = b"<html><head><style>.a{color:red}</style><script>var x = 'lots of code';</script></head><body><h1>Hei  verden</h1><!-- c --><p>tekst <b>her</b></p><noscript>nei</noscript></body></html>";
        // "Heiverden" (9) + "tekst" (5) + "her" (3) = 17
        assert_eq!(visible_text_len(html), 17);
        assert!(visible_text_len(&shell_body()) < 40);
    }

    #[tokio::test]
    async fn shell_detection_then_browser_failure_returns_the_shell_not_an_error() {
        let static_shell = Arc::new(BodyDriver {
            kind: DriverKind::Static,
            body: shell_body(),
            calls: AtomicU32::new(0),
        });
        let browser = Arc::new(FailDriver::new(DriverKind::Browser));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, static_shell.clone());
        drivers.insert(DriverKind::Browser, browser.clone());
        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Browser], drivers);
        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .expect("shell is returned instead of the browser error");
        assert_eq!(resp.served_by, DriverKind::Static);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn browser_render_that_adds_no_text_yields_to_the_shell() {
        let static_shell = Arc::new(BodyDriver {
            kind: DriverKind::Static,
            body: shell_body(),
            calls: AtomicU32::new(0),
        });
        // Browser "renders" an even emptier document.
        let browser = Arc::new(BodyDriver {
            kind: DriverKind::Browser,
            body: b"<html><body></body></html>".to_vec(),
            calls: AtomicU32::new(0),
        });
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, static_shell);
        drivers.insert(DriverKind::Browser, browser);
        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Browser], drivers);
        let resp = fb.fetch(&"https://example.com".parse().unwrap()).await.unwrap();
        assert_eq!(resp.served_by, DriverKind::Static);
        assert!(resp.body.len() > 20_000);
    }

    #[tokio::test]
    async fn browser_render_with_real_text_wins_over_the_shell() {
        let static_shell = Arc::new(BodyDriver {
            kind: DriverKind::Static,
            body: shell_body(),
            calls: AtomicU32::new(0),
        });
        let browser = Arc::new(BodyDriver {
            kind: DriverKind::Browser,
            body: "<html><body><main><h1>Hygiene for matindustrien</h1><p>Vi leverer hygieneløsninger til næringsmiddelindustrien.</p></main></body></html>".as_bytes().to_vec(),
            calls: AtomicU32::new(0),
        });
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, static_shell);
        drivers.insert(DriverKind::Browser, browser);
        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Browser], drivers);
        let resp = fb.fetch(&"https://example.com".parse().unwrap()).await.unwrap();
        assert_eq!(resp.served_by, DriverKind::Browser);
    }

    struct StatusDriver {
        kind: DriverKind,
        status: u16,
        body: Vec<u8>,
        calls: AtomicU32,
    }

    #[async_trait]
    impl Driver for StatusDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(FetchResponse {
                status: self.status,
                final_url: url.clone(),
                headers: vec![],
                body: self.body.clone(),
                duration_ms: 1,
                served_by: self.kind,
            })
        }
    }

    /// Live aquatiq.com shape: static follows the apex → www redirect and
    /// gets the Next.js shell; the TLS driver (no redirect following) hands
    /// back a 308 with a one-line body; the browser renders real text.
    #[tokio::test]
    async fn unfollowed_redirect_from_tls_does_not_beat_the_shell_or_stop_the_chain() {
        let static_shell = Arc::new(BodyDriver {
            kind: DriverKind::Static,
            body: shell_body(),
            calls: AtomicU32::new(0),
        });
        let tls_redirect = Arc::new(StatusDriver {
            kind: DriverKind::Tls,
            status: 308,
            body: b"Redirecting (308) The document has moved <a href=\"https://www.example.com/\">here</a>".to_vec(),
            calls: AtomicU32::new(0),
        });
        let browser = Arc::new(BodyDriver {
            kind: DriverKind::Browser,
            body: "<html><body><main><h1>Hygiene for matindustrien</h1><p>Vi leverer hygieneløsninger til næringsmiddelindustrien.</p></main></body></html>".as_bytes().to_vec(),
            calls: AtomicU32::new(0),
        });
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, static_shell);
        drivers.insert(DriverKind::Tls, tls_redirect.clone());
        drivers.insert(DriverKind::Browser, browser.clone());
        let fb = FallbackDriver::new(
            DriverKind::Static,
            vec![DriverKind::Tls, DriverKind::Browser],
            drivers,
        );
        let resp = fb.fetch(&"https://example.com".parse().unwrap()).await.unwrap();
        assert_eq!(tls_redirect.calls.load(Ordering::Relaxed), 1);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 1);
        assert_eq!(resp.served_by, DriverKind::Browser);
        assert_eq!(resp.status, 200);
    }

    /// Same chain, but the browser fails: the shell (title, links, metadata)
    /// wins over the redirect stub and over an error.
    #[tokio::test]
    async fn redirect_stub_then_browser_failure_returns_the_shell() {
        let static_shell = Arc::new(BodyDriver {
            kind: DriverKind::Static,
            body: shell_body(),
            calls: AtomicU32::new(0),
        });
        let tls_redirect = Arc::new(StatusDriver {
            kind: DriverKind::Tls,
            status: 308,
            body: b"Redirecting".to_vec(),
            calls: AtomicU32::new(0),
        });
        let browser = Arc::new(FailDriver::new(DriverKind::Browser));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, static_shell);
        drivers.insert(DriverKind::Tls, tls_redirect);
        drivers.insert(DriverKind::Browser, browser);
        let fb = FallbackDriver::new(
            DriverKind::Static,
            vec![DriverKind::Tls, DriverKind::Browser],
            drivers,
        );
        let resp = fb.fetch(&"https://example.com".parse().unwrap()).await.unwrap();
        assert_eq!(resp.served_by, DriverKind::Static);
        assert_eq!(resp.status, 200);
        assert!(resp.body.len() > 20_000);
    }

    /// A redirect from the LAST driver in the chain is still returned (the
    /// caller decides what a 3xx means); we only rotate when there is a next.
    #[tokio::test]
    async fn redirect_from_the_last_driver_is_returned_as_is() {
        let tls_redirect = Arc::new(StatusDriver {
            kind: DriverKind::Tls,
            status: 301,
            body: b"moved".to_vec(),
            calls: AtomicU32::new(0),
        });
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Tls, tls_redirect);
        let fb = FallbackDriver::new(DriverKind::Tls, vec![], drivers);
        let resp = fb.fetch(&"https://example.com".parse().unwrap()).await.unwrap();
        assert_eq!(resp.status, 301);
    }

    #[tokio::test]
    async fn missing_driver_skipped() {
        let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Tls, fallback.clone());

        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers);
        let url: Url = "https://example.com".parse().unwrap();
        let resp = fb.fetch(&url).await.unwrap();
        assert_eq!(resp.status, 200);
    }
}
