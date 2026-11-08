# WS02-run-context-cache - Run context for Do/Plan/scheduled runs + prompt-cache discipline

Date 2026-10-15. Audit items F-t3-001 and F-t3-022. Wave W1. Design only; no product file was changed.
Conventions: **WT** = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean). **LIVE** = `C:/dev/CoresSystem` (HEAD plus ~324 uncommitted entries). Paths are relative to `apps/Model Plane/` unless they start with `apps/`. `file:line` was opened in WT unless tagged `[live]`. `[live-only]` = exists only as uncommitted LIVE work. Rust paths abbreviate `rust/services/` as `RS/`.

## 1. Scope, items resolved, owner

**Items:** F-t3-001 (one server-assembled run context for Do, Plan and scheduled runs) and F-t3-022 (prompt-cache discipline).
**Owner plane:** Model Plane. Services: `model-gateway` (composes), `execution-core` (consumes), `inference-core` (provider wire), `cost-core` (ledger/view), `session-core` (run record). Frontend Plane is touched only for display (Usage metric, Insights tile). No Control, Data, Ingestion or Quarry change.

**IN**
- A typed `RunContext` on `RunAgentRequest`: composed authored instructions (ADR-0003), bounded prior-turn history, memory prefetch block, completeness status. Composed in model-gateway before the agentic early return's dispatch; seeded in `run_rounds`; subagents inherit instructions only.
- Completeness flags stamped on the run record (`runs.metadata`, no migration).
- Cache: Usage frame cache fields, hit-rate view with miss reasons, Do-round usage into the ledger (prerequisite for the view to see Do), prompt tier contract shared by both loops, stable-prefix order measurement on OpenAI/Azure, Anthropic tiered system blocks, provider cache scope key, compaction begin/commit bracket.

**OUT (explicit)**
- Merging the two loops. Gateway `dispatch_tool` (read-only) and execution-core `execute_step_inner` (capability/hook/permission) stay separate (Model Plane CLAUDE.md; postmortem 0001; ledger 2026-08-14). WS02 shares a context contract and a tier vocabulary, never dispatch code.
- Scheduled-run population of `RunContext` (WS20), attachments (WS08), memory pause/off semantics (WS07), durable fold checkpoints (WS03), live cost meter and pre-run estimate (WS27), `reattach_context` for the Do loop (WS05/WS24 decide), Space audience-safe memory (Space session).
- Any response/answer cache keyed on the prompt (removed twice; `RS/model-gateway/src/langcache.rs:1-126`). Slice S10 is a provider routing hint, not a cache.

**Slice tags:** [FINISH] = built, dark or half-wired; [BUILD] = new.

| Slice | Title | Tag | Effort |
|---|---|---|---|
| S1 | Usage frame carries cache tokens; SPA metric shows hit rate | FINISH | S, 1-2 d |
| S2 | Cache hit-rate view and miss reasons (cost-core + gateway + Insights) | FINISH | M, 4-6 d |
| S3 | Do-round usage reaches the ledger (`RunAgentResponse.usage`) | BUILD | M, 4-6 d |
| S4 | Run-context contract: proto, `mp-contracts::run_context`, run-record RPC, executor boundary check | BUILD | M, 3-5 d |
| S5 | Prompt tier contract + CI order check | BUILD | S-M, 3-4 d |
| S6 | Gateway run-context composer and agentic wiring | BUILD | M, 5-7 d |
| S7 | execution-core seeding, subagent inheritance, Do prefix stability | BUILD | M, 4-6 d |
| S8 | Stable-prefix order on staging OpenAI/Azure + eval rerun | FINISH | S, 2-3 d (calendar-bound) |
| S9 | Anthropic tiered system blocks | BUILD | M, 4-6 d |
| S10 | Provider cache scope key (thread = lineage) | BUILD | S, 2-3 d |
| S11 | Compaction begin/commit/abort bracket | BUILD | S, 2-3 d |
| S12 | Todo recitation | BUILD | not scheduled (no producer; see S12) |

## 2. Verified current state

### 2.1 F-t3-001 (what runs today)

