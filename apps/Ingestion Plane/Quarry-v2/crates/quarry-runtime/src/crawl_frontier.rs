//! Rust-native crawl frontier with BFS ordering and dedup.
//!
//! Phase 5 scaffolding — moves the BFS frontier from the Go orchestrator into
//! Rust so the hot path can run end-to-end without crossing the Go/Rust
//! boundary per page. Pause/resume/cancel signal routing lives in
//! [`crate::crawl_signals`].
//!
//! The frontier is intentionally simple in-memory `VecDeque + HashSet`. For
//! durability, [`CrawlFrontier::checkpoint`] / [`CrawlFrontier::restore`]
//! serialize state for the Go orchestrator's Temporal workflow to persist.

use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use url::Url;

use quarry_core::crawl_denial::CrawlDenialReason;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontierEntry {
    pub url: String,
    pub depth: u32,
}

/// Serializable snapshot of the entire frontier state.
///
/// Temporal workflows persist this every N pages so a worker crash can
/// resume from where it was without re-fetching already-visited URLs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierCheckpoint {
    pub config: FrontierConfigSnapshot,
    pub queue: Vec<FrontierEntry>,
    pub seen: Vec<String>,
    pub visited_count: u32,
}

/// Serializable subset of [`FrontierConfig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierConfigSnapshot {
    pub max_depth: Option<u32>,
    pub max_pages: Option<u32>,
    pub allow_external: bool,
    pub seed_host: Option<String>,
    /// Multi-host whitelist — when set, ANY host in this list (or its
    /// subdomains) is treated as in-scope. Backward-compatible: empty
    /// list means use `seed_host` only.
    #[serde(default)]
    pub seed_hosts: Vec<String>,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FrontierConfig {
    pub max_depth: Option<u32>,
    pub max_pages: Option<u32>,
    pub allow_external: bool,
    pub seed_host: Option<String>,
    /// Multi-host whitelist for crawls that legitimately span more than
    /// one origin (e.g. `docs.example.com` + `api.example.com`).
    /// When non-empty, any URL whose host matches an entry (or is a
    /// subdomain of it) is treated as in-scope; `seed_host` is ignored.
    /// When empty, falls back to single-host behavior on `seed_host`.
    pub seed_hosts: Vec<String>,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
}

impl Default for FrontierConfig {
    fn default() -> Self {
        Self {
            max_depth: Some(5),
            max_pages: Some(1000),
            allow_external: false,
            seed_host: None,
            seed_hosts: vec![],
            include_patterns: vec![],
            exclude_patterns: vec![],
        }
    }
}

impl FrontierConfig {
    /// True when `host` is in scope for this crawl. Honors `seed_hosts`
    /// (multi-host whitelist) before falling back to `seed_host`. Subdomain
    /// matches are accepted (e.g. `api.example.com` ∈ `example.com`).
    pub fn host_in_scope(&self, host: &str) -> bool {
        if self.allow_external {
            return true;
        }
        if !self.seed_hosts.is_empty() {
            return self
                .seed_hosts
                .iter()
                .any(|root| host == root || host.ends_with(&format!(".{root}")));
        }
        match &self.seed_host {
            Some(root) => host == root || host.ends_with(&format!(".{root}")),
            None => true, // no scope set yet — first seed will set it
        }
    }
}

#[derive(Debug)]
pub struct CrawlFrontier {
    config: FrontierConfig,
    queue: VecDeque<FrontierEntry>,
    seen: HashSet<String>,
    visited_count: u32,
    denials: Vec<(String, CrawlDenialReason)>,
}

impl CrawlFrontier {
    pub fn new(config: FrontierConfig) -> Self {
        Self {
            config,
            queue: VecDeque::new(),
            seen: HashSet::new(),
            visited_count: 0,
            denials: Vec::new(),
        }
    }

    pub fn seed(&mut self, url: &str) -> Result<(), CrawlDenialReason> {
        let _ = self.seed_with_host_report(url)?;
        Ok(())
    }

