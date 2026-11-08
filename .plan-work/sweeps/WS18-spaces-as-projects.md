# WS18 spaces-as-projects: code-map sweep (F-t3-013)

Sweep date 2026-10-15. Read-only: no product code, config or docs were edited and no git write commands were run. This file is the only output.

## Roots and conventions

- HEAD = worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` at afb83928. Clean apart from untracked `.plan-work/`.
- Live = `C:/dev/CoresSystem`: HEAD plus the 324 entries in `.plan-work/live-tree-status.txt`. "clean" below means the path is not in that file.
- Prefixes: `FP` = `apps/Frontend Plane/verevonv3`; `FPG` = `FP/apps/gateway`; `MP` = `apps/Model Plane`; `GW` = `MP/rust/services/model-gateway`; `SC` = `MP/rust/services/session-core`; `EC` = `MP/rust/services/execution-core`; `CP` = `apps/Control Plane`; `UC` = `CP/user-core`; `AP` = `apps/Application Plane`; `DP` = `apps/Data Plane v2`.
- Audit: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`. Card F-t3-013 is at lines 532-547.

## 0. Findings in brief

1. **Core gap confirmed at HEAD and live.** Do (agentic) runs never read authored instructions. The gateway stamps `space_instructions` on chat bodies (`FPG/src/domains/spaces.rs:4291-4324`). `InvokeRequest` parses it (`GW/src/http_routes.rs:6753` HEAD, 6859 live). Only the inline composer reads it (`GW/src/sse.rs:1747` HEAD, 1855 live). The agentic branch (`sse.rs:1381-1447` HEAD) passes `space_id` and no instructions. `RunAgentRequest` has no instruction field (`MP/proto/model_plane/v1/execution.proto:76-211`).
2. **Card line numbers are live-tree numbers.** Against HEAD, 4 code citations drift (sse.rs x3, chat-client.ts x1). 1 is absent at HEAD (`retrieval_notice`). 1 is misattributed (spaces.rs range). Full table in section 2. Against live, 10 match, 1 is partial, 1 is misattributed.
3. **`retrieval_notice` is uncommitted.** It does not exist at HEAD. It is live-only work: `GW/src/sse_events.rs` live 368, `FP/src/shared/api/chat-client.ts` live 250. Only `source_scope_applied` is committed.
4. **`resolve_space_role` is not at spaces.rs:4281-4330.** That range is `inject_authored_instructions` (fn 4291-4324), which calls `resolve_space_role` at 4318. The definition is at `spaces.rs:3728-3763`.
5. **Plane mismatch.** The card's plane column says Application Plane owns membership. `apps/master-ownership-matrix.md:165-166` (rules 9-10) make Control the membership authority, and Application projections may only narrow access. The live membership read is Control: `CP/user-core/internal/http/server.go:355`.
6. **Plan B3 and scope.** "Plan B3" is `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:109`: "Pass persona, org and Space instructions (and identity) into agentic runs", size M, build group 3 at line 104. In the WS02 design it is S6 (`.plan-work/designs/WS02-run-context-cache.md:30`; decision D3 at :104; "single function compose_authored_instructions" at :425). B3 is wider than F-t3-013 because it also covers org and persona. The agentic function takes no org or persona parameter either (`sse.rs:9167-9192` HEAD).
7. **OD19 is open.** `CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:507` and `.plan-work/common/rules-ledger-adrs.md:259` ("Open"). No ruling found in the repo.
8. **Inferred, not traced end to end.** The invoke, stream and AG-UI handlers that call `inject_authored_instructions` (`FPG/src/domains/chat/json_handlers.rs:99`, `FPG/src/domains/chat/streams.rs:70`, `FPG/src/domains/ag_ui.rs:101`) are the entry points that carry the `agentic` feature (`sse.rs:1221` HEAD). If so, Do bodies already carry `space_instructions`, and the fix is a proto field plus a call in `build_run_agent_request`, not a new fetch.
9. **Cross-card drift in the same audit.** F-t3-001 (audit line 346) cites the agentic return at `sse.rs:1405-1500` and the composer at `6360-6420`. Live numbers are 1449 (`if agentic_requested`) and 6375 (fn). F-t3-013's numbers match live.

## 1. Card resolution (step 1)

- Card header (audit 534-536): Tier Priority; Verevon today Partial; Effort L; Plane "Application Plane (membership, projections) with Model Plane (instruction and scope layering) and Data Plane v2 (knowledge)"; Wave W2; Confidence Medium.
- Card "What to build" (538): resolve OD19 (server-synced folders with a project-instruction layer, plan I21, versus private-room Space projects); add a project picker to the chat sidebar; layer Space-authored instructions into Do runs; add a knowledge-mode badge (in context versus retrieval).
- Card "Verevon today" (544): sidebar folder list `chat-organization.ts:2-34`; no Space entry point `use-chat-controller.ts:2274-2286`; Space page already has instructions, knowledge, members and room timeline; authored instructions only on the inline path; `RunAgentRequest` lacks the field; `retrieval_notice` is not a knowledge mode.
- Card "How to build it" (546): one proto field, composed in the agentic branch, never logged (ZDR); badge via `source_scope_applied` (not `retrieval_notice`); any Space picker via `resolve_space_role` and the Control-signed Space decision; Space access never widens an owner ACL; depends on F-t3-001, F-t3-012, OD19.
- Section 9.1: row at audit line 1563, inside Wave W2 (header 1551). Dependencies listed: F-t3-001, F-t3-012, "ADR Spaces as project (OD19)".
- Appendix B row: 1831. Appendix C entry: 2024-2031 (reference check partial, high confidence; prevalence corrected to 6 of 12 sources and 4 vendors). Appendix D: no F-t3-013 line.
- OD19 question: section 10.2, line 1724 (Spaces as chat project container, instruction precedence org/Space/project/persona, branch and rewind, share model, slash-template authors).
- Other audit mentions (text only): 58, 82, 99, 131, 291, 1404, 1487, 1499, 1600, 1698.

