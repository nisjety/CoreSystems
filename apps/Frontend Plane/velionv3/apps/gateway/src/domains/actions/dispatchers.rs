use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    upstream::{proxy_bearer_json, proxy_json},
};

use super::shared::{cookie_header, quarry_token};

pub(super) async fn dispatch_recrawl(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let source_id = input.get("sourceId").and_then(|v| v.as_str()).unwrap_or("");
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let cookie = cookie_header(headers);
    let token = quarry_token(state, user, &cookie).await;

    let url = format!("{}/v1/crawl", state.quarry_edge_url);
    let crawl_body = json!({ "source_id": source_id, "org_id": org_id });

    let (status, Json(resp)) = proxy_bearer_json(
        state,
        Method::POST,
        &url,
        Some(crawl_body),
        token.as_deref(),
        &user.user_id,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    let run_id = resp
        .get("id")
        .or_else(|| resp.get("job_id"))
        .or_else(|| resp.get("run_id"))
        .or_else(|| resp.get("jobId"))
        .or_else(|| resp.pointer("/data/id"))
        .or_else(|| resp.pointer("/data/job_id"))
        .or_else(|| resp.pointer("/data/jobId"))
        .or_else(|| resp.pointer("/data/run_id"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("run_{}", user.user_id));

    let execution = ok(json!({
        "actionId": "knowledge.recrawl_source",
        "runId": run_id,
        "status": "queued",
        "auditId": format!("audit_{}_{}", run_id, user.user_id),
        "eventStream": format!("/api/v1/knowledge/runs/{}/events", run_id),
    }));

    (StatusCode::OK, Json(execution)).into_response()
}

/// `knowledge.scrape_url` → quarry-edge `POST /v1/scrape` with `ingest: true` so
/// the fetched page is persisted into the knowledge base (the action's stated
/// purpose). Synchronous: scrape returns page content directly, so the execution
/// is reported `completed` with no run-event stream.
pub(super) async fn dispatch_scrape_url(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let target = input.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let cookie = cookie_header(headers);
    let token = quarry_token(state, user, &cookie).await;

    let url = format!("{}/v1/scrape", state.quarry_edge_url);
    let body = json!({ "url": target, "ingest": true });

    let (status, Json(resp)) = proxy_bearer_json(
        state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        &user.user_id,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    let run_id = if target.is_empty() {
        format!("scrape_{}", user.user_id)
    } else {
        target.to_owned()
    };
    let execution = ok(json!({
        "actionId": "knowledge.scrape_url",
        "runId": run_id,
        "status": "completed",
        "auditId": format!("audit_{}_{}", run_id, user.user_id),
    }));

    (StatusCode::OK, Json(execution)).into_response()
}

/// `knowledge.crawl_site` → quarry-edge `POST /v1/crawl`. Async run; events
/// stream from `/api/v1/knowledge/runs/:id/events`.
pub(super) async fn dispatch_crawl_site(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let target = input.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let cookie = cookie_header(headers);
    let token = quarry_token(state, user, &cookie).await;

    let url = format!("{}/v1/crawl", state.quarry_edge_url);
    let mut crawl_body = json!({ "url": target, "org_id": org_id });
    if let Some(max_pages) = input.get("maxPages").and_then(|v| v.as_u64()) {
        crawl_body["max_pages"] = json!(max_pages);
    }
    // Phase 6 selective ingest: forward the resolved decision (default NEVER at
    // quarry when absent → working-set only).
    if let Some(ingest) = input.get("ingest").and_then(|v| v.as_bool()) {
        crawl_body["ingest"] = json!(ingest);
    }

    let (status, Json(resp)) = proxy_bearer_json(
        state,
        Method::POST,
        &url,
        Some(crawl_body),
        token.as_deref(),
        &user.user_id,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    let run_id = resp
        .get("id")
        .or_else(|| resp.get("job_id"))
        .or_else(|| resp.get("run_id"))
        .or_else(|| resp.get("jobId"))
        .or_else(|| resp.pointer("/data/id"))
        .or_else(|| resp.pointer("/data/job_id"))
        .or_else(|| resp.pointer("/data/jobId"))
        .or_else(|| resp.pointer("/data/run_id"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("crawl_{}", user.user_id));

    let execution = ok(json!({
        "actionId": "knowledge.crawl_site",
        "runId": run_id,
        "status": "queued",
        "auditId": format!("audit_{}_{}", run_id, user.user_id),
        "eventStream": format!("/api/v1/knowledge/runs/{}/events", run_id),
    }));

    (StatusCode::OK, Json(execution)).into_response()
}

/// `knowledge.import_source` adds a single web URL to the knowledge base. URL
/// ingestion belongs to quarry-edge (`POST /v1/scrape` with `ingest: true`), not
/// imports-core — whose source-job endpoint is exclusively for SaaS connectors
/// (notion/hubspot/salesforce/…); a future `knowledge.connect_source` action will
/// own those. Synchronous: the page is fetched + indexed before we reply.
pub(super) async fn dispatch_import_source(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let target = input
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if target.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_input",
                "knowledge.import_source requires a non-empty 'url'",
            )),
        )
            .into_response();
    }

    let cookie = cookie_header(headers);
    let token = quarry_token(state, user, &cookie).await;

    let url = format!("{}/v1/scrape", state.quarry_edge_url);
    let (status, Json(resp)) = proxy_bearer_json(
        state,
        Method::POST,
        &url,
        Some(json!({ "url": target, "ingest": true })),
        token.as_deref(),
        &user.user_id,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    let run_id = target.to_owned();
    let execution = ok(json!({
        "actionId": "knowledge.import_source",
        "runId": run_id,
        "status": "completed",
        "auditId": format!("audit_{}_{}", run_id, user.user_id),
    }));

    (StatusCode::OK, Json(execution)).into_response()
}

/// `knowledge.connect_source` → imports-core `POST /api/v1/import/jobs/source`,
/// the connector path imports-core actually implements: pull documents from a
/// connected SaaS source (notion/hubspot/salesforce/…). `org_id` is sent both as
/// the `x-org-id` header (auth) and in the body (imports-core's `SourceImportRequest`
/// requires it there). Async job; progress streams from the import events route.
pub(super) async fn dispatch_connect_source(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let source_type = input
        .get("sourceType")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if source_type.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_input",
                "knowledge.connect_source requires a 'sourceType'",
            )),
        )
            .into_response();
    }

    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };

    let body = json!({
        "org_id": org_id.clone(),
        "source_type": source_type,
        "connection": input.get("connection").cloned().unwrap_or_else(|| json!({})),
        "options": input.get("options").cloned().unwrap_or_else(|| json!({})),
    });

    let url = format!("{}/api/v1/import/jobs/source", state.imports_api_url);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(body),
        Some(org_id.as_str()),
        Some(&actor),
        None,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    let run_id = resp
        .get("id")
        .or_else(|| resp.get("jobId"))
        .or_else(|| resp.get("job_id"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("connect_{}", user.user_id));

    let execution = ok(json!({
        "actionId": "knowledge.connect_source",
        "runId": run_id,
        "status": "queued",
        "auditId": format!("audit_{}_{}", run_id, user.user_id),
        "eventStream": format!("/api/v1/knowledge/imports/{}/events", run_id),
    }));

    (StatusCode::OK, Json(execution)).into_response()
}

