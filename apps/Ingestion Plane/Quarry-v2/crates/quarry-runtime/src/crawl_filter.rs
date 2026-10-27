//! Crawl scope evaluation for one fetched page (WS14.S2).
//!
//! [`CompiledScope::compile`] validates a [`CrawlContext`] and compiles its
//! patterns once. [`CompiledScope::evaluate`] then decides each link of the
//! page in a fixed order and returns the allowed links with a
//! [`CrawlReport`]. Nothing here does I/O. Robots decisions come from a
//! caller-supplied lookup that reads cached rules only.
//!
//! Check order per link: parse, scheme, fragment-only, depth, host scope,
//! section scope, exclude, include, extension, robots, then duplicate against
//! the links already allowed. The first failing check is the denial reason.
//!
//! Pattern grammar (include and exclude lists):
//! - `re:<regex>` matches the URL path; `re_url:<regex>` the full URL.
//! - `url:<pattern>` matches the full URL, otherwise the path.
//! - A plain pattern with `*` or `?` is an anchored glob over the subject.
//! - A plain pattern without either is a prefix of the subject.

use std::collections::HashSet;

use regex::RegexBuilder;
use url::Url;

use quarry_core::crawl_denial::CrawlDenialReason;
use quarry_core::crawl_scope::{
    CrawlContext, CrawlFieldError, CrawlKind, CrawlReport, CrawlScope, DenialCounts, RobotsMode,
    ScopeMode, CRAWL_CONTRACT_VERSION, MAX_LINKS_EVALUATED,
};

use crate::crawl_frontier::{glob_match, normalize_url};

/// Compiled regular expressions may use at most this much memory.
const REGEX_SIZE_LIMIT: usize = 256 * 1024;
/// And at most this much for the lazily built DFA.
const REGEX_DFA_SIZE_LIMIT: usize = 1024 * 1024;
/// Longest href echoed back in a denial sample.
const SAMPLE_URL_CHARS: usize = 2048;

/// Built-in extension denylist, used when `scope.deny_extensions` is absent.
/// Static assets and media only. Documents are parsed by the edge, so they stay
/// followable.
pub const DEFAULT_DENY_EXTENSIONS: &[&str] = &[
    "7z", "avi", "bin", "bmp", "css", "dmg", "eot", "exe", "gif", "gz", "ico", "jpeg", "jpg", "js",
    "mov", "mp3", "mp4", "png", "rar", "svg", "tar", "ttf", "webm", "webp", "woff", "woff2", "zip",
];

/// What the robots cache knows about one URL. `Unknown` means no cached rules
/// were found, and the link is not denied for robots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RobotsPeek {
    Allowed,
    Unknown,
    Disallowed { user_agent: String, path: String },
    Unreachable,
}

/// A cached-rules lookup, borrowed for one evaluation.
pub type RobotsLookup<'a> = &'a dyn Fn(&Url) -> RobotsPeek;

/// The links to return and the report for one page.
#[derive(Debug)]
pub struct FilterOutcome {
    pub links: Vec<String>,
    pub report: CrawlReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Path,
    FullUrl,
}

#[derive(Debug)]
enum Matcher {
    Prefix(String),
    Glob(String),
    Regex(regex::Regex),
}

#[derive(Debug)]
struct Pattern {
    raw: String,
    target: Target,
    matcher: Matcher,
}

impl Pattern {
    fn matches(&self, path: &str, full: &str) -> bool {
        let subject = match self.target {
            Target::Path => path,
            Target::FullUrl => full,
        };
        match &self.matcher {
            Matcher::Prefix(p) => subject.starts_with(p.as_str()),
            Matcher::Glob(g) => glob_match(g, subject),
            Matcher::Regex(re) => re.is_match(subject),
        }
    }
}

fn compile_pattern(raw: &str, field: &str) -> Result<Pattern, CrawlFieldError> {
    let (target, is_regex, body) = if let Some(b) = raw.strip_prefix("re_url:") {
        (Target::FullUrl, true, b)
    } else if let Some(b) = raw.strip_prefix("re:") {
        (Target::Path, true, b)
    } else if let Some(b) = raw.strip_prefix("url:") {
        (Target::FullUrl, false, b)
    } else {
        (Target::Path, false, raw)
    };
    if body.trim().is_empty() {
        return Err(CrawlFieldError::new(
            field,
            "pattern is empty after its prefix",
        ));
    }
    let matcher = if is_regex {
        let re = RegexBuilder::new(body)
            .size_limit(REGEX_SIZE_LIMIT)
            .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
            .build()
            .map_err(|e| {
                CrawlFieldError::new(field, format!("not a valid regular expression: {e}"))
            })?;
        Matcher::Regex(re)
    } else if body.contains('*') || body.contains('?') {
        Matcher::Glob(body.to_string())
    } else {
        Matcher::Prefix(body.to_string())
    };
    Ok(Pattern {
        raw: raw.to_string(),
        target,
        matcher,
    })
}

