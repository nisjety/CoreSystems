//! ShareDialog backend (PR-6, re-pointed by WS14-S4) — proxies documents-api.
//!
//! Backs the verevonv3 ShareDialog: list who a document is shared with, share it
//! with a user (view-only), un-share, and the read-only "shared with me" list.
//!
//! Authority discipline: documents-api OWNS the document, so it is the service
//! that proves the signed-in user owns it before Control writes the grant
//! (`resource_grants` stays the single grant authority). This gateway holds no
//! `authz:*` scope and never calls Control's grant surface: it relays the
//! user's own `aud=data-plane` token (`proxy_data_plane_json`), and the client
//! supplies ONLY the subject (and an optional role, which documents-api refuses
//! unless it is `view`). `org_id` and the granter come from the verified
//! session / token, never from the body.
//!
//! The `ownership.share_document` / `ownership.revoke_document_share` actions
//! call the same two upstream helpers below, so there is exactly one share path.

use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::knowledge::shared::proxy_data_plane_json,
    envelope::error,
    middleware::{require_session, AuthenticatedUser},
    upstream::authorized_org_id,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/documents/{id}/shares",
            get(list_shares).post(create_share),
        )
        .route(
            "/api/v1/documents/{id}/shares/{subject_id}",
            delete(revoke_share),
        )
        .route("/api/v1/shares/shared-with-me", get(shared_with_me))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

#[derive(Deserialize)]
struct CreateShareRequest {
    subject_id: String,
    #[serde(default)]
    role: Option<String>,
}

fn default_error_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "invalid_request",
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::CONFLICT => "conflict",
        StatusCode::SERVICE_UNAVAILABLE => "sharing_unavailable",
        _ => "upstream_error",
    }
}

fn is_safe_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// documents-api answers `{ "error": "message", "code": "stable_code" }`. The
/// browser contract is `{ error: { code, message } }`, so rewrite the failure
/// here. The stable code is relayed only when it is a plain snake_case token;
/// the message is the service's own short text, never an upstream URL.
fn normalize_error(status: StatusCode, body: &Value) -> Value {
    if body.get("error").map(Value::is_object).unwrap_or(false) {
        return body.clone();
    }
    let code = body
        .get("code")
        .and_then(Value::as_str)
        .filter(|c| is_safe_code(c))
        .unwrap_or_else(|| default_error_code(status));
    let message = body
        .get("error")
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty() && m.len() <= 300)
        .unwrap_or("The sharing request could not be completed.");
    json!({ "error": { "code": code, "message": message } })
}

async fn org_for(state: &AppState, user: &AuthenticatedUser) -> Option<String> {
    let org = authorized_org_id(state, user).await;
    (!org.is_empty()).then_some(org)
}

/// Share `doc_id` with `subject_id` through documents-api (owner check there).
/// Shared by the REST route and the `ownership.share_document` action.
pub(crate) async fn share_document_upstream(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    doc_id: &str,
    subject_id: &str,
    role: Option<&str>,
) -> (StatusCode, Value) {
    let org_id = org_for(state, user).await;
    let url = format!(
        "{}/v1/documents/{}/shares",
        state.documents_api_url,
        urlencoding::encode(doc_id)
    );
    let mut payload = json!({ "subject_id": subject_id });
    if let Some(role) = role.map(str::trim).filter(|r| !r.is_empty()) {
        payload["role"] = json!(role);
    }
    let (status, Json(body)) = proxy_data_plane_json(
        state,
        user,
        headers,
        Method::POST,
        &url,
        Some(payload),
        org_id.as_deref(),
        None,
    )
    .await;
    if status.is_success() {
        (status, body)
    } else {
        (status, normalize_error(status, &body))
    }
}

