use axum::{
    body::Bytes,
    extract::{Extension, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::{config::AppState, middleware::AuthenticatedUser};

/// Ephemeral extraction: imports-core authenticates the ingestion audience and
/// reads bytes in memory. No document/job is created and no storage scope is accepted.
pub(crate) async fn extract_chat_document(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let Some(token) = super::shared::ingestion_token(&state, &user, &headers).await else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(crate::envelope::error(
            "imports_auth_unavailable", "Document reading is temporarily unavailable.",
        ))).into_response();
    };
    let mut response = crate::upstream::proxy_bearer_json(
        &state, Method::POST, &format!("{}/api/v1/import/extract", state.imports_api_url),
        Some(payload), Some(&token), &user.user_id,
    ).await.into_response();
    response.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
    response
}

/// Non-empty trimmed string field, or `None`.
fn text_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// The `metadata.origin` of a chat attachment, and the only one while the promote switch is off.
const CHAT_ATTACHMENT_ORIGIN: &str = "chat-attachment";
/// The `metadata.origin` of an artifact revision saved to knowledge (EP-3 S8).
const CHAT_ARTIFACT_ORIGIN: &str = "chat-artifact";
/// Longest thread or artifact id a `source_ref` may carry. Ids are opaque labels, never content.
const MAX_SOURCE_REF_ID_CHARS: usize = 256;
/// The composer capabilities are a handful of booleans; anything larger is not that answer.
const CAPABILITIES_BODY_LIMIT: usize = 64 * 1024;

/// A body this route refuses with 400 and the typed envelope.
#[derive(Debug)]
struct BodyRefusal {
    code: &'static str,
    message: &'static str,
    details: Value,
}

/// The document's `metadata` (EP-3 S8).
///
/// Off, it is the fixed attachment origin whatever the body says, exactly as before the switch
/// existed. On, the caller may name an origin from the allowlist, and a `chat-artifact` copy must
/// carry its `source_ref` (thread, artifact, version) so the Data Plane source trace says which
/// revision it came from. Only those three keys are copied: the rest of the body never reaches
/// metadata. The reference is the caller's own label for its own copy, not a grant: the copy is
/// made from the text in the body, under the user's token, and reads nothing from the thread.
fn document_metadata(payload: &Value, promote_enabled: bool) -> Result<Value, BodyRefusal> {
    if !promote_enabled {
        return Ok(json!({ "origin": CHAT_ATTACHMENT_ORIGIN }));
    }
    let origin = match payload.get("origin") {
        None | Some(Value::Null) => CHAT_ATTACHMENT_ORIGIN,
        Some(Value::String(origin)) if origin == CHAT_ATTACHMENT_ORIGIN => CHAT_ATTACHMENT_ORIGIN,
        Some(Value::String(origin)) if origin == CHAT_ARTIFACT_ORIGIN => CHAT_ARTIFACT_ORIGIN,
        Some(_) => {
            return Err(BodyRefusal {
                code: "invalid_origin",
                message: "`origin` must be `chat-attachment` or `chat-artifact`.",
                details: json!({ "allowed": [CHAT_ATTACHMENT_ORIGIN, CHAT_ARTIFACT_ORIGIN] }),
            })
        }
    };
    match (origin, payload.get("source_ref").filter(|value| !value.is_null())) {
        (CHAT_ARTIFACT_ORIGIN, Some(source_ref)) => Ok(json!({
            "origin": CHAT_ARTIFACT_ORIGIN,
            "source_ref": parse_source_ref(source_ref)?,
        })),
        (CHAT_ARTIFACT_ORIGIN, None) => Err(invalid_source_ref(
            "A `chat-artifact` copy needs `source_ref` with `thread_id`, `artifact_id` and `version`.",
        )),
        (_, Some(_)) => Err(invalid_source_ref(
            "`source_ref` is only accepted with `origin: chat-artifact`.",
        )),
        (origin, None) => Ok(json!({ "origin": origin })),
    }
}

/// `{thread_id, artifact_id, version}`: two non-empty ids and a version of at least 1.
fn parse_source_ref(source_ref: &Value) -> Result<Value, BodyRefusal> {
    let id = |key: &str| {
        source_ref
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.chars().count() <= MAX_SOURCE_REF_ID_CHARS)
            .map(str::to_owned)
    };
    let version = source_ref
        .get("version")
        .and_then(Value::as_u64)
        .filter(|version| *version >= 1);
    match (id("thread_id"), id("artifact_id"), version) {
        (Some(thread_id), Some(artifact_id), Some(version)) => Ok(json!({
            "thread_id": thread_id,
            "artifact_id": artifact_id,
            "version": version,
        })),
        _ => Err(invalid_source_ref(
            "`source_ref` needs a `thread_id`, an `artifact_id` and a `version` of at least 1.",
        )),
    }
}

