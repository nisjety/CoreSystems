//! Outbound proxy pool with per-(org, host) sticky affinity.
//!
//! Why:
//! - Sites geo-block and rate-limit by IP; a fixed egress address gets
//!   degraded after a few hundred requests.
//! - But naive round-robin breaks session continuity — A/B-test
//!   buckets, cart state, login cookies all expect IP stability across
//!   a session.
//!
//! Strategy: hash the (org_id, host) tuple onto the proxy list. Same
//! org hitting the same host sees the same proxy for the lifetime of
//! the process. Different orgs (or the same org hitting a different
//! host) get a different slot, spreading load across the pool.
//!
//! Configuration: a `;`-separated list of proxy URIs in
//! `QUARRY_PROXY_POOL`. Schemes accepted:
//!   - `socks5://user:pass@host:port` (client resolves the target hostname,
//!     then hands the proxy an IP -- reqwest's configured DNS resolver runs
//!     here, so `fetch.rs` pins it exactly like direct egress)
//!   - `socks5h://...` (DNS through the proxy -- the raw hostname goes to
//!     the proxy, which resolves it; no client-side resolver is ever
//!     consulted, so a pin here would be a no-op)
//!   - `http://...`   (HTTP CONNECT proxy -- same server-side resolution as
//!     `socks5h://`: the `CONNECT host:port` line carries the hostname
//!     verbatim)
//!   - `https://...`  (HTTP CONNECT over a TLS-fronted proxy connection;
//!     same server-side resolution as `http://`)
//!
//! Only `socks5://` (and, if ever configured, plain `socks4://`) gets a
//! client-side DNS pin today. For the other three schemes, Quarry has no
//! visibility into what address the proxy resolves the target to -- that is
//! a proxy-trust-boundary question, not something a client-side fix can
//! close. See the "Provider gates" section of `docs/POLICY.md` for the
//! documented contract (`QUARRY_PROXY_FIRST_PARTY`, third-party processor
//! gating) that governs which proxies are allowed to be configured here.
//!
//! Empty / unset → `ProxyPool::empty()`; callers see `None` from
//! `pick()` and fall back to direct egress. This is the default in
//! dev and single-tenant deployments.
//!
//! Health checking and block-aware ordering live in `egress_broker`; this pool
//! only owns deterministic candidate ordering.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// A proxy entry. Wraps a parsed URI string; the actual `Proxy` value
/// is constructed by the driver because each driver (reqwest /
/// reqwest 0.13 / wreq) has its own `Proxy` type and feature gates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEntry {
    /// Full proxy URI (e.g. `socks5://user:pass@host:1080`). Always
    /// non-empty.
    pub uri: String,
}

impl ProxyEntry {
    pub fn new(uri: impl Into<String>) -> Option<Self> {
        let uri = uri.into();
        let uri = uri.trim();
        if uri.is_empty() {
            return None;
        }
        // We don't validate the scheme here — reqwest::Proxy::all()
        // will reject bad schemes at driver construction time with a
        // clearer error than we'd produce ad-hoc.
        Some(Self {
            uri: uri.to_string(),
        })
    }
}

/// Pool of proxies. Construct once at startup; clone is cheap (just
/// an `Arc`-able `Vec` of strings).
#[derive(Debug, Clone, Default)]
pub struct ProxyPool {
    entries: Vec<ProxyEntry>,
}

