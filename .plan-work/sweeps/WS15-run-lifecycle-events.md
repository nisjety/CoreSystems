# WS15-run-lifecycle-events: code-map sweep (F-t3-049)

Read-only sweep, 2026-10-15. WT = worktree at HEAD afb83928 (clean). LIVE = C:/dev/CoresSystem (HEAD plus uncommitted; the audit read this tree). Audit: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`. Gate runs used `--check` only (never rewrites the doc).
Verdicts: MATCH (same symbol at cited lines), DRIFT (same symbol at other lines), ABSENT (not found), DIFFERS-IN-LIVE (file is M), LIVE-ONLY (??).

## 0. Key findings

1. "23 ChatEvent variants" holds only in LIVE. WT `sse_events.rs` enum (`:105-345`) has 21 variants. LIVE (`:144-404`) has 23: adds `RetrievalNotice` and `ZdrError`.
2. `done` is not a ChatEvent variant. It is a transport frame sent with `.event("done")` in `sse.rs` (WT :416, :4272, :5792, :6034, :6910, :7288, :8254, :9755) and listed as a non-ChatEvent case in the gate (`gen-sse-event-taxonomy.py` :43-49). Only `Stopped` and `Error` are variants among the card's three terminal frames.
3. Silent behaviours confirmed (log only, no frame): tool-retry loops (`execution-core/.../agent.rs` WT :1912-1958; `model-gateway/src/tool_loop.rs` WT :4011-4060), context-overflow shed (`sse.rs` WT 2783-2857, LIVE 2911-2984), compaction fallback to truncation (LIVE `sse.rs` 5607-5610), skill-guidance truncation (LIVE `sse.rs` 5758-5766).
4. No identifiers for retry_scheduled, compaction marker, context-change row, thread status, turn_end or connector incident exist in `apps/Model Plane/rust/services` in either tree (grep found only a test name `context_changes_must_invalidate...`). Thread status and status pills exist only in the frontend.
5. The card's proto-change premise is partly superseded. `ProviderTiming` already carries `attempts` (6), `failed_attempts_ms` (7) and `retry_wait_ms` (8) (`inference.proto` :385-423). model-gateway source has no reference to `retry_wait_ms` or `failed_attempts_ms`; TurnTiming reads `attempts` (`turn_timing.rs` :248, :261). Tool retries wrap tool dispatch, not inference-core calls.
6. Closest precedent for notice rows is LIVE-only: `ChatEvent::RetrievalNotice` (ungated control event; LIVE `sse_events.rs` :368-370), emitted from grounding notices (`sse.rs`) and from tool results (`tool_loop.rs` `retrieval_notice_events`).
7. Origin: only thread origin exists (`sessions.proto` `CreateThreadRequest` :198, `origin = 17` :263, values chat/space/agent_run/support/system). No run-record origin or trigger-kind field in any proto.
8. Connector incidents: no code. The capability-core "incident" hits (33 occurrences, 8 files) are capability scoring (`internal/scoring/scoring.go` :25, :38, :51, :68-70).
9. Unseen-completion state: no implementation found in `apps/Frontend Plane/verevonv3/src` (grep hits are comments and CSS only).
10. Section 8 citation `streams.rs:99-109` (for "BFF relays bytes verbatim") is a MISMATCH: those lines settle ZDR posture. The byte-level proxy comment is at :186.
11. Windows trap: the gate reads the doc with the default cp1252 codec (`read()` :52-61). Without `PYTHONUTF8=1` it reports "stale" on both trees (mojibake). Use `PYTHONUTF8=1` for a true result.

## 1. Card inventory

| Field | Value |
|---|---|
| Card | audit :1004-1015, "F-t3-049 · Typed run lifecycle events and status cues: retry, compaction, context-change rows, thread status, connector incidents" |
| Tier / effort / plane / wave / confidence | Recommended (section 5 starts :700) / L (Appendix D :2137: "M to L") / Model Plane + Frontend Plane / W2 / Medium |
| 9.1 row | :1572, inside "#### Wave W2" (heading :1551, next heading W3 :1575) |
| Appendix B row | :1866 (W2, source "deepseek") |
| Appendix C | No entry (Appendix C covers F-t3-001..023 only; this item is Recommended, not Priority) |
| Appendix D | :2137 (M to L: retry visibility needs an inference-core to gateway contract change; AG-UI vocabulary; connector incidents as a separate capability-core feature) |
| Other references | :63, :312 (best-of-breed table), :834 (F-t3-034 dependency), :1369 (DeepSeek: retries silent, no compaction event), :1382 (T3 Code status pills; F-t3-015), :1510-1511 (section 8 "do not regress": new events go through the CI gate; BFF stays allowlist-free) |
| Card scope | retry_scheduled; compaction marker (counts only); context-change/notice rows (history shed, skill truncated, trigger kind); one thread status with fixed precedence plus unseen-completion; connector incidents (capability-core, redacted); no turn_end (normalise done/stopped/error client-side) |
| Protect | byte-verbatim relay; taxonomy CI gate; ZDR (compaction markers carry counts only, never the summary) |

## 2. Citation verification

Card citations (from "Verevon today and approach", :1012, and "Dependencies", :1014):

| Card | Cited path:lines | Claim | WT verdict | LIVE status | Corrected path:lines |
|---|---|---|---|---|---|
| F-t3-049 | `apps/Model Plane/rust/services/model-gateway/src/sse_events.rs:142-180` | ChatEvent done/Stopped/Error frames | DRIFT. WT 142-180 are `Citation` variant doc comments and fields (enum starts :105; Citation :132-191). No done variant exists | DIFFERS-IN-LIVE (M). LIVE 142-180 = enum head and ResultReceipt..Citation (enum :144) | WT: Stopped :320, Error :323-327. LIVE: RetrievalNotice :368, Stopped :372, Error :375-379, ZdrError :384-386 |
| F-t3-049 | `sse_events.rs:340-395` | Stopped and Error frames; gating | DRIFT. WT 340-395 = SubscriptionError tail (:337-344), `impl ChatEvent` (:347), `family()` (:351-382), `should_emit()` (:389-394) | DIFFERS-IN-LIVE (M). MATCH in LIVE: Verification :334, MemoryRecall :345, QueuedInput :360, Stopped :372, Error :375, ZdrError :384, SubscriptionError :396 | WT: family() :351, should_emit :389, name() :398 (arms "stopped" :418, "error" :419). LIVE: family() :410 |
| F-t3-049 | `apps/Model Plane/rust/services/model-gateway/src/sse.rs:2905-2990` (card short form "sse.rs:2905-2990") | Context overflow sheds history, log only | DRIFT. WT 2905-2990 = `run_infer_fallback` call and liveness Error frames (:2968, :2988). Shed block is WT 2783-2857 (check :2783, shed select :2801, shed call :2807, warn :2855) | DIFFERS-IN-LIVE (M). MATCH in LIVE: block :2911-2985; success path log only (`tracing::warn!` :2979-2984); failure path `emit_prepared_failure` :2962-2974 | WT ~2777-2862. LIVE 2905-2990 as cited |
| F-t3-049 | `apps/Model Plane/rust/services/execution-core/src/runtime_loop/retry.rs:1-60` | Tool retries run unseen | MATCH for content (module doc; `MAX_TOOL_ATTEMPTS` :37; `BASE_BACKOFF_MS` :45; `TRANSIENT_MARKERS` :53). The retry loop is in `agent.rs` WT :1912-1958 (warn :1957) | clean (retry.rs not in live status) | Functions: `is_transient_tool_failure` :130, `tool_call_is_retryable` :149, `backoff_before_attempt` :177. Loop: LIVE agent.rs :1916, warn :1964 |

Adjacent citations for F-t3-049 in section 8 (:1510-1511):

| Cited from | Cited path:lines | Claim | WT | LIVE | Corrected |
|---|---|---|---|---|---|
| sec 8 :1511 | `sse_events.rs:144` | 23 event variants | DRIFT: enum starts :105, 21 variants | MATCH: enum :144, 23 variants | LIVE only |
| sec 8 :1511 | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/streams.rs:99-109` | BFF relays bytes verbatim | MISMATCH: :99-109 = `apply_org_zdr_posture` and `effective_zdr`. Proxy call :113; byte-level proxy comment :186 | clean | :113 and :186 |
| sec 8 :1511 | `apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts:1488-1760` | SPA stream consumer | MATCH (approx.). `onUnknownEvent` :664, :1451, :1719; cases 'done' :1471 through 'queued_input' :1693 | DIFFERS-IN-LIVE (M, +53). Cases 'done' :1502, 'stopped' :1510, 'error' :1525, 'queued_input' :1725, 'retrieval_notice' :1740 | LIVE 1502-1740 |

