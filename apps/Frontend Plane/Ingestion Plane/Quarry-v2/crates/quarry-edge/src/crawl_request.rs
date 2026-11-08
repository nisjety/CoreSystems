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
//! - `ignore_robots_txt: true` is a 403 `policy_denied` unless the WS14.S7a override
//!   allows it ([`robots_override`]): a person (never a service, an API key or the
//!   Model), web policy and the org claim both enforced, the org's signed claim raising
//!   the robots ceiling to `ignore` (`web.robots_override`, WS7.H/WS7.I), no operator
//!   ceiling below it, and the org on the operator grant list
//!   (`QUARRY_EDGE__ROBOTS_OVERRIDE_ORGS`, OQ-11: platform operators grant). Any miss
//!   is the same 403 as before, with the same field.
//! - Patterns obey the D3 caps (100 patterns, 512 characters each, 8192 in total)
//!   and must compile (`re:` prefixes are regular expressions).
//!
//! - W3-E3.5-3: `webhook` is reserved (a 400) unless `QUARRY_EDGE__JOB_WEBHOOK` is on
//!   (default off). With it on, `webhook` is a per-job target `{url, events}` (or a bare URL
//!   string) checked by [`check_job_webhook`]: https only, no credentials or fragment, no
//!   literal private, loopback or link-local address and no `localhost`, `events` from the
//!   closed [`JOB_WEBHOOK_EVENTS`] set. Control vets the resolved host again (the SSRF
//!   guard) and signs every delivery. A ZDR request may not carry one.
//!
//! `max_pages: 0` is the default in `lenient` and a 400 `invalid_option` in
//! `strict` (the W1 WS6.A semantics until strict is set). `max_depth: 0` takes
//! the default in both modes, as the existing contract documents.
//!
//! Field errors are `{path, code, message}`: the path is the spelling the caller
//! sent, so a caller can correct itself. Messages never echo a caller's value.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use quarry_core::crawl_scope::{
    CrawlContext, CrawlScope, MAX_EXTENSIONS, MAX_PATTERNS, MAX_PATTERN_CHARS,
    MAX_PATTERN_TOTAL_CHARS,
};
use quarry_core::web_policy::{OrgInput, RobotsMax};
use serde_json::{Map, Value};

use crate::auth::Claims;
use crate::crawl_policy::CrawlFeatures;
use crate::crawl_scrape::CrawlScrapeOptions;
use crate::policy_baseline::{Enforcement, PolicyBaseline};

/// The environment name. Read once through [`CrawlRequestSettings::current`].
pub const ENV_REQUEST_VALIDATION: &str = "QUARRY_EDGE__CRAWL_REQUEST_VALIDATION";

/// W3-E3.5-3: the per-job webhook switch. Off (the default): `webhook` stays a reserved key.
pub const ENV_JOB_WEBHOOK: &str = "QUARRY_EDGE__JOB_WEBHOOK";

/// The request key a per-job webhook travels under (crawl and batch alike).
pub const JOB_WEBHOOK_KEY: &str = "webhook";

/// The closed vocabulary a per-job webhook may name in `events`: the stored event types of
/// the job's run. Control holds the same set (`JobWebhookEvents` in `webhooks.go`).
pub const JOB_WEBHOOK_EVENTS: &[&str] = &[
    "page_failed",
    "page_fetched",
    "run_cancelled",
    "run_completed",
    "run_failed",
    "run_started",
    "run_timed_out",
];

/// What a per-job webhook gets when `events` is absent: the run's terminal events.
pub const DEFAULT_JOB_WEBHOOK_EVENTS: &[&str] =
    &["run_cancelled", "run_completed", "run_failed", "run_timed_out"];

/// The longest webhook URL accepted, as for a webhook subscription.
pub const MAX_JOB_WEBHOOK_URL_CHARS: usize = 2048;

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
    // W3-E3.5-3: also in RESERVED_KEYS, which refuses it while the flag is off.
    ("webhook", &["webhook"]),
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
    /// W3-E3.5-3: `QUARRY_EDGE__JOB_WEBHOOK`. Off: `webhook` is a reserved key.
    pub job_webhook: bool,
}

/// `on`, `1` or `true` (any case) turn the flag on. Anything else, including a typo, is off.
fn job_webhook_flag(value: Option<String>) -> bool {
    matches!(
        value.as_deref().map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("on" | "1" | "true")
    )
}

