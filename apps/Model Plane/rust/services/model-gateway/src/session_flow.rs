use anyhow::{Context, Result};
use mp_contracts::model_plane::v1::{
    AppendMessageRequest, CancelRunRequest, CompleteDirectRunWithMessageRequest,
    CompleteDirectRunWithMessageResponse, CreateThreadRequest, HeartbeatManagedRunRequest,
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
/// Bound on one attempt of the atomic save-and-complete call. It does what an
/// append and a terminal receipt did, in one transaction, so it gets more room
/// than a heartbeat; a lost or slow answer is retried once with the same key.
const DIRECT_COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);
/// Attempts of the atomic call. The idempotency key makes a retry after a lost
/// response return the original result instead of saving a second message.
const DIRECT_COMPLETION_ATTEMPTS: usize = 2;
/// Session Core answers UNIMPLEMENTED for `CompleteDirectRunWithMessage` until
/// it is rebuilt; the Gateway then uses the two-call path and says so once.
static DIRECT_COMPLETION_UNIMPLEMENTED_LOGGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
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
                // Left for session-core to derive from space_id/session_key
                // conventions (see resolve_thread_origin) -- space context above
                // already yields "space", the support_* and agent_run/ session
                // key prefixes yield "support"/"agent_run", and everything else
                // is "chat". No caller here has a reason to override that.
                origin: String::new(),
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

/// Builds the durable `messages.metadata` Struct for the first-persisted user
/// turn (chat-wave2 scope spec §2.4): `source_scope` when the thread is
/// conversation-only, plus `retrieval_scope` when this turn's per-turn filter
/// is not `auto` — mirrored, not merged, with the immutable `source_scope`
/// write, since a `conversation_only` turn never carries a meaningful
/// `retrieval_scope` (that mode already excludes both source families).
/// `None` when there is nothing to record, matching today's byte-identical
/// absent-metadata behavior.
fn user_message_metadata(
    conversation_only: bool,
    retrieval_scope: Option<&str>,
) -> Option<prost_types::Struct> {
    let mut fields = std::collections::BTreeMap::new();
    if conversation_only {
        fields.insert("source_scope".to_owned(), json_to_prost_value(&serde_json::json!("conversation")));
    } else if let Some(scope) = retrieval_scope.filter(|s| !s.is_empty()) {
        fields.insert("retrieval_scope".to_owned(), json_to_prost_value(&serde_json::json!(scope)));
    }
    if fields.is_empty() {
        None
    } else {
        Some(prost_types::Struct { fields })
    }
}

async fn append_user_message(
    client: &mut mp_contracts::model_plane::v1::session_core_client::SessionCoreClient<
        tonic::transport::Channel,
    >,
    thread_id: &str,
    goal: &str,
    bearer: Option<&str>,
    space: Option<&ThreadSpaceContext>,
    conversation_only: bool,
    retrieval_scope: Option<&str>,
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
            metadata: user_message_metadata(conversation_only, retrieval_scope),
            agent_name: String::new(),
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
    conversation_only: bool,
    // Per-turn retrieval-source filter (chat-wave2 scope spec), threaded
    // only from the plain HTTP/SSE chat path — see `sse.rs`'s
    // `retrieval_scope` (already forced to `None`-equivalent under
    // `conversation_only` before it reaches here). `None` for every other
    // caller of this function, matching today's byte-identical behavior.
    retrieval_scope: Option<&str>,
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
        conversation_only,
        retrieval_scope,
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
        false,
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
    conversation_only: bool,
    retrieval_scope: Option<&str>,
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
            append_user_message(&mut session_client, &thread_id, goal, bearer, append_space, conversation_only, retrieval_scope).await
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
                append_user_message(&mut session_client, &thread_id, goal, bearer, append_space, conversation_only, retrieval_scope)
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

    if let Err(error) = append_user_message(&mut client, &thread_id, goal, bearer, None, false, None).await {
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
            append_user_message(&mut client, &thread_id, goal, bearer, None, false, None)
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
        // A draft whose check or repair the provider could not answer is a
        // provider outage too, not a rejected draft.
        DirectInferenceTerminal::Failed(
            "inference_unavailable" | "vision_unavailable" | "response_validation_unavailable",
        ) => (TerminalOutcome::Failed, "provider_unavailable"),
        // A ChatGPT subscription that refused or could not answer is a
        // provider outage in Session Core's fixed vocabulary; the reason
        // itself reached the client on the stream. A request ChatGPT rejected
        // is the request's own failure.
        DirectInferenceTerminal::Failed(
            crate::subscription_failure::SUBSCRIPTION_USAGE_LIMIT
            | crate::subscription_failure::SUBSCRIPTION_REAUTH_REQUIRED
            | crate::subscription_failure::SUBSCRIPTION_UPSTREAM_UNAVAILABLE
            | crate::subscription_failure::RATE_LIMITED
            | crate::subscription_failure::SUBSCRIPTION_ROUTE_UNAVAILABLE,
        ) => (TerminalOutcome::Failed, "provider_unavailable"),
        DirectInferenceTerminal::Failed(
            crate::subscription_failure::SUBSCRIPTION_BAD_REQUEST
            | crate::subscription_failure::PROMPT_TOO_LONG,
        ) => (TerminalOutcome::Failed, "inference_failed"),
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

/// The final assistant message of a completed direct turn.
pub(crate) struct DirectAnswer<'a> {
    pub content: &'a str,
    pub agent_name: Option<&'a str>,
    /// Build with `turn_evidence_metadata(...)`, as for an append.
    pub metadata: Option<prost_types::Struct>,
}

/// How completing a direct run with its answer ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectCompletion {
    /// The answer (when it had content) and the Completed receipt are durable.
    Completed,
    /// Session Core reports the run no longer active. Nothing was saved and no
    /// receipt may be attempted for it.
    AlreadyTerminal,
}