## 3. Affected-file map

Line numbers are WT (HEAD) unless marked LIVE. Status is the live `git status` class.

### 3a. Model Plane: producer, gateway, execution

| Path | Role | Lines | Status | Tests (path, test names where cheap) |
|---|---|---|---|---|
| model-gateway/src/sse_events.rs | ChatEvent enum; `family()` gating; `should_emit()`; `name()`; `payload()` | enum :105-345; family :351-382; should_emit :389; name :398 | M (+203). LIVE enum :144-404 (23 variants); RetentionClass :112-116; RetrievalNotice :368; ZdrError :384 (uses `mp_contracts::zdr_refusal::Refusal`) | In-file: plain_path_emits_only_control_events :1363; opt_in_unlocks_only_the_requested_families :1448; tools_family_covers_both_call_and_result :1489; names_and_payload_shapes_are_stable :1507; a_subscription_error_is_an_error_frame_that_omits_unknown_timing :1073; usage tests :1631, :1692; turn_timing_is_gated_exactly_like_usage :1796; the_wire_carries_what_was_recalled_not_only_how_much :1856. LIVE adds helper retrieval_notice() :1997 |
| model-gateway/src/sse.rs | Stream driver; `.event("done")` sites; overflow shed; compaction and skill logs; helpers emit_and_buffer, emit_prepared_failure, emit_turn_timing | Shed 2783-2857; consts CONTEXT_LENGTH_DROP_GROUP :4450, KEEP_TAIL :4457; compaction-memory warn :5009. LIVE: shed 2911-2984; consts :4581, :4588; compaction warn 5607-5610; skill warn 5758-5766; with/persisted_compaction_summary :3865, :5421 | M (+883): adds zdr_error_stream, prepared_zdr_refusal_stream, RetrievalNotice from grounding notice | In-file test module (not enumerated). Contract tests below |
| model-gateway/src/compaction.rs | Compaction helpers: drop_oldest_group :791; drop_oldest_history_group :829; is_context_length_status :941 | HEAD | clean | In-file tests; tests/overflow_contract.rs; tests/skill_budget_contract.rs |
| model-gateway/src/session_flow.rs | Compaction summary persistence (`persisted_compaction_summary`, `with_compaction_summary`, called from sse.rs) | - | M (+5) | not enumerated |
| model-gateway/src/tool_loop.rs | Gateway tool retry loop (plain-chat loop); retry warn | Loop 4011-4060; warn :4055. LIVE warn :4064 | M (+425): adds retrieval_notice_events(), RETRIEVAL_NOTICE_KEY | not enumerated |
| model-gateway/src/tool_retry.rs | Gateway mirror of the retry policy: MAX_TOOL_ATTEMPTS :29; is_transient_tool_failure :101 | HEAD | clean | tests/tool_retry_contract.rs (asserts marker lists byte-identical; refs :96, :176) |
| model-gateway/src/turn_timing.rs | TurnTimingReport; consumes ProviderTiming.attempts | :248, :261 | clean | In-file |
| model-gateway/src/http_routes.rs | Subscription failure JSON: retryable :647, retry_after_ms :653, retry_at :656 | HEAD | M (+237) | not enumerated |
| model-gateway/src/zdr_precheck.rs | ZDR refusal wire; retryable field :92 | HEAD | M | not read |
| model-gateway/src/chat_turn_registry.rs | Turn registry | - | M (+36, test) | a_private_answer_is_flagged_for_the_turn_that_follows_it |
| execution-core/src/runtime_loop/agent.rs | Deployed-agent tool retry loop; warn "run_agent: retrying tool after a transient failure" | Loop 1912-1958; warn :1957. LIVE loop :1916, warn :1964 | M (+207, mostly tests) | In-file (LIVE adds plain_caller, real_waived_rules_for, waived-run lineage test) |
| execution-core/src/runtime_loop/retry.rs | Pure retry policy (see section 2) | :37, :45, :53, :130, :149, :177 | clean | In-file tests from :283 (asserts at :305, :308, :407) |
| execution-core/src/runtime_loop/skill_budget.rs | Skill block fitting; truncated/dropped counters; marker text | :38, :64, :86, :92 | clean | In-file tests :113, :141; model-gateway/tests/skill_budget_contract.rs |
| execution-core/src/runtime_loop/mod.rs | Runtime loop entry | - | M (+18). Added lines have no keyword match | not determined |
| execution-core/src/compaction_budget.rs | Compaction budget (doc cites runtime_loop::retry) | :18 | clean | execution-core/tests/compaction_parity_contract.rs |
| session-core/src/compaction.rs, compaction_tests.rs | Session-owned compaction (ownership row in section 6) | - | clean | compaction_tests.rs |
| session-core/src/terminalization.rs | Terminal outcome receipts; retention-aware managed-run start | - | M (+123) | not enumerated |
| inference-core/src/grpc.rs | Inference gRPC adapter; ProviderTiming on responses | - | M (+131) | not read |
| inference-core/src/provider/timing.rs; metered.rs; streaming.rs | ProviderTiming production (from grep) | - | clean | not read |
| model-gateway/src/grpc.rs | Gateway gRPC adapter | - | M (+47) | not read |

