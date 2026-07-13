//! Ticketing domain — durable support work queue.
//!
//! Browser-facing `/api/v1/tickets/*` routes are thin authenticated proxies to
//! conversation-core-go. The org scope is derived from the validated session.

use axum::{
    extract::{Extension, Path, State},
    http::Uri,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState,
    middleware::{require_session, AuthenticatedUser},
    upstream::proxy_conversation_json,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/tickets", get(list_tickets).post(create_ticket))
        .route(
            "/api/v1/tickets/conversations/:id/classifications",
            post(classify_conversation),
        )
        .route("/api/v1/tickets/:id", get(get_ticket).patch(patch_ticket))
        .route("/api/v1/tickets/:id/links", post(link_ticket_resource))
        .route(
            "/api/v1/tickets/:id/macros/:macro_id/run",
            post(run_ticket_macro),
        )
        .route(
            "/api/v1/tickets/:id/checklists",
            post(create_ticket_checklist),
        )
        .route(
            "/api/v1/tickets/:id/checklists/:checklist_id/items/:item_id",
            axum::routing::patch(patch_ticket_checklist_item),
        )
        .route(
            "/api/v1/ticket-views",
            get(list_ticket_views).post(create_ticket_view),
        )
        .route(
            "/api/v1/ticket-views/:id",
            axum::routing::patch(patch_ticket_view),
        )
        .route(
            "/api/v1/ticket-macros",
            get(list_ticket_macros).post(create_ticket_macro),
        )
        .route(
            "/api/v1/ticket-macros/:id",
            axum::routing::patch(patch_ticket_macro),
        )
        .route(
            "/api/v1/ticket-automation-rules",
            get(list_ticket_automation_rules).post(create_ticket_automation_rule),
        )
        .route(
            "/api/v1/ticket-automation-rules/:id",
            axum::routing::patch(patch_ticket_automation_rule),
        )
        .route(
            "/api/v1/sla-policies",
            get(list_sla_policies).post(create_sla_policy),
        )
        .route(
            "/api/v1/sla-policies/:id",
            axum::routing::patch(patch_sla_policy),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

fn qs(uri: &Uri) -> String {
    uri.query()
        .filter(|q| !q.is_empty())
        .map(|q| format!("?{q}"))
        .unwrap_or_default()
}

async fn list_tickets(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    uri: Uri,
) -> Response {
    let url = format!("{}/api/v1/tickets{}", state.conversation_core_url, qs(&uri));
    proxy_conversation_json(&state, Method::GET, &url, None, &user, None)
        .await
        .into_response()
}

async fn create_ticket(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(&state, &user, Method::POST, "/api/v1/tickets", Some(body)).await
}

async fn get_ticket(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> Response {
    let path = format!("/api/v1/tickets/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::GET, &path, None).await
}

async fn patch_ticket(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/tickets/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn link_ticket_resource(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/tickets/{}/links", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::POST, &path, Some(body)).await
}

async fn run_ticket_macro(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, macro_id)): Path<(String, String)>,
) -> Response {
    let path = format!(
        "/api/v1/tickets/{}/macros/{}/run",
        urlencoding::encode(&id),
        urlencoding::encode(&macro_id)
    );
    forward_ticket_json(&state, &user, Method::POST, &path, None).await
}

async fn create_ticket_checklist(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/tickets/{}/checklists", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::POST, &path, Some(body)).await
}

async fn patch_ticket_checklist_item(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, checklist_id, item_id)): Path<(String, String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!(
        "/api/v1/tickets/{}/checklists/{}/items/{}",
        urlencoding::encode(&id),
        urlencoding::encode(&checklist_id),
        urlencoding::encode(&item_id)
    );
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn list_ticket_views(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(&state, &user, Method::GET, "/api/v1/ticket-views", None).await
}

async fn create_ticket_view(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::POST,
        "/api/v1/ticket-views",
        Some(body),
    )
    .await
}

async fn patch_ticket_view(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/ticket-views/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn list_ticket_macros(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(&state, &user, Method::GET, "/api/v1/ticket-macros", None).await
}

async fn create_ticket_macro(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::POST,
        "/api/v1/ticket-macros",
        Some(body),
    )
    .await
}

async fn patch_ticket_macro(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/ticket-macros/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn list_ticket_automation_rules(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::GET,
        "/api/v1/ticket-automation-rules",
        None,
    )
    .await
}

async fn create_ticket_automation_rule(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::POST,
        "/api/v1/ticket-automation-rules",
        Some(body),
    )
    .await
}

async fn patch_ticket_automation_rule(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!(
        "/api/v1/ticket-automation-rules/{}",
        urlencoding::encode(&id)
    );
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn list_sla_policies(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    forward_ticket_json(&state, &user, Method::GET, "/api/v1/sla-policies", None).await
}

async fn create_sla_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> Response {
    forward_ticket_json(
        &state,
        &user,
        Method::POST,
        "/api/v1/sla-policies",
        Some(body),
    )
    .await
}

async fn patch_sla_policy(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!("/api/v1/sla-policies/{}", urlencoding::encode(&id));
    forward_ticket_json(&state, &user, Method::PATCH, &path, Some(body)).await
}

async fn classify_conversation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = format!(
        "/api/v1/conversations/{}/ticket-classifications",
        urlencoding::encode(&id)
    );
    forward_ticket_json(&state, &user, Method::POST, &path, Some(body)).await
}

async fn forward_ticket_json(
    state: &AppState,
    user: &AuthenticatedUser,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Response {
    let url = format!("{}{}", state.conversation_core_url, path);
    proxy_conversation_json(state, method, &url, body, user, None)
        .await
        .into_response()
}
