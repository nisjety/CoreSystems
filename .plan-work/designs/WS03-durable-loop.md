# WS03-durable-loop - Durable agent loop core

Audit item: F-t3-002. Wave W1. Effort of this design: XL overall (core path S1-S3 = L). All paths are relative to the worktree root `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/` (HEAD afb83928) unless tagged `[live]`. `MP` = `apps/Model Plane`, `MPR` = `apps/Model Plane/rust`, `FE` = `apps/Frontend Plane/verevonv3`.

## 1. Scope, items resolved, owner

**Item.** F-t3-002 core slice: persist the Do loop's message fold in session-core, resume it after an approval (and later an answer or a crash), close dangling tool calls with typed closers, keep an effect ledger, park instead of fail on provider exhaustion. `ask_user` itself is WS14; this stream builds the pause state machine it shares.

**Owner plane / services.** Model Plane only.
- session-core (durable owner): fold store, approval consumption, park rows, recovery-worker change, ledger widening.
- execution-core (loop owner): fold writer, resume branch, closers, released-call gate, parking decisions.
- model-gateway: `/continue` SSE route, stage-1 seed brief, `RunAgentRequest.resume` population.
- Frontend gateway (BFF) + verevonv3 SPA: relay route, client method, action contract only (cards are WS13/WS14/WS15).
- orchestrator-core / Temporal: no change (decision D1).

**IN.**
| Slice | Tag | What |
|---|---|---|
| S1 | [BUILD] | ADR, shared fold/closer/ledger contract types in mp-contracts |
| S2 | [BUILD] | Approval single-use consume + continuation mode; cold worker skips loop-owned approvals |
| S3 | [FINISH]+[BUILD] | Stage 1: continue-as-fresh-segment end to end (replaces the dead-end `resume_durable_approval_continuation` and the gateway quarantine for loop approvals) |
| S4 | [BUILD] | session-core encrypted fold store, CAS claim, purge, GDPR |
| S5 | [BUILD] | Effect ledger (widen `session_tool_audit_intents`) + typed closers |
| S6 | [BUILD] | Fold writer in the Do loop (round, pre-effect, pause) |
| S7 | [BUILD] | Stage 2: true in-place resume from the fold, stage 1 kept as fallback |
| S8 | [BUILD] | Parking: credential window, provider exhaustion, crash interruption |
| S9 | [BUILD] | Proof scripts, ZDR matrix test, contract tests, live evidence |

**OUT.** `ask_user` tool and answer UI (WS14); approval cards, scopes, step-up (WS13); lifecycle event taxonomy and status cues (WS15); attention/notifications (WS16); unattended auto-resume with no user present (needs OD8 or the Control reauthorizer; WS20 owns scheduled/unattended authority); declared side-effect classes (WS05 supplies them; this stream ships a stub classifier); browser_agent resume (Quarry/WS21); durable thread workspace (WS22); replica-safe cancel/queue state (WS04); Temporal host for the Do loop.

## 2. Verified current state

All lines opened in this pass unless marked "(sweep)", "(sweep, live)" or "[live]".

**The loop and its fold.**
- `run_rounds` keeps the fold in a local `Vec<ChatMessage>`: `MP/rust/services/execution-core/src/runtime_loop/agent.rs:1168-1197` (system prompt + goal), skills inserted at 1206-1221. Per-run counters are locals: `rounds_executed`, `delegated_rounds`, `step_seq`, `attempted_retrievals`, `children_started`, `compaction_triggered`, `grounded` (1224-1245). Nothing is persisted between rounds.
- Round structure: guards at loop top 1255-1300 (route stop, latched refusal, `RunDeadline`), heartbeat 1301, `clear_stale_tool_results` 1319, `Infer` 1389, failed round returns `InferFailed` 1393-1442, no tool calls = answer 1499-1503, pre-pass slots 1585-1849, concurrent dispatch (`execute_step_with_subagent`, retry loop) 1856-1970 (dispatch call at 1919), outcome pass 1976-2075, pause 2086-2108, fold append 2112-2125.
- The fold carries tool results as a framed *user* message (`format_tool_context`, 2120-2125), not typed tool_result blocks. A dangling call therefore has no marker in the history; only the ledger can say whether it ran.
- `OutOfTime` ends the run as failed with a reply (run_agent_with_route 943); the deadline is credential-derived only for a subscription-pinned run (`run_budget.rs:1-120`, `CREDENTIAL_MARGIN` 60 s).
- Subscription refusals are already typed (`subscription_refusal.rs:48-69`: usage_limit, reauth_required, upstream_unavailable, rate_limited with retry-after/resets-at trailers) but end the run as failed (agent.rs 1393-1442). Unpinned runs get `InferFailed(None)`, a generic apology.

**Pause and resume.**
- `pause_for_approval` `agent.rs:4469-4545`: builds the descriptor (4551-4620), calls `CreateApproval` with idempotency key `{run_id}:{step_id}` (4502), sets in-memory status `AwaitingApproval` (4520-4522), returns `awaiting_approval`. The round's other outcomes and the assistant text are discarded (pause returns before the append at 2112). Descriptor creation refuses ZDR (4558-4562).
- RunAgent is a unary blocking RPC: `execution-core/src/grpc.rs:1203-1271`; it refuses ZDR at 1237 (`enforce_persistence_free_execution`, 650-661) and returns on pause. After a pause nothing holds the loop.
- `resume_run` (`grpc.rs:1068-1132`) only flips in-memory state; the approval branch calls `resume_durable_approval_continuation`, which always returns `unavailable` (691-698). `ResumeRunRequest.checkpoint_id` (`MP/proto/model_plane/v1/execution.proto`, field 2) is never read.
- model-gateway `decide_approval` (`MP/rust/services/model-gateway/src/http_routes.rs:1493-1582`) records the decision, then only calls `quarantine_granted_approval_continuation` (`approvals.rs:254-277`), which always errors; the response gets `continuation_delivery: "pending"`. BFF retries `/decide` once on 502/503 (`FE/apps/gateway/src/domains/orchestration.rs:225-284`).
- There is NO gate path that lets a granted call through in the loop: `execute_step_inner` (`runtime_loop/mod.rs:454-561`) returns `awaiting_approval` for capability `Ask` (506-509), hook `Ask` (548) and permission `AwaitApproval` (559) every time. Only provider writes consult the durable approval afterwards (`resolve_write_approval`, 1720-1819, same key `{run_id}:{step_id}`).
- Cold worker: `execution-core/src/approval_delivery_worker.rs:1-130` re-executes only `execute_provider_action`, `book_shipment`, `tickets.create`; every grant inserts an outbox row (session-core `orchestration_store.rs:797-806`, `INSERT_APPROVAL_DELIVERY_OUTBOX_SQL` 835-841). Running both the cold worker and an in-loop resume for the same grant would execute a write twice. S2 closes that.
- Pause leaves the sandbox lease and hydrated workspace cached in-memory only: `run_agent_with_route` returns on `Paused` (929-932) before `release_sandbox_lease_if_any` (984).

