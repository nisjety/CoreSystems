//! Governed owner-action adapter for Conversation Core tickets.
//!
//! This module intentionally performs a fixed two-hop sequence:
//!
//! 1. Control Plane re-resolves the source run and current Space policy, then
//!    signs a short-lived, target-bound decision.
//! 2. Conversation Core verifies that decision, recomputes this exact payload
//!    commitment, and authorizes the target conversation in its own tenant
//!    before it writes a durable owner receipt.
//!
//! The execution runtime never receives a target resource grant and never
//! treats the source thread authorization as one. Empty or partial deployment
//! configuration disables the adapter, so it is safe to compile everywhere
//! while remaining unoffered and incapable of egress in default dev.

use std::time::Duration;

use base64::Engine as _;
use chrono::Utc;
use hmac::{Hmac, Mac};
use rand::RngCore;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::control_http_client::{
    bounded_secret, env_flag, env_value, service_base_url, service_endpoint as endpoint,
};
use crate::tool_bridge::ToolExecution;

pub const TOOL_NAME: &str = "tickets.create";
pub const CAPABILITY_ID: &str = "cap.tool.ticket.create";

const CONTROL_DECISION_PATH: &str = "/api/v1/internal/spaces/run-action-decision";
const CONTROL_MODEL_ACTION_VIEW_PATH: &str = "/api/v1/internal/spaces/model-action-view";
const APPLICATION_TICKET_PATH: &str = "/internal/v1/agent-ticket-operations";
const APPLICATION_TICKET_RECONCILE_PATH: &str = "/internal/v1/agent-ticket-operations/reconcile";
pub const FAILURE_APPROVAL_NOT_GRANTED: &str = "approval_not_granted";
pub const FAILURE_UNKNOWN_OUTCOME: &str = "unknown_outcome";
pub const FAILURE_DEFINITELY_NOT_ACCEPTED: &str = "definitely_not_accepted";
// The Application owner contract is versioned and independently checked by
// Conversation Core. A mismatch is a denial at the owner, never a downgrade.
pub const TICKET_CREATE_SCHEMA_SHA256: &str =
    "sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df";
// Capability Core returns this exact server-owned definition only after it has
// verified the signed Control run view and fresh capability health. Keeping the
// bytes pinned here means a compromised catalog response cannot change the
// executor's accepted input surface while retaining the same schema hash.
pub const TICKET_CREATE_MODEL_PARAMETERS_JSON: &str = r#"{"type":"object","additionalProperties":false,"properties":{"conversation_id":{"type":"string","maxLength":200},"work_type":{"type":"string","enum":["","customer_case","internal_work","incident"]},"priority":{"type":"string","enum":["","low","normal","high","urgent"]},"severity":{"type":"string","enum":["","low","medium","high","critical"]},"category":{"type":"string","maxLength":80},"intent":{"type":"string","maxLength":240}},"required":["conversation_id"]}"#;
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct AgentTicketActionClient {
    control_base_url: Url,
    control_service_token: String,
    control_model_action_view_token: Option<String>,
    application_base_url: Url,
    application_delegation_secret: String,
    http: Client,
}

// This client is intentionally construction-friendly in tests and startup
// diagnostics, but it carries three credentials. Never let an incidental
// `{:?}` in a tracing field turn a configuration error into a secret leak.
impl std::fmt::Debug for AgentTicketActionClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentTicketActionClient")
            .field("control_base_url", &self.control_base_url)
            .field("control_service_token", &"[REDACTED]")
            .field(
                "control_model_action_view_token",
                &self
                    .control_model_action_view_token
                    .as_ref()
                    .map(|_| "[REDACTED]"),
            )
            .field("application_base_url", &self.application_base_url)
            .field("application_delegation_secret", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TicketCreateArguments {
    conversation_id: String,
    #[serde(default)]
    work_type: String,
    #[serde(default)]
    priority: String,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    intent: String,
}

#[derive(Debug, Clone, Serialize)]
struct TicketPayloadCommitment<'a> {
    action_id: &'static str,
    run_id: &'a str,
    org_id: &'a str,
    idempotency_key: &'a str,
    conversation_id: &'a str,
    work_type: &'a str,
    priority: &'a str,
    severity: &'a str,
    category: &'a str,
    intent: &'a str,
}

#[derive(Debug, Serialize)]
struct ControlDecisionRequest<'a> {
    run_id: &'a str,
    org_id: &'a str,
    action_id: &'static str,
    action_schema_hash: &'static str,
    payload_digest: &'a str,
    idempotency_key: &'a str,
}

#[derive(Debug, Serialize)]
struct ControlModelActionViewRequest<'a> {
    run_id: &'a str,
    org_id: &'a str,
}

#[derive(Debug, Deserialize)]
struct ControlResponseEnvelope {
    data: ControlDecisionData,
}

#[derive(Debug, Deserialize)]
struct ControlDecisionData {
    token: String,
}

