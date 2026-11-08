# WS15-run-lifecycle-events - Typed run lifecycle events and status cues

Item: F-t3-049 (Recommended, L, W2, Model Plane + Frontend Plane). Notation: WT = worktree at HEAD afb83928; LIVE = `C:/dev/CoresSystem` (HEAD + uncommitted). `MP` = `apps/Model Plane`, `MG` = `MP/rust/services/model-gateway/src`, `EC` = `MP/rust/services/execution-core/src`, `SC` = `MP/rust/services/session-core/src`, `FE` = `apps/Frontend Plane/verevonv3`. Line numbers are LIVE unless marked WT. `[live-only]` = exists only as uncommitted live work.

## 1. Scope, items resolved, owner

**Item.** F-t3-049. **Owner.** Model Plane (model-gateway emits; execution-core and session-core carry the Do-loop signal) and Frontend Plane (renders; owns thread-status derivation). Connector incidents are not on the critical path (see S9 and D5).

**IN**

| Slice | Tag | What |
|---|---|---|
| S1 | [BUILD] | Closed vocabulary (`mp-contracts`), three new ChatEvent variants (`retry_scheduled`, `compaction`, `context_change`), gate-complete SPA stubs, AG-UI mapping, regenerated taxonomy doc |
| S2 | [FINISH] | Emit the silent context changes that already happen: history shed on overflow, compaction outcome (summarized / reused / truncated), skill guidance truncated, Do-run tool-result clearing |
| S3 | [BUILD] | Retry visibility in both tool loops (gateway `tool_loop.rs`, execution-core `agent.rs`), loops kept separate |
| S4 | [FINISH] | Post-hoc provider wait from fields that already exist (`ProviderTiming.retry_wait_ms`, `failed_attempts_ms`); no proto change |
| S5 | [BUILD] | SPA status cues: lifecycle node, retry cue in `deriveStreamActivity`, live-region line; also wires the unrendered `retrieval_notice` |
| S6 | [BUILD] | One shared thread-status function with fixed precedence plus unseen-completion marker (replaces four vocabularies) |
| S7 | [FINISH] | Promote WS03's raw `run_parked` frame to a typed ChatEvent (conditional on WS03 S8) |
| S8 | [BUILD, optional] | Counts-only lifecycle notes persisted on the assistant message so a reload still explains the answer (D4) |
| S9 | [BUILD, optional, recommend hand-off to WS23] | Connector incident ring (redacted) |

**OUT (explicit).** `turn_end` event (the card says normalise `done`/`stopped`/`error` client-side; already true, see 2.1). Trigger-kind chip (needs a run-level origin field that no proto has; WS20/F-t3-064 own it; D7). Server-side read state, notifications, push, inbox (WS16). Live, pre-first-token inference retry/rate-limit signalling (needs `infer_stream` restructuring; D2). Approval cards (WS13), ask-user (WS14), cost meter (WS27), the Trace `message.delta` collapse (WS09 D8 stays there). No new persistence in S1-S7: every event is an ephemeral stream frame.

## 2. Verified current state

### 2.1 ChatEvent, gate, relay

- LIVE `MG/sse_events.rs`: `enum ChatEvent` at :144 (23 variants; WT has 21, `RetrievalNotice` :368 and `ZdrError` :384 are live-only). `family()` :410 is exhaustive, no wildcard; control events (family `None`) :430-443; `should_emit` :451; `name()` :460 (`Error`/`ZdrError`/`SubscriptionError` all map to `"error"`); `payload()` :490; `RichEventSink` :767 with `emit(&self, ..)` :1026 (resume-buffered, shareable by reference across concurrent futures).
- `done` is not a variant: it is a transport frame (`.event("done")` in `sse.rs`; gate `NON_CHATEVENT_CONSUMER_CASES` `MP/scripts/gen-sse-event-taxonomy.py:43-49`). Terminal variants are `Stopped` and `Error`; the SPA already normalises all three (`FE/src/shared/chat/verevon-ui-events.ts:316-318`: `done`->`message.done`, `stopped`->`run.stopped`, `error`->`run.error`). `turn_end` is therefore unnecessary.
- Gate: `VARIANT_ARM` regex (`gen-sse-event-taxonomy.py:81-83`) needs a `ChatEvent::X { .. }` arm in both `family()` and `name()`; every produced name needs `case '<name>':` in `FE/src/shared/api/chat-client.ts` (LIVE native switch from :1487, `default`/`onUnknownEvent` :1719-ish, `retrieval_notice` case :1740); doc `MP/docs/sse-event-taxonomy.md` must be regenerated. CI: `.github/workflows/sse-taxonomy.yml` (`--check` :45, drift proof :49-62). HEAD fails the gate (doc lacks `plan_proposed`, `source_scope_applied`); LIVE passes with 28 rows (`PYTHONUTF8=1` mandatory on Windows, else a false cp1252 "stale").
- BFF relay: `FE/apps/gateway/src/domains/chat/streams.rs:113` proxy call, comment at :186 "raw byte-level SSE proxy ... no event-name allowlist". The audit's `streams.rs:99-109` cite is wrong (that is ZDR posture). The BFF has no unit tests of the relay. The AG-UI adapter `FE/apps/gateway/src/domains/ag_ui.rs` is NOT verbatim: `map_upstream_frame` (:752) maps a fixed name list to AG-UI `CUSTOM` (:883-906) and sends unknown names to `RAW` (:907). New names must be added to that list, test `maps_rich_chat_events_to_ag_ui_custom_events` (:1263).

### 2.2 The silent behaviours (all confirmed, all log-only)

