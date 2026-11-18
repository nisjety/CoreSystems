//! WS17-S8: the per-turn personal-memory opt-in, gateway side (build plan WS17-S8; design WS17 D6).
//!
//! A room turn may use the signed-in person's own personal memory for that one answer, and only when the person asked
//! on this turn. The browser sends `use_personal_memory: true` in the chat body. Behind
//! [`super::inject_personal_thread_context_for_wake`], this module turns that request into a Control decision:
//!
//! - the request flag is removed from the body on every turn, and only an exact `true` asks;
//! - a turn with no Space context, or a room turn with no existing thread, gets nothing: there is no room thread to
//!   recall into;
//! - the rollout flag `SPACE_PERSONAL_MEMORY_TURN_API` off answers nothing, so the turn runs without personal memory;
//! - Zero Data Retention refuses the turn with the typed 412 `zdr_durable_mutation_forbidden` before any Control call;
//! - Control's `memory-personal-context-decision` (scope `spaces:issue`, under the caller's own delegation) mints the
//!   per-turn decision: a refusal is 403 `personal_memory_not_authorized`, an outage or unusable answer is 503
//!   `space_decision_unavailable`;
//! - the decision's token and ref go into the turn's `space_context` as `personal_memory_turn`, which model-gateway
//!   forwards to Session Core. The browser never supplies that field (it is a forged authority field).
//!
//! This module stores nothing and decides nothing about authority. Control decides who may use personal memory in
//! this room; Session Core verifies the decision, the owner and the thread again before it returns a single row.

use axum::{http::StatusCode, Json};
use reqwest::Method;
use serde_json::{json, Value};

use super::denial;
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::proxy_json,
    zdr::durable_write_refusal,
};

/// The body flag the browser sends. Only an exact `true` asks; any other value is ignored.
const REQUEST_FIELD: &str = "use_personal_memory";
/// The field, inside `space_context`, that carries the minted decision to model-gateway.
const TURN_FIELD: &str = "personal_memory_turn";
/// The one fixed refusal message for every refusal of the decision.
const DENIED: &str = "Personal memory use in this Space is not authorized";

type Reply = (StatusCode, Json<Value>);

