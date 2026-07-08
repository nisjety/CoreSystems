//! Agent run driver — governed multi-tool loop.
//!
//! Bridges the gap where `session-core.StartRun` durably records a run as
//! `'queued'` + a `RUN_STARTED` event + a draft plan, then returns with nothing
//! to dispatch it. [`run_agent`] is that dispatch: it transitions the draft plan
//! to executing, then runs a capped ReAct-style loop — offer the read-tool
//! allowlist, `Infer`, dispatch any requested tool calls back through
//! `runtime_loop::execute_step` (the same permission/hook-gated path the
//! `ExecuteStep` RPC uses), feed the outcomes back, and re-infer until the model
//! answers or the round budget is exhausted. The assistant answer is persisted
//! to the run's thread and the run is finalized with exactly ONE terminal
//! `CompleteStep`.
//!
//! Governance is the point of this layer:
//!   * **HITL** — when a tool call is gated (`ask` posture + a risky tool),
//!     `execute_step` returns `awaiting_approval`; the loop mints a durable
//!     `CreateApproval` (exactly as the `ExecuteStep` RPC does), flips the run
//!     `AwaitingApproval`, and RETURNS early. Resume re-invokes `run_agent`.
//!   * **Default posture is `ask`** — deployed agents must not silently run
//!     `shell`; only an explicit `auto` mode opens that gate.
//!   * **Purpose-lock (scope lock)** — the agent may ONLY call tools in the
//!     offered allowlist; an un-offered tool name is rejected, never dispatched.
//!     The bound goal is logged once at run start.
//!   * **GDPR** — every dispatched tool's `data_category` and the run's real
//!     ZDR flag (`RunAgentRequest.zdr`, populated by the gateway from the chat
//!     request) are carried on the per-tool step's output detail so session-core
//!     can publish an audit event, and the ZDR flag is threaded into every
//!     inference round.
//!
//! Per-tool steps use a NON-TERMINAL status (`"running"`); only the single
//! `step_id="final"` step is terminal. This is load-bearing: session-core's
//! `record_run_terminal` flips the run status on the FIRST `completed`/`failed`
//! `CompleteStep`, so a per-tool terminal status would end the run mid-loop.
//!
//! A run must NEVER be left `'queued'`: on any Infer/persist error this still
//! appends a graceful assistant reply, flips the run terminal with a failed
//! `CompleteStep`, transitions the plan to FAILED, and returns
//! `RunAgentResponse { status: "failed", .. }`.

use std::collections::BTreeSet;

use mp_contracts::model_plane::v1::{
    self as pb, inference_core_client::InferenceCoreClient,
    orchestration_core_service_client::OrchestrationCoreServiceClient,
    session_core_client::SessionCoreClient,
};
use tonic::transport::Channel;
use tracing::{info, warn};

use crate::permission::PermissionMode;
use crate::runtime_loop::{self, StepOutcome};

/// Short agent preamble used as the system message. Names the bound scope so
/// the model stays on the offered tools — including the WRITE-capable ones,
/// which earlier wording omitted entirely (it described the toolset as
/// exclusively "read-only fact-gathering," a leftover from before
/// `book_shipment`/`execute_provider_action`/`publish_social_post` existed). That
/// framing measurably suppressed real tool use: models defaulted to a
/// generic "I cannot post on your behalf" refusal and drafted copy-paste
/// text instead of calling the tool, even with a genuinely connected
/// account (observed 2026-07-08 calibrating the eval harness's HITL case).
const AGENT_PREAMBLE: &str = "You are Velion, a concise and helpful assistant for a Norwegian \
business. You have two kinds of tools: READ tools to gather facts (weather, traffic, news, \
shipment tracking, the Brønnøysund company registry, the organization's own knowledge base, and \
the public web), and ACTION tools that take REAL effect for this organization — booking a \
shipment, running a connected provider's operation (e.g. posting to Meta/LinkedIn/Slack), or \
publishing a social media post. You DO have genuine capability to take these actions through the \
listed tools; you are not limited to suggesting text for the user to act on themselves. When the \
user asks you to do something an action tool covers, CALL the tool — do not default to 'I cannot \
access your accounts' or offer copy-paste text instead, and do not ask the user to do it manually \
unless the tool call itself reports that it cannot proceed (e.g. no connected account). Risky \
action tools require human approval before they run; that pause is expected and is not a reason \
to avoid calling the tool — say what you are attempting and let the approval step do its job. \
Only use the tools you have been given. When you have enough information or have taken the \
requested action, answer the user's request directly and clearly.";

/// Temperature for each inference round.
const TEMPERATURE: f32 = 0.7;

/// Max tokens for each inference round.
const MAX_TOKENS: i32 = 1024;

/// Default round budget when the request does not specify one.
const DEFAULT_MAX_ROUNDS: u32 = 4;

/// Hard ceiling on rounds regardless of the request, so a misbehaving caller
/// can't drive an unbounded loop.
const MAX_ROUNDS_CEILING: u32 = 8;

/// Cap on a single tool outcome rendered back into the conversation context.
const MAX_TOOL_CONTEXT_CHARS: usize = 4000;

/// Graceful reply persisted to the thread when no inference round yields an
/// answer, so a user-facing answer always exists.
const GRACEFUL_FAILURE_REPLY: &str =
    "I wasn't able to complete that request right now. Please try again.";

/// Cap on the `CompleteStep.output` payload (the durable step record); the full
/// answer still lives in the appended assistant message.
const OUTPUT_TRUNCATE: usize = 2000;

/// GDPR data-classification of a tool's results, carried on the step audit
/// detail. The five Norwegian read tools + the public web touch only public,
/// non-personal data; `knowledge_search` reads the org's own private corpus.
fn data_category(tool_name: &str) -> &'static str {
    match tool_name {
        "knowledge_search" => "customer_private",
        // External MCP tools can touch arbitrary data of unknown provenance —
        // classify honestly as `unclassified` rather than over-claim that an
        // external server returns only public, non-personal data.
        name if name.starts_with("mcp__") => "unclassified",
        _ => "public_non_personal",
    }
}

/// One requested tool call's outcome, framed for the next round's context.
struct ToolStepResult {
    name: String,
    output: String,
    error: Option<String>,
}

/// Drive an agent run to a terminal answer through a governed multi-tool loop.
///
/// Emits `PlanTransitioned DRAFT→EXECUTING`, then loops (offer tools → `Infer` →
/// dispatch tool calls via [`runtime_loop::execute_step`] → feed outcomes back)
/// up to the round budget. On a final answer it appends the assistant message,
/// finalizes with ONE terminal `CompleteStep`, emits
/// `PlanTransitioned EXECUTING→COMPLETED`, and snapshots the `StateStore`. If a
/// tool call is gated it mints an approval and returns `awaiting_approval` early.
/// On any failure it takes the graceful path and returns a `"failed"` response —
/// never leaving the run `'queued'`.
pub async fn run_agent(
    state: &crate::state::StateStore,
    session_channel: Channel,
    inference_channel: Channel,
    req: pb::RunAgentRequest,
) -> pb::RunAgentResponse {
    let tools = merged_tool_defs(&req.org_id, &req.user_id).await;
    run_agent_with_tools(state, session_channel, inference_channel, req, tools).await
}

/// The built-in [`offered_tool_defs`] plus the org's registered MCP tools,
/// discovered best-effort via the gateway (matrix §G2). MCP tools are
/// namespaced `mcp__<server>__<tool>`; appending them here both OFFERS them to
/// the model AND admits them into the purpose-lock allowlist (derived from this
/// Vec at ~line 154), so the `runtime_loop` dispatch arm can route them. A
/// built-in name always wins a (vanishingly unlikely) collision.
async fn merged_tool_defs(org_id: &str, user_id: &str) -> Vec<pb::ToolDefinition> {
    let mut tools = offered_tool_defs();
    if let Some(client) = crate::mcp_gateway::McpGatewayClient::from_env() {
        for tool in client.list_tools(org_id, user_id).await {
            if !tools.iter().any(|existing| existing.name == tool.name) {
                tools.push(tool);
            }
        }
    }
    tools
}

