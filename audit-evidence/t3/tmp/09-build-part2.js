const MP = "apps/Model Plane/rust";
const FP = "apps/Frontend Plane/verevonv3";
module.exports = [
{
  id: "R-t3-09-06",
  title: "Role table and purpose tags for every inference call, plus multimodal input routing",
  summary: "Add a role dimension to RoutingPolicy (chat, planner, worker, subagent:type, title, follow_ups, verification_judge, compaction, memory_extract, vision, scheduled) mapping each role to a model or intent mode, effort and fallback, and replace the hard-coded gpt-4o-mini constants with role lookups. Carry a role/purpose tag in the inference request so metering, cost and ZDR audit can attribute auxiliary calls. On top, add a gateway pre-processor driven by the model record: native image parts when the model accepts them, describe-first through the vision role when it does not, with per-request image budgets and a visible image-offload notice.",
  whyItMatters: "Background inference gets cheaper and policy-consistent, and Verevon can route perception or long-context subagents to the best provider (for example Gemini for video) without touching the main loop. Multimodal parity lets images and documents join agent tool loops on any routed model.",
  referenceBestPractice: {
    bestSource: "hermes (hermes-architecture-and-surfaces-099 roles, 104 image input mode); deepseek (044, 157 purpose tags)",
    others: ["antigravity (084, ModelTier per subagent and system job)", "codexagent", "cursor (109, perception as subagent)", "t3code", "web-perplexity"],
    prevalence: "6 of 12 for role slots (common); 4 of 12 for multimodal routing (common); 1 of 12 for purpose tags (unique)",
    mechanism: "auxiliary.<task>.{provider, model, effort} for about 20 side tasks resolved through the same runtime resolver as the main model; GenerateOptions.purpose lets adapters attach transport metadata without changing model input; decide_image_input_mode picks native versus describe-first once per turn from the main model capability. [SRC]"
  },
  verevonToday: {
    status: "partial",
    evidence: [
      MP + "/services/model-gateway/src/sse.rs:7839-7894 (verified: TITLE_MODEL, FOLLOW_UPS_MODEL, VERIFICATION_JUDGE_MODEL are gpt-4o-mini constants)",
      MP + "/services/model-gateway/src/deep_research.rs:263",
      MP + "/services/execution-core/src/llm_planner.rs:113-148",
      MP + "/services/inference-core/src/metered.rs:1-8",
      MP + "/services/inference-core/src/provider/vision.rs (image generation, analysis and OCR provider; wiring into the chat tool loop not verified)"
    ],
    gap: "Side tasks exist but their models are constants and the planner resolves its own model; there is no editable role table. Whether calls carry a purpose tag, whether non-vision models get a describe pre-step, and whether image budgets or offload notices exist are unknown (no baseline record): check how attachments reach tool loops in model-gateway."
  },
  approach: "inference-core: role enum and a role-to-route table inside RoutingPolicy (routing_policy.rs), resolved through the same intent/fallback path so the privacy floor and ZDR apply equally; default every role to today's constant so nothing changes on day one. Add role/purpose to the inference proto and record it in the metered decorator and cost entries. model-gateway: replace the constants in sse.rs and deep_research.rs; attachment pre-processor reading input_modalities from the model record, Data Plane documents passed as handles resolved by a read tool. Admin edits the role table in the existing router-policy page (apps/gateway/src/domains/router_policy.rs).",
  effort: "M",
  dependencies: ["RoutingPolicy schema change", "R-t3-09-05 model record (input modalities)", "Data Plane document handles"],
  planeOwner: "Model Plane",
  risks: "Misconfigured roles can raise cost (default to current constants); describe-first loses detail (show which path was used); do not send org identifiers to providers unless contracts allow it.",
  tier: "recommended", significance: 3, confidence: "medium",
  alreadyPlanned: { is: false, where: "" },
  needsVerification: true,
  coversClusters: ["C-t3-09-008", "C-t3-09-039", "C-t3-09-009"],
  extraSrc: 1
},
{
  id: "R-t3-09-07",
  title: "Per-step routing decision and model continuity: resolver, lifecycle successors and a reasoning replay envelope",
  summary: "Implement the planned step-level RoutingDecision {requested, resolved model, provider, effort, reason, complexity, budget posture} in inference-core, persist it on every run step in Session Core, and surface model and reason in the served-by line with an optional routing-feedback control. Build one resolve_model() with fixed precedence (explicit, thread last step, Space default, user last used, org default, intent default) that chat, Do, scheduled runs and subagents all call; add lifecycle states and successor ids so retired model ids resolve to successors with a model_switched event; insert a notice only when the provider family changes. Capture Anthropic signature and redacted thinking blocks (and later Gemini thought signatures) in a ReplayEnvelope stored with the step and replay them only to the matching adapter and model.",
  whyItMatters: "Serves the DeepSeek-style transparency goal, gives the router-policy page real data to tune Budget/Balance/Genius, and protects saved threads, agents and scheduled runs from provider model retirements. Replay is required for correct multi-turn tool use with thinking models and is a blocker for Gemini.",
  referenceBestPractice: {
    bestSource: "hermes (097 reasoning replay, 093 single resolver); cursor (073, 108 routed model); codexagent (035 lifecycle and upgrade target); deepseek (094, 134 mid-thread notice)",
    others: ["t3code (057, 003, 097)", "antigravity", "web-claudeai (085)"],
    prevalence: "replay 4 of 12 (common); routed-model stream 2 of 12, lifecycle 3 of 12, switching semantics 2 of 12, layered defaults 2 of 12 (differentiators)",
    mechanism: "reasoning_carriers stores opaque reasoning as typed private records lifted back only for the matching route, with fingerprints of signature-rejected blocks excluded on retry; the agent stream emits routed_model with a display name; the catalogue declares upgrade {model, migration note, retirement_at} and a thread switches automatically after retirement; a durable notice tells the new model earlier turns came from another model. [SRC]"
  },
  verevonToday: {
    status: "partial",
    evidence: [
      MP + "/services/model-gateway/src/sse_events.rs:39-60 (ServedBy carries provider deployment, residency and privacy tier but no resolved model or reason)",
      FP + "/src/features/chat/components/chat-types.ts:290-297",
      MP + "/services/inference-core/src/provider/anthropic.rs:378-396 (verified: signature_delta is discarded)",
      MP + "/services/inference-core/src/config.rs:83 (only a comment about a retired Cohere model name)",
      FP + "/src/shared/api/chat-client.ts:808-866 (per-org remembered choice)"
    ],
    gap: "Routing is request-level only (step-level RoutingDecision is planned), the served-by line has no model id or reason, signed thinking cannot be replayed, and there is no routing feedback. Lifecycle handling, per-message or per-thread model storage in Session Core, behaviour when a saved model is no longer routable, and a single resolver for chat, Do and scheduled runs have no baseline record (unknown); the Space default does not exist."
  },
  approach: "inference-core: RoutingDecision per call plus a resolve_model(ctx) used by every surface; lifecycle {state, retire_at, successor_id} in the model record (R-t3-09-05). Session Core: extend the step schema with the route and an optional ReplayEnvelope {adapter, model, blocks}; skip persisting envelopes on ZDR turns. model-gateway: routed_model / model_switched SSE events, a small router-feedback endpoint, an org setting to show tier names only; strip replay state and add a notice on family change. Application Plane projection carries the Space default; it can only select among already-entitled models.",
  effort: "L",
  dependencies: ["Session Core step schema", "R-t3-09-05 model record (lifecycle fields)", "R-t3-09-02 (entitlements filter the resolved model)"],
  planeOwner: "Model Plane (with Session Core)",
  risks: "Reasoning content retention must follow ZDR; automatic successor mapping can change cost or behaviour (record it per run and show it); notices consume tokens and can break caches (insert only on family change); exposing model names may conflict with abstraction choices.",
  tier: "recommended", significance: 3, confidence: "medium",
  alreadyPlanned: { is: true, where: "Step-level RoutingDecision is planned in docs/MODEL_PLANE_IMPROVEMENTS_2026.md section 9 (v-modelplane baseline line 245)" },
  needsVerification: true,
  coversClusters: ["C-t3-09-007", "C-t3-09-003", "C-t3-09-020", "C-t3-09-021", "C-t3-09-022"],
  extraSrc: 1
},
{
  id: "R-t3-09-08",
  title: "Resilient routing: failure vocabulary, credential pools, benching, usage-limit and refusal fallbacks, provider health",
  summary: "Extend the shipped fallback chain in inference-core with a closed failure code set (auth, quota, rate limit, context too long, server, timeout, refusal, empty response) carrying retry_after, per-route retry policy, pooled deployments or credentials with model-scoped cooldowns, and benching of a failed primary until its declared reset. Add declared fallback models for usage-limit exhaustion with a switch-back at reset_at (opt-in when it crosses from a user subscription to platform billing), a policy-owned refusal fallback that records the original model and is undone on rewind or edit, and a per-provider health map exposed through ListModels so the picker greys unavailable routes with the reason.",
  whyItMatters: "Fewer failed answers and stalled agent runs during provider incidents or quota exhaustion, which matters most for unattended scheduled agents and Azure deployment quotas. The subscription route currently stops at its limit with a typed message but no alternative.",
  referenceBestPractice: {
    bestSource: "hermes (hermes-architecture-and-surfaces-095, 094 failover and credential pools); codexagent (036 usage-limit fallback and switch-back); claude (112 reversible refusal fallback); t3code (002, 050 provider health snapshot)",
    others: ["deepseek (126, 095 closed failure vocabulary and per-route retry modes)", "web-chatgpt", "web-claudeai"],
    prevalence: "2 to 3 of 12 per facet; differentiator",
    mechanism: "try_activate_fallback() rebuilds the client in place; cooldowns follow the provider-declared reset (exponential 60 s to 4 h when absent); credential pools rotate on plan-limit 429s before switching provider; limit errors carry reset_at, blocked model and fallback models, and the client announces switches and switch-backs; a model_refusal_fallback message switches the model and remembers the original. [SRC]"
  },
  verevonToday: {
    status: "partial",
    evidence: [
      MP + "/services/inference-core/src/provider/fallback.rs:505-700",
      MP + "/services/inference-core/src/provider/overflow.rs:100",
      MP + "/services/model-gateway/src/subscription_failure.rs:195-215",
      MP + "/services/inference-core/src/http_health.rs:1-6"
    ],
    gap: "A fallback chain with bounded retries and retry-after, typed context overflow, and typed subscription refusals with a reset time exist. Missing: credential pool, reset-aware benching, failover on the subscription route, automatic fallback model and switch-back at limit, org-quota fallback. Refusal-driven rerouting and whether provider health or quota state reaches /v1/models have no baseline record (unknown)."
  },
  approach: "inference-core fallback.rs: add the failure enum and per-route retry policy, a cooldown map keyed by (credential, model), benching with reset_at, and reroute events written into the RoutingDecision (R-t3-09-07). Every hop is filtered through the privacy floor, entitlements and cost gates. subscription route: declared fallback models plus a user consent setting for platform billing. Provider health map feeds the model record availability field and the picker. Refusal fallback lists live in RoutingPolicy.",
  effort: "L",
  dependencies: ["R-t3-09-07 RoutingDecision and reroute events", "R-t3-09-05 availability field", "user consent setting for subscription-to-platform fallback"],
  planeOwner: "Model Plane",
  risks: "Failover can silently cross privacy or cost lines (gate and display every hop); falling back from a user subscription to platform billing must be opt-in; stale health data can hide a working route (short-lived cheap probes); safety rerouting must always be disclosed.",
  tier: "recommended", significance: 3, confidence: "medium",
  alreadyPlanned: { is: true, where: "BKL #25 multi-provider auth-profile failover (docs/verevon_CHAT_PARITY_BACKLOG.md, v-modelplane baseline line 349)" },
  needsVerification: true,
  coversClusters: ["C-t3-09-017", "C-t3-09-018", "C-t3-09-019", "C-t3-09-034"],
  extraSrc: 0
},
{
  id: "R-t3-09-09",
  title: "Cost by token category, live credit meter, quota display and a context ring",
  summary: "Extend the usage record with disjoint uncached input, cache read, cache write, output and reasoning tokens plus role/tool attribution, price each category in cost-core, and aggregate per turn, per subagent and per thread for the answer details popover and Work panel. Add the planned live credit meter fed by cost-core, per-model or per-family remaining quota with reset time from Control quotas, and a pre-run price confirmation above a threshold. Add a per-model compaction policy in the model record and a context ring with a manual Compact action driven by a context snapshot event.",
  whyItMatters: "Users and admins see spend before and during long agent runs, which is the main fear with autonomous agents, and it delivers the maximum-information, transparent goal. Context pressure becomes visible instead of silent.",
  referenceBestPractice: {
    bestSource: "antigravity (antigravity-agent-ux-and-gemini-088 cost, 087 quota)",
    others: ["deepseek (043 disjoint-count convention, 096 published compaction trigger formula)", "cursor (079 pre-run price check)", "t3code (032 context ring with manual compaction)", "hermes (073)", "web-chatgpt", "web-perplexity"],
    prevalence: "cost 4 of 12 (common); quota 3 of 12 (differentiator); context management 4 of 12 (common)",
    mechanism: "CostDisplaySection {input, cache_read, output, thinking, tools, tool_breakdown, estimated_cost_usd} with per-turn, per-subagent and cumulative totals; QuotaInfo {remaining_fraction, reset_time} per model; a context snapshot {used, max, auto_compact_at} rendered as a ring with a Compact button. [SRC]"
  },
  verevonToday: {
    status: "partial",
    evidence: [
      MP + "/services/model-gateway/src/pricing.rs:206-257",
      FP + "/src/features/chat/components/ChatPanels.tsx:926-940",
      MP + "/crates/mp-tokens/src/lib.rs:1-5",
      MP + "/services/inference-core/src/provider/anthropic.rs:473-613",
      FP + "/src/features/chat/components/ChatPanels.tsx:571-720",
      MP + "/services/inference-core/src/provider/routing_policy.rs:127-222"
    ],
    gap: "The usage event carries tokens and cost-core cost (null when unknown) and cache tokens are priced, but there is no reasoning-token category, per-tool or per-subagent breakdown, cumulative thread total, live per-run meter (DOC: not built), per-model quota display or pre-run confirmation. Context: BPE counting, typed overflow and an assembled-context inspector exist, but no per-model declared compaction policy, no published trigger formula, no composer ring or manual compact. The inference-core metering decorator is reported built but not deployed (baseline DOC, ledger 2026-10-06)."
  },
  approach: "inference-core and cost-core: extend the usage record and pricing categories, aggregate by role and subagent in model-gateway and emit a usage/meter event during runs; Control Plane quotas API for remaining budget; model-gateway admission compares a pre-run estimate to budget and asks for confirmation above a threshold. Frontend Plane: render in the existing answer details popover and Work panel, add the ring to the composer. Label all figures as estimates and reconcile with Azure actual cost.",
  effort: "M",
  dependencies: ["R-t3-09-06 purpose tags", "cost-core live aggregation", "Control Plane quotas API", "R-t3-09-05 model record (context fields)", "metering decorator deployed"],
  planeOwner: "Model Plane (with cost-core, Control quotas and Frontend Plane)",
  risks: "Live estimates during streaming are approximate and may differ from invoices; label as estimates; ensure the metering decorator is actually deployed before relying on per-call attribution.",
  tier: "recommended", significance: 3, confidence: "high",
  alreadyPlanned: { is: true, where: "Live credit meter planned (warn at 80 percent, OD19; v-planned-decisions baseline lines 121 and 228)" },
  needsVerification: false,
  coversClusters: ["C-t3-09-027", "C-t3-09-028", "C-t3-09-036"],
  extraSrc: 0
},
{
  id: "R-t3-09-10",
  title: "Declarative provider registry and bring-your-own connections",
  summary: "Turn provider registration from environment configuration into data: a ProviderProfile (wire protocol, base and models URL, auth kind, compat switches, retry and timeout budgets, residency, privacy tier) stored by inference-core, added through a schema-generated admin form, with model discovery that proposes entries without mutating config until saved and an admin overlay (custom model ids, hide, reorder, unverified capability overrides). Then generalise the ChatGPT-subscription pattern into BYO connections held by Integration Core (API key, cloud role or helper command for Bedrock, Vertex, Azure Foundry, or a customer gateway), referenced by opaque connection id, with per-connection routing so a user or org can hold several connections of one provider.",
  whyItMatters: "Enterprise and sovereign customers can add their own gateway, on-prem endpoint or cloud tenancy without a Verevon release, and heavy users can bring their own keys. It also covers the local-model need in a form appropriate for a cloud product.",
  referenceBestPractice: {
    bestSource: "hermes (hermes-architecture-and-surfaces-091, 41 overridable provider profiles); claude desktop (claude-product-ux-107, claude-runtime-architecture-072 for BYO cloud accounts and helper credentials with TTL)",
    others: ["deepseek (090, 093, 099, 100)", "codexagent (037, tight 21-field provider record)", "t3code (001, 032, 086 instances and catalogue curation)", "cursor (096 team gateway)", "antigravity", "manus", "web-chatgpt", "web-claudeai"],
    prevalence: "registry 5 of 12 (common); BYO 6 of 12 (table-stakes); multi-instance 1 of 12; curation 2 of 12",
    mechanism: "A ProviderProfile dataclass per provider with api mode, auth type, base and models URLs and error classification, overridable by a user profile of the same name; BYO credentials come from key, cloud role or a helper script with TTL and silent refresh, with health probes on save, and platform and customer credentials never fall back to each other. [SRC]"
  },
  verevonToday: {
    status: "partial",
    evidence: [
      MP + "/services/inference-core/src/config.rs:209-214",
      MP + "/services/inference-core/src/config.rs:286-326",
      MP + "/services/inference-core/src/provider/codex_subscription.rs:1-60",
      FP + "/src/shared/api/chatgpt-subscription-client.ts:1",
      MP + "/services/model-gateway/src/sse_events.rs:40-41"
    ],
    gap: "Providers come from environment configuration (INFERENCE_PROVIDER_ORDER plus fixed adapters). Only the user-owned ChatGPT subscription route exists as a connection; there is no admin-defined provider record, compat switches, discovery, add-provider UI, API-key BYOK, customer cloud account or customer gateway. Whether a user or org can hold several connections of one provider, and any per-org model visibility beyond the router-policy table, have no baseline record (unknown)."
  },
  approach: "inference-core: provider-profile table with secrets referenced, never stored; discovery calls GET /models; profiles flow into the same fallback chain, privacy-tier gate and metered decorator. Customer-declared profiles default to the lowest privacy tier and no ZDR claim until attested. Integration Core: new credential kinds and short-lived token minting per call; Model Plane receives only an opaque connection id. cost-core: customer-billed flag. Frontend Plane: schema-annotated settings form. Phase 1 registry plus curation, phase 2 BYO.",
  effort: "XL",
  dependencies: ["Integration Core credential kinds and token minting", "cost-core customer-billed flag", "R-t3-09-05 model record", "ZDR and residency attestation flow"],
  planeOwner: "Model Plane (registry) with Integration Core (credentials)",
  risks: "Unverified endpoints can claim residency or ZDR they do not have; customer endpoints may log prompts; confusing which account paid (show the connection on the served-by line); wrong capability overrides cause failures (mark unverified and log).",
  tier: "recommended", significance: 3, confidence: "high",
  alreadyPlanned: { is: false, where: "" },
  needsVerification: true,
  coversClusters: ["C-t3-09-012", "C-t3-09-013", "C-t3-09-016", "C-t3-09-035"],
  extraSrc: 0
},
{
  id: "R-t3-09-11",
  title: "Router tuning loop: eval gate for RoutingPolicy changes, experiment ids and opt-in best-of-N",
  summary: "Add a Model Plane eval job (Go durable tier) that runs a fixed task suite per (role, candidate model, effort) and records quality, cost and latency; require a passing comparison against the current table before a production org's RoutingPolicy edit is accepted, and publish results in the router-policy page. Generalise RoutingPolicy storage in Session Core into a typed config overlay with versions and a closed list of overridable keys, and stamp the overlay version and experiment ids on every run. As a later phase, add opt-in best-of-N: fork one turn to N routes with a cost cap, record the user's preferred answer and reasons, and feed that into the evals.",
  whyItMatters: "The router is a core Verevon idea and the routing table is already runtime-editable, so it needs evidence to be safe to change and to justify defaults to customers. Preference data and experiment attribution make tuning empirical.",
  referenceBestPractice: {
    bestSource: "codexagent (codexagent-agent-runtime-and-models-091 model-migration playbook); antigravity (091 best-of-N battle mode, 090 experiment overrides)",
    others: ["web-perplexity (085 published cost-per-task comparisons)", "deepseek", "claude (071 rule-table flags)"],
    prevalence: "3 of 12 for evals (differentiator); 2 of 12 for remote config; 1 of 12 for best-of-N (unique)",
    mechanism: "An embedded skill inventories every model usage site, classifies it, maps flagship, balanced and fast roles and validates against a matrix rather than blind string replacement; ExperimentKey overrides with experiment ids stamped on every execution; StartBattleModeRequest forks N runs with preference telemetry. [SRC for Codex and Antigravity, DOC for Perplexity]"
  },
  verevonToday: {
    status: "partial",
    evidence: [
      MP + "/services/inference-core/src/provider/routing_policy.rs:127-222",
      FP + "/apps/gateway/src/domains/router_policy.rs:30-66"
    ],
    gap: "Only the routing table is runtime-editable; prompts, loop budgets and defaults need a deploy and no experiment ids are stamped on runs. Whether the Model Plane eval-lab covers routing changes or any RoutingPolicy edit is gated on an evaluation has no baseline record (unknown). Side-by-side comparison is absent (searched compareModels, arena and side-by-side in the SPA and model-gateway; none found)."
  },
  approach: "Go durable tier: eval job over a task suite, results stored with an eval id; Session Core: versioned config overlay; gateway PUT for RoutingPolicy requires a stored eval id for production orgs. Refresh suites only from anonymised, consented, non-ZDR traces. Best-of-N: model-gateway fan-out with cost cap and confirmation, preference records in Session Core, Solid split view; opt-in only and the cost shown.",
  effort: "L",
  dependencies: ["task suite", "R-t3-09-06 role table", "R-t3-09-07 RoutingDecision", "Session Core config store"],
  planeOwner: "Model Plane",
  risks: "Eval suites drift from real usage; best-of-N multiplies cost; config sprawl (keep a closed key list); never use ZDR traces.",
  tier: "recommended", significance: 3, confidence: "medium",
  alreadyPlanned: { is: true, where: "Eval architecture proposed in docs/MODEL_PLANE_IMPROVEMENTS_2026.md section 19 (v-modelplane baseline line 329)" },
  needsVerification: true,
  coversClusters: ["C-t3-09-032", "C-t3-09-031", "C-t3-09-038"],
  extraSrc: 0
},
{
  id: "R-t3-09-12",
  title: "Second-order latency and cache levers: speed tier, prewarm, cache-preserving in-band changes, Responses continuation",
  summary: "After the cache and effort basics are measured, add optional levers in this order: a user-facing speed tier (Standard or Fast) declared in the model record and mapped to provider priority processing with a cost multiplier in cost-core; a prewarm call on composer focus (connection warm-up first, cache priming only if measured to pay) with deadline-based optional context sources and a context-status block; append-only in-band updates for changed prompts, effort and added tools, with background forks reusing the exact parent prefix; and, last, an OpenAI Responses adapter mode with incremental continuation for non-ZDR turns.",
  whyItMatters: "Lower time to first token and cost on long sessions and connector-heavy threads. Plumbing already exists for the priority tier on the Codex route, and turn_timing gives a baseline to prove any gain.",
  referenceBestPractice: {
    bestSource: "codexagent (034 service tiers, 038 Responses WebSocket); cursor (134, 110 prewarm and progressive hydration); deepseek (037, 038 in-history updates)",
    others: ["hermes (103 time-bounded fast windows, 058 cache-parity review forks)", "web-chatgpt"],
    prevalence: "speed tier 3 of 12, in-band changes 3 of 12 (differentiators); prewarm 1 of 12, Responses WebSocket 1 of 12 (unique)",
    mechanism: "Per-model service_tiers with multipliers and a default tier; a request-header tool diff that emits one developer message instead of rewriting the tool list; prewarm_request mirroring a run request without an action; previous_response_id continuation with HTTPS fallback. [SRC]"
  },
  verevonToday: {
    status: "partial",
    evidence: [
      MP + "/services/inference-core/src/provider/mod.rs:155",
      MP + "/services/inference-core/src/provider/codex_subscription.rs:143",
      MP + "/services/model-gateway/src/tool_loop.rs:6760",
      MP + "/services/model-gateway/src/source_validation.rs:2281",
      MP + "/services/model-gateway/src/latency_levers.rs:1-20 (existing env kill-switch pattern for latency levers)"
    ],
    gap: "A prefer_priority_service_tier flag exists (Codex route, some internal gateway paths) but there is no user-facing speed control, no tier on platform OpenAI routes and no cost multiplier. Prewarm, append-only prompt or tool updates, parent-prefix reuse by background calls, and Responses continuation have no baseline record (a grep for prewarm and previous_response_id in Model Plane Rust found nothing): check how a mid-thread connector or skill addition changes the tool list."
  },
  approach: "Follow the latency_levers.rs kill-switch pattern for each lever. Speed tier: model-record service_tiers plus composer toggle, price in cost-core. Prewarm: model-gateway endpoint plus deadline-bounded context sources, using turn_timing to prove a gain. In-band changes: header diffing (system_hash, tools_hash, effort) in prompt_context, enabled per model only where tested. Responses mode: optional adapter flag, rebuildable from Session Core history and disabled on ZDR turns.",
  effort: "L",
  dependencies: ["R-t3-09-03 measured cache baseline", "R-t3-09-05 model record", "turn_timing telemetry", "OpenAI Responses adapter (last phase)"],
  planeOwner: "Model Plane",
  risks: "Cost surprises from the fast tier (show the multiplier); prewarm cache-priming spends tokens; provider-held state (previous_response_id) conflicts with ZDR and durable Session Core history, so restrict to non-ZDR turns; provider-specific in-band semantics.",
  tier: "nice-to-have", significance: 2, confidence: "medium",
  alreadyPlanned: { is: false, where: "" },
  needsVerification: true,
  coversClusters: ["C-t3-09-030", "C-t3-09-025", "C-t3-09-024", "C-t3-09-026"],
  extraSrc: 1
}
];
