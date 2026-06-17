use axum::{
    extract::{Extension, Path, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::ok,
    middleware::{require_session, AuthenticatedUser},
    upstream::proxy_json,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        // Static routes before param routes to avoid shadowing
        .route("/api/v1/notifications/preferences", get(get_preferences))
        .route(
            "/api/v1/notifications/preferences/:event_type/:channel",
            put(update_preference),
        )
        .route("/api/v1/notifications/unread/count", get(unread_count))
        .route("/api/v1/notifications/mark-all-read", post(mark_all_read))
        // Param routes after statics
        .route("/api/v1/notifications", get(list_notifications))
        .route("/api/v1/notifications/:id/read", post(mark_read))
        .route("/api/v1/notifications/:id", delete(delete_notification))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

// ── Preferences ───────────────────────────────────────────────────────────────

async fn get_preferences(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl axum::response::IntoResponse {
    let url = format!("{}/preferences", state.notification_core_url);
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn update_preference(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((event_type, channel)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/preferences/{}/{}",
        state.notification_core_url,
        urlencoding::encode(&event_type),
        urlencoding::encode(&channel)
    );
    proxy_json(
        &state,
        Method::PUT,
        &url,
        Some(body),
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

// ── Notifications ─────────────────────────────────────────────────────────────

async fn list_notifications(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl axum::response::IntoResponse {
    let url = format!("{}/notifications", state.notification_core_url);
    let (status, Json(body)) = proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(body));
    }
    // notification-core returns `{ notifications: [...] }` (the navbar reads the same
    // shape), but the SPA's listNotifications() expects a bare `Notification[]`.
    // Normalize to the array the client's type promises.
    let items = body
        .get("notifications")
        .or_else(|| body.get("data"))
        .filter(|value| value.is_array())
        .cloned()
        .unwrap_or_else(|| {
            if body.is_array() {
                body.clone()
            } else {
                json!([])
            }
        });
    (status, Json(ok(items)))
}

async fn unread_count(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl axum::response::IntoResponse {
    let url = format!("{}/notifications/unread/count", state.notification_core_url);
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn mark_read(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/notifications/{}/read",
        state.notification_core_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::POST,
        &url,
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn mark_all_read(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/notifications/mark-all-read",
        state.notification_core_url
    );
    proxy_json(
        &state,
        Method::POST,
        &url,
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}

async fn delete_notification(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/notifications/{}",
        state.notification_core_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::DELETE,
        &url,
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await
}