fn invalid_source_ref(message: &'static str) -> BodyRefusal {
    BodyRefusal {
        code: "invalid_source_ref",
        message,
        details: json!({ "field": "source_ref" }),
    }
}

/// Adds `artifact_promote: true` to a successful composer-capabilities answer when the promote
/// switch is on (EP-3 S8). The switch is the BFF's own, so the gateway cannot report it; the SPA
/// reads one capabilities answer either way. Off, or for any answer that is not a success, the
/// response passes through untouched, so a capability is never offered on a guess.
pub(super) async fn with_promote_capability(response: Response, enabled: bool) -> Response {
    if !enabled || !response.status().is_success() {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, CAPABILITIES_BODY_LIMIT).await else {
        // The body is gone; an error the SPA reads as "nothing offered" is the honest answer.
        return (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::error(
                "capabilities_unavailable",
                "The composer capabilities could not be read.",
            )),
        )
            .into_response();
    };
    let mut payload = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    let Some(data) = payload.get_mut("data").and_then(Value::as_object_mut) else {
        return Response::from_parts(parts, axum::body::Body::from(bytes));
    };
    data.insert("artifact_promote".to_owned(), Value::Bool(true));
    // The body changed length; the old header would truncate or stall the reply.
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    Response::from_parts(parts, axum::body::Body::from(payload.to_string()))
}