impl CrawlRequestSettings {
    /// Read the setting through a lookup, so tests need no process environment.
    /// An unknown value is logged (the value is not) and treated as `lenient`.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let value = lookup(ENV_REQUEST_VALIDATION).map(|value| value.trim().to_ascii_lowercase());
        let features = CrawlFeatures::from_lookup(&lookup);
        let job_webhook = job_webhook_flag(lookup(ENV_JOB_WEBHOOK));
        let strict = match value.as_deref() {
            None | Some("") | Some("lenient") => false,
            Some("strict") => true,
            Some(_) => {
                tracing::warn!(
                    "QUARRY_EDGE__CRAWL_REQUEST_VALIDATION is not lenient or strict; using lenient"
                );
                false
            }
        };
        Self {
            strict,
            features,
            job_webhook,
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
    /// WS14.S7a: the spelling the caller sent `ignore_robots_txt: true` under, `None` when the
    /// key is absent or false. A body that asks for it is only accepted by the handoff after
    /// [`robots_override`] allows it; otherwise it is the 403 it always was.
    pub ignore_robots_txt: Option<String>,
    /// W3-E3.5-3: the checked per-job webhook. `None` when absent (or the flag is off, which
    /// refuses the key instead).
    pub webhook: Option<JobWebhook>,
    pub warnings: Vec<RequestWarning>,
}

/// W3-E3.5-3: a per-job webhook target as the edge accepted it. The URL passed the syntax
/// and literal-address checks here; control resolves the host through its egress guard before
/// it stores the hook on the job, and the dispatcher's pinned dial checks it again per delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobWebhook {
    pub url: String,
    /// Sorted and without duplicates, so equal sets compare equal.
    pub events: Vec<String>,
}

impl JobWebhook {
    /// The value the handoff stamps into the job params under [`JOB_WEBHOOK_KEY`].
    pub fn params_value(&self) -> Value {
        serde_json::json!({ "url": self.url, "events": self.events })
    }
}

/// An address a webhook may never target: private, loopback, link-local, unspecified,
/// broadcast, documentation, multicast, carrier-grade NAT, "this network", unique-local, and an
/// IPv4-mapped IPv6 form of any of them.
fn is_blocked_webhook_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || octets[0] == 0
                || (octets[0] == 100 && (octets[1] & 0xc0) == 64)
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| is_blocked_webhook_ip(IpAddr::V4(v4)))
        }
    }
}

/// Check a per-job webhook URL. The reason codes are the ones control answers for a webhook
/// subscription (`validateWebhookURL`), so a caller sees one vocabulary.
fn check_job_webhook_url(path: &str, raw: &str) -> Result<String, RequestField> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(RequestField::new(path, "url_invalid", "a webhook URL is required"));
    }
    if raw.chars().count() > MAX_JOB_WEBHOOK_URL_CHARS {
        return Err(RequestField::new(
            path,
            "url_too_long",
            format!("at most {MAX_JOB_WEBHOOK_URL_CHARS} characters"),
        ));
    }
    let Ok(parsed) = url::Url::parse(raw) else {
        return Err(RequestField::new(path, "url_invalid", "is not a valid absolute URL"));
    };
    if parsed.scheme() != "https" {
        return Err(RequestField::new(path, "url_not_https", "must use https"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(RequestField::new(path, "url_userinfo", "must not carry credentials"));
    }
    if parsed.fragment().is_some() || raw.contains('#') {
        return Err(RequestField::new(path, "url_fragment", "must not carry a fragment"));
    }
    let blocked = match parsed.host() {
        None => {
            return Err(RequestField::new(path, "url_invalid", "has no host"));
        }
        Some(url::Host::Ipv4(ip)) => is_blocked_webhook_ip(std::net::IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_blocked_webhook_ip(std::net::IpAddr::V6(ip)),
        Some(url::Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            domain == "localhost" || domain.ends_with(".localhost")
        }
    };
    if blocked {
        return Err(RequestField::new(
            path,
            "url_private_address",
            "must not target a private, loopback or link-local address",
        ));
    }
    Ok(parsed.to_string())
}

/// Check a per-job `events` list: non-empty, every entry in [`JOB_WEBHOOK_EVENTS`], duplicates
/// dropped, sorted. Absent is [`DEFAULT_JOB_WEBHOOK_EVENTS`].
fn check_job_webhook_events(
    path: &str,
    value: Option<&Value>,
    invalid: &mut Vec<RequestField>,
) -> Vec<String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return DEFAULT_JOB_WEBHOOK_EVENTS.iter().map(|e| (*e).to_owned()).collect();
    };
    let Value::Array(items) = value else {
        invalid.push(RequestField::new(path, "invalid_type", "must be an array of event names"));
        return Vec::new();
    };
    if items.is_empty() {
        invalid.push(RequestField::new(path, "events_empty", "must name at least one event"));
        return Vec::new();
    }
    let mut events = BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        match item.as_str().map(str::trim) {
            Some(name) if JOB_WEBHOOK_EVENTS.contains(&name) => {
                events.insert(name.to_owned());
            }
            Some(_) => invalid.push(RequestField::new(
                format!("{path}[{index}]"),
                "events_unsupported",
                format!("must be one of {}", JOB_WEBHOOK_EVENTS.join(", ")),
            )),
            None => invalid.push(RequestField::new(
                format!("{path}[{index}]"),
                "invalid_type",
                "must be a string",
            )),
        }
    }
    events.into_iter().collect()
}