impl ProxyPool {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Parse a `;`-separated list of proxy URIs.
    ///
    /// Whitespace around each entry is trimmed; empty entries are
    /// skipped. Unparseable entries are silently dropped — a startup
    /// log captures the count of valid entries.
    pub fn from_env_string(raw: &str) -> Self {
        let entries: Vec<ProxyEntry> = raw.split(';').filter_map(ProxyEntry::new).collect();
        Self { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over the configured entries in the order they were
    /// declared. Used by drivers that need to pre-build one HTTP
    /// client per proxy at startup.
    pub fn entries(&self) -> impl Iterator<Item = &ProxyEntry> {
        self.entries.iter()
    }

    /// Pick a sticky proxy for the (org_id, host) tuple, or `None` if
    /// the pool is empty.
    ///
    /// `org_id` may be empty (anonymous / cross-tenant requests); the
    /// hash still works deterministically, just maps everything onto
    /// the first proxy. In single-tenant deployments where every
    /// scrape carries the same org the bias is intentional — that's
    /// session continuity.
    pub fn pick(&self, org_id: &str, host: &str) -> Option<&ProxyEntry> {
        self.slot(org_id, host)
            .and_then(|slot| self.entries.get(slot))
    }

    /// Return every proxy in deterministic retry order for `(org_id, host)`.
    /// The sticky primary is first; remaining entries wrap around from there.
    pub fn ordered_candidates(&self, org_id: &str, host: &str) -> Vec<ProxyEntry> {
        let Some(start) = self.slot(org_id, host) else {
            return Vec::new();
        };
        (0..self.entries.len())
            .filter_map(|offset| self.entries.get((start + offset) % self.entries.len()))
            .cloned()
            .collect()
    }

    fn slot(&self, org_id: &str, host: &str) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        let mut hasher = DefaultHasher::new();
        org_id.hash(&mut hasher);
        // Domain separation byte so org_id="abc" + host=""
        // doesn't collide with org_id="" + host="abc".
        0u8.hash(&mut hasher);
        host.hash(&mut hasher);
        Some((hasher.finish() as usize) % self.entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pool_returns_none() {
        let pool = ProxyPool::empty();
        assert!(pool.is_empty());
        assert!(pool.pick("acme", "example.com").is_none());
    }

    #[test]
    fn parses_semicolon_list() {
        let pool =
            ProxyPool::from_env_string("socks5://p1:1080 ; socks5://p2:1080 ;socks5://p3:1080");
        assert_eq!(pool.len(), 3);
    }

    #[test]
    fn skips_empty_entries() {
        let pool = ProxyPool::from_env_string(";;socks5://only:1080;;");
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn picks_deterministically_for_same_tuple() {
        let pool = ProxyPool::from_env_string("socks5://p1:1080;socks5://p2:1080;socks5://p3:1080");
        let a = pool.pick("acme", "example.com").unwrap().clone();
        let b = pool.pick("acme", "example.com").unwrap().clone();
        assert_eq!(a, b);
    }

    #[test]
    fn different_hosts_can_pick_different_proxies() {
        let pool = ProxyPool::from_env_string(
            "socks5://p1:1080;socks5://p2:1080;socks5://p3:1080;socks5://p4:1080",
        );
        let mut distinct = std::collections::HashSet::new();
        for host in [
            "a.example",
            "b.example",
            "c.example",
            "d.example",
            "e.example",
            "f.example",
            "g.example",
            "h.example",
        ] {
            distinct.insert(pool.pick("acme", host).unwrap().uri.clone());
        }
        // With 4 proxies and 8 distinct hosts we expect >1 slot used.
        // Hash collisions are possible but extremely unlikely to map
        // all 8 onto one slot.
        assert!(distinct.len() > 1, "hash should spread across the pool");
    }

    #[test]
    fn org_and_host_are_domain_separated() {
        // Without a separation byte, the pair ("ab", "c") could hash
        // the same as ("a", "bc"). We can't directly observe the
        // hasher state, but we can verify the pick function still
        // returns *some* entry deterministically for both shapes.
        let pool = ProxyPool::from_env_string("socks5://p1:1080;socks5://p2:1080");
        let x = pool.pick("ab", "c").unwrap().clone();
        let y = pool.pick("a", "bc").unwrap().clone();
        // Both must resolve to an entry; equality is allowed but rare.
        let _ = (x, y);
    }

    #[test]
    fn ordered_candidates_starts_with_sticky_pick() {
        let pool = ProxyPool::from_env_string("socks5://p1:1080;socks5://p2:1080;socks5://p3:1080");
        let primary = pool.pick("acme", "example.com").unwrap().clone();
        let ordered = pool.ordered_candidates("acme", "example.com");

        assert_eq!(ordered.len(), 3);
        assert_eq!(ordered[0], primary);
    }
}
