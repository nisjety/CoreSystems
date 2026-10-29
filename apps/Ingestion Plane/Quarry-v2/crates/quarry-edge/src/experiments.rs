//! A/B experiment tagging (QRY-18).
//!
//! Layered on top of [`crate::canary::CanarySplitter`]. While canary is a
//! single-axis rollout (in cohort / out cohort), experiments can compose
//! multiple deterministic splits across the same request to compare drivers,
//! transforms, and provider stacks side-by-side.
//!
//! Metadata flows in three places:
//! - HTTP response headers (`x-quarry-experiments: pinned-tls=variant_a;driver=variant_b`)
//! - Event payload `experiments` field (NATS / SSE consumers)
//! - Scoreboard rows so eval CI can compare by variant
//!
//! ZDR-safe: experiment metadata records only the variant token, never user
//! input or content payloads.

// scaffolding: experiment-tagging API surface, wired into routes in a follow-up.
#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub name: String,
    pub variants: Vec<String>,
    pub salt: String,
}

impl Experiment {
    pub fn new(
        name: impl Into<String>,
        variants: Vec<impl Into<String>>,
        salt: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            variants: variants.into_iter().map(Into::into).collect(),
            salt: salt.into(),
        }
    }

    /// Pick the variant for a given routing key, e.g. `org_id` or `request_id`.
    /// Deterministic for the same `(salt, key)`.
    pub fn assign(&self, key: &str) -> Option<&str> {
        if self.variants.is_empty() {
            return None;
        }
        let bucket = bucket_for(&self.salt, key);
        let idx = (bucket as usize) % self.variants.len();
        Some(self.variants[idx].as_str())
    }
}

/// Stable BLAKE3-keyed bucket value for `(salt, key)` pairs. Mirrors the
/// canary splitter's bucketing function so experiments and canary use a
/// shared hashing convention.
fn bucket_for(salt: &str, key: &str) -> u32 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(salt.as_bytes());
    hasher.update(b":");
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let bytes = digest.as_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) % 100
}

/// Registry of experiments active on this edge process. Cloned freely.
#[derive(Debug, Clone, Default)]
pub struct ExperimentRegistry {
    by_name: BTreeMap<String, Experiment>,
}

impl ExperimentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, exp: Experiment) -> &mut Self {
        self.by_name.insert(exp.name.clone(), exp);
        self
    }

    pub fn get(&self, name: &str) -> Option<&Experiment> {
        self.by_name.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(|s| s.as_str())
    }

    /// Assign every active experiment for `key`. Returns a deterministic
    /// `BTreeMap<name, variant>` so headers and event payloads are stable.
    pub fn assignments(&self, key: &str) -> Assignments {
        let mut out = BTreeMap::new();
        for (name, exp) in &self.by_name {
            if let Some(variant) = exp.assign(key) {
                out.insert(name.clone(), variant.to_string());
            }
        }
        Assignments(out)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Assignments(BTreeMap<String, String>);

impl Assignments {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(|s| s.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Encode as a header-safe string, e.g. `pinned-tls=variant_a;driver=variant_b`.
    pub fn to_header(&self) -> String {
        let parts: Vec<String> = self.0.iter().map(|(k, v)| format!("{k}={v}")).collect();
        parts.join(";")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> ExperimentRegistry {
        let mut r = ExperimentRegistry::new();
        r.register(Experiment::new(
            "driver",
            vec!["static_first", "browser_first"],
            "drv-2026",
        ));
        r.register(Experiment::new(
            "transform",
            vec!["readability", "html2md_only"],
            "tx-2026",
        ));
        r
    }

    #[test]
    fn registry_assigns_all_experiments_deterministically() {
        let r = registry();
        let a = r.assignments("org_abc");
        let b = r.assignments("org_abc");
        assert_eq!(a.to_header(), b.to_header());
        assert!(a.get("driver").is_some());
        assert!(a.get("transform").is_some());
    }

    #[test]
    fn different_keys_can_yield_different_variants() {
        let r = registry();
        let a = r.assignments("k1");
        let b = r.assignments("k2");
        // Across enough keys, at least one will diverge — sample 100 keys
        let mut differ = 0;
        for i in 0..100 {
            let key = format!("k{i}");
            let asgn = r.assignments(&key);
            if asgn.get("driver") != a.get("driver") || asgn.get("driver") != b.get("driver") {
                differ += 1;
            }
        }
        assert!(differ > 10);
    }

    #[test]
    fn header_format_is_stable() {
        let r = registry();
        let a = r.assignments("k1");
        let header = a.to_header();
        // BTreeMap order means "driver" comes before "transform"
        assert!(header.starts_with("driver="));
        assert!(header.contains(";transform="));
    }

    #[test]
    fn empty_registry_returns_empty_assignments() {
        let r = ExperimentRegistry::new();
        let a = r.assignments("anything");
        assert!(a.is_empty());
        assert_eq!(a.to_header(), "");
    }

    #[test]
    fn unregistered_experiment_returns_none() {
        let r = registry();
        let a = r.assignments("k");
        assert!(a.get("nonexistent").is_none());
    }

    #[test]
    fn empty_variants_returns_no_assignment() {
        let exp = Experiment::new("x", Vec::<&str>::new(), "salt");
        assert!(exp.assign("k").is_none());
    }
}