/// W3-E3.5-3: check a per-job webhook value sent under `path`. A bare string is the URL with
/// the default events; an object takes `url` and `events`. `headers` is refused (a caller does
/// not choose what control sends), `metadata` is not built, and any other key is unknown in
/// both modes. Every problem is reported together.
pub fn check_job_webhook(path: &str, value: &Value) -> Result<JobWebhook, Vec<RequestField>> {
    let mut invalid = Vec::new();
    let (url_value, events_value) = match value {
        Value::String(_) => (Some(value), None),
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            for key in keys {
                match key.as_str() {
                    "url" | "events" => {}
                    "headers" => invalid.push(RequestField::new(
                        format!("{path}.{key}"),
                        "reserved_field",
                        "a per-job webhook does not take headers",
                    )),
                    "metadata" => invalid.push(RequestField::new(
                        format!("{path}.{key}"),
                        "unsupported_option",
                        "is not supported yet",
                    )),
                    _ => invalid.push(RequestField::new(
                        format!("{path}.{key}"),
                        "unknown_field",
                        format!("`{key}` is not a webhook field"),
                    )),
                }
            }
            (object.get("url"), object.get("events"))
        }
        _ => {
            return Err(vec![RequestField::new(
                path,
                "invalid_type",
                "must be a URL string or an object with url and events",
            )]);
        }
    };
    let url_path = match value {
        Value::String(_) => path.to_owned(),
        _ => format!("{path}.url"),
    };
    let url = match url_value.filter(|value| !value.is_null()) {
        None => {
            invalid.push(RequestField::new(url_path.as_str(), "required", "a webhook URL is required"));
            String::new()
        }
        Some(Value::String(raw)) => match check_job_webhook_url(&url_path, raw) {
            Ok(url) => url,
            Err(field) => {
                invalid.push(field);
                String::new()
            }
        },
        Some(_) => {
            invalid.push(RequestField::new(url_path.as_str(), "invalid_type", "must be a string"));
            String::new()
        }
    };
    let events = check_job_webhook_events(&format!("{path}.events"), events_value, &mut invalid);
    if invalid.is_empty() {
        Ok(JobWebhook { url, events })
    } else {
        Err(invalid)
    }
}

/// W3-E3.5-3: the per-job webhook of a request body, for any job route (crawl here, batch in
/// its own handler). `Ok(None)` when the key is absent. With the flag off a present key is the
/// `reserved_field` 400 it has always been. A ZDR request (`zdr: true` in the body or on the
/// caller's claims, which the caller passes as `zdr`) may not carry one: control would keep
/// the delivery payloads.
pub fn job_webhook_field(
    object: &Map<String, Value>,
    enabled: bool,
    zdr: bool,
) -> Result<Option<JobWebhook>, Vec<RequestField>> {
    let Some(value) = object.get(JOB_WEBHOOK_KEY).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    if !enabled {
        return Err(vec![RequestField::new(
            JOB_WEBHOOK_KEY,
            "reserved_field",
            format!("`{JOB_WEBHOOK_KEY}` is not accepted"),
        )]);
    }
    if zdr {
        return Err(vec![RequestField::new(
            JOB_WEBHOOK_KEY,
            "zdr_refused",
            "a zero-data-retention job cannot carry a webhook",
        )]);
    }
    check_job_webhook(JOB_WEBHOOK_KEY, value).map(Some)
}

/// WS14.S7a (OQ-11): the operator grant list for the robots.txt override, a comma-separated
/// list of org ids. Unset or empty (the default): no org is granted, so the override is
/// refused for everyone whatever the org's claim says. Only platform operators, who set the
/// edge environment, can grant it; an org admin can only raise the org's own ceiling (WS7.H).
pub const ENV_ROBOTS_OVERRIDE_ORGS: &str = "QUARRY_EDGE__ROBOTS_OVERRIDE_ORGS";

/// WS14.S7a: the entitlement an override job names as its authority (`params.robots.authority`).
pub const ROBOTS_OVERRIDE_AUTHORITY: &str = "web.robots_override";

/// WS14.S7a: the org ids the operators granted the robots.txt override, read once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RobotsOverrideGrants {
    orgs: BTreeSet<String>,
}

impl RobotsOverrideGrants {
    /// Read the list through a lookup, so tests need no process environment. Entries are
    /// trimmed; empty entries are dropped. The value is never logged.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let orgs = lookup(ENV_ROBOTS_OVERRIDE_ORGS)
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|org| !org.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Self { orgs }
    }

    /// The process list, read once from the environment.
    pub fn current() -> &'static Self {
        static GRANTS: OnceLock<RobotsOverrideGrants> = OnceLock::new();
        GRANTS.get_or_init(|| Self::from_lookup(|name| std::env::var(name).ok()))
    }

    /// Whether the operators granted `org_id` the override.
    pub fn grants(&self, org_id: &str) -> bool {
        !org_id.trim().is_empty() && self.orgs.contains(org_id)
    }
}

