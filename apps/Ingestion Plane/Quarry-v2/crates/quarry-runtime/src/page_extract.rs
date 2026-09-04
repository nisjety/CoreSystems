//! Page-level extraction for the `page_extracted` event: markdown → plain
//! excerpt, word count, generic-title detection, and the optional Model
//! Plane hop that turns a missing/generic `<title>` into a clean short title.
//!
//! Boundary rules (see `docs/ONBOARDING_LIVE_CRAWL.md` and `docs/SELF_HOST.md`):
//!
//! * Quarry owns the source artifact; Model Plane only *proposes* a title and
//!   a one-sentence summary from a bounded excerpt. Quarry validates (length,
//!   emptiness, fences) and decides what to emit.
//! * The hop is best-effort and time-boxed. Any failure — transport, 4xx/5xx,
//!   timeout, unparsable JSON — degrades to the host label and never fails
//!   the page.
//! * Cost guards: the excerpt sent is capped, the prompt asks for JSON only,
//!   at most a few calls run concurrently, a rolling per-hour cap bounds
//!   spend, and results are cached by content fingerprint so re-crawls of an
//!   unchanged page never pay twice.
//! * ZDR: the hop is skipped outright when ZDR is active or the privacy
//!   classification forbids third-party processing, and nothing is cached
//!   for such runs. The `page_extracted` event itself is routed through
//!   `EventSink::emit_for_zdr`, so under ZDR it reaches only live
//!   subscribers and never the durable publisher or NATS.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use quarry_core::output::{DriverKind, PageExtraction, TitleSource};
use quarry_core::privacy::{PrivacyClassification, PrivacyPolicy};
use quarry_core::zdr::ZdrMode;
use tokio::sync::Semaphore;
use url::Url;

use crate::mp_client::{ModelPlaneClient, ModelPlaneInvokeRequest};

/// Default excerpt length (chars) carried on the event.
pub const DEFAULT_EXCERPT_CHARS: usize = 300;
/// Hard cap on the display title we accept from any source.
pub const MAX_TITLE_CHARS: usize = 60;
/// Chars of excerpt handed to the model — bounded so a 200 KB page never
/// becomes a 200 KB prompt.
const MODEL_INPUT_CHARS: usize = 1_200;
/// Routed Model Plane alias for the cheapest capable model. The intent layer
/// in inference-core resolves `verevon-budget` to a concrete deployment from
/// the live `RoutingPolicy` (its `budget` ladder / `cheap_fallback`), so no
/// vendor model id is ever hardcoded here. Override with
/// `QUARRY_EDGE__PAGE_TITLE_MODEL`.
pub const DEFAULT_TITLE_MODEL: &str = "verevon-budget";
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(2_000);
const DEFAULT_MAX_CONCURRENT: usize = 3;
const DEFAULT_HOURLY_CAP: u64 = 600;
const CACHE_MAX_ENTRIES: usize = 4_096;

/// Reduce readable markdown to a single-line plain-text excerpt.
///
/// Strips fenced code, headings markers, list bullets, blockquote markers,
/// emphasis, images (keeps alt), links (keeps text), inline HTML, and
/// collapses all whitespace. Truncates on a word boundary at `max_chars`
/// (with an ellipsis) so the result is safe to render verbatim.
pub fn excerpt_from_markdown(markdown: &str, max_chars: usize) -> String {
    let plain = markdown_to_plain(markdown);
    truncate_words(&plain, max_chars)
}

/// Whitespace-separated token count over the plain text of the markdown.
pub fn word_count(markdown: &str) -> u64 {
    markdown_to_plain(markdown).split_whitespace().count() as u64
}

