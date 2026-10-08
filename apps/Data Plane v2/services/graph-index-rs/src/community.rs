//! GraphRAG community detection (Microsoft-GraphRAG-style level-0 communities:
//! connected components of the visibility-gated entity/relationship graph).
//!
//! Cost shape: exactly TWO bulk queries (visible entities and visible edge
//! pairs) plus one upsert-and-prune transaction — never a per-entity N+1.
//! Detection is deterministic given the graph, and community ids are derived
//! from the member set (`store::community_identity`), so persistence is
//! idempotent at the *row* level: an unchanged community lands on the same row
//! and **keeps its generated summary**, while communities that no longer exist
//! are pruned. This is what makes P1-5 summarisation affordable — detection
//! runs on every ingest that adds a relationship, and the previous
//! delete-then-insert with fresh v4 ids discarded every summary each time.
//!
//! Read-side visibility is enforced independently by the consumers
//! (retrieval-engine's `COMMUNITY_SUMMARY_SQL` requires the queried entity set
//! to cover the community), so a stale community can narrow results but never
//! widen them.

use std::collections::{HashMap, HashSet};

use crate::model::Community;
use crate::pipeline::{self, Ports};
use crate::store::GraphStore;

/// Undirected connected components over `entity_ids` using `edges`, returning
/// only components with at least `min_size` members. Pure and deterministic:
/// components surface in first-seen entity order, members sorted for stable
/// output. Edge endpoints outside `entity_ids` (non-visible entities) are
/// ignored so a hidden entity can never bridge two visible components.
fn connected_components(
    entity_ids: &[String],
    edges: &[(String, String)],
    min_size: usize,
) -> Vec<Vec<String>> {
    let known: HashSet<&str> = entity_ids.iter().map(String::as_str).collect();
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for (a, b) in edges {
        if known.contains(a.as_str()) && known.contains(b.as_str()) {
            adjacency.entry(a.as_str()).or_default().push(b.as_str());
            adjacency.entry(b.as_str()).or_default().push(a.as_str());
        }
    }

    let mut visited: HashSet<&str> = HashSet::new();
    let mut components = Vec::new();
    for entity in entity_ids {
        if visited.contains(entity.as_str()) {
            continue;
        }
        let mut component: Vec<String> = Vec::new();
        let mut stack = vec![entity.as_str()];
        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            component.push(node.to_string());
            if let Some(neighbors) = adjacency.get(node) {
                for n in neighbors {
                    if !visited.contains(n) {
                        stack.push(n);
                    }
                }
            }
        }
        if component.len() >= min_size {
            component.sort();
            components.push(component);
        }
    }
    components
}

/// Detects level-0 communities for `org_id` and atomically replaces the org's
/// derived `graph_communities` rows. Errors propagate — a failed listing must
/// NOT silently wipe existing communities (the replace only runs on success).
pub async fn detect_communities(
    store: &GraphStore,
    org_id: &str,
    min_size: usize,
) -> anyhow::Result<usize> {
    let entity_ids = store.list_visible_entity_ids(org_id).await?;
    let edges = store.list_visible_relationship_pairs(org_id).await?;

    let communities: Vec<Community> = connected_components(&entity_ids, &edges, min_size.max(2))
        .into_iter()
        .map(|entity_ids| Community {
            // Deterministic on the member set (P1-5): an unchanged community
            // keeps its id across recomputes, so `replace_communities` can
            // preserve its generated summary instead of discarding it. Was
            // `Uuid::new_v4()`, which made every ingest mint fresh ids.
            community_id: crate::store::community_identity(org_id, &entity_ids),
            org_id: org_id.to_string(),
            // `summary: None` here means "not computed by detection" — it does
            // NOT clear a stored summary. `replace_communities` leaves the
            // existing `summary` column untouched on conflict.
            summary: None,
            entity_ids,
            level: 0,
        })
        .collect();

    store.replace_communities(org_id, &communities).await?;
    tracing::info!(org_id, count = communities.len(), "communities detected");
    Ok(communities.len())
}

