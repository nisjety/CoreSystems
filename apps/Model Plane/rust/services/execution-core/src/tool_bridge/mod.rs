//! Tool execution bridge.

use mp_contracts::model_plane::v1::ValidateGrantResponse;

use crate::{browser_agent, wiki_agent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecution {
    pub output: String,
    pub error: Option<String>,
}

/// Structured terminal result of the live browser-agent loop.
///
/// Browser work is not a generic text tool: cancelling a run, denying its
/// approval, or timing out an approval must retain that lifecycle meaning
/// through the runtime and gRPC response. Keeping this separate from
/// [`ToolExecution`] prevents the generic `error: None => completed` fallback
/// from manufacturing a successful browser result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BrowserAgentExecution {
    Completed { output: String },
    Failed { reason: String },
    PermissionDenied { reason: String },
    TimedOut { reason: String },
    ResourceExhausted { reason: String },
    Cancelled { reason: String },
    Aborted { reason: String },
    AwaitingApproval,
}

/// Browser authority resolved by BrowserBroker against a separately verified
/// `aud=browser-broker` credential. The raw tool JSON never becomes authority:
/// it may name a grant for lookup, but only this object can reach Quarry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedBrowserGrant {
    grant_id: String,
    allowed_domains: Vec<String>,
    // What the broker said about the grant itself. Read only to decide whether a
    // derived run grant stands in for a high-risk `ask`
    // (`runtime_loop::browser_grant_approval`); never authority on their own.
    kind: String,
    parent_grant_id: String,
    owner_id: String,
    zdr: bool,
    // The grant's budget claim (WS6.E); zero means no bound on that dimension.
    // It can only tighten the run (see `PlanCeilings`).
    budget_max_steps: i32,
    budget_max_runtime_s: i32,
}

impl ValidatedBrowserGrant {
    /// Construct a dispatch capability from a BrowserBroker response. Bind the
    /// echoed id and require the response policy to already be canonical so a
    /// malformed broker/proxy response cannot weaken a browser run.
    pub(crate) fn from_broker_response(
        requested_grant_id: &str,
        response: &ValidateGrantResponse,
    ) -> Result<Self, String> {
        let requested_grant_id = requested_grant_id.trim();
        if requested_grant_id.is_empty()
            || !response.active
            || response.grant_id.trim() != requested_grant_id
        {
            return Err(
                "browser grant is not active or does not match the requested grant".to_owned(),
            );
        }
        let canonical = browser_agent::canonicalize_allowed_domains(&response.allowed_domains)?;
        if canonical != response.allowed_domains {
            return Err("browser grant domain policy is not canonical".to_owned());
        }
        Ok(Self {
            grant_id: requested_grant_id.to_owned(),
            allowed_domains: canonical,
            kind: response.kind.clone(),
            parent_grant_id: response.parent_grant_id.clone(),
            owner_id: response.owner_id.clone(),
            zdr: response.zdr,
            budget_max_steps: response.budget_max_steps,
            budget_max_runtime_s: response.budget_max_runtime_s,
        })
    }

    /// A grant as the broker would have validated it, for tests that need one
    /// without a broker.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_test(
        grant_id: &str,
        kind: &str,
        parent_grant_id: &str,
        owner_id: &str,
        zdr: bool,
    ) -> Self {
        Self {
            grant_id: grant_id.to_owned(),
            allowed_domains: vec!["example.com".to_owned()],
            kind: kind.to_owned(),
            parent_grant_id: parent_grant_id.to_owned(),
            owner_id: owner_id.to_owned(),
            zdr,
            budget_max_steps: 0,
            budget_max_runtime_s: 0,
        }
    }

    #[must_use]
    pub(crate) fn grant_id(&self) -> &str {
        &self.grant_id
    }

    #[must_use]
    pub(crate) fn allowed_domains(&self) -> &[String] {
        &self.allowed_domains
    }

    /// `session`, `run` or `sensitive_approval`, as the broker reported it.
    #[must_use]
    pub(crate) fn kind(&self) -> &str {
        &self.kind
    }

    /// The session grant a derived run grant descends from; empty otherwise.
    #[must_use]
    pub(crate) fn parent_grant_id(&self) -> &str {
        &self.parent_grant_id
    }

    /// The user (or service actor) the broker issued the grant to.
    #[must_use]
    pub(crate) fn owner_id(&self) -> &str {
        &self.owner_id
    }

    /// Whether the broker holds the grant under Zero Data Retention.
    #[must_use]
    pub(crate) fn zdr(&self) -> bool {
        self.zdr
    }
}

/// Execute a tool by name.
///
/// This is intentionally deterministic while execution-core is being bootstrapped.
pub fn execute(tool_name: &str, tool_input: &str) -> ToolExecution {
    if tool_name.is_empty() {
        return ToolExecution {
            output: "reasoning_step_completed".to_owned(),
            error: None,
        };
    }

    match tool_name {
        "echo" => ToolExecution {
            output: tool_input.to_owned(),
            error: None,
        },
        "fail" => ToolExecution {
            output: String::new(),
            error: Some("tool execution failed".to_owned()),
        },
        // `browser_agent` is async (drives the live Quarry loop); it is
        // dispatched from the async `runtime_loop::execute_step`, not here.
        "wiki_propose_edit" => execute_wiki_propose_edit(tool_input),
        "wiki_lint" => execute_wiki_lint(tool_input),
        // A tool that reached the fallback has no executor (e.g. a client-
        // declared tool the model elected, or a typo'd name). Return an honest
        // error so the ReAct loop can adapt — NEVER a fabricated success echo,
        // which made the model believe an un-run tool had succeeded.
        _ => ToolExecution {
            output: String::new(),
            error: Some(format!(
                "tool '{tool_name}' is not implemented by execution-core"
            )),
        },
    }
}

fn execute_wiki_propose_edit(tool_input: &str) -> ToolExecution {
    let parsed: Result<wiki_agent::ProposeEditInput, _> = serde_json::from_str(tool_input);
    match parsed {
        Ok(input) => match wiki_agent::propose_edit(input) {
            Ok(env) => match serde_json::to_string(&env) {
                Ok(out) => ToolExecution {
                    output: out,
                    error: None,
                },
                Err(e) => ToolExecution {
                    output: String::new(),
                    error: Some(format!("serialize proposal: {e}")),
                },
            },
            Err(e) => ToolExecution {
                output: String::new(),
                error: Some(e),
            },
        },
        Err(e) => ToolExecution {
            output: String::new(),
            error: Some(format!("invalid wiki_propose_edit input: {e}")),
        },
    }
}

fn execute_wiki_lint(tool_input: &str) -> ToolExecution {
    let parsed: Result<wiki_agent::LintInput, _> = serde_json::from_str(tool_input);
    match parsed {
        Ok(input) => {
            let report = wiki_agent::lint(input);
            match serde_json::to_string(&report) {
                Ok(out) => ToolExecution {
                    output: out,
                    error: None,
                },
                Err(e) => ToolExecution {
                    output: String::new(),
                    error: Some(format!("serialize lint report: {e}")),
                },
            }
        }
        Err(e) => ToolExecution {
            output: String::new(),
            error: Some(format!("invalid wiki_lint input: {e}")),
        },
    }
}

