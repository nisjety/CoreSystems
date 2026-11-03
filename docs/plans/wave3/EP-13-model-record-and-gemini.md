# EP-13 (Wave 3 part) - Model record v2, role table, and the Gemini provider family

Abbreviations: `MP` = `apps/Model Plane`; `MPR` = `MP/rust`; `IC` = `MPR/services/inference-core/src`; `MG` = `MPR/services/model-gateway/src`; `EC` = `MPR/services/execution-core/src`; `SC` = `MPR/services/session-core`; `CC` = `MP/go/services/cost-core`; `CAP` = `MP/go/services/capability-core`; `PROTO` = `MP/proto/model_plane/v1`; `FE` = `apps/Frontend Plane/verevonv3`; `FES` = `FE/src`; `BFF` = `FE/apps/gateway/src`. Every `path:line` below was opened in worktree `epic-shaw-4d613b` at HEAD `c18196af2` (branch `model-plane`, Waves 1 and 2 built). `[INF]` = inference, not read in code or docs. `[verify live]` = only a running stack can prove it. Audit = `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`; where it disagrees with the tree, the tree wins and the claim is marked STALE.

Design only. No code, migration or proto was changed to write this.

## 1. Scope, items resolved, owner

**Items (W3 only):**
- F-t3-042 Model record v2 and role table (audit: Recommended, Partial, L, W3). Audit dependency F-t3-023 (WS10).
- F-t3-041 Gemini provider family on Vertex AI in an EU region (audit: Recommended, Absent, L, W3). Audit dependencies F-t3-029 (WS11), F-t3-042, and the Vertex residency and ZDR attestation question (audit 10.2 Q13, plan DEC-40).

**Owners (CLAUDE.md plane rules):**
- Model Plane, inference-core: the model record (limits, lifecycle, effort ladder, privacy, residency), the role resolver, the Gemini adapter, the provider chain gates. It never stores price.
- Model Plane, cost-core (`CC`, it lives under `apps/Model Plane`): the price catalogue (`model_pricing`). The price is joined by model id at read time in model-gateway; it is never copied into the inference-core record.
- Model Plane, session-core: stores the routing policy as an opaque JSON singleton (`IC/provider/routing_policy.rs:21-22`; `IC/provider/policy_client.rs:6`). The role table rides inside that JSON, so session-core needs no migration.
- Model Plane, model-gateway: the `/v1/models` read model (price join), the purpose tag on its own side-task calls, the role-routing switch.
- Control Plane: model entitlement only (WS10, org-core decides, inference-core enforces). No Control change in this epic.
- Frontend Plane: picker search and record display, the role editor on the router-policy page, BFF pass-through (already verbatim, `BFF/domains/chat/json_handlers.rs:612-637`).

| Slice | Tag | Item | In |
|---|---|---|---|
| S1 | BUILD | 042 | Picker search in both pickers, list price joined from cost-core at `/v1/models` (switch off) |
| S2 | BUILD | 042 | Record v2 fields: per-model context window, max output, lifecycle from one inference-core table; `ModelInfo` 9-11 |
| S3 | BUILD | 042 | Purpose tag on every inference call (`InferRequest.purpose`), `purpose` metrics label; side-task calls inherit the turn's privacy floors |
| S4 | BUILD | 042 | Role table in `RoutingPolicy` and the `verevon-role` resolver in inference-core; gateway side tasks use it behind a switch |
| S5 | BUILD | 042 | Role editor on the router-policy page; the action contract round-trips the whole policy |
| S6 | BUILD | 041 | Gemini adapter core on Vertex (token provider, text infer and stream, EU residency from location, never ZDR), registered dark |
| S7 | BUILD | 041 | Gemini tools and effort: tool-schema sanitiser, function calls, thinking-level wire classes |
| S8 | FINISH | 041 | Gemini price rows, release-gate route check, compose wiring off, ledger entry, owner-funded `[verify live]` gate |

**Explicitly OUT:**
- F-t3-065 declarative provider registry and bring-your-own (BYO) connections: W4 member of EP-13. No admin overlay, no customer gateway, no BYO key, no Integration Core custody work here. The role table stays platform-scoped so it does not pre-empt the W4 overlay.
- A Gemini ZDR attestation (`VERTEX_GEMINI_ZDR_*`). Commercial and evidence-bound like Z-40 (WS01-S13, blocked-external). S6 does not read any attestation for Gemini, so it cannot claim ZDR.
- A Gemini prompt profile (per-family prompt variants). There is no per-family prompt branching at HEAD to build on (see section 2), so this is a measured item that belongs behind the eval gate of EP-14 (F-t3-040). Move to W5 or EP-14.
- Gemini embeddings, image, video, speech, media parts (F-t3-090, F-t3-051 in EP-5). CLAUDE.md: no embeddings outside Data Plane.
- Purpose as a column of the cost ledger (billing attribution). DEC-18 says Control confirms attribution first. W3 ships the metric label only.
- W5 items that depend on 042: F-t3-072 resilient routing, F-t3-073 per-step routing decision, F-t3-095 cache levers.
- Re-enabling the soft-deleted capability-core Gemini row. It is decorative; the picker reads `ListModels` (audit agrees).
- Per-org role tables and per-org provider overrides (W4, with F-t3-065).

## 2. Verified current state

**The model record today.**
- `ModelInfo` (`PROTO/inference.proto:620-654`) has `id 1, provider 2, modality 3, streaming 4, features 5, privacy_tier 6, residency 7, effort_levels 8`. Next free field: **9**. `ListModelsRequest` (`:611-618`) has `modality 1, provider 2`; next free 3. `ListModelsResponse.access = 2` (`:656-667`, WS10). The audit cites `:481-525` and lists no effort ladder: STALE (line drift, and WS11-S5 already added `effort_levels`).
- inference-core maps its internal record to the wire at `IC/grpc.rs:65-89`; `cheap` travels as a `"cheap"` entry in `features`. The internal `ModelInfo` is `IC/provider/mod.rs:489-510`. `list_models` (`IC/grpc.rs:442-...`) merges the chat chain with speech, translation, vision, doc-intel, language, realtime and video providers, dedups by id, then applies the org policy to chat models only.
- Limits live on `ProviderCapabilities` (`IC/provider/mod.rs:527-576`: `max_context_tokens`, `max_output_tokens`), which is **per provider, not per model**. Anthropic sets 1,000,000 context and 128,000 output for every direct Claude model and 200,000 / 8,192 for every Foundry deployment (`IC/provider/anthropic.rs:1462-1472`). OpenAI and Azure OpenAI declare a "conservative" 128,000 / 16,384 for every model (`IC/provider/openai.rs:845-870`). The default is 8,192 / 4,096 (`IC/provider/mod.rs:598-599`). New finding: copying these numbers into `ModelInfo` would publish wrong per-model limits (Haiku at 1M context). S2 adds a per-model table and leaves unknown limits at 0.
- No lifecycle field exists anywhere in the record.
- Price lives in cost-core: table `model_pricing(model, input_per_million, output_per_million, currency)` seeded and upserted on every boot (`CC/migrations/0002_model_pricing.up.sql:18-55`), served at `GET /api/v1/pricing` (`CC/internal/server/server.go:151`). Unknown models resolve to the mandatory `default` row (3.00 / 15.00). model-gateway already caches this catalogue (`MG/pricing.rs:1-20`), resolves Exact, Prefix, Default, Subscription, Unavailable (`MG/pricing.rs:117-128`), and reports a Default-only match as no price (ledger 2026-10-23 WS27-S1, `MP/docs/decisions/ledger.md:209-213`). `PricingCache::snapshot()` (`MG/pricing.rs:309-318`) gives one refresh then synchronous pricing.
- Gateway `/v1/models` (`MG/http_routes.rs:335`, handler `:10141-10199`) maps to `ModelDescriptor` (`:10070-10087`: id, provider, modality, streaming, features, privacy_tier label, residency, effort_levels) plus `entitlement` and `attachment_policy` siblings. No price. The BFF passes it through (`BFF/domains/chat/json_handlers.rs:612-637`).
- FE: `ModelInfo` type `FES/shared/api/chat-client.ts:1223-1249`; parser `listModelCatalogue` `:3777-3825` (already reads a `cost_tier` if one ever arrives, never sent today); grouping `groupChatModels` `:4025-4048` (provider buckets, unknown providers fall into "Other models"). Two pickers render it: `FES/shared/components/AiModelPicker.tsx` (238 lines, no search; grouping at `:75-78`) and the main composer menu `FES/features/dashboard/home/DashboardComposer.tsx:3014-3040` (groups at `:1219-1224`). Neither has search.