/// Markdown → plain text, whitespace collapsed to single spaces.
///
/// Two passes: a per-line block pass (fences, headings, bullets, quotes,
/// table rules) that joins the surviving lines with spaces, then one inline
/// pass over the joined text so links/images that `html_to_readable_markdown`
/// splits across lines (`[text\n](href)`) are still collapsed to their text.
/// Any markup characters that survive both passes (`[`, `]`, runs of `#`)
/// are dropped so the excerpt never shows raw markdown.
pub fn markdown_to_plain(markdown: &str) -> String {
    let joined = strip_block_markup(markdown);
    let mut inline = String::with_capacity(joined.len());
    strip_inline(&joined, &mut inline);
    let cleaned: String = inline
        .split_whitespace()
        .filter(|tok| !tok.chars().all(|c| c == '#'))
        .map(|tok| tok.replace(['[', ']'], ""))
        .filter(|tok| !tok.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    collapse_ws(&cleaned)
}

fn strip_block_markup(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len().min(8_192));
    let mut in_fence = false;
    for raw_line in markdown.lines() {
        let line = raw_line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if line.is_empty() {
            out.push(' ');
            continue;
        }
        // Table separator rows / horizontal rules carry no text.
        if line
            .chars()
            .all(|c| matches!(c, '-' | '|' | ':' | '=' | '*' | '_' | ' '))
        {
            out.push(' ');
            continue;
        }
        let mut line = line;
        // Heading / blockquote / list markers.
        line = line.trim_start_matches('#').trim_start();
        line = line.trim_start_matches('>').trim_start();
        if let Some(rest) = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| line.strip_prefix("+ "))
        {
            line = rest;
        } else if let Some(idx) = line.find(". ") {
            if idx <= 3 && line[..idx].chars().all(|c| c.is_ascii_digit()) {
                line = &line[idx + 2..];
            }
        }
        out.push_str(line);
        out.push(' ');
    }
    out
}

fn strip_inline(line: &str, out: &mut String) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // Image: ![alt](src) → alt
        if c == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if let Some((text, end)) = bracket_link(line, i + 1) {
                out.push_str(&text);
                i = end;
                continue;
            }
        }
        // Link: [text](href) → text
        if c == b'[' {
            if let Some((text, end)) = bracket_link(line, i) {
                out.push_str(&text);
                i = end;
                continue;
            }
        }
        // Inline HTML tag
        if c == b'<' {
            if let Some(close) = line[i..].find('>') {
                let tag = &line[i + 1..i + close];
                if tag
                    .chars()
                    .next()
                    .map(|ch| ch.is_ascii_alphabetic() || ch == '/' || ch == '!')
                    .unwrap_or(false)
                {
                    out.push(' ');
                    i += close + 1;
                    continue;
                }
            }
        }
        // Emphasis / code markers
        if matches!(c, b'*' | b'_' | b'`' | b'~') {
            i += 1;
            continue;
        }
        // Copy one UTF-8 char.
        let ch_len = utf8_len(c);
        if let Some(s) = line.get(i..i + ch_len) {
            out.push_str(s);
        }
        i += ch_len;
    }
}

/// Parse `[text](href)` starting at `open` (index of `[`). Returns the
/// text (recursively stripped) and the index just past the closing `)`.
fn bracket_link(line: &str, open: usize) -> Option<(String, usize)> {
    let rest = &line[open..];
    let close = rest.find(']')?;
    let after = &rest[close + 1..];
    if !after.starts_with('(') {
        return None;
    }
    let paren_close = after.find(')')?;
    let text = &rest[1..close];
    let mut inner = String::new();
    strip_inline(text, &mut inner);
    Some((inner, open + close + 1 + paren_close + 1))
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out.trim().to_string()
}

/// Truncate to at most `max_chars` chars, preferring a word boundary and
/// appending `…` when anything was cut.
pub fn truncate_words(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let budget = max_chars.saturating_sub(1).max(1);
    let hard: String = text.chars().take(budget).collect();
    let cut = match hard.rfind(' ') {
        Some(idx) if idx >= budget / 2 => hard[..idx].trim_end(),
        _ => hard.trim_end(),
    };
    let cut = cut.trim_end_matches(|c: char| matches!(c, ',' | ';' | ':' | '-' | '–' | '—'));
    format!("{cut}…")
}

/// Host label for a URL (`www.` stripped), or the raw input when unparsable.
pub fn host_label(url: &Url) -> String {
    url.host_str()
        .map(|h| h.trim_start_matches("www.").to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| url.to_string())
}

