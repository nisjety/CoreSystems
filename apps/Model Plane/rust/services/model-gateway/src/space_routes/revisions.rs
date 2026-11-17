//! Edit, delete and resolve one message of a Space thread (build plan WS12-S6).
//!
//! - `PATCH  /v1/threads/:thread_id/messages/:message_id` edits one message.
//! - `DELETE /v1/threads/:thread_id/messages/:message_id` deletes one message (a tombstone for
//!   every reader; the original is purged or retained by Control's retention rule).
//! - `GET    /v1/threads/:thread_id/messages/:message_id/target` tells the caller the role and
//!   revision state of a message, so the BFF can decide what to ask Control for.
//!
//! A thin relay of Session Core's `MessageRevisionService`. Authority is Control's and Session
//! Core's: the BFF mints the `model.thread.revise` decision (a v1 append decision cannot revise)
//! and this module forwards it with the caller's verified `aud=session-core` bearer. It holds no
//! authority of its own: `org_id` and the caller come from the verified claims, the bodies are
//! typed with `deny_unknown_fields` (so a forged `actor_class`, `original_retention`, `role` or
//! `user_id` is a 400 and is never forwarded), and the actor class, the retention mode and the
//! moderator right are all derived by Session Core from the signed decision and the stored row.
//!
//! The edit text is forwarded byte for byte (Control's decision commits to its digest), so it is
//! validated but never trimmed. Edit and delete are durable writes of free text: a zero-retention
//! or waived caller is refused FIRST, with the typed catalogue refusal, before the flag, the body
//! or any upstream call. The target route is a read and is open to a ZDR caller.
//!
//! Errors, as `{error:{code,message,details}}` with fixed words (never the upstream text): 400
//! `invalid_message` / `revision_noop` / `invalid_idempotency_key`, 401 `unauthorized`, 403
//! `space_message_revise_not_authorized` / `message_revise_forbidden` /
//! `space_read_not_authorized`, 404 `space_message_revise_disabled` / `message_not_found` /
//! `thread_not_found`, 409 `message_already_deleted` / `thread_run_active` /
//! `idempotency_key_conflict` / `space_authority_stale` / `revision_limit` /
//! `message_state_conflict` / `erasure_blocked_legal_hold` (a legal hold, `retryable: false`),
//! 412 the typed ZDR refusal, 503 `session_unavailable` / `erasure_hold_check_unavailable`
//! (a hold check that could not be made, `retryable: true`).

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use mp_contracts::message_revision as rev;
use mp_contracts::model_plane::v1::{ResolveMessageTargetRequest, ReviseMessageRequest};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::{authenticated_session_request, require_non_zdr_durable_mutation, timestamp_to_rfc3339};
use crate::state::AppState;

const MAX_ID_BYTES: usize = 128;
const MAX_KEY_BYTES: usize = 128;
const MAX_DECISION_BYTES: usize = 8192;
/// Session Core's own trailer for a closed error word (`run_phase::ERROR_CODE_TRAILER`).
const ERROR_CODE_TRAILER: &str = "x-mp-error-code";

/// The revise routes. They share a path prefix with the existing thread routes, so the method
/// routers merge when this router is merged into the authenticated one.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/threads/:thread_id/messages/:message_id",
            axum::routing::patch(edit_message).delete(delete_message),
        )
        .route(
            "/v1/threads/:thread_id/messages/:message_id/target",
            get(resolve_target),
        )
}

