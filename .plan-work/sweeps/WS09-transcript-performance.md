# WS09-transcript-performance: mechanical code-map sweep

Audit item: F-t3-021 "Transcript performance" (Priority, Partial, effort M, wave W1, confidence High). Read-only sweep; no product file was changed.

## Sources and conventions

- Audit card: `scratch-2026-10-12-69a4ee/03-model-plane-chat-and-agents-audit.md` lines 646-662. Other placements: 89 (summary), 301, 1383, 1543 (9.1), 1593, 1659, 1669 (9.2), 1748 ([INF] claim), 1838 (Appendix B), 2088-2093 (Appendix C). Appendix D (2111-2161) has no F-t3-021 entry.
- WT = worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean except untracked `.plan-work/`).
- LV = live tree `C:/dev/CoresSystem` (same HEAD plus 324 entries in `.plan-work/live-tree-status.txt`). The audit read LV.
- FP = `apps/Frontend Plane/verevonv3`. BFF = `FP/apps/gateway`. MP = `apps/Model Plane`. All paths are repo-relative.
- Verdicts: MATCH (symbol or behaviour at the cited lines), DRIFT (found at other lines, new range given), ABSENT (not found). Live status: clean, M (differs from HEAD), ?? (LV-only untracked), D (deleted).
- Where WT and LV differ, both are given as "WT / LV".

## 0. Result summary

- All card ranges resolve to real files. In LV (the tree the audit read) every cited range MATCHES except one mis-citation: `ChatMessages.tsx:669` is the per-message node list, not the list of turns.
- Two ranges DRIFT in WT only, because LV carries uncommitted edits above them: `sessions.proto:901-918` (WT 890-907) and `http_routes.rs:8470-8535` (WT 8366-8423). Both still match the card in LV.
- The turn-level list is `FP/src/features/chat/components/ChatPage.tsx` `<For each={messageRows()} keyed=...>` at 827 (WT) / 838 (LV), keyed by `row.turn.id`. Older turns are not remounted; the cost is DOM size.
- ABSENT: any `vite:preloadError` listener or reload-once guard (WT git grep; LV source grep). The only `vite:preloadError` text is Vite's own preload helper inside an LV-only built demo bundle (`FP/apps/verevon-web/public/product-demo/assets/web-BUbEXY8B.js`).
- ABSENT: `reconcileMarkdownBlocks` (named only in the I5 plan JSON, line 1337).
- NOT BUILT: delta batching. `use-chat-controller.ts` appends per token at 2614 (live) and 1643 (resume). The only rAF in the file is launch motion (1947-1952).
- Unbounded reads confirmed in both trees: `ListConversationRequest` has no limit or cursor field; session-core `list_conversation` runs `ORDER BY m.sequence` with no LIMIT. Index `idx_messages_thread_id (thread_id, sequence)` exists in `session-core/migrations/0001_init.sql`.
- The only existing paged read near the transcript is the events replay (`/v1/threads/:id/events`, forward from `after_event_id`, default 160, max 500). It is not tail-first and the card does not cite it.
- The audit's [INF] claim about a browser-local snapshot is now SRC-supported: `chat-thread-history.ts:13` stores `verevon.chat.threadTranscripts.v1` in localStorage, capped at 40 threads, 120 turns per thread and 32,000 characters per turn (lines 18-24).
- Benchmarks: no DOM render benchmark and no cold-start request budget exist. Present: a unit per-token budget (`derive.budget.test.ts`), a Playwright SSE latency microbenchmark (`product-latency.spec.ts`, Q08) and browser-clock phase stamps (`product-phase-timing.ts`).
- LV uncommitted edits touch the same files (ChatMessages, ChatPage, App, index.tsx, use-chat-controller, chat-client, sessions.proto, http_routes, session-core grpc). An added-line scan found no change to the markdown render, delta append or list/paging code. Those edits are ZDR refusal plumbing, work-period and suggestion UI, demo boot and retention counts. This is an added-line scan, not a full diff review.

## 1. Card digest