/// `brreg_lookup_organization` → org-core Brreg proxy. Read-only lookup against
/// the public Norwegian Enhetsregisteret data, wrapped as a completed action.
pub(super) async fn dispatch_brreg_lookup(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let query = input.get("q").and_then(|v| v.as_str()).unwrap_or("").trim();
    if query.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_input",
                "brreg_lookup_organization requires a non-empty 'q'",
            )),
        )
            .into_response();
    }

    let size = input
        .get("size")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(8)
        .clamp(1, 20);
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/brreg/search?q={}&size={}",
        state.org_core_url,
        urlencoding::encode(query),
        size,
    );
    let (status, Json(resp)) =
        proxy_json(state, Method::GET, &url, None, None, Some(&actor), None).await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    let execution = ok(json!({
        "actionId": "brreg_lookup_organization",
        "runId": format!("brreg_{}_{}", urlencoding::encode(query), user.user_id),
        "status": "completed",
        "auditId": format!("audit_brreg_{}_{}", urlencoding::encode(query), user.user_id),
        "result": resp,
    }));

    (StatusCode::OK, Json(execution)).into_response()
}

/// `knowledge.upload_files` is structurally human-only — it needs file bytes the
/// AI/JSON path cannot carry. Return an honest, typed error pointing at the real
/// multipart route rather than a synthetic "queued" run.
pub(super) async fn dispatch_upload_files() -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(error(
            "upload_requires_multipart",
            "File upload must use POST /api/v1/knowledge/imports/upload (multipart form-data); it cannot be executed as a JSON action.",
        )),
    )
        .into_response()
}