**Durable substrate.**
- `checkpoints` table: `session-core/migrations/0001_init.sql:46-53` (id, run_id, state BYTEA plaintext, created_at) + ordinal trigger `0002_events_and_ordinals.sql:74-97`. `SaveCheckpoint` handler `session-core/src/grpc.rs:4701-4777`: 4 MiB cap (78), reserved auto namespace (111), inserts a row and a `CHECKPOINT_SAVED` event per call, has no ZDR check. Only writer: ExecuteStep (`execution-core/src/grpc.rs:1007-1038`). No reader. `RunDetail.checkpoint_index` is `COUNT(*)` (`run_service_grpc.rs:378-381`). `EVENT_TYPE_CHECKPOINT_RESTORED = 111` has no producer (`events.proto`).
- Descriptor AEAD: `session-core/src/continuation_crypto.rs:14-107` (AES-256-GCM, key `SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY`, AAD approval/org/user). Reusable pattern.
- Effect-ledger primitives: `session_tool_audit_intents` (`migrations/0013_tool_audit_intents.sql`: status reserved|completed|failed, partial index on `reserved`), RPCs `ReserveToolAction` / `FinalizeToolAction` (`sessions.proto` 55-59, messages 652-688). Callers today: model-gateway inline loop only (sweep). execution-core's Do loop reserves nothing.
- Crash recovery: `terminalization.rs` `claim_due_recovery` (1601-1643) selects managed runs past a 15 min heartbeat deadline (`MANAGED_DEADLINE_SECS`, 23) and terminalizes them as `outcome_unknown` (1718-1735); it explicitly spares runs with a `requested`/`granted` approval (1617-1623, 1705-1709). It "never reruns a tool" (1738-1740). So today a crashed Do run ends failed; a granted-but-never-continued run stays open forever.
- Approval row: `approvals` table (`migrations/0003_orchestration_tables.sql:135-152`) has no consume marker; `CreateApprovalRequest` highest field 11 (`continuation_descriptor_json`, `orchestration.proto`).

**Host question evidence.** orchestrator-core Temporal drives `ExecuteStep` per turn (`orchestrator-core/cmd/workflows/step_loop.go:62-84`), waits on an `approval` signal that carries no payload check (`interactive_run.go:128-149`), keeps no Control bearer in history (`interactive_run.go:28-36`), and its `ExecuteStepLoopActivity` is "NOT resume-safe" (`cmd/activities/activities.go:610-620`). It never calls RunAgent. The chat Do path is gateway -> `RunAgent` (`sse.rs:8703-8709`).

**Auth/bearer facts.**
- RunAgent needs five user-bound bearers (execution, data-plane, session, inference, plus sandbox for Space runs): `sse.rs:8296-8346`. The BFF mints them per request (`FE/apps/gateway/src/domains/chat/shared.rs:630-720`, helpers `required_*_token`, `streams.rs:18-130`). `/decide` and `/resume` already mint execution+session (`orchestration.rs:138-203`). A click-time `/continue` can mint the full set. No bearer is persisted anywhere with a paused run.
- Managed-run heartbeats use a service token, not the user bearer (`agent.rs:4698-4724`).

