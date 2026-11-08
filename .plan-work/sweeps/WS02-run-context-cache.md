# WS02-run-context-cache: code-map sweep

Date 2026-10-14. Items: F-t3-001 (one server-assembled run context for Do, Plan and scheduled runs; Priority, M, W1, Model Plane) and F-t3-022 (prompt-cache discipline; Priority, M, W1, Model Plane). Read-only sweep. No product file, build or test was run.

- **WT** = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` at afb83928, clean except untracked `.plan-work/`.
- **LIVE** = `C:/dev/CoresSystem`, same HEAD plus uncommitted work. Status re-checked with `git --no-optional-locks status|diff` for every path below. Agrees with `.plan-work/live-tree-status.txt` (324 entries, snapshot 20:56).
- **Audit** = `03-model-plane-chat-and-agents-audit.md`. F-t3-001 card L335-351. F-t3-022 card L665-681. Rows: sec 9.1 W1 L1534 and L1544; sec 9.2 L1667 and L1670; App B L1819 and L1839; App C L1922-1927 and L2095-2109. Appendix D has no entry for either ID.
- Two-loop rule kept: inline Ask loop (`model-gateway/src/sse.rs`, after the agentic early return at 1381) and the governed Do loop (`execution-core/src/runtime_loop/agent.rs` `run_rounds`) are not merged.

## 0. Findings

1. **Card line numbers are LIVE line numbers.** Drift is caused by uncommitted LIVE edits in `model-gateway/src/sse.rs` (883 lines changed), `sse_events.rs` (203) and `execution-core/src/runtime_loop/agent.rs` (207; +4 from 1168). Files clean in LIVE match the cards (exception: RunAgentRequest end line, sec 0.5).
2. **Citations:** 21 explicit path citations: 13 MATCH, 8 DRIFT (each matches LIVE at the cited lines), 0 ABSENT. 1 unresolved (audit sec 6.2, sec 2 row 13).
3. **ABSENT** (grep, both trees): lineage-scoped cache key; cache miss-reason field; prompt tier contract (stable, context, volatile) in `mp-contracts`; Anthropic two-block system split; todo recitation; run-record completeness flags; computed cache hit-rate (only design comments exist).
4. **PARTIAL:** compaction has a visible marker (`SUMMARY_PREFIX` compaction.rs:51, `DROPPED_HISTORY_NOTICE` :43, honest-marker test :1251) but no begin/commit bracket and no unmatched-start state. `transaction|bracket|unmatched` in model-gateway `compaction.rs` returns nothing.
5. **Card wording to correct:**
   - F-t3-022: the SPA metric is hidden, not "reads zero". `ChatPanels.tsx:935` wraps it in `Show when={cacheTotal() > 0}`.
   - F-t3-001 (ZDR): WT refuses agentic on ZDR at `sse.rs:986-993` and returns ZDR turns at `:1069`, before memory prefetch (`:1586`). LIVE now refuses through `crate::zdr_features::refusal` (live `sse.rs:1052-1053`; live-only `zdr_features.rs:305-306`) and marks memory "neither read nor written" on ZDR turns (`zdr_features.rs:166-167`).
   - `session-core/src/compaction.rs` is a periodic checkpoint-watermark job (header lines 1-9), not chat prompt compaction. The chat budget planner is `model-gateway/src/compaction.rs:197-259`. `capability-ownership-matrix.md:24` lists session-core `compaction.rs` under token efficiency, which is misleading.
   - `RunAgentRequest` spans `execution.proto:76-209` (card: 76-200).
6. **LIVE-only behaviour the cards do not mention:** ZDR persistence-free answers now compose authored instructions (live `sse.rs:8380-8405`, call at `:8385`, persona forced on). The Usage frame gains `retention_class` (live `sse_events.rs:283-285`). Neither tree adds cache fields to any frame.
7. **Migration number risk:** WT latest session-core migration is `0043_audit_outbox_requeue.sql`. LIVE has untracked `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql` and `migrations/revert/`. Any run-record flag column needs a number agreed against LIVE.
8. **Conflict:** `apps/Model Plane/go/go.work` exists (with `go.work.sum`). The user's Model Plane build memory says there is no go.work. Verify before running Go builds.

## 1. Citation table, F-t3-001

| # | Cited (card) | Claim | WT verdict | LIVE | WT corrected path:lines (LIVE lines) |
|---|---|---|---|---|---|
| 1 | model-gateway/src/sse.rs:6360-6420 | ADR-0003 composer on Ask turns | DRIFT | M; fn at 6375 (inside card range) | sse.rs:6243-6296 (doc 6215-6242) (6375) |
| 2 | sse.rs:1655 | memory prefetch, 400 ms bound | DRIFT | M; gate 1655, call 1658 | sse.rs:1586-1589 (gate and call); const MEMORY_PREFETCH_TIMEOUT :5036; fetch fn :5060 (LIVE: const 5167, fn 5191) |
| 3 | sse.rs:1405-1500 | agentic branch returns before context or memory | DRIFT (claim holds) | M; return at 1491 | sse.rs:1381-1446, return agentic_run_stream at :1423. Later: context load :1491, memory :1589, authored :1747 (LIVE 1560, 1658, 1855) |
| 4 | sse.rs:9118-9190 | build_run_agent_request sends content plus approved-plan block only | DRIFT | M; fn 9118, plan block 9138 | sse.rs:8758-8835 (goal prefix 8785-8788). Caller spawn_run_dispatch :8618, build call :8678, run_agent :8705 (LIVE spawn 8978) |
| 5 | proto/model_plane/v1/execution.proto:76-200 | RunAgentRequest has no history, instruction, persona or memory field | MATCH (message ends at 209) | clean | execution.proto:76-209. Fields 1-17 confirmed. plan_mode=13 (:143), space_id=14 (:168), provider_hint=16 (:201), subscription_connection_id=17 (:208) |
| 6 | execution-core/src/runtime_loop/agent.rs:1185-1226 | run_rounds seeds fresh preamble plus goal history | DRIFT (-4) | M; fn 1172, seed 1188 | agent.rs:1168 (run_rounds, fn 1168-2160); isolation comment 1181-1183; seed vec 1184-1197; skill insert 1199-1221 |
| 7 | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/ag_ui.rs:85-130 | BFF injects org, Space and persona on AG-UI request | MATCH | clean | ag_ui.rs:83-124 (calls 93, 101, 108, 113; route 58; fn 66) |
| 8 | apps/gateway/src/domains/chat/streams.rs:55-100 (resolved: apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/streams.rs) | BFF injects on chat ingress | MATCH | clean | streams.rs:55-92 (calls 57, 70, 82, 87; fn stream_chat :18; route chat.rs:17) |
| 9 | CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:109-110 (resolved: apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md) | rows B3 (persona, org, Space instructions into agentic runs) and B4 (hook_context) | MATCH | clean | same lines |
| 10 | no line: authored_instructions_message, load_context_assembly_messages, fetch_chat_memory_context, compaction.rs budget planner, space_id, plan_mode | named symbols | MATCH | see sec 3 | see sec 3 |

## 2. Citation table, F-t3-022

| # | Cited (card) | Claim | WT verdict | LIVE | WT corrected path:lines (LIVE lines) |
|---|---|---|---|---|---|
| 1 | inference-core/src/provider/anthropic.rs:823 | Anthropic cache read and write tokens parsed | MATCH | clean | anthropic.rs:823-830 (read 823, write 829); usage_tokens 835; streaming parse 1128-1195 |
| 2 | inference-core/src/provider/openai.rs:371-395 | OpenAI and Azure cache tokens parsed | MATCH (fn 380-397) | clean | openai.rs:369-397 (fn openai_cache_tokens 380-397) |
| 3 | go/services/cost-core/internal/postgres/store.go:153, 195-210 | cost-core persists and sums cache tokens | MATCH | clean | store.go:153 (INSERT columns); 195-210 (GetUsage SUM at 202-203); GetRunUsage :221; Aggregate :263 |
| 4 | ChatPanels.tsx:926-940 (resolved: apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPanels.tsx) | per-turn Prompt cache metric | MATCH (wording caveat, sec 0.5) | clean | ChatPanels.tsx:928-930 (cacheTotal); 935-937 (Show when > 0, Metric) |
| 5 | model-gateway/src/sse_events.rs:268-286 (resolved) | Usage frame carries no cache fields | DRIFT (claim MATCH) | M; Usage 268-286 incl. retention_class 283-285 | sse_events.rs:229-244 (Usage variant), 515-539 (payload), 410 (usage name). No cache key in either tree |
| 6 | execution-core/src/runtime_loop/agent.rs:1448-1466 | Do loop logs cache tokens per round | DRIFT (-4) | M; message 1466 | agent.rs:1450-1463 (info!; cache fields 1458-1459). Test each_round_logs_its_token_usage_and_never_its_content at :8698 |
| 7 | model-gateway/src/prompt_context.rs:1-60 | stable-prefix order, off by default | MATCH | clean | prompt_context.rs:1-19 (header), env flags :26, :30, :35; CacheOptions :91; prompt_order :153; PromptOrder :182; PromptContext :198; render_stable_prefix fn :305 |
| 8 | inference-core/src/provider/anthropic.rs:296-345 | Anthropic: system prompt joined into one cached block | MATCH (inside fn) | clean | apply_prompt_caching fn 284-376: tools marker 296-313; single system block 315-333; messages marker 335-361. Join at build_request_body_with_context_management 680-683 and 727-728 |
| 9 | CHAT_PARITY_AUDIT_2026-10-01.md:110 (resolved: apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_PARITY_AUDIT_2026-10-01.md) | todo or plan recitation, "not built" | MATCH | clean | same line |
| 10 | agent.rs:1185-1226 (fold, "append-only") | duplicate of F-t3-001 row 6 | DRIFT (-4) | M | agent.rs:1181-1222 |
| 11 | inference-core/src/cache.rs:1-10 | response cache excludes ZDR | MATCH | clean | cache.rs:1-6 (module doc), 99-101 (get guard), 118-120 (put guard), test :245. The gateway `apps/Frontend Plane/verevonv3/apps/gateway/src/cache.rs` is a Dragonfly result cache with no ZDR text |
| 12 | CACHE_COMPACTION_AUDIT_2026-09-17.md:85-100 (resolved: apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CACHE_COMPACTION_AUDIT_2026-09-17.md) | PromptCache not on the chat path; prompt-hash drift | MATCH | clean | same lines. Code check: fallback.rs `cache.get` :1160 and `cache.put` :1206 inside infer() (:1134). infer_stream (:1526 to about :1600) has no cache reference |
| 13 | audit "section 6.2" (F-t3-022 how-to, gateway-signalled boundary) | not a code cite | NOT DETERMINED | n/a | Audit L1272-1280 lists F-t3-070 to 073 only. No boundary text seen |

## 3. Named symbols and absence checks

| Claim | WT result | LIVE | Evidence |
|---|---|---|---|
| authored_instructions_message | MATCH: fn :6243, call :1747 | fn 6375; call 1855; ZDR use 8385 (LIVE only) | sse.rs |
| load_context_assembly_messages | MATCH: fn :4677, call :1491 | fn 4808; call 1560 | sse.rs |
| fetch_chat_memory_context | MATCH: fn :5060, call :1589 | fn 5191; call 1658 | sse.rs |
| MEMORY_PREFETCH_TIMEOUT (400 ms) | MATCH :5036 | 5167 | sse.rs |
| memory prefetch ZDR guard | No ZDR text in `memory_prefetch.rs` or sse.rs 5036-5200. ZDR turns return earlier (sse.rs :976-1072) | live `zdr_features.rs:166-167` (untracked) | sse.rs, zdr_features.rs |
| compaction.rs budget planner | MATCH: BUDGET_* consts :197-213; plan_head_summary_by_budget :253-259; plan_by_budget :261 | clean | model-gateway compaction.rs |
| space_id and plan_mode | MATCH: proto :168 and :143 | clean | execution.proto |
| RunAgentRequest literal sites | 27: sse.rs 6, execution-core agent_route.rs 4, grpc.rs 3, runtime_loop/agent.rs 13, model-gateway tests/e2e_invoke_chain_test.rs 1. Adding fields breaks these unless `..Default::default()` | mixed | grep |
| Scheduled-run entry | ExecuteScheduledStepRequest execution.proto:48-64; execution-core grpc.rs and scheduled_step_decision.rs | clean | proto, grep |
| Anthropic two-block system split | ABSENT: one system block at anthropic.rs:315-333 | clean | grep |
| Lineage-scoped cache key | ABSENT. Key = blake3(tenant scope, model, messages, response-shaping), inference-core cache.rs:4-5, `cache_key` :67-68 | clean | cache.rs |
| Cache miss-reason field | ABSENT (no miss_reason, miss reason match) | clean | grep |
| Prompt tier contract in mp-contracts | ABSENT (no PromptTier, prompt_tier match) | mp-contracts M | grep |
| Append-only tail, todo recitation | ABSENT (no "recit" match). CHAT_PARITY:110 says "not built" | clean | grep |
| Todo store | MATCH: append_todo :575, update_todo_status :604, list_todos_by_plan :618 | M (8 changed); LIVE adds list_todos_by_plan_full at 1175 | session-core orchestration_store.rs |
| Compaction bracket and marker | PARTIAL: marker SUMMARY_PREFIX :51, DROPPED_HISTORY_NOTICE :43; no bracket or unmatched-start | clean | compaction.rs |
| Native compaction checkpoint | PRESENT: anthropic_compaction_tests.rs (5 tests); session_flow.rs:1650 test | session_flow M (5 changed) | grep |
| Context inspector route | MATCH: model-gateway http_routes.rs:153-159 `GET /v1/threads/:thread_id/context` (LIVE :159); BFF chat.rs:54; json_handlers.rs:319-335 | http_routes M (237 changed) | routes |
| Cost hit-rate view | Token fields only: cost-core server.go routes :141-146; JSON cache keys :65-66, :82-83, :109-110; no ratio. "hit rate" matches are comments only: provider/mod.rs:253, :301; model-gateway http_routes.rs:9262; grpc.rs:553; inference-core cache.rs:60 | clean | grep |
| Run-record completeness flags | ABSENT. "completeness" matches are unrelated (source_validation.rs, tool_loop.rs, memory_erasure.rs, source_review_live.rs) | n/a | grep |

## 4. Affected-file map

| Path | Role | WT lines | LIVE | Tests (existing) |
|---|---|---|---|---|
| model-gateway/src/sse.rs | Ask path: ZDR branch 976-1072; agentic early return 1381-1446; CacheOptions 1486; context and memory 1491-1589; PromptContext 1575; authored 1747; stable order 1963-1971, 2649, 2801; spawn_run_dispatch 8618 (build call 8678); build_run_agent_request 8758-8835; agentic_run_stream 9167-9765 | M (883 changed) | Authored and persona: :10756, :10784, :10803, :10815, :10842, :10853, :10862 (fn lines). Approved plan: :10402, :10542. Usage cache telemetry: :11251 usage_envelope_carries_cache_token_telemetry. Context block: :11650, :11730 |
| model-gateway/src/prompt_context.rs | PromptContext, CacheOptions, PromptOrder, render_stable_prefix | clean | the_stable_prefix_order_puts_standing_blocks_first_and_per_turn_blocks_last; a_later_turn_repeats_the_earlier_prefix_byte_for_byte; every_option_defaults_off_and_only_applies_on_an_eligible_route; only_a_pinned_openai_chat_completions_route_is_eligible; both_orders_send_the_same_messages; the_legacy_order_is_byte_identical_to_the_old_insertions |
| model-gateway/src/compaction.rs | Chat budget planner 197-259; SUMMARY_PREFIX :51; DROPPED_HISTORY_NOTICE :43; summary_prompt_with_memory :343; SUMMARY_STABILITY_CLAUSE :371; apply_head_summary :399-422 | clean | both_summary_prompt_variants_carry_the_stability_clause; a_fresh_budget_compaction_does_not_trigger_again_on_the_next_turn; a_stored_summary_is_reused_byte_for_byte_without_a_new_summary_call; a_failed_summary_degrades_to_truncation_with_an_honest_marker (:1251); leading_system_messages_are_never_compacted_by_budget |
| model-gateway/src/memory_prefetch.rs | is_trivial_prompt gate (used at sse.rs:1586); no ZDR text | clean | not collected |
| model-gateway/src/sse_events.rs | ChatEvent::Usage 229-244 and payload 515-539; name map: usage :410, memory_recall :413, memory alias :364 | M (203 changed) | usage_carries_server_stamped_provenance_and_omits_it_when_empty (:1692). No cache-field test |
| model-gateway/src/http_routes.rs | Context inspector route 153-159 | M (237 changed) | not collected |
| model-gateway/src/zdr_precheck.rs | ZDR precheck | M (284 changed) | not collected |
| model-gateway/src/session_flow.rs | Native checkpoint and ZDR | M (5 changed) | native_checkpoint_survives_durable_metadata_encoding_and_respects_zdr (:1650) |
| model-gateway/src/pricing.rs | Cache-read discount rates (doc :29-31) | clean | not collected |
| model-gateway/src/langcache.rs | Doc-only tombstone (126 lines). Records why an earlier prompt cache was removed | clean | none |
| model-gateway/src/grpc.rs | gRPC Invoke path (memory twin); cache-hit comment :553 | M (47 changed) | not collected |
| execution-core/src/runtime_loop/agent.rs | run_rounds 1168-2160; fresh history 1184-1197; skills 1199-1221; per-round usage log 1450-1463 | M (207 changed) | each_round_logs_its_token_usage_and_never_its_content (:8698); subagent_runs_a_real_nested_loop_in_an_isolated_context_and_returns_its_answer (:10015) |
| execution-core/src/grpc.rs | run_agent handler 1203 (WT) | M (98 changed) | a_waived_subscription_run_is_admitted_only_on_its_own_connection |
| execution-core/src/agent_route.rs | Route admission (is_waived, refused) | M (285 changed) | a_waived_run_on_its_own_route_is_admitted_and_marked_waived |
| execution-core/src/compaction_budget.rs | Tier-1 stale tool-result clearing, clear_stale_tool_results :111 | clean | in-file tests at :150-203 |
| execution-core/src/scheduled_step_decision.rs | Scheduled-step decision | clean | not read |
| execution-core/tests/runtime_loop_test.rs | Loop integration | clean | compaction_triggers_when_output_large (:217) |
| inference-core/src/provider/anthropic.rs | apply_prompt_caching 284-376; system block 315-333; system join 680-683, 727-728; native context management 519-613; cache tokens 823-852 | clean | zdr_request_must_not_send_cache_control_anywhere_in_the_body; large_non_zdr_request_marks_tools_system_and_the_last_message; breakpoint_count_never_exceeds_anthropics_limit_of_four; prompt_below_the_minimum_cacheable_length_is_not_marked; input_tokens_still_reports_the_whole_input_when_the_prefix_was_cached; build_request_body_hoists_system_and_omits_temperature |
| inference-core/src/provider/openai.rs | openai_cache_tokens 380-397; stream cache legs 804-975 | clean | cache_tokens_read_the_plain_openai_shape; cache_tokens_read_the_gpt_5_6_write_leg; cache_tokens_fall_back_to_the_responses_api_details; missing_or_null_cache_details_count_as_zero; cache_legs_never_exceed_the_prompt_they_are_part_of; unary_infer_reports_both_cache_legs; a_stream_without_done_still_reports_the_cache_legs |
| inference-core/src/provider/anthropic_compaction_tests.rs | Native compaction checkpoint | clean | checkpoint_round_trip_preserves_original_history_pins_and_follow_up; unary_checkpoint_is_separate_and_compaction_cost_is_counted_once; streamed_checkpoint_never_becomes_answer_text_and_returns_on_final_chunk |
| inference-core/src/cache.rs | Response cache, ZDR-excluded; cache_key :67 | clean | zdr_request_never_persists_or_reads_durable_state (:245); an_identical_prompt_from_another_org_does_not_hit; tests/cache_test.rs: hit_after_put, tools_change_the_cache_key |
| inference-core/src/provider/fallback.rs | cache.get :1160 and cache.put :1206 in infer() (:1134); infer_stream (:1526) has no cache reference | clean | not collected |
| inference-core/src/grpc.rs | gRPC handlers; usage reporting path | M (131 changed) | added lines are refusal-catalogue tests, no cache lines |
| session-core/src/grpc.rs | GetContextAssembly handler; start_run_inner; count_waived_runs | M (861 changed) | zdr_receipt_a_zdr_owner_may_archive_and_delete_their_threads; update_thread_presentation_lets_a_zdr_owner_set_the_controls_but_not_persist_text |
| session-core/src/orchestration_store.rs | Todos: append_todo :575, update_todo_status :604, list_todos_by_plan :618 | M (8 changed) | not collected |
| session-core/src/compaction.rs | Checkpoint-watermark job (header 1-9); compact_once :170 | clean | not collected |
| session-core/src/dreaming.rs | Memory extraction | M (52 changed) | not collected |
| session-core/migrations/0001_init.sql | runs table (CREATE TABLE runs) | clean | n/a |
| session-core/migrations/0042_thread_persona_toggle.sql | Per-thread persona override | clean | n/a |
| proto/model_plane/v1/execution.proto | RunAgentRequest 76-209 | clean | none for RunAgentRequest (Go wire goldens cover RunAgentResponse only) |
| proto/model_plane/v1/inference.proto | ChatMessage :198 (compaction_summary :210); InferResponse :256 (cache_read :299, compaction_summary :301); InferChunk :310 (cache_read :361, cache_creation :362, compaction_summary :364) | clean | n/a |
| proto/model_plane/v1/sessions.proto | GetContextAssembly rpc :71; request :725 | M (42 changed) | n/a |
| go/services/cost-core/internal/postgres/store.go | RecordEntry :133-153; GetUsage :195-210; GetRunUsage :221; Aggregate :263 | clean | store_integration_test.go: TestPostgresLedger_Integration (:29, Postgres) |
| go/services/cost-core/internal/ledger/ledger.go | Entry cache fields :93-101; validation :142-144; Usage roll-up :202-205 | clean | ledger_test.go:37 TestStore_RecordAndGetUsageRollsUpCacheTokens |
| go/services/cost-core/internal/server/server.go | Routes :140-149; JSON cache keys :65-66, :82-83, :109-110 | clean | server_test.go:115 TestRecordThenUsageWithCacheTokenTelemetry; :156 TestRecordUsageDirectAppliesCacheReadDiscount |
| go/services/cost-core/migrations/0004_cache_tokens.up.sql | ALTER TABLE cost_entries, add cache columns | clean | n/a |
| rust/crates/mp-contracts/src/lib.rs, dataplane_posture.rs | Shared contracts crate; ZDR posture | M (2 and 67 changed) | waived-turn tests in dataplane_posture.rs |
| apps/Model Plane/deploy/docker-compose.yml | Env passthrough 415-417 | clean | n/a |
| Frontend src/features/chat/components/ChatPanels.tsx | SPA metric 928-937 | clean | none assert it (ChatPanels.test.tsx:607 cache match is an unrelated comment; ChatMessages.metrics.test.tsx has none) |
| Frontend src/shared/api/chat-client.ts | usage case :1355 and :1600; cache parse :1363-1364, :1608-1609 | M (53 changed; no cache lines added) | chat-client.test.ts (M) |
| Frontend src/shared/api/ag-ui-client.ts | cache parse :476-477, :659-660 | clean | none found |
| Frontend src/shared/chat/verevon-ui-events.ts | cache mention | clean | n/a |
| BFF apps/gateway/src/domains/ag_ui.rs | Stream route :58; handler :66; injection :83-124 | clean | normalizes_tanstack_run_agent_input_to_model_invoke_body (:1131) |
| BFF apps/gateway/src/domains/chat/streams.rs | stream_chat :18; injection :55-92 | clean | not collected |
| BFF apps/gateway/src/domains/chat.rs; chat/json_handlers.rs | Routes :17, :18, :54; context inspector upstream :319-335 | clean | not collected |
| BFF apps/gateway/src/domains/spaces.rs | inject fns :54, :4291, :4366, :4399; Space context route :665 | clean | not collected |
| BFF apps/gateway/src/cache.rs | Dragonfly result cache (not the response cache) | clean | n/a |
| Frontend src/shared/actions/action-registry.ts | spaceUpdateInstructionsInput :271-279 (Space instructions, max 4000) | clean | n/a |
| apps/AUTHORED_INSTRUCTIONS_ADR_2026-08-19.md | ADR-0003 (cited at sse.rs:1746) | clean | n/a |

## 5. Related contracts

- **Proto.** `execution.proto` RunAgentRequest: highest tag 17 (:208). Next free tag is 18 unless reserved ranges exist (not checked). `plan_mode`=13 (:143), `space_id`=14 (:168). `mp-contracts/build.rs` compiles sessions, inference and execution protos (lines 41, 47, 48).
- **Proto gates.** `.github/workflows/buf.yml:21` `buf lint apps/Model Plane/proto`; `:23` `buf breaking ... --against apps/Model Plane/proto/baseline.binpb`.
- **Go wire goldens.** `go/gen/model_plane/v1/proto_wire_parity_execution_test.go` covers RunAgentResponse only (:40, :72, :115).
- **SSE events.** `usage` (sse_events.rs:410 WT; :472 LIVE), `memory_recall` (:413), `memory` alias (:364). Freshness gate: `.github/workflows/sse-taxonomy.yml:45` runs `apps/Model Plane/scripts/gen-sse-event-taxonomy.py --check`. `apps/Model Plane/docs/sse-event-taxonomy.md` is M in LIVE (4 changed).
- **SQL.** cost-core migrations latest `0010_billing_usage_outbox.{up,down}.sql`; `0004_cache_tokens.{up,down}.sql` adds cache columns to `cost_entries`. session-core latest tracked `0043_audit_outbox_requeue.sql` (43 files). LIVE untracked `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `migrations/revert/`.
- **Env.** `MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER`, `MODEL_GATEWAY_COMPACTION_TOKEN_BUDGET`, `MODEL_GATEWAY_DEDUP_CONTEXT_HISTORY` (prompt_context.rs:26, :30, :35; passed through `deploy/docker-compose.yml:415-417` with empty defaults). `MEMORY_PREFETCH_TIMEOUT` is a code constant (sse.rs:5036), not an env var. Native context management flag: anthropic.rs:519-532.
- **Gateway routes.** BFF: `POST /api/v1/ag-ui/stream` (ag_ui.rs:58), `POST /api/v1/chat/stream` (chat.rs:17), `POST /api/v1/chat/invoke` (chat.rs:18), `/api/v1/chat/threads/{thread_id}/context` (chat.rs:54). model-gateway: `GET /v1/threads/:thread_id/context` (http_routes.rs:159). cost-core: `GET /api/v1/usage` (server.go:141), `/api/v1/cost/run` (:142), `/api/v1/cost/aggregate` (:143), `/api/v1/cost/entries` (:146).
- **Action contracts.** `src/shared/actions/action-registry.ts:271-279` (Space instructions input, max 4000). No run-context or cache-metric action found. CLAUDE.md requires an action contract before wiring a meaningful UI operation.
- **Context assembly RPC.** `sessions.proto` GetContextAssembly :71 and GetContextAssemblyRequest :725 (WT) / :736 (LIVE).

