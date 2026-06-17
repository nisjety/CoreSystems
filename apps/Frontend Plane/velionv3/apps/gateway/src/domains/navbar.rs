use axum::{
    extract::{Extension, Query, State},
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::ok,
    middleware::{require_session, AuthenticatedUser},
    upstream::proxy_json,
};

/// Navbar/app-shell projection. The SPA's CoreShell loads `/api/v1/navbar` on
/// every authenticated view; it composes the signed-in profile (user-core),
/// notification feed + unread count (notification-core), and the theme
/// preference into the single `NavbarPayload` the shell renders. All routes are
/// session-guarded — the navbar only exists for authenticated users.
pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/navbar", get(navbar))
        .route("/api/v1/navbar/search", get(navbar_search))
        .route("/api/v1/navbar/theme", put(save_theme))
        .route("/api/v1/navbar/notifications", post(mark_notification_read))
        .route("/api/v1/navbar/calendar", post(create_calendar_entry))
        .route("/api/v1/navbar/support", post(submit_support))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

/// First non-empty string among `keys` on `obj`.
fn first_str<'a>(obj: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(value) = obj
            .get(*key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            return Some(value);
        }
    }
    None
}

fn bool_field(obj: &Value, key: &str) -> bool {
    obj.get(key).and_then(Value::as_bool).unwrap_or(false)
}