### 3b. Protobuf, contracts, gate

| Path | Role | Lines | Status |
|---|---|---|---|
| proto/model_plane/v1/inference.proto | ProviderTiming message; provider_timing fields | fields :306 (14), :369 (15); message :385-423; attempts :411; failed_attempts_ms :415; retry_wait_ms :419 | clean |
| proto/model_plane/v1/execution.proto | retry_after_ms = 7 (containing message not opened) | :257-260 | clean |
| proto/model_plane/v1/sessions.proto | CreateThreadRequest (:198); origin field; CreateThreadResponse (:266) | origin :263 | M (+42): CountWaivedRunsRequest/Response, WaivedRunCount |
| api/contracts/infer-statuses.json | inference-core provider-error vocabulary v1 (kinds include rate_limited, too_long, subscription_usage_limit; trailer x-mp-retry-after-ms) | 6657 bytes | clean. Consumer: inference-core/src/grpc.rs (grep hit) |
| api/contracts/zdr-refusals.json (+ .sha256) | ZDR refusal catalogue | - | LIVE-only (??). Frontend copy at verevonv3/src/shared/api/contracts/zdr-refusals.json (+ .sha256) also ?? |
| api/contracts/README.md | Contracts readme | - | M (+1) |
| docs/sse-event-taxonomy.md | Committed generated taxonomy. WT 24 rows starting "| `"; LIVE 28 | WT lacks plan_proposed and source_scope_applied rows | M (+4) |
| scripts/gen-sse-event-taxonomy.py | The gate. PRODUCER :37 (sse_events.rs); CONSUMER :38 (chat-client.ts); DOC :39; NON_CHATEVENT_CONSUMER_CASES :43-49 (connected, chunk, done, citations, search_results); read() :52-61; VARIANT_ARM regex :81-83 | clean. No unit tests; drift-proof step lives in CI |
| .github/workflows/sse-taxonomy.yml | CI: path triggers on sse_events.rs, sse-event-taxonomy.md, the gate script and chat-client.ts; runs --check; drift-proof step deletes the memory_recall row, expects failure | clean |
| .github/workflows/model-plane-rust-ci.yml | cargo fmt/check/test steps at :99, :105, :112, :119 | M (+50, not read) |