#[derive(Debug, Serialize)]
struct ApplicationTicketRequest<'a> {
    run_id: &'a str,
    control_decision_token: &'a str,
    idempotency_key: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    owner_user_id: &'a str,
    conversation_id: &'a str,
    work_type: &'a str,
    priority: &'a str,
    severity: &'a str,
    category: &'a str,
    intent: &'a str,
}

#[derive(Debug, Serialize)]
struct ApplicationTicketReconcileRequest<'a> {
    run_id: &'a str,
    org_id: &'a str,
    control_decision_token: &'a str,
    action_schema_hash: &'static str,
    payload_digest: &'a str,
    idempotency_key: &'a str,
}

#[derive(Debug, Deserialize)]
struct ApplicationResponseEnvelope {
    data: ApplicationResponseData,
}

#[derive(Debug, Deserialize)]
struct ApplicationResponseData {
    ticket: ApplicationTicket,
    operation: ApplicationOperation,
}

#[derive(Debug, Deserialize)]
struct ApplicationTicket {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ApplicationOperation {
    operation_id: String,
    status: String,
    #[serde(default)]
    replayed: bool,
}

impl AgentTicketActionClient {
    /// Returns `None` only if no relevant setting is configured. A partial
    /// configuration is an error: a caller must not confuse bad wiring with a
    /// deliberate disabled state.
    pub fn from_env() -> Result<Option<Self>, String> {
        let values = [
            env_value("CONTROL_PLANE_USER_CORE_URL"),
            env_value("EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN"),
            env_value("CONVERSATION_CORE_AGENT_ACTION_URL"),
            env_value("CONVERSATION_EXECUTION_CORE_SERVICE_TOKEN"),
        ];
        let model_action_view_token = env_value("EXECUTION_CORE_CONTROL_MODEL_ACTION_VIEW_TOKEN");

        if values.iter().all(Option::is_none) {
            if model_action_view_token.is_some() {
                return Err("EXECUTION_CORE_CONTROL_MODEL_ACTION_VIEW_TOKEN requires the complete ticket action configuration".to_owned());
            }
            return Ok(None);
        }
        if values.iter().any(Option::is_none) {
            return Err(
                "ticket action requires CONTROL_PLANE_USER_CORE_URL, EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN, CONVERSATION_CORE_AGENT_ACTION_URL, and CONVERSATION_EXECUTION_CORE_SERVICE_TOKEN together".to_owned(),
            );
        }
        let allow_insecure_loopback = env_flag("EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK");
        let mut client = Self::new_with_transport(
            values[0].as_deref().expect("checked"),
            values[1].as_deref().expect("checked"),
            values[2].as_deref().expect("checked"),
            values[3].as_deref().expect("checked"),
            allow_insecure_loopback,
        )?;
        if let Some(token) = model_action_view_token {
            client.control_model_action_view_token = Some(bounded_secret(
                &token,
                "Control model-action-view service token",
            )?);
        }
        Ok(Some(client))
    }

    pub fn new(
        control_base_url: &str,
        control_service_token: &str,
        application_base_url: &str,
        application_delegation_secret: &str,
    ) -> Result<Self, String> {
        Self::new_with_transport(
            control_base_url,
            control_service_token,
            application_base_url,
            application_delegation_secret,
            false,
        )
    }

    /// Constructs the adapter with an explicit transport posture. Plain HTTP
    /// is rejected for service names and remote/private-network hosts. The
    /// only development exception is an explicit opt-in for IP-loopback
    /// endpoints, which keeps test/dev harnesses usable without turning an
    /// internal Docker network into an implicit authentication boundary.
    pub fn new_with_transport(
        control_base_url: &str,
        control_service_token: &str,
        application_base_url: &str,
        application_delegation_secret: &str,
        allow_insecure_loopback: bool,
    ) -> Result<Self, String> {
        let control_base_url =
            service_base_url(control_base_url, "Control Plane", allow_insecure_loopback)?;
        let application_base_url = service_base_url(
            application_base_url,
            "Conversation Core",
            allow_insecure_loopback,
        )?;
        let control_service_token =
            bounded_secret(control_service_token, "Control run-action service token")?;
        let application_delegation_secret = bounded_secret(
            application_delegation_secret,
            "Conversation Core execution delegation secret",
        )?;
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(HTTP_TIMEOUT);
        // This lane requires HTTPS to every non-loopback peer, but reqwest is
        // built here with rustls + webpki-roots, which trusts the bundled public
        // CA set and ignores the system trust store entirely -- SSL_CERT_FILE and
        // /etc/ssl/certs do nothing. A privately-signed internal endpoint is
        // therefore unreachable unless its CA is added explicitly.
        //
        // Additive only: the public roots stay trusted, so setting this cannot
        // downgrade verification of a publicly-signed peer. Unset means unchanged
        // behaviour. A configured-but-unreadable or malformed bundle is a hard
        // error rather than a silent fallback to the public roots, because that
        // fallback would look identical to a working deployment right up until
        // the first request failed.
        if let Some(path) = env_value("EXECUTION_CORE_TICKET_CA_BUNDLE") {
            let pem = std::fs::read(&path).map_err(|error| {
                format!("ticket action CA bundle {path} is unreadable: {error}")
            })?;
            let anchors = reqwest::Certificate::from_pem_bundle(&pem)
                .map_err(|_| format!("ticket action CA bundle {path} is not valid PEM"))?;
            if anchors.is_empty() {
                return Err(format!(
                    "ticket action CA bundle {path} contains no certificate"
                ));
            }
            for anchor in anchors {
                builder = builder.add_root_certificate(anchor);
            }
        }
        let http = builder
            .build()
            .map_err(|_| "ticket action HTTP client is unavailable".to_owned())?;
        Ok(Self {
            control_base_url,
            control_service_token,
            control_model_action_view_token: None,
            application_base_url,
            application_delegation_secret,
            http,
        })
    }

