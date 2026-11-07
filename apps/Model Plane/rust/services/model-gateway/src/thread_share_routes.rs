//! Share snapshot routes (Wave 3 EP-12 S14, decision D10; ADR
//! `docs/decisions/conversation-share-model.md`, proposed):
//!
//! - `POST /v1/threads/:thread_id/snapshots`: take an immutable, redacted snapshot of a thread the
//!   caller owns (201).
//! - `GET /v1/threads/:thread_id/snapshots`: the caller's live snapshots of the thread.
//! - `GET /v1/snapshots/:snapshot_id`: one live snapshot with the copy it holds.
//! - `DELETE /v1/snapshots/:snapshot_id`: revoke a snapshot; its body is deleted.
//!
//! Session Core's `ThreadShareService` is the authority: it takes the snapshot from the export read
//! in the transaction that decides whether the thread may leave (EP-12 S9), and it decides who may
//! read one. The gateway forwards the caller's session bearer only, never an owner from the path.
//! Reads are owner-only in this version, because Control has no grant decision for
//! `conversation_snapshot` that Session Core can verify; the gateway writes no grant and never
//! holds an `authz:*` scope. Snapshots are for people: the route policy refuses a key principal
//! before these handlers run.
//!
//! Every route answers 404 `thread_shares_disabled` while `MODEL_GATEWAY_THREAD_SHARES` is not the
//! literal `true`. Taking a snapshot refuses a zero-retention or waived caller first (feature
//! `conversation_share`), as the export does; revoking and reading do not, since neither makes a
//! copy. Errors are the typed envelope `{error:{code,message,details}}` with `details` an object,
//! except a ZDR refusal, which keeps its catalogue body. A snapshot read is `Cache-Control:
//! no-store`, because its body is the owner's conversation.

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use mp_contracts::model_plane::v1::{
    CreateSnapshotRequest, ExportThreadResponse, ExportedMessage, ExportedRun, GetSnapshotRequest,
    ListSnapshotsRequest, RevokeSnapshotRequest, ThreadShareSnapshot,
};
use mp_contracts::zdr_refusal::Refusal;
use serde_json::{json, Value};

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::{authenticated_session_request, timestamp_to_rfc3339, HttpJsonError};
use crate::state::AppState;

/// The gateway's switch for the snapshot routes. Opt-in: only the literal `true` enables it. Read
/// at each call, as the other thread switches are.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_THREAD_SHARES";

/// The ZDR feature key a snapshot refusal names (`zdr-refusals.json`, EP-12 S3).
const ZDR_FEATURE: &str = "conversation_share";

/// The typed code while the switch is off. Session Core answers `Unavailable` with the same word
/// while its own store switch is off, and the routes give the same 404 for both.
const DISABLED: &str = "thread_shares_disabled";
const DISABLED_MESSAGE: &str = "conversation sharing is not enabled on this deployment";

/// The typed codes Session Core puts on its trailers, and the trailers that carry them.
const NOT_EXPORTABLE: &str = "thread_not_exportable";
const LIMIT_REACHED: &str = "snapshot_limit_reached";
const TOO_LARGE: &str = "snapshot_too_large";
const ERROR_CODE_TRAILER: &str = "x-mp-error-code";
const ERROR_REASON_TRAILER: &str = "x-mp-error-reason";

/// Whether the snapshot routes are on in this process.
#[must_use]
pub fn enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The routes, merged into the authed router beside the export.
pub(crate) fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/v1/threads/:thread_id/snapshots",
            axum::routing::post(create_snapshot).get(list_snapshots),
        )
        .route(
            "/v1/snapshots/:snapshot_id",
            axum::routing::get(get_snapshot).delete(revoke_snapshot),
        )
}

/// What a refusal of a route names when Session Core reports that nothing was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subject {
    Thread,
    Snapshot,
}

