//! Query synonym expansion (OSS-parity P2 3A — analyzer slice).
//!
//! Elasticsearch-style synonym analysis, applied at *query* time: expand each
//! term with its synonyms as `OR` alternatives so a search for "js" also
//! matches "javascript". Pure-logic + table-driven; the lexical index
//! (`TantivyLocalIndex`) and SERP providers both accept the expanded string.
//!
//! (Tantivy index snapshot / reindex / aliases are ops-level filesystem
//! operations and are tracked as a separate follow-up; this module covers the
//! analyzer/synonym half of 3A, which is the search-quality lever.)

use std::collections::HashMap;

/// Case-insensitive term → synonyms table.
#[derive(Debug, Clone, Default)]
pub struct SynonymMap {
    map: HashMap<String, Vec<String>>,
}

impl SynonymMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register synonyms for `term` (stored lowercase; merged if present).
    pub fn insert(&mut self, term: &str, synonyms: &[&str]) {
        let entry = self.map.entry(term.to_lowercase()).or_default();
        for s in synonyms {
            let s = s.to_string();
            if !entry.contains(&s) {
                entry.push(s);
            }
        }
    }

    /// A small built-in tech/web synonym set. Bidirectional pairs are inserted
    /// both ways so expansion works regardless of which token the user typed.
    pub fn with_defaults() -> Self {
        let mut m = Self::new();
        let pairs = [
            ("js", "javascript"),
            ("ts", "typescript"),
            ("k8s", "kubernetes"),
            ("db", "database"),
            ("ml", "machine learning"),
            ("auth", "authentication"),
            ("docs", "documentation"),
        ];
        for (a, b) in pairs {
            m.insert(a, &[b]);
            m.insert(b, &[a]);
        }
        m
    }

    pub fn synonyms_of(&self, term: &str) -> &[String] {
        self.map
            .get(&term.to_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// Expand a query: each term with known synonyms becomes
/// `term OR syn1 OR syn2`. Terms without synonyms pass through unchanged.
/// Original token casing is preserved; lookup is case-insensitive.
pub fn expand_query(query: &str, map: &SynonymMap) -> String {
    let mut parts: Vec<String> = Vec::new();
    for term in query.split_whitespace() {
        let syns = map.synonyms_of(term);
        if syns.is_empty() {
            parts.push(term.to_string());
        } else {
            let mut alt = vec![term.to_string()];
            for s in syns {
                if !alt.iter().any(|x| x.eq_ignore_ascii_case(s)) {
                    alt.push(s.clone());
                }
            }
            parts.push(alt.join(" OR "));
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_known_term() {
        let m = SynonymMap::with_defaults();
        let out = expand_query("js tutorial", &m);
        assert_eq!(out, "js OR javascript tutorial");
    }

    #[test]
    fn bidirectional_default_pairs() {
        let m = SynonymMap::with_defaults();
        assert!(m.synonyms_of("kubernetes").contains(&"k8s".to_string()));
        assert!(m.synonyms_of("k8s").contains(&"kubernetes".to_string()));
    }

    #[test]
    fn unknown_terms_pass_through() {
        let m = SynonymMap::with_defaults();
        assert_eq!(expand_query("quarry rocks", &m), "quarry rocks");
    }

    #[test]
    fn custom_synonyms_merge() {
        let mut m = SynonymMap::new();
        m.insert("car", &["automobile", "vehicle"]);
        let out = expand_query("Car", &m);
        assert_eq!(out, "Car OR automobile OR vehicle"); // casing preserved
    }
}
