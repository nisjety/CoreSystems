//! Challenge classification and the one-recovery rule (WS3.H).
//!
//! Quarry classifies what a page says about access and refuses to repeat a
//! failed recovery. It does not evade anything: no marker here changes the
//! browser's identity, rotates a proxy or retries on its own. The planner (or a
//! person, through user take-over) decides what happens next.
//!
//! Taxonomy. A page is one of:
//!
//! * **strong**: a bot wall stands in front of it. The closed set of reasons is
//!   [`BlockReason`]: `captcha_widget`, `waf_interstitial`,
//!   `unexpected_bot_error` (two independent markers) and `challenge_loop`
//!   (a strong signal again after the one allowed recovery, assigned by
//!   [`BlockEpisodes`], never by [`classify`]).
//! * **weak**: a hint worth a look at the screenshot (one marker of the second
//!   class, a widget on a page that is otherwise content). Not counted, no
//!   escalation.
//! * **non-signal**: an ordinary access state that a planner must not take for
//!   a bot wall. Its first evidence entry names it: `bare_status`,
//!   `rate_limited`, `auth_required`, `consent_required`, `paywall`,
//!   `geo_blocked`, `server_error` ([`NON_SIGNAL_LABELS`]). A `timeout` is a
//!   driver error, not a page, so it is never produced here.
//!
//! Marker classes, in order of weight:
//!
//! * **A**, infrastructure identifiers in the HTML source (a CAPTCHA widget
//!   script, a WAF interstitial's own variables). One is enough for the
//!   interstitials, which never appear on a served page; a widget also needs the
//!   page to look like the challenge itself (see [`classify`]).
//! * **B**, phrases in the title or the visible text of a short page, grouped so
//!   that two phrases about the same thing count once. Two independent groups
//!   (a title hit counts twice, an error status counts once) make a strong
//!   signal; one is weak.
//! * **C**, single words ("captcha", "forbidden") that never decide anything on
//!   their own. Prose about CAPTCHAs, an article titled "How CAPTCHAs work", a
//!   sentence containing "forbidden" produce no signal.
//!
//! Nothing in a signal's evidence is page text: only marker ids and the status.

use std::collections::{BTreeMap, HashMap};

use quarry_browser::BrowserDevtoolsEvent;
use quarry_core::contracts::{
    AgentAction, BlockReason, ChallengeKind, ChallengeRecovery, ChallengeSignal, ChallengeStrength,
};
use quarry_core::error::{QuarryError, QuarryResult};
use quarry_core::zdr::ZdrMode;

/// Names of the non-signals, the first evidence entry of each. `timeout` is
/// part of the taxonomy but is a driver error: no page is classified as one.
pub const NON_SIGNAL_LABELS: [&str; 8] = [
    "bare_status",
    "rate_limited",
    "auth_required",
    "consent_required",
    "paywall",
    "geo_blocked",
    "server_error",
    "timeout",
];

/// A page with more visible text than this is content, not a challenge page.
const THIN_PAGE_CHARS: usize = 3000;
/// HTML scanned for markers. A challenge page is small; anything past this is
/// not looked at.
const MAX_SCANNED_BYTES: usize = 512 * 1024;
/// A host that shows this many strong signals in one run is looping even if
/// every one of them was followed by a recovery that "worked".
const MAX_STRONG_PER_HOST: u32 = 3;

const CONF_A_BASE: f32 = 0.95;
const CONF_A_MAX: f32 = 0.99;
const CONF_B_BASE: f32 = 0.85;
const CONF_B_MAX: f32 = 0.92;
const CONF_WEAK: f32 = 0.5;
const CONF_WEAK_TITLE: f32 = 0.4;
const CONF_NON_SIGNAL_STATUS: f32 = 0.95;
const CONF_NON_SIGNAL_STRUCTURE: f32 = 0.9;
const CONF_NON_SIGNAL_PHRASE: f32 = 0.7;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Which classifier runs. `Legacy` is the substring classifier of before WS3.H,
/// kept for one release as a rollback: it flags login pages and prose as
/// escalations, and a run under it is not subject to the one-recovery rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifierMode {
    V2,
    Legacy,
}

/// What happens when the one-recovery rule says no. `Log` records a
/// `would_reject` line and lets the action through, for the period in which the
/// corpus is still being validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforceMode {
    Enforce,
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChallengeConfig {
    pub classifier: ClassifierMode,
    pub enforce: EnforceMode,
}

impl Default for ChallengeConfig {
    fn default() -> Self {
        Self {
            classifier: ClassifierMode::V2,
            enforce: EnforceMode::Enforce,
        }
    }
}

impl ChallengeConfig {
    /// `QUARRY_CHALLENGE_CLASSIFIER=v2|legacy` and `QUARRY_CHALLENGE_ENFORCE=
    /// enforce|log`. Anything else is the default.
    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var("QUARRY_CHALLENGE_CLASSIFIER").ok().as_deref(),
            std::env::var("QUARRY_CHALLENGE_ENFORCE").ok().as_deref(),
        )
    }

    /// The pure half of [`from_env`](Self::from_env), so a test never touches
    /// the process environment.
    pub fn from_values(classifier: Option<&str>, enforce: Option<&str>) -> Self {
        let classifier = match classifier.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("legacy") => ClassifierMode::Legacy,
            _ => ClassifierMode::V2,
        };
        let enforce = match enforce.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("log") => EnforceMode::Log,
            _ => EnforceMode::Enforce,
        };
        Self {
            classifier,
            enforce,
        }
    }
}

// ---------------------------------------------------------------------------
// Input and marker tables
// ---------------------------------------------------------------------------

/// What the classifier looks at. Every field is borrowed from the observation
/// that was already built; nothing here is fetched again.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChallengeInput<'a> {
    pub url: &'a str,
    pub title: Option<&'a str>,
    /// The HTML of the page as the browser holds it. `None` for a caller that
    /// only has text (the legacy wrapper); class A markers then cannot match.
    pub html: Option<&'a str>,
    pub text_snippet: Option<&'a str>,
    /// The status of the main document, from the devtools events.
    pub main_doc_status: Option<u16>,
}

/// Class A, interstitials: identifiers that only an interstitial carries.
/// `(id, needles)`, matched against the lower-cased HTML source.
///
/// `/cdn-cgi/challenge-platform/scripts/jsd/` is deliberately absent: Cloudflare
/// injects that script into ordinary, served pages for bot scoring.
const INTERSTITIAL_MARKERS: &[(&str, &[&str])] = &[
    (
        "cloudflare_challenge",
        &[
            "_cf_chl_opt",
            "orchestrate/chl_page",
            "orchestrate/jsch",
            "cf-challenge-running",
        ],
    ),
    ("akamai_reference", &["errors.edgesuite.net"]),
    (
        "imperva_incapsula",
        &["_incapsula_resource", "incapsula incident id"],
    ),
];

/// Class A, widgets: a CAPTCHA or proof-of-humanity widget in the page.
const WIDGET_MARKERS: &[(&str, &[&str])] = &[
    (
        "cloudflare_turnstile",
        &["challenges.cloudflare.com/turnstile", "cf-turnstile"],
    ),
    (
        "hcaptcha",
        &[
            "hcaptcha.com/1/api.js",
            "js.hcaptcha.com",
            "newassets.hcaptcha.com",
            "hcaptcha.com/captcha/",
            "h-captcha",
        ],
    ),
    ("datadome_captcha", &["captcha-delivery.com"]),
    ("human_px_captcha", &["px-captcha", "captcha.px-cdn.net"]),
    (
        "aws_waf_captcha",
        &["awswafcaptcha", "aws-waf-captcha", "captcha.awswaf.com"],
    ),
    ("arkose_funcaptcha", &["arkoselabs.com", "funcaptcha"]),
    ("geetest", &["geetest.com/"]),
];

/// Class B, text groups: `(group id, phrases)` matched against the lower-cased
/// title and visible text. Two phrases of one group count once.
const TEXT_GROUPS: &[(&str, &[&str])] = &[
    (
        "unusual_traffic",
        &[
            "unusual traffic",
            "unusual activity from your",
            "suspicious activity from your",
            "suspicious traffic",
        ],
    ),
    (
        "automated_access",
        &[
            "automated queries",
            "automated requests",
            "automated access",
            "automated software",
            "robotic activity",
            "bots and scrapers",
        ],
    ),
    (
        "prove_human",
        &[
            "verify you are human",
            "verify you are a human",
            "verify that you are human",
            "verify that you are not a robot",
            "confirm you are human",
            "confirm that you are human",
            "prove you are human",
            "prove you're human",
            "i'm not a robot",
            "i am not a robot",
            "are you a robot",
            "are you human",
            "verifying you are human",
            "complete the security check",
            "press & hold",
            "press and hold to confirm",
        ],
    ),
    (
        "browser_check",
        &[
            "checking your browser",
            "checking if the site connection is secure",
            "just a moment...",
            "just a moment\u{2026}",
            "enable javascript and cookies to continue",
            "performing security verification",
            "performing a security check",
            "verifying your browser",
            "one more step",
        ],
    ),
    (
        "blocked_message",
        &[
            "you have been blocked",
            "you've been blocked",
            "your request has been blocked",
            "this request was blocked",
            "has been blocked by",
            "access to this page has been denied",
            "pardon our interruption",
            "attention required",
        ],
    ),
    ("ray_id", &["ray id:", "cloudflare ray id"]),
];

/// Groups that point at an interstitial rather than at a bot-suspicion message.
const INTERSTITIAL_GROUPS: &[&str] = &["browser_check", "blocked_message", "ray_id", "denied_reference"];

/// Paths that are a challenge by name.
const CHALLENGE_PATHS: &[&str] = &[
    "/cdn-cgi/l/chk_captcha",
    "/cdn-cgi/challenge-platform",
    "/sorry/index",
    "/_incapsula_resource",
    "/distil_r_captcha",
    "/captcha",
];

