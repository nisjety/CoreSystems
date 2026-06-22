//! Ingestion sources — `GET /api/ingestions/sources`.
//!
//! Port of velionv2's `app/api/ingestions/sources/route.ts` +
//! `lib/integrations/integration-corev2.ts` (`knowledgeSourcesFromSummary`).
//! Assembles the SPA's `SourcePayload`:
//!   * `quarrySources` — durable web/crawl sources from quarry-control (bearer).
//!   * `integrations`   — Data-Plane document-derived source cards, plus
//!     integration-core connection cards (with safe discovery metadata),
//!     deduplicated by provider key.
//!   * `graph`          — entity-graph availability + node/edge counts.
//!
//! Cross-plane fan-out (documents-api / integration-core / graph-index) is
//! org-scoped using the authoritative org resolved from the session, NEVER a
//! client header. Quarry sources ride the bearer audience token.

use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use axum::{
    extract::{Extension, State},
    http::HeaderMap,
    response::Response,
};
use futures_util::future::join_all;
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, contracts::ActionActor, envelope::unwrap_data, middleware::AuthenticatedUser,
};

use super::shared::{
    actor_for, authorized_org_id, cookie_header, fetch_internal_json, first_str, obj_or_empty,
    okay, quarry_call, quarry_token, str_at,
};

pub(super) async fn list_sources(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let actor = actor_for(&user);
    let org = authorized_org_id(&state, &user).await;

    // Without an authoritative org, the cross-plane (org-scoped) lookups are
    // meaningless — return quarry web sources only.
    if org.trim().is_empty() {
        let quarry_sources = load_quarry_sources(&state, token.as_deref(), &user.user_id).await;
        return okay(json!({
            "graph": empty_graph(),
            "integrations": Vec::<Value>::new(),
            "quarrySources": quarry_sources,
        }));
    }

    let (quarry_sources, documents, providers, connections_raw, sync_jobs_raw, graph_snapshot) = tokio::join!(
        load_quarry_sources(&state, token.as_deref(), &user.user_id),
        load_documents(&state, &org, &actor),
        load_providers(&state, &org, &actor),
        load_connections(&state, &org, &actor),
        load_sync_jobs(&state, &org, &actor),
        load_graph(&state, &org, &actor),
    );

    let connections = build_connections(&connections_raw, &sync_jobs_raw, &providers);
    let connections = attach_discovery(&state, &org, &actor, connections).await;
    let integrations = knowledge_sources(&documents, &connections);

    okay(json!({
        "graph": graph_summary(graph_snapshot.as_ref()),
        "integrations": integrations,
        "quarrySources": quarry_sources,
    }))
}

// ── Loaders ───────────────────────────────────────────────────────────────

async fn load_quarry_sources(state: &AppState, token: Option<&str>, user_id: &str) -> Vec<Value> {
    let (status, body) = quarry_call(
        state,
        Method::GET,
        "/v1/sources?limit=100",
        None,
        token,
        user_id,
    )
    .await;
    if !status.is_success() {
        return vec![];
    }
    unwrap_data(&body)
        .get("items")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(to_quarry_source).collect())
        .unwrap_or_default()
}

fn to_quarry_source(source: &Value) -> Option<Value> {
    let id = str_at(source, "source_id");
    if id.is_empty() {
        return None;
    }
    Some(json!({
        "id": id,
        "name": str_at(source, "name"),
        "url": str_at(source, "url"),
        "kind": str_at(source, "kind"),
        "status": str_at(source, "status"),
        "createdAt": str_at(source, "created_at"),
        "updatedAt": str_at(source, "updated_at"),
        "config": obj_or_empty(source, "config"),
    }))
}

struct DocSummary {
    title: String,
    source: String,
    doc_type: String,
    status: String,
}

