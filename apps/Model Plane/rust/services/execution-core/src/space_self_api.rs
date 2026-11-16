//! Space agent self-API adapter (WS18-S4): the execution-core side of the run-bound registry.
//!
//! A room agent acts on its own run's thread through the golden registry
//! (`mp_contracts::space_self_api`). This module is the only place execution-core offers or
//! executes those rows. It follows `ticket_tools` for the Control decision, and the effect is one
//! `SpaceRunActions.ApplyRunThreadAction` call that carries the decision, the typed arguments and
//! the run owner's delegated bearer. Session Core recomputes the payload digest itself, so the
//! digest here is computed over the same bytes, flattened the way `space/self_api.rs` flattens them.
//!
//! The offer stays narrow:
//! * The definitions are server-resolved only. Control lists the rows, Capability Core signs them,
//!   and every name, parameter byte, schema hash, approval flag and rung must equal the golden row.
//!   A view that names anything outside the registry is refused whole.
//! * Only a run whose tool profile is `space_self_api` is offered these rows (`offered_tools`).
//! * A row whose effective level is `ask` is not offered unless `EXECUTION_CORE_SPACE_SELF_API_RESUMABLE`
//!   is on, because approval resume for this kind is WS10-S9.
//! * A row this build cannot execute is dropped, never offered and never executed:
//!   `space.thread.post_card` until WS18-S7, and the phase 2 and 3 rows.
//!
//! Zero-retention and waived runs get the typed `zdr_durable_mutation_forbidden` refusal before any
//! Control call. `EXECUTION_CORE_SPACE_SELF_API` is a rollout switch and never an authorization input:
//! Control, Capability Core and Session Core each still verify their own part.

use std::collections::BTreeSet;
use std::sync::OnceLock;
use std::time::Duration;

use mp_contracts::autonomy::rung_from_keyword;
use mp_contracts::model_plane::v1::apply_run_thread_action_request::Args as ActionArgs;
use mp_contracts::model_plane::v1::space_run_actions_client::SpaceRunActionsClient;
use mp_contracts::model_plane::v1::{
    ApplyRunThreadActionRequest, ApplyRunThreadActionResponse, AutonomyRung, CardField,
    PinThreadArgs, PostCardArgs, ProbeRunThreadActionsRequest, ProbeRunThreadActionsResponse,
    RenameThreadArgs, ToolDefinition,
};
use mp_contracts::space_self_api as sa;
use mp_contracts::zdr_refusal::Refusal;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::transport::Channel;
use tonic::{Code, Request, Status};

use crate::capability_policy::ServiceTokenProvider;
use crate::control_http_client::{
    bounded_secret, env_flag, env_value, service_base_url, service_endpoint as endpoint,
};
use crate::session_terminal_auth::SessionTerminalTokenProvider;
use crate::tool_bridge::ToolExecution;

/// The `tool_profile` value that offers the Space registry rows and nothing else.
pub(crate) const TOOL_PROFILE: &str = "space_self_api";
/// The rollout switch of the view, the adapter and the probe. Unset or `0` offers no Space tool.
pub(crate) const SWITCH_ENV: &str = "EXECUTION_CORE_SPACE_SELF_API";
/// Lets an `ask` row be offered. Set only once WS10-S9 registers the resumable kind.
pub(crate) const RESUMABLE_ENV: &str = "EXECUTION_CORE_SPACE_SELF_API_RESUMABLE";

const LEGACY_ACTION_ID: &str = sa::LEGACY_ACTION_ID;
const RENAME_ACTION: &str = "space.thread.rename";
const PIN_ACTION: &str = "space.thread.pin";
const POST_CARD_ACTION: &str = "space.thread.post_card";
/// The card tones Session Core accepts (`CARD_TONES` in `space/self_api.rs`).
const CARD_TONES: [&str; 4] = ["info", "progress", "done", "blocked"];
/// The rows this build executes and attests. `space.thread.post_card` joins with WS18-S7 (its card write
/// and slot rule); the phase 2 (`space.agent.pause_self`) and phase 3 (`space.schedule.follow_up`) rows have
/// no executor here.
const BUILT_ACTIONS: [&str; 2] = [RENAME_ACTION, PIN_ACTION];
const MAX_VIEW_ACTIONS: usize = 8;
const MAX_VIEW_BYTES: usize = 32 << 10;
const MAX_TOKEN_BYTES: usize = 16_384;
const MAX_IDENTIFIER_BYTES: usize = 200;
const CONTROL_DECISION_PATH: &str = "/api/v1/internal/spaces/run-action-decision";
const CONTROL_VIEW_PATH: &str = "/api/v1/internal/spaces/model-action-view";
const CAPABILITY_RUN_VIEW_PATH: &str = "/api/v1/internal/model-actions/run-view";
const VIEW_SCOPE: &str = "capability:model-action:view";
/// The trailer Session Core sets on its typed refusals (`coded_status` in `space/self_api.rs`).
const SESSION_ERROR_TRAILER: &str = "x-mp-error-code";
const SESSION_WORDS: [&str; 8] = [
    "feature_disabled",
    "not_offered",
    "not_authorized",
    "run_not_active",
    "title_owned_by_human",
    "limit_reached",
    "idempotency_key_reused",
    "invalid_arguments",
];
const SESSION_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const SESSION_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const OUTCOME_APPLIED: i32 = 1;
const OUTCOME_REPLAYED: i32 = 2;
const OUTCOME_NOOP: i32 = 3;

