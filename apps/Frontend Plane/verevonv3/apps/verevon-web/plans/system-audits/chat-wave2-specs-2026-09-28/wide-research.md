# Wide-Research Fan-Out — Implementation Spec

Scope: Wave-2 backlog item "raising the subagent ceiling (8 → configurable, with a cost guard)" plus the wide-research UX (live run panel, fan-out tool, source-checking). Repo root `C:/dev/CoresSystem`. All line numbers verified by direct read on 2026-09-28.

## 1. What exists today (file:line evidence)

**Fan-out mechanism (already real, already parallel).**
- `execution-core/src/subagent/mod.rs:27` — `MAX_DEPTH: u32 = 1` (a subagent may not itself delegate).
- `execution-core/src/subagent/mod.rs:55` — `MAX_TOTAL_CHILDREN: u32 = 8`, a hard compile-time constant, independent of the round budget (rationale documented at `:29-54`).
- `execution-core/src/subagent/mod.rs:67-76` — `guard_child_ceiling(started)`, a pure refusal function against the constant.
- `execution-core/src/runtime_loop/agent.rs:979` — `children_started: Arc<AtomicU32>`, run-scoped (not round-scoped) counter, one per `run_agent` invocation.
- `execution-core/src/runtime_loop/agent.rs:1402` — `futures::future::join_all(...)` dispatches every tool call in one round concurrently, `subagent.task` calls included, so if the model emits N `subagent.task` calls in one round they already run in parallel today, bounded only by `MAX_TOTAL_CHILDREN` and by the round-budget pool.
- `execution-core/src/runtime_loop/agent.rs:1671-1757` (`LoopSubagentDispatch::spawn`) — the actual per-call gate: `children_started.fetch_add(1, Relaxed)` (an exclusive claim, not a check-then-act, documented as race-safe at `:1730-1745`) then `shared_rounds_remaining.swap(0, Relaxed)` — the FIRST sibling in a round claims the entire remaining round-budget pool, so a wide fan-out with a small remaining budget will starve most items of rounds. This is the load-bearing mechanic a "configurable ceiling" has to extend without breaking.
- `execution-core/src/runtime_loop/agent.rs:2083-2160` (`run_subagent`) — the child `LoopContext` inherits `req`, tools, ZDR, credentials verbatim; only `depth+1` and `step_prefix` change. No new context is added for "batch/fan-out" grouping.
- `execution-core/src/runtime_loop/agent.rs:1815-1910` (`register_delegated_child_run`) — writes `StartManagedRunRequest{goal, ...}` (durable, has the goal text) then `AttachSubagentRequest{thread_id, parent_run_id, child_run_id, role}` (lineage edge — **no goal field**). `role` is hardcoded to `SUBAGENT_LINEAGE_ROLE = pb::SubagentRole::Generic` (`agent.rs:1791-1799`), even though `SUBAGENT_ROLE_RESEARCHER = 3` already exists (`proto/model_plane/v1/orchestration.proto:254`) and is unused by the in-loop path.
- `proto/model_plane/v1/orchestration.proto:863-875` — `AttachSubagentRequest` has exactly 4 fields: `thread_id, parent_run_id, child_run_id, role`. No goal text, no batch/group id. This is why a subagent's stated task can't be shown on a live card today without an extra fetch.
- **Duplicate tool definition, pre-existing bug**: `execution-core/src/runtime_loop/agent.rs:3183` and `:3226` both define `pb::ToolDefinition{ name: "subagent.task", ... }` with two different description strings, inside the same `offered_tool_defs()` vec (fn starts `:3049`). The model is handed the same tool name twice with conflicting docs. Any new fan-out tool definition must not repeat this pattern, and cleaning the duplicate is in-scope collateral for whoever touches this vec next (flagged here, not fixed — this task is read-only).
- **Single tool shape today**: the only fan-out primitive the model has is `subagent.task` — one goal, one call. There is no schema for "N items, one call, structured per-item results, merged." (`agent.rs:3178-3187` and the identical-name duplicate at `:3221-3229`).