/// Why completing a direct run with its answer failed.
#[derive(Debug)]
pub(crate) enum DirectCompletionError {
    /// The answer was not saved: the atomic call failed, or on the two-call
    /// path the append failed. The run is still open.
    Persist(anyhow::Error),
    /// Two-call path only: the answer was saved but its terminal receipt was
    /// not recorded.
    Terminal(anyhow::Error),
}

/// Codes after which a retry with the same key is both safe and useful: the
/// call may or may not have committed, or failed transiently.
fn direct_completion_is_retryable(error: &anyhow::Error) -> bool {
    if error
        .downcast_ref::<tokio::time::error::Elapsed>()
        .is_some()
    {
        return true;
    }
    error.downcast_ref::<tonic::Status>().is_some_and(|status| {
        matches!(
            status.code(),
            Code::Unavailable
                | Code::DeadlineExceeded
                | Code::Unknown
                | Code::Cancelled
                | Code::Aborted
                | Code::Internal
        )
    })
}

/// Only UNIMPLEMENTED, and only on the first attempt, selects the two-call
/// path: the atomic call provably did not run. A refusal (PERMISSION_DENIED,
/// FAILED_PRECONDITION, UNAUTHENTICATED, ...) never does, even though it wrote
/// nothing. Routing around it would save with the delegated user bearer what
/// Session Core refused to save (another thread, another producer's run, a
/// zero-retention run), and it would hide a broken atomic path behind a quiet
/// return of the cancel race and the stale-bearer failure it exists to close.
fn direct_completion_is_unimplemented(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<tonic::Status>()
        .is_some_and(|status| status.code() == Code::Unimplemented)
}

/// A Session Core failure for a log line: every context layer, with a gRPC
/// status reduced to its code and Session Core's own reason, never its
/// metadata or details. No layer carries request content.
pub(crate) fn session_error_chain(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(|cause| match cause.downcast_ref::<tonic::Status>() {
            Some(status) => format!("status {:?}: {}", status.code(), status.message()),
            None => cause.to_string(),
        })
        .collect::<Vec<_>>()
        .join(": ")
}

/// The gRPC code of a Session Core failure, or `none` when the call got no
/// status (a timeout, or no service credential to make it with).
pub(crate) fn session_error_code(error: &anyhow::Error) -> String {
    error.downcast_ref::<tonic::Status>().map_or_else(
        || "none".to_owned(),
        |status| format!("{:?}", status.code()),
    )
}

/// One attempt of `CompleteDirectRunWithMessage` with the Gateway's own
/// terminalization credential, the one `RecordTerminalOutcome` uses.
async fn complete_direct_run_rpc(
    state: &AppState,
    run: &SessionRun,
    request: &CompleteDirectRunWithMessageRequest,
) -> Result<CompleteDirectRunWithMessageResponse> {
    let provider = state.session_terminal_tokens.as_ref().context(
        "managed terminalization service credential unavailable; refusing delegated-user fallback",
    )?;
    let service_token = provider
        .terminalize_token(&run.org_id)
        .await
        .context("unable to mint managed terminalization service credential")?;
    let mut client = state.managed_run_client.clone();
    let response = tokio::time::timeout(
        DIRECT_COMPLETION_TIMEOUT,
        client.complete_direct_run_with_message(service_request(request.clone(), &service_token)?),
    )
    .await
    .context("session-core direct run completion timed out")?
    .context("session-core direct run completion failed")?;
    Ok(response.into_inner())
}

