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
    /// The capability the call was bound to, as this process resolved it (never
    /// what the response claims); empty from a policy that does not say.
    pub capability_id: String,
    /// The pinned definition digest of the registry binding this process resolved
    /// for an MCP tool, never the response's claim; empty for a built-in tool and
    /// from a policy that does not say.
    pub tool_digest: String,
    /// capability-core's reason for the decision, verbatim; empty from a policy
    /// that does not say. A deny logs it only once it passes a slug check.
    pub reason: String,
    /// Whether an `ask` carried decision evidence that verifies for exactly this
    /// call, by the same check an `allow` must pass. A failed check is not an
    /// error: the ask still parks the step; it only means nothing may stand in for
    /// the approval (`runtime_loop::browser_grant_approval`). False for allow and
    /// deny.
    pub ask_evidence_verified: bool,
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
            capability_id: String::new(),
            tool_digest: String::new(),
            reason: String::new(),
            // Mirrors `evidence_verified`: a policy with no wire proof is trusted
            // as a whole or not at all.
            ask_evidence_verified: true,
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

/// Strict browser-only rollout; it does not change other tool classes.
#[must_use]
pub fn browser_agent_policy_enforced() -> bool {
    matches!(std::env::var("BROWSER_AGENT_POLICY_ENFORCED").as_deref(), Ok("true"))
}

/// The org permission rules switch (build plan W5-074, F-t3-074). Execution-core names a call's tool,
/// the person it acts for and its target to capability-core only while this is the literal `true`;
/// off, the request carries none of them.
#[must_use]
pub fn permission_rules_enabled() -> bool {
    matches!(
        std::env::var("CAPABILITY_PERMISSION_RULES_ENABLED").as_deref(),
        Ok("true")
    )
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

    /// `evaluate_tool_call` for a call whose input is known, so the call can name the target it acts
    /// on (F-t3-074). The target is read from `_tool_input` and named on the wire only while the
    /// permission rules switch is on; `_retention_bound` withholds its value. Policies with no wire
    /// ignore both.
    async fn evaluate_tool_call_with_input(
        &self,
        tool_name: &str,
        _tool_input: &str,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        _retention_bound: bool,
    ) -> Result<CapabilityEvaluation, Status> {
        self.evaluate_tool_call(tool_name, run_id, org_id, user_id)
            .await
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
    browser_agent_policy_enforced: bool,
    /// Whether a call names its tool, its person and its target to capability-core
    /// (`CAPABILITY_PERMISSION_RULES_ENABLED`).
    permission_rules: bool,
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
            browser_agent_policy_enforced: browser_agent_policy_enforced(),
            permission_rules: permission_rules_enabled(),
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
            browser_agent_policy_enforced: false,
            permission_rules: false,
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
            .evaluate_response(tool_name, run_id, org_id, "", None, "")
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
        // A call made without its input names no target, and a caller that never stated a run
        // posture must not claim "persistent": the retention is stated empty (WS13 S7).
        self.evaluate_tool_call_inner(tool_name, "", run_id, org_id, user_id, "", false)
            .await
    }

    async fn evaluate_tool_call_with_input(
        &self,
        tool_name: &str,
        tool_input: &str,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        retention_bound: bool,
    ) -> Result<CapabilityEvaluation, Status> {
        // The caller states the run's posture: "retention_bound" for a zero-retention or waived run,
        // "persistent" for a run that keeps state (WS13 S7, D-WS13-1).
        let run_retention = if retention_bound {
            "retention_bound"
        } else {
            "persistent"
        };
        self.evaluate_tool_call_inner(
            tool_name,
            tool_input,
            run_id,
            org_id,
            user_id,
            run_retention,
            retention_bound,
        )
        .await
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
    fn effective_gate(&self, tool_name: &str) -> ToolClassGate {
        if self.browser_agent_policy_enforced && tool_name == "browser_agent" {
            ToolClassGate::Enforce
        } else {
            self.gate
        }
    }

    /// Whether a call names its tool and the person it acts for to capability-core: while the tool
    /// class gate is on (it decides on them), and while the permission rules are on (a rule is keyed
    /// to the tool and the person).
    fn names_call(&self, tool_name: &str) -> bool {
        self.effective_gate(tool_name) != ToolClassGate::Off || self.permission_rules
    }

    /// The targets a call is checked against, one check each (F-t3-074), read from its input and
    /// named only while the permission rules switch is on: none otherwise, so the request is
    /// unchanged. A zero-retention or waived run withholds every value, so its targets collapse to one
    /// check of the kind with the value withheld. Past `MAX_TARGET_CHECKS` distinct values the call is
    /// checked the same way: capability-core reads a withheld value as every value of the kind, so that
    /// one check is at least as strict as each value on its own.
    fn wire_targets(
        &self,
        tool_name: &str,
        tool_input: &str,
        retention_bound: bool,
    ) -> Vec<(&'static str, String)> {
        if !self.permission_rules {
            return Vec::new();
        }
        let targets = derive_targets(tool_name, tool_input);
        let Some(kind) = targets.first().map(|(kind, _)| *kind) else {
            return Vec::new();
        };
        if retention_bound || targets.len() > MAX_TARGET_CHECKS {
            return vec![(kind, String::new())];
        }
        targets
    }

    /// The body of a tool-call evaluation. `run_retention` is the posture stated on the wire
    /// ("persistent", "retention_bound", or "" when the caller never stated one); `retention_bound`
    /// separately withholds the target value (WS13 S7).
    #[allow(clippy::too_many_arguments)]
    async fn evaluate_tool_call_inner(
        &self,
        tool_name: &str,
        tool_input: &str,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        run_retention: &'static str,
        retention_bound: bool,
    ) -> Result<CapabilityEvaluation, Status> {
        let targets = self.wire_targets(tool_name, tool_input, retention_bound);
        if targets.is_empty() {
            return self
                .evaluate_target(tool_name, run_id, org_id, user_id, None, run_retention)
                .await;
        }
        // A call with targets is checked once per target, and the strictest answer decides it: a
        // command or a recipient the policy denies cannot be hidden behind an allowed one (F-t3-074).
        let mut evaluations = Vec::with_capacity(targets.len());
        for target in targets {
            evaluations.push(
                self.evaluate_target(
                    tool_name,
                    run_id,
                    org_id,
                    user_id,
                    Some(target),
                    run_retention,
                )
                .await?,
            );
        }
        strictest_evaluation(evaluations)
    }

    /// One check of a tool call: the request for `target` (or for no target at all), and the
    /// verification of what capability-core answers to it.
    async fn evaluate_target(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        target: Option<(&'static str, String)>,
        run_retention: &'static str,
    ) -> Result<CapabilityEvaluation, Status> {
        let strict_browser = self.browser_agent_policy_enforced && tool_name == "browser_agent";
        let (binding, response) = self
            .evaluate_response(tool_name, run_id, org_id, user_id, target, run_retention)
            .await?;
        let evaluation = map_capability_response(&response)?;
        let call = EvidenceCall {
            tool_name,
            run_id,
            org_id,
            user_id,
            capability_id: &binding.capability_id,
            registry: binding.registry.as_ref(),
            gate: self.effective_gate(tool_name),
            named: self.names_call(tool_name),
        };
        if strict_browser {
            // A valid v2 signature alone is insufficient: older capability-core
            // builds can sign a decision while ignoring stored user/org policies.
            let checked = self.decision_verifier
                .verified_claims(&response.decision_evidence, &response, &call)
                .is_some_and(|claims| claims.tool_policy_checked);
            if !checked {
                return Err(Status::permission_denied(
                    "browser policy requires signed tool-policy enforcement evidence",
                ));
            }
        }
        // An `ask` is checked too, but never refused for failing: it parks the
        // step either way. Only a verified ask may later be satisfied by a
        // broker-validated browser run grant, so it is held to the strict
        // check whatever the gate mode (see `verify_ask`).
        let ask_evidence_verified = evaluation.decision == CapabilityDecision::Ask
            && self
                .decision_verifier
                .verify_ask(&response.decision_evidence, &response, &call);
        if evaluation.decision == CapabilityDecision::Allow {
            if !self
                .decision_verifier
                .verify(&response.decision_evidence, &response, &call)
            {
                return Err(Status::permission_denied(
                    "capability policy returned invalid decision evidence",
                ));
            }
            self.pin_binding(&call, &response)?;
        } else if strict_browser && evaluation.decision == CapabilityDecision::Ask {
            // The broker can satisfy the high-risk ask only when its class has
            // remained pinned under the same verified policy during the run.
            self.pin_binding(&call, &response)?;
        }
        self.refuse_dynamic_unless_enforced(&binding)?;
        Ok(CapabilityEvaluation {
            evidence_verified: evaluation.decision != CapabilityDecision::Allow
                || self.decision_verifier.is_configured(),
            capability_id: binding.capability_id.clone(),
            tool_digest: binding
                .registry
                .as_ref()
                .map(|registry| registry.digest.clone())
                .unwrap_or_default(),
            ask_evidence_verified,
            ..evaluation
        })
    }

    /// Bind `tool_name` and ask capability-core for a decision on it. The binding
    /// comes back so the caller can verify the evidence against what was bound.
    async fn evaluate_response(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
        user_id: &str,
        target: Option<(&'static str, String)>,
        run_retention: &'static str,
    ) -> Result<(Binding, EvaluatePolicyResponse), Status> {
        if run_id.trim().is_empty() || org_id.trim().is_empty() {
            return Err(Status::invalid_argument(
                "run and tenant are required for capability policy",
            ));
        }
        if self.browser_agent_policy_enforced && tool_name == "browser_agent" && user_id.trim().is_empty() {
            return Err(Status::permission_denied("browser policy requires the verified run owner"));
        }
        let binding = self.binding_for(tool_name, run_id, org_id, user_id).await?;
        let bearer = self.tokens.token(org_id).await.map_err(|error| {
            tracing::warn!(%error, "capability service credential unavailable");
            Status::unavailable("capability policy credential unavailable")
        })?;
        // The tool and the user are named only while the gate or the permission rules need them, so
        // with both off the request is byte-for-byte what it was before tool classes.
        let named = self.names_call(tool_name);
        let (target_kind, target_value) = target.map_or_else(
            || (String::new(), String::new()),
            |(kind, value)| (kind.to_owned(), value),
        );
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
            target_kind,
            target_value,
            run_retention: run_retention.to_owned(),
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
        if call.gate == ToolClassGate::Off || response.side_effect_class.is_empty() {
            return Ok(());
        }
        // The run may have been evicted since the binding was resolved. A dynamic
        // tool has no pin to compare with then, so it fails closed instead of
        // pinning afresh; a built-in tool is not affected.
        if call.registry.is_some() && self.bindings.pin_lost(call.org_id, call.run_id) {
            return Err(pin_lost());
        }
        let class = SideEffectClass::from_wire(&response.side_effect_class);
        // The narrow browser gate also pins the signed floor: losing or lowering
        // an approval requirement during the same run is never a new allow.
        let browser_version = (self.browser_agent_policy_enforced && call.tool_name == "browser_agent")
            .then(|| format!("{}\0{}", response.capability_version, response.approval_floor));
        let version = browser_version.as_deref().unwrap_or(&response.capability_version);
        if self.bindings.pin(
            call.org_id,
            call.run_id,
            call.tool_name,
            version,
            class,
        ) {
            return Ok(());
        }
        if call.gate == ToolClassGate::Enforce {
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
        // The binding is the caller's, not the response's: set by
        // `evaluate_tool_call`.
        capability_id: String::new(),
        // Set by `evaluate_tool_call_with_input` from the binding.
        tool_digest: String::new(),
        reason: response.reason.clone(),
        ask_evidence_verified: false,
    })
}

/// The evaluation a call checked against several targets carries: the strictest decision (a deny over
/// an ask over an allow), and of that decision the first one. The whole evaluation is kept, never
/// its fields mixed, so its evidence still describes the decision it carries.
fn strictest_evaluation(
    evaluations: Vec<CapabilityEvaluation>,
) -> Result<CapabilityEvaluation, Status> {
    evaluations
        .into_iter()
        .reduce(|current, next| {
            if decision_strictness(next.decision) > decision_strictness(current.decision) {
                next
            } else {
                current
            }
        })
        .ok_or_else(|| Status::internal("a tool call was checked against no target"))
}

/// How strict a decision is: a deny outranks an ask, which outranks an allow.
fn decision_strictness(decision: CapabilityDecision) -> u8 {
    match decision {
        CapabilityDecision::Allow => 0,
        CapabilityDecision::Ask => 1,
        CapabilityDecision::Deny => 2,
    }
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
        self.verified_claims(evidence, response, call).is_some()
    }

    /// `verify` for an `ask` that a browser run grant may later satisfy. On top
    /// of `verify`, the signed v2 claims must bind what the lift relies on, in
    /// every gate mode: the policy source (so a stored tool-policy ask can never
    /// pass as the risk-level floor), and the tool and user exactly as this
    /// process named them in the request (empty while neither the gate nor the
    /// permission rules name them). `verify` alone skips these checks under `Off`
    /// and only counts a mismatch under `Shadow`.
    fn verify_ask(
        &self,
        evidence: &str,
        response: &EvaluatePolicyResponse,
        call: &EvidenceCall<'_>,
    ) -> bool {
        let Some(claims) = self.verified_claims(evidence, response, call) else {
            return false;
        };
        let named = call.named;
        claims.v == "2"
            && claims.policy_source == response.policy_source
            && claims.tool_name == if named { call.tool_name } else { "" }
            && claims.user_id == if named { call.user_id } else { "" }
    }

    /// The claims of evidence that passes `verify`, or `None`.
    fn verified_claims(
        &self,
        evidence: &str,
        response: &EvaluatePolicyResponse,
        call: &EvidenceCall<'_>,
    ) -> Option<DecisionClaims> {
        let key = self.key.as_ref()?;
        let parts: Vec<&str> = evidence.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        let decode = |part: &str| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(part)
                .ok()
        };
        let (Some(header), Some(payload), Some(signature)) =
            (decode(parts[0]), decode(parts[1]), decode(parts[2]))
        else {
            return None;
        };
        #[derive(Deserialize)]
        struct Header {
            alg: String,
            typ: String,
        }
        let header = serde_json::from_slice::<Header>(&header).ok()?;
        let claims = serde_json::from_slice::<DecisionClaims>(&payload).ok()?;
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
            return None;
        }
        let signature = Signature::from_slice(&signature).ok()?;
        if key
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .is_err()
        {
            return None;
        }
        Self::version_accepts(&claims, response, call).then_some(claims)
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
    #[serde(default)]
    tool_policy_checked: bool,
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
    /// Whether the request named the tool and the person (`GrpcCapabilityPolicy::names_call`).
    named: bool,
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
        // The granular MCP facade never creates raw browser authority: BrowserBroker separately binds
        // its opaque run grant. The legacy autonomous loop and the grant-bound console read
        // (`browser.console`, WS9.E) keep cap.browser.open. The read tools of the Do loop
        // (browser.open, browser.observe; WS21-S5a) and its act name each have their own capability,
        // so a read-level grant never answers for a write. Migration 0025 seeds them.
        "browser_agent" | "browser.console" => "cap.browser.open",
        "browser.open" | "browser.observe" | "browser.close" => "cap.browser.observe",
        "browser.act" => "cap.browser.act",
        // The Do loop's computer tools (W4-C C1): a look at the screen and one action on it. Each has its own
        // capability, as the browser tools do, so a read never answers for an act. Migration 0038 seeds both.
        "computer.observe" => "cap.computer.observe",
        "computer.act" => "cap.computer.act",
        "web_search" | "web_fetch" | "web.search" | "web.read" => "cap.tool.http",
        // The long-page reader's two tools (EP-2-S10) read pages a fetch already held for this run, so they
        // carry the fetch's own class. They are named only while the reader is on: with it off they stay
        // unbound and fail closed, as they did before the reader existed.
        "read_page" | "find_in_page" if crate::web_tools::reader_enabled() => "cap.tool.http",
        // E3.7.10: the bulk-acquisition tools. A crawl or a batch multiplies pages, spend and
        // egress by a count the model picks, so each family has its own capability instead of
        // widening `cap.tool.http` (capability-core migration 0031 seeds the three ids and the
        // tool classes; they stay unavailable until execution-core attests them).
        "knowledge.crawl_site" => "cap.web.crawl",
        "knowledge.map" => "cap.web.map",
        // Q4-E4.3-055c1: the read-only crawl planner maps a site (the same `/v1/map` read) and drafts a scope
        // for a person to approve, so it rides the map's capability; its seed row is Q4-E4.3-055c2.
        "knowledge.crawl_plan" => "cap.web.map",
        "knowledge.batch" => "cap.web.batch",
        "knowledge_search" => "cap.retrieval.query",
        // The owner document inventory (WS24-S8) is a read of the same Data Plane corpus under the
        // run's own bearer, so it is governed by the same retrieval capability.
        "knowledge_list_documents" => "cap.retrieval.query",
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
        // The question tool (WS14 S4) writes one owner-scoped row through session-core, the same
        // dependency and low-risk shape as `plan.propose`, so it binds to that seeded, attested id.
        // Its class is seeded beside `plan.propose`'s row in capability-core; the parity test
        // (tests/tool_class_registry_parity.rs) refuses a bound name that has no seed.
        "ask_user" => "cap.memory.index",
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
        // The custom REST tools (W4-CC-7): one capability each, seeded unavailable by capability-core's migration
        // 0041 until an attestation names them. Each name binds to its own seeded row here, and never to a name
        // the model proposed.
        "custom_rest.read" => "cap.custom_rest.read",
        "custom_rest.write" => "cap.custom_rest.write",
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

/// Whether a Do run's agent tool allowlist (Wave 3 EP-6 S9) permits a tool: the tool must have a governed
/// capability ([`trusted_capability_id`]) and that capability must be one the allowlist lists. Fail closed: a tool
/// with no binding, every MCP tool among them, is never permitted, so an allowlist only ever removes tools.
#[must_use]
pub fn agent_allowlist_permits(capability_ids: &[String], tool_name: &str) -> bool {
    trusted_capability_id(tool_name)
        .is_some_and(|capability| capability_ids.iter().any(|id| *id == capability))
}

/// Most distinct values one call is checked against, one check each (F-t3-074). Past this a call is
/// checked once with its value withheld (`GrpcCapabilityPolicy::wire_targets`).
const MAX_TARGET_CHECKS: usize = 32;

/// The targets a tool call acts on, read from its input by execution-core and never from the model's
/// text (F-t3-074), each as its kind and value: the host of a web address (`web_fetch`,
/// `browser.open`); the domain of every recipient of a mail send, in `to`, `cc` and `bcc`
/// (`execute_provider_action`); and each command a shell runs (`command_names`; `shell`, and
/// `code_interpreter` and `process_start` in `sh`): the arguments a shell is given after `-c`, or the withheld value where it reads
/// its commands from a script file or its standard input, or has no command string (`shell_script_arguments`), the withheld value for a command word that is a
/// path outside the system binary directories, which may be a script (`is_system_binary_path`), the command a wrapper program
/// runs (`env`, `sudo`, `xargs`, `builtin` and the others in `WRAPPERS`, and the list of a zsh `repeat`, read by
/// `list_names`), the command of each `find -exec` predicate, the command that an option or address of an
/// `OPTION_VALUE_COMMANDS` program gives (`option_value_command_names`), the withheld value of an argument of one that
/// holds an expansion (`argument_may_hide_an_option`), the withheld command of a program in
/// `WITHHELD_COMMANDS`, the text `eval` runs, the text a `trap` action or an `alias` value runs, and the withheld value
/// where a call of an alias may run the words it is given (`alias_value_hides_a_command`). Empty for every
/// other tool, and for an input that does not read. Each value is named once, in the order it appears;
/// an empty value is the withheld one, which covers every value of its kind (see `mail_recipient_domains`).
#[must_use]
pub fn derive_targets(tool_name: &str, tool_input: &str) -> Vec<(&'static str, String)> {
    let names_a_target = matches!(
        tool_name,
        "web_fetch"
            | "browser.open"
            | "execute_provider_action"
            | "shell"
            | "code_interpreter"
            | "process_start"
    );
    if !names_a_target {
        return Vec::new();
    }
    let Ok(input) = serde_json::from_str::<serde_json::Value>(tool_input) else {
        return Vec::new();
    };
    let (kind, values): (&'static str, Vec<String>) = match tool_name {
        "web_fetch" | "browser.open" => (
            "domain",
            input
                .get("url")
                .and_then(serde_json::Value::as_str)
                .and_then(url_host)
                .into_iter()
                .collect(),
        ),
        "execute_provider_action" => ("recipient", mail_recipient_domains(&input)),
        "shell" => ("command_prefix", shell_program_commands(&input)),
        // A Python program names no command; only the `sh` language runs one. `code_interpreter` lowercases its language,
        // so `SH` runs `sh` there. `process_start` runs only the trimmed language `sh`, exactly, and refuses any other
        // spelling before it starts anything (process_tools.rs), so only that spelling is read for it. Its `args` are the
        // script's positional parameters, which name nothing here.
        "code_interpreter" | "process_start" => {
            let is_sh = input
                .get("language")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|language| {
                    let language = language.trim();
                    if tool_name == "process_start" {
                        language == "sh"
                    } else {
                        language.eq_ignore_ascii_case("sh")
                    }
                });
            if !is_sh {
                return Vec::new();
            }
            let script = input.get("code").and_then(serde_json::Value::as_str);
            (
                "command_prefix",
                script.map(command_prefixes).unwrap_or_default(),
            )
        }
        _ => return Vec::new(),
    };
    values.into_iter().map(|value| (kind, value)).collect()
}

/// The host of a web address, lower-case and without a port or a trailing dot. Only `http` and `https`
/// addresses name a host.
fn url_host(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// The domains of every recipient a mail send names, in `to`, `cc` and `bcc` as the tool input carries
/// them, each once. Empty for an operation that is not a mail send, and for a send that names no
/// recipient. A send whose recipients cannot be checked by domain is checked as every recipient
/// instead: a body that integration-corev2 passes on unread (a raw message for Gmail, a whole message
/// object for Microsoft), and a recipient whose domain is not a plain DNS name (an address literal, an
/// internationalized name) or that names none. That is the single empty value, which capability-core
/// reads as any recipient (`GrpcCapabilityPolicy::wire_targets`).
fn mail_recipient_domains(input: &serde_json::Value) -> Vec<String> {
    let Some(operation) = input.get("operation").and_then(serde_json::Value::as_str) else {
        return Vec::new();
    };
    if !mp_contracts::provider_writes::is_mail_send(operation) {
        return Vec::new();
    }
    let body = input.get("body");
    let passed_on_unread = body.is_some_and(|body| {
        body.get("raw")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|raw| !raw.trim().is_empty())
            || body
                .get("message")
                .is_some_and(serde_json::Value::is_object)
    });
    if passed_on_unread {
        return vec![String::new()];
    }
    let mut domains = Vec::new();
    for field in ["to", "cc", "bcc"] {
        let recipients =
            mp_contracts::provider_writes::mail_recipients(body.and_then(|body| body.get(field)));
        for recipient in recipients {
            match recipient_domains(&recipient) {
                Some(found) => domains.extend(found),
                None => return vec![String::new()],
            }
        }
    }
    distinct(domains)
}

/// The domain of each address a recipient string holds: what follows each `@`, up to the first
/// delimiter. A string that holds two addresses, or an `@` inside a comment, names the domain of each,
/// so no address hides behind another; never the address itself. `None` when one of those domains is
/// not a plain DNS name this can match exactly (a character outside letters, digits, `.` and `-`, or no
/// domain at all), since a truncated name could match a rule the address does not belong to.
fn recipient_domains(recipient: &str) -> Option<Vec<String>> {
    let mut domains = Vec::new();
    for (at, _) in recipient.match_indices('@') {
        // A domain ends at whitespace or at a mark that closes, separates or quotes an address.
        let token = recipient[at + 1..]
            .split(|c: char| c.is_whitespace() || "<>()[],;:\"'\\@".contains(c))
            .next()
            .unwrap_or("");
        let domain = token.trim_end_matches('.');
        if domain.is_empty()
            || !domain
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        {
            return None;
        }
        domains.push(domain.to_ascii_lowercase());
    }
    (!domains.is_empty()).then_some(domains)
}

/// The distinct command names a shell text runs: the command of each of its segments, and what that command
/// runs (`command_words_names`). A backtick substitution and an extended glob are read as `$(` first
/// (`read_expansions_as_substitutions`). A line continuation is read both as written, where its line break ends
/// a command, and joined, as a shell that joins the line runs it. Each reading names its commands, so a command
/// that one reading hides is named by the other. The text is also cut where `quote_aware_segments` cuts it, which
/// a separator inside quotes does not, so a quoted script reaches `command_words_names` whole and the reading of
/// that script names the commands it runs. Each command substitution is lifted out and read as shell text of its
/// own (`lift_substitutions`), so the quotes inside it cannot hide the commands around it. A zsh special hash's array
/// (`aliases=(...)`, `functions+=(...)`, `commands=(...)`) is lifted out too, and each element is read (`lift_zsh_hash_arrays`).
/// A text that ends inside
/// an open quote or substitution withholds its value, and so does one the readings part on: an ANSI-C string outside
/// quotes (`quote_aware_segments`), a cut segment that the quote-aware reading does not start where it starts
/// (`cut_segment_out_of_step`), and a substitution whose body holds a `case` word (`holds_case_word`).
fn command_prefixes(text: &str) -> Vec<String> {
    command_prefixes_at(text, 0)
}

/// `command_prefixes` for a text `depth` levels into nested scripts and wrapped commands. Past `MAX_SHELL_DEPTH`
/// the names are withheld instead of followed, which bounds the recursion. A segment that the readings share is
/// named once.
fn command_prefixes_at(text: &str, depth: usize) -> Vec<String> {
    if depth > MAX_SHELL_DEPTH {
        return vec![String::new()];
    }
    let text = read_expansions_as_substitutions(text);
    let (text, mut bodies, mut hash_elements, unclosed) = lift_substitutions(&text);
    let joined = join_line_continuations(&text);
    // The joined text is lifted too, where a line continuation changes it: the joining can make a `$(` or a name's `=(`
    // that the text as written does not show, so its substitutions and arrays are read as they are in the joined text.
    let joined_lifted = (joined != text).then(|| lift_substitutions(&joined));
    let mut segments = shell_segments(&text);
    let (aware, aware_open) = quote_aware_segments(&text);
    let mut withheld = unclosed || aware_open || cut_segment_out_of_step(&text, &aware);
    segments.extend(aware);
    if let Some((lifted, joined_bodies, joined_elements, joined_open)) = &joined_lifted {
        bodies.extend(joined_bodies.iter().copied());
        hash_elements.extend(joined_elements.iter().cloned());
        segments.extend(shell_segments(lifted));
        let (aware, joined_quote_open) = quote_aware_segments(lifted);
        withheld |= *joined_open || joined_quote_open || cut_segment_out_of_step(lifted, &aware);
        segments.extend(aware);
    }
    withheld |= bodies.iter().any(|body| holds_case_word(body));
    let mut seen = HashSet::new();
    let mut names: Vec<String> = segments
        .into_iter()
        .filter(|segment| seen.insert(*segment))
        .flat_map(|segment| command_prefix(segment, depth))
        .collect();
    if withheld {
        names.push(String::new());
    }
    names.extend(
        bodies
            .into_iter()
            .flat_map(|body| command_prefixes_at(body, depth + 1)),
    );
    names.extend(
        hash_elements
            .into_iter()
            .flat_map(|(kind, element)| hash_element_names(kind, &element, depth)),
    );
    distinct(names)
}

/// The text with each line continuation joined, as a shell that joins the line reads it: a backslash before
/// a line feed, or before a carriage return and a line feed, is removed with the break. Backslashes pair up
/// as escapes, so only the last one of an odd run continues the line; after an even run the break stays.
fn join_line_continuations(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut joined = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        let run_start = i;
        while i < bytes.len() && bytes[i] == b'\\' {
            i += 1;
        }
        let break_len = match (bytes.get(i), bytes.get(i + 1)) {
            (Some(b'\n'), _) => 1,
            (Some(b'\r'), Some(b'\n')) => 2,
            _ => 0,
        };
        if break_len > 0 && (i - run_start) % 2 == 1 {
            // The last backslash of the run continues the line, so it and the break are dropped.
            joined.push_str(&text[copied..i - 1]);
            i += break_len;
            copied = i;
        }
    }
    joined.push_str(&text[copied..]);
    joined
}

/// The text with each backtick substitution written as `$(` and `)`, the two ends of a substitution in turn, and
/// each extended-glob group (`+(`, `@(`, `!(`) written as `$(`. A substitution or a pattern that starts a
/// command word then puts a `$` in it, which `is_expanding_word` withholds, and the commands inside are still
/// split and read. (`?(` and `*(` already hold a `?` or a `*` there.)
fn read_expansions_as_substitutions(text: &str) -> String {
    let mut written = String::with_capacity(text.len());
    let mut inside_backticks = false;
    for c in text.chars() {
        match c {
            '`' => {
                written.push_str(if inside_backticks { ")" } else { "$(" });
                inside_backticks = !inside_backticks;
            }
            '(' if matches!(written.chars().next_back(), Some('+' | '@' | '!')) => {
                written.pop();
                written.push_str("$(");
            }
            _ => written.push(c),
        }
    }
    written
}

/// The command names a `shell` call runs. Its `program` runs with its `args` and no shell, so the program's own
/// name is always a command (`program_name`), and a program that is more than a plain name is read as shell text
/// too. The program's words and its `args`, as one argv, are then read as a command (`command_words_names`): a
/// wrapper among them runs the command it is given, and a shell is given its script through `-c`.
fn shell_program_commands(input: &serde_json::Value) -> Vec<String> {
    let Some(program) = input.get("program").and_then(serde_json::Value::as_str) else {
        return Vec::new();
    };
    let mut names: Vec<String> = program_name(program);
    if !is_plain_program(program) {
        names.extend(command_prefixes(program));
    }
    let mut argv: Vec<String> = program
        .split_whitespace()
        .skip_while(|word| is_plain_assignment(word))
        .map(str::to_owned)
        .collect();
    argv.extend(
        input
            .get("args")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned),
    );
    names.extend(command_words_names(&argv, 0, false));
    distinct(names)
}

/// Whether a program is a plain name: letters, digits and the marks of a path or a flag, with no space and no shell
/// syntax. Read as shell text, such a name names only itself, plus the default command of a wrapper with no command
/// in its text (`xargs` runs `echo`). The argv gives the command a wrapper runs, so that default is not added here.
fn is_plain_program(program: &str) -> bool {
    program
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '+'))
}

/// The names of the program a `shell` call runs: the first word of its `program` that is not a plain assignment
/// (`is_plain_assignment`), read from the whitespace-separated text as the command name always was (`command_names`). The
/// program runs with its arguments and no shell, so no keyword or option is skipped: `time` names itself, and a word such as
/// `A+=/bin/rm` is the program too, as no shell reads it as an assignment.
fn program_name(program: &str) -> Vec<String> {
    program
        .split_whitespace()
        .find(|word| !is_plain_assignment(word))
        .map(command_names)
        .unwrap_or_default()
}

/// The POSIX-family shells, whose `-c` text is read as shell text (`shell_script_arguments`): the Bourne-style shells,
/// bash and its restricted form, and the Korn-style shells.
const POSIX_SHELLS: &[&str] = &[
    "sh", "bash", "rbash", "dash", "ash", "zsh", "ksh", "mksh", "oksh", "pdksh", "rksh", "posh",
    "yash",
];

/// The shells whose syntax the reader does not model: csh and tcsh, fish, PowerShell, elvish, nu, xonsh and cmd. Their
/// `-c` text is read as a POSIX shell's is, and their value is withheld too (`command_words_names`): a command written in
/// their own syntax, such as `^rm` in nu, `iex` in PowerShell or `e:rm` in elvish, runs a command the reader does not name.
const OTHER_SHELLS: &[&str] = &[
    "csh",
    "tcsh",
    "fish",
    "pwsh",
    "powershell",
    "elvish",
    "nu",
    "xonsh",
    "cmd",
];

/// The name of a program: its last path component, whichever separator it uses.
fn shell_name(program: &str) -> &str {
    program.rsplit(['/', '\\']).next().unwrap_or_default()
}

/// Whether a program is a POSIX-family shell, by its name whatever its directory.
fn is_posix_shell(program: &str) -> bool {
    POSIX_SHELLS.contains(&shell_name(program))
}

/// Whether a program is one of the shells in `OTHER_SHELLS`, by its name whatever its directory.
fn is_other_shell(program: &str) -> bool {
    OTHER_SHELLS.contains(&shell_name(program))
}

/// Whether a program is PowerShell, which takes its command after `-Command` (`powershell_command_arguments`).
fn is_powershell(program: &str) -> bool {
    matches!(shell_name(program), "pwsh" | "powershell")
}

/// The options a POSIX-family shell takes a value for, by its name: `-o option` in every shell, bash's `-O shopt`,
/// `--rcfile` and `--init-file`, mksh's `-T tty`, and ksh93's `-R file` (`rksh` is its restricted form). The value is
/// read with its option, so a word shaped like `-c` after one is no `-c` flag (`shell_script_arguments`). An option this
/// table leaves out is read as a flag, so the table holds only the options named here.
fn shell_value_options(program: &str) -> (&'static str, &'static [&'static str]) {
    const NONE: &[&str] = &[];
    const BASH_LONG: &[&str] = &["rcfile", "init-file"];
    match shell_name(program) {
        "bash" | "rbash" => ("oO", BASH_LONG),
        "mksh" => ("oT", NONE),
        "ksh" | "rksh" => ("oR", NONE),
        _ => ("o", NONE),
    }
}

/// bash's long options, which it reads ahead of its short options, as the leading words of its arguments, with one dash
/// or two (`bash_long_option`). These are the names bash 5.3.15's `--help` lists, `protected` among them, which takes no
/// value (`-protected`). Only `rcfile` and `init-file` take a value, from `shell_value_options`.
const BASH_LONG_OPTIONS: &[&str] = &[
    "debug",
    "debugger",
    "dump-po-strings",
    "dump-strings",
    "help",
    "init-file",
    "login",
    "noediting",
    "noprofile",
    "norc",
    "posix",
    "pretty-print",
    "protected",
    "rcfile",
    "restricted",
    "verbose",
    "version",
    "wordexp",
];

/// The name of the bash long option a word is, read with one dash or two (`-login`, `--login`), when the name is one of
/// `BASH_LONG_OPTIONS`. Any other word, `--` and a lone `-` included, is no long option, and so ends the phase that reads
/// them (`shell_script_arguments`).
fn bash_long_option(word: &str) -> Option<&str> {
    let name = word.strip_prefix("--").or_else(|| word.strip_prefix('-'))?;
    BASH_LONG_OPTIONS.contains(&name).then_some(name)
}

/// The arguments a shell reads as shell text, when its commands come from its `-c` option: every argument from the `-c` on,
/// the command string among them. A bash or rbash reads its long options first, the leading words `bash_long_option`
/// names, and the first word that is none ends that phase. The options after it are read as getopt reads them, letter by
/// letter: a flag letter leads to the next, a `c` after a `-` is the `-c` flag, and a value option (`shell_value_options`)
/// takes the next word when it ends its word. A value option with letters after it in its word is `None`, since its value
/// may be the rest of the word, and so is one that comes before a `c`. `--` ends the options, and a long option that takes
/// a value takes the next word. Every argument after the `-c` is read, which names more commands than the shell runs, never
/// fewer, since an option's value after the `-c` may be among them (`-O shopt_option`).
///
/// `None` when the shell's commands are not the `-c` text, so no name here can read them: a script file, which is the
/// first operand before any `-c` (`sh x.sh -c 'rm'` runs `x.sh`, and the `-c` is one of its arguments), the standard
/// input, which a shell reads when no operand is left after its options (`sh -s`, `sh`, `sh -`, `sh --`), or a `-c` with no
/// command string after its options (`command_string_present`), whose string a wrapper such as `xargs` gives at run time.
fn shell_script_arguments<'a>(program: &str, args: &'a [&'a str]) -> Option<&'a [&'a str]> {
    let (letters, longs) = shell_value_options(program);
    let mut index = 0;
    // bash reads its long options before its short ones, as its `parse_long_options` does: each leading word that
    // `bash_long_option` names is one, and takes the next word as its value when `longs` says so. The first word that is
    // none ends the phase. A single-dash word after it is short letters again, so `-e -rcfile x` has `r`, `c`, `f`, `i`,
    // `l` and `e` as its letters, and its `c` is the `-c` flag.
    if matches!(shell_name(program), "bash" | "rbash") {
        while let Some(&arg) = args.get(index) {
            let Some(name) = bash_long_option(arg) else {
                break;
            };
            index += 1;
            if longs.contains(&name) {
                index += 1;
            }
        }
    }
    while let Some(&arg) = args.get(index) {
        index += 1;
        // An operand is the script file, and a lone `-` or `--` ends the options with no `-c`, so the input is read.
        let Some(sign) = arg.chars().next().filter(|sign| matches!(*sign, '-' | '+')) else {
            return None;
        };
        let body = &arg[1..];
        if arg == "--" || body.is_empty() {
            return None;
        }
        if let Some(long) = arg.strip_prefix("--") {
            if longs.contains(&long) {
                index += 1;
            }
            continue;
        }
        for (at, letter) in body.char_indices() {
            // Only `-` sets an option, so a `c` after `+` is no `-c` flag.
            if sign == '-' && letter == 'c' {
                let rest = &args[index..];
                let after_c = &body[at + letter.len_utf8()..];
                return command_string_present(program, after_c, rest).then_some(rest);
            }
            if letters.contains(letter) {
                // A value option that ends its word takes the next word as its value. With letters after it, it is withheld.
                if at + letter.len_utf8() < body.len() {
                    return None;
                }
                index += 1;
                break;
            }
        }
    }
    None
}

/// Whether the `-c` has its command string on the command line: the first operand that its options leave, among the
/// letters after the `c` in its own word (`after_c`) and the `words` after that word. Each value letter takes the next
/// word wherever it sits in its word, as bash and dash read it, so `-co pipefail`, `-oe pipefail` and `-coo pipefail
/// errexit` each take the words they name. `--` ends the options, and the word after it is the operand; a lone `-` does
/// the same, as bash and dash read it. A lone `+` is no option, and the options go on past it, as bash and dash read
/// it. Without an operand, the string is one a wrapper gives at run time: `xargs` appends its input after its options,
/// as operands, so `echo 'rm -rf x' | xargs -0 sh -c` runs `sh -c 'rm -rf x'`, and the string is withheld.
fn command_string_present(program: &str, after_c: &str, words: &[&str]) -> bool {
    let (letters, longs) = shell_value_options(program);
    // The value letters after the `c` take the first of the words that follow it.
    let mut index = value_letters_in(after_c, letters);
    while let Some(&word) = words.get(index) {
        index += 1;
        // `--` and a lone `-` end the options, so the word after one is the operand, if there is one.
        if word == "--" || word == "-" {
            return index < words.len();
        }
        // A word that is no option is the command string. A lone `+` has no option letter, so it is read past.
        if !word.starts_with(['-', '+']) {
            return true;
        }
        if let Some(long) = word.strip_prefix("--") {
            if longs.contains(&long) {
                index += 1;
            }
            continue;
        }
        index += value_letters_in(&word[1..], letters);
    }
    false
}

/// The number of the letters of `word` that are value letters of its shell (`letters`). Each takes the next word as its
/// value, wherever it sits in its word (`command_string_present`).
fn value_letters_in(word: &str, letters: &str) -> usize {
    word.chars()
        .filter(|letter| letters.contains(*letter))
        .count()
}

/// The arguments PowerShell reads as its command: those after its `-Command` parameter, which PowerShell matches by case
/// and by a leading part of its name (`-c`, `-co`, `-command`). `None` with no such parameter, or with nothing after it.
/// Its other parameters run what they are given too (`-File`, `-EncodedCommand`), so the withheld value covers them
/// (`command_words_names`).
fn powershell_command_arguments<'a>(args: &'a [&'a str]) -> Option<&'a [&'a str]> {
    let at = args.iter().position(|arg| {
        arg.len() >= 2
            && arg.starts_with('-')
            && "-command"
                .get(..arg.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(arg))
    })?;
    let rest = &args[at + 1..];
    (!rest.is_empty()).then_some(rest)
}

/// The segments of a shell text: the pieces between the characters that end one command and start the
/// next. Those are `;`, `|` (and so `||`), `&` (and so `&&`), `(`, `)`, a backtick and a line break. An
/// `&` that is part of a redirection (`2>&1`, `>&2`, `<&3`, `&>file`) ends no command. Quotes are not
/// read here, so a separator inside quotes splits as well: that checks more names, never fewer.
fn shell_segments(text: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    for (index, c) in text.char_indices() {
        let ends_command = match c {
            ';' | '|' | '(' | ')' | '`' | '\n' | '\r' => true,
            '&' => !is_redirection_ampersand(text, index),
            _ => false,
        };
        if ends_command {
            segments.push(&text[start..index]);
            start = index + c.len_utf8();
        }
    }
    segments.push(&text[start..]);
    segments
}

/// Whether the `&` at `index` belongs to a redirection: it follows `<` or `>`, or it is followed by `>`.
fn is_redirection_ampersand(text: &str, index: usize) -> bool {
    let before = text[..index].chars().next_back();
    let after = text[index + 1..].chars().next();
    matches!(before, Some('<' | '>')) || after == Some('>')
}

/// The segments of a shell text cut only where a separator counts: outside quotes and not escaped by a backslash, with
/// the separators of `shell_segments`, except that a `|` right after an unquoted `>` or `>&` is the clobber mark of
/// `>|` or `>&|`, and a `;` there is the sync mark of ksh's `>;`, which no separator is. So a quoted script, which a
/// separator inside its quotes would cut, stays whole, and `command_words_names` reads its word quote-removed. The
/// quotes and backslashes are read as `shell_tokens` reads them. The flag says the text ends inside an open quote, or
/// with a backslash that escapes nothing: the text then
/// goes on past the last segment, which is cut there. It also says the text holds an ANSI-C string (`$'...'`) outside
/// quotes: bash reads its `\'` as an escaped quote, where dash, which has no such string, reads that quote as the end
/// of one. The two readings part there, so the value is withheld rather than read one way. A `#` that begins a word,
/// outside quotes, starts a comment: its text is no command and opens no quote, so it is in no segment, and it runs
/// to the end of its line.
fn quote_aware_segments(text: &str) -> (Vec<&str>, bool) {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut quote: Option<char> = None;
    let mut escaped_at_end = false;
    let mut ansi_c = false;
    // Whether the next character begins a word, outside quotes, and whether a comment is open.
    let mut word_start = true;
    let mut comment = false;
    // Whether the character just read is the `>` of a clobber operator, or the `&` of `>&`, so that a `|` right after it
    // is the operator's mark.
    let mut after_redirect = false;
    let mut chars = text.char_indices();
    while let Some((index, c)) = chars.next() {
        if comment {
            if c != '\n' && c != '\r' {
                continue;
            }
            // The line break ends the comment, and is read as the separator it is.
            comment = false;
            start = index;
        }
        if let Some(open) = quote {
            if c == open {
                quote = None;
            } else if open == '"' && c == '\\' {
                // A backslash in double quotes escapes the character after it.
                if chars.next().is_none() {
                    escaped_at_end = true;
                }
            }
            continue;
        }
        let ends_command = match c {
            '\'' | '"' => {
                quote = Some(c);
                word_start = false;
                false
            }
            '\\' => {
                if chars.next().is_none() {
                    escaped_at_end = true;
                }
                word_start = false;
                false
            }
            '#' if word_start => {
                segments.push(&text[start..index]);
                start = text.len();
                comment = true;
                false
            }
            // The clobber mark of `>|` or `>&|` is part of its redirection, not a pipe: its target is the word after it.
            // So is the sync mark of ksh's `>;` (and of `>>;`, `&>;`): no shell that runs `>;` reads its `;` as a
            // separator.
            '|' | ';' if after_redirect => {
                word_start = false;
                false
            }
            ';' | '|' | '(' | ')' | '`' | '\n' | '\r' => {
                word_start = true;
                true
            }
            '&' => {
                let ends_command = !is_redirection_ampersand(text, index);
                word_start = ends_command;
                ends_command
            }
            ' ' | '\t' => {
                word_start = true;
                false
            }
            '$' if chars.clone().next().is_some_and(|(_, next)| next == '\'') => {
                ansi_c = true;
                word_start = false;
                false
            }
            _ => {
                word_start = false;
                false
            }
        };
        after_redirect = c == '>' || (c == '&' && after_redirect);
        if ends_command {
            segments.push(&text[start..index]);
            start = index + c.len_utf8();
        }
    }
    segments.push(&text[start..]);
    (segments, quote.is_some() || escaped_at_end || ansi_c)
}

/// The text with each command substitution that a `$(` opens, quoted or not, replaced by a bare `$`, and the body of each
/// outermost substitution it replaced, in order: each body is shell text of its own, read by `command_prefixes_at`. A
/// substitution inside double quotes holds quotes that the text around it would misread, so it is lifted out whole. One
/// inside single quotes is literal and stays. A substitution that no `)` closes stays too, and the flag says so: the
/// text then ends inside an open substitution, which withholds its value. A comment is read as in
/// `quote_aware_segments`. One pass over the text, so a long one costs no more than its length. The same pass cuts out
/// each zsh special-hash array that starts a word outside any substitution (`hash_array_at`): its name, its `=` and its `(`
/// up to its `)` are replaced by a space, and its elements are returned with the kind of hash they belong to
/// (`hash_element_names`). Its word and
/// comment state is this pass's, so the array is found wherever the rest of the text is read as a word.
fn lift_substitutions(text: &str) -> (String, Vec<&str>, Vec<(HashKind, String)>, bool) {
    let bytes = text.as_bytes();
    let mut lifted = String::with_capacity(text.len());
    let mut bodies = Vec::new();
    let mut hash_elements = Vec::new();
    let mut hash_open = false;
    let mut copied = 0;
    // The quote open in the innermost level. Each enclosing level keeps its own quote, and, when it is a substitution,
    // the index its body starts at.
    let mut quote: Option<u8> = None;
    let mut enclosing: Vec<(Option<u8>, Option<usize>)> = Vec::new();
    // The outermost open substitution, as the index of its `$` and of its body: a nested one is part of its body.
    let mut outermost: Option<(usize, usize)> = None;
    let mut open_substitutions = 0;
    // Whether the next byte begins a word, outside quotes, and whether a comment is open: as in `quote_aware_segments`,
    // so that a comment's apostrophe opens no quote here either.
    let mut word_start = true;
    let mut comment = false;
    // Whether the byte before is an unquoted, unescaped `>`, or the `&` of `>&`: then a `;` or `|` after it is the mark of
    // its redirection (`>;`, `>|`), and the `#` after the mark starts the target word, not a comment, as in
    // `quote_aware_segments`.
    let mut after_redirect = false;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if comment {
            if byte != b'\n' && byte != b'\r' {
                i += 1;
                continue;
            }
            comment = false;
        }
        let after = std::mem::take(&mut after_redirect);
        if quote == Some(b'\'') {
            if byte == b'\'' {
                quote = None;
            }
        } else if byte == b'\\' {
            // A line continuation is joined out (`join_line_continuations`), so the byte after it reads as the byte before
            // it did: a `;` after a `>` stays the mark of `>;`, and the word state carries over the break.
            match line_break_length(bytes, i + 1) {
                0 => {
                    word_start = false;
                    i += 1;
                }
                break_len => {
                    after_redirect = after;
                    i += break_len;
                }
            }
        } else if byte == b'$' && bytes.get(i + 1) == Some(&b'(') {
            enclosing.push((quote, Some(i + 2)));
            if open_substitutions == 0 {
                outermost = Some((i, i + 2));
            }
            open_substitutions += 1;
            quote = None;
            word_start = true;
            i += 1;
        } else if quote == Some(b'"') {
            if byte == b'"' {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            word_start = false;
        } else if let Some((kind, open_at)) =
            hash_array_at(bytes, i).filter(|_| word_start && open_substitutions == 0)
        {
            // The array is taken out whole, its name and `=` as well as its `(` and its elements: a `(` left in the text would
            // split them into segments of their own, so a value would be read as a command name only where a separator
            // precedes it. A name left behind with its `+=` would be read as a command word (`is_extended_assignment`).
            lifted.push_str(&text[copied..i]);
            lifted.push(' ');
            match array_close(bytes, open_at) {
                Some(close) => {
                    push_hash_elements(&mut hash_elements, kind, &text[open_at + 1..close]);
                    copied = close + 1;
                    i = close;
                }
                None => {
                    push_hash_elements(&mut hash_elements, kind, &text[open_at + 1..]);
                    hash_open = true;
                    copied = bytes.len();
                    break;
                }
            }
            word_start = false;
        } else if byte == b'#' && word_start {
            comment = true;
        } else if byte == b'(' {
            enclosing.push((None, None));
            word_start = true;
        } else if byte == b')' {
            if let Some((outer_quote, body)) = enclosing.pop() {
                quote = outer_quote;
                // A substitution is part of the word it is in, so the text after it begins no word; a subshell ends one.
                word_start = body.is_none();
                if body.is_some() {
                    open_substitutions -= 1;
                    if open_substitutions == 0 {
                        if let Some((dollar, start)) = outermost.take() {
                            lifted.push_str(&text[copied..dollar]);
                            lifted.push('$');
                            bodies.push(&text[start..i]);
                            copied = i + 1;
                        }
                    }
                }
            }
        } else if matches!(byte, b';' | b'|') && after {
            // The mark of `>;` or `>|` is part of its redirection, so the word after it is the target and no word
            // starts there: a `#` after the mark is no comment, as `quote_aware_segments` reads it.
            word_start = false;
        } else {
            after_redirect = byte == b'>' || (byte == b'&' && after);
            word_start = match byte {
                b' ' | b'\t' | b';' | b'|' | b'\n' | b'\r' | b'`' => true,
                b'&' => !is_redirection_ampersand(text, i),
                _ => false,
            };
        }
        i += 1;
    }
    lifted.push_str(&text[copied..]);
    (
        lifted,
        bodies,
        hash_elements,
        hash_open || open_substitutions > 0,
    )
}

/// One token of a shell segment: a word, with its quotes and backslashes removed as the shell removes
/// them, or a redirection operator. The word after a redirection is its target, never a command.
enum ShellToken {
    Word(String),
    Redirect,
}

/// The tokens of one segment. Blanks split words, except inside quotes. An unquoted `<`, `>` or `&>` is
/// a redirection, and a file-descriptor number right before one (`2>`) belongs to it, not to a word. So does the mark of
/// a clobber operator, which ends in `>` or in the `&` of `>&`: a `|` (`>|`, `>&|`) and ksh's `;` (`>;`) in every
/// shell, and, with `zsh_clobber`, a `!` (`>!`, `>&!`), which bash and dash read as the first character of the target
/// instead. The word after the mark is the redirection's target either way. The flag says the segment is cut: it ends
/// inside a quote, or with a backslash that escapes the separator after it, so its text goes on in the next segment,
/// or, as the last segment of a text, the text ends there.
fn shell_tokens(segment: &str, zsh_clobber: bool) -> (Vec<ShellToken>, bool) {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut escaped_at_end = false;
    let mut chars = segment.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            if c == open {
                quote = None;
            } else if open == '"' && c == '\\' {
                word.extend(chars.next());
            } else {
                word.push(c);
            }
            continue;
        }
        let redirect = match c {
            '<' | '>' => true,
            '&' => chars.peek() == Some(&'>'),
            _ => false,
        };
        if redirect {
            let descriptor =
                in_word && !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit());
            if in_word && !descriptor {
                tokens.push(ShellToken::Word(std::mem::take(&mut word)));
            }
            word.clear();
            in_word = false;
            tokens.push(ShellToken::Redirect);
            let mut last = c;
            let mut has_gt = c == '>';
            while let Some(&next) = chars.peek() {
                if !matches!(next, '<' | '>' | '&') {
                    break;
                }
                has_gt |= next == '>';
                last = next;
                chars.next();
            }
            // The operator is a clobber operator when it ends in `>` (`>`, `>>`, `&>`) or in the `&` of `>&`. Its mark is
            // then one operator with no pipe or separator, so the word after it is the redirection's target: `|` and
            // ksh's `;` are the mark in every shell that reads them, and `!` is the mark in zsh alone.
            let clobber = last == '>' || (last == '&' && has_gt);
            if clobber && matches!(chars.peek(), Some(&('|' | ';'))) {
                chars.next();
            } else if zsh_clobber && clobber && chars.peek() == Some(&'!') {
                chars.next();
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
            in_word = true;
        } else if c == '\\' {
            match chars.next() {
                Some(next) => word.push(next),
                None => escaped_at_end = true,
            }
            in_word = true;
        } else if c.is_whitespace() {
            if in_word {
                tokens.push(ShellToken::Word(std::mem::take(&mut word)));
                in_word = false;
            }
        } else {
            word.push(c);
            in_word = true;
        }
    }
    if in_word {
        tokens.push(ShellToken::Word(word));
    }
    (tokens, quote.is_some() || escaped_at_end)
}

/// The command names of one shell segment, read as each of its readings takes it (`segment_readings`): those of
/// its commands (`command_positions`), each the first word that is not a leading word (see `is_leading_word`) and not
/// a redirection's target, and what that command runs (`command_words_names`). Each reading names its own commands, so
/// a command that one reading hides is named by the other. The leading assignments of each reading may define names
/// through an element of a special array (`element_assignment_names`). A leading assignment that only bash and zsh read as
/// one (`extended_assignment_positions`) is the command word in a POSIX shell, so it is read as a command word too.
/// Empty when no reading's command names anything.
fn command_prefix(segment: &str, depth: usize) -> Vec<String> {
    let (readings, cut) = segment_readings(segment);
    let mut names = Vec::new();
    for words in &readings {
        names.extend(element_assignment_names(words, depth, cut));
        for start in command_positions(words)
            .into_iter()
            .chain(extended_assignment_positions(words))
        {
            names.extend(command_words_names(&words[start..], depth, cut));
        }
    }
    distinct(names)
}

/// The words of a segment as each reading of its `!` takes them, each reading once. Bash and dash read a `!` after a
/// clobber operator's `>` as the first character of the redirection's target, so the word after it is a command; zsh
/// reads the `!` as the clobber mark, so the word after it is the target and the command is the one after that
/// (`shell_tokens`). The flag says the segment is cut, which both readings share.
fn segment_readings(segment: &str) -> (Vec<Vec<String>>, bool) {
    let (bash_tokens, cut) = shell_tokens(segment, false);
    let (zsh_tokens, _) = shell_tokens(segment, true);
    let bash = segment_words(bash_tokens);
    let zsh = segment_words(zsh_tokens);
    let readings = if bash == zsh {
        vec![bash]
    } else {
        vec![bash, zsh]
    };
    (readings, cut)
}

/// The positions of a segment's command words, in order: the first word that is not a leading word
/// (`is_leading_word`) and not a name a keyword introduces. A `function` defines its body, and the command is in the
/// body (`function_positions`, as in `function f { rm x; }`). `coproc` may take a NAME before a compound command
/// (`coproc NAME { rm x; }`), which bash reads so, while zsh runs the word after `coproc` as the command: both
/// readings are named, so each is checked.
fn command_positions(words: &[String]) -> Vec<usize> {
    let mut index = 0;
    while let Some(word) = words.get(index) {
        let rest = &words[index + 1..];
        match word.as_str() {
            "function" => return function_positions(words, index),
            "coproc" if coproc_takes_name(rest) => {
                let body = command_positions(&words[index + 2..]);
                return std::iter::once(index + 1)
                    .chain(body.into_iter().map(|at| at + index + 2))
                    .collect();
            }
            "coproc" => index += 1,
            word if is_leading_word(word) => index += 1,
            _ => return vec![index],
        }
    }
    Vec::new()
}

/// The positions of the commands a `function` keyword at `at` defines. The words before a body are the function's
/// names, which run nothing (`function NAME { list }`, and zsh's `function [ -T ] word ... { list }`), so each
/// compound-start word after the keyword may open the body, and the command each one opens is read
/// (`command_positions`). A name may itself be a compound-start word (`function for { rm x; }` names `for`, and its
/// body runs `rm`), so every one is read. A segment with no such word has no body in it, so each word after the
/// keyword that is not a leading word is read as a command, since any of them may be one; with none, the keyword is.
fn function_positions(words: &[String], at: usize) -> Vec<usize> {
    let openers: Vec<usize> = (at + 1..words.len())
        .filter(|&position| is_compound_start(&words[position]))
        .collect();
    if openers.is_empty() {
        let names: Vec<usize> = (at + 1..words.len())
            .filter(|&position| !is_leading_word(&words[position]))
            .collect();
        return if names.is_empty() { vec![at] } else { names };
    }
    openers
        .into_iter()
        .flat_map(|start| {
            command_positions(&words[start..])
                .into_iter()
                .map(move |position| start + position)
        })
        .collect()
}

/// Whether a `function` keyword in the words has no compound-start word after it, so its body lies past the words:
/// a cut segment may hide that body (`may_hide_a_command`).
fn has_function_without_body(words: &[String]) -> bool {
    words.iter().enumerate().any(|(at, word)| {
        word == "function" && !words[at + 1..].iter().any(|word| is_compound_start(word))
    })
}

/// Whether the `coproc` before `rest` takes a NAME: the first word after it is an identifier, and the second opens a
/// compound command (`coproc NAME { rm x; }`). Otherwise the first word after `coproc` is the command.
fn coproc_takes_name(rest: &[String]) -> bool {
    rest.first().is_some_and(|name| is_identifier(name))
        && rest.get(1).is_some_and(|word| is_compound_start(word))
}

/// Whether a cut segment of `text` may hide a command that the readings miss. The plain reading cuts at every
/// separator, one inside quotes included, so a segment cut inside quotes ends where that separator stands. Where
/// the quote-aware reading starts a segment at the same place, the readings agree there, and that quote-aware
/// segment has read the rest of the command whole. Where it does not, the readings are out of step, and a segment
/// whose words may hide a command in either of its readings (`segment_readings`, `may_hide_a_command`) is withheld.
/// A plain command that runs nothing it is given (`make "a;b"`) hides none, so it is not.
fn cut_segment_out_of_step(text: &str, aware: &[&str]) -> bool {
    let base = text.as_ptr() as usize;
    let offset = |segment: &str| segment.as_ptr() as usize - base;
    shell_segments(text).into_iter().any(|segment| {
        // Only a quote or a backslash can cut a segment, so a segment without either is read no further.
        if !segment.contains(['\'', '"', '\\']) {
            return false;
        }
        let (readings, cut) = segment_readings(segment);
        if !cut {
            return false;
        }
        // The quote-aware segments come in order of their place, so the one that starts here is found by search.
        let starts_here = aware
            .binary_search_by_key(&offset(segment), |quote_aware| offset(quote_aware))
            .is_ok();
        !starts_here && readings.iter().any(|words| may_hide_a_command(words))
    })
}

/// Whether a segment's words may hide a command past where the segment is cut: they name no command, or name one
/// that runs what it is given, a wrapper, `find`, `eval`, `trap`, `alias`, `emulate`, `busybox`, `toybox`, a withheld program or
/// a shell (see `command_words_names`). A leading assignment that a POSIX shell reads as the command word is checked as one
/// too (`extended_assignment_positions`).
fn may_hide_a_command(words: &[String]) -> bool {
    let positions = command_positions(words);
    if positions.is_empty() || has_function_without_body(words) {
        return true;
    }
    let mut candidates = positions;
    candidates.extend(extended_assignment_positions(words));
    candidates.into_iter().any(|position| {
        let word = words[position].as_str();
        let base = command_basename(word);
        wrapper_spec(base).is_some()
            || matches!(
                base,
                "eval"
                    | "trap"
                    | "alias"
                    | "emulate"
                    | "busybox"
                    | "toybox"
                    | "find"
                    | "hash"
                    | "."
                    | "source"
            )
            || WITHHELD_COMMANDS.contains(&base)
            || is_posix_shell(word)
            || is_other_shell(word)
    })
}

/// Whether a call of an alias whose value is `value` may run a command of its own, from the words the call appends to
/// the value. Those words follow the value's last command, which may run them (`may_hide_a_command`), or the value ends
/// in no command (a separator, a keyword, an assignment alone), so the call's own first word is one. The value is read
/// as `command_prefixes_at` reads its text, its substitutions lifted and its line continuations joined, and its last
/// segment is found under both segmentations, the plain and the quote-aware.
fn alias_value_hides_a_command(value: &str) -> bool {
    let text = read_expansions_as_substitutions(value);
    let (lifted, _, _, _) = lift_substitutions(&text);
    let joined = join_line_continuations(&lifted);
    for read in [lifted.as_str(), joined.as_str()] {
        let plain = shell_segments(read);
        let (aware, _) = quote_aware_segments(read);
        for last in [plain.last(), aware.last()].into_iter().flatten() {
            let (readings, _) = segment_readings(last);
            if readings.iter().any(|words| may_hide_a_command(words)) {
                return true;
            }
        }
    }
    false
}

/// The kinds of zsh special hash whose array assignment defines names through its elements (`ZSH_NAME_HASHES`).
#[derive(Clone, Copy)]
enum HashKind {
    /// `aliases`, `galiases` and `saliases`: a value is shell text, which a call of its name runs with the words the call
    /// appends, so a value that may run them is withheld (`alias_value_hides_a_command`).
    Alias,
    /// `functions`: a body is shell text, which runs when its name is called.
    Function,
    /// `commands`: a value is the path that a name runs, with the words a call gives it (`bound_path_names`).
    Command,
}

/// The zsh special hashes whose array assignment, `name=(...)` or `name+=(...)`, defines names through its elements. Each
/// element is read as shell text, and the kind says what its hash makes of it (`hash_element_names`).
const ZSH_NAME_HASHES: &[(&str, HashKind)] = &[
    ("aliases", HashKind::Alias),
    ("galiases", HashKind::Alias),
    ("saliases", HashKind::Alias),
    ("functions", HashKind::Function),
    ("commands", HashKind::Command),
];

/// The kind of zsh special hash that an array is assigned to at `start`, and the index of the `(` that opens the array:
/// the bytes at `start` must read `NAME=(` or `NAME+=(` for one of `ZSH_NAME_HASHES`.
fn hash_array_at(bytes: &[u8], start: usize) -> Option<(HashKind, usize)> {
    let rest = &bytes[start..];
    ZSH_NAME_HASHES.iter().find_map(|(name, kind)| {
        let after_name = rest.strip_prefix(name.as_bytes())?;
        let after_op = after_name
            .strip_prefix(b"+=")
            .or_else(|| after_name.strip_prefix(b"="))?;
        after_op
            .starts_with(b"(")
            .then(|| (*kind, start + rest.len() - after_op.len()))
    })
}

/// The index of the `)` that closes the array whose `(` is at `open`: the first unquoted one that no `(` inside the array
/// opens. A `\` escapes the byte after it, and a quote holds its bytes. `None` when the text ends first.
fn array_close(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = open + 1;
    while i < bytes.len() {
        let byte = bytes[i];
        if quote == Some(b'\'') {
            if byte == b'\'' {
                quote = None;
            }
        } else if byte == b'\\' {
            i += 1;
        } else if quote == Some(b'"') {
            if byte == b'"' {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
        i += 1;
    }
    None
}

/// Pushes each word of a hash array's body under its hash's kind, the word with its quotes removed as `shell_tokens` removes
/// them. A line continuation is joined out first.
fn push_hash_elements(elements: &mut Vec<(HashKind, String)>, kind: HashKind, body: &str) {
    let (tokens, _) = shell_tokens(&join_line_continuations(body), false);
    elements.extend(tokens.into_iter().filter_map(|token| match token {
        ShellToken::Word(word) => Some((kind, word)),
        ShellToken::Redirect => None,
    }));
}

/// The length of the line break at `at`: a line feed, or a carriage return and a line feed. Zero when no break is there.
fn line_break_length(bytes: &[u8], at: usize) -> usize {
    match (bytes.get(at), bytes.get(at + 1)) {
        (Some(b'\n'), _) => 1,
        (Some(b'\r'), Some(b'\n')) => 2,
        _ => 0,
    }
}

/// The names that one element of a zsh special hash runs, by the hash's kind (`lift_zsh_hash_arrays`). A body is shell
/// text, read as it is. An alias value is shell text too, and it is withheld where a call's appended words may run.
/// A command's path is bound to its name (`bound_path_names`).
fn hash_element_names(kind: HashKind, element: &str, depth: usize) -> Vec<String> {
    match kind {
        HashKind::Function => command_prefixes_at(element, depth + 1),
        HashKind::Alias => {
            let mut names = command_prefixes_at(element, depth + 1);
            if alias_value_hides_a_command(element) {
                names.push(String::new());
            }
            names
        }
        HashKind::Command => bound_path_names(element, depth),
    }
}

/// The names a command bound to `path` runs. `commands=(NAME PATH)` and `hash NAME=PATH` or `hash -p PATH NAME` bind a name
/// to a path, and a call of the name runs that path with the words the call gives it. So the path is read as shell text, as
/// a command word is (`command_prefixes_at`), and it is withheld where it lies outside the system binary directories, as a
/// run path is, or where its program may run the words a call gives it (`alias_value_hides_a_command`).
fn bound_path_names(path: &str, depth: usize) -> Vec<String> {
    let mut names = command_prefixes_at(path, depth + 1);
    if !is_system_binary_path(path) || alias_value_hides_a_command(path) {
        names.push(String::new());
    }
    names
}

/// The names the `hash` builtin binds (`bound_path_names`): the path of its `-p` option (bash), and the value of each
/// `NAME=PATH` (zsh). Its options are read as getopt reads them (bash's `dlp:rt`): `p` takes a path, which is the rest
/// of its word or the next word when it ends its word, wherever it stands among the option letters, so `-p/bin/rm`,
/// `-rp /bin/rm` and `-lp /bin/rm` each bind a name to `/bin/rm`. `--` ends the options, and after it only a
/// `NAME=PATH` binds. Its other words name no command. A `-p` with no path after it, or a cut segment, may have its path
/// past the cut, so the withheld value is named then.
fn hash_names(args: &[String], depth: usize, cut: bool) -> Vec<String> {
    let mut names = Vec::new();
    let mut path_due = false;
    let mut options_done = false;
    for arg in args {
        let arg = arg.as_str();
        if std::mem::take(&mut path_due) {
            names.extend(bound_path_names(arg, depth));
        } else if !options_done && arg == "--" {
            options_done = true;
        } else if !options_done && arg.starts_with('-') {
            // The letters after the dash are read in order, and `p` takes the rest of its word as its path.
            if let Some(at) = arg[1..].find('p') {
                let path = &arg[at + 2..];
                if path.is_empty() {
                    path_due = true;
                } else {
                    names.extend(bound_path_names(path, depth));
                }
            }
        } else if let Some((_, path)) = arg.split_once('=') {
            names.extend(bound_path_names(path, depth));
        }
    }
    if path_due || cut {
        names.push(String::new());
    }
    names
}

/// The kind of element that a subscripted assignment to the special array `name` defines a name through: bash's
/// `BASH_ALIASES` (its alias list) and `BASH_CMDS` (its command hash), and the zsh special hashes of `ZSH_NAME_HASHES`.
/// `None` for any other name.
fn special_element_kind(name: &str) -> Option<HashKind> {
    match name {
        "BASH_ALIASES" => Some(HashKind::Alias),
        "BASH_CMDS" => Some(HashKind::Command),
        _ => ZSH_NAME_HASHES
            .iter()
            .find(|(hash, _)| *hash == name)
            .map(|(_, kind)| *kind),
    }
}

/// The kind, key and value of a word that assigns one element of a special array (`special_element_kind`):
/// `NAME[key]=value` or `NAME[key]+=value`, with the key running to the first `]`, as `is_assignment` reads it. `None`
/// for any other word.
fn element_assignment(word: &str) -> Option<(HashKind, &str, &str)> {
    let name_len = word
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        .count();
    let kind = special_element_kind(&word[..name_len])?;
    let subscripted = word[name_len..].strip_prefix('[')?;
    let close = subscripted.find(']')?;
    let after = &subscripted[close + 1..];
    let value = after
        .strip_prefix("+=")
        .or_else(|| after.strip_prefix('='))?;
    Some((kind, &subscripted[..close], value))
}

/// The names that the leading assignments of a segment's words define through an element of a special array
/// (`element_assignment`), each read as `element_definition_names` reads it. Only the leading assignments are read
/// (`leading_assignment_positions`).
fn element_assignment_names(words: &[String], depth: usize, cut: bool) -> Vec<String> {
    let mut names = Vec::new();
    for at in leading_assignment_positions(words) {
        if let Some((kind, key, value)) = element_assignment(&words[at]) {
            names.extend(element_definition_names(kind, key, value, depth, cut));
        }
    }
    names
}

/// The names that one element of a special array defines (`element_assignment`): the key is the name the element defines,
/// read as shell text, and the value is read as the element of its kind (`hash_element_names`). A cut segment may hold
/// more of the value past the cut, so the withheld value is named then.
fn element_definition_names(
    kind: HashKind,
    key: &str,
    value: &str,
    depth: usize,
    cut: bool,
) -> Vec<String> {
    let mut names = command_prefixes_at(key, depth + 1);
    names.extend(hash_element_names(kind, value, depth));
    if cut {
        names.push(String::new());
    }
    names
}

/// The positions of the leading assignments among a segment's words: the assignment words that `command_positions` steps
/// past before each command word. A `function` keyword's body, and a `coproc` NAME's, are read from their opening word
/// on, as `function_positions` and `command_positions` read them.
fn leading_assignment_positions(words: &[String]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut index = 0;
    while let Some(word) = words.get(index) {
        let rest = &words[index + 1..];
        match word.as_str() {
            "function" => {
                for start in (index + 1..words.len()).filter(|&at| is_compound_start(&words[at])) {
                    positions.extend(
                        leading_assignment_positions(&words[start..])
                            .into_iter()
                            .map(|at| at + start),
                    );
                }
                return positions;
            }
            "coproc" if coproc_takes_name(rest) => {
                positions.extend(
                    leading_assignment_positions(&words[index + 2..])
                        .into_iter()
                        .map(|at| at + index + 2),
                );
                return positions;
            }
            "coproc" => index += 1,
            word if is_assignment(word) => {
                positions.push(index);
                index += 1;
            }
            word if is_leading_word(word) => index += 1,
            _ => return positions,
        }
    }
    positions
}

/// The positions of the leading assignments that are assignments only in bash's and zsh's sense (`is_extended_assignment`),
/// among those `leading_assignment_positions` finds. A POSIX shell such as dash runs each as the command word, so
/// `command_prefix` reads each one as a command too, and `may_hide_a_command` checks it as one.
fn extended_assignment_positions(words: &[String]) -> Vec<usize> {
    leading_assignment_positions(words)
        .into_iter()
        .filter(|&at| is_extended_assignment(&words[at]))
        .collect()
}

/// Whether a substitution's body holds the word `case`. A `)` that ends a case pattern is no end of the substitution,
/// but `lift_substitutions` takes the first unquoted `)` for one: the commands after it are then read as if they stood
/// outside the substitution, and one of them can go unread. Such a body withholds its value.
fn holds_case_word(body: &str) -> bool {
    body.match_indices("case").any(|(at, word)| {
        let before = body[..at].chars().next_back();
        let after = body[at + word.len()..].chars().next();
        !before.is_some_and(is_name_char) && !after.is_some_and(is_name_char)
    })
}

/// The words of one segment in order, each with its quotes and backslashes removed, but not the target of a
/// redirection: a file, which is neither a command nor an argument.
fn segment_words(tokens: Vec<ShellToken>) -> Vec<String> {
    let mut words = Vec::new();
    let mut target_next = false;
    for token in tokens {
        match token {
            ShellToken::Redirect => target_next = true,
            ShellToken::Word(word) => {
                if !std::mem::take(&mut target_next) {
                    words.push(word);
                }
            }
        }
    }
    words
}

/// How deep the shell reading goes through nested scripts and wrapped commands before it withholds what lies
/// beyond. It is far past any command written by hand, and it bounds the recursion.
const MAX_SHELL_DEPTH: usize = 8;

/// How deep a wrapper's operand is read both ways, an assignment where an operand is due being that operand and also a
/// variable (`wrapped_from`). Each such wrapper doubles the readings, so past this depth the word is withheld instead.
const MAX_HEDGED_DEPTH: usize = 4;

/// A program that runs its arguments as a command, and how it reads its own: which options take no value, which
/// take one, and the operands before its command. Its table is in `WRAPPERS`.
struct Wrapper {
    /// Single-letter options that take no value. A bare `-` is one only where the table lists it (`env`'s is `-i`).
    short_flags: &'static str,
    /// Single-letter options that take a value: the next word, or the rest of the word after the letter.
    short_values: &'static str,
    /// Single-letter options whose value must be attached (`-i{}`): alone, they take none.
    short_optional: &'static str,
    /// Long options that take no value. Their `=` form is one word, with nothing read after it.
    long_flags: &'static [&'static str],
    /// Long options that take a value: the next word, or the `=` form.
    long_values: &'static [&'static str],
    /// Operands before the command that are not commands: `timeout`'s duration. `chrt`'s priority is one only when it
    /// is all digits and more operands follow it (util-linux 2.41), or a decimal number that util-linux before 2.41
    /// reads as one (`wrapped_command_names`). After the last operand no option is read, since getopt's `+` ends the
    /// options at the first non-option (`command_after_operands`).
    operands_before_command: usize,
    /// The command run when none is given: `xargs` runs `echo`. `Some("")` names the withheld value instead, for a
    /// program whose default is a shell that reads its commands from stdin (`fakeroot`, `chroot`).
    default_command: Option<&'static str>,
}

/// The wrapper programs, by the basename of their command word, with the options each one's manual gives it. An
/// option a table leaves out is withheld (`wrapped_command_names`), so a short table fails closed, never open.
const WRAPPERS: &[(&str, Wrapper)] = &[
    (
        "env",
        Wrapper {
            // A bare `-` is GNU env's `-i`, so it is a flag here and in no other table.
            short_flags: "i0v-",
            short_values: "uC",
            short_optional: "",
            long_flags: &[
                "ignore-environment",
                "null",
                "debug",
                "help",
                "version",
                "block-signal",
                "default-signal",
                "ignore-signal",
            ],
            long_values: &["unset", "chdir"],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        // A digit is the old `-N` adjustment, given in its own word.
        "nice",
        Wrapper {
            short_flags: "0123456789",
            short_values: "n",
            short_optional: "",
            long_flags: &["help", "version"],
            long_values: &["adjustment"],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        // The duration is the operand before the command. The words after it are the command, and none is read as an
        // option, since getopt's `+` ends the options at the first non-option.
        "timeout",
        Wrapper {
            short_flags: "v",
            short_values: "sk",
            short_optional: "",
            long_flags: &[
                "foreground",
                "preserve-status",
                "verbose",
                "help",
                "version",
            ],
            long_values: &["signal", "kill-after"],
            operands_before_command: 1,
            default_command: None,
        },
    ),
    (
        "xargs",
        Wrapper {
            short_flags: "0prtxo",
            short_values: "adEILnPs",
            short_optional: "eil",
            long_flags: &[
                "null",
                "no-run-if-empty",
                "interactive",
                "verbose",
                "exit",
                "show-limits",
                "open-tty",
                "help",
                "version",
                "eof",
                "max-lines",
                "replace",
            ],
            long_values: &[
                "arg-file",
                "delimiter",
                "max-args",
                "max-procs",
                "max-chars",
                "process-slot-var",
            ],
            operands_before_command: 0,
            default_command: Some("echo"),
        },
    ),
    (
        // `-e` (sudoedit), `-h` (help), `-i` (`--login`: the command becomes shell text) and `-s` (a shell given the
        // words) are left out, so their words are withheld.
        "sudo",
        Wrapper {
            short_flags: "AbBEHKklLnPSVv",
            short_values: "CDgpRrtTUu",
            short_optional: "",
            long_flags: &[
                "askpass",
                "background",
                "bell",
                "non-interactive",
                "preserve-env",
                "preserve-groups",
                "reset-timestamp",
                "remove-timestamp",
                "set-home",
                "stdin",
                "validate",
                "version",
                "list",
                "no-update",
            ],
            long_values: &[
                "close-from",
                "chdir",
                "chroot",
                "group",
                "other-user",
                "prompt",
                "role",
                "type",
                "command-timeout",
                "user",
            ],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        "nohup",
        Wrapper {
            short_flags: "",
            short_values: "",
            short_optional: "",
            long_flags: &["help", "version"],
            long_values: &[],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        "exec",
        Wrapper {
            short_flags: "cl",
            short_values: "a",
            short_optional: "",
            long_flags: &[],
            long_values: &[],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        "command",
        Wrapper {
            short_flags: "pvV",
            short_values: "",
            short_optional: "",
            long_flags: &[],
            long_values: &[],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        // `builtin NAME args` runs the shell builtin NAME with its arguments, which are read as `eval`, `trap` and `exec`
        // read theirs. It takes no option of its own, so an option is withheld.
        "builtin",
        Wrapper {
            short_flags: "",
            short_values: "",
            short_optional: "",
            long_flags: &[],
            long_values: &[],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        "time",
        Wrapper {
            short_flags: "apqv",
            short_values: "fo",
            short_optional: "",
            long_flags: &[
                "append",
                "portability",
                "quiet",
                "verbose",
                "help",
                "version",
            ],
            long_values: &["format", "output"],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        // zsh's precommand modifiers run the command after them and take no options of their own, so an option is
        // withheld.
        "noglob",
        Wrapper {
            short_flags: "",
            short_values: "",
            short_optional: "",
            long_flags: &[],
            long_values: &[],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        "nocorrect",
        Wrapper {
            short_flags: "",
            short_values: "",
            short_optional: "",
            long_flags: &[],
            long_values: &[],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        // zsh's `repeat COUNT sublist` runs the sublist after its count, and `repeat COUNT do list done` runs its list.
        // The reserved words that open a sublist run no command, so the command is read past them (`list_names`).
        // `repeat` takes no option, so an option after the count is withheld.
        "repeat",
        Wrapper {
            short_flags: "",
            short_values: "",
            short_optional: "",
            long_flags: &[],
            long_values: &[],
            operands_before_command: 1,
            default_command: None,
        },
    ),
    (
        "setsid",
        Wrapper {
            short_flags: "cfwVh",
            short_values: "",
            short_optional: "",
            long_flags: &["ctty", "fork", "wait", "version", "help"],
            long_values: &[],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        "stdbuf",
        Wrapper {
            short_flags: "",
            short_values: "ioe",
            short_optional: "",
            long_flags: &["help", "version"],
            long_values: &["input", "output", "error"],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        // `-p`, `-P` and `-u` name the processes the call acts on, so they are left out and withhold the command.
        "ionice",
        Wrapper {
            short_flags: "tVh",
            short_values: "cn",
            short_optional: "",
            long_flags: &["ignore", "version", "help"],
            long_values: &["class", "classdata"],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        // The priority is the operand before the command. util-linux 2.41 and later read it as the priority only when
        // it is all digits and more operands follow it, and any other word as the command, which `-o`, `-b`, `-i` and
        // `-d` run (the other policies reject it). Before 2.41 a decimal number such as `+10` is the priority too, so
        // such a word is read both ways (`wrapped_command_names`). The words after the priority are the command and its
        // arguments, none read as an option, since chrt's getopt starts with `+`. `-p` names an existing process, and runs
        // no command.
        "chrt",
        Wrapper {
            short_flags: "abdfimoprRvhV",
            short_values: "TPD",
            short_optional: "",
            long_flags: &[
                "all-tasks",
                "batch",
                "deadline",
                "fifo",
                "idle",
                "max",
                "other",
                "pid",
                "rr",
                "reset-on-fork",
                "verbose",
                "help",
                "version",
            ],
            long_values: &["sched-runtime", "sched-period", "sched-deadline"],
            operands_before_command: 1,
            default_command: None,
        },
    ),
    (
        // The CPU mask or list is the operand before the command, and the words after it are the command, none read as an
        // option (getopt's `+`). `-p` names an existing process, and runs no command.
        "taskset",
        Wrapper {
            short_flags: "acpVh",
            short_values: "",
            short_optional: "",
            long_flags: &["all-tasks", "cpu-list", "pid", "help", "version"],
            long_values: &[],
            operands_before_command: 1,
            default_command: None,
        },
    ),
    (
        // The lock file is the operand before the command. `-c` runs its text through a shell, so it is left out.
        // `-e` is an alias of `-x`, so it takes no value. Its synopsis `flock FILE -c command` puts an option after the
        // lock file, so an option-shaped word there is read both as the command and as an option (`operand_names`).
        "flock",
        Wrapper {
            short_flags: "sxeunoFvVh",
            short_values: "wE",
            short_optional: "",
            long_flags: &[
                "shared",
                "exclusive",
                "unlock",
                "nb",
                "nonblock",
                "close",
                "no-fork",
                "verbose",
                "version",
                "help",
            ],
            long_values: &["timeout", "conflict-exit-code"],
            operands_before_command: 1,
            default_command: None,
        },
    ),
    (
        // `-s` runs a shell over the words it is given, so it is left out.
        "doas",
        Wrapper {
            short_flags: "n",
            short_values: "aCu",
            short_optional: "",
            long_flags: &[],
            long_values: &[],
            operands_before_command: 0,
            default_command: None,
        },
    ),
    (
        // coreutils' `chroot` takes long options only, and its NEWROOT is the operand before the command. With no command
        // it runs `$SHELL -i`, which reads stdin, so the withheld value is its default, as for `fakeroot`.
        "chroot",
        Wrapper {
            short_flags: "",
            short_values: "",
            short_optional: "",
            long_flags: &["skip-chdir", "help", "version"],
            long_values: &["userspec", "groups"],
            operands_before_command: 1,
            default_command: Some(""),
        },
    ),
    (
        // `eatmydata COMMAND [ARG]...` runs its command with the library preloaded and takes no option of its own, so
        // its first word is the command, and an option is withheld.
        "eatmydata",
        Wrapper {
            short_flags: "",
            short_values: "",
            short_optional: "",
            long_flags: &[],
            long_values: &[],
            operands_before_command: 0,
            default_command: None,
        },
    ),
];

/// The programs whose command is withheld, because each runs the command it is given in a form no table reads. `ssh`
/// runs its remote command as shell text; `su`, `runuser` and `script` run a `-c` text; `watch` and `parallel` take a
/// shell text or a template; `gdb` runs the gdb commands of `-ex`, `-x` and its stdin; `screen`, `tmux` and `perf` run
/// their command after subcommands or options too many or too varied to table. The others have options too many or
/// too varied to table, or take operands a table cannot count (`firejail`, `bwrap`, `run0`, `dbus-run-session`,
/// `faketime`, `torsocks`, `proxychains`, `chpst`). A variant of a program is withheld too, since its options are not
/// read (`fakeroot-sysv`, `fakeroot-tcp`). A program that runs a command under a subcommand
/// (`npm run`, `npx`, `pnpm exec`, `uv run`, `bundle exec`, `cargo run`, `go run`, `kubectl exec`, `docker exec`,
/// `podman exec`, `ip netns exec`) is withheld whole: the global options before the subcommand are too varied to read,
/// so the subcommand cannot be found reliably, and `npm install` is withheld with `npm run`. A program that reads its
/// command from a file or stdin (`at`, `batch`, `expect`) is withheld too, and so are `newgrp`, which starts a shell that
/// reads its commands from stdin, and `arch-chroot`, which runs a shell or the command it is given in a new root. The
/// option tables of `fakeroot`, `fakechroot`, `cgexec` and `xvfb-run` are not verified on Linux, so those are withheld
/// whole. So are the container init and privilege-drop helpers (`tini`, `dumb-init`, `gosu`, `su-exec`), the package
/// runners, version managers and project runners that run a command under a subcommand or a file (`uvx`, `pipx`, `bunx`,
/// `deno`, `mise`, `hatch`, `tox`), and the container, cluster and machine tools that run one (`nerdctl`, `oc`, `incus`,
/// `machinectl`, `srun`). Such a program names itself and the withheld value, which covers every command
/// (`command_words_names`).
const WITHHELD_COMMANDS: &[&str] = &[
    "ssh",
    "su",
    "runuser",
    "script",
    "watch",
    "parallel",
    "unshare",
    "nsenter",
    "strace",
    "ltrace",
    "valgrind",
    "setpriv",
    "prlimit",
    "numactl",
    "runcon",
    "pkexec",
    "unbuffer",
    "sg",
    "sshpass",
    "systemd-run",
    "gdb",
    "gdb-multiarch",
    "screen",
    "tmux",
    "perf",
    "firejail",
    "bwrap",
    "run0",
    "dbus-run-session",
    "faketime",
    "torsocks",
    "proxychains",
    "proxychains4",
    "chpst",
    "fakeroot-sysv",
    "fakeroot-tcp",
    "ssh-agent",
    "setarch",
    "linux32",
    "linux64",
    // util-linux installs setarch under the names of the architectures it sets as well, and each of those runs the
    // command it is given (`x86_64 rm x`), so each is withheld too.
    "i386",
    "i486",
    "i586",
    "i686",
    "x86_64",
    "uname26",
    "ppc",
    "ppc32",
    "ppc64",
    "ppc64le",
    "s390",
    "s390x",
    "sparc",
    "sparc32",
    "sparc32bash",
    "sparc64",
    "sparc64bash",
    "mips",
    "mips64",
    "hppa",
    "hppa64",
    "parisc",
    "parisc64",
    "armv7l",
    "aarch64",
    "ia64",
    "alpha",
    "systemd-inhibit",
    "chronic",
    "ifne",
    "daemonize",
    "dbus-launch",
    "capsh",
    "softlimit",
    "setuidgid",
    "envdir",
    "envuidgid",
    "setlock",
    "pgrphack",
    "catchsegv",
    "schroot",
    "lxc-attach",
    "distrobox",
    "flatpak-spawn",
    "systemd-nspawn",
    "proot",
    "start-stop-daemon",
    "apptainer",
    "singularity",
    "hyperfine",
    "entr",
    "tsocks",
    "torify",
    "trickle",
    "sem",
    "nocache",
    "rr",
    "xterm",
    "at",
    "batch",
    "expect",
    "nix-shell",
    "npx",
    "npm",
    "pnpm",
    "yarn",
    "uv",
    "poetry",
    "pipenv",
    "conda",
    "bundle",
    "direnv",
    "cargo",
    "go",
    "kubectl",
    "docker",
    "podman",
    "docker-compose",
    "ip",
    "newgrp",
    "arch-chroot",
    // The option tables of these are not verified on Linux, so the programs are withheld whole (see above).
    "fakeroot",
    "fakechroot",
    "cgexec",
    "xvfb-run",
    // Container init and privilege-drop helpers, which run the command they are given.
    "tini",
    "dumb-init",
    "gosu",
    "su-exec",
    // Package runners, version managers and project runners, which run a command under a subcommand or a file.
    "uvx",
    "pipx",
    "bunx",
    "bun",
    "pnpx",
    "deno",
    "corepack",
    "composer",
    "mise",
    "asdf",
    "rbenv",
    "pyenv",
    "nvm",
    "hatch",
    "pdm",
    "rye",
    "tox",
    "nox",
    "toolbox",
    // Container, cluster and machine tools, which run a command in a container, pod, job or machine.
    "nerdctl",
    "crictl",
    "buildah",
    "oc",
    "incus",
    "lxc",
    "machinectl",
    "systemd-cat",
    "srun",
    "mpirun",
    "mpiexec",
    "flatpak",
    "nsjail",
    "sandbox-exec",
    // Other programs that run the command they are given.
    "nix",
    "mosh",
    "run-parts",
    "cpulimit",
    "multitime",
    "watchexec",
];

/// The wrapper program a command word names, from `WRAPPERS`.
fn wrapper_spec(base: &str) -> Option<&'static Wrapper> {
    WRAPPERS
        .iter()
        .find(|(name, _)| *name == base)
        .map(|(_, wrapper)| wrapper)
}

/// Whether a wrapper reads a NAME=VALUE word as a variable set for its command: `env` and `sudo` do, and zsh's
/// `nocorrect`, a reserved word whose assignments the shell reads as its own. Every other wrapper runs its command with
/// `execvp`, so such a word is the command itself, run by that path when it holds a slash (`wrapped_from`). zsh's
/// `noglob` is a builtin, not a reserved word, so its word is read both ways instead (`wrapped_from`). `env` reads a word
/// with an `=` as a variable set whatever its name, unless it holds an expansion, and `sudo` reads a word with an `=` that
/// no shell reads as an assignment both ways (`wrapped_from`).
fn reads_assignments(program: &str) -> bool {
    matches!(program, "env" | "sudo" | "nocorrect")
}

/// How an option word takes its value: nothing, the next word (`-n 10`), or the rest of its own word (`-n10`).
#[derive(Clone, Copy)]
enum Arity {
    Flag,
    NextWord,
    Attached,
}

/// How a wrapper's option word takes its value, from the wrapper's table. `None` for an option the table does not
/// list, which may take a value that the command is then read past.
fn option_arity(wrapper: &Wrapper, word: &str) -> Option<Arity> {
    // A bare `-` is an option only where the table lists it, as GNU env's `-` is `-i`. Elsewhere getopt reads it as an
    // operand, which no table counts, so it is withheld like an option the table leaves out.
    if word == "-" {
        return wrapper.short_flags.contains('-').then_some(Arity::Flag);
    }
    if let Some(long) = word.strip_prefix("--") {
        let (name, attached) = match long.split_once('=') {
            Some((name, _)) => (name, true),
            None => (long, false),
        };
        return if wrapper.long_values.contains(&name) {
            Some(if attached {
                Arity::Attached
            } else {
                Arity::NextWord
            })
        } else if wrapper.long_flags.contains(&name) {
            Some(if attached {
                Arity::Attached
            } else {
                Arity::Flag
            })
        } else {
            None
        };
    }
    // The letters are read in order, as getopt reads them: a flag leads to the next letter, and the first letter
    // that takes a value ends the word, its value being the rest of the word or the next word.
    let letters = &word[1..];
    for (at, letter) in letters.char_indices() {
        if wrapper.short_flags.contains(letter) {
            continue;
        }
        let last = at + letter.len_utf8() == letters.len();
        return if wrapper.short_values.contains(letter) {
            Some(if last {
                Arity::NextWord
            } else {
                Arity::Attached
            })
        } else if wrapper.short_optional.contains(letter) {
            Some(if last { Arity::Flag } else { Arity::Attached })
        } else {
            None
        };
    }
    Some(Arity::Flag)
}

/// The command names a wrapper runs, read from the words it is given (`args`, after its own name). Its options are
/// read through its table: a flag or an attached value takes one word, a value takes the next word too, and `--`
/// ends them. Each `NAME=VALUE` before the command sets a variable for it and names none, where `reads_assignments`
/// says so; any other wrapper runs it as the command, as `execvp` does, and zsh's `noglob` reads it both ways
/// (`wrapped_from`). The operands the table lists come next (`timeout`'s duration) and are skipped, but `chrt`'s
/// priority is skipped only when it is all digits and more operands follow it (`operand_names`): any other word there is
/// the command. A decimal number that is not all digits (`is_decimal_number`) is also the priority on util-linux before
/// 2.41, which runs the word after it, so that word is named too. The word after the last operand is the command, with no
/// option read there (`command_after_operands`). Without one, a cut segment's command may lie past the cut, so the
/// withheld value is named; otherwise the wrapper's default command, if it has one, runs. `program` is the wrapper's
/// name: zsh's `repeat` is one whose count is followed by the list it repeats, which `list_names` reads.
fn wrapped_command_names(
    wrapper: &Wrapper,
    args: &[String],
    depth: usize,
    cut: bool,
    program: &str,
) -> Vec<String> {
    wrapped_from(
        wrapper,
        args,
        wrapper.operands_before_command,
        false,
        depth,
        cut,
        program,
    )
}

/// `wrapped_command_names` from the point where `operands_to_skip` operands are still due, with `options_done` saying
/// whether `--` has ended the options. An assignment-shaped word where an operand is due is read both ways: as that
/// operand (`operand_names`), which zsh's `repeat` takes as its count even when it is an assignment (`repeat X=1 rm x`
/// runs `rm x`), and which `flock` takes as its lock file (`flock X=1 rm x` runs `rm x`); and as a variable set for the
/// command, which the loop reads on past it. The operand reading of the first such word covers the ones after it, since
/// each leaves the same words to read, except for `chrt`, which reads each word as the command it is, so it takes each.
fn wrapped_from(
    wrapper: &Wrapper,
    args: &[String],
    operands_to_skip: usize,
    mut options_done: bool,
    depth: usize,
    cut: bool,
    program: &str,
) -> Vec<String> {
    let mut names = Vec::new();
    let mut operand_read = false;
    // The replace strings `xargs` is given: it puts each input line in place of each one in the command it runs.
    let mut replace_strings: Vec<String> = Vec::new();
    let mut index = 0;
    while let Some(word) = args.get(index) {
        let word = word.as_str();
        index += 1;
        if !options_done && word == "--" {
            options_done = true;
        } else if !options_done && word.starts_with('-') {
            if program == "xargs" {
                replace_strings.extend(xargs_replace_string(wrapper, word, args.get(index)));
            }
            match option_arity(wrapper, word) {
                Some(Arity::Flag | Arity::Attached) => {}
                Some(Arity::NextWord) => index += 1,
                // An option the table does not list may take a value, so the command after it cannot be read.
                None => {
                    names.push(String::new());
                    return names;
                }
            }
        } else if operands_to_skip > 0 && is_assignment(word) {
            // Past `MAX_HEDGED_DEPTH` nested wrappers the word is withheld rather than read both ways, which bounds the readings.
            if depth > MAX_HEDGED_DEPTH {
                names.push(String::new());
                return names;
            }
            if program == "chrt" || !operand_read {
                names.extend(operand_names(
                    wrapper,
                    &args[index - 1..],
                    operands_to_skip,
                    options_done,
                    depth,
                    cut,
                    program,
                ));
                operand_read = true;
            }
        } else if is_assignment(word) && reads_assignments(program) {
            // A variable set for the command names no command.
        } else if program == "env" && word.contains('=') && !is_expanding_word(word) {
            // GNU env reads any word with an `=` before its command as a variable set, whatever the name before the `=`
            // (`env 1=x rm` sets the variable `1` and runs `rm`), so it names no command. A word with an expansion may
            // split into a command, so it is not read this way: it is read as the command it may be, which
            // `command_names` withholds.
        } else if program == "sudo" && word.contains('=') && !is_expanding_word(word) {
            // sudo's reading of a word that no shell reads as an assignment is not verified here, so the word is read as
            // the command it may be too. The loop reads on past it as a variable set, as `noglob`'s word is.
            names.extend(command_words_names(&args[index - 1..], depth, cut));
        } else if is_assignment(word) && program == "noglob" {
            // zsh's `noglob` is a builtin, so the word after it may be the command it runs (`noglob A=/bin/rm x` runs
            // the path `A=/bin/rm`), or a variable set for the command after it. The word is read here as the command, and
            // the loop reads on past it as a variable set.
            names.extend(command_words_names(&args[index - 1..], depth, cut));
        } else if operands_to_skip > 0 {
            names.extend(operand_names(
                wrapper,
                &args[index - 1..],
                operands_to_skip,
                options_done,
                depth,
                cut,
                program,
            ));
            return names;
        } else {
            // The words of the command are read with its run-time substitutions in place (`substituted`).
            let command = substituted(&args[index - 1..], &replace_strings);
            names.extend(command_words_names(&command, depth, cut));
            return names;
        }
    }
    if cut {
        names.push(String::new());
    } else {
        names.extend(wrapper.default_command.map(str::to_owned));
    }
    names
}

/// The names read when `args[0]` is the word an operand is due for, with `operands_to_skip` operands due in all and
/// `options_done` as `wrapped_from` has it. `chrt` takes its priority only when the word is all digits and more operands
/// follow it: any other word is the command, which the policies that take no priority run, and a decimal number that is
/// not all digits is the priority on util-linux before 2.41, which runs the word after it, so that word is named too. A
/// lone all-digit word is the command on util-linux 2.41, so it is named, and a cut may hide the operands that would make
/// it the priority, so the withheld value is named too. The list after a `repeat` count is read from its first word on
/// (`list_names`), since the reserved words that open it run no command. Any other operand is read past, and the words
/// after it are read with one operand fewer. The last operand ends them: the words after it are the command and its
/// arguments, none read as an option (`command_after_operands`).
fn operand_names(
    wrapper: &Wrapper,
    args: &[String],
    operands_to_skip: usize,
    options_done: bool,
    depth: usize,
    cut: bool,
    program: &str,
) -> Vec<String> {
    let Some((word, rest)) = args.split_first() else {
        return Vec::new();
    };
    if program == "chrt" {
        if !is_digits(word) {
            let mut names = command_words_names(args, depth, cut);
            if is_decimal_number(word) {
                // On util-linux before 2.41 the word is the priority, and the word after it is the command.
                names.extend(command_after_operands(wrapper, rest, depth, cut));
            }
            return names;
        }
        if rest.is_empty() {
            // No operand follows an all-digit word, so util-linux 2.41 runs it as the command.
            let mut names = command_words_names(args, depth, cut);
            if cut {
                names.push(String::new());
            }
            return names;
        }
    }
    if program == "repeat" && operands_to_skip == 1 {
        return list_names(rest, depth, cut);
    }
    if operands_to_skip > 1 {
        return wrapped_from(
            wrapper,
            rest,
            operands_to_skip - 1,
            options_done,
            depth,
            cut,
            program,
        );
    }
    // The last operand. For chrt that is an all-digit priority with more operands after it. getopt's `+` ends the options
    // at the first non-option, so the words after it are the command and its arguments, none read as an option.
    let mut names = command_after_operands(wrapper, rest, depth, cut);
    // flock's getopt reads options after its lock file too (`flock FILE -c command`), so an option-shaped word there is
    // read as an option as well, and the command after those options is named too.
    if program == "flock" && rest.first().is_some_and(|first| first.starts_with('-')) {
        names.extend(wrapped_from(wrapper, rest, 0, false, depth, cut, program));
    }
    names
}

/// The names of the command a wrapper runs, read from the words after its last operand, where no option is read: getopt's
/// `+` ends the options at the first non-option, so an option-shaped word, a `--` and a NAME=VALUE word are each the
/// command as written (`command_words_names`). With no word after the operand, the command is the wrapper's default, or
/// the withheld value where a cut may put one past the cut.
fn command_after_operands(
    wrapper: &Wrapper,
    words: &[String],
    depth: usize,
    cut: bool,
) -> Vec<String> {
    if words.is_empty() {
        return if cut {
            vec![String::new()]
        } else {
            wrapper
                .default_command
                .map(str::to_owned)
                .into_iter()
                .collect()
        };
    }
    command_words_names(words, depth, cut)
}

/// The replace string an `xargs` option word sets, if it sets one. `-I STR` takes the next word (`next`) when `I` ends its
/// word, and the rest of the word otherwise (`-ISTR`); `-i` takes the rest of its word, with `{}` when that is none (`-i`,
/// `-iSTR`); `--replace` and `--replace=STR` are the long forms. The first letter that is no flag decides, as `wrapper`'s
/// table reads the word: a letter that takes a value before `I` or `i` ends it, so `-nI` gives `I` as the number.
fn xargs_replace_string(wrapper: &Wrapper, word: &str, next: Option<&String>) -> Option<String> {
    if let Some(long) = word.strip_prefix("--") {
        return match long.split_once('=') {
            Some(("replace", value)) => Some(value.to_owned()),
            None if long == "replace" => Some("{}".to_owned()),
            _ => None,
        };
    }
    let letters = &word[1..];
    for (at, letter) in letters.char_indices() {
        if wrapper.short_flags.contains(letter) {
            continue;
        }
        let value = &letters[at + letter.len_utf8()..];
        return match letter {
            'I' if value.is_empty() => next.cloned(),
            'I' => Some(value.to_owned()),
            'i' if value.is_empty() => Some("{}".to_owned()),
            'i' => Some(value.to_owned()),
            _ => None,
        };
    }
    None
}

/// The words of a command that a run-time substitution may change: each word that holds one of `strings`, the replace
/// string `xargs` substitutes or the `{}` `find` does, becomes `$(`, an open substitution. Where such a word is run as a
/// command, or read as shell text, what it runs or says is withheld, since `$(` is an expansion or leaves a substitution
/// open (`command_names`, `command_prefixes_at`). Where it is only an argument of a plain command, it names nothing. The
/// whole word is replaced, since a substitution inside quotes would be read as literal text.
fn substituted<S: AsRef<str>>(words: &[String], strings: &[S]) -> Vec<String> {
    words
        .iter()
        .map(|word| {
            if strings.iter().any(|string| word.contains(string.as_ref())) {
                "$(".to_owned()
            } else {
                word.clone()
            }
        })
        .collect()
}

/// The names the list of a zsh `repeat` runs, read as a segment's words are (`command_positions`), so the keywords the
/// list opens with (`!`, `{`, `do`, `time`, `coproc`, ...) are read past. An option where the command stands is
/// withheld instead: `repeat` takes none, so that word names a command that no table reads. The options of a
/// `function` header are read by `function_positions`, not withheld. A cut segment whose list has no command in its
/// words may have that command past the cut, so the list is withheld.
fn list_names(words: &[String], depth: usize, cut: bool) -> Vec<String> {
    // The keywords the list opens with, and the assignments before its command, run no command and are read past.
    // `coproc` is one too, so an option after it is the command's and is withheld.
    let keywords = words
        .iter()
        .take_while(|word| {
            word.as_str() == "coproc" || (is_leading_word(word) && !word.starts_with('-'))
        })
        .count();
    if words
        .get(keywords)
        .is_some_and(|word| word.starts_with('-'))
    {
        return vec![String::new()];
    }
    let positions = command_positions(words);
    if positions.is_empty() && cut {
        return vec![String::new()];
    }
    positions
        .into_iter()
        .flat_map(|at| command_words_names(&words[at..], depth, cut))
        .collect()
}

/// The command names a command runs, read from its words: `words[0]` is the command word, quote-removed, and the
/// rest are its arguments. The word's own name is named (`command_names`). Then, by what the word is: a wrapper runs
/// the command it is given (`wrapped_command_names`); `find` runs the command of each `-exec` predicate
/// (`find_command_names`); a program in `WITHHELD_COMMANDS` runs a command no table reads, so its value is withheld;
/// a program in `OPTION_VALUE_COMMANDS` runs the command its option or address gives (`option_value_command_names`);
/// `eval` runs its arguments joined, as shell text; `trap` runs its action as shell text (`trap_action`); `alias` reads
/// the value of each NAME=VALUE operand as shell text; `emulate` runs the text after each `-c` option as shell text
/// (`emulate_script_names`), and is withheld where an expanding word may stand for that option (`emulate_option_may_be_hidden`);
/// `busybox` and `toybox` run the applet they are given; a shell
/// reads its `-c` script, or, when its commands come from a script file or its standard input, its value is withheld
/// (`shell_script_arguments`), and the shells in `OTHER_SHELLS` also withhold it (`is_other_shell`). `hash` names the paths it
/// binds (`hash_names`), and the declaring builtins (`typeset`, `declare`, ...) name the element that each `NAME[KEY]=VALUE`
/// operand defines (`element_definition_names`). `.` or `source` runs the file it is given with the words after it, which
/// are read as a run by that path is (`command_words_names` one level deeper), and the file is withheld where it is no
/// system binary. `cut` says the segment goes on past these words.
fn command_words_names(words: &[String], depth: usize, cut: bool) -> Vec<String> {
    let Some((word, args)) = words.split_first() else {
        return Vec::new();
    };
    if depth > MAX_SHELL_DEPTH {
        return vec![String::new()];
    }
    let mut names = command_names(word);
    if is_expanding_word(word) {
        return names;
    }
    let base = command_basename(word);
    if let Some(wrapper) = wrapper_spec(base) {
        names.extend(wrapped_command_names(wrapper, args, depth + 1, cut, base));
    } else if base == "find" {
        names.extend(find_command_names(args, depth + 1, cut));
    } else if WITHHELD_COMMANDS.contains(&base) {
        names.push(String::new());
    } else if OPTION_VALUE_COMMANDS.contains(&base) {
        names.extend(option_value_command_names(base, args, depth + 1, cut));
    } else if base == "eval" {
        names.extend(command_prefixes_at(&args.join(" "), depth + 1));
        // The text `eval` runs is its joined arguments, and those go on past a cut.
        if cut {
            names.push(String::new());
        }
    } else if base == "trap" {
        // The action runs as shell text when the signal comes. A cut segment may end inside it, so it is withheld as
        // `eval`'s is.
        if let Some(action) = trap_action(args) {
            names.extend(command_prefixes_at(action, depth + 1));
        }
        if cut {
            names.push(String::new());
        }
    } else if base == "alias" {
        // zsh expands an alias in a `-c` script, and bash does after `shopt -s expand_aliases`, so the command an alias
        // runs is the text it defines: the value of each NAME=VALUE operand is read as shell text, as `eval`'s arguments
        // are. alias takes no option that carries a `=`, so every operand that has one is read, after `--` or with a
        // leading `-` too. A call of the alias appends its words to the value, so a value that may run them, or that ends
        // in no command, is withheld (`alias_value_hides_a_command`).
        for arg in args {
            if let Some((_, value)) = arg.split_once('=') {
                names.extend(command_prefixes_at(value, depth + 1));
                if alias_value_hides_a_command(value) {
                    names.push(String::new());
                }
            } else if is_expanding_word(arg) {
                // The shell expands the operand before `alias` reads it, so it can be a NAME=VALUE no name here gives:
                // it is withheld, as `eval`'s expanding text is.
                names.push(String::new());
            }
        }
        // A cut segment may end inside an operand, which goes on past the cut, so it is withheld, as `eval`'s are.
        if cut {
            names.push(String::new());
        }
    } else if base == "emulate" {
        // zsh's `emulate [-LR] SHELL -c ARG` evaluates ARG as shell text, as `eval` evaluates its arguments: the text is one of
        // the words after each option that holds a `c` (`emulate_script_names`). A cut segment may have its option, or the text
        // after it, past the cut, so the withheld value is named then, as `eval`'s is.
        names.extend(emulate_script_names(args, depth));
        // An expanding word that may stand for the option, or for the words that carry it, is expanded before `emulate`
        // reads it, so the text it runs is withheld then (`emulate_option_may_be_hidden`).
        if emulate_option_may_be_hidden(args) {
            names.push(String::new());
        }
        if cut {
            names.push(String::new());
        }
    } else if matches!(base, "busybox" | "toybox") {
        // The options before the applet are the binary's own, and the first word that is not one is the applet.
        match args.iter().position(|arg| !arg.starts_with('-')) {
            Some(applet) => names.extend(command_words_names(&args[applet..], depth + 1, cut)),
            None if cut => names.push(String::new()),
            None => {}
        }
    } else if base == "hash" {
        names.extend(hash_names(args, depth, cut));
    } else if matches!(
        base,
        "typeset" | "declare" | "local" | "export" | "readonly" | "integer" | "float"
    ) {
        // These builtins declare their operands, and a `NAME[KEY]=VALUE` operand assigns an element of a special array, as a
        // leading assignment does (`element_assignment_names`). Each such operand defines its element.
        for arg in args {
            if let Some((kind, key, value)) = element_assignment(arg) {
                names.extend(element_definition_names(kind, key, value, depth, cut));
            }
        }
    } else if matches!(base, "." | "source") {
        // `.` and `source` run the commands of the file they are given, in the shell, so the file is read as a script run by
        // its path is (`command_names`), and the words after it are its arguments. A file that is a wrapper, such as
        // `eatmydata` or `xvfb-run`, runs those words as a command, which `command_words_names` reads as it reads a run by
        // path. A bare name is searched for in the working directory too, so it is withheld as well. An option, such as
        // bash's `-p path`, or a `--` is no file that can be read, so the withheld value covers it. With no file, a cut
        // segment may have one past the cut.
        match args.first() {
            Some(file) if file.starts_with('-') => names.push(String::new()),
            Some(file) => {
                names.extend(command_words_names(args, depth + 1, cut));
                if !is_system_binary_path(file) {
                    names.push(String::new());
                }
            }
            None if cut => names.push(String::new()),
            None => {}
        }
    } else if is_posix_shell(word) || is_other_shell(word) {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let scripts = if is_powershell(word) {
            powershell_command_arguments(&args)
        } else {
            shell_script_arguments(word, &args)
        };
        match scripts {
            Some(scripts) => {
                for script in scripts {
                    names.extend(command_prefixes_at(script, depth + 1));
                }
            }
            // The commands are in a script file or on the standard input, which no name here reads, or the command
            // string is one a wrapper gives at run time (`shell_script_arguments`).
            None => names.push(String::new()),
        }
        // A shell whose syntax the reader does not model may run a command it does not name, so its value is withheld too.
        if is_other_shell(word) {
            names.push(String::new());
        }
    }
    names
}

/// The commands `find` runs, read from the words after its own name: the words after each `-exec`, `-execdir`, `-ok`
/// and `-okdir` predicate, up to the terminator that ends it (a `;`, or a `+` right after `{}`), each read as a
/// command (`command_words_names`). No other word of `find`, a path or a test, is a command. A predicate whose command
/// word lies past a cut has that word withheld, as any command word past a cut is. Each `{}` of the command is replaced
/// by a file name at run time, so a word of the command that holds one is read as that substitution (`substituted`).
fn find_command_names(args: &[String], depth: usize, cut: bool) -> Vec<String> {
    let mut names = Vec::new();
    for (at, arg) in args.iter().enumerate() {
        if !matches!(arg.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
            continue;
        }
        let command = &args[at + 1..];
        let end = command
            .iter()
            .enumerate()
            .position(|(index, word)| {
                word == ";" || (word == "+" && index > 0 && command[index - 1] == "{}")
            })
            .unwrap_or(command.len());
        if end == 0 && cut {
            names.push(String::new());
        }
        // `find` puts each file name in place of every `{}`, so a word of the command that holds one is a run-time value.
        names.extend(command_words_names(
            &substituted(&command[..end], &["{}"]),
            depth,
            cut,
        ));
    }
    names
}

/// The programs whose command is the value of one of their options, or the address of `socat`, rather than a word of
/// their own: `fd` and `fdfind` run the words given with `-x` or `-X`, up to a `;`; `tar` runs the value of
/// `--to-command`, `--use-compress-program` (`-I`), `--info-script` (`-F`), `--new-volume-script`, `--rsh-command`,
/// `--rmt-command`, or the `exec=` action of `--checkpoint-action`; `rg` runs the value of `--pre` and
/// `--hostname-bin`; `socat` runs the command of its `EXEC:`, `SYSTEM:` or `SHELL:` address; and the netcat programs run
/// the value of `-e` and `-c`, and in `ncat` of `--exec` and `--sh-exec`. Their other options are too many to table, so each
/// program is read for these forms alone (`option_value_command_names`), and an argument holding an expansion, which may
/// stand for one of them, withholds the command (`argument_may_hide_an_option`).
const OPTION_VALUE_COMMANDS: &[&str] = &[
    "fd",
    "fdfind",
    "tar",
    "gtar",
    "bsdtar",
    "rg",
    "socat",
    "nc",
    "ncat",
    "netcat",
    "nc.openbsd",
    "nc.traditional",
];

/// The long options of `tar` whose value is a command it runs; `--checkpoint-action` is read by `tar_exec_command`.
const TAR_COMMAND_OPTIONS: &[&str] = &[
    "to-command",
    "use-compress-program",
    "info-script",
    "new-volume-script",
    "rsh-command",
    "rmt-command",
];

/// The command names an option-value program runs, read from the words after its own name. Every word is read for the
/// forms that give a command, wherever it stands, since these programs take their options among their operands, and a
/// word that is a value is read as well: naming too much is safe, naming too little is not. A command option may lie
/// past a cut, which these words do not show, so the withheld value is named then, as `eval`'s is. An argument that holds
/// an expansion may stand for one of these forms, so the withheld value is named for it too (`argument_may_hide_an_option`).
fn option_value_command_names(
    program: &str,
    args: &[String],
    depth: usize,
    cut: bool,
) -> Vec<String> {
    let mut names = Vec::new();
    if cut {
        names.push(String::new());
    }
    if args.iter().any(|arg| argument_may_hide_an_option(arg)) {
        names.push(String::new());
    }
    names.extend(match program {
        "fd" | "fdfind" => fd_exec_names(args, depth, cut),
        "tar" | "gtar" | "bsdtar" => tar_option_names(args, depth),
        "rg" => rg_option_names(args, depth),
        "socat" => socat_address_names(args, depth),
        // The netcat programs: `nc`, `ncat` and `netcat`, and their variants.
        _ => netcat_option_names(args, depth),
    });
    names
}

/// Whether an argument of an option-value program holds an expansion, which the shell makes before the program reads the
/// argument (`is_expanding_word`). Its value may be an option or an address the program reads, and it may split into
/// several words, one of them an option, so it is withheld wherever it stands, even after a literal character: `x$X` gives
/// two words when `X` holds ` --to-command=rm`. The braces `fd` fills (`FD_PLACEHOLDERS`) are literal to the shell, as a
/// bare `{}` is, so a word of only those is no expansion.
fn argument_may_hide_an_option(arg: &str) -> bool {
    let literal = FD_PLACEHOLDERS
        .iter()
        .fold(arg.to_owned(), |word, placeholder| {
            word.replace(*placeholder, "")
        });
    is_expanding_word(&literal)
}

/// The placeholders `fd` puts a file name in place of, in a word of a command it runs with `-x` or `-X`: `{}` is the path,
/// `{/}` the basename, `{//}` the parent directory, and `{.}` and `{/.}` the path and the basename without an extension.
/// The shell reads none of them as an expansion, so a word of only these is literal text to it.
const FD_PLACEHOLDERS: &[&str] = &["{}", "{/}", "{//}", "{.}", "{/.}"];

/// The commands `fd` and `fdfind` run with `-x` or `-X` (`--exec`, `--exec-batch`). The words given with each such
/// option, up to the `;` that ends them, are one command and its arguments, and a value attached to the option (`-xrm`,
/// `--exec=rm`) starts it. Each command is read as `command_words_names` reads one, with each placeholder of
/// `FD_PLACEHOLDERS` put in place (`substituted`), so a word that holds one is a run-time value, as `find`'s `{}` is.
/// Every other word is read as any word is. In a short cluster, a letter that takes a value before the `x` takes it
/// instead (`-tx` is the executable type), so that `x` is no option. A `\;` or a quoted `';'` cuts its segment, which
/// names the withheld value as well (`option_value_command_names`).
fn fd_exec_names(args: &[String], depth: usize, cut: bool) -> Vec<String> {
    let mut names = Vec::new();
    for (at, arg) in args.iter().enumerate() {
        let attached = if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = split_long_option(long);
            if !long_option_named(name, &["exec", "exec-batch"]) {
                continue;
            }
            value
        } else if let Some(letters) = arg.strip_prefix('-') {
            // The first of the exec letters `x` and `X`, and of the letters that take a value (`-d`, `-E`, `-e`, `-t`,
            // `-S`, `-o`, `-j`, `-c`), decides the word: an exec letter starts the command, and a value letter takes
            // the rest of the word.
            let Some(index) = letters.find(|letter: char| "xXdEetSojc".contains(letter)) else {
                continue;
            };
            if !matches!(letters.as_bytes()[index], b'x' | b'X') {
                continue;
            }
            Some(&letters[index + 1..])
        } else {
            continue;
        };
        let command: Vec<String> = attached
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .into_iter()
            .chain(
                args[at + 1..]
                    .iter()
                    .take_while(|word| word.as_str() != ";")
                    .cloned(),
            )
            .collect();
        // `fd` puts a file name in place of each placeholder in any word of the command, so a word that holds one is a
        // run-time value, read as `find`'s `{}` is (`substituted`).
        names.extend(command_words_names(
            &substituted(&command, FD_PLACEHOLDERS),
            depth,
            cut,
        ));
    }
    names
}

/// The commands `tar` runs by one of its options: the value of each long option in `TAR_COMMAND_OPTIONS`, the value of
/// `-I` and `-F` (a cluster holds either letter, with its value attached or in the next word), and the `exec=COMMAND`
/// action of `--checkpoint-action`. A long option may be abbreviated, as getopt reads it. The first word may be a
/// cluster with no dash, which tar's old style reads (`tar xI rm x.tar`). Each value is read as shell text, which names
/// the command it starts (`command_prefixes_at`).
fn tar_option_names(args: &[String], depth: usize) -> Vec<String> {
    let mut names = Vec::new();
    for (at, arg) in args.iter().enumerate() {
        let next = args.get(at + 1).map(String::as_str);
        if let Some(long) = arg.strip_prefix("--") {
            let (name, attached) = split_long_option(long);
            let value = attached.or(next);
            let command = if long_option_named(name, &["checkpoint-action"]) {
                value.and_then(tar_exec_command)
            } else if long_option_named(name, TAR_COMMAND_OPTIONS) {
                value
            } else {
                None
            };
            if let Some(command) = command {
                names.extend(command_prefixes_at(command, depth));
            }
        } else if let Some(letters) = arg.strip_prefix('-') {
            if let Some(index) = letters.find(['I', 'F']) {
                if let Some(command) = cluster_value(letters, index, next) {
                    names.extend(command_prefixes_at(command, depth));
                }
            }
        } else if at == 0 {
            // A first word with no dash is tar's old style, whose letters take their values from the words after it, in
            // order, and not from the rest of it. So the value of an `I` or `F` in it is any of those words, or the
            // rest of the word, and each of them is read as a command.
            if let Some(index) = arg.find(['I', 'F']) {
                if let Some(command) = cluster_value(arg, index, None) {
                    names.extend(command_prefixes_at(command, depth));
                }
                for word in &args[1..] {
                    names.extend(command_prefixes_at(word, depth));
                }
            }
        }
    }
    names
}

/// The commands `rg` runs by `--pre`, the preprocessor it runs on each file, and by `--hostname-bin`. The value is
/// attached (`--pre=COMMAND`) or the next word, and is read as shell text (`command_prefixes_at`).
fn rg_option_names(args: &[String], depth: usize) -> Vec<String> {
    let mut names = Vec::new();
    for (at, arg) in args.iter().enumerate() {
        let Some(long) = arg.strip_prefix("--") else {
            continue;
        };
        let (name, attached) = split_long_option(long);
        if long_option_named(name, &["pre", "hostname-bin"]) {
            let next = args.get(at + 1).map(String::as_str);
            if let Some(command) = attached.or(next) {
                names.extend(command_prefixes_at(command, depth));
            }
        }
    }
    names
}

/// The keywords of the `socat` addresses that run a command: `EXEC` runs its program, and `SYSTEM` and `SHELL` run their
/// command through a shell. The socat 1.8.0.0 man page documents all three (`socat_address_names`).
const SOCAT_COMMAND_ADDRESSES: &[&str] = &["exec", "system", "shell"];

/// The separators that put one address after another in an argument of `socat`. `!!` joins the reading and the writing
/// address of a dual address, as that man page documents. `|` chains addresses in socat 2, and `%` may be read as a
/// separator by an older build; neither is documented for 1.8.0.0, so both are read too, as reading more only names more.
const SOCAT_ADDRESS_SEPARATORS: &[&str] = &["!!", "|", "%"];

/// The commands `socat` runs by an `EXEC:`, `SYSTEM:` or `SHELL:` address, which takes its command after the colon. An
/// address starts an argument or follows one of `SOCAT_ADDRESS_SEPARATORS`, so every address of a dual or chained argument
/// is read: `OPEN:/dev/null!!EXEC:rm x` runs `rm` from its second address. `SYSTEM` and `SHELL` bar `!!` inside their
/// commands, and a command runs to the end of its argument, so the text past a separator is read as shell text too
/// (`command_prefixes_at`). The keyword is read without regard to case, or to the spaces and marks before it.
fn socat_address_names(args: &[String], depth: usize) -> Vec<String> {
    args.iter()
        .flat_map(|arg| socat_address_starts(arg))
        .filter_map(|address| address.split_once(':'))
        .filter(|(keyword, _)| {
            let keyword = keyword.trim_matches(|c: char| !c.is_ascii_alphanumeric());
            SOCAT_COMMAND_ADDRESSES
                .iter()
                .any(|name| keyword.eq_ignore_ascii_case(name))
        })
        .flat_map(|(_, command)| command_prefixes_at(command, depth))
        .collect()
}

/// The text from each place an address of `socat` may start in an argument: its beginning, and the text after each of
/// `SOCAT_ADDRESS_SEPARATORS`. Each is read as an address on its own, so an address after a separator is read as one at
/// the beginning of an argument is.
fn socat_address_starts(arg: &str) -> Vec<&str> {
    let mut starts = vec![arg];
    for separator in SOCAT_ADDRESS_SEPARATORS {
        starts.extend(
            arg.match_indices(*separator)
                .map(|(at, _)| &arg[at + separator.len()..]),
        );
    }
    starts
}

/// The commands `nc`, `ncat` and `netcat` run by `-e` and `-c`, in a short cluster too, with the value attached or in
/// the next word, and by `ncat`'s `--exec` and `--sh-exec`. Each is read as shell text (`command_prefixes_at`).
/// `ncat`'s `--lua-exec` runs a Lua script, whose commands no name here reads, so its value is withheld.
fn netcat_option_names(args: &[String], depth: usize) -> Vec<String> {
    let mut names = Vec::new();
    for (at, arg) in args.iter().enumerate() {
        let next = args.get(at + 1).map(String::as_str);
        if let Some(long) = arg.strip_prefix("--") {
            let (name, attached) = split_long_option(long);
            if long_option_named(name, &["lua-exec"]) {
                names.push(String::new());
            } else if long_option_named(name, &["exec", "sh-exec"]) {
                if let Some(command) = attached.or(next) {
                    names.extend(command_prefixes_at(command, depth));
                }
            }
        } else if let Some(letters) = arg.strip_prefix('-') {
            if let Some(index) = letters.find(['e', 'c']) {
                if let Some(command) = cluster_value(letters, index, next) {
                    names.extend(command_prefixes_at(command, depth));
                }
            }
        }
    }
    names
}

/// The command an `exec=COMMAND` action of `tar --checkpoint-action` runs. A prefix of `exec` is read as `exec`, which
/// names more than tar does if it is no such action, and never less. The other actions run no command.
fn tar_exec_command(action: &str) -> Option<&str> {
    let (name, command) = action.split_once('=')?;
    long_option_named(name, &["exec"]).then_some(command)
}

/// A long option's name and the value its `=` attaches, if it has one: `to-command=rm` is `to-command` with `rm`.
fn split_long_option(long: &str) -> (&str, Option<&str>) {
    match long.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (long, None),
    }
}

/// Whether a long option's name is one of `options`, or the start of one, as getopt reads an abbreviation. An ambiguous
/// abbreviation is no option, but it is read as one, which names too much and never too little.
fn long_option_named(name: &str, options: &[&str]) -> bool {
    !name.is_empty() && options.iter().any(|option| option.starts_with(name))
}

/// The value of the option letter at `index` in a cluster of short options: the rest of the word after the letter, or
/// else the next word (`-Ixz` and `-I xz` both give `xz`).
fn cluster_value<'a>(letters: &'a str, index: usize, next: Option<&'a str>) -> Option<&'a str> {
    let attached = &letters[index + 1..];
    (!attached.is_empty()).then_some(attached).or(next)
}

/// The action a `trap` is given: its first operand after its options, where `--` ends them. A lone `-` is an operand,
/// the action that resets a signal. `None` when there is no operand, as with `trap -l`.
fn trap_action(args: &[String]) -> Option<&str> {
    let mut options_done = false;
    for arg in args {
        let arg = arg.as_str();
        if !options_done && arg == "--" {
            options_done = true;
        } else if options_done || arg == "-" || !arg.starts_with('-') {
            return Some(arg);
        }
    }
    None
}

/// The names the text of zsh's `emulate` runs (`command_words_names`). `emulate [-LR] SHELL -c ARG` evaluates ARG as shell text,
/// as `eval` evaluates its arguments. The text is one of the words after each option that holds a `c` (`-c`, `-Lc`, `+c`).
/// zsh takes the option words before the text (`+x`, and `-o NAME`, whose `-o` takes the next word as its value), so which of
/// the words after the option is the text is not known here. Each of them is read as shell text on its own, and the words joined
/// with spaces are read too (`command_prefixes_at`). The option is read wherever it stands among the words, not only after the
/// SHELL operand, so no command is named less.
fn emulate_script_names(args: &[String], depth: usize) -> Vec<String> {
    args.iter()
        .enumerate()
        .filter(|(_, arg)| is_c_option(arg))
        .flat_map(|(at, _)| {
            let text = &args[at + 1..];
            let mut names = command_prefixes_at(&text.join(" "), depth + 1);
            for word in text {
                names.extend(command_prefixes_at(word, depth + 1));
                // After `--` or a lone `-` zsh takes the next word as the text whatever it starts with, so an expanding
                // word that looks like an option (`-$x`) can be the text and is read as a leading word above. It is
                // withheld, as an expanding word that starts a command is.
                if is_expanding_word(word) {
                    names.push(String::new());
                }
            }
            names
        })
        .collect()
}

/// Whether an argument of `emulate` is a `-c` option: a word that starts with `-` or `+` and holds a `c`, as `-c`, `-Lc` and
/// `+c` do. zsh's parseopts reads the `c` under either sign as the command flag, so `+c` takes the text as `-c` does.
fn is_c_option(arg: &str) -> bool {
    arg.strip_prefix(['-', '+'])
        .is_some_and(|letters| letters.contains('c'))
}

/// Whether `emulate`'s arguments may hide its `-c` option behind an expanding word, so the text it runs is withheld. The
/// shell expands a word before `emulate` reads it, so the option may be a word no name here reads, or a word that carries
/// it (`a=(sh -c 'rm x'); emulate $a`). The words before the last literal `-c` option are checked, and with no such option
/// every word is. The text is one of the words after that option, which `emulate_script_names` reads each on its own, so an
/// expanding word there that starts a command is withheld by that reading.
fn emulate_option_may_be_hidden(args: &[String]) -> bool {
    let before_option = args
        .iter()
        .rposition(|arg| is_c_option(arg))
        .unwrap_or(args.len());
    args[..before_option]
        .iter()
        .any(|arg| is_expanding_word(arg))
}

/// The names a command word stands for. A word that holds an expansion runs as whatever the shell expands it
/// to, which no name here can give, so it names the withheld value (`""`), which covers every value of its
/// kind. Any other word names itself (`command_name`), and a path names its basename as well: `/bin/rm` and
/// `./rm` run `rm`. A path outside the system binary directories (`is_system_binary_path`) runs a file the shell
/// reads, which may be a script whose commands no name here reads, so it names the withheld value too, as
/// `sh x.sh` does (`shell_script_arguments`). Empty when the word has no name to read.
fn command_names(word: &str) -> Vec<String> {
    if is_expanding_word(word) {
        return vec![String::new()];
    }
    let mut names: Vec<String> = command_name(word).into_iter().collect();
    if word.contains(['/', '\\']) {
        // Read from the whole word, so a separator after a character that ends the name (`/tmp/x@y/rm`)
        // still leaves `rm` named.
        names.extend(command_name(command_basename(word)));
        if !is_system_binary_path(word) {
            names.push(String::new());
        }
    }
    names
}

/// The last name of a path word, as the shell runs it: `rm` for `/bin/rm`, and for `/bin/rm/` too. Read from the
/// whole word, so a character that ends a name in a directory does not hide the name after it.
fn command_basename(word: &str) -> &str {
    word.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
}

/// The directories of the default `PATH` (`FALLBACK_PATH` in `code_interpreter`), where the system's programs are. The
/// sandbox mounts the root read-only and binds writable only the roots it is given (`sandbox.rs`, `--ro-bind / /`), so a
/// program in one of them is the image's own binary (`is_system_binary_path`).
const SYSTEM_BINARY_DIRECTORIES: &[&str] = &[
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
];

/// Whether a command word is a path straight into a system binary directory (`/usr/bin/rm`), so the program it runs is
/// the image's own binary, and the name it carries is that program. Any other path runs a file the shell reads, which may
/// be a script whose commands no name here reads: a relative one (`./x.sh`, `../bin/rm`), one elsewhere (`/workspace/x`),
/// one in a subdirectory of a system directory (`/usr/bin/x/y`), or one that climbs out of it (`/usr/bin/../../x.sh`). A
/// word that names no program in such a directory (`/bin/`, `/bin/..`) is not one either. A workspace file can be
/// written by the model (`workspace_put`), so only the image's directories are trusted.
fn is_system_binary_path(word: &str) -> bool {
    word.rsplit_once('/').is_some_and(|(directory, name)| {
        SYSTEM_BINARY_DIRECTORIES.contains(&directory)
            && !matches!(name, "" | "." | "..")
            && !name.contains('\\')
    })
}

/// Whether a command word holds an expansion, so the shell runs something no name here can give: a parameter,
/// command or arithmetic expansion (`$`, or a backtick), a brace (`{`, `}`), a pathname pattern (`*`, `?`, or a
/// `[` with a `]` after it), a tilde (`~`), or the `=` that a zsh equals expansion takes before a command name. A
/// `[` alone is the test command, not a pattern.
fn is_expanding_word(word: &str) -> bool {
    let equals_expansion = word
        .strip_prefix('=')
        .is_some_and(|name| name.starts_with(is_name_char));
    let bracket_pattern = word
        .find('[')
        .is_some_and(|open| word[open..].contains(']'));
    equals_expansion || bracket_pattern || word.contains(['$', '`', '{', '}', '*', '?', '~'])
}

/// Whether a character can be part of a command name (see `command_name`).
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '+')
}

/// The name a command word stands for: the word cut where it stops being a name, and at most 64 characters
/// long. `None` when the word has no name to read.
fn command_name(word: &str) -> Option<String> {
    let name: String = word
        .chars()
        .take_while(|c| is_name_char(*c))
        .take(64)
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Words that are not the command their segment runs: an assignment, an option (a leading `-`), and a
/// word that opens or negates a compound command (`if`, `then`, `!`, `{`, `time`, ...). The command is
/// the word after them.
fn is_leading_word(word: &str) -> bool {
    is_assignment(word)
        || word.starts_with('-')
        || matches!(
            word,
            "!" | "{" | "}" | "if" | "then" | "else" | "elif" | "do" | "while" | "until" | "time"
        )
}

/// Whether a word is a shell identifier, a letter or `_` and then letters, digits and `_`: the NAME of a `coproc` or
/// a `function`.
fn is_identifier(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether a word opens a compound command, whose words run what they hold (`coproc NAME { rm x; }`).
fn is_compound_start(word: &str) -> bool {
    matches!(
        word,
        "{" | "if" | "while" | "until" | "for" | "case" | "select" | "[["
    )
}

/// The values of `values`, each once, in their first order.
fn distinct(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

/// Whether a word is a shell assignment in the sense bash and zsh give it: a name, an optional subscript, then `=` or
/// `+=` and its value. A POSIX shell such as dash reads only a plain `NAME=VALUE` as one (`is_plain_assignment`), so a
/// word that is no plain assignment is also the command word there, and `command_prefix` reads it both ways.
fn is_assignment(word: &str) -> bool {
    if !word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return false;
    }
    let name_len = word
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        .count();
    // An array element's assignment, `NAME[subscript]=value`, is one too: its subscript runs to the first `]`, and may hold a
    // `=` of its own.
    let rest = &word[name_len..];
    let rest = match rest.strip_prefix('[') {
        Some(subscripted) => match subscripted.find(']') {
            Some(close) => &subscripted[close + 1..],
            None => return false,
        },
        None => rest,
    };
    // `NAME+=value` appends to the variable (bash and zsh alike), so its `+` is the operator's, not part of the name.
    rest.starts_with('=') || rest.starts_with("+=")
}

/// Whether a word is a plain assignment, `NAME=VALUE`: an identifier and then `=`, the one form every POSIX shell reads as
/// an assignment. The `+=` and subscripted forms are not (`is_assignment`). The program of a `shell` call runs with no
/// shell, so `program_name` and the argv of `shell_program_commands` skip only a plain assignment.
fn is_plain_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let starts_as_name = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
    starts_as_name && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether a word is an assignment only in bash's and zsh's sense (`NAME+=VALUE`, `NAME[subscript]=VALUE`): one that
/// `is_assignment` reads and `is_plain_assignment` does not. In a POSIX shell such as dash it is the command word, so
/// `A+=/bin/rm` runs the path `A+=/bin/rm` and `A[1]=x` is a glob. `command_prefix` reads such a word as a command too.
fn is_extended_assignment(word: &str) -> bool {
    is_assignment(word) && !is_plain_assignment(word)
}

/// Whether a word is a number written in decimal digits: one or more ASCII digits and nothing else.
fn is_digits(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether `strtoimax` in base 10 reads a word whole: leading whitespace, an optional sign, then decimal digits and
/// nothing after them. util-linux before 2.41 reads such a word as `chrt`'s priority (`wrapped_command_names`). The
/// whitespace is read as Unicode whitespace, a superset of the C locale's, so a word that is read as a number is never
/// missed.
fn is_decimal_number(word: &str) -> bool {
    let unsigned = word.trim_start();
    is_digits(unsigned.strip_prefix(['+', '-']).unwrap_or(unsigned))
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
                named: gate != ToolClassGate::Off,
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
            trusted_capability_id("knowledge_list_documents").as_deref(),
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
        // The question tool (WS14 S4) is bound, to a seeded id, so the gate evaluates it rather
        // than refusing it as unbound.
        assert_eq!(
            trusted_capability_id("ask_user").as_deref(),
            Some("cap.memory.index")
        );
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
        // browser.open and browser.observe share one capability (WS21-S5a); only the signed tool
        // name tells one read from the other. browser.act has its own capability now.
        let browser = Fixture::builtin("browser.observe");
        assert_eq!(browser.capability_id, "cap.browser.observe");
        assert_eq!(
            Fixture::builtin("browser.act").capability_id,
            "cap.browser.act"
        );
        let (response, open_evidence) = v2_pair(
            &key,
            &V2 {
                capability_id: "cap.browser.observe",
                tool_name: "browser.open",
                user_id: "user-a",
            },
            "read",
            "none",
            "",
        );

        assert!(
            !verifier.verify(
                &open_evidence,
                &response,
                &browser.call("browser.observe", ToolClassGate::Enforce)
            ),
            "enforce must refuse evidence minted for another tool"
        );
        // Shadow lets the legacy tuple decide for a built-in tool, and counts it.
        assert!(verifier.verify(
            &open_evidence,
            &response,
            &browser.call("browser.observe", ToolClassGate::Shadow)
        ));
        // The same evidence is, of course, good for the tool it names.
        assert!(verifier.verify(
            &open_evidence,
            &response,
            &browser.call("browser.open", ToolClassGate::Enforce)
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
        /// The decision the stand-in answers; `allow` when unset.
        decision: Mutex<Option<String>>,
        /// The decision the stand-in answers for a request that names this target value, in place of
        /// `decision`: one target of a multi-target call answered differently from the others.
        decision_by_target: Mutex<HashMap<String, String>>,
        /// When set, the evidence names this decision instead of the one answered.
        signed_decision: Mutex<Option<String>>,
        /// When set, the evidence names this run instead of the one asked about.
        signed_for_run: Mutex<Option<String>>,
        /// When set, the evidence names this policy source instead of the one
        /// answered: a response rewritten in transit.
        signed_policy_source: Mutex<Option<String>>,
        /// When set, the evidence carries this claim-set version instead of "2".
        signed_version: Mutex<Option<String>>,
        signed_for_user: Mutex<Option<String>>,
        signed_for_org: Mutex<Option<String>>,
        signed_class: Mutex<Option<String>>,
        signed_floor: Mutex<Option<String>>,
        /// The policy source the stand-in answers; `risk_level` when unset.
        policy_source: Mutex<Option<String>>,
        tamper_signature: Mutex<bool>,
        policy_checked: Mutex<bool>,
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
            let answered = self
                .state
                .decision_by_target
                .lock()
                .expect("decision by target lock")
                .get(&request.target_value)
                .cloned()
                .or_else(|| self.state.decision.lock().expect("decision lock").clone());
            let mut response = EvaluatePolicyResponse {
                decision: answered.unwrap_or_else(|| "allow".to_owned()),
                reason: "ok".to_owned(),
                budget_context: "budget".to_owned(),
                decision_id: "decision-1".to_owned(),
                capability_version: self.state.version.lock().expect("version lock").clone(),
                decision_evidence: String::new(),
                side_effect_class: class,
                approval_floor: floor,
                policy_source: self
                    .state
                    .policy_source
                    .lock()
                    .expect("policy source lock")
                    .clone()
                    .unwrap_or_else(|| "risk_level".to_owned()),
                tool_digest: digest,
            };
            let signed_tool = self
                .state
                .signed_for_tool
                .lock()
                .expect("override lock")
                .clone()
                .unwrap_or_else(|| request.tool_name.clone());
            let signed_decision = self
                .state
                .signed_decision
                .lock()
                .expect("signed decision lock")
                .clone()
                .unwrap_or_else(|| response.decision.clone());
            let signed_run = self
                .state
                .signed_for_run
                .lock()
                .expect("signed run lock")
                .clone()
                .unwrap_or_else(|| request.run_id.clone());
            let signed_source = self
                .state
                .signed_policy_source
                .lock()
                .expect("signed source lock")
                .clone()
                .unwrap_or_else(|| response.policy_source.clone());
            let signed_version = self
                .state
                .signed_version
                .lock()
                .expect("signed version lock")
                .clone()
                .unwrap_or_else(|| "2".to_owned());
            let signed_user = self.state.signed_for_user.lock().unwrap().clone()
                .unwrap_or_else(|| request.user_id.clone());
            let signed_class = self.state.signed_class.lock().unwrap().clone()
                .unwrap_or_else(|| response.side_effect_class.clone());
            let signed_floor = self.state.signed_floor.lock().unwrap().clone()
                .unwrap_or_else(|| response.approval_floor.clone());
            let signed_org = self.state.signed_for_org.lock().unwrap().clone()
                .unwrap_or_else(|| request.org_id.clone());
            let policy_checked = *self.state.policy_checked.lock().unwrap();
            let now = now_unix();
            response.decision_evidence = sign_json(
                &self.key,
                &serde_json::json!({
                    "v": signed_version,
                    "iat": now - 1,
                    "exp": now + 30,
                    "decision_id": response.decision_id,
                    "capability_id": request.capability_id,
                    "capability_version": response.capability_version,
                    "org_id": signed_org,
                    "run_id": signed_run,
                    "agent_id": request.agent_id,
                    "scope": request.scope,
                    "decision": signed_decision,
                    "reason": response.reason,
                    "budget_context": response.budget_context,
                    "user_id": signed_user,
                    "tool_name": signed_tool,
                    "side_effect_class": signed_class,
                    "approval_floor": signed_floor,
                    "policy_source": signed_source,
                    "tool_digest": response.tool_digest,
                    "tool_policy_checked": policy_checked,
                }),
            );
            if *self.state.tamper_signature.lock().unwrap() {
                response.decision_evidence.push('x');
            }
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

        async fn get_procedure_version(
            &self,
            _: Request<mp_contracts::model_plane::v1::GetProcedureVersionRequest>,
        ) -> Result<Response<mp_contracts::model_plane::v1::ProcedureVersion>, Status> {
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
            browser_agent_policy_enforced: false,
            permission_rules: false,
            bindings: Arc::default(),
        };
        (policy, state)
    }

    fn denied(result: Result<CapabilityEvaluation, Status>) -> Status {
        result.expect_err("the call must be refused")
    }

    fn ask_from(state: &MockState) {
        *state.decision.lock().expect("decision lock") = Some("ask".to_owned());
    }

    #[tokio::test]
    async fn a_correctly_signed_ask_is_marked_verified_and_names_its_capability() {
        for gate in ALL_GATES {
            let (policy, state) = harness(gate).await;
            ask_from(&state);
            let evaluation = policy
                .evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a")
                .await
                .expect("an ask is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Ask, "{gate:?}");
            assert!(evaluation.ask_evidence_verified, "{gate:?}");
            assert_eq!(evaluation.capability_id, "cap.browser.open", "{gate:?}");
            assert_eq!(evaluation.reason, "ok", "{gate:?}");
            assert_eq!(evaluation.policy_source, "risk_level", "{gate:?}");
        }
    }

    #[tokio::test]
    async fn a_tampered_ask_is_still_an_ask_but_never_verified() {
        // Another run's decision, or a decision that does not say `ask`, is not
        // evidence for this ask. Neither is refused: the step parks either way.
        let (policy, state) = harness(ToolClassGate::Enforce).await;
        ask_from(&state);
        *state.signed_for_run.lock().unwrap() = Some("run-b".to_owned());
        let evaluation = policy
            .evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a")
            .await
            .expect("a tampered ask still evaluates");
        assert_eq!(evaluation.decision, CapabilityDecision::Ask);
        assert!(!evaluation.ask_evidence_verified, "replayed run");

        let (policy, state) = harness(ToolClassGate::Enforce).await;
        ask_from(&state);
        *state.signed_decision.lock().unwrap() = Some("allow".to_owned());
        let evaluation = policy
            .evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a")
            .await
            .expect("a tampered ask still evaluates");
        assert_eq!(evaluation.decision, CapabilityDecision::Ask);
        assert!(!evaluation.ask_evidence_verified, "tampered decision");
    }

    #[tokio::test]
    async fn an_ask_is_verified_only_when_its_signed_claims_bind_the_policy_source() {
        // The response says `risk_level` but the signature covers a stored
        // tool-policy ask: a rewritten response must not pass as the risk-level
        // floor, in any gate mode, including `Off` where `verify` skips v2 claims.
        for gate in ALL_GATES {
            let (policy, state) = harness(gate).await;
            ask_from(&state);
            *state.signed_policy_source.lock().unwrap() = Some("tool_policy_ask".to_owned());
            let evaluation = policy
                .evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a")
                .await
                .expect("a rewritten ask still evaluates");
            assert_eq!(evaluation.decision, CapabilityDecision::Ask, "{gate:?}");
            assert_eq!(evaluation.policy_source, "risk_level", "{gate:?}");
            assert!(!evaluation.ask_evidence_verified, "{gate:?}");
        }
    }

    #[tokio::test]
    async fn an_ask_needs_v2_claims_and_the_tool_it_was_asked_for_in_every_gate_mode() {
        // A v1 claim set carries no policy source, so it never verifies an ask,
        // although `verify` accepts it with the gate off.
        for gate in ALL_GATES {
            let (policy, state) = harness(gate).await;
            ask_from(&state);
            *state.signed_version.lock().unwrap() = Some("1".to_owned());
            let evaluation = policy
                .evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a")
                .await
                .expect("a v1 ask still evaluates");
            assert!(!evaluation.ask_evidence_verified, "{gate:?}");
        }
        // Under `Shadow` a decision for another tool is only counted by `verify`;
        // for an ask it is not evidence. Under `Off` the request names no tool, so
        // evidence naming one was not made for this request either.
        for gate in ALL_GATES {
            let (policy, state) = harness(gate).await;
            ask_from(&state);
            *state.signed_for_tool.lock().unwrap() = Some("browser.act".to_owned());
            let evaluation = policy
                .evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a")
                .await
                .expect("a replayed ask still evaluates");
            assert!(!evaluation.ask_evidence_verified, "{gate:?}");
        }
    }

    #[tokio::test]
    async fn without_a_verification_key_an_ask_is_never_verified() {
        let (mut policy, state) = harness(ToolClassGate::Off).await;
        policy.decision_verifier = DecisionEvidenceVerifier::disabled_for_test();
        ask_from(&state);
        let evaluation = policy
            .evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a")
            .await
            .expect("an ask is an evaluation");
        assert_eq!(evaluation.decision, CapabilityDecision::Ask);
        assert!(!evaluation.ask_evidence_verified);
    }

    #[tokio::test]
    async fn an_allow_signed_for_another_run_is_still_refused() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        *state.signed_for_run.lock().unwrap() = Some("run-b".to_owned());
        let error = denied(
            policy
                .evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a")
                .await,
        );
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert!(error.message().contains("invalid decision evidence"));
    }

    #[tokio::test]
    async fn allow_and_deny_are_never_ask_verified() {
        let (policy, _state) = harness(ToolClassGate::Off).await;
        let allow = policy
            .evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a")
            .await
            .expect("allow");
        assert_eq!(allow.decision, CapabilityDecision::Allow);
        assert!(allow.evidence_verified);
        assert!(!allow.ask_evidence_verified);

        let (policy, state) = harness(ToolClassGate::Off).await;
        *state.decision.lock().unwrap() = Some("deny".to_owned());
        let deny = policy
            .evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a")
            .await
            .expect("deny");
        assert_eq!(deny.decision, CapabilityDecision::Deny);
        assert!(!deny.ask_evidence_verified);
        assert_eq!(deny.capability_id, "cap.browser.open");
    }

    // The connected-app levels (EP-10 S7) need no Do-loop code: capability-core's engine answers a stored
    // `ask` or `never` on the provider capabilities, and the loop pauses or refuses on that answer. These pin
    // that the provider tools are evaluated against their own capabilities with the person named, so the
    // person's own row can apply. With the class gate off the person is not named (see
    // `the_tool_and_the_user_are_named_to_capability_core_only_once_the_gate_is_on`).
    const NAMING_GATES: [ToolClassGate; 2] = [ToolClassGate::Shadow, ToolClassGate::Enforce];

    #[tokio::test]
    async fn user_ask_on_provider_read_pauses() {
        for gate in NAMING_GATES {
            let (policy, state) = harness(gate).await;
            ask_from(&state);
            *state.policy_source.lock().unwrap() = Some("tool_policy_ask".to_owned());
            let evaluation = policy
                .evaluate_tool_call("read_provider_action", "run-a", "org-a", "user-a")
                .await
                .expect("an ask is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Ask, "{gate:?}");
            assert_eq!(evaluation.policy_source, "tool_policy_ask", "{gate:?}");
            assert_eq!(
                evaluation.capability_id, "cap.tool.provider.read",
                "{gate:?}"
            );
            let sent = state.evaluated.lock().unwrap();
            assert_eq!(sent[0].capability_id, "cap.tool.provider.read", "{gate:?}");
            assert_eq!(sent[0].user_id, "user-a", "{gate:?}");
        }
    }

    #[tokio::test]
    async fn user_never_on_provider_read_denies() {
        for gate in NAMING_GATES {
            for tool in ["read_provider_action", "list_provider_actions"] {
                let (policy, state) = harness(gate).await;
                *state.decision.lock().unwrap() = Some("deny".to_owned());
                *state.policy_source.lock().unwrap() = Some("tool_policy_never".to_owned());
                let case = format!("{gate:?} {tool}");
                let evaluation = policy
                    .evaluate_tool_call(tool, "run-a", "org-a", "user-a")
                    .await
                    .expect("a deny is an evaluation");
                assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{case}");
                assert_eq!(evaluation.policy_source, "tool_policy_never", "{case}");
                assert_eq!(evaluation.capability_id, "cap.tool.provider.read", "{case}");
                assert!(!evaluation.ask_evidence_verified, "{case}");
                let sent = state.evaluated.lock().unwrap();
                assert_eq!(sent[0].user_id, "user-a", "{case}");
            }
        }
    }

    #[tokio::test]
    async fn user_never_on_provider_execute_denies() {
        for gate in NAMING_GATES {
            let (policy, state) = harness(gate).await;
            *state.decision.lock().unwrap() = Some("deny".to_owned());
            *state.policy_source.lock().unwrap() = Some("tool_policy_never".to_owned());
            let evaluation = policy
                .evaluate_tool_call("execute_provider_action", "run-a", "org-a", "user-a")
                .await
                .expect("a deny is an evaluation");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{gate:?}");
            assert_eq!(
                evaluation.capability_id, "cap.tool.provider.execute",
                "{gate:?}"
            );
            let sent = state.evaluated.lock().unwrap();
            assert_eq!(
                sent[0].capability_id, "cap.tool.provider.execute",
                "{gate:?}"
            );
            assert_eq!(sent[0].user_id, "user-a", "{gate:?}");
        }
    }

    #[test]
    fn a_local_policy_is_trusted_for_an_ask_as_it_is_for_an_allow() {
        let evaluation = CapabilityEvaluation::unverified(CapabilityDecision::Ask);
        assert!(evaluation.evidence_verified);
        assert!(evaluation.ask_evidence_verified);
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

    #[tokio::test]
    async fn browser_scoped_policy_requires_named_checked_v2_evidence() {
        let (mut policy, state) = harness(ToolClassGate::Off).await;
        policy.browser_agent_policy_enforced = true;
        let mut browser = wire_binding("browser_agent", "external_write", "none");
        browser.tool_digest.clear(); // Built-in tools have no MCP definition digest.
        state.bindings.lock().unwrap().push(browser);
        ask_from(&state);
        *state.policy_checked.lock().unwrap() = true;
        let answer = policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.expect("signed scoped ask");
        assert_eq!(answer.decision, CapabilityDecision::Ask);
        assert!(answer.ask_evidence_verified);
        let requests = state.evaluated.lock().unwrap();
        assert_eq!(requests[0].tool_name, "browser_agent");
        assert_eq!(requests[0].user_id, "user-a");
        drop(requests);

        // A newly signed response still may not lower or change the floor this
        // same run already pinned, even when the capability version is unchanged.
        state.bindings.lock().unwrap()[0].approval_floor = "ask".to_owned();
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "floor changed within run");
        state.bindings.lock().unwrap()[0].approval_floor = "none".to_owned();

        *state.policy_checked.lock().unwrap() = false;
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "unchecked policy");
        *state.policy_checked.lock().unwrap() = true;
        *state.signed_version.lock().unwrap() = Some("1".to_owned());
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "old evidence");
        *state.signed_version.lock().unwrap() = None;
        *state.signed_for_tool.lock().unwrap() = Some("web_fetch".to_owned());
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "replayed tool");
        *state.signed_for_tool.lock().unwrap() = None;
        *state.signed_for_user.lock().unwrap() = Some("other-user".to_owned());
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "replayed user");
        *state.signed_for_user.lock().unwrap() = None;
        *state.signed_for_org.lock().unwrap() = Some("other-org".to_owned());
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "replayed org");
        *state.signed_for_org.lock().unwrap() = None;
        *state.signed_for_run.lock().unwrap() = Some("other-run".to_owned());
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "replayed run");
        *state.signed_for_run.lock().unwrap() = None;
        *state.signed_class.lock().unwrap() = Some("read".to_owned());
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "tampered class");
        *state.signed_class.lock().unwrap() = None;
        *state.signed_floor.lock().unwrap() = Some("ask".to_owned());
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "tampered floor");
        *state.signed_floor.lock().unwrap() = None;
        *state.tamper_signature.lock().unwrap() = true;
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "tampered signature");
        *state.tamper_signature.lock().unwrap() = false;
        *state.signed_policy_source.lock().unwrap() = Some("tool_policy_ask".to_owned());
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "tampered policy source");
        *state.signed_policy_source.lock().unwrap() = None;
        *state.policy_source.lock().unwrap() = Some("tool_policy_ask".to_owned());
        let stored_ask = policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.expect("signed stored ask");
        assert_eq!(stored_ask.policy_source, "tool_policy_ask");
        assert!(stored_ask.ask_evidence_verified);
        *state.decision.lock().unwrap() = Some("deny".to_owned());
        *state.policy_source.lock().unwrap() = Some("tool_policy_never".to_owned());
        let stored_deny = policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.expect("signed stored deny");
        assert_eq!(stored_deny.decision, CapabilityDecision::Deny);
        assert_eq!(stored_deny.policy_source, "tool_policy_never");
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "").await.is_err(), "missing owner");
        policy.decision_verifier = DecisionEvidenceVerifier::disabled_for_test();
        assert!(policy.evaluate_tool_call("browser_agent", "run-a", "org-a", "user-a").await.is_err(), "missing verification key");
    }

    #[tokio::test]
    async fn browser_scoped_policy_does_not_name_or_change_other_tools() {
        let (mut policy, state) = harness(ToolClassGate::Off).await;
        policy.browser_agent_policy_enforced = true;
        let result = policy.evaluate_tool_call("web_fetch", "run-other", "org-a", "user-a").await.expect("other tool unchanged");
        assert_eq!(result.decision, CapabilityDecision::Allow);
        let calls = state.evaluated.lock().unwrap();
        assert_eq!(calls[0].tool_name, "");
        assert_eq!(calls[0].user_id, "");
    }

    /// W2-WS13-S7-rule-use-a: the request each entry point sends states the run's retention as
    /// execution-core resolved it: "persistent" or "retention_bound" only where the caller stated the
    /// posture (`evaluate_tool_call_with_input`), and empty where it did not, so a caller that never
    /// stated one never claims "persistent". In every gate mode, since the field is sent always.
    #[tokio::test]
    async fn every_entry_point_states_persistent_retention_bound_or_nothing() {
        for gate in ALL_GATES {
            let (policy, state) = harness(gate).await;
            policy
                .evaluate_tool_call_with_input("web_fetch", "{}", "run-a", "org-a", "user-a", false)
                .await
                .expect("persistent evaluates");
            policy
                .evaluate_tool_call_with_input("web_fetch", "{}", "run-a", "org-a", "user-a", true)
                .await
                .expect("retention-bound evaluates");
            policy
                .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
                .await
                .expect("no stated posture evaluates");
            policy
                .evaluate_with_evidence("web_fetch", "run-a", "org-a")
                .await
                .expect("with evidence evaluates");
            policy
                .evaluate("web_fetch", "run-a", "org-a")
                .await
                .expect("no user evaluates");
            let stated: Vec<String> = state
                .evaluated
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.run_retention.clone())
                .collect();
            assert_eq!(
                stated,
                ["persistent", "retention_bound", "", "", ""],
                "{gate:?}"
            );
        }
    }

    /// The retention-bound posture is a closed word on the wire and withholds the call's target
    /// value, while the persistent one names it (only once permission rules ask for targets).
    #[tokio::test]
    async fn a_retention_bound_call_names_no_target_value() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        let input = r#"{"url":"https://example.org/a"}"#;
        policy
            .evaluate_tool_call_with_input("web_fetch", input, "run-a", "org-a", "user-a", true)
            .await
            .expect("retention-bound evaluates");
        policy
            .evaluate_tool_call_with_input("web_fetch", input, "run-a", "org-a", "user-a", false)
            .await
            .expect("persistent evaluates");
        let calls = state.evaluated.lock().unwrap();
        assert_eq!(calls[0].run_retention, "retention_bound");
        assert_eq!(
            calls[0].target_value, "",
            "no content of a retention-bound call"
        );
        assert_eq!(calls[1].run_retention, "persistent");
    }

    // ---------------------------------------------------------------------
    // Targets checked one by one, and the strictest answer decides (V3-ec-1)
    // ---------------------------------------------------------------------

    const MAIL_SEND: &str = r#"{"connection_id":"conn-1","operation":"gmail.send","body":{"to":"ann@allowed.example, bob@denied.example","cc":["cy@denied.example","dee@other.example"],"bcc":"eve@x.example; fay@y.example"}}"#;

    fn targets_of(tool: &str, input: &str) -> Vec<(String, String)> {
        derive_targets(tool, input)
            .into_iter()
            .map(|(kind, value)| (kind.to_owned(), value))
            .collect()
    }

    fn target_pairs(sent: &[EvaluatePolicyRequest]) -> Vec<(String, String)> {
        sent.iter()
            .map(|request| (request.target_kind.clone(), request.target_value.clone()))
            .collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(kind, value)| ((*kind).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn a_mail_send_names_the_domain_of_every_recipient_in_to_cc_and_bcc() {
        let values: Vec<String> = targets_of("execute_provider_action", MAIL_SEND)
            .into_iter()
            .map(|(kind, value)| {
                assert_eq!(kind, "recipient");
                value
            })
            .collect();
        assert_eq!(
            values,
            [
                "allowed.example",
                "denied.example",
                "other.example",
                "x.example",
                "y.example"
            ]
        );
        // A recipient string that holds a comment with an `@` names that domain too.
        let commented = r#"{"operation":"mail.send","body":{"to":"bob@denied.example (note: ann@allowed.example)"}}"#;
        let values: Vec<String> = targets_of("execute_provider_action", commented)
            .into_iter()
            .map(|(_, value)| value)
            .collect();
        assert_eq!(values, ["denied.example", "allowed.example"]);
        // An operation that is not a mail send names no recipient.
        assert!(targets_of(
            "execute_provider_action",
            r#"{"operation":"calendar.create","body":{"to":"a@b.example"}}"#
        )
        .is_empty());
    }

    #[test]
    fn a_mail_body_the_executor_passes_on_unread_is_checked_as_every_recipient() {
        // Gmail passes a raw message on and Microsoft a whole message object: their recipients are
        // not read here, so the one withheld value, which covers every recipient rule, is checked.
        assert_eq!(
            targets_of(
                "execute_provider_action",
                r#"{"operation":"gmail.send","body":{"raw":"fixture-raw-message"}}"#
            ),
            pairs(&[("recipient", "")])
        );
        assert_eq!(
            targets_of(
                "execute_provider_action",
                r#"{"operation":"mail.send","body":{"message":{"toRecipients":[]}}}"#
            ),
            pairs(&[("recipient", "")])
        );
        // A recipient with no domain to read (an address literal, say) is checked as every recipient.
        assert_eq!(
            targets_of(
                "execute_provider_action",
                r#"{"operation":"gmail.send","body":{"to":"ann@[192.0.2.1]"}}"#
            ),
            pairs(&[("recipient", "")])
        );
        // So is a send with one such recipient beside readable ones, and an internationalized domain,
        // which a stored rule (a plain DNS name) could never match exactly.
        assert_eq!(
            targets_of(
                "execute_provider_action",
                r#"{"operation":"gmail.send","body":{"to":"bob@allowed.example, ann@bücher.example"}}"#
            ),
            pairs(&[("recipient", "")])
        );
        assert_eq!(
            targets_of(
                "execute_provider_action",
                r#"{"operation":"gmail.send","body":{"to":"bob@allowed.example, ann@evil.example%"}}"#
            ),
            pairs(&[("recipient", "")])
        );
        // A blank raw body is not passed on, so `to` is read as usual (and there is none here).
        assert!(targets_of(
            "execute_provider_action",
            r#"{"operation":"gmail.send","body":{"raw":"  "}}"#
        )
        .is_empty());
    }

    #[test]
    fn a_web_address_and_a_sh_program_name_their_targets_as_before() {
        assert_eq!(
            targets_of("web_fetch", r#"{"url":"https://Example.ORG./a?q=1"}"#),
            pairs(&[("domain", "example.org")])
        );
        assert_eq!(
            targets_of("browser.open", r#"{"url":"http://sub.example.org:8080/x"}"#),
            pairs(&[("domain", "sub.example.org")])
        );
        assert!(targets_of("web_fetch", r#"{"url":"file:///etc/passwd"}"#).is_empty());
        // A `sh` program names the commands of its code; a Python program names none.
        assert_eq!(
            targets_of(
                "code_interpreter",
                r#"{"language":"sh","code":"true && rm -rf x"}"#
            ),
            pairs(&[("command_prefix", "true"), ("command_prefix", "rm")])
        );
        assert!(targets_of(
            "code_interpreter",
            r#"{"language":"python","code":"import os"}"#
        )
        .is_empty());
    }

    #[test]
    fn a_background_sh_process_names_the_commands_of_its_code_as_code_interpreter_does() {
        // `process_start` runs its `sh` code as `code_interpreter` does, so the commands it names are the same. Its
        // language defaults to Python, as `code_interpreter`'s does, and a Python process names no command.
        let code = r#"{"language":"sh","code":"true && rm -rf x"}"#;
        assert_eq!(
            targets_of("process_start", code),
            pairs(&[("command_prefix", "true"), ("command_prefix", "rm")])
        );
        assert_eq!(
            targets_of("process_start", code),
            targets_of("code_interpreter", code)
        );
        assert!(targets_of("process_start", r#"{"code":"import os"}"#).is_empty());
        assert!(targets_of(
            "process_start",
            r#"{"language":"python","code":"import os"}"#
        )
        .is_empty());
        // Its arguments are the script's positional parameters, which name nothing here.
        assert_eq!(
            targets_of(
                "process_start",
                r#"{"language":"sh","code":"true","args":["rm","-rf","x"]}"#
            ),
            pairs(&[("command_prefix", "true")])
        );
        // Only `process_start` starts a command; the other process tools name none.
        for tool in [
            "process_read",
            "process_stdin",
            "process_signal",
            "process_list",
        ] {
            assert!(targets_of(tool, code).is_empty(), "{tool}");
        }
    }

    #[test]
    fn process_start_reads_only_the_sh_it_runs_while_code_interpreter_lowercases_its_language() {
        // `process_start` matches the trimmed language exactly, so `SH` starts nothing and names nothing, and a padded
        // `sh` runs `sh` and is read. `code_interpreter` lowercases its language, so `SH` runs `sh` there and is read.
        let code = |language: &str| format!(r#"{{"language":"{language}","code":"rm x"}}"#);
        assert!(targets_of("process_start", &code("SH")).is_empty());
        assert!(targets_of("process_start", &code("Sh")).is_empty());
        assert_eq!(
            targets_of("process_start", &code(" sh ")),
            pairs(&[("command_prefix", "rm")])
        );
        assert_eq!(
            targets_of("code_interpreter", &code("SH")),
            pairs(&[("command_prefix", "rm")])
        );
    }

    #[tokio::test]
    async fn a_mail_body_passed_on_unread_is_decided_by_a_recipient_rule_whatever_its_domain() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert(String::new(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "execute_provider_action",
                r#"{"operation":"gmail.send","body":{"raw":"fixture-raw-message"}}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
        assert_eq!(
            target_pairs(&state.evaluated.lock().unwrap()),
            pairs(&[("recipient", "")])
        );
    }

    #[test]
    fn the_shell_text_of_a_call_names_the_command_of_every_segment() {
        let names = |text: &str| command_prefixes(text);
        assert_eq!(names("true && rm -rf x"), ["true", "rm"]);
        assert_eq!(
            names("true || rm x; cat y | grep z"),
            ["true", "rm", "cat", "grep"]
        );
        assert_eq!(names("true & rm -rf x"), ["true", "rm"]);
        assert_eq!(names("true\nrm -rf x"), ["true", "rm"]);
        // A redirection is not a separator, and its target is not a command.
        assert_eq!(names("ls 2>&1 | grep x"), ["ls", "grep"]);
        assert_eq!(names("cat x &>/dev/null; rm y"), ["cat", "rm"]);
        assert_eq!(names(">/dev/null rm -rf x"), ["rm"]);
        assert_eq!(names("2>/dev/null > out.txt rm x"), ["rm"]);
        assert_eq!(names("2> err.log"), Vec::<String>::new());
        // Substitutions, subshells and groups run their commands too.
        assert_eq!(names("echo $(rm -rf x)"), ["echo", "rm"]);
        assert_eq!(names("(rm -rf x)"), ["rm"]);
        // A backtick substitution is read as `$(...)`: the command word it starts is withheld, and `rm` is named.
        assert_eq!(names("`rm -rf x`"), ["", "rm"]);
        assert_eq!(names("{ rm -rf x; }"), ["rm"]);
        assert!(names("if true; then rm -rf x; fi")
            .iter()
            .any(|name| name == "rm"));
        assert_eq!(names("time -p rm x"), ["rm"]);
        // Quotes and backslashes are removed from a command word, as the shell removes them.
        assert_eq!(names("\"rm\" -rf x"), ["rm"]);
        assert_eq!(names("'rm' x"), ["rm"]);
        assert_eq!(names("\\rm -rf x"), ["rm"]);
        assert_eq!(names("X=\"a b\" rm -rf x"), ["rm"]);
        // Assignments name no command. A command word built by expansion (a variable) names the withheld value.
        assert_eq!(names("FOO=1 BAR=2"), Vec::<String>::new());
        assert_eq!(names("$CMD arg"), [""]);
    }

    #[test]
    fn a_line_continuation_is_joined_as_the_shell_joins_it() {
        // A backslash before a line feed (or a carriage return and a line feed) joins the two lines. The
        // backslashes pair up as escapes, so after an even run the line break stays.
        assert_eq!(join_line_continuations("r\\\nm -rf x"), "rm -rf x");
        assert_eq!(join_line_continuations("r\\\r\nm -rf x"), "rm -rf x");
        assert_eq!(join_line_continuations("a\\\\\nb"), "a\\\\\nb");
        assert_eq!(join_line_continuations("a\\\\\\\nb"), "a\\\\b");
        // Joined, `r` and `m` are the one command `rm`; as written they are two. Both are named, so the
        // command is named whichever way the shell reads the break.
        let names = command_prefixes("r\\\nm -rf x");
        assert!(names.contains(&"rm".to_owned()), "{names:?}");
        assert!(names.contains(&"r".to_owned()) && names.contains(&"m".to_owned()));
        // An escaped backslash ends no word, so the break after it still ends the command.
        assert_eq!(command_prefixes("echo x\\\\\nrm -rf y"), ["echo", "rm"]);
    }

    #[test]
    fn a_shell_program_given_a_script_with_dash_c_names_the_commands_of_the_script() {
        assert_eq!(
            targets_of(
                "shell",
                r#"{"program":"sh","args":["-c","true && rm -rf x"]}"#
            ),
            pairs(&[
                ("command_prefix", "sh"),
                ("command_prefix", "true"),
                ("command_prefix", "rm"),
            ])
        );
        // A combined flag, with an option before it.
        assert_eq!(
            targets_of(
                "shell",
                r#"{"program":"/bin/bash","args":["-e","-lc","rm -rf x"]}"#
            ),
            pairs(&[
                ("command_prefix", "/bin/bash"),
                ("command_prefix", "bash"),
                ("command_prefix", "rm"),
            ])
        );
        // Not a shell: its arguments are not read as a script.
        assert_eq!(
            targets_of("shell", r#"{"program":"grep","args":["-c","rm"]}"#),
            pairs(&[("command_prefix", "grep")])
        );
    }

    #[test]
    fn a_dash_c_script_is_found_past_the_options_a_shell_takes_around_it() {
        // `--` ends the options, and an option after `-c` is skipped: the script is the operand left.
        assert_eq!(
            targets_of("shell", r#"{"program":"sh","args":["-c","--","rm -rf x"]}"#),
            pairs(&[("command_prefix", "sh"), ("command_prefix", "rm")])
        );
        assert_eq!(
            targets_of("shell", r#"{"program":"sh","args":["-c","-e","rm -rf x"]}"#),
            pairs(&[("command_prefix", "sh"), ("command_prefix", "rm")])
        );
        // The options before `-c` take their values, and the script after `-c` is still the one read.
        assert_eq!(
            targets_of(
                "shell",
                r#"{"program":"bash","args":["-e","-o","pipefail","-c","rm -rf x"]}"#
            ),
            pairs(&[("command_prefix", "bash"), ("command_prefix", "rm")])
        );
        // An option that takes a value after `-c` (`-o option`, `-O shopt_option`) does not hide the script.
        for args in [
            r#"{"program":"bash","args":["-c","-o","pipefail","rm -rf x"]}"#,
            r#"{"program":"bash","args":["-c","-O","extglob","rm -rf x"]}"#,
        ] {
            let named = targets_of("shell", args);
            assert!(
                named
                    .iter()
                    .any(|(kind, value)| kind == "command_prefix" && value == "rm"),
                "{named:?}"
            );
        }
    }

    #[test]
    fn a_shell_that_reads_its_commands_from_stdin_or_a_script_file_withholds_its_value() {
        // A POSIX shell with no `-c` text runs the commands on its standard input or in a script file, and no name
        // here reads them. Its value is withheld, which covers every command. The shell still names itself, and any
        // command the text names beside it. A `-c` text is still read, and a `-c` after a script file is one of the
        // file's arguments.
        for (text, expected) in [
            ("echo 'rm -rf x' | sh", vec!["echo", "sh", ""]),
            ("sh", vec!["sh", ""]),
            ("sh -s", vec!["sh", ""]),
            ("sh -", vec!["sh", ""]),
            ("sh -- -c 'rm x'", vec!["sh", ""]),
            ("sh +c 'rm x'", vec!["sh", ""]),
            ("sh -e x.sh", vec!["sh", ""]),
            ("bash -l", vec!["bash", ""]),
            ("bash ./x.sh", vec!["bash", ""]),
            ("sh -o pipefail x.sh", vec!["sh", ""]),
            ("sh --rcfile rc.sh x.sh", vec!["sh", ""]),
            ("sh x.sh -c 'echo ok'", vec!["sh", ""]),
            ("sh -oc 'rm x'", vec!["sh", ""]),
            ("sh -s -c 'rm x'", vec!["sh", "rm"]),
            ("sh -o pipefail -c 'rm x'", vec!["sh", "rm"]),
            ("sh -co pipefail 'rm x'", vec!["sh", "pipefail", "rm"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
        // A heredoc is the standard input of the shell it feeds, so that shell's commands are withheld too.
        let heredoc = command_prefixes("sh <<EOF\nrm -rf x\nEOF");
        assert!(
            heredoc.contains(&String::new()) && heredoc.contains(&"rm".to_owned()),
            "{heredoc:?}"
        );
        // `chroot` with no command runs `$SHELL -i`, a shell on the standard input, so its default is withheld too.
        assert_eq!(
            command_prefixes("echo 'rm x' | chroot /srv"),
            ["echo", "chroot", ""]
        );
        assert_eq!(command_prefixes("chroot /srv"), ["chroot", ""]);
        assert_eq!(command_prefixes("chroot /srv rm x"), ["chroot", "rm"]);
    }

    #[test]
    fn a_shell_reading_stdin_or_a_script_file_names_the_withheld_value_as_a_target() {
        assert_eq!(
            targets_of(
                "code_interpreter",
                r#"{"language":"sh","code":"echo 'rm -rf x' | sh"}"#
            ),
            pairs(&[
                ("command_prefix", "echo"),
                ("command_prefix", "sh"),
                ("command_prefix", ""),
            ])
        );
        assert_eq!(
            targets_of("shell", r#"{"program":"bash","args":["./x.sh"]}"#),
            pairs(&[("command_prefix", "bash"), ("command_prefix", "")])
        );
        assert_eq!(
            targets_of("shell", r#"{"program":"sh","args":["-s"]}"#),
            pairs(&[("command_prefix", "sh"), ("command_prefix", "")])
        );
        // `chroot` with no command runs `$SHELL -i`, which reads stdin, so the withheld value is its target too.
        assert_eq!(
            targets_of("shell", r#"{"program":"chroot","args":["/srv"]}"#),
            pairs(&[("command_prefix", "chroot"), ("command_prefix", "")])
        );
        // A wrapper runs the shell it is given, which reads the file it is given, so the file's commands are withheld.
        assert_eq!(
            targets_of("shell", r#"{"program":"env","args":["sh","x.sh"]}"#),
            pairs(&[
                ("command_prefix", "env"),
                ("command_prefix", "sh"),
                ("command_prefix", ""),
            ])
        );
        // A `-c` text is still read as shell text, and the shell's own name is still a target.
        assert_eq!(
            targets_of("shell", r#"{"program":"sh","args":["-s","-c","rm -rf x"]}"#),
            pairs(&[("command_prefix", "sh"), ("command_prefix", "rm")])
        );
    }

    #[test]
    fn a_shell_program_names_itself_even_when_it_is_a_keyword_such_as_time() {
        // The program runs with its arguments and no shell, so its own name is a command: `time` is one. And
        // `time` runs the command it is given, which is named too.
        assert_eq!(
            targets_of("shell", r#"{"program":"time","args":["rm","x"]}"#),
            pairs(&[("command_prefix", "time"), ("command_prefix", "rm")])
        );
    }

    #[test]
    fn a_call_checked_against_several_targets_takes_the_strictest_decision_and_its_own_evidence() {
        use CapabilityDecision::{Allow, Ask, Deny};
        let answer = |decision: CapabilityDecision, id: &str| CapabilityEvaluation {
            decision_id: id.to_owned(),
            ..CapabilityEvaluation::unverified(decision)
        };
        let decided = |answers: Vec<CapabilityEvaluation>| {
            strictest_evaluation(answers).expect("one answer per target")
        };
        assert_eq!(
            decided(vec![answer(Allow, "a"), answer(Allow, "b")]).decision,
            Allow
        );
        assert_eq!(
            decided(vec![
                answer(Allow, "a"),
                answer(Ask, "b"),
                answer(Allow, "c")
            ])
            .decision,
            Ask
        );
        assert_eq!(
            decided(vec![
                answer(Ask, "a"),
                answer(Deny, "b"),
                answer(Allow, "c")
            ])
            .decision,
            Deny
        );
        // Of equally strict answers the first is kept, whole, with its own id.
        assert_eq!(
            decided(vec![answer(Ask, "first"), answer(Ask, "second")]).decision_id,
            "first"
        );
        assert!(strictest_evaluation(Vec::new()).is_err());
    }

    #[tokio::test]
    async fn the_tool_and_the_person_are_named_while_the_permission_rules_are_on_even_with_the_gate_off(
    ) {
        let (mut policy, state) = harness(ToolClassGate::Off).await;
        policy.permission_rules = true;
        ask_from(&state);
        let evaluation = policy
            .evaluate_tool_call("web_fetch", "run-a", "org-a", "user-a")
            .await
            .expect("an ask is an evaluation");
        {
            let sent = state.evaluated.lock().unwrap();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].tool_name, "web_fetch");
            assert_eq!(sent[0].user_id, "user-a");
        }
        // The signed claims name the same tool and person, so the ask verifies for this call.
        assert_eq!(evaluation.decision, CapabilityDecision::Ask);
        assert!(evaluation.ask_evidence_verified);
    }

    #[tokio::test]
    async fn every_recipient_domain_is_checked_and_a_denied_one_denies_the_send() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert("denied.example".to_owned(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "execute_provider_action",
                MAIL_SEND,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(
            target_pairs(&sent),
            pairs(&[
                ("recipient", "allowed.example"),
                ("recipient", "denied.example"),
                ("recipient", "other.example"),
                ("recipient", "x.example"),
                ("recipient", "y.example"),
            ])
        );
        assert!(sent
            .iter()
            .all(|request| request.tool_name == "execute_provider_action"
                && request.user_id == "user-a"));
    }

    #[tokio::test]
    async fn an_ask_on_one_recipient_asks_the_send_with_its_own_verified_evidence() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert("denied.example".to_owned(), "ask".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "execute_provider_action",
                MAIL_SEND,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("an ask is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Ask);
        assert!(evaluation.ask_evidence_verified);
        assert_eq!(state.evaluated.lock().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn a_denied_command_is_not_escaped_by_chaining_it_after_an_allowed_one() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert("rm".to_owned(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"true && rm -rf x"}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(
            target_pairs(&sent),
            pairs(&[("command_prefix", "true"), ("command_prefix", "rm")])
        );
    }

    #[tokio::test]
    async fn a_denied_command_given_to_a_shell_with_dash_c_is_denied_too() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert("rm".to_owned(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"sh","args":["-c","true && rm -rf x"]}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
    }

    #[tokio::test]
    async fn a_denied_command_after_dash_c_and_a_dash_dash_is_denied_too() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert("rm".to_owned(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"sh","args":["-c","--","rm -rf x"]}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
    }

    #[tokio::test]
    async fn a_denied_program_named_time_is_denied_too() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert("time".to_owned(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"time","args":["rm","x"]}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(
            target_pairs(&sent),
            pairs(&[("command_prefix", "time"), ("command_prefix", "rm")])
        );
    }

    #[tokio::test]
    async fn a_denied_command_in_a_background_sh_process_is_denied_too() {
        // A background process runs its `sh` code as a `shell` call or a `code_interpreter` program does, so a
        // command the policy denies is denied there too, and the request names the tool and the person.
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert("rm".to_owned(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "process_start",
                r#"{"language":"sh","code":"true && rm -rf x"}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(
            target_pairs(&sent),
            pairs(&[("command_prefix", "true"), ("command_prefix", "rm")])
        );
        assert!(sent
            .iter()
            .all(|request| request.tool_name == "process_start" && request.user_id == "user-a"));
    }

    #[tokio::test]
    async fn with_both_switches_off_a_call_sends_one_unnamed_request_as_before() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            ("execute_provider_action", MAIL_SEND),
            ("shell", r#"{"program":"true && rm -rf x"}"#),
            ("web_fetch", r#"{"url":"https://example.org/a"}"#),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 3, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
            assert_eq!(request.run_retention, "persistent");
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_a_background_sh_process_sends_one_unnamed_request() {
        // The switch off, a background process is sent as it was before the rules: one request that names
        // neither the tool nor the person, and no command.
        let (policy, state) = harness(ToolClassGate::Off).await;
        policy
            .evaluate_tool_call_with_input(
                "process_start",
                r#"{"language":"sh","code":"true && rm -rf x"}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("an allowed call evaluates");
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 1, "one request, no check per command");
        assert_eq!(sent[0].tool_name, "");
        assert_eq!(sent[0].user_id, "");
        assert_eq!(sent[0].target_kind, "");
        assert_eq!(sent[0].target_value, "");
        assert_eq!(sent[0].run_retention, "persistent");
    }

    #[tokio::test]
    async fn a_retention_bound_or_very_long_call_is_checked_once_with_its_value_withheld() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        // A zero-retention run withholds every value, so its targets are one check of the kind.
        policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"true && rm -rf x"}"#,
                "run-a",
                "org-a",
                "user-a",
                true,
            )
            .await
            .expect("evaluates");
        // More distinct commands than one call is checked against: one check, value withheld.
        let commands: Vec<String> = (0..=MAX_TARGET_CHECKS)
            .map(|n| format!("tool{n}"))
            .collect();
        let program = format!(r#"{{"program":"{}"}}"#, commands.join(" && "));
        policy
            .evaluate_tool_call_with_input("shell", &program, "run-b", "org-a", "user-a", false)
            .await
            .expect("evaluates");
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(
            target_pairs(&sent),
            pairs(&[("command_prefix", ""), ("command_prefix", "")])
        );
        assert_eq!(sent[0].run_retention, "retention_bound");
        assert_eq!(sent[1].run_retention, "persistent");
    }

    #[tokio::test]
    async fn a_retention_bound_background_sh_process_is_checked_once_with_its_command_withheld() {
        // A zero-retention or waived run withholds the command's value, so the call is checked once as the
        // withheld value, which a rule on any command covers.
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert(String::new(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "process_start",
                r#"{"language":"sh","code":"true && rm -rf x"}"#,
                "run-a",
                "org-a",
                "user-a",
                true,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(target_pairs(&sent), pairs(&[("command_prefix", "")]));
        assert_eq!(sent[0].tool_name, "process_start");
        assert_eq!(sent[0].run_retention, "retention_bound");
    }

    #[test]
    fn a_command_word_built_by_expansion_names_the_withheld_value() {
        // The shell runs what the expansion gives, which no name here can say, so the word names the value
        // withheld, which covers every rule of the kind.
        for text in [
            "$CMD -rf x",
            "r$@m -rf x",
            "{rm,-rf,x}",
            "$'rm' -rf x",
            "\"$CMD\" x",
            "${CMD:-rm} x",
            "~/bin/rm x",
            "r* -rf x",
            "r? -rf x",
            "[r]m -rf x",
            "a{b,c} x",
            "=rm -rf x",
        ] {
            assert_eq!(command_prefixes(text), [""], "{text}");
        }
        // A substitution or an extended glob that starts a command word is withheld, and the commands inside
        // it are named as well.
        for text in [
            "`echo rm` -rf x",
            "$(echo rm) -rf x",
            "@(rm|ls) x",
            "+(rm) x",
            "!(rm) x",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&String::new()), "{text}: {names:?}");
        }
        assert!(command_prefixes("@(rm|ls) x").contains(&"rm".to_owned()));
        assert!(command_prefixes("+(rm) x").contains(&"rm".to_owned()));
        // A substitution in an argument position starts no command, and `[` and `[[` are the test command.
        assert_eq!(command_prefixes("echo `date`"), ["echo", "date"]);
        assert_eq!(command_prefixes("echo $HOME"), ["echo"]);
        assert_eq!(command_prefixes("[ -f x ] && rm -rf x"), ["rm"]);
        assert_eq!(command_prefixes("[[ -f x ]] && cat y"), ["cat"]);
        // A run of equals signs, as a markdown rule may be, names no command.
        assert_eq!(command_prefixes("===== x"), Vec::<String>::new());
    }

    #[test]
    fn a_path_names_its_basename_as_well_as_itself() {
        // The shell runs `rm` at `/bin/rm`, at `./rm` and at `../bin/rm`, so the basename is named with the path. Only
        // `/bin/rm` is in a system binary directory: `./rm` and `../bin/rm` are files the shell runs, which may be
        // scripts, so their value is withheld too (`is_system_binary_path`).
        assert_eq!(command_prefixes("/bin/rm -rf x"), ["/bin/rm", "rm"]);
        assert_eq!(command_prefixes("./rm -rf x"), ["./rm", "rm", ""]);
        assert_eq!(command_prefixes("../bin/rm x"), ["../bin/rm", "rm", ""]);
        // A trailing separator names the same basename, and the basename is read from the whole path, so a
        // character that ends the name in a directory does not hide it.
        assert!(command_prefixes("/bin/rm/ x").contains(&"rm".to_owned()));
        assert!(command_prefixes("/tmp/x@y/rm -rf x").contains(&"rm".to_owned()));
        // A path longer than a name is cut at 64 characters, and its basename is still named.
        let long = format!("/{}/rm -rf x", "a".repeat(80));
        assert!(command_prefixes(&long).contains(&"rm".to_owned()));
        // Quoted or escaped, a path is read as the shell reads it.
        assert_eq!(command_prefixes("'/bin/rm' x"), ["/bin/rm", "rm"]);
        assert_eq!(command_prefixes("/bin/r\\m x"), ["/bin/rm", "rm"]);
    }

    #[test]
    fn a_path_outside_the_system_binary_directories_withholds_its_value() {
        // A script run by its path names its own name and its basename, and the withheld value, which covers every
        // command rule. The commands in the file are not read here, and a workspace file can hold any of them.
        assert_eq!(command_prefixes("./x.sh"), ["./x.sh", "x.sh", ""]);
        assert_eq!(command_prefixes("/workspace/x"), ["/workspace/x", "x", ""]);
        assert_eq!(
            command_prefixes("true && ../bin/x.sh"),
            ["true", "../bin/x.sh", "x.sh", ""]
        );
        // A path that climbs out of a system directory, or goes into a subdirectory of one, is not a binary of it.
        assert_eq!(
            command_prefixes("/usr/bin/../../workspace/x.sh"),
            ["/usr/bin/../../workspace/x.sh", "x.sh", ""]
        );
        assert_eq!(command_prefixes("/usr/bin/x/y"), ["/usr/bin/x/y", "y", ""]);
        // A path directly in a system directory is the binary it names, and names nothing more.
        assert_eq!(command_prefixes("/usr/bin/rm x"), ["/usr/bin/rm", "rm"]);
        assert_eq!(
            command_prefixes("/usr/local/bin/tool x"),
            ["/usr/local/bin/tool", "tool"]
        );
        assert!(!is_system_binary_path("/bin/"));
        assert!(!is_system_binary_path("/bin/.."));
        assert!(is_system_binary_path("/sbin/init"));
        // A wrapper runs the path it is given, and that path is withheld as a command of its own.
        assert_eq!(
            command_prefixes("/usr/bin/env ./x.sh"),
            ["/usr/bin/env", "env", "./x.sh", "x.sh", ""]
        );
        // A shell at a path reads the `-c` text it is given, and its own value is withheld as any shell's is.
        assert_eq!(command_prefixes("./sh -c 'rm x'"), ["./sh", "sh", "", "rm"]);
        assert_eq!(command_prefixes("bash ./x.sh"), ["bash", ""]);
    }

    #[test]
    fn a_script_run_by_its_path_names_the_withheld_value_as_a_target_in_every_tool_that_runs_sh() {
        // `shell` runs its program with no shell, and `code_interpreter` and `process_start` run their `sh` code. A
        // script named by its path is withheld, as `sh x.sh` is, and a system binary is named as before.
        assert_eq!(
            targets_of("shell", r#"{"program":"./x.sh"}"#),
            pairs(&[
                ("command_prefix", "./x.sh"),
                ("command_prefix", "x.sh"),
                ("command_prefix", ""),
            ])
        );
        assert_eq!(
            targets_of("shell", r#"{"program":"/workspace/x"}"#),
            pairs(&[
                ("command_prefix", "/workspace/x"),
                ("command_prefix", "x"),
                ("command_prefix", ""),
            ])
        );
        for tool in ["code_interpreter", "process_start"] {
            assert_eq!(
                targets_of(tool, r#"{"language":"sh","code":"./x.sh"}"#),
                pairs(&[
                    ("command_prefix", "./x.sh"),
                    ("command_prefix", "x.sh"),
                    ("command_prefix", ""),
                ]),
                "{tool}"
            );
        }
        assert_eq!(
            targets_of("shell", r#"{"program":"/usr/bin/rm","args":["-rf","x"]}"#),
            pairs(&[("command_prefix", "/usr/bin/rm"), ("command_prefix", "rm")])
        );
    }

    #[test]
    fn a_program_or_script_word_built_by_expansion_is_withheld_and_a_path_is_named() {
        assert_eq!(
            targets_of("shell", r#"{"program":"/bin/rm","args":["-rf","x"]}"#),
            pairs(&[("command_prefix", "/bin/rm"), ("command_prefix", "rm")])
        );
        assert_eq!(
            targets_of("shell", r#"{"program":"$CMD","args":["-rf","x"]}"#),
            pairs(&[("command_prefix", "")])
        );
        assert_eq!(
            targets_of("shell", r#"{"program":"sh","args":["-c","$CMD -rf x"]}"#),
            pairs(&[("command_prefix", "sh"), ("command_prefix", "")])
        );
        assert_eq!(
            targets_of(
                "code_interpreter",
                r#"{"language":"sh","code":"r$@m -rf x"}"#
            ),
            pairs(&[("command_prefix", "")])
        );
    }

    #[tokio::test]
    async fn a_denied_command_built_by_expansion_is_denied_too() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        // The withheld value covers every command rule, so a deny on it denies a call whose command is hidden.
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert(String::new(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"r$@m -rf x"}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
        assert_eq!(
            target_pairs(&state.evaluated.lock().unwrap()),
            pairs(&[("command_prefix", "")])
        );
    }

    #[tokio::test]
    async fn a_denied_rm_run_by_its_path_is_denied_too() {
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert("rm".to_owned(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"/bin/rm","args":["-rf","x"]}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(
            target_pairs(&sent),
            pairs(&[("command_prefix", "/bin/rm"), ("command_prefix", "rm")])
        );
    }

    #[tokio::test]
    async fn a_script_run_by_its_path_is_denied_by_a_deny_on_the_withheld_value() {
        // A file the workspace writes can hold `rm` where no name reads it, so the script is checked as the withheld
        // value. A deny on that value denies the call, for a `shell` call and a background process alike.
        for (tool, input) in [
            ("shell", r#"{"program":"./x.sh"}"#),
            ("process_start", r#"{"language":"sh","code":"./x.sh"}"#),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert(String::new(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{tool}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                target_pairs(&sent).contains(&("command_prefix".to_owned(), String::new())),
                "{tool}: {:?}",
                target_pairs(&sent)
            );
            assert!(sent
                .iter()
                .all(|request| request.tool_name == tool && request.user_id == "user-a"));
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_a_script_run_by_its_path_is_not_named() {
        // The switch off, a script run by its path is sent as any other call: one request that names neither the tool
        // nor a target, and no check per command.
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            ("shell", r#"{"program":"./x.sh"}"#),
            ("process_start", r#"{"language":"sh","code":"./x.sh"}"#),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 2, "one request per call, no check per command");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_an_expansion_or_a_path_is_not_named() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for input in [
            r#"{"program":"r$@m -rf x"}"#,
            r#"{"program":"/bin/rm -rf x"}"#,
            r#"{"program":"sh","args":["-c","$CMD -rf x"]}"#,
        ] {
            policy
                .evaluate_tool_call_with_input("shell", input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 3, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[tokio::test]
    async fn a_denied_command_read_from_stdin_or_a_script_file_is_denied_too() {
        // The withheld value covers every command rule, so a deny on it denies a shell whose commands are not read.
        for (tool, input) in [
            ("shell", r#"{"program":"bash","args":["./x.sh"]}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"echo 'rm -rf x' | sh"}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert(String::new(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                target_pairs(&sent).contains(&("command_prefix".to_owned(), String::new())),
                "{input}: {:?}",
                target_pairs(&sent)
            );
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_a_shell_reading_its_commands_names_no_command() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            ("shell", r#"{"program":"bash","args":["./x.sh"]}"#),
            ("shell", r#"{"program":"sh","args":["-s"]}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"echo 'rm -rf x' | sh"}"#,
            ),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 3, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[test]
    fn a_wrapper_program_names_the_command_it_runs_as_well_as_itself() {
        // The program's argv is read as a command: a wrapper among its words runs the command after its options,
        // and that command is named with its own name.
        let shell = |input: &str| targets_of("shell", input);
        assert_eq!(
            shell(r#"{"program":"env","args":["rm","-rf","x"]}"#),
            pairs(&[("command_prefix", "env"), ("command_prefix", "rm")])
        );
        // `NAME=VALUE` words and options come before the command, and an option's value is no command.
        assert_eq!(
            shell(r#"{"program":"env","args":["-i","FOO=1","rm","-rf","x"]}"#),
            pairs(&[("command_prefix", "env"), ("command_prefix", "rm")])
        );
        assert_eq!(
            shell(r#"{"program":"nice","args":["-n","10","rm","x"]}"#),
            pairs(&[("command_prefix", "nice"), ("command_prefix", "rm")])
        );
        // `timeout`'s duration is an operand, not a command.
        assert_eq!(
            shell(r#"{"program":"timeout","args":["-s","KILL","5","rm","x"]}"#),
            pairs(&[("command_prefix", "timeout"), ("command_prefix", "rm")])
        );
        assert_eq!(
            shell(r#"{"program":"xargs","args":["-0","-I","{}","rm","{}"]}"#),
            pairs(&[("command_prefix", "xargs"), ("command_prefix", "rm")])
        );
        assert_eq!(
            shell(r#"{"program":"sudo","args":["-u","root","rm","x"]}"#),
            pairs(&[("command_prefix", "sudo"), ("command_prefix", "rm")])
        );
        assert_eq!(
            shell(r#"{"program":"nohup","args":["rm","x"]}"#),
            pairs(&[("command_prefix", "nohup"), ("command_prefix", "rm")])
        );
        assert_eq!(
            shell(r#"{"program":"exec","args":["-a","n","rm","x"]}"#),
            pairs(&[("command_prefix", "exec"), ("command_prefix", "rm")])
        );
        assert_eq!(
            shell(r#"{"program":"command","args":["-p","rm","x"]}"#),
            pairs(&[("command_prefix", "command"), ("command_prefix", "rm")])
        );
        // `eval` runs its arguments joined, as shell text.
        assert_eq!(
            shell(r#"{"program":"eval","args":["rm -rf x; ls"]}"#),
            pairs(&[
                ("command_prefix", "eval"),
                ("command_prefix", "rm"),
                ("command_prefix", "ls"),
            ])
        );
        // A busybox applet is the command it runs, and a shell applet reads its `-c` script.
        assert_eq!(
            shell(r#"{"program":"busybox","args":["sh","-c","rm -rf x"]}"#),
            pairs(&[
                ("command_prefix", "busybox"),
                ("command_prefix", "sh"),
                ("command_prefix", "rm"),
            ])
        );
        // A shell program given a script that runs a wrapper names what the script runs.
        assert_eq!(
            shell(r#"{"program":"/bin/sh","args":["-c","env rm x"]}"#),
            pairs(&[
                ("command_prefix", "/bin/sh"),
                ("command_prefix", "sh"),
                ("command_prefix", "env"),
                ("command_prefix", "rm"),
            ])
        );
    }

    #[test]
    fn a_wrapper_in_shell_text_names_the_command_it_runs_through_every_nesting() {
        let sh = |script: &str| {
            let input = serde_json::json!({ "language": "sh", "code": script }).to_string();
            targets_of("code_interpreter", &input)
        };
        assert_eq!(
            sh("env rm -rf x"),
            pairs(&[("command_prefix", "env"), ("command_prefix", "rm")])
        );
        // A shell given a script reads it, whether through `-c` or with options in front.
        assert_eq!(
            sh("sh -c 'rm -rf x'"),
            pairs(&[("command_prefix", "sh"), ("command_prefix", "rm")])
        );
        assert_eq!(
            sh(r#"bash -lc "env rm x""#),
            pairs(&[
                ("command_prefix", "bash"),
                ("command_prefix", "env"),
                ("command_prefix", "rm"),
            ])
        );
        assert_eq!(
            sh("busybox sh -c 'rm -rf x'"),
            pairs(&[
                ("command_prefix", "busybox"),
                ("command_prefix", "sh"),
                ("command_prefix", "rm"),
            ])
        );
        assert_eq!(
            sh("exec rm x"),
            pairs(&[("command_prefix", "exec"), ("command_prefix", "rm")])
        );
        assert_eq!(
            sh("eval 'rm -rf x'"),
            pairs(&[("command_prefix", "eval"), ("command_prefix", "rm")])
        );
        assert_eq!(
            sh("nohup /bin/rm x"),
            pairs(&[
                ("command_prefix", "nohup"),
                ("command_prefix", "/bin/rm"),
                ("command_prefix", "rm"),
            ])
        );
        // A wrapper's path, and a name that is quoted or escaped, are read as the shell reads them.
        assert_eq!(
            sh("/usr/bin/env rm x"),
            pairs(&[
                ("command_prefix", "/usr/bin/env"),
                ("command_prefix", "env"),
                ("command_prefix", "rm"),
            ])
        );
        assert_eq!(
            sh(r"\env rm x"),
            pairs(&[("command_prefix", "env"), ("command_prefix", "rm")])
        );
        // Wrappers nest, and the command each one runs is named.
        assert_eq!(
            sh("sudo -u root env -i nice -n 5 timeout 10 nohup rm x"),
            pairs(&[
                ("command_prefix", "sudo"),
                ("command_prefix", "env"),
                ("command_prefix", "nice"),
                ("command_prefix", "timeout"),
                ("command_prefix", "nohup"),
                ("command_prefix", "rm"),
            ])
        );
    }

    #[test]
    fn a_wrapper_option_the_table_does_not_list_withholds_the_command() {
        // An option a wrapper's table does not list may take a value, so the command after it cannot be told apart:
        // the withheld value is named, which covers every command.
        assert_eq!(
            targets_of("shell", r#"{"program":"nice","args":["-Z","rm","x"]}"#),
            pairs(&[("command_prefix", "nice"), ("command_prefix", "")])
        );
        let sh = |script: &str| {
            let input = serde_json::json!({ "language": "sh", "code": script }).to_string();
            targets_of("code_interpreter", &input)
        };
        assert_eq!(
            sh("env -S 'rm -rf x'"),
            pairs(&[("command_prefix", "env"), ("command_prefix", "")])
        );
        assert_eq!(
            sh("sudo -s 'rm x'"),
            pairs(&[("command_prefix", "sudo"), ("command_prefix", "")])
        );
        assert_eq!(
            sh("xargs --unknown rm"),
            pairs(&[("command_prefix", "xargs"), ("command_prefix", "")])
        );
    }

    #[test]
    fn a_wrapper_whose_arguments_a_quote_cuts_withholds_the_command() {
        // The separator inside the quotes cuts the text segment by segment, so the wrapper's command lies past the cut
        // and is withheld. The quote-aware reading takes the quoted word whole, and names the command it runs.
        let names = command_prefixes(r#"env -u "a;b" rm x"#);
        assert!(names.contains(&"env".to_owned()), "{names:?}");
        assert!(names.contains(&String::new()), "{names:?}");
        assert!(names.contains(&"rm".to_owned()), "{names:?}");
        // `eval` runs its joined arguments, and the cut goes on past them too.
        let names = command_prefixes(r#"eval env -u "a;" rm x"#);
        assert!(names.contains(&String::new()), "{names:?}");
        // A shell's script cut by a quote is read whole by the quote-aware reading, so nothing is withheld for it.
        let names = command_prefixes(r#"sudo bash -c "cd x && make""#);
        assert!(!names.contains(&String::new()), "{names:?}");
        assert!(names.contains(&"cd".to_owned()), "{names:?}");
        assert!(names.contains(&"make".to_owned()), "{names:?}");
    }

    #[test]
    fn a_nested_script_whose_quotes_a_separator_splits_names_the_command_it_runs() {
        // Each script is read whole through its escaped quotes, so the `rm` it runs is named, both as a shell's
        // script and as a wrapper's command, and in a `code_interpreter` `sh` program.
        for script in [
            r#"sh -c 'true;'\''r'\''m -rf x'"#,
            r#"sh -c "true;\"r\"m -rf x""#,
            r#"env sh -c "true;\"r\"m -rf x""#,
        ] {
            let names = command_prefixes(script);
            assert!(names.contains(&"rm".to_owned()), "{script}: {names:?}");
            let input = serde_json::json!({ "language": "sh", "code": script }).to_string();
            let named = targets_of("code_interpreter", &input);
            assert!(
                named.contains(&("command_prefix".to_owned(), "rm".to_owned())),
                "{script}: {named:?}"
            );
        }
    }

    /// Scripts that run `rm` in a shell while no reading names it: the readings part where the shell does not. A
    /// heredoc's body holds a quote, which the quote-aware reading takes to open one, so the command after it is read
    /// inside it. An ANSI-C string's `\'` is a quote's escape to bash and not to the readings. A case pattern's `)` ends
    /// a substitution for the readings, but not for the shell. Each withholds its value, or names `rm`.
    const PARTED_SCRIPTS: [&str; 9] = [
        "cat <<'EOF'\n'\nEOF\nsh -c 'true;'\\''r'\\''m -rf x'",
        "cat <<'EOF'\n'\nEOF\nenv sh -c 'true;'\\''r'\\''m -rf x'",
        "x=$'\\''; sh -c 'true;'\\''r'\\''m -rf x'",
        "x=$'\\'' sh -c 'true;'\\''r'\\''m -rf x'",
        "echo $'\\'' ; sh -c 'true;'\\''r'\\''m -rf x' #'",
        "sh -c \"x=\\$'\\\\''; sh -c 'true;'\\\\''r'\\\\''m -rf x'\"",
        "echo $(case x in a) rm -rf x;; esac)",
        "echo $(case x in a) sh -c 'true;'\\''r'\\''m -rf x';; esac)",
        "echo `case x in a) rm -rf x;; esac`",
    ];

    #[test]
    fn a_script_the_readings_part_on_is_withheld_or_read_whole() {
        for text in PARTED_SCRIPTS {
            let names = command_prefixes(text);
            assert!(
                names.contains(&String::new()) || names.contains(&"rm".to_owned()),
                "{text}: {names:?}"
            );
        }
    }

    #[test]
    fn a_parted_script_is_withheld_for_a_shell_program_and_a_sh_program_alike() {
        let names_rm_or_withholds = |named: &[(String, String)]| {
            named.iter().any(|(kind, value)| {
                kind == "command_prefix" && (value.is_empty() || value == "rm")
            })
        };
        for text in PARTED_SCRIPTS {
            let sh = serde_json::json!({ "language": "sh", "code": text }).to_string();
            let named = targets_of("code_interpreter", &sh);
            assert!(names_rm_or_withholds(&named), "{text}: {named:?}");
            let program =
                serde_json::json!({ "program": "bash", "args": ["-c", text] }).to_string();
            let named = targets_of("shell", &program);
            assert!(names_rm_or_withholds(&named), "{text}: {named:?}");
        }
    }

    #[test]
    fn a_dollar_quote_inside_quotes_or_a_case_word_outside_a_substitution_withholds_nothing() {
        // A `$'` inside quotes is no ANSI-C string, and a `case` word that no substitution holds breaks no reading.
        assert_eq!(command_prefixes("grep -E 'foo$' file"), ["grep"]);
        assert_eq!(command_prefixes(r#"echo "$'" x"#), ["echo"]);
        assert_eq!(
            command_prefixes(r#"echo "upper case" $(date)"#),
            ["echo", "date"]
        );
        // `make` runs nothing it is given, so a separator inside its quotes hides no command.
        assert!(!command_prefixes(r#"make "a;b" x"#).contains(&String::new()));
    }

    #[test]
    fn a_text_that_ends_inside_an_open_quote_withholds_its_value() {
        // The quote-aware reading ends inside the open quote, so what lies past it is not read: the value is withheld,
        // and the commands the text does name are named as well.
        let names = command_prefixes(r#"sh -c "rm -rf x"#);
        assert!(names.contains(&"rm".to_owned()), "{names:?}");
        assert!(names.contains(&String::new()), "{names:?}");
        assert_eq!(command_prefixes(r#"echo "x"#), ["echo", ""]);
        // A closed quote is no open quote, so nothing is withheld.
        assert_eq!(command_prefixes(r#"echo "x""#), ["echo"]);
    }

    #[test]
    fn a_substitution_is_read_as_a_script_of_its_own_whatever_quotes_it_holds() {
        // The quotes inside a substitution are its own, so they cannot hide the command that follows the quoted word, or
        // the command the substitution runs, whichever way its quotes are escaped.
        for text in [
            r#"echo "$(sh -c 'true;'\''r'\''m -rf x')""#,
            r#"X="$(echo "a b")" rm x"#,
            r#"x="$(date "+%F")" && rm -rf x"#,
            r#"(echo "$(sh -c 'true;'\''r'\''m -rf x')")"#,
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
            let input = serde_json::json!({ "language": "sh", "code": text }).to_string();
            let named = targets_of("code_interpreter", &input);
            assert!(
                named.contains(&("command_prefix".to_owned(), "rm".to_owned())),
                "{text}: {named:?}"
            );
        }
        // A substitution whose quotes hold nothing to run names only the commands it runs, and withholds nothing.
        assert_eq!(
            command_prefixes(r#"echo "$(date "+%F")""#),
            ["echo", "date"]
        );
        // A substitution that no `)` closes withholds its value, and the commands it does name are named.
        let names = command_prefixes(r#"foo $(rm x"#);
        assert!(names.contains(&String::new()), "{names:?}");
        assert!(names.contains(&"rm".to_owned()), "{names:?}");
    }

    #[test]
    fn a_comment_names_nothing_and_its_quotes_open_nothing() {
        // A `#` that begins a word starts a comment to the end of its line. An apostrophe in it opens no quote, so
        // nothing is withheld, and the commands after the line break are read as usual.
        assert_eq!(command_prefixes("echo hi # don't"), ["echo"]);
        let names = command_prefixes("# don't\necho \"$(sh -c 'true;'\\''r'\\''m -rf x')\"");
        assert!(names.contains(&"rm".to_owned()), "{names:?}");
        // A `#` inside a word, or after an escaped space, begins no comment, so the separator after it still splits.
        assert!(command_prefixes("echo a#b; rm x").contains(&"rm".to_owned()));
        assert!(command_prefixes("echo a\\ #; rm x").contains(&"rm".to_owned()));
    }

    #[test]
    fn a_nesting_past_the_bound_is_withheld_rather_than_followed() {
        // Each `env` runs the next word as a command. Past the bound the rest is withheld, which bounds the recursion.
        let deep = format!("{}rm x", "env ".repeat(MAX_SHELL_DEPTH + 4));
        assert!(command_prefixes(&deep).contains(&String::new()));
    }

    #[tokio::test]
    async fn a_denied_command_run_by_a_wrapper_is_denied_too() {
        for (tool, input) in [
            ("shell", r#"{"program":"env","args":["rm","-rf","x"]}"#),
            (
                "shell",
                r#"{"program":"sudo","args":["-u","root","rm","x"]}"#,
            ),
            (
                "shell",
                r#"{"program":"xargs","args":["-I","{}","rm","{}"]}"#,
            ),
            (
                "shell",
                r#"{"program":"busybox","args":["sh","-c","rm -rf x"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"sh -c 'rm -rf x'"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"env rm -rf x"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"eval 'rm -rf x'"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"true; timeout 5 rm -rf x"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"sh -c 'true;'\\''r'\\''m -rf x'"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"sh -c \"true;\\\"r\\\"m -rf x\""}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"env sh -c \"true;\\\"r\\\"m -rf x\""}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"echo \"$(sh -c 'true;'\\''r'\\''m -rf x')\""}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"X=\"$(echo \"a b\")\" rm x"}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert("rm".to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
        }
    }

    #[tokio::test]
    async fn a_withheld_wrapper_command_is_checked_as_every_command() {
        for (tool, input) in [
            ("shell", r#"{"program":"nice","args":["-Z","rm","x"]}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"env -u \"a;b\" rm x"}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert(String::new(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value.is_empty()
                }),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn a_parted_script_is_denied_when_its_withheld_value_is_denied() {
        for text in PARTED_SCRIPTS {
            for (tool, input) in [
                (
                    "code_interpreter",
                    serde_json::json!({ "language": "sh", "code": text }).to_string(),
                ),
                (
                    "shell",
                    serde_json::json!({ "program": "bash", "args": ["-c", text] }).to_string(),
                ),
            ] {
                let (mut policy, state) = harness(ToolClassGate::Enforce).await;
                policy.permission_rules = true;
                state
                    .decision_by_target
                    .lock()
                    .unwrap()
                    .insert(String::new(), "deny".to_owned());
                let evaluation = policy
                    .evaluate_tool_call_with_input(tool, &input, "run-a", "org-a", "user-a", false)
                    .await
                    .expect("a deny is an evaluation, not an error");
                assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
                let sent = state.evaluated.lock().unwrap();
                assert!(
                    sent.iter().any(|request| {
                        request.target_kind == "command_prefix" && request.target_value.is_empty()
                    }),
                    "{input}"
                );
            }
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_a_wrapper_names_no_command() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            ("shell", r#"{"program":"env","args":["rm","-rf","x"]}"#),
            ("shell", r#"{"program":"chroot","args":["/srv"]}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"sh -c 'rm -rf x'"}"#,
            ),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 3, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[test]
    fn xargs_names_the_command_it_runs_in_every_form() {
        // The command xargs runs is the first word its options leave, whatever those options are, so every form of
        // the call names `rm` beside `xargs`.
        for text in [
            "xargs rm",
            "xargs -I{} rm {}",
            "xargs -I {} rm {}",
            "xargs -0 -I{} -n1 rm {}",
            "xargs -n1 rm",
            "xargs -P 4 -n 1 -0 rm",
            "xargs -L1 -d, -- rm",
            "xargs -iREP rm REP",
            "xargs -a list -E END rm",
            "xargs -r -t rm",
            "xargs --max-args=1 --null rm",
            "xargs --replace={} rm {}",
        ] {
            assert_eq!(command_prefixes(text), ["xargs", "rm"], "{text}");
        }
        // Wrappers nest, and the command each one runs is named as well.
        assert_eq!(
            command_prefixes("env xargs sudo -u root rm x"),
            ["env", "xargs", "sudo", "rm"]
        );
        assert_eq!(
            command_prefixes("env -i xargs -0 -I{} sudo -u root rm {}"),
            ["env", "xargs", "sudo", "rm"]
        );
        assert_eq!(
            command_prefixes("sudo xargs -I{} rm {}"),
            ["sudo", "xargs", "rm"]
        );
        // A pipe feeds xargs, and the command after it is named.
        assert_eq!(
            command_prefixes("echo a | xargs -I{} rm {}"),
            ["echo", "xargs", "rm"]
        );
    }

    #[test]
    fn a_xargs_with_no_command_names_the_default_echo_and_one_with_a_command_does_not() {
        // xargs runs `echo` when it is given no command, so that default is named; once it is given one, it is not.
        assert_eq!(command_prefixes("xargs"), ["xargs", "echo"]);
        assert_eq!(command_prefixes("xargs -0 -I{}"), ["xargs", "echo"]);
        assert_eq!(command_prefixes("xargs -0 rm"), ["xargs", "rm"]);
    }

    #[test]
    fn a_clobber_redirect_is_no_pipe_so_the_command_after_its_target_is_named() {
        // `>|` (overwrite even under noclobber) is one redirection: the word after it is its target, and the command is
        // the word after that. A split at its `|` left xargs with no command, so only its default `echo` was named.
        for text in [
            "xargs >|x rm",
            "xargs >| x rm",
            "xargs 2>|x rm",
            "xargs >|x >|y rm",
            ">|x rm -rf y",
            "echo >|out && rm -rf y",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
        }
        // A `|` that is a pipe still ends a command: after a `>` and a space, after an escaped `>`, or after a quote.
        for text in ["echo > | rm", "echo \\>| rm", "echo '>' | rm"] {
            let names = command_prefixes(text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
        }
    }

    #[test]
    fn a_zsh_clobber_spelling_names_the_command_after_its_target_and_the_one_bash_runs() {
        // zsh reads `>!`, `>&!`, `>>!`, `>>&!`, `&>!` and `&>>!` as clobber operators: the `!` is part of the operator,
        // so the word after it is the target and the command is the word after that. `>&|`, `>>&|` and `&>>|` are
        // clobber operators with a `|`, as `>|` is. Bash and dash read the `!` as the first character of the target
        // instead, and run the word after it, so each reading names the command it runs.
        for text in [
            "xargs >! x rm",
            "xargs >&! x rm",
            "xargs >>! x rm",
            "xargs >>&! x rm",
            "xargs &>! x rm",
            "xargs &>>! x rm",
            "xargs >&| x rm",
            "xargs >>&| x rm",
            "xargs &>>| x rm",
            ">! x rm -rf y",
            ">&| x rm -rf y",
            "echo a >!b && rm -rf y",
            "echo a >!b; rm -rf y",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
        }
        // The word after the `!` is the command bash and dash run, and the command zsh runs is named beside it.
        assert_eq!(command_prefixes("xargs >! x rm"), ["xargs", "x", "rm"]);
        assert_eq!(command_prefixes(">! x rm -rf y"), ["x", "rm"]);
        // Read as zsh reads it, a target written against the `!` is a file, so it names no command.
        assert_eq!(command_prefixes("echo hi >!out.txt"), ["echo"]);
    }

    #[tokio::test]
    async fn a_denied_command_run_through_xargs_is_denied_whatever_its_form() {
        for (tool, input) in [
            ("shell", r#"{"program":"xargs","args":["rm"]}"#),
            (
                "shell",
                r#"{"program":"xargs","args":["-0","-I","{}","-n1","rm","{}"]}"#,
            ),
            (
                "shell",
                r#"{"program":"env","args":["xargs","sudo","-u","root","rm","x"]}"#,
            ),
            ("shell", r#"{"program":"xargs >|x rm"}"#),
            ("code_interpreter", r#"{"language":"sh","code":"xargs rm"}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs -I{} rm {}"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >| x rm"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"echo a >|b; >|x rm -rf y"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"echo a >&!b; xargs >>! x rm"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >&| x rm"}"#,
            ),
            ("shell", r#"{"program":"xargs >! x rm"}"#),
            (
                "shell",
                r#"{"program":"zsh","args":["-c",">! x rm -rf y"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >;x rm -rf y"}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","noglob xargs rm x"]}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert("rm".to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value == "rm"
                }),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn a_denied_command_run_by_an_option_value_program_is_denied_too() {
        // `fd`, `tar`, `rg`, `socat` and the netcat programs run the command their option or address gives, so a deny
        // on that command denies the call, and its target is sent.
        for (tool, input) in [
            ("shell", r#"{"program":"fd","args":["-x","rm","{}"]}"#),
            (
                "shell",
                r#"{"program":"fdfind","args":["-X","rm","{}","+"]}"#,
            ),
            (
                "shell",
                r#"{"program":"tar","args":["--to-command=rm","-x"]}"#,
            ),
            (
                "shell",
                r#"{"program":"tar","args":["-I","rm","-xf","a.tar"]}"#,
            ),
            (
                "shell",
                r#"{"program":"tar","args":["-cf","a.tar","--checkpoint-action=exec=rm","."]}"#,
            ),
            ("shell", r#"{"program":"rg","args":["--pre","rm","foo"]}"#),
            ("shell", r#"{"program":"socat","args":["-","SYSTEM:rm"]}"#),
            ("shell", r#"{"program":"nc","args":["-e","rm","host","1"]}"#),
            (
                "shell",
                r#"{"program":"ncat","args":["--sh-exec","rm x","host","1"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"tar --use-compress-program=rm -xf a.tar"}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert("rm".to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value == "rm"
                }),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn an_expansion_that_may_stand_for_an_option_is_checked_as_the_withheld_value() {
        // The shell expands `$A` before `tar` reads it, and `A` may hold `--to-command=rm`, so the call is checked as the
        // withheld value, and a deny on that value denies it. `fd -x sh -c 'rm {}'` runs the `rm` of each file name, and
        // is checked the same way.
        for (tool, input) in [
            ("shell", r#"{"program":"tar","args":["$A","-xf","a.tar"]}"#),
            (
                "shell",
                r#"{"program":"fd","args":["-x","sh","-c","rm {}"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"A=--to-command=rm; tar $A -xf a.tar"}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert(String::new(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value.is_empty()
                }),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn a_wrapper_and_its_command_are_each_checked_and_the_strictest_answer_decides() {
        // The wrapper's own name is a target too: an ask on `xargs` asks the whole call, whatever command it runs.
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert("xargs".to_owned(), "ask".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"xargs","args":["rm"]}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("an ask is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Ask);
        assert!(evaluation.ask_evidence_verified);
        assert_eq!(
            target_pairs(&state.evaluated.lock().unwrap()),
            pairs(&[("command_prefix", "xargs"), ("command_prefix", "rm")])
        );
        // A bare `xargs` runs `echo`, so a deny on `echo` denies the call.
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert("echo".to_owned(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"xargs"}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny);
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_a_zsh_clobber_names_no_command() {
        // The switch is off, so no call names a command, whichever reading of its clobber mark a shell would take.
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            (
                "shell",
                r#"{"program":"zsh","args":["-c",">! x rm -rf y"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >&| x rm"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >! x rm"}"#,
            ),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 3, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_an_xargs_call_names_no_command() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            ("shell", r#"{"program":"xargs","args":["-I{}","rm","{}"]}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >|x rm"}"#,
            ),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 2, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[test]
    fn a_coproc_names_the_command_it_runs_with_or_without_a_name() {
        // `coproc` takes a NAME only before a compound command. Otherwise its next word is the command it runs.
        for text in [
            "coproc rm x",
            "coproc { rm x; }",
            "coproc NAME { rm x; }",
            "coproc xargs rm",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
        }
        // zsh has no NAME, so the word after `coproc` is the command even before a brace: `coproc rm { x; }` runs rm.
        assert!(command_prefixes("coproc rm { x; }").contains(&"rm".to_owned()));
        // A NAME before a simple command is the command, as bash reads it: `coproc NAME rm x` runs NAME, not rm.
        assert_eq!(command_prefixes("coproc NAME rm x"), ["NAME"]);
    }

    #[test]
    fn a_function_keyword_reads_the_body_after_its_name() {
        // `function NAME` names a function, which is not run: the body after it is the command that runs.
        for text in [
            "function f { rm x; }; f",
            "function f { rm x; }",
            "function f() { rm x; }",
            "function f\n{ rm x; }\nf",
            "function f { xargs rm; }",
            // zsh's `function [ -T ] word ... { list }`: every word before the body is a name, and the body runs rm.
            "function -T f { rm x; }; f",
            "function a b { rm x; }; a",
            "function f g h { rm x; }",
            // A name may be a compound-start word, so the body after it is read too.
            "function for { rm x; }",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
        }
        assert!(command_prefixes("function f { xargs rm; }").contains(&"xargs".to_owned()));
        // With no body in the segment, each word after the keyword that is not a leading word is named, so a command
        // among them is named. With none, the keyword is named, so the segment never names nothing.
        assert!(command_prefixes("function f rm x").contains(&"rm".to_owned()));
        assert!(!command_prefixes("function -T").is_empty());
        // That body may lie past a cut, so a segment with a `function` and no compound-start word after it may hide
        // a command.
        let bodiless: Vec<String> = ["function", "f"].into_iter().map(str::to_owned).collect();
        assert!(may_hide_a_command(&bodiless));
    }

    #[test]
    fn a_trap_reads_its_action_as_shell_text() {
        // The action runs as shell text when the signal comes, so each command it names is named, quoted or not.
        for text in [
            "trap 'rm -rf x' EXIT",
            "trap \"rm -rf x\" EXIT",
            "trap -- 'rm -rf x' EXIT",
            "trap 'rm -rf x' INT TERM",
            "trap rm EXIT",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
        }
        // `trap -l` takes no action, and an empty action runs nothing.
        assert_eq!(command_prefixes("trap -l"), ["trap"]);
        assert_eq!(command_prefixes("trap '' INT"), ["trap"]);
        // An action that expands is withheld, as any command word that expands is.
        assert!(command_prefixes("trap '$CMD' EXIT").contains(&String::new()));
    }

    #[test]
    fn a_ksh_sync_mark_is_a_redirect_so_the_command_after_its_target_is_named() {
        // ksh's `>;` writes its target and syncs it. The `;` is no separator, so `rm` is the command after the target.
        for text in [
            "xargs >;x rm",
            "xargs >>;x rm",
            "xargs >&;x rm",
            "xargs 2>;x rm",
            "xargs &>;x rm",
            ">;x rm -rf y",
            "<>;x rm",
            "xargs <>;x rm",
            "<>;x rm -rf y",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
        }
        assert!(command_prefixes("xargs >;x rm").contains(&"xargs".to_owned()));
        // Read with the `;` as a separator, `echo a >; rm x` names rm as well, so either reading names it.
        assert!(command_prefixes("echo a >; rm x").contains(&"rm".to_owned()));
    }

    #[test]
    fn a_semicolon_quoted_escaped_or_spaced_from_its_redirect_still_separates_the_commands() {
        // Only a `;` right after an unquoted, unescaped `>` is the sync mark. Quoted, escaped or spaced from it, the `;`
        // separates the commands on either side, so the command after it is named as a command of its own.
        assert_eq!(command_prefixes("echo \">\";rm"), ["echo", "rm"]);
        assert_eq!(command_prefixes("echo '>';rm"), ["echo", "rm"]);
        assert_eq!(command_prefixes("echo \\>;rm"), ["echo", "rm"]);
        assert_eq!(command_prefixes("echo > ;rm"), ["echo", "rm"]);
        assert_eq!(command_prefixes("xargs \">\";rm"), ["xargs", "rm"]);
    }

    #[test]
    fn a_hash_after_a_sync_or_clobber_mark_is_no_comment_so_a_substitution_after_it_is_read() {
        // The `#` after `>;` or `>|` is read as the start of the target word, not a comment, as `quote_aware_segments` reads
        // it: a shell that reads it so runs the substitution in that target (ksh93 is not checked here). The substitution is
        // lifted and read, and its `>;x rm` (or `>|x rm`) names rm, as it does with no `#` after the mark.
        for text in [
            "xargs >;#>\"$(>;x rm)\"",
            "xargs >|#>\"$(>|x rm)\"",
            "xargs >;#>\"`>;x rm`\"",
            "xargs >>;#>\"$(>;x rm)\"",
            "xargs &>;#>\"$(>;x rm)\"",
            "xargs <>;#>\"$(>;x rm)\"",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
        }
        assert!(command_prefixes("xargs >#>\"$(>;x rm)\"").contains(&"rm".to_owned()));
        // After a blank, a `#` starts a comment in every shell, so the substitution after it runs nothing.
        assert!(!command_prefixes("xargs >; #>\"$(>;x rm)\"").contains(&"rm".to_owned()));
    }

    #[tokio::test]
    async fn a_denied_command_behind_a_ksh_sync_mark_is_denied_in_every_shell() {
        for (tool, input) in [
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >;x rm -rf y"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs <>;x rm -rf y"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >;#>\"$(>;x rm)\""}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >|#>\"$(>|x rm)\""}"#,
            ),
            ("shell", r#"{"program":"ksh","args":["-c","xargs >;x rm"]}"#),
            (
                "shell",
                r#"{"program":"ksh","args":["-c","xargs <>;x rm"]}"#,
            ),
            (
                "shell",
                r#"{"program":"ksh","args":["-c","xargs >;#>\"$(>;x rm)\""]}"#,
            ),
            ("shell", r#"{"program":"zsh","args":["-c",">;x rm"]}"#),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert("rm".to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value == "rm"
                }),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_a_ksh_sync_mark_names_no_command() {
        // The switch is off, so no call names a command: each call is one request with no tool, person or target.
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >;x rm -rf y"}"#,
            ),
            ("code_interpreter", r#"{"language":"sh","code":">;x rm"}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs <>;x rm"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >;#>\"$(>;x rm)\""}"#,
            ),
            ("shell", r#"{"program":"ksh","args":["-c","xargs >;x rm"]}"#),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 5, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[test]
    fn a_zsh_precommand_runs_the_command_after_it() {
        // zsh's `noglob` and `nocorrect` run the command after them, and `repeat COUNT` runs the command after its
        // count.
        assert_eq!(
            command_prefixes("noglob xargs rm"),
            ["noglob", "xargs", "rm"]
        );
        assert_eq!(
            command_prefixes("nocorrect xargs rm"),
            ["nocorrect", "xargs", "rm"]
        );
        assert_eq!(command_prefixes("repeat 2 rm x"), ["repeat", "rm"]);
        // `repeat COUNT do list done` opens its list with `do`, which runs no command: the list's command is named,
        // past any keyword the list opens with.
        assert_eq!(
            command_prefixes("repeat 2 do rm x; done"),
            ["repeat", "rm", "done"]
        );
        assert_eq!(command_prefixes("repeat 2 do ! rm x"), ["repeat", "rm"]);
        // Only `repeat` opens a list after its count: after `timeout`'s duration, `do` is the command it names.
        assert_eq!(command_prefixes("timeout 5 do rm x"), ["timeout", "do"]);
        // They take no options of their own, so an option one is given is withheld.
        assert!(command_prefixes("noglob -x rm").contains(&String::new()));
    }

    #[tokio::test]
    async fn a_denied_command_behind_a_keyword_or_a_precommand_is_denied_in_every_shell() {
        for (tool, input) in [
            (
                "code_interpreter",
                r#"{"language":"sh","code":"coproc rm x"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"function f { rm x; }; f"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"trap 'rm -rf x' EXIT"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs >;x rm -rf y"}"#,
            ),
            ("shell", r#"{"program":"bash","args":["-c","coproc rm x"]}"#),
            (
                "shell",
                r#"{"program":"ksh","args":["-c","function f { rm x; }; f"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","trap 'rm -rf x' EXIT"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","repeat 2 rm x"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","function -T f { rm x; }; f"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","repeat 2 do rm x; done"]}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert("rm".to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value == "rm"
                }),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_a_keyword_or_a_precommand_names_no_command() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            (
                "code_interpreter",
                r#"{"language":"sh","code":"coproc rm x"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"function f { rm x; }; f"}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","trap 'rm -rf x' EXIT"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","noglob xargs >;x rm"]}"#,
            ),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 4, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[test]
    fn a_zsh_function_reads_every_name_and_option_before_its_body() {
        // zsh's `function [-T] word ... { list }`: each word before the body is a name or an option, and runs nothing, so
        // the command in the body is named, and a later call of a name is named as its own command.
        assert_eq!(command_prefixes("function -T f { rm x; }; f"), ["rm", "f"]);
        assert_eq!(command_prefixes("function a b { rm x; }; a"), ["rm", "a"]);
        assert_eq!(command_prefixes("function f g h { rm x; }"), ["rm"]);
    }

    #[test]
    fn a_zsh_repeat_reads_its_sublist_past_a_reserved_word_after_the_count() {
        // The short form `repeat COUNT sublist` takes a sublist that may open with a reserved word, which runs no command,
        // so the command after such a word is named, as it is after `do` in the long form.
        assert_eq!(command_prefixes("repeat 2 ! rm x"), ["repeat", "rm"]);
        assert_eq!(command_prefixes("repeat 2 coproc rm x"), ["repeat", "rm"]);
        assert_eq!(command_prefixes("repeat 2 time rm x"), ["repeat", "rm"]);
        assert_eq!(command_prefixes("repeat 2 { rm x; }"), ["repeat", "rm"]);
        assert_eq!(
            command_prefixes("repeat 2 function f { rm x; }"),
            ["repeat", "rm"]
        );
        // `repeat` takes no option, so an option before its sublist is withheld, as the other programs' are.
        assert_eq!(command_prefixes("repeat 2 -x rm"), ["repeat", ""]);
    }

    #[test]
    fn a_builtin_runs_the_command_it_is_given() {
        // `builtin NAME args` runs the shell builtin NAME with its arguments, so the command it is given is read as that
        // command is: `eval`, `trap` and `exec` run what they are given. `builtin` takes no option of its own.
        for text in [
            "builtin eval 'rm x'",
            "builtin trap 'rm x' EXIT",
            "builtin exec rm x",
            "builtin command rm x",
            "builtin rm x",
            "builtin -- rm x",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
        }
        assert_eq!(
            command_prefixes("builtin eval 'rm x'"),
            ["builtin", "eval", "rm"]
        );
        assert_eq!(
            command_prefixes("builtin trap 'rm x' EXIT"),
            ["builtin", "trap", "rm"]
        );
        assert_eq!(
            command_prefixes("builtin exec rm x"),
            ["builtin", "exec", "rm"]
        );
        assert_eq!(command_prefixes("builtin -x rm"), ["builtin", ""]);
    }

    #[test]
    fn an_emulate_runs_the_text_after_its_dash_c_option_as_shell_text() {
        // zsh's `emulate [-LR] SHELL -c ARG` evaluates ARG as shell text, as `eval` does, so the commands of ARG are named.
        // The option is read wherever it stands among the words, so ARG is read whether `-c` follows the SHELL operand or not.
        assert_eq!(command_prefixes("emulate sh -c 'rm x'"), ["emulate", "rm"]);
        assert_eq!(
            command_prefixes("zsh -c \"emulate sh -c 'rm x'\""),
            ["zsh", "emulate", "rm"]
        );
        assert_eq!(
            command_prefixes("emulate -LR zsh -c 'echo rm'"),
            ["emulate", "echo"]
        );
        // Each word after the option is read on its own as well as joined, so the `sh` after the text is read too. That names
        // more than zsh runs, which is the direction the reading errs in: the names are never fewer.
        assert_eq!(
            command_prefixes("emulate -c 'rm x' sh"),
            ["emulate", "rm", "sh", ""]
        );
        assert!(command_prefixes("emulate sh -c 'ls && rm x'").contains(&"rm".to_owned()));
        // The option words zsh takes before the text (`+x`, and `-o NAME`) are among the words after the option, so the text
        // is named whichever word it is.
        assert!(command_prefixes("emulate sh -c +x 'rm x'").contains(&"rm".to_owned()));
        assert!(command_prefixes("emulate sh -co errexit 'rm x'").contains(&"rm".to_owned()));
        assert!(command_prefixes("emulate sh -c +o errexit 'rm x'").contains(&"rm".to_owned()));
        // zsh's parseopts reads the `c` of `+c` as the command flag too.
        assert!(command_prefixes("emulate sh +c 'rm x'").contains(&"rm".to_owned()));
        // After `--` or a lone `-` the next word is the text whatever it starts with, so an expanding word shaped like an
        // option (`-$x`) is withheld, not skipped as a leading word.
        assert!(command_prefixes("emulate sh -c -- \"-$x\"").contains(&String::new()));
        assert!(command_prefixes("emulate sh -c - \"-$x\"").contains(&String::new()));
        // An option with no text after it, or an emulation with no `-c`, runs nothing.
        assert_eq!(command_prefixes("emulate sh -c"), ["emulate"]);
        assert_eq!(command_prefixes("emulate -L zsh"), ["emulate"]);
        // A word of the text with an expansion is withheld, as `eval`'s is.
        assert_eq!(command_prefixes("emulate sh -c \"$X\""), ["emulate", ""]);
        // An expanding word that may stand for the `-c` option, or for the words that carry it, is withheld, as the shell
        // expands it before `emulate` reads it (`emulate_option_may_be_hidden`).
        assert!(command_prefixes("emulate sh $o 'rm x'").contains(&String::new()));
        assert!(command_prefixes("emulate $a").contains(&String::new()));
        assert!(command_prefixes("builtin emulate sh $o 'rm x'").contains(&String::new()));
        // A cut segment may have its `-c`, and the text after it, past the cut, so the withheld value is named.
        let words = vec!["emulate".to_owned()];
        assert_eq!(command_words_names(&words, 0, true), ["emulate", ""]);
        let open = command_prefixes("emulate sh -c 'rm x");
        assert!(open.contains(&"rm".to_owned()), "{open:?}");
        assert!(open.contains(&String::new()), "{open:?}");
        // An alias whose value is an emulation may get its `-c` from the words a call appends, so its value is withheld.
        assert!(command_prefixes("alias x='emulate sh'\nx -c 'rm y'").contains(&String::new()));
        // The program of a `shell` call names the same commands.
        assert_eq!(
            targets_of(
                "shell",
                r#"{"program":"emulate","args":["sh","-c","rm x"]}"#
            ),
            pairs(&[("command_prefix", "emulate"), ("command_prefix", "rm")])
        );
    }

    #[test]
    fn an_alias_reads_the_text_it_defines_as_shell_text() {
        // zsh expands an alias in a `-c` script, and bash does after `shopt -s expand_aliases`, so the command its value
        // runs is named where the alias is defined, as the text of `eval` is read.
        assert_eq!(
            command_prefixes("alias x='rm -rf y'\nx"),
            ["alias", "rm", "x"]
        );
        assert_eq!(
            command_prefixes("alias x='rm -rf y'; x"),
            ["alias", "rm", "x"]
        );
        assert_eq!(
            command_prefixes("alias x=\"rm -rf y\"\nx"),
            ["alias", "rm", "x"]
        );
        assert_eq!(
            command_prefixes("alias -g x='rm -rf y'\nx"),
            ["alias", "rm", "x"]
        );
        assert_eq!(
            command_prefixes("alias a='ls' b='rm -rf y'"),
            ["alias", "ls", "rm"]
        );
        // A value is shell text of its own: each command it runs is named, a pipeline's and a substitution's too.
        assert!(command_prefixes("alias x='echo a | rm y'").contains(&"rm".to_owned()));
        assert!(command_prefixes("alias x='$(rm y)'").contains(&"rm".to_owned()));
        // An alias that defines nothing, with no `=` or with `-p`, runs no command.
        assert_eq!(command_prefixes("alias x"), ["alias"]);
        assert_eq!(command_prefixes("alias -p"), ["alias"]);
        // An operand that starts with `-` is read as well, after `--` too: alias takes no option that carries a `=`.
        assert_eq!(
            command_prefixes("alias -- -x='rm -rf y'\n-x"),
            ["alias", "rm"]
        );
        assert_eq!(command_prefixes("alias -x='rm -rf y'"), ["alias", "rm"]);
        assert!(command_prefixes("alias -- -x=sudo\n-x rm").contains(&String::new()));
        // A value that a quote leaves open may go on past the cut, so it is withheld, as `eval`'s is.
        let open = command_prefixes("alias x='rm -rf y");
        assert!(open.contains(&"rm".to_owned()), "{open:?}");
        assert!(open.contains(&String::new()), "{open:?}");
    }

    #[test]
    fn a_zsh_repeat_reads_an_assignment_count_and_the_list_after_it() {
        // zsh reads the word after `repeat` as its count even when it is an assignment, and evaluates it: `repeat X=1 rm x`
        // runs `rm x` with X set to 1. A later word may be the count instead, so both readings are named.
        assert_eq!(command_prefixes("repeat X=1 rm x"), ["repeat", "rm", "x"]);
        assert_eq!(
            command_prefixes("repeat X=1 Y=2 rm x"),
            ["repeat", "rm", "x"]
        );
        assert!(command_prefixes("repeat X=1 ! rm x").contains(&"rm".to_owned()));
        assert!(command_prefixes("repeat X=1 coproc rm x").contains(&"rm".to_owned()));
        // An option after the assignment is withheld, as it is after a count.
        assert_eq!(command_prefixes("repeat X=1 -x rm"), ["repeat", ""]);
        // The list the assignment would count begins with `--`, which is an option, so its command is withheld.
        let dashes = command_prefixes("repeat X=1 -- rm x");
        assert!(dashes.contains(&String::new()), "{dashes:?}");
        assert!(dashes.contains(&"x".to_owned()), "{dashes:?}");
        // An assignment with no list after it names no command.
        assert_eq!(command_prefixes("repeat X=1"), ["repeat"]);
    }

    #[test]
    fn an_assignment_where_a_wrapper_operand_is_due_is_read_as_that_operand_too() {
        // `timeout`, `flock`, `chroot` and `taskset` take an operand before their command. `flock X=1 rm x` takes X=1 as
        // its lock file and runs `rm x`, so the assignment is read as that operand as well as a variable set.
        assert_eq!(command_prefixes("flock X=1 rm x"), ["flock", "rm", "x"]);
        assert_eq!(command_prefixes("timeout X=1 rm x"), ["timeout", "rm", "x"]);
        assert_eq!(command_prefixes("chroot X=1 rm x"), ["chroot", "rm", "x"]);
        assert_eq!(command_prefixes("taskset X=1 rm x"), ["taskset", "rm", "x"]);
        assert_eq!(command_prefixes("flock -x X=1 rm"), ["flock", "rm"]);
        // Past the operand, timeout execs the assignment as its command (`X=1`), so no `rm` runs and none is named.
        assert_eq!(command_prefixes("timeout 5 X=1 rm x"), ["timeout", "X"]);
        // `chrt` reads each word after its options as the priority, or, when it is not all digits, as the command it runs.
        assert!(command_prefixes("chrt X=1 rm x").contains(&"rm".to_owned()));
    }

    #[test]
    fn an_alias_value_that_runs_the_words_a_call_appends_withholds_them() {
        // A call runs its alias's value with the words the call gives after its name. The value's last command may run
        // those words (a wrapper, `eval`, a POSIX shell, `builtin`, `trap`, `find`, `alias`), or the value ends in no
        // command, so the call's own first word is one. Such a value is withheld.
        for text in [
            "alias s=sudo\ns rm x",
            "alias e=eval\ne 'rm x'",
            "alias b='bash -c'\nb 'rm x'",
            "alias bi=builtin\nbi eval 'rm x'",
            "alias t=trap\nt 'rm x' EXIT",
            "alias f=find\nf . -exec rm {} ;",
            "alias x=alias\nx y=rm",
            "alias x='echo a;'\nx rm",
            "alias x=''\nx rm",
            "alias x='FOO=1'\nx rm",
            "alias -- -x=sudo\n-x rm",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&String::new()), "{text}: {names:?}");
        }
        assert_eq!(
            command_prefixes("alias s=sudo\ns rm x"),
            ["alias", "sudo", "", "s"]
        );
        assert_eq!(
            command_prefixes("alias e=eval\ne 'rm x'"),
            ["alias", "eval", "", "e"]
        );
        // An operand that expands may be a NAME=VALUE no name gives, so it is withheld (V3c-ec-2-rel).
        for text in [
            "a='x=rm -rf y'; alias \"$a\"\nx",
            "alias $(printf x=rm)\nx -rf y",
            "alias `printf x=rm`\nx",
        ] {
            let names = command_prefixes(text);
            assert!(names.contains(&String::new()), "{text}: {names:?}");
        }
        // A value whose last command runs no word of a call's own withholds nothing.
        assert!(!command_prefixes("alias l=ls\nl -la").contains(&String::new()));
        assert!(!command_prefixes("alias x='rm -rf y'\nx").contains(&String::new()));
    }

    #[test]
    fn a_zsh_special_hash_array_defines_names_through_its_elements() {
        // `aliases=(name value ...)` defines an alias whose value runs when its name is used, and `functions+=(name body)`
        // a function. Each element is shell text, so a command in an element is named where the array is assigned, and
        // `NAME+=` is an assignment's operator, the append of bash and zsh, not part of the name.
        assert_eq!(command_prefixes("aliases=(x 'rm -rf y')"), ["x", "rm"]);
        assert_eq!(command_prefixes("aliases+=(x 'rm -rf y')"), ["x", "rm"]);
        assert_eq!(command_prefixes("galiases=(x 'rm -rf y')"), ["x", "rm"]);
        assert_eq!(command_prefixes("saliases=(x 'rm -rf y')"), ["x", "rm"]);
        assert_eq!(command_prefixes("functions+=(f 'rm x')"), ["f", "rm"]);
        assert_eq!(command_prefixes("aliases=(x 'rm -rf y')\nx"), ["x", "rm"]);
        // A value that a call's own words may run is withheld, as an alias's is (`alias_value_hides_a_command`).
        assert!(command_prefixes("aliases=(s sudo)\ns rm x").contains(&String::new()));
        // An array that no `)` closes may go on past the text, so its value is withheld too.
        let open = command_prefixes("aliases=(x 'rm -rf y'");
        assert!(open.contains(&"rm".to_owned()), "{open:?}");
        assert!(open.contains(&String::new()), "{open:?}");
        // A plain command, and an assignment with no command after it, name what they run as before.
        assert_eq!(command_prefixes("x -rf y"), ["x"]);
        // `A+=1` is an assignment in bash and zsh, and in a POSIX shell such as dash the command word `A+=1`, which names
        // itself as any command word does (`command_name`).
        assert_eq!(command_prefixes("A+=1"), ["A+"]);
        // An appending prefix (`A+=1 rm`) is an assignment, so the command after it is named, as in bash and zsh; a POSIX
        // shell also names the word `A+=1` as its command.
        assert_eq!(command_prefixes("A+=1 rm -rf x"), ["rm", "A+"]);
        assert_eq!(command_prefixes("A+=1 B=2 rm -rf x"), ["rm", "A+"]);
        // An array element's assignment is a prefix assignment too, and its subscript may hold a `=`. A POSIX shell reads the
        // word as a glob command, which is withheld.
        assert_eq!(command_prefixes("A[1]=x rm -rf y"), ["rm", ""]);
        assert_eq!(command_prefixes("A[x=1]+=y rm -rf z"), ["rm", ""]);
        // Without an `=` after the subscript, `A[1]` is a glob that names a file, so its value is withheld and nothing more.
        assert_eq!(command_prefixes("A[1] rm -rf z"), [""]);
        // Where a `#` follows a redirect's mark, it is the target's word and no comment, so an array after it is read.
        assert!(command_prefixes("echo >&# x; aliases=(x 'rm -rf y')").contains(&"rm".to_owned()));
        assert!(command_prefixes("echo >;# x; aliases=(x 'rm -rf y')").contains(&"rm".to_owned()));
        // A line continuation can join a name to its `=(`, which only the joined line shows, so the array is read there too.
        assert!(command_prefixes("aliases\\\n=(x 'rm -rf y')").contains(&"rm".to_owned()));
        // An array inside a substitution is read by the substitution's own text, and one in quotes is no array.
        assert!(command_prefixes("echo $(aliases=(x 'rm -rf y'))").contains(&"rm".to_owned()));
        assert!(!command_prefixes("echo \"aliases=(x 'rm -rf y')\"").contains(&"rm".to_owned()));
        // The same array in a tool's code is read the same way.
        assert_eq!(
            targets_of(
                "code_interpreter",
                r#"{"language":"sh","code":"aliases+=(x 'rm -rf y')"}"#
            ),
            pairs(&[("command_prefix", "x"), ("command_prefix", "rm")])
        );
    }

    #[test]
    fn a_subscripted_assignment_to_a_special_array_names_its_element() {
        // bash's `BASH_CMDS[NAME]=PATH` binds NAME to PATH in its command hash, and `BASH_ALIASES[NAME]=VALUE` defines an
        // alias. zsh's `aliases`, `galiases`, `saliases`, `functions` and `commands` take the same subscripted form, and each
        // element is read as its hash reads it (`hash_element_names`): the key is the name it defines, and the value is what
        // a call of that name runs. Such a word before a command is an assignment, so it defines its element; as an argument
        // it does not.
        // Each subscripted word is also a command word in a POSIX shell, where it is a glob, so its withheld value is named too.
        assert_eq!(
            command_prefixes("BASH_CMDS[x]=/bin/rm; x -rf y"),
            ["x", "/bin/rm", "rm", ""]
        );
        assert_eq!(
            command_prefixes("BASH_ALIASES[x]='rm -rf y'\nx"),
            ["x", "rm", ""]
        );
        assert_eq!(
            command_prefixes("BASH_ALIASES[x]+='rm -rf y'"),
            ["x", "rm", ""]
        );
        assert_eq!(command_prefixes("aliases[x]='rm -rf y'"), ["x", "rm", ""]);
        assert_eq!(command_prefixes("galiases[x]='rm -rf y'"), ["x", "rm", ""]);
        assert_eq!(command_prefixes("saliases[x]='rm -rf y'"), ["x", "rm", ""]);
        assert_eq!(command_prefixes("functions[f]='rm x'"), ["f", "rm", ""]);
        assert_eq!(
            command_prefixes("commands[x]=/bin/rm"),
            ["x", "/bin/rm", "rm", ""]
        );
        // A value that a call's own words may run is withheld, as an alias's is (`alias_value_hides_a_command`).
        assert!(command_prefixes("aliases[s]=sudo\ns rm x").contains(&String::new()));
        // Where no assignment stands, as an argument of a command, the subscripted word defines nothing.
        assert_eq!(command_prefixes("echo aliases[x]='rm -rf y'"), ["echo"]);
        // An element of an array that defines no name runs nothing, and an assignment before a command leaves that command
        // named: `A[1]=x rm -rf y` names `rm`, and `BASH_CMDS[1]=x` binds `1` to `x` and names the withheld value of `x`.
        // A POSIX shell runs `A[1]=/bin/rm` as a glob command word, so the withheld value is named there too (`A1=` may be a
        // symlink to `/`), and the command after it is `x`.
        assert_eq!(command_prefixes("A[1]=/bin/rm x"), ["x", ""]);
        assert_eq!(command_prefixes("A[1]=x rm -rf y"), ["rm", ""]);
        assert_eq!(
            command_prefixes("BASH_CMDS[1]=x rm -rf y"),
            ["1", "x", "", "rm"]
        );
        // A body opened by `function` or by a `coproc` NAME is read from its opening word on, so its assignments are read.
        assert_eq!(
            command_prefixes("function f { BASH_CMDS[x]=/bin/rm; }; x -rf y"),
            ["x", "/bin/rm", "rm", ""]
        );
        assert_eq!(
            command_prefixes("coproc NAME { BASH_CMDS[x]=/bin/rm; }; x -rf y"),
            ["x", "/bin/rm", "rm", "NAME", ""]
        );
    }

    #[test]
    fn a_plus_or_subscripted_assignment_is_also_the_command_a_posix_shell_runs() {
        // `NAME+=VALUE` and `NAME[sub]=VALUE` are assignments in bash and zsh, but in dash they are the command word: `A+=/bin/rm`
        // runs the path `A+=/bin/rm`, which a symlink `A+=` to `/` resolves to `/bin/rm`, and `A[1]=/bin/rm` is a glob. The
        // reading as an assignment names the command after the word, and the reading as a command word names the word, so both
        // are named.
        assert_eq!(command_prefixes("A+=/bin/rm -rf y"), ["y", "A+", "rm", ""]);
        assert_eq!(command_prefixes("A[1]=/bin/rm x"), ["x", ""]);
        assert_eq!(
            command_prefixes("ln -s / A+=; A+=/bin/rm -rf y"),
            ["ln", "y", "A+", "rm", ""]
        );
        assert_eq!(
            command_prefixes("nohup A+=/bin/rm x"),
            ["nohup", "A+", "rm", ""]
        );
        // A word after a keyword, after a redirection or after `time` is read as a command word the same way.
        assert!(command_prefixes("if A+=/bin/rm x; then :; fi").contains(&"rm".to_owned()));
        assert!(command_prefixes("time A+=/bin/rm x").contains(&"rm".to_owned()));
        assert!(command_prefixes("2>f A+=/bin/rm x").contains(&"rm".to_owned()));
        // Negative controls. A plain assignment is an assignment only, so the command after it is the one named, and `env` and
        // `sudo` read such a word as a variable set for the command they run.
        assert_eq!(command_prefixes("A=1 rm x"), ["rm"]);
        assert_eq!(command_prefixes("env A+=/bin/rm x"), ["env", "x"]);
        assert_eq!(command_prefixes("env A[1]=/bin/rm x"), ["env", "x"]);
        // The shell tool's program runs with no shell, so its extended assignment word is the program, named as a command.
        assert_eq!(
            targets_of("shell", r#"{"program":"A+=/bin/rm","args":["-rf","y"]}"#),
            pairs(&[
                ("command_prefix", "A+"),
                ("command_prefix", "rm"),
                ("command_prefix", ""),
            ])
        );
    }

    #[test]
    fn a_declaring_builtin_defines_the_element_of_each_subscripted_operand() {
        // `typeset`, `declare` and the other builtins that declare their operands define an element of a special array from a
        // `NAME[KEY]=VALUE` operand, as a leading assignment does: `typeset aliases[x]='rm -rf y'` defines the alias `x`, and a
        // call of `x` runs `rm -rf y`.
        assert_eq!(
            command_prefixes("typeset aliases[x]='rm -rf y'; x"),
            ["typeset", "x", "rm"]
        );
        assert_eq!(
            command_prefixes("declare BASH_CMDS[x]=/bin/rm; x"),
            ["declare", "x", "/bin/rm", "rm"]
        );
        assert_eq!(
            command_prefixes("readonly BASH_CMDS[1]=x"),
            ["readonly", "1", "x", ""]
        );
        assert_eq!(
            command_prefixes("integer aliases[x]=rm"),
            ["integer", "x", "rm"]
        );
        // A value that a call's own words may run is withheld, as an alias's is.
        assert!(command_prefixes("local -x aliases[s]=sudo\ns rm x").contains(&String::new()));
        // An option or a name with no subscript defines no element, and so does an argument of a command that declares nothing.
        assert_eq!(command_prefixes("typeset -A aliases"), ["typeset"]);
        assert_eq!(command_prefixes("typeset X=1 rm x"), ["typeset"]);
        assert_eq!(
            command_prefixes("echo typeset aliases[x]='rm -rf y'"),
            ["echo"]
        );
    }

    #[test]
    fn a_hash_binds_a_name_to_the_path_it_names_and_that_path_is_read() {
        // `hash NAME=PATH` (zsh) and `hash -p PATH NAME` (bash) bind NAME to PATH, so a later `NAME -rf y` runs the program
        // at PATH: the path is read as a command word is, and a path outside the system binary directories is withheld.
        assert_eq!(
            command_prefixes("hash x=/bin/rm"),
            ["hash", "/bin/rm", "rm"]
        );
        assert_eq!(
            command_prefixes("hash -p /bin/rm x"),
            ["hash", "/bin/rm", "rm"]
        );
        assert_eq!(
            command_prefixes("hash x=/bin/rm\nx -rf y"),
            ["hash", "/bin/rm", "rm", "x"]
        );
        assert_eq!(
            command_prefixes("hash -p ./x.sh x"),
            ["hash", "./x.sh", "x.sh", ""]
        );
        assert_eq!(
            command_prefixes("hash x=/workspace/y"),
            ["hash", "/workspace/y", "y", ""]
        );
        // A path to a program that runs the words a call gives it is withheld too (`alias_value_hides_a_command`).
        assert!(command_prefixes("hash x=/usr/bin/env").contains(&String::new()));
        // zsh's `commands` hash binds names to paths the same way, from an array.
        assert!(command_prefixes("commands=(x /bin/rm)").contains(&"rm".to_owned()));
        assert!(command_prefixes("commands=(x /usr/bin/env)").contains(&String::new()));
        // A `hash` that binds nothing names nothing more.
        assert_eq!(command_prefixes("hash rm"), ["hash"]);
        assert_eq!(command_prefixes("hash -r"), ["hash"]);
        // A `-p` with no path, or a cut segment whose path may lie past the cut, withholds its value.
        assert_eq!(command_prefixes("hash -p"), ["hash", ""]);
        let words: Vec<String> = ["hash", "-p"]
            .iter()
            .map(|word| (*word).to_owned())
            .collect();
        assert_eq!(command_words_names(&words, 0, true), ["hash", ""]);
    }

    #[test]
    fn a_hash_option_that_clusters_or_attaches_its_path_binds_that_path_too() {
        // bash's `hash` reads its options as getopt does (`dlp:rt`), so `-p` may follow other option letters, or be attached
        // to its path: `-rp /bin/rm`, `-p/bin/rm` and `-rp/bin/rm` bind NAME to `/bin/rm`, as `-p /bin/rm` does.
        assert_eq!(
            command_prefixes("hash -p/bin/rm x\nx -rf y"),
            ["hash", "/bin/rm", "rm", "x"]
        );
        assert_eq!(
            command_prefixes("hash -rp/bin/rm x"),
            ["hash", "/bin/rm", "rm"]
        );
        for option in ["-rp", "-tp", "-lp", "-dp"] {
            let text = format!("hash {option} /bin/rm x\nx -rf y");
            let names = command_prefixes(&text);
            assert!(names.contains(&"rm".to_owned()), "{text}: {names:?}");
        }
        // The rest of the word is the path, so `-pr` binds to `r`, and a `-rp` with no path after it withholds one.
        assert_eq!(command_prefixes("hash -pr x"), ["hash", "r", ""]);
        assert_eq!(command_prefixes("hash -rp"), ["hash", ""]);
        // Options without a `p` name no path, and after `--` a word binds nothing but a `NAME=PATH`.
        assert_eq!(command_prefixes("hash -tr x"), ["hash"]);
        assert_eq!(command_prefixes("hash -- -p /bin/rm"), ["hash"]);
        assert_eq!(
            command_prefixes("hash -- x=/bin/rm"),
            ["hash", "/bin/rm", "rm"]
        );
    }

    #[test]
    fn a_dot_or_source_builtin_withholds_the_file_it_runs() {
        // `. FILE` and `source FILE` run the commands of FILE in the shell, so FILE is read as a script run by its path is:
        // its path and basename are named, and the withheld value covers its commands. The builtin's own name is named too.
        assert_eq!(command_prefixes(". ./x.sh"), [".", "./x.sh", "x.sh", ""]);
        assert_eq!(command_prefixes(". x.sh"), [".", "x.sh", ""]);
        assert_eq!(
            command_prefixes("source ./x.sh rm -rf y"),
            ["source", "./x.sh", "x.sh", ""]
        );
        // A system binary's path is the image's own file, so it is named without the withheld value.
        assert_eq!(command_prefixes(". /usr/bin/x"), [".", "/usr/bin/x", "x"]);
        assert_eq!(
            command_prefixes("echo rm | . /dev/stdin"),
            ["echo", ".", "/dev/stdin", "stdin", ""]
        );
        // An option, such as bash's `-p path`, and a `--` before the file withhold the file too.
        assert!(command_prefixes(". -p /usr/bin ./x.sh").contains(&String::new()));
        assert!(command_prefixes(". -- ./x.sh").contains(&String::new()));
        // With no file the builtin runs nothing, and a cut segment may have its file past the cut.
        assert_eq!(command_prefixes("."), ["."]);
        let words = vec![".".to_owned()];
        assert_eq!(command_words_names(&words, 0, true), [".", ""]);
        assert_eq!(
            targets_of("shell", r#"{"program":".","args":["./x.sh"]}"#),
            pairs(&[
                ("command_prefix", "."),
                ("command_prefix", "./x.sh"),
                ("command_prefix", "x.sh"),
                ("command_prefix", ""),
            ])
        );
    }

    #[test]
    fn dot_or_source_runs_the_words_after_its_file_as_a_run_by_path_would() {
        // `. FILE ARGS` runs FILE with ARGS, so a system file that is a wrapper runs the command it is given, as a run by its
        // path does: `eatmydata` and `setsid` exec the words after them. A system file of a program withheld whole, such as
        // `xvfb-run`, names that program and the withheld value. A system file that runs nothing it is given names no more
        // than its path does, and a file outside the system directories keeps its withheld value.
        assert_eq!(
            command_prefixes(". /usr/bin/eatmydata rm -rf x"),
            [".", "/usr/bin/eatmydata", "eatmydata", "rm"]
        );
        assert_eq!(
            command_prefixes("source /usr/bin/setsid rm x"),
            ["source", "/usr/bin/setsid", "setsid", "rm"]
        );
        assert_eq!(
            command_prefixes("source /usr/bin/xvfb-run rm x"),
            ["source", "/usr/bin/xvfb-run", "xvfb-run", ""]
        );
        assert_eq!(command_prefixes(". /usr/bin/x"), [".", "/usr/bin/x", "x"]);
        assert_eq!(
            command_prefixes(". /usr/bin/x rm -rf y"),
            [".", "/usr/bin/x", "x"]
        );
        // An option that the wrapper's table leaves out withholds its command, as its own run does.
        assert_eq!(
            command_prefixes(". /usr/bin/eatmydata -v rm x"),
            [".", "/usr/bin/eatmydata", "eatmydata", ""]
        );
        assert_eq!(
            command_prefixes(". ./eatmydata rm x"),
            [".", "./eatmydata", "eatmydata", "", "rm"]
        );
        assert_eq!(
            targets_of(
                "shell",
                r#"{"program":".","args":["/usr/bin/eatmydata","rm","-rf","x"]}"#
            ),
            pairs(&[
                ("command_prefix", "."),
                ("command_prefix", "/usr/bin/eatmydata"),
                ("command_prefix", "eatmydata"),
                ("command_prefix", "rm"),
            ])
        );
    }

    #[test]
    fn newgrp_and_arch_chroot_start_a_shell_so_their_value_is_withheld() {
        // `newgrp` starts a shell that reads its commands from stdin, and `arch-chroot` runs a shell, or the command it is
        // given, in a new root. Each names itself and the withheld value, as `schroot` and `systemd-nspawn` do.
        assert_eq!(
            command_prefixes("echo 'rm x' | newgrp"),
            ["echo", "newgrp", ""]
        );
        assert_eq!(command_prefixes("newgrp"), ["newgrp", ""]);
        assert_eq!(command_prefixes("newgrp mygroup"), ["newgrp", ""]);
        assert_eq!(
            command_prefixes("/usr/bin/newgrp -"),
            ["/usr/bin/newgrp", "newgrp", ""]
        );
        assert_eq!(
            command_prefixes("arch-chroot /mnt rm x"),
            ["arch-chroot", ""]
        );
        assert_eq!(command_prefixes("arch-chroot /mnt"), ["arch-chroot", ""]);
    }

    #[test]
    fn chroot_reads_no_option_or_assignment_after_its_newroot() {
        // coreutils' `chroot` takes its options before NEWROOT only, so the words after it are the command and its
        // arguments: an option-shaped word is that command, and NAME=VALUE is a program of that name, not a variable set.
        assert_eq!(
            command_prefixes("chroot /srv --userspec x rm"),
            ["chroot", "--userspec"]
        );
        assert_eq!(command_prefixes("chroot /srv X=1 rm x"), ["chroot", "X"]);
        assert_eq!(command_prefixes("chroot /srv rm -rf x"), ["chroot", "rm"]);
        // The options before NEWROOT are still read, so the command after it is named.
        assert_eq!(
            command_prefixes("chroot --userspec=0:0 /srv rm x"),
            ["chroot", "rm"]
        );
    }

    #[test]
    fn a_wrapper_that_execs_its_command_runs_a_name_value_word_as_that_command() {
        // `nohup`, `nice`, `xargs`, `command`, `exec` and `eatmydata` run their command with `execvp`, so `A=/bin/rm` is a
        // program run by that path, not a variable set. It names `A`, the basename `rm` and the withheld value, as a
        // command word does.
        assert_eq!(
            command_prefixes("nohup A=/bin/rm x"),
            ["nohup", "A", "rm", ""]
        );
        assert_eq!(
            command_prefixes("ln -s / A=; nohup A=/bin/rm x"),
            ["ln", "nohup", "A", "rm", ""]
        );
        assert_eq!(
            command_prefixes("nice -n 1 A=/bin/rm x"),
            ["nice", "A", "rm", ""]
        );
        assert_eq!(
            command_prefixes("xargs A=/bin/rm"),
            ["xargs", "A", "rm", ""]
        );
        assert_eq!(
            command_prefixes("command A=/bin/rm x"),
            ["command", "A", "rm", ""]
        );
        assert_eq!(
            command_prefixes("exec A=/bin/rm x"),
            ["exec", "A", "rm", ""]
        );
        assert_eq!(
            command_prefixes("eatmydata A=/bin/rm x"),
            ["eatmydata", "A", "rm", ""]
        );
        // The program a NAME=VALUE word names runs no command of its own, so the word after it is only its argument.
        assert_eq!(command_prefixes("nohup A=1 rm x"), ["nohup", "A"]);
        // env and sudo read an assignment as a variable set for the command they run.
        assert_eq!(command_prefixes("env A=1 rm x"), ["env", "rm"]);
        assert_eq!(command_prefixes("sudo A=1 rm x"), ["sudo", "rm"]);
    }

    #[test]
    fn env_takes_any_word_with_an_equals_sign_as_a_variable_set_whatever_its_name() {
        // GNU env (coreutils 8.32) reads every word with an `=` before its command as a variable set, whatever the name
        // before the `=`: `env 1=x rm` sets the variable `1` and runs `rm`, so `rm` is named as it is after `env A=1`.
        // `A-B` and `A.B` are such names too, after options and after `--`.
        assert_eq!(command_prefixes("env 1=x rm -rf y"), ["env", "rm"]);
        assert_eq!(command_prefixes("env A-B=x rm -rf y"), ["env", "rm"]);
        assert_eq!(command_prefixes("env A.B=1 rm -rf y"), ["env", "rm"]);
        assert_eq!(command_prefixes("env -i 1=x A-B=2 rm x"), ["env", "rm"]);
        assert_eq!(command_prefixes("env -- 1=x rm x"), ["env", "rm"]);
        assert_eq!(
            targets_of(
                "shell",
                r#"{"program":"env","args":["1=x","rm","-rf","x"]}"#
            ),
            pairs(&[("command_prefix", "env"), ("command_prefix", "rm")])
        );
        // A word with an expansion may split into a command, so it is withheld as it was before.
        assert_eq!(command_prefixes("env 1=$Y rm x"), ["env", ""]);
        assert_eq!(command_prefixes("env $X=1 rm x"), ["env", ""]);
        // sudo's reading of such a word is not verified here, so the word is also read as the command it may be, and the
        // `rm` after it is named.
        assert_eq!(command_prefixes("sudo 1=x rm -rf y"), ["sudo", "1", "rm"]);
        assert_eq!(
            command_prefixes("sudo A-B=x rm -rf y"),
            ["sudo", "A-B", "rm"]
        );
        // nocorrect reads only the assignments the shell reads, so `1=x` is the command it runs, as it is in zsh.
        assert_eq!(command_prefixes("nocorrect 1=x rm x"), ["nocorrect", "1"]);
    }

    #[test]
    fn noglob_reads_its_word_as_the_command_it_runs_and_as_a_variable() {
        // zsh's `noglob` is a builtin, not a reserved word, so `noglob A=/bin/rm x` runs the path `A=/bin/rm` (a symlink `A=`
        // to `/` resolves it to `/bin/rm`), and its word may also be a variable set for the command after it. Both are read.
        assert_eq!(
            command_prefixes("ln -s / A=; noglob A=/bin/rm x"),
            ["ln", "noglob", "A", "rm", "", "x"]
        );
        assert_eq!(command_prefixes("noglob A=1 rm x"), ["noglob", "A", "rm"]);
        assert_eq!(command_prefixes("noglob A=1"), ["noglob", "A"]);
        // `nocorrect` is a reserved word that reads its assignments as variable sets, so the command after them is named.
        assert_eq!(command_prefixes("nocorrect A=1 rm x"), ["nocorrect", "rm"]);
        // A command after `noglob` with no assignment is named as before.
        assert_eq!(command_prefixes("noglob rm x"), ["noglob", "rm"]);
    }

    #[test]
    fn a_line_continuation_after_a_redirect_is_read_as_the_joined_line_is() {
        // `>\<LF>;` joins to `>;`, the ksh sync mark, so the `;` is no separator and the `#` after it is a word, not a
        // comment. The substitution after that `#` is lifted from the text as written, so its commands and its withheld
        // value are read: the command in `>;x rm` is `rm`, and `x` is its redirect's target.
        assert!(command_prefixes("xargs >\\\n;#>\"$(>;x rm)\"").contains(&"rm".to_owned()));
        let open = command_prefixes("echo >\\\n;# $(rm x");
        assert!(open.contains(&"rm".to_owned()), "{open:?}");
        assert!(open.contains(&String::new()), "{open:?}");
        let body = command_prefixes("echo >\\\n;# $(case x in x) rm;; esac)");
        assert!(body.contains(&String::new()), "{body:?}");
        // Without the continuation the same text is read as before.
        assert!(command_prefixes("xargs >;#>\"$(>;x rm)\"").contains(&"rm".to_owned()));
    }

    #[test]
    fn a_nested_chain_of_assignment_operands_is_read_within_a_bound() {
        // An assignment where a wrapper's operand is due is read both ways, so each such wrapper in a chain doubles the
        // readings. Past `MAX_HEDGED_DEPTH` nested wrappers the rest is withheld, which covers every command, so a chain of
        // eight is read at once. A shallow chain still names the command it runs.
        let repeats = format!("{}rm x", "repeat X=1 ".repeat(8));
        let names = command_prefixes(&repeats);
        assert!(names.contains(&String::new()), "{names:?}");
        assert!(names.contains(&"repeat".to_owned()), "{names:?}");
        let timeouts = format!("{}rm x", "timeout X=1 ".repeat(8));
        assert!(command_prefixes(&timeouts).contains(&String::new()));
        assert!(command_prefixes("repeat X=1 repeat X=1 rm x").contains(&"rm".to_owned()));
        assert!(command_prefixes("timeout X=1 timeout X=1 rm x").contains(&"rm".to_owned()));
    }

    #[tokio::test]
    async fn a_withheld_alias_value_is_denied_when_its_withheld_value_is_denied() {
        for (tool, input) in [
            (
                "shell",
                r#"{"program":"zsh","args":["-c","alias s=sudo\ns rm x"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"alias e=eval\ne 'rm x'"}"#,
            ),
            (
                "shell",
                r#"{"program":"bash","args":["-c","alias b='bash -c'\nb 'rm x'"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","alias bi=builtin\nbi eval 'rm x'"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","a='x=rm -rf y'; alias \"$a\"\nx"]}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert(String::new(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value.is_empty()
                }),
                "{input}"
            );
        }
        // A value that hides no command is checked by its own commands alone, so the withheld value is never asked about.
        let (mut policy, state) = harness(ToolClassGate::Off).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert(String::new(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"zsh","args":["-c","alias l=ls\nl -la"]}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("an allow is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Allow);
        let sent = state.evaluated.lock().unwrap();
        assert!(sent.iter().all(|request| !request.target_value.is_empty()));
    }

    #[tokio::test]
    async fn a_denied_command_bound_by_a_special_array_a_dot_file_or_noglob_is_denied() {
        for input in [
            r#"{"program":"bash","args":["-c","BASH_CMDS[x]=/bin/rm; x -rf y"]}"#,
            r#"{"program":"bash","args":["-c","function f { BASH_CMDS[x]=/bin/rm; }; x -rf y"]}"#,
            r#"{"program":"bash","args":["-c","BASH_ALIASES[x]='rm -rf y'\nx"]}"#,
            r#"{"program":"zsh","args":["-c","aliases[x]='rm -rf y'\nx"]}"#,
            r#"{"program":"zsh","args":["-c","functions[f]='rm x'\nf"]}"#,
            r#"{"program":"zsh","args":["-c","commands[x]=/bin/rm\nx -rf y"]}"#,
            r#"{"program":"zsh","args":["-c","ln -s / A=; noglob A=/bin/rm x"]}"#,
            r#"{"program":"zsh","args":["-c","noglob A=1 rm x"]}"#,
            r#"{"program":"bash","args":["-c",". /usr/bin/eatmydata rm -rf x"]}"#,
            r#"{"program":"bash","args":["-c","source /usr/bin/setsid rm x"]}"#,
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert("rm".to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input("shell", input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value == "rm"
                }),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn a_denied_command_behind_a_builtin_repeat_or_alias_is_denied_in_every_shell() {
        for (tool, input) in [
            (
                "shell",
                r#"{"program":"zsh","args":["-c","repeat 2 ! rm x"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","repeat 2 coproc rm x"]}"#,
            ),
            (
                "shell",
                r#"{"program":"bash","args":["-c","builtin eval 'rm x'"]}"#,
            ),
            (
                "shell",
                r#"{"program":"bash","args":["-c","builtin trap 'rm -rf x' EXIT"]}"#,
            ),
            (
                "shell",
                r#"{"program":"bash","args":["-c","builtin exec rm x"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","alias x='rm -rf y'\nx"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"alias x='rm -rf y'\nx"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"builtin eval 'rm x'"}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","repeat X=1 rm x"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","alias -- -x='rm -rf y'\n-x"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","emulate sh -c 'rm x'"]}"#,
            ),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","emulate sh -c +x 'rm x'"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"emulate -L zsh -c 'rm x'"}"#,
            ),
            ("shell", r#"{"program":"flock","args":["X=1","rm","x"]}"#),
            ("shell", r#"{"program":"timeout","args":["X=1","rm","x"]}"#),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert("rm".to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value == "rm"
                }),
                "{input}"
            );
        }
        // A `-c` that an expansion stands for hides the text it runs, so the withheld value is named for it, and a deny on
        // that value denies the call (`emulate_option_may_be_hidden`). `rm` is not named, as the text is not read.
        let input = r#"{"program":"zsh","args":["-c","o=-c; emulate sh $o 'rm x'"]}"#;
        let (mut policy, state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        state
            .decision_by_target
            .lock()
            .unwrap()
            .insert(String::new(), "deny".to_owned());
        let evaluation = policy
            .evaluate_tool_call_with_input("shell", input, "run-a", "org-a", "user-a", false)
            .await
            .expect("a deny is an evaluation, not an error");
        assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
        let sent = state.evaluated.lock().unwrap();
        assert!(
            sent.iter().any(|request| {
                request.target_kind == "command_prefix" && request.target_value.is_empty()
            }),
            "{input}"
        );
    }

    #[tokio::test]
    async fn a_denied_command_named_by_a_dash_word_or_a_declaring_builtin_is_denied() {
        // Each call's command is named by the reading a POSIX shell (dash, or sh) gives its extended-assignment word, or by the
        // declaring builtin that defines the element, and the denied name is one of those names.
        for (input, denied) in [
            (
                r#"{"program":"sh","args":["-c","ln -s / A+=; A+=/bin/rm -rf y"]}"#,
                "rm",
            ),
            (
                r#"{"program":"sh","args":["-c","ln -s / A1=; A[1]=/bin/rm x"]}"#,
                "",
            ),
            (r#"{"program":"A+=/bin/rm","args":["-rf","y"]}"#, "rm"),
            (
                r#"{"program":"zsh","args":["-c","typeset aliases[x]='rm -rf y'\nx"]}"#,
                "rm",
            ),
            (
                r#"{"program":"bash","args":["-c","declare BASH_CMDS[x]=/bin/rm; x -rf y"]}"#,
                "rm",
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert(denied.to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input("shell", input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value == denied
                }),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_a_builtin_repeat_or_alias_names_no_command() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            (
                "code_interpreter",
                r#"{"language":"sh","code":"builtin eval 'rm x'"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"repeat 2 ! rm x"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"alias x='rm -rf y'\nx"}"#,
            ),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 3, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[test]
    fn find_names_the_command_of_each_exec_predicate() {
        // `find` runs the command of each `-exec`, `-execdir`, `-ok` and `-okdir` predicate, up to the terminator
        // that ends it: a `;`, or a `+` right after `{}`. Its paths and tests name nothing, and a `+` anywhere else
        // is an argument.
        for (text, expected) in [
            ("find . -exec rm {} +", vec!["find", "rm"]),
            ("find . -exec rm {} \\;", vec!["find", "rm"]),
            ("find . -exec rm {} ;", vec!["find", "rm"]),
            ("find . -execdir rm {} ;", vec!["find", "rm"]),
            ("find . -ok rm {} ;", vec!["find", "rm"]),
            ("find . -okdir rm {} ;", vec!["find", "rm"]),
            ("find . -exec rm", vec!["find", "rm"]),
            ("find . -name rm -print", vec!["find"]),
            ("find . -exec env rm {} \\;", vec!["find", "env", "rm"]),
            ("find . -exec xargs rm {} +", vec!["find", "xargs", "rm"]),
            // A `{}` in the `-c` text is replaced at run time, so that text is withheld (`command_words_names`).
            ("find . -exec sh -c 'rm {}' \\;", vec!["find", "sh", ""]),
            ("find . -exec sh -c 'rm x' \\;", vec!["find", "sh", "rm"]),
            (
                "find . -exec rm {} + -exec ls \\;",
                vec!["find", "rm", "ls"],
            ),
            ("find . -exec echo + rm \\;", vec!["find", "echo"]),
            ("xargs find . -exec rm {} +", vec!["xargs", "find", "rm"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
        // A command word that expands is withheld, as any command word is.
        assert_eq!(command_prefixes("find . -exec {} \\;"), ["find", ""]);
        assert_eq!(command_prefixes("find . -exec $CMD {} +"), ["find", ""]);
        // The shell tool's argv reads the same predicates.
        assert_eq!(
            targets_of(
                "shell",
                r#"{"program":"find","args":[".","-exec","rm","{}","+"]}"#
            ),
            pairs(&[("command_prefix", "find"), ("command_prefix", "rm")])
        );
    }

    #[test]
    fn fd_names_the_command_its_exec_option_runs_up_to_the_terminator() {
        // `fd` and `fdfind` run the words given with `-x` or `-X` (`--exec`, `--exec-batch`), up to the `;` that
        // ends them, as one command: a value attached to the option (`-xrm`, `--exec=rm`) starts it. The other words
        // are fd's own.
        for (text, expected) in [
            ("fd -x rm {}", vec!["fd", "rm"]),
            ("fd -X rm {}", vec!["fd", "rm"]),
            ("fd --exec rm {}", vec!["fd", "rm"]),
            ("fd --exec=rm {}", vec!["fd", "rm"]),
            ("fdfind --exec-batch rm {}", vec!["fdfind", "rm"]),
            ("fd -xrm {}", vec!["fd", "rm"]),
            ("fd -Hx rm {}", vec!["fd", "rm"]),
            ("fd -e rs -x wc -l", vec!["fd", "wc"]),
            ("fd -x sh -c 'rm x'", vec!["fd", "sh", "rm"]),
            ("fd -x 'rm x' {}", vec!["fd", "rm"]),
            // `fd` puts a file name in place of each placeholder in a word of the command, so a word that holds one is a
            // run-time value (`substituted`): the `rm` a shell runs with the name is withheld, as the command of a word
            // that holds a placeholder is. A placeholder is no shell expansion, so a word of the command that holds only
            // one is read as text, and `{.}.jpg` names nothing.
            ("fd -x sh -c 'rm {}'", vec!["fd", "sh", ""]),
            ("fd -x sh -c 'rm {/}'", vec!["fd", "sh", ""]),
            ("fd -x sh -c 'rm {.}'", vec!["fd", "sh", ""]),
            ("fd -x rm {.}.bak", vec!["fd", "rm"]),
            ("fd -e png -x convert {} {.}.jpg", vec!["fd", "convert"]),
            ("/usr/bin/fd -x rm {}", vec!["/usr/bin/fd", "fd", "rm"]),
            // A command word that expands is withheld, as any command word is, and an option with no command runs none.
            ("fd -x {} x", vec!["fd", ""]),
            ("fd -e rs src", vec!["fd"]),
            // A letter that takes a value before an `x` takes it: `-tx` is the type `x`, and `-ex` the extension `x`.
            ("fd -tx . src", vec!["fd"]),
            ("fd -ex txt src", vec!["fd"]),
            ("fd -x", vec!["fd"]),
            // An escaped `\;` and a quoted `';'` each cut their segment, as a quoted separator cuts a wrapper's
            // arguments (`a_wrapper_whose_arguments_a_quote_cuts_withholds_the_command`), so the value is withheld too.
            ("fd -x rm {} \\;", vec!["fd", "", "rm"]),
            ("fd -x wc -l ';' -e rs", vec!["fd", "", "wc"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
        assert_eq!(
            targets_of("shell", r#"{"program":"fd","args":["-x","rm","{}",";"]}"#),
            pairs(&[("command_prefix", "fd"), ("command_prefix", "rm")])
        );
    }

    #[test]
    fn tar_names_the_command_its_option_runs_in_each_form() {
        for (text, expected) in [
            ("tar --to-command=rm -xf a.tar", vec!["tar", "rm"]),
            ("tar --to-command rm -xf a.tar", vec!["tar", "rm"]),
            ("tar -I rm -xf a.tar", vec!["tar", "rm"]),
            ("tar -Irm -xf a.tar", vec!["tar", "rm"]),
            ("tar -xIrm a.tar", vec!["tar", "rm"]),
            ("tar -x -I 'zstd -19' -f a.tar", vec!["tar", "zstd"]),
            ("tar --use-compress-program=rm -xf a.tar", vec!["tar", "rm"]),
            ("tar --use-compress-program rm -xf a.tar", vec!["tar", "rm"]),
            ("tar -cf a.tar -F rm .", vec!["tar", "rm"]),
            ("tar -cf a.tar --info-script=rm .", vec!["tar", "rm"]),
            ("tar -cf a.tar --new-volume-script rm .", vec!["tar", "rm"]),
            ("tar -xf a.tar --rsh-command=rm host:x", vec!["tar", "rm"]),
            ("tar -xf a.tar --rmt-command rm host:x", vec!["tar", "rm"]),
            (
                "tar -cf a.tar --checkpoint-action=exec=rm .",
                vec!["tar", "rm"],
            ),
            (
                "tar -cf a.tar --checkpoint-action exec=rm .",
                vec!["tar", "rm"],
            ),
            (
                r#"tar '--checkpoint-action=exec=sh -c "rm x"' -cf a.tar ."#,
                vec!["tar", "sh", "rm"],
            ),
            // getopt_long reads an unambiguous abbreviation of a long option as that option.
            ("tar --to-c=rm -xf a.tar", vec!["tar", "rm"]),
            // The first word may be a cluster with no dash, whose values are the words after it in order, so each of
            // them is read as a command (`tar fI x.tar rm a` gives `I` the value `rm` after `f`'s `x.tar`).
            ("tar xI rm x.tar", vec!["tar", "rm", "x.tar"]),
            ("tar fI x.tar rm a", vec!["tar", "x.tar", "rm", "a"]),
            ("tar Ixf rm a.tar", vec!["tar", "xf", "rm", "a.tar"]),
            // The other options run no command, so only tar is named.
            ("tar -czf x.tgz dir", vec!["tar"]),
            ("tar --to-stdout -xf a.tar", vec!["tar"]),
            ("tar --totals -cf a.tar .", vec!["tar"]),
            ("tar --checkpoint=100 -cf a.tar .", vec!["tar"]),
            ("tar -cf - dir", vec!["tar"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn rg_socat_and_the_netcat_programs_name_the_command_an_option_or_address_gives() {
        for (text, expected) in [
            ("rg --pre rm foo", vec!["rg", "rm"]),
            ("rg --pre=rm foo", vec!["rg", "rm"]),
            ("rg --hostname-bin rm foo", vec!["rg", "rm"]),
            ("rg --pre ./x.sh foo", vec!["rg", "./x.sh", "x.sh", ""]),
            ("rg -n --no-pre --pretty foo src", vec!["rg"]),
            ("socat TCP-LISTEN:1234 EXEC:rm", vec!["socat", "rm"]),
            ("socat - SYSTEM:'rm x'", vec!["socat", "rm"]),
            ("socat - exec:'rm x',pty", vec!["socat", "rm"]),
            ("socat - TCP:127.0.0.1:80", vec!["socat"]),
            ("nc -e rm host 1", vec!["nc", "rm"]),
            ("nc -lp 4444 -e rm", vec!["nc", "rm"]),
            ("nc -erm host 1", vec!["nc", "rm"]),
            // A shell that the value starts reads its commands from the standard input, which is withheld.
            ("nc -lp 4444 -e /bin/sh", vec!["nc", "/bin/sh", "sh", ""]),
            ("nc -zv host 80", vec!["nc"]),
            ("nc -lvnp 4444", vec!["nc"]),
            ("ncat -c 'rm x' host 1", vec!["ncat", "rm"]),
            ("ncat -lvc 'rm x'", vec!["ncat", "rm"]),
            ("ncat --sh-exec 'rm x' host 1", vec!["ncat", "rm"]),
            ("ncat --sh-exec='rm x' host 1", vec!["ncat", "rm"]),
            ("ncat --exec rm host 1", vec!["ncat", "rm"]),
            // `--lua-exec` runs a Lua script, whose commands no name here reads, so its value is withheld.
            ("ncat --lua-exec hook.lua -l 1", vec!["ncat", ""]),
            ("ncat --ssl -l 1", vec!["ncat"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn socat_names_the_command_of_every_address_in_a_dual_or_chained_argument() {
        // `!!` joins the reading and the writing address of a dual argument, and `|` and `%` are read as separators too.
        // An address after any of them runs its command as one at the start of an argument does. The keyword is read
        // without regard to case, and `SHELL` runs its command through a shell as `SYSTEM` does (the 1.8.0.0 man page
        // documents `SHELL`). A command runs to the end of its argument, so a later address is read in its text too.
        for (words, expected) in [
            (
                vec!["socat", "-", "OPEN:/dev/null!!EXEC:rm x"],
                vec!["socat", "rm"],
            ),
            (
                vec!["socat", "-", "STDIO!!SYSTEM:rm x"],
                vec!["socat", "rm"],
            ),
            (
                vec!["socat", "-", "STDIO!!system:rm x"],
                vec!["socat", "rm"],
            ),
            (
                vec!["socat", "-", "OPEN:/dev/null!! EXEC:rm x"],
                vec!["socat", "rm"],
            ),
            (vec!["socat", "-", "STDIO!!SHELL:rm x"], vec!["socat", "rm"]),
            (
                vec!["socat", "-", "TCP:host:80|EXEC:rm"],
                vec!["socat", "rm"],
            ),
            (
                vec!["socat", "-", "TCP:host:80%SYSTEM:rm"],
                vec!["socat", "rm"],
            ),
            (
                vec!["socat", "-", "OPEN:/dev/null!!TCP:host:80"],
                vec!["socat"],
            ),
            (vec!["socat", "-", "EXEC:rm x!!STDIO"], vec!["socat", "rm"]),
        ] {
            let words: Vec<String> = words.into_iter().map(str::to_owned).collect();
            assert_eq!(command_words_names(&words, 0, false), expected, "{words:?}");
        }
    }

    #[test]
    fn an_expansion_that_may_stand_for_an_option_or_address_withholds_the_command() {
        // The shell expands an argument before the program reads it, so an expansion may be an option or an address the
        // program reads: `tar $T a.tar` is `tar Ixf a.tar` when `T` holds `Ixf`, and `rg "$P" .` is `rg --pre=rm .` when
        // `P` holds `--pre=rm`. It may split into several words, one of them an option, so it is withheld wherever it
        // stands. A bare `{}`, and each placeholder `fd` fills, is literal to the shell, so it is no expansion.
        for (text, expected) in [
            ("tar $A -xf a.tar", vec!["tar", ""]),
            ("A=--to-command=rm; tar $A -xf a.tar", vec!["tar", ""]),
            ("tar $T a.tar", vec!["tar", ""]),
            ("tar -cf a.tar x$X", vec!["tar", ""]),
            ("socat - $A", vec!["socat", ""]),
            ("socat - \"$ADDR\"", vec!["socat", ""]),
            ("socat - 'OPEN:$F'", vec!["socat", ""]),
            ("rg $P foo", vec!["rg", ""]),
            ("rg \"$P\" .", vec!["rg", ""]),
            ("nc $O host 1", vec!["nc", ""]),
            ("fd $X rm {}", vec!["fd", ""]),
            ("fd -x rm $X", vec!["fd", "", "rm"]),
            ("fd -x rm {}", vec!["fd", "rm"]),
            ("fd -e png -x convert {} {.}.jpg", vec!["fd", "convert"]),
            ("tar -czf x.tgz dir", vec!["tar"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn an_option_value_program_names_its_command_and_withholds_a_cut_one() {
        // A command option may lie past a cut, so the withheld value is named then, as `eval`'s is. A command the given
        // words already hold is named as well.
        for (words, expected) in [
            (vec!["tar", "-czf", "x.tgz"], vec!["tar", ""]),
            (vec!["fd", "-x", "rm"], vec!["fd", "", "rm"]),
            (vec!["socat", "-", "EXEC:rm"], vec!["socat", "", "rm"]),
        ] {
            let words: Vec<String> = words.into_iter().map(str::to_owned).collect();
            assert_eq!(command_words_names(&words, 0, true), expected, "{words:?}");
        }
    }

    #[test]
    fn the_programs_given_a_table_name_the_command_they_run_past_their_options() {
        // Each table lists the options that take no value and those that take one, and the operands before the command.
        for (text, expected) in [
            ("setsid rm x", vec!["setsid", "rm"]),
            ("setsid -f -w rm x", vec!["setsid", "rm"]),
            ("setsid --fork rm x", vec!["setsid", "rm"]),
            ("stdbuf -oL rm x", vec!["stdbuf", "rm"]),
            ("stdbuf -o L -e 0 rm x", vec!["stdbuf", "rm"]),
            ("stdbuf --output=L rm x", vec!["stdbuf", "rm"]),
            ("ionice rm x", vec!["ionice", "rm"]),
            ("ionice -c3 rm x", vec!["ionice", "rm"]),
            ("ionice -c 3 -n 7 -t rm x", vec!["ionice", "rm"]),
            ("ionice --class=3 --ignore rm x", vec!["ionice", "rm"]),
            ("chrt -f 10 rm x", vec!["chrt", "rm"]),
            ("chrt --rr 10 rm x", vec!["chrt", "rm"]),
            ("chrt -R -T 100 10 rm x", vec!["chrt", "rm"]),
            // The policies that take no priority (`-o`, `-b`, `-i`, `-d`) run the word after their options as the
            // command, unless that word is all digits and more operands follow it, which is the priority.
            ("chrt -o rm x", vec!["chrt", "rm"]),
            ("chrt --other rm x", vec!["chrt", "rm"]),
            ("chrt --batch rm x", vec!["chrt", "rm"]),
            ("chrt -i rm x", vec!["chrt", "rm"]),
            ("chrt -d -T 1 -P 2 -D 3 rm x", vec!["chrt", "rm"]),
            ("chrt -o -- rm x", vec!["chrt", "rm"]),
            ("chrt -o 5 rm x", vec!["chrt", "rm"]),
            // A word that is not a decimal number is no priority on any version: `0x10` is the command, and the
            // earlier versions fail to read it.
            ("chrt -o 0x10 rm x", vec!["chrt", "0x10"]),
            // A decimal number that is not all digits is the priority on util-linux before 2.41, which runs the word
            // after it, and the command on 2.41 and later: both are named. The quoted space is part of the word, which
            // names no command.
            ("chrt -f +10 rm x", vec!["chrt", "+10", "rm"]),
            ("chrt +10 rm x", vec!["chrt", "+10", "rm"]),
            ("chrt -o +0 rm x", vec!["chrt", "+0", "rm"]),
            ("chrt -o -- -0 rm x", vec!["chrt", "-0", "rm"]),
            ("chrt ' 10' rm x", vec!["chrt", "rm"]),
            ("taskset 0x1 rm x", vec!["taskset", "rm"]),
            ("taskset -c 0-3 rm x", vec!["taskset", "rm"]),
            ("taskset --cpu-list 0 rm x", vec!["taskset", "rm"]),
            ("flock /tmp/lock rm x", vec!["flock", "rm"]),
            ("flock -w 5 -x /tmp/lock rm x", vec!["flock", "rm"]),
            ("flock --timeout=5 /tmp/lock rm x", vec!["flock", "rm"]),
            ("doas rm x", vec!["doas", "rm"]),
            ("doas -n -u root rm x", vec!["doas", "rm"]),
            ("doas -C conf.file -a style rm x", vec!["doas", "rm"]),
            ("chroot /srv rm x", vec!["chroot", "rm"]),
            ("chroot --userspec=0:0 /srv rm x", vec!["chroot", "rm"]),
            (
                "chroot --groups 0 --skip-chdir /srv rm x",
                vec!["chroot", "rm"],
            ),
            ("xargs setsid -f rm x", vec!["xargs", "setsid", "rm"]),
            ("toybox sh -c 'rm x'", vec!["toybox", "sh", "rm"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn a_program_that_runs_its_command_in_a_form_no_table_reads_names_the_withheld_value() {
        // `ssh` runs its remote command as shell text, and `su`, `script`, `watch` and the others run theirs in a
        // form no table reads: each names itself and the withheld value, which covers every command.
        for (text, expected) in [
            ("ssh host rm -rf x", vec!["ssh", ""]),
            ("/usr/bin/ssh host rm", vec!["/usr/bin/ssh", "ssh", ""]),
            ("su -c 'rm x' root", vec!["su", ""]),
            ("script -c 'rm x' log.txt", vec!["script", ""]),
            ("watch -n 1 rm x", vec!["watch", ""]),
            ("parallel rm ::: x", vec!["parallel", ""]),
            ("unshare -r rm x", vec!["unshare", ""]),
            ("nsenter -t 1 -m rm x", vec!["nsenter", ""]),
            ("strace -f rm x", vec!["strace", ""]),
            ("valgrind --tool=memcheck rm x", vec!["valgrind", ""]),
            ("runuser -u root -- rm x", vec!["runuser", ""]),
            ("setpriv --reuid=0 rm x", vec!["setpriv", ""]),
            ("xargs ssh host rm", vec!["xargs", "ssh", ""]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
        // A remote command is withheld whatever shell text it holds.
        assert!(command_prefixes("ssh host 'ls; rm x'").contains(&String::new()));
        assert_eq!(
            targets_of("shell", r#"{"program":"su","args":["-c","rm x","root"]}"#),
            pairs(&[("command_prefix", "su"), ("command_prefix", "")])
        );
    }

    #[test]
    fn an_option_a_table_leaves_out_withholds_the_command() {
        // An option a table does not list may take a value, or run its text as a command: `flock -c` and `sudo -i`
        // (`--login`) run a shell text, so the command is withheld rather than read as `FOO` in `FOO=1 rm x`.
        for (text, expected) in [
            ("flock /tmp/lock -c 'rm x'", vec!["flock", "-c", ""]),
            ("flock -c 'rm x' /tmp/lock", vec!["flock", ""]),
            ("sudo -i 'FOO=1 rm x'", vec!["sudo", ""]),
            ("sudo --login 'FOO=1 rm x'", vec!["sudo", ""]),
            ("sudo -i rm x", vec!["sudo", ""]),
            ("ionice -p 1 rm x", vec!["ionice", ""]),
            ("doas -s rm x", vec!["doas", ""]),
            ("setsid --unknown rm x", vec!["setsid", ""]),
            ("chroot -x /srv rm x", vec!["chroot", ""]),
            ("taskset -Z 0x1 rm x", vec!["taskset", ""]),
            ("stdbuf -Z rm x", vec!["stdbuf", ""]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn every_program_that_runs_a_command_names_that_command_or_withholds_it() {
        // A program with a table names the command it runs past the options the table reads. One whose options or form
        // no table reads names the withheld value, which covers every command. `fakeroot`, `cgexec` and `xvfb-run` are
        // withheld whole, since their option tables are not verified, so each names the withheld value.
        for (text, expected) in [
            ("setsid -f -w rm x", vec!["setsid", "rm"]),
            ("stdbuf -oL rm x", vec!["stdbuf", "rm"]),
            ("ionice -c3 rm x", vec!["ionice", "rm"]),
            ("chrt -o 5 rm x", vec!["chrt", "rm"]),
            ("taskset 0x1 rm x", vec!["taskset", "rm"]),
            ("flock /tmp/l rm x", vec!["flock", "rm"]),
            ("flock -e /tmp/l rm x", vec!["flock", "rm"]),
            ("flock /tmp/l -c 'rm x'", vec!["flock", "-c", ""]),
            ("doas -n rm x", vec!["doas", "rm"]),
            ("chroot /srv rm x", vec!["chroot", "rm"]),
            ("fakeroot -l /tmp/lib -- rm x", vec!["fakeroot", ""]),
            ("fakeroot --save-file=/tmp/s rm x", vec!["fakeroot", ""]),
            ("fakeroot -b rm x", vec!["fakeroot", ""]),
            ("fakeroot", vec!["fakeroot", ""]),
            ("cgexec -g memory:/x rm x", vec!["cgexec", ""]),
            ("cgexec --sticky -g cpu,memory:/x rm x", vec!["cgexec", ""]),
            ("cgexec -s rm x", vec!["cgexec", ""]),
            (
                "xvfb-run -a -s '-screen 0 1x1x24' rm x",
                vec!["xvfb-run", ""],
            ),
            ("xvfb-run -n 99 -e /dev/null rm x", vec!["xvfb-run", ""]),
            ("xvfb-run -x rm x", vec!["xvfb-run", ""]),
            ("xvfb-run", vec!["xvfb-run", ""]),
            ("runuser -u root -- rm x", vec!["runuser", ""]),
            ("su -c 'rm x' root", vec!["su", ""]),
            ("script -c 'rm x' log.txt", vec!["script", ""]),
            ("nsenter -t 1 -m rm x", vec!["nsenter", ""]),
            ("unshare -r rm x", vec!["unshare", ""]),
            ("strace -f rm x", vec!["strace", ""]),
            ("ltrace -S rm x", vec!["ltrace", ""]),
            ("watch -n 1 rm x", vec!["watch", ""]),
            ("parallel rm ::: x", vec!["parallel", ""]),
            ("setpriv --reuid=0 rm x", vec!["setpriv", ""]),
            ("unbuffer rm x", vec!["unbuffer", ""]),
            ("gdb --args rm x", vec!["gdb", ""]),
            ("gdb-multiarch --args rm x", vec!["gdb-multiarch", ""]),
            ("screen -dm rm x", vec!["screen", ""]),
            ("screen -dmS work rm x", vec!["screen", ""]),
            ("tmux new-session -d 'rm x'", vec!["tmux", ""]),
            ("tmux new-window rm x", vec!["tmux", ""]),
            ("perf stat -e cycles rm x", vec!["perf", ""]),
            ("perf record -- rm x", vec!["perf", ""]),
            ("firejail --noprofile rm x", vec!["firejail", ""]),
            ("bwrap --ro-bind / / rm x", vec!["bwrap", ""]),
            ("run0 rm x", vec!["run0", ""]),
            ("dbus-run-session -- rm x", vec!["dbus-run-session", ""]),
            ("faketime '2020-01-01' rm x", vec!["faketime", ""]),
            ("torsocks rm x", vec!["torsocks", ""]),
            ("proxychains rm x", vec!["proxychains", ""]),
            ("proxychains4 -q rm x", vec!["proxychains4", ""]),
            ("chpst -u app rm x", vec!["chpst", ""]),
            ("fakeroot-sysv rm x", vec!["fakeroot-sysv", ""]),
            ("fakeroot-tcp rm x", vec!["fakeroot-tcp", ""]),
            ("find . -exec rm {} \\;", vec!["find", "rm"]),
            ("find . -execdir rm {} \\;", vec!["find", "rm"]),
            ("find . -ok rm {} \\;", vec!["find", "rm"]),
            ("find . -okdir rm {} \\;", vec!["find", "rm"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn a_program_that_runs_the_command_it_is_given_names_that_command_or_withholds_it() {
        // Each program runs the command it is given, or one it reads from a file or stdin, in a form no table reads
        // (`WITHHELD_COMMANDS`): it names itself and the withheld value, and nothing past it. A subcommand that runs no
        // command, such as `npm install`, is withheld with the rest, since no table tells the subcommands apart.
        for program in [
            "ssh-agent",
            "setarch",
            "linux32",
            "linux64",
            "systemd-inhibit",
            "chronic",
            "ifne",
            "daemonize",
            "dbus-launch",
            "capsh",
            "softlimit",
            "setuidgid",
            "envdir",
            "envuidgid",
            "setlock",
            "pgrphack",
            "catchsegv",
            "schroot",
            "lxc-attach",
            "distrobox",
            "flatpak-spawn",
            "systemd-nspawn",
            "proot",
            "start-stop-daemon",
            "apptainer",
            "singularity",
            "hyperfine",
            "entr",
            "tsocks",
            "torify",
            "trickle",
            "sem",
            "nocache",
            "rr",
            "xterm",
            "at",
            "batch",
            "expect",
            "nix-shell",
            "npx",
            "npm",
            "pnpm",
            "yarn",
            "uv",
            "poetry",
            "pipenv",
            "conda",
            "bundle",
            "direnv",
            "cargo",
            "go",
            "kubectl",
            "docker",
            "podman",
            "docker-compose",
            "ip",
            "tini",
            "dumb-init",
            "gosu",
            "su-exec",
            "fakeroot",
            "fakechroot",
            "cgexec",
            "xvfb-run",
            "uvx",
            "pipx",
            "bunx",
            "bun",
            "pnpx",
            "deno",
            "corepack",
            "composer",
            "mise",
            "asdf",
            "rbenv",
            "pyenv",
            "nvm",
            "hatch",
            "pdm",
            "rye",
            "tox",
            "nox",
            "toolbox",
            "nerdctl",
            "crictl",
            "buildah",
            "oc",
            "incus",
            "lxc",
            "machinectl",
            "systemd-cat",
            "srun",
            "mpirun",
            "mpiexec",
            "flatpak",
            "nsjail",
            "sandbox-exec",
            "nix",
            "mosh",
            "run-parts",
            "cpulimit",
            "multitime",
            "watchexec",
        ] {
            assert_eq!(
                command_prefixes(&format!("{program} rm x")),
                [program, ""],
                "{program}"
            );
        }
        // A path names its basename too, and a subcommand after global options is withheld the same way.
        assert_eq!(
            command_prefixes("/usr/local/bin/npm --prefix app run build"),
            ["/usr/local/bin/npm", "npm", ""]
        );
        assert_eq!(
            command_prefixes("nix-shell --run 'rm x'"),
            ["nix-shell", ""]
        );
        assert_eq!(command_prefixes("docker exec app rm x"), ["docker", ""]);
        assert_eq!(
            targets_of("shell", r#"{"program":"npx","args":["rm","x"]}"#),
            pairs(&[("command_prefix", "npx"), ("command_prefix", "")])
        );
    }

    #[test]
    fn setarch_is_withheld_under_each_architecture_name_util_linux_installs_it_as() {
        // util-linux installs setarch under the names of the architectures it sets, and each of those runs the
        // command it is given, as `setarch` and its `linux32` and `linux64` aliases do. Each name is withheld bare,
        // by its path, as the program of a `shell` call, and as the command a wrapper runs, so none names the `rm`.
        for name in [
            "i386",
            "i486",
            "i586",
            "i686",
            "x86_64",
            "uname26",
            "ppc",
            "ppc32",
            "ppc64",
            "ppc64le",
            "s390",
            "s390x",
            "sparc",
            "sparc32",
            "sparc32bash",
            "sparc64",
            "sparc64bash",
            "mips",
            "mips64",
            "hppa",
            "hppa64",
            "parisc",
            "parisc64",
            "armv7l",
            "aarch64",
            "ia64",
            "alpha",
        ] {
            assert_eq!(
                command_prefixes(&format!("{name} rm x")),
                [name, ""],
                "{name}"
            );
            assert_eq!(
                command_prefixes(&format!("/usr/bin/{name} rm x")),
                [format!("/usr/bin/{name}"), name.to_owned(), String::new()],
                "/usr/bin/{name}"
            );
            assert_eq!(
                command_prefixes(&format!("nice {name} rm x")),
                ["nice", name, ""],
                "nice {name}"
            );
            assert_eq!(
                targets_of(
                    "shell",
                    &format!(r#"{{"program":"{name}","args":["rm","x"]}}"#)
                ),
                pairs(&[("command_prefix", name), ("command_prefix", "")]),
                "{name}"
            );
        }
    }

    #[test]
    fn eatmydata_names_the_command_it_is_given_and_withholds_an_option() {
        // `eatmydata` takes no option of its own, so its command is its first word, and an option is withheld.
        for (text, expected) in [
            ("eatmydata rm x", vec!["eatmydata", "rm"]),
            (
                "/usr/bin/eatmydata rm x",
                vec!["/usr/bin/eatmydata", "eatmydata", "rm"],
            ),
            ("eatmydata -v rm x", vec!["eatmydata", ""]),
            ("eatmydata --help", vec!["eatmydata", ""]),
            ("eatmydata", vec!["eatmydata"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
        assert_eq!(
            targets_of("shell", r#"{"program":"eatmydata","args":["rm","x"]}"#),
            pairs(&[("command_prefix", "eatmydata"), ("command_prefix", "rm")])
        );
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_the_programs_that_run_a_command_name_no_command() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            ("shell", r#"{"program":"eatmydata","args":["rm","x"]}"#),
            ("shell", r#"{"program":"npx","args":["rm","x"]}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"nix-shell --run 'rm x'"}"#,
            ),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 3, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_the_option_value_programs_name_no_command() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            ("shell", r#"{"program":"fd","args":["-x","rm","{}"]}"#),
            (
                "shell",
                r#"{"program":"tar","args":["--to-command=rm","-x"]}"#,
            ),
            ("shell", r#"{"program":"socat","args":["-","EXEC:rm"]}"#),
            ("shell", r#"{"program":"ncat","args":["-c","rm x"]}"#),
            ("shell", r#"{"program":"tar","args":["$A","-xf","a.tar"]}"#),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 5, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[test]
    fn nested_wrappers_name_every_command_down_the_chain() {
        assert_eq!(
            command_prefixes("env nice setsid rm x"),
            ["env", "nice", "setsid", "rm"]
        );
        assert_eq!(
            command_prefixes("sudo env timeout 5 nice -n 1 chrt -o 0 rm x"),
            ["sudo", "env", "timeout", "nice", "chrt", "rm"]
        );
        assert_eq!(
            command_prefixes("setsid -f nice -n 1 timeout 5 rm x"),
            ["setsid", "nice", "timeout", "rm"]
        );
        // A program withheld whole withholds the value, and nothing past it is read.
        assert_eq!(
            command_prefixes("fakeroot cgexec -g memory:/x xvfb-run -a rm x"),
            ["fakeroot", ""]
        );
        assert_eq!(
            command_prefixes("xargs setsid -f doas -n rm x"),
            ["xargs", "setsid", "doas", "rm"]
        );
        // A withheld program inside a table program withholds the value, and nothing past it is read.
        assert_eq!(
            command_prefixes("env nice gdb --args rm x"),
            ["env", "nice", "gdb", ""]
        );
    }

    #[test]
    fn chrt_names_its_command_whether_its_operand_is_the_priority_or_the_command() {
        for (text, expected) in [
            // A lone all-digit operand is the command on util-linux 2.41, since the priority is one only when more
            // operands follow it.
            ("chrt -o 5", vec!["chrt", "5"]),
            // chrt's getopt starts with `+`, so the words after the priority are the command and its arguments, and none
            // is read as an option: a `--` or an option-shaped word after the priority is the command.
            ("chrt -o 5 --", vec!["chrt", "--"]),
            ("chrt -o 5 -T 10", vec!["chrt", "-T"]),
            ("chrt -o 5 -v rm", vec!["chrt", "-v"]),
            ("chrt -o 5 -Z rm x", vec!["chrt", "-Z"]),
            ("chrt -o 5 rm x", vec!["chrt", "rm"]),
            ("chrt -o 5 -- rm x", vec!["chrt", "--"]),
            ("chrt -o 0 -- rm x", vec!["chrt", "--"]),
            // A decimal number that is not all digits is the priority before 2.41, and the command after it on 2.41:
            // both are named.
            ("chrt -f +10 -- rm x", vec!["chrt", "+10", "--"]),
            ("chrt -o +10 -- rm x", vec!["chrt", "+10", "--"]),
            ("chrt -f +10 -v", vec!["chrt", "+10", "-v"]),
            ("chrt -f +10 rm x", vec!["chrt", "+10", "rm"]),
            ("chrt -f +10", vec!["chrt", "+10"]),
            // After a `--` the words are operands, so `-o` is the command that follows the priority.
            ("chrt -o -- 5 -o", vec!["chrt", "-o"]),
            // An assignment where the priority is due is read as the operand it may be, whose path is named. That path is
            // a file `chrt` runs by its path, outside the system binary directories, so its value is withheld too.
            ("chrt -o A=/tmp/evil x", vec!["chrt", "A", "evil", "", "x"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
        // Past a cut the command may be the lone operand, or the operand may be followed by a command, so the withheld
        // value is named too.
        let words: Vec<String> = ["chrt", "-o", "5"]
            .iter()
            .map(|word| (*word).to_owned())
            .collect();
        assert_eq!(command_words_names(&words, 0, true), ["chrt", "5", ""]);
        // A decimal priority with nothing after it may have its command past the cut, which is withheld.
        let words: Vec<String> = ["chrt", "+10"]
            .iter()
            .map(|word| (*word).to_owned())
            .collect();
        assert_eq!(command_words_names(&words, 0, true), ["chrt", "+10", ""]);
    }

    #[test]
    fn a_wrapper_reads_no_option_after_its_operand_and_the_word_after_it_is_the_command() {
        // getopt's `+` ends the options at the first non-option, so an option-shaped word after timeout's duration,
        // taskset's mask or chroot's NEWROOT is the command, and the words after it are its arguments.
        assert_eq!(
            command_prefixes("timeout 5 -s KILL rm x"),
            ["timeout", "-s"]
        );
        assert_eq!(command_prefixes("timeout 5 -v rm x"), ["timeout", "-v"]);
        assert_eq!(command_prefixes("timeout 5 -- rm x"), ["timeout", "--"]);
        assert_eq!(command_prefixes("taskset 0x1 -c rm x"), ["taskset", "-c"]);
        assert_eq!(command_prefixes("chroot /srv -x rm x"), ["chroot", "-x"]);
        // The plain case still names only the command it runs.
        assert_eq!(command_prefixes("timeout 5 rm x"), ["timeout", "rm"]);
        assert_eq!(command_prefixes("taskset 0x1 rm x"), ["taskset", "rm"]);
        assert_eq!(command_prefixes("chroot /srv rm x"), ["chroot", "rm"]);
        assert_eq!(command_prefixes("echo rm x"), ["echo"]);
        // flock's getopt reads options after its lock file too, so an option-shaped word there is named as the command and
        // as an option, and the command after the options is named as well.
        assert_eq!(
            command_prefixes("flock /tmp/l -x rm x"),
            ["flock", "-x", "rm"]
        );
        assert_eq!(command_prefixes("flock /tmp/l rm x"), ["flock", "rm"]);
        // A bare `-` is no option of flock, and no table reads it as an operand, so the value is withheld.
        assert_eq!(command_prefixes("flock - rm x"), ["flock", ""]);
        // GNU env's bare `-` is its `-i` option, so the command after it is `rm`.
        assert_eq!(command_prefixes("env - rm x"), ["env", "rm"]);
    }

    #[test]
    fn an_assignment_after_a_wrapper_operand_is_the_program_it_execs() {
        // A wrapper that execs its command runs `A=1` as the program after its operands, so `rm` is only its argument.
        assert_eq!(command_prefixes("timeout 5 A=1 rm x"), ["timeout", "A"]);
        assert_eq!(command_prefixes("taskset 0x1 A=1 rm x"), ["taskset", "A"]);
        assert_eq!(command_prefixes("chroot /srv A=1 rm x"), ["chroot", "A"]);
        assert_eq!(command_prefixes("nice A=1 rm x"), ["nice", "A"]);
    }

    #[test]
    fn a_dash_c_with_no_script_operand_withholds_its_value() {
        // The script of `-c` is the first operand after the options that follow it. With none on the command line, the
        // script comes at run time: `xargs` appends its input as the operands (`echo 'rm -rf x' | xargs -0 sh -c` runs
        // `sh -c 'rm -rf x'`), and a `find -exec` with no operand gives none at all. The value is withheld, as its script
        // is unread.
        for (text, expected) in [
            (
                "echo 'rm -rf x' | xargs -0 sh -c",
                vec!["echo", "xargs", "sh", ""],
            ),
            ("xargs sh -c -e", vec!["xargs", "sh", ""]),
            ("sh -c", vec!["sh", ""]),
            ("sh -c -e", vec!["sh", ""]),
            ("sh -c --", vec!["sh", ""]),
            ("sh -c -o pipefail", vec!["sh", ""]),
            ("sh -o pipefail -c", vec!["sh", ""]),
            ("sudo -u root sh -c", vec!["sudo", "sh", ""]),
            ("find . -exec sh -c \\;", vec!["find", "sh", ""]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
        // A script on the command line is read, after the options that follow the `-c`.
        for (text, expected) in [
            ("sh -c -e 'rm x'", vec!["sh", "rm"]),
            ("sh -c -- 'rm x'", vec!["sh", "rm"]),
            ("sh -c -o pipefail 'rm x'", vec!["sh", "pipefail", "rm"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn a_word_xargs_or_find_substitutes_at_run_time_names_the_withheld_value() {
        // `xargs -I STR` (and `-iSTR`, `-i`, `--replace`, `--replace=STR`) and `find -exec`'s `{}` put a run-time value
        // where the string stands. Where that value is run as a command, or read as shell text, it is withheld: the
        // command word, a `-c` text, an `eval` text, or a wrapper's command that holds the string.
        for (text, expected) in [
            (
                "echo 'rm x' | xargs -I X sh -c X",
                vec!["echo", "xargs", "sh", ""],
            ),
            (
                "echo 'rm x' | xargs -I{} sh -c {}",
                vec!["echo", "xargs", "sh", ""],
            ),
            (
                "echo 'rm x' | xargs -iX sh -c X",
                vec!["echo", "xargs", "sh", ""],
            ),
            (
                "echo 'rm x' | xargs --replace=X sh -c X",
                vec!["echo", "xargs", "sh", ""],
            ),
            (
                "echo 'rm x' | xargs -I{} sh -c 'echo {}'",
                vec!["echo", "xargs", "sh", ""],
            ),
            (
                "echo 'rm x' | xargs -I X sh -c \"echo 'X'\"",
                vec!["echo", "xargs", "sh", ""],
            ),
            ("xargs -I X X x", vec!["xargs", ""]),
            ("xargs -I X env X x", vec!["xargs", "env", ""]),
            ("xargs -I X sudo X", vec!["xargs", "sudo", ""]),
            ("xargs -I X eval 'X x'", vec!["xargs", "eval", ""]),
            ("xargs -I X find . -exec X \\;", vec!["xargs", "find", ""]),
            ("find . -exec sh -c 'echo {}' \\;", vec!["find", "sh", ""]),
            ("find . -exec sh -c 'rm {}' \\;", vec!["find", "sh", ""]),
            ("find . -exec eval 'echo {}' \\;", vec!["find", "eval", ""]),
            ("find . -exec {} \\;", vec!["find", ""]),
            // The default string `{}` of a bare `-i` and of a bare `--replace`, and a flag letter before the `I`.
            (
                "echo 'rm x' | xargs -i sh -c {}",
                vec!["echo", "xargs", "sh", ""],
            ),
            (
                "echo 'rm x' | xargs --replace sh -c {}",
                vec!["echo", "xargs", "sh", ""],
            ),
            ("xargs -0I X sh -c X", vec!["xargs", "sh", ""]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
        // The string in an argument names nothing, and the command it is an argument of is named as before.
        for (text, expected) in [
            ("xargs -i rm {}", vec!["xargs", "rm"]),
            ("xargs --replace rm {}", vec!["xargs", "rm"]),
            ("xargs -I{} rm {}", vec!["xargs", "rm"]),
            ("xargs -I X echo X", vec!["xargs", "echo"]),
            ("find . -exec rm {} \\;", vec!["find", "rm"]),
            ("xargs -I X sh -c 'rm x'", vec!["xargs", "sh", "rm"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn the_shells_the_table_did_not_list_read_their_script_or_withhold_it() {
        // The POSIX-family shells the table did not list read their `-c` text as the others do, and a shell with no
        // script withholds its value.
        for (text, expected) in [
            ("rbash -c 'rm x'", vec!["rbash", "rm"]),
            ("posh -c 'rm x'", vec!["posh", "rm"]),
            ("yash -c 'rm x'", vec!["yash", "rm"]),
            ("oksh -c 'rm x'", vec!["oksh", "rm"]),
            ("pdksh -c 'rm x'", vec!["pdksh", "rm"]),
            ("rksh -c 'rm x'", vec!["rksh", "rm"]),
            (
                "/usr/bin/rbash -c 'rm x'",
                vec!["/usr/bin/rbash", "rbash", "rm"],
            ),
            ("echo 'rm x' | rbash", vec!["echo", "rbash", ""]),
            ("rbash ./x.sh", vec!["rbash", ""]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
        // The shells whose syntax the reader does not model also withhold their value, whatever their text names: `^rm`
        // in nu, `iex` in PowerShell and `e:rm` in elvish run commands the reader does not name.
        for (text, expected) in [
            ("tcsh -c 'rm x'", vec!["tcsh", "rm", ""]),
            ("csh -c 'rm x'", vec!["csh", "rm", ""]),
            ("echo 'rm x' | fish", vec!["echo", "fish", ""]),
            ("fish -c 'rm x'", vec!["fish", "rm", ""]),
            ("pwsh -c 'rm x'", vec!["pwsh", "rm", ""]),
            ("pwsh -Command 'rm x'", vec!["pwsh", "rm", ""]),
            ("pwsh -EncodedCommand cm0geA==", vec!["pwsh", ""]),
            ("pwsh -File x.ps1", vec!["pwsh", ""]),
            ("nu -c '^rm x'", vec!["nu", ""]),
            ("nu", vec!["nu", ""]),
            ("elvish -c 'e:rm x'", vec!["elvish", "e", ""]),
            ("xonsh -c 'rm x'", vec!["xonsh", "rm", ""]),
            ("cmd /c 'rm x'", vec!["cmd", ""]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn a_value_an_option_takes_is_not_read_as_a_dash_c_flag() {
        // mksh's `-T tty` and ksh93's `-R file` take a value, so a word shaped like `-c` right after one is that value,
        // and the script file that follows it is withheld, not read as `-c` text.
        for (text, expected) in [
            ("mksh -T -c ./evil.sh", vec!["mksh", ""]),
            ("mksh -T -c rm", vec!["mksh", ""]),
            ("ksh -R -c 'rm x'", vec!["ksh", ""]),
            ("mksh -T tty -c 'rm x'", vec!["mksh", "rm"]),
            ("ksh -R file -c 'rm x'", vec!["ksh", "rm"]),
            ("mksh -c 'rm x'", vec!["mksh", "rm"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn the_options_after_a_dash_c_decide_whether_its_text_is_on_the_command_line() {
        // The `-c` text is the first word that the options after the `-c` leave. Each value letter takes the word
        // after it, wherever it sits in its word (`-co pipefail`, `-oe pipefail`, `-coo pipefail errexit`), as bash and
        // dash read them. A lone `+` is no option, so the options go on past it, as bash and dash read it. A lone `-`
        // ends the options, as `--` does. With no word left for the text, the text is the run-time input `xargs`
        // appends, so it is withheld.
        for (text, expected) in [
            (
                "echo 'rm x' | xargs -0 bash -c -",
                vec!["echo", "xargs", "bash", ""],
            ),
            (
                "echo 'rm x' | xargs -0 bash -c +",
                vec!["echo", "xargs", "bash", ""],
            ),
            (
                "echo 'rm x' | xargs -0 bash -c + -o pipefail",
                vec!["echo", "xargs", "bash", ""],
            ),
            (
                "echo 'rm x' | xargs -0 sh -c + -o noglob",
                vec!["echo", "xargs", "sh", ""],
            ),
            (
                "echo 'rm x' | xargs -0 bash -co pipefail",
                vec!["echo", "xargs", "bash", ""],
            ),
            (
                "echo 'rm x' | xargs -0 bash -cO extglob",
                vec!["echo", "xargs", "bash", ""],
            ),
            (
                "echo 'rm x' | xargs -0 bash -coo pipefail errexit",
                vec!["echo", "xargs", "bash", ""],
            ),
            (
                "echo 'rm x' | xargs -0 bash -c -oe pipefail",
                vec!["echo", "xargs", "bash", ""],
            ),
            (
                "echo 'rm x' | xargs -0 mksh -cT tty",
                vec!["echo", "xargs", "mksh", ""],
            ),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
        // A script on the command line is read after the words its options take. A lone `-` ends the options, so the
        // `-o` after it is the command text, and the run-time input is only an argument of it: `pipefail` is named,
        // and no withheld value is needed.
        for (text, expected) in [
            ("bash -c - 'rm x'", vec!["bash", "rm"]),
            ("bash -c + 'rm x'", vec!["bash", "+", "rm"]),
            ("bash -co pipefail 'rm x'", vec!["bash", "pipefail", "rm"]),
            ("bash -cO extglob 'rm x'", vec!["bash", "extglob", "rm"]),
            (
                "bash -coo pipefail errexit 'rm x'",
                vec!["bash", "pipefail", "errexit", "rm"],
            ),
            (
                "bash -c -oe pipefail 'rm x'",
                vec!["bash", "pipefail", "rm"],
            ),
            ("mksh -cT tty 'rm x'", vec!["mksh", "tty", "rm"]),
            (
                "echo 'rm x' | xargs -0 bash -c - -o pipefail",
                vec!["echo", "xargs", "bash", "pipefail"],
            ),
            (
                "echo 'rm x' | xargs -0 sh -c - -o noglob",
                vec!["echo", "xargs", "sh", "noglob"],
            ),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[test]
    fn a_bash_long_option_is_read_ahead_of_its_short_options() {
        // bash reads its long options first, as the leading words, with one dash or two: `-rcfile x` and `-init-file x` take
        // the next word as their value, and `-restricted`, `-login` and the others take none. The first word that is no long
        // option ends that phase, and a single-dash word after it is short letters again, so in `-e -rcfile` the `c` is the
        // `-c` flag. With no command string after the options, the text is withheld, as a script file's is.
        for (text, expected) in [
            (
                "echo 'rm x' | xargs -0 bash -rcfile x -c",
                vec!["echo", "xargs", "bash", ""],
            ),
            ("xargs -0 rbash -init-file x -c", vec!["xargs", "rbash", ""]),
            (
                "xargs -0 bash -login -restricted -c",
                vec!["xargs", "bash", ""],
            ),
            ("bash -restricted s.sh", vec!["bash", ""]),
            ("bash -rcfile x s.sh", vec!["bash", ""]),
            ("bash -init-file x -c", vec!["bash", ""]),
            ("bash -rcfile", vec!["bash", ""]),
            ("bash -rcfile -c 'rm x'", vec!["bash", ""]),
            ("bash -e -login -c 'rm x'", vec!["bash", ""]),
            // The command string after the long options is read, as it is with none before it.
            ("bash -rcfile x -c 'rm x'", vec!["bash", "rm"]),
            ("bash -restricted -c 'rm x'", vec!["bash", "rm"]),
            ("bash -login -c 'rm x'", vec!["bash", "rm"]),
            ("bash --rcfile x -c 'rm x'", vec!["bash", "rm"]),
            ("bash -posix -protected -c 'rm x'", vec!["bash", "rm"]),
            ("rbash -rcfile x -e -c 'rm x'", vec!["rbash", "rm"]),
            // Not leading, `-rcfile` is short letters, so its `c` is the `-c` flag and `rm x` is the command string.
            ("bash -e -rcfile 'rm x'", vec!["bash", "rm"]),
        ] {
            assert_eq!(command_prefixes(text), expected, "{text}");
        }
    }

    #[tokio::test]
    async fn a_bash_long_option_before_its_dash_c_text_leaves_that_text_read() {
        // The command string after a leading long option is read, so a deny on the command it runs denies the call.
        for input in [
            r#"{"program":"bash","args":["-rcfile","x","-c","rm x"]}"#,
            r#"{"program":"bash","args":["-restricted","-c","rm x"]}"#,
            r#"{"program":"bash","args":["-login","-c","rm x"]}"#,
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert("rm".to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input("shell", input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
        }
        // Nothing is withheld for these, so a deny on the withheld value alone leaves the call allowed.
        for input in [
            r#"{"program":"bash","args":["-rcfile","x","-c","echo hi"]}"#,
            r#"{"program":"bash","args":["-restricted","-c","echo hi"]}"#,
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert(String::new(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input("shell", input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allow is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Allow, "{input}");
        }
    }

    #[tokio::test]
    async fn a_script_supplied_at_run_time_is_denied_when_its_withheld_value_is_denied() {
        // The run-time script names no command the reader can read, so the call names the withheld value, which covers
        // every command rule: a deny on it denies the call.
        for (tool, input) in [
            ("shell", r#"{"program":"xargs","args":["-0","sh","-c"]}"#),
            ("shell", r#"{"program":"xargs","args":["-i","sh","-c","{}"]}"#),
            (
                "shell",
                r#"{"program":"zsh","args":["-c","a=';'; b=rm; c=' z'; emulate sh -c -- \"-$a$b$c\""]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"echo 'rm -rf x' | xargs -0 sh -c"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"echo 'rm x' | xargs -I X sh -c X"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"find . -exec sh -c 'echo {}' \\;"}"#,
            ),
            (
                "shell",
                r#"{"program":"find","args":[".","-exec","sh","-c","echo {}",";"]}"#,
            ),
            (
                "shell",
                r#"{"program":"xargs","args":["-0","bash","-c","+","-o","pipefail"]}"#,
            ),
            ("shell", r#"{"program":"tcsh","args":["-c","rm x"]}"#),
            ("shell", r#"{"program":"nu","args":["-c","^rm x"]}"#),
            (
                "shell",
                r#"{"program":"xargs","args":["-0","bash","-rcfile","x","-c"]}"#,
            ),
            (
                "shell",
                r#"{"program":"xargs","args":["-0","rbash","-init-file","x","-c"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"echo 'rm x' | xargs -0 bash -rcfile x -c"}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert(String::new(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                target_pairs(&sent).contains(&("command_prefix".to_owned(), String::new())),
                "{input}: {:?}",
                target_pairs(&sent)
            );
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_a_script_supplied_at_run_time_names_no_command() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            ("shell", r#"{"program":"xargs","args":["-0","sh","-c"]}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"find . -exec sh -c {} \\;"}"#,
            ),
            ("shell", r#"{"program":"nu","args":["-c","^rm x"]}"#),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 3, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }

    #[tokio::test]
    async fn a_program_that_runs_a_command_is_denied_when_any_name_it_gives_is_denied() {
        // The call is checked once for each name it gives, and the strictest answer decides it: the program itself, the
        // command it runs, a command nested in another, and the withheld value each deny the call on their own.
        for (tool, input, denied) in [
            (
                "shell",
                r#"{"program":"setsid","args":["-f","rm","x"]}"#,
                "rm",
            ),
            (
                "shell",
                r#"{"program":"fakeroot","args":["rm","x"]}"#,
                "fakeroot",
            ),
            ("shell", r#"{"program":"fakeroot","args":["rm","x"]}"#, ""),
            (
                "shell",
                r#"{"program":"xvfb-run","args":["-a","rm","x"]}"#,
                "",
            ),
            (
                "shell",
                r#"{"program":"cgexec","args":["-g","memory:/x","rm","x"]}"#,
                "",
            ),
            ("shell", r#"{"program":"fakeroot","args":[]}"#, ""),
            ("shell", r#"{"program":"chroot","args":["/srv"]}"#, ""),
            (
                "shell",
                r#"{"program":"firejail","args":["--noprofile","rm","x"]}"#,
                "",
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"env nice setsid rm x"}"#,
                "rm",
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"env nice setsid rm x"}"#,
                "setsid",
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"sudo env timeout 5 nice -n 1 chrt -o 0 rm x"}"#,
                "chrt",
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert(denied.to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(
                evaluation.decision,
                CapabilityDecision::Deny,
                "{input} with {denied:?} denied"
            );
        }
        // With every name allowed, the same call is allowed.
        let (mut policy, _state) = harness(ToolClassGate::Enforce).await;
        policy.permission_rules = true;
        let evaluation = policy
            .evaluate_tool_call_with_input(
                "shell",
                r#"{"program":"setsid","args":["-f","rm","x"]}"#,
                "run-a",
                "org-a",
                "user-a",
                false,
            )
            .await
            .expect("an allowed call evaluates");
        assert_eq!(evaluation.decision, CapabilityDecision::Allow);
    }

    #[tokio::test]
    async fn a_denied_command_run_by_find_or_by_a_table_program_is_denied_too() {
        for (tool, input) in [
            (
                "shell",
                r#"{"program":"find","args":[".","-exec","rm","{}","+"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"find . -exec rm {} \\;"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"xargs find . -exec rm {} +"}"#,
            ),
            ("shell", r#"{"program":"setsid","args":["rm","x"]}"#),
            ("shell", r#"{"program":"eatmydata","args":["rm","x"]}"#),
            ("shell", r#"{"program":"chrt","args":["-o","rm","x"]}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"chrt -o rm x"}"#,
            ),
            ("shell", r#"{"program":"chrt","args":["+10","rm","x"]}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"chrt -f +10 rm x"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"chrt ' 10' rm x"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"flock /tmp/l rm x"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"chroot /srv rm x"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"doas -n rm x"}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert("rm".to_owned(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value == "rm"
                }),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn a_withheld_command_is_denied_when_its_withheld_value_is_denied() {
        for (tool, input) in [
            ("shell", r#"{"program":"ssh","args":["host","rm","x"]}"#),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"flock /tmp/l -c 'rm x'"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"sudo -i 'rm x'"}"#,
            ),
            ("shell", r#"{"program":"su","args":["-c","rm x","root"]}"#),
            (
                "shell",
                r#"{"program":"ssh-agent","args":["host","rm","x"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"npm run build"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"nix-shell --run 'rm x'"}"#,
            ),
        ] {
            let (mut policy, state) = harness(ToolClassGate::Enforce).await;
            policy.permission_rules = true;
            state
                .decision_by_target
                .lock()
                .unwrap()
                .insert(String::new(), "deny".to_owned());
            let evaluation = policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("a deny is an evaluation, not an error");
            assert_eq!(evaluation.decision, CapabilityDecision::Deny, "{input}");
            let sent = state.evaluated.lock().unwrap();
            assert!(
                sent.iter().any(|request| {
                    request.target_kind == "command_prefix" && request.target_value.is_empty()
                }),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn with_the_permission_rules_off_a_table_less_or_withheld_program_names_no_command() {
        let (policy, state) = harness(ToolClassGate::Off).await;
        for (tool, input) in [
            (
                "shell",
                r#"{"program":"find","args":[".","-exec","rm","{}","+"]}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"ssh host rm x"}"#,
            ),
            (
                "code_interpreter",
                r#"{"language":"sh","code":"sudo -i 'rm x'"}"#,
            ),
        ] {
            policy
                .evaluate_tool_call_with_input(tool, input, "run-a", "org-a", "user-a", false)
                .await
                .expect("an allowed call evaluates");
        }
        let sent = state.evaluated.lock().unwrap();
        assert_eq!(sent.len(), 3, "one request per call, no extra checks");
        for request in sent.iter() {
            assert_eq!(request.tool_name, "");
            assert_eq!(request.user_id, "");
            assert_eq!(request.target_kind, "");
            assert_eq!(request.target_value, "");
        }
    }
}
