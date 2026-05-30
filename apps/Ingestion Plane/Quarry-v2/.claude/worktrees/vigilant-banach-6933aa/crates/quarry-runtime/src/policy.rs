//! Cycle 21 / cluster #2 — Determinism guarantees + RunPolicy bag.
//!
//! `RunPolicy` is the immutable contract the runtime promises a scrape
//! against: which DOM-cleanup version ran, which markdown converter,
//! which fingerprint algorithm, which retry classifier, whether robots
//! was respected, what counts as a "block" for the host scheduler,
//! and how often we checkpoint the frontier.
//!
//! Two policies that hash to the same `policy_fingerprint` are
//! semantically equivalent: a Strict run with two identical
//! `RunPolicy` values is required to produce identical
//! `NormalizedOutput.fingerprint` across repeated invocations.
//!
//! ## Why this matters
//!
//! Enterprise pipelines need reproducibility for audit and diff. A
//! cached scrape that swaps DOM-cleanup heuristics silently is worse
//! than a fresh fetch — the fingerprint hasn't changed but the
//! content has. By stamping the policy hash into artifact meta, the
//! downstream consumer can detect "same URL + same content but
//! different policy" and decide whether to trust the cache.

use blake3::Hasher;
use serde::{Deserialize, Serialize};

/// Determinism mode the caller asked for.
///
/// - `Strict` — every input that can change output must be pinned;
///   identical inputs MUST produce identical fingerprints across runs.
///   Failure to honour pins is a hard error.
/// - `BestEffort` — the runtime pins what it can but allows minor
///   non-determinism (e.g., timing-dependent JS execution order).
///   Default for everyday scrapes.
/// - `Off` — no pins; output is whatever the driver currently produces.
///   Cheapest; used for one-shot exploration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Determinism {
    Strict,
    #[default]
    BestEffort,
    Off,
}

impl Determinism {
    pub fn is_strict(self) -> bool {
        matches!(self, Determinism::Strict)
    }
}

// =============================================================================
// Sub-policies
// =============================================================================

/// How the runtime decides which URLs to enqueue from a discovery pass
/// (sitemap / RSS / link-walk). Pinned so a "strict" crawl visits
/// exactly the same URL set when re-run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryPolicy {
    /// Respect `robots.txt`. `false` only for explicitly-permissioned
    /// internal corpora.
    pub respect_robots: bool,
    /// Honour `<meta name="robots" content="noindex">`.
    pub respect_meta_robots: bool,
    /// Whether the seed host's subdomains are in-scope.
    pub include_subdomains: bool,
    /// Whether sitemap.xml is consulted before walking links.
    pub use_sitemap: bool,
}

impl Default for DiscoveryPolicy {
    fn default() -> Self {
        Self {
            respect_robots: true,
            respect_meta_robots: true,
            include_subdomains: false,
            use_sitemap: true,
        }
    }
}

/// Fetch-layer pins: which UA, which TLS profile, which timeout.
/// Pinned for reproducibility — same UA + same TLS = same WAF
/// response on most defended sites.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchPolicy {
    pub user_agent: String,
    /// One of `static`, `tls`, `browserless`, `kernel`. Pins which
    /// driver impl is in scope; the runtime asserts this matches the
    /// resolved driver before fetching in Strict mode.
    pub driver_kind: String,
    pub timeout_s: u64,
    /// Custom headers in canonical order. Vec<(name,value)> with
    /// stable sort so the policy hash is deterministic.
    pub headers: Vec<(String, String)>,
}

impl Default for FetchPolicy {
    fn default() -> Self {
        Self {
            user_agent: "Quarry/2.0".into(),
            driver_kind: "static".into(),
            timeout_s: 30,
            headers: Vec::new(),
        }
    }
}

/// Pinned retry classifier. Two retry policies with the same
/// `transient_codes` list will treat the same error sequence
/// identically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Max attempts including the first. `1` disables retries.
    pub max_attempts: u32,
    /// Base delay before the first retry, in ms. Successive retries
    /// use exponential backoff with jitter; the algorithm is
    /// `crate::retry::backoff_delay`.
    pub base_delay_ms: u64,
    /// HTTP status codes treated as transient (retry-eligible).
    /// Sorted ascending for stable hashing.
    pub transient_codes: Vec<u16>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 500,
            transient_codes: vec![408, 429, 500, 502, 503, 504],
        }
    }
}

/// Block-classification pins — what counts as "the host blocked us".
/// Drives the [`crate::host_scheduler::HostScheduler`] response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockPolicy {
    /// HTTP status codes treated as blocks (not transient errors).
    pub block_codes: Vec<u16>,
    /// Substrings in response body that signal a CDN-level block
    /// (e.g. `cf-chl-bypass`, `Just a moment`). Lower-cased.
    pub block_body_markers: Vec<String>,
}

impl Default for BlockPolicy {
    fn default() -> Self {
        Self {
            block_codes: vec![401, 403, 451, 999],
            block_body_markers: vec![
                "cf-chl-bypass".into(),
                "just a moment".into(),
                "access denied".into(),
            ],
        }
    }
}

