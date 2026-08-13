use anyhow::{Context, Result};
use mp_contracts::model_plane::v1::{
    AppendMessageRequest, CancelRunRequest, CreateThreadRequest, HeartbeatManagedRunRequest,
    ManagedRunSource, RecordTerminalOutcomeRequest, ResolveRunOwnerRequest, StartManagedRunRequest,
    StartRunRequest, TerminalOutcome,
};
use mp_ids::new_ulid;
use std::time::Duration;
use tonic::Code;

use crate::auth::VerifiedSessionBearer;
use crate::state::AppState;

/// Session Core renews a managed producer deadline for 15 minutes. Refresh at
/// most every five minutes so a long direct stream has two independent retry
/// windows before recovery can classify the outcome as unknown.
pub(crate) const MANAGED_RUN_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// A stuck Session Core connection must not keep a provider/browser dispatch
/// alive indefinitely. The next interval may retry only while the caller is
/// still otherwise active; initial liveness always fails closed.
const MANAGED_RUN_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MANAGED_GOAL_BYTES: usize = 60 * 1024;

/// Session Core stores a run goal as single-line metadata and rejects control
/// characters. Preserve the full prompt in the user message/inference request,
/// while sending a bounded, contract-safe projection to the managed lifecycle.
fn managed_goal(goal: &str) -> String {
    if goal.len() <= MAX_MANAGED_GOAL_BYTES && !goal.chars().any(char::is_control) {
        return goal.to_owned();
    }

    let digest = blake3::hash(goal.as_bytes()).to_hex();
    let digest_suffix = format!(" [full-goal-blake3:{digest}]");
    let content_limit = MAX_MANAGED_GOAL_BYTES - digest_suffix.len();
    let mut normalized = String::with_capacity(goal.len().min(content_limit));
    let mut pending_space = false;

    for character in goal.chars() {
        if character.is_control() || character.is_whitespace() {
            pending_space = !normalized.is_empty();
            continue;
        }

        let required = character.len_utf8() + usize::from(pending_space);
        if normalized.len() + required > content_limit {
            break;
        }
        if pending_space {
            normalized.push(' ');
            pending_space = false;
        }
        normalized.push(character);
    }

    if normalized.is_empty() {
        digest_suffix.trim_start().to_owned()
    } else {
        normalized.push_str(&digest_suffix);
        normalized
    }
}

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

#[derive(Clone, Debug)]
pub struct SessionRun {
    pub thread_id: String,
    pub run_id: String,
    /// Trusted owner from the managed start response context. This is used only
    /// to mint a tenant-bound service receipt token; Session Core still derives
    /// the durable owner from `run_id` when applying the terminal outcome.
    pub org_id: String,
    /// An exact retry of a managed start must not trigger a second provider or
    /// execution dispatch. Callers surface it as an observable retry state.
    pub already_started: bool,
    /// The server-validated producer source selected when the managed run was
    /// created. Gateway only heartbeats sources it is authorized to own.
    pub terminal_source: ManagedRunSource,
}

/// The only gateway-owned terminal outcomes for a direct inference run.
///
/// Agentic runs are deliberately excluded: execution-core owns their governed
/// progression and may be awaiting human approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectInferenceTerminal {
    Completed,
    Failed(&'static str),
}

/// The Gateway-owned terminal projection for a standalone browser-agent run.
///
/// Execution Core records the browser action step as non-terminal because that
/// same primitive is also used by multi-step agents. Gateway owns this
/// standalone browser endpoint and only applies this projection after receiving
/// a confirmed `ExecuteStep` response. Approval pauses intentionally have no
/// variant here and remain non-terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserAgentTerminal {
    Completed,
    Failed(&'static str),
}

