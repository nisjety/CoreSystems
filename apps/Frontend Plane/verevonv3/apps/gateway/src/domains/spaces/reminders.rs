//! Reminders: the BFF half of WS11-S11.
//!
//! * `POST /api/v1/spaces/{space_ref}/reminders {thread_id, message_id?, remind_at, idempotency_key?}`
//!   creates a reminder for the CALLER on one thread (and optionally one message) of a room they
//!   belong to: `201 {data:{reminder_id, remind_at}}`, or `200` with `deduplicated: true` when an
//!   identical pending reminder already exists.
//! * `GET /api/v1/spaces/reminders?state=pending` lists the caller's own pending reminders.
//! * `DELETE /api/v1/spaces/reminders/{reminder_id}` cancels one of the caller's own pending
//!   reminders. Another member's reminder answers exactly like one that does not exist.
//! * `spaces.create_reminder` and `spaces.cancel_reminder` ride [`create_reminder_core`] and
//!   [`cancel_reminder_core`], the same functions as the routes.
//!
//! Content. A reminder row holds ids, a closed state word and timestamps only; there is no note
//! field (a note would be user content). Creating one is still a durable write, so under Zero Data
//! Retention it is the typed 412 before any Application call. Cancelling removes data and listing
//! reads only the caller's own rows, so both stay allowed. Membership of the room is proved here
//! (Control's live role) before the write: Application only checks that the Space exists in the
//! caller's organization. At fire time Application re-checks the audience again.
//!
//! The flag (`SPACE_REMINDERS_API`, `space_reminders_api`) is a rollout control, never an
//! authorization input. Flag off answers 404 `space_reminders_disabled` on every route.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_request` / `invalid_space` /
//! `invalid_thread` / `invalid_message` / `invalid_state` / `invalid_reminder_time`, 403
//! `active_membership_required`, 404 `space_reminders_disabled` / `space_not_found` /
//! `reminder_not_found`, 412 `zdr_durable_mutation_forbidden`, 429 `too_many_reminders` (above 50
//! pending), 503 `reminders_unavailable` / `zdr_decision_unavailable`.

use std::collections::HashMap;

use axum::{
    extract::{rejection::JsonRejection, Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    convex_gateway_call, resolve_space_role, room_admin::convex_mutation_message,
};
use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
};

type Reply = (StatusCode, Json<Value>);

const MAX_ID_LEN: usize = 200;
/// Application caps pending reminders per person here too; the 429 detail reports it.
const MAX_PENDING: u32 = 50;

/// Registered through `module_routes()` behind `require_session`. The `reminders` segments are
/// static, which matchit ranks above `{space_ref}`, so a room can never shadow them.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/spaces/reminders", get(list_reminders))
        .route(
            "/api/v1/spaces/reminders/{reminder_id}",
            delete(cancel_reminder),
        )
        .route("/api/v1/spaces/{space_ref}/reminders", post(create_reminder))
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn disabled() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "space_reminders_disabled",
        "Space reminders are not enabled.",
    )
}

fn membership_required() -> Reply {
    refuse(
        StatusCode::FORBIDDEN,
        "active_membership_required",
        "An active organization and room membership is required to use reminders.",
    )
}

fn unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "reminders_unavailable",
        "Reminders are unavailable right now. Nothing was changed.",
    )
}

fn invalid_request() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "The request body is not valid.",
    )
}

fn invalid_time() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_reminder_time",
        "remind_at must be a time between five seconds and 366 days from now.",
    )
}

fn not_found() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "reminder_not_found",
        "That reminder was not found.",
    )
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_LEN && !value.chars().any(char::is_control)
}

/// Flag, then organization. Everything every route shares.
async fn gate(state: &AppState, user: &AuthenticatedUser) -> Result<String, Reply> {
    if !state.space_features.reminders_api {
        return Err(disabled());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(membership_required());
    }
    Ok(org_id)
}

