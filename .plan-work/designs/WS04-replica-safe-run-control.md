# WS04-replica-safe-run-control - Multi-replica-safe run control

Abbreviations: `MP` = `apps/Model Plane`; `MG` = `MP/rust/services/model-gateway/src`; `SC` = `MP/rust/services/session-core`; `FE` = `apps/Frontend Plane/verevonv3`; `BFF` = `FE/apps/gateway/src`. Unless marked `L:` (live tree `C:/dev/CoresSystem`), every `path:line` was opened in the worktree (HEAD afb83928). `[live-only]` = exists only as uncommitted work in the live tree.

## 1. Scope, items resolved, owner

**Item:** F-t3-016 (Recommended, wave W1, audit effort M). Dependency of F-t3-002 (WS03), 011 (WS14), 012 (WS17), 015 (WS16), 064.

**Owner:** Model Plane. model-gateway (Rust) owns hot-path cancel, queue, resume liveness and turn records. session-core owns anything durable (grants, message receipts). Frontend Plane owns only the queue strip, the send-id mint and the BFF pass-through. Control Plane is unchanged: the per-message `thread:append` decision the BFF mints stays as is (`BFF/domains/chat/json_handlers.rs:176-220`).

| Slice | Tag | In |
|---|---|---|
| S1 | [BUILD] | Run-control bus seam on NATS core request/reply, ACL, two-replica proof profile (dark, flag off) |
| S2 | [BUILD] | Shared stream cancel and durable-cancel fan-out to the owning replica |
| S3 | [BUILD] | Shared queued input (owner-routed) and cross-replica producer liveness for resume |
| S4 | [BUILD] | Durable plan-approval grant and approved-plan link in session-core (replaces the in-memory thread grants) |
| S5 | [BUILD] | Shared turn record (request_id to run_id and skill ids, ids only) for ratings |
| S6 | [BUILD] | Durable send receipts: `client_message_id` dedupe in session-core, SPA mint, queue receipt ids, end-of-run reconcile |
| S7 | [BUILD] | Shared idempotency claim and attach-to-original for retried sends (flagged, last) |
| S8 | [BUILD, decision-gated] | Queue tray controls (retract/edit). Not scheduled until D4 |
| S9 | [FINISH] | Flip defaults, docs and ledger, two-replica evidence run |

**Explicitly OUT:**
- Execution-core's per-process `StateStore` and making the Do loop observe cancel (WS03). Observed, not owned: the gateway durable cancel calls only `RunService.CancelRun` (`MG/http_routes.rs:1639`; execution-core `CancelRun` is called only from `MG/browser_run.rs:725`), and no `runtime_loop` file reads the cancel latch (grep of `is_cancelled|RunStatus::Cancelled|cancel_requested` over `execution-core/src/runtime_loop`, `grpc.rs`, `state.rs`: only `state.rs:197` sets it).
- Registering `agentic_run_stream` (`MG/sse.rs:9167`) with the cancel/queue registries. It has no registration, no queue and no stream buffer today (no `cancel|queued_inputs|stream_buffers|buffer_key` match in `sse.rs` 9167-9800). Registering only the relay would "stop" a relay while the loop keeps running, which is a dishonest Stop. WS03 `/continue` assumes it registers; that assumption is wrong at HEAD (see section 5).
- `TeamWorkerStore` (`MG/coordinator.rs:401`): no caller outside `coordinator.rs` and `grpc.rs` (grep of rust, go, FE). Matrix row 70 sends team lifecycle to orchestrator-core. Left per-replica and documented.
- `ApprovalStore` (`state.rs:264`, WS13), `artifact_versions`, `tool_results` (WS25 F-t3-063), `PreviousTurnRegistry` (holds up to 4096 bytes of user text per turn, `chat_turn_registry.rs:251`; deliberately NOT shared, privacy).
- Editable queue and split send button: gated (D4). Editing a persisted message needs message versions (WS17).
- Provider-side or browser-run cancel (WS21/WS22).

## 2. Verified current state

### 2a. What exists (worktree)