/// Model-facing failure codes. Session Core's own words pass through unchanged (`SESSION_WORDS`).
pub(crate) const CODE_INVALID_ARGUMENTS: &str = "invalid_arguments";
pub(crate) const CODE_NOT_OFFERED: &str = "not_offered";
pub(crate) const CODE_NOT_AUTHORIZED: &str = "not_authorized";
pub(crate) const CODE_UNKNOWN_OUTCOME: &str = "unknown_outcome";
pub(crate) const CODE_DECISION_UNAVAILABLE: &str = "decision_unavailable";
pub(crate) const CODE_ZDR: &str = "zdr_durable_mutation_forbidden";

/// The golden registry this build was compiled with. A load failure is reported to every caller.
pub(crate) fn golden() -> Result<&'static sa::Registry, String> {
    sa::registry().map_err(|error| error.to_string())
}

fn registry_row(action_id: &str) -> Option<&'static sa::RegistryRow> {
    golden().ok().and_then(|registry| registry.row(action_id))
}

/// Whether a name is reserved for the run-bound resolver: a golden row (`tickets.create` included),
/// or any `space.*` name, so a client or MCP tool can never take the name even if the golden is absent.
#[must_use]
pub(crate) fn is_reserved(name: &str) -> bool {
    registry_row(name).is_some() || name.starts_with("space.")
}

/// Whether a name is a Space row, which this adapter serves (`tickets.create` has its own adapter).
#[must_use]
pub(crate) fn is_space_action(name: &str) -> bool {
    name != LEGACY_ACTION_ID && registry_row(name).is_some()
}

/// Whether a `tool_profile` value is the Space profile.
#[must_use]
pub(crate) fn is_profile(profile: &str) -> bool {
    profile == TOOL_PROFILE
}

/// The capability row that governs a Space row, for the policy gate.
#[must_use]
pub(crate) fn capability_id(name: &str) -> Option<String> {
    if !is_space_action(name) {
        return None;
    }
    registry_row(name).map(|row| row.capability_id.clone())
}

/// The autonomy rung a Space row needs. `None` for `tickets.create` and for any other name.
#[must_use]
pub(crate) fn autonomy_rung(name: &str) -> Option<AutonomyRung> {
    if !is_space_action(name) {
        return None;
    }
    registry_row(name).and_then(|row| rung_from_keyword(&row.autonomy_rung))
}

/// Whether a Space row is risky by its declared level and approval. `None` outside the Space rows,
/// where the name classifier decides as before. A row that Control and the golden allow without
/// approval is not risky, so `ask` mode does not pause on it: approval resume is not built for it.
#[must_use]
pub(crate) fn registry_risky(name: &str) -> Option<bool> {
    if !is_space_action(name) {
        return None;
    }
    registry_row(name).map(|row| row.default_level != "allow" || row.approval != "none")
}

/// Whether a golden row needs a durable owner approval before its adapter runs.
#[must_use]
pub(crate) fn requires_owner_approval(name: &str) -> bool {
    registry_row(name).is_some_and(|row| row.approval == "owner_approval")
}

/// The capabilities this build executes and may attest on the owner lane.
#[must_use]
pub(crate) fn attested_capability_ids() -> Vec<String> {
    BUILT_ACTIONS
        .iter()
        .filter_map(|action_id| registry_row(action_id).map(|row| row.capability_id.clone()))
        .collect()
}

/// Whether a row is executed by this build: the run-thread rows, and the room canvas rows while their own switch and configuration
/// are complete (`space_canvas_tools`, WS19-S9). Only the thread rows feed `attested_capability_ids`.
#[must_use]
fn built_action(name: &str) -> bool {
    BUILT_ACTIONS.contains(&name) || crate::space_canvas_tools::is_built_action(name)
}

/// Whether a resolved definition may join the offered set: a built Space row whose description is
/// non-empty and whose parameters are the golden bytes. Exact name, no trimming.
#[must_use]
pub(crate) fn admits(tool: &ToolDefinition) -> bool {
    is_space_action(&tool.name)
        && built_action(&tool.name)
        && !tool.description.trim().is_empty()
        && registry_row(&tool.name).is_some_and(|row| tool.parameters_json == row.parameters_json)
}