/// WS14.S7a: why the override was refused. Closed labels, for the counter and the log only:
/// the caller always gets the same 403 field, whichever check refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideRefusal {
    /// A service principal, an API key (the Model reaches Quarry only as a service), or a token
    /// without a user to attribute the audit event to.
    NotAPerson,
    /// `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` or `QUARRY_EDGE__WEB_POLICY_CLAIM_ENFORCEMENT` is not
    /// `enforce`: the org's claim does not bind the run, so it cannot unlock anything either.
    NotEnforced,
    /// No readable org policy: the claim is absent, unreadable, or the baseline `rev` `"0"`.
    NoOrgPolicy,
    /// The org's claim does not raise the robots ceiling to `ignore`.
    NotEntitled,
    /// The operator baseline caps robots below `ignore`.
    OperatorCeiling,
    /// The org is not on the operator grant list (OQ-11).
    NotGranted,
}

impl OverrideRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotAPerson => "not_a_person",
            Self::NotEnforced => "not_enforced",
            Self::NoOrgPolicy => "no_org_policy",
            Self::NotEntitled => "not_entitled",
            Self::OperatorCeiling => "operator_ceiling",
            Self::NotGranted => "not_granted",
        }
    }
}

/// WS14.S7a: an allowed override. `rev` is the org policy revision that allowed it, in the
/// issuer's 12-hex form (`web_policy_edge::policy_rev_hex`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotsOverride {
    pub rev: String,
}

impl RobotsOverride {
    /// `params.robots` of the job: what quarry-control audits before it acknowledges the job
    /// (WS14.S7b) and what the orchestrator runs the crawl with.
    pub fn params_value(&self) -> Value {
        serde_json::json!({
            "mode": "ignore",
            "authority": ROBOTS_OVERRIDE_AUTHORITY,
            "rev": self.rev,
        })
    }

    /// `effective.robots_override` of the handoff acknowledgement (`effective.robots` stays the
    /// mode string, `"ignore"`).
    pub fn effective_value(&self) -> Value {
        serde_json::json!({
            "authority": ROBOTS_OVERRIDE_AUTHORITY,
            "rev": self.rev,
        })
    }
}

/// WS14.S7a: may this caller ignore robots.txt on a crawl?
///
/// The web-policy lattice is tighten-only and the catalogue baseline for `robots.max_mode` is
/// `respect`, so a resolved policy never reaches `ignore` (v1 builds no unlock path). The
/// override is therefore this explicit unlock, the `require(RobotsIgnore, Human)` of the plan:
/// every check below must pass, in this order.
pub fn decide_robots_override(
    claims: &Claims,
    baseline: &PolicyBaseline,
    grants: &RobotsOverrideGrants,
) -> Result<RobotsOverride, OverrideRefusal> {
    // ADR-1: never a service (the Model and every plane service) and never an API key.
    if !claims.is_person() || claims.user_id.trim().is_empty() {
        return Err(OverrideRefusal::NotAPerson);
    }
    if baseline.enforcement() != Enforcement::Enforce || baseline.claim_mode() != Enforcement::Enforce {
        return Err(OverrideRefusal::NotEnforced);
    }
    let (layer, rev) = match baseline.org_input(claims.web_policy_claim()) {
        OrgInput::Present { layer, rev } if rev != 0 => (layer, rev),
        OrgInput::Present { .. } | OrgInput::Absent | OrgInput::Unreadable => {
            return Err(OverrideRefusal::NoOrgPolicy)
        }
    };
    if layer.robots_max() != Some(RobotsMax::Ignore) {
        return Err(OverrideRefusal::NotEntitled);
    }
    if baseline
        .baseline()
        .robots_max()
        .is_some_and(|max| max < RobotsMax::Ignore)
    {
        return Err(OverrideRefusal::OperatorCeiling);
    }
    if !grants.grants(&claims.org_id) {
        return Err(OverrideRefusal::NotGranted);
    }
    Ok(RobotsOverride {
        rev: crate::web_policy_edge::policy_rev_hex(rev),
    })
}