/// A BFF-injected Control decision for the one scoped thread-creation slice.
/// It is not an authority by itself: Session Core verifies the signed token and
/// recomputes the exact effect digest before it persists anything.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct ThreadSpaceContext {
    pub space_id: String,
    pub space_decision_ref: String,
    pub recipient_audience_ref: String,
    pub recipient_audience_revision: u64,
    pub recipient_audience_hash: String,
    pub privacy_policy_ref: String,
    pub authority_revision: u64,
    pub resource_authorization_ref: String,
    pub space_decision_token: String,
    #[serde(default)]
    pub retrieval_decision_token: String,
    pub action_schema_hash: String,
    pub payload_digest: String,
    pub idempotency_key: String,
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
    space: Option<&ThreadSpaceContext>,
) -> Result<String> {
    let space = space.cloned().unwrap_or_else(|| ThreadSpaceContext {
        space_id: String::new(),
        space_decision_ref: String::new(),
        recipient_audience_ref: String::new(),
        recipient_audience_revision: 0,
        recipient_audience_hash: String::new(),
        privacy_policy_ref: String::new(),
        authority_revision: 0,
        resource_authorization_ref: String::new(),
        space_decision_token: String::new(),
        retrieval_decision_token: String::new(),
        action_schema_hash: String::new(),
        payload_digest: String::new(),
        idempotency_key: String::new(),
    });
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
                // Chat ingress does not yet resolve a Control Space decision.
                // Keep the entire envelope empty rather than treating its
                // session_key or workspace context as authority.
                space_id: space.space_id,
                space_decision_ref: space.space_decision_ref,
                recipient_audience_ref: space.recipient_audience_ref,
                recipient_audience_revision: space.recipient_audience_revision,
                recipient_audience_hash: space.recipient_audience_hash,
                privacy_policy_ref: space.privacy_policy_ref,
                authority_revision: space.authority_revision,
                resource_authorization_ref: space.resource_authorization_ref,
                space_decision_token: space.space_decision_token,
                action_schema_hash: space.action_schema_hash,
                payload_digest: space.payload_digest,
                idempotency_key: space.idempotency_key,
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
    space: Option<&ThreadSpaceContext>,
) -> Result<(), tonic::Status> {
    let space = space.cloned().unwrap_or_else(|| ThreadSpaceContext {
        space_id: String::new(),
        space_decision_ref: String::new(),
        recipient_audience_ref: String::new(),
        recipient_audience_revision: 0,
        recipient_audience_hash: String::new(),
        privacy_policy_ref: String::new(),
        authority_revision: 0,
        resource_authorization_ref: String::new(),
        space_decision_token: String::new(),
        retrieval_decision_token: String::new(),
        action_schema_hash: String::new(),
        payload_digest: String::new(),
        idempotency_key: String::new(),
    });
    let request = authenticated_request(
        AppendMessageRequest {
            thread_id: thread_id.to_owned(),
            role: "user".to_owned(),
            content: goal.to_owned(),
            metadata: None,
            space_id: space.space_id,
            space_decision_ref: space.space_decision_ref,
            recipient_audience_ref: space.recipient_audience_ref,
            recipient_audience_revision: space.recipient_audience_revision,
            recipient_audience_hash: space.recipient_audience_hash,
            privacy_policy_ref: space.privacy_policy_ref,
            authority_revision: space.authority_revision,
            resource_authorization_ref: space.resource_authorization_ref,
            space_decision_token: space.space_decision_token,
            action_schema_hash: space.action_schema_hash,
            payload_digest: space.payload_digest,
            idempotency_key: space.idempotency_key,
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

/// Start a Gateway-owned managed run. The additive lifecycle contract makes
/// run creation and its terminalization obligation atomic, while preserving
/// the legacy `StartRun` RPC for external compatibility callers.
///
/// For a ZDR-authorized caller this intentionally does not create a thread or
/// append the prompt locally: Session Core owns creation of a fresh
/// metadata-only thread and redacts every content-bearing durable field.
#[allow(clippy::too_many_arguments)]
pub async fn prepare_managed_run_authenticated(
    state: &AppState,
    requested_thread_id: Option<&str>,
    requested_session_key: Option<&str>,
    org_id: &str,
    user_id: &str,
    goal: &str,
    agent_id: &str,
    mode: &str,
    start_key: &str,
    source: ManagedRunSource,
    zdr: bool,
    bearer: &VerifiedSessionBearer,
    space: Option<&ThreadSpaceContext>,
    append_space: Option<&ThreadSpaceContext>,
) -> Result<SessionRun> {
    prepare_managed_run_with_bearer(
        state,
        requested_thread_id,
        requested_session_key,
        org_id,
        user_id,
        goal,
        agent_id,
        mode,
        start_key,
        source,
        zdr,
        Some(bearer.as_str()),
        space,
        append_space,
    )
    .await
}

/// Crate-internal managed-start adapter for an ingress boundary that already
/// verified and bound a dedicated user `aud=session-core` bearer. It is never
/// valid to pass a raw caller credential to this function.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare_managed_run_with_token(
    state: &AppState,
    requested_thread_id: Option<&str>,
    requested_session_key: Option<&str>,
    org_id: &str,
    user_id: &str,
    goal: &str,
    agent_id: &str,
    mode: &str,
    start_key: &str,
    source: ManagedRunSource,
    zdr: bool,
    bearer: &str,
) -> Result<SessionRun> {
    prepare_managed_run_with_bearer(
        state,
        requested_thread_id,
        requested_session_key,
        org_id,
        user_id,
        goal,
        agent_id,
        mode,
        start_key,
        source,
        zdr,
        Some(bearer),
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn prepare_managed_run_with_bearer(
    state: &AppState,
    requested_thread_id: Option<&str>,
    requested_session_key: Option<&str>,
    org_id: &str,
    user_id: &str,
    goal: &str,
    agent_id: &str,
    mode: &str,
    start_key: &str,
    source: ManagedRunSource,
    zdr: bool,
    bearer: Option<&str>,
    space: Option<&ThreadSpaceContext>,
    append_space: Option<&ThreadSpaceContext>,
) -> Result<SessionRun> {
    let thread_id = if zdr {
        // Do not persist a caller-provided thread id or prompt before Session
        // Core selects its metadata-only ZDR thread inside the same transaction
        // as the managed run/outbox row.
        String::new()
    } else {
        let mut session_client = state.session_client.clone();
        let mut thread_id = if let Some(thread_id) = non_empty(requested_thread_id) {
            thread_id.to_owned()
        } else {
            create_thread(
                &mut session_client,
                requested_session_key,
                org_id,
                user_id,
                bearer,
                space,
            )
            .await?
        };
        if let Err(error) =
            append_user_message(&mut session_client, &thread_id, goal, bearer, append_space).await
        {
            if non_empty(requested_thread_id).is_some() && missing_thread_append_error(&error) {
                tracing::info!(
                    requested_thread_id = %thread_id,
                    "requested thread was not present in session-core; creating durable thread"
                );
                thread_id = create_thread(
                    &mut session_client,
                    requested_session_key.or(requested_thread_id),
                    org_id,
                    user_id,
                    bearer,
                    space,
                )
                .await?;
                append_user_message(&mut session_client, &thread_id, goal, bearer, append_space)
                    .await
                    .context("session-core append_message(user) failed")?;
            } else {
                return Err(error).context("session-core append_message(user) failed");
            }
        }
        thread_id
    };

    // Session Core independently derives the caller's ZDR posture, but the
    // Gateway also minimizes what crosses this durable-boundary RPC. The live
    // inference request retains the prompt; the managed lifecycle receives no
    // caller-controlled content for a ZDR run.
    let (managed_agent_id, managed_goal, managed_mode) = if zdr {
        (String::new(), String::new(), String::new())
    } else {
        (agent_id.to_owned(), managed_goal(goal), mode.to_owned())
    };

    let mut lifecycle = state.managed_run_client.clone();
    let response = lifecycle
        .start_managed_run(authenticated_request(
            StartManagedRunRequest {
                thread_id: thread_id.clone(),
                parent_run_id: String::new(),
                agent_id: managed_agent_id,
                goal: managed_goal,
                mode: managed_mode,
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
                start_key: start_key.to_owned(),
                terminal_source: source as i32,
            },
            bearer,
        )?)
        .await
        .context("session-core start_managed_run failed")?
        .into_inner();

    if response.thread_id.trim().is_empty() {
        anyhow::bail!("session-core managed start returned an empty thread id");
    }
    if !zdr && response.thread_id != thread_id {
        anyhow::bail!("session-core managed start returned a different durable thread");
    }

    Ok(SessionRun {
        thread_id: response.thread_id,
        run_id: response.run_id,
        org_id: org_id.to_owned(),
        already_started: response.already_started,
        terminal_source: source,
    })
}

/// Require Session Core's durable tenant/user ownership record before a
/// gateway endpoint mutates or publishes an event for a caller-selected run.
///
/// The caller must have already validated `org_id`, `user_id`, and the
/// dedicated Session Core bearer at its ingress boundary. This function never
/// accepts a raw client credential or a caller-supplied identity field as
/// authority.
///
/// # Errors
/// Returns `invalid_argument` for missing identifiers, `permission_denied`
/// when Session Core reports a different owner, and `unavailable` when the
/// durable ownership authority cannot be reached. The latter deliberately
/// fails closed before any local mutation or event publication.
#[allow(clippy::result_large_err)]
pub(crate) async fn require_durable_run_owner_with_token(
    state: &AppState,
    run_id: &str,
    org_id: &str,
    user_id: &str,
    bearer: &str,
) -> std::result::Result<(), tonic::Status> {
    if run_id.trim().is_empty() {
        return Err(tonic::Status::invalid_argument("run_id is required"));
    }
    if org_id.trim().is_empty() || user_id.trim().is_empty() {
        return Err(tonic::Status::permission_denied(
            "verified user identity required for run mutation",
        ));
    }

    let request = authenticated_request(
        ResolveRunOwnerRequest {
            run_id: run_id.to_owned(),
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
        },
        Some(bearer),
    )
    .map_err(|_| tonic::Status::unauthenticated("verified session credential required"))?;

    let mut client = state.run_client.clone();
    let authorized = client
        .resolve_run_owner(request)
        .await
        .map(|response| response.into_inner().authorized)
        .map_err(|error| {
            tracing::warn!(code = ?error.code(), "Session Core run ownership lookup unavailable");
            tonic::Status::unavailable("run ownership unavailable")
        })?;
    if !authorized {
        return Err(tonic::Status::permission_denied("run access denied"));
    }
    Ok(())
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
        create_thread(
            &mut client,
            requested_session_key,
            org_id,
            user_id,
            bearer,
            None,
        )
        .await?
    };

    if let Err(error) = append_user_message(&mut client, &thread_id, goal, bearer, None).await {
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
                None,
            )
            .await?;
            append_user_message(&mut client, &thread_id, goal, bearer, None)
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
        org_id: org_id.to_owned(),
        already_started: false,
        terminal_source: ManagedRunSource::GatewayDirect,
    })
}

fn service_request<T>(value: T, bearer: &str) -> Result<tonic::Request<T>> {
    let mut request = tonic::Request::new(value);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {bearer}")
            .parse()
            .context("managed terminalization service credential is not forwardable")?,
    );
    Ok(request)
}

