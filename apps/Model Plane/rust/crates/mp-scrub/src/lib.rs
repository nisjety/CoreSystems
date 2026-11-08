//! Secret scrubbing shared by the Model Plane services (WS12.S4).
//!
//! Two sets live here:
//!
//! **Legacy** (moved unchanged from `execution-core::scrub`): aggressive masking for
//! checkpoints, step payloads and command lines, run before anything is persisted
//! (Security gate #1 in `docs/VERIFICATION.md`).
//! - [`scrub_json_value`]: redacts values in a [`serde_json::Value`] whose object key
//!   matches a sensitive-name pattern.
//! - [`scrub_string`]: redacts known secret token patterns inside free-form strings
//!   (Bearer tokens, JWTs, `sk-*`, AWS access keys, PEM blocks, connection-string
//!   passwords, inline `key=secret` assignments, Slack / Google API keys).
//! - [`redact_command`]: the above plus the positional `--token <secret>` rule.
//!
//! **Live** (new): masks only strings with the shape of a credential, so it can run on
//! the text of a web page on its way to a model, or into an event, without eating the
//! page. [`scrub_live`], [`scrub_url`], [`scrub_live_json`]. Held to Quarry's authored
//! golden, vendored at `tests/fixtures/secret-shapes.v1.json` (`tests/golden.rs`).
//!
//! The legacy pattern set was extended 2026-05-30 per `docs/capability-ownership-matrix.md`
//! section G5, adapting the regex coverage of `OpenAI` Codex's `secrets` crate
//! (`redact_secrets`, Apache-2.0), independently reimplemented here.
//!
//! This crate depends on nothing of the Model Plane (not `mp-contracts`), so the
//! fold-specific `scrub_fold` stays in `execution-core`.

use std::sync::OnceLock;

use regex::{Captures, Regex, RegexSet};
use serde_json::Value;

/// What replaces a secret, in both sets.
pub const MARKER: &str = "[REDACTED]";

/// A value that starts with this is already masked; the Live rules skip it so a second
/// pass changes nothing.
const MARKER_STEM: &str = "[REDACTED";

const REDACTED: &str = MARKER;

fn sensitive_key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(password|secret|token|api[_-]?key|auth|credential|bearer|private[_-]?key|session[_-]?id|cookie)",
        )
        .expect("sensitive_key_re compiles")
    })
}

fn env_key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Treat any ALL_CAPS env-style key ending in _KEY/_TOKEN/_SECRET/_PASSWORD as sensitive.
    RE.get_or_init(|| {
        Regex::new(r"^[A-Z][A-Z0-9_]*(_KEY|_TOKEN|_SECRET|_PASSWORD|_PWD)$")
            .expect("env_key_re compiles")
    })
}

fn string_patterns() -> &'static [Regex] {
    static RE: OnceLock<Vec<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        vec![
            // Bearer <token>
            Regex::new(r"(?i)bearer\s+[A-Za-z0-9._\-+/=]+").expect("bearer regex"),
            // JWT: three dot-separated base64url segments
            Regex::new(r"eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+").expect("jwt regex"),
            // OpenAI / Anthropic style: sk-...
            Regex::new(r"sk-[A-Za-z0-9_\-]{16,}").expect("sk regex"),
            // AWS access key id
            Regex::new(r"AKIA[0-9A-Z]{16}").expect("aws akid regex"),
            // GitHub tokens
            Regex::new(r"gh[pousr]_[A-Za-z0-9]{20,}").expect("github token regex"),
            // Slack tokens (bot/user/app/refresh/legacy)
            Regex::new(r"xox[baprs]-[A-Za-z0-9-]{10,}").expect("slack token regex"),
            // Google API keys
            Regex::new(r"AIza[0-9A-Za-z_\-]{35}").expect("google api key regex"),
            // HTTP Basic auth header value
            Regex::new(r"(?i)basic\s+[A-Za-z0-9+/=]{8,}").expect("basic auth regex"),
            // PEM private key block
            Regex::new(
                r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            )
            .expect("pem regex"),
        ]
    })
}