/// Save a completed direct turn's answer and record its Completed receipt in
/// one Session Core transaction.
///
/// This closes two gaps of the two-call path: a durable cancel that landed
/// between the append and the receipt left a saved answer on a cancelled run,
/// and the append rode the delegated user bearer minted at the start of the
/// request, which a long turn can outlive. The atomic call uses the Gateway's
/// scoped service credential instead; Session Core derives the owner from the
/// run. `idempotency_key` (the request id) makes a retry after a lost response
/// return the original result, so one retry is made on an ambiguous failure.
///
/// While Session Core answers UNIMPLEMENTED (not yet rebuilt), the old path
/// runs: append with `bearer`, then the heartbeat-guarded terminal receipt.
///
/// With `timing`, the atomic call is recorded as `durable_append` and the
/// receipt, which arrives with it, as an empty `terminal_ack` at its end; the
/// two-call path records each call as before.
pub(crate) async fn complete_direct_inference_run_with_message(
    state: &AppState,
    run: &SessionRun,
    answer: DirectAnswer<'_>,
    idempotency_key: &str,
    bearer: &VerifiedSessionBearer,
    timing: Option<&crate::turn_timing::TurnTiming>,
) -> std::result::Result<DirectCompletion, DirectCompletionError> {
    use crate::turn_timing::Phase;

    require_gateway_owned_source(run, ManagedRunSource::GatewayDirect)
        .map_err(DirectCompletionError::Persist)?;
    let content = answer.content.trim();
    let agent_name = answer.agent_name.unwrap_or_default().trim();
    let request = CompleteDirectRunWithMessageRequest {
        run_id: run.run_id.clone(),
        thread_id: run.thread_id.clone(),
        content: content.to_owned(),
        agent_name: agent_name.to_owned(),
        metadata: answer.metadata,
        idempotency_key: idempotency_key.to_owned(),
    };

    let started = timing.map(crate::turn_timing::TurnTiming::now_ms);
    let mut attempt = 0;
    let outcome = loop {
        attempt += 1;
        match complete_direct_run_rpc(state, run, &request).await {
            Err(error)
                if attempt < DIRECT_COMPLETION_ATTEMPTS
                    && direct_completion_is_retryable(&error) =>
            {
                tracing::warn!(
                    error = %session_error_chain(&error),
                    grpc_code = %session_error_code(&error),
                    run_id = %run.run_id,
                    request_id = %idempotency_key,
                    attempt,
                    "direct run completion failed; retrying with the same key"
                );
            }
            other => break other,
        }
    };
    match outcome {
        Ok(response) => {
            if let (Some(timing), Some(started)) = (timing, started) {
                let ended = timing.close(Phase::DurableAppend, started);
                timing.close(Phase::TerminalAck, ended);
            }
            Ok(if response.already_terminal {
                DirectCompletion::AlreadyTerminal
            } else {
                DirectCompletion::Completed
            })
        }
        // Only when the FIRST attempt was refused as unknown. A retry that
        // reaches a not-yet-rebuilt replica after an ambiguous first attempt
        // may follow a commit; appending then could save the answer twice, so
        // that case stays an unsaved-answer failure below.
        Err(error) if attempt == 1 && direct_completion_is_unimplemented(&error) => {
            if !DIRECT_COMPLETION_UNIMPLEMENTED_LOGGED
                .swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                tracing::warn!(
                    run_id = %run.run_id,
                    "Session Core does not implement CompleteDirectRunWithMessage yet; saving the answer and completing the run in two calls until it is rebuilt"
                );
            }
            let append_started = timing.map(crate::turn_timing::TurnTiming::now_ms);
            let appended = append_assistant_message_with_bearer(
                state,
                &run.thread_id,
                content,
                Some(bearer.as_str()),
                Some(agent_name),
                request.metadata,
            )
            .await;
            if let (Some(timing), Some(started)) = (timing, append_started) {
                timing.close(Phase::DurableAppend, started);
            }
            appended.map_err(DirectCompletionError::Persist)?;
            let ack_started = timing.map(crate::turn_timing::TurnTiming::now_ms);
            let acknowledged = terminalize_direct_inference_run_with_bearer(
                state,
                run,
                DirectInferenceTerminal::Completed,
                Some(bearer.as_str()),
            )
            .await;
            if let (Some(timing), Some(started)) = (timing, ack_started) {
                timing.close(Phase::TerminalAck, started);
            }
            acknowledged.map_err(DirectCompletionError::Terminal)?;
            Ok(DirectCompletion::Completed)
        }
        Err(error) => {
            if let (Some(timing), Some(started)) = (timing, started) {
                timing.close(Phase::DurableAppend, started);
            }
            Err(DirectCompletionError::Persist(error))
        }
    }
}

/// Why a prepared agent run is being closed without ever having run.
///
/// Both variants share the `GatewayAgentDispatchRejected` source — the
/// gateway's dispatch is the producer that failed either way — and differ only
/// in the durable failure code, so an operator reading a receipt can tell a
/// refused request from an unreachable runner. Deliberately a type rather than
/// a `&str`: the receipt vocabulary (`dispatch_*`) is NOT the client-facing SSE
/// vocabulary (`agent_dispatch_*`), and passing one where the other belongs
/// would be silently rejected by Session Core's allowlist at runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentDispatchFailure {
    /// Execution Core answered with a deterministic refusal, so retrying the
    /// same request reproduces it.
    Rejected,
    /// The request never left the gateway because the connect phase failed, so
    /// nothing ran and a fresh attempt may succeed.
    Unreachable,
}

