//! Knowledge workspace aggregator.
//!
//! `GET /api/v1/knowledge/sources` returns the full `LiveKnowledgePayload` the
//! Velion v3 Knowledge surface renders: document-derived sources with chunk
//! previews + freshness, the entity graph, finspo storage analytics, connected
//! integrations, recent files, web (crawl) sources, sync metrics, and a live
//! Data Plane v2 diagnostics report.
//!
//! This is the Rust port of velionv2's `loadKnowledgeWorkspace`
//! (`src/lib/knowledge/knowledge-workspace.ts`). The SPA only speaks to the
//! gateway, so the cross-plane fan-out that v2 did in its Next.js BFF lives
//! here instead. Upstream paths/fields are pinned to the authoritative Data
//! Plane v2 service contracts (e.g. freshness is `POST /v1/retrieve/freshness`,
//! documents-api responses are enveloped as `{documents|sources, total}`).

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use axum::{
    extract::{Extension, State},
    http::HeaderMap,
    response::IntoResponse,
    Json,
};
use futures_util::future::join_all;
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    domains::knowledge::{
        diagnostics,
        shared::{self, fetch_json},
    },
    middleware::AuthenticatedUser,
};

const KNOWLEDGE_SOURCE_LIMIT: usize = 8;
const GRAPH_NODE_LIMIT: usize = 40;
const GRAPH_EDGE_LIMIT: usize = 80;
const GRAPH_SOURCE_REF_LIMIT: usize = 80;
const CHUNK_PREVIEW_LIMIT: usize = 3;
const RING_RADII: [f64; 3] = [110.0, 155.0, 195.0];

pub(super) async fn load_workspace(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let actor = shared::actor_for(&user);
    let cookie = shared::cookie_header(&headers);
    let org_opt = (!org_id.trim().is_empty()).then(|| org_id.clone());

    // Stage 1 — documents + integration summary load regardless of org scope.
    let (documents, integration) = tokio::join!(
        load_documents(&state, org_opt.as_deref(), &actor),
        load_integration_summary(&state, org_opt.as_deref(), &actor),
    );
    let indexed_count = count_indexed(&documents);
    let generated_at = chrono::Utc::now().to_rfc3339();

    if org_opt.is_none() {
        let diagnostics = diagnostics::load_diagnostics(
            &state,
            &actor,
            diagnostics::DiagInput {
                document_count: documents.len() as i64,
                graph_available: false,
                graph_node_count: 0,
                graph_edge_count: 0,
                indexed_count,
            },
        )
        .await;

        let payload = json!({
            "generatedAt": generated_at,
            "orgId": Value::Null,
            "collections": build_collections(&documents, &[]),
            "dataPlane": { "available": false, "documentCount": documents.len(), "indexedCount": indexed_count },
            "graph": empty_graph(),
            "metrics": integration.metrics_json(),
            "metricCards": build_metric_cards(documents.len() as i64, indexed_count, 0, &GraphMetrics { available: false, node_count: 0, edge_count: 0 }, 0, 0, &integration),
            "folders": build_folder_cards(&[], &documents),
            "integrations": build_integration_cards(&integration.connections, &documents, 0),
            "files": build_files(&documents),
            "sources": Value::Array(vec![]),
            "webSources": Value::Array(vec![]),
            "diagnostics": diagnostics,
            "finspo": empty_finspo(),
        });
        return Json(payload);
    }

    let org = org_opt.clone().unwrap();

    // Stage 2 — graph snapshot, finspo analytics, quarry web sources (org-scoped).
    let (graph_snapshot, finspo, quarry_sources) = tokio::join!(
        load_graph_snapshot(&state, &org, &actor),
        load_finspo(&state, &org, &actor),
        load_quarry_sources(&state, &user, &cookie),
    );

    // Resolve graph entity source refs → documents so nodes can cite real sources.
    let graph_refs = collect_graph_source_refs(graph_snapshot.as_ref());
    let chunk_lookup = load_graph_reference_chunks(&state, &org, &actor, &graph_refs).await;
    let graph = build_graph(graph_snapshot.as_ref(), &chunk_lookup);

    let selected = sort_by_recency(&documents)
        .into_iter()
        .take(KNOWLEDGE_SOURCE_LIMIT)
        .cloned()
        .collect::<Vec<_>>();
    let selected_ids = selected.iter().map(|d| d.id.clone()).collect::<Vec<_>>();

    // Stage 3 — per-document chunk previews, freshness rows, diagnostics.
    let (chunk_previews, freshness, diag) = tokio::join!(
        load_document_chunk_previews(&state, &org, &actor, &selected),
        load_freshness(&state, &org, &actor, &selected_ids),
        diagnostics::load_diagnostics(
            &state,
            &actor,
            diagnostics::DiagInput {
                document_count: documents.len() as i64,
                graph_available: graph.available,
                graph_node_count: graph.node_count,
                graph_edge_count: graph.edge_count,
                indexed_count,
            },
        ),
    );

    // Compute every graph-derived value before the payload literal moves `graph.value`.
    let graph_metrics = graph.value_for_metrics();
    let sources = build_sources(
        &selected,
        &chunk_previews,
        &freshness,
        &graph.related_labels,
    );
    let web_sources = build_web_sources(&quarry_sources);

    let payload = json!({
        "generatedAt": generated_at,
        "orgId": org,
        "collections": build_collections(&documents, &web_sources),
        "dataPlane": { "available": !documents.is_empty(), "documentCount": documents.len(), "indexedCount": indexed_count },
        "graph": graph.value,
        "metrics": integration.metrics_json(),
        "metricCards": build_metric_cards(
            documents.len() as i64,
            indexed_count,
            finspo.duplicate_groups as i64,
            &graph_metrics,
            finspo.recommendation_count as i64,
            finspo.reclaimable_bytes,
            &integration,
        ),
        "folders": build_folder_cards(&finspo.aggregates, &documents),
        "integrations": build_integration_cards(&integration.connections, &documents, finspo.source_count),
        "files": build_files(&documents),
        "sources": Value::Array(sources),
        "webSources": Value::Array(web_sources),
        "diagnostics": diag,
        "finspo": json!({
            "available": finspo.available(),
            "sourceCount": finspo.source_count,
            "largestCount": finspo.largest_count,
            "inactiveCount": finspo.inactive_count,
            "duplicateGroups": finspo.duplicate_groups,
            "recommendationCount": finspo.recommendation_count,
            "reclaimableBytes": finspo.reclaimable_bytes,
        }),
    });

    Json(payload)
}