/// Default cap on communities summarised per run.
///
/// Bounds cost and latency: each one is an LLM call, and this runs after every
/// ingest that adds a relationship. Because
/// [`GraphStore::list_communities_needing_summary`] only returns
/// `summary IS NULL` rows ordered largest-first, a backlog drains over
/// successive ingests, biggest (most useful) first, instead of stalling one
/// ingest on a hundred calls. Override with `COMMUNITY_SUMMARY_MAX_PER_RUN`.
fn max_summaries_per_run() -> i64 {
    // Boot has already refused a value that does not parse (`Config::validate`), so the fallback
    // is for a test that never booted: the default.
    std::env::var("COMMUNITY_SUMMARY_MAX_PER_RUN")
        .ok()
        .and_then(|v| parse_max_summaries_per_run(&v).ok())
        .unwrap_or(DEFAULT_MAX_SUMMARIES_PER_RUN)
}

/// `COMMUNITY_SUMMARY_MAX_PER_RUN` when it is not set.
pub(crate) const DEFAULT_MAX_SUMMARIES_PER_RUN: i64 = 5;
/// The most a deployment may ask for: each summary is a paid call after an ingest.
pub(crate) const MAX_SUMMARIES_PER_RUN_CEILING: i64 = 1000;

/// Read `COMMUNITY_SUMMARY_MAX_PER_RUN`: blank is the default, otherwise a whole number from 1 to
/// [`MAX_SUMMARIES_PER_RUN_CEILING`]. Anything else is an error that names the variable: a cap
/// that quietly became 5 (or 0) is a budget control that does not do what its owner set.
pub(crate) fn parse_max_summaries_per_run(value: &str) -> anyhow::Result<i64> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(DEFAULT_MAX_SUMMARIES_PER_RUN);
    }
    match value.parse::<i64>() {
        Ok(parsed) if (1..=MAX_SUMMARIES_PER_RUN_CEILING).contains(&parsed) => Ok(parsed),
        _ => anyhow::bail!(
            "COMMUNITY_SUMMARY_MAX_PER_RUN must be a whole number from 1 to \
             {MAX_SUMMARIES_PER_RUN_CEILING}, got `{value}`"
        ),
    }
}

/// Read `COMMUNITY_SUMMARY_ENABLED`: `1`, `true` or `yes` is on; blank, `0`, `false` or `no` is
/// off (case does not matter). Anything else is an error that names the variable: a switch that
/// spends a paid call per community must not be left off, or on, by a typo.
pub(crate) fn parse_summaries_enabled(value: &str) -> anyhow::Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Ok(true),
        "" | "0" | "false" | "no" => Ok(false),
        other => anyhow::bail!(
            "COMMUNITY_SUMMARY_ENABLED must be 1, true or yes to turn community summaries on, \
             or blank, 0, false or no to leave them off, got `{other}`"
        ),
    }
}

/// Whether to generate community summaries at all (plan P1-5).
///
/// Default **off**: this spends real inference budget on every ingest, so it is
/// an explicit operational opt-in rather than something that switches on with a
/// deploy. Set `COMMUNITY_SUMMARY_ENABLED=true`.
fn summaries_enabled() -> bool {
    // A value `Config::validate` would have refused reads as off: the safe side of a paid switch.
    std::env::var("COMMUNITY_SUMMARY_ENABLED")
        .ok()
        .and_then(|v| parse_summaries_enabled(&v).ok())
        .unwrap_or(false)
}

