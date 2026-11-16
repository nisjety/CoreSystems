//! WS17-S6: the owner's private panel, the gateway side.
//!
//! Routes (merged by `module_routes()` in `spaces/mod.rs`, behind the session):
//! `GET /api/v1/spaces/{space_ref}/private-panel` reads the caller's panel pointer for one room and
//! `POST` opens it (get or create). The action `spaces.open_private_panel` runs the same open core
//! (`dispatchers_spaces/ws17.rs`), so the flag, the ZDR refusal, the membership check and the typed
//! errors exist once.
//!
//! The gateway stores nothing. The pointer (ids only: the panel's thread id and the agent subject) lives
//! in Convex (`convex/spacePrivatePanels.ts`, read through `getSpacePrivatePanelForGateway`). Room
//! membership is Control's fact (`/api/v1/internal/spaces/{ref}/membership`) under the caller's own
//! delegation, and it is checked before any Convex call. Nothing here reads a message, and the browser
//! never holds a token. No flag is an authorization input.
//!
//! Creation is refused in this build (see [`gated`]). The panel is an owner thread with
//! `sourceScope:'conversation'` (SD-22 v1), and no create path locks that scope when a thread is
//! created: `CreateThread` ignores its metadata, `threads.source_scope` defaults to `workspace`, and only
//! the first user message locks it. A thread opened here would be a workspace-scope panel, so a room
//! with no pointer answers 501 and writes nothing.
//!
//! Order of refusals on every route (each stops the next): flag (404 `feature_disabled`), organization
//! (403 `active_membership_required`), Space reference (400 `invalid_space`), Zero Data Retention (typed
//! 412 `zdr_durable_mutation_forbidden`, before any Control or Convex call), Control membership (403
//! `active_membership_required`; a Control outage is 503 `space_decision_unavailable`), the Convex
//! pointer read (503 `private_panel_unavailable`), then creation (501 `private_panel_creation_unavailable`).
//! Errors are `{error:{code,message,details}}`.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error,
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
    zdr::durable_write_refusal,
};

use super::{control_membership_url, convex_gateway_call, denial};

type Reply = (StatusCode, Json<Value>);

/// Convex's id bound (`requireId`): the gateway refuses a longer Space reference first.
const MAX_SPACE_REF_CHARS: usize = 200;
/// The surface a ZDR refusal names (the catalogue code is feature-less).
const PRIVATE_PANEL_SURFACE: &str = "private_panel";
const POINTER_QUERY: &str = "spacePrivatePanels:getSpacePrivatePanelForGateway";

/// Which half of the surface a request is. Membership is read differently: a read keeps an archived
/// room's own pointer visible, a write needs a live room.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Open,
}

/// The WS17-S6 routes, merged by `module_routes()` in `spaces/mod.rs`.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/{space_ref}/private-panel",
        get(get_private_panel).post(open_private_panel),
    )
}

/// `GET .../private-panel`: the caller's pointer for one room, or `thread_id: null` when there is none.
async fn get_private_panel(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
) -> Reply {
    gated(&state, &user, &headers, &space_ref, Access::Read).await
}

/// `POST .../private-panel`: get or create the caller's pointer for one room.
async fn open_private_panel(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
) -> Reply {
    gated(&state, &user, &headers, &space_ref, Access::Open).await
}

/// `spaces.open_private_panel`: the open core, with the request headers the ZDR check reads.
pub(crate) async fn open_private_panel_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
) -> Reply {
    gated(state, user, headers, space_ref, Access::Open).await
}

async fn gated(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    access: Access,
) -> Reply {
    if !state.space_features.private_panel_ui {
        return feature_disabled();
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return no_membership();
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return invalid_space();
    }
    if let Some((status, Json(body))) = durable_write_refusal(state, user, Some(headers), None).await {
        return label_zdr(status, body);
    }
    if let Err(reply) = membership(state, user, &org_id, space_ref, access == Access::Read).await {
        return reply;
    }
    let pointer = match pointer_for(state, user, &org_id, space_ref).await {
        Ok(pointer) => pointer,
        Err(reply) => return reply,
    };
    match (pointer, access) {
        (Some(pointer), _) => ok(panel_body(space_ref, Some(&pointer))),
        (None, Access::Read) => ok(panel_body(space_ref, None)),
        // No create path locks conversation scope (see the module note), so no thread is made here.
        (None, Access::Open) => creation_unavailable(),
    }
}

/// Control's membership fact for the caller, under their own delegation. A Control refusal is the
/// caller's lack of membership (403); a Control outage is 503. Nothing is decided here.
async fn membership(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    readable: bool,
) -> Result<(), Reply> {
    let url = control_membership_url(state, space_ref, readable);
    let actor = actor_for(user);
    let (status, Json(body)) =
        proxy_json(state, Method::GET, &url, None, Some(org_id), Some(&actor), None).await;
    if status.is_success() {
        return Ok(());
    }
    if denial::refusal_status(status, &body).is_server_error() {
        return Err(decision_unavailable());
    }
    Err(no_membership())
}

/// The caller's pointer row from Convex, or `None` when there is none. Read only after membership.
async fn pointer_for(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<Option<Value>, Reply> {
    let value = convex_gateway_call(
        state,
        "query",
        POINTER_QUERY,
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    .map_err(|()| unavailable())?;
    Ok(value.is_object().then_some(value))
}

/// The wire body both routes answer with: ids only. An absent pointer or field is null.
fn panel_body(space_ref: &str, pointer: Option<&Value>) -> Value {
    let field = |name: &str| {
        pointer
            .and_then(|pointer| pointer.get(name))
            .filter(|value| value.is_string())
            .cloned()
            .unwrap_or(Value::Null)
    };
    json!({
        "space_ref": space_ref,
        "thread_id": field("threadId"),
        "agent_subject_id": field("agentSubjectId"),
    })
}

fn ok(data: Value) -> Reply {
    (StatusCode::OK, Json(json!({ "data": data })))
}

/// The actor the gateway signs for at Control, exactly as the existing room routes do.
fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn valid_space_ref(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_SPACE_REF_CHARS
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '?' | '#' | '\\'))
}

/// The typed ZDR refusal, labelled with the private panel surface.
fn label_zdr(status: StatusCode, mut body: Value) -> Reply {
    if status == StatusCode::PRECONDITION_FAILED {
        if let Some(object) = body.get_mut("error").and_then(Value::as_object_mut) {
            object.insert("details".to_owned(), json!({ "surface": PRIVATE_PANEL_SURFACE }));
        }
    }
    (status, Json(body))
}

fn feature_disabled() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "feature_disabled",
            "The private panel is not enabled in this environment.",
        )),
    )
}

fn no_membership() -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(error(
            "active_membership_required",
            "An active membership is required for this room.",
        )),
    )
}

fn invalid_space() -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error("invalid_space", "A Space reference is required.")),
    )
}

fn decision_unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "space_decision_unavailable",
            "Room authority could not be confirmed right now. Nothing was changed.",
        )),
    )
}

fn unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "private_panel_unavailable",
            "Your private assistant is temporarily unavailable. Try again; nothing was changed.",
        )),
    )
}

fn creation_unavailable() -> Reply {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(error(
            "private_panel_creation_unavailable",
            "Your private assistant for this room cannot be opened yet. Nothing was created.",
        )),
    )
}