/// Remove the user grant for `subject_id` on `doc_id` through documents-api.
/// Shared by the REST route and the `ownership.revoke_document_share` action.
pub(crate) async fn revoke_document_share_upstream(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    doc_id: &str,
    subject_id: &str,
) -> (StatusCode, Value) {
    let org_id = org_for(state, user).await;
    let url = format!(
        "{}/v1/documents/{}/shares/{}",
        state.documents_api_url,
        urlencoding::encode(doc_id),
        urlencoding::encode(subject_id)
    );
    let (status, Json(body)) = proxy_data_plane_json(
        state,
        user,
        headers,
        Method::DELETE,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await;
    if status.is_success() {
        (status, body)
    } else {
        (status, normalize_error(status, &body))
    }
}

/// GET /api/v1/documents/:id/shares — the ShareDialog "shared with" list. The
/// SPA reads `grants`; documents-api names the same rows `shares`. Space rows
/// carry `subject_type: "space"` and are read-only.
async fn list_shares(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(doc_id): Path<String>,
) -> impl IntoResponse {
    let org_id = org_for(&state, &user).await;
    let url = format!(
        "{}/v1/documents/{}/shares",
        state.documents_api_url,
        urlencoding::encode(&doc_id)
    );
    let (status, Json(body)) = proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(normalize_error(status, &body))).into_response();
    }
    let grants = body.get("shares").cloned().unwrap_or_else(|| json!([]));
    (status, Json(json!({ "grants": grants }))).into_response()
}

/// POST /api/v1/documents/:id/shares — share the document with a user (view).
/// Only `subject_id` (and an optional role documents-api validates) is
/// client-supplied.
async fn create_share(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(doc_id): Path<String>,
    Json(req): Json<CreateShareRequest>,
) -> impl IntoResponse {
    let subject_id = req.subject_id.trim().to_string();
    if subject_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("invalid_request", "subject_id is required")),
        )
            .into_response();
    }
    let (status, body) = share_document_upstream(
        &state,
        &user,
        &headers,
        &doc_id,
        &subject_id,
        req.role.as_deref(),
    )
    .await;
    (status, Json(body)).into_response()
}