    pub async fn execute(
        &self,
        tool_input: &str,
        run_id: &str,
        org_id: &str,
        step_id: &str,
    ) -> Result<String, String> {
        let arguments = parse_arguments(tool_input)?;
        let run_id = required_bound_identifier(run_id, "run")?;
        let org_id = required_bound_identifier(org_id, "organization")?;
        let step_id = required_bound_identifier(step_id, "step")?;
        let idempotency_key = derived_idempotency_key(&run_id, &step_id);
        // Control intentionally does not accept an actor or target resource
        // from execution. It resolves the former from Session Core and binds it
        // into its signed decision; the payload commitment itself binds the
        // run, organization, derived retry key, and owner action fields.
        let payload_digest = payload_digest(&run_id, &org_id, &idempotency_key, &arguments);
        let token = self
            .request_control_decision(&run_id, &org_id, &payload_digest, &idempotency_key)
            .await?;

        // Conversation Core verifies the signature and binds the exact payload
        // before it derives the actor from the decision for its owner write.
        self.request_ticket_operation(&token, &run_id, &org_id, &idempotency_key, "", &arguments)
            .await
    }

    /// Resume a frozen approval descriptor only when every immutable contract
    /// fact agrees with the current ticket adapter. This is intentionally
    /// stricter than `execute`: the continuation worker must not silently
    /// recompute a different schema, payload, or retry key from mutable input.
    pub async fn execute_approved_continuation(
        &self,
        tool_input: &str,
        run_id: &str,
        org_id: &str,
        step_id: &str,
        schema_sha256: &str,
        payload_sha256: &str,
        ticket_idempotency_key: &str,
        owner_user_id: &str,
    ) -> Result<String, String> {
        if schema_sha256 != TICKET_CREATE_SCHEMA_SHA256
            || owner_user_id.trim().is_empty()
            || !payload_sha256.starts_with("sha256:")
        {
            return Err("invalid_continuation".to_owned());
        }
        let arguments = parse_arguments(tool_input)?;
        let run_id = required_bound_identifier(run_id, "run")?;
        let org_id = required_bound_identifier(org_id, "organization")?;
        let step_id = required_bound_identifier(step_id, "step")?;
        let idempotency_key = derived_idempotency_key(&run_id, &step_id);
        if idempotency_key != ticket_idempotency_key {
            return Err("invalid_continuation".to_owned());
        }
        let computed = payload_digest(&run_id, &org_id, &idempotency_key, &arguments);
        if computed != payload_sha256 {
            return Err("invalid_continuation".to_owned());
        }
        let token = self
            .request_control_decision(&run_id, &org_id, &computed, &idempotency_key)
            .await?;
        self.request_ticket_operation(
            &token,
            &run_id,
            &org_id,
            &idempotency_key,
            owner_user_id,
            &arguments,
        )
        .await
    }

    /// Resolves the Model-facing ticket definition from Control for one active
    /// run. This bearer is view-only: it cannot be submitted to Conversation
    /// Core and carries no payload or target-resource grant.
    pub async fn request_model_action_view(
        &self,
        run_id: &str,
        org_id: &str,
    ) -> Result<String, String> {
        let run_id = required_bound_identifier(run_id, "run")?;
        let org_id = required_bound_identifier(org_id, "organization")?;
        let control_service_token =
            self.control_model_action_view_token
                .as_deref()
                .ok_or_else(|| {
                    "tickets.create model action view is not configured for this execution runtime"
                        .to_owned()
                })?;
        let endpoint = endpoint(&self.control_base_url, CONTROL_MODEL_ACTION_VIEW_PATH)?;
        let response = self
            .http
            .post(endpoint)
            .header("x-service-id", "execution-core")
            .header("x-service-token", control_service_token)
            .json(&ControlModelActionViewRequest {
                run_id: &run_id,
                org_id: &org_id,
            })
            .send()
            .await
            .map_err(|_| "Control model action view is unavailable".to_owned())?;
        if !response.status().is_success() {
            return Err("Control denied or could not issue the model action view".to_owned());
        }
        let envelope = response
            .json::<ControlResponseEnvelope>()
            .await
            .map_err(|_| "Control returned an invalid model action view".to_owned())?;
        let token = envelope.data.token.trim();
        if token.is_empty() || token.len() > 16_384 {
            return Err("Control returned an invalid model action view".to_owned());
        }
        Ok(token.to_owned())
    }

