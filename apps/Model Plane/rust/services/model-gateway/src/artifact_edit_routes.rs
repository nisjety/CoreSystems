//! The person's own edits of a thread artifact in the Output panel (Wave 3 EP-3 S4).
//!
//! `PUT /v1/threads/:thread_id/artifacts/:artifact_id` saves the text the person typed as a new
//! revision (editor `user`); `POST .../restore` copies an older revision's text and title into a
//! new one (editor `restore`). Both are a compare-and-set on the head the panel last showed
//! (`expected_version`): session-core's `ArtifactRevisionService` holds the head and decides, so
//! two tabs or two gateway replicas cannot both win. On success this replica's commit store takes
//! the new head under `lock_commit`, so its next turn reads the person's text; another replica
//! reads it through S3's turn-start seeding from the table.
//!
//! Off unless `MODEL_GATEWAY_ARTIFACT_EDITING` is the literal `true`, and that is refused at
//! startup unless `MODEL_GATEWAY_ARTIFACT_REVISIONS` is `on`: an edit lives only in the revision
//! table, so editing over a shadow or absent table would save text no turn ever reads (the D2-578
//! two-switch trap, made loud). Off, both routes answer 404 `artifact_editing_unavailable`.
//!
//! Owner decisions taken by their recommendation (EP-3 section 3): the store is thread-owned in
//! session-core (OD-1, OD-2); a Space thread is read-only for edits (OD-9, 403
//! `artifact_edit_space_thread`); a thread that holds a waived run is refused (OD-11, DEC-32, 403
//! `artifact_edit_waived_thread`). A ZDR credential is refused here with the durable-mutation 412
//! before anything is read. Only text kinds are editable (`document`, `code`, `html`): a deck
//! keeps its own structure and is never edited as text (C-B8). An edit is refused while a run on
//! the thread is running (409 `artifact_edit_run_active`); a run that starts while the editor is
//! open fails its commit with the retryable `artifact_revision_conflict` instead. Content is never
//! logged.

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    routing::{post, put},
    Extension, Json, Router,
};
use mp_contracts::model_plane::v1::{
    AppendArtifactRevisionRequest, AppendOutcome, ArtifactRevision, GetArtifactRevisionRequest,
    ListRunsRequest,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::artifact_revisions::ArtifactRevisionsMode;
use crate::artifacts::MAX_TEXT_ARTIFACT_CHARS;
use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::{
    authenticated_session_request, require_non_zdr_durable_mutation, timestamp_to_rfc3339,
    HttpJsonError,
};
use crate::state::AppState;

/// The switch. Only the literal `true` turns editing on.
pub const EDITING_ENV: &str = "MODEL_GATEWAY_ARTIFACT_EDITING";

/// The kinds a person may edit as source text. Everything else (a deck, a spreadsheet, an image,
/// a generated file) is refused: its text is not what the person sees.
const EDITABLE_KINDS: [&str; 3] = ["document", "code", "html"];
/// The run phase that blocks an edit: the run's commit would race the person's save.
const RUNNING: &str = "running";

/// session-core's refusals this route turns into its own codes.
const REVISION_MISMATCH: &str = "revision_mismatch";
const REVISION_MISSING: &str = "revision_missing";
const REVISION_LIMIT: &str = "artifact_revision_limit";
const SPACE_THREAD: &str = "artifact_edit_space_thread";
const STORE_WAIVED_THREAD: &str = "artifact_revisions_waived_thread";
const WAIVED_THREAD: &str = "artifact_edit_waived_thread";

/// The strict parse of [`EDITING_ENV`] against the revision mode. Only the literal `true`
/// enables; any other value (unset included) is off, as every gateway switch reads.
///
/// # Errors
///
/// `true` while the revision mode is not `on`.
pub fn parse_editing(raw: Option<&str>, mode: ArtifactRevisionsMode) -> Result<bool, String> {
    if raw != Some("true") {
        return Ok(false);
    }
    if mode != ArtifactRevisionsMode::On {
        return Err(format!(
            "{EDITING_ENV}=true requires {} to be on",
            crate::artifact_revisions::MODE_ENV
        ));
    }
    Ok(true)
}

/// Read [`EDITING_ENV`] once at startup, after the revision mode.
///
/// # Errors
///
/// Editing is switched on without the revision table being on.
pub fn editing_from_env(mode: ArtifactRevisionsMode) -> anyhow::Result<bool> {
    parse_editing(std::env::var(EDITING_ENV).ok().as_deref(), mode).map_err(anyhow::Error::msg)
}

/// The two write routes, merged into the authenticated router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/threads/:thread_id/artifacts/:artifact_id",
            put(save_artifact_edit),
        )
        .route(
            "/v1/threads/:thread_id/artifacts/:artifact_id/restore",
            post(restore_artifact_revision),
        )
}

