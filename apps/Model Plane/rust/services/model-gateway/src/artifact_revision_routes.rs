//! Read routes over session-core's artifact revision store, and the composer's capability read
//! (Wave 3 EP-3 S3).
//!
//! - `GET /v1/threads/:thread_id/artifacts`: the head of every artifact in the thread, no text.
//! - `GET /v1/threads/:thread_id/artifacts/:artifact_id/revisions?before_version=&limit=`: one
//!   artifact's revisions newest first, text included, at most [`MAX_PAGE`] per page.
//! - `GET /v1/chat/composer-capabilities`: which composer and panel features this gateway serves,
//!   from its own switches, so the SPA never reads a build-time flag for server behaviour.
//!
//! The history routes answer 404 `artifact_history_unavailable` unless
//! `MODEL_GATEWAY_ARTIFACT_REVISIONS` is `on`: in `off` and `shadow` nothing the user sees changes.
//! Session Core is the authority on who may read a thread; the gateway forwards the verified
//! session bearer and the claims' org only, never an owner from the path or query. Another
//! person's thread and a thread in another org both answer the same 404 as a missing one, so the
//! route says nothing about which threads exist. A zero-retention credential is refused with the
//! existing 412 before any call: a ZDR turn writes no revision, so there is nothing to read, and
//! the refusal keeps that explicit. `has_more` rides inside `data` because the SPA's request
//! helper unwraps `data` and drops `meta`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use mp_contracts::model_plane::v1::{ArtifactRevision, ListArtifactRevisionsRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::artifact_revisions::ArtifactRevisionsMode;
use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::{
    authenticated_session_request, require_non_zdr_durable_mutation, session_thread_error,
    timestamp_to_rfc3339, HttpJsonError,
};
use crate::state::AppState;

/// Revisions per page: each carries up to 200,000 characters of text.
const MAX_PAGE: u32 = 20;

/// Heads per thread read. Session Core reads more than 50 as 50.
const MAX_HEADS: u32 = 50;

/// What session-core answers while its own store switch is off.
const STORE_DISABLED: &str = "artifact_revisions_disabled";

/// The routes, merged at the end of the authed router.
pub(crate) fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/v1/threads/:thread_id/artifacts",
            axum::routing::get(list_artifact_heads),
        )
        .route(
            "/v1/threads/:thread_id/artifacts/:artifact_id/revisions",
            axum::routing::get(list_artifact_revisions),
        )
        .route(
            "/v1/chat/composer-capabilities",
            axum::routing::get(composer_capabilities),
        )
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RevisionsQuery {
    /// Only versions below this one; absent or 0 starts at the head.
    before_version: Option<u32>,
    /// At most [`MAX_PAGE`]; absent or 0 means a full page.
    limit: Option<u32>,
}

/// One stored revision as the panel reads it. The editor's subject id is not sent: the owner is
/// the only reader, and `editor_kind` is what the stepper labels.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct RevisionJson {
    id: String,
    artifact_id: String,
    version: u32,
    kind: String,
    title: String,
    /// Absent on a heads read, which carries no text.
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    editor_kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    run_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    source_message_id: String,
    /// For a restore, the version whose text was restored.
    #[serde(skip_serializing_if = "Option::is_none")]
    restored_from_version: Option<u32>,
    content_sha256: String,
    created_at: String,
}

impl RevisionJson {
    fn from_proto(revision: ArtifactRevision, with_content: bool) -> Self {
        Self {
            id: revision.id,
            artifact_id: revision.artifact_id,
            version: revision.version,
            kind: revision.kind,
            title: revision.title,
            content: with_content.then_some(revision.content),
            editor_kind: revision.editor_kind,
            run_id: revision.run_id,
            source_message_id: revision.source_message_id,
            restored_from_version: (revision.restored_from_version > 0)
                .then_some(revision.restored_from_version),
            content_sha256: revision.content_sha256,
            created_at: timestamp_to_rfc3339(revision.created_at),
        }
    }
}

/// The 404 every history route answers while the switch is not `on`, or while session-core's
/// own store switch is off.
fn history_unavailable() -> HttpJsonError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": {
                "code": "artifact_history_unavailable",
                "message": "artifact history is not enabled on this deployment",
                "details": {},
            }
        })),
    )
}

