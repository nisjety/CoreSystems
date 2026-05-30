use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::model::Community;
use crate::store::GraphStore;

#[allow(dead_code)] // wired in a follow-up phase; keeps the GraphRAG community builder intact
pub async fn detect_communities(
    store: &GraphStore,
    org_id: &str,
    min_size: usize,
) -> anyhow::Result<Vec<Community>> {
    let (entities, _) = store
        .list_entities_by_type(org_id, "%", 10000, 0)
        .await
        .unwrap_or_default();

    let mut adjacency: HashMap<String, HashSet<String>> = HashMap::new();
    for entity in &entities {
        let rels = store
            .get_relationships(org_id, &entity.entity_id, None)
            .await?;
        for rel in &rels {
            adjacency
                .entry(rel.entity_a_id.clone())
                .or_default()
                .insert(rel.entity_b_id.clone());
            adjacency
                .entry(rel.entity_b_id.clone())
                .or_default()
                .insert(rel.entity_a_id.clone());
        }
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut communities = Vec::new();

    for entity in &entities {
        if visited.contains(&entity.entity_id) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![entity.entity_id.clone()];
        while let Some(node) = stack.pop() {
            if visited.contains(&node) {
                continue;
            }
            visited.insert(node.clone());
            component.push(node.clone());
            if let Some(neighbors) = adjacency.get(&node) {
                for n in neighbors {
                    if !visited.contains(n) {
                        stack.push(n.clone());
                    }
                }
            }
        }

        if component.len() >= min_size {
            let community = Community {
                community_id: Uuid::new_v4().to_string(),
                org_id: org_id.to_string(),
                entity_ids: component,
                summary: None,
                level: 0,
            };
            store.save_community(&community).await?;
            communities.push(community);
        }
    }

    tracing::info!(org_id, count = communities.len(), "communities detected");
    Ok(communities)
}