/// Which hosts and paths the crawl may follow, beyond the host rules.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Section {
    /// Any path on an allowed host.
    Domain,
    /// Paths under this prefix. From `seed_url` when `crawl_entire_domain` is off.
    Prefix(String),
    /// Paths under the directory of the page being run.
    PageDirectory,
}

/// A crawl scope with its patterns compiled.
#[derive(Debug)]
pub struct CompiledScope {
    kind: CrawlKind,
    mode: ScopeMode,
    robots: RobotsMode,
    depth: u32,
    max_depth: u32,
    seed_hosts: Vec<String>,
    scope: CrawlScope,
    section: Section,
    deny_extensions: Vec<String>,
    include: Vec<Pattern>,
    exclude: Vec<Pattern>,
}

/// Outcome of one link.
enum Verdict {
    Allow(String),
    Deny {
        url: String,
        reason: CrawlDenialReason,
    },
}

/// Per-page values the link checks share.
struct EvalCtx<'a> {
    page: &'a Url,
    roots: &'a [String],
    section_prefix: Option<String>,
    link_depth: u32,
    robots: Option<RobotsLookup<'a>>,
}

impl CompiledScope {
    /// Validate the context and compile its patterns. Every error names the
    /// field it came from, so the caller can answer with `details.fields`.
    pub fn compile(ctx: &CrawlContext) -> Result<Self, Vec<CrawlFieldError>> {
        ctx.validate()?;
        let mut errs = Vec::new();
        let mut include = Vec::new();
        for (i, raw) in ctx.scope.include_paths.iter().enumerate() {
            match compile_pattern(raw, &format!("crawl.scope.include_paths[{i}]")) {
                Ok(p) => include.push(p),
                Err(e) => errs.push(e),
            }
        }
        let mut exclude = Vec::new();
        for (i, raw) in ctx.scope.exclude_paths.iter().enumerate() {
            match compile_pattern(raw, &format!("crawl.scope.exclude_paths[{i}]")) {
                Ok(p) => exclude.push(p),
                Err(e) => errs.push(e),
            }
        }
        let section = if ctx.scope.crawl_entire_domain {
            Section::Domain
        } else {
            match ctx.seed_url.as_deref() {
                Some(seed) => match Url::parse(seed) {
                    Ok(u) => Section::Prefix(directory_prefix(u.path())),
                    Err(_) => {
                        errs.push(CrawlFieldError::new(
                            "crawl.seed_url",
                            "must be an absolute http or https URL",
                        ));
                        Section::Domain
                    }
                },
                None => Section::PageDirectory,
            }
        };
        let deny_extensions = match &ctx.scope.deny_extensions {
            Some(list) => list
                .iter()
                .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
                .collect(),
            None => DEFAULT_DENY_EXTENSIONS
                .iter()
                .map(|e| (*e).to_string())
                .collect(),
        };
        if !errs.is_empty() {
            return Err(errs);
        }
        Ok(Self {
            kind: ctx.kind,
            mode: ctx.scope_mode,
            robots: ctx.robots,
            depth: ctx.depth,
            max_depth: ctx.max_depth,
            seed_hosts: ctx.seed_hosts.clone(),
            scope: ctx.scope.clone(),
            section,
            deny_extensions,
            include,
            exclude,
        })
    }

    /// Whether this crawl consults robots.txt (`crawl.robots == respect`). The
    /// edge gates a page's fetch on it (WS14.S4).
    pub fn respects_robots(&self) -> bool {
        self.robots == RobotsMode::Respect
    }

