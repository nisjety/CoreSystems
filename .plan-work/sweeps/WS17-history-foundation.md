# WS17-history-foundation: code-map sweep (F-t3-012)

Item: F-t3-012 "Conversation history foundation in session-core" (Priority, Partial, effort L, Model Plane + Frontend Plane, wave W2, confidence High).

Trees:
- WT = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b`, HEAD afb83928, clean except untracked `.plan-work/`.
- LIVE = `C:/dev/CoresSystem`, HEAD afb83928 plus the uncommitted set in `.plan-work/live-tree-status.txt`. The audit read LIVE.
- Verdicts: MATCH, DRIFT (present at other lines, new range given), ABSENT, PARTIAL. Live status: clean / M / ?? (live-only) / D.
- Method: symbol and pattern greps in both trees; `git --no-optional-locks diff -U0` hunk ranges to compute live line shifts; reads of cited windows. Git was used read-only.

---

## 0. Findings

1. The card's gap list holds. Absent in code: Postgres FTS or trigram index, cursor paging, fork/rewind/supersede/search routes, a message tree (parent, ordinal, leaf), `recall_conversations`, unread/star fields, fork lineage on threads.
2. Reusable pieces exist: `threads.pinned_at` / `archived_at` (migration 0021), `agent_memory.supersedes_id` / `is_latest` (migration 0037), session-core `ListConversation`, `ListThreads`, `DeleteThread`, `delete_thread_rows`, `verify_thread_read_space_decision`.
3. Two card citations match only LIVE, not WT: `tool_loop.rs:3142` (recall_memory arm) and `tool_loop.rs:4294-4302` (reattach_context definition). In WT they are at 3151-3206 and 4281-4289. Implementers working from the committed tree need WT numbers.
4. The CHAT_RESUME spec citation (1-80) is partial. That range is the #47 resume design. The versions rejection is at lines 87-99 and the persisted-versions path at 124-129.
5. Live-only changes that alter this item:
   - migrations 0044/0045 (`runs.retention_class`, the ZDR receipt);
   - `authorize_owner_housekeeping` (LIVE `auth.rs:295`, not in WT);
   - live edits to the presentation, archive and delete RPCs that change ZDR rules (presentation refuses only title and preview under ZDR; archive's explicit ZDR refusal is removed; the authorize_owner_housekeeping body was not read).
   The next migration number is 0046 once the live 0044/0045 are committed. From WT the next free number is 0044 and collides.
6. Messages carry no run or user column (`0001_init.sql:14-22`). Owner comes from `threads.user_id`, and ZDR status can only be joined through `threads` to `runs.thread_id`. Live Dreaming already does a thread-level waived-run exclusion (precedent).
7. Sidebar search exists but is client-side only. It filters the sessions already loaded by `listChatThreads` (at most 80).
8. Sibling Space items: F-t1-018 (permission-checked message search on a Session Core index, ZDR exclusion) is the same index; F-t1-017 (edit and delete as revision entries) touches the same message rows. Rows at `01-verevon-space-audit.md:93-94`; open question at `:114`. Not read beyond those rows.

---

## 1. Audit anchors (03-model-plane-chat-and-agents-audit.md)

| Anchor | Location | Content |
|---|---|---|
| Card | :516-531 | Tier Priority; Verevon today Partial; effort L; Model Plane (session-core single writer, model-gateway routes) with Frontend Plane (sidebar, versions UI); W2; High. |
| Wave table (9.1, W2) | :1551-1565, row :1562 | Dependencies: F-t3-016, OD19 branch semantics. |
| Summary row | :81 | Partial, L, Model Plane and Frontend Plane, W2. |
| Appendix B index row | :1830 | Reference column: Hermes Agent. |
| Appendix C reference corrections | :2015-2022 | Claude desktop search is a substring scan, not an index. Hermes is the only verified indexed search. Persisted versions with model or effort switch are 4 of 12, switch part DOC-only. Search prevalence conflicts (7, 3, 2 of 12). |
| Dependency notes 9.4 | item 5 at :1700; item 10 at :1705 | Index before tree; index in session-core, exclude ZDR, honour erase; F-t3-016 remaining M slice is W1 work. |
| Open decision OD19 | :1724 (section 10) | Branch and rewind semantics, share model. Required before the supersede step. |
| Appendix D | :2111 onward | No F-t3-012 entry (grep). |
| Evidence README | audit-evidence/README.md | Describes the pack only; no item data. |

---

## 2. Citation verification

| card id | cited path:lines | claim (3-6 words) | worktree verdict | live status | corrected path:lines (WT / LIVE) |
|---|---|---|---|---|---|
| F-t3-012 | apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:137-185 | gateway has list, archive, messages, replay; no fork or search | MATCH. Thread routes at 144, 147, 149-152, 159-160, 177, 181. Grep for fork, rewind, supersede, search finds no route. | M (hunks start at :824; route table untouched) | 137-185 / 137-185 |
| F-t3-012 | (no path) session-core migrations | no tsvector or trigram index | MATCH. No tsvector, trigram, pg_trgm or "USING GIN" index in session-core src or migrations. Earlier hits for "tsquery" and "fts" were false positives (ReplayRunEventsQuery, drafts). | migrations clean; live adds 0044/0045 (no FTS) | n/a |
| F-t3-012 | tool_loop.rs:3142 | recall_memory is semantic memory | DRIFT. recall_memory arm at 3151-3206; it calls `memory_client.search_memory` (3156-3162). 3142 is the browser_agent error arm. | M. LIVE arm at 3142: MATCH. | WT 3151-3206 / LIVE 3142-3197 |
| F-t3-012 | tool_loop.rs:4294-4302 | reattach_context definition | DRIFT. WT name at 4281 (web_search def starts 4290). | M. LIVE name at 4294: MATCH. | WT 4281-4289 / LIVE 4294-4302 |
| F-t3-012 | tool_loop.rs reattach arm (no line in card) | reattach reads only current thread | MATCH. Arm uses thread_id and org_id from the verified request; space fields left empty (3252-3254). | M | WT 3225-3269 / LIVE 3216-3260 |
| F-t3-012 | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/history.rs:21 | MAX_THREADS = 80 | MATCH | clean | 21 / 21 |
| F-t3-012 | history.rs:340-365 | BFF list capped; GET /v1/threads?limit=80&origin=chat | MATCH. `read_durable_threads` at 335; URL at 350-353. | clean | 335-370 / 335-370 |
| F-t3-012 | apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md:1-80 | versions client-only; persisted design rejected | PARTIAL. 1-16 header says both first designs rejected. Versions rejection list at 87-99; client-only v1 at 101-122; persisted path at 124-129. | clean | 82-130 (versions section) |
| F-t3-012 | chat-versions.ts:1-60 (short path) | client-only, session-lifetime state | MATCH. Full path apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-versions.ts. Constraints in header 1-22; helpers to 60. | clean | same |
| F-t3-012 | threads.pinned_at / archived_at (migration 0021) | reusable pin and archive columns | MATCH. apps/Model Plane/rust/services/session-core/migrations/0021_thread_presentation.sql:11-12 | clean | same |
| F-t3-012 | agent_memory supersedes_id / is_latest transaction (migration 0037) | reusable lineage and flip | MATCH. Columns 0037_agent_memory_lineage.sql:66-68; index 73-75. Write path dreaming.rs: doc 866-872, UPDATE is_latest=false 940-941, INSERT ... ON CONFLICT 956-967 and 989-1000 (WT). | migration clean; dreaming.rs M | dreaming.rs WT 866-1000 / LIVE doc 888, UPDATE 962 |
| F-t3-012 | session-core ListConversation (no line) | returns active path for every consumer (premise) | ABSENT as premise. Returns every row in sequence order; no leaf or active filter; no include_superseded. grpc.rs 5216-5330 (SQL 5284-5291, ORDER BY m.sequence). Proto rpc sessions.proto:102; messages 888-945. | grpc.rs M; proto M (+11 shift after :196) | grpc.rs WT 5216-5330 / LIVE 5387-5501; ListConversationRequest WT 890 / LIVE 901 |
| F-t3-012 | Space-audience threads need Control read decision (no line) | verify_thread_read_space_decision | MATCH. grpc.rs:1968 (pub(crate)); used by list_conversation and list_threads. | M | WT 1968 / LIVE 1970 |
| F-t3-012 | honour durable erase on thread delete (no line) | erase rows and side ledgers | MATCH. DeleteThread RPC WT 5767-5834. delete_thread_rows (WT 360-498) deletes agent_memory (368), events, session_audit_outbox, dream_runs, memory_index (379-391), then checkpoints, session_tool_audit_intents, managed_run_terminalization_outbox, runs and messages (484-488). The threads row is not deleted in that function. Org-wide erase gdpr.rs:289-290 (messages) and 368 (threads). | grpc.rs M; gdpr.rs clean | delete RPC WT 5767 / LIVE 5929; delete_thread_rows WT 360 / LIVE 362 |
| F-t3-012 | session-core list cap (no line) | default and max thread list size | MATCH. DEFAULT_THREAD_LIST_LIMIT = 80 and MAX = 200 at grpc.rs 68-69; clamp_thread_limit 148-153. Model-gateway default `unwrap_or(80)` at http_routes.rs:7882. | M | constants WT 68-69 / LIVE 70-71; clamp WT 148-153 / LIVE 150-155 |
| F-t3-012 | ListThreads paging (no line) | no cursor, only limit | MATCH. ListThreadsRequest (sessions.proto WT 947-974) has `limit` (953) and no cursor or offset. SQL ORDER BY pinned, recency, created, id with LIMIT $3 (grpc.rs WT 5412-5420). | proto M; grpc.rs M | proto WT 947-974 / LIVE 958-985; SQL WT 5412-5420 |
| F-t3-012 | pin and archive, server side (no line) | existing server-side pin and archive | MATCH. Pin SQL grpc.rs WT 5531-5536; archive 5630-5652; archive-all 5718-5719. | M (auth and ZDR rules changed in these RPCs; section 7) | presentation RPC WT 5482-5540 / LIVE hunks 5653-5660; archive_thread WT 5602 (hunk 5609) / LIVE hunk 5781 |
| F-t3-012 | plan items I8, I9, I12 (no line) | branch, supersede, FTS plus recall tool | MATCH. apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:220 (I8), 221 (I9), 222 (I12); exit criteria 228. | clean | same |
| F-t3-012 | sidebar search (no line) | sidebar search to add | PARTIAL. Client-side filter over already-loaded sessions only. ChatSidebarOrganizer.tsx:26 (searchOpen), 140 (selectOrganizedChats(props.sessions, ..., query())), 196 (button), 207-208 (input). | M (+1 shift) | WT 26, 140, 196, 207-208 / LIVE 27, 141 |
| F-t3-012 | localStorage folder list, 100-project cap (chat-organization.ts:2-34) | cap and storage | MATCH for cap: apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-organization.ts:34. Storage is via shared/session/client-storage (import line 2; key 8-9; read and write 11-18). The file has no literal localStorage. | clean | same |
| F-t3-012 | recall_conversations tool (no line) | read-only recall tool to add | ABSENT in code. Named only in CHAT_GAP_CLOSURE_PLAN:222 and CHAT_PARITY_AUDIT_2026-10-01.md:111. | n/a | n/a |
| F-t3-012 | unread and star fields (no line) | fields to add | ABSENT. No unread, starred, favorite or bookmark in session-core src or migrations. | n/a | n/a |
| F-t3-012 | fork lineage (no line) | fork-as-copy with lineage | ABSENT. No parent or forked_from column on threads (0001_init.sql:4-10 and later ALTERs). Only lineage in session-core: agent_memory (0037) and tasks parent. | n/a | n/a |
| F-t3-012 | F-t3-016 dependency (no line) | durable loop prerequisite | Not read beyond audit :1705. | n/a | n/a |
| F-t3-012 | ZDR exclusion key (no line) | run-level ZDR marker | PARTIAL. WT has ZDR flag only in managed_run_terminalization_outbox.zdr (0015:40) and session_tool_audit_intents.zdr (0013:7). Invoke path: `invoke_persistence_plan(zdr)` returns SuppressAllForZdr (http_routes.rs WT 9316-9322, LIVE 9447). Live-only runs.retention_class (0044). | n/a | n/a |
| F-t3-012 | listChatThreads client (no line) | frontend thread list, three callers | MATCH. chat-client.ts WT 2136-2137 (`requestJson('/api/v1/chat/threads')`); comment at 2122-2124 names three callers. | M (+53, unrelated type export) | WT 2136 / LIVE 2189 |
| F-t3-012 | recall tool allowlist (no line) | conversation tool allowlist | MATCH. `conversation_tool_allowed` WT tool_loop.rs:4214 / LIVE 4227. The scope doc cites 4151-4153, which is stale. | M | WT 4214 / LIVE 4227 |

---

## 3. Affected-file map

### A. session-core (Rust, Model Plane)

| path | role | approx lines (WT; LIVE if different) | live status | existing tests |
|---|---|---|---|---|
| apps/Model Plane/rust/services/session-core/src/grpc.rs | ListConversation, ListThreads, pin, archive, delete, clamp, erase | consts 68-69; clamp 148-153; delete_thread_rows 360-498; verify decision 1968; ListConversation 5216-5330; ListThreads 5332-5440; presentation 5482-5540; archive 5602-5765; DeleteThread 5767-5834; DeleteThreads 5838-5892 | M (861 changed lines) | persona_db_tests (2934+); delete_thread_and_bulk_delete_are_owner_bound_against_real_pg (9866); delete_space_threads_is_exactly_owner_and_space_bound_against_real_pg (10031); refuses_zdr_runs_foreign_threads_and_other_producers_without_writing (12496); get_context_assembly_reports_persona_enabled_from_the_threads_row (2849). Real-PG tests need DATABASE_URL. |
| apps/Model Plane/rust/services/session-core/src/auth.rs | authz helpers; live-only authorize_owner_housekeeping | authorize_thread_owner_row WT 450; LIVE authorize_owner_housekeeping 295 | M (396 added lines) | not enumerated |
| apps/Model Plane/rust/services/session-core/src/dreaming.rs | EXTEND/SUPERSEDE write; direct messages reader | WT 866-1000, 1226-1240; LIVE doc 888, UPDATE 962, load_pending_messages 1263, thread_has_waived_run ~1248-1262 | M (52 changed) | dream_once_only_grounds_when_a_classifier_is_configured (LIVE 2311); supersede tests (LIVE 2316, 2597) |
| apps/Model Plane/rust/services/session-core/src/gdpr.rs | org-wide erase of messages and threads | 289-290, 368; ordering test near 680 | clean | ordering assertion `pos("DELETE FROM messages")` (~680) |
| apps/Model Plane/rust/services/session-core/src/memory_erasure.rs | bulk-erasure propagation; pins delete_thread_rows call text | 1-9, 241-247, 321 | clean | both_bulk_deletes_capture_the_memory_ids_they_remove (261) |
| apps/Model Plane/rust/services/session-core/src/memory_grpc.rs | SearchMemory, ListMemory, IndexMemory (semantic recall backend) | file-wide | M (123 changed) | pg_row fixture test (1293) |
| apps/Model Plane/rust/services/session-core/src/memory_control.rs | user memory controls; direct messages read | 32 | clean | none listed |
| apps/Model Plane/rust/services/session-core/migrations/0001_init.sql | base threads, messages, runs | threads 4-10; messages 14-24; runs 26 | clean | n/a |
| migrations 0021, 0023, 0024, 0025, 0034, 0035, 0037, 0042 | thread and message columns; lineage | 0021:11-12; 0023:5-11; 0024:5-13; 0025:6-14; 0034:14-15 and later; 0035:15; 0037:66-75; 0042 | clean | n/a |
| apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql | runs.retention_class (zdr / persistent / zdr_waived), waiver columns, immutability trigger | lines 21-80 | ?? | retention_receipt::db_tests (ignored, need DB) |
| apps/Model Plane/rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql | partial index on waived runs | lines 14-17 | ?? | n/a |
| apps/Model Plane/rust/services/session-core/migrations/revert/ | down migrations for 0044 and 0045 | directory | ?? | n/a |
| apps/Model Plane/rust/services/session-core/src/retention_receipt.rs | RetentionClass, RunRetention, stored_run_retention | LIVE 42, 81, 291 | ?? | db_tests (ignored) |
| apps/Model Plane/rust/services/session-core/src/terminalization.rs, learning_events.rs, orchestration_grpc.rs, run_service_grpc.rs, orchestration_store.rs, memory_dedup.rs, main.rs | other session-core edits in live | file-wide | M | not inspected |

### B. Contracts (Model Plane)

| path | role | approx lines | live status | existing tests |
|---|---|---|---|---|
| apps/Model Plane/proto/model_plane/v1/sessions.proto | rpc GetContextAssembly 71, ListConversation 102, ListThreads 106, DeleteThread 125; messages ListConversationRequest 890, Response ~940, ListThreadsRequest 947-974, ListThreadsResponse 1018 | as listed | M (42 changed; ListConversation and ListThreads unchanged; live adds rpcs after :195 and messages after :1207) | none (proto) |
| apps/Model Plane/proto/model_plane/v1/memory.proto | SearchMemory 14, IndexMemory 17, ListMemory 22 | as listed | clean | none |
| apps/Model Plane/go/gen/model_plane/v1/sessions.pb.go, sessions_grpc.pb.go | generated Go stubs | file-wide | clean | n/a |
| apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/sessions_pb2.py | generated Python stubs | file-wide | clean | n/a |
| apps/Model Plane/rust/crates/mp-contracts/build.rs; src/lib.rs | proto to Rust build and re-export | file-wide | build.rs clean; lib.rs M | n/a |
| apps/Model Plane/api/contracts/zdr-refusals.json (+ .sha256) | ZDR refusal inventory | file-wide | ?? | n/a |

### C. model-gateway (Rust, Model Plane)

| path | role | approx lines (WT; LIVE if different) | live status | existing tests |
|---|---|---|---|---|
| apps/Model Plane/rust/services/model-gateway/src/http_routes.rs | thread routes and handlers | routes 144-181; get_thread_context 7816; list_threads 7867-7918 (LIVE 7980); update_thread_presentation 7923; archive_thread 7967; archive_threads 8003; delete_thread 8033 (LIVE 8146); list_thread_messages 8366 (LIVE 8479); replay_thread_events 8465; require_non_zdr_durable_mutation 823-835 (LIVE 823); invoke_persistence_plan 9316-9322 (LIVE 9447) | M (237 changed; thread handler bodies not in any hunk, shifted +113 to +131) | no thread-handler unit tests in file; see e2e_invoke_chain_test.rs below |
| apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs | recall_memory arm; reattach_context arm and definition; conversation_tool_allowed; web-search grounding | recall WT 3151-3206 / LIVE 3142-3197; reattach arm WT 3225-3269 / LIVE 3216-3260; def WT 4281-4289 / LIVE 4294-4302; allowlist WT 4214 / LIVE 4227 | M (425 changed) | reattach_context_without_a_thread_says_so (WT 12456); reattach_context_without_a_credential_never_reports_an_empty_conversation (WT 12493); reattach_context_is_described_as_conversation_only (WT 12534); recall_memory duplicate-signature test (WT 9645); inline_tool_allowed reattach test (WT 12448) |
| apps/Model Plane/rust/services/model-gateway/src/sse.rs | read_latest_assistant (ListConversation caller); reattach gating | read_latest_assistant WT 7491-7510 / LIVE 7628; gating WT 1543, 1897 | M (883 changed) | unauthenticated-ListConversation test (WT 5228-5234) |
| apps/Model Plane/rust/services/model-gateway/src/compaction.rs | compaction notices tell model to call reattach_context | 46, 54 | clean | in-file tests |
| apps/Model Plane/rust/services/model-gateway/src/prompt_context.rs | CONDITIONAL_TOOLS; reattach tool entries | 354; 803-813 | clean | in-file tests |
| apps/Model Plane/rust/services/model-gateway/src/audit.rs | audit classes for recall_memory and reattach_context | 18, 44-45 | clean | in-file |
| apps/Model Plane/rust/services/model-gateway/src/moderation.rs | TrustClass for recall_memory | 941 | clean | in-file |
| apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs | mock SessionCore incl. ListConversation; only test file referencing ListThreads or ListConversation | mock 1328-1346 | M | itself |
| apps/Model Plane/rust/services/model-gateway/tests/context_recovery_contract.rs | pins reattach dispatch arm text | 69-71, 138 | clean | itself |
| apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs; zdr_features.rs | ZDR posture and feature gating (live-only) | file-wide | ?? | tests/zdr_posture_matrix.rs, zdr_feature_table.rs, zdr_refusal_inventory.rs (all ??) |
| apps/Model Plane/rust/services/model-gateway/src/zdr_precheck.rs | ZDR precheck | file-wide | M | not inspected |

### D. capability-core (Go, Model Plane)

| path | role | approx lines | live status | existing tests |
|---|---|---|---|---|
| apps/Model Plane/go/services/capability-core/internal/sessionreview/transcript.go | ListConversation reader interface; skill-learning transcript read (spec cites :43) | interface 15-19 | clean | transcript_test.go (fakeReader 16-24) |
| apps/Model Plane/go/services/capability-core/internal/sessionreview/consumer.go | review consumer; comment on ListConversation | 41 | clean | consumer_test.go (fakeSessionClient 23-29) |
| apps/Model Plane/go/services/capability-core/internal/sessionreview/settle_test.go | fixtures for review settle | 106-117 | clean | itself |
| apps/Model Plane/go/services/capability-core/cmd/learning_review_switch_test.go | forbidden session client test double | 79-81 | clean | itself |
| apps/Model Plane/go/services/capability-core/cmd/servicetoken_wiring_test.go | ListConversation through service-token credential | 151-182, 357-401 | clean | itself |

### E. Frontend BFF (verevonv3/apps/gateway, Rust)

| path | role | approx lines | live status | existing tests |
|---|---|---|---|---|
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/history.rs | MAX_THREADS; BFF list_threads; chat_history_sessions filter; read_durable_threads; delete proxy | MAX 21; list_threads 152; chat_history_sessions 189; read_durable_threads 335-370; delete_durable_thread ~460-480 | clean | chat_history_excludes_space_scoped_conversations (803); chat_history_excludes_unscoped_non_chat_origins (857); chat_history_excludes_a_blank_origin_rather_than_assuming_chat (900); thread_listing_reports_persona_enabled_default_true_for_legacy_threads (985) |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat.rs | route table; `/api/v1/chat/threads` | 20 | clean | n/a |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/json_handlers.rs | proxies to thread messages, events, context | 248, 277, 335 | clean | n/a |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs | Space thread list (with space_id and decision); Space messages | 1176; 2687 | clean | wiremock tests near 6234-9411 |

### F. Frontend UI (verevonv3/src)

| path | role | approx lines | live status | existing tests |
|---|---|---|---|---|
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts | listChatThreads; applyRetentionFrom | WT listChatThreads 2136-2137 / LIVE 2189-2190; applyRetentionFrom 2128-2134 | M (53 changed) | chat-client.test.ts; chat-client.replay.test.ts; chat-client.served-by.test.ts; chat-client.zdr-capability.test.ts |
| apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-versions.ts | client-only version navigation state | header 1-22; helpers to 60 | clean | chat-versions.test.ts |
| apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts; ChatMessages.tsx; ChatPage.tsx | versions UI call sites (import chat-versions) | file-wide | M (+4; +229; +13) | use-chat-controller.f12.test.ts; ChatMessages.failure-actions.test.tsx (M) |
| apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-organization.ts | organisation model; 100-project cap; storage key | cap 34; key 8-9; read/write 11-18 | clean | chat-organization.test.ts |
| apps/Frontend Plane/verevonv3/src/shared/actions/chat-organization.ts | zod action contract (view.update, thread.move, project.*) | file-wide | clean | action-client.test.ts |
| apps/Frontend Plane/verevonv3/src/features/core/components/sidebar/ChatSidebarOrganizer.tsx | sidebar client-side search; pinned grouping | WT 26, 140, 196, 207-208 / LIVE 27, 141 | M (+2) | ChatSidebarOrganizer.test.tsx |
| apps/Frontend Plane/verevonv3/src/features/core/components/CoreSidebar.tsx | sidebar host; mounts organizer | file-wide | M (+2) | not enumerated |
| apps/Frontend Plane/verevonv3/src/shared/chat-nodes/tool-presentation.ts | display names for recall_memory (91), reattach_context (96); a new recall tool needs an entry | 91, 96 | clean | tool-presentation.test.ts (26-27) |
| apps/Frontend Plane/verevonv3/src/shared/session/client-storage.ts | readClientValue, writeClientJson | file-wide | clean | not enumerated |
| apps/Frontend Plane/verevonv3/tests/e2e/product-acceptance-helpers.ts | asserts tool names including reattach_context | 217 | clean | itself |

### G. Adjacent planes and docs (precedent only; do not change for F-t3-012)

| path | role | approx lines | live status | tests |
|---|---|---|---|---|
| apps/Data Plane v2/infra/postgres/migrations/20260519190000_bm25_tsvector.sql; 20260805120000_language_neutral_fts.sql | Data Plane document FTS (cross-plane DB not allowed) | file-wide | clean | not enumerated |
| apps/Data Plane v2/services/retrieval-engine-rs/src/search/sparse.rs | Data Plane sparse search | file-wide | clean | not enumerated |
| apps/Application Plane (all) | no FTS found (grep tsvector, trigram, tsquery) | n/a | n/a | n/a |
| apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md | version design history | 82-130 | clean | n/a |
| apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md | I8, I9, I12 plan items | 220-222; exit 228 | clean | n/a |
| apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_PARITY_AUDIT_2026-10-01.md | search and recall gap row | 111 | clean | n/a |
| apps/Model Plane/docs/decisions/ledger.md | subagent fork-semantics REJECTED (2026-08-24); not chat fork but check before any fork design | 1798-1800 | clean | n/a |
| apps/Model Plane/docs/capability-ownership-matrix.md | owner rows | 38, 53-57, 109-110 | clean | n/a |
| apps/master-ownership-matrix.md | session-core owner row | 126 | clean | n/a |

---

## 4. Related contracts

- Migrations (session-core): WT has 0001_init.sql through 0043_audit_outbox_requeue.sql (43 files). LIVE adds 0044_run_retention_receipt.sql and 0045_zdr_waived_runs_org_index.sql (both ??) and migrations/revert/ (??). Next free number: 0046 after the live pair is committed; 0044 from WT. Data Plane v2 has separate migrations under `apps/Data Plane v2/infra/postgres/migrations/`.
- Proto (sessions.proto): ListConversation (102; messages 888-945), ListThreads (106; 947-1018), DeleteThread (125), GetContextAssembly (71). Proto memory.proto: SearchMemory (14), IndexMemory (17), ListMemory (22). A cursor, search or recall RPC means new fields or messages, then regeneration: `buf generate` (Model Plane CLAUDE.md; buf config not located here), mp-contracts build.rs, go/gen, python _gen.
- Gateway routes (model-gateway, WT lines in http_routes.rs): GET and DELETE /v1/threads (144); POST /v1/threads/archive (147); POST /v1/threads/:thread_id/presentation (149-151); POST /v1/threads/:thread_id/archive (152); GET /v1/threads/:thread_id/context (159); DELETE /v1/threads/:thread_id (160); GET /v1/threads/:thread_id/messages (177); GET /v1/threads/:thread_id/events (181); GET and POST /v1/memories (192); DELETE /v1/memories/:memory_id (193). No search, fork, rewind, supersede or recall route.
- BFF routes: GET /api/v1/chat/threads (domains/chat.rs:20). Thread proxies in domains/chat/history.rs (list 511 WT; presentation 430; delete 478) and json_handlers.rs (248, 277, 335).
- SSE: apps/Model Plane/docs/sse-event-taxonomy.md (LIVE M, +4 lines); sse_events.rs (LIVE M, 203 changed). No SSE event for thread search or fork exists. Recall output would travel as tool results. NOT DETERMINED which event carries tool output.
- Config and env: DATABASE_URL (session-core real-PG tests; compaction_tests.rs:353; approval_delivery.rs:1112); REQUIRE_CONTRACT in live CI job session-core-receipts; BFF model_gateway_url (history.rs:352). No new env var identified.
- Action contracts: apps/Frontend Plane/verevonv3/src/shared/actions/chat-organization.ts. Root CLAUDE.md requires the contract before a UI operation, so search, pin, archive, star and unread need entries. Location not decided.
- ZDR contracts: model-gateway require_non_zdr_durable_mutation (WT 823-835; error code zdr_durable_mutation_forbidden, status 412). Live ZDR refusal inventory apps/Model Plane/api/contracts/zdr-refusals.json (??); frontend shared/api/zdr-refusals.ts (??).

---

## 5. Test and build commands

- Model Plane Rust (`apps/Model Plane/rust`, workspace members services/model-gateway and services/session-core, Cargo.toml lines 14-15; crate names model-gateway and session-core, Cargo.toml line 2):
  - `cargo test -p session-core --all-targets` and `cargo test -p model-gateway --all-targets` (Model Plane CLAUDE.md, Testing).
  - `cargo test --workspace --no-fail-fast` (.github/workflows/model-plane-rust-ci.yml:119); `cargo fmt --all -- --check` (:99); `cargo check --workspace --all-targets` (:112). Working directory apps/Model Plane/rust (:57).
  - DB-backed session-core tests need `DATABASE_URL` pointing at a migrated Postgres (src/compaction_tests.rs:353; src/approval_delivery.rs:1112). LIVE CI adds job session-core-receipts (Postgres 16) for #[ignore]d receipt tests (live model-plane-rust-ci.yml, +50 lines).
  - Stack: `./scripts/compose.sh up -d` (Model Plane CLAUDE.md, Build and Run).
  - User memory notes that two cargo targets fail on Windows pre-existing; use --lib for those. Not repo-verified here.
- Go: in apps/Model Plane/go/services/capability-core run `go test ./...` and `go vet ./...` (Model Plane CLAUDE.md, per service). go.mod not checked here.
- BFF gateway (`apps/Frontend Plane/verevonv3/apps/gateway`, crate verevon-gateway-rs, Cargo.toml line 2): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (.github/workflows/verevonv3-ci.yml:133, 138, 141). Root CLAUDE.md: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`.
- Frontend (`apps/Frontend Plane/verevonv3`, package.json scripts block lines 9-17): `pnpm typecheck` (tsc -b), `pnpm lint` (eslint .), `pnpm test` (node --test ... && vitest run), `pnpm build`, `pnpm test:e2e` (playwright). Targeted: `pnpm vitest run src/features/chat/lib/chat-versions.test.ts src/features/chat/lib/chat-organization.test.ts src/features/core/components/sidebar/ChatSidebarOrganizer.test.tsx src/shared/api/chat-client.test.ts`.
- CI files: .github/workflows/model-plane-rust-ci.yml (LIVE +50), .github/workflows/verevonv3-ci.yml (LIVE +55), apps/Frontend Plane/verevonv3/package.json (LIVE +2 demo scripts).