/// WS14.S7a: the override for a parsed request. `Ok(None)` when the request does not ask to
/// ignore robots.txt. A refused ask is the same 403 `policy_denied` field the request got before
/// the override existed; the reason is counted and logged as a closed label only.
pub fn robots_override(
    parsed: &ParsedCrawl,
    claims: &Claims,
    baseline: &PolicyBaseline,
    grants: &RobotsOverrideGrants,
) -> Result<Option<RobotsOverride>, Refusal> {
    let Some(path) = parsed.ignore_robots_txt.as_deref() else {
        return Ok(None);
    };
    match decide_robots_override(claims, baseline, grants) {
        Ok(allowed) => {
            metrics::counter!("crawl_robots_override_total", "outcome" => "allowed", "reason" => "granted")
                .increment(1);
            tracing::info!(target: "quarry.web_policy", "crawl.robots_override_allowed");
            Ok(Some(allowed))
        }
        Err(reason) => {
            metrics::counter!(
                "crawl_robots_override_total",
                "outcome" => "refused",
                "reason" => reason.as_str()
            )
            .increment(1);
            tracing::info!(
                target: "quarry.web_policy",
                reason = reason.as_str(),
                "crawl.robots_override_refused"
            );
            Err(Refusal::PolicyDenied(vec![robots_denied_field(path)]))
        }
    }
}