## 2. Citation verification (step 3)

Verdicts: MATCH; PARTIAL; DRIFT (new range given); ABSENT; MISATTRIBUTED. "HEAD" = worktree; "live" = `C:/dev/CoresSystem`.

| Card | Cited path:lines | Claim | HEAD verdict | Live status | Corrected path:lines (HEAD / live) |
|---|---|---|---|---|---|
| F-t3-013 | `FP/src/features/chat/lib/chat-organization.ts:2-34` | localStorage project list, 100 cap | MATCH (cap at :34; storage via `FP/src/shared/session/client-storage.ts:7` `window.localStorage`) | clean | 2-34 / same |
| F-t3-013 | `FP/src/features/chat/components/use-chat-controller.ts:2274-2286` | no Space entry point on /chat | MATCH | M (+4 lines, all below 2274; window unchanged) | 2274-2288 / 2274-2288 |
| F-t3-013 | `FP/src/features/spaces/components/SpacePage.tsx:434` | knowledge panel on Space page | MATCH (`kunnskap:` tab mounts `SpaceKnowledgePanel`) | clean | 434 / 434 |
| F-t3-013 | `FP/src/features/spaces/components/SpacePage.tsx:626-720` | Space instructions section | MATCH (`SpaceInstructionsSection` fn 632; UI 670-729) | clean | 626-729 / same |
| F-t3-013 | `FP/src/features/spaces/components/SpaceRoomTimeline.tsx:60-100` | room timeline author attribution, reply, @agent mention | PARTIAL: attribution MATCH (turn normalisation 55-76; `resolveAuthorName` 87-103). Reply and mention are not in this range | clean | attribution 55-103; reply props 118-186; mention and reply docs in `FP/src/features/spaces/components/SpaceRoomComposer.tsx:34-69` |
| F-t3-013 | `GW/src/sse.rs:1855` | authored composer on inline path | DRIFT: call at 1747 | M: MATCH 1855 | 1747 / 1855 |
| F-t3-013 | `GW/src/sse.rs:1443-1520` | agentic return follows inline composer | DRIFT: `if agentic_requested` at 1381; `return agentic_run_stream(` at 1423; args to 1447 | M: MATCH 1443-1515 (`return` at 1491) | 1381-1447 / 1443-1515 |
| F-t3-013 | `GW/src/sse.rs:6375-6420` | `authored_instructions_message` composer | DRIFT: fn at 6243; layers 6250 (platform), 6253 (org), 6259 (Space), 6274 (persona); end not re-read, about 6295 | M: MATCH 6375-6427 | 6243-~6295 / 6375-6427 |
| F-t3-013 | `MP/proto/model_plane/v1/execution.proto:76-170` | `RunAgentRequest` has no authored-instructions field | MATCH (message 76-211; `string space_id = 14` at 168; no "instruction" text at all in `execution.proto`; the only `instructions` fields in `MP/proto/model_plane/v1/` are `ExtractStructuredRequest` (`gateway.proto:457`) and `CreateRealtimeSessionRequest` (`inference.proto:1173`), neither Space-related) | clean | 76-211 / same |
| F-t3-013 | `FP/src/shared/api/chat-client.ts:246-250` | `retrieval_notice` = reduced answer | ABSENT at HEAD (no `retrieval_notice` string under `apps/`) | M: MATCH (doc 247; type `ChatRetrievalNoticeEvent` 250; parse case 1740). Live-only | live-only: 246-250 |
| F-t3-013 | `FP/src/shared/api/chat-client.ts:556-566` | `source_scope_applied` frame type | DRIFT: doc 546-552; type `ChatSourceScopeAppliedEvent` 553-556; parse cases 1324, 1554 | M: MATCH 558-568 (+12 shift) | 546-556 / 558-568 |
| F-t3-013 | `FPG/src/domains/spaces.rs:4281-4330` ("resolve_space_role") | role gate reused for Space layer | MISATTRIBUTED: range is `inject_authored_instructions` (doc 4268-4290; fn 4291-4324; calls `resolve_space_role` at 4318) | clean | resolve_space_role 3728-3763; inject 4291-4324 |
| F-t3-013 | (no line in card) `inject_authored_instructions` | stamps org and Space layers on chat turns | MATCH | clean | 4291-4324; callers `FPG/src/domains/chat/json_handlers.rs:99`, `streams.rs:70`, `FPG/src/domains/ag_ui.rs:101` |
| F-t3-013 | (no line in card) `RunAgentRequest.space_id` | Space id on Do run | MATCH | clean | 168 (field 14; comment 156-167 says same meaning as `ExecuteStepRequest.space_id`, field 11) |

Appendix D has no F-t3-013 line, so no tier or effort change was recorded for this item.

## 3. Verified flow (supporting, HEAD unless marked live)