/// Submit a metadata-only terminal receipt using the Gateway's own scoped
/// workload identity. A delegated user bearer must never be reused here: the
/// service token is minted per org and can contain only `session:terminalize`.
async fn record_gateway_managed_terminal_outcome(
    state: &AppState,
    run_id: &str,
    org_id: &str,
    source: ManagedRunSource,
    outcome: TerminalOutcome,
    failure_code: &str,
) -> Result<()> {
    let provider = state.session_terminal_tokens.as_ref().context(
        "managed terminalization service credential unavailable; refusing delegated-user fallback",
    )?;
    let service_token = provider
        .terminalize_token(org_id)
        .await
        .context("unable to mint managed terminalization service credential")?;
    let mut client = state.managed_run_client.clone();
    client
        .record_terminal_outcome(service_request(
            RecordTerminalOutcomeRequest {
                run_id: run_id.to_owned(),
                source: source as i32,
                outcome: outcome as i32,
                failure_code: failure_code.to_owned(),
            },
            &service_token,
        )?)
        .await
        .context("session-core managed terminal receipt failed")?;
    Ok(())
}

/// Renew the server-owned managed-run deadline with a fixed-scope workload
/// token. The caller cannot provide a bearer, tenant, user, or deadline:
/// Session Core derives all of those from the durable run and service identity.
async fn heartbeat_gateway_managed_run(
    state: &AppState,
    run: &SessionRun,
    source: ManagedRunSource,
) -> Result<bool> {
    let provider = state.session_terminal_tokens.as_ref().context(
        "managed heartbeat service credential unavailable; refusing delegated-user fallback",
    )?;
    let service_token = provider
        .heartbeat_token(&run.org_id)
        .await
        .context("unable to mint managed heartbeat service credential")?;
    let mut client = state.managed_run_client.clone();
    let response = tokio::time::timeout(
        MANAGED_RUN_HEARTBEAT_TIMEOUT,
        client.heartbeat_managed_run(service_request(
            HeartbeatManagedRunRequest {
                run_id: run.run_id.clone(),
                source: source as i32,
            },
            &service_token,
        )?),
    )
    .await
    .context("session-core managed heartbeat timed out")?
    .context("session-core managed heartbeat failed")?
    .into_inner();
    Ok(!response.already_terminal)
}

