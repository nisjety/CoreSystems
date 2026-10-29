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
        })
    }

    #[must_use]
    pub(crate) fn grant_id(&self) -> &str {
        &self.grant_id
    }

    #[must_use]
    pub(crate) fn allowed_domains(&self) -> &[String] {
        &self.allowed_domains
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
            let planner = crate::llm_planner::LlmPlanner::from_env(inference_bearer);
            // Captured before the loop consumes the config — the postcondition
            // judgment below needs all three, and the loop result carries
            // none of them.
            let verification_inputs = BrowserVerificationInputs {
                postcondition: plan_config.postcondition.clone(),
                stop_criteria: plan_config.stop_criteria.clone(),
                zdr: plan_config.zdr,
            };
            let result = browser_agent::run_browser_agent_loop(
                plan_config,
                client.as_ref(),
                planner.as_ref(),
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
        .step(
            &request.quarry_run_id,
            &request.lease_id,
            verified_org_id,
            action,
            &constraints,
            verified_zdr,
            validated_grant.grant_id(),
        )
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
            BrowserResourceLimit::MaxSteps => BrowserAgentExecution::ResourceExhausted {
                reason: result.summary,
            },
            BrowserResourceLimit::Runtime => BrowserAgentExecution::TimedOut {
                reason: result.summary,
            },
        };
    }

    match result.status {
        PlanStatus::Completed => BrowserAgentExecution::Completed {
            output: format!(
                "status=completed summary={} verification={}",
                result.summary,
                browser_verification_note(&result.observations, verification)
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
}

fn browser_plan_config(
    input: BrowserAgentInput,
    verified_org_id: &str,
    verified_zdr: bool,
    validated_grant: &ValidatedBrowserGrant,
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
        max_steps: input.max_steps.unwrap_or(20),
        max_runtime_s: input.max_runtime_s.unwrap_or(120),
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
                },
                abort_reason: None,
                resource_limit: Some(resource_limit),
            };

            assert_eq!(
                browser_execution_from_loop_result(result, &BrowserVerificationInputs::default()),
                expected
            );
        }
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
        };
        let note = browser_verification_note(&observations, &inputs);
        assert!(note.contains("Zero Data Retention"), "got: {note}");
    }
}
