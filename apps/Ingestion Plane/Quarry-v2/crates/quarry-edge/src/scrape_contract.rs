//! The `/v1/scrape` request contract: which keys are options, which are
//! refused, which are merely unknown, and how strict the edge is about them.
//!
//! The endpoint used to deserialize its body straight into a struct, so an
//! unknown key vanished without a trace and a malformed body got axum's plain
//! rejection. A caller (the gateway, a Model Plane agent) that sent
//! `{"formats":["markdown"],"maxPages":1}` was told nothing about either. Now:
//!
//! - A key in [`quarry_core::scrape::RESERVED_REJECTED`] is a 400 from the first
//!   request, whatever the strictness: those would change what a request is
//!   allowed to do (TLS verification, proxies, headers, stealth).
//! - An unknown key is ignored with a `warnings[] {code: unknown_field, field}`
//!   entry, an `effective_options.ignored_fields` entry, a log line naming the
//!   keys (names only, never values), and `Deprecation`/`Sunset` headers when a
//!   sunset date is configured. In strict mode it is a 400 instead.
//! - A bad value is a 400 `BAD_REQUEST` with `details.fields[] {path, code,
//!   message}`, so a caller can correct itself.
//!
//! Strictness is latched at start from `QUARRY_EDGE__SCRAPE_REQUEST_VALIDATION`
//! (`lenient`, the default, or `strict`) and `QUARRY_EDGE__SCRAPE_STRICT_AFTER`
//! (a date: strict from then on, without a redeploy). Flip it once the consumers
//! send only known fields; the `quarry.scrape.unknown_field` log line shows who
//! still does not.

pub mod ai_formats;

use std::sync::OnceLock;

use axum::http::{HeaderName, HeaderValue};
use chrono::{DateTime, NaiveDate, Utc};
use quarry_core::scrape::{
    FieldError, ScrapeLimits, ScrapeOptions, ScrapeWarning, RESERVED_REJECTED,
};
use serde_json::Value;

/// The keys of the request itself (what `ScrapeRequest` reads). `policy` is
/// reserved for the run-policy field of a later slice: accepted, not applied.
pub const REQUEST_KEYS: [&str; 11] = [
    "url",
    "prev_fingerprint",
    "cache",
    "zdr",
    "privacy",
    "signals",
    "ingest",
    "org_id",
    "render",
    "prefer_http3",
    "policy",
];

const INLINE_CEILING: u64 = 8 * 1024 * 1024;

/// What the operator configured, read once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrapeSettings {
    pub strict_validation: bool,
    pub strict_after: Option<DateTime<Utc>>,
    pub inline_max_bytes: u64,
    /// `QUARRY_EDGE__SCRAPE_OPTIONS_ENABLED=0` turns the option keys back into
    /// unknown keys (ignored with a warning): the kill switch.
    pub options_enabled: bool,
    /// `QUARRY_EDGE__SCRAPE_RECORDS=0` stops storing a record per scrape and
    /// omits `scrape_id`.
    pub records_enabled: bool,
}

impl Default for ScrapeSettings {
    fn default() -> Self {
        Self::from_lookup(|_| None)
    }
}

