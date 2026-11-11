//! Tool execution bridge.

use std::collections::BTreeMap;
use std::sync::Arc;

use mp_contracts::model_plane::v1::ValidateGrantResponse;

use crate::browser_agent::recipe::{bind_slots, Recipe, RecipeRefusal};
use crate::browser_agent::recipe_planner::{QuarryReplayChecker, RecipePlanner};
use crate::{browser_agent, browser_session, computer_action, recipe_store, wiki_agent};

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
    // Whether the broker narrowed the grant to the read level (WS21-S4). A read-level grant refuses
    // every browser action that changes the page (WS21-S5b).
    read_only: bool,
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
            read_only: response.read_only,
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
            read_only: false,
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

    /// Whether the broker narrowed the grant to the read level (WS21-S4).
    #[must_use]
    pub(crate) fn read_only(&self) -> bool {
        self.read_only
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
        Ok(mut input) => {
            // A saved recipe is taken out before the plan config takes the rest of the input (Q4-E4.2-3h).
            let recipe = input.recipe.take();
            let plan_config =
                match browser_plan_config(input, verified_org_id, verified_zdr, validated_grant) {
                    Ok(config) => config,
                    Err(error) => {
                        return BrowserAgentExecution::Failed { reason: error };
                    }
                };
            if plan_config.attach.is_some()
                && plan_config.policy.as_ref().is_some_and(|ask| !ask.is_empty())
                && plan_config.planner_flag.as_deref() == Some("unavailable")
            {
                return BrowserAgentExecution::Failed {
                    reason: "browser planner organization policy unavailable".to_owned(),
                };
            }
            // A recipe run replays one saved procedure. It is off by default, and it never attaches to
            // the person's own session: it runs in a browser session of its own, under its own grant.
            if recipe.is_some() {
                if !recipe_store::recipe_runner_enabled() {
                    return BrowserAgentExecution::Failed {
                        reason: "recipe runner is not enabled".to_owned(),
                    };
                }
                if plan_config.attach.is_some() {
                    return BrowserAgentExecution::Failed {
                        reason: "recipe runs need their own browser session".to_owned(),
                    };
                }
            }
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
            // A recipe run replays its saved procedure with `RecipePlanner` in place of the planner
            // policy's choice (Q4-E4.2-3h). Its planner is built after the client it checks with, and a
            // planner that cannot be built fails the run.
            let recipe_planner = match recipe {
                None => None,
                Some(recipe) => {
                    let Some(quarry) = client.clone() else {
                        return BrowserAgentExecution::Failed {
                            reason: "browser agent unavailable: set QUARRY_BROWSER_AGENT_ENABLED=1 and QUARRY_EDGE_URL"
                                .to_owned(),
                        };
                    };
                    let Some(source) = recipe_store::default_source() else {
                        return BrowserAgentExecution::Failed {
                            reason: "recipe source is unavailable".to_owned(),
                        };
                    };
                    match recipe_planner_for_run(
                        source.as_ref(),
                        &recipe,
                        validated_grant.owner_id(),
                        &plan_config.org_id,
                        plan_config.request_id.clone(),
                        quarry,
                    )
                    .await
                    {
                        Ok(planner) => Some(planner),
                        Err(reason) => return BrowserAgentExecution::Failed { reason },
                    }
                }
            };
            let recipe_run = recipe_planner.is_some();
            // The planner policy (deployment mode plus the org's flag) decides;
            // a selected planner that cannot be built fails the run with a
            // reason, it never degrades to the deterministic loop.
            let (planner, deterministic_reason) = match recipe_planner {
                Some(recipe_planner) => {
                    let planner: Box<dyn crate::llm_planner::ActionPlanner> =
                        Box::new(recipe_planner);
                    (Some(planner), None)
                }
                None => match select_planner(
                    &crate::planner_policy::PlannerPolicy::from_env(),
                    &plan_config.org_id,
                    plan_config.zdr,
                    plan_config.planner_flag.as_deref(),
                    inference_bearer,
                ) {
                    PlannerSelection::Llm(planner) => {
                        let planner: Box<dyn crate::llm_planner::ActionPlanner> = Box::new(planner);
                        (Some(planner), None)
                    }
                    PlannerSelection::Deterministic(reason) => {
                        if plan_config.attach.is_some() && plan_config.policy.as_ref().is_some_and(|ask| !ask.is_empty()) {
                            // A restricted attached run is surfaced as a live AI
                            // assistant. Never silently substitute observation-only
                            // deterministic behavior when its planner is disabled.
                            return BrowserAgentExecution::Failed {
                                reason: format!("browser planner unavailable ({})", reason.as_str()),
                            };
                        }
                        tracing::info!(reason = reason.as_str(), "browser run uses the deterministic loop");
                        browser_agent::metrics::record_deterministic(reason);
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
                        browser_agent::metrics::record_run(
                            browser_agent::metrics::planner_label(true),
                            "failed",
                        );
                        return BrowserAgentExecution::Failed { reason };
                    }
                },
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
            let planner_label = if recipe_run {
                browser_agent::metrics::RECIPE_PLANNER_LABEL
            } else {
                browser_agent::metrics::planner_label(planner.is_some())
            };
            let result = browser_agent::run_browser_agent_loop(
                plan_config,
                client.as_ref(),
                planner.as_deref(),
                sink,
                state,
            )
            .await;
            browser_agent::metrics::record_planner_tokens(result.usage);
            if let Some(kind) = result.failure_kind {
                browser_agent::metrics::record_failure(kind);
            }
            let execution = browser_execution_from_loop_result(result, &verification_inputs);
            browser_agent::metrics::record_run(planner_label, outcome_label(&execution));
            execution
        }
        Err(e) => BrowserAgentExecution::Failed {
            reason: format!("invalid browser_agent input: {e}"),
        },
    }
}

/// Reads the saved procedure a recipe run replays and builds the planner that replays it (Q4-E4.2-3h).
/// The request is judged, the procedure is read and checked against its digest, parsed, bound to the
/// person's slot values, and then checked step by step with Quarry through `quarry`. Every refusal is a
/// fixed reason, and neither the procedure nor a slot value is ever echoed into one.
async fn recipe_planner_for_run(
    source: &dyn recipe_store::RecipeSource,
    recipe: &RecipeInput,
    grant_owner: &str,
    org_id: &str,
    request_id: Option<String>,
    quarry: crate::quarry_agent::QuarryAgentClient,
) -> Result<RecipePlanner, String> {
    let procedure_id = recipe.id.trim();
    let for_user_id = recipe.for_user_id.trim();
    let tag = recipe
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|tag| !tag.is_empty());
    if let Some(refusal) =
        recipe_store::request_refusal(procedure_id, recipe.version, tag, for_user_id, grant_owner)
    {
        return Err(refusal.to_owned());
    }
    let fetched = source
        .get(org_id, procedure_id, recipe.version, tag, for_user_id)
        .await
        .map_err(|status| {
            tracing::warn!(code = ?status.code(), "recipe procedure read refused");
            "recipe procedure is unavailable".to_owned()
        })?;
    let saved = Recipe::parse(&fetched.body).map_err(recipe_refusal_reason)?;
    let bound = bind_slots(&saved, &recipe.slots).map_err(recipe_refusal_reason)?;
    let procedure: serde_json::Value = serde_json::from_slice(&fetched.body)
        .map_err(|_| recipe_refusal_reason(RecipeRefusal::Malformed))?;
    let checker = QuarryReplayChecker::new(quarry, org_id, request_id);
    Ok(RecipePlanner::new(procedure, bound, Arc::new(checker)))
}

/// The fixed reason a refused recipe fails its run with: the closed refusal token, and nothing of the
/// procedure or its values.
fn recipe_refusal_reason(refusal: RecipeRefusal) -> String {
    format!("recipe refused: {}", refusal.as_str())
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
    model_run_id: &str,
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

    let action = match request.kind {
        BrowserMcpKind::Observe => crate::quarry_agent::AgentAction::GetContent,
        BrowserMcpKind::Act(action) => action,
        // E3.2-e2: a tab operation, not a step: no lease, no action; the same
        // service token and 401 retry, with the advisory run id and label.
        BrowserMcpKind::Tabs(op) => {
            return execute_tab_op(
                &client,
                op,
                verified_org_id,
                request.quarry_run_id.trim(),
                validated_grant.grant_id(),
                model_run_id,
            )
            .await;
        }
        // WS9.E: a read of the run's console, not a step: no lease, no action,
        // the same service token and 401 retry.
        BrowserMcpKind::Console(query) => {
            return console_for_grant(
                &client,
                verified_org_id,
                &request.quarry_run_id,
                validated_grant.grant_id(),
                query,
            )
            .await;
        }
    };
    let constraints = crate::quarry_agent::AgentConstraints {
        max_steps: 1,
        allowed_domains: validated_grant.allowed_domains().to_vec(),
        max_runtime_s: Some(30),
        max_cost_usd: None,
    };
    match client
        .step({
            let mut params = crate::quarry_agent::StepParams::new(
                &request.quarry_run_id,
                &request.lease_id,
                verified_org_id,
                action,
                &constraints,
                verified_zdr,
                validated_grant.grant_id(),
            );
            // The model proposed this action (WS8.G): Quarry records it as an
            // agent's, which it would for a service principal anyway.
            params.initiated_by = Some("agent".to_owned());
            params
        })
        .await
    {
        Ok(observation) => match serde_json::to_string(&observation) {
            Ok(output) => ToolExecution {
                output,
                error: None,
            },
            Err(error) => browser_mcp_error(format!("serialize Quarry observation: {error}")),
        },
        // A policy refusal is final: the fixed text tells the chat model not to
        // retry or look for another route, and carries no host or message.
        Err(crate::quarry_agent::AgentClientError::PolicyDenied(denial)) => {
            browser_mcp_error(denial.fixed_text())
        }
        Err(error) => browser_mcp_error(format!("Quarry browser operation failed: {error}")),
    }
}

/// E3.2-e2: the agent label sent with a tab call (advisory, never authority).
const TAB_AGENT_LABEL: &str = "model-plane";
/// Most tabs one tab result lists.
const MAX_TAB_LIST: usize = 20;
/// Most characters of a tab title in a result.
const MAX_TAB_TITLE_CHARS: usize = 200;
/// Most characters of a tab URL in a result.
const MAX_TAB_URL_CHARS: usize = 300;
/// Longest URL a `tab_new` may name.
const MAX_TAB_NEW_URL_CHARS: usize = 2048;

/// E3.2-e2: one tab operation on a Quarry run.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TabOp {
    List,
    New { url: Option<String> },
    Select(String),
    Close(String),
}