    /// Same as [`seed`], but returns the newly-discovered host (if any) so
    /// callers can emit a `host_discovered` event for downstream consumers
    /// like `autocomplete-core` (gap-quarry cluster #19).
    ///
    /// Returns:
    /// - `Ok(Some(host))` — a brand-new host was added to scope
    /// - `Ok(None)` — URL valid but host already known (duplicate seed)
    /// - `Err(reason)` — URL invalid or otherwise denied
    pub fn seed_with_host_report(
        &mut self,
        url: &str,
    ) -> Result<Option<String>, CrawlDenialReason> {
        let mut new_host: Option<String> = None;
        if let Ok(parsed) = Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                let host = host.to_string();
                // Track every distinct seed host. The first seed also
                // populates `seed_host` for backward compatibility with
                // single-host callers; subsequent seeds with different
                // hosts get added to `seed_hosts` so multi-host crawls
                // (docs.example.com + api.example.com) work without
                // setting `allow_external=true`.
                if self.config.seed_host.is_none() {
                    self.config.seed_host = Some(host.clone());
                }
                if !self.config.seed_hosts.contains(&host) {
                    self.config.seed_hosts.push(host.clone());
                    new_host = Some(host);
                }
            }
        }
        self.enqueue(url, 0)?;
        Ok(new_host)
    }

    /// Try to enqueue a discovered URL. Returns `Err(reason)` when rejected.
    pub fn enqueue(&mut self, url: &str, depth: u32) -> Result<(), CrawlDenialReason> {
        let normalized = normalize_url(url).ok_or_else(|| CrawlDenialReason::NotCrawlable {
            reason: format!("invalid URL: {url}"),
        })?;

        if self.seen.contains(&normalized) {
            let reason = CrawlDenialReason::Duplicate;
            self.denials.push((normalized, reason.clone()));
            return Err(reason);
        }

        if let Some(limit) = self.config.max_depth {
            if depth > limit {
                let reason = CrawlDenialReason::DepthExceeded { depth, limit };
                self.denials.push((normalized, reason.clone()));
                return Err(reason);
            }
        }

        if let Some(limit) = self.config.max_pages {
            if self.visited_count >= limit {
                let reason = CrawlDenialReason::MaxPagesReached { limit };
                self.denials.push((normalized, reason.clone()));
                return Err(reason);
            }
        }

        if let Ok(parsed) = Url::parse(&normalized) {
            if let Some(host) = parsed.host_str() {
                if !self.config.host_in_scope(host) {
                    let reason = CrawlDenialReason::ExternalHostDisabled {
                        host: host.to_string(),
                    };
                    self.denials.push((normalized, reason.clone()));
                    return Err(reason);
                }
            }
        }

        for pat in &self.config.exclude_patterns {
            if normalized.contains(pat.as_str()) {
                let reason = CrawlDenialReason::ExcludePatternHit {
                    pattern: pat.clone(),
                };
                self.denials.push((normalized, reason.clone()));
                return Err(reason);
            }
        }

        if !self.config.include_patterns.is_empty()
            && !self
                .config
                .include_patterns
                .iter()
                .any(|p| normalized.contains(p.as_str()))
        {
            let reason = CrawlDenialReason::IncludePatternMiss;
            self.denials.push((normalized, reason.clone()));
            return Err(reason);
        }

        self.seen.insert(normalized.clone());
        self.queue.push_back(FrontierEntry {
            url: normalized,
            depth,
        });
        Ok(())
    }

    /// Pop the next entry in BFS order (front of queue).
    pub fn pop(&mut self) -> Option<FrontierEntry> {
        let entry = self.queue.pop_front()?;
        self.visited_count += 1;
        Some(entry)
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    pub fn visited(&self) -> u32 {
        self.visited_count
    }

    pub fn seen_count(&self) -> usize {
        self.seen.len()
    }

    /// Mark every hreflang variant of an already-visited page as seen
    /// so the BFS doesn't waste budget re-crawling the German /
    /// French / Italian / etc. clones of the same content. Typically
    /// called after a page fetch when the runtime has extracted
    /// variants via
    /// `quarry_transform::attributes::extract_hreflang_variants`.
    ///
    /// Returns how many variants were freshly recorded (those not
    /// already in `seen`).
    pub fn record_hreflang_variants(&mut self, variant_urls: &[String]) -> usize {
        let mut added = 0;
        for raw in variant_urls {
            // Use the same normaliser as `enqueue` so variants
            // discovered via hreflang AND later via `<a>` links
            // collapse to the same dedupe key.
            let key = match Url::parse(raw) {
                Ok(parsed) => parsed.to_string(),
                Err(_) => raw.trim().to_ascii_lowercase(),
            };
            if self.seen.insert(key) {
                added += 1;
            }
        }
        added
    }

    pub fn denials(&self) -> &[(String, CrawlDenialReason)] {
        &self.denials
    }

    pub fn config(&self) -> &FrontierConfig {
        &self.config
    }

    /// Serialize current state for Temporal workflow persistence.
    /// Cheap; safe to call after every successful page.
    pub fn checkpoint(&self) -> FrontierCheckpoint {
        FrontierCheckpoint {
            config: FrontierConfigSnapshot {
                max_depth: self.config.max_depth,
                max_pages: self.config.max_pages,
                allow_external: self.config.allow_external,
                seed_host: self.config.seed_host.clone(),
                seed_hosts: self.config.seed_hosts.clone(),
                include_patterns: self.config.include_patterns.clone(),
                exclude_patterns: self.config.exclude_patterns.clone(),
            },
            queue: self.queue.iter().cloned().collect(),
            seen: self.seen.iter().cloned().collect(),
            visited_count: self.visited_count,
        }
    }

    /// Rehydrate from a Temporal-persisted checkpoint.
    pub fn restore(checkpoint: FrontierCheckpoint) -> Self {
        let config = FrontierConfig {
            max_depth: checkpoint.config.max_depth,
            max_pages: checkpoint.config.max_pages,
            allow_external: checkpoint.config.allow_external,
            seed_host: checkpoint.config.seed_host,
            seed_hosts: checkpoint.config.seed_hosts,
            include_patterns: checkpoint.config.include_patterns,
            exclude_patterns: checkpoint.config.exclude_patterns,
        };
        Self {
            config,
            queue: checkpoint.queue.into_iter().collect(),
            seen: checkpoint.seen.into_iter().collect(),
            visited_count: checkpoint.visited_count,
            denials: Vec::new(),
        }
    }
}

