//! Q4-E4.1-6a: the saved job definition, a pure module.
//!
//! A saved definition is a Source with a typed, versioned options object (plan section 3.4). It
//! lives at `Source.config.definition`:
//!
//! ```json
//! { "version": 1, "job_kind": "scrape" | "crawl", "options": { ... } }
//! ```
//!
//! [`check`] runs the options through the same validators the live routes use (the scrape
//! option reader and limits for a scrape, the `/v1/crawl` body parser in strict mode for a
//! crawl) and returns the canonical definition, which holds only what those validators
//! accepted. A Source has one URL, so `url` is never part of the options, and `batch` is not a
//! saved kind.
//!
//! A saved job can never carry a robots override, a webhook target or a ZDR declaration: those
//! three change what a run may do or keep, so a definition that names one is refused as
//! `not_saveable` (the lesson of the `fast` preset). A definition holds options only, never
//! page content, so it needs no ZDR matrix row.
//!
//! [`crawl_schedule_config`] turns a canonical definition into the config a scheduled fire
//! starts the job with. The orchestrator reads only seeds, depth, pages, ingest and
//! `scrape_options` from a scheduled crawl, and the scrape workflow reads only the URL and
//! `ingest`, so an option the fire path cannot honour is refused as `not_schedulable`, naming
//! the key, rather than saved and then silently ignored.
//!
//! Nothing here touches the network or `AppState`. Dark behind `QUARRY_EDGE__SAVED_JOBS`
//! (default off), read per request through [`enabled`].
// The source routes and the schedule fire path that call this arrive in later units; the
// binary builds the module before then.
#![allow(dead_code)]

use serde_json::{Map, Value};

use quarry_core::scrape::{FieldError, ScrapeLimits, ScrapeOptions, RESERVED_REJECTED};

use crate::crawl_request::{self, CrawlRequestSettings, RequestField};
use crate::job_results_routes::flag_is_on;

/// Turns saved job definitions on. Off (the default), the routes that take a definition answer
/// as if the field did not exist.
pub const ENV_SAVED_JOBS: &str = "QUARRY_EDGE__SAVED_JOBS";

/// The only definition version this build reads and writes.
pub const DEFINITION_VERSION: u64 = 1;

/// The largest definition accepted, measured on its compact JSON form.
pub const MAX_DEFINITION_BYTES: usize = 16 * 1024;

/// The key a definition is stored under in `Source.config`.
pub const DEFINITION_KEY: &str = "definition";

/// The job kinds a definition may name.
pub const SAVED_KINDS: &[&str] = &["scrape", "crawl"];

/// Where a field error from the options is reported.
const OPTIONS_PATH: &str = "definition.options";

/// Option keys no saved definition may carry, in any spelling. `url` belongs to the Source;
/// the other three change what a run may do or keep.
const NOT_SAVEABLE_KEYS: &[&str] = &[
    "url",
    "zdr",
    "webhook",
    "ignore_robots_txt",
    "ignoreRobotsTxt",
    "ignoreRobots",
];

/// The crawl options a scheduled fire carries through to the job.
const SCHEDULABLE_CRAWL_KEYS: &[&str] = &["max_pages", "max_depth", "ingest", "scrape_options"];

/// Whether saved job definitions are enabled. Read per request, so a flip needs no restart.
pub fn enabled() -> bool {
    flag_is_on(std::env::var(ENV_SAVED_JOBS).ok().as_deref())
}

fn field(path: impl Into<String>, code: &'static str, message: impl Into<String>) -> RequestField {
    RequestField {
        path: path.into(),
        code,
        message: message.into(),
    }
}

/// The wire code of a scrape-validator error, as a static string. Codes this module does not
/// know map to `invalid_option`, as the crawl handoff does.
fn static_code(code: &str) -> &'static str {
    match code {
        "invalid_type" => "invalid_type",
        "unsupported_format" => "unsupported_format",
        "unsupported_option" => "unsupported_option",
        "unsupported_extractor" => "unsupported_extractor",
        "unsupported_on_error" => "unsupported_on_error",
        "unsupported_redact_pii" => "unsupported_redact_pii",
        "conflicts_with_only_main_content" => "conflicts_with_only_main_content",
        "conflicting_aliases" => "conflicting_aliases",
        "reserved_field" => "reserved_field",
        "unknown_field" => "unknown_field",
        "out_of_range" => "out_of_range",
        "too_many" => "too_many",
        "too_long" => "too_long",
        "empty" => "empty",
        "feature_disabled" => "feature_disabled",
        "requires_zdr" => "requires_zdr",
        "not_supported_with_actions" => "not_supported_with_actions",
        "not_supported_with_no_egress" => "not_supported_with_no_egress",
        "not_supported_with_redact_pii" => "not_supported_with_redact_pii",
        "invalid_locale" => "invalid_locale",
        "invalid_timezone" => "invalid_timezone",
        "missing_field" => "required",
        "required" => "required",
        "invalid_pattern" => "invalid_pattern",
        _ => "invalid_option",
    }
}

