//! Semantic *response* cache — the Data-Plane-v2-owned vector tier for the
//! model-gateway's `SemanticCache` seam.
//!
//! The gateway is barred from embedding its own vector store (see its
//! `retrieval.rs`: "the gateway does NOT embed a second RAG/vector store"), so
//! the *semantic* (similarity) cache lives here, beside the embeddings + Qdrant
//! this service already owns. A near-duplicate prompt (cosine ≥ threshold) from
//! the same org + model + embedding namespace **and authz scope** returns the
//! previously-cached LLM response — the response text rides in the Qdrant point
//! payload. The gateway's
//! Dragonfly exact-match KV tier stays where it is; this adds fuzzy recall.
//!
//! Both entry points are best-effort: callers treat any `Err`/`None` as a miss
//! and fall through to inference. Disabled unless `SEMANTIC_CACHE_ENABLED=true`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use qdrant_client::qdrant::{
    value::Kind as QdrantKind, Condition, CountPointsBuilder, CreateCollectionBuilder,
    DeletePointsBuilder, Distance, FieldCondition, Filter, Match, PointStruct, Range,
    SearchPointsBuilder, UpsertPointsBuilder, Value as QdrantValue, VectorParamsBuilder,
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

fn semantic_cache_allowed(zdr: bool) -> bool {
    !zdr
}

/// Deterministic point id from the cache coordinates so re-storing the same
/// `(namespace, org, model, scope, prompt)` overwrites in place rather than
/// accumulating duplicate points. A 64-bit collision only ever yields a cache
/// miss/overwrite (never a cross-tenant leak — search is org- and scope-filtered).
fn point_id(namespace: &str, org_id: &str, model: &str, scope: &str, prompt: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for part in [namespace, org_id, model, scope, prompt] {
        part.hash(&mut hasher);
        0u8.hash(&mut hasher); // domain separator between fields
    }
    hasher.finish()
}