| Concern | Evidence | Multi-replica behaviour today |
|---|---|---|
| Stream cancel | `MG/cancel_registry.rs:47` `register_run`, `:76` `cancel_run_for`, `:92` `cancel_for`, `:112` `finish`, `:126` `is_live_for`, `:133` `is_cancelled`; route `MG/http_routes.rs:140`; handler `:6873` | `DashMap<request_id, AtomicBool>` in one process. A Stop landing on another replica answers 404, and the SPA has already aborted its fetch (`FE/src/features/chat/components/use-chat-controller.ts:2501-2510, 2952`), so the producer "detaches ... finishing for resume" and runs to the end (`MG/sse.rs:6477`, also `:3118`): wasted tokens, answer saved onto a run the user stopped |
| Durable cancel | `MG/http_routes.rs:1602` `cancel_run` -> `run_client.cancel_run` `:1639` -> local-only `cancel_run_for` `:1655` | `live_stream` is true only when the stream is in this process; the doc says another replica "relies on its own liveness check before the save" |
| Hot-path polls | `MG/sse.rs:6485, 6641`; `MG/tool_loop.rs:6687, 6987, 7060` (50 ms poll of a sync bool) | Must stay a sync local read; the bus must set the local flag, not replace the read |
| Queued input | `MG/queued_input.rs:121` register, `:149` `thread_for`, `:163` `enqueue_for`, `:214` `drain`, `:226` `finish`; bounds `:62` (2000 chars), `:66` (3 msgs); single drain site `MG/tool_loop.rs:6528`; handler `MG/http_routes.rs:7308` | Per-process. A queue POST on another replica is 404 -> BFF turns it into `{queued:false, resend_as_new_turn:true}` (`BFF/domains/chat/json_handlers.rs:228-`) -> SPA resends as a new turn while the run is still working (`use-chat-controller.ts:2239`) |
| Undelivered queue items | `queued_input.rs:226` `finish` drops them; no code handles leftovers (grep `undelivered|leftover` in `queued_input.rs, sse.rs, tool_loop.rs, http_routes.rs`) | An item accepted (202, persisted) after the last round boundary is never read by the model and the strip stays "venter" (`ChatMessages.tsx` `QueuedInputStrip` ~2105). Whether every streaming turn reaches the drain (`run_tool_rounds_for_model`, `sse.rs:2412`) is not determined |
| Enqueue/persist order | Ledger 2026-08-24 (`MP/docs/decisions/ledger.md:1663-1666`, "Persist before deliver") and handler doc `http_routes.rs:7289-7303` vs handler body: `enqueue_for` at ~7351, then `append_queued_user_message` (`MG/session_flow.rs:1602`) | Code enqueues first, persists after (`persisted:false` is reported). Doc and ledger say the reverse. A code/ledger contradiction (D3) |
| Idempotency | `MG/idempotency_registry.rs:161` `claim`, DONE_TTL 600 s `:57`, PENDING_TTL 120 s `:61`, 1024 entries `:69`; claim in `MG/sse.rs:1180`, unary `MG/http_routes.rs:~8915` | Per-process, caches the answer text (<=64 KiB). **The SPA never sends `idempotency_key`** (no match in `FE/src/features/chat`, `FE/src/shared/api/chat-client.ts`; body builder `chat-client.ts:1090-1135`), so this registry is dormant for the product |
| Durable dedupe | `SC/migrations/0002_events_and_ordinals.sql:35-38` unique `(org_id, idempotency_key)` on **events** | Protects step and terminal events (`SC/src/grpc.rs:3228-3303`), NOT messages. `append_message_in_tx` (`SC/src/grpc.rs:2430-2670`) inserts unconditionally (`:2569`), and its MESSAGE_APPENDED idempotency hash derives from a fresh ULID (`:2440, :2596`). `AppendMessageRequest.idempotency_key = 16` (`MP/proto/model_plane/v1/sessions.proto:303`) is a Control Space-decision field. **The audit's "reuse session-core idempotency for durable receipts" is wrong for user messages** |
| Resume liveness | `MG/sse.rs:7196` `invoke_resume_sse`, `:7230-7234` `producer_live` = local `is_live_for`, used at `:7234` and `:7350`; tail limits `TAIL_INACTIVITY_LIMIT` 120 s `:7338`, `TAIL_MAX` 15 min `:7339` | A resume on replica B for a producer on A closes after 120 s without frames and answers 404 when nothing is buffered yet |
| Stream buffer | `MG/stream_buffer.rs:9-14, 33, 36, 288, 528, 543`; `MG/state.rs:778, 827` | Already Redis-backed with a 250 ms breaker; cache-mode Dragonfly, 256 MB shared by three services (`MP/deploy/docker-compose.yml:226-241`, `:117-118`) |
| ZDR vs buffer | `MG/sse.rs:976` `if effective_zdr` -> `zdr_direct_stream` (`:1069`, def `:8107`) returns before `stream_buffers` (`:1152`), `claim` (`:1180`), `register_run` (`:1978`); `MP/docs/ZDR.md:96` ("no run, thread, message, event, buffer, idempotency record") | **Resolves the sweep's "not determined": ZDR turns never buffer, claim or register.** Waived turns are ordinary persistent runs |
| Plan mode | `MG/coordinator.rs:69` `PlanModeStore` = run flags (`enter/exit/is_plan_mode :135`) + thread-keyed grants (`record_grant :166`, `take_approved_plan :197`, `granted_rung :208`); readers `MG/sse.rs:8491` (`approved_plan_block_for`), `:8663` (autonomy rung into `RunAgentRequest`); writer `coordinator.rs:327` via `plan_approval` (`http_routes.rs:7041`) | Flag half: the only RPC consumers are gateway's own handlers (`grpc.rs:1381`); no caller in rust/go; durable truth is already `runs.mode` (`SetRunMode`, `SC/src/grpc.rs:4984`; `mark_run_plan_mode` `sse.rs:6154`). **Grant half is the hazard**: approve on A, next Do run on B reads nothing, rung falls to UNSPECIFIED (`sse.rs:8663`, fail-open for the graded ladder by the ledger's own admission, `ledger.md:2611`), and the plan link is lost. Also `handle_exit_plan_mode` publishes `RUN_PLAN_MODE_EXITED` only `if was_active` (`coordinator.rs:329`), so approval on another replica skips the grant's audit event and answers `was_in_plan_mode:false` |
| Durable grant data | `SC/src/grpc.rs:5015-5070`: `SetRunMode` merges `autonomy_rung`, `autonomy_justification` into `runs.metadata` of the PLAN run only | Not readable by thread. `RunDetail` has `mode = 6` and `metadata = 16` (`MP/proto/model_plane/v1/runs.proto:286, 306`); `plan_approval` already calls `GetRun` for the thread id |
| Turn record | `MG/chat_turn_registry.rs:45` (TURN_TTL 24 h, 20k cap), `global()` `:175`, `PreviousTurnRegistry` `:255`, `record_chat_turn` `:186` called `sse.rs:1768`, read `http_routes.rs:6412, 6476` | Per-replica; a rating on another replica answers 409 `turn_not_rateable` unless the client names `run_id` (then skill attribution is lost). `global()` and `PreviousTurnRegistry` exist at HEAD (the sweep says live-only; wrong) |
| NATS ACL | `MP/deploy/nats.conf:14-35` model-gateway-runtime: publish `mp.v1.run.*.event`, `mp.v1.stream.opened/closed`, `mp.v1.usage.*`, `mp.v1.ingress.accepted`, `mp.v1.orchestration.run`, `mp.v1.feedback.rated`, `mp.v1.finetune.>`; subscribe `dataplane.documents.indexed`, `mp.v1.capability.>`, `_INBOX.MODEL_GATEWAY_RUNTIME.>` | No runctl subject allowed in either direction. A new subject tree needs an ACL change (the ACL ships in the release artifact, `MP/scripts/release-artifact.sh:92`). Gateway cannot subscribe `mp.v1.run.*.event`, so the existing `RUN_CANCEL_REQUESTED` publish (`http_routes.rs:~1668`, "nothing in the repository acts on it") cannot be reused |
| NATS client | `async-nats 0.49.1`, `redis 0.27.6` already in `MP/rust/Cargo.toml:73,79`; connector `MG/nats_connection.rs` (custom inbox prefix `_INBOX.MODEL_GATEWAY_RUNTIME`); `NatsPublisher` keeps its client private (`MG/nats_publisher.rs:13-15`); other consumers dial their own connection (`MG/main.rs:67, 74`) | No new dependency needed |
| Replica shape | `MP/deploy/docker-compose.yml:190-260`: host ports `127.0.0.1:8080/9090`, `stop_grace_period 150s`, drain 120 s (`MG/drain.rs:46`; waits on `cancels.active()` `:134, 148`); no `replicas` key anywhere under `MP/deploy`; opt-in overlay precedent `MODEL_PLANE_BRIDGES` (`MP/scripts/compose.sh:122-123`), forbidden in artifact mode (`:35`) | The drain comment "listener stays open for resume and stop" assumes the stop reaches the draining replica, true only with one replica or affinity |

### 2b. Live-only work this stream collides with (land before S2)