| Behaviour | Site (LIVE) | Today |
|---|---|---|
| Context overflow sheds oldest history and retries | `MG/sse.rs:2931-2984` (`shed` select, `length_retries`, `warn!` :2979-2984); consts `MAX_CONTEXT_LENGTH_RETRIES=3` :4578, `CONTEXT_LENGTH_DROP_GROUP=6` :4581, `KEEP_TAIL=4` :4588 | `tracing::warn!` only; no frame |
| Compaction falls back to truncation | `MG/sse.rs:5609` (`summarize_head` None). `apply_head_summary` (`MG/compaction.rs:399`) then inserts `DROPPED_HISTORY_NOTICE` for the MODEL only | log only; user never told |
| Compaction planned/reused | `budget_compacted_thread` :5621 (`plan.reused`, `plan.head`), returns `RecentThread{messages, head_summary}` :5068; caller :1571, `head_summary` read :1593 | `tracing::debug!` |
| Skill guidance truncated/dropped | `fetch_skill_context` :5662, `fitted.truncated/dropped` :5758-5766 | log only |
| Do-run prompt compaction | `RunAgentResponse.compaction_triggered = 5` (`MP/proto/model_plane/v1/execution.proto:237`); read at `MG/sse.rs:9874-9880` with the comment "A log line and nothing user-facing, deliberately" | log only (a prior in-code decision, reversed by D1) |
| Tool retry, deployed-agent loop | `EC/runtime_loop/agent.rs:1916` (`for attempt in 1..=MAX_TOOL_ATTEMPTS`), warn :1964; policy `EC/runtime_loop/retry.rs` (`MAX_TOOL_ATTEMPTS=3`, `backoff_before_attempt`, risky/subagent calls never retried) | log only |
| Tool retry, chat loop | `MG/tool_loop.rs:4022` loop inside `dispatch_audited_tool` :3947 (called from :4130, :5263, :7231), warn :4064; mirror policy `MG/tool_retry.rs` | log only |
| Inference retry / rate-limit wait | `inference-core/src/provider/fallback.rs:1526` `infer_stream` completes ALL retry passes (`:1537-1570`, `wait` :1557) before returning the `Receiver`; reported only on the final chunk as `ProviderTiming.attempts/failed_attempts_ms/retry_wait_ms` (`MP/proto/model_plane/v1/inference.proto:411/415/419`, `InferChunk.provider_timing = 15` :365) | `MG/turn_timing.rs:248-261` forwards only `attempts`; `retry_wait_ms`/`failed_attempts_ms` are never read by the gateway |

Audit corrections: the card's "retry visibility needs an inference-core proto change" is wrong for post-hoc (fields exist, S4) and right only for a live signal (D2). Tool-retry backoff is 100 ms then 200 ms, so the user-visible stall is the failed attempt's own duration, not the wait. The Do loop reaches the chat stream through session-core, not a stream: `RunAgent` is unary (`execution.proto:34`); events flow `RecordOrchestrationEvent` (`orchestration.proto:122`) -> session-core broadcast (`SC/orchestration_grpc.rs:3192`, run-id routing in `event_run_id` :1819, only browser events persisted via `persist_browser_event` :3227) -> `StreamRunEvents` -> gateway `follow_agentic_run` closure (`MG/sse.rs:9677-9690`) which today maps only to `step_update`. Precedent for a best-effort execution-core publish: `announce_delegated_child_stopped` (`EC/runtime_loop/agent.rs:2932`). The Do-path frames are sent directly to `tx` (no `seq`/buffer), so they are not replayed on resume (same as `step_update` today).

### 2.3 Frontend today

- `retrieval_notice` is handled in `chat-client.ts` (:1740, `onRetrievalNotice` :675, type :250) [live-only] but no controller or component consumes it (grep: only `chat-client.ts` and tests). The precedent typed notice has no renderer; S5 fixes that.
- Four divergent thread-status vocabularies: `FE/src/features/core/components/CoreSidebar.tsx:874` `chatRunStatusChip` (calm: queued/running/attention/failed only), `FE/src/features/chat/lib/chat-organization.ts:50-51` (`activeStatuses`/`attentionStatuses` sets), `FE/src/features/spaces/lib/space-thread-presentation.ts:33` `threadStatus` (label), AgentRunConsole pill (`FE/src/features/agents/components/AgentRunConsole.tsx` ~:2314). Input exists everywhere: `latest_run_status`, `latest_run_updated_at` flow `session-core grpc.rs:5556` -> BFF `history.rs:64,146` -> `chat-client.ts:2348` -> `ChatThreadSession`. Server statuses seen: running, paused, awaiting_approval, completed, failed, cancelled (`EC/state.rs` `RunStatus`) plus queued; WS03 adds parked.
- Unseen-completion: the audit/sweep say none; correction: Spaces have an "unread since arrival" dot (`FE/src/features/spaces/lib/space-live-work.ts:97 unreadThreadIdsIn`, `publishSpaceUnread`, sidebar dot `CoreSidebarSpacesPanel.tsx:295-317`), per room, post-based. Personal chat threads have none.
- Stream cue precedents to extend, not duplicate: `FE/src/features/chat/lib/stream-activity.ts` (`deriveStreamActivity`, "only real state"), `ChatPage.tsx:462` `streamAnnouncement` live region (lifecycle, not tokens), node registry `ChatMessages.tsx:214 CHAT_NODE_REGISTRY` (`memory-recall` entry :247), node kinds `FE/src/shared/chat-nodes/types.ts:92-158`, `derive.ts` (`truncated` node is the closest precedent), controller handler pattern `use-chat-controller.ts:3239 onMemoryRecall`, `ChatTurn` fields `chat-types.ts:358-367`.

### 2.4 Live-only prerequisites (WS01 S0 must land first)