1. **Client.** Plan mode sets the `agentic` feature family (`FP/src/shared/api/chat-client.ts:135-137`; `FP/src/features/chat/components/use-chat-controller.ts:1043-1048`). There is no Space picker. The Space scope is a `space_ref` in the body only.
2. **BFF stamp.** `spaces.rs` strips client-supplied `org_instructions` and `space_instructions` (4306-4307). The org layer comes from Convex `organizations:instructionsForGateway` (4231-4236). The Space layer is fetched only after `resolve_space_role` (4317-4320), from Convex `spaces:instructionsForGateway` (4249-4262).
3. **Control membership.** `resolve_space_role` (3728-3763) calls `GET {user_core}/api/v1/internal/spaces/{space_ref}/membership` (`CP/user-core/internal/http/server.go:331` internal group, `:343` spaces group, `:355` route; handler `CP/user-core/internal/http/spaces.go:448`).
4. **Model-gateway input.** `InvokeRequest` carries `org_instructions`, `space_instructions`, `persona_instructions` (`http_routes.rs:6747-6762`; live `space_instructions` at 6859).
5. **Inline composer.** `prompt.authored_instructions = authored_instructions_message(...)` (`sse.rs:1747`; live 1855). Lands in `prompt_context.rs:204, 279, 331`.
6. **Agentic (Do).** `agentic_requested` = features contains "agentic" (`sse.rs:1221`). Branch 1381-1447 calls `agentic_run_stream` (fn 9167-9192; params include `space_id` at 9188 and no instruction parameter), which calls `build_run_agent_request` (fn 8758-8772; params include `space_id`, `approved_plan_context`, `route`; no instruction parameter). That builds `RunAgentRequest` (`execution.proto:76`), which reaches execution-core `run_agent` (`EC/src/grpc.rs:1203`) and `run_agent_with_tools` (`EC/src/runtime_loop/agent.rs:743`).
7. **Gap.** Nothing between step 4 and step 6 reads `space_instructions` on the agentic branch, and execution-core receives no instruction field. Confirmed at HEAD by reading the signatures. Live: the only `space_instructions` hits in `GW/src` are http_routes 6859, normalize 482, sse 6391 and tests, zdr_features 139 (live-only).
8. **Receipt (committed).** `ChatEvent::SourceScopeApplied` is emitted when `retrieval_scope != Auto` (`sse.rs:2200` HEAD; 2325 live). Defined at `sse_events.rs:199` (HEAD). Frontend: type `chat-client.ts:553-556`; consumers `ChatMessages.tsx:254` (`SourceScopeNotice`, def 3299), `ChatPanels.tsx:616`, `derive.ts:149`, `use-chat-controller.ts:3094-3097`.

## 4. Affected-file map (step 4)

Depth 2 from the cited symbols. "Live" = live status. Tests are names or counts from a cheap grep or read.

### Frontend (FP)

| Path | Role | Approx lines | Live | Tests |
|---|---|---|---|---|
| `FP/src/features/chat/lib/chat-organization.ts` | sidebar project store; localStorage key 8-10; cap 34 | 1-40 | clean | `chat-organization.test.ts`: 6 `it`s (scopes to account and workspace; malformed data; rename and pin; move and unassign; search; sort) |
| `FP/src/shared/actions/chat-organization.ts` | action schema for organisation (imported at chat-organization.ts:1) | not read | clean | none read |
| `FP/src/features/core/components/sidebar/ChatSidebarOrganizer.tsx` | sidebar organiser; project-picker target | whole | M (+2: renders `ChatWorkLinks`) | `ChatSidebarOrganizer.test.tsx`: 5 `it`s (43, 61, 89, 120, 149) |
| `FP/src/features/core/components/sidebar/chat-sidebar-organizer.css` | organiser styles | n/a | M (+5 `.chat-work-links`) | none |
| `FP/src/features/core/components/sidebar/ChatWorkLinks.tsx` | work links above organiser | new | ?? live-only | none |
| `FP/src/features/core/components/sidebar/CoreSidebarKnowledgePanel.tsx` | existing sidebar knowledge panel; `KnowledgeSidebarModeId` = folders/sources/tags (45); labels 402-426, 488-492 | 45-492 | clean | not read |
| `FP/src/features/chat/components/use-chat-controller.ts` | send path; Space-ref comment; plan-to-agentic 1043-1048; source-scope receipt 3094-3097 | 1043-3097 | M (+4, ZDR refusal feature; not Space) | `use-chat-controller.retrieval-scope.test.ts`: 4 `it`s (102, 125, 134, 151 "lands the server's source_scope_applied receipt...") |
| `FP/src/shared/api/chat-client.ts` | SSE parse; request scope fields 62, 69, 692, 695; `source_scope_applied` type 553-556; cases 1324, 1554; 2239 maps `source_scope` | 62-2239 | M (+53: adds `retrieval_notice` type 250 and case 1740) | `chat-client.test.ts` (M); `chat-client.replay.test.ts:92` (`source_scope` fixture) |
| `FP/src/shared/actions/chat-source-scope.ts` | `ChatSourceScope` = workspace/conversation (5); `RetrievalScope` = auto/knowledge_only/web_only (18) | 5, 18 | clean | none read |
| `FP/src/features/chat/components/chat-types.ts` | turn types; `scopeApplied` doc | 435 | M (+7: `open_integrations` action; not Space) | none |
| `FP/src/features/chat/components/ChatPanels.tsx` | context inspector; `scopeApplied` type | 557-634 (type 616) | clean | none read |
| `FP/src/features/chat/components/ChatMessages.tsx` | `SourceScopeNotice` badge | 254, 3299 (live 256, 3297) | M (+229; not Space-specific; not read in full) | `ChatMessages.failure-actions.test.tsx` (M) |
| `FP/src/shared/chat-nodes/derive.ts` | `scopeApplied` derivation | 149 | clean | none read |
| `FP/src/features/spaces/components/SpacePage.tsx` | Space page: knowledge tab 434; instruction section 626-729; role floors 59-73 (mirrors gateway); members 1068 | 49-1068 | clean | `SpacePage.test.tsx`: 6 `it`s (208, 230, 257, 313, 342, 428) |
| `FP/src/features/spaces/components/SpaceKnowledgePanel.tsx` | Space knowledge panel; `getSpaceKnowledge` (84) | 4, 84 | clean | `SpaceKnowledgePanel.test.tsx` (names not read) |
| `FP/src/features/spaces/components/SpaceRoomTimeline.tsx` | timeline; attribution 55-103; reply 118-186 | 55-186 | clean | `SpaceRoomTimeline.test.tsx`: 6 `it`s (49-93) |
| `FP/src/features/spaces/components/SpaceRoomComposer.tsx` | composer; mention and reply docs | 34-69 | M (+2/-1: `translateApiError` gets `feature`) | `SpaceRoomComposer.test.tsx` (M, about 20 changed lines) |
| `FP/src/shared/api/spaces-client.ts` | client: `listSpaces` 124, `getSpaceInstructions` 135, `updateSpaceInstructions` 142, `getSpaceContext` 153, `getSpaceKnowledge` 558 | as listed | clean | not read |
| `FP/src/shared/actions/action-registry.ts` | action ids `spaces.*` (1991-2134); `spaces.update_space_instructions` 2013 | 1991-2134 | clean | not read |

