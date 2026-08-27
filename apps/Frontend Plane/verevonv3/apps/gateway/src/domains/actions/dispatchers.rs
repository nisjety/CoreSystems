use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::domains::inbox::require_support_ai_review;
use crate::{
    audience_tokens::get_audience_token,
    config::AppState,
    contracts::ActionActor,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    upstream::{
        proxy_bearer_json, proxy_bearer_json_with_headers, proxy_conversation_json, proxy_json,
    },
};

use super::shared::{cookie_header, quarry_token};

/// Fetch the Conversation Core catalog through the same signed delegation
/// boundary used by its effects. Only a structurally complete human contract
/// is returned: malformed or agent-only entries are unavailable rather than
/// being optimistically advertised by the BFF.
pub(super) async fn list_owner_action_contracts(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Response {
    match human_owner_action_catalog_for(state, user).await {
        Ok(catalog) => (StatusCode::OK, Json(ok(catalog))).into_response(),
        Err((status, payload)) => (status, Json(payload)).into_response(),
    }
}

/// Reusable human-only owner-contract projection. Its caller must independently
/// establish current Space/resource predicates before exposing this catalog;
/// this function never makes Model eligibility or effect authority decisions.
pub(crate) async fn human_owner_action_catalog_for(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<Value, (StatusCode, Value)> {
    let url = format!("{}/api/v1/action-contracts", state.conversation_core_url);
    let (status, Json(payload)) =
        proxy_conversation_json(state, Method::GET, &url, None, user, None).await;
    if !status.is_success() {
        return Err((status, payload));
    }
    let Some(catalog) = human_owner_action_catalog(&payload) else {
        return Err((
            StatusCode::BAD_GATEWAY,
            error(
                "owner_action_contract_invalid",
                "Conversation Core returned no valid human action contracts.",
            ),
        ));
    };
    Ok(catalog)
}

fn human_owner_action_catalog(payload: &Value) -> Option<Value> {
    let data = payload.get("data")?;
    let catalog_version = data.get("catalog_version")?.as_str()?;
    if catalog_version.trim().is_empty() {
        return None;
    }
    let actions = data.get("actions")?.as_array()?;
    let verified: Vec<Value> = actions
        .iter()
        .filter(|action| owner_contract_is_human_executable(action))
        .filter_map(human_contract_projection)
        .collect();
    if verified.is_empty() {
        return None;
    }
    Some(json!({
        "catalogVersion": catalog_version,
        "actorType": "human",
        "actions": verified,
    }))
}

// The browser is never a transport for a planned workload operation. Owner
// contracts may describe disabled actor paths for release review, but the
// human projection strips every non-human actor and rewrites eligibility rather
// than trusting the source object to be presentation-safe.
fn human_contract_projection(action: &Value) -> Option<Value> {
    let mut projected = action.as_object()?.clone();
    projected.insert("eligible_actor_types".to_owned(), json!(["human"]));
    if let Some(requirements) = projected.get_mut("actor_requirements") {
        let requirements = requirements.as_array_mut()?;
        requirements.retain(|requirement| {
            requirement.get("actor_type").and_then(Value::as_str) == Some("human")
                && requirement.get("availability").and_then(Value::as_str) == Some("available")
        });
    }
    Some(Value::Object(projected))
}

fn owner_contract_is_human_executable(action: &Value) -> bool {
    let has_human_actor = action
        .get("eligible_actor_types")
        .and_then(Value::as_array)
        .is_some_and(|actors| actors.iter().any(|actor| actor.as_str() == Some("human")));
    has_human_actor
        && action.get("action_id").and_then(Value::as_str) == Some("tickets.create")
        && action.get("owner_plane").and_then(Value::as_str) == Some("application")
        && action
            .get("required_service_identity")
            .and_then(Value::as_str)
            == Some("verevon-gateway")
        && action.get("required_delegation").and_then(Value::as_str)
            == Some("verified_user_org_role")
        && action.get("idempotency").and_then(Value::as_str) == Some("caller_supplied")
        && action.get("receipt_contract").and_then(Value::as_str) == Some("durable_owner_receipt")
        && action
            .get("input_schema")
            .filter(|schema| schema.is_object())
            .and_then(canonical_schema_sha256)
            .is_some_and(|digest| {
                action.get("schema_sha256").and_then(Value::as_str) == Some(digest.as_str())
            })
}

fn canonical_schema_sha256(schema: &Value) -> Option<String> {
    let canonical = serde_json::to_vec(schema).ok()?;
    Some(format!("sha256:{:x}", Sha256::digest(canonical)))
}

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
    headers: &HeaderMap,
    input: &Value,
    idempotency_key: &str,
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
    let cookie = cookie_header(headers);
    let Some(token) = get_audience_token(state, &user.user_id, &cookie, "ingestion").await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "imports_auth_unavailable",
                "Import authentication is temporarily unavailable.",
            )),
        )
            .into_response();
    };

    let mut clean_input = input.clone();
    let import_decision = match crate::domains::spaces::personal_import_ingress_decision(
        state,
        user,
        &org_id,
        &mut clean_input,
        idempotency_key,
    )
    .await
    {
        Ok(token) => token,
        Err((status, body)) => return (status, body).into_response(),
    };
    let body = json!({
        "org_id": org_id.clone(),
        "source_type": source_type,
        "connection": clean_input.get("connection").cloned().unwrap_or_else(|| json!({})),
        "options": clean_input.get("options").cloned().unwrap_or_else(|| json!({})),
    });

    let url = format!("{}/api/v1/import/jobs/source", state.imports_api_url);
    let extra_headers = import_decision
        .map(|token| vec![("X-Space-Import-Ingress-Decision".to_owned(), token)])
        .unwrap_or_default();
    let (status, Json(resp)) = proxy_bearer_json_with_headers(
        state,
        Method::POST,
        &url,
        Some(body),
        Some(&token),
        &user.user_id,
        &extra_headers,
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

/// `shipping.get_quotes` → shipping-core's carrier-fleet quote fan-out. Same
/// "ingestion" audience bearer as the dedicated `/api/v1/shipping/quotes`
/// route (see `domains::shipping::shipping_token`) — this dispatcher exists
/// so the model can call it as a chat tool, not just the SPA's shipping page.
pub(super) async fn dispatch_shipping_quotes(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let cookie = cookie_header(headers);
    let Some(token) = get_audience_token(state, &user.user_id, &cookie, "ingestion").await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "shipping_auth_unavailable",
                "Shipping authentication is temporarily unavailable.",
            )),
        )
            .into_response();
    };

    let url = format!("{}/api/quotes", state.shipping_core_url);
    let (status, Json(resp)) = proxy_bearer_json(
        state,
        Method::POST,
        &url,
        Some(input.clone()),
        Some(&token),
        &user.user_id,
    )
    .await;

    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }

    let run_id = format!("shipping_quotes_{}", user.user_id);
    let execution = ok(json!({
        "actionId": "shipping.get_quotes",
        "runId": run_id,
        "status": "completed",
        "auditId": format!("audit_{run_id}"),
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
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let body = json!({
        "requested_by": user.user_id.clone(),
        "generated_from": input.get("generatedFrom").cloned().unwrap_or_else(|| json!({
            "source": "knowledge-workspace",
            "capability": "operating_map.generate",
        })),
    });
    let url = format!("{}/v1/wiki/operating-map/refresh", state.wiki_store_url);
    let (status, Json(resp)) = crate::domains::knowledge::shared::proxy_data_plane_json(
        state,
        user,
        headers,
        Method::POST,
        &url,
        Some(body),
        Some(org_id.as_str()),
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

    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let url = format!(
        "{}/v1/wiki/operating-map/proposals/{}/review",
        state.wiki_store_url,
        urlencoding::encode(proposal_id)
    );
    let (status, Json(resp)) = crate::domains::knowledge::shared::proxy_data_plane_json(
        state,
        user,
        headers,
        Method::POST,
        &url,
        Some(json!({ "decision": decision, "reviewed_by": user.user_id.clone() })),
        Some(org_id.as_str()),
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

    let org_id = crate::upstream::authorized_org_id(state, user).await;
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
    let (status, Json(resp)) = crate::domains::knowledge::shared::proxy_data_plane_json(
        state,
        user,
        headers,
        Method::POST,
        &url,
        Some(body),
        Some(org_id.as_str()),
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

fn ticket_string<'a>(input: &'a Value, key: &str) -> &'a str {
    input
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
}

// `tickets.classify_conversation` is the Inbox's proposal path, not the
// generic automation ingress. Make its review posture explicit rather than
// allowing conversation-core to infer `auto_ticket` from a high confidence
// value. A different, explicitly governed automation route may opt into that
// capability later; this shared UI action must always create reviewable work.
fn ticket_classification_body(input: &Value) -> serde_json::Map<String, Value> {
    let mut body = ticket_remap(
        input,
        &[
            ("confidence", "confidence"),
            ("reason", "reason"),
            ("suggestedFields", "suggested_fields"),
            ("evidenceMessageIds", "evidence_message_ids"),
        ],
    );
    body.insert(
        "outcome".to_owned(),
        Value::String("suggest_ticket".to_owned()),
    );
    body
}

fn ticket_bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_input", message)),
    )
        .into_response()
}

pub(super) async fn dispatch_conversation_follow(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = input
        .get("conversationId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let Some(following) = input.get("following").and_then(Value::as_bool) else {
        return ticket_bad_request("inbox.follow_conversation requires boolean 'following'");
    };
    if conversation_id.is_empty() {
        return ticket_bad_request(
            "inbox.follow_conversation requires a non-empty 'conversationId'",
        );
    }
    let path = format!(
        "/api/v1/conversations/{}/follow",
        urlencoding::encode(conversation_id)
    );
    let method = if following {
        Method::POST
    } else {
        Method::DELETE
    };
    let url = format!("{}{}", state.conversation_core_url, path);
    let (status, Json(resp)) = proxy_conversation_json(state, method, &url, None, user, None).await;
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }
    (
        StatusCode::OK,
        Json(ok(json!({
            "actionId": "inbox.follow_conversation",
            "runId": format!("conversation_follow_{}_{}", conversation_id, user.user_id),
            "status": "completed",
            "auditId": format!("audit_conversation_follow_{}_{}", conversation_id, user.user_id),
            "eventStream": "",
        }))),
    )
        .into_response()
}

pub(super) async fn dispatch_conversation_csat_preference(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = input
        .get("conversationId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let Some(opted_in) = input.get("optedIn").and_then(Value::as_bool) else {
        return ticket_bad_request("inbox.set_csat_preference requires boolean 'optedIn'");
    };
    if conversation_id.is_empty() {
        return ticket_bad_request(
            "inbox.set_csat_preference requires a non-empty 'conversationId'",
        );
    }
    let path = format!(
        "/api/v1/conversations/{}/csat-preference",
        urlencoding::encode(conversation_id)
    );
    let url = format!("{}{}", state.conversation_core_url, path);
    let (status, Json(resp)) = proxy_conversation_json(
        state,
        Method::PATCH,
        &url,
        Some(json!({ "opted_in": opted_in })),
        user,
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }
    (StatusCode::OK, Json(ok(json!({
        "actionId": "inbox.set_csat_preference",
        "runId": format!("conversation_csat_preference_{}_{}", conversation_id, user.user_id),
        "status": "completed",
        "auditId": format!("audit_conversation_csat_preference_{}_{}", conversation_id, user.user_id),
        "eventStream": "",
    })))).into_response()
}

pub(super) async fn dispatch_ticket_record_csat_outcome(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = input
        .get("ticketId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let Some(score) = input.get("score").and_then(Value::as_i64) else {
        return ticket_bad_request("tickets.record_csat_outcome requires integer 'score'");
    };
    if ticket_id.is_empty() || !(1..=5).contains(&score) {
        return ticket_bad_request(
            "tickets.record_csat_outcome requires a ticketId and score from 1 to 5",
        );
    }
    let path = format!(
        "/api/v1/tickets/{}/csat-outcome",
        urlencoding::encode(ticket_id)
    );
    let url = format!("{}{}", state.conversation_core_url, path);
    let (status, Json(resp)) = proxy_conversation_json(
        state,
        Method::PUT,
        &url,
        Some(json!({ "score": score })),
        user,
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }
    (
        StatusCode::OK,
        Json(ok(json!({
            "actionId": "tickets.record_csat_outcome",
            "runId": format!("ticket_csat_outcome_{}_{}", ticket_id, user.user_id),
            "status": "completed",
            "auditId": format!("audit_ticket_csat_outcome_{}_{}", ticket_id, user.user_id),
            "eventStream": "",
        }))),
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
    let url = format!("{}{}", state.conversation_core_url, path);
    let (status, Json(resp)) = proxy_conversation_json(state, method, &url, body, user, None).await;
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }
    let ticket_id = ticket_id_from_ticket_action_response(&resp);
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

// `tickets.create` is the first action migrated to an owner-issued operation
// receipt. The gateway validates and routes; it does not manufacture a run or
// audit identifier from the browser actor.
async fn forward_ticket_create_operation(
    state: &AppState,
    user: &AuthenticatedUser,
    body: Value,
) -> Response {
    let url = format!("{}/api/v1/tickets", state.conversation_core_url);
    let (status, Json(resp)) =
        proxy_conversation_json(state, Method::POST, &url, Some(body), user, None).await;
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }
    ticket_operation_receipt_response(resp)
}

fn ticket_operation_receipt_response(resp: Value) -> Response {
    let operation_id = resp
        .pointer("/data/operation/operation_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let audit_event_id = resp
        .pointer("/data/operation/audit_event_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let operation_status = resp
        .pointer("/data/operation/status")
        .and_then(Value::as_str)
        .filter(|value| *value == "completed");
    let (Some(operation_id), Some(audit_event_id), Some(operation_status)) =
        (operation_id, audit_event_id, operation_status)
    else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "ticket_operation_receipt_invalid",
                "Conversation Core returned no valid durable ticket operation receipt.",
            )),
        )
            .into_response();
    };
    let ticket_id = resp
        .pointer("/data/ticket/id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let replayed = resp
        .pointer("/data/operation/replayed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    (
        StatusCode::OK,
        Json(ok(json!({
            "actionId": "tickets.create",
            "runId": operation_id,
            "status": operation_status,
            "auditId": audit_event_id,
            "operationId": operation_id,
            "auditEventId": audit_event_id,
            "replayed": replayed,
            "ticketId": ticket_id,
            "result": resp,
        }))),
    )
        .into_response()
}