// ── Loaders ───────────────────────────────────────────────────────────────

#[derive(Clone)]
struct DocSummary {
    id: String,
    title: String,
    source: String,
    doc_type: String,
    status: String,
    content: String,
    created_by: String,
    created_at: String,
    updated_at: String,
}

async fn load_documents(
    state: &AppState,
    org: Option<&str>,
    actor: &ActionActor,
) -> Vec<DocSummary> {
    let url = format!(
        "{}/v1/documents?limit=100&offset=0",
        state.documents_api_url
    );
    let payload = fetch_json(
        state,
        Method::GET,
        &url,
        None,
        org,
        actor,
        Duration::from_millis(2_500),
    )
    .await;
    array_from(payload.as_ref(), "documents")
        .iter()
        .filter_map(|doc| {
            let id = str_any(doc, &["document_id", "id"]);
            if id.is_empty() {
                return None;
            }
            Some(DocSummary {
                id,
                title: {
                    let t = str_at(doc, "title");
                    if t.is_empty() {
                        "Untitled".to_owned()
                    } else {
                        t
                    }
                },
                source: str_at(doc, "source"),
                doc_type: str_any(doc, &["type", "kind"]),
                status: str_at(doc, "status"),
                content: str_at(doc, "content"),
                created_by: str_any(doc, &["created_by", "createdBy"]),
                created_at: str_any(doc, &["created_at", "createdAt"]),
                updated_at: str_any(doc, &["updated_at", "updatedAt"]),
            })
        })
        .collect()
}

struct Connection {
    id: String,
    provider_key: String,
    provider_label: String,
    display_name: String,
    status: String,
    sync_status: String,
    latest_sync_updated_at: String,
    deleted: bool,
}

struct IntegrationSummary {
    connected: i64,
    syncing: i64,
    failed: i64,
    connections: Vec<Connection>,
}

impl IntegrationSummary {
    fn metrics_json(&self) -> Value {
        json!({ "connected": self.connected, "syncing": self.syncing, "failed": self.failed })
    }
}

async fn load_integration_summary(
    state: &AppState,
    org: Option<&str>,
    actor: &ActionActor,
) -> IntegrationSummary {
    let Some(org) = org else {
        return IntegrationSummary {
            connected: 0,
            syncing: 0,
            failed: 0,
            connections: vec![],
        };
    };
    let connections_url = format!(
        "{}/api/v1/connections?organizationId={}",
        state.integration_core_url,
        urlencoding::encode(org)
    );
    let sync_jobs_url = format!(
        "{}/api/v1/sync-jobs?organizationId={}",
        state.integration_core_url,
        urlencoding::encode(org)
    );
    let (connections_payload, sync_jobs_payload) = tokio::join!(
        fetch_json(
            state,
            Method::GET,
            &connections_url,
            None,
            Some(org),
            actor,
            Duration::from_millis(3_000)
        ),
        fetch_json(
            state,
            Method::GET,
            &sync_jobs_url,
            None,
            Some(org),
            actor,
            Duration::from_millis(3_000)
        ),
    );

    // connectionId → latest sync-job updated_at, for freshness display.
    let mut latest_jobs: HashMap<String, String> = HashMap::new();
    for job in array_from(sync_jobs_payload.as_ref(), "syncJobs")
        .iter()
        .chain(array_from(sync_jobs_payload.as_ref(), "sync_jobs").iter())
    {
        let connection_id = str_any(job, &["connectionId", "connection_id"]);
        if connection_id.is_empty() {
            continue;
        }
        let updated = str_any(job, &["updatedAt", "updated_at"]);
        latest_jobs.entry(connection_id).or_insert(updated);
    }

    let connections = array_from(connections_payload.as_ref(), "connections")
        .iter()
        .filter_map(|conn| {
            let id = str_any(conn, &["id", "connectionId", "connection_id"]);
            if id.is_empty() {
                return None;
            }
            let provider_key = str_any(conn, &["providerKey", "provider_key"]);
            let status = str_at(conn, "status");
            let sync_status = str_any(conn, &["syncStatus", "sync_status"]);
            let deleted_at = str_any(conn, &["deletedAt", "deleted_at"]);
            let latest_sync_updated_at = nested_str(
                conn,
                &["latestSyncJob", "latest_sync_job"],
                &["updatedAt", "updated_at"],
            )
            .filter(|s| !s.is_empty())
            .or_else(|| latest_jobs.get(&id).cloned())
            .unwrap_or_default();
            Some(Connection {
                id,
                provider_label: {
                    let l = str_any(conn, &["providerLabel", "provider_label"]);
                    if l.is_empty() {
                        provider_key.clone()
                    } else {
                        l
                    }
                },
                display_name: str_any(conn, &["displayName", "display_name"]),
                status: status.clone(),
                sync_status,
                latest_sync_updated_at,
                deleted: !deleted_at.is_empty(),
                provider_key,
            })
        })
        .collect::<Vec<_>>();

    let mut connected = 0i64;
    let mut syncing = 0i64;
    let mut failed = 0i64;
    for conn in connections.iter().filter(|c| !c.deleted) {
        let bucket = integration_card_status(if conn.sync_status.is_empty() {
            &conn.status
        } else {
            &conn.sync_status
        });
        match bucket {
            "Syncing" => syncing += 1,
            "Review" => failed += 1,
            _ => connected += 1,
        }
    }

    IntegrationSummary {
        connected,
        syncing,
        failed,
        connections,
    }
}