`MG/sse_events.rs` (RetrievalNotice, ZdrError, RetentionClass), `MG/sse.rs` (+883), `MG/tool_loop.rs` (+425, `retrieval_notice_events` :354), `EC/runtime_loop/agent.rs` (+207), `MP/docs/sse-event-taxonomy.md` (+4 rows), `FE/src/shared/api/chat-client.ts` (+53), `chat-types.ts`, `ChatMessages.tsx` (+229), `CoreSidebar.tsx`, `mp-contracts` `zdr_refusal.rs`/`retention.rs` (`sse_events.rs:385` imports `mp_contracts::zdr_refusal::Refusal`, so `sse_events.rs` cannot be committed without it), `MP/rust/services/model-gateway/tests/zdr_*` and `tests/fixtures/zdr-posture-matrix.json`. This stream starts only after those are committed; otherwise every hunk below rebases against 1000+ uncommitted lines.

## 3. Decisions and open questions

| ID | Question | Options | Recommendation | Gates |
|---|---|---|---|---|
| D1 | Reverse the in-code decision (`sse.rs:9862-9873`) that Do-run compaction is "a log line and nothing user-facing"? | keep silent / counts-only calm row | Reverse, and record a new ledger entry (`MP/docs/decisions/ledger.md`, CLAUDE rule). Reason: the comment itself says a thin answer has no explanation; a one-line calm row (not a banner) with no content is cheap. Ledger L1674 and L1874 (compaction recoverable, both loops) are NOT reversed. | S2 part (Do-run). Others proceed |
| D2 | Inference retry visibility: post-hoc vs live | (a) post-hoc from `ProviderTiming` (S4), (b) live `InferChunk` field 16 `retry_notice` + restructure `fallback.rs infer_stream` to return the `Receiver` before retry passes (L, hot file) | (a) now; (b) only if post-hoc data shows long waits matter. No proto change for (a) | Only the deferred live slice |
| D3 | Thread-status vocabulary and precedence (shared with WS16 and the Space session) | per-surface / one function | One function, order high to low: `needs_input` (awaiting_approval, paused, blocked, ambiguous, browser approval) > `failed` (failed, timed_out, denied, rejected) > `running` > `parked` (WS03 resumable) > `queued` > `unseen_done` (completed/cancelled newer than last seen) > `idle`. Own-stream (this tab) counts as `running` | S6 |
| D4 | Persist counts-only lifecycle notes on the assistant message? | stream-only / persist | Persist counts and closed tokens for non-ZDR turns (S8); ZDR turns persist nothing. A reload that silently hides "history was condensed" reproduces the original honesty gap | S8 only |
| D5 | Connector incident owner | capability-core MCP registry / integration-corev2 / WS23 | Hand to WS23 (connected apps). capability-core owns the MCP registry (`mcp_servers`, ownership matrix L59) but MCP execution is in `MG/mcp_gateway.rs`; provider connector tokens belong to integration-corev2. stdio transport is out (WS05), so there is no stderr tail to redact: incidents are typed kinds only | S9 |
| D6 | Event shape | one generic `run_notice` / typed variants | Three typed variants plus `run_parked`; payloads carry closed tokens and counts, never sentences (wording key lives in the SPA, as `retrieval_notice` does). Gating: `retry_scheduled` family `steps` (composer default-on, `chat-client.ts` DEFAULT_FEATURES); `compaction`, `context_change`, `run_parked` control (answer-affecting; a client that opted into nothing must still hear them) | S1 |
| D7 | Trigger-kind chip | build / defer | Defer until a run-level `origin` exists (WS20 scheduled runs / F-t3-064 / WS02 `runs.metadata`). Only thread `origin` exists today (`sessions.proto:263`) | not scheduled |
| D8 | WS09 D8 (`message.delta` Trace row) | WS15 replaces / WS09 keeps | WS09 keeps its first-delta collapse; WS15 adds no `message.delta` event | none |

No ADR required. Proceed without decisions: S1, S2 (non-Do part), S3, S4, S5, S6 (apart from D3 sign-off of the order, which is cheap to change).

## 4. Slices (PR-sized, in build order)

Common: flag `MODEL_GATEWAY_LIFECYCLE_EVENTS` (strict `true`/`false` via a `pub const ENABLED_ENV` in `mp-contracts/src/lifecycle_notice.rs`, per the repo flag pattern; compose passthrough `${MODEL_GATEWAY_LIFECYCLE_EVENTS:-false}` in the model-gateway environment block of `MP/deploy/docker-compose.yml` plus a compose-contract assertion modelled on `scripts/tests/letta-tool-search-compose-contract-test.sh`). Default `false` through S4 so no frame ships before the SPA renders it; S5 DoD flips the default to `true`. Rollback = set `false` (the SPA ignores absence). The SPA needs no flag. Evidence for every slice: rebuild with `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build model-gateway` (and `execution-core`/`session-core` where touched), confirm containers on the new image, drive the flow through the BFF, capture the SSE transcript with `curl -N`.

