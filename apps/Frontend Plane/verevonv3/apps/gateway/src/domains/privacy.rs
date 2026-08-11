//! Privacy / GDPR self-service domain.
//!
//! Lets the authenticated user export (Art. 15 DSAR) or erase (Art. 17) THEIR
//! OWN Control-Plane account data. The target is always the validated session's
//! `user.user_id` — these handlers take NO path/query/body id, so a client cannot
//! aim the operation at another user. user-core additionally enforces caller ==
//! subject (via the forwarded `x-user-id` actor header) before running the
//! `gdpr_*` procs, so the self-scope is defended on both sides.
//!
//! Erasure is irreversible: the gateway refuses with `422 confirmation_required`
//! unless the body carries `{"confirm": true}` (a boolean), before proxying. The
//! DELETE inherits the gateway's global per-identity rate limiter (see
//! `rate_limit.rs`), keyed by the session identity — a tighter erase-specific
//! limit is a possible follow-up but unnecessary today (the operation is
//! self-targeted and one-shot). NOTE: step-up re-auth is NOT yet enforced (here
//! or in user-core); it is a documented Phase-3 follow-up tied to the admin
//! erase-another-user surface — today erasure through the gateway is self-only.

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::{authorized_org_id, proxy_json},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/privacy/export", get(export))
        .route("/api/v1/privacy/erase", delete(erase))
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

/// user-core DSAR export path for a given subject. Built only from the validated
/// session id — never a client-supplied value.
fn export_url(base: &str, user_id: &str) -> String {
    format!(
        "{}/api/v1/users/{}/gdpr/export",
        base,
        urlencoding::encode(user_id)
    )
}

/// user-core hard-erase path for a given subject (see [`export_url`]).
fn erase_url(base: &str, user_id: &str) -> String {
    format!(
        "{}/api/v1/users/{}/gdpr/erase",
        base,
        urlencoding::encode(user_id)
    )
}

/// True only when the body carries a literal `{"confirm": true}` (boolean). A
/// missing body, `{}`, `false`, or the string `"true"` all read as not confirmed.
fn confirm_requested(body: &Option<Value>) -> bool {
    body.as_ref()
        .and_then(|value| value.get("confirm"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

async fn export(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    let org_id = authorized_org_id(&state, &user).await;
    let url = export_url(&state.user_core_url, &user.user_id);
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor_for(&user)),
        None,
    )
    .await
    .into_response()
}

async fn erase(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    body: Option<Json<Value>>,
) -> Response {
    let body = body.map(|Json(value)| value);
    if !confirm_requested(&body) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(error(
                "confirmation_required",
                "Account erasure is irreversible. Resend with {\"confirm\": true} to proceed.",
            )),
        )
            .into_response();
    }
    let org_id = authorized_org_id(&state, &user).await;
    let url = erase_url(&state.user_core_url, &user.user_id);
    let response = proxy_json(
        &state,
        Method::DELETE,
        &url,
        Some(json!({ "confirm": true })),
        Some(&org_id),
        Some(&actor_for(&user)),
        None,
    )
    .await;

    // The Frontend Plane holds its OWN copy of this subject's conversations —
    // the chat-history index and per-thread transcripts in Dragonfly (see
    // `domains::chat::history`) — and it sits outside every erasure path the
    // owning planes run. session-core deletes the messages, events and threads
    // it owns and reports success, while a complete copy of the same turns kept
    // living here for up to 90 days, readable through the transcript endpoint.
    // An erasure attestation that is inaccurate for its retention window is not
    // an erasure, so purge the local copy as part of the same operation.
    //
    // AFTER the upstream call and only on success: erasure is the irreversible
    // step, and dropping the local copy for a request user-core rejected (not
    // confirmed, not permitted, unreachable) would destroy data on a no-op.
    // Markers go too — `PurgeScope::Everything` — because a ZDR marker is itself
    // a record that this person held a conversation under that id, and there is
    // no longer a subject for it to protect.
    let (status, _) = &response;
    if status.is_success() {
        let removed = crate::domains::chat::history::purge_user_history(
            &state,
            &org_id,
            &user.user_id,
            crate::domains::chat::history::PurgeScope::Everything,
        )
        .await;
        tracing::info!(
            removed,
            "erasure: purged the Frontend Plane chat-history copy for the subject"
        );
    }
    response.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_self_scoped_to_the_authenticated_user() {
        // The handlers build the upstream path from the validated session user id
        // ONLY — there is no Path/Query/body id extractor — so a client cannot
        // target another user's data. A spoofed id in the body is structurally
        // ignored: it never reaches the path builder.
        assert_eq!(
            export_url("http://user-core:3012", "user-real"),
            "http://user-core:3012/api/v1/users/user-real/gdpr/export"
        );
        assert_eq!(
            erase_url("http://user-core:3012", "user-real"),
            "http://user-core:3012/api/v1/users/user-real/gdpr/erase"
        );
    }

    #[test]
    fn erase_requires_explicit_boolean_confirm() {
        assert!(!confirm_requested(&None));
        assert!(!confirm_requested(&Some(json!({}))));
        assert!(!confirm_requested(&Some(json!({ "confirm": false }))));
        assert!(!confirm_requested(&Some(json!({ "confirm": "true" })))); // string, not bool
        assert!(confirm_requested(&Some(json!({ "confirm": true }))));
    }
}
