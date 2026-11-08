# WS11-effort-ladder - Canonical reasoning-effort ladder

Item: F-t3-029 (Recommended, wave W1, audit effort "M (was S)"). Design date 2026-10-15. Worktree HEAD afb83928; live tree = same HEAD + uncommitted work. Path shorthand: `MP` = `apps/Model Plane`; `RS` = `MP/rust/services`; `FE` = `apps/Frontend Plane/verevonv3`; `IC` = `RS/inference-core/src`; `MG` = `RS/model-gateway/src`. Lines are worktree (WT) unless tagged `[live]`.

## 1. Scope, items resolved, owner

**Item.** F-t3-029: one canonical effort ladder, clamped per model, rendered per provider wire format. Today the user's effort reaches a provider only as a derived token budget; the OpenAI/Azure adapter ignores it; Anthropic gets a legacy `budget_tokens` shape; Codex gets low or high.

**Owner.** Model Plane. `inference-core` owns the ladder, the per-model table and every wire rendering (matrix: `MP/docs/capability-ownership-matrix.md:63`, `apps/master-ownership-matrix.md:127`). `model-gateway` owns the user-facing profile and the HTTP/SSE surface. Frontend Plane owns the composer. Control Plane owns any per-role cap (entitlement), Ingestion Plane owns the Codex broker. No cross-plane DB access is introduced.

**IN.**
- A canonical ladder (`Minimal < Low < Medium < High < XHigh < Max`, plus "unspecified = provider default") and a pure `resolve` step in inference-core that all three adapters call: clamp to the nearest weaker supported level, then render the provider vocabulary.
- Per-model support table (data, with `verified_on`), surfaced to the SPA as `ModelInfo.effort_levels`.
- One derivation function in model-gateway used by the normal chat path AND the ZDR direct path.
- Composer levels driven by the catalogue.
- Fixes for four defects found while reading the code (section 2, items D1-D4).

**OUT.**
- Do/Plan-run effort: product decision, designed as optional S8 and gated (section 3, Q1). The Do loop keeps `thinking_budget_tokens: 0` (`RS/execution-core/src/runtime_loop/agent.rs:1332-1335`; live 1336-1339) until that is ruled.
- Tool rounds in the gateway loop (`tool_loop.rs` sets 0) stay at 0: effort applies to the final answer call only (audit card; unchanged).
- Gemini mapping (F-t3-041, W3) and model record v2 (F-t3-042, W3): WS11 provides the table they extend; it does not build them.
- Per-role cap source (WS10) and the live cost meter (WS27): WS11 provides seams only.
- Codex ladder beyond low/high: needs an Ingestion Plane change (S9, gated).

**FINISH-WHAT-IS-BUILT vs BUILD-NEW.**