### S1 - Vocabulary, variants, taxonomy gate [BUILD] (M, 3-4 d)
**Goal.** The wire contract exists and the CI gate passes before any producer or renderer.
**Files**
- NEW `MP/rust/crates/mp-contracts/src/lifecycle_notice.rs` + `pub mod lifecycle_notice;` in `mp-contracts/src/lib.rs` [live-modified]: `ENABLED_ENV`; closed token consts `RETRY_REASONS = [timeout, connection, upstream_unavailable, rate_limited, other]`, `COMPACTION_SCOPES = [history, tool_results]`, `COMPACTION_OUTCOMES = [summarized, reused, truncated, cleared]`, `CONTEXT_CHANGES = [history_shed, skills_truncated, run_context_skipped]`; `fn is_closed_token(..)`; a payload-key allowlist per event used by tests (no free-text key may exist).
- `MG/sse_events.rs` (hot; add as one fenced block `// ---- lifecycle (WS15) ----` in each of enum, `family()`, `name()`, `payload()` to ease rebases): `RetryScheduled { scope: &'static str, tool: String, attempt: u32, max_attempts: u32, wait_ms: u64, reason: &'static str }` family `Some("steps")`, name `retry_scheduled`; `Compaction { scope, outcome, messages_compacted: Option<u32>, messages_kept: Option<u32>, pinned_kept: Option<u32> }` control, name `compaction`; `ContextChange { change: &'static str, messages_dropped: Option<u32>, skills_truncated: Option<u32>, skills_dropped: Option<u32>, layer: Option<&'static str>, state: Option<&'static str> }` control, name `context_change`. Payloads carry only those keys plus `request_id`; never the tool arguments, tool output, error text or summary. Extend tests near `plain_path_emits_only_control_events` and `opt_in_unlocks_only_the_requested_families` and `names_and_payload_shapes_are_stable`.
- `MP/docs/sse-event-taxonomy.md` regenerate (`PYTHONUTF8=1 python "apps/Model Plane/scripts/gen-sse-event-taxonomy.py"`, then `--check`); do not hand-edit.
- `FE/src/shared/api/chat-client.ts`: types `ChatRetryScheduledEvent`, `ChatCompactionEvent`, `ChatContextChangeEvent`; handlers `onRetryScheduled`, `onCompaction`, `onContextChange` in `ChatStreamHandlers` (near :675); native `case 'retry_scheduled' | 'compaction' | 'context_change'` before `default` (add-only hunk); unknown `change`/`outcome` tokens pass through as strings (forward-compatible).
- `FE/src/shared/chat/verevon-ui-events.ts`: UI events `lifecycle.retry`, `lifecycle.compaction`, `lifecycle.context` in `VerevonUiEvent` and `toVerevonUiEvent`.
- `FE/apps/gateway/src/domains/ag_ui.rs`: add the three names to the CUSTOM list (:883-906); extend test :1263. `streams.rs` is untouched (byte-verbatim relay is a guard, section 7).
- `MP/docs/decisions/ledger.md`: entry "Lifecycle events are typed, counts-only, ephemeral" including the D1 reversal.
**Proto/migration.** None. **ZDR.** Payloads are counts and closed tokens; no store. Add test `MP/rust/services/model-gateway/tests/lifecycle_event_contract.rs` (NEW): every payload key is in the allowlist; a `Compaction` carries no `summary`; a ZDR-class turn builds identical frames (no content to strip). **Tests.** `cd "apps/Model Plane/rust" && cargo test -q -p mp-contracts && cargo test -q -p model-gateway --lib sse_events`; `PYTHONUTF8=1 python "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`; `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/shared/api/chat-client.test.ts src/shared/chat/verevon-ui-events.test.ts` (NEW test file if absent) and `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`. If WS12 S7 has landed, add samples to `payload_contract` and regenerate `MP/api/contracts/sse-event-payloads.json` (`MP_UPDATE_GOLDEN=1 cargo test -p model-gateway --lib sse_event_payload_golden`, then the sync script); if not, WS12 adds them (second lander regenerates).
**DoD.** Gate green; removing the `retry_scheduled` SPA case turns CI red; a plain-profile client (no features) receives `compaction`/`context_change` but not `retry_scheduled`. **Evidence.** CI gate run on the PR plus the gate mutation output. **Deps.** WS01 S0. **Risks.** Merge conflicts in `sse_events.rs` (hot file, 5 workstreams).

### S2 - Emit the silent context changes [FINISH] (M, 3-5 d)
**Goal.** Every place that today only logs a change to what the model sees emits a typed event.
**Files**
- `MG/sse.rs` shed block (:2931-2984): measure `before = grpc_req.messages.len()` around `shed(..)`; after a successful shed `emit_and_buffer(&tx,&stream_buffers,&buffer_key,&mut seq,&features,&req_id, ChatEvent::ContextChange{change:"history_shed", messages_dropped: before-after, ..})` (the `{{ ... }}` block already uses these bindings for `emit_prepared_failure`). The terminal "still too long" path is unchanged (typed `Error` already).
- `MG/sse.rs` `RecentThread` :5068: add `compaction: Option<CompactionFacts{outcome, compacted, kept, pinned}>`; fill in `budget_compacted_thread` :5621 (`plan.reused`, `head.end-head.start`, summary `None` -> `truncated`) and in the message-count path of `load_recent_thread_messages` :5350. At :1593 carry the facts into the spawned stream task and emit `ChatEvent::Compaction` as the first frame after `connected`. Behaviour must stay byte-identical when the flag is off (existing "before the fields existed" test pattern).
- `MG/sse.rs` `fetch_skill_context` :5662 / `SkillContext` (:5758-5766 data, caller :1863): add `truncated`, `dropped` counts; emit `ContextChange{change:"skills_truncated", skills_truncated, skills_dropped}` only when either is non-zero.
- `MG/sse.rs` :9874-9880 (Do-run): `ChatEvent::Compaction{scope:"tool_results", outcome:"cleared"}` (bool only; no counts; no proto change) - D1.
- `MG/compaction.rs`: expose `HeadSummary` size helper if needed (no behaviour change).
**Proto.** None (`compaction_triggered` already field 5). **Flag.** `MODEL_GATEWAY_LIFECYCLE_EVENTS`. **ZDR.** Counts only; ephemeral frame; nothing persisted (S8 separate). Compaction under ZDR still uses `summarizer.zdr`; the frame says only that it happened. Golden-matrix case: `tests/lifecycle_event_contract.rs` row "zdr turn: compaction frame has no summary key". **Tests.** `cargo test -q -p model-gateway --lib sse` new names: `history_shed_emits_a_context_change_with_the_dropped_count`, `a_reused_summary_reports_reused_not_summarized`, `summarizer_outage_reports_truncated`, `skill_truncation_reports_counts_only`, `flag_off_stream_is_byte_identical`; existing `tests/overflow_contract.rs`, `tests/skill_budget_contract.rs`, `tests/context_recovery_contract.rs` must stay green. **DoD.** Each of the four silent paths emits exactly one frame per occurrence; `history_shed` repeats at most `MAX_CONTEXT_LENGTH_RETRIES`. **Evidence.** Seed a thread with enough long messages to exceed a small-context model, send a turn through the BFF, capture the SSE transcript showing `context_change` before the first `chunk`; repeat with the summarizer failing (stop inference-core briefly) to capture `compaction` outcome `truncated`; one Do run that clears tool results. **Deps.** S1. **Risks.** `sse.rs` size-ratchet file (WS12 S11): put the new helper functions in a NEW `MG/lifecycle_events.rs` module and keep `sse.rs` hunks to call-ins.