/// `PUT` body: the full new text, an optional new title, and the head the editor opened.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SaveArtifactBody {
    content: String,
    #[serde(default)]
    title: Option<String>,
    expected_version: u32,
}

/// `POST .../restore` body: the version to copy and the head the panel showed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestoreArtifactBody {
    version: u32,
    expected_version: u32,
}

/// What a write asks session-core to append, after the head was checked.
struct PlannedEdit {
    kind: String,
    title: String,
    content: String,
    editor_kind: &'static str,
    restored_from_version: u32,
    expected_version: u32,
}

/// `PUT /v1/threads/:thread_id/artifacts/:artifact_id` - save the person's text as a new revision.
pub(crate) async fn save_artifact_edit(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((thread_id, artifact_id)): Path<(String, String)>,
    body: Result<Json<SaveArtifactBody>, JsonRejection>,
) -> Result<Json<Value>, HttpJsonError> {
    admit(&state, &claims)?;
    let Json(body) = body.map_err(|_| {
        invalid("the body must be { content, title?, expected_version }")
    })?;
    if body.content.is_empty() || body.content.chars().count() > MAX_TEXT_ARTIFACT_CHARS {
        return Err(invalid(
            "content must be between 1 and 200000 characters",
        ));
    }
    ensure_no_running_run(&state, &bearer, &thread_id).await?;
    let head = read_head(&state, &claims, &bearer, &thread_id, &artifact_id).await?;
    check_editable(&head, body.expected_version)?;
    let title = body
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map_or_else(|| head.title.clone(), ToOwned::to_owned);
    let planned = PlannedEdit {
        kind: head.kind,
        title,
        content: body.content,
        editor_kind: "user",
        restored_from_version: 0,
        expected_version: body.expected_version,
    };
    append_and_publish(&state, &claims, &bearer, &thread_id, &artifact_id, planned).await
}

/// `POST /v1/threads/:thread_id/artifacts/:artifact_id/restore` - make an older revision's text
/// the newest one.
pub(crate) async fn restore_artifact_revision(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    Path((thread_id, artifact_id)): Path<(String, String)>,
    body: Result<Json<RestoreArtifactBody>, JsonRejection>,
) -> Result<Json<Value>, HttpJsonError> {
    admit(&state, &claims)?;
    let Json(body) = body.map_err(|_| invalid("the body must be { version, expected_version }"))?;
    // Version 0 reads the head in session-core; a restore always names a stored version.
    if body.version == 0 {
        return Err(invalid("version must be a stored version"));
    }
    ensure_no_running_run(&state, &bearer, &thread_id).await?;
    let head = read_head(&state, &claims, &bearer, &thread_id, &artifact_id).await?;
    check_editable(&head, body.expected_version)?;
    let source = match get_revision(&state, &claims, &bearer, &thread_id, &artifact_id, body.version)
        .await
    {
        Ok(source) => source,
        Err(status) if status.code() == tonic::Code::NotFound => {
            return Err(refusal(
                StatusCode::CONFLICT,
                REVISION_MISSING,
                "The version to restore does not exist.",
                json!({ "version": body.version }),
            ));
        }
        Err(status) => return Err(store_error(&status)),
    };
    let planned = PlannedEdit {
        // The kind is the artifact's, fixed at creation: a restore never changes it.
        kind: head.kind,
        title: source.title,
        content: source.content,
        editor_kind: "restore",
        restored_from_version: body.version,
        expected_version: body.expected_version,
    };
    append_and_publish(&state, &claims, &bearer, &thread_id, &artifact_id, planned).await
}

/// The checks every write passes before anything is read: the switch, then the posture.
fn admit(state: &AppState, claims: &Claims) -> Result<(), HttpJsonError> {
    if !state.artifact_editing_enabled {
        return Err(refusal(
            StatusCode::NOT_FOUND,
            "artifact_editing_unavailable",
            "Editing artifacts is not available.",
            json!({}),
        ));
    }
    require_non_zdr_durable_mutation(claims)
}

/// Refuse while a run on the thread is running. Its commit would otherwise race the save; the
/// commit's own conflict check is the backstop for a run that starts after this read. Fails
/// closed: a run state nobody could read refuses the edit.
async fn ensure_no_running_run(
    state: &AppState,
    bearer: &VerifiedSessionBearer,
    thread_id: &str,
) -> Result<(), HttpJsonError> {
    let request = authenticated_session_request(
        ListRunsRequest {
            thread_id: thread_id.to_owned(),
            status_filter: RUNNING.to_owned(),
            limit: 1,
            ..Default::default()
        },
        bearer,
    )?;
    let runs = state
        .run_client
        .clone()
        .list_runs(request)
        .await
        .map_err(|status| {
            tracing::debug!(code = ?status.code(), "artifact edit: run state unreadable");
            refusal(
                StatusCode::SERVICE_UNAVAILABLE,
                "artifact_edit_run_state_unavailable",
                "Whether a run is working on this conversation could not be read. Try again.",
                json!({ "retryable": true }),
            )
        })?
        .into_inner()
        .runs;
    if runs.is_empty() {
        return Ok(());
    }
    Err(refusal(
        StatusCode::CONFLICT,
        "artifact_edit_run_active",
        "Verevon is still working in this conversation. Save when the answer is done.",
        json!({}),
    ))
}