async fn load_graph_snapshot(state: &AppState, org: &str, actor: &ActionActor) -> Option<Value> {
    let url = format!(
        "{}/v1/graphs/{}?limit_nodes=120&limit_edges=240",
        state.graph_index_url,
        urlencoding::encode(org)
    );
    fetch_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org),
        actor,
        Duration::from_millis(2_500),
    )
    .await
}

/// Map graph entity source refs (knowledge ids) → owning document ids.
async fn load_graph_reference_chunks(
    state: &AppState,
    org: &str,
    actor: &ActionActor,
    knowledge_ids: &[String],
) -> HashMap<String, String> {
    if knowledge_ids.is_empty() {
        return HashMap::new();
    }
    let body = json!({
        "org_id": org,
        "knowledge_ids": knowledge_ids.iter().take(GRAPH_SOURCE_REF_LIMIT).collect::<Vec<_>>(),
    });
    let url = format!("{}/v1/retrieve/chunks", state.retrieval_engine_url);
    let payload = fetch_json(
        state,
        Method::POST,
        &url,
        Some(body),
        Some(org),
        actor,
        Duration::from_millis(4_000),
    )
    .await;
    array_from(payload.as_ref(), "chunks")
        .iter()
        .filter_map(|chunk| {
            let knowledge_id = str_at(chunk, "knowledge_id");
            let document_id = str_at(chunk, "document_id");
            if knowledge_id.is_empty() || document_id.is_empty() {
                None
            } else {
                Some((knowledge_id, document_id))
            }
        })
        .collect()
}

struct ChunkPreview {
    count: i64,
    previews: Vec<Value>,
}

async fn load_document_chunk_previews(
    state: &AppState,
    org: &str,
    actor: &ActionActor,
    documents: &[DocSummary],
) -> HashMap<String, ChunkPreview> {
    let futures = documents.iter().map(|doc| async move {
        let body = json!({
            "org_id": org,
            "document_ids": [doc.id],
            "limit": CHUNK_PREVIEW_LIMIT,
            "offset": 0,
        });
        let url = format!("{}/v1/retrieve/chunks", state.retrieval_engine_url);
        let payload = fetch_json(
            state,
            Method::POST,
            &url,
            Some(body),
            Some(org),
            actor,
            Duration::from_millis(4_000),
        )
        .await;
        (
            doc.id.clone(),
            chunk_preview_from_payload(payload.as_ref(), &doc.content),
        )
    });
    join_all(futures).await.into_iter().collect()
}

async fn load_freshness(
    state: &AppState,
    org: &str,
    actor: &ActionActor,
    document_ids: &[String],
) -> HashMap<String, f64> {
    if document_ids.is_empty() {
        return HashMap::new();
    }
    let body = json!({ "org_id": org, "document_ids": document_ids });
    let url = format!("{}/v1/retrieve/freshness", state.retrieval_engine_url);
    let payload = fetch_json(
        state,
        Method::POST,
        &url,
        Some(body),
        Some(org),
        actor,
        Duration::from_millis(3_000),
    )
    .await;
    array_from(payload.as_ref(), "freshness")
        .iter()
        .filter_map(|row| {
            let document_id = str_at(row, "document_id");
            if document_id.is_empty() {
                None
            } else {
                Some((document_id, num_at(row, "freshness_score")))
            }
        })
        .collect()
}

struct FinspoData {
    source_count: usize,
    largest_count: usize,
    inactive_count: usize,
    duplicate_groups: usize,
    recommendation_count: usize,
    reclaimable_bytes: i64,
    aggregates: Vec<Value>,
}

impl FinspoData {
    fn available(&self) -> bool {
        self.source_count > 0
            || !self.aggregates.is_empty()
            || self.largest_count > 0
            || self.inactive_count > 0
            || self.duplicate_groups > 0
            || self.recommendation_count > 0
    }
}

async fn load_finspo(state: &AppState, org: &str, actor: &ActionActor) -> FinspoData {
    async fn finspo_get(
        state: &AppState,
        org: &str,
        actor: &ActionActor,
        path: &str,
    ) -> Option<Value> {
        let url = format!("{}{}", state.finspo_core_url, path);
        let payload = fetch_json(
            state,
            Method::GET,
            &url,
            None,
            Some(org),
            actor,
            Duration::from_millis(4_000),
        )
        .await?;
        // finspo wraps results in `{ data: ... }`.
        Some(payload.get("data").cloned().unwrap_or(payload))
    }

    let (sources, aggregates, largest, inactive, duplicates, recommendations) = tokio::join!(
        finspo_get(state, org, actor, "/api/v1/sources"),
        finspo_get(state, org, actor, "/api/v1/analytics/by-site"),
        finspo_get(state, org, actor, "/api/v1/analytics/largest?limit=8"),
        finspo_get(
            state,
            org,
            actor,
            "/api/v1/analytics/inactive?limit=8&older_than=4320h"
        ),
        finspo_get(
            state,
            org,
            actor,
            "/api/v1/analytics/duplicates?max_groups=8&min_count=2"
        ),
        finspo_get(
            state,
            org,
            actor,
            "/api/v1/recommendations?max_groups=8&inactive_limit=8&older_than=4320h"
        ),
    );

    let drafts = array_from(recommendations.as_ref(), "drafts");
    let reclaimable_bytes = drafts
        .iter()
        .map(|d| num_at(d, "estimated_bytes") as i64)
        .sum();

    FinspoData {
        source_count: array_from(sources.as_ref(), "sources").len(),
        largest_count: array_from(largest.as_ref(), "items").len(),
        inactive_count: array_from(inactive.as_ref(), "items").len(),
        duplicate_groups: array_from(duplicates.as_ref(), "groups").len(),
        recommendation_count: drafts.len(),
        reclaimable_bytes,
        aggregates: array_from(aggregates.as_ref(), "aggregates"),
    }
}