**Cost / budget (the real gap: no guard exists in execution-core at all today).**
- `go/services/cost-core/internal/ledger/ledger.go:259-269` — `Ledger.CheckBudget(orgID, userID, maxCostUSD, maxTokens)`, scoped to **org+user cumulative usage**, not to a single run.
- `go/services/cost-core/internal/ledger/ledger.go:191-221` — `meteredTokens(e)` returns `(0,0)` when `e.CostBasis == CostBasisSubscription`. Combined with `CheckBudget` at `:466-472` comparing only `MeteredInputTokens+MeteredOutputTokens`, **a subscription-served run contributes nothing to either the cost or the token budget check.**
- `rust/services/model-gateway/src/pricing.rs:56-74` (`CostBasis::of_route`) — recognizes `"openai-codex-subscription"`/`"codex-subscription"` provider hints and marks them `Subscription`. Verevon's chosen route for gpt-5.6-terra (memory: "Keep the Terra route") is exactly this basis.
- **Consequence**: on the Terra/subscription route, cost-core's USD and token budgets are structurally toothless for bounding a wide-research fan-out — they were built for metered API spend, not for a free-per-token subscription seat. The only real levers left are breadth (`MAX_TOTAL_CHILDREN`), rounds, wall-clock, and the provider's own rate limit.
- `rust/services/model-gateway/src/budget.rs:1-4,42-73` (`check_budget`) — pre-flight check called from **model-gateway**, per inference round-trip, against `org_id+user_id` aggregate. **Execution-core has no cost-core client anywhere** (confirmed: no `COST_CORE_URL`/`cost_core` reference in `execution-core/src`). Every subagent's own inference calls already pass through this gate indirectly (each child's model call still goes gateway→budget→cost-core), but there is no fan-out-aware preflight ("would spawning 40 more children blow the run's budget") anywhere.
- `rust/services/model-gateway/src/org_quota.rs:23-26,71-95` — org ceilings are keyed `max_cost_per_run_usd_micros` / `max_tokens_per_run` (org-core `org_quotas` table), read by model-gateway, but enforced by cost-core's org+user aggregate — i.e. the keys say "per run" but the enforcement is NOT run-scoped. A genuine per-run cap needs `ledger.GetRunUsage(runID)` (`ledger.go:246-248`, already implemented, exposed at `GET /api/v1/cost/run` — `server.go:122`) or a new `run_id`-scoped variant of `POST /api/v1/budget/check` (`server.go:128`, `handleBudgetCheck` at `:444-540`) — today that handler takes only `org_id`/`user_id` (`server.go:98-104`), no `run_id`.