/// Adds the per-turn personal-memory decision to a room turn's `space_context` when the person asked for it.
/// The request flag is always removed. `Ok` means the turn may go on, with or without personal memory; `Err` is a
/// typed refusal the turn returns unchanged.
pub(super) async fn inject_for_turn(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    body: &mut Value,
) -> Result<(), Reply> {
    let Some(object) = body.as_object_mut() else {
        return Ok(());
    };
    let asked = object.remove(REQUEST_FIELD).and_then(|value| value.as_bool()) == Some(true);
    if !asked || !state.space_features.space_personal_memory_turn_api {
        return Ok(());
    }
    let space_ref = object
        .get("space_context")
        .and_then(|context| context.get("space_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let thread_id = object
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let (Some(space_ref), Some(thread_id)) = (space_ref, thread_id) else {
        return Ok(());
    };
    // Zero Data Retention first, before any Control call: no durable personal memory exists under ZDR.
    if let Some(refusal) = durable_write_refusal(state, user, None, None).await {
        return Err(refusal);
    }
    let (decision_ref, token) = mint(state, user, org_id, &space_ref, &thread_id).await?;
    if let Some(context) = object
        .get_mut("space_context")
        .and_then(Value::as_object_mut)
    {
        context.insert(
            TURN_FIELD.to_owned(),
            json!({ "decision_ref": decision_ref, "decision_token": token }),
        );
    }
    Ok(())
}

/// Asks Control for one per-turn decision under the caller's own delegation. A refusal (403, or a 404 from a Control
/// without the endpoint, read as a refusal as the sibling decisions do) is 403 `personal_memory_not_authorized`, typed
/// when Control gave a reason. An outage, a 5xx or an unusable answer is 503 `space_decision_unavailable`. Nothing here
/// grants. The idempotency key is random per turn, so a retry of this turn never reuses another turn's decision.
async fn mint(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    thread_id: &str,
) -> Result<(String, String), Reply> {
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!(
        "{}/api/v1/internal/spaces/memory-personal-context-decision",
        state.user_core_url
    );
    let (status, Json(response)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_ref": space_ref,
            "thread_id": thread_id,
            "idempotency_key": format!("space-personal-memory-{:016x}", rand::random::<u64>()),
        })),
        Some(org_id),
        Some(&actor),
        None,
    )
    .await;
    let status = denial::refusal_status(status, &response);
    if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
        let body = match denial::SpaceDenial::from_response(state, status, &response) {
            Some(denial) => error_with_details(
                "personal_memory_not_authorized",
                DENIED,
                denial.details_value(),
            ),
            None => error("personal_memory_not_authorized", DENIED),
        };
        return Err((StatusCode::FORBIDDEN, Json(body)));
    }
    if !status.is_success() {
        return Err(decision_unavailable());
    }
    let data = unwrap_data(&response);
    let decision_ref = data
        .get("decision")
        .and_then(Value::as_object)
        .and_then(|decision| decision.get("decision_ref"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if decision_ref.is_empty() || token.is_empty() {
        return Err(decision_unavailable());
    }
    Ok((decision_ref, token))
}

fn decision_unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "space_decision_unavailable",
            "The Space decision is unavailable.",
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use axum::http::StatusCode;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    /// A room turn that asked for personal memory with `asked` as its `use_personal_memory` value.
    fn room_turn(asked: Value) -> Value {
        json!({
            "content": "hi",
            "use_personal_memory": asked,
            "thread_id": "thread-1",
            "space_context": {"space_id": "room-1", "space_decision_token": "room-token"}
        })
    }

    fn mint_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "decision": {"decision_ref": "pm-ref-1"},
            "token": "pm-token-1"
        }}))
    }

    /// The doubles a turn reaches: org-core's posture (`zdr`) and Control's per-turn mint (`mint`).
    struct Doubles {
        state: AppState,
        user_core: MockServer,
        _org: MockServer,
    }

    async fn doubles(flag: bool, zdr: bool, mint: ResponseTemplate) -> Doubles {
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}
            })))
            .mount(&org)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/internal/spaces/memory-personal-context-decision"))
            .respond_with(mint)
            .mount(&user_core)
            .await;
        let mut state = crate::tests::test_state(false);
        state.org_core_url = org.uri();
        state.user_core_url = user_core.uri();
        state.space_features.space_personal_memory_turn_api = flag;
        Doubles {
            state,
            user_core,
            _org: org,
        }
    }

    async fn calls(server: &MockServer) -> usize {
        server.received_requests().await.unwrap_or_default().len()
    }

    #[tokio::test]
    async fn flag_off_removes_the_request_flag_and_mints_nothing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let d = doubles(false, false, mint_ok()).await;
        let mut body = room_turn(json!(true));
        inject_for_turn(&d.state, &user(), "org-1", &mut body)
            .await
            .expect("the turn goes on without personal memory");
        assert!(body.get("use_personal_memory").is_none());
        assert!(body["space_context"].get("personal_memory_turn").is_none());
        assert_eq!(calls(&d.user_core).await, 0);
    }

    #[tokio::test]
    async fn only_an_exact_true_asks_and_a_non_room_turn_mints_nothing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let d = doubles(true, false, mint_ok()).await;
        // Any value other than the boolean true is removed and does not ask.
        let mut loose = room_turn(json!("true"));
        inject_for_turn(&d.state, &user(), "org-1", &mut loose)
            .await
            .expect("the turn goes on");
        assert!(loose.get("use_personal_memory").is_none());
        assert!(loose["space_context"].get("personal_memory_turn").is_none());
        // A personal turn has no Space context to recall into.
        let mut personal = json!({"content": "hi", "use_personal_memory": true});
        inject_for_turn(&d.state, &user(), "org-1", &mut personal)
            .await
            .expect("the turn goes on");
        assert_eq!(personal, json!({"content": "hi"}));
        // A Space turn with no existing thread has no room thread to recall into.
        let mut new_thread = json!({
            "content": "hi",
            "use_personal_memory": true,
            "space_context": {"space_id": "room-1"}
        });
        inject_for_turn(&d.state, &user(), "org-1", &mut new_thread)
            .await
            .expect("the turn goes on");
        assert!(new_thread.get("use_personal_memory").is_none());
        assert!(new_thread["space_context"].get("personal_memory_turn").is_none());
        assert_eq!(calls(&d.user_core).await, 0);
    }

    #[tokio::test]
    async fn zdr_is_a_typed_412_before_any_control_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let d = doubles(true, true, mint_ok()).await;
        let mut body = room_turn(json!(true));
        let (status, Json(refusal)) = inject_for_turn(&d.state, &user(), "org-1", &mut body)
            .await
            .expect_err("a ZDR org gets no personal memory");
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(refusal["error"]["code"], "zdr_durable_mutation_forbidden");
        assert_eq!(calls(&d.user_core).await, 0);
    }

    #[tokio::test]
    async fn a_control_refusal_maps_to_personal_memory_not_authorized() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let d = doubles(
            true,
            false,
            ResponseTemplate::new(403).set_body_json(json!({"error": "upstream wording"})),
        )
        .await;
        let mut body = room_turn(json!(true));
        let (status, Json(refusal)) = inject_for_turn(&d.state, &user(), "org-1", &mut body)
            .await
            .expect_err("Control refused the mint");
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(refusal["error"]["code"], "personal_memory_not_authorized");
        assert!(
            !refusal.to_string().contains("upstream wording"),
            "upstream text never reaches the browser"
        );
        assert!(body["space_context"].get("personal_memory_turn").is_none());
        assert_eq!(calls(&d.user_core).await, 1);
    }

    #[tokio::test]
    async fn success_injects_personal_memory_turn_into_space_context() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let d = doubles(true, false, mint_ok()).await;
        let mut body = room_turn(json!(true));
        inject_for_turn(&d.state, &user(), "org-1", &mut body)
            .await
            .expect("the decision is minted");
        assert!(body.get("use_personal_memory").is_none());
        assert_eq!(
            body["space_context"]["personal_memory_turn"],
            json!({"decision_ref": "pm-ref-1", "decision_token": "pm-token-1"})
        );
        let sent = d.user_core.received_requests().await.unwrap_or_default();
        assert_eq!(sent.len(), 1);
        let sent: Value = serde_json::from_slice(&sent[0].body).expect("a JSON mint request");
        assert_eq!(sent["space_ref"], "room-1");
        assert_eq!(sent["thread_id"], "thread-1");
        assert!(sent["idempotency_key"]
            .as_str()
            .unwrap_or_default()
            .starts_with("space-personal-memory-"));
    }

    #[tokio::test]
    async fn a_browser_supplied_personal_memory_turn_is_stripped() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let d = doubles(true, false, mint_ok()).await;
        // The browser names the minted field at the top level and inside its own space_context. The injector that runs
        // before this step removes both, and with no Space reference left nothing is minted.
        let mut body = json!({
            "content": "hi",
            "use_personal_memory": true,
            "thread_id": "thread-1",
            "personal_memory_turn": {"decision_ref": "forged", "decision_token": "forged"},
            "space_context": {
                "space_id": "room-1",
                "personal_memory_turn": {"decision_ref": "forged", "decision_token": "forged"}
            }
        });
        crate::domains::spaces::inject_personal_thread_context_for_wake(
            &d.state,
            &user(),
            "org-1",
            &mut body,
            None,
        )
        .await
        .expect("the turn goes on");
        assert!(body.get("personal_memory_turn").is_none(), "a forged top-level decision is stripped");
        assert!(body.get("space_context").is_none(), "a forged space_context is stripped with its decision");
        assert!(!body.to_string().contains("forged"));
        assert_eq!(calls(&d.user_core).await, 0);
    }
}