- Items (card 652): (1) Playwright DOM render benchmark and cold-start request budget; (2) plan I5, block-level incremental Markdown render with trailing-block re-parse and frame-batched deltas; (3) reload once on an unrecoverable chunk error; (4) only if measured, a virtualised message list with live-follow and a scroll-to-end pill, plus tail-first paged transcript reads.
- Sequence (card 660): benchmark, I5, reload-once, then conditional items. Drop the worker-highlighting sub-item: the highlighter is a regex scanner skipped above 120,000 characters.
- Plan I5: `FP/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:217` (size S) and `chat-gap-closure-plan-2026-10-01.json:1334-1352`. Caveat at JSON 1749: do not change I5 without the mount-counter test.
- Appendix C corrections (03:2091): Claude's 257-module registry is not transcript evidence; DeepSeek's startup budget counts only settings and describe calls; "benchmark first" is a Verevon recommendation, since no reference benchmarks transcript rendering.

## 2. Citation verification

Columns: card id | cited path:lines | claim | WT verdict | LV status | corrected path:lines.

| id | cited | claim | WT verdict | LV status | corrected (WT / LV) |
|---|---|---|---|---|---|
| 021 | FP/src/app/App.tsx:8-40 (card: App.tsx:8-40) | route-level lazy loading | MATCH: lazy declarations 8-36, SessionLoading 38-45 | M (+11: ChatUtilityPage lazy, scene routes) | 8-36 / 8-37 |
| 021 | FP/src/shared/chat-nodes/derive.budget.test.ts:1-40 | per-token streaming budget test | MATCH (heavy-turn fixture 39-50) | clean | 1-50 / same |
| 021 | FP/src/features/chat/components/ChatMessages.tsx:1085-1110 | ChatMarkdown reparses and remounts per token | MATCH: ChatMarkdown 1084-1092, For at 1087 | M (+2 above this region) | 1084-1092 / 1086-1093 |
| 021 | ChatMessages.tsx:1205-1250 | TranscriptCodeBlock remounts; highlight only after fence closes | MATCH: comment 1192-1215, function 1216, highlighted memo 1246-1249 | M | 1216-1249 / 1218, memo 1248-1251 |
| 021 | FP/src/features/chat/components/chat-media-markdown.tsx:745-790 | parseMarkdownBlocks reparses whole message; closed flag on fences | MATCH: FENCE_OPEN_PATTERN 749, parseMarkdownBlocks 751, closed 785-787 | clean | same |
| 021 | ChatMessages.tsx:669 | "all turns sit in one un-virtualised list" | WRONG OBJECT: 667 is the per-message node list (`<For each={nodeEntries()}>`) | M | node list 667 / 669; turn list is ChatPage.tsx 827 / 838 |
| 021 | FP/src/features/chat/components/use-chat-controller.ts:1954-1966 | autoFollow, handleScroll (dist < 80), scrollToBottom | MATCH: handleScroll 1954-1959, scrollToBottom 1961-1966 | M, lines unchanged (LV edits at 3283+) | same |
| 021 | card: "scroll-to-latest button" | scroll-to-end pill | pill at ChatPage.tsx 921-926 | M | 921-926 / 932-937 |
| 021 | FP/tests/e2e/product-phase-timing.ts (card: "stamps") | phase timing stamps for the budget | MATCH: 318 lines, performance.now() stamps (header 7, 18), splitTimedFrames 116, productPhaseReport 164 | clean | same |
| 021 | FP/apps/verevon-web/plans/chat-workspace/VEREVON_CHAT_WORKSPACE_IMPLEMENTATION_PLAN.md:435 | virtualise only after measurement | MATCH: rule 9 at 435 | clean | same |
| 021 | FP/apps/gateway/src/domains/chat/history.rs:590-645 | BFF transcript read and adapter | MATCH: read_canonical_transcript 595-640, canonical_messages_to_transcript 642 | clean | same |
| 021 | history.rs (card: canonical_messages_to_transcript, read before paging design) | adapter exists | MATCH: fn 642, call 636, tests 1089-1297 | clean | same |
| 021 | MP/proto/model_plane/v1/sessions.proto:901-918 | ListConversationRequest has no limit or cursor | DRIFT: message at 890-907 (fields org_id, thread_id, space_id, decision ref, decision token; no limit or cursor) | M (+11) | 890-907 / 901-918 (MATCH) |
| 021 | MP/rust/services/model-gateway/src/http_routes.rs:8470-8535 | list handler; query struct has no limit | DRIFT: ThreadMessagesQuery 8356-8364, list_thread_messages 8366-8423, ListConversationRequest built at 8387 | M (+237 lines, ZDR) | 8356-8423 / 8470-8477 struct, 8479-8536 handler, request 8500 (MATCH) |
| 021 | card: "No vite:preloadError self-heal" | no reload-once on preload error | ABSENT: git grep no hits | LV source: ABSENT; only Vite helper text in a ?? built bundle | n/a |
| 021 | card: reload once on unrecoverable chunk error | reload guard | ABSENT: no location.reload and no window error or unhandledrejection listener in FP/src (non-test). Entry: FP/src/index.tsx:9 | index.tsx M; no guard added in LV | n/a |
| 021 | I5 plan: reconcileMarkdownBlocks(prev, next) | reconcile helper | ABSENT in code; named in plan JSON 1337 only | n/a | n/a |
| 021 | I5 plan JSON: ChatMarkdown :1052-1060; copyOutcomeCache ~:1156-1215; node list ~:642 | audit-era line refs | DRIFT: ChatMarkdown 1084-1092, copyOutcomeCache 1199, node For 667; keyed-off comment 653-657 (JSON cites 627-632) | M | 1084-1092 / 1086-1093; 1199 / 1201; 667 / 669 |
| 021 | I5 plan JSON: use-chat-controller onMessage :2609-2615 and :1619-1645 | delta append sites | MATCH: live 2609 and append 2614; resume 1619 and append 1643 | M, unchanged here | same |
| 021 | I5 plan / card: rAF "frame-batched deltas" | is there frame batching today? | NO: rAF at 1948 is launch motion inside triggerLaunchMotion 1947-1952 | M | same |
| 021 | FP/src/features/chat/components/chat-code-highlight.ts:1-40 | regex scanner, skipped above 120,000 chars | MATCH: header 33-37, MAX_HIGHLIGHT_CHARS = 120_000 at 38; skip branch at 213 (outside cited window) | clean | 38 and 213 / same |
| 021 | card: session-core ListConversation paging (scope) | unpaged session-core read | MATCH: list_conversation 5216-5328; SQL "ORDER BY m.sequence" at 5291; no LIMIT | M (+861 lines) | 5216-5328 / 5387; SQL 5462; no LIMIT in 5387-5507 |
| 021 | audit [INF]: SPA keeps a local snapshot (03:1748) | browser-local transcript cache | SRC-SUPPORTED: key at chat-thread-history.ts:13; caps 18-24; writes 417 and 426; read 431; writeClientJson in shared/session/client-storage.ts:63 (localStorage, sessionStorage fallback 7-12) | clean | same |

