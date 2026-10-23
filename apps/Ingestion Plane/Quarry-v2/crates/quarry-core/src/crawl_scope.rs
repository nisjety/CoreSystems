//! Crawl scope contract for `POST /v1/internal/run_page` (WS14.S2).
//!
//! The orchestrator sends an optional `crawl` object with each page it runs.
//! The edge evaluates that page's links against the scope and returns the
//! allowed links plus a typed denial report. A request with no `crawl` object
//! never touches these types, so its response does not change.
//!
//! Wire version 1, snake_case throughout. The scope refuses unknown keys so a
//! misspelt field cannot silently widen a crawl. Pure data and validation here;
//! the evaluation lives in `quarry-runtime::crawl_filter`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::crawl_denial::CrawlDenialReason;

/// The only value `crawl.v` may take in this build.
pub const CRAWL_CONTRACT_VERSION: u32 = 1;
/// Include and exclude patterns per request, both lists together.
pub const MAX_PATTERNS: usize = 100;
/// Characters in one pattern, prefix included.
pub const MAX_PATTERN_CHARS: usize = 512;
/// Characters across every pattern in one request.
pub const MAX_PATTERN_TOTAL_CHARS: usize = 8192;
/// Links evaluated per page. Links past this are reported as unevaluated.
pub const MAX_LINKS_EVALUATED: usize = 5000;
/// Seed hosts per request.
pub const MAX_SEED_HOSTS: usize = 100;
/// Extension entries in `deny_extensions`.
pub const MAX_EXTENSIONS: usize = 100;
/// Denial samples kept per code, per page.
pub const MAX_SAMPLES_PER_CODE: usize = 3;
/// Denial samples kept in total, per page.
pub const MAX_SAMPLES_PER_PAGE: usize = 20;
/// Robots denials listed per page. Counts keep counting past this.
pub const MAX_ROBOTS_DENIALS_PER_PAGE: usize = 50;

/// What the crawl does with the page's links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlKind {
    /// Follow the links that pass the scope.
    #[default]
    Crawl,
    /// A batch page: its links are never followed, so the response carries none.
    Batch,
}

/// Whether the scope changes the response or only reports on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeMode {
    /// `links` holds only the allowed links, normalised.
    #[default]
    Enforce,
    /// `links` holds every href as sent; the report says what enforce would drop.
    Observe,
}

/// Whether cached robots.txt rules are consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RobotsMode {
    /// Deny links that cached robots rules disallow.
    #[default]
    Respect,
    /// Never consult robots rules for this crawl.
    Ignore,
}

/// The scope of one crawl. Defaults: same host only (`www.` equivalent),
/// external links off, subdomains off, whole domain, built-in extension list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CrawlScope {
    /// Follow subdomains of a seed host.
    pub allow_subdomains: bool,
    /// Follow links to hosts that are not related to a seed host.
    pub allow_external_links: bool,
    /// When false, only links under the seed URL's directory are followed.
    pub crawl_entire_domain: bool,
    /// Patterns a link must match at least one of, when non-empty.
    pub include_paths: Vec<String>,
    /// Patterns that deny a link when one matches. Beats `include_paths`.
    pub exclude_paths: Vec<String>,
    /// Extensions to deny (without the dot). `None` takes the built-in list.
    pub deny_extensions: Option<Vec<String>>,
    /// Set by the orchestrator for jobs accepted before scope existed.
    pub legacy: bool,
}

impl Default for CrawlScope {
    fn default() -> Self {
        Self {
            allow_subdomains: false,
            allow_external_links: false,
            crawl_entire_domain: true,
            include_paths: Vec::new(),
            exclude_paths: Vec::new(),
            deny_extensions: None,
            legacy: false,
        }
    }
}

/// The `crawl` object a caller sends with a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CrawlContext {
    /// Contract version. Must be [`CRAWL_CONTRACT_VERSION`].
    pub v: u32,
    pub kind: CrawlKind,
    /// Depth of the page being run (0 for the seed).
    pub depth: u32,
    /// Deepest link depth to follow. 0 means unlimited.
    pub max_depth: u32,
    /// Hosts the crawl started from. Empty means the page's own host.
    pub seed_hosts: Vec<String>,
    /// The crawl's seed URL. Used only when `crawl_entire_domain` is false;
    /// absent, the directory of the page being run is used.
    pub seed_url: Option<String>,
    pub scope: CrawlScope,
    pub scope_mode: ScopeMode,
    pub robots: RobotsMode,
}

impl Default for CrawlContext {
    fn default() -> Self {
        Self {
            v: CRAWL_CONTRACT_VERSION,
            kind: CrawlKind::default(),
            depth: 0,
            max_depth: 0,
            seed_hosts: Vec::new(),
            seed_url: None,
            scope: CrawlScope::default(),
            scope_mode: ScopeMode::default(),
            robots: RobotsMode::default(),
        }
    }
}