/// Pinned DOM-cleanup + markdown-conversion pipeline versions. These
/// are bumped when the implementation changes in a way that could
/// alter output bytes — even cosmetically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionPolicy {
    /// Bumped when DOM-cleanup heuristics change. Hard-coded to the
    /// shipped version; callers do NOT set this directly.
    pub dom_cleanup_version: String,
    /// Bumped when the markdown converter changes.
    pub markdown_converter_version: String,
    /// Hash algorithm for `NormalizedOutput.fingerprint`. Always
    /// `blake3` today but pinned so a future swap is observable.
    pub fingerprint_algorithm: String,
    /// Whether to emit per-field source traces (`source_trace`).
    pub emit_source_trace: bool,
}

impl Default for ExtractionPolicy {
    fn default() -> Self {
        Self {
            dom_cleanup_version: "v2".into(),
            markdown_converter_version: "html2md-0.2".into(),
            fingerprint_algorithm: "blake3".into(),
            emit_source_trace: true,
        }
    }
}

/// Checkpoint persistence cadence. Cycle 20 added the durable
/// checkpoint store; this policy controls how often it's invoked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointPolicy {
    /// Save a frontier checkpoint every N pages. `0` disables.
    pub every_n_pages: u32,
    /// Save a checkpoint at least every N seconds even if N pages
    /// haven't been processed. `0` disables.
    pub min_interval_s: u64,
}

impl Default for CheckpointPolicy {
    fn default() -> Self {
        Self {
            every_n_pages: 25,
            min_interval_s: 60,
        }
    }
}

// =============================================================================
// RunPolicy
// =============================================================================

/// The complete policy bag a run runs under. Two runs with the same
/// `RunPolicy` AND the same input URL MUST produce identical
/// `NormalizedOutput.fingerprint` when `determinism == Strict`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunPolicy {
    pub determinism: Determinism,
    pub discovery: DiscoveryPolicy,
    pub fetch: FetchPolicy,
    pub retry: RetryPolicy,
    pub block: BlockPolicy,
    pub extraction: ExtractionPolicy,
    pub checkpoint: CheckpointPolicy,
}

impl Default for RunPolicy {
    /// `best_effort` — the everyday scrape preset.
    fn default() -> Self {
        Self::best_effort()
    }
}

impl RunPolicy {
    /// All pins set to the most-deterministic values. Use for
    /// reproducibility-critical workloads (compliance archives,
    /// regression baselines).
    pub fn strict() -> Self {
        Self {
            determinism: Determinism::Strict,
            discovery: DiscoveryPolicy {
                respect_robots: true,
                respect_meta_robots: true,
                include_subdomains: false,
                use_sitemap: true,
            },
            fetch: FetchPolicy::default(),
            retry: RetryPolicy {
                // Strict mode disables retries by default — repeated
                // attempts can mask race conditions that produce
                // non-identical output.
                max_attempts: 1,
                base_delay_ms: 0,
                transient_codes: vec![],
            },
            block: BlockPolicy::default(),
            extraction: ExtractionPolicy::default(),
            checkpoint: CheckpointPolicy::default(),
        }
    }

    /// Default everyday preset.
    pub fn best_effort() -> Self {
        Self {
            determinism: Determinism::BestEffort,
            discovery: DiscoveryPolicy::default(),
            fetch: FetchPolicy::default(),
            retry: RetryPolicy::default(),
            block: BlockPolicy::default(),
            extraction: ExtractionPolicy::default(),
            checkpoint: CheckpointPolicy::default(),
        }
    }

    /// No pins, no retries, no robots respect. Exploratory only —
    /// the runtime stamps `Determinism::Off` on the output so
    /// consumers can refuse to ingest the result into durable stores.
    pub fn off() -> Self {
        Self {
            determinism: Determinism::Off,
            discovery: DiscoveryPolicy {
                respect_robots: false,
                respect_meta_robots: false,
                include_subdomains: false,
                use_sitemap: false,
            },
            fetch: FetchPolicy::default(),
            retry: RetryPolicy {
                max_attempts: 1,
                base_delay_ms: 0,
                transient_codes: vec![],
            },
            block: BlockPolicy::default(),
            extraction: ExtractionPolicy::default(),
            checkpoint: CheckpointPolicy {
                every_n_pages: 0,
                min_interval_s: 0,
            },
        }
    }
}

// =============================================================================
// Fingerprint + identity
// =============================================================================

/// Hex blake3 of canonical-JSON of the policy. Two equal policies
/// always produce the same fingerprint; stamping it into artifact
/// metadata lets consumers detect "same URL + same content but
/// different policy" cases that should NOT be considered a cache hit.
pub fn policy_fingerprint(policy: &RunPolicy) -> String {
    // serde_json with sorted keys: relies on the field-order in the
    // struct definition being stable (which it is — Rust doesn't
    // reorder fields). For multi-value fields (Vecs of headers etc.)
    // we hash them as-presented; callers SHOULD pre-sort them for
    // canonical comparison.
    let canonical = serde_json::to_vec(policy)
        .expect("RunPolicy must serialize; types in this module are all serde-derive");
    let mut h = Hasher::new();
    h.update(&canonical);
    format!("blake3:{}", h.finalize().to_hex())
}