/// The 403 field of a refused `ignore_robots_txt: true`. Unchanged since WS14.S6.
fn robots_denied_field(path: &str) -> RequestField {
    RequestField::new(
        path,
        "policy_denied",
        "ignoring robots.txt is not available to this request",
    )
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
        // W3-E3.5-3: with the flag on, `webhook` is an option and is checked below.
        .filter(|key| !(settings.job_webhook && key.as_str() == JOB_WEBHOOK_KEY))
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

    // WS14.S7a: `true` is recorded here and decided by [`robots_override`] in the handoff, which
    // has the caller's claims. The 400s of the body still come first.
    let mut ignore_robots_txt = None;
    if let Some((path, value)) = lookup(object, "ignore_robots_txt") {
        match value {
            Value::Bool(true) => ignore_robots_txt = Some(path.to_owned()),
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

    // W3-E3.5-3: the per-job webhook. Only reached with the flag on (off, the key is reserved
    // above). A body that also asks for ZDR is refused; the handoff checks the claims' ZDR.
    let webhook = if settings.job_webhook {
        match job_webhook_field(object, true, zdr == Some(true)) {
            Ok(found) => found,
            Err(found) => {
                invalid.extend(found);
                None
            }
        }
    } else {
        None
    };

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
        ignore_robots_txt,
        webhook,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lenient() -> CrawlRequestSettings {
        CrawlRequestSettings { strict: false, features: CrawlFeatures::default(), job_webhook: false }
    }

    fn strict() -> CrawlRequestSettings {
        CrawlRequestSettings { strict: true, features: CrawlFeatures::default(), job_webhook: false }
    }

    fn with_features(strict: bool, sitemap: bool, scrape_options: bool) -> CrawlRequestSettings {
        CrawlRequestSettings {
            strict,
            features: CrawlFeatures { sitemap, scrape_options },
            job_webhook: false,
        }
    }

    fn with_job_webhook(strict: bool) -> CrawlRequestSettings {
        CrawlRequestSettings { strict, features: CrawlFeatures::default(), job_webhook: true }
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
        // Without the override (the default edge, a person's token, no claim) the handoff's
        // answer is the 403 it always was: same code, the caller's spelling as the path.
        let claims = crate::test_support::claims_for_org("org_a");
        let baseline = PolicyBaseline::default();
        let grants = RobotsOverrideGrants::default();
        for key in ["ignore_robots_txt", "ignoreRobotsTxt", "ignoreRobots"] {
            let mut body = json!({ "url": "https://e.test/" });
            body[key] = json!(true);
            let parsed = parse(body.clone(), lenient()).expect("the body itself is valid");
            assert_eq!(parsed.ignore_robots_txt.as_deref(), Some(key));
            match robots_override(&parsed, &claims, &baseline, &grants) {
                Err(Refusal::PolicyDenied(fields)) => {
                    assert_eq!(fields.len(), 1);
                    assert_eq!(fields[0].code, "policy_denied");
                    assert_eq!(fields[0].path, key);
                    assert_eq!(
                        fields[0].message,
                        "ignoring robots.txt is not available to this request"
                    );
                }
                other => panic!("expected policy_denied for {key}, got {other:?}"),
            }
            body[key] = json!(false);
            let parsed = parse(body, lenient()).expect("false is accepted");
            assert_eq!(parsed.ignore_robots_txt, None, "{key}: false asks for nothing");
            assert_eq!(robots_override(&parsed, &claims, &baseline, &grants), Ok(None));
        }
        // A wrong type is still a 400, before any policy decision.
        assert_eq!(
            codes(&parse(json!({ "url": "https://e.test/", "ignore_robots_txt": "yes" }), lenient())),
            vec![("ignore_robots_txt".to_owned(), "invalid_type")]
        );
    }

    // WS14.S7a: the override branch.

    const ORG_REV: &str = "0123456789ab";

    fn enforcing() -> PolicyBaseline {
        PolicyBaseline::from_lookup(|name| match name {
            crate::policy_baseline::ENFORCEMENT_ENV => Some("enforce".to_owned()),
            crate::policy_baseline::CLAIM_ENFORCEMENT_ENV => Some("enforce".to_owned()),
            _ => None,
        })
        .expect("valid baseline")
    }

    fn granted() -> RobotsOverrideGrants {
        RobotsOverrideGrants::from_lookup(|name| {
            assert_eq!(name, ENV_ROBOTS_OVERRIDE_ORGS);
            Some(" org_b , org_a ,".to_owned())
        })
    }

    fn person_with_claim(robots_max: Option<&str>) -> Claims {
        let mut claims = crate::test_support::claims_for_org("org_a");
        let mut claim = json!({ "v": 1, "rev": ORG_REV });
        if let Some(max) = robots_max {
            claim["robots_max"] = json!(max);
        }
        claims.web_policy = Some(claim);
        claims
    }

    fn ignore_request() -> ParsedCrawl {
        parse(json!({ "url": "https://e.test/", "ignore_robots_txt": true }), lenient()).unwrap()
    }

    #[test]
    fn override_is_allowed_with_the_policy_and_stamps_params() {
        let allowed = robots_override(
            &ignore_request(),
            &person_with_claim(Some("ignore")),
            &enforcing(),
            &granted(),
        )
        .expect("allowed")
        .expect("an override");
        assert_eq!(allowed.rev, ORG_REV);
        assert_eq!(
            allowed.params_value(),
            json!({ "mode": "ignore", "authority": "web.robots_override", "rev": ORG_REV })
        );
        assert_eq!(
            allowed.effective_value(),
            json!({ "authority": "web.robots_override", "rev": ORG_REV })
        );
    }

    #[test]
    fn override_is_refused_without_the_entitlement() {
        let baseline = enforcing();
        let grants = granted();
        for (robots_max, reason) in [
            (Some("respect"), OverrideRefusal::NotEntitled),
            (Some("strict"), OverrideRefusal::NotEntitled),
            (None, OverrideRefusal::NotEntitled),
        ] {
            assert_eq!(
                decide_robots_override(&person_with_claim(robots_max), &baseline, &grants),
                Err(reason),
                "{robots_max:?}"
            );
        }
        // No claim, an unreadable claim, and the baseline rev "0" are no org policy.
        let mut none = person_with_claim(None);
        none.web_policy = None;
        assert_eq!(
            decide_robots_override(&none, &baseline, &grants),
            Err(OverrideRefusal::NoOrgPolicy)
        );
        let mut unreadable = person_with_claim(None);
        unreadable.web_policy = Some(json!({ "v": 2, "rev": ORG_REV, "robots_max": "ignore" }));
        assert_eq!(
            decide_robots_override(&unreadable, &baseline, &grants),
            Err(OverrideRefusal::NoOrgPolicy)
        );
        let mut baseline_rev = person_with_claim(None);
        baseline_rev.web_policy = Some(json!({ "v": 1, "rev": "0", "robots_max": "ignore" }));
        assert_eq!(
            decide_robots_override(&baseline_rev, &baseline, &grants),
            Err(OverrideRefusal::NoOrgPolicy)
        );
        // And the answer to the caller is the unchanged 403.
        assert!(matches!(
            robots_override(&ignore_request(), &person_with_claim(Some("respect")), &baseline, &grants),
            Err(Refusal::PolicyDenied(_))
        ));
    }

    #[test]
    fn override_is_refused_for_a_service_principal_and_an_api_key() {
        let baseline = enforcing();
        let grants = granted();
        let mut service = person_with_claim(Some("ignore"));
        service.principal_type = Some("service".into());
        service.service_id = Some("model-gateway".into());
        service.sub = "model-gateway".into();
        assert_eq!(
            decide_robots_override(&service, &baseline, &grants),
            Err(OverrideRefusal::NotAPerson)
        );
        let mut key = person_with_claim(Some("ignore"));
        key.principal_type = Some("api_key".into());
        key.key_id = Some("key_1".into());
        key.acting_user_id = Some("u1".into());
        assert_eq!(
            decide_robots_override(&key, &baseline, &grants),
            Err(OverrideRefusal::NotAPerson)
        );
        let mut anonymous = person_with_claim(Some("ignore"));
        anonymous.user_id = String::new();
        assert_eq!(
            decide_robots_override(&anonymous, &baseline, &grants),
            Err(OverrideRefusal::NotAPerson)
        );
        assert!(matches!(
            robots_override(&ignore_request(), &service, &baseline, &grants),
            Err(Refusal::PolicyDenied(_))
        ));
    }

    #[test]
    fn override_is_refused_unless_policy_and_claim_are_both_enforced() {
        let claims = person_with_claim(Some("ignore"));
        let grants = granted();
        // The defaults: enforcement enforce, the claim observed.
        assert_eq!(
            decide_robots_override(&claims, &PolicyBaseline::default(), &grants),
            Err(OverrideRefusal::NotEnforced)
        );
        for (enforcement, claim) in [
            ("observe", "enforce"),
            ("off", "enforce"),
            ("enforce", "observe"),
            ("enforce", "off"),
        ] {
            let baseline = PolicyBaseline::from_lookup(|name| match name {
                crate::policy_baseline::ENFORCEMENT_ENV => Some(enforcement.to_owned()),
                crate::policy_baseline::CLAIM_ENFORCEMENT_ENV => Some(claim.to_owned()),
                _ => None,
            })
            .unwrap();
            assert_eq!(
                decide_robots_override(&claims, &baseline, &grants),
                Err(OverrideRefusal::NotEnforced),
                "{enforcement}/{claim}"
            );
        }
    }

    #[test]
    fn override_is_refused_below_an_operator_ceiling_and_off_the_grant_list() {
        let claims = person_with_claim(Some("ignore"));
        let capped = PolicyBaseline::from_lookup(|name| match name {
            crate::policy_baseline::ENFORCEMENT_ENV => Some("enforce".to_owned()),
            crate::policy_baseline::CLAIM_ENFORCEMENT_ENV => Some("enforce".to_owned()),
            crate::policy_baseline::BASELINE_ENV => Some(r#"{"robots_max":"respect"}"#.to_owned()),
            _ => None,
        })
        .unwrap();
        assert_eq!(
            decide_robots_override(&claims, &capped, &granted()),
            Err(OverrideRefusal::OperatorCeiling)
        );
        // OQ-11: the default grant list is empty, so nobody is granted.
        assert_eq!(
            decide_robots_override(&claims, &enforcing(), &RobotsOverrideGrants::default()),
            Err(OverrideRefusal::NotGranted)
        );
        let other_org = RobotsOverrideGrants::from_lookup(|_| Some("org_b".to_owned()));
        assert_eq!(
            decide_robots_override(&claims, &enforcing(), &other_org),
            Err(OverrideRefusal::NotGranted)
        );
    }

    #[test]
    fn grant_list_is_trimmed_and_ignores_empty_entries() {
        let grants = granted();
        assert!(grants.grants("org_a"));
        assert!(grants.grants("org_b"));
        assert!(!grants.grants(""));
        assert!(!grants.grants(" org_a "));
        assert_eq!(RobotsOverrideGrants::from_lookup(|_| None), RobotsOverrideGrants::default());
        assert_eq!(
            RobotsOverrideGrants::from_lookup(|_| Some(" , ".to_owned())),
            RobotsOverrideGrants::default()
        );
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

    // W3-E3.5-3: the per-job webhook.

    #[test]
    fn job_webhook_flag_grammar_defaults_off() {
        let read = |value: Option<&str>| {
            CrawlRequestSettings::from_lookup(|name| {
                (name == ENV_JOB_WEBHOOK).then(|| value.map(str::to_owned)).flatten()
            })
            .job_webhook
        };
        assert!(!read(None));
        assert!(!read(Some("")));
        assert!(!read(Some("yes please")));
        assert!(read(Some("on")));
        assert!(read(Some(" TRUE ")));
        assert!(read(Some("1")));
    }

    #[test]
    fn job_webhook_is_reserved_while_the_flag_is_off() {
        let body = json!({ "url": "https://e.test/", "webhook": { "url": "https://hooks.example.com/q" } });
        for settings in [lenient(), strict()] {
            assert_eq!(
                codes(&parse(body.clone(), settings)),
                vec![("webhook".to_owned(), "reserved_field")]
            );
        }
    }

    #[test]
    fn job_webhook_is_accepted_with_the_flag_on() {
        for strict_mode in [false, true] {
            let parsed = parse(
                json!({
                    "url": "https://e.test/",
                    "webhook": {
                        "url": "https://hooks.example.com/q?x=1",
                        "events": ["run_completed", "page_fetched", "run_completed"]
                    }
                }),
                with_job_webhook(strict_mode),
            )
            .expect("accepted");
            let hook = parsed.webhook.expect("a webhook");
            assert_eq!(hook.url, "https://hooks.example.com/q?x=1");
            assert_eq!(hook.events, vec!["page_fetched".to_owned(), "run_completed".to_owned()]);
            assert_eq!(
                hook.params_value(),
                json!({ "url": "https://hooks.example.com/q?x=1", "events": ["page_fetched", "run_completed"] })
            );
            assert!(parsed.warnings.is_empty(), "webhook is a known field");
        }
    }

    #[test]
    fn a_bare_job_webhook_url_takes_the_terminal_events() {
        let parsed = parse(
            json!({ "url": "https://e.test/", "webhook": "https://hooks.example.com/q" }),
            with_job_webhook(false),
        )
        .expect("accepted");
        let hook = parsed.webhook.expect("a webhook");
        let expected: Vec<String> =
            DEFAULT_JOB_WEBHOOK_EVENTS.iter().map(|e| (*e).to_owned()).collect();
        assert_eq!(hook.events, expected);
    }

    #[test]
    fn job_webhook_without_the_key_is_none() {
        let parsed = parse(json!({ "url": "https://e.test/" }), with_job_webhook(false)).unwrap();
        assert_eq!(parsed.webhook, None);
    }

    #[test]
    fn job_webhook_urls_are_checked_like_subscriptions() {
        let cases = [
            ("http://hooks.example.com/q", "url_not_https"),
            ("https://user:pw@hooks.example.com/q", "url_userinfo"),
            ("https://hooks.example.com/q#frag", "url_fragment"),
            ("not a url", "url_invalid"),
            ("", "url_invalid"),
            ("https://127.0.0.1/q", "url_private_address"),
            ("https://10.1.2.3/q", "url_private_address"),
            ("https://192.168.0.10/q", "url_private_address"),
            ("https://169.254.169.254/latest", "url_private_address"),
            ("https://100.64.0.1/q", "url_private_address"),
            ("https://2130706433/q", "url_private_address"),
            ("https://[::1]/q", "url_private_address"),
            ("https://[fd00::1]/q", "url_private_address"),
            ("https://[fe80::1]/q", "url_private_address"),
            ("https://[::ffff:127.0.0.1]/q", "url_private_address"),
            ("https://localhost/q", "url_private_address"),
            ("https://api.localhost./q", "url_private_address"),
        ];
        for (url, code) in cases {
            let result = parse(
                json!({ "url": "https://e.test/", "webhook": { "url": url } }),
                with_job_webhook(false),
            );
            assert_eq!(codes(&result), vec![("webhook.url".to_owned(), code)], "{url}");
        }
        let long = format!("https://hooks.example.com/{}", "a".repeat(MAX_JOB_WEBHOOK_URL_CHARS));
        assert_eq!(
            codes(&parse(json!({ "url": "https://e.test/", "webhook": long }), with_job_webhook(false))),
            vec![("webhook".to_owned(), "url_too_long")]
        );
    }

    #[test]
    fn job_webhook_events_are_a_closed_set() {
        let empty = parse(
            json!({ "url": "https://e.test/", "webhook": { "url": "https://h.example.com/", "events": [] } }),
            with_job_webhook(false),
        );
        assert_eq!(codes(&empty), vec![("webhook.events".to_owned(), "events_empty")]);
        let unknown = parse(
            json!({ "url": "https://e.test/", "webhook": { "url": "https://h.example.com/", "events": ["run_completed", "change_detected", 3] } }),
            with_job_webhook(false),
        );
        assert_eq!(
            codes(&unknown),
            vec![
                ("webhook.events[1]".to_owned(), "events_unsupported"),
                ("webhook.events[2]".to_owned(), "invalid_type"),
            ]
        );
        let not_a_list = parse(
            json!({ "url": "https://e.test/", "webhook": { "url": "https://h.example.com/", "events": "run_completed" } }),
            with_job_webhook(false),
        );
        assert_eq!(codes(&not_a_list), vec![("webhook.events".to_owned(), "invalid_type")]);
    }

    #[test]
    fn job_webhook_refuses_headers_metadata_and_unknown_keys() {
        let result = parse(
            json!({ "url": "https://e.test/", "webhook": {
                "url": "https://h.example.com/", "headers": { "x": "y" }, "metadata": {}, "secret": "s"
            } }),
            with_job_webhook(false),
        );
        assert_eq!(
            codes(&result),
            vec![
                ("webhook.headers".to_owned(), "reserved_field"),
                ("webhook.metadata".to_owned(), "unsupported_option"),
                ("webhook.secret".to_owned(), "unknown_field"),
            ]
        );
        let wrong_type = parse(json!({ "url": "https://e.test/", "webhook": 7 }), with_job_webhook(false));
        assert_eq!(codes(&wrong_type), vec![("webhook".to_owned(), "invalid_type")]);
        let no_url = parse(
            json!({ "url": "https://e.test/", "webhook": { "events": ["run_completed"] } }),
            with_job_webhook(false),
        );
        assert_eq!(codes(&no_url), vec![("webhook.url".to_owned(), "required")]);
    }

    #[test]
    fn a_zdr_request_cannot_carry_a_job_webhook() {
        let result = parse(
            json!({ "url": "https://e.test/", "zdr": true, "webhook": "https://h.example.com/" }),
            with_job_webhook(false),
        );
        assert_eq!(codes(&result), vec![("webhook".to_owned(), "zdr_refused")]);
        let object = json!({ "webhook": "https://h.example.com/" });
        let object = object.as_object().unwrap();
        assert!(job_webhook_field(object, true, true).is_err(), "claims ZDR refuses too");
        assert!(job_webhook_field(object, true, false).unwrap().is_some());
        assert_eq!(
            job_webhook_field(object, false, false).unwrap_err()[0].code,
            "reserved_field"
        );
        assert_eq!(job_webhook_field(&Map::new(), false, true), Ok(None));
    }
}
