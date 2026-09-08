//! `/api/v1/orgs/:id/instructions` — ADR-0003's org layer of the
//! authored-instruction hierarchy.
//!
//! Storage is Convex (`organizations.instructions`), not org-core — a
//! different backend than `settings.rs`'s ZDR/support-AI posture, which is
//! why this is a separate route rather than a third field on that endpoint.
//! Reading is open to any active org member (the instructions shape every
//! member's chat turns); writing is gated to owner/admin, mirroring the
//! existing org-skill-authoring surface (`agent_actions.rs::can_author_skills`).

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

use super::shared::{require_active_org, require_org_admin, GatewayJsonResponse};

const MAX_ORG_INSTRUCTIONS_LENGTH: usize = 4000;

pub(super) async fn get_org_instructions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_active_org(&user, &id) {
        return response;
    }
    let instructions = crate::domains::spaces::convex_gateway_call(
        &state,
        "query",
        "organizations:instructionsForGateway",
        json!({ "externalOrgId": id }),
    )
    .await
    .ok()
    .and_then(|value| {
        value
            .get("instructions")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    (
        StatusCode::OK,
        Json(json!({"data": {"instructions": instructions}})),
    )
}

#[derive(Deserialize)]
pub(crate) struct UpdateOrgInstructionsRequest {
    #[serde(default)]
    pub(crate) instructions: Option<String>,
}

pub(crate) async fn update_org_instructions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<UpdateOrgInstructionsRequest>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    let instructions = body
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    // Character count, not byte length: Convex (`instructions?.length`, UTF-16
    // code units) and the browser's `<textarea maxlength>` both count
    // characters, and this product's Norwegian text is full of 2-byte-UTF-8
    // æ/ø/å — a byte-length check here would reject valid input those two
    // already accepted.
    if instructions.is_some_and(|value| value.chars().count() > MAX_ORG_INSTRUCTIONS_LENGTH) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "instructions_too_long",
                "Organization instructions must be 4000 characters or fewer.",
            )),
        );
    }
    let result = crate::domains::spaces::convex_gateway_call(
        &state,
        "mutation",
        "organizations:setInstructionsForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": id,
            "instructions": instructions,
        }),
    )
    .await;
    match result {
        Ok(value) => (StatusCode::OK, Json(json!({"data": value}))),
        Err(()) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "instructions_unavailable",
                "Organization instructions could not be saved.",
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{
        body_partial_json as wm_body_partial_json, method as wm_method, path as wm_path,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// `org_role` is the caller's `/api/v1/me/session-context` role — the
    /// authoritative org-admin gate (`require_org_admin`) reads
    /// `authorized_membership.role`, populated live from this endpoint by
    /// `require_session`'s middleware, never from a client-supplied header.
    async fn org_instructions_response(
        org_role: &str,
        method: &str,
        write_body: Option<Value>,
    ) -> (u16, Value) {
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
                "userId":"user-1", "orgId":"org-1", "role": org_role, "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "organizations:instructionsForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"instructions": "Existing org rule."}
            })))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .and(wm_body_partial_json(
                json!({"path": "organizations:setInstructionsForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": {"instructions": "New org rule."}
            })))
            .mount(&application)
            .await;
        let mut state = crate::tests::test_state(false);
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();

        let request = Request::builder()
            .method(method)
            .uri("/api/v1/orgs/org-1/instructions")
            .header("cookie", "better-auth.session_token=session-1");
        let request = if let Some(body) = write_body {
            request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        } else {
            request.body(Body::empty()).unwrap()
        };
        let response = crate::build_router(state).oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let parsed: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed)
    }

    #[tokio::test]
    async fn a_plain_member_can_read_org_instructions() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = org_instructions_response("member", "GET", None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["instructions"], "Existing org rule.");
    }

    #[tokio::test]
    async fn a_plain_member_cannot_write_org_instructions() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = org_instructions_response(
            "member",
            "PATCH",
            Some(json!({"instructions": "New org rule."})),
        )
        .await;
        assert_eq!(status, 403, "{body}");
    }

    #[tokio::test]
    async fn an_admin_can_write_org_instructions() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = org_instructions_response(
            "admin",
            "PATCH",
            Some(json!({"instructions": "New org rule."})),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["instructions"], "New org rule.");
    }

    #[tokio::test]
    async fn overlong_org_instructions_are_rejected_before_reaching_convex() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = org_instructions_response(
            "admin",
            "PATCH",
            Some(json!({"instructions": "x".repeat(4001)})),
        )
        .await;
        assert_eq!(status, 400, "{body}");
    }

    /// 'æ' is 2 bytes in UTF-8. 4000 of them is exactly at the character
    /// limit but 8000 bytes — a byte-length check would wrongly reject this
    /// valid Norwegian text.
    #[tokio::test]
    async fn four_thousand_norwegian_characters_are_accepted() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = org_instructions_response(
            "admin",
            "PATCH",
            Some(json!({"instructions": "æ".repeat(4000)})),
        )
        .await;
        assert_eq!(status, 200, "{body}");
    }
}