| Fact | Evidence (WT) |
|---|---|
| Ask turns compose ADR-0003 instructions in one system message: platform, org, Space, persona (gated by per-thread `persona_enabled`), then agent persona. Pure over `InvokeRequest` plus `state.platform_instructions`. | `RS/model-gateway/src/sse.rs:6243-6296`; call at `:1747`; fields `http_routes.rs:6736-6762` |
| Ask turns read session-core context assembly (also yields `persona_enabled`), history, memory (400 ms bound, trivial-prompt gate). | assembly `sse.rs:4677-4776`, `:4755`; history `:5219-5425`; memory `:5036`, `:5060-5138`, gate+call `:1586-1611` |
| The agentic branch returns before any of that: `agentic_run_stream(... req.content.clone() ...)` at `:1423`; context loaders only start at `:1491`. | `sse.rs:1381-1446`, `:1491-1611` |
| `build_run_agent_request` sends goal (plus approved-plan block), ids, mode, rounds, zdr, tier, plan_mode, tools, rung, space_id, budgets, route. No instruction, history, persona or memory field. | `sse.rs:8758-8835`; proto `proto/model_plane/v1/execution.proto:76-209` (tags 1-17; **next free 18**; no `reserved`) |
| BFF already injects `org_instructions`, `space_instructions`, `persona_instructions`, `agent_*` on both ingress paths (client values stripped, Space layer gated on Control). So the data reaches model-gateway and is dropped there. | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/ag_ui.rs:83-124`; `.../chat/streams.rs:55-92` |
| `run_rounds` seeds `[system preamble, user goal]`, then inserts skills at index 1. It is reentrant: `run_subagent` calls it with `depth+1` and a fresh history. | `RS/execution-core/src/runtime_loop/agent.rs:1168-1221`; subagent `:3100-3152`; root call `:922` |
| Per-round `InferRequest` is a full struct literal (no `Default`) with `thinking_budget_tokens: 0`, `prefer_priority_service_tier: false`. | `agent.rs:1331-1381` |
| Executor boundary: `run_agent` authenticates, authorizes the run, `enforce_persistence_free_execution(&caller, req.zdr, ..)`, `enforce_agent_route`, logs ids only, then `runtime_loop::agent::run_agent`. | `RS/execution-core/src/grpc.rs:1203-1250` (boundary check at `:1237`) |
| `goal` is logged at INFO in the run start line. Any new field must not be. | `agent.rs:866-878` |

### 2.2 F-t3-022 (what runs today)

| Fact | Evidence (WT) |
|---|---|
| Cache read/write tokens are parsed for Anthropic and OpenAI/Azure; `input_tokens` already includes both legs, so hit rate = `cache_read / input_tokens`. | `RS/inference-core/src/provider/anthropic.rs:823-852` (fold-in `total_input_tokens`); `openai.rs:369-397` |
| cost-core stores and sums cache tokens (`cost_entries`), per run/org/user. `/api/v1/cost/analytics` buckets (day, model, provider, basis) carry **no cache columns**. | `go/services/cost-core/internal/postgres/store.go:133-153`, `:195-263`; `internal/postgres/analytics.go`; `internal/ledger/analytics.go:15-25`; `internal/server/analytics.go:10-70` |
| **The only USAGE_ENVELOPE producer is model-gateway.** cost-core accepts only `producer == "model-gateway"`. execution-core has no NATS/Envelope code (grep, both trees). Do-loop rounds only log tokens. The gateway comment at `sse.rs:9706-9720` ("captured by execution-core's usage envelopes") is stale: `RunAgentResponse` has no usage field and the agentic Usage frame is hard-coded 0. | `cmd/main.go:54`, `:501`; `sse.rs:9792-9842`; `:9716-9734`; `agent.rs:1450-1463`; `execution.proto:212-264` |
| Usage frame has no cache keys; SPA already parses `cache_read_tokens`/`cache_write_tokens` and **hides** the metric when both are 0. | `RS/model-gateway/src/sse_events.rs:229-244`, `:515-539`; `apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts:1355-1366`; `src/features/chat/components/ChatPanels.tsx:928-937` |
| Cache vars are in scope at the two Ask Usage construction sites. | `sse.rs:3184-3191` (vars), `:4112`, `:6849` |
| Stable-prefix order exists, off by default, effective only for a turn pinned to an OpenAI/Azure chat-completions provider. An unhinted turn is `CacheRoute::Other` because inference-core walks `INFERENCE_PROVIDER_ORDER` (Anthropic first). | `RS/model-gateway/src/prompt_context.rs:23-26`, `:59-86`, `:153-159`, `:305-348` |
| Anthropic joins **all** system messages (wherever they sit) into one `system` string, then marks one block. Reordering is a no-op there. Breakpoints: tools, system, last message; max 4; ZDR returns before any marker. | `anthropic.rs:180`, `:284-376`, `:667-728`; test names `zdr_request_must_not_send_cache_control_anywhere_in_the_body`, `breakpoint_count_never_exceeds_anthropics_limit_of_four` |
| Response cache (`inference-core/src/cache.rs`) is unary-only, ZDR-excluded, key = blake3(tenant, model, all messages). Not on `infer_stream`. | `cache.rs:1-6`, `:67-97`, `:99-120`; `provider/fallback.rs:1160`, `:1206`, `:1526` |
| No `prompt_cache_key` anywhere in inference-core. | grep |
| Compaction markers exist. Both tell the model to call `reattach_context`. **The Do loop does not offer that tool** (only `reattach_skill`). | `RS/model-gateway/src/compaction.rs:43-55`, `:383-422`; `RS/execution-core/src/runtime_loop/mod.rs:92`, `:654` |
| No model-facing todo tool in either loop; todos exist only in session-core orchestration and REST. | `execution-core` grep; `session-core/src/orchestration_store.rs:575-618`; `http_routes.rs:278-309` |
| `PromptContext` slots, no tier labels; legacy order byte-identical to old insertions. | `prompt_context.rs:195-349` |

### 2.3 Live-only / differs-in-live (must land before this stream edits those files)

| LIVE path | State | Why it matters to WS02 |
|---|---|---|
| `RS/model-gateway/src/sse.rs` | M, 883 lines | Anchors shift +68..+360 (agentic return `:1491` [live]; authored fn `:6375` [live]; build_request `:9118` [live]; spawn `:8978` [live]). ZDR direct-answer composes authored instructions (`:8380-8405` [live]). S1, S3, S6 edit it. |
| `RS/model-gateway/src/zdr_features.rs` | `??`, 365 lines | `refusal()` rejects `agentic`, `plan_mode`, `skills`, `pinned_messages` on ZDR (`:305-306`); memory `Inert` (`:165-168`). Replaces the WT early-return at `sse.rs:976-994`. |
| `RS/model-gateway/src/zdr_posture.rs`, `sse.rs:959-1019` [live] | `??` / M | `turn_posture.is_waived()` (rule 17); `retention_class`. Needed for the waived-run rule (D5). |
| `RS/model-gateway/src/sse_events.rs` | M, 203 lines | Adds `retention_class` to Usage (`:283-285` [live]); S1 edits the same variant. |
| `RS/execution-core/src/runtime_loop/agent.rs`, `grpc.rs`, `agent_route.rs` | M | Waived-route helpers (`is_waived`, `agent_route.rs:227`). Seed moved +4. |
| `proto/model_plane/v1/sessions.proto` | M, +42 | `CountWaivedRuns`, `SessionRetentionService`; `GetContextAssemblyRequest` moves 725 to 736. S4 appends an RPC to `SessionCore`. |
| `docs/sse-event-taxonomy.md` | M, +4 | At HEAD `gen-sse-event-taxonomy.py --check` fails; any PR touching `sse_events.rs` fails CI until the fixed doc lands. |
| session-core migrations | `0044`, `0045` untracked | WS02 needs **no** session-core migration (JSONB merge). cost-core needs one (own numbering). |

**Rule:** WS01 commits the ZDR wave first. S1/S3/S6/S7 branch from the commit that contains the LIVE `sse.rs`, `sse_events.rs`, `agent.rs`, `grpc.rs` changes. A branch from HEAD conflicts on ~883+207 lines.

### 2.4 Where the audit is stale or wrong (code wins)

1. **"Measurement plumbing is built end to end" (F-t3-022)** holds for gateway chat turns only. Do rounds never reach cost-core (section 2.2). Hit rate over the ledger is blind to the loop F-t3-001 is about to make heavier.
2. "Prompt cache metric reads zero": it is hidden, not zero (`ChatPanels.tsx:935`).
3. "Audit section 6.2 gateway-signalled boundary": no such text found (audit lines 1272-1280 list F-t3-070..073). The boundary is specified here (S9).
4. "Inside a Do run the fold is already append-only" is true within a run. Across runs the seed prefix is preamble + tools only, because skills (goal-matched) sit at index 1 and the goal is last.
5. "Reuse the compaction.rs budget planner" for Do: its markers name a tool the Do loop lacks. Reuse the planner, not the markers (D2).
6. "Lineage-scoped keys": in Verevon the thread id never changes across compaction (ledger 2026-08-24 "compaction edits the prompt, never the thread"), so the lineage root is the thread. This shrinks S10.
7. `session-core/src/compaction.rs` is a checkpoint-watermark job, not prompt compaction; `capability-ownership-matrix.md:24` is misleading.
8. Adjacent finding: `pii_redaction_required` is computed at `sse.rs:963` but the agentic return at `:1423` passes `req.content` unredacted (Ask redacts at `:1477`). Hand to WS26; S6 applies the same rule to the goal it composes (one line) only if WS26 agrees.
9. `go/go.work` exists (19 modules, no `.` entry); memory note "no go.work" is stale. Build cost-core per module: `cd "apps/Model Plane/go/services/cost-core" && go test ./...`.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without |
|---|---|---|---|---|---|
| D1 | Do-run inference becomes ledger usage. cost-core enqueues `cost_billing_outbox` for metered/subscription entries (`store.go:187`), so S3 starts billing Do runs. Owner: Control billing + Model lead. | (a) record and bill; (b) record with a non-billable basis; (c) defer | (a) behind flag, default off; Control confirms attribution first | S3 **enablement** (not build); S2 Do visibility | S3 build, S1, S2 (gateway turns) |
| D2 | Do history shape | (a) newest-N tail by token budget, no summarizer call, own omission notice with no `reattach_context` mention; (b) reuse Ask compaction (summary + marker naming `reattach_context`); (c) (a) now and add read-only `reattach_context` to the Do loop later | (c). (b) tells the model to call a tool the purpose-lock rejects | S6 marker text | all of S1-S5 |
| D3 | Instruction precedence (OD19) | keep ADR-0003 order platform, org, Space, persona, agent persona | No new ruling. S6 calls the same composer, so an OD19 change edits one function. No ledger reversal. Add ledger entry "Do runs inherit the Ask context; loops stay separate" in S6 | nothing | everything |
| D4 | Memory on Space-scoped Do runs. Space and agent_run threads can feed user-scope memory (overlap-space row 6). | (a) prefetch like Ask; (b) skip when `space_id` non-empty | (b) in v1, status `SKIPPED_BY_POLICY(space_scope)`; lift when audience predicate (F-t3-060 / Space S533) lands. Owner: Space session | S6 Space behaviour only | non-Space runs |
| D5 | Waived runs (rule 17). Ask path treats a waived turn as persistent for its connection (`sse.rs:1780` [live]). | (a) same as Ask; (b) instructions only | (a). Owner: ZDR/Control to confirm | S6 `full` mode for waived | `instructions` mode |
| D6 | Anthropic phase 2: fold volatile system blocks into the message tail (changes role, so changes authority framing). | build now / after data | After S2 shows the Anthropic cross-turn hit rate. Not designed here | nothing | S9 phase 1 |
| D7 | `prompt_cache_key` accepted by the deployed Azure api-version? Not determined. | send / omit per provider | Verify on staging; send only to providers that accept it | S10 enablement | S10 build |
| D8 | Proto field allocation across streams. `RunAgentRequest` 18 and `InferRequest` 17+ are contested (WS08, WS11, WS03). | first merge wins / integrator reserves | Integrator reserves: `RunAgentRequest.run_context = 18` (WS02), later streams extend `RunContext` or take 19+; `InferRequest`: 17 to WS11 (effort), `prompt_tiers = 18`, `cache_scope_key = 19` to WS02. `RunAgentResponse.usage = 9` | S3, S4, S9, S10 merge order | design/tests |
| D9 | Compaction transaction depth. Durable commit needs a durable summary store (WS17). | in-process bracket now / wait | In-process now (S11), durable later | nothing | S11 |
| D10 | Pinned ChatGPT-subscription route re-sends the whole prompt every round, cached nowhere (`prompt_context.rs:17-19`). | full context / instructions only | `instructions` mode on a pinned route until S2/S3 show quota impact | S6 default for pinned routes | rest |

**Reversals required:** none. Ledger 2026-08-14 (unify loops) stays rejected. Ledger 2026-08-24 "compaction is recoverable" is respected by D2(c). langcache removal is respected (S10 is a routing hint).

## 4. Slices (in build order)

Three tracks after S1: **A (context)** S4, S5, S6, S7; **B (measure)** S1, S2, S3; **C (cache levers)** S8, S9, S10, S11 after S5 and the S2 baseline. S5 lands before or with S6 (audit: tier contract before F-t3-001).

Common test commands (run from repo root; on Windows use `--lib` where a target fails pre-existing):
- `cd "apps/Model Plane/rust" && cargo fmt --all -- --check && cargo test -p <crate> --lib`
- `cargo test -p model-gateway --test e2e_invoke_chain_test` (gateway e2e harness)
- `cd "apps/Model Plane/go/services/cost-core" && go test ./...`
- `buf lint "apps/Model Plane/proto"` and `buf breaking "apps/Model Plane/proto" --against "apps/Model Plane/proto/baseline.binpb"` (lint has pre-existing debt; breaking must pass)
- `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`
- `cd "apps/Frontend Plane/verevonv3" && pnpm vitest run <files>`
After any proto edit: regenerate Go bindings (`buf generate`, writes `go/gen`; buf not on host, use the buf image) and force a Rust proto rebuild (`mp-contracts/build.rs:74-94` fingerprint caveat).

Deploy evidence recipe (every slice that ships a binary): rebuild the touched images with `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build <service...>`, confirm containers healthy, then run the named end-to-end flow against the running stack.

---

### S1. Usage frame carries cache tokens; SPA shows hit rate [FINISH] (S, 1-2 d)

**Goal.** The wire the SPA already reads becomes true. WS27 consumes this field.

**Files**
- `RS/model-gateway/src/sse_events.rs` - `ChatEvent::Usage` (`:229-244`) gains `cache_read_tokens: u32`, `cache_write_tokens: u32`; payload builder (`:515-539`) writes `cache_read_tokens`/`cache_write_tokens` only when either is non-zero (older clients and every earlier payload stay valid, same pattern as `cost_basis`). Update the ~8 in-file constructions in tests (`:1374`, `:1454`, `:1508`, `:1634`, `:1647`, `:1677`, `:1802`) and add `usage_carries_cache_tokens_and_omits_them_when_zero` next to `usage_carries_server_stamped_provenance_and_omits_it_when_empty` (`:1692`).
- `RS/model-gateway/src/sse.rs` - set the two fields at the construction sites `:4112` (stream; vars at `:3184-3191`) and `:6849` (unary fallback; `cache_read_tokens`/`cache_creation_tokens` in scope); the agentic site `:9716` stays 0 until S3.
- `apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPanels.tsx:928-937` - show `Prompt cache: <read> read (<pct>% of input)` using `current.inputTokens`; keep the `Show when > 0` rule (no zero shown as a number).
- `apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts:1355-1366`, `:1600-1610` - no change needed; add a case in `chat-client.test.ts`.
- New test `src/features/chat/components/ChatPanels.cache.test.tsx`.

**Contract:** no proto change. SSE event names unchanged, so the taxonomy gate only needs the doc already fixed in LIVE.
**Flag:** none (additive keys). Rollback = redeploy previous gateway.
**ZDR clause:** ZDR sends no `cache_control` (`anthropic.rs:287`) and the ZDR Usage frame is honoured by `zdr_features` (`usage` row, `:165-176` [live]); tokens are numeric, no content. Golden case: existing `zdr_posture_matrix.rs` [live-only] unchanged; add to `usage_carries_cache_tokens...` a case with both 0 (ZDR shape) asserting keys absent.
**Tests:** `cargo test -p model-gateway --lib usage_`; `pnpm vitest run src/features/chat/components/ChatPanels.cache.test.tsx src/shared/api/chat-client.test.ts`; taxonomy `--check`.
**DoD:** a multi-round Anthropic turn on the live stack shows the "Prompt cache" metric with read tokens; zero-cache turns show no metric.
**Evidence:** rebuild model-gateway + frontend; two consecutive long turns on one thread (second within 5 min); capture the SSE `usage` frame JSON (cache keys present on the second) and a screenshot of the metric.
**Deps:** WS01 (LIVE `sse_events.rs` committed). **Risks:** tool-loop turns may report only the last round's cache legs (aggregation in `tool_loop.rs:6828` logs per round; final-frame aggregation not read: verify and sum if not).

---

### S2. Cache hit-rate view and miss reasons [FINISH] (M, 4-6 d)

**Goal.** Answer "what fraction of input tokens come from cache, per provider/model/day, and why not" over the existing ledger. Definition: token-weighted `Σcache_read / Σinput_tokens` over buckets where the provider reported any cache activity; otherwise show "No cache activity reported", never 0%. Subscription basis excluded (broker caches nothing between calls).

**Files**
- `go/services/cost-core/internal/ledger/analytics.go:15-25` - `UsageBucket` += `CacheReadInputTokens`, `CacheCreationInputTokens` (json `cache_read_input_tokens`, `cache_creation_input_tokens`); in-memory store sums them (`:31-80`). Add `CacheMissBucket{reason, requests, input_tokens}` list and extend the response.
- `go/services/cost-core/internal/postgres/analytics.go` - select `SUM(cache_read_input_tokens)`, `SUM(cache_creation_input_tokens)`; second query `GROUP BY cache_miss_reason`.
- `go/services/cost-core/internal/server/analytics.go:70-90` - add `cache_breakdown` beside `buckets` (response stays `{data:{...}}`).
- NEW `go/services/cost-core/migrations/<next>_cache_miss_reason.up.sql` / `.down.sql` - `ALTER TABLE cost_entries ADD COLUMN IF NOT EXISTS cache_miss_reason TEXT` (idempotent: cost-core re-runs every `*.up.sql` at boot).
- `go/services/cost-core/internal/ledger/ledger.go` (Entry `:93-101`, `ValidateEntry` `:142-144`) - `CacheMissReason`, validated against the closed set `{below_minimum, prefix_changed, ttl_expired, no_baseline, unexplained}` (empty allowed).
- `go/services/cost-core/internal/postgres/store.go:133-153`, `:263` - persist the column.
- `go/services/cost-core/cmd/main.go:96-97`, `:531-546` - `usagePayload.CacheMissReason`; copy into `Entry`.
- NEW `RS/model-gateway/src/cache_telemetry.rs` (+ `pub mod cache_telemetry;` in `lib.rs`) - `classify(provider_used, input_tokens, read, creation, digests, tracker) -> Option<CacheMissReason>`; bounded per-replica LRU keyed by `cache_scope_key(org, thread)` holding `(leading_system_digest, all_system_digest, last_ts)`; reasons: `below_minimum` (input < 1024), `prefix_changed` (digest differs from last call), `ttl_expired` (> 5 min), `no_baseline` (no entry), `unexplained` (same digest, in TTL, read = 0). Advisory, per replica, never gates behaviour. Digest = blake3 over the leading run of system messages (OpenAI/default) or all system messages (Anthropic) plus tool names; held in memory only.
- `RS/model-gateway/src/sse.rs:9792-9842` `build_usage_envelope` - add `cache_miss_reason` to the payload (additive); sites `:3987`, `:6811` pass it.
- `apps/Frontend Plane/verevonv3/src/shared/api/cost-client.ts:4-17` - zod: new fields `.optional().default(0)` / `.default([])` so an older cost-core still parses.
- `apps/Frontend Plane/verevonv3/src/features/insights/lib/usage-analytics.ts` - `cacheHitRate(buckets)`; `UsageAnalyticsSection.tsx` - one tile and a reasons list; CSV gains two columns (`usageCsv`).
- No BFF change: `apps/gateway/src/domains/cost.rs:344-370` proxies bytes. No new action contract (read-only columns in an existing section); confirm with `node --test apps/gateway/scripts/action-surface-contract.test.mjs`.

**Contract:** NATS payload additive; JSON additive; DB additive. **Flag:** none (read model). Rollback: previous images; column is nullable.
**ZDR clause:** ZDR envelopes are never persisted (`cmd/main.go:492` rejects `env.ZDR`); digests never leave process memory; `cache_miss_reason` is a closed enum. View therefore contains no ZDR turn. Waived turns are persistent for their connection and included. Golden case: cost-core test `TestDecodeUsageMessage_ZDRNeverReachesLedger` already covers rejection; add a table case with `cache_miss_reason` set on a ZDR envelope asserting rejection.
**Tests:** `go test ./...` in cost-core: new `analytics_cache_test.go`, extend `ledger_test.go:37`, `server_test.go:115`, `store_integration_test.go` (Postgres), `cmd/main_test.go`; Rust `cargo test -p model-gateway --lib cache_telemetry`; vitest `usage-analytics.test.ts`.
**DoD:** Insights shows hit rate per model/provider/day and a reasons list; a provider that reports no cache legs shows "No cache activity reported".
**Evidence:** rebuild cost-core + model-gateway + frontend; 6-turn thread on Anthropic and on a pinned `openai` provider; capture `GET /api/v1/cost/analytics?scope=organization` JSON and the Insights screenshot; capture the baseline numbers for S8/S9 decisions.
**Deps:** S1 optional; D1 not needed (gateway turns only). **Risks:** per-replica LRU gives false `no_baseline` behind a load balancer (labelled advisory); `provider_used` strings unverified per provider ("not determined") so eligibility is by observed cache activity, not by name.

---

### S3. Do-round usage reaches the ledger [BUILD] (M, 4-6 d; enablement gated by D1)

**Goal.** Make Do rounds measurable and meterable, with cache fields, without giving execution-core a NATS identity (cost-core accepts only `model-gateway`).

**Files**
- `proto/model_plane/v1/execution.proto` - `RunAgentResponse` (`:212-264`) += `RunUsage usage = 9;` NEW `message RunUsage { uint64 input_tokens = 1; uint64 output_tokens = 2; uint64 cache_read_input_tokens = 3; uint64 cache_creation_input_tokens = 4; uint32 inference_rounds = 5; string provider_used = 6; string model_used = 7; }` (parent plus delegated subagent rounds; per invocation; numeric and ids only).
- `RS/execution-core/src/runtime_loop/agent.rs` - `LoopContext` (`:362-422`) += `usage: Arc<UsageTally>` (cloned into child contexts at `:3109-3140` like `refusal`); `run_rounds` adds each round's `response.*_tokens`/cache legs after the log at `:1450-1463`; fill `RunAgentResponse.usage` at all 5 literal sites (including the `Paused` return path).
- `RS/model-gateway/src/sse.rs` - in `agentic_run_stream` after the response (`:9640+`): `build_usage_envelope(... run_id ...)` with `idempotency_key = "{request_id}-AGENT_USAGE"`, event type stays `USAGE_ENVELOPE`, payload `run_id`, cache fields, `cost_basis` from `CostBasis::of_route`; publish to `usage_subject(org)` only when `!zdr` and `MODEL_GATEWAY_AGENT_USAGE_ENVELOPE` on. Fill the agentic `ChatEvent::Usage` tokens and cache fields from `response.usage` (cost stays `None`; WS27 prices). Replace the stale comments at `:9706-9720`.
- `crates/mp-contracts/tests/proto_wire_parity_execution.rs` and `go/gen/model_plane/v1/proto_wire_parity_execution_test.go` - add a `RunAgentResponse` golden with `usage`.
- Update the 6 `RunAgentResponse {` literals in `RS/model-gateway/tests/e2e_invoke_chain_test.rs` and 5 in `sse.rs` tests.

**Flag:** `MODEL_GATEWAY_AGENT_USAGE_ENVELOPE` (default **off**; add pass-through next to `deploy/docker-compose.yml:415-417`). Rollout: deploy execution-core first (old gateway ignores field 9), then gateway; flip after D1. Rollback: unset.
**ZDR clause:** ZDR Do runs are refused at admission; defensively the gateway publishes nothing when `zdr`. Numeric only. Golden: gateway unit test `agent_usage_envelope_is_never_published_for_a_zdr_run`.
**Tests:** `cargo test -p execution-core --lib` new `a_run_reports_the_sum_of_its_rounds_including_delegated_ones` (uses `spawn_inference_channel_with_messages`, `agent.rs:~5538` mock), `a_paused_run_reports_the_usage_of_its_invocation`; `cargo test -p model-gateway --test e2e_invoke_chain_test` new `agentic_run_publishes_one_usage_envelope_with_run_id`; wire-parity Rust and Go.
**DoD:** a Do run produces one ledger row with `run_id`, tokens, cache legs; `GET /api/v1/cost/run?run_id=` is non-zero; Usage frame shows real tokens.
**Evidence:** rebuild execution-core, model-gateway, cost-core; one multi-round Do run; capture NATS/ledger row (`/api/v1/cost/entries`), `/api/v1/cost/run`, the frame, and the unchanged row count with the flag off.
**Deps:** D1, D8. **Risks:** billing impact (D1); double counting on a resumed run (idempotency key per dispatch `request_id`; WS03 owns resume semantics, WS27 extends with per-round progress); Windows cargo target failures (use `--lib`).

---

### S4. Run-context contract [BUILD] (M, 3-5 d)

**Goal.** Land the wire, the shared bounds and the run-record stamp with **no behaviour change**: absent field = today.

**Files**
- `proto/model_plane/v1/execution.proto` - `RunAgentRequest` (`:76-209`) += `RunContext run_context = 18;` NEW:
  - `message RunContext { string authored_instructions = 1; repeated ChatMessage history = 2; string memory_block = 3; RunContextStatus status = 4; }` (`ChatMessage` from the already imported `inference.proto`; fields 5+ reserved for WS08 attachments and WS20 run notes, not declared here).
  - `message RunContextStatus { RunContextPart instructions = 1; RunContextPart history = 2; RunContextPart memory = 3; }`, `message RunContextPart { RunContextState state = 1; uint32 included = 2; uint32 omitted = 3; string reason = 4; }` (`reason` is a closed token, never content).
  - `enum RunContextState { RUN_CONTEXT_STATE_UNSPECIFIED = 0; INCLUDED = 1; TRUNCATED = 2; EMPTY = 3; SKIPPED_BY_POLICY = 4; UNAVAILABLE = 5; }`.
- `proto/model_plane/v1/sessions.proto` - `SessionCore` (`:18-153`) += `rpc RecordRunContext(RecordRunContextRequest) returns (RecordRunContextResponse)`; `RecordRunContextRequest { string run_id = 1; string org_id = 2; RunContextStatus status = 3; }` (declare `RunContextStatus` in `execution.proto`; import, or duplicate under a `session` name if the import would be circular: `execution.proto` imports `runs.proto`, check cycles).
- NEW `crates/mp-contracts/src/run_context.rs` (+ `pub mod run_context;` in `lib.rs:1-5`) - constants `MAX_AUTHORED_INSTRUCTION_BYTES = 16384`, `MAX_HISTORY_MESSAGES = 24`, `MAX_HISTORY_BYTES = 65536`, `MAX_MEMORY_BYTES = 8192`; `validate(&RunContext, zdr: bool) -> Result<(), RunContextRefusal>` (refuses: ZDR with non-empty history or memory; roles other than user/assistant except one leading system notice; over-cap sizes; history last message role not user is allowed); closed reason tokens; `status_json(&RunContextStatus) -> serde_json::Value` for the run record.
- `RS/session-core/src/grpc.rs` - handler beside `set_run_mode` (`:4984-5076`): `authorize_operation(&caller, "session:write")`, `caller.authorize_org`, `authorize_run_owner(.., OwnerIntent::Mutate)`, then `UPDATE runs SET metadata = COALESCE(metadata,'{}'::jsonb) || jsonb_build_object('run_context', $2::jsonb) WHERE id=$1 AND org_id=$3` (same merge pattern as `:5031-5059`; **no migration**; `GetRun` already returns metadata). Refuse when the run is terminal? Not determined: mirror `set_run_mode`, which does not.
- `RS/execution-core/src/grpc.rs:1237` - after `enforce_persistence_free_execution`, call `mp_contracts::run_context::validate(req.run_context.as_ref(), req.zdr)`; refusal maps to `Status::invalid_argument` with the closed token only.
- Struct-literal fixes (`..Default::default()` or the new field): execution-core `agent_route.rs:263,273`, `grpc.rs:1873,1959`, `agent.rs` 13 sites (`:5018`.. `:8701`), model-gateway `sse.rs:644-800` tests and `build_run_agent_request`, `tests/e2e_invoke_chain_test.rs`; 27 sites in total (sweep count).

**Flag:** none (inert until S6). **Compat:** old executor skips field 18 (safe: it ignores context, which is today's behaviour); gateway sends only when S6's flag is on.
**ZDR clause:** `validate` enforces: ZDR run refuses history and memory; instructions allowed (persona precedent, `sse.rs:6232-6242`). Status JSON carries enums and counts only. Golden case: NEW `crates/mp-contracts/tests/run_context_posture.rs` - table over (zdr, waived, persistent) x (instructions, history, memory); runs as part of `cargo test -p mp-contracts`.
**Tests:** `cargo test -p mp-contracts`; session-core unit `record_run_context_merges_without_dropping_other_keys` (and ignored Postgres test following the `zdr_receipt` pattern); execution-core `a_zdr_run_with_history_is_refused_at_the_boundary`; `buf breaking` (additive); Go bindings regenerated; wire-parity golden for `RunAgentRequest` (none exists today: add).
**DoD:** all literals compile; no behaviour change; `buf breaking` green; migrations untouched.
**Evidence:** rebuild session-core, execution-core, model-gateway; run an existing Do flow unchanged (regression) and `grpcurl`/test call `RecordRunContext`, capture `runs.metadata` JSON.
**Deps:** D8; WS01 (LIVE `sessions.proto`, `grpc.rs`). **Risks:** Rust proto rebuild fingerprint; `RunContextStatus` import cycle; concurrent edits to `sessions.proto` by the ZDR wave.

---

### S5. Prompt tier contract and CI order check [BUILD] (S-M, 3-4 d)

**Goal.** One vocabulary for "what may precede the cache breakpoint" in both loops, enforced by a test, before more context is added.

**Tiers (closed, ordered):** `Stable` < `Context` < `Conversation` < `Volatile` < `Current`.
- Stable: platform preamble / response discipline / user-supplied-args notice / conversation-scope notice.
- Context: authored instructions (ADR-0003 text), identity, verbosity, hoisted pins and head summary.
- Conversation: prior turns.
- Volatile: date notice, artifact inventory, calendar, grounding, context-assembly block, memory, skills, generated-image state, format guidance, approved plan, (later) todo recitation.
- Current: the current user turn / goal.

**Files**
- NEW `crates/mp-contracts/src/prompt_tier.rs` (+ `lib.rs`) - `enum PromptTier`, `struct PromptTierPlan { stable: u32, context: u32 }` (leading message counts), `fn plan_of(tiers: &[PromptTier]) -> Result<PromptTierPlan, OrderViolation>` (errors if tiers decrease), `fn blocks_of(...)`.
- `RS/model-gateway/src/prompt_context.rs:305-348` - `render_stable_prefix` returns tiers alongside messages via NEW `render_tiered(order) -> (Vec<ChatMessage>, Option<PromptTierPlan>)`; the `Legacy` order returns `None` (byte-identical). The date notice moves from the standing group to Volatile (after history): deliberate, flag-gated, the order is unmeasured today. Update `the_stable_prefix_order_puts_standing_blocks_first_and_per_turn_blocks_last` accordingly and add `every_slot_has_a_tier_and_stable_order_never_decreases`.
- `RS/execution-core/src/runtime_loop/agent.rs:1181-1221` - tier labels attached in S7, not here.
- NEW `RS/execution-core/tests/prompt_tier_contract.rs` - follows `tests/compaction_parity_contract.rs` (reads model-gateway source, CRLF-normalised): asserts both loops reference `mp_contracts::prompt_tier`, and that the gateway slot table and the execution seed order agree on tier names. Source-reading, not a shared dispatch (loops stay separate).

**Flag:** none (pure refactor; legacy order untouched; `MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER` still gates the order).
**ZDR clause:** nothing persisted; tiers are in-memory labels. No new golden case.
**Tests:** `cargo test -p mp-contracts`; `cargo test -p model-gateway --lib prompt_context`; `cargo test -p execution-core --test prompt_tier_contract`.
**DoD:** CI check fails when a Volatile slot is placed before `Conversation` in the stable order or when an unlabelled slot is added (struct is destructured without `..`, as `zdr_features::invoke_field_names` does).
**Evidence:** `cargo test` output; no deploy required (no behaviour change with the flag off).
**Deps:** none. **Risks:** moving the date notice changes prefix bytes for flagged routes only.

---

### S6. Gateway run-context composer and agentic wiring [BUILD] (M, 5-7 d)

**Goal.** Every Do and Plan run starts with the Ask-turn instruction stack, bounded history and memory, with honest completeness flags.

**Design**
- NEW `RS/model-gateway/src/run_context.rs` (+ `pub mod run_context;` in `lib.rs`, between `retrieval_tools` and `run_budget`) so `sse.rs` takes a thin diff. Contents: `RunContextMode { Off, Instructions, Full }` from `MODEL_GATEWAY_RUN_CONTEXT`; `AuthoredLayers` (owned org, Space, persona, agent name and prompt strings); `compose(state, ctx) -> RunContext`; bounds from `mp_contracts::run_context`; `DO_HISTORY_OMITTED_NOTICE` (honest, names no tool, D2).
- Extract the pure body of `authored_instructions_message` (`sse.rs:6243-6296`) into `compose_authored_instructions(platform, &AuthoredLayers, persona_enabled)`; keep `authored_instructions_message` as a wrapper so its tests (`sse.rs:10756-10862`) pass unchanged. **One composer for Ask and Do.**
- Visibility: `pub(crate)` on `authored_instructions_message`, `load_context_assembly_messages` (`:4677`), `fetch_chat_memory_context` (`:5060`), `authenticated_session_request` (`:68`) and the return types they need. Memory goes through the same helper so WS07's pause/off gate applies once.
- Compose **inside** `agentic_run_stream`'s spawned task (`sse.rs:9196`) after the `connected` frame, before `spawn_run_dispatch`, so first byte is not delayed. The three reads run concurrently with `tokio::join!`: (a) `load_context_assembly_messages` with `data_plane_bearer = None` and `max_tokens` at the minimum, used **only** for `persona_enabled` (no retrieval fan-out; the assembly block is not injected; v1); (b) `list_conversation` (same request shape as `:5233-5254`, owner-bound, `space_id` empty), drop the last user message (the goal carries it), keep `user`/`assistant` only, plan the tail with `compaction::plan_head_summary_by_budget` (`compaction.rs:253`) and a new `apply_head_drop(messages, head, notice)` beside `apply_head_summary` (`:399-422`) - no summarizer call, no inference in admission; (c) `fetch_chat_memory_context` (400 ms, trivial-prompt gate, same block as Ask via `memory_context_block`).
- Status per part: `INCLUDED | TRUNCATED | EMPTY | SKIPPED_BY_POLICY | UNAVAILABLE` with closed reasons (`zdr`, `waived_default`, `space_scope`, `route_uncached`, `trivial_prompt`, `timeout`, `no_credential`, `error`). Never fabricate `INCLUDED`.
- Stamp via `RecordRunContext` best-effort (mirror `mark_run_plan_mode`, `sse.rs:6154-6193`).
- `spawn_run_dispatch` (`:8618-8709`) and `build_run_agent_request` (`:8758-8835`) take `run_context: Option<RunContext>`; `agentic_run_stream` (`:9167`) takes `AuthoredLayers`. Call site `:1423-1445` builds `AuthoredLayers` from `req`.
- Mode rules: `Off` = no field (today). `Instructions` = instructions only. `Full` = instructions + history + memory. Pinned subscription route forces `Instructions` (D10). Space-scoped (`agentic_space_id` non-empty) skips memory (D4). ZDR/waived per D5 and the LIVE `turn_posture`.

**Files**
- NEW `RS/model-gateway/src/run_context.rs`; `RS/model-gateway/src/lib.rs`; `RS/model-gateway/src/sse.rs` (above); `RS/model-gateway/src/compaction.rs` (`apply_head_drop`); `deploy/docker-compose.yml:415-417` neighbourhood (pass `MODEL_GATEWAY_RUN_CONTEXT`, `MODEL_GATEWAY_RUN_CONTEXT_HISTORY_TOKENS`, empty default).
- `docs/decisions/ledger.md` - new entry "Do runs inherit the Ask run context; the two loops stay separate".
- `python/eval-lab-py/cases/run_context/15_do_run_remembers_thread_fact.yaml`, `16_do_run_follows_org_instruction.yaml` (if the harness can set `org_instructions`; else cover instructions in the gateway e2e) and update `tests/test_runner.py:31` (suite count assertion, currently 14).

**Flags:** `MODEL_GATEWAY_RUN_CONTEXT=off|instructions|full` default **off**; `MODEL_GATEWAY_RUN_CONTEXT_HISTORY_TOKENS` default 8000, clamp 1000-40000. Read once (OnceLock, restart to change, like `prompt_context.rs:117-129`). Rollout: staging `instructions`, then `full`; prod after S7 and the eval gate. Rollback: set `off`, restart.
**ZDR clause:** agentic ZDR is refused before this code (`sse.rs:976-994`; `zdr_features.rs:305-306` [live]). Defensive: `compose` returns history and memory `SKIPPED_BY_POLICY(zdr)` and logs counts only; run record stores enums only; text never logged. Golden cases: gateway test `a_zdr_turn_never_reaches_run_context_composition`; mp-contracts table from S4.
**Tests:**
- Unit `cargo test -p model-gateway --lib run_context`: `composes_the_same_instruction_text_as_an_ask_turn`; `precedence_is_platform_org_space_persona_agent`; `persona_off_thread_omits_persona`; `history_drops_the_current_user_turn`; `history_tail_respects_the_token_budget_and_leaves_an_honest_notice`; `memory_timeout_is_unavailable_not_included`; `trivial_prompt_skips_memory`; `space_scoped_run_skips_memory`; `pinned_route_gets_instructions_only`; `off_sends_no_field`.
- e2e `cargo test -p model-gateway --test e2e_invoke_chain_test` new `agentic_dispatch_carries_the_composed_run_context` using `run_agent_request_for_turn` (`:4358`) with seeded `MockSessionCore` history and `org_instructions` in the body; plus `..._with_the_flag_off_is_byte_identical` (reuse the "before the fields existed" pattern at `sse.rs:707`).
- Existing guards to keep green: `the_assembled_prompt_is_not_a_function_of_the_question` (`sse.rs:11559`), compaction tests in `compaction.rs`.
**DoD:** flag `full` on staging: a Do run on a thread with prior turns, an org instruction and a persona answers consistently with them; `runs.metadata.run_context` shows the states; flag `off` is byte-identical.
**Evidence:** rebuild model-gateway (and execution-core from S7); flow: thread turn 1 (Ask) states a fact; set an org instruction in Control UI; turn 2 in Do mode asks about the fact; capture the gateway log (statuses and counts only), `GET` run record metadata, the answer, and the SPA run console.
**Deps:** S4, S5, S7 (to be useful end to end), WS01 (LIVE `sse.rs`), D2-D5, D10; WS07 for pause/off. **Risks:** first-byte latency (concurrent reads, 400 ms memory cap); prompt-size growth raises every round's cost until S8/S9 land (hence flag off, `instructions` first); `sse.rs` merge pressure (thin diff, new module).

---

### S7. execution-core seeding and Do prefix stability [BUILD] (M, 4-6 d)

**Goal.** Consume the context in `run_rounds` so cross-run prefixes repeat, without touching dispatch.

**Seed order (when `run_context` present)** - `agent.rs:1181-1221`:
1. system: `compose_system_prompt(&allowlist)` (Stable)
2. system: `authored_instructions` verbatim (Context) - **inherited by subagents**
3. history messages oldest-first, any leading system notice first (Conversation) - **root loop only** (`ctx.depth == 0`)
4. system: `memory_block` (Volatile) - root only
5. system: skills (Volatile; today inserted at index 1 at `:1206-1221`, moves here only when `run_context` is present)
6. user: goal incl. approved-plan block (Current)
Without `run_context` the seed is exactly today's (test pins it). Execution-core never re-resolves instructions, reads Space data or calls session-core for history.

**Files**
- `RS/execution-core/src/runtime_loop/agent.rs` - seed builder (new fn `seed_messages(ctx, goal)`); `PromptTierPlan` attached to each round's `InferRequest` (field added in S9; here only compute and unit-test); child contexts at `:3109-3140` inherit instructions only.
- `agent.rs` `merged_tool_defs` (`:622-690`) - sort the appended MCP and client-tool segments by name so the tool list prefix is deterministic across runs (built-ins keep their order).
- `RS/execution-core/src/grpc.rs` - nothing beyond S4.
- Logs: one content-free line `run_agent: seeded run context` with `instructions_bytes`, `history_messages`, `memory_present`, statuses. Never the text. `goal` logging at `:876` is pre-existing; do not copy it.
- `RS/execution-core/tests/prompt_tier_contract.rs` (S5) extended with the Do seed order.

**Flag:** none in executor (the gateway flag is the single switch; an executor-side switch would make recorded status lie). **Compat:** old gateway sends no field = today.
**ZDR clause:** `validate` (S4) runs first; text never logged; checkpoints written by WS03 later must scrub these messages under the existing `scrub_checkpoint` pattern (`tests/scrub_checkpoint_test.rs`): note to WS03. Golden: `run_context_text_is_never_logged` modelled on `each_round_logs_its_token_usage_and_never_its_content` (`agent.rs:8698`).
**Tests:** `cargo test -p execution-core --lib`: `seed_without_run_context_is_unchanged`; `seed_orders_instructions_history_memory_skills_goal`; `subagent_inherits_instructions_but_not_history_or_memory` (extends the isolation test at `:10015`, which asserts a child seed of 2 messages for a request without context: keep it as the no-context case); `run_context_text_cannot_change_the_permission_posture` (text such as "skip approvals" leaves `mode`, rung and gates unchanged: the gate reads `req.mode`/`autonomy_rung` only); `tool_list_order_is_deterministic_for_mcp_and_client_tools`; `cargo test -p execution-core --test cross_service_loop_contract --test compaction_parity_contract` stay green.
**DoD:** the S6 evidence flow passes end to end with the executor rebuilt; subagent test green; no new log line contains content.
**Evidence:** rebuild execution-core + model-gateway; the S6 flow plus a delegating run; capture executor log line, observed inference messages count/roles from a debug-only test run (the capturing mock) and the answer.
**Deps:** S4, S5; S6 for end-to-end. **Risks:** old-run resume re-seeding is WS03's (the same `seed_messages` must be callable from a continue path); history in the fold interacts with `clear_stale_tool_results` (`compaction_budget.rs:111`): history carries no tool results, so no change; confirm by test.

---

### S8. Stable-prefix order measured on staging OpenAI/Azure [FINISH] (S, 2-3 d, calendar-bound)

**Goal.** Turn the built, dark order into a decision backed by numbers.
**Steps (no code except the S5 tier alignment already landed):**
1. Staging stack with an OpenAI or Azure deployment. Requests must **pin** `provider` to `openai`/`azure-openai`, otherwise the turn is `CacheRoute::Other` and the flags do nothing (`prompt_context.rs:64-67`).
2. Baseline run, flags off: 3 threads x 8 turns, 2 Do runs per thread. Record the S2 view per provider/model.
3. Set `MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER=1` (optionally `MODEL_GATEWAY_COMPACTION_TOKEN_BUDGET`, `MODEL_GATEWAY_DEDUP_CONTEXT_HISTORY`, one change per run), restart, repeat.
4. Eval parity: `cd "apps/Model Plane/python/eval-lab-py" && make selftest` (offline) and `make eval` live (`EVAL_LIVE=1`), flag off vs on, three reruns; no baseline or compaction case may flip pass to fail.
5. Record the answer-quality effect (previously unmeasured) in the ledger.
**Files:** `docs/decisions/ledger.md` entry; `deploy/docker-compose.yml:415-417` default only if the owner approves.
**ZDR clause:** the order is already off for ZDR (no durable path); S5 tests unchanged. **Flag:** existing. **Rollback:** unset + restart.
**DoD:** a written A/B table (hit rate, cache write share, eval pass rate) and an owner decision to flip or not.
**Evidence:** the two `cost/analytics` JSON captures and the two eval reports.
**Deps:** S2, S5; staging OpenAI/Azure credentials (not determined). **Risks:** low prompt volume gives noisy rates; the OpenAI route is not the default route (Anthropic first), so this tests a minority path: say so in the table.

---

### S9. Anthropic tiered system blocks [BUILD] (M, 4-6 d; value gated by S2 data)

**Goal.** Make the default route's prefix stable across turns and runs. Phase 1 only: split `system` into blocks with breakpoints after Stable and Context; Volatile system content follows with no breakpoint. (Phase 2, folding volatile blocks into the message tail, is D6 and not designed here.)

**Files**
- `proto/model_plane/v1/inference.proto` - `InferRequest` (`:100-175`, tags 1-16 used) += `PromptTierPlan prompt_tiers = 18;` NEW `message PromptTierPlan { uint32 stable_messages = 1; uint32 context_messages = 2; }` (counts of leading `messages`; avoids adding a field to `ChatMessage`, whose full-literal sites number ~156). Tag per D8.
- `RS/inference-core/src/provider/mod.rs:120-165` - internal `InferRequest` += `prompt_tiers: Option<PromptTierPlan>`; `RS/inference-core/src/grpc.rs:~1089` maps it.
- `RS/inference-core/src/provider/anthropic.rs:667-728` - when tiers present, flag on and `!zdr`: build `system` as ordered blocks (Stable, Context, Volatile text) instead of one string; `apply_prompt_caching` (`:284-376`) marks the Stable and Context blocks (subject to `min_cacheable_tokens` accumulation and `MAX_CACHE_BREAKPOINTS = 4`: tools, stable, context, last message). Absent tiers or flag off: byte-identical.
- `RS/model-gateway/src/prompt_context.rs` / `sse.rs` / `tool_loop.rs:6653` and `RS/execution-core/.../agent.rs:1381` - send the plan computed in S5/S7 (the literal at `agent.rs:1381` and `tool_loop.rs:6653` need the new field).
- `deploy/docker-compose.yml` - pass `INFERENCE_ANTHROPIC_TIERED_SYSTEM` in the inference-core block (`:808+`).

**Flag:** `INFERENCE_ANTHROPIC_TIERED_SYSTEM` default **off**. Rollout: inference-core first (callers' unknown field is skipped by old builds), then enable. Rollback: unset.
**ZDR clause:** the ZDR gate stays the first statement of `apply_prompt_caching` (`:287`); with `zdr` the tier plan is ignored and the body is today's. Golden: `zdr_tiered_request_sends_no_cache_control_and_one_system_string`.
**Tests:** `cargo test -p inference-core --lib anthropic`: `tiered_system_marks_stable_and_context_not_volatile`; `tiered_system_never_exceeds_four_breakpoints`; `no_tiers_is_byte_identical_to_today`; `below_minimum_prefix_marks_nothing`; existing `zdr_request_must_not_send_cache_control_anywhere_in_the_body` and `breakpoint_count_never_exceeds_anthropics_limit_of_four` must pass unchanged; wire-parity for `InferRequest` additive; `buf breaking`.
**DoD:** on staging, two consecutive turns with a changed volatile block show `cache_read_input_tokens` covering the Stable+Context blocks (read > 0 where today it is 0).
**Evidence:** rebuild inference-core + model-gateway; Anthropic thread, turn 1 then turn 2 with a different memory recall; capture S2 view before/after and the per-turn `cache_read_input_tokens` from the S1 frame.
**Deps:** S2 (baseline decides whether this ships), S5, S7, D8. **Risks:** Anthropic content-block limits and minimum-prefix rules differ by model (`min_cacheable_tokens` `:206`); changing the system representation affects native compaction negotiation (`anthropic.rs:519-613`): run `anthropic_compaction_tests.rs` (verify it compiles under `cargo test`: not determined).

---

### S10. Provider cache scope key [BUILD] (S, 2-3 d; enablement gated by D7)

**Goal.** Pin provider-side prefix-cache routing per thread (lineage root = thread, section 2.4 item 6). **Not a response cache.**
**Files**
- `crates/mp-contracts/src/prompt_tier.rs` - `fn cache_scope_key(org_id, thread_id) -> String` = first 32 hex of blake3 over `"verevon.prompt-cache.v1"` plus length-prefixed org and thread (domain-separated, no user content). Subagent loops append `:sub`.
- `proto/model_plane/v1/inference.proto` - `InferRequest.cache_scope_key = 19`.
- `RS/inference-core/src/provider/openai.rs:411-441` - add `prompt_cache_key` to the chat-completions body only for providers verified to accept it and only when `!zdr`; flag `INFERENCE_OPENAI_PROMPT_CACHE_KEY` default **off**. Anthropic ignores it.
- Callers set it: gateway Ask/tool-loop sites and `agent.rs:1381`.
- `inference-core/src/cache.rs` is **not** changed; `langcache.rs` doc and `the_assembled_prompt_is_not_a_function_of_the_question` stay.
**ZDR clause:** omitted under ZDR; the key is a hash of ids, no content. Golden: `openai_zdr_body_has_no_prompt_cache_key`.
**Tests:** `cargo test -p inference-core --lib openai`: `prompt_cache_key_sent_only_when_enabled_and_not_zdr`; `cargo test -p mp-contracts` determinism and domain separation.
**DoD / Evidence:** staging: S8 flow with the key on shows non-decreasing hit rate versus S8's best run; capture the S2 view.
**Deps:** S8, D7, D8. **Risks:** Azure may reject the unknown parameter (hence per-provider gate); benefit unproven (staging measure decides).

---

### S11. Compaction begin/commit/abort bracket [BUILD] (S, 2-3 d)

**Goal.** Make compaction an explicit transaction with an unmatched-start state, without a new store.
**Files**
- `RS/model-gateway/src/compaction.rs` - `CompactionTx { begin(lineage, head_range) -> Tx; commit(summary_digest); abort(reason) }`; `plan_head_summary_by_budget` callers (`sse.rs:5326-5338`, `:5405-5419`) bracket the summarizer call; a timeout or failure is `abort` and yields the existing honest marker (`DROPPED_HISTORY_NOTICE`, test `a_failed_summary_degrades_to_truncation_with_an_honest_marker`, `compaction.rs:~1251`).
- In-process unmatched-start guard keyed by `(thread, generation)`: a begun-never-committed entry older than the summary timeout (`COMPACTION_SUMMARY_TIMEOUT` 6 s, `sse.rs:4443`) is reported and treated as aborted; prevents repeated summariser storms on one thread.
- Metric `mp_gateway_compaction_total{phase,outcome}`; `compaction_generation` carried in the stored head-summary metadata (`session_flow::persisted_head_summary`) for observability.
- Durable commit (summary persisted when computed, not with the answer) needs a session-core store: **WS17**. Interface only.
**Flag:** none. **ZDR clause:** in-process, no content. **Tests:** `cargo test -p model-gateway --lib compaction`: `a_begun_but_never_committed_compaction_is_reported_unmatched`; `commit_after_abort_is_refused`; existing `a_fresh_budget_compaction_does_not_trigger_again_on_the_next_turn`, `a_stored_summary_is_reused_byte_for_byte_without_a_new_summary_call`, `leading_system_messages_are_never_compacted_by_budget` unchanged. `compaction_parity_contract.rs` stays green (notices untouched).
**DoD / Evidence:** metric visible on `/metrics`; a forced summariser timeout shows `abort` and the honest marker; capture both. **Deps:** none; durable half WS17. **Risk:** low value until WS17; per-replica state (WS04).

---

### S12. Todo recitation [BUILD] - not scheduled
No producer: neither loop has a model-facing todo tool (section 2.2), and todos are written by session-core orchestration. Recitation in the Volatile tail needs a source. Revisit when WS14/WS24/WS03 introduce a todo tool or plan-step list; then add `list_todos_by_plan` (`orchestration_store.rs:618`) rendering under a fixed header in tier Volatile. No work now; the tier contract already reserves the slot.

## 5. Interfaces

**PROVIDES**

| To | Contract |
|---|---|
| WS03 durable loop | `RunAgentRequest.run_context = 18` and `run_context::compose` reusable for a "continue" seeded run; `seed_messages(ctx, goal)` callable from a resume path; fold messages from `RunContext` must be scrubbed in checkpoints (`scrub_checkpoint_test.rs`). |
| WS20 scheduled runs | `RunContext` message and composer; scheduled lane (`ExecuteScheduledStepRequest`, content-free by design) must load its instruction snapshot itself and set fields 5+ for run notes. Org/Space/persona text for an unattended run has no live BFF: WS20 must define a signed snapshot. |
| WS27 cost meter | `ChatEvent::Usage.cache_read_tokens/cache_write_tokens` (S1); `RunAgentResponse.usage` (S3, tag 9); `cost_entries.run_id` populated for Do runs; hit-rate buckets (S2). |
| WS07 memory governance | Do runs call the same `fetch_chat_memory_context` helper; WS07's pause/off/reset gate applies once. Request: SearchMemory (or the helper) returns a typed skip reason so S6 can stamp `SKIPPED_BY_POLICY(memory_off)`. |
| WS08 attachments | `RunContext` fields 5+ reserved; WS08 adds attachment refs without a new `RunAgentRequest` field. |
| WS11 effort | `InferRequest` tag 17 left to WS11 (D8). |
| WS15 lifecycle events | `runs.metadata.run_context` (enum states) for status cues ("ran without memory"). |
| WS17 history foundation | Do history source is `list_conversation`; WS17 replaces it behind `run_context::load_history`; durable summary store completes S11. |
| WS24/WS25 | `reattach_context` for the Do loop (D2c) is theirs; tier vocabulary for hook-injected context. |

**CONSUMES**

| From | Contract |
|---|---|
| WS01 | LIVE ZDR wave committed (`sse.rs`, `sse_events.rs`, `agent.rs`, `grpc.rs`, `sessions.proto`, taxonomy doc) before S1/S3/S6/S7; staging deploy path. |
| WS05/WS24 | Read-only tool class for `reattach_context`; no gate bypass. |
| WS07 | Memory mode semantics (above). |
| WS26 | Decision on PII redaction of the Do goal (section 2.4 item 8). |
| Control (billing) | D1 attribution for Do-run usage. |
| Space session | D4 audience predicate; Space composer starting to send `agentic` makes Space runs consume S6 automatically. |

**Shared files and edit order (conflict hot spots)**

| File | Others touching it | Rule |
|---|---|---|
| `model-gateway/src/sse.rs` | WS01, WS03, WS04, WS27 | WS01 lands LIVE changes; WS02 then edits with thin diffs (new module `run_context.rs`, `cache_telemetry.rs`); WS02 S1/S6 merge before WS27 |
| `proto/.../execution.proto` | WS03, WS08, WS11, WS24 | WS02 (S3, S4) edits first: tag 18 and response tag 9; others extend `RunContext` or take 19+ |
| `proto/.../inference.proto` | WS11 | WS11 17, WS02 18 and 19 (D8) |
| `proto/.../sessions.proto` | WS01 (ZDR wave), WS17 | WS01 first; WS02 appends one RPC at the end of `SessionCore` |
| `execution-core/.../agent.rs` | WS03, WS05, WS11, WS24, WS25 | WS02 S7 touches seed (`:1181-1221`), `InferRequest` literal (`:1331-1381`), response literals; WS03 owns the checkpoint/resume sections |
| `model-gateway/src/sse_events.rs` | WS15, WS27 | S1 adds two Usage fields; others add variants |
| `prompt_context.rs`, `anthropic.rs` | none | WS02 only |
| `deploy/docker-compose.yml` | all | one pass-through line per flag, empty default |

## 6. Cross-session interfaces

**(a) Verevon Space session**
- Shared piece: Space-scoped Do runs. The Space composer sends no `agentic` flag today (overlap-space row 7, `SpaceRoomComposer.tsx:402-420` per that digest), so room agents run only the inline loop. Owner of the composer change: Space session. WS02 assumes that when it starts sending `agentic`, the BFF keeps injecting `space_instructions` after Control membership (`domains/spaces.rs inject_authored_instructions`), and `agentic_space_id` is set (`sse.rs:1411-1416`).
- Memory containment (row 6): Space and agent_run threads can feed user-scope memory extraction. Until the Space session ships the audience/scope predicate, WS02 skips memory on Space-scoped Do runs (D4). Owner: Model Plane/session-core, one migration per the overlap digest (WS07/F-t3-060).
- History in Space threads: v1 uses the owner-bound `list_conversation` path (no shared-Space read decision), same as Ask. A member-triggered run reading other members' messages needs the Space read decision token; not in scope. Owner: Space session.
- Instruction precedence for Space and project layers (OD19): single function `compose_authored_instructions`; Space session must not fork it.

**(b) Quarry v2 / browser session**
- No shared piece. The Do loop's `browser_agent` tool and "Model proposes, Quarry executes" are untouched; `RunContext` carries no grants, credentials or browser state. Assumed from (b): nothing. Conflict watch: both sessions may edit `execution.proto`/`agent.rs`; follow the shared-file rule in section 5. Prompt tiers are not a Quarry contract.

## 7. Regression guards (audit section 8)

| Strength | How WS02 touches it | Gate that proves no regression |
|---|---|---|
| Two loops kept separate | Shares a context contract and tier names only | `execution-core/tests/cross_service_loop_contract.rs`, `compaction_parity_contract.rs`; new `prompt_tier_contract.rs`; no change to `tool_loop.rs` dispatch or `execute_step_inner`; code review rule: no import of gateway code in execution-core |
| ZDR by construction | New persisted state (run record enums), new cache fields, tier plan | S4 `run_context_posture.rs`; S9 `zdr_tiered_request...`; S10 `openai_zdr_body...`; existing `zdr_request_must_not_send_cache_control_anywhere_in_the_body`, `cache.rs:245` ZDR test; `bash scripts/tests/zdr-contract-sync.test.sh` |
| Context management (pins, recall, `reattach_context`) | Do history uses the planner but not the summariser; markers differ | `leading_system_messages_are_never_compacted_by_budget`, `a_failed_summary_degrades_to_truncation_with_an_honest_marker`, eval cases 13-14, `compaction_parity_contract.rs`; D2(c) keeps recovery honest |
| Honest outcomes | Hit rate must not invent numbers; Do cost stays `None` until priced | S2 UI "No cache activity reported"; `cost_usd: None` retained at `sse.rs:9716`; cost-core `unknown_basis` fields |
| Typed ChatEvent taxonomy | Usage payload gains keys | `gen-sse-event-taxonomy.py --check`; `usage_carries_...` tests; byte-verbatim BFF relay untouched |
| Memory provenance | Memory block is the Ask block (`memory_context_block`) | `memory_provenance.rs` tests; no second memory store |
| Multi-tenant server runtime | Run record write is org-scoped like `set_run_mode` | session-core RPC tests; cost-core org-pinned analytics (`pinOrganization`) |

## 8. Effort and risks

**Total:** ~34-50 engineer-days across 11 scheduled slices (S12 unscheduled). Parallel tracks A and B overlap; with two engineers about 4-5 calendar weeks plus S8's measurement window.
**Critical path:** WS01 LIVE-commit, S4, S5, S6, S7 (about 17-25 d), then S8 (flip decision). S1-S3 run in parallel; S9-S11 follow S2's baseline.

| # | Risk | Mitigation |
|---|---|---|
| 1 | Heavier Do prompts raise cost per round before cache levers land; subscription route re-sends uncached. | Flag default off; `instructions` first; pinned route instructions-only (D10); S2/S3 baseline before `full`; S8/S9 follow. |
| 2 | Do-round usage is invisible to cost-core; enabling S3 starts billing. | S3 flag off until D1; deploy order executor then gateway; WS27 consumes the same field. |
| 3 | Merge pressure on LIVE-modified `sse.rs`, `agent.rs`, `sessions.proto`, `execution.proto`. | WS01 commits first; new modules; thin diffs; field reservations (D8); proto edits add-only, `buf breaking` in CI. |
| 4 | Context leakage or authority drift: Space-origin memory, instruction text mistaken for authority, memory as injection channel. | Space memory skipped (D4); instruction text only changes prompt, never `mode`/rung (test); memory block framed as before; ZDR validation at the executor boundary; text never logged. |
| 5 | Cache gains unproven (OpenAI route is a minority; Anthropic split value unknown; `prompt_cache_key` support unknown). | S2 baseline gates S9/S10; S8 A/B with eval parity; every lever behind a default-off flag; per-replica miss reasons labelled advisory. |