/// The wire shape of one reminder.
fn reminder_json(value: &Value) -> Value {
    json!({
        "reminder_id": value.get("reminderId").cloned().unwrap_or(Value::Null),
        "space_ref": value.get("spaceRef").cloned().unwrap_or(Value::Null),
        "thread_id": value.get("threadId").cloned().unwrap_or(Value::Null),
        "message_id": value.get("messageId").cloned().unwrap_or(Value::Null),
        "remind_at": value.get("remindAt").cloned().unwrap_or(Value::Null),
        "state": value.get("state").cloned().unwrap_or(Value::Null),
        "created_at": value.get("createdAt").cloned().unwrap_or(Value::Null),
    })
}

// ── create ───────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBody {
    thread_id: String,
    message_id: Option<String>,
    /// Epoch milliseconds; Application bounds it to five seconds..366 days ahead.
    remind_at: i64,
    /// Accepted for the contract; Application deduplicates on the natural key (room, thread,
    /// message, time), so an identical retry returns the same reminder either way.
    #[allow(dead_code)]
    idempotency_key: Option<String>,
}

async fn create_reminder(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Ok(Json(body)) = body else {
        return invalid_request();
    };
    create_reminder_core(&state, &user, Some(&headers), &space_ref, &body).await
}

/// The reminder write, shared by the route and the `spaces.create_reminder` action. Flag, then
/// organization, then ids, then the caller's LIVE membership of the room (Control), then the Zero
/// Data Retention refusal, then the Application call.
pub(crate) async fn create_reminder_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: Option<&HeaderMap>,
    space_ref: &str,
    body: &Value,
) -> Reply {
    let org_id = match gate(state, user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let Ok(body) = serde_json::from_value::<CreateBody>(body.clone()) else {
        return invalid_request();
    };
    let space_ref = space_ref.trim();
    if !valid_id(space_ref) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        );
    }
    let thread_id = body.thread_id.trim();
    if !valid_id(thread_id) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "A thread id is required.",
        );
    }
    let message_id = body
        .message_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    if message_id.is_some_and(|id| !valid_id(id)) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_message",
            "The message id is not valid.",
        );
    }
    match resolve_space_role(state, user, &org_id, space_ref).await {
        Ok(role) if !role.trim().is_empty() => {}
        Ok(_) => return membership_required(),
        Err(reply) => return reply,
    }
    if let Some(refusal) = crate::zdr::durable_write_refusal(state, user, headers, None).await {
        return refusal;
    }
    let mut args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "threadId": thread_id,
        "remindAt": body.remind_at,
    });
    if let (Some(id), Some(object)) = (message_id, args.as_object_mut()) {
        object.insert("messageId".to_owned(), json!(id));
    }
    match convex_mutation_message(state, "spaceReminders:createReminderForGateway", args).await {
        Ok(value) => {
            let deduplicated = value
                .get("deduplicated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let status = if deduplicated {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            (
                status,
                Json(json!({"data": {
                    "reminder_id": value.get("reminderId").cloned().unwrap_or(Value::Null),
                    "remind_at": value.get("remindAt").cloned().unwrap_or(Value::Null),
                    "deduplicated": deduplicated,
                }})),
            )
        }
        Err(Some(message)) => map_create_failure(&message),
        Err(None) => unavailable(),
    }
}

/// Maps Application's stable mutation messages to the typed browser errors.
fn map_create_failure(message: &str) -> Reply {
    if message.contains("invalid_reminder_time") {
        invalid_time()
    } else if message.contains("too_many_reminders") {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(error_with_details(
                "too_many_reminders",
                "You have the most pending reminders allowed. Cancel one to add another.",
                json!({"max_pending": MAX_PENDING}),
            )),
        )
    } else if message.contains("space_not_found") {
        refuse(
            StatusCode::NOT_FOUND,
            "space_not_found",
            "That room was not found.",
        )
    } else if message.contains("invalid_space_ref") {
        refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        )
    } else if message.contains("invalid_thread_id") {
        refuse(
            StatusCode::BAD_REQUEST,
            "invalid_thread",
            "A thread id is required.",
        )
    } else if message.contains("invalid_message_id") {
        refuse(
            StatusCode::BAD_REQUEST,
            "invalid_message",
            "The message id is not valid.",
        )
    } else {
        unavailable()
    }
}

