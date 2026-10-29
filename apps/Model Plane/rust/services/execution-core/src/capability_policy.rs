//! Authoritative capability policy enforcement for execution dispatch.
//!
//! Tool names are mapped to capability identifiers by trusted server code;
//! callers and model output can never pair an allowed capability with a
//! different executor. Unknown tools fail closed before dispatch.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use mp_contracts::model_plane::v1::{
    capability_core_client::CapabilityCoreClient, EvaluatePolicyRequest, EvaluatePolicyResponse,
    ResolveToolBindingsRequest, ToolBinding as WireToolBinding,
};
use mp_contracts::tool_class::{effective_floor, ApprovalFloor, SideEffectClass};
use reqwest::{Client, Url};
use serde::Deserialize;
use tonic::transport::Channel;
use tonic::{Request, Status};

const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(30);
const MAX_TOKEN_TTL_SECONDS: u64 = 3_600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityDecision {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone)]
pub struct CapabilityEvaluation {
    pub decision: CapabilityDecision,
    pub decision_id: String,
    pub capability_version: String,
    pub evidence_verified: bool,
    /// What produced the decision, as capability-core reports it
    /// (`risk_level`, `approval_floor`, `tool_review`, `capability_gate`); empty
    /// from a policy that does not say. It only picks which pause cause an `ask`
    /// is shown under.
    pub policy_source: String,
    /// The class and approval floor the decision carried for the tool, as the policy
    /// worded them; empty from a policy that does not say. A governed MCP call is
    /// compared with them at dispatch (WS05-S4): the offer's snapshot of the class must
    /// still be the one capability-core just decided on.
    pub side_effect_class: String,
    pub approval_floor: String,
}

impl CapabilityEvaluation {
    fn unverified(decision: CapabilityDecision) -> Self {
        // Test/local policy implementations predate the wire proof. The
        // production gRPC implementation below always verifies evidence;
        // preserving this default keeps pure runtime-loop tests focused on
        // dispatch semantics without inventing a fake wire token.
        Self {
            decision,
            decision_id: String::new(),
            capability_version: String::new(),
            evidence_verified: true,
            policy_source: String::new(),
            side_effect_class: String::new(),
            approval_floor: String::new(),
        }
    }
}

/// How far execution-core trusts the registry's declared tool class
/// (`TOOL_CLASS_GATE`, build plan WS05).
///
/// * `Off`: the request and the verification are exactly what they were before
///   tool classes existed, and an `mcp__` name has no binding.
/// * `Shadow`: the request names the tool and the user and the signed v2 claims
///   are checked against them. A mismatch is counted and logged while the legacy
///   answer still decides: the legacy tuple for a built-in tool, a refusal for an
///   `mcp__` name, which is resolved and checked but never dispatchable here.
/// * `Enforce`: every v2 check decides, v1 evidence is refused and a registered
///   `mcp__` tool can be allowed.
///
/// Parsed strictly: only the exact literals `shadow` and `enforce` move off
/// `Off`, so a typo can only leave the gate off, never half on.
///
/// Rollout order: execution-core first, then capability-core. capability-core
/// always signs v2 decisions, and this verifier accepts v1 and v2 while the gate
/// is `Off` or `Shadow`, so an execution-core that carries this code is safe
/// against either signer. Only an execution-core build that predates v2 refuses
/// a v2 decision, which is what rolling capability-core first would hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolClassGate {
    #[default]
    Off,
    Shadow,
    Enforce,
}

impl ToolClassGate {
    pub const ENV: &'static str = "TOOL_CLASS_GATE";

    /// Read the gate from the process environment, once, at construction.
    #[must_use]
    pub fn from_env() -> Self {
        let raw = std::env::var(Self::ENV).ok();
        let gate = Self::from_raw(raw.as_deref());
        if gate == Self::Off && raw.as_deref().is_some_and(|v| !v.is_empty() && v != "off") {
            tracing::warn!(
                "{} is not one of off, shadow or enforce; the tool class gate stays off",
                Self::ENV
            );
        }
        gate
    }

    fn from_raw(raw: Option<&str>) -> Self {
        match raw {
            Some("shadow") => Self::Shadow,
            Some("enforce") => Self::Enforce,
            _ => Self::Off,
        }
    }
}

#[tonic::async_trait]
pub trait CapabilityPolicy: Send + Sync {
    async fn evaluate(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
    ) -> Result<CapabilityDecision, Status>;

    async fn evaluate_with_evidence(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
    ) -> Result<CapabilityEvaluation, Status> {
        self.evaluate(tool_name, run_id, org_id)
            .await
            .map(CapabilityEvaluation::unverified)
    }

    /// `evaluate_with_evidence` for a call made on behalf of `user_id`, the
    /// person the run acts for. Policies that do not bind the user (every test
    /// double, and the gate while it is off) ignore it.
    async fn evaluate_tool_call(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
        _user_id: &str,
    ) -> Result<CapabilityEvaluation, Status> {
        self.evaluate_with_evidence(tool_name, run_id, org_id).await
    }

    /// Resolve the registry bindings of a run's dynamic (`mcp__`) tools with one
    /// lookup before its first call, so a call does not pay for it. Best effort:
    /// a name left unresolved is looked up again, once, when it is called, and is
    /// refused if the registry still does not hold it.
    async fn prefetch_tool_bindings(
        &self,
        _run_id: &str,
        _org_id: &str,
        _user_id: &str,
        _tool_names: &[String],
    ) -> Result<(), Status> {
        Ok(())
    }

    /// Returns only server-resolved, run-bound tool definitions. The default
    /// deliberately exposes no owner-plane action: test/local policies cannot
    /// accidentally invent one by omitting this method.
    async fn resolve_server_tool_definitions(
        &self,
        _run_id: &str,
        _org_id: &str,
    ) -> Result<Vec<mp_contracts::model_plane::v1::ToolDefinition>, Status> {
        Ok(Vec::new())
    }
}

#[derive(Clone)]
pub struct GrpcCapabilityPolicy {
    channel: Channel,
    tokens: TokenSource,
    decision_verifier: DecisionEvidenceVerifier,
    model_action_view: Option<ServerResolvedModelActionViewClient>,
    gate: ToolClassGate,
    bindings: Arc<RunBindings>,
}