impl Subject {
    const fn not_found(self) -> (&'static str, &'static str) {
        match self {
            Self::Thread => ("thread_not_found", "thread not found"),
            Self::Snapshot => ("snapshot_not_found", "snapshot not found"),
        }
    }
}

/// `POST /v1/threads/:thread_id/snapshots`. The switch and the ZDR gate come first, then Session
/// Core's export read and write.
pub(crate) async fn create_snapshot(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(thread_id): Path<String>,
) -> Result<Response, HttpJsonError> {
    switch_gate()?;
    share_gate(&claims)?;
    let created = state
        .thread_share_client
        .clone()
        .create_snapshot(authenticated_session_request(
            CreateSnapshotRequest {
                thread_id: thread_id.trim().to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| share_error(&error, Subject::Thread))?
        .into_inner();
    let snapshot = created.snapshot.as_ref().ok_or_else(share_failed)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "data": { "snapshot": snapshot_json(snapshot) } })),
    )
        .into_response())
}

/// `GET /v1/threads/:thread_id/snapshots`.
pub(crate) async fn list_snapshots(
    State(state): State<AppState>,
    bearer: VerifiedSessionBearer,
    Path(thread_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    switch_gate()?;
    let listed = state
        .thread_share_client
        .clone()
        .list_snapshots(authenticated_session_request(
            ListSnapshotsRequest {
                thread_id: thread_id.trim().to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| share_error(&error, Subject::Thread))?
        .into_inner();
    let snapshots: Vec<Value> = listed.snapshots.iter().map(snapshot_json).collect();
    Ok(Json(json!({ "data": { "snapshots": snapshots } })))
}

/// `GET /v1/snapshots/:snapshot_id`. The body is the copy as it was taken; nothing here calls a
/// model, connector or tool.
pub(crate) async fn get_snapshot(
    State(state): State<AppState>,
    bearer: VerifiedSessionBearer,
    Path(snapshot_id): Path<String>,
) -> Result<Response, HttpJsonError> {
    switch_gate()?;
    let read = state
        .thread_share_client
        .clone()
        .get_snapshot(authenticated_session_request(
            GetSnapshotRequest {
                snapshot_id: snapshot_id.trim().to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| share_error(&error, Subject::Snapshot))?
        .into_inner();
    let snapshot = read.snapshot.as_ref().ok_or_else(share_failed)?;
    let body = read.body.as_ref().ok_or_else(share_failed)?;
    let data = json!({
        "snapshot": snapshot_json(snapshot),
        "body": body_json(body)?,
    });
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({ "data": data })),
    )
        .into_response())
}

/// `DELETE /v1/snapshots/:snapshot_id`. Revoking makes no copy, so a zero-retention owner may
/// revoke what they shared before.
pub(crate) async fn revoke_snapshot(
    State(state): State<AppState>,
    bearer: VerifiedSessionBearer,
    Path(snapshot_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    switch_gate()?;
    let revoked = state
        .thread_share_client
        .clone()
        .revoke_snapshot(authenticated_session_request(
            RevokeSnapshotRequest {
                snapshot_id: snapshot_id.trim().to_owned(),
            },
            &bearer,
        )?)
        .await
        .map_err(|error| share_error(&error, Subject::Snapshot))?
        .into_inner();
    let snapshot = revoked.snapshot.as_ref().ok_or_else(share_failed)?;
    Ok(Json(json!({ "data": { "snapshot": snapshot_json(snapshot) } })))
}

/// The switch first: a deployment without the feature answers the same for every caller.
fn switch_gate() -> Result<(), HttpJsonError> {
    gate_for(enabled())
}

/// The switch's answer, with the switch passed in so the rule is testable without the environment.
fn gate_for(enabled: bool) -> Result<(), HttpJsonError> {
    if enabled {
        Ok(())
    } else {
        Err(disabled())
    }
}

fn disabled() -> HttpJsonError {
    envelope(StatusCode::NOT_FOUND, DISABLED, DISABLED_MESSAGE, json!({}))
}

/// A zero-retention caller may not make a durable copy of a conversation (DEC-32). A waived caller
/// is refused here too, as a route mismatch, because its waiver names one route and this is not
/// it. Session Core refuses both again behind this gate.
fn share_gate(claims: &Claims) -> Result<(), HttpJsonError> {
    if crate::zdr_posture::zdr_or_refuse(claims, &crate::zdr_posture::Route::plain(false))? {
        return Err(crate::zdr_posture::http(&Refusal::feature_not_supported(
            ZDR_FEATURE,
        )));
    }
    Ok(())
}

/// Session Core's refusals as the routes answer them. A refusal the ZDR catalogue names keeps its
/// catalogue body; a thread that may not leave names its reason; something another person owns
/// reads as something that does not exist.
fn share_error(error: &tonic::Status, subject: Subject) -> HttpJsonError {
    if let Some(refusal) = Refusal::from_status(error) {
        return crate::zdr_posture::http(&refusal);
    }
    let code = trailer_text(error, ERROR_CODE_TRAILER);
    match error.code() {
        tonic::Code::Unavailable if error.message() == DISABLED => disabled(),
        tonic::Code::FailedPrecondition if code == Some(NOT_EXPORTABLE) => {
            let reason = trailer_text(error, ERROR_REASON_TRAILER);
            envelope(
                StatusCode::PRECONDITION_FAILED,
                NOT_EXPORTABLE,
                not_shareable_message(reason),
                reason.map_or_else(|| json!({}), |reason| json!({ "reason": reason })),
            )
        }
        tonic::Code::FailedPrecondition if code == Some(LIMIT_REACHED) => envelope(
            StatusCode::CONFLICT,
            LIMIT_REACHED,
            "this conversation already has the most live snapshots; revoke one first",
            json!({}),
        ),
        tonic::Code::FailedPrecondition if code == Some(TOO_LARGE) => envelope(
            StatusCode::PAYLOAD_TOO_LARGE,
            TOO_LARGE,
            "the conversation is too large to share as one snapshot",
            json!({}),
        ),
        tonic::Code::NotFound | tonic::Code::PermissionDenied => {
            let (code, message) = subject.not_found();
            envelope(StatusCode::NOT_FOUND, code, message, json!({}))
        }
        tonic::Code::InvalidArgument => envelope(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the snapshot request is not valid",
            json!({}),
        ),
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => envelope(
            StatusCode::SERVICE_UNAVAILABLE,
            "thread_shares_unavailable",
            "snapshots cannot be reached right now; try again",
            json!({}),
        ),
        _ => share_failed(),
    }
}

/// A trailer's text, when Session Core set it.
fn trailer_text<'a>(error: &'a tonic::Status, name: &str) -> Option<&'a str> {
    error.metadata().get(name)?.to_str().ok()
}

/// The sentence a refused thread shows, by the closed reason word Session Core names.
fn not_shareable_message(reason: Option<&str>) -> &'static str {
    match reason {
        Some("space_thread") => "a Space conversation cannot be shared",
        Some("non_persistent_run") => {
            "a run here was kept outside retention, so the conversation cannot be shared in full"
        }
        _ => "this conversation cannot be shared in full",
    }
}