    /// Decide every href of one fetched page. `page_url` is the final URL after
    /// redirects. `robots` is `None` when no cached rules are available.
    pub fn evaluate(
        &self,
        page_url: &Url,
        hrefs: &[String],
        robots: Option<RobotsLookup<'_>>,
    ) -> FilterOutcome {
        // The orchestrator ends a crawl whose report lacks scope_applied (D2-029). Observe
        // evaluates the scope as well; only enforce changes the links.
        let mut report = CrawlReport {
            v: CRAWL_CONTRACT_VERSION,
            scope_applied: true,
            mode: self.mode,
            links_seen: saturating_u32(hrefs.len()),
            links_allowed: 0,
            links_denied: 0,
            links_unevaluated: 0,
            denials: DenialCounts::default(),
            hosts_added: Vec::new(),
            crawl_delay_ms: None,
        };
        if self.kind == CrawlKind::Batch {
            // A batch page's links are never followed, so none are evaluated or returned.
            report.links_unevaluated = report.links_seen;
            return FilterOutcome {
                links: Vec::new(),
                report,
            };
        }

        let (roots, hosts_added) = self.roots_for(page_url);
        report.hosts_added = hosts_added;
        let section_prefix = match &self.section {
            Section::Domain => None,
            Section::Prefix(p) => Some(p.clone()),
            Section::PageDirectory => Some(directory_prefix(page_url.path())),
        };
        let link_depth = self.depth.saturating_add(1);
        let cx = EvalCtx {
            page: page_url,
            roots: &roots,
            section_prefix,
            link_depth,
            robots,
        };

        let mut allowed: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut denied: u32 = 0;
        for (index, href) in hrefs.iter().enumerate() {
            if index >= MAX_LINKS_EVALUATED {
                report.links_unevaluated = saturating_u32(hrefs.len() - index);
                break;
            }
            match self.decide(&cx, href) {
                Verdict::Allow(url) => {
                    if seen.insert(url.clone()) {
                        allowed.push(url);
                    } else {
                        denied += 1;
                        report
                            .denials
                            .record(&url, link_depth, &CrawlDenialReason::Duplicate);
                    }
                }
                Verdict::Deny { url, reason } => {
                    denied += 1;
                    report.denials.record(&url, link_depth, &reason);
                }
            }
        }
        report.links_allowed = saturating_u32(allowed.len());
        report.links_denied = denied;
        let links = match self.mode {
            ScopeMode::Enforce => allowed,
            ScopeMode::Observe => hrefs.to_vec(),
        };
        FilterOutcome { links, report }
    }

    /// The hosts the crawl follows for this page, and the hosts this page
    /// added (depth 0 only). At depth 0 an unknown host (a redirect, say) joins
    /// the scope. Without seed hosts the page's own host is the root.
    fn roots_for(&self, page: &Url) -> (Vec<String>, Vec<String>) {
        let mut roots = self.seed_hosts.clone();
        let mut added = Vec::new();
        if let Some(host) = page.host_str() {
            let known = roots.iter().any(|r| host_matches(r, host, false));
            if !known && self.depth == 0 {
                roots.push(host.to_string());
                added.push(host.to_string());
            }
            if roots.is_empty() {
                roots.push(host.to_string());
            }
        }
        (roots, added)
    }