impl std::fmt::Debug for GrpcCapabilityPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GrpcCapabilityPolicy")
            .field("bearer", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl GrpcCapabilityPolicy {
    /// Build the execution-owned capability policy client. The service
    /// credential is exchanged for an exact tenant/audience/scope token and is
    /// never sent to capability-core itself.
    ///
    /// # Errors
    ///
    /// Returns an error when the required deployment-owned service-principal
    /// configuration is missing or invalid.
    pub fn from_env(channel: Channel) -> anyhow::Result<Self> {
        Ok(Self {
            channel,
            tokens: TokenSource::Service(Arc::new(ServiceTokenProvider::from_env()?)),
            decision_verifier: DecisionEvidenceVerifier::from_env()?,
            model_action_view: ServerResolvedModelActionViewClient::from_env()?,
            gate: ToolClassGate::from_env(),
            bindings: Arc::default(),
        })
    }

    #[cfg(test)]
    #[must_use]
    pub fn with_static_token(channel: Channel, bearer: &str) -> Self {
        Self {
            channel,
            tokens: TokenSource::Static(Arc::from(bearer)),
            decision_verifier: DecisionEvidenceVerifier::disabled_for_test(),
            model_action_view: None,
            gate: ToolClassGate::Off,
            bindings: Arc::default(),
        }
    }
}

#[tonic::async_trait]
impl CapabilityPolicy for GrpcCapabilityPolicy {
    async fn evaluate(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
    ) -> Result<CapabilityDecision, Status> {
        let (binding, response) = self
            .evaluate_response(tool_name, run_id, org_id, "")
            .await?;
        self.refuse_dynamic_unless_enforced(&binding)?;
        map_capability_response(&response).map(|evaluation| evaluation.decision)
    }

    async fn evaluate_with_evidence(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
    ) -> Result<CapabilityEvaluation, Status> {
        self.evaluate_tool_call(tool_name, run_id, org_id, "").await
    }

    async fn evaluate_tool_call(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> Result<CapabilityEvaluation, Status> {
        let (binding, response) = self
            .evaluate_response(tool_name, run_id, org_id, user_id)
            .await?;
        let evaluation = map_capability_response(&response)?;
        if evaluation.decision == CapabilityDecision::Allow {
            let call = EvidenceCall {
                tool_name,
                run_id,
                org_id,
                user_id,
                capability_id: &binding.capability_id,
                registry: binding.registry.as_ref(),
                gate: self.gate,
            };
            if !self
                .decision_verifier
                .verify(&response.decision_evidence, &response, &call)
            {
                return Err(Status::permission_denied(
                    "capability policy returned invalid decision evidence",
                ));
            }
            self.pin_binding(&call, &response)?;
        }
        self.refuse_dynamic_unless_enforced(&binding)?;
        Ok(CapabilityEvaluation {
            evidence_verified: evaluation.decision != CapabilityDecision::Allow
                || self.decision_verifier.is_configured(),
            ..evaluation
        })
    }

    async fn prefetch_tool_bindings(
        &self,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        tool_names: &[String],
    ) -> Result<(), Status> {
        // An evicted run's dynamic tools are refused when called; resolving them
        // here would only churn the table that evicted it.
        if self.gate == ToolClassGate::Off || self.bindings.pin_lost(org_id, run_id) {
            return Ok(());
        }
        let mut seen = HashSet::new();
        let pending: Vec<String> = tool_names
            .iter()
            .filter(|name| is_mcp_tool_name(name))
            .filter(|name| {
                matches!(
                    self.bindings.lookup(org_id, run_id, name),
                    BindingLookup::Unknown
                )
            })
            .filter(|name| seen.insert(name.as_str()))
            .take(MAX_TOOLS_PER_RUN)
            .cloned()
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        self.resolve_and_record(run_id, org_id, user_id, &pending)
            .await
    }

    async fn resolve_server_tool_definitions(
        &self,
        run_id: &str,
        org_id: &str,
    ) -> Result<Vec<mp_contracts::model_plane::v1::ToolDefinition>, Status> {
        let Some(client) = &self.model_action_view else {
            return Ok(Vec::new());
        };
        let TokenSource::Service(tokens) = &self.tokens else {
            return Ok(Vec::new());
        };
        client.resolve(tokens, run_id, org_id).await
    }
}

#[derive(Debug, Clone)]
struct ServerResolvedModelActionViewClient {
    ticket_client: crate::ticket_tools::AgentTicketActionClient,
    capability_core_url: Url,
    http: Client,
}

#[derive(Debug, serde::Serialize)]
struct ModelActionViewRequest<'a> {
    run_id: &'a str,
    control_view_token: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelActionViewEnvelope {
    data: ModelActionViewData,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelActionViewData {
    run_id: String,
    actions: Vec<ModelActionViewTool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelActionViewTool {
    name: String,
    description: String,
    parameters_json: String,
    action_schema_hash: String,
    requires_approval: bool,
}

impl ServerResolvedModelActionViewClient {
    fn from_env() -> anyhow::Result<Option<Self>> {
        let capability_core_url = std::env::var("CAPABILITY_CORE_HTTP_URL")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_owned())
            .filter(|value| !value.is_empty());
        let ticket_client =
            crate::ticket_tools::AgentTicketActionClient::from_env().map_err(anyhow::Error::msg)?;
        // CAPABILITY_CORE_HTTP_URL also enables Execution Core's unrelated
        // sandbox-health reporter. Its presence alone must not make startup
        // depend on the optional owner-action configuration. Conversely, a fully
        // wired effect adapter without this endpoint simply remains unoffered.
        let (Some(capability_core_url), Some(ticket_client)) = (capability_core_url, ticket_client)
        else {
            return Ok(None);
        };
        if !ticket_client.model_action_view_is_configured() {
            return Ok(None);
        }
        let capability_core_url = Url::parse(&capability_core_url)
            .map_err(|_| anyhow::anyhow!("CAPABILITY_CORE_HTTP_URL is invalid"))?;
        if !matches!(capability_core_url.scheme(), "http" | "https")
            || capability_core_url.host_str().is_none()
            || !capability_core_url.username().is_empty()
            || capability_core_url.password().is_some()
            || capability_core_url.query().is_some()
            || capability_core_url.fragment().is_some()
        {
            anyhow::bail!("CAPABILITY_CORE_HTTP_URL must be an absolute http(s) service URL without credentials, query, or fragment")
        }
        Ok(Some(Self {
            ticket_client,
            capability_core_url,
            http: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()?,
        }))
    }

    async fn resolve(
        &self,
        tokens: &ServiceTokenProvider,
        run_id: &str,
        org_id: &str,
    ) -> Result<Vec<mp_contracts::model_plane::v1::ToolDefinition>, Status> {
        if run_id.trim().is_empty() || org_id.trim().is_empty() {
            return Err(Status::invalid_argument(
                "run and tenant are required for server-resolved model actions",
            ));
        }
        let control_view_token = self
            .ticket_client
            .request_model_action_view(run_id, org_id)
            .await
            .map_err(|_| Status::unavailable("Control model action view unavailable"))?;
        let bearer = tokens
            .token_with_scopes(
                org_id,
                &["capability:model-action:view"],
                "resolve a Control-authorized run-bound model action view",
            )
            .await
            .map_err(|_| Status::unavailable("model action view credential unavailable"))?;
        let endpoint = self
            .capability_core_url
            .join("/api/v1/internal/model-actions/run-view")
            .map_err(|_| Status::internal("model action view endpoint is invalid"))?;
        let response = self
            .http
            .post(endpoint)
            .bearer_auth(bearer)
            .json(&ModelActionViewRequest {
                run_id,
                control_view_token: &control_view_token,
            })
            .send()
            .await
            .map_err(|_| Status::unavailable("model action view unavailable"))?;
        if !response.status().is_success() {
            return Err(Status::permission_denied("model action view denied"));
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| Status::unavailable("model action view unavailable"))?;
        if body.len() > 32 << 10 {
            return Err(Status::permission_denied(
                "model action view response is oversized",
            ));
        }
        let response = serde_json::from_slice::<ModelActionViewEnvelope>(&body)
            .map_err(|_| Status::permission_denied("model action view response is invalid"))?;
        if response.data.run_id != run_id || response.data.actions.len() > 1 {
            return Err(Status::permission_denied(
                "model action view response is not run-bound",
            ));
        }
        response
            .data
            .actions
            .into_iter()
            .map(|tool| {
                if tool.name != crate::ticket_tools::TOOL_NAME
                    || tool.description.trim().is_empty()
                    || tool.parameters_json
                        != crate::ticket_tools::TICKET_CREATE_MODEL_PARAMETERS_JSON
                    || tool.action_schema_hash != crate::ticket_tools::TICKET_CREATE_SCHEMA_SHA256
                    || !tool.requires_approval
                {
                    return Err(Status::permission_denied(
                        "model action view returned an invalid tool definition",
                    ));
                }
                Ok(mp_contracts::model_plane::v1::ToolDefinition {
                    name: tool.name,
                    description: tool.description,
                    parameters_json: tool.parameters_json,
                })
            })
            .collect()
    }
}

/// What a tool name is bound to for one call: the capability whose policy
/// governs it, and, for a dynamic tool, the registry binding the signed
/// evidence must agree with.
#[derive(Debug, Clone)]
struct Binding {
    capability_id: String,
    registry: Option<RegistryBinding>,
}

impl GrpcCapabilityPolicy {
    /// Bind `tool_name` and ask capability-core for a decision on it. The binding
    /// comes back so the caller can verify the evidence against what was bound.
    async fn evaluate_response(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> Result<(Binding, EvaluatePolicyResponse), Status> {
        if run_id.trim().is_empty() || org_id.trim().is_empty() {
            return Err(Status::invalid_argument(
                "run and tenant are required for capability policy",
            ));
        }
        let binding = self.binding_for(tool_name, run_id, org_id, user_id).await?;
        let bearer = self.tokens.token(org_id).await.map_err(|error| {
            tracing::warn!(%error, "capability service credential unavailable");
            Status::unavailable("capability policy credential unavailable")
        })?;
        // The tool and the user are named only once the gate is on, so with it
        // off the request is byte-for-byte what it was before tool classes.
        let named = self.gate != ToolClassGate::Off;
        let mut request = Request::new(EvaluatePolicyRequest {
            capability_id: binding.capability_id.clone(),
            run_id: run_id.to_owned(),
            agent_id: "execution-core".to_owned(),
            org_id: org_id.to_owned(),
            scope: "global".to_owned(),
            user_id: if named {
                user_id.to_owned()
            } else {
                String::new()
            },
            tool_name: if named {
                tool_name.to_owned()
            } else {
                String::new()
            },
        });
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {bearer}").parse().map_err(|_| {
                Status::internal("verified capability credential is not forwardable")
            })?,
        );
        let nested = tokio::time::timeout(
            Duration::from_secs(3),
            CapabilityCoreClient::new(self.channel.clone()).evaluate_policy(request),
        )
        .await
        .map_err(|_| Status::unavailable("capability policy timed out"))?;
        let response = nested.map_err(|error| {
            tracing::warn!(code = ?error.code(), "capability policy unavailable");
            Status::unavailable("capability policy unavailable")
        })?;
        Ok((binding, response.into_inner()))
    }

    /// Shadow observes a registry-bound tool, it does not run it. The legacy
    /// answer for an `mcp__` name is a refusal (it has no governed binding), and
    /// nothing yet ties the call that is dispatched to the digest checked here (the
    /// gateway is not handed it), so only `enforce` lets a dynamic tool through.
    /// Every path that can answer allow for a name goes through this, so a shadow
    /// gate can never turn the legacy refusal into a live allow.
    #[allow(clippy::result_large_err)]
    fn refuse_dynamic_unless_enforced(&self, binding: &Binding) -> Result<(), Status> {
        if binding.registry.is_some() && self.gate != ToolClassGate::Enforce {
            return Err(Status::permission_denied(
                "tool has no governed capability binding",
            ));
        }
        Ok(())
    }

    /// The capability a tool name is governed by. A built-in tool resolves from
    /// the static server-owned map. An `mcp__` name resolves only from the
    /// registry, once the gate is on: from this run's table, else by one bounded
    /// lookup, else it has no binding and is refused.
    async fn binding_for(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> Result<Binding, Status> {
        if let Some(capability_id) = trusted_capability_id(tool_name) {
            return Ok(Binding {
                capability_id,
                registry: None,
            });
        }
        let unbound = || Status::permission_denied("tool has no governed capability binding");
        if self.gate == ToolClassGate::Off || !is_mcp_tool_name(tool_name) {
            return Err(unbound());
        }
        // A run whose table was evicted has lost the version and class it pinned
        // for its dynamic tools. Resolving the tool again would pin whatever the
        // registry holds now and lapse the mid-run change guard, so it is refused.
        if self.bindings.pin_lost(org_id, run_id) {
            return Err(pin_lost());
        }
        match self.bindings.lookup(org_id, run_id, tool_name) {
            BindingLookup::Bound(registry) => return Ok(Binding::from_registry(registry)),
            BindingLookup::Unbound => return Err(unbound()),
            BindingLookup::Unknown => {}
        }
        self.resolve_and_record(run_id, org_id, user_id, &[tool_name.to_owned()])
            .await?;
        match self.bindings.lookup(org_id, run_id, tool_name) {
            BindingLookup::Bound(registry) => Ok(Binding::from_registry(registry)),
            BindingLookup::Unbound | BindingLookup::Unknown => Err(unbound()),
        }
    }

    /// One `ResolveToolBindings` call for `names`, recorded in the run's table.
    /// A name the registry does not return, returns twice, or returns in a form
    /// that cannot be trusted is recorded as unbound for the rest of the run.
    async fn resolve_and_record(
        &self,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        names: &[String],
    ) -> Result<(), Status> {
        let bearer = self.tokens.token(org_id).await.map_err(|error| {
            tracing::warn!(%error, "capability service credential unavailable");
            Status::unavailable("capability policy credential unavailable")
        })?;
        let mut request = Request::new(ResolveToolBindingsRequest {
            org_id: org_id.to_owned(),
            run_id: run_id.to_owned(),
            user_id: user_id.to_owned(),
            tool_names: names.to_vec(),
        });
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {bearer}").parse().map_err(|_| {
                Status::internal("verified capability credential is not forwardable")
            })?,
        );
        let nested = tokio::time::timeout(
            Duration::from_secs(3),
            CapabilityCoreClient::new(self.channel.clone()).resolve_tool_bindings(request),
        )
        .await
        .map_err(|_| Status::unavailable("tool binding lookup timed out"))?;
        let wire = nested
            .map_err(|error| {
                tracing::warn!(code = ?error.code(), "tool binding lookup unavailable");
                Status::unavailable("tool binding lookup unavailable")
            })?
            .into_inner()
            .bindings;
        let mut resolved = Vec::new();
        let mut unbound = Vec::new();
        for name in names {
            let mut matches = wire.iter().filter(|binding| &binding.tool_name == name);
            let binding = match (matches.next(), matches.next()) {
                (Some(binding), None) => RegistryBinding::from_wire(name, binding),
                _ => None,
            };
            match binding {
                Some(binding) => resolved.push((name.clone(), binding)),
                None => unbound.push(name.clone()),
            }
        }
        self.bindings.record(org_id, run_id, resolved, unbound);
        Ok(())
    }

    /// Pin the capability version and class a tool was first allowed under in
    /// this run. A later allow under a different version or class means the
    /// registry changed the tool mid-run; that fails closed under `enforce`, and is
    /// logged and counted under `shadow`.
    #[allow(clippy::result_large_err)]
    fn pin_binding(
        &self,
        call: &EvidenceCall<'_>,
        response: &EvaluatePolicyResponse,
    ) -> Result<(), Status> {
        if self.gate == ToolClassGate::Off || response.side_effect_class.is_empty() {
            return Ok(());
        }
        // The run may have been evicted since the binding was resolved. A dynamic
        // tool has no pin to compare with then, so it fails closed instead of
        // pinning afresh; a built-in tool is not affected.
        if call.registry.is_some() && self.bindings.pin_lost(call.org_id, call.run_id) {
            return Err(pin_lost());
        }
        let class = SideEffectClass::from_wire(&response.side_effect_class);
        if self.bindings.pin(
            call.org_id,
            call.run_id,
            call.tool_name,
            &response.capability_version,
            class,
        ) {
            return Ok(());
        }
        if self.gate == ToolClassGate::Enforce {
            tracing::warn!(tool = %call.tool_name, "tool binding changed during the run");
            return Err(Status::permission_denied(
                "tool binding changed during the run",
            ));
        }
        record_divergence("binding_pin", call.tool_name);
        Ok(())
    }
}

impl Binding {
    fn from_registry(registry: RegistryBinding) -> Self {
        Self {
            capability_id: registry.capability_id.clone(),
            registry: Some(registry),
        }
    }
}

/// Every qualified MCP tool name starts with this; the registry row, never a
/// parse of the name, says what it is bound to.
const MCP_TOOL_PREFIX: &str = "mcp__";

/// Capability ids the registry derives for MCP tool rows. A binding the registry
/// returns for an `mcp__` name under any other id is refused, so a registry quirk
/// cannot bind a dynamic name to a built-in capability's authority.
const MCP_CAPABILITY_PREFIX: &str = "cap.mcp_tool.";

/// Most distinct tools one run tracks, and most runs one process tracks. Both
/// are memory bounds: a run offers a few dozen tools, and the oldest run is
/// forgotten first.
const MAX_TOOLS_PER_RUN: usize = 256;
const MAX_TRACKED_RUNS: usize = 1024;

/// Most evicted run keys remembered. A run evicted while it is still going lost
/// the pins its dynamic tools were first allowed under; remembering its key is
/// what lets those tools fail closed instead of silently pinning afresh. The set
/// is itself bounded (oldest forgotten first), so the guard is per-process and
/// best effort: a run idle through this many later evictions, or a process
/// restart, forgets it again.
const MAX_EVICTED_RUNS: usize = 16_384;

/// Refusal for a dynamic tool whose run lost its pins; the typed reason leads the
/// message so a log search or a client can key on it.
fn pin_lost() -> Status {
    Status::permission_denied(
        "binding_pin_lost: this run's tool binding pins were evicted from this process, \
         so a dynamic tool cannot be shown unchanged; start a new run",
    )
}

fn is_mcp_tool_name(tool_name: &str) -> bool {
    tool_name.len() > MCP_TOOL_PREFIX.len() && tool_name.starts_with(MCP_TOOL_PREFIX)
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn record_divergence(check: &'static str, tool_name: &str) {
    tracing::warn!(check, tool = %tool_name, "tool class gate divergence (shadow: legacy decides)");
    metrics::counter!("execution_core_tool_class_gate_divergence_total", "check" => check)
        .increment(1);
}

/// What the registry binds a dynamic tool to, as resolved for one run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistryBinding {
    capability_id: String,
    capability_version: String,
    class: SideEffectClass,
    floor: ApprovalFloor,
    digest: String,
}

impl RegistryBinding {
    /// Accept a wire binding only when it is exactly what an approved MCP tool
    /// looks like: the name asked for, an MCP capability id, an approved review
    /// state and a well-formed pinned digest. Anything else is not a binding.
    fn from_wire(requested: &str, wire: &WireToolBinding) -> Option<Self> {
        if wire.tool_name != requested
            || !wire.capability_id.starts_with(MCP_CAPABILITY_PREFIX)
            || wire.capability_version.is_empty()
            || wire.review_state != "approved"
            || !is_sha256_hex(&wire.tool_digest)
        {
            return None;
        }
        let class = SideEffectClass::from_wire(&wire.side_effect_class);
        Some(Self {
            capability_id: wire.capability_id.clone(),
            capability_version: wire.capability_version.clone(),
            class,
            floor: effective_floor(class, ApprovalFloor::from_wire(&wire.approval_floor)),
            digest: wire.tool_digest.clone(),
        })
    }
}

#[derive(Debug)]
enum BindingLookup {
    Bound(RegistryBinding),
    /// The registry was asked and did not bind it; not asked again this run.
    Unbound,
    Unknown,
}

/// Per-process, per-run record of what the registry bound each dynamic tool to
/// and of the version and class each tool was first allowed under.
///
/// The pin is per-process and best effort. It lives in memory, so a restart
/// forgets it, and it is held for at most `MAX_TRACKED_RUNS` runs: an idle run
/// can be evicted, and then its dynamic tools fail closed (`binding_pin_lost`)
/// rather than re-pinning against whatever the registry holds now. Remembering
/// the evicted keys is bounded too (`MAX_EVICTED_RUNS`). The authoritative
/// answer to "did this tool change" is always the registry and the signed digest;
/// the pin only catches a change between two calls of one run in one process.
#[derive(Default)]
struct RunBindings {
    state: Mutex<BindingState>,
}

