//! WS14.S6: the `/v1/crawl` request contract (`CrawlRequest` v2).
//!
//! The handoff used to deserialize its body into a struct without
//! `deny_unknown_fields`, so a misspelt scope key vanished and a bad pattern
//! reached the orchestrator. Now the body is read as a JSON value and checked
//! field by field:
//!
//! - A key in [`RESERVED_KEYS`] is a 400 in both modes: those keys would change
//!   what a request may do (proxies, TLS verification, headers, stealth).
//! - A key that is not a known option is a warning (`lenient`, the default) or a
//!   400 (`strict`, `QUARRY_EDGE__CRAWL_REQUEST_VALIDATION=strict`).
//! - Each option takes its canonical name and the spellings in [`FIELDS`]. Sending
//!   two spellings of one option is a 400.
//! - Options that are known but not built yet (`sitemap` other than `skip`,
//!   `ignore_query_parameters`, `scrape_options`) are a 400 `unsupported_option`
//!   in both modes, so a caller learns that the option did nothing.
//! - `ignore_robots_txt: true` is a 403 `policy_denied` (WS14.S7 owns the
//!   entitlement that would allow it; until then it is never allowed).
//! - Patterns obey the D3 caps (100 patterns, 512 characters each, 8192 in total)
//!   and must compile (`re:` prefixes are regular expressions).
//!
//! `max_pages: 0` is the default in `lenient` and a 400 `invalid_option` in
//! `strict` (the W1 WS6.A semantics until strict is set). `max_depth: 0` takes
//! the default in both modes, as the existing contract documents.
//!
//! Field errors are `{path, code, message}`: the path is the spelling the caller
//! sent, so a caller can correct itself. Messages never echo a caller's value.

use std::sync::OnceLock;

use quarry_core::crawl_scope::{
    CrawlContext, CrawlScope, MAX_EXTENSIONS, MAX_PATTERNS, MAX_PATTERN_CHARS,
    MAX_PATTERN_TOTAL_CHARS,
};
use serde_json::{Map, Value};

/// The environment name. Read once through [`CrawlRequestSettings::current`].
pub const ENV_REQUEST_VALIDATION: &str = "QUARRY_EDGE__CRAWL_REQUEST_VALIDATION";

/// Each option, its canonical name first, then every spelling it takes.
pub const FIELDS: &[(&str, &[&str])] = &[
    ("url", &["url"]),
    ("max_pages", &["max_pages", "maxPages", "limit"]),
    ("max_depth", &["max_depth", "maxDepth", "max_discovery_depth"]),
    ("ingest", &["ingest"]),
    ("zdr", &["zdr"]),
    ("allow_subdomains", &["allow_subdomains", "allowSubdomains"]),
    (
        "allow_external_links",
        &["allow_external_links", "allowExternalLinks"],
    ),
    ("crawl_entire_domain", &["crawl_entire_domain", "crawlEntireDomain"]),
    ("include_paths", &["include_paths", "includePaths"]),
    ("exclude_paths", &["exclude_paths", "excludePaths"]),
    ("deny_extensions", &["deny_extensions", "denyExtensions"]),
    ("sitemap", &["sitemap"]),
    (
        "ignore_query_parameters",
        &["ignore_query_parameters", "ignoreQueryParameters"],
    ),
    ("scrape_options", &["scrape_options", "scrapeOptions"]),
    (
        "ignore_robots_txt",
        &["ignore_robots_txt", "ignoreRobotsTxt", "ignoreRobots"],
    ),
];

/// Keys that would change what a request may do. Refused in both modes.
pub const RESERVED_KEYS: &[&str] = &[
    "headers",
    "proxy",
    "proxy_url",
    "proxyUrl",
    "skip_tls_verification",
    "skipTlsVerification",
    "stealth",
    "mobile",
    "webhook",
    "delay",
    "max_concurrency",
    "maxConcurrency",
];

/// What the operator configured, read once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CrawlRequestSettings {
    /// `strict` refuses unknown keys and `max_pages: 0`. `lenient` (default) warns.
    pub strict: bool,
}