/// DELETE /api/v1/documents/:id/shares/:subject_id — un-share (user grants only).
async fn revoke_share(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((doc_id, subject_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let (status, body) =
        revoke_document_share_upstream(&state, &user, &headers, &doc_id, &subject_id).await;
    (status, Json(body)).into_response()
}

/// GET /api/v1/shares/shared-with-me — documents explicitly shared with the
/// viewer. The subject is the verified token's user, never client-supplied.
async fn shared_with_me(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let org_id = org_for(&state, &user).await;
    let url = format!("{}/v1/documents/shared-with-me", state.documents_api_url);
    let (status, Json(body)) = proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        None,
    )
    .await;
    if status.is_success() {
        (status, Json(body)).into_response()
    } else {
        (status, Json(normalize_error(status, &body))).into_response()
    }
}

// WS14-S4: the gateway only relays. These tests pin that the share routes go to
// documents-api with the user's own data-plane token, never to user-core, and
// that documents-api's flat `{error, code}` becomes the browser envelope.
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, response::Response};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const DP_TOKEN: &str = "dp-token-aaaaaaaaaaaa";

    fn caller() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    struct Stack {
        state: AppState,
        auth_core: MockServer,
        documents: MockServer,
        user_core: MockServer,
    }

    async fn stack() -> Stack {
        let auth_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/data-plane/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "token": DP_TOKEN, "expiresInSeconds": 300 })),
            )
            .mount(&auth_core)
            .await;
        let documents = MockServer::start().await;
        let user_core = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth_core.uri();
        state.documents_api_url = documents.uri();
        state.user_core_url = user_core.uri();
        Stack {
            state,
            auth_core,
            documents,
            user_core,
        }
    }

    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("cookie", "session=abc".parse().unwrap());
        headers
    }

    async fn read(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[test]
    fn is_safe_code_accepts_only_short_snake_case_tokens() {
        for ok in ["role_not_supported", "self_share", "a1", "x"] {
            assert!(is_safe_code(ok), "{ok}");
        }
        for bad in [
            "",
            "Role",
            "has space",
            "dash-ed",
            "<script>",
            "../x",
            &"a".repeat(65),
        ] {
            assert!(!is_safe_code(bad), "{bad:?}");
        }
        assert!(is_safe_code(&"a".repeat(64)));
    }

    #[test]
    fn normalize_error_rewrites_the_flat_shape_and_falls_back_for_unsafe_codes() {
        let flat = json!({ "error": "role is not supported for documents", "code": "role_not_supported" });
        assert_eq!(
            normalize_error(StatusCode::BAD_REQUEST, &flat),
            json!({ "error": { "code": "role_not_supported", "message": "role is not supported for documents" } })
        );
        // An unsafe code is replaced by the status default, never relayed.
        let unsafe_code = json!({ "error": "nope", "code": "Bad Code!<x>" });
        assert_eq!(
            normalize_error(StatusCode::CONFLICT, &unsafe_code)["error"]["code"],
            "conflict"
        );
        // No code at all: the status default; no message: the generic text.
        let bare = json!({});
        let out = normalize_error(StatusCode::NOT_FOUND, &bare);
        assert_eq!(out["error"]["code"], "not_found");
        assert_eq!(out["error"]["message"], "The sharing request could not be completed.");
        assert_eq!(
            normalize_error(StatusCode::SERVICE_UNAVAILABLE, &bare)["error"]["code"],
            "sharing_unavailable"
        );
        assert_eq!(
            normalize_error(StatusCode::FORBIDDEN, &bare)["error"]["code"],
            "forbidden"
        );
        assert_eq!(
            normalize_error(StatusCode::BAD_GATEWAY, &bare)["error"]["code"],
            "upstream_error"
        );
        // An over-long message is dropped, not relayed.
        let long = json!({ "error": "x".repeat(301), "code": "self_share" });
        assert_eq!(
            normalize_error(StatusCode::CONFLICT, &long)["error"]["message"],
            "The sharing request could not be completed."
        );
        // Already the browser envelope: untouched.
        let envelope = json!({ "error": { "code": "custom", "message": "m" } });
        assert_eq!(normalize_error(StatusCode::BAD_REQUEST, &envelope), envelope);
    }

    #[tokio::test]
    async fn list_proxies_to_documents_api_with_the_data_plane_bearer_and_maps_shares_to_grants() {
        let s = stack().await;
        Mock::given(method("GET"))
            .and(path("/v1/documents/doc-1/shares"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "shares": [
                    { "subject_type": "user", "subject_id": "user-2", "role": "view" },
                    { "subject_type": "space", "subject_id": "space:org-1:r", "role": "view" }
                ]
            })))
            .mount(&s.documents)
            .await;

        let response = list_shares(
            State(s.state.clone()),
            Extension(caller()),
            headers(),
            Path("doc-1".to_owned()),
        )
        .await
        .into_response();
        let (status, body) = read(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["grants"].as_array().unwrap().len(), 2);
        assert_eq!(body["grants"][1]["subject_type"], "space");
        assert!(body.get("shares").is_none(), "the SPA reads `grants`");

        let upstream = s.documents.received_requests().await.unwrap();
        assert_eq!(upstream.len(), 1);
        assert_eq!(
            upstream[0].headers.get("authorization").unwrap().to_str().unwrap(),
            format!("Bearer {DP_TOKEN}")
        );
        assert_eq!(upstream[0].headers.get("x-org-id").unwrap().to_str().unwrap(), "org-1");
        assert!(
            s.user_core.received_requests().await.unwrap().is_empty(),
            "the gateway must never call user-core's grant surface"
        );
        let minted = s.auth_core.received_requests().await.unwrap();
        assert_eq!(minted.len(), 1, "one data-plane token minted from the session");
        assert_eq!(minted[0].headers.get("cookie").unwrap().to_str().unwrap(), "session=abc");
    }

    #[tokio::test]
    async fn list_rewrites_an_upstream_failure_and_a_missing_token_is_a_503_without_a_documents_call() {
        let s = stack().await;
        Mock::given(method("GET"))
            .and(path("/v1/documents/doc-2/shares"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({ "error": "document not found" })))
            .mount(&s.documents)
            .await;
        let (status, body) = read(
            list_shares(State(s.state.clone()), Extension(caller()), headers(), Path("doc-2".to_owned()))
                .await
                .into_response(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["message"], "document not found");

        // Auth Core cannot mint: fail closed, never fall back to a shared key.
        let mut broken = stack().await;
        broken.auth_core.reset().await;
        let dead = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&dead)
            .await;
        broken.state.auth_core_url = dead.uri();
        let (status, body) = read(
            list_shares(State(broken.state.clone()), Extension(caller()), headers(), Path("doc-1".to_owned()))
                .await
                .into_response(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "delegated_auth_unavailable");
        assert!(broken.documents.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_passes_the_role_through_so_documents_api_can_refuse_edit() {
        let s = stack().await;
        Mock::given(method("POST"))
            .and(path("/v1/documents/doc-1/shares"))
            .respond_with(ResponseTemplate::new(400).set_body_json(
                json!({ "error": "role is not supported for documents", "code": "role_not_supported" }),
            ))
            .mount(&s.documents)
            .await;
        let response = create_share(
            State(s.state.clone()),
            Extension(caller()),
            headers(),
            Path("doc-1".to_owned()),
            Json(CreateShareRequest {
                subject_id: "  user-2 ".to_owned(),
                role: Some("edit".to_owned()),
            }),
        )
        .await
        .into_response();
        let (status, body) = read(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "role_not_supported");

        let upstream = s.documents.received_requests().await.unwrap();
        let sent: Value = serde_json::from_slice(&upstream[0].body).unwrap();
        assert_eq!(sent, json!({ "subject_id": "user-2", "role": "edit" }));
        assert!(s.user_core.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_sends_no_role_when_blank_and_refuses_a_blank_subject_locally() {
        let s = stack().await;
        Mock::given(method("POST"))
            .and(path("/v1/documents/doc-1/shares"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "subject_id": "user-2" })))
            .mount(&s.documents)
            .await;
        let (status, _) = read(
            create_share(
                State(s.state.clone()),
                Extension(caller()),
                headers(),
                Path("doc-1".to_owned()),
                Json(CreateShareRequest {
                    subject_id: "user-2".to_owned(),
                    role: Some("   ".to_owned()),
                }),
            )
            .await
            .into_response(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let sent: Value =
            serde_json::from_slice(&s.documents.received_requests().await.unwrap()[0].body).unwrap();
        assert_eq!(sent, json!({ "subject_id": "user-2" }));

        let before = s.documents.received_requests().await.unwrap().len();
        let (status, body) = read(
            create_share(
                State(s.state.clone()),
                Extension(caller()),
                headers(),
                Path("doc-1".to_owned()),
                Json(CreateShareRequest {
                    subject_id: "   ".to_owned(),
                    role: None,
                }),
            )
            .await
            .into_response(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_request");
        assert_eq!(s.documents.received_requests().await.unwrap().len(), before);
    }

    #[tokio::test]
    async fn revoke_deletes_the_encoded_subject_through_documents_api_only() {
        let s = stack().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/documents/doc-1/shares/user%202"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
            .mount(&s.documents)
            .await;
        let (status, body) = read(
            revoke_share(
                State(s.state.clone()),
                Extension(caller()),
                headers(),
                Path(("doc-1".to_owned(), "user 2".to_owned())),
            )
            .await
            .into_response(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["success"], true);
        assert!(s.user_core.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn shared_with_me_relays_success_and_normalises_failure() {
        let s = stack().await;
        Mock::given(method("GET"))
            .and(path("/v1/documents/shared-with-me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ids": ["d1"], "documents": [], "all_org": false })))
            .mount(&s.documents)
            .await;
        let (status, body) = read(
            shared_with_me(State(s.state.clone()), Extension(caller()), headers())
                .await
                .into_response(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ids"][0], "d1");

        s.documents.reset().await;
        Mock::given(method("GET"))
            .and(path("/v1/documents/shared-with-me"))
            .respond_with(ResponseTemplate::new(503).set_body_json(json!({ "error": "down", "code": "sharing_unavailable" })))
            .mount(&s.documents)
            .await;
        let (status, body) = read(
            shared_with_me(State(s.state.clone()), Extension(caller()), headers())
                .await
                .into_response(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "sharing_unavailable");
    }
}
