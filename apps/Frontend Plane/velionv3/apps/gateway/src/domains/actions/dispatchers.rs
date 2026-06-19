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

use super::shared::{cookie_header, org_id_from_headers, quarry_token};

pub(super) async fn dispatch_recrawl(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let source_id = input.get("sourceId").and_then(|v| v.as_str()).unwrap_or("");
    let org_id = org_id_from_headers(headers).unwrap_or_default();
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
    let org_id = org_id_from_headers(headers).unwrap_or_default();
    let cookie = cookie_header(headers);
    let token = quarry_token(state, user, &cookie).await;

    let url = format!("{}/v1/crawl", state.quarry_edge_url);
    let mut crawl_body = json!({ "url": target, "org_id": org_id });
    if let Some(max_pages) = input.get("maxPages").and_then(|v| v.as_u64()) {
        crawl_body["max_pages"] = json!(max_pages);
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
    headers: &HeaderMap,
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

    let org_id = org_id_from_headers(headers).unwrap_or_default();
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
    headers: &HeaderMap,
    input: &Value,
    action_id: &str,
) -> Response {
    let org_id = org_id_from_headers(headers).unwrap_or_default();
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
    headers: &HeaderMap,
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

    let org_id = org_id_from_headers(headers).unwrap_or_default();
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
    headers: &HeaderMap,
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

    let org_id = org_id_from_headers(headers).unwrap_or_default();
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
}