fn require_gateway_owned_source(run: &SessionRun, expected: ManagedRunSource) -> Result<()> {
    if run.terminal_source != expected {
        anyhow::bail!("managed run has a different terminalization owner");
    }
    Ok(())
}

/// Renew a direct-inference managed run. `false` means Session Core reports
/// the run as already terminal; callers must not dispatch or expose success.
pub(crate) async fn heartbeat_direct_inference_run(
    state: &AppState,
    run: &SessionRun,
) -> Result<bool> {
    require_gateway_owned_source(run, ManagedRunSource::GatewayDirect)?;
    heartbeat_gateway_managed_run(state, run, ManagedRunSource::GatewayDirect).await
}

/// Renew the Gateway-owned standalone-browser producer lease. Execution Core
/// performs the browser action, but it must not impersonate GatewayBrowser to
/// extend this Gateway-owned managed run.
pub(crate) async fn heartbeat_browser_agent_run(
    state: &AppState,
    run: &SessionRun,
) -> Result<bool> {
    require_gateway_owned_source(run, ManagedRunSource::GatewayBrowser)?;
    heartbeat_gateway_managed_run(state, run, ManagedRunSource::GatewayBrowser).await
}

/// Obtain the initial direct-inference liveness receipt before provider work.
/// A missing, rejected, or terminal heartbeat fails closed so a new provider
/// request cannot outlive its durable terminalization obligation.
pub async fn ensure_direct_inference_run_liveness(
    state: &AppState,
    run: &SessionRun,
) -> Result<()> {
    if !heartbeat_direct_inference_run(state, run).await? {
        anyhow::bail!("managed direct inference run is already terminal");
    }
    Ok(())
}