impl CrawlRequestSettings {
    /// Read the setting through a lookup, so tests need no process environment.
    /// An unknown value is logged (the value is not) and treated as `lenient`.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let value = lookup(ENV_REQUEST_VALIDATION).map(|value| value.trim().to_ascii_lowercase());
        match value.as_deref() {
            None | Some("") | Some("lenient") => Self { strict: false },
            Some("strict") => Self { strict: true },
            Some(_) => {
                tracing::warn!(
                    "QUARRY_EDGE__CRAWL_REQUEST_VALIDATION is not lenient or strict; using lenient"
                );
                Self { strict: false }
            }
        }
    }

    /// The process setting, read once from the environment.
    pub fn current() -> Self {
        static SETTINGS: OnceLock<CrawlRequestSettings> = OnceLock::new();
        *SETTINGS.get_or_init(|| Self::from_lookup(|name| std::env::var(name).ok()))
    }
}

/// One rejected field: `path` is the spelling the caller sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestField {
    pub path: String,
    pub code: &'static str,
    pub message: String,
}

impl RequestField {
    fn new(path: impl Into<String>, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            code,
            message: message.into(),
        }
    }
}

/// Why a request is refused before anything is created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// 400: fields that cannot be accepted.
    Invalid(Vec<RequestField>),
    /// 403: an option this request may not use (`policy_denied`).
    PolicyDenied(Vec<RequestField>),
}

impl Refusal {
    /// The field errors of either kind.
    pub fn fields(&self) -> &[RequestField] {
        match self {
            Self::Invalid(fields) | Self::PolicyDenied(fields) => fields,
        }
    }
}

/// `fields[] {path, code, message}` as the envelope carries it.
pub fn fields_json(fields: &[RequestField]) -> Value {
    Value::Array(
        fields
            .iter()
            .map(|field| {
                serde_json::json!({
                    "path": field.path,
                    "code": field.code,
                    "message": field.message,
                })
            })
            .collect(),
    )
}

/// A warning on an accepted request (lenient mode): a key that was ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestWarning {
    pub code: &'static str,
    pub field: String,
}

/// A request body that passed every check.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCrawl {
    pub url: String,
    /// `None` when absent, or `0` in `lenient` mode (the default applies).
    pub max_pages: Option<u32>,
    /// `None` when absent or `0` (the default applies).
    pub max_depth: Option<u32>,
    pub ingest: Option<bool>,
    pub zdr: Option<bool>,
    /// The scope the caller asked for. `legacy` is set later, by the handoff.
    pub scope: CrawlScope,
    pub warnings: Vec<RequestWarning>,
}

/// Look up an option by any of its spellings. A null value counts as absent.
fn lookup<'a>(object: &'a Map<String, Value>, canonical: &str) -> Option<(&'static str, &'a Value)> {
    let spellings = FIELDS.iter().find(|(name, _)| *name == canonical)?.1;
    spellings.iter().find_map(|spelling| {
        object
            .get(*spelling)
            .filter(|value| !value.is_null())
            .map(|value| (*spelling, value))
    })
}

fn canonical_for(key: &str) -> Option<&'static str> {
    FIELDS
        .iter()
        .find(|(_, spellings)| spellings.contains(&key))
        .map(|(canonical, _)| *canonical)
}

fn take_flag(found: Option<(&str, &Value)>, invalid: &mut Vec<RequestField>) -> Option<bool> {
    match found {
        None => None,
        Some((_, Value::Bool(flag))) => Some(*flag),
        Some((path, _)) => {
            invalid.push(RequestField::new(path, "invalid_type", "must be true or false"));
            None
        }
    }
}

/// A page or depth bound. `zero_is_refused` makes `0` a 400 (strict `max_pages`).
fn take_bound(
    found: Option<(&str, &Value)>,
    invalid: &mut Vec<RequestField>,
    zero_is_refused: bool,
) -> Option<u32> {
    let (path, value) = found?;
    match value.as_u64() {
        Some(0) if zero_is_refused => {
            invalid.push(RequestField::new(
                path,
                "invalid_option",
                "must be at least 1; omit the field for the default",
            ));
            None
        }
        Some(0) => None,
        Some(number) => match u32::try_from(number) {
            Ok(number) => Some(number),
            Err(_) => {
                invalid.push(RequestField::new(path, "invalid_option", "is too large"));
                None
            }
        },
        None => {
            invalid.push(RequestField::new(
                path,
                "invalid_type",
                "must be a whole number",
            ));
            None
        }
    }
}