    #[must_use]
    pub fn model_action_view_is_configured(&self) -> bool {
        self.control_model_action_view_token.is_some()
    }

    async fn request_control_decision(
        &self,
        run_id: &str,
        org_id: &str,
        payload_digest: &str,
        idempotency_key: &str,
    ) -> Result<String, String> {
        let endpoint = endpoint(&self.control_base_url, CONTROL_DECISION_PATH)?;
        let response = self
            .http
            .post(endpoint)
            .header("x-service-id", "execution-core")
            .header("x-service-token", &self.control_service_token)
            .json(&ControlDecisionRequest {
                run_id,
                org_id,
                action_id: TOOL_NAME,
                action_schema_hash: TICKET_CREATE_SCHEMA_SHA256,
                payload_digest,
                idempotency_key,
            })
            .send()
            .await
            .map_err(|_| "Control owner-action decision is unavailable".to_owned())?;
        if !response.status().is_success() {
            if response.status().is_client_error() {
                return Err(FAILURE_APPROVAL_NOT_GRANTED.to_owned());
            }
            return Err("Control denied or could not issue the owner-action decision".to_owned());
        }
        let envelope = response
            .json::<ControlResponseEnvelope>()
            .await
            .map_err(|_| "Control returned an invalid owner-action decision".to_owned())?;
        let token = envelope.data.token.trim();
        if token.is_empty() || token.len() > 16_384 {
            return Err("Control returned an invalid owner-action decision".to_owned());
        }
        Ok(token.to_owned())
    }

    async fn request_ticket_operation(
        &self,
        control_decision_token: &str,
        run_id: &str,
        org_id: &str,
        idempotency_key: &str,
        owner_user_id: &str,
        arguments: &TicketCreateArguments,
    ) -> Result<String, String> {
        let endpoint = endpoint(&self.application_base_url, APPLICATION_TICKET_PATH)?;
        let payload = serde_json::to_vec(&ApplicationTicketRequest {
            run_id,
            control_decision_token,
            idempotency_key,
            owner_user_id,
            conversation_id: &arguments.conversation_id,
            work_type: &arguments.work_type,
            priority: &arguments.priority,
            severity: &arguments.severity,
            category: &arguments.category,
            intent: &arguments.intent,
        })
        .map_err(|_| "ticket action request could not be encoded".to_owned())?;
        let headers = application_delegation_headers(
            &self.application_delegation_secret,
            endpoint.path(),
            &payload,
        )?;
        let response = match self
            .http
            .post(endpoint)
            .header("content-type", "application/json")
            .headers(headers)
            .body(payload)
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => {
                return self
                    .reconcile_ticket_operation(
                        control_decision_token,
                        run_id,
                        org_id,
                        idempotency_key,
                        &payload_digest(run_id, org_id, idempotency_key, arguments),
                    )
                    .await;
            }
        };
        if !response.status().is_success() {
            if response.status().is_client_error() {
                return Err(FAILURE_APPROVAL_NOT_GRANTED.to_owned());
            }
            return self
                .reconcile_ticket_operation(
                    control_decision_token,
                    run_id,
                    org_id,
                    idempotency_key,
                    &payload_digest(run_id, org_id, idempotency_key, arguments),
                )
                .await;
        }
        let envelope = match response.json::<ApplicationResponseEnvelope>().await {
            Ok(envelope) => envelope,
            Err(_) => {
                return self
                    .reconcile_ticket_operation(
                        control_decision_token,
                        run_id,
                        org_id,
                        idempotency_key,
                        &payload_digest(run_id, org_id, idempotency_key, arguments),
                    )
                    .await;
            }
        };
        if envelope.data.ticket.id.trim().is_empty()
            || envelope.data.operation.operation_id.trim().is_empty()
            || envelope.data.operation.status.trim().is_empty()
        {
            return Err("Conversation Core returned an invalid owner receipt".to_owned());
        }
        serde_json::to_string(&serde_json::json!({
            "ticket_id": envelope.data.ticket.id,
            "operation_id": envelope.data.operation.operation_id,
            "status": envelope.data.operation.status,
            "replayed": envelope.data.operation.replayed,
        }))
        .map_err(|_| "ticket action receipt could not be encoded".to_owned())
    }

