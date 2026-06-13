//! Semantic *response* cache — the Data-Plane-v2-owned vector tier for the
//! model-gateway's `SemanticCache` seam.
//!
//! The gateway is barred from embedding its own vector store (see its
//! `retrieval.rs`: "the gateway does NOT embed a second RAG/vector store"), so
//! the *semantic* (similarity) cache lives here, beside the embeddings + Qdrant
//! this service already owns. A near-duplicate prompt (cosine ≥ threshold) from
//! the same org + model + embedding namespace returns the previously-cached LLM
//! response — the response text rides in the Qdrant point payload. The gateway's
//! Dragonfly exact-match KV tier stays where it is; this adds fuzzy recall.
//!
//! Both entry points are best-effort: callers treat any `Err`/`None` as a miss
//! and fall through to inference. Disabled unless `SEMANTIC_CACHE_ENABLED=true`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use qdrant_client::qdrant::{
    value::Kind as QdrantKind, Condition, CreateCollectionBuilder, Distance, FieldCondition,
    Filter, Match, PointStruct, SearchPointsBuilder, UpsertPointsBuilder, Value as QdrantValue,
    VectorParamsBuilder,
};
use qdrant_client::Qdrant;

use crate::config::Config;
use crate::embed::EmbeddingClient;

/// A semantic-cache hit: the cached response plus the cosine score that cleared
/// the configured threshold.
pub struct SemanticHit {
    pub response: String,
    pub score: f32,
}

fn keyword(key: &str, value: &str) -> Condition {
    Condition::from(FieldCondition {
        key: key.to_string(),
        r#match: Some(Match {
            match_value: Some(qdrant_client::qdrant::r#match::MatchValue::Keyword(
                value.to_string(),
            )),
        }),
        ..Default::default()
    })
}

fn str_val(value: impl Into<String>) -> QdrantValue {
    QdrantValue {
        kind: Some(QdrantKind::StringValue(value.into())),
    }
}

fn int_val(value: i64) -> QdrantValue {
    QdrantValue {
        kind: Some(QdrantKind::IntegerValue(value)),
    }
}

fn get_str(payload: &HashMap<String, QdrantValue>, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| v.kind.as_ref())
        .and_then(|k| match k {
            QdrantKind::StringValue(s) => Some(s.clone()),
            _ => None,
        })
}