async fn load_documents(state: &AppState, org: &str, actor: &ActionActor) -> Vec<DocSummary> {
    let url = format!(
        "{}/v1/documents?limit=100&offset=0",
        state.documents_api_url
    );
    let payload = fetch_internal_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org),
        actor,
        Duration::from_millis(2_500),
    )
    .await;
    array_from_data(payload.as_ref(), "documents")
        .iter()
        .filter_map(|doc| {
            // A document without an id is dropped (v2 parity) even though the id
            // isn't surfaced in source cards — it signals a malformed record.
            first_str(doc, &["document_id", "documentId"])?;
            let source = str_at(doc, "source");
            let title = str_at(doc, "title");
            if source.is_empty() || title.is_empty() {
                return None;
            }
            Some(DocSummary {
                title,
                source,
                doc_type: first_str(doc, &["type"]).unwrap_or_else(|| "document".to_owned()),
                status: first_str(doc, &["status"]).unwrap_or_else(|| "unknown".to_owned()),
            })
        })
        .collect()
}

/// Provider key → display label, from integration-core's provider registry.
async fn load_providers(
    state: &AppState,
    org: &str,
    actor: &ActionActor,
) -> HashMap<String, String> {
    let url = format!("{}/api/v1/providers", state.integration_core_url);
    let payload = fetch_internal_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org),
        actor,
        Duration::from_millis(3_000),
    )
    .await;
    let mut labels = HashMap::new();
    for provider in array_from_data(payload.as_ref(), "providers") {
        let Some(key) = first_str(&provider, &["key"]) else {
            continue;
        };
        let label = first_str(&provider, &["label"]).unwrap_or_else(|| key.clone());
        labels.insert(normalize_provider_key(&key), label);
    }
    labels
}

async fn load_connections(state: &AppState, org: &str, actor: &ActionActor) -> Vec<Value> {
    let url = format!(
        "{}/api/v1/connections?organizationId={}",
        state.integration_core_url,
        urlencoding::encode(org)
    );
    let payload = fetch_internal_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org),
        actor,
        Duration::from_millis(3_000),
    )
    .await;
    array_from_data(payload.as_ref(), "connections")
}

async fn load_sync_jobs(state: &AppState, org: &str, actor: &ActionActor) -> Vec<Value> {
    let url = format!(
        "{}/api/v1/sync-jobs?organizationId={}",
        state.integration_core_url,
        urlencoding::encode(org)
    );
    let payload = fetch_internal_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org),
        actor,
        Duration::from_millis(3_000),
    )
    .await;
    let mut jobs = array_from_data(payload.as_ref(), "syncJobs");
    if jobs.is_empty() {
        jobs = array_from_data(payload.as_ref(), "sync_jobs");
    }
    jobs
}

