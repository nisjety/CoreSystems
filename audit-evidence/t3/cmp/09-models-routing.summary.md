# T3 / 09 Models & routing: cross-source comparison

The inputs were 187 reference records from 12 sources and 41 code-verified Verevon baseline records. They form **43 clusters**. 185 records are assigned to a cluster and 2 are excluded: CLI-installer trivia from T3 Code. Detail is in `09-models-routing.clusters.json`.

Statuses come from the baseline. For clusters the baseline does not cover, I ran a few read-only greps in `apps/Model Plane/rust` and marked them SRC where I used them.

Verevon status per cluster:

| Status | Clusters |
|---|---|
| shipped | 6 |
| partial | 19 |
| absent | 4 |
| unknown | 14 (each needs a code check) |

## Shared baseline (table-stakes)

These are adjusted for sources that only document their web product:

- **Provider-neutral adapter layer.** Verevon has shipped this.
- **Per-model reasoning-effort ladder.** Verevon is partial.
- **Plan, role and org model entitlements.** Absent in Verevon.
- **BYO keys and cloud accounts.** Verevon is partial.
- **Prompt-cache management.** Verevon is partial.
- **Multi-vendor lineup including Gemini.** Verevon is partial.

The following are common (4 to 6 sources): a rich per-model capability record, intent presets, role-specific model slots, a declarative provider registry, cost accounting by token category, and context-window management.

## Best of breed

| Capability | Best source (records) | Why |
|---|---|---|
| Capability record | Codex (codexagent-029, 098) | About 40 fields per model: reasoning ladder, tiers, plans and prompt templates, projected by model/list. |
| Catalogue distribution | Claude desktop (claude-runtime-069) | Signed catalogue with a signature sidecar, size caps and a custom URL. |
| Effort ladder | Hermes (hermes-arch-096) | One canonical scale. An unsupported level is clamped to the nearest weaker one the provider accepts. Capability probes are cached. |
| Role and task slots | Hermes (hermes-arch-099) | About 20 auxiliary tasks, each with its own provider, model and effort, through one resolver with hooks. |
| Failover | Hermes (hermes-arch-095, 094) | Benches a failed primary until its reset time and rotates credential pools before switching provider. |
| Usage-limit fallback | Codex (codexagent-036) | Declared fallback models, then an automatic switch back when the limit resets. |
| Reasoning replay | Hermes (hermes-arch-097) | Replays reasoning state only to the matching route and recovers from rejected signatures. |
| Prompt cache | Hermes (hermes-learning-056, hermes-arch-112) | Three-tier system prompt, plus a cache key scoped to the compaction lineage. |
| In-band changes that keep the cache | DeepSeek (deepseek-harness-037, 038) | Prompt updates are appended to history and tool changes are announced, not rewritten. |
| Adapter layer and event stream | T3 Code (t3code-ui-056, 055) | 48-type canonical runtime event taxonomy behind a driver SPI. |
| Provider registry | Hermes (hermes-arch-091) | 41 declarative provider profiles that a user can override. |
| BYOK and enterprise gateway | Claude desktop (claude-product-107, claude-runtime-072) | Bedrock, Vertex, Foundry and gateway setup, with health probes and credential helpers that have a TTL. |
| Entitlements | ChatGPT (web-chatgpt-105) | Per-surface admin defaults, additive roles, and an explainer for a member's effective access. |
| Cost breakdown | Antigravity (antigravity-ux-088) | Cost per turn, per subagent and cumulative, with a breakdown per tool. |
| Picker UX | T3 Code (t3code-ui-004) | Provider rail, fuzzy search, favourites and a Legacy fold. |

## Differentiators worth noting

- **Antigravity:** best-of-N model battles.
- **Cursor:** visible routed model, routing feedback and progressive context hydration.
- **Codex:** a WebSocket Responses API with incremental continuation, and a model-migration validation playbook.
- **DeepSeek:** prepared calls validated before dispatch, and in-history system-prompt updates.
- **Claude:** reversible refusal-fallback model swaps.

## Verevon's biggest gaps

The priority gaps:

1. **No reasoning effort on the OpenAI/Azure route.** That route carries most intent defaults. Fix with a canonical ladder mapped to each provider's wire format (C-t3-09-005; S).
2. **No Gemini provider.** The seed row was soft-deleted because no adapter can serve it. Add a Gemini family through Vertex in an EU region, with thought-signature replay and schema sanitising (C-t3-09-041; L).
3. **The org/role model allowlist has no owner.** model-gateway refuses `allowed_models` (grpc.rs:151-157). Give it a home in Control Plane: Control decides and Model enforces (C-t3-09-004; M).
4. **Prompt caching is built but no hit has been measured.** The OpenAI stable prefix is off by default. Add stability tiers and lineage-scoped keys (C-t3-09-023; M).

Recommended:

- **Hard-coded side-task models.** `TITLE_MODEL`, `FOLLOW_UPS_MODEL` and `VERIFICATION_JUDGE_MODEL` are all `gpt-4o-mini` (sse.rs:7839-7894). Replace them with a role table (C-t3-09-008).
- **Routed model and reason missing from the served-by line** (C-t3-09-007).
- **No reasoning-signature replay.** `signature_delta` is dropped (anthropic.rs:383-394) (C-t3-09-021).
- **Usage-limit fallback and switch-back** (C-t3-09-018).
- **Failover** with credential pools and benching (C-t3-09-017).
- **Declarative provider registry, then BYOK** (C-t3-09-012, 013).
- **Rest of the recommended set:** cost per token category, the live meter, purpose tags, per-family prompt variants, router evals and picker search.

Skip:

- **Local model runtimes.** Verevon is a cloud web app, and the self-hosted Needle attempt was cancelled.
- **Provider-native web search.** It would bypass Quarry.

## Verevon strengths (keep)

- **Budget/Balance/Genius intent routing.** Combines task complexity with cost-core budget posture and can be edited at runtime.
- **Privacy floor.** A per-turn privacy-tier and residency floor is enforced before the network and fails closed. No reference does more than warn.
- **Subscription route.** The user-owned ChatGPT subscription route keeps its tokens only in Integration Core.
- **Typed SSE events.** Provenance is stamped server-side and is never fabricated.
- **Structured outputs on every route.**
- **Verbosity dial.**
- **Assembled-context inspector.**
- **Features no reference has in this category:** fine-tuning jobs, logprob-based certainty, and TOON compact encoding.

## Caveats

- **Table-stakes overrides.** No cluster reached 7 of 12 sources. Six clusters were labelled table-stakes by judgement, because web-only sources expose little internal routing.
- **Unknown clusters.** 14 clusters are `unknown` because no baseline record covers them: lifecycle, multimodal routing, refusal fallback, mid-thread switching and others. Each one names what to check in code.
- **Unverified strength.** In cluster 029 (privacy and cost guards) Verevon is rated ahead only by comparing with Hermes; the claim has not been tested.
- **DOC-heavy clusters.** The web benchmarks are DOC only. The lineup and presets clusters lean on DOC records (DOC 5/6 and 4/6), so their best-in-class claims rest on vendor documentation.
- **Doc-reported baseline facts.** Several baseline facts are reported by docs, not code: no cache hit measured, no live meter, and failover UX.
- **Accidental write outside the scratchpad.** During setup an empty file `/tmp/x.js` was created by mistake outside the scratchpad, then removed straight away.