/// The gate in front of every history read: the switch first (a deployment without the feature
/// answers the same for every caller), then the ZDR refusal.
fn history_gate(state: &AppState, claims: &Claims) -> Result<(), HttpJsonError> {
    if state.artifact_revisions_mode != ArtifactRevisionsMode::On {
        return Err(history_unavailable());
    }
    require_non_zdr_durable_mutation(claims)
}

/// Session Core's refusals as the panel reads them. Not found and permission denied are one 404,
/// so a thread someone else owns reads like one that does not exist.
fn history_error(error: &tonic::Status) -> HttpJsonError {
    match error.code() {
        tonic::Code::Unavailable if error.message() == STORE_DISABLED => history_unavailable(),
        tonic::Code::NotFound | tonic::Code::PermissionDenied => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "code": "thread_not_found",
                    "message": "thread not found",
                    "details": {},
                }
            })),
        ),
        _ => session_thread_error("session-core list_artifact_revisions failed", error),
    }
}

/// A page size the store accepts: absent or 0 is a full page, anything past it is clamped.
fn page_limit(raw: Option<u32>) -> u32 {
    match raw {
        None | Some(0) => MAX_PAGE,
        Some(limit) => limit.min(MAX_PAGE),
    }
}

async fn list(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    request: ListArtifactRevisionsRequest,
) -> Result<(Vec<ArtifactRevision>, bool), HttpJsonError> {
    let response = state
        .artifact_revision_client
        .clone()
        .list_artifact_revisions(authenticated_session_request(request, bearer)?)
        .await
        .map_err(|error| history_error(&error))?
        .into_inner();
    Ok((response.revisions, response.has_more))
}

pub(crate) async fn list_artifact_heads(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path(thread_id): Path<String>,
) -> Result<Json<Value>, HttpJsonError> {
    history_gate(&state, &claims)?;
    let (heads, has_more) = list(
        &state,
        &bearer,
        ListArtifactRevisionsRequest {
            org_id: claims.org_id.clone(),
            thread_id,
            artifact_id: String::new(),
            include_content: false,
            limit: MAX_HEADS,
            before_version: 0,
        },
    )
    .await?;
    let artifacts: Vec<RevisionJson> = heads
        .into_iter()
        .map(|head| RevisionJson::from_proto(head, false))
        .collect();
    Ok(Json(json!({
        "data": { "artifacts": artifacts, "has_more": has_more }
    })))
}

pub(crate) async fn list_artifact_revisions(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((thread_id, artifact_id)): Path<(String, String)>,
    Query(query): Query<RevisionsQuery>,
) -> Result<Json<Value>, HttpJsonError> {
    history_gate(&state, &claims)?;
    let (revisions, has_more) = list(
        &state,
        &bearer,
        ListArtifactRevisionsRequest {
            org_id: claims.org_id.clone(),
            thread_id,
            artifact_id,
            include_content: true,
            limit: page_limit(query.limit),
            before_version: query.before_version.unwrap_or(0),
        },
    )
    .await?;
    let revisions: Vec<RevisionJson> = revisions
        .into_iter()
        .map(|revision| RevisionJson::from_proto(revision, true))
        .collect();
    Ok(Json(json!({
        "data": { "revisions": revisions, "has_more": has_more }
    })))
}

/// What the composer and the Output panel may offer, from this gateway's own switches. Only
/// `artifact_history` has a server half today; each later EP-3 slice wires its own field here.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct ComposerCapabilities {
    artifact_history: bool,
    artifact_editing: bool,
    context_refs: bool,
    thread_references: bool,
    selection_edit: bool,
}

fn capabilities_for(mode: ArtifactRevisionsMode, zdr: bool) -> ComposerCapabilities {
    ComposerCapabilities {
        // A ZDR caller would only be refused the read, so the panel is told not to ask.
        artifact_history: mode == ArtifactRevisionsMode::On && !zdr,
        artifact_editing: false,
        context_refs: false,
        thread_references: false,
        selection_edit: false,
    }
}