/// The BFF-injected `model.thread.revise` decision (the twelve Space decision fields). It is not
/// an authority by itself: Session Core verifies the signed token and recomputes the digest from
/// the request and the verified caller. `idempotency_key` is part of the signed digest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviseContext {
    space_id: String,
    space_decision_ref: String,
    recipient_audience_ref: String,
    recipient_audience_revision: u64,
    recipient_audience_hash: String,
    privacy_policy_ref: String,
    authority_revision: u64,
    resource_authorization_ref: String,
    space_decision_token: String,
    action_schema_hash: String,
    payload_digest: String,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditBody {
    content: String,
    space_revise_context: ReviseContext,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteBody {
    space_revise_context: ReviseContext,
    /// Closed set, honoured by Session Core only for a moderator delete.
    #[serde(default)]
    moderation_reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetQuery {
    space_id: String,
    #[serde(default)]
    space_read_decision_ref: String,
    #[serde(default)]
    space_read_decision_token: String,
}

fn error_response(status: StatusCode, code: &str, message: &str, details: Value) -> Response {
    let mut response = (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": details } })),
    )
        .into_response();
    no_store(&mut response);
    response
}

fn no_store(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
}

fn invalid_message(field: &str) -> Response {
    error_response(
        StatusCode::BAD_REQUEST,
        "invalid_message",
        "the request is not valid",
        json!({ "field": field, "max_bytes": rev::MAX_EDIT_BYTES }),
    )
}

fn disabled() -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "space_message_revise_disabled",
        "editing and deleting messages is not enabled",
        json!({}),
    )
}

fn user_principal_required(code: &str) -> Response {
    error_response(
        StatusCode::FORBIDDEN,
        code,
        "a user principal is required",
        json!({}),
    )
}

fn not_found(code: &str, message: &str) -> Response {
    error_response(StatusCode::NOT_FOUND, code, message, json!({}))
}

/// The closed error word Session Core attached to a refusal, when it is one this route knows.
fn upstream_code(status: &tonic::Status) -> Option<&'static str> {
    let raw = status.metadata().get(ERROR_CODE_TRAILER)?.to_str().ok()?;
    [
        "revise_disabled",
        "message_not_found",
        "thread_not_found",
        "space_authority_stale",
        "thread_run_active",
        "message_already_deleted",
        "idempotency_key_conflict",
        "revision_noop",
        "revision_limit",
        "message_revise_forbidden",
    ]
    .into_iter()
    .find(|known| *known == raw)
}

/// EP-9 C4 (V3-sc-1, V3-mg-5-rel2): a destructive revision that a legal hold refuses, or whose hold check could not
/// be made, keeps the typed erasure code that Session Core puts on the `x-mp-error-code` trailer, as the thread erase
/// route does (`thread_erase_error`). Without it the hold reads as the generic 409 `message_state_conflict`, the
/// outage as the generic 503 `session_unavailable`, and the retryable flag is lost. The answer is in fixed words, so
/// no upstream text crosses. `None` for every other refusal, which keeps its generic answer.
fn erasure_hold_error(status: &tonic::Status) -> Option<Response> {
    let trailer = status.metadata().get(ERROR_CODE_TRAILER)?.to_str().ok()?;
    let (http, code, message, retryable) = match trailer {
        "erasure_blocked_legal_hold" => (
            StatusCode::CONFLICT,
            "erasure_blocked_legal_hold",
            "A legal hold covers this data, so nothing was changed. The change can be made once the hold is released.",
            false,
        ),
        "erasure_hold_check_unavailable" => (
            StatusCode::SERVICE_UNAVAILABLE,
            "erasure_hold_check_unavailable",
            "Legal holds could not be checked, so nothing was changed. Try again shortly.",
            true,
        ),
        _ => return None,
    };
    Some(error_response(
        http,
        code,
        message,
        json!({ "retryable": retryable }),
    ))
}