**Side-task models today.**
- Title, follow-ups and the verification judge share one model: `aux_model()` reads `MODEL_GATEWAY_AUX_MODEL` once, default `gpt-4o-mini` (`MG/sse.rs:9490-9575`); compose passes it through empty by default (`MP/deploy/docker-compose.yml:224-226`). This is the D1-807 fix (register row resolved 2026-10-28). The audit's "three gpt-4o-mini constants at sse.rs:7835-7900": STALE (one operator variable now), but there is still no role dimension and no purpose tag.
- Deep research planning pins the tier `verevon-budget` (`MG/deep_research.rs:255-263`). Session-core dreaming and memory grounding default to `claude-haiku-4-5` with their own variables `DREAMING_LLM_MODEL`, `MEMORY_GROUNDING_MODEL` (`SC/src/dream_extractor.rs:81,417`; `SC/src/memory_grounding.rs:127,367`). Injection screening has `CONTENT_SAFETY_INJECTION_MODEL` (`MG/semantic_screening.rs:139`). The browser planner and scheduled runs resolve through `verevon-balance` (`EC/llm_planner.rs:114`; `EC/scheduled_agent.rs:21`); the audit already corrected that premise.
- `InferRequest` is built in 14 source files (gateway `deep_research.rs`, `grpc.rs`, `http_routes.rs`, `result_validation.rs`, `review_proposal_rule.rs`, `source_validation.rs`, `sse.rs`, `tool_loop.rs`, `vision_parts.rs`; execution-core `grpc.rs`, `llm_planner.rs`, `runtime_loop/agent.rs`; session-core `dream_extractor.rs`, `memory_grounding.rs`), 98 literal constructions including tests.
- New finding (privacy): the side-task request builders copy the turn's `zdr`, `min_privacy_tier` and `min_residency` only when the turn rides a subscription (`subscription_scoped_request`, `MG/sse.rs:802-812`). On every other route the title (`MG/sse.rs:9746-9775`, `zdr: false`, skipped for ZDR turns at the call site) and the judge (`:9578-9612`, `zdr` copied) drop the turn's privacy-tier and residency floors. A turn the user floored to EU-resident can therefore have its question and answer summarised by a provider below that floor. `[verify live]` on a stack with a Global provider; on the current all-EU dev chain it has no visible effect. S3 fixes it.
- `CallPhase {Answer, ToolRound, AgentRound, Overhead}` is the gateway-local meter vocabulary (`MG/usage_meter.rs:13-36`). inference-core metrics are `inference_requests_total{rpc,service,model,provider,outcome}` and `inference_tokens_total{service,model,provider,direction}` with bounded labels (`IC/metrics.rs:1-31`). No purpose dimension anywhere.

**Routing policy.**
- `RoutingPolicy` (`IC/provider/routing_policy.rs:25-55`): `enabled, budget_cap_usd, constrained_fraction, cheap_fallback, tool_fallback_ladder (serde default), complexity, table`. No role field. Default table `:125-160`. session-core only checks the string is JSON (`IC/provider/policy_client.rs:12`). The singleton is platform-scoped: inference-core reads it under a sentinel tenant (`IC/provider/policy_client.rs:28`); the table is created by `SC/migrations/0008_routing_policy.sql` (file not re-opened).
- Writes: inference-core `PUT /internal/v1/router-policy` (`IC/http_health.rs:137-170`) requires scope `inference:policy:admin` (`IC/auth.rs:266-278`), persists to session-core and hot-swaps. BFF `/api/v1/router-policy` proxies it (`BFF/domains/router_policy.rs:1-31`); SPA page `FES/features/router-policy/components/RouterPolicyPage.tsx`.
- New finding: the SPA action contract `routerPolicyUpdateInput` (`FES/shared/actions/action-registry.ts:1598-1623`, action `router_policy.update` at `:3906`) has no `tool_fallback_ladder`. Because the PUT is a full document and the field is `#[serde(default = ...)]`, any save from the page resets a customised ladder to the default. A `roles` field would be wiped the same way. S5 fixes the round-trip before any role editor exists.

**Providers and the chain.**
- `ModelFamily` has two variants, `OpenAiCompatible` and `Anthropic` (`IC/provider/mod.rs:612-620`; audit cites `:462-478`, line drift only).
- Family routing is two-way by prefix: `provider_serves_model` (`IC/provider/fallback.rs:551-570`) gives Anthropic-family providers `claude*` ids and gives OpenAI-family providers **every other id**, unless the provider sets `exclusive_catalog`. `is_anthropic_model` is a `claude` prefix check (`:326-328`). New finding: an id like `gemini-2.5-pro` would today be offered to Azure OpenAI and 404 there; S6 must exclude Gemini ids from the OpenAI family, not only add a Gemini family.
- Registration is a `match` over `INFERENCE_PROVIDER_ORDER` (default `anthropic,openai`, `IC/config.rs:212-218`) at `IC/provider/fallback.rs:687-800`. Residency is classified per resource from region signals (`:644-676`); a provider that declares no residency only logs a warning unless `MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY` (`:1005-1025`; `IC/config.rs:289-290`).
- ZDR is evidence-bound: `resolve_zdr(...)` per surface at boot (`IC/config.rs:264-283`), states in `IC/provider/zdr.rs:83-160` (`Incomplete`, `DigestMismatch` fatal; `Expired`, `NotYetEffective` non-fatal per DEC-33). `supports_zdr` is false unless an attestation is in force (`IC/provider/anthropic.rs:1460`). Privacy tiers: `GLOBAL 1, EU_RESIDENT 2, ZDR_CONTRACTUAL 3, SOVEREIGN 4` (`PROTO/inference.proto:93-104`).
- Effort wire classes: `Wire {None, AnthropicBudget, AnthropicAdaptive, OpenAiReasoningEffort, CodexEffort}` and `Family {OpenAi, Anthropic, Codex}` (`IC/provider/effort.rs:88-110`). The plan's "fourth Wire variant" is off by one: there are already four non-None classes; Gemini adds one or two.
- The seeded `google/gemini-1.5-pro` capability row is soft-deleted (`CAP/migrations/0013_privacy_tier_columns.up.sql:23-34`; test `CAP/internal/registry/models_integration_test.go:285-300`). Confirmed.
- Nothing serves Gemini: no `gemini`/`vertex` string in any Rust, Go, SQL or proto file under `MP` except that test, a compaction test fixture and a roadmap note.
- Prompt branching: the audit says "prompt branching is a claude-prefix check (`compaction.rs:136-150`)". STALE: `is_anthropic_family_model` is now only a pin hint and `should_run_local_compaction` returns true for every model (`MG/compaction.rs:141-153`). There is no per-family prompt profile at HEAD at all, which is why the profile is OUT.
- Thought signatures: the audit says the native adapter is needed for thought-signature replay. STALE for this tree: neither loop replays native function-call parts. The chat tool loop and the Do loop append the model's text as an `assistant` message and the tool outcomes as a framed `user` message (`MG/tool_loop.rs:8928-8945`; `EC/runtime_loop/agent.rs:4724-4737`), and `ChatMessage` has only `role, content, name, compaction_summary` (`PROTO/inference.proto:295-306`). Each round is a fresh request with text history, so there is no prior signed function-call turn to return. `[INF]` that Gemini then needs no signature replay; `[verify live]` in S7 with a two-round tool turn.
- Metering: the audit says Gemini turns "would be unmetered". Partly STALE: cost-core prices an unknown model at the `default` row (3.00/15.00 per million) in the durable ledger (`CC/migrations/0002_model_pricing.up.sql:30-33`), while the stream states no cost (`MG/pricing.rs`, WS27-S1). Gemini would be mis-metered in the ledger and unpriced in the meter. Sourced rows are still a precondition (S8), and the ledger already refuses to invent prices (`OD-PRICES`, `MP/docs/decisions/ledger.md:1458-1463`).
- Release gate: `release-artifact.sh` validates provider routes per surface (Azure OpenAI, Azure Anthropic at `MP/scripts/release-artifact.sh:1001-1022`). A new provider needs its own route check and env partition.

**ZDR contracts.** `MP/api/contracts/zdr-posture-matrix.json` has 44 `decision_rows`, all about org posture, connections, acknowledgements and outages; none models platform provider routing. `zdr-refusals.json` already has `zdr_unavailable` ("no model deployment is attested"). `connection-zdr-capabilities.json` covers user-owned connections only (BYO side, W4).

**Prerequisite status (queue `.plan-work/execution/queue.json`, register `docs/plans/model-plane-depth-register.md`):**