/// True when `title` is missing or too generic to show as a page name:
/// empty/whitespace, equal to the host (with or without `www.`/scheme),
/// equal to the URL, one of a small stop-list ("Home", "Untitled", "Index",
/// "Welcome", "Forside", "Hjem", "Startside", …), or shorter than 3 chars.
pub fn is_generic_title(title: Option<&str>, url: &Url) -> bool {
    let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) else {
        return true;
    };
    let lower = title.to_lowercase();
    if lower.chars().count() < 3 {
        return true;
    }
    let host = url.host_str().unwrap_or("").to_lowercase();
    let bare_host = host.trim_start_matches("www.");
    let url_lower = url.as_str().trim_end_matches('/').to_lowercase();
    let stripped = lower
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .trim_start_matches("www.");
    if !bare_host.is_empty() && (stripped == bare_host || stripped == host) {
        return true;
    }
    if lower.trim_end_matches('/') == url_lower {
        return true;
    }
    // Some sites use "example.com – Home" or "Home | example.com".
    let without_host: String = if bare_host.is_empty() {
        lower.clone()
    } else {
        lower.replace(bare_host, "")
    };
    let normalized: String = without_host
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    const GENERIC: &[&str] = &[
        "",
        "home",
        "homepage",
        "home page",
        "index",
        "untitled",
        "untitled document",
        "welcome",
        "start",
        "startside",
        "forside",
        "hjem",
        "hjemmeside",
        "startsida",
        "hem",
        "etusivu",
        "startseite",
        "accueil",
        "inicio",
        "main",
        "main page",
        "default",
        "new page",
        "page",
        "site",
        "website",
        "loading",
        "document",
        "just a moment",
        "attention required",
    ];
    GENERIC.contains(&normalized.as_str())
}

/// Whether the Model Plane title hop may run for this request. Mirrors the
/// posture `PrivacyPolicy::guard_third_party_processing` applies to browser
/// providers: ZDR (mode or classification) and credential-bearing content
/// never leave Quarry for enrichment.
pub fn model_hop_allowed(zdr: ZdrMode, privacy: &PrivacyPolicy) -> bool {
    if zdr.is_active() || privacy.zdr.is_active() {
        return false;
    }
    !matches!(
        privacy.privacy_classification,
        PrivacyClassification::ZdrEphemeral | PrivacyClassification::CredentialOrSecret
    )
}

/// Model-proposed title (and optional summary) after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTitle {
    pub title: String,
    pub summary: Option<String>,
}

/// Best-effort, time-boxed, cost-guarded Model Plane title enricher.
pub struct PageTitleEnricher {
    client: Arc<ModelPlaneClient>,
    model: String,
    timeout: Duration,
    hourly_cap: u64,
    permits: Semaphore,
    cache: Mutex<HashMap<String, Option<ModelTitle>>>,
    window_started: Mutex<Instant>,
    window_calls: AtomicU64,
    total_calls: AtomicU64,
    total_cache_hits: AtomicU64,
}

impl PageTitleEnricher {
    pub fn new(client: Arc<ModelPlaneClient>) -> Self {
        Self {
            client,
            model: DEFAULT_TITLE_MODEL.to_string(),
            timeout: DEFAULT_TIMEOUT,
            hourly_cap: DEFAULT_HOURLY_CAP,
            permits: Semaphore::new(DEFAULT_MAX_CONCURRENT),
            cache: Mutex::new(HashMap::new()),
            window_started: Mutex::new(Instant::now()),
            window_calls: AtomicU64::new(0),
            total_calls: AtomicU64::new(0),
            total_cache_hits: AtomicU64::new(0),
        }
    }