impl AgentDispatchFailure {
    /// The allowlisted `failure_code` persisted on the terminal receipt. Must
    /// stay within `terminalization::ALLOWED_FAILURE_CODES` in Session Core and
    /// the matching CHECK on `managed_run_terminalization_outbox`.
    fn failure_code(self) -> &'static str {
        match self {
            Self::Rejected => "dispatch_rejected",
            Self::Unreachable => "dispatch_unreachable",
        }
    }
}

/// Terminalize an agentic run only when Gateway has confirmed execution never
/// accepted it — either a pre-dispatch rejection (authentication or
/// request-shape) or a dispatch that never left this process. Runtime/transport
/// ambiguity must use the observable degraded dispatch path instead: Gateway
/// must never impersonate agent completion or override an approval pause.
pub async fn terminalize_agent_dispatch_rejection_authenticated(
    state: &AppState,
    run: &SessionRun,
    failure: AgentDispatchFailure,
    _delegated_user_bearer: &VerifiedSessionBearer,
) -> Result<()> {
    record_gateway_managed_terminal_outcome(
        state,
        &run.run_id,
        &run.org_id,
        ManagedRunSource::GatewayAgentDispatchRejected,
        TerminalOutcome::Failed,
        failure.failure_code(),
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
    append_assistant_message_with_bearer(state, thread_id, content, None, None, None).await
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
    agent_name: Option<&str>,
    // Build with `grounding_metadata(...)`; `None` on paths that ground
    // nothing (vision and image-generation streams).
    metadata: Option<prost_types::Struct>,
) -> Result<()> {
    append_assistant_message_with_bearer(
        state,
        thread_id,
        content,
        Some(bearer.as_str()),
        agent_name,
        metadata,
    )
    .await
}

/// Crate-internal adapter for an ingress boundary that has already verified a
/// dedicated `aud=session-core` bearer and bound it to the Model Plane caller.
pub(crate) async fn append_assistant_message_with_token(
    state: &AppState,
    thread_id: &str,
    content: &str,
    bearer: &str,
) -> Result<()> {
    append_assistant_message_with_bearer(state, thread_id, content, Some(bearer), None, None).await
}

async fn append_assistant_message_with_bearer(
    state: &AppState,
    thread_id: &str,
    content: &str,
    bearer: Option<&str>,
    // The persona this turn answered as, recorded on the message for the
    // room's per-turn attribution. Identity history, never authority.
    agent_name: Option<&str>,
    // The turn's evidence (grounding, and the citations nested inside it),
    // persisted so reopening the thread on another device can still show
    // where the answer came from. `None` for turns that grounded nothing.
    metadata: Option<prost_types::Struct>,
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
                metadata,
                agent_name: agent_name.unwrap_or_default().trim().to_owned(),
                ..Default::default()
            },
            bearer,
        )?)
        .await
        .context("session-core append_message(assistant) failed")?;

    Ok(())
}

/// Shape a turn's evidence into the `metadata` Struct persisted with the
/// assistant message.
///
/// Two independent sources, because the product has two:
/// - `grounding` — Data Plane retrieval (knowledge base). Written under the
///   `grounding` key, exactly what the SPA reads as `turn.grounding`.
/// - `citations` — everything the tool loop cited (web search, deep research).
///   These never appear in `Grounding`; they stream straight out through
///   `RichEventSink` and are captured there. Written under `citations`, which
///   the SPA reads as `turn.citations`.
///
/// A turn can have either, both, or neither. Returns `None` for neither, so an
/// ungrounded turn stores no metadata rather than an empty object.
///
/// - `artifacts` — the turn's authored work product (documents, code, HTML),
///   latest version per id, written under `artifacts` which the SPA reads as
///   `turn.artifacts`. Before this, the durable read returned zero artifacts
///   for a turn that had produced three (RUN-LOG finding 3): the panel was
///   empty on any device but the one that streamed the turn.
///
/// Generated-image and file artifacts are deliberately excluded: on that path
/// they are base64 data URIs, far too large to belong in a message metadata
/// column (`RecordedArtifact::from_event` never records them).
pub(crate) fn with_compaction_summary(
    metadata: Option<prost_types::Struct>, summary: &str, zdr: bool,
) -> Option<prost_types::Struct> {
    if zdr || summary.trim().is_empty() || summary.len() > 128_000 { return metadata; }
    let mut metadata = metadata.unwrap_or_default();
    metadata.fields.insert("native_compaction_summary".to_owned(), json_to_prost_value(&serde_json::json!(summary)));
    Some(metadata)
}

pub(crate) fn persisted_compaction_summary(metadata: Option<&prost_types::Struct>) -> String {
    match metadata.and_then(|m| m.fields.get("native_compaction_summary")).and_then(|v| v.kind.as_ref()) {
        Some(prost_types::value::Kind::StringValue(value)) if value.len() <= 128_000 => value.clone(),
        _ => String::new(),
    }
}

/// Metadata key for a token-budget compaction summary
/// (`compaction::StoredHeadSummary`).
const HEAD_SUMMARY_KEY: &str = "head_summary";

