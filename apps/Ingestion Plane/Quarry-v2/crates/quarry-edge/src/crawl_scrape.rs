//! INNH-6b: the `scrape_options` of `POST /v1/crawl`, a per-page extraction subset.
//!
//! `/v1/scrape` takes a whole option set (formats, cleaning, captures, actions, AI
//! formats). A crawl runs the same cleaning on every page it fetches, so only the
//! options that shape the extracted text apply per page:
//!
//! - `formats`: `markdown`, `html`, `rawHtml`, `links`, `metadata` (accepted for
//!   Firecrawl parity; a crawl page stores its artifacts whatever the list says,
//!   so the list never changes a stored result and never defeats the page cache)
//! - `only_main_content`, `include_tags`, `exclude_tags`, `remove_base64_images`,
//!   `extractor`: the cleaning that makes the markdown (and so the stored artifact
//!   and what `ingest: true` sends to the Data Plane)
//! - `timeout_ms`: the bound on one page's run, 1000 to 55000
//! - `redact_pii`: `on` redacts each page's text before anything is stored, through
//!   the pipeline stage; it needs `QUARRY_EDGE__PII_REDACTION` on (see `crawl_request`)
//!
//! `actions`, `actions_on_error`, `max_inline_bytes`, `screenshot`, `pdf`, `json`,
//! `summary`, `question`, `highlights` and the capture or AI formats cannot apply
//! per page and are `unsupported_option`. The validators are the ones `/v1/scrape`
//! uses (`ScrapeOptions::from_value` and `resolve`), so the same limits and codes
//! apply; only the set of accepted keys is narrower.
//!
//! The handoff validates and stamps the canonical form ([`CrawlScrapeOptions`])
//! into the job params. `POST /v1/internal/run_page` re-validates it (defense in
//! depth: the orchestrator is trusted, a stored job param is not) and cleans with it.
//! Behind `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS`; see `crawl_policy::CrawlFeatures`.

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::scrape::{
    Extractor, FieldError, PiiRedaction, ScrapeLimits, ScrapeOptions, RESERVED_REJECTED,
};
use quarry_runtime::pipeline::PII_REDACTION_FAILED;
use quarry_transform::clean_html::CleanOptions;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The `formats` strings a crawl accepts.
pub const SUPPORTED_FORMATS: [&str; 6] = [
    "markdown", "html", "rawHtml", "raw_html", "links", "metadata",
];

/// Format names that exist on `/v1/scrape` but cannot apply per crawl page.
const UNSUPPORTED_FORMAT_NAMES: [&str; 6] = [
    "screenshot",
    "pdf",
    "summary",
    "json",
    "question",
    "highlights",
];

/// Each supported key, snake_case first and its camelCase alias.
const KEYS: [(&str, &str); 8] = [
    ("formats", "formats"),
    ("only_main_content", "onlyMainContent"),
    ("include_tags", "includeTags"),
    ("exclude_tags", "excludeTags"),
    ("remove_base64_images", "removeBase64Images"),
    ("extractor", "extractor"),
    ("timeout_ms", "timeoutMs"),
    ("redact_pii", "redactPii"),
];

/// Keys that exist on `/v1/scrape` (or are Firecrawl options) and cannot apply per page.
const UNSUPPORTED_KEYS: [&str; 16] = [
    // Q3-E3.8.8w: a stored-capture read is a per-request mode; dropped here, a crawl page
    // would be fetched live while the caller asked for no egress.
    "no_egress",
    "noEgress",
    "actions",
    "actions_on_error",
    "actionsOnError",
    "max_inline_bytes",
    "maxInlineBytes",
    "screenshot",
    "pdf",
    "json",
    "summary",
    "question",
    "highlights",
    "render",
    "wait_for",
    "waitFor",
];

/// The checked, canonical `scrape_options` of a crawl. This is what the handoff
/// stamps into the job params and what `run_page` receives as `scrape`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrawlScrapeOptions {
    /// The formats the caller named, wire names, de-duplicated. Absent when none were named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formats: Option<Vec<String>>,
    pub only_main_content: bool,
    pub include_tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub remove_base64_images: bool,
    pub extractor: Extractor,
    /// The bound on one page's run. Absent: the route's own limits apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// CW-1: `on` redacts each page's text before it is stored or returned. Absent for `off`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redact_pii: Option<PiiRedaction>,
}