    async fn reconcile_ticket_operation(
        &self,
        control_decision_token: &str,
        run_id: &str,
        org_id: &str,
        idempotency_key: &str,
        payload_digest: &str,
    ) -> Result<String, String> {
        let endpoint = endpoint(
            &self.application_base_url,
            APPLICATION_TICKET_RECONCILE_PATH,
        )?;
        let payload = serde_json::to_vec(&ApplicationTicketReconcileRequest {
            run_id,
            org_id,
            control_decision_token,
            action_schema_hash: TICKET_CREATE_SCHEMA_SHA256,
            payload_digest,
            idempotency_key,
        })
        .map_err(|_| "ticket reconciliation request could not be encoded".to_owned())?;
        let headers = application_delegation_headers(
            &self.application_delegation_secret,
            endpoint.path(),
            &payload,
        )?;
        let response = self
            .http
            .post(endpoint)
            .header("content-type", "application/json")
            .headers(headers)
            .body(payload)
            .send()
            .await
            .map_err(|_| FAILURE_UNKNOWN_OUTCOME.to_owned())?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(FAILURE_DEFINITELY_NOT_ACCEPTED.to_owned());
        }
        if !response.status().is_success() {
            return Err(FAILURE_UNKNOWN_OUTCOME.to_owned());
        }
        let envelope = response
            .json::<ApplicationResponseEnvelope>()
            .await
            .map_err(|_| FAILURE_UNKNOWN_OUTCOME.to_owned())?;
        if envelope.data.ticket.id.trim().is_empty()
            || envelope.data.operation.operation_id.trim().is_empty()
            || envelope.data.operation.status.trim().is_empty()
        {
            return Err(FAILURE_UNKNOWN_OUTCOME.to_owned());
        }
        serde_json::to_string(&serde_json::json!({
            "ticket_id": envelope.data.ticket.id,
            "operation_id": envelope.data.operation.operation_id,
            "status": envelope.data.operation.status,
            "replayed": true,
        }))
        .map_err(|_| FAILURE_UNKNOWN_OUTCOME.to_owned())
    }
}

/// Executes only if every dedicated control/application credential is present.
/// Model-facing availability remains server-authoritative and is intentionally
/// not inferred from this local configuration predicate.
pub async fn execute_if_configured(
    tool_input: &str,
    run_id: &str,
    org_id: &str,
    step_id: &str,
) -> ToolExecution {
    match AgentTicketActionClient::from_env() {
        Ok(Some(client)) => match client.execute(tool_input, run_id, org_id, step_id).await {
            Ok(output) => ToolExecution {
                output,
                error: None,
            },
            Err(error) => ToolExecution {
                output: String::new(),
                error: Some(error),
            },
        },
        Ok(None) => ToolExecution {
            output: String::new(),
            error: Some("tickets.create is not configured for this execution runtime".to_owned()),
        },
        Err(error) => ToolExecution {
            output: String::new(),
            error: Some(error),
        },
    }
}

#[must_use]
pub fn is_configured() -> bool {
    matches!(AgentTicketActionClient::from_env(), Ok(Some(_)))
}

/// Compute the immutable continuation tuple for a frozen `tickets.create`
/// descriptor without contacting Control or Conversation Core.
pub fn continuation_binding(
    tool_input: &str,
    run_id: &str,
    org_id: &str,
    step_id: &str,
) -> Result<(String, String, String), String> {
    let arguments = parse_arguments(tool_input)?;
    let run_id = required_bound_identifier(run_id, "run")?;
    let org_id = required_bound_identifier(org_id, "organization")?;
    let step_id = required_bound_identifier(step_id, "step")?;
    let idempotency_key = derived_idempotency_key(&run_id, &step_id);
    let payload_sha256 = payload_digest(&run_id, &org_id, &idempotency_key, &arguments);
    Ok((
        TICKET_CREATE_SCHEMA_SHA256.to_owned(),
        payload_sha256,
        idempotency_key,
    ))
}

fn parse_arguments(input: &str) -> Result<TicketCreateArguments, String> {
    let mut arguments = serde_json::from_str::<TicketCreateArguments>(input)
        .map_err(|_| "tickets.create input is invalid".to_owned())?;
    arguments.conversation_id = bounded_value(&arguments.conversation_id, 200, "conversation_id")?;
    arguments.work_type = normalized_enum(
        &arguments.work_type,
        &["", "customer_case", "internal_work", "incident"],
        "work_type",
    )?;
    arguments.priority = normalized_enum(
        &arguments.priority,
        &["", "low", "normal", "high", "urgent"],
        "priority",
    )?;
    arguments.severity = normalized_enum(
        &arguments.severity,
        &["", "low", "medium", "high", "critical"],
        "severity",
    )?;
    arguments.category = bounded_optional_value(&arguments.category, 80, "category")?;
    arguments.intent = bounded_optional_value(&arguments.intent, 120, "intent")?;
    Ok(arguments)
}

fn payload_digest(
    run_id: &str,
    org_id: &str,
    idempotency_key: &str,
    arguments: &TicketCreateArguments,
) -> String {
    let payload = TicketPayloadCommitment {
        action_id: TOOL_NAME,
        run_id,
        org_id,
        idempotency_key,
        conversation_id: &arguments.conversation_id,
        work_type: &arguments.work_type,
        priority: &arguments.priority,
        severity: &arguments.severity,
        category: &arguments.category,
        intent: &arguments.intent,
    };
    let encoded = serde_json::to_vec(&payload).expect("fixed ticket payload serializes");
    format!("sha256:{:x}", Sha256::digest(encoded))
}