/// Obtain the initial standalone-browser liveness receipt before handing work
/// to Execution Core. This still uses the Gateway's fixed service credential;
/// it never forwards the user's Session Core bearer.
pub async fn ensure_browser_agent_run_liveness(state: &AppState, run: &SessionRun) -> Result<()> {
    if !heartbeat_browser_agent_run(state, run).await? {
        anyhow::bail!("managed browser run is already terminal");
    }
    Ok(())
}

fn direct_terminal_outcome(terminal: DirectInferenceTerminal) -> (TerminalOutcome, &'static str) {
    match terminal {
        DirectInferenceTerminal::Completed => (TerminalOutcome::Completed, ""),
        // Preserve a bounded, content-free classification. The original code
        // is intentionally not forwarded because Session Core's durable
        // contract accepts only its fixed failure vocabulary.
        DirectInferenceTerminal::Failed("inference_unavailable" | "vision_unavailable") => {
            (TerminalOutcome::Failed, "provider_unavailable")
        }
        DirectInferenceTerminal::Failed("assistant_persist_failed" | "ingress_publish_failed") => {
            (TerminalOutcome::Failed, "execution_failed")
        }
        DirectInferenceTerminal::Failed("provider_timeout") => {
            (TerminalOutcome::Failed, "provider_timeout")
        }
        DirectInferenceTerminal::Failed(_) => (TerminalOutcome::Failed, "inference_failed"),
    }
}