/// Whether the adapter and the view are switched on (`EXECUTION_CORE_SPACE_SELF_API`).
#[must_use]
pub(crate) fn enabled() -> bool {
    mp_contracts::agentic_subscription::switch_value_is_on(std::env::var(SWITCH_ENV).ok().as_deref())
}

/// Whether an `ask` row may be offered (`EXECUTION_CORE_SPACE_SELF_API_RESUMABLE`).
#[must_use]
pub(crate) fn resumable() -> bool {
    mp_contracts::agentic_subscription::switch_value_is_on(
        std::env::var(RESUMABLE_ENV).ok().as_deref(),
    )
}

/// One tool of a Control-authorized Capability Core view, as the view signs and writes it.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ViewTool {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters_json: String,
    pub(crate) action_schema_hash: String,
    pub(crate) requires_approval: bool,
    #[serde(default)]
    pub(crate) level: Option<String>,
    #[serde(default)]
    pub(crate) autonomy_rung: Option<String>,
}

/// Admits the rows of one view. Any row outside the registry, a repeated row, or a field that
/// disagrees with its golden row refuses the whole view. The v1 view (`view_v2 == false`) is the
/// `tickets.create` view and keeps only that row. The v2 view is the Space view and keeps only Space
/// rows, so a room-scoped self-API token never reaches the ticket path and the ticket view never
/// reaches the Space path.
pub(crate) fn admit_view(
    run_id: &str,
    view_run_id: &str,
    actions: &[ViewTool],
    view_v2: bool,
) -> Result<Vec<ToolDefinition>, Status> {
    if view_run_id != run_id || actions.len() > MAX_VIEW_ACTIONS {
        return Err(Status::permission_denied(
            "model action view response is not run-bound",
        ));
    }
    let registry = golden().map_err(|detail| Status::internal(detail))?;
    let invalid = || {
        Status::permission_denied("model action view returned an invalid tool definition")
    };
    let mut seen = BTreeSet::new();
    let mut offered = Vec::new();
    for tool in actions {
        let Some(row) = registry.row(&tool.name) else {
            return Err(Status::permission_denied(
                "model action view names an action outside the registry",
            ));
        };
        if !seen.insert(row.action_id.as_str()) {
            return Err(Status::permission_denied(
                "model action view repeats an action",
            ));
        }
        if tool.description.trim().is_empty()
            || tool.parameters_json != row.parameters_json
            || tool.action_schema_hash != row.schema_sha256
            || tool.requires_approval != (row.approval == "owner_approval")
        {
            return Err(invalid());
        }
        if let Some(rung) = tool.autonomy_rung.as_deref().filter(|rung| !rung.is_empty()) {
            if rung != row.autonomy_rung.as_str() {
                return Err(invalid());
            }
        }
        // Every layer can only narrow. The level Control signed and the golden default both have
        // to allow the row, so either one saying `ask` makes it an ask.
        let signed_ask = match tool.level.as_deref() {
            Some("allow") => false,
            Some("ask") => true,
            Some(_) => return Err(invalid()),
            None if view_v2 => return Err(invalid()),
            None => false,
        };
        let asks = signed_ask || row.default_level == "ask";
        if row.action_id == LEGACY_ACTION_ID {
            // `tickets.create` reaches the model through its own view and durable approval path.
            if view_v2 {
                continue;
            }
        } else {
            if !view_v2 {
                continue;
            }
            if !built_action(&row.action_id) {
                tracing::info!(
                    action = %row.action_id,
                    "space action is registered but not executed by this build; not offered"
                );
                continue;
            }
            if asks && !resumable() {
                tracing::info!(
                    action = %row.action_id,
                    "space action at level ask is not offered without EXECUTION_CORE_SPACE_SELF_API_RESUMABLE"
                );
                continue;
            }
        }
        offered.push(ToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters_json: tool.parameters_json.clone(),
        });
    }
    Ok(offered)
}

#[derive(Debug, Serialize)]
struct ControlViewRequest<'a> {
    run_id: &'a str,
    org_id: &'a str,
    view_version: u8,
}

#[derive(Debug, Serialize)]
struct RunViewRequest<'a> {
    run_id: &'a str,
    control_view_token: &'a str,
}

#[derive(Debug, Serialize)]
struct ControlDecisionRequest<'a> {
    run_id: &'a str,
    org_id: &'a str,
    action_id: &'a str,
    action_schema_hash: &'a str,
    payload_digest: &'a str,
    idempotency_key: &'a str,
}

#[derive(Debug, Deserialize)]
struct ControlEnvelope {
    data: ControlToken,
}

#[derive(Debug, Deserialize)]
struct ControlToken {
    token: String,
}