fn derived_idempotency_key(run_id: &str, step_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"tickets.create\x00");
    hasher.update(run_id.as_bytes());
    hasher.update(b"\x00");
    hasher.update(step_id.as_bytes());
    format!("tickets.create:{:x}", hasher.finalize())
}

fn application_delegation_headers(
    secret: &str,
    uri: &str,
    body: &[u8],
) -> Result<reqwest::header::HeaderMap, String> {
    let timestamp = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut nonce_bytes = [0_u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nonce_bytes);
    let body_digest = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(body));
    let canonical = [
        "v2",
        "execution-core",
        "conversation-core",
        &timestamp,
        &nonce,
        "POST",
        uri,
        "",
        "",
        "",
        &body_digest,
    ]
    .join("\n");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| "Conversation Core execution credential is invalid".to_owned())?;
    mac.update(canonical.as_bytes());
    let signature =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in [
        ("x-service-id", "execution-core".to_owned()),
        ("x-delegation-timestamp", timestamp),
        ("x-delegation-nonce", nonce),
        ("x-delegation-body-sha256", body_digest),
        ("x-delegation-signature", signature),
    ] {
        headers.insert(
            reqwest::header::HeaderName::from_static(name),
            value
                .parse()
                .map_err(|_| "Conversation Core delegation headers are invalid".to_owned())?,
        );
    }
    Ok(headers)
}

fn required_bound_identifier(value: &str, name: &str) -> Result<String, String> {
    bounded_value(value, 200, name)
}

fn bounded_value(value: &str, maximum: usize, name: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > maximum {
        return Err(format!("tickets.create {name} is invalid"));
    }
    Ok(value.to_owned())
}

fn bounded_optional_value(value: &str, maximum: usize, name: &str) -> Result<String, String> {
    let value = value.trim();
    if value.len() > maximum {
        return Err(format!("tickets.create {name} is invalid"));
    }
    Ok(value.to_owned())
}