| Prereq | Queue | Open rows that matter | Effect here |
|---|---|---|---|
| WS10-S1, S3, S5 | done | none | `ListModels` already filters by org policy; reuse |
| WS10-S2 | partial | D1-125/126 (store never met real Postgres), D1-751 (history not append-only), D1-121 (non-member rule) | none of our slices write policy; S4 reads entitlement through inference-core only. Start anyway |
| WS10-S4 | partial | D1-498 (gateway admission fails open), D1-503 (live proof), D1-504 (inventory failure) | S4's resolver must call inference-core's own `entitlement().resolve` and skip non-entitled candidates; it does not depend on gateway admission. Start anyway |
| WS10-S6 | partial | D1-516/521 (e2e unrun), D1-522 (pre-existing typecheck), D1-523 | S1 edits the same two picker files and `chat-client.ts`; rebase on WS10-S6, keep its tests green. Start anyway |
| WS10-S7 | code done, live blocked-external | D1-590 | none |
| WS11-S1..S5 | code done, live blocked-external | D1-024 resolved (live on claude-sonnet-5-5) | S2 reads `effort_levels`; S7 adds Gemini wire classes on this table. Start anyway |
| WS11-S6 | done | D2-008 (live) | picker already renders levels |
| WS27-S1 | done | D2-009 (P3) | S1 uses `PriceSource`; Default-only stays "no price" |
| WS27-S2 | partial | D2-565..569, D2-601 (overhead money gaps, D3/D4 acceptance) | S3's purpose tag gives these calls a name; it does not wait for S2's flip |
| WS27-S3 | partial | D2-659 (per-call `ensure_fresh`) | S1 must price the catalogue from one `snapshot()` per request, not per model |
| WS01-S5 | partial | D1-138/141 (release tests not run on Linux), D1-140 (lapse boot proof), D1-145 | S8's release-gate case rides the same untested scripts: run them in CI. Gemini reads no attestation, so S6/S7 start anyway |
| WS01-S13 | blocked-external | D1-327 (no real Z-40 contract) | Gemini ZDR is the same kind of commercial gate: OUT |

Ledger rulings that apply: WS27-S1 "an unlisted model's cost is unknown" (`ledger.md:209-213`); `OD-PRICES` no invented prices (`:1458-1463`); WS11-S2/S3 effort wire behind switches (`:81-88`); WS11-S6 composer levels from the catalogue (`:205-207`). No ledger entry mentions Gemini, Vertex, a role table or a purpose tag.

Cross-session overlaps: `overlap-space.md` row 3 says Control owns `allowed_models` and F-t3-023 lands before any Space model field; nothing in Space or Quarry claims the model record, the role table or Gemini. `overlap-quarry.md`: Quarry has its own `QUARRY_BROWSER_AGENT_MODEL`; the role table does not reach into Quarry (Model proposes, Quarry executes). `migrations-protos.md` and `flags-and-config.md`: used for numbering and switch vocabulary below.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| D1 | Gemini posture bar (DEC-40, audit 10.2 Q13) | (a) no Gemini traffic until a Vertex ZDR attestation exists (DEC-40 literally); (b) register Gemini as an EU-resident, non-ZDR provider: it serves only non-ZDR turns whose floor is at most `EU_RESIDENT`; ZDR attestation is a later, separate gate; (c) also allow the Vertex global endpoint | **(b).** It is exactly today's Foundry Claude posture ("Eu without ZDR", `IC/provider/mod.rs:620-630` doc). The chain already skips non-ZDR providers for ZDR turns and floors, so (b) cannot leak a ZDR turn. (a) blocks a product goal on a commercial contract that has no date. (c) breaks the residency promise. Needs an owner ledger entry narrowing DEC-40 | S8 enablement only | Yes: S6, S7 build dark |
| D2 | Gemini prices | invent from the vendor page / owner or operator supplies sourced rows | **Sourced rows only** (ledger `OD-PRICES`). Until then cost-core charges the `default` row and the meter shows no cost, so enablement waits | S8 | Yes (S1-S7) |
| D3 | Live Gemini proof | run on the owner's stack / skip | The owner's live-test rule allows only the Anthropic key and `claude-sonnet-5-5`. Gemini needs a Vertex project, an EU location and a service account the owner funds. **No Gemini switch turns on until the owner funds and runs the S8 gate** | S8 DoD | Yes (build and unit tests) |
| D4 | Adapter path | (a) native Vertex `generateContent` / `streamGenerateContent`; (b) Vertex's OpenAI-compatible endpoint through `OpenAiProvider` | **(a).** It gives explicit control of location (residency), thinking config and safety settings; (b) is unverified against Vertex [INF], would still need a token provider, and would put Gemini ids in the OpenAI family that S6 must exclude | S6, S7 | n/a |
| D5 | Thought signatures | proto carrier (`ChatMessage` field 5) / none | **None in W3.** No loop replays native function-call parts (section 2). The adapter ignores returned signatures. Re-open only if a loop moves to native tool-call replay (then `ChatMessage` field 5, opaque, never logged) | S7 | Yes |
| D6 | Role table home and authority | (a) inside the platform `RoutingPolicy` singleton, scope `inference:policy:admin`; (b) per org in org-core | **(a)** for W3. It reuses the existing store, hot-swap and admin path, needs no migration, and leaves per-org overrides to the W4 registry overlay (F-t3-065). An org still narrows through WS10 entitlement | S4, S5 | Yes |
| D7 | What a role resolves to, and what happens when nothing fits | fail the side task / fall back | **Ordered candidates, first that passes all filters; else the turn's own resolved model; else today's `aux_model()`.** Filters: listed in the chain's catalogue; entitled for the caller (WS10); meets the turn's ZDR, privacy-tier and residency floors; for small-budget purposes (title, follow-ups, judge) the model must accept a no-reasoning call (effort `Wire::None` or a `minimal` level) because 24 and 8 token budgets are spent by hidden reasoning (`MG/sse.rs:9480-9510` history). A side task never fails a turn | S4 | Yes |
| D8 | Side tasks inherit the turn's floors on every route | keep subscription-only / always | **Always** copy `zdr` (where the call carries turn content), `min_privacy_tier`, `min_residency`. It is a tightening, so no switch; a title that no longer has an eligible model is skipped, which is today's failure mode anyway. Owner may veto in review | S3 | Yes (default tighten) |
| D9 | Purpose in the cost ledger | metric label only / ledger column now | **Metric label only in W3.** A `cost_entries.purpose` column changes billing attribution (DEC-18: Control confirms first) | none in W3 | Yes |
| D10 | Who sees list prices in the picker | everyone / admins only / nobody | **Everyone, as list price per million tokens, behind `MODEL_GATEWAY_CATALOGUE_PRICES` (off).** The run-cost meter (WS27) already shows spend to members. Product owner flips | S1 flip | Yes (build) |
| D11 | [RECONCILE] Plan EP-13 lists "Control Plane (pricing join via cost-core)" | Control service / Model Plane cost-core | **cost-core is a Model Plane service** (`apps/Model Plane/go/services/cost-core`); the join is model-gateway reading cost-core. Control is involved only for billing attribution (D9, OUT) | S1 | Yes |
| D12 | Vertex residency classification | trust an env label / derive from location | **Derive from `VERTEX_GEMINI_LOCATION` against an allowlist of EU locations** (EU multi-region and `europe-*` regions [INF], confirmed against Google's current list at build). Anything else is `Global`, so the existing floors skip it. Same "Eu requires positive evidence" rule as `Residency::classify` | S6 | Yes |
| D13 | Gemini credential custody | env file mounted as a secret in inference-core / Integration Core | **inference-core secret, like the Azure keys.** It is a platform provider credential, not a user's; DEC-21's Integration Core custody covers BYO and connector tokens (W4). The service-account file path is a secret-partition key in the release gate; the token is minted per hour and never logged | S6, S8 | Yes |

Ledger entries this design adds (newest first, `MP/docs/decisions/ledger.md`): "The model record joins price by id at read time; limits come from a per-model table, unknown is 0" (S1/S2); "Every inference call names its purpose; side tasks inherit the turn's floors" (S3, D8); "Side-task models come from a platform role table; a role never fails a turn" (S4, D6, D7); "Gemini on Vertex is an EU-resident, non-ZDR provider until attested" (S8, D1, needs the owner). DEC-40 is narrowed (Gemini half) by the owner's D1 ruling; nothing else is reversed.

## 4. Slices

Commands used below. Rust (from `MP/rust`): `cargo test -q -p inference-core --lib`, `cargo test -q -p model-gateway --lib`, `-p execution-core --lib`, `-p session-core --lib`, `-p mp-contracts`; integration targets by `--test <name>`; `cargo fmt --all -- --check`; `cargo check --workspace --all-targets`. Two cargo targets fail pre-existing on Windows, so use `--lib` plus the named `--test` targets (memory note, not re-verified). Use a fresh `CARGO_TARGET_DIR` or `cargo clean -p` for touched crates: register notes stale warm targets (memory "Stale mp-target-w2 builds"). Proto (from `MP/proto`): `buf lint` against `buf-lint-baseline.json`, `buf generate`, `buf breaking --against baseline.binpb`; Go bindings are checked in under `MP/go/gen/model_plane/v1/`. Go (from `CC`): `go test ./...` with `GOWORK=off`. FE (from `FE`): `pnpm exec vitest run <files>`, `pnpm lint`, `tsc6 -p tsconfig.app.json` (memory: `tsc -p tsconfig.json` checks nothing). BFF: `cargo test --manifest-path "FE/apps/gateway/Cargo.toml" --all-targets`. Release gate: `bash MP/scripts/tests/release-runtime-partition-test.sh`, `release-contract-test.sh`, `release-input-security-test.sh` (Linux or CI only, D1-138). ZDR: `bash scripts/tests/zdr-contract-sync.test.sh`. Live proofs follow the owner's rule: Anthropic key and `claude-sonnet-5-5` only, rebuilt images, second Vite dev server (`:5173` is the main checkout's container).

