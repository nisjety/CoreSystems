use anyhow::Context;
use qdrant_client::qdrant::{
    Condition, FieldCondition, Filter, Match, SearchPointsBuilder, Value as QdrantValue,
};
use qdrant_client::Qdrant;

use crate::pipeline::types::ScoredCandidate;

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

    let search = SearchPointsBuilder::new(collection, query_vector, top_k as u64)
        .filter(filter)
        .with_payload(true);

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
