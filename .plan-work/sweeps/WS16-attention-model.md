# WS16-attention-model: code map for F-t3-015

Generated 2026-10-15. Read-only sweep: no product code, config or docs edited; no git write run.
- WT = worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean).
- LIVE = `C:/dev/CoresSystem` (same HEAD, 324 uncommitted entries; list in `.plan-work/live-tree-status.txt`).
- Live status below comes from `git --no-optional-locks status|diff` on LIVE. "clean" = identical to HEAD.
- Audit card read: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`.
- Line numbers are WT unless marked "live". Lines marked "grep" were seen in grep output, not in a full Read.

## 0. Key findings

1. Card F-t3-015 is Priority, XL, W2, planes Model Plane / Application Plane / Frontend Plane. All 6 code-cited "Verevon today" paths exist. Four cited ranges drift in WT; CoreSidebar.tsx differs in LIVE (+2 lines).
2. No PWA and no Web Push code exists in WT or LIVE. `index.html` has no manifest or theme-color link. Only plan and audit docs mention the terms.
3. Approval and pause events ARE emitted, but in-process only. session-core `create_approval` (WT orchestration_grpc.rs:2461, emit 2572-2585) calls `broadcast_event` into a per-run replay buffer. `StreamRunEvents` and model-gateway `run_events_sse` serve it. This path is not NATS, not the outbox and not notification-core.
4. The session-core outbox carries one run-lifecycle event: RUN_COMPLETED for the skill-learning loop (learning_events.rs). RUN_FAILED, paused and needs-* events are not enqueued.
5. The orchestrator-core typed publisher is unwired. `orchestration.New()` returns the Service and cmd/main.go:305 discards it (`_, handlers :=`). So `Service.Publish` (service.go:52) and `NewRunPausedForApproval` (types.go:108) have no production caller. postmortem 0002:272 says orchestrator-core publishes mp.v1.orchestration.*; the code does not. "MP_ORCHESTRATION_EVENTS holds 0 messages" fits "no publisher", not "dropped grant". Fixing nats.conf alone does not produce typed events.
6. orchestrator-core's real run-event publishes go to `mp.v1.run.<id>.event` (activities.go:412; RUN_COMPLETED :752, RUN_FAILED :764). Its grant allows that subject (nats.conf:105). RUN_COMPLETED therefore has two publishers: session-core (outbox) and orchestrator-core (direct). Whether the run-watch consumer deduplicates them is NOT DETERMINED (see section 10).
7. Run-watch notifications set no `retention_mode` and never read the envelope `zdr` flag (consumer.go:226-232). notification-core defaults them to `standard` (service.go:621-623). Payload is content-free (consumer.go:258-279). "Content-free on ALL threads" is not enforced at this boundary.
8. Notification types are wired for two run types only: `modelplane.run_completed`, `modelplane.run_failed`. Allowlist handlers.go:211-214. Channel rows migration 013. Tests server_test.go:523-526, 535, 590.
9. Run status is a free string end to end (runs.proto:282-283; sessions.proto:995 WT / 1006 live; chat-client.ts:733 WT). No proto enum for run status exists. Frontend status vocabulary is duplicated: CoreSidebar.tsx:878-894 and chat-organization.ts:47-48.
10. The "REST refetch" the card wants to replace is the navbar resource: CoreShell.tsx:35, timer 59-82 (60 s visible, 180 s hidden, plus a visibilitychange refetch). No per-user stream exists.
11. Migrations: session-core WT latest is 0043_audit_outbox_requeue.sql. LIVE adds 0044 and 0045 (untracked) plus revert/. A WS16 migration must take the next free number of the tree it lands in.
12. Memory conflict: user memory says "no go.work", but `apps/Model Plane/go/go.work` is tracked (425 bytes) and lists capability-core, nats-provisioner, orchestrator-core. NOT DETERMINED which mode is current.
13. Sibling reconcile: F-t1-008 / F-t1-009 (01-verevon-space-audit.md) overlap on the allowlist, per-user aggregate stream, badges, sidebar and ZDR. See section 9.

## 1. Audit cross-references (03 audit file)

| Where | Lines | Content |
|---|---|---|
| Card F-t3-015 | 564-579 | Tier row 568 (Priority, Partial, XL, W2, High). What 570; why 572; references 574; today 576; build 578. |
| Section 9.1 row | 1565 | Under "Wave W2" (header 1551; goal 1553). Priority, XL, deps F-t3-016, F-t3-012. |
| Summary table | 84 | Partial, XL, W2. |
| Section 2.12 | 248-251, 254 | Rows cite F-t3-015. Line 254 cites consumer.go:81-100 and states "no PWA manifest and no Web Push". |
| Section 3 | 294 | Inbox row (T3 Code, Claude desktop). |
| F-t3-011 card | 514 | Depends on "the attention events of F-t3-015" (ask_user needs_input). |
| Section 7 | 1340, 1382 | Claude desktop and T3 Code references. 1382 also cites F-t3-049. |
| Section 9.1 goal | 1553 | W2 goal names "the attention model". |
| Section 9.1 F-t3-011 row | 1561 | F-t3-011 depends on F-t3-015. |
| Section 9.4 | 1700, 1705 | 1700: F-t3-012 precedes F-t3-015. 1705: F-t3-016 remaining M slice is W1 work, listed as dependency of F-t3-015. |
| Section 10.1 | 1712 | ZDR risk: "attention notifications must be content-free on all threads". |
| Appendix B | 1833 | Priority, partial, XL, Model Plane / Application Plane / Frontend Plane, W2; reference Claude desktop. |
| Appendix C | 2040-2047 | Reference-side check partial, confidence medium. Corrections: closed attention set is a Verevon synthesis; Antigravity push is FCM not VAPID; PWA is a variant enum only; mobile and push are DOC-only; same-vendor pairs inflate counts. |
| Appendix D | none | No F-t3-015 or "attention" entry in 2111-2161. |
| Related card F-t3-049 | 1004-1014 | "Derive one thread status with fixed precedence and an unseen-completion state" overlaps the WS16 enum and "Unseen results". |

## 2. Citation verification (card F-t3-015)

Verdicts: MATCH (symbol or behaviour present; drift noted), DRIFT, ABSENT, PARTIAL.

| # | Cited in card | WT verdict and location | Claim (short) | Live | Corrected path:lines (WT / live) |
|---|---|---|---|---|---|
| C1 | apps/Model Plane/go/services/capability-core/internal/runwatch/consumer.go:81-100 | MATCH, drift to 80-97 (const 80-83; terminalNotificationType 88-97; Watcher 100-103) | runwatch maps only run_completed and run_failed | clean | consumer.go:80-97 |
| C2 | apps/Application Plane/notification-core/migrations/013_register_run_watch_notifications.up.sql:1-14 | MATCH, drift to 1-16 (rows 4-15; ON CONFLICT 16) | in_app on, email off, per run type | clean | 013 up.sql:1-16 |
| C3 | apps/Model Plane/proto/model_plane/v1/runs.proto:282 | MATCH (comment lists status words; field `string status = 5` at 283; RunDetail 269-304) | run status is free string | clean | runs.proto:282-283 |
| C4 | apps/Frontend Plane/verevonv3/src/features/core/components/CoreSidebar.tsx:869-900 | MATCH, drift to 867-897 (fn 872-897; use 643) | status string to two chip tones | M (+2 lines) = DIFFERS-IN-LIVE | WT 867-897; live 869-899 (fn 874-899; use 644) |
| C5 | chat-organization.ts:47-62 (short path, resolved) | MATCH at apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-organization.ts:47-62 (status sets 47-48; selectOrganizedChats 49-62). src/shared/actions/chat-organization.ts is 28 lines and is not the cited file | status sets and attention filter | clean | features/chat/lib/chat-organization.ts:47-62 |
| C6 | apps/Model Plane/rust/services/execution-core/src/state.rs:16-40 | MATCH, drift to 15-40 (enum 15-26; as_str 28-40). Variants: Running, Paused, AwaitingApproval, Completed, Failed, Cancelled. No needs_input, plan_ready or archived | RunStatus enum exists | clean | state.rs:15-40 |
| C7 | (no cite) "run.paused_for_approval ... orchestrator-core publishers the broker drops" | PARTIAL. Names exist: orchestrator types.go:18, 108-115, 151; mp-events subjects.rs:232; orchestration.proto:1036. No production caller of NewRunPausedForApproval or of Service.Publish (service.go:52); cmd/main.go:305 discards the Service. The production emitter is session-core (section 3, A) | typed event publisher | clean (types.go, cmd/main.go). session-core orchestration_grpc.rs is M | types.go:18, 108, 151; cmd/main.go:305 |
| C8 | apps/Model Plane/go/services/nats-provisioner/publishers_registry_test.go:70-73 | MATCH (gatewayGrant 70-71; orchestratorGrant 72-73) | ACL gap constants | clean | 70-73 |
| C9 | apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md:260-278 | MATCH, section 261-278; table 266-273; grant row 272 | publish grants dropped; gap rows | clean | 261-278 (row 272) |
| C10 | (no cite) notification-core README: full payload reaches Novu, ZDR unproven | MATCH: README.md:14 (retention_mode standard or zdr); 62-67 (zdr transient; full payload still sent to Novu; do not enable) | ZDR content delivery not enabled | clean | README.md:62-67 |
| C11 | (no cite) "typed run-state events must come from session-core's outbox" | MATCH as a seed. Outbox table audit_publisher.rs:799 (CREATE); drainer classify_outbox_subject :69. Only RUN_COMPLETED is enqueued: learning_events.rs:185 (fn), grpc.rs:3364, terminalization.rs:1112 | outbox carries RUN_COMPLETED only | learning_events.rs, terminalization.rs, grpc.rs are M | live: learning_events.rs:208; call sites grpc.rs:3506, terminalization.rs:1186 |
| C12 | (no cite) "CoreNavbar unread badge" | MATCH: CoreNavbar.tsx:86-88 (counts); 650-655 (BadgeButton) | unread badge reusable | M (2 lines at 182; badge lines unchanged) | 86-88, 650-655 in both trees |
| C13 | (no cite) "per-run run_events SSE" | MATCH: http_routes.rs:218-220 (replay 218; SSE 220); sse.rs:7398 run_events_sse | per-run SSE exists | sse.rs M (run_events_sse live 7533); http_routes.rs M (route line not re-read live) | sse.rs:7398 WT / 7533 live |
| C14 | (card) "orchestrator-core publish grants" | MATCH: deploy/nats.conf:105 publish `mp.v1.run.*.event` only; 106-107 subscribe `mp.v1.orchestration.>` and `mp.v1.feedback.rated` | orchestrator cannot publish mp.v1.orchestration.* | clean | nats.conf:102-107 |
| C15 | (no cite) "No PushManager, VAPID or manifest code exists in verevonv3 or Application Plane" | MATCH: repo-wide grep finds only plan and audit docs (verevonv3/apps/verevon-web/plans/system-audits). LIVE project code: only node_modules hits. index.html:5-8 has favicon and fonts only | no PWA, no push | clean | index.html:5-8 |

Card items with no code citation and no verdict needed: "service worker never caches authenticated content" (design), "Depends on F-t3-016 and F-t3-012" (audit dependency, see section 1).

## 3. Behaviour checks behind the card (grep and Read verified)

A. Session-core emits approval and pause events in-process. WT orchestration_grpc.rs: `create_approval` 2461; ApprovalStateChanged broadcast 2555; RunPausedForApproval broadcast 2572-2585. Live: create_approval 2509; RunPausedForApproval constructor 2626-2627. `broadcast_event` (WT :1726, live :1762) sends to a broadcast channel and a per-run ReplayBuffer; no NATS publish and no outbox row. `stream_run_events` (WT :3177, live :3239) replays after `after_event_id`, then tails. Model-gateway `run_events_sse` (sse.rs:7398; live 7533) forwards Last-Event-Id and renders via `orchestration_event_to_sse` (WT :9849; live :10211), name `run_paused_for_approval` (WT :9857; live :10219).

B. orchestrator-core publishes run lifecycle to NATS. activities.go:374-415 `publishRunEvent` publishes to `natsx.RunEventSubject(runID)` at 412. RUN_COMPLETED 752; RUN_FAILED 764. Eval events: evalinfer.go:85, 114; evaloptimizer.go:224. The envelope carries `zdr` from retention (activities.go:401; envelopeFlag at 218). The doc comment above 374 says a RetentionZeroData event is dropped before the bus.

C. orchestration.Service publisher has no production caller. service.go:43 NewService; :52 Publish; :96 raw publish. wire.go:13-16 `New` returns (svc, handlers). cmd/main.go:305 `_, handlers := orchestration.New(pub, logger, sessionClient)`. Grep for `.Publish(` in internal and cmd (non-test): no caller of Service.Publish. handlers.go has no Publish reference.

D. Outbox scope. Migration 0016:26-34 widens session_audit_outbox subject CHECK to `verevon.audit.v2.model.session-core.%` OR `mp.v1.run.%.event`. Writers: learning_events.rs enqueue_row (248; INSERT 278) for RUN_COMPLETED only. approval_delivery.rs header (1-6): identifier-only approval outbox with no dispatcher. RUN_FAILED is written to `events` (grpc.rs:3286 mapping; terminalization.rs:329), not the outbox.

E. Typed vocabulary elsewhere is unused by services. mp-orchestration `RunEvent` enum (run_event.rs:33, 150) is referenced only from its crate lib.rs. mp-events constants subjects.rs:229-233. No proto enum for run status. ChatEvent (model-gateway sse_events.rs) has no run-paused or attention variant; the only approval hits are PlanApprovalControl comments (WT 266, 372, 1410). Grep for needs_input, plan_ready, ask_user finds no typed producer (execution-core permission/mod.rs:503 is a test name).

F. Gateway approval subjects are ad hoc. approvals.rs:755 (`agents.approval.requested`), 801 and 842 (`agents.approval.resolved`). capability-ownership-matrix.md:88 says no in-repo consumer subscribes. postmortem 0002:272 says the broker drops them.

G. Run-watch path. consumer.go:64 `RunEventSubject` = `mp.v1.run.*.event`; 69 stream `MODEL_PLANE_RUN_EVENTS`; 76 durable `capability-core-run-watch-notify`; nats-provisioner main.go:112-115 binds the durable with DeliverSubject `_VEREVON.MODEL.DELIVER.capability.run_watch_notify` (grant nats.conf:96). Terminal map 88-97. Watchers: store.go:28 PendingWatchers, :55 MarkNotified. Accept at consumer.go:226-232. Idempotency key shape: client_test.go:28 (`run-watch:<org>:<run>:<user>:RUN_COMPLETED`).

H. Retention gap. consumer.go:226-232 sets OrganizationID, IdempotencyKey, Recipient, Type, Payload. It sets no RetentionMode and reads no zdr (grep: no retention or zdr token in consumer.go). notifyclient client.go:104 `RetentionMode` is omitempty. notification-core service.go:621-623 defaults to `standard`. Payload builders consumer.go:258-279 (not re-read line by line). Whether ZDR runs can reach this consumer is NOT DETERMINED.

I. Notification allowlist and channels. handlers.go:199-218 `isNotificationTypeAuthorized`; capability-core entries 211-214. Channel rows: migration 013 (org `_default`, in_app on, email off). Per-user preferences: migration 004 and internal/preferences/preferences.go (not re-read).

J. Frontend status chips and filters. CoreSidebar.tsx WT 867-897 (`chatRunStatusChip`): active for queued, pending, running, in_progress; attention for awaiting_approval, waiting_approval, paused, blocked, ambiguous, failed, timed_out, denied, rejected. chat-organization.ts:47-48 repeats the same sets. Organizer filter ChatSidebarOrganizer.tsx:202 (all, pinned, active, attention).

K. Section labels. "Waiting for you", "Unseen results", "Settled" are absent from features/core and features/chat/lib. "Needs attention" exists at CoreSidebar.tsx:891 and CoreSidebarSupportPanel.tsx:420. "Working" exists only as a Spaces label (CoreSidebarSpacesPanel.tsx:386, isSpaceWorking at 252).

L. Polling and streams. CoreShell.tsx:35 navbar resource; 59-82 timer (line 71: 180 s hidden, 60 s visible) and visibilitychange refetch (73-78). CoreSidebarSpacesPanel.tsx:248: sidebar fetches nothing for Spaces; the Space page publishes on its own poll. Per-run SSE client: run-console-client.ts:394 (events), 460 (replay). Generic reader: sse.ts:37 (Accept text/event-stream).

M. Run-watch frontend. RunNotifyButton.tsx: module-level state Map (51; MAX 50); toggle 106-126; uses runs-client.ts watchRun 198, unwatchRun 207, getRunWatchStatus 215 (import at 25). Gateway run_watchers.rs (router 52-57) -> model-gateway http_routes.rs:396-399 -> capability-core run_watchers.go. No action contract for run watch (action-registry.ts has only notifications.*).

N. Gateway SSE pass-through. chat.rs:74 route `/api/v1/runs/{run_id}/events`; streams.rs:197 upstream `{}/v1/runs/{}/events`. chat.rs:50 thread events.

O. Other run-state writers. grpc.rs RUN_STARTED insert 3054-3083; event-kind ordering 7020-7029 (RUN_STARTED 90, RUN_FAILED 92, BROWSER_RUN_PAUSED 142). execution-core RunStatus users: browser_agent.rs, browser_events.rs, grpc.rs, runtime_loop/agent.rs, state.rs.

## 4. Affected-file map (depth 2)

Columns: path | role | lines (WT; live where different) | live | tests (path, names where cheap).

### 4a. Model Plane, Rust

| path | role | lines | live | tests |
|---|---|---|---|---|
| rust/services/session-core/src/learning_events.rs | RUN_COMPLETED to session_audit_outbox (only run event enqueued) | enqueue_run_completed 185 (live 208); enqueue_row 248 (live 279); INSERT 278 | M (+77) | same file: golden_run_completed_wire_body 316; subject_is_the_fixed_run_event_subject 417; both_terminal_paths_enqueue_the_learning_event 588 |
| rust/services/session-core/src/audit_publisher.rs | outbox drainer, subject classes, authority check | classify_outbox_subject 69; check_outbox_authority 87; SUBJECT_CLASSES 608; CREATE TABLE 799 | clean | same file: classify cases ~851-864; metrics 1323-1330 |
| rust/services/session-core/migrations/0016_outbox_run_lifecycle_subjects.sql | outbox CHECK allows mp.v1.run.%.event | 26-34 | clean | none |
| rust/services/session-core/src/terminalization.rs | terminal writes; RUN_FAILED outcome; learning enqueue | RUN_FAILED 329; insert_managed_run_started 519 (live 535); enqueue call 1112 (live 1186); start_managed_run_inner 656 (live 691) | M (+123) | same file (not enumerated) |
| rust/services/session-core/src/grpc.rs | RUN_STARTED insert; RUN_FAILED mapping; enqueue call; thread list latest_run_status | RUN_STARTED 3054-3083; RUN_FAILED 3286; enqueue 3364 (live 3506); latest_run SQL 5384-5458 (live 5555-5629); kind ordering 7020-7029 | M (+861) | same file: 9920 (RUN_STARTED insert test), 11194 (terminal events query) |
| rust/services/session-core/src/orchestration_grpc.rs | in-process approval and pause events; StreamRunEvents | broadcast_event 1726 (live 1762); RunPausedForApproval map 1797 (live 1833); create_approval 2461 (live 2509); emit 2572-2585 (live 2626); stream_run_events 3177 (live 3239) | M (+249) | live: broadcast_event_assigns_id_and_buffers_per_run 4053; replay_buffer_evicts_oldest_beyond_cap 4130 |
| rust/services/session-core/src/orchestration_store.rs | Postgres plans, approvals, provenance | RunProvenanceRow 1333 | M (+8) | same file (not enumerated) |
| rust/services/session-core/src/orchestration_nats.rs | consumes mp.v1.orchestration.> and re-broadcasts | file header line 1 | clean | none read |
| rust/services/session-core/src/approval_delivery.rs | identifier-only approval outbox; no dispatcher | header 1-6; MAX_APPROVAL_DELIVERY_ATTEMPTS | clean | same file (not enumerated) |
| rust/services/model-gateway/src/sse.rs | per-run SSE proxy; orchestration event SSE names | run_events_sse 7398 (live 7533); orchestration_event_to_sse 9849 (live 10211); name 9857 (live 10219); projection 9941 (live 10303); 10133 (live 10495) | M (+883) | sse.rs test modules (subscription_route_tests ~562-571 in diff context) |
| rust/services/model-gateway/src/http_routes.rs | routes; watcher proxies | replay 218; SSE 220; watchers 391-399; replay_run_events 1282; proxies 5652-5703 (live 5750-5801) | M (+237) | rust/services/model-gateway/tests/orchestration_http_test.rs (exists) |
| rust/services/model-gateway/src/sse_events.rs | ChatEvent enum; SSE taxonomy producer | ChatEvent around 243 (diff context); no run-paused variant | M (+203) | same file mod tests (~1381, ~1461) |
| rust/services/model-gateway/src/approvals.rs | ad hoc agents.approval.* publishes | 755, 801, 842 | clean | not read |
| rust/services/execution-core/src/state.rs | RunStatus enum; in-memory run store | enum 15-26 (Paused at 21); as_str 28-40 | clean | not read |
| rust/services/execution-core/src/grpc.rs; runtime_loop/agent.rs | RunStatus users (also browser_agent.rs, browser_events.rs) | RunStatus:: sites (grep) | M (+98; +207) | rust/services/execution-core/tests/zdr_refusal_inventory.rs (untracked) |
| rust/crates/mp-events/src/subjects.rs | run subject builders; event-name constants | run subject 72-75; RUN_EVENTS_WILDCARD 90-91; SUBJECT_APPROVAL 221; SUBJECT_RUN 223; EVENT_APPROVAL_STATE_CHANGED 229; EVENT_RUN_PAUSED_FOR_APPROVAL 232; EVENT_RUN_RESUMED_AFTER_APPROVAL 233; legacy 243-256 | clean | same file 389-526 (legacy translate tests at 458-459) |
| rust/crates/mp-orchestration/src/run_event.rs | Rust RunEvent enum; used only in crate lib.rs | RunPausedForApproval variant 33; struct variant 150 | clean | rust/crates/mp-orchestration/tests/proto_wire_parity.rs:160; src/proto_shim.rs:820 |

### 4b. Model Plane, Go

| path | role | lines | live | tests |
|---|---|---|---|---|
| go/services/capability-core/internal/runwatch/consumer.go | JetStream consumer; terminal map; Accept | RunEventSubject 64; stream 69; durable 76; terminal map 80-97; Run 163; handle 178; process 194; Request 226-232; idempotencyKey 253; payload 258; copy 267; runIDFrom 281 | clean | consumer_test.go: 11 tests (RunCompleted_NotifiesEachPendingWatcherAndMarksNotified 137; RunFailed_UsesFailedNotificationType 176; OneNotifyFailure_RetriesWholeMessageButNotDuplicates 212; ...) |
| go/services/capability-core/internal/runwatch/store.go | Postgres watcher store | PendingWatchers 28; MarkNotified 55 | clean | no store_test.go; fakeStore at consumer_test.go:19-55 |
| go/services/capability-core/internal/api/run_watchers.go | watcher HTTP handlers | file (no line cites) | clean | run_watchers_test.go: 9 tests (12-204; e.g. CreateBindsOwnerToVerifiedPrincipal 33, GetReturnsOnlyCallersOwnWatch 111) |
| go/services/capability-core/internal/notifyclient/client.go | HTTP client to notification-core | path const 75; Request.RetentionMode 104; Accept 146 | clean | client_test.go: 7 tests (27-220; Accept_SignsAndSendsExpectedRequest 100) |
| go/services/capability-core/migrations/0012_run_watch_subscriptions.up.sql | run_watch_subscriptions table | file | clean | none |
| go/services/orchestrator-core/internal/orchestration/types.go | event-type constants; subjects; EventTypeOf | EventTypeRunPausedForApproval 18; subject consts 22-28; NewRunPausedForApproval 107-115 (no caller); EventTypeOf 135-157 | clean | internal/orchestration/handlers_test.go (TestHandlers_* 142-290) |
| go/services/orchestrator-core/internal/orchestration/service.go | publisher to mp.v1.orchestration.* | NewService 43; Publish 52 (no production caller); raw publish 96 | clean | none |
| go/services/orchestrator-core/internal/orchestration/wire.go | builds Service and Handlers | 13-16 | clean | none |
| go/services/orchestrator-core/cmd/main.go | wiring | 305 (result discarded); compat wiring 197-217 | clean | none |
| go/services/orchestrator-core/cmd/activities/activities.go | run lifecycle publish | publishRunEvent 374-415 (publish 412); envelope zdr 401; RUN_COMPLETED 752; RUN_FAILED 764 | clean | not read |
| go/services/orchestrator-core/internal/compat/subscriber.go | legacy run events republished to v1 subject (gated by compatAdapterEnabled, cmd/main.go:197) | publish 111 (newSubject not re-read) | clean | not read |
| go/services/nats-provisioner/main.go | stream specs; consumer bindings | MP_ORCHESTRATION_EVENTS 36; MODEL_PLANE_RUN_EVENTS 45; run-watch binding 112-115; other bindings 170, 191, 206 | clean | main_test.go: run-watch binding assertion 40-41; skill-review 74-75; nats-provisioner *_test.go: TestTopologySpecsCoverRuntimeBindings (11), TestEnsureConsumer* (112, 219, 308) |
| go/services/nats-provisioner/publishers_registry_test.go | publisher ACL registry; gap rows | grants 70-73; registry rows 98-200 | clean | itself (contract test) |
| go/go.work | Go workspace (tracked) | use block: capability-core, nats-provisioner, orchestrator-core, pkg/* | clean | n/a |

### 4c. Model Plane, proto, ACL, docs

| path | role | lines | live | tests |
|---|---|---|---|---|
| proto/model_plane/v1/orchestration.proto | OrchestrationEvent; StreamRunEvents; enums | message 1010; RunPausedForApproval field 15 at 1036; rpc StreamRunEvents 113; enums PlanState 154, PlanStepState 178, ApprovalState 210, TodoState 224 | clean | none |
| proto/model_plane/v1/runs.proto | RunDetail; run_status fields | RunDetail 269, status 282-283; run_status in ResolveRunActionAuthorityResponse (219; field 235) and ResolveScheduledStepAuthorityResponse (251; field 265) | clean | none |
| proto/model_plane/v1/sessions.proto | ThreadSummary.latest_run_status | message 976; field 995 (live 1006) | M (+42: SessionRetentionService 196-206 live; block at 1219) | none |
| deploy/nats.conf | per-user NATS ACL | model-gateway publish 17 (run at 18); session-core publish 38 (run at 44); capability-core subscribe 83 (run at 88; DELIVER run_watch_notify 96); orchestrator-core publish 105, subscribe 106-107 (`mp.v1.orchestration.>` at 107); password env 103 | clean | scripts/tests/nats-runtime-principal-contract-test.sh (greps file; lines 73, 97) |
| scripts/gen-sse-event-taxonomy.py | SSE taxonomy generator | PRODUCER sse_events.rs 37; CONSUMER chat-client.ts 38; DOC 39; --check 182-183 | clean | CI gate (section 5) |
| docs/sse-event-taxonomy.md | generated taxonomy | file | M (+4) | CI gate |
| docs/capability-ownership-matrix.md | owner rules | 19; 26; 37; 42; 54-55; 68; 83-91 (subject divergence 88; correct fix 91) | clean | n/a |
| docs/decisions/ledger.md | watch primitive decisions | 4174-4200 (capability-core owns Watch; 4194 run_watch_subscriptions not deleted); 4258-4264 (S4.4 delivery states; runwatch keeps own path) | clean | n/a |

### 4d. Application Plane (notification-core)

| path | role | lines | live | tests |
|---|---|---|---|---|
| internal/http/handlers.go | POST /api/v1/notification-requests; allowlist | CreateNotificationRequest 130; isNotificationTypeAuthorized 199-218 (capability-core 211-214) | clean | internal/http/server_test.go: TestServicePrincipalsHaveExplicitNotificationTypeAllowlists 502; TestCreateNotificationRequestAcceptsCapabilityCoreRunNotification 535; TestCreateNotificationRequestRejectsCapabilityCoreOutsideAllowlist 590 |
| internal/notification/service.go | Accept; delivery; ZDR refusals | Accept 284; async ZDR refused 296-301; ZDR branch 337; feed skipped for ZDR 522; retention default 621-623; ZDR resolution 730 | clean | service_test.go (TestAcceptValidRequest 9 and others), service_flow_test.go, service_preferences_test.go, delivery_*_test.go |
| internal/notification/feed_projection.go | in-app feed projection | ZDR projection refused 133-136 | clean | feed_projection_test.go: TestFeedProjectionWorkerRefusesZDRProjection 152 |
| migrations/013_register_run_watch_notifications.up.sql / .down.sql | run-watch channel rows | 1-16 | clean | none |
| migrations/005_create_channel_configs.up.sql; 004_create_preferences.up.sql | channel matrix; preferences | not re-read | clean | service_preferences_test.go |
| README.md | retention_mode; Novu note | 14; 62-67 | clean | n/a |
| internal/consumers/conversation_followed_message.go | refs-only consumer template (cited by F-t1-008) | not re-read | clean | conversation_followed_message_test.go |

### 4e. Frontend Plane (apps/Frontend Plane/verevonv3)

| path | role | lines | live | tests |
|---|---|---|---|---|
| src/features/core/components/CoreSidebar.tsx | sidebar; chatRunStatusChip; ChatSidebarPanel | chip fn 872-897 (live 874-899); use 643 (live 644); ChatWorkLinks JSX in live | M (+2) | CoreSidebar.chat-sessions.test.tsx (describe 63; mail-task describe 371); CoreSidebar.home-tab.test.tsx (not read) |
| src/features/core/components/sidebar/ChatSidebarOrganizer.tsx | organizer; view filters | filter select 202 (live +1 shift) | M (+2) | none by name |
| src/features/core/components/sidebar/chat-sidebar-organizer.css | organizer styles | file | M (+5) | none |
| src/features/core/components/CoreNavbar.tsx | unread badges; navbar | counts 86-88; BadgeButton 650-655 | M (2 lines at 182) | CoreNavbar.test.tsx (names not read) |
| src/features/core/components/CoreShell.tsx | navbar resource; poll timer | 35; 59-82 | clean | none by name |
| src/features/core/lib/shell-data.ts | navbar data shaping | file | clean | not read |
| src/features/chat/lib/chat-organization.ts | status sets; selectOrganizedChats; date groups | 47-48; 49-62; 64-69 | clean | chat-organization.test.ts: 7 tests (15, 26, 33, 44, 57, 70, 78) |
| src/features/chat/lib/chat-thread-history.ts | ChatThreadHistoryItem.latestRunStatus | 53, 81; propagation 270-537 | clean | not enumerated |
| src/features/chat/components/RunNotifyButton.tsx | per-run notify toggle | state Map 51; MAX 50; toggle 106-126 | clean | none (no test file) |
| src/features/chat/components/use-chat-controller.ts | run status updates; ACTIVE_RUN_STATUSES use | latestRunStatus 672; active check 1095-1096; live +4 near 3283 | M (+4) | use-chat-controller.recovery.test.ts; use-chat-controller.plan-reload.test.ts; zdr-refusal test (untracked) |
| src/shared/api/chat-client.ts | thread normalizer; thread events | type 733 (live 750); normalizer 2295 (live 2348); output 2310 (live 2363); thread events path 2194 | M (+53) | chat-client.test.ts (M); chat-client.replay.test.ts; chat-client.served-by.test.ts |
| src/shared/api/runs-client.ts | run list, get, watch | listRuns 117; getRun 174; watchRun 198; unwatchRun 207; getRunWatchStatus 215 | clean | runs-client.test.ts |
| src/shared/api/run-console-client.ts | per-run SSE and replay client | events 394; replay 460 | clean | run-console-client.test.ts (path test ~74) |
| src/shared/api/sse.ts | SSE reader | Accept header 37; readSseStream | M (+1 feature field) | none by name |
| src/shared/api/notifications-client.ts | notifications REST | interfaces Notification 3, UnreadCount 14, NotificationPreference 18; listNotifications 24; getUnreadCount 28; markRead 32; markAllRead 38; deleteNotification 42; prefs 48, 52 | M (18 lines) | none |
| src/shared/actions/action-registry.ts | action contracts | notification zod inputs 677-699; entries 2222-2256 (mark_read 2222, mark_all_read 2233, delete 2244, update_preference 2255) | clean | none by name |
| src/shared/actions/chat-organization.ts | org state reducer (28 lines; no status logic) | file | clean | none |
| index.html | document head | 5-8 (favicon, preconnect, fonts) | clean | n/a |

### 4f. Frontend gateway (apps/Frontend Plane/verevonv3/apps/gateway/src/domains)

| path | role | lines | live | tests |
|---|---|---|---|---|
| run_watchers.rs | BFF watcher proxy | router 52-57; watch_run 68; unwatch_run 94; get_watch_status 120; watchers_url 144 | clean | none (no #[test]) |
| notifications.rs | BFF notification routes | router 17-30 | clean | none (no #[test]) |
| chat.rs | route registration | thread events 50; run events 74 | clean | n/a |
| chat/streams.rs | SSE proxy to model-gateway | upstream URL 197 | clean | n/a |
| chat/history.rs | thread summary struct | latest_run_status 64, 146-147, 407; test 938-951 | clean | in-file |
| navbar.rs | navbar payload (polled by CoreShell) | file | clean | n/a |

### 4g. CI and tooling

| path | role | lines | live | tests |
|---|---|---|---|---|
| .github/workflows/sse-taxonomy.yml | taxonomy freshness gate | paths 18-28; run --check 45; drift proof 49-56 | clean | itself |
| .github/workflows/notification-core-service.yml | notification-core CI | working dir 27-28; go mod download 39; go test -v ./... 42; go build 45 | clean | itself |
| .github/workflows/model-plane-rust-ci.yml | Rust CI | working dir 57; fmt 99; check 105, 112; test 119 | M (hunks not inspected) | itself |
| .github/workflows/verevonv3-ci.yml | frontend and gateway CI | gateway dir 113; fmt 133; clippy 138; test 141; frontend dir 148; pnpm verify 193 | M (+50 live: session-core-receipts job 121-170) | itself |

## 5. Related contracts

- Proto. No proto enum for run status (grep). RunDetail.status comment lists queued, running, completed, failed, cancelled, awaiting_approval (runs.proto:282); execution-core RunStatus also has `paused` (state.rs:21). A closed enum needs a home: candidates runs.proto RunDetail.status, sessions.proto ThreadSummary.latest_run_status (995), or a new enum in orchestration.proto.
- Rust subject contract: mp-events subjects.rs (72-91, 221-256); legacy translation tests 458-459.
- SQL migration directories (latest names):
  - session-core WT: 0043_audit_outbox_requeue.sql (preceded by 0042_thread_persona_toggle.sql, 0041_mail_task_thread_origin.sql, 0040_direct_run_completion_key.sql). Also 0016_outbox_run_lifecycle_subjects.sql.
  - session-core LIVE adds (untracked): 0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql, migrations/revert/. Next free number: 0044 (WT) / 0046 (LIVE).
  - notification-core (WT and LIVE): 014_register_mail_task_drafted.up.sql / .down.sql (013 has up and down).
  - capability-core (WT and LIVE): 0015_background_process_capability.up.sql / .down.sql. Run-watch table: 0012_run_watch_subscriptions.up.sql / .down.sql.
  - orchestrator-core (WT and LIVE): 0002_feedback_implicit_source.up.sql / .down.sql. No run or approval tables.
- Config and env: deploy/nats.conf (user ACL; password env MODEL_ORCHESTRATOR_CORE_NATS_PASSWORD at 103); deploy/docker-compose.yml:1293 (NATS_USER orchestrator-core-runtime); nats-provisioner main.go stream and consumer specs (36, 45, 112-115).
- SSE event types. ChatEvent in sse_events.rs (producer). Orchestration SSE names from sse.rs orchestration_event_to_sse (WT 9849-9869): plan_transitioned, todo_transitioned, approval_state_changed, subagent_attached, subagent_stopped, run_paused_for_approval, run_resumed_after_approval, browser_* names. Taxonomy doc generated from sse_events.rs and consumed by chat-client.ts (gen script 37-38). CI gate sse-taxonomy.yml:45.
- Gateway routes:
  - GET, POST, DELETE /api/v1/runs/{run_id}/watchers: run_watchers.rs:52-57 -> model-gateway http_routes.rs:396-399 -> capability-core.
  - /api/v1/notifications, /preferences, /unread/count, /mark-all-read, /{id}/read, /{id}: notifications.rs:17-30.
  - /api/v1/runs/{run_id}/events and /events/replay: chat.rs:74; streams.rs:197; model-gateway http_routes.rs:218-220.
  - /api/v1/chat/threads/{thread_id}/events: chat.rs:50.
  - notification-core canonical POST /api/v1/notification-requests: notifyclient client.go:75; master-ownership-matrix.md:167.
- Action contracts (apps/Frontend Plane/verevonv3/src/shared/actions). notifications.mark_read, mark_all_read, delete, update_preference (action-registry.ts 2222-2255; zod 677-699). No contract for run watch, run status or attention. Root CLAUDE.md requires an action contract before a meaningful UI operation; RunNotifyButton calls runs-client directly.
- Context packs (src/shared/context-packs): not inspected.

## 6. Test and build commands

- Rust (apps/Model Plane/rust, Cargo workspace; members include crates/mp-events, services/model-gateway, services/session-core, services/execution-core; mp-orchestration membership NOT checked).
  - CI (model-plane-rust-ci.yml:99-119): `cargo fmt --all -- --check`; `cargo check --workspace`; `cargo check --workspace --all-targets`; `cargo test --workspace --no-fail-fast`.
  - Per crate (apps/Model Plane/CLAUDE.md): `cargo test -q -p session-core --all-targets`; same for model-gateway, execution-core, mp-events.
  - User memory: two cargo targets fail on Windows; use `--lib`.
- Go (per service directory; apps/Model Plane/CLAUDE.md: `go test ./...` and `go vet ./...`):
  - capability-core (module github.com/triodelab/model-plane/services/capability-core): `cd "apps/Model Plane/go/services/capability-core" && go test ./...`.
  - orchestrator-core and nats-provisioner: same pattern (module github.com/triodelab/model-plane/services/<name>).
  - User memory: Go -race needs cgo (unavailable on this host); use GOWORK=off for new dependencies.
- NATS ACL contract (not run): `bash "apps/Model Plane/scripts/tests/nats-runtime-principal-contract-test.sh"`.
- SSE taxonomy (not run): `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (sse-taxonomy.yml:45).
- Application Plane notification-core (module github.com/I-Dacosta/AquatiqCMS/apps/notification-core): `cd "apps/Application Plane/notification-core" && go mod download && go test -v ./... && go build -v ./cmd/server` (notification-core-service.yml:27-45).
- Frontend (apps/Frontend Plane/verevonv3; package.json scripts): `pnpm lint` (eslint .); `pnpm typecheck` (tsc -b); `pnpm test` (four node --test gateway scripts, then vitest run); `pnpm build`; `pnpm verify` (lint, typecheck, test, build; verevonv3-ci.yml:193); `pnpm test:e2e` (playwright test; tests/e2e/*.spec.ts). Targeted: `pnpm exec vitest run <file>`.
  - User memory: pre-existing typecheck, lint and vitest failures exist in worktrees; :5173 is the main checkout, so start a second dev server.
- Gateway (apps/Frontend Plane/verevonv3/apps/gateway; crate verevon-gateway-rs): `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test` (verevonv3-ci.yml:133-141). CLAUDE.md form: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` from the verevonv3 root.
- No tests were executed in this sweep.

## 7. Owners

- Run lifecycle, run-state rows and run events: Model Plane, session-core. capability-ownership-matrix.md:54 ("Run metadata + lifecycle | session-core | ... | execution-core transitions | NATS run-events"); master-ownership-matrix.md:126; root CLAUDE.md ("Model Plane owns ... sessions/runs").
- Approvals and plan events: CONFLICT. capability-ownership-matrix.md:55 assigns approvals to session-core (orchestration_store.rs). master-ownership-matrix.md:129 assigns "approvals" to orchestrator-core. apps/Model Plane/CLAUDE.md names capability-ownership-matrix.md the authoritative de-duplication reference, so session-core applies until a ruling changes it. NOT DETERMINED without a ruling.
- Gateway caches (coordinator.rs, approvals.rs): ephemeral. Must publish mutations to NATS for the owner (capability-ownership-matrix.md:42; fix at 91: mp.v1.orchestration.{plan,approval} plus session-core consumer).
- Run-watch record and run-event notification producer: capability-core. ledger.md:4178-4179 (capability-core owns the Watch); 4194 (run_watch_subscriptions not deleted). Delivery-state owner: S4.4, not built (ledger.md:4258-4264).
- Notification request API, delivery ledgers, realtime projections and notifications: Application Plane. master-ownership-matrix.md:12, :31, :167; root CLAUDE.md. Rule 10 (:166): projections only narrow access.
- Push subscriptions and per-user stream: the card assigns Application Plane (consistent with master :31). No existing per-user channel in Application Plane was found. NOT DETERMINED beyond that.
- PWA, sidebar, shell presentation, BFF normalisation: Frontend Plane. Root CLAUDE.md; master-ownership-matrix.md:20 ("App Shell presents"), :164 (rule 8: UX is App/Shell-owned).
- NATS publish and subscribe grants: Model Plane (deploy/nats.conf plus nats-provisioner registry; publishers_registry_test.go is the ACL contract).
- ZDR propagation: root CLAUDE.md ("Zero Data Retention must propagate through any content-persisting boundary"); audit 1712 lists F-t3-015 among ZDR items.

## 8. Uncommitted-only (LIVE tree)

Only in LIVE (untracked, ??):
- apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql; 0045_zdr_waived_runs_org_index.sql; migrations/revert/. Affects next migration number.
- apps/Model Plane/rust/services/session-core/src/retention_receipt.rs. Live terminalization.rs:682 calls `retention_receipt::enqueue_run_started_zdr_waived` (grep). Same start and terminal path as WS16, but ZDR receipts, not attention.
- apps/Frontend Plane/verevonv3/src/features/core/components/sidebar/ChatWorkLinks.tsx. Links to Findings, Reports, Routines. Not attention. Imported by CoreSidebar.tsx and ChatSidebarOrganizer.tsx (both M).
- apps/Frontend Plane/verevonv3/src/shared/actions/chat-work-period.ts (+ .test.ts). Period context for chat turns. Not attention.
- apps/Frontend Plane/verevonv3/src/features/chat/lib/prompt-suggestions.ts (+ .test.ts). Not attention.
- ZDR refusal work: use-chat-controller.zdr-refusal.test.ts; chat-client.zdr-refusal.test.ts; execution-core/tests/zdr_refusal_inventory.rs; model-gateway src zdr_features.rs, zdr_posture.rs, relevance_calibration.rs (+ calibration/); tests zdr_feature_table.rs, zdr_posture_matrix.rs, zdr_refusal_inventory.rs.

Modified in LIVE (relevant to WS16):
- CoreSidebar.tsx +2: `ChatWorkLinks` import (hunk 76-78) and `<Show when={!chatAppearanceActive('organizer')}><ChatWorkLinks /></Show>` (hunk 688-690).
- CoreNavbar.tsx 2: logo src uses BASE_URL (line 182). Badge code unchanged.
- ChatSidebarOrganizer.tsx +2: import (line 9) and `<ChatWorkLinks />` (line 212).
- chat-sidebar-organizer.css +5.
- use-chat-controller.ts +4: zdr refusal `feature` field near 3283-3295. ACTIVE_RUN_STATUSES use (1095-1096) unchanged.
- notifications-client.ts 18: Notification shape moved to snake_case (organization_id, recipient_id, event_type, channel, archived, submitted_at, cta_href, payload); listNotifications takes AbortSignal. This is a live-only contract drift against notification-core fields.
- chat-client.ts +53 and chat-client.test.ts +22: zdr feature fields; normalizer shifts to 2348.
- sse.ts +1: ApiError carries feature.
- home-updates.ts 2: feed enum 'aquatiq' -> 'workspace'.
- sse-event-taxonomy.md +4 (generator output).
- sessions.proto +42: new SessionRetentionService (CountWaivedRuns) at 196-206 and a block near 1219.
- session-core grpc.rs +861 (net, incl. retention receipts; latest_run SQL moved to 5555); learning_events.rs +77 (enqueue moved to 208); terminalization.rs +123 (enqueue at 1186); orchestration_grpc.rs +249 (create_approval 2509; stream 3239); orchestration_store.rs +8.
- model-gateway sse.rs +883 (run_events_sse 7533); http_routes.rs +237 (watcher proxies 5750-5801); sse_events.rs +203 (no run-state variant added).
- execution-core grpc.rs +98; runtime_loop/agent.rs +207.
- .github/workflows/model-plane-rust-ci.yml (M; hunks not inspected); verevonv3-ci.yml (M; +50 session-core-receipts job at 121-170).

Clean in LIVE (identical to HEAD): all notification-core; capability-core runwatch, api, notifyclient, migrations; nats-provisioner; orchestrator-core; deploy/nats.conf; runs.proto; orchestration.proto; mp-events; mp-orchestration; execution-core state.rs; session-core audit_publisher.rs, approval_delivery.rs, orchestration_nats.rs, migration 0016; model-gateway approvals.rs; RunNotifyButton.tsx; chat-organization.ts (both); runs-client.ts; run-console-client.ts; CoreShell.tsx; shell-data.ts; index.html; gateway run_watchers.rs, notifications.rs, chat.rs, chat/streams.rs, chat/history.rs, navbar.rs; sse-taxonomy.yml; notification-core-service.yml; go.work.

## 9. Sibling reconcile (F-t1-008, F-t1-009) and dependent items

Sources: 01-verevon-space-audit.md cards 452-482; table rows 84-85; cross-room note 72; realtime ZDR note 107; push dependency 879.

| Topic | WS16 (F-t3-015) | Sibling (F-t1-008 / F-t1-009) | Reconcile action |
|---|---|---|---|
| Producer allowlist | capability-core run types in notification-core handlers.go:199-218 | F-t1-008: per-service, per-type allowlist for Space signals | One allowlist in handlers.go; add types there |
| Signal vocabulary | run states: working, needs_approval, needs_input, plan_ready, completed, failed, archived | signals: mention, reply, approval_needed, agent_done, agent_awaiting, invite | One closed taxonomy owned by Model Plane; map sibling names onto it (approval_needed vs needs_approval; agent_done vs completed) |
| Per-user stream | "per-user shell stream" replacing 60/180 s navbar polling (CoreShell.tsx:59-82) | F-t1-009: server-side unread/working aggregate, polled then pushed (card 474, 482); F-t1-008 content-free wake push (107) | One per-user aggregate stream; F-t1-009 aggregate becomes a payload of it |
| Badges | tab badge; sidebar counts | CoreNavbar unread (86-88); F-t1-009 "defined badge semantics", double-count risk (482) | One counter definition shared by tab badge, navbar and Home |
| Sidebar | sections Waiting for you / Working / Unseen results / Settled | F-t1-009 Home "needs you" inbox; Agents and Activity destinations | Both reshape CoreSidebar.tsx; sequence them and agree the section model |
| Working state | run states on thread rows | CoreSidebarSpacesPanel isSpaceWorking (252); F-t1-009 per-Space latest-activity summary | One working definition |
| ZDR | content-free on all threads; notification-core refuses ZDR async (296-301) and ZDR feed (feed_projection 133-136) | F-t1-009: in-app feed is not a ZDR-safe store; use an Application attention projection of refs only (482); F-t1-008 consistent (462-466) | Attention list from session-core rows plus Application projection of refs; notification-core only for out-of-app delivery |
| Dependencies | F-t3-016, F-t3-012 (audit 1565) | F-t1-004 (run state); F-t1-003 (push) | Confirm owners before W2 start |

Dependent items in the same audit:
- F-t3-011 (ask_user needs_input) depends on F-t3-015 attention events (audit 514). The needs_input producer does not exist today (section 3, E).
- F-t3-049 (audit 1008) derives one thread status with fixed precedence and an unseen-completion state. This overlaps the WS16 closed enum and "Unseen results". One owner must be named.
- F-t3-018 (audit 602-610) off-surface approval via Application Plane delivery overlaps the needs_approval notification path.
- F-t3-012 (session-core conversation foundation) and F-t3-016 (shared run control) are the card's stated dependencies (audit 1565, 1705).

## 10. NOT DETERMINED

1. Whether ZDR-flagged run events can reach the run-watch consumer (section 3, H). The orchestrator drops RetentionZeroData at publish (activities.go comment above 374); session-core learning envelopes carry zdr (learning_events.rs:29-30 doc). Not traced further.
2. Dedup behaviour for two RUN_COMPLETED publishers (session-core outbox and orchestrator-core activities.go:752) on mp.v1.run.*.event. Idempotency key shape known (consumer.go:253; client_test.go:28); whether both carry the same key is not verified.
3. The compat adapter's republish target (orchestrator-core internal/compat/subscriber.go:111, newSubject not re-read). Gated by compatAdapterEnabled (cmd/main.go:197).
4. Approval ownership conflict: session-core (capability-ownership-matrix.md:55) vs orchestrator-core (master-ownership-matrix.md:129). No ruling found.
5. Whether `apps/Model Plane/go/go.work` is the active build mode (user memory says "no go.work"). File is tracked in WT and LIVE.
6. Whether mp-orchestration is a member of the Rust workspace (members grep did not list it).
7. Live CI changes in model-plane-rust-ci.yml and verevonv3-ci.yml beyond the Z-17 job (hunks not inspected).
8. Live line for http_routes.rs:220 and chat.rs:74 (not re-read in LIVE; file is M or clean respectively).
9. Runtime delivery of session-core's in-process RunPausedForApproval to the browser (code path verified; no stack run).
10. Whether any consumer reads `run_paused_for_approval` from SSE into a UI state: the frontend SSE consumer for run events (run-console-client.ts) was not traced to the event names.
11. Whether a per-user shell stream belongs in Application Plane (card) or in the Frontend gateway BFF (no decision recorded in code or ledger).
12. Test pass or fail status for any affected suite (no tests run).
13. Content of notificationCopy and payload builders (consumer.go:258-279) beyond the grep-observed "run completed" and "run failed" copy.
14. Whether chat-thread-history.ts and CoreNavbar.test.tsx cover status rendering (not enumerated).
