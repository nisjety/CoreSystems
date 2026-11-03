//! Fallback driver — tries drivers in order until one succeeds.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use quarry_core::contracts::ChallengeStrength;
use quarry_core::error::{driver_failed_reason, ErrorCode};
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_tls::TlsProfile;
use tracing::{info, warn};
use url::Url;

use crate::challenge::{classify, ChallengeInput, MAX_SCANNED_BYTES};
use crate::driver::{Driver, FetchHints};
use crate::escalation::{escalation_allowed, trigger_for_error, EscalationMode, EscalationTrigger};
use crate::fetch::FetchResponse;

pub struct FallbackDriver {
    primary: DriverKind,
    chain: Vec<DriverKind>,
    drivers: HashMap<DriverKind, Arc<dyn Driver>>,
    /// Whether a block status (or a block-class error) may move the request to
    /// the next driver (WS3.I). Non-block reasons (a JS shell, an unfollowed
    /// redirect, a transport failure) move it in both modes.
    mode: EscalationMode,
    /// Whether a waterfall that ends in DRIVER_FAILED names its reason (E3.8.1,
    /// `QUARRY_EDGE__FETCH_SUB_CODES`, default off). Off: the error is as the tiers left it.
    fetch_sub_codes: bool,
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
            mode: EscalationMode::current(),
            fetch_sub_codes: fetch_sub_codes_enabled(),
        }
    }

    /// Pins the escalation mode instead of reading the process default.
    pub fn with_escalation_mode(mut self, mode: EscalationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Pins whether a waterfall that ends in DRIVER_FAILED names its reason (E3.8.1),
    /// instead of reading `QUARRY_EDGE__FETCH_SUB_CODES`.
    pub fn with_fetch_sub_codes(mut self, enabled: bool) -> Self {
        self.fetch_sub_codes = enabled;
        self
    }

    /// Settles the reason of a DRIVER_FAILED that leaves this chain (E3.8.1). It is
    /// a no-op unless the flag is on.
    fn settle_chain_error(
        &self,
        error: quarry_core::QuarryError,
        tier_classes: &[Option<&'static str>],
        attempts: usize,
    ) -> quarry_core::QuarryError {
        if self.fetch_sub_codes {
            settle_driver_failed(error, tier_classes, attempts)
        } else {
            error
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
            mode: EscalationMode::current(),
            fetch_sub_codes: fetch_sub_codes_enabled(),
        }
    }

    /// A strong challenge classified on a body that answered 200 is not a page
    /// that withheld its content: it is an interstitial or a CAPTCHA widget, and
    /// a browser could run its script. Under `honest` that is a change of
    /// identity prompted by a challenge, which ADR-5 refuses
    /// ([`EscalationTrigger::StrongChallenge`]); the body is returned as it is
    /// and the scrape path classifies it. Under `legacy` the trigger is allowed
    /// and this is always false. The classifier is called directly (not through
    /// `classify_fetched`) so the invariant does not depend on
    /// `QUARRY_CHALLENGE_CLASSIFIER`. Only the strong class counts; a weak hint
    /// or a non-signal (a login page, a rate limit) is not a challenge.
    fn challenge_forbids_escalation(&self, url: &Url, resp: &FetchResponse) -> bool {
        if escalation_allowed(self.mode, EscalationTrigger::StrongChallenge) {
            return false;
        }
        let head = &resp.body[..resp.body.len().min(MAX_SCANNED_BYTES)];
        let html = String::from_utf8_lossy(head);
        classify(&ChallengeInput {
            url: url.as_str(),
            title: None,
            html: Some(&html),
            text_snippet: None,
            main_doc_status: Some(resp.status),
        })
        .is_some_and(|signal| signal.strength == ChallengeStrength::Strong)
    }

    /// True when this response is one `honest` refuses to change identity on:
    /// a block status, or a 200 that is a strong challenge. Always false under
    /// `legacy`.
    fn refuses_rotation(&self, url: &Url, resp: &FetchResponse) -> bool {
        (crate::fingerprint_rotation::is_block_status(resp.status)
            && !escalation_allowed(self.mode, EscalationTrigger::BlockStatus(resp.status)))
            || self.challenge_forbids_escalation(url, resp)
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
        // One entry per tier that failed with an error: the sub-code it named, if any.
        let mut tier_classes: Vec<Option<&'static str>> = Vec::with_capacity(order.len());
        let mut last_err: Option<quarry_core::QuarryError> = None;
        // 2D — on a block STATUS (403/429/503/…), fall through to the next
        // driver (a different TLS fingerprint / transport) instead of
        // returning the block. Bounded naturally by the chain length.
        // Only under `legacy`: `honest` never changes driver on a status
        // (WS3.I, ADR-5), the block is returned and the caller decides.
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
                        && escalation_allowed(
                            self.mode,
                            EscalationTrigger::BlockStatus(resp.status),
                        )
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
                    // Detect pages that answered 200 but withheld their
                    // content, and re-fetch them through a browser.
                    //
                    // `StructuredYield::Unknown`: the structured harvest
                    // (quarry-transform `structured.rs`) runs downstream of
                    // the driver, so at this point nobody has looked inside
                    // the embedded JSON yet. When the harvest result becomes
                    // available here, pass it — a `Found` suppresses the
                    // browser round trip entirely.
                    //
                    // Not for a challenge page: under `honest` a strong
                    // challenge never moves the request to another identity
                    // (see `challenge_forbids_escalation`).
                    if needs_browser_escalation(&resp.body, resp.status, StructuredYield::Unknown)
                        && idx < order.len() - 1
                        && *kind != DriverKind::Browser
                        && !self.challenge_forbids_escalation(url, &resp)
                    {
                        // A browser must be REGISTERED, not merely named in
                        // the plan's chain. Deployments running
                        // QUARRY_EDGE__BROWSER_PROVIDER=static still get
                        // `DriverKind::Browser` in every plan while the
                        // registry holds none; treating that as "escalation
                        // possible" parks a perfectly serviceable static body
                        // in `best_shell` and rotates through the rest of the
                        // chain to reach a driver that does not exist.
                        let has_browser_ahead = order[idx + 1..]
                            .iter()
                            .any(|k| *k == DriverKind::Browser && self.drivers.contains_key(k));
                        if has_browser_ahead {
                            attempts.push((
                                *kind,
                                "js-shell detected — needs browser rendering".into(),
                            ));
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
                    //
                    // Not when the response is a block status (or a strong
                    // challenge) that `honest` refuses to rotate on: a block
                    // body is always "thinner" than a shell, and rotating on
                    // that would hand the page that just refused this identity
                    // to the browser, which is the status-driven change of
                    // identity the first branch above forbids. The gate below
                    // then returns the shell (WS3.I). Under `legacy` a block
                    // status never reaches here with a driver ahead (the first
                    // branch took it), so this changes nothing there.
                    if *kind != DriverKind::Browser && idx < order.len() - 1 {
                        let is_redirect = (300..400).contains(&resp.status);
                        let thinner = best_shell
                            .as_ref()
                            .map(|b| visible_text_len(&resp.body) <= visible_text_len(&b.body))
                            .unwrap_or(false)
                            && !self.refuses_rotation(url, &resp);
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
                    // A block status or a strong challenge from a later driver
                    // that `honest` refuses to rotate on, while a shell from an
                    // earlier driver is already held: the browser is not
                    // called (the branches above forbid it), and the content
                    // that was already fetched is not thrown away either. The
                    // shell is returned, exactly as the error arm below does
                    // when a later driver fails; its page still has a title,
                    // links and metadata. Under `legacy` this is never true:
                    // a block status or a challenge was rotated on above.
                    if *kind != DriverKind::Browser
                        && best_shell.is_some()
                        && self.refuses_rotation(url, &resp)
                    {
                        if let Some(shell) = best_shell.take() {
                            warn!(
                                driver = ?kind,
                                status = resp.status,
                                shell_text_chars = visible_text_len(&shell.body),
                                "block or challenge refused under honest after a JS shell; returning the shell response"
                            );
                            return Ok(shell);
                        }
                    }
                    if *kind == DriverKind::Browser {
                        if let Some(shell) = best_shell.take() {
                            let rendered = visible_text_len(&resp.body);
                            let shell_text = visible_text_len(&shell.body);
                            if rendered <= shell_text {
                                warn!(
                                    rendered_text_chars = rendered,
                                    shell_text_chars = shell_text,
                                    rendered_bytes = resp.body.len(),
                                    "browser fallback rendered no more readable text than the static shell; keeping the static response"
                                );
                                return Ok(shell);
                            }
                            info!(
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
                    // A block-class error is a status that became an error
                    // (a proxied 403 from the static driver), so it is gated
                    // like the status itself.
                    let is_retryable = is_retryable_driver_error(e.code)
                        && escalation_allowed(self.mode, trigger_for_error(e.code));
                    attempts.push((
                        *kind,
                        format!("{}: {}", error_code_short(e.code), e.message),
                    ));
                    tier_classes.push(tier_failure_class(&e));
                    if is_retryable && idx < order.len() - 1 {
                        warn!(
                            driver = ?kind,
                            next = ?order.get(idx + 1),
                            error_class = e.code.wire_name(),
                            "driver failed, falling back"
                        );
                        last_err = Some(e);
                        continue;
                    }
                    if let Some(shell) = best_shell.take() {
                        warn!(
                            driver = ?kind,
                            error_class = e.code.wire_name(),
                            shell_text_chars = visible_text_len(&shell.body),
                            "browser fallback failed after JS-shell detection; returning the static shell response"
                        );
                        return Ok(shell);
                    }
                    let error = with_fallback_attempts(e, &attempts);
                    return Err(self.settle_chain_error(error, &tier_classes, attempts.len()));
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
            // A count, not `attempts` itself: each entry carries a driver's error
            // text, which can name the URL.
            warn!(
                attempts = attempts.len(),
                "no driver improved on the JS shell; returning the static shell response"
            );
            return Ok(shell);
        }

        let aggregate_msg = attempts
            .iter()
            .map(|(k, e)| format!("{k:?}={e}"))
            .collect::<Vec<_>>()
            .join("; ");

        let error = last_err
            .map(|e| with_fallback_attempts(e, &attempts))
            .unwrap_or_else(|| {
                quarry_core::QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("no drivers succeeded in fallback chain: {aggregate_msg}"),
                )
            });
        Err(self.settle_chain_error(error, &tier_classes, attempts.len()))
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

/// Below this a body is never a shell: a shell is markup plus a hydration
/// payload, and that does not fit in 8 KB. First test in the predicate so
/// the overwhelmingly common small-page case costs one length compare.
const SHELL_MIN_BODY_BYTES: usize = 8_000;

/// The symptom branch wants a body at least this large before it spends a
/// browser session. Between 8 KB and 20 KB a text-thin page is far more
/// likely to be a genuinely short one — a redirect notice, an error page, a
/// login form, a paywall — than a document being withheld from us.
const SHELL_SYMPTOM_MIN_BODY_BYTES: usize = 20_000;

/// Symptom branch: visible text may be at most 1/50th (2%) of the body.
/// Measured on www.ssb.no/kommunefakta/oslo — 620 966 body bytes against
/// ~130 visible characters (0.02%), because 74% of that page was hydration
/// JSON inside `<script type="application/json">`, which readability strips.
/// A page spending fifty bytes per readable character has not handed us its
/// content. Prose pages sit an order of magnitude higher (aquatiq.com:
/// 219 KB / ~4 800 chars ≈ 2.2%, and the text gate excludes it anyway).
const SHELL_MAX_TEXT_BYTES_PER_CHAR: usize = 50;

/// Embedded-JSON branch: this many bytes inside `<script type="…json">` is a
/// hydration payload, not a sprinkle of schema.org markup. SSB carried
/// 461 890 bytes across 29 such blocks; a typical JSON-LD block is 1–3 KB.
const SHELL_MIN_EMBEDDED_JSON_BYTES: usize = 16_000;

/// What a structured-data harvest (quarry-transform `structured.rs`) made of
/// this body, when one has run.
///
/// Escalating to a browser costs a session lease plus a hydration settle;
/// harvesting the JSON the page already shipped costs a parse and yields
/// typed values. So the harvest wins whenever it produces anything, and the
/// browser is reserved for the case where the page withheld its prose AND
/// carried nothing harvestable. A caller that has already harvested says so;
/// one that has not passes `Unknown` and gets the pre-harvest behaviour.
// `Found`/`Empty` are built by whoever owns the harvest result; until that
// call site threads it down here only `Unknown` is constructed in this crate.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StructuredYield {
    /// No harvest has run at this point in the chain. The fallback driver
    /// itself is here: it sees bytes, not extraction results.
    Unknown,
    /// The harvest produced usable typed values — the page HAS given us its
    /// content, just not as prose. A browser would only re-render what we
    /// already hold.
    Found,
    /// The harvest ran and came back with nothing usable.
    Empty,
}

/// Should this statically-fetched body be re-fetched through a browser?
///
/// This used to be an allowlist of vendor markers (`__NEXT_DATA__`,
/// `__NUXT__`, `_next/static`, …). www.ssb.no runs Enonic XP, scored zero on
/// all six, and sailed through as a good fetch while 74% of the page sat
/// unread in hydration JSON — the answer to "Oslo's population" was in there
/// and got discarded with the markup. An allowlist of framework names can
/// only ever describe the CMSes someone already met, so the markers are kept
/// as a cheap positive signal and the actual gate is the symptom: a body
/// large enough to be carrying a document that nonetheless exposes almost no
/// readable text has not given us its content, whoever built it.
pub(crate) fn needs_browser_escalation(body: &[u8], status: u16, harvest: StructuredYield) -> bool {
    if status != 200 {
        return false;
    }
    if body.len() < SHELL_MIN_BODY_BYTES {
        return false;
    }
    // Plenty of framework sites — aquatiq.com among them — server-render the
    // whole page AND ship the hydration bundle, so they trip every marker
    // while already exposing thousands of characters of real text.
    // Escalating those costs a browser session plus a hydration settle per
    // page and returns nothing extra (the live logs said so: "browser
    // fallback rendered no more readable text than the static shell"), which
    // pushed a 4-page crawl past the onboarding preview's event-poll ceiling
    // so only the seed page ever reached the wizard.
    let text = visible_text_len(body);
    if text >= SHELL_MAX_TEXT_CHARS {
        return false;
    }
    // The page withheld its prose, but the harvester already recovered the
    // values from the payload it shipped. Rendering it would cost a browser
    // round trip to arrive back at data we are holding.
    if harvest == StructuredYield::Found {
        return false;
    }
    // Cheap positive: a known hydration marker on a text-thin body is
    // conclusive, and costs six substring scans instead of the measurements
    // below. Never the sole gate — that was the SSB bug.
    if has_hydration_marker(body) {
        return true;
    }
    if body.len() < SHELL_SYMPTOM_MIN_BODY_BYTES {
        return false;
    }
    if text.saturating_mul(SHELL_MAX_TEXT_BYTES_PER_CHAR) <= body.len() {
        return true;
    }
    // Ratio alone misses a page that pairs a real hydration payload with a
    // page's worth of chrome (nav, footer, cookie banner) — the text is not
    // proportionally tiny, but the document still lives in the JSON.
    embedded_json_bytes(body) >= SHELL_MIN_EMBEDDED_JSON_BYTES
}

/// Pre-harvest form of [`needs_browser_escalation`], for call sites that
/// only have the bytes.
pub(crate) fn is_js_shell_needing_browser(body: &[u8], status: u16) -> bool {
    needs_browser_escalation(body, status, StructuredYield::Unknown)
}

/// Known client-rendering markers. A positive short-circuits the
/// measurements; a negative proves nothing (SSB scored zero on all six).
fn has_hydration_marker(body: &[u8]) -> bool {
    const MARKERS: &[&[u8]] = &[
        b"__NEXT_DATA__",
        b"data-next-head",
        b"_next/static",
        b"__NUXT__",
        b"id=\"__next\"",
        b"id=\"root\"", // generic SPA root
    ];
    MARKERS
        .iter()
        .any(|m| body.windows(m.len()).any(|w| w == *m))
}

/// Total bytes held inside `<script type="…json…">` blocks — the hydration
/// payloads readability strips and the structured harvester reads. Single
/// forward pass, no DOM, and it runs only after the marker and ratio tests
/// have both declined, so the settle-poll path never pays for it.
fn embedded_json_bytes(body: &[u8]) -> usize {
    let mut total = 0usize;
    let mut i = 0usize;
    while let Some(start) = find_ci(body, i, b"<script") {
        let open_end = match find(body, start, b">") {
            Some(c) => c,
            None => break,
        };
        let content_start = open_end + 1;
        let content_end = match find_ci(body, content_start, b"</script") {
            Some(e) => e,
            None => break,
        };
        // Matching "json" anywhere in the open tag covers every spelling in
        // the wild — `application/json`, `application/ld+json`, `text/json`,
        // SSB's `type="application/json" data-portal-component=…` — without
        // an attribute parser. `application/javascript` does not contain it,
        // so executable scripts stay out of the count.
        if find_ci(&body[start..open_end], 0, b"json").is_some() {
            total += content_end - content_start;
        }
        i = content_end + 1;
    }
    total
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
        .position(|w| {
            w.iter()
                .zip(needle_lower)
                .all(|(a, b)| a.to_ascii_lowercase() == *b)
        })
        .map(|p| p + from)
}

/// The environment variable that turns the fetch sub-codes on (E3.8.1).
pub const FETCH_SUB_CODES_ENV: &str = "QUARRY_EDGE__FETCH_SUB_CODES";

/// The pure half of [`fetch_sub_codes_enabled`]. `1`, `true` and `on` (any case,
/// surrounding blanks ignored) turn it on. Unset, blank or any other value is
/// off, so a typo leaves every envelope as it was.
pub fn fetch_sub_codes_from_value(value: Option<&str>) -> bool {
    matches!(
        value.map(|raw| raw.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "on")
    )
}

/// The process-wide default, read from the environment once.
pub fn fetch_sub_codes_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let raw = std::env::var(FETCH_SUB_CODES_ENV).ok();
        let enabled = fetch_sub_codes_from_value(raw.as_deref());
        info!(enabled = enabled, "quarry.fetch_sub_codes");
        enabled
    })
}

/// The fetch sub-code a tier's own error named (E3.8.1), if it named one.
/// `all_drivers_failed` is the verdict on a whole chain, so it is never a tier's reason.
fn tier_failure_class(error: &quarry_core::QuarryError) -> Option<&'static str> {
    match error.driver_failed_reason()? {
        driver_failed_reason::ALL_DRIVERS_FAILED => None,
        class => Some(class),
    }
}

/// What a waterfall that ended in DRIVER_FAILED reports (E3.8.1): the sub-code
/// every tried tier failed with, when they all agree on one. A tier that answered
/// with a status or a body the chain moved on from, a tier that named no sub-code,
/// or two tiers that disagree, make it `all_drivers_failed`. `attempts` counts every
/// tier that was tried; `tier_classes` has one entry per tier that failed with an
/// error, so the two lengths match only when every attempt failed that way.
fn chain_reason(tier_classes: &[Option<&'static str>], attempts: usize) -> &'static str {
    if tier_classes.len() != attempts {
        return driver_failed_reason::ALL_DRIVERS_FAILED;
    }
    match tier_classes.split_first() {
        Some((Some(first), rest)) if rest.iter().all(|class| class == &Some(*first)) => *first,
        _ => driver_failed_reason::ALL_DRIVERS_FAILED,
    }
}

/// Stamps the reason and the retryable flag on a DRIVER_FAILED that leaves the
/// waterfall (E3.8.1). A typed reason a tier already gave (`action_failed`,
/// `session_reaped`, ...) is kept with its own flag: only an absent reason or a
/// fetch sub-code is replaced. Any other code is returned as it is.
fn settle_driver_failed(
    mut error: quarry_core::QuarryError,
    tier_classes: &[Option<&'static str>],
    attempts: usize,
) -> quarry_core::QuarryError {
    if error.code != ErrorCode::DriverFailed {
        return error;
    }
    let keeps_its_reason = error
        .details
        .as_ref()
        .is_some_and(|details| details.get("reason").is_some())
        && error.driver_failed_reason().is_none();
    if keeps_its_reason {
        return error;
    }
    if error
        .details
        .as_ref()
        .is_some_and(|details| !details.is_object())
    {
        return error;
    }
    let reason = chain_reason(tier_classes, attempts);
    let mut details = error
        .details
        .take()
        .unwrap_or_else(|| serde_json::json!({}));
    details["reason"] = serde_json::Value::String(reason.to_owned());
    error.details = Some(details);
    error.retryable = driver_failed_reason::retryable(reason);
    error
}

pub(crate) fn is_retryable_driver_error(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::Timeout | ErrorCode::UpstreamBlocked | ErrorCode::DriverFailed
    )
}

/// The error with the per-driver attempts added to its details. A typed reason the driver
/// already put there (`action_failed` with the ordered outcomes of a scrape-time action
/// run, a block reason) stays: the attempts are one more key, never a replacement.
fn with_fallback_attempts(
    mut error: quarry_core::QuarryError,
    attempts: &[(DriverKind, String)],
) -> quarry_core::QuarryError {
    let listed = serde_json::json!(attempts
        .iter()
        .map(|(k, msg)| serde_json::json!({"driver": format!("{k:?}"), "error": msg}))
        .collect::<Vec<_>>());
    match error.details.as_mut().and_then(serde_json::Value::as_object_mut) {
        Some(object) => {
            object.insert("fallback_attempts".to_owned(), listed);
            error
        }
        None => error.with_details(serde_json::json!({ "fallback_attempts": listed })),
    }
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
        ErrorCode::UnsupportedMedia => "unsupported_media",
        ErrorCode::BudgetExhausted => "budget_exhausted",
        ErrorCode::UserTookOver => "user_took_over",
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
                captures: None,
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

    /// Fails with a typed reason in the details, as a scrape-time action run does.
    struct ReasonedFailDriver;

    #[async_trait]
    impl Driver for ReasonedFailDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Browser
        }

        async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
            Err(
                QuarryError::new(ErrorCode::DriverFailed, "a scrape action failed").with_details(
                    serde_json::json!({ "reason": "action_failed", "actions": [{ "index": 0 }] }),
                ),
            )
        }
    }

    /// The attempts are added to a driver's own details, never put in their place:
    /// the typed `action_failed` reason and its ordered outcomes reach the caller.
    #[tokio::test]
    async fn a_typed_reason_survives_the_fallback_attempts() {
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Browser, Arc::new(ReasonedFailDriver));
        let fb = FallbackDriver::new(DriverKind::Browser, vec![], drivers);
        let url: Url = "https://example.com".parse().unwrap();
        let err = fb.fetch(&url).await.unwrap_err();
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["reason"], "action_failed");
        assert_eq!(details["actions"][0]["index"], 0);
        assert_eq!(details["fallback_attempts"].as_array().map(Vec::len), Some(1));
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
                captures: None,
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
        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers)
            .with_escalation_mode(EscalationMode::Legacy);
        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.served_by, DriverKind::Tls);
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 1);
    }

    /// A driver that always fails with a chosen error code.
    struct CodeFailDriver {
        kind: DriverKind,
        code: ErrorCode,
        calls: AtomicU32,
    }

    impl CodeFailDriver {
        fn new(kind: DriverKind, code: ErrorCode) -> Self {
            Self {
                kind,
                code,
                calls: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl Driver for CodeFailDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }

        async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Err(QuarryError::new(self.code, "simulated failure"))
        }
    }

    fn chain(mode: EscalationMode, order: Vec<(DriverKind, Arc<dyn Driver>)>) -> FallbackDriver {
        let primary = order[0].0;
        let rest = order[1..].iter().map(|(k, _)| *k).collect();
        let drivers: HashMap<DriverKind, Arc<dyn Driver>> = order.into_iter().collect();
        FallbackDriver::new(primary, rest, drivers).with_escalation_mode(mode)
    }

    #[tokio::test]
    async fn honest_block_status_returns_without_next_driver() {
        let primary = Arc::new(BlockDriver::new(DriverKind::Static));
        let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
        let browser = Arc::new(OkDriver::new(DriverKind::Browser));
        let fb = chain(
            EscalationMode::Honest,
            vec![
                (DriverKind::Static, primary.clone()),
                (DriverKind::Tls, fallback.clone()),
                (DriverKind::Browser, browser.clone()),
            ],
        );

        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status, 403, "the block is surfaced as it is");
        assert_eq!(resp.served_by, DriverKind::Static);
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 0);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn honest_upstream_blocked_error_does_not_fall_through() {
        // The static driver turns a proxied 403 into an UpstreamBlocked error;
        // the error path is a second way a status could change the driver.
        for code in [ErrorCode::UpstreamBlocked, ErrorCode::RateLimited] {
            let primary = Arc::new(CodeFailDriver::new(DriverKind::Static, code));
            let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
            let fb = chain(
                EscalationMode::Honest,
                vec![
                    (DriverKind::Static, primary.clone()),
                    (DriverKind::Tls, fallback.clone()),
                ],
            );

            let err = fb
                .fetch(&"https://example.com".parse().unwrap())
                .await
                .unwrap_err();

            assert_eq!(err.code, code);
            assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
            assert_eq!(fallback.calls.load(Ordering::Relaxed), 0, "{code:?}");
        }
    }

    #[tokio::test]
    async fn legacy_upstream_blocked_error_still_falls_through() {
        let primary = Arc::new(CodeFailDriver::new(
            DriverKind::Static,
            ErrorCode::UpstreamBlocked,
        ));
        let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
        let fb = chain(
            EscalationMode::Legacy,
            vec![
                (DriverKind::Static, primary.clone()),
                (DriverKind::Tls, fallback.clone()),
            ],
        );

        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(resp.served_by, DriverKind::Tls);
        assert_eq!(fallback.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn honest_timeout_still_falls_through() {
        for code in [ErrorCode::Timeout, ErrorCode::DriverFailed] {
            let primary = Arc::new(CodeFailDriver::new(DriverKind::Static, code));
            let fallback = Arc::new(OkDriver::new(DriverKind::Tls));
            let fb = chain(
                EscalationMode::Honest,
                vec![
                    (DriverKind::Static, primary.clone()),
                    (DriverKind::Tls, fallback.clone()),
                ],
            );

            let resp = fb
                .fetch(&"https://example.com".parse().unwrap())
                .await
                .unwrap();

            assert_eq!(resp.served_by, DriverKind::Tls, "{code:?}");
            assert_eq!(fallback.calls.load(Ordering::Relaxed), 1, "{code:?}");
        }
    }

    #[tokio::test]
    async fn honest_js_shell_still_escalates_to_browser() {
        // Not a status: a 200 page that withheld its content. The browser tier
        // stays available for that in both modes.
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
        let fb = chain(
            EscalationMode::Honest,
            vec![
                (DriverKind::Static, static_shell.clone()),
                (DriverKind::Browser, browser.clone()),
            ],
        );

        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(resp.served_by, DriverKind::Browser);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 1);
    }

    /// A 200 challenge interstitial that is also large and text-thin (what the
    /// shell gate looks for): a script-heavy Cloudflare-style page.
    fn challenge_interstitial_body() -> Vec<u8> {
        let mut b = shell_body();
        b.extend_from_slice(b"<script>window._cf_chl_opt={cvId:'3',cType:'managed'};</script>");
        b
    }

    #[test]
    fn the_challenge_test_body_is_a_shell_and_a_strong_challenge() {
        // Pins the premise of the two tests below: without it they could pass
        // for the wrong reason (the body not being a shell at all).
        let body = challenge_interstitial_body();
        assert!(needs_browser_escalation(
            &body,
            200,
            StructuredYield::Unknown
        ));
        let fb = FallbackDriver::new(DriverKind::Static, vec![], HashMap::new())
            .with_escalation_mode(EscalationMode::Honest);
        let resp = FetchResponse {
            status: 200,
            final_url: "https://example.com".parse().unwrap(),
            headers: vec![],
            body,
            duration_ms: 1,
            served_by: DriverKind::Static,
            captures: None,
        };
        assert!(fb.challenge_forbids_escalation(&"https://example.com".parse().unwrap(), &resp));
        let legacy = fb.with_escalation_mode(EscalationMode::Legacy);
        assert!(
            !legacy.challenge_forbids_escalation(&"https://example.com".parse().unwrap(), &resp)
        );
    }

    #[tokio::test]
    async fn honest_challenge_interstitial_with_200_is_not_escalated_to_browser() {
        let static_driver = Arc::new(BodyDriver {
            kind: DriverKind::Static,
            body: challenge_interstitial_body(),
            calls: AtomicU32::new(0),
        });
        let browser = Arc::new(OkDriver::new(DriverKind::Browser));
        let fb = chain(
            EscalationMode::Honest,
            vec![
                (DriverKind::Static, static_driver.clone()),
                (DriverKind::Browser, browser.clone()),
            ],
        );

        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(resp.served_by, DriverKind::Static);
        assert_eq!(resp.status, 200);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn legacy_challenge_interstitial_with_200_still_escalates_to_browser() {
        let static_driver = Arc::new(BodyDriver {
            kind: DriverKind::Static,
            body: challenge_interstitial_body(),
            calls: AtomicU32::new(0),
        });
        // The browser must render more readable text than the shell, or the
        // "no better than the shell" rule returns the shell for another reason.
        let browser = Arc::new(BodyDriver {
            kind: DriverKind::Browser,
            body: "<html><body><main><h1>Hygiene for matindustrien</h1><p>Vi leverer hygieneløsninger til næringsmiddelindustrien.</p></main></body></html>".as_bytes().to_vec(),
            calls: AtomicU32::new(0),
        });
        let fb = chain(
            EscalationMode::Legacy,
            vec![
                (DriverKind::Static, static_driver.clone()),
                (DriverKind::Browser, browser.clone()),
            ],
        );

        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(resp.served_by, DriverKind::Browser);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 1);
    }

    fn rendered_body() -> Vec<u8> {
        "<html><body><main><h1>Hygiene for matindustrien</h1><p>Vi leverer hygieneløsninger til næringsmiddelindustrien.</p></main></body></html>".as_bytes().to_vec()
    }

    fn body_driver(kind: DriverKind, body: Vec<u8>) -> Arc<BodyDriver> {
        Arc::new(BodyDriver {
            kind,
            body,
            calls: AtomicU32::new(0),
        })
    }

    /// The production chain is `[Static, Tls, Browser]`. Static returns a JS
    /// shell (kept as `best_shell`), TLS answers 403. The 403 body is always
    /// thinner than the shell, so the "not an improvement" branch used to hand
    /// the page that had just refused the TLS identity to the browser: a
    /// status-driven change of identity under `honest`. The browser is not
    /// called, and the shell that was already fetched is returned (the same
    /// outcome as a TLS error after a shell).
    #[tokio::test]
    async fn honest_block_from_tls_after_a_shell_returns_the_shell_not_the_browser() {
        let static_shell = body_driver(DriverKind::Static, shell_body());
        let tls = Arc::new(BlockDriver::new(DriverKind::Tls));
        let browser = body_driver(DriverKind::Browser, rendered_body());
        let fb = chain(
            EscalationMode::Honest,
            vec![
                (DriverKind::Static, static_shell.clone()),
                (DriverKind::Tls, tls.clone()),
                (DriverKind::Browser, browser.clone()),
            ],
        );

        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status, 200, "the shell that was already fetched");
        assert_eq!(resp.served_by, DriverKind::Static);
        assert_eq!(&resp.body[..], shell_body().as_slice());
        assert_eq!(tls.calls.load(Ordering::Relaxed), 1);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn legacy_block_from_tls_after_a_shell_still_reaches_the_browser() {
        let static_shell = body_driver(DriverKind::Static, shell_body());
        let tls = Arc::new(BlockDriver::new(DriverKind::Tls));
        let browser = body_driver(DriverKind::Browser, rendered_body());
        let fb = chain(
            EscalationMode::Legacy,
            vec![
                (DriverKind::Static, static_shell.clone()),
                (DriverKind::Tls, tls.clone()),
                (DriverKind::Browser, browser.clone()),
            ],
        );

        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(resp.served_by, DriverKind::Browser);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn honest_strong_challenge_from_tls_after_a_shell_returns_the_shell_not_the_browser() {
        let static_shell = body_driver(DriverKind::Static, shell_body());
        let tls = body_driver(DriverKind::Tls, challenge_interstitial_body());
        let browser = body_driver(DriverKind::Browser, rendered_body());
        let fb = chain(
            EscalationMode::Honest,
            vec![
                (DriverKind::Static, static_shell.clone()),
                (DriverKind::Tls, tls.clone()),
                (DriverKind::Browser, browser.clone()),
            ],
        );

        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(resp.served_by, DriverKind::Static);
        assert_eq!(resp.status, 200);
        assert_eq!(&resp.body[..], shell_body().as_slice());
        assert_eq!(tls.calls.load(Ordering::Relaxed), 1);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn legacy_strong_challenge_from_tls_after_a_shell_still_reaches_the_browser() {
        let static_shell = body_driver(DriverKind::Static, shell_body());
        let tls = body_driver(DriverKind::Tls, challenge_interstitial_body());
        let browser = body_driver(DriverKind::Browser, rendered_body());
        let fb = chain(
            EscalationMode::Legacy,
            vec![
                (DriverKind::Static, static_shell.clone()),
                (DriverKind::Tls, tls.clone()),
                (DriverKind::Browser, browser.clone()),
            ],
        );

        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(resp.served_by, DriverKind::Browser);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 1);
    }

    /// Only the strong class forbids the browser under `honest`. A weak hint
    /// (a prove-human phrase, a login form with a reCAPTCHA widget) and a
    /// non-signal (a plain login page) are JS shells like any other: the page
    /// withheld its content and a browser may render it.
    #[tokio::test]
    async fn honest_weak_hint_and_non_signal_shells_still_escalate_to_browser() {
        let mut weak_phrase = shell_body();
        weak_phrase.extend_from_slice(b"<p>Please verify you are human to continue.</p>");
        let mut weak_login_widget = shell_body();
        weak_login_widget.extend_from_slice(
            b"<form><input type=\"email\" name=\"e\"><input type=\"password\" name=\"p\"><div class=\"g-recaptcha\" data-sitekey=\"k\"></div></form>",
        );
        let mut plain_login = shell_body();
        plain_login.extend_from_slice(
            b"<form><input type=\"email\" name=\"e\"><input type=\"password\" name=\"p\"></form>",
        );
        let url: Url = "https://example.com".parse().unwrap();

        for (label, body, expected_class) in [
            ("prove-human phrase", weak_phrase, ChallengeStrength::Weak),
            (
                "login form with a recaptcha",
                weak_login_widget,
                ChallengeStrength::Weak,
            ),
            (
                "plain login form",
                plain_login,
                ChallengeStrength::NonSignal,
            ),
        ] {
            // Premises: still a shell, and not a strong challenge.
            assert!(
                needs_browser_escalation(&body, 200, StructuredYield::Unknown),
                "{label}: premise, the body is a shell"
            );
            let head =
                String::from_utf8_lossy(&body[..body.len().min(MAX_SCANNED_BYTES)]).into_owned();
            let seen = classify(&ChallengeInput {
                url: url.as_str(),
                title: None,
                html: Some(&head),
                text_snippet: None,
                main_doc_status: Some(200),
            });
            assert_ne!(
                seen.as_ref().map(|s| s.strength),
                Some(ChallengeStrength::Strong),
                "{label}: premise, not a strong challenge"
            );
            assert_eq!(
                seen.as_ref().map(|s| s.strength),
                Some(expected_class),
                "{label}: premise, the class the test is about"
            );

            let static_driver = body_driver(DriverKind::Static, body);
            let browser = body_driver(DriverKind::Browser, rendered_body());
            let fb = chain(
                EscalationMode::Honest,
                vec![
                    (DriverKind::Static, static_driver.clone()),
                    (DriverKind::Browser, browser.clone()),
                ],
            );

            let resp = fb.fetch(&url).await.unwrap();

            assert_eq!(resp.served_by, DriverKind::Browser, "{label}");
            assert_eq!(browser.calls.load(Ordering::Relaxed), 1, "{label}");
        }
    }

    /// A connect failure on the static driver is reported as
    /// `UpstreamBlocked`, the same code a proxied 403 gets, and the error
    /// alone cannot tell them apart. Under `honest` it therefore does not fall
    /// through to the TLS driver (fail closed, documented in `escalation.rs`
    /// and RUNBOOKS); under `legacy` it does. Pinned so the choice is a
    /// visible one: changing the mapping must change this test.
    #[tokio::test]
    async fn honest_connect_failure_on_the_static_driver_does_not_fall_through_to_tls() {
        use quarry_core::privacy::PrivacyPolicy;
        let dead = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap()
        };
        let hints = FetchHints {
            org_id: "org_connect_failure".into(),
            privacy: PrivacyPolicy {
                allow_third_party_processing: true,
                processor_id: Some("quarry_proxy_pool".into()),
                ..PrivacyPolicy::default()
            },
            ..FetchHints::default()
        };
        let url: Url = "http://example.com/".parse().unwrap();

        for mode in [EscalationMode::Honest, EscalationMode::Legacy] {
            let pool = crate::proxy_pool::ProxyPool::from_env_string(&format!("http://{dead}"));
            let static_driver = Arc::new(
                crate::fetch::StaticDriver::with_proxy_pool_and_processor(
                    std::time::Duration::from_secs(5),
                    "QuarryTest/1.0",
                    pool,
                    Some("quarry_proxy_pool".into()),
                )
                .unwrap()
                .with_escalation_mode(mode),
            );
            let tls = Arc::new(OkDriver::new(DriverKind::Tls));
            let fb = chain(
                mode,
                vec![
                    (DriverKind::Static, static_driver),
                    (DriverKind::Tls, tls.clone()),
                ],
            );

            let result = fb.fetch_conditional(&url, &hints).await;

            match mode {
                EscalationMode::Honest => {
                    let err = result.expect_err("honest: the connect failure is returned");
                    assert_eq!(err.code, ErrorCode::UpstreamBlocked);
                    assert_eq!(tls.calls.load(Ordering::Relaxed), 0);
                }
                EscalationMode::Legacy => {
                    let resp = result.expect("legacy: the TLS driver serves it");
                    assert_eq!(resp.served_by, DriverKind::Tls);
                    assert_eq!(tls.calls.load(Ordering::Relaxed), 1);
                }
            }
        }
    }

    #[tokio::test]
    async fn all_blocked_returns_last_block_response() {
        let primary = Arc::new(BlockDriver::new(DriverKind::Static));
        let fallback = Arc::new(BlockDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, primary.clone());
        drivers.insert(DriverKind::Tls, fallback.clone());
        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Tls], drivers)
            .with_escalation_mode(EscalationMode::Legacy);
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
        b.extend_from_slice(
            b"<html><head><title>aquatiq.com</title></head><body><div id=\"__next\"></div>",
        );
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
                captures: None,
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

    /// The measured www.ssb.no/kommunefakta/oslo shape: a ~465 KB body whose
    /// content lives in 29 `<script type="application/json">` hydration
    /// blobs, a line of chrome around them, and not one vendor marker — SSB
    /// runs Enonic XP, so the old allowlist scored zero and the page sailed
    /// through as a good fetch.
    fn ssb_shaped_body() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"<html><head><title>Kommunefakta Oslo</title></head><body>");
        b.extend_from_slice(b"<nav>Hopp til innhold Meny Statistikkbanken Kontakt oss</nav>");
        b.extend_from_slice(b"<main><h1>Kommunefakta Oslo</h1></main>");
        for _ in 0..29 {
            b.extend_from_slice(
                b"<script type=\"application/json\" data-portal-component=\"part\">",
            );
            b.extend_from_slice(
                format!(
                    "{{\"folketallet\":\"729 437\",\"tid\":\"2. kvartal 2026\",\"pad\":\"{}\"}}",
                    "x".repeat(15_900)
                )
                .as_bytes(),
            );
            b.extend_from_slice(b"</script>");
        }
        b.extend_from_slice(b"</body></html>");
        b
    }

    /// The incident: fetched successfully (620 966 bytes), answered "could
    /// not find it". No vendor marker, so only the symptom test can catch it.
    #[test]
    fn ssb_shape_is_detected_without_any_vendor_marker() {
        let body = ssb_shaped_body();
        assert!(
            !has_hydration_marker(&body),
            "fixture must not smuggle in a vendor marker, or it proves nothing"
        );
        assert!(body.len() > 400_000);
        assert!(visible_text_len(&body) < 200);
        assert!(embedded_json_bytes(&body) > 400_000);
        assert!(is_js_shell_needing_browser(&body, 200));
    }

    #[test]
    fn small_bodies_are_never_shells() {
        let mut body = b"<html><body><p>Kort side.</p>".to_vec();
        body.extend_from_slice(&vec![b' '; 4_000]);
        body.extend_from_slice(b"</body></html>");
        assert!(body.len() < SHELL_MIN_BODY_BYTES);
        assert!(!is_js_shell_needing_browser(&body, 200));
    }

    #[test]
    fn a_large_page_full_of_visible_text_is_not_a_shell() {
        let body = "<html><body><article><p>Folketalet i Oslo var 729 437 personar ved utgangen av andre kvartal 2026. </p></article></body></html>"
            .repeat(900)
            .into_bytes();
        assert!(body.len() > SHELL_SYMPTOM_MIN_BODY_BYTES);
        assert!(visible_text_len(&body) >= SHELL_MAX_TEXT_CHARS);
        assert!(!is_js_shell_needing_browser(&body, 200));
    }

    /// Guard against escalating every heavy page: bytes spent on a base64
    /// image or inline CSS are not a withheld document, and a browser would
    /// return exactly the same text at the cost of a session.
    #[test]
    fn a_large_page_whose_bytes_are_markup_not_payload_is_not_escalated() {
        let mut body = Vec::new();
        body.extend_from_slice(b"<html><body>");
        body.extend_from_slice(b"<img alt=\"\" src=\"data:image/png;base64,");
        body.extend_from_slice(&vec![b'A'; 24_000]);
        body.extend_from_slice(b"\">");
        body.extend_from_slice(
            "<p>Kontaktinformasjon og opningstider </p>"
                .repeat(30)
                .as_bytes(),
        );
        body.extend_from_slice(b"</body></html>");
        assert!(body.len() > SHELL_SYMPTOM_MIN_BODY_BYTES);
        assert!(visible_text_len(&body) < SHELL_MAX_TEXT_CHARS);
        assert_eq!(embedded_json_bytes(&body), 0);
        assert!(!is_js_shell_needing_browser(&body, 200));
    }

    /// A hydration payload paired with a page's worth of chrome (nav,
    /// footer, cookie banner) is not proportionally text-thin, so the ratio
    /// declines it; the payload itself is what gives it away.
    #[test]
    fn a_hydration_payload_behind_ordinary_chrome_is_caught_by_the_json_branch() {
        let mut body = Vec::new();
        body.extend_from_slice(b"<html><body>");
        body.extend_from_slice(
            "<p>Kontaktinformasjon og opningstider </p>"
                .repeat(26)
                .as_bytes(),
        );
        body.extend_from_slice(b"<script type=\"application/ld+json\">");
        body.extend_from_slice(&vec![b'z'; 24_000]);
        body.extend_from_slice(b"</script></body></html>");
        let text = visible_text_len(&body);
        assert!(text < SHELL_MAX_TEXT_CHARS);
        assert!(
            text.saturating_mul(SHELL_MAX_TEXT_BYTES_PER_CHAR) > body.len(),
            "this fixture must fail the ratio test so it exercises the JSON branch"
        );
        assert!(is_js_shell_needing_browser(&body, 200));
    }

    /// The ordering the sibling harvester (quarry-transform `structured.rs`)
    /// makes possible: the JSON is free and typed, the browser is not.
    #[test]
    fn a_successful_structured_harvest_suppresses_the_browser_round_trip() {
        let body = ssb_shaped_body();
        assert!(needs_browser_escalation(&body, 200, StructuredYield::Empty));
        assert!(!needs_browser_escalation(
            &body,
            200,
            StructuredYield::Found
        ));
    }

    #[test]
    fn non_200_responses_are_never_shells() {
        assert!(!is_js_shell_needing_browser(&ssb_shaped_body(), 404));
    }

    /// BROWSER_PROVIDER=static: every plan still names `Browser`, the
    /// registry holds none. A true positive must come back as the best
    /// static response we have — never an error, never an empty body.
    #[tokio::test]
    async fn a_detected_shell_with_no_browser_registered_returns_the_static_response() {
        let static_shell = Arc::new(BodyDriver {
            kind: DriverKind::Static,
            body: ssb_shaped_body(),
            calls: AtomicU32::new(0),
        });
        let tls = Arc::new(OkDriver::new(DriverKind::Tls));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, static_shell.clone());
        drivers.insert(DriverKind::Tls, tls.clone());
        let fb = FallbackDriver::new(
            DriverKind::Static,
            vec![DriverKind::Tls, DriverKind::Browser],
            drivers,
        );
        let resp = fb
            .fetch(&"https://www.ssb.no/kommunefakta/oslo".parse().unwrap())
            .await
            .expect("no browser to escalate to must degrade to the static body, not an error");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.served_by, DriverKind::Static);
        assert!(resp.body.len() > 400_000);
        assert_eq!(
            tls.calls.load(Ordering::Relaxed),
            0,
            "with nothing to escalate to there is no reason to rotate off a 200"
        );
    }

    /// Same true positive, but a browser IS registered and fails: the static
    /// body still wins over an error.
    #[tokio::test]
    async fn a_detected_shell_with_a_failing_browser_returns_the_static_response() {
        let static_shell = Arc::new(BodyDriver {
            kind: DriverKind::Static,
            body: ssb_shaped_body(),
            calls: AtomicU32::new(0),
        });
        let browser = Arc::new(FailDriver::new(DriverKind::Browser));
        let mut drivers: HashMap<DriverKind, Arc<dyn Driver>> = HashMap::new();
        drivers.insert(DriverKind::Static, static_shell.clone());
        drivers.insert(DriverKind::Browser, browser.clone());
        let fb = FallbackDriver::new(DriverKind::Static, vec![DriverKind::Browser], drivers);
        let resp = fb
            .fetch(&"https://www.ssb.no/kommunefakta/oslo".parse().unwrap())
            .await
            .expect("browser failure after detection must return the static body");
        assert_eq!(resp.served_by, DriverKind::Static);
        assert!(resp.body.len() > 400_000);
        assert_eq!(browser.calls.load(Ordering::Relaxed), 1);
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
        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();
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
        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();
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
                captures: None,
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
        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();
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
        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();
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
        let resp = fb
            .fetch(&"https://example.com".parse().unwrap())
            .await
            .unwrap();
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

#[cfg(test)]
mod fetch_sub_code_tests {
    use super::*;
    use quarry_core::QuarryError;
    use serde_json::json;

    /// Fails every call with one error, as a tier that names its own failure does.
    struct ErrorDriver {
        kind: DriverKind,
        error: QuarryError,
    }

    #[async_trait]
    impl Driver for ErrorDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }

        async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
            Err(self.error.clone())
        }
    }

    /// Serves the page on every call.
    struct ServesDriver {
        kind: DriverKind,
    }

    #[async_trait]
    impl Driver for ServesDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }

        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![],
                body: b"ok".to_vec(),
                duration_ms: 1,
                served_by: self.kind,
                captures: None,
            })
        }
    }

    /// Answers 403 on every call. Under `legacy` the chain rotates on the status.
    struct BlockedDriver {
        kind: DriverKind,
    }

    #[async_trait]
    impl Driver for BlockedDriver {
        fn kind(&self) -> DriverKind {
            self.kind
        }

        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            Ok(FetchResponse {
                status: 403,
                final_url: url.clone(),
                headers: vec![],
                body: b"blocked".to_vec(),
                duration_ms: 1,
                served_by: self.kind,
                captures: None,
            })
        }
    }

    fn tier(kind: DriverKind, driver: impl Driver + 'static) -> (DriverKind, Arc<dyn Driver>) {
        let driver: Arc<dyn Driver> = Arc::new(driver);
        (kind, driver)
    }

    fn failing(kind: DriverKind, error: QuarryError) -> (DriverKind, Arc<dyn Driver>) {
        tier(kind, ErrorDriver { kind, error })
    }

    fn chain(
        mode: EscalationMode,
        sub_codes: bool,
        order: Vec<(DriverKind, Arc<dyn Driver>)>,
    ) -> FallbackDriver {
        let primary = order[0].0;
        let rest = order[1..].iter().map(|(kind, _)| *kind).collect();
        let drivers: HashMap<DriverKind, Arc<dyn Driver>> = order.into_iter().collect();
        FallbackDriver::new(primary, rest, drivers)
            .with_escalation_mode(mode)
            .with_fetch_sub_codes(sub_codes)
    }

    fn target_url() -> Url {
        "https://example.com/".parse().expect("a valid URL")
    }

    fn dns() -> QuarryError {
        QuarryError::driver_failed(driver_failed_reason::DNS, "lookup failed")
    }

    fn connect() -> QuarryError {
        QuarryError::driver_failed(driver_failed_reason::CONNECT, "connection refused")
    }

    fn tls() -> QuarryError {
        QuarryError::driver_failed(driver_failed_reason::TLS, "handshake failed")
    }

    fn plain() -> QuarryError {
        QuarryError::new(ErrorCode::DriverFailed, "simulated failure")
    }

    fn reason_of(error: &QuarryError) -> Option<&str> {
        error.details.as_ref()?.get("reason")?.as_str()
    }

    fn attempts_of(error: &QuarryError) -> usize {
        error
            .details
            .as_ref()
            .and_then(|details| details.get("fallback_attempts"))
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len)
    }

    #[tokio::test]
    async fn with_the_flag_off_the_error_is_left_as_the_tiers_made_it() {
        let fb = chain(
            EscalationMode::Honest,
            false,
            vec![
                failing(DriverKind::Static, plain()),
                failing(DriverKind::Tls, plain()),
            ],
        );
        let err = fb.fetch(&target_url()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
        assert!(!err.retryable);
        assert_eq!(
            reason_of(&err),
            None,
            "no reason is stamped with the flag off"
        );
        assert_eq!(attempts_of(&err), 2);
    }

    #[tokio::test]
    async fn tiers_that_fail_without_a_sub_code_report_all_drivers_failed() {
        let fb = chain(
            EscalationMode::Honest,
            true,
            vec![
                failing(DriverKind::Static, plain()),
                failing(DriverKind::Tls, plain()),
            ],
        );
        let err = fb.fetch(&target_url()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
        assert_eq!(reason_of(&err), Some("all_drivers_failed"));
        assert!(!err.retryable);
        assert_eq!(attempts_of(&err), 2);
    }

    #[tokio::test]
    async fn tiers_that_all_fail_the_same_way_report_that_sub_code_and_its_flag() {
        let fb = chain(
            EscalationMode::Honest,
            true,
            vec![
                failing(DriverKind::Static, dns()),
                failing(DriverKind::Tls, dns()),
            ],
        );
        let err = fb.fetch(&target_url()).await.unwrap_err();
        assert_eq!(reason_of(&err), Some("dns"));
        assert!(err.retryable, "dns carries its own retryable flag");
        assert_eq!(err.driver_failed_reason(), Some("dns"));
    }

    #[tokio::test]
    async fn tiers_that_disagree_report_all_drivers_failed() {
        let fb = chain(
            EscalationMode::Honest,
            true,
            vec![
                failing(DriverKind::Static, dns()),
                failing(DriverKind::Tls, connect()),
            ],
        );
        let err = fb.fetch(&target_url()).await.unwrap_err();
        assert_eq!(reason_of(&err), Some("all_drivers_failed"));
        assert!(!err.retryable);
    }

    #[tokio::test]
    async fn a_single_tier_sub_code_is_kept_with_its_own_flag() {
        let fb = chain(
            EscalationMode::Honest,
            true,
            vec![failing(DriverKind::Static, tls())],
        );
        let err = fb.fetch(&target_url()).await.unwrap_err();
        assert_eq!(reason_of(&err), Some("tls"));
        assert!(!err.retryable, "tls is not retryable");
    }

    #[tokio::test]
    async fn a_sub_code_still_falls_through_to_a_tier_that_serves() {
        // The waterfall is unchanged: dns is a transport failure and moves on.
        let fb = chain(
            EscalationMode::Honest,
            true,
            vec![
                failing(DriverKind::Static, dns()),
                tier(
                    DriverKind::Tls,
                    ServesDriver {
                        kind: DriverKind::Tls,
                    },
                ),
            ],
        );
        let resp = fb.fetch(&target_url()).await.unwrap();
        assert_eq!(resp.served_by, DriverKind::Tls);
    }

    #[tokio::test]
    async fn a_typed_reason_a_tier_gave_is_kept_with_its_own_flag() {
        let typed = QuarryError::new(ErrorCode::DriverFailed, "a scrape action failed")
            .with_details(json!({ "reason": "action_failed" }));
        let fb = chain(
            EscalationMode::Honest,
            true,
            vec![failing(DriverKind::Browser, typed)],
        );
        let err = fb.fetch(&target_url()).await.unwrap_err();
        assert_eq!(reason_of(&err), Some("action_failed"));
        assert!(!err.retryable);
    }

    #[tokio::test]
    async fn only_a_driver_failed_is_settled() {
        let timeout = QuarryError::new(ErrorCode::Timeout, "too slow");
        let fb = chain(
            EscalationMode::Honest,
            true,
            vec![
                failing(DriverKind::Static, timeout.clone()),
                failing(DriverKind::Tls, timeout),
            ],
        );
        let err = fb.fetch(&target_url()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Timeout);
        assert_eq!(reason_of(&err), None);
    }

    #[tokio::test]
    async fn a_status_the_chain_moved_on_from_breaks_the_agreement() {
        // Under legacy the static tier answers 403 and the chain rotates on it. The
        // static tier answered, so the dns of the next tier is not the chain's cause.
        let fb = chain(
            EscalationMode::Legacy,
            true,
            vec![
                tier(
                    DriverKind::Static,
                    BlockedDriver {
                        kind: DriverKind::Static,
                    },
                ),
                failing(DriverKind::Tls, dns()),
            ],
        );
        let err = fb.fetch(&target_url()).await.unwrap_err();
        assert_eq!(reason_of(&err), Some("all_drivers_failed"));
        assert!(!err.retryable);
    }

    #[test]
    fn the_flag_is_on_only_for_a_true_value() {
        assert!(!fetch_sub_codes_from_value(None));
        assert!(!fetch_sub_codes_from_value(Some("")));
        assert!(!fetch_sub_codes_from_value(Some("0")));
        assert!(!fetch_sub_codes_from_value(Some("yes")));
        assert!(fetch_sub_codes_from_value(Some("1")));
        assert!(fetch_sub_codes_from_value(Some("TRUE")));
        assert!(fetch_sub_codes_from_value(Some(" on ")));
    }
}