// Capability Core's run view. `reason_code` is content-free and is logged when the view is empty.
#[derive(Debug, Deserialize)]
struct ViewEnvelope {
    data: ViewData,
}

#[derive(Debug, Deserialize)]
struct ViewData {
    run_id: String,
    #[serde(default)]
    actions: Vec<ViewTool>,
    #[serde(default)]
    reason_code: Option<String>,
}

/// The Control and Capability Core clients of the Space view, and the Control decision client of the
/// effect. Built once per process from the environment when the switch is on.
pub(crate) struct SpaceSelfApiClient {
    control_base_url: Url,
    decision_token: String,
    view_token: String,
    capability_core_url: Url,
    http: Client,
}

static SHARED: OnceLock<Result<Option<SpaceSelfApiClient>, String>> = OnceLock::new();

/// The process-wide client: `Ok(None)` while the switch is off, `Err` when the switch is on and the
/// configuration is incomplete (a partial configuration is an error, never a silent disable).
pub(crate) fn shared() -> Result<Option<&'static SpaceSelfApiClient>, String> {
    match SHARED.get_or_init(SpaceSelfApiClient::from_env).as_ref() {
        Ok(client) => Ok(client.as_ref()),
        Err(detail) => Err(detail.clone()),
    }
}

impl SpaceSelfApiClient {
    fn from_env() -> Result<Option<Self>, String> {
        if !enabled() {
            return Ok(None);
        }
        let control = env_value("CONTROL_PLANE_USER_CORE_URL");
        let decision_token = env_value("EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN");
        let view_token = env_value("EXECUTION_CORE_CONTROL_MODEL_ACTION_VIEW_TOKEN");
        let capability = env_value("CAPABILITY_CORE_HTTP_URL");
        let (Some(control), Some(decision_token), Some(view_token), Some(capability)) =
            (control, decision_token, view_token, capability)
        else {
            return Err(format!(
                "{SWITCH_ENV} requires CONTROL_PLANE_USER_CORE_URL, EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN, EXECUTION_CORE_CONTROL_MODEL_ACTION_VIEW_TOKEN and CAPABILITY_CORE_HTTP_URL together"
            ));
        };
        let allow_insecure = env_flag("EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK");
        let control_base_url = service_base_url(&control, "Control Plane", allow_insecure)?;
        let decision_token = bounded_secret(&decision_token, "Control run-action service token")?;
        let view_token = bounded_secret(&view_token, "Control model-action-view service token")?;
        let capability_core_url = capability_core_url(&capability)?;
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(SESSION_CONNECT_TIMEOUT)
            .timeout(CONTROL_TIMEOUT);
        // The same private-CA bundle the ticket lane reads: additive, and a bad bundle is an error.
        if let Some(path) = env_value("EXECUTION_CORE_TICKET_CA_BUNDLE") {
            let pem = std::fs::read(&path)
                .map_err(|error| format!("Space self-API CA bundle {path} is unreadable: {error}"))?;
            let anchors = reqwest::Certificate::from_pem_bundle(&pem)
                .map_err(|_| format!("Space self-API CA bundle {path} is not valid PEM"))?;
            if anchors.is_empty() {
                return Err(format!("Space self-API CA bundle {path} contains no certificate"));
            }
            for anchor in anchors {
                builder = builder.add_root_certificate(anchor);
            }
        }
        let http = builder
            .build()
            .map_err(|_| "Space self-API HTTP client is unavailable".to_owned())?;
        Ok(Some(Self {
            control_base_url,
            decision_token,
            view_token,
            capability_core_url,
            http,
        }))
    }