/// Capture-preserving redaction patterns: `(regex, replacement)`.
///
/// Unlike [`string_patterns`], these keep the surrounding context (the key
/// name, the URI scheme/host) and redact only the secret portion via capture
/// groups, so scrubbed output stays diagnosable — e.g.
/// `postgres://svc:[REDACTED]@db/app` and `DB_PASSWORD=[REDACTED]`.
fn capture_patterns() -> &'static [(Regex, &'static str)] {
    static RE: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RE.get_or_init(|| {
        vec![
            // Credentials in connection-string / URI userinfo:
            // scheme://user:SECRET@host  →  scheme://user:[REDACTED]@host
            (
                Regex::new(r"(?i)\b([a-z][a-z0-9+.\-]*://[^:/?#\s]+:)([^@/?#\s]+)(@)")
                    .expect("uri userinfo regex"),
                "${1}[REDACTED]${3}",
            ),
            // Inline assignment in free text / shell commands. The leading
            // `[a-z0-9_]*` lets ENV-style prefixes match (DB_PASSWORD,
            // OPENAI_API_KEY) — `\bpassword` alone misses `_PASSWORD` since
            // `_P` is not a word boundary.
            // DB_PASSWORD=secret | api_key: "xyz"  →  DB_PASSWORD=[REDACTED]
            (
                Regex::new(
                    r#"(?i)\b([a-z0-9_]*(?:password|passwd|pwd|secret|api[_-]?key|access[_-]?key|secret[_-]?key|auth[_-]?token|client[_-]?secret|private[_-]?key|token))(\s*[:=]\s*)("?)([^\s"'&,;]{3,})"#,
                )
                .expect("inline assignment regex"),
                "${1}${2}${3}[REDACTED]",
            ),
        ]
    })
}

/// Recursively redact sensitive-keyed values in a JSON tree.
///
/// Any object entry whose key matches the sensitive-name pattern has its
/// value replaced with `"[REDACTED]"`. String values elsewhere are passed
/// through [`scrub_string`] to catch inline secrets.
pub fn scrub_json_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if sensitive_key_re().is_match(key) || env_key_re().is_match(key) {
                    *v = Value::String(REDACTED.to_owned());
                } else {
                    scrub_json_value(v);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                scrub_json_value(item);
            }
        }
        Value::String(s) => {
            let scrubbed = scrub_string(s);
            if scrubbed != *s {
                *s = scrubbed;
            }
        }
        _ => {}
    }
}

/// Redact known secret patterns inside a free-form string.
///
/// Matched patterns: `Bearer <token>`, `Basic <b64>`, JWTs, `sk-...`, AWS
/// access key ids, GitHub / Slack tokens, Google API keys, PEM private key
/// blocks, connection-string passwords, and inline `key=value` secret
/// assignments. The capture-preserving patterns keep surrounding context and
/// redact only the secret portion.
#[must_use]
pub fn scrub_string(s: &str) -> String {
    let mut out = s.to_owned();
    for re in string_patterns() {
        out = re.replace_all(&out, REDACTED).into_owned();
    }
    for (re, replacement) in capture_patterns() {
        out = re.replace_all(&out, *replacement).into_owned();
    }
    out
}

// ---------------------------------------------------------------------------
// The Live set (WS12.S4): structured token shapes only.
// ---------------------------------------------------------------------------
//
// The legacy set above is deliberately aggressive (any `Bearer x`, any
// `token=...`): right for a checkpoint or a command line, wrong for the text of
// a web page that is about to be read by a model, where "the bearer of the
// message" and `max_tokens=4096` are ordinary prose. The Live set masks only
// strings that have the shape of a credential, so it may run on page text,
// titles, URLs and observations without eating the page.
//
// It is held to Quarry's authored golden (`tests/fixtures/secret-shapes.v1.json`,
// vendored byte for byte): every case whose `applies_to` names `mp` must pass.
// The rule table is a port of `quarry_core::redaction::shapes` (the Live
// profile); the two differ only in the marker (`[REDACTED]` here, a typed
// `[redacted:kind]` there), which the harness maps.
//
// Idempotent: a second pass over its own output changes nothing, because the
// marker never matches a rule and a value that already is the marker is skipped.

/// Opaque bearer tokens outside an `Authorization` header must be at least this
/// long and carry a digit or a base64 symbol.
const BEARER_BARE_MIN: usize = 16;
/// Same for a bare `Basic <base64>`.
const BASIC_BARE_MIN: usize = 12;
/// How far before a match `Authorization` may sit to count as its header.
const HEADER_LOOKBEHIND: usize = 48;
/// Safety cap on the fixed-point loop. One pass is the normal case.
const MAX_PASSES: usize = 4;