/// The HTTP form of a Session Core refusal of a revise or target call. Messages are fixed words,
/// never the upstream text. A typed ZDR refusal keeps its catalogue code and status (412), and a
/// typed legal-hold refusal keeps its erasure code (409 or 503). `read` is true for the target
/// route, whose denial is a read denial.
pub(crate) fn session_revise_error(status: &tonic::Status, read: bool) -> Response {
    if let Some(refusal) = mp_contracts::zdr_refusal::Refusal::from_status(status) {
        return (StatusCode::PRECONDITION_FAILED, Json(refusal.json_body())).into_response();
    }
    if let Some(hold) = erasure_hold_error(status) {
        return hold;
    }
    let code = upstream_code(status);
    match status.code() {
        tonic::Code::Unauthenticated => error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "authentication required",
            json!({}),
        ),
        tonic::Code::PermissionDenied if read => error_response(
            StatusCode::FORBIDDEN,
            "space_read_not_authorized",
            "reading this Space is not permitted",
            json!({}),
        ),
        tonic::Code::PermissionDenied => {
            if code == Some("message_revise_forbidden") {
                error_response(
                    StatusCode::FORBIDDEN,
                    "message_revise_forbidden",
                    "this message cannot be changed by this person",
                    json!({}),
                )
            } else {
                error_response(
                    StatusCode::FORBIDDEN,
                    "space_message_revise_not_authorized",
                    "changing this message is not permitted",
                    json!({}),
                )
            }
        }
        tonic::Code::NotFound => {
            if code == Some("thread_not_found") {
                not_found("thread_not_found", "the thread was not found")
            } else {
                not_found("message_not_found", "the message was not found")
            }
        }
        tonic::Code::AlreadyExists => error_response(
            StatusCode::CONFLICT,
            "idempotency_key_conflict",
            "this idempotency key was already used for a different change",
            json!({}),
        ),
        tonic::Code::Aborted => {
            if code == Some("thread_run_active") {
                error_response(
                    StatusCode::CONFLICT,
                    "thread_run_active",
                    "an agent run is answering this message",
                    json!({}),
                )
            } else {
                error_response(
                    StatusCode::CONFLICT,
                    "space_authority_stale",
                    "the Space authority changed; request a fresh decision",
                    json!({}),
                )
            }
        }
        tonic::Code::FailedPrecondition => match code {
            Some("revise_disabled") => disabled(),
            Some("message_already_deleted") => error_response(
                StatusCode::CONFLICT,
                "message_already_deleted",
                "the message was already deleted",
                json!({}),
            ),
            Some("revision_limit") => error_response(
                StatusCode::CONFLICT,
                "revision_limit",
                "the message has reached its revision limit",
                json!({}),
            ),
            _ => error_response(
                StatusCode::CONFLICT,
                "message_state_conflict",
                "the message cannot be changed in its current state",
                json!({}),
            ),
        },
        tonic::Code::InvalidArgument => {
            if code == Some("revision_noop") {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "revision_noop",
                    "the edit changes nothing",
                    json!({}),
                )
            } else {
                invalid_message("request")
            }
        }
        tonic::Code::Unimplemented => disabled(),
        _ => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_unavailable",
            "the conversation service is unavailable",
            json!({}),
        ),
    }
}

/// A path id: trimmed, non-empty and bounded. A malformed id is the same answer as a missing one.
fn bounded_id(raw: &str) -> Option<String> {
    let id = raw.trim();
    (!id.is_empty() && id.len() <= MAX_ID_BYTES).then(|| id.to_owned())
}

/// The checks the decision envelope passes before any upstream call. Session Core verifies the
/// signature and every other field; this only refuses a body that cannot be a revision at all.
fn check_context(context: &ReviseContext) -> Result<(), Response> {
    if context.space_id.trim().is_empty() {
        return Err(invalid_message("space_revise_context"));
    }
    let key = context.idempotency_key.as_str();
    if key.trim().is_empty() || key.len() > MAX_KEY_BYTES {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "invalid_idempotency_key",
            "the idempotency key is not valid",
            json!({}),
        ));
    }
    if context.space_decision_token.len() > MAX_DECISION_BYTES
        || context.space_decision_ref.len() > MAX_DECISION_BYTES
    {
        return Err(invalid_message("space_revise_context"));
    }
    Ok(())
}