async fn execute_tab_op(
    client: &crate::quarry_agent::QuarryAgentClient,
    op: TabOp,
    org_id: &str,
    quarry_run_id: &str,
    grant_id: &str,
    model_run_id: &str,
) -> ToolExecution {
    let advisory = crate::quarry_agent::TabAdvisory::new(model_run_id, TAB_AGENT_LABEL);
    let params = crate::quarry_agent::TabCallParams {
        org_id,
        run_id: quarry_run_id,
        advisory: &advisory,
        request_id: None,
    };
    let result = match op {
        TabOp::List => client
            .tab_state(org_id, quarry_run_id, grant_id, None)
            .await
            .map(|tabs| tabs_output(None, &tabs)),
        TabOp::New { url } => client
            .open_tab(params, url.as_deref())
            .await
            .map(|opened| tabs_output(Some(&opened.tab), &opened.tabs)),
        TabOp::Select(tab_id) => client
            .select_tab(params, &tab_id)
            .await
            .map(|tabs| tabs_output(None, &tabs)),
        TabOp::Close(tab_id) => client
            .close_tab(params, &tab_id)
            .await
            .map(|tabs| tabs_output(None, &tabs)),
    };
    match result {
        Ok(output) => ToolExecution {
            output,
            error: None,
        },
        Err(crate::quarry_agent::AgentClientError::PolicyDenied(denial)) => {
            browser_mcp_error(denial.fixed_text())
        }
        Err(error) => browser_mcp_error(format!("Quarry browser tab operation failed: {error}")),
    }
}

/// A tab result for the model: `{tab?, tabs:[{tab_id, title, url, active}], truncated}`.
/// Titles and URLs are page-authored, so they go through the Live scrub, the URL
/// without its fragment, and a length cut; at most [`MAX_TAB_LIST`] tabs.
fn tabs_output(opened: Option<&crate::quarry_agent::WireTab>, tabs: &[crate::quarry_agent::WireTab]) -> String {
    let entry = |tab: &crate::quarry_agent::WireTab| {
        let title: String = crate::scrub::scrub_live(tab.title.as_deref().unwrap_or_default())
            .chars()
            .take(MAX_TAB_TITLE_CHARS)
            .collect();
        let url: String = crate::scrub::scrub_url(
            tab.url
                .as_deref()
                .unwrap_or_default()
                .split_once('#')
                .map_or(tab.url.as_deref().unwrap_or_default(), |(head, _)| head),
        )
        .chars()
        .take(MAX_TAB_URL_CHARS)
        .collect();
        serde_json::json!({
            "tab_id": tab.tab_id,
            "title": title,
            "url": url,
            "active": tab.active,
        })
    };
    let mut output = serde_json::json!({
        "tabs": tabs.iter().take(MAX_TAB_LIST).map(&entry).collect::<Vec<_>>(),
        "truncated": tabs.len() > MAX_TAB_LIST,
    });
    if let Some(tab) = opened {
        output["tab"] = entry(tab);
    }
    output.to_string()
}

fn browser_mcp_error(error: impl Into<String>) -> ToolExecution {
    ToolExecution {
        output: String::new(),
        error: Some(error.into()),
    }
}

/// The refusal of a console read for a run the grant did not start. The same words answer a run that
/// cannot be read, so the answer does not say which runs exist.
const CONSOLE_NOT_GRANT_RUN: &str = "browser.console reads only the Quarry run its grant started";

/// `browser.console` (WS9.E, EP-1 D3-652): the console of a Quarry run, for the grant the call holds
/// and no other. The input names the run, so Quarry's own record of the run is read first, and it
/// must name this grant as the one the run was started with. A record that names another grant, or
/// one that cannot be read, is refused the same way, before any console is read.
async fn console_for_grant(
    client: &crate::quarry_agent::QuarryAgentClient,
    org_id: &str,
    quarry_run_id: &str,
    grant_id: &str,
    query: ConsoleQuery,
) -> ToolExecution {
    let bound = matches!(
        client.run_control(quarry_run_id, org_id).await,
        Ok(record) if record.grant_id == grant_id
    );
    if !bound {
        return browser_mcp_error(CONSOLE_NOT_GRANT_RUN);
    }
    match client
        .devtools(crate::quarry_agent::DevtoolsParams {
            org_id,
            run_id: quarry_run_id,
            levels: query.levels.as_deref(),
            filter: query.filter.as_deref(),
            after_sequence: query.since_sequence,
            limit: Some(query.limit),
            request_id: None,
        })
        .await
    {
        Ok(page) => ToolExecution {
            output: console_output(page),
            error: None,
        },
        Err(error) => browser_mcp_error(format!("Quarry browser console read failed: {error}")),
    }
}

/// A Do loop browser call (WS21-S5a): `browser.open`, `browser.observe` or `browser.close`, served
/// by the run's own browser session. The input is read strictly, so no grant, run or lease id can
/// be passed in, and the result carries none back.
pub(crate) async fn execute_browser_do_tool(
    tool_name: &str,
    tool_input: &str,
    dispatch: &browser_session::BrowserDispatch<'_>,
) -> ToolExecution {
    let quarry = crate::quarry_agent::QuarryAgentClient::from_env()
        .ok()
        .flatten();
    let outcome = match tool_name {
        browser_session::OPEN_TOOL => {
            match browser_do_input::<browser_session::OpenInput>(tool_input) {
                Err(refusal) => Err(refusal),
                Ok(input) => match quarry.as_ref() {
                    Some(quarry) => browser_session::open(dispatch, quarry, &input.url).await,
                    None => Err(browser_session::UNAVAILABLE),
                },
            }
        }
        browser_session::OBSERVE_TOOL => {
            match (
                browser_do_input::<browser_session::NoInput>(tool_input),
                quarry.as_ref(),
            ) {
                (Err(refusal), _) => Err(refusal),
                (Ok(_), None) => Err(browser_session::UNAVAILABLE),
                (Ok(_), Some(quarry)) => browser_session::observe(dispatch, quarry).await,
            }
        }
        browser_session::CLOSE_TOOL => {
            match browser_do_input::<browser_session::NoInput>(tool_input) {
                Err(refusal) => Err(refusal),
                Ok(_) => Ok(browser_session::close(dispatch, quarry.as_ref()).await),
            }
        }
        browser_session::ACT_TOOL => match quarry.as_ref() {
            Some(quarry) => browser_do_act_run(dispatch, quarry, tool_input).await,
            None => Err(browser_session::UNAVAILABLE),
        },
        _ => Err(browser_session::UNAVAILABLE),
    };
    match outcome {
        Ok(output) => ToolExecution {
            output,
            error: None,
        },
        Err(refusal) => ToolExecution {
            output: String::new(),
            error: Some(refusal.text()),
        },
    }
}

/// Reads a Do browser tool's input. An empty input is an empty object, and a field the tool does
/// not name is refused.
fn browser_do_input<T: serde::de::DeserializeOwned>(
    tool_input: &str,
) -> Result<T, browser_session::Refusal> {
    let input = if tool_input.trim().is_empty() {
        "{}"
    } else {
        tool_input
    };
    serde_json::from_str(input).map_err(|_| browser_session::INVALID_INPUT)
}

/// The refusal of a `browser.act` on a page this run holds at the read level (WS21-S5b). The page was
/// opened to be read, and no approval the person gives changes that.
const ACT_NOT_PERMITTED: browser_session::Refusal = browser_session::Refusal {
    code: "browser_act_not_permitted",
    message: "This page was opened for reading only, so this run cannot act on it.",
};

/// The refusal of a `browser.act` that needs an approval this run cannot request for itself: a dialog
/// answer, an upload or a download.
const ACT_NOT_AVAILABLE: browser_session::Refusal = browser_session::Refusal {
    code: "browser_action_not_available",
    message: "This action needs an approval this run cannot request. Ask the person to do it themselves.",
};

/// The refusal of an act the browser did not perform on the page.
const ACT_REFUSED: browser_session::Refusal = browser_session::Refusal {
    code: "browser_action_refused",
    message: "The browser did not perform that action on the page.",
};

/// The budget of the Quarry run an act is sent under: the one a session's own steps run under.
const ACT_MAX_STEPS: u32 = 32;
const ACT_MAX_RUNTIME_SECONDS: u32 = 15 * 60;

/// The input of `browser.act`: one action on an element of the page this run has open, named by the
/// reference the last observation gave it. The page's ids stay with the session, so none is named.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserDoActInput {
    action: BrowserMcpAction,
}

/// The Quarry action a `browser.act` input names. A dialog answer, an upload and a download are refused
/// here: each needs an approval that only the person can give.
fn browser_do_act(
    tool_input: &str,
) -> Result<crate::quarry_agent::AgentAction, browser_session::Refusal> {
    use crate::quarry_agent::AgentAction;

    let input: BrowserDoActInput = browser_do_input(tool_input)?;
    let action = input
        .action
        .into_quarry_action()
        .map_err(|_| browser_session::INVALID_INPUT)?;
    match action {
        AgentAction::RespondDialog { .. }
        | AgentAction::UploadRef { .. }
        | AgentAction::FrameUploadRef { .. }
        | AgentAction::DownloadRef { .. }
        | AgentAction::FrameDownloadRef { .. } => Err(ACT_NOT_AVAILABLE),
        action => Ok(action),
    }
}

/// The refusal for a BrowserBroker failure, in the words the session gives it. The session keeps its
/// own mapping private, so an act carries this one.
fn broker_refusal(error: browser_session::BrokerError) -> browser_session::Refusal {
    use browser_session::BrokerError;

    match error {
        BrokerError::SiteDenied => browser_session::SITE_DENIED,
        BrokerError::CredentialRefused => browser_session::CREDENTIAL_EXPIRED,
        BrokerError::Unavailable => browser_session::UNAVAILABLE,
        BrokerError::Refused => browser_session::GRANT_REFUSED,
    }
}

/// The refusal for a Quarry failure on an act. It follows the session's mapping for its own steps.
fn act_refusal(error: &crate::quarry_agent::AgentClientError) -> browser_session::Refusal {
    use crate::quarry_agent::AgentClientError;

    match error {
        AgentClientError::Challenge(_) => browser_session::BLOCKED,
        AgentClientError::Status {
            status: 404 | 410, ..
        } => browser_session::SESSION_LOST,
        AgentClientError::Typed(typed) if matches!(typed.status, 404 | 410) => {
            browser_session::SESSION_LOST
        }
        AgentClientError::Transport(_) | AgentClientError::Authentication(_) => {
            browser_session::UNAVAILABLE
        }
        // Quarry's web policy refused the act: the same refusal as a policy denial in the observation.
        AgentClientError::PolicyDenied(_) => ACT_REFUSED,
        AgentClientError::Status { .. }
        | AgentClientError::Typed(_)
        | AgentClientError::Decode(_) => browser_session::SESSION_FAILED,
    }
}

