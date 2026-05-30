use anyhow::{Context, Result};
use mp_contracts::model_plane::v1::{AppendMessageRequest, CreateThreadRequest, StartRunRequest};
use mp_ids::new_ulid;

use crate::state::AppState;

pub struct SessionRun {
    pub thread_id: String,
    pub run_id: String,
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
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
    let mut client = state.session_client.clone();

    let thread_id = if let Some(thread_id) = non_empty(requested_thread_id) {
        thread_id.to_owned()
    } else {
        let response = client
            .create_thread(CreateThreadRequest {
                session_key: non_empty(requested_session_key)
                    .unwrap_or("")
                    .to_owned()
                    .if_empty_then(new_ulid()),
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
                metadata: None,
            })
            .await
            .context("session-core create_thread failed")?;
        response.into_inner().thread_id
    };

    client
        .append_message(AppendMessageRequest {
            thread_id: thread_id.clone(),
            role: "user".to_owned(),
            content: goal.to_owned(),
            metadata: None,
        })
        .await
        .context("session-core append_message(user) failed")?;

    let response = client
        .start_run(StartRunRequest {
            thread_id: thread_id.clone(),
            parent_run_id: String::new(),
            agent_id: "model-gateway".to_owned(),
            goal: goal.to_owned(),
            mode: "execute".to_owned(),
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
        })
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
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let mut client = state.session_client.clone();
    client
        .append_message(AppendMessageRequest {
            thread_id: thread_id.to_owned(),
            role: "assistant".to_owned(),
            content: trimmed.to_owned(),
            metadata: None,
        })
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
