//! Reciprocal Rank Fusion (RRF) for hybrid search (OSS-parity P0 1C).
//!
//! Merges Quarry's lexical recall (`TantivyLocalIndex` / SERP `SearchResult`s)
//! with Data Plane semantic hits (`VectorHit`) into one ranking. RRF needs no
//! score calibration between the two systems — it fuses by rank position:
//!
//!   fused(d) = Σ_lists 1 / (k + rank_list(d))
//!
//! `k=60` is the canonical constant from the original RRF paper (Cormack et al.).

use std::collections::HashMap;

use crate::serp::SearchResult;
use crate::vector_index::VectorHit;

/// Canonical RRF dampening constant.
pub const RRF_K: f32 = 60.0;

struct Acc {
    score: f32,
    title: Option<String>,
    snippet: Option<String>,
}

/// Fuse lexical + vector result lists via RRF. Dedup is by URL; metadata
/// prefers whichever list first supplied a non-empty title/snippet. Ties in
/// fused score keep first-seen order (lexical list first). Returns the top
/// `limit` as `SearchResult`s tagged `provider = "hybrid"`.
pub fn rrf_fuse(
    lexical: &[SearchResult],
    vector: &[VectorHit],
    k: f32,
    limit: usize,
) -> Vec<SearchResult> {
    let mut acc: HashMap<String, Acc> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    let mut bump = |url: &str, rank: usize, title: &Option<String>, snippet: &Option<String>| {
        let e = acc.entry(url.to_string()).or_insert_with(|| {
            order.push(url.to_string());
            Acc { score: 0.0, title: None, snippet: None }
        });
        e.score += 1.0 / (k + rank as f32);
        if e.title.is_none() {
            e.title = title.clone();
        }
        if e.snippet.is_none() {
            e.snippet = snippet.clone();
        }
    };

    for (i, r) in lexical.iter().enumerate() {
        bump(&r.url, i + 1, &r.title, &r.snippet);
    }
    for (i, h) in vector.iter().enumerate() {
        bump(&h.url, i + 1, &h.title, &h.snippet);
    }

    // Build in first-seen order, then stable-sort by fused score desc so ties
    // preserve lexical-first ordering.
    let mut items: Vec<(f32, SearchResult)> = order
        .iter()
        .map(|url| {
            let a = &acc[url];
            (
                a.score,
                SearchResult {
                    url: url.clone(),
                    title: a.title.clone(),
                    snippet: a.snippet.clone(),
                    rank: 0,
                    provider: "hybrid".into(),
                },
            )
        })
        .collect();
    items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    items
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(i, (_, mut r))| {
            r.rank = (i + 1) as u32;
            r
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sr(url: &str) -> SearchResult {
        SearchResult {
            url: url.into(),
            title: Some(url.into()),
            snippet: None,
            rank: 0,
            provider: "lex".into(),
        }
    }
    fn vh(url: &str, score: f32) -> VectorHit {
        VectorHit {
            url: url.into(),
            score,
            title: None,
            snippet: Some(format!("snip-{url}")),
            document_id: None,
        }
    }

    #[test]
    fn item_in_both_lists_ranks_first() {
        // "b" appears in both → highest fused score.
        let lex = vec![sr("a"), sr("b"), sr("c")];
        let vec = vec![vh("b", 0.9), vh("d", 0.8)];
        let fused = rrf_fuse(&lex, &vec, RRF_K, 10);
        assert_eq!(fused[0].url, "b");
        assert_eq!(fused[0].provider, "hybrid");
    }

    #[test]
    fn union_includes_vector_only_items() {
        let lex = vec![sr("a")];
        let vec = vec![vh("z", 0.5)];
        let fused = rrf_fuse(&lex, &vec, RRF_K, 10);
        let urls: Vec<&str> = fused.iter().map(|r| r.url.as_str()).collect();
        assert!(urls.contains(&"a") && urls.contains(&"z"));
    }

    #[test]
    fn empty_vector_preserves_lexical_order() {
        let lex = vec![sr("a"), sr("b"), sr("c")];
        let fused = rrf_fuse(&lex, &[], RRF_K, 10);
        let urls: Vec<&str> = fused.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["a", "b", "c"]);
    }

    #[test]
    fn metadata_merges_snippet_from_vector() {
        let lex = vec![sr("a")]; // snippet None
        let vec = vec![vh("a", 0.9)]; // snippet Some
        let fused = rrf_fuse(&lex, &vec, RRF_K, 10);
        assert_eq!(fused[0].snippet.as_deref(), Some("snip-a"));
    }

    #[test]
    fn limit_truncates_and_reranks() {
        let lex = vec![sr("a"), sr("b"), sr("c")];
        let fused = rrf_fuse(&lex, &[], RRF_K, 2);
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].rank, 1);
        assert_eq!(fused[1].rank, 2);
    }
}