pub(super) async fn dispatch_toggle_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let workflow_id = input
        .get("workflowId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let enabled = input
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!("{}/api/v1/settings/ai", state.user_core_url);
    let body = serde_json::json!({
        "workflowPolicies": { workflow_id: enabled }
    });
    let (status, Json(resp)) = proxy_json(
        state,
        Method::PUT,
        &url,
        Some(body),
        None,
        Some(&actor),
        None,
    )
    .await;

    let run_id = format!("wf_policy_{}_{}", workflow_id, user.user_id);
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    (
        StatusCode::OK,
        Json(ok(serde_json::json!({
            "actionId": "workflows.toggle_policy",
            "runId": run_id,
            "status": "completed",
        }))),
    )
        .into_response()
}

pub(super) async fn dispatch_operating_map_generate(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
    action_id: &str,
) -> Response {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let body = json!({
        "requested_by": user.user_id.clone(),
        "generated_from": input.get("generatedFrom").cloned().unwrap_or_else(|| json!({
            "source": "knowledge-workspace",
            "capability": "operating_map.generate",
        })),
    });
    let url = format!("{}/v1/wiki/operating-map/refresh", state.wiki_store_url);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(body),
        Some(org_id.as_str()),
        Some(&actor),
        None,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    let run_id = resp
        .get("run_id")
        .or_else(|| resp.pointer("/proposal/generated_by_run_id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("operating_map_{}", user.user_id));
    (
        StatusCode::OK,
        Json(ok(json!({
            "actionId": action_id,
            "runId": run_id,
            "status": "queued",
            "auditId": format!("audit_{}_{}", run_id, user.user_id),
            "eventStream": format!("/api/v1/knowledge/operating-map/runs/{}/events", run_id),
            "result": resp,
        }))),
    )
        .into_response()
}

pub(super) async fn dispatch_operating_map_review(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let proposal_id = input
        .get("proposalId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let decision = input
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if proposal_id.is_empty() || !matches!(decision, "accept" | "reject") {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_input",
                "operating_map.review_proposal requires proposalId and decision accept|reject",
            )),
        )
            .into_response();
    }

    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/v1/wiki/operating-map/proposals/{}/review",
        state.wiki_store_url,
        urlencoding::encode(proposal_id)
    );
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({ "decision": decision, "reviewed_by": user.user_id.clone() })),
        Some(org_id.as_str()),
        Some(&actor),
        None,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    let run_id = format!("operating_map_review_{}_{}", proposal_id, user.user_id);
    (
        StatusCode::OK,
        Json(ok(json!({
            "actionId": "operating_map.review_proposal",
            "runId": run_id,
            "status": "completed",
            "auditId": format!("audit_{}", run_id),
            "result": resp,
        }))),
    )
        .into_response()
}

pub(super) async fn dispatch_operating_map_blueprint(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let version_id = input
        .get("versionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let blueprint_id = input
        .get("blueprintId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let role = input
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if version_id.is_empty() || blueprint_id.is_empty() || role.is_empty() || name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_input",
                "operating_map.create_agent_blueprint requires versionId, blueprintId, role, and name",
            )),
        )
            .into_response();
    }

    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let body = json!({
        "version_id": version_id,
        "blueprint_id": blueprint_id,
        "role": role,
        "source_workflow_id": input.get("sourceWorkflowId").and_then(Value::as_str).unwrap_or(""),
        "name": name,
        "requested_by": user.user_id.clone(),
        "payload": input.get("payload").cloned().unwrap_or_else(|| json!({})),
    });
    let url = format!(
        "{}/v1/wiki/operating-map/agent-blueprints",
        state.wiki_store_url
    );
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(body),
        Some(org_id.as_str()),
        Some(&actor),
        None,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    let run_id = format!("agent_blueprint_{}_{}", blueprint_id, user.user_id);
    (
        StatusCode::OK,
        Json(ok(json!({
            "actionId": "operating_map.create_agent_blueprint",
            "runId": run_id,
            "status": "completed",
            "auditId": format!("audit_{}", run_id),
            "result": resp,
        }))),
    )
        .into_response()
}

// --- Ticketing actions (conversation-core-go) ------------------------------
//
// The tickets.* registry actions have real backends in conversation-core-go —
// the same endpoints the dedicated /api/v1/tickets/* routes proxy to. These
// dispatchers make them executable through the generic action surface too, so
// the shared action contract + Model-Plane tool exposure are honest instead of
// a registered-but-501 promise. Each remaps the registry's camelCase input to
// conversation-core's snake_case body and forwards with the caller's org scope;
// conversation-core owns tickets, the gateway only scopes and forwards.

// ticket_remap copies present, non-null fields from a camelCase action input
// into a snake_case upstream body under the mapped keys.
fn ticket_remap(input: &Value, pairs: &[(&str, &str)]) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for (from, to) in pairs {
        if let Some(v) = input.get(*from) {
            if !v.is_null() {
                out.insert((*to).to_string(), v.clone());
            }
        }
    }
    out
}

