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
//!   in both modes, so a caller learns that the option did nothing. INNH-6 builds
//!   `sitemap` (`include`, `only`) and `scrape_options` behind the default-off
//!   flags `QUARRY_EDGE__CRAWL_SITEMAP` and `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS`;
//!   with a flag off the 400 stays exactly as it was.
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

use crate::crawl_policy::CrawlFeatures;
use crate::crawl_scrape::CrawlScrapeOptions;

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
    /// INNH-6: which of the two default-off options the handoff accepts.
    pub features: CrawlFeatures,
}

impl CrawlRequestSettings {
    /// Read the setting through a lookup, so tests need no process environment.
    /// An unknown value is logged (the value is not) and treated as `lenient`.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let value = lookup(ENV_REQUEST_VALIDATION).map(|value| value.trim().to_ascii_lowercase());
        match value.as_deref() {
            None | Some("") | Some("lenient") => Self {
                strict: false,
                features: CrawlFeatures::from_lookup(&lookup),
            },
            Some("strict") => Self {
                strict: true,
                features: CrawlFeatures::from_lookup(&lookup),
            },
            Some(_) => {
                tracing::warn!(
                    "QUARRY_EDGE__CRAWL_REQUEST_VALIDATION is not lenient or strict; using lenient"
                );
                Self {
                    strict: false,
                    features: CrawlFeatures::from_lookup(&lookup),
                }
            }
        }
    }

    /// The process setting, read once from the environment.
    pub fn current() -> Self {
        static SETTINGS: OnceLock<CrawlRequestSettings> = OnceLock::new();
        *SETTINGS.get_or_init(|| Self::from_lookup(|name| std::env::var(name).ok()))
    }
}

/// INNH-6a: what a crawl does with the site's sitemaps. The wire values are the
/// Firecrawl ones. Absent is `skip`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SitemapMode {
    /// Follow links only (today's behaviour).
    #[default]
    Skip,
    /// Seed the frontier with the sitemap's URLs as well as following links.
    Include,
    /// Crawl the sitemap's URLs and do not follow the seed page's links.
    Only,
}

impl SitemapMode {
    /// The wire name.
    pub fn wire(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Include => "include",
            Self::Only => "only",
        }
    }
}

/// The wire code of a shared-validator error, as a static string for [`RequestField`].
fn static_code(code: &str) -> &'static str {
    match code {
        "invalid_type" => "invalid_type",
        "unsupported_format" => "unsupported_format",
        "unsupported_option" => "unsupported_option",
        "unsupported_extractor" => "unsupported_extractor",
        "conflicts_with_only_main_content" => "conflicts_with_only_main_content",
        "conflicting_aliases" => "conflicting_aliases",
        "reserved_field" => "reserved_field",
        "unknown_field" => "unknown_field",
        "out_of_range" => "out_of_range",
        "too_many" => "too_many",
        "too_long" => "too_long",
        "empty" => "empty",
        _ => "invalid_option",
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
    /// `None` when absent. In `lenient` mode a `0` is `Some(0)`: the limits resolver takes the
    /// default for it and echoes the `0` as the requested value.
    pub max_pages: Option<u32>,
    /// `None` when absent or `0` (the default applies).
    pub max_depth: Option<u32>,
    pub ingest: Option<bool>,
    pub zdr: Option<bool>,
    /// The scope the caller asked for. `legacy` is set later, by the handoff.
    pub scope: CrawlScope,
    /// INNH-6a: `Skip` unless `sitemap` was `include` or `only` (and the flag is on).
    pub sitemap: SitemapMode,
    /// INNH-6b: the checked per-page options. `None` when absent (or the flag is off, which
    /// refuses the key instead).
    pub scrape_options: Option<CrawlScrapeOptions>,
    pub warnings: Vec<RequestWarning>,
}

