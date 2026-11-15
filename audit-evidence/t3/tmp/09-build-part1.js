const MP = "apps/Model Plane/rust";
const FP = "apps/Frontend Plane/verevonv3";
module.exports = [
{
  id: "R-t3-09-01",
  title: "Canonical reasoning-effort ladder mapped per model to each provider wire format",
  summary: "Define one canonical effort scale (none, minimal, low, medium, high, xhigh) and a single resolve_effort() in inference-core that every provider adapter calls: clamp the request to the nearest weaker level the model supports, then map it to the provider vocabulary (OpenAI/Azure reasoning effort, Anthropic thinking budget, Codex reasoning_effort, later Gemini thinking level). Start with a static per-model table and move it into the model record (R-t3-09-05) later; keep the composer's Auto / Raskt svar / Dyp research dial as presets over the canonical scale.",
  whyItMatters: "Budget/Balance/Genius defaults are mostly OpenAI-family models, and that route currently sends no reasoning effort at all, so the user's depth choice is ignored on the most-used path. This is a code-verified gap with a small, well-understood fix that six of twelve references implement.",
  referenceBestPractice: {
    bestSource: "hermes (hermes-architecture-and-surfaces-096)",
    others: ["codexagent (032, best declared per-model ladder with descriptions)", "deepseek (092, refuses unsupported values before dispatch)", "t3code (005, renders controls from descriptors)", "claude", "web-claudeai"],
    prevalence: "6 of 12 sources; table-stakes",
    mechanism: "An EFFORT_LADDER plus named per-generation wire vocabularies and clamp_effort() that picks the nearest weaker supported level instead of sending a value that returns 400; capability probes are cached per (model, base_url). The composer renders only the levels the selected model declares. [SRC for Hermes/Codex/DeepSeek]"
  },
  verevonToday: {
    status: "partial",
    evidence: [
      MP + "/services/model-gateway/src/thinking.rs:1-36",
      MP + "/services/inference-core/src/provider/anthropic.rs:402-428",
      MP + "/services/inference-core/src/provider/codex_subscription.rs:142-156",
      "grep of inference-core/src/provider/openai.rs finds no reasoning_effort (verified read-only); only codex_subscription.rs sets it"
    ],
    gap: "Quick/Standard/Deep profiles produce an Anthropic thinking budget and a Codex low/high effort only. The OpenAI and Azure chat path sends no effort, there is no per-model declared ladder, no clamp-to-nearest rule, and no per-role or per-surface cap."
  },
  approach: "Model Plane: add an Effort enum and resolve_effort(model, requested, role_cap) in inference-core/src/provider (shared by openai.rs, anthropic.rs, codex_subscription.rs); wire OpenAI/Azure reasoning models first and declare 'none' for non-reasoning ids such as gpt-4o-mini; keep model-gateway/src/thinking.rs profiles as the preset layer that picks a canonical level instead of a raw budget. Log the clamp on the run and show the cost multiplier for high levels. Composer reads supported levels from the model record once R-t3-09-05 exists.",
  effort: "S",
  dependencies: ["R-t3-09-05 model record (optional; can start with a static table)"],
  planeOwner: "Model Plane",
  risks: "Higher effort raises cost and latency; tie default levels to budget posture and show the multiplier. Do not send effort to models that reject the parameter.",
  tier: "priority", significance: 4, confidence: "high",
  alreadyPlanned: { is: false, where: "Named as a gap in the v-modelplane baseline (no OpenAI reasoning-effort mapping); no plan found" },
  needsVerification: false,
  coversClusters: ["C-t3-09-005"],
  extraSrc: 1
},
{
  id: "R-t3-09-02",
  title: "Org and role model entitlements: Control Plane decides, Model Plane enforces at admission",
  summary: "Give the refused allowed_models policy field a real owner: a Control Plane entitlement policy (org, role, group, plan) with allow/deny model lists, maximum effort, admin default model/effort per surface, and an explain endpoint for one member's effective access. Model Plane receives the resolved entitlement in the auth context, filters ListModels, and refuses disallowed models at admission with a typed reason, never silently substituting.",
  whyItMatters: "Enterprise buyers expect to restrict expensive or non-EU models per role. The code itself flags this hole: the gateway refuses the allowlist by name because no service owns it, so operators currently cannot restrict models at all.",
  referenceBestPractice: {
    bestSource: "web-chatgpt (web-chatgpt-web-product-105)",
    others: ["claude (152, per-role effort caps, most permissive role wins)", "codexagent (045, 043, plan lists and access programs as catalogue data)", "t3code", "web-claudeai"],
    prevalence: "5 of 12 sources; table-stakes (judgement: web-only sources expose little internals)",
    mechanism: "Admins set starting model, reasoning level and speed separately per surface; new models are off by default for enterprise; roles are additive (union of allows) and the most permissive effort cap wins; a Model Test tool shows a member's effective access. [DOC for ChatGPT, SRC for Claude desktop and Codex]"
  },
  verevonToday: {
    status: "absent",
    evidence: [
      MP + "/services/model-gateway/src/grpc.rs:148-160 (verified: allowed_models is the only policy field with no owner and is refused by name)",
      MP + "/crates/mp-authctx"
    ],
    gap: "No per-org or per-role model allowlist, no role-level effort cap, no admin default per surface, no effective-access explainer. The privacy-tier floor in admission already fails closed but is not an entitlement system."
  },
  approach: "Control Plane: policy schema {subject, allow_models[], deny_models[], max_effort, default_model, default_effort} with resolution (union of role allows, intersect with plan, most permissive effort cap) and an explain API. Model Plane: extend mp-authctx claims to carry the resolved entitlement; inference-core filters ListModels and enforces in admission next to the privacy-tier floor. Frontend Plane: admin page beside the router-policy page (apps/gateway/src/domains/router_policy.rs). The entitlement can only narrow access; it must never widen an owner-plane ACL or a Space's rights.",
  effort: "M",
  dependencies: ["Control Plane policy schema and API", "mp-authctx claims extension", "R-t3-09-05 (locked-model reasons in the picker)"],
  planeOwner: "Control Plane (policy) with Model Plane (enforcement) and Frontend Plane (admin UI)",
  risks: "Resolution order must be documented and tested; entitlement must not widen Space or owner-plane ACLs; stale cached entitlements could allow a revoked model.",
  tier: "priority", significance: 4, confidence: "high",
  alreadyPlanned: { is: false, where: "The code comment acknowledges the gap (grpc.rs:148-157) but no plan was found" },
  needsVerification: false,
  coversClusters: ["C-t3-09-004"],
  extraSrc: 1
},
{
  id: "R-t3-09-03",
  title: "Prompt-cache stability tiers, lineage-scoped cache keys and a measured hit rate",
  summary: "Turn on and measure what is already built: enable the OpenAI stable-prefix prompt order in staging, record cache_read/cache_write tokens and a cache-miss reason per route, then enforce stable-to-volatile segment tiers in prompt_context (org and Space blocks in the shared prefix, per-user memory and time late), add a prompt_cache_key derived from org, compaction-lineage root and tools hash, and use rolling Anthropic cache_control breakpoints with a block limit.",
  whyItMatters: "Cache savings are the largest cost and latency lever for long agent runs and Space-heavy prompts and directly feed the Budget posture the router depends on, yet no cache hit has been measured on any route and the OpenAI-side reordering is off by default.",
  referenceBestPractice: {
    bestSource: "hermes (hermes-learning-and-memory-056, hermes-architecture-and-surfaces-112)",
    others: ["cursor (105, 076, rolling four-breakpoint rule and prefix-reusing summary calls)", "deepseek", "antigravity", "web-claudeai"],
    prevalence: "5 of 12 sources; table-stakes",
    mechanism: "A three-tier system prompt (stable, context, volatile) rebuilt only on compaction keeps the longest prefix byte-identical for a session; the provider cache key is scoped to the compression-lineage root so compaction does not reset it while branches and delegates get their own scope; Anthropic breakpoints are rolled (last message always marked, older ones dropped). [SRC]"
  },
  verevonToday: {
    status: "partial",
    evidence: [
      MP + "/services/inference-core/src/provider/anthropic.rs:173-361",
      MP + "/services/model-gateway/src/prompt_context.rs:1-23 (verified: stable-prefix order gated by MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER, off unless set)",
      MP + "/services/model-gateway/src/prompt_context.rs:243",
      FP + "/src/features/chat/components/ChatPanels.tsx:926-940"
    ],
    gap: "Anthropic cache_control is applied to tools, system and history, but the OpenAI stable-prefix order is off by default, no cache hit has been measured on any route per the cache audit (DOC), there is no lineage-scoped cache key, no cache-miss reasons, and memory or time blocks are not placed in a volatile tail by contract."
  },
  approach: "model-gateway prompt_context: assign each slot a stability class and render in tier order; turn the OpenAI order on by default once a staging A/B shows hits; add prompt_cache_key = hash(org, lineage_root, tools_hash) in the OpenAI adapter and rolling breakpoints in anthropic.rs; add cache_miss_reason (ttl_expired, prefix_changed, tools_changed) to the usage event and show it in the Work panel. Needs a compaction lineage id from Session Core. Do not reintroduce a semantic answer cache (langcache.rs is a deliberate tombstone).",
  effort: "M",
  dependencies: ["compaction lineage id from Session Core", "per-provider ZDR contract check for provider-side caching"],
  planeOwner: "Model Plane",
  risks: "ZDR turns must not use provider-side caching where that implies retention (check each provider contract); reordering blocks can change answer behaviour, so A/B on recorded journeys first.",
  tier: "priority", significance: 4, confidence: "high",
  alreadyPlanned: { is: true, where: "Built but undeployed OpenAI-route cache items per the CACHE_COMPACTION_AUDIT (v-planned-decisions baseline line 212); no measurement yet" },
  needsVerification: false,
  coversClusters: ["C-t3-09-023"],
  extraSrc: 1
},
{
  id: "R-t3-09-04",
  title: "Gemini provider family (Vertex, EU region) with tool-schema sanitising and a Gemini prompt profile",
  summary: "Add a third ModelFamily for Gemini in inference-core: a native generateContent/streamGenerateContent adapter on Vertex AI in an EU region, thinking-level mapping from the effort ladder, thought-signature replay through the replay envelope (R-t3-09-07), and a decorator that sanitises tool JSON Schemas to the Gemini subset. Add a minimal Gemini prompt/tool profile and re-enable the soft-deleted catalogue row, then expose Gemini in the picker and router table (long-context and perception roles).",
  whyItMatters: "Gemini and its long-context and video strengths are named in the product goal (Antigravity capabilities), and five of twelve references ship Gemini or a multi-vendor lineup. Without it Verevon cannot route perception or long-context roles to the best model.",
  referenceBestPractice: {
    bestSource: "web-perplexity (web-perplexity-web-product-082) for the lineup; antigravity (082) for Gemini thinking levels",
    others: ["codexagent (039, per-model prompt templates)", "hermes (098, sanitises tool schemas for Gemini and Kimi)", "cursor (075, 104, family detection and prompt variants)", "deepseek (050, argument order)", "web-chatgpt"],
    prevalence: "5 of 12 sources for the multi-vendor lineup (table-stakes); 4 of 12 for per-family prompt/tool variants (common)",
    mechanism: "One picker across vendors with a Best auto-router and a per-model Thinking toggle; native adapters per vendor registered as provider families with a full capability record and residency; schema sanitisers per strict provider. [DOC for lineup, SRC for sanitiser and prompt variants]"
  },
  verevonToday: {
    status: "absent",
    evidence: [
      MP + "/services/inference-core/src/provider/mod.rs:471 (verified: ModelFamily has only OpenAiCompatible and Anthropic)",
      MP + "/go/services/capability-core/migrations/0013_privacy_tier_columns.up.sql:23 (Gemini seed row soft-deleted because no adapter can serve it)"
    ],
    gap: "No Gemini adapter, no native DeepSeek adapter, no Gemini group in the picker. The per-family prompt and tool variants half of this item has no baseline record (status unknown): check model-gateway prompt assembly for branching by provider or model family and whether tool schemas are sanitised per provider."
  },
  approach: "inference-core: add ModelFamily::Gemini and provider/gemini.rs behind the provider trait, with the metered decorator and the privacy-tier/residency gates applied like every other provider; add a SchemaSanitiser decorator; seed capability-core rows with residency and privacy tier from the Vertex contract. model-gateway: prompt profile keyed from the model record, limited to families with measured differences. Do not route privacy-floored or ZDR turns to Gemini until Vertex terms are attested.",
  effort: "L",
  dependencies: ["R-t3-09-07 replay envelope (thought signatures)", "R-t3-09-01 effort ladder", "R-t3-09-05 model record", "ZDR and residency attestation for Vertex"],
  planeOwner: "Model Plane",
  risks: "Residency and ZDR terms for Vertex must be attested before privacy-floored turns can use it; prompt-variant explosion (limit to measured families).",
  tier: "priority", significance: 4, confidence: "high",
  alreadyPlanned: { is: false, where: "Baseline: Gemini absent, no plan found (docs/MODEL_PLANE_V2_PARITY.md lists it as missing)" },
  needsVerification: true,
  coversClusters: ["C-t3-09-041", "C-t3-09-040"],
  extraSrc: 1
},
{
  id: "R-t3-09-05",
  title: "Model record v2 served by inference-core, validated at admission and rendered by a searchable picker",
  summary: "Replace the thin ModelDescriptor with a versioned ModelRecord (context window, max output, input modalities, reasoning ladder and default, verbosity support, service tiers, lifecycle, price ref, privacy tier, residency, availability, prompt profile id) owned by inference-core and returned whole by ListModels. Use it for a PreparedCall step in admission (validate effort, modalities and model/provider pairing before any network call, with typed refusals and suggestions), and render it in a Solid picker with fuzzy search, server-persisted favourites and recents, a Legacy fold and locked-model reasons. Later, load the catalogue from a signed, versioned artifact with an on-disk last-good copy.",
  whyItMatters: "One record drives the picker, effort control, context budgeting, router decisions and entitlements; adding Gemini or a new GPT generation becomes a data change. The grouped picker without search will not scale once Gemini and BYO endpoints land.",
  referenceBestPractice: {
    bestSource: "codexagent (codexagent-029, 098) for the record; t3code (004) for the picker; claude desktop (069) for signed distribution; deepseek (036) for prepared calls",
    others: ["antigravity (081, 083)", "web-chatgpt (104)", "hermes (101, stale-while-revalidate)", "t3code (073, 089, normalising catalogue sources)"],
    prevalence: "4 of 12 for the record, 4 of 12 for distribution, 3 of 12 for prepared calls, 2 of 12 for the picker; common",
    mechanism: "An embedded, remotely refreshed models.json with about 40 fields per model projected to clients by model/list; a signed catalogue with a signature sidecar, size caps and a custom URL; a frozen prepared call bound to the adapter after validation; a picker with provider rail, tokenised fuzzy search, favourites first and a collapsible Legacy section. [SRC]"
  },
  verevonToday: {
    status: "partial",
    evidence: [
      MP + "/services/model-gateway/src/http_routes.rs:7593-7640",
      MP + "/services/inference-core/src/grpc.rs:394",
      MP + "/services/inference-core/src/provider/routing_policy.rs:127-222",
      MP + "/services/inference-core/src/config.rs:259-283",
      MP + "/services/inference-core/src/provider/overflow.rs:100",
      FP + "/src/features/dashboard/home/DashboardComposer.tsx:1008-1095",
      FP + "/src/shared/api/chat-client.ts:2665-2705"
    ],
    gap: "ListModels returns only id, provider, modality, streaming, feature families, privacy tier and residency. No context window, max output, ladder, service tiers, lifecycle, price or plan gating, so the SPA cannot gate controls per model. There is no general prepared-call validation, no versioned or signed catalogue, and the picker has no search, favourites, recents, legacy fold or locked-model explanation."
  },
  approach: "mp-contracts: extend the inference-core ListModels proto with the record fields (version it so the gateway passes it through uninterpreted); seed from the existing feature list and cost-core price ids; add a startup test that every routable model has a complete record. inference-core admission: prepare(req, catalogue) -> PreparedCall or typed Refusal, allowing an unverified-but-allowed state. Frontend Plane: Solid popover component with favourites and recents stored per user through the gateway; locked entries rendered from R-t3-09-02 reasons, availability from R-t3-09-08. Last phase: catalogue loader with signature verification, ETag cache and a compiled-in floor; RoutingPolicy remains the overlay.",
  effort: "L",
  dependencies: ["mp-contracts proto change", "cost-core price catalogue ids", "R-t3-09-02 (locked reasons)", "R-t3-09-08 (availability)", "signing key management for the signed catalogue phase"],
  planeOwner: "Model Plane (record, admission, catalogue) with Frontend Plane (picker)",
  risks: "Records drift from provider reality (mitigate with a startup probe and test); over-strict validation can block valid provider ids; a bad catalogue push could hide every model, so require schema validation, staged rollout and a compiled-in floor.",
  tier: "recommended", significance: 3, confidence: "high",
  alreadyPlanned: { is: false, where: "" },
  needsVerification: false,
  coversClusters: ["C-t3-09-001", "C-t3-09-011", "C-t3-09-002", "C-t3-09-033"],
  extraSrc: 0
}
];