**Rate limits on the openai-codex-subscription route.**
- `rust/services/inference-core/src/provider/codex_subscription.rs:41-47` — `RATE_LIMITED`, `DEFAULT_RATE_LIMIT_RETRY_MS = 1_000`.
- `:33` — `USAGE_LIMIT = "subscription_usage_limit"`, with `windowMinutes`/`resetsAt` parsed from the body (tests at `:1404-1435`, `:1600-1640` show a real observed reset window of up to 300 minutes and retry-after up to 10.8M ms ≈ 3 hours).
- `:280-370` — `retry_after_header_ms` + per-code mapping to `ProviderError::RateLimited{retry_after_ms}` / `SubscriptionRefused{reason, retry_after_ms, resets_at}`.
- **No concurrency semaphore anywhere in inference-core or model-gateway** for the codex-subscription route (confirmed by search: no `Semaphore`/`max_concurrent` hit in either service's `src`). A wide fan-out today would open as many simultaneous upstream calls as `MAX_TOTAL_CHILDREN` allows, with zero client-side throttling — the only backstop is the upstream's own 429.

**Live run panel / UI.**
- `src/features/chat/components/ChatLiveRunPanel.tsx:1-15` — the existing chat-side live panel is browser-evidence-oriented (screenshots via `BrowserChrome`); it has no notion of parallel subagent cards.
- `src/shared/api/run-console-client.ts:43-53` — `SubagentAttachedEvent{parentRunId, childRunId, role, at}` and `SubagentStoppedEvent{childRunId, status, at}` are the **only** two lifecycle events. No goal text, no batch/group id, no progress-within-run signal.
- `src/features/agents/components/AgentRunConsole.tsx:571-588` — `onSubagentAttached`/`onSubagentStopped` append one timeline row each; `:1866` maps `kind: 'subagent'` to a `Users` icon. This is a flat timeline, not a card grid.
- `src/shared/api/orchestration-client.ts:285-291` (`getLineage`) — a separate "orchestration surface" lineage tree endpoint, `unknown` shape, distinct from the in-loop SSE events (the code comment at `execution-core/src/runtime_loop/agent.rs:6762` notes this split was itself a historical bug: "made GetSubagentLineage return nothing for every real subagent" until fixed).
- Chat's request path to execution-core's durable `RunAgent` goes through `apps/gateway/src/domains/ag_ui.rs` and `apps/gateway/src/domains/chat/shared.rs` (confirmed by grep; the inline `model-gateway/src/tool_loop.rs` chat loop has **no** subagent concept at all — delegation is exclusively a durable-`RunAgent` capability, matching the plan's Wave-1 note that durable-run features are already wired to chat).

**Proto wire contract.**
- `proto/model_plane/v1/execution.proto:76-140` (`RunAgentRequest`) — already has a precedent field, `max_rounds` (uint32, `:99-101`), clamped server-side against `MAX_ROUNDS_CEILING = 32` (`agent.rs:274`, `DEFAULT_MAX_ROUNDS = 12` at `:270`). There is no `max_subagents` field today.

## 2. Change list per file

### 2.1 Configurable breadth ceiling (replaces the hardcoded 8)

**`proto/model_plane/v1/execution.proto`**
- Add to `RunAgentRequest` (after `min_privacy_tier`, next free field number): `uint32 max_subagents = N;` — same "0 = unspecified, use default" convention as `max_rounds` (`:99-101`).
- Regenerate `go/gen/model_plane/v1/execution.pb.go` and the Rust `pb` bindings (build step, not a hand edit).

**`execution-core/src/subagent/mod.rs`**
- Replace `pub const MAX_TOTAL_CHILDREN: u32 = 8;` (`:55`) with:
  - `pub const DEFAULT_MAX_CHILDREN: u32 = 8;` (keep 8 as the unchanged default — no behavior change for a run that doesn't opt in).
  - `pub const MAX_CHILDREN_CEILING: u32 = 100;` (matches Manus-scale ask; hard server-side cap regardless of what org policy or the request asks for).
  - `pub fn resolve_child_ceiling(requested: Option<u32>, org_ceiling: Option<u32>) -> u32` — mirrors `resolve_round_budget` (`:184-200`): `requested.unwrap_or(org_ceiling.unwrap_or(DEFAULT_MAX_CHILDREN)).min(MAX_CHILDREN_CEILING).min(org_ceiling.unwrap_or(MAX_CHILDREN_CEILING))`. Org ceiling always wins if tighter than the request; request can only tighten, never loosen past the org's own cap (same non-negotiable-widening rule the docstring at org_quota.rs:8-11 already establishes for cost).
  - Change `guard_child_ceiling(started: u32)` (`:67-76`) to `guard_child_ceiling(started: u32, ceiling: u32)`, replacing the literal `MAX_TOTAL_CHILDREN` with the parameter; update the refusal string to interpolate `ceiling` instead of the constant.
- **Do not touch `MAX_DEPTH`** — the item is explicitly about breadth (`MAX_TOTAL_CHILDREN`), not nesting; raising depth past 1 multiplies the audit-chain problem the docstring at `:20-26` already argues against, and the plan's owner direction doesn't ask for it.

**`execution-core/src/runtime_loop/agent.rs`**
- `:979` `children_started` stays an `AtomicU32`; add alongside it a `child_ceiling: u32` resolved once per run (from `req.max_subagents` and a new org-ceiling lookup, see below), threaded into `LoopContext` (new field) so `LoopSubagentDispatch` (`:1671-1683`) reads it instead of the constant.
- `:1730` `guard_child_ceiling(claimed)` → `guard_child_ceiling(claimed, self.parent.child_ceiling)`.
- Org ceiling source: reuse the existing `OrgDirectoryClient` pattern execution-core already has for other org policy reads (same client family `org_quota.rs`'s docstring says model-gateway uses) — add a call to fetch a new org-core quota key, e.g. `max_subagents_per_run`, alongside wherever ZDR/privacy-tier org defaults are already fetched for the run. This is new wiring; execution-core does not currently read org-core policy for subagents at all.
- Update the two duplicate `subagent.task` `ToolDefinition` entries (`:3183`, `:3226`) — collapse to one (dedupe is in-scope collateral since this file is being touched anyway for the new fan-out tool below); keep the more detailed description (`:3226`'s, which states the read-only-tools-only contract).

**`go/services/cost-core`** (org-core quota key, not cost-core itself — quota *definition* lives in org-core; cost-core only accounts spend). Add `max_subagents_per_run` as a recognized org-core quota key (org-core is out of this task's listed services but is the source of truth per `org_quota.rs:1-11`'s own doc comment — flag as a dependency, see §7).

### 2.2 Cost guard

**`go/services/cost-core/internal/server/server.go`**
- Extend `budgetCheckRequest` (`:98-104`) with an optional `run_id string \`json:"run_id,omitempty"\`` field.
- In `handleBudgetCheck` (`:444-540`), when `run_id` is present, call `s.ledger.Aggregate(ctx, ledger.AggregateFilter{RunID: req.RunID})` instead of `GetUsage(orgID, userID)`, and compare against the request's caps the same way `budgetDecision` (`:568-575`) already does. This makes a genuine per-run budget possible for the first time — today's endpoint literally cannot express it.
- `go/services/cost-core/internal/server/server_test.go` gets new cases (see §3).

**`execution-core/src/cost_guard.rs`** (new file — execution-core currently has no cost-core client of any kind)
- A small HTTP client mirroring `model-gateway/src/budget.rs`'s shape: `check_run_budget(http, cost_core_url, run_id, org_id, user_id, max_cost_usd, max_tokens) -> Result<(), CostGuardRefusal>`.
- Called from the new fan-out tool's dispatch path (§2.3), **before** expanding to N children, using an **estimated** cost: `items.len() as f64 * average_cost_per_item`, where `average_cost_per_item` is a static per-model estimate (a few cents/typical-tokens for a metered model; **for `CostBasis::Subscription` routes this check is a no-op by construction** — see §5 risk — so the binding guard on Terra is the breadth ceiling and a new concurrency limiter, not this cost check).
- Fails closed the same way `model-gateway/src/budget.rs`'s doc comment (`:36-38`) mandates: transport/auth failure → refuse, never silently allow.

### 2.3 The fan-out tool itself

**`execution-core/src/subagent/research_fan_out.rs`** (new file, sibling to `subagent/mod.rs`)
- `pub const RESEARCH_TOOL_NAME: &str = "subagent.research_fan_out";`
- `parse_fan_out(tool_input: &str) -> Result<FanOutRequest, String>` — schema:
  ```json
  {
    "items": [
      { "id": "optional-stable-id", "goal": "self-contained research question for this item" }
    ],
    "max_rounds_per_item": 6,
    "result_schema_hint": "optional free-text hint on what shape each item's answer should take"
  }
  ```
  `items` required, 1..=`child_ceiling` entries (validated against the SAME ceiling as individual `subagent.task` calls — one fan-out call still counts every item against `children_started`, so a run cannot bypass the breadth ceiling by using one big fan-out call instead of N `subagent.task` calls).
- `pub struct FanOutItemResult { id: String, goal: String, status: ItemStatus /* Ok | Failed | Refused */, answer: Option<String>, sources: Vec<SourceRef>, error: Option<String> }` — `sources` is populated from whatever the child's own `web_tools`/`knowledge_search` grounding produced (the child already runs the normal loop with the normal tools; this just requires the merge step to ask the child, or parse its final tool-context, for source URLs rather than only prose — see the merge-step note below).
- `merge(results: Vec<FanOutItemResult>) -> String` — produces the single tool-output string handed back to the parent model: a structured digest (one paragraph per item, `id`, status, answer, and a numbered source list), NOT a raw concatenation — this is what lets the parent model's final answer cite per-item sources instead of one undifferentiated blob, and is what the existing "Verification notice" UI (plan §2, `ChatMessages.tsx:2096-2168`) can key off if the merge step also emits a machine-readable `verification: {checked: N, confirmed: M}` summary line.

**`execution-core/src/runtime_loop/agent.rs`**
- Add `pb::ToolDefinition{ name: "subagent.research_fan_out", description: "...", parameters_json: ... }` to `offered_tool_defs()` (same list as the deduped `subagent.task`), gated the same as `subagent.task` (offered only where delegation is already offered — reuse whatever flag currently guards `subagent.task`'s presence).
- `LoopSubagentDispatch::spawn` (`:1730-1757`) branches on `tool_name`: `is_subagent_tool` already prefix-matches `subagent.`, so `subagent.research_fan_out` is dispatched through the SAME entry point (`execute_step_with_subagent`) with a NEW arm in `subagent::mod.rs`'s dispatch logic that, instead of running one `run_subagent`, runs `futures::future::join_all` over `items.len()` calls to `run_subagent` — reusing `run_subagent` (`:2083-2160`) verbatim per item, each with `role = SubagentRole::Researcher` (finally using the existing-but-unused enum value, `orchestration.proto:254`) instead of the hardcoded `Generic`.
- Each item's `children_started.fetch_add` claim and `guard_child_ceiling` check happens per-item inside that inner `join_all`, so partial fan-out (e.g. ceiling hit at item 6 of 10) degrades to "6 ran, 4 refused with the real ceiling reason", not an all-or-nothing failure — the merge step reports refused items as `status: Refused` with the model-readable reason from `guard_child_ceiling`'s message.

**`capability_policy.rs`**
- `trusted_capability_id` (`:740-745`) already matches on the `subagent.` prefix generically, so `subagent.research_fan_out` maps to `cap.agent.spawn` with zero changes needed here — confirmed by the prefix check at `:742`.

### 2.4 Lineage / SSE: carry the goal and a batch id

**`proto/model_plane/v1/orchestration.proto`**
- Add to `AttachSubagentRequest` (`:863-875`): `string goal_summary = 5;` (truncate at the same `MAX_RESULT_CHARS`-style bound `integration_tools.rs:47` uses, e.g. 200 chars) and `string batch_id = 6;` (empty for a standalone `subagent.task`, set to the fan-out call's own step_id for every sibling spawned by one `research_fan_out` call).
- Mirror on `LineageEdge`/`SubagentLineage` if those messages persist role/goal (check at implementation time; not read in this pass).

**`execution-core/src/runtime_loop/agent.rs`**
- `register_delegated_child_run` (`:1815-1910`) gains a `batch_id: Option<&str>` and `goal` truncation before building `AttachSubagentRequest`; the fan-out dispatch path (§2.3) passes its own step_id as `batch_id` for every item, `run_subagent`'s existing solo-call path passes `None`.

**`src/shared/api/run-console-client.ts`**
- Extend `SubagentAttachedEvent` (`:43-48`) with `goalSummary?: string` and `batchId?: string`; parse them in the same handler block that currently reads `role`/`childRunId` (`:238-244`).

### 2.5 Frontend: the live run panel

**`src/features/chat/components/ChatWideResearchPanel.tsx`** (new, sibling to `ChatLiveRunPanel.tsx`)
- Groups `SubagentAttachedEvent`s sharing one `batchId` into a card grid: one card per item, `goalSummary` as the title, a status pill (`queued`→`running`→`done`/`failed`/`refused`) driven by pairing each `SubagentAttachedEvent` with its later `SubagentStoppedEvent` (`childRunId` is the join key, already present at `:50-53`).
- Reuses the existing honest-state pattern from `src/features/chat/lib/chat-run-watch.ts` (`screenshotStateFor`-style: never show an unresolvable spinner — a card whose stop event never arrives before the run ends shows "did not finish" not a perpetual spinner).
- Mounted from `ChatPanels.tsx` alongside `ChatLiveRunPanel`, switched in whenever the current run's timeline contains ≥2 items sharing a `batchId` (i.e. it only appears for an actual wide-research call, not for a single delegated `subagent.task`).

**`src/features/agents/components/AgentRunConsole.tsx`**
- `onSubagentAttached`/`onSubagentStopped` (`:571-588`) gain the same `batchId`/`goalSummary` fields on the appended timeline row's `detail` text, so the flat console view also shows the goal instead of just a bare run id — small, additive change, no new kind needed.

## 3. Tests to add

**Rust — `execution-core/src/subagent/mod.rs`** (extend the existing `#[cfg(test)] mod tests`, `:203-323`):
- `resolve_child_ceiling_prefers_the_tighter_of_request_and_org_ceiling` — request > org ceiling → org wins; request < org ceiling → request wins; neither set → `DEFAULT_MAX_CHILDREN`.
- `resolve_child_ceiling_never_exceeds_the_hard_ceiling` — request/org both `u32::MAX` → clamped to `MAX_CHILDREN_CEILING`.
- `guard_child_ceiling_reports_the_configured_ceiling_not_a_constant` — refusal text interpolates the passed `ceiling`, not a hardcoded 8.

**Rust — `execution-core/src/subagent/research_fan_out.rs`** (new test module):
- `parse_fan_out_accepts_one_to_n_items_and_rejects_empty` — mirrors `task_parsing_accepts_goal_or_task_and_rejects_an_empty_delegation` (`subagent/mod.rs:271-298`).
- `parse_fan_out_rejects_more_items_than_the_run_ceiling` (parse-time bound check, before any dispatch).
- `merge_reports_partial_failure_honestly` — 10 items, ceiling hit at item 6 → merged output lists 6 `Ok`/`Failed` and 4 `Refused` with the real ceiling reason, never silently drops the refused ones.
- `merge_preserves_per_item_sources_not_a_flattened_blob` — two items each with distinct `SourceRef`s → merged text attributes each source list to its own item id.

**Rust — `execution-core/src/runtime_loop/agent.rs`** (extend existing test module, pattern at `:2671-2673`, `:2980`, `:6374`):
- `two_concurrent_delegations_via_research_fan_out_share_the_same_round_budget_pool` — analogous to the existing sibling test named in the doc comment at `:1698-1704`, but through the new fan-out tool.
- `research_fan_out_items_are_recorded_with_researcher_role_and_a_shared_batch_id` — asserts the lineage edge role is `SUBAGENT_ROLE_RESEARCHER`, not `Generic`, and that all N edges share one `batch_id`.
- `offered_tool_defs_has_no_duplicate_tool_names` — a regression test for the pre-existing duplicate `subagent.task` entry found in this review (`:3183`/`:3226`); asserts every `name` in `offered_tool_defs()` is unique.
- `cost_guard_is_a_no_op_for_subscription_basis_and_binding_for_metered` — the risk from §1/§5, made explicit: a fake cost-core client returning "over budget" still allows a `Subscription`-basis run through (documenting, not silently accepting, the limitation) while refusing a `Metered`-basis one.

**Go — `go/services/cost-core/internal/server/server_test.go`**:
- `budget_check_with_run_id_scopes_to_the_run_not_the_org_user_pair` — two entries same org+user, different `run_id`; a run-scoped check on one run's spend passes even though the org+user aggregate would fail.
- `budget_check_without_run_id_keeps_the_existing_org_user_behavior` (regression, confirms no behavior change for existing callers like model-gateway).

**Go — `go/services/cost-core/internal/ledger/ledger_test.go`**:
- Confirm (as a documented, asserted behavior, not just implicit) that `Aggregate` with `AggregateFilter{RunID: ...}` correctly rolls up cost/tokens per run — this already works per `matches()` (`ledger.go:505-524`) but has no existing test pinning it for the budget-check use case specifically.

**Frontend — `src/shared/api/run-console-client.test.ts`**:
- Parses `goal_summary`/`batch_id` (snake_case wire) into `goalSummary`/`batchId` (camelCase), same defensive-parse convention the file already documents (`:9-13`).

**Frontend — new `src/features/chat/components/ChatWideResearchPanel.test.tsx`**:
- Renders N cards from N `SubagentAttachedEvent`s sharing a `batchId`.
- A card transitions `running`→`done` when its matching `SubagentStoppedEvent` arrives (joined by `childRunId`).
- A card that never receives a stop event before the run's own terminal status shows "did not finish", never a perpetual spinner (mirrors the existing fallback contract in `chat-run-watch.ts`).
- Only mounts when ≥2 timeline items share one `batchId` (a lone `subagent.task` delegation still renders via the existing `ChatLiveRunPanel`/`AgentRunConsole` path, unchanged).

## 4. Rollout

- **Rebuild required**: `execution-core` (Rust), `cost-core` (Go), the frontend `verevon-web` container, and the `gateway` BFF only if the SSE payload's snake_case→camelCase mapping in the gateway needs to pass the two new fields through untouched (likely a no-op forward, per the existing comment at `run-console-client.ts:9` — "gateway forwards the model-gateway/session-core/execution-core events verbatim"). Per project convention, build images via the launcher (`docker compose build` by hand misses the Control Plane env chain).
- **Proto regeneration**: `execution.proto` and `orchestration.proto` changes require regenerating both the Go (`go/gen/model_plane/v1/*.pb.go`) and Rust bindings before either service builds — this is a cross-plane build-order dependency (Rust `pb` crate must regenerate before `execution-core` compiles against the new `AttachSubagentRequest.goal_summary`/`batch_id` fields).
- **session-core**: not in this task's listed Rust services but is the actual server of `OrchestrationCoreService`/`AttachSubagent` (confirmed at `agent.rs:1888-1895` — the RPC target is `session_channel`, i.e. session-core) — it must also rebuild to accept and persist the two new `AttachSubagentRequest` fields, and its `StreamRunEvents` SSE serializer must include them on the wire. **This is a hidden fourth service in the rollout, not listed as in-scope by the harness's service list — flag to the owner before starting.**
- **org-core** (Control Plane, also outside this task's listed planes): must recognize a new `max_subagents_per_run` quota key the same way it already recognizes `max_cost_per_run_usd_micros`/`max_tokens_per_run` (`org_quota.rs:24-26`). Another cross-plane dependency to flag.
- **Flags**: none strictly required — `max_subagents` on the request defaults to 0 (→ `DEFAULT_MAX_CHILDREN=8`, today's behavior unchanged) and the new fan-out tool is additive (offered alongside, not instead of, `subagent.task`). A kill switch is still worth adding: an env var on execution-core (`WIDE_RESEARCH_FAN_OUT_ENABLED`, default `true` post-launch) so the tool can be un-offered without a rebuild if the codex-subscription route gets rate-limited in practice.
- **Sequencing**: land in this order to keep each deploy independently testable: (1) cost-core `run_id`-scoped budget check (additive, no caller changes) → (2) proto changes + session-core/execution-core rebuild for the configurable ceiling alone (no new tool yet, `subagent.task` unchanged) → (3) the `research_fan_out` tool + frontend panel, once the ceiling and lineage plumbing are proven live.

## 5. Risks and edge cases

- **The cost guard is a no-op on the Terra/subscription route.** Because `CostBasisSubscription` entries score 0 on both cost and metered tokens (`ledger.go:216-221`), a USD/token "cost guard" cannot bound a wide-research run served by gpt-5.6-terra via openai-codex-subscription — the only real guards there are the breadth ceiling and a concurrency limiter (not yet designed — see below). The spec above builds the cost-guard machinery mainly for a future metered route; document this limitation prominently in the tool's own description string so an implementer doesn't believe it's protecting the Terra route when it isn't.
- **No concurrency limiter exists for the subscription route at all.** `MAX_TOTAL_CHILDREN` bounds total children per run, not *simultaneous in-flight* children. Raising the ceiling to, say, 40 and letting one `research_fan_out` call claim all 40 at once means up to 40 simultaneous upstream calls against one ChatGPT-subscription seat — realistically well past any sane per-account concurrency limit, likely to trigger `subscription_usage_limit` (observed reset windows up to 300 minutes, `codex_subscription.rs:1412`) for the *whole org*, not just this run, since the ledger's `RateLimited`/`SubscriptionRefused` handling is per-adapter-instance, not per-run. **Recommend a client-side `tokio::sync::Semaphore` inside the fan-out dispatch (a genuinely new piece, not present anywhere in inference-core/model-gateway today), sized conservatively (start at 4–6 concurrent) and exposed as a second, separate config knob from the total-children ceiling** — exactly the same two-knobs-not-one argument `subagent/mod.rs:29-46` already makes for rounds vs. breadth, applied one level deeper (breadth vs. concurrency).
- **Round-budget starvation under wide fan-out.** `LoopSubagentDispatch::spawn`'s `swap(0, ...)` (`:1730-1745`) gives the entire remaining round pool to whichever sibling's `spawn` executes first. With 8 children this was tolerable; with 40–100 the first-mover-take-all behavior means most items get 0 rounds and fail immediately unless the round budget is also raised (and the request's `max_rounds` ceiling, `MAX_ROUNDS_CEILING=32`, may need reconsidering alongside the children ceiling — these two existing constants were sized for 8 children, not 100). This needs either (a) a per-item round floor reserved before the pool is divided, or (b) splitting the pool evenly across items known-up-front (the `research_fan_out` call, unlike ad hoc `subagent.task` calls, DOES know N up front — `items.len()` — so it can pre-divide `remaining/N` rather than winner-take-all; this is a real design choice, not just an estimate — see owner decision list).
- **`role` defaulting to `Generic` for years of prior lineage data** means any UI logic that filters by `SUBAGENT_ROLE_RESEARCHER` will show nothing for pre-change runs — cosmetic, not a correctness bug, but worth a note in the panel's empty state.
- **The duplicate `subagent.task` tool definition** (`agent.rs:3183`/`:3226`) — if the model-facing tool array is ever turned into a lookup by name (e.g. a future refactor), the second entry silently wins or a panic occurs on `HashMap` construction from a `Vec` with duplicate keys — currently harmless because it's consumed as a linear `Vec`, but it is exactly the kind of latent trap this task's new tool-definition edit sits next to.
- **A goal string leaking into `AttachSubagentRequest.goal_summary`** is now duplicated data (already durable via `StartManagedRunRequest.goal`) flowing over the run-events SSE stream — for a ZDR run, confirm this truncated goal text is still subject to the same ZDR non-retention posture as the rest of the stream (the existing `zdr` flag already gates other SSE content per the plan's "server-enforced privacy, ZDR and residency router" — verify the new field is threaded through that same enforcement point, not bolted on separately).
- **Norwegian-first UI**: every new user-facing string in `ChatWideResearchPanel.tsx` and the fan-out tool's own description (which the model may quote back to the user) must go through `i18n.tr('norsk', 'English')` — the existing panel components (`ChatLiveRunPanel.tsx:129,143-150`) are the pattern to copy exactly.

## 6. Genuine owner decisions

1. **Round-budget division policy for a known-N fan-out**: pre-divide the remaining round pool evenly across the N declared items (fair but caps each item's depth), or keep today's first-claim-wins semantics (simpler, but starves later items under wide fan-out). This changes the design, not just a parameter.
2. **Concurrency ceiling default for the subscription route**: a hardcoded conservative default (e.g. 4–6 simultaneous children) is a real product/cost tradeoff against wall-clock latency for a 100-item wide-research call — worth an explicit number from the owner rather than an engineer's guess, especially since it directly trades off against the "latency targets" the item itself asks for.
3. **Whether a wide-research call should be offered under `ask` posture without per-item approval**, given `subagent.task`'s existing contract already forbids any side-effecting action inside a delegated subagent (`agent.rs:3227` description) — if that invariant holds for `research_fan_out` too (recommended: yes, it's read/analysis-only by the same rule), no new approval gate is needed; if the owner wants a single approval covering the whole batch, that's new UI (a "approve this 40-item research plan" gate) not covered by this spec.

## 7. Size and dependencies

**Size: L.** Breakdown: configurable ceiling + org-quota plumbing (M, touches 3 planes: org-core, execution-core, model-gateway's existing quota-read pattern to copy), cost-core run-scoped budget endpoint (S, additive), the `research_fan_out` tool + merge step (M, new file + new dispatch arm), lineage/proto goal+batch fields (S but touches session-core, a 4th service outside this task's declared scope), frontend panel (M, new component + event-shape extension).

**Dependencies on other Wave-2 items**:
- Shares the lineage/SSE plumbing with **"background task with completion notice"** (Wave 2/3) — both need session-core to carry richer per-run metadata over the event stream; sequencing these two together avoids touching `StreamRunEvents`' serializer twice.
- The **"editable plan gate"** item (Wave 2) and this item both touch the model's mid-run tool vocabulary presented in `offered_tool_defs()` — worth landing in the same PR wave to do the duplicate-`subagent.task` cleanup once, not twice.
- Depends on **org-core** and **session-core** rebuilds that are outside the three services this task's harness explicitly named (model-gateway, execution-core, inference-core, session-core were named — session-core is in scope; org-core is Control Plane and is NOT — flag this cross-plane dependency to the owner before scheduling implementation).