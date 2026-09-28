use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

use super::shared::proxy_for_user;

/// Read-only listing/search operations the composer's connector-attach
/// picker may ask integration-corev2 to run. Every one of these is already
/// catalogued as a read in integration-corev2's own capability gate
/// (`requiredCapabilityForOperation`/`actionRequiresApproval`) — this
/// allowlist is defence in depth on top of that gate, not a substitute for
/// it. This is the FIRST time the browser can reach
/// `/connections/{id}/actions` at all (only execution-core could, server to
/// server, before this); the allowlist means a browser client can never
/// smuggle a write operation through this new surface even if a future
/// catalog change mis-classified one — the string simply never leaves this
/// list to reach integration-corev2.
const READ_ONLY_BROWSE_OPS: &[&str] = &[
    "mail.messages",
    "gmail.messages",
    "drive.files",
    "calendar.events",
];

/// Read-only single-item content-fetch operations for the same picker, once
/// a search result row has been selected.
const READ_ONLY_CONTENT_OPS: &[&str] = &[
    "drive.file.content",
    "microsoft.drive.file.content",
    "google.drive.file.content",
    "mail.messages.get",
    "microsoft.mail.messages.get",
    "google.gmail.messages.get",
    "gmail.messages.get",
];

/// Rejects any operation string not in `allowlist` with a 400 BEFORE any
/// upstream call is attempted. Kept pure and synchronous on purpose: the
/// "a browser client can never reach an unlisted operation" guarantee has to
/// be checkable without a network call, not just true in practice.
fn reject_unless_allowlisted(
    body: &Value,
    allowlist: &[&str],
) -> Option<(StatusCode, Json<Value>)> {
    let operation = body
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if operation.is_empty() {
        return Some((
            StatusCode::BAD_REQUEST,
            Json(error(
                "operation_required",
                "operation is required",
            )),
        ));
    }
    if allowlist.contains(&operation) {
        return None;
    }
    Some((
        StatusCode::BAD_REQUEST,
        Json(error(
            "operation_not_allowed",
            format!("operation '{operation}' is not permitted on this route"),
        )),
    ))
}

/// `POST /api/v1/integrations/connections/{id}/browse` — list/search a
/// connected provider (Files/E-post/Kalender tabs in the picker). Thin
/// passthrough to integration-corev2's existing action executor once the
/// operation clears the allowlist above.
pub(super) async fn browse_connection(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    if let Some(rejection) = reject_unless_allowlisted(&body, READ_ONLY_BROWSE_OPS) {
        return rejection;
    }
    let url = format!(
        "{}/api/v1/connections/{}/actions",
        state.integration_core_url,
        urlencoding::encode(&id)
    );
    proxy_for_user(&state, &user, &headers, Method::POST, &url, Some(body)).await
}

/// `POST /api/v1/integrations/connections/{id}/content` — fetch one picked
/// item's content (file bytes / mail body). Same passthrough shape as
/// `browse_connection`, gated by the sibling content allowlist.
pub(super) async fn content_connection(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    if let Some(rejection) = reject_unless_allowlisted(&body, READ_ONLY_CONTENT_OPS) {
        return rejection;
    }
    let url = format!(
        "{}/api/v1/connections/{}/actions",
        state.integration_core_url,
        urlencoding::encode(&id)
    );
    proxy_for_user(&state, &user, &headers, Method::POST, &url, Some(body)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        matchers::{method as wm_method, path as wm_path},
        Mock, MockServer, ResponseTemplate,
    };

    fn picker_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    #[test]
    fn rejects_an_operation_outside_the_browse_allowlist_before_any_upstream_call() {
        // `mail.send` is a write op in integration-corev2's own catalog —
        // this route must never even attempt to forward it.
        let rejection =
            reject_unless_allowlisted(&json!({"operation": "mail.send"}), READ_ONLY_BROWSE_OPS);
        let (status, Json(body)) = rejection.expect("mail.send must be rejected");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "operation_not_allowed");
    }

    #[test]
    fn rejects_a_browse_op_on_the_content_route_and_vice_versa() {
        // The two allowlists are disjoint by design: a listing operation is
        // not a valid content fetch and shouldn't silently work as one.
        assert!(
            reject_unless_allowlisted(&json!({"operation": "drive.files"}), READ_ONLY_CONTENT_OPS)
                .is_some(),
            "drive.files is a browse op, not a content op"
        );
        assert!(
            reject_unless_allowlisted(
                &json!({"operation": "drive.file.content"}),
                READ_ONLY_BROWSE_OPS
            )
            .is_some(),
            "drive.file.content is a content op, not a browse op"
        );
    }

    #[test]
    fn accepts_every_allowlisted_browse_and_content_operation() {
        for op in READ_ONLY_BROWSE_OPS {
            assert!(
                reject_unless_allowlisted(&json!({"operation": op}), READ_ONLY_BROWSE_OPS)
                    .is_none(),
                "{op} should be allowed on the browse route"
            );
        }
        for op in READ_ONLY_CONTENT_OPS {
            assert!(
                reject_unless_allowlisted(&json!({"operation": op}), READ_ONLY_CONTENT_OPS)
                    .is_none(),
                "{op} should be allowed on the content route"
            );
        }
    }

    #[test]
    fn rejects_a_missing_or_blank_operation() {
        for body in [json!({}), json!({"operation": "  "})] {
            let rejection = reject_unless_allowlisted(&body, READ_ONLY_BROWSE_OPS);
            let (status, Json(payload)) = rejection.expect("missing operation must be rejected");
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(payload["error"]["code"], "operation_required");
        }
    }

    #[tokio::test]
    async fn forwards_an_allowlisted_browse_operation_and_passes_the_response_through_unchanged() {
        let auth_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/ingestion/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})))
            .mount(&auth_core)
            .await;

        let integration_core = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/v1/connections/conn-1/actions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "data": {
                    "action": {
                        "providerKey": "microsoft",
                        "operation": "drive.files",
                        "result": {"value": [{"id": "item-1", "name": "Report.docx"}]}
                    }
                }
            })))
            .mount(&integration_core)
            .await;

        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth_core.uri();
        state.integration_core_url = integration_core.uri();

        let (status, Json(body)) = browse_connection(
            State(state),
            Extension(picker_user()),
            HeaderMap::new(),
            Path("conn-1".to_owned()),
            Json(json!({"operation": "drive.files", "params": {"query": "report"}})),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["data"]["action"]["result"]["value"][0]["name"],
            "Report.docx"
        );
    }

    #[tokio::test]
    async fn a_rejected_operation_never_touches_the_upstream_mock() {
        // No Mock is registered on this server at all — the assertion below
        // on received_requests is what actually proves the upstream was
        // never called, not just that the handler returned 400.
        let integration_core = MockServer::start().await;

        let mut state = crate::tests::test_state(false);
        state.integration_core_url = integration_core.uri();

        let (status, Json(body)) = content_connection(
            State(state),
            Extension(picker_user()),
            HeaderMap::new(),
            Path("conn-1".to_owned()),
            Json(json!({"operation": "drive.files"})),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "operation_not_allowed");
        let received = integration_core
            .received_requests()
            .await
            .expect("request recording is enabled by default");
        assert!(
            received.is_empty(),
            "the disallowed operation must never reach integration-corev2, got {received:?}"
        );
    }
}