/// POST /api/v1/chat/documents — persist a chat text attachment as a durable
/// Data Plane v2 document so retrieval can ground later turns on it.
///
/// With the artifact-promote switch on (EP-3 S8, `space_features.chat_artifact_promote`) it also saves one artifact
/// revision to knowledge (`origin: chat-artifact` plus `source_ref`). That copy
/// is independent of the thread: it follows Data Plane retention and erase, so
/// deleting the chat leaves it. With the switch on the BFF refuses a Zero Data
/// Retention caller (request or org posture) with the typed 412 itself, before
/// documents-api is called, instead of relying on the upstream to refuse.
///
/// This used to forward to model-gateway `/v1/chat/documents`, which is dead:
/// that handler calls `DocumentService.CreateDocument` over gRPC, and Data
/// Plane v2 has disabled the method, answering every call with
/// "gRPC DocumentService.CreateDocument is disabled; use documents-api-go
/// POST /v1/documents instead". The whole chain returned 502, so the upload was
/// unreachable rather than merely unwired. We now take the route Data Plane v2
/// names, reusing the same documents-api leg the knowledge domain already
/// proxies through, which mints the scoped `data-plane` audience token.
pub(crate) async fn upload_chat_document(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_body",
                    "message": "Expected a JSON object with `title` and `content`."
                }
            })),
        )
            .into_response();
    };

    let promote_enabled = state.space_features.chat_artifact_promote;
    if promote_enabled {
        // A durable copy of chat content is exactly what Zero Data Retention forbids; an
        // undecidable posture answers 503 rather than guessing.
        if let Some(refusal) =
            crate::zdr::durable_write_refusal(&state, &user, Some(&headers), Some(&payload)).await
        {
            return refusal.into_response();
        }
    }

    // Title and content are the document; without them there is nothing to
    // ground on, and documents-api would reject the row anyway.
    let (Some(title), Some(content)) = (
        text_field(&payload, "title"),
        text_field(&payload, "content"),
    ) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "invalid_body",
                    "message": "`title` and `content` are both required and must be non-empty."
                }
            })),
        )
            .into_response();
    };

    let metadata = match document_metadata(&payload, promote_enabled) {
        Ok(metadata) => metadata,
        Err(refusal) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(crate::envelope::error_with_details(
                    refusal.code,
                    refusal.message,
                    refusal.details,
                )),
            )
                .into_response()
        }
    };

    let org_id = {
        let resolved = crate::upstream::authorized_org_id(&state, &user).await;
        (!resolved.is_empty()).then_some(resolved)
    };

    let document = json!({
        "org_id": org_id.clone().unwrap_or_default(),
        "source": text_field(&payload, "source").unwrap_or_else(|| "chat-upload".to_owned()),
        "type": text_field(&payload, "type").unwrap_or_else(|| "text".to_owned()),
        "title": title,
        "content": content,
        "zdr_classification": "internal",
        "metadata": metadata,
    });

    // documents-api answers with the document row itself, whose `document_id`
    // and `status` are the two fields the composer reports back to the user, so
    // the upstream body passes through unchanged.
    let url = format!("{}/v1/documents", state.documents_api_url);
    crate::domains::knowledge::shared::proxy_data_plane_json(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(document),
        org_id.as_deref(),
        None,
    )
    .await
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-test".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User Test".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-test".to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: "org-test".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn artifact_body() -> Value {
        json!({
            "title": "Rapport",
            "content": "tekst",
            "origin": "chat-artifact",
            "source_ref": { "thread_id": "thread-1", "artifact_id": "report", "version": 2 },
        })
    }

    async fn body_json(response: Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn origin_allowlist_rejects_unknown() {
        for origin in [json!("knowledge"), json!("CHAT-ARTIFACT"), json!(""), json!(7)] {
            let refusal = document_metadata(&json!({ "origin": origin }), true).unwrap_err();
            assert_eq!(refusal.code, "invalid_origin", "{origin}");
            // A list travels under a named key: the SPA client drops array details.
            assert!(refusal.details["allowed"].is_array());
        }
        assert_eq!(
            document_metadata(&json!({}), true).unwrap(),
            json!({ "origin": "chat-attachment" })
        );
        assert_eq!(
            document_metadata(&json!({ "origin": "chat-attachment" }), true).unwrap(),
            json!({ "origin": "chat-attachment" })
        );
    }

    #[test]
    fn source_ref_lands_in_metadata() {
        let mut body = artifact_body();
        // Keys beyond the three are dropped, and the ids are trimmed.
        body["source_ref"]["owner"] = json!("someone-else");
        body["source_ref"]["thread_id"] = json!(" thread-1 ");
        assert_eq!(
            document_metadata(&body, true).unwrap(),
            json!({
                "origin": "chat-artifact",
                "source_ref": { "thread_id": "thread-1", "artifact_id": "report", "version": 2 },
            })
        );
    }

    #[test]
    fn an_artifact_copy_needs_a_whole_source_ref_and_an_attachment_takes_none() {
        let mut missing = artifact_body();
        missing.as_object_mut().unwrap().remove("source_ref");
        assert_eq!(document_metadata(&missing, true).unwrap_err().code, "invalid_source_ref");
        for broken in [
            json!({ "thread_id": "thread-1", "artifact_id": "report", "version": 0 }),
            json!({ "thread_id": "thread-1", "artifact_id": "report", "version": 1.5 }),
            json!({ "thread_id": "", "artifact_id": "report", "version": 1 }),
            json!({ "thread_id": "thread-1", "artifact_id": "x".repeat(257), "version": 1 }),
            json!({ "thread_id": "thread-1", "version": 1 }),
            json!("thread-1/report/1"),
        ] {
            let mut body = artifact_body();
            body["source_ref"] = broken.clone();
            let refusal = document_metadata(&body, true).unwrap_err();
            assert_eq!(refusal.code, "invalid_source_ref", "{broken}");
            assert_eq!(refusal.details, json!({ "field": "source_ref" }));
        }
        let mut attachment = artifact_body();
        attachment["origin"] = json!("chat-attachment");
        assert_eq!(document_metadata(&attachment, true).unwrap_err().code, "invalid_source_ref");
    }

    #[test]
    fn switch_off_ignores_artifact_origin() {
        // Off, the metadata is what the route always wrote, and nothing in the body is refused.
        for body in [
            artifact_body(),
            json!({ "origin": "anything", "source_ref": "junk" }),
            json!({}),
        ] {
            assert_eq!(
                document_metadata(&body, false).unwrap(),
                json!({ "origin": "chat-attachment" })
            );
        }
    }

    #[tokio::test]
    async fn zdr_caller_refused_412() {
        let mut state = AppState::for_tests();
        state.space_features.chat_artifact_promote = true;
        // The request's own posture decides first, so no upstream is ever called.
        let mut headers = HeaderMap::new();
        headers.insert("x-zdr", HeaderValue::from_static("true"));
        let response = upload_chat_document(
            State(state.clone()),
            Extension(user()),
            headers,
            Bytes::from(artifact_body().to_string()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            body_json(response).await["error"]["code"],
            json!(crate::zdr::ZDR_DURABLE_MUTATION_FORBIDDEN)
        );

        // A temporary chat says so in the body; that alone refuses too.
        let mut body = artifact_body();
        body["zdr"] = json!(true);
        let response = upload_chat_document(
            State(state),
            Extension(user()),
            HeaderMap::new(),
            Bytes::from(body.to_string()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn capabilities_gain_artifact_promote_only_with_the_switch_on() {
        let answer = || {
            (StatusCode::OK, Json(json!({ "data": { "artifact_history": true } }))).into_response()
        };
        let on = body_json(with_promote_capability(answer(), true).await).await;
        assert_eq!(on, json!({ "data": { "artifact_history": true, "artifact_promote": true } }));

        let off = body_json(with_promote_capability(answer(), false).await).await;
        assert_eq!(off, json!({ "data": { "artifact_history": true } }));

        // A failed capabilities read stays a failure: nothing is offered on a guess.
        let failed = (
            StatusCode::NOT_FOUND,
            Json(crate::envelope::error("not_found", "No such route.")),
        )
            .into_response();
        let failed = with_promote_capability(failed, true).await;
        assert_eq!(failed.status(), StatusCode::NOT_FOUND);
        assert!(body_json(failed).await["data"].is_null());
    }
}