    /// Routed model alias (never a vendor id). Empty/whitespace keeps the default.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        let model = model.into();
        if !model.trim().is_empty() {
            self.model = model.trim().to_string();
        }
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_hourly_cap(mut self, cap: u64) -> Self {
        self.hourly_cap = cap;
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn calls(&self) -> u64 {
        self.total_calls.load(Ordering::Relaxed)
    }

    pub fn cache_hits(&self) -> u64 {
        self.total_cache_hits.load(Ordering::Relaxed)
    }

    fn within_budget(&self) -> bool {
        let mut started = self
            .window_started
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if started.elapsed() >= Duration::from_secs(3_600) {
            *started = Instant::now();
            self.window_calls.store(0, Ordering::Relaxed);
        }
        self.window_calls.load(Ordering::Relaxed) < self.hourly_cap
    }

    /// Resolve a clean title for a page whose HTML title was generic.
    ///
    /// `fingerprint` is the content fingerprint used as the cache key;
    /// `org_id` is the verified request org (needed for production token
    /// minting). `cache_writes = false` (ZDR posture) bypasses the cache in
    /// both directions. Returns `None` on any failure or when the budget is
    /// spent.
    pub async fn enrich(
        &self,
        org_id: Option<&str>,
        url: &Url,
        lang: Option<&str>,
        excerpt: &str,
        fingerprint: &str,
        cache_writes: bool,
    ) -> Option<ModelTitle> {
        let excerpt = excerpt.trim();
        if excerpt.chars().count() < 20 {
            return None;
        }
        let cache_key = format!("{}|{}", self.model, fingerprint);
        if cache_writes {
            if let Some(hit) = self
                .cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&cache_key)
                .cloned()
            {
                self.total_cache_hits.fetch_add(1, Ordering::Relaxed);
                return hit;
            }
        }
        if !self.within_budget() {
            tracing::debug!(url = %url, "page-title enrichment skipped: hourly cap reached");
            return None;
        }
        let _permit = match tokio::time::timeout(self.timeout / 2, self.permits.acquire()).await
        {
            Ok(Ok(p)) => p,
            _ => {
                tracing::debug!(url = %url, "page-title enrichment skipped: concurrency saturated");
                return None;
            }
        };
        self.window_calls.fetch_add(1, Ordering::Relaxed);
        self.total_calls.fetch_add(1, Ordering::Relaxed);

        let bounded: String = excerpt.chars().take(MODEL_INPUT_CHARS).collect();
        let prompt = build_prompt(url, lang, &bounded);
        let req = ModelPlaneInvokeRequest {
            content: prompt,
            model: Some(self.model.clone()),
            session_key: None,
            thread_id: None,
        };
        let started = Instant::now();
        let call = async {
            match org_id {
                Some(org) => self.client.invoke_for_org(org, &req).await,
                None => self.client.invoke(&req).await,
            }
        };
        let result = match tokio::time::timeout(self.timeout, call).await {
            Ok(Ok(resp)) => parse_model_title(&resp.content, url),
            Ok(Err(e)) => {
                tracing::warn!(error = %e, url = %url, model = %self.model, "page-title enrichment failed");
                None
            }
            Err(_) => {
                tracing::warn!(
                    url = %url,
                    model = %self.model,
                    timeout_ms = self.timeout.as_millis() as u64,
                    "page-title enrichment timed out"
                );
                None
            }
        };
        tracing::info!(
            url = %url,
            model = %self.model,
            elapsed_ms = started.elapsed().as_millis() as u64,
            resolved = result.is_some(),
            "page-title enrichment"
        );
        if cache_writes {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if cache.len() >= CACHE_MAX_ENTRIES {
                cache.clear();
            }
            cache.insert(cache_key, result.clone());
        }
        result
    }
}

fn build_prompt(url: &Url, lang: Option<&str>, excerpt: &str) -> String {
    let lang_hint = match lang.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => format!("The page language is `{l}`; answer in that language."),
        None => "Answer in the language of the excerpt.".to_string(),
    };
    format!(
        "You name web pages for a knowledge base. Given the page URL and an excerpt of its \
readable text, return ONLY a JSON object {{\"title\": string, \"summary\": string}}.\n\
title: a clean, specific page title of at most {MAX_TITLE_CHARS} characters — no site-name suffix, \
no quotes, no trailing punctuation, not just the domain, not \"Home\".\n\
summary: ONE plain sentence (max 160 characters) saying what the page is about.\n\
{lang_hint}\n\nURL: {url}\nEXCERPT:\n{excerpt}"
    )
}

/// Validate the model reply: JSON object with a non-empty, non-generic
/// `title` ≤ 60 chars; `summary` optional, one line, ≤ 200 chars.
pub fn parse_model_title(body: &str, url: &Url) -> Option<ModelTitle> {
    let stripped = strip_json_fences(body);
    let value: serde_json::Value = serde_json::from_str(stripped).ok().or_else(|| {
        // Tolerate prose around the object.
        let start = stripped.find('{')?;
        let end = stripped.rfind('}')?;
        serde_json::from_str(&stripped[start..=end]).ok()
    })?;
    let title = value.get("title")?.as_str()?;
    let title = clean_title(title)?;
    if is_generic_title(Some(&title), url) {
        return None;
    }
    let summary = value
        .get("summary")
        .and_then(|s| s.as_str())
        .map(collapse_ws)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_words(&s, 200));
    Some(ModelTitle { title, summary })
}