/// The artifact's head revision. A thread or artifact the caller may not read answers like one
/// that does not exist, so the route is no existence oracle.
async fn read_head(
    state: &AppState,
    claims: &Claims,
    bearer: &VerifiedSessionBearer,
    thread_id: &str,
    artifact_id: &str,
) -> Result<ArtifactRevision, HttpJsonError> {
    get_revision(state, claims, bearer, thread_id, artifact_id, 0)
        .await
        .map_err(|status| store_error(&status))
}

async fn get_revision(
    state: &AppState,
    claims: &Claims,
    bearer: &VerifiedSessionBearer,
    thread_id: &str,
    artifact_id: &str,
    version: u32,
) -> Result<ArtifactRevision, tonic::Status> {
    let request = authenticated_session_request(
        GetArtifactRevisionRequest {
            org_id: claims.org_id.clone(),
            thread_id: thread_id.to_owned(),
            artifact_id: artifact_id.to_owned(),
            version,
        },
        bearer,
    )
    .map_err(|_| tonic::Status::internal("the session credential is not forwardable"))?;
    state
        .artifact_revision_client
        .clone()
        .get_artifact_revision(request)
        .await?
        .into_inner()
        .revision
        .ok_or_else(|| tonic::Status::not_found(REVISION_MISSING))
}

/// A text kind, and the head the editor opened. The head check here answers the common stale
/// tab without a write; session-core's compare-and-set still decides the race.
fn check_editable(head: &ArtifactRevision, expected_version: u32) -> Result<(), HttpJsonError> {
    if !EDITABLE_KINDS.contains(&head.kind.as_str()) {
        return Err(refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "artifact_edit_kind_unsupported",
            "This kind of artifact cannot be edited as text.",
            json!({ "kind": head.kind }),
        ));
    }
    if head.version != expected_version {
        return Err(mismatch(head.version));
    }
    Ok(())
}

/// Append under this replica's commit lock, then publish the new head into the commit store, so
/// a local run's commit cannot land between the durable append and the store update.
async fn append_and_publish(
    state: &AppState,
    claims: &Claims,
    bearer: &VerifiedSessionBearer,
    thread_id: &str,
    artifact_id: &str,
    planned: PlannedEdit,
) -> Result<Json<Value>, HttpJsonError> {
    let editor_kind = planned.editor_kind;
    let request = authenticated_session_request(
        AppendArtifactRevisionRequest {
            org_id: claims.org_id.clone(),
            thread_id: thread_id.to_owned(),
            artifact_id: artifact_id.to_owned(),
            expected_version: planned.expected_version,
            kind: planned.kind,
            title: planned.title,
            content: planned.content,
            editor_kind: editor_kind.to_owned(),
            run_id: String::new(),
            source_message_id: String::new(),
            restored_from_version: planned.restored_from_version,
            idempotency_key: String::new(),
            // session-core assigns head + 1.
            version: 0,
        },
        bearer,
    )?;
    let _commit = state.artifact_versions.lock_commit(thread_id).await;
    let response = match state
        .artifact_revision_client
        .clone()
        .append_artifact_revision(request)
        .await
    {
        Ok(response) => response.into_inner(),
        Err(status) => {
            count(editor_kind, "refused");
            if status.code() == tonic::Code::Aborted && status.message() == REVISION_MISMATCH {
                return Err(moved_head(state, claims, bearer, thread_id, artifact_id).await);
            }
            return Err(store_error(&status));
        }
    };
    let revision = response.revision.ok_or_else(|| {
        refusal(
            StatusCode::BAD_GATEWAY,
            "artifact_edit_failed",
            "The revision store gave no revision back.",
            json!({}),
        )
    })?;
    state.artifact_versions.record_user_revision(
        thread_id,
        artifact_id,
        &revision.title,
        &revision.content,
        revision.version,
    );
    let duplicate = matches!(
        AppendOutcome::try_from(response.outcome),
        Ok(AppendOutcome::Duplicate)
    );
    count(editor_kind, if duplicate { "duplicate" } else { "appended" });
    // Ids only.
    tracing::info!(
        thread_id = %thread_id,
        artifact_id = %artifact_id,
        version = revision.version,
        editor = editor_kind,
        "artifact revision saved by the person"
    );
    Ok(Json(json!({ "data": { "revision": revision_value(&revision) } })))
}

