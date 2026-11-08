# WS27-cost-meter - Live run-cost meter and pre-run estimate

Conventions. W = worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928). L = live tree `C:/dev/CoresSystem` (same HEAD plus uncommitted work). Paths are repo-relative; `MP` = `apps/Model Plane`, `MG` = `MP/rust/services/model-gateway/src`, `IC` = `MP/rust/services/inference-core/src`, `EC` = `MP/rust/services/execution-core/src`, `CC` = `MP/go/services/cost-core`, `FE` = `apps/Frontend Plane/verevonv3`. Line numbers are W unless marked `[L]`. "Plan I16" (card 760) was not found in the audit's `final/` or `cons/` folders: this design is built from the card text and the code, not from I16.

## 1. Scope, items resolved, owner

**Item:** F-t3-028 (Partial, L, W2). **Owner:** Model Plane (model-gateway meter and frames, inference-core routing note, execution-core Do-round events, cost-core window), with Control Plane quota semantics (org-core, billing-core read-only) and Frontend Plane (SPA meter + BFF action contract). One meter, one ledger: the meter lives in model-gateway; cost-core stays ledger of record (`MP/docs/capability-ownership-matrix.md` row 74; `apps/master-ownership-matrix.md` line 88).

**IN**
- A per-turn / per-run cumulative meter that counts EVERY provider call of a chat turn (today only the final answer call is counted, section 2.1), prices it honestly, and ships it as a new `usage_progress` frame after each call plus a corrected, cumulative final `usage` frame.
- Cache fields and cost by token category (uncached input, cache read, cache write, output) on the wire. Reasoning tokens are NOT a separate category (providers count them inside output; card 760).
- A live meter in the Work panel and Run Console; warnings at 80 percent and 100 percent of the effective per-run ceiling; the silent budget downgrade made visible in the same warning.
- ZDR turns: cost computed in-stream only, nothing persisted.
- Do-run per-round progress over the existing orchestration event path.
- Quota readout for members (existing BFF route, new SPA client + action contract) and an "at most" upper bound on the plan card.