/// One rejected field of a `crawl` object. `field` is a dotted path such as
/// `crawl.scope.include_paths[3]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrawlFieldError {
    pub field: String,
    pub message: String,
}

impl CrawlFieldError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl CrawlContext {
    /// Parse and validate a raw `crawl` JSON value. Every error names the
    /// field, so the caller can return them as a 400 with `details.fields`.
    pub fn from_json(raw: serde_json::Value) -> Result<Self, Vec<CrawlFieldError>> {
        let ctx: CrawlContext = serde_json::from_value(raw).map_err(|e| {
            vec![CrawlFieldError::new(
                "crawl",
                format!("malformed crawl object: {e}"),
            )]
        })?;
        ctx.validate()?;
        Ok(ctx)
    }

    /// Check the fields that serde cannot: version, host shapes, the seed URL
    /// and the scope caps. Regular expressions are checked when the scope is
    /// compiled in `quarry-runtime`.
    pub fn validate(&self) -> Result<(), Vec<CrawlFieldError>> {
        let mut errs = Vec::new();
        if self.v != CRAWL_CONTRACT_VERSION {
            errs.push(CrawlFieldError::new(
                "crawl.v",
                format!("must be {CRAWL_CONTRACT_VERSION}"),
            ));
        }
        if self.seed_hosts.len() > MAX_SEED_HOSTS {
            errs.push(CrawlFieldError::new(
                "crawl.seed_hosts",
                format!("at most {MAX_SEED_HOSTS} hosts"),
            ));
        }
        for (i, host) in self.seed_hosts.iter().enumerate() {
            if !valid_host_token(host) {
                errs.push(CrawlFieldError::new(
                    format!("crawl.seed_hosts[{i}]"),
                    "must be a bare host name without scheme, path or port",
                ));
            }
        }
        if let Some(seed) = &self.seed_url {
            let ok = Url::parse(seed)
                .map(|u| matches!(u.scheme(), "http" | "https"))
                .unwrap_or(false);
            if !ok {
                errs.push(CrawlFieldError::new(
                    "crawl.seed_url",
                    "must be an absolute http or https URL",
                ));
            }
        }
        self.scope.validate_into("crawl.scope", &mut errs);
        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
    }
}

impl CrawlScope {
    /// Push one field error per rejected scope field, under `base`.
    fn validate_into(&self, base: &str, errs: &mut Vec<CrawlFieldError>) {
        let total_patterns = self.include_paths.len() + self.exclude_paths.len();
        if total_patterns > MAX_PATTERNS {
            errs.push(CrawlFieldError::new(
                format!("{base}.include_paths"),
                format!(
                    "include_paths and exclude_paths together hold at most {MAX_PATTERNS} patterns"
                ),
            ));
        }
        let mut total_chars = 0usize;
        for (list_name, list) in [
            ("include_paths", &self.include_paths),
            ("exclude_paths", &self.exclude_paths),
        ] {
            for (i, pattern) in list.iter().enumerate() {
                let field = format!("{base}.{list_name}[{i}]");
                let chars = pattern.chars().count();
                total_chars += chars;
                if pattern.trim().is_empty() {
                    errs.push(CrawlFieldError::new(field, "must not be empty"));
                } else if chars > MAX_PATTERN_CHARS {
                    errs.push(CrawlFieldError::new(
                        field,
                        format!("at most {MAX_PATTERN_CHARS} characters"),
                    ));
                }
            }
        }
        if total_chars > MAX_PATTERN_TOTAL_CHARS {
            errs.push(CrawlFieldError::new(
                base.to_string(),
                format!("patterns hold at most {MAX_PATTERN_TOTAL_CHARS} characters in total"),
            ));
        }
        if let Some(list) = &self.deny_extensions {
            if list.len() > MAX_EXTENSIONS {
                errs.push(CrawlFieldError::new(
                    format!("{base}.deny_extensions"),
                    format!("at most {MAX_EXTENSIONS} extensions"),
                ));
            }
            for (i, ext) in list.iter().enumerate() {
                let trimmed = ext.trim().trim_start_matches('.');
                let ok = !trimmed.is_empty()
                    && trimmed.len() <= 16
                    && trimmed.chars().all(|c| c.is_ascii_alphanumeric());
                if !ok {
                    errs.push(CrawlFieldError::new(
                        format!("{base}.deny_extensions[{i}]"),
                        "must be an alphanumeric extension of at most 16 characters",
                    ));
                }
            }
        }
    }
}