/// The entries of a pattern list, each with its indexed path.
fn string_list(found: Option<(&str, &Value)>, invalid: &mut Vec<RequestField>) -> Vec<(String, String)> {
    match found {
        None => Vec::new(),
        Some((path, Value::Array(items))) => items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                Value::String(text) => Some((format!("{path}[{index}]"), text.clone())),
                _ => {
                    invalid.push(RequestField::new(
                        format!("{path}[{index}]"),
                        "invalid_type",
                        "must be a string",
                    ));
                    None
                }
            })
            .collect(),
        Some((path, _)) => {
            invalid.push(RequestField::new(
                path,
                "invalid_type",
                "must be an array of strings",
            ));
            Vec::new()
        }
    }
}

/// Check the patterns of both lists against the D3 caps. `global` counts across
/// the include list and then the exclude list, so the 101st pattern is refused
/// whichever list it is in.
fn check_patterns(
    entries: &[(String, String)],
    global: &mut usize,
    total_chars: &mut usize,
    invalid: &mut Vec<RequestField>,
) -> Vec<String> {
    let mut kept = Vec::with_capacity(entries.len());
    for (path, pattern) in entries {
        let index = *global;
        *global += 1;
        *total_chars += pattern.chars().count();
        if index >= MAX_PATTERNS {
            invalid.push(RequestField::new(
                path.as_str(),
                "invalid_pattern",
                format!(
                    "include_paths and exclude_paths together hold at most {MAX_PATTERNS} patterns"
                ),
            ));
        } else if pattern.trim().is_empty() {
            invalid.push(RequestField::new(
                path.as_str(),
                "invalid_pattern",
                "must not be empty",
            ));
        } else if pattern.chars().count() > MAX_PATTERN_CHARS {
            invalid.push(RequestField::new(
                path.as_str(),
                "invalid_pattern",
                format!("at most {MAX_PATTERN_CHARS} characters"),
            ));
        } else {
            kept.push(pattern.clone());
        }
    }
    kept
}