// ── list ─────────────────────────────────────────────────────────────────────────────────────────

async fn list_reminders(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Reply {
    let org_id = match gate(&state, &user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    // Only pending reminders are stored as live rows; settled ones are healed away.
    if params.get("state").is_some_and(|word| word != "pending") {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_state",
            "state must be pending.",
        );
    }
    match convex_gateway_call(
        &state,
        "query",
        "spaceReminders:listPendingRemindersForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
        }),
    )
    .await
    {
        Ok(value) => {
            let reminders: Vec<Value> = value
                .as_array()
                .map(|rows| rows.iter().map(reminder_json).collect())
                .unwrap_or_default();
            (StatusCode::OK, Json(json!({"data": {"reminders": reminders}})))
        }
        Err(()) => unavailable(),
    }
}

// ── cancel ───────────────────────────────────────────────────────────────────────────────────────

async fn cancel_reminder(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(reminder_id): Path<String>,
) -> Reply {
    cancel_reminder_core(&state, &user, &reminder_id).await
}

/// The cancel, shared by the route and the `spaces.cancel_reminder` action. Cancelling removes
/// data, so it is allowed under Zero Data Retention.
pub(crate) async fn cancel_reminder_core(
    state: &AppState,
    user: &AuthenticatedUser,
    reminder_id: &str,
) -> Reply {
    let org_id = match gate(state, user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let reminder_id = reminder_id.trim();
    if !valid_id(reminder_id) {
        return not_found();
    }
    match convex_mutation_message(
        state,
        "spaceReminders:cancelReminderForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "reminderId": reminder_id,
        }),
    )
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"data": {"reminder_id": reminder_id, "cancelled": true}})),
        ),
        Err(Some(message)) if message.contains("reminder_not_found") => not_found(),
        Err(_) => unavailable(),
    }
}

#[cfg(test)]
mod tests {
    //! WS11-S11.gw tests: the reminder routes through the real router, with wiremock for auth-core,
    //! Control (user-core), org-core and Application (Convex). No model is called.
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const CREATE: &str = "spaceReminders:createReminderForGateway";
    const LIST: &str = "spaceReminders:listPendingRemindersForGateway";
    const CANCEL: &str = "spaceReminders:cancelReminderForGateway";
    const ROOM_REMINDERS: &str = "/api/v1/spaces/room-a/reminders";

    struct Fx {
        state: AppState,
        _auth: MockServer,
        user_core: MockServer,
        org: MockServer,
        application: MockServer,
    }