#[derive(Default)]
struct BindingState {
    clock: u64,
    runs: HashMap<String, RunTable>,
    /// Keys of runs evicted from `runs`, oldest first, bounded by
    /// `MAX_EVICTED_RUNS`; `evicted_set` answers membership.
    evicted: VecDeque<String>,
    evicted_set: HashSet<String>,
}

#[derive(Default)]
struct RunTable {
    touched: u64,
    bindings: HashMap<String, RegistryBinding>,
    unbound: HashSet<String>,
    pins: HashMap<String, (String, SideEffectClass)>,
}

impl BindingState {
    /// Note that a run's table was dropped to make room, counted and logged so an
    /// operator can see long runs outliving the table. The dropped table is not
    /// recoverable; the key only makes its dynamic tools fail closed.
    fn remember_eviction(&mut self, key: String) {
        tracing::warn!(
            tracked_runs = MAX_TRACKED_RUNS,
            "a run's tool binding table was evicted; its dynamic tools now fail closed"
        );
        metrics::counter!("execution_core_tool_binding_run_evictions_total").increment(1);
        if !self.evicted_set.insert(key.clone()) {
            return;
        }
        self.evicted.push_back(key);
        while self.evicted.len() > MAX_EVICTED_RUNS {
            if let Some(forgotten) = self.evicted.pop_front() {
                self.evicted_set.remove(&forgotten);
            }
        }
    }

    fn existing(&mut self, key: &str) -> Option<&mut RunTable> {
        self.clock += 1;
        let clock = self.clock;
        let table = self.runs.get_mut(key)?;
        table.touched = clock;
        Some(table)
    }

    fn table(&mut self, key: &str) -> &mut RunTable {
        if !self.runs.contains_key(key) && self.runs.len() >= MAX_TRACKED_RUNS {
            let oldest = self
                .runs
                .iter()
                .min_by_key(|(_, table)| table.touched)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.runs.remove(&oldest);
                self.remember_eviction(oldest);
            }
        }
        self.clock += 1;
        let clock = self.clock;
        let table = self.runs.entry(key.to_owned()).or_default();
        table.touched = clock;
        table
    }
}

impl RunBindings {
    fn guard(&self) -> MutexGuard<'_, BindingState> {
        // The table holds plain data and every update is a single insert, so a
        // panic elsewhere cannot leave it half written.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn key(org_id: &str, run_id: &str) -> String {
        format!("{org_id}\u{1f}{run_id}")
    }

    /// Whether this run's table was evicted while the process was tracking it, so
    /// the version and class its dynamic tools were pinned under are gone. Stays
    /// true for the rest of the run: a table rebuilt afterwards (by a built-in
    /// tool's pin) does not restore what was lost.
    fn pin_lost(&self, org_id: &str, run_id: &str) -> bool {
        self.guard()
            .evicted_set
            .contains(&Self::key(org_id, run_id))
    }

    fn lookup(&self, org_id: &str, run_id: &str, tool_name: &str) -> BindingLookup {
        let mut state = self.guard();
        let Some(table) = state.existing(&Self::key(org_id, run_id)) else {
            return BindingLookup::Unknown;
        };
        if let Some(binding) = table.bindings.get(tool_name) {
            BindingLookup::Bound(binding.clone())
        } else if table.unbound.contains(tool_name) {
            BindingLookup::Unbound
        } else {
            BindingLookup::Unknown
        }
    }

    fn record(
        &self,
        org_id: &str,
        run_id: &str,
        resolved: Vec<(String, RegistryBinding)>,
        unbound: Vec<String>,
    ) {
        let mut state = self.guard();
        let table = state.table(&Self::key(org_id, run_id));
        for (name, binding) in resolved {
            if table.bindings.len() < MAX_TOOLS_PER_RUN || table.bindings.contains_key(&name) {
                table.unbound.remove(&name);
                table.bindings.insert(name, binding);
            }
        }
        for name in unbound {
            if table.unbound.len() < MAX_TOOLS_PER_RUN {
                table.unbound.insert(name);
            }
        }
    }

    /// Pin the first `(version, class)` a tool is allowed under. `false` when a
    /// later allow differs, or when the run already tracks as many tools as it may.
    fn pin(
        &self,
        org_id: &str,
        run_id: &str,
        tool_name: &str,
        version: &str,
        class: SideEffectClass,
    ) -> bool {
        let mut state = self.guard();
        let table = state.table(&Self::key(org_id, run_id));
        match table.pins.get(tool_name) {
            Some((pinned_version, pinned_class)) => {
                pinned_version == version && *pinned_class == class
            }
            None if table.pins.len() < MAX_TOOLS_PER_RUN => {
                table
                    .pins
                    .insert(tool_name.to_owned(), (version.to_owned(), class));
                true
            }
            None => false,
        }
    }
}

fn map_capability_response(
    response: &EvaluatePolicyResponse,
) -> Result<CapabilityEvaluation, Status> {
    let decision = match response.decision.as_str() {
        "allow" => CapabilityDecision::Allow,
        "ask" => CapabilityDecision::Ask,
        "deny" | "fallback" => CapabilityDecision::Deny,
        _ => {
            return Err(Status::permission_denied(
                "capability policy returned an invalid decision",
            ))
        }
    };
    Ok(CapabilityEvaluation {
        decision,
        decision_id: response.decision_id.clone(),
        capability_version: response.capability_version.clone(),
        evidence_verified: false,
        policy_source: response.policy_source.clone(),
        side_effect_class: response.side_effect_class.clone(),
        approval_floor: response.approval_floor.clone(),
    })
}

#[derive(Clone)]
struct DecisionEvidenceVerifier {
    key: Option<VerifyingKey>,
}

impl DecisionEvidenceVerifier {
    fn from_env() -> anyhow::Result<Self> {
        let raw = std::env::var("EXECUTION_CORE_CAPABILITY_DECISION_PUBLIC_KEY")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        Self::from_raw(raw.as_deref())
    }

    fn from_raw(raw: Option<&str>) -> anyhow::Result<Self> {
        // Development compose intentionally leaves decision evidence
        // unconfigured; the policy response remains usable but carries no
        // cryptographic evidence. The production overlay makes this variable
        // mandatory, so an explicitly supplied malformed key must still fail
        // closed rather than silently disabling verification.
        let Some(raw) = raw else {
            return Ok(Self { key: None });
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|_| anyhow::anyhow!("capability decision public key is not valid base64"))?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("capability decision public key must be 32 bytes"))?;
        Ok(Self {
            key: Some(
                VerifyingKey::from_bytes(&bytes)
                    .map_err(|_| anyhow::anyhow!("capability decision public key is invalid"))?,
            ),
        })
    }

    #[cfg(test)]
    fn disabled_for_test() -> Self {
        Self { key: None }
    }

    fn is_configured(&self) -> bool {
        self.key.is_some()
    }

    fn verify(
        &self,
        evidence: &str,
        response: &EvaluatePolicyResponse,
        call: &EvidenceCall<'_>,
    ) -> bool {
        let Some(key) = &self.key else {
            return false;
        };
        let parts: Vec<&str> = evidence.split('.').collect();
        if parts.len() != 3 {
            return false;
        }
        let decode = |part: &str| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(part)
                .ok()
        };
        let (Some(header), Some(payload), Some(signature)) =
            (decode(parts[0]), decode(parts[1]), decode(parts[2]))
        else {
            return false;
        };
        #[derive(Deserialize)]
        struct Header {
            alg: String,
            typ: String,
        }
        let Ok(header) = serde_json::from_slice::<Header>(&header) else {
            return false;
        };
        let Ok(claims) = serde_json::from_slice::<DecisionClaims>(&payload) else {
            return false;
        };
        if header.alg != "EdDSA"
            || header.typ != "model-plane.capability-decision+jws"
            || !matches!(claims.v.as_str(), "1" | "2")
            || claims.decision_id != response.decision_id
            || claims.capability_version != response.capability_version
            || claims.capability_id != call.capability_id
            || claims.org_id != call.org_id
            || claims.run_id != call.run_id
            || claims.agent_id != "execution-core"
            || claims.scope != "global"
            || claims.decision != response.decision
            || claims.reason != response.reason
            || claims.budget_context != response.budget_context
            || claims.iat > now_unix() + 5
            || claims.exp < now_unix()
            || claims.exp - claims.iat > 90
        {
            return false;
        }
        let Ok(signature) = Signature::from_slice(&signature) else {
            return false;
        };
        if key
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .is_err()
        {
            return false;
        }
        Self::version_accepts(&claims, response, call)
    }

    /// The part of verification the gate controls. `Off` is the legacy tuple and
    /// nothing else. Otherwise a v2 claim set must name this tool, this user and
    /// this class and floor, and a dynamic tool must also match the registry
    /// binding it was resolved to. A v1 claim set predates all of that.
    fn version_accepts(
        claims: &DecisionClaims,
        response: &EvaluatePolicyResponse,
        call: &EvidenceCall<'_>,
    ) -> bool {
        if call.gate == ToolClassGate::Off {
            return true;
        }
        let failed = if claims.v == "2" {
            v2_mismatch(claims, response, call)
        } else {
            Some("evidence_version")
        };
        let Some(check) = failed else {
            return true;
        };
        if call.gate == ToolClassGate::Enforce {
            tracing::warn!(check, tool = %call.tool_name, "capability decision evidence refused");
            return false;
        }
        record_divergence(check, call.tool_name);
        true
    }
}

/// What a signed decision claims. v1 predates the last six fields, so they
/// default to empty and a v1 claim set can never satisfy a v2 check.
#[derive(Deserialize)]
struct DecisionClaims {
    v: String,
    iat: i64,
    exp: i64,
    decision_id: String,
    capability_id: String,
    capability_version: String,
    org_id: String,
    run_id: String,
    agent_id: String,
    scope: String,
    decision: String,
    reason: String,
    budget_context: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    tool_name: String,
    #[serde(default)]
    side_effect_class: String,
    #[serde(default)]
    approval_floor: String,
    #[serde(default)]
    policy_source: String,
    #[serde(default)]
    tool_digest: String,
}

/// What a decision is verified against: the call as this process made it and the
/// binding it resolved, never anything the evidence itself says.
struct EvidenceCall<'a> {
    tool_name: &'a str,
    run_id: &'a str,
    org_id: &'a str,
    user_id: &'a str,
    capability_id: &'a str,
    registry: Option<&'a RegistryBinding>,
    gate: ToolClassGate,
}

