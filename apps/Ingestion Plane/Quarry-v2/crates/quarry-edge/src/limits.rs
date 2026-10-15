//! Crawl bounds applied at the edge.
//!
//! A crawl with no page limit ran until the 30-minute workflow timeout, and a
//! request of `max_pages: 0` meant "unlimited". Every crawl handed to the
//! orchestrator now carries a finite page and depth bound: the request's value
//! when it is in range, the default when it is absent or zero, the hard ceiling
//! when it is above it. The edge returns what it resolved so a caller can see
//! when it was clamped, never silently.
//!
//! The same environment names and defaults are read by the orchestrator
//! (`services/quarry-orchestrator/internal/jobs/limits.go`), which re-resolves
//! the job parameters so a job row written elsewhere is bounded too.
//!
//! A `0` request is lenient here: it takes the default rather than being
//! refused. A strict `400 invalid_option` for it belongs to the request
//! validation work and is opt-in.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const ENV_DEFAULT_PAGES: &str = "QUARRY_CRAWL_DEFAULT_MAX_PAGES";
const ENV_HARD_PAGES: &str = "QUARRY_CRAWL_HARD_MAX_PAGES";
const ENV_DEFAULT_DEPTH: &str = "QUARRY_CRAWL_DEFAULT_MAX_DEPTH";
const ENV_HARD_DEPTH: &str = "QUARRY_CRAWL_HARD_MAX_DEPTH";

pub const DEFAULT_MAX_PAGES: u32 = 1000;
pub const HARD_MAX_PAGES: u32 = 5000;
pub const DEFAULT_MAX_DEPTH: u32 = 5;
pub const HARD_MAX_DEPTH: u32 = 10;

/// Operator-set bounds. The hard ceilings are never disabled; a zero default
/// means "no default", so an absent request takes the hard ceiling instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrawlLimits {
    pub default_pages: u32,
    pub hard_pages: u32,
    pub default_depth: u32,
    pub hard_depth: u32,
}

impl Default for CrawlLimits {
    fn default() -> Self {
        Self {
            default_pages: DEFAULT_MAX_PAGES,
            hard_pages: HARD_MAX_PAGES,
            default_depth: DEFAULT_MAX_DEPTH,
            hard_depth: HARD_MAX_DEPTH,
        }
    }
}

/// Why a resolved value is what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClampedBy {
    /// The request was absent or zero and took the default.
    Default,
    /// The request was above the hard ceiling and was clamped to it.
    Hard,
}

/// One bound as the caller sees it: what was asked, what will run, and why they
/// differ. `clamped_by` is empty when the request was used unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bound {
    pub requested: Option<u32>,
    pub effective: u32,
    pub clamped_by: Vec<ClampedBy>,
}

/// The resolved bounds echoed in the handoff response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrawlLimitsAck {
    pub max_pages: Bound,
    pub max_depth: Bound,
}

impl CrawlLimits {
    /// Read the bounds from the environment. An unset variable keeps the
    /// built-in value; an unparsable value, a zero hard ceiling or a default
    /// above its ceiling is an error naming the variable.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let mut limits = Self::default();
        for (name, slot) in [
            (ENV_DEFAULT_PAGES, &mut limits.default_pages),
            (ENV_HARD_PAGES, &mut limits.hard_pages),
            (ENV_DEFAULT_DEPTH, &mut limits.default_depth),
            (ENV_HARD_DEPTH, &mut limits.hard_depth),
        ] {
            let Some(raw) = lookup(name) else { continue };
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            *slot = raw
                .parse::<u32>()
                .map_err(|e| format!("{name}={raw:?} is not a non-negative integer: {e}"))?;
        }
        if limits.hard_pages == 0 {
            return Err(format!(
                "{ENV_HARD_PAGES} must be greater than zero: the hard page ceiling cannot be disabled"
            ));
        }
        if limits.hard_depth == 0 {
            return Err(format!(
                "{ENV_HARD_DEPTH} must be greater than zero: the hard depth ceiling cannot be disabled"
            ));
        }
        if limits.default_pages > limits.hard_pages {
            return Err(format!(
                "{ENV_DEFAULT_PAGES} ({}) exceeds {ENV_HARD_PAGES} ({})",
                limits.default_pages, limits.hard_pages
            ));
        }
        if limits.default_depth > limits.hard_depth {
            return Err(format!(
                "{ENV_DEFAULT_DEPTH} ({}) exceeds {ENV_HARD_DEPTH} ({})",
                limits.default_depth, limits.hard_depth
            ));
        }
        Ok(limits)
    }

    /// Apply the bounds to one request. A request of zero counts as absent and
    /// takes the default (or the hard ceiling when the default is disabled); a
    /// request above the ceiling is clamped to it; anything else is used as
    /// asked. The result is never zero, so a crawl is never unbounded.
    pub fn resolve(&self, req_pages: Option<u32>, req_depth: Option<u32>) -> CrawlLimitsAck {
        CrawlLimitsAck {
            max_pages: resolve_bound(req_pages, self.default_pages, self.hard_pages),
            max_depth: resolve_bound(req_depth, self.default_depth, self.hard_depth),
        }
    }
}