/// What a `browser.act` must pass before anyone is asked to approve it or Quarry is sent it (WS21-S5b):
/// the input is a permitted action, the run holds a page, and the broker still reads that page's grant
/// at the act level. A read-level grant is refused here, so nobody is asked to approve an act the grant
/// cannot make.
pub(crate) async fn browser_act_precheck(
    dispatch: &browser_session::BrowserDispatch<'_>,
    tool_input: &str,
) -> Result<(), browser_session::Refusal> {
    browser_do_act(tool_input)?;
    let session = dispatch
        .state
        .browser_session(dispatch.run_id)
        .ok_or(browser_session::NOT_OPEN)?;
    let answer = dispatch
        .broker
        .validate_grant(dispatch.bearer, &session.grant_id)
        .await
        .map_err(broker_refusal)?;
    let grant = ValidatedBrowserGrant::from_broker_response(&session.grant_id, &answer)
        .map_err(|_| browser_session::GRANT_REFUSED)?;
    if grant.read_only() {
        return Err(ACT_NOT_PERMITTED);
    }
    Ok(())
}

/// `browser.act` (WS21-S5b): the precheck, one Quarry action on the page, then the page as Quarry reads
/// it now, which is what the model acts on next. The page is read through the session's own
/// observation, so an act that takes the page out of its fence ends the session as an observation does.
async fn browser_do_act_run(
    dispatch: &browser_session::BrowserDispatch<'_>,
    quarry: &crate::quarry_agent::QuarryAgentClient,
    tool_input: &str,
) -> Result<String, browser_session::Refusal> {
    use crate::quarry_agent::AgentConstraints;

    browser_act_precheck(dispatch, tool_input).await?;
    let action = browser_do_act(tool_input)?;
    let session = dispatch
        .state
        .browser_session(dispatch.run_id)
        .ok_or(browser_session::NOT_OPEN)?;
    let constraints = AgentConstraints {
        max_steps: ACT_MAX_STEPS,
        allowed_domains: session.fence.clone(),
        max_runtime_s: Some(ACT_MAX_RUNTIME_SECONDS),
        max_cost_usd: None,
    };
    let acted = quarry
        .step(crate::quarry_agent::StepParams::new(
            &session.quarry_run_id,
            &session.lease_id,
            dispatch.org_id,
            action,
            &constraints,
            false,
            &session.grant_id,
        ))
        .await
        .map_err(|error| act_refusal(&error))?;
    if acted.challenge.is_some() {
        return Err(browser_session::BLOCKED);
    }
    if !acted.policy_denials.is_empty() {
        return Err(ACT_REFUSED);
    }
    browser_session::observe(dispatch, quarry).await
}

/// The risk category a `browser.act` is held under (WS21-S5b), read from its input alone. The labels of
/// the elements its references name are not in the input, so a reference-bound act has no evidence to
/// judge until the session keeps the page it was chosen from. Until then the category is `None`.
pub(crate) fn browser_act_risk_category(tool_input: &str) -> Option<&'static str> {
    let action = browser_do_act(tool_input).ok()?;
    crate::browser_agent::RiskCategory::of_wire_action(&action, None)
        .map(crate::browser_agent::RiskCategory::as_str)
}

/// The answer to a Do browser call this run cannot be served: it holds no verified browser sign-in,
/// or it is zero-retention, under which the browser tools are refused by their feature key.
pub(crate) fn browser_do_unavailable(zdr: bool) -> ToolExecution {
    let error = if zdr {
        let refusal = mp_contracts::zdr_refusal::Refusal::feature_not_supported("browser_tool");
        format!("{}: {}", refusal.code(), refusal.message())
    } else {
        "browser_unavailable: browser tools need a verified browser sign-in for this run".to_owned()
    };
    ToolExecution {
        output: String::new(),
        error: Some(error),
    }
}

/// A computer call (W4-C C1), answered with the text [`computer_action::refusal_text`] gives: no computer
/// backend serves it in this slice.
pub(crate) fn computer_tool(tool_name: &str, tool_input: &str, zdr: bool) -> ToolExecution {
    ToolExecution {
        output: String::new(),
        error: Some(computer_action::refusal_text(
            tool_name,
            tool_input,
            zdr,
            computer_action::computer_enabled(),
        )),
    }
}

const MAX_BROWSER_MCP_TEXT_BYTES: usize = 64 * 1024;
const MAX_BROWSER_MCP_WAIT_MS: u32 = 30_000;

/// WS9.E: most console entries one `browser.console` call returns.
const MAX_CONSOLE_ENTRIES: usize = 100;
/// Entries asked for when the model names no limit.
const DEFAULT_CONSOLE_LIMIT: u32 = 50;
/// Most bytes of entries in one result (the envelope is on top of this).
const MAX_CONSOLE_OUTPUT_BYTES: usize = 16 * 1024;
/// Most characters of one entry's text.
const MAX_CONSOLE_TEXT_CHARS: usize = 500;
/// Longest substring filter, in characters (Quarry's own limit).
const MAX_CONSOLE_FILTER_CHARS: usize = 200;
/// Most levels one call may name (Quarry's own limit).
const MAX_CONSOLE_LEVELS: usize = 5;
/// Longest Quarry run id the console path puts in a URL path.
const MAX_CONSOLE_RUN_ID_CHARS: usize = 128;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserMcpObserveInput {
    grant_id: String,
    quarry_run_id: String,
    lease_id: String,
}

/// `browser.console` input (WS9.E). Strict: an unknown key is an error, so a
/// model cannot ask for `payload`, a category or a tab filter that is not here.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserMcpConsoleInput {
    grant_id: String,
    quarry_run_id: String,
    lease_id: String,
    /// `debug`, `info`, `log`, `warn`, `warning`, `error`, `verbose`; at most five.
    #[serde(default)]
    levels: Option<Vec<String>>,
    /// Case-insensitive substring over the console text (Quarry matches it).
    #[serde(default)]
    filter: Option<String>,
    /// Clamped to 1..=100.
    #[serde(default)]
    limit: Option<i64>,
    /// Continue after this sequence (the previous result's `next_sequence`).
    #[serde(default)]
    since_sequence: Option<u64>,
}

/// A validated console read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConsoleQuery {
    levels: Option<String>,
    filter: Option<String>,
    limit: u32,
    since_sequence: Option<u64>,
}

impl BrowserMcpConsoleInput {
    fn into_query(self) -> Result<ConsoleQuery, String> {
        let mut levels: Vec<String> = Vec::new();
        for raw in self.levels.unwrap_or_default() {
            let level = raw.trim().to_ascii_lowercase();
            if level.is_empty() {
                continue;
            }
            if !matches!(
                level.as_str(),
                "debug" | "verbose" | "info" | "log" | "warn" | "warning" | "error"
            ) {
                return Err(
                    "invalid browser.console input: levels must be from debug, info, log, warn, warning, error, verbose"
                        .to_owned(),
                );
            }
            if !levels.contains(&level) {
                levels.push(level);
            }
        }
        if levels.len() > MAX_CONSOLE_LEVELS {
            return Err(format!(
                "invalid browser.console input: at most {MAX_CONSOLE_LEVELS} levels"
            ));
        }
        let filter = match self.filter.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(text) if text.chars().any(char::is_control) => {
                return Err("invalid browser.console input: filter has a control character".to_owned());
            }
            Some(text) if text.chars().count() > MAX_CONSOLE_FILTER_CHARS => {
                return Err(format!(
                    "invalid browser.console input: filter is longer than {MAX_CONSOLE_FILTER_CHARS} characters"
                ));
            }
            Some(text) => Some(text.to_owned()),
        };
        let limit = self
            .limit
            .map_or(DEFAULT_CONSOLE_LIMIT, |limit| {
                u32::try_from(limit.clamp(1, MAX_CONSOLE_ENTRIES as i64))
                    .unwrap_or(DEFAULT_CONSOLE_LIMIT)
            });
        Ok(ConsoleQuery {
            levels: (!levels.is_empty()).then(|| levels.join(",")),
            filter,
            limit,
            since_sequence: self.since_sequence.filter(|sequence| *sequence > 0),
        })
    }
}

/// The result of a console read (WS9.E): `{entries:[{sequence, tab_id, level,
/// text}], next_sequence, truncated}`. At most 100 entries and 16 KB of
/// entries; each text is scrubbed (the Live set, before the cut so a secret
/// that straddles it is masked whole) and capped at 500 characters. Nothing
/// but those four fields is read from Quarry's event, so `payload` cannot be
/// here. `truncated` is true when an entry was dropped or a text was cut;
/// `next_sequence` then points after the last entry that was kept, so the
/// model can ask for the rest.
fn console_output(page: crate::quarry_agent::WireDevtoolsPage) -> String {
    // Room for the envelope around the entries.
    const ENVELOPE_RESERVE_BYTES: usize = 128;
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    let mut dropped = false;
    let mut last_kept = None;
    for event in page.events {
        let scrubbed = crate::scrub::scrub_live(event.text.as_deref().unwrap_or(""));
        let (text, cut) = match scrubbed.char_indices().nth(MAX_CONSOLE_TEXT_CHARS) {
            Some((index, _)) => (&scrubbed[..index], true),
            None => (scrubbed.as_str(), false),
        };
        let entry = serde_json::json!({
            "sequence": event.sequence,
            "tab_id": event.tab_id.as_deref().map(|id| id.chars().take(64).collect::<String>()),
            "level": event.level.as_deref().map(|level| level.chars().take(16).collect::<String>()),
            "text": text,
        });
        let size = entry.to_string().len() + 1;
        if entries.len() >= MAX_CONSOLE_ENTRIES
            || bytes + size + ENVELOPE_RESERVE_BYTES > MAX_CONSOLE_OUTPUT_BYTES
        {
            dropped = true;
            break;
        }
        bytes += size;
        truncated |= cut;
        last_kept = Some(event.sequence);
        entries.push(entry);
    }
    let next_sequence = match (dropped, last_kept) {
        (true, Some(last)) => last,
        _ => page.next_sequence,
    };
    serde_json::json!({
        "entries": entries,
        "next_sequence": next_sequence,
        "truncated": truncated || dropped,
    })
    .to_string()
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
    /// E3.2-e2: list the run's tabs. A struct variant, not a unit one: serde's
    /// `deny_unknown_fields` does not reject extra keys on an internally tagged unit variant.
    TabList {},
    /// E3.2-e2: open a tab, at a URL the grant's domains cover or blank.
    TabNew { url: Option<String> },
    /// E3.2-e2: make a tab the active one.
    TabSelect { tab_id: String },
    /// E3.2-e2: close a tab.
    TabClose { tab_id: String },
}

/// What a browser MCP call asks Quarry to do.
enum BrowserMcpKind {
    /// Read the page (`get_content`).
    Observe,
    /// One governed action.
    Act(crate::quarry_agent::AgentAction),
    /// Read the run's console (WS9.E).
    Console(ConsoleQuery),
    /// A tab operation (E3.2-e2): list, open, select or close.
    Tabs(TabOp),
}