/// Persist a compaction summary with the answer of the turn that wrote it, so
/// later turns send the same summary again instead of writing a new one.
/// Skipped under ZDR and over the same size cap as the native checkpoint.
pub(crate) fn with_head_summary(
    metadata: Option<prost_types::Struct>,
    record: Option<&crate::compaction::StoredHeadSummary>,
    zdr: bool,
) -> Option<prost_types::Struct> {
    let Some(record) = record else {
        return metadata;
    };
    if zdr
        || record.summary.trim().is_empty()
        || record.through_id.trim().is_empty()
        || record.summary.len() > 128_000
    {
        return metadata;
    }
    let mut metadata = metadata.unwrap_or_default();
    metadata.fields.insert(HEAD_SUMMARY_KEY.to_owned(), json_to_prost_value(&serde_json::json!({
        "summary": record.summary, "through_id": record.through_id, "pinned_ids": record.pinned_ids,
    })));
    Some(metadata)
}

/// The compaction summary a turn persisted, if its record is complete.
pub(crate) fn persisted_head_summary(
    metadata: Option<&prost_types::Struct>,
) -> Option<crate::compaction::StoredHeadSummary> {
    use prost_types::value::Kind;
    let Some(Kind::StructValue(record)) = metadata?.fields.get(HEAD_SUMMARY_KEY)?.kind.as_ref()
    else {
        return None;
    };
    let text = |key: &str| match record.fields.get(key).and_then(|v| v.kind.as_ref()) {
        Some(Kind::StringValue(value)) => Some(value.clone()),
        _ => None,
    };
    let (summary, through_id) = (text("summary")?, text("through_id")?);
    if summary.trim().is_empty() || summary.len() > 128_000 || through_id.trim().is_empty() {
        return None;
    }
    // Every record lists its pins, an empty list included; one that does not
    // cannot say which pins its summary left out.
    let Some(Kind::ListValue(pins)) = record
        .fields
        .get("pinned_ids")
        .and_then(|v| v.kind.as_ref())
    else {
        return None;
    };
    let pinned_ids = pins
        .values
        .iter()
        .map(|value| match &value.kind {
            Some(Kind::StringValue(id)) => Some(id.clone()),
            _ => None,
        })
        .collect::<Option<Vec<String>>>()?;
    Some(crate::compaction::StoredHeadSummary {
        summary,
        through_id,
        pinned_ids,
    })
}

pub(crate) fn turn_evidence_metadata(
    grounding: Option<&crate::retrieval::Grounding>,
    citations: &[crate::sse_events::RecordedCitation],
    artifacts: &[crate::sse_events::RecordedArtifact],
    quality: TurnQuality,
) -> Option<prost_types::Struct> {
    let mut fields = std::collections::BTreeMap::new();

    if let Some(payload) = grounding.filter(|g| !g.is_empty()) {
        if let Ok(value) = serde_json::to_value(payload) {
            fields.insert("grounding".to_owned(), json_to_prost_value(&value));
        }
    }

    if !citations.is_empty() {
        if let Ok(value) = serde_json::to_value(citations) {
            fields.insert("citations".to_owned(), json_to_prost_value(&value));
        }
    }

    if !artifacts.is_empty() {
        if let Ok(value) = serde_json::to_value(artifacts) {
            fields.insert("artifacts".to_owned(), json_to_prost_value(&value));
        }
    }

    // Persisted so the score survives the turn that produced it. It arrives on
    // the `usage` SSE event, which only the browser watching the stream ever
    // sees; a thread reopened on another device replayed the answer with no
    // score at all, and the UI cannot tell "unscored" from "never checked".
    if let Some(confidence) = quality.confidence.filter(|value| value.is_finite()) {
        fields.insert(
            "confidence".to_owned(),
            json_to_prost_value(&serde_json::json!(confidence)),
        );
    }
    if let Some(verification) = quality.verification {
        fields.insert(
            "verification".to_owned(),
            json_to_prost_value(&serde_json::json!({
                "verdict": verification.verdict,
                "kbCitations": verification.kb_citations,
                "webCitations": verification.web_citations,
                "webAllowed": verification.web_allowed,
            })),
        );
    }

    if fields.is_empty() {
        return None;
    }
    Some(prost_types::Struct { fields })
}

/// The turn's answer-quality assessment, for persistence alongside its
/// evidence. Both halves are optional: a turn may be unscored, and only a
/// low-scoring one is ever verified.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TurnQuality {
    pub(crate) confidence: Option<f64>,
    pub(crate) verification: Option<TurnVerification>,
}

/// What the verification pass found, in the shape the SPA reads back.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TurnVerification {
    pub(crate) verdict: &'static str,
    pub(crate) kb_citations: u32,
    pub(crate) web_citations: u32,
    pub(crate) web_allowed: bool,
}