/// The 409 for a head another tab, replica or run moved, naming the head now stored so the
/// editor can offer "load newest" and "overwrite".
async fn moved_head(
    state: &AppState,
    claims: &Claims,
    bearer: &VerifiedSessionBearer,
    thread_id: &str,
    artifact_id: &str,
) -> HttpJsonError {
    match get_revision(state, claims, bearer, thread_id, artifact_id, 0).await {
        Ok(head) => mismatch(head.version),
        Err(status) => store_error(&status),
    }
}

fn mismatch(head_version: u32) -> HttpJsonError {
    refusal(
        StatusCode::CONFLICT,
        REVISION_MISMATCH,
        "The artifact changed after you opened it.",
        json!({ "head_version": head_version }),
    )
}

/// session-core's answers in this route's vocabulary. A typed ZDR refusal (a waived credential)
/// keeps its catalogue body; another person's or org's thread reads as not found.
fn store_error(status: &tonic::Status) -> HttpJsonError {
    if let Some(zdr) = mp_contracts::zdr_refusal::Refusal::from_status(status) {
        return (StatusCode::PRECONDITION_FAILED, Json(zdr.json_body()));
    }
    match (status.code(), status.message()) {
        (tonic::Code::FailedPrecondition, SPACE_THREAD) => refusal(
            StatusCode::FORBIDDEN,
            SPACE_THREAD,
            "Artifacts in a Space conversation cannot be edited here.",
            json!({}),
        ),
        (tonic::Code::FailedPrecondition, STORE_WAIVED_THREAD) => refusal(
            StatusCode::FORBIDDEN,
            WAIVED_THREAD,
            "This conversation ran without data retention, so its artifacts cannot be edited.",
            json!({}),
        ),
        (tonic::Code::FailedPrecondition, REVISION_MISSING) => refusal(
            StatusCode::CONFLICT,
            REVISION_MISSING,
            "The version to restore does not exist.",
            json!({}),
        ),
        (tonic::Code::ResourceExhausted, REVISION_LIMIT) => refusal(
            StatusCode::CONFLICT,
            REVISION_LIMIT,
            "This artifact has reached its revision limit. Save it to knowledge to keep working on it.",
            json!({}),
        ),
        (tonic::Code::NotFound | tonic::Code::PermissionDenied, _) => refusal(
            StatusCode::NOT_FOUND,
            "artifact_not_found",
            "The artifact was not found.",
            json!({}),
        ),
        (tonic::Code::InvalidArgument, message) => invalid(message),
        (tonic::Code::Unavailable, _) => refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "artifact_revisions_unavailable",
            "The revision store is not available. Try again.",
            json!({ "retryable": true }),
        ),
        (code, _) => {
            tracing::warn!(?code, "artifact edit: revision store refused");
            refusal(
                StatusCode::BAD_GATEWAY,
                "artifact_edit_failed",
                "The revision could not be saved.",
                json!({}),
            )
        }
    }
}

fn invalid(message: &str) -> HttpJsonError {
    refusal(
        StatusCode::BAD_REQUEST,
        "invalid_artifact_edit",
        message,
        json!({}),
    )
}

/// The typed envelope; `details` is always an object.
fn refusal(status: StatusCode, code: &str, message: &str, details: Value) -> HttpJsonError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": details } })),
    )
}

fn count(editor_kind: &'static str, outcome: &'static str) {
    metrics::counter!(
        "mp_gateway_artifact_edits_total",
        "editor" => editor_kind,
        "outcome" => outcome
    )
    .increment(1);
}