impl ScrapeSettings {
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let strict_validation = match lookup("QUARRY_EDGE__SCRAPE_REQUEST_VALIDATION")
            .as_deref()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            None | Some("" | "lenient") => false,
            Some("strict") => true,
            Some(other) => {
                tracing::warn!(
                    value = other,
                    "QUARRY_EDGE__SCRAPE_REQUEST_VALIDATION is not lenient or strict; using lenient"
                );
                false
            }
        };
        let strict_after = lookup("QUARRY_EDGE__SCRAPE_STRICT_AFTER")
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| {
                let parsed = DateTime::parse_from_rfc3339(value)
                    .map(|date| date.with_timezone(&Utc))
                    .ok()
                    .or_else(|| {
                        NaiveDate::parse_from_str(value, "%Y-%m-%d")
                            .ok()
                            .and_then(|date| date.and_hms_opt(0, 0, 0))
                            .map(|date| date.and_utc())
                    });
                if parsed.is_none() {
                    tracing::warn!(
                        "QUARRY_EDGE__SCRAPE_STRICT_AFTER is not an RFC 3339 time or YYYY-MM-DD; ignored"
                    );
                }
                parsed
            });
        let inline_max_bytes = lookup("QUARRY_EDGE__SCRAPE_INLINE_MAX_BYTES")
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|value| *value > 0)
            .map(|value| value.min(INLINE_CEILING))
            .unwrap_or(quarry_core::scrape::DEFAULT_INLINE_MAX_BYTES as u64);
        let options_enabled = !matches!(
            lookup("QUARRY_EDGE__SCRAPE_OPTIONS_ENABLED")
                .as_deref()
                .map(|value| value.trim().to_ascii_lowercase())
                .as_deref(),
            Some("0" | "false" | "off")
        );
        let records_enabled = !matches!(
            lookup("QUARRY_EDGE__SCRAPE_RECORDS")
                .as_deref()
                .map(|value| value.trim().to_ascii_lowercase())
                .as_deref(),
            Some("0" | "false" | "off")
        );
        Self {
            strict_validation,
            strict_after,
            inline_max_bytes,
            options_enabled,
            records_enabled,
        }
    }

    /// The process settings, read once from the environment.
    pub fn current() -> &'static ScrapeSettings {
        static SETTINGS: OnceLock<ScrapeSettings> = OnceLock::new();
        SETTINGS.get_or_init(|| Self::from_lookup(|name| std::env::var(name).ok()))
    }

    pub fn is_strict(&self, now: DateTime<Utc>) -> bool {
        self.strict_validation || self.strict_after.is_some_and(|after| now >= after)
    }

    pub fn limits(&self) -> ScrapeLimits {
        ScrapeLimits {
            default_inline_bytes: self.inline_max_bytes,
            captures_enabled: crate::scrape_actions::capture_mode().enabled(),
            actions_enabled: crate::scrape_actions::actions_mode().enabled(),
            ai_formats_enabled: ai_formats::enabled(),
            ..ScrapeLimits::default()
        }
    }
}

/// A request body, checked.
#[derive(Debug, Clone)]
pub struct ParsedScrape {
    pub value: Value,
    pub options: ScrapeOptions,
    /// Keys that are not part of the contract and were ignored (lenient mode).
    pub unknown_keys: Vec<String>,
    pub strict: bool,
}

/// Check a request body. All problems of one kind are reported together.
pub fn parse_body(
    body: &[u8],
    settings: &ScrapeSettings,
    now: DateTime<Utc>,
) -> Result<ParsedScrape, Vec<FieldError>> {
    let value: Value = serde_json::from_slice(body).map_err(|error| {
        vec![FieldError::new(
            "$",
            "invalid_json",
            format!("the request body is not valid JSON: {error}"),
        )]
    })?;
    let Some(object) = value.as_object() else {
        return Err(vec![FieldError::new(
            "$",
            "invalid_type",
            "the request body must be a JSON object",
        )]);
    };

    let reserved: Vec<FieldError> = RESERVED_REJECTED
        .iter()
        .filter(|key| object.contains_key(**key))
        .map(|key| {
            FieldError::new(
                *key,
                "reserved_field",
                format!("`{key}` is not accepted by /v1/scrape"),
            )
        })
        .collect();
    if !reserved.is_empty() {
        return Err(reserved);
    }

    let option_names = ScrapeOptions::field_names();
    let mut unknown_keys: Vec<String> = object
        .keys()
        .filter(|key| {
            !REQUEST_KEYS.contains(&key.as_str())
                && !(settings.options_enabled && option_names.contains(&key.as_str()))
        })
        .cloned()
        .collect();
    unknown_keys.sort();

    let strict = settings.is_strict(now);
    if strict && !unknown_keys.is_empty() {
        return Err(unknown_keys
            .iter()
            .map(|key| {
                FieldError::new(
                    key,
                    "unknown_field",
                    format!("`{key}` is not a /v1/scrape field"),
                )
            })
            .collect());
    }

    let options = if settings.options_enabled {
        let options = ScrapeOptions::from_value(&value)?;
        let mut bad = Vec::new();
        for (name, selectors) in [
            ("include_tags", options.include_tags.as_deref()),
            ("exclude_tags", options.exclude_tags.as_deref()),
        ] {
            for (index, reason) in
                quarry_transform::clean_html::validate_selectors(selectors.unwrap_or_default())
            {
                bad.push(FieldError::new(
                    format!("{name}[{index}]"),
                    "invalid_selector",
                    format!("not a CSS selector: {reason}"),
                ));
            }
        }
        if !bad.is_empty() {
            return Err(bad);
        }
        options
    } else {
        ScrapeOptions::default()
    };
    Ok(ParsedScrape {
        value,
        options,
        unknown_keys,
        strict,
    })
}