/// A host token: non-empty, at most 253 bytes, and no characters that would
/// make it a URL part (scheme, path, port, userinfo, query, fragment).
fn valid_host_token(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host
            .chars()
            .any(|c| c.is_whitespace() || "/:@?#\\[]".contains(c))
}

/// One denied link in the report. At most [`MAX_SAMPLES_PER_CODE`] per code
/// and [`MAX_SAMPLES_PER_PAGE`] per page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenialSample {
    pub url: String,
    pub code: String,
    pub message: String,
    /// What an operator can change to get the URL crawled, when there is one.
    pub remedy: Option<String>,
    /// Depth the denied link would have been crawled at.
    pub depth: u32,
}

/// One robots.txt denial. At most [`MAX_ROBOTS_DENIALS_PER_PAGE`] per page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RobotsDenial {
    pub url: String,
    pub code: String,
    pub user_agent: String,
    pub path: String,
}

/// Denial counts by code, with capped samples and robots entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenialCounts {
    /// Code to count. Sorted by code so the wire form is stable.
    pub counts: BTreeMap<String, u32>,
    pub samples: Vec<DenialSample>,
    pub robots: Vec<RobotsDenial>,
}

impl DenialCounts {
    /// Count one denial and keep a sample of it, within the caps. Robots
    /// denials also get a [`RobotsDenial`] entry.
    pub fn record(&mut self, url: &str, depth: u32, reason: &CrawlDenialReason) {
        let code = reason.code();
        *self.counts.entry(code.to_string()).or_insert(0) += 1;
        let same_code = self.samples.iter().filter(|s| s.code == code).count();
        if same_code < MAX_SAMPLES_PER_CODE && self.samples.len() < MAX_SAMPLES_PER_PAGE {
            self.samples.push(DenialSample {
                url: url.to_string(),
                code: code.to_string(),
                message: reason.message(),
                remedy: reason.remedy().map(str::to_string),
                depth,
            });
        }
        let (user_agent, path) = match reason {
            CrawlDenialReason::RobotsDisallowed { user_agent, path } => {
                (user_agent.clone(), path.clone())
            }
            CrawlDenialReason::RobotsUnreachable { .. } => (String::new(), String::new()),
            _ => return,
        };
        if self.robots.len() < MAX_ROBOTS_DENIALS_PER_PAGE {
            self.robots.push(RobotsDenial {
                url: url.to_string(),
                code: code.to_string(),
                user_agent,
                path,
            });
        }
    }

    /// Total denials counted, across every code.
    pub fn total(&self) -> u32 {
        self.counts.values().sum()
    }
}