**Live-only (uncommitted) work that touches this stream; land before S3 starts.**
| Live path | Why it matters |
|---|---|
| `MPR/crates/mp-contracts/src/zdr_refusal.rs` (??), `retention.rs` (??), `lib.rs` (+2) | live `agent.rs` continuation_descriptor uses typed `Refusal` (sweep, live 4565-4570); HEAD agent.rs does not build against it. Feature keys `agent_run`, `run_control`, `approval_continuation` exist (live `zdr_refusal.rs:~270-285`). |
| `MPR/services/execution-core/src/runtime_loop/agent.rs` (M +207), `grpc.rs` (M +98), `runtime_loop/mod.rs` (M +18), `session_core_rules_double.rs` (M +537) | every S3/S6/S7 edit lands here; run_rounds passes `route.is_waived()` (sweep, live 1937-1939) |
| `MPR/services/session-core/migrations/0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `migrations/revert/` (??) | new migrations must number after 0045; adopt the `revert/*.down.sql` convention |
| `MP/proto/model_plane/v1/sessions.proto` (M +42, `SessionRetentionService`) | S4/S5 append messages to the same file |
| `session-core/src/terminalization.rs` (M +123), `grpc.rs` (M +861), `orchestration_grpc.rs` (M +249) | S2/S4/S8 edit these |
| `model-gateway/src/sse.rs` (M +883), `http_routes.rs` (M +237), `grpc.rs`, `tool_loop.rs` | S3 edits `spawn_run_dispatch` and the route table |
| `MP/api/contracts/zdr-refusals.json` + `.sha256`, `zdr-posture-matrix.json` (readable in live; posture matrix tests live-only `model-gateway/tests/zdr_posture_matrix.rs`) | S9 ZDR matrix test |
| `FE/src/shared/api/zdr-refusals.ts` (??), `chat-client.ts` (M +53) | typed refusals on the continue path |

**Where the audit is stale or wrong.**
1. "Reuse the `checkpoints` table and SaveCheckpoint" - unsuitable (plaintext BYTEA, one row per call, COUNT semantics, no ZDR check, 4 MiB user namespace). Decision D3 picks a new table.
2. "terminalization recovery loop" as a host - it only terminalizes and spares approval-bearing runs; it cannot drive a loop. It is reused as a *park-or-terminalize* gate (S8), not as a driver.
3. "stream_buffer replay / approval rehydration" - not needed: resume reuses the agentic SSE stream and durable run events.
4. "resume must re-authorise through Control with a fresh delegated bearer (OD8)" - satisfied by the BFF minting at click time; no renewable bearer class is required for user-present resume (D2).
5. `docs/core-research/execution-core.md:48-52` says ResumeRunRequest has no approval id; the proto has `approval_id=4` (sweep).
6. Card line numbers drifted (pause 4476-4560 -> 4469-4545 at HEAD; resume stub 706-713 -> 691-698 at HEAD, 706-713 live).

## 3. Decisions and open questions

| # | Question | Options | Recommendation | Gates |
|---|---|---|---|---|
| D1 | Loop host: execution-core vs orchestrator-core Temporal | (a) execution-core owns loop/pause/resume, session-core owns the fold; (b) Temporal workflow re-enters RunAgent | (a). Temporal drives ExecuteStep, has no RunAgent client, needs a fresh user bearer per activity it cannot persist, and its loop activity is not resume-safe (evidence in section 2). Write the ADR as a new `docs/decisions/ledger.md` entry; no existing entry covers the chat-loop host (Temporal lines at ledger :2943/:3119-3129/:4247 per sweep, not reversed). Temporal stays host for ExecuteStep-driven runs. Owner ruling needed. | S1 text; S6, S7 (code can be written, not merged) |
| D2 | OD8 renewable delegated bearer | (a) block on OD8; (b) user-present resume with click-time bearers, defer unattended | (b). Every WS03 resume trigger has a user present (approve click, "Continue", answer). The BFF mints execution/data-plane/session/inference(/sandbox) bearers for that request, so the resumed segment has a fresh ~300 s authority and no bearer is stored. OD8 stays open and gates only unattended auto-resume (WS20/F-t3-018/037/047). Segment length is bounded by the credential window; S8 turns that bound into a park instead of a failure. Do not use the approval-worker service principal (`approval_delivery_worker.rs`, scope `approval:deliver`) for user runs: it would break delegated identity. | none of S1-S9; it only removes "auto-continue with no user" from scope |
| D3 | Fold storage | (a) reuse `checkpoints` + SaveCheckpoint; (b) new `run_loop_checkpoints` table, one row per run, latest-wins with CAS, AEAD ciphertext, purged at terminalization | (b) (reasons in section 2 stale item 1). Encryption key: reuse `SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY` with a distinct AAD domain (`loopfold:`) now; separate key later if rotation needs diverge (wiring cost avoided: compose 755, production 70, `.env.example` 93 per sweep). | S4 |
| D4 | Resume semantics and the cold worker | (a) keep cold worker for everything; (b) loop-originated approvals are consumed by the loop; cold worker only for ExecuteStep approvals and for `tickets.create`/owner actions | (b). Add `continuation_mode` ("" = cold, "segment", "loop") at pause time; outbox insert skipped for non-cold modes; single-use `ConsumeApproval`. This amends the earlier "worker resumes three kinds" design for loop approvals only; record it in the S1 ADR. Reserved owner actions (`requires_durable_owner_approval`, permission/mod.rs:77) stay cold. | S2, S3 |
| D5 | Resume = same run id, new execution segment | (a) new run per continuation; (b) same run_id, `segment_no` increments | (b). One managed terminal receipt per run, one thread answer, approvals keyed `{run_id}:{step_id}` stay valid, plan EXECUTING transition not repeated. | S3, S7 |
| D6 | ResumeRun RPC | (a) extend `ResumeRun` with the fold; (b) re-enter through `RunAgentRequest.resume` | (b). RunAgent already carries the five bearers, route, tools and the blocking response. `ResumeRun` stays the user-pause latch; `ResumeRunRequest.checkpoint_id` is documented as superseded, not removed (wire stability). | S3, S7 |
| D7 | Own-connection ZDR waived runs | persist folds / refuse | Refuse (treat as ZDR for fold persistence), consistent with ledger rule 17 "waived run skips content-derived jobs". Owner to confirm. | S4 (default is refuse, so nothing waits) |
| D8 | Space-scoped runs on `/continue` | support now / refuse until Space session delivers a thread-scope helper | Refuse with typed `continue_space_scope_unsupported` until the BFF can rebuild Control Space scoping from a thread (sandbox bearer needs it: `streams.rs:112`). Personal-thread Do runs work day one. Tracked in section 6. | S3 (typed refusal ships; support later) |

Slices that can proceed without any owner ruling: S1 (draft), S4, S5 (dark behind flags, additive migrations). S2/S3 need D4 and D2 acknowledged; S6/S7 need D1.

## 4. Slices

Order: S1 -> S2 -> S3 (stage 1 shippable) in parallel with S4, S5 (session-core) -> S6 -> S7 -> S8 -> S9 (S9 evidence scripts are written incrementally; final run after S8).

### S1 - ADR and shared contract types [BUILD]
**Goal.** One typed vocabulary shared by execution-core (writer/reader) and session-core (validator), recorded as a decision.
**Files.**
- `MP/docs/decisions/ledger.md` - append entry "Durable agent loop: execution-core hosts, session-core stores the fold, user-present resume, same-run segments" covering D1-D8, the amendment to the cold-worker design (D4), and that Temporal is not the Do-loop host.
- `MPR/crates/mp-contracts/src/loop_fold.rs` - NEW. `LoopFoldV1` (serde): `v`, `run_id`, `segment_no`, `rounds_executed`, `delegated_rounds`, `step_seq`, `children_started`, `grounded`, `compaction_triggered`, `attempted_retrievals`, `messages[{role,content,name,compaction_summary}]`, `round_in_flight{assistant_content, calls[{id,name,arguments_json,step_id}], completed[{step_id,output,error}]}`, `pending: PendingPause{kind: approval|input|park, pause_ref, step_id, call, action_fingerprint}`, `authority{mode,plan_mode,autonomy_rung,space_id,min_privacy_tier,provider_hint,tool_names,max_rounds}`, `prompt_digest`, `extensions` (unknown-tolerant map for WS25 loop-guard state). Plus `MAX_FOLD_BYTES = 4 MiB`, `authority_digest()`, `Authority::narrows(&Authority)`, enums `CloserKind {UnknownOutcome, ResultLost, NotResumableTool, Denied, TimedOut, WorkspaceStateLost}`, `LedgerStatus`, `ParkReason {CredentialWindow, ProviderExhausted, UpstreamUnavailable, CrashInterrupted, ReauthRequired}`. `closer_text(kind, tool)` returns the stable model-facing sentence.
- `MPR/crates/mp-contracts/src/lib.rs` - declare `pub mod loop_fold;` (live file already M +2; rebase after landing).
- `MPR/crates/mp-contracts/tests/loop_fold_golden.rs` - NEW golden: a checked-in JSON fixture round-trips, an unknown field is ignored, a `v:2` is rejected, a bearer-shaped string fails `assert_no_credentials`.
**Contract changes.** none on the wire.
**Flag.** none. **ZDR.** types only; `LoopFoldV1::refuse_if_zdr(zdr: bool)` returns the typed refusal feature `agent_run`.
**Tests.** `cd "apps/Model Plane/rust" && cargo test -q -p mp-contracts loop_fold`; `cargo fmt --all -- --check`; `cargo check --workspace --all-targets`.
**DoD.** ADR merged with owner ruling on D1/D2; types compile; golden passes. **Evidence.** source-verified only (no deployment claim).
**Effort.** S, 2-3 d. **Deps.** D1 text. **Risks.** type churn; keep `extensions` for growth.

### S2 - Approval consume + continuation mode [BUILD]
**Goal.** Make a granted approval consumable exactly once by the loop and stop the cold worker from also executing loop-owned approvals.
**Files.**
- `MP/proto/model_plane/v1/orchestration.proto` - `CreateApprovalRequest.continuation_mode = 12` (string: "", "segment", "loop"); `Approval` gets `consumed_at = 14` (Timestamp, next free after `org_id = 13`) and `continuation_mode = 15`; new RPC `ConsumeApproval(ConsumeApprovalRequest) returns (ConsumeApprovalResponse)` on `OrchestrationCoreService` (user-bearer, not service-only): request `{approval_id, org_id, run_id, step_id, action_fingerprint, claim_token}`, response `{consumed, already_consumed, same_claim}`.
- `MPR/services/session-core/migrations/<next>_approval_consume_and_mode.sql` - `ALTER TABLE approvals ADD COLUMN continuation_mode TEXT NOT NULL DEFAULT '', ADD COLUMN consumed_at TIMESTAMPTZ, ADD COLUMN consumed_claim_hash TEXT`; plus `revert/<next>_*.down.sql`.
- `session-core/src/orchestration_store.rs` - `create_approval` persists the mode (insert near the descriptor insert, ~690-735); `decide_approval` (755-824) skips the `INSERT_APPROVAL_DELIVERY_OUTBOX_SQL` branch when `continuation_mode <> ''`; new `consume_approval` CAS (`WHERE status='granted' AND consumed_at IS NULL AND org_id=$ AND run_id=$ AND user_id=$caller`, compares `approval_continuation_descriptors.action_fingerprint` when a descriptor row exists).
- `session-core/src/orchestration_grpc.rs` - `consume_approval` handler beside `get_approval`/`decide_approval`; owner-bound (`authorize_run_owner` Mutate), ZDR refusal feature `approval_continuation`; emits `RunResumedAfterApproval` (message exists in `OrchestrationEvent`, tag 16; emission site not determined by the sweep) in the same transaction; replay-safe.
- `session-core/src/terminalization.rs` - `claim_due_recovery`/`has_pending_approval` (1617-1623, 1645-1659): a `granted` approval that is `consumed` no longer counts as pending activity.
- `execution-core/src/runtime_loop/agent.rs` - `pause_for_approval` (4469) sets `continuation_mode` from the flag; unchanged when flag off.
- `model-gateway/src/approvals.rs` - `quarantine_granted_approval_continuation` (264-277) returns Ok for non-cold modes; `http_routes.rs decide_approval` (1564-1575) reports `continuation_delivery: "awaiting_continue"` instead of `"pending"` for them.
- `session_core_rules_double.rs` (execution-core test double) and model-gateway test double (`grpc.rs:3565-3587`) - implement `ConsumeApproval`.
**Flag.** `AGENT_CONTINUE_AFTER_APPROVAL_ENABLED` (execution-core + model-gateway), default false (`${...:-false}` in compose services at `MP/deploy/docker-compose.yml` model-gateway :191 block, execution-core :1005 block); pure parser + unit test per the repo flag pattern. Rollback: flag off -> new pauses use cold mode; approvals already in loop/segment mode stay consumable only while the route flag is on (see risks).
**ZDR.** No approval is created in a non-cold mode for a ZDR run (the loop already refuses ZDR; `continuation_descriptor` refuses 4558-4562). `consume_approval` refuses ZDR callers. Golden case in S9.
**Tests.** unit: store CAS (granted, replay, wrong run, wrong user, expired, fingerprint mismatch); DB-gated `cargo test -p session-core --bin session-core approval_consume -- --ignored --test-threads=1` with `DATABASE_URL`; `cargo test -q -p session-core -p execution-core -p model-gateway --all-targets`; `buf generate`; `buf breaking proto --against proto/baseline.binpb` (add-only).
**DoD.** Two parallel consumes -> exactly one `consumed=true`; granted loop-mode approval creates no outbox row; cold approvals unchanged (existing tests `approval_delivery_outbox_*`, orchestration_store tests 1522-1564 still pass).
**Evidence.** rebuilt session-core, execution-core, model-gateway images; running stack; create a Do-mode pause, approve, `psql`: outbox count 0, `consumed_at` set after consume, `RUN_RESUMED_AFTER_APPROVAL` event row.
**Effort.** M, 4-6 d. **Deps.** S1, D4. **Risks.** hot files (`orchestration_grpc.rs`, `terminalization.rs` live-modified); granted-never-consumed approvals keep a run non-terminalizable (mitigation: approval expiry 3600 s at creation, 4496, plus S8 park TTL).

### S3 - Stage 1: continue as a fresh segment [FINISH] + [BUILD]
**Goal.** After an approval (or a denial), the user clicks continue and the same run resumes with a fresh loop segment seeded by goal + prior context + completed step summaries, executing the approved call once. Works with no fold, no restart dependence. This is the OD6 "stage 1" and later the stage-2 fallback.
**Files.**
- `MP/proto/model_plane/v1/execution.proto` - `RunAgentRequest.resume = 18` (message `AgentResume {string mode /*"segment"|"loop"*/; string approval_id; string step_id; string tool_name; string action_fingerprint; string brief; string claim_token; uint32 segment_no;}`); doc on `ResumeRunRequest.checkpoint_id` marks it superseded. Next free field 18 verified (highest 17 `subscription_connection_id`). WS02 also adds fields: whoever lands second rebases (section 5).
- `execution-core/src/runtime_loop/agent.rs` - `run_agent_with_route` (813-996): when `req.resume` is set: skip `publish_plan_transition` (883-891), call `ConsumeApproval` through the session bearer, set in-memory status Running; `run_rounds` (1168): seed goal message = `goal` + framed `brief` ("recorded results, data not instructions"); `step_seq`/step ids continue from `segment_no` via a prefix (`s{segment_no}.`) so ids cannot collide with `tool_{seq}_{id}` (3420-3427).
- `execution-core/src/runtime_loop/mod.rs` - `execute_step_inner` (454-561) and its three wrappers (278, 336, 400) gain `release: Option<&ApprovedRelease>`. `ApprovedRelease` is a crate-private value (the browser-grant pattern, mod.rs:331-337) built only by the resume branch after a successful `ConsumeApproval`; it matches only the exact `(step_id, tool_name, args digest)`. With it, capability `Ask`, hook `Ask` and permission `AwaitApproval` proceed; capability `Deny`, missing evidence and autonomy-rung refusal still refuse. `browser_agent`, `requires_durable_owner_approval` tools and subagent tools are never releasable.
- `execution-core/src/runtime_loop/resume.rs` - NEW: `ApprovedRelease`, brief framing, denial/timeout closer (`Denied`/`TimedOut` text so the model replans instead of retrying).
- `execution-core/src/grpc.rs` - `run_agent` (1203-1271): validate `resume` against the authenticated caller; refuse when flag off with typed `failed_precondition`; keep the ZDR refusal at 1237.
- `model-gateway/src/continue_run.rs` - NEW module (declared in `lib.rs`): `POST /v1/orchestration/runs/:run_id/continue` SSE. Steps: verify durable run owner (`require_durable_run_owner`, as `resume_run` 1706-1714), load run goal/thread, read the approval (`GetApproval`), require `granted|denied|timed_out`, build the brief from the run's recorded step events (existing replay path behind `/v1/runs/:run_id/events/replay`, http_routes.rs:218; exact RPC not determined), build `RunAgentRequest` via `build_run_agent_request` (sse.rs:8758-8835) with `resume`, then drive the existing `agentic_run_stream` (sse.rs:9167) so error handling, deadline follow, outcome classification, `awaiting_approval` frame and final `chunk`/`done` frames are shared. Authority fields (mode, rung, plan_mode, tools) come from server state/run metadata, never from the request body.
- `model-gateway/src/sse.rs` - `agentic_run_stream`, `spawn_run_dispatch` (8618-8709), `build_run_agent_request` take an `Option<AgentResume>`; no behavior change when `None`.
- `model-gateway/src/http_routes.rs` - register the route in the orchestration table (303-317); typed JSON errors `{error:{code,message,details}}`: `run_not_found`, `continuation_already_consumed` (409), `approval_not_granted` (409), `not_resumable` (409), `continue_disabled` (409), `continue_space_scope_unsupported` (422), ZDR refusal via the existing typed helper (feature `approval_continuation`).
- `FE/apps/gateway/src/domains/chat/streams.rs` - `continue_run_stream`: mints model/execution/data-plane/inference/session (+ no sandbox in S3) bearers like `stream_chat` (18-47) and relays bytes via `proxy_sse_stream_with_data_plane`; `FE/apps/gateway/src/domains/chat.rs` (route table, near :33/:75) registers `POST /api/v1/orchestration/runs/{run_id}/continue`.
- `FE/src/shared/api/orchestration-client.ts` - `continueRun(runId, approvalId, signal)` returning the SSE stream reader (existing `resumeRun` at 105 stays for user pause); `FE/src/shared/actions/action-registry.ts` (+ test) - action contract `chat.run.continue` added BEFORE any UI wiring (repo rule); UI cards are WS13/WS14.
**Flag.** `AGENT_CONTINUE_AFTER_APPROVAL_ENABLED` (from S2). Default off; flip on the proof stack only after S9 evidence.
**ZDR.** Route refuses ZDR callers (`require_non_zdr_durable_mutation` pattern, http_routes.rs:1500/1713) with feature `approval_continuation`; the loop already refuses ZDR. No new persistence in this slice except the S2 consume row (content-free). Golden case `agent_continue` in S9.
**Tests.** unit: release gate (granted exact call passes; changed args fails; Deny stays Deny; second call of same tool still gates), brief framing, route typed errors; integration: extend `execution-core/src/runtime_loop/agent.rs` tests beside `gated_tool_creates_approval_and_returns_early` (9088 per sweep) with a pause -> continue test; `model-gateway/tests/orchestration_http_test.rs` continue route; BFF `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (from `FE`); FE `pnpm exec vitest run src/shared/api/orchestration-client.test.ts src/shared/actions/action-registry.test.ts`, then `pnpm lint && pnpm typecheck`. New: `execution-core/tests/durable_loop_contract.rs` (cross-service invariant in the `cross_service_loop_contract.rs` style): gateway cannot construct a `resume` without a granted consume.
**DoD.** Approved `shell`-class gated call executes exactly once after `/continue`; run completes with one assistant message and one terminal receipt; `/continue` twice -> second 409; restart of execution-core between pause and approve still works.
**Evidence.** Rebuild session-core, execution-core, model-gateway, BFF (`verevon-gateway-rs`) images; stack up per `apps/Model Plane/scripts/compose.sh` and `FE` compose. Flow: Do-mode chat with `ask` posture -> run pauses (`awaiting_approval` frame) -> `docker restart execution-core` -> `POST /api/v1/orchestration/approvals/{id}/decide` approve -> `POST .../runs/{id}/continue`. Capture: both SSE transcripts, `psql` rows (`approvals.consumed_at`, outbox count 0, run terminal receipt, `RUN_RESUMED_AFTER_APPROVAL`), execution-core log lines for the release, second `/continue` 409 body, a ZDR-org attempt returning the typed refusal.
**Effort.** L, 8-12 d. **Deps.** S1, S2, D2, D4, WS02 field/builder ordering. **Risks.** Stage-1 seed is lossy (only recorded step outputs, truncated) - label it so in the brief; tool outputs in the brief are untrusted data (frame as data, scrub via `scrub::scrub_string`); Space runs refused (D8).

### S4 - Fold store in session-core [BUILD]
**Goal.** Durable, encrypted, CAS-guarded store for the latest fold of a run.
**Files.**
- `session-core/migrations/<next>_run_loop_checkpoints.sql` (+ revert) - table `run_loop_checkpoints(run_id PK REFERENCES runs(id) ON DELETE CASCADE, org_id, user_id, seq BIGINT, kind CHECK in (round,pre_effect,pause,park), state CHECK in (active,paused,parked,resuming,closed), pause_ref, park_reason, park_until, authority_digest CHAR(64), schema_version SMALLINT, ciphertext TEXT CHECK length <= 6 MiB, fold_digest CHAR(64), claim_token_hash, claim_expires_at, segment_no INT, created_at, updated_at)` + index on `(state, updated_at)`.
- `session-core/src/loop_checkpoint.rs` - NEW (`mod loop_checkpoint;` in `main.rs` module list at :11-41): store functions `save` (CAS on `expected_seq`; refuses `closed`), `claim` (CAS `paused|parked|active-and-stale -> resuming` with 120 s claim lease, returns plaintext), `close`, `purge_run`; AEAD via a `FoldCipher` sharing the key loader pattern of `continuation_crypto.rs:14-107` with AAD `loopfold:{run_id}:org:{org}:user:{user}:seq:{seq}`.
- `MP/proto/model_plane/v1/sessions.proto` - append messages and 3 RPCs on `SessionCore` (appended after the existing block; file is live-modified): `SaveLoopCheckpoint{run_id=1, expected_seq=2, kind=3, fold=4 (bytes, scrubbed JSON), authority_digest=5, pause_ref=6, park_reason=7, park_until=8}`/`Response{seq=1}`; `ClaimLoopCheckpoint{run_id=1, pause_ref=2, claim_token=3}`/`Response{seq=1, fold=2, segment_no=3, authority_digest=4, kind=5}`; `CloseLoopCheckpoint{run_id=1, seq=2, reason=3}`.
- `session-core/src/grpc.rs` - handlers next to `save_checkpoint` (4701-4777): `session:write` + `authorize_run_owner(Mutate)`; ZDR/waived refusal read from the run row (same predicate compaction uses, `compaction.rs:84`); size check `MAX_FOLD_BYTES`; event `CHECKPOINT_SAVED` for kinds `pause`/`park` only (content-free `{checkpoint_id, kind, size}`), `CHECKPOINT_RESTORED` on claim (first producer for event 111).
- `session-core/src/terminalization.rs` - `transition_managed_run_terminal` path (992-1057): purge the fold row in the terminal transaction; `session-core/src/gdpr.rs` (236 area; order test at 674-691): delete `run_loop_checkpoints` before `runs`; `session-core/src/grpc.rs:484` (thread delete) likewise.
- `session-core/src/run_service_grpc.rs` and `MP/proto/model_plane/v1/runs.proto` - `RunDetail.resumable = 17` (bool) / `park_reason = 18`; `checkpoint_index` stays the legacy COUNT.
**Flag.** `SESSION_CORE_LOOP_CHECKPOINT_ENABLED`, default false (handlers return `failed_precondition loop_checkpoint_disabled`); compose `session-core` block (:712).
**ZDR.** Never persisted for ZDR or waived runs (handler + writer both check); ciphertext only; purge at terminalization, GDPR erasure and thread delete; no plaintext in logs or events.
**Tests.** unit (cipher round trip, AAD mismatch, CAS, size cap, ZDR refusal), DB-gated `cargo test -p session-core --bin session-core loop_checkpoint -- --ignored --test-threads=1`, GDPR order test extended, `cargo test -q -p session-core --all-targets`, `buf breaking`.
**DoD.** Stale `expected_seq` rejected; concurrent claims -> one winner; DB bytes contain no plaintext marker; erasure removes rows.
**Evidence.** rebuilt session-core image; `psql` shows ciphertext only, `grep` for a seeded marker string over `ciphertext` finds nothing; erase an org and confirm rows gone.
**Effort.** M-L, 6-8 d. **Deps.** S1, D3. **Risks.** key coupling with descriptors (D3); proto file hot spot (live +42); BYTEA vs TEXT size growth (4 MiB cap per row, one row per run).

### S5 - Effect ledger and typed closers [BUILD]
**Goal.** Know, per effectful call, whether it was reserved, ran, failed or is unknown, and say so to the model honestly after an interruption.
**Files.**
- `session-core/migrations/<next>_tool_action_effect_ledger.sql` (+ revert) - widen `session_tool_audit_intents.status` CHECK (0013) to add `not_run` and `unknown_outcome`; add `args_digest TEXT`, `effect_class TEXT`, `closed_by TEXT`, `provider_receipt_id TEXT`.
- `MP/proto/model_plane/v1/sessions.proto` - `ReserveToolActionRequest` fields `args_digest = 7`, `effect_class = 8` (next free after 6); `FinalizeToolActionRequest` `receipt_id = 4`, outcome values add `not_run`, `unknown_outcome`.
- `session-core/src/grpc.rs` - `reserve_tool_action_inner` (3650 per sweep) / `finalize_tool_action_inner` (3732 per sweep) accept the new fields and statuses; replay stays idempotent; a sweep (spawned in `main.rs` next to the compaction task at :154) closes `reserved` rows older than a TTL whose run is terminal or parked as `unknown_outcome`.
- `execution-core/src/runtime_loop/effect.rs` - NEW: `effect_class_of(tool, args) -> EffectClass` (stub: `permission::is_risky_call`, permission/mod.rs:195, plus `requires_durable_owner_approval`; WS05 replaces the body with declared classes), `args_digest` (blake3 of canonical JSON, content-free).
- `execution-core/src/runtime_loop/closers.rs` - NEW: `close_dangling(fold_calls, ledger_rows) -> Vec<ToolStepResult>`: no row -> re-dispatch (class read/none) or `NeverStarted`; `reserved` -> `UnknownOutcome` (never retried: `retry::tool_call_is_retryable` rules, retry.rs:149, still forbid replaying writes); `completed` without captured output -> `ResultLost`; `not_run` -> re-dispatch; `browser_agent` -> `NotResumableTool`; `code_interpreter`/`process_*` with a missing lease -> `WorkspaceStateLost`.
- `execution-core/src/runtime_loop/agent.rs` - around the dispatch at 1919: reserve before an effectful call, finalize after (`completed|failed|not_run` when the gate returned `awaiting_approval`). Best-effort off the hot path for reads; fail-closed (call not dispatched, tool error "effect ledger unavailable") for effectful calls when the flag is on.
**Flag.** `EXECUTION_EFFECT_LEDGER_ENABLED`, default false.
**ZDR.** Rows carry tool name, class, digest only (no arguments, no output); the Do loop is ZDR-refused anyway; the rows are skipped for ZDR with the existing `zdr` column.
**Tests.** unit for closers (table of ledger state x class -> closer text); session-core DB-gated ledger transitions; `cargo test -q -p execution-core --all-targets`; golden text fixtures for closer sentences in `mp-contracts` (S1 type).
**DoD.** Killing the process between reserve and finalize leaves `reserved`; the sweep or a claim turns it into `unknown_outcome`; closer text contains the class and tool name and never the arguments.
**Evidence.** rebuilt images; `docker kill execution-core` mid effectful call; `psql` shows `reserved` -> `unknown_outcome`, closer text visible in the next segment's log (content-free) and the model-facing message in the fixture test.
**Effort.** M, 5-7 d. **Deps.** S1; WS05 (consumes classes; stub ships first). **Risks.** the audit-event schema for `FinalizeToolAction` feeds WS06's hash chain: new outcomes must be added there in the same PR (section 5).