async fn load_quarry_sources(
    state: &AppState,
    user: &AuthenticatedUser,
    cookie: &str,
) -> Vec<Value> {
    let token = shared::quarry_token(state, user, cookie).await;
    let url = format!("{}/v1/sources?limit=24", state.quarry_edge_url);
    let mut req = state
        .client
        .request(Method::GET, &url)
        .timeout(Duration::from_millis(2_500))
        .header("x-user-id", user.user_id.as_str());
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            let payload = resp.json::<Value>().await.ok();
            array_from(payload.as_ref(), "items")
        }
        _ => vec![],
    }
}

// ── Builders ────────────────────────────────────────────────────────────────

fn build_metric_cards(
    data_plane_count: i64,
    indexed_count: i64,
    duplicate_groups: i64,
    graph: &GraphMetrics,
    recommendation_count: i64,
    reclaimable_bytes: i64,
    integration: &IntegrationSummary,
) -> Value {
    let pending = (data_plane_count - indexed_count).max(0);
    json!([
        {
            "label": "Indexed documents",
            "value": format_count(indexed_count),
            "delta": if pending > 0 { format!("{} pending", format_count(pending)) } else { "All indexed".to_owned() },
            "tone": if pending > 0 { "warn" } else { "good" },
        },
        {
            "label": "Connected integrations",
            "value": format_count(integration.connected),
            "delta": if integration.syncing > 0 { format!("{} syncing", format_count(integration.syncing)) } else { "Healthy".to_owned() },
            "tone": if integration.failed > 0 { "warn" } else { "good" },
        },
        {
            "label": "Graph entities",
            "value": format_count(graph.node_count),
            "delta": if graph.available { format!("{} edges", format_count(graph.edge_count)) } else { "Graph offline".to_owned() },
            "tone": if graph.available { "good" } else { "warn" },
        },
        {
            "label": "Reclaim opportunities",
            "value": if reclaimable_bytes > 0 { format_bytes(reclaimable_bytes) } else { format_count(duplicate_groups) },
            "delta": if recommendation_count > 0 { format!("{} recommendations", format_count(recommendation_count)) } else { "No cleanup queued".to_owned() },
            "tone": if recommendation_count > 0 { "warn" } else { "good" },
        },
    ])
}

fn build_collections(documents: &[DocSummary], web_sources: &[Value]) -> Value {
    let mut counts: HashMap<String, (String, i64)> = HashMap::new();
    for doc in documents {
        let provider_key = {
            let k = normalize_provider_key(&doc.source);
            if k.is_empty() {
                "docs".to_owned()
            } else {
                k
            }
        };
        let entry = counts
            .entry(provider_key.clone())
            .or_insert((source_label(&provider_key), 0));
        entry.1 += 1;
    }
    if !web_sources.is_empty() {
        counts.insert(
            "web".to_owned(),
            ("Web sources".to_owned(), web_sources.len() as i64),
        );
    }

    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then_with(|| a.1 .0.cmp(&b.1 .0)));

    let mut out = vec![json!({
        "id": "all",
        "label": "General Knowledge",
        "count": documents.len() as i64 + web_sources.len() as i64,
    })];
    for (provider_key, (label, count)) in ranked.into_iter().take(6) {
        out.push(json!({
            "id": if provider_key == "web" { "web".to_owned() } else { format!("provider:{provider_key}") },
            "label": label,
            "count": count,
        }));
    }
    Value::Array(out)
}

fn build_folder_cards(aggregates: &[Value], documents: &[DocSummary]) -> Value {
    let tones = ["warm", "green", "blue", "gray"];
    let mut folders: Vec<Value> = vec![];

    for aggregate in aggregates.iter().take(2) {
        let tone = tones[folders.len() % tones.len()];
        let title = {
            let drive = str_at(aggregate, "drive_name");
            if !drive.is_empty() {
                drive
            } else {
                let site = str_at(aggregate, "site_id");
                if site.is_empty() {
                    "SharePoint drive".to_owned()
                } else {
                    site
                }
            }
        };
        let id = {
            let sid = str_at(aggregate, "source_id");
            if sid.is_empty() {
                format!("finspo:{}", folders.len())
            } else {
                sid
            }
        };
        folders.push(json!({
            "id": id,
            "title": title,
            "subtitle": "SharePoint / OneDrive",
            "providerKey": "microsoft",
            "primaryValue": format_count(num_at(aggregate, "file_count") as i64),
            "primaryLabel": "Files",
            "secondaryValue": format_bytes(num_at(aggregate, "total_bytes") as i64),
            "secondaryLabel": "Stored",
            "connections": ["Microsoft 365", "Finspo"],
            "tone": tone,
        }));
    }

    let groups = group_documents_by_source(documents);
    let remaining = 4usize.saturating_sub(folders.len());
    for (source, group) in groups.into_iter().take(remaining) {
        let tone = tones[folders.len() % tones.len()];
        let provider_key = {
            let k = normalize_provider_key(&source);
            if k.is_empty() {
                source.clone()
            } else {
                k
            }
        };
        folders.push(json!({
            "id": format!("source:{source}"),
            "title": source_label(&source),
            "subtitle": format!("{} retrieval documents", format_count(group.len() as i64)),
            "providerKey": provider_key,
            "primaryValue": format_count(group.len() as i64),
            "primaryLabel": "Docs",
            "secondaryValue": format_count(count_indexed(&group)),
            "secondaryLabel": "Indexed",
            "connections": [source_label(&source), "Data Plane"],
            "tone": tone,
        }));
    }

    Value::Array(folders)
}