| Live path | State | Why it matters |
|---|---|---|
| `MG/zdr_posture.rs` | untracked | `invoke_queue_input`, `plan_approval`, `cancel_run` appear in `DURABLE_MUTATIONS` (`L:MG/zdr_posture.rs:825-840`); `L:MG/http_routes.rs:7416+` refuses ZDR and waived tokens first via `require_non_zdr_durable_mutation`; `L:http_routes.rs:~1606+` `cancel_run` uses `continuation_gate`. At HEAD neither gate exists. **S3 must not carry queue text on NATS until this gate is committed** |
| `MG/http_routes.rs` (+190/-47), `sse.rs` (+814/-69), `sse_events.rs` (+202), `tool_loop.rs` (+370/-55), `state.rs`, `lib.rs`, `main.rs`, `chat_turn_registry.rs` (+36), `session_flow.rs`, `grpc.rs` | modified | Cited symbols moved: `invoke_cancel` 6873->6979, `invoke_queue_input` 7308->7416, `cancel_run` 1602->1606, register 1978->2088, drain 6528->6635, `QueuedInput` 318->360 |
| `SC/src/grpc.rs` (+817/-44), `MP/proto/.../sessions.proto` (+42), `SC/migrations/0044, 0045` + `revert/` | modified / untracked | Number collision for new migrations (HEAD next 0044, live next 0046). The integrator assigns numbers. `buf breaking` outcome against `proto/baseline.binpb` is not determined for the live proto |
| `FE` `chat-client.ts` (+53), `ChatPage.tsx`, `ChatMessages.tsx` (+185/-44), `chat-types.ts`, `use-chat-controller.ts`, `chat-client.test.ts` | modified | S6 edits the same files; rebase on the live set |
| `MG/tests/zdr_posture_matrix.rs` + fixtures; `MP/api/contracts/zdr-posture-matrix.json` is committed | untracked / clean | Rust matrix test is live-only |

**Rule:** the live ZDR + session-core wave is committed first (integrator order in `overlap-space.md` section D.1). S1 is almost all new files and may start in parallel; S2 onward waits.

### 2c. Other per-process stores in `AppState` (audit completeness)