---

## 6. Owners

- master-ownership-matrix.md:126: `session-core` (Rust) owns threads, runs, checkpoints, memory index, compaction and context assembly.
- capability-ownership-matrix.md:38 (one writer per resource), :53 (thread and message timeline: session-core, Postgres, gateway ingress, outbox), :56 (checkpoints, context assembly, compaction), :57 and :109-110 (memory index is session-core's canonical index).
- Root CLAUDE.md, Architecture Rules: Model Plane owns sessions and runs; Control Plane owns identity, users, orgs, sessions, audit, entitlements. The Space read decision is Control-signed (grpc.rs:5231-5236 comment). Data Plane v2 owns documents and retrieval, with no cross-plane DB. Application Plane owns collaborative projections and notifications. Frontend Plane owns the UI and BFF normalisation. ZDR must propagate through any content-persisting boundary.
- apps/Model Plane/CLAUDE.md, Code Style: model-gateway dispatch_tool is the read-only router ("plain chat" loop). A read-only recall tool fits that loop's refusal boundary (inference from CLAUDE.md; execution-core's loop is the side-effect path). One canonical owner per capability, checked against capability-ownership-matrix.md.
- Sibling: F-t1-018 search index placement is an open question (01-verevon-space-audit.md:114). The capability matrix names session-core as the thread and message timeline owner (:53).