fn build_integration_cards(
    connections: &[Connection],
    documents: &[DocSummary],
    finspo_source_count: usize,
) -> Value {
    let mut counts_by_provider: HashMap<String, i64> = HashMap::new();
    for (source, group) in group_documents_by_source(documents) {
        counts_by_provider.insert(normalize_provider_key(&source), group.len() as i64);
    }

    let mut cards: Vec<Value> = connections
        .iter()
        .filter(|c| !c.deleted)
        .map(|conn| {
            let provider_key = normalize_provider_key(&conn.provider_key);
            let document_count = counts_by_provider.get(&provider_key).copied().unwrap_or(0);
            let status = integration_card_status(if conn.sync_status.is_empty() {
                &conn.status
            } else {
                &conn.sync_status
            });
            let detail = if provider_key == "microsoft" && finspo_source_count > 0 {
                format!("{} drives · {} docs", format_count(finspo_source_count as i64), format_count(document_count))
            } else {
                format!("{} docs", format_count(document_count))
            };
            let name = if !conn.display_name.is_empty() { &conn.display_name } else { &conn.provider_label };
            json!({
                "id": conn.id,
                "name": name,
                "providerKey": provider_key,
                "status": status,
                "documents": detail,
                "freshness": if conn.latest_sync_updated_at.is_empty() { "Live".to_owned() } else { relative_time(&conn.latest_sync_updated_at) },
                "detail": conn.provider_label,
            })
        })
        .collect();

    let has_microsoft = cards
        .iter()
        .any(|c| c.get("providerKey").and_then(Value::as_str) == Some("microsoft"));
    if !has_microsoft && finspo_source_count > 0 {
        cards.push(json!({
            "id": "finspo:microsoft",
            "name": "Microsoft 365",
            "providerKey": "microsoft",
            "status": "Connected",
            "documents": format!("{} drives", format_count(finspo_source_count as i64)),
            "freshness": "Finspo",
            "detail": "SharePoint / OneDrive",
        }));
    }

    cards.truncate(8);
    Value::Array(cards)
}

fn build_files(documents: &[DocSummary]) -> Value {
    let files = sort_by_recency(documents)
        .into_iter()
        .take(10)
        .map(|doc| {
            let provider_key = {
                let k = normalize_provider_key(&doc.source);
                if k.is_empty() { doc.source.clone() } else { k }
            };
            json!({
                "id": doc.id,
                "name": doc.title,
                "addedBy": if doc.created_by.is_empty() { "System".to_owned() } else { doc.created_by.clone() },
                "source": source_label(&doc.source),
                "providerKey": provider_key,
                "updated": relative_time(if doc.updated_at.is_empty() { &doc.created_at } else { &doc.updated_at }),
                "type": document_source_type(doc),
            })
        })
        .collect::<Vec<_>>();
    Value::Array(files)
}