/// Record the one Gateway-owned terminal receipt for direct inference. The
/// payload has no assistant content, error detail, or caller-controlled scope;
/// Session Core chooses the canonical step from `GatewayDirect`.
async fn terminalize_direct_inference_run_with_bearer(
    state: &AppState,
    run: &SessionRun,
    terminal: DirectInferenceTerminal,
    _delegated_user_bearer: Option<&str>,
) -> Result<()> {
    // The terminal receipt itself is a liveness boundary. Refuse to publish a
    // provider outcome if Session Core cannot first confirm that Gateway still
    // owns an active direct-inference lease; this avoids a late writer
    // terminalizing a recovered/reconciled run.
    if !heartbeat_direct_inference_run(state, run).await? {
        anyhow::bail!("managed direct inference run is already terminal before terminal receipt");
    }
    let (outcome, failure_code) = direct_terminal_outcome(terminal);
    record_gateway_managed_terminal_outcome(
        state,
        &run.run_id,
        &run.org_id,
        ManagedRunSource::GatewayDirect,
        outcome,
        failure_code,
    )
    .await
}

/// Terminalize a direct HTTP/SSE inference run using the independently
/// verified Session Core audience bearer.
///
/// # Errors
/// Returns an error when Session Core rejects or cannot durably assign the
/// single terminal outcome.
pub async fn terminalize_direct_inference_run_authenticated(
    state: &AppState,
    run: &SessionRun,
    terminal: DirectInferenceTerminal,
    bearer: &VerifiedSessionBearer,
) -> Result<()> {
    terminalize_direct_inference_run_with_bearer(state, run, terminal, Some(bearer.as_str())).await
}

/// Crate-internal terminalization adapter for an ingress that already verified
/// and bound the dedicated Session Core audience bearer.
pub(crate) async fn terminalize_direct_inference_run_with_token(
    state: &AppState,
    run: &SessionRun,
    terminal: DirectInferenceTerminal,
    bearer: &str,
) -> Result<()> {
    terminalize_direct_inference_run_with_bearer(state, run, terminal, Some(bearer)).await
}

/// Terminalize an agentic run only when Gateway has confirmed execution never
/// accepted it (for example, a pre-dispatch authentication or request-shape
/// rejection). Runtime/transport ambiguity must use the observable degraded
/// dispatch path instead: Gateway must never impersonate agent completion or
/// override an approval pause.
pub async fn terminalize_agent_dispatch_rejection_authenticated(
    state: &AppState,
    run: &SessionRun,
    _failure_code: &'static str,
    _delegated_user_bearer: &VerifiedSessionBearer,
) -> Result<()> {
    record_gateway_managed_terminal_outcome(
        state,
        &run.run_id,
        &run.org_id,
        ManagedRunSource::GatewayAgentDispatchRejected,
        TerminalOutcome::Failed,
        "dispatch_rejected",
    )
    .await
}

/// Persist the final state of a standalone browser-agent run after a confirmed
/// Execution Core response.
///
/// Output is metadata-only: Execution Core has already persisted the scrubbed
/// step outcome, and copying browser content again would unnecessarily widen
/// the retention surface.
pub async fn terminalize_browser_agent_run_authenticated(
    state: &AppState,
    run: &SessionRun,
    terminal: BrowserAgentTerminal,
) -> Result<()> {
    // Gateway, rather than Execution Core, owns the GatewayBrowser producer
    // source. A final browser response therefore also refreshes the exact
    // Gateway-owned lease before it can be turned into a terminal receipt.
    if !heartbeat_browser_agent_run(state, run).await? {
        anyhow::bail!("managed browser run is already terminal before terminal receipt");
    }
    let (outcome, failure_code) = match terminal {
        BrowserAgentTerminal::Completed => (TerminalOutcome::Completed, ""),
        BrowserAgentTerminal::Failed(_) => (TerminalOutcome::Failed, "browser_failed"),
    };
    record_gateway_managed_terminal_outcome(
        state,
        &run.run_id,
        &run.org_id,
        ManagedRunSource::GatewayBrowser,
        outcome,
        failure_code,
    )
    .await
}