fn ticket_bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_input", message)),
    )
        .into_response()
}

// forward_ticket_action proxies a ticket action to conversation-core-go with the
// caller's authorized org scope + actor and wraps the upstream ticket in the
// standard action-execution envelope.
async fn forward_ticket_action(
    state: &AppState,
    user: &AuthenticatedUser,
    action_id: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Response {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "org_scope_required",
                "An authorized organization scope is required.",
            )),
        )
            .into_response();
    }
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!("{}{}", state.conversation_core_url, path);
    let (status, Json(resp)) =
        proxy_json(state, method, &url, body, Some(&org_id), Some(&actor), None).await;
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }
    let ticket_id = resp
        .get("data")
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    (
        StatusCode::OK,
        Json(ok(json!({
            "actionId": action_id,
            "runId": format!("{}_{}", action_id, user.user_id),
            "status": "completed",
            "auditId": format!("audit_{}_{}", action_id, user.user_id),
            "ticketId": ticket_id,
            "result": resp,
        }))),
    )
        .into_response()
}

pub(super) async fn dispatch_ticket_create(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = input
        .get("conversationId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    if conversation_id.is_empty() {
        return ticket_bad_request("tickets.create requires a non-empty 'conversationId'");
    }
    let body = ticket_remap(
        input,
        &[
            ("conversationId", "conversation_id"),
            ("priority", "priority"),
            ("severity", "severity"),
            ("category", "category"),
            ("intent", "intent"),
        ],
    );
    forward_ticket_action(
        state,
        user,
        "tickets.create",
        Method::POST,
        "/api/v1/tickets",
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_classify(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = input
        .get("conversationId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    if conversation_id.is_empty() {
        return ticket_bad_request(
            "tickets.classify_conversation requires a non-empty 'conversationId'",
        );
    }
    let body = ticket_remap(
        input,
        &[
            ("confidence", "confidence"),
            ("reason", "reason"),
            ("suggestedFields", "suggested_fields"),
            ("evidenceMessageIds", "evidence_message_ids"),
        ],
    );
    let path = format!(
        "/api/v1/tickets/conversations/{}/classifications",
        urlencoding::encode(conversation_id)
    );
    forward_ticket_action(
        state,
        user,
        "tickets.classify_conversation",
        Method::POST,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_update(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = input
        .get("ticketId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    if ticket_id.is_empty() {
        return ticket_bad_request("tickets.update requires a non-empty 'ticketId'");
    }
    let body = ticket_remap(
        input,
        &[
            ("status", "status"),
            ("priority", "priority"),
            ("severity", "severity"),
            ("category", "category"),
            ("intent", "intent"),
        ],
    );
    let path = format!("/api/v1/tickets/{}", urlencoding::encode(ticket_id));
    forward_ticket_action(
        state,
        user,
        "tickets.update",
        Method::PATCH,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_assign(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = input
        .get("ticketId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    if ticket_id.is_empty() {
        return ticket_bad_request("tickets.assign requires a non-empty 'ticketId'");
    }
    let body = ticket_remap(
        input,
        &[
            ("assigneeUserId", "assignee_user_id"),
            ("assigneeName", "assignee_name"),
            ("teamId", "team_id"),
            ("teamName", "team_name"),
        ],
    );
    let path = format!("/api/v1/tickets/{}", urlencoding::encode(ticket_id));
    forward_ticket_action(
        state,
        user,
        "tickets.assign",
        Method::PATCH,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_link_resource(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = input
        .get("ticketId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    if ticket_id.is_empty() {
        return ticket_bad_request("tickets.link_resource requires a non-empty 'ticketId'");
    }
    let resource_kind = input
        .get("resourceKind")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    if resource_kind.is_empty() {
        return ticket_bad_request("tickets.link_resource requires a non-empty 'resourceKind'");
    }
    let body = ticket_remap(
        input,
        &[
            ("resourceKind", "resource_kind"),
            ("resourceId", "resource_id"),
            ("resourceUrl", "resource_url"),
            ("label", "label"),
        ],
    );
    let path = format!("/api/v1/tickets/{}/links", urlencoding::encode(ticket_id));
    forward_ticket_action(
        state,
        user,
        "tickets.link_resource",
        Method::POST,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_resolve(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = input
        .get("ticketId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    if ticket_id.is_empty() {
        return ticket_bad_request("tickets.resolve requires a non-empty 'ticketId'");
    }
    // conversation-core has no free-text resolution field; resolving is a status
    // transition. The optional 'resolution' note has no backend home and is
    // intentionally not forwarded rather than dropped into a wrong field.
    let path = format!("/api/v1/tickets/{}", urlencoding::encode(ticket_id));
    forward_ticket_action(
        state,
        user,
        "tickets.resolve",
        Method::PATCH,
        &path,
        Some(json!({ "status": "resolved" })),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upload_files_action_rejects_json_execution() {
        // upload_files is human-only (needs file bytes) — the JSON action path must
        // return a typed 422 pointing at the multipart route, never a fake success.
        let response = dispatch_upload_files().await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn ticket_remap_maps_camel_to_snake_and_omits_absent_or_null() {
        let input = json!({
            "conversationId": "conv_1",
            "priority": "high",
            "category": null,
        });
        let out = ticket_remap(
            &input,
            &[
                ("conversationId", "conversation_id"),
                ("priority", "priority"),
                ("severity", "severity"),
                ("category", "category"),
            ],
        );
        assert_eq!(
            out.get("conversation_id").and_then(|v| v.as_str()),
            Some("conv_1")
        );
        assert_eq!(out.get("priority").and_then(|v| v.as_str()), Some("high"));
        // absent field is omitted (not sent as null)...
        assert!(!out.contains_key("severity"));
        // ...and an explicit null is omitted too, so it never clobbers upstream state.
        assert!(!out.contains_key("category"));
    }
}

pub(super) async fn dispatch_social_create_draft(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let title = input
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let body_text = input
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let platforms = input
        .get("platforms")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if body_text.is_empty() || platforms.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_input",
                "social.create_draft requires a non-empty 'body' and at least one platform",
            )),
        )
            .into_response();
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let source_kind = input
        .get("sourceKind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual");
    let mut source = json!({ "kind": source_kind });
    if let Some(source_id) = input
        .get("sourceId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        source["metadata"] = json!({ "source_id": source_id });
    }
    let core_body = json!({
        "title": title,
        "body": body_text,
        "platforms": platforms,
        "source": source,
    });
    let actor = social_actor(user);
    let url = format!("{}/api/v1/social/posts", state.social_core_url);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(core_body),
        Some(&org_id),
        Some(&actor),
        Some("application/json"),
    )
    .await;
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }
    social_execution("social.create_draft", "completed", &resp, user)
}

pub(super) async fn dispatch_social_schedule_post(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let post_id = input
        .get("postId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let scheduled_at = input
        .get("scheduledAt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if post_id.is_empty() || scheduled_at.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_input",
                "social.schedule_post requires 'postId' and 'scheduledAt'",
            )),
        )
            .into_response();
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let actor = social_actor(user);
    let url = format!(
        "{}/api/v1/social/posts/{}/schedule",
        state.social_core_url,
        urlencoding::encode(post_id)
    );
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({ "scheduled_at": scheduled_at })),
        Some(&org_id),
        Some(&actor),
        Some("application/json"),
    )
    .await;
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }
    social_execution("social.schedule_post", "queued", &resp, user)
}

pub(super) async fn dispatch_social_publish_post(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let post_id = input
        .get("postId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if post_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_input",
                "social.publish_post requires 'postId'",
            )),
        )
            .into_response();
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let actor = social_actor(user);
    // social-core enforces the approval gate on this endpoint (Phase 2): an
    // unapproved post yields 409 approval_required, surfaced as-is below.
    let url = format!(
        "{}/api/v1/social/posts/{}/publish-jobs",
        state.social_core_url,
        urlencoding::encode(post_id)
    );
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({})),
        Some(&org_id),
        Some(&actor),
        Some("application/json"),
    )
    .await;
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }
    social_execution("social.publish_post", "queued", &resp, user)
}

fn social_actor(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn no_active_org() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error(
            "no_active_org",
            "No active organization is resolved for this session.",
        )),
    )
        .into_response()
}

/// Build the ActionExecution envelope from a social-core mutation response,
/// deriving the runId from the real returned post/job id.
fn social_execution(
    action_id: &str,
    status: &str,
    resp: &Value,
    user: &AuthenticatedUser,
) -> Response {
    let run_id = resp
        .pointer("/data/job/id")
        .or_else(|| resp.pointer("/data/id"))
        .or_else(|| resp.pointer("/data/post/id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("social_{}", user.user_id));
    let execution = ok(json!({
        "actionId": action_id,
        "runId": run_id,
        "status": status,
        "auditId": format!("audit_{}_{}", action_id.replace('.', "_"), run_id),
        "eventStream": "",
    }));
    (StatusCode::OK, Json(execution)).into_response()
}