pub(crate) async fn execute_browser_agent(
    tool_input: &str,
    verified_org_id: &str,
    verified_zdr: bool,
    sink: Option<&dyn browser_agent::BrowserEventSink>,
    state: Option<&crate::state::StateStore>,
    inference_bearer: Option<&str>,
    validated_grant: Option<&ValidatedBrowserGrant>,
) -> BrowserAgentExecution {
    if verified_org_id.trim().is_empty() {
        return BrowserAgentExecution::Failed {
            reason: "browser_agent requires the verified run organization".to_owned(),
        };
    }
    let Some(validated_grant) = validated_grant else {
        return BrowserAgentExecution::Failed {
            reason: "browser_agent requires a broker-validated grant".to_owned(),
        };
    };
    let config: Result<BrowserAgentInput, _> = serde_json::from_str(tool_input);
    match config {
        Ok(input) => {
            let plan_config =
                match browser_plan_config(input, verified_org_id, verified_zdr, validated_grant) {
                    Ok(config) => config,
                    Err(error) => {
                        return BrowserAgentExecution::Failed { reason: error };
                    }
                };
            // Real Quarry agent client from env (`QUARRY_BROWSER_AGENT_ENABLED`
            // + `QUARRY_EDGE_URL`); `None` → the loop fails fast.
            let client = match crate::quarry_agent::QuarryAgentClient::from_env() {
                Ok(client) => client,
                Err(error) => {
                    return BrowserAgentExecution::Failed {
                        reason: format!("browser_agent unavailable: {error}"),
                    };
                }
            };
            // The planner policy (deployment mode plus the org's flag) decides;
            // a selected planner that cannot be built fails the run with a
            // reason, it never degrades to the deterministic loop.
            let selection = select_planner(
                &crate::planner_policy::PlannerPolicy::from_env(),
                &plan_config.org_id,
                plan_config.zdr,
                plan_config.planner_flag.as_deref(),
                inference_bearer,
            );
            let (planner, deterministic_reason) = match selection {
                PlannerSelection::Llm(planner) => (Some(planner), None),
                PlannerSelection::Deterministic(reason) => {
                    tracing::info!(reason = reason.as_str(), "browser run uses the deterministic loop");
                    // The default deployment (`mode_off`) keeps the output it
                    // always had; every other reason is worth saying.
                    (
                        None,
                        (reason != crate::planner_policy::DeterministicReason::ModeOff)
                            .then(|| reason.as_str()),
                    )
                }
                PlannerSelection::Unavailable(reason) => {
                    tracing::warn!("browser planner selected but unavailable");
                    return BrowserAgentExecution::Failed { reason };
                }
            };
            // Captured before the loop consumes the config — the postcondition
            // judgment below needs all three, and the loop result carries
            // none of them.
            let verification_inputs = BrowserVerificationInputs {
                postcondition: plan_config.postcondition.clone(),
                stop_criteria: plan_config.stop_criteria.clone(),
                zdr: plan_config.zdr,
                deterministic_reason,
            };
            let result = browser_agent::run_browser_agent_loop(
                plan_config,
                client.as_ref(),
                planner
                    .as_ref()
                    .map(|planner| planner as &dyn crate::llm_planner::ActionPlanner),
                sink,
                state,
            )
            .await;
            browser_execution_from_loop_result(result, &verification_inputs)
        }
        Err(e) => BrowserAgentExecution::Failed {
            reason: format!("invalid browser_agent input: {e}"),
        },
    }
}

/// Execute one narrow, already-leased Quarry browser operation for the
/// Model-Plane MCP facade. This deliberately does *not* create a browser run,
/// accept a URL/selector/script, or expose a CDP channel: callers can only
/// observe an existing Quarry run or execute a current opaque snapshot target.
///
/// BrowserBroker remains the authority for the run grant. Sensitive transfer
/// and dialog operations retain their one-shot approval grant and Quarry
/// validates that scope before an effect reaches the driver.
pub(crate) async fn execute_quarry_browser_mcp(
    tool_name: &str,
    tool_input: &str,
    verified_org_id: &str,
    verified_zdr: bool,
    validated_grant: Option<&ValidatedBrowserGrant>,
) -> ToolExecution {
    if verified_org_id.trim().is_empty() {
        return browser_mcp_error("browser MCP requires the verified run organization");
    }
    let Some(validated_grant) = validated_grant else {
        return browser_mcp_error("browser MCP requires a broker-validated grant");
    };

    let request = match BrowserMcpRequest::parse(tool_name, tool_input, validated_grant) {
        Ok(request) => request,
        Err(error) => return browser_mcp_error(error),
    };
    let client = match crate::quarry_agent::QuarryAgentClient::from_env() {
        Ok(Some(client)) => client,
        Ok(None) => {
            return browser_mcp_error(
                "browser MCP unavailable: QUARRY_BROWSER_AGENT_ENABLED or QUARRY_EDGE_URL is not configured",
            );
        }
        Err(error) => return browser_mcp_error(format!("browser MCP unavailable: {error}")),
    };

    let action = request
        .action
        .unwrap_or(crate::quarry_agent::AgentAction::GetContent);
    let constraints = crate::quarry_agent::AgentConstraints {
        max_steps: 1,
        allowed_domains: validated_grant.allowed_domains().to_vec(),
        max_runtime_s: Some(30),
        max_cost_usd: None,
    };
    match client
        .step(crate::quarry_agent::StepParams::new(
            &request.quarry_run_id,
            &request.lease_id,
            verified_org_id,
            action,
            &constraints,
            verified_zdr,
            validated_grant.grant_id(),
        ))
        .await
    {
        Ok(observation) => match serde_json::to_string(&observation) {
            Ok(output) => ToolExecution {
                output,
                error: None,
            },
            Err(error) => browser_mcp_error(format!("serialize Quarry observation: {error}")),
        },
        Err(error) => browser_mcp_error(format!("Quarry browser operation failed: {error}")),
    }
}

fn browser_mcp_error(error: impl Into<String>) -> ToolExecution {
    ToolExecution {
        output: String::new(),
        error: Some(error.into()),
    }
}

const MAX_BROWSER_MCP_TEXT_BYTES: usize = 64 * 1024;
const MAX_BROWSER_MCP_WAIT_MS: u32 = 30_000;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserMcpObserveInput {
    grant_id: String,
    quarry_run_id: String,
    lease_id: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserMcpActInput {
    grant_id: String,
    quarry_run_id: String,
    lease_id: String,
    action: BrowserMcpAction,
}

/// The public browser-action grammar is intentionally smaller than Quarry's
/// internal action enum. Legacy CSS actions, navigation, screen coordinates,
/// arbitrary JavaScript, and host-file paths have no MCP representation.
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum BrowserMcpAction {
    ClickRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
    },
    FrameClickRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
    },
    TypeRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        text: String,
    },
    FrameTypeRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        text: String,
    },
    SelectRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        value: String,
    },
    FrameSelectRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        value: String,
    },
    WaitForRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        timeout_ms: u32,
    },
    FrameWaitForRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        timeout_ms: u32,
    },
    RespondDialog {
        dialog_id: String,
        accept: bool,
        approval_grant_id: String,
        prompt_text: Option<String>,
    },
    UploadRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        artifact_id: String,
        approval_grant_id: String,
    },
    FrameUploadRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        artifact_id: String,
        approval_grant_id: String,
    },
    DownloadRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        approval_grant_id: String,
    },
    FrameDownloadRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        approval_grant_id: String,
    },
}

struct BrowserMcpRequest {
    quarry_run_id: String,
    lease_id: String,
    action: Option<crate::quarry_agent::AgentAction>,
}

impl BrowserMcpRequest {
    fn parse(
        tool_name: &str,
        tool_input: &str,
        validated_grant: &ValidatedBrowserGrant,
    ) -> Result<Self, String> {
        match tool_name {
            "browser.observe" => {
                let input: BrowserMcpObserveInput = serde_json::from_str(tool_input)
                    .map_err(|error| format!("invalid browser.observe input: {error}"))?;
                validate_browser_mcp_scope(
                    &input.grant_id,
                    &input.quarry_run_id,
                    &input.lease_id,
                    validated_grant,
                )?;
                Ok(Self {
                    quarry_run_id: input.quarry_run_id,
                    lease_id: input.lease_id,
                    action: None,
                })
            }
            "browser.act" => {
                let input: BrowserMcpActInput = serde_json::from_str(tool_input)
                    .map_err(|error| format!("invalid browser.act input: {error}"))?;
                validate_browser_mcp_scope(
                    &input.grant_id,
                    &input.quarry_run_id,
                    &input.lease_id,
                    validated_grant,
                )?;
                Ok(Self {
                    quarry_run_id: input.quarry_run_id,
                    lease_id: input.lease_id,
                    action: Some(input.action.into_quarry_action()?),
                })
            }
            _ => Err("unsupported Quarry browser MCP tool".to_owned()),
        }
    }
}

