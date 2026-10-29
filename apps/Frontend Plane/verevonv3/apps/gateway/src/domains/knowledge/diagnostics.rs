//! Live Data Plane v2 diagnostics for the Knowledge surface.
//!
//! Rust port of verevonv2's `loadKnowledgeDiagnostics`. Probes the readiness of
//! every Data Plane v2 service the knowledge stack depends on. Storage health is
//! derived only from those service contracts; Qdrant, Quickwit, and MinIO remain
//! private to `dpv2-net` and are never exposed to the frontend plane.

use std::time::Duration;

use reqwest::Method;
use serde_json::{json, Value};

use crate::{config::AppState, contracts::ActionActor};

pub(super) struct DiagInput {
    pub(super) document_count: i64,
    pub(super) graph_available: bool,
    pub(super) graph_node_count: i64,
    pub(super) graph_edge_count: i64,
    pub(super) indexed_count: i64,
}

pub(super) async fn load_diagnostics(
    state: &AppState,
    _actor: &ActionActor,
    input: DiagInput,
) -> Value {
    let documents_url = format!("{}/readyz", state.documents_api_url);
    let retrieval_url = format!("{}/readyz", state.retrieval_engine_url);
    let embedding_url = format!("{}/readyz", state.embedding_engine_url);
    let graph_url = format!("{}/readyz", state.graph_index_url);
    let wiki_url = format!("{}/readyz", state.wiki_store_url);
    let quickwit_adapter_url = format!("{}/readyz", state.quickwit_adapter_url);
    let (documents, retrieval, embedding, graph, wiki, quickwit_adapter) = tokio::join!(
        probe(state, &documents_url),
        probe(state, &retrieval_url),
        probe(state, &embedding_url),
        probe(state, &graph_url),
        probe(state, &wiki_url),
        probe(state, &quickwit_adapter_url),
    );

    let documents_ready = is_ready(&documents);
    let retrieval_ready = is_ready(&retrieval);
    let embedding_ready = is_ready(&embedding);
    let graph_ready = is_ready(&graph);
    let wiki_ready = is_ready(&wiki);
    let quickwit_adapter_ready = is_ready(&quickwit_adapter);

    let sparse_backend = retrieval
        .body
        .as_ref()
        .and_then(|b| b.get("sparse_backend"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|s| !s.is_empty());
    let qdrant_check = retrieval
        .body
        .as_ref()
        .and_then(|b| b.get("checks"))
        .and_then(|c| c.get("qdrant"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let vector_collections: Vec<String> = Vec::new();
    let quickwit_indexes = quickwit_adapter_index_ids(&quickwit_adapter);

    let services = json!([
        service_item(
            "documents-api",
            "Documents API",
            documents_ready,
            "Canonical store for documents, graph, wiki, traces, and evals is reachable.",
            "documents-api readyz did not confirm a healthy status.",
            "documents, graph, wiki, traces",
        ),
        service_item(
            "retrieval-engine",
            "Retrieval engine",
            retrieval_ready,
            &format!(
                "Sparse backend {} is serving live retrieval.",
                sparse_backend
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned())
            ),
            "retrieval-engine readyz did not confirm a healthy status.",
            &service_label(&retrieval, "retrieval-engine-rs"),
        ),
        service_item(
            "embedding-engine",
            "Embedding engine",
            embedding_ready,
            &format!(
                "Qdrant dependency is {} through retrieval-engine readiness.",
                if qdrant_check {
                    "healthy"
                } else {
                    "unconfirmed"
                }
            ),
            "embedding-engine readyz did not confirm a healthy status.",
            &service_label(&embedding, "embedding-engine-rs"),
        ),
        service_item(
            "graph-index",
            "Graph index",
            graph_ready,
            &format!(
                "GraphRAG entity store is live with {} entities and {} relations.",
                format_count(input.graph_node_count),
                format_count(input.graph_edge_count)
            ),
            "graph-index readyz did not confirm a healthy status.",
            &service_label(&graph, "graph-index-rs"),
        ),
        service_item(
            "wiki-store",
            "Wiki store",
            wiki_ready,
            "Wiki CRUD and publish flow are live in Data Plane v2.",
            "wiki-store readyz did not confirm a healthy status.",
            &service_label(&wiki, "wiki-store-go"),
        ),
        service_item(
            "quickwit-adapter",
            "Quickwit adapter",
            quickwit_adapter_ready,
            &format!(
                "{} Quickwit indexes are visible from the adapter.",
                format_count(quickwit_indexes.len() as i64)
            ),
            "Quickwit sparse search adapter did not report a healthy status.",
            &service_label(&quickwit_adapter, "quickwit-adapter-rs"),
        ),
    ]);

    let capabilities = json!([
        capability_item(
            "embedding-system",
            "Embedding system",
            embedding_ready && qdrant_check,
            "Dense embeddings and the Qdrant dependency are confirmed through Data Plane readiness.",
            "Embedding pipeline is not confirmed online.",
        ),
        capability_item(
            "sparse-retrieval",
            "Sparse retrieval",
            sparse_backend.is_some() && quickwit_adapter_ready,
            &format!(
                "Sparse backend {} is wired through the Quickwit adapter.",
                sparse_backend
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned())
            ),
            "Sparse retrieval backend is not confirmed online.",
        ),
        capability_item(
            "graphrag",
            "GraphRAG",
            graph_ready && input.graph_available,
            "Graph-aware retrieval can fuse entity relationships into answers.",
            "GraphRAG is offline or the org has no graph yet.",
        ),
        capability_item(
            "agentic-rag",
            "Agentic RAG",
            retrieval_ready,
            "Multi-step retrieval with source resolution and chunk expansion is live.",
            "Agentic retrieval loop is not confirmed online.",
        ),
        capability_item(
            "context-pack",
            "Context pack",
            retrieval_ready,
            "Token-budgeted context packs can be assembled for model calls.",
            "Context-pack assembly is not confirmed online.",
        ),
        capability_item(
            "llm-wiki",
            "LLM Wiki",
            wiki_ready,
            "Generated wiki pages with versions, diffs, and backlinks are live.",
            "Wiki generation store is not confirmed online.",
        ),
    ]);

    let storage =
        json!([
        storage_item(
            "qdrant",
            "Qdrant vectors",
            qdrant_check,
            "retrieval-engine readiness confirms its private Qdrant dependency.",
            "Qdrant could not be confirmed through the retrieval contract.",
            "contract check",
        ),
        storage_item(
            "quickwit",
            "Quickwit indexes",
            !quickwit_indexes.is_empty(),
            &format!("Sparse indexes include {}.", join_names(&quickwit_indexes, 3)),
            "Quickwit indexes could not be confirmed from the live runtime.",
            &format!("{} indexes", format_count(quickwit_indexes.len() as i64)),
        ),
        storage_item(
            "minio",
            "MinIO object storage",
            false,
            "MinIO is healthy through a Data Plane service contract.",
            "MinIO is private to Data Plane and no scoped storage-health contract is available.",
            "private backend",
        ),
    ]);

    let available = documents_ready
        || retrieval_ready
        || embedding_ready
        || graph_ready
        || wiki_ready
        || quickwit_adapter_ready
        || input.document_count > 0
        || input.indexed_count > 0;

    json!({
        "available": available,
        "services": services,
        "capabilities": capabilities,
        "storage": storage,
        "vectorCollections": vector_collections,
        "quickwitIndexes": quickwit_indexes,
        "sparseBackend": sparse_backend,
    })
}

// ── Probing ────────────────────────────────────────────────────────────────

struct Probe {
    status: u16,
    body: Option<Value>,
}

/// GET a health/listing endpoint with a tight timeout. Captures the body even on
/// non-2xx so fields like `sparse_backend` survive a `503 not_ready` response.
async fn probe(state: &AppState, url: &str) -> Probe {
    match state
        .client
        .request(Method::GET, url)
        .timeout(Duration::from_millis(1_800))
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.json::<Value>().await.ok();
            Probe { status, body }
        }
        Err(_) => Probe {
            status: 0,
            body: None,
        },
    }
}