type Verify = fn(&str, &Captures<'_>) -> bool;

#[derive(Clone, Copy)]
enum Replace {
    /// The whole match is the secret.
    Whole,
    /// Only this capture group is the secret; the rest of the match (the scheme,
    /// the `Bearer` word) stays.
    Group(usize),
}

struct LiveRule {
    pattern: &'static str,
    replace: Replace,
    verify: Option<Verify>,
}

/// The Live rule table, in application order.
const LIVE_RULES: &[LiveRule] = &[
    // PEM private key. An unterminated block (a truncated page) is masked to the
    // end of the text: failing closed is the safe reading.
    LiveRule {
        pattern: r"-----BEGIN [A-Z ]*PRIVATE KEY(?: BLOCK)?-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY(?: BLOCK)?-----|\z)",
        replace: Replace::Whole,
        verify: None,
    },
    // JWT. The `eyJ` prefix matches in any case: an element fingerprint keeps its
    // text lower-cased, and a token that went through that normaliser is still one.
    LiveRule {
        pattern: r"(?-u:\b)(?i:eyj)[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]*",
        replace: Replace::Whole,
        verify: None,
    },
    // URI userinfo password: scheme://user:SECRET@host
    LiveRule {
        pattern: r"(?i)(?-u:\b)([a-z][a-z0-9+.\-]*://[^:/?#@\s]+:)([^@/?#\s]+)(@)",
        replace: Replace::Group(2),
        verify: None,
    },
    // AWS access key id (long-lived and temporary).
    LiveRule {
        pattern: r"(?-u:\b)(?:AKIA|ASIA)[0-9A-Z]{16}(?-u:\b)",
        replace: Replace::Whole,
        verify: None,
    },
    // GitHub tokens (classic and fine-grained).
    LiveRule {
        pattern: r"(?-u:\b)(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})",
        replace: Replace::Whole,
        verify: None,
    },
    // GitLab personal access token.
    LiveRule {
        pattern: r"(?-u:\b)glpat-[A-Za-z0-9_-]{20,}",
        replace: Replace::Whole,
        verify: None,
    },
    // npm token.
    LiveRule {
        pattern: r"(?-u:\b)npm_[A-Za-z0-9]{30,}",
        replace: Replace::Whole,
        verify: None,
    },
    // Slack bot / user / app / refresh / legacy tokens.
    LiveRule {
        pattern: r"(?-u:\b)(?:xox[baprso]-|xapp-)[A-Za-z0-9-]{10,}",
        replace: Replace::Whole,
        verify: None,
    },
    // Google API key.
    LiveRule {
        pattern: r"(?-u:\b)AIza[0-9A-Za-z_-]{35}",
        replace: Replace::Whole,
        verify: None,
    },
    // OpenAI / Anthropic style key.
    LiveRule {
        pattern: r"(?-u:\b)sk-[A-Za-z0-9_-]{16,}",
        replace: Replace::Whole,
        verify: None,
    },
    // Stripe secret and restricted keys. A publishable key (pk_) is public.
    LiveRule {
        pattern: r"(?-u:\b)[sr]k_(?:live|test)_[A-Za-z0-9]{16,}",
        replace: Replace::Whole,
        verify: None,
    },
    // Stripe webhook signing secret.
    LiveRule {
        pattern: r"(?-u:\b)whsec_[A-Za-z0-9+/=]{16,}",
        replace: Replace::Whole,
        verify: None,
    },
    // Bearer <token>: a header context, or a token that looks like one.
    LiveRule {
        pattern: r"(?i)(?-u:\b)bearer\s+([A-Za-z0-9._~+/=-]{6,})",
        replace: Replace::Group(1),
        verify: Some(bearer_ok),
    },
    // Basic <base64>: a header context, or a token that looks like one.
    LiveRule {
        pattern: r"(?i)(?-u:\b)basic\s+([A-Za-z0-9+/]{8,}={0,2})",
        replace: Replace::Group(1),
        verify: Some(basic_ok),
    },
];

/// Secret-bearing query parameters of a URL (matched case-insensitively, after
/// percent-decoding the key). Keys starting with [`URL_QUERY_KEY_PREFIXES`] too.
pub const URL_QUERY_KEYS: &[&str] = &[
    "token",
    "access_token",
    "id_token",
    "refresh_token",
    "code",
    "key",
    "api_key",
    "apikey",
    "sig",
    "signature",
    "password",
    "secret",
    "session",
    "sid",
    "otp",
    "jwt",
];

/// Query key prefixes that are always secrets (`X-Amz-Signature`, `X-Amz-Credential`).
pub const URL_QUERY_KEY_PREFIXES: &[&str] = &["x-amz-"];

/// Secret-bearing fragment parameters (an OAuth implicit-flow `#access_token=...`).
pub const URL_FRAGMENT_KEYS: &[&str] = &["access_token", "id_token", "refresh_token", "token", "code"];

/// URL-only text rules, for a URL embedded in prose (no structure to split) and for
/// an HTML-escaped separator (`&amp;token=`). Applied by [`scrub_url`] only: plain
/// page text with `?token=` in it is left to the shapes.
const URL_TEXT_RULES: &[LiveRule] = &[
    LiveRule {
        pattern: r#"(#(?:[^\s#=&]*=[^\s#&]*&)*?(?i:access_token|id_token|refresh_token|token|code)=)([^&\s"'<>]+)"#,
        replace: Replace::Group(2),
        verify: None,
    },
    LiveRule {
        pattern: r#"([?&;](?i:token|access_token|id_token|refresh_token|code|key|api_key|apikey|sig|signature|password|secret|session|sid|otp|jwt|x-amz-[a-z0-9-]+)=)([^&#\s"'<>]+)"#,
        replace: Replace::Group(2),
        verify: None,
    },
];

/// Does `Authorization` sit just before `pos`?
fn after_authorization(text: &str, pos: usize) -> bool {
    const NEEDLE: &[u8] = b"authorization";
    let lo = pos.saturating_sub(HEADER_LOOKBEHIND);
    text.as_bytes()[lo..pos]
        .windows(NEEDLE.len())
        .any(|w| w.eq_ignore_ascii_case(NEEDLE))
}

fn has_digit_or_symbol(s: &str, symbols: &str) -> bool {
    s.bytes()
        .any(|b| b.is_ascii_digit() || symbols.as_bytes().contains(&b))
}

fn bearer_ok(text: &str, caps: &Captures<'_>) -> bool {
    let (Some(whole), Some(token)) = (caps.get(0), caps.get(1)) else {
        return false;
    };
    after_authorization(text, whole.start())
        || (token.len() >= BEARER_BARE_MIN && has_digit_or_symbol(token.as_str(), "._~+/="))
}

fn basic_ok(text: &str, caps: &Captures<'_>) -> bool {
    let (Some(whole), Some(token)) = (caps.get(0), caps.get(1)) else {
        return false;
    };
    after_authorization(text, whole.start())
        || (token.len() >= BASIC_BARE_MIN && has_digit_or_symbol(token.as_str(), "+/="))
}

struct Compiled {
    live: Vec<Regex>,
    live_set: RegexSet,
    url: Vec<Regex>,
    url_set: RegexSet,
}

fn compiled() -> &'static Compiled {
    static C: OnceLock<Compiled> = OnceLock::new();
    C.get_or_init(|| {
        let build = |rules: &[LiveRule]| -> (Vec<Regex>, RegexSet) {
            let regexes = rules
                .iter()
                .map(|r| {
                    Regex::new(r.pattern)
                        .unwrap_or_else(|e| panic!("scrub rule does not compile: {e}"))
                })
                .collect();
            let set = RegexSet::new(rules.iter().map(|r| r.pattern))
                .unwrap_or_else(|e| panic!("scrub rule set does not compile: {e}"));
            (regexes, set)
        };
        let (live, live_set) = build(LIVE_RULES);
        let (url, url_set) = build(URL_TEXT_RULES);
        Compiled {
            live,
            live_set,
            url,
            url_set,
        }
    })
}