## 3. Affected-file map (depth 2)

Columns: path | role | lines WT / LV | LV status | existing tests.

### 3a. Frontend (FP)

| path | role | lines WT / LV | status | tests |
|---|---|---|---|---|
| FP/src/index.tsx | SPA bootstrap (index.html:14); render(App) | 9 / 9-20 | M (+12/-1: boot() wrapper; VITE_SCENE_MODE dynamic import of @/demo/install) | none found |
| FP/src/app/App.tsx | lazy route table (ChatPage, ChatUtilityPage, ...) | 8-36 / 8-37 | M (+11) | none direct |
| FP/src/features/chat/components/ChatPage.tsx | turn list: messageRows memo 279 / 288; keyed For 827 / 838; onScroll 803 / 814; scroll pill 921-926 / 932-937 | as listed | M (+13) | ChatPage.voice-first-turn.test.tsx |
| FP/src/features/chat/components/ChatMessages.tsx | ChatMarkdown 1084-1092; MarkdownBlockView 1094; MarkdownCodeBlock 1170; copyOutcomeCache 1199; TranscriptCodeBlock 1216-1249; node memo 528 and 532; node For 667 / 669 | as listed | M (229 lines: suggestion and starter-prompt rows; no markdown change in added lines) | ChatMessages.codeblock.test.tsx (MAX_HIGHLIGHT_CHARS fallback, lines 69-70); ChatMessages.failure-actions.test.tsx (M) |
| FP/src/features/chat/components/chat-media-markdown.tsx | parseMarkdownBlocks (pure); FENCE_OPEN_PATTERN; closed flag | 749, 751, 785-787 | clean | chat-markdown.test.ts (fence and table cases; closed:true at line 13) |
| FP/src/features/chat/components/chat-code-highlight.ts | dependency-free scanner; MAX_HIGHLIGHT_CHARS; highlightCode | 33-38, 209, 213 | clean | ChatMessages.codeblock.test.tsx:69-70 |
| FP/src/features/chat/components/use-chat-controller.ts | per-token delta append (resume 1619-1643, live 2609-2615); autoFollow ref 374; showScrollDown signal 253; scroll effect 1940-1944; launch rAF 1947-1952; handleScroll and scrollToBottom 1954-1966; reasoning delta 3109-3114. Its flush() calls are store-write flushes, not render frames | as listed (LV identical below 3282) | M (+4 at 3283-3295) | use-chat-controller.resume-replay.test.ts; .follow-up.test.ts; .recovery.test.ts; .continue-generation.test.ts; ?? use-chat-controller.zdr-refusal.test.ts |
| FP/src/shared/chat-nodes/derive.ts | deriveConversationNodes (per-message node list) | whole file | clean | derive.test.ts; derive.budget.test.ts (1-50) |
| FP/src/shared/chat-nodes/registry.ts | CHAT_NODE_REGISTRY.render used by ChatMessages node For | whole file | clean | registry.test.ts |
| FP/src/shared/api/chat-client.ts | getThreadMessages 2110 / 2163 (GET /messages); getChatThreadTranscript 2174 / 2227 (GET /transcript); listChatThreadEvents 2183 / 2236 (GET /events with after_event_id and limit); SSE chunk case 1466 / 1497 | as listed | M (+53: retrieval_notice event, ZDR feature) | chat-client.test.ts (M, +22); ?? chat-client.zdr-refusal.test.ts |
| FP/src/shared/api/sse.ts | readSseStream; ApiError gains feature | 65-66 | M (+4/-2) | none direct |
| FP/src/shared/session/client-storage.ts | writeClientJson / readClientJson to localStorage, sessionStorage fallback | 7-12, 19-63 | clean | none direct |
| FP/src/features/chat/lib/chat-thread-history.ts | browser-local thread and transcript snapshots: keys 12-13; caps 18-24; remove 394; writes 417, 426; read 431 | as listed | clean | chat-thread-history.test.ts; chat-thread-history.persistence.test.ts |
| FP/src/shared/actions/ (action-registry.ts) | no transcript-read action; "history" hits are integrations inbox only | n/a | clean | none |
| FP/tests/e2e/product-latency.spec.ts | Q08 SSE and answer latency microbenchmark (samples from Q08_PERF_SAMPLES, line 9; test at 11); Q13 quality checks; no DOM timing | whole file | clean | itself; project product-readiness (testMatch includes "latency") |
| FP/tests/e2e/product-phase-timing.ts | browser-clock phase model; turn_timing kept verbatim | 318 lines; 116, 164 | clean | FP/tests/product-phase-timing.test.ts (tracked); FP/tests/e2e/product-phase-report.ts |
| FP/tests/e2e/chat-workspace.spec.ts | chat e2e; cold-start UI test (comment 39, test 77) | whole file | clean | project chat |
| FP/playwright.config.ts | projects: setup, e2e, local-setup, local, chat, product-recording, product-readiness; env consts 44-47; CHAT_WORKSPACE_SPECS 58; PRODUCT_READINESS_SPECS 59; testDir 68; timeout 69; product-readiness depends on local-setup (125) | whole file | clean | n/a |
| FP/vite.config.ts | no build preload or chunk options (comment at line 50 only) | n/a | clean | n/a |
| FP/index.html | loads /src/index.tsx (line 14) | 14 | clean | n/a |
| FP/vite.demo.config.ts | separate demo Vite build | whole file | ?? | n/a |
| FP/src/demo/ | product-scene runtime (install, imported by index.tsx under VITE_SCENE_MODE=1); product-demo/bridge.ts window error and unhandledrejection listeners (69, 82-83); DemoApp.tsx:31 reload button | n/a | ?? (LV-only) | none |
| FP/package.json | scripts: build 9, lint 11, test 13, test:e2e 15, test:e2e:install 16, typecheck 17, verify 18; deps: @solidjs/web ^2.0.0-rc.0 (25), solid-js ^2.0.0-rc.0 (34), @playwright/test ^1.63.0 (40), vite ^8.2.2 (51), vitest ^4.1.10 (53) | n/a | M (+2: demo:build, demo:probe) | n/a |
| FP/apps/verevon-web/plans/chat-workspace/VEREVON_CHAT_WORKSPACE_IMPLEMENTATION_PLAN.md | rule 9 at 435; virtualisation trigger 732; section 21 stress profile 777-857 (point 11 at 857: browser-side paint measurement absent) | as listed | clean | n/a |
| FP/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md | I5 row | 217 | clean | n/a |
| FP/apps/verevon-web/plans/system-audits/chat-gap-closure-plan-2026-10-01.json | I5 entry 1334-1352; reconcile note 1337; file line refs 1337-1340; caveat 1749; planned include_superseded 1409 | as listed | clean | n/a |

