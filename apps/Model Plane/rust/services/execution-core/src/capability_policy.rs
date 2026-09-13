//! Authoritative capability policy enforcement for execution dispatch.
//!
//! Tool names are mapped to capability identifiers by trusted server code;
//! callers and model output can never pair an allowed capability with a
//! different executor. Unknown tools fail closed before dispatch.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use mp_contracts::model_plane::v1::{
    capability_core_client::CapabilityCoreClient, EvaluatePolicyRequest, EvaluatePolicyResponse,
};
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
        let response = self.evaluate_response(tool_name, run_id, org_id).await?;
        map_capability_response(&response).map(|evaluation| evaluation.decision)
    }

    async fn evaluate_with_evidence(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
    ) -> Result<CapabilityEvaluation, Status> {
        let response = self.evaluate_response(tool_name, run_id, org_id).await?;
        let evaluation = map_capability_response(&response)?;
        if evaluation.decision == CapabilityDecision::Allow
            && !self.decision_verifier.verify(
                &response.decision_evidence,
                &response,
                tool_name,
                run_id,
                org_id,
            )
        {
            return Err(Status::permission_denied(
                "capability policy returned invalid decision evidence",
            ));
        }
        Ok(CapabilityEvaluation {
            evidence_verified: evaluation.decision != CapabilityDecision::Allow
                || self.decision_verifier.is_configured(),
            ..evaluation
        })
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

impl GrpcCapabilityPolicy {
    async fn evaluate_response(
        &self,
        tool_name: &str,
        run_id: &str,
        org_id: &str,
    ) -> Result<EvaluatePolicyResponse, Status> {
        if run_id.trim().is_empty() || org_id.trim().is_empty() {
            return Err(Status::invalid_argument(
                "run and tenant are required for capability policy",
            ));
        }
        let capability_id = trusted_capability_id(tool_name)
            .ok_or_else(|| Status::permission_denied("tool has no governed capability binding"))?;
        let bearer = self.tokens.token(org_id).await.map_err(|error| {
            tracing::warn!(%error, "capability service credential unavailable");
            Status::unavailable("capability policy credential unavailable")
        })?;
        let mut request = Request::new(EvaluatePolicyRequest {
            capability_id,
            run_id: run_id.to_owned(),
            agent_id: "execution-core".to_owned(),
            org_id: org_id.to_owned(),
            scope: "global".to_owned(),
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
        Ok(response.into_inner())
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
        tool_name: &str,
        run_id: &str,
        org_id: &str,
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
        #[derive(Deserialize)]
        struct Claims {
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
        }
        let Ok(header) = serde_json::from_slice::<Header>(&header) else {
            return false;
        };
        let Ok(claims) = serde_json::from_slice::<Claims>(&payload) else {
            return false;
        };
        if header.alg != "EdDSA"
            || header.typ != "model-plane.capability-decision+jws"
            || claims.v != "1"
            || claims.decision_id != response.decision_id
            || claims.capability_version != response.capability_version
            || claims.capability_id != trusted_capability_id(tool_name).unwrap_or_default()
            || claims.org_id != org_id
            || claims.run_id != run_id
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
        key.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .is_ok()
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
        "list_provider_actions" => "cap.tool.provider.read",
        "execute_provider_action" => "cap.tool.provider.execute",
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

    fn signed_evidence(
        key: &SigningKey,
        response: &EvaluatePolicyResponse,
        iat: i64,
        exp: i64,
    ) -> String {
        let header = serde_json::json!({
            "alg": "EdDSA",
            "kid": "capability-decision-v1",
            "typ": "model-plane.capability-decision+jws"
        });
        let claims = serde_json::json!({
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
        });
        let encoder = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let encoded_header = encoder.encode(serde_json::to_vec(&header).expect("header JSON"));
        let encoded_claims = encoder.encode(serde_json::to_vec(&claims).expect("claims JSON"));
        let signing_input = format!("{encoded_header}.{encoded_claims}");
        let signature = key.sign(signing_input.as_bytes());
        format!("{signing_input}.{}", encoder.encode(signature.to_bytes()))
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

        assert!(verifier.verify(&evidence, &response, "shell", "run-a", "org-a"));
        assert!(!verifier.verify(&evidence, &response, "shell", "run-other", "org-a"));
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
        assert!(verifier.verify(&valid, &response, "shell", "run-a", "org-a"));
        assert!(!verifier.verify(&tampered, &response, "shell", "run-a", "org-a"));

        let expired = signed_evidence(&signing_key, &response, now - 120, now - 60);
        assert!(!verifier.verify(&expired, &response, "shell", "run-a", "org-a"));
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
}