async fn load_graph(state: &AppState, org: &str, actor: &ActionActor) -> Option<Value> {
    let url = format!(
        "{}/v1/graphs/{}?limit_nodes=120&limit_edges=240",
        state.graph_index_url,
        urlencoding::encode(org)
    );
    fetch_internal_json(
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

// ── Connection summaries + discovery ─────────────────────────────────────────

struct Discovery {
    workspace_name: String,
    entity_counts: Value,
    sample_entities: Vec<Value>,
    availability: Value,
}

struct ConnSummary {
    id: String,
    provider_key: String,
    provider_label: String,
    display_name: String,
    status: String,
    sync_status: String,
    capabilities: Vec<String>,
    deleted: bool,
    discovery: Option<Discovery>,
}

fn build_connections(
    connections_raw: &[Value],
    sync_jobs_raw: &[Value],
    labels: &HashMap<String, String>,
) -> Vec<ConnSummary> {
    // connectionId → (latest job status, updated_at) — newest wins by timestamp.
    let mut latest: HashMap<String, (String, String)> = HashMap::new();
    for job in sync_jobs_raw {
        let conn_id = first_str(job, &["connectionId", "connection_id"]).unwrap_or_default();
        let id = str_at(job, "id");
        if conn_id.is_empty() || id.is_empty() {
            continue;
        }
        let status = first_str(job, &["status"]).unwrap_or_else(|| "unknown".to_owned());
        let updated = first_str(job, &["updatedAt", "updated_at"]).unwrap_or_default();
        match latest.get(&conn_id) {
            Some((_, current)) if updated.as_str() <= current.as_str() => {}
            _ => {
                latest.insert(conn_id, (status, updated));
            }
        }
    }

    connections_raw
        .iter()
        .filter_map(|conn| {
            let id = str_at(conn, "id");
            let provider_key = normalize_provider_key(
                &first_str(conn, &["providerKey", "provider_key"]).unwrap_or_default(),
            );
            if id.is_empty() || provider_key.is_empty() {
                return None;
            }
            let status = first_str(conn, &["status"]).unwrap_or_else(|| "unknown".to_owned());
            let provider_label = labels
                .get(&provider_key)
                .cloned()
                .unwrap_or_else(|| provider_label(&provider_key));
            let display_name = first_str(conn, &["displayName", "display_name"])
                .unwrap_or_else(|| provider_label.clone());
            let sync_status = first_str(conn, &["lastSyncStatus", "last_sync_status"])
                .or_else(|| latest.get(&id).map(|(s, _)| s.clone()))
                .unwrap_or_else(|| status.clone());
            Some(ConnSummary {
                id,
                provider_key,
                provider_label,
                display_name,
                status,
                sync_status,
                capabilities: string_array(conn, "capabilities"),
                deleted: !first_str(conn, &["deletedAt", "deleted_at"])
                    .unwrap_or_default()
                    .is_empty(),
                discovery: None,
            })
        })
        .collect()
}

/// Attach safe discovery metadata to the first few active connections (capped —
/// each is an integration-core round-trip). Failures degrade to `None`.
async fn attach_discovery(
    state: &AppState,
    org: &str,
    actor: &ActionActor,
    mut connections: Vec<ConnSummary>,
) -> Vec<ConnSummary> {
    const MAX_DISCOVERY: usize = 8;
    let ids: Vec<String> = connections
        .iter()
        .filter(|c| !c.deleted)
        .take(MAX_DISCOVERY)
        .map(|c| c.id.clone())
        .collect();
    if ids.is_empty() {
        return connections;
    }

    let state_ref = &state;
    let snapshots = join_all(ids.iter().map(|id| {
        let url = format!(
            "{}/api/v1/connections/{}/discovery",
            state.integration_core_url,
            urlencoding::encode(id)
        );
        async move {
            let payload = fetch_internal_json(
                state_ref,
                Method::GET,
                &url,
                None,
                Some(org),
                actor,
                Duration::from_millis(2_000),
            )
            .await;
            (id.clone(), safe_discovery(payload.as_ref()))
        }
    }))
    .await;

    let mut by_connection: HashMap<String, Discovery> = snapshots
        .into_iter()
        .filter_map(|(id, d)| d.map(|d| (id, d)))
        .collect();
    for conn in connections.iter_mut() {
        conn.discovery = by_connection.remove(&conn.id);
    }
    connections
}

fn safe_discovery(payload: Option<&Value>) -> Option<Discovery> {
    let payload = payload?;
    let data = payload
        .get("data")
        .filter(|d| d.is_object())
        .unwrap_or(payload);
    let discovery = data
        .get("discovery")
        .filter(|d| d.is_object())
        .unwrap_or(data);
    if str_at(discovery, "connectionId").is_empty() || str_at(discovery, "providerKey").is_empty() {
        return None;
    }
    Some(Discovery {
        workspace_name: str_at(discovery, "workspaceName"),
        entity_counts: number_record(discovery.get("entityCounts")),
        sample_entities: sample_entities(discovery.get("sampleEntities")),
        availability: bool_record(discovery.get("availability")),
    })
}

// ── Source-card assembly (knowledgeSourcesFromSummary) ───────────────────────

/// `[...dataPlaneSources, ...integrationSources]` where integration sources are
/// dropped if a data-plane source already covers that provider key.
fn knowledge_sources(documents: &[DocSummary], connections: &[ConnSummary]) -> Vec<Value> {
    let data_plane = data_plane_sources(documents);
    let seen: HashSet<String> = data_plane.iter().map(|(key, _)| key.clone()).collect();
    let mut out: Vec<Value> = data_plane.into_iter().map(|(_, card)| card).collect();
    for conn in connections {
        if conn.deleted || seen.contains(&conn.provider_key) {
            continue;
        }
        out.push(integration_card(conn));
    }
    out
}

/// Group documents by provider key into data-plane source cards. Returns
/// `(provider_key, card)` so the caller can dedup integration cards by key.
fn data_plane_sources(documents: &[DocSummary]) -> Vec<(String, Value)> {
    let mut order: Vec<String> = vec![];
    let mut groups: HashMap<String, Vec<&DocSummary>> = HashMap::new();
    for doc in documents {
        let key = {
            let normalized = normalize_provider_key(&doc.source);
            if normalized.is_empty() {
                doc.source.clone()
            } else {
                normalized
            }
        };
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(doc);
    }

    order
        .into_iter()
        .map(|source| {
            let docs = groups.remove(&source).unwrap_or_default();

            let mut status_counts: serde_json::Map<String, Value> = serde_json::Map::new();
            for doc in &docs {
                let next = status_counts
                    .get(&doc.status)
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    + 1;
                status_counts.insert(doc.status.clone(), json!(next));
            }
            let indexed = ["indexed", "active", "completed"]
                .iter()
                .find_map(|key| status_counts.get(*key).and_then(Value::as_i64))
                .unwrap_or(0);
            let len = docs.len() as i64;
            let samples: Vec<Value> = docs
                .iter()
                .take(4)
                .map(|doc| json!({ "kind": doc.doc_type, "label": doc.title }))
                .collect();
            let status = if indexed > 0 {
                "indexed".to_owned()
            } else {
                docs.first()
                    .map(|d| d.status.clone())
                    .unwrap_or_else(|| "pending".to_owned())
            };

            // counts = { documents, indexed, ...statusCounts } — status keys win on collision.
            let mut counts = serde_json::Map::new();
            counts.insert("documents".into(), json!(len));
            counts.insert("indexed".into(), json!(indexed));
            for (key, value) in &status_counts {
                counts.insert(key.clone(), value.clone());
            }

            let card = json!({
                "id": format!("dataplane:{source}"),
                "title": source_label(&source),
                "provider": "Data Plane",
                "status": status,
                "detail": format!("{len} documents · {indexed} indexed"),
                "counts": Value::Object(counts),
                "samples": samples,
                "capabilities": Vec::<String>::new(),
                "readOnly": true,
            });
            (source, card)
        })
        .collect()
}

fn integration_card(conn: &ConnSummary) -> Value {
    let counts = conn
        .discovery
        .as_ref()
        .map(|d| d.entity_counts.clone())
        .unwrap_or_else(|| json!({}));
    let samples: Vec<Value> = conn
        .discovery
        .as_ref()
        .map(|d| d.sample_entities.iter().take(4).cloned().collect())
        .unwrap_or_default();

    let count_label = counts
        .as_object()
        .map(|obj| {
            obj.iter()
                .take(3)
                .map(|(key, value)| format!("{}: {}", format_count_key(key), value))
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();
    let availability_label = conn
        .discovery
        .as_ref()
        .and_then(|d| d.availability.as_object())
        .map(|obj| {
            obj.iter()
                .filter(|(_, value)| value.as_bool() == Some(true))
                .map(|(key, _)| format_count_key(key))
                .take(4)
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();
    let detail = if !count_label.is_empty() {
        count_label
    } else if !availability_label.is_empty() {
        availability_label
    } else {
        "Connected source metadata".to_owned()
    };

    let title = conn
        .discovery
        .as_ref()
        .map(|d| d.workspace_name.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| (!conn.display_name.is_empty()).then(|| conn.display_name.clone()))
        .unwrap_or_else(|| conn.provider_label.clone());
    let status = if conn.sync_status.is_empty() {
        conn.status.clone()
    } else {
        conn.sync_status.clone()
    };

    json!({
        "id": conn.id,
        "title": title,
        "provider": conn.provider_label,
        "status": status,
        "detail": detail,
        "counts": counts,
        "capabilities": conn.capabilities,
        "samples": samples,
        "readOnly": true,
    })
}

fn graph_summary(snapshot: Option<&Value>) -> Value {
    match snapshot {
        Some(body) => json!({
            "available": true,
            "nodeCount": body.get("node_count").and_then(Value::as_i64).unwrap_or(0),
            "edgeCount": body.get("edge_count").and_then(Value::as_i64).unwrap_or(0),
        }),
        None => empty_graph(),
    }
}

fn empty_graph() -> Value {
    json!({ "available": false, "nodeCount": 0, "edgeCount": 0 })
}

// ── Value extraction + classifiers ───────────────────────────────────────────

/// Owned array `key` from a payload, unwrapping a `{ data: {…} }` envelope.
fn array_from_data(payload: Option<&Value>, key: &str) -> Vec<Value> {
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
        .cloned()
        .unwrap_or_default()
}

fn string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn number_record(value: Option<&Value>) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(obj) = value.and_then(Value::as_object) {
        for (key, v) in obj {
            if v.is_number() && v.as_f64().map(f64::is_finite).unwrap_or(false) {
                out.insert(key.clone(), v.clone());
            }
        }
    }
    Value::Object(out)
}

fn bool_record(value: Option<&Value>) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(obj) = value.and_then(Value::as_object) {
        for (key, v) in obj {
            if v.is_boolean() {
                out.insert(key.clone(), v.clone());
            }
        }
    }
    Value::Object(out)
}

/// Safe sample entities: drop blank/email-like/over-long labels, cap at 6.
fn sample_entities(value: Option<&Value>) -> Vec<Value> {
    let Some(arr) = value.and_then(Value::as_array) else {
        return vec![];
    };
    arr.iter()
        .filter_map(|item| {
            let label = str_at(item, "label");
            if label.is_empty() || label.chars().count() > 100 || label.contains('@') {
                return None;
            }
            let kind = {
                let k = str_at(item, "kind");
                if k.is_empty() {
                    "entity".to_owned()
                } else {
                    k
                }
            };
            Some(json!({ "kind": kind, "label": label }))
        })
        .take(6)
        .collect()
}

fn normalize_provider_key(value: &str) -> String {
    match value.trim().to_lowercase().as_str() {
        "m365" | "microsoft365" | "microsoft-365" | "microsoft-graph" | "onedrive" | "outlook"
        | "sharepoint" | "teams" => "microsoft".to_owned(),
        "gdrive" | "gmail" | "google-drive" | "google-workspace" => "google".to_owned(),
        other => other.to_owned(),
    }
}

fn provider_label(provider_key: &str) -> String {
    let label = match normalize_provider_key(provider_key).as_str() {
        "microsoft" => "Microsoft 365",
        "google" => "Google Workspace",
        "github" => "GitHub",
        "notion" => "Notion",
        "slack" => "Slack",
        "shopify" => "Shopify",
        "stripe" => "Stripe",
        "okta" => "Okta",
        "scim" => "SCIM",
        _ => return provider_key.to_owned(),
    };
    label.to_owned()
}

fn source_label(source: &str) -> String {
    let key = normalize_provider_key(source);
    if matches!(
        key.as_str(),
        "microsoft"
            | "google"
            | "github"
            | "notion"
            | "slack"
            | "shopify"
            | "stripe"
            | "okta"
            | "scim"
    ) {
        return provider_label(&key);
    }
    if let Some(rest) = source.strip_prefix("onboarding:") {
        let provider = normalize_provider_key(rest.split(':').next().unwrap_or(rest));
        return if provider.is_empty() {
            "Onboarding source".to_owned()
        } else {
            provider_label(&provider)
        };
    }
    let spaced: String = source
        .chars()
        .map(|c| if matches!(c, '_' | ':' | '-') { ' ' } else { c })
        .collect();
    titlecase_words(&spaced.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn format_count_key(key: &str) -> String {
    titlecase_words(&key.replace('_', " "))
}

fn titlecase_words(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(source: &str, status: &str) -> DocSummary {
        DocSummary {
            title: format!("{source} doc"),
            source: source.to_owned(),
            doc_type: "pdf".to_owned(),
            status: status.to_owned(),
        }
    }

    #[test]
    fn data_plane_sources_group_count_and_classify() {
        let docs = vec![
            doc("notion", "indexed"),
            doc("notion", "pending"),
            doc("sharepoint", "active"),
        ];
        let sources = data_plane_sources(&docs);
        assert_eq!(sources.len(), 2);

        let (notion_key, notion) = &sources[0];
        assert_eq!(notion_key, "notion");
        assert_eq!(notion["id"], "dataplane:notion");
        assert_eq!(notion["title"], "Notion");
        assert_eq!(notion["status"], "indexed");
        assert_eq!(notion["counts"]["documents"], 2);
        assert_eq!(notion["counts"]["indexed"], 1);

        // sharepoint normalizes to the microsoft provider key.
        let (ms_key, ms) = &sources[1];
        assert_eq!(ms_key, "microsoft");
        assert_eq!(ms["title"], "Microsoft 365");
        assert_eq!(ms["status"], "indexed"); // "active" counts as indexed
    }

    #[test]
    fn knowledge_sources_dedupe_integration_by_provider_key() {
        let docs = vec![doc("notion", "indexed")];
        let connections = vec![
            ConnSummary {
                id: "c-notion".into(),
                provider_key: "notion".into(),
                provider_label: "Notion".into(),
                display_name: "Notion".into(),
                status: "connected".into(),
                sync_status: "connected".into(),
                capabilities: vec![],
                deleted: false,
                discovery: None,
            },
            ConnSummary {
                id: "c-slack".into(),
                provider_key: "slack".into(),
                provider_label: "Slack".into(),
                display_name: "Acme Slack".into(),
                status: "connected".into(),
                sync_status: "syncing".into(),
                capabilities: vec!["messages".into()],
                deleted: false,
                discovery: None,
            },
        ];
        let sources = knowledge_sources(&docs, &connections);
        // notion data-plane card + slack integration card; notion connection deduped.
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0]["id"], "dataplane:notion");
        assert_eq!(sources[1]["id"], "c-slack");
        assert_eq!(sources[1]["title"], "Acme Slack");
        assert_eq!(sources[1]["status"], "syncing");
        assert_eq!(sources[1]["detail"], "Connected source metadata");
        assert_eq!(sources[1]["readOnly"], true);
    }

    #[test]
    fn integration_card_detail_prefers_count_label() {
        let conn = ConnSummary {
            id: "c1".into(),
            provider_key: "microsoft".into(),
            provider_label: "Microsoft 365".into(),
            display_name: "Corp M365".into(),
            status: "connected".into(),
            sync_status: String::new(),
            capabilities: vec![],
            deleted: false,
            discovery: Some(Discovery {
                workspace_name: "Contoso".into(),
                entity_counts: json!({ "drive_count": 3, "site_count": 2 }),
                sample_entities: vec![json!({ "kind": "drive", "label": "Shared" })],
                availability: json!({ "files": true }),
            }),
        };
        let card = integration_card(&conn);
        assert_eq!(card["title"], "Contoso");
        assert_eq!(card["status"], "connected"); // falls back to status when syncStatus empty
                                                 // BTreeMap ordering: drive_count before site_count.
        assert_eq!(card["detail"], "Drive Count: 3 · Site Count: 2");
        assert_eq!(card["counts"]["drive_count"], 3);
        assert_eq!(card["samples"][0]["label"], "Shared");
    }

    #[test]
    fn safe_discovery_filters_email_labels_and_requires_ids() {
        let payload = json!({
            "data": {
                "discovery": {
                    "connectionId": "c1",
                    "providerKey": "microsoft",
                    "workspaceName": "Contoso",
                    "entityCounts": { "drives": 2, "bad": "x" },
                    "sampleEntities": [
                        { "kind": "user", "label": "alice@contoso.com" },
                        { "kind": "drive", "label": "Marketing" }
                    ],
                    "availability": { "files": true, "mail": false }
                }
            }
        });
        let discovery = safe_discovery(Some(&payload)).unwrap();
        assert_eq!(discovery.workspace_name, "Contoso");
        assert_eq!(discovery.entity_counts["drives"], 2);
        assert!(discovery.entity_counts.get("bad").is_none()); // non-number dropped
        assert_eq!(discovery.sample_entities.len(), 1); // email label dropped
        assert_eq!(discovery.sample_entities[0]["label"], "Marketing");

        assert!(safe_discovery(Some(&json!({ "data": {} }))).is_none());
    }

    #[test]
    fn graph_summary_reports_counts_when_present() {
        let snap = json!({ "node_count": 42, "edge_count": 99 });
        let graph = graph_summary(Some(&snap));
        assert_eq!(graph["available"], true);
        assert_eq!(graph["nodeCount"], 42);
        assert_eq!(graph["edgeCount"], 99);
        assert_eq!(graph_summary(None)["available"], false);
    }
}