    fn decide(&self, cx: &EvalCtx<'_>, href: &str) -> Verdict {
        let raw = href.trim();
        let shown = truncate_chars(raw, SAMPLE_URL_CHARS);
        if raw.starts_with('#') {
            return Verdict::Deny {
                url: shown,
                reason: CrawlDenialReason::NotCrawlable {
                    reason: "fragment_only".into(),
                },
            };
        }
        let joined = match cx.page.join(raw) {
            Ok(u) => u,
            Err(_) => {
                return Verdict::Deny {
                    url: shown,
                    reason: CrawlDenialReason::NotCrawlable {
                        reason: "invalid_url".into(),
                    },
                }
            }
        };
        if !matches!(joined.scheme(), "http" | "https") {
            return Verdict::Deny {
                url: shown,
                reason: CrawlDenialReason::NotCrawlable {
                    reason: format!("scheme {}", joined.scheme()),
                },
            };
        }
        let normalized: String = match normalize_url(joined.as_str()) {
            Some(n) => n,
            None => {
                return Verdict::Deny {
                    url: shown,
                    reason: CrawlDenialReason::NotCrawlable {
                        reason: "invalid_url".into(),
                    },
                }
            }
        };
        let link = match Url::parse(&normalized) {
            Ok(u) => u,
            Err(_) => {
                return Verdict::Deny {
                    url: shown,
                    reason: CrawlDenialReason::NotCrawlable {
                        reason: "invalid_url".into(),
                    },
                }
            }
        };
        let deny = |reason: CrawlDenialReason| Verdict::Deny {
            url: normalized.clone(),
            reason,
        };

        if self.max_depth > 0 && cx.link_depth > self.max_depth {
            return deny(CrawlDenialReason::DepthExceeded {
                depth: cx.link_depth,
                limit: self.max_depth,
            });
        }

        let host = match link.host_str() {
            Some(h) => h.to_string(),
            None => {
                return deny(CrawlDenialReason::NotCrawlable {
                    reason: "no_host".into(),
                })
            }
        };
        if !self.scope.allow_external_links
            && !cx.roots.iter().any(|r| host_matches(r, &host, false))
        {
            let related = cx.roots.iter().any(|r| host_matches(r, &host, true));
            if !related {
                return deny(CrawlDenialReason::ExternalHostDisabled { host });
            }
            if !self.scope.allow_subdomains {
                return deny(CrawlDenialReason::OutOfScope {
                    detail: format!("subdomain '{host}' is not followed; allow_subdomains is off"),
                });
            }
        }

        if let Some(prefix) = &cx.section_prefix {
            if !link.path().starts_with(prefix.as_str()) {
                return deny(CrawlDenialReason::OutOfScope {
                    detail: "the link is outside the crawl's section; crawl_entire_domain is off"
                        .into(),
                });
            }
        }

        let path = link.path();
        let full = link.as_str();
        if let Some(p) = self.exclude.iter().find(|p| p.matches(path, full)) {
            return deny(CrawlDenialReason::ExcludePatternHit {
                pattern: p.raw.clone(),
            });
        }
        if !self.include.is_empty() && !self.include.iter().any(|p| p.matches(path, full)) {
            return deny(CrawlDenialReason::IncludePatternMiss);
        }

        if let Some(ext) = extension_of(path) {
            if self.deny_extensions.iter().any(|e| *e == ext) {
                return deny(CrawlDenialReason::ContentTypeRejected { content_type: ext });
            }
        }

        if self.robots == RobotsMode::Respect {
            if let Some(lookup) = cx.robots {
                match lookup(&link) {
                    RobotsPeek::Disallowed { user_agent, path } => {
                        return deny(CrawlDenialReason::RobotsDisallowed { user_agent, path })
                    }
                    RobotsPeek::Unreachable => {
                        return deny(CrawlDenialReason::RobotsUnreachable { host: host.clone() })
                    }
                    RobotsPeek::Allowed | RobotsPeek::Unknown => {}
                }
            }
        }

        Verdict::Allow(normalized)
    }
}

/// True when `candidate` is `root`, or, with `allow_subdomains`, a subdomain of
/// it. A leading `www.` is ignored on either side, so `www.example.com` and
/// `example.com` are the same host. Both sides are canonicalised: lower case,
/// no trailing dot, IDN as punycode. A value with a port, path, query, or
/// userinfo is never a host and never matches.
///
/// Shared with [`crate::crawl_frontier::CrawlFrontier::host_in_scope`] so the
/// frontier and the edge agree on host decisions.
pub fn host_matches(root: &str, candidate: &str, allow_subdomains: bool) -> bool {
    let (Some(root), Some(candidate)) = (canon_host(root), canon_host(candidate)) else {
        return false;
    };
    let root = strip_www(&root);
    let candidate = strip_www(&candidate);
    if candidate == root {
        return true;
    }
    allow_subdomains
        && candidate.len() > root.len() + 1
        && candidate.ends_with(root)
        && candidate.as_bytes()[candidate.len() - root.len() - 1] == b'.'
}

fn canon_host(input: &str) -> Option<String> {
    let trimmed = input.trim().trim_end_matches('.');
    if trimmed.is_empty()
        || trimmed
            .chars()
            .any(|c| c.is_whitespace() || "/@:?#\\[]".contains(c))
    {
        return None;
    }
    let parsed = Url::parse(&format!("https://{trimmed}")).ok()?;
    let host = parsed.host_str()?.trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn strip_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

/// The path up to and including its last `/`.
fn directory_prefix(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..=i].to_string(),
        None => "/".to_string(),
    }
}