### S6 - Fold writer in the Do loop [BUILD]
**Goal.** Persist the fold every round and before effectful dispatch and at pause, with no behavior change when the flag is off.
**Files.**
- `execution-core/src/runtime_loop/fold.rs` - NEW: `LoopState` (the locals at agent.rs:1184-1245 as one struct), `to_fold`, `from_fold`, `FoldWriter` (calls `SaveLoopCheckpoint` with the session bearer, `scrub::scrub_json_value` (scrub.rs:114) before serialization, enforces size by running `clear_stale_tool_results` (`compaction_budget`) once then giving up with `resumable=false`).
- `execution-core/src/runtime_loop/agent.rs` - refactor `run_rounds` to hold `LoopState`; checkpoint kinds: `round` after the append at 2125 (best-effort), `pre_effect` before phase-2 dispatch when any slot is effectful (fail-closed under flag: no dispatch if it cannot be written), `pause` before `CreateApproval` inside `pause_for_approval` (4469-4545), carrying the round's assistant content, completed outcomes and the pending call. If the pause fold cannot be saved, the approval is created in `segment` mode (stage 1 fallback), never lost. Depth > 0 loops never checkpoint (subagent replay semantics, ledger 2026-08-24 "a replayed delegation resumes; it never re-runs").
- `execution-core/src/lib.rs` - `pub mod` for the new modules.
- `execution-core/tests/scrub_checkpoint_test.rs` (exists, :68) - extend: serialized fold contains no secrets and no bearer/JWT-shaped strings.
**Flag.** `EXECUTION_LOOP_CHECKPOINT_ENABLED`, default false; requires `SESSION_CORE_LOOP_CHECKPOINT_ENABLED`.
**ZDR.** Writer is never constructed when `req.zdr` or the route is waived; compile-time-free runtime assertion mirrors `continuation_descriptor` (4558-4562); matrix test in S9.
**Tests.** the existing agent.rs loop tests must pass unchanged with the flag off (regression gate); new: checkpoint cadence (N rounds -> N `round` saves), pre-effect fail-closed, pause saves before approval, oversized fold degrades to stage 1; `cargo test -q -p execution-core --all-targets`; `compaction_parity_contract.rs` still green.
**DoD.** Flag on: a 3-round run leaves exactly one session-core row with the latest seq; flag off: byte-identical behavior.
**Evidence.** rebuilt execution-core + session-core; run a 3-round Do run; `psql` shows one row, `seq` = rounds, `kind` transitions; latency delta per round measured and recorded (budget: one local RPC, log p50/p95).
**Effort.** L, 6-9 d. **Deps.** S4, S5 (pre_effect needs ledger), S1. **Risks.** agent.rs is a conflict hot spot (section 5); per-round write latency; fold size growth.

### S7 - Stage 2: in-place resume [BUILD]
**Goal.** `/continue` restores the fold and re-enters the loop at the round boundary instead of re-seeding.
**Files.**
- `execution-core/src/runtime_loop/resume.rs` - extend: `resume_from_fold(req, fold, bearers)`: `ClaimLoopCheckpoint` (CAS) -> decrypt result -> check `schema_version`, `prompt_digest`, and that the new request's `Authority` narrows (never widens) the stored one (mode, rung, plan_mode, tool_names subset, space_id, min_privacy_tier) -> run `closers::close_dangling` -> apply the released call via `ApprovedRelease` (S3) -> rebuild the round: `assistant_content` + `format_tool_context(completed + released + closers)` in original call order -> continue at the next round without incrementing `rounds_executed`; `delegated_rounds`, `children_started`, `attempted_retrievals` restored.
- `execution-core/src/runtime_loop/agent.rs` - `run_agent_with_route`: `AgentResume.mode == "loop"` branch; on any restore failure (missing row, decrypt failure, version mismatch, authority widening) fall back to the stage-1 seeded segment when the approval is still consumable, else a typed failure; emit `CHECKPOINT_RESTORED` via session-core claim.
- `model-gateway/src/continue_run.rs` - choose `mode: "loop"` when `RunDetail.resumable` (S4) and flag on, else `segment`.
- `execution-core/src/state.rs` - no schema change; resume sets status Running through `update` (state.rs:125); document that status is per-replica cache.
**Flag.** `EXECUTION_LOOP_RESUME_ENABLED`, default false (needs S6 + S4 flags).
**ZDR.** Claim refuses ZDR/waived; no fold is read for them because none exists.
**Tests.** unit: authority narrowing table; closers integration; pause-fold -> resume yields the same next-round request messages as an uninterrupted run (message-equality test on the `Infer` request); crash tests with the session-core double; `cargo test -q -p execution-core --all-targets`.
**DoD.** Message history after resume equals the uninterrupted history up to the closers; a widened rung/mode/tool set is refused; fold-less runs still continue via stage 1.
**Evidence.** rebuilt images; (1) pause, `docker restart execution-core session-core`, approve, continue: completes and the Infer request log shows the restored history length; (2) two parallel `/continue` calls: one 200 stream, one 409; (3) `docker kill execution-core` between reserve and finalize of an effectful call, then continue: the model-facing closer is `unknown_outcome` and the call was not replayed (ledger row, provider receipt count unchanged).
**Effort.** XL, 10-14 d. **Deps.** S3, S4, S5, S6, D1. **Risks.** subtle fold/rebuild divergence (mitigated by the equality test); Space workspace edits since hydrate are lost on restart (closer `WorkspaceStateLost`; WS22 owns durable workspaces).