| Slice | Tag | One line |
|---|---|---|
| S1 | [FINISH] | `effort.rs` ladder + table + legacy-budget mapping; cache-key fix |
| S2 | [FINISH] | OpenAI/Azure `reasoning_effort` (the audit's "smallest correct slice") |
| S3 | [FINISH] | Anthropic: budget vs adaptive+`output_config.effort`; fixes 4.7+ budget 400 |
| S4 | [FINISH] | Codex adapter through the ladder; keep low/high; fix the 1024 coupling |
| S5 | [BUILD] | Proto enum + `ModelInfo.effort_levels` + single gateway derivation (normal + ZDR) |
| S6 | [BUILD] | Composer levels from the catalogue; unify the five frontend type sites |
| S7 | [BUILD] | Policy seams: budget-posture clamp now, role cap hook for WS10 (gated) |
| S8 | [BUILD] | Do-run effort (gated on Q1) |
| S9 | [BUILD] | Codex broker accepts medium/xhigh (Ingestion Plane, gated on Q7) |

## 2. Verified current state

### 2.1 Wire path (opened)

| Hop | Evidence |
|---|---|
| UI | `FE/src/features/dashboard/home/DashboardComposer.tsx:152` `ResponseMode = auto\|quick\|deep`; chat-appearance select 3543-3554 (Rask/Standard/Grundig); wire mapping 3830-3834 (quick->`effort:"quick"`, deep->`"deep"`, auto omits). Live shifts to 154/3560/3838 (file M, effort lines unchanged). |
| Client types | `FE/src/shared/api/chat-client.ts:115` `effort?: 'quick'\|'standard'\|'deep'`; omit-if-standard 1096-1102. `FE/src/features/chat/components/chat-types.ts:520-521` narrower (`'quick'\|'deep'`); `FE/src/features/chat/lib/pending-chat-launch.ts:160-162` `normalizeEffort` quick/deep only. |
| AG-UI | `FE/src/shared/api/ag-ui-client.ts:266-280` copies `effort`; BFF `FE/apps/gateway/src/domains/ag_ui.rs:469` forwards any string, no allow-list. |
| Model Plane HTTP | `MG/http_routes.rs:6674-6679` `InvokeRequest.effort: Option<String>` (live 6780-6785). |
| Derivation (normal) | `MG/sse.rs:2539-2562`: `thinking::budget_tokens(effort, answer_token_budget())` -> `InferRequest.thinking_budget_tokens`. Live 2670-2690. |
| Derivation (ZDR) [live-only] | `C:/dev/CoresSystem/apps/Model Plane/rust/services/model-gateway/src/sse.rs` `ZdrTurnSettings.thinking_budget_tokens` 8349, `zdr_turn_settings` 8380-8410 (derives from the caller's lower `max_tokens`, not the ceiling), `direct_infer_turn` param 7777/set 7800; `http_routes.rs:8966, 8986`. Declared Honored in untracked `zdr_features.rs:120`; `reasoning` Inert under ZDR at 174-175. |
| Budget table | `MG/thinking.rs:16-59`: deep 4096, quick 1024, headroom 1024 (`:18,21,29`), step-down (`:44-51`), unknown -> 0 (`:40-41`). |
| Proto | `MP/proto/model_plane/v1/inference.proto:100-175` InferRequest, tags 1-16 used; `thinking_budget_tokens = 13` (155-162, "advisory"). 17 is free. `ModelInfo` 493-520 uses tags 1-7 (8 free). `InferChunk` last tag 15 (369), `InferResponse` 14 (306). |
| Internal request | `IC/provider/mod.rs:120-152` (`InferRequest`, derives Default; literals use `..Default::default()`), `IC/grpc.rs:1062-1114` maps the proto. |
| Adapters | OpenAI/Azure `IC/provider/openai.rs:431-449` (no reasoning field; `is_reasoning_model` 560-563 only switches `max_completion_tokens` vs `max_tokens`+`temperature`). Anthropic `IC/provider/anthropic.rs:402-470, 708-720`. Codex `IC/provider/codex_subscription.rs:114-145, 152-176`. |
| Broker (Ingestion) | `apps/Ingestion Plane/integration-corev2/internal/codexsubscription/manager.go:422-427` only low/high; `direct.go:497` writes `reasoning.effort`; `direct_request_test.go:276-302` pins it. |
| Catalogue to SPA | inference-core `ListModels` -> `MG/http_routes.rs:7452-7529` (`ModelDescriptor`, verbatim passthrough in BFF `FE/apps/gateway/src/domains/chat/json_handlers.rs:453-480`) -> `chat-client.ts:2443-2485` `listModels` -> `modelSupportsAgentic` pattern at `chat-client.ts:2571`. Feature strings are provider-level: `IC/provider/mod.rs:669-673` adds `reasoning` for every model of a `supports_thinking` provider. |

### 2.2 Files that exist only as uncommitted live-tree work [live-only]

| Live path | Why it matters to WS11 | Must land first |
|---|---|---|
| `RS/model-gateway/src/sse.rs` (M +883) | holds the second budget derivation (ZDR direct path) | Yes, before S5 (otherwise S5 edits a derivation that does not exist at HEAD) |
| `RS/model-gateway/src/http_routes.rs` (M +237) | `invoke` ZDR call site 8966/8986; `InvokeRequest` moves to 6780 | Yes, before S5 |
| `RS/model-gateway/src/zdr_features.rs` (??) | field treatment table; `effort` Honored; adding an `InvokeRequest` field stops the build there | Yes, before S5 (D2 avoids adding one) |
| `RS/execution-core/src/runtime_loop/agent.rs` (M +200/-7) | S8 only; owned by WS02/WS03/WS05 edits | Only if S8 is approved |
| `MP/proto/model_plane/v1/sessions.proto` (M +42) | unrelated; `buf breaking` outcome for live not determined | n/a |

Clean in live (safe to edit from HEAD): `thinking.rs`, `openai.rs`, `anthropic.rs`, `codex_subscription.rs`, `provider/mod.rs`, `fallback.rs`, `cache.rs`, `intent.rs`, `routing_policy.rs`, `inference.proto`, `execution.proto`, `manager.go`, `direct.go`, `ag_ui.rs`.

### 2.3 Defects and corrections (code wins over the audit)

- **D1. `deep` is `quick` on the default configuration.** `thinking::budget_tokens("deep", 4096)` returns 1024 (`thinking.rs:44-51`, test `thinking.rs:91-99`). The ceiling passed is `answer_token_budget()` (`sse.rs:2542-2545`), whose default is 4096 (`sse.rs:4485`; live 4616; clamp max 16 384). `MODEL_GATEWAY_ANSWER_TOKENS` is read only in `sse.rs`; grep of `MP/deploy` and `MP/docs` found no setting. Consequence on a default deploy: Anthropic gets a 1024 budget for Deep; Codex gets `low` effort plus priority tier for Deep (`codex_reasoning_effort(1024)` = low, `codex_service_tier` fires at exactly 1024, `codex_subscription.rs:155-176`). The Deep choice never reaches high anywhere unless an operator raised the ceiling. The audit does not mention this.
- **D2. Anthropic budget shape is probably a 400 on current models.** `anthropic.rs:418-426` excludes only pre-3.7 families; the test `current_and_future_models_are_allowed_to_think` pins `claude-opus-4-8` and `claude-opus-5` to `Some(2048)` (`anthropic.rs:1903-1915`). `claude-opus-4-8` is in the default Foundry catalogue (`IC/config.rs:313-320`). The `claude-api` reference loaded for this design (cached 2026-10-06) says `budget_tokens` is rejected with a 400 on Opus 4.7/4.8/5/5.5, Sonnet 5/5.5, Fable and Haiku 5.5, and deprecated on 4.6; those models take `thinking:{type:"adaptive"}` plus `output_config.effort`. Not live-verified on this Foundry resource; S1 includes a probe. If confirmed, Quick/Deep on `claude-opus-4-8` fails today.
- **D3. Cache key ignores effort.** `IC/cache.rs:67-97` hashes model, messages, temperature, max_tokens, tools, schema; not `thinking_budget_tokens`. The cache is consulted in `fallback.rs:1160` for `infer` with response cache (the streaming chat path says it consults none, `sse.rs:2573-2576`, but other callers do). A cached standard-effort answer can be served to a Deep request inside the 5-minute TTL.
- **D4. Reasoning is invisible on two routes.** The OpenAI adapter always emits `reasoning_delta: String::new()` (`openai.rs:848, 927, 954`); Chat Completions returns no reasoning text. Newer Claude models default to omitted thinking display (per the same reference), so `reasoning_delta` is empty unless `thinking.display:"summarized"` is sent. The SSE `reasoning` family is advertised for every model of these providers (`provider/mod.rs:671`), including `claude-3-5-haiku-20241022` in the Direct list (`anthropic.rs:947-959`), which cannot think.
- **D5. Three derivations and one hard-coded producer.** Normal path (`sse.rs:2542`), ZDR path [live] (`max_tokens`-based), `source_validation.rs:2274-2277` (4096 meaning "high" on subscription; asserts at `source_validation.rs` tests per sweep). A fourth consumer, the confidence score, compares `output_tokens` (which include reasoning) with the answer ceiling (`sse.rs:3297, 6597`; `confidence.rs:299-330`): a Deep turn that reasons then answers inside the ceiling can score as truncated.
- **Audit staleness.** `agent.rs:1336-1339` is correct only in the live tree; worktree is 1332-1335. "Anthropic thinking budget" is true only for models where it is accepted (D2). The Codex route does honor effort (low/high), so "ignores the user's choice" holds for OpenAI/Azure only. There is no existing `effort` or `reasoning` golden in `MP/api/contracts/` and none in `docs/actions-surface-operations.md`.

### 2.4 Strengths and patterns reused

- Precedent for a per-model predicate: `provider/logprobs.rs` `model_supports_logprobs` (reasoning models rejected; test 437). Precedent for source-text cross-service contract: `RS/execution-core/tests/cross_service_loop_contract.rs`. Precedent for proto-less catalogue extension: `grpc.rs:55-73` surfaced `cheap` as a feature string. Flag pattern: section 8.1 of `.plan-work/common/flags-and-config.md` (constant, pure parser, vocabulary C default OFF for new risky behaviour, compose `${VAR:-false}`).

## 3. Decisions and open questions

| # | Question | Options | Recommendation | Gates |
|---|---|---|---|---|
| Q1 | Extend effort to Do/Plan runs? (audit 10.2 #15; product decision) | (a) keep 0 (deployed agent budget is its round budget, `agent.rs:1332-1335`); (b) apply the chosen level to the final/synthesis round only; (c) apply to every round | (a) for W1. The seam is cheap (S8). Revisit when WS03 durable loop and WS27 cost meter exist, because (c) multiplies cost per round. Replay risk is low today: the Anthropic adapter replays text-only history (`anthropic.rs:690-706`), so no thinking-block round trip is needed, but verify on S8. | S8 only. S1-S7 and S9 proceed without it. |
| Q2 | Wire vocabulary | (a) keep one `effort` string, accept profiles (`quick/standard/deep`, aliases) AND canonical names (`minimal..max`); (b) new `effort_level` request field | (a). No new `InvokeRequest` field, so no edit to `zdr_features.rs` `FIELD_TREATMENTS`, BFF `ag_ui.rs:469` or `ag-ui-client.ts`. Unknown values: warn + treat as standard, with a counter (never 400, keeps old clients working). | S5, S6 |
| Q3 | Ceiling policy: does Deep mean Deep? | (a) keep step-down (D1 stays); (b) raise the provider ceiling by a per-level allowance so thinking does not eat the answer budget | (b). Anthropic: `max_tokens = answer + budget`; OpenAI: `max_completion_tokens = answer + allowance` (reasoning counts inside it; the title code already hit this, `sse.rs:7582-7640` comments). Providers bill used tokens; the cost bound is the allowance (Low 1024, Medium 2048, High 4096, XHigh 8192, Max 16384; total capped at 32 768 pending per-model verification). Needs owner OK on cost. | S2, S3, S5 (behaviour change for effort turns only) |
| Q4 | What does the composer show? | (a) 3 profiles only; (b) 3 profiles plus a fourth "Maksimal" only when the selected model lists `xhigh`; (c) raw six-level picker | (b). Verevon modes (budget/balance/genius) resolve the model server-side, so the 3 profiles always exist; the extra option appears from `effort_levels` only. No raw six-level picker. | S6 |
| Q5 | Meaning of "standard" | omit the key (provider default) vs explicit `medium` | Keep omit (byte-identical default, `thinking.rs:8-14`). Codex standard stays `low` for latency (`codex_subscription.rs:152-154`). If OD19 "reasoning default" (CHATGAP L507) rules otherwise, change one table cell. | none |
| Q6 | Clamp rule when no weaker level exists (e.g. `gpt-5-pro` high-only) | omit vs send weakest supported | Weaker-else-weakest-supported-else omit; log `clamped=true`. | S1 |
| Q7 | Widen Codex broker to medium/xhigh (Ingestion Plane `manager.go:426`) | do now vs defer | Defer until product wants it; adapter clamps into low/high meanwhile. | S9 only |
| Q8 | Per-role/org effort cap and budget-posture default | Control-owned entitlement (WS10) vs Model Plane policy | Seam now (`clamp_to_cap`), cap source from WS10. Budget posture: clamp to Low only when the intent layer already forces the cheap fallback. | S7 |
| Q9 | Anthropic 4.6 class (budget still works, deprecated) | stay budget vs move to adaptive | Stay on budget for `claude-sonnet-4-6`/`claude-opus-4-6` until the S1 probe says otherwise; 4.7+ and unknown future `claude-*` go adaptive. This reverses the existing "unknown models get budget_tokens" test intent explicitly (S3 updates `anthropic.rs:1903-1915`). | S3 |

**Ledger.** No settled/rejected entry is reversed. History: `MP/docs/decisions/ledger.md:2385-2387` (dial had zero writers), `:2600-2606` (C2 wired), `:2895-2899` (budget now wired). Add one entry in S1: "Canonical effort ladder lives in inference-core; gateway sends a level, adapters render" (status proposed; flips at deploy). If Q1(b)/(c) is chosen, add a second entry that explicitly supersedes the in-code decision at `agent.rs:1332-1335` (that decision exists only as a code comment, not in the ledger).

**Can proceed without any decision:** S1, S2 (with Q3 default recommended), S3, S4, S5, S6 (with Q4 default), S9 design. Only S8 is blocked (Q1); S7 needs Q8; S9 needs Q7.

## 4. Slices (PR-sized, in build order)

Build order: S1 -> {S2, S3, S4 in parallel} -> S5 (needs live ZDR files landed) -> S6 -> S7 -> S8/S9 (gated). Probe task P0 (0.5 d) runs first inside S1: send one request each to (i) a gpt-5-family deployment with `reasoning_effort`, (ii) `claude-opus-4-8` on Foundry with `thinking:{type:"adaptive",display:"summarized"}` + `output_config.effort`, (iii) the same with legacy `budget_tokens` (to confirm D2). Record outputs in the PR; they fix the table `verified_on` and the S3 flag default.

---

### S1 - Canonical ladder module, legacy mapping, cache key [FINISH]

**Goal.** One pure, table-tested place that answers "given model + requested level + ceiling, what do we send?". No behaviour change on the wire.

**Files.**
- `IC/provider/effort.rs` - NEW. `enum Level {Minimal, Low, Medium, High, XHigh, Max}` (ordered, `parse`/`as_str`); `enum Wire {None, AnthropicBudget, AnthropicAdaptive, OpenAiReasoningEffort, CodexEffort}`; `struct Support {wire, levels: &'static [Level], verified_on}`; `fn support(family, model) -> Support` (Anthropic: `NoThinking` for the existing exclusion markers `anthropic.rs:418-426`, `Budget` for claude-3-7/-4/-4-1/-4-5/sonnet-4-5/haiku-4-5/4-6, `AnthropicAdaptive` for opus-4-7+/5*/sonnet-5*/haiku-5-5/fable/mythos and any unmatched `claude-*`; OpenAI: gpt-5/-mini/-nano `[Minimal..High]`, o1/o3/o4-mini `[Low..High]`, gpt-5-pro `[High]`, gpt-5.1+ and others per probe, non-reasoning ids `None`; Codex `[Low, High]`); `fn resolve(requested: Option<Level>, legacy_budget: i32, support, max_tokens) -> Applied {level, clamped, wire, budget_tokens, allowance}`; `fn from_legacy_budget(i32) -> Option<Level>` (0 -> None, <=1024 Low, <=2048 Medium, else High, so `source_validation`'s 4096 stays "high"); `fn allowance(Level) -> i32`. Level names for OpenAI/Azure aliases via env `INFERENCE_CORE_EFFORT_MODEL_ALIASES` (csv `deployment=model`, because Azure deployment ids are operator-chosen, `IC/config.rs:303-306`).
- `IC/provider/mod.rs` - `pub mod effort;`; `InferRequest` (120-152) += `reasoning_effort: Option<effort::Level>` (Default None); `ModelInfo` (349-369) += `effort_levels: Vec<String>`; doc comment on `thinking_budget_tokens` updated (legacy input, mapped by `effort`).
- `IC/cache.rs` - `cache_key` (67-97) hashes `reasoning_effort` and `thinking_budget_tokens`; fixes D3.
- `MP/docs/decisions/ledger.md` - new proposed entry (section 3).

**Contracts/data.** None on the wire. No migration. Proto untouched in S1.

**Flag.** None (inert; no adapter calls it yet).

**ZDR.** Nothing persisted; pure functions. No golden-matrix case (the matrix pins retention decisions, `MP/api/contracts/README.md`, not request parameters).

**Tests.** `cargo test -q -p inference-core --lib effort` (Windows host: `--lib` only, see memory `model-plane-build-and-deploy`). New inline tests in `effort.rs`: `ladder_is_ordered`, `clamp_picks_nearest_weaker`, `no_weaker_level_picks_weakest_supported`, `unsupported_model_resolves_to_none`, `legacy_budget_maps_1024_to_low_and_4096_to_high`, `every_configured_default_model_has_a_declared_support` (iterates the lists in `config.rs:298, 313-320` and `routing_policy.rs:136-160`, so a new default model without a table row fails), `allowance_is_monotonic`. New in `cache.rs` tests next to 331-334: `a_different_effort_misses_the_cache`.

**DoD.** Module compiles, all adapters unchanged, cache test green, ledger entry merged, probe results recorded in the PR.

**Evidence.** Rebuild not required (no behaviour change); evidence is the probe transcript (request body, response status, reasoning-token usage) for P0 (i)-(iii).

**Effort.** S-M, 2.5-3.5 d. **Deps.** none. **Risks.** table values drift; mitigated by `verified_on` and the `every_configured_default_model...` test.

---

### S2 - OpenAI/Azure `reasoning_effort` [FINISH]

**Goal.** The route that serves Budget/Balance defaults (gpt-5-nano/gpt-4o-mini/gpt-5-mini, `routing_policy.rs:143-159`) honors the user's depth.

**Files.**
- `IC/provider/openai.rs` - `build_request_body` (431-449): when the flag is on and `effort::resolve(...)` returns a level for `Wire::OpenAiReasoningEffort`, set `body["reasoning_effort"]`; raise `max_completion_tokens` by `allowance` (Q3) up to the cap; keep `is_reasoning_model` (560-563) for the token-parameter switch but source it from `effort::support` so `o4*` gets `max_completion_tokens` too (today `o4` would send `max_tokens`+`temperature`; the logprobs predicate already includes `o4`); no change when level is None, so the default body is byte-identical. `capabilities()` (641-656) and `list_models` (1063-1085) fill `effort_levels` per model; fix `supports_thinking` semantics only through `effort_levels` (the `reasoning` feature string stays, see D4 note in S5). Debug log `effort_resolved{request_id, model, wire, requested, applied, clamped}` (never prompt text).
- On a 400 whose message names `reasoning_effort`, one bounded retry without it, counted and logged (consistent with "degrade, never 400", `anthropic.rs:430-435`), same flag.
- `MP/deploy/docker-compose.yml` - inference-core service (808): `INFERENCE_CORE_OPENAI_REASONING_EFFORT: ${INFERENCE_CORE_OPENAI_REASONING_EFFORT:-false}` with a comment on cost; `INFERENCE_CORE_EFFORT_MODEL_ALIASES: ${...:-}`.

**Flag.** `INFERENCE_CORE_OPENAI_REASONING_EFFORT`, default OFF (vocabulary C, pure parser `parse_enabled(Option<&str>)` per flags digest 8.1). Read at point of use; a change needs a restart. Rollout: local -> staging deployment -> on. Rollback: unset and restart; the legacy path (no key) returns.

**ZDR.** Request parameter only; same adapters serve ZDR deployments (Azure). Nothing persisted; the `reasoning_effort` value is not logged with content. No matrix row. The Azure `api-version` default `2025-01-01-preview` (`IC/config.rs:296-297`, compose 908) may not accept the parameter on every deployment: not determined; P0 decides, and the retry above is the safety net.

**Tests.** `cargo test -q -p inference-core --lib openai`. New inline in `openai.rs` tests: `reasoning_effort_sent_for_gpt5_family_only`, `quick_on_gpt5_mini_clamps_and_sends_low`, `standard_body_is_byte_identical_to_before` (golden JSON equality with `reasoning_effort: None`), `non_reasoning_model_never_gets_the_key`, `azure_deployment_alias_resolves_to_its_base_model`, `o4_uses_max_completion_tokens`, `allowance_raises_max_completion_tokens_not_max_tokens`.

**DoD.** Flag-on request for gpt-5-mini with `quick` vs `deep` shows strictly more reasoning tokens for Deep; flag-off bodies unchanged.

**Evidence.** From the main checkout after merge: `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build inference-core` (3-4 min). Flow: authenticated invoke on a gpt-5-mini route with `effort:"quick"` then `"deep"` and the same prompt (through the SPA at :5173 or `POST /v1/invoke/stream`). Capture: `effort_resolved` log lines; provider `usage.completion_tokens_details.reasoning_tokens` for both (read via a temporary debug log of the parsed usage object, removed after); one negative probe on `gpt-4o-mini` showing no key sent.

**Effort.** M, 3-4 d. **Deps.** S1, Q3. **Risks.** Azure api-version rejection; `max_completion_tokens` allowance cost; gpt-5 variants differing in accepted values (table + retry).

---

### S3 - Anthropic wire classes [FINISH]

**Goal.** Send the shape each Claude generation accepts; fix D2; make reasoning visible.

**Files.**
- `IC/provider/anthropic.rs` - replace `resolve_thinking_budget` (436-470) and the body block (713-720) with `effort::resolve`: `Budget` class keeps `thinking:{type:"enabled",budget_tokens}` (floor 1024, `:404`) with `max_tokens = answer + budget`; `AnthropicAdaptive` class sends `thinking:{type:"adaptive",display:"summarized"}` and `output_config:{effort: low|medium|high|xhigh|max}`; `NoThinking` omits; standard (None) omits everything as today. Fill `effort_levels` in `list_models` (947-996). Keep `split_content_block_delta` (thinking deltas never become answer text, test 1948).
- Update tests in `anthropic.rs:1872-1943`: `current_and_future_models_are_allowed_to_think` now asserts the adaptive shape for `claude-opus-4-8`/`claude-opus-5` and the budget shape for `claude-sonnet-4-5` (explicit reversal, Q9); `a_budget_below_the_provider_floor...` stays for the Budget class.
- compose inference-core: `INFERENCE_CORE_ANTHROPIC_ADAPTIVE_THINKING`.

**Flag.** `INFERENCE_CORE_ANTHROPIC_ADAPTIVE_THINKING`. Default decided by P0(iii): if the legacy shape 400s on `claude-opus-4-8`, ship default ON as a kill switch (vocabulary A, because it fixes a failing turn); otherwise default OFF (vocabulary C). Rollback: flag flip restores the legacy body.

**ZDR.** Parameter-only. Summarized thinking is streamed ephemerally and never appended to `assistant_output` (`sse.rs:3067-3071`); ZDR turns stream no reasoning text anyway (`zdr_features.rs:174-175`).

**Tests.** `cargo test -q -p inference-core --lib anthropic`. New: `adaptive_models_send_adaptive_plus_effort_not_budget`, `claude_unknown_future_model_defaults_to_adaptive`, `budget_class_unchanged_for_sonnet_4_5`, `standard_omits_thinking_and_output_config` (golden), `summarized_display_is_requested`, `max_tokens_includes_the_budget`, `foundry_and_direct_share_the_shape` (both flavors, `anthropic.rs:39-52`).

**DoD.** Quick and Deep on `claude-opus-4-8` succeed with visible reasoning; Sonnet 4.5 path unchanged.

**Evidence.** Rebuild inference-core (as S2). Flow: Deep on `claude-opus-4-8` via Foundry and via a Budget-class model; capture request body log (keys only), HTTP status, `reasoning` SSE events count, `usage.output_tokens`. If Foundry rejects `output_config`, record it and fall back to `thinking`-only for that flavor (not determined until probed).

**Effort.** M, 3-5 d. **Deps.** S1, P0. **Risks.** Foundry may not mirror first-party parameters; pinned legacy tests must be changed deliberately.

---

### S4 - Codex adapter through the ladder [FINISH]

**Goal.** Same resolve step; remove the hidden 1024 coupling; behaviour unchanged by default.

**Files.**
- `IC/provider/codex_subscription.rs` - `request_body` (142-143): `reasoning_effort` from `effort::resolve` with `Wire::CodexEffort`, levels `[Low, High]` (Medium -> Low, XHigh/Max -> High: nearest weaker); `codex_service_tier` (166-176) keys on "resolved level is Low from an explicit profile" instead of `thinking_budget_tokens == 1_024` (legacy 1024 maps to Low, so behaviour is identical via the legacy path). Standard (None) still sends `low` (latency, `:152-154`).
- Update tests 1004-1007 (`maps_gateway_effort_to_codex_reasoning_effort`), 1021, 1039-1068.

**Flag.** None needed: legacy budgets map to the same outputs; a unit table proves it. (Behaviour for `source_validation`'s 4096 -> High is unchanged.)

**ZDR.** Codex runs only on the waived/subscription route; effort is a broker parameter. Broker already sends `store:false` and `reasoning.encrypted_content` (`direct.go:497-500`), unchanged.

**Tests.** `cargo test -q -p inference-core --lib codex_subscription`. New: `every_level_clamps_into_low_or_high`, `legacy_budgets_map_identically` (0, 1024, 2048, 4096), `quick_keeps_priority_tier_only_on_the_five_models`, `standard_is_low_without_priority`.

**DoD.** Existing Codex body tests pass unchanged except for renamed helpers.

**Evidence.** Rebuild inference-core; if a Codex connection exists, one Quick and one Deep turn show broker `reasoningEffort` low/high in the broker request log; otherwise mark "not proven live" (connection not available is a gap to state, not hide).

**Effort.** S, 1-2 d. **Deps.** S1. **Risks.** tier semantics; covered by `legacy_budgets_map_identically`.

---

### S5 - Proto level, catalogue field, single gateway derivation [BUILD]

**Goal.** The gateway sends a level (not a model-blind budget); inference-core resolves per attempt model (fallback may change model/provider, `fallback.rs:1193-1207`); the catalogue tells the SPA what each model supports.

**Proto (additive, `buf breaking` safe).** `MP/proto/model_plane/v1/inference.proto`:
- `enum ReasoningEffort { REASONING_EFFORT_UNSPECIFIED = 0; REASONING_EFFORT_MINIMAL = 1; _LOW = 2; _MEDIUM = 3; _HIGH = 4; _XHIGH = 5; _MAX = 6; }`
- `InferRequest.reasoning_effort = 17` (verified free; WS02 D8 reserves 17 for WS11, 18 and 19 for WS02). Doc: UNSPECIFIED = legacy/none; field 13 stays honored as legacy input (mapped by `from_legacy_budget`); level wins when both are set.
- `ModelInfo.repeated string effort_levels = 8` (verified free).
- Applied-effort reporting back on `InferChunk`/`InferResponse` is NOT in W1 (tags 16/15 free but invasive literals); the debug log from S2-S4 is the evidence; WS27 may take it with the cost meter.

**Files.**
- `IC/grpc.rs` - `to_internal_request` (1087-1114): map proto enum to `Option<Level>` (unknown numeric -> `invalid_argument`, same precedent as `tier_from_wire`); `From<provider::ModelInfo>` (55-73): copy `effort_levels`.
- `IC/provider/fallback.rs` - none required (the request is cloned per attempt; adapters resolve per model). Note `resolve_intent` (1149) returns a modified clone: S7 clamps there.
- `MG/thinking.rs` - add `pub fn effort_level(profile) -> ReasoningEffort` (profiles `quick`->Low, `standard`/unset->Unspecified, `deep`->High; aliases `brief/extended/thorough` kept; canonical names accepted; unknown -> Unspecified + `tracing::warn` with a counter) and `pub struct EffortPlan {level, legacy_budget}` with `pub fn plan(profile, ceiling, ladder_on) -> EffortPlan` used by BOTH call sites. Existing `budget_tokens` stays for the flag-off path and the unit tests at `thinking.rs:61-117`. `fn reasoning_allowance(level)` mirrors the inference-core table (cross-service constant; contract test below).
- `MG/sse.rs` - 2539-2564: use `thinking::plan`, set `reasoning_effort` (and legacy budget only when flag off); `truncation_budget` call sites (3297, 6597) add `reasoning_allowance(requested)` so reasoning tokens do not trip the truncation penalty (D5). [live-only] `zdr_turn_settings` (8380-8410), `ZdrTurnSettings` (8336-8352), `direct_infer_turn` (7768-7800) and its caller (8440-8448): replace `thinking_budget_tokens` with the plan; `http_routes.rs:8986` likewise. These edits must follow the landing of the live files.
- `MG/http_routes.rs` - `ModelDescriptor` (7452-7465) += `effort_levels: Vec<String>`; `list_models` (7514-7526) copies it; doc comment on `InvokeRequest.effort` (6674-6679) lists accepted names.
- `MG/tool_loop.rs`, `MG/grpc.rs` `build_infer_request` (425-450), `MG/source_validation.rs` (2274-2277): no behaviour change; confirm tool rounds stay Unspecified; migrate source review to `reasoning_effort = High` (keep its tests green).
- `MG/tests/effort_ladder_contract.rs` - NEW source-text contract (pattern: `RS/execution-core/tests/cross_service_loop_contract.rs`): asserts the allowance numbers in `thinking.rs` equal those in `IC/provider/effort.rs`, and that every ladder name the gateway accepts exists in the proto enum. Reads source text, no build dependency.
- compose model-gateway (service at line 191): `MODEL_GATEWAY_EFFORT_LADDER: ${MODEL_GATEWAY_EFFORT_LADDER:-false}`.
- `MP/docs/sse-event-taxonomy.md`: no change (no new event, generator-owned).

**Flag.** `MODEL_GATEWAY_EFFORT_LADDER`, default OFF. Off: gateway sends the legacy budget exactly as today. On: sends the level and budget 0. Rollout order: deploy inference-core (S1-S4, flags off) -> deploy gateway with the flag off -> enable inference-core flags -> enable the gateway flag. Rollback: flag off, no restart of inference-core needed.

**ZDR.** The effort value is already declared Honored under ZDR (`zdr_features.rs:120`); the ZDR path now carries the level through `direct_infer_turn`. Under ZDR: nothing persisted; no reasoning text streamed (`zdr_features.rs:174-175`); the thinking summary is never written to session-core (`sse.rs:3067-3071`). No new `InvokeRequest` field, so `FIELD_TREATMENTS` is untouched (Q2). Golden: no matrix row; add Rust test `zdr_turn_honors_the_effort_level` in `sse.rs` tests and keep `zdr_features` table test green. Live ZDR proof is blocked until an attested provider exists (audit F-t3-005, WS01); state "test-proven only" until then.

**Tests.** `cargo test -q -p model-gateway --lib thinking`, `--lib zdr`, `-p inference-core --lib grpc`, `-p model-gateway --test effort_ladder_contract`. New: `profile_maps_to_level`, `canonical_names_pass_through`, `unknown_effort_is_standard_and_counted`, `flag_off_sends_legacy_budget_only`, `flag_on_sends_level_and_zero_budget`, `normal_and_zdr_paths_derive_the_same_level`, `truncation_budget_includes_the_allowance`, `list_models_carries_effort_levels`, inference-core `proto_level_roundtrips_to_internal`, `unknown_enum_value_is_invalid_argument`. Proto: `buf lint`/`buf breaking` per `.github/workflows/buf.yml` (known pre-existing debt: paths with spaces unquoted, so do not cite it as passing); Go bindings under `MP/go/gen/` go stale but wire-compatible (buf not installed on host).

**DoD.** Flag on: Deep reaches High on gpt-5-mini, `claude-opus-4-8` and Codex; fallback from one provider to another re-resolves; ZDR path parity test green.

**Evidence.** Rebuild both services (`compose.sh up -d --no-deps --build inference-core model-gateway`). Flow: via the SPA (second dev server per memory `verevonv3-worktree-verification`, or :5173 after merge) pick Quick/Deep on three routes; capture `/v1/models` JSON showing `effort_levels` per model, gateway log of the plan, inference-core `effort_resolved` lines, and one forced fallback (pin an unavailable first provider) showing re-resolution on the second model.

**Effort.** L, 4-6 d. **Deps.** S1-S4, live ZDR files landed, Q2/Q3. **Risks.** hot files (`sse.rs`, `http_routes.rs`); proto tag contention (D8 in WS02); flag-on without inference-core S1-S4 deployed (rollout order above).

---

### S6 - Composer levels from the catalogue [BUILD]

**Goal.** The control follows the server; one effort type across the SPA.

**Files (all verified).**
- `FE/src/shared/api/chat-client.ts` - export `type EffortProfile = 'quick'|'standard'|'deep'`, `type EffortLevel = 'minimal'|'low'|'medium'|'high'|'xhigh'|'max'`; `ChatInvokeRequest.effort` (115) widened; `ModelInfo` (849-869) += `effortLevels?: EffortLevel[]`; `listModels` (2443-2485) parses `effort_levels`; `modelEffortLevels(model)` helper beside `modelSupportsAgentic` (2571); omit rule at 1096-1102 unchanged.
- `FE/src/features/chat/components/chat-types.ts:520-521` and `FE/src/features/chat/lib/pending-chat-launch.ts:160-162` - use the shared type; `normalizeEffort` accepts the full set.
- `FE/src/features/chat/components/use-chat-controller.ts` (1904, 2551, 2890) - types only.
- `FE/src/features/dashboard/home/DashboardComposer.tsx` - `ResponseMode` (152), select (3543-3554), mapping (3830-3834), `handleResponseMode` (2280): when the selected catalogue model lists `xhigh`, add "Maksimal/Maximum"; options never exceed what `effort_levels` allows; Verevon modes keep the 3 profiles. No raw six-level picker (Q4).
- `FE/src/shared/api/ag-ui-client.ts:266-280` and BFF `ag_ui.rs:469`: no change (single `effort` string, Q2).
- Action contract: none. Effort is a request option like `tone`, not an operation; `src/shared/actions/` has no effort symbol and `docs/actions-surface-operations.md` has no effort mention (both greps clean).

**Flag.** None; the extra option appears only when the server lists levels (server-controlled behaviour from the catalogue, per flags digest 8.4).

**ZDR.** Nothing stored; `pending-chat-launch.ts` already persists the launch effort, so no new content class.

**Tests.** `cd "apps/Frontend Plane/verevonv3" && pnpm vitest run src/shared/api/chat-client.test.ts src/features/chat/lib/pending-chat-launch.test.ts src/shared/api/ag-ui-client.test.ts`; `pnpm typecheck` (diff failing set against HEAD, pre-existing failures). Extend `chat-client.test.ts` effort block (describe ~1281; live 1303): `parses_effort_levels_from_models`, `sends_a_canonical_level_when_chosen`, `still_omits_effort_for_standard`. New `FE/src/features/dashboard/home/DashboardComposer.effort.test.tsx` (pattern: `DashboardComposer.privacy-tier.test.tsx`): `offers_three_profiles_by_default`, `offers_maksimal_only_when_the_model_lists_xhigh`, `launch_effort_survives_roundtrip`.

**DoD.** No type drift across the five sites; the select reflects `/v1/models`.

**Evidence.** Second Vite dev server from the worktree (`pnpm dev --port 5174 --strictPort`; :5173 is the main checkout); screenshot the select on a model with and without `xhigh`; capture the invoke body.

**Effort.** M, 3-4 d. **Deps.** S5. **Risks.** i18n copy (Norwegian first); stale `global.css` caching after restart (memory `verevonv3-vite-needs-restart`).

---

### S7 - Policy seams [BUILD, gated on Q8]

**Goal.** Effort cost is governed without inventing an entitlement store.

**Files.**
- `IC/provider/intent.rs` (`choose` 201, `resolve` 236) and `fallback.rs` `resolve_intent` (1149): when the intent layer forces the cheap fallback (tests `exhausted_budget_forces_cheap_fallback` 851, `constrained_budget_downgrades_one_tier` 872), clamp `reasoning_effort` to at most Low on the returned clone.
- `IC/provider/effort.rs` - `pub fn clamp_to_cap(level, cap: Option<Level>) -> Option<Level>`; the cap source is WS10's decision (claim or entitlement); until then `None`.
- `MP/docs/capability-ownership-matrix.md`: one line stating the per-role effort cap is Control-owned, consumed here.

**Flag.** `INFERENCE_CORE_EFFORT_BUDGET_CLAMP`, default OFF (vocabulary C).

**ZDR.** None. **Tests.** `cargo test -q -p inference-core --lib intent`: `exhausted_budget_clamps_effort_to_low`, `pinned_model_is_not_clamped`, `cap_clamps_down_never_up`. **DoD.** Clamp visible in `effort_resolved` log. **Evidence.** rebuild inference-core; drive an org to exhausted budget (cost-core fixture) and show Deep resolved to Low. **Effort.** S-M, 2-3 d. **Deps.** S5, WS10 for the cap source. **Risks.** surprising users: the clamp must be visible (WS27/WS15 surface).

---

### S8 - Do/Plan-run effort [BUILD, gated on Q1]

**Goal (only if Q1 is (b) or (c)).** Carry the level into execution-core.

**Files.** `MP/proto/model_plane/v1/execution.proto` `RunAgentRequest` (76-209; highest tag 17 at 208; 18 is contested by WS02/WS03, so `ReasoningEffort reasoning_effort = <integrator>` at 19 or later); gateway run-start builder `MG/sse.rs:644-707` (`build`/`RunAgentRequest` literals; not read in full); `RS/execution-core/src/runtime_loop/agent.rs` `run_rounds` (1332-1335; live 1336-1339): replace the literal `0` with the run's level for the chosen rounds (b: final/synthesis round only); persist nothing beyond the run request. Ledger entry superseding the code comment. The two tool loops stay separate (this touches only the inference request, not dispatch).

**Flag.** `EXECUTION_CORE_RUN_EFFORT`, default OFF. **ZDR.** Runs under ZDR refuse as today; no change. **Tests.** `cargo test -q -p execution-core --lib`: `run_rounds_passes_the_run_level`, `default_run_is_unchanged`; no existing test pins thinking in execution-core (grep clean), so add one. **Evidence.** rebuild execution-core + gateway; Do run with Deep, capture per-round `effort_resolved` and cost delta. **Effort.** M, 4-6 d. **Deps.** S5; WS02/WS03 land `agent.rs`/proto first; WS27 for cost visibility. **Risks.** cost multiplication per round; thinking-block replay for Anthropic tool loops (text-only replay today, verify).

---

### S9 - Codex broker medium/xhigh [BUILD, cross-plane, gated on Q7]

**Files.** `apps/Ingestion Plane/integration-corev2/internal/codexsubscription/manager.go:422-427` (accept `medium`, `xhigh`), `direct.go:497` and the app-server transport (`appserver.go`, thread/turn start params) already pass the string (not re-read here); update `direct_request_test.go:276-302` (`TestDirectEffortMapping`) and the manager test `TestManagerRejectsUnsupportedReasoningEffort` (`manager_test.go`). Model Plane `effort.rs` Codex row becomes `[Low, Medium, High, XHigh]`. **Tests.** `cd "apps/Ingestion Plane/integration-corev2" && go test ./internal/codexsubscription/... ./internal/api/...` and `go vet` on the same. **ZDR/flag.** none new; deploy broker before the Model Plane table change. **Effort.** S, 1-2 d. **Risks.** Codex per-model ladders differ (not determined); do not touch `integration-corev2/.env` tokens (memory `integration-core-env-drift`).

## 5. Interfaces

**PROVIDES**
- To WS10 (model entitlements): `effort::support(family, model).levels` and `ModelInfo.effort_levels` (per-model catalogue facts); `clamp_to_cap(level, cap)`.
- To F-t3-042/F-t3-041 (W3): the per-model support table as the seed of "effort ladder" in model record v2, and the Gemini thinking-level wire as a fourth `Wire` variant.
- To WS02 (run context and cache): `InferRequest.reasoning_effort = 17` is part of the cache identity (Anthropic invalidates the cached message prefix when thinking parameters change; per the claude-api reference), so a mid-thread effort change should be treated as a cache-scope change; S1 already puts effort in the inference-core response-cache key.
- To WS27 (cost meter): `effort::allowance(level)` as the pre-run upper bound on extra reasoning spend; `effort_resolved` log fields; applied-effort reporting left open for it.
- To WS03/WS24: nothing in W1; S8 reserves the Do seam.

**CONSUMES**
- WS02 D8 proto allocation: `InferRequest` 17 to WS11, 18-19 to WS02; `RunAgentRequest` 18 contested between WS02 and WS03, S8 takes 19+.
- WS01: an attested ZDR provider, to move ZDR evidence from test-proven to live-proven.
- WS10: org/role cap value.
- WS12: frontend CI gates (typecheck/lint ratchet) for S6.
- WS03/WS02/WS05: land their `agent.rs` signature changes before S8.

**Conflict hot spots and edit-order rule**

| File | Also touched by | Rule |
|---|---|---|
| `MG/sse.rs`, `MG/http_routes.rs`, `MG/zdr_features.rs` | WS02, WS03, WS15, WS27 | Land the live uncommitted set first; S5 makes hunk-minimal edits (derivation sites, descriptor), rebases last |
| `IC/provider/mod.rs`, `IC/grpc.rs` | WS02 (`prompt_tiers`, `cache_scope_key`), WS10 | S1 edits the struct first (add-only field); others rebase |
| `inference.proto` | WS02 (18, 19), WS27 | Integrator reserves tags; S5 uses 17 and ModelInfo 8 |
| `anthropic.rs`, `openai.rs`, `codex_subscription.rs` | WS02 (cache markers), F-t3-041 later | S2-S4 are independent files; WS02 rebases on them |
| `execution.proto`, `agent.rs` | WS02, WS03, WS05, WS24 | S8 last, after the others |
| `chat-client.ts`, `DashboardComposer.tsx` | WS08, WS09, WS18, WS27 | S6 touches only the effort/ModelInfo hunks |

## 6. Cross-session interfaces

- **Sibling Verevon Space session (a).** The Space room composer (`FE/src/features/spaces/components/SpaceRoomComposer.tsx`) has an "effort" text hit I did not inspect; its relation to chat effort is not determined. Rule: if Space offers an effort control it must consume `modelEffortLevels`/`EffortLevel` from `chat-client.ts` (S6) and must not add its own enum. I assume (a) neither renames `ModelInfo` nor adds a second model catalogue endpoint.
- **Quarry v2 / browser session (b).** No shared file. Browser planning is a Do-type loop in execution-core (`llm_planner`, `browser_agent`); effort never reaches Quarry, and "Model proposes, Quarry executes" is untouched. If (b) adds a planner round that wants reasoning, it goes through S8, not a private budget.
- **Ingestion Plane (not Quarry).** S9 is the only cross-plane change: owner is the integration-corev2 maintainers; deploy order is broker first. No Control or Data Plane contract changes.

## 7. Regression guards

| Strength (audit section 8) | How WS11 touches it | Guard |
|---|---|---|
| ZDR by construction | second derivation lives in the ZDR path; effort is Honored, reasoning text Inert | `zdr_features.rs` table test (no `..` in `invoke_field_names`), `zdr_turn_honors_the_effort_level`, `bash scripts/tests/zdr-contract-sync.test.sh` unchanged and green; no new store, so no matrix row |
| Typed ChatEvent taxonomy | `reasoning` stream gets more content, no new event | `.github/workflows/sse-taxonomy.yml` generator unchanged; SSE `reasoning` family still opt-in |
| Intent-based routing with provenance | effort must not change routing; resolved per attempt model | `intent.rs` tests 773-917 unchanged; new `pinned_model_is_not_clamped`; S5 fallback re-resolution test |
| Two loops separate | S8 touches only the inference request in execution-core | `autonomy_ladder_contract.rs`, `cross_service_loop_contract.rs` unchanged; no dispatch edits |
| Honest outcomes | truncation/confidence counts reasoning tokens; clamped level is logged, not hidden | `truncation_budget_includes_the_allowance`; `effort_resolved` clamped flag |
| Multi-tenant, proto-first | additive proto only | `buf breaking` (additive fields/enum), `baseline.binpb` not regenerated; `cargo test -p mp-contracts` |
| Default path unchanged | flags off and `standard` produce identical bodies | golden-body tests in S2/S3; `thinking.rs` tests 61-117 retained |

## 8. Effort and risks

**Total.** S1-S6: 17-24 engineer-days (S1 2.5-3.5, S2 3-4, S3 3-5, S4 1-2, S5 4-6, S6 3-4). Gated: S7 2-3, S8 4-6, S9 1-2. **Critical path:** P0 probes -> S1 -> S3 (largest unknown) -> S5 (needs the live ZDR files landed) -> S6. S2 and S4 run in parallel with S3.

**Top risks.**
1. **Provider wire drift or unverified values** (OpenAI/Azure api-version and per-model levels, Foundry `output_config`, the claude-api table is a cached 2026-10-06 reference). Mitigation: P0 probes, `verified_on` per table row, bounded single retry without the parameter, test that every configured default model has a row.
2. **Live-only uncommitted files** (`sse.rs`, `http_routes.rs`, `zdr_features.rs`) hold half of the derivation. Mitigation: land them first; S5 hunk-minimal; ZDR parity test.
3. **Cost increase** from Q3 allowances and from Deep finally meaning Deep (today it is silently Quick, D1). Mitigation: bounded allowances, Q3 owner sign-off, S7 posture clamp, WS27 visibility, flags default off.
4. **Pinned legacy tests encode the old Anthropic behaviour** (`anthropic.rs:1903-1915`, Codex 1004-1068, `source_validation` 4096 asserts). Mitigation: change deliberately in S3/S4/S5 with the reversal named in the PR and ledger.
5. **Proto tag contention** (InferRequest 17-19, RunAgentRequest 18). Mitigation: integrator reserves per WS02 D8; S5 lands add-only.