`doc_ready` (best-effort optimisation, documented), `trajectories` (ring buffer, durable path is NATS), `artifact_versions`, `tool_results`, `approvals`, `team_workers`: not run control; owners named in section 1. Not audited for rehydration here.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| D1 | Transport for cancel and queue | (a) NATS core request/reply routed to the owning replica; (b) Redis state (flag key polled, queue list); (c) sticky routing by request_id at the BFF/LB | **(a).** Matches the in-code promotion notes (`cancel_registry.rs:7-9`, `queued_input.rs` "# Shape") and the audit. No message content stored anywhere (b would put mid-run text in cache-mode Dragonfly that can evict and, per the compose note, snapshot). (c) is deployment-specific, breaks on replica loss, and does not help the durable-cancel fan-out; compatible as an optimisation | S1-S3 | Yes (audit already recommends it). Needs an ops sign-off on the ACL change (WS01) |
| D2 | Down-behaviour when the bus fails | 404 vs 503 vs local-only | Local hit wins. Local miss + authoritative no-responder = 404 (today's body). Bus error/timeout = **503 `run_control_unavailable`, retryable**, never a fake 202/404. Boot self-test; `/readyz` reports `run_control: ok\|degraded` | S1, S2, S3 | Yes |
| D3 | Persist-before-deliver (ledger 2026-08-24) vs code (enqueue first) | (a) amend the ledger to match code; (b) reorder to persist first | Keep code order through S3 (no behaviour change), record the contradiction as a ledger correction in S1. Reorder in S6 only, because persist-first plus "run ended -> resend as new turn" duplicates the message unless S6's dedupe exists. Must be explicit: this reverses the *code*, not the ledger | S6 only | Yes for S1-S5 |
| D4 | Editable queue and split send button | (a) drop (audit); (b) retract/edit a pending item, needs a `withdrawn` marker on the persisted message (WS17 message versions); (c) client-only "Stop and send" composed from existing `cancelInvocation` + send, no server change; conflicts with Space C10 (`overlap-space.md:62`, member steering, editable chips) | Owner ruling needed. Recommend (c) now (outside this stream, trivial in the SPA) and (b) behind WS17. Persist-at-delivery is infeasible: the Space `thread:append` token is content-bound and short-lived, and the session bearer cannot be held across a long turn (see the `CompleteDirectRunWithMessage` comment above `sessions.proto:193`) | S8 only | Yes (S8 is not scheduled) |
| D5 | Plan-grant carrier | (a) session-core columns on `threads` + read RPC; (b) Redis thread-keyed | **(a).** The grant widens authority; a cache-mode eviction silently downgrades to UNSPECIFIED. Matches ledger `ledger.md:2611/2664` ("durable carrier belongs in session-core") and matrix section 4.1 (gateway stores = latency caches with read-through). Additive ledger entry | S4 | Yes |
| D6 | Scope of "M" | S1-S3+S5 (cancel, queue, liveness, turn) vs plus S4/S6/S7 (grants, receipts, claim) | Core = S1-S3+S5. S4, S6, S7 each independently schedulable; S6/S7 exist because the audit's "reuse session-core idempotency" premise is false (2a). Integrator re-estimates F-t3-016 at L/XL if all included | planning | Yes |
| D7 | Redis/Dragonfly in every deploy | Operator fact (audit 10.3) | WS04 uses Redis only best-effort (S5, S7) with documented degrade, never for authority. Capacity note for the broker-grant store decision in `overlap-quarry.md` | S5, S7 | Yes |
| D8 | When replicas become real (`deploy.replicas` in production overlay) | WS01 | Not a WS04 decision; S1 ships the dev overlay only (not allowed in artifact mode) | S9 | Yes |

Ledger: no entry on replicas exists (grep). New entries added by this stream: "Run control is routed to the owning replica over NATS request/reply (not shared state)" (S1); correction "queue persists after enqueue today" (S1); "Plan grants live in session-core" (S4).

## 4. Slices

Test commands (repo): `cd "MP/rust" && cargo test -q -p model-gateway --all-targets`; `cargo test -q -p session-core --all-targets`; `cargo test -q -p mp-events -p mp-contracts --all-targets`; `cargo fmt --all -- --check` (CI `model-plane-rust-ci.yml:99`); real-PG tests are `#[ignore]` with `DATABASE_URL` (pattern `SC/src/grpc.rs:9338`); proto `buf generate` in `MP/proto` (Go bindings are checked in under `MP/go/gen/model_plane/v1/sessions*.pb.go`; buf is not installed on this host [U]) and `buf lint`/`buf breaking ... --against baseline.binpb` (`.github/workflows/buf.yml:21,23`); SPA `cd FE && pnpm vitest run <files>`, `pnpm typecheck`, `pnpm lint`; BFF `cargo test --manifest-path "FE/apps/gateway/Cargo.toml" --all-targets`; SSE `python3 "MP/scripts/gen-sse-event-taxonomy.py" --check`; ZDR `bash scripts/tests/zdr-contract-sync.test.sh`. Per the build memory, two cargo targets fail pre-existing on Windows (use `--lib`); not re-verified here.

### S1 - Run-control bus seam, ACL, two-replica profile [BUILD], dark

**Goal.** One small module that can reach "the replica that owns request R" and be tested without a broker, with the flag off by default so nothing changes.

**Files**
- `MG/run_control.rs` NEW. `RunControl` (Clone, in `AppState`) with `enum Transport { Off, Loopback(Arc<LoopbackHub>), Nats(Box<NatsTransport>) }` (the repo's `StreamBufferStore` enum pattern, `stream_buffer.rs:528`; the second real backend is the in-process loopback used by dev and tests). Serde types `RunControlMsg {v, op: cancel|queue|probe, org_id, user_id, request_id|run_id, op_id, text?, claimed_thread_id?}` and `RunControlReply`. `subject_token()` accepts `[A-Za-z0-9_-]{1,64}` only (request and run ids come from URL paths; refuse `.`, `*`, `>`, whitespace). `RemoteListener` (Drop aborts the task). `request(subject, msg)` with timeout `MODEL_GATEWAY_RUN_CONTROL_TIMEOUT_MS` (default 750, clamp 100-5000), no automatic retry. Reply is only ever sent by the owner; non-owners never subscribe (per-stream subscription, see S2), so no-responders = authoritative "no owner".
- `MG/lib.rs` add `pub mod run_control;`. `MG/state.rs` add field beside `cancels/queued_inputs` (`:239-247`); build in the NATS branch (`:736`) using `nats_connection::connect` when flag is `on`, in the dev branch (`:~820`) as `Off`/`Loopback`.
- `MG/gateway_metrics.rs`: `run_control_requests_total{op,outcome=local|peer|no_owner|timeout|error}`.
- `MP/rust/crates/mp-events/src/subjects.rs` (constants live here, `:62` `PREFIX`): `runctl_request_subject(request_id, op)` = `mp.v1.runctl.req.{id}.{op}`, `runctl_run_cancel_subject(run_id)` = `mp.v1.runctl.run.{id}.cancel`, wildcard consts; unit tests beside the existing ones.
- `MP/deploy/nats.conf` (model-gateway-runtime, `:14-35`): add publish `mp.v1.runctl.>` and `_INBOX.MODEL_GATEWAY_RUNTIME.>` (reply path), subscribe `mp.v1.runctl.>`. Deploy order: ACL before flag (it rides `release-artifact.sh:92`).
- `MP/deploy/docker-compose.yml` (model-gateway env, `:212-247`): `MODEL_GATEWAY_RUN_CONTROL_BUS: ${MODEL_GATEWAY_RUN_CONTROL_BUS:-off}`. Whether `docker-compose.production.yml:48` / `.release.yml:29` inherit it unchanged is not determined; check.
- `MP/deploy/docker-compose.replicas.yml` NEW: `model-gateway-b` via `extends` of `model-gateway`, host ports `127.0.0.1:8081:8080` / `9091:9090`. `MP/scripts/compose.sh` (near `:122`): `MODEL_PLANE_REPLICAS=1` appends it; refused in artifact mode like bridges (`:35`).
- `MP/docs/decisions/ledger.md` entries (D1, D3 correction).

**Contracts.** No proto, no migration. Wire is JSON on core NATS (at-most-once), payload <= 8 KiB (NATS `max_payload 1MB`, `nats.conf:4`).
**Flag.** `MODEL_GATEWAY_RUN_CONTROL_BUS=off` (default) | `on`. Rollback = set `off`, restart; no data. At boot with `on`, the gateway publishes/subscribes a self-test subject; on a permission violation it logs an error, sets `degraded`, and behaves as `off` (guards the ACL-drift risk).
**ZDR clause.** S1 carries no content. Rule fixed here: no op builder exists that takes content without a `&Claims` that already passed `require_non_zdr_durable_mutation` (S3).
**Tests.** Unit in `run_control.rs`: subject injection refused; loopback request/reply; timeout; no-responders mapping; drop of listener unsubscribes. `MG/../tests/run_control_nats_acl_contract.rs` NEW: `include_str!("../../../../deploy/nats.conf")`, asserts model-gateway-runtime can publish/subscribe every subject the helpers emit (the repo's cross-service invariant pattern, `MP/CLAUDE.md` Code Style). `MG/../tests/run_control_nats_live.rs` NEW `#[ignore]`, needs `NATS_URL`: confirms async-nats 0.49.1 returns a no-responders error against nats 2.12.12 (`docker-compose.yml:47`); this is unverified and the 404 vs 503 mapping depends on it.
**DoD.** Flag off: byte-identical behaviour (existing gateway tests green). Flag on, single replica: cancel/queue unchanged; self-test passes against the compose NATS.
**Evidence.** Rebuilt model-gateway image, `MODEL_PLANE_REPLICAS=1 ./scripts/compose.sh up -d` (two replicas healthy on :8080 and :8081), NATS log shows no "Permissions Violation" for `mp.v1.runctl`, `/readyz` `run_control: ok` on both.
**Effort.** M (3-4 d). **Deps.** WS01 for ACL deploy; none else. **Risks.** async-nats no-responders semantics; ACL drift.

### S2 - Shared stream cancel and durable-cancel fan-out [BUILD]

**Goal.** Stop reaches the producer on any replica; durable run cancel also stops a live stream on another replica.

**Files**
- `MG/cancel_registry.rs`: entry gains `Option<RemoteListener>`; `finish` (`:112`) drops it, so the 12+ `finish_stream_registrations` call sites (`sse.rs:59-65`, used 2264...4408) need no edit. Sync API (`is_cancelled :133`) unchanged. New `attach_remote(request_id, run_id)`.
- `MG/run_control.rs`: per-stream subscription `mp.v1.runctl.req.{request_id}.*` and `mp.v1.runctl.run.{run_id}.cancel`; owner handler calls `cancel_for` / `cancel_run_for` with the (org,user) carried in the message and replies `{cancelled}`. `cancel_stream()` returns `Local | Peer | NoOwner | Unavailable`.
- `MG/sse.rs`: one call-in after `register_run` (`:1978`): `state.run_control.attach(...).await`, bounded 250 ms, failure = warn and continue local-only. `attach` also flushes before the stream's `connected` frame, so no remote Stop can precede the subscription. (sse.rs is the hot file: single-line call-in only.)
- `MG/http_routes.rs`: `invoke_cancel` (`:6873`): local `cancel_for`, else bus -> 202 `{cancelled:true, via:"peer"}` | 404 (unchanged body) | 503. `cancel_run` (`:1602`): after the durable cancel, local `cancel_run_for` (`:1655`) OR bus run-subject; the existing outcome log line gains `via`.
- No FE/BFF change: contract and 404 semantics are the same; a 503 lands in the existing `recordCancelFailure` path.

**Contracts.** None new beyond S1 subjects. **Flag.** same bus flag.
**ZDR clause.** Ids only on the wire. ZDR streams never register (`sse.rs:976, 1069, 1978`), so Stop for a ZDR request id is a 404 and nothing persists. Add test `zdr_turn_registers_no_run_control_listener`. No golden-matrix row (no persistence); the inventory test is unaffected because stream cancel is not a durable mutation.
**Tests.** `MG/../tests/run_control_two_replica_test.rs` NEW (two `AppState`s over one `LoopbackHub`, harness in the style of `http_routes.rs:10235` `test_state` and `orchestration_http_test.rs:710-866`): stream registered on A, cancel on B sets A's flag; wrong user/org = 404 (no oracle); run-cancel fan-out reaches A; no owner = 404; bus down = 503; listener removed on `finish`; owner dies mid-request = timeout 503. Extend `http_routes.rs` tests near `:10599, 10635`.
**DoD.** All above green; with the flag on, a Stop on the non-owner replica ends the owner's stream with a terminal `stopped` and `gateway_active_streams` returns to 0.
**Evidence.** Two-replica run: start `/v1/invoke/stream` on :8080 with a slow tool turn, `POST /v1/invoke/{id}/cancel` on :8081; capture the `stopped` frame on the :8080 stream, both replicas' `run_control outcome=peer` log lines, and `run_control_requests_total`.
**Effort.** M (3-4 d). **Deps.** S1; live ZDR wave committed. **Risks.** subscription race (mitigated by flush before `connected`); sse.rs conflicts.

### S3 - Shared queued input and cross-replica liveness [BUILD]

**Goal.** A queue POST on any replica reaches the owner and returns its real outcome; resume tails a producer on another replica.

**Files**
- `MG/queued_input.rs`: derive Serialize/Deserialize on `EnqueueOutcome` (`:75`), add `ThreadMismatch`; new `enqueue_checked(request_id, org, user, text, claimed_thread_id, op_id)` that merges `thread_for` (`:149`) and `enqueue_for` (`:163`) into one atomic owner-side call; bounded per-entry `seen_ops` (16) so one retry after a lost reply cannot double-enqueue; `finish` (`:226`) returns the count of undelivered items (callers ignore it; feeds `queued_input_undelivered_total` and a warn log).
- `MG/run_control.rs`: `queue` and `probe` ops. Probe replies `{live:true, thread_id}`.
- `MG/http_routes.rs` `invoke_queue_input` (`:7308`): ZDR/waived gate first, then local, else bus; map `ThreadMismatch` -> 409 (today `:7336-7344`), `NoOwner` -> the existing 404 + `resend_as_new_turn`, `Unavailable` -> 503 `{queued:false, retryable:true}` (SPA maps it to `refused/other`, `chat-client.ts:~1999`). Persist step (`append_queued_user_message`) unchanged and still after enqueue (D3).
- `MG/sse.rs` `invoke_resume_sse` (`:7196`): replace the sync `producer_live` closure (`:7233`) with a `ProducerLiveness` that is local `is_live_for` OR a probe cached 2 s; used at `:7234` and in the tail loop (`:7350`). Effect: a quiet producer on another replica is followed up to `TAIL_MAX` instead of closing at the 120 s inactivity limit, and an unbuffered-yet producer is not a 404.
- Docs: none for taxonomy (no event change in S3).

**ZDR clause.** Queue text crosses NATS only after `require_non_zdr_durable_mutation` (live `zdr_posture.rs`, which refuses ZDR **and waived** tokens). If this slice lands before the live wave, it adds the guard itself. Test `queue_bus_message_is_never_built_for_zdr_or_waived_claims`. Content is held only in the owner's memory queue (as today) and the NATS core subject (not persisted by core NATS). Golden matrix: the existing inventory test `every_durable_mutation_refuses_a_zdr_or_waived_token_before_it_runs` (`L:zdr_posture.rs:825`) must stay green; add matrix enforcement row `e_zdr_token_queue_input_refused` (proposed name; integrator aligns with `zdr-posture-matrix.json` `enforcement_rows`).
**Tests.** Two-replica file extended: queue on B delivered through A's `drain`; Full (3), TooLong (2000), Empty, foreign user = NoOwner; duplicate `op_id` after simulated lost reply = one item; thread mismatch = 409; probe keeps resume tail alive past 120 s with a fake clock; `queued_input.rs` unit tests (`:277-463` exist) extended. FE: none.
**DoD.** Queue and resume behave identically from any replica; no message lost or duplicated in the loopback fault tests.
**Evidence.** Two-replica run: stream on A with a tool loop, queue via :8081; capture `queued_input` SSE on A's stream, the persisted message in session-core (`persisted:true`), and a resume issued on :8081 for a producer quiet >120 s still tailing. Include `queued_input_undelivered_total` for the leftover case.
**Effort.** M (4-5 d). **Deps.** S2; live ZDR wave. **Risks.** persisted-order contradiction left in place (D3); at-most-once loss (surfaced as 503, never silent).

### S4 - Durable plan grant [BUILD]

**Goal.** An approval on replica A governs the next Do run on replica B, with its audit event and plan link.

**Files**
- `MP/proto/model_plane/v1/sessions.proto`: `SetRunModeRequest` (`:1136-1160`) add `string approved_plan_id = 6;` (fields 1-5 used; verified). New small service `ThreadAutonomy { rpc GetThreadGrant(GetThreadGrantRequest{thread_id=1, org_id=2}) returns (GetThreadGrantResponse{AutonomyRung granted_rung=1; string justification=2; string approved_plan_id=3; Timestamp expires_at=4}) }`, separate from `SessionCore` so mocks stay source-compatible (same reason as `ManagedRunLifecycle`, `sessions.proto:~185-195`). Regenerate `MP/go/gen/model_plane/v1/sessions*.pb.go`.
- `SC/migrations/<next>_thread_autonomy_grant.sql` NEW (+ `revert/<next>_thread_autonomy_grant.down.sql`, the live folder convention): `ALTER TABLE threads ADD COLUMN granted_rung TEXT, granted_justification TEXT, granted_plan_id TEXT, granted_at TIMESTAMPTZ, granted_expires_at TIMESTAMPTZ`, CHECK on allowed rung labels. Additive, no backfill.
- `SC/src/grpc.rs`: `set_run_mode` (`:4984`) writes the thread columns in the same transaction as the run metadata when a grant is present (expiry `MAX_PLAN_TTL_SECS` = 86 400 s, mirrors `coordinator.rs:52`); new `ThreadAutonomy` impl (auth: `authorize_operation(.., "session:read")` + `authorize_thread_owner` `:292`), registered where `ManagedRunLifecycleServer` is added (`:7177`); expiry enforced server-side.
- `MG/session_flow.rs`: `thread_grant(state, thread_id, bearer)`. `MG/sse.rs`: resolve rung and approved plan in the same async pre-step as `approved_plan_context` (`:8384`), pass the rung down to `spawn_run_dispatch`'s request build (`:8663` today); plan CAS (`claim`) stays the single-use guard; a plan no longer APPROVED is skipped silently (log level down).
- `MG/http_routes.rs` `plan_approval` (`:7041`): `was_in_plan_mode = RunDetail.mode == "plan"` from the `GetRun` it already performs; publish `RUN_PLAN_MODE_EXITED` regardless of the local flag; pass `approved_plan_id` in `SetRunMode`.
- `MG/coordinator.rs`: with the flag on, `record_grant`/`take_approved_plan`/`granted_rung` (`:166-208`) and the `record_grant` call (`:327`) are no longer used; the run-flag half stays as a documented per-replica advisory cache (no external callers). Remove the dead half in S9.
- Docs: `MP/docs/capability-ownership-matrix.md` row 26 and section 4.1; ledger entry.

**Flag.** `MODEL_GATEWAY_DURABLE_PLAN_GRANT` (default off, on after evidence). Rollback: off falls back to the in-memory grants (data remains, harmless).
**ZDR clause.** `plan_approval` already refuses ZDR/waived tokens (`continuation_gate`, `L:`), and agentic runs are refused under ZDR (`ZDR.md:96`), so no grant row can originate from a ZDR turn. Content stored: a rung label, the user's free-text justification (already persisted today in `runs.metadata`), a plan id. Erasure: columns live on `threads`, removed by `DELETE FROM threads` (`SC/src/gdpr.rs:368`) and Space deletion. Golden matrix: add enforcement row `e_zdr_token_plan_grant_refused` (proposed name); `plan_approval` is already in the live inventory.
**Tests.** Real-PG `#[ignore]` tests (pattern `grpc.rs:9338`) for write-on-approve, expiry, wrong-org read, wrong-thread-owner read; extend `MP/scripts/verify-durable-layer.sh` (it already covers `SetRunMode`); gateway test with a mock `ThreadAutonomy` server (mock pattern `orchestration_http_test.rs:866`): approve on state A, next run on state B carries the rung; two concurrent starts, one wins the plan CAS; `was_in_plan_mode` correct cross-replica; the existing coordinator tests (`:612-659, 800-899`) updated.
**DoD.** Cross-replica approve-then-run test green; flag on in the two-replica stack.
**Evidence.** Plan approval via :8080, next Do-run via :8081; capture `RunAgentRequest` autonomy rung in the gateway log (add an info line with the rung; today only budget is logged), the `RUN_PLAN_MODE_EXITED` event in session-core `events`, and the thread row.
**Effort.** L (5-7 d). **Deps.** live session-core wave; migration number from the integrator; WS14 consumes. **Risks.** Go-bindings regen needs buf (not installed here); one more RPC per Do-run start (single PK read, no cache, on purpose: a cached "no grant" would reintroduce the bug).

### S5 - Shared turn record [BUILD]

**Goal.** A rating on any replica resolves the turn and keeps skill attribution.

**Files.** `MG/chat_turn_registry.rs`: best-effort Redis mirror (`mp:gw:turn:{org}\x1f{user}\x1f{request_id}`, ids only: `run_id`, `skill_ids`, TTL 24 h = `TURN_TTL`), reusing the stream buffer's breaker and manager config (make `stream_buffer.rs:288` helpers `pub(crate)`; no new abstraction). `MG/sse.rs:1768` also writes shared (spawned, never blocks the turn). `MG/http_routes.rs`: `resolve_feedback_target` (`:6412`) stays sync for local; `ingest_feedback` (`:6476`) adds an async shared lookup before returning `turn_not_rateable`. `PreviousTurnRegistry` stays local.
**Flag.** `MODEL_GATEWAY_SHARED_TURN_RECORD` (off -> on in S9).
**ZDR clause.** ZDR turns never call `record_chat_turn` (`zdr_direct_stream`); the record holds ids only, identity-scoped key, no content. Waived turns are persistent. Test `shared_turn_key_is_identity_scoped`.
**Tests.** Unit with a fake store; extend `http_routes.rs` tests `:10378-10480`. **DoD/Evidence.** Rate a turn from :8081 that streamed on :8080, skill ids present in the feedback envelope (`http_routes.rs:~6529`). **Effort.** S (1-2 d). **Deps.** S1 not required. **Risks.** Dragonfly eviction -> today's degrade (run-only), documented.

### S6 - Durable send receipts [BUILD]

**Goal.** A send or queued message carries a client id; a retry or an end-of-run resend can never create a second transcript row; the strip shows truthful delivery state.

**Files**
- `sessions.proto`: `AppendMessageRequest.client_message_id = 18` (last used 17, `:312-ish`, verified in HEAD and live), `AppendMessageResponse.message_id = 2`, `bool deduplicated = 3`.
- `SC/migrations/<next>_message_client_id.sql` NEW: `ALTER TABLE messages ADD COLUMN client_message_id TEXT CHECK (client_message_id IS NULL OR client_message_id ~ '^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$')` (the 0040 key pattern, `0040_direct_run_completion_key.sql`), `CREATE UNIQUE INDEX ... ON messages (thread_id, client_message_id) WHERE client_message_id IS NOT NULL`.
- `SC/src/grpc.rs` `append_message_in_tx` (`:2430`): after the thread lock (the `SELECT ... FROM threads ... FOR UPDATE` at the top of the function) and before the INSERT (`:2569`), look up `(thread_id, client_message_id)`: same role+content = return the existing sequence, `deduplicated=true`, no event, no memory extraction; different content = `already_exists` (immutable-input rule used for steps). `AppendedMessage` gains `message_id`, `deduplicated`; `CompleteDirectRunWithMessage` callers unaffected.
- `MG/session_flow.rs`: `append_user_message` (`:249`) and `append_queued_user_message` (`:1602`) take `client_message_id`; `prepare_run_with_bearer` passes `req.idempotency_key` when non-empty and the turn is not `regenerated`/`edited_resubmit`. `MG/queued_input.rs`: owner records the item's `client_message_id` (replaces S3's `op_id`). `MG/http_routes.rs` `QueueInputRequest` (`:7274`) + response: `client_message_id`, `message_id`, `persisted`. After S6 reorder to persist-then-enqueue (D3 closed): owner first probes (thread id), then persist, then enqueue; a run that ended in between returns `resend_as_new_turn:true, persisted:true`, and the resend dedupes.
- `MG/sse_events.rs` (`ChatEvent::QueuedInput { messages }`, `:318`, wire arm `:417`, `to_sse :594`): additive `ids: Vec<String>` parallel to `messages` (omitted when empty); regenerate `MP/docs/sse-event-taxonomy.md` (`gen-sse-event-taxonomy.py`).
- FE: `FE/src/shared/api/chat-client.ts`: request body `...(clientMessageId && !zdr ? { idempotency_key: clientMessageId } : {})` (body builder `:1090-1135`), `queueInvocationInput` (`:~2000`) sends `client_message_id`, `queued_input` handler (`:1693`) passes `ids`; `use-chat-controller.ts` mints `clientMessageId` per user send (`createId`, `chat-normalizers.ts:1217`), matches deliveries by id (falls back to content, `:3225-3238`), and on stream end moves still-`pending` accepted items to `deferredSends` (`:2183, 2990-3015`) with the same id so the resend dedupes; `chat-types.ts` `QueuedInput` gains `clientMessageId`; `ChatMessages.tsx` strip shows delivered / not delivered / resent (read-only). Action contract FIRST: `FE/src/shared/actions/action-registry.ts` `chatQueueInvocationInput` (`:1034`) add `clientMessageId`, then `BFF/domains/actions/dispatchers.rs:4600` forwards it; update `FE/src/shared/actions/catalog-manifest.ts` `executionContractFor` only when the owner issues a receipt (idempotency `caller_supplied`) and its test.
- `BFF/domains/chat/json_handlers.rs:176-220` already forwards the body; no logic change.

**Flag.** `MODEL_GATEWAY_SEND_RECEIPTS` (gateway: honour `client_message_id`; default off), SPA sends the id regardless (server ignores unknown/unused). Rollback: off; column stays NULL.
**ZDR clause.** ZDR turns have no messages (`ZDR.md:96`), so no id is stored; SPA omits the key when `zdr`. Waived/persistent threads store an opaque client id only (format-checked, not content-derived). Erasure: column on `messages`, deleted with the thread. Golden matrix: unchanged posture; add a real-PG test that a ZDR claim cannot reach `append_message` with a client id.
**Tests.** Real-PG: duplicate (same content) = same sequence, one row, one MESSAGE_APPENDED, one memory extraction; conflicting content = error; concurrent duplicates serialise on the thread lock; Rust gateway two-replica: queue dedupe on retry; SPA `chat-client.test.ts` (extend `describe 'mid-run queued input'` ~297, `queueInvocationInput` cases 335-463), `mid-run-input.test.ts` (source-text guards: update, do not delete), new `use-chat-controller.send-receipts.test.ts`; BFF `json_handlers.rs` tests (`:512-603`).
**DoD.** Run-ended race and end-of-run leftover each produce exactly one transcript row and one model answer.
**Evidence.** Two-replica run: queue a message after the final round boundary, observe strip -> "resent as new turn", one row in `messages` (psql), one answer.
**Effort.** L (6-8 d). **Deps.** S3; live session-core wave; WS17 (shares `AppendMessage*`, section 5). **Risks.** hot proto/`append_message_in_tx` file; Space non-owner append work (Space session) edits the same function.

### S7 - Shared idempotency claim and attach [BUILD] (flagged, last)

**Goal.** A retried send with the same key on another replica attaches to the original run instead of starting a second.
**Files.** `MG/idempotency_registry.rs`: optional Redis `SET NX PX` claim (`mp:gw:idem:{scoped}`; value = `{request_id, run_id, thread_id, state}`, no answer text; Pending 120 s, Done 600 s, matching `:57, :61`); local-first, shared on local miss; `MG/sse.rs:1180`: for a shared `InFlight`/`Cached`, serve `invoke_resume_sse`'s replay path (refactor into `resume_stream_for(state, claims, request_id, after_seq)`; the buffer holds the original `connected` frame so the SPA proceeds normally) instead of `duplicate_in_flight`. Unary `/v1/invoke` (`http_routes.rs:~8915`) stays per-replica (API-only callers; documented).
**Flag.** `MODEL_GATEWAY_SHARED_IDEMPOTENCY` default off. **ZDR clause.** ZDR never claims (`sse.rs:1069` before `:1180`); shared value holds ids only. **Tests.** two-replica duplicate send; buffer evicted -> error with original `request_id`; fake Redis outage -> local behaviour. **Evidence.** duplicate POST with one key to :8080 and :8081 yields one run (session-core `runs`) and two clients seeing the same frames. **Effort.** M (3-5 d). **Deps.** S6 (SPA sends keys), S1. **Risks.** sse.rs refactor conflicts (WS02, WS03, WS15, WS09): land as one early small PR.

### S8 - Queue tray controls [BUILD, decision-gated, not scheduled]

Only after D4. Owner op `retract(client_message_id)` -> `Retracted | AlreadyDelivered | NotFound` (owner-side, cheap, can be built any time); transcript side needs a `withdrawn` marker on the persisted message in session-core, which depends on WS17 message versions. `FE` strip gains Edit/Remove; WS14 docks it as a tray. ZDR: queue is refused for ZDR/waived, so no new case. **Effort.** M (3-4 d) after WS17. **Risk.** reversing the ledger's "persist when accepted" invariant for withdrawn messages; must be an explicit ledger reversal.

### S9 - Flip defaults, clean-up, evidence [FINISH]

Flip `MODEL_GATEWAY_RUN_CONTROL_BUS`, `_DURABLE_PLAN_GRANT`, `_SHARED_TURN_RECORD`, `_SEND_RECEIPTS` to on in `docker-compose.yml`; keep `_SHARED_IDEMPOTENCY` off until its own proof. Remove the dead grant half of `PlanModeStore`. Update ledger, `capability-ownership-matrix.md` rows 19/26/70, `MP/docs/ZDR.md` section 2 (add the "no run control for ZDR" statement), `MP/MODEL_PLANE_STATUS.md` (evidence-state row: source-verified vs live-verified with date).
**Evidence (the repo counts a deployment claim only after rebuilt images, a running stack and a verified end-to-end flow).** New `MP/scripts/verify-replica-run-control.sh` (modelled on `verify-durable-layer.sh`) against `MODEL_PLANE_REPLICAS=1` compose: (1) Stop via the other replica; (2) queue via the other replica with `queued_input` delivery; (3) approve plan on A, Do-run on B; (4) kill -9 the owner replica mid-stream and check: Stop returns 404 then the SPA's durable `cancelRun` ends the run, queue returns `resend_as_new_turn`, resume answers from the buffer; (5) NATS stopped: 503 not 202. Capture the script output, both replicas' logs, `run_control_requests_total`. **Effort.** S (2 d).

## 5. Interfaces

**PROVIDES**

| To | Contract |
|---|---|
| WS03 durable loop | `RunControl::attach(request_id, run_id, org, user)` and run-keyed cancel subject `mp.v1.runctl.run.{run_id}.cancel`, so `/continue` segments on any replica are cancellable. **Correction to WS03 section 5:** `agentic_run_stream` does not register with the cancel registry at HEAD (`sse.rs:9167-9800`); WS03 must register it, and first make the execution-core loop observe cancel (WS04 does not). Also `ThreadAutonomy.GetThreadGrant` for resume seeding |
| WS14 hitl | Durable thread grant (`SetRunModeRequest.approved_plan_id = 6`, `GetThreadGrant`); `queued_input.ids` + `client_message_id` receipts for the docked queue tray; correct `was_in_plan_mode`. WS14 owns approve-then-continue |
| WS17 history | `AppendMessageRequest.client_message_id = 18`, `AppendMessageResponse.message_id = 2`, `deduplicated = 3`, column `messages.client_message_id` + unique `(thread_id, client_message_id)`; S8 depends on WS17 versions |
| WS16 attention | Cross-replica producer liveness probe, undelivered-queue metric, shared turn record |
| WS15 run events | `queued_input` additive `ids`; any new lifecycle event goes through the taxonomy gate |
| WS01 ops | ACL change in `deploy/nats.conf`, compose env, `deploy/docker-compose.replicas.yml`, deploy order ACL -> gateway -> flag |
| Integrator | Migration numbers for `<next>_thread_autonomy_grant`, `<next>_message_client_id`; proto field numbers 6 / 18 / 2-3 reserved here |

**CONSUMES**

| From | Contract |
|---|---|
| Live ZDR wave (WS01/Z-tasks) | `zdr_posture.rs` gates before S3; migrations 0044/0045 numbering |
| WS01 | Rebuilt images, running stack, NATS ACL deploy, replica count |
| WS03 | Loop-level cancel observation; agentic stream registration |
| WS17 | Message versions for S8 |
| WS02 | Composer edits to `sse.rs` land first |

**Conflict hot-spots and edit-first rule**

| File | Others | Rule |
|---|---|---|
| `MG/sse.rs`, `MG/http_routes.rs` | WS02, WS03, WS09, WS15, WS25 | Live set first, WS02 second; WS04 adds single-line call-ins (S2, S3) and one early refactor PR (S7) |
| `MG/state.rs`, `lib.rs` | all | S1 first, one field |
| `MP/proto/.../sessions.proto`, `SC/src/grpc.rs` | WS03, WS14, WS15, WS17, Space F-t1-001 | append-only; numbers above reserved for WS04; others take the next free number after rebase |
| `MP/deploy/nats.conf`, `docker-compose.yml` | WS01, WS06, WS15 | WS01 owns deploy ordering; WS04 adds only the runctl lines |
| `FE chat-client.ts`, `use-chat-controller.ts`, `ChatMessages.tsx` | WS09, WS12, WS14, WS16 | live set first; S6 touches the send path and strip only |
| `SC/migrations` | all | integrator assigns numbers; names in this doc are purposes |

## 6. Cross-session interfaces

- **Space session (a).** Member steering and cancel (F-t1-040, C10 in `overlap-space.md`): shared cancel/queue state must precede it (D.7). WS04 owns the bus; ownership checks are owner-bound `(org_id, user_id)` today. Space adds a Control "run steer" decision; WS04 will accept an `authorized_actor` field on the bus message once that decision exists. Assume the Space session delivers the decision verification at the BFF/gateway edge, not on the bus. Shared hot spot: `append_message_in_tx` (S6 vs F-t1-001 non-owner append); whoever lands first owns the function, the other rebases. `stream_run_events` read branch is theirs.
- **Quarry/browser session (b).** Browser runs have their own control path (`MG/browser_run.rs:725`); WS04 does not register them. Shared resource only: Dragonfly capacity (cache-mode, 256 MB, three services, `docker-compose.yml:117-118`); WS04's Redis use is small and best-effort. The broker grant-store choice (Redis vs Postgres, `overlap-quarry.md:66`) should not assume Dragonfly durability.

## 7. Regression guards (audit section 8)

| Strength | Touched how | Gate |
|---|---|---|
| ZDR by construction | New stores (bus, thread grant, message id, turn/idempotency keys) | `zdr_turn_registers_no_run_control_listener`; `queue_bus_message_is_never_built_for_zdr_or_waived_claims`; live inventory test `every_durable_mutation_refuses_a_zdr_or_waived_token_before_it_runs`; matrix rows `e_zdr_token_queue_input_refused`, `e_zdr_token_plan_grant_refused`; `zdr-contract-sync.test.sh` |
| Two loops stay separate | None: `tool_loop.rs:6528` still drains the local queue; the bus only feeds it | `cross_service_loop_contract.rs` unchanged; code review rule "no dispatch edits" |
| Honest outcomes | Cancel/queue report `peer`, 404, 503 distinctly, never a fabricated success | two-replica fault tests; the existing `cancel_run_logs_one_outcome_line_per_call` (`http_routes.rs:10725`) extended |
| Typed ChatEvent taxonomy, byte-verbatim relay | `queued_input.ids` additive | `gen-sse-event-taxonomy.py --check`; producer/consumer coverage; BFF unchanged relay |
| Multi-tenant server-side runtime, resume across replicas | S3 strengthens | existing stream-buffer tests; new tests that every shared key is identity-scoped like `scoped_stream_key` (`stream_buffer.rs:~50`) |
| Credential isolation | Bus never carries bearers | test that `RunControlMsg` has no credential field |
| Signed capability gate, Model-proposes/Quarry-executes | None | none required |

## 8. Effort and risks

| Slice | Effort | Days |
|---|---|---|
| S1 | M | 3-4 |
| S2 | M | 3-4 |
| S3 | M | 4-5 |
| S4 | L | 5-7 |
| S5 | S | 1-2 |
| S6 | L | 6-8 |
| S7 | M | 3-5 |
| S8 | M (gated) | 3-4 |
| S9 | S | 2 |

Total about 30-41 days without S8, several slices parallel. **Critical path** S1 -> S2 -> S3 -> S9 = 12-15 days; S4, S5, S6 only need the live session-core wave and can run beside it. The audit's M fits S1-S3+S5 (about 11-15 days); with S4/S6/S7 it is L/XL (D6).

**Top risks**
1. **ACL/ops drift** (nats.conf deploys with the release artifact; flag on before ACL = silent permission violations). Mitigation: boot self-test falls back to off and flips `/readyz` to degraded; ACL contract test; deploy order in S9 evidence.
2. **Hot-file conflicts** (`sse.rs` +814, `http_routes.rs` +190 live, plus WS02/03/09/15). Mitigation: land live set first, single-line call-ins, one early refactor PR.
3. **Scope growth and a false premise** (no message dedupe in session-core). Mitigation: S6/S7 separately schedulable, re-estimate F-t3-016 (D6).
4. **Core NATS is at-most-once and no-responders semantics are unverified** for async-nats 0.49.1 / nats 2.12.12. Mitigation: ignored live test in S1 before S2; ops return explicit 503; one safe retry via `op_id`.
5. **Cache-mode Dragonfly eviction** for S5/S7. Mitigation: ids only, best-effort, degrade to today's behaviour; authority-bearing state (grants) is in Postgres.