/// Add an exact-result receipt to the same durable turn as the result itself.
/// Retention and authenticated ownership remain those of the existing append.
pub(crate) fn with_result_receipt(
    metadata: Option<prost_types::Struct>,
    receipt: Option<&serde_json::Value>,
) -> Option<prost_types::Struct> {
    let Some(receipt) = receipt else { return metadata; };
    let mut metadata = metadata.unwrap_or_default();
    metadata.fields.insert("resultReceipt".to_owned(), json_to_prost_value(receipt));
    Some(metadata)
}

fn json_to_prost_value(value: &serde_json::Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match value {
        serde_json::Value::Null => Kind::NullValue(0),
        serde_json::Value::Bool(b) => Kind::BoolValue(*b),
        serde_json::Value::Number(n) => n
            .as_f64()
            .map_or_else(|| Kind::StringValue(n.to_string()), Kind::NumberValue),
        serde_json::Value::String(s) => Kind::StringValue(s.clone()),
        serde_json::Value::Array(items) => Kind::ListValue(prost_types::ListValue {
            values: items.iter().map(json_to_prost_value).collect(),
        }),
        serde_json::Value::Object(map) => Kind::StructValue(prost_types::Struct {
            fields: map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_prost_value(v)))
                .collect(),
        }),
    };
    prost_types::Value { kind: Some(kind) }
}

