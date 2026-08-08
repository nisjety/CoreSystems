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
            let result = browser_agent::run_browser_agent_loop(
                plan_config,
                client.as_ref(),
                planner.as_ref(),
                sink,
                state,
            )
            .await;
            browser_execution_from_loop_result(result)
        }
        Err(e) => BrowserAgentExecution::Failed {
            reason: format!("invalid browser_agent input: {e}"),
        },
    }
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
///
/// Browser procedures can currently be refuted but never confirmed; see
/// [`crate::postcondition::judge_browser_procedure`] for why (no
/// postcondition is declared independently of the loop's own stop criteria).
fn browser_verification_note(observations: &[browser_agent::BrowserObservation]) -> String {
    let final_observation = observations.last();
    let evidence = crate::postcondition::BrowserProcedureEvidence {
        aborted: false,
        final_status: final_observation.map(|o| o.status.as_str()),
        final_page_title: final_observation.map_or("", |o| o.page_title.as_str()),
        final_page_text: final_observation.map_or("", |o| o.extracted_text.as_str()),
        has_durable_evidence: final_observation.is_some_and(|o| {
            !o.screenshot_ref.trim().is_empty() || !o.dom_snapshot_ref.trim().is_empty()
        }),
        // The loop result carries no ZDR flag; a ZDR run simply arrives with
        // empty page content, which the judgment already treats as
        // unmatchable rather than as a failure.
        zdr: false,
        stop_criteria: "",
        // PlanConfig carries no independently-declared postcondition yet.
        declared_postcondition: None,
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
                browser_verification_note(&result.observations)
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

            assert_eq!(browser_execution_from_loop_result(result), expected);
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

            assert_eq!(browser_execution_from_loop_result(result), expected);
        }
    }
}