/// One `unknown_field` warning per ignored key.
pub fn unknown_field_warnings(keys: &[String]) -> Vec<ScrapeWarning> {
    keys.iter()
        .map(|key| {
            ScrapeWarning::for_field(
                "unknown_field",
                key,
                format!("`{key}` is not a /v1/scrape field and was ignored"),
            )
        })
        .collect()
}

/// `Deprecation: true`, and `Sunset` when a strict-after date is configured, for
/// a response that carried unknown-field warnings.
pub fn deprecation_headers(settings: &ScrapeSettings) -> Vec<(HeaderName, HeaderValue)> {
    let mut headers = vec![(
        HeaderName::from_static("deprecation"),
        HeaderValue::from_static("true"),
    )];
    if let Some(after) = settings.strict_after {
        let http_date = after.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        if let Ok(value) = HeaderValue::from_str(&http_date) {
            headers.push((HeaderName::from_static("sunset"), value));
        }
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(body: Value, settings: &ScrapeSettings) -> Result<ParsedScrape, Vec<FieldError>> {
        parse_body(body.to_string().as_bytes(), settings, Utc::now())
    }

    fn lenient() -> ScrapeSettings {
        ScrapeSettings::default()
    }

    fn strict() -> ScrapeSettings {
        ScrapeSettings {
            strict_validation: true,
            ..ScrapeSettings::default()
        }
    }

    #[test]
    fn lenient_mode_ignores_unknown_keys_and_names_them() {
        let parsed = parse(
            json!({ "url": "https://example.com/a", "formats": ["markdown"], "maxPages": 1, "waitFor": 5 }),
            &lenient(),
        )
        .unwrap();
        assert_eq!(parsed.unknown_keys, vec!["maxPages", "waitFor"]);
        assert!(!parsed.strict);
        let warnings = unknown_field_warnings(&parsed.unknown_keys);
        assert_eq!(warnings[0].code, "unknown_field");
        assert_eq!(warnings[0].field.as_deref(), Some("maxPages"));
    }

    #[test]
    fn strict_mode_rejects_unknown_keys_with_their_paths() {
        let errors = parse(
            json!({ "url": "https://example.com", "maxPages": 1 }),
            &strict(),
        )
        .unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, "maxPages");
        assert_eq!(errors[0].code, "unknown_field");
    }

    #[test]
    fn a_reserved_key_is_rejected_even_in_lenient_mode() {
        for key in [
            "skipTlsVerification",
            "skip_tls_verification",
            "proxy",
            "proxyUrl",
            "headers",
            "mobile",
            "stealth",
        ] {
            let errors = parse(
                json!({ "url": "https://example.com", key: true }),
                &lenient(),
            )
            .unwrap_err();
            assert_eq!(errors[0].code, "reserved_field", "{key}");
            assert_eq!(errors[0].path, key);
        }
    }

    #[test]
    fn org_id_prefer_http3_and_render_are_known_and_silent() {
        let parsed = parse(
            json!({ "url": "https://example.com", "org_id": "x", "prefer_http3": true, "render": {}, "policy": {}, "ingest": false }),
            &lenient(),
        )
        .unwrap();
        assert!(parsed.unknown_keys.is_empty());
        // And they are fine under strict.
        assert!(parse(
            json!({ "url": "https://example.com", "org_id": "x", "render": {} }),
            &strict()
        )
        .is_ok());
    }

    #[test]
    fn the_request_keys_are_exactly_what_scrape_request_reads() {
        // ScrapeRequest's serde field names, plus the reserved `policy`.
        let mut expected = vec![
            "url",
            "prev_fingerprint",
            "cache",
            "zdr",
            "privacy",
            "signals",
            "ingest",
            "org_id",
            "render",
            "prefer_http3",
            "policy",
        ];
        expected.sort_unstable();
        let mut actual = REQUEST_KEYS.to_vec();
        actual.sort_unstable();
        assert_eq!(actual, expected);
        // No request key is an option, so a key never has two meanings.
        for key in REQUEST_KEYS {
            assert!(!ScrapeOptions::field_names().contains(&key), "{key}");
        }
    }

    #[test]
    fn bad_option_values_come_back_as_field_errors() {
        // An unknown format name is a field error at its index. ("pdf" is a format now.)
        let errors = parse(
            json!({ "url": "https://example.com", "formats": ["markdown", "docx"] }),
            &lenient(),
        )
        .unwrap_err();
        assert_eq!(errors[0].path, "formats[1]");
        assert_eq!(errors[0].code, "unsupported_format");
        // A bad value inside a capture option is a field error at the format's index.
        let errors = parse(
            json!({ "url": "https://example.com", "formats": [{ "type": "pdf", "format": "Quarto" }] }),
            &lenient(),
        )
        .unwrap_err();
        assert_eq!(errors[0].path, "formats[0]");
        assert_eq!(errors[0].code, "invalid_type");
    }

    #[test]
    fn a_body_that_is_not_json_or_not_an_object_is_a_field_error_not_a_plain_rejection() {
        let errors = parse_body(b"{not json", &lenient(), Utc::now()).unwrap_err();
        assert_eq!(errors[0].code, "invalid_json");
        let errors = parse_body(b"[1,2]", &lenient(), Utc::now()).unwrap_err();
        assert_eq!(errors[0].code, "invalid_type");
    }

    #[test]
    fn the_kill_switch_turns_option_keys_back_into_unknown_keys() {
        let settings = ScrapeSettings {
            options_enabled: false,
            ..ScrapeSettings::default()
        };
        let parsed = parse(
            json!({ "url": "https://example.com", "formats": ["markdown"] }),
            &settings,
        )
        .unwrap();
        assert_eq!(parsed.unknown_keys, vec!["formats"]);
        assert_eq!(parsed.options, ScrapeOptions::default());
    }

    #[test]
    fn settings_read_the_environment_names_and_latch_strict_after() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            ScrapeSettings::from_lookup(move |name| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_owned())
            })
        };
        assert!(!env(&[]).strict_validation);
        assert!(env(&[("QUARRY_EDGE__SCRAPE_REQUEST_VALIDATION", "strict")]).strict_validation);
        // A value that is neither lenient nor strict is lenient, not an accident of typos.
        assert!(!env(&[("QUARRY_EDGE__SCRAPE_REQUEST_VALIDATION", "stricter")]).strict_validation);

        let dated = env(&[("QUARRY_EDGE__SCRAPE_STRICT_AFTER", "2026-12-01")]);
        let before = DateTime::parse_from_rfc3339("2026-11-30T23:59:59Z")
            .unwrap()
            .with_timezone(&Utc);
        let after = DateTime::parse_from_rfc3339("2026-12-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!dated.is_strict(before));
        assert!(dated.is_strict(after));

        assert_eq!(env(&[]).inline_max_bytes, 1_048_576);
        assert_eq!(
            env(&[("QUARRY_EDGE__SCRAPE_INLINE_MAX_BYTES", "65536")]).inline_max_bytes,
            65_536
        );
        assert_eq!(
            env(&[("QUARRY_EDGE__SCRAPE_INLINE_MAX_BYTES", "999999999")]).inline_max_bytes,
            INLINE_CEILING
        );
        assert!(!env(&[("QUARRY_EDGE__SCRAPE_OPTIONS_ENABLED", "0")]).options_enabled);
        assert!(env(&[]).options_enabled);
        assert!(env(&[]).records_enabled);
        assert!(!env(&[("QUARRY_EDGE__SCRAPE_RECORDS", "0")]).records_enabled);
    }

    #[test]
    fn deprecation_headers_carry_the_sunset_date_when_configured() {
        let none = deprecation_headers(&ScrapeSettings::default());
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].0.as_str(), "deprecation");
        let settings = ScrapeSettings {
            strict_after: Some(
                DateTime::parse_from_rfc3339("2026-12-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            ..ScrapeSettings::default()
        };
        let headers = deprecation_headers(&settings);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[1].0.as_str(), "sunset");
        assert_eq!(
            headers[1].1.to_str().unwrap(),
            "Tue, 01 Dec 2026 00:00:00 GMT"
        );
    }
}