### S3 - Retry visibility in both tool loops [BUILD] (L, 5-7 d)
**Goal.** A retried tool shows "retrying (2 of 3)" instead of a stall; the two loops stay separate and agree on vocabulary.
**Files**
- `MG/tool_retry.rs` and `EC/runtime_loop/retry.rs`: add `transient_reason(error: &str) -> &'static str` (maps the existing marker groups to the closed `RETRY_REASONS`; the error text itself never leaves the function). Extend `MG/tests/tool_retry_contract.rs` to assert the two mapping tables are byte-identical (same source-text technique as the marker lists).
- `MG/tool_loop.rs` `dispatch_audited_tool` :3947: new parameter `retry_sink: Option<&RichEventSink>`; in the loop (:4022-4064) before the next attempt call `sink.emit(ChatEvent::RetryScheduled{scope:"tool", tool, attempt: attempt+1, max_attempts: MAX_TOOL_ATTEMPTS, wait_ms: backoff, reason})`. Callers: :4130 and :5263 pass `None`; the main-loop caller :7231 passes `Some` when `events` is `ToolEvents::Live(&sink)` (:5142) and `None` when `Buffered`. `emit(&self)` is safe inside the `join_all` futures.
- `EC/runtime_loop/agent.rs` retry loop :1916-1964: before sleeping, `announce_tool_retry(ctx, step_id, tool, attempt, wait, reason)` modelled on `announce_delegated_child_stopped` :2932 (`RecordOrchestrationEvent`, best-effort, warn on failure, identifiers and tokens only; session-core already refuses orchestration writes for ZDR credentials).
- `MP/proto/model_plane/v1/orchestration.proto` `OrchestrationEvent` oneof: add `ToolRetryScheduled tool_retry_scheduled = N` with message fields `run_id=1, step_id=2, tool=3, attempt=4, max_attempts=5, wait_ms=6, reason=7`. Verified highest tag today is 23; WS03 reserves 24 (`RunParked`) and 25 (`RunResumed`), so propose N=26, integrator confirms at merge. Additive; regenerate Go bindings; `buf breaking` clean.
- `SC/orchestration_grpc.rs` `event_run_id` :1819: new arm returning `run_id`; do NOT add to `persist_browser_event` (ephemeral).
- `MG/sse.rs`: `orchestration_event_to_sse` :10211 name arm `"tool_retry_scheduled"` and `event_payload_value` :10421 arm (keeps the Agent Run Console working); new `orchestration_event_to_lifecycle(event) -> Option<ChatEvent>` and chain it in the `follow_agentic_run` closure :9677-9690 behind the flag (`.or_else`, `should_emit(&features)`).
- `MG/lifecycle_events.rs` (S2) hosts the mapping.
**ZDR.** Do runs are never ZDR (refused before dispatch); the chat loop under ZDR is the persistence-free path with no tool loop, so no retry frame exists there; frames carry tool NAME, counts and a closed reason, never the error string, arguments or output. Test: `retry_frame_never_contains_the_error_text`. **Tests.** `cargo test -q -p model-gateway --lib tool_loop` (NEW `a_transient_failure_emits_one_retry_scheduled_per_extra_attempt`, `buffered_events_mode_emits_none`), `cargo test -q -p model-gateway --test tool_retry_contract`, `cargo test -q -p execution-core --lib retry` and `agent` (NEW `a_retried_tool_announces_each_extra_attempt`, `a_risky_call_announces_no_retry`), `cargo test -q -p session-core --lib orchestration_grpc` (NEW `event_run_id_extracts_tool_retry`). Windows: use `--lib` (two cargo targets fail pre-existing). **DoD.** Both loops emit; a non-retryable or risky call emits nothing; frames replay on resume in the chat loop (sink is buffered). **Evidence.** Stop the Data Plane retrieval service so `knowledge_search` fails transiently in a chat turn (capture two `retry_scheduled` then `tool_result` error), and one Do run whose tool is made transient (capture the orchestration-derived frame and the console event). **Deps.** S1, S2 (module). **Risks.** `dispatch_audited_tool` already has ~23 parameters (add one, no struct refactor in this PR); loop separation must hold (no shared dispatch code, only `mp-contracts` tokens); `agent.rs`/`mod.rs` are live-modified.