### Frontend BFF (FPG, crate `verevon-gateway-rs`)

| Path | Role | Approx lines | Live | Tests |
|---|---|---|---|---|
| `FPG/src/domains/spaces.rs` | Space routes 665 (context), 685-686 (instructions GET/PATCH), 710 (threads), 712 (knowledge); thread-read decision 1268-1299; retrieval decision 1664-1677; `SPACE_INSTRUCTIONS_WRITE_ROLES` 3720; `resolve_space_role` 3728; GET/PATCH instructions 3768, 3811; `peek_space_ref` 4218; `fetch_org_instructions` 4231; `fetch_space_instructions` 4249; `inject_authored_instructions` 4291-4324 | as listed | clean | in-file: `org_instructions_are_injected_on_a_personal_turn_with_no_space` (9549); `space_instructions_are_injected_alongside_org_instructions_when_scoped` (9569); `a_non_member_never_receives_that_spaces_instructions` (9588); `client_supplied_persona_instructions_are_discarded_and_replaced` (9698); `a_viewer_can_read_space_instructions` (9795); `a_viewer_cannot_write_space_instructions` (9809); `an_editor_can_write_space_instructions` (9822); `an_owner_can_write_space_instructions` (9836); `overlong_space_instructions_are_rejected_before_reaching_convex` (9849); `update_space_instructions_request_deserializes_a_missing_instructions_field` (9878) |
| `FPG/src/domains/chat.rs` | chat routes: `/api/v1/chat/stream` (17), `/api/v1/chat/invoke` (18), threads and invocations (20-60) | 17-60 | clean | none read |
| `FPG/src/domains/chat/json_handlers.rs` | invoke path; `inject_authored_instructions` call | 99 | clean | none read |
| `FPG/src/domains/chat/streams.rs` | stream path; `inject_authored_instructions` call | 70 | clean | none read |
| `FPG/src/domains/ag_ui.rs` | AG-UI path; `inject_authored_instructions` call | 101 | clean | none read |
| `FPG/src/domains/actions/handlers.rs`, `dispatchers.rs` | `spaces.update_space_instructions` dispatch | handlers 482; dispatchers 5458-5472 | clean | none read |

### Model Plane (GW, SC, EC)