/// Prefix an options-relative path with where the options sit in the definition.
fn options_path(path: &str) -> String {
    if path == "$" || path.is_empty() {
        OPTIONS_PATH.to_owned()
    } else {
        format!("{OPTIONS_PATH}.{path}")
    }
}

fn from_scrape_errors(errors: Vec<FieldError>) -> Vec<RequestField> {
    errors
        .into_iter()
        .map(|error| {
            field(
                options_path(&error.path),
                static_code(&error.code),
                error.message,
            )
        })
        .collect()
}

/// `onlyMainContent` -> `only_main_content`. A key that is already snake case is unchanged.
fn camel_to_snake(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    for ch in key.chars() {
        if ch.is_ascii_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// A `not_saveable` error for every key of `options` that a definition may not carry. Sorted by
/// key, so the answer does not depend on the order the caller sent.
fn not_saveable(options: &Map<String, Value>) -> Vec<RequestField> {
    let mut keys: Vec<&String> = options
        .keys()
        .filter(|key| NOT_SAVEABLE_KEYS.contains(&key.as_str()))
        .collect();
    keys.sort();
    keys.into_iter()
        .map(|key| {
            let message = if key == "url" {
                "the URL is the source's own; a definition holds options only".to_owned()
            } else {
                format!("`{key}` cannot be part of a saved job")
            };
            field(options_path(key), "not_saveable", message)
        })
        .collect()
}

/// Check a saved job definition and return its canonical form.
///
/// `source_kind` is the Source's kind (`scrape` or `crawl`; anything else, a `search` source
/// for one, is `unsupported_kind`), `source_url` the Source's URL. `settings` is the process
/// crawl request setting; the crawl check always runs strict and never accepts a per-job
/// webhook, whatever `settings` says. Every problem found at one level is reported together.
pub fn check(
    source_kind: &str,
    source_url: &str,
    raw: &Value,
    settings: CrawlRequestSettings,
) -> Result<Value, Vec<RequestField>> {
    let size = serde_json::to_vec(raw)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    if size > MAX_DEFINITION_BYTES {
        return Err(vec![field(
            DEFINITION_KEY,
            "too_long",
            format!("a saved job definition is at most {MAX_DEFINITION_BYTES} bytes"),
        )]);
    }
    let Some(object) = raw.as_object() else {
        return Err(vec![field(
            DEFINITION_KEY,
            "invalid_type",
            "a saved job definition must be a JSON object",
        )]);
    };

    let mut errors: Vec<RequestField> = Vec::new();
    let mut keys: Vec<&String> = object.keys().collect();
    keys.sort();
    for key in keys {
        if !matches!(key.as_str(), "version" | "job_kind" | "options") {
            errors.push(field(
                format!("{DEFINITION_KEY}.{key}"),
                "unknown_field",
                format!("`{key}` is not a saved job definition field"),
            ));
        }
    }
    for required in ["version", "job_kind", "options"] {
        if !object.contains_key(required) {
            errors.push(field(
                format!("{DEFINITION_KEY}.{required}"),
                "required",
                format!("`{required}` is required"),
            ));
        }
    }
    match object.get("version") {
        None => {}
        Some(Value::Number(number)) => {
            if number.as_u64() != Some(DEFINITION_VERSION) {
                errors.push(field(
                    format!("{DEFINITION_KEY}.version"),
                    "unsupported_version",
                    format!("only version {DEFINITION_VERSION} is supported"),
                ));
            }
        }
        Some(_) => errors.push(field(
            format!("{DEFINITION_KEY}.version"),
            "invalid_type",
            "must be a number",
        )),
    }
    let mut kind: Option<&str> = None;
    match object.get("job_kind") {
        None => {}
        Some(Value::String(text)) => {
            if !SAVED_KINDS.contains(&text.as_str()) {
                errors.push(field(
                    format!("{DEFINITION_KEY}.job_kind"),
                    "unsupported_kind",
                    format!("must be one of {}", SAVED_KINDS.join(", ")),
                ));
            } else if text != source_kind {
                errors.push(field(
                    format!("{DEFINITION_KEY}.job_kind"),
                    "unsupported_kind",
                    "must match the kind of the source",
                ));
            } else {
                kind = Some(if text == "crawl" { "crawl" } else { "scrape" });
            }
        }
        Some(_) => errors.push(field(
            format!("{DEFINITION_KEY}.job_kind"),
            "invalid_type",
            "must be a string",
        )),
    }
    let options = match object.get("options") {
        None => None,
        Some(Value::Object(options)) => Some(options),
        Some(_) => {
            errors.push(field(OPTIONS_PATH, "invalid_type", "must be a JSON object"));
            None
        }
    };
    if !errors.is_empty() {
        return Err(errors);
    }
    let (Some(kind), Some(options)) = (kind, options) else {
        // Unreachable once `errors` is empty; refuse rather than guess.
        return Err(vec![field(
            DEFINITION_KEY,
            "invalid_option",
            "is not a valid definition",
        )]);
    };

    let canonical_options = match kind {
        "crawl" => check_crawl(source_url, options, settings)?,
        _ => check_scrape(options)?,
    };

    let mut canonical = Map::new();
    canonical.insert("version".to_owned(), Value::from(DEFINITION_VERSION));
    canonical.insert("job_kind".to_owned(), Value::from(kind));
    canonical.insert("options".to_owned(), Value::Object(canonical_options));
    Ok(Value::Object(canonical))
}

/// The scrape options of a definition. The scrape reader ignores keys that are not options, so
/// the key checks the live route leaves to its body type are made here: a reserved key, a key
/// that is not saveable and an unknown key are each refused. The canonical form is the accepted
/// options under their snake case names.
fn check_scrape(options: &Map<String, Value>) -> Result<Map<String, Value>, Vec<RequestField>> {
    let mut errors = not_saveable(options);
    let known = ScrapeOptions::field_names();

    let mut keys: Vec<&String> = options.keys().collect();
    keys.sort();
    let mut normalised: Map<String, Value> = Map::new();
    for key in keys {
        if NOT_SAVEABLE_KEYS.contains(&key.as_str()) {
            continue;
        }
        if RESERVED_REJECTED.contains(&key.as_str()) {
            errors.push(field(
                options_path(key),
                "reserved_field",
                format!("`{key}` is not accepted"),
            ));
            continue;
        }
        let snake = camel_to_snake(key);
        if !known.contains(&key.as_str()) || !known.contains(&snake.as_str()) {
            errors.push(field(
                options_path(key),
                "unknown_field",
                format!("`{key}` is not a scrape option"),
            ));
            continue;
        }
        let value = &options[key];
        if value.is_null() {
            continue;
        }
        if normalised.contains_key(&snake) {
            errors.push(field(
                options_path(&snake),
                "conflicting_aliases",
                format!("send one spelling of `{snake}`"),
            ));
            continue;
        }
        normalised.insert(snake, value.clone());
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let root = Value::Object(normalised);
    let parsed = ScrapeOptions::from_value(&root).map_err(from_scrape_errors)?;
    parsed
        .resolve(false, &ScrapeLimits::default())
        .map_err(from_scrape_errors)?;
    match root {
        Value::Object(map) => Ok(map),
        _ => Ok(Map::new()),
    }
}

/// The crawl options of a definition: the source URL goes in as the seed and the body runs
/// through the `/v1/crawl` parser in strict mode. The canonical form is re-serialised from what
/// the parser accepted, with defaults left out.
fn check_crawl(
    source_url: &str,
    options: &Map<String, Value>,
    settings: CrawlRequestSettings,
) -> Result<Map<String, Value>, Vec<RequestField>> {
    let refused = not_saveable(options);
    if !refused.is_empty() {
        return Err(refused);
    }
    let mut body = options.clone();
    body.insert("url".to_owned(), Value::String(source_url.to_owned()));
    let bytes = serde_json::to_vec(&Value::Object(body)).map_err(|_| {
        vec![field(
            OPTIONS_PATH,
            "invalid_json",
            "the options are not valid JSON",
        )]
    })?;
    let settings = CrawlRequestSettings {
        strict: true,
        job_webhook: false,
        ..settings
    };
    let parsed = crawl_request::parse_body(&bytes, settings).map_err(|refusal| {
        refusal
            .fields()
            .iter()
            .map(|found| {
                let path = if found.path == "url" {
                    "source.url".to_owned()
                } else {
                    options_path(&found.path)
                };
                field(path, found.code, found.message.clone())
            })
            .collect::<Vec<_>>()
    })?;

    // The parser records these rather than refusing them; the key check above already did, so
    // a value here means the two drifted. Refuse rather than save it.
    if parsed.ignore_robots_txt.is_some() || parsed.webhook.is_some() || parsed.zdr.is_some() {
        return Err(vec![field(
            OPTIONS_PATH,
            "not_saveable",
            "a robots override, a webhook and a ZDR declaration cannot be part of a saved job",
        )]);
    }

    let mut canonical = Map::new();
    if let Some(max_pages) = parsed.max_pages {
        canonical.insert("max_pages".to_owned(), Value::from(max_pages));
    }
    if let Some(max_depth) = parsed.max_depth {
        canonical.insert("max_depth".to_owned(), Value::from(max_depth));
    }
    if let Some(ingest) = parsed.ingest {
        canonical.insert("ingest".to_owned(), Value::Bool(ingest));
    }
    let scope = &parsed.scope;
    if scope.allow_subdomains {
        canonical.insert("allow_subdomains".to_owned(), Value::Bool(true));
    }
    if scope.allow_external_links {
        canonical.insert("allow_external_links".to_owned(), Value::Bool(true));
    }
    if !scope.crawl_entire_domain {
        canonical.insert("crawl_entire_domain".to_owned(), Value::Bool(false));
    }
    if !scope.include_paths.is_empty() {
        canonical.insert(
            "include_paths".to_owned(),
            Value::from(scope.include_paths.clone()),
        );
    }
    if !scope.exclude_paths.is_empty() {
        canonical.insert(
            "exclude_paths".to_owned(),
            Value::from(scope.exclude_paths.clone()),
        );
    }
    if let Some(extensions) = &scope.deny_extensions {
        canonical.insert(
            "deny_extensions".to_owned(),
            Value::from(extensions.clone()),
        );
    }
    if parsed.sitemap != crawl_request::SitemapMode::Skip {
        canonical.insert("sitemap".to_owned(), Value::from(parsed.sitemap.wire()));
    }
    if let Some(scrape_options) = &parsed.scrape_options {
        canonical.insert("scrape_options".to_owned(), scrape_options.to_value());
    }
    Ok(canonical)
}

/// The config a scheduled fire starts the job with, from a canonical definition (the output of
/// [`check`]) and the source URL: `{url, max_pages?, max_depth?, ingest?, scrape_options?,
/// definition_version}`.
///
/// An option the fire path cannot honour is refused as `not_schedulable`, naming the key
/// (`allow_subdomains`, `allow_external_links`, `crawl_entire_domain`, `include_paths`,
/// `exclude_paths`, `deny_extensions`, `sitemap`, `ignore_query_parameters` and anything else
/// outside the crawl list). A scrape definition with any option is `not_schedulable` as well;
/// one without options yields `{url, definition_version}`.
pub fn crawl_schedule_config(source_url: &str, canonical: &Value) -> Result<Value, RequestField> {
    let Some(object) = canonical.as_object() else {
        return Err(field(
            DEFINITION_KEY,
            "invalid_type",
            "a saved job definition must be a JSON object",
        ));
    };
    let version = match object.get("version").and_then(Value::as_u64) {
        Some(version) if version == DEFINITION_VERSION => version,
        _ => {
            return Err(field(
                format!("{DEFINITION_KEY}.version"),
                "unsupported_version",
                format!("only version {DEFINITION_VERSION} is supported"),
            ))
        }
    };
    let kind = object
        .get("job_kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let empty = Map::new();
    let options = match object.get("options") {
        Some(Value::Object(options)) => options,
        None => &empty,
        Some(_) => return Err(field(OPTIONS_PATH, "invalid_type", "must be a JSON object")),
    };

    let mut config = Map::new();
    config.insert("url".to_owned(), Value::String(source_url.to_owned()));
    match kind {
        "scrape" => {
            if let Some(key) = options.keys().min() {
                return Err(not_schedulable(
                    key,
                    "a scheduled scrape runs with the URL alone",
                ));
            }
        }
        "crawl" => {
            let mut keys: Vec<&String> = options.keys().collect();
            keys.sort();
            for key in keys {
                if !SCHEDULABLE_CRAWL_KEYS.contains(&key.as_str()) {
                    return Err(not_schedulable(
                        key,
                        "a scheduled crawl does not carry this option yet",
                    ));
                }
            }
            for key in SCHEDULABLE_CRAWL_KEYS {
                if let Some(value) = options.get(*key) {
                    config.insert((*key).to_owned(), value.clone());
                }
            }
        }
        _ => {
            return Err(field(
                format!("{DEFINITION_KEY}.job_kind"),
                "unsupported_kind",
                format!("must be one of {}", SAVED_KINDS.join(", ")),
            ))
        }
    }
    config.insert("definition_version".to_owned(), Value::from(version));
    Ok(Value::Object(config))
}

fn not_schedulable(key: &str, why: &str) -> RequestField {
    field(
        options_path(key),
        "not_schedulable",
        format!("`{key}`: {why}"),
    )
}