/// Generates summaries for this org's un-summarised communities (plan P1-5).
///
/// Fills `graph_communities.summary`, which
/// `retrieval-engine::search::graph::community_summary_search` (already wired
/// into `api/mod.rs`) reads but which nothing ever wrote — GraphRAG "global
/// search" was scaffolded and inert.
///
/// Best-effort per community: one failure is logged and the others continue, so
/// a single provider hiccup cannot abort the batch. Returns how many were
/// written.
///
/// A summary is a paid call like an extraction (design task CR-T12), so it passes the same
/// gate: the organization's processing policy (`graph_extraction = off` withholds it), its
/// retention posture under its own bearer reason (`graph-community-summary`: a zero-retention
/// organization gets no call, and a posture Auth Core could not resolve is a wait), and the
/// background budget (alert-only by default; `stop` withholds it, `throttle` writes one at a
/// time). Its calls are metered under their own feature, and its failures are typed: a
/// refused credential, a throttle or a provider that is down ends the run instead of asking
/// once per community.
pub async fn summarize_communities<P: Ports>(ports: &P, org_id: &str) -> anyhow::Result<usize> {
    if !summaries_enabled() {
        return Ok(0);
    }
    pipeline::summarize_communities(ports, org_id, max_summaries_per_run()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn finds_components_and_filters_by_min_size() {
        let entities = s(&["a", "b", "c", "d", "e"]);
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string()),
            ("d".to_string(), "e".to_string()),
        ];
        // min_size 3: only {a,b,c} qualifies; {d,e} is too small.
        let got = connected_components(&entities, &edges, 3);
        assert_eq!(got, vec![s(&["a", "b", "c"])]);
        // min_size 2: both components qualify, first-seen order.
        let got2 = connected_components(&entities, &edges, 2);
        assert_eq!(got2, vec![s(&["a", "b", "c"]), s(&["d", "e"])]);
    }

    #[test]
    fn hidden_entities_never_bridge_visible_components() {
        // "x" is NOT in the visible entity list; its edges must not merge
        // {a,b} and {c,d} into one community.
        let entities = s(&["a", "b", "c", "d"]);
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("a".to_string(), "x".to_string()),
            ("x".to_string(), "c".to_string()),
            ("c".to_string(), "d".to_string()),
        ];
        let got = connected_components(&entities, &edges, 2);
        assert_eq!(got, vec![s(&["a", "b"]), s(&["c", "d"])]);
    }

    #[test]
    fn singletons_are_excluded_and_empty_graph_is_empty() {
        let entities = s(&["a", "b"]);
        assert!(connected_components(&entities, &[], 2).is_empty());
        assert!(connected_components(&[], &[], 2).is_empty());
    }

    /// The switch that spends a paid call per community: off by default (blank is what compose
    /// passes when nothing is set), on only for the three spellings the runtime has always read, and
    /// a typo is an error rather than a switch left in the wrong position.
    #[test]
    fn the_summary_switch_is_off_by_default_and_a_typo_is_refused() {
        for off in ["", "  ", "0", "false", "FALSE", "no", "No"] {
            assert!(!parse_summaries_enabled(off).expect(off), "{off:?}");
        }
        for on in ["1", "true", "True", "yes", " YES "] {
            assert!(parse_summaries_enabled(on).expect(on), "{on:?}");
        }
        for typo in ["on", "enabled", "ture", "2", "off"] {
            let error = parse_summaries_enabled(typo).expect_err(typo).to_string();
            assert!(
                error.contains("COMMUNITY_SUMMARY_ENABLED"),
                "{typo}: {error}"
            );
        }
    }

    #[test]
    fn the_summary_cap_is_a_whole_number_in_range_and_never_quietly_five() {
        assert_eq!(parse_max_summaries_per_run("").expect("blank"), 5);
        assert_eq!(parse_max_summaries_per_run(" 12 ").expect("12"), 12);
        assert_eq!(parse_max_summaries_per_run("1").expect("1"), 1);
        assert_eq!(parse_max_summaries_per_run("1000").expect("ceiling"), 1000);
        for bad in ["0", "-3", "1001", "five", "2.5"] {
            let error = parse_max_summaries_per_run(bad).expect_err(bad).to_string();
            assert!(
                error.contains("COMMUNITY_SUMMARY_MAX_PER_RUN"),
                "{bad}: {error}"
            );
        }
    }
}