/// The first v2 claim that does not match the call, named for the log and the
/// divergence counter, or `None` when every claim matches.
fn v2_mismatch(
    claims: &DecisionClaims,
    response: &EvaluatePolicyResponse,
    call: &EvidenceCall<'_>,
) -> Option<&'static str> {
    if claims.tool_name != call.tool_name {
        return Some("tool_name");
    }
    if claims.user_id != call.user_id {
        return Some("user_id");
    }
    if claims.side_effect_class != response.side_effect_class
        || claims.approval_floor != response.approval_floor
        || claims.policy_source != response.policy_source
        || claims.tool_digest != response.tool_digest
    {
        return Some("response_claims");
    }
    let class = SideEffectClass::from_wire(&claims.side_effect_class);
    let floor = ApprovalFloor::from_wire(&claims.approval_floor);
    // A class or floor outside the vocabulary, or a floor below what the class
    // implies, is not something capability-core would sign.
    if class.as_str() != claims.side_effect_class
        || floor.as_str() != claims.approval_floor
        || effective_floor(class, floor) != floor
    {
        return Some("class_vocabulary");
    }
    // A floor of `ask` or above is an approval, never a silent allow:
    // capability-core turns such a decision into an `ask`, so a signed `allow`
    // that still carries the floor was decided without it (a ceiling nobody
    // enforced, or a forgery) and must not be dispatched on that evidence.
    if claims.decision == "allow" && floor >= ApprovalFloor::Ask {
        return Some("allow_with_approval_floor");
    }
    match call.registry {
        Some(registry) => {
            if claims.capability_version != registry.capability_version {
                Some("capability_version")
            } else if class != registry.class {
                Some("class")
            } else if floor != registry.floor {
                Some("floor")
            } else if claims.tool_digest != registry.digest {
                Some("tool_digest")
            } else {
                None
            }
        }
        None if !claims.tool_digest.is_empty() => Some("tool_digest"),
        None => None,
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[derive(Clone)]
enum TokenSource {
    #[cfg(test)]
    Static(Arc<str>),
    Service(Arc<ServiceTokenProvider>),
}

impl TokenSource {
    async fn token(&self, org_id: &str) -> Result<String, anyhow::Error> {
        match self {
            #[cfg(test)]
            Self::Static(token) => Ok(token.to_string()),
            Self::Service(provider) => provider.token(org_id).await,
        }
    }
}

struct CachedToken {
    value: String,
    expires_at: Instant,
}

/// Mints Auth-Core-issued, audience-bound capability-core credentials from
/// execution-core's deployment service principal.
///
/// This is the ONLY credential path to capability-core. Anything else that needs
/// to call capability-core (health attestation, for example) requests a
/// differently-scoped token from here rather than inventing a second identity —
/// Auth Core owns the scope ceiling and the retention posture per audience, and
/// a second path would mean a second place for those to drift.
pub struct ServiceTokenProvider {
    auth_core_url: String,
    service_id: String,
    credential: String,
    http: reqwest::Client,
    /// Keyed by tenant AND scope set: a token is only valid for the scopes it
    /// was minted with, so caching by tenant alone would hand a `read` token to
    /// a caller that asked for `health:global:write`.
    cache: tokio::sync::Mutex<HashMap<String, CachedToken>>,
}

impl std::fmt::Debug for ServiceTokenProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceTokenProvider")
            .field("auth_core_url", &self.auth_core_url)
            .field("service_id", &self.service_id)
            .field("credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    token: String,
    expires_in_seconds: u64,
    audience: String,
}

impl ServiceTokenProvider {
    /// Build the provider from execution-core's deployment configuration
    /// (`AUTH_CORE_URL`, `EXECUTION_CORE_SERVICE_ID`,
    /// `EXECUTION_CORE_SERVICE_API_KEY`).
    ///
    /// # Errors
    /// Returns an error when required deployment configuration is missing or
    /// blank, or the HTTP client cannot be built.
    pub fn from_env() -> anyhow::Result<Self> {
        let required = |name: &'static str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("{name} is required"))
        };
        Ok(Self {
            auth_core_url: required("AUTH_CORE_URL")?.trim_end_matches('/').to_owned(),
            service_id: std::env::var("EXECUTION_CORE_SERVICE_ID")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "execution-core".to_owned()),
            credential: required("EXECUTION_CORE_SERVICE_API_KEY")?,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()?,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(auth_core_url: &str, service_id: &str, credential: &str) -> Self {
        Self {
            auth_core_url: auth_core_url.trim_end_matches('/').to_owned(),
            service_id: service_id.to_owned(),
            credential: credential.to_owned(),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()
                .expect("test HTTP client"),
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    async fn token(&self, org_id: &str) -> Result<String, anyhow::Error> {
        self.token_with_scopes(org_id, &["capability:read"], "execution dispatch policy")
            .await
    }

    /// Mint (or reuse) a capability-core token carrying exactly `scopes`.
    ///
    /// `reason` is audited by Auth Core, so it must describe the actual purpose
    /// rather than being copied from another caller.
    ///
    /// # Errors
    /// Fails when the tenant or scope set is empty, when Auth Core refuses the
    /// principal (a scope outside the deployment allowlist is refused outright,
    /// never silently narrowed), or when the issued credential is not a valid
    /// capability-core token.
    pub async fn token_with_scopes(
        &self,
        org_id: &str,
        scopes: &[&str],
        reason: &str,
    ) -> Result<String, anyhow::Error> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            anyhow::bail!("capability token tenant is required");
        }
        if scopes.is_empty() {
            anyhow::bail!("capability token scopes are required");
        }
        let cache_key = format!("{org_id}|{}", scopes.join(" "));
        let now = Instant::now();
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, token| token.expires_at > now + TOKEN_REFRESH_SKEW);
            if let Some(token) = cache.get(&cache_key) {
                return Ok(token.value.clone());
            }
        }

        let mut credential = reqwest::header::HeaderValue::from_str(&self.credential)
            .map_err(|_| anyhow::anyhow!("execution service credential is malformed"))?;
        credential.set_sensitive(true);
        let response = self
            .http
            .post(format!(
                "{}/api/capability-core/internal-token",
                self.auth_core_url
            ))
            .header("x-service-id", &self.service_id)
            .header("x-service-api-key", credential)
            .json(&serde_json::json!({
                "orgId": org_id,
                "scopes": scopes,
                "reason": reason,
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Auth Core refused a capability-core credential for scopes [{}] (status {})",
                scopes.join(", "),
                response.status()
            );
        }
        let bundle = response.json::<TokenResponse>().await?;
        if bundle.token.trim().is_empty()
            || bundle.audience != "capability-core"
            || !(1..=MAX_TOKEN_TTL_SECONDS).contains(&bundle.expires_in_seconds)
        {
            anyhow::bail!("Auth Core returned an invalid capability policy credential");
        }
        self.cache.lock().await.insert(
            cache_key,
            CachedToken {
                value: bundle.token.clone(),
                expires_at: Instant::now() + Duration::from_secs(bundle.expires_in_seconds),
            },
        );
        Ok(bundle.token)
    }
}

/// Map an executor name to its immutable policy capability. The mapping is
/// server-owned, so caller/model input can select a tool but cannot forge the
/// capability evaluated for that tool.
#[must_use]
pub fn trusted_capability_id(tool_name: &str) -> Option<String> {
    if tool_name.starts_with("subagent.") && tool_name.len() > "subagent.".len() {
        return Some("cap.agent.spawn".to_owned());
    }
    // `plan.*` (plan-gate §2.1) durably writes a plan through session-core —
    // the same mechanical shape, risk class, and ZDR gate as `save_memory`'s
    // `cap.memory.index` below: a low-risk, org/thread-scoped, reversible
    // durable write, refused outright under ZDR before it ever reaches this
    // gate (`memory_tool_preflight`, the exact mechanism `save_memory` uses).
    // Reusing that id — this function's own established convention, see
    // `reattach_skill`/`tickets.create` below — is deliberate: an unseeded
    // id (this arm previously returned bare `None`, on the mistaken belief
    // that `None` means "no capability check", matching `list_subagent_results`)
    // evaluates against no registry row and fails CLOSED in
    // `GrpcCapabilityPolicy::evaluate_response`, which requires
    // `trusted_capability_id` to return `Some` for every tool it ever
    // dispatches — `list_subagent_results` itself is bound to
    // `cap.agent.lineage.read` a few lines below, never `None`. The bare
    // `None` here meant every real `plan.propose` call failed with "tool has
    // no governed capability binding"; no test caught it because every test
    // in this codebase dispatches through a permissive `AllowCapabilityPolicy`
    // that never calls this function at all.
    if crate::plan::is_plan_tool(tool_name) {
        return Some("cap.memory.index".to_owned());
    }
    let capability = match tool_name {
        "shell" => "cap.command.shell",
        // Distinct from `cap.command.shell` on purpose: `code_interpreter` runs
        // in a hermetic per-call workspace (read-only rootfs, no network,
        // wall-clock timeout, scrubbed output, workspace deleted afterwards), so
        // capability-core governs it as a LOW-risk capability while arbitrary
        // host commands stay behind the high-risk shell capability.
        "code_interpreter" => "cap.command.sandbox",
        // The S4.2 background-process family, all five names on ONE capability
        // because they are one authority over one object: a caller that may
        // start a process may read what it printed, feed it stdin, stop it,
        // and list its own Space's processes. Five ids would make "may start
        // but may not stop" reachable by operator error.
        //
        // `cap.process.background` is seeded LOW by capability-core's
        // migration 0015 for the same reasons `cap.command.sandbox` above is:
        // same code-body-only sandbox, same read-only rootfs, same disabled
        // network. What it adds is bounded on both axes — a TTL that never
        // outlives the lease, and a count-limited number of them. The
        // capability is not the whole gate either: the lease must separately
        // carry `space:processes`, which Control grants only to an entitled
        // Space.
        "process_start" | "process_read" | "process_stdin" | "process_signal"
        | "process_list" => "cap.process.background",
        // The granular MCP facade never creates raw browser authority. It is
        // governed by the same browser capability as the agent loop, while
        // BrowserBroker separately binds its opaque run grant.
        "browser_agent" | "browser.observe" | "browser.act" => "cap.browser.open",
        "web_search" | "web_fetch" | "web.search" | "web.read" => "cap.tool.http",
        "knowledge_search" => "cap.retrieval.query",
        // Reads back a result the same run parked under a handle (WS25-S5).
        "result_query" => "cap.result.query",
        // The two context-memory tools. `cap.memory.index`/`cap.memory.search`
        // are the registry's own seeded ids (capability-core's
        // `registry.go`, both `RiskLow`, scope `workspace`) — not new names
        // minted here, which would evaluate against nothing and fail closed.
        //
        // Low risk, and deliberately so: a memory write is durable but
        // reversible, org- and thread-scoped, and refused outright under ZDR
        // before it reaches this gate. Classing it high would put an approval
        // prompt in front of every remembered fact, which is how the feature
        // stops being used at all. The narrower restriction it DOES carry is
        // `permission::is_restricted_context_write` — blocked in plan mode and
        // for delegated subagents — which is orthogonal to capability risk.
        "save_memory" => "cap.memory.index",
        "recall_memory" => "cap.memory.search",
        // Reading this run's own delegation records. `cap.agent.lineage.read`
        // covers both: the content-free listing and the approval-gated answer
        // read are the same authority over the same records, and the difference
        // between them is the human consent gate
        // (`permission::requires_consent_to_disclose`), not a second capability.
        // Low risk — it is a read of the tenant's own runs — and seeded in
        // capability-core's `registry.go` plus its migration, so it evaluates
        // against a real row rather than failing closed against nothing.
        "list_subagent_results" | "read_subagent_result" => "cap.agent.lineage.read",
        // Reading one of the org's own skill instructions back in full. Bound to
        // the SAME id as the summarize skill surface — it is the same authority
        // over the same operator-authored assets, and minting a new id here
        // would evaluate against nothing and fail closed.
        "reattach_skill" => "cap.skill.summarize",
        "yr_weather" | "traffic" | "news" | "company_lookup" => "cap.tool.information.read",
        "track_shipment" => "cap.tool.shipping.track",
        "get_shipping_quotes" | "shipping_carriers" => "cap.tool.shipping.read",
        "book_shipment" => "cap.tool.shipping.book",
        "list_social_accounts" => "cap.tool.social.read",
        "publish_social_post" => "cap.tool.social.publish",
        "list_provider_actions" | "read_provider_action" => "cap.tool.provider.read",
        "execute_provider_action" => "cap.tool.provider.execute",
        // The Space's read-only workspace tools (WS24-S3) share one capability: reading a file and
        // listing the tree are one authority over one object, as the process family is.
        "workspace_read" | "workspace_grep" | "workspace_glob" | "workspace_list" => {
            "cap.workspace.read"
        }
        // The two Space workspace writes (WS24-S4) are their own capability, so an org can switch
        // them off and keep the reads. They change only this run's overlay.
        "workspace_edit" | "workspace_put" => "cap.workspace.write",
        // A Conversation Core ticket is a model-proposed effect, never a
        // generic service call. The executor's dedicated adapter obtains a
        // current Control target-action decision and the owner verifies it.
        "tickets.create" => crate::ticket_tools::CAPABILITY_ID,
        // Dynamic MCP tools require a durable registry-owned binding. Do not
        // synthesize one from an untrusted model-facing name.
        _ => return None,
    };
    Some(capability.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sign_json(key: &SigningKey, claims: &serde_json::Value) -> String {
        let header = serde_json::json!({
            "alg": "EdDSA",
            "kid": "capability-decision-v1",
            "typ": "model-plane.capability-decision+jws"
        });
        let encoder = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let encoded_header = encoder.encode(serde_json::to_vec(&header).expect("header JSON"));
        let encoded_claims = encoder.encode(serde_json::to_vec(claims).expect("claims JSON"));
        let signing_input = format!("{encoded_header}.{encoded_claims}");
        let signature = key.sign(signing_input.as_bytes());
        format!("{signing_input}.{}", encoder.encode(signature.to_bytes()))
    }

    fn signed_evidence(
        key: &SigningKey,
        response: &EvaluatePolicyResponse,
        iat: i64,
        exp: i64,
    ) -> String {
        sign_json(
            key,
            &serde_json::json!({
                "v": "1",
                "iat": iat,
                "exp": exp,
                "decision_id": response.decision_id,
                "capability_id": "cap.command.shell",
                "capability_version": response.capability_version,
                "org_id": "org-a",
                "run_id": "run-a",
                "agent_id": "execution-core",
                "scope": "global",
                "decision": response.decision,
                "reason": response.reason,
                "budget_context": response.budget_context
            }),
        )
    }

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// What a v2 decision claims about the tool it was made for.
    struct V2 {
        capability_id: &'static str,
        tool_name: &'static str,
        user_id: &'static str,
    }

    /// A response carrying the v2 fields, and v2 evidence over exactly those
    /// fields for `claimed`. The two are built together so a test can change one
    /// side and see the verifier notice.
    fn v2_pair(
        key: &SigningKey,
        claimed: &V2,
        class: &str,
        floor: &str,
        digest: &str,
    ) -> (EvaluatePolicyResponse, String) {
        let response = EvaluatePolicyResponse {
            side_effect_class: class.to_owned(),
            approval_floor: floor.to_owned(),
            policy_source: "risk_level".to_owned(),
            tool_digest: digest.to_owned(),
            ..allow_response()
        };
        let evidence = v2_evidence(key, &response, claimed);
        (response, evidence)
    }

    fn v2_evidence(key: &SigningKey, response: &EvaluatePolicyResponse, claimed: &V2) -> String {
        let now = now_unix();
        sign_json(
            key,
            &serde_json::json!({
                "v": "2",
                "iat": now - 1,
                "exp": now + 30,
                "decision_id": response.decision_id,
                "capability_id": claimed.capability_id,
                "capability_version": response.capability_version,
                "org_id": "org-a",
                "run_id": "run-a",
                "agent_id": "execution-core",
                "scope": "global",
                "decision": response.decision,
                "reason": response.reason,
                "budget_context": response.budget_context,
                "user_id": claimed.user_id,
                "tool_name": claimed.tool_name,
                "side_effect_class": response.side_effect_class,
                "approval_floor": response.approval_floor,
                "policy_source": response.policy_source,
                "tool_digest": response.tool_digest,
            }),
        )
    }

    /// The call a test verifies evidence against.
    struct Fixture {
        capability_id: String,
        registry: Option<RegistryBinding>,
    }

    impl Fixture {
        fn builtin(tool: &str) -> Self {
            Self {
                capability_id: trusted_capability_id(tool).expect("a built-in tool"),
                registry: None,
            }
        }

        fn registry(class: SideEffectClass, floor: ApprovalFloor) -> Self {
            let binding = RegistryBinding {
                capability_id: "cap.mcp_tool.abc".to_owned(),
                capability_version: "cap-v3".to_owned(),
                class,
                floor,
                digest: DIGEST.to_owned(),
            };
            Self {
                capability_id: binding.capability_id.clone(),
                registry: Some(binding),
            }
        }

        fn call<'a>(&'a self, tool: &'a str, gate: ToolClassGate) -> EvidenceCall<'a> {
            EvidenceCall {
                tool_name: tool,
                run_id: "run-a",
                org_id: "org-a",
                user_id: "user-a",
                capability_id: &self.capability_id,
                registry: self.registry.as_ref(),
                gate,
            }
        }
    }

    fn allow_response() -> EvaluatePolicyResponse {
        EvaluatePolicyResponse {
            decision: "allow".to_owned(),
            reason: "policy-approved".to_owned(),
            budget_context: "budget-1".to_owned(),
            decision_id: "decision-1".to_owned(),
            capability_version: "cap-v3".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn an_evaluation_carries_the_class_and_floor_the_decision_named() {
        let mut response = allow_response();
        response.side_effect_class = "external_write".to_owned();
        response.approval_floor = "ask".to_owned();
        let evaluation = map_capability_response(&response).expect("a decision");
        assert_eq!(evaluation.side_effect_class, "external_write");
        assert_eq!(evaluation.approval_floor, "ask");
        // A policy that does not say leaves both empty, which a governed MCP call treats
        // as unable to confirm its offered class.
        let silent = map_capability_response(&allow_response()).expect("a decision");
        assert!(silent.side_effect_class.is_empty() && silent.approval_floor.is_empty());
    }

    #[test]
    fn absent_capability_decision_key_is_allowed_only_for_dev_mode() {
        let verifier = DecisionEvidenceVerifier::from_raw(None).expect("missing key is dev mode");
        assert!(!verifier.is_configured());
        assert!(DecisionEvidenceVerifier::from_raw(Some("not-base64")).is_err());
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let valid = base64::engine::general_purpose::STANDARD
            .encode(signing_key.verifying_key().to_bytes());
        assert!(DecisionEvidenceVerifier::from_raw(Some(&valid)).is_ok());
    }

    #[test]
    fn decision_evidence_verifies_and_binds_the_dispatch_tuple() {
        let signing_key = SigningKey::from_bytes(&[7; 32]);
        let verifier = DecisionEvidenceVerifier {
            key: Some(signing_key.verifying_key()),
        };
        let response = allow_response();
        let now = now_unix();
        let evidence = signed_evidence(&signing_key, &response, now - 1, now + 30);

        let shell = Fixture::builtin("shell");
        let call = shell.call("shell", ToolClassGate::Off);
        assert!(verifier.verify(&evidence, &response, &call));
        let other_run = EvidenceCall {
            run_id: "run-other",
            ..shell.call("shell", ToolClassGate::Off)
        };
        assert!(!verifier.verify(&evidence, &response, &other_run));
        let other_org = EvidenceCall {
            org_id: "org-other",
            ..shell.call("shell", ToolClassGate::Off)
        };
        assert!(!verifier.verify(&evidence, &response, &other_org));
    }

    #[test]
    fn decision_evidence_rejects_tampering_and_expiry() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let verifier = DecisionEvidenceVerifier {
            key: Some(signing_key.verifying_key()),
        };
        let response = allow_response();
        let now = now_unix();
        let valid = signed_evidence(&signing_key, &response, now - 1, now + 30);
        let mut tampered = valid.clone();
        let last = tampered.pop().expect("signature is non-empty");
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        let shell = Fixture::builtin("shell");
        let call = shell.call("shell", ToolClassGate::Off);
        assert!(verifier.verify(&valid, &response, &call));
        assert!(!verifier.verify(&tampered, &response, &call));

        let expired = signed_evidence(&signing_key, &response, now - 120, now - 60);
        assert!(!verifier.verify(&expired, &response, &call));
    }

    #[test]
    fn bindings_are_exact_and_unknown_or_dynamic_tools_fail_closed() {
        assert_eq!(
            trusted_capability_id("knowledge_search").as_deref(),
            Some("cap.retrieval.query")
        );
        assert_eq!(
            trusted_capability_id("book_shipment").as_deref(),
            Some("cap.tool.shipping.book")
        );
        assert_eq!(
            trusted_capability_id("tickets.create").as_deref(),
            Some(crate::ticket_tools::CAPABILITY_ID)
        );
        // The two execution tools must never collapse onto one capability: the
        // sandboxed interpreter is governed as low-risk, arbitrary shell is not.
        assert_eq!(
            trusted_capability_id("code_interpreter").as_deref(),
            Some("cap.command.sandbox")
        );
        assert_eq!(
            trusted_capability_id("shell").as_deref(),
            Some("cap.command.shell")
        );
        assert!(trusted_capability_id("knowledge_search ").is_none());
        assert!(trusted_capability_id("echo").is_none());
        assert!(trusted_capability_id("mcp__server__tool").is_none());
        assert_eq!(
            trusted_capability_id("subagent.research").as_deref(),
            Some("cap.agent.spawn")
        );
        // plan-gate: `plan.propose` must resolve to a REAL, already-seeded
        // capability id, never `None` — `GrpcCapabilityPolicy::evaluate_response`
        // treats `None` as "tool has no governed capability binding" and
        // refuses the call outright, so a bare `None` here would make every
        // `plan.propose` call fail in any environment that uses the real
        // (non-test) capability policy.
        assert_eq!(
            trusted_capability_id("plan.propose").as_deref(),
            Some("cap.memory.index")
        );
        assert!(trusted_capability_id("plan.propose").is_some());
        // The bare prefix is not itself a callable tool (mirrors
        // `is_plan_tool`'s own "prefix alone is not a tool" rule).
        assert!(trusted_capability_id("plan.").is_none());
    }

    #[tokio::test]
    async fn service_token_is_exactly_scoped_and_cached_without_disclosing_credential() {
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/capability-core/internal-token"))
            .and(header("x-service-id", "execution-core"))
            .and(header("x-service-api-key", "service-secret"))
            .and(body_json(serde_json::json!({
                "orgId": "org-a",
                "scopes": ["capability:read"],
                "reason": "execution dispatch policy"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "capability-token",
                "expiresInSeconds": 300,
                "audience": "capability-core"
            })))
            .expect(1)
            .mount(&auth)
            .await;
        let provider =
            ServiceTokenProvider::new_for_test(&auth.uri(), "execution-core", "service-secret");

        assert_eq!(provider.token("org-a").await.unwrap(), "capability-token");
        assert_eq!(provider.token("org-a").await.unwrap(), "capability-token");
        assert!(!format!("{provider:?}").contains("service-secret"));
    }

    #[tokio::test]
    async fn service_token_rejects_wrong_audience_and_redirects() {
        let target = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "wrong-audience-token",
                "expiresInSeconds": 300,
                "audience": "model-gateway"
            })))
            .mount(&target)
            .await;
        let redirect = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/stolen", target.uri())),
            )
            .mount(&redirect)
            .await;
        let provider =
            ServiceTokenProvider::new_for_test(&redirect.uri(), "execution-core", "secret");

        let error = provider
            .token("org-a")
            .await
            .expect_err("redirected service credentials must fail closed");
        assert!(error.to_string().contains("refused"));

        let direct = ServiceTokenProvider::new_for_test(&target.uri(), "execution-core", "secret");
        let error = direct
            .token("org-a")
            .await
            .expect_err("wrong token audience must fail closed");
        assert!(error
            .to_string()
            .contains("invalid capability policy credential"));
    }

    // ---------------------------------------------------------------------
    // Signed decision v2 (WS05-S3)
    // ---------------------------------------------------------------------

    fn verifier_for(key: &SigningKey) -> DecisionEvidenceVerifier {
        DecisionEvidenceVerifier {
            key: Some(key.verifying_key()),
        }
    }

    const ALL_GATES: [ToolClassGate; 3] = [
        ToolClassGate::Off,
        ToolClassGate::Shadow,
        ToolClassGate::Enforce,
    ];

    #[test]
    fn the_gate_is_parsed_strictly_and_defaults_off() {
        assert_eq!(ToolClassGate::from_raw(None), ToolClassGate::Off);
        assert_eq!(ToolClassGate::from_raw(Some("off")), ToolClassGate::Off);
        assert_eq!(
            ToolClassGate::from_raw(Some("shadow")),
            ToolClassGate::Shadow
        );
        assert_eq!(
            ToolClassGate::from_raw(Some("enforce")),
            ToolClassGate::Enforce
        );
        // Only the exact literals count: a typo, a different case, padding or an
        // empty value leaves the gate off.
        for raw in [
            "", "Shadow", "ENFORCE", " shadow", "enforce ", "true", "on", "1",
        ] {
            assert_eq!(
                ToolClassGate::from_raw(Some(raw)),
                ToolClassGate::Off,
                "{raw:?}"
            );
        }
        assert_eq!(ToolClassGate::default(), ToolClassGate::Off);
    }

    #[test]
    fn v2_evidence_verifies_for_the_tool_it_names_under_every_gate() {
        let key = SigningKey::from_bytes(&[11; 32]);
        let verifier = verifier_for(&key);
        let web = Fixture::builtin("web_fetch");
        let (response, evidence) = v2_pair(
            &key,
            &V2 {
                capability_id: "cap.tool.http",
                tool_name: "web_fetch",
                user_id: "user-a",
            },
            "read",
            "none",
            "",
        );
        for gate in ALL_GATES {
            assert!(
                verifier.verify(&evidence, &response, &web.call("web_fetch", gate)),
                "{gate:?}"
            );
        }
    }

    #[test]
    fn a_read_decision_replayed_against_another_tool_of_the_same_capability_is_refused() {
        let key = SigningKey::from_bytes(&[12; 32]);
        let verifier = verifier_for(&key);
        // browser.observe and browser.act share one capability; only the signed
        // tool name tells a read from a write.
        let browser = Fixture::builtin("browser.act");
        assert_eq!(browser.capability_id, "cap.browser.open");
        let (response, observe_evidence) = v2_pair(
            &key,
            &V2 {
                capability_id: "cap.browser.open",
                tool_name: "browser.observe",
                user_id: "user-a",
            },
            "read",
            "none",
            "",
        );

        assert!(
            !verifier.verify(
                &observe_evidence,
                &response,
                &browser.call("browser.act", ToolClassGate::Enforce)
            ),
            "enforce must refuse evidence minted for another tool"
        );
        // Shadow lets the legacy tuple decide for a built-in tool, and counts it.
        assert!(verifier.verify(
            &observe_evidence,
            &response,
            &browser.call("browser.act", ToolClassGate::Shadow)
        ));
        // The same evidence is, of course, good for the tool it names.
        assert!(verifier.verify(
            &observe_evidence,
            &response,
            &browser.call("browser.observe", ToolClassGate::Enforce)
        ));
    }

    #[test]
    fn v1_evidence_is_refused_only_under_enforce() {
        let key = SigningKey::from_bytes(&[13; 32]);
        let verifier = verifier_for(&key);
        let response = allow_response();
        let now = now_unix();
        let v1 = signed_evidence(&key, &response, now - 1, now + 30);
        let shell = Fixture::builtin("shell");

        assert!(verifier.verify(&v1, &response, &shell.call("shell", ToolClassGate::Off)));
        assert!(verifier.verify(&v1, &response, &shell.call("shell", ToolClassGate::Shadow)));
        assert!(
            !verifier.verify(&v1, &response, &shell.call("shell", ToolClassGate::Enforce)),
            "enforce refuses v1 evidence"
        );

        // A dynamic tool is no different: v1 evidence says nothing about it, which
        // enforce refuses and shadow only counts (the call is refused there anyway,
        // see `a_registered_mcp_tool_is_only_dispatchable_under_enforce`).
        let registry = Fixture::registry(SideEffectClass::Read, ApprovalFloor::None);
        let mcp_v1 = sign_json(
            &key,
            &serde_json::json!({
                "v": "1",
                "iat": now - 1,
                "exp": now + 30,
                "decision_id": response.decision_id,
                "capability_id": "cap.mcp_tool.abc",
                "capability_version": response.capability_version,
                "org_id": "org-a",
                "run_id": "run-a",
                "agent_id": "execution-core",
                "scope": "global",
                "decision": response.decision,
                "reason": response.reason,
                "budget_context": response.budget_context
            }),
        );
        assert!(
            verifier.verify(
                &mcp_v1,
                &response,
                &registry.call("mcp__srv__echo", ToolClassGate::Off)
            ),
            "the same evidence is well formed, so only the version policy refuses it"
        );
        assert!(verifier.verify(
            &mcp_v1,
            &response,
            &registry.call("mcp__srv__echo", ToolClassGate::Shadow)
        ));
        assert!(!verifier.verify(
            &mcp_v1,
            &response,
            &registry.call("mcp__srv__echo", ToolClassGate::Enforce)
        ));
    }

    /// A floor of `ask` or above is an approval, so capability-core never signs an
    /// `allow` that carries one: it signs an `ask`. An allow that does carry one
    /// (a ceiling nobody enforced when the registry held no row, or a forgery) is
    /// not dispatchable on that evidence under `enforce`, and is counted under
    /// `shadow`, where the legacy gate still decides.
    #[test]
    fn an_allow_that_carries_an_approval_floor_is_refused_under_enforce_and_counted_under_shadow() {
        let key = SigningKey::from_bytes(&[14; 32]);
        let verifier = verifier_for(&key);
        let ticket = Fixture::builtin("tickets.create");
        let lineage = Fixture::builtin("read_subagent_result");
        let web = Fixture::builtin("web_fetch");
        let cases: [(&Fixture, &'static str, &'static str, &str, &str); 4] = [
            (
                &ticket,
                crate::ticket_tools::CAPABILITY_ID,
                "tickets.create",
                "owner_effect",
                "durable_owner",
            ),
            (
                &lineage,
                "cap.agent.lineage.read",
                "read_subagent_result",
                "read",
                "ask",
            ),
            (
                &lineage,
                "cap.agent.lineage.read",
                "read_subagent_result",
                "unclassified",
                "ask",
            ),
            (&web, "cap.tool.http", "web_fetch", "read", "none"),
        ];
        for (fixture, capability_id, tool, class, floor) in cases {
            let (response, evidence) = v2_pair(
                &key,
                &V2 {
                    capability_id,
                    tool_name: tool,
                    user_id: "user-a",
                },
                class,
                floor,
                "",
            );
            let carries_floor = floor != "none";
            assert_eq!(
                v2_mismatch(
                    &serde_json::from_slice::<DecisionClaims>(
                        &base64::engine::general_purpose::URL_SAFE_NO_PAD
                            .decode(evidence.split('.').nth(1).expect("payload"))
                            .expect("payload base64")
                    )
                    .expect("claims"),
                    &response,
                    &fixture.call(tool, ToolClassGate::Enforce)
                ),
                carries_floor.then_some("allow_with_approval_floor"),
                "{tool} {class}/{floor}"
            );
            // The gate decides what the mismatch means: refused only under
            // enforce, accepted (and counted) under shadow, never looked at off.
            assert_eq!(
                verifier.verify(
                    &evidence,
                    &response,
                    &fixture.call(tool, ToolClassGate::Enforce)
                ),
                !carries_floor,
                "enforce {tool} {class}/{floor}"
            );
            for gate in [ToolClassGate::Off, ToolClassGate::Shadow] {
                assert!(
                    verifier.verify(&evidence, &response, &fixture.call(tool, gate)),
                    "{gate:?} {tool} {class}/{floor}"
                );
            }
        }
    }

    #[test]
    fn v2_claims_must_agree_with_the_response_the_user_and_the_vocabulary() {
        let key = SigningKey::from_bytes(&[14; 32]);
        let verifier = verifier_for(&key);
        let web = Fixture::builtin("web_fetch");
        let claimed = V2 {
            capability_id: "cap.tool.http",
            tool_name: "web_fetch",
            user_id: "user-a",
        };
        let (response, evidence) = v2_pair(&key, &claimed, "read", "none", "");
        let enforce = || web.call("web_fetch", ToolClassGate::Enforce);
        assert!(verifier.verify(&evidence, &response, &enforce()));

        // A response that disagrees with what was signed.
        for tampered in [
            EvaluatePolicyResponse {
                side_effect_class: "external_write".to_owned(),
                ..response.clone()
            },
            EvaluatePolicyResponse {
                approval_floor: "ask".to_owned(),
                ..response.clone()
            },
            EvaluatePolicyResponse {
                policy_source: "tool_review".to_owned(),
                ..response.clone()
            },
            EvaluatePolicyResponse {
                tool_digest: DIGEST.to_owned(),
                ..response.clone()
            },
        ] {
            assert!(!verifier.verify(&evidence, &tampered, &enforce()));
        }

        // Another person's decision.
        let other_user = EvidenceCall {
            user_id: "user-b",
            ..enforce()
        };
        assert!(!verifier.verify(&evidence, &response, &other_user));

        // A built-in tool carries no pinned digest.
        let (digest_response, digest_evidence) = v2_pair(&key, &claimed, "read", "none", DIGEST);
        assert!(!verifier.verify(&digest_evidence, &digest_response, &enforce()));

        // A class outside the vocabulary, or a floor below what the class implies,
        // is not something capability-core signs.
        for (class, floor) in [
            ("read-ish", "none"),
            ("read", "sometimes"),
            ("owner_effect", "none"),
            ("owner_effect", "ask"),
            ("unclassified", "none"),
            ("", ""),
        ] {
            let (response, evidence) = v2_pair(&key, &claimed, class, floor, "");
            assert!(
                !verifier.verify(&evidence, &response, &enforce()),
                "{class:?}/{floor:?}"
            );
        }
        // Well formed, and still not an allow: an owner effect is an approval, so a
        // signed `allow` that carries its durable_owner floor is refused (see
        // `an_allow_that_carries_an_approval_floor_is_refused_under_enforce...`).
        let (response, evidence) = v2_pair(&key, &claimed, "owner_effect", "durable_owner", "");
        assert!(!verifier.verify(&evidence, &response, &enforce()));
        let (response, evidence) = v2_pair(&key, &claimed, "context_write", "none", "");
        assert!(verifier.verify(&evidence, &response, &enforce()));
    }

    #[test]
    fn a_dynamic_tool_must_match_its_registry_binding_under_enforce_and_is_counted_under_shadow() {
        let key = SigningKey::from_bytes(&[15; 32]);
        let verifier = verifier_for(&key);
        let registry = Fixture::registry(SideEffectClass::Read, ApprovalFloor::None);
        let claimed = V2 {
            capability_id: "cap.mcp_tool.abc",
            tool_name: "mcp__srv__echo",
            user_id: "user-a",
        };
        let (response, evidence) = v2_pair(&key, &claimed, "read", "none", DIGEST);
        for gate in [ToolClassGate::Shadow, ToolClassGate::Enforce] {
            assert!(
                verifier.verify(&evidence, &response, &registry.call("mcp__srv__echo", gate)),
                "{gate:?}"
            );
        }

        // A digest other than the pinned one: the definition changed after the
        // binding was resolved. Refused under enforce; shadow only counts it, and
        // refuses the call as a whole.
        let other_digest = "f".repeat(64);
        let (drifted, drifted_evidence) = v2_pair(&key, &claimed, "read", "none", &other_digest);
        // A class the registry did not bind.
        let (looser, looser_evidence) = v2_pair(&key, &claimed, "sandbox_compute", "none", DIGEST);
        // A floor the registry did not bind.
        let (stricter, stricter_evidence) = v2_pair(&key, &claimed, "read", "ask", DIGEST);
        // A capability version other than the one resolved.
        let moved = EvaluatePolicyResponse {
            capability_version: "cap-v4".to_owned(),
            ..response.clone()
        };
        let moved_evidence = v2_evidence(&key, &moved, &claimed);
        // Another capability's authority.
        let (borrowed, borrowed_evidence) = v2_pair(
            &key,
            &V2 {
                capability_id: "cap.command.shell",
                ..claimed
            },
            "read",
            "none",
            DIGEST,
        );
        for (name, response, evidence) in [
            ("digest", &drifted, &drifted_evidence),
            ("class", &looser, &looser_evidence),
            ("floor", &stricter, &stricter_evidence),
            ("version", &moved, &moved_evidence),
        ] {
            assert!(
                !verifier.verify(
                    evidence,
                    response,
                    &registry.call("mcp__srv__echo", ToolClassGate::Enforce)
                ),
                "{name} under enforce"
            );
            assert!(
                verifier.verify(
                    evidence,
                    response,
                    &registry.call("mcp__srv__echo", ToolClassGate::Shadow)
                ),
                "{name} is only counted under shadow"
            );
        }
        // The capability id is part of the signed tuple every gate checks.
        for gate in ALL_GATES {
            assert!(
                !verifier.verify(
                    &borrowed_evidence,
                    &borrowed,
                    &registry.call("mcp__srv__echo", gate)
                ),
                "another capability's authority under {gate:?}"
            );
        }
    }

    #[test]
    fn v2_evidence_rejects_tampering_and_expiry() {
        let key = SigningKey::from_bytes(&[16; 32]);
        let verifier = verifier_for(&key);
        let web = Fixture::builtin("web_fetch");
        let claimed = V2 {
            capability_id: "cap.tool.http",
            tool_name: "web_fetch",
            user_id: "user-a",
        };
        let (response, valid) = v2_pair(&key, &claimed, "read", "none", "");
        let call = web.call("web_fetch", ToolClassGate::Enforce);
        assert!(verifier.verify(&valid, &response, &call));

        let mut tampered = valid.clone();
        let last = tampered.pop().expect("signature is non-empty");
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        assert!(!verifier.verify(&tampered, &response, &call));

        // Editing a claim without re-signing breaks the signature.
        let parts: Vec<&str> = valid.split('.').collect();
        let encoder = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let mut claims: serde_json::Value =
            serde_json::from_slice(&encoder.decode(parts[1]).expect("payload")).expect("claims");
        claims["tool_name"] = serde_json::json!("browser.act");
        let edited = format!(
            "{}.{}.{}",
            parts[0],
            encoder.encode(serde_json::to_vec(&claims).expect("claims JSON")),
            parts[2]
        );
        let browser = Fixture::builtin("browser.act");
        assert!(!verifier.verify(
            &edited,
            &response,
            &browser.call("browser.act", ToolClassGate::Off)
        ));

        let now = now_unix();
        claims["iat"] = serde_json::json!(now - 120);
        claims["exp"] = serde_json::json!(now - 60);
        claims["tool_name"] = serde_json::json!("web_fetch");
        let expired = sign_json(&key, &claims);
        assert!(!verifier.verify(&expired, &response, &call));
    }

    #[test]
    fn a_registry_binding_is_only_what_an_approved_mcp_tool_looks_like() {
        let wire = |edit: &dyn Fn(&mut WireToolBinding)| {
            let mut binding = WireToolBinding {
                tool_name: "mcp__srv__echo".to_owned(),
                capability_id: "cap.mcp_tool.abc".to_owned(),
                capability_version: "1.0.0-0123456789ab".to_owned(),
                side_effect_class: "read".to_owned(),
                approval_floor: "none".to_owned(),
                tool_digest: DIGEST.to_owned(),
                review_state: "approved".to_owned(),
                effective_policy: "allow".to_owned(),
            };
            edit(&mut binding);
            RegistryBinding::from_wire("mcp__srv__echo", &binding)
        };

        let good = wire(&|_| {}).expect("an approved MCP tool is a binding");
        assert_eq!(good.class, SideEffectClass::Read);
        assert_eq!(good.floor, ApprovalFloor::None);
        assert_eq!(good.digest, DIGEST);

        assert!(wire(&|b| b.tool_name = "mcp__srv__other".to_owned()).is_none());
        // A dynamic name never borrows a built-in capability's authority.
        assert!(wire(&|b| b.capability_id = "cap.command.shell".to_owned()).is_none());
        assert!(wire(&|b| b.capability_id = String::new()).is_none());
        assert!(wire(&|b| b.capability_version = String::new()).is_none());
        for state in ["", "pending", "drifted", "Approved", "approved "] {
            assert!(
                wire(&|b| b.review_state = state.to_owned()).is_none(),
                "{state:?}"
            );
        }
        for digest in ["", "abc", &"A".repeat(64), &"g".repeat(64), &"a".repeat(63)] {
            assert!(
                wire(&|b| b.tool_digest = digest.to_owned()).is_none(),
                "{digest:?}"
            );
        }
        // The stored class and floor are read the way the registry reads them: an
        // unknown class is unclassified, and the floor is never below the class's.
        let unknown = wire(&|b| {
            b.side_effect_class = "mystery".to_owned();
            b.approval_floor = "none".to_owned();
        })
        .expect("still a binding");
        assert_eq!(unknown.class, SideEffectClass::Unclassified);
        assert_eq!(unknown.floor, ApprovalFloor::Ask);
        let empty_floor = wire(&|b| b.approval_floor = String::new()).expect("still a binding");
        assert_eq!(empty_floor.floor, ApprovalFloor::DurableOwner);
    }

    #[test]
    fn run_bindings_are_per_run_and_org_and_pin_the_first_version_and_class() {
        let bindings = RunBindings::default();
        let binding = RegistryBinding {
            capability_id: "cap.mcp_tool.abc".to_owned(),
            capability_version: "1".to_owned(),
            class: SideEffectClass::Read,
            floor: ApprovalFloor::None,
            digest: DIGEST.to_owned(),
        };
        assert!(matches!(
            bindings.lookup("org-a", "run-a", "mcp__s__t"),
            BindingLookup::Unknown
        ));
        bindings.record(
            "org-a",
            "run-a",
            vec![("mcp__s__t".to_owned(), binding.clone())],
            vec!["mcp__s__gone".to_owned()],
        );
        assert!(matches!(
            bindings.lookup("org-a", "run-a", "mcp__s__t"),
            BindingLookup::Bound(found) if found == binding
        ));
        assert!(matches!(
            bindings.lookup("org-a", "run-a", "mcp__s__gone"),
            BindingLookup::Unbound
        ));
        // Another run or another tenant resolves nothing from this table.
        assert!(matches!(
            bindings.lookup("org-a", "run-b", "mcp__s__t"),
            BindingLookup::Unknown
        ));
        assert!(matches!(
            bindings.lookup("org-b", "run-a", "mcp__s__t"),
            BindingLookup::Unknown
        ));

        assert!(bindings.pin(
            "org-a",
            "run-a",
            "shell",
            "1.0.0",
            SideEffectClass::Destructive
        ));
        assert!(bindings.pin(
            "org-a",
            "run-a",
            "shell",
            "1.0.0",
            SideEffectClass::Destructive
        ));
        assert!(!bindings.pin(
            "org-a",
            "run-a",
            "shell",
            "1.0.1",
            SideEffectClass::Destructive
        ));
        assert!(!bindings.pin("org-a", "run-a", "shell", "1.0.0", SideEffectClass::Read));
        // A different run starts its own pins.
        assert!(bindings.pin("org-a", "run-b", "shell", "1.0.1", SideEffectClass::Read));
    }

    #[test]
    fn the_oldest_run_is_forgotten_first_and_a_runs_tool_table_is_bounded() {
        let bindings = RunBindings::default();
        for run in 0..=MAX_TRACKED_RUNS {
            assert!(bindings.pin(
                "org-a",
                &format!("run-{run}"),
                "shell",
                "1",
                SideEffectClass::Read
            ));
        }
        // run-0 was forgotten, so its pin starts over; the newest is still pinned.
        assert!(bindings.pin("org-a", "run-0", "shell", "2", SideEffectClass::Read));
        assert!(!bindings.pin(
            "org-a",
            &format!("run-{MAX_TRACKED_RUNS}"),
            "shell",
            "2",
            SideEffectClass::Read
        ));

        let one_run = RunBindings::default();
        for tool in 0..MAX_TOOLS_PER_RUN {
            assert!(one_run.pin(
                "org-a",
                "run-a",
                &format!("tool-{tool}"),
                "1",
                SideEffectClass::Read
            ));
        }
        assert!(
            !one_run.pin("org-a", "run-a", "one-too-many", "1", SideEffectClass::Read),
            "a run that tracks as many tools as it may fails closed for a new one"
        );
    }

    #[test]
    fn an_evicted_run_is_remembered_so_its_pins_cannot_silently_restart() {
        let bindings = RunBindings::default();
        for run in 0..=MAX_TRACKED_RUNS {
            assert!(bindings.pin(
                "org-a",
                &format!("run-{run}"),
                "mcp__s__t",
                "1",
                SideEffectClass::Read
            ));
        }
        // run-0 was evicted to make room for the newest; every other run is intact.
        assert!(bindings.pin_lost("org-a", "run-0"));
        assert!(!bindings.pin_lost("org-b", "run-0"), "per tenant");
        assert!(!bindings.pin_lost("org-a", "run-1"));
        assert!(!bindings.pin_lost("org-a", &format!("run-{MAX_TRACKED_RUNS}")));
        assert!(!bindings.pin_lost("org-a", "never-seen"));
        // The table it left behind is gone, which is exactly why the key is kept.
        assert!(matches!(
            bindings.lookup("org-a", "run-0", "mcp__s__t"),
            BindingLookup::Unknown
        ));
        // A table rebuilt for the run (a built-in tool pins afresh) does not clear it.
        assert!(bindings.pin("org-a", "run-0", "shell", "1", SideEffectClass::Read));
        assert!(bindings.pin_lost("org-a", "run-0"));

        // The memory is bounded: the oldest evicted key is forgotten first, and
        // the set never holds more than `MAX_EVICTED_RUNS` keys.
        let next = MAX_TRACKED_RUNS + 1;
        for run in next..(next + MAX_EVICTED_RUNS) {
            bindings.pin(
                "org-a",
                &format!("run-{run}"),
                "shell",
                "1",
                SideEffectClass::Read,
            );
        }
        let state = bindings.guard();
        assert_eq!(state.evicted.len(), MAX_EVICTED_RUNS);
        assert_eq!(state.evicted_set.len(), MAX_EVICTED_RUNS);
        assert!(state.runs.len() <= MAX_TRACKED_RUNS);
        drop(state);
        assert!(
            !bindings.pin_lost("org-a", "run-0"),
            "the oldest evicted key has aged out of the bounded set"
        );
        assert!(bindings.pin_lost("org-a", &format!("run-{}", MAX_EVICTED_RUNS)));
    }

    // ---------------------------------------------------------------------
    // Through a real gRPC channel to a stand-in CapabilityCore
    // ---------------------------------------------------------------------

    use mp_contracts::model_plane::v1::{
        capability_core_server::{CapabilityCore, CapabilityCoreServer},
        CapabilityDetail, CheckSkillPromotionRequest, CheckSkillPromotionResponse,
        GetCapabilityRequest, ListCapabilitiesRequest, ListCapabilitiesResponse,
        PromoteSkillRequest, PromoteSkillResponse, ResolveToolBindingsResponse,
        ValidateSkillBundleRequest, ValidateSkillBundleResponse,
    };
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::{Endpoint, Server};
    use tonic::Response;

    #[derive(Default)]
    struct MockState {
        evaluated: Mutex<Vec<EvaluatePolicyRequest>>,
        resolved: Mutex<Vec<ResolveToolBindingsRequest>>,
        bindings: Mutex<Vec<WireToolBinding>>,
        version: Mutex<String>,
        /// When set, the evidence is signed for this tool name instead of the one
        /// asked about: a replay of another tool's decision.
        signed_for_tool: Mutex<Option<String>>,
    }

    struct MockCapabilityCore {
        state: Arc<MockState>,
        key: SigningKey,
    }

    #[tonic::async_trait]
    impl CapabilityCore for MockCapabilityCore {
        async fn list_capabilities(
            &self,
            _: Request<ListCapabilitiesRequest>,
        ) -> Result<Response<ListCapabilitiesResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn get_capability(
            &self,
            _: Request<GetCapabilityRequest>,
        ) -> Result<Response<CapabilityDetail>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn evaluate_policy(
            &self,
            request: Request<EvaluatePolicyRequest>,
        ) -> Result<Response<EvaluatePolicyResponse>, Status> {
            let request = request.into_inner();
            self.state
                .evaluated
                .lock()
                .expect("evaluated lock")
                .push(request.clone());
            let bound = self
                .state
                .bindings
                .lock()
                .expect("bindings lock")
                .iter()
                .find(|binding| binding.tool_name == request.tool_name)
                .cloned();
            let (class, floor, digest) = match bound {
                Some(binding) => (
                    binding.side_effect_class,
                    binding.approval_floor,
                    binding.tool_digest,
                ),
                None => ("read".to_owned(), "none".to_owned(), String::new()),
            };
            let mut response = EvaluatePolicyResponse {
                decision: "allow".to_owned(),
                reason: "ok".to_owned(),
                budget_context: "budget".to_owned(),
                decision_id: "decision-1".to_owned(),
                capability_version: self.state.version.lock().expect("version lock").clone(),
                decision_evidence: String::new(),
                side_effect_class: class,
                approval_floor: floor,
                policy_source: "risk_level".to_owned(),
                tool_digest: digest,
            };
            let signed_tool = self
                .state
                .signed_for_tool
                .lock()
                .expect("override lock")
                .clone()
                .unwrap_or_else(|| request.tool_name.clone());
            let now = now_unix();
            response.decision_evidence = sign_json(
                &self.key,
                &serde_json::json!({
                    "v": "2",
                    "iat": now - 1,
                    "exp": now + 30,
                    "decision_id": response.decision_id,
                    "capability_id": request.capability_id,
                    "capability_version": response.capability_version,
                    "org_id": request.org_id,
                    "run_id": request.run_id,
                    "agent_id": request.agent_id,
                    "scope": request.scope,
                    "decision": response.decision,
                    "reason": response.reason,
                    "budget_context": response.budget_context,
                    "user_id": request.user_id,
                    "tool_name": signed_tool,
                    "side_effect_class": response.side_effect_class,
                    "approval_floor": response.approval_floor,
                    "policy_source": response.policy_source,
                    "tool_digest": response.tool_digest,
                }),
            );
            Ok(Response::new(response))
        }

        async fn resolve_tool_bindings(
            &self,
            request: Request<ResolveToolBindingsRequest>,
        ) -> Result<Response<ResolveToolBindingsResponse>, Status> {
            let request = request.into_inner();
            let requested = request.tool_names.clone();
            self.state
                .resolved
                .lock()
                .expect("resolved lock")
                .push(request);
            let bindings = self
                .state
                .bindings
                .lock()
                .expect("bindings lock")
                .iter()
                .filter(|binding| requested.contains(&binding.tool_name))
                .cloned()
                .collect();
            Ok(Response::new(ResolveToolBindingsResponse { bindings }))
        }

        async fn validate_skill_bundle(
            &self,
            _: Request<ValidateSkillBundleRequest>,
        ) -> Result<Response<ValidateSkillBundleResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn check_skill_promotion(
            &self,
            _: Request<CheckSkillPromotionRequest>,
        ) -> Result<Response<CheckSkillPromotionResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn promote_skill(
            &self,
            _: Request<PromoteSkillRequest>,
        ) -> Result<Response<PromoteSkillResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn list_skill_bundles(
            &self,
            _: Request<mp_contracts::model_plane::v1::ListSkillBundlesRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ListSkillBundlesResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn get_skill_bundle_manifest(
            &self,
            _: Request<mp_contracts::model_plane::v1::GetSkillBundleManifestRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::SkillBundleManifest>, Status> {
            Err(Status::unimplemented("not used"))
        }

        async fn get_skill_blob(
            &self,
            _: Request<mp_contracts::model_plane::v1::GetSkillBlobRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::GetSkillBlobResponse>, Status> {
            Err(Status::unimplemented("not used"))
        }
    }

    fn wire_binding(name: &str, class: &str, floor: &str) -> WireToolBinding {
        WireToolBinding {
            tool_name: name.to_owned(),
            capability_id: "cap.mcp_tool.abc".to_owned(),
            capability_version: "1.0.0".to_owned(),
            side_effect_class: class.to_owned(),
            approval_floor: floor.to_owned(),
            tool_digest: DIGEST.to_owned(),
            review_state: "approved".to_owned(),
            effective_policy: "allow".to_owned(),
        }
    }

    async fn harness(gate: ToolClassGate) -> (GrpcCapabilityPolicy, Arc<MockState>) {
        let key = SigningKey::from_bytes(&[5; 32]);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stand-in");
        let address = listener.local_addr().expect("stand-in address");
        let state = Arc::new(MockState::default());
        *state.version.lock().expect("version lock") = "1.0.0".to_owned();
        let mock = MockCapabilityCore {
            state: state.clone(),
            key: key.clone(),
        };
        tokio::spawn(async move {
            Server::builder()
                .add_service(CapabilityCoreServer::new(mock))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        let channel = Endpoint::from_shared(format!("http://{address}"))
            .expect("endpoint")
            .connect()
            .await
            .expect("connect to the stand-in");
        let policy = GrpcCapabilityPolicy {
            channel,
            tokens: TokenSource::Static(Arc::from("token")),
            decision_verifier: verifier_for(&key),
            model_action_view: None,
            gate,
            bindings: Arc::default(),
        };
        (policy, state)
    }

    fn denied(result: Result<CapabilityEvaluation, Status>) -> Status {
        result.expect_err("the call must be refused")
    }

    #[tokio::test]
    async fn a_forged_mcp_name_without_a_registry_row_is_denied_without_an_evaluation() {
        for gate in [ToolClassGate::Shadow, ToolClassGate::Enforce] {
            let (policy, state) = harness(gate).await;
            let error = denied(
                policy
                    .evaluate_tool_call("mcp__x__y", "run-a", "org-a", "user-a")
                    .await,
            );
            assert_eq!(error.code(), tonic::Code::PermissionDenied, "{gate:?}");
            assert!(state.evaluated.lock().unwrap().is_empty(), "{gate:?}");
            assert_eq!(state.resolved.lock().unwrap().len(), 1, "{gate:?}");

            // The refusal is remembered for the run: no second lookup.
            let again = denied(
                policy
                    .evaluate_tool_call("mcp__x__y", "run-a", "org-a", "user-a")
                    .await,
            );
            assert_eq!(again.code(), tonic::Code::PermissionDenied);
            assert_eq!(state.resolved.lock().unwrap().len(), 1, "{gate:?}");
        }
    }

    #[tokio::test]
    async fn with_the_gate_off_an_mcp_name_has_no_binding_and_nothing_is_asked() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        state
            .bindings
            .lock()
            .unwrap()
            .push(wire_binding("mcp__srv__echo", "read", "none"));
        let error = denied(
            policy
                .evaluate_tool_call("mcp__srv__echo", "run-a", "org-a", "user-a")
                .await,
        );
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert!(state.resolved.lock().unwrap().is_empty());
        assert!(state.evaluated.lock().unwrap().is_empty());
        policy
            .prefetch_tool_bindings("run-a", "org-a", "user-a", &["mcp__srv__echo".to_owned()])
            .await
            .expect("a prefetch is a no-op while off");
        assert!(state.resolved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_registered_mcp_tool_is_only_dispatchable_under_enforce() {
        // Shadow does everything enforce does, to observe it, and then refuses:
        // it is the legacy answer for an `mcp__` name that decides.
        for gate in [ToolClassGate::Shadow, ToolClassGate::Enforce] {
            let (policy, state) = harness(gate).await;
            state
                .bindings
                .lock()
                .unwrap()
                .push(wire_binding("mcp__srv__echo", "read", "none"));

            for _ in 0..2 {
                let result = policy
                    .evaluate_tool_call("mcp__srv__echo", "run-a", "org-a", "user-a")
                    .await;
                if gate == ToolClassGate::Shadow {
                    let error = denied(result);
                    assert_eq!(error.code(), tonic::Code::PermissionDenied);
                    assert!(error.message().contains("no governed capability binding"));
                    continue;
                }
                let evaluation = result.expect("a registered approved tool is evaluated");
                assert_eq!(evaluation.decision, CapabilityDecision::Allow, "{gate:?}");
                assert!(evaluation.evidence_verified, "{gate:?}");
                assert_eq!(evaluation.policy_source, "risk_level", "{gate:?}");
            }
            let evaluated = state.evaluated.lock().unwrap();
            assert_eq!(evaluated.len(), 2);
            assert_eq!(evaluated[0].tool_name, "mcp__srv__echo");
            assert_eq!(evaluated[0].user_id, "user-a");
            assert_eq!(evaluated[0].capability_id, "cap.mcp_tool.abc");
            assert_eq!(evaluated[0].org_id, "org-a");
            let resolved = state.resolved.lock().unwrap();
            assert_eq!(
                resolved.len(),
                1,
                "{gate:?}: the binding is cached for the run"
            );
            assert_eq!(resolved[0].tool_names, vec!["mcp__srv__echo".to_owned()]);
            assert_eq!(resolved[0].user_id, "user-a");
        }
    }

    #[tokio::test]
    async fn the_tool_and_the_user_are_named_to_capability_core_only_once_the_gate_is_on() {
        let (off, off_state) = harness(ToolClassGate::Off).await;
        let evaluation = off
            .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
            .await
            .expect("a built-in tool");
        assert_eq!(evaluation.decision, CapabilityDecision::Allow);
        let sent = off_state.evaluated.lock().unwrap();
        assert_eq!(sent[0].tool_name, "");
        assert_eq!(sent[0].user_id, "");
        assert_eq!(sent[0].capability_id, "cap.tool.http");
        drop(sent);

        let (shadow, shadow_state) = harness(ToolClassGate::Shadow).await;
        shadow
            .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
            .await
            .expect("a built-in tool");
        let sent = shadow_state.evaluated.lock().unwrap();
        assert_eq!(sent[0].tool_name, "web_fetch");
        assert_eq!(sent[0].user_id, "user-a");
        assert!(shadow_state.resolved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn evidence_minted_for_another_tool_is_refused_under_enforce_and_only_counted_under_shadow(
    ) {
        let (shadow, shadow_state) = harness(ToolClassGate::Shadow).await;
        // A built-in tool under shadow: counted, and the legacy tuple decides.
        shadow
            .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
            .await
            .map(|evaluation| assert_eq!(evaluation.decision, CapabilityDecision::Allow))
            .expect("shadow lets the legacy tuple decide for a built-in tool");
        *shadow_state.signed_for_tool.lock().unwrap() = Some("web_search".to_owned());
        shadow
            .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
            .await
            .expect("a replayed decision for a built-in tool is only counted under shadow");

        let (enforce, enforce_state) = harness(ToolClassGate::Enforce).await;
        *enforce_state.signed_for_tool.lock().unwrap() = Some("web_search".to_owned());
        let error = denied(
            enforce
                .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
                .await,
        );
        assert!(error.message().contains("invalid decision evidence"));

        // Replay: the stand-in signs the decision for a different tool name. A
        // dynamic tool is refused on it under enforce.
        enforce_state
            .bindings
            .lock()
            .unwrap()
            .push(wire_binding("mcp__srv__echo", "read", "none"));
        *enforce_state.signed_for_tool.lock().unwrap() = Some("web_fetch".to_owned());
        let error = denied(
            enforce
                .evaluate_tool_call("mcp__srv__echo", "run-a", "org-a", "user-a")
                .await,
        );
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert!(error.message().contains("invalid decision evidence"));
    }

    #[tokio::test]
    async fn a_tool_whose_version_changes_mid_run_fails_closed() {
        // A built-in tool under enforce: the first allow pins its version.
        let (enforce, state) = harness(ToolClassGate::Enforce).await;
        enforce
            .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
            .await
            .expect("first allow");
        *state.version.lock().unwrap() = "1.0.1".to_owned();
        let error = denied(
            enforce
                .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
                .await,
        );
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert!(error.message().contains("changed during the run"));
        // Another run is a new pin.
        enforce
            .evaluate_tool_call("web_fetch", "run-b", "org-a", "user-a")
            .await
            .expect("a new run pins afresh");

        // Under shadow a built-in tool is only counted.
        let (shadow, shadow_state) = harness(ToolClassGate::Shadow).await;
        shadow
            .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
            .await
            .expect("first allow");
        *shadow_state.version.lock().unwrap() = "1.0.1".to_owned();
        shadow
            .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
            .await
            .expect("shadow only counts a built-in tool's version change");

        // A dynamic tool is never allowed to move: its binding is the resolved one.
        state
            .bindings
            .lock()
            .unwrap()
            .push(wire_binding("mcp__srv__echo", "read", "none"));
        *state.version.lock().unwrap() = "1.0.0".to_owned();
        enforce
            .evaluate_tool_call("mcp__srv__echo", "run-a", "org-a", "user-a")
            .await
            .expect("resolved at 1.0.0");
        *state.version.lock().unwrap() = "1.0.2".to_owned();
        let error = denied(
            enforce
                .evaluate_tool_call("mcp__srv__echo", "run-a", "org-a", "user-a")
                .await,
        );
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert!(error.message().contains("invalid decision evidence"));
    }

    #[tokio::test]
    async fn a_prefetch_resolves_a_runs_dynamic_tools_in_one_lookup() {
        let (policy, state) = harness(ToolClassGate::Enforce).await;
        for name in ["mcp__srv__a", "mcp__srv__b"] {
            state
                .bindings
                .lock()
                .unwrap()
                .push(wire_binding(name, "read", "none"));
        }
        let names = [
            "mcp__srv__a",
            "mcp__srv__b",
            "mcp__srv__a",
            "mcp__srv__missing",
            "web_fetch",
            "mcp__",
        ]
        .map(str::to_owned);
        policy
            .prefetch_tool_bindings("run-a", "org-a", "user-a", &names)
            .await
            .expect("prefetch");
        {
            let resolved = state.resolved.lock().unwrap();
            assert_eq!(resolved.len(), 1);
            assert_eq!(
                resolved[0].tool_names,
                vec![
                    "mcp__srv__a".to_owned(),
                    "mcp__srv__b".to_owned(),
                    "mcp__srv__missing".to_owned()
                ],
                "only dynamic names, each once"
            );
        }
        for name in ["mcp__srv__a", "mcp__srv__b"] {
            policy
                .evaluate_tool_call(name, "run-a", "org-a", "user-a")
                .await
                .expect("a prefetched tool is evaluated without a second lookup");
        }
        let error = denied(
            policy
                .evaluate_tool_call("mcp__srv__missing", "run-a", "org-a", "user-a")
                .await,
        );
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(state.resolved.lock().unwrap().len(), 1);

        // Prefetching again asks nothing: every name is already known.
        policy
            .prefetch_tool_bindings("run-a", "org-a", "user-a", &names)
            .await
            .expect("prefetch");
        assert_eq!(state.resolved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_registry_binding_that_cannot_be_trusted_is_not_a_binding() {
        let cases: Vec<(&str, Vec<WireToolBinding>)> = vec![
            ("borrowed a built-in capability", {
                let mut binding = wire_binding("mcp__srv__echo", "read", "none");
                binding.capability_id = "cap.command.shell".to_owned();
                vec![binding]
            }),
            ("not approved", {
                let mut binding = wire_binding("mcp__srv__echo", "read", "none");
                binding.review_state = "drifted".to_owned();
                vec![binding]
            }),
            ("malformed digest", {
                let mut binding = wire_binding("mcp__srv__echo", "read", "none");
                binding.tool_digest = "not-a-digest".to_owned();
                vec![binding]
            }),
            (
                "ambiguous",
                vec![
                    wire_binding("mcp__srv__echo", "read", "none"),
                    wire_binding("mcp__srv__echo", "external_write", "none"),
                ],
            ),
        ];
        for (name, bindings) in cases {
            let (policy, state) = harness(ToolClassGate::Enforce).await;
            *state.bindings.lock().unwrap() = bindings;
            let error = denied(
                policy
                    .evaluate_tool_call("mcp__srv__echo", "run-a", "org-a", "user-a")
                    .await,
            );
            assert_eq!(error.code(), tonic::Code::PermissionDenied, "{name}");
            assert!(state.evaluated.lock().unwrap().is_empty(), "{name}");
        }
    }

    #[tokio::test]
    async fn a_name_unbound_in_one_run_is_resolved_afresh_by_the_next() {
        let (policy, state) = harness(ToolClassGate::Enforce).await;
        // The registry has no row for the tool yet.
        let error = denied(
            policy
                .evaluate_tool_call("mcp__srv__late", "run-a", "org-a", "user-a")
                .await,
        );
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        // A different run sees the tool once it is registered.
        state
            .bindings
            .lock()
            .unwrap()
            .push(wire_binding("mcp__srv__late", "read", "none"));
        policy
            .evaluate_tool_call("mcp__srv__late", "run-b", "org-a", "user-a")
            .await
            .expect("a later run resolves a tool registered since");
    }

    /// The legacy answer for an `mcp__` name is a refusal. Shadow may resolve and
    /// verify it, and count what disagrees, but no entry point may turn that
    /// refusal into an allow: only enforce does. This is the test that fails if
    /// shadow ever dispatches a dynamic tool the legacy gate denies.
    #[tokio::test]
    async fn shadow_never_allows_an_mcp_name_that_the_legacy_gate_denies() {
        for gate in [
            ToolClassGate::Off,
            ToolClassGate::Shadow,
            ToolClassGate::Enforce,
        ] {
            let (policy, state) = harness(gate).await;
            state
                .bindings
                .lock()
                .unwrap()
                .push(wire_binding("mcp__srv__echo", "read", "none"));
            // Every way the policy can answer for a name: with evidence, for a
            // user, and the bare decision.
            let with_user = policy
                .evaluate_tool_call("mcp__srv__echo", "run-a", "org-a", "user-a")
                .await;
            let with_evidence = policy
                .evaluate_with_evidence("mcp__srv__echo", "run-a", "org-a")
                .await;
            let bare = policy.evaluate("mcp__srv__echo", "run-a", "org-a").await;
            if gate == ToolClassGate::Enforce {
                assert_eq!(
                    with_user.expect("enforce allows it").decision,
                    CapabilityDecision::Allow
                );
                assert_eq!(
                    with_evidence.expect("enforce allows it").decision,
                    CapabilityDecision::Allow
                );
                assert_eq!(bare.expect("enforce decides"), CapabilityDecision::Allow);
                continue;
            }
            for (path, outcome) in [
                ("evaluate_tool_call", with_user.map(|e| e.decision)),
                ("evaluate_with_evidence", with_evidence.map(|e| e.decision)),
                ("evaluate", bare),
            ] {
                let error =
                    outcome.expect_err(&format!("{gate:?} must refuse an mcp name on {path}"));
                assert_eq!(
                    error.code(),
                    tonic::Code::PermissionDenied,
                    "{gate:?} {path}"
                );
                assert!(
                    error.message().contains("no governed capability binding"),
                    "{gate:?} {path}: {}",
                    error.message()
                );
            }
        }
    }

    /// A run idle long enough to be evicted from the bounded table has lost the
    /// version and class its dynamic tools were first allowed under. It must not
    /// silently pin whatever the registry holds now: the dynamic tool is refused
    /// with a typed reason, and a built-in tool, which resolves from the static map
    /// and not from the table, is unaffected.
    #[tokio::test]
    async fn a_dynamic_tool_on_an_evicted_run_fails_closed_and_a_builtin_does_not() {
        let (policy, state) = harness(ToolClassGate::Enforce).await;
        state
            .bindings
            .lock()
            .unwrap()
            .push(wire_binding("mcp__srv__echo", "read", "none"));
        policy
            .evaluate_tool_call("mcp__srv__echo", "run-a", "org-a", "user-a")
            .await
            .expect("pinned at 1.0.0 before the eviction");
        assert_eq!(state.resolved.lock().unwrap().len(), 1);

        // Enough other runs arrive to push run-a out of the table.
        for run in 0..MAX_TRACKED_RUNS {
            assert!(policy.bindings.pin(
                "org-a",
                &format!("other-{run}"),
                "web_fetch",
                "1.0.0",
                SideEffectClass::Read
            ));
        }
        assert!(policy.bindings.pin_lost("org-a", "run-a"));

        // The registry moved the tool on meanwhile; without the guard the run
        // would resolve and pin the new version as if it were the first.
        *state.version.lock().unwrap() = "9.9.9".to_owned();
        let evaluated_before = state.evaluated.lock().unwrap().len();
        let error = denied(
            policy
                .evaluate_tool_call("mcp__srv__echo", "run-a", "org-a", "user-a")
                .await,
        );
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert!(error.message().starts_with("binding_pin_lost"), "{error:?}");
        assert_eq!(
            state.evaluated.lock().unwrap().len(),
            evaluated_before,
            "no decision is requested for a tool whose pin was lost"
        );
        assert_eq!(
            state.resolved.lock().unwrap().len(),
            1,
            "and the registry is not asked to bind it afresh"
        );
        // A prefetch for the evicted run asks nothing either.
        policy
            .prefetch_tool_bindings("run-a", "org-a", "user-a", &["mcp__srv__echo".to_owned()])
            .await
            .expect("a prefetch is best effort");
        assert_eq!(state.resolved.lock().unwrap().len(), 1);

        // A built-in tool of the same run is not affected, and neither is a run
        // that was never evicted.
        *state.version.lock().unwrap() = "1.0.0".to_owned();
        policy
            .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
            .await
            .expect("a built-in tool does not depend on the pin table");
        policy
            .evaluate_tool_call("mcp__srv__echo", "run-b", "org-a", "user-a")
            .await
            .expect("a run that was not evicted resolves and pins as usual");
    }
}