### 3b. BFF (FP/apps/gateway, crate verevon-gateway-rs)

| path | role | lines | status | tests |
|---|---|---|---|---|
| FP/apps/gateway/src/domains/chat.rs | routes: transcript 28-29; messages 46-47; events 49-51 | as listed | clean | n/a |
| FP/apps/gateway/src/domains/chat/history.rs | get_thread_transcript 196; read_canonical_transcript 595-640 (GET MG /v1/threads/{id}/messages); canonical_messages_to_transcript 642 | as listed | clean | in-file: chat_history_* 803, 857, 900; canonical_messages_* 1089, 1135, 1192, 1256, 1297; empty-thread assert 1127 |
| FP/apps/gateway/src/domains/chat/json_handlers.rs | get_thread_messages 239 (proxy, no paging params); get_thread_events 267 (forwards after_event_id) | as listed | clean | none found by name |

### 3c. Model Plane (MP)

| path | role | lines WT / LV | status | tests |
|---|---|---|---|---|
| MP/proto/model_plane/v1/sessions.proto | rpc ReplayThread 66; rpc ListConversation 102; ReplayThreadRequest 712-720 / 723-731 (limit 0 = unlimited); ListConversationRequest 890-907 / 901-918; SessionMessage 922 (WT); ListConversationResponse 940-943 / 951- | as listed | M (+42: SessionRetentionService.CountWaivedRuns) | n/a |
| MP/rust/services/model-gateway/src/http_routes.rs | route 177 /v1/threads/:thread_id/messages to list_thread_messages; route 181 /events to replay_thread_events; list handler 8366-8423 / 8479-8536; replay handler 8465 / 8578 with DEFAULT_LIMIT 160 and MAX_LIMIT 500 at 8480-8481 / 8593-8594 | as listed | M (+237, ZDR) | none found by name for the messages route (grep of src and tests) |
| MP/rust/services/model-gateway/src/sse.rs | load_recent_thread_messages 5219 (ListConversation call 5263); read_latest_assistant 7493 (call 7520); chunk emitters at 402, 3111, 3170, 5706; test modules at 6998 and 10332 | as listed | M (+883: ZDR refusal streams) | in-file mod tests (10332) |
| MP/rust/services/model-gateway/src/tool_loop.rs | reattach_context tool reads ListConversation | 3226-3261 | M (+425) | in-file tests (not enumerated) |
| MP/rust/services/model-gateway/src/sse_events.rs | SSE event type definitions | n/a | M (+203) | sse-taxonomy CI gate (section 4) |
| MP/rust/services/session-core/src/grpc.rs | list_conversation 5216-5328 / 5387; SQL FROM messages m 5285-5291 / ORDER BY m.sequence at 5462; no LIMIT; record_metrics 5328 | as listed | M (+861) | 9338 set_run_mode_and_upsert_skill_against_real_pg (calls 9438, 9459); 9500 replay_thread_includes_thread_owned_events_against_real_pg; 10183 a_space_scoped_system_thread_is_readable_only_through_a_verified_space_decision (call 10276) |
| MP/rust/services/session-core/migrations/0001_init.sql | messages table; idx_messages_thread_id ON messages (thread_id, sequence) | n/a | clean | n/a |
| MP/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs | mock SessionCore list_conversation | 1326 | M (+288) | itself |
| MP/rust/services/execution-core/src/runtime_loop/agent.rs | test double list_conversation | 6135 | M (+207) | itself |
| MP/rust/crates/mp-contracts/src/lib.rs | shared crate that exports model_plane::v1 types used by model-gateway and session-core | n/a | M (+2); dataplane_posture.rs M | n/a |
| MP/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md | planned include_superseded on ListConversationRequest (doc only) | 127 | clean | n/a |