    /// The v2 view token Control signs for this run. `Ok(None)` when Control refuses to list a row:
    /// an empty `CONTROL_SPACE_SELF_API_ACTIONS` answers 403, which is "no Space tools", not a failure.
    async fn request_view(&self, run_id: &str, org_id: &str) -> Result<Option<String>, Status> {
        let url = endpoint(&self.control_base_url, CONTROL_VIEW_PATH)
            .map_err(|_| Status::internal("Space action view endpoint is invalid"))?;
        let response = self
            .http
            .post(url)
            .header("x-service-id", "execution-core")
            .header("x-service-token", &self.view_token)
            .json(&ControlViewRequest {
                run_id,
                org_id,
                view_version: 2,
            })
            .send()
            .await
            .map_err(|_| Status::unavailable("Control Space action view is unavailable"))?;
        if response.status() == StatusCode::FORBIDDEN {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Status::unavailable("Control Space action view is unavailable"));
        }
        let envelope = response
            .json::<ControlEnvelope>()
            .await
            .map_err(|_| Status::permission_denied("Control returned an invalid Space action view"))?;
        let token = envelope.data.token.trim();
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Err(Status::permission_denied(
                "Control returned an invalid Space action view",
            ));
        }
        Ok(Some(token.to_owned()))
    }

    /// The server-resolved Space definitions for one run: Control's v2 token, then Capability Core's
    /// run view, admitted row by row against the golden registry.
    pub(crate) async fn resolve(
        &self,
        tokens: &ServiceTokenProvider,
        run_id: &str,
        org_id: &str,
    ) -> Result<Vec<ToolDefinition>, Status> {
        if run_id.trim().is_empty() || org_id.trim().is_empty() {
            return Err(Status::invalid_argument(
                "run and tenant are required for Space actions",
            ));
        }
        let Some(control_view_token) = self.request_view(run_id, org_id).await? else {
            tracing::info!(
                run_id,
                "Control lists no Space action for this run; no Space tool is offered"
            );
            return Ok(Vec::new());
        };
        let bearer = tokens
            .token_with_scopes(
                org_id,
                &[VIEW_SCOPE],
                "resolve a Control-authorized run-bound Space action view",
            )
            .await
            .map_err(|_| Status::unavailable("Space action view credential unavailable"))?;
        let endpoint = self
            .capability_core_url
            .join(CAPABILITY_RUN_VIEW_PATH)
            .map_err(|_| Status::internal("Space action view endpoint is invalid"))?;
        let response = self
            .http
            .post(endpoint)
            .bearer_auth(bearer)
            .json(&RunViewRequest {
                run_id,
                control_view_token: &control_view_token,
            })
            .send()
            .await
            .map_err(|_| Status::unavailable("Space action view unavailable"))?;
        if response.status() == StatusCode::FORBIDDEN {
            tracing::info!(
                run_id,
                "Capability Core lists no Space action for this run; no Space tool is offered"
            );
            return Ok(Vec::new());
        }
        if !response.status().is_success() {
            return Err(Status::permission_denied("Space action view denied"));
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| Status::unavailable("Space action view unavailable"))?;
        if body.len() > MAX_VIEW_BYTES {
            return Err(Status::permission_denied(
                "Space action view response is oversized",
            ));
        }
        let envelope: ViewEnvelope = serde_json::from_slice(&body)
            .map_err(|_| Status::permission_denied("Space action view response is invalid"))?;
        if envelope.data.actions.is_empty() {
            tracing::info!(
                run_id,
                reason_code = envelope.data.reason_code.as_deref().unwrap_or("none"),
                "Space action view is empty; no Space tool is offered"
            );
        }
        admit_view(run_id, &envelope.data.run_id, &envelope.data.actions, true)
    }

    /// Control's `run-action-v1` decision for one call, bound to the payload digest and key.
    async fn request_decision(
        &self,
        run_id: &str,
        org_id: &str,
        action_id: &str,
        action_schema_hash: &str,
        payload_digest: &str,
        idempotency_key: &str,
    ) -> Result<String, String> {
        let url = endpoint(&self.control_base_url, CONTROL_DECISION_PATH)?;
        let response = self
            .http
            .post(url)
            .header("x-service-id", "execution-core")
            .header("x-service-token", &self.decision_token)
            .json(&ControlDecisionRequest {
                run_id,
                org_id,
                action_id,
                action_schema_hash,
                payload_digest,
                idempotency_key,
            })
            .send()
            .await
            .map_err(|_| CODE_DECISION_UNAVAILABLE.to_owned())?;
        if response.status().is_client_error() {
            return Err(CODE_NOT_AUTHORIZED.to_owned());
        }
        if !response.status().is_success() {
            return Err(CODE_DECISION_UNAVAILABLE.to_owned());
        }
        let envelope = response
            .json::<ControlEnvelope>()
            .await
            .map_err(|_| CODE_DECISION_UNAVAILABLE.to_owned())?;
        let token = envelope.data.token.trim();
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Err(CODE_DECISION_UNAVAILABLE.to_owned());
        }
        Ok(token.to_owned())
    }
}

/// The Capability Core base URL, validated as the capability lane validates it.
fn capability_core_url(raw: &str) -> Result<Url, String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let url = Url::parse(trimmed)
        .map_err(|_| "CAPABILITY_CORE_HTTP_URL is invalid".to_owned())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "CAPABILITY_CORE_HTTP_URL must be an absolute http(s) service URL without credentials, query, or fragment"
                .to_owned(),
        );
    }
    Ok(url)
}