fn revise_request(
    org_id: &str,
    thread_id: String,
    message_id: String,
    action: &str,
    content: String,
    moderation_reason: String,
    context: ReviseContext,
) -> ReviseMessageRequest {
    ReviseMessageRequest {
        org_id: org_id.to_owned(),
        thread_id,
        message_id,
        action: action.to_owned(),
        content,
        moderation_reason,
        space_id: context.space_id,
        space_decision_ref: context.space_decision_ref,
        recipient_audience_ref: context.recipient_audience_ref,
        recipient_audience_revision: context.recipient_audience_revision,
        recipient_audience_hash: context.recipient_audience_hash,
        privacy_policy_ref: context.privacy_policy_ref,
        authority_revision: context.authority_revision,
        resource_authorization_ref: context.resource_authorization_ref,
        space_decision_token: context.space_decision_token,
        action_schema_hash: context.action_schema_hash,
        payload_digest: context.payload_digest,
        idempotency_key: context.idempotency_key,
    }
}

async fn send_revision(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    request: ReviseMessageRequest,
) -> Response {
    let request = match authenticated_session_request(request, bearer) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };
    match state.message_revision_client.clone().revise_message(request).await {
        Ok(response) => {
            let response = response.into_inner();
            let mut out = Json(json!({
                "message_id": response.message_id,
                "revision_state": response.revision_state,
                "revision_no": response.revision_no,
                "revised_at": timestamp_to_rfc3339(response.revised_at),
                "deduplicated": response.deduplicated,
                "actor_class": response.actor_class,
            }))
            .into_response();
            no_store(&mut out);
            out
        }
        Err(status) => {
            tracing::warn!(grpc_code = ?status.code(), "space message revision refused");
            session_revise_error(&status, false)
        }
    }
}