fn build_sources(
    documents: &[DocSummary],
    chunk_previews: &HashMap<String, ChunkPreview>,
    freshness: &HashMap<String, f64>,
    related_labels: &HashMap<String, Vec<String>>,
) -> Vec<Value> {
    documents
        .iter()
        .map(|doc| {
            let fallback = preview_from_content(&doc.content);
            let preview = chunk_previews.get(&doc.id).unwrap_or(&fallback);
            let freshness_score = freshness.get(&doc.id).copied().unwrap_or(0.8);
            let content_size = doc.content.len() as i64;
            let provider_key = {
                let k = normalize_provider_key(&doc.source);
                if k.is_empty() { doc.source.clone() } else { k }
            };
            let tags = compact(&[
                source_label(&doc.source),
                doc.doc_type.clone(),
                format_document_status(&doc.status),
            ]);
            let related = related_labels
                .get(&doc.id)
                .map(|labels| labels.iter().take(3).cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            json!({
                "id": doc.id,
                "title": doc.title,
                "description": format!("{} · {}", source_label(&doc.source), format_document_status(&doc.status)),
                "type": document_source_type(doc),
                "provider": source_label(&doc.source),
                "providerKey": provider_key,
                "category": if doc.doc_type.is_empty() { "document".to_owned() } else { doc.doc_type.clone() },
                "owner": if doc.created_by.is_empty() { "System".to_owned() } else { doc.created_by.clone() },
                "updated": relative_time(if doc.updated_at.is_empty() { &doc.created_at } else { &doc.updated_at }),
                "size": format_bytes(content_size),
                "status": knowledge_source_status(&doc.status),
                "chunks": preview.count,
                "hitRate": format!("{}%", (freshness_score * 100.0).round() as i64),
                "coverage": if preview.count > 0 { "100%" } else { "0%" },
                "similarity": format!("{:.2}", freshness_score),
                "tags": tags.into_iter().take(3).collect::<Vec<_>>(),
                "related": related,
                "chunksPreview": preview.previews.clone(),
            })
        })
        .collect()
}

fn build_web_sources(quarry_sources: &[Value]) -> Vec<Value> {
    let mut out = quarry_sources
        .iter()
        .filter_map(|source| {
            let id = str_at(source, "source_id");
            let url = str_at(source, "url");
            if id.is_empty() || url.is_empty() {
                return None;
            }
            let name = {
                let n = str_at(source, "name");
                if n.is_empty() {
                    host_label(&url)
                } else {
                    n
                }
            };
            let updated_source = {
                let u = str_at(source, "updated_at");
                if u.is_empty() {
                    str_at(source, "created_at")
                } else {
                    u
                }
            };
            let kind = {
                let k = str_at(source, "kind");
                if k.is_empty() {
                    "crawl".to_owned()
                } else {
                    k
                }
            };
            let status = {
                let s = str_at(source, "status");
                if s.is_empty() {
                    "unknown".to_owned()
                } else {
                    s
                }
            };
            Some(json!({
                "id": id,
                "kind": kind,
                "name": name,
                "providerKey": "web",
                "status": status,
                "updated": relative_time(&updated_source),
                "url": url,
            }))
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| {
        let an = a.get("name").and_then(Value::as_str).unwrap_or("");
        let bn = b.get("name").and_then(Value::as_str).unwrap_or("");
        an.cmp(bn)
    });
    out
}

struct GraphMetrics {
    available: bool,
    node_count: i64,
    edge_count: i64,
}

struct BuiltGraph {
    value: Value,
    available: bool,
    node_count: i64,
    edge_count: i64,
    related_labels: HashMap<String, Vec<String>>,
}

impl BuiltGraph {
    fn value_for_metrics(&self) -> GraphMetrics {
        GraphMetrics {
            available: self.available,
            node_count: self.node_count,
            edge_count: self.edge_count,
        }
    }
}

fn build_graph(snapshot: Option<&Value>, chunk_lookup: &HashMap<String, String>) -> BuiltGraph {
    let Some(snapshot) = snapshot else {
        return BuiltGraph {
            value: empty_graph(),
            available: false,
            node_count: 0,
            edge_count: 0,
            related_labels: HashMap::new(),
        };
    };

    // Filter + cap nodes (drop org-level nodes, require id+label).
    struct RawNode {
        id: String,
        label: String,
        group: String,
        source_refs: Vec<String>,
    }
    let filtered_nodes = array_from(Some(snapshot), "nodes")
        .iter()
        .map(|node| {
            let id = str_any(node, &["entity_id", "id"]);
            let label = {
                let text = str_any(node, &["entity_text", "text"]);
                if text.is_empty() {
                    id.clone()
                } else {
                    text
                }
            };
            let group = {
                let g = str_any(node, &["entity_type", "type"]);
                if g.is_empty() {
                    "entity".to_owned()
                } else {
                    g.to_lowercase()
                }
            };
            RawNode {
                id,
                label,
                group,
                source_refs: str_array(node, "source_refs"),
            }
        })
        .filter(|n| !n.id.is_empty() && !n.label.is_empty() && n.group != "org")
        .take(GRAPH_NODE_LIMIT)
        .collect::<Vec<_>>();
    let node_ids: HashSet<&str> = filtered_nodes.iter().map(|n| n.id.as_str()).collect();

    // Edges where both endpoints survive node filtering.
    struct RawLink {
        from: String,
        to: String,
        label: String,
        strength: i64,
        source_refs: Vec<String>,
    }
    let links_raw = array_from(Some(snapshot), "edges")
        .iter()
        .map(|edge| {
            let confidence = {
                let c = num_at(edge, "confidence");
                if c == 0.0 {
                    0.4
                } else {
                    c
                }
            };
            RawLink {
                from: str_at(edge, "entity_a_id"),
                to: str_at(edge, "entity_b_id"),
                label: {
                    let r = str_at(edge, "relation_type");
                    if r.is_empty() {
                        "related".to_owned()
                    } else {
                        r
                    }
                },
                strength: ((confidence * 3.0).round() as i64).max(1),
                source_refs: str_array(edge, "source_refs"),
            }
        })
        .filter(|e| {
            !e.from.is_empty()
                && !e.to.is_empty()
                && node_ids.contains(e.from.as_str())
                && node_ids.contains(e.to.as_str())
        })
        .take(GRAPH_EDGE_LIMIT)
        .collect::<Vec<_>>();

    let mut degrees: HashMap<String, i64> = HashMap::new();
    for link in &links_raw {
        *degrees.entry(link.from.clone()).or_insert(0) += 1;
        *degrees.entry(link.to.clone()).or_insert(0) += 1;
    }

    let mut groups: Vec<String> = vec![];
    for node in &filtered_nodes {
        if !groups.contains(&node.group) {
            groups.push(node.group.clone());
        }
    }
    groups.truncate(8);
    let group_index: HashMap<&str, usize> = groups
        .iter()
        .enumerate()
        .map(|(i, g)| (g.as_str(), i))
        .collect();
    let total = filtered_nodes.len().max(1) as f64;

    let mut related_labels: HashMap<String, Vec<String>> = HashMap::new();
    let nodes = filtered_nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let group_order = *group_index.get(node.group.as_str()).unwrap_or(&0);
            let angle = (std::f64::consts::PI * 2.0 * index as f64) / total;
            let ring = RING_RADII[group_order % RING_RADII.len()];
            let degree = *degrees.get(&node.id).unwrap_or(&0) as f64;
            let radius = clamp(
                16.0 + degree * 1.5 + (node.source_refs.len().min(4) as f64),
                16.0,
                32.0,
            );

            let mut source_ids: Vec<String> = vec![];
            let mut seen = HashSet::new();
            for r in &node.source_refs {
                if let Some(doc_id) = chunk_lookup.get(r) {
                    if seen.insert(doc_id.clone()) {
                        source_ids.push(doc_id.clone());
                    }
                }
            }
            for doc_id in &source_ids {
                related_labels
                    .entry(doc_id.clone())
                    .or_default()
                    .push(node.label.clone());
            }

            json!({
                "id": node.id,
                "label": node.label,
                "group": node.group,
                "tone": graph_tone(&node.group),
                "x": (320.0 + angle.cos() * ring).round() as i64,
                "y": (210.0 + angle.sin() * (ring * 0.78)).round() as i64,
                "radius": radius,
                "sourceRefs": node.source_refs.clone(),
                "sourceIds": source_ids,
            })
        })
        .collect::<Vec<_>>();

    // Dedup related labels per document while preserving insertion order.
    for labels in related_labels.values_mut() {
        let mut seen = HashSet::new();
        labels.retain(|l| seen.insert(l.clone()));
    }

    let links = links_raw
        .iter()
        .map(|link| {
            json!({
                "from": link.from,
                "to": link.to,
                "label": link.label,
                "strength": link.strength,
                "sourceRefs": link.source_refs.clone(),
            })
        })
        .collect::<Vec<_>>();

    let node_count = {
        let declared = num_at(snapshot, "node_count") as i64;
        if declared > 0 {
            declared
        } else {
            nodes.len() as i64
        }
    };
    let edge_count = {
        let declared = num_at(snapshot, "edge_count") as i64;
        if declared > 0 {
            declared
        } else {
            links.len() as i64
        }
    };
    let available = !nodes.is_empty() || !links.is_empty();

    BuiltGraph {
        value: json!({
            "available": available,
            "nodeCount": node_count,
            "edgeCount": edge_count,
            "groups": groups,
            "nodes": nodes,
            "links": links,
            "truncated": snapshot.get("truncated").and_then(Value::as_bool).unwrap_or(false),
        }),
        available,
        node_count,
        edge_count,
        related_labels,
    }
}

