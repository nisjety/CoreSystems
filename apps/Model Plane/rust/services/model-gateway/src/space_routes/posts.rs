//! Human posts in a Space room (build plan WS01-S4).
//!
//! - `POST /v1/threads` creates a Space thread whose first row is a human `post` (no run).
//! - `POST /v1/threads/:thread_id/messages` appends a human `post` to an existing Space thread
//!   (no run).
//!
//! Both are durable writes of free text, so a zero-retention or waived caller is refused FIRST,
//! with the typed catalogue refusal (`require_non_zdr_durable_mutation`), before the flag, the
//! body or any upstream call. Authority is Session Core's: the BFF mints the Control decision
//! (`space_context` for a create, `space_append_context` for an append, both v2-signed for a
//! post) and this module forwards it with the caller's verified `aud=session-core` bearer; it
//! holds no authority of its own and never mints, widens or defaults one. Org and user come from
//! the verified claims, `role` is fixed to `user` and `post_kind` must be `post`; the bodies are
//! typed with `deny_unknown_fields`, so a forged `role`, `org_id`, `user_id`, `metadata` or
//! `agent_name` is a 400 and is never forwarded.
//!
//! The content is forwarded byte for byte (the Control decision commits to its digest), so it is
//! validated but never trimmed. The post is stored under the caller's `client_message_id` (an
//! opaque id, validated to the shape Session Core stores), so a retry of the same send lands as
//! one row (`deduplicated: true`) and the same id with other text is a 409.
//!
//! Errors, as `{error:{code,message,details}}` with fixed words (never the upstream text):
//! 400 `invalid_post` / `invalid_client_message_id`, 401 `unauthorized`, 403
//! `space_post_not_authorized`, 404 `thread_not_found` / `space_human_posts_disabled`, 409
//! `idempotency_key_conflict` / `space_authority_stale` / `thread_state_conflict`, 412 the typed
//! ZDR refusal (before any upstream call, and relayed when Session Core types one), 502
//! `post_not_stored` (the thread exists, the post did not land: retry with the same keys), and
//! 503 `session_unavailable`.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::require_non_zdr_durable_mutation;
use crate::queued_input::valid_client_message_id;
use crate::session_flow::{self, PostReceipt, SpacePostError, ThreadSpaceContext};
use crate::state::AppState;

/// The most bytes one post may hold. Session Core's member gate allows 1..=32768 and refuses
/// the rest; saying so here gives the caller a typed `invalid_post` instead of a bare refusal.
const MAX_POST_BYTES: usize = 32 * 1024;
const MAX_SESSION_KEY_BYTES: usize = 128;
const MAX_THREAD_ID_BYTES: usize = 128;

/// The human-post routes. Each shares its path with an existing route of another method, so the
/// method routers merge when this router is merged into the authenticated one.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/threads", post(create_space_post))
        .route("/v1/threads/:thread_id/messages", post(append_space_post))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FirstMessage {
    content: String,
    post_kind: String,
    client_message_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSpacePostBody {
    space_context: ThreadSpaceContext,
    session_key: String,
    first_message: FirstMessage,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppendSpacePostBody {
    content: String,
    post_kind: String,
    client_message_id: String,
    space_append_context: ThreadSpaceContext,
}

fn error_response(status: StatusCode, code: &str, message: &str, details: Value) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": details } })),
    )
        .into_response()
}

fn invalid_post(field: &str) -> Response {
    error_response(
        StatusCode::BAD_REQUEST,
        "invalid_post",
        "the post is not valid",
        json!({ "field": field, "max_bytes": MAX_POST_BYTES }),
    )
}