fn validate_browser_mcp_scope(
    input_grant_id: &str,
    quarry_run_id: &str,
    lease_id: &str,
    validated_grant: &ValidatedBrowserGrant,
) -> Result<(), String> {
    if input_grant_id.trim() != validated_grant.grant_id() {
        return Err(
            "browser tool input grant does not match the broker-validated grant".to_owned(),
        );
    }
    if quarry_run_id.trim().is_empty() || lease_id.trim().is_empty() {
        return Err("browser MCP requires a non-empty Quarry run and lease".to_owned());
    }
    Ok(())
}

impl BrowserMcpAction {
    fn into_quarry_action(self) -> Result<crate::quarry_agent::AgentAction, String> {
        use crate::quarry_agent::AgentAction;

        match self {
            Self::ClickRef {
                snapshot_id,
                generation,
                ref_id,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                Ok(AgentAction::ClickRef {
                    snapshot_id,
                    generation,
                    ref_id,
                })
            }
            Self::FrameClickRef {
                snapshot_id,
                generation,
                frame_id,
                ref_id,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                require_non_empty("frame_id", &frame_id)?;
                Ok(AgentAction::FrameClickRef {
                    snapshot_id,
                    generation,
                    frame_id,
                    ref_id,
                })
            }
            Self::TypeRef {
                snapshot_id,
                generation,
                ref_id,
                text,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                validate_bounded_text("type_ref text", &text, false)?;
                Ok(AgentAction::TypeRef {
                    snapshot_id,
                    generation,
                    ref_id,
                    text,
                })
            }
            Self::FrameTypeRef {
                snapshot_id,
                generation,
                frame_id,
                ref_id,
                text,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                require_non_empty("frame_id", &frame_id)?;
                validate_bounded_text("frame_type_ref text", &text, false)?;
                Ok(AgentAction::FrameTypeRef {
                    snapshot_id,
                    generation,
                    frame_id,
                    ref_id,
                    text,
                })
            }
            Self::SelectRef {
                snapshot_id,
                generation,
                ref_id,
                value,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                validate_bounded_text("select_ref value", &value, true)?;
                Ok(AgentAction::SelectRef {
                    snapshot_id,
                    generation,
                    ref_id,
                    value,
                })
            }
            Self::FrameSelectRef {
                snapshot_id,
                generation,
                frame_id,
                ref_id,
                value,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                require_non_empty("frame_id", &frame_id)?;
                validate_bounded_text("frame_select_ref value", &value, true)?;
                Ok(AgentAction::FrameSelectRef {
                    snapshot_id,
                    generation,
                    frame_id,
                    ref_id,
                    value,
                })
            }
            Self::WaitForRef {
                snapshot_id,
                generation,
                ref_id,
                timeout_ms,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                if timeout_ms == 0 || timeout_ms > MAX_BROWSER_MCP_WAIT_MS {
                    return Err(format!(
                        "wait_for_ref timeout_ms must be between 1 and {MAX_BROWSER_MCP_WAIT_MS}"
                    ));
                }
                Ok(AgentAction::WaitForRef {
                    snapshot_id,
                    generation,
                    ref_id,
                    timeout_ms,
                })
            }
            Self::FrameWaitForRef {
                snapshot_id,
                generation,
                frame_id,
                ref_id,
                timeout_ms,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                require_non_empty("frame_id", &frame_id)?;
                if timeout_ms == 0 || timeout_ms > MAX_BROWSER_MCP_WAIT_MS {
                    return Err(format!(
                        "frame_wait_for_ref timeout_ms must be between 1 and {MAX_BROWSER_MCP_WAIT_MS}"
                    ));
                }
                Ok(AgentAction::FrameWaitForRef {
                    snapshot_id,
                    generation,
                    frame_id,
                    ref_id,
                    timeout_ms,
                })
            }
            Self::RespondDialog {
                dialog_id,
                accept,
                approval_grant_id,
                prompt_text,
            } => {
                require_non_empty("dialog_id", &dialog_id)?;
                require_non_empty("dialog approval_grant_id", &approval_grant_id)?;
                if let Some(prompt_text) = prompt_text.as_deref() {
                    validate_bounded_text("dialog prompt_text", prompt_text, true)?;
                }
                Ok(AgentAction::RespondDialog {
                    dialog_id,
                    accept,
                    approval_grant_id,
                    prompt_text,
                })
            }
            Self::UploadRef {
                snapshot_id,
                generation,
                ref_id,
                artifact_id,
                approval_grant_id,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                require_non_empty("artifact_id", &artifact_id)?;
                require_non_empty("upload approval_grant_id", &approval_grant_id)?;
                Ok(AgentAction::UploadRef {
                    snapshot_id,
                    generation,
                    ref_id,
                    artifact_id,
                    approval_grant_id,
                })
            }
            Self::FrameUploadRef {
                snapshot_id,
                generation,
                frame_id,
                ref_id,
                artifact_id,
                approval_grant_id,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                require_non_empty("frame_id", &frame_id)?;
                require_non_empty("artifact_id", &artifact_id)?;
                require_non_empty("upload approval_grant_id", &approval_grant_id)?;
                Ok(AgentAction::FrameUploadRef {
                    snapshot_id,
                    generation,
                    frame_id,
                    ref_id,
                    artifact_id,
                    approval_grant_id,
                })
            }
            Self::DownloadRef {
                snapshot_id,
                generation,
                ref_id,
                approval_grant_id,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                require_non_empty("download approval_grant_id", &approval_grant_id)?;
                Ok(AgentAction::DownloadRef {
                    snapshot_id,
                    generation,
                    ref_id,
                    approval_grant_id,
                })
            }
            Self::FrameDownloadRef {
                snapshot_id,
                generation,
                frame_id,
                ref_id,
                approval_grant_id,
            } => {
                validate_snapshot_target(&snapshot_id, &ref_id)?;
                require_non_empty("frame_id", &frame_id)?;
                require_non_empty("download approval_grant_id", &approval_grant_id)?;
                Ok(AgentAction::FrameDownloadRef {
                    snapshot_id,
                    generation,
                    frame_id,
                    ref_id,
                    approval_grant_id,
                })
            }
        }
    }
}

fn validate_snapshot_target(snapshot_id: &str, ref_id: &str) -> Result<(), String> {
    require_non_empty("snapshot_id", snapshot_id)?;
    require_non_empty("ref_id", ref_id)
}

fn validate_bounded_text(name: &str, value: &str, permit_empty: bool) -> Result<(), String> {
    if !permit_empty && value.is_empty() {
        return Err(format!("{name} must be non-empty"));
    }
    if value.len() > MAX_BROWSER_MCP_TEXT_BYTES {
        return Err(format!("{name} exceeds {MAX_BROWSER_MCP_TEXT_BYTES} bytes"));
    }
    Ok(())
}

fn require_non_empty(name: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{name} must be non-empty"));
    }
    Ok(())
}

/// The plan fields the postcondition judgment needs, captured before the
/// browser loop consumes the config.
#[derive(Default)]
pub(crate) struct BrowserVerificationInputs {
    pub(crate) postcondition: String,
    pub(crate) stop_criteria: String,
    pub(crate) zdr: bool,
    /// Why this run used the deterministic loop, when that is worth saying
    /// (anything but the default `mode_off`). Recorded in the usage line.
    pub(crate) deterministic_reason: Option<&'static str>,
}