### S1 - Picker search and list price at /v1/models [BUILD] (042)

**Goal.** The cheapest user-visible value: a person can type to find a model in either picker, and (switch on) sees its list price. No proto change.

**Files.**
- `MG/http_routes.rs` - `ModelDescriptor` (`:10070-10087`) gains `price: Option<ModelPrice>` with `#[serde(skip_serializing_if = "Option::is_none")]`; `ModelPrice { input_per_million: f64, output_per_million: f64, currency: String, source: "exact"|"prefix" }`. In `list_models` (`:10141`), when the switch is on, take **one** `state.pricing.snapshot()` (D2-659 lesson) and price each chat model by id. `Default`, `Unavailable` and `Subscription` give no `price` (a subscription model carries `features` already).
- `MG/pricing.rs` - add `RateSnapshot::list_price(model) -> Option<(Rate, PriceSource)>` reusing the existing resolver; no new matching rules.
- `MG/catalogue_prices.rs` - NEW, switch constant `pub const CATALOGUE_PRICES_ENV: &str = "MODEL_GATEWAY_CATALOGUE_PRICES";` and a pure parser (vocabulary C, `flags-and-config.md` 8.1).
- `MP/deploy/docker-compose.yml` - `MODEL_GATEWAY_CATALOGUE_PRICES: ${MODEL_GATEWAY_CATALOGUE_PRICES:-false}` on model-gateway.
- `FES/shared/api/chat-client.ts` - `ModelInfo.price?: { inputPerMillion, outputPerMillion, currency }` parsed in `listModelCatalogue` (`:3777-3825`); unknown or partial shapes are dropped, never guessed. Export a pure `filterModels(models, query)` (case- and accent-insensitive match on name, id, provider label; Norwegian letters preserved).
- `FES/shared/components/ModelSearchField.tsx` - NEW small search input (Norwegian and English strings through `useI18n`, `aria-controls` the list, Escape clears, focus stays in the menu).
- `FES/shared/components/AiModelPicker.tsx` and `FES/features/dashboard/home/DashboardComposer.tsx` (`:3014-3040`) - mount the field above the groups only when the menu lists more than 6 chat models; groups filter through `filterModels`; empty result shows "Ingen modeller passer". Show price as a muted second line when present.
- Action contract: none (search is a view filter; selecting a model is the existing selection, not a new operation).

**Contract changes.** `/v1/models` JSON gains optional `price` per model. No proto, no migration.
**Flag.** `MODEL_GATEWAY_CATALOGUE_PRICES`, default off. Search has no flag (pure FE view, rollback by revert). Turn-on: the S1 live proof plus the D10 product ruling.
**ZDR.** Reads a price catalogue and the model list; no content, no new store, no egress. Temporary-chat and ZDR orgs see the same list.
**Tests.**
- `MG/http_routes.rs` unit: `list_models_omits_price_when_switch_off`, `list_models_prices_exact_and_prefix_matches`, `list_models_never_prices_from_default_row`, `list_models_takes_one_snapshot_per_request` (counts fetches against a stub cost-core).
- `MPR/services/model-gateway/tests/attachment_policy_contract.rs` sibling: NEW `MPR/services/model-gateway/tests/catalogue_price_contract.rs` (`price_is_a_sibling_field_or_absent`).
- FE: `FES/shared/api/chat-client.model-price.test.ts` (parse, drop malformed), `FES/shared/components/ModelSearchField.test.tsx`, update `AiModelPicker.model-entitlement.test.tsx` and `DashboardComposer.model-entitlement.test.tsx` (search keeps the stale-model notice working).
- Commands: `cargo test -q -p model-gateway --lib list_models`, `cargo test -q -p model-gateway --test catalogue_price_contract`, `pnpm exec vitest run src/shared/api src/shared/components src/features/dashboard/home`, `pnpm lint`, `tsc6 -p tsconfig.app.json`.
**Live proof and evidence.** Rebuild model-gateway and the frontend on the Anthropic-only stack. `curl` `/api/v1/models` with the switch off (no `price` key, byte-compare with the previous build) and on (`claude-sonnet-5-5` priced by the `claude-sonnet` prefix row, 3.00/15.00). Screenshots of both pickers at 1280 and 375 px with a search typed. Evidence file `docs/compliance/evidence/ep13-s1-catalogue-<date>.txt`.
**Effort.** M, 3-4 d. **Deps.** WS27-S1 (done); rebase on WS10-S6 picker edits. **Risks.** Two pickers drift: put the filter and the field in shared code; `DashboardComposer.tsx` is 6,382 lines and a hot file, so keep the edit to the menu block.

### S2 - Record v2: per-model limits and lifecycle [BUILD] (042)

**Goal.** The record states each model's context window, maximum output and lifecycle so the SPA can show and later gate controls per model. One table in inference-core; unknown stays unknown.

**Files.**
- `IC/provider/model_record.rs` - NEW. A static table `fn declared_record(family: effort::Family, model: &str) -> Option<RecordLimits>` keyed the same way as `effort::declared_support` (`IC/provider/effort.rs:303`): `context_window_tokens`, `max_output_tokens`, `lifecycle: Ga|Preview|Deprecated`. Rows only for models whose numbers are sourced (vendor docs cited in a comment per row); no row gives 0 and empty lifecycle. Honour the effort alias env `INFERENCE_CORE_EFFORT_MODEL_ALIASES` so an Azure deployment name maps to its base model.
- `IC/provider/mod.rs` - internal `ModelInfo` (`:489-510`) gains `context_window_tokens: u32`, `max_output_tokens: u32`, `lifecycle: &'static str`. Filled in each provider's `list_models` (`anthropic.rs:1478`, `openai.rs`, `codex_subscription.rs:894`) from the table, clamped to the provider's own `max_*` when the provider's is lower (Foundry's 8,192 output stays the truth for Foundry).
- `IC/grpc.rs` - `From<provider::ModelInfo>` (`:65-89`) maps the three fields.
- `PROTO/inference.proto` - `ModelInfo` add, with doc comments naming EP-13 S2:
  - `uint32 context_window_tokens = 9;` (0 = unknown)
  - `uint32 max_output_tokens = 10;` (0 = unknown; the serving surface's ceiling)
  - `string lifecycle = 11;` ("ga", "preview", "deprecated"; empty = unknown)
  Next free after this slice: 12. Verified next free at HEAD: 9.
- `MG/http_routes.rs` - `ModelDescriptor` gains the three fields, each skipped when zero or empty, so the JSON for an unknown model is unchanged.
- `FES/shared/api/chat-client.ts` - `ModelInfo.contextWindow?`, `maxOutputTokens?`, `lifecycle?`; picker rows show "200k kontekst" and a "Forhåndsversjon" chip for preview; deprecated models fold under a "Eldre modeller" disclosure in `groupChatModels` (`:4025`).
- `MP/go/gen/model_plane/v1/inference.pb.go` regenerated.

**Contract changes.** Proto additive (3 fields). No migration.
**Flag.** None: read-only metadata, absent when unknown, rollback by revert. (No switch is added, so the "default off" rule has nothing to apply to; say so in the PR.)
**ZDR.** None; metadata only.
**Tests.**
- `IC/provider/model_record.rs` unit: `every_row_has_a_source_comment` (source-text test), `unknown_model_has_zero_limits`, `alias_maps_to_base_row`, `provider_ceiling_clamps_the_table`.
- `IC/grpc.rs` unit: `list_models_carries_record_v2_fields`.
- `MPR/services/model-gateway/tests/effort_ladder_contract.rs` sibling test `list_models_carries_context_and_lifecycle`.
- FE: `chat-client.model-record.test.ts`, `groupChatModels` deprecated fold test.
- Commands: `cargo test -q -p inference-core --lib model_record`, `cargo test -q -p model-gateway --test effort_ladder_contract`, `buf lint`, `buf breaking --against baseline.binpb`, vitest as S1.
**Live proof.** Rebuild inference-core, model-gateway, frontend. `/api/v1/models` shows `context_window_tokens` and `max_output_tokens` for `claude-sonnet-5-5` equal to the sourced row; a model with no row shows neither. Screenshot. Evidence file as S1.
**Effort.** M, 3-5 d (most time is sourcing numbers). **Deps.** WS11-S5 code (done). **Risks.** Wrong vendor numbers: every row cites its source; a reviewer checks them. buf lint ratchet: keep comments on each new field.

### S3 - Purpose tag on every inference call; side tasks inherit floors [BUILD] (042)

**Goal.** Every call says what it is for, so metrics (and later cost) can tell an answer from a title. Side-task calls stop dropping the turn's privacy floors (D8).

**Files.**
- `PROTO/inference.proto` - NEW `enum InferPurpose` (additive, doc-commented, owner tag EP-13 S3): `INFER_PURPOSE_UNSPECIFIED = 0; ANSWER = 1; TOOL_ROUND = 2; AGENT_ROUND = 3; TITLE = 4; FOLLOW_UPS = 5; VERIFICATION_JUDGE = 6; RESEARCH_PLAN = 7; RESEARCH_SYNTHESIS = 8; COMPACTION_SUMMARY = 9; MEMORY_EXTRACTION = 10; MEMORY_GROUNDING = 11; BROWSER_PLANNER = 12; SAFETY_SCREEN = 13; VALIDATION = 14; DICTATION_CLEANUP = 15; SCHEDULED_ROUND = 16;`. `InferRequest` add `InferPurpose purpose = 21;` (verified next free at HEAD: 21; WS-era reservations end at 20 in plan 4.3; the integrator assigns if another W3 design claims 21).
- `IC/metrics.rs` - add a `purpose` label to `inference_requests_total` and `inference_tokens_total` from the enum's closed name set (bounded cardinality, matches the module's rule at `:20-31`); unknown numerics become `unspecified`. Document in the module header.
- `IC/grpc.rs` - read `purpose` (no routing effect in this slice); an unknown numeric is accepted as unspecified (a newer client must not be refused for a label).
- Call sites (non-test): gateway `sse.rs` (answer, title `:9746`, follow-ups, judge `:9578`), `tool_loop.rs`, `deep_research.rs` (plan, synthesis), `result_validation.rs`, `source_validation.rs`, `review_proposal_rule.rs`, `http_routes.rs` (dictation and others), `grpc.rs`, `vision_parts.rs`, `semantic_screening.rs`; execution-core `runtime_loop/agent.rs` (agent round), `llm_planner.rs` (browser planner), `scheduled_agent.rs` path, `grpc.rs`; session-core `dream_extractor.rs`, `memory_grounding.rs`. A source-text test lists every non-test `InferRequest {` construction and fails if one lacks `purpose` (pattern: `zdr_refusal_inventory.rs`).
- `MG/sse.rs` - `subscription_scoped_request` (`:802-812`) splits into two steps: `inherit_turn_floors(request, route)` always copies `min_privacy_tier` and `min_residency` (and `zdr` for calls that carry turn content, the judge already does), then the subscription step as today. Title stays skipped for ZDR turns (call site unchanged).
- `MG/usage_meter.rs` - `CallPhase` gains `fn purpose(self) -> InferPurpose` for the four existing phases; no meter change (WS27 owns the meter).