pub(crate) async fn composer_capabilities(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Json<Value> {
    // A posture that cannot be decided reads as ZDR: a capability is never offered on a guess.
    let zdr = crate::zdr_posture::zdr_or_refuse(
        &claims,
        &crate::zdr_posture::Route::plain(false),
    )
    .unwrap_or(true);
    Json(json!({ "data": capabilities_for(state.artifact_revisions_mode, zdr) }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::model_plane::v1::{
        artifact_revision_service_client::ArtifactRevisionServiceClient,
        artifact_revision_service_server::{
            ArtifactRevisionService, ArtifactRevisionServiceServer,
        },
        AppendArtifactRevisionRequest, AppendArtifactRevisionResponse, GetArtifactRevisionRequest,
        GetArtifactRevisionResponse, ListArtifactRevisionsResponse,
    };
    use std::sync::{Arc, Mutex};

    const OWNER: &str = "user-1";

    /// A revision store double: answers the owner of `org-1` with three revisions of one
    /// artifact, and anyone else the way session-core does (not found across orgs, permission
    /// denied inside one). Keeps every list request it saw.
    #[derive(Clone, Default)]
    struct FakeStore {
        seen: Arc<Mutex<Vec<ListArtifactRevisionsRequest>>>,
        disabled: bool,
    }

    fn revision(version: u32) -> ArtifactRevision {
        ArtifactRevision {
            id: format!("rev-{version}"),
            thread_id: "thread-1".to_owned(),
            artifact_id: "report".to_owned(),
            version,
            kind: "document".to_owned(),
            title: "Report".to_owned(),
            content: format!("text {version}"),
            editor_kind: if version == 2 { "user" } else { "model" }.to_owned(),
            editor_subject_id: OWNER.to_owned(),
            run_id: String::new(),
            source_message_id: String::new(),
            restored_from_version: 0,
            content_sha256: "ab".to_owned(),
            created_at: None,
        }
    }

    #[tonic::async_trait]
    impl ArtifactRevisionService for FakeStore {
        async fn append_artifact_revision(
            &self,
            _request: tonic::Request<AppendArtifactRevisionRequest>,
        ) -> Result<tonic::Response<AppendArtifactRevisionResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }

        async fn list_artifact_revisions(
            &self,
            request: tonic::Request<ListArtifactRevisionsRequest>,
        ) -> Result<tonic::Response<ListArtifactRevisionsResponse>, tonic::Status> {
            if self.disabled {
                return Err(tonic::Status::unavailable(STORE_DISABLED));
            }
            let bearer = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let request = request.into_inner();
            self.seen.lock().unwrap().push(request.clone());
            if request.org_id != "org-1" {
                return Err(tonic::Status::not_found("thread not found"));
            }
            if bearer != format!("Bearer {OWNER}") {
                return Err(tonic::Status::permission_denied("not the thread owner"));
            }
            let all: Vec<ArtifactRevision> = (1..=3)
                .rev()
                .map(revision)
                .filter(|r| request.before_version == 0 || r.version < request.before_version)
                .map(|mut r| {
                    if !request.include_content {
                        r.content.clear();
                    }
                    r
                })
                .collect();
            let limit = request.limit as usize;
            Ok(tonic::Response::new(ListArtifactRevisionsResponse {
                has_more: all.len() > limit,
                revisions: all.into_iter().take(limit).collect(),
            }))
        }

        async fn get_artifact_revision(
            &self,
            _request: tonic::Request<GetArtifactRevisionRequest>,
        ) -> Result<tonic::Response<GetArtifactRevisionResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("not used"))
        }
    }

    async fn state_with(fake: FakeStore, mode: ArtifactRevisionsMode) -> AppState {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(ArtifactRevisionServiceServer::new(fake))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await;
        });
        let mut state = AppState::new();
        state.artifact_revision_client =
            ArtifactRevisionServiceClient::connect(format!("http://{address}"))
                .await
                .unwrap();
        state.artifact_revisions_mode = mode;
        state
    }

    fn claims(org: &str, user: &str, zdr: bool) -> Claims {
        serde_json::from_value(json!({
            "sub": user,
            "iss": "https://auth.test/model",
            "aud": "model-gateway",
            "exp": 4_102_444_800_i64,
            "nbf": 1_700_000_000_i64,
            "org_id": org,
            "user_id": user,
            "principal_type": "user",
            "zdr": zdr,
            "org_zdr": zdr,
        }))
        .expect("a minted token's payload is valid Claims")
    }

    async fn revisions_as(
        state: &AppState,
        org: &str,
        user: &str,
        query: RevisionsQuery,
    ) -> Result<Json<Value>, HttpJsonError> {
        list_artifact_revisions(
            State(state.clone()),
            Extension(claims(org, user, false)),
            VerifiedSessionBearer::for_test(user),
            Path(("thread-1".to_owned(), "report".to_owned())),
            Query(query),
        )
        .await
    }

    fn error_code(error: &HttpJsonError) -> (StatusCode, String) {
        (
            error.0,
            error.1 .0["error"]["code"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        )
    }

    #[tokio::test]
    async fn history_routes_404_unless_on() {
        for mode in [ArtifactRevisionsMode::Off, ArtifactRevisionsMode::Shadow] {
            let fake = FakeStore::default();
            let state = state_with(fake.clone(), mode).await;
            let error = revisions_as(&state, "org-1", OWNER, RevisionsQuery::default())
                .await
                .expect_err("history is off");
            assert_eq!(
                error_code(&error),
                (StatusCode::NOT_FOUND, "artifact_history_unavailable".to_owned())
            );
            assert!(error.1 .0["error"]["details"].is_object());
            let heads = list_artifact_heads(
                State(state.clone()),
                Extension(claims("org-1", OWNER, false)),
                VerifiedSessionBearer::for_test(OWNER),
                Path("thread-1".to_owned()),
            )
            .await
            .expect_err("heads are off too");
            assert_eq!(heads.0, StatusCode::NOT_FOUND);
            assert!(fake.seen.lock().unwrap().is_empty(), "{mode:?} made no call");
        }
    }

    #[tokio::test]
    async fn a_disabled_store_reads_as_unavailable_history() {
        let fake = FakeStore {
            disabled: true,
            ..FakeStore::default()
        };
        let state = state_with(fake, ArtifactRevisionsMode::On).await;
        let error = revisions_as(&state, "org-1", OWNER, RevisionsQuery::default())
            .await
            .expect_err("store off");
        assert_eq!(
            error_code(&error),
            (StatusCode::NOT_FOUND, "artifact_history_unavailable".to_owned())
        );
    }

    #[tokio::test]
    async fn history_is_owner_only() {
        let fake = FakeStore::default();
        let state = state_with(fake.clone(), ArtifactRevisionsMode::On).await;
        let other_user = revisions_as(&state, "org-1", "user-2", RevisionsQuery::default())
            .await
            .expect_err("another person in the org");
        let other_org = revisions_as(&state, "org-2", "user-3", RevisionsQuery::default())
            .await
            .expect_err("another org");
        assert_eq!(error_code(&other_user), error_code(&other_org));
        assert_eq!(
            error_code(&other_user),
            (StatusCode::NOT_FOUND, "thread_not_found".to_owned())
        );
        // The org forwarded is the caller's own; nothing in the request names another.
        let seen = fake.seen.lock().unwrap().clone();
        assert_eq!(seen[0].org_id, "org-1");
        assert_eq!(seen[1].org_id, "org-2");
    }

    #[tokio::test]
    async fn history_refuses_a_zdr_credential_before_any_call() {
        let fake = FakeStore::default();
        let state = state_with(fake.clone(), ArtifactRevisionsMode::On).await;
        let error = list_artifact_revisions(
            State(state),
            Extension(claims("org-1", OWNER, true)),
            VerifiedSessionBearer::for_test(OWNER),
            Path(("thread-1".to_owned(), "report".to_owned())),
            Query(RevisionsQuery::default()),
        )
        .await
        .expect_err("zdr");
        assert_eq!(error.0, StatusCode::PRECONDITION_FAILED);
        assert!(fake.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn history_pages_by_before_version() {
        let fake = FakeStore::default();
        let state = state_with(fake.clone(), ArtifactRevisionsMode::On).await;
        let Json(first) = revisions_as(
            &state,
            "org-1",
            OWNER,
            RevisionsQuery {
                before_version: None,
                limit: Some(2),
            },
        )
        .await
        .expect("first page");
        let versions: Vec<u64> = first["data"]["revisions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["version"].as_u64().unwrap())
            .collect();
        assert_eq!(versions, [3, 2]);
        assert_eq!(first["data"]["has_more"], true);
        assert_eq!(first["data"]["revisions"][1]["editor_kind"], "user");
        assert_eq!(first["data"]["revisions"][0]["content"], "text 3");
        assert!(first["data"]["revisions"][0].get("editor_subject_id").is_none());

        let Json(second) = revisions_as(
            &state,
            "org-1",
            OWNER,
            RevisionsQuery {
                before_version: Some(2),
                limit: Some(500),
            },
        )
        .await
        .expect("second page");
        assert_eq!(second["data"]["revisions"].as_array().unwrap().len(), 1);
        assert_eq!(second["data"]["has_more"], false);
        let seen = fake.seen.lock().unwrap().clone();
        assert_eq!(seen[1].before_version, 2);
        assert_eq!(seen[1].limit, MAX_PAGE, "a page past the cap is clamped");
        assert!(seen.iter().all(|r| r.include_content));
    }

    #[tokio::test]
    async fn heads_carry_no_text() {
        let fake = FakeStore::default();
        let state = state_with(fake.clone(), ArtifactRevisionsMode::On).await;
        let Json(heads) = list_artifact_heads(
            State(state),
            Extension(claims("org-1", OWNER, false)),
            VerifiedSessionBearer::for_test(OWNER),
            Path("thread-1".to_owned()),
        )
        .await
        .expect("heads");
        let first = &heads["data"]["artifacts"][0];
        assert!(first.get("content").is_none());
        assert_eq!(first["content_sha256"], "ab");
        let seen = fake.seen.lock().unwrap().clone();
        assert!(seen[0].artifact_id.is_empty());
        assert!(!seen[0].include_content);
        assert_eq!(seen[0].limit, MAX_HEADS);
    }

    #[test]
    fn page_limit_defaults_and_clamps() {
        assert_eq!(page_limit(None), MAX_PAGE);
        assert_eq!(page_limit(Some(0)), MAX_PAGE);
        assert_eq!(page_limit(Some(5)), 5);
        assert_eq!(page_limit(Some(21)), MAX_PAGE);
    }

    #[test]
    fn capabilities_reflect_switches() {
        assert!(!capabilities_for(ArtifactRevisionsMode::Off, false).artifact_history);
        assert!(!capabilities_for(ArtifactRevisionsMode::Shadow, false).artifact_history);
        assert!(capabilities_for(ArtifactRevisionsMode::On, false).artifact_history);
        assert!(!capabilities_for(ArtifactRevisionsMode::On, true).artifact_history);
        let on = capabilities_for(ArtifactRevisionsMode::On, false);
        assert!(!on.artifact_editing && !on.context_refs && !on.thread_references && !on.selection_edit);
    }

    #[tokio::test]
    async fn the_capabilities_route_answers_every_key_in_the_envelope() {
        let state = state_with(FakeStore::default(), ArtifactRevisionsMode::On).await;
        let Json(body) =
            composer_capabilities(State(state), Extension(claims("org-1", OWNER, false))).await;
        assert_eq!(
            body,
            json!({ "data": {
                "artifact_history": true,
                "artifact_editing": false,
                "context_refs": false,
                "thread_references": false,
                "selection_edit": false,
            }})
        );
    }

    #[test]
    fn a_restore_names_its_source_and_a_model_revision_does_not() {
        let mut restored = revision(4);
        restored.editor_kind = "restore".to_owned();
        restored.restored_from_version = 2;
        let json = serde_json::to_value(RevisionJson::from_proto(restored, true)).unwrap();
        assert_eq!(json["restored_from_version"], 2);
        let model = serde_json::to_value(RevisionJson::from_proto(revision(3), true)).unwrap();
        assert!(model.get("restored_from_version").is_none());
        assert!(model.get("run_id").is_none());
    }
}