| Path | Role | Approx lines (HEAD / live) | Live | Tests |
|---|---|---|---|---|
| `GW/src/sse.rs` | inline composer call; agentic branch; `SourceScopeApplied` emit; `authored_instructions_message`; `build_run_agent_request`; `agentic_run_stream`; RunAgentRequest compat test | agentic flag 1221 / 1449; branch 1381-1447 / 1443-1515; inline call 1747 / 1855; emit 2200 / 2325; composer 6243 / 6375; zdr_turn_settings -- / 8380-8391; build 8758 / ~9118 (WS02 anchor, not re-grepped); agentic_run_stream 9167 / 9527; compat test 644-720 / n.r. | M (+883) | HEAD tests: `no_authored_instructions_message_when_every_layer_is_empty` (10756); `org_layer_is_wrapped_in_non_override_framing` (10772); `all_four_layers_compose_in_platform_org_space_agent_order` (10784); `blank_org_and_space_instructions_are_treated_as_absent` (10803); `persona_layer_sits_between_space_and_agent_persona` (10815); `persona_layer_absent_when_thread_persona_disabled` (10842); `persona_layer_absent_when_persona_instructions_blank` (10853); `agent_persona_still_overrides_personal_persona_on_a_mentioned_turn` (10862). Live adds `retrieval_notice` tests 13061-13140 |
| `GW/src/http_routes.rs` | `InvokeRequest` (6563); instruction fields 6747-6762 | 6563-6762 / space_instructions 6859 | M (no instruction-related added lines) | `e2e_invoke_chain_test.rs` (M) |
| `GW/src/normalize.rs` | `RetrievalScope` enum (58); normalized defaults for instructions 474-476 / 482 | 30-31, 58, 474-476 / 482 | M (no instruction adds) | none read |
| `GW/src/state.rs` | `platform_instructions` field (210); env `PLATFORM_SYSTEM_INSTRUCTIONS` (625-626) | 210, 625-626 | M (no instruction adds) | none read |
| `GW/src/prompt_context.rs` | `PromptContext.authored_instructions` (204); ordering (279, 331); render (421) | 204-421 | clean | in-file tests (453 region) |
| `GW/src/sse_events.rs` | `ChatEvent::SourceScopeApplied` (199 / 238); name map (406 / 468); payload (491 / 556); citations family (359 / 418); `RetrievalNotice` live-only (368 / 480 / 672) | as listed | M (+203) | in-file tests incl. `retrieval_notice()` builders (live 1997-2020) |
| `GW/src/zdr_features.rs` | ZDR treatment table; `space_instructions` Honored (139; also 239, 277) | live 139-277 | ?? live-only | `GW/tests/zdr_feature_table.rs` (?? live-only; `space_instructions` row 233-234) |
| `GW/src/retrieval_metadata.rs` | `RetrievalMetadata` (HEAD 46); `RetrievalNotice` struct (live 54), `REDUCED` (62) | live 54-69 | M | none read |
| `GW/src/tool_loop.rs` | `RETRIEVAL_NOTICE_KEY` (live 347); `retrieval_notice_events` (live 354) | live 347-354 | M | live tests 9758-9795 |
| `SC/src/grpc.rs` | run creation with `space_id` (930, 1858, 1973, 4521 HEAD); StartRun not read in full | as listed | M (4 instruction-related adds, all ZDR persona asserts) | in-file tests |
| `SC/src/run_service_grpc.rs` | run service; `space_id` fields 203, 221 | 203, 221 | M (no instruction adds) | in-file 1079, 1122 |
| `SC/migrations/0038_thread_source_scope.sql` | `threads.source_scope` workspace or conversation | 1-3 | clean | none |
| `EC/src/grpc.rs` | `run_agent` (1203); `space_id` mapping (270, 910) | as listed | M (no instruction adds) | in-file 1872-1873 `subscription_run` |
| `EC/src/runtime_loop/agent.rs` | `run_agent_with_tools` (743) | 743 | M (no instruction adds) | none read |
| `EC/src/agent_route.rs` | `AgentRoute`; test builders 262, 272 | 262-273 | M (no instruction adds) | in-file |
| `MP/proto/model_plane/v1/execution.proto` | `rpc RunAgent` (34); `RunAgentRequest` (76-211); `space_id` (168); `RunAgentResponse` (212) | as listed | clean | compat test `sse.rs:707-720` |
| `MP/proto/model_plane/v1/sessions.proto` | M adds `CountWaivedRuns` only | n/a | M (+42) | none |

### Data Plane v2 (DP/services/retrieval-engine-rs, crate `retrieval-engine-rs`)

| Path | Role | Approx lines | Live | Tests |
|---|---|---|---|---|
| `DP/services/retrieval-engine-rs/src/space_scope.rs` | `verify_retrieval_space_decision` (59); v2 Ed25519 envelope; audience `data-plane-retrieval` (29); `MAX_SPACE_DOCUMENTS` 2000 (26); "a Space never widens a document ACL" (8-9) | as listed | clean | in-file tests (434 import; 585 call) |
| `.../src/api/mod.rs` | HTTP retrieve calls verifier | 632 | M (no Space-term adds checked; full diff not read) | none read |
| `.../src/grpc/retrieval_svc.rs` | gRPC retrieval calls verifier | 137 | M | none read |
| `.../tests/service_scope.rs` | Space service-scope test | n/a | ?? live-only | not read |

### Control Plane (CP/user-core, Go)

| Path | Role | Approx lines | Live | Tests |
|---|---|---|---|---|
| `UC/internal/http/server.go` | `/api/v1` (160) and `/internal` (331) groups; spaces group 343-377: membership GET (355), thread-append-decision (358), thread-read-decision (359), retrieval-decision (361) | 331-377 | clean | none read |
| `UC/internal/http/spaces.go` | membership 448; thread decision 523; thread-read decision 640; retrieval decision 729; capability decision 1106; decision material 1423; sign 1444 | as listed | clean | `space_decision_body_test.go`; `thread_read_decision_live_test.go`; `service_auth_test.go` |
| `UC/internal/spaces/repository.go`, `authority_test.go` | Space authority storage and tests | not read | clean | `authority_test.go`; `membership_owner_live_test.go` |
| `UC/migrations/017_space_authority.up.sql` | Space authority schema | n/a | clean | none |

### Application Plane

| Path | Role | Approx lines | Live | Tests |
|---|---|---|---|---|
| `AP/convex-core/convex/spaces.ts` | `instructionsForGateway` query (1503-1513; args spaceRef, externalOrgId, serviceKey); `setInstructionsForGateway` mutation (1526; ADR-0003 authoring write) | 1503-1526 | clean | none found (grep of all of AP for `instructionsForGateway`) |
| `AP/convex-core/convex/organizations.ts` | org `instructionsForGateway` | 196 | clean | none found |
| `AP/conversation-core/conversation-core-go/internal/http/server.go`, `space_activity.go` | `GET /spaces/:space_ref/activity` (server.go:81) | 81 | clean | not read |

### Docs and decision records