/// The report returned with a page when its request carried `crawl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrawlReport {
    pub v: u32,
    /// True when the scope changed `links` (enforce mode).
    pub scope_applied: bool,
    pub mode: ScopeMode,
    /// Hrefs on the page, before any evaluation.
    pub links_seen: u32,
    pub links_allowed: u32,
    pub links_denied: u32,
    /// Hrefs past [`MAX_LINKS_EVALUATED`], or every href for a batch page.
    pub links_unevaluated: u32,
    pub denials: DenialCounts,
    /// Hosts this page added to the crawl's scope. Only ever set at depth 0.
    pub hosts_added: Vec<String>,
    /// Crawl delay from robots.txt. Reserved: set once robots peeks supply one.
    pub crawl_delay_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(scope: CrawlScope) -> CrawlContext {
        CrawlContext {
            scope,
            ..CrawlContext::default()
        }
    }

    #[test]
    fn default_scope_is_same_host_whole_domain() {
        let scope = CrawlScope::default();
        assert!(scope.crawl_entire_domain);
        assert!(!scope.allow_subdomains);
        assert!(!scope.allow_external_links);
        assert!(scope.deny_extensions.is_none());
    }

    #[test]
    fn empty_object_takes_every_default() {
        let ctx = CrawlContext::from_json(serde_json::json!({})).unwrap();
        assert_eq!(ctx, CrawlContext::default());
        assert_eq!(ctx.v, CRAWL_CONTRACT_VERSION);
    }

    #[test]
    fn unknown_scope_key_is_refused_with_the_crawl_field() {
        let errs = CrawlContext::from_json(serde_json::json!({
            "scope": { "allow_subdomian": true }
        }))
        .unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].field, "crawl");
        assert!(errs[0].message.contains("allow_subdomian"));
    }

    #[test]
    fn wrong_version_is_refused() {
        let ctx = CrawlContext {
            v: 2,
            ..CrawlContext::default()
        };
        let errs = ctx.validate().unwrap_err();
        assert_eq!(errs[0].field, "crawl.v");
    }

    #[test]
    fn seed_hosts_must_be_bare_host_names() {
        let ctx = CrawlContext {
            seed_hosts: vec![
                "example.com".into(),
                "https://example.com".into(),
                "example.com:8080".into(),
                "".into(),
            ],
            ..CrawlContext::default()
        };
        let errs = ctx.validate().unwrap_err();
        let fields: Vec<&str> = errs.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(
            fields,
            vec![
                "crawl.seed_hosts[1]",
                "crawl.seed_hosts[2]",
                "crawl.seed_hosts[3]"
            ]
        );
    }

    #[test]
    fn seed_url_must_be_absolute_http() {
        let ctx = CrawlContext {
            seed_url: Some("javascript:alert(1)".into()),
            ..CrawlContext::default()
        };
        assert_eq!(ctx.validate().unwrap_err()[0].field, "crawl.seed_url");
    }

    #[test]
    fn pattern_count_cap_is_on_the_two_lists_together() {
        let many: Vec<String> = (0..=MAX_PATTERNS).map(|i| format!("/p{i}")).collect();
        let scope = CrawlScope {
            include_paths: many,
            ..CrawlScope::default()
        };
        let errs = ctx_with(scope).validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "crawl.scope.include_paths"));
    }

    #[test]
    fn pattern_length_cap_names_the_exact_index() {
        let scope = CrawlScope {
            include_paths: vec!["/ok".into(), "x".repeat(MAX_PATTERN_CHARS + 1)],
            exclude_paths: vec!["".into()],
            ..CrawlScope::default()
        };
        let errs = ctx_with(scope).validate().unwrap_err();
        let fields: Vec<&str> = errs.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"crawl.scope.include_paths[1]"));
        assert!(fields.contains(&"crawl.scope.exclude_paths[0]"));
    }

    #[test]
    fn pattern_total_cap_reports_the_scope() {
        let chunk = "y".repeat(MAX_PATTERN_CHARS);
        let scope = CrawlScope {
            include_paths: (0..17).map(|_| chunk.clone()).collect(),
            ..CrawlScope::default()
        };
        let errs = ctx_with(scope).validate().unwrap_err();
        assert!(errs.iter().any(|e| e.field == "crawl.scope"));
    }

    #[test]
    fn extension_entries_are_checked() {
        let scope = CrawlScope {
            deny_extensions: Some(vec![".png".into(), "a/b".into(), "".into()]),
            ..CrawlScope::default()
        };
        let errs = ctx_with(scope).validate().unwrap_err();
        let fields: Vec<&str> = errs.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(
            fields,
            vec![
                "crawl.scope.deny_extensions[1]",
                "crawl.scope.deny_extensions[2]"
            ]
        );
    }

    #[test]
    fn denial_samples_cap_per_code_and_per_page() {
        let mut counts = DenialCounts::default();
        for i in 0..10 {
            counts.record(
                &format!("https://a.test/{i}"),
                1,
                &CrawlDenialReason::IncludePatternMiss,
            );
        }
        for i in 0..10 {
            counts.record(
                &format!("https://b.test/{i}"),
                1,
                &CrawlDenialReason::ExternalHostDisabled {
                    host: format!("h{i}.test"),
                },
            );
        }
        for i in 0..10 {
            counts.record(
                &format!("https://c.test/{i}"),
                1,
                &CrawlDenialReason::ContentTypeRejected {
                    content_type: "png".into(),
                },
            );
        }
        assert_eq!(counts.total(), 30);
        assert_eq!(counts.counts["include_pattern_miss"], 10);
        assert_eq!(counts.samples.len(), MAX_SAMPLES_PER_PAGE);
        let include_samples = counts
            .samples
            .iter()
            .filter(|s| s.code == "include_pattern_miss")
            .count();
        assert_eq!(include_samples, MAX_SAMPLES_PER_CODE);
    }

    #[test]
    fn robots_entries_cap_but_counts_keep_going() {
        let mut counts = DenialCounts::default();
        for i in 0..(MAX_ROBOTS_DENIALS_PER_PAGE + 5) {
            counts.record(
                &format!("https://a.test/{i}"),
                1,
                &CrawlDenialReason::RobotsDisallowed {
                    user_agent: "Quarry".into(),
                    path: "/x".into(),
                },
            );
        }
        assert_eq!(counts.robots.len(), MAX_ROBOTS_DENIALS_PER_PAGE);
        assert_eq!(
            counts.counts["robots_disallowed"],
            (MAX_ROBOTS_DENIALS_PER_PAGE + 5) as u32
        );
    }

    #[test]
    fn robots_unreachable_is_listed_as_a_robots_denial() {
        let mut counts = DenialCounts::default();
        counts.record(
            "https://a.test/x",
            1,
            &CrawlDenialReason::RobotsUnreachable {
                host: "a.test".into(),
            },
        );
        assert_eq!(counts.robots.len(), 1);
        assert_eq!(counts.robots[0].code, "robots_unreachable");
    }
}