/// All inputs that, combined with the URL, must produce identical
/// output bytes when `determinism == Strict`. Hashed into a single
/// `DeterminismIdentity` string suitable for cache-keying and
/// audit-trail stamping.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeterminismIdentity {
    pub url: String,
    pub policy_fp: String,
    /// Per-input hash combining url + policy_fp + driver_kind + UA.
    /// Two runs that hash to the same `id` are required to produce
    /// identical `NormalizedOutput.fingerprint` in Strict mode.
    pub id: String,
}

/// Build a [`DeterminismIdentity`] for the given run. Stamping this
/// into output meta lets a downstream auditor verify the claim by
/// rebuilding the identity from the recorded policy + url and
/// comparing to the recorded id.
pub fn record_determinism_inputs(url: &str, policy: &RunPolicy) -> DeterminismIdentity {
    let policy_fp = policy_fingerprint(policy);
    let mut h = Hasher::new();
    h.update(url.as_bytes());
    h.update(b"|");
    h.update(policy_fp.as_bytes());
    h.update(b"|");
    h.update(policy.fetch.driver_kind.as_bytes());
    h.update(b"|");
    h.update(policy.fetch.user_agent.as_bytes());
    let id = format!("dq:{}", h.finalize().to_hex());
    DeterminismIdentity {
        url: url.to_string(),
        policy_fp,
        id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_best_effort_preset() {
        let d = RunPolicy::default();
        assert_eq!(d.determinism, Determinism::BestEffort);
        let p = RunPolicy::best_effort();
        // Both serialize to the same canonical JSON.
        assert_eq!(
            serde_json::to_string(&d).unwrap(),
            serde_json::to_string(&p).unwrap()
        );
    }

    #[test]
    fn strict_disables_retries_to_avoid_masking_race_conditions() {
        let p = RunPolicy::strict();
        assert_eq!(p.retry.max_attempts, 1);
        assert!(p.retry.transient_codes.is_empty());
    }

    #[test]
    fn off_disables_robots_and_checkpoints() {
        let p = RunPolicy::off();
        assert!(!p.discovery.respect_robots);
        assert_eq!(p.checkpoint.every_n_pages, 0);
        assert_eq!(p.checkpoint.min_interval_s, 0);
    }

    #[test]
    fn fingerprint_is_stable_for_equal_policies() {
        let a = RunPolicy::strict();
        let b = RunPolicy::strict();
        assert_eq!(policy_fingerprint(&a), policy_fingerprint(&b));
        assert!(policy_fingerprint(&a).starts_with("blake3:"));
    }

    #[test]
    fn fingerprint_differs_when_determinism_changes() {
        let a = RunPolicy::strict();
        let b = RunPolicy::best_effort();
        assert_ne!(policy_fingerprint(&a), policy_fingerprint(&b));
    }

    #[test]
    fn fingerprint_differs_when_extraction_version_changes() {
        let mut a = RunPolicy::strict();
        let mut b = RunPolicy::strict();
        b.extraction.markdown_converter_version = "html2md-0.3".into();
        assert_ne!(policy_fingerprint(&a), policy_fingerprint(&b));
        // Sanity: with a equal-mutation both differ from baseline.
        a.extraction.markdown_converter_version = "html2md-0.3".into();
        assert_eq!(policy_fingerprint(&a), policy_fingerprint(&b));
    }

    #[test]
    fn determinism_identity_round_trips_same_id_for_same_inputs() {
        let p = RunPolicy::strict();
        let id1 = record_determinism_inputs("https://example.com/", &p);
        let id2 = record_determinism_inputs("https://example.com/", &p);
        assert_eq!(id1, id2);
        assert!(id1.id.starts_with("dq:"));
    }

    #[test]
    fn determinism_identity_differs_when_url_changes() {
        let p = RunPolicy::strict();
        let a = record_determinism_inputs("https://example.com/a", &p);
        let b = record_determinism_inputs("https://example.com/b", &p);
        assert_ne!(a.id, b.id);
        // policy_fp is the same — only the URL changed.
        assert_eq!(a.policy_fp, b.policy_fp);
    }

    #[test]
    fn determinism_identity_differs_when_driver_kind_changes() {
        let mut a = RunPolicy::strict();
        let mut b = RunPolicy::strict();
        b.fetch.driver_kind = "tls".into();
        let id_a = record_determinism_inputs("https://x/", &a);
        let id_b = record_determinism_inputs("https://x/", &b);
        assert_ne!(id_a.id, id_b.id);
    }

    #[test]
    fn determinism_is_strict_helper_matches_variant() {
        assert!(Determinism::Strict.is_strict());
        assert!(!Determinism::BestEffort.is_strict());
        assert!(!Determinism::Off.is_strict());
    }
}
