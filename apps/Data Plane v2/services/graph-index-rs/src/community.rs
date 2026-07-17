//! GraphRAG community detection (Microsoft-GraphRAG-style level-0 communities:
//! connected components of the visibility-gated entity/relationship graph).
//!
//! Cost shape: exactly TWO bulk queries (visible entities and visible edge
//! pairs) plus one delete-and-replace transaction — never a per-entity N+1.
//! Detection is deterministic given the graph; persistence is idempotent at
//! the set level (re-running replaces the org's derived rows atomically).
//!
//! Read-side visibility is enforced independently by the consumers
//! (retrieval-engine's `COMMUNITY_SUMMARY_SQL` requires the queried entity set
//! to cover the community), so a stale community can narrow results but never
//! widen them.

use std::collections::{HashMap, HashSet};
use uuid::Uuid;

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
            community_id: Uuid::new_v4().to_string(),
            org_id: org_id.to_string(),
            entity_ids,
            summary: None,
            level: 0,
        })
        .collect();

    store.replace_communities(org_id, &communities).await?;
    tracing::info!(org_id, count = communities.len(), "communities detected");
    Ok(communities.len())
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