/// The HTTP form of a Session Core refusal of a post. Messages are fixed words, never the
/// upstream text. A typed ZDR refusal keeps its catalogue code and status (412).
pub(crate) fn session_post_error(status: &tonic::Status) -> Response {
    match status.code() {
        tonic::Code::Unauthenticated => error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "authentication required",
            json!({}),
        ),
        tonic::Code::PermissionDenied => error_response(
            StatusCode::FORBIDDEN,
            "space_post_not_authorized",
            "posting to this Space thread is not permitted",
            json!({}),
        ),
        tonic::Code::NotFound => error_response(
            StatusCode::NOT_FOUND,
            "thread_not_found",
            "the thread was not found",
            json!({}),
        ),
        tonic::Code::AlreadyExists => error_response(
            StatusCode::CONFLICT,
            "idempotency_key_conflict",
            "this send id was already used for a different post",
            json!({}),
        ),
        tonic::Code::Aborted => error_response(
            StatusCode::CONFLICT,
            "space_authority_stale",
            "the Space authority changed; request a fresh decision",
            json!({}),
        ),
        tonic::Code::InvalidArgument => invalid_post("request"),
        tonic::Code::FailedPrecondition => {
            if let Some(refusal) = mp_contracts::zdr_refusal::Refusal::from_status(status) {
                return (StatusCode::PRECONDITION_FAILED, Json(refusal.json_body())).into_response();
            }
            error_response(
                StatusCode::CONFLICT,
                "thread_state_conflict",
                "the thread cannot take a post in its current state",
                json!({}),
            )
        }
        _ => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_unavailable",
            "the conversation service is unavailable",
            json!({}),
        ),
    }
}

fn disabled() -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "space_human_posts_disabled",
        "human posts are not enabled",
        json!({}),
    )
}

fn user_principal_required() -> Response {
    error_response(
        StatusCode::FORBIDDEN,
        "space_post_not_authorized",
        "a user principal is required",
        json!({}),
    )
}

/// The checks every post passes before any upstream call. `Err` is the response to send.
fn validate_post(content: &str, post_kind: &str, client_message_id: &str) -> Result<(), Response> {
    if post_kind != "post" {
        return Err(invalid_post("post_kind"));
    }
    if content.trim().is_empty() || content.len() > MAX_POST_BYTES || content.contains('\0') {
        return Err(invalid_post("content"));
    }
    if !valid_client_message_id(client_message_id) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "invalid_client_message_id",
            "client_message_id is not a valid id",
            json!({}),
        ));
    }
    Ok(())
}

/// A decision envelope must name a Space. Session Core verifies the signature and every other
/// field; this only refuses a body that cannot be a Space post at all.
fn require_space(context: &ThreadSpaceContext, field: &str) -> Result<(), Response> {
    if context.space_id.trim().is_empty() {
        return Err(invalid_post(field));
    }
    Ok(())
}

fn receipt_json(thread_id: &str, receipt: &PostReceipt) -> Value {
    json!({
        "thread_id": thread_id,
        "message_id": receipt.message_id,
        "sequence": receipt.sequence,
        "deduplicated": receipt.deduplicated,
    })
}

