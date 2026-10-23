//! Per-tenant parse cache keyed by `(org_id, sha256(source bytes))` (E3.6-9, F-t2-037).
//!
//! A pure, bounded, in-process cache for parsed document output (PDF, DOCX and
//! the later Office parsers). Nothing is wired into the pipeline yet; the stage
//! that calls it is a later slice.
//!
//! Rules this module enforces itself, so a caller cannot forget them:
//! * Tenant boundary. The org id is part of the key. Two orgs that upload the
//!   same bytes get two separate entries, and a lookup under one org never
//!   returns the other org's value. An empty or whitespace-only org id is never
//!   a key: [`ParseCacheKey::new`] returns `None`, reads miss and writes are
//!   dropped.
//! * ZDR. Under [`ZdrMode::On`] nothing is read from and nothing is written to
//!   the cache (`WriteKind::Cache` in `quarry_core::zdr::guard`). A `get` under
//!   ZDR misses even when an entry was stored earlier with ZDR off, so a ZDR
//!   request never sees a value that a non-ZDR request left behind.
//! * Bounded memory. At `max_entries` the oldest insertion is evicted first.
//!
//! The cache holds whatever value type the caller parses into. It does not log,
//! does not touch the filesystem and does not read the environment.

use std::collections::{HashMap, VecDeque};

use quarry_core::zdr::{guard, WriteKind, ZdrMode};
use sha2::{Digest, Sha256};

/// Default bound used by callers that do not pick one.
pub const DEFAULT_MAX_ENTRIES: usize = 256;

/// The SHA-256 digest of `source`.
pub fn sha256_of(source: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(source);
    hasher.finalize().into()
}

/// A cache key: the org id and the SHA-256 of the source bytes.
///
/// Built only through [`ParseCacheKey::new`] or [`ParseCacheKey::from_digest`],
/// both of which refuse an empty org id, so a key always names a tenant.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParseCacheKey {
    org_id: String,
    sha256: [u8; 32],
}

impl ParseCacheKey {
    /// Key for `source` under `org_id`. `None` for an empty or whitespace-only
    /// org id.
    pub fn new(org_id: &str, source: &[u8]) -> Option<Self> {
        Self::from_digest(org_id, sha256_of(source))
    }

    /// Key for a digest the caller already computed. `None` for an empty or
    /// whitespace-only org id.
    pub fn from_digest(org_id: &str, sha256: [u8; 32]) -> Option<Self> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return None;
        }
        Some(Self {
            org_id: org_id.to_owned(),
            sha256,
        })
    }

    pub fn org_id(&self) -> &str {
        &self.org_id
    }

    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    /// Lower-case hex of the digest, for diagnostics and tests.
    pub fn sha256_hex(&self) -> String {
        self.sha256.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// A bounded map from [`ParseCacheKey`] to a parsed value, with FIFO eviction.
#[derive(Debug)]
pub struct ParseCache<V: Clone> {
    max_entries: usize,
    entries: HashMap<ParseCacheKey, V>,
    insertion_order: VecDeque<ParseCacheKey>,
}

impl<V: Clone> ParseCache<V> {
    /// A cache holding at most `max_entries` values (at least one).
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            entries: HashMap::new(),
            insertion_order: VecDeque::new(),
        }
    }

    /// The value for `key`, or `None` on a miss. Always `None` under ZDR.
    pub fn get(&self, zdr: ZdrMode, key: &ParseCacheKey) -> Option<V> {
        if guard(zdr, WriteKind::Cache).is_err() {
            return None;
        }
        self.entries.get(key).cloned()
    }

    /// Store `value` under `key`. Returns `true` when the value is stored.
    /// Returns `false`, storing nothing, under ZDR. A re-put of an existing key
    /// replaces the value and keeps its place in the eviction order.
    pub fn put(&mut self, zdr: ZdrMode, key: ParseCacheKey, value: V) -> bool {
        if guard(zdr, WriteKind::Cache).is_err() {
            return false;
        }
        if let Some(slot) = self.entries.get_mut(&key) {
            *slot = value;
            return true;
        }
        while self.entries.len() >= self.max_entries {
            match self.insertion_order.pop_front() {
                Some(oldest) => {
                    self.entries.remove(&oldest);
                }
                None => break,
            }
        }
        self.insertion_order.push_back(key.clone());
        self.entries.insert(key, value);
        true
    }

    /// Number of stored entries across all orgs.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of stored entries for one org.
    pub fn len_for_org(&self, org_id: &str) -> usize {
        let org_id = org_id.trim();
        self.entries
            .keys()
            .filter(|key| key.org_id == org_id)
            .count()
    }

    /// Drop every entry.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.insertion_order.clear();
    }
}