/// Replace every accepted match of one rule. `None` when nothing changed.
fn apply_rule(rule: &LiveRule, re: &Regex, text: &str) -> Option<String> {
    let mut out = String::new();
    let mut last = 0;
    let mut changed = false;
    let mut take = |start: usize, end: usize, out: &mut String| {
        out.push_str(&text[last..start]);
        out.push_str(MARKER);
        last = end;
        changed = true;
    };
    match rule.replace {
        Replace::Whole => {
            for m in re.find_iter(text) {
                if m.as_str().is_empty() || m.as_str().starts_with(MARKER_STEM) {
                    continue;
                }
                take(m.start(), m.end(), &mut out);
            }
        }
        Replace::Group(group) => {
            for caps in re.captures_iter(text) {
                let Some(secret) = caps.get(group) else {
                    continue;
                };
                if secret.as_str().is_empty() || secret.as_str().starts_with(MARKER_STEM) {
                    continue;
                }
                if rule.verify.is_some_and(|v| !v(text, &caps)) {
                    continue;
                }
                take(secret.start(), secret.end(), &mut out);
            }
        }
    }
    if changed {
        out.push_str(&text[last..]);
        Some(out)
    } else {
        None
    }
}

/// Run a rule table to a fixed point.
fn run_rules(rules: &[LiveRule], regexes: &[Regex], set: &RegexSet, input: &str) -> Option<String> {
    let mut text: Option<String> = None;
    for _ in 0..MAX_PASSES {
        let current = text.as_deref().unwrap_or(input);
        let mut next: Option<String> = None;
        for index in set.matches(current).iter() {
            let base = next.as_deref().unwrap_or(current);
            if let Some(replaced) = apply_rule(&rules[index], &regexes[index], base) {
                next = Some(replaced);
            }
        }
        match next {
            Some(n) => text = Some(n),
            None => break,
        }
    }
    text
}

/// Mask every credential-shaped string in `s`: PEM private keys, JWTs, `sk-` /
/// `sk_live_` / `rk_live_` / `whsec_` keys, AWS key ids, GitHub / GitLab / npm /
/// Slack / Google tokens, URI userinfo passwords, and `Bearer` / `Basic` values that
/// are in an `Authorization` header or are token-shaped (16 or more characters with a
/// digit or symbol for `Bearer`). Everything else, prose included, is returned as it
/// came. Idempotent.
#[must_use]
pub fn scrub_live(s: &str) -> String {
    let c = compiled();
    run_rules(LIVE_RULES, &c.live, &c.live_set, s).unwrap_or_else(|| s.to_owned())
}