fn get_i64(payload: &HashMap<String, QdrantValue>, key: &str) -> i64 {
    payload
        .get(key)
        .and_then(|v| v.kind.as_ref())
        .and_then(|k| match k {
            QdrantKind::IntegerValue(i) => Some(*i),
            _ => None,
        })
        .unwrap_or(0)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Deterministic point id from the cache coordinates so re-storing the same
/// `(namespace, org, model, prompt)` overwrites in place rather than
/// accumulating duplicate points. A 64-bit collision only ever yields a cache
/// miss/overwrite (never a cross-tenant leak — search is org-filtered).
fn point_id(namespace: &str, org_id: &str, model: &str, prompt: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in [namespace, org_id, model, prompt] {
        part.hash(&mut hasher);
        0u8.hash(&mut hasher); // domain separator between fields
    }
    hasher.finish()
}

async fn ensure_collection(qdrant: &Qdrant, collection: &str, dim: u64) -> anyhow::Result<()> {
    if !qdrant.collection_exists(collection).await? {
        qdrant
            .create_collection(
                CreateCollectionBuilder::new(collection)
                    .vectors_config(VectorParamsBuilder::new(dim, Distance::Cosine)),
            )
            .await
            .context("create semantic-cache collection")?;
        tracing::info!(collection, "semantic-cache collection created");
    }
    Ok(())
}

/// Look up a cached response for a semantically-near prompt, scoped to
/// `org_id` + `model` + the active embedding namespace. Returns `None` on a
/// miss, when the cache is disabled, when nothing has been stored yet, or when
/// the nearest hit is older than the configured TTL.
pub async fn search(
    qdrant: &Qdrant,
    embedder: &EmbeddingClient,
    cfg: &Config,
    org_id: &str,
    model: &str,
    prompt: &str,
) -> anyhow::Result<Option<SemanticHit>> {
    if !cfg.semantic_cache_enabled {
        return Ok(None);
    }
    let collection = &cfg.semantic_cache_collection;
    // Nothing stored yet → clean miss without touching the embedder.
    if !qdrant.collection_exists(collection).await? {
        return Ok(None);
    }

    let namespace = embedder.cache_namespace();
    let vector = embedder
        .embed_query(org_id, prompt)
        .await
        .context("embed prompt for semantic-cache search")?;

    let filter = Filter {
        must: vec![
            keyword("org_id", org_id),
            keyword("model", model),
            keyword("embed_namespace", &namespace),
        ],
        ..Default::default()
    };

    let search = SearchPointsBuilder::new(collection, vector, 1)
        .filter(filter)
        .score_threshold(cfg.semantic_cache_min_score)
        .with_payload(true);

    let results = qdrant
        .search_points(search)
        .await
        .context("semantic-cache search")?;

    let Some(point) = results.result.into_iter().next() else {
        return Ok(None);
    };

    // Logical TTL: a Qdrant point has no native expiry, so honor the window in
    // process. A stale nearest-neighbour degrades to a miss (inference reruns
    // and `store` overwrites the point in place via its deterministic id).
    if cfg.semantic_cache_ttl_secs > 0 {
        let created = get_i64(&point.payload, "created_at");
        if created > 0 && now_secs().saturating_sub(created) > cfg.semantic_cache_ttl_secs as i64 {
            return Ok(None);
        }
    }

    Ok(get_str(&point.payload, "response")
        .filter(|r| !r.is_empty())
        .map(|response| SemanticHit {
            response,
            score: point.score,
        }))
}

/// Store a prompt/response pair for future semantically-near hits. Best-effort;
/// lazily creates the collection sized to the embedding dimension. A no-op when
/// the cache is disabled or the response is empty.
pub async fn store(
    qdrant: &Qdrant,
    embedder: &EmbeddingClient,
    cfg: &Config,
    org_id: &str,
    model: &str,
    prompt: &str,
    response: &str,
) -> anyhow::Result<()> {
    if !cfg.semantic_cache_enabled || response.is_empty() {
        return Ok(());
    }
    let collection = &cfg.semantic_cache_collection;
    let namespace = embedder.cache_namespace();
    let vector = embedder
        .embed_query(org_id, prompt)
        .await
        .context("embed prompt for semantic-cache store")?;
    ensure_collection(qdrant, collection, vector.len() as u64).await?;

    let mut payload: HashMap<String, QdrantValue> = HashMap::new();
    payload.insert("org_id".into(), str_val(org_id));
    payload.insert("model".into(), str_val(model));
    payload.insert("embed_namespace".into(), str_val(namespace.clone()));
    payload.insert("response".into(), str_val(response));
    payload.insert("created_at".into(), int_val(now_secs()));

    let id = point_id(&namespace, org_id, model, prompt);
    let point = PointStruct::new(id, vector, payload);
    qdrant
        .upsert_points(UpsertPointsBuilder::new(collection, vec![point]).wait(false))
        .await
        .context("semantic-cache upsert")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_id_is_deterministic_and_field_scoped() {
        let base = point_id("ns", "org-1", "m", "hello");
        assert_eq!(base, point_id("ns", "org-1", "m", "hello"), "deterministic");
        assert_ne!(base, point_id("ns", "org-2", "m", "hello"), "org-scoped");
        assert_ne!(base, point_id("ns", "org-1", "m2", "hello"), "model-scoped");
        assert_ne!(
            base,
            point_id("ns2", "org-1", "m", "hello"),
            "namespace-scoped"
        );
        assert_ne!(
            base,
            point_id("ns", "org-1", "m", "HELLO"),
            "prompt-sensitive"
        );
        // Domain separation: concatenation collisions must not occur.
        assert_ne!(point_id("a", "b", "c", "d"), point_id("ab", "c", "d", ""));
    }
}
