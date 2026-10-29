use anyhow::Context;
use qdrant_client::qdrant::{
    Condition, FieldCondition, Filter, Match, SearchPointsBuilder, Value as QdrantValue,
};
use qdrant_client::Qdrant;

use crate::pipeline::types::ScoredCandidate;

/// Similarity cutoff for the dense arm (plan P1-7).
///
/// Applied here, at the Qdrant query, rather than after fusion — because this is
/// the only place in the pipeline where the score is *calibrated*. Cosine
/// similarity is an absolute 0..1 quantity, so "below 0.30 is not a real match"
/// is a statement that means something. RRF's `final_score` is not: a rank-1
/// fused score is ~0.03 (1/(60+1)), so any absolute threshold on it would be
/// arbitrary and would trip on good results.
///
/// Purpose: when nothing in the corpus actually answers the query, the dense arm
/// previously still returned its `top_k` nearest neighbours — whatever they were
/// — and RRF happily ranked them. That fed the model confident-looking but
/// irrelevant context instead of an honest empty result.
///
/// `None`/unset keeps the previous behaviour exactly, so this is opt-in.
fn dense_score_threshold() -> Option<f32> {
    static THRESHOLD: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        let raw = std::env::var("DENSE_SCORE_THRESHOLD").ok()?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        match trimmed.parse::<f32>() {
            // A threshold outside 0..1 cannot be a cosine score. Refusing it is
            // safer than silently filtering everything (>1) or nothing (<0).
            Ok(v) if (0.0..=1.0).contains(&v) => Some(v),
            Ok(v) => {
                tracing::warn!(
                    value = v,
                    "DENSE_SCORE_THRESHOLD outside 0.0..=1.0; ignoring (cosine scores are 0..1)"
                );
                None
            }
            Err(_) => {
                tracing::warn!(value = %trimmed, "DENSE_SCORE_THRESHOLD is not a number; ignoring");
                None
            }
        }
    })
}

#[tracing::instrument(
    name = "qdrant.search",
    skip(qdrant, query_vector, filter_conditions),
    fields(
        otel.kind = "client",
        db.system = "qdrant",
        qdrant.collection = collection,
        org_id = org_id,
        top_k = top_k,
    ),
)]
pub async fn vector_search(
    qdrant: &Qdrant,
    collection: &str,
    query_vector: Vec<f32>,
    org_id: &str,
    filter_conditions: Vec<Condition>,
    top_k: usize,
) -> anyhow::Result<Vec<ScoredCandidate>> {
    let mut must = vec![Condition::from(FieldCondition {
        key: "org_id".to_string(),
        r#match: Some(Match {
            match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Keyword(
                org_id.to_string(),
            )),
        }),
        ..Default::default()
    })];
    must.extend(filter_conditions);

    let filter = Filter {
        must,
        ..Default::default()
    };

    let mut search = SearchPointsBuilder::new(collection, query_vector, top_k as u64)
        .filter(filter)
        .with_payload(true);
    // P1-7: let Qdrant drop sub-threshold neighbours server-side rather than
    // returning them for the pipeline to rank anyway.
    if let Some(threshold) = dense_score_threshold() {
        search = search.score_threshold(threshold);
    }

    let results = qdrant
        .search_points(search)
        .await
        .context("qdrant search failed")?;

    let candidates = results
        .result
        .into_iter()
        .map(|point| {
            let payload = &point.payload;
            ScoredCandidate {
                knowledge_id: get_str(payload, "knowledge_id"),
                document_id: get_str(payload, "document_id"),
                text: get_str(payload, "text"),
                dense_score: point.score,
                sparse_score: 0.0,
                rerank_score: 0.0,
                final_score: point.score,
                chunk_index: get_i64(payload, "chunk_index") as i32,
                metadata: payload.clone(),
            }
        })
        .collect();

    Ok(candidates)
}

fn get_str(payload: &std::collections::HashMap<String, QdrantValue>, key: &str) -> String {
    payload
        .get(key)
        .and_then(|v| v.kind.as_ref())
        .and_then(|k| match k {
            qdrant_client::qdrant::value::Kind::StringValue(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn get_i64(payload: &std::collections::HashMap<String, QdrantValue>, key: &str) -> i64 {
    payload
        .get(key)
        .and_then(|v| v.kind.as_ref())
        .and_then(|k| match k {
            qdrant_client::qdrant::value::Kind::IntegerValue(i) => Some(*i),
            _ => None,
        })
        .unwrap_or(0)
}