/// Percent-decode a query key and lower-case it (a `+` is a space, as in a form).
fn decode_key(key: &str) -> String {
    let bytes = key.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_ascii_lowercase()
}

fn is_query_key(key: &str) -> bool {
    URL_QUERY_KEYS.contains(&key) || URL_QUERY_KEY_PREFIXES.iter().any(|p| key.starts_with(p))
}

fn is_fragment_key(key: &str) -> bool {
    URL_FRAGMENT_KEYS.contains(&key)
}

/// Mask the secret values of `k=v&k=v` pairs. `None` when nothing was masked.
fn mask_pairs(pairs: &str, is_secret_key: fn(&str) -> bool) -> Option<String> {
    let mut changed = false;
    let rebuilt: Vec<String> = pairs
        .split('&')
        .map(|pair| {
            if let Some((k, v)) = pair.split_once('=') {
                if is_secret_key(&decode_key(k)) && !v.is_empty() && !v.starts_with(MARKER_STEM) {
                    changed = true;
                    return format!("{k}={MARKER}");
                }
            }
            pair.to_owned()
        })
        .collect();
    changed.then(|| rebuilt.join("&"))
}

/// Mask query and fragment secrets by structure. `None` when nothing changed.
fn mask_url_structure(input: &str) -> Option<String> {
    let (rest, fragment) = match input.split_once('#') {
        Some((a, b)) => (a, Some(b)),
        None => (input, None),
    };
    let (base, query) = match rest.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let query2 = query.and_then(|q| mask_pairs(q, is_query_key));
    let fragment2 = fragment.and_then(|f| mask_pairs(f, is_fragment_key));
    if query2.is_none() && fragment2.is_none() {
        return None;
    }
    let mut out = String::with_capacity(input.len() + 16);
    out.push_str(base);
    if let Some(q) = query2.as_deref().or(query) {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = fragment2.as_deref().or(fragment) {
        out.push('#');
        out.push_str(f);
    }
    Some(out)
}

/// [`scrub_live`] for a URL: the scheme, host, path and parameter names are kept, and
/// what carries a secret is masked. That is a userinfo password, a credential-shaped
/// string anywhere in it, the value of a query parameter in [`URL_QUERY_KEYS`] (or
/// starting with `x-amz-`), and the value of a fragment parameter in
/// [`URL_FRAGMENT_KEYS`]. The work is done on the string, never by re-serializing a
/// parsed URL, so a URL with nothing to mask comes back byte-identical. Idempotent.
#[must_use]
pub fn scrub_url(url: &str) -> String {
    let mut out = scrub_live(url);
    if !out.contains(char::is_whitespace) {
        if let Some(masked) = mask_url_structure(&out) {
            out = masked;
        }
    }
    let c = compiled();
    if let Some(masked) = run_rules(URL_TEXT_RULES, &c.url, &c.url_set, &out) {
        out = masked;
    }
    out
}

/// Whether an object key names a field that holds a URL, so its string goes
/// through [`scrub_url`] (query and fragment secrets too) rather than [`scrub_live`].
fn is_url_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    matches!(
        key.as_str(),
        "url" | "href" | "src" | "uri" | "location" | "origin" | "referrer" | "referer"
    ) || key.ends_with("url")
        || key.ends_with("_href")
        || key.ends_with("_uri")
}

fn scrub_live_json_inner(value: &mut Value, urlish: bool) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                scrub_live_json_inner(v, is_url_key(key));
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                scrub_live_json_inner(item, urlish);
            }
        }
        Value::String(s) => {
            let scrubbed = if urlish { scrub_url(s) } else { scrub_live(s) };
            if scrubbed != *s {
                *s = scrubbed;
            }
        }
        _ => {}
    }
}

/// [`scrub_live`] over every string in a JSON tree, in place. A string under a key
/// that names a URL (`url`, `href`, `src`, `page_url`, ...) goes through
/// [`scrub_url`]. Keys and non-string values are never touched, and there is no
/// sensitive-key rule here: that is the legacy [`scrub_json_value`], right for a
/// checkpoint and wrong for a page snapshot whose field names are the page's own.
pub fn scrub_live_json(value: &mut Value) {
    scrub_live_json_inner(value, false);
}

