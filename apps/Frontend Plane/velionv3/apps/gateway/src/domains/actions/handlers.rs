use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

use super::dispatchers::{
    dispatch_brreg_lookup, dispatch_connect_source, dispatch_crawl_site, dispatch_import_source,
    dispatch_operating_map_blueprint, dispatch_operating_map_generate,
    dispatch_operating_map_review, dispatch_recrawl, dispatch_scrape_url, dispatch_ticket_assign,
    dispatch_ticket_classify, dispatch_ticket_create, dispatch_ticket_link_resource,
    dispatch_ticket_resolve, dispatch_ticket_update, dispatch_toggle_policy, dispatch_upload_files,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExecuteActionRequest {
    action_id: String,
    input: Value,
}

pub(super) async fn execute_action(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<ExecuteActionRequest>,
) -> Response {
    match body.action_id.as_str() {
        "knowledge.recrawl_source" => dispatch_recrawl(&state, &user, &headers, &body.input).await,
        "knowledge.scrape_url" => dispatch_scrape_url(&state, &user, &headers, &body.input).await,
        "knowledge.crawl_site" => dispatch_crawl_site(&state, &user, &headers, &body.input).await,
        "knowledge.import_source" => {
            dispatch_import_source(&state, &user, &headers, &body.input).await
        }
        "knowledge.connect_source" => dispatch_connect_source(&state, &user, &body.input).await,
        "knowledge.upload_files" => dispatch_upload_files().await,
        "brreg_lookup_organization" | "brreg.lookup_organization" => {
            dispatch_brreg_lookup(&state, &user, &body.input).await
        }
        "operating_map.generate" | "operating_map.refresh" => {
            dispatch_operating_map_generate(&state, &user, &body.input, &body.action_id).await
        }
        "operating_map.review_proposal" => {
            dispatch_operating_map_review(&state, &user, &body.input).await
        }
        "operating_map.create_agent_blueprint" => {
            dispatch_operating_map_blueprint(&state, &user, &body.input).await
        }
        "workflows.toggle_policy" => dispatch_toggle_policy(&state, &user, &body.input).await,
        // Ticketing actions -> conversation-core-go (the same backend the dedicated
        // /api/v1/tickets/* routes proxy to), scoped to the caller's org.
        "tickets.create" => dispatch_ticket_create(&state, &user, &body.input).await,
        "tickets.classify_conversation" => {
            dispatch_ticket_classify(&state, &user, &body.input).await
        }
        "tickets.update" => dispatch_ticket_update(&state, &user, &body.input).await,
        "tickets.assign" => dispatch_ticket_assign(&state, &user, &body.input).await,
        "tickets.link_resource" => dispatch_ticket_link_resource(&state, &user, &body.input).await,
        "tickets.resolve" => dispatch_ticket_resolve(&state, &user, &body.input).await,
        other => (
            StatusCode::NOT_IMPLEMENTED,
            Json(error(
                "not_implemented",
                format!("no live dispatch for action '{other}'"),
            )),
        )
            .into_response(),
    }
}
