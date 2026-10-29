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
    std::env::var("COMMUNITY_SUMMARY_MAX_PER_RUN")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(5)
}

/// Whether to generate community summaries at all (plan P1-5).
///
/// Default **off**: this spends real inference budget on every ingest, so it is
/// an explicit operational opt-in rather than something that switches on with a
/// deploy. Set `COMMUNITY_SUMMARY_ENABLED=true`.
fn summaries_enabled() -> bool {
    std::env::var("COMMUNITY_SUMMARY_ENABLED")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
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
pub async fn summarize_communities(
    store: &GraphStore,
    extractor: &crate::extractor::GraphExtractor,
    org_id: &str,
) -> anyhow::Result<usize> {
    if !summaries_enabled() {
        return Ok(0);
    }
    let pending = store
        .list_communities_needing_summary(org_id, max_summaries_per_run())
        .await?;
    if pending.is_empty() {
        return Ok(0);
    }

    let mut written = 0usize;
    for (community_id, labels) in pending {
        // `zdr: false` — graph entities only exist for non-restrictive content
        // (the ingest consumer drops restrictive-ZDR events before extraction),
        // and `summarize_community` still refuses the direct-Azure path if that
        // ever stops being true.
        match extractor.summarize_community(&labels, org_id, false).await {
            Ok(summary) => match store
                .set_community_summary(org_id, &community_id, &summary)
                .await
            {
                // `false` means another run summarised it first — not an error.
                Ok(true) => written += 1,
                Ok(false) => tracing::debug!(
                    org_id,
                    community_id,
                    "community summary already present; skipped"
                ),
                Err(e) => {
                    tracing::warn!(err = %e, org_id, community_id, "store community summary failed")
                }
            },
            Err(e) => {
                tracing::warn!(err = %e, org_id, community_id, "community summarisation failed")
            }
        }
    }
    if written > 0 {
        tracing::info!(org_id, written, "community summaries generated");
    }
    Ok(written)
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
}