### S4 - Provider wait, post-hoc [FINISH] (S, 1-2 d)
**Goal.** An answer that waited on a provider rate limit says so, from fields that already arrive.
**Files.** `MG/turn_timing.rs:225-260` `ProviderSummary`: add `failed_attempts_ms: u64` and `retry_wait_ms: u64` (`#[serde(skip_serializing_if = zero)]`) from `ProviderTiming` (:415, :419); `FE/src/features/chat/components/ChatMessages.tsx` processing line/metrics (`answerProcessingNote` :438) shows "waited N s for the provider" when `retry_wait_ms > 2000`; FE type for the `turn_timing` payload. Gating is unchanged (`usage` family). **Proto/migration.** None. **Flag.** Rides `MODEL_GATEWAY_LIFECYCLE_EVENTS` for the FE copy only. **ZDR.** Durations only (the proto says so, `inference.proto:380-384`). **Tests.** `cargo test -q -p model-gateway --lib turn_timing` (NEW `provider_summary_reports_wait_only_when_nonzero`); vitest NEW case in `ChatMessages.processing-line.test.tsx`. **DoD.** `turn_timing` frame carries the wait after a forced 429 with `retry-after`. **Evidence.** inference-core fake provider returning 429 once, capture the frame. **Deps.** S1. **Risk.** None notable.

### S5 - SPA status cues [BUILD] (M-L, 4-6 d)
**Goal.** The events become calm, accessible, localised cues; `retrieval_notice` finally renders.
**Files**
- `FE/src/features/chat/components/chat-types.ts` (:358-367 neighbourhood): `ChatTurn.lifecycle?: LifecycleNotice[]` (capped 20, de-duplicated by `change+count`), `ChatToolCall.attempts?: number`.
- `FE/src/features/chat/components/use-chat-controller.ts`: handlers beside `onMemoryRecall` (:3239) for `onRetryScheduled` (sets `toolCall.attempts`, transient `turn.retrying`), `onCompaction`, `onContextChange`, and wire `onRetrievalNotice` (chat-client :675) into the same list. Small local hunks (WS09 rule).
- `FE/src/shared/chat-nodes/types.ts` + `derive.ts`: node `{ kind: 'lifecycle-notices', notices }` pushed after `memory-recall`, before `truncated`; add `nodeKey` override only if WS09 S2 has landed (single node per turn, default key is fine).
- `FE/src/features/chat/components/ChatMessages.tsx` `CHAT_NODE_REGISTRY` (:214): renderer `lifecycle-notices` (role="note", one calm line per notice, text from `i18n.tr(no, en)` by token; unknown token renders the generic "Context was adjusted"). No banner, no colour escalation; `compaction`/`context_change` are informational, `truncated` outcome gets the existing warning tone.
- `FE/src/features/chat/lib/stream-activity.ts`: `StreamActivity` gains `{ kind: 'retrying', tool, attempt, max }` (derived from `ChatToolCall.attempts` while the call is unsettled; cleared by the result); `streamActivityLabel` updated so the visible line and the spoken line stay one function.
- `FE/src/features/chat/components/ChatPage.tsx` `streamAnnouncement` (:462): add one polite sentence per new notice (not per retry tick), via the existing `spokenActivity` path.
- Strings must pass WS12 S4 ui-gates (`tr(no,en)`, no raw colours); add the state checklist rows (WS12 S5 `UI_STATE_CHECKLIST.md`): retrying, history trimmed, compacted, parked.
**Action contract.** None needed: these are read-only cues, no operation. **ZDR.** Notices live in turn state only; `chat-thread-history.ts` local persistence of `lifecycle` is skipped for temporary/ZDR threads (reuse the existing `isTemporaryThread` guard, `use-chat-controller.ts:661`). **Tests.** `pnpm exec vitest run src/shared/chat-nodes/derive.test.ts src/features/chat/lib/stream-activity.test.ts src/features/chat/components/ChatMessages.lifecycle.test.tsx src/features/chat/components/use-chat-controller.lifecycle.test.ts` (NEW files: ChatMessages.lifecycle.test.tsx, use-chat-controller.lifecycle.test.ts); `pnpm typecheck`; `pnpm lint`; e2e NEW `tests/e2e/chat-lifecycle-cues.spec.ts` with a routed SSE fixture (stack-free): `pnpm test:e2e -- chat-lifecycle-cues`. Locale smoke under `en` and `no` (WS12 S5). **DoD.** A fixture stream with `retry_scheduled`, `compaction`, `context_change` renders the rows, announces once each, and renders nothing for a plain stream; default `MODEL_GATEWAY_LIFECYCLE_EVENTS` flipped to `true`. **Evidence.** Browser run (second dev server per the verevonv3 worktree note) against the rebuilt gateway: screenshot of the Trace rows and the live-region text from S2/S3 flows. **Deps.** S1; S2/S3 for live data. **Risks.** `ChatMessages.tsx`/`use-chat-controller.ts` are the hottest FE files: WS09 S2 merges first; keep hunks add-only.