**Contract changes.** Proto additive (one enum, one field). No migration.
**Flag.** None for the tag (a label, no behaviour). The floor inheritance is an ungated tightening (D8); the PR states it and the ledger entry records it.
**ZDR.** The tag carries no content. Floor inheritance only narrows which providers a side task may reach; ZDR side tasks keep their existing behaviour.
**Tests.**
- `MPR/services/model-gateway/tests/infer_purpose_inventory.rs` - NEW `every_non_test_infer_request_names_a_purpose` (source text over gateway, execution-core, session-core sources).
- `MG/sse.rs` unit: `title_request_inherits_the_turn_privacy_floor`, `judge_request_inherits_residency_floor`, `subscription_route_still_overrides_model_and_hint`.
- `IC/metrics.rs` unit: `purpose_label_is_bounded`, `unknown_purpose_counts_as_unspecified`.
- `MPR/services/inference-core/tests/span_attribution_test.rs` extend: purpose recorded on the span.
- Commands: `cargo test -q -p model-gateway --lib sse::`, `cargo test -q -p model-gateway --test infer_purpose_inventory`, `cargo test -q -p inference-core --lib metrics`, `-p execution-core --lib`, `-p session-core --lib`, `buf lint`, `buf breaking`.
**Live proof.** Rebuild inference-core, model-gateway, execution-core, session-core. One chat turn on `claude-sonnet-5-5`; `:8082/metrics` shows `purpose="answer"`, `purpose="title"`, `purpose="follow_ups"` series moving. Evidence file `ep13-s3-purpose-<date>.txt` with the metric lines.
**Effort.** M, 3-4 d. **Deps.** none beyond HEAD; pairs with WS27-S2 (partial, D2-567 gaps now nameable). **Risks.** Many hot files (`sse.rs`, `agent.rs`, `tool_loop.rs`): one mechanical commit per crate; merge after the current train. A missed call site: the inventory test catches it.

### S4 - Role table and the `verevon-role` resolver [BUILD] (042)

**Goal.** Side-task models come from a platform role table, resolved where the catalogue, entitlement and floors are known, instead of one env var.