| Path | Role | Lines | Live |
|---|---|---|---|
| `apps/AUTHORED_INSTRUCTIONS_ADR_2026-08-19.md` | ADR-0003 (layers; composer returns None only when every layer is empty) | 7, 165 | clean |
| `MP/docs/decisions/ledger.md` | waived Do run needs `AGENTIC_SUBSCRIPTION_ENABLED` (default off); Do and agentic refused under ZDR with typed reason | 296-298, 309-310 | clean |
| `FP/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md` | B3 row 109; build group 104; OD19 507; owner needs 230, 257, 278 | 104, 109, 230, 507 | clean |
| `.plan-work/designs/WS02-run-context-cache.md` | S6 (30); anchors (75); D3 (104); single composer (425) | 30, 75, 104, 425 | untracked (worktree only) |
| `.plan-work/sweeps/WS02-run-context-cache.md` | B3 mapping (37); anchors (75, 95) | 37, 75, 95 | untracked (worktree only) |
| `.plan-work/common/rules-ledger-adrs.md` | OD19 row "Open" (259); ADR-0003 precedence (228, 273) | 228, 259, 273 | untracked (worktree only) |
| `apps/master-ownership-matrix.md` | plane rules | 11-15, 40, 71, 165-166 | clean |
| `MP/docs/capability-ownership-matrix.md` | one-owner rows | 38-39, 53-56, 60 | clean |

## 5. Related contracts (step 5)

- **Proto.** `MP/proto/model_plane/v1/execution.proto`: `rpc RunAgent` (34), `RunAgentRequest` (76-211), `space_id = 14` (168). No "instruction" text in `execution.proto`. The only `instructions` fields in `proto/model_plane/v1/` are `ExtractStructuredRequest` (`gateway.proto:457`) and `CreateRealtimeSessionRequest` (`inference.proto:1173`); neither carries Space or Do-run instructions. Live `sessions.proto` adds `CountWaivedRuns` only.
- **SQL.** session-core latest migration is `0043_audit_outbox_requeue.sql`. Previous: `0042_thread_persona_toggle.sql`, `0041_mail_task_thread_origin.sql`, `0040_direct_run_completion_key.sql`, `0039_memory_user_control.sql`, `0038_thread_source_scope.sql` (adds `threads.source_scope TEXT NOT NULL DEFAULT 'workspace' CHECK (IN ('workspace','conversation'))`). user-core latest is `029_mail_task_addresses.up.sql`; Space authority is `017_space_authority.up.sql`. Data Plane migrations not listed (not determined). Convex schema is not SQL (not read).
- **Config and env.** `PLATFORM_SYSTEM_INSTRUCTIONS` (`GW/src/state.rs:625-626`). `AGENTIC_SUBSCRIPTION_ENABLED` (ledger 297; default off). ZDR treatment for `space_instructions` (live-only `zdr_features.rs:139`).
- **SSE events.** Committed: `source_scope_applied` (`sse_events.rs:199` HEAD, name 406), citations family (359). Live-only: `retrieval_notice` (368 / 480). Frontend parse: `chat-client.ts:1324, 1554` (committed), `1740` (live).
- **Gateway routes (BFF).** `/api/v1/chat/stream`, `/api/v1/chat/invoke` (`FPG/src/domains/chat.rs:17-18`). `/api/v1/spaces/{space_ref}/instructions` GET and PATCH (`spaces.rs:685-686`). `/api/v1/spaces/{space_ref}/knowledge` (712). `/context` (665). `/threads` (710). Gateway calls to Control: `/api/v1/internal/spaces/retrieval-decision` (1677), `/thread-read-decision` (1299), `/thread-append-decision` (139 region), `/membership` (resolve_space_role 3741-3743).
- **Control internal routes.** `/api/v1/internal/spaces/{space_ref}/membership` (`UC/internal/http/server.go:355`), `/thread-read-decision` (359), `/retrieval-decision` (361).
- **Convex functions (called by gateway).** `organizations:instructionsForGateway` (spaces.rs:4235; `AP/convex-core/convex/organizations.ts:196`); `spaces:instructionsForGateway` (spaces.rs:4257; `spaces.ts:1503`); `spaces:setInstructionsForGateway` (`spaces.ts:1526`).
- **Action contracts.** `FP/src/shared/actions/action-registry.ts`: `spaces.update_space_instructions` (2013), `spaces.create_room` (2112), `spaces.add_member` (2123), `spaces.remove_member` (2134). `FP/src/shared/actions/chat-source-scope.ts` (5, 18). `FP/src/shared/actions/chat-organization.ts` (schema for the sidebar store).
- **Data Plane decision contract.** Control-issued v2 decision, Ed25519 envelope, audience `data-plane-retrieval` (`space_scope.rs:28-29`). The gateway passes `space_read_decision_ref` and `space_read_decision_token` on space reads (spaces.rs 1169, 1469, 2427, 2540, 2687).
- **ZDR.** Root CLAUDE.md: ZDR must propagate through any content-persisting boundary. `GW/src/zdr_features.rs:139` (live-only) treats `space_instructions` as Honored.

## 6. Test and build commands (step 6)