/// Check a request body. Every problem of the request is reported together.
pub fn parse_body(body: &[u8], settings: CrawlRequestSettings) -> Result<ParsedCrawl, Refusal> {
    let value: Value = serde_json::from_slice(body).map_err(|_| {
        Refusal::Invalid(vec![RequestField::new(
            "$",
            "invalid_json",
            "the request body is not valid JSON",
        )])
    })?;
    let Some(object) = value.as_object() else {
        return Err(Refusal::Invalid(vec![RequestField::new(
            "$",
            "invalid_type",
            "the request body must be a JSON object",
        )]));
    };
    let mut keys: Vec<&String> = object.keys().collect();
    keys.sort();

    let reserved: Vec<RequestField> = keys
        .iter()
        .filter(|key| RESERVED_KEYS.contains(&key.as_str()))
        .map(|key| {
            RequestField::new(
                key.as_str(),
                "reserved_field",
                format!("`{key}` is not accepted by /v1/crawl"),
            )
        })
        .collect();
    if !reserved.is_empty() {
        return Err(Refusal::Invalid(reserved));
    }

    let mut invalid: Vec<RequestField> = Vec::new();
    let mut policy: Vec<RequestField> = Vec::new();
    let mut warnings: Vec<RequestWarning> = Vec::new();

    for (canonical, spellings) in FIELDS {
        let present: Vec<&str> = spellings
            .iter()
            .copied()
            .filter(|spelling| object.contains_key(*spelling))
            .collect();
        if present.len() > 1 {
            invalid.push(RequestField::new(
                *canonical,
                "invalid_option",
                format!("send one spelling of `{canonical}`, not {}", present.join(" and ")),
            ));
        }
    }

    for key in &keys {
        if canonical_for(key.as_str()).is_none() {
            if settings.strict {
                invalid.push(RequestField::new(
                    key.as_str(),
                    "unknown_field",
                    format!("`{key}` is not a /v1/crawl field"),
                ));
            } else {
                warnings.push(RequestWarning {
                    code: "unknown_field",
                    field: key.to_string(),
                });
            }
        }
    }

    let url = match lookup(object, "url") {
        None => {
            invalid.push(RequestField::new("url", "required", "a seed URL is required"));
            String::new()
        }
        Some((_, Value::String(text))) if !text.trim().is_empty() => text.trim().to_owned(),
        Some((path, Value::String(_))) => {
            invalid.push(RequestField::new(path, "required", "a seed URL is required"));
            String::new()
        }
        Some((path, _)) => {
            invalid.push(RequestField::new(path, "invalid_type", "must be a string"));
            String::new()
        }
    };

    let max_pages = take_bound(lookup(object, "max_pages"), &mut invalid, settings.strict);
    let max_depth = take_bound(lookup(object, "max_depth"), &mut invalid, false);
    let ingest = take_flag(lookup(object, "ingest"), &mut invalid);
    let zdr = take_flag(lookup(object, "zdr"), &mut invalid);
    let allow_subdomains = take_flag(lookup(object, "allow_subdomains"), &mut invalid).unwrap_or(false);
    let allow_external_links =
        take_flag(lookup(object, "allow_external_links"), &mut invalid).unwrap_or(false);
    let crawl_entire_domain =
        take_flag(lookup(object, "crawl_entire_domain"), &mut invalid).unwrap_or(true);

    if let Some((path, value)) = lookup(object, "ignore_robots_txt") {
        match value {
            Value::Bool(true) => policy.push(RequestField::new(
                path,
                "policy_denied",
                "ignoring robots.txt is not available to this request",
            )),
            Value::Bool(false) => {}
            _ => invalid.push(RequestField::new(path, "invalid_type", "must be true or false")),
        }
    }

    if let Some((path, value)) = lookup(object, "sitemap") {
        if value.as_str() != Some("skip") {
            invalid.push(RequestField::new(
                path,
                "unsupported_option",
                "only \"skip\" is accepted; sitemap seeding is not built yet (F-t2-041)",
            ));
        }
    }
    if let Some((path, _)) = lookup(object, "ignore_query_parameters") {
        invalid.push(RequestField::new(
            path,
            "unsupported_option",
            "is not supported yet (F-t2-041)",
        ));
    }
    if let Some((path, _)) = lookup(object, "scrape_options") {
        invalid.push(RequestField::new(
            path,
            "unsupported_option",
            "is not accepted on /v1/crawl until WS5 slice 4 (references only)",
        ));
    }

    let include = string_list(lookup(object, "include_paths"), &mut invalid);
    let exclude = string_list(lookup(object, "exclude_paths"), &mut invalid);
    let mut global = 0usize;
    let mut total_chars = 0usize;
    let include_paths = check_patterns(&include, &mut global, &mut total_chars, &mut invalid);
    let exclude_paths = check_patterns(&exclude, &mut global, &mut total_chars, &mut invalid);
    if total_chars > MAX_PATTERN_TOTAL_CHARS {
        invalid.push(RequestField::new(
            "patterns",
            "invalid_pattern",
            format!("include_paths and exclude_paths hold at most {MAX_PATTERN_TOTAL_CHARS} characters in total"),
        ));
    }

    let deny_extensions = match lookup(object, "deny_extensions") {
        None => None,
        Some((path, Value::Array(items))) => {
            if items.len() > MAX_EXTENSIONS {
                invalid.push(RequestField::new(
                    path,
                    "invalid_option",
                    format!("at most {MAX_EXTENSIONS} extensions"),
                ));
            }
            let mut list = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let cleaned = item
                    .as_str()
                    .map(|text| text.trim().trim_start_matches('.'))
                    .filter(|text| {
                        !text.is_empty()
                            && text.len() <= 16
                            && text.chars().all(|c| c.is_ascii_alphanumeric())
                    });
                match cleaned {
                    Some(text) => list.push(text.to_owned()),
                    None => invalid.push(RequestField::new(
                        format!("{path}[{index}]"),
                        "invalid_option",
                        "must be an alphanumeric extension of at most 16 characters",
                    )),
                }
            }
            Some(list)
        }
        Some((path, _)) => {
            invalid.push(RequestField::new(
                path,
                "invalid_type",
                "must be an array of strings",
            ));
            None
        }
    };

    // The regular expressions are compiled here, so a bad pattern is a 400 at the
    // handoff and never reaches the orchestrator. Runs only when the patterns above
    // are clean, so one request does not produce the same error twice.
    let pattern_errors = invalid.len();
    if invalid.len() == pattern_errors
        && (!include_paths.is_empty() || !exclude_paths.is_empty())
    {
        let scope = CrawlScope {
            include_paths: include_paths.clone(),
            exclude_paths: exclude_paths.clone(),
            ..CrawlScope::default()
        };
        let ctx = CrawlContext {
            scope,
            ..CrawlContext::default()
        };
        if let Err(errors) = quarry_runtime::crawl_filter::CompiledScope::compile(&ctx) {
            for error in errors {
                let path = error
                    .field
                    .strip_prefix("crawl.scope.")
                    .unwrap_or(&error.field)
                    .to_owned();
                invalid.push(RequestField::new(path, "invalid_pattern", error.message));
            }
        }
    }

    if !invalid.is_empty() {
        return Err(Refusal::Invalid(invalid));
    }
    if !policy.is_empty() {
        return Err(Refusal::PolicyDenied(policy));
    }

    Ok(ParsedCrawl {
        url,
        max_pages,
        max_depth,
        ingest,
        zdr,
        scope: CrawlScope {
            allow_subdomains,
            allow_external_links,
            crawl_entire_domain,
            include_paths,
            exclude_paths,
            deny_extensions,
            legacy: false,
        },
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lenient() -> CrawlRequestSettings {
        CrawlRequestSettings { strict: false }
    }

    fn strict() -> CrawlRequestSettings {
        CrawlRequestSettings { strict: true }
    }

    fn parse(body: Value, settings: CrawlRequestSettings) -> Result<ParsedCrawl, Refusal> {
        parse_body(&serde_json::to_vec(&body).unwrap(), settings)
    }

    fn codes(result: &Result<ParsedCrawl, Refusal>) -> Vec<(String, &'static str)> {
        match result {
            Ok(_) => Vec::new(),
            Err(refusal) => refusal
                .fields()
                .iter()
                .map(|field| (field.path.clone(), field.code))
                .collect(),
        }
    }

    fn patterns(count: usize, each: &str) -> Vec<String> {
        (0..count).map(|i| format!("/{each}{i}")).collect()
    }

    #[test]
    fn zero_pages_is_default_in_lenient_and_400_in_strict() {
        let body = json!({ "url": "https://example.com/", "max_pages": 0 });
        let ok = parse(body.clone(), lenient()).expect("lenient accepts zero");
        assert_eq!(ok.max_pages, None, "zero takes the default");
        let refused = parse(body, strict());
        assert_eq!(
            codes(&refused),
            vec![("max_pages".to_owned(), "invalid_option")]
        );
    }

    #[test]
    fn page_and_depth_aliases_map_to_the_canonical_bounds() {
        let limit = parse(json!({ "url": "https://e.test/", "limit": 7 }), lenient()).unwrap();
        assert_eq!(limit.max_pages, Some(7));
        let camel = parse(json!({ "url": "https://e.test/", "maxPages": 9 }), lenient()).unwrap();
        assert_eq!(camel.max_pages, Some(9));
        let depth = parse(
            json!({ "url": "https://e.test/", "max_discovery_depth": 2 }),
            lenient(),
        )
        .unwrap();
        assert_eq!(depth.max_depth, Some(2));
        let zero_depth = parse(json!({ "url": "https://e.test/", "max_depth": 0 }), strict()).unwrap();
        assert_eq!(zero_depth.max_depth, None, "depth zero takes the default in both modes");
    }

    #[test]
    fn two_spellings_of_one_option_are_refused() {
        let result = parse(
            json!({ "url": "https://e.test/", "limit": 3, "max_pages": 4 }),
            lenient(),
        );
        assert_eq!(codes(&result), vec![("max_pages".to_owned(), "invalid_option")]);
    }

    #[test]
    fn page_limit_boundaries_accept_u32_range_and_refuse_beyond() {
        let max = parse(json!({ "url": "https://e.test/", "max_pages": u32::MAX }), lenient());
        assert_eq!(max.unwrap().max_pages, Some(u32::MAX));
        let over = parse(
            json!({ "url": "https://e.test/", "max_pages": u64::from(u32::MAX) + 1 }),
            lenient(),
        );
        assert_eq!(codes(&over), vec![("max_pages".to_owned(), "invalid_option")]);
        let negative = parse(json!({ "url": "https://e.test/", "max_pages": -1 }), lenient());
        assert_eq!(codes(&negative), vec![("max_pages".to_owned(), "invalid_type")]);
    }

    #[test]
    fn unknown_keys_warn_in_lenient_and_refuse_in_strict() {
        let body = json!({ "url": "https://e.test/", "maxPagez": 3 });
        let ok = parse(body.clone(), lenient()).unwrap();
        assert_eq!(
            ok.warnings,
            vec![RequestWarning {
                code: "unknown_field",
                field: "maxPagez".to_owned()
            }]
        );
        assert_eq!(
            codes(&parse(body, strict())),
            vec![("maxPagez".to_owned(), "unknown_field")]
        );
    }

    #[test]
    fn reserved_keys_are_refused_in_both_modes() {
        for key in RESERVED_KEYS {
            let mut body = json!({ "url": "https://e.test/" });
            body[*key] = json!(true);
            for settings in [lenient(), strict()] {
                let result = parse(body.clone(), settings);
                assert_eq!(codes(&result), vec![((*key).to_owned(), "reserved_field")], "{key}");
            }
        }
    }

    #[test]
    fn known_but_unbuilt_options_are_unsupported_in_both_modes() {
        let cases = [
            ("sitemap", json!("include")),
            ("ignore_query_parameters", json!(true)),
            ("scrape_options", json!({ "formats": ["markdown"] })),
        ];
        for (key, value) in cases {
            let mut body = json!({ "url": "https://e.test/" });
            body[key] = value;
            for settings in [lenient(), strict()] {
                assert_eq!(
                    codes(&parse(body.clone(), settings)),
                    vec![(key.to_owned(), "unsupported_option")],
                    "{key}"
                );
            }
        }
    }

    #[test]
    fn sitemap_skip_is_accepted() {
        let ok = parse(json!({ "url": "https://e.test/", "sitemap": "skip" }), strict());
        assert!(ok.is_ok());
    }

    #[test]
    fn ignore_robots_txt_true_is_policy_denied_and_false_is_accepted() {
        for key in ["ignore_robots_txt", "ignoreRobotsTxt", "ignoreRobots"] {
            let mut body = json!({ "url": "https://e.test/" });
            body[key] = json!(true);
            match parse(body.clone(), lenient()) {
                Err(Refusal::PolicyDenied(fields)) => {
                    assert_eq!(fields[0].code, "policy_denied");
                    assert_eq!(fields[0].path, key);
                }
                other => panic!("expected policy_denied for {key}, got {other:?}"),
            }
            body[key] = json!(false);
            assert!(parse(body, lenient()).is_ok(), "{key}: false is accepted");
        }
    }

    #[test]
    fn required_url_and_json_shape_are_checked() {
        assert_eq!(codes(&parse(json!({}), lenient())), vec![("url".to_owned(), "required")]);
        assert_eq!(
            codes(&parse(json!({ "url": "   " }), lenient())),
            vec![("url".to_owned(), "required")]
        );
        assert_eq!(
            codes(&parse(json!({ "url": 5 }), lenient())),
            vec![("url".to_owned(), "invalid_type")]
        );
        assert_eq!(
            codes(&parse_body(b"[1,2]", lenient())),
            vec![("$".to_owned(), "invalid_type")]
        );
        assert_eq!(
            codes(&parse_body(b"{not json", lenient())),
            vec![("$".to_owned(), "invalid_json")]
        );
    }

    #[test]
    fn pattern_count_boundary_is_one_hundred() {
        let at_cap = patterns(100, "a");
        let ok = parse(json!({ "url": "https://e.test/", "include_paths": at_cap }), lenient());
        assert_eq!(ok.unwrap().scope.include_paths.len(), 100);

        let over = parse(
            json!({ "url": "https://e.test/", "include_paths": patterns(101, "a") }),
            lenient(),
        );
        assert_eq!(
            codes(&over),
            vec![("include_paths[100]".to_owned(), "invalid_pattern")]
        );
    }

    #[test]
    fn pattern_count_counts_both_lists_together() {
        let result = parse(
            json!({
                "url": "https://e.test/",
                "include_paths": patterns(60, "i"),
                "exclude_paths": patterns(41, "x"),
            }),
            lenient(),
        );
        assert_eq!(
            codes(&result),
            vec![("exclude_paths[40]".to_owned(), "invalid_pattern")]
        );
    }

    #[test]
    fn pattern_length_boundary_is_512_characters() {
        let fits = "a".repeat(512);
        let too_long = "a".repeat(513);
        assert!(parse(json!({ "url": "https://e.test/", "include_paths": [fits] }), lenient()).is_ok());
        assert_eq!(
            codes(&parse(json!({ "url": "https://e.test/", "include_paths": [too_long] }), lenient())),
            vec![("include_paths[0]".to_owned(), "invalid_pattern")]
        );
    }

    #[test]
    fn pattern_total_boundary_is_8192_characters() {
        let mut fits: Vec<String> = (0..16).map(|_| "a".repeat(512)).collect();
        assert!(parse(json!({ "url": "https://e.test/", "include_paths": fits.clone() }), lenient()).is_ok());
        fits.push("b".to_owned());
        assert_eq!(
            codes(&parse(json!({ "url": "https://e.test/", "include_paths": fits }), lenient())),
            vec![("patterns".to_owned(), "invalid_pattern")]
        );
    }

    #[test]
    fn empty_pattern_is_refused_and_regex_must_compile() {
        assert_eq!(
            codes(&parse(json!({ "url": "https://e.test/", "exclude_paths": ["  "] }), lenient())),
            vec![("exclude_paths[0]".to_owned(), "invalid_pattern")]
        );
        let bad = parse(json!({ "url": "https://e.test/", "include_paths": ["re:("] }), lenient());
        let fields = codes(&bad);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].1, "invalid_pattern");
        assert!(fields[0].0.contains("include_paths"), "{fields:?}");
        let linear = parse(
            json!({ "url": "https://e.test/", "include_paths": ["re:(a+)+$"] }),
            lenient(),
        );
        assert!(linear.is_ok(), "the regex engine is linear time; this compiles");
    }

    #[test]
    fn glob_and_regex_patterns_keep_their_order() {
        let ok = parse(
            json!({
                "url": "https://e.test/",
                "include_paths": ["/docs/*", "re:^/guide"],
                "exclude_paths": ["re:^/blog/"],
            }),
            lenient(),
        )
        .unwrap();
        assert_eq!(ok.scope.include_paths, vec!["/docs/*", "re:^/guide"]);
        assert_eq!(ok.scope.exclude_paths, vec!["re:^/blog/"]);
    }

    #[test]
    fn deny_extensions_are_trimmed_and_checked() {
        let ok = parse(
            json!({ "url": "https://e.test/", "deny_extensions": [".PDF", " zip "] }),
            lenient(),
        )
        .unwrap();
        assert_eq!(
            ok.scope.deny_extensions,
            Some(vec!["PDF".to_owned(), "zip".to_owned()])
        );
        assert_eq!(
            codes(&parse(
                json!({ "url": "https://e.test/", "deny_extensions": ["a-b"] }),
                lenient()
            )),
            vec![("deny_extensions[0]".to_owned(), "invalid_option")]
        );
    }

    #[test]
    fn scope_switches_and_flags_are_read_with_their_defaults() {
        let parsed = parse(json!({ "url": "https://e.test/" }), lenient()).unwrap();
        assert_eq!(parsed.scope, CrawlScope::default());
        let set = parse(
            json!({
                "url": "https://e.test/",
                "allowSubdomains": true,
                "allow_external_links": true,
                "crawlEntireDomain": false,
                "ingest": true,
                "zdr": false,
            }),
            lenient(),
        )
        .unwrap();
        assert!(set.scope.allow_subdomains && set.scope.allow_external_links);
        assert!(!set.scope.crawl_entire_domain);
        assert_eq!(set.ingest, Some(true));
        assert_eq!(set.zdr, Some(false));
    }

    #[test]
    fn invalid_field_types_are_reported_by_their_spelling() {
        let result = parse(
            json!({ "url": "https://e.test/", "allowSubdomains": "yes", "include_paths": "/a" }),
            lenient(),
        );
        assert_eq!(
            codes(&result),
            vec![
                ("allowSubdomains".to_owned(), "invalid_type"),
                ("include_paths".to_owned(), "invalid_type"),
            ]
        );
    }

    #[test]
    fn settings_default_to_lenient_and_unknown_values_are_lenient() {
        assert!(!CrawlRequestSettings::from_lookup(|_| None).strict);
        assert!(!CrawlRequestSettings::from_lookup(|_| Some("lenient".into())).strict);
        assert!(CrawlRequestSettings::from_lookup(|_| Some(" STRICT ".into())).strict);
        assert!(!CrawlRequestSettings::from_lookup(|_| Some("maybe".into())).strict);
    }
}