### 3c. Frontend Plane (Verevon v3) and BFF

| Path | Role | Lines | Status | Tests |
|---|---|---|---|---|
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts | SPA stream consumer; onUnknownEvent; event cases | onUnknownEvent :664, :1451, :1719. WT cases: 'done' :1471, 'stopped' :1479, 'error' :1494, 'memory_recall' :1679, 'queued_input' :1693. LIVE: 'done' :1502, 'stopped' :1510, 'error' :1525, 'queued_input' :1725, 'retrieval_notice' :1740 | M (+53): ChatRetrievalNoticeEvent type, onRetrievalNotice handler, case 'retrieval_notice' | chat-client.test.ts (M, +22); chat-client.replay.test.ts; chat-client.served-by.test.ts; chat-client.zdr-capability.test.ts (clean) |
| .../src/shared/api/sse.ts | SSE line parser | - | M (+4), not read | - |
| .../src/shared/api/ag-ui-client.ts | AG-UI run vocabulary (card: must be covered) | - | clean, mapping not read | ag-ui-client.test.ts (clean) |
| .../src/features/chat/components/chat-types.ts | StreamFailureAction union ('retry', 'reconnect', 'wait', 'open_integrations', 'none') | - | M (+7) | - |
| .../src/features/chat/components/chat-normalizers.ts | Normalisers; zdr-refusal action mapping ('retry') | - | M (+39) | chat-normalizers.test.ts, .wave2.test.ts, .reconcile.test.ts (clean); .zdr-refusal.test.ts (??) |
| .../src/features/chat/components/use-chat-controller.ts | Stream state and recovery | - | M (+4), not read | use-chat-controller.recovery.test.ts, .follow-up.test.ts, .resume-replay.test.ts, .plan-gate.test.ts (clean); .zdr-refusal.test.ts (??) |
| .../src/features/chat/components/ChatMessages.tsx | Message rendering; error notice links | - | M (+229) | ChatMessages.failure-actions.test.tsx (M, +34); .processing-line, .run-notify, .truth tests (clean) |
| .../src/features/chat/components/ChatPage.tsx | Page wiring | - | M (+13), not read | ChatPanels.test.tsx (clean) |
| .../src/features/agents/components/AgentRunConsole.tsx | Run console; status pill (WT :2314) | :2314 | M (+11) | AgentRunConsole.test.tsx (M, +40) |
| .../src/features/core/components/sidebar/ChatSidebarOrganizer.tsx | Sidebar organizer (card F-t3-015: status pills) | - | M (+2), not read | not identified |
| .../src/features/spaces/lib/space-thread-presentation.ts | threadStatus(thread, tr) label function; precedence body not read | :33 | clean | Call sites SpaceRoomTimeline.tsx :384, SpacePage.tsx :1298; SpaceRoomTimeline.test.tsx :79 (status pill colour) |
| .../src/features/chat/components/ChatWideResearchPanel.tsx | statusPillClass | :78 | clean | ChatWideResearchPanel.test.tsx |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/streams.rs | BFF byte-level SSE proxy; ZDR posture settled before send | ZDR :99-109; proxy call :113; "raw byte-level SSE proxy" comment :186 | clean | not enumerated |
| apps/Frontend Plane/verevonv3/package.json | Scripts (test, typecheck, lint, build, test:e2e) | - | M (+2: demo:build, demo:probe) | n/a |