### S8 - Parking [BUILD]
**Goal.** Provider exhaustion, credential-window expiry and crashes park the run (resumable, user-present) instead of failing it.
**Files.**
- `execution-core/src/runtime_loop/agent.rs` - `RoundsOutcome` (440-458) gains `Parked(ParkReason)`; mapping: `OutOfTime` with cause Credential (`run_budget.rs` `DeadlineCause`) -> park `CredentialWindow` (wall-clock cause stays a failure); typed subscription refusal (`subscription_refusal.rs:48-69`) -> park `ProviderExhausted` (usage_limit, rate_limited with `retry_after_ms`), `UpstreamUnavailable`, `ReauthRequired`; for unpinned runs an exhausted inference ladder -> park `ProviderExhausted` (flag-gated). Writes a `park` fold (S6 writer) and returns a new response status `parked`; `run_agent_with_route` (929-932) returns it like `Paused`.
- `model-gateway/src/sse.rs` - `classify_agentic_run_outcome` (8872-8881) adds `Parked`; emit a raw `run_parked` SSE frame next to `awaiting_approval` (9523-9546) with `{run_id, reason, retry_after_ms?, resets_at?}` (raw frame, no new ChatEvent variant; WS15 promotes it).
- `MP/proto/model_plane/v1/orchestration.proto` - `OrchestrationEvent` oneof `RunParked run_parked = 24` and `RunResumed run_resumed = 25` (next free after 23); `session-core/src/orchestration_grpc.rs` broadcasts them from the park/claim handlers.
- `session-core/src/terminalization.rs` - recovery worker (1601-1772): a due run with a resumable `active` fold is parked (`CrashInterrupted`) and its obligation deadline pushed to the park TTL instead of terminalized; any run with `paused|parked|resuming` fold state is treated like an open approval in `claim_due_recovery` (1617-1623) and `has_pending_approval` (1645-1659); park TTL expiry (default 24 h, env) falls through to `outcome_unknown` so nothing leaks.
- `session-core/src/loop_checkpoint.rs` - park helpers; `claim` accepts `parked`.
- `FE/src/shared/api/chat-client.ts` (M +53 live) - handle `run_parked` frame minimally (surface as the existing awaiting state with a "Continue" action); full cues are WS15/WS16.
**Flag.** `EXECUTION_LOOP_PARKING_ENABLED`, default false; `SESSION_CORE_PARK_TTL_SECONDS` default 86400.
**ZDR.** Parking requires a persisted fold, so ZDR/waived runs never park: they fail as today with the existing typed refusals.
**Tests.** unit: outcome mapping table, TTL expiry terminalizes, recovery worker park-vs-terminalize (DB-gated: `cargo test -p session-core --bin session-core recover_parks -- --ignored --test-threads=1`), `cargo test -q -p execution-core -p model-gateway -p session-core --all-targets`, `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (must stay green: raw frame only).
**DoD.** A pinned run with a spent usage limit parks with `resets_at`; `/continue` after the reset resumes it; a killed execution-core becomes `parked(crash_interrupted)` within the deadline and resumes by user click; unclaimed parks terminalize after TTL.
**Evidence.** rebuilt images; (1) fake inference-core returning a typed `subscription_usage_limit` trailer; (2) `docker kill execution-core` mid-run and wait past the 15 min heartbeat deadline (shorten via test env for the proof run and record it); capture `psql` fold state transitions and the `run_parked` frame.
**Effort.** M-L, 6-8 d. **Deps.** S4, S6, S7. **Risks.** terminalization.rs is live-modified and safety-critical (honest outcomes); changing "terminalize" to "park" must keep the immutable-receipt invariants (supersede logic 1139-1160).

### S9 - Proof scripts, matrix test, contract tests [BUILD]
**Goal.** Repeatable evidence and regression gates.
**Files.**
- `MP/scripts/tests/durable-loop-proof.sh` - NEW, modelled on `approval-continuation-proof.sh` (disposable Postgres, no real credentials): runs the DB-gated session-core tests for S2/S4/S5/S8.
- `MP/scripts/tests/durable-loop-live-proof.sh` - NEW: drives the live-stack flows in S3/S7/S8 evidence and prints captured artifacts (this is the deployment claim runner; not in CI).
- `execution-core/tests/loop_fold_zdr_matrix.rs` - NEW: iterates the `enforcement_rows` of `MP/api/contracts/zdr-posture-matrix.json` through the mp-contracts resolver and asserts a fold/consume/park request is refused unless the decision is `persistent`.
- `execution-core/tests/durable_loop_contract.rs` - NEW (cross-service invariant pattern, `cross_service_loop_contract.rs`): gateway-side `AgentResume` builder and execution-core validator agree on fields; the two loops stay separate (gateway `tool_loop.rs` has no resume path).
- `.github/workflows/model-plane-rust-ci.yml` - nothing new needed (tests run under `cargo test --workspace`); add the DB-gated job to the existing receipts job only if WS01 chooses (not required).
**Tests.** `cargo test --workspace --no-fail-fast` in `MPR`; proof scripts; BFF and FE suites from S3.
**DoD / Evidence.** Dated evidence bundle recorded in `MP/MODEL_PLANE_STATUS.md` as source-verified or live-verified per its evidence states, never as production evidence; capability rows stay `source_only` until candidate/rollback evidence exists (`MP/docs/CAPABILITY_PROMOTION_LEDGER.tsv`).
**Effort.** M, 4-5 d (spread across slices). **Deps.** all.

**Totals.** S1 3 + S2 5 + S3 10 + S4 7 + S5 6 + S6 8 + S7 12 + S8 7 + S9 5 = about 63 engineer-days. With S2/S4/S5 (session-core) parallel to S3 (gateway/execution) two engineers deliver stage 1 in about 3 weeks and the full set in about 8-9 weeks.

## 5. Interfaces

**PROVIDES.**
| To | Contract |
|---|---|
| WS13 approvals v2 | `ConsumeApproval` single-use release; `ApprovedRelease` gate (the only way a granted call passes `execute_step_inner`); `continuation_mode`; `/continue` route; "this run" grants should call the same consume with a standing-grant check added by WS13 |
| WS14 hitl | `PendingPause{kind: input}` in the fold; `/continue` body reserved field `answer`; `ParkReason`/closer enums; ask_user budget must be a field in `extensions`; ZDR rule: fold-less in-stream ask or none (audit card 011) |
| WS15 run lifecycle | `OrchestrationEvent.RunParked=24`, `RunResumed=25`; `RunDetail.resumable=17`, `park_reason=18`; raw `run_parked` frame to promote into the typed taxonomy |
| WS16 attention | `RunParked`/`RunPausedForApproval` content-free triggers; `resumable` flag |
| WS20 scheduled runs | parked/claim contract; unattended resume explicitly needs OD8 or Control reauthorizer and is not implemented here |
| WS22 sandbox computer | lease is not released on pause (agent.rs 929-932); closer `WorkspaceStateLost`; WS22 owns durable workspace |
| WS25 policy/loop guard | `LoopFoldV1.extensions` for LoopGuard counters and policy digest binding |
| WS06 audit chain | new ledger outcomes `not_run`, `unknown_outcome` and events `CHECKPOINT_SAVED/RESTORED`, `RunResumed` must enter the chain schema |
| WS27 cost meter | per-segment usage must continue under the same `run_id`; fold `extensions.metering` placeholder |

**CONSUMES.**
| From | Contract |
|---|---|
| WS02 | shared context resolver output reused when `/continue` rebuilds `RunAgentRequest`; field numbering and the 27 `RunAgentRequest` literal sites (WS02 sweep) |
| WS05 | declared tool effect classes replace the stub in `effect.rs`; MCP idempotency declarations decide `Denied/UnknownOutcome` policy for MCP tools (no exactly-once promise for MCP) |
| WS04 | shared cancel/queue state so a cancel reaches a resumed segment on another replica (execution-core `StateStore` is per-replica memory, state.rs:100-106); `/continue` registers with the cancel registry like `agentic_run_stream` |
| WS01 | deploy/rebuild harness and flag flip procedure |
| WS12 | CI gates for the new FE action contract and golden fixtures |

**Conflict hot spots and edit-first rule.**
| File | Others touching | Rule |
|---|---|---|
| `MP/proto/model_plane/v1/execution.proto` (RunAgentRequest next free 18) | WS02, WS05, WS13, WS25 | WS02 first (context fields, mechanical `..Default::default()` conversion of the 27 literal sites), then WS03 `resume=18` (or next free at merge time; integrator assigns) |
| `execution-core/src/runtime_loop/agent.rs`, `grpc.rs`, `mod.rs` | WS02, WS05, WS13, WS24, WS25 | land the live-only M changes first; WS03 S6 (LoopState refactor) merges as one early PR to minimise rebase cost for others; later slices rebase |
| `model-gateway/src/sse.rs`, `http_routes.rs` | WS02, WS15, WS09 | WS02 first (composer), WS03 adds `Option<AgentResume>` parameters |
| `MP/proto/model_plane/v1/sessions.proto`, `orchestration.proto` | WS14, WS15, WS17, WS06 | append-only; integrator assigns migration numbers and field numbers |
| `session-core/src/terminalization.rs` | WS15, WS16 | WS03 S8 owns the park-vs-terminalize change; others rebase |
| `FE/apps/gateway/src/domains/chat/streams.rs`, `chat.rs`, `FE/src/shared/api/orchestration-client.ts` | WS13, WS14 | WS03 adds `continue_run_stream`/`continueRun` first; cards consume them |

## 6. Cross-session interfaces

**(a) Verevon Space session.** Shared piece: approvals and Space-scoped runs. Assumption: Space thread scope can be rebuilt server-side from a thread id (today `shared::sandbox_token(&state,&user,&headers,&outbound_body)` needs a body Control has scoped, `streams.rs:112`; `inject_personal_thread_context` consumes Space reference fields). WS03 ships `/continue` for personal Do runs and refuses Space runs with `continue_space_scope_unsupported` until the Space session delivers a `space_scope_for_thread(thread_id)` helper in the BFF (owner: Space session, it owns Control decisions). Approvals in Space threads remain run-owner only here; collaborator approval and the grant store split belong to WS13/Space (overlap table row 2: Model Plane owns record, scope, cause, resume; Control owns only the signed standing grant). Both audits' "resume strategy" conflict (widen kinds vs loop checkpoint) is resolved here as loop-level; the Space plan should not add resumable kinds to the cold worker.

**(b) Quarry v2 / browser session.** Shared piece: browser runs and grants. WS03 explicitly does not resume `browser_agent` (closer `NotResumableTool`); the Do loop carries no browser grant (sweep, `runtime_loop/mod.rs:272-280, 392-401`). Assumption: Quarry/WS21 deliver durable grants (overlap-quarry row 1) and a browser run that survives a pause before any browser resume is attempted; `human_assist` approvals (F-t3-026) reuse `/continue` and the same consume. Owner of grant durability: Model Plane broker plan, not this stream.

## 7. Regression guards (audit section 8 strengths touched)

| Strength | How touched | Gate |
|---|---|---|
| Signed, fail-closed capability gate | released-call gate converts only `Ask` to proceed for one exact call after a consume; `Deny` and missing evidence still refuse | unit tests in `execute_step_inner` neighbourhood (grant passes once; changed args, other tool, second use, Deny all refuse); `autonomy_ladder_contract.rs` unchanged and green |
| Autonomy ladder / Ask-Do switch | resume authority may only narrow (`Authority::narrows`) | S7 narrowing table test; `check_autonomy_rung` still runs per call (agent.rs 1685-1706) |
| Two loops kept separate | resume lives in execution-core; gateway `tool_loop.rs` untouched; `/continue` only calls RunAgent | `durable_loop_contract.rs`; `cross_service_loop_contract.rs` |
| ZDR by construction | every new store/route refuses ZDR and waived; typed refusals reuse features `agent_run`, `approval_continuation` | `loop_fold_zdr_matrix.rs` over `zdr-posture-matrix.json`; existing `zdr-contract-sync.test.sh`; `bash scripts/tests/zdr-contract-sync.test.sh` |
| Honest outcomes | closers say "unknown outcome" instead of retrying or inventing success; terminal receipt stays single and immutable | closer fixture tests; terminalization tests (supersede, immutable receipt) re-run in S8 |
| Credential isolation | no bearer is persisted in the fold, ledger, park or events | extended `scrub_checkpoint_test.rs` + `assert_no_credentials` in the golden |
| Typed ChatEvent taxonomy | no new ChatEvent variant (raw frames only) | `gen-sse-event-taxonomy.py --check` |
| Model proposes, Quarry executes | `browser_agent` never re-dispatched | closer test `NotResumableTool` |
| Multi-tenant server runtime | claim CAS is DB-level, tenant-scoped, replica-safe | DB-gated claim race test |
| Context management | the fold keeps `clear_stale_tool_results`; compaction parity unchanged | `compaction_parity_contract.rs` |

## 8. Effort and risks

**Total.** XL, about 63 engineer-days (card says L for the core slice and XL when closers, ledger, parking are bundled; this matches). **Critical path.** D1/D2 rulings -> S1 -> S2 -> S3 (stage 1 live) -> S6 -> S7 -> S8; S4 and S5 run in parallel to S3 and must finish before S6.

**Top risks.**
1. **Hot-file conflicts and live-only work.** agent.rs, grpc.rs, terminalization.rs, sessions.proto, sse.rs carry large uncommitted changes. Mitigation: land the live set first (section 2 table), merge the LoopState refactor early, add-only proto edits, integrator assigns numbers.
2. **Double execution.** Cold worker plus in-loop release on the same grant, or a replay after a crash. Mitigation: D4 mode + outbox skip (S2), single-use consume, ledger + closers (S5), no replay of effectful calls, MCP and browser excluded from exactly-once claims.
3. **Rollback strands paused runs.** Flag off after approvals were created in loop/segment mode leaves them unconsumable. Mitigation: `/continue` kill switch returns typed `continue_disabled`; park/approval TTL terminalizes as `outcome_unknown`; ship the flag as flip-forward-only after the dated proof; document in the ADR.
4. **Fold correctness and size.** Resumed history diverging from the uninterrupted one, or folds near 4 MiB. Mitigation: message-equality test, in-fold compaction, fall back to stage 1 on any restore failure, `resumable=false` rather than partial writes.
5. **Authority drift / credential window.** Resuming with wider authority or relying on a 300 s bearer. Mitigation: narrow-only authority tuple, bearers minted per click and never stored, S8 parks at the credential margin; unattended resume stays out of scope until OD8/Control reauthorizer (WS20).