/// Title cues that make a widget on a short page the challenge itself.
const TITLE_CUES: &[&str] = &[
    "just a moment",
    "attention required",
    "verify you are",
    "verifying you",
    "are you human",
    "are you a robot",
    "robot",
    "security check",
    "access denied",
    "you have been blocked",
    "pardon",
    "captcha",
    "one more step",
    "checking your browser",
    "challenge",
];

const PAYWALL_PHRASES: &[&str] = &[
    "subscribe to continue reading",
    "subscribe to read",
    "subscribe to keep reading",
    "continue reading with a subscription",
    "this content is for subscribers",
    "subscribers only",
    "reached your free article limit",
    "reached your article limit",
    "unlock this article",
    "become a member to continue",
];

const GEO_PHRASES: &[&str] = &[
    "not available in your country",
    "not available in your region",
    "not available in your location",
    "not available in your area",
    "unavailable in your country",
    "unavailable in your region",
    "blocked in your country",
    "geo-restricted",
];

const RATE_PHRASES: &[&str] = &[
    "too many requests",
    "rate limit exceeded",
    "you are being rate limited",
];

const CONSENT_PHRASES: &[&str] = &[
    "we use cookies",
    "this website uses cookies",
    "this site uses cookies",
    "accept all cookies",
    "accept cookies",
    "cookie consent",
    "cookie preferences",
    "cookie settings",
    "consent required",
];

const CMP_MARKERS: &[&str] = &[
    "onetrust",
    "cookiebot",
    "didomi",
    "usercentrics",
    "quantcast",
    "trustarc",
    "cookieconsent",
    "cookie-consent",
    "cookie-banner",
    "consent-banner",
    "osano",
    "iubenda",
    "klaro",
];

const DENIED_PHRASES: &[&str] = &["access denied", "forbidden", "403 forbidden"];

// ---------------------------------------------------------------------------
// Page facts
// ---------------------------------------------------------------------------

struct Page {
    status: Option<u16>,
    /// Lower-cased HTML source, capped. `None` when the caller had no HTML.
    source: Option<String>,
    title: String,
    /// Lower-cased visible text, capped (the snippet when there is no HTML).
    text: String,
    path: String,
    host: String,
    thin: bool,
    has_password: bool,
}

impl Page {
    fn build(input: &ChallengeInput<'_>) -> Self {
        let source = input
            .html
            .map(|html| cap_str(html, MAX_SCANNED_BYTES).to_ascii_lowercase());
        let snippet = input.text_snippet.unwrap_or_default();
        let (text, thin) = match source.as_deref() {
            Some(src) => {
                let text = visible_text(src);
                let thin = text.chars().count() <= THIN_PAGE_CHARS;
                (text, thin)
            }
            None => {
                // A snippet is cut at 500 bytes with a trailing "...": a cut
                // one says nothing about how long the page is.
                let cut = snippet.trim_end().ends_with("...");
                (snippet.to_ascii_lowercase(), !cut)
            }
        };
        let (path, host) = url::Url::parse(input.url)
            .ok()
            .map(|u| {
                (
                    u.path().to_ascii_lowercase(),
                    u.host_str().unwrap_or_default().to_ascii_lowercase(),
                )
            })
            .unwrap_or_default();
        let has_password = source.as_deref().is_some_and(|src| {
            ["type=\"password\"", "type='password'", "type=password"]
                .iter()
                .any(|needle| src.contains(needle))
        });
        Self {
            status: input.main_doc_status,
            source,
            title: input.title.unwrap_or_default().to_ascii_lowercase(),
            text,
            path,
            host,
            thin,
            has_password,
        }
    }

    fn error_status(&self) -> bool {
        matches!(self.status, Some(403 | 429 | 503))
    }

    fn status_2xx_or_unknown(&self) -> bool {
        self.status.is_none_or(|s| (200..300).contains(&s))
    }

    fn title_has(&self, needles: &[&str]) -> bool {
        needles.iter().any(|n| self.title.contains(n))
    }

    fn text_has(&self, needles: &[&str]) -> bool {
        needles.iter().any(|n| self.text.contains(n))
    }

    fn source_has(&self, needles: &[&str]) -> bool {
        self.source
            .as_deref()
            .is_some_and(|src| needles.iter().any(|n| src.contains(n)))
    }

    fn status_evidence(&self) -> Option<String> {
        self.status.map(|s| format!("status:{s}"))
    }
}

/// `s` cut to at most `max` bytes on a character boundary.
fn cap_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The visible text of lower-cased HTML: tags, comments, and the bodies of
/// script, style, template and svg elements removed, entities decoded,
/// whitespace collapsed. An approximation made for length and phrase checks,
/// not a renderer.
fn visible_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 4);
    let mut i = 0;
    let len = html.len();
    while i < len {
        if html.as_bytes()[i] == b'<' {
            if html[i..].starts_with("<!--") {
                i = html[i..].find("-->").map_or(len, |p| i + p + 3);
                out.push(' ');
                continue;
            }
            let rest = &html[i + 1..];
            let name_end = rest
                .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .unwrap_or(rest.len());
            let name = &rest[..name_end];
            i = rest.find('>').map_or(len, |p| i + 1 + p + 1);
            if matches!(name, "script" | "style" | "template" | "svg") {
                let close = format!("</{name}");
                i = html[i..]
                    .find(&close)
                    .map(|p| i + p)
                    .and_then(|p| html[p..].find('>').map(|q| p + q + 1))
                    .unwrap_or(len);
            }
            out.push(' ');
        } else {
            let next = html[i..].find('<').map_or(len, |p| i + p);
            out.push_str(&decode_entities(&html[i..next]));
            i = next;
        }
    }
    let mut collapsed = String::with_capacity(out.len());
    for word in out.split_whitespace() {
        if !collapsed.is_empty() {
            collapsed.push(' ');
        }
        collapsed.push_str(word);
    }
    collapsed
}