/// The planner policy's answer for one run, with the planner built.
pub(crate) enum PlannerSelection {
    Llm(crate::llm_planner::LlmPlanner),
    Deterministic(crate::planner_policy::DeterministicReason),
    /// The policy picked the planner but it cannot be built (no usable
    /// inference credential or endpoint). The run fails with this reason.
    Unavailable(String),
}

/// Resolve the planner policy for one run and build the planner when it picks
/// one. `flag` is `tool_input.planner_flag` (already limited to the gateway's
/// three values).
pub(crate) fn select_planner(
    policy: &crate::planner_policy::PlannerPolicy,
    org_id: &str,
    zdr: bool,
    flag: Option<&str>,
    inference_bearer: Option<&str>,
) -> PlannerSelection {
    use crate::planner_policy::{OrgPlannerFlag, PlannerDecision};
    match policy.resolve(org_id, zdr, OrgPlannerFlag::from_wire(flag)) {
        PlannerDecision::Deterministic(reason) => PlannerSelection::Deterministic(reason),
        PlannerDecision::Llm => match crate::llm_planner::LlmPlanner::connect(inference_bearer) {
            Some(planner) => PlannerSelection::Llm(planner),
            None => PlannerSelection::Unavailable(
                "browser planner unavailable: no usable inference credential or endpoint"
                    .to_owned(),
            ),
        },
    }
}

/// ` usage=planner=deterministic reason=<reason>` for a run the policy kept off
/// the planner, empty otherwise. A closed reason, no content.
fn deterministic_suffix(verification: &BrowserVerificationInputs) -> String {
    verification
        .deterministic_reason
        .map_or_else(String::new, |reason| {
            format!(" usage=planner=deterministic reason={reason}")
        })
}

/// Judge a completed browser loop against its own final observation and
/// render the verdict for the step output (roadmap P1 item 3).
///
/// Deliberately **annotates** rather than rewrites the lifecycle status: a
/// `Completed` plan whose final observation failed is worth stating plainly,
/// but downgrading it to `Failed` would change execution semantics on an
/// inference about when those two can legitimately co-occur, which has not
/// been established. Surfacing the verdict makes the mismatch visible to the
/// model, the run console, and any operator reading the step — without
/// silently altering behavior.
fn browser_verification_note(
    observations: &[browser_agent::BrowserObservation],
    inputs: &BrowserVerificationInputs,
) -> String {
    let final_observation = observations.last();
    let evidence = crate::postcondition::BrowserProcedureEvidence {
        aborted: false,
        final_status: final_observation.map(|o| o.status.as_str()),
        final_page_title: final_observation.map_or("", |o| o.page_title.as_str()),
        final_page_text: final_observation.map_or("", |o| o.extracted_text.as_str()),
        has_durable_evidence: final_observation.is_some_and(|o| {
            !o.screenshot_ref.trim().is_empty() || !o.dom_snapshot_ref.trim().is_empty()
        }),
        zdr: inputs.zdr,
        stop_criteria: &inputs.stop_criteria,
        // Empty means the caller declared none. The judgment then refuses to
        // confirm rather than falling back to the (circular) stop criteria.
        declared_postcondition: Some(inputs.postcondition.as_str())
            .filter(|value| !value.trim().is_empty()),
    };
    let outcome = crate::postcondition::judge_browser_procedure(&evidence);
    format!(
        "{}: {}",
        outcome.method().unwrap_or("none"),
        outcome.detail()
    )
}

fn browser_execution_from_loop_result(
    result: browser_agent::BrowserAgentLoopResult,
    verification: &BrowserVerificationInputs,
) -> BrowserAgentExecution {
    use browser_agent::{BrowserAbortReason, BrowserResourceLimit, PlanStatus};

    if let Some(resource_limit) = result.resource_limit {
        return match resource_limit {
            // Time-shaped limits are a timeout; every budget and backpressure
            // limit is resource exhaustion. None of them is `Failed`.
            BrowserResourceLimit::Runtime
            | BrowserResourceLimit::Credential
            | BrowserResourceLimit::QuarryBudget(crate::quarry_agent::BudgetDimension::Runtime) => {
                BrowserAgentExecution::TimedOut {
                    reason: result.summary,
                }
            }
            BrowserResourceLimit::MaxSteps
            | BrowserResourceLimit::Tokens
            | BrowserResourceLimit::QuarryBudget(_)
            | BrowserResourceLimit::QuarryBackpressure => {
                BrowserAgentExecution::ResourceExhausted {
                    reason: result.summary,
                }
            }
        };
    }

    match result.status {
        PlanStatus::Completed => BrowserAgentExecution::Completed {
            output: format!(
                "status=completed summary={} verification={}{}{}",
                result.summary,
                browser_verification_note(&result.observations, verification),
                usage_suffix(result.usage),
                deterministic_suffix(verification)
            ),
        },
        PlanStatus::Failed => BrowserAgentExecution::Failed {
            reason: result.summary,
        },
        PlanStatus::WaitingApproval => BrowserAgentExecution::AwaitingApproval,
        PlanStatus::Aborted => match result.abort_reason {
            Some(BrowserAbortReason::Cancelled) => BrowserAgentExecution::Cancelled {
                reason: result.summary,
            },
            Some(BrowserAbortReason::ApprovalDenied) => BrowserAgentExecution::PermissionDenied {
                reason: result.summary,
            },
            Some(BrowserAbortReason::ApprovalTimedOut) => BrowserAgentExecution::TimedOut {
                reason: result.summary,
            },
            Some(BrowserAbortReason::Other) | None => BrowserAgentExecution::Aborted {
                reason: result.summary,
            },
        },
        // The loop contract should not return an in-progress state after its
        // run future resolves. If it does, fail closed rather than falling
        // through to generic successful tool handling.
        PlanStatus::Planning | PlanStatus::Executing => BrowserAgentExecution::Failed {
            reason: format!(
                "browser agent returned a non-terminal plan status: {}",
                result.status.as_str()
            ),
        },
    }
}

/// ` usage=<counts>` when a planner ran, empty otherwise (a deterministic run
/// keeps the output it always had). Counts only, no content.
fn usage_suffix(usage: browser_agent::BrowserLoopUsage) -> String {
    if usage.planner_calls == 0 {
        String::new()
    } else {
        format!(" usage={}", usage.line())
    }
}

/// Default and ceiling for the per-run bounds, read from the environment on
/// every call (`QUARRY_BROWSER_AGENT_{DEFAULT,CEILING}_MAX_{STEPS,RUNTIME_S}`,
/// `QUARRY_BROWSER_AGENT_MAX_PLANNER_TOKENS` and `_CEILING`). An unset or
/// unparsable value takes the built-in number, so with no env set a run gets
/// the 20 steps and 120 s it always had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlanCeilings {
    default_steps: i32,
    ceiling_steps: i32,
    default_runtime_s: i32,
    ceiling_runtime_s: i32,
    default_planner_tokens: u64,
    ceiling_planner_tokens: u64,
    max_consecutive_failures: u32,
    max_total_failures: u32,
    procedure: browser_agent::procedure::ProcedureId,
    widened_actions: bool,
}

impl Default for PlanCeilings {
    fn default() -> Self {
        Self {
            default_steps: 20,
            ceiling_steps: 100,
            default_runtime_s: 120,
            ceiling_runtime_s: 900,
            default_planner_tokens: 200_000,
            ceiling_planner_tokens: 1_000_000,
            max_consecutive_failures: 3,
            max_total_failures: 8,
            procedure: browser_agent::procedure::ProcedureId::Legacy0,
            widened_actions: false,
        }
    }
}