fn resolve_bound(requested: Option<u32>, default: u32, hard: u32) -> Bound {
    let (effective, clamped_by) = match requested {
        None | Some(0) => (
            if default == 0 { hard } else { default },
            vec![ClampedBy::Default],
        ),
        Some(value) if value > hard => (hard, vec![ClampedBy::Hard]),
        Some(value) => (value, Vec::new()),
    };
    Bound {
        requested,
        effective,
        clamped_by,
    }
}

/// The process-wide bounds, read from the environment once. Boot validates the
/// environment first ([`CrawlLimits::from_env`] in `main`), so the fallback to
/// the built-in defaults only ever covers a value that was valid at start-up.
pub fn crawl_limits() -> &'static CrawlLimits {
    static LIMITS: OnceLock<CrawlLimits> = OnceLock::new();
    LIMITS.get_or_init(|| CrawlLimits::from_env().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn zero_is_absent_and_takes_default() {
        let ack = CrawlLimits::default().resolve(Some(0), None);
        assert_eq!(ack.max_pages.effective, 1000);
        assert_eq!(ack.max_pages.clamped_by, vec![ClampedBy::Default]);
        assert_eq!(ack.max_pages.requested, Some(0));
        let ack = CrawlLimits::default().resolve(None, None);
        assert_eq!(ack.max_pages.effective, 1000);
        assert_eq!(ack.max_pages.requested, None);
    }

    #[test]
    fn in_range_request_is_used_as_asked() {
        let ack = CrawlLimits::default().resolve(Some(42), Some(3));
        assert_eq!(ack.max_pages.effective, 42);
        assert!(ack.max_pages.clamped_by.is_empty());
        assert_eq!(ack.max_depth.effective, 3);
        // The ceiling itself is in range.
        let ack = CrawlLimits::default().resolve(Some(5000), Some(10));
        assert_eq!(ack.max_pages.effective, 5000);
        assert!(ack.max_pages.clamped_by.is_empty());
    }

    #[test]
    fn above_hard_is_clamped_and_reported() {
        let ack = CrawlLimits::default().resolve(Some(1_000_000), Some(99));
        assert_eq!(ack.max_pages.effective, 5000);
        assert_eq!(ack.max_pages.clamped_by, vec![ClampedBy::Hard]);
        assert_eq!(ack.max_pages.requested, Some(1_000_000));
        assert_eq!(ack.max_depth.effective, 10);
        assert_eq!(ack.max_depth.clamped_by, vec![ClampedBy::Hard]);
    }

    #[test]
    fn depth_resolves_like_pages() {
        let ack = CrawlLimits::default().resolve(None, Some(0));
        assert_eq!(ack.max_depth.effective, 5);
        assert_eq!(ack.max_depth.clamped_by, vec![ClampedBy::Default]);
    }

    #[test]
    fn a_disabled_default_takes_the_hard_ceiling_never_unbounded() {
        let limits = CrawlLimits {
            default_pages: 0,
            ..CrawlLimits::default()
        };
        assert_eq!(limits.resolve(None, None).max_pages.effective, 5000);
        assert_eq!(limits.resolve(Some(9999), None).max_pages.effective, 5000);
    }

    #[test]
    fn from_env_overrides_and_keeps_defaults_for_unset() {
        let limits =
            CrawlLimits::from_lookup(env(&[(ENV_DEFAULT_PAGES, "7"), (ENV_HARD_PAGES, " 20 ")]))
                .unwrap();
        assert_eq!(limits.default_pages, 7);
        assert_eq!(limits.hard_pages, 20);
        assert_eq!(limits.default_depth, DEFAULT_MAX_DEPTH);
        assert_eq!(
            CrawlLimits::from_lookup(env(&[])).unwrap(),
            CrawlLimits::default()
        );
    }

    #[test]
    fn from_env_rejects_unparsable_naming_the_variable() {
        for (pairs, name) in [
            (&[(ENV_HARD_PAGES, "lots")][..], ENV_HARD_PAGES),
            (&[(ENV_DEFAULT_DEPTH, "-1")][..], ENV_DEFAULT_DEPTH),
            (&[(ENV_HARD_PAGES, "0")][..], ENV_HARD_PAGES),
            (&[(ENV_HARD_DEPTH, "0")][..], ENV_HARD_DEPTH),
            (&[(ENV_DEFAULT_PAGES, "9000")][..], ENV_DEFAULT_PAGES),
            (&[(ENV_DEFAULT_DEPTH, "11")][..], ENV_DEFAULT_DEPTH),
        ] {
            let pairs: &'static [(&str, &str)] = Box::leak(pairs.to_vec().into_boxed_slice());
            let error = CrawlLimits::from_lookup(env(pairs)).unwrap_err();
            assert!(error.contains(name), "{error} must name {name}");
        }
    }
}