/// Flag names whose NEXT argv element is the secret.
///
/// Every pattern in this module keys on a `:` or `=` separator, so a
/// space-separated pair (`--token abc123`, `-p hunter2`) passes through all of
/// them untouched. Matching that is positional and cannot be done on a joined
/// string, which is why [`redact_command`] exists rather than callers doing
/// `scrub_string(&argv.join(" "))`.
///
/// Deliberately narrow, and mirrored by sandbox-manager's own
/// `internal/redact`. `--key` and `-k` are NOT here: `--key` is a file path
/// far more often than a secret and `-k` is curl's insecure flag, so including
/// them would redact ordinary arguments and teach readers to distrust the
/// redaction.
const SECRET_FLAGS: &[&str] = &[
    "token",
    "auth-token",
    "access-token",
    "refresh-token",
    "password",
    "passwd",
    "pwd",
    "pass",
    "p",
    "secret",
    "client-secret",
    "secret-key",
    "api-key",
    "apikey",
    "access-key",
    "private-key",
    "credential",
    "credentials",
    "auth",
];

/// Redact a command's identity for durable storage: [`scrub_string`] over the
/// program and every argument, plus the positional [`SECRET_FLAGS`] rule that
/// no text pattern can express.
///
/// Returns copies; the caller's argv is untouched, because the unredacted form
/// is still what actually gets spawned.
#[must_use]
pub fn redact_command(program: &str, args: &[String]) -> (String, Vec<String>) {
    let mut redacted: Vec<String> = args.iter().map(|arg| scrub_string(arg)).collect();
    for index in 0..args.len().saturating_sub(1) {
        if !is_secret_flag(&args[index]) {
            continue;
        }
        // A following token that is itself a flag is the next option, not this
        // one's value.
        if args[index + 1].starts_with('-') {
            continue;
        }
        REDACTED.clone_into(&mut redacted[index + 1]);
    }
    (scrub_string(program), redacted)
}