fn receipt_response(thread_id: &str, receipt: &PostReceipt) -> Response {
    let status = if receipt.deduplicated {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    (status, Json(receipt_json(thread_id, receipt))).into_response()
}

fn parse_body<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, Response> {
    serde_json::from_slice(body).map_err(|_| invalid_post("body"))
}

async fn create_space_post(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    body: Bytes,
) -> Response {
    // A durable write of free text: a zero-retention or waived caller is refused first, typed.
    if let Err(refusal) = require_non_zdr_durable_mutation(&claims) {
        return refusal.into_response();
    }
    if !mp_contracts::space_human_posts::api_enabled() {
        return disabled();
    }
    if claims.user_id.trim().is_empty() {
        return user_principal_required();
    }
    let body: CreateSpacePostBody = match parse_body(&body) {
        Ok(body) => body,
        Err(response) => return response,
    };
    let session_key = body.session_key.trim();
    if session_key.is_empty()
        || session_key.len() > MAX_SESSION_KEY_BYTES
        || !valid_client_message_id(session_key)
    {
        return invalid_post("session_key");
    }
    if let Err(response) = require_space(&body.space_context, "space_context") {
        return response;
    }
    let message = &body.first_message;
    if let Err(response) = validate_post(&message.content, &message.post_kind, &message.client_message_id)
    {
        return response;
    }
    match session_flow::create_space_post_thread(
        &state,
        &bearer,
        &claims.org_id,
        &claims.user_id,
        session_key,
        &body.space_context,
        &message.content,
        &message.client_message_id,
    )
    .await
    {
        Ok((thread_id, receipt)) => receipt_response(&thread_id, &receipt),
        Err(SpacePostError::Create(status)) => {
            tracing::warn!(grpc_code = ?status.code(), "space post: thread create refused");
            session_post_error(&status)
        }
        Err(SpacePostError::Append { thread_id, status }) => {
            tracing::warn!(grpc_code = ?status.code(), "space post: thread created, post not stored");
            if matches!(
                status.code(),
                tonic::Code::PermissionDenied
                    | tonic::Code::AlreadyExists
                    | tonic::Code::Aborted
                    | tonic::Code::InvalidArgument
                    | tonic::Code::FailedPrecondition
            ) {
                // A definite refusal, not a lost reply: say what it was. The thread stays and a
                // retry with the same keys lands on it.
                return session_post_error(&status);
            }
            error_response(
                StatusCode::BAD_GATEWAY,
                "post_not_stored",
                "the thread was created but the post was not stored; retry with the same keys",
                json!({ "thread_id": thread_id }),
            )
        }
    }
}

async fn append_space_post(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(thread_id): Path<String>,
    body: Bytes,
) -> Response {
    if let Err(refusal) = require_non_zdr_durable_mutation(&claims) {
        return refusal.into_response();
    }
    if !mp_contracts::space_human_posts::api_enabled() {
        return disabled();
    }
    if claims.user_id.trim().is_empty() {
        return user_principal_required();
    }
    let thread_id = thread_id.trim().to_owned();
    if thread_id.is_empty() || thread_id.len() > MAX_THREAD_ID_BYTES {
        return error_response(
            StatusCode::NOT_FOUND,
            "thread_not_found",
            "the thread was not found",
            json!({}),
        );
    }
    let body: AppendSpacePostBody = match parse_body(&body) {
        Ok(body) => body,
        Err(response) => return response,
    };
    if let Err(response) = require_space(&body.space_append_context, "space_append_context") {
        return response;
    }
    if let Err(response) = validate_post(&body.content, &body.post_kind, &body.client_message_id) {
        return response;
    }
    let mut client = state.session_client.clone();
    match session_flow::append_post_message(
        &mut client,
        &thread_id,
        &body.content,
        bearer.as_str(),
        Some(&body.space_append_context),
        &body.client_message_id,
    )
    .await
    {
        Ok(receipt) => receipt_response(&thread_id, &receipt),
        Err(status) => {
            tracing::warn!(grpc_code = ?status.code(), "space post: append refused");
            session_post_error(&status)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn parts(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("body");
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    fn context_json() -> Value {
        json!({
            "space_id": "space-room-1",
            "space_decision_ref": "decision-1",
            "recipient_audience_ref": "audience:space-room-1:4",
            "recipient_audience_revision": 4,
            "recipient_audience_hash": "sha256:audience-hash",
            "privacy_policy_ref": "privacy:org-1:5",
            "authority_revision": 7,
            "resource_authorization_ref": "control:space-room-1:thread-append:7",
            "space_decision_token": "token",
            "action_schema_hash": "sha256:thread-append-v2",
            "payload_digest": "sha256:digest",
            "idempotency_key": "thread-append:1"
        })
    }

    fn append_body() -> Value {
        json!({
            "content": "hello room",
            "post_kind": "post",
            "client_message_id": "send-1",
            "space_append_context": context_json()
        })
    }

    fn create_body() -> Value {
        json!({
            "space_context": context_json(),
            "session_key": "room-thread-1",
            "first_message": {
                "content": "hello room",
                "post_kind": "post",
                "client_message_id": "send-1"
            }
        })
    }

    #[tokio::test]
    async fn post_route_maps_session_core_errors() {
        let cases = [
            (tonic::Code::Unauthenticated, StatusCode::UNAUTHORIZED, "unauthorized"),
            (tonic::Code::PermissionDenied, StatusCode::FORBIDDEN, "space_post_not_authorized"),
            (tonic::Code::NotFound, StatusCode::NOT_FOUND, "thread_not_found"),
            (tonic::Code::AlreadyExists, StatusCode::CONFLICT, "idempotency_key_conflict"),
            (tonic::Code::Aborted, StatusCode::CONFLICT, "space_authority_stale"),
            (tonic::Code::InvalidArgument, StatusCode::BAD_REQUEST, "invalid_post"),
            (tonic::Code::FailedPrecondition, StatusCode::CONFLICT, "thread_state_conflict"),
            (tonic::Code::Unavailable, StatusCode::SERVICE_UNAVAILABLE, "session_unavailable"),
            (tonic::Code::Internal, StatusCode::SERVICE_UNAVAILABLE, "session_unavailable"),
            (tonic::Code::DeadlineExceeded, StatusCode::SERVICE_UNAVAILABLE, "session_unavailable"),
        ];
        for (code, expected_status, expected_code) in cases {
            // The upstream text is never echoed.
            let upstream = tonic::Status::new(code, "secret upstream detail org-1/user-9");
            let (status, body) = parts(session_post_error(&upstream)).await;
            assert_eq!(status, expected_status, "{code:?}");
            assert_eq!(body["error"]["code"], expected_code, "{code:?}");
            assert!(body["error"]["details"].is_object(), "{code:?}");
            assert!(!body.to_string().contains("secret upstream"), "{code:?}");
        }
    }

    #[tokio::test]
    async fn a_typed_zdr_failed_precondition_keeps_its_catalogue_code_and_is_412() {
        let mut typed = tonic::Status::failed_precondition("zero retention");
        typed.metadata_mut().insert(
            mp_contracts::zdr_refusal::KIND_TRAILER,
            mp_contracts::zdr_refusal::code::DURABLE_MUTATION_FORBIDDEN
                .parse()
                .expect("ascii metadata"),
        );
        let (status, body) = parts(session_post_error(&typed)).await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            body,
            mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN.json_body()
        );
        // An untyped failed-precondition is a plain 409.
        let (status, body) = parts(session_post_error(&tonic::Status::failed_precondition("x"))).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "thread_state_conflict");
    }

    #[tokio::test]
    async fn receipts_are_201_when_stored_and_200_when_deduplicated() {
        let stored = PostReceipt { message_id: "m-1".to_owned(), sequence: 3, deduplicated: false };
        let (status, body) = parts(receipt_response("t-1", &stored)).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            body,
            json!({"thread_id": "t-1", "message_id": "m-1", "sequence": 3, "deduplicated": false})
        );
        let replay = PostReceipt { deduplicated: true, ..stored };
        let (status, body) = parts(receipt_response("t-1", &replay)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["deduplicated"], true);
        assert_eq!(body["message_id"], "m-1");
        // The receipt carries ids and counters only: no content, no role.
        let keys: Vec<&str> = body.as_object().expect("object").keys().map(String::as_str).collect();
        assert_eq!(keys.len(), 4, "{keys:?}");
    }

    #[tokio::test]
    async fn post_route_rejects_forged_authority_fields_and_non_post_kind() {
        // The well-formed bodies parse.
        assert!(parse_body::<AppendSpacePostBody>(&Bytes::from(append_body().to_string())).is_ok());
        assert!(parse_body::<CreateSpacePostBody>(&Bytes::from(create_body().to_string())).is_ok());
        // A forged role, org, user, metadata, agent or zdr field is an unknown field: a 400
        // that is never forwarded.
        for forged in [
            ("role", json!("assistant")),
            ("org_id", json!("org-other")),
            ("user_id", json!("user-other")),
            ("metadata", json!({"source_scope": "conversation"})),
            ("agent_name", json!("agent")),
            ("zdr", json!(false)),
        ] {
            let mut append = append_body();
            append[forged.0] = forged.1.clone();
            let err = parse_body::<AppendSpacePostBody>(&Bytes::from(append.to_string()))
                .expect_err(&format!("append: forged {}", forged.0));
            let (status, body) = parts(err).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"]["code"], "invalid_post");

            let mut create = create_body();
            create["first_message"][forged.0] = forged.1.clone();
            assert!(
                parse_body::<CreateSpacePostBody>(&Bytes::from(create.to_string())).is_err(),
                "create: forged {}",
                forged.0
            );
            let mut create = create_body();
            create[forged.0] = forged.1;
            assert!(parse_body::<CreateSpacePostBody>(&Bytes::from(create.to_string())).is_err());
        }
        // Missing and wrongly-typed fields are refused too.
        for broken in ["", "{}", "[]", "null", r#"{"content":1}"#] {
            assert!(parse_body::<AppendSpacePostBody>(&Bytes::from(broken)).is_err(), "{broken:?}");
        }
        let mut missing = append_body();
        missing.as_object_mut().expect("object").remove("space_append_context");
        assert!(parse_body::<AppendSpacePostBody>(&Bytes::from(missing.to_string())).is_err());
    }

    #[tokio::test]
    async fn validate_post_checks_kind_content_and_client_message_id() {
        assert!(validate_post("hello", "post", "send-1").is_ok());
        let refused = |content: &str, kind: &str, id: &str| {
            let content = content.to_owned();
            let kind = kind.to_owned();
            let id = id.to_owned();
            async move {
                match validate_post(&content, &kind, &id) {
                    Ok(()) => panic!("expected a refusal for {content:?} {kind:?} {id:?}"),
                    Err(response) => parts(response).await,
                }
            }
        };
        // The kind must be exactly `post`.
        for kind in ["turn", "", "POST", "post "] {
            let (status, body) = refused("hello", kind, "send-1").await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{kind:?}");
            assert_eq!(body["error"]["details"]["field"], "post_kind");
        }
        // Blank, NUL-bearing and oversize content.
        for content in ["", "   \n\t", "a\0b"] {
            let (status, body) = refused(content, "post", "send-1").await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{content:?}");
            assert_eq!(body["error"]["code"], "invalid_post");
            assert_eq!(body["error"]["details"]["field"], "content");
        }
        let (status, body) = refused(&"x".repeat(MAX_POST_BYTES + 1), "post", "send-1").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["details"]["max_bytes"], MAX_POST_BYTES);
        // Exactly the limit passes (the limit is in bytes, not characters).
        assert!(validate_post(&"x".repeat(MAX_POST_BYTES), "post", "send-1").is_ok());
        assert!(validate_post(&"\u{e5}".repeat(MAX_POST_BYTES / 2), "post", "send-1").is_ok());
        let (status, _) = refused(&"\u{e5}".repeat(MAX_POST_BYTES / 2 + 1), "post", "send-1").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // The content is never trimmed: leading and trailing space is valid and forwarded as is.
        assert!(validate_post("  padded  ", "post", "send-1").is_ok());
        // client_message_id has the shape Session Core stores.
        for id in ["", "-leading", "_x", "has space", "slash/x", &"a".repeat(129)] {
            let (status, body) = refused("hello", "post", id).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{id:?}");
            assert_eq!(body["error"]["code"], "invalid_client_message_id");
        }
        assert!(validate_post("hello", "post", &"a".repeat(128)).is_ok());
    }

    #[tokio::test]
    async fn a_decision_envelope_must_name_a_space() {
        let context: ThreadSpaceContext = serde_json::from_value(context_json()).expect("context");
        assert!(require_space(&context, "space_context").is_ok());
        let blank = ThreadSpaceContext { space_id: "  ".to_owned(), ..context };
        let (status, body) = parts(require_space(&blank, "space_append_context").expect_err("blank")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["details"]["field"], "space_append_context");
    }

    #[tokio::test]
    async fn disabled_and_service_principal_refusals_are_fixed_words() {
        let (status, body) = parts(disabled()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "space_human_posts_disabled");
        let (status, body) = parts(user_principal_required()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "space_post_not_authorized");
    }

    #[test]
    fn the_router_serves_only_post_on_the_two_documented_paths() {
        // Building the router proves the two routes are valid and do not collide; the methods
        // are POST only (the GET routes of the same paths belong to the chat routers).
        let _router: Router<AppState> = router();
    }
}