Connector incidents: no file found. integration-corev2 (Ingestion Plane) not opened.

## 4. Related contracts

- SSE event names. ChatEvent WT (21 variants): result_receipt, reasoning_delta, step_update, tool_call, tool_result, citation, source_scope_applied, grounding, artifact, attachment, usage, turn_timing, title, follow_ups, plan_proposed, verification, memory_recall, queued_input, stopped, error (Error and SubscriptionError both map to "error"). LIVE (23) adds retrieval_notice and ZdrError (ZdrError maps to "error"). Transport names: connected, chunk, done, citations, search_results (gate :43-49).
- SPA-only names not in the gate's exception list (WT chat-client.ts :1368-1411): awaiting_approval, run_paused_for_approval, run_resumed_after_approval, approval_state_changed, browser_action_approval_required, browser_action_decided, approval_continuation_verified. Emitter not traced.
- Error frame already carries `retryable` (WT `sse_events.rs` :323-327). A user-facing retry flag exists without a new event.
- Proto: see 3b. gateway.proto not opened.
- Migrations: `apps/Model Plane/rust/services/session-core/migrations/`. WT latest = 0043_audit_outbox_requeue.sql (0038-0043 listed). LIVE adds 0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql (both ??) and revert/ (??). No migration name refers to retry, compaction or thread status.
- Gateway route: the BFF proxies through `proxy_sse_stream_with_data_plane` (streams.rs :113). The Model Plane SSE route is not named here (not traced).
- Config and env: none determined.
- Action contracts (`apps/Frontend Plane/verevonv3/src/shared/actions/`): directory exists (home-updates.ts M live; chat-work-period.ts ?? live). No run-lifecycle action contract identified.