/// The lower-cased extension of the last path segment, when it has one.
fn extension_of(path: &str) -> Option<String> {
    let last = path.rsplit('/').next()?;
    let (stem, ext) = last.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn saturating_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crawl_frontier::{CrawlFrontier, FrontierConfig};
    use proptest::prelude::*;
    use quarry_core::crawl_scope::{MAX_PATTERNS, MAX_ROBOTS_DENIALS_PER_PAGE};

    fn base() -> CrawlContext {
        CrawlContext {
            seed_hosts: vec!["example.com".into()],
            ..CrawlContext::default()
        }
    }

    fn run(ctx: CrawlContext, page: &str, hrefs: &[&str]) -> FilterOutcome {
        let scope = CompiledScope::compile(&ctx).expect("context compiles");
        let hrefs: Vec<String> = hrefs.iter().map(|h| h.to_string()).collect();
        scope.evaluate(&Url::parse(page).unwrap(), &hrefs, None)
    }

    fn count(out: &FilterOutcome, code: &str) -> u32 {
        out.report.denials.counts.get(code).copied().unwrap_or(0)
    }

    #[test]
    fn same_host_and_www_are_equivalent_both_ways() {
        let out = run(
            base(),
            "https://example.com/",
            &["/a", "https://www.example.com/b"],
        );
        assert_eq!(out.links.len(), 2);
        let out = run(
            CrawlContext {
                seed_hosts: vec!["www.example.com".into()],
                ..CrawlContext::default()
            },
            "https://www.example.com/",
            &["https://example.com/c"],
        );
        assert_eq!(out.links, vec!["https://example.com/c".to_string()]);
        assert!(host_matches("example.com", "www.example.com", false));
        assert!(host_matches("www.example.com", "example.com", false));
    }

    #[test]
    fn trailing_dot_and_uppercase_do_not_change_the_host() {
        assert!(host_matches("example.com.", "EXAMPLE.com", false));
        let out = run(base(), "https://example.com/", &["https://EXAMPLE.com./x"]);
        assert_eq!(out.links.len(), 1);
    }

    #[test]
    fn idn_host_matches_its_punycode_form() {
        assert!(host_matches(
            "bücher.example",
            "xn--bcher-kva.example",
            false
        ));
        assert!(host_matches(
            "xn--bcher-kva.example",
            "bücher.example",
            false
        ));
    }

    #[test]
    fn userinfo_trick_is_the_real_host() {
        let out = run(
            base(),
            "https://example.com/",
            &["http://example.com@evil.test/"],
        );
        assert!(out.links.is_empty());
        assert_eq!(count(&out, "external_host_disabled"), 1);
        assert!(!host_matches("example.com", "example.com@evil.test", true));
    }

    #[test]
    fn port_does_not_change_the_host() {
        let out = run(
            base(),
            "https://example.com/",
            &["https://example.com:8443/x"],
        );
        assert_eq!(out.links.len(), 1);
        assert!(!host_matches("example.com", "example.com:8443", true));
    }

    #[test]
    fn subdomains_follow_only_when_allowed() {
        let off = run(
            base(),
            "https://example.com/",
            &["https://docs.example.com/"],
        );
        assert!(off.links.is_empty());
        assert_eq!(count(&off, "out_of_scope"), 1);
        let on = run(
            CrawlContext {
                scope: CrawlScope {
                    allow_subdomains: true,
                    ..CrawlScope::default()
                },
                ..base()
            },
            "https://example.com/",
            &["https://docs.example.com/"],
        );
        assert_eq!(on.links.len(), 1);
    }

    #[test]
    fn external_hosts_follow_only_when_allowed() {
        let off = run(base(), "https://example.com/", &["https://other.test/"]);
        assert_eq!(count(&off, "external_host_disabled"), 1);
        let on = run(
            CrawlContext {
                scope: CrawlScope {
                    allow_external_links: true,
                    ..CrawlScope::default()
                },
                ..base()
            },
            "https://example.com/",
            &["https://other.test/"],
        );
        assert_eq!(on.links.len(), 1);
    }

    #[test]
    fn depth_boundary_is_inclusive_and_zero_is_unlimited() {
        let page = "https://example.com/";
        // Page at depth 1 produces links at depth 2; max_depth 2 allows them.
        let at_limit = run(
            CrawlContext {
                depth: 1,
                max_depth: 2,
                ..base()
            },
            page,
            &["/x"],
        );
        assert_eq!(at_limit.links.len(), 1);
        // Page at depth 2 produces links at depth 3; max_depth 2 refuses them.
        let over = run(
            CrawlContext {
                depth: 2,
                max_depth: 2,
                ..base()
            },
            page,
            &["/x"],
        );
        assert_eq!(count(&over, "depth_exceeded"), 1);
        let unlimited = run(
            CrawlContext {
                depth: 40,
                max_depth: 0,
                ..base()
            },
            page,
            &["/x"],
        );
        assert_eq!(unlimited.links.len(), 1);
    }

    #[test]
    fn non_web_schemes_are_not_crawlable() {
        let out = run(
            base(),
            "https://example.com/",
            &[
                "mailto:someone@example.com",
                "javascript:void(0)",
                "tel:+4712345678",
                "data:text/html,hi",
            ],
        );
        assert!(out.links.is_empty());
        assert_eq!(count(&out, "not_crawlable"), 4);
    }

    #[test]
    fn fragment_only_links_are_denied_and_fragments_collapse() {
        let out = run(
            base(),
            "https://example.com/",
            &["#top", "/page#one", "/page", "/page#two"],
        );
        assert_eq!(count(&out, "not_crawlable"), 1);
        assert_eq!(out.links, vec!["https://example.com/page".to_string()]);
        assert_eq!(count(&out, "duplicate"), 2);
    }

    #[test]
    fn default_extension_list_denies_assets_case_insensitively() {
        let out = run(
            base(),
            "https://example.com/",
            &["/logo.PNG", "/a.zip", "/a.html"],
        );
        assert_eq!(count(&out, "content_type_rejected"), 2);
        assert_eq!(out.links, vec!["https://example.com/a.html".to_string()]);
    }

    #[test]
    fn explicit_extension_list_replaces_the_default() {
        let out = run(
            CrawlContext {
                scope: CrawlScope {
                    deny_extensions: Some(vec![".html".into()]),
                    ..CrawlScope::default()
                },
                ..base()
            },
            "https://example.com/",
            &["/a.html", "/logo.png"],
        );
        assert_eq!(out.links, vec!["https://example.com/logo.png".to_string()]);
    }

    #[test]
    fn directory_prefix_limits_links_when_entire_domain_is_off() {
        let out = run(
            CrawlContext {
                seed_url: Some("https://example.com/docs/index.html".into()),
                scope: CrawlScope {
                    crawl_entire_domain: false,
                    ..CrawlScope::default()
                },
                ..base()
            },
            "https://example.com/docs/index.html",
            &["/docs/a", "/blog/x"],
        );
        assert_eq!(out.links, vec!["https://example.com/docs/a".to_string()]);
        assert_eq!(count(&out, "out_of_scope"), 1);
    }

    #[test]
    fn include_pattern_miss_is_denied() {
        let out = run(
            CrawlContext {
                scope: CrawlScope {
                    include_paths: vec!["/docs/*".into()],
                    ..CrawlScope::default()
                },
                ..base()
            },
            "https://example.com/",
            &["/blog", "/docs/x"],
        );
        assert_eq!(count(&out, "include_pattern_miss"), 1);
        assert_eq!(out.links, vec!["https://example.com/docs/x".to_string()]);
    }

    #[test]
    fn exclude_pattern_hit_is_denied() {
        let out = run(
            CrawlContext {
                scope: CrawlScope {
                    exclude_paths: vec!["/admin".into()],
                    ..CrawlScope::default()
                },
                ..base()
            },
            "https://example.com/",
            &["/admin/x", "/public"],
        );
        assert_eq!(count(&out, "exclude_pattern_hit"), 1);
    }

    #[test]
    fn exclude_beats_include() {
        let out = run(
            CrawlContext {
                scope: CrawlScope {
                    include_paths: vec!["/docs/*".into()],
                    exclude_paths: vec!["/docs/private*".into()],
                    ..CrawlScope::default()
                },
                ..base()
            },
            "https://example.com/",
            &["/docs/private/x", "/docs/open"],
        );
        assert_eq!(count(&out, "exclude_pattern_hit"), 1);
        assert_eq!(out.links, vec!["https://example.com/docs/open".to_string()]);
    }

    #[test]
    fn grammar_prefixes_select_the_subject() {
        let out = run(
            CrawlContext {
                scope: CrawlScope {
                    exclude_paths: vec!["re:^/secret".into(), "url:https://example.com/tmp".into()],
                    ..CrawlScope::default()
                },
                ..base()
            },
            "https://example.com/",
            &["/secret/1", "/tmp/2", "/ok"],
        );
        assert_eq!(out.links, vec!["https://example.com/ok".to_string()]);
    }

    #[test]
    fn duplicates_collapse_to_one_allowed_link() {
        let out = run(base(), "https://example.com/", &["/a", "/a", "/a#x"]);
        assert_eq!(out.links, vec!["https://example.com/a".to_string()]);
        assert_eq!(out.report.links_allowed, 1);
        assert_eq!(count(&out, "duplicate"), 2);
    }

    #[test]
    fn robots_denials_are_capped_in_the_list_but_counted_in_full() {
        let scope = CompiledScope::compile(&base()).unwrap();
        let hrefs: Vec<String> = (0..(MAX_ROBOTS_DENIALS_PER_PAGE + 10))
            .map(|i| format!("/r{i}"))
            .collect();
        let peek = |_: &Url| RobotsPeek::Disallowed {
            user_agent: "Quarry".into(),
            path: "/".into(),
        };
        let out = scope.evaluate(
            &Url::parse("https://example.com/").unwrap(),
            &hrefs,
            Some(&peek as RobotsLookup<'_>),
        );
        assert_eq!(out.report.denials.robots.len(), MAX_ROBOTS_DENIALS_PER_PAGE);
        assert_eq!(
            count(&out, "robots_disallowed") as usize,
            MAX_ROBOTS_DENIALS_PER_PAGE + 10
        );
    }

    #[test]
    fn robots_unreachable_denies_the_host_links() {
        let scope = CompiledScope::compile(&base()).unwrap();
        let peek = |_: &Url| RobotsPeek::Unreachable;
        let out = scope.evaluate(
            &Url::parse("https://example.com/").unwrap(),
            &["/a".to_string()],
            Some(&peek as RobotsLookup<'_>),
        );
        assert_eq!(count(&out, "robots_unreachable"), 1);
        assert_eq!(out.report.denials.robots[0].code, "robots_unreachable");
    }

    #[test]
    fn robots_ignore_skips_the_lookup() {
        let scope = CompiledScope::compile(&CrawlContext {
            robots: RobotsMode::Ignore,
            ..base()
        })
        .unwrap();
        let peek = |_: &Url| RobotsPeek::Unreachable;
        let out = scope.evaluate(
            &Url::parse("https://example.com/").unwrap(),
            &["/a".to_string()],
            Some(&peek as RobotsLookup<'_>),
        );
        assert_eq!(out.links.len(), 1);
    }

    #[test]
    fn six_thousand_links_stop_at_the_evaluation_cap() {
        let hrefs: Vec<String> = (0..6000).map(|i| format!("/p{i}")).collect();
        let scope = CompiledScope::compile(&base()).unwrap();
        let out = scope.evaluate(&Url::parse("https://example.com/").unwrap(), &hrefs, None);
        assert_eq!(out.report.links_seen, 6000);
        assert_eq!(out.report.links_unevaluated, 1000);
        assert_eq!(out.report.links_allowed as usize, MAX_LINKS_EVALUATED);
        assert_eq!(out.links.len(), MAX_LINKS_EVALUATED);
    }

    #[test]
    fn invalid_regex_is_a_field_error_with_its_index() {
        let errs = CompiledScope::compile(&CrawlContext {
            scope: CrawlScope {
                include_paths: vec!["/ok".into(), "re:(unclosed".into()],
                ..CrawlScope::default()
            },
            ..base()
        })
        .unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].field, "crawl.scope.include_paths[1]");
    }

    #[test]
    fn oversize_regex_is_a_field_error_not_a_panic() {
        // Whether this exceeds the size limit depends on the regex engine. The
        // contract is that it either compiles or is refused by field, never a panic.
        let pattern = format!("re:{}", "(\\p{L}{500}){50}");
        match CompiledScope::compile(&CrawlContext {
            scope: CrawlScope {
                include_paths: vec![pattern],
                ..CrawlScope::default()
            },
            ..base()
        }) {
            Ok(_) => {}
            Err(errs) => assert_eq!(errs[0].field, "crawl.scope.include_paths[0]"),
        }
    }

    #[test]
    fn catastrophic_pattern_completes_quickly() {
        let scope = CompiledScope::compile(&CrawlContext {
            scope: CrawlScope {
                include_paths: vec!["re:(a+)+$".into()],
                ..CrawlScope::default()
            },
            ..base()
        })
        .unwrap();
        let long = format!("/{}!", "a".repeat(10_000));
        let started = std::time::Instant::now();
        let _ = run_with(&scope, "https://example.com/", &[long.as_str()]);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(250),
            "regex evaluation took {:?}",
            started.elapsed()
        );
    }

    fn run_with(scope: &CompiledScope, page: &str, hrefs: &[&str]) -> FilterOutcome {
        let hrefs: Vec<String> = hrefs.iter().map(|h| h.to_string()).collect();
        scope.evaluate(&Url::parse(page).unwrap(), &hrefs, None)
    }

    #[test]
    fn redirect_host_joins_scope_only_at_depth_zero() {
        let at_seed = run(base(), "https://other.test/landing", &["/inside"]);
        assert_eq!(at_seed.report.hosts_added, vec!["other.test".to_string()]);
        assert_eq!(at_seed.links, vec!["https://other.test/inside".to_string()]);
        let deeper = run(
            CrawlContext { depth: 1, ..base() },
            "https://other.test/landing",
            &["/inside"],
        );
        assert!(deeper.report.hosts_added.is_empty());
        assert!(deeper.links.is_empty());
    }

    #[test]
    fn observe_mode_returns_every_href_and_reports_the_denials() {
        let out = run(
            CrawlContext {
                scope_mode: ScopeMode::Observe,
                ..base()
            },
            "https://example.com/",
            &["/a", "https://other.test/"],
        );
        assert_eq!(
            out.links,
            vec!["/a".to_string(), "https://other.test/".to_string()]
        );
        assert!(
            out.report.scope_applied,
            "observe is evaluated, so the orchestrator keeps the crawl"
        );
        assert_eq!(count(&out, "external_host_disabled"), 1);
    }

    #[test]
    fn observe_reports_scope_applied_on_every_page_kind() {
        let observe = CrawlContext {
            scope_mode: ScopeMode::Observe,
            ..base()
        };
        let crawled = run(observe.clone(), "https://example.com/", &["/a"]);
        assert!(crawled.report.scope_applied, "the orchestrator needs the proof (D2-029)");
        assert_eq!(crawled.links, vec!["/a".to_string()]);
        let batch = run(
            CrawlContext {
                kind: CrawlKind::Batch,
                ..observe
            },
            "https://example.com/",
            &["/a"],
        );
        assert!(batch.report.scope_applied);
        assert!(batch.links.is_empty());
        let enforced = run(
            CrawlContext {
                scope_mode: ScopeMode::Enforce,
                ..base()
            },
            "https://example.com/",
            &["/a"],
        );
        assert!(enforced.report.scope_applied);
    }

    #[test]
    fn batch_pages_return_no_links() {
        let out = run(
            CrawlContext {
                kind: CrawlKind::Batch,
                ..base()
            },
            "https://example.com/",
            &["/a", "/b"],
        );
        assert!(out.links.is_empty());
        assert_eq!(out.report.links_unevaluated, 2);
    }

    #[test]
    fn oversized_pattern_list_is_refused_before_compiling() {
        let errs = CompiledScope::compile(&CrawlContext {
            scope: CrawlScope {
                include_paths: (0..=MAX_PATTERNS).map(|i| format!("/p{i}")).collect(),
                ..CrawlScope::default()
            },
            ..base()
        })
        .unwrap_err();
        assert!(errs.iter().any(|e| e.field == "crawl.scope.include_paths"));
    }

    #[test]
    fn host_matches_refuses_non_hosts() {
        assert!(!host_matches("", "example.com", true));
        assert!(!host_matches("example.com", "", true));
        assert!(!host_matches("example.com", "evil-example.com", true));
        assert!(!host_matches("example.com", "example.com.evil.test", true));
        assert!(host_matches("example.com", "a.example.com", true));
        assert!(!host_matches("example.com", "a.example.com", false));
    }

    proptest! {
        /// The frontier and the edge filter must agree on host decisions: both
        /// call `host_matches(root, host, true)`.
        #[test]
        fn frontier_enqueue_and_host_matches_agree(
            root in prop::sample::select(vec!["example.com", "www.example.com", "bücher.example"]),
            host in prop::sample::select(vec![
                "example.com",
                "www.example.com",
                "a.example.com",
                "www.www.example.com",
                "evil-example.com",
                "example.com.evil.test",
                "other.test",
                "xn--bcher-kva.example",
                "bücher.example",
            ]),
        ) {
            let mut frontier = CrawlFrontier::new(FrontierConfig {
                seed_hosts: vec![root.to_string()],
                ..FrontierConfig::default()
            });
            let accepted = frontier.enqueue(&format!("https://{host}/page"), 1).is_ok();
            prop_assert_eq!(accepted, host_matches(root, host, true));
        }
    }
}