async fn edit_message(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((thread_id, message_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    // A durable write of free text: a zero-retention or waived caller is refused first, typed.
    if let Err(refusal) = require_non_zdr_durable_mutation(&claims) {
        return refusal.into_response();
    }
    if !mp_contracts::space_message_lifecycle::api_enabled() {
        return disabled();
    }
    if claims.user_id.trim().is_empty() {
        return user_principal_required("space_message_revise_not_authorized");
    }
    let (Some(thread_id), Some(message_id)) = (bounded_id(&thread_id), bounded_id(&message_id))
    else {
        return not_found("message_not_found", "the message was not found");
    };
    let body: EditBody = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(_) => return invalid_message("body"),
    };
    if body.content.trim().is_empty()
        || body.content.len() > rev::MAX_EDIT_BYTES
        || body.content.contains('\0')
    {
        return invalid_message("content");
    }
    if let Err(response) = check_context(&body.space_revise_context) {
        return response;
    }
    let request = revise_request(
        &claims.org_id,
        thread_id,
        message_id,
        rev::ACTION_EDIT,
        body.content,
        String::new(),
        body.space_revise_context,
    );
    send_revision(&state, &bearer, request).await
}

async fn delete_message(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((thread_id, message_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    if let Err(refusal) = require_non_zdr_durable_mutation(&claims) {
        return refusal.into_response();
    }
    if !mp_contracts::space_message_lifecycle::api_enabled() {
        return disabled();
    }
    if claims.user_id.trim().is_empty() {
        return user_principal_required("space_message_revise_not_authorized");
    }
    let (Some(thread_id), Some(message_id)) = (bounded_id(&thread_id), bounded_id(&message_id))
    else {
        return not_found("message_not_found", "the message was not found");
    };
    let body: DeleteBody = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(_) => return invalid_message("body"),
    };
    if !body.moderation_reason.is_empty()
        && !rev::MODERATION_REASONS.contains(&body.moderation_reason.as_str())
    {
        return invalid_message("moderation_reason");
    }
    if let Err(response) = check_context(&body.space_revise_context) {
        return response;
    }
    let request = revise_request(
        &claims.org_id,
        thread_id,
        message_id,
        rev::ACTION_DELETE,
        String::new(),
        body.moderation_reason,
        body.space_revise_context,
    );
    send_revision(&state, &bearer, request).await
}

async fn resolve_target(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((thread_id, message_id)): Path<(String, String)>,
    query: Result<Query<TargetQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    // A read: a ZDR caller may resolve a target, so there is no ZDR refusal here.
    if !mp_contracts::space_message_lifecycle::api_enabled() {
        return disabled();
    }
    if claims.user_id.trim().is_empty() {
        return user_principal_required("space_read_not_authorized");
    }
    let (Some(thread_id), Some(message_id)) = (bounded_id(&thread_id), bounded_id(&message_id))
    else {
        return not_found("message_not_found", "the message was not found");
    };
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return invalid_message("query"),
    };
    let space_id = query.space_id.trim().to_owned();
    if space_id.is_empty()
        || space_id.len() > MAX_ID_BYTES
        || query.space_read_decision_ref.len() > MAX_DECISION_BYTES
        || query.space_read_decision_token.len() > MAX_DECISION_BYTES
    {
        return invalid_message("query");
    }
    let request = match authenticated_session_request(
        ResolveMessageTargetRequest {
            org_id: claims.org_id.clone(),
            thread_id,
            message_id,
            space_id,
            space_read_decision_ref: query.space_read_decision_ref,
            space_read_decision_token: query.space_read_decision_token,
        },
        &bearer,
    ) {
        Ok(request) => request,
        Err((status, body)) => return (status, body).into_response(),
    };
    match state
        .message_revision_client
        .clone()
        .resolve_message_target(request)
        .await
    {
        Ok(response) => {
            let response = response.into_inner();
            let mut out = Json(json!({
                "role": response.role,
                "revision_state": response.revision_state,
            }))
            .into_response();
            no_store(&mut out);
            out
        }
        Err(status) => {
            tracing::warn!(grpc_code = ?status.code(), "space message target refused");
            session_revise_error(&status, true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use mp_contracts::model_plane::v1::message_revision_service_client as revision_client;

    async fn parts(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A Session Core refusal carrying the `x-mp-error-code` trailer, as the revise RPC sends a legal hold.
    fn coded(mut status: tonic::Status, code: &'static str) -> tonic::Status {
        status.metadata_mut().insert(
            ERROR_CODE_TRAILER,
            tonic::metadata::MetadataValue::from_static(code),
        );
        status
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
            "resource_authorization_ref": "control:space-room-1:message-revise:7",
            "space_decision_token": "token",
            "action_schema_hash": "sha256:thread-revise-v1",
            "payload_digest": "sha256:digest",
            "idempotency_key": "message-delete:1"
        })
    }

    /// V3-mg-5-rel2: a legal hold on a destructive revision is the typed, non-retryable 409, and a hold check that
    /// could not be made is the typed, retryable 503. A refusal without the trailer keeps its generic answer, as a
    /// refusal from a Session Core that never sends the trailer (the hold switch off) does, and no upstream text crosses.
    #[tokio::test]
    async fn destructive_revision_holds_keep_their_typed_codes() {
        let (held_status, held) = parts(session_revise_error(
            &coded(
                tonic::Status::failed_precondition("upstream-hold-detail"),
                "erasure_blocked_legal_hold",
            ),
            false,
        ))
        .await;
        assert_eq!(held_status, StatusCode::CONFLICT);
        assert_eq!(held["error"]["code"], "erasure_blocked_legal_hold");
        assert_eq!(held["error"]["details"]["retryable"], false);

        let (outage_status, outage) = parts(session_revise_error(
            &coded(
                tonic::Status::unavailable("upstream-check-detail"),
                "erasure_hold_check_unavailable",
            ),
            false,
        ))
        .await;
        assert_eq!(outage_status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(outage["error"]["code"], "erasure_hold_check_unavailable");
        assert_eq!(outage["error"]["details"]["retryable"], true);

        let (plain_status, plain) = parts(session_revise_error(
            &tonic::Status::failed_precondition("upstream-hold-detail"),
            false,
        ))
        .await;
        assert_eq!(plain_status, StatusCode::CONFLICT);
        assert_eq!(plain["error"]["code"], "message_state_conflict");
        assert_eq!(plain["error"]["details"], json!({}));

        let (plain_outage_status, plain_outage) = parts(session_revise_error(
            &tonic::Status::unavailable("upstream-check-detail"),
            false,
        ))
        .await;
        assert_eq!(plain_outage_status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(plain_outage["error"]["code"], "session_unavailable");
        assert_eq!(plain_outage["error"]["details"], json!({}));

        let text = format!("{held}{outage}{plain}{plain_outage}");
        assert!(
            !text.contains("upstream-"),
            "upstream text never crosses: {text}"
        );
    }

    type RevisionRequest = tonic::codegen::http::Request<tonic::body::BoxBody>;
    type RevisionResponse = tonic::codegen::http::Response<tonic::body::BoxBody>;

    /// A stand-in Session Core that answers every message revision RPC with one refusal, trailer and all, so the
    /// route sees what `revise_message` sends back for a held or unverifiable destructive revision.
    #[derive(Clone)]
    struct RefusingRevision(tonic::Status);

    impl tonic::server::NamedService for RefusingRevision {
        const NAME: &'static str = "model_plane.v1.MessageRevisionService";
    }

    impl tower::Service<RevisionRequest> for RefusingRevision {
        type Response = RevisionResponse;
        type Error = std::convert::Infallible;
        type Future = std::future::Ready<Result<RevisionResponse, std::convert::Infallible>>;

        fn poll_ready(
            &mut self,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: RevisionRequest) -> Self::Future {
            std::future::ready(Ok(self.0.clone().into_http()))
        }
    }

    /// The route's HTTP answer to a delete that Session Core refuses with `refusal` over the wire.
    async fn delete_answer_to_session_refusal(refusal: tonic::Status) -> Response {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind revision stand-in");
        let address = listener.local_addr().expect("revision stand-in address");
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(RefusingRevision(refusal))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
            .expect("revision stand-in endpoint")
            .connect()
            .await
            .expect("connect revision stand-in");
        let mut state = AppState::new();
        state.message_revision_client = revision_client::MessageRevisionServiceClient::new(channel);
        let context: ReviseContext =
            serde_json::from_value(context_json()).expect("a decision envelope");
        let request = revise_request(
            "org-a",
            "thread-a".to_owned(),
            "message-a".to_owned(),
            rev::ACTION_DELETE,
            String::new(),
            String::new(),
            context,
        );
        send_revision(
            &state,
            &VerifiedSessionBearer::for_test("session-core-token"),
            request,
        )
        .await
    }

    /// V3-mg-5-rel2: a legal hold on a delete survives the wire and answers with the typed, non-retryable 409.
    #[tokio::test]
    async fn a_delete_refused_by_a_legal_hold_keeps_its_typed_code_over_the_wire() {
        let response = delete_answer_to_session_refusal(coded(
            tonic::Status::failed_precondition("upstream-hold-detail"),
            "erasure_blocked_legal_hold",
        ))
        .await;
        let (status, body) = parts(response).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "erasure_blocked_legal_hold");
        assert_eq!(body["error"]["details"]["retryable"], false);
        assert!(!body.to_string().contains("upstream-hold-detail"));
    }

    /// V3-mg-5-rel2: a hold check that could not be made survives the wire and answers with the typed, retryable 503.
    #[tokio::test]
    async fn a_delete_whose_hold_check_failed_keeps_its_typed_retryable_code_over_the_wire() {
        let response = delete_answer_to_session_refusal(coded(
            tonic::Status::unavailable("upstream-check-detail"),
            "erasure_hold_check_unavailable",
        ))
        .await;
        let (status, body) = parts(response).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "erasure_hold_check_unavailable");
        assert_eq!(body["error"]["details"]["retryable"], true);
        assert!(!body.to_string().contains("upstream-check-detail"));
    }
}
