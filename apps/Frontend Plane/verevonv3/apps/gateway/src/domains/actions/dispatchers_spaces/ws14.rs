//! WS14-S7: action dispatchers for room files (`space.files.*`). `list`, `read_text` and `delete`
//! ride the same core functions as routes R3 to R5 in `domains/spaces/files.rs`, so the flag, the
//! Control decisions and the typed errors exist once. `attach` is human-only multipart: a JSON
//! dispatch cannot carry bytes and answers 422 `upload_requires_multipart`. None of the four ids
//! is model-executable (model-access.ts closes all of them).

use axum::{
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde_json::Value;

use crate::config::AppState;
use crate::domains::spaces::files;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_space_files_attach() -> Response {
    files::attach_requires_multipart().into_response()
}

pub(in crate::domains::actions) async fn dispatch_space_files_list(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("space.files.list requires a non-empty 'spaceRef'");
    }
    let limit = input
        .get("limit")
        .and_then(Value::as_i64)
        .map(|n| n.to_string());
    let cursor = ticket_string(input, "cursor");
    let cursor = (!cursor.is_empty()).then_some(cursor);
    let (status, axum::Json(resp)) =
        match files::list_core(state, user, headers, space_ref, limit.as_deref(), cursor).await {
            Ok(reply) | Err(reply) => reply,
        };
    owner_json_response_to_envelope("space.files.list", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_space_files_read_text(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let document_id = ticket_string(input, "documentId");
    if space_ref.is_empty() || document_id.is_empty() {
        return ticket_bad_request(
            "space.files.read_text requires a non-empty 'spaceRef' and 'documentId'",
        );
    }
    let max_chars = input
        .get("maxChars")
        .and_then(Value::as_u64)
        .map(|n| n.to_string());
    let (status, axum::Json(resp)) = match files::read_text_core(
        state,
        user,
        headers,
        space_ref,
        document_id,
        max_chars.as_deref(),
    )
    .await
    {
        Ok(reply) | Err(reply) => reply,
    };
    owner_json_response_to_envelope("space.files.read_text", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_space_files_delete(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let document_id = ticket_string(input, "documentId");
    if space_ref.is_empty() || document_id.is_empty() {
        return ticket_bad_request(
            "space.files.delete requires a non-empty 'spaceRef' and 'documentId'",
        );
    }
    let (status, axum::Json(resp)) =
        match files::delete_core(state, user, headers, space_ref, document_id).await {
            Ok(reply) | Err(reply) => reply,
        };
    owner_json_response_to_envelope("space.files.delete", user, status, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use serde_json::json;

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "ws14-user".to_owned(),
            user_email: "ws14@example.com".to_owned(),
            user_name: "Ws14".to_owned(),
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

    async fn read(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// The default state has `room_files_api` off and every upstream unreachable.
    fn state() -> AppState {
        crate::tests::test_state(false)
    }

    #[tokio::test]
    async fn attach_is_human_only_multipart_422() {
        let (status, body) = read(dispatch_space_files_attach().await).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "upload_requires_multipart");
    }

    #[tokio::test]
    async fn list_read_text_and_delete_require_their_ids() {
        let state = state();
        let headers = HeaderMap::new();
        let response = dispatch_space_files_list(&state, &user(), &headers, &json!({})).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        for input in [json!({ "spaceRef": "space-1" }), json!({ "documentId": "doc-1" })] {
            let response = dispatch_space_files_read_text(&state, &user(), &headers, &input).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{input}");
            let response = dispatch_space_files_delete(&state, &user(), &headers, &input).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{input}");
        }
    }

    #[tokio::test]
    async fn with_the_flag_off_every_dispatch_is_404_room_files_disabled() {
        let state = state();
        let headers = HeaderMap::new();
        let both = json!({ "spaceRef": "space-1", "documentId": "doc-1" });
        let list =
            dispatch_space_files_list(&state, &user(), &headers, &json!({ "spaceRef": "space-1" }))
                .await;
        let text = dispatch_space_files_read_text(&state, &user(), &headers, &both).await;
        let delete = dispatch_space_files_delete(&state, &user(), &headers, &both).await;
        for response in [list, text, delete] {
            let (status, body) = read(response).await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(body["error"]["code"], "room_files_disabled");
        }
    }
}
