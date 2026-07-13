use anyhow::{Context, Result};
use mp_contracts::model_plane::v1::{AppendMessageRequest, CreateThreadRequest, StartRunRequest};
use mp_ids::new_ulid;
use tonic::Code;

use crate::auth::VerifiedSessionBearer;
use crate::state::AppState;

fn authenticated_request<T>(value: T, bearer: Option<&str>) -> Result<tonic::Request<T>> {
    let bearer = bearer.context("verified session credential required")?;
    let mut request = tonic::Request::new(value);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {bearer}")
            .parse()
            .context("verified session credential is not forwardable")?,
    );
    Ok(request)
}

pub struct SessionRun {
    pub thread_id: String,
    pub run_id: String,
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

async fn create_thread(
    client: &mut mp_contracts::model_plane::v1::session_core_client::SessionCoreClient<
        tonic::transport::Channel,
    >,
    requested_session_key: Option<&str>,
    org_id: &str,
    user_id: &str,
    bearer: Option<&str>,
) -> Result<String> {
    let response = client
        .create_thread(authenticated_request(
            CreateThreadRequest {
                session_key: non_empty(requested_session_key)
                    .unwrap_or("")
                    .to_owned()
                    .if_empty_then(new_ulid()),
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
                metadata: None,
            },
            bearer,
        )?)
        .await
        .context("session-core create_thread failed")?;

    Ok(response.into_inner().thread_id)
}

fn missing_thread_append_error(error: &tonic::Status) -> bool {
    if error.code() == Code::NotFound {
        return true;
    }

    error.code() == Code::Internal && error.message().to_ascii_lowercase().contains("foreign key")
}

async fn append_user_message(
    client: &mut mp_contracts::model_plane::v1::session_core_client::SessionCoreClient<
        tonic::transport::Channel,
    >,
    thread_id: &str,
    goal: &str,
    bearer: Option<&str>,
) -> Result<(), tonic::Status> {
    let request = authenticated_request(
        AppendMessageRequest {
            thread_id: thread_id.to_owned(),
            role: "user".to_owned(),
            content: goal.to_owned(),
            metadata: None,
        },
        bearer,
    )
    .map_err(|_| tonic::Status::unauthenticated("verified session credential required"))?;
    client.append_message(request).await.map(|_| ())
}

/// Ensure the request has a thread, persist the user prompt, and create a run.
///
/// # Errors
///
/// Returns an error when any session-core RPC in the setup sequence fails.
pub async fn prepare_run(
    state: &AppState,
    requested_thread_id: Option<&str>,
    requested_session_key: Option<&str>,
    org_id: &str,
    user_id: &str,
    goal: &str,
) -> Result<SessionRun> {
    prepare_run_with_bearer(
        state,
        requested_thread_id,
        requested_session_key,
        org_id,
        user_id,
        goal,
        None,
    )
    .await
}

/// Authenticated HTTP/SSE path. The independently verified, dedicated
/// `aud=session-core` bearer is forwarded to session-core; it is never accepted
/// from a caller field.
///
/// # Errors
/// Returns an error when Session Core rejects or cannot complete any run setup
/// operation.
pub async fn prepare_run_authenticated(
    state: &AppState,
    requested_thread_id: Option<&str>,
    requested_session_key: Option<&str>,
    org_id: &str,
    user_id: &str,
    goal: &str,
    bearer: &VerifiedSessionBearer,
) -> Result<SessionRun> {
    prepare_run_with_bearer(
        state,
        requested_thread_id,
        requested_session_key,
        org_id,
        user_id,
        goal,
        Some(bearer.as_str()),
    )
    .await
}

/// Crate-internal adapter for an ingress boundary that has already verified a
/// dedicated `aud=session-core` bearer and bound it to the Model Plane caller.
///
/// Raw caller input must never be passed to this function without that boundary
/// verification.
pub(crate) async fn prepare_run_with_token(
    state: &AppState,
    requested_thread_id: Option<&str>,
    requested_session_key: Option<&str>,
    org_id: &str,
    user_id: &str,
    goal: &str,
    bearer: &str,
) -> Result<SessionRun> {
    prepare_run_with_bearer(
        state,
        requested_thread_id,
        requested_session_key,
        org_id,
        user_id,
        goal,
        Some(bearer),
    )
    .await
}

async fn prepare_run_with_bearer(
    state: &AppState,
    requested_thread_id: Option<&str>,
    requested_session_key: Option<&str>,
    org_id: &str,
    user_id: &str,
    goal: &str,
    bearer: Option<&str>,
) -> Result<SessionRun> {
    let mut client = state.session_client.clone();

    let mut thread_id = if let Some(thread_id) = non_empty(requested_thread_id) {
        thread_id.to_owned()
    } else {
        create_thread(&mut client, requested_session_key, org_id, user_id, bearer).await?
    };

    if let Err(error) = append_user_message(&mut client, &thread_id, goal, bearer).await {
        if non_empty(requested_thread_id).is_some() && missing_thread_append_error(&error) {
            tracing::info!(
                requested_thread_id = %thread_id,
                "requested thread was not present in session-core; creating durable thread"
            );
            thread_id = create_thread(
                &mut client,
                requested_session_key.or(requested_thread_id),
                org_id,
                user_id,
                bearer,
            )
            .await?;
            append_user_message(&mut client, &thread_id, goal, bearer)
                .await
                .context("session-core append_message(user) failed")?;
        } else {
            return Err(error).context("session-core append_message(user) failed");
        }
    }

    let response = client
        .start_run(authenticated_request(
            StartRunRequest {
                thread_id: thread_id.clone(),
                parent_run_id: String::new(),
                agent_id: "model-gateway".to_owned(),
                goal: goal.to_owned(),
                mode: "execute".to_owned(),
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
            },
            bearer,
        )?)
        .await
        .context("session-core start_run failed")?;

    Ok(SessionRun {
        thread_id,
        run_id: response.into_inner().run_id,
    })
}

/// Persist the assistant response into the thread when content is available.
///
/// # Errors
///
/// Returns an error when session-core rejects the assistant message append.
pub async fn append_assistant_message(
    state: &AppState,
    thread_id: &str,
    content: &str,
) -> Result<()> {
    append_assistant_message_with_bearer(state, thread_id, content, None).await
}

/// Persist an assistant message with an independently verified Session Core
/// audience credential.
///
/// # Errors
/// Returns an error when the credential cannot be forwarded or Session Core
/// rejects or cannot complete the append.
pub async fn append_assistant_message_authenticated(
    state: &AppState,
    thread_id: &str,
    content: &str,
    bearer: &VerifiedSessionBearer,
) -> Result<()> {
    append_assistant_message_with_bearer(state, thread_id, content, Some(bearer.as_str())).await
}

/// Crate-internal adapter for an ingress boundary that has already verified a
/// dedicated `aud=session-core` bearer and bound it to the Model Plane caller.
pub(crate) async fn append_assistant_message_with_token(
    state: &AppState,
    thread_id: &str,
    content: &str,
    bearer: &str,
) -> Result<()> {
    append_assistant_message_with_bearer(state, thread_id, content, Some(bearer)).await
}

/// Optional-token variant used only by compatibility code that must fail
/// closed when no independently verified session credential is available.
pub(crate) async fn append_assistant_message_with_optional_token(
    state: &AppState,
    thread_id: &str,
    content: &str,
    bearer: Option<&str>,
) -> Result<()> {
    append_assistant_message_with_bearer(state, thread_id, content, bearer).await
}

async fn append_assistant_message_with_bearer(
    state: &AppState,
    thread_id: &str,
    content: &str,
    bearer: Option<&str>,
) -> Result<()> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let mut client = state.session_client.clone();
    client
        .append_message(authenticated_request(
            AppendMessageRequest {
                thread_id: thread_id.to_owned(),
                role: "assistant".to_owned(),
                content: trimmed.to_owned(),
                metadata: None,
            },
            bearer,
        )?)
        .await
        .context("session-core append_message(assistant) failed")?;

    Ok(())
}

trait StringExt {
    fn if_empty_then(self, fallback: String) -> String;
}

impl StringExt for String {
    fn if_empty_then(self, fallback: String) -> String {
        if self.is_empty() {
            fallback
        } else {
            self
        }
    }
}