/// Loop body driving `req` with an explicit tool allowlist. The public
/// [`run_agent`] passes [`offered_tool_defs`]; tests inject a smaller (or
/// gated) tool set to exercise the purpose-lock and HITL branches without a
/// live tool backend.
// One cohesive ReAct driver (setup → round loop → finalize); splitting it would
// scatter the shared loop state across helpers for no clarity gain. Mirrors
// model-gateway's `run_tool_rounds`.
#[allow(clippy::too_many_lines)]
async fn run_agent_with_tools(
    state: &crate::state::StateStore,
    session_channel: Channel,
    inference_channel: Channel,
    req: pb::RunAgentRequest,
    tools: Vec<pb::ToolDefinition>,
) -> pb::RunAgentResponse {
    let plan_id = format!("plan_{}", req.run_id);

    // Posture: deployed agents default to `ask` (risky tools gated behind a
    // human approval). Only an explicit `auto` opens the gate; an unknown/empty
    // mode is treated as `ask` so `shell` is never silently allowed.
    let mode = resolve_mode(&req.mode);
    let permission_wire = mode_wire(mode);

    // Purpose-lock: the offered allowlist IS the scope lock. Tool calls outside
    // it are rejected, never dispatched.
    let allowlist: BTreeSet<String> = tools.iter().map(|t| t.name.clone()).collect();
    let max_rounds = if req.max_rounds == 0 {
        DEFAULT_MAX_ROUNDS
    } else {
        req.max_rounds.min(MAX_ROUNDS_CEILING)
    };

    // The run's Zero-Data-Retention flag, threaded through every inference
    // round (so inference-core's prompt cache skips durable read/write) and
    // onto each tool step's GDPR audit detail. Sourced from `RunAgentRequest`,
    // which the gateway populates from the chat request's `zdr` flag.
    let zdr = req.zdr;

    info!(
        run_id = %req.run_id,
        mode = %permission_wire,
        max_rounds,
        tools = ?allowlist,
        goal = %req.goal,
        "run_agent: purpose-locked governed multi-tool run starting"
    );

    // 1. Plan: DRAFT → EXECUTING.
    publish_plan_transition(
        &session_channel,
        &plan_id,
        &req.run_id,
        pb::PlanState::Draft,
        pb::PlanState::Executing,
    )
    .await;

    let mut messages = vec![
        pb::ChatMessage {
            role: "system".to_owned(),
            content: AGENT_PREAMBLE.to_owned(),
            name: String::new(),
        },
        pb::ChatMessage {
            role: "user".to_owned(),
            content: req.goal.clone(),
            name: String::new(),
        },
    ];

    let mut inference = InferenceCoreClient::new(inference_channel);
    let mut answer: Option<String> = None;
    let mut rounds_executed: u32 = 0;
    let mut step_seq: u32 = 0;

    for _round in 0..max_rounds {
        rounds_executed += 1;
        let infer_result = inference
            .infer(pb::InferRequest {
                request_id: req.run_id.clone(),
                org_id: req.org_id.clone(),
                model: req.model.clone(),
                provider_hint: String::new(),
                messages: messages.clone(),
                temperature: TEMPERATURE,
                max_tokens: MAX_TOKENS,
                structured_output_schema: String::new(),
                zdr,
                tools: tools.clone(),
                tool_choice: "auto".to_owned(),
            })
            .await;

        let response = match infer_result {
            Ok(r) => r.into_inner(),
            Err(error) => {
                warn!(
                    run_id = %req.run_id,
                    error = %error,
                    "run_agent: inference failed; finalizing run as failed (graceful reply)"
                );
                return finalize(
                    state,
                    &session_channel,
                    &plan_id,
                    &req,
                    GRACEFUL_FAILURE_REPLY,
                    false,
                    rounds_executed,
                )
                .await;
            }
        };

        // No tool calls → the model is ready to answer.
        if response.tool_calls.is_empty() {
            answer = Some(response.content);
            break;
        }

        // Dispatch each requested tool call through the gated execute_step path.
        let mut outcomes = Vec::with_capacity(response.tool_calls.len());
        for call in &response.tool_calls {
            // Purpose-lock: reject any tool not in the offered allowlist.
            if !allowlist.contains(&call.name) {
                warn!(
                    run_id = %req.run_id,
                    tool = %call.name,
                    "run_agent: rejecting un-offered tool (purpose-lock)"
                );
                outcomes.push(ToolStepResult {
                    name: call.name.clone(),
                    output: String::new(),
                    error: Some(format!(
                        "tool '{}' is not in this agent's allowed scope",
                        call.name
                    )),
                });
                continue;
            }

            // Stable per-(run, step) id computed BEFORE dispatch so it is shared
            // by the gate, the durable approval binding (a provider write forwards
            // this step's real approval id), and the audit step record.
            step_seq += 1;
            let step_id = tool_step_id(&req.run_id, step_seq, call);

            let outcome = runtime_loop::execute_step(
                &call.name,
                &call.arguments_json,
                permission_wire,
                "",
                &req.org_id,
                &req.user_id,
                &req.run_id,
                &step_id,
                Some(session_channel.clone()),
                None,
                None,
            )
            .await;

            // HITL: a gated tool pauses the whole run. Mint the durable approval
            // (same path as the ExecuteStep RPC), flip the run AwaitingApproval,
            // and return early — resume re-invokes run_agent.
            if outcome.status == "awaiting_approval" {
                return pause_for_approval(
                    state,
                    &session_channel,
                    &req,
                    &step_id,
                    &call.name,
                    rounds_executed,
                )
                .await;
            }

            // Non-terminal per-tool step (status "running") carrying the GDPR
            // audit detail. NEVER a terminal status — that's the final step's job.
            record_tool_step(
                &session_channel,
                &req.run_id,
                &step_id,
                &call.name,
                &outcome,
                zdr,
            )
            .await;

            outcomes.push(ToolStepResult {
                name: call.name.clone(),
                output: outcome.output,
                error: if outcome.error.is_empty() {
                    None
                } else {
                    Some(outcome.error)
                },
            });
        }

        // Carry the model's interim reasoning forward, then append the framed
        // tool outcomes so the next round can answer from them.
        if !response.content.trim().is_empty() {
            messages.push(pb::ChatMessage {
                role: "assistant".to_owned(),
                content: response.content,
                name: String::new(),
            });
        }
        messages.push(pb::ChatMessage {
            role: "user".to_owned(),
            content: format_tool_context(&outcomes),
            name: String::new(),
        });
    }

    // The loop ended either with an answer or by exhausting the round budget.
    let (final_answer, success) = if let Some(text) = answer {
        (text, true)
    } else {
        warn!(
            run_id = %req.run_id,
            rounds_executed,
            "run_agent: round budget exhausted without a final answer"
        );
        (GRACEFUL_FAILURE_REPLY.to_owned(), false)
    };
    finalize(
        state,
        &session_channel,
        &plan_id,
        &req,
        &final_answer,
        success,
        rounds_executed,
    )
    .await
}

/// Resolve the request's wire mode into a posture. Deployed agents default to
/// `ask`: only an explicit `auto` allows risky tools (e.g. `shell`) without a
/// gate; `deny` blocks all tools.
fn resolve_mode(wire: &str) -> PermissionMode {
    match wire {
        "auto" => PermissionMode::Auto,
        "deny" => PermissionMode::Deny,
        // "ask", "execute", "", and anything else → ask (safe default).
        _ => PermissionMode::Ask,
    }
}

/// Map a posture back to the wire string `execute_step`/`permission::evaluate`
/// expect.
fn mode_wire(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Auto => "auto",
        PermissionMode::Ask => "ask",
        PermissionMode::Deny => "deny",
    }
}