/// Normalize a candidate title: collapse whitespace, strip wrapping quotes and
/// trailing punctuation, cap at [`MAX_TITLE_CHARS`]. `None` when empty.
pub fn clean_title(raw: &str) -> Option<String> {
    let mut t = collapse_ws(raw);
    t = t
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '“' | '”' | '«' | '»' | '`'))
        .trim_end_matches(|c: char| {
            matches!(c, '.' | ',' | ';' | ':' | '!' | '-' | '–' | '—' | '|')
        })
        .trim()
        .to_string();
    if t.is_empty() {
        return None;
    }
    if t.chars().count() > MAX_TITLE_CHARS {
        t = truncate_words(&t, MAX_TITLE_CHARS);
    }
    Some(t)
}

fn strip_json_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim().trim_end_matches("```").trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim().trim_end_matches("```").trim();
    }
    s
}

/// Inputs the pipeline hands to [`build_extraction`].
pub struct ExtractionInput<'a> {
    pub url: &'a Url,
    pub html_title: Option<&'a str>,
    pub markdown: &'a str,
    pub lang: Option<&'a str>,
    pub driver: DriverKind,
}

/// Pure part of the extraction: excerpt, word count, and the HTML/host title
/// decision. The model hop (if any) is layered on by the caller via
/// [`apply_model_title`].
pub fn build_extraction(input: ExtractionInput<'_>) -> PageExtraction {
    let excerpt = excerpt_from_markdown(input.markdown, DEFAULT_EXCERPT_CHARS);
    let word_count = word_count(input.markdown);
    let (title, title_source) = if is_generic_title(input.html_title, input.url) {
        (host_label(input.url), TitleSource::Host)
    } else {
        (
            clean_title(input.html_title.unwrap_or_default())
                .unwrap_or_else(|| host_label(input.url)),
            TitleSource::Html,
        )
    };
    PageExtraction {
        url: input.url.to_string(),
        title,
        title_source,
        excerpt,
        summary: None,
        word_count,
        lang: input
            .lang
            .map(str::to_string)
            .filter(|l| !l.is_empty()),
        driver: input.driver,
    }
}

/// Overlay a validated model title onto a host-titled extraction.
pub fn apply_model_title(extraction: &mut PageExtraction, model: ModelTitle) {
    extraction.title = model.title;
    extraction.title_source = TitleSource::Model;
    extraction.summary = model.summary;
}