impl<V: Clone> Default for ParseCache<V> {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_ENTRIES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC_A: &[u8] = b"%PDF-1.7 document a";
    const DOC_B: &[u8] = b"%PDF-1.7 document b";

    fn key(org: &str, bytes: &[u8]) -> ParseCacheKey {
        ParseCacheKey::new(org, bytes).expect("non-empty org")
    }

    #[test]
    fn sha256_matches_the_known_vector_for_abc() {
        assert_eq!(
            ParseCacheKey::from_digest("org-1", sha256_of(b"abc"))
                .unwrap()
                .sha256_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn empty_or_blank_org_is_never_a_key() {
        assert!(ParseCacheKey::new("", DOC_A).is_none());
        assert!(ParseCacheKey::new("   ", DOC_A).is_none());
        assert!(ParseCacheKey::from_digest("\t", [0u8; 32]).is_none());
    }

    #[test]
    fn org_id_is_trimmed_so_padded_and_bare_ids_share_an_entry() {
        assert_eq!(key(" org-1 ", DOC_A), key("org-1", DOC_A));
    }

    #[test]
    fn same_org_and_bytes_hit_and_different_bytes_miss() {
        let mut cache = ParseCache::new(8);
        assert!(cache.put(ZdrMode::Off, key("org-1", DOC_A), "parsed-a".to_owned()));
        assert_eq!(
            cache.get(ZdrMode::Off, &key("org-1", DOC_A)).as_deref(),
            Some("parsed-a")
        );
        assert_eq!(cache.get(ZdrMode::Off, &key("org-1", DOC_B)), None);
    }

    #[test]
    fn same_bytes_under_another_org_miss() {
        let mut cache = ParseCache::new(8);
        cache.put(ZdrMode::Off, key("org-1", DOC_A), "org-1 output".to_owned());
        assert_eq!(cache.get(ZdrMode::Off, &key("org-2", DOC_A)), None);
        assert_eq!(cache.len_for_org("org-1"), 1);
        assert_eq!(cache.len_for_org("org-2"), 0);
    }

    #[test]
    fn zdr_on_put_stores_nothing_and_reports_false() {
        let mut cache = ParseCache::new(8);
        assert!(!cache.put(ZdrMode::On, key("org-1", DOC_A), "parsed-a".to_owned()));
        assert!(cache.is_empty());
    }

    #[test]
    fn zdr_on_get_misses_even_after_a_non_zdr_put() {
        let mut cache = ParseCache::new(8);
        cache.put(ZdrMode::Off, key("org-1", DOC_A), "parsed-a".to_owned());
        assert_eq!(cache.get(ZdrMode::On, &key("org-1", DOC_A)), None);
        // The entry is still there for non-ZDR callers.
        assert_eq!(
            cache.get(ZdrMode::Off, &key("org-1", DOC_A)).as_deref(),
            Some("parsed-a")
        );
    }

    #[test]
    fn oldest_insertion_is_evicted_at_capacity() {
        let mut cache = ParseCache::new(2);
        cache.put(ZdrMode::Off, key("org-1", b"a"), 1u32);
        cache.put(ZdrMode::Off, key("org-1", b"b"), 2u32);
        cache.put(ZdrMode::Off, key("org-1", b"c"), 3u32);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(ZdrMode::Off, &key("org-1", b"a")), None);
        assert_eq!(cache.get(ZdrMode::Off, &key("org-1", b"b")), Some(2));
        assert_eq!(cache.get(ZdrMode::Off, &key("org-1", b"c")), Some(3));
    }

    #[test]
    fn re_put_replaces_value_without_growing_or_evicting() {
        let mut cache = ParseCache::new(2);
        cache.put(ZdrMode::Off, key("org-1", b"a"), 1u32);
        cache.put(ZdrMode::Off, key("org-1", b"b"), 2u32);
        assert!(cache.put(ZdrMode::Off, key("org-1", b"a"), 10u32));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(ZdrMode::Off, &key("org-1", b"a")), Some(10));
        assert_eq!(cache.get(ZdrMode::Off, &key("org-1", b"b")), Some(2));
    }

    #[test]
    fn zero_capacity_is_raised_to_one() {
        let mut cache = ParseCache::new(0);
        cache.put(ZdrMode::Off, key("org-1", b"a"), 1u32);
        cache.put(ZdrMode::Off, key("org-1", b"b"), 2u32);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(ZdrMode::Off, &key("org-1", b"b")), Some(2));
    }

    #[test]
    fn clear_drops_every_org() {
        let mut cache = ParseCache::new(8);
        cache.put(ZdrMode::Off, key("org-1", DOC_A), 1u32);
        cache.put(ZdrMode::Off, key("org-2", DOC_A), 2u32);
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.get(ZdrMode::Off, &key("org-1", DOC_A)), None);
    }
}