### S6 - One thread status and unseen completion [BUILD] (M, 3-4 d)
**Goal.** One derivation with a fixed precedence (D3) behind every status label, plus an unseen-completion marker for personal chat threads.
**Files**
- NEW `FE/src/shared/chat/thread-status.ts`: `deriveThreadStatus({ runStatus, parked?, ownStreaming, updatedAt, lastSeenAt })` returning the D3 token; `threadStatusLabel(status, tr)`; status-set constants (`NEEDS_INPUT`, `FAILED`, `ACTIVE`) replacing the literals; NEW `thread-status.test.ts` (table test for every server status and every precedence pair).
- `FE/src/features/core/components/CoreSidebar.tsx:874` `chatRunStatusChip` and its call :644: delegate; add a `done` dot tone for `unseen_done`. `FE/src/features/chat/lib/chat-organization.ts:50-51`: derive `activeStatuses`/`attentionStatuses` from the shared constants. `FE/src/features/agents/components/AgentRunConsole.tsx` pill: delegate.
- NEW `FE/src/shared/chat/thread-seen.ts`: per-device `lastSeenAt` by `threadId` in `localStorage` (try/catch, renders without it); set when the thread is opened or its stream ends while visible. This is a per-viewer convenience; server read-state is WS16 (D3 interface).
- `FE/src/features/spaces/lib/space-thread-presentation.ts:33` `threadStatus`: delegate for the label (Space session owns the call sites; see 6).
**Action contract.** Local marker only; if WS16 moves it server-side it adds the action contract under `FE/src/shared/actions/`. **ZDR.** No server data. **Tests.** `pnpm exec vitest run src/shared/chat/thread-status.test.ts src/features/core/components/CoreSidebar.test.tsx src/features/chat/lib/chat-organization.test.ts src/features/agents/components/AgentRunConsole.test.tsx`. **DoD.** No status literal set remains outside `thread-status.ts` (grep check added to the test); a completed run on a thread not opened since shows the dot, opening clears it. **Evidence.** Browser run: start a long turn, switch thread, return; capture the three states. **Deps.** D3, WS16 (consumes the enum when it ships). **Risk.** Space tests assert label strings (`SpaceRoomTimeline.test.tsx:79`); keep labels identical.