fn is_ready(probe: &Probe) -> bool {
    if probe.status != 200 {
        return false;
    }
    match &probe.body {
        Some(body) => {
            let status_ok = body
                .get("status")
                .and_then(Value::as_str)
                .map(|s| s.eq_ignore_ascii_case("ready") || s.eq_ignore_ascii_case("ok"))
                .unwrap_or(false);
            let ready_flag = body.get("ready").and_then(Value::as_bool).unwrap_or(false);
            // Qdrant `/collections` and similar listing endpoints have no status
            // field but a 200 with a body is healthy.
            status_ok || ready_flag || body.get("status").is_none()
        }
        None => true,
    }
}

fn service_label(probe: &Probe, fallback: &str) -> String {
    probe
        .body
        .as_ref()
        .and_then(|b| b.get("service"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| fallback.to_owned())
}

fn quickwit_adapter_index_ids(probe: &Probe) -> Vec<String> {
    probe
        .body
        .as_ref()
        .and_then(|body| body.get("index"))
        .and_then(Value::as_str)
        .filter(|index| !index.is_empty())
        .map(|index| vec![index.to_owned()])
        .unwrap_or_default()
}

// ── Item builders ────────────────────────────────────────────────────────

fn service_item(
    id: &str,
    label: &str,
    ready: bool,
    detail_ready: &str,
    detail_down: &str,
    meta: &str,
) -> Value {
    json!({
        "id": id,
        "label": label,
        "status": if ready { "Healthy" } else { "Unavailable" },
        "tone": if ready { "good" } else { "bad" },
        "detail": if ready { detail_ready } else { detail_down },
        "meta": meta,
    })
}

fn capability_item(
    id: &str,
    label: &str,
    ready: bool,
    detail_ready: &str,
    detail_down: &str,
) -> Value {
    json!({
        "id": id,
        "label": label,
        "status": if ready { "Enabled" } else { "Degraded" },
        "tone": if ready { "good" } else { "warn" },
        "detail": if ready { detail_ready } else { detail_down },
    })
}

fn storage_item(
    id: &str,
    label: &str,
    ready: bool,
    detail_ready: &str,
    detail_down: &str,
    meta: &str,
) -> Value {
    json!({
        "id": id,
        "label": label,
        "status": if ready { "Healthy" } else { "Unconfirmed" },
        "tone": if ready { "good" } else { "warn" },
        "detail": if ready { detail_ready } else { detail_down },
        "meta": meta,
    })
}

fn join_names(names: &[String], limit: usize) -> String {
    if names.is_empty() {
        return "none".to_owned();
    }
    let shown = names
        .iter()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let extra = names.len().saturating_sub(limit);
    if extra > 0 {
        format!("{shown} +{extra} more")
    } else {
        shown
    }
}

fn format_count(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let bytes = digits.as_bytes();
    let mut grouped = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(*b as char);
    }
    if value < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_quickwit_index_from_adapter_contract() {
        let probe = Probe {
            status: 200,
            body: Some(json!({
                "status": "ready",
                "service": "quickwit-adapter-rs",
                "index": "dataplane-corpus"
            })),
        };

        assert_eq!(quickwit_adapter_index_ids(&probe), vec!["dataplane-corpus"]);
    }

    #[test]
    fn missing_adapter_index_is_honestly_empty() {
        let probe = Probe {
            status: 503,
            body: Some(json!({"status": "not_ready"})),
        };

        assert!(quickwit_adapter_index_ids(&probe).is_empty());
    }
}