fn normalized_enum(value: &str, allowed: &[&str], name: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(format!("tickets.create {name} is invalid"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_client(
        control_base_url: &str,
        control_service_token: &str,
        application_base_url: &str,
        application_delegation_secret: &str,
    ) -> Result<AgentTicketActionClient, String> {
        AgentTicketActionClient::new_with_transport(
            control_base_url,
            control_service_token,
            application_base_url,
            application_delegation_secret,
            true,
        )
    }

    #[test]
    fn ticket_transport_rejects_plaintext_service_or_private_hosts() {
        assert!(
            service_base_url("http://conversation-core:3160", "Conversation Core", false).is_err()
        );
        assert!(service_base_url("http://10.0.0.8:3160", "Conversation Core", true).is_err());
        assert!(AgentTicketActionClient::new(
            "http://127.0.0.1:8080",
            "control-test-secret-at-least-32-bytes",
            "https://conversation.example.test",
            "conversation-test-secret-at-least-32-bytes",
        )
        .is_err());
    }

    #[test]
    fn ticket_transport_allows_only_explicit_ip_loopback_development_http() {
        assert!(service_base_url("http://127.0.0.1:3160", "Conversation Core", false).is_err());
        assert!(service_base_url("http://127.0.0.1:3160", "Conversation Core", true).is_ok());
        assert!(service_base_url("http://[::1]:3160", "Conversation Core", true).is_ok());
    }

    #[tokio::test]
    async fn model_action_view_requires_its_own_control_scope_token() {
        let client = test_client(
            "https://control.example.test",
            "control-test-secret-at-least-32-bytes",
            "https://conversation.example.test",
            "conversation-test-secret-at-least-32-bytes",
        )
        .expect("ticket client");
        let error = client
            .request_model_action_view("run-1", "org-1")
            .await
            .expect_err("effect scope must not double as a model view scope");
        assert!(error.contains("model action view is not configured"));
    }

    #[test]
    fn payload_digest_matches_the_owner_contract_shape() {
        let arguments = parse_arguments(r#"{"conversation_id":"conv_1","work_type":"CUSTOMER_CASE","priority":"HIGH","category":" refund ","intent":"review"}"#).expect("valid arguments");
        let digest = payload_digest("run_1", "org_1", "ticket-key", &arguments);
        assert_eq!(
            digest,
            "sha256:bebabce524e06a9a546016c7587b3e5834746bad59a1996cdcba5b788a557a18"
        );
    }

    #[tokio::test]
    async fn approved_continuation_rejects_schema_or_retry_key_drift_before_network() {
        let client = test_client(
            "https://control.example.test",
            "control-test-secret-at-least-32-bytes",
            "https://conversation.example.test",
            "conversation-test-secret-at-least-32-bytes",
        )
        .expect("ticket client");
        let error = client
            .execute_approved_continuation(
                r#"{"conversation_id":"conv_1"}"#,
                "run_1",
                "org_1",
                "step_1",
                "sha256:wrong",
                "sha256:wrong",
                "tickets.create:wrong",
                "user_1",
            )
            .await
            .expect_err("schema drift must be rejected before egress");
        assert_eq!(error, "invalid_continuation");
    }

    #[tokio::test]
    async fn approved_continuation_carries_the_frozen_owner_to_the_owner_boundary() {
        let control = MockServer::start().await;
        let application = MockServer::start().await;
        let arguments = parse_arguments(
            r#"{"conversation_id":"conv_1","work_type":"customer_case","priority":"high","category":"refund","intent":"review"}"#,
        )
        .expect("valid arguments");
        let idempotency_key = derived_idempotency_key("run_1", "step_1");
        let digest = payload_digest("run_1", "org_1", &idempotency_key, &arguments);
        Mock::given(method("POST"))
            .and(path(CONTROL_DECISION_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data":{"token":"control-signed-decision"}})),
            )
            .expect(1)
            .mount(&control)
            .await;
        Mock::given(method("POST"))
            .and(path(APPLICATION_TICKET_PATH))
            .and(body_json(serde_json::json!({
                "run_id":"run_1", "control_decision_token":"control-signed-decision",
                "idempotency_key":idempotency_key, "owner_user_id":"user_1",
                "conversation_id":"conv_1", "work_type":"customer_case", "priority":"high",
                "severity":"", "category":"refund", "intent":"review",
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "data":{"ticket":{"id":"ticket_1"},"operation":{"operation_id":"op_1","status":"completed","replayed":false}}
            })))
            .expect(1)
            .mount(&application)
            .await;
        let client = test_client(
            &control.uri(),
            "control-test-secret-at-least-32-bytes",
            &application.uri(),
            "execution-core-test-secret-at-least-32-bytes",
        )
        .expect("client");
        let output = client
            .execute_approved_continuation(
                &serde_json::to_string(&arguments).expect("arguments JSON"),
                "run_1",
                "org_1",
                "step_1",
                TICKET_CREATE_SCHEMA_SHA256,
                &digest,
                &idempotency_key,
                "user_1",
            )
            .await
            .expect("owner receipt");
        assert!(output.contains("ticket_1"));
    }

    #[test]
    fn arguments_are_narrow_and_reject_actor_or_owner_fields() {
        assert!(
            parse_arguments(r#"{"conversation_id":"conv_1","actor_user_id":"forged"}"#).is_err()
        );
        assert!(parse_arguments(r#"{"conversation_id":"conv_1","team_id":"forged"}"#).is_err());
        assert!(parse_arguments(r#"{"conversation_id":"conv_1","work_type":"bad"}"#).is_err());
    }

    #[test]
    fn application_delegation_uses_no_user_or_organization_claim() {
        let headers = application_delegation_headers(
            "execution-core-test-secret-at-least-32-bytes",
            APPLICATION_TICKET_PATH,
            b"{}",
        )
        .expect("headers");
        assert_eq!(headers["x-service-id"], "execution-core");
        assert!(headers.get("x-user-id").is_none());
        assert!(headers.get("x-org-id").is_none());
        assert!(headers.get("x-user-role").is_none());
    }

    #[test]
    fn credential_bearing_ticket_client_debug_is_redacted() {
        let client = test_client(
            "https://control.example.test",
            "control-test-secret-at-least-32-bytes",
            "https://conversation.example.test",
            "conversation-test-secret-at-least-32-bytes",
        )
        .expect("ticket client");
        let debug = format!("{client:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("control-test-secret-at-least-32-bytes"));
        assert!(!debug.contains("conversation-test-secret-at-least-32-bytes"));
    }

    #[tokio::test]
    async fn two_hop_execution_requests_control_then_owner_and_returns_only_receipt() {
        let control = MockServer::start().await;
        let application = MockServer::start().await;
        let arguments = parse_arguments(r#"{"conversation_id":"conv_1","work_type":"customer_case","priority":"high","category":"refund","intent":"review"}"#).expect("valid arguments");
        let idempotency_key = derived_idempotency_key("run_1", "step_1");
        let provisional = payload_digest("run_1", "org_1", &idempotency_key, &arguments);
        Mock::given(method("POST"))
            .and(path(CONTROL_DECISION_PATH))
            .and(header("x-service-id", "execution-core"))
            .and(header(
                "x-service-token",
                "control-test-secret-at-least-32-bytes",
            ))
            .and(body_json(serde_json::json!({
                "run_id":"run_1", "org_id":"org_1", "action_id":TOOL_NAME,
                "action_schema_hash":TICKET_CREATE_SCHEMA_SHA256, "payload_digest":provisional,
                "idempotency_key":idempotency_key,
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data":{"token":"control-signed-decision"}})),
            )
            .expect(1)
            .mount(&control)
            .await;
        Mock::given(method("POST"))
            .and(path(APPLICATION_TICKET_PATH))
            .and(header("x-service-id", "execution-core"))
            .and(body_json(serde_json::json!({
                "run_id":"run_1", "control_decision_token":"control-signed-decision", "idempotency_key":idempotency_key,
                "conversation_id":"conv_1", "work_type":"customer_case", "priority":"high", "severity":"", "category":"refund", "intent":"review",
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "data":{"ticket":{"id":"ticket_1"},"operation":{"operation_id":"op_1","status":"completed","replayed":false}}
            })))
            .expect(1)
            .mount(&application)
            .await;
        let client = test_client(
            &control.uri(),
            "control-test-secret-at-least-32-bytes",
            &application.uri(),
            "execution-core-test-secret-at-least-32-bytes",
        )
        .expect("client");
        let output = client
            .execute(
                r#"{"conversation_id":"conv_1","work_type":"customer_case","priority":"high","category":"refund","intent":"review"}"#,
                "run_1",
                "org_1",
                "step_1",
            )
            .await
            .expect("owner receipt");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).expect("receipt JSON"),
            serde_json::json!({"ticket_id":"ticket_1","operation_id":"op_1","status":"completed","replayed":false})
        );
    }

    #[tokio::test]
    async fn ambiguous_owner_response_reconciles_without_replaying_the_effect() {
        let control = MockServer::start().await;
        let application = MockServer::start().await;
        let arguments =
            parse_arguments(r#"{"conversation_id":"conv_1"}"#).expect("valid arguments");
        let idempotency_key = derived_idempotency_key("run_1", "step_1");
        let digest = payload_digest("run_1", "org_1", &idempotency_key, &arguments);
        Mock::given(method("POST"))
            .and(path(CONTROL_DECISION_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data":{"token":"control-signed-decision"}})),
            )
            .expect(1)
            .mount(&control)
            .await;
        Mock::given(method("POST"))
            .and(path(APPLICATION_TICKET_PATH))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&application)
            .await;
        Mock::given(method("POST"))
            .and(path(APPLICATION_TICKET_RECONCILE_PATH))
            .and(body_json(serde_json::json!({
                "run_id":"run_1", "org_id":"org_1", "control_decision_token":"control-signed-decision",
                "action_schema_hash":TICKET_CREATE_SCHEMA_SHA256, "payload_digest":digest,
                "idempotency_key":idempotency_key,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data":{"ticket":{"id":"ticket_1"},"operation":{"operation_id":"op_1","status":"completed","replayed":true}}
            })))
            .expect(1)
            .mount(&application)
            .await;
        let client = test_client(
            &control.uri(),
            "control-test-secret-at-least-32-bytes",
            &application.uri(),
            "execution-core-test-secret-at-least-32-bytes",
        )
        .expect("client");
        let output = client
            .execute(
                r#"{"conversation_id":"conv_1"}"#,
                "run_1",
                "org_1",
                "step_1",
            )
            .await
            .expect("reconciled owner receipt");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).expect("receipt JSON"),
            serde_json::json!({"ticket_id":"ticket_1","operation_id":"op_1","status":"completed","replayed":true})
        );
    }

    #[tokio::test]
    async fn ambiguous_owner_response_with_no_receipt_is_safe_to_retry() {
        let control = MockServer::start().await;
        let application = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(CONTROL_DECISION_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data":{"token":"control-signed-decision"}})),
            )
            .expect(1)
            .mount(&control)
            .await;
        Mock::given(method("POST"))
            .and(path(APPLICATION_TICKET_PATH))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&application)
            .await;
        Mock::given(method("POST"))
            .and(path(APPLICATION_TICKET_RECONCILE_PATH))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&application)
            .await;
        let client = test_client(
            &control.uri(),
            "control-test-secret-at-least-32-bytes",
            &application.uri(),
            "execution-core-test-secret-at-least-32-bytes",
        )
        .expect("client");
        let error = client
            .execute(
                r#"{"conversation_id":"conv_1"}"#,
                "run_1",
                "org_1",
                "step_1",
            )
            .await
            .expect_err("missing receipt must not be called completed");
        assert_eq!(error, FAILURE_DEFINITELY_NOT_ACCEPTED);
    }

    #[tokio::test]
    async fn a_control_denial_never_calls_the_owner() {
        let control = MockServer::start().await;
        let application = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(CONTROL_DECISION_PATH))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&control)
            .await;
        Mock::given(method("POST"))
            .and(path(APPLICATION_TICKET_PATH))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&application)
            .await;
        let client = test_client(
            &control.uri(),
            "control-test-secret-at-least-32-bytes",
            &application.uri(),
            "execution-core-test-secret-at-least-32-bytes",
        )
        .expect("client");
        assert!(client
            .execute(
                r#"{"conversation_id":"conv_1"}"#,
                "run_1",
                "org_1",
                "step_1"
            )
            .await
            .is_err());
    }
}