// ── Chunk previews ────────────────────────────────────────────────────────

fn preview_from_content(content: &str) -> ChunkPreview {
    let parts = content
        .split("\n\n")
        .map(normalize_whitespace)
        .filter(|p| !p.is_empty())
        .take(CHUNK_PREVIEW_LIMIT)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return ChunkPreview {
            count: 0,
            previews: vec![json!({
                "id": "chunk-0",
                "title": "Awaiting chunk preview",
                "score": "#0",
                "text": "The retrieval engine has not returned chunks for this document yet.",
            })],
        };
    }
    ChunkPreview {
        count: parts.len() as i64,
        previews: parts
            .iter()
            .enumerate()
            .map(|(i, part)| {
                json!({
                    "id": format!("chunk-{}", i + 1),
                    "title": format!("Excerpt {}", i + 1),
                    "score": format!("#{}", i + 1),
                    "text": part,
                })
            })
            .collect(),
    }
}

fn chunk_preview_from_payload(payload: Option<&Value>, content: &str) -> ChunkPreview {
    let chunks = array_from(payload, "chunks");
    if chunks.is_empty() {
        return preview_from_content(content);
    }
    let declared = payload.map(|p| num_at(p, "count") as i64).unwrap_or(0);
    ChunkPreview {
        count: declared.max(chunks.len() as i64),
        previews: chunks
            .iter()
            .take(CHUNK_PREVIEW_LIMIT)
            .map(|chunk| {
                let index = num_at(chunk, "chunk_index") as i64;
                let id = {
                    let k = str_at(chunk, "knowledge_id");
                    if k.is_empty() {
                        format!("chunk-{}", index + 1)
                    } else {
                        k
                    }
                };
                json!({
                    "id": id,
                    "title": format!("Chunk {}", index + 1),
                    "score": format!("#{}", index + 1),
                    "text": normalize_whitespace(&str_at(chunk, "text")),
                })
            })
            .collect(),
    }
}

// ── Helpers / classifiers ─────────────────────────────────────────────────

fn group_documents_by_source(documents: &[DocSummary]) -> Vec<(String, Vec<DocSummary>)> {
    let mut groups: HashMap<String, Vec<DocSummary>> = HashMap::new();
    let mut order: Vec<String> = vec![];
    for doc in documents {
        let key = {
            let k = normalize_provider_key(&doc.source);
            if k.is_empty() {
                doc.source.clone()
            } else {
                k
            }
        };
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(doc.clone());
    }
    let mut grouped = order
        .into_iter()
        .map(|key| {
            let group = groups.remove(&key).unwrap_or_default();
            (key, group)
        })
        .collect::<Vec<_>>();
    grouped.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    grouped
}

fn sort_by_recency(documents: &[DocSummary]) -> Vec<&DocSummary> {
    let mut out = documents.iter().collect::<Vec<_>>();
    out.sort_by(|a, b| {
        let av = sort_date_value(if a.updated_at.is_empty() {
            &a.created_at
        } else {
            &a.updated_at
        });
        let bv = sort_date_value(if b.updated_at.is_empty() {
            &b.created_at
        } else {
            &b.updated_at
        });
        bv.cmp(&av)
    });
    out
}

fn collect_graph_source_refs(snapshot: Option<&Value>) -> Vec<String> {
    let Some(snapshot) = snapshot else {
        return vec![];
    };
    let mut refs: Vec<String> = vec![];
    let mut seen: HashSet<String> = HashSet::new();
    for collection in ["nodes", "edges"] {
        for item in array_from(Some(snapshot), collection) {
            for r in str_array(&item, "source_refs") {
                if seen.insert(r.clone()) {
                    refs.push(r);
                    if refs.len() >= GRAPH_SOURCE_REF_LIMIT {
                        return refs;
                    }
                }
            }
        }
    }
    refs
}

fn count_indexed(documents: &[DocSummary]) -> i64 {
    documents
        .iter()
        .filter(|d| {
            matches!(
                d.status.to_lowercase().as_str(),
                "indexed" | "active" | "completed" | "chunked"
            )
        })
        .count() as i64
}

fn integration_card_status(sync_status: &str) -> &'static str {
    match sync_status.to_lowercase().as_str() {
        "queued" | "running" | "waiting_provider" | "handoff_data_plane" | "syncing" => "Syncing",
        "failed" | "error" | "needs_refresh" | "cancelled" => "Review",
        _ => "Connected",
    }
}