---

## 7. Uncommitted-only files (LIVE tree) relevant to F-t3-012

### 7a. Live-only (??)

- apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql: runs.retention_class (zdr / persistent / zdr_waived), waiver columns, immutability trigger, index (lines 21-80).
- apps/Model Plane/rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql: partial index on waived runs (14-17).
- apps/Model Plane/rust/services/session-core/migrations/revert/: down migrations.
- apps/Model Plane/rust/services/session-core/src/retention_receipt.rs: RetentionClass (42), RunRetention (81), stored_run_retention (291).
- apps/Model Plane/api/contracts/zdr-refusals.json and .sha256.
- apps/Model Plane/rust/crates/mp-contracts/src/retention.rs, src/zdr_refusal.rs, tests/zdr_retention_matrix.rs, tests/fixtures/.
- apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs, src/zdr_features.rs; tests zdr_posture_matrix.rs, zdr_feature_table.rs, zdr_refusal_inventory.rs; tests/fixtures/zdr-posture-matrix.json.
- apps/Frontend Plane/verevonv3/src/features/core/components/sidebar/ChatWorkLinks.tsx (mounted by ChatSidebarOrganizer and CoreSidebar).
- apps/Frontend Plane/verevonv3/src/shared/actions/chat-work-period.ts (+ test); src/features/chat/components/ChatWorkPeriodPicker.tsx.
- apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts (+ tests); src/shared/api/contracts/; chat-normalizers.zdr-refusal.test.ts; use-chat-controller.zdr-refusal.test.ts; chat-client.zdr-refusal.test.ts.