impl PlanCeilings {
    pub(crate) fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub(crate) fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        fn positive<T: std::str::FromStr + PartialOrd + Default>(
            lookup: &impl Fn(&str) -> Option<String>,
            name: &str,
            fallback: T,
        ) -> T {
            lookup(name)
                .and_then(|value| value.trim().parse::<T>().ok())
                .filter(|value| *value > T::default())
                .unwrap_or(fallback)
        }
        let built_in = Self::default();
        let ceiling_steps = positive(
            &lookup,
            "QUARRY_BROWSER_AGENT_CEILING_MAX_STEPS",
            built_in.ceiling_steps,
        );
        let ceiling_runtime_s = positive(
            &lookup,
            "QUARRY_BROWSER_AGENT_CEILING_MAX_RUNTIME_S",
            built_in.ceiling_runtime_s,
        );
        let ceiling_planner_tokens = positive(
            &lookup,
            "QUARRY_BROWSER_AGENT_MAX_PLANNER_TOKENS_CEILING",
            built_in.ceiling_planner_tokens,
        );
        // A default above its ceiling would hand out more than the ceiling.
        Self {
            default_steps: positive(
                &lookup,
                "QUARRY_BROWSER_AGENT_DEFAULT_MAX_STEPS",
                built_in.default_steps,
            )
            .min(ceiling_steps),
            ceiling_steps,
            default_runtime_s: positive(
                &lookup,
                "QUARRY_BROWSER_AGENT_DEFAULT_MAX_RUNTIME_S",
                built_in.default_runtime_s,
            )
            .min(ceiling_runtime_s),
            ceiling_runtime_s,
            default_planner_tokens: positive(
                &lookup,
                "QUARRY_BROWSER_AGENT_MAX_PLANNER_TOKENS",
                built_in.default_planner_tokens,
            )
            .min(ceiling_planner_tokens),
            ceiling_planner_tokens,
            max_consecutive_failures: positive(
                &lookup,
                "QUARRY_BROWSER_AGENT_MAX_CONSECUTIVE_FAILURES",
                built_in.max_consecutive_failures,
            ),
            max_total_failures: positive(
                &lookup,
                "QUARRY_BROWSER_AGENT_MAX_TOTAL_FAILURES",
                built_in.max_total_failures,
            ),
            procedure: browser_agent::procedure::ProcedureId::from_lookup(&lookup),
            // Literal `1`, `true` or `on` enables; anything else is off.
            widened_actions: lookup("QUARRY_BROWSER_AGENT_WIDENED_ACTIONS")
                .is_some_and(|value| matches!(value.trim(), "1" | "true" | "on")),
        }
    }

    /// The step bound: the caller's value (zero, negative or absent take the
    /// default), clamped to the ceiling, then tightened by the grant's claim.
    pub(crate) fn steps(&self, requested: Option<i32>, claim: i32) -> i32 {
        tighten(
            requested
                .filter(|value| *value > 0)
                .unwrap_or(self.default_steps)
                .min(self.ceiling_steps),
            claim,
        )
    }

    /// The runtime bound in seconds; same rules as [`Self::steps`].
    pub(crate) fn runtime_s(&self, requested: Option<i32>, claim: i32) -> i32 {
        tighten(
            requested
                .filter(|value| *value > 0)
                .unwrap_or(self.default_runtime_s)
                .min(self.ceiling_runtime_s),
            claim,
        )
    }

    /// The planner token budget: the caller's value (absent or non-positive
    /// takes the default), never above the ceiling.
    pub(crate) fn planner_tokens(&self, requested: Option<i64>) -> u64 {
        requested
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0)
            .unwrap_or(self.default_planner_tokens)
            .min(self.ceiling_planner_tokens)
    }
}

/// `min(value, claim)` where a non-positive claim means no bound.
fn tighten(value: i32, claim: i32) -> i32 {
    if claim > 0 {
        value.min(claim)
    } else {
        value
    }
}

/// The org planner switch values the gateway writes; anything else is unset.
fn known_planner_flag(value: Option<&str>) -> Option<String> {
    let value = value?.trim().to_ascii_lowercase();
    matches!(value.as_str(), "on" | "off" | "unavailable").then_some(value)
}

#[derive(serde::Deserialize)]
struct BrowserAgentInput {
    grant_id: String,
    plan_id: Option<String>,
    run_id: Option<String>,
    system_prompt: Option<String>,
    max_steps: Option<i32>,
    max_runtime_s: Option<i32>,
    #[serde(rename = "allowed_domains")]
    _allowed_domains: Option<Vec<String>>,
    stop_criteria: Option<String>,
    require_approval: Option<bool>,
    max_cost_usd: Option<f64>,
    /// Persistent Quarry browser profile to reuse cookie/session state from
    /// (Phase 2). `None` acquires a fresh, isolated Quarry session.
    profile_id: Option<String>,
    /// Where to navigate first (Phase 2) — see `PlanConfig::start_url`.
    start_url: Option<String>,
    /// Independent success condition for postcondition verification — see
    /// `PlanConfig::postcondition`. Must differ from `stop_criteria` to be
    /// worth anything; omitting it means the procedure can be refuted but
    /// never confirmed.
    postcondition: Option<String>,
    /// Planner token budget the gateway computed (the tighter of the request
    /// and the org ceiling). Tighten-only: it is clamped to the env ceiling
    /// here and can never raise it.
    max_planner_tokens: Option<i64>,
    /// The org planner switch the gateway wrote from the org quota row
    /// (`on`, `off`, `unavailable`); read by the planner policy (WS11.S2).
    planner_flag: Option<String>,
}

fn browser_plan_config(
    input: BrowserAgentInput,
    verified_org_id: &str,
    verified_zdr: bool,
    validated_grant: &ValidatedBrowserGrant,
) -> Result<browser_agent::PlanConfig, String> {
    browser_plan_config_with(
        input,
        verified_org_id,
        verified_zdr,
        validated_grant,
        &PlanCeilings::from_env(),
    )
}

fn browser_plan_config_with(
    input: BrowserAgentInput,
    verified_org_id: &str,
    verified_zdr: bool,
    validated_grant: &ValidatedBrowserGrant,
    ceilings: &PlanCeilings,
) -> Result<browser_agent::PlanConfig, String> {
    if input.grant_id.trim() != validated_grant.grant_id() {
        return Err(
            "browser tool input grant does not match the broker-validated grant".to_owned(),
        );
    }
    let plan = browser_agent::PlanConfig {
        plan_id: input.plan_id.unwrap_or_else(mp_ids::new_ulid),
        grant_id: validated_grant.grant_id().to_owned(),
        run_id: input.run_id.unwrap_or_default(),
        org_id: verified_org_id.to_owned(),
        system_prompt: input.system_prompt.unwrap_or_default(),
        max_steps: ceilings.steps(input.max_steps, validated_grant.budget_max_steps),
        max_runtime_s: ceilings.runtime_s(
            input.max_runtime_s,
            validated_grant.budget_max_runtime_s,
        ),
        // Caller-supplied `allowed_domains` is deliberately ignored. The
        // only policy that reaches Quarry is the broker-owned validation.
        allowed_domains: validated_grant.allowed_domains().to_vec(),
        stop_criteria: input.stop_criteria.unwrap_or_default(),
        require_approval: input.require_approval.unwrap_or(false),
        max_cost_usd: input.max_cost_usd,
        zdr: verified_zdr,
        profile_id: input.profile_id,
        start_url: input.start_url,
        postcondition: input.postcondition.unwrap_or_default(),
        max_planner_tokens: Some(ceilings.planner_tokens(input.max_planner_tokens)),
        // No source for the credential expiry yet (see DEPTH.md).
        deadline_at: None,
        planner_flag: known_planner_flag(input.planner_flag.as_deref()),
        max_consecutive_failures: ceilings.max_consecutive_failures,
        max_total_failures: ceilings.max_total_failures,
        procedure: ceilings.procedure,
        widened_actions: ceilings.widened_actions,
    };
    browser_agent::validate_plan_config(&plan)?;
    Ok(plan)
}