/// Whether `arg` is a flag whose value must be redacted. An `=`-joined form is
/// left to the inline capture pattern in [`scrub_string`].
fn is_secret_flag(arg: &str) -> bool {
    let Some(name) = arg.strip_prefix('-') else {
        return false;
    };
    let name = name.trim_start_matches('-');
    if name.is_empty() || name.contains('=') {
        return false;
    }
    let normalized = name.to_ascii_lowercase().replace('_', "-");
    SECRET_FLAGS.contains(&normalized.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_sensitive_keys() {
        let mut v = json!({
            "tool_name": "bash",
            "env": { "API_KEY": "super-secret", "PATH": "/usr/bin" },
            "password": "hunter2",
            "note": "all good"
        });
        scrub_json_value(&mut v);
        assert_eq!(v["env"]["API_KEY"], json!(REDACTED));
        assert_eq!(v["env"]["PATH"], json!("/usr/bin"));
        assert_eq!(v["password"], json!(REDACTED));
        assert_eq!(v["tool_name"], json!("bash"));
        assert_eq!(v["note"], json!("all good"));
    }

    #[test]
    fn redacts_bearer_and_jwt_in_strings() {
        let out = scrub_string(
            "Authorization: Bearer eyJhbGciOi.eyJzdWIi.sig and sk-abcdef1234567890XYZ",
        );
        assert!(out.contains(REDACTED));
        assert!(!out.contains("eyJhbGciOi"));
        assert!(!out.contains("sk-abcdef"));
    }

    #[test]
    fn preserves_non_sensitive_content() {
        let mut v = json!({"step": {"id": "s1", "status": "completed", "output": "hello"}});
        scrub_json_value(&mut v);
        assert_eq!(v["step"]["output"], json!("hello"));
        assert_eq!(v["step"]["status"], json!("completed"));
    }

    #[test]
    fn redacts_connection_string_password_keeps_host() {
        // Valid DSNs URL-encode reserved chars, so the password has no raw '@'.
        let out = scrub_string("postgres://svc_user:s3cr3tPass@db.internal:5432/app");
        assert!(out.contains("[REDACTED]"), "got: {out}");
        assert!(!out.contains("s3cr3tPass"), "password leaked: {out}");
        // host + user + scheme preserved for diagnosability
        assert!(out.contains("postgres://svc_user:"));
        assert!(out.contains("@db.internal:5432/app"));
    }

    #[test]
    fn redacts_inline_assignments_keeps_key() {
        let out = scrub_string("export DB_PASSWORD=hunter2 && api_key: \"abc123def456\"");
        assert!(!out.contains("hunter2"), "value leaked: {out}");
        assert!(!out.contains("abc123def456"), "value leaked: {out}");
        // key names preserved
        assert!(out.contains("DB_PASSWORD="));
        assert!(out.contains("api_key"));
    }

    #[test]
    fn redacts_slack_and_google_keys() {
        // Same inputs as the original test; the Google key is joined at run time so the
        // source holds no key-shaped literal (the secret gate scans a moved test as new).
        let google = ["AIzaSyA1234567890", "abcdefghijklmnopqrstuvw"].concat();
        let out = scrub_string(&format!(
            "tok=xoxb-1234567890-abcdefghijklmnop key={google}"
        ));
        assert!(!out.contains("xoxb-1234567890"), "slack leaked: {out}");
        assert!(!out.contains("AIzaSyA1234567890"), "google leaked: {out}");
    }

    #[test]
    fn preserves_ordinary_assignments() {
        // Non-secret key=value must survive untouched.
        let out = scrub_string("status=completed path=/usr/bin count=42");
        assert_eq!(out, "status=completed path=/usr/bin count=42");
    }

    #[test]
    fn redact_command_covers_a_separated_flag_value() {
        // The rule that exists only here: every pattern above keys on a ':'
        // or '=', so `--token abc123` as two argv elements reaches none of
        // them.
        let secret = "s".repeat(20);
        let args = vec![
            "--token".to_owned(),
            secret.clone(),
            "https://api.example.com".to_owned(),
        ];
        let (program, redacted) = redact_command("curl", &args);
        assert_eq!(program, "curl");
        assert_eq!(redacted[1], REDACTED);
        assert_eq!(redacted[2], "https://api.example.com");
        assert_eq!(args[1], secret, "the caller's argv must not be mutated");
    }

    #[test]
    fn redact_command_recognizes_each_flag_spelling() {
        let secret = "s".repeat(20);
        for flag in [
            "--token",
            "--password",
            "-p",
            "--api-key",
            "--API_KEY",
            "--access-key",
            "--auth",
            "--client-secret",
        ] {
            let args = vec![flag.to_owned(), secret.clone()];
            let (_, redacted) = redact_command("tool", &args);
            assert_eq!(redacted[1], REDACTED, "flag {flag} did not redact");
        }
    }

    #[test]
    fn redact_command_leaves_the_joined_form_to_the_patterns() {
        // `--token=x` never reaches the positional rule (an '=' disqualifies
        // it); the inline capture pattern handles it instead, so the two
        // rules compose rather than overlap.
        let secret = "s".repeat(20);
        let args = vec![format!("--token={secret}")];
        let (_, redacted) = redact_command("tool", &args);
        assert!(!redacted[0].contains(&secret));
        assert!(redacted[0].starts_with("--token="));
    }

    #[test]
    fn redact_command_does_not_swallow_the_next_flag_or_an_ordinary_path() {
        // `--token --verbose` is a missing value, not a secret; and `--key`
        // is deliberately not a secret flag because it is a file path far
        // more often than a credential.
        let args = vec!["--token".to_owned(), "--verbose".to_owned()];
        let (_, redacted) = redact_command("tool", &args);
        assert_eq!(redacted, vec!["--token", "--verbose"]);

        let args = vec!["--key".to_owned(), "/etc/ssl/app.pem".to_owned()];
        let (_, redacted) = redact_command("tool", &args);
        assert_eq!(redacted, vec!["--key", "/etc/ssl/app.pem"]);
    }

    #[test]
    fn redact_command_handles_a_trailing_secret_flag() {
        let args = vec!["run".to_owned(), "--token".to_owned()];
        let (_, redacted) = redact_command("tool", &args);
        assert_eq!(redacted, vec!["run", "--token"]);
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use serde_json::json;

    fn jwt() -> String {
        [
            "eyJhbGciOiJI",
            "UzI1NiIsInR5cCI6",
            "IkpXVCJ9",
            ".eyJzdWIiOiIxMjM0",
            "NTY3ODkwIiwibmFtZSI6Ikpv",
            ".SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
        ]
        .concat()
    }

    #[test]
    fn marker_is_the_legacy_marker() {
        assert_eq!(MARKER, "[REDACTED]");
        assert_eq!(scrub_live(&format!("t {}", jwt())), "t [REDACTED]");
    }

    #[test]
    fn live_keeps_prose_the_legacy_set_would_eat() {
        for prose in [
            "the bearer of the message arrived",
            "Basic infrastructure overview is below",
            "max_tokens=4096 token_count: 12 tokens used",
            "password=hunter2 is only a Durable rule",
            "status=completed path=/usr/bin count=42",
            "send Bearer nightingaleinthedark here",
        ] {
            assert_eq!(scrub_live(prose), prose, "{prose}");
        }
        // The legacy set masks the first of these: that is why the Live set exists.
        assert_ne!(scrub_string("the bearer of the message arrived"), "the bearer of the message arrived");
    }

    #[test]
    fn bearer_in_a_header_is_masked_whatever_its_length() {
        assert_eq!(
            scrub_live("Authorization: Bearer abcdef"),
            "Authorization: Bearer [REDACTED]"
        );
        assert_eq!(
            scrub_live("send Bearer abcdefghijklmnop1 here"),
            "send Bearer [REDACTED] here"
        );
        // Fifteen characters with a digit are below the bare minimum.
        let short = "send Bearer abcdefghijklmn1 here";
        assert_eq!(scrub_live(short), short);
    }

    #[test]
    fn userinfo_password_is_masked_and_the_host_kept() {
        assert_eq!(
            scrub_live("postgres://svc:hunter2-pw@db.internal:5432/app"),
            "postgres://svc:[REDACTED]@db.internal:5432/app"
        );
    }

    #[test]
    fn a_pem_without_an_end_line_is_masked_to_the_end() {
        let s = "before\n-----BEGIN PRIVATE KEY-----\nAAAA\nBBBB";
        assert_eq!(scrub_live(s), "before\n[REDACTED]");
    }

    #[test]
    fn live_is_idempotent_on_every_shape() {
        // Built from fragments: no secret-shaped literal in the source (the secret gate).
        let pat = ["ghp_", "aB3dE6gH9jK2mN5pQ8sT1uV4wX7yZ0"].concat();
        let akid = ["AKIA", "IOSFODNN7EXAMPLE"].concat();
        let key = ["sk-proj-", "Ab12Cd34Ef56", "Gh78Ij90Kl12"].concat();
        let basic = ["dXNlcjpw", "YXNzd29yZA=="].concat();
        let input = format!(
            "{} Authorization: Bearer tok_8f3a1d9 Basic {basic}              postgres://svc:hunter2-pw@db/app {pat} {akid} {key}",
            jwt()
        );
        let once = scrub_live(&input);
        assert!(!once.contains("eyJ") && !once.contains("hunter2") && !once.contains("AKIA"));
        assert_eq!(scrub_live(&once), once);
    }

    #[test]
    fn url_query_and_fragment_secrets_are_masked_and_names_kept() {
        assert_eq!(
            scrub_url("https://api.example.com/v1/items?page=2&access_token=abc123&limit=5"),
            "https://api.example.com/v1/items?page=2&access_token=[REDACTED]&limit=5"
        );
        assert_eq!(
            scrub_url("https://app.example.com/cb#access_token=abc123&token_type=bearer&state=xyz"),
            "https://app.example.com/cb#access_token=[REDACTED]&token_type=bearer&state=xyz"
        );
        assert_eq!(
            scrub_url("https://svc:hunter2@h.example/p?token=abc123"),
            "https://svc:[REDACTED]@h.example/p?token=[REDACTED]"
        );
        assert_eq!(scrub_url("/cb?token=abc123"), "/cb?token=[REDACTED]");
    }

    #[test]
    fn url_keys_are_percent_decoded_and_html_escaped_separators_caught() {
        assert_eq!(
            scrub_url("https://h.example/p?access%5Ftoken=abc123"),
            "https://h.example/p?access%5Ftoken=[REDACTED]"
        );
        assert_eq!(
            scrub_url("https://h.example/p?a=1&amp;token=abc123"),
            "https://h.example/p?a=1&amp;token=[REDACTED]"
        );
        assert_eq!(
            scrub_url("https://h.example/p?X-Amz-Signature=deadbeef"),
            "https://h.example/p?X-Amz-Signature=[REDACTED]"
        );
    }

    #[test]
    fn an_ordinary_url_comes_back_byte_identical() {
        for u in [
            "https://example.com/search?q=rust&page=2#top",
            "GET /v1/items?page=2&sort=name&limit=5 200",
            "https://example.com/",
            "about:blank",
        ] {
            assert_eq!(scrub_url(u), u);
        }
    }

    #[test]
    fn url_scrub_is_idempotent() {
        let once = scrub_url("https://u:pw@h.example/p?code=abc123&X-Amz-Signature=deadbeef#id_token=zzz");
        assert_eq!(scrub_url(&once), once);
        assert!(!once.contains("abc123") && !once.contains("deadbeef") && !once.contains("zzz"));
    }

    #[test]
    fn json_strings_are_scrubbed_keys_and_numbers_are_not() {
        let mut v = json!({
            "password": "hunter2",
            "page_url": "https://h.example/p?token=abc123",
            "nodes": [{"text": format!("tok {}", jwt()), "count": 3, "ok": true}],
            "note": "the bearer of the message arrived",
        });
        scrub_live_json(&mut v);
        // No sensitive-key rule in the Live set: the legacy scrub_json_value owns that.
        assert_eq!(v["password"], json!("hunter2"));
        assert_eq!(v["page_url"], json!("https://h.example/p?token=[REDACTED]"));
        assert_eq!(v["nodes"][0]["text"], json!("tok [REDACTED]"));
        assert_eq!(v["nodes"][0]["count"], json!(3));
        assert_eq!(v["note"], json!("the bearer of the message arrived"));
        let again = v.clone();
        scrub_live_json(&mut v);
        assert_eq!(v, again);
    }

    #[test]
    fn query_secret_in_plain_text_is_left_to_the_shapes() {
        // scrub_live does not know a page's prose from a URL; scrub_url does.
        let s = "see ?token=abc123 in the docs";
        assert_eq!(scrub_live(s), s);
    }
}