    /// `zdr`: org-core's answer for `org-1` (`None` is an outage, so an unknown posture). `role`:
    /// what Control says the caller holds in `room-a` (empty is "not a member"). The reminders
    /// flag is on.
    async fn fx(zdr: Option<bool>, role: &str) -> Fx {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/room-a/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "room-a", "kind": "room", "role": role}
            })))
            .mount(&user_core)
            .await;
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(match zdr {
                Some(zdr) => ResponseTemplate::new(200).set_body_json(
                    json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}),
                ),
                None => ResponseTemplate::new(500),
            })
            .mount(&org)
            .await;
        let application = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org.uri();
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.reminders_api = true;
        Fx {
            state,
            _auth: auth,
            user_core,
            org,
            application,
        }
    }

    async fn mount_convex(fx: &Fx, route: &str, function: &str, template: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(wm_path(route))
            .and(body_partial_json(json!({"path": function})))
            .respond_with(template)
            .mount(&fx.application)
            .await;
    }

    fn value(value: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "success", "value": value}))
    }

    fn convex_error(message: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": message}))
    }

    async fn call(
        fx: &Fx,
        method: &str,
        uri: &str,
        body: Option<Value>,
        extra: Option<(&str, &str)>,
    ) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        if let Some((name, value)) = extra {
            request = request.header(name, value);
        }
        let response = crate::build_router(fx.state.clone())
            .oneshot(
                request
                    .body(match &body {
                        Some(value) => Body::from(value.to_string()),
                        None => Body::empty(),
                    })
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let payload: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, payload)
    }

    /// Bodies of the Convex calls to one function.
    async fn convex_calls(fx: &Fx, function: &str) -> Vec<Value> {
        fx.application
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .filter(|body| body["path"] == function)
            .collect()
    }

    async fn request_count(server: &MockServer) -> usize {
        server.received_requests().await.unwrap().len()
    }

    /// How many times Control was asked for a room membership.
    async fn membership_calls(fx: &Fx) -> usize {
        fx.user_core
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path().ends_with("/membership"))
            .count()
    }

    fn create_body() -> Value {
        json!({"thread_id": "thread-1", "message_id": "msg-1", "remind_at": 1_900_000_000_000_i64})
    }

    // ---- create ----------------------------------------------------------------------------------

    #[tokio::test]
    async fn create_reminder_201_then_200_deduplicated() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "viewer").await;
        // The first answer creates the row; every later identical call finds it.
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .and(body_partial_json(json!({"path": CREATE})))
            .respond_with(value(json!({"reminderId": "rem-1", "remindAt": 1_900_000_000_000_i64})))
            .up_to_n_times(1)
            .mount(&fx.application)
            .await;
        mount_convex(
            &fx,
            "/api/mutation",
            CREATE,
            value(json!({"reminderId": "rem-1", "remindAt": 1_900_000_000_000_i64, "deduplicated": true})),
        )
        .await;
        let mut body = create_body();
        body["idempotency_key"] = json!("retry-key-1");
        let (status, got) = call(&fx, "POST", ROOM_REMINDERS, Some(body.clone()), None).await;
        assert_eq!(status, 201, "{got}");
        assert_eq!(
            got["data"],
            json!({"reminder_id": "rem-1", "remind_at": 1_900_000_000_000_i64, "deduplicated": false})
        );
        let (status, got) = call(&fx, "POST", ROOM_REMINDERS, Some(body), None).await;
        assert_eq!(status, 200, "{got}");
        assert_eq!(got["data"]["reminder_id"], "rem-1");
        assert_eq!(got["data"]["deduplicated"], true);

        let writes = convex_calls(&fx, CREATE).await;
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(writes[0]["args"]["externalOrgId"], "org-1");
        assert_eq!(writes[0]["args"]["spaceRef"], "room-a");
        assert_eq!(writes[0]["args"]["threadId"], "thread-1");
        assert_eq!(writes[0]["args"]["messageId"], "msg-1");
        assert_eq!(writes[0]["args"]["remindAt"], 1_900_000_000_000_i64);
        assert!(
            writes[0]["args"].get("idempotencyKey").is_none(),
            "the key is accepted and Application deduplicates on the natural key"
        );

        // A reminder on a whole thread carries no message id.
        let (status, _) = call(
            &fx,
            "POST",
            ROOM_REMINDERS,
            Some(json!({"thread_id": "thread-1", "message_id": "  ", "remind_at": 1_900_000_000_000_i64})),
            None,
        )
        .await;
        assert_eq!(status, 200);
        assert!(convex_calls(&fx, CREATE).await[2]["args"].get("messageId").is_none());
    }

    #[tokio::test]
    async fn create_requires_room_membership_and_refuses_zdr() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // A person who is not in the room: 403 before the posture is read and before Application.
        let stranger = fx(Some(true), "").await;
        mount_convex(&stranger, "/api/mutation", CREATE, value(json!({"reminderId": "rem-1"}))).await;
        let (status, got) = call(&stranger, "POST", ROOM_REMINDERS, Some(create_body()), None).await;
        assert_eq!(status, 403, "{got}");
        assert_eq!(got["error"]["code"], "active_membership_required");
        assert!(convex_calls(&stranger, CREATE).await.is_empty());
        assert_eq!(request_count(&stranger.org).await, 0, "the membership is proved before the posture is read");

        // Control refusing the membership read is relayed with its status.
        let refused = fx(Some(false), "viewer").await;
        refused.user_core.reset().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })))
            .mount(&refused.user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/room-a/membership"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": {"code": "room_not_found"}})))
            .mount(&refused.user_core)
            .await;
        let (status, got) = call(&refused, "POST", ROOM_REMINDERS, Some(create_body()), None).await;
        assert_eq!(status, 404, "{got}");
        assert!(convex_calls(&refused, CREATE).await.is_empty());

        // A Zero Data Retention organization: typed 412, nothing written.
        let zdr = fx(Some(true), "editor").await;
        mount_convex(&zdr, "/api/mutation", CREATE, value(json!({"reminderId": "rem-1"}))).await;
        let (status, got) = call(&zdr, "POST", ROOM_REMINDERS, Some(create_body()), None).await;
        assert_eq!(status, 412, "{got}");
        assert_eq!(got["error"]["code"], "zdr_durable_mutation_forbidden");
        assert!(convex_calls(&zdr, CREATE).await.is_empty());

        // An unknown posture fails closed in its own words.
        let unknown = fx(None, "editor").await;
        mount_convex(&unknown, "/api/mutation", CREATE, value(json!({"reminderId": "rem-1"}))).await;
        let (status, got) = call(&unknown, "POST", ROOM_REMINDERS, Some(create_body()), None).await;
        assert_eq!(status, 503, "{got}");
        assert_eq!(got["error"]["code"], "zdr_decision_unavailable");
        assert!(convex_calls(&unknown, CREATE).await.is_empty());

        // A request-level ZDR header alone refuses the write, even for an org that is not ZDR.
        let open = fx(Some(false), "editor").await;
        mount_convex(&open, "/api/mutation", CREATE, value(json!({"reminderId": "rem-1", "remindAt": 5}))).await;
        let (status, got) =
            call(&open, "POST", ROOM_REMINDERS, Some(create_body()), Some(("x-zdr", "true"))).await;
        assert_eq!(status, 412, "{got}");
        assert!(convex_calls(&open, CREATE).await.is_empty());
        let (status, got) = call(&open, "POST", ROOM_REMINDERS, Some(create_body()), None).await;
        assert_eq!(status, 201, "{got}");
        assert_eq!(convex_calls(&open, CREATE).await.len(), 1);
    }

    #[tokio::test]
    async fn create_maps_convex_codes_and_refuses_a_bad_request_before_any_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (message, want_status, want_code) in [
            ("Uncaught Error: invalid_reminder_time", 400, "invalid_reminder_time"),
            ("Uncaught Error: too_many_reminders", 429, "too_many_reminders"),
            ("Uncaught Error: space_not_found", 404, "space_not_found"),
            ("Uncaught Error: invalid_space_ref", 400, "invalid_space"),
            ("Uncaught Error: invalid_thread_id", 400, "invalid_thread"),
            ("Uncaught Error: invalid_message_id", 400, "invalid_message"),
            ("Uncaught Error: the stack trace of something else", 503, "reminders_unavailable"),
        ] {
            let fx = fx(Some(false), "editor").await;
            mount_convex(&fx, "/api/mutation", CREATE, convex_error(message)).await;
            let (status, got) = call(&fx, "POST", ROOM_REMINDERS, Some(create_body()), None).await;
            assert_eq!(status, want_status, "{message}: {got}");
            assert_eq!(got["error"]["code"], want_code, "{message}");
            assert!(!got.to_string().contains("stack trace"), "no raw Convex text reaches the browser");
            if want_code == "too_many_reminders" {
                assert_eq!(got["error"]["details"]["max_pending"], 50);
            }
        }
        // An Application outage is a 503 that says nothing was changed.
        let down = fx(Some(false), "editor").await;
        mount_convex(&down, "/api/mutation", CREATE, ResponseTemplate::new(500)).await;
        let (status, got) = call(&down, "POST", ROOM_REMINDERS, Some(create_body()), None).await;
        assert_eq!(status, 503);
        assert_eq!(got["error"]["code"], "reminders_unavailable");

        // Shape problems are refused before Control, org-core and Application are asked at all.
        let fx = fx(Some(false), "editor").await;
        mount_convex(&fx, "/api/mutation", CREATE, value(json!({"reminderId": "rem-1"}))).await;
        for (uri, body, code) in [
            (ROOM_REMINDERS, json!({"thread_id": "thread-1"}), "invalid_request"),
            (ROOM_REMINDERS, json!({"thread_id": "thread-1", "remind_at": "soon"}), "invalid_request"),
            (
                ROOM_REMINDERS,
                json!({"thread_id": "thread-1", "remind_at": 1, "org_id": "forged"}),
                "invalid_request",
            ),
            (
                ROOM_REMINDERS,
                json!({"thread_id": "thread-1", "remind_at": 1, "user_id": "forged"}),
                "invalid_request",
            ),
            (ROOM_REMINDERS, json!({"thread_id": "  ", "remind_at": 1}), "invalid_thread"),
            (
                ROOM_REMINDERS,
                json!({"thread_id": "thread-1", "message_id": "m\u{7}1", "remind_at": 1}),
                "invalid_message",
            ),
            ("/api/v1/spaces/%20/reminders", create_body(), "invalid_space"),
        ] {
            let (status, got) = call(&fx, "POST", uri, Some(body.clone()), None).await;
            assert_eq!(status, 400, "{uri} {body}");
            assert_eq!(got["error"]["code"], code, "{uri} {body}");
        }
        assert_eq!(membership_calls(&fx).await, 0, "no refused request reached Control");
        assert_eq!(request_count(&fx.org).await, 0);
        assert!(convex_calls(&fx, CREATE).await.is_empty());
    }

    // ---- list and cancel -------------------------------------------------------------------------

    #[tokio::test]
    async fn list_pending_only_and_cancel() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx(Some(false), "viewer").await;
        mount_convex(
            &fx,
            "/api/query",
            LIST,
            value(json!([
                {"reminderId": "rem-1", "spaceRef": "room-a", "threadId": "thread-1", "messageId": "msg-1",
                 "remindAt": 1_900_000_000_000_i64, "state": "pending", "createdAt": 1_800_000_000_000_i64,
                 "externalAuthId": "leak"},
                {"reminderId": "rem-2", "spaceRef": "room-a", "threadId": "thread-2", "remindAt": 1_900_000_100_000_i64,
                 "state": "pending"}
            ])),
        )
        .await;
        for uri in ["/api/v1/spaces/reminders?state=pending", "/api/v1/spaces/reminders"] {
            let (status, got) = call(&fx, "GET", uri, None, None).await;
            assert_eq!(status, 200, "{uri}: {got}");
            let reminders = got["data"]["reminders"].as_array().unwrap();
            assert_eq!(reminders.len(), 2);
            assert_eq!(
                reminders[0],
                json!({
                    "reminder_id": "rem-1", "space_ref": "room-a", "thread_id": "thread-1", "message_id": "msg-1",
                    "remind_at": 1_900_000_000_000_i64, "state": "pending", "created_at": 1_800_000_000_000_i64
                })
            );
            assert!(reminders[1]["message_id"].is_null());
            assert!(!got.to_string().contains("leak"), "only the wire fields of a row are shown");
        }
        let asked = convex_calls(&fx, LIST).await;
        assert_eq!(asked[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(asked[0]["args"]["externalOrgId"], "org-1");
        for word in ["fired", "cancelled", ""] {
            let (status, got) =
                call(&fx, "GET", &format!("/api/v1/spaces/reminders?state={word}"), None, None).await;
            assert_eq!(status, 400, "{word}");
            assert_eq!(got["error"]["code"], "invalid_state");
        }
        assert_eq!(convex_calls(&fx, LIST).await.len(), 2, "a refused state never reached Application");

        // Cancelling: the caller's own pending row, then the same answer for a foreign, unknown or
        // already-fired id.
        mount_convex(&fx, "/api/mutation", CANCEL, value(json!({"reminderId": "rem-1"}))).await;
        let (status, got) = call(&fx, "DELETE", "/api/v1/spaces/reminders/rem-1", None, None).await;
        assert_eq!(status, 200, "{got}");
        assert_eq!(got["data"], json!({"reminder_id": "rem-1", "cancelled": true}));
        let cancels = convex_calls(&fx, CANCEL).await;
        assert_eq!(cancels[0]["args"]["reminderId"], "rem-1");
        assert_eq!(cancels[0]["args"]["externalAuthId"], "user-1");
        assert_eq!(cancels[0]["args"]["externalOrgId"], "org-1");
        let gone = self::fx(Some(false), "viewer").await;
        mount_convex(&gone, "/api/mutation", CANCEL, convex_error("Uncaught Error: reminder_not_found")).await;
        for id in ["someone-elses", "unknown", "fired-1"] {
            let (status, got) =
                call(&gone, "DELETE", &format!("/api/v1/spaces/reminders/{id}"), None, None).await;
            assert_eq!(status, 404, "{id}");
            assert_eq!(got["error"]["code"], "reminder_not_found", "{id}");
        }
        // An id that cannot be one is the same 404 and never reaches Application.
        let (status, got) = call(&gone, "DELETE", "/api/v1/spaces/reminders/%07", None, None).await;
        assert_eq!(status, 404);
        assert_eq!(got["error"]["code"], "reminder_not_found");
        assert_eq!(convex_calls(&gone, CANCEL).await.len(), 3);
        // Any other failure is a 503 that says nothing changed.
        let down = self::fx(Some(false), "viewer").await;
        mount_convex(&down, "/api/mutation", CANCEL, ResponseTemplate::new(500)).await;
        let (status, got) = call(&down, "DELETE", "/api/v1/spaces/reminders/rem-1", None, None).await;
        assert_eq!(status, 503);
        assert_eq!(got["error"]["code"], "reminders_unavailable");
        // Application failing on the list is the same 503.
        mount_convex(&down, "/api/query", LIST, ResponseTemplate::new(500)).await;
        let (status, got) = call(&down, "GET", "/api/v1/spaces/reminders", None, None).await;
        assert_eq!(status, 503);
        assert_eq!(got["error"]["code"], "reminders_unavailable");

        // Cancelling removes data, so a Zero Data Retention organization may do it, and the
        // posture is not read for it.
        let zdr = self::fx(Some(true), "viewer").await;
        mount_convex(&zdr, "/api/mutation", CANCEL, value(json!({"reminderId": "rem-1"}))).await;
        let (status, got) = call(&zdr, "DELETE", "/api/v1/spaces/reminders/rem-1", None, None).await;
        assert_eq!(status, 200, "{got}");
        assert_eq!(request_count(&zdr.org).await, 0);
    }

    #[tokio::test]
    async fn flag_off_gives_404_space_reminders_disabled_on_all_three_routes() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fx(Some(false), "editor").await;
        fx.state.space_features.reminders_api = false;
        for (method, uri, body) in [
            ("POST", ROOM_REMINDERS, Some(create_body())),
            ("GET", "/api/v1/spaces/reminders?state=pending", None),
            ("DELETE", "/api/v1/spaces/reminders/rem-1", None),
        ] {
            let (status, got) = call(&fx, method, uri, body, None).await;
            assert_eq!(status, 404, "{method} {uri}");
            assert_eq!(got["error"]["code"], "space_reminders_disabled", "{method} {uri}");
        }
        assert_eq!(request_count(&fx.application).await, 0);
        assert_eq!(request_count(&fx.org).await, 0);
        assert!(
            fx.user_core
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.url.path() == "/api/v1/me/session-context"),
            "Control is asked only for the session context"
        );
    }
}