fn decode_entities(chunk: &str) -> String {
    if !chunk.contains('&') {
        return chunk.to_owned();
    }
    let mut out = String::with_capacity(chunk.len());
    let mut rest = chunk;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let decoded = tail.find(';').filter(|end| *end <= 10).and_then(|end| {
            let entity = &tail[1..end];
            let ch = match entity {
                "amp" => Some('&'),
                "nbsp" => Some(' '),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "lt" => Some('<'),
                "gt" => Some('>'),
                _ => entity.strip_prefix('#').and_then(|num| {
                    let code = match num.strip_prefix('x') {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => num.parse::<u32>().ok(),
                    };
                    code.and_then(char::from_u32)
                }),
            };
            ch.map(|c| (c, end + 1))
        });
        match decoded {
            Some((c, used)) => {
                out.push(c);
                rest = &tail[used..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn recaptcha_widget(src: &str) -> bool {
    // v3 is invisible and sits on ordinary pages; its anchor iframe says so.
    if src.contains("size=invisible") {
        return false;
    }
    src.contains("class=\"g-recaptcha")
        || src.contains("class='g-recaptcha")
        || src.contains("recaptcha/api2/anchor")
        || src.contains("recaptcha/api2/bframe")
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// A group that matched, and whether it matched in the title.
struct GroupHit {
    id: &'static str,
    in_title: bool,
}

fn text_group_hits(page: &Page) -> Vec<GroupHit> {
    // Phrases only count on a page that is short or that came with an error
    // status: a long article may say anything.
    if !(page.thin || page.error_status()) {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (id, phrases) in TEXT_GROUPS {
        let in_title = page.title_has(phrases);
        if in_title || page.text_has(phrases) {
            hits.push(GroupHit { id: *id, in_title });
        }
    }
    if page.text.contains("access denied")
        && (page.text.contains("reference #") || page.text.contains("incident id"))
    {
        hits.push(GroupHit {
            id: "denied_reference",
            in_title: false,
        });
    }
    if CHALLENGE_PATHS.iter().any(|p| page.path.contains(p)) {
        hits.push(GroupHit {
            id: "url",
            in_title: false,
        });
    }
    hits
}

fn matched_markers(page: &Page, table: &'static [(&'static str, &'static [&'static str])]) -> Vec<&'static str> {
    table
        .iter()
        .filter(|(_, needles)| page.source_has(needles))
        .map(|(id, _)| *id)
        .collect()
}

fn signal(
    kind: ChallengeKind,
    strength: ChallengeStrength,
    confidence: f32,
    evidence: Vec<String>,
    block_reason: Option<BlockReason>,
) -> ChallengeSignal {
    let strong = strength == ChallengeStrength::Strong;
    ChallengeSignal {
        kind,
        confidence,
        evidence,
        requires_escalation: strong,
        strength,
        block_reason,
        recovery: if strong {
            ChallengeRecovery::Available
        } else {
            ChallengeRecovery::NotApplicable
        },
    }
}

fn non_signal(kind: ChallengeKind, confidence: f32, label: &str, page: &Page) -> ChallengeSignal {
    let mut evidence = vec![label.to_owned()];
    evidence.extend(page.status_evidence());
    signal(kind, ChallengeStrength::NonSignal, confidence, evidence, None)
}

/// Classify one observed page. `None` means the page says nothing about
/// access. A returned signal is advisory: it names the state and never asks
/// Quarry to act on it.
///
/// Order: strong (an interstitial marker; a widget on a page that is itself
/// the challenge; two independent text groups), a widget hint, the
/// non-signals, then single-group hints.
pub fn classify(input: &ChallengeInput<'_>) -> Option<ChallengeSignal> {
    let page = Page::build(input);
    let interstitials = matched_markers(&page, INTERSTITIAL_MARKERS);
    let mut widgets = matched_markers(&page, WIDGET_MARKERS);
    if page.source.as_deref().is_some_and(recaptcha_widget) {
        widgets.push("google_recaptcha");
    }
    let hits = text_group_hits(&page);

    if let Some(strong) = classify_strong(&page, &interstitials, &widgets, &hits) {
        return Some(strong);
    }
    if !widgets.is_empty() && (page.thin || page.has_password) {
        let mut evidence = vec!["captcha_widget_present".to_owned()];
        evidence.extend(widgets.iter().map(|w| (*w).to_owned()));
        if page.has_password {
            evidence.push("auth_form".to_owned());
        }
        evidence.extend(page.status_evidence());
        return Some(signal(
            ChallengeKind::Captcha,
            ChallengeStrength::Weak,
            CONF_WEAK,
            evidence,
            None,
        ));
    }
    if let Some(non) = classify_non_signal(&page) {
        return Some(non);
    }
    classify_weak(&page, &hits)
}

fn classify_strong(
    page: &Page,
    interstitials: &[&'static str],
    widgets: &[&'static str],
    hits: &[GroupHit],
) -> Option<ChallengeSignal> {
    let status_corroborates = page.error_status();
    let groups_say_interstitial = hits.iter().any(|h| INTERSTITIAL_GROUPS.contains(&h.id));

    // Class A, interstitial: one marker decides.
    if !interstitials.is_empty() {
        let extra = (interstitials.len() - 1)
            + widgets.len()
            + hits.len().min(2)
            + usize::from(status_corroborates);
        let confidence = (CONF_A_BASE + 0.02 * extra as f32).min(CONF_A_MAX);
        let mut evidence: Vec<String> = interstitials.iter().map(|m| (*m).to_owned()).collect();
        evidence.extend(widgets.iter().map(|m| (*m).to_owned()));
        evidence.extend(hits.iter().map(|h| h.id.to_owned()));
        evidence.extend(page.status_evidence());
        return Some(signal(
            ChallengeKind::Waf,
            ChallengeStrength::Strong,
            confidence,
            evidence,
            Some(BlockReason::WafInterstitial),
        ));
    }

    // Class A, widget: needs the page to look like the challenge itself. A
    // CAPTCHA on a content page, or on a login form, is a gate on one control,
    // not a blocked page.
    if !widgets.is_empty() {
        let title_cue = page.title_has(TITLE_CUES);
        let non_prove_group = hits.iter().any(|h| h.id != "prove_human");
        let corroborated = if page.has_password && page.status_2xx_or_unknown() {
            title_cue || non_prove_group
        } else {
            status_corroborates || title_cue || !hits.is_empty() || (page.thin && !page.has_password)
        };
        let gate_open = corroborated && (page.thin || status_corroborates);
        if gate_open {
            let extra = (widgets.len() - 1) + hits.len().min(2) + usize::from(status_corroborates);
            let confidence = (CONF_A_BASE + 0.02 * extra as f32).min(CONF_A_MAX);
            let waf = hits.iter().any(|h| h.id == "browser_check");
            let mut evidence: Vec<String> = widgets.iter().map(|m| (*m).to_owned()).collect();
            evidence.extend(hits.iter().map(|h| h.id.to_owned()));
            evidence.extend(page.status_evidence());
            return Some(signal(
                if waf {
                    ChallengeKind::Waf
                } else {
                    ChallengeKind::Captcha
                },
                ChallengeStrength::Strong,
                confidence,
                evidence,
                Some(if waf {
                    BlockReason::WafInterstitial
                } else {
                    BlockReason::CaptchaWidget
                }),
            ));
        }
        return None;
    }

    // Class B: two independent markers. A title hit counts twice, an error
    // status once.
    if !hits.is_empty() {
        let score: usize = hits.iter().map(|h| if h.in_title { 2 } else { 1 }).sum::<usize>()
            + usize::from(status_corroborates);
        if score >= 2 {
            let confidence = (CONF_B_BASE + 0.03 * (score - 2) as f32).min(CONF_B_MAX);
            let waf = groups_say_interstitial;
            let human = hits.iter().any(|h| h.id == "prove_human");
            let mut evidence: Vec<String> = hits.iter().map(|h| h.id.to_owned()).collect();
            evidence.extend(page.status_evidence());
            return Some(signal(
                if human && !waf {
                    ChallengeKind::Captcha
                } else {
                    ChallengeKind::Waf
                },
                ChallengeStrength::Strong,
                confidence,
                evidence,
                Some(if waf {
                    BlockReason::WafInterstitial
                } else {
                    BlockReason::UnexpectedBotError
                }),
            ));
        }
    }
    None
}

fn classify_non_signal(page: &Page) -> Option<ChallengeSignal> {
    let status = page.status;

    if status == Some(429) {
        return Some(non_signal(
            ChallengeKind::RateLimit,
            CONF_NON_SIGNAL_STATUS,
            "rate_limited",
            page,
        ));
    }
    if status == Some(451) {
        return Some(non_signal(
            ChallengeKind::GeoBlocked,
            CONF_NON_SIGNAL_STATUS,
            "geo_blocked",
            page,
        ));
    }
    if status == Some(402) {
        return Some(non_signal(
            ChallengeKind::Paywall,
            CONF_NON_SIGNAL_STATUS,
            "paywall",
            page,
        ));
    }
    if page.thin && page.text_has(RATE_PHRASES) {
        return Some(non_signal(
            ChallengeKind::RateLimit,
            CONF_NON_SIGNAL_PHRASE,
            "rate_limited",
            page,
        ));
    }
    if paywall_structure(page) {
        return Some(non_signal(
            ChallengeKind::Paywall,
            CONF_NON_SIGNAL_STRUCTURE,
            "paywall",
            page,
        ));
    }
    if page.text_has(PAYWALL_PHRASES) {
        return Some(non_signal(
            ChallengeKind::Paywall,
            CONF_NON_SIGNAL_PHRASE,
            "paywall",
            page,
        ));
    }
    if page.thin && page.text_has(GEO_PHRASES) {
        return Some(non_signal(
            ChallengeKind::GeoBlocked,
            CONF_NON_SIGNAL_PHRASE,
            "geo_blocked",
            page,
        ));
    }
    if status == Some(401) {
        return Some(non_signal(
            ChallengeKind::Login,
            CONF_NON_SIGNAL_STATUS,
            "auth_required",
            page,
        ));
    }
    if page.has_password && (page.thin || login_cue(page)) {
        return Some(non_signal(
            ChallengeKind::Login,
            CONF_NON_SIGNAL_STRUCTURE,
            "auth_required",
            page,
        ));
    }
    if page.thin && login_cue(page) {
        return Some(non_signal(
            ChallengeKind::Login,
            CONF_NON_SIGNAL_PHRASE,
            "auth_required",
            page,
        ));
    }
    let consent_host = page.host.starts_with("consent.");
    if consent_host
        || (page.text_has(CONSENT_PHRASES) && (page.thin || page.source_has(CMP_MARKERS)))
    {
        return Some(non_signal(
            ChallengeKind::Consent,
            if consent_host {
                CONF_NON_SIGNAL_STRUCTURE
            } else {
                CONF_NON_SIGNAL_PHRASE
            },
            "consent_required",
            page,
        ));
    }
    if status.is_some_and(|s| (500..600).contains(&s)) {
        return Some(non_signal(
            ChallengeKind::Unknown,
            CONF_NON_SIGNAL_STATUS,
            "server_error",
            page,
        ));
    }
    if matches!(status, Some(403 | 407 | 418 | 999)) {
        return Some(non_signal(
            ChallengeKind::AccessDenied,
            CONF_NON_SIGNAL_STATUS,
            "bare_status",
            page,
        ));
    }
    if status.is_none() && page.thin && page.text_has(DENIED_PHRASES) {
        return Some(non_signal(
            ChallengeKind::AccessDenied,
            CONF_NON_SIGNAL_PHRASE,
            "bare_status",
            page,
        ));
    }
    None
}

fn login_cue(page: &Page) -> bool {
    let path_cue = ["/login", "/signin", "/sign-in", "/log-in"]
        .iter()
        .any(|p| page.path.contains(p));
    let title_cue = ["log in", "login", "sign in", "signin"]
        .iter()
        .any(|t| page.title.starts_with(t));
    path_cue || title_cue
}

/// `"isAccessibleForFree": false` in the page's structured data.
fn paywall_structure(page: &Page) -> bool {
    let Some(src) = page.source.as_deref() else {
        return false;
    };
    let mut from = 0;
    while let Some(pos) = src[from..].find("isaccessibleforfree") {
        let after = &src[from + pos + "isaccessibleforfree".len()..];
        let value: String = after
            .chars()
            .take(12)
            .filter(|c| !matches!(c, ':' | '"' | '\'' | ' ' | '\n' | '\t' | '\r'))
            .collect();
        if value.starts_with("false") {
            return true;
        }
        from += pos + 1;
    }
    false
}

fn classify_weak(page: &Page, hits: &[GroupHit]) -> Option<ChallengeSignal> {
    if let Some(hit) = hits.first() {
        let kind = if hit.id == "prove_human" {
            ChallengeKind::Captcha
        } else {
            ChallengeKind::Waf
        };
        let mut evidence = vec![hit.id.to_owned()];
        evidence.extend(page.status_evidence());
        return Some(signal(kind, ChallengeStrength::Weak, CONF_WEAK, evidence, None));
    }
    let captcha_title = ["captcha", "recaptcha", "hcaptcha"]
        .iter()
        .any(|w| page.title.contains(w));
    if captcha_title && page.thin {
        let mut evidence = vec!["title_mentions_captcha".to_owned()];
        evidence.extend(page.status_evidence());
        return Some(signal(
            ChallengeKind::Captcha,
            ChallengeStrength::Weak,
            CONF_WEAK_TITLE,
            evidence,
            None,
        ));
    }
    None
}

/// The classifier that shipped before WS3.H: any of a handful of substrings
/// anywhere in the URL, title or text. Kept behind
/// `QUARRY_CHALLENGE_CLASSIFIER=legacy` for one release. It reports every hit
/// as a strong signal that asks for escalation, login pages and prose included.
pub fn classify_legacy(
    url: &str,
    title: Option<&str>,
    text: Option<&str>,
) -> Option<ChallengeSignal> {
    let haystack = format!(
        "{} {} {}",
        url,
        title.unwrap_or_default(),
        text.unwrap_or_default()
    )
    .to_ascii_lowercase();
    let (kind, evidence) = if ["captcha", "recaptcha", "hcaptcha", "verify you are human"]
        .iter()
        .any(|needle| haystack.contains(needle))
    {
        (ChallengeKind::Captcha, vec!["captcha_marker".to_owned()])
    } else if [
        "access denied",
        "forbidden",
        "blocked by",
        "status code 403",
    ]
    .iter()
    .any(|needle| haystack.contains(needle))
    {
        (
            ChallengeKind::AccessDenied,
            vec!["access_denied_marker".to_owned()],
        )
    } else if ["sign in", "log in", "login required", "/login"]
        .iter()
        .any(|needle| haystack.contains(needle))
    {
        (ChallengeKind::Login, vec!["login_marker".to_owned()])
    } else if ["cookie consent", "accept cookies", "consent required"]
        .iter()
        .any(|needle| haystack.contains(needle))
    {
        (ChallengeKind::Consent, vec!["consent_marker".to_owned()])
    } else if ["too many requests", "rate limit", "status code 429"]
        .iter()
        .any(|needle| haystack.contains(needle))
    {
        (
            ChallengeKind::RateLimit,
            vec!["rate_limit_marker".to_owned()],
        )
    } else {
        return None;
    };
    Some(ChallengeSignal {
        kind,
        confidence: 0.98,
        evidence,
        requires_escalation: true,
        strength: ChallengeStrength::Strong,
        block_reason: None,
        recovery: ChallengeRecovery::NotApplicable,
    })
}

// ---------------------------------------------------------------------------
// Main-document status from the devtools events
// ---------------------------------------------------------------------------

/// `scheme://host[:port]/path` with a trailing slash dropped: the form the
/// browser's redacted network events carry, so a page URL and its response
/// event compare equal.
fn document_key(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    let host = url.host_str()?;
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let path = url.path().trim_end_matches('/');
    Some(format!("{}://{}{}{}", url.scheme(), host, port, path))
}

/// The status of the response that delivered the document the browser is on:
/// the latest `Network.responseReceived` whose redacted URL equals the current
/// one. `None` when the page was reached without a response in the window (a
/// history push, an event buffer that rolled over).
pub fn main_document_status(events: &[BrowserDevtoolsEvent], current_url: &str) -> Option<u16> {
    let want = document_key(current_url)?;
    events
        .iter()
        .rev()
        .filter(|event| event.name == "Network.responseReceived")
        .filter(|event| event.url.as_deref().and_then(document_key).as_deref() == Some(&want))
        .find_map(|event| event.status)
}

/// The host of `raw`, lower-cased.
pub fn host_of(raw: &str) -> Option<String> {
    url::Url::parse(raw)
        .ok()?
        .host_str()
        .map(str::to_ascii_lowercase)
}

fn same_url(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(mut a), Ok(mut b)) => {
            a.set_fragment(None);
            b.set_fragment(None);
            a == b
        }
        _ => a == b,
    }
}

// ---------------------------------------------------------------------------
// Episodes: the one recovery
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct HostEpisode {
    /// Strong signals seen on this host in this run, across clearings.
    strong_seen: u32,
    /// A wait or a reload of the same URL happened while the host was
    /// challenging.
    recovery_used: bool,
    /// The recovery was spent and the host challenged again.
    exhausted: bool,
    /// Fingerprint of the page at the last strong signal.
    last_fingerprint: Option<String>,
}

/// Per-run, per-host bookkeeping for challenges. In memory only: it is not
/// persisted, not shared between runs, and a resumed run starts empty.
///
/// A host that shows a strong signal gets one recovery, a wait or a reload of
/// the same URL, proposed by the planner (Quarry never reloads on its own: a
/// reload is an action nobody asked for and can re-submit a POST). If the host
/// still challenges afterwards the episode is exhausted, the signal becomes
/// `challenge_loop`, and [`reject_if_exhausted`](Self::reject_if_exhausted)
/// refuses further navigation to the host and interaction with its page. A
/// later observation that is not strong clears the episode: the challenge
/// went away.
///
/// In a Zero Data Retention run the host is held as a hash and only reason
/// counts are kept. The host is never logged either way.
#[derive(Debug, Clone)]
pub struct BlockEpisodes {
    config: ChallengeConfig,
    hosts: HashMap<String, HostEpisode>,
    reasons: BTreeMap<&'static str, u32>,
}

impl Default for BlockEpisodes {
    fn default() -> Self {
        Self::new(ChallengeConfig::from_env())
    }
}

impl BlockEpisodes {
    pub fn new(config: ChallengeConfig) -> Self {
        Self {
            config,
            hosts: HashMap::new(),
            reasons: BTreeMap::new(),
        }
    }

    pub fn config(&self) -> ChallengeConfig {
        self.config
    }

    /// Strong signals seen, by reason name.
    pub fn reason_counts(&self) -> &BTreeMap<&'static str, u32> {
        &self.reasons
    }

    fn key(host: &str, zdr: ZdrMode) -> String {
        let host = host.to_ascii_lowercase();
        if zdr.is_active() {
            format!("z:{}", &blake3::hash(host.as_bytes()).to_hex()[..16])
        } else {
            host
        }
    }

    /// Record what the classifier made of the page just observed on `host` and
    /// return the signal the caller should report: a strong signal after the
    /// recovery was spent (or a third one on the host) becomes `challenge_loop`
    /// with `recovery: exhausted`; any other strong signal says the recovery is
    /// still available. `fingerprint` is the page hash, kept to say whether the
    /// page changed across the recovery.
    pub fn observe(
        &mut self,
        host: &str,
        zdr: ZdrMode,
        signal: Option<ChallengeSignal>,
        fingerprint: &str,
    ) -> Option<ChallengeSignal> {
        let key = Self::key(host, zdr);
        let mut signal = match signal {
            Some(signal) if signal.strength == ChallengeStrength::Strong => signal,
            // A hint leaves the episode as it is. Nothing, or a non-signal,
            // means the page is not a block any more: the challenge is gone.
            other => {
                if other
                    .as_ref()
                    .is_none_or(|s| s.strength != ChallengeStrength::Weak)
                {
                    self.clear(&key);
                }
                return other;
            }
        };
        let episode = self.hosts.entry(key).or_default();
        episode.strong_seen = episode.strong_seen.saturating_add(1);
        let looped =
            episode.exhausted || episode.recovery_used || episode.strong_seen >= MAX_STRONG_PER_HOST;
        if looped {
            let unchanged = episode
                .last_fingerprint
                .as_deref()
                .is_some_and(|previous| previous == fingerprint);
            episode.exhausted = true;
            signal.block_reason = Some(BlockReason::ChallengeLoop);
            signal.recovery = ChallengeRecovery::Exhausted;
            signal.evidence.push(
                if unchanged {
                    "unchanged_after_recovery"
                } else {
                    "challenged_again"
                }
                .to_owned(),
            );
        } else {
            signal.recovery = ChallengeRecovery::Available;
        }
        episode.last_fingerprint = Some(fingerprint.to_owned());
        let reason = signal
            .block_reason
            .map_or("unknown", BlockReason::as_str);
        *self.reasons.entry(reason).or_default() += 1;
        // Count and reason only: never the host, the URL or the page.
        tracing::info!(
            reason,
            strong_seen = self.hosts.values().map(|e| e.strong_seen).sum::<u32>(),
            "quarry.challenge.strong"
        );
        Some(signal)
    }

    fn clear(&mut self, key: &str) {
        if let Some(episode) = self.hosts.get_mut(key) {
            episode.recovery_used = false;
            episode.exhausted = false;
        }
    }

    /// Note that the planner is about to spend the recovery on the page at
    /// `current_url`: a `Wait`, or a `Navigate` to the URL the browser is
    /// already on. Does nothing when the host has no live strong signal.
    pub fn note_recovery_action(&mut self, action: &AgentAction, current_url: &str, zdr: ZdrMode) {
        let is_recovery = match action {
            AgentAction::Wait { .. } => true,
            AgentAction::Navigate { url } => same_url(url, current_url),
            _ => false,
        };
        if !is_recovery {
            return;
        }
        let Some(host) = host_of(current_url) else {
            return;
        };
        if let Some(episode) = self.hosts.get_mut(&Self::key(&host, zdr)) {
            if episode.strong_seen > 0 && !episode.exhausted {
                episode.recovery_used = true;
            }
        }
    }

    /// Refuse an action on a host whose recovery is spent. Navigation to an
    /// exhausted host (from any page) and every interaction with the page of an
    /// exhausted `current_host` (click, type, press, select) are refused with
    /// `CHALLENGE_DETECTED`. Observation (screenshot, content, wait, scroll),
    /// navigation to another host and history steps are allowed: the planner
    /// can still look, report, or leave.
    ///
    /// The error never names the host in a Zero Data Retention run.
    pub fn reject_if_exhausted(
        &self,
        action: &AgentAction,
        current_host: &str,
        zdr: ZdrMode,
    ) -> QuarryResult<()> {
        let blocked_host = match action {
            AgentAction::Navigate { url } => host_of(url).filter(|host| self.is_exhausted(host, zdr)),
            AgentAction::Click { .. }
            | AgentAction::ClickRef { .. }
            | AgentAction::FrameClickRef { .. }
            | AgentAction::ClickSemantic { .. }
            | AgentAction::ClickPoint { .. }
            | AgentAction::Type { .. }
            | AgentAction::TypeRef { .. }
            | AgentAction::FrameTypeRef { .. }
            | AgentAction::TypeSemantic { .. }
            | AgentAction::Press { .. }
            | AgentAction::Select { .. }
            | AgentAction::SelectRef { .. }
            | AgentAction::FrameSelectRef { .. }
            | AgentAction::SelectSemantic { .. } => {
                (!current_host.is_empty() && self.is_exhausted(current_host, zdr))
                    .then(|| current_host.to_ascii_lowercase())
            }
            // New action variants are allowed until someone decides they are
            // an interaction; the cost of a miss is one more attempt.
            _ => None,
        };
        match blocked_host {
            Some(host) => Err(QuarryError::challenge_detected(
                if zdr.is_active() { "" } else { &host },
                BlockReason::ChallengeLoop.as_str(),
            )),
            None => Ok(()),
        }
    }

    fn is_exhausted(&self, host: &str, zdr: ZdrMode) -> bool {
        self.hosts
            .get(&Self::key(host, zdr))
            .is_some_and(|episode| episode.exhausted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- fixtures -------------------------------------------------------

    const CF_INTERSTITIAL: &str = r#"<!DOCTYPE html><html lang="en-US"><head><title>Just a moment...</title><meta http-equiv="refresh" content="390"></head><body class="no-js"><div class="main-wrapper"><h1><span>example.com</span></h1><h2 id="challenge-running">Checking if the site connection is secure</h2><div id="challenge-stage"></div><noscript>Enable JavaScript and cookies to continue</noscript></div><script>(function(){window._cf_chl_opt={cvId:'3',cZone:'example.com'};var a=document.createElement('script');a.src='/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=8abc';document.head.appendChild(a);})();</script><script src="https://challenges.cloudflare.com/turnstile/v0/g/abc/api.js"></script></body></html>"#;

    const TURNSTILE_PAGE: &str = r#"<html><head><title>Turnstile Test</title></head><body><h1>Demo</h1><div class="cf-turnstile" data-sitekey="1x00000000000000000000AA"></div><script src="https://challenges.cloudflare.com/turnstile/v0/api.js" async defer></script></body></html>"#;

    const HCAPTCHA_PAGE: &str = r#"<html><head><title>hCaptcha demo</title></head><body><form><div class="h-captcha" data-sitekey="10000000-ffff-ffff-ffff-000000000001"></div><script src="https://js.hcaptcha.com/1/api.js" async defer></script></form></body></html>"#;

    const RECAPTCHA_V2_PAGE: &str = r#"<html><head><title>Check</title></head><body><div class="g-recaptcha" data-sitekey="abc"></div><script src="https://www.google.com/recaptcha/api.js"></script></body></html>"#;

    const RECAPTCHA_IFRAME_PAGE: &str = r#"<html><head><title>Check</title></head><body><iframe src="https://www.google.com/recaptcha/api2/anchor?ar=1&k=abc&size=normal" title="reCAPTCHA"></iframe></body></html>"#;

    const RECAPTCHA_V3_PAGE: &str = r#"<html><head><title>Contact</title></head><body><form><input name="email"><textarea id="g-recaptcha-response"></textarea></form><iframe src="https://www.google.com/recaptcha/api2/anchor?ar=1&k=abc&size=invisible"></iframe></body></html>"#;

    const DATADOME_PAGE: &str = r#"<html><head><title>example.com</title><style>#cmsg{animation: A 1.5s;}</style></head><body style="margin:0"><p id="cmsg">Please enable JS and disable any ad blocker</p><script>var dd={'rt':'c','cid':'AHrlqA'}</script><script src="https://ct.captcha-delivery.com/c.js"></script></body></html>"#;

    const HUMAN_PX_PAGE: &str = r#"<html><head><title>Access to this page has been denied</title></head><body><div id="px-captcha"></div><p>Press &amp; Hold to confirm you are a human (and not a bot).</p><script src="https://captcha.px-cdn.net/PXabc/captcha.js"></script></body></html>"#;

    const AKAMAI_PAGE: &str = r#"<HTML><HEAD><TITLE>Access Denied</TITLE></HEAD><BODY><H1>Access Denied</H1> You don't have permission to access "http&#58;&#47;&#47;www.example.com&#47;" on this server.<P> Reference&#32;&#35;18&#46;5b2c1002&#46;1700000000&#46;1a2b3c <P> <A HREF="https://errors.edgesuite.net/18.5b2c1002.1700000000.1a2b3c">https&#58;&#47;&#47;errors&#46;edgesuite&#46;net&#47;18&#46;5b2c1002</A></BODY></HTML>"#;

    const AWS_WAF_PAGE: &str = r#"<html><head><title></title></head><body><div id="captcha-container"></div><script src="https://abc.captcha.awswaf.com/abc/captcha.js"></script><script>AwsWafCaptcha.renderCaptcha(document.getElementById('captcha-container'), {});</script></body></html>"#;

    const IMPERVA_PAGE: &str = r#"<html style="height:100%"><head><META NAME="ROBOTS" CONTENT="NOINDEX, NOFOLLOW"></head><body style="margin:0px;height:100%"><iframe id="main-iframe" src="/_Incapsula_Resource?SWUDNSAI=31&xinfo=1" frameborder=0 width="100%" height="100%">Request unsuccessful. Incapsula incident ID: 123-456</iframe></body></html>"#;

    const CF_ERROR_1020: &str = r#"<html><head><title>Attention Required! | Cloudflare</title></head><body><h1>Sorry, you have been blocked</h1><p>You are unable to access example.com</p><div>Cloudflare Ray ID: <strong>8abc123</strong></div></body></html>"#;

    const PARDON_PAGE: &str = r#"<html><head><title>Pardon Our Interruption</title></head><body><h1>Pardon Our Interruption</h1><p>As you were browsing something about your browser made us think you were a bot.</p></body></html>"#;

    const GOOGLE_SORRY: &str = r#"<html><head><title>https://www.google.com/search?q=x</title></head><body><div>Our systems have detected unusual traffic from your computer network. This page checks to see if it's really you sending the requests, and not a robot.</div></body></html>"#;

    const NGINX_403: &str = "<html>\r\n<head><title>403 Forbidden</title></head>\r\n<body>\r\n<center><h1>403 Forbidden</h1></center>\r\n<hr><center>nginx</center>\r\n</body>\r\n</html>";

    const LOGIN_PAGE: &str = r#"<html><head><title>Sign in</title></head><body><form action="/session" method="post"><label>Email <input type="email" name="email"></label><label>Password <input type="password" name="password"></label><button>Sign in</button></form></body></html>"#;

    const LOGIN_WITH_CAPTCHA: &str = r#"<html><head><title>Sign in</title></head><body><form><input type="email" name="e"><input type="password" name="p"><div class="g-recaptcha" data-sitekey="k"></div></form></body></html>"#;

    const COOKIE_WALL: &str = r#"<html><head><title>Example</title></head><body><div class="consent"><p>We use cookies to improve your experience.</p><button>Accept all cookies</button></div></body></html>"#;

    const JSD_PAGE: &str = r#"<html><head><title>Shop</title></head><body><h1>Shop</h1><p>Things for sale.</p><script src="/cdn-cgi/challenge-platform/scripts/jsd/main.js"></script></body></html>"#;

    const PAYWALL_LD: &str = r#"<html><head><title>Story</title><script type="application/ld+json">{"@type":"NewsArticle","isAccessibleForFree": "False"}</script></head><body><article><p>The first paragraph of the story.</p></article></body></html>"#;

    /// A long, ordinary article: more visible text than a challenge page ever has.
    fn article(extra_head: &str, extra_body: &str) -> String {
        let paragraph = "<p>Rivers carry water, silt and stories from the hills to the sea, and the people who live along them have always kept careful records of the floods.</p>";
        format!(
            "<html><head><title>On rivers</title>{extra_head}</head><body><article><h1>On rivers</h1>{}</article>{extra_body}</body></html>",
            paragraph.repeat(40)
        )
    }

    // ---- the table ------------------------------------------------------

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Expect {
        Strong(BlockReason),
        Weak,
        NonSignal(&'static str),
        Nothing,
    }

    struct Case {
        name: &'static str,
        url: &'static str,
        title: Option<&'static str>,
        html: Option<String>,
        snippet: Option<&'static str>,
        status: Option<u16>,
        expect: Expect,
    }

    fn case(
        name: &'static str,
        url: &'static str,
        title: Option<&'static str>,
        html: Option<&str>,
        status: Option<u16>,
        expect: Expect,
    ) -> Case {
        Case {
            name,
            url,
            title,
            html: html.map(str::to_owned),
            snippet: None,
            status,
            expect,
        }
    }

    fn text_case(
        name: &'static str,
        url: &'static str,
        title: Option<&'static str>,
        snippet: &'static str,
        status: Option<u16>,
        expect: Expect,
    ) -> Case {
        Case {
            name,
            url,
            title,
            html: None,
            snippet: Some(snippet),
            status,
            expect,
        }
    }

    fn table() -> Vec<Case> {
        use BlockReason::{CaptchaWidget, UnexpectedBotError, WafInterstitial};
        use Expect::{Nothing, NonSignal, Strong, Weak};
        let long_with_widget = article("", r#"<div class="g-recaptcha" data-sitekey="k"></div>"#);
        let long_with_turnstile = article("", r#"<div class="cf-turnstile"></div><script src="https://challenges.cloudflare.com/turnstile/v0/api.js"></script>"#);
        let long_article = article("", "");
        let long_forbidden = article("", "<p>In the old days the river was forbidden to anyone without a licence.</p>");
        let long_unusual = article("", "<p>Unusual traffic on the river, and automated queries of the water level, are covered in chapter nine.</p>");
        let long_consent = article("", r#"<div id="onetrust-banner-sdk">We use cookies. <button>Accept all cookies</button></div>"#);
        let long_paywall = article("", "<div>Subscribe to continue reading</div>");
        let thin_captcha_title = "<html><head><title>How CAPTCHAs work</title></head><body><p>Short stub.</p></body></html>";
        let blocked_text_403 = "<html><body><p>You have been blocked.</p></body></html>";
        let too_many_html = "<html><body><h1>Too many requests</h1></body></html>";
        let unavailable_503 = "<html><head><title>503 Service Unavailable</title></head><body><h1>Service Unavailable</h1></body></html>";
        let geo_text = "<html><body><p>This content is not available in your country.</p></body></html>";
        let bare_teapot = "<html><body>teapot</body></html>";
        let thin_prove = "<html><body><p>Please verify you are human to continue.</p></body></html>";
        let captcha_path = "<html><body><p>Verify you are human.</p></body></html>";
        let login_thin_no_pw = "<html><head><title>Welcome</title></head><body><a href=\"/x\">Continue</a></body></html>";
        let json_429 = r#"{"error":"rate_limited","retry_after":30}"#;
        vec![
            // ---- strong, class A ----
            case("cloudflare interstitial", "https://example.com/", Some("Just a moment..."), Some(CF_INTERSTITIAL), Some(403), Strong(WafInterstitial)),
            case("cloudflare interstitial, status unknown", "https://example.com/", Some("Just a moment..."), Some(CF_INTERSTITIAL), None, Strong(WafInterstitial)),
            case("turnstile test page", "https://example.com/t", Some("Turnstile Test"), Some(TURNSTILE_PAGE), Some(200), Strong(CaptchaWidget)),
            case("hcaptcha test page", "https://example.com/h", Some("hCaptcha demo"), Some(HCAPTCHA_PAGE), Some(200), Strong(CaptchaWidget)),
            case("recaptcha v2 widget", "https://example.com/r", Some("Check"), Some(RECAPTCHA_V2_PAGE), Some(200), Strong(CaptchaWidget)),
            case("recaptcha anchor iframe", "https://example.com/r2", Some("Check"), Some(RECAPTCHA_IFRAME_PAGE), Some(200), Strong(CaptchaWidget)),
            case("datadome captcha", "https://example.com/d", Some("example.com"), Some(DATADOME_PAGE), Some(403), Strong(CaptchaWidget)),
            case("human px-captcha press and hold", "https://example.com/p", Some("Access to this page has been denied"), Some(HUMAN_PX_PAGE), Some(403), Strong(CaptchaWidget)),
            case("akamai reference page", "https://example.com/a", Some("Access Denied"), Some(AKAMAI_PAGE), Some(403), Strong(WafInterstitial)),
            case("aws waf captcha", "https://example.com/w", Some(""), Some(AWS_WAF_PAGE), Some(405), Strong(CaptchaWidget)),
            case("imperva incapsula", "https://example.com/i", None, Some(IMPERVA_PAGE), Some(200), Strong(WafInterstitial)),
            case("cloudflare error 1020", "https://example.com/e", Some("Attention Required! | Cloudflare"), Some(CF_ERROR_1020), Some(403), Strong(WafInterstitial)),
            // ---- strong, class B (two independent markers) ----
            case("pardon our interruption", "https://example.com/", Some("Pardon Our Interruption"), Some(PARDON_PAGE), Some(200), Strong(WafInterstitial)),
            case("google sorry page, two groups and a path", "https://www.google.com/sorry/index", Some("https://www.google.com/search?q=x"), Some(GOOGLE_SORRY), Some(429), Strong(UnexpectedBotError)),
            text_case("unusual traffic and automated queries", "https://search.example/results", None, "We have detected unusual traffic from your network. Automated queries are not allowed.", Some(200), Strong(UnexpectedBotError)),
            text_case("title says checking your browser", "https://example.com/", Some("Just a moment..."), "Checking your browser before accessing example.com.", None, Strong(WafInterstitial)),
            text_case("blocked text plus a 403", "https://example.com/", None, "You have been blocked.", Some(403), Strong(WafInterstitial)),
            case("blocked text in html plus a 403", "https://example.com/", None, Some(blocked_text_403), Some(403), Strong(WafInterstitial)),
            case("captcha path plus a prove-human phrase", "https://example.com/captcha/verify", None, Some(captcha_path), Some(200), Strong(UnexpectedBotError)),
            text_case("wrapper style: a title that asks for proof of humanity", "https://example.test/challenge", Some("Verify you are human"), "captcha required", None, Strong(UnexpectedBotError)),
            // ---- non-signals ----
            case("login page with a password field", "https://example.com/login", Some("Sign in"), Some(LOGIN_PAGE), Some(200), NonSignal("auth_required")),
            case("login page, status unknown, no url cue", "https://example.com/", Some("Sign in"), Some(LOGIN_PAGE), None, NonSignal("auth_required")),
            case("bare 401", "https://example.com/private", None, Some(""), Some(401), NonSignal("auth_required")),
            case("login by path and title only", "https://example.com/login", Some("Log in"), Some(login_thin_no_pw), Some(200), NonSignal("auth_required")),
            case("cookie wall", "https://example.com/", Some("Example"), Some(COOKIE_WALL), Some(200), NonSignal("consent_required")),
            case("cookie banner over a long article", "https://example.com/a", Some("On rivers"), Some(&long_consent), Some(200), NonSignal("consent_required")),
            case("consent host", "https://consent.example.com/m", Some("Before you continue"), Some("<html><body>x</body></html>"), Some(200), NonSignal("consent_required")),
            case("paywall by structured data", "https://news.example/s", Some("Story"), Some(PAYWALL_LD), Some(200), NonSignal("paywall")),
            case("paywall by phrase on a long page", "https://news.example/s", Some("On rivers"), Some(&long_paywall), Some(200), NonSignal("paywall")),
            case("paywall by status 402", "https://news.example/s", None, Some(""), Some(402), NonSignal("paywall")),
            case("429 json with retry-after", "https://api.example/x", None, Some(json_429), Some(429), NonSignal("rate_limited")),
            case("429 with an html body", "https://example.com/", None, Some(too_many_html), Some(429), NonSignal("rate_limited")),
            case("bare nginx 403", "https://example.com/x", Some("403 Forbidden"), Some(NGINX_403), Some(403), NonSignal("bare_status")),
            case("407", "https://example.com/x", None, Some(""), Some(407), NonSignal("bare_status")),
            case("418", "https://example.com/x", None, Some(bare_teapot), Some(418), NonSignal("bare_status")),
            text_case("denied text with no status", "https://example.com/x", None, "403 Forbidden", None, NonSignal("bare_status")),
            case("451 unavailable for legal reasons", "https://example.com/x", None, Some(""), Some(451), NonSignal("geo_blocked")),
            case("geo phrase on a short page", "https://example.com/x", None, Some(geo_text), Some(200), NonSignal("geo_blocked")),
            case("500", "https://example.com/x", None, Some("<html>oops</html>"), Some(500), NonSignal("server_error")),
            case("502", "https://example.com/x", None, Some(""), Some(502), NonSignal("server_error")),
            case("503 maintenance page", "https://example.com/x", Some("503 Service Unavailable"), Some(unavailable_503), Some(503), NonSignal("server_error")),
            // ---- hints ----
            case("login form with a recaptcha", "https://example.com/login", Some("Sign in"), Some(LOGIN_WITH_CAPTCHA), Some(200), Weak),
            text_case("one prove-human phrase on a short page", "https://example.com/x", None, "please verify you are human to continue", Some(200), Weak),
            case("one prove-human phrase in html", "https://example.com/x", None, Some(thin_prove), Some(200), Weak),
            case("short page titled about captchas", "https://example.com/c", Some("How CAPTCHAs work"), Some(thin_captcha_title), Some(200), Weak),
            // ---- nothing at all ----
            case("article titled how captchas work", "https://example.com/c", Some("How CAPTCHAs work"), Some(&long_article), Some(200), Nothing),
            case("article with forbidden in prose", "https://example.com/f", Some("On rivers"), Some(&long_forbidden), Some(200), Nothing),
            case("article about unusual traffic", "https://example.com/u", Some("On rivers"), Some(&long_unusual), Some(200), Nothing),
            case("long article with a comment-form recaptcha", "https://example.com/r", Some("On rivers"), Some(&long_with_widget), Some(200), Nothing),
            case("long article with a turnstile-protected form", "https://example.com/r", Some("On rivers"), Some(&long_with_turnstile), Some(200), Nothing),
            case("invisible recaptcha v3 on a contact form", "https://example.com/contact", Some("Contact"), Some(RECAPTCHA_V3_PAGE), Some(200), Nothing),
            case("cloudflare jsd script on a served page", "https://shop.example/", Some("Shop"), Some(JSD_PAGE), Some(200), Nothing),
            text_case("ordinary text", "https://example.test/article", Some("Example article"), "A normal page with source text", None, Nothing),
            case("404", "https://example.com/missing", Some("Not found"), Some("<html><body>Not found</body></html>"), Some(404), Nothing),
            case("empty html", "https://example.com/", None, Some(""), Some(200), Nothing),
        ]
    }

    fn run(case: &Case) -> Option<ChallengeSignal> {
        classify(&ChallengeInput {
            url: case.url,
            title: case.title,
            html: case.html.as_deref(),
            text_snippet: case.snippet,
            main_doc_status: case.status,
        })
    }

    #[test]
    fn the_classification_table_holds() {
        let cases = table();
        assert!(cases.len() >= 40, "the corpus must have at least 40 rows");
        let mut failures = Vec::new();
        for case in &cases {
            let got = run(case);
            let ok = match (case.expect, &got) {
                (Expect::Nothing, None) => true,
                (Expect::Strong(reason), Some(s)) => {
                    s.strength == ChallengeStrength::Strong && s.block_reason == Some(reason)
                }
                (Expect::Weak, Some(s)) => s.strength == ChallengeStrength::Weak,
                (Expect::NonSignal(label), Some(s)) => {
                    s.strength == ChallengeStrength::NonSignal
                        && s.evidence.first().map(String::as_str) == Some(label)
                }
                _ => false,
            };
            if !ok {
                failures.push(format!(
                    "{}: expected {:?}, got {:?}",
                    case.name,
                    case.expect,
                    got.as_ref().map(|s| (s.strength, s.block_reason, s.kind.clone(), s.evidence.clone()))
                ));
            }
        }
        assert!(failures.is_empty(), "classification failures:\n{}", failures.join("\n"));
    }

    #[test]
    fn no_non_signal_row_reports_a_strong_signal() {
        for case in table() {
            if matches!(case.expect, Expect::NonSignal(_) | Expect::Nothing | Expect::Weak) {
                if let Some(signal) = run(&case) {
                    assert_ne!(
                        signal.strength,
                        ChallengeStrength::Strong,
                        "false strong signal on {}",
                        case.name
                    );
                    assert!(!signal.requires_escalation, "{} must not escalate", case.name);
                }
            }
        }
    }

    #[test]
    fn signal_invariants_and_confidence_bands() {
        for case in table() {
            let Some(signal) = run(&case) else { continue };
            let name = case.name;
            match signal.strength {
                ChallengeStrength::Strong => {
                    assert!(signal.requires_escalation, "{name}");
                    assert!(signal.block_reason.is_some(), "{name}");
                    assert_eq!(signal.recovery, ChallengeRecovery::Available, "{name}");
                    assert!(
                        signal.confidence >= CONF_B_BASE && signal.confidence <= CONF_A_MAX,
                        "{name}: {}",
                        signal.confidence
                    );
                }
                ChallengeStrength::Weak => {
                    assert!(!signal.requires_escalation, "{name}");
                    assert!(signal.block_reason.is_none(), "{name}");
                    assert_eq!(signal.recovery, ChallengeRecovery::NotApplicable, "{name}");
                    assert!(signal.confidence <= CONF_WEAK, "{name}");
                }
                ChallengeStrength::NonSignal => {
                    assert!(!signal.requires_escalation, "{name}");
                    assert!(signal.block_reason.is_none(), "{name}");
                    assert_eq!(signal.recovery, ChallengeRecovery::NotApplicable, "{name}");
                    assert!(signal.confidence >= CONF_NON_SIGNAL_PHRASE, "{name}");
                    assert!(
                        NON_SIGNAL_LABELS.contains(&signal.evidence[0].as_str()),
                        "{name}: {:?}",
                        signal.evidence
                    );
                }
            }
            // Evidence is marker ids and a status, never page text.
            for entry in &signal.evidence {
                assert!(
                    entry.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == ':'),
                    "{name}: evidence entry {entry:?} looks like page text"
                );
            }
        }
    }

    #[test]
    fn marker_classes_set_the_confidence_band() {
        let a_only = classify(&ChallengeInput {
            url: "https://example.com/i",
            html: Some(IMPERVA_PAGE),
            main_doc_status: Some(200),
            ..Default::default()
        })
        .unwrap();
        assert!((a_only.confidence - CONF_A_BASE).abs() < 1e-6, "{}", a_only.confidence);

        // More independent corroboration raises it, up to the cap.
        let a_more = classify(&ChallengeInput {
            url: "https://example.com/",
            title: Some("Just a moment..."),
            html: Some(CF_INTERSTITIAL),
            main_doc_status: Some(403),
            ..Default::default()
        })
        .unwrap();
        assert!(a_more.confidence > a_only.confidence);
        assert!(a_more.confidence <= CONF_A_MAX + 1e-6);

        // Class B alone sits below class A.
        let b_pair = classify(&ChallengeInput {
            url: "https://search.example/results",
            text_snippet: Some("unusual traffic detected. automated queries are not allowed."),
            ..Default::default()
        })
        .unwrap();
        assert!((b_pair.confidence - CONF_B_BASE).abs() < 1e-6, "{}", b_pair.confidence);
        assert!(b_pair.confidence < a_only.confidence);

        // A hint is below both.
        let weak = classify(&ChallengeInput {
            url: "https://example.com/x",
            text_snippet: Some("please verify you are human"),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(weak.strength, ChallengeStrength::Weak);
        assert!(weak.confidence < b_pair.confidence);

        // Class C (a word in a title) is the lowest band.
        let c_only = classify(&ChallengeInput {
            url: "https://example.com/c",
            title: Some("How CAPTCHAs work"),
            html: Some("<html><body>stub</body></html>"),
            main_doc_status: Some(200),
            ..Default::default()
        })
        .unwrap();
        assert!(c_only.confidence < weak.confidence);
    }

    #[test]
    fn a_login_page_does_not_ask_for_escalation() {
        let signal = classify(&ChallengeInput {
            url: "https://example.com/login",
            title: Some("Sign in"),
            html: Some(LOGIN_PAGE),
            main_doc_status: Some(200),
            ..Default::default()
        })
        .expect("a login page is named");
        assert_eq!(signal.kind, ChallengeKind::Login);
        assert_eq!(signal.strength, ChallengeStrength::NonSignal);
        assert!(!signal.requires_escalation);
    }

    #[test]
    fn non_signal_kinds_are_the_named_ones() {
        let kind_of = |html: &str, status: u16| {
            classify(&ChallengeInput {
                url: "https://example.com/x",
                html: Some(html),
                main_doc_status: Some(status),
                ..Default::default()
            })
            .unwrap()
            .kind
        };
        assert_eq!(kind_of("", 429), ChallengeKind::RateLimit);
        assert_eq!(kind_of("", 451), ChallengeKind::GeoBlocked);
        assert_eq!(kind_of("", 402), ChallengeKind::Paywall);
        assert_eq!(kind_of("", 401), ChallengeKind::Login);
        assert_eq!(kind_of("", 403), ChallengeKind::AccessDenied);
        assert_eq!(kind_of("", 500), ChallengeKind::Unknown);
    }

    #[test]
    fn a_status_alone_never_makes_a_strong_signal() {
        for status in [401u16, 402, 403, 404, 407, 410, 418, 429, 451, 500, 502, 503, 504, 999] {
            let signal = classify(&ChallengeInput {
                url: "https://example.com/x",
                html: Some("<html><body>ok</body></html>"),
                main_doc_status: Some(status),
                ..Default::default()
            });
            assert!(
                signal.is_none_or(|s| s.strength != ChallengeStrength::Strong),
                "status {status} alone must not be strong"
            );
        }
    }

    // ---- text handling --------------------------------------------------

    #[test]
    fn visible_text_drops_scripts_styles_and_decodes_entities() {
        let text = visible_text(
            "<html><head><style>.a{content:'unusual traffic'}</style></head><body>a<script>var x='unusual traffic'</script> &amp; b&nbsp;c <!-- unusual traffic --> d&#35;e&#x21;</body></html>",
        );
        assert_eq!(text, "a & b c d#e!");
    }

    #[test]
    fn long_multibyte_pages_do_not_panic() {
        // 4-byte characters straddling the scan cap and the snippet cut.
        let page = format!("<html><body>{}</body></html>", "\u{1F600}".repeat(200_000));
        assert!(page.len() > MAX_SCANNED_BYTES);
        let signal = classify(&ChallengeInput {
            url: "https://example.com/",
            html: Some(&page),
            main_doc_status: Some(200),
            ..Default::default()
        });
        assert!(signal.is_none());
        assert_eq!(cap_str("a\u{1F600}b", 2), "a");
        assert_eq!(cap_str("abc", 10), "abc");
    }

    #[test]
    fn an_unparseable_url_does_not_stop_classification() {
        let signal = classify(&ChallengeInput {
            url: "not a url",
            html: Some(CF_INTERSTITIAL),
            ..Default::default()
        });
        assert_eq!(
            signal.unwrap().block_reason,
            Some(BlockReason::WafInterstitial)
        );
    }

    // ---- legacy and configuration ---------------------------------------

    #[test]
    fn the_legacy_classifier_keeps_its_old_behaviour() {
        let signal = classify_legacy(
            "https://example.test/challenge",
            Some("Verify you are human"),
            Some("captcha required"),
        )
        .unwrap();
        assert_eq!(signal.kind, ChallengeKind::Captcha);
        assert!(signal.requires_escalation);
        assert!((signal.confidence - 0.98).abs() < 1e-6);
        assert_eq!(signal.strength, ChallengeStrength::Strong);
        // It still flags the very pages v2 is built not to.
        let login = classify_legacy("https://example.test/login", Some("Sign in"), None).unwrap();
        assert!(login.requires_escalation);
        assert!(classify_legacy("https://example.test/a", Some("Article"), Some("plain")).is_none());
    }

    #[test]
    fn config_values_parse_with_safe_defaults() {
        let default = ChallengeConfig::from_values(None, None);
        assert_eq!(default, ChallengeConfig::default());
        assert_eq!(default.classifier, ClassifierMode::V2);
        assert_eq!(default.enforce, EnforceMode::Enforce);
        let legacy = ChallengeConfig::from_values(Some(" Legacy "), Some("LOG"));
        assert_eq!(legacy.classifier, ClassifierMode::Legacy);
        assert_eq!(legacy.enforce, EnforceMode::Log);
        let junk = ChallengeConfig::from_values(Some("v3"), Some("maybe"));
        assert_eq!(junk, ChallengeConfig::default());
    }

    // ---- main-document status --------------------------------------------

    fn response_event(url: &str, status: u16) -> BrowserDevtoolsEvent {
        BrowserDevtoolsEvent {
            sequence: 0,
            tab_id: None,
            category: "network".into(),
            name: "Network.responseReceived".into(),
            level: None,
            method: None,
            url: Some(url.into()),
            status: Some(status),
            text: None,
            timestamp_ms: 0,
            payload: json!({}),
        }
    }

    #[test]
    fn the_main_document_status_is_the_latest_response_for_the_current_url() {
        let events = vec![
            response_event("https://example.com/old", 301),
            response_event("https://example.com/page", 403),
            response_event("https://example.com/static/app.js", 200),
            response_event("https://example.com/page", 200),
            response_event("https://cdn.example.net/page", 500),
        ];
        // The query string and fragment are not in the redacted events.
        assert_eq!(
            main_document_status(&events, "https://example.com/page?a=1#top"),
            Some(200)
        );
        assert_eq!(
            main_document_status(&events, "https://example.com/page/"),
            Some(200)
        );
        assert_eq!(main_document_status(&events, "https://example.com/other"), None);
        assert_eq!(main_document_status(&[], "https://example.com/page"), None);
        assert_eq!(main_document_status(&events, ""), None);
    }

    #[test]
    fn requests_and_other_events_are_not_a_document_status() {
        let mut request = response_event("https://example.com/page", 200);
        request.name = "Network.requestWillBeSent".into();
        assert_eq!(main_document_status(&[request], "https://example.com/page"), None);
    }

    // ---- episodes ---------------------------------------------------------

    fn strong() -> ChallengeSignal {
        classify(&ChallengeInput {
            url: "https://blocked.example/page",
            title: Some("Just a moment..."),
            html: Some(CF_INTERSTITIAL),
            main_doc_status: Some(403),
            ..Default::default()
        })
        .unwrap()
    }

    fn nav(url: &str) -> AgentAction {
        AgentAction::Navigate { url: url.into() }
    }

    fn click() -> AgentAction {
        AgentAction::Click { selector: "#go".into() }
    }

    const HOST: &str = "blocked.example";
    const PAGE: &str = "https://blocked.example/page";

    fn episodes() -> BlockEpisodes {
        BlockEpisodes::new(ChallengeConfig::default())
    }

    /// An episode that has used its recovery and challenged again.
    fn exhausted() -> BlockEpisodes {
        let mut e = episodes();
        e.observe(HOST, ZdrMode::Off, Some(strong()), "h1");
        e.note_recovery_action(&AgentAction::Wait { ms: 10 }, PAGE, ZdrMode::Off);
        let signal = e.observe(HOST, ZdrMode::Off, Some(strong()), "h2").unwrap();
        assert_eq!(signal.block_reason, Some(BlockReason::ChallengeLoop));
        e
    }

    #[test]
    fn a_first_strong_signal_keeps_its_reason_and_the_recovery_available() {
        let mut e = episodes();
        let signal = e.observe(HOST, ZdrMode::Off, Some(strong()), "h1").unwrap();
        assert_eq!(signal.recovery, ChallengeRecovery::Available);
        assert_eq!(signal.block_reason, Some(BlockReason::WafInterstitial));
        assert!(signal.requires_escalation);
        assert_eq!(e.reason_counts().get("waf_interstitial"), Some(&1));
        // Nothing is refused yet.
        assert!(e.reject_if_exhausted(&nav("https://blocked.example/x"), HOST, ZdrMode::Off).is_ok());
        assert!(e.reject_if_exhausted(&click(), HOST, ZdrMode::Off).is_ok());
    }

    #[test]
    fn a_wait_and_a_same_url_navigate_then_a_second_strong_signal_is_a_loop() {
        let mut e = episodes();
        e.observe(HOST, ZdrMode::Off, Some(strong()), "h1");
        e.note_recovery_action(&AgentAction::Wait { ms: 10 }, PAGE, ZdrMode::Off);
        e.note_recovery_action(&nav("https://blocked.example/page#frag"), PAGE, ZdrMode::Off);
        let signal = e.observe(HOST, ZdrMode::Off, Some(strong()), "h2").unwrap();
        assert_eq!(signal.block_reason, Some(BlockReason::ChallengeLoop));
        assert_eq!(signal.recovery, ChallengeRecovery::Exhausted);
        assert!(signal.requires_escalation);
        assert!(signal.evidence.iter().any(|e| e == "challenged_again"));
        assert_eq!(e.reason_counts().get("challenge_loop"), Some(&1));
    }

    #[test]
    fn a_single_wait_is_enough_to_spend_the_recovery() {
        let mut e = episodes();
        e.observe(HOST, ZdrMode::Off, Some(strong()), "h1");
        e.note_recovery_action(&AgentAction::Wait { ms: 10 }, PAGE, ZdrMode::Off);
        let signal = e.observe(HOST, ZdrMode::Off, Some(strong()), "h1").unwrap();
        assert_eq!(signal.recovery, ChallengeRecovery::Exhausted);
        assert!(signal.evidence.iter().any(|e| e == "unchanged_after_recovery"));
    }

    #[test]
    fn a_navigate_to_another_url_is_not_a_recovery() {
        let mut e = episodes();
        e.observe(HOST, ZdrMode::Off, Some(strong()), "h1");
        e.note_recovery_action(&nav("https://blocked.example/other"), PAGE, ZdrMode::Off);
        let signal = e.observe(HOST, ZdrMode::Off, Some(strong()), "h2").unwrap();
        assert_eq!(signal.recovery, ChallengeRecovery::Available);
        assert_eq!(signal.block_reason, Some(BlockReason::WafInterstitial));
    }

    #[test]
    fn a_recovery_action_without_a_strong_signal_does_nothing() {
        let mut e = episodes();
        e.note_recovery_action(&AgentAction::Wait { ms: 10 }, PAGE, ZdrMode::Off);
        let signal = e.observe(HOST, ZdrMode::Off, Some(strong()), "h1").unwrap();
        assert_eq!(signal.recovery, ChallengeRecovery::Available);
    }

    #[test]
    fn a_third_strong_signal_on_a_host_is_a_loop_even_without_a_recovery() {
        let mut e = episodes();
        let first = e.observe(HOST, ZdrMode::Off, Some(strong()), "a").unwrap();
        let second = e.observe(HOST, ZdrMode::Off, Some(strong()), "b").unwrap();
        let third = e.observe(HOST, ZdrMode::Off, Some(strong()), "c").unwrap();
        assert_eq!(first.recovery, ChallengeRecovery::Available);
        assert_eq!(second.recovery, ChallengeRecovery::Available);
        assert_eq!(third.recovery, ChallengeRecovery::Exhausted);
        assert_eq!(third.block_reason, Some(BlockReason::ChallengeLoop));
    }

    #[test]
    fn reject_if_exhausted_refuses_navigation_and_interaction_and_allows_the_rest() {
        let e = exhausted();
        let refuse = |action: AgentAction, host: &str| {
            let error = e
                .reject_if_exhausted(&action, host, ZdrMode::Off)
                .expect_err("must be refused");
            assert_eq!(error.code, quarry_core::error::ErrorCode::ChallengeDetected);
            assert_eq!(error.code.http_status(), 403);
            let details = error.details.unwrap();
            assert_eq!(details["recovery"], "exhausted");
            assert_eq!(details["reason"], "challenge_loop");
            assert_eq!(details["host"], HOST);
            assert_eq!(details["next"], "user_takeover_or_other_source");
        };
        refuse(nav("https://blocked.example/anything"), HOST);
        // Coming back to an exhausted host from another page is refused too.
        refuse(nav("https://blocked.example/anything"), "other.example");
        refuse(click(), HOST);
        refuse(AgentAction::ClickPoint { x: 1.0, y: 2.0 }, HOST);
        refuse(AgentAction::Type { selector: "#q".into(), text: "x".into() }, HOST);
        refuse(AgentAction::Press { key: "Enter".into() }, HOST);
        refuse(AgentAction::Select { selector: "#s".into(), value: "v".into() }, HOST);

        let allow = |action: AgentAction, host: &str| {
            assert!(
                e.reject_if_exhausted(&action, host, ZdrMode::Off).is_ok(),
                "{action:?} must be allowed"
            );
        };
        allow(AgentAction::Screenshot { full_page: false }, HOST);
        allow(AgentAction::Wait { ms: 10 }, HOST);
        allow(AgentAction::Scroll { target: "#a".into() }, HOST);
        allow(AgentAction::MouseWheel { x: 0.0, y: 0.0, delta_x: 0.0, delta_y: 10.0 }, HOST);
        allow(AgentAction::GetContent, HOST);
        allow(AgentAction::Pdf, HOST);
        allow(AgentAction::Back, HOST);
        allow(nav("https://other.example/"), HOST);
        // Interacting on another host is fine.
        allow(click(), "other.example");
        // An unparseable target is left to the browser to reject.
        allow(nav("not a url"), HOST);
    }

    #[test]
    fn a_non_strong_observation_clears_the_episode_but_a_hint_does_not() {
        let mut e = exhausted();
        let hint = classify(&ChallengeInput {
            url: PAGE,
            text_snippet: Some("please verify you are human"),
            ..Default::default()
        });
        assert_eq!(hint.as_ref().unwrap().strength, ChallengeStrength::Weak);
        let kept = e.observe(HOST, ZdrMode::Off, hint.clone(), "h3");
        assert_eq!(kept, hint, "a hint passes through unchanged");
        assert!(e.reject_if_exhausted(&click(), HOST, ZdrMode::Off).is_err());

        // The challenge went away (a user took over, or it resolved).
        let login = classify(&ChallengeInput {
            url: PAGE,
            html: Some(LOGIN_PAGE),
            main_doc_status: Some(200),
            ..Default::default()
        });
        let passed = e.observe(HOST, ZdrMode::Off, login.clone(), "h4");
        assert_eq!(passed, login, "a non-signal passes through unchanged");
        assert!(e.reject_if_exhausted(&click(), HOST, ZdrMode::Off).is_ok());
        assert!(e.reject_if_exhausted(&nav("https://blocked.example/x"), HOST, ZdrMode::Off).is_ok());
        // Nothing at all passes through as nothing.
        assert_eq!(e.observe(HOST, ZdrMode::Off, None, "h5"), None);
    }

    #[test]
    fn hosts_are_tracked_separately() {
        let mut e = exhausted();
        let other = e.observe("other.example", ZdrMode::Off, Some(strong()), "x").unwrap();
        assert_eq!(other.recovery, ChallengeRecovery::Available);
        assert!(e.reject_if_exhausted(&click(), "other.example", ZdrMode::Off).is_ok());
        assert!(e.reject_if_exhausted(&click(), HOST, ZdrMode::Off).is_err());
        // Host names compare without case.
        assert!(e.reject_if_exhausted(&click(), "BLOCKED.example", ZdrMode::Off).is_err());
    }

    #[test]
    fn zdr_runs_hold_hashes_and_leave_the_host_out_of_the_error() {
        let mut e = episodes();
        e.observe(HOST, ZdrMode::On, Some(strong()), "h1");
        e.note_recovery_action(&AgentAction::Wait { ms: 10 }, PAGE, ZdrMode::On);
        e.observe(HOST, ZdrMode::On, Some(strong()), "h2");
        assert!(
            e.hosts.keys().all(|key| !key.contains("blocked")),
            "a ZDR run must not keep the host name: {:?}",
            e.hosts.keys().collect::<Vec<_>>()
        );
        let error = e
            .reject_if_exhausted(&nav("https://blocked.example/x"), HOST, ZdrMode::On)
            .expect_err("still refused");
        let details = error.details.unwrap();
        assert!(details.get("host").is_none());
        assert_eq!(details["recovery"], "exhausted");
        assert!(!error.message.contains("blocked"));
        // The same counters by reason, no host anywhere in them.
        assert_eq!(e.reason_counts().get("challenge_loop"), Some(&1));
    }
}