## 5. Test and build commands

Rust (`apps/Model Plane/CLAUDE.md`, Testing and Build):
- Per crate: `cd "apps/Model Plane/rust" && cargo test -q -p model-gateway --all-targets` (also `-p execution-core`, `-p session-core`, `-p inference-core`). Workspace: `cargo test --workspace`. Crate names verified from Cargo.toml: model-gateway, execution-core, session-core, inference-core, mp-contracts.
- User memory note (Model Plane build): two cargo targets fail pre-existing on Windows, so use --lib. Go -race needs cgo (unavailable).
- CI rust (`.github/workflows/model-plane-rust-ci.yml`): `cargo fmt --all -- --check` (:99), `cargo check --workspace` (:105), `cargo check --workspace --all-targets` (:112), `cargo test --workspace --no-fail-fast` (:119).

Taxonomy gate (`.github/workflows/sse-taxonomy.yml`):
- `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`. Windows: `PYTHONUTF8=1 PYTHONIOENCODING=utf-8 python "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`.
- WT (HEAD), PYTHONUTF8=1: exit 1. Only drift: the committed doc lacks the `plan_proposed` and `source_scope_applied` rows (generated side only).
- LIVE, PYTHONUTF8=1: exit 0.
- Without PYTHONUTF8: both trees exit 1 with a cp1252 false "stale" (mojibake in committed side).
- Never run without `--check` (it rewrites the doc).

Contract tests (named by grep): model-gateway/tests/tool_retry_contract.rs, skill_budget_contract.rs, overflow_contract.rs; execution-core/tests/compaction_parity_contract.rs, autonomy_ladder_contract.rs.

Go: per service, `go test ./...` and `go vet ./...` (`apps/Model Plane/CLAUDE.md`). capability-core module not run.

Frontend (`apps/Frontend Plane/verevonv3/package.json`): `pnpm test` (node --test on four gateway scripts, then `vitest run`), `pnpm typecheck` (`tsc -b`), `pnpm lint` (`eslint .`), `pnpm build` (typecheck plus vite build), `pnpm test:e2e` (playwright). Single file: `pnpm vitest run <path>`.

BFF crate: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (from `apps/Frontend Plane/verevonv3`, root CLAUDE.md).

User memory note (verevonv3): pre-existing typecheck, lint and vitest failures; use a second dev server for worktree checks.

## 6. Owners

- `apps/master-ownership-matrix.md`: :41 (planning, reasoning, run loops, tool choice = Model Plane). :125-130 (model-gateway: public invoke and stream; session-core: threads, runs, checkpoints, compaction, context assembly; inference-core: provider routing and streaming; capability-core (Go): tools, MCP, safety). :85-86 (MCP and plugin registries = capability-core).
- `apps/Model Plane/docs/capability-ownership-matrix.md`: :54 (run metadata and lifecycle = session-core; execution-core transitions; NATS run-events). :56 (checkpoints, context assembly, compaction = session-core; execution-core triggers). :59 (tool, skill, model, plugin, MCP, routing, safety registries = capability-core). :65 (sandbox leases = sandbox-manager). :70 (subagent lifecycle = orchestrator-core). :88 (cost ledger = cost-core).
- Root `CLAUDE.md`: Frontend Plane owns the Verevon v3 UI and same-origin gateway/BFF normalisation. Application Plane owns collaborative/realtime projections and notifications. Provider actions go through integration-corev2's actions surface.
- `apps/Model Plane/CLAUDE.md` (Code Style): the two tool-dispatch loops (gateway dispatch_tool and execution-core execute_step_inner) must stay separate. Retry visibility therefore has two sites (tool_loop.rs and agent.rs) unless one is chosen deliberately.
- Connector incidents: owner not determined. The card puts them in capability-core (consistent with master :130), but connector tokens and actions belong to integration-corev2 (root CLAUDE.md), and capability-ownership-matrix has no connector row (only line 9, Visma MCP as an external connector).

## 7. Uncommitted-only (live tree)

Modified files relevant to this item (LIVE diff stat in C:/dev/CoresSystem, changed lines):

