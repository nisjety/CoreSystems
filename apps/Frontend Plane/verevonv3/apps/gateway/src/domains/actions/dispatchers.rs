use axum::{
    extract::{Extension as ExtensionExtractor, Path as PathExtractor, State as StateExtractor},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::domains::agents_runs::approve_plan;
use crate::domains::ai::{proxy_dictate, proxy_speech};
use crate::domains::browser::{
    close_session as close_browser_session, close_tab as close_browser_tab, control_ai_run,
    create_browser_profile, create_session as create_browser_session,
    delete_profile as delete_browser_profile, new_tab as new_browser_tab, rename_browser_profile,
    restore_profile_probe as restore_browser_profile_probe, run_action as run_browser_action,
    select_tab as select_browser_tab, set_control_mode as set_browser_control_mode,
    start_ai_run as start_browser_ai_run, suggest_action as suggest_browser_action,
    ActionBody as BrowserActionBody, BrowserActionActor, BrowserProfileScope,
    ControlBody as BrowserControlBody, CreateProfileBody, CreateSessionBody, NewTabBody,
    RenameProfileBody, RestoreProbeBody, StartAiRunBody, SuggestActionBody,
    Viewport as BrowserViewport,
};
use crate::domains::chat::history::{
    clear_threads as clear_chat_threads, delete_thread as delete_chat_thread,
    save_thread as save_chat_thread, SaveThreadRequest,
};
use crate::domains::chat::json_handlers::{
    cancel_invocation as cancel_chat_invocation,
    queue_invocation_input as queue_chat_invocation_input, submit_feedback as submit_chat_feedback,
};
use crate::domains::inbox::{create_ai_text_proposal, require_support_ai_review};
use crate::domains::ingestions::runs::create_run as create_ingestion_run;
use crate::domains::ingestions::schedules::{
    create_schedule as create_ingestion_schedule, schedule_actions as run_ingestion_schedule_action,
};
use crate::domains::ingestions::sources::{
    create_source as create_ingestion_source, delete_source as delete_ingestion_source,
};
use crate::domains::integrations::connections::{
    disconnect as disconnect_integration_connection,
    extend_inbox_history as extend_integration_inbox_history,
    trigger_inbox_sync as trigger_integration_inbox_sync, trigger_sync as trigger_integration_sync,
};
use crate::domains::integrations::model_subscriptions::{
    disconnect_openai_codex_subscription, start_openai_codex_subscription,
};
use crate::domains::integrations::providers::start_connect_session as start_integration_connect_session;
use crate::domains::knowledge::documents::create_document;
use crate::domains::knowledge::products::{extract_products, summarize_products};
use crate::domains::mcp::{delete_server as delete_mcp_server, share_server as share_mcp_server};
use crate::domains::memory::delete_memory;
use crate::domains::monitoring::{check_now, CheckRequest};
use crate::domains::orchestration::{
    cancel_run as cancel_orchestration_run, decide_approval as decide_orchestration_approval,
    resume_run as resume_orchestration_run,
};
use crate::domains::orgs::deletion::{
    acknowledge as acknowledge_org_deletion, mark_exported as mark_org_exported,
    restore as restore_org, soft_delete as soft_delete_org,
};
use crate::domains::orgs::instructions::{update_org_instructions, UpdateOrgInstructionsRequest};
use crate::domains::orgs::members::{
    invite_member as invite_org_member, remove_member as remove_org_member,
    update_member_role as update_org_member_role,
};
use crate::domains::orgs::quotas::set_quota as set_org_quota;
use crate::domains::orgs::settings::update_org_settings;
use crate::domains::orgs::switch::switch_active_org;
use crate::domains::privacy::erase as erase_my_account;
use crate::domains::router_policy::put_policy as put_router_policy;
use crate::domains::settings::api_keys::{create_api_key, delete_api_key};
use crate::domains::social::{
    create_campaign, create_draft_from_inbox, decide_approval, CreateCampaignBody,
    DecideApprovalBody, InboxDraftBody,
};
use crate::domains::spaces::{
    bind_existing_space_agent, create_personal_space, create_space_agent, ensure_organization_room,
    request_personal_space_deletion, update_space_instructions, DeleteSpaceRequest,
    UpdateSpaceInstructionsRequest,
};
use crate::domains::studio::{
    create_project as create_studio_project, export_social_draft as export_studio_social_draft,
    update_project as update_studio_project, CreateProjectBody, ExportSocialDraftBody, StudioBlock,
    UpdateProjectBody,
};
use crate::{
    audience_tokens::get_audience_token,
    config::AppState,
    contracts::ActionActor,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    upstream::{
        proxy_bearer_json, proxy_bearer_json_with_headers, proxy_conversation_json, proxy_json,
        proxy_notification_json,
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

// The support-ops dispatchers below (incidents/problems/SLA/automation/teams/
// views/macro-update) all forward to conversation-core routes confirmed
// mounted in domains::tickets (apps/gateway/src/domains/tickets.rs, wired at
// main.rs). Automation rule conditions/actions are validated as a real
// allowlist by conversation-core-go's normalizeTicketAutomationRule (7
// condition keys, 6 action keys, enum-checked values, 1-4/1-3 count bounds),
// so this gateway forwards the body unmodified rather than re-deriving that
// allowlist and risking drift between two copies of it.
pub(super) async fn dispatch_ticket_create_incident(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let title = ticket_string(input, "title");
    if title.is_empty() {
        return ticket_bad_request("tickets.create_incident requires a non-empty 'title'");
    }
    let body = ticket_remap(
        input,
        &[
            ("title", "title"),
            ("status", "status"),
            ("severity", "severity"),
            ("ownerUserId", "owner_user_id"),
            ("ownerName", "owner_name"),
            ("customerImpact", "customer_impact"),
            ("problemId", "problem_id"),
        ],
    );
    forward_ticket_action(
        state,
        user,
        "tickets.create_incident",
        Method::POST,
        "/api/v1/incidents",
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_update_incident(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let incident_id = ticket_string(input, "incidentId");
    if incident_id.is_empty() {
        return ticket_bad_request("tickets.update_incident requires a non-empty 'incidentId'");
    }
    let body = ticket_remap(
        input,
        &[
            ("title", "title"),
            ("status", "status"),
            ("severity", "severity"),
            ("ownerUserId", "owner_user_id"),
            ("ownerName", "owner_name"),
            ("customerImpact", "customer_impact"),
            ("problemId", "problem_id"),
        ],
    );
    let path = format!("/api/v1/incidents/{}", urlencoding::encode(incident_id));
    forward_ticket_action(
        state,
        user,
        "tickets.update_incident",
        Method::PATCH,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_link_incident_ticket(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let incident_id = ticket_string(input, "incidentId");
    let ticket_id = ticket_string(input, "ticketId");
    let relationship = ticket_string(input, "relationship");
    if incident_id.is_empty()
        || ticket_id.is_empty()
        || !matches!(relationship, "affected" | "root_cause" | "related")
    {
        return ticket_bad_request(
            "tickets.link_incident_ticket requires incidentId, ticketId, and an affected, root_cause, or related relationship",
        );
    }
    let path = format!(
        "/api/v1/incidents/{}/tickets",
        urlencoding::encode(incident_id)
    );
    forward_ticket_action(
        state,
        user,
        "tickets.link_incident_ticket",
        Method::POST,
        &path,
        Some(json!({ "ticket_id": ticket_id, "relationship": relationship })),
    )
    .await
}

pub(super) async fn dispatch_ticket_create_problem(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let title = ticket_string(input, "title");
    if title.is_empty() {
        return ticket_bad_request("tickets.create_problem requires a non-empty 'title'");
    }
    let body = ticket_remap(
        input,
        &[
            ("title", "title"),
            ("status", "status"),
            ("ownerUserId", "owner_user_id"),
            ("ownerName", "owner_name"),
            ("summary", "summary"),
            ("rootCause", "root_cause"),
        ],
    );
    forward_ticket_action(
        state,
        user,
        "tickets.create_problem",
        Method::POST,
        "/api/v1/problems",
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_update_problem(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let problem_id = ticket_string(input, "problemId");
    if problem_id.is_empty() {
        return ticket_bad_request("tickets.update_problem requires a non-empty 'problemId'");
    }
    let body = ticket_remap(
        input,
        &[
            ("title", "title"),
            ("status", "status"),
            ("ownerUserId", "owner_user_id"),
            ("ownerName", "owner_name"),
            ("summary", "summary"),
            ("rootCause", "root_cause"),
        ],
    );
    let path = format!("/api/v1/problems/{}", urlencoding::encode(problem_id));
    forward_ticket_action(
        state,
        user,
        "tickets.update_problem",
        Method::PATCH,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_create_sla_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let name = ticket_string(input, "name");
    if name.is_empty() {
        return ticket_bad_request("tickets.create_sla_policy requires a non-empty 'name'");
    }
    let body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("active", "active"),
            ("conditions", "conditions"),
            ("calendarRef", "calendar_ref"),
            ("firstResponseMinutes", "first_response_minutes"),
            ("nextResponseMinutes", "next_response_minutes"),
            ("resolutionMinutes", "resolution_minutes"),
        ],
    );
    forward_ticket_action(
        state,
        user,
        "tickets.create_sla_policy",
        Method::POST,
        "/api/v1/sla-policies",
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_update_sla_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let policy_id = ticket_string(input, "policyId");
    if policy_id.is_empty() {
        return ticket_bad_request("tickets.update_sla_policy requires a non-empty 'policyId'");
    }
    let body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("active", "active"),
            ("conditions", "conditions"),
            ("calendarRef", "calendar_ref"),
            ("firstResponseMinutes", "first_response_minutes"),
            ("nextResponseMinutes", "next_response_minutes"),
            ("resolutionMinutes", "resolution_minutes"),
        ],
    );
    let path = format!("/api/v1/sla-policies/{}", urlencoding::encode(policy_id));
    forward_ticket_action(
        state,
        user,
        "tickets.update_sla_policy",
        Method::PATCH,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_create_automation_rule(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let name = ticket_string(input, "name");
    let event_name = ticket_string(input, "eventName");
    if name.is_empty() || !matches!(event_name, "ticket.created" | "ticket.updated") {
        return ticket_bad_request(
            "tickets.create_automation_rule requires a non-empty 'name' and eventName of ticket.created or ticket.updated",
        );
    }
    let body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("eventName", "event_name"),
            ("active", "active"),
            ("conditions", "conditions"),
            ("actions", "actions"),
        ],
    );
    forward_ticket_action(
        state,
        user,
        "tickets.create_automation_rule",
        Method::POST,
        "/api/v1/ticket-automation-rules",
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_update_automation_rule(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let rule_id = ticket_string(input, "ruleId");
    if rule_id.is_empty() {
        return ticket_bad_request("tickets.update_automation_rule requires a non-empty 'ruleId'");
    }
    if let Some(event_name) = input.get("eventName").and_then(Value::as_str) {
        if !matches!(event_name, "ticket.created" | "ticket.updated") {
            return ticket_bad_request(
                "tickets.update_automation_rule eventName must be ticket.created or ticket.updated",
            );
        }
    }
    let body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("eventName", "event_name"),
            ("active", "active"),
            ("conditions", "conditions"),
            ("actions", "actions"),
        ],
    );
    let path = format!(
        "/api/v1/ticket-automation-rules/{}",
        urlencoding::encode(rule_id)
    );
    forward_ticket_action(
        state,
        user,
        "tickets.update_automation_rule",
        Method::PATCH,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_create_team(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let name = ticket_string(input, "name");
    if name.is_empty() {
        return ticket_bad_request("tickets.create_team requires a non-empty 'name'");
    }
    let body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("description", "description"),
            ("active", "active"),
        ],
    );
    forward_ticket_action(
        state,
        user,
        "tickets.create_team",
        Method::POST,
        "/api/v1/ticket-teams",
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_update_team(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let team_id = ticket_string(input, "teamId");
    if team_id.is_empty() {
        return ticket_bad_request("tickets.update_team requires a non-empty 'teamId'");
    }
    let body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("description", "description"),
            ("active", "active"),
        ],
    );
    let path = format!("/api/v1/ticket-teams/{}", urlencoding::encode(team_id));
    forward_ticket_action(
        state,
        user,
        "tickets.update_team",
        Method::PATCH,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_create_view(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    if let Some(scope) = input.get("scope").and_then(Value::as_str) {
        if !matches!(scope, "org" | "user" | "team") {
            return ticket_bad_request("tickets.create_view scope must be org, user, or team");
        }
    }
    if let Some(visibility) = input.get("visibility").and_then(Value::as_str) {
        if !matches!(visibility, "sidebar" | "hidden") {
            return ticket_bad_request("tickets.create_view visibility must be sidebar or hidden");
        }
    }
    let body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("scope", "scope"),
            ("ownerUserId", "owner_user_id"),
            ("teamId", "team_id"),
            ("visibility", "visibility"),
            ("filter", "filter"),
            ("sort", "sort"),
            ("groupBy", "group_by"),
            ("sidebarOrder", "sidebar_order"),
        ],
    );
    forward_ticket_action(
        state,
        user,
        "tickets.create_view",
        Method::POST,
        "/api/v1/ticket-views",
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_update_view(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let view_id = ticket_string(input, "viewId");
    if view_id.is_empty() {
        return ticket_bad_request("tickets.update_view requires a non-empty 'viewId'");
    }
    if let Some(scope) = input.get("scope").and_then(Value::as_str) {
        if !matches!(scope, "org" | "user" | "team") {
            return ticket_bad_request("tickets.update_view scope must be org, user, or team");
        }
    }
    if let Some(visibility) = input.get("visibility").and_then(Value::as_str) {
        if !matches!(visibility, "sidebar" | "hidden") {
            return ticket_bad_request("tickets.update_view visibility must be sidebar or hidden");
        }
    }
    let body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("scope", "scope"),
            ("ownerUserId", "owner_user_id"),
            ("teamId", "team_id"),
            ("visibility", "visibility"),
            ("filter", "filter"),
            ("sort", "sort"),
            ("groupBy", "group_by"),
            ("sidebarOrder", "sidebar_order"),
        ],
    );
    let path = format!("/api/v1/ticket-views/{}", urlencoding::encode(view_id));
    forward_ticket_action(
        state,
        user,
        "tickets.update_view",
        Method::PATCH,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

pub(super) async fn dispatch_ticket_update_macro(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let macro_id = ticket_string(input, "macroId");
    if macro_id.is_empty() {
        return ticket_bad_request("tickets.update_macro requires a non-empty 'macroId'");
    }
    if let Some(visibility) = input.get("visibility").and_then(Value::as_str) {
        if !matches!(visibility, "personal" | "team" | "org") {
            return ticket_bad_request(
                "tickets.update_macro visibility must be personal, team, or org",
            );
        }
    }
    let mut body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("description", "description"),
            ("visibility", "visibility"),
            ("active", "active"),
        ],
    );
    // Matches tickets.create_macro's posture: only a status transition is
    // exposed through this generic action surface, not arbitrary
    // actions/conditions editing. The dedicated macro-builder UI (which calls
    // updateTicketMacro directly, not through action execution) is the place
    // for that richer editing.
    if let Some(status) = input.get("status").and_then(Value::as_str) {
        if !matches!(status, "waiting_customer" | "waiting_team" | "resolved") {
            return ticket_bad_request(
                "tickets.update_macro status must be waiting_customer, waiting_team, or resolved",
            );
        }
        body.insert("actions".to_string(), json!({ "status": status }));
    }
    if body.is_empty() {
        return ticket_bad_request("tickets.update_macro requires at least one field to update");
    }
    let path = format!("/api/v1/ticket-macros/{}", urlencoding::encode(macro_id));
    forward_ticket_action(
        state,
        user,
        "tickets.update_macro",
        Method::PATCH,
        &path,
        Some(Value::Object(body)),
    )
    .await
}

// The dispatchers below call the real Spaces route handlers directly rather
// than re-deriving their logic against convex_gateway_call. Those handlers
// carry safety-relevant behavior a parallel reimplementation could drift from
// or silently drop: require_space_agent_grant_role (a per-room grant, not an
// org-wide role, and never assertable from the request body), the two-step
// create/bind-then-confirm-membership sequence, and per-field length limits
// enforced nowhere else. Reusing the exact function is the only way to
// guarantee none of that is bypassed.
fn owner_json_response_to_envelope(
    action_id: &str,
    user: &AuthenticatedUser,
    status: StatusCode,
    payload: Value,
) -> Response {
    if !status.is_success() {
        return (status, Json(payload)).into_response();
    }
    let result = payload.get("data").cloned().unwrap_or(payload);
    (
        StatusCode::OK,
        Json(ok(json!({
            "actionId": action_id,
            "runId": format!("{}_{}", action_id, user.user_id),
            "status": "completed",
            "auditId": format!("audit_{}_{}", action_id, user.user_id),
            "result": result,
        }))),
    )
        .into_response()
}

// Unlike the ticket/space dispatchers above, these three owner handlers
// already return a complete `Response` rather than `(StatusCode, Json<Value>)`
// — buffering and re-parsing the body is the price of reusing the real
// handler (see the comment on owner_json_response_to_envelope above) instead
// of duplicating create_document's Data Plane call or extract_products' /
// summarize_products' Model Plane prompt-and-parse logic.
async fn response_to_status_and_json(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let payload: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({}));
    (status, payload)
}

pub(super) async fn dispatch_knowledge_create_document(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let content = input.get("content").and_then(Value::as_str).unwrap_or("");
    if content.trim().is_empty() {
        return ticket_bad_request("knowledge.create_document requires a non-empty 'content'");
    }
    // sourceUrl is forwarded under its camelCase name unchanged:
    // create_document itself accepts either sourceUrl or source_url.
    let body = ticket_remap(
        input,
        &[
            ("content", "content"),
            ("sourceUrl", "sourceUrl"),
            ("title", "title"),
            ("type", "type"),
        ],
    );
    let response = create_document(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(Value::Object(body)),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("knowledge.create_document", user, status, payload)
}

pub(super) async fn dispatch_knowledge_extract_products(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let url = input.get("url").and_then(Value::as_str).unwrap_or("");
    if url.trim().is_empty() {
        return ticket_bad_request("knowledge.extract_products requires a non-empty 'url'");
    }
    let body = ticket_remap(input, &[("url", "url"), ("prompt", "prompt")]);
    let response = extract_products(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(Value::Object(body)),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("knowledge.extract_products", user, status, payload)
}

pub(super) async fn dispatch_knowledge_summarize_products(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let has_products = input
        .get("products")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty());
    if !has_products {
        return ticket_bad_request(
            "knowledge.summarize_products requires a non-empty 'products' array",
        );
    }
    let body = ticket_remap(input, &[("products", "products"), ("prompt", "prompt")]);
    let response = summarize_products(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(Value::Object(body)),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("knowledge.summarize_products", user, status, payload)
}

// These three reuse the real social.rs handlers rather than re-deriving their
// logic, the same reasoning as the Space and knowledge dispatchers above:
// create_draft_from_inbox composes an actual draft title/body/source from
// ticket fields (a canned template, not a passthrough), and duplicating that
// by hand here is exactly how the copy would drift from the real one.
pub(super) async fn dispatch_social_create_campaign(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return ticket_bad_request("social.create_campaign requires a non-empty 'name'");
    }
    let body = CreateCampaignBody {
        name: name.to_owned(),
        brief: input
            .get("brief")
            .and_then(Value::as_str)
            .map(str::to_owned),
        goal: input.get("goal").and_then(Value::as_str).map(str::to_owned),
        status: input
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_owned),
        platforms: input
            .get("platforms")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            }),
        starts_at: input
            .get("startsAt")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ends_at: input
            .get("endsAt")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    let response = create_campaign(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("social.create_campaign", user, status, payload)
}

pub(super) async fn dispatch_social_create_draft_from_inbox(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ticket_id = ticket_string(input, "ticketId");
    let ticket_title = ticket_string(input, "ticketTitle");
    if ticket_id.is_empty() || ticket_title.is_empty() {
        return ticket_bad_request(
            "social.create_draft_from_inbox requires non-empty 'ticketId' and 'ticketTitle'",
        );
    }
    let body = InboxDraftBody {
        ticket_id: ticket_id.to_owned(),
        ticket_title: ticket_title.to_owned(),
        support_ticket_id: input
            .get("supportTicketId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        conversation_id: input
            .get("conversationId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        customer_name: input
            .get("customerName")
            .and_then(Value::as_str)
            .map(str::to_owned),
        channel: input
            .get("channel")
            .and_then(Value::as_str)
            .map(str::to_owned),
        excerpt: input
            .get("excerpt")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    let response = create_draft_from_inbox(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("social.create_draft_from_inbox", user, status, payload)
}

pub(super) async fn dispatch_social_decide_approval(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let approval_id = ticket_string(input, "approvalId");
    let decision = ticket_string(input, "decision");
    if approval_id.is_empty() || !matches!(decision, "approved" | "rejected") {
        return ticket_bad_request(
            "social.decide_approval requires 'approvalId' and a decision of approved or rejected",
        );
    }
    let reason = input
        .get("reason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let response = decide_approval(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(approval_id.to_owned()),
        Json(DecideApprovalBody {
            decision: decision.to_owned(),
            reason,
        }),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("social.decide_approval", user, status, payload)
}

// The eight simple inbox dispatchers below proxy directly to conversation-core
// (matching this file's own dispatch_conversation_follow precedent) rather
// than reusing inbox.rs's forward_conversation_write: that helper is a plain
// URL-template-and-proxy with no gate of its own, so there's nothing here a
// hand-written direct proxy could drift from.
pub(super) async fn dispatch_inbox_add_tag(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let tag = ticket_string(input, "tag");
    if conversation_id.is_empty() || tag.is_empty() {
        return ticket_bad_request("inbox.add_tag requires non-empty 'conversationId' and 'tag'");
    }
    let url = format!(
        "{}/api/v1/conversations/{}/tags",
        state.conversation_core_url,
        urlencoding::encode(conversation_id)
    );
    let (status, Json(resp)) = proxy_conversation_json(
        state,
        Method::POST,
        &url,
        Some(json!({ "tag": tag })),
        user,
        None,
    )
    .await;
    owner_json_response_to_envelope("inbox.add_tag", user, status, resp)
}

pub(super) async fn dispatch_inbox_remove_tag(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let tag = ticket_string(input, "tag");
    if conversation_id.is_empty() || tag.is_empty() {
        return ticket_bad_request(
            "inbox.remove_tag requires non-empty 'conversationId' and 'tag'",
        );
    }
    let url = format!(
        "{}/api/v1/conversations/{}/tags/{}",
        state.conversation_core_url,
        urlencoding::encode(conversation_id),
        urlencoding::encode(tag)
    );
    let (status, Json(resp)) =
        proxy_conversation_json(state, Method::DELETE, &url, None, user, None).await;
    owner_json_response_to_envelope("inbox.remove_tag", user, status, resp)
}

pub(super) async fn dispatch_inbox_claim_draft_lease(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    if conversation_id.is_empty() {
        return ticket_bad_request("inbox.claim_draft_lease requires a non-empty 'conversationId'");
    }
    let url = format!(
        "{}/api/v1/conversations/{}/draft-lease",
        state.conversation_core_url,
        urlencoding::encode(conversation_id)
    );
    let (status, Json(resp)) =
        proxy_conversation_json(state, Method::POST, &url, None, user, None).await;
    owner_json_response_to_envelope("inbox.claim_draft_lease", user, status, resp)
}

pub(super) async fn dispatch_inbox_release_draft_lease(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    if conversation_id.is_empty() {
        return ticket_bad_request(
            "inbox.release_draft_lease requires a non-empty 'conversationId'",
        );
    }
    let url = format!(
        "{}/api/v1/conversations/{}/draft-lease",
        state.conversation_core_url,
        urlencoding::encode(conversation_id)
    );
    let (status, Json(resp)) =
        proxy_conversation_json(state, Method::DELETE, &url, None, user, None).await;
    owner_json_response_to_envelope("inbox.release_draft_lease", user, status, resp)
}

pub(super) async fn dispatch_inbox_save_draft(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let Some(internal) = input.get("internal").and_then(Value::as_bool) else {
        return ticket_bad_request("inbox.save_draft requires boolean 'internal'");
    };
    if conversation_id.is_empty() {
        return ticket_bad_request("inbox.save_draft requires a non-empty 'conversationId'");
    }
    let body_text = ticket_string(input, "bodyText");
    // Mirrors inbox.rs's require_draft_persistence: an active org is required
    // before a draft can be persisted at all.
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let url = format!(
        "{}/api/v1/conversations/{}/draft",
        state.conversation_core_url,
        urlencoding::encode(conversation_id)
    );
    let (status, Json(resp)) = proxy_conversation_json(
        state,
        Method::PUT,
        &url,
        Some(json!({ "body_text": body_text, "internal": internal })),
        user,
        None,
    )
    .await;
    owner_json_response_to_envelope("inbox.save_draft", user, status, resp)
}

pub(super) async fn dispatch_inbox_delete_draft(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    if conversation_id.is_empty() {
        return ticket_bad_request("inbox.delete_draft requires a non-empty 'conversationId'");
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let url = format!(
        "{}/api/v1/conversations/{}/draft",
        state.conversation_core_url,
        urlencoding::encode(conversation_id)
    );
    let (status, Json(resp)) =
        proxy_conversation_json(state, Method::DELETE, &url, None, user, None).await;
    owner_json_response_to_envelope("inbox.delete_draft", user, status, resp)
}

pub(super) async fn dispatch_inbox_set_status(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let status_value = ticket_string(input, "status");
    if conversation_id.is_empty() || status_value.is_empty() {
        return ticket_bad_request(
            "inbox.set_status requires non-empty 'conversationId' and 'status'",
        );
    }
    let url = format!(
        "{}/api/v1/conversations/{}/status",
        state.conversation_core_url,
        urlencoding::encode(conversation_id)
    );
    let (status, Json(resp)) = proxy_conversation_json(
        state,
        Method::PATCH,
        &url,
        Some(json!({ "status": status_value })),
        user,
        None,
    )
    .await;
    owner_json_response_to_envelope("inbox.set_status", user, status, resp)
}

pub(super) async fn dispatch_inbox_set_assignment(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let assignee_user_id = ticket_string(input, "assigneeUserId");
    let assignee_name = ticket_string(input, "assigneeName");
    if conversation_id.is_empty() || assignee_user_id.is_empty() || assignee_name.is_empty() {
        return ticket_bad_request(
            "inbox.set_assignment requires non-empty 'conversationId', 'assigneeUserId', and 'assigneeName'",
        );
    }
    let url = format!(
        "{}/api/v1/conversations/{}/assignment",
        state.conversation_core_url,
        urlencoding::encode(conversation_id)
    );
    let (status, Json(resp)) = proxy_conversation_json(
        state,
        Method::PATCH,
        &url,
        Some(json!({ "assignee_user_id": assignee_user_id, "assignee_name": assignee_name })),
        user,
        None,
    )
    .await;
    owner_json_response_to_envelope("inbox.set_assignment", user, status, resp)
}

pub(super) async fn dispatch_inbox_send_reply(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let body_text = ticket_string(input, "body");
    let idempotency_key = ticket_string(input, "idempotencyKey");
    let Some(internal) = input.get("internal").and_then(Value::as_bool) else {
        return ticket_bad_request("inbox.send_reply requires boolean 'internal'");
    };
    if conversation_id.is_empty() || body_text.is_empty() || idempotency_key.is_empty() {
        return ticket_bad_request(
            "inbox.send_reply requires non-empty 'conversationId', 'body', and 'idempotencyKey'",
        );
    }
    let path = if internal { "notes" } else { "messages" };
    let url = format!(
        "{}/api/v1/conversations/{}/{}",
        state.conversation_core_url,
        urlencoding::encode(conversation_id),
        path
    );
    let (status, Json(resp)) = proxy_conversation_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "body_text": body_text,
            "internal": internal,
            "idempotency_key": idempotency_key,
        })),
        user,
        None,
    )
    .await;
    owner_json_response_to_envelope("inbox.send_reply", user, status, resp)
}

pub(super) async fn dispatch_inbox_submit_feedback(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let body_text = ticket_string(input, "bodyText");
    if body_text.is_empty() {
        return ticket_bad_request("inbox.submit_feedback requires a non-empty 'bodyText'");
    }
    let body = ticket_remap(
        input,
        &[
            ("bodyText", "body_text"),
            ("fromName", "from_name"),
            ("fromEmail", "from_email"),
            ("pageUrl", "page_url"),
        ],
    );
    let url = format!("{}/api/v1/feedback", state.conversation_core_url);
    let (status, Json(resp)) = proxy_conversation_json(
        state,
        Method::POST,
        &url,
        Some(Value::Object(body)),
        user,
        None,
    )
    .await;
    owner_json_response_to_envelope("inbox.submit_feedback", user, status, resp)
}

pub(super) async fn dispatch_inbox_review_ai_action(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let ai_action_id = ticket_string(input, "aiActionId");
    let decision = ticket_string(input, "decision");
    if ai_action_id.is_empty() || !matches!(decision, "approve" | "reject") {
        return ticket_bad_request(
            "inbox.review_ai_action requires 'aiActionId' and a decision of approve or reject",
        );
    }
    let mut body = serde_json::Map::new();
    if let Some(comment) = input
        .get("comment")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.insert("comment".to_owned(), Value::String(comment.to_owned()));
    }
    // Matches reviewAiAction's own client-side rule: edited_fields is only
    // meaningful, and only ever forwarded, alongside an approve decision.
    if decision == "approve" {
        if let Some(edited) = input.get("editedFields").and_then(Value::as_object) {
            if !edited.is_empty() {
                body.insert("edited_fields".to_owned(), Value::Object(edited.clone()));
            }
        }
    }
    let url = format!(
        "{}/api/v1/ai-actions/{}/{}",
        state.conversation_core_url,
        urlencoding::encode(ai_action_id),
        decision
    );
    let (status, Json(resp)) = proxy_conversation_json(
        state,
        Method::POST,
        &url,
        Some(Value::Object(body)),
        user,
        None,
    )
    .await;
    owner_json_response_to_envelope("inbox.review_ai_action", user, status, resp)
}

// The five dispatchers below all reuse inbox.rs's create_ai_text_proposal
// directly rather than proxying to /api/v1/ai-actions by hand: it gates on
// require_support_ai_review (an AI-review eligibility check, not just org
// membership) before any proposal reaches conversation-core, and that gate
// must never have a second, driftable copy.
pub(super) async fn dispatch_inbox_create_draft_reply_proposal(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let body_text = ticket_string(input, "bodyText");
    if conversation_id.is_empty() || body_text.is_empty() {
        return ticket_bad_request(
            "inbox.create_draft_reply_proposal requires non-empty 'conversationId' and 'bodyText'",
        );
    }
    let mut body = ticket_remap(
        input,
        &[
            ("conversationId", "conversation_id"),
            ("bodyText", "body_text"),
            ("proposalGroupId", "proposal_group_id"),
        ],
    );
    body.insert("kind".to_owned(), json!("draft.reply"));
    let response = create_ai_text_proposal(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        Json(Value::Object(body)),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("inbox.create_draft_reply_proposal", user, status, payload)
}

pub(super) async fn dispatch_inbox_create_internal_note_proposal(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let body_text = ticket_string(input, "bodyText");
    if conversation_id.is_empty() || body_text.is_empty() {
        return ticket_bad_request(
            "inbox.create_internal_note_proposal requires non-empty 'conversationId' and 'bodyText'",
        );
    }
    let mut body = ticket_remap(
        input,
        &[
            ("conversationId", "conversation_id"),
            ("bodyText", "body_text"),
            ("proposalGroupId", "proposal_group_id"),
        ],
    );
    body.insert("kind".to_owned(), json!("internal.note"));
    let response = create_ai_text_proposal(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        Json(Value::Object(body)),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("inbox.create_internal_note_proposal", user, status, payload)
}

pub(super) async fn dispatch_inbox_create_ticket_update_proposal(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let ticket_id = ticket_string(input, "ticketId");
    if conversation_id.is_empty() || ticket_id.is_empty() {
        return ticket_bad_request(
            "inbox.create_ticket_update_proposal requires non-empty 'conversationId' and 'ticketId'",
        );
    }
    let mut body = ticket_remap(
        input,
        &[
            ("conversationId", "conversation_id"),
            ("ticketId", "ticket_id"),
            ("confidence", "confidence"),
            ("reason", "reason"),
            ("evidenceMessageIds", "evidence_message_ids"),
            ("proposalGroupId", "proposal_group_id"),
            ("suggestedFields", "suggested_fields"),
        ],
    );
    body.insert("kind".to_owned(), json!("ticket.update"));
    let response = create_ai_text_proposal(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        Json(Value::Object(body)),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("inbox.create_ticket_update_proposal", user, status, payload)
}

pub(super) async fn dispatch_inbox_create_incident_proposal(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let ticket_id = ticket_string(input, "ticketId");
    let title = ticket_string(input, "title");
    if conversation_id.is_empty() || ticket_id.is_empty() || title.is_empty() {
        return ticket_bad_request(
            "inbox.create_incident_proposal requires non-empty 'conversationId', 'ticketId', and 'title'",
        );
    }
    let mut body = ticket_remap(
        input,
        &[
            ("conversationId", "conversation_id"),
            ("ticketId", "ticket_id"),
            ("title", "title"),
            ("severity", "severity"),
            ("customerImpact", "customer_impact"),
            ("confidence", "confidence"),
            ("reason", "reason"),
            ("evidenceMessageIds", "evidence_message_ids"),
            ("proposalGroupId", "proposal_group_id"),
        ],
    );
    body.insert("kind".to_owned(), json!("incident.create"));
    let response = create_ai_text_proposal(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        Json(Value::Object(body)),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("inbox.create_incident_proposal", user, status, payload)
}

pub(super) async fn dispatch_inbox_create_problem_proposal(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let title = ticket_string(input, "title");
    let summary = ticket_string(input, "summary");
    if conversation_id.is_empty() || title.is_empty() || summary.is_empty() {
        return ticket_bad_request(
            "inbox.create_problem_proposal requires non-empty 'conversationId', 'title', and 'summary'",
        );
    }
    let mut body = ticket_remap(
        input,
        &[
            ("conversationId", "conversation_id"),
            ("title", "title"),
            ("summary", "summary"),
            ("rootCause", "root_cause"),
            ("confidence", "confidence"),
            ("reason", "reason"),
            ("evidenceMessageIds", "evidence_message_ids"),
            ("proposalGroupId", "proposal_group_id"),
        ],
    );
    body.insert("kind".to_owned(), json!("problem.create"));
    let response = create_ai_text_proposal(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        Json(Value::Object(body)),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("inbox.create_problem_proposal", user, status, payload)
}

pub(super) async fn dispatch_notification_mark_read(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let id = ticket_string(input, "id");
    if id.is_empty() {
        return ticket_bad_request("notifications.mark_read requires a non-empty 'id'");
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let url = format!(
        "{}/notifications/{}/read",
        state.notification_core_url,
        urlencoding::encode(id)
    );
    let (status, Json(resp)) = proxy_notification_json(
        state,
        Method::POST,
        &url,
        None,
        &org_id,
        &actor_for_user(user),
    )
    .await;
    owner_json_response_to_envelope("notifications.mark_read", user, status, resp)
}

pub(super) async fn dispatch_notification_mark_all_read(
    state: &AppState,
    user: &AuthenticatedUser,
    _input: &Value,
) -> Response {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let url = format!(
        "{}/notifications/mark-all-read",
        state.notification_core_url
    );
    let (status, Json(resp)) = proxy_notification_json(
        state,
        Method::POST,
        &url,
        None,
        &org_id,
        &actor_for_user(user),
    )
    .await;
    owner_json_response_to_envelope("notifications.mark_all_read", user, status, resp)
}

pub(super) async fn dispatch_notification_delete(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let id = ticket_string(input, "id");
    if id.is_empty() {
        return ticket_bad_request("notifications.delete requires a non-empty 'id'");
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let url = format!(
        "{}/notifications/{}",
        state.notification_core_url,
        urlencoding::encode(id)
    );
    let (status, Json(resp)) = proxy_notification_json(
        state,
        Method::DELETE,
        &url,
        None,
        &org_id,
        &actor_for_user(user),
    )
    .await;
    owner_json_response_to_envelope("notifications.delete", user, status, resp)
}

pub(super) async fn dispatch_notification_update_preference(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let event_type = ticket_string(input, "eventType");
    let channel = ticket_string(input, "channel");
    let Some(enabled) = input.get("enabled").and_then(Value::as_bool) else {
        return ticket_bad_request("notifications.update_preference requires boolean 'enabled'");
    };
    if event_type.is_empty() || channel.is_empty() {
        return ticket_bad_request(
            "notifications.update_preference requires non-empty 'eventType' and 'channel'",
        );
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let url = format!(
        "{}/preferences/{}/{}",
        state.notification_core_url,
        urlencoding::encode(event_type),
        urlencoding::encode(channel)
    );
    let (status, Json(resp)) = proxy_notification_json(
        state,
        Method::PUT,
        &url,
        Some(json!({ "enabled": enabled })),
        &org_id,
        &actor_for_user(user),
    )
    .await;
    owner_json_response_to_envelope("notifications.update_preference", user, status, resp)
}

pub(super) async fn dispatch_navbar_save_theme(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let theme = ticket_string(input, "theme");
    if !matches!(theme, "light" | "dark" | "system") {
        return ticket_bad_request("navbar.save_theme requires theme of light, dark, or system");
    }
    let mut body = serde_json::Map::new();
    body.insert("theme".to_owned(), Value::String(theme.to_owned()));
    if let Some(color_scheme) = input.get("colorScheme") {
        body.insert("colorScheme".to_owned(), color_scheme.clone());
    }
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::PUT,
        &format!("{}/api/v1/navbar/theme", state.user_core_url),
        Some(Value::Object(body)),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("navbar.save_theme", user, status, resp)
}

pub(super) async fn dispatch_navbar_mark_notification_read(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let notification_id = ticket_string(input, "notificationId");
    if notification_id.is_empty() {
        return ticket_bad_request(
            "navbar.mark_notification_read requires a non-empty 'notificationId'",
        );
    }
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &format!("{}/api/v1/navbar/notifications", state.user_core_url),
        Some(json!({ "notificationId": notification_id })),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("navbar.mark_notification_read", user, status, resp)
}

pub(super) async fn dispatch_navbar_create_calendar_event(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let start = ticket_string(input, "start");
    let end = ticket_string(input, "end");
    let title = ticket_string(input, "title");
    let event_type = ticket_string(input, "type");
    if start.is_empty() || end.is_empty() || title.is_empty() || event_type.is_empty() {
        return ticket_bad_request(
            "navbar.create_calendar_event requires non-empty 'start', 'end', 'title', and 'type'",
        );
    }
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &format!("{}/api/v1/navbar/calendar", state.user_core_url),
        Some(json!({ "start": start, "end": end, "title": title, "type": event_type })),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("navbar.create_calendar_event", user, status, resp)
}

pub(super) async fn dispatch_navbar_create_calendar_note(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let date = ticket_string(input, "date");
    let text = ticket_string(input, "text");
    if date.is_empty() || text.is_empty() {
        return ticket_bad_request(
            "navbar.create_calendar_note requires non-empty 'date' and 'text'",
        );
    }
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &format!("{}/api/v1/navbar/calendar", state.user_core_url),
        Some(json!({ "date": date, "kind": "note", "text": text })),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("navbar.create_calendar_note", user, status, resp)
}

pub(super) async fn dispatch_navbar_submit_support_request(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let context = ticket_string(input, "context");
    let message = ticket_string(input, "message");
    let subject = ticket_string(input, "subject");
    if context.is_empty() || message.is_empty() || subject.is_empty() {
        return ticket_bad_request(
            "navbar.submit_support_request requires non-empty 'context', 'message', and 'subject'",
        );
    }
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &format!("{}/api/v1/support/requests", state.user_core_url),
        Some(json!({ "context": context, "message": message, "subject": subject })),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("navbar.submit_support_request", user, status, resp)
}

pub(super) async fn dispatch_inbox_workspace_set_pinned(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let Some(enabled) = input.get("enabled").and_then(Value::as_bool) else {
        return ticket_bad_request("inbox.set_conversation_pinned requires boolean 'enabled'");
    };
    if conversation_id.is_empty() {
        return ticket_bad_request(
            "inbox.set_conversation_pinned requires a non-empty 'conversationId'",
        );
    }
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &format!("{}/api/v1/inbox-workspace/pins", state.user_core_url),
        Some(json!({ "conversationId": conversation_id, "enabled": enabled })),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("inbox.set_conversation_pinned", user, status, resp)
}

pub(super) async fn dispatch_inbox_workspace_set_read(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let conversation_id = ticket_string(input, "conversationId");
    let Some(enabled) = input.get("enabled").and_then(Value::as_bool) else {
        return ticket_bad_request("inbox.set_conversation_read requires boolean 'enabled'");
    };
    if conversation_id.is_empty() {
        return ticket_bad_request(
            "inbox.set_conversation_read requires a non-empty 'conversationId'",
        );
    }
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &format!("{}/api/v1/inbox-workspace/read", state.user_core_url),
        Some(json!({ "conversationId": conversation_id, "enabled": enabled })),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("inbox.set_conversation_read", user, status, resp)
}

pub(super) async fn dispatch_ownership_share_document(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let doc_id = ticket_string(input, "docId");
    let subject_id = ticket_string(input, "subjectId");
    if doc_id.is_empty() || subject_id.is_empty() {
        return ticket_bad_request(
            "ownership.share_document requires non-empty 'docId' and 'subjectId'",
        );
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let actor = actor_for_user(user);
    let url = format!("{}/api/v1/internal/authz/grant", state.user_core_url);
    let payload = json!({
        "org_id": org_id,
        "resource_type": "document",
        "resource_id": doc_id,
        "subject_id": subject_id,
        "subject_type": "user",
    });
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(payload),
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("ownership.share_document", user, status, resp)
}

pub(super) async fn dispatch_ownership_revoke_document_share(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let doc_id = ticket_string(input, "docId");
    let subject_id = ticket_string(input, "subjectId");
    if doc_id.is_empty() || subject_id.is_empty() {
        return ticket_bad_request(
            "ownership.revoke_document_share requires non-empty 'docId' and 'subjectId'",
        );
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let actor = actor_for_user(user);
    let url = format!(
        "{}/api/v1/internal/authz/grant?org_id={}&resource_type=document&resource_id={}&subject_id={}&subject_type=user",
        state.user_core_url,
        urlencoding::encode(&org_id),
        urlencoding::encode(doc_id),
        urlencoding::encode(subject_id),
    );
    let (status, Json(resp)) = proxy_json(
        state,
        Method::DELETE,
        &url,
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("ownership.revoke_document_share", user, status, resp)
}

// Reuses memory.rs's real delete_memory rather than reconstructing its two
// token resolutions (model + session) and its ZDR no-op-not-error posture by
// hand: per that module's own doc comment, org/user scope is re-derived and
// enforced independently by session-core from the verified bearer, "so a
// client cannot widen scope by tampering with anything sent here" — a
// property only worth stating because a hand-rolled copy could break it.
pub(super) async fn dispatch_memory_delete(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let memory_id = ticket_string(input, "memoryId");
    if memory_id.is_empty() {
        return ticket_bad_request("memory.delete requires a non-empty 'memoryId'");
    }
    // model_token/session_token both resolve from the request's own cookie
    // header (see chat::shared) — an empty HeaderMap here would silently mint
    // no token and break the delete for every caller, not just reject a bad one.
    let response = delete_memory(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(memory_id.to_owned()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("memory.delete", user, status, payload)
}

pub(super) async fn dispatch_settings_update_me(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    if input.as_object().is_none_or(|obj| obj.is_empty()) {
        return ticket_bad_request("settings.update_me requires at least one field to update");
    }
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::PATCH,
        &format!("{}/api/v1/users/me", state.user_core_url),
        Some(input.clone()),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("settings.update_me", user, status, resp)
}

pub(super) async fn dispatch_settings_update_preferences(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    if input.as_object().is_none_or(|obj| obj.is_empty()) {
        return ticket_bad_request(
            "settings.update_preferences requires at least one field to update",
        );
    }
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::PATCH,
        &format!("{}/api/v1/preferences", state.user_core_url),
        Some(input.clone()),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("settings.update_preferences", user, status, resp)
}

pub(super) async fn dispatch_settings_update_setting(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let key = ticket_string(input, "key");
    if key.is_empty() {
        return ticket_bad_request("settings.update_setting requires a non-empty 'key'");
    }
    let Some(value) = input.get("value") else {
        return ticket_bad_request("settings.update_setting requires a 'value'");
    };
    let actor = actor_for_user(user);
    let url = format!(
        "{}/api/v1/settings/{}",
        state.user_core_url,
        urlencoding::encode(key)
    );
    let (status, Json(resp)) = proxy_json(
        state,
        Method::PUT,
        &url,
        Some(json!({ "key": key, "value": value })),
        None,
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("settings.update_setting", user, status, resp)
}

// Reuses the real create_api_key/delete_api_key rather than re-deriving them:
// create_api_key's response carries a raw, one-time credential secret
// (auth-core never returns it again), and both authenticate to auth-core via
// the request's own cookie header (post_auth_core), the same class of bug
// dispatch_memory_delete's empty-HeaderMap draft had before it was caught —
// so both take `headers` here rather than a synthesized one.
pub(super) async fn dispatch_settings_create_api_key(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let name = ticket_string(input, "name");
    if name.is_empty() {
        return ticket_bad_request("settings.create_api_key requires a non-empty 'name'");
    }
    let body = ticket_remap(input, &[("name", "name"), ("expiresAt", "expiresAt")]);
    let response = create_api_key(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(Value::Object(body)),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("settings.create_api_key", user, status, payload)
}

pub(super) async fn dispatch_settings_delete_api_key(
    state: &AppState,
    _user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let id = ticket_string(input, "id");
    if id.is_empty() {
        return ticket_bad_request("settings.delete_api_key requires a non-empty 'id'");
    }
    let response = delete_api_key(
        StateExtractor(state.clone()),
        headers.clone(),
        PathExtractor(id.to_owned()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("settings.delete_api_key", _user, status, payload)
}

// register_server/connect_server (registerMcpServer/connectMcpServer) are
// deliberately NOT dispatched here: both gate on org_scope_forbidden
// (admin-only for an org-wide server) on top of the same delegated
// model-gateway capability-token exchange cron-client.ts's handlers use —
// the same depth this file already declined to rush for cron. delete_server
// and share_server carry no such admin gate, only the capability-token
// exchange, so those two are reused directly below.
pub(super) async fn dispatch_mcp_delete_server(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let server_id = ticket_string(input, "serverId");
    if server_id.is_empty() {
        return ticket_bad_request("mcp.delete_server requires a non-empty 'serverId'");
    }
    let response = delete_mcp_server(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(server_id.to_owned()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("mcp.delete_server", user, status, payload)
}

pub(super) async fn dispatch_mcp_share_server(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let server_id = ticket_string(input, "serverId");
    let Some(user_ids) = input.get("userIds").and_then(Value::as_array) else {
        return ticket_bad_request("mcp.share_server requires an array 'userIds'");
    };
    if server_id.is_empty() {
        return ticket_bad_request("mcp.share_server requires a non-empty 'serverId'");
    }
    let response = share_mcp_server(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(server_id.to_owned()),
        Json(json!({ "user_ids": user_ids })),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("mcp.share_server", user, status, payload)
}

// Reuses the real check_now rather than re-deriving it: on a normal call (no
// pre-computed fingerprint) it performs an actual scrape through quarry-edge
// to compute one, and that fetch-then-fingerprint-then-compare logic is
// exactly the kind of thing a hand-written copy could get subtly wrong.
pub(super) async fn dispatch_monitoring_check_now(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let url = ticket_string(input, "url");
    if url.is_empty() {
        return ticket_bad_request("monitoring.check_now requires a non-empty 'url'");
    }
    let response = check_now(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(CheckRequest {
            url: Some(url.to_owned()),
            fingerprint: None,
        }),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("monitoring.check_now", user, status, payload)
}

// Reuses the real erase handler rather than reconstructing its URL and
// confirm-check by hand — this is the highest-stakes action in the registry
// (irreversible account erasure), and a hand-rolled copy is exactly where a
// wrong path segment or a loosened confirm check would do the most damage.
// `confirm: true` is always sent, never read from `input`, mirroring
// eraseMyAccount's own posture: the client-side typed-confirm + step-up
// re-auth gate is the actual safety control here, not this body field.
pub(super) async fn dispatch_privacy_erase_my_account(
    state: &AppState,
    user: &AuthenticatedUser,
    _input: &Value,
) -> Response {
    let response = erase_my_account(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        Some(Json(json!({ "confirm": true }))),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("privacy.erase_my_account", user, status, payload)
}

pub(super) async fn dispatch_leads_create_list(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let name = ticket_string(input, "name");
    if name.is_empty() {
        return ticket_bad_request("leads.create_list requires a non-empty 'name'");
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let companies = input.get("companies").cloned().unwrap_or(json!([]));
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &format!("{}/api/v1/leads/lists", state.leads_core_url),
        Some(json!({ "name": name, "companies": companies })),
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("leads.create_list", user, status, resp)
}

pub(super) async fn dispatch_leads_delete_list(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let id = ticket_string(input, "id");
    if id.is_empty() {
        return ticket_bad_request("leads.delete_list requires a non-empty 'id'");
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_active_org();
    }
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::DELETE,
        &format!("{}/api/v1/leads/lists/{}", state.leads_core_url, id),
        None,
        Some(&org_id),
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("leads.delete_list", user, status, resp)
}

pub(super) async fn dispatch_finetune_create_job(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    if input.as_object().is_none_or(|obj| obj.is_empty()) {
        return ticket_bad_request("finetune.create_job requires a job configuration body");
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &format!("{}/v1/finetune/jobs", state.model_gateway_url),
        Some(input.clone()),
        Some(org_id.as_str()),
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("finetune.create_job", user, status, resp)
}

pub(super) async fn dispatch_finetune_cancel_job(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let job_id = ticket_string(input, "jobId");
    if job_id.is_empty() {
        return ticket_bad_request("finetune.cancel_job requires a non-empty 'jobId'");
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::DELETE,
        &format!(
            "{}/v1/finetune/jobs/{}",
            state.model_gateway_url,
            urlencoding::encode(job_id)
        ),
        None,
        Some(org_id.as_str()),
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("finetune.cancel_job", user, status, resp)
}

pub(super) async fn dispatch_finetune_deploy_job(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let job_id = ticket_string(input, "jobId");
    if job_id.is_empty() {
        return ticket_bad_request("finetune.deploy_job requires a non-empty 'jobId'");
    }
    let tier = input
        .get("tier")
        .and_then(Value::as_str)
        .unwrap_or("production");
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let actor = actor_for_user(user);
    let (status, Json(resp)) = proxy_json(
        state,
        Method::POST,
        &format!(
            "{}/v1/finetune/jobs/{}/deploy",
            state.model_gateway_url,
            urlencoding::encode(job_id)
        ),
        Some(json!({ "tier": tier })),
        Some(org_id.as_str()),
        Some(&actor),
        None,
    )
    .await;
    owner_json_response_to_envelope("finetune.deploy_job", user, status, resp)
}

// The three studio dispatchers below reuse the real handlers rather than
// reimplementing them: studio.rs owns its own in-memory, RAM-only project
// store (studio_store) plus block-geometry and duplicate-id validation
// (normalize_blocks / normalize_title / normalize_selected_block_id) — real
// invariants a parallel reimplementation could get subtly wrong, exactly the
// class of mistake the ephemeral-persistence contract test already guards.
fn parse_studio_blocks(value: Option<&Value>) -> Option<Vec<StudioBlock>> {
    value.and_then(|v| serde_json::from_value::<Vec<StudioBlock>>(v.clone()).ok())
}

pub(super) async fn dispatch_studio_create_project(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let body = CreateProjectBody {
        title: input
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocks: parse_studio_blocks(input.get("blocks")),
        selected_block_id: input
            .get("selectedBlockId")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    let response = create_studio_project(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("studio.create_project", user, status, payload)
}

pub(super) async fn dispatch_studio_save_project(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let project_id = ticket_string(input, "projectId");
    if project_id.is_empty() {
        return ticket_bad_request("studio.save_project requires a non-empty 'projectId'");
    }
    let body = UpdateProjectBody {
        title: input
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocks: parse_studio_blocks(input.get("blocks")),
        selected_block_id: input
            .get("selectedBlockId")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    let response = update_studio_project(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(project_id.to_owned()),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("studio.save_project", user, status, payload)
}

pub(super) async fn dispatch_studio_export_social_draft(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let project_id = ticket_string(input, "projectId");
    if project_id.is_empty() {
        return ticket_bad_request("studio.export_social_draft requires a non-empty 'projectId'");
    }
    let platforms = input
        .get("platforms")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        });
    let body = ExportSocialDraftBody {
        title: input
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned),
        body: input.get("body").and_then(Value::as_str).map(str::to_owned),
        platforms,
        scheduled_at: input
            .get("scheduledAt")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    let response = export_studio_social_draft(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(project_id.to_owned()),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("studio.export_social_draft", user, status, payload)
}

// The five ingestions dispatchers below all reuse the real handlers directly.
// create_source alone has confirmed, tested logic worth not re-deriving
// (SSRF guards on the target URL, cross-tenant org stripping — see
// domains::ingestions::sources::tests); create_run/create_schedule/
// schedule_actions take raw JSON already, so reuse costs nothing extra here
// and keeps every one of these five on the same real validation path.
pub(super) async fn dispatch_ingestions_create_run(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let kind = ticket_string(input, "kind");
    if kind.is_empty() {
        return ticket_bad_request("ingestions.create_run requires a non-empty 'kind'");
    }
    let response = create_ingestion_run(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(input.clone()),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("ingestions.create_run", user, status, payload)
}

pub(super) async fn dispatch_ingestions_create_schedule(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let name = ticket_string(input, "name");
    let kind = ticket_string(input, "kind");
    let target_url = ticket_string(input, "targetUrl");
    let has_timing = input
        .get("cron")
        .and_then(Value::as_str)
        .is_some_and(|v| !v.trim().is_empty())
        || input
            .get("scheduleAt")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.trim().is_empty());
    if name.is_empty() || kind.is_empty() || target_url.is_empty() || !has_timing {
        return ticket_bad_request(
            "ingestions.create_schedule requires 'name', 'kind', 'targetUrl', and either 'cron' or 'scheduleAt'",
        );
    }
    let response = create_ingestion_schedule(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(input.clone()),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("ingestions.create_schedule", user, status, payload)
}

pub(super) async fn dispatch_ingestions_run_schedule_action(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let action = ticket_string(input, "action");
    let schedule_id = ticket_string(input, "scheduleId");
    if !matches!(
        action,
        "pause_schedule"
            | "unpause_schedule"
            | "trigger_schedule"
            | "backfill_schedule"
            | "delete_schedule"
    ) || schedule_id.is_empty()
    {
        return ticket_bad_request(
            "ingestions.run_schedule_action requires a non-empty 'scheduleId' and a known 'action'",
        );
    }
    let response = run_ingestion_schedule_action(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(input.clone()),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("ingestions.run_schedule_action", user, status, payload)
}

pub(super) async fn dispatch_ingestions_create_source(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let name = ticket_string(input, "name");
    let url = ticket_string(input, "url");
    let kind = ticket_string(input, "kind");
    if name.is_empty() || url.is_empty() || kind.is_empty() {
        return ticket_bad_request(
            "ingestions.create_source requires non-empty 'name', 'url', and 'kind'",
        );
    }
    let response = create_ingestion_source(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(input.clone()),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("ingestions.create_source", user, status, payload)
}

pub(super) async fn dispatch_ingestions_delete_source(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let id = ticket_string(input, "id");
    if id.is_empty() {
        return ticket_bad_request("ingestions.delete_source requires a non-empty 'id'");
    }
    let response = delete_ingestion_source(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(id.to_owned()),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("ingestions.delete_source", user, status, payload)
}

pub(super) async fn dispatch_integrations_start_connect_session(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let provider = ticket_string(input, "provider");
    if provider.is_empty() {
        return ticket_bad_request(
            "integrations.start_connect_session requires a non-empty 'provider'",
        );
    }
    let body = input.get("body").cloned().unwrap_or_else(|| json!({}));
    let response = start_integration_connect_session(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(provider.to_owned()),
        Json(body),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("integrations.start_connect_session", user, status, payload)
}

pub(super) async fn dispatch_integrations_disconnect(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let id = ticket_string(input, "id");
    if id.is_empty() {
        return ticket_bad_request("integrations.disconnect requires a non-empty 'id'");
    }
    let response = disconnect_integration_connection(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(id.to_owned()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("integrations.disconnect", user, status, payload)
}

pub(super) async fn dispatch_integrations_start_chatgpt_subscription(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    _input: &Value,
) -> Response {
    let response = start_openai_codex_subscription(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(json!({})),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope(
        "integrations.start_chatgpt_subscription",
        user,
        status,
        payload,
    )
}

pub(super) async fn dispatch_integrations_disconnect_chatgpt_subscription(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let connection_id = ticket_string(input, "connectionId");
    if connection_id.is_empty() {
        return ticket_bad_request(
            "integrations.disconnect_chatgpt_subscription requires a non-empty 'connectionId'",
        );
    }
    let response = disconnect_openai_codex_subscription(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(connection_id.to_owned()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope(
        "integrations.disconnect_chatgpt_subscription",
        user,
        status,
        payload,
    )
}

pub(super) async fn dispatch_integrations_trigger_sync(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let id = ticket_string(input, "id");
    if id.is_empty() {
        return ticket_bad_request("integrations.trigger_sync requires a non-empty 'id'");
    }
    let response = trigger_integration_sync(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(id.to_owned()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("integrations.trigger_sync", user, status, payload)
}

pub(super) async fn dispatch_integrations_trigger_inbox_sync(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let id = ticket_string(input, "id");
    let channel = ticket_string(input, "channel");
    if id.is_empty() || !matches!(channel, "email" | "teams" | "slack") {
        return ticket_bad_request(
            "integrations.trigger_inbox_sync requires a non-empty 'id' and channel of email, teams, or slack",
        );
    }
    let response = trigger_integration_inbox_sync(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(id.to_owned()),
        Json(json!({ "channel": channel })),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("integrations.trigger_inbox_sync", user, status, payload)
}

pub(super) async fn dispatch_integrations_extend_inbox_history(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let id = ticket_string(input, "id");
    if id.is_empty() {
        return ticket_bad_request("integrations.extend_inbox_history requires a non-empty 'id'");
    }
    let response = extend_integration_inbox_history(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(id.to_owned()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("integrations.extend_inbox_history", user, status, payload)
}

// The twelve org-admin dispatchers below all reuse the real handlers, never a
// hand-rolled proxy: every one of them gates on require_org_admin (or, for
// the self-service deletion checkpoints, require_active_org) and getting that
// gate wrong in a copy would be a real authorization bypass, not a cosmetic
// bug. soft_delete/restore/mark_exported/acknowledge/set_quota/
// update_org_instructions/update_org_settings return the concrete
// (StatusCode, Json<Value>) tuple GatewayJsonResponse aliases, so they are
// destructured directly; invite_member/remove_member/update_member_role/
// switch_active_org return Response directly, so they are passed straight
// through with no .into_response() needed either way — only a function
// returning the opaque impl IntoResponse (like mcp::delete_server or
// memory::delete_memory above) needs that conversion.
pub(super) async fn dispatch_org_soft_delete(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let org_name = ticket_string(input, "orgName");
    let Some(true) = input.get("confirm").and_then(Value::as_bool) else {
        return ticket_bad_request("org.soft_delete requires boolean 'confirm': true");
    };
    if org_id.is_empty() || org_name.is_empty() {
        return ticket_bad_request(
            "org.soft_delete requires non-empty 'orgId' and the exact 'orgName'",
        );
    }
    let (status, Json(resp)) = soft_delete_org(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(org_id.to_owned()),
        Json(json!({ "confirm": true, "org_name": org_name })),
    )
    .await;
    owner_json_response_to_envelope("org.soft_delete", user, status, resp)
}

pub(super) async fn dispatch_org_restore(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    if org_id.is_empty() {
        return ticket_bad_request("org.restore requires a non-empty 'orgId'");
    }
    let (status, Json(resp)) = restore_org(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(org_id.to_owned()),
    )
    .await;
    owner_json_response_to_envelope("org.restore", user, status, resp)
}

pub(super) async fn dispatch_org_mark_exported(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    if org_id.is_empty() {
        return ticket_bad_request("org.mark_exported requires a non-empty 'orgId'");
    }
    let (status, Json(resp)) = mark_org_exported(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(org_id.to_owned()),
    )
    .await;
    owner_json_response_to_envelope("org.mark_exported", user, status, resp)
}

pub(super) async fn dispatch_org_acknowledge_deletion(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    if org_id.is_empty() {
        return ticket_bad_request("org.acknowledge_deletion requires a non-empty 'orgId'");
    }
    let (status, Json(resp)) = acknowledge_org_deletion(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(org_id.to_owned()),
    )
    .await;
    owner_json_response_to_envelope("org.acknowledge_deletion", user, status, resp)
}

pub(super) async fn dispatch_org_set_quota(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let key = ticket_string(input, "key");
    let Some(limit) = input.get("limit").and_then(Value::as_i64) else {
        return ticket_bad_request("org.set_quota requires integer 'limit'");
    };
    if org_id.is_empty() || key.is_empty() {
        return ticket_bad_request("org.set_quota requires non-empty 'orgId' and 'key'");
    }
    let mut body = serde_json::Map::new();
    body.insert("limit".to_owned(), json!(limit));
    if let Some(reset_period) = input.get("reset_period").and_then(Value::as_str) {
        body.insert(
            "reset_period".to_owned(),
            Value::String(reset_period.to_owned()),
        );
    }
    let (status, Json(resp)) = set_org_quota(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor((org_id.to_owned(), key.to_owned())),
        Json(Value::Object(body)),
    )
    .await;
    owner_json_response_to_envelope("org.set_quota", user, status, resp)
}

pub(super) async fn dispatch_org_update_instructions(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    if org_id.is_empty() {
        return ticket_bad_request("org.update_instructions requires a non-empty 'orgId'");
    }
    let instructions = input
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let (status, Json(resp)) = update_org_instructions(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(org_id.to_owned()),
        Json(UpdateOrgInstructionsRequest { instructions }),
    )
    .await;
    owner_json_response_to_envelope("org.update_instructions", user, status, resp)
}

pub(super) async fn dispatch_org_update_zdr(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let Some(zero_data_retention) = input.get("zeroDataRetention").and_then(Value::as_bool) else {
        return ticket_bad_request("org.update_zdr requires boolean 'zeroDataRetention'");
    };
    if org_id.is_empty() {
        return ticket_bad_request("org.update_zdr requires a non-empty 'orgId'");
    }
    let (status, Json(resp)) = update_org_settings(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(org_id.to_owned()),
        Json(json!({ "zeroDataRetention": zero_data_retention })),
    )
    .await;
    owner_json_response_to_envelope("org.update_zdr", user, status, resp)
}

pub(super) async fn dispatch_org_update_support_ai_mode(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let support_ai_mode = ticket_string(input, "supportAiMode");
    if org_id.is_empty() || !matches!(support_ai_mode, "off" | "assist" | "review") {
        return ticket_bad_request(
            "org.update_support_ai_mode requires 'orgId' and supportAiMode of off, assist, or review",
        );
    }
    let (status, Json(resp)) = update_org_settings(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(org_id.to_owned()),
        Json(json!({ "supportAiMode": support_ai_mode })),
    )
    .await;
    owner_json_response_to_envelope("org.update_support_ai_mode", user, status, resp)
}

pub(super) async fn dispatch_membership_invite_member(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let email = ticket_string(input, "email");
    let role = ticket_string(input, "role");
    if org_id.is_empty() || email.is_empty() || !matches!(role, "member" | "admin") {
        return ticket_bad_request(
            "membership.invite_member requires 'orgId', 'email', and role of member or admin",
        );
    }
    let response = invite_org_member(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(org_id.to_owned()),
        Json(json!({ "email": email, "role": role })),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("membership.invite_member", user, status, payload)
}

pub(super) async fn dispatch_membership_remove_member(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let user_id = ticket_string(input, "userId");
    if org_id.is_empty() || user_id.is_empty() {
        return ticket_bad_request(
            "membership.remove_member requires non-empty 'orgId' and 'userId'",
        );
    }
    let response = remove_org_member(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor((org_id.to_owned(), user_id.to_owned())),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("membership.remove_member", user, status, payload)
}

pub(super) async fn dispatch_membership_update_member_role(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let org_id = ticket_string(input, "orgId");
    let user_id = ticket_string(input, "userId");
    let role = ticket_string(input, "role");
    if org_id.is_empty() || user_id.is_empty() || !matches!(role, "member" | "admin") {
        return ticket_bad_request(
            "membership.update_member_role requires 'orgId', 'userId', and role of member or admin",
        );
    }
    let response = update_org_member_role(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor((org_id.to_owned(), user_id.to_owned())),
        Json(json!({ "role": role })),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("membership.update_member_role", user, status, payload)
}

pub(super) async fn dispatch_organization_switch_active(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let organization_id = ticket_string(input, "organizationId");
    if organization_id.is_empty() {
        return ticket_bad_request(
            "organization.switch_active requires a non-empty 'organizationId'",
        );
    }
    let response = switch_active_org(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Some(Json(json!({ "organizationId": organization_id }))),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("organization.switch_active", user, status, payload)
}

// approve_plan is the highest-stakes reuse in this file: it grants a running
// model invocation an autonomy rung up to danger_full_access. The owner
// checks run ownership server-side (see agents_runs.rs's own comment on
// approve_plan); this dispatcher adds no gate of its own and must not, since
// duplicating "checks that they own the run" here would be exactly the kind
// of second copy that drifts from the real check.
pub(super) async fn dispatch_chat_approve_plan(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let run_id = ticket_string(input, "runId");
    let granted_rung = ticket_string(input, "grantedRung");
    let justification = ticket_string(input, "justification");
    if run_id.is_empty()
        || !matches!(
            granted_rung,
            "read_only" | "workspace_write" | "danger_full_access"
        )
        || justification.is_empty()
    {
        return ticket_bad_request(
            "chat.approve_plan requires 'runId', a known 'grantedRung', and a non-empty 'justification'",
        );
    }
    let response = approve_plan(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(run_id.to_owned()),
        Json(json!({ "granted_rung": granted_rung, "justification": justification })),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("chat.approve_plan", user, status, payload)
}

pub(super) async fn dispatch_chat_cancel_invocation(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let request_id = ticket_string(input, "requestId");
    if request_id.is_empty() {
        return ticket_bad_request("chat.cancel_invocation requires a non-empty 'requestId'");
    }
    let response = cancel_chat_invocation(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(request_id.to_owned()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("chat.cancel_invocation", user, status, payload)
}

pub(super) async fn dispatch_chat_queue_invocation_input(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let request_id = ticket_string(input, "requestId");
    let content = ticket_string(input, "content");
    if request_id.is_empty() || content.is_empty() {
        return ticket_bad_request(
            "chat.queue_invocation_input requires non-empty 'requestId' and 'content'",
        );
    }
    let mut body = serde_json::Map::new();
    body.insert("content".to_owned(), Value::String(content.to_owned()));
    if let Some(thread_id) = input.get("threadId") {
        body.insert("threadId".to_owned(), thread_id.clone());
    }
    if let Some(space_ref) = input.get("spaceRef") {
        body.insert("spaceRef".to_owned(), space_ref.clone());
    }
    let response = queue_chat_invocation_input(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(request_id.to_owned()),
        Json(Value::Object(body)),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("chat.queue_invocation_input", user, status, payload)
}

pub(super) async fn dispatch_chat_clear_threads(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    _input: &Value,
) -> Response {
    let response = clear_chat_threads(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("chat.clear_threads", user, status, payload)
}

pub(super) async fn dispatch_chat_delete_thread(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let thread_id = ticket_string(input, "threadId");
    if thread_id.is_empty() {
        return ticket_bad_request("chat.delete_thread requires a non-empty 'threadId'");
    }
    let response = delete_chat_thread(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(thread_id.to_owned()),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("chat.delete_thread", user, status, payload)
}

pub(super) async fn dispatch_chat_save_thread_snapshot(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let thread_id = ticket_string(input, "threadId");
    if thread_id.is_empty() {
        return ticket_bad_request("chat.save_thread_snapshot requires a non-empty 'threadId'");
    }
    let body = SaveThreadRequest {
        title: input
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned),
        pinned: input.get("pinned").and_then(Value::as_bool),
        preview: input
            .get("preview")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    let response = save_chat_thread(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(thread_id.to_owned()),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("chat.save_thread_snapshot", user, status, payload)
}

pub(super) async fn dispatch_chat_submit_feedback(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let request_id = ticket_string(input, "requestId");
    let rating = ticket_string(input, "rating");
    if request_id.is_empty() || !matches!(rating, "positive" | "negative") {
        return ticket_bad_request(
            "chat.submit_feedback requires 'requestId' and rating of positive or negative",
        );
    }
    let mut body = serde_json::Map::new();
    body.insert("requestId".to_owned(), Value::String(request_id.to_owned()));
    body.insert("rating".to_owned(), Value::String(rating.to_owned()));
    if let Some(note) = input.get("note").and_then(Value::as_str) {
        body.insert("note".to_owned(), Value::String(note.to_owned()));
    }
    if let Some(run_id) = input.get("runId").and_then(Value::as_str) {
        body.insert("runId".to_owned(), Value::String(run_id.to_owned()));
    }
    let response = submit_chat_feedback(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(Value::Object(body)),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("chat.submit_feedback", user, status, payload)
}

pub(super) async fn dispatch_audio_transcribe(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let audio_base64 = ticket_string(input, "audioBase64");
    let format = ticket_string(input, "format");
    let language = ticket_string(input, "language");
    if audio_base64.is_empty() || !matches!(format, "webm" | "ogg" | "wav" | "mp3") {
        return ticket_bad_request(
            "audio.transcribe requires 'audioBase64' and format of webm, ogg, wav, or mp3",
        );
    }
    let response = proxy_speech(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(json!({
            "audio_base64": audio_base64,
            "format": format,
            "language": language,
            "operation": "transcribe",
            "provider": "azure",
        })),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("audio.transcribe", user, status, payload)
}

pub(super) async fn dispatch_audio_dictate(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let audio_base64 = ticket_string(input, "audioBase64");
    let format = ticket_string(input, "format");
    let language = ticket_string(input, "language");
    if audio_base64.is_empty() || !matches!(format, "webm" | "ogg" | "wav" | "mp3") {
        return ticket_bad_request(
            "audio.dictate requires 'audioBase64' and format of webm, ogg, wav, or mp3",
        );
    }
    let mut body = serde_json::Map::new();
    body.insert(
        "audio_base64".to_owned(),
        Value::String(audio_base64.to_owned()),
    );
    body.insert("format".to_owned(), Value::String(format.to_owned()));
    body.insert("language".to_owned(), Value::String(language.to_owned()));
    if let Some(context) = input.get("context").and_then(Value::as_str) {
        body.insert("context".to_owned(), Value::String(context.to_owned()));
    }
    let response = proxy_dictate(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(Value::Object(body)),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("audio.dictate", user, status, payload)
}

pub(super) async fn dispatch_orchestration_decide_approval(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let approval_id = ticket_string(input, "approvalId");
    let decision = ticket_string(input, "decision");
    if approval_id.is_empty() || !matches!(decision, "approve" | "reject") {
        return ticket_bad_request(
            "orchestration.decide_approval requires 'approvalId' and decision of approve or reject",
        );
    }
    let reason = input
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let response = decide_orchestration_approval(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(approval_id.to_owned()),
        Json(json!({ "decision": decision, "reason": reason })),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("orchestration.decide_approval", user, status, payload)
}

pub(super) async fn dispatch_orchestration_resume_run(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let run_id = ticket_string(input, "runId");
    if run_id.is_empty() {
        return ticket_bad_request("orchestration.resume_run requires a non-empty 'runId'");
    }
    let response = resume_orchestration_run(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(run_id.to_owned()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("orchestration.resume_run", user, status, payload)
}

pub(super) async fn dispatch_orchestration_cancel_run(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let run_id = ticket_string(input, "runId");
    if run_id.is_empty() {
        return ticket_bad_request("orchestration.cancel_run requires a non-empty 'runId'");
    }
    let response = cancel_orchestration_run(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(run_id.to_owned()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("orchestration.cancel_run", user, status, payload)
}

// Starts a durable, server-side, multi-step browser-agent run. requireApproval
// is deliberately NOT accepted here: StartAiRunBody documents it as accepted
// for API back-compat only and never read (it would arm a confirmed-broken
// legacy gate in execution-core), so offering it as a working input would
// misrepresent what this action actually does. maxCostUsd IS real and is
// forwarded (see build_ai_run_request in browser.rs).
pub(super) async fn dispatch_browser_run_start(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let session_id = ticket_string(input, "sessionId");
    let goal = ticket_string(input, "goal");
    if session_id.is_empty() || goal.is_empty() {
        return ticket_bad_request("browser_run.start requires non-empty 'sessionId' and 'goal'");
    }
    let allowed_domains = input
        .get("allowedDomains")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        });
    let body = StartAiRunBody {
        goal: goal.to_owned(),
        allowed_domains,
        max_steps: input
            .get("maxSteps")
            .and_then(Value::as_i64)
            .map(|v| v as i32),
        max_runtime_s: input
            .get("maxRuntimeSeconds")
            .and_then(Value::as_i64)
            .map(|v| v as i32),
        stop_criteria: input
            .get("stopCriteria")
            .and_then(Value::as_str)
            .map(str::to_owned),
        require_approval: None,
        max_cost_usd: input.get("maxCostUsd").and_then(Value::as_f64),
    };
    let response = start_browser_ai_run(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(session_id.to_owned()),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_run.start", user, status, payload)
}

pub(super) async fn dispatch_browser_run_control(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let run_id = ticket_string(input, "runId");
    let action = ticket_string(input, "action");
    if run_id.is_empty() || !matches!(action, "pause" | "resume" | "stop") {
        return ticket_bad_request(
            "browser_run.control requires 'runId' and action of pause, resume, or stop",
        );
    }
    let response = control_ai_run(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(run_id.to_owned()),
        Json(json!({ "action": action })),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_run.control", user, status, payload)
}

fn parse_browser_profile_scope(value: &str) -> Option<BrowserProfileScope> {
    serde_json::from_value(Value::String(value.to_owned())).ok()
}

pub(super) async fn dispatch_browser_create_session(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let url = ticket_string(input, "url");
    if url.is_empty() {
        return ticket_bad_request("browser_session.create requires a non-empty 'url'");
    }
    let scope = match input.get("scope").and_then(Value::as_str) {
        Some(raw) => match parse_browser_profile_scope(raw) {
            Some(scope) => Some(scope),
            None => {
                return ticket_bad_request(
                    "browser_session.create 'scope' must be one of ephemeral, user_private, org_shared, run_scoped",
                )
            }
        },
        None => None,
    };
    let viewport = input.get("viewport").and_then(|v| {
        let width = v.get("width").and_then(Value::as_u64)? as u32;
        let height = v.get("height").and_then(Value::as_u64)? as u32;
        Some(BrowserViewport { width, height })
    });
    let body = CreateSessionBody {
        url: url.to_owned(),
        profile_id: input
            .get("profileId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        persistent_profile: input
            .get("persistentProfile")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        viewport,
        zdr: input.get("zdr").and_then(Value::as_bool).unwrap_or(false),
        scope,
    };
    let response = create_browser_session(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_session.create", user, status, payload)
}

pub(super) async fn dispatch_browser_close_session(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let session_id = ticket_string(input, "sessionId");
    if session_id.is_empty() {
        return ticket_bad_request("browser_session.close requires a non-empty 'sessionId'");
    }
    let response = close_browser_session(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(session_id.to_owned()),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_session.close", user, status, payload)
}

pub(super) async fn dispatch_browser_create_tab(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let session_id = ticket_string(input, "sessionId");
    if session_id.is_empty() {
        return ticket_bad_request("browser_tab.create requires a non-empty 'sessionId'");
    }
    let body = NewTabBody {
        url: input.get("url").and_then(Value::as_str).map(str::to_owned),
    };
    let response = new_browser_tab(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(session_id.to_owned()),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_tab.create", user, status, payload)
}

pub(super) async fn dispatch_browser_select_tab(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let session_id = ticket_string(input, "sessionId");
    let tab_id = ticket_string(input, "tabId");
    if session_id.is_empty() || tab_id.is_empty() {
        return ticket_bad_request("browser_tab.select requires 'sessionId' and 'tabId'");
    }
    let response = select_browser_tab(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor((session_id.to_owned(), tab_id.to_owned())),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_tab.select", user, status, payload)
}

pub(super) async fn dispatch_browser_close_tab(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let session_id = ticket_string(input, "sessionId");
    let tab_id = ticket_string(input, "tabId");
    if session_id.is_empty() || tab_id.is_empty() {
        return ticket_bad_request("browser_tab.close requires 'sessionId' and 'tabId'");
    }
    let response = close_browser_tab(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor((session_id.to_owned(), tab_id.to_owned())),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_tab.close", user, status, payload)
}

// The owner-side gate this reuses is the actor==Agent && mode==HumanTakeover
// -> 409 CONFLICT check in `run_action`, plus `sanitize_action`'s validation
// (blocks private navigation, denies raw script eval, normalizes navigation
// URLs, rejects unbounded coordinate takeover). The action payload is passed
// through as raw JSON rather than modeled field-by-field here: `sanitize_action`
// is the real enforcement point, and re-deriving its schema here would only
// create a second, driftable copy of that validation.
pub(super) async fn dispatch_browser_run_action(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let session_id = ticket_string(input, "sessionId");
    let action = input.get("action").cloned();
    if session_id.is_empty() || action.is_none() {
        return ticket_bad_request("browser_action.run requires 'sessionId' and 'action'");
    }
    let actor_raw = input
        .get("actor")
        .and_then(Value::as_str)
        .unwrap_or("human");
    let actor = match actor_raw {
        "agent" => BrowserActionActor::Agent,
        "human" => BrowserActionActor::Human,
        _ => return ticket_bad_request("browser_action.run 'actor' must be 'human' or 'agent'"),
    };
    let body = BrowserActionBody {
        action: action.unwrap_or(Value::Null),
        actor,
    };
    let response = run_browser_action(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(session_id.to_owned()),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_action.run", user, status, payload)
}

// Control authority lives in Quarry (see set_control_mode's own comment in
// browser.rs) — this dispatcher is a thin, gate-preserving proxy, not a
// second place that decides hand-off authority.
pub(super) async fn dispatch_browser_set_control_mode(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let session_id = ticket_string(input, "sessionId");
    let mode_raw = ticket_string(input, "mode");
    if session_id.is_empty() || !matches!(mode_raw, "agent_control" | "human_takeover") {
        return ticket_bad_request(
            "browser_action.set_control_mode requires 'sessionId' and mode of agent_control or human_takeover",
        );
    }
    let mode = if mode_raw == "human_takeover" {
        crate::domains::browser::BrowserControlMode::HumanTakeover
    } else {
        crate::domains::browser::BrowserControlMode::AgentControl
    };
    let response = set_browser_control_mode(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(session_id.to_owned()),
        Json(BrowserControlBody { mode }),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_action.set_control_mode", user, status, payload)
}

pub(super) async fn dispatch_browser_suggest_action(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let session_id = ticket_string(input, "sessionId");
    if session_id.is_empty() {
        return ticket_bad_request("browser_action.suggest requires a non-empty 'sessionId'");
    }
    let body = SuggestActionBody {
        goal: ticket_string(input, "goal").to_owned(),
        include_screenshot: input
            .get("includeScreenshot")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    };
    let response = suggest_browser_action(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(session_id.to_owned()),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_action.suggest", user, status, payload)
}

pub(super) async fn dispatch_browser_create_profile(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let scope_raw = ticket_string(input, "scope");
    let Some(scope) = parse_browser_profile_scope(scope_raw) else {
        return ticket_bad_request(
            "browser_profile.create requires 'scope' to be one of user_private, org_shared, run_scoped",
        );
    };
    let body = CreateProfileBody {
        name: input.get("name").and_then(Value::as_str).map(str::to_owned),
        scope,
    };
    let response = create_browser_profile(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        Json(body),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_profile.create", user, status, payload)
}

pub(super) async fn dispatch_browser_rename_profile(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let profile_id = ticket_string(input, "profileId");
    if profile_id.is_empty() {
        return ticket_bad_request("browser_profile.rename requires a non-empty 'profileId'");
    }
    let name = input.get("name").and_then(Value::as_str).map(str::to_owned);
    let scope = match input.get("scope").and_then(Value::as_str) {
        Some(raw) => match parse_browser_profile_scope(raw) {
            Some(scope) => Some(scope),
            None => {
                return ticket_bad_request(
                    "browser_profile.rename 'scope' must be one of user_private, org_shared, run_scoped",
                )
            }
        },
        None => None,
    };
    if name.is_none() && scope.is_none() {
        return ticket_bad_request("browser_profile.rename requires 'name' and/or 'scope'");
    }
    let response = rename_browser_profile(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(profile_id.to_owned()),
        Json(RenameProfileBody { name, scope }),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_profile.rename", user, status, payload)
}

pub(super) async fn dispatch_browser_delete_profile(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let profile_id = ticket_string(input, "profileId");
    if profile_id.is_empty() {
        return ticket_bad_request("browser_profile.delete requires a non-empty 'profileId'");
    }
    let response = delete_browser_profile(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(profile_id.to_owned()),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_profile.delete", user, status, payload)
}

pub(super) async fn dispatch_browser_probe_profile_restore(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let profile_id = ticket_string(input, "profileId");
    let url = ticket_string(input, "url");
    if profile_id.is_empty() || url.is_empty() {
        return ticket_bad_request("browser_profile.probe_restore requires 'profileId' and 'url'");
    }
    let response = restore_browser_profile_probe(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        headers.clone(),
        PathExtractor(profile_id.to_owned()),
        Json(RestoreProbeBody {
            url: url.to_owned(),
        }),
    )
    .await;
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("browser_profile.probe_restore", user, status, payload)
}

// The owner (inference-core, via router_policy.rs) enforces no admin gate of
// its own on this write today — only org-scoping. That is a real fact about
// the current system, not something to paper over: this action's registry
// risk/approval posture is set conservatively (high risk, requires approval)
// specifically to compensate for the absence of an owner-side check, since
// changing an org's entire model-routing table (cost caps, which models
// serve which complexity tier) has organization-wide consequences regardless
// of how loosely it happens to be gated today.
pub(super) async fn dispatch_router_policy_update(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    if input.as_object().is_none_or(|obj| obj.is_empty()) {
        return ticket_bad_request("router_policy.update requires a full routing policy document");
    }
    let response = put_router_policy(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        Json(input.clone()),
    )
    .await
    .into_response();
    let (status, payload) = response_to_status_and_json(response).await;
    owner_json_response_to_envelope("router_policy.update", user, status, payload)
}

pub(super) async fn dispatch_space_create_personal(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let body = name.map(|name| Json(json!({ "name": name })));
    let (status, Json(payload)) = create_personal_space(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        body,
    )
    .await;
    owner_json_response_to_envelope("spaces.create_personal_space", user, status, payload)
}

pub(super) async fn dispatch_space_ensure_organization_room(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let body = name.map(|name| Json(json!({ "name": name })));
    let (status, Json(payload)) = ensure_organization_room(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        body,
    )
    .await;
    owner_json_response_to_envelope("spaces.ensure_organization_room", user, status, payload)
}

pub(super) async fn dispatch_space_update_instructions(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request(
            "spaces.update_space_instructions requires a non-empty 'spaceRef'",
        );
    }
    let instructions = input
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let (status, Json(payload)) = update_space_instructions(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(space_ref.to_owned()),
        Json(UpdateSpaceInstructionsRequest { instructions }),
    )
    .await;
    owner_json_response_to_envelope("spaces.update_space_instructions", user, status, payload)
}

pub(super) async fn dispatch_space_create_agent(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.create_space_agent requires a non-empty 'spaceRef'");
    }
    let body = ticket_remap(
        input,
        &[
            ("name", "name"),
            ("instructions", "instructions"),
            ("avatarColor", "avatar_color"),
        ],
    );
    let (status, Json(payload)) = create_space_agent(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(space_ref.to_owned()),
        Json(Value::Object(body)),
    )
    .await;
    owner_json_response_to_envelope("spaces.create_space_agent", user, status, payload)
}

pub(super) async fn dispatch_space_bind_agent(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let agent_ref = ticket_string(input, "agentRef");
    if space_ref.is_empty() || agent_ref.is_empty() {
        return ticket_bad_request(
            "spaces.bind_space_agent requires non-empty 'spaceRef' and 'agentRef'",
        );
    }
    let (status, Json(payload)) = bind_existing_space_agent(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(space_ref.to_owned()),
        Json(json!({ "agent_ref": agent_ref })),
    )
    .await;
    owner_json_response_to_envelope("spaces.bind_space_agent", user, status, payload)
}

pub(super) async fn dispatch_space_request_personal_deletion(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let idempotency_key = ticket_string(input, "idempotencyKey");
    if space_ref.is_empty() || idempotency_key.is_empty() {
        return ticket_bad_request(
            "spaces.request_personal_space_deletion requires non-empty 'spaceRef' and 'idempotencyKey'",
        );
    }
    let (status, Json(payload)) = request_personal_space_deletion(
        StateExtractor(state.clone()),
        ExtensionExtractor(user.clone()),
        PathExtractor(space_ref.to_owned()),
        Json(DeleteSpaceRequest {
            idempotency_key: idempotency_key.to_owned(),
        }),
    )
    .await;
    owner_json_response_to_envelope(
        "spaces.request_personal_space_deletion",
        user,
        status,
        payload,
    )
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
    let actor = actor_for_user(user);
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
    let actor = actor_for_user(user);
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
    let actor = actor_for_user(user);
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

fn actor_for_user(user: &AuthenticatedUser) -> ActionActor {
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
