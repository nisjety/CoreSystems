# WS11 effort ladder: code-map sweep (F-t3-029)

- Date: 2026-10-14. Read-only sweep. No builds, tests or git writes were run.
- Worktree: `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b`, HEAD afb83928, clean except `.plan-work/`.
- Live tree: `C:/dev/CoresSystem`, same HEAD, 324 entries in `.plan-work/live-tree-status.txt`. "clean" below means not listed there.
- Audit: `03-model-plane-chat-and-agents-audit.md`. Card F-t3-029 at lines 765-775. Wave W1 row 1546 (section 9.1). Cheap-wins row 1671. Open question 10.2 #15 at 1738. Appendix D at 2130. Index row 1846. Cross-references at 213, 308, 918, 1590, 1702. No Appendix C entry (F-t3-029 is Recommended, not Priority).
- Legend: MATCH (behaviour found where cited), DRIFT (found at other lines), ABSENT (not found). M = modified in live, ?? = untracked in live, DIFFERS-IN-LIVE = live line numbers differ from worktree.

## 0. Summary

- The three cited code paths all exist. Two MATCH in the worktree. The third (audit scope note and 10.2 #15, `agent.rs:1336-1339`) is DRIFT in the worktree: the thinking-off line is 1335 there. It is correct only in the live tree (1336-1339), where the file is M.
- Effort reaches a provider only as a derived token budget. Path: HTTP `InvokeRequest.effort` (string), then `thinking::budget_tokens`, then proto `InferRequest.thinking_budget_tokens` (tag 13). No proto message carries an effort enum. The budget is derived before and independently of the resolved model (`sse.rs:2542` worktree).
- Three inference-core adapters, three behaviours. Anthropic omits thinking for unsupported models or budgets below the floor (`anthropic.rs:436-470`). OpenAI API and Azure send nothing (`openai.rs:411-480`), yet declare `supports_thinking: true` (`openai.rs:649`). Codex maps to low or high (`codex_subscription.rs:155-161`).
- Cross-plane gap, not in the card: the Codex effort contract ends in Ingestion Plane `integration-corev2`. Its Go broker accepts only low or high (`manager.go:422-427`). Widening the Codex ladder needs an Ingestion Plane change.
- Product decision to flag: Do-run thinking is hard-set to 0 (`agent.rs` 1335 worktree, 1339 live). No test pins it, and `execution.proto` has no effort field.
- Live-only changes that matter: `sse.rs` and `http_routes.rs` add an uncommitted ZDR thinking-budget path. `agent.rs` shifts by +4 lines. The frontend effort lines are unchanged in live.

## 1. Citation verification

| card id | cited path:lines | claim | worktree verdict | live status | corrected path:lines |
|---|---|---|---|---|---|
| F-t3-029 | apps/Model Plane/rust/services/model-gateway/src/thinking.rs:1-80 | three-value effort profile to budget | MATCH: `budget_tokens` 36-53; unknown profile maps to 0 (40-41); consts 18-21; headroom 29; `is_active` 57-59; tests 61-117 | clean | thinking.rs:16-59 (logic), 61-117 (tests) |
| F-t3-029 | apps/Model Plane/rust/services/inference-core/src/provider/openai.rs:430-455 | OpenAI/Azure body sends no reasoning effort | MATCH: `build_request_body` 411-480 read; no reasoning field; `is_reasoning_model` only switches `max_completion_tokens` (444-449); grep: no non-test read of thinking budget in openai.rs | clean | openai.rs:411-480; predicate 560-563 |
| F-t3-029 | apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:1334-1340 | Do loop hard-sets thinking budget to 0 | MATCH: `thinking_budget_tokens: 0` at 1335 inside `run_rounds` (fn 1168); comment 1332-1334 | M (+200/-7; no effort line changed). Field now at 1339, fn 1172 | worktree 1332-1335; live 1336-1339 |
| F-t3-029 scope note; 10.2 #15 | apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:1336-1339 | same claim, cited lines | DRIFT: worktree 1336-1339 are `request_id`, `org_id` and route comments | DIFFERS-IN-LIVE: 1336-1339 correct in live | worktree 1332-1335; live 1336-1339 |
| F-t3-029 (symbol, no path) | `codex_reasoning_effort` | Codex gets low or high only | MATCH: `codex_subscription.rs:155-161` (budget > 1024 gives high, else low); call at 142; tests 1004-1007 | clean | codex_subscription.rs:142, 155-161 |
| F-t3-029 (no path) | card: Budget and Balance defaults mostly OpenAI-family | `provider/routing_policy.rs:136-160` | MATCH: Budget 3 of 3 cells OpenAI-family (gpt-5-nano, gpt-4o-mini, gpt-5-mini); Balance 2 of 3 (complex is claude-sonnet-4-6); Genius 1 of 3 | clean | routing_policy.rs:143-159 |
| F-t3-029 (no path) | card: additive InferRequest proto field | apps/Model Plane/proto/model_plane/v1/inference.proto:100-175 | MATCH: tags 1-16 in use; 17 is free; `thinking_budget_tokens = 13` at 162 (comment 155-161) | clean | inference.proto:155-162; next free tag 17 |
| F-t3-029 (no path) | card: composer levels | apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx:152, 3543-3554 | MATCH: composer offers Rask / Standard / Grundig (quick / auto / deep). Wire types differ (section 2 hop 2) | M (+7; effort lines unchanged) | worktree 152, 3543-3554, 3830-3834; live 154, 3560, 3838 |
| F-t3-029 (no path) | card: Anthropic thinking budget | anthropic.rs (not cited in card) | MATCH: `resolve_thinking_budget` 436-470; floor const 404 (1024); body 713-718 | clean | anthropic.rs:404, 436-470, 713-718 |
| F-t3-029 (no path) | card: "Reference practice" [SRC] (Hermes, Codex, DeepSeek, T3 Code) | none | NOT DETERMINED: external references, no repo path | n/a | n/a |

## 2. Effort wire path (verified hops)

1. UI selection. `DashboardComposer.tsx:152` `ResponseMode` = auto | quick | deep. Segmented options at 3531. Select fallback 3543-3554 with options Rask (quick), Standard (auto), Grundig (deep). Wire mapping 3830-3834: quick sends `effort: "quick"`, deep sends `effort: "deep"`, auto omits the key. Send-options type at 362.
2. Client types. `chat-client.ts:61` `ChatInvokeRequest`; `:115` `effort?: 'quick' | 'standard' | 'deep'`; omit-if-standard at 1096-1102 in `buildChatWireBody` (1004). `chat-types.ts:520-521` is narrower (`'quick' | 'deep'`). `pending-chat-launch.ts:160-162` `normalizeEffort` accepts only quick and deep.
3. AG-UI transport. `ag-ui-client.ts:276` copies `effort` when set. Gateway `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/ag_ui.rs:469` forwards any string as `effort`, with no allow-list.
4. Model Plane HTTP. `http_routes.rs:6563` `pub struct InvokeRequest`; `effort: Option<String>` at 6674-6679 (live: struct 6669, field 6780-6785). Route `/v1/invoke/stream` (`http_routes.rs:137`) goes to `sse::invoke_stream_sse` (`sse.rs:835` worktree, 875 live).
5. Budget derivation. `sse.rs:2539-2545` calls `thinking::budget_tokens(effort, answer_token_budget())`. The result is set on `InferRequest` at 2562 (live: derivation 2670-2671, field 2690). Unset effort becomes empty string, which yields 0.
6. Live-only ZDR path (uncommitted). `sse.rs:7768-7800` `direct_infer_turn` takes `thinking_budget_tokens` (param 7777, set 7800). `sse.rs:8336-8352` `ZdrTurnSettings.thinking_budget_tokens` (8349). Derivation in `zdr_turn_settings` (fn 8380; 8399-8402). Used in `zdr_direct_stream` (fn 8411; 8444). `http_routes.rs:8986` inside `async fn invoke` (live only; worktree count 0).
7. Hard-coded producers. `grpc.rs:425-450` `build_infer_request` sets 0 (449; live 450), with callers at 765 and 975. `grpc.rs` `extract_structured` (2507) sets 0 (2584; live 2599). `tool_loop.rs` `run_tool_rounds_for_model` (6334) sets 0 (6652; live 6759), so tool rounds get no effort. `source_validation.rs` `review_candidate` (2167) sets 4096, or 0 if tool-reporting (2277, same in live). `agent.rs` `run_rounds` sets 0 (see section 1).
8. Proto. `inference.proto:100-175` `InferRequest`. `thinking_budget_tokens = 13` (162) is advisory: 0 is default, and it is forwarded only to models that accept it (comment 155-161). `InferChunk.reasoning_delta` at 347.
9. Inference-core. `grpc.rs:1062` `to_internal_request` (live fn 1072) copies `thinking_budget_tokens` at 1088 (live 1099) into `provider::InferRequest` (`provider/mod.rs:120`; field 152).
10. Adapters.
    - Anthropic: `anthropic.rs:404` floor 1024. `resolve_thinking_budget` 436-470 returns None for a zero budget, for models that predate thinking, for budgets under the floor, and when no room is left for the answer. Body 713-718 writes `thinking.budget_tokens`. `supports_thinking: true` at 934.
    - OpenAI API and Azure, one module (`OpenAiFlavor` 24-28, with an Azure variant at 28). No reasoning field in the body. `supports_thinking: true` at 649. `is_reasoning_model` (560-563) matches `gpt-5*`, `o1*`, `o3*`, and gates only `max_completion_tokens`.
    - Codex subscription: `codex_subscription.rs:114` `request_body`. `reasoning_effort` at 142 from `codex_reasoning_effort` (155-161; > 1024 gives high, else low). `service_tier` at 143 and 166-176: priority when budget is not 1024 or priority is requested, for five model ids.
11. Ingestion broker (Codex). `integration-corev2/internal/api/codex_subscription.go:339-349` maps `body.ReasoningEffort` (struct 475; JSON `reasoningEffort` at 483 worktree, 491 live). `codexsubscription/manager.go:86-94` `InvokeRequest` (field 92). `manager.go:422-427` defaults to "low" and rejects anything other than low or high (`ErrInvalidRequest`). Transports: app-server `appserver.go:483-484` sends `model_reasoning_effort` in thread/start only when `WithThreadEffort` is on (60; env `CODEX_APP_SERVER_THREAD_EFFORT`, default true). turn/start `appserver.go:503-506`. Direct transport `direct.go:491-506` writes `reasoning.effort` (497).
12. Output. `reasoning_delta` becomes SSE `reasoning` (`apps/Model Plane/docs/sse-event-taxonomy.md:30`; M in live). The client must include feature `reasoning` (`gateway.proto:241-244`; `chat-client.ts:110-113` comment).

## 3. Affected-file map

| path | role | approx lines (worktree; live if different) | live status | tests (path and names where cheap) |
|---|---|---|---|---|
| model-gateway/src/thinking.rs | effort profile to budget (quick 1024, deep 4096, step-down) | 16-59; tests 61-117 | clean | thinking.rs: standard_and_unknown_request_no_thinking (67), profiles_map_to_their_budgets (79), a_tight_ceiling_steps_down_rather_than_truncating_the_answer (91), the_answer_always_keeps_its_headroom (104) |
| model-gateway/src/sse.rs | chat stream: derives budget, builds InferRequest; ZDR direct path | invoke_stream_sse 835 (live 875); derivation 2539-2562 (live 2670-2690); live-only 7768-7800, 8336-8444 | M (+883; 12 effort or thinking diff lines, live-only ZDR path) | none for effort found by grep |
| model-gateway/src/http_routes.rs | `InvokeRequest.effort`; routes | struct 6563; field 6674-6679 (live 6669, 6780-6785); routes 137, 413; `ai_chat` 2306; live-only 8986 | M (+237; 1 effort line) | none found by grep |
| model-gateway/src/grpc.rs | gRPC entry builds InferRequest with budget 0 | build_infer_request 425-450; callers 765, 975; extract_structured 2507-2584 (live 2599) | M (+47; no effort line changed) | none found |
| model-gateway/src/tool_loop.rs | tool rounds force budget 0 | run_tool_rounds_for_model 6334; budget 6652 (live 6759) | M (+425; no effort line changed) | none found |
| model-gateway/src/normalize.rs | test helper `InvokeRequest { effort: None }` | 439-441 | M (+17; no effort line changed) | used by normalize tests (names not extracted) |
| model-gateway/src/source_validation.rs | subscription source review forces budget 4096 (or 0) | review_candidate 2167; budget 2274-2277 | clean | batched_partial_results_reconfirmed_after_repair (3143; asserts 0 at 3161); reviewer_edits_never_bypass_an_independent_complete_recheck (3292; 4096 at 3312); long_subscription_reviews_overlap_but_keep_complete_context_and_coverage (4593; 4096 at 4606); subscription_route_constrains_repairs_and_rechecks_without_changing_provider (5318; 0 and 4096 at 5339, 5343) |
| inference-core/src/grpc.rs | proto to provider mapping | to_internal_request 1062 (live 1072); copy at 1088 (live 1099); test budget 0 at 1542 (live 1553) | M (+131; no effort line changed) | internal_infer_scope_comes_only_from_verified_principal (1539) only; other coverage not determined |
| inference-core/src/provider/mod.rs | provider `InferRequest` (120, field 152); `ProviderCapabilities` (386); supports_thinking (422); feature_flags 669-673 | 120-152; 386-453; 664-676; capabilities() 719; supports_thinking true 781 | clean | capabilities_serialize_to_json (1197) |
| inference-core/src/provider/anthropic.rs | Anthropic thinking budget | floor 404; resolve 436-470; body 713-718; supports_thinking 934; tests from 1872 | clean | extended_thinking_tests (mod 1872): no_budget_requested_omits_the_parameter (1876), models_that_predate_thinking_omit_it (1884), a_budget_below_the_provider_floor_is_omitted_not_clamped (1919), a_thinking_delta_never_becomes_answer_text (1948) |
| inference-core/src/provider/openai.rs | OpenAI API and Azure builder; no effort | OpenAiFlavor 24-28; build_request_body 411-480; is_reasoning_model 560-563; supports_thinking 649 | clean | no effort test in file (budget 0 fixture at 1124) |
| inference-core/src/provider/codex_subscription.rs | Codex BrokerRequest effort and service tier | request_body 114; reasoning_effort 142; codex_reasoning_effort 155-161; codex_service_tier 166-176; BrokerRequest 178-193 | clean | maps_gateway_effort_to_codex_reasoning_effort (1004), broker_request_includes_reasoning_effort (1021); further body tests 1039-1068 (names not extracted) |
| inference-core/src/provider/routing_policy.rs | Budget, Balance and Genius default table | 136-160 | clean | value assertions from 293 |
| inference-core/src/provider/intent.rs | VerevonMode parse, classify, choose, resolve; cheap fallback | enum 29-35; parse_mode 63-67; CHEAP_FALLBACK 196; choose 201; resolve 236; header 3-11 | clean | choose_budget_prefers_cheap (773), choose_balance_scales_with_complexity (796), choose_genius_reaches_for_the_best (828), exhausted_budget_forces_cheap_fallback (851), constrained_budget_downgrades_one_tier (872), choose_honours_a_modified_table_cell (917) |
| inference-core/src/provider/logprobs.rs | precedent: reasoning models reject a parameter | test at 437 | clean | reasoning_models_do_not_accept_the_parameter (437) |
| inference-core/src/provider/fallback.rs | fallback chain; no non-test budget reference | fixtures 3007-4283 (all 0) | clean | fixtures only; how fallback calls carry budget not traced |
| execution-core/src/runtime_loop/agent.rs | Do loop `run_rounds`; budget forced to 0 | run_rounds 1168 (live 1172); budget 1332-1335 (live 1336-1339) | M (+200/-7; no effort line changed) | none pin thinking (no execution-core test references thinking budget) |
| proto/model_plane/v1/inference.proto | InferRequest contract; reasoning_delta | 100-175; budget 155-162; reasoning_delta 347 | clean | none (contract) |
| proto/model_plane/v1/gateway.proto | gateway request: features (19, line 244), min_privacy_tier (22, 257); no effort field | 241-257 | clean | none |
| proto/model_plane/v1/execution.proto | per-round privacy tier copy; no effort field | 118-124; 121; 205 | clean | none |
| proto/model_plane/v1/sessions.proto | unrelated; no effort, thinking or budget match | n/a | M (+42) | none |
| proto/buf.gen.yaml | codegen: Go, Python, TypeScript; Rust via build.rs | 3-7 comment | clean | none |
| docs/sse-event-taxonomy.md | reasoning_delta to reasoning event row | 30 | M (+4) | none |
| docs/decisions/ledger.md | settled history of the effort dial | 2385-2387 (zero writers, old); 2600-2606 (C2 fix); 2895-2899 (budget now wired) | clean | n/a |
| Frontend: src/shared/api/chat-client.ts | ChatInvokeRequest.effort; wire omit rule; Verevon mode catalogue | 61; 106-115; buildChatWireBody 1004; mapping 1096-1102; modes 800-835; default 812 | M (+53; no effort line changed; live 116, 1130) | see chat-client.test.ts |
| Frontend: src/shared/api/chat-client.test.ts | effort wire tests | describe 1281 (live 1303) | M (+22) | "omits the field entirely on an ordinary turn" (first it, ~1282 wt); "sends the profile when a non-default effort is chosen" (1293) |
| Frontend: src/shared/api/ag-ui-client.ts | AG-UI copy of effort | 266-280 (276) | clean | ag-ui-client.test.ts:22, 45 (names not extracted) |
| Frontend: src/features/chat/components/chat-types.ts | composer effort type (quick or deep) | 520-521 (live 524) | M (+7; no effort line changed) | none found |
| Frontend: src/features/chat/components/use-chat-controller.ts | effort passed into send payload and stream | 1904, 2551, 2890 | M (+4; no effort line changed) | none found |
| Frontend: src/features/chat/lib/pending-chat-launch.ts | persisted launch effort; normalizer | 49, 121, 144, 160-162 | clean | pending-chat-launch.test.ts:28, 38 (names not extracted) |
| Frontend: src/features/dashboard/home/DashboardComposer.tsx | response-mode control and effort mapping | 152, 154, 354-362, 2280, 3531, 3543-3554, 3830-3834 (live 154, 3560, 3838) | M (+7; no effort line changed) | none for effort in DashboardComposer*.test.tsx |
| Frontend: src/styles/global.css | effort select styling | 47493-47510, 47843, 47982 | M (+263; no effort line changed) | none |
| Frontend: src/shared/actions/ | action contracts | none (no effort symbol) | clean (home-updates.ts is M, unrelated) | none |
| Frontend: apps/gateway/src/domains/ag_ui.rs | AG-UI to model-invoke body normalizer | 469; test fn 1131 | clean | normalizes_tanstack_run_agent_input_to_model_invoke_body (1131; asserts 1166, 1188) |
| Frontend: apps/gateway/src/domains/chat/ | REST chat BFF | no effort symbol (history.rs `retrieval_scope` only) | clean | none |
| Ingestion: integration-corev2/internal/api/codex_subscription.go | HTTP body and mapping for Codex | struct 475; reasoningEffort 483 (live 491); mapping fn 339-349 (346, live 351) | M (+8; revocation lines; no effort line changed) | none for effort found in this file |
| Ingestion: integration-corev2/internal/codexsubscription/manager.go | broker InvokeRequest; low or high validation | struct 86-94 (92); prepareInvocation 409; validation 422-427 | clean | TestManagerRejectsUnsupportedReasoningEffort (manager_test.go:69); TestManagerRejectsUnsupportedServiceTier (85) |
| Ingestion: integration-corev2/internal/codexsubscription/appserver.go | app-server transport effort | WithThreadEffort 60; checkThreadEffort 431-440; threadStartParams 479-484; turnStartParams 503-506 | clean | TestThreadStartEffortSwitch (appserver_test.go:214); TestCheckThreadEffortLogsOnlyAMismatch (258); TestTurnStartParamsIncludesOptionalPriorityTier (294); effort-by-tier loop at 149-173 (test name not extracted); TestAppServerThreadEffortSwitchOnTheWire (appserver_process_test.go:374) |
| Ingestion: integration-corev2/internal/codexsubscription/direct.go | direct transport effort | requestBody 491; reasoning effort 497; lite context 506 | clean | TestDirectEffortMapping (direct_request_test.go:276); TestDirectServiceTier (304) |
| Ingestion: integration-corev2/internal/codexsubscription/direct_profiles.go | per-model Codex profiles (verbosity, tiers, lite) | 20-24 | clean | direct_request_test.go (above) |
| Ingestion: integration-corev2/.env.example | documents transport and thread-effort env | 33, 38 | clean | codex_subscription_compose_test.go:56, 92-96 |
| Ingestion: apps/Ingestion Plane/docker-compose.yml | `CODEX_APP_SERVER_THREAD_EFFORT` default true | 514 (live 536, same default expression) | M (+36/-14) | codex_subscription_compose_test.go |

## 4. Related contracts

- Proto. `InferRequest` fields 1-16 are in use; tag 17 is free (`inference.proto:100-175`). `InferChunk.reasoning_delta` is at 347. No effort enum exists in any proto. Rust bindings for these protos are generated at `cargo build` by `mp-contracts/build.rs` (per `buf.gen.yaml` comment 3-7; build.rs not opened).
- SQL migrations. The only Model Plane migrations directory is `apps/Model Plane/rust/services/session-core/migrations/`. Latest in worktree: `0043_audit_outbox_requeue.sql`. Live adds `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql` and `revert/` (all untracked). No effort persistence exists. The current design needs none; whether a per-model table should persist is not determined.
- Config and env. `CODEX_APP_SERVER_THREAD_EFFORT` (docker-compose 514 worktree / 536 live; `.env.example:38`; compose test `codex_subscription_compose_test.go:56, 92-96`). `CODEX_SUBSCRIPTION_TRANSPORT` (`.env.example:33`). No `*_EFFORT` env var exists in Model Plane rust or go (grep). Per user memory, do not add Control tokens to integration-corev2/.env.
- SSE. `reasoning_delta` maps to `reasoning` (`sse-event-taxonomy.md:30`). Feature gate: `gateway.proto:241-244`.
- Gateway routes. Model Plane: `/v1/invoke/stream` (`http_routes.rs:137`), `/v1/ai/chat` (`http_routes.rs:413`, handler `ai_chat` 2306; no thinking write in http_routes.rs). gRPC entry builds via `build_infer_request` (grpc.rs 765, 975), which sets 0. BFF: `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/ag_ui.rs:469`. REST chat BFF `domains/chat/`: no effort symbol.
- Action contracts. `apps/Frontend Plane/verevonv3/src/shared/actions/` has no effort symbol (grep). `docs/actions-surface-operations.md` has no effort or reasoning mention (grep).
- Codex broker JSON. `reasoningEffort` at `integration-corev2/internal/api/codex_subscription.go:483` (worktree) and `codexsubscription/manager.go:92`.
- Model catalogue. `chat-client.ts:800-835` (`VerevonMode`: verevon-budget, verevon-balance, verevon-genius; default at 812). Label source `src/shared/ai/model-selection.ts:27`.

## 5. Test and build commands

- Model Plane CI, from `.github/workflows/model-plane-rust-ci.yml` (worktree): working directory `apps/Model Plane/rust` (56-57); `cargo fmt --all -- --check` (99); `cargo check --workspace` (105); `cargo check --workspace --all-targets` (112); `cargo test --workspace --no-fail-fast` (119). Live CI file is M (+50, new `session-core-receipts` job, unrelated to effort).
- Per crate, from `apps/Model Plane/CLAUDE.md` Testing: `cargo test -q -p inference-core --all-targets`, `cargo test -q -p model-gateway --all-targets`, `cargo test -q -p execution-core --all-targets`. Crate names from `Cargo.toml`: `inference-core` (lib `inference_core`), `model-gateway`, `execution-core`, `mp-contracts`.
- Focused filters (suggested, not run): `cargo test -p inference-core --lib codex_reasoning_effort`; `cargo test -p inference-core --lib resolve_thinking_budget`; `cargo test -p model-gateway --lib thinking::`.
- Windows caveat: user memory note `model-plane-build-and-deploy.md` (indexed in MEMORY.md) says two cargo targets fail on Windows before this change; use `--lib`.
- Proto: `buf generate` in `apps/Model Plane/proto` (Model Plane CLAUDE.md, Build and Run). `buf lint` has pre-existing debt.
- Go, Ingestion broker: module at `apps/Ingestion Plane/integration-corev2` (go.mod). No Makefile in that directory (ls). Suggested: `go test ./internal/codexsubscription/... ./internal/api/... ./internal/config/...` and `go vet` on the same, per the root CLAUDE.md Go convention (not run).
- Frontend: `apps/Frontend Plane/verevonv3/package.json`. `pnpm test` (line 13: node --test scripts, then vitest run). `pnpm typecheck` (`tsc -b`, line 17). `pnpm lint` (`eslint .`, line 11). Focused: `pnpm vitest run src/shared/api/chat-client.test.ts src/features/chat/lib/pending-chat-launch.test.ts` (suggested).
- BFF gateway: crate `verevon-gateway-rs` (`apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml:2`). Root CLAUDE.md: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`.

## 6. Owners

- Model Plane owns reasoning, sessions, inference, execution loops and cost (repo-root `CLAUDE.md` Architecture Rules). Rule 7 in `apps/master-ownership-matrix.md:163` says reasoning is Model Plane-owned. Table row at 41 says the same.
- inference-core owns provider routing and inference: `apps/Model Plane/docs/capability-ownership-matrix.md:63`. Target row `apps/master-ownership-matrix.md:127`.
- execution-core owns the Do loop (`agent.rs`). Model Plane CLAUDE.md says the two tool-dispatch loops must stay separate.
- model-gateway owns budget derivation (`thinking.rs`) and the HTTP and SSE surface (`sse.rs`, `http_routes.rs`).
- Control Plane owns entitlements (repo-root `CLAUDE.md:23`). The card's per-role effort cap is therefore Control-owned. The two matrices do not name entitlements (grep: no match), so this is not confirmed in them.
- Cost and budget posture: `apps/master-ownership-matrix.md:88` (cost-core-go is ledger of record). Budget posture read in `intent.rs` 3-11 and `BudgetClient` 276.
- Ingestion Plane owns the integration-corev2 subscription broker (`CLAUDE.md`: provider actions go through integration-corev2's actions surface).
- Frontend Plane owns the composer, chat-client and gateway/BFF normalisation (`CLAUDE.md` Architecture Rules).
- Proto: the audit names an `execution.proto` steward (line 1534). Not identified in the matrices.

## 7. Uncommitted-only (live tree)

| path | live status | one-line change summary |
|---|---|---|
| apps/Model Plane/rust/services/model-gateway/src/sse.rs | M (+883) | Adds ZDR thinking budget (ZdrTurnSettings 8349, derivation 8399-8402, use 8444) and budget param to direct_infer_turn (7777, 7800). 12 effort or thinking diff lines. |
| apps/Model Plane/rust/services/model-gateway/src/http_routes.rs | M (+237) | `InvokeRequest.effort` moves to 6780-6785; adds `thinking_budget_tokens: settings.thinking_budget_tokens` in `invoke` (8986). 1 effort line. |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs | M (+200/-7) | Large Do-loop changes above 1332; thinking-off line moves to 1339; no effort line changed. |
| apps/Model Plane/rust/services/model-gateway/src/grpc.rs | M (+47) | No effort line changed; budget 0 moves to 450. |
| apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs | M (+425) | No effort line changed; budget 0 moves to 6759. |
| apps/Model Plane/rust/services/model-gateway/src/normalize.rs | M (+17) | No effort line changed. |
| apps/Model Plane/rust/services/inference-core/src/grpc.rs | M (+131) | No effort line changed; pass-through moves to 1099. |
| apps/Model Plane/proto/model_plane/v1/sessions.proto | M (+42) | No effort, thinking or budget symbol. |
| .github/workflows/model-plane-rust-ci.yml | M (+50) | New `session-core-receipts` Postgres job; unrelated. |
| apps/Model Plane/docs/sse-event-taxonomy.md | M (+4) | Not read in full; reasoning row at 30 (worktree). |
| apps/Ingestion Plane/integration-corev2/internal/api/codex_subscription.go | M (+8) | Revocation-acknowledgement lines on subscription removal; no effort line changed. |
| apps/Ingestion Plane/docker-compose.yml | M (+36/-14) | Same `CODEX_APP_SERVER_THREAD_EFFORT` default expression, now at 536. |
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts | M (+53) | No effort line changed (live 116, 1130). |
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.test.ts | M (+22) | No effort line changed; effort describe block shifts to 1303. |
| apps/Frontend Plane/verevonv3/src/features/chat/components/chat-types.ts | M (+7) | No effort line changed (live 524). |
| apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts | M (+4) | No effort line changed; zdr-refusal copy. |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx | M (+13) | No effort symbol in worktree copy. |
| apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx | M (+7) | No effort line changed (live 154, 3560, 3838). |
| apps/Frontend Plane/verevonv3/src/styles/global.css | M (+263) | No effort line changed. |
| Clean in live (not in status list) | clean | thinking.rs, openai.rs, anthropic.rs, codex_subscription.rs, provider/mod.rs, fallback.rs, routing_policy.rs, intent.rs, logprobs.rs, source_validation.rs, inference.proto, gateway.proto, execution.proto, ledger.md, capability-ownership-matrix.md, apps/master-ownership-matrix.md, CLAUDE.md, docs/actions-surface-operations.md, ag-ui-client.ts, pending-chat-launch.ts, ag_ui.rs, manager.go, appserver.go, direct.go, direct_profiles.go, .env.example |

Untracked (`??`) relevant items: none under the effort paths. Live untracked session-core migrations 0044 and 0045 exist (section 4). Other Ingestion `zdr_*` untracked files are unrelated.

## 8. Flags and risks (for the planning step)

1. Product decision: Do-run thinking is disabled on purpose. The code comment (worktree 1332-1334) says the effort dial is a chat-surface control and a deployed agent's budget is its round budget. Audit 10.2 #15 agrees. `execution.proto` 118-124 copies the privacy tier into each round but carries no effort, so Do support would need a proto field too.
2. Cross-plane constraint: the Codex broker accepts only low or high (`manager.go:422-427`). Any wider Codex ladder needs an Ingestion Plane change. The card does not mention this.
3. Hidden coupling: Codex service tier and effort thresholds are keyed to the literal 1024 (`codex_subscription.rs:156` `> 1_024`; `:167` `!= 1_024`). `thinking.rs:21` sets `QUICK_BUDGET_TOKENS = 1024`. Changing the quick budget or adding levels would silently change Codex tier and effort. No test ties the two modules together (not verified).
4. Capability mismatch: OpenAI-compatible capabilities declare `supports_thinking: true` (`openai.rs:649`), and `feature_flags()` (`provider/mod.rs:671-672`) then advertises `reasoning`. The request body never sends a budget. The card's claim holds for the OpenAI API and Azure flavours only. The Codex subscription route honours effort, so the card wording should say so.
5. Reasoning-model gate is prefix-based (`openai.rs:560-563`: gpt-5, o1, o3). o4 models would not match.
6. Second producer: `source_validation.rs:2277` forces 4096 for subscription source review, bypassing the user's effort. Its tests assert the literal. Fold it into the per-model table or keep it explicit.
7. Unknown values are silent: `thinking.rs:40-41` maps any unknown effort string to 0, and the AG-UI gateway forwards any string (`ag_ui.rs:469`). A typo silently disables thinking. The card's "clamp" design would need explicit validation.
8. Semantics differ. thinking.rs steps down by ceiling headroom (deep, then quick, then 0). Anthropic omits the budget entirely (`anthropic.rs:436-470`). Codex maps by threshold. The card's "nearest weaker supported level" would need one rule for all three.
9. Derivation is model-blind: the budget is computed at `sse.rs:2542` before any per-model step, so there is no per-model clamp today.
10. Uncommitted ZDR path: sse.rs and http_routes.rs now derive the budget a second time (`zdr_turn_settings`). Any ladder change must cover both derivations or ZDR turns will diverge.
11. Frontend type drift: the composer offers quick, auto (standard) and deep. `chat-types.ts:521` and `pending-chat-launch.ts:160-162` accept only quick and deep. `chat-client.ts:115` accepts three values. A six-level ladder needs changes in at least five frontend places.
12. Downstream of model resolution: intent modes (routing_policy 136-160) pick a model, and the budget then applies to whatever model resolves. Anthropic's "predates thinking" list is the only per-model gate on the budget.

## 9. NOT DETERMINED

- External "Reference practice" claims (Hermes EFFORT_LADDER, Codex per-model ladder, DeepSeek, T3 Code, "6 of 12"). No repo path.
- Whether `fallback.rs` carries the budget into fallback calls. Only test fixtures reference the field.
- Which builder `ai_chat` (`http_routes.rs:2306`) uses. No thinking write in http_routes.rs (worktree count 0).
- Whether the REST chat BFF route forwards the body verbatim (no effort symbol under `domains/chat/`).
- Enclosing test names for `appserver_test.go:149-173`, `ag-ui-client.test.ts:22, 45`, `pending-chat-launch.test.ts:28, 38`, `source_validation.rs:4641`, and codex body tests 1039-1068.
- Whether Control Plane holds a per-role effort cap or entitlement for effort. Only the repo-root CLAUDE.md:23 rule was found.
- Whether DEFAULT_FEATURES includes `reasoning` (chat-client.ts comment says so; not verified).
- Whether the Azure Foundry Claude path (`AZURE_ANTHROPIC_*` in inference-core config) reuses `anthropic.rs`.
- Whether `mp-contracts/build.rs` generates the Rust bindings (per buf.gen.yaml comment only).
- Test status: no build or test was run. Live-only ZDR changes in sse.rs and http_routes.rs have no tests checked.
- Whether a routing-policy override from the store changes the Budget, Balance or Genius table at runtime (not traced).