/// Mark a run cancelled through Session Core's authoritative run lifecycle
/// service. A false acknowledgement means another terminal transition won the
/// race, so callers must not attempt a second outcome.
async fn cancel_run_with_bearer(
    state: &AppState,
    run_id: &str,
    reason: &'static str,
    bearer: Option<&str>,
) -> Result<bool> {
    let mut client = state.run_client.clone();
    let response = client
        .cancel_run(authenticated_request(
            CancelRunRequest {
                run_id: run_id.to_owned(),
                // Stable, metadata-only reason: do not copy user prompt or
                // downstream/provider errors into the durable cancellation.
                reason: reason.to_owned(),
            },
            bearer,
        )?)
        .await
        .context("session-core cancel_run failed")?;
    Ok(response.into_inner().cancelled)
}

/// Cancel a direct HTTP/SSE inference run with the independently verified
/// Session Core audience bearer.
///
/// # Errors
/// Returns an error when Session Core cannot authoritatively resolve the
/// cancellation.
pub async fn cancel_direct_inference_run_authenticated(
    state: &AppState,
    run: &SessionRun,
    bearer: &VerifiedSessionBearer,
) -> Result<bool> {
    cancel_run_with_bearer(
        state,
        &run.run_id,
        "client_stream_cancelled",
        Some(bearer.as_str()),
    )
    .await
}