/// The stored revision as the panel reads it.
fn revision_value(revision: &ArtifactRevision) -> Value {
    json!({
        "id": revision.id,
        "artifact_id": revision.artifact_id,
        "version": revision.version,
        "kind": revision.kind,
        "title": revision.title,
        "content": revision.content,
        "editor_kind": revision.editor_kind,
        "restored_from_version": (revision.restored_from_version > 0)
            .then_some(revision.restored_from_version),
        "content_sha256": revision.content_sha256,
        "created_at": timestamp_to_rfc3339(revision.created_at.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::model_plane::v1::{
        artifact_revision_service_client::ArtifactRevisionServiceClient,
        artifact_revision_service_server::{
            ArtifactRevisionService, ArtifactRevisionServiceServer,
        },
        run_service_client::RunServiceClient,
        run_service_server::{RunService, RunServiceServer},
        AppendArtifactRevisionResponse, CancelRunRequest, CancelRunResponse, GetArtifactRevisionResponse,
        GetRunRequest, ListArtifactRevisionsRequest, ListArtifactRevisionsResponse, ListRunsResponse,
        ListSystemRunsRequest, ResolveRunActionAuthorityRequest, ResolveRunActionAuthorityResponse,
        ResolveRunOwnerRequest, ResolveRunOwnerResponse, RunDetail,
    };
    use std::sync::{Arc, Mutex};
    use tonic::{Request as TonicRequest, Response as TonicResponse, Status};

    /// A revision store double with session-core's append rules: the head is the highest stored
    /// version, a stale `expected_version` is `aborted` `revision_mismatch`, a restore must name a
    /// stored version, and `refuse` stands in for an owner-side refusal (Space, waived).
    #[derive(Clone, Default)]
    struct FakeRevisions {
        rows: Arc<Mutex<Vec<ArtifactRevision>>>,
        appends: Arc<Mutex<Vec<AppendArtifactRevisionRequest>>>,
        refuse: Option<(tonic::Code, &'static str)>,
    }

    impl FakeRevisions {
        fn with(rows: Vec<ArtifactRevision>) -> Self {
            Self {
                rows: Arc::new(Mutex::new(rows)),
                ..Self::default()
            }
        }

        fn head(&self, artifact_id: &str) -> Option<ArtifactRevision> {
            self.rows
                .lock()
                .unwrap()
                .iter()
                .filter(|row| row.artifact_id == artifact_id)
                .max_by_key(|row| row.version)
                .cloned()
        }
    }

    #[tonic::async_trait]
    impl ArtifactRevisionService for FakeRevisions {
        async fn append_artifact_revision(
            &self,
            request: TonicRequest<AppendArtifactRevisionRequest>,
        ) -> Result<TonicResponse<AppendArtifactRevisionResponse>, Status> {
            let request = request.into_inner();
            self.appends.lock().unwrap().push(request.clone());
            if let Some((code, message)) = self.refuse {
                return Err(Status::new(code, message));
            }
            let head = self.head(&request.artifact_id).map_or(0, |row| row.version);
            if head != request.expected_version {
                return Err(Status::aborted(REVISION_MISMATCH));
            }
            let mut rows = self.rows.lock().unwrap();
            if request.restored_from_version > 0
                && !rows.iter().any(|row| {
                    row.artifact_id == request.artifact_id
                        && row.version == request.restored_from_version
                })
            {
                return Err(Status::failed_precondition(REVISION_MISSING));
            }
            let row = ArtifactRevision {
                id: format!("rev-{}", head + 1),
                thread_id: request.thread_id,
                artifact_id: request.artifact_id,
                version: head + 1,
                kind: request.kind,
                title: request.title,
                content: request.content,
                editor_kind: request.editor_kind,
                editor_subject_id: "user-a".to_owned(),
                restored_from_version: request.restored_from_version,
                ..ArtifactRevision::default()
            };
            rows.push(row.clone());
            Ok(TonicResponse::new(AppendArtifactRevisionResponse {
                revision: Some(row),
                outcome: AppendOutcome::Appended as i32,
            }))
        }

        async fn list_artifact_revisions(
            &self,
            _request: TonicRequest<ListArtifactRevisionsRequest>,
        ) -> Result<TonicResponse<ListArtifactRevisionsResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn get_artifact_revision(
            &self,
            request: TonicRequest<GetArtifactRevisionRequest>,
        ) -> Result<TonicResponse<GetArtifactRevisionResponse>, Status> {
            let request = request.into_inner();
            let rows = self.rows.lock().unwrap();
            let found = rows
                .iter()
                .filter(|row| {
                    row.artifact_id == request.artifact_id
                        && (request.version == 0 || row.version == request.version)
                })
                .max_by_key(|row| row.version)
                .cloned();
            found
                .map(|revision| {
                    TonicResponse::new(GetArtifactRevisionResponse {
                        revision: Some(revision),
                    })
                })
                .ok_or_else(|| Status::not_found(REVISION_MISSING))
        }
    }

    /// A run service double that answers only `ListRuns`: one running run, or none.
    #[derive(Clone, Default)]
    struct FakeRuns {
        running: bool,
    }

    #[tonic::async_trait]
    impl RunService for FakeRuns {
        async fn list_runs(
            &self,
            request: TonicRequest<ListRunsRequest>,
        ) -> Result<TonicResponse<ListRunsResponse>, Status> {
            let request = request.into_inner();
            assert_eq!(request.status_filter, RUNNING);
            let runs = if self.running {
                vec![RunDetail {
                    run_id: "run-live".to_owned(),
                    thread_id: request.thread_id,
                    status: RUNNING.to_owned(),
                    ..RunDetail::default()
                }]
            } else {
                Vec::new()
            };
            Ok(TonicResponse::new(ListRunsResponse {
                runs,
                has_more: false,
            }))
        }

        async fn get_run(
            &self,
            _: TonicRequest<GetRunRequest>,
        ) -> Result<TonicResponse<RunDetail>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn list_system_runs(
            &self,
            _: TonicRequest<ListSystemRunsRequest>,
        ) -> Result<TonicResponse<ListRunsResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn cancel_run(
            &self,
            _: TonicRequest<CancelRunRequest>,
        ) -> Result<TonicResponse<CancelRunResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn resolve_run_owner(
            &self,
            _: TonicRequest<ResolveRunOwnerRequest>,
        ) -> Result<TonicResponse<ResolveRunOwnerResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn resolve_thread_owner(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::ResolveThreadOwnerRequest>,
        ) -> Result<TonicResponse<mp_contracts::model_plane::v1::ResolveThreadOwnerResponse>, Status>
        {
            Err(Status::unimplemented("not used"))
        }

        async fn resolve_run_action_authority(
            &self,
            _: TonicRequest<ResolveRunActionAuthorityRequest>,
        ) -> Result<TonicResponse<ResolveRunActionAuthorityResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn resolve_approval_authority(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::ResolveApprovalAuthorityRequest>,
        ) -> Result<
            TonicResponse<mp_contracts::model_plane::v1::ResolveApprovalAuthorityResponse>,
            Status,
        > {
            Err(Status::unimplemented("not used"))
        }

        async fn get_scheduled_step_context(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::GetScheduledStepContextRequest>,
        ) -> Result<TonicResponse<mp_contracts::model_plane::v1::ScheduledStepContext>, Status>
        {
            Err(Status::unimplemented("not used"))
        }

        async fn resolve_scheduled_step_authority(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::ResolveScheduledStepAuthorityRequest>,
        ) -> Result<
            TonicResponse<mp_contracts::model_plane::v1::ResolveScheduledStepAuthorityResponse>,
            Status,
        > {
            Err(Status::unimplemented("not used"))
        }

        async fn authorize_room_run_attach(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::AuthorizeRoomRunAttachRequest>,
        ) -> Result<TonicResponse<mp_contracts::model_plane::v1::AuthorizeRoomRunAttachResponse>, Status>
        {
            Err(Status::unimplemented("not used"))
        }

        async fn cancel_run_as_room_manager(
            &self,
            _: TonicRequest<mp_contracts::model_plane::v1::CancelRunAsRoomManagerRequest>,
        ) -> Result<
            TonicResponse<mp_contracts::model_plane::v1::CancelRunAsRoomManagerResponse>,
            Status,
        > {
            Err(Status::unimplemented("not used"))
        }
    }

    async fn state_with(revisions: FakeRevisions, runs: FakeRuns) -> AppState {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(ArtifactRevisionServiceServer::new(revisions))
                .add_service(RunServiceServer::new(runs))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await;
        });
        let url = format!("http://{address}");
        let mut state = AppState::new();
        state.artifact_revision_client = ArtifactRevisionServiceClient::connect(url.clone())
            .await
            .unwrap();
        state.run_client = RunServiceClient::connect(url).await.unwrap();
        state.artifact_revisions_mode = ArtifactRevisionsMode::On;
        state.artifact_editing_enabled = true;
        state
    }

    fn claims(zdr: bool) -> Claims {
        Claims {
            key_id: None,
            acting_user_id: None,
            sub: "user-a".to_owned(),
            iss: "auth-core".to_owned(),
            exp: i64::MAX,
            org_id: "org-a".to_owned(),
            user_id: "user-a".to_owned(),
            nbf: None,
            aud: Some("model-gateway".to_owned()),
            scopes: Vec::new(),
            zdr,
            sovereign: None,
            org_zdr: None,
            zdr_waiver: None,
            principal_type: Some("user".to_owned()),
            service_id: None,
            reason: None,
        }
    }

    fn row(version: u32, kind: &str, editor_kind: &str, content: &str) -> ArtifactRevision {
        ArtifactRevision {
            id: format!("rev-{version}"),
            thread_id: "thread-a".to_owned(),
            artifact_id: "report".to_owned(),
            version,
            kind: kind.to_owned(),
            title: format!("Report v{version}"),
            content: content.to_owned(),
            editor_kind: editor_kind.to_owned(),
            ..ArtifactRevision::default()
        }
    }

    fn model_history() -> FakeRevisions {
        FakeRevisions::with(vec![
            row(1, "document", "model", "first draft"),
            row(2, "document", "model", "second draft"),
        ])
    }

    fn path() -> Path<(String, String)> {
        Path(("thread-a".to_owned(), "report".to_owned()))
    }

    async fn save_edit(
        state: &AppState,
        zdr: bool,
        body: Value,
    ) -> Result<Json<Value>, HttpJsonError> {
        save_artifact_edit(
            State(state.clone()),
            Extension(claims(zdr)),
            VerifiedSessionBearer::for_test("session-token"),
            path(),
            Ok(Json(serde_json::from_value(body).unwrap())),
        )
        .await
    }

    async fn restore(
        state: &AppState,
        version: u32,
        expected_version: u32,
    ) -> Result<Json<Value>, HttpJsonError> {
        restore_artifact_revision(
            State(state.clone()),
            Extension(claims(false)),
            VerifiedSessionBearer::for_test("session-token"),
            path(),
            Ok(Json(RestoreArtifactBody {
                version,
                expected_version,
            })),
        )
        .await
    }

    fn refused(result: Result<Json<Value>, HttpJsonError>) -> (StatusCode, Value) {
        let (status, Json(body)) = result.expect_err("the write must be refused");
        (status, body)
    }

    #[test]
    fn editing_flag_requires_revisions_on() {
        assert_eq!(parse_editing(None, ArtifactRevisionsMode::Off), Ok(false));
        assert_eq!(parse_editing(Some("true"), ArtifactRevisionsMode::On), Ok(true));
        for mode in [ArtifactRevisionsMode::Off, ArtifactRevisionsMode::Shadow] {
            assert!(parse_editing(Some("true"), mode).is_err(), "{mode:?}");
        }
        // Only the literal enables; anything else is off, never an error.
        for raw in ["", "TRUE", "1", "yes", " true", "on"] {
            assert_eq!(parse_editing(Some(raw), ArtifactRevisionsMode::On), Ok(false), "{raw:?}");
        }
    }

    #[tokio::test]
    async fn the_default_state_cannot_edit() {
        assert!(!AppState::new().artifact_editing_enabled);
    }

    #[tokio::test]
    async fn routes_404_while_editing_is_off() {
        let fake = model_history();
        let mut state = state_with(fake.clone(), FakeRuns::default()).await;
        state.artifact_editing_enabled = false;
        let (status, body) = refused(
            save_edit(&state, false, json!({ "content": "x", "expected_version": 2 })).await,
        );
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "artifact_editing_unavailable");
        let (status, _) = refused(restore(&state, 1, 2).await);
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(fake.appends.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn put_appends_user_revision() {
        let fake = model_history();
        let state = state_with(fake.clone(), FakeRuns::default()).await;
        let Json(body) = save_edit(
            &state,
            false,
            json!({ "content": "the person's text", "title": " Edited ", "expected_version": 2 }),
        )
        .await
        .expect("saved");
        let revision = &body["data"]["revision"];
        assert_eq!(revision["version"], 3);
        assert_eq!(revision["editor_kind"], "user");
        assert_eq!(revision["title"], "Edited");
        assert_eq!(revision["kind"], "document", "the kind is the head's");
        assert!(revision["restored_from_version"].is_null());

        let appends = fake.appends.lock().unwrap().clone();
        assert_eq!(appends.len(), 1);
        assert_eq!(appends[0].editor_kind, "user");
        assert_eq!(appends[0].expected_version, 2);
        assert_eq!(appends[0].version, 0, "session-core assigns the number");
        assert_eq!(appends[0].org_id, "org-a", "the org is the verified caller's");
        assert!(appends[0].run_id.is_empty());
    }

    #[tokio::test]
    async fn put_without_a_title_keeps_the_head_title() {
        let fake = model_history();
        let state = state_with(fake.clone(), FakeRuns::default()).await;
        save_edit(&state, false, json!({ "content": "text", "expected_version": 2 }))
            .await
            .expect("saved");
        assert_eq!(fake.appends.lock().unwrap()[0].title, "Report v2");
    }

    #[tokio::test]
    async fn put_with_stale_expected_version_is_409_with_head() {
        let fake = model_history();
        let state = state_with(fake.clone(), FakeRuns::default()).await;
        let (status, body) =
            refused(save_edit(&state, false, json!({ "content": "x", "expected_version": 1 })).await);
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], REVISION_MISMATCH);
        assert_eq!(body["error"]["details"]["head_version"], 2);
        assert!(fake.appends.lock().unwrap().is_empty(), "a stale tab writes nothing");
    }

    #[tokio::test]
    async fn a_head_moved_during_the_save_is_409_with_the_new_head() {
        let fake = model_history();
        let state = state_with(fake.clone(), FakeRuns::default()).await;
        // Another replica appended between the head read and the append: the store's CAS decides.
        let moved = fake.clone();
        let head = read_head(
            &state,
            &claims(false),
            &VerifiedSessionBearer::for_test("session-token"),
            "thread-a",
            "report",
        )
        .await
        .unwrap();
        moved.rows.lock().unwrap().push(row(3, "document", "model", "third"));
        let planned = PlannedEdit {
            kind: head.kind,
            title: head.title,
            content: "mine".to_owned(),
            editor_kind: "user",
            restored_from_version: 0,
            expected_version: head.version,
        };
        let (status, body) = refused(
            append_and_publish(
                &state,
                &claims(false),
                &VerifiedSessionBearer::for_test("session-token"),
                "thread-a",
                "report",
                planned,
            )
            .await,
        );
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["details"]["head_version"], 3);
        assert_eq!(state.artifact_versions.current_version("thread-a", "report"), None);
    }

    #[tokio::test]
    async fn restore_copies_old_content_as_new_version() {
        let fake = model_history();
        let state = state_with(fake.clone(), FakeRuns::default()).await;
        let Json(body) = restore(&state, 1, 2).await.expect("restored");
        let revision = &body["data"]["revision"];
        assert_eq!(revision["version"], 3);
        assert_eq!(revision["editor_kind"], "restore");
        assert_eq!(revision["restored_from_version"], 1);
        assert_eq!(revision["content"], "first draft");
        assert_eq!(revision["title"], "Report v1");
        assert_eq!(
            state.artifact_versions.content_of("thread-a", "report").as_deref(),
            Some("first draft")
        );
        assert!(state.artifact_versions.edited_by_user("thread-a", "report"));
    }

    #[tokio::test]
    async fn restore_of_missing_version_is_409() {
        let fake = model_history();
        let state = state_with(fake.clone(), FakeRuns::default()).await;
        let (status, body) = refused(restore(&state, 7, 2).await);
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], REVISION_MISSING);
        assert!(fake.appends.lock().unwrap().is_empty());
        let (status, _) = refused(restore(&state, 0, 2).await);
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn edit_refused_for_zdr_credential() {
        let fake = model_history();
        let state = state_with(fake.clone(), FakeRuns::default()).await;
        let (status, body) =
            refused(save_edit(&state, true, json!({ "content": "x", "expected_version": 2 })).await);
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            body["error"]["code"],
            mp_contracts::zdr_refusal::code::DURABLE_MUTATION_FORBIDDEN
        );
        assert!(fake.appends.lock().unwrap().is_empty(), "nothing is sent under ZDR");
    }

    #[tokio::test]
    async fn edit_refused_for_waived_thread() {
        let fake = FakeRevisions {
            refuse: Some((tonic::Code::FailedPrecondition, STORE_WAIVED_THREAD)),
            ..model_history()
        };
        let state = state_with(fake, FakeRuns::default()).await;
        let (status, body) =
            refused(save_edit(&state, false, json!({ "content": "x", "expected_version": 2 })).await);
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], WAIVED_THREAD);
        assert!(body["error"]["details"].is_object());
        assert_eq!(state.artifact_versions.current_version("thread-a", "report"), None);
    }

    #[tokio::test]
    async fn edit_refused_for_space_thread() {
        let fake = FakeRevisions {
            refuse: Some((tonic::Code::FailedPrecondition, SPACE_THREAD)),
            ..model_history()
        };
        let state = state_with(fake, FakeRuns::default()).await;
        let (status, body) = refused(restore(&state, 1, 2).await);
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], SPACE_THREAD);
    }

    #[tokio::test]
    async fn edit_refused_while_run_streams() {
        let fake = model_history();
        let state = state_with(fake.clone(), FakeRuns { running: true }).await;
        let (status, body) =
            refused(save_edit(&state, false, json!({ "content": "x", "expected_version": 2 })).await);
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "artifact_edit_run_active");
        assert!(fake.appends.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_deck_is_never_edited_as_text() {
        let fake = FakeRevisions::with(vec![row(1, "deck", "model", "{\"slides\":[]}")]);
        let state = state_with(fake.clone(), FakeRuns::default()).await;
        let (status, body) =
            refused(save_edit(&state, false, json!({ "content": "x", "expected_version": 1 })).await);
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "artifact_edit_kind_unsupported");
        assert_eq!(body["error"]["details"]["kind"], "deck");
        assert!(fake.appends.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unknown_artifact_is_404() {
        let state = state_with(FakeRevisions::default(), FakeRuns::default()).await;
        let (status, body) =
            refused(save_edit(&state, false, json!({ "content": "x", "expected_version": 0 })).await);
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "artifact_not_found");
    }

    #[tokio::test]
    async fn empty_or_oversized_content_is_refused() {
        let fake = model_history();
        let state = state_with(fake.clone(), FakeRuns::default()).await;
        for content in [String::new(), "x".repeat(MAX_TEXT_ARTIFACT_CHARS + 1)] {
            let (status, _) = refused(
                save_edit(&state, false, json!({ "content": content, "expected_version": 2 })).await,
            );
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
        assert!(fake.appends.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn next_turn_reads_user_version() {
        let fake = model_history();
        let state = state_with(fake, FakeRuns::default()).await;
        state
            .artifact_versions
            .seed_persisted("thread-a", "report", "Report v2", "second draft", 2);
        save_edit(&state, false, json!({ "content": "the person's text", "expected_version": 2 }))
            .await
            .expect("saved");
        // The next turn forks the commit store: read_artifact and the inventory see the edit.
        let turn = state.artifact_versions.fork_thread("thread-a");
        assert_eq!(turn.content_of("thread-a", "report").as_deref(), Some("the person's text"));
        assert_eq!(turn.current_version("thread-a", "report"), Some(3));
        assert!(turn
            .inventory_context("thread-a")
            .unwrap()
            .contains("\"edited_by\":\"user\""));
    }
}