**OUT (deferred, named)**
- Cost attribution by space/agent/tool and thread totals (card 760; Space session owns the dims, section 6). Image-generation metering (audit 10.1 risk 5), embeddings/rerank prices (ledger OD-PRICES), persisting ZDR metering to the ledger (`DEF-Z-06b`, `MP/docs/decisions/ledger.md:832`).
- Per-user allowance (needs Control billing-core support; billing-core allowances are org-scoped, `apps/Control Plane/billing-core/internal/http/server.go:62-69` per sweep).
- A statistical pre-run estimate (needs the meter's own data; only a labelled upper bound ships, S11).
- Cache hit-rate analytics and Do usage into the ledger: WS02 S2/S3 own those.

| Slice | Tag | Title |
|---|---|---|
| S0 | [FINISH] | Land-first, ADR, characterisation tests |
| S1 | [FINISH] | Honest pricing primitives and the pure `UsageMeter` |
| S2 | [FINISH] | Count tool rounds and overhead calls; cumulative final frame |
| S3 | [BUILD] | `usage_progress` frame, gateway to SPA parse |
| S4 | [BUILD] | SPA run-cost meter (Work panel, Run Console) |
| S5 | [FINISH] | ZDR in-stream cost |
| S6 | [BUILD] | Per-run ceiling resolution, 80/100 percent warnings, optional enforcement |
| S7 | [BUILD] | Surface the silent budget downgrade (`RouteNote`) |
| S8 | [FINISH] | Windowed org budget (honour `reset_period`) |
| S9 | [BUILD] | Do-run per-round progress |
| S10 | [BUILD] | Member quota readout |
| S11 | [BUILD, gated] | Plan-card upper bound |

## 2. Verified current state

### 2.1 What runs today (code wins over the audit)

| Fact | Evidence |
|---|---|
| `ChatEvent::Usage` has no cache fields; keys `input_tokens`, `output_tokens`, `cost_usd` (always written, null when unknown), `latency_ms`, `confidence`, optional `cost_basis`, `served_by`. Family `usage` (with `result_receipt`, `turn_timing`). | `MG/sse_events.rs:228-244`, `:361-363`, `:515-540` |
| The SPA parses `cache_read_tokens`/`cache_write_tokens` but `onUsage` copies only tokens, latency, cost, basis, servedBy, confidence; the "Prompt cache" metric is hidden (not zero) when both are 0, inside a closed "Technical activity" disclosure. | `FE/src/shared/api/chat-client.ts:323-339`, `:1355-1367`; `FE/src/features/chat/components/use-chat-controller.ts:3187-3206`; `FE/src/features/chat/components/ChatPanels.tsx:908-941` |
| **Tool-decision rounds are never metered.** `run_tool_rounds_for_model` calls `client.infer` per round (`MG/tool_loop.rs:6677`), reads `resp` (`:6699-6709`) and only `tracing::debug!`s the tokens (`:6823-6831`). `ToolRounds` has no usage field (`:4565-4613`). The usage envelope and the Usage frame are built once from the final streamed chunk (`MG/sse.rs:3177-3198`, `:3987-4001`, `:4032-4041`, `:4112-4120`). So a tool-heavy turn under-reports tokens and cost in the ledger AND the frame. The audit says only "fires once at turn end"; it is worse. | as cited |
| Other provider calls in a chat turn that bypass the meter (classification of each not determined, enumerated by grep): `MG/sse.rs:7551`, `:7782` (verification judge), `:7811`, `:7898`, `MG/result_validation.rs:62,66`, forced web search `MG/tool_loop.rs:5157`. | grep `.infer(` / `.infer_stream(` |
| `build_usage_envelope` carries `request_id` but **no `run_id`**; cost-core already decodes `run_id` (`CC/cmd/main.go:90`). So chat-turn ledger rows have an empty `run_id` and `GET /api/v1/cost/run` (`CC/internal/server/server.go:369-407`) is empty for chat. `session_run.run_id` is in scope at the emitter (`MG/sse.rs:1991`). | `MG/sse.rs:9792-9847` |
| Cost: `PricingCache::cost_usd_for` returns `Some(0.0)` for subscription, `None` when the catalogue is cold or pricing disabled. **But `lookup` falls back to the catalogue's `default` row for any unknown model** (fixture 3.0/15.0 per million), so an unknown model gets a confident, wrong cost. The ledger records "default ~25x too high for embeddings" as a known hazard. Gemini turns would be priced from this row (audit 10.1 risk 5). | `MG/pricing.rs:213-262`, `:337-359`; `MP/docs/decisions/ledger.md:824-829` |
| Cache legs are priced correctly: `cost_from_rate` re-prices read and write legs and the sum of the four categories equals the total (identity: uncached x in + read x in x cr + write x in x cw + out x out-rate). | `MG/pricing.rs:314-333` |
| **Budget semantics mismatch (audit is silent on it).** The org ceiling key is `max_cost_per_run_usd_micros` and `max_tokens_per_run` (`MG/org_quota.rs:23-26`), but cost-core's `handleBudgetCheck` compares the cap against **accumulated** org or org+user totals: no time filter, no run filter (`CC/internal/server/server.go:585-614`; `CC/internal/postgres/store.go:195-218` sums all rows). `org_quota.rs` reads only `key` and `limit` from the quota row (`:59-63`), so org-core's `reset_period` (`apps/Control Plane/org-core/internal/org/types.go:33-41`) is never honoured. Effect: a "per run" cap behaves as a lifetime cap on the user; the audit's "quotas with reset period" holds only for display. `AggregateFilter` already has `Since`/`Until` (`CC/internal/ledger/ledger.go:239-246`). | as cited |
| Budget gate on the chat path: after the ZDR branch, `fetch_org_limits` (fails open) then `check_budget` (fails closed, 402/503). A request-supplied cap wins, the org ceiling fills unset dimensions. `max_tokens` counts metered tokens only. | `MG/sse.rs:1085-1118`; `MG/budget.rs:48-80`; `MG/org_quota.rs:112-157`; `CC/internal/server/server.go:605-612` |
| The ZDR branch returns before the budget gate (`MG/sse.rs:976`), emits no envelope, and states no cost: `zdr_answer_stream` builds Usage with `cost_usd: None, cost_basis: None` and the comment "Metering of ZDR turns is the deferred Z-06b task" ([L] `MG/sse.rs:8521-8538`; `DirectAnswer` has tokens only, `MG/sse.rs:7670-7681`). ZDR.md names the gap (`MP/docs/ZDR.md:102`). The audit's "zdr ledger envelopes are suppressed" is imprecise: no envelope exists on that path at all; cost-core would also reject one (`CC/cmd/main.go:492-493`). | as cited |
| The Do (agentic) path emits a Usage frame with tokens 0, cost null, basis None, and a stale comment claiming execution-core's envelopes carry the cost. | `MG/sse.rs:9705-9737` |
| The silent downgrade: `BudgetPosture::Constrained` at `>= constrained_fraction (0.8) x cap` routes one tier cheaper, `Exhausted` forces `cheap_fallback`; `Decision.posture` is only logged. `budget_cap_usd` defaults to 50.0. The decision is made in `resolve_intent` before the provider call and never reaches the response. | `IC/provider/intent.rs:176-218`, `:236-269`; `IC/provider/fallback.rs:1011-1041`; `IC/provider/routing_policy.rs:130-131` (per sweep) |
| Two budget authorities with different failure semantics (gateway fail closed; inference-core `BudgetClient` 400 ms, falls back to `Unknown`). | `MG/budget.rs:157-165`; `IC/provider/intent.rs:186-188` |
| Do rounds log per-round usage (`EC/runtime_loop/agent.rs:1450-1463`) but `RunAgent` is unary (`MP/proto/model_plane/v1/execution.proto:34`) and `RunAgentResponse` has no usage (WS02 S3 adds field 9). A precedent for live Do events exists: `publish_plan_transition` -> session-core `RecordOrchestrationEvent` (broadcast + replay, persisted only for browser events) -> gateway `follow_agentic_run` maps events to SSE through a synchronous closure. | `EC/runtime_loop/agent.rs:4909-4949`; `session-core/src/orchestration_grpc.rs:3192-3232`; `MG/sse.rs:9020-9046`, `:9066-9100` |
| Member-readable quota exists but is unused: BFF `GET /api/v1/billing/quotas/{metric}` needs only the authorised org (no billing-admin), returns `QuotaStatus{limit,used,remaining,is_exceeded,utilization}`. The allowance route requires `require_billing_admin`. No SPA caller of either quota route. | `FE/apps/gateway/src/domains/billing.rs:55-65`, `:120-135`; `apps/Control Plane/billing-core/internal/http/handlers.go:178-193`; `.../billing/types.go:56-64`; `.../billing/service.go:845-873` |
| SPA says cost is "never estimated" (`ChatMessages.tsx:1726` per sweep): any estimate must be labelled and never stored in `costUsd`. | sweep |

### 2.2 [live-only] and must-land-first

| Live path | State | Why it matters |
|---|---|---|
| `MG/sse_events.rs` | M | `Usage` gains `retention_class` ([L] `:283-285`). S3/S2 edit the same variant. |
| `MG/sse.rs` (+781) | M | ZDR posture wiring; Usage sites move to [L] `:4242`, `:6983`, `:8525`, `:10077`. |
| `MG/zdr_features.rs`, `zdr_posture.rs`, `tests/zdr_posture_matrix.rs` | `??` / present | The `usage` row says "No cost is stated" ([L] `zdr_features.rs:169-172`): S5 changes it. |
| `FE/src/shared/api/chat-client.ts` (+52), `ChatMessages.tsx` (+181), `use-chat-controller.ts` (+4), `AgentRunConsole.tsx` | M | S3/S4 hunks land in files with uncommitted edits. |
| `MP/docs/sse-event-taxonomy.md` | M (+4) | At HEAD `gen-sse-event-taxonomy.py --check` fails (WS02 2.3). |

**Rule:** WS01 S0 commits the ZDR wave first; every MG/FE slice here branches from that commit. cost-core, billing-core, org-core, `budget.rs`, `org_quota.rs`, `pricing.rs`, inference-core `intent.rs` are clean in L (no landing needed).

### 2.3 Audit staleness
1. "Usage fires once at turn end" omits that the one frame covers only the answer call; tool rounds are unmetered in ledger and frame.
2. "Per-run aggregate in the ledger" is true for the table but empty for chat turns (no `run_id` in the envelope).
3. "Quotas with reset period" does not apply to enforcement (section 2.1).
4. "The Prompt cache metric stays at zero": hidden, not zero (WS02 2.4 agrees).
5. `routing_policy.rs:126-134` holds the 0.8 constant; the downgrade is in `intent.rs:211-216`.

## 3. Decisions and open questions

| ID | Decision | Options | Recommendation | Gates | Proceeds without |
|---|---|---|---|---|---|
| D1 | Meaning of `max_cost_per_run_usd_micros` / `max_tokens_per_run` (Control quota, Model enforcement) | (a) per-run, enforced by the meter; (b) keep lifetime | (a) for the meter and warnings; fix the lifetime behaviour separately in S8 with `reset_period`. Owner: Control + Model lead. Ledger entry in S0. | S6 enforcement, S8 | S1-S5, S6 warn-only |
| D2 | Windowed org budget in cost-core (S8) | (a) honour `reset_period` via `since`; (b) leave lifetime | (a). Behaviour change: orgs capped on lifetime spend are unblocked at the next window. Control confirms. | S8 | all others |
| D3 | Unknown-model price | (a) keep default row, label it; (b) cost null in frames | (b) for frames ("cost null when unknown"), plus `cost_source` in the ledger path later; cost-core `CostWithCache` default unchanged in this stream (flag drift in ledger). | S1 | S0, S2 build |
| D4 | ZDR in-stream cost: reverses the "no cost is stated" rule in `zdr_features.rs` (L) and narrows the ZDR.md:102 gap. Persistence stays `DEF-Z-06b`. | (a) state cost in-stream; (b) keep silent | (a): numbers only, no content, nothing stored. Needs a ledger entry ("ZDR turns state an ephemeral cost; the ledger still sees none"). Owner: ZDR/Control. | S5 | all others |
| D5 | Billing effect of metering tool rounds: cost-core enqueues `cost_billing_outbox` for metered entries (`CC/internal/postgres/store.go:187` per WS02), so S2 raises billed usage. Same family as WS02 D1. | (a) record and bill; (b) record non-billable basis | (a) behind a flag default off; Control confirms attribution first. | S2 **enablement** | S2 build, S3, S4 |
| D6 | Behaviour at 100 percent of the ceiling | (a) stop starting new tool rounds, answer with what exists, notice; (b) hard stop | (a), flag default off. Precedent: the phase deadline ends the tool phase (`MG/tool_loop.rs:6690-6693`). | S6 enforce | S6 warn-only |
| D7 | Final `usage` frame becomes turn-cumulative (adds `calls`). SPA must replace, not add. | accept / new frame only | accept; version noted in the taxonomy. | S2 | - |
| D8 | Proto tags: `InferChunk` 16 / `InferResponse` 15 (WS11 left applied-effort for WS27); `OrchestrationEvent` oneof 27 (WS03 uses 24/25, WS15 proposes 26). Integrator reserves. | first merge wins / reserve | One shared `RouteNote` message so WS11 applied-effort and WS27 downgrade share a single field. | S7, S9 | S0-S6, S8 |
| D9 | Which billing metric member readout shows | name not determined (policies are free-form) | Org-configured token metric named in an env/setting read by the BFF; per-user stays Control backlog. | S10 | S0-S9 |

Nothing here reverses a `docs/decisions/ledger.md` entry silently. Explicit amendments, all recorded in S0: ZDR "no cost stated" (D4); lifetime budget semantics (D1/D2). Ledger has no cost-meter entry (sweep).

## 4. Slices

Common commands (repo root; on Windows use `--lib` where a target fails pre-existing, per memory):
- `cd "apps/Model Plane/rust" && cargo fmt --all -- --check && cargo test -q -p model-gateway --lib` (also `-p inference-core --lib`, `-p execution-core --lib`, `-p session-core --lib`)
- `cd "apps/Model Plane/go/services/cost-core" && go test ./... && go vet ./...`
- `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` after every ChatEvent change; WS12 golden: `MP_UPDATE_GOLDEN=1 cargo test -p model-gateway --lib sse_event_payload_golden` when WS12 S7 exists (second lander regenerates).
- `cd "apps/Frontend Plane/verevonv3" && pnpm vitest run <files> && pnpm typecheck && pnpm lint`; `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`; `node --test apps/gateway/scripts/action-surface-contract.test.mjs`.
- Proto: `buf lint` (pre-existing debt), `buf breaking "apps/Model Plane/proto" --against "apps/Model Plane/proto/baseline.binpb"` must pass; regenerate Go bindings (buf image) and force a Rust proto rebuild (`mp-contracts/build.rs` fingerprint caveat).
- Deploy evidence recipe: `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build <service...>`, confirm containers on the new image, then the named flow against the running stack through the BFF (`curl -N` for SSE). The frontend runs in a container over a bind mount: restart it to see host edits (memory).
- Flag pattern: strict `true`/`false` constant, compose passthrough `${NAME:-false}` next to `MP/deploy/docker-compose.yml:415-417`, compose-contract assertion like `scripts/tests/letta-tool-search-compose-contract-test.sh`.

### S0 [FINISH] Land-first, ADR, characterisation (S, 1-2 d)
**Goal.** Start from a committed base and pin today's behaviour so later diffs are visible.
**Files**
- `MP/docs/decisions/ledger.md` - entries: "One run-cost meter in model-gateway; cost-core stays ledger"; "Cost is null when unknown, never a default row" (D3); "ZDR turns state an ephemeral cost" (D4, marked proposed until owner confirms); "per-run ceiling vs windowed budget" (D1/D2).
- `MP/docs/capability-ownership-matrix.md` - add row "Run-cost meter and ceiling warnings": owner model-gateway (frames), inputs from cost-core pricing and Control quotas.
- NEW `MG/tests/usage_meter_contract.rs` - characterisation: `tool_round_tokens_are_absent_from_the_final_usage_frame` (expected to flip in S2), `unknown_model_priced_from_default_row` (flips in S1).
**Contract/flag/ZDR:** none. **Tests:** `cargo test -q -p model-gateway --test usage_meter_contract`. **DoD:** ledger entries merged; WS01 S0 commit identified as base. **Evidence:** CI. **Deps:** WS01 S0. **Risks:** none.

### S1 [FINISH] Honest pricing primitives and the pure `UsageMeter` (M, 3-4 d)
**Goal.** One pure, tested accumulator and a price lookup that returns null instead of a wrong number.
**Files**
- `MG/pricing.rs` - `lookup` (`:337-359`) returns `Lookup::{Exact, Prefix, Default}`; new `PricingCache::priced(basis, model, legs) -> Priced { usd: Option<f64>, by_category: Option<CategoryCost>, source }` where `Default` yields `usd: None` (D3); keep `cost_usd_for` as a thin wrapper used by the existing callers until S2 migrates them. New `PricingCache::snapshot() -> RateSnapshot` (one `ensure_fresh`, then a synchronous `priced(...)`) for the synchronous Do relay (S9). `CategoryCost { input_usd, cache_read_usd, cache_write_usd, output_usd }` computed from the same `cost_from_rate` terms; property test: sum equals `cost_from_rate`.
- NEW `MG/usage_meter.rs` (+ `pub mod usage_meter;` in `MG/lib.rs`) - `CallUsage { phase: Phase, model, provider, basis: CostBasis, input, output, cache_read, cache_write }`, `Phase = Answer | ToolRound | AgentRound | Overhead`; `UsageMeter::record(&CallUsage, &Priced) -> Snapshot`; `Snapshot { calls, input, output, cache_read, cache_write, cost_usd: Option<f64>, unpriced_calls, basis: Option<CostBasis>, by_category }`. Rules: cost is `None` once any metered call is unpriced (never a partial sum presented as total; `unpriced_calls` states why); a subscription call adds tokens, no USD; uncached input = input - read - write (saturating); counters saturate, never wrap.
- `MG/state.rs:426-427` - comment stays true ("a disabled cache emits a null cost"); no change expected.
**Contract:** none on the wire yet. **Flag:** none. **ZDR:** pure arithmetic over counts; no content. **Tests:** `cargo test -q -p model-gateway --lib pricing usage_meter`; new tests `unknown_model_is_unpriced_not_default_priced`, `category_costs_sum_to_total`, `any_unpriced_metered_call_nulls_the_total`, `subscription_calls_add_tokens_and_no_usd`; update `pricing.rs:533,564` fixtures. **DoD:** characterisation test `unknown_model_priced_from_default_row` flipped. **Evidence:** unit tests only (no deploy). **Effort:** M, 3-4 d. **Deps:** S0, D3. **Risks:** an org that relied on default-row display sees an empty cost for an uncatalogued model; mitigated by showing tokens and "price unavailable" (S4).

### S2 [FINISH] Count tool rounds and overhead calls; cumulative final frame (L, 5-7 d; enablement gated by D5)
**Goal.** The ledger and the frame describe the whole turn, and chat rows carry a `run_id`.
**Files**
- `MG/tool_loop.rs` - after `resp` is read (`:6699-6709`) build a `CallUsage` (phase `ToolRound`, basis `CostBasis::of_route(provider_hint, subscription_connection_id).served_by(&resp.provider_used)`), price it, push to a local `usage: Vec<CallUsage>`; add `pub usage: Vec<CallUsage>` to `ToolRounds` (`:4565`) and fill it at the two production literals (`:5344` forced web search, `:7428` main) and the test literal (`:9752`). Remove nothing: the debug log stays.
- `MG/sse.rs` - the single caller (`:2412`) keeps `rounds.usage`; at the final site (`:3987-4130`) fold `rounds.usage` + the answer call into one `UsageMeter` snapshot; the final `ChatEvent::Usage` carries the cumulative tokens and cost (D7). Under the flag, publish one extra `USAGE_ENVELOPE` per tool-round call with `idempotency_key = "{request_id}-USAGE_ENVELOPE-r{n}"`, `run_id = session_run.run_id` (also add `run_id` to the answer envelope; `build_usage_envelope` `:9792-9847` gains a `run_id: &str` argument, additive payload key). Overhead calls (`:7551`, `:7782`, `:7811`, `:7898`, `result_validation.rs:62,66`): classify each by reading it, record those that spend tokens as phase `Overhead` through one helper `RichEventSink::record_overhead`; any not classified stays listed in the ADR as unmetered.
- `MG/sse_events.rs` - `Usage` (`:229-244`) gains `calls: u32`, `unpriced_calls: u32`, `cost_by_category: Option<CategoryCost>`; payload (`:515-540`) writes `calls` always, `unpriced_calls` and `cost_by_category` only when present. (Cache keys come from WS02 S1; if S1 of WS02 has not landed, this slice adds them with the same names: `cache_read_tokens`, `cache_write_tokens`, written only when non-zero.)
- `FE/src/shared/api/chat-client.ts` (`:323-339`, `:1355-1367`, `:1600-1609`) - `ChatUsageEvent` += `calls`, `unpricedCalls`, `costByCategory`; parse additive keys; `use-chat-controller.ts:3187-3206` replaces turn fields (not adds).
- `deploy/docker-compose.yml` - pass-through for `MODEL_GATEWAY_TURN_USAGE_COMPLETE`.
**Proto/data:** none (no migration: `cost_entries.run_id` exists; cost-core accepts `run_id`, `CC/cmd/main.go:90`). **Flag:** `MODEL_GATEWAY_TURN_USAGE_COMPLETE` default `false` controls the extra envelopes and `run_id`; the cumulative FINAL FRAME is always honest (it is a display fix) and ships ungated. Rollback: unset.
**ZDR:** the tool loop does not run on ZDR turns (`zdr_features.rs` `tools` row Inert, [L] `:158-160`); envelopes are never published for ZDR (`CC/cmd/main.go:492-493` also rejects). Golden case: add `usage_envelopes_are_not_published_for_a_zdr_turn` to `MG/tests/zdr_feature_table.rs`.
**Tests:** `cargo test -q -p model-gateway --lib tool_loop usage sse_events`; update `tool_loop.rs` tests that build `ToolRounds`; new `tool_rounds_report_every_provider_call`, `final_usage_is_the_sum_of_tool_rounds_and_answer`, `chat_envelopes_carry_run_id`, `overhead_calls_are_recorded_once` in `MG/tests/usage_meter_contract.rs`; `go test ./...` in cost-core (no change; run as guard); taxonomy `--check`; vitest `chat-client.test.ts` (extend `:1685-1708` cases) and `use-chat-controller.recovery.test.ts`.
**DoD:** a 3-round tool turn shows tokens equal to the provider-reported sum (flag off: frame only; flag on: N+1 ledger rows with `run_id`, `GET /api/v1/cost/run?run_id=` non-zero).
**Evidence:** rebuild `model-gateway` and the frontend; run one turn that triggers 2-3 tool rounds; capture the SSE `usage` JSON, the per-round debug lines, `GET /api/v1/cost/entries` rows and `/api/v1/cost/run` (flag on), and the unchanged row count with the flag off.
**Effort:** L, 5-7 d. **Deps:** S1, WS01 S0, WS02 S1 (soft), D5/D7. **Risks:** double counting against the answer row if a round's usage is also folded into the answer chunk (assert in test: provider counts are per call); extra NATS publishes per turn (bounded by tool rounds <= ceiling); ledger totals jump (communicate; flag).

### S3 [BUILD] `usage_progress` frame, gateway to SPA parse (M, 3-5 d)
**Goal.** A cumulative frame after every provider call.
**Files**
- `MG/sse_events.rs` - new variant `UsageProgress { snapshot: Snapshot, phase: &'static str, ceiling: Option<Ceiling>, warning: Option<&'static str>, routing: Option<&'static str> }` in one fenced block `// ---- usage progress (WS27) ----` in enum, `family()` (add to the `usage` arm at `:361-363`), `name()` (`"usage_progress"`, `:398-410`), `payload()`. Keys: `calls`, `phase`, `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `cost_usd` (null when unknown or subscription), `cost_basis` (omitted if mixed), `unpriced_calls` (omitted when 0), and, from S6/S7, `ceiling_usd`, `ceiling_tokens`, `fraction`, `warning`, `routing`. Closed tokens only; never content, tool names or arguments.
- `MG/sse_events.rs` `RichEventSink` (`:674`, `emit` at `:933`) - `record_call(CallUsage, Priced)`: updates the sink's meter and emits `UsageProgress` through the same gated path. `tool_loop.rs` calls it where S2 records the call (`sink` already threaded, `:6284`).
- `MG/sse.rs` - the streaming answer call has no mid-stream usage (providers report at the end): the final `usage` frame stays the terminal authority; no progress frame for the answer phase beyond the one emitted just before `usage`.
- `MP/docs/sse-event-taxonomy.md` - regenerate. WS12 golden sample rows for `usage_progress` (min and full).
- `FE/src/shared/api/chat-client.ts` - `ChatUsageProgressEvent`, handler `onUsageProgress` in `ChatStreamHandlers` (near `:639`), native `case 'usage_progress'` (add-only hunk); `FE/src/shared/chat/verevon-ui-events.ts` - `usage.progress`; `FE/apps/gateway/src/domains/ag_ui.rs:878-910` - add `"usage_progress"` to the CUSTOM list and extend its test. `streams.rs` untouched (byte-verbatim relay is a guard).
**Contract:** SSE only, additive; a client that predates it ignores an unknown event name. **Flag:** `MODEL_GATEWAY_USAGE_PROGRESS` default `false` until S4 DoD, then `true`. The SPA needs no flag. **ZDR:** no tool loop on ZDR; the only ZDR frame is the single `usage` (S5), so no progress frame exists there. Golden: `zdr_feature_table.rs` asserts a ZDR turn emits no `usage_progress`.
**Tests:** `cargo test -q -p model-gateway --lib sse_events` (extend `opt_in_unlocks_only_the_requested_families`, `names_and_payload_shapes_are_stable`; new `usage_progress_has_only_closed_keys`, `usage_progress_is_gated_with_the_usage_family`); taxonomy `--check`; `pnpm vitest run src/shared/api/chat-client.test.ts src/shared/chat/verevon-ui-events.test.ts`; gateway `cargo test` for ag_ui.
**DoD:** with the flag on, `curl -N` of a tool turn shows one `usage_progress` per provider call with monotonic cumulative counters; a client with no `usage` family receives none.
**Evidence:** rebuild model-gateway, then the BFF stream; capture the SSE transcript. **Effort:** M, 3-5 d. **Deps:** S2, WS01 S0, WS12 S7 (soft). **Risks:** `sse_events.rs` is the hot file (WS02, WS04, WS12, WS15, WS18): fenced block, second lander regenerates the golden.

### S4 [BUILD] SPA run-cost meter (M, 4-6 d)
**Goal.** The meter is visible where the work is, in calm wording, honest when unknown.
**Files**
- NEW `FE/src/features/chat/components/RunCostMeter.tsx` (+ `RunCostMeter.test.tsx`) - compact line in the Work panel header: tokens in/out, cache read share (`read / input`, hidden at 0, WS02 S1 rule), cost or "price unavailable"/"included in subscription" (never `$0`), phase of the last call; expandable breakdown by category (uncached input, cache read, cache write, output) from `costByCategory`; warning chip from `warning`; `aria-live="polite"` only on warning changes, never per token (audit section 8 live-region rule).
- `FE/src/features/chat/components/use-chat-controller.ts` - `onUsageProgress` writes a per-turn `meter` object (monotonic guard: ignore a frame whose `calls` is lower than the stored value); `onUsage` finalises it. `chat-types.ts` - `Message.meter`.
- `FE/src/features/chat/components/ChatLiveRunPanel.tsx` / `ChatPanels.tsx:908-941` - mount the meter in the Work panel; keep the "Prompt cache" metric but source it from the same fields.
- `FE/src/features/agents/components/AgentRunConsole.tsx` (`:141-161`, `:747-754`) - extend `RunUsage` with the meter fields and render `RunCostMeter` in `TrustPanel` (`:1467`).
- i18n via `i18n.tr` (Norwegian first); WS12 `spa-ui-gates` applies (no bare copy, no raw colour).
**Contract:** none beyond S3. **Flag:** none; flip `MODEL_GATEWAY_USAGE_PROGRESS` default to `true` at DoD. **ZDR:** renders only the single ZDR usage line (S5); nothing stored client-side beyond the existing turn state.
**Tests:** `pnpm vitest run src/features/chat/components/RunCostMeter.test.tsx ChatMessages.metrics.test.tsx src/features/agents/components/AgentRunConsole.test.tsx`; cases: subscription never shows `$0`, unknown price shows tokens, monotonic guard, warning announces once. Playwright `tests/e2e/chat-cost-meter.spec.ts` NEW.
**DoD:** live stack: meter climbs during a 3-round turn, final frame matches the last progress frame, subscription and unknown-model turns render honestly.
**Evidence:** rebuild model-gateway, restart the frontend container; screenshots of metered, subscription, unknown-price turns; the SSE transcript beside them. **Effort:** M, 4-6 d. **Deps:** S3, WS09 S2 (`renderLive`/`nodeKey`, soft). **Risks:** `ChatMessages.tsx`/`chat-client.ts` conflict with WS09/WS11/WS14/WS16/WS17: add-only hunks, WS09 S2 first.

### S5 [FINISH] ZDR in-stream cost (S, 2-3 d; gated by D4)
**Goal.** A ZDR answer states its cost, computed in-stream, persisted nowhere.
**Files**
- `MG/sse.rs` `DirectAnswer` (`:7670-7681`) - add `cache_read_tokens`, `cache_write_tokens` (from `InferResponse.cache_read_input_tokens`/`cache_creation_input_tokens`, `IC` proto `inference.proto:299-300`); `zdr_answer_stream` ([L] `:8513-8538`) takes a pre-priced `Priced`; `zdr_direct_stream` (`:8107`) prices via `state.pricing.priced(...)` before building the stream. `cost_basis` set from the route.
- `MG/zdr_features.rs` [L] `:169-172` - update the `usage` row comment ("counts, provenance, retention class and, when the catalogue is available, an ephemeral cost").
- `MP/docs/ZDR.md:102` - Metering row: split "in-stream cost: done; ledger persistence: DEF-Z-06b".
**Contract:** same `usage` keys. **Flag:** `MODEL_GATEWAY_ZDR_STREAM_COST` default `false`. **ZDR clause:** inputs are counts and the model id; the pricing catalogue GET carries no turn data (`pricing.rs:281`); nothing is written, no envelope, no budget gate added (ZDR still bypasses the gate; see risk 4). Golden: extend `zdr_posture_matrix.rs`/`zdr_feature_table.rs` with `zdr_usage_states_cost_but_publishes_no_envelope`.
**Tests:** `cargo test -q -p model-gateway --test zdr_feature_table --test zdr_posture_matrix` and `--lib zdr`; `bash scripts/tests/zdr-contract-sync.test.sh`.
**DoD:** ZDR-TEST org turn shows a cost line; `GET /api/v1/cost/entries` shows no row for it. **Evidence:** `zdr-live-proof.sh` (WS01) with the usage frame captured plus the empty ledger query. **Effort:** S, 2-3 d. **Deps:** S1, WS01 (working ZDR org + attested provider), D4. **Risks:** a displayed cost the org cannot see in dashboards (stated in the ADR).

### S6 [BUILD] Per-run ceiling resolution, 80/100 percent warnings, optional enforcement (M, 4-6 d; enforcement gated by D1/D6)
**Goal.** The user is warned before the ceiling, with real per-run numbers.
**Files**
- NEW `MG/run_ceiling.rs` - `RunCeiling::resolve(request_cap, org_limits)` with the exact precedence of `budget.rs:56-63` (request cap wins; org fills unset; token cap counts metered tokens only; subscription turns exempt from both). `fraction(snapshot)`; `warning_state(prev, now) -> Option<"run_ceiling_80" | "run_ceiling_reached">` fires each once.
- `MG/sse.rs` (`:1085-1118`) - keep `org_limits` in scope (today it is dropped after `check_budget`) and hand `RunCeiling` to the sink; `MG/sse_events.rs` `RichEventSink::record_call` attaches `ceiling_*`, `fraction`, `warning`.
- `MG/tool_loop.rs` - when `MODEL_GATEWAY_RUN_CEILING_ENFORCE=true` and the ceiling is reached, end the tool phase before the next round (same exit as the phase deadline, `:6690-6693`), noting it through the warning token; the answer still streams.
- `FE` - `RunCostMeter` renders the ceiling bar and the warning (S4 hook); no new copy beyond i18n keys.
**Contract:** additive keys on `usage_progress`. **Flags:** `MODEL_GATEWAY_RUN_CEILING_WARN` default `false` then `true` after S4; `MODEL_GATEWAY_RUN_CEILING_ENFORCE` default `false`. **ZDR:** ZDR turns have no tool loop and no gate; the ceiling does not apply (documented).
**Tests:** `cargo test -q -p model-gateway --lib run_ceiling budget org_quota`; new `request_cap_wins_and_org_fills_unset`, `warning_fires_once_at_80_and_once_at_100`, `subscription_is_exempt`, `enforcement_ends_tool_phase_and_still_answers`; vitest for the bar.
**DoD:** with an org ceiling of $0.05 a tool turn warns at 80 percent and (enforce on) ends the tool phase at 100 percent with a notice and an answer.
**Evidence:** rebuild model-gateway; set the quota through the existing org-core PUT (`FE/apps/gateway/src/domains/orgs/quotas.rs:72`) in a test org; capture frames and screenshot. **Effort:** M, 4-6 d. **Deps:** S3, D1/D6. **Risks:** the number a user sees as a "ceiling" differs from the cost-core lifetime gate until S8.

### S7 [BUILD] Surface the silent budget downgrade (M, 4-6 d)
**Goal.** "Routed one tier cheaper because the budget is constrained" is a visible fact.
**Files**
- `MP/proto/model_plane/v1/inference.proto` - NEW `message RouteNote { string budget_posture = 1; bool tier_shifted = 2; bool cheap_fallback_forced = 3; string applied_effort = 4; }` (shared with WS11); `InferResponse.route_note = 15` (after `provider_timing = 14`, `:306`), `InferChunk.route_note = 16` (after `provider_timing = 15`, `:369`). Add-only; integrator confirms tags (D8).
- `IC/provider/mod.rs` (`provider_timing` fields at `:264`, `:311` per grep) - `route_note: Option<RouteNote>` beside them, stamped in `IC/provider/fallback.rs` after `resolve_intent` (`:1011-1041`: `decision.posture` is in scope) and copied to the pb types where `provider_timing` is (`IC/grpc.rs:1046`, `IC/streaming.rs:47`). Use `..Default::default()` to avoid editing the ~15 `provider_timing: None` literals.
- `MG/tool_loop.rs`/`MG/sse.rs` - read `route_note` from the response/final chunk and set `routing: "tier_shifted" | "cheap_fallback"` on the next `usage_progress` (closed tokens, only when posture is Constrained/Exhausted).
- `FE` - `RunCostMeter` shows one calm line ("Budget is constrained; a cheaper model was used") from `routing`.
- Go: `buf generate` -> `MP/go/gen`; Rust rebuild.
**Contract:** proto add-only (`buf breaking` must pass); additive frame key. **Flag:** `INFERENCE_ROUTE_NOTE` default `false` (inference-core) so old gateways see no change; deploy inference-core first. **ZDR:** posture and booleans only; ZDR requests bypass intent routing for pinned models; no content.
**Tests:** `cargo test -q -p inference-core --lib intent fallback` (new `constrained_posture_stamps_tier_shifted`, `exhausted_stamps_cheap_fallback`, `pinned_model_stamps_nothing`), `--test fallback_test`; gateway `usage_progress` carries `routing`; proto wire-parity golden.
**DoD:** driving an org past 80 percent of `budget_cap_usd` shows the line. **Evidence:** rebuild inference-core + model-gateway; drive the org with a cost-core fixture (the same recipe WS11 S5 uses). **Effort:** M, 4-6 d. **Deps:** S3, D8, WS11 S5 (shares `RouteNote`). **Risks:** WS11 and WS27 both touch `inference.proto` and `fallback.rs`: one `RouteNote`, first lander defines it.

### S8 [FINISH] Windowed org budget (M, 4-6 d; gated by D2)
**Goal.** A "per period" ceiling stops being a lifetime ceiling.
**Files**
- `CC/internal/server/server.go` (`budgetCheckRequest`, `handleBudgetCheck` `:540-628`) - optional `since` (RFC3339); when set, per-user and org-wide reads both use `Aggregate(AggregateFilter{OrgID, UserID, Since})` (`ledger.go:239-246`) instead of `GetUsage`. Default (absent) is unchanged, so old gateways and inference-core keep today's behaviour.
- `MG/org_quota.rs` (`QuotaRow` `:59-63`) - read `reset_period` and compute the window start in UTC (`daily` = start of day, `monthly` = start of month, `none` = no `since`); `MG/budget.rs` (`check_budget_at` `:82-155`, `BudgetCheckRequest` `:16-22`) forwards `since`.
- `IC/provider/intent.rs` `BudgetClient` (`:276-345`) - unchanged in this slice; the advisory posture stays lifetime until a follow-up (documented).
**Contract:** cost-core JSON additive (note `decodeRequestJSON` uses `DisallowUnknownFields`, `server.go:724`, so cost-core must deploy BEFORE the gateway sends `since`). **Flag:** `MODEL_GATEWAY_BUDGET_WINDOW` default `false`. **ZDR:** ZDR rows are never in the ledger; no change.
**Tests:** `go test ./...` in cost-core: NEW `internal/server/budget_window_test.go` (`since_excludes_older_entries`, `absent_since_keeps_lifetime_behaviour`); `store_integration_test.go` (needs `COST_CORE_TEST_DATABASE_URL`); gateway `budget.rs` wiremock: `reset_period_daily_sends_start_of_utc_day`.
**DoD:** an org at its monthly ceiling last month is admitted this month. **Evidence:** rebuild cost-core then model-gateway; two fixture entries (last month, this month); `POST /api/v1/budget/check` with and without `since`. **Effort:** M, 4-6 d. **Deps:** D1, D2; independent of S1-S7. **Risks:** behaviour change for capped orgs (D2); clock/timezone (UTC stated); deploy order.

### S9 [BUILD] Do-run per-round progress (L, 6-8 d; builds on WS02 S3 and WS03)
**Goal.** A Do run shows spend per round, not a hard-coded 0 (`MG/sse.rs:9716-9719`).
**Files**
- `MP/proto/model_plane/v1/orchestration.proto` - `OrchestrationEvent` oneof (`:1031` onward; 23 is last at `:1051`): `RunUsageProgress run_usage_progress = 27;` NEW `message RunUsageProgress { string run_id = 1; uint32 round = 2; uint64 input_tokens = 3; uint64 output_tokens = 4; uint64 cache_read_input_tokens = 5; uint64 cache_creation_input_tokens = 6; string model_used = 7; string provider_used = 8; }` cumulative per run, counts and ids only. Tag 27 proposed after WS03 (24, 25) and WS15 (26); integrator assigns.
- `EC/runtime_loop/agent.rs` - after the per-round log (`:1450-1463`) publish through a new `publish_run_usage_progress` modelled on `publish_plan_transition` (`:4909-4949`): best-effort, bearer and `session_channel` from the same context. Tally shared with WS02 S3's `UsageTally` (cloned into children), so parent plus delegated rounds are one cumulative number.
- `session-core/src/orchestration_grpc.rs` - `event_run_id` (`:1783-1795`) gains the arm; broadcast and replay as is; NOT persisted (only browser events persist, `:3227`). Extend the `event_run_id_extracts_per_variant` test (`:4102`).
- `MG/sse.rs` - `follow_agentic_run` (`:9066-9100`) takes a `step_frame` closure: add a second synchronous mapper that prices with a `RateSnapshot` taken once before the loop and a running `UsageMeter`, producing `ChatEvent::UsageProgress` (`phase: "agent_round"`). Replace the hard-coded final Usage (`:9716-9734`) with `RunAgentResponse.usage` (WS02 S3, field 9) priced the same way; delete the stale comment.
- `FE` - no new parser (same `usage_progress` frame); Run Console meter works unchanged (S4).
**Flag:** `EXECUTION_RUN_USAGE_PROGRESS` default `false`; gateway side needs none. Deploy execution-core + session-core first, then gateway. **ZDR:** Do runs are refused under ZDR at admission (`zdr_features.rs` `agentic` row, [L] `:150`); counts only; not persisted.
**Tests:** `cargo test -p execution-core --lib` NEW `each_round_publishes_cumulative_run_usage`, `delegated_rounds_are_included`; `cargo test -p session-core --lib event_run_id`; `cargo test -p model-gateway --lib orchestration_event_to_sse follow_agentic_run`; proto wire-parity.
**DoD:** a multi-round Do run shows a rising meter and the final frame equals `RunAgentResponse.usage`. **Evidence:** rebuild execution-core, session-core, model-gateway; one multi-round Do run; capture the SSE transcript and `/api/v1/cost/run` (with WS02 S3 enabled). **Effort:** L, 6-8 d. **Deps:** S3, WS02 S3 (and D1 there), WS03 (resume keeps `run_id`; per-segment usage continues under the same id, WS03 note). **Risks:** event volume (one per round, bounded by `max_rounds` <= 32, `MG/run_budget.rs:61`); pricing in a synchronous closure (hence `RateSnapshot`); unary `RunAgent` means a long single round shows nothing until it ends (stated limit).

### S10 [BUILD] Member quota readout (M, 4-6 d; gated by D9)
**Goal.** A member sees remaining allowance and a warning at 80 percent, without admin rights and without amounts they should not see.
**Files**
- NEW `FE/src/shared/actions/billing-quota.ts` - `billing.quota.read` contract mirroring `billing-allowances.ts:3-6`; register next to `billing.allowances.read` in `FE/src/shared/actions/action-registry.ts` (action contract before UI wiring, repo rule); `node --test apps/gateway/scripts/action-surface-contract.test.mjs`.
- NEW `FE/src/shared/api/billing-quota.ts` (+ test) - zod schema for `QuotaStatus{limit,used,remaining,is_exceeded,utilization}` and `getBillingQuota(metric)` over the existing BFF `GET /api/v1/billing/quotas/{metric}` (`billing.rs:26`); no BFF change expected. If product wants coarse state only (no numbers) for non-admins, add a trimmed projection in `FE/apps/gateway/src/domains/billing.rs` (decision with Control).
- `RunCostMeter.tsx` - secondary line "Allowance: N percent used" and warning at `utilization >= 0.8` / `is_exceeded`; fetched once per thread open and after a turn, not per frame.
- `MG` - none. Per-user allowance: out of scope (Control).
**Flag:** `VITE_QUOTA_READOUT` or existing FE flag pattern (name not determined; follow `FE/docs` flag conventions); default off. **ZDR:** reads Control data only; nothing about turns.
**Tests:** `pnpm vitest run src/shared/api/billing-quota.test.ts src/features/chat/components/RunCostMeter.test.tsx`; gateway `cargo test` for any projection. **DoD:** a non-admin member sees utilization from the live billing-core. **Evidence:** rebuild frontend; configure an allowance via the admin route in a test org; screenshot as member and admin. **Effort:** M, 4-6 d. **Deps:** S4, D9, Control billing-core (existing). **Risks:** allowances are org-wide, so a member may be warned for spend that is not theirs (label "organisation").

### S11 [BUILD, gated; not scheduled] Plan-card upper bound (S-M, 3-4 d)
**Goal.** The plan card shows "at most about X" (a bound, never "estimate" without a label), computed from server-resolved budgets.
**Files**
- `MG/sse_events.rs` `PlanProposed` (`:272-276`, payload `:584`) - optional `cost_bound: Option<CostBound { max_usd: Option<f64>, max_tokens: u64, rounds: u32, basis: "upper_bound" }>`; emitted at `MG/sse.rs:9637`.
- NEW `MG/cost_bound.rs` - bound = `RunBudget.max_rounds` (`MG/run_budget.rs`) x (context tokens x input rate + per-round output ceiling x output rate); context tokens from session-core's assembled estimate (`mp_tokens::count_for_budget`, `session-core/src/grpc.rs:6872`, already shown as `estimatedTokens` in `ChatPanels.tsx:620`); `max_usd: None` when the model is unpriced or subscription. WS11 `effort::allowance(level)` widens the output term.
- `FE` plan card - label "Upper bound, not a prediction"; never written to `costUsd`.
**Gate:** build only after S2-S4 have produced real per-run data to sanity-check the bound against (card: defer estimate). **Flag:** `MODEL_GATEWAY_PLAN_COST_BOUND` default `false`. **ZDR:** plan mode is refused under ZDR. **Tests:** `cargo test -p model-gateway --lib cost_bound` (`bound_never_below_observed_run_cost`, `unpriced_model_gives_no_usd`); vitest. **DoD/Evidence:** plan card with the labelled bound vs the actual meter after approval. **Deps:** S1, S3, WS11, WS13/WS14 (plan card ownership), WS10 (entitled models only). **Risks:** bound too loose to be useful; WS13/WS14 touch `PlanProposed`.

## 5. Interfaces

**PROVIDES**

| To | Contract |
|---|---|
| WS09 (transcript) | `usage_progress` frame and `Message.meter`; cumulative `usage` with `calls`, `cost_by_category` |
| WS15 (lifecycle) | closed `warning` and `routing` tokens, counts-only; WS27 does not use `context_change` (self-contained); WS15 may mirror them as `context_change.change` tokens later |
| WS11 (effort ladder) | single `RouteNote.applied_effort` (S7) and the posture clamp visibility WS11 S5 needs |
| WS20 (scheduled runs) | `RunCeiling` resolution reusable for a scheduled run's cap (D10 there); run usage on step responses keyed by `run_id` |
| WS02 | extends its `RunAgentResponse.usage` and `ChatEvent::Usage` cache keys with progress and pricing; chat envelopes gain `run_id` |
| WS21 / Quarry (b) | cost-core ledger key `run_id` and the ceiling value to be min-ed with grant claims; Quarry enforces browser step cost |
| WS10 | priced-model set = entitled set; honest-null rule for unpriced models |
| Space session (a) | `run_id` join key on chat rows; extensible payload for later space/agent dims |

**CONSUMES**

| From | Contract |
|---|---|
| WS01 S0 | live ZDR wave committed (sse.rs, sse_events.rs, zdr_features.rs) |
| WS02 S1 / S3 | Usage cache keys; `RunAgentResponse.usage = 9`; `UsageTally`; D1 billing decision |
| WS03 | per-segment usage under one `run_id` on resume; `extensions.metering` placeholder |
| WS11 | `effort::allowance`, posture clamp, shared `RouteNote` |
| WS12 | taxonomy gate, payload golden, `spa-ui-gates` |
| WS13/WS14 | plan card ownership (S11) |
| Control | org-core quota rows (`reset_period`), billing-core `QuotaStatus` |

**Conflict hot-spots and edit order**

| File | Others | Rule |
|---|---|---|
| `MG/sse_events.rs` | WS02, WS04, WS12, WS15, WS18 | WS01 S0, WS02 S1 (cache fields), then fenced `// ---- usage progress (WS27) ----`; second lander regenerates golden and taxonomy |
| `MG/sse.rs`, `MG/tool_loop.rs` | WS01-WS04, WS08, WS21, WS25 | WS01 lands live set; WS02 first; WS27 hunks are calls into `usage_meter.rs`/`run_ceiling.rs` |
| `FE/src/shared/api/chat-client.ts`, `use-chat-controller.ts`, `ChatMessages.tsx`, `ChatPanels.tsx` | WS08, WS09, WS11, WS14, WS16, WS17, WS18 | WS09 S2 first; add-only hunks, no reformat |
| `inference.proto`, `IC/provider/fallback.rs`, `provider/mod.rs` | WS02 (InferRequest 17-19), WS11 | integrator reserves tags; one `RouteNote` |
| `orchestration.proto`, `session-core/src/orchestration_grpc.rs` | WS03, WS13, WS14, WS15, WS17 | append-only; tag 27 proposed |
| `EC/runtime_loop/agent.rs` | WS03, WS05, WS24, WS25 | WS03 first |
| `deploy/docker-compose.yml` | all | passthrough lines only |

## 6. Cross-session interfaces

- **Space session (a):** overlap row 16 says cost dims and the Usage frame are touched twice. Rule: Model (cost-core + this stream) owns the meter and frames; the Space session adds `space_id`/agent dimensions to cost-core and reads the same `run_id`. Assumed from (a): no second usage frame; any Space cost chip reads `usage_progress`/`usage`. WS27 adds no dims (card defers attribution).
- **Quarry / browser session (b):** overlap row 13 (`max_cost_usd` is enforced nowhere today; Quarry enforces steps only). Rule: Quarry enforces browser cost from grant claims (min of request, grant, org); Model supplies the ceiling value (`RunCeiling`) and the ledger key. Assumed from (b): `ValidateGrantResponse` v2 `budget{max_cost_usd,max_runtime_s}`; browser spend reaches cost-core under the same `run_id`.
- **Control (not a session):** quota semantics (D1), `reset_period` (D2), per-user allowance (out), metric name (D9). org-core is the one quota source (overlap row 15).

## 7. Regression guards (audit section 8)

| Strength | Touch | Gate |
|---|---|---|
| Honest outcomes: cost null when unknown, basis marks subscription (`:1510`) | extended | S1 tests `unknown_model_is_unpriced_not_default_priced`, `subscription_calls_add_tokens_and_no_usd`; existing `usage_cost_basis_explains_a_subscription_cost_and_is_otherwise_optional` (`sse_events.rs:1631`), `ChatMessages.metrics.test.tsx`, `CostDashboardPage.test.tsx:45` stay green |
| Typed ChatEvent taxonomy, CI gate, byte-verbatim relay (`:1511`) | new variant | taxonomy `--check`, WS12 payload golden, `streams.rs` untouched, `ag_ui.rs` test |
| ZDR by construction (`:1509`) | S2/S3/S5/S9 | zdr matrix cases in S2, S3, S5, S9; `zdr-contract-sync.test.sh`; no envelope or persistence under ZDR |
| Two loops kept separate (`:1508`) | meter in gateway loop; Do progress via events | no shared loop code; `cross_service_loop_contract.rs` unchanged; execution-core only publishes counts |
| Intent routing with provenance (`:1520`) | S7 reads, never replaces | `intent.rs` tests (`constrained_budget_downgrades_one_tier`, `exhausted_budget_forces_cheap_fallback`) unchanged and green |
| Calm workspace / live region (`:1518`) | S4 | meter announces on warning change only; vitest |
| Credential isolation (`:1512`) | S10 uses the BFF delegated path | no new token surface; action contract test |

## 8. Effort and risks

**Total:** about 43-62 engineer-days across 12 slices (S0 1-2, S1 3-4, S2 5-7, S3 3-5, S4 4-6, S5 2-3, S6 4-6, S7 4-6, S8 4-6, S9 6-8, S10 4-6, S11 3-4). **Critical path:** WS01 S0 -> S0 -> S1 -> S2 -> S3 -> S4, then S6 and S9 (S9 also needs WS02 S3). S5, S7, S8 are parallel once S1 and decisions land; S10 after S4; S11 last.

**Top risks**
1. **Ledger and billing jump when tool rounds become metered (S2).** Mitigation: flag off by default, D5 Control confirmation, frame fix ships independently, evidence compares row counts flag off vs on.
2. **Lifetime-vs-per-run budget confusion (D1/D2).** The cap users configure is not what is enforced; S6 warnings can disagree with the 402 gate until S8. Mitigation: ADR in S0, user-facing wording says "this run", S8 before enforcement default flips.
3. **Hot-file conflicts** in `sse.rs`, `sse_events.rs`, `chat-client.ts`, `inference.proto` with six other streams, plus ~19 percent of cited files being uncommitted. Mitigation: land the live wave first, new modules with thin call-ins, fenced blocks, one `RouteNote`, integrator-reserved tags.
4. **ZDR gap persists:** ZDR turns bypass the budget gate (`MG/sse.rs:976`) and the ledger, so org admins undercount and ceilings do not apply to ZDR. S5 only displays cost. Mitigation: state it in ZDR.md; `DEF-Z-06b` owner decision, outside this stream.
5. **Honesty regressions:** a progress frame that sums an unpriced call, shows `$0` for a subscription, or shows an upper bound as cost. Mitigation: the pure `UsageMeter` rules plus the contract tests in S1/S3/S11 and the SPA tests in S4.