### 7b. Modified (M), one-line summary

- session-core grpc.rs (861 changed): run-start edits in start_run_inner (WT 2974-3218; likely the retention receipt, not read). Presentation, archive and delete RPCs switch from authorize_operation to authorize_owner_housekeeping and drop the blanket ZDR refusal: presentation refuses only title and preview under ZDR (live comment: pin and persona are the owner's under every posture, "Owner decision 17"); archive's explicit ZDR refusal is removed; authorize_owner_housekeeping body not read (hunks at WT 5482-5488, 5609, 5698, 5774, 5845). list_conversation and list_threads bodies have no hunk.
- session-core auth.rs (396 added): adds authorize_owner_housekeeping (LIVE 295). Rest not read.
- session-core dreaming.rs (52 changed): adds thread_has_waived_run and a NOT EXISTS zdr_waived filter in load_pending_messages (LIVE ~1248-1290).
- session-core memory_grpc.rs (123), terminalization.rs (123), orchestration_grpc.rs (249), learning_events.rs (77), run_service_grpc.rs (21), orchestration_store.rs (8), memory_dedup.rs (5), main.rs (1): not read beyond stat.
- proto sessions.proto (42 changed): adds ManagedRunLifecycle rpcs after WT :195 and scheduled-step receipt messages after :1207. ListConversation and ListThreads unchanged.
- model-gateway http_routes.rs (237 changed): approval, plan, cancel, resume, ai_* and invoke edits (WT 824-829 guard body; 8810-9003 invoke). Thread handler bodies unchanged.
- model-gateway tool_loop.rs (425 changed): adds web-search grounding gate (WT ~5691-5986) and artifact id handling (~345). recall and reattach arms only shift.
- model-gateway sse.rs (883), sse_events.rs (203), chat_turn_registry.rs (36), state.rs (6), zdr_precheck.rs: not read beyond stat.
- docs/sse-event-taxonomy.md (4): not read.
- frontend chat-client.ts (53): adds ChatRetrievalNoticeEvent type; listChatThreads unchanged.
- ChatSidebarOrganizer.tsx (2), CoreSidebar.tsx (2), CoreNavbar.tsx (2): ChatWorkLinks import and mount.
- use-chat-controller.ts (4), ChatMessages.tsx (229), ChatPage.tsx (13), chat-normalizers.ts (39), chat-types.ts (7), chat-sidebar-organizer.css (5): not read.
- .github/workflows/model-plane-rust-ci.yml (50): adds job session-core-receipts (Postgres 16) running #[ignore]d receipt DB tests.
- .github/workflows/verevonv3-ci.yml (55): not read.
- apps/Frontend Plane/verevonv3/package.json (2): demo:build and demo:probe scripts.

### 7c. Deleted (D)

- None relevant to F-t3-012.

### 7d. Clean and relevant (committed, unchanged in LIVE)

- gateway history.rs, chat-versions.ts (+ test), chat-organization.ts (+ test), shared/actions/chat-organization.ts, ChatSidebarOrganizer.test.tsx.
- Migrations 0001-0043 (in particular 0021 and 0037 cited by the card).
- gdpr.rs, memory_erasure.rs, memory_control.rs, memory.proto, sessionreview (capability-core), CHAT_RESUME spec, CHAT_GAP plan, ledger.md, ownership matrices.

---

## 8. Not determined

1. Whether the session-core run path appends messages rows for `retention_class = 'zdr'` runs. Only the invoke path was traced (SuppressAllForZdr). Messages have no run or user column, so exclusion needs a thread-to-runs join or a stamped column. Whether one thread can mix ZDR and persistent runs is unknown.
2. Whether `zdr_waived` content (persisted on the waiver connection) should be indexed. LIVE Dreaming excludes waived threads; the comment cites "decision D-B". The audit text does not cover this.
3. Owner decisions cited in live code ("Owner decision 17" in grpc.rs; "decision D-B" in dreaming.rs) are not located in repo docs. ledger.md was grepped only for OD19 and persisted-version terms (no hits).
4. Sibling F-t1-018 index design: only the audit rows were read (01-verevon-space-audit.md:93-94, :114). Ownership and exclusion for the shared index are not reconciled.
5. F-t3-016 card not read (dependency).
6. OD19 status: open per audit :1724. Whether a repo ledger entry resolves it was not checked.
7. FTS configuration (tokenizer, language) not determined. Data Plane has a language-neutral FTS migration as precedent (20260805120000_language_neutral_fts.sql).
8. Whether any caller other than the chat sidebar relies on the 80-thread cap. Space listing (spaces.rs:1176) passes its own limit; not traced.
9. LIVE line numbers for session-core RPC bodies come from hunk arithmetic plus symbol greps; not each line re-read.
10. capability-core go.mod location and buf config location not located.
11. Which LIVE sse.rs changes touch reattach gating (WT 1543, 1897): not read.
12. Whether the chat-versions UI call sites (use-chat-controller.ts, ChatMessages.tsx, ChatPage.tsx) rely on version state that a persisted tree would replace. Only the import sites were located; their LIVE edits were not read.
13. Whether the frontend should apply `applyRetentionFrom` (chat-client.ts:2128-2134) to new search or recall responses. Design question.