/// A body that passed the checks, with the keys it carried that nothing reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub options: CrawlScrapeOptions,
    /// Keys that are not options (lenient mode only; strict refuses them).
    pub unknown: Vec<String>,
}

impl CrawlScrapeOptions {
    /// The cleaning every page of the crawl runs with.
    pub fn clean_options(&self) -> CleanOptions {
        CleanOptions {
            only_main_content: self.only_main_content,
            include_tags: self.include_tags.clone(),
            exclude_tags: self.exclude_tags.clone(),
            remove_base64_images: self.remove_base64_images,
            extractor: self.extractor,
        }
    }

    /// Whether the page cache may serve or store a page run with these options.
    /// The page cache is keyed by the page's fingerprint alone, so a page cleaned
    /// differently from the default must neither read nor write it. `formats` and
    /// `timeout_ms` shape delivery, not the stored result. A redacted page neither reads
    /// nor writes it either: the entry is keyed by the page alone, not by the redaction.
    pub fn cache_admissible(&self) -> bool {
        self.clean_options().is_default() && self.redact_pii.is_none()
    }

    /// The canonical JSON form (what is stamped and sent).
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// Re-check a stamped value. Lenient about unknown keys, so a later version can
    /// add an option without every page of an in-flight crawl failing.
    pub fn from_stamped(value: &Value) -> Result<Self, Vec<FieldError>> {
        check(value, false).map(|checked| checked.options)
    }
}