### S7 - Promote `run_parked` [FINISH] (S, 1-2 d; conditional on WS03 S8)
**Files.** `MG/sse_events.rs`: `RunParked { run_id, reason, retry_after_ms: Option<u64>, resets_at: Option<String> }`, control, name `run_parked` - the payload keys are exactly WS03's raw frame, so the wire is unchanged and this is a code move; replace WS03's raw `.event("run_parked")` emission at its `classify_agentic_run_outcome` site; `chat-client.ts` `case 'run_parked'` (replace WS03's minimal handling) -> `onRunParked`; `ag_ui.rs` list; ledger line; taxonomy regen; S6 `parked` status uses it. If WS03 also emits `run_resumed`, promote it identically. **Tests.** `cargo test -q -p model-gateway --lib sse_events` (`run_parked_is_a_control_event`), gate, vitest. **Evidence.** The WS03 kill-mid-run flow, capturing the typed frame. **Deps.** WS03 S8, S1, S6.

### S8 - Persisted counts-only notes [BUILD, optional] (S-M, 2-3 d, D4)
**Files.** `MG/session_flow.rs` next to `with_head_summary` :1422: `with_lifecycle_notes(metadata, notes, zdr)` (returns metadata unchanged when `zdr`; counts and tokens only; cap 8); call at the completion site (`sse.rs` ~:3865 metadata chain); `FE/src/features/chat/components/chat-normalizers.ts` `messageToTurn` reads `metadata.lifecycle_notes` into `turn.lifecycle`. No migration (message metadata is a JSON struct). **ZDR.** Refused under ZDR; persisted for persistent and `zdr_waived` turns as counts only. Golden case: NEW test `MG/tests/lifecycle_event_contract.rs::zdr_turn_persists_no_lifecycle_notes`. **Tests.** `cargo test -q -p model-gateway --lib session_flow`; `pnpm exec vitest run src/features/chat/components/chat-normalizers.test.ts`. **Evidence.** Reload a thread that had a shed notice, row still present (psql read of the message metadata). **Deps.** S2, S5.

### S9 - Connector incident ring [BUILD, optional, recommend WS23 owns] (L, 5-8 d, D5)
Minimum design if taken: NEW capability-core migration "mcp_server_incidents" (`MP/go/services/capability-core/migrations/`, next number assigned by the integrator) with `(org_id, server_id, at, kind, http_class)` and a ring of 10 per server; closed `kind` list (connect_failed, tool_timeout, auth_expired, dns_blocked, schema_invalid); NEW recorder called from `MG/mcp_gateway.rs`; read endpoint in `internal/api/registry_apis.go`; no stderr and no error text stored. ZDR: persist nothing for ZDR callers (`proxy_mcp_tool` already guards ZDR by caller identity). Tests: `go test ./...` in capability-core. Deps: WS05, WS23. Not scheduled in this stream's critical path.

## 5. Interfaces

**PROVIDES**

| To | Contract |
|---|---|
| WS12 | Four new ChatEvent names (`retry_scheduled`, `compaction`, `context_change`, `run_parked`) with sample rows for `payload_contract`; one SPA `case` each (native switch); AG-UI list entries |
| WS09 | `ChatTurn.lifecycle` node `lifecycle-notices` (single per turn); no `message.delta` event (D8) |
| WS16 | `deriveThreadStatus` token set and precedence (D3); content-free signals: `run_parked`, `run_paused_for_approval`, `awaiting_approval`; local `lastSeenAt` store as the placeholder for server read-state |
| WS03 | Typed home for `run_parked` (S7) |
| WS27 | `lifecycle_notice` token module and `lifecycle_events.rs`; a cost-limit clamp or budget notice should be a new `context_change.change` token, not a new variant |
| WS11 | The effort clamp surface (WS11 S5 "clamp must be visible") can use `context_change` with a new token `effort_clamped` |
| WS23 | Incident vocabulary (S9) |
| Space session | `thread-status.ts` (label + precedence) and the lifecycle node renderer |

**CONSUMES**

| From | Contract |
|---|---|
| WS01 S0 | Live ZDR wave committed (files in 2.4) |
| WS02 | `runs.metadata.run_context` enum states -> `context_change{change:"run_context_skipped", layer, state}` (producer lands with WS02 S6; this stream defines the token only) |
| WS03 | `RunParked`/`RunResumed` orchestration tags 24/25, raw `run_parked` frame, `RunDetail.resumable/park_reason` |
| WS04 | `queued_input` additive `ids` (no conflict; same file) |
| WS12 | S4 ui-gates, S7 payload golden, S6 taxonomy gate extension (orchestration names) |
| WS09 | S2 `renderLive`/`nodeKey` before S5 |
| WS16 | Server run-state enum, when shipped, replaces the status input of S6 |

**Conflict hot-spots and edit order**

| File | Others | Rule |
|---|---|---|
| `MG/sse_events.rs` | WS02, WS04, WS12, WS13, WS14, WS27 | WS01 S0, WS12 S7 (tests only), WS02/WS04 (fields), then variants in arrival order (WS13/WS14 before WS15); fenced block; the second lander regenerates the golden and the taxonomy doc |
| `MG/sse.rs`, `MG/tool_loop.rs` | WS02, WS03, WS04, WS09, WS25 | WS02 first; WS15 hunks are call-ins to `MG/lifecycle_events.rs` |
| `orchestration.proto`, `sessions.proto` | WS03, WS13, WS14, WS17 | append-only; integrator assigns tags (propose 26 here) |
| `chat-client.ts`, `use-chat-controller.ts`, `ChatMessages.tsx`, `CoreSidebar.tsx` | WS09, WS11, WS14, WS16, WS17 | WS09 S2 first; add-only hunks; do not reformat |
| `EC/runtime_loop/agent.rs` | WS03, WS05, WS24, WS25 | WS03 first (park path); S3 adds one helper and one call |

## 6. Cross-session interfaces

- **Space session (a).** Shared pieces: thread-status vocabulary and the unseen marker. Owner: WS15 (`FE/src/shared/chat/thread-status.ts`) because it is a Frontend-plane derivation over `latest_run_status` with no server state; the Space session keeps its post-based "new since arrival" dot (`space-live-work.ts`, per room) and calls the shared label function. Assumption: the Space session does not add a fifth vocabulary and does not change status words used in `SpaceRoomTimeline.test.tsx`. Space room cues (retry/compaction rows in a room transcript) reuse the `lifecycle-notices` renderer through `createConversationNodeRegistry`; the Space composer sending `agentic` makes Space runs emit the same frames.
- **Quarry/browser session (b).** Shared: `browser_run_paused`, `browser_action_approval_required` (already mapped to `needs_input` in S6), and the envelope-v2/origin work (`overlap-quarry.md` row 5, owner Model Plane). Assumption: Quarry publishes through `RecordOrchestrationEvent` as today and adds no new chat-facing event names without going through the taxonomy gate; the trigger-kind chip waits for the run-level origin from that envelope (D7).

## 7. Regression guards (audit section 8)

| Strength | Touched? | Guard |
|---|---|---|
| Typed ChatEvent taxonomy and byte-verbatim relay (:1511) | Yes, directly | Taxonomy gate (`sse-taxonomy.yml`) + `--check`; WS12 S7 payload golden; `streams.rs` untouched (diff must be empty; BFF `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`); `ag_ui.rs` test extended; control events never gated (`plain_path_emits_only_control_events`); plain profile receives no `retry_scheduled` |
| ZDR by construction (:1509) | Yes | Counts/tokens-only allowlist test; S8 persistence refused under ZDR with a golden-style test; no new store in S1-S7; no content in any payload |
| Honest outcomes (:1510) | Yes (extends) | Cues only report observed events; no invented progress (`stream-activity.ts` rule); `truncated` compaction is reported as truncated, never as summarized; no number is fabricated (`messages_compacted` is `Option`) |
| Two loops separate (:1508) | Yes | No shared dispatch code; only `mp-contracts` tokens; `tool_retry_contract.rs` extended to the reason table |
| Context management ahead of the references (:1515) | Adjacent | `reattach_context`, pins and recall untouched; `overflow_contract.rs`, `skill_budget_contract.rs`, `compaction_parity_contract.rs`, `context_recovery_contract.rs` stay green; byte-identical stream with the flag off |
| Rendering/accessibility calm workspace (:1518) | Yes | One polite sentence per notice (not per tick); no focus stealing; `ChatPage` live region test; WS12 locale smoke |
| Capability gate, Quarry executes, credential isolation | No | Retry events never change dispatch; risky calls are still never retried |

## 8. Effort and risks

**Total.** Core S1-S7: about 20-30 engineer-days (S1 3-4, S2 3-5, S3 5-7, S4 1-2, S5 4-6, S6 3-4, S7 1-2). Optional S8 2-3, S9 5-8. **Critical path.** WS01 S0 -> S1 -> S2 -> S3 -> S5 (flip default) -> S6; S4 and S7 run beside it.

| Risk | Mitigation |
|---|---|
| `sse_events.rs`, `sse.rs`, `chat-client.ts`, `ChatMessages.tsx` contention with 5-20 workstreams and ~1000 uncommitted live lines | Start only after WS01 S0; fenced add-only blocks; logic in NEW `MG/lifecycle_events.rs`; second lander regenerates golden and doc |
| Events ship before the SPA can render them, or flood a calm UI | Flag default off until S5; one node per turn, cap 20, de-dup; retry cue is transient and clears on result; informational tone only |
| ZDR leak through "helpful" detail (tool args, error text, summary) | Closed-token payloads, key allowlist test, error text never leaves `transient_reason`; S8 refused under ZDR |
| Do-loop retry frames are not resume-buffered and the tail may not be open | Accepted (same as `step_update`); cue is best-effort; documented in S3 DoD; the tool result and Trace remain authoritative |
| Tool-retry cue flashes (backoff 100-200 ms) and inference stalls stay invisible live | Show the cue until the call settles (covers the failed attempt); S4 reports provider waits post-hoc; live variant deferred (D2) with explicit trigger |
| Windows gate false "stale" and pre-existing red targets mislead evidence | Run the gate with `PYTHONUTF8=1`; use `--lib`; diff failing sets against HEAD rather than trusting exit codes |