| Path | Changed lines | One-line summary |
|---|---|---|
| model-gateway/src/sse_events.rs | 203 | Adds RetrievalNotice (control, :368), ZdrError (:384), RetentionClass (:112-116 region); "error" arm covers ZdrError |
| model-gateway/src/sse.rs | 883 | zdr_error_stream, prepared_zdr_refusal_stream, RetrievalNotice from grounding; shed block moved +128 lines |
| model-gateway/src/tool_loop.rs | 425 | retrieval_notice_events(): tool-result notice becomes a typed event; retry warn moved to :4064 |
| execution-core/src/runtime_loop/agent.rs | 207 | Tests added; retry loop moved to :1916 |
| execution-core/src/runtime_loop/mod.rs | 18 | Not determined |
| session-core/src/terminalization.rs | 123 | Retention-aware managed-run start; retry-receipt match |
| model-gateway/src/chat_turn_registry.rs | 36 | Test added |
| model-gateway/src/http_routes.rs | 237 | Not read |
| model-gateway/src/normalize.rs, state.rs, session_flow.rs | 17, 6, 5 | Not read |
| model-gateway/src/zdr_precheck.rs | (M) | Not read |
| model-gateway/src/grpc.rs; inference-core/src/grpc.rs | 47; 131 | Not read |
| proto/model_plane/v1/sessions.proto | 42 | Adds CountWaivedRunsRequest/Response and WaivedRunCount |
| docs/sse-event-taxonomy.md | 4 | 24 to 28 rows; LIVE gate passes. Inferred composition: ZdrError, retrieval_notice, plan_proposed, source_scope_applied (from the WT gate diff and the variant lists) |
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts | 53 | Adds ChatRetrievalNoticeEvent, onRetrievalNotice, case 'retrieval_notice' |
| .../src/shared/api/sse.ts | 4 | Not read |
| .../src/shared/api/chat-client.test.ts | 22 | Tests added |
| .../src/features/chat/components/chat-types.ts | 7 | StreamFailureAction union |
| .../src/features/chat/components/chat-normalizers.ts | 39 | Zdr refusal action mapping ('retry') |
| .../src/features/chat/components/use-chat-controller.ts | 4 | Not read |
| .../src/features/chat/components/ChatMessages.tsx | 229 | Error notice links (ChatGPT connection) and related rendering |
| .../src/features/chat/components/ChatMessages.failure-actions.test.tsx | 34 | Tests |
| .../src/features/chat/components/ChatPage.tsx | 13 | Not read |
| .../src/features/agents/components/AgentRunConsole.tsx; AgentRunConsole.test.tsx | 11; 40 | Not read; tests |
| .../src/features/core/components/sidebar/ChatSidebarOrganizer.tsx | 2 | Not read |
| apps/Frontend Plane/verevonv3/package.json | 2 | Adds demo:build and demo:probe scripts |
| .github/workflows/model-plane-rust-ci.yml; verevonv3-ci.yml | 50; 55 | Not read |
| apps/Model Plane/api/contracts/README.md | 1 | Not read |

Untracked, LIVE-only, relevant (??):
- apps/Model Plane/api/contracts/zdr-refusals.json (+ .sha256); apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json (+ .sha256)
- apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts (+ zdr-refusals.test.ts); http.zdr-refusal.test.ts; chat-client.zdr-refusal.test.ts; src/shared/i18n/errors.zdr-refusal.test.ts
- apps/Frontend Plane/verevonv3/src/features/chat/components/chat-normalizers.zdr-refusal.test.ts; use-chat-controller.zdr-refusal.test.ts; ChatUtilityPage.tsx; ChatWorkPeriodPicker.tsx; EmptyChatState.test.tsx
- apps/Frontend Plane/verevonv3/src/features/core/components/sidebar/ChatWorkLinks.tsx; src/shared/actions/chat-work-period.ts (+ test)
- apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs; retention.rs; tests/zdr_retention_matrix.rs; tests/fixtures/
- apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs; zdr_posture.rs; relevance_calibration.rs; calibration/; tests/zdr_feature_table.rs; tests/zdr_posture_matrix.rs; tests/zdr_refusal_inventory.rs; tests/fixtures/zdr-posture-matrix.json (+ .sha256)
- apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs; inference-core/tests/zdr_refusal_inventory.rs; inference-core/src/grpc/zdr_waiver_route_tests.rs
- apps/Model Plane/rust/services/session-core/src/retention_receipt.rs; migrations/0044_run_retention_receipt.sql; migrations/0045_zdr_waived_runs_org_index.sql; migrations/revert/