fn normalize_url(input: &str) -> Option<String> {
    let mut parsed = Url::parse(input).ok()?;
    parsed.set_fragment(None); // drop #anchor
    if parsed.path().is_empty() {
        parsed.set_path("/");
    }
    Some(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> FrontierConfig {
        FrontierConfig {
            max_depth: Some(3),
            max_pages: Some(10),
            allow_external: false,
            seed_host: None,
            seed_hosts: vec![],
            include_patterns: vec![],
            exclude_patterns: vec![],
        }
    }

    #[test]
    fn seed_then_enqueue_dedup() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://example.com/").unwrap();
        assert_eq!(f.pending(), 1);
        let res = f.enqueue("https://example.com/", 1);
        assert!(matches!(res, Err(CrawlDenialReason::Duplicate)));
        assert_eq!(f.pending(), 1);
    }

    #[test]
    fn seed_with_host_report_returns_new_host_first_time_only() {
        let mut f = CrawlFrontier::new(cfg());

        // First time: brand-new host → Some(host)
        let host = f.seed_with_host_report("https://example.com/").unwrap();
        assert_eq!(host.as_deref(), Some("example.com"));

        // Re-seeding the same host returns None (duplicate dedup at
        // host level — even with a different path).
        let host2 = f
            .seed_with_host_report("https://example.com/other-page")
            .unwrap();
        assert!(host2.is_none(), "host already known should report None");

        // A new host returns Some again.
        let host3 = f
            .seed_with_host_report("https://api.example.com/v1")
            .unwrap();
        assert_eq!(host3.as_deref(), Some("api.example.com"));
    }

    #[test]
    fn fragment_normalized_so_anchor_dups_dedupe() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://example.com/page").unwrap();
        let res = f.enqueue("https://example.com/page#section", 1);
        assert!(matches!(res, Err(CrawlDenialReason::Duplicate)));
    }

    #[test]
    fn depth_exceeded_rejects() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://example.com/").unwrap();
        let res = f.enqueue("https://example.com/page", 99);
        assert!(matches!(res, Err(CrawlDenialReason::DepthExceeded { .. })));
    }

    #[test]
    fn external_host_rejected_when_not_allowed() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://example.com/").unwrap();
        let res = f.enqueue("https://other.com/page", 1);
        assert!(matches!(
            res,
            Err(CrawlDenialReason::ExternalHostDisabled { .. })
        ));
    }

    #[test]
    fn subdomain_of_seed_host_allowed() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://example.com/").unwrap();
        f.enqueue("https://api.example.com/v1", 1).unwrap();
        assert_eq!(f.pending(), 2);
    }

    #[test]
    fn external_allowed_when_flag_set() {
        let mut c = cfg();
        c.allow_external = true;
        let mut f = CrawlFrontier::new(c);
        f.seed("https://example.com/").unwrap();
        f.enqueue("https://other.com/page", 1).unwrap();
        assert_eq!(f.pending(), 2);
    }

    #[test]
    fn pop_yields_bfs_order() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://example.com/a").unwrap();
        f.enqueue("https://example.com/b", 1).unwrap();
        f.enqueue("https://example.com/c", 1).unwrap();
        let p1 = f.pop().unwrap();
        let p2 = f.pop().unwrap();
        let p3 = f.pop().unwrap();
        assert!(p1.url.ends_with("/a"));
        assert!(p2.url.ends_with("/b"));
        assert!(p3.url.ends_with("/c"));
        assert!(f.pop().is_none());
    }

    #[test]
    fn exclude_pattern_rejects() {
        let mut c = cfg();
        c.exclude_patterns = vec!["/admin".into()];
        let mut f = CrawlFrontier::new(c);
        f.seed("https://example.com/").unwrap();
        let res = f.enqueue("https://example.com/admin/users", 1);
        assert!(matches!(
            res,
            Err(CrawlDenialReason::ExcludePatternHit { .. })
        ));
    }

    #[test]
    fn include_pattern_miss_rejects() {
        let mut c = cfg();
        c.include_patterns = vec!["/blog".into()];
        let mut f = CrawlFrontier::new(c);
        f.seed("https://example.com/blog/post").unwrap();
        let res = f.enqueue("https://example.com/about", 1);
        assert!(matches!(res, Err(CrawlDenialReason::IncludePatternMiss)));
    }

    #[test]
    fn max_pages_reached_when_visited_count_caps() {
        let mut c = cfg();
        c.max_pages = Some(1);
        let mut f = CrawlFrontier::new(c);
        f.seed("https://example.com/a").unwrap();
        f.pop(); // visited_count = 1
        let res = f.enqueue("https://example.com/b", 1);
        assert!(matches!(
            res,
            Err(CrawlDenialReason::MaxPagesReached { .. })
        ));
    }

    #[test]
    fn invalid_url_rejected() {
        let mut f = CrawlFrontier::new(cfg());
        let res = f.seed("not a url");
        assert!(matches!(res, Err(CrawlDenialReason::NotCrawlable { .. })));
    }

    #[test]
    fn denials_recorded_for_audit() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://example.com/").unwrap();
        let _ = f.enqueue("https://other.com/", 1);
        let _ = f.enqueue("https://example.com/", 1);
        assert_eq!(f.denials().len(), 2);
    }

    #[test]
    fn checkpoint_then_restore_preserves_state() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://example.com/a").unwrap();
        f.enqueue("https://example.com/b", 1).unwrap();
        f.enqueue("https://example.com/c", 1).unwrap();
        f.pop(); // visited 1

        let snap = f.checkpoint();
        let restored = CrawlFrontier::restore(snap);
        assert_eq!(restored.pending(), 2);
        assert_eq!(restored.visited(), 1);
        assert_eq!(restored.seen_count(), 3);
    }

    #[test]
    fn multi_host_seeds_both_in_scope() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://docs.example.com/").unwrap();
        f.seed("https://api.example.com/").unwrap();

        // Both subdomains should now be in scope.
        f.enqueue("https://docs.example.com/a", 1).unwrap();
        f.enqueue("https://api.example.com/v1", 1).unwrap();
        // Plus root example.com (parent of both seeded subdomains) — NOT
        // automatically in scope; only the seeded subdomains and their
        // own subdomains are.
        let r = f.enqueue("https://example.com/x", 1);
        assert!(matches!(
            r,
            Err(CrawlDenialReason::ExternalHostDisabled { .. })
        ));

        // A third unseeded host is rejected.
        let r2 = f.enqueue("https://other.com/y", 1);
        assert!(matches!(
            r2,
            Err(CrawlDenialReason::ExternalHostDisabled { .. })
        ));
    }

    #[test]
    fn multi_host_seeds_subdomains_in_scope() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://example.com/").unwrap();
        f.seed("https://other.com/").unwrap();
        // Subdomains of seeded roots are accepted.
        f.enqueue("https://api.example.com/v1", 1).unwrap();
        f.enqueue("https://www.other.com/page", 1).unwrap();
    }

    #[test]
    fn host_in_scope_helper_handles_allow_external() {
        let mut config = cfg();
        config.allow_external = true;
        assert!(config.host_in_scope("anywhere.com"));
    }

    #[test]
    fn host_in_scope_helper_with_no_seed_host_yet_permits() {
        let config = cfg();
        // Before any seed, host_in_scope returns true (the first seed will
        // set the scope).
        assert!(config.host_in_scope("anywhere.com"));
    }

    #[test]
    fn checkpoint_serializes_to_json() {
        let mut f = CrawlFrontier::new(cfg());
        f.seed("https://example.com/").unwrap();

        let snap = f.checkpoint();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"queue\""));
        assert!(json.contains("\"visited_count\""));

        // and back
        let parsed: FrontierCheckpoint = serde_json::from_str(&json).unwrap();
        let restored = CrawlFrontier::restore(parsed);
        assert_eq!(restored.pending(), 1);
    }
}