/// Check a `scrape_options` value. Paths are relative to the object (`include_tags[2]`).
/// Every problem is reported together. `strict` refuses an unknown key; otherwise it
/// is listed in [`Checked::unknown`].
pub fn check(value: &Value, strict: bool) -> Result<Checked, Vec<FieldError>> {
    let Some(object) = value.as_object() else {
        return Err(vec![FieldError::new(
            "$",
            "invalid_type",
            "scrape_options must be a JSON object",
        )]);
    };
    let mut errors: Vec<FieldError> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    let mut accepted = Map::new();
    for (key, item) in object {
        if KEYS
            .iter()
            .any(|(snake, camel)| key == snake || key == camel)
        {
            accepted.insert(key.clone(), item.clone());
        } else if UNSUPPORTED_KEYS.contains(&key.as_str()) {
            errors.push(FieldError::new(
                key.as_str(),
                "unsupported_option",
                "cannot apply to every page of a crawl (it needs a browser or a per-request body)",
            ));
        } else if RESERVED_REJECTED.contains(&key.as_str()) {
            errors.push(FieldError::new(
                key.as_str(),
                "reserved_field",
                "is not accepted by /v1/crawl",
            ));
        } else if strict {
            errors.push(FieldError::new(
                key.as_str(),
                "unknown_field",
                "is not a scrape_options field",
            ));
        } else {
            unknown.push(key.clone());
        }
    }

    // A format that cannot apply per page is reported at its own index, and replaced
    // by a placeholder so the shared validator neither repeats it nor shifts the
    // indices of the entries after it. The placeholder is never used: errors end the call.
    if let Some(Value::Array(items)) = accepted.get_mut("formats") {
        for (index, item) in items.iter_mut().enumerate() {
            let supported = item
                .as_str()
                .is_some_and(|t| SUPPORTED_FORMATS.contains(&t));
            if supported {
                continue;
            }
            let per_page_impossible = item.is_object()
                || item
                    .as_str()
                    .is_some_and(|t| UNSUPPORTED_FORMAT_NAMES.contains(&t));
            if per_page_impossible {
                errors.push(FieldError::new(
                    format!("formats[{index}]"),
                    "unsupported_option",
                    "this format cannot apply to every page of a crawl; use markdown, html, rawHtml, links or metadata",
                ));
                *item = Value::String("markdown".to_owned());
            }
        }
    }

    let mut parsed = match ScrapeOptions::from_value(&Value::Object(accepted)) {
        Ok(parsed) => Some(parsed),
        Err(found) => {
            errors.extend(found);
            None
        }
    };
    // A crawl never runs under ZDR (`refuse_durable_job_under_zdr`), so the scrape route's
    // rule that `redact_pii: on` needs ZDR would refuse every crawl. The pipeline stage
    // redacts a crawl page instead, and `run_page` refuses it while the stage is off.
    let redact_pii = parsed
        .as_ref()
        .and_then(|parsed| parsed.redact_pii)
        .filter(|option| *option == PiiRedaction::On);
    if let Some(parsed) = parsed.as_mut() {
        parsed.redact_pii = None;
    }
    let resolved = match parsed.as_ref() {
        Some(parsed) => match parsed.clone().resolve(false, &ScrapeLimits::default()) {
            Ok(resolved) => Some(resolved),
            Err(found) => {
                errors.extend(found);
                None
            }
        },
        None => None,
    };
    let (Some(parsed), Some(resolved)) = (parsed, resolved) else {
        return Err(errors);
    };
    if !errors.is_empty() {
        return Err(errors);
    }

    let formats = parsed.formats.as_ref().map(|list| {
        let mut names: Vec<String> = Vec::new();
        for format in list {
            let name = format.wire_name().to_owned();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    });
    Ok(Checked {
        options: CrawlScrapeOptions {
            formats,
            only_main_content: resolved.only_main_content,
            include_tags: resolved.include_tags,
            exclude_tags: resolved.exclude_tags,
            remove_base64_images: resolved.remove_base64_images,
            extractor: resolved.extractor,
            timeout_ms: parsed.timeout_ms,
            redact_pii,
        },
        unknown,
    })
}

/// CW-1: whether a stamped `scrape` value asks for redaction, in either spelling.
pub fn stamped_requests_redaction(value: &Value) -> bool {
    ["redact_pii", "redactPii"]
        .iter()
        .any(|key| value.get(*key).and_then(Value::as_str) == Some("on"))
}

/// CW-1: the refusal of a page that asks for redaction and cannot be served with it: the
/// crawl options are off, or the pipeline stage is off. Such a page is refused, never run
/// unredacted. `None` when the page may run.
pub fn redaction_refusal(
    requested: bool,
    scrape_options_on: bool,
    stage_on: bool,
) -> Option<QuarryError> {
    if !requested || (scrape_options_on && stage_on) {
        return None;
    }
    Some(
        QuarryError::new(
            ErrorCode::Forbidden,
            "redact_pii cannot be served: PII redaction is not enabled on this edge (QUARRY_EDGE__PII_REDACTION)",
        )
        .with_details(json!({ "reason": PII_REDACTION_FAILED })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn codes(result: &Result<Checked, Vec<FieldError>>) -> Vec<(String, String)> {
        match result {
            Ok(_) => Vec::new(),
            Err(errors) => errors
                .iter()
                .map(|error| (error.path.clone(), error.code.clone()))
                .collect(),
        }
    }

    fn pair(path: &str, code: &str) -> (String, String) {
        (path.to_owned(), code.to_owned())
    }

    #[test]
    fn an_empty_object_is_the_default_cleaning() {
        let checked = check(&json!({}), true).unwrap();
        assert!(checked.options.cache_admissible());
        assert!(checked.options.clean_options().is_default());
        assert_eq!(checked.options.formats, None);
        assert_eq!(checked.options.timeout_ms, None);
    }

    #[test]
    fn the_supported_subset_is_accepted_and_resolved() {
        let checked = check(
            &json!({
                "formats": ["markdown", "rawHtml", "markdown"],
                "only_main_content": false,
                "include_tags": ["article", ".post"],
                "exclude_tags": ["nav", "footer"],
                "remove_base64_images": true,
                "extractor": "clean_full",
                "timeout_ms": 20000,
            }),
            true,
        )
        .unwrap();
        let options = checked.options;
        assert_eq!(
            options.formats,
            Some(vec!["markdown".to_owned(), "raw_html".to_owned()])
        );
        assert!(!options.only_main_content);
        assert_eq!(options.include_tags, vec!["article", ".post"]);
        assert_eq!(options.exclude_tags, vec!["nav", "footer"]);
        assert!(options.remove_base64_images);
        assert_eq!(options.extractor, Extractor::CleanFull);
        assert_eq!(options.timeout_ms, Some(20_000));
        assert!(!options.cache_admissible());
    }

    #[test]
    fn camel_case_aliases_are_accepted() {
        let checked = check(
            &json!({ "onlyMainContent": false, "includeTags": ["main"], "excludeTags": ["aside"],
                     "removeBase64Images": true, "timeoutMs": 5000 }),
            true,
        )
        .unwrap();
        assert!(!checked.options.only_main_content);
        assert_eq!(checked.options.include_tags, vec!["main"]);
        assert_eq!(checked.options.exclude_tags, vec!["aside"]);
        assert_eq!(checked.options.timeout_ms, Some(5000));
    }

    #[test]
    fn formats_alone_do_not_defeat_the_page_cache() {
        let checked = check(
            &json!({ "formats": ["markdown", "links"], "timeout_ms": 9000 }),
            true,
        )
        .unwrap();
        assert!(checked.options.cache_admissible());
    }

    #[test]
    fn exclude_tags_and_the_extractor_defeat_the_page_cache() {
        assert!(!check(&json!({ "exclude_tags": ["nav"] }), true)
            .unwrap()
            .options
            .cache_admissible());
        assert!(!check(
            &json!({ "extractor": "raw", "only_main_content": false }),
            true
        )
        .unwrap()
        .options
        .cache_admissible());
        assert!(!check(&json!({ "remove_base64_images": true }), true)
            .unwrap()
            .options
            .cache_admissible());
    }

    #[test]
    fn options_that_cannot_apply_per_page_are_unsupported() {
        for key in [
            "actions",
            "actions_on_error",
            "actionsOnError",
            "max_inline_bytes",
            "screenshot",
            "pdf",
            "json",
            "summary",
            "question",
            "highlights",
        ] {
            let mut body = Map::new();
            body.insert(key.to_owned(), json!([]));
            let result = check(&Value::Object(body), false);
            assert_eq!(
                codes(&result),
                vec![pair(key, "unsupported_option")],
                "{key}"
            );
        }
    }

    #[test]
    fn capture_and_ai_formats_are_unsupported_at_their_index() {
        let result = check(
            &json!({ "formats": ["markdown", "screenshot", { "type": "pdf" }, "summary", "links"] }),
            false,
        );
        assert_eq!(
            codes(&result),
            vec![
                pair("formats[1]", "unsupported_option"),
                pair("formats[2]", "unsupported_option"),
                pair("formats[3]", "unsupported_option"),
            ]
        );
    }

    #[test]
    fn an_unknown_format_name_is_the_shared_validators_error() {
        let result = check(&json!({ "formats": ["markdwon"] }), false);
        assert_eq!(
            codes(&result),
            vec![pair("formats[0]", "unsupported_format")]
        );
    }

    #[test]
    fn reserved_keys_are_refused() {
        let result = check(&json!({ "headers": { "x": "y" }, "proxy": "p" }), false);
        let mut found = codes(&result);
        found.sort();
        assert_eq!(
            found,
            vec![
                pair("headers", "reserved_field"),
                pair("proxy", "reserved_field")
            ]
        );
    }

    #[test]
    fn redact_pii_on_is_kept_stamped_and_keeps_the_page_cache_apart() {
        // CW-1: the pipeline stage redacts a crawl page, so `on` is accepted here (the
        // scrape route's ZDR rule does not apply to a crawl) and carried to `run_page`.
        for key in ["redact_pii", "redactPii"] {
            let checked = check(&json!({ key: "on" }), false).unwrap();
            assert_eq!(checked.options.redact_pii, Some(PiiRedaction::On), "{key}");
            assert!(!checked.options.cache_admissible(), "{key}");
            let stamped = checked.options.to_value();
            assert_eq!(stamped["redact_pii"], json!("on"), "{key}");
            assert_eq!(
                CrawlScrapeOptions::from_stamped(&stamped).unwrap(),
                checked.options,
                "{key}"
            );
        }
    }

    #[test]
    fn redact_pii_off_is_the_default_and_is_not_stamped() {
        let off = check(&json!({ "redact_pii": "off" }), false).unwrap();
        assert_eq!(off.options.redact_pii, None);
        assert!(off.options.to_value().get("redact_pii").is_none());
        assert!(off.options.cache_admissible());
    }

    #[test]
    fn redact_pii_accepts_only_off_and_on() {
        for key in ["redact_pii", "redactPii"] {
            let found = codes(&check(&json!({ key: "maybe" }), false));
            assert_eq!(found.len(), 1, "{key}");
            assert_eq!(found[0].1, "unsupported_redact_pii", "{key}");
        }
    }

    #[test]
    fn stamped_redaction_is_read_in_either_spelling() {
        assert!(stamped_requests_redaction(&json!({ "redact_pii": "on" })));
        assert!(stamped_requests_redaction(&json!({ "redactPii": "on" })));
        assert!(!stamped_requests_redaction(&json!({ "redact_pii": "off" })));
        assert!(!stamped_requests_redaction(
            &json!({ "exclude_tags": ["nav"] })
        ));
        assert!(!stamped_requests_redaction(&Value::Null));
    }

    #[test]
    fn a_page_that_asks_for_redaction_is_refused_unless_both_switches_are_on() {
        assert!(redaction_refusal(false, false, false).is_none());
        assert!(redaction_refusal(true, true, true).is_none());
        for (scrape_on, stage_on) in [(true, false), (false, true), (false, false)] {
            let refusal = redaction_refusal(true, scrape_on, stage_on).expect("refused");
            assert_eq!(refusal.code, ErrorCode::Forbidden);
            assert_eq!(
                refusal.details.as_ref().expect("details")["reason"],
                PII_REDACTION_FAILED
            );
        }
    }

    #[test]
    fn selector_and_timeout_limits_are_the_scrape_limits() {
        let too_many: Vec<String> = (0..33).map(|i| format!(".c{i}")).collect();
        let result = check(
            &json!({ "include_tags": too_many, "exclude_tags": ["ok", " "], "timeout_ms": 999 }),
            false,
        );
        let found = codes(&result);
        assert!(
            found.contains(&pair("include_tags", "too_many")),
            "{found:?}"
        );
        assert!(
            found.contains(&pair("exclude_tags[1]", "empty")),
            "{found:?}"
        );
        assert!(
            found.contains(&pair("timeout_ms", "out_of_range")),
            "{found:?}"
        );
        let long = "a".repeat(257);
        let result = check(&json!({ "include_tags": ["ok", "ok", long] }), false);
        assert_eq!(codes(&result), vec![pair("include_tags[2]", "too_long")]);
        let result = check(&json!({ "timeout_ms": 55_001 }), false);
        assert_eq!(codes(&result), vec![pair("timeout_ms", "out_of_range")]);
    }

    #[test]
    fn extractor_and_only_main_content_must_agree() {
        let result = check(
            &json!({ "extractor": "raw", "only_main_content": true }),
            false,
        );
        assert_eq!(
            codes(&result),
            vec![pair("extractor", "conflicts_with_only_main_content")]
        );
        assert!(check(
            &json!({ "extractor": "raw", "only_main_content": false }),
            false
        )
        .is_ok());
        // An unset one follows the other.
        let follows = check(&json!({ "extractor": "raw" }), false).unwrap();
        assert!(!follows.options.only_main_content);
        let result = check(&json!({ "extractor": "nope" }), false);
        assert_eq!(
            codes(&result),
            vec![pair("extractor", "unsupported_extractor")]
        );
    }

    #[test]
    fn wrong_types_are_reported_by_path() {
        let result = check(
            &json!({ "only_main_content": "yes", "include_tags": "main", "timeout_ms": "5" }),
            false,
        );
        let mut found = codes(&result);
        found.sort();
        assert_eq!(
            found,
            vec![
                pair("include_tags", "invalid_type"),
                pair("only_main_content", "invalid_type"),
                pair("timeout_ms", "invalid_type"),
            ]
        );
        assert_eq!(
            codes(&check(&json!([1]), false)),
            vec![pair("$", "invalid_type")]
        );
    }

    #[test]
    fn unknown_keys_warn_when_lenient_and_refuse_when_strict() {
        let lenient = check(&json!({ "exclude_tags": ["nav"], "colour": 1 }), false).unwrap();
        assert_eq!(lenient.unknown, vec!["colour".to_owned()]);
        let strict = check(&json!({ "colour": 1 }), true);
        assert_eq!(codes(&strict), vec![pair("colour", "unknown_field")]);
    }

    #[test]
    fn the_canonical_form_round_trips() {
        let checked = check(
            &json!({ "formats": ["html"], "excludeTags": ["nav"], "timeoutMs": 12000 }),
            true,
        )
        .unwrap();
        let stamped = checked.options.to_value();
        assert_eq!(stamped["exclude_tags"], json!(["nav"]));
        assert_eq!(stamped["extractor"], json!("readability"));
        let again = CrawlScrapeOptions::from_stamped(&stamped).unwrap();
        assert_eq!(again, checked.options);
    }

    #[test]
    fn a_stamped_value_is_rechecked_by_the_same_rules() {
        let errors = CrawlScrapeOptions::from_stamped(&json!({ "actions": [] })).unwrap_err();
        assert_eq!(errors[0].code, "unsupported_option");
        let errors = CrawlScrapeOptions::from_stamped(&json!({ "timeout_ms": 1 })).unwrap_err();
        assert_eq!(errors[0].code, "out_of_range");
    }
}