Deleted in LIVE (" D") and relevant to this item: none.

Commit-order risk: LIVE `sse_events.rs` (M) names `mp_contracts::zdr_refusal::Refusal` (:385), and `mp-contracts/src/zdr_refusal.rs` is ??. Committing sse_events.rs or sse.rs without that untracked module (and mp-contracts lib.rs, M) would not compile at HEAD. The new gate pass also depends on the combined live tree.

## 8. Protect list

- Byte-verbatim relay: the BFF proxies raw bytes (streams.rs :113 call; :186 comment). New events need no gateway allowlist. Keep the BFF allowlist-free (audit :1511).
- Taxonomy gate: (a) each variant must appear as a `ChatEvent::X { .. }` arm in family() and name(), which is the form VARIANT_ARM matches (gen script :81-83); (b) each event name needs `case '<name>':` in chat-client.ts (consumer coverage, gen script :14-19); (c) regenerate docs/sse-event-taxonomy.md; (d) control events (family None) always emit, rich events need a feature family.
- Unknown names are logged through `onUnknownEvent` (chat-client.ts :1451, :1719), not fatal at runtime; the gate is what fails at build time.
- ZDR: compaction markers carry counts only, never the summary (card :1014). New stores refuse or degrade under ZDR and add a golden-matrix case (audit :1509). Golden-matrix files are LIVE-only (??).
- Two tool-dispatch loops stay separate (apps/Model Plane/CLAUDE.md). Retry visibility applies to tool_loop.rs and agent.rs.

## 9. Corrections to the card

- "23 variants": true in LIVE only. WT (HEAD) has 21.
- "ChatEvent has done, Stopped and Error frames": only Stopped and Error are variants. done is a transport frame (gate :43-49).
- sse_events.rs:142-180 and :340-395, and sse.rs:2905-2990, match LIVE lines, not WT. retry.rs:1-60 matches in content, but the retry loop is in agent.rs (and tool_loop.rs).
- "Retry visibility needs an inference-core proto change": partly superseded. ProviderTiming already has failed_attempts_ms and retry_wait_ms. Gateway source does not reference them. Tool-level retries do not involve inference-core.
- "Trigger kind on the run record": only thread origin exists (sessions.proto :263, on CreateThreadRequest). No run-level origin found.
- Compaction is already silent in two places: sse.rs WT :5009 (warn "compaction memory lookup failed; summarizing without it") and LIVE 5607-5610 (warn, truncation fallback).
- turn_end: no identifier in apps/Model Plane/rust/services in either tree (0 matches). The terminal frames are done (transport), stopped and error, which matches the card's "normalise client-side" approach.

## 10. NOT DETERMINED

- Whether execution-core loop events can reach the chat stream (the session_channel sink was not traced).
- Content of LIVE-only changes not read: runtime_loop/mod.rs (+18), http_routes.rs, normalize.rs, state.rs, session_flow.rs, zdr_precheck.rs, ChatPage.tsx, ChatSidebarOrganizer.tsx, use-chat-controller.ts, sse.ts, AgentRunConsole.tsx, model-gateway/grpc.rs, inference-core/grpc.rs, both CI workflows, api/contracts/README.md.
- Whether a sidebar derives one thread status with precedence and unseen state: ChatSidebarOrganizer.tsx and ChatWorkLinks.tsx were not read. The grep for unseen/unread found no implementation.
- threadStatus precedence body (space-thread-presentation.ts :33) and the AG-UI mapping (ag-ui-client.ts).
- Emitter of the SPA approval event names and whether they belong in the gate's exception list.
- Connector incident source and owner (section 6). capability-core "incident" hits are scoring only.
- Whether retry_wait_ms and failed_attempts_ms are consumed inside inference-core (only model-gateway was grepped).
- Gate result on Ubuntu CI (not run here). The Windows results above use PYTHONUTF8=1 for the real diff.
- No test or build suite was executed (read-only sweep).
- The "last 10 incidents per MCP server" reference claim (Claude desktop) was not verified.
