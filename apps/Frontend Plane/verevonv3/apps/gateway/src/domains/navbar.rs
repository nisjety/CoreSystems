use axum::{
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
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
    envelope::{error, ok},
    middleware::{require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json, proxy_notification_json},
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
        .route(
            "/api/v1/navbar/calendar",
            get(calendar_state).post(create_calendar_entry),
        )
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
    let org_id = authorized_org_id(&state, &user).await;

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
    let (notif_status, Json(feed)) = proxy_notification_json(
        &state,
        Method::GET,
        &format!("{}/notifications", state.notification_core_url),
        None,
        &org_id,
        &actor,
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

    let (_, Json(count_body)) = proxy_notification_json(
        &state,
        Method::GET,
        &format!("{}/notifications/unread/count", state.notification_core_url),
        None,
        &org_id,
        &actor,
    )
    .await;
    let unread_count = count_body
        .get("count")
        .or_else(|| count_body.get("unread"))
        .or_else(|| count_body.get("unreadCount"))
        .and_then(Value::as_u64)
        .or_else(|| feed.get("total_count").and_then(Value::as_u64))
        .unwrap_or(0);

    // Plan ← billing-core is the source of truth for the org's plan (user-core
    // /me carries no plan). Fall back to any plan on /me, then "trial" only when
    // billing-core is unavailable / no account exists.
    let plan = {
        let (status, Json(acct)) = proxy_json(
            &state,
            Method::GET,
            &format!(
                "{}/api/v1/billing/orgs/{}/account",
                state.billing_core_url, org_id
            ),
            None,
            Some(org_id.as_str()),
            Some(&actor),
            None,
        )
        .await;
        let billing_plan = if status.is_success() {
            let data = crate::envelope::unwrap_data(&acct);
            first_str(&data, &["plan"])
                .filter(|p| !p.trim().is_empty())
                .map(str::to_owned)
        } else {
            None
        };
        billing_plan
            .or_else(|| first_str(&me_user, &["account_plan", "plan"]).map(str::to_owned))
            .unwrap_or_else(|| "trial".to_owned())
    };

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
        // Personal calendar state is owned and persisted by user-core. Keep a
        // degraded navbar render usable if that optional projection is down;
        // the dedicated calendar endpoint reports the upstream error instead
        // of pretending a save succeeded.
        "calendar": calendar_state_for(&state, &actor).await,
    })))
}

async fn navbar_search(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let org_id = authorized_org_id(&state, &user).await;
    let query = params.get("q").cloned().unwrap_or_default();
    if query.trim().is_empty() {
        return Json(ok(json!({ "results": [] })));
    }

    // Knowledge-scoped quick search → retrieval-engine. Degrade to an empty
    // (real) result set if retrieval is unavailable rather than erroring the navbar.
    let (status, Json(body)) = crate::domains::knowledge::shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::POST,
        &format!("{}/v1/knowledge/search", state.retrieval_engine_url),
        Some(json!({ "org_id": org_id, "query": query, "top_k": 6, "filters": {} })),
        Some(&org_id),
        None,
    )
    .await;

    if !status.is_success() {
        return Json(ok(json!({ "results": [] })));
    }

    let source_titles = body
        .get("sources")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|source| {
                    Some((
                        first_str(source, &["document_id"])?.to_owned(),
                        (
                            first_str(source, &["title"])
                                .unwrap_or("Knowledge result")
                                .to_owned(),
                            first_str(source, &["source"])
                                .unwrap_or("knowledge")
                                .to_owned(),
                        ),
                    ))
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let results: Vec<Value> = body
        .get("candidates")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|r| {
                    let document_id = first_str(r, &["document_id"]).unwrap_or("");
                    let (title, source) = source_titles
                        .get(document_id)
                        .map(|(title, source)| (title.as_str(), source.as_str()))
                        .unwrap_or(("Knowledge result", "knowledge"));
                    json!({
                        "id": first_str(r, &["knowledge_id"]).unwrap_or(document_id),
                        "label": title,
                        "excerpt": first_str(r, &["text"]).unwrap_or(""),
                        "href": format!("/knowledge?source={}", urlencoding::encode(document_id)),
                        "source": source,
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
    let org_id = authorized_org_id(&state, &user).await;
    let (status, Json(body)) = proxy_notification_json(
        &state,
        Method::POST,
        &format!(
            "{}/notifications/{}/read",
            state.notification_core_url,
            urlencoding::encode(&id)
        ),
        None,
        &org_id,
        &actor_for(&user),
    )
    .await;
    if !status.is_success() {
        return (status, Json(body)).into_response();
    }
    Json(ok(json!({ "ok": true }))).into_response()
}

async fn calendar_state(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let actor = actor_for(&user);
    let (status, Json(body)) = proxy_json(
        &state,
        Method::GET,
        &format!("{}/api/v1/calendar/events", state.user_core_url),
        None,
        None,
        Some(&actor),
        None,
    )
    .await;

    (status, Json(body)).into_response()
}

async fn calendar_state_for(state: &AppState, actor: &ActionActor) -> Value {
    let (status, Json(body)) = proxy_json(
        state,
        Method::GET,
        &format!("{}/api/v1/calendar/events", state.user_core_url),
        None,
        None,
        Some(actor),
        None,
    )
    .await;

    if status.is_success() {
        crate::envelope::unwrap_data(&body)
    } else {
        json!({ "events": [], "notes": [] })
    }
}

async fn create_calendar_entry(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let actor = actor_for(&user);
    let path = if body.get("kind").and_then(Value::as_str) == Some("note") {
        "/api/v1/calendar/notes"
    } else {
        "/api/v1/calendar/events"
    };
    let (status, Json(response)) = proxy_json(
        &state,
        Method::POST,
        &format!("{}{}", state.user_core_url, path),
        Some(body),
        None,
        Some(&actor),
        None,
    )
    .await;

    (status, Json(response)).into_response()
}

async fn submit_support(
    State(_state): State<AppState>,
    Extension(_user): Extension<AuthenticatedUser>,
    Json(_body): Json<Value>,
) -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "support_intake_unavailable",
            "Support intake is disabled until an authoritative support identity mapping is configured",
        )),
    )
        .into_response()
}