/// The arguments of one call, typed per action. Each struct refuses unknown fields, so the model
/// cannot name a thread, a Space or any other field the registry does not declare.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Arguments {
    Rename {
        title: String,
    },
    Pin {
        pinned: bool,
    },
    // Built for WS18-S7: the row is not in BUILT_ACTIONS, so no call reaches this arm in this build.
    PostCard {
        title: String,
        body: String,
        tone: String,
        fields: Vec<(String, String)>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameInput {
    title: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PinInput {
    pinned: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PostCardInput {
    title: String,
    body: String,
    tone: String,
    // An absent list is the empty list: it flattens to `fields.len` 0, as Session Core does.
    #[serde(default)]
    fields: Vec<CardFieldInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CardFieldInput {
    label: String,
    value: String,
}

fn parse_arguments(action_id: &str, input: &str) -> Result<Arguments, String> {
    match action_id {
        RENAME_ACTION => {
            let parsed: RenameInput =
                serde_json::from_str(input).map_err(|_| CODE_INVALID_ARGUMENTS.to_owned())?;
            // The bound Session Core checks on the untrimmed value it digests: 1 to 96 characters.
            let characters = parsed.title.chars().count();
            if !(1..=96).contains(&characters) {
                return Err(CODE_INVALID_ARGUMENTS.to_owned());
            }
            Ok(Arguments::Rename {
                title: parsed.title,
            })
        }
        PIN_ACTION => {
            let parsed: PinInput =
                serde_json::from_str(input).map_err(|_| CODE_INVALID_ARGUMENTS.to_owned())?;
            Ok(Arguments::Pin {
                pinned: parsed.pinned,
            })
        }
        POST_CARD_ACTION => {
            let parsed: PostCardInput =
                serde_json::from_str(input).map_err(|_| CODE_INVALID_ARGUMENTS.to_owned())?;
            // The same bounds Session Core applies to the untrimmed values it digests.
            let within = |value: &str, min: usize, max: usize| {
                let count = value.chars().count();
                (min..=max).contains(&count)
            };
            if !within(parsed.title.as_str(), 1, 120)
                || !within(parsed.body.as_str(), 1, 2000)
                || !CARD_TONES.contains(&parsed.tone.as_str())
                || parsed.fields.len() > 8
            {
                return Err(CODE_INVALID_ARGUMENTS.to_owned());
            }
            let mut fields = Vec::with_capacity(parsed.fields.len());
            for field in parsed.fields {
                if !within(field.label.as_str(), 1, 40) || !within(field.value.as_str(), 1, 200) {
                    return Err(CODE_INVALID_ARGUMENTS.to_owned());
                }
                fields.push((field.label, field.value));
            }
            Ok(Arguments::PostCard {
                title: parsed.title,
                body: parsed.body,
                tone: parsed.tone,
                fields,
            })
        }
        _ => Err(CODE_NOT_OFFERED.to_owned()),
    }
}

/// The argument pairs in the golden `argument_order`, exactly as `space/self_api.rs` flattens them.
fn argument_pairs(arguments: &Arguments) -> Vec<(String, String)> {
    match arguments {
        Arguments::Rename { title } => vec![("title".to_owned(), title.clone())],
        Arguments::Pin { pinned } => vec![("pinned".to_owned(), pinned.to_string())],
        Arguments::PostCard {
            title,
            body,
            tone,
            fields,
        } => {
            let mut pairs = vec![
                ("title".to_owned(), title.clone()),
                ("body".to_owned(), body.clone()),
                ("tone".to_owned(), tone.clone()),
                ("fields.len".to_owned(), fields.len().to_string()),
            ];
            for (index, (label, value)) in fields.iter().enumerate() {
                pairs.push((format!("fields.{index}.label"), label.clone()));
                pairs.push((format!("fields.{index}.value"), value.clone()));
            }
            pairs
        }
    }
}

fn payload_digest(
    action_id: &str,
    run_id: &str,
    org_id: &str,
    idempotency_key: &str,
    arguments: &Arguments,
) -> Result<String, String> {
    let pairs = argument_pairs(arguments);
    let borrowed: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    sa::payload_digest(action_id, run_id, org_id, idempotency_key, &borrowed)
        .map_err(|_| CODE_INVALID_ARGUMENTS.to_owned())
}

fn proto_args(arguments: &Arguments) -> ActionArgs {
    match arguments {
        Arguments::Rename { title } => ActionArgs::Rename(RenameThreadArgs {
            title: title.clone(),
        }),
        Arguments::Pin { pinned } => ActionArgs::Pin(PinThreadArgs { pinned: *pinned }),
        Arguments::PostCard {
            title,
            body,
            tone,
            fields,
        } => ActionArgs::PostCard(PostCardArgs {
            title: title.clone(),
            body: body.clone(),
            tone: tone.clone(),
            fields: fields
                .iter()
                .map(|(label, value)| CardField {
                    label: label.clone(),
                    value: value.clone(),
                })
                .collect(),
        }),
    }
}

pub(crate) fn bounded_identifier(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
        return Err(CODE_NOT_OFFERED.to_owned());
    }
    Ok(value.to_owned())
}

fn is_transport(status: &Status) -> bool {
    matches!(
        status.code(),
        Code::Unavailable | Code::DeadlineExceeded | Code::Unknown | Code::Cancelled
    )
}

/// The model-facing code for a Session Core refusal.
fn map_session_status(status: &Status) -> String {
    if let Some(refusal) = Refusal::from_status(status) {
        return refusal.code().to_owned();
    }
    if let Some(word) = status
        .metadata()
        .get(SESSION_ERROR_TRAILER)
        .and_then(|value| value.to_str().ok())
    {
        if SESSION_WORDS.contains(&word) {
            return word.to_owned();
        }
    }
    let code: &str = match status.code() {
        Code::InvalidArgument => CODE_INVALID_ARGUMENTS,
        Code::PermissionDenied | Code::Unauthenticated | Code::NotFound => CODE_NOT_AUTHORIZED,
        Code::Unimplemented | Code::FailedPrecondition => CODE_NOT_OFFERED,
        _ => CODE_UNKNOWN_OUTCOME,
    };
    code.to_owned()
}

async fn apply_once(
    channel: &Channel,
    bearer: &str,
    request: &ApplyRunThreadActionRequest,
) -> Result<ApplyRunThreadActionResponse, Status> {
    let authorization: MetadataValue<Ascii> =
        MetadataValue::try_from(format!("Bearer {bearer}"))
            .map_err(|_| Status::unauthenticated("delegated run credential is malformed"))?;
    let mut message = Request::new(request.clone());
    message.set_timeout(SESSION_TIMEOUT);
    message.metadata_mut().insert("authorization", authorization);
    SpaceRunActionsClient::new(channel.clone())
        .apply_run_thread_action(message)
        .await
        .map(tonic::Response::into_inner)
}

/// Applies one call. A transport failure is retried once with a fresh decision under the same key and
/// digest: Session Core checks the decision's expiry before it looks up the receipt, so the first token
/// cannot be reused. A second transport failure is `unknown_outcome`.
async fn apply_with_replay(
    client: &SpaceSelfApiClient,
    channel: &Channel,
    bearer: &str,
    decision_inputs: &DecisionInputs<'_>,
    arguments: &Arguments,
) -> Result<ApplyRunThreadActionResponse, String> {
    let mut attempt = 0_u8;
    loop {
        let token = client
            .request_decision(
                decision_inputs.run_id,
                decision_inputs.org_id,
                decision_inputs.action_id,
                decision_inputs.schema_sha256,
                decision_inputs.digest,
                decision_inputs.idempotency_key,
            )
            .await?;
        let request = ApplyRunThreadActionRequest {
            org_id: decision_inputs.org_id.to_owned(),
            run_id: decision_inputs.run_id.to_owned(),
            action_id: decision_inputs.action_id.to_owned(),
            idempotency_key: decision_inputs.idempotency_key.to_owned(),
            control_decision_token: token,
            args: Some(proto_args(arguments)),
        };
        match apply_once(channel, bearer, &request).await {
            Ok(response) => return Ok(response),
            Err(status) if is_transport(&status) && attempt == 0 => attempt += 1,
            Err(status) if is_transport(&status) => return Err(CODE_UNKNOWN_OUTCOME.to_owned()),
            Err(status) => return Err(map_session_status(&status)),
        }
    }
}

/// The values one decision and one apply are bound to.
struct DecisionInputs<'a> {
    run_id: &'a str,
    org_id: &'a str,
    action_id: &'a str,
    schema_sha256: &'a str,
    digest: &'a str,
    idempotency_key: &'a str,
}

async fn execute(
    tool_name: &str,
    tool_input: &str,
    run_id: &str,
    org_id: &str,
    step_id: &str,
    retention_bound: bool,
    session_channel: Option<Channel>,
    session_bearer: Option<&str>,
) -> Result<String, String> {
    if !is_space_action(tool_name) || !BUILT_ACTIONS.contains(&tool_name) {
        return Err(CODE_NOT_OFFERED.to_owned());
    }
    // A zero-retention run is refused before any Control call (design 3.12).
    if retention_bound {
        return Err(CODE_ZDR.to_owned());
    }
    let client = match shared() {
        Ok(Some(client)) => client,
        Ok(None) => return Err(CODE_NOT_OFFERED.to_owned()),
        Err(_) => return Err(CODE_DECISION_UNAVAILABLE.to_owned()),
    };
    let arguments = parse_arguments(tool_name, tool_input)?;
    let run_id = bounded_identifier(run_id)?;
    let org_id = bounded_identifier(org_id)?;
    let step_id = bounded_identifier(step_id)?;
    let row = registry_row(tool_name).ok_or_else(|| CODE_NOT_OFFERED.to_owned())?;
    // Refuse before asking Control anything when the effect could not be applied anyway.
    let channel = session_channel.ok_or_else(|| CODE_NOT_OFFERED.to_owned())?;
    let bearer = session_bearer.ok_or_else(|| CODE_NOT_AUTHORIZED.to_owned())?;
    let idempotency_key = sa::idempotency_key(tool_name, &run_id, &step_id);
    let digest = payload_digest(tool_name, &run_id, &org_id, &idempotency_key, &arguments)?;
    let inputs = DecisionInputs {
        run_id: &run_id,
        org_id: &org_id,
        action_id: tool_name,
        schema_sha256: &row.schema_sha256,
        digest: &digest,
        idempotency_key: &idempotency_key,
    };
    let response = apply_with_replay(client, &channel, bearer, &inputs, &arguments).await?;
    let status = match response.outcome {
        OUTCOME_APPLIED => "applied",
        OUTCOME_REPLAYED => "replayed",
        OUTCOME_NOOP => "noop",
        _ => return Err(CODE_UNKNOWN_OUTCOME.to_owned()),
    };
    if response.receipt_id.trim().is_empty() {
        return Err(CODE_UNKNOWN_OUTCOME.to_owned());
    }
    serde_json::to_string(&serde_json::json!({
        "status": status,
        "receipt_id": response.receipt_id,
        "thread_id": response.thread_id,
    }))
    .map_err(|_| CODE_UNKNOWN_OUTCOME.to_owned())
}

/// Executes one Space action call. Model-facing availability is decided by the signed view, not by
/// this local predicate; this function refuses anything the view did not offer.
pub(crate) async fn execute_if_configured(
    tool_name: &str,
    tool_input: &str,
    run_id: &str,
    org_id: &str,
    step_id: &str,
    retention_bound: bool,
    session_channel: Option<Channel>,
    session_bearer: Option<&str>,
) -> ToolExecution {
    match execute(
        tool_name,
        tool_input,
        run_id,
        org_id,
        step_id,
        retention_bound,
        session_channel,
        session_bearer,
    )
    .await
    {
        Ok(output) => ToolExecution {
            output,
            error: None,
        },
        Err(code) => ToolExecution {
            output: String::new(),
            error: Some(code),
        },
    }
}

/// The Session Core probe of the owner path (`ProbeRunThreadActions`). Its credential is the
/// execution-core service token for `session:run-thread-action:probe`, and it never carries content.
pub(crate) struct SessionProbe {
    channel: Channel,
    tokens: SessionTerminalTokenProvider,
}

impl SessionProbe {
    /// `None` while the switch is off, or when Session Core's address or credential is not configured.
    pub(crate) fn from_env() -> Option<Self> {
        if !enabled() {
            return None;
        }
        let url = std::env::var("SESSION_CORE_URL")
            .or_else(|_| std::env::var("SESSION_CORE_ADDR"))
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let channel = match tonic::transport::Endpoint::from_shared(url) {
            Ok(endpoint) => endpoint
                .connect_timeout(SESSION_CONNECT_TIMEOUT)
                .connect_lazy(),
            Err(error) => {
                tracing::warn!(%error, "Session Core address is invalid; the Space owner path is not probed");
                return None;
            }
        };
        let tokens = match SessionTerminalTokenProvider::from_env() {
            Ok(tokens) => tokens,
            Err(error) => {
                tracing::warn!(%error, "Session Core probe credential is not configured; the Space owner path is not probed");
                return None;
            }
        };
        Some(Self { channel, tokens })
    }

    /// The probe's three booleans, or `None` when the call did not complete.
    pub(crate) async fn report(&self) -> Option<ProbeRunThreadActionsResponse> {
        let token = match self.tokens.run_thread_action_probe_token().await {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!(%error, "Session Core probe token is unavailable");
                return None;
            }
        };
        let authorization: MetadataValue<Ascii> =
            MetadataValue::try_from(format!("Bearer {token}")).ok()?;
        let mut message = Request::new(ProbeRunThreadActionsRequest {});
        message.set_timeout(PROBE_TIMEOUT);
        message.metadata_mut().insert("authorization", authorization);
        match SpaceRunActionsClient::new(self.channel.clone())
            .probe_run_thread_actions(message)
            .await
        {
            Ok(response) => Some(response.into_inner()),
            Err(status) => {
                tracing::warn!(code = ?status.code(), "Space run-action probe did not complete");
                None
            }
        }
    }
}

impl SpaceSelfApiClient {
    /// The server-resolved Space definitions for a run, for the `space_self_api` profile.
    pub(crate) async fn resolve_profile(
        tokens: &ServiceTokenProvider,
        run_id: &str,
        org_id: &str,
    ) -> Result<Vec<ToolDefinition>, Status> {
        match shared() {
            Ok(Some(client)) => client.resolve(tokens, run_id, org_id).await,
            Ok(None) => Ok(Vec::new()),
            Err(_) => Err(Status::unavailable(
                "Space self-API configuration is unavailable",
            )),
        }
    }
}