fn knowledge_source_status(status: &str) -> &'static str {
    match status.to_lowercase().as_str() {
        "indexed" | "active" | "completed" | "chunked" => "Indexed",
        "processing" | "running" | "queued" | "pending" => "Re-indexing",
        _ => "Pending review",
    }
}

fn document_source_type(doc: &DocSummary) -> &'static str {
    let t = doc.doc_type.to_lowercase();
    let s = doc.source.to_lowercase();
    if t.contains("pdf") {
        "PDF"
    } else if s.contains("notion") || t.contains("notion") {
        "Notion"
    } else if s.starts_with("http") || s.contains("website") || s.contains("crawler") {
        "URL"
    } else {
        "Docs"
    }
}

fn graph_tone(group: &str) -> &'static str {
    let has = |tokens: &[&str]| tokens.iter().any(|t| group.contains(t));
    if has(&["policy", "claim", "rule", "compliance"]) {
        "policy"
    } else if has(&["product", "feature", "plan", "sku"]) {
        "product"
    } else if has(&["risk", "fraud", "incident"]) {
        "risk"
    } else if has(&["workspace", "site", "drive", "channel", "folder"]) {
        "support"
    } else {
        "core"
    }
}

fn format_document_status(status: &str) -> String {
    let normalized = status
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect::<String>();
    let collapsed = normalize_whitespace(&normalized);
    if collapsed.is_empty() {
        return "Unknown".to_owned();
    }
    titlecase(&collapsed)
}

fn source_label(source: &str) -> String {
    match normalize_provider_key(source).as_str() {
        "microsoft" => "Microsoft 365".to_owned(),
        "google" => "Google Workspace".to_owned(),
        "notion" => "Notion".to_owned(),
        "github" => "GitHub".to_owned(),
        "slack" => "Slack".to_owned(),
        _ => {
            let stripped = source.strip_prefix("onboarding:").unwrap_or(source);
            let spaced = stripped
                .chars()
                .map(|c| {
                    if c == '_' || c == ':' || c == '-' {
                        ' '
                    } else {
                        c
                    }
                })
                .collect::<String>();
            titlecase(&spaced)
        }
    }
}

fn normalize_provider_key(value: &str) -> String {
    match value.trim().to_lowercase().as_str() {
        "m365" | "microsoft365" | "microsoft-365" | "microsoft-graph" | "onedrive" | "outlook"
        | "sharepoint" | "teams" => "microsoft".to_owned(),
        "gdrive" | "gmail" | "google-drive" | "google-workspace" => "google".to_owned(),
        other => other.to_owned(),
    }
}

fn titlecase(value: &str) -> String {
    value
        .split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter(|v| !v.trim().is_empty())
        .cloned()
        .collect()
}

fn host_label(value: &str) -> String {
    // Strip scheme then take the authority component.
    let without_scheme = value.split("://").nth(1).unwrap_or(value);
    let host = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme);
    if host.is_empty() {
        value.to_owned()
    } else {
        host.to_owned()
    }
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_count(value: i64) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let bytes = digits.as_bytes();
    let mut grouped = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(*b as char);
    }
    if negative {
        format!("-{grouped}")
    } else {
        grouped
    }
}

fn format_bytes(bytes: i64) -> String {
    if bytes <= 0 {
        return "0 B".to_owned();
    }
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < units.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    let digits = if value >= 10.0 || unit_index == 0 {
        0
    } else {
        1
    };
    format!("{:.*} {}", digits, value, units[unit_index])
}

fn relative_time(value: &str) -> String {
    if value.trim().is_empty() {
        return "Live".to_owned();
    }
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(value) else {
        return "Live".to_owned();
    };
    let seconds = (chrono::Utc::now().timestamp() - parsed.timestamp()).max(0);
    if seconds < 90 {
        return "Just now".to_owned();
    }
    let minutes = ((seconds as f64) / 60.0).round() as i64;
    if minutes < 90 {
        return format!("{minutes}m ago");
    }
    let hours = ((minutes as f64) / 60.0).round() as i64;
    if hours < 36 {
        return format!("{hours}h ago");
    }
    format!("{}d ago", ((hours as f64) / 24.0).round() as i64)
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

fn sort_date_value(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

fn empty_graph() -> Value {
    json!({
        "available": false,
        "nodeCount": 0,
        "edgeCount": 0,
        "groups": [],
        "nodes": [],
        "links": [],
        "truncated": false,
    })
}

fn empty_finspo() -> Value {
    json!({
        "available": false,
        "duplicateGroups": 0,
        "inactiveCount": 0,
        "largestCount": 0,
        "recommendationCount": 0,
        "reclaimableBytes": 0,
        "sourceCount": 0,
    })
}

// ── Value extraction ──────────────────────────────────────────────────────

/// Read array `key` from a payload, transparently unwrapping a `{ data: { … } }`
/// envelope (integration-core and several Data Plane services wrap their lists).
fn array_from(payload: Option<&Value>, key: &str) -> Vec<Value> {
    let Some(payload) = payload else {
        return vec![];
    };
    let scope = payload
        .get("data")
        .filter(|d| d.is_object())
        .unwrap_or(payload);
    scope
        .get(key)
        .and_then(Value::as_array)
        .map(|arr| arr.to_vec())
        .unwrap_or_default()
}

fn str_at(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

fn str_any(value: &Value, keys: &[&str]) -> String {
    for key in keys {
        let v = str_at(value, key);
        if !v.is_empty() {
            return v;
        }
    }
    String::new()
}

fn nested_str(value: &Value, parents: &[&str], keys: &[&str]) -> Option<String> {
    for parent in parents {
        if let Some(child) = value.get(parent) {
            let v = str_any(child, keys);
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

fn num_at(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn str_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