/// Stable per-tool step id: `tool_{seq}_{call_id_or_name}`.
fn tool_step_id(run_id: &str, seq: u32, call: &pb::ToolCall) -> String {
    let suffix = if call.id.is_empty() {
        &call.name
    } else {
        &call.id
    };
    let _ = run_id;
    format!("tool_{seq}_{suffix}")
}

/// The read-tool allowlist offered to the model. JSON-Schema literals follow the
/// `model-gateway::tool_loop::builtin_tool_defs` pattern. This set IS the
/// purpose-lock scope: only these tools may be called.
fn offered_tool_defs() -> Vec<pb::ToolDefinition> {
    vec![
        pb::ToolDefinition {
            name: "yr_weather".to_owned(),
            description: "Get the current Yr/met.no weather forecast for a coordinate in Norway.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"lat":{"type":"number","description":"Latitude"},"lon":{"type":"number","description":"Longitude"}},"required":["lat","lon"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "traffic".to_owned(),
            description: "Get traffic registration stations (volume/speed) near a coordinate from Statens Vegvesen.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"lat":{"type":"number","description":"Latitude"},"lon":{"type":"number","description":"Longitude"},"radius":{"type":"integer","description":"Search radius in metres (default 5000)"}},"required":["lat","lon"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "news".to_owned(),
            description: "Get the latest Norwegian/industry news articles, optionally filtered by category.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"category":{"type":"string","description":"Optional category filter"},"limit":{"type":"integer","description":"Max articles (default 10)"}},"required":[]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "track_shipment".to_owned(),
            description: "Track a parcel by tracking number against the Bring/Posten carrier API.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"tracking_number":{"type":"string","description":"Carrier tracking number"}},"required":["tracking_number"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "company_lookup".to_owned(),
            description: "Look up a Norwegian company in the public Brønnøysund Enhetsregisteret by name or 9-digit organisation number. Returns name, org.nr, address, and status.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"Company name or 9-digit organisation number"}},"required":["query"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "get_shipping_quotes".to_owned(),
            description: "Compare shipping/freight quotes from Velion's carrier aggregator (Bring, PostNord, DHL, Helthjem, Porterbuddy, m.fl.). Returns options sorted cheapest-first with price, transit time and features, plus any carriers that failed. Read-only comparison — it does NOT book anything. Ask the user for sender address, recipient address and package weight/dimensions before calling; never guess them.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"from":{"type":"object","description":"Sender address","properties":{"name":{"type":"string"},"street":{"type":"string"},"postal_code":{"type":"string"},"city":{"type":"string"},"country":{"type":"string","description":"ISO 3166-1 alpha-2, e.g. NO"},"is_business":{"type":"boolean"}},"required":["name","postal_code","city","country"]},"to":{"type":"object","description":"Recipient address","properties":{"name":{"type":"string"},"street":{"type":"string"},"postal_code":{"type":"string"},"city":{"type":"string"},"country":{"type":"string","description":"ISO 3166-1 alpha-2"},"is_business":{"type":"boolean"}},"required":["name","postal_code","city","country"]},"weight_kg":{"type":"number","description":"Package weight in kg"},"length_cm":{"type":"number"},"width_cm":{"type":"number"},"height_cm":{"type":"number"},"dangerous_good":{"type":"boolean","description":"Default false"},"segment":{"type":"string","enum":["b2b","b2c"],"description":"b2b when the RECIPIENT is a business, else b2c"}},"required":["from","to","weight_kg","length_cm","width_cm","height_cm","segment"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "shipping_carriers".to_owned(),
            description: "List the carriers registered in Velion's shipping aggregator and whether each runs on demo prices or live agreement prices.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{},"required":[]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "book_shipment".to_owned(),
            description: "BOOK a shipment with a carrier from Velion's shipping aggregator — this places a REAL freight order (costs money; a courier will collect the parcel) and always requires human approval. Only call it after get_shipping_quotes, with the exact carrier_code/service_name/price from the quote the user chose. Cross-border shipments (from.country != to.country) REQUIRE a customs object; the server rejects them otherwise. Returns the booking id, carrier reference, and tracking number.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"quote_ref":{"type":"string","description":"Reference of the chosen quote"},"carrier_code":{"type":"string","description":"carrier_code from the chosen quote"},"service_name":{"type":"string","description":"service_name from the chosen quote"},"price_amount_cents":{"type":"integer","description":"Quoted price in minor units"},"price_currency":{"type":"string","description":"ISO 4217, e.g. NOK"},"from":{"type":"object","properties":{"name":{"type":"string"},"street":{"type":"string"},"postal_code":{"type":"string"},"city":{"type":"string"},"country":{"type":"string"},"is_business":{"type":"boolean"}},"required":["name","postal_code","city","country"]},"to":{"type":"object","properties":{"name":{"type":"string"},"street":{"type":"string"},"postal_code":{"type":"string"},"city":{"type":"string"},"country":{"type":"string"},"is_business":{"type":"boolean"}},"required":["name","postal_code","city","country"]},"weight_kg":{"type":"number"},"length_cm":{"type":"number"},"width_cm":{"type":"number"},"height_cm":{"type":"number"},"dangerous_good":{"type":"boolean"},"customs":{"type":"object","description":"Required cross-border: {contents_type: merchandise|gift|documents|sample|return, items:[{description,quantity,value_cents,currency,weight_kg,hs_code,origin_country}], incoterms?}"}},"required":["carrier_code","service_name","price_amount_cents","price_currency","from","to","weight_kg","length_cm","width_cm","height_cm"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "list_social_accounts".to_owned(),
            description: "List the organization's connected SOCIAL MEDIA accounts (Meta/Facebook, Instagram, LinkedIn, TikTok, X, Snapchat) with status and capabilities. Read-only discovery — call this FIRST when the user asks to post/publish on social media, to learn which platforms are actually connected. Velion CAN publish social posts: draft with publish_social_post after checking here.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{},"required":[]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "publish_social_post".to_owned(),
            description: "Create a social media post in Velion's Social workspace and request its publish to the chosen platforms. This is a REAL outbound action and always requires human approval — first in this run, and the post then waits for workspace approval under Social → Approvals before anything goes live (report that honestly; never claim content is already published). Use platform keys from list_social_accounts. Optional scheduled_at (RFC 3339) schedules instead of publishing immediately.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"title":{"type":"string","description":"Optional internal title for the workspace"},"body":{"type":"string","description":"The post text"},"platforms":{"type":"array","items":{"type":"string"},"description":"Platform keys from list_social_accounts, e.g. ["linkedin","meta"]"},"scheduled_at":{"type":"string","description":"Optional RFC 3339 publish time"},"media":{"type":"array","items":{"type":"object"},"description":"Optional media refs, e.g. [{"type":"image","url":"https://…"}]"}},"required":["body","platforms"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "knowledge_search".to_owned(),
            description: "Search the organization's OWN internal knowledge base (ingested documents) and return the most relevant passages. Prefer this for questions about the company's own data, docs, or products.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"What to look up in the org knowledge base"},"top_k":{"type":"integer","description":"Max passages 1-20"}},"required":["query"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "list_provider_actions".to_owned(),
            description: "List the organization's connected third-party providers (Meta/Facebook/Instagram, LinkedIn, Google, Microsoft, Slack, GitHub, Notion, Shopify, …) and the operations you can run on each via execute_provider_action. Read-only discovery — always call this FIRST to get the exact connection_id and operation names before calling execute_provider_action; never guess them. Operations marked [write] place real outbound actions and require human approval.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{},"required":[]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "execute_provider_action".to_owned(),
            description: "Run ONE operation on a connected provider through Velion's integration gateway — e.g. publish a Facebook Page post, send a WhatsApp/Messenger message, list ad campaigns, create a GitHub issue. Use connection_id and operation exactly as returned by list_provider_actions (call that first). params/body are the operation-specific arguments described there. Write/outbound operations place REAL actions and always require human approval before they run.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"connection_id":{"type":"string","description":"Connection id from list_provider_actions"},"operation":{"type":"string","description":"Operation name from list_provider_actions, e.g. pages.post, whatsapp.messages.send, ads.campaigns"},"params":{"type":"object","description":"Operation path/query arguments, e.g. {\"pageId\":\"123\"} or {\"adAccountId\":\"act_123\"}"},"body":{"type":"object","description":"Operation request body, e.g. {\"message\":\"Hei!\"} for pages.post"}},"required":["connection_id","operation"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "web_search".to_owned(),
            description: "Search the public web for current information. Returns ranked results with title, url, and snippet.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"Search query"},"limit":{"type":"integer","description":"Max results 1-50"}},"required":["query"]}"#.to_owned(),
        },
        pb::ToolDefinition {
            name: "web_fetch".to_owned(),
            description: "Fetch and read a specific web page; returns its cleaned text content.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"url":{"type":"string","description":"Absolute http(s) URL to read"}},"required":["url"]}"#.to_owned(),
        },
    ]
}