/// W2-U64 (D2-029, D2-073): what a crawl does when the edge's answer does not prove the scope
/// was applied, or when the job carries no scope at all. The orchestrator
/// (`workflows/crawl_scope.go`, `verdict`, and `jobs/dispatcher.go`) decides this in production;
/// the rule lives here too so the edge side of a rolling deploy reads the same table, and the
/// two are pinned by the same cases.
///
/// - The scope is applied (`scope_applied: true`): [`ScopeSkew::Scoped`].
/// - Not applied, or no scope on the job, in `observe` (also the empty and any unknown mode):
///   [`ScopeSkew::LegacyUnscoped`]: the crawl goes on as one that never had a scope.
/// - Not applied, or no scope, in `enforce`: [`ScopeSkew::Refused`]. Enforce promises that
///   links the scope drops are not followed; an unscoped page cannot keep that promise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeSkew {
    Scoped,
    LegacyUnscoped,
    Refused,
}

/// Decide a page (or a job) under version skew. `scope_present` is whether the job carries a
/// scope object; `scope_applied` is the edge's report (`None` when the report is absent).
pub fn scope_skew(scope_mode: &str, scope_present: bool, scope_applied: Option<bool>) -> ScopeSkew {
    if scope_present && scope_applied == Some(true) {
        return ScopeSkew::Scoped;
    }
    if scope_mode == "enforce" {
        ScopeSkew::Refused
    } else {
        ScopeSkew::LegacyUnscoped
    }
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

/// A page or depth bound. `zero_is_refused` makes `0` a 400 (strict `max_pages`). Otherwise a
/// zero is kept as asked, and the limits resolver takes the default for it.
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
        // Kept as asked: the resolver echoes it as the requested value and takes the default.
        Some(0) => Some(0),
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
    // Depth zero is the default and is not echoed; a page zero is echoed (see `take_bound`).
    let max_depth =
        take_bound(lookup(object, "max_depth"), &mut invalid, false).filter(|depth| *depth != 0);
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

    let mut sitemap = SitemapMode::Skip;
    if let Some((path, value)) = lookup(object, "sitemap") {
        match (value.as_str(), settings.features.sitemap) {
            (Some("skip"), _) => {}
            (Some("include"), true) => sitemap = SitemapMode::Include,
            (Some("only"), true) => sitemap = SitemapMode::Only,
            (Some(_), true) => invalid.push(RequestField::new(
                path,
                "invalid_option",
                "must be skip, include or only",
            )),
            (None, true) => invalid.push(RequestField::new(
                path,
                "invalid_type",
                "must be a string: skip, include or only",
            )),
            (_, false) => invalid.push(RequestField::new(
                path,
                "unsupported_option",
                "only \"skip\" is accepted; sitemap seeding is not built yet (F-t2-041)",
            )),
        }
    }
    if let Some((path, _)) = lookup(object, "ignore_query_parameters") {
        invalid.push(RequestField::new(
            path,
            "unsupported_option",
            "is not supported yet (F-t2-041)",
        ));
    }
    let mut scrape_options = None;
    if let Some((path, value)) = lookup(object, "scrape_options") {
        if !settings.features.scrape_options {
            invalid.push(RequestField::new(
                path,
                "unsupported_option",
                "is not accepted on /v1/crawl until WS5 slice 4 (references only)",
            ));
        } else {
            match crate::crawl_scrape::check(value, settings.strict) {
                Ok(checked) => {
                    for key in checked.unknown {
                        warnings.push(RequestWarning {
                            code: "unknown_field",
                            field: format!("{path}.{key}"),
                        });
                    }
                    scrape_options = Some(checked.options);
                }
                Err(found) => {
                    for field in found {
                        let full = if field.path == "$" {
                            path.to_owned()
                        } else {
                            format!("{path}.{}", field.path)
                        };
                        invalid.push(RequestField::new(full, static_code(&field.code), field.message));
                    }
                }
            }
        }
    }

    // Errors from here on are pattern errors; the regex step runs only when there are none.
    let pattern_errors = invalid.len();
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
        sitemap,
        scrape_options,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lenient() -> CrawlRequestSettings {
        CrawlRequestSettings { strict: false, features: CrawlFeatures::default() }
    }

    fn strict() -> CrawlRequestSettings {
        CrawlRequestSettings { strict: true, features: CrawlFeatures::default() }
    }

    fn with_features(strict: bool, sitemap: bool, scrape_options: bool) -> CrawlRequestSettings {
        CrawlRequestSettings { strict, features: CrawlFeatures { sitemap, scrape_options } }
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
        assert_eq!(ok.max_pages, Some(0), "zero is kept as asked; the resolver takes the default");
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
    fn sitemap_include_and_only_are_accepted_with_the_flag_on() {
        for settings in [with_features(false, true, false), with_features(true, true, false)] {
            let include = parse(json!({ "url": "https://e.test/", "sitemap": "include" }), settings).unwrap();
            assert_eq!(include.sitemap, SitemapMode::Include);
            let only = parse(json!({ "url": "https://e.test/", "sitemap": "only" }), settings).unwrap();
            assert_eq!(only.sitemap, SitemapMode::Only);
            let skip = parse(json!({ "url": "https://e.test/", "sitemap": "skip" }), settings).unwrap();
            assert_eq!(skip.sitemap, SitemapMode::Skip);
            let absent = parse(json!({ "url": "https://e.test/" }), settings).unwrap();
            assert_eq!(absent.sitemap, SitemapMode::Skip);
        }
    }

    #[test]
    fn sitemap_values_are_checked_with_the_flag_on() {
        let settings = with_features(false, true, false);
        let unknown = parse(json!({ "url": "https://e.test/", "sitemap": "both" }), settings);
        assert_eq!(codes(&unknown), vec![("sitemap".to_owned(), "invalid_option")]);
        let wrong_type = parse(json!({ "url": "https://e.test/", "sitemap": true }), settings);
        assert_eq!(codes(&wrong_type), vec![("sitemap".to_owned(), "invalid_type")]);
    }

    #[test]
    fn sitemap_stays_a_400_with_the_flag_off() {
        for value in [json!("include"), json!("only"), json!("both"), json!(true)] {
            let body = json!({ "url": "https://e.test/", "sitemap": value });
            for settings in [lenient(), strict(), with_features(false, false, true)] {
                assert_eq!(
                    codes(&parse(body.clone(), settings)),
                    vec![("sitemap".to_owned(), "unsupported_option")]
                );
            }
        }
    }

    #[test]
    fn scrape_options_are_accepted_and_stamped_with_the_flag_on() {
        let settings = with_features(false, false, true);
        let ok = parse(
            json!({
                "url": "https://e.test/",
                "scrapeOptions": { "formats": ["markdown"], "excludeTags": ["nav", "footer"], "colour": 1 },
            }),
            settings,
        )
        .unwrap();
        let options = ok.scrape_options.expect("scrape options kept");
        assert_eq!(options.exclude_tags, vec!["nav", "footer"]);
        assert_eq!(
            ok.warnings,
            vec![RequestWarning { code: "unknown_field", field: "scrapeOptions.colour".to_owned() }]
        );
        let absent = parse(json!({ "url": "https://e.test/" }), settings).unwrap();
        assert!(absent.scrape_options.is_none());
        let null = parse(json!({ "url": "https://e.test/", "scrape_options": null }), settings).unwrap();
        assert!(null.scrape_options.is_none());
    }

    #[test]
    fn scrape_options_refuse_what_cannot_apply_per_page_with_prefixed_paths() {
        let settings = with_features(false, false, true);
        let body = json!({
            "url": "https://e.test/",
            "scrape_options": {
                "actions": [{ "type": "wait", "ms": 10 }],
                "screenshot": true,
                "formats": ["markdown", "pdf"],
                "include_tags": ["ok", "ok", " "],
                "timeout_ms": 10,
            },
        });
        let mut found = codes(&parse(body, settings));
        found.sort();
        assert_eq!(
            found,
            vec![
                ("scrape_options.actions".to_owned(), "unsupported_option"),
                ("scrape_options.formats[1]".to_owned(), "unsupported_option"),
                ("scrape_options.include_tags[2]".to_owned(), "empty"),
                ("scrape_options.screenshot".to_owned(), "unsupported_option"),
                ("scrape_options.timeout_ms".to_owned(), "out_of_range"),
            ]
        );
        let not_object = parse(json!({ "url": "https://e.test/", "scrape_options": [1] }), settings);
        assert_eq!(codes(&not_object), vec![("scrape_options".to_owned(), "invalid_type")]);
        let conflict = parse(
            json!({ "url": "https://e.test/", "scrape_options": { "extractor": "raw", "only_main_content": true } }),
            settings,
        );
        assert_eq!(
            codes(&conflict),
            vec![("scrape_options.extractor".to_owned(), "conflicts_with_only_main_content")]
        );
    }

    #[test]
    fn scrape_options_unknown_keys_are_refused_in_strict_mode() {
        let body = json!({ "url": "https://e.test/", "scrape_options": { "colour": 1 } });
        assert_eq!(
            codes(&parse(body, with_features(true, false, true))),
            vec![("scrape_options.colour".to_owned(), "unknown_field")]
        );
    }

    #[test]
    fn scrape_options_stay_a_400_with_the_flag_off() {
        for value in [json!({ "formats": ["markdown"] }), json!({}), json!("x")] {
            let body = json!({ "url": "https://e.test/", "scrape_options": value });
            for settings in [lenient(), strict(), with_features(false, true, false)] {
                assert_eq!(
                    codes(&parse(body.clone(), settings)),
                    vec![("scrape_options".to_owned(), "unsupported_option")]
                );
            }
        }
    }

    #[test]
    fn the_flags_read_on_one_and_true_and_default_off() {
        assert_eq!(CrawlFeatures::from_lookup(|_| None), CrawlFeatures::default());
        let lookup = |on: &'static str| move |name: &str| {
            (name == crate::crawl_policy::ENV_CRAWL_SITEMAP).then(|| on.to_owned())
        };
        for on in ["on", "1", "true", " TRUE ", "On"] {
            let features = CrawlFeatures::from_lookup(lookup(on));
            assert!(features.sitemap && !features.scrape_options, "{on}");
        }
        for off in ["", "off", "0", "yes", "enable"] {
            assert!(!CrawlFeatures::from_lookup(lookup(off)).sitemap, "{off}");
        }
        let both = CrawlRequestSettings::from_lookup(|name| {
            (name != ENV_REQUEST_VALIDATION).then(|| "on".to_owned())
        });
        assert!(both.features.sitemap && both.features.scrape_options && !both.strict);
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

    #[test]
    fn scope_skew_tolerates_a_missing_scope_applied_in_observe_only() {
        // Same table as TestScopeVerdict / TestCrawlInputFromJobAbsentScope in the orchestrator.
        let cases = [
            ("observe", true, Some(true), ScopeSkew::Scoped),
            ("enforce", true, Some(true), ScopeSkew::Scoped),
            ("observe", true, Some(false), ScopeSkew::LegacyUnscoped),
            ("observe", true, None, ScopeSkew::LegacyUnscoped),
            ("", true, None, ScopeSkew::LegacyUnscoped),
            ("strict-ish", true, Some(false), ScopeSkew::LegacyUnscoped),
            ("enforce", true, Some(false), ScopeSkew::Refused),
            ("enforce", true, None, ScopeSkew::Refused),
            ("Enforce", true, None, ScopeSkew::LegacyUnscoped), // the wire value is lowercase; anything else is observe
        ];
        for (mode, present, applied, want) in cases {
            assert_eq!(scope_skew(mode, present, applied), want, "{mode:?} {present} {applied:?}");
        }
    }

    #[test]
    fn scope_skew_for_an_absent_scope_runs_unscoped_in_observe_and_is_refused_in_enforce() {
        // D2-073: no scope object. A report cannot prove a scope that was never sent.
        assert_eq!(scope_skew("observe", false, None), ScopeSkew::LegacyUnscoped);
        assert_eq!(scope_skew("", false, None), ScopeSkew::LegacyUnscoped);
        assert_eq!(scope_skew("observe", false, Some(true)), ScopeSkew::LegacyUnscoped);
        assert_eq!(scope_skew("enforce", false, None), ScopeSkew::Refused);
        assert_eq!(scope_skew("enforce", false, Some(true)), ScopeSkew::Refused);
    }
}