**Files.**
- `IC/provider/routing_policy.rs` - `RoutingPolicy` gains `#[serde(default)] pub roles: RoleTable` where `RoleTable = BTreeMap<RolePurpose, RoleEntry>`, `RoleEntry { candidates: Vec<String>, #[serde(default)] effort: Option<String> }`. Roles in W3: `title`, `follow_ups`, `verification_judge`, `research_plan`. Default table empty, so a policy without `roles` behaves exactly as today. Validation `fn validate_roles(&self, catalogue, effort tables) -> Result<(), RoleError>`: purpose known, 1-4 candidates, each id in the chain catalogue, small-budget purposes only with models that accept a no-reasoning call (D7), effort a canonical name.
- `IC/http_health.rs` - `put_policy` (`:137`) runs `validate_roles` and refuses with 400 `{error:{code:"invalid_role_table", details:[...]}}` before persisting.
- `IC/provider/role.rs` - NEW resolver. `pub const ROLE_MODEL_ID: &str = "verevon-role";`. For a request with `model == ROLE_MODEL_ID`: require `purpose` (else `invalid_argument`); walk `roles[purpose].candidates`; keep the first that the chain serves, the caller is entitled to (`chain.entitlement().resolve`, same answer `ListModels` uses), and that meets `zdr`/`min_privacy_tier`/`min_residency`; else the request's `fallback_model` (below); else `policy.cheap_fallback`. Record which in `RouteNote`.
- `IC/provider/fallback.rs` - call the resolver before `is_unspecified_model` (`:315-321`) so `verevon-role` never reaches a provider verbatim.
- `PROTO/inference.proto` - `RouteNote` add `string role = 5;` ("" when no role table entry chose the model; else the purpose name) and `string role_source = 6;` ("table", "turn_model", "cheap_fallback"). Next free at HEAD: 5. `InferRequest` add `string role_fallback_model = 22;` (the turn's own resolved model, which the gateway already knows; used only with `verevon-role`). Next free after S3: 22.
- `MG/sse.rs`, `MG/deep_research.rs` - behind `MODEL_GATEWAY_ROLE_ROUTING` (NEW, vocabulary C, default off; constant in a NEW `MG/role_routing.rs`): title, follow-ups and judge send `model: "verevon-role"`, the matching `purpose`, `role_fallback_model` = the turn's served model; research plan sends `verevon-role` with `research_plan` and fallback `verevon-budget`. Switch off: byte-identical requests to S3.
- `MP/deploy/docker-compose.yml` - `MODEL_GATEWAY_ROLE_ROUTING: ${MODEL_GATEWAY_ROLE_ROUTING:-false}`.
- Rollout order: inference-core first (it understands `verevon-role` whenever asked), then the gateway switch. An old inference-core would 404 `verevon-role`, so the compose comment states the order.

**Contract changes.** Proto additive (`RouteNote` 5, 6; `InferRequest` 22). Routing-policy JSON additive (`roles`). No migration (session-core stores the JSON string).
**Flag.** `MODEL_GATEWAY_ROLE_ROUTING`, default off. Turn-on: the S4 live proof (below) plus S5 shipped, so admins can see and edit what is used.
**ZDR.** No new store, no content. Resolver honours ZDR and floors; a ZDR judge call with no attested candidate falls to the turn's model, which already served the ZDR turn, so it is attested by construction. Title is still never sent for ZDR turns.
**Tests.**
- `IC/provider/role.rs` unit: `role_id_without_purpose_is_invalid_argument`, `first_entitled_candidate_wins`, `candidate_absent_from_catalogue_is_skipped`, `zdr_request_skips_unattested_candidates`, `eu_floor_skips_global_candidate`, `small_budget_role_rejects_reasoning_only_model`, `empty_table_uses_turn_model`, `route_note_names_role_and_source`.
- `IC/provider/routing_policy.rs` unit: `policy_without_roles_deserialises_to_empty_table`, `roles_round_trip`.
- `IC/http_health.rs` unit: `put_policy_refuses_unknown_model_in_roles`.
- `MPR/services/inference-core/tests/fallback_test.rs` extend: `verevon_role_never_reaches_a_provider_verbatim`.
- `MPR/services/inference-core/tests/model_entitlement_chain_test.rs` extend: `role_candidate_not_entitled_is_skipped`.
- `MG/role_routing.rs` unit: switch parser; `MG/sse.rs` unit `aux_requests_use_role_id_only_when_switch_on`.
- Commands: `cargo test -q -p inference-core --lib role`, `--test fallback_test`, `--test model_entitlement_chain_test`, `cargo test -q -p model-gateway --lib role_routing`, `buf lint`, `buf breaking`.
**Live proof.** Anthropic-only stack (only `claude-sonnet-5-5` allowed): PUT a role table with `title: ["gpt-4o-mini","claude-sonnet-5-5"]`; switch on; one turn. Evidence: the title appears; inference-core logs and `RouteNote` show `role=title, role_source=table` and model `claude-sonnet-5-5` (gpt-4o-mini skipped as absent from the chain); `MODEL_GATEWAY_AUX_MODEL` unset. Then PUT an empty table: title still appears via `turn_model`. Switch off: requests identical to S3 (diff of logged request shapes).
**Effort.** M, 4-6 d. **Deps.** S3 (purpose field). WS10-S3 done (entitlement resolve). **Risks.** (1) The policy is platform-wide: a bad table affects every org; validation at PUT plus resolve-time skip keeps it non-fatal. (2) A role pointing at a reasoning-only model starves the 24-token title: validation refuses it; D7. (3) Two sources of truth while `MODEL_GATEWAY_AUX_MODEL` exists: keep the env as the last fallback until the switch is default-on, then retire it in a later ledger entry.

### S5 - Role editor; the policy PUT round-trips every field [BUILD] (042)

**Goal.** An admin can see and edit the role table, and a save never silently resets fields the page does not show.

**Files.**
- `FES/shared/actions/action-registry.ts` - `routerPolicyUpdateInput` (`:1598-1623`) gains `tool_fallback_ladder: z.array(z.string().min(1)).max(8)` and `roles: z.record(z.enum(['title','follow_ups','verification_judge','research_plan']), z.object({ candidates: z.array(z.string().min(1)).min(1).max(4), effort: z.string().optional() })).optional()`. Action `router_policy.update` (`:3906`) keeps its high-risk, requires-approval posture.
- `FES/features/router-policy/components/RouterPolicyPage.tsx` - the page keeps the loaded document and sends it back whole (fixes the ladder reset), adds a "Modeller for hjelpeoppgaver" section: one row per purpose, ordered candidate chips chosen from the chat catalogue (S1/S2 record shows context and price), a note that the first eligible model is used and the turn's own model is the fallback. Server errors `invalid_role_table` render per row.
- `FES/features/router-policy/api.ts` (or the existing client file the page uses) - types.
- BFF: none (`BFF/domains/router_policy.rs` already proxies the whole JSON).

**Contract changes.** Action input schema widened (additive). No proto, no migration.
**Flag.** None in FE (server behaviour is S4's switch; `flags-and-config.md` 8.4 forbids new VITE flags for server behaviour). The section renders read-only with a "ikke aktivert" note when `/api/v1/models` shows no role routing [needs a sibling `role_routing: bool` on `/v1/models`, set from the S4 switch: one line in `MG/http_routes.rs`].
**ZDR.** None.
**Tests.** `FES/features/router-policy/components/RouterPolicyPage.test.tsx` (new or extended): `save_round_trips_tool_fallback_ladder`, `save_round_trips_roles`, `invalid_role_table_error_marks_the_row`; `action-registry` contract test for the widened schema. Commands: `pnpm exec vitest run src/features/router-policy src/shared/actions`, `pnpm lint`, `tsc6 -p tsconfig.app.json`.
**Live proof.** Second Vite server; edit the ladder and a role, save, reload, both persist (`GET /api/v1/router-policy`). Screenshot at 1280 and 375 px. Evidence file `ep13-s5-roles-<date>.txt`.
**Effort.** S-M, 2-3 d. **Deps.** S4 (server validation). The ladder round-trip half can ship before S4. **Risks.** The page is admin-only by SPA gate plus scope; do not widen who can write.

### S6 - Gemini adapter core on Vertex, registered dark [BUILD] (041)

**Goal.** inference-core can serve Gemini chat (unary and streamed text) from a Vertex EU location, declares itself honestly (EU-resident, never ZDR, no embeddings), and never registers unless explicitly enabled.

**Files.**
- `IC/provider/vertex_auth.rs` - NEW short-lived token provider. Reads a service-account JSON from `VERTEX_GEMINI_CREDENTIALS_FILE` (a mounted secret; never the env value itself), signs a JWT with `jsonwebtoken` (already a dependency, `IC/../Cargo.toml:39`), exchanges it at Google's OAuth token endpoint for an access token with the `cloud-platform` scope [INF: confirm endpoint and scope against Google's docs at build], caches it in memory, refreshes 5 minutes before expiry, single-flight. Debug output redacts the key and token. Workload-identity metadata server as a later option (not in W3).
- `IC/provider/gemini.rs` - NEW `GeminiProvider` implementing `ProviderRouter`: `infer` and `infer_stream` over `{location}-aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/publishers/google/models/{model}:generateContent` and `:streamGenerateContent?alt=sse` [INF: exact host for the EU multi-region endpoint to confirm]. Maps `ChatMessage` roles (system to `systemInstruction`, assistant to `model`), `max_tokens`, `temperature`, `structured_output_schema` (to `responseSchema` only after the S7 sanitiser; until then refuse with `invalid_argument` kind `feature_unsupported` so the chain moves on), usage (`promptTokenCount`, `candidatesTokenCount`, `cachedContentTokenCount` to `cache_read_input_tokens`), stop reasons, safety blocks to a typed provider error. `image_parts`: refuse `vision_unsupported` in W3 (no typed media parts; EP-5/F-t3-090). `capabilities()`: `provider_id "vertex-gemini"`, `model_family: Gemini`, `residency` from D12, `exclusive_catalog: true` (serves only `VERTEX_GEMINI_MODELS`), `supports_tools: false` until S7, `supports_vision: false`, `supports_thinking: false` until S7, `supports_embeddings: false`, `supports_zdr: false` (constant; no attestation is read), limits from the S2 table. `list_models` from the configured catalogue with record v2 fields.
- `IC/provider/mod.rs` - `ModelFamily::Gemini` (serde `gemini`). `IC/provider/fallback.rs` - `provider_serves_model` (`:551-570`): `Gemini => is_gemini_model(model)`; `OpenAiCompatible => !is_anthropic_model(model) && !is_gemini_model(model)` (the finding in section 2); registration arm `"vertex-gemini" | "gemini"` (`:687-800`) that registers only when `VERTEX_GEMINI_ENABLED` is truthy (vocabulary C) AND project, location, credentials file and a non-empty model list are present; otherwise log one warning and skip. Boot residency log (`:1005-1025`) covers it unchanged.
- `IC/config.rs` - read `VERTEX_GEMINI_ENABLED`, `VERTEX_GEMINI_PROJECT`, `VERTEX_GEMINI_LOCATION`, `VERTEX_GEMINI_MODELS`, `VERTEX_GEMINI_CREDENTIALS_FILE`. EU allowlist `fn is_vertex_eu_location` next to `is_eu_region`.
- `IC/provider/effort.rs` - `Family::Gemini` with `declared_support` returning `Wire::None` for every Gemini model in S6 (effort is S7).
- `IC/provider/model_record.rs` (S2) - Gemini rows with sourced limits.
- No proto change (family is internal; `provider` string is data). No change to `connection-zdr-capabilities.json` (platform provider, not a connection).

**Contract changes.** None on the wire. New env keys (secret: `VERTEX_GEMINI_CREDENTIALS_FILE`; runtime: the rest).
**Flag.** `VERTEX_GEMINI_ENABLED`, default off, plus the provider must be named in `INFERENCE_PROVIDER_ORDER`. Not added to compose in this slice (S8 adds it, off). Turn-on: S8 gate only.
**ZDR.** New egress: prompt content to Google Vertex (a new subprocessor). Never for ZDR turns: `supports_zdr` is the constant false, so the chain's ZDR gate skips it and a ZDR turn with no other provider gets the existing typed `zdr_unavailable`. A floor of `ZDR_CONTRACTUAL` or `SOVEREIGN` skips it; `EU_RESIDENT` reaches it only with an allowlisted EU location. Waived runs (DEC-32) name a user connection route, never a platform provider, so Gemini is never a waiver route. Context caching: the adapter sends no explicit cache; Vertex implicit caching and abuse logging are the reasons it is not ZDR [INF, the D1 owner note records it].
**Tests.** (wiremock or the crate's existing mock pattern; no network)
- `IC/provider/vertex_auth.rs` unit: `token_is_cached_until_refresh_margin`, `concurrent_callers_share_one_exchange`, `debug_redacts_key_and_token`, `missing_file_refuses_registration_not_boot`.
- `IC/provider/gemini.rs` unit: `maps_system_to_system_instruction`, `maps_usage_and_cache_tokens`, `safety_block_is_typed`, `stream_assembles_deltas_and_usage`, `image_parts_refuse_vision_unsupported`, `structured_output_refused_until_sanitiser`.
- `MPR/services/inference-core/tests/fallback_test.rs`: `gemini_id_never_reaches_openai_family`, `claude_id_never_reaches_gemini`, `zdr_request_never_reaches_vertex_gemini`, `zdr_contractual_floor_skips_vertex_gemini`, `eu_floor_reaches_vertex_gemini_only_on_eu_location`, `vertex_gemini_absent_without_enabled_switch`.
- `IC/config.rs` unit: `vertex_location_allowlist`, `global_location_classifies_global`.
- Commands: `cargo test -q -p inference-core --lib gemini`, `--lib vertex_auth`, `--test fallback_test`, `cargo check --workspace --all-targets`.
**Live proof.** None possible under the owner's live rule; `[verify live]` moves to S8. Local evidence: unit and fallback test output; inference-core boots with the switch off and the provider absent from `/v1/models` on the Anthropic-only stack (regression proof).
**Effort.** M, 5-7 d. **Deps.** S2 (record table) for limits; nothing else. **Risks.** Vertex API details are [INF] until tested against Google; the adapter is dark, so a wrong detail costs nothing until S8. Credential file handling must not log paths with secrets.

### S7 - Gemini tools, schema sanitiser and effort [BUILD] (041)

**Goal.** Gemini can take part in tool rounds and honours the effort ladder.

**Files.**
- `IC/provider/gemini_schema.rs` - NEW sanitiser from `ToolDefinition.parameters_json` (JSON Schema) to Vertex `functionDeclarations` parameters (an OpenAPI 3 subset [INF]). Rules: inline local `$ref`s up to a depth bound, drop keywords Vertex rejects (`$schema`, `additionalProperties`, `patternProperties`, `const` to single-item `enum`, numeric `exclusiveMinimum`), collapse `type: [x, "null"]` to `nullable`, cap enum length. A schema it cannot express drops only that tool and counts `inference_gemini_tool_dropped_total{reason}`; it never rewrites semantics silently (each rule has a test). Arguments are still checked downstream (`MG/argument_repair.rs`, execution-core validation).
- `IC/provider/gemini.rs` - tool calls: `functionCall` parts to `InferResponse.tool_calls` (ids synthesised as `gemini-call-{n}` because Vertex returns none [INF]); `tool_choice` mapping (`auto`, `none`, named to `ANY` with `allowedFunctionNames`); thought signatures ignored (D5). `supports_tools: true`, `supports_thinking: true`. `structured_output_schema` through the same sanitiser.
- `IC/provider/effort.rs` - `Wire::GeminiThinkingBudget` (`thinkingConfig.thinkingBudget` through the existing `allowance` table) and `Wire::GeminiThinkingLevel` (`thinkingConfig.thinkingLevel` low or high) [INF: which Gemini generation takes which; confirm at build]. `declared_support(Family::Gemini, model)` per sourced row; `advertised_effort_levels` so `ModelInfo.effort_levels` is honest. Behind the existing per-adapter effort switch pattern (WS11-S2/S3): `INFERENCE_GEMINI_EFFORT_ENABLED`, default off.

**Contract changes.** None on the wire.
**Flag.** `INFERENCE_GEMINI_EFFORT_ENABLED` (default off). Tools need no separate switch: the provider itself is off until S8.
**ZDR.** Same as S6; tool schemas are platform metadata, tool results are turn content already covered by S6's egress clause.
**Tests.**
- `IC/provider/gemini_schema.rs` unit, one per rule: `inlines_local_ref`, `drops_additional_properties`, `const_becomes_single_enum`, `nullable_from_type_union`, `unexpressible_schema_drops_only_that_tool`, `sanitiser_is_idempotent`; plus a golden over every built-in tool schema in the registry fixture (`every_builtin_tool_schema_sanitises`).
- `IC/provider/gemini.rs` unit: `function_call_maps_to_tool_calls_with_stable_ids`, `named_tool_choice_maps_to_any_with_allowlist`, `thought_signature_is_ignored`.
- `IC/provider/effort.rs` unit: `gemini_wire_classes`, `gemini_levels_advertised_only_with_switch`.
- Commands: `cargo test -q -p inference-core --lib gemini`, `--lib effort`.
**Live proof.** `[verify live]` in S8: a two-round tool turn (proves D5) and a deep-effort turn.
**Effort.** M, 4-5 d. **Deps.** S6; WS11-S2/S3 pattern (done). **Risks.** Sanitiser over- or under-strips: the built-in tool golden runs in CI; dropped tools are counted, not hidden.

### S8 - Gemini enablement gate: prices, release gate, wiring, owner-funded live proof [FINISH] (041)

**Goal.** Everything needed to switch Gemini on is in place and refuses to pass until the owner's three decisions are met (D1 posture ruling, D2 sourced prices, D3 funded live run).

**Files.**
- `CC/migrations/0012_gemini_model_pricing.up.sql` and `.down.sql` (cost-core head at HEAD: `0011_cache_miss_reason`; propose `0012`, integrator assigns). Idempotent `INSERT ... ON CONFLICT (model) DO UPDATE` rows for the exact Gemini model keys, values **supplied by the owner or operator with a cited source** (D2). Until supplied the file is not merged; there is no placeholder price.
- `MP/deploy/docker-compose.yml` - inference-core: `VERTEX_GEMINI_ENABLED: ${VERTEX_GEMINI_ENABLED:-false}`, the four config keys with empty defaults, the credentials file as a Docker secret mount; comment states cost and posture (EU, not ZDR).
- `MP/scripts/release-artifact.sh` - a Vertex route check beside `:1001-1022`: when `VERTEX_GEMINI_ENABLED=true`, require project, an allowlisted EU location, a model catalogue matching `^[A-Za-z0-9._-]+(,...)*$`, a credentials secret reference (secret partition), and refuse any `VERTEX_GEMINI_ZDR_*` key (no attestation path exists in W3, so asserting one is an error). Matching cases in `MP/scripts/tests/release-runtime-partition-test.sh` and `release-input-security-test.sh`.
- `docs/compliance/evidence/tools/export-provider-posture.sh` - add `VERTEX_GEMINI_ENABLED` and `VERTEX_GEMINI_LOCATION` to the posture snapshot (non-secret).
- `MP/docs/decisions/ledger.md` - entry "Gemini on Vertex is an EU-resident, non-ZDR provider until attested" with the owner's D1 ruling, the price source, and what proof was run.
- `MP/docs/ZDR.md` - one row: Vertex Gemini, not ZDR-capable, skipped for ZDR turns.
- Optional, owner's choice after the live run: a role-table candidate entry or a routing-table cell using Gemini (S4/S5 surfaces), never a default in code.

**Contract changes.** cost-core seed rows; release-gate rules. No proto.
**Flag.** Turns `VERTEX_GEMINI_ENABLED` on only on the proof stack, after D1 and D2; production stays off until the owner flips it.
**ZDR.** Golden cases (inference-core, since the posture matrix models no platform routes): `zdr_request_never_reaches_vertex_gemini`, `zdr_contractual_floor_skips_vertex_gemini`, `eu_floor_reaches_vertex_gemini_only_on_eu_location` (from S6). `zdr-posture-matrix.json`: no new row; its decision layer is unchanged, and the PR says so so the reviewer does not look for one. `zdr-refusals.json`: no new code; `zdr_unavailable` covers "only an unattested provider was eligible". If the posture matrix later gains platform-route rows, add `d_zdr_on_only_unattested_platform_provider` with expect `refuse zdr_unavailable` [verify the matrix's scope with its owner].
**Tests.** Release scripts (Linux or CI): `vertex_route_requires_eu_location`, `vertex_route_refuses_zdr_keys`, `vertex_credentials_are_secret_partition`. cost-core: `go test ./...` with `GOWORK=off` (seed upsert idempotent on second boot, pattern `CC/internal/postgres/store_integration_test.go:311`). Compose contract assertion modelled on `letta-tool-search-compose-contract-test.sh`: switch defaults false.
**Live proof `[verify live]` (owner-funded, D3).** On a side-by-side proof stack, never the shared dev stack: Vertex project in an allowlisted EU location, owner-provided service account. (1) `/api/v1/models` lists the Gemini models with `privacy_tier=eu_resident`, `residency=eu`, record v2 fields and S1 price; (2) a streamed answer from a Gemini model with a priced `usage` frame and a cost-core ledger row at the sourced price; (3) a two-round tool turn (proves D5); (4) a temporary-chat (ZDR) turn with only Gemini selectable gets `zdr_unavailable` and zero thread/message rows; (5) an `EU_RESIDENT`-floored turn reaches Gemini, a `ZDR_CONTRACTUAL`-floored turn does not; (6) `inference_requests_total{provider="vertex-gemini",purpose=...}` moves. Evidence file `docs/compliance/evidence/ep13-gemini-live-<date>.txt` with quoted outputs. Until this run exists the switch stays off everywhere and the queue status is `blocked-external`.
**Effort.** S-M, 2-3 d of build plus the owner's live session. **Deps.** S6, S7, S1, S2; D1, D2, D3. **Risks.** Release scripts cannot run on Windows (D1-138): this slice is not done until CI runs them.

**Dependency summary.** S1, S3 and S6 can start now and in parallel (S6 needs S2's table only for limits; it can start with provider-level limits and switch to the table when S2 lands). S2 before S7 rows. S3 before S4. S4 before S5's role half (the ladder round-trip half of S5 can go first). S6 before S7 before S8. S8 also waits on D1, D2, D3.

**Migrations and proto (for the integrator).**
- cost-core: `0012_gemini_model_pricing` (head `0011`, propose `0012`), S8, owner-sourced values only.
- session-core (head `0071`) and capability-core (head `0030`): none.
- `inference.proto`: `ModelInfo` 9, 10, 11 (S2; next free verified 9); `enum InferPurpose` new and `InferRequest.purpose = 21` (S3; next free verified 21); `InferRequest.role_fallback_model = 22` (S4); `RouteNote.role = 5`, `RouteNote.role_source = 6` (S4; next free verified 5). `ChatMessage` field 5 stays free (D5). `InferChunk` next free 17 is not used (plan 4.3 notes WS15's withdrawn retry would take 17).

**Switches (all default off).** `MODEL_GATEWAY_CATALOGUE_PRICES` (S1), `MODEL_GATEWAY_ROLE_ROUTING` (S4), `VERTEX_GEMINI_ENABLED` (S6/S8), `INFERENCE_GEMINI_EFFORT_ENABLED` (S7). Turn-on proofs: S1 live proof plus D10; S4 live proof plus S5 shipped; S8 owner-funded run plus D1 and D2; S7 the S8 deep-effort turn.

## 5. ZDR clause

| New store or egress | Slice | Holds content? | ZDR behaviour | Golden case |
|---|---|---|---|---|
| `price` sibling on `/v1/models` | S1 | No (catalogue) | Same for all postures | `price_is_a_sibling_field_or_absent` (contract test) |
| Record v2 fields | S2 | No | Same for all postures | `list_models_carries_record_v2_fields` |
| `purpose` metrics label | S3 | No (closed enum, no org or user label) | Unchanged; ZDR calls are counted like others, as today | `purpose_label_is_bounded` |
| Side-task floor inheritance | S3 | Narrows egress | Title still never sent for ZDR turns; judge keeps `zdr`; floors now always applied | `title_request_inherits_the_turn_privacy_floor`, `judge_request_inherits_residency_floor` |
| `roles` in the routing-policy JSON | S4 | No (model ids) | Resolver skips unattested candidates for ZDR calls; falls back to the turn's attested model | `zdr_request_skips_unattested_candidates` |
| Egress to Google Vertex (prompts, tool schemas and results, outputs) | S6-S8 | Yes, in transit; Vertex implicit caching and abuse logging are not attested [INF] | Never ZDR: `supports_zdr` constant false; ZDR turns refuse with existing `zdr_unavailable` when Gemini is the only eligible provider; `ZDR_CONTRACTUAL`/`SOVEREIGN` floors skip it; never a waiver route | `zdr_request_never_reaches_vertex_gemini`, `zdr_contractual_floor_skips_vertex_gemini`, `eu_floor_reaches_vertex_gemini_only_on_eu_location` (all in `inference-core/tests/fallback_test.rs`) |
| Vertex access token (in memory) | S6 | Credential, no content | Never logged, never persisted | `debug_redacts_key_and_token` |
| cost-core Gemini price rows | S8 | No | Prices only; a ZDR turn is still not ledgered (DEF-Z-06b unchanged) | seed idempotency test |

`zdr-posture-matrix.json`: no new rows (it models org posture and user connections, not platform routes; checked all 44 rows). `zdr-refusals.json`: no new codes. If either file is edited, re-pin `.sha256` and run `bash scripts/tests/zdr-contract-sync.test.sh` (compare the failing set with HEAD, D1-145).

## 6. Proof plan

| Slice | Unit / contract | Live (owner's rule: Anthropic key, `claude-sonnet-5-5`) | Evidence |
|---|---|---|---|
| S1 | gateway price tests, FE search and parse tests | Anthropic stack: `/api/v1/models` with switch off (unchanged) and on (Sonnet priced by prefix); both pickers searched, 1280 and 375 px | `ep13-s1-catalogue-<date>.txt`, screenshots |
| S2 | record table, grpc mapping, gateway contract | `/api/v1/models` shows sourced limits for `claude-sonnet-5-5`, none for an unlisted model | same file |
| S3 | purpose inventory, floor inheritance, metrics | one turn; `:8082/metrics` purpose series move | `ep13-s3-purpose-<date>.txt` |
| S4 | resolver, policy validation, chain and entitlement tests | role table with an absent first candidate; title served by `claude-sonnet-5-5` with `role_source=table`; empty table falls to the turn model; switch off identical | `ep13-s4-roles-<date>.txt` |
| S5 | page round-trip tests | edit ladder and role, reload, both persist | `ep13-s5-roles-<date>.txt` |
| S6 | adapter, auth, chain gates | none possible; regression: switch off, provider absent, boot clean | test output |
| S7 | sanitiser goldens, tool and effort mapping | none possible here | test output |
| S8 | release-gate cases in CI, cost-core seed | `[verify live]` owner-funded six-step Gemini run (section 4) | `ep13-gemini-live-<date>.txt` |

The epic is "done" for W3 when S1-S5 are live-proven on the Anthropic stack and S6-S8 are merged dark with S8's gate recorded as `blocked-external` waiting for the owner. Gemini is "enabled" only after the S8 live run.

Regression guards (audit section 8 strengths): the served-by line and intent routing are extended, not replaced (`RouteNote` gains fields; `RoutingPolicy` gains a defaulted field); credential isolation holds (the Vertex key lives only in inference-core, never reaches a model, the sandbox or the browser); deny-by-default residency and ZDR gates are reused, not bypassed; the honest-cost rule holds (no default-row price shown, no invented Gemini price).

## 7. Risks and what the owner must decide

**Owner decisions (blocking only what is named):**
1. D1 - Gemini posture: accept Vertex Gemini as an EU-resident, non-ZDR provider for non-ZDR turns (narrowing DEC-40), or wait for a Vertex ZDR attestation. Blocks S8 enablement only.
2. D2 - Supply sourced Gemini prices for cost-core. Blocks S8 merge of the seed rows and enablement.
3. D3 - Fund the Gemini live test (Vertex project in an EU location, a service account, spend) outside the current Anthropic-only rule. Blocks S8 DoD.
4. D10 - Product: may members see list prices in the picker. Blocks the S1 switch flip only.
5. D8 - Confirm the side-task floor tightening ships ungated (default: yes). Blocks nothing unless vetoed.

**Risks.**
- Platform-wide role table: one bad PUT changes side-task models for every org. Mitigated by PUT validation, resolve-time skips and the never-fail rule; per-org tables stay W4.
- Price display can mislead when a catalogue key is a prefix (e.g. `claude-sonnet` covers several versions at one price). The response states `source: prefix`; the FE shows "listepris" without claiming the exact model.
- Vertex API details (endpoints, thinking config by generation, function-call ids, schema subset) are [INF] until the S8 run. Everything Gemini is dark until then, so wrong details cost no user traffic.
- Ownership drift: the price must stay in cost-core and the record in inference-core; a reviewer rejects any copy of price into `ModelInfo` proto or of limits into cost-core.
- Hot files: `MG/sse.rs`, `MG/http_routes.rs`, `EC/runtime_loop/agent.rs`, `MG/tool_loop.rs`, `DashboardComposer.tsx`, `chat-client.ts`, `inference.proto`. S3 is the widest; land it alone in its train.
- Proto numbering: other W3 designs written in parallel may also claim `InferRequest` 21/22 or `ModelInfo` 9+. Numbers here are proposals; the integrator assigns in merge order and records them in the plan 4.3 table and the ledger.
- `MODEL_GATEWAY_AUX_MODEL` and the role table coexist until the S4 switch is default-on; a later ledger entry retires the env var.