/// A typed error body: `{error:{code,message,details}}`, with `details` always an object.
fn envelope(status: StatusCode, code: &str, message: &str, details: Value) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": details } })),
    )
}

/// Session Core answered with something the routes cannot show whole: a missing snapshot, or a
/// copy field that is not the JSON it should be. Refused rather than shown in part.
fn share_failed() -> HttpJsonError {
    envelope(
        StatusCode::BAD_GATEWAY,
        "thread_share_failed",
        "the snapshot could not be read",
        json!({}),
    )
}

/// One snapshot without its body. An absent digest or revocation time is `null`.
fn snapshot_json(snapshot: &ThreadShareSnapshot) -> Value {
    json!({
        "id": snapshot.snapshot_id,
        "thread_id": snapshot.thread_id,
        "title": snapshot.title,
        "created_at": timestamp_to_rfc3339(snapshot.created_at.clone()),
        "body_digest": non_empty(&snapshot.body_digest),
        "revoked_at": snapshot.revoked_at.clone().map(|at| timestamp_to_rfc3339(Some(at))),
    })
}

fn non_empty(text: &str) -> Option<&str> {
    (!text.is_empty()).then_some(text)
}

/// The copy a snapshot holds, in the shape of the export document's thread, messages, runs and
/// truncation flag, so the replay viewer reads both the same way.
fn body_json(body: &ExportThreadResponse) -> Result<Value, HttpJsonError> {
    let messages = body
        .messages
        .iter()
        .map(message_json)
        .collect::<Result<Vec<_>, _>>()?;
    let runs: Vec<Value> = body.runs.iter().map(run_json).collect();
    Ok(json!({
        "thread": {
            "id": body.thread_id,
            "title": body.title,
            "created_at": timestamp_to_rfc3339(body.created_at.clone()),
        },
        "messages": messages,
        "runs": runs,
        "truncated": body.truncated,
    }))
}