## 6. Test and build commands

- **Rust, Model Plane workspace** (`apps/Model Plane/rust`). From `.github/workflows/model-plane-rust-ci.yml`: `cargo fmt --all -- --check` (:99), `cargo check --workspace --all-targets` (:112), `cargo test --workspace --no-fail-fast` (:119). Per package (names from Cargo.toml): `cargo test -p model-gateway`, `-p inference-core`, `-p execution-core`, `-p session-core`, `-p mp-contracts`. Narrow with a test-name filter, for example `cargo test -p inference-core cache`. User memory note: two cargo targets fail pre-existing on Windows; use `--lib`.
- **Contract goldens.** `bash scripts/tests/zdr-contract-sync.test.sh` (CI :95-96; the script is M in LIVE).
- **Go, cost-core.** Module `github.com/triodelab/model-plane/services/cost-core` (go.mod). `go test ./...` inside `apps/Model Plane/go/services/cost-core` (CLAUDE.md Testing). `-race` needs cgo, which is unavailable per user memory. Resolve the go.work conflict first (sec 0.8).
- **Proto.** `buf lint apps/Model Plane/proto`; `buf breaking apps/Model Plane/proto --against apps/Model Plane/proto/baseline.binpb` (buf.yml:21, :23).
- **SSE taxonomy.** `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (sse-taxonomy.yml:45).
- **Stack.** `apps/Model Plane/scripts/compose.sh up -d` (script exists; user memory recipe). session-core DB tests need Postgres. A LIVE-only harness `plans/system-audits/program-2026-10-03/run_session_db_tests.sh` exists untracked and was not read.
- **Frontend, verevonv3** (`apps/Frontend Plane/verevonv3`). `package.json:13` test = node --test gateway scripts, then `vitest run`; `:17` typecheck = `tsc -b`; `:11` lint = `eslint .`. Targeted: `pnpm vitest run src/features/chat/components/ChatPanels.test.tsx src/shared/api/chat-client.test.ts`. User memory: the :5173 dev server is the main checkout, and typecheck, lint and vitest have pre-existing failures.
- **BFF gateway Rust.** `apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml` (package `verevon-gateway-rs`). From the verevonv3 root: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (CLAUDE.md Build And Run).

## 7. Owners

- **Model Plane** owns sessions and runs, inference, execution loops and cost (CLAUDE.md Architecture Rules; `apps/master-ownership-matrix.md:41`).
- **model-gateway**: public invoke and stream, composition, ZDR and cross-plane guards (master :125). Ingress caches must defer to durable owners (`apps/Model Plane/docs/capability-ownership-matrix.md:37`).
- **session-core**: threads, runs, checkpoints, memory index, compaction, context assembly (master :126; capability :56). Plans, steps and todos live in `orchestration_store.rs` (capability :55).
- **inference-core**: provider routing, streaming, prompt cache (master :127; capability :63).
- **execution-core**: tool loop, hooks, subagents (master :128). Delegated loops never re-resolve Space data (card F-t3-001).
- **cost-core**: ledger of record (master :88 and :133; capability :74). Rule 16 (master :175): every paid provider call is metered.
- **ZDR.** Rule 4 (master :160): ZDR propagates. Rule 17 (master :177): waiver enforcement in model-gateway, inference-core and execution-core, with the receipt in session-core. Memory prefetch and context composition on a ZDR turn must honour rule 4.
- **Frontend Plane** owns the verevonv3 UI and same-origin gateway or BFF normalization (CLAUDE.md). The BFF performs the injection (ag_ui.rs:93-117; streams.rs:57-87), which matches the card's "the BFF resolves" claim.

## 8. Uncommitted-only (LIVE)

**Modified (M), relevant. Diff stat from `git --no-optional-locks diff --stat`, plus added-symbol summary where available.**

| Path | Stat | One-line change |
|---|---|---|
| model-gateway/src/sse.rs | 883 lines changed | Adds ZDR direct-answer path (`prepared_zdr_refusal_stream`, `direct_infer_turn`, `zdr_turn_settings`) and composes authored instructions into ZDR answers (8385). Shifts WT anchors by +68 to +360 |
| model-gateway/src/sse_events.rs | 203 changed | Adds `RetentionClass` enum, `retention_class` (:283-285) and `retention_reason` on Usage. No cache fields |
| execution-core/src/runtime_loop/agent.rs | 207 changed | Adds waived-route tests and helpers (plain_caller, real_waived_rules_for, lineage test). Seed shifted +4; seed content not inspected |
| execution-core/src/grpc.rs | 98 changed | Adds waived-subscription route tests (route_caller) |
| execution-core/src/agent_route.rs | 285 changed | Adds waiver admission (is_waived, waived_caller, refused, refusal_of) and tests |
| execution-core/src/runtime_loop/session_core_rules_double.rs | 537 changed | Test double gains ZDR waiver claim parsing (ZdrWaiver, WaiverClaim, parse_zdr_waiver, Receipt) |
| execution-core/src/runtime_loop/mod.rs | 18 changed | Not read |
| inference-core/src/grpc.rs | 131 changed | Added lines are refusal-catalogue tests; no cache lines |
| model-gateway/src/tool_loop.rs | 425 changed | Rerank and retrieval notice events (retrieval_notice_events, rerank_gate_sentence, knowledge_search_on_turn); not cache |
| model-gateway/src/zdr_precheck.rs | 284 changed (mostly deletions) | ZDR precheck rewritten; not read in detail |
| model-gateway/src/http_routes.rs | 237 changed | Not summarised; no new function names in added lines |
| model-gateway/src/grpc.rs | 47 changed | Not summarised |
| model-gateway/src/session_flow.rs | 5 changed | Not summarised |
| session-core/src/grpc.rs | 861 changed | Adds count_waived_runs (WaivedRunCount), SessionRetentionGrpc, ZDR receipt tests; start_run_inner added or changed (pub(crate)) |
| session-core/src/orchestration_store.rs | 8 changed | Not summarised |
| session-core/src/dreaming.rs | 52 changed | Not summarised |
| proto/model_plane/v1/sessions.proto | 42 changed | Adds rpc CountWaivedRuns and messages CountWaivedRunsRequest, CountWaivedRunsResponse, WaivedRunCount. Shifts GetContextAssemblyRequest 725 to 736 |
| mp-contracts/src/lib.rs | 2 changed | Module exports |
| mp-contracts/src/dataplane_posture.rs | 67 changed | Waived-turn filtering tests and posture modes |
| docs/sse-event-taxonomy.md | 4 changed | Taxonomy lines |
| docs/ZDR.md | 64 changed | Not summarised |
| Frontend src/shared/api/chat-client.ts | 53 changed | Adds parsing; cache parse lines (:1363-1364, :1608-1609) are not among added lines |
| Frontend src/features/chat/components/ChatMessages.tsx | 229 changed | Not cache-related per added-line grep |
| Frontend chat-normalizers.ts, chat-types.ts, use-chat-controller.ts, shared/api/sse.ts | 39, 7, 4, 4 changed | Not summarised; no cache lines added |
| .github/workflows/model-plane-rust-ci.yml | 50 changed | Not read; contract-golden step lines may have shifted |

**Untracked (??), live-only, relevant:**

- session-core/migrations/0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql, migrations/revert/ (number collision, sec 0.7)
- session-core/src/retention_receipt.rs
- model-gateway/src/zdr_features.rs (365 lines; ZDR refusal catalogue; agentic refused at :305-306; memory Inert at :166-167), zdr_posture.rs, relevance_calibration.rs (with calibration/, tests/zdr_posture_matrix.rs, tests/zdr_feature_table.rs, tests/zdr_refusal_inventory.rs, tests/fixtures/)
- mp-contracts/src/retention.rs, zdr_refusal.rs; tests/zdr_retention_matrix.rs; tests/fixtures/
- execution-core/tests/zdr_refusal_inventory.rs; inference-core/tests/zdr_refusal_inventory.rs; inference-core/src/grpc/zdr_waiver_route_tests.rs
- api/contracts/zdr-refusals.json (+ .sha256)
- Frontend src/shared/api/zdr-refusals.ts (+ tests); chat-client.zdr-refusal.test.ts; http.zdr-refusal.test.ts; chat-normalizers.zdr-refusal.test.ts; use-chat-controller.zdr-refusal.test.ts; shared/api/contracts/ directory
- Frontend plans/system-audits/program-2026-10-03/run_session_db_tests.sh (test harness)

**Clean in LIVE (identical to HEAD):** execution.proto, inference.proto, cost-core (all), compaction.rs (model-gateway and session-core), prompt_context.rs, memory_prefetch.rs, langcache.rs, inference-core cache.rs, provider/anthropic.rs, provider/openai.rs, provider/fallback.rs, anthropic_compaction_tests.rs, execution-core compaction_budget.rs, run_budget.rs, runtime_loop_test.rs, ChatPanels.tsx and its tests, ag-ui-client.ts, verevon-ui-events.ts, all BFF `domains/*` files, gateway cache.rs, action-registry.ts, deploy/docker-compose.yml, scripts, go.work, buf files, sse-taxonomy workflow, both CHAT_* plan docs, CACHE_COMPACTION audit, AUTHORED_INSTRUCTIONS ADR, master and capability ownership matrices.

## 9. Not determined

1. LIVE line numbers for WT-only anchors not re-derived: session-core grpc.rs GetContextAssembly handler, execution-core grpc.rs run_agent, chat-client.ts cache lines.
2. Whether session-core SearchMemory or memory recall honours ZDR. The prefetch code has no ZDR text, and LIVE `zdr_features.rs` marks memory Inert on ZDR turns (live behaviour, untracked file not fully read).
3. Whether the LIVE ZDR direct-answer authored-instruction composition (sse.rs:8385) has tests. Not read.
4. Audit sec 6.2 "gateway-signalled boundary" (F-t3-022 how-to) is not found at audit L1272-1280.
5. Run-record completeness flags: no existing field found. Whether the session-core runs JSON columns could carry them was not checked.
6. Whether `anthropic_compaction_tests.rs` is compiled under `cargo test`. Not checked.
7. go.work conflict between the file and the user memory note.
8. `fallback.rs` `infer_stream` closing line (~1600) is inferred from indentation, not verified.
9. Whether the live `inference-core/src/grpc.rs` (+131) changes usage or cache reporting. Only refusal-catalogue lines were visible in added lines.
10. Test names not collected for memory_prefetch.rs, http_routes.rs, zdr_precheck.rs, session-core grpc.rs, cost-core server tests beyond those listed.
11. No build or test was executed. Counts and pass or fail status are unknown.