/// Cancel a standalone browser-agent run after Execution Core confirms a
/// cancelled/aborted browser result. This is intentionally a `RunService`
/// cancellation rather than a terminal `CompleteStep`: Session Core only
/// accepts completed/failed terminal steps, and an abort must never be
/// projected as completed.
pub async fn cancel_browser_agent_run_authenticated(
    state: &AppState,
    run_id: &str,
    reason: &'static str,
    bearer: &VerifiedSessionBearer,
) -> Result<bool> {
    cancel_run_with_bearer(state, run_id, reason, Some(bearer.as_str())).await
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
                ..Default::default()
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

#[cfg(test)]
mod tests {
    use super::{
        managed_goal, prepare_managed_run_with_token, terminalize_direct_inference_run_with_token,
        DirectInferenceTerminal, SessionRun,
    };
    use crate::state::AppState;
    use mp_contracts::model_plane::v1::{
        managed_run_lifecycle_client::ManagedRunLifecycleClient,
        managed_run_lifecycle_server::{ManagedRunLifecycle, ManagedRunLifecycleServer},
        HeartbeatManagedRunRequest, HeartbeatManagedRunResponse, ManagedRunSource,
        RecordTerminalOutcomeRequest, RecordTerminalOutcomeResponse, StartManagedRunRequest,
        StartManagedRunResponse,
    };
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{transport::Endpoint, transport::Server, Request, Response, Status};

    #[derive(Clone)]
    struct CapturingManagedLifecycle {
        starts: Arc<Mutex<Vec<StartManagedRunRequest>>>,
    }

    #[tonic::async_trait]
    impl ManagedRunLifecycle for CapturingManagedLifecycle {
        async fn start_managed_run(
            &self,
            request: Request<StartManagedRunRequest>,
        ) -> Result<Response<StartManagedRunResponse>, Status> {
            self.starts.lock().unwrap().push(request.into_inner());
            Ok(Response::new(StartManagedRunResponse {
                run_id: "managed-run-1".to_owned(),
                created_at: None,
                terminal_step_id: "model-gateway-direct-inference-final".to_owned(),
                already_started: false,
                thread_id: "metadata-only-thread".to_owned(),
            }))
        }

        async fn record_terminal_outcome(
            &self,
            _: Request<RecordTerminalOutcomeRequest>,
        ) -> Result<Response<RecordTerminalOutcomeResponse>, Status> {
            Err(Status::unimplemented("not needed by managed-start test"))
        }

        async fn heartbeat_managed_run(
            &self,
            _: Request<HeartbeatManagedRunRequest>,
        ) -> Result<Response<HeartbeatManagedRunResponse>, Status> {
            Err(Status::unimplemented("not needed by managed-start test"))
        }
    }

    async fn managed_lifecycle_client(
        lifecycle: CapturingManagedLifecycle,
    ) -> ManagedRunLifecycleClient<tonic::transport::Channel> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind managed lifecycle test server");
        let address = listener
            .local_addr()
            .expect("managed lifecycle test server address");
        tokio::spawn(async move {
            Server::builder()
                .add_service(ManagedRunLifecycleServer::new(lifecycle))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .expect("managed lifecycle test server");
        });
        let channel = Endpoint::from_shared(format!("http://{address}"))
            .expect("managed lifecycle endpoint")
            .connect()
            .await
            .expect("connect managed lifecycle client");
        ManagedRunLifecycleClient::new(channel)
    }

    #[tokio::test]
    async fn direct_terminalization_never_falls_back_to_a_delegated_user_bearer() {
        let error = terminalize_direct_inference_run_with_token(
            &AppState::new(),
            &SessionRun {
                thread_id: "thread-1".to_owned(),
                run_id: "run-1".to_owned(),
                org_id: "org-1".to_owned(),
                already_started: false,
                terminal_source: ManagedRunSource::GatewayDirect,
            },
            DirectInferenceTerminal::Completed,
            "delegated-user-session-token",
        )
        .await
        .expect_err("a managed receipt needs a separately minted service token");

        assert!(
            error
                .to_string()
                .contains("service credential unavailable; refusing delegated-user fallback"),
            "expected service-token-only managed lifecycle failure, got: {error:#}"
        );
    }

    #[tokio::test]
    async fn zdr_managed_start_delegates_thread_creation_to_session_core() {
        let starts = Arc::new(Mutex::new(Vec::new()));
        let lifecycle = CapturingManagedLifecycle {
            starts: Arc::clone(&starts),
        };
        let mut state = AppState::new();
        state.managed_run_client = managed_lifecycle_client(lifecycle).await;

        let run = prepare_managed_run_with_token(
            &state,
            Some("caller-thread-must-not-be-persisted"),
            Some("caller-session-must-not-be-persisted"),
            "org-1",
            "user-1",
            "sensitive ZDR prompt",
            "model-gateway",
            "execute",
            "opaque-retry-key-1",
            ManagedRunSource::GatewayDirect,
            true,
            "delegated-user-session-token",
        )
        .await
        .expect("ZDR managed start");

        assert_eq!(run.run_id, "managed-run-1");
        assert_eq!(run.thread_id, "metadata-only-thread");
        let starts = starts.lock().unwrap();
        assert_eq!(starts.len(), 1);
        assert!(
            starts[0].thread_id.is_empty(),
            "the Gateway must let Session Core mint the ZDR metadata thread"
        );
        assert!(
            starts[0].goal.is_empty(),
            "the Gateway must not forward a ZDR prompt across the durable lifecycle boundary"
        );
        assert!(
            starts[0].agent_id.is_empty() && starts[0].mode.is_empty(),
            "the Gateway must not forward caller-controlled lifecycle content for ZDR"
        );
        assert_eq!(
            starts[0].terminal_source,
            ManagedRunSource::GatewayDirect as i32
        );
    }

    #[test]
    fn managed_goal_removes_control_characters_and_stays_within_session_contract() {
        let prompt = format!("Route this request.\n\nCustomer:\t{}", "æ".repeat(40_000));
        let normalized = managed_goal(&prompt);

        assert!(normalized.starts_with("Route this request. Customer:"));
        assert!(!normalized.chars().any(char::is_control));
        assert!(normalized.len() <= 60 * 1024);
        assert!(normalized.is_char_boundary(normalized.len()));
    }

    #[test]
    fn managed_goal_preserves_already_valid_prompts_for_retry_compatibility() {
        let prompt = "Route this customer message exactly as provided.";

        assert_eq!(managed_goal(prompt), prompt);
    }

    #[test]
    fn managed_goal_digest_preserves_retry_conflict_identity_after_truncation() {
        let shared_prefix = "x".repeat(70 * 1024);
        let first = managed_goal(&format!("{shared_prefix}-first"));
        let second = managed_goal(&format!("{shared_prefix}-second"));

        assert_ne!(first, second);
        assert!(first.contains("full-goal-blake3:"));
        assert!(second.contains("full-goal-blake3:"));
    }
}