## 4. Related contracts

- Proto (MP/proto/model_plane/v1/sessions.proto): ListConversationRequest has org_id=1, thread_id=2, space_id=3, space_read_decision_ref=4, space_read_decision_token=5, and no limit or cursor. ListConversationResponse holds repeated SessionMessage messages=1. ReplayThreadRequest has thread_id=1, after_event_id=2, limit=3 (0 means unlimited).
- Planned, not in tree: `include_superseded` on ListConversationRequest (MP/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md:127; chat-gap-closure JSON 1409).
- The SPA (FP/src) does not reference ListConversationRequest; it consumes BFF JSON. Generated TS for this proto exists only in a different app, `apps/Frontend Plane/verevon/src/lib/model-plane/gen/` (sessions_pb.ts:1737).
- SQL migrations: WT latest is `0043_audit_outbox_requeue.sql`. LV adds `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql` and `revert/` (all ??). The messages index `idx_messages_thread_id (thread_id, sequence)` is in `0001_init.sql`; a tail-first `ORDER BY sequence DESC LIMIT n` would be index-backed (inference, not run through EXPLAIN).
- Config and env: E2E_BASE_URL (default http://localhost:5199), LOCAL_E2E_BASE_URL (default http://localhost:5173), Q08_PERF_SAMPLES (1-40), VITE_SCENE_MODE (LV index.tsx and App.tsx).
- SSE event types emitted by MG sse.rs (count of `.event("...")` sites): chunk 9, done 9, connected 3, step_update 2, error 2, awaiting_approval 1. The chunk frame carries `delta` or `content` (product-latency.spec.ts, line 22). Gate: `.github/workflows/sse-taxonomy.yml` runs `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (line 45).
- Gateway routes: BFF GET `/api/v1/chat/threads/{thread_id}/transcript` (domains/chat.rs:28), `/messages` (46), `/events` (50); MG GET `/v1/threads/:thread_id/messages` (http_routes.rs:177) and `/v1/threads/:thread_id/events` (181). SPA client functions: section 3a.
- Action contracts (FP/src/shared/actions): none for transcript or thread-message reads; action-registry.ts only has integrations inbox-history entries. A paged read would change the API client (chat-client.ts). Whether it also needs an action contract is NOT DETERMINED; FP CLAUDE.md asks for one before wiring a meaningful UI operation.

## 5. Test and build commands

Frontend (cwd FP; `.github/workflows/verevonv3-ci.yml` SPA job, lines 144-193):
- `pnpm verify` = `pnpm lint && pnpm typecheck && pnpm test && pnpm build` (package.json:18). CI runs `pnpm install --frozen-lockfile`, `pnpm --filter @verevon/remote-core run build`, then `pnpm verify` (pnpm 11.3.0, Node 22).
- Targeted unit: `pnpm exec vitest run src/shared/chat-nodes/derive.budget.test.ts`; `pnpm exec vitest run src/features/chat/components/chat-markdown.test.ts src/features/chat/components/ChatMessages.codeblock.test.tsx`; `pnpm exec vitest run tests/product-phase-timing.test.ts` (vitest include pattern not read).
- `pnpm test` = node --test gate scripts, then `vitest run` (package.json:13). `pnpm typecheck` = `tsc -b` (:17). `pnpm lint` = `eslint .` (:11). `pnpm build` (:9).
- Playwright (`pnpm test:e2e` = `playwright test`, :15; browsers via `pnpm test:e2e:install`, :16): latency candidate `pnpm exec playwright test --project=product-readiness tests/e2e/product-latency.spec.ts` with `Q08_PERF_SAMPLES=<n>`; the project depends on local-setup. Chat e2e: `pnpm exec playwright test --project=chat`. The SPA CI job has no Playwright step.

Gateway BFF (crate `verevon-gateway-rs`, cwd FP/apps/gateway):
- CI (verevonv3-ci.yml 109-141): `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test`.
- Local (root CLAUDE.md, Build And Run): `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets`; name filter, e.g. `canonical_messages`.

Model Plane (cwd MP/rust; crates model-gateway, session-core, mp-contracts):
- CI (`.github/workflows/model-plane-rust-ci.yml`): `cargo fmt --all -- --check` (line 99); `cargo test --workspace --no-fail-fast` (line 119).
- Crate-scoped (MP CLAUDE.md, Testing): `cargo test -q -p model-gateway --all-targets`; `cargo test -q -p session-core --all-targets`. User memory (model-plane-build-and-deploy): on Windows two cargo targets fail pre-existing; use `--lib`.
- Tests with the `_against_real_pg` suffix (e.g. session-core grpc.rs 9338, 9500) need Postgres by name; not run here.
- Proto regeneration: `buf generate` (MP CLAUDE.md, Build and Run); `buf lint` has pre-existing debt.
- SSE gate: `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (sse-taxonomy.yml:45), needed only when SSE event names change.

## 6. Owners

- Root CLAUDE.md:29: "Frontend Plane owns Verevon v3 UI plus same-origin gateway/BFF normalization." Owns the renderer, the browser benchmark, the reload guard (FP/src, FP/tests) and the BFF history.rs and domains/chat.
- Root CLAUDE.md:27: "Model Plane owns reasoning, sessions/runs, inference, execution loops..." Owns the MG read route and the sse.rs and tool_loop.rs callers.
- `apps/Model Plane/docs/capability-ownership-matrix.md:53`: "Thread / message timeline | session-core | Postgres | gateway ingress | outbox". A paged read is implemented in session-core and exposed through MG and the BFF.
- `apps/master-ownership-matrix.md:126`: session-core (Rust) owns threads, runs, checkpoints, memory index, compaction and context assembly.
- Root CLAUDE.md:32: "No direct database crossing between planes." The BFF must not query session-core Postgres; paging passes through model-gateway.
- Root CLAUDE.md:35: "Zero Data Retention must propagate through any content-persisting boundary." Any new transcript cache or benchmark artifact must respect ZDR.
- MP CLAUDE.md, Code Style: Rust owns the request hot path, so paging code stays in Rust.
- Card assignment (03:650): "Frontend Plane (renderer, benchmark, chunk self-heal) with Model Plane (paged reads)."

## 7. Uncommitted-only (LV vs HEAD)

M (differs in LV), relevant files, with one-line summaries from `git diff --stat` and hunk headers:
- FP/src/index.tsx (+12/-1): boot() wrapper; VITE_SCENE_MODE dynamic import before render. A reload guard would sit here.
- FP/src/app/App.tsx (+11): ChatUtilityPage lazy route and findings, reports and routines routes; scene routes behind VITE_SCENE_MODE.
- FP/src/features/chat/components/ChatMessages.tsx (229 lines): suggestion and starter-prompt rows. The added-line scan found no change to ChatMarkdown, parseMarkdownBlocks, TranscriptCodeBlock or delta lines.
- FP/src/features/chat/components/ChatPage.tsx (+13): work-period composer state and props, orgId and userId props; shifts ChatPage lines by +11 above the turn For.
- FP/src/features/chat/components/use-chat-controller.ts (+4 at 3283-3295): ZDR feature field on errors; delta paths unchanged.
- FP/src/shared/api/chat-client.ts (+53): retrieval_notice event and ZDR feature; shifts the transcript, messages and events functions by +53.
- FP/src/shared/api/chat-client.test.ts (+22); ChatMessages.failure-actions.test.tsx (+34); chat-normalizers.ts (+39); chat-types.ts (7 lines); shared/api/http.ts (24 lines); shared/api/sse.ts (+4/-2): ZDR refusal plumbing.
- FP/package.json (+2): demo:build and demo:probe scripts.
- .github/workflows/verevonv3-ci.yml (+55): new demo-product-build job; SPA job unchanged. .github/workflows/model-plane-rust-ci.yml (+50): not read in detail.
- MP/proto/model_plane/v1/sessions.proto (+42): SessionRetentionService.CountWaivedRuns (Z-21a). ListConversationRequest unchanged.
- MP/rust/services/model-gateway/src/http_routes.rs (237 lines): ZDR refusal and waiver checks; list_thread_messages body unchanged (LV 8479-8536).
- MP/rust/services/session-core/src/grpc.rs (861 lines): retention and ZDR; list_conversation SQL unchanged (no LIMIT in LV 5387-5507).
- MP/rust/services/model-gateway/src/sse.rs (883), tool_loop.rs (425), sse_events.rs (203): ZDR refusal streams; the added-line scan found no change to ListConversation callers.
- MP/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs (288) and MP/rust/services/execution-core/src/runtime_loop/agent.rs (207): test doubles and ZDR paths; not read in detail.
- MP/rust/crates/mp-contracts/src/lib.rs (+2); dataplane_posture.rs (M, not sized).

?? (LV-only, untracked, not in HEAD):
- FP/src/demo/ (product-scene runtime; imported by LV index.tsx); FP/vite.demo.config.ts; FP/apps/verevon-web/public/product-demo/ (built demo; contains Vite preload dispatch text).
- FP/src/shared/api/contracts/zdr-refusals.json (+ .sha256).
- FP/src/features/chat/components/ChatUtilityPage.tsx, ChatWorkPeriodPicker.tsx, EmptyChatState.test.tsx, chat-normalizers.zdr-refusal.test.ts, use-chat-controller.zdr-refusal.test.ts; FP/src/features/chat/lib/prompt-suggestions.ts (+ test); FP/src/features/core/components/sidebar/ChatWorkLinks.tsx; FP/src/shared/actions/chat-work-period.ts (+ test); FP/src/shared/api/chat-client.zdr-refusal.test.ts, http.zdr-refusal.test.ts, zdr-refusals.ts (+ test).
- MP/rust/services/session-core/migrations/0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql, migrations/revert/; session-core/src/retention_receipt.rs; mp-contracts/src/retention.rs.

D (LV deleted, outside F-t3-021 scope): FP/apps/verevon-web/src/app/dev/product-demo/*, FP/apps/verevon-web/src/components/home/product-demo/*.

## 8. NOT DETERMINED

- Reference-side claims (T3 Code LegendList and 40 px follow; DeepSeek UNSTABLE_TAIL_BLOCKS = 2 and grammar resume; Claude desktop getTranscriptTail and getTranscriptPage) were not re-verified; the evidence pack was not opened. REA was not used (its MCP server failed to connect this session), and Appendix C says REA was not re-run.
- Whether LV edits to ChatMessages.tsx, use-chat-controller.ts or chat-client.ts change removed lines on the render or delta paths (only an added-line scan was done).
- Whether a server-side cap on ListConversation exists below the gRPC layer. None is in the session-core SQL; interceptors and transport were not read.
- Whether any browser code renders from the local transcript snapshot before the network returns (only the storage and write paths were traced).
- Whether I5 batching interacts with the store flush() calls (use-chat-controller.ts, e.g. 1423, 1463, 2495, 3529). Those appear to be store-write flushes, not render frames; not analysed.
- Solid 2.0 rc keyed `<For>` behaviour (I5 caveat, JSON 1749): mapArray in node_modules was not read.
- Whether vitest picks up tests/product-phase-timing.test.ts (vitest config not read), and whether verevonv3-security.yml or any other CI job runs Playwright (not read).
- Whether any existing test covers the /v1/threads/:id/messages or /events routes or the BFF json_handlers (no name match found; not exhaustively searched).
- Whether mp-contracts generates Rust types from proto at build time (build.rs not read).
- Exact LV line of the assistant node memo (WT 528 and 532) and of the keyed-off comment (WT 653-657).
- Content of the remaining M items beyond their stat (agent.rs, e2e_invoke_chain_test.rs, dataplane_posture.rs, model-plane-rust-ci.yml).
- No measured browser numbers exist for transcript rendering; plan section 21 (777-857) is unit-level only.