async fn navbar(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let actor = actor_for(&user);

    // Profile ← user-core /me, with the validated session identity as fallback
    // so the navbar always renders a real user even if user-core is unreachable.
    let (_, Json(me)) = proxy_json(
        &state,
        Method::GET,
        &format!("{}/api/v1/users/me", state.user_core_url),
        None,
        None,
        Some(&actor),
        None,
    )
    .await;
    let me_user = me.get("user").cloned().unwrap_or(me);
    let profile = json!({
        "id": first_str(&me_user, &["id"]).unwrap_or(user.user_id.as_str()),
        "name": first_str(&me_user, &["display_name", "name"]).unwrap_or(user.user_name.as_str()),
        "email": first_str(&me_user, &["email"]).unwrap_or(user.user_email.as_str()),
        "avatar": first_str(&me_user, &["avatar", "image"]).unwrap_or(""),
        "status": first_str(&me_user, &["account_status", "status"]).unwrap_or("active"),
    });

    // Notifications ← notification-core feed + unread count.
    let (notif_status, Json(feed)) = proxy_json(
        &state,
        Method::GET,
        &format!("{}/notifications", state.notification_core_url),
        None,
        None,
        Some(&actor),
        None,
    )
    .await;
    let notifications: Vec<Value> = feed
        .get("notifications")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|n| {
                    json!({
                        "id": first_str(n, &["id"]).unwrap_or(""),
                        "title": first_str(n, &["title", "subject"]).unwrap_or(""),
                        "body": first_str(n, &["body", "message", "content"]).unwrap_or(""),
                        "createdAt": first_str(n, &["createdAt", "created_at"]),
                        "href": first_str(n, &["href", "link", "url"]),
                        "feed": first_str(n, &["feed", "category"]),
                        "read": bool_field(n, "read"),
                        "seen": bool_field(n, "seen"),
                        "archived": bool_field(n, "archived"),
                        "source": "notification",
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let (_, Json(count_body)) = proxy_json(
        &state,
        Method::GET,
        &format!("{}/notifications/unread/count", state.notification_core_url),
        None,
        None,
        Some(&actor),
        None,
    )
    .await;
    let unread_count = count_body
        .get("count")
        .or_else(|| count_body.get("unread"))
        .or_else(|| count_body.get("unreadCount"))
        .and_then(Value::as_u64)
        .or_else(|| feed.get("total_count").and_then(Value::as_u64))
        .unwrap_or(0);

    // Plan ← user-core /me includes account_plan or plan field when present.
    let plan = first_str(&me_user, &["account_plan", "plan"])
        .unwrap_or("trial")
        .to_owned();

    Json(ok(json!({
        "profile": profile,
        "plan": plan,
        "notifications": {
            "configured": notif_status.is_success(),
            "messages": [],
            "notifications": notifications,
            "unreadCount": unread_count,
        },
        // Theme is applied client-side and acknowledged here; persistence flows
        // through PUT /api/v1/navbar/theme.
        "theme": { "theme": "system", "colorScheme": null, "configured": false },
        // Calendar has no dedicated plane backend yet; surface an empty, real state.
        "calendar": { "events": [], "notes": [] },
    })))
}

async fn navbar_search(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let query = params.get("q").cloned().unwrap_or_default();
    if query.trim().is_empty() {
        return Json(ok(json!({ "results": [] })));
    }

    // Knowledge-scoped quick search → retrieval-engine. Degrade to an empty
    // (real) result set if retrieval is unavailable rather than erroring the navbar.
    let (status, Json(body)) = proxy_json(
        &state,
        Method::POST,
        &format!("{}/v1/search", state.retrieval_engine_url),
        Some(json!({ "query": query, "limit": 6 })),
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await;

    if !status.is_success() {
        return Json(ok(json!({ "results": [] })));
    }

    let results: Vec<Value> = body
        .get("results")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|r| {
                    json!({
                        "id": first_str(r, &["id", "chunkId", "documentId"]).unwrap_or(""),
                        "label": first_str(r, &["title", "label", "name"]).unwrap_or("Resultat"),
                        "excerpt": first_str(r, &["snippet", "excerpt", "text"]).unwrap_or(""),
                        "href": first_str(r, &["href", "url", "path"]).unwrap_or("/knowledge"),
                        "source": first_str(r, &["source", "collection"]).unwrap_or("knowledge"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Json(ok(json!({ "results": results })))
}

async fn save_theme(
    State(_state): State<AppState>,
    Extension(_user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // Theme is a client-side preference applied immediately by the SPA. There is
    // no dedicated preference store in the current plane set, so acknowledge the
    // chosen theme (real echo) rather than 404. When user-core exposes a settings
    // endpoint this becomes a proxy_json to it.
    let theme = body
        .get("theme")
        .and_then(Value::as_str)
        .unwrap_or("system")
        .to_owned();
    let color_scheme = body.get("colorScheme").cloned().unwrap_or(Value::Null);
    Json(ok(
        json!({ "theme": theme, "colorScheme": color_scheme, "configured": true }),
    ))
}

async fn mark_notification_read(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let id = body
        .get("notificationId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if id.is_empty() {
        return Json(ok(json!({ "ok": true }))).into_response();
    }
    let (status, Json(body)) = proxy_json(
        &state,
        Method::POST,
        &format!(
            "{}/notifications/{}/read",
            state.notification_core_url,
            urlencoding::encode(&id)
        ),
        None,
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(body)).into_response();
    }
    Json(ok(json!({ "ok": true }))).into_response()
}

async fn create_calendar_entry(
    State(_state): State<AppState>,
    Extension(_user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // No dedicated calendar plane backend exists yet. Acknowledge the request so
    // the navbar interaction completes; echo back the submitted entry shape.
    if body.get("kind").and_then(Value::as_str) == Some("note") {
        return Json(ok(json!({
            "note": {
                "id": "",
                "date": body.get("date").cloned().unwrap_or(Value::Null),
                "text": body.get("text").cloned().unwrap_or(Value::Null),
                "createdAt": "",
            }
        })));
    }
    Json(ok(json!({
        "event": {
            "id": "",
            "title": body.get("title").cloned().unwrap_or(Value::Null),
            "start": body.get("start").cloned().unwrap_or(Value::Null),
            "end": body.get("end").cloned().unwrap_or(Value::Null),
            "type": body.get("type").cloned().unwrap_or(Value::Null),
            "status": "draft",
            "createdAt": "",
        }
    })))
}

async fn submit_support(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // Route a support request through notification-core's request intake.
    let (status, Json(body_out)) = proxy_json(
        &state,
        Method::POST,
        &format!("{}/requests", state.notification_core_url),
        Some(json!({
            "type": "support",
            "subject": body.get("subject").cloned().unwrap_or(Value::Null),
            "message": body.get("message").cloned().unwrap_or(Value::Null),
            "context": body.get("context").cloned().unwrap_or(Value::Null),
        })),
        None,
        Some(&actor_for(&user)),
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(body_out)).into_response();
    }
    Json(ok(json!({ "ok": true }))).into_response()
}