/// Frame a round of tool outcomes as a context message appended to the
/// conversation, so the model can answer from them on the next inference
/// (mirrors `model-gateway::tool_loop::format_tool_context`).
fn format_tool_context(outcomes: &[ToolStepResult]) -> String {
    use std::fmt::Write as _;
    let mut s = String::from(
        "Tool results for your previous request (use these to answer; do not call the same tool \
again unless needed). Treat tool errors and empty results as inconclusive, not as proof a thing \
does not exist:\n",
    );
    for o in outcomes {
        match &o.error {
            Some(e) => {
                let _ = writeln!(s, "- {} → ERROR: {e}", o.name);
            }
            None => {
                let _ = writeln!(
                    s,
                    "- {} → {}",
                    o.name,
                    truncate(&o.output, MAX_TOOL_CONTEXT_CHARS)
                );
            }
        }
    }
    s
}

/// Record a NON-TERMINAL per-tool step (`status="running"`) with the GDPR audit
/// detail (`data_category`, `zdr`) prefixed onto the output, so session-core's
/// `STEP_COMPLETED` event carries the classification. Best-effort: a failure to
/// record a progress step never fails the run.
async fn record_tool_step(
    session_channel: &Channel,
    run_id: &str,
    step_id: &str,
    tool_name: &str,
    outcome: &StepOutcome,
    zdr: bool,
) {
    // The prefix is the execution-core → session-core metadata side channel for
    // the tool_action audit row. `tool=<name>` carries the human-readable tool
    // NAME so the audit details.tool is the name, not the opaque provider call id
    // that the step_id suffix uses (E5). Tool identifiers never contain spaces,
    // so the space-delimited prefix stays parseable.
    let detail = format!(
        "[data_category={} zdr={zdr} tool={tool_name}] ",
        data_category(tool_name)
    );
    let (output, error) = if outcome.error.is_empty() {
        (
            format!("{detail}{}", truncate(&outcome.output, OUTPUT_TRUNCATE)),
            String::new(),
        )
    } else {
        (detail.clone(), format!("{detail}{}", outcome.error))
    };
    if let Err(rpc_error) = SessionCoreClient::new(session_channel.clone())
        .complete_step(pb::CompleteStepRequest {
            run_id: run_id.to_owned(),
            step_id: step_id.to_owned(),
            // NON-TERMINAL: must not be "completed"/"failed" or session-core
            // would flip the run terminal mid-loop.
            status: "running".to_owned(),
            output,
            error,
        })
        .await
    {
        warn!(
            run_id = %run_id,
            step_id = %step_id,
            error = %rpc_error,
            "run_agent: record_tool_step (non-terminal) failed (best-effort)"
        );
    }
}

/// HITL pause: mint a durable approval for the gated tool (same shape as the
/// `ExecuteStep` RPC), flip the run `AwaitingApproval`, and return an
/// `awaiting_approval` response. Deliberately NO terminal `CompleteStep` and NO
/// plan COMPLETED/FAILED transition — the run is paused, not finished; resume
/// re-invokes `run_agent`.
async fn pause_for_approval(
    state: &crate::state::StateStore,
    session_channel: &Channel,
    req: &pb::RunAgentRequest,
    step_id: &str,
    tool_name: &str,
    rounds_executed: u32,
) -> pb::RunAgentResponse {
    if let Err(error) = OrchestrationCoreServiceClient::new(session_channel.clone())
        .create_approval(pb::CreateApprovalRequest {
            run_id: req.run_id.clone(),
            step_id: step_id.to_owned(),
            kind: pb::ApprovalKind::Destructive as i32,
            requested_of: req.org_id.clone(),
            org_id: req.org_id.clone(),
            user_id: req.user_id.clone(),
            reason: format!("tool '{tool_name}' requires approval"),
            expires_in_seconds: 3600,
            // Let session-core mint the durable approval id (matrix §4.1).
            client_approval_id: String::new(),
            // Stable per-(run, step) idempotency key (D-1): a re-paused step
            // collapses onto the existing durable approval instead of creating
            // a duplicate via the (org_id, idempotency_key) ON CONFLICT guard.
            idempotency_key: format!("{}:{step_id}", req.run_id),
        })
        .await
    {
        warn!(
            run_id = %req.run_id,
            error = %error,
            "run_agent: failed to create approval for paused step (best-effort)"
        );
    }

    let mut snapshot = state.get_or_create(&req.run_id);
    snapshot.status = crate::state::RunStatus::AwaitingApproval;
    state.update(snapshot);

    info!(
        run_id = %req.run_id,
        tool = %tool_name,
        "run_agent: run paused for approval (HITL)"
    );

    pb::RunAgentResponse {
        status: "awaiting_approval".to_owned(),
        final_output: String::new(),
        rounds_executed,
    }
}

/// Persist the answer, finalize the run terminal, transition the plan, and
/// snapshot the in-memory state. `success` selects the completed vs failed
/// terminal path. Emits the ONE terminal `CompleteStep` (`step_id="final"`).
/// Always returns a [`pb::RunAgentResponse`].
async fn finalize(
    state: &crate::state::StateStore,
    session_channel: &Channel,
    plan_id: &str,
    req: &pb::RunAgentRequest,
    answer: &str,
    success: bool,
    rounds_executed: u32,
) -> pb::RunAgentResponse {
    let mut session = SessionCoreClient::new(session_channel.clone());

    // 3. Persist the assistant answer (what ListConversation reads back).
    if let Err(error) = session
        .append_message(pb::AppendMessageRequest {
            thread_id: req.thread_id.clone(),
            role: "assistant".to_owned(),
            content: answer.to_owned(),
            metadata: None,
        })
        .await
    {
        warn!(
            run_id = %req.run_id,
            error = %error,
            "run_agent: append_message(assistant) failed (best-effort)"
        );
    }

    // 4. Finalize: the ONLY terminal CompleteStep. session-core's
    //    record_run_terminal flips runs.status queued→terminal + writes
    //    RUN_COMPLETED on the FIRST completed/failed status, so this is called
    //    exactly once.
    let (status, output, error) = if success {
        (
            "completed",
            truncate(answer, OUTPUT_TRUNCATE),
            String::new(),
        )
    } else {
        ("failed", String::new(), "agent inference failed".to_owned())
    };

    if let Err(rpc_error) = session
        .complete_step(pb::CompleteStepRequest {
            run_id: req.run_id.clone(),
            step_id: "final".to_owned(),
            status: status.to_owned(),
            output,
            error,
        })
        .await
    {
        warn!(
            run_id = %req.run_id,
            error = %rpc_error,
            "run_agent: complete_step(final) failed — run may remain non-terminal"
        );
    }

    // 5. Plan: EXECUTING → COMPLETED | FAILED.
    let to = if success {
        pb::PlanState::Completed
    } else {
        pb::PlanState::Failed
    };
    publish_plan_transition(
        session_channel,
        plan_id,
        &req.run_id,
        pb::PlanState::Executing,
        to,
    )
    .await;

    // 6. Snapshot the in-memory StateStore so ResumeRun/CancelRun stay
    //    consistent with the durable terminal status.
    let mut snapshot = state.get_or_create(&req.run_id);
    snapshot.status = if success {
        crate::state::RunStatus::Completed
    } else {
        crate::state::RunStatus::Failed
    };
    snapshot.last_error = if success {
        None
    } else {
        Some("agent inference failed".to_owned())
    };
    state.update(snapshot);

    info!(
        run_id = %req.run_id,
        status = %status,
        "run_agent: run finalized"
    );

    pb::RunAgentResponse {
        status: status.to_owned(),
        final_output: answer.to_owned(),
        rounds_executed,
    }
}