fn message_json(message: &ExportedMessage) -> Result<Value, HttpJsonError> {
    Ok(json!({
        "id": message.message_id,
        "role": message.role,
        "content": message.content,
        "created_at": timestamp_to_rfc3339(message.created_at.clone()),
        "artifacts": json_field(&message.artifacts_json, Value::is_array, json!([]))?,
        "citations": json_field(&message.citations_json, Value::is_array, json!([]))?,
        "metadata": json_field(&message.metadata_json, Value::is_object, json!({}))?,
    }))
}

fn run_json(run: &ExportedRun) -> Value {
    json!({
        "id": run.run_id,
        "status": run.status,
        "origin": run.origin,
        "model": run.model,
        "input_tokens": run.input_tokens,
        "output_tokens": run.output_tokens,
        "created_at": timestamp_to_rfc3339(run.created_at.clone()),
    })
}

/// A field the copy carries as JSON text: `empty` when there is none, refused when it is not JSON
/// of the expected kind.
fn json_field(raw: &str, kind: fn(&Value) -> bool, empty: Value) -> Result<Value, HttpJsonError> {
    if raw.is_empty() {
        return Ok(empty);
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(value) if kind(&value) => Ok(value),
        _ => Err(share_failed()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, middleware};
    use mp_contracts::zdr_refusal::code;
    use tonic::metadata::MetadataValue;
    use tower::ServiceExt;

    fn claims(zdr: bool) -> Claims {
        serde_json::from_value(json!({
            "sub": "user-a",
            "iss": "auth-core",
            "exp": 4_102_444_800_i64,
            "org_id": "org-a",
            "user_id": "user-a",
            "zdr": zdr,
            "scopes": ["session:read"],
        }))
        .expect("a user token decodes")
    }

    fn key_claims() -> Claims {
        serde_json::from_value(json!({
            "sub": "apikey:key-1",
            "iss": "auth-core",
            "exp": 4_102_444_800_i64,
            "org_id": "org-a",
            "zdr": false,
            "principal_type": "api_key",
            "key_id": "key-1",
            "acting_user_id": "user-a",
            "scopes": ["agent:runs"],
        }))
        .expect("an api key token decodes")
    }

    fn with_code(status: tonic::Status, code: &'static str) -> tonic::Status {
        let mut status = status;
        status
            .metadata_mut()
            .insert(ERROR_CODE_TRAILER, MetadataValue::from_static(code));
        status
    }

    #[test]
    fn switch_off_routes_answer_disabled() {
        let (status_code, Json(body)) = gate_for(false).expect_err("a switched-off route refuses");
        assert_eq!(status_code, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], DISABLED);
        assert!(body["error"]["details"].is_object());
        assert!(gate_for(true).is_ok());

        let store_off = tonic::Status::unavailable(DISABLED);
        let (status_code, Json(body)) = share_error(&store_off, Subject::Snapshot);
        assert_eq!(status_code, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], DISABLED, "the store's switch reads the same");
    }

    #[test]
    fn zdr_caller_is_refused_before_any_snapshot() {
        let (status_code, Json(body)) =
            share_gate(&claims(true)).expect_err("a ZDR caller is refused");
        assert_eq!(status_code, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["feature"], ZDR_FEATURE);
        assert!(share_gate(&claims(false)).is_ok());
    }

    #[test]
    fn zdr_refusal_keeps_its_catalogue_body() {
        let status = Refusal::feature_not_supported(ZDR_FEATURE)
            .grpc_status(tonic::Code::FailedPrecondition);
        let (status_code, Json(body)) = share_error(&status, Subject::Thread);
        assert_eq!(status_code, StatusCode::PRECONDITION_FAILED);
        assert_eq!(body["error"]["code"], code::FEATURE_NOT_SUPPORTED);
        assert_eq!(body["error"]["feature"], ZDR_FEATURE);
    }

    #[test]
    fn refusals_are_typed_envelopes_with_object_details() {
        let mut not_exportable = with_code(
            tonic::Status::failed_precondition(NOT_EXPORTABLE),
            NOT_EXPORTABLE,
        );
        not_exportable.metadata_mut().insert(
            ERROR_REASON_TRAILER,
            MetadataValue::from_static("space_thread"),
        );
        let cases = [
            (
                not_exportable,
                Subject::Thread,
                StatusCode::PRECONDITION_FAILED,
                NOT_EXPORTABLE,
            ),
            (
                with_code(tonic::Status::failed_precondition("cap"), LIMIT_REACHED),
                Subject::Thread,
                StatusCode::CONFLICT,
                LIMIT_REACHED,
            ),
            (
                with_code(tonic::Status::failed_precondition("big"), TOO_LARGE),
                Subject::Thread,
                StatusCode::PAYLOAD_TOO_LARGE,
                TOO_LARGE,
            ),
            (
                tonic::Status::not_found("thread not found"),
                Subject::Thread,
                StatusCode::NOT_FOUND,
                "thread_not_found",
            ),
            (
                tonic::Status::permission_denied("not the owner"),
                Subject::Snapshot,
                StatusCode::NOT_FOUND,
                "snapshot_not_found",
            ),
            (
                tonic::Status::invalid_argument("snapshot_id is required"),
                Subject::Snapshot,
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
            (
                tonic::Status::unavailable("connection refused"),
                Subject::Snapshot,
                StatusCode::SERVICE_UNAVAILABLE,
                "thread_shares_unavailable",
            ),
            (
                tonic::Status::internal("snapshot body is unreadable"),
                Subject::Snapshot,
                StatusCode::BAD_GATEWAY,
                "thread_share_failed",
            ),
        ];
        for (status, subject, expected_status, expected_code) in cases {
            let (status_code, Json(body)) = share_error(&status, subject);
            assert_eq!(status_code, expected_status, "{expected_code}");
            assert_eq!(body["error"]["code"], expected_code);
            assert!(body["error"]["message"].is_string());
            assert!(body["error"]["details"].is_object(), "{expected_code}");
        }
    }

    #[test]
    fn a_refused_thread_names_its_reason() {
        let mut status = with_code(
            tonic::Status::failed_precondition(NOT_EXPORTABLE),
            NOT_EXPORTABLE,
        );
        status.metadata_mut().insert(
            ERROR_REASON_TRAILER,
            MetadataValue::from_static("non_persistent_run"),
        );
        let (_, Json(body)) = share_error(&status, Subject::Thread);
        assert_eq!(body["error"]["details"]["reason"], "non_persistent_run");
    }

    #[test]
    fn snapshot_json_names_absent_digest_and_revocation_as_null() {
        let live = ThreadShareSnapshot {
            snapshot_id: "snap_1".to_owned(),
            thread_id: "thread-1".to_owned(),
            title: "Notes".to_owned(),
            created_at: None,
            body_digest: "sha256:ab".to_owned(),
            revoked_at: None,
        };
        let value = snapshot_json(&live);
        assert_eq!(value["id"], "snap_1");
        assert_eq!(value["body_digest"], "sha256:ab");
        assert!(value["revoked_at"].is_null());

        let revoked = ThreadShareSnapshot {
            body_digest: String::new(),
            revoked_at: Some(prost_types::Timestamp {
                seconds: 1_793_000_000,
                nanos: 0,
            }),
            ..live
        };
        let value = snapshot_json(&revoked);
        assert!(value["body_digest"].is_null());
        assert!(value["revoked_at"].is_string());
    }

    #[test]
    fn body_json_parses_copy_fields_and_refuses_broken_ones() {
        let message = ExportedMessage {
            message_id: "m1".to_owned(),
            role: "assistant".to_owned(),
            content: "Answer".to_owned(),
            created_at: None,
            artifacts_json: r#"[{"id":"a1","content":"Text"}]"#.to_owned(),
            citations_json: String::new(),
            metadata_json: String::new(),
        };
        let body = ExportThreadResponse {
            thread_id: "thread-1".to_owned(),
            title: "Notes".to_owned(),
            created_at: None,
            messages: vec![message.clone()],
            runs: Vec::new(),
            truncated: false,
        };
        let value = body_json(&body).expect("the copy renders");
        assert_eq!(value["thread"]["id"], "thread-1");
        assert_eq!(value["messages"][0]["artifacts"][0]["content"], "Text");
        assert_eq!(value["messages"][0]["citations"], json!([]));
        assert_eq!(value["messages"][0]["metadata"], json!({}));
        assert_eq!(value["truncated"], false);

        let broken = ExportThreadResponse {
            messages: vec![ExportedMessage {
                metadata_json: "[]".to_owned(),
                ..message
            }],
            ..body
        };
        let (status_code, Json(error)) = body_json(&broken).expect_err("an array is not metadata");
        assert_eq!(status_code, StatusCode::BAD_GATEWAY);
        assert_eq!(error["error"]["code"], "thread_share_failed");
    }

    #[tokio::test]
    async fn api_key_principal_is_refused() {
        for (method, uri) in [
            ("POST", "/v1/threads/thread-1/snapshots"),
            ("GET", "/v1/threads/thread-1/snapshots"),
            ("GET", "/v1/snapshots/snap_1"),
            ("DELETE", "/v1/snapshots/snap_1"),
        ] {
            let app = router()
                .layer(middleware::from_fn(crate::auth::authorize_principal_route))
                .layer(Extension(key_claims()))
                .with_state(AppState::new());
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("a request");
            let response = app.oneshot(request).await.expect("the router answers");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {uri}");
        }
    }

    #[tokio::test]
    async fn every_route_answers_disabled_while_the_switch_is_off() {
        for (method, uri) in [
            ("POST", "/v1/threads/thread-1/snapshots"),
            ("GET", "/v1/threads/thread-1/snapshots"),
            ("GET", "/v1/snapshots/snap_1"),
            ("DELETE", "/v1/snapshots/snap_1"),
        ] {
            let app = router()
                .layer(Extension(claims(false)))
                .layer(Extension(VerifiedSessionBearer::for_test("session-token")))
                .with_state(AppState::new());
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("a request");
            let response = app.oneshot(request).await.expect("the router answers");
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {uri}");
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("a body");
            let body: Value = serde_json::from_slice(&bytes).expect("a JSON envelope");
            assert_eq!(body["error"]["code"], DISABLED, "{method} {uri}");
        }
    }
}