struct BrowserMcpRequest {
    quarry_run_id: String,
    lease_id: String,
    kind: BrowserMcpKind,
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
                    kind: BrowserMcpKind::Observe,
                })
            }
            "browser.console" => {
                let input: BrowserMcpConsoleInput = serde_json::from_str(tool_input)
                    .map_err(|error| format!("invalid browser.console input: {error}"))?;
                validate_browser_mcp_scope(
                    &input.grant_id,
                    &input.quarry_run_id,
                    &input.lease_id,
                    validated_grant,
                )?;
                // The run id is put in a URL path: a Quarry id, nothing else.
                let run_id = input.quarry_run_id.trim();
                if run_id.chars().count() > MAX_CONSOLE_RUN_ID_CHARS
                    || !run_id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
                {
                    return Err("invalid browser.console input: quarry_run_id is not a Quarry run id".to_owned());
                }
                let run_id = run_id.to_owned();
                let lease_id = input.lease_id.clone();
                Ok(Self {
                    quarry_run_id: run_id,
                    lease_id,
                    kind: BrowserMcpKind::Console(input.into_query()?),
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
                    kind: input.action.into_kind(validated_grant.allowed_domains())?,
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

/// A Quarry tab id as the driver reports it (`[A-Za-z0-9_-]{1,64}`); it goes in a
/// URL path, so nothing else may pass.
fn valid_tab_id(tab_id: &str) -> Result<String, String> {
    let tab_id = tab_id.trim();
    if !tab_id.is_empty()
        && tab_id.len() <= 64
        && tab_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        Ok(tab_id.to_owned())
    } else {
        Err("invalid browser.act input: tab_id is not a Quarry tab id".to_owned())
    }
}

impl BrowserMcpAction {
    /// The request kind for this action: a tab operation, or a governed step.
    fn into_kind(self, allowed_domains: &[String]) -> Result<BrowserMcpKind, String> {
        match self {
            Self::TabList {} => Ok(BrowserMcpKind::Tabs(TabOp::List)),
            Self::TabNew { url } => {
                let url = url.map(|url| url.trim().to_owned()).filter(|url| !url.is_empty());
                if let Some(url) = &url {
                    if url.chars().count() > MAX_TAB_NEW_URL_CHARS
                        || !crate::browser_agent::url_allowed_by(url, allowed_domains)
                    {
                        return Err(
                            "invalid browser.act input: tab_new url is not an http(s) URL on the grant's allowed domains"
                                .to_owned(),
                        );
                    }
                }
                Ok(BrowserMcpKind::Tabs(TabOp::New { url }))
            }
            Self::TabSelect { tab_id } => Ok(BrowserMcpKind::Tabs(TabOp::Select(valid_tab_id(&tab_id)?))),
            Self::TabClose { tab_id } => Ok(BrowserMcpKind::Tabs(TabOp::Close(valid_tab_id(&tab_id)?))),
            other => Ok(BrowserMcpKind::Act(other.into_quarry_action()?)),
        }
    }

    fn into_quarry_action(self) -> Result<crate::quarry_agent::AgentAction, String> {
        use crate::quarry_agent::AgentAction;

        match self {
            Self::TabList {} | Self::TabNew { .. } | Self::TabSelect { .. } | Self::TabClose { .. } => {
                Err("tab operations are not step actions".to_owned())
            }
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

/// The closed `outcome` metric label for how a browser tool call ended.
fn outcome_label(execution: &BrowserAgentExecution) -> &'static str {
    match execution {
        BrowserAgentExecution::Completed { .. } => "completed",
        BrowserAgentExecution::Failed { .. } => "failed",
        BrowserAgentExecution::PermissionDenied { .. } => "permission_denied",
        BrowserAgentExecution::TimedOut { .. } => "timed_out",
        BrowserAgentExecution::ResourceExhausted { .. } => "resource_exhausted",
        BrowserAgentExecution::Cancelled { .. } => "cancelled",
        BrowserAgentExecution::Aborted { .. } => "aborted",
        BrowserAgentExecution::AwaitingApproval => "awaiting_approval",
    }
}

/// Log the one-line startup banner for the browser planner (mode in force,
/// allowlist size, procedure, limits; no org ids, no secrets). A `default_on`
/// request without its acknowledgement is downgraded, with one error line.
/// Called once from `main`.
pub fn log_browser_planner_banner() {
    // `from_env` logs the downgrade error (once per process) and the legacy
    // variable's deprecation warning.
    let policy = crate::planner_policy::PlannerPolicy::from_env();
    tracing::info!("{}", policy.banner(&PlanCeilings::from_env().banner_facts()));
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
    /// `EXECUTION_CORE_REQUIRE_QUARRY_REDACTION_STAMP` (WS12.S4), default off.
    require_quarry_redaction_stamp: bool,
    /// `QUARRY_BROWSER_AGENT_APPROVAL_REDIRECT` (E3.2-c), default off.
    approval_redirect: bool,
    /// `QUARRY_BROWSER_AGENT_STEER` (E3.3-f), default off.
    steer: bool,
    /// `QUARRY_BROWSER_AGENT_OBSERVATION_QUALITY` (E3.3-l), default off.
    observation_quality: bool,
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
            require_quarry_redaction_stamp: false,
            approval_redirect: false,
            steer: false,
            observation_quality: false,
        }
    }
}

impl PlanCeilings {
    pub(crate) fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// The numbers the startup banner prints.
    pub(crate) fn banner_facts(&self) -> crate::planner_policy::BannerFacts {
        crate::planner_policy::BannerFacts {
            procedure: self.procedure.as_str().to_owned(),
            widened_actions: self.widened_actions,
            default_steps: self.default_steps,
            ceiling_steps: self.ceiling_steps,
            default_runtime_s: self.default_runtime_s,
            ceiling_runtime_s: self.ceiling_runtime_s,
            planner_tokens_default: self.default_planner_tokens,
            planner_tokens_ceiling: self.ceiling_planner_tokens,
            max_consecutive_failures: self.max_consecutive_failures,
            max_total_failures: self.max_total_failures,
        }
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
            require_quarry_redaction_stamp: lookup(
                "EXECUTION_CORE_REQUIRE_QUARRY_REDACTION_STAMP",
            )
            .is_some_and(|value| matches!(value.trim(), "1" | "true" | "on")),
            approval_redirect: lookup("QUARRY_BROWSER_AGENT_APPROVAL_REDIRECT")
                .is_some_and(|value| matches!(value.trim(), "1" | "true" | "on")),
            steer: browser_agent::steer_enabled_from(
                lookup(browser_agent::STEER_ENV).as_deref(),
            ),
            observation_quality: lookup("QUARRY_BROWSER_AGENT_OBSERVATION_QUALITY")
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
    /// Tighten-only request ask (WS7.L): capabilities to disable and hosts to
    /// deny or allow for this run. The only way a Space or caller restriction
    /// reaches Quarry. `robots_max`, `proxy_max` and any unknown key fail the
    /// input (the type is `deny_unknown_fields`), never get dropped.
    policy: Option<RunPolicyInput>,
    /// Plan mode: unions the four write-capable capabilities into the ask.
    plan_mode: Option<bool>,
    /// Correlation id for every Quarry call of the run (WS10.S4). A value that
    /// is not a trace token is dropped.
    request_id: Option<String>,
    /// Trace context (WS10.S4): `origin` and `agent` labels. The run id is the
    /// verified Model run id, never read from here.
    trace: Option<crate::quarry_agent::WireRunTrace>,
    /// WS8.G attach mode: the person's Quarry run the agent attaches to. Set
    /// by the gateway together with a derived `attach` grant; when present the
    /// run never acquires a lease of its own.
    #[serde(default)]
    attach_session_id: Option<String>,
    #[serde(default)]
    attach_tab: Option<crate::quarry_agent::AttachTabTarget>,
    /// WS8.J: the mark an attached run puts on its tab when it completes
    /// (`none`, `deliverable`, `handoff`; absent = `deliverable`). A value
    /// outside the set fails the input. Meaningless without `attach_session_id`.
    #[serde(default)]
    tab_mark_on_complete: Option<crate::quarry_agent::TabMark>,
    /// A saved recipe the run replays (Q4-E4.2-3h). The gateway writes it from the person's request and
    /// verified claims (3i); `for_user_id` must be the person the run's grant was issued to. Absent, the
    /// run is an ordinary browser run.
    #[serde(default)]
    recipe: Option<RecipeInput>,
}

/// A saved recipe a run replays (Q4-E4.2-3h), read strictly: an unknown key fails the input.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeInput {
    /// The saved procedure's identifier: "proc_" and a UUID.
    id: String,
    /// The version to replay. Zero or absent reads the tag named, or the newest version when none is.
    #[serde(default)]
    version: Option<u32>,
    /// The tag to replay when no version is named.
    #[serde(default)]
    tag: Option<String>,
    /// The values a person supplied for the recipe's typed steps, by slot name.
    #[serde(default)]
    slots: BTreeMap<String, String>,
    /// The person the procedure belongs to, written by the gateway from the verified claims.
    for_user_id: String,
}

/// The three keys of [`crate::quarry_agent::WireRequestAsk`], strictly.
#[derive(serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RunPolicyInput {
    #[serde(default)]
    disable: Vec<String>,
    #[serde(default)]
    deny_domains: Vec<String>,
    #[serde(default)]
    allow_domains: Option<Vec<String>>,
}

/// The ask the run sends: plan mode's disables unioned with the caller's ask,
/// tighten-only. `None` when neither asks for anything.
fn effective_policy(
    plan_mode: Option<bool>,
    policy: Option<RunPolicyInput>,
) -> Option<crate::quarry_agent::WireRequestAsk> {
    use crate::quarry_agent::WireRequestAsk;
    let from_caller = policy.map(|policy| WireRequestAsk {
        disable: policy.disable,
        deny_domains: policy.deny_domains,
        allow_domains: policy.allow_domains,
    });
    let ask = match (plan_mode.unwrap_or(false), from_caller) {
        (true, Some(caller)) => WireRequestAsk::plan_mode().tightened_with(&caller),
        (true, None) => WireRequestAsk::plan_mode(),
        (false, Some(caller)) => caller.tightened_with(&WireRequestAsk::default()),
        (false, None) => return None,
    };
    (!ask.is_empty()).then_some(ask)
}

/// The trace the run sends: the caller's labels with the verified Model run id.
/// `None` when the input named no trace, so a caller that sends none changes
/// nothing on the wire.
fn effective_trace(
    trace: Option<crate::quarry_agent::WireRunTrace>,
    run_id: &str,
) -> Option<crate::quarry_agent::WireRunTrace> {
    let mut trace = trace?;
    trace.run_id = Some(run_id.trim().to_owned()).filter(|id| !id.is_empty());
    trace.sanitized()
}

/// The broker's kind for a grant derived with `purpose=attach` (WS8.C).
const ATTACH_GRANT_KIND: &str = "attach";

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
    let run_id = input.run_id.unwrap_or_default();
    // Attach mode needs an attach grant, and an attach grant drives nothing
    // but an attached run (the broker scoped it to the person's run).
    let attach_session_id = input
        .attach_session_id
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty());
    let attach_grant = validated_grant.kind() == ATTACH_GRANT_KIND;
    if input.attach_tab.is_some() && (attach_session_id.is_none() || input.start_url.is_some()) {
        return Err("continuing in a browser tab requires an attached run without a start URL".to_owned());
    }
    let attach = match (attach_session_id, attach_grant) {
        (Some(session_run_id), true) => Some(browser_agent::AttachConfig {
            session_run_id,
            target_tab: input.attach_tab,
        }),
        (None, false) => None,
        (Some(_), false) => {
            return Err("browser attach requires a broker-derived attach grant".to_owned());
        }
        (None, true) => {
            return Err("an attach grant can only drive an attached browser run".to_owned());
        }
    };
    // An attached run uses the person's own session and cookie jar, never a
    // persistent profile of its own.
    let profile_id = if attach.is_some() { None } else { input.profile_id };
    let input_tab_mark = input.tab_mark_on_complete;
    let plan = browser_agent::PlanConfig {
        plan_id: input.plan_id.unwrap_or_else(mp_ids::new_ulid),
        grant_id: validated_grant.grant_id().to_owned(),
        policy: effective_policy(input.plan_mode, input.policy),
        trace: effective_trace(input.trace, &run_id),
        request_id: input
            .request_id
            .filter(|id| crate::quarry_agent::is_trace_token(id)),
        run_id,
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
        zdr: verified_zdr || validated_grant.zdr(),
        profile_id,
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
        attach,
        // D5 default (600 s, capped by the runtime in the loop); the gateway
        // sends no override.
        takeover_wait_s: 0,
        takeover_poll: None,
        require_quarry_redaction_stamp: ceilings.require_quarry_redaction_stamp,
        tab_mark_on_complete: input_tab_mark.unwrap_or_default(),
        approval_redirect: ceilings.approval_redirect,
        steer: ceilings.steer,
        observation_quality: ceilings.observation_quality,
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

    /// A Do browser input is read strictly: a grant, run or lease id named in it is refused, so the
    /// model cannot pick a session. The empty input is the empty object.
    #[test]
    fn a_do_browser_input_names_no_grant_run_or_lease() {
        use crate::browser_session::{NoInput, OpenInput, INVALID_INPUT};
        assert!(browser_do_input::<NoInput>("").is_ok());
        assert!(browser_do_input::<NoInput>("{}").is_ok());
        for forged in [
            r#"{"grant_id":"grant-1"}"#,
            r#"{"quarry_run_id":"run-1","lease_id":"lease-1"}"#,
        ] {
            assert_eq!(browser_do_input::<NoInput>(forged).err(), Some(INVALID_INPUT), "{forged}");
        }
        assert!(browser_do_input::<OpenInput>(r#"{"url":"https://example.com/"}"#).is_ok());
        assert_eq!(
            browser_do_input::<OpenInput>(r#"{"url":"https://example.com/","grant_id":"g"}"#).err(),
            Some(INVALID_INPUT)
        );
        assert_eq!(browser_do_input::<OpenInput>("{}").err(), Some(INVALID_INPUT));
    }

    /// Under zero retention a Do browser call is refused with the feature it is refused for, and
    /// with no sign-in it is refused as unavailable. Neither names an id.
    #[test]
    fn a_do_browser_call_without_a_session_is_refused_typed() {
        let zdr = browser_do_unavailable(true).error.expect("refused");
        assert!(zdr.starts_with("zdr_feature_not_supported: "), "{zdr}");
        let signed_out = browser_do_unavailable(false).error.expect("refused");
        assert!(signed_out.starts_with("browser_unavailable: "), "{signed_out}");
    }

    #[test]
    fn legacy_attach_refuses_when_broker_grant_is_zdr() {
        let input: BrowserAgentInput = serde_json::from_str(
            r#"{"grant_id":"grant-att","attach_session_id":"session-1","max_steps":3}"#,
        ).unwrap();
        let grant = ValidatedBrowserGrant::for_test("grant-att", "attach", "parent", "user-1", true);
        assert!(browser_plan_config(input, "org-1", false, &grant).is_err(),
            "broker ZDR cannot be downgraded by a non-ZDR request");
    }

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
    fn broker_response_carries_the_read_level() {
        let response = |read_only| ValidateGrantResponse {
            grant_id: "grant-1".to_owned(),
            active: true,
            allowed_domains: vec!["example.com".to_owned()],
            read_only,
            ..Default::default()
        };
        let read = ValidatedBrowserGrant::from_broker_response("grant-1", &response(true))
            .expect("a read-level grant validates");
        let act = ValidatedBrowserGrant::from_broker_response("grant-1", &response(false))
            .expect("an act-level grant validates");
        assert!(read.read_only());
        assert!(!act.read_only());
    }

    /// A BrowserBroker for the act tests: it answers the grant at the level the test chose and counts
    /// how often the grant was read back.
    struct ActBroker {
        read_only: bool,
        validations: std::sync::Mutex<u32>,
    }

    #[tonic::async_trait]
    impl browser_session::BrowserBroker for ActBroker {
        async fn acquire_run_grant(
            &self,
            _bearer: &str,
            _grant: browser_session::RunGrant<'_>,
        ) -> Result<String, browser_session::BrokerError> {
            Err(browser_session::BrokerError::Refused)
        }

        async fn validate_grant(
            &self,
            _bearer: &str,
            grant_id: &str,
        ) -> Result<ValidateGrantResponse, browser_session::BrokerError> {
            *self.validations.lock().unwrap() += 1;
            Ok(ValidateGrantResponse {
                grant_id: grant_id.to_owned(),
                active: true,
                allowed_domains: vec!["example.com".to_owned()],
                read_only: self.read_only,
                ..Default::default()
            })
        }

        async fn revoke_grant(
            &self,
            _bearer: &str,
            _grant_id: &str,
            _reason: &str,
        ) -> Result<(), browser_session::BrokerError> {
            Ok(())
        }
    }

    const CLICK_REF: &str =
        r#"{"action":{"type":"click_ref","snapshot_id":"snap-1","generation":3,"ref_id":"@e1"}}"#;

    /// A run holding one page on example.com, as a session's open leaves it.
    fn holding_a_page(state: &crate::state::StateStore) {
        state.replace_browser_session(
            "run-1",
            browser_session::BrowserSession {
                grant_id: "grant-1".to_owned(),
                quarry_run_id: "run-q1".to_owned(),
                lease_id: "lease-q1".to_owned(),
                host: "example.com".to_owned(),
                fence: vec!["example.com".to_owned()],
            },
        );
    }

    fn act_dispatch<'a>(
        state: &'a crate::state::StateStore,
        broker: &'a ActBroker,
    ) -> browser_session::BrowserDispatch<'a> {
        browser_session::BrowserDispatch {
            state,
            broker,
            bearer: "bearer-1",
            org_id: "org-1",
            run_id: "run-1",
        }
    }

    /// Quarry's answer to one step, the shape `/step` returns, on `url`.
    fn step_answer(url: &str) -> serde_json::Value {
        serde_json::json!({
            "run_id": "run-q1",
            "step": 2,
            "url": url,
            "title": "Example",
            "snapshot": {
                "snapshot_id": "snap-2",
                "generation": 4,
                "targets": [{
                    "ref_id": "@e1",
                    "role": "button",
                    "name": "Buy",
                    "text": "Buy",
                    "frame_id": null
                }],
                "frames": []
            },
            "observed_at": "2026-10-31T00:00:00Z"
        })
    }

    fn step_envelope(data: serde_json::Value) -> wiremock::ResponseTemplate {
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": data,
            "meta": { "request_id": "req_1" },
            "error": null,
        }))
    }

    /// The act input names no grant, run or lease, and a dialog answer, an upload or a download is
    /// refused: each needs an approval only the person can give.
    #[test]
    fn act_input_names_no_id_and_refuses_what_needs_a_separate_approval() {
        assert!(browser_do_act(CLICK_REF).is_ok());
        assert!(browser_do_act(
            r#"{"action":{"type":"select_ref","snapshot_id":"snap-1","generation":3,"ref_id":"@e1","value":"no"}}"#
        )
        .is_ok());
        for forged in [
            r#"{"grant_id":"grant-1","action":{"type":"click_ref","snapshot_id":"snap-1","generation":3,"ref_id":"@e1"}}"#,
            r#"{"action":{"type":"click_ref","snapshot_id":"snap-1","generation":3,"ref_id":"@e1","grant_id":"grant-1"}}"#,
            r#"{"action":{"type":"click_ref","snapshot_id":"snap-1","generation":3,"ref_id":"@e1"},"lease_id":"lease-1"}"#,
            "",
        ] {
            assert_eq!(
                browser_do_act(forged).err(),
                Some(browser_session::INVALID_INPUT),
                "{forged}"
            );
        }
        for needs_approval in [
            r#"{"action":{"type":"respond_dialog","dialog_id":"dialog-1","accept":true,"approval_grant_id":"approval-1"}}"#,
            r#"{"action":{"type":"download_ref","snapshot_id":"snap-1","generation":3,"ref_id":"@e1","approval_grant_id":"approval-1"}}"#,
        ] {
            assert_eq!(
                browser_do_act(needs_approval).err(),
                Some(ACT_NOT_AVAILABLE),
                "{needs_approval}"
            );
        }
    }

    /// A read-level grant refuses an act before the approval is asked and before Quarry is sent
    /// anything: the step mock below expects no call at all.
    #[tokio::test]
    async fn act_on_read_only_grant_refused() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/agent/runs/run-q1/step"))
            .respond_with(step_envelope(step_answer("https://example.com/")))
            .expect(0)
            .mount(&server)
            .await;
        let state = crate::state::StateStore::new();
        holding_a_page(&state);
        let broker = ActBroker {
            read_only: true,
            validations: std::sync::Mutex::new(0),
        };
        let quarry = crate::quarry_agent::QuarryAgentClient::new(server.uri(), "tok")
            .expect("a client for the stub");

        let precheck = browser_act_precheck(&act_dispatch(&state, &broker), CLICK_REF).await;
        assert_eq!(precheck.err(), Some(ACT_NOT_PERMITTED));
        let act = browser_do_act_run(&act_dispatch(&state, &broker), &quarry, CLICK_REF).await;
        assert_eq!(act.err(), Some(ACT_NOT_PERMITTED));
        assert_eq!(*broker.validations.lock().unwrap(), 2, "the grant was read at each check");
    }

    /// An act with no page open is refused before the broker is asked anything.
    #[tokio::test]
    async fn an_act_without_a_page_is_refused_before_the_broker() {
        let state = crate::state::StateStore::new();
        let broker = ActBroker {
            read_only: false,
            validations: std::sync::Mutex::new(0),
        };
        let refusal = browser_act_precheck(&act_dispatch(&state, &broker), CLICK_REF).await;
        assert_eq!(refusal.err(), Some(browser_session::NOT_OPEN));
        assert_eq!(*broker.validations.lock().unwrap(), 0);
    }

    /// On an act-level grant an act is one Quarry action on the page, and the page is then read back
    /// through the session, so the model reads what its act did. No id reaches the model.
    #[tokio::test]
    async fn an_act_on_an_act_grant_sends_one_action_then_reads_the_page() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/agent/runs/run-q1/step"))
            .and(wiremock::matchers::body_partial_json(serde_json::json!({
                "action": {
                    "type": "click_ref",
                    "snapshot_id": "snap-1",
                    "generation": 3,
                    "ref_id": "@e1"
                }
            })))
            .respond_with(step_envelope(step_answer("https://example.com/done")))
            .expect(1)
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/agent/runs/run-q1/step"))
            .and(wiremock::matchers::body_partial_json(
                serde_json::json!({ "action": { "type": "get_content" } }),
            ))
            .respond_with(step_envelope(step_answer("https://example.com/done")))
            .expect(1)
            .mount(&server)
            .await;
        let state = crate::state::StateStore::new();
        holding_a_page(&state);
        let broker = ActBroker {
            read_only: false,
            validations: std::sync::Mutex::new(0),
        };
        let quarry = crate::quarry_agent::QuarryAgentClient::new(server.uri(), "tok")
            .expect("a client for the stub");

        let text = browser_do_act_run(&act_dispatch(&state, &broker), &quarry, CLICK_REF)
            .await
            .expect("an act on an act-level grant runs");

        assert!(text.starts_with("<untrusted_page_content>"), "{text}");
        assert!(text.contains("\"page_url\":\"https://example.com/done\""), "{text}");
        for id in ["grant-1", "run-q1", "lease-q1"] {
            assert!(!text.contains(id), "{id} reached the model: {text}");
        }
    }

    /// An act Quarry reports as blocked by policy is not reported as done, and the page is not read
    /// as if it had changed.
    #[tokio::test]
    async fn a_blocked_act_is_not_reported_as_done() {
        let server = wiremock::MockServer::start().await;
        let mut answer = step_answer("https://example.com/");
        answer["policy_denials"] = serde_json::json!(["navigation outside the fence"]);
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/agent/runs/run-q1/step"))
            .respond_with(step_envelope(answer))
            .expect(1)
            .mount(&server)
            .await;
        let state = crate::state::StateStore::new();
        holding_a_page(&state);
        let broker = ActBroker {
            read_only: false,
            validations: std::sync::Mutex::new(0),
        };
        let quarry = crate::quarry_agent::QuarryAgentClient::new(server.uri(), "tok")
            .expect("a client for the stub");

        let refusal = browser_do_act_run(&act_dispatch(&state, &broker), &quarry, CLICK_REF).await;

        assert_eq!(refusal.err(), Some(ACT_REFUSED));
    }

    /// A failed act is never replayed: the risk gate that holds every browser act is what decides the
    /// retry, and an act is risky by name.
    #[test]
    fn act_is_never_retried() {
        let gate = crate::permission::RunGate::with_gate(
            crate::capability_policy::ToolClassGate::Off,
            "org-1",
            "run-1",
        );
        assert!(!crate::runtime_loop::retry::tool_call_is_retryable(
            &gate,
            "browser.act",
            CLICK_REF,
            "upstream timeout",
        ));
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
    fn outcome_labels_cover_every_execution_and_stay_in_the_closed_list() {
        let all = [
            BrowserAgentExecution::Completed { output: String::new() },
            BrowserAgentExecution::Failed { reason: "r".to_owned() },
            BrowserAgentExecution::PermissionDenied { reason: "r".to_owned() },
            BrowserAgentExecution::TimedOut { reason: "r".to_owned() },
            BrowserAgentExecution::ResourceExhausted { reason: "r".to_owned() },
            BrowserAgentExecution::Cancelled { reason: "r".to_owned() },
            BrowserAgentExecution::Aborted { reason: "r".to_owned() },
            BrowserAgentExecution::AwaitingApproval,
        ];
        let labels: std::collections::BTreeSet<&str> = all.iter().map(outcome_label).collect();
        assert_eq!(labels.len(), all.len());
        for label in labels {
            assert!(browser_agent::metrics::OUTCOME_LABELS.contains(&label), "{label}");
        }
        // The banner facts are the ceilings in force.
        let facts = PlanCeilings::default().banner_facts();
        assert_eq!((facts.default_steps, facts.ceiling_steps), (20, 100));
        assert_eq!(facts.procedure, "legacy/0");
    }

    #[test]
    fn approval_redirect_default_off_and_only_literal_values_enable() {
        assert!(!ceilings_with(&[]).approval_redirect);
        assert!(!PlanCeilings::default().approval_redirect);
        for on in ["1", "true", "on", " 1 "] {
            let set = ceilings_with(&[("QUARRY_BROWSER_AGENT_APPROVAL_REDIRECT", on)]);
            assert!(set.approval_redirect, "{on:?}");
        }
        for off in ["0", "false", "yes", "", "TRUE"] {
            let set = ceilings_with(&[("QUARRY_BROWSER_AGENT_APPROVAL_REDIRECT", off)]);
            assert!(!set.approval_redirect, "{off:?}");
        }
    }

    #[test]
    fn observation_quality_default_off_and_only_literal_values_enable() {
        assert!(!ceilings_with(&[]).observation_quality);
        assert!(!PlanCeilings::default().observation_quality);
        for on in ["1", "true", "on"] {
            let set = ceilings_with(&[("QUARRY_BROWSER_AGENT_OBSERVATION_QUALITY", on)]);
            assert!(set.observation_quality, "{on:?}");
        }
        for off in ["0", "false", "yes", "", "TRUE"] {
            let set = ceilings_with(&[("QUARRY_BROWSER_AGENT_OBSERVATION_QUALITY", off)]);
            assert!(!set.observation_quality, "{off:?}");
        }
    }

    #[test]
    fn steer_default_off_and_only_literal_values_enable() {
        assert!(!ceilings_with(&[]).steer);
        assert!(!PlanCeilings::default().steer);
        for on in ["1", "true", "on", " on "] {
            assert!(ceilings_with(&[("QUARRY_BROWSER_AGENT_STEER", on)]).steer, "{on:?}");
        }
        for off in ["0", "false", "yes", "", "TRUE"] {
            assert!(!ceilings_with(&[("QUARRY_BROWSER_AGENT_STEER", off)]).steer, "{off:?}");
        }
    }

    #[test]
    fn require_quarry_redaction_stamp_default_off_and_only_literal_values_enable() {
        assert!(!ceilings_with(&[]).require_quarry_redaction_stamp);
        assert!(!PlanCeilings::default().require_quarry_redaction_stamp);
        for on in ["1", "true", "on", " 1 "] {
            let set = ceilings_with(&[("EXECUTION_CORE_REQUIRE_QUARRY_REDACTION_STAMP", on)]);
            assert!(set.require_quarry_redaction_stamp, "{on:?}");
        }
        for off in ["0", "", "off", "yes", "TRUE", "enforce"] {
            let set = ceilings_with(&[("EXECUTION_CORE_REQUIRE_QUARRY_REDACTION_STAMP", off)]);
            assert!(!set.require_quarry_redaction_stamp, "{off:?}");
        }
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

    fn build_plan(json: &str) -> Result<browser_agent::PlanConfig, String> {
        let grant = ValidatedBrowserGrant::for_test("grant-1", "run", "", "u", false);
        let input: BrowserAgentInput = serde_json::from_str(json).map_err(|e| e.to_string())?;
        browser_plan_config_with(input, "org", false, &grant, &PlanCeilings::default())
    }

    #[test]
    fn space_restriction_enters_only_through_the_ask() {
        // Nothing but `policy` (and `plan_mode`) can restrict a run: a caller's
        // stray top-level keys are not restrictions and change nothing.
        let stray = build_plan(
            r#"{"grant_id":"grant-1","deny_domains":["a.example"],"disable":["browser.act.upload"],
                "space_restriction":{"disable":["browser.act.input"]}}"#,
        )
        .unwrap();
        assert!(stray.policy.is_none());

        let asked = build_plan(
            r#"{"grant_id":"grant-1","policy":{"disable":["browser.act.upload"],
                "deny_domains":["a.example"]}}"#,
        )
        .unwrap();
        let ask = asked.policy.expect("the ask is carried");
        assert_eq!(ask.disable, ["browser.act.upload"]);
        assert_eq!(ask.deny_domains, ["a.example"]);
        // The ask never rewrites the broker's host policy.
        assert_eq!(asked.allowed_domains, stray.allowed_domains);

        // Quarry's loosening-capable keys are refused, not dropped.
        for key in ["robots_max", "proxy_max", "mode"] {
            let json = format!(r#"{{"grant_id":"grant-1","policy":{{"{key}":"off"}}}}"#);
            assert!(build_plan(&json).is_err(), "{key}");
        }
        // An ask the run cannot send as given fails the run.
        let bad = build_plan(r#"{"grant_id":"grant-1","policy":{"deny_domains":["https://x.example/"]}}"#);
        assert!(bad.is_err());
    }

    #[test]
    fn plan_mode_unions_with_the_callers_ask() {
        let plan = build_plan(
            r#"{"grant_id":"grant-1","plan_mode":true,
                "policy":{"disable":["browser.act.upload","browser.act.scroll"],
                          "deny_domains":["a.example"]}}"#,
        )
        .unwrap();
        let ask = plan.policy.expect("ask");
        assert_eq!(
            ask.disable,
            [
                "browser.act.download",
                "browser.act.input",
                "browser.act.interact",
                "browser.act.scroll",
                "browser.act.upload",
            ]
        );
        assert_eq!(ask.deny_domains, ["a.example"]);
        assert!(build_plan(r#"{"grant_id":"grant-1","plan_mode":false}"#).unwrap().policy.is_none());
        assert!(build_plan(r#"{"grant_id":"grant-1","policy":{}}"#).unwrap().policy.is_none());
    }

    #[test]
    fn trace_and_request_id_come_from_the_input_and_the_run_id_from_the_run() {
        let plan = build_plan(
            r#"{"grant_id":"grant-1","run_id":"run_77","request_id":"req_ab-1",
                "trace":{"run_id":"forged","origin":"SCHEDULE",
                         "agent":{"id":"browser-agent","role":"browser"}}}"#,
        )
        .unwrap();
        assert_eq!(plan.request_id.as_deref(), Some("req_ab-1"));
        let trace = plan.trace.expect("trace");
        assert_eq!(trace.run_id.as_deref(), Some("run_77"), "the run id is never the caller's");
        assert_eq!(trace.origin.as_deref(), Some("SCHEDULE"));
        assert_eq!(trace.agent.and_then(|a| a.id).as_deref(), Some("browser-agent"));

        // Hostile values are dropped; no trace input, no trace.
        let hostile = build_plan(
            r#"{"grant_id":"grant-1","run_id":"run_77","request_id":"a b\n",
                "trace":{"origin":"USER; drop table","agent":{"id":"x y"}}}"#,
        )
        .unwrap();
        assert_eq!(hostile.request_id, None);
        let kept = hostile.trace.expect("the run id still labels the run");
        assert_eq!(kept.origin, None);
        assert_eq!(kept.agent, None);
        assert!(build_plan(r#"{"grant_id":"grant-1","run_id":"run_77"}"#).unwrap().trace.is_none());
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
            rendered_controls: Vec::new(),
            snapshot_targets: vec![],
            error_message: String::new(),
            challenge: None,
            redaction_rule_version: None,
            quality: Default::default(),
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

    // WS8.G attach mode at the bridge.

    #[test]
    fn attach_input_with_an_attach_grant_selects_attach_mode_and_drops_the_profile() {
        let input: BrowserAgentInput = serde_json::from_str(
            r#"{"grant_id":"grant-att","attach_session_id":" run_user_1 ","profile_id":"prof-1","max_steps":5}"#,
        )
        .unwrap();
        let grant = ValidatedBrowserGrant::for_test("grant-att", "attach", "grant-run", "u", false);
        let plan = browser_plan_config(input, "verified-org", false, &grant).unwrap();
        assert_eq!(
            plan.attach,
            Some(browser_agent::AttachConfig {
                target_tab: None,
                session_run_id: "run_user_1".to_owned()
            })
        );
        assert_eq!(plan.profile_id, None, "an attached run never names a profile");
        assert_eq!(plan.grant_id, "grant-att");
        assert_eq!(plan.takeover_wait_s, 0, "the D5 default applies");
    }

    #[test]
    fn attach_input_without_an_attach_grant_is_refused() {
        for kind in ["run", "session", "human_browse", ""] {
            let input: BrowserAgentInput = serde_json::from_str(
                r#"{"grant_id":"grant-1","attach_session_id":"run_user_1"}"#,
            )
            .unwrap();
            let grant = ValidatedBrowserGrant::for_test("grant-1", kind, "p", "u", false);
            let error = browser_plan_config(input, "verified-org", false, &grant).unwrap_err();
            assert!(error.contains("attach grant"), "{kind}: {error}");
        }
    }

    #[test]
    fn an_attach_grant_cannot_drive_a_leased_run() {
        for body in [r#"{"grant_id":"grant-att"}"#, r#"{"grant_id":"grant-att","attach_session_id":"  "}"#] {
            let input: BrowserAgentInput = serde_json::from_str(body).unwrap();
            let grant = ValidatedBrowserGrant::for_test("grant-att", "attach", "grant-run", "u", false);
            let error = browser_plan_config(input, "verified-org", false, &grant).unwrap_err();
            assert!(error.contains("attached browser run"), "{body}: {error}");
        }
    }

    #[test]
    fn a_leased_input_is_unchanged_by_attach_mode() {
        let input: BrowserAgentInput =
            serde_json::from_str(r#"{"grant_id":"grant-1","profile_id":"prof-1"}"#).unwrap();
        let grant = ValidatedBrowserGrant::for_test("grant-1", "run", "grant-s", "u", false);
        let plan = browser_plan_config(input, "verified-org", false, &grant).unwrap();
        assert_eq!(plan.attach, None);
        assert_eq!(plan.profile_id.as_deref(), Some("prof-1"));
    }

    #[test]
    fn attach_is_refused_for_a_zdr_run_at_the_bridge() {
        let input: BrowserAgentInput = serde_json::from_str(
            r#"{"grant_id":"grant-att","attach_session_id":"run_user_1"}"#,
        )
        .unwrap();
        let grant = ValidatedBrowserGrant::for_test("grant-att", "attach", "grant-run", "u", true);
        let error = browser_plan_config(input, "verified-org", true, &grant).unwrap_err();
        assert!(error.contains("zero-data-retention"), "{error}");
    }

    #[test]
    fn tab_mark_on_complete_defaults_to_deliverable_and_takes_only_the_closed_set() {
        use crate::quarry_agent::TabMark;
        let grant = ValidatedBrowserGrant::for_test("grant-att", "attach", "grant-run", "u", false);
        let build = |extra: &str| {
            let input: BrowserAgentInput = serde_json::from_str(&format!(
                r#"{{"grant_id":"grant-att","attach_session_id":"run_user_1"{extra}}}"#
            ))
            .unwrap();
            browser_plan_config(input, "verified-org", false, &grant).unwrap()
        };
        assert_eq!(build("").tab_mark_on_complete, TabMark::Deliverable);
        for (name, mark) in [
            ("none", TabMark::None),
            ("deliverable", TabMark::Deliverable),
            ("handoff", TabMark::Handoff),
        ] {
            let plan = build(&format!(r#","tab_mark_on_complete":"{name}""#));
            assert_eq!(plan.tab_mark_on_complete, mark, "{name}");
        }
        for bad in ["keep", "Deliverable", "", "pinned"] {
            let parsed: Result<BrowserAgentInput, _> = serde_json::from_str(&format!(
                r#"{{"grant_id":"g","tab_mark_on_complete":"{bad}"}}"#
            ));
            assert!(parsed.is_err(), "{bad:?} is outside the closed set");
        }
        // A leased input carries the default too; the loop only marks an attachment.
        let input: BrowserAgentInput = serde_json::from_str(r#"{"grant_id":"grant-1"}"#).unwrap();
        let leased = ValidatedBrowserGrant::for_test("grant-1", "run", "", "u", false);
        let plan = browser_plan_config(input, "verified-org", false, &leased).unwrap();
        assert_eq!((plan.attach.is_none(), plan.tab_mark_on_complete), (true, TabMark::Deliverable));
    }

    // ---- WS9.E browser.console ------------------------------------------

    fn console_request(input: &str) -> Result<BrowserMcpRequest, String> {
        let grant = ValidatedBrowserGrant::for_test("grant-1", "run", "", "u", false);
        BrowserMcpRequest::parse("browser.console", input, &grant)
    }

    fn console_query(input: &str) -> ConsoleQuery {
        match console_request(input).expect("a valid console input").kind {
            BrowserMcpKind::Console(query) => query,
            _ => panic!("not a console request"),
        }
    }

    const CONSOLE_BASE: &str = r#""grant_id":"grant-1","quarry_run_id":"run_ab-1","lease_id":"lease-1""#;

    /// Quarry's owner projection of run `run-q1`, which the grant `grant_id` started.
    fn console_projection(grant_id: &str) -> wiremock::ResponseTemplate {
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "runId": "run-q1",
                "controlMode": "agent_control",
                "controlEpoch": 0,
                "grantId": grant_id,
                "tabs": []
            },
            "meta": { "request_id": "req_1" },
            "error": null,
        }))
    }

    /// One console line, as Quarry's devtools read answers it.
    fn console_page() -> wiremock::ResponseTemplate {
        wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "events": [{
                    "sequence": 5, "tabId": "tab_9", "category": "console", "name": "console.error",
                    "level": "error", "text": "boom", "timestampMs": 1, "payload": null
                }],
                "nextSequence": 5,
                "zdr": false
            },
            "meta": {},
            "error": null,
        }))
    }

    /// `browser.console` for `grant-1` on run `run-q1`, whose record is `record`; the stub expects
    /// `reads` console reads and fails the test when Quarry gets another number.
    async fn console_as_grant_1(record: wiremock::ResponseTemplate, reads: u64) -> ToolExecution {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/v1/agent/runs/run-q1/browser-session",
            ))
            .respond_with(record)
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/v1/agent/runs/run-q1/devtools"))
            .respond_with(console_page())
            .expect(reads)
            .mount(&server)
            .await;
        let client = crate::quarry_agent::QuarryAgentClient::new(server.uri(), "tok")
            .expect("a client for the stub");
        console_for_grant(
            &client,
            "org_1",
            "run-q1",
            "grant-1",
            console_query(&format!("{{{CONSOLE_BASE}}}")),
        )
        .await
    }

    /// `browser.console` reads a run only when the run's own record names the grant the call holds
    /// (EP-1 D3-652). The input's run id is not trusted: a run another grant started, a record with no
    /// grant, and a run that cannot be read are all refused before the console is read.
    #[tokio::test]
    async fn console_reads_a_run_only_for_the_grant_that_started_it() {
        let bound = console_as_grant_1(console_projection("grant-1"), 1).await;
        assert_eq!(bound.error, None);
        assert!(bound.output.contains("boom"), "{}", bound.output);

        for record in [console_projection("grant-2"), console_projection("")] {
            let refused = console_as_grant_1(record, 0).await;
            assert_eq!(refused.error.as_deref(), Some(CONSOLE_NOT_GRANT_RUN));
            assert!(refused.output.is_empty());
        }

        let unreadable = console_as_grant_1(
            wiremock::ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "data": null,
                "meta": { "request_id": "req_1" },
                "error": { "code": "NOT_FOUND", "message": "no such run" },
            })),
            0,
        )
        .await;
        assert_eq!(unreadable.error.as_deref(), Some(CONSOLE_NOT_GRANT_RUN));
    }

    // ---- E3.2-e2 tab operations through browser.act ---------------------

    fn act_kind(action: &str) -> Result<BrowserMcpKind, String> {
        let grant = ValidatedBrowserGrant::for_test("grant-1", "run", "", "u", false);
        BrowserMcpRequest::parse(
            "browser.act",
            &format!(r#"{{{CONSOLE_BASE},"action":{action}}}"#),
            &grant,
        )
        .map(|request| request.kind)
    }

    fn tab_op(action: &str) -> TabOp {
        match act_kind(action).expect("a valid tab action") {
            BrowserMcpKind::Tabs(op) => op,
            _ => panic!("not a tab request"),
        }
    }

    #[test]
    fn tab_actions_parse_to_tab_operations() {
        assert_eq!(tab_op(r#"{"type":"tab_list"}"#), TabOp::List);
        assert_eq!(tab_op(r#"{"type":"tab_new"}"#), TabOp::New { url: None });
        assert_eq!(
            tab_op(r#"{"type":"tab_new","url":"  https://docs.example.com/a "}"#),
            TabOp::New { url: Some("https://docs.example.com/a".to_owned()) }
        );
        assert_eq!(tab_op(r#"{"type":"tab_new","url":"  "}"#), TabOp::New { url: None });
        assert_eq!(tab_op(r#"{"type":"tab_select","tab_id":"tab_2"}"#), TabOp::Select("tab_2".to_owned()));
        assert_eq!(tab_op(r#"{"type":"tab_close","tab_id":" tab-9 "}"#), TabOp::Close("tab-9".to_owned()));
    }

    #[test]
    fn tab_actions_refuse_foreign_urls_bad_ids_and_extra_fields() {
        for action in [
            r#"{"type":"tab_new","url":"https://evil.com/"}"#,
            r#"{"type":"tab_new","url":"https://example.com.evil.com/"}"#,
            r#"{"type":"tab_new","url":"file:///etc/passwd"}"#,
            r#"{"type":"tab_new","url":"javascript:alert(1)"}"#,
            r#"{"type":"tab_new","url":"example.com/a"}"#,
            r#"{"type":"tab_select","tab_id":"tab/../x"}"#,
            r#"{"type":"tab_select","tab_id":""}"#,
            r#"{"type":"tab_close","tab_id":"a?b=1"}"#,
            r#"{"type":"tab_list","extra":1}"#,
            r#"{"type":"tab_select"}"#,
        ] {
            assert!(act_kind(action).is_err(), "{action}");
        }
        let long = format!(r#"{{"type":"tab_new","url":"https://example.com/{}"}}"#, "a".repeat(2100));
        assert!(act_kind(&long).is_err());
    }

    #[test]
    fn a_governed_action_is_still_an_act_and_tab_variants_are_never_steps() {
        assert!(matches!(
            act_kind(r#"{"type":"click_ref","snapshot_id":"s","generation":1,"ref_id":"@e1"}"#),
            Ok(BrowserMcpKind::Act(_))
        ));
        assert!(BrowserMcpAction::TabList {}.into_quarry_action().is_err());
        assert!(BrowserMcpAction::TabSelect { tab_id: "t".to_owned() }.into_quarry_action().is_err());
    }

    #[test]
    fn tab_output_is_scrubbed_bounded_and_fragmentless() {
        use crate::quarry_agent::WireTab;
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdEFGH1234";
        let tabs: Vec<WireTab> = (0..25)
            .map(|n| WireTab {
                tab_id: format!("tab_{n}"),
                title: Some(format!("Title {n} {jwt} {}", "x".repeat(400))),
                url: Some(format!("https://example.com/p{n}?lang=en#secret-fragment")),
                active: n == 0,
            })
            .collect();
        let value: serde_json::Value = serde_json::from_str(&tabs_output(Some(&tabs[3]), &tabs)).unwrap();
        assert_eq!(value["tabs"].as_array().unwrap().len(), MAX_TAB_LIST);
        assert_eq!(value["truncated"], true);
        assert_eq!(value["tab"]["tab_id"], "tab_3");
        let text = value.to_string();
        assert!(!text.contains("eyJhbGciOiJIUzI1NiJ9"), "{text}");
        assert!(!text.contains("secret-fragment"), "{text}");
        assert!(value["tabs"][0]["title"].as_str().unwrap().chars().count() <= MAX_TAB_TITLE_CHARS);
        assert!(value["tabs"][0]["url"].as_str().unwrap().starts_with("https://example.com/p0?lang=en"));
        let short: serde_json::Value = serde_json::from_str(&tabs_output(None, &tabs[..2])).unwrap();
        assert_eq!(short["truncated"], false);
        assert!(short.get("tab").is_none());
    }

    #[test]
    fn browser_console_input_parses_with_defaults_and_every_field() {
        let bare = console_query(&format!("{{{CONSOLE_BASE}}}"));
        assert_eq!(
            bare,
            ConsoleQuery { levels: None, filter: None, limit: 50, since_sequence: None }
        );
        let full = console_query(&format!(
            r#"{{{CONSOLE_BASE},"levels":["Error","warn","error"],"filter":"  boom  ","limit":20,"since_sequence":7}}"#
        ));
        assert_eq!(full.levels.as_deref(), Some("error,warn"), "lower-cased, de-duplicated");
        assert_eq!(full.filter.as_deref(), Some("boom"));
        assert_eq!((full.limit, full.since_sequence), (20, Some(7)));
        assert_eq!(
            console_query(&format!(r#"{{{CONSOLE_BASE},"levels":[],"filter":" ","since_sequence":0}}"#)),
            bare,
            "empty levels, a blank filter and sequence 0 are no constraint"
        );
    }

    #[test]
    fn browser_console_input_rejects_unknown_fields_levels_and_a_foreign_grant() {
        for extra in [
            r#""payload":true"#,
            r#""category":"network""#,
            r#""tab_id":"tab_1""#,
            r#""after_sequence":3"#,
        ] {
            let error = console_request(&format!("{{{CONSOLE_BASE},{extra}}}")).err().expect(extra);
            assert!(error.contains("unknown field"), "{extra}: {error}");
        }
        assert!(console_request(&format!(r#"{{{CONSOLE_BASE},"levels":["fatal"]}}"#)).is_err());
        assert!(console_request(&format!(
            r#"{{{CONSOLE_BASE},"levels":["debug","info","log","warn","error","verbose"]}}"#
        ))
        .is_err(), "six names (five distinct canonical levels at most per call)");
        let long = "x".repeat(201);
        assert!(console_request(&format!(r#"{{{CONSOLE_BASE},"filter":"{long}"}}"#)).is_err());
        assert!(console_request(&format!(r#"{{{CONSOLE_BASE},"filter":"a\nb"}}"#)).is_err());
        // The grant must be the broker-validated one; the run id must be a Quarry id.
        let foreign = r#"{"grant_id":"someone-else","quarry_run_id":"run_1","lease_id":"l"}"#;
        let refused = |input: &str| match console_request(input) {
            Err(error) => error,
            Ok(_) => panic!("{input} was accepted"),
        };
        assert!(refused(foreign).contains("does not match"));
        let traversal = r#"{"grant_id":"grant-1","quarry_run_id":"../admin","lease_id":"l"}"#;
        assert!(refused(traversal).contains("not a Quarry run id"));
        let missing = r#"{"grant_id":"grant-1","quarry_run_id":"run_1"}"#;
        assert!(console_request(missing).is_err());
    }

    #[test]
    fn browser_console_limit_is_clamped_to_one_through_one_hundred() {
        for (asked, sent) in [(0_i64, 1_u32), (-5, 1), (1, 1), (100, 100), (101, 100), (9_999_999_999, 100)] {
            let query = console_query(&format!(r#"{{{CONSOLE_BASE},"limit":{asked}}}"#));
            assert_eq!(query.limit, sent, "limit {asked}");
        }
    }

    fn page_from(json: serde_json::Value) -> crate::quarry_agent::WireDevtoolsPage {
        serde_json::from_value(json).expect("a page")
    }

    #[test]
    fn browser_console_output_has_no_payload_and_only_the_four_fields() {
        let page = page_from(serde_json::json!({
            "events": [{
                "sequence": 11, "tabId": "tab_1", "category": "console", "name": "console.error",
                "level": "error", "text": "Uncaught TypeError", "url": "https://example.com/x?token=zzz",
                "method": "GET", "status": 500, "timestampMs": 1,
                "payload": {"stack": "secret stack", "args": ["hunter2"]}
            }],
            "nextSequence": 11, "zdr": false
        }));
        let out: serde_json::Value = serde_json::from_str(&console_output(page)).unwrap();
        assert_eq!(out["next_sequence"], 11);
        assert_eq!(out["truncated"], false);
        let entries = out["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        let entry = entries[0].as_object().unwrap();
        let mut keys: Vec<&str> = entry.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["level", "sequence", "tab_id", "text"]);
        assert_eq!(entry["text"], "Uncaught TypeError");
        let raw = out.to_string();
        for forbidden in ["payload", "secret stack", "hunter2", "token=zzz", "timestamp"] {
            assert!(!raw.contains(forbidden), "{forbidden}: {raw}");
        }
    }

    #[test]
    fn browser_console_output_is_scrubbed_and_capped_per_text() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdEFGH1234";
        let long = format!("{}{jwt}{}", "a".repeat(495), "b".repeat(2_000));
        let page = page_from(serde_json::json!({
            "events": [
                {"sequence": 1, "tabId": null, "level": "log", "text": format!("token {jwt} here")},
                {"sequence": 2, "tabId": "t", "level": "warning", "text": long},
                {"sequence": 3, "tabId": "t", "level": "info"}
            ],
            "nextSequence": 3
        }));
        let out: serde_json::Value = serde_json::from_str(&console_output(page)).unwrap();
        let entries = out["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 3);
        assert!(!out.to_string().contains("eyJhbGciOiJIUzI1NiJ9"), "{out}");
        assert_eq!(entries[0]["text"], "token [REDACTED] here");
        assert!(entries[0]["tab_id"].is_null());
        assert_eq!(entries[1]["text"].as_str().unwrap().chars().count(), 500);
        assert_eq!(entries[2]["text"], "", "an event with no text is an empty text");
        assert_eq!(out["truncated"], true, "a cut text is reported");
        assert_eq!(out["next_sequence"], 3, "nothing was dropped, so Quarry's cursor stands");
    }

    #[test]
    fn browser_console_output_stops_at_100_entries_and_16_kb_and_points_past_the_last_kept() {
        let events = |n: u64, text: &str| -> Vec<serde_json::Value> {
            (1..=n)
                .map(|i| serde_json::json!({"sequence": i, "tabId": "t", "level": "log", "text": text}))
                .collect()
        };
        // 150 short entries: the 100-entry cap.
        let page = page_from(serde_json::json!({"events": events(150, "x"), "nextSequence": 150}));
        let out: serde_json::Value = serde_json::from_str(&console_output(page)).unwrap();
        assert_eq!(out["entries"].as_array().unwrap().len(), 100);
        assert_eq!(out["truncated"], true);
        assert_eq!(out["next_sequence"], 100, "resume after the last entry returned");

        // 100 entries of 500 characters: the 16 KB cap bites first.
        let page = page_from(
            serde_json::json!({"events": events(100, &"y".repeat(500)), "nextSequence": 100}),
        );
        let raw = console_output(page);
        assert!(raw.len() <= 16 * 1024, "{} bytes", raw.len());
        let out: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let kept = out["entries"].as_array().unwrap().len();
        assert!(kept > 20 && kept < 100, "{kept}");
        assert_eq!(out["truncated"], true);
        assert_eq!(out["next_sequence"], kept as u64);

        // An empty page keeps Quarry's cursor and is not truncated.
        let out: serde_json::Value =
            serde_json::from_str(&console_output(page_from(serde_json::json!({"nextSequence": 42})))).unwrap();
        assert_eq!((out["entries"].as_array().unwrap().len(), out["next_sequence"].clone(), out["truncated"].clone()),
            (0, serde_json::json!(42), serde_json::json!(false)));
    }

    #[test]
    fn browser_console_is_not_an_observe_or_act_request() {
        let observe = {
            let grant = ValidatedBrowserGrant::for_test("grant-1", "run", "", "u", false);
            BrowserMcpRequest::parse("browser.observe", &format!("{{{CONSOLE_BASE}}}"), &grant).unwrap()
        };
        assert!(matches!(observe.kind, BrowserMcpKind::Observe));
        // The observe input has no console keys: `levels` is refused there.
        let grant = ValidatedBrowserGrant::for_test("grant-1", "run", "", "u", false);
        assert!(BrowserMcpRequest::parse(
            "browser.observe",
            &format!(r#"{{{CONSOLE_BASE},"levels":["error"]}}"#),
            &grant
        )
        .is_err());
    }
}