- **Frontend (FP).** `cd "apps/Frontend Plane/verevonv3"`. `pnpm test` (`package.json:13`: node --test on four gateway scripts, then `vitest run`). `pnpm typecheck` (line 17: `tsc -b`). `pnpm lint` (line 11: `eslint .`). `pnpm build` (line 9: typecheck, then vite build). Targeted vitest: `pnpm vitest run src/features/chat/lib/chat-organization.test.ts src/features/spaces/components/SpacePage.test.tsx src/features/chat/components/use-chat-controller.retrieval-scope.test.ts src/features/core/components/sidebar/ChatSidebarOrganizer.test.tsx`. CI workflow `.github/workflows/verevonv3-ci.yml` (M live).
- **Frontend BFF (Rust).** `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (root CLAUDE.md Build and Run). Filter with `space_instructions` or `instructions`.
- **Model Plane (Rust).** Crates from Cargo.toml: `model-gateway`, `session-core`, `execution-core`. Workspace members `MP/rust/Cargo.toml:14-17`. `cd "apps/Model Plane/rust" && cargo test -q -p model-gateway --all-targets` (same for `-p session-core`, `-p execution-core`). `cargo test --workspace` for everything (`MP/CLAUDE.md` Testing). Proto regen: `buf generate` (`MP/CLAUDE.md`). Windows caveat from user memory (model-plane build note): two cargo targets fail pre-existing on Windows; use `--lib`.
- **Data Plane (Rust).** Crate `retrieval-engine-rs`. `cd "apps/Data Plane v2" && make check-rs && make test-rs && make test-integration` (root CLAUDE.md).
- **Control Plane user-core (Go).** Module `github.com/I-Dacosta/AquatiqCMS/apps/user-service-go`. `cd "apps/Control Plane/user-core" && make test` (`Makefile:44-46`: `go test -v ./...`). Coverage: `make test-coverage` (48-50).
- **Convex.** No test script read. Not determined.

## 7. Owners (step 7)

- **Root CLAUDE.md, Architecture Rules.** Control Plane owns identity, users, orgs, sessions, audit and entitlements. Data Plane v2 owns documents, chunks, embeddings and retrieval. Model Plane owns sessions and runs, inference and execution loops. Application Plane owns collaborative and realtime workspace projections. Frontend Plane owns the Verevon v3 UI and same-origin BFF normalization. No direct database crossing between planes. ZDR must propagate through any content-persisting boundary.
- **Membership.** `apps/master-ownership-matrix.md:165` (rule 9): identity and membership are Control Plane-owned; active-organization headers, session hints and Application projections are not grants. `:166` (rule 10): Application projections only narrow access. The card's Application Plane attribution for membership conflicts with these rules.
- **Knowledge and retrieval.** `apps/master-ownership-matrix.md:40` (durable knowledge: Data Plane) and `:71` (retrieval and rerank: retrieval-engine-rs owner).
- **Thread, run and context.** `MP/docs/capability-ownership-matrix.md:53` (thread and message timeline: session-core), `:54` (run metadata and lifecycle: session-core), `:56` (checkpoints, context assembly and compaction: session-core, execution-core triggers), `:38-39` (durable state: one writer per resource). Today the Space and org composer sits in model-gateway (`sse.rs:6243` HEAD), not session-core. WS02 D3 keeps a single gateway composer (`.plan-work/designs/WS02-run-context-cache.md:104`). This is an ownership tension for the owner to rule on, not a settled decision.
- **Two tool loops.** `MP/CLAUDE.md` (Code Style): model-gateway dispatch and execution-core `execute_step_inner` stay separate. Check `MP/docs/decisions/ledger.md` before proposing a merge.
- **Space instruction storage.** Application Plane (Convex `spaces.ts:1503-1526`), written only through the gateway after a Space-role check (`spaces.ts:1517-1525` comment block).
- **BFF stamping.** Frontend Plane BFF (`spaces.rs:4291-4324`) currently stamps org and Space layers and strips client-supplied values (4306-4307).

## 8. Uncommitted-only (live tree) (step 8)

Status from `.plan-work/live-tree-status.txt`. Diff stats from `git --no-optional-locks diff` in `C:/dev/CoresSystem`.

| Path | Status | What changed (1 line) |
|---|---|---|
| `MP/rust/services/model-gateway/src/sse.rs` | M (+883 lines) | Adds `zdr_turn_settings` composing authored instructions for ZDR turns (live 8380-8391, call 8385); adds `retrieval_notice` emission path. `agentic_run_stream` gains no instruction input. |
| `MP/rust/services/model-gateway/src/sse_events.rs` | M (+203) | Adds `ChatEvent::RetrievalNotice` variant (368), name (480), payload (672). |
| `MP/rust/services/model-gateway/src/retrieval_metadata.rs` | M | Adds `RetrievalNotice` struct (54) and `REDUCED` (62) and `NOTICES` constants. |
| `MP/rust/services/model-gateway/src/tool_loop.rs` | M | Adds `RETRIEVAL_NOTICE_KEY` (347) and `retrieval_notice_events` (354). |
| `MP/rust/services/model-gateway/src/zdr_features.rs` | ?? live-only | ZDR treatment table; `space_instructions` Honored (139). |
| `MP/rust/services/model-gateway/tests/zdr_feature_table.rs` | ?? live-only | ZDR table test with `space_instructions` row (233-234). |
| `MP/rust/services/model-gateway/src/http_routes.rs` | M | No instruction-related added lines; `space_instructions` now at 6859. |
| `MP/rust/services/model-gateway/src/normalize.rs` | M | No instruction-related added lines. |
| `MP/rust/services/model-gateway/src/state.rs` | M | No instruction-related added lines. |
| `MP/proto/model_plane/v1/sessions.proto` | M (+42) | Adds `CountWaivedRuns` RPC and messages; no Space or instruction fields. |
| `MP/rust/services/session-core/src/grpc.rs` | M | 4 instruction-related added lines, all ZDR persona assertions (owner decision 17). |
| `MP/rust/services/session-core/src/run_service_grpc.rs` | M | No instruction-related added lines. |
| `MP/rust/services/execution-core/src/grpc.rs` | M | No instruction-related added lines (route-less run assertion). |
| `MP/rust/services/execution-core/src/runtime_loop/agent.rs` | M | Caller argument change; no instruction fields. |
| `MP/rust/services/execution-core/src/agent_route.rs` | M | No instruction-related added lines. |
| `DP/services/retrieval-engine-rs/src/api/mod.rs` | M | Not Space-specific in the instruction key-term grep; full diff not read. |
| `DP/services/retrieval-engine-rs/src/grpc/retrieval_svc.rs` | M | Not read. |
| `DP/services/retrieval-engine-rs/tests/service_scope.rs` | ?? live-only | Not read. |
| `FP/src/shared/api/chat-client.ts` | M (+53) | Adds `ChatRetrievalNoticeEvent` (250), `onRetrievalNotice`, case `retrieval_notice` (1740). |
| `FP/src/shared/api/chat-client.test.ts` | M | Not read. |
| `FP/src/features/chat/components/use-chat-controller.ts` | M (+4) | ZDR refusal feature on failure; nothing at 2274 or 3094. |
| `FP/src/features/chat/components/chat-types.ts` | M (+7) | `StreamFailureAction` adds `open_integrations`. |
| `FP/src/features/chat/components/ChatMessages.tsx` | M (+229) | Not Space-specific; `SourceScopeNotice` moves 254 to 256 and 3299 to 3297. |
| `FP/src/features/chat/components/ChatPage.tsx` | M (+13) | Not read. |
| `FP/src/features/core/components/sidebar/ChatSidebarOrganizer.tsx` | M (+2) | Renders `<ChatWorkLinks />` above the organiser. |
| `FP/src/features/core/components/sidebar/chat-sidebar-organizer.css` | M (+5) | Styles for `.chat-work-links`. |
| `FP/src/features/core/components/sidebar/ChatWorkLinks.tsx` | ?? live-only | New sidebar work-links component. |
| `FP/src/features/spaces/components/SpaceRoomComposer.tsx` | M (+2/-1) | `translateApiError` receives `event.feature`. |
| `FP/src/features/spaces/components/SpaceRoomComposer.test.tsx` | M | About 20 changed lines; not read. |
| `.plan-work/` (worktree only) | ?? untracked | Planning and sweep outputs, including the WS02 design used above. Not product code. |

Clean (no live change) and relevant: `SpacePage.tsx`, `SpaceRoomTimeline.tsx`, `SpaceKnowledgePanel.tsx`, `chat-organization.ts`, `spaces-client.ts`, `action-registry.ts`, `chat-source-scope.ts`, `ChatPanels.tsx`, `derive.ts`, `prompt_context.rs`, `spaces.rs`, `json_handlers.rs`, `streams.rs`, `ag_ui.rs`, `chat.rs`, `execution.proto`, `space_scope.rs`, all `CP/user-core` Space files, `AP/convex-core` Space files, `0038_thread_source_scope.sql`, the ADR, `ledger.md`, `CHAT_GAP_CLOSURE_PLAN`.

## 9. NOT DETERMINED (step 9)

- **OD19 ruling.** Open in the repo (`CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:507`; `.plan-work/common/rules-ledger-adrs.md:259`). No ruling found.
- **Owner of Space-as-project membership.** Card says Application Plane; master matrix rules 9-10 say Control. Which plane fronts the Space picker is not settled.
- **Do-run channel.** Whether any instruction can reach execution-core by a channel other than `RunAgentRequest` (for example session-core StartRun or `space_id` lookup) was not determined. StartRun was not read in full.
- **Whether the live `zdr_turn_settings` change is intended** (authored instructions on ZDR turns; `zdr_features.rs:139` Honored). No ledger entry found for it.
- **Do-mode label mapping.** Plan mode maps to the `agentic` feature (`chat-client.ts:135-137`; `use-chat-controller.ts:1043-1048`). The exact UI label "Do" was not located.
- **Knowledge-mode badge source.** No `knowledge_mode` or in-context field found. Candidates: `RetrievalScope` (`GW/src/normalize.rs:58`; `chat-source-scope.ts:18`), `ChatSourceScope` workspace/conversation (`chat-source-scope.ts:5`; `threads.source_scope`, 0038), or the sidebar `KnowledgeSidebarModeId` (`CoreSidebarKnowledgePanel.tsx:45`, a different concept). Which one the badge uses was not determined.
- **Org and persona in Do runs.** B3 includes them. Code confirms they are also absent from the agentic branch. Whether they are in scope for WS18 or WS02 was not determined.
- **Convex tests.** No test found referencing `instructionsForGateway` (grep). Whether any Convex test runner covers `spaces.ts` was not determined.
- **Data Plane migrations for `documents.space_ref`.** The gateway comment (`spaces.rs:1716-1728`) says `documents.space_ref` is the edge. Migration names were not listed.
- **Line 9118 (`build_run_agent_request`, live).** Taken from the WS02 sweep anchor; not re-grepped in live.
- **Sibling Verevon Space audit.** `C:/dev/verevon-audit-2026-10-14/01-verevon-space-audit.md` overlaps this item. Not read in this sweep.
- **Card F-t3-013 section 9.1 dependency "F-t3-012".** Its state was not checked here.