/// Reconcile an ambiguous ticket-create response through Conversation Core's
/// actor-bound owner receipt lookup. No create endpoint is called here.
pub(super) async fn reconcile_ticket_create(
    state: &AppState,
    user: &AuthenticatedUser,
    idempotency_key: &str,
) -> Response {
    let idempotency_key = idempotency_key.trim();
    if idempotency_key.is_empty()
        || idempotency_key.len() > 200
        || idempotency_key.chars().any(char::is_control)
    {
        return ticket_bad_request("tickets.create requires a bounded idempotencyKey");
    }
    let url = format!(
        "{}/api/v1/ticket-operations/{}",
        state.conversation_core_url,
        urlencoding::encode(idempotency_key),
    );
    let (status, Json(resp)) =
        proxy_conversation_json(state, Method::GET, &url, None, user, None).await;
    if !status.is_success() {
        return (status, Json(resp)).into_response();
    }
    ticket_operation_receipt_response(resp)
}

/// Most ticket actions return the ticket itself as `data`. Ticket
/// classification is intentionally different: it returns a classification
/// record with the newly-created suggested ticket nested at `data.ticket`.
/// Keep the action envelope's `ticketId` canonical in both cases so callers
/// can safely open the ticket rather than accidentally navigating to an AI
/// action record.
fn ticket_id_from_ticket_action_response(resp: &Value) -> String {
    resp.pointer("/data/ticket/id")
        .or_else(|| resp.pointer("/data/id"))
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string()
}