/// Serialize an extraction as the `page_extracted` event payload. Kept as a
/// function so the event shape has exactly one definition.
pub fn event_payload(
    extraction: &PageExtraction,
    content_type: Option<&str>,
    fingerprint: &str,
) -> serde_json::Value {
    let mut value =
        serde_json::to_value(extraction).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(map) = value.as_object_mut() {
        map.insert(
            "content_type".into(),
            content_type
                .map(|c| serde_json::Value::String(c.to_string()))
                .unwrap_or(serde_json::Value::Null),
        );
        map.insert(
            "fingerprint".into(),
            serde_json::Value::String(fingerprint.to_string()),
        );
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn excerpt_strips_markup_and_collapses_whitespace() {
        let md = "# Aquatiq\n\nWe deliver **hygiene** solutions to the [food industry](https://x.y).\n\n- item one\n- item two\n\n```\ncode here\n```\n\n![logo](https://img)\n\n> quoted   text\n";
        let ex = excerpt_from_markdown(md, 300);
        assert_eq!(
            ex,
            "Aquatiq We deliver hygiene solutions to the food industry. item one item two logo quoted text"
        );
        assert!(!ex.contains('#') && !ex.contains('*') && !ex.contains('['));
    }

    // Live aquatiq.com shape: readability emits links split across lines and
    // headings that end up mid-line once lines are joined.
    #[test]
    fn excerpt_collapses_multiline_links_and_stray_markup() {
        let md = "Spesialisert kjemi for matindustrien.\n\n[Næringsmiddel \n](/no/naeringsmiddelindustri)[Transport\n](/no/transport)\n\nHvordan kan vi hjelpe deg? ### Aquatiq tilbyr [ Chemistry\n\n### Kjemi";
        let ex = excerpt_from_markdown(md, 300);
        assert_eq!(
            ex,
            "Spesialisert kjemi for matindustrien. Næringsmiddel Transport Hvordan kan vi hjelpe deg? Aquatiq tilbyr Chemistry Kjemi"
        );
    }

    #[test]
    fn excerpt_truncates_on_word_boundary_with_ellipsis() {
        let md = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let ex = excerpt_from_markdown(md, 22);
        assert!(ex.chars().count() <= 22, "{ex}");
        assert!(ex.ends_with('…'));
        assert_eq!(ex, "alpha beta gamma…");
    }

    #[test]
    fn excerpt_handles_multibyte_text() {
        let md = "Vi leverer løsninger for næringsmiddelindustrien – trygg mat, hver dag. Ærlig og åpen.";
        let ex = excerpt_from_markdown(md, 40);
        assert!(ex.chars().count() <= 40);
        assert!(ex.starts_with("Vi leverer løsninger"));
    }

    #[test]
    fn word_count_ignores_markup() {
        assert_eq!(word_count("# One\n\n**two** [three](x) `four`"), 4);
        assert_eq!(word_count(""), 0);
    }

    #[test]
    fn generic_titles_are_detected() {
        let url = u("https://www.aquatiq.com/");
        for t in [
            None,
            Some(""),
            Some("   "),
            Some("aquatiq.com"),
            Some("www.aquatiq.com"),
            Some("https://www.aquatiq.com"),
            Some("Home"),
            Some("HOME"),
            Some("Forside"),
            Some("Untitled"),
            Some("Index"),
            Some("Aquatiq.com – Home"),
            Some("Home | aquatiq.com"),
            Some("Hi"),
        ] {
            assert!(is_generic_title(t, &url), "{t:?} should be generic");
        }
    }

    #[test]
    fn specific_titles_are_kept() {
        let url = u("https://www.aquatiq.com/");
        for t in [
            "Aquatiq – Hygiene solutions for the food industry",
            "Om oss",
            "Contact Aquatiq",
            "Kjemikalier og rengjøringsutstyr",
        ] {
            assert!(!is_generic_title(Some(t), &url), "{t} should be specific");
        }
    }

    #[test]
    fn host_label_strips_www() {
        assert_eq!(host_label(&u("https://www.aquatiq.com/about")), "aquatiq.com");
        assert_eq!(host_label(&u("https://shop.example.no/")), "shop.example.no");
    }

    #[test]
    fn build_extraction_falls_back_to_host_for_generic_title() {
        let url = u("https://aquatiq.com/");
        let ex = build_extraction(ExtractionInput {
            url: &url,
            html_title: Some("aquatiq.com"),
            markdown: "Vi leverer hygieneløsninger til næringsmiddelindustrien.",
            lang: Some("no"),
            driver: DriverKind::Browser,
        });
        assert_eq!(ex.title, "aquatiq.com");
        assert_eq!(ex.title_source, TitleSource::Host);
        assert_eq!(ex.word_count, 5);
        assert_eq!(ex.lang.as_deref(), Some("no"));
        assert_eq!(ex.driver, DriverKind::Browser);
        assert!(ex.excerpt.starts_with("Vi leverer"));
    }

    #[test]
    fn build_extraction_keeps_specific_html_title() {
        let url = u("https://aquatiq.com/om-oss");
        let ex = build_extraction(ExtractionInput {
            url: &url,
            html_title: Some("  Om oss – Aquatiq  "),
            markdown: "Aquatiq er ...",
            lang: None,
            driver: DriverKind::Static,
        });
        assert_eq!(ex.title, "Om oss – Aquatiq");
        assert_eq!(ex.title_source, TitleSource::Html);
    }

    #[test]
    fn apply_model_title_marks_provenance() {
        let url = u("https://aquatiq.com/");
        let mut ex = build_extraction(ExtractionInput {
            url: &url,
            html_title: None,
            markdown: "text text text",
            lang: None,
            driver: DriverKind::Tls,
        });
        apply_model_title(
            &mut ex,
            ModelTitle {
                title: "Aquatiq – hygiene for matindustrien".into(),
                summary: Some("Leverandør av hygieneløsninger.".into()),
            },
        );
        assert_eq!(ex.title_source, TitleSource::Model);
        assert_eq!(ex.title, "Aquatiq – hygiene for matindustrien");
        assert_eq!(
            ex.summary.as_deref(),
            Some("Leverandør av hygieneløsninger.")
        );
    }

    #[test]
    fn parse_model_title_validates_and_cleans() {
        let url = u("https://aquatiq.com/");
        let ok = parse_model_title(
            "```json\n{\"title\": \"\\\"Aquatiq – Hygiene for the food industry.\\\"\", \"summary\": \"  Supplier of\\n hygiene   systems. \"}\n```",
            &url,
        )
        .expect("valid reply");
        assert_eq!(ok.title, "Aquatiq – Hygiene for the food industry");
        assert_eq!(ok.summary.as_deref(), Some("Supplier of hygiene systems."));

        // Generic model titles are rejected rather than shown.
        assert!(parse_model_title("{\"title\": \"Home\"}", &url).is_none());
        assert!(parse_model_title("{\"title\": \"aquatiq.com\"}", &url).is_none());
        // Garbage is rejected.
        assert!(parse_model_title("not json", &url).is_none());
        assert!(parse_model_title("{\"summary\": \"x\"}", &url).is_none());
        // Prose around the object is tolerated.
        assert!(
            parse_model_title("Sure! {\"title\": \"Om Aquatiq\"} hope that helps", &url).is_some()
        );
    }

    #[test]
    fn parse_model_title_caps_length() {
        let url = u("https://example.com/");
        let long = "A ".repeat(80);
        let got = parse_model_title(&format!("{{\"title\": \"{long}\"}}"), &url).unwrap();
        assert!(got.title.chars().count() <= MAX_TITLE_CHARS);
    }

    #[test]
    fn model_hop_respects_zdr_and_classification() {
        let open = PrivacyPolicy::default();
        assert!(model_hop_allowed(ZdrMode::Off, &open));
        assert!(!model_hop_allowed(ZdrMode::On, &open));
        let zdr_policy = PrivacyPolicy::default().with_zdr(ZdrMode::On);
        assert!(!model_hop_allowed(ZdrMode::Off, &zdr_policy));
        let mut secret = PrivacyPolicy::default();
        secret.privacy_classification = PrivacyClassification::CredentialOrSecret;
        assert!(!model_hop_allowed(ZdrMode::Off, &secret));
        let mut ephemeral = PrivacyPolicy::default();
        ephemeral.privacy_classification = PrivacyClassification::ZdrEphemeral;
        assert!(!model_hop_allowed(ZdrMode::Off, &ephemeral));
    }

    #[test]
    fn event_payload_has_stable_shape() {
        let url = u("https://aquatiq.com/");
        let ex = build_extraction(ExtractionInput {
            url: &url,
            html_title: Some("Om oss"),
            markdown: "hello world",
            lang: Some("en"),
            driver: DriverKind::Static,
        });
        let payload = event_payload(&ex, Some("text/html"), "blake3:abc");
        assert_eq!(payload["url"], "https://aquatiq.com/");
        assert_eq!(payload["title"], "Om oss");
        assert_eq!(payload["title_source"], "html");
        assert_eq!(payload["excerpt"], "hello world");
        assert_eq!(payload["word_count"], 2);
        assert_eq!(payload["driver"], "static");
        assert_eq!(payload["content_type"], "text/html");
        assert_eq!(payload["fingerprint"], "blake3:abc");
        assert!(payload.get("summary").is_none());
    }

    #[test]
    fn default_model_is_a_routed_alias_not_a_vendor_id() {
        assert!(DEFAULT_TITLE_MODEL.starts_with("verevon-"));
        assert!(!DEFAULT_TITLE_MODEL.contains("gpt") && !DEFAULT_TITLE_MODEL.contains("claude"));
    }
}
