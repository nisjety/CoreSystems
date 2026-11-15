const fs = require("fs");
const path = require("path");
const T = "C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/claude/C--Users-ImaFernandesDaCosta-AppData-Roaming-Claude-scratch-workspaces-5e23d905-dd91-4518-8ff4-252efcbe0520-b5ff8b3a-437c-4499-9a7f-469de994f319-scratch-2026-10-12-69a4ee/1e4c8548-5aa8-429f-9d17-693211694c40/scratchpad/findings/t3";
const clusters = JSON.parse(fs.readFileSync(T + "/cmp/09-models-routing.clusters.json", "utf8")).clusters;
const byId = Object.fromEntries(clusters.map(c => [c.id, c]));
const live = clusters.filter(c => c.tier !== "skip" && c.tier !== "n/a").map(c => c.id);

const defs = [...require(T + "/tmp/09-build-part1.js"), ...require(T + "/tmp/09-build-part2.js")];
const items = defs.map(d => {
  const mix = { REA: 0, SRC: 0, DOC: 0, INF: 0 };
  for (const cid of d.coversClusters) {
    if (!byId[cid]) throw new Error("unknown cluster " + cid);
    for (const k of Object.keys(mix)) mix[k] += (byId[cid].provenanceMix || {})[k] || 0;
  }
  mix.SRC += d.extraSrc || 0;
  const { extraSrc, ...rest } = d;
  return { ...rest, provenanceMix: mix };
});

const dropped = [
  { cluster: "C-t3-09-015", reason: "Not a fit: Verevon is a hosted cloud web product, the self-hosted Needle attempt was cancelled after measured poor quality, and on-prem or sovereign needs are served by customer endpoints in the provider registry (R-t3-09-10)." },
  { cluster: "C-t3-09-043", reason: "Not a fit: provider-native web search inside the model call would bypass Quarry, which owns web execution and capture (Model proposes, Quarry executes); web research is covered in the research category." }
];

const strengthsToKeep = [
  "Budget/Balance/Genius intent routing combines task complexity with the org's cost-core budget posture, a tool-use floor and a fallback ladder, and is runtime-editable through RoutingPolicy with an admin page; more adaptive than the fixed presets in most references (apps/Model Plane/rust/services/inference-core/src/provider/intent.rs, routing_policy.rs:127-222, apps/Frontend Plane/verevonv3/apps/gateway/src/domains/router_policy.rs).",
  "Per-turn privacy-tier and residency floor (global, eu_resident, zdr_contractual, sovereign) is enforced before any network call and fails closed, and fallback hops respect it; no reference goes beyond warning (apps/Model Plane/rust/services/inference-core/src/config.rs:259-283, provider/fallback.rs:505-700).",
  "User-owned ChatGPT subscription route keeps the OAuth token only in Integration Core and shows Model Plane an opaque connection id, with typed refusals for usage limit, reauth, rate limit and context too long; equal to Hermes on the idea and stricter on token custody (inference-core/src/provider/codex_subscription.rs, model-gateway/src/subscription_failure.rs).",
  "Provider-neutral seam: a provider trait that declares its own ModelFamily, a metered decorator intended to cover every RPC, and a typed SSE event stream with server-stamped provenance that omits unknown fields instead of faking them (inference-core/src/provider/mod.rs:471, metered.rs:1-8, model-gateway/src/sse_events.rs:20-80). Caveat: the metering decorator is reported built but not deployed.",
  "Structured outputs work on OpenAI, Anthropic (tool or instruction) and the subscription route (inference-core/src/provider/openai.rs:459-486, anthropic.rs:726-731, codex_subscription_tools.rs:15-33); gap is only a rejection memo for json_object-only MaaS endpoints.",
  "Verbosity dial (concise, balanced, detailed) is shipped on every route (model-gateway/src/verbosity.rs, apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx:169-173).",
  "Cost is never fabricated: cost-core prices are null when unknown and the subscription route says included in the subscription; cache creation and read tokens are priced (model-gateway/src/pricing.rs:206-257).",
  "Assembled-context inspector (tokens per segment versus budget) is more detailed than any reference, and real BPE token counting plus typed context overflow exist (apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPanels.tsx:571-720, apps/Model Plane/rust/crates/mp-tokens/src/lib.rs:1-5, inference-core/src/provider/overflow.rs:100).",
  "Features no reference has in this category: fine-tuning jobs (model-gateway/src/finetune_routes.rs), logprob-based certainty (inference-core/src/provider/logprobs.rs) and TOON compact encoding (apps/Model Plane/rust/crates/mp-toon/src/lib.rs).",
  "EU-resident Azure deployments of OpenAI, Claude and MaaS models with a residency and privacy tier on every registered provider give residency coverage most references lack (inference-core/src/config.rs:286-326)."
];

const out = { category: "Models & routing", items, dropped, strengthsToKeep };

// validation
const covered = new Map();
for (const it of items) for (const c of it.coversClusters) {
  if (covered.has(c)) throw new Error("duplicate coverage " + c);
  covered.set(c, it.id);
}
const droppedIds = new Set(dropped.map(d => d.cluster));
const missing = live.filter(c => !covered.has(c) && !droppedIds.has(c));
const overlap = dropped.filter(d => covered.has(d.cluster));
if (missing.length) throw new Error("missing " + missing.join(","));
if (overlap.length) throw new Error("overlap " + JSON.stringify(overlap));
const tiers = { priority: 0, recommended: 0, "nice-to-have": 0 };
for (const it of items) {
  tiers[it.tier]++;
  for (const k of ["id","title","summary","whyItMatters","referenceBestPractice","verevonToday","approach","effort","dependencies","planeOwner","risks","tier","significance","confidence","provenanceMix","alreadyPlanned","needsVerification","coversClusters"]) {
    if (it[k] === undefined) throw new Error(it.id + " missing " + k);
  }
  for (const k of ["bestSource","others","prevalence","mechanism"]) if (!it.referenceBestPractice[k]) throw new Error(it.id + " rbp " + k);
  for (const k of ["status","evidence","gap"]) if (!it.verevonToday[k]) throw new Error(it.id + " vt " + k);
}
fs.writeFileSync(T + "/cons/09-models-routing.items.json", JSON.stringify(out, null, 2));
const nonLiveCovered = [...covered.keys()].filter(c => !live.includes(c));
console.log(JSON.stringify({ live: live.length, covered: covered.size, dropped: dropped.length, items: items.length, tiers, nonLiveCovered }));