pub(super) async fn dispatch_ticket_create(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
    idempotency_key: &str,
) -> Response {
    let conversation_id = input
        .get("conversationId")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    if conversation_id.is_empty() {
        return ticket_bad_request("tickets.create requires a non-empty 'conversationId'");
    }
    let idempotency_key = idempotency_key.trim();
    if idempotency_key.is_empty() || idempotency_key.len() > 200 {
        return ticket_bad_request("tickets.create requires a bounded idempotencyKey");
    }
    let body = ticket_create_body(input, idempotency_key);
    forward_ticket_create_operation(state, user, Value::Object(body)).await
}

fn ticket_create_body(input: &Value, idempotency_key: &str) -> serde_json::Map<String, Value> {
    let mut body = ticket_remap(
        input,
        &[
            ("conversationId", "conversation_id"),
            ("workType", "work_type"),
            ("priority", "priority"),
            ("severity", "severity"),
            ("category", "category"),
            ("intent", "intent"),
            ("dueAt", "due_at"),
            ("followUpAt", "follow_up_at"),
            ("snoozedUntil", "snoozed_until"),
        ],
    );
    body.insert(
        "idempotency_key".to_owned(),
        Value::String(idempotency_key.trim().to_owned()),
    );
    body
}

pub(super) async fn dispatch_ticket_classify(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    if let Err(response) = require_support_ai_review(state, user).await {
        return response;
    }
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
    let body = ticket_classification_body(input);
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
            ("workType", "work_type"),
            ("priority", "priority"),
            ("severity", "severity"),
            ("category", "category"),
            ("intent", "intent"),
            ("dueAt", "due_at"),
            ("followUpAt", "follow_up_at"),
            ("snoozedUntil", "snoozed_until"),
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
            ("linkType", "link_type"),
            ("resourceKind", "resource_kind"),
            ("resourceId", "resource_id"),
            ("resourceUrl", "resource_url"),
            ("label", "label"),
            ("metadata", "metadata"),
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

pub(super) async fn dispatch_ticket_run_macro(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = input
        .get("ticketId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let macro_id = input
        .get("macroId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if ticket_id.is_empty() || macro_id.is_empty() {
        return ticket_bad_request("tickets.run_macro requires non-empty 'ticketId' and 'macroId'");
    }
    let expected_updated_at = input
        .get("expectedMacroUpdatedAt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let path = format!(
        "/api/v1/tickets/{}/macros/{}/run",
        urlencoding::encode(ticket_id),
        urlencoding::encode(macro_id),
    );
    let body = expected_updated_at.map(|value| json!({ "expected_updated_at": value }));
    forward_ticket_action(state, user, "tickets.run_macro", Method::POST, &path, body).await
}

pub(super) async fn dispatch_ticket_create_macro(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if name.is_empty() {
        return ticket_bad_request("tickets.create_macro requires a non-empty 'name'");
    }
    let description = input
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let status = input
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("waiting_team");
    if !matches!(status, "waiting_customer" | "waiting_team" | "resolved") {
        return ticket_bad_request(
            "tickets.create_macro status must be waiting_customer, waiting_team, or resolved",
        );
    }
    let visibility = input
        .get("visibility")
        .and_then(Value::as_str)
        .unwrap_or("team");
    if !matches!(visibility, "personal" | "team" | "org") {
        return ticket_bad_request(
            "tickets.create_macro visibility must be personal, team, or org",
        );
    }
    let mut body = serde_json::Map::new();
    body.insert("name".to_string(), Value::String(name.to_string()));
    body.insert("active".to_string(), Value::Bool(true));
    body.insert(
        "visibility".to_string(),
        Value::String(visibility.to_string()),
    );
    body.insert("actions".to_string(), json!({ "status": status }));
    body.insert("conditions".to_string(), json!({}));
    if let Some(description) = description {
        body.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    forward_ticket_action(
        state,
        user,
        "tickets.create_macro",
        Method::POST,
        "/api/v1/ticket-macros",
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_create_checklist(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = input
        .get("ticketId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if ticket_id.is_empty() || name.is_empty() {
        return ticket_bad_request(
            "tickets.create_checklist requires non-empty 'ticketId' and 'name'",
        );
    }
    let body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("templateId", "template_id"),
            ("items", "items"),
        ],
    );
    let path = format!(
        "/api/v1/tickets/{}/checklists",
        urlencoding::encode(ticket_id)
    );
    forward_ticket_action(
        state,
        user,
        "tickets.create_checklist",
        Method::POST,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_update_checklist_item(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = input
        .get("ticketId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let checklist_id = input
        .get("checklistId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let item_id = input
        .get("itemId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let Some(completed) = input.get("completed").and_then(Value::as_bool) else {
        return ticket_bad_request("tickets.update_checklist_item requires boolean 'completed'");
    };
    if ticket_id.is_empty() || checklist_id.is_empty() || item_id.is_empty() {
        return ticket_bad_request("tickets.update_checklist_item requires non-empty ticket, checklist, and item identifiers");
    }
    let path = format!(
        "/api/v1/tickets/{}/checklists/{}/items/{}",
        urlencoding::encode(ticket_id),
        urlencoding::encode(checklist_id),
        urlencoding::encode(item_id),
    );
    forward_ticket_action(
        state,
        user,
        "tickets.update_checklist_item",
        Method::PATCH,
        &path,
        Some(json!({ "completed": completed })),
    )
    .await
}

pub(super) async fn dispatch_ticket_create_side_conversation(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = ticket_string(input, "ticketId");
    let subject = ticket_string(input, "subject");
    let body_text = ticket_string(input, "bodyText");
    if ticket_id.is_empty() || subject.is_empty() || body_text.is_empty() {
        return ticket_bad_request(
            "tickets.create_side_conversation requires non-empty ticketId, subject, and bodyText",
        );
    }
    let path = format!(
        "/api/v1/tickets/{}/side-conversations",
        urlencoding::encode(ticket_id)
    );
    forward_ticket_action(
        state,
        user,
        "tickets.create_side_conversation",
        Method::POST,
        &path,
        Some(json!({
            "subject": subject,
            "body_text": body_text,
        })),
    )
    .await
}

pub(super) async fn dispatch_ticket_add_side_conversation_message(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = ticket_string(input, "ticketId");
    let side_conversation_id = ticket_string(input, "sideConversationId");
    let body_text = ticket_string(input, "bodyText");
    if ticket_id.is_empty() || side_conversation_id.is_empty() || body_text.is_empty() {
        return ticket_bad_request("tickets.add_side_conversation_message requires non-empty ticketId, sideConversationId, and bodyText");
    }
    let path = format!(
        "/api/v1/tickets/{}/side-conversations/{}/messages",
        urlencoding::encode(ticket_id),
        urlencoding::encode(side_conversation_id),
    );
    forward_ticket_action(
        state,
        user,
        "tickets.add_side_conversation_message",
        Method::POST,
        &path,
        Some(json!({ "body_text": body_text })),
    )
    .await
}

pub(super) async fn dispatch_ticket_record_chat_handoff(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = ticket_string(input, "ticketId");
    if ticket_id.is_empty() {
        return ticket_bad_request("tickets.record_chat_handoff requires a non-empty ticketId");
    }
    let path = format!(
        "/api/v1/tickets/{}/chat-handoff",
        urlencoding::encode(ticket_id)
    );
    forward_ticket_action(
        state,
        user,
        "tickets.record_chat_handoff",
        Method::POST,
        &path,
        None,
    )
    .await
}

pub(super) async fn dispatch_ticket_update_side_conversation(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = ticket_string(input, "ticketId");
    let side_conversation_id = ticket_string(input, "sideConversationId");
    let status = ticket_string(input, "status");
    if ticket_id.is_empty()
        || side_conversation_id.is_empty()
        || !matches!(status, "open" | "closed")
    {
        return ticket_bad_request("tickets.update_side_conversation requires ticketId, sideConversationId, and open or closed status");
    }
    let path = format!(
        "/api/v1/tickets/{}/side-conversations/{}",
        urlencoding::encode(ticket_id),
        urlencoding::encode(side_conversation_id),
    );
    forward_ticket_action(
        state,
        user,
        "tickets.update_side_conversation",
        Method::PATCH,
        &path,
        Some(json!({ "status": status })),
    )
    .await
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
    fn owner_catalog_keeps_only_complete_human_contracts() {
        let schema = json!({"type": "object"});
        let digest = canonical_schema_sha256(&schema).expect("schema digest");
        let payload = json!({
            "data": {
                "catalog_version": "conversation-core/v1",
                "actions": [
                    {
                        "action_id": "tickets.create",
                        "owner_plane": "application",
                        "eligible_actor_types": ["human"],
                        "required_service_identity": "verevon-gateway",
                        "required_delegation": "verified_user_org_role",
                        "idempotency": "caller_supplied",
                        "receipt_contract": "durable_owner_receipt",
                        "schema_sha256": digest,
                        "input_schema": schema
                    },
                    {
                        "action_id": "tickets.create",
                        "owner_plane": "application",
                        "eligible_actor_types": ["model"],
                        "required_service_identity": "verevon-gateway",
                        "required_delegation": "verified_user_org_role",
                        "idempotency": "caller_supplied",
                        "receipt_contract": "durable_owner_receipt",
                        "schema_sha256": digest,
                        "input_schema": schema
                    }
                ]
            }
        });

        let catalog = human_owner_action_catalog(&payload).expect("valid catalog");
        assert_eq!(catalog["actorType"], "human");
        assert_eq!(catalog["actions"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn owner_catalog_does_not_expose_a_disabled_model_transport_to_the_browser() {
        let schema = json!({"type": "object"});
        let digest = canonical_schema_sha256(&schema).expect("schema digest");
        let payload = json!({
            "data": {
                "catalog_version": "conversation-core/v1",
                "actions": [{
                    "action_id": "tickets.create",
                    "owner_plane": "application",
                    "eligible_actor_types": ["human"],
                    "actor_requirements": [
                        {
                            "actor_type": "human",
                            "availability": "available",
                            "required_service_identity": "verevon-gateway",
                            "required_delegation": "verified_user_org_role",
                            "http_method": "POST",
                            "path": "/api/v1/tickets"
                        },
                        {
                            "actor_type": "model",
                            "availability": "not_enabled",
                            "required_service_identity": "execution-core",
                            "required_delegation": "control_target_action_decision",
                            "http_method": "POST",
                            "path": "/internal/v1/agent-ticket-operations"
                        }
                    ],
                    "required_service_identity": "verevon-gateway",
                    "required_delegation": "verified_user_org_role",
                    "idempotency": "caller_supplied",
                    "receipt_contract": "durable_owner_receipt",
                    "schema_sha256": digest,
                    "input_schema": schema
                }]
            }
        });

        let catalog = human_owner_action_catalog(&payload).expect("valid catalog");
        let action = &catalog["actions"][0];
        assert_eq!(action["eligible_actor_types"], json!(["human"]));
        assert_eq!(
            action["actor_requirements"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(action["actor_requirements"][0]["actor_type"], "human");
        assert!(!catalog.to_string().contains("agent-ticket-operations"));
    }

    #[test]
    fn owner_catalog_fails_closed_when_the_contract_is_incomplete() {
        let payload = json!({
            "data": {
                "catalog_version": "conversation-core/v1",
                "actions": [{
                    "action_id": "tickets.create",
                    "owner_plane": "application",
                    "eligible_actor_types": ["human"]
                }]
            }
        });

        assert!(human_owner_action_catalog(&payload).is_none());
    }

    #[test]
    fn owner_catalog_rejects_a_schema_with_a_forged_digest() {
        let payload = json!({
            "data": {
                "catalog_version": "conversation-core/v1",
                "actions": [{
                    "action_id": "tickets.create",
                    "owner_plane": "application",
                    "eligible_actor_types": ["human"],
                    "required_service_identity": "verevon-gateway",
                    "required_delegation": "verified_user_org_role",
                    "idempotency": "caller_supplied",
                    "receipt_contract": "durable_owner_receipt",
                    "schema_sha256": "sha256:forged",
                    "input_schema": {"type": "object"}
                }]
            }
        });

        assert!(human_owner_action_catalog(&payload).is_none());
    }

    #[test]
    fn ticket_remap_maps_camel_to_snake_and_omits_absent_or_null() {
        let input = json!({
            "conversationId": "conv_1",
            "priority": "high",
            "workType": "incident",
            "dueAt": "2026-08-04T10:00:00.000Z",
            "followUpAt": "2026-08-05T10:00:00.000Z",
            "snoozedUntil": "2026-08-03T10:00:00.000Z",
            "category": null,
        });
        let out = ticket_remap(
            &input,
            &[
                ("conversationId", "conversation_id"),
                ("priority", "priority"),
                ("workType", "work_type"),
                ("severity", "severity"),
                ("category", "category"),
                ("dueAt", "due_at"),
                ("followUpAt", "follow_up_at"),
                ("snoozedUntil", "snoozed_until"),
            ],
        );
        assert_eq!(
            out.get("conversation_id").and_then(|v| v.as_str()),
            Some("conv_1")
        );
        assert_eq!(out.get("priority").and_then(|v| v.as_str()), Some("high"));
        assert_eq!(
            out.get("work_type").and_then(|v| v.as_str()),
            Some("incident")
        );
        assert_eq!(
            out.get("due_at").and_then(|v| v.as_str()),
            Some("2026-08-04T10:00:00.000Z")
        );
        assert_eq!(
            out.get("follow_up_at").and_then(|v| v.as_str()),
            Some("2026-08-05T10:00:00.000Z")
        );
        assert_eq!(
            out.get("snoozed_until").and_then(|v| v.as_str()),
            Some("2026-08-03T10:00:00.000Z")
        );
        // absent field is omitted (not sent as null)...
        assert!(!out.contains_key("severity"));
        // ...and an explicit null is omitted too, so it never clobbers upstream state.
        assert!(!out.contains_key("category"));
    }

    #[test]
    fn ticket_create_body_preserves_every_current_catalog_field() {
        let body = ticket_create_body(
            &json!({
                "conversationId": "conv_1",
                "workType": "incident",
                "priority": "urgent",
                "severity": "critical",
                "category": "outage",
                "intent": "restore service",
            }),
            "ticket-create-1",
        );

        assert_eq!(body.get("conversation_id"), Some(&json!("conv_1")));
        assert_eq!(body.get("work_type"), Some(&json!("incident")));
        assert_eq!(body.get("priority"), Some(&json!("urgent")));
        assert_eq!(body.get("severity"), Some(&json!("critical")));
        assert_eq!(body.get("category"), Some(&json!("outage")));
        assert_eq!(body.get("intent"), Some(&json!("restore service")));
        assert_eq!(body.get("idempotency_key"), Some(&json!("ticket-create-1")));
    }

    #[test]
    fn inbox_classification_actions_are_always_submitted_for_human_review() {
        let input = json!({
            "conversationId": "conv_1",
            "confidence": 0.97,
            "reason": "Evidence supports a delivery case.",
            "suggestedFields": { "category": "delivery", "priority": "high" },
            "evidenceMessageIds": ["message_1"],
        });

        let out = ticket_classification_body(&input);

        assert_eq!(
            out.get("outcome").and_then(|value| value.as_str()),
            Some("suggest_ticket")
        );
        assert_eq!(
            out.get("confidence").and_then(|value| value.as_f64()),
            Some(0.97)
        );
        assert_eq!(
            out.get("suggested_fields")
                .and_then(|value| value.get("category"))
                .and_then(|value| value.as_str()),
            Some("delivery"),
        );
    }

    // --- forged-call denial for tickets.create -------------------------------
    //
    // tickets.create is the first action in MODEL_EXECUTABLE_ACTION_IDS, so a
    // model can now be offered it as a tool. The owner (conversation-core)
    // verifies the signed Control decision and is covered by its own suite; the
    // invariant THIS side must hold is narrower and just as load-bearing: the
    // gateway may never manufacture a receipt the owner did not issue. Every
    // identifier it returns has to come out of the owner's response, so a
    // caller can never be told an effect happened durably when it did not.

    fn receipt_status(resp: Value) -> StatusCode {
        ticket_operation_receipt_response(resp).status()
    }

    #[test]
    fn ticket_receipt_is_refused_when_the_owner_returned_no_operation() {
        // A ticket body with no operation block at all: the write may or may not
        // have happened, and the honest answer is a gateway error, never a 200.
        let resp = json!({ "data": { "ticket": { "id": "ticket_123" } } });
        assert_eq!(receipt_status(resp), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn ticket_receipt_is_refused_when_the_operation_is_not_completed() {
        // `reserved` means Control holds a reservation that was never committed.
        // Reporting that as success is exactly the false-success this contract
        // exists to prevent.
        let resp = json!({ "data": { "operation": {
            "operation_id": "ticketop_1",
            "audit_event_id": "audit_1",
            "status": "reserved"
        }}});
        assert_eq!(receipt_status(resp), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn ticket_receipt_is_refused_without_a_durable_audit_event() {
        // An operation id with no audit event is an unwitnessed effect. Clause 4
        // of the action contract is that everything the model does is visible,
        // so an unauditable receipt is not a receipt.
        let resp = json!({ "data": { "operation": {
            "operation_id": "ticketop_1",
            "status": "completed"
        }}});
        assert_eq!(receipt_status(resp), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn ticket_receipt_is_refused_when_owner_identifiers_are_blank() {
        // Present-but-empty is the shape a fabricated response takes when a
        // caller pads the contract to look complete.
        let resp = json!({ "data": { "operation": {
            "operation_id": "   ",
            "audit_event_id": "",
            "status": "completed"
        }}});
        assert_eq!(receipt_status(resp), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn ticket_receipt_echoes_only_owner_issued_identifiers() {
        // The success path: every id in the envelope is the OWNER's. The gateway
        // mints no run id and no audit id of its own -- that substitution is what
        // made the pre-migration synthetic ids unverifiable.
        let resp = json!({ "data": {
            "operation": {
                "operation_id": "ticketop_aeabd1dd",
                "audit_event_id": "audit_9d2c1644",
                "status": "completed",
                "replayed": true
            },
            "ticket": { "id": "ticket_8dffc325" }
        }});
        let response = ticket_operation_receipt_response(resp);
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn ticket_action_response_uses_nested_ticket_for_classifications() {
        let response = json!({
            "data": {
                "id": "aiact_123",
                "ticket": { "id": "ticket_456" }
            }
        });

        assert_eq!(
            ticket_id_from_ticket_action_response(&response),
            "ticket_456"
        );
    }

    #[test]
    fn ticket_action_response_uses_direct_ticket_for_other_actions() {
        let response = json!({ "data": { "id": "ticket_456" } });

        assert_eq!(
            ticket_id_from_ticket_action_response(&response),
            "ticket_456"
        );
    }

    #[tokio::test]
    async fn ticket_create_body_strips_sensitive_and_control_only_fields_before_conversation_upstream(
    ) {
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, MockServer, ResponseTemplate,
        };

        let conversation_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/tickets"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "data": {
                    "operation": {
                        "operation_id": "op_1",
                        "audit_event_id": "audit_1",
                        "status": "completed"
                    },
                    "ticket": { "id": "ticket_1", "conversation_id": "conv_1" }
                }
            })))
            .mount(&conversation_core)
            .await;

        let mut state = crate::tests::test_state(false);
        state.conversation_core_url = conversation_core.uri();
        let user = crate::middleware::AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "Proof User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("owner".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "owner".to_owned(),
            }),
        };

        let input = json!({
            "conversationId": "conv_1",
            "workType": "customer_case",
            "priority": "high",
            "controlDecisionToken": "forged-control-dec",
            "space_decision_token": "forged-space-token",
            "payloadDigest": "forged-digest",
            "space_ref": "room-1",
            "decision": "forged-decision",
        });

        let response = dispatch_ticket_create(&state, &user, &input, "idem-proof-1").await;
        assert_eq!(response.status(), StatusCode::OK);

        let received = conversation_core
            .received_requests()
            .await
            .expect("Conversation Core request");
        let request = received
            .first()
            .expect("a Conversation Core ticket create request");
        let outbound: Value = serde_json::from_slice(&request.body).expect("request JSON body");

        assert_eq!(outbound["conversation_id"], "conv_1");
        assert_eq!(outbound["idempotency_key"], "idem-proof-1");
        assert_eq!(outbound["work_type"], "customer_case");
        assert_eq!(outbound["priority"], "high");
        assert!(outbound.get("controlDecisionToken").is_none());
        assert!(outbound.get("space_decision_token").is_none());
        assert!(outbound.get("payloadDigest").is_none());
        assert!(outbound.get("space_ref").is_none());
        assert!(outbound.get("decision").is_none());
    }
}