/// Resolve the authorization scope that partitions the cache so a response
/// grounded on one principal's *visible document set* can never be served to a
/// different set — the cross-user leak the original org-only key allowed.
///
/// - `Some(key)` — the caller's scope token: a per-user visible-set hash, or the
///   literal `"org-shared"` when the answer was grounded only on org-public docs.
/// - `None` — no scope supplied. Fail closed when `semantic_cache_require_scope`
///   (the default) → returns `None` so the cache no-ops rather than risk a leak.
///   Operators of a single-tenant / org-shared-only deployment may set it false
///   to fall back to org-wide sharing under the reserved `__org_wide__` bucket.
fn resolve_scope(cfg: &Config, scope_key: Option<&str>) -> Option<String> {
    match scope_key.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(s.to_string()),
        None if cfg.semantic_cache_require_scope => None,
        None => Some("__org_wide__".to_string()),
    }
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
/// `org_id` + `model` + the active embedding namespace + the authz `scope_key`.
/// Returns `None` on a miss, when the cache is disabled, when no scope is
/// supplied and the cache requires one (fail-closed), when nothing has been
/// stored yet, or when the nearest hit is older than the configured TTL.
#[allow(clippy::too_many_arguments)]
pub async fn search(
    qdrant: &Qdrant,
    embedder: &EmbeddingClient,
    cfg: &Config,
    org_id: &str,
    model: &str,
    prompt: &str,
    zdr: bool,
    scope_key: Option<&str>,
) -> anyhow::Result<Option<SemanticHit>> {
    if !semantic_cache_allowed(zdr) || !cfg.semantic_cache_enabled {
        return Ok(None);
    }
    // Authz gate: no scope under a require-scope deployment ⇒ fail closed before
    // any network/embedder work, so a forgetful caller degrades to a miss (never
    // a cross-principal hit).
    let Some(scope) = resolve_scope(cfg, scope_key) else {
        return Ok(None);
    };
    let collection = &cfg.semantic_cache_collection;
    // Nothing stored yet → clean miss without touching the embedder.
    if !qdrant.collection_exists(collection).await? {
        return Ok(None);
    }

    let namespace = embedder.cache_namespace();
    // ZDR prompts must not egress to a retaining embedding provider; the embed
    // layer's egress guard enforces it on the direct-Azure backend.
    let vector = embedder
        .embed_query(org_id, prompt, zdr)
        .await
        .context("embed prompt for semantic-cache search")?;

    let filter = Filter {
        must: vec![
            keyword("org_id", org_id),
            keyword("model", model),
            keyword("embed_namespace", &namespace),
            keyword("scope", &scope),
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
/// the cache is disabled, the response is empty, or no authz scope is supplied
/// under a require-scope deployment.
// Each arg is a distinct concern (ZDR egress flag + authz scope) for an internal
// best-effort cache helper with two call sites — a params struct would add
// indirection without value.
#[allow(clippy::too_many_arguments)]
pub async fn store(
    qdrant: &Qdrant,
    embedder: &EmbeddingClient,
    cfg: &Config,
    org_id: &str,
    model: &str,
    prompt: &str,
    response: &str,
    zdr: bool,
    scope_key: Option<&str>,
) -> anyhow::Result<()> {
    if !semantic_cache_allowed(zdr) || !cfg.semantic_cache_enabled || response.is_empty() {
        return Ok(());
    }
    // Authz gate (see `search`): never persist a response under a scope we
    // cannot attribute, or it could later be served cross-principal.
    let Some(scope) = resolve_scope(cfg, scope_key) else {
        return Ok(());
    };
    let collection = &cfg.semantic_cache_collection;
    let namespace = embedder.cache_namespace();
    // ZDR prompts must not egress to a retaining embedding provider; the embed
    // layer's egress guard enforces it on the direct-Azure backend.
    let vector = embedder
        .embed_query(org_id, prompt, zdr)
        .await
        .context("embed prompt for semantic-cache store")?;
    ensure_collection(qdrant, collection, vector.len() as u64).await?;

    let mut payload: HashMap<String, QdrantValue> = HashMap::new();
    payload.insert("org_id".into(), str_val(org_id));
    payload.insert("model".into(), str_val(model));
    payload.insert("embed_namespace".into(), str_val(namespace.clone()));
    payload.insert("scope".into(), str_val(scope.clone()));
    payload.insert("response".into(), str_val(response));
    payload.insert("created_at".into(), int_val(now_secs()));

    let id = point_id(&namespace, org_id, model, &scope, prompt);
    let point = PointStruct::new(id, vector, payload);
    qdrant
        .upsert_points(UpsertPointsBuilder::new(collection, vec![point]).wait(false))
        .await
        .context("semantic-cache upsert")?;
    Ok(())
}

/// Delete cache points whose `created_at` is older than `older_than_secs`.
/// Returns how many were removed (0 when the collection does not exist). Qdrant
/// has no native per-point TTL, so a periodic caller (a cron hitting the admin
/// endpoint) keeps the collection from growing unbounded.
pub async fn prune(
    qdrant: &Qdrant,
    cfg: &Config,
    org_id: &str,
    older_than_secs: i64,
    dry_run: bool,
) -> anyhow::Result<u64> {
    let collection = &cfg.semantic_cache_collection;
    if !qdrant.collection_exists(collection).await? {
        return Ok(0);
    }
    let cutoff = now_secs().saturating_sub(older_than_secs.max(0));
    let filter = Filter {
        must: vec![
            keyword("org_id", org_id),
            Condition::from(FieldCondition {
                key: "created_at".to_string(),
                range: Some(Range {
                    lt: Some(cutoff as f64),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        ],
        ..Default::default()
    };
    // `delete_points` reports no count, so count the matches against the same
    // filter first (exact), then delete.
    let pruned = qdrant
        .count(
            CountPointsBuilder::new(collection)
                .filter(filter.clone())
                .exact(true),
        )
        .await
        .context("count stale semantic-cache points")?
        .result
        .map_or(0, |r| r.count);
    if !dry_run {
        qdrant
            .delete_points(
                DeletePointsBuilder::new(collection)
                    .points(filter)
                    .wait(true),
            )
            .await
            .context("prune stale semantic-cache points")?;
        if pruned > 0 {
            tracing::info!(collection, pruned, cutoff, "semantic-cache pruned");
        }
    }
    Ok(pruned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg(require_scope: bool) -> Config {
        serde_json::from_value(serde_json::json!({
            "database_url": "",
            "qdrant_url": "",
            "semantic_cache_require_scope": require_scope,
        }))
        .expect("minimal config from defaults")
    }

    #[test]
    fn point_id_is_deterministic_and_field_scoped() {
        let base = point_id("ns", "org-1", "m", "scope-1", "hello");
        assert_eq!(
            base,
            point_id("ns", "org-1", "m", "scope-1", "hello"),
            "deterministic"
        );
        assert_ne!(
            base,
            point_id("ns", "org-2", "m", "scope-1", "hello"),
            "org-scoped"
        );
        assert_ne!(
            base,
            point_id("ns", "org-1", "m2", "scope-1", "hello"),
            "model-scoped"
        );
        assert_ne!(
            base,
            point_id("ns", "org-1", "m", "scope-2", "hello"),
            "authz-scope-sensitive"
        );
        assert_ne!(
            base,
            point_id("ns2", "org-1", "m", "scope-1", "hello"),
            "namespace-scoped"
        );
        assert_ne!(
            base,
            point_id("ns", "org-1", "m", "scope-1", "HELLO"),
            "prompt-sensitive"
        );
        // Domain separation: concatenation collisions must not occur.
        assert_ne!(
            point_id("a", "b", "c", "d", "e"),
            point_id("ab", "c", "d", "e", "")
        );
    }

    #[test]
    fn zdr_requests_bypass_semantic_cache_reads_and_writes() {
        assert!(!semantic_cache_allowed(true));
        assert!(semantic_cache_allowed(false));
    }

    #[test]
    fn resolve_scope_fails_closed_without_key() {
        let cfg = test_cfg(true);
        assert_eq!(resolve_scope(&cfg, None), None, "no scope → fail closed");
        assert_eq!(
            resolve_scope(&cfg, Some("   ")),
            None,
            "blank scope → fail closed"
        );
        assert_eq!(
            resolve_scope(&cfg, Some("u:abc")).as_deref(),
            Some("u:abc"),
            "explicit scope passes through"
        );
    }

    #[test]
    fn resolve_scope_opt_out_falls_back_to_org_wide() {
        let cfg = test_cfg(false);
        assert_eq!(
            resolve_scope(&cfg, None).as_deref(),
            Some("__org_wide__"),
            "opt-out → org-wide bucket"
        );
        assert_eq!(
            resolve_scope(&cfg, Some("u:abc")).as_deref(),
            Some("u:abc"),
            "explicit scope still honored under opt-out"
        );
    }
}
