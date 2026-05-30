use std::collections::HashMap;

use crate::pipeline::types::ScoredCandidate;

/// Reciprocal Rank Fusion: merge dense + sparse candidate lists.
/// RRF(d) = sum_over_lists( 1 / (k + rank_in_list) )
pub fn reciprocal_rank_fusion(
    dense: &[ScoredCandidate],
    sparse: &[ScoredCandidate],
    k: f32,
    sparse_weight: f32,
) -> Vec<ScoredCandidate> {
    let mut scores: HashMap<String, (f32, ScoredCandidate)> = HashMap::new();

    let dense_weight = 1.0 - sparse_weight;

    for (rank, c) in dense.iter().enumerate() {
        let rrf = dense_weight / (k + rank as f32 + 1.0);
        scores
            .entry(c.knowledge_id.clone())
            .and_modify(|(s, _)| *s += rrf)
            .or_insert((rrf, c.clone()));
    }

    for (rank, c) in sparse.iter().enumerate() {
        let rrf = sparse_weight / (k + rank as f32 + 1.0);
        scores
            .entry(c.knowledge_id.clone())
            .and_modify(|(s, existing)| {
                *s += rrf;
                existing.sparse_score = c.sparse_score;
            })
            .or_insert_with(|| {
                let mut merged = c.clone();
                merged.sparse_score = c.sparse_score;
                (rrf, merged)
            });
    }

    let mut fused: Vec<ScoredCandidate> = scores
        .into_values()
        .map(|(score, mut c)| {
            c.final_score = score;
            c
        })
        .collect();

    fused.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    fused
}