/// Persist a user message the caller sent MID-RUN, so the durable thread
/// records what the user actually said and when.
///
/// Without this the assistant's reply to a queued message would appear in
/// history with nothing prompting it — the answer to a question the transcript
/// does not contain.
///
/// `space` carries the fresh, content-bound `thread:append` decision the gateway
/// minted for THIS message. It is required for a Space-scoped thread and absent
/// for an ordinary one; the empty default below is what an ordinary thread's
/// append has always sent, so this path grants no authority the normal send
/// path does not.
///
/// # Errors
/// Returns an error when the credential cannot be forwarded, or Session Core
/// rejects or cannot complete the append.
pub async fn append_queued_user_message(
    state: &AppState,
    thread_id: &str,
    content: &str,
    bearer: &VerifiedSessionBearer,
    space: Option<&ThreadSpaceContext>,
    conversation_only: bool,
) -> Result<(), tonic::Status> {
    let mut client = state.session_client.clone();
    append_user_message(
        &mut client,
        thread_id,
        content,
        Some(bearer.as_str()),
        space,
        conversation_only,
        // Deliberately `None`: chat-wave2 scope spec §5's mid-run rule
        // already has the in-flight stream enforce its OWN retrieval_scope
        // (fixed at stream start) against a queued message; persisting a
        // scope value for the queued message's own durable row is not part
        // of this item's receipt/citation guarantee (see `EnqueueOutcome`'s
        // doc comment in `queued_input.rs`).
        None,
    )
    .await
    .map_err(|error| {
        tracing::warn!(%error, %thread_id, "session-core append_message(queued user) failed");
        error
    })
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
    #[test]
    fn native_checkpoint_survives_durable_metadata_encoding_and_respects_zdr() {
        use prost::Message;
        let metadata = super::with_compaction_summary(None, "K3; 9 packed; 3 pending", false).unwrap();
        let stored = metadata.encode_to_vec();
        let reloaded = prost_types::Struct::decode(stored.as_slice()).unwrap();
        assert_eq!(super::persisted_compaction_summary(Some(&reloaded)), "K3; 9 packed; 3 pending");
        assert!(super::with_compaction_summary(None, "private source", true).is_none());
        assert!(super::with_compaction_summary(None, &"x".repeat(128_001), false).is_none());
        assert!(super::persisted_compaction_summary(None).is_empty());
    }

    /// A reload shows a failed run by its recorded code, so a draft the
    /// provider could not check must read as a provider outage there too.
    #[test]
    fn a_draft_check_the_provider_could_not_answer_is_recorded_as_an_outage() {
        use super::{direct_terminal_outcome, DirectInferenceTerminal, TerminalOutcome};
        let unavailable = crate::result_validation::VALIDATION_UNAVAILABLE;
        assert_eq!(
            direct_terminal_outcome(DirectInferenceTerminal::Failed(unavailable)),
            (TerminalOutcome::Failed, "provider_unavailable")
        );
        let rejected = crate::result_validation::VALIDATION_FAILED;
        assert_eq!(
            direct_terminal_outcome(DirectInferenceTerminal::Failed(rejected)),
            (TerminalOutcome::Failed, "inference_failed")
        );
    }

    /// Session Core's durable vocabulary is fixed, so each subscription code
    /// lands on the nearest allowed one: an unanswered subscription is a
    /// provider outage, a request ChatGPT rejected is the request's failure.
    #[test]
    fn subscription_failures_map_onto_the_fixed_durable_vocabulary() {
        use super::{direct_terminal_outcome, DirectInferenceTerminal, TerminalOutcome};
        use crate::subscription_failure as sf;
        for (code, durable) in [
            (sf::SUBSCRIPTION_USAGE_LIMIT, "provider_unavailable"),
            (sf::SUBSCRIPTION_REAUTH_REQUIRED, "provider_unavailable"),
            (sf::SUBSCRIPTION_UPSTREAM_UNAVAILABLE, "provider_unavailable"),
            (sf::RATE_LIMITED, "provider_unavailable"),
            (sf::SUBSCRIPTION_ROUTE_UNAVAILABLE, "provider_unavailable"),
            (sf::SUBSCRIPTION_BAD_REQUEST, "inference_failed"),
            (sf::PROMPT_TOO_LONG, "inference_failed"),
        ] {
            assert_eq!(
                direct_terminal_outcome(DirectInferenceTerminal::Failed(code)),
                (TerminalOutcome::Failed, durable),
                "{code}"
            );
            assert_eq!(
                crate::turn_timing::Outcome::of_failure(code),
                crate::turn_timing::Outcome::Failed,
                "{code}"
            );
        }
    }

    #[test]
    fn a_head_summary_survives_durable_metadata_encoding_and_respects_zdr() {
        use prost::Message;
        let record = crate::compaction::StoredHeadSummary {
            summary: "K3; 9 packed; NOK".to_owned(),
            through_id: "msg-17".to_owned(),
            pinned_ids: vec!["msg-3".to_owned()],
        };
        let metadata = super::with_head_summary(None, Some(&record), false).unwrap();
        let reloaded = prost_types::Struct::decode(metadata.encode_to_vec().as_slice()).unwrap();
        assert_eq!(
            super::persisted_head_summary(Some(&reloaded)),
            Some(record.clone())
        );
        // Beside the other keys, never instead of them.
        let native = super::with_compaction_summary(None, "checkpoint", false);
        let both = super::with_head_summary(native, Some(&record), false).unwrap();
        assert_eq!(
            super::persisted_compaction_summary(Some(&both)),
            "checkpoint"
        );
        assert_eq!(
            super::persisted_head_summary(Some(&both)),
            Some(record.clone())
        );
        // ZDR, an empty or oversized summary, or no record: nothing is written.
        assert!(super::with_head_summary(None, Some(&record), true).is_none());
        assert!(super::with_head_summary(None, None, false).is_none());
        let empty = crate::compaction::StoredHeadSummary {
            summary: "  ".to_owned(),
            ..record.clone()
        };
        assert!(super::with_head_summary(None, Some(&empty), false).is_none());
        let huge = crate::compaction::StoredHeadSummary {
            summary: "x".repeat(128_001),
            ..record.clone()
        };
        assert!(super::with_head_summary(None, Some(&huge), false).is_none());
        let unanchored = crate::compaction::StoredHeadSummary {
            through_id: String::new(),
            ..record
        };
        assert!(super::with_head_summary(None, Some(&unanchored), false).is_none());
        assert!(super::persisted_head_summary(None).is_none());
    }

    #[test]
    fn an_incomplete_head_summary_record_is_ignored() {
        let record = |value: serde_json::Value| prost_types::Struct {
            fields: [(
                "head_summary".to_owned(),
                super::json_to_prost_value(&value),
            )]
            .into_iter()
            .collect(),
        };
        for value in [
            serde_json::json!("just text"),
            serde_json::json!({ "summary": "s", "through_id": "m1" }),
            serde_json::json!({ "summary": "s", "through_id": "m1", "pinned_ids": "m2" }),
            serde_json::json!({ "summary": "s", "through_id": "m1", "pinned_ids": [7] }),
            serde_json::json!({ "summary": "", "through_id": "m1", "pinned_ids": [] }),
            serde_json::json!({ "summary": "s", "through_id": " ", "pinned_ids": [] }),
        ] {
            assert!(
                super::persisted_head_summary(Some(&record(value.clone()))).is_none(),
                "{value}"
            );
        }
        let complete =
            record(serde_json::json!({ "summary": "s", "through_id": "m1", "pinned_ids": [] }));
        assert_eq!(
            super::persisted_head_summary(Some(&complete)).map(|r| r.pinned_ids.len()),
            Some(0)
        );
    }

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

        async fn record_run_output(
            &self,
            _: Request<mp_contracts::model_plane::v1::RecordRunOutputRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::RecordRunOutputResponse>, Status>
        {
            Err(Status::unimplemented("not needed by managed-start test"))
        }

        async fn complete_direct_run_with_message(
            &self,
            _: Request<mp_contracts::model_plane::v1::CompleteDirectRunWithMessageRequest>,
        ) -> Result<
            Response<mp_contracts::model_plane::v1::CompleteDirectRunWithMessageResponse>,
            Status,
        > {
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

    /// The atomic completion writes content, so it must not quietly fall back
    /// to the delegated user bearer when the service credential is missing:
    /// that bearer is what a long turn outlives.
    #[tokio::test]
    async fn direct_completion_never_falls_back_to_a_delegated_user_bearer() {
        let outcome = super::complete_direct_inference_run_with_message(
            &AppState::new(),
            &SessionRun {
                thread_id: "thread-1".to_owned(),
                run_id: "run-1".to_owned(),
                org_id: "org-1".to_owned(),
                already_started: false,
                terminal_source: ManagedRunSource::GatewayDirect,
            },
            super::DirectAnswer {
                content: "Svar.",
                agent_name: None,
                metadata: None,
            },
            "01J00000000000000000000000",
            &crate::auth::VerifiedSessionBearer::for_test("delegated-user-session-token"),
            None,
        )
        .await;

        let Err(super::DirectCompletionError::Persist(error)) = outcome else {
            panic!("expected an unsaved-answer failure, got {outcome:?}");
        };
        assert!(
            format!("{error:#}")
                .contains("service credential unavailable; refusing delegated-user fallback"),
            "{error:#}"
        );
    }

    /// Another producer's run is refused before any Session Core call.
    #[tokio::test]
    async fn direct_completion_is_only_for_gateway_direct_runs() {
        let outcome = super::complete_direct_inference_run_with_message(
            &AppState::new(),
            &SessionRun {
                thread_id: "thread-1".to_owned(),
                run_id: "run-1".to_owned(),
                org_id: "org-1".to_owned(),
                already_started: false,
                terminal_source: ManagedRunSource::GatewayBrowser,
            },
            super::DirectAnswer {
                content: "Svar.",
                agent_name: None,
                metadata: None,
            },
            "01J00000000000000000000000",
            &crate::auth::VerifiedSessionBearer::for_test("delegated-user-session-token"),
            None,
        )
        .await;
        assert!(
            matches!(outcome, Err(super::DirectCompletionError::Persist(_))),
            "{outcome:?}"
        );
    }

    /// Only a failure after which the call may or may not have committed, or a
    /// transient one, is retried with the same key; a refusal is final, and
    /// UNIMPLEMENTED selects the two-call path instead.
    #[tokio::test]
    async fn direct_completion_retries_only_ambiguous_failures() {
        use anyhow::Context as _;
        let status = |code: tonic::Code| {
            Err::<(), _>(tonic::Status::new(code, "x"))
                .context("session-core direct run completion failed")
                .unwrap_err()
        };
        for code in [
            tonic::Code::Unavailable,
            tonic::Code::DeadlineExceeded,
            tonic::Code::Unknown,
            tonic::Code::Cancelled,
            tonic::Code::Aborted,
            tonic::Code::Internal,
        ] {
            assert!(
                super::direct_completion_is_retryable(&status(code)),
                "{code:?}"
            );
        }
        for code in [
            tonic::Code::InvalidArgument,
            tonic::Code::NotFound,
            tonic::Code::PermissionDenied,
            tonic::Code::FailedPrecondition,
            tonic::Code::Unauthenticated,
            tonic::Code::Unimplemented,
        ] {
            assert!(
                !super::direct_completion_is_retryable(&status(code)),
                "{code:?}"
            );
        }
        assert!(super::direct_completion_is_unimplemented(&status(
            tonic::Code::Unimplemented
        )));
        // A refusal is never routed around through the two-call path.
        for code in [
            tonic::Code::Unavailable,
            tonic::Code::PermissionDenied,
            tonic::Code::FailedPrecondition,
            tonic::Code::Unauthenticated,
            tonic::Code::InvalidArgument,
            tonic::Code::NotFound,
        ] {
            assert!(
                !super::direct_completion_is_unimplemented(&status(code)),
                "{code:?}"
            );
        }
        let elapsed = tokio::time::timeout(std::time::Duration::ZERO, std::future::pending::<()>())
            .await
            .context("session-core direct run completion timed out")
            .unwrap_err();
        assert!(super::direct_completion_is_retryable(&elapsed));
        assert!(!super::direct_completion_is_retryable(&anyhow::anyhow!(
            "unable to mint managed terminalization service credential"
        )));
    }

    /// The 2026-09-24 log line said only "session-core direct run completion
    /// failed", the outer context. The rendering keeps every layer and the
    /// status code and reason, and drops the status metadata.
    #[tokio::test]
    async fn a_failed_direct_completion_logs_its_whole_chain_and_code_without_metadata() {
        use anyhow::Context as _;
        let mut refused = tonic::Status::failed_precondition(
            "ZDR credentials cannot access durable session writes",
        );
        refused
            .metadata_mut()
            .insert("x-response-secret", "must-not-be-logged".parse().unwrap());
        let error = Err::<(), _>(refused)
            .context("session-core direct run completion failed")
            .unwrap_err();

        assert_eq!(
            super::session_error_chain(&error),
            "session-core direct run completion failed: status FailedPrecondition: \
             ZDR credentials cannot access durable session writes"
        );
        assert_eq!(super::session_error_code(&error), "FailedPrecondition");
        assert!(!super::session_error_chain(&error).contains("must-not-be-logged"));

        let timed_out =
            tokio::time::timeout(std::time::Duration::ZERO, std::future::pending::<()>())
                .await
                .context("session-core direct run completion timed out")
                .unwrap_err();
        assert_eq!(
            super::session_error_chain(&timed_out),
            "session-core direct run completion timed out: deadline has elapsed"
        );
        assert_eq!(super::session_error_code(&timed_out), "none");
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