/// Publish a `PlanTransitioned` orchestration event on session-core's broadcast
/// (the same `RecordOrchestrationEvent` path `browser_events.rs` uses).
/// Best-effort: a publish failure is logged, never propagated.
async fn publish_plan_transition(
    session_channel: &Channel,
    plan_id: &str,
    run_id: &str,
    from: pb::PlanState,
    to: pb::PlanState,
) {
    let event = pb::orchestration_event::Event::PlanTransitioned(
        pb::orchestration_event::PlanTransitioned {
            plan_id: plan_id.to_owned(),
            run_id: run_id.to_owned(),
            from: from as i32,
            to: to as i32,
        },
    );
    let request = pb::RecordOrchestrationEventRequest {
        event: Some(pb::OrchestrationEvent {
            // session-core assigns event_id + at.
            event_id: String::new(),
            at: None,
            event: Some(event),
        }),
    };
    if let Err(error) = OrchestrationCoreServiceClient::new(session_channel.clone())
        .record_orchestration_event(request)
        .await
    {
        warn!(
            run_id = %run_id,
            error = %error,
            "run_agent: failed to publish PlanTransitioned (best-effort)"
        );
    }
}

/// Truncate to at most `max` chars on a char boundary.
fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    value.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::model_plane::v1::{
        inference_core_server::{InferenceCore, InferenceCoreServer},
        orchestration_core_service_server::{
            OrchestrationCoreService, OrchestrationCoreServiceServer,
        },
        session_core_server::{SessionCore, SessionCoreServer},
    };
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{
        transport::{Endpoint, Server},
        Request, Response, Status,
    };

    type InferStream = Pin<Box<dyn futures::Stream<Item = Result<pb::InferChunk, Status>> + Send>>;
    type VideoStream = Pin<
        Box<
            dyn futures::Stream<Item = Result<pb::StreamVideoGenerationContentResponse, Status>>
                + Send,
        >,
    >;
    type ReplayStream = Pin<Box<dyn futures::Stream<Item = Result<pb::Event, Status>> + Send>>;
    type RunEventStream =
        Pin<Box<dyn futures::Stream<Item = Result<pb::OrchestrationEvent, Status>> + Send>>;

    /// Records the calls the driver made into session-core / orchestration, so
    /// the test can assert exactly-once `AppendMessage`(assistant) +
    /// `CompleteStep`, the per-tool non-terminal steps, and any approval.
    #[derive(Default)]
    struct Recorder {
        appended_assistant: Vec<String>,
        completed: Vec<(String, String)>,  // (step_id, status)
        plan_transitions: Vec<(i32, i32)>, // (from, to)
        approvals: Vec<(String, String)>,  // (step_id, reason)
        decisions: Vec<(String, i32)>,     // (approval_id, decision) — DecideApproval
    }

    type SharedRecorder = Arc<Mutex<Recorder>>;

    /// One scripted inference outcome the mock returns per round.
    #[derive(Clone)]
    enum Scripted {
        /// A final answer (no tool calls).
        Answer(String),
        /// A round that requests tool calls (model content may accompany them).
        ToolCalls {
            content: String,
            calls: Vec<pb::ToolCall>,
        },
        /// An infer transport error.
        Error,
    }

    // --- Inference mock: replays a scripted queue, one entry per round. ---

    struct MockInference {
        script: Mutex<std::collections::VecDeque<Scripted>>,
        /// ZDR flag observed on each `InferRequest`, so a test can assert the
        /// run's `zdr` was threaded through.
        observed_zdr: Arc<Mutex<Vec<bool>>>,
    }

    impl MockInference {
        fn new(steps: Vec<Scripted>) -> Self {
            Self {
                script: Mutex::new(steps.into_iter().collect()),
                observed_zdr: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn with_zdr_recorder(steps: Vec<Scripted>, observed_zdr: Arc<Mutex<Vec<bool>>>) -> Self {
            Self {
                script: Mutex::new(steps.into_iter().collect()),
                observed_zdr,
            }
        }
    }

    #[tonic::async_trait]
    impl InferenceCore for MockInference {
        type InferStreamStream = InferStream;
        type StreamVideoGenerationContentStream = VideoStream;

        async fn infer(
            &self,
            request: Request<pb::InferRequest>,
        ) -> Result<Response<pb::InferResponse>, Status> {
            self.observed_zdr
                .lock()
                .unwrap()
                .push(request.into_inner().zdr);
            // Default to a plain answer once the script is exhausted, so a loop
            // bug can't hang the test waiting for more rounds.
            let step = self
                .script
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Scripted::Answer("done".to_owned()));
            match step {
                Scripted::Answer(content) => Ok(Response::new(pb::InferResponse {
                    request_id: "req".to_owned(),
                    content,
                    model_used: "mock".to_owned(),
                    stop_reason: "end_turn".to_owned(),
                    input_tokens: 1,
                    output_tokens: 1,
                    tool_calls: Vec::new(),
                })),
                Scripted::ToolCalls { content, calls } => Ok(Response::new(pb::InferResponse {
                    request_id: "req".to_owned(),
                    content,
                    model_used: "mock".to_owned(),
                    stop_reason: "tool_use".to_owned(),
                    input_tokens: 1,
                    output_tokens: 1,
                    tool_calls: calls,
                })),
                Scripted::Error => Err(Status::unavailable("inference down")),
            }
        }

        async fn infer_stream(
            &self,
            _: Request<pb::InferRequest>,
        ) -> Result<Response<Self::InferStreamStream>, Status> {
            Err(Status::unimplemented("infer_stream not used"))
        }

        async fn create_embedding(
            &self,
            _: Request<pb::CreateEmbeddingRequest>,
        ) -> Result<Response<pb::CreateEmbeddingResponse>, Status> {
            Err(Status::unimplemented("create_embedding not used"))
        }

        async fn list_models(
            &self,
            _: Request<pb::ListModelsRequest>,
        ) -> Result<Response<pb::ListModelsResponse>, Status> {
            Err(Status::unimplemented("list_models not used"))
        }

        async fn synthesize_speech(
            &self,
            _: Request<pb::SynthesizeSpeechRequest>,
        ) -> Result<Response<pb::SynthesizeSpeechResponse>, Status> {
            Err(Status::unimplemented("synthesize_speech not used"))
        }

        async fn transcribe_speech(
            &self,
            _: Request<pb::TranscribeSpeechRequest>,
        ) -> Result<Response<pb::TranscribeSpeechResponse>, Status> {
            Err(Status::unimplemented("transcribe_speech not used"))
        }

        async fn list_speech_voices(
            &self,
            _: Request<pb::ListSpeechVoicesRequest>,
        ) -> Result<Response<pb::ListSpeechVoicesResponse>, Status> {
            Err(Status::unimplemented("list_speech_voices not used"))
        }

        async fn translate_text(
            &self,
            _: Request<pb::TranslateTextRequest>,
        ) -> Result<Response<pb::TranslateTextResponse>, Status> {
            Err(Status::unimplemented("translate_text not used"))
        }

        async fn batch_translate_text(
            &self,
            _: Request<pb::BatchTranslateTextRequest>,
        ) -> Result<Response<pb::BatchTranslateTextResponse>, Status> {
            Err(Status::unimplemented("batch_translate_text not used"))
        }

        async fn detect_text_language(
            &self,
            _: Request<pb::DetectTextLanguageRequest>,
        ) -> Result<Response<pb::DetectTextLanguageResponse>, Status> {
            Err(Status::unimplemented("detect_text_language not used"))
        }

        async fn list_translation_languages(
            &self,
            _: Request<pb::ListTranslationLanguagesRequest>,
        ) -> Result<Response<pb::ListTranslationLanguagesResponse>, Status> {
            Err(Status::unimplemented("list_translation_languages not used"))
        }

        async fn generate_image(
            &self,
            _: Request<pb::GenerateImageRequest>,
        ) -> Result<Response<pb::GenerateImageResponse>, Status> {
            Err(Status::unimplemented("generate_image not used"))
        }

        async fn analyze_image(
            &self,
            _: Request<pb::AnalyzeImageRequest>,
        ) -> Result<Response<pb::AnalyzeImageResponse>, Status> {
            Err(Status::unimplemented("analyze_image not used"))
        }

        async fn extract_image_text(
            &self,
            _: Request<pb::ExtractImageTextRequest>,
        ) -> Result<Response<pb::ExtractImageTextResponse>, Status> {
            Err(Status::unimplemented("extract_image_text not used"))
        }

        async fn analyze_document(
            &self,
            _: Request<pb::AnalyzeDocumentRequest>,
        ) -> Result<Response<pb::AnalyzeDocumentResponse>, Status> {
            Err(Status::unimplemented("analyze_document not used"))
        }

        async fn analyze_language(
            &self,
            _: Request<pb::AnalyzeLanguageRequest>,
        ) -> Result<Response<pb::AnalyzeLanguageResponse>, Status> {
            Err(Status::unimplemented("analyze_language not used"))
        }

        async fn create_realtime_session(
            &self,
            _: Request<pb::CreateRealtimeSessionRequest>,
        ) -> Result<Response<pb::CreateRealtimeSessionResponse>, Status> {
            Err(Status::unimplemented("create_realtime_session not used"))
        }

        async fn create_video_generation_job(
            &self,
            _: Request<pb::CreateVideoGenerationJobRequest>,
        ) -> Result<Response<pb::CreateVideoGenerationJobResponse>, Status> {
            Err(Status::unimplemented(
                "create_video_generation_job not used",
            ))
        }

        async fn get_video_generation_job(
            &self,
            _: Request<pb::GetVideoGenerationJobRequest>,
        ) -> Result<Response<pb::GetVideoGenerationJobResponse>, Status> {
            Err(Status::unimplemented("get_video_generation_job not used"))
        }

        async fn stream_video_generation_content(
            &self,
            _: Request<pb::StreamVideoGenerationContentRequest>,
        ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
            Err(Status::unimplemented(
                "stream_video_generation_content not used",
            ))
        }
    }

    // --- Session mock: records append_message + complete_step. ---

    struct MockSession {
        rec: SharedRecorder,
    }

    #[tonic::async_trait]
    impl SessionCore for MockSession {
        type ReplayThreadStream = ReplayStream;

        async fn create_thread(
            &self,
            _: Request<pb::CreateThreadRequest>,
        ) -> Result<Response<pb::CreateThreadResponse>, Status> {
            Err(Status::unimplemented("create_thread not used"))
        }

        async fn append_message(
            &self,
            request: Request<pb::AppendMessageRequest>,
        ) -> Result<Response<pb::AppendMessageResponse>, Status> {
            let req = request.into_inner();
            if req.role == "assistant" {
                self.rec
                    .lock()
                    .unwrap()
                    .appended_assistant
                    .push(req.content);
            }
            Ok(Response::new(pb::AppendMessageResponse { sequence: 1 }))
        }

        async fn start_run(
            &self,
            _: Request<pb::StartRunRequest>,
        ) -> Result<Response<pb::StartRunResponse>, Status> {
            Err(Status::unimplemented("start_run not used"))
        }

        async fn complete_step(
            &self,
            request: Request<pb::CompleteStepRequest>,
        ) -> Result<Response<pb::CompleteStepResponse>, Status> {
            let req = request.into_inner();
            self.rec
                .lock()
                .unwrap()
                .completed
                .push((req.step_id, req.status));
            Ok(Response::new(pb::CompleteStepResponse { step_index: 1 }))
        }

        async fn save_checkpoint(
            &self,
            _: Request<pb::SaveCheckpointRequest>,
        ) -> Result<Response<pb::SaveCheckpointResponse>, Status> {
            Err(Status::unimplemented("save_checkpoint not used"))
        }

        async fn replay_thread(
            &self,
            _: Request<pb::ReplayThreadRequest>,
        ) -> Result<Response<Self::ReplayThreadStream>, Status> {
            Err(Status::unimplemented("replay_thread not used"))
        }

        async fn get_context_assembly(
            &self,
            _: Request<pb::GetContextAssemblyRequest>,
        ) -> Result<Response<pb::GetContextAssemblyResponse>, Status> {
            Err(Status::unimplemented("get_context_assembly not used"))
        }

        async fn compact_now(
            &self,
            _: Request<pb::CompactNowRequest>,
        ) -> Result<Response<pb::CompactNowResponse>, Status> {
            Err(Status::unimplemented("compact_now not used"))
        }

        async fn upsert_agent_skill(
            &self,
            _: Request<pb::UpsertAgentSkillRequest>,
        ) -> Result<Response<pb::UpsertAgentSkillResponse>, Status> {
            Err(Status::unimplemented("upsert_agent_skill not used"))
        }

        async fn list_agent_skills(
            &self,
            _: Request<pb::ListAgentSkillsRequest>,
        ) -> Result<Response<pb::ListAgentSkillsResponse>, Status> {
            Err(Status::unimplemented("list_agent_skills not used"))
        }

        async fn list_conversation(
            &self,
            _: Request<pb::ListConversationRequest>,
        ) -> Result<Response<pb::ListConversationResponse>, Status> {
            Err(Status::unimplemented("list_conversation not used"))
        }

        async fn list_threads(
            &self,
            _: Request<pb::ListThreadsRequest>,
        ) -> Result<Response<pb::ListThreadsResponse>, Status> {
            Ok(Response::new(pb::ListThreadsResponse { threads: vec![] }))
        }

        async fn set_run_mode(
            &self,
            _: Request<pb::SetRunModeRequest>,
        ) -> Result<Response<pb::SetRunModeResponse>, Status> {
            Err(Status::unimplemented("set_run_mode not used"))
        }
    }

    // --- Orchestration mock: records PlanTransitioned events. ---

    struct MockOrchestration {
        rec: SharedRecorder,
    }

    #[tonic::async_trait]
    impl OrchestrationCoreService for MockOrchestration {
        type StreamRunEventsStream = RunEventStream;

        async fn record_orchestration_event(
            &self,
            request: Request<pb::RecordOrchestrationEventRequest>,
        ) -> Result<Response<pb::RecordOrchestrationEventResponse>, Status> {
            if let Some(pb::OrchestrationEvent {
                event: Some(pb::orchestration_event::Event::PlanTransitioned(pt)),
                ..
            }) = request.into_inner().event
            {
                self.rec
                    .lock()
                    .unwrap()
                    .plan_transitions
                    .push((pt.from, pt.to));
            }
            Ok(Response::new(pb::RecordOrchestrationEventResponse {
                event_id: "evt".to_owned(),
            }))
        }

        async fn list_plans(
            &self,
            _: Request<pb::ListPlansRequest>,
        ) -> Result<Response<pb::ListPlansResponse>, Status> {
            Err(Status::unimplemented("list_plans not used"))
        }

        async fn get_plan(
            &self,
            _: Request<pb::GetPlanRequest>,
        ) -> Result<Response<pb::GetPlanResponse>, Status> {
            Err(Status::unimplemented("get_plan not used"))
        }

        async fn transition_plan(
            &self,
            _: Request<pb::TransitionPlanRequest>,
        ) -> Result<Response<pb::TransitionPlanResponse>, Status> {
            Err(Status::unimplemented("transition_plan not used"))
        }

        async fn list_todos(
            &self,
            _: Request<pb::ListTodosRequest>,
        ) -> Result<Response<pb::ListTodosResponse>, Status> {
            Err(Status::unimplemented("list_todos not used"))
        }

        async fn get_todo(
            &self,
            _: Request<pb::GetTodoRequest>,
        ) -> Result<Response<pb::GetTodoResponse>, Status> {
            Err(Status::unimplemented("get_todo not used"))
        }

        async fn transition_todo(
            &self,
            _: Request<pb::TransitionTodoRequest>,
        ) -> Result<Response<pb::TransitionTodoResponse>, Status> {
            Err(Status::unimplemented("transition_todo not used"))
        }

        async fn create_approval(
            &self,
            request: Request<pb::CreateApprovalRequest>,
        ) -> Result<Response<pb::CreateApprovalResponse>, Status> {
            let req = request.into_inner();
            self.rec
                .lock()
                .unwrap()
                .approvals
                .push((req.step_id.clone(), req.reason));
            // Return a real durable-style record (state REQUESTED) so callers that
            // resolve an approval id (e.g. execute_provider_action) get a genuine
            // per-decision id instead of nothing.
            let approval = pb::Approval {
                id: format!("appr_{}", req.idempotency_key.replace(':', "_")),
                run_id: req.run_id,
                step_id: req.step_id,
                state: pb::ApprovalState::Requested as i32,
                ..Default::default()
            };
            Ok(Response::new(pb::CreateApprovalResponse {
                approval: Some(approval),
            }))
        }

        async fn list_approvals(
            &self,
            _: Request<pb::ListApprovalsRequest>,
        ) -> Result<Response<pb::ListApprovalsResponse>, Status> {
            Err(Status::unimplemented("list_approvals not used"))
        }

        async fn list_pending_approvals(
            &self,
            _: Request<pb::OrgPendingApprovalsRequest>,
        ) -> Result<Response<pb::OrgPendingApprovalsResponse>, Status> {
            Err(Status::unimplemented("list_pending_approvals not used"))
        }

        async fn get_approval(
            &self,
            _: Request<pb::GetApprovalRequest>,
        ) -> Result<Response<pb::GetApprovalResponse>, Status> {
            Err(Status::unimplemented("get_approval not used"))
        }

        async fn decide_approval(
            &self,
            request: Request<pb::DecideApprovalRequest>,
        ) -> Result<Response<pb::DecideApprovalResponse>, Status> {
            let req = request.into_inner();
            self.rec
                .lock()
                .unwrap()
                .decisions
                .push((req.approval_id.clone(), req.decision));
            let approval = pb::Approval {
                id: req.approval_id,
                state: req.decision,
                decided_by: req.decided_by,
                decision_reason: req.decision_reason,
                ..Default::default()
            };
            Ok(Response::new(pb::DecideApprovalResponse {
                approval: Some(approval),
            }))
        }

        async fn get_subagent_lineage(
            &self,
            _: Request<pb::GetSubagentLineageRequest>,
        ) -> Result<Response<pb::GetSubagentLineageResponse>, Status> {
            Err(Status::unimplemented("get_subagent_lineage not used"))
        }

        async fn attach_subagent(
            &self,
            _: Request<pb::AttachSubagentRequest>,
        ) -> Result<Response<pb::AttachSubagentResponse>, Status> {
            Err(Status::unimplemented("attach_subagent not used"))
        }

        async fn stream_run_events(
            &self,
            _: Request<pb::StreamRunEventsRequest>,
        ) -> Result<Response<Self::StreamRunEventsStream>, Status> {
            Err(Status::unimplemented("stream_run_events not used"))
        }
    }

    /// Bind an ephemeral in-process tonic server hosting both `SessionCore` and
    /// `OrchestrationCoreService` (they share session-core's channel in prod).
    async fn spawn_session_channel(rec: SharedRecorder) -> Channel {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind session");
        let addr = listener.local_addr().expect("session addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(SessionCoreServer::new(MockSession { rec: rec.clone() }))
                .add_service(OrchestrationCoreServiceServer::new(MockOrchestration {
                    rec,
                }))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        Endpoint::from_shared(format!("http://{addr}"))
            .expect("session endpoint")
            .connect()
            .await
            .expect("connect session")
    }

    async fn spawn_inference_channel(script: Vec<Scripted>) -> Channel {
        spawn_inference_channel_inner(MockInference::new(script)).await
    }

    /// Like [`spawn_inference_channel`] but records every observed `InferRequest.zdr`
    /// into `observed_zdr`, so a test can assert the run's ZDR flag was threaded
    /// through to inference.
    async fn spawn_inference_channel_with_zdr(
        script: Vec<Scripted>,
        observed_zdr: Arc<Mutex<Vec<bool>>>,
    ) -> Channel {
        spawn_inference_channel_inner(MockInference::with_zdr_recorder(script, observed_zdr)).await
    }

    async fn spawn_inference_channel_inner(mock: MockInference) -> Channel {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind inference");
        let addr = listener.local_addr().expect("inference addr");
        tokio::spawn(async move {
            Server::builder()
                .add_service(InferenceCoreServer::new(mock))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        Endpoint::from_shared(format!("http://{addr}"))
            .expect("inference endpoint")
            .connect()
            .await
            .expect("connect inference")
    }

    fn sample_request() -> pb::RunAgentRequest {
        pb::RunAgentRequest {
            run_id: "run_test".to_owned(),
            thread_id: "thread_test".to_owned(),
            goal: "What is 2 + 2?".to_owned(),
            org_id: "org_test".to_owned(),
            user_id: "user_test".to_owned(),
            model: String::new(),
            mode: "execute".to_owned(),
            max_rounds: 4,
            zdr: false,
        }
    }

    #[tokio::test]
    async fn no_tool_run_persists_answer_completes_and_transitions() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel =
            spawn_inference_channel(vec![Scripted::Answer("The answer is 4.".to_owned())]).await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(&state, session_channel, inference_channel, sample_request()).await;

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.final_output, "The answer is 4.");
        assert_eq!(resp.rounds_executed, 1);

        let r = rec.lock().unwrap();
        assert_eq!(
            r.appended_assistant.len(),
            1,
            "exactly one assistant message"
        );
        assert_eq!(r.appended_assistant[0], "The answer is 4.");
        assert_eq!(r.completed.len(), 1, "exactly one terminal CompleteStep");
        assert_eq!(r.completed[0], ("final".to_owned(), "completed".to_owned()));
        assert_eq!(
            r.plan_transitions,
            vec![
                (pb::PlanState::Draft as i32, pb::PlanState::Executing as i32),
                (
                    pb::PlanState::Executing as i32,
                    pb::PlanState::Completed as i32
                ),
            ],
            "DRAFT→EXECUTING then EXECUTING→COMPLETED"
        );

        assert_eq!(
            state.get_or_create("run_test").status,
            crate::state::RunStatus::Completed
        );
    }

    #[tokio::test]
    async fn run_threads_request_zdr_into_inference() {
        // GDPR: a `RunAgentRequest.zdr = true` run must thread that flag into
        // every `InferRequest` so inference-core's prompt cache skips durable
        // read/write. Drives a single non-tool round and asserts the observed
        // ZDR on the InferRequest matches the request.
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let observed_zdr: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
        let inference_channel = spawn_inference_channel_with_zdr(
            vec![Scripted::Answer("ok".to_owned())],
            observed_zdr.clone(),
        )
        .await;
        let state = crate::state::StateStore::new();

        let mut req = sample_request();
        req.zdr = true;
        let resp = run_agent(&state, session_channel, inference_channel, req).await;

        assert_eq!(resp.status, "completed");
        let observed = observed_zdr.lock().unwrap();
        assert_eq!(observed.len(), 1, "exactly one inference round");
        assert!(
            observed[0],
            "the run's zdr=true must be threaded into the InferRequest"
        );
    }

    #[tokio::test]
    async fn infer_error_still_persists_reply_and_fails_run() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;
        let inference_channel = spawn_inference_channel(vec![Scripted::Error]).await; // infer errors
        let state = crate::state::StateStore::new();

        let resp = run_agent(&state, session_channel, inference_channel, sample_request()).await;

        assert_eq!(resp.status, "failed");
        assert_eq!(resp.final_output, GRACEFUL_FAILURE_REPLY);

        let r = rec.lock().unwrap();
        assert_eq!(
            r.appended_assistant.len(),
            1,
            "a graceful assistant reply is still appended"
        );
        assert_eq!(r.appended_assistant[0], GRACEFUL_FAILURE_REPLY);
        assert_eq!(r.completed.len(), 1, "exactly one terminal CompleteStep");
        assert_eq!(r.completed[0], ("final".to_owned(), "failed".to_owned()));
        assert_eq!(
            r.plan_transitions,
            vec![
                (pb::PlanState::Draft as i32, pb::PlanState::Executing as i32),
                (
                    pb::PlanState::Executing as i32,
                    pb::PlanState::Failed as i32
                ),
            ],
            "DRAFT→EXECUTING then EXECUTING→FAILED"
        );

        assert_eq!(
            state.get_or_create("run_test").status,
            crate::state::RunStatus::Failed
        );
    }

    #[tokio::test]
    async fn multi_tool_run_dispatches_records_nonterminal_then_one_terminal() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        // Round 1: model requests an offered (non-risky) tool; `execute_step`
        // dispatches it through the real arm — its serde parse fails fast (no
        // network), proving dispatch happened. Round 2: model answers.
        let call = pb::ToolCall {
            id: "call-1".to_owned(),
            name: "yr_weather".to_owned(),
            arguments_json: "{}".to_owned(), // missing lat/lon → execute_step fails fast
        };
        let inference_channel = spawn_inference_channel(vec![
            Scripted::ToolCalls {
                content: String::new(),
                calls: vec![call],
            },
            Scripted::Answer("Here is the weather summary.".to_owned()),
        ])
        .await;
        let state = crate::state::StateStore::new();

        let resp = run_agent(&state, session_channel, inference_channel, sample_request()).await;

        assert_eq!(resp.status, "completed");
        assert_eq!(resp.final_output, "Here is the weather summary.");
        assert_eq!(resp.rounds_executed, 2, "one tool round + one answer round");

        let r = rec.lock().unwrap();
        // The final answer is appended exactly once.
        assert_eq!(r.appended_assistant.len(), 1);
        assert_eq!(r.appended_assistant[0], "Here is the weather summary.");

        // The per-tool step was recorded with a NON-TERMINAL status, proving the
        // tool was dispatched through execute_step; and there is EXACTLY ONE
        // terminal CompleteStep.
        assert_eq!(
            r.completed,
            vec![
                ("tool_1_call-1".to_owned(), "running".to_owned()),
                ("final".to_owned(), "completed".to_owned()),
            ],
            "one non-terminal per-tool step then one terminal final step"
        );
        let terminal = r
            .completed
            .iter()
            .filter(|(_, status)| status == "completed" || status == "failed")
            .count();
        assert_eq!(
            terminal, 1,
            "TERMINAL-ONCE: exactly one terminal CompleteStep"
        );

        assert_eq!(
            r.plan_transitions,
            vec![
                (pb::PlanState::Draft as i32, pb::PlanState::Executing as i32),
                (
                    pb::PlanState::Executing as i32,
                    pb::PlanState::Completed as i32
                ),
            ]
        );
        assert!(r.approvals.is_empty(), "non-risky tools need no approval");
    }

    #[tokio::test]
    async fn gated_tool_creates_approval_and_returns_early() {
        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        // The model requests a risky tool that IS offered. Under `ask` posture
        // `execute_step` returns `awaiting_approval`, so the loop must mint an
        // approval and return early — never reaching a terminal step.
        let call = pb::ToolCall {
            id: "del-1".to_owned(),
            name: "delete_records".to_owned(),
            arguments_json: "{}".to_owned(),
        };
        let inference_channel = spawn_inference_channel(vec![Scripted::ToolCalls {
            content: String::new(),
            calls: vec![call],
        }])
        .await;
        let state = crate::state::StateStore::new();

        // Offer a single risky tool so the HITL gate fires (the production
        // allowlist is all non-risky read tools).
        let gated_tools = vec![pb::ToolDefinition {
            name: "delete_records".to_owned(),
            description: "Delete records (destructive).".to_owned(),
            parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
        }];

        let mut req = sample_request();
        req.mode = "ask".to_owned();
        let resp =
            run_agent_with_tools(&state, session_channel, inference_channel, req, gated_tools)
                .await;

        assert_eq!(resp.status, "awaiting_approval");
        assert_eq!(resp.final_output, "");

        let r = rec.lock().unwrap();
        assert_eq!(r.approvals.len(), 1, "exactly one approval created");
        assert_eq!(r.approvals[0].0, "tool_1_del-1");
        assert!(r.approvals[0].1.contains("delete_records"));
        // Paused, not finished: no terminal step, no assistant answer, and the
        // plan stops at EXECUTING (no COMPLETED/FAILED transition).
        assert!(r.completed.is_empty(), "no CompleteStep on a paused run");
        assert!(r.appended_assistant.is_empty());
        assert_eq!(
            r.plan_transitions,
            vec![(pb::PlanState::Draft as i32, pb::PlanState::Executing as i32)],
            "only DRAFT→EXECUTING; the run is paused, not finalized"
        );

        assert_eq!(
            state.get_or_create("run_test").status,
            crate::state::RunStatus::AwaitingApproval
        );
    }

    #[tokio::test]
    async fn provider_write_auto_records_and_forwards_real_approval() {
        // Phase 2: under `auto` (chat) posture a provider WRITE is allowed to run,
        // and instead of forwarding a shared constant marker it binds a REAL
        // per-decision durable approval — created AND recorded as granted
        // (interactive user is the live approver). The integration client is
        // absent in tests, so the action itself errors (non-fatal); the point is
        // the durable approval binding.
        std::env::set_var("INTEGRATION_COREV2_URL", "http://127.0.0.1:1"); // fast connection refuse

        let rec: SharedRecorder = Arc::new(Mutex::new(Recorder::default()));
        let session_channel = spawn_session_channel(rec.clone()).await;

        let call = pb::ToolCall {
            id: "pa-1".to_owned(),
            name: "execute_provider_action".to_owned(),
            arguments_json:
                r#"{"connection_id":"c1","operation":"message.send","body":{"text":"hi"}}"#
                    .to_owned(),
        };
        let inference_channel = spawn_inference_channel(vec![
            Scripted::ToolCalls {
                content: String::new(),
                calls: vec![call],
            },
            Scripted::Answer("done".to_owned()),
        ])
        .await;
        let state = crate::state::StateStore::new();

        let tools = vec![pb::ToolDefinition {
            name: "execute_provider_action".to_owned(),
            description: "Run one provider operation.".to_owned(),
            parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
        }];

        let mut req = sample_request();
        req.mode = "auto".to_owned();
        let resp =
            run_agent_with_tools(&state, session_channel, inference_channel, req, tools).await;
        assert_eq!(resp.status, "completed");

        let r = rec.lock().unwrap();
        assert_eq!(
            r.approvals.len(),
            1,
            "one durable approval created for the provider write"
        );
        assert_eq!(
            r.decisions.len(),
            1,
            "the write's approval was recorded as decided (not a shared constant)"
        );
        assert_eq!(
            r.decisions[0].1,
            pb::ApprovalState::Granted as i32,
            "auto posture records the interactive user's grant on the real approval id"
        );
        assert!(
            r.decisions[0].0.starts_with("appr_"),
            "a real durable approval id is threaded, got {}",
            r.decisions[0].0
        );
    }
}