/// Parse an opaque browser grant lookup key. This intentionally performs no
/// authorization; callers must send it to BrowserBroker with a verified
/// browser audience credential before a `ValidatedBrowserGrant` is created.
pub(crate) fn requested_browser_grant_id(tool_input: &str) -> Result<String, String> {
    let input: BrowserAgentInput = serde_json::from_str(tool_input)
        .map_err(|error| format!("invalid browser_agent input: {error}"))?;
    let grant_id = input.grant_id.trim();
    if grant_id.is_empty() {
        return Err("browser_agent grant_id is required".to_owned());
    }
    Ok(grant_id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_plan_uses_verified_org_not_model_input() {
        let input: BrowserAgentInput = serde_json::from_str(
            r#"{"grant_id":"grant-1","org_id":"attacker-org","zdr":false,"max_steps":3}"#,
        )
        .unwrap();

        let grant = ValidatedBrowserGrant::from_broker_response(
            "grant-1",
            &ValidateGrantResponse {
                grant_id: "grant-1".to_owned(),
                active: true,
                allowed_domains: vec!["example.com".to_owned()],
                ..Default::default()
            },
        )
        .unwrap();

        let plan = browser_plan_config(input, "verified-org", true, &grant).unwrap();
        assert_eq!(plan.org_id, "verified-org");
        assert!(
            plan.zdr,
            "verified run ZDR cannot be downgraded by tool input"
        );
        assert_eq!(plan.max_steps, 3);
    }

    #[test]
    fn broker_validated_policy_replaces_caller_domains() {
        let input: BrowserAgentInput = serde_json::from_str(
            r#"{"grant_id":"grant-1","allowed_domains":["evil.example"],"start_url":"https://api.example.com/start"}"#,
        )
        .unwrap();
        let grant = ValidatedBrowserGrant::from_broker_response(
            "grant-1",
            &ValidateGrantResponse {
                grant_id: "grant-1".to_owned(),
                active: true,
                allowed_domains: vec!["example.com".to_owned()],
                ..Default::default()
            },
        )
        .unwrap();

        let config = browser_plan_config(input, "verified-org", false, &grant).unwrap();
        assert_eq!(config.allowed_domains, vec!["example.com"]);
        let plan = browser_agent::AgentPlan::new(config);
        assert!(plan.is_domain_allowed("https://api.example.com/start"));
        assert!(!plan.is_domain_allowed("https://evil.example/"));
    }

    #[test]
    fn broker_response_rejects_empty_or_mismatched_policy() {
        let empty = ValidatedBrowserGrant::from_broker_response(
            "grant-1",
            &ValidateGrantResponse {
                grant_id: "grant-1".to_owned(),
                active: true,
                ..Default::default()
            },
        );
        assert!(empty.is_err());

        let mismatched = ValidatedBrowserGrant::from_broker_response(
            "grant-1",
            &ValidateGrantResponse {
                grant_id: "forged-grant".to_owned(),
                active: true,
                allowed_domains: vec!["example.com".to_owned()],
                ..Default::default()
            },
        );
        assert!(mismatched.is_err());
    }

    #[test]
    fn broker_response_carries_the_grants_lineage_owner_and_retention() {
        let grant = ValidatedBrowserGrant::from_broker_response(
            "grant-1",
            &ValidateGrantResponse {
                grant_id: "grant-1".to_owned(),
                active: true,
                allowed_domains: vec!["example.com".to_owned()],
                parent_grant_id: "grant-session".to_owned(),
                kind: "run".to_owned(),
                owner_id: "user-1".to_owned(),
                zdr: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(grant.kind(), "run");
        assert_eq!(grant.parent_grant_id(), "grant-session");
        assert_eq!(grant.owner_id(), "user-1");
        assert!(grant.zdr());

        let bare = ValidatedBrowserGrant::from_broker_response(
            "grant-1",
            &ValidateGrantResponse {
                grant_id: "grant-1".to_owned(),
                active: true,
                allowed_domains: vec!["example.com".to_owned()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(bare.kind(), "");
        assert_eq!(bare.parent_grant_id(), "");
        assert_eq!(bare.owner_id(), "");
        assert!(!bare.zdr());
    }

    #[test]
    fn requested_grant_id_is_only_an_opaque_lookup_key() {
        assert_eq!(
            requested_browser_grant_id(r#"{"grant_id":"grant-1","org_id":"attacker"}"#).unwrap(),
            "grant-1"
        );
        assert!(requested_browser_grant_id(r#"{"grant_id":"   "}"#).is_err());
    }

    #[test]
    fn browser_abort_reasons_remain_non_successful_across_the_tool_bridge() {
        use browser_agent::BrowserAbortReason;

        let cases = [
            (
                BrowserAbortReason::Cancelled,
                BrowserAgentExecution::Cancelled {
                    reason: "cancelled by user".to_owned(),
                },
            ),
            (
                BrowserAbortReason::ApprovalDenied,
                BrowserAgentExecution::PermissionDenied {
                    reason: "cancelled by user".to_owned(),
                },
            ),
            (
                BrowserAbortReason::ApprovalTimedOut,
                BrowserAgentExecution::TimedOut {
                    reason: "cancelled by user".to_owned(),
                },
            ),
        ];

        for (abort_reason, expected) in cases {
            let result = browser_agent::BrowserAgentLoopResult {
                status: browser_agent::PlanStatus::Aborted,
                observations: Vec::new(),
                summary: "cancelled by user".to_owned(),
                abort_reason: Some(abort_reason),
                resource_limit: None,
                usage: browser_agent::BrowserLoopUsage::default(),
                failure_kind: None,
            };

            assert_eq!(
                browser_execution_from_loop_result(result, &BrowserVerificationInputs::default()),
                expected
            );
        }
    }

    #[test]
    fn browser_resource_limits_remain_non_successful_across_the_tool_bridge() {
        use browser_agent::BrowserResourceLimit;

        let cases = [
            (
                BrowserResourceLimit::MaxSteps,
                BrowserAgentExecution::ResourceExhausted {
                    reason: "max_steps exceeded".to_owned(),
                },
            ),
            (
                BrowserResourceLimit::Runtime,
                BrowserAgentExecution::TimedOut {
                    reason: "max_runtime_s exceeded".to_owned(),
                },
            ),
        ];

        for (resource_limit, expected) in cases {
            let result = browser_agent::BrowserAgentLoopResult {
                status: browser_agent::PlanStatus::Failed,
                observations: Vec::new(),
                summary: match resource_limit {
                    BrowserResourceLimit::MaxSteps => "max_steps exceeded".to_owned(),
                    BrowserResourceLimit::Runtime => "max_runtime_s exceeded".to_owned(),
                    _ => unreachable!("only the two original limits are listed here"),
                },
                abort_reason: None,
                resource_limit: Some(resource_limit),
                usage: browser_agent::BrowserLoopUsage::default(),
                failure_kind: None,
            };

            assert_eq!(
                browser_execution_from_loop_result(result, &BrowserVerificationInputs::default()),
                expected
            );
        }
    }

    #[test]
    fn resource_limit_outcomes_map_to_step_outcomes() {
        use crate::quarry_agent::BudgetDimension as D;
        use browser_agent::BrowserResourceLimit as L;

        let resource_exhausted = [
            L::MaxSteps,
            L::Tokens,
            L::QuarryBackpressure,
            L::QuarryBudget(D::Steps),
            L::QuarryBudget(D::Attempts),
            L::QuarryBudget(D::Cost),
            L::QuarryBudget(D::OrgAllowance),
            L::QuarryBudget(D::Other),
        ];
        let timed_out = [L::Runtime, L::Credential, L::QuarryBudget(D::Runtime)];
        let run = |limit: L| {
            browser_execution_from_loop_result(
                browser_agent::BrowserAgentLoopResult {
                    status: browser_agent::PlanStatus::Failed,
                    observations: Vec::new(),
                    summary: limit.reason().to_owned(),
                    abort_reason: None,
                    resource_limit: Some(limit),
                    usage: browser_agent::BrowserLoopUsage::default(),
                    failure_kind: None,
                },
                &BrowserVerificationInputs::default(),
            )
        };
        for limit in resource_exhausted {
            assert_eq!(
                run(limit),
                BrowserAgentExecution::ResourceExhausted {
                    reason: limit.reason().to_owned()
                },
                "{limit:?}"
            );
        }
        for limit in timed_out {
            assert_eq!(
                run(limit),
                BrowserAgentExecution::TimedOut {
                    reason: limit.reason().to_owned()
                },
                "{limit:?}"
            );
        }
    }

    #[test]
    fn usage_line_is_counts_only() {
        let usage = browser_agent::BrowserLoopUsage {
            planner_calls: 3,
            planner_input_tokens: 1200,
            planner_output_tokens: 80,
        };
        assert_eq!(
            usage.line(),
            "planner_calls=3 planner_input_tokens=1200 planner_output_tokens=80"
        );
        assert_eq!(usage_suffix(browser_agent::BrowserLoopUsage::default()), "");
        let completed = browser_execution_from_loop_result(
            browser_agent::BrowserAgentLoopResult {
                status: browser_agent::PlanStatus::Completed,
                observations: Vec::new(),
                summary: "agent reported task complete".to_owned(),
                abort_reason: None,
                resource_limit: None,
                usage,
                failure_kind: None,
            },
            &BrowserVerificationInputs::default(),
        );
        let BrowserAgentExecution::Completed { output } = completed else {
            panic!("expected Completed");
        };
        assert!(
            output.ends_with(
                " usage=planner_calls=3 planner_input_tokens=1200 planner_output_tokens=80"
            ),
            "{output}"
        );
    }

    fn ceilings_with(vars: &[(&str, &str)]) -> PlanCeilings {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        PlanCeilings::from_lookup(|name| {
            vars.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        })
    }

    #[test]
    fn caller_steps_above_ceiling_are_clamped() {
        let ceilings = ceilings_with(&[]);
        assert_eq!(ceilings.steps(Some(500), 0), 100);
        assert_eq!(ceilings.steps(Some(7), 0), 7);
        let tight = ceilings_with(&[("QUARRY_BROWSER_AGENT_CEILING_MAX_STEPS", "30")]);
        assert_eq!(tight.steps(Some(50), 0), 30);
        // The grant's claim tightens further, never loosens.
        assert_eq!(ceilings.steps(Some(50), 12), 12);
        assert_eq!(ceilings.steps(Some(5), 12), 5);
        // Runtime and planner tokens clamp the same way.
        assert_eq!(ceilings.runtime_s(Some(5000), 0), 900);
        assert_eq!(ceilings.planner_tokens(Some(5_000_000)), 1_000_000);
        assert_eq!(ceilings.planner_tokens(Some(40_000)), 40_000);
    }

    #[test]
    fn zero_or_absent_steps_take_the_default() {
        let ceilings = ceilings_with(&[]);
        for requested in [None, Some(0), Some(-4)] {
            assert_eq!(ceilings.steps(requested, 0), 20, "{requested:?}");
            assert_eq!(ceilings.runtime_s(requested, 0), 120, "{requested:?}");
        }
        assert_eq!(ceilings.planner_tokens(None), 200_000);
        assert_eq!(ceilings.planner_tokens(Some(0)), 200_000);
        // Garbage in the environment is the built-in number, and a default
        // above its ceiling is cut to the ceiling.
        let odd = ceilings_with(&[
            ("QUARRY_BROWSER_AGENT_DEFAULT_MAX_STEPS", "lots"),
            ("QUARRY_BROWSER_AGENT_DEFAULT_MAX_RUNTIME_S", "2000"),
        ]);
        assert_eq!(odd.steps(None, 0), 20);
        assert_eq!(odd.runtime_s(None, 0), 900);
    }

    #[test]
    fn failure_budget_limits_come_from_env() {
        assert_eq!(ceilings_with(&[]).max_consecutive_failures, 3);
        assert_eq!(ceilings_with(&[]).max_total_failures, 8);
        let set = ceilings_with(&[
            ("QUARRY_BROWSER_AGENT_MAX_CONSECUTIVE_FAILURES", "5"),
            ("QUARRY_BROWSER_AGENT_MAX_TOTAL_FAILURES", "nope"),
        ]);
        assert_eq!(set.max_consecutive_failures, 5);
        assert_eq!(set.max_total_failures, 8);
    }

    #[test]
    fn widened_actions_default_off_and_only_literal_values_enable() {
        assert!(!ceilings_with(&[]).widened_actions);
        for on in ["1", "true", "on", " 1 "] {
            let set = ceilings_with(&[("QUARRY_BROWSER_AGENT_WIDENED_ACTIONS", on)]);
            assert!(set.widened_actions, "{on:?}");
        }
        for off in ["0", "", "yes", "TRUE", "enabled", "2"] {
            let set = ceilings_with(&[("QUARRY_BROWSER_AGENT_WIDENED_ACTIONS", off)]);
            assert!(!set.widened_actions, "{off:?}");
        }
    }

    #[test]
    fn procedure_defaults_to_legacy_and_the_env_selects_browse_1() {
        use browser_agent::procedure::ProcedureId;
        assert_eq!(ceilings_with(&[]).procedure, ProcedureId::Legacy0);
        let set = ceilings_with(&[("QUARRY_BROWSER_AGENT_PROCEDURE", "browse/1")]);
        assert_eq!(set.procedure, ProcedureId::Browse1);
        let junk = ceilings_with(&[("QUARRY_BROWSER_AGENT_PROCEDURE", "browse/9")]);
        assert_eq!(junk.procedure, ProcedureId::Legacy0);
    }

    fn completed_output(verification: &BrowserVerificationInputs) -> String {
        let result = browser_agent::BrowserAgentLoopResult {
            status: browser_agent::PlanStatus::Completed,
            observations: Vec::new(),
            summary: "agent reported task complete".to_owned(),
            abort_reason: None,
            resource_limit: None,
            usage: browser_agent::BrowserLoopUsage::default(),
            failure_kind: None,
        };
        let BrowserAgentExecution::Completed { output } =
            browser_execution_from_loop_result(result, verification)
        else {
            panic!("expected Completed");
        };
        output
    }

    #[test]
    fn deterministic_reason_is_recorded_in_usage_line() {
        use crate::planner_policy::{DeterministicReason, OrgPlannerFlag, PlannerMode, PlannerPolicy};
        let policy = PlannerPolicy {
            mode: PlannerMode::DefaultOn,
            ..PlannerPolicy::default()
        };
        let PlannerSelection::Deterministic(reason) =
            select_planner(&policy, "org_a", false, Some("off"), Some("tok"))
        else {
            panic!("an org that said off gets the deterministic loop");
        };
        assert_eq!(reason, DeterministicReason::OrgOff);
        let with = BrowserVerificationInputs {
            deterministic_reason: Some(reason.as_str()),
            ..Default::default()
        };
        assert!(
            completed_output(&with).ends_with(" usage=planner=deterministic reason=org_off"),
            "{}",
            completed_output(&with)
        );
        // The default deployment (mode off) keeps the output it always had.
        assert!(!completed_output(&BrowserVerificationInputs::default()).contains("usage="));
        assert_eq!(OrgPlannerFlag::from_wire(Some("off")), OrgPlannerFlag::Off);
    }

    #[tokio::test]
    async fn llm_selected_without_bearer_fails_with_reason_not_silent_fallback() {
        use crate::planner_policy::{PlannerMode, PlannerPolicy};
        let policy = PlannerPolicy {
            mode: PlannerMode::DefaultOn,
            ..PlannerPolicy::default()
        };
        for bearer in [None, Some(""), Some("two words")] {
            match select_planner(&policy, "org_a", false, None, bearer) {
                PlannerSelection::Unavailable(reason) => {
                    assert!(reason.starts_with("browser planner unavailable"), "{reason}");
                }
                PlannerSelection::Llm(_) => panic!("{bearer:?} must not build a planner"),
                PlannerSelection::Deterministic(_) => {
                    panic!("{bearer:?}: a selected planner never falls back silently")
                }
            }
        }
        assert!(matches!(
            select_planner(&policy, "org_a", false, None, Some("verified-bearer")),
            PlannerSelection::Llm(_)
        ));
    }

    #[test]
    fn tool_input_planner_flag_unknown_value_is_unset() {
        for value in ["maybe", "1", "true", "", "ON "] {
            let kept = known_planner_flag(Some(value));
            // Only the gateway's three values survive; case and spaces are tolerated.
            assert_eq!(kept.is_some(), value.trim().eq_ignore_ascii_case("on"), "{value:?}");
        }
        assert_eq!(known_planner_flag(Some("off")).as_deref(), Some("off"));
        assert_eq!(known_planner_flag(Some("unavailable")).as_deref(), Some("unavailable"));
        assert_eq!(known_planner_flag(None), None);
        assert_eq!(
            crate::planner_policy::OrgPlannerFlag::from_wire(known_planner_flag(Some("maybe")).as_deref()),
            crate::planner_policy::OrgPlannerFlag::Unset
        );
    }

    #[test]
    fn runtime_default_is_120() {
        assert_eq!(ceilings_with(&[]).runtime_s(None, 0), 120);
        assert_eq!(PlanCeilings::default().runtime_s(None, 0), 120);
        let set = ceilings_with(&[("QUARRY_BROWSER_AGENT_DEFAULT_MAX_RUNTIME_S", "300")]);
        assert_eq!(set.runtime_s(None, 0), 300);
    }

    #[test]
    fn plan_config_reads_planner_tokens_and_flag_from_tool_input() {
        let grant = ValidatedBrowserGrant::for_test("grant-1", "run", "", "u", false);
        let build = |json: &str| {
            let input: BrowserAgentInput = serde_json::from_str(json).unwrap();
            browser_plan_config_with(input, "org", false, &grant, &PlanCeilings::default())
                .unwrap()
        };
        let plain = build(r#"{"grant_id":"grant-1"}"#);
        assert_eq!(plain.max_steps, 20);
        assert_eq!(plain.max_runtime_s, 120);
        assert_eq!(plain.max_planner_tokens, Some(200_000));
        assert_eq!(plain.planner_flag, None);
        assert!(plain.deadline_at.is_none());

        let tight = build(
            r#"{"grant_id":"grant-1","max_planner_tokens":40000,"planner_flag":"Off","max_steps":999}"#,
        );
        assert_eq!(tight.max_planner_tokens, Some(40_000));
        assert_eq!(tight.planner_flag.as_deref(), Some("off"));
        assert_eq!(tight.max_steps, 100);
        // The request can never raise the ceiling; an unknown flag is unset.
        let raised = build(r#"{"grant_id":"grant-1","max_planner_tokens":9000000,"planner_flag":"yes"}"#);
        assert_eq!(raised.max_planner_tokens, Some(1_000_000));
        assert_eq!(raised.planner_flag, None);
    }

    #[test]
    fn grant_budget_claim_tightens_the_plan_bounds() {
        let grant = ValidatedBrowserGrant::from_broker_response(
            "grant-1",
            &ValidateGrantResponse {
                grant_id: "grant-1".to_owned(),
                active: true,
                allowed_domains: vec!["example.com".to_owned()],
                budget_max_steps: 6,
                budget_max_runtime_s: 45,
                ..Default::default()
            },
        )
        .unwrap();
        let input: BrowserAgentInput =
            serde_json::from_str(r#"{"grant_id":"grant-1","max_steps":50}"#).unwrap();
        let plan =
            browser_plan_config_with(input, "org", false, &grant, &PlanCeilings::default())
                .unwrap();
        assert_eq!(plan.max_steps, 6);
        assert_eq!(plan.max_runtime_s, 45);
    }

    // --- Browser postcondition threading (roadmap P1 item 3) ----------------

    fn observation(
        status: browser_agent::ObservationStatus,
        text: &str,
    ) -> browser_agent::BrowserObservation {
        browser_agent::BrowserObservation {
            observation_id: "obs_1".to_owned(),
            action_id: "act_1".to_owned(),
            grant_id: "grant_1".to_owned(),
            status,
            page_url: "https://example.test/done".to_owned(),
            page_title: "Done".to_owned(),
            extracted_text: text.to_owned(),
            screenshot_ref: "shot_1".to_owned(),
            dom_snapshot_ref: String::new(),
            snapshot_generation: None,
            snapshot_targets: vec![],
            error_message: String::new(),
            challenge: None,
        }
    }

    #[test]
    fn a_declared_postcondition_reaches_the_judgment_and_confirms() {
        let observations = vec![observation(
            browser_agent::ObservationStatus::Success,
            "Your order 12345 is confirmed.",
        )];
        let inputs = BrowserVerificationInputs {
            postcondition: "order 12345".to_owned(),
            stop_criteria: "confirmed".to_owned(),
            zdr: false,
            ..Default::default()
        };
        let note = browser_verification_note(&observations, &inputs);
        assert!(
            note.starts_with("postcondition:"),
            "a declared postcondition must produce a postcondition-method verdict, got: {note}"
        );
        assert!(note.contains("order 12345"));
    }

    #[test]
    fn omitting_the_postcondition_still_refuses_to_confirm() {
        let observations = vec![observation(
            browser_agent::ObservationStatus::Success,
            "Your order 12345 is confirmed.",
        )];
        let note = browser_verification_note(&observations, &BrowserVerificationInputs::default());
        assert!(
            note.starts_with("none:"),
            "no declared postcondition must stay unverified, got: {note}"
        );
    }

    #[test]
    fn a_postcondition_equal_to_the_stop_criteria_is_refused_as_circular() {
        let observations = vec![observation(
            browser_agent::ObservationStatus::Success,
            "Your order is confirmed.",
        )];
        let inputs = BrowserVerificationInputs {
            postcondition: "confirmed".to_owned(),
            stop_criteria: "confirmed".to_owned(),
            zdr: false,
            ..Default::default()
        };
        let note = browser_verification_note(&observations, &inputs);
        assert!(note.starts_with("none:"), "got: {note}");
        assert!(note.contains("stop decision"));
    }

    #[test]
    fn a_failed_final_observation_is_refuted_even_with_a_good_postcondition() {
        let observations = vec![observation(
            browser_agent::ObservationStatus::Failed,
            "Your order 12345 is confirmed.",
        )];
        let inputs = BrowserVerificationInputs {
            postcondition: "order 12345".to_owned(),
            stop_criteria: "confirmed".to_owned(),
            zdr: false,
            ..Default::default()
        };
        let note = browser_verification_note(&observations, &inputs);
        assert!(note.starts_with("postcondition:"), "got: {note}");
        assert!(note.contains("failed"));
    }

    #[test]
    fn a_zdr_run_reaches_the_judgment_as_zdr() {
        let observations = vec![observation(browser_agent::ObservationStatus::Success, "")];
        let inputs = BrowserVerificationInputs {
            postcondition: "order 12345".to_owned(),
            stop_criteria: "confirmed".to_owned(),
            zdr: true,
            ..Default::default()
        };
        let note = browser_verification_note(&observations, &inputs);
        assert!(note.contains("Zero Data Retention"), "got: {note}");
    }
}
