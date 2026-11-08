# WS20-scheduled-runs - Scheduled runs as governed, tool-using Do runs

Legend: MP = `apps/Model Plane`; FE = `apps/Frontend Plane/verevonv3`; CP = `apps/Control Plane`; AP = `apps/Application Plane`; RS = `MP/rust/services`; GO = `MP/go/services`. Citations opened at worktree HEAD afb83928 unless marked [live]. Plan doc = `FE/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md` (sweep cited it by bare name).

## 1. Scope, items resolved, owner

**Item:** F-t3-018 (Priority, L by audit; this design sizes it L-XL, see section 8). **Owner:** Model Plane (orchestrator-core supervision, execution-core run, session-core run record and receipts, capability-core schedules/fires/notify producer). **Control Plane (user-core)** signs the consent contract binding. **Application Plane (notification-core)** delivers off-surface. **Frontend Plane** consent card, "See results", origin chip.

**IN**
- Replace the tool-less step with a budgeted, tool-using Do run driven by the same governed driver (`run_agent_with_tools`) and the same capability/permission pipeline as chat Do, from a new service-owned branch of `ExecuteScheduledStep` (no user bearer exists in an unattended run, so literally calling the `RunAgent` RPC is impossible: it needs four user-delegated bearers, `RS/execution-core/src/grpc.rs:1207-1232`).
- A stored approval contract (per-tool, class-capped, expiring) committed in the Control-signed template digest, re-verified at create, fire and every step.
- A non-delegable tool set derived from WS05 classes.
- Draft-and-notify instead of fail-closed when an effect is not covered; off-surface notice through notification-core; no Allow action anywhere.
- Working "See results" (needs result persistence + truthful fire status first) and a run-level origin tag {chat, schedule, trigger, api}.
- ADR amending the 2026-09-28 draft-only ruling; resolution of OD8/OD9 for this stream.

**OUT**
- Any unattended write/external effect (contract ceiling stays `read`; widening is a separate ADR).
- Owner-bound data reads (`knowledge_search`, personal connectors) and runs longer than ~240 s: need OD8 renewable bearer or Control data-read decision; designed as S9, not scheduled.
- Event triggers (F-t3-047), goals (F-t3-037), parking/resume of a scheduled run (WS03), shared-Space schedules (Space session Control decision family), "Run now" (F-t3-019 phase 1), org-wide quotas beyond the guards in S8.

| Slice | Tag | One line |
|---|---|---|
| S0 | [FINISH] | Land-first, ADR + ledger, characterise today's lane |
| S1 | [FINISH] | Persist the scheduled result + truthful fire status (plan D5/D6) for the existing lane |
| S2 | [BUILD] | Run-level origin enum and stamp |
| S3 | [FINISH] | "See results" (BFF, fires endpoint, panel) |
| S4 | [BUILD] | Consent contract: schema, Control binding, create-time verification, consent card |
| S5 | [BUILD] | Carry and re-verify the contract at fire and step (decision v3) |
| S6 | [BUILD] | Scheduled Do run in execution-core |
| S7 | [BUILD] | Off-surface outcome notice |
| S8 | [BUILD] | Guards: expiry/failure auto-pause, renewal |
| S9 | [BUILD, gated] | Owner-bound reads (design only) |

## 2. Verified current state

**Pipeline today (all opened).** Capability-core sweeper inserts a task with the non-secret fire intent and a `cron_fires` row (`GO/capability-core/internal/cron/sweeper.go:226-262`, status `'pending'` at 261, never updated afterwards). The dispatcher re-authorizes the fire with Control and attaches the prepared run (`GO/capability-core/internal/taskexec/workflow_dispatcher.go:215-236`, guard 228-232). Orchestrator `buildScheduledRunInput` re-hashes the template and derives only `{goal, policy}` (`GO/orchestrator-core/internal/orchestration/workflowreg.go:249-330`; `PolicyDigest` = sha256 of the policy string). `ScheduledRunSupervision` (`GO/orchestrator-core/cmd/workflows/interactive_run.go:188-259`) starts the run via `StartScheduledRunActivity`, loops `executeScheduledStepLoop` (`cmd/workflows/step_loop.go:97-136`, up to `defaultMaxTurns`=10 single-inference steps), fails the run on `NeedsApproval` (242-243), then `CompleteRunActivity` with a **count-only summary** (`step_loop.go:134`).

**The step** (`RS/execution-core/src/grpc.rs:238-451`): claim receipt in session-core, read goal via `GetScheduledStepContext`, one `Infer` (`scheduled_inference_request` 487-520: model `verevon-balance`, `max_tokens 1024`, `tool_choice none`, 60 s timeout), record receipt with **output digest only** (407-418). The model output is returned to Temporal and **never written to the thread** (grep: no `AppendMessage` in the scheduled path). So "See results" has nothing to show even if enabled: the audit missed this; plan rows D5/D6 are real prerequisites.

**Control side.** Create/fire/run/step decisions are Ed25519 tokens with `permissions`, `zero_data_retention`, expiry; all refuse ZDR (`CP/user-core/internal/spaces/schedule_create_decision.go:76-97`, step 151-174). The create payload binds only `template_digest` (45-71): Control never sees the template body. There is no tool or approval field anywhere in `controlDecision` (`GO/capability-core/internal/cron/control_authorizer.go:147-170`). Template digest is checked three times (gateway `FE/apps/gateway/src/domains/spaces/schedule_template.rs:1-30`, dispatcher, orchestrator).

**Execution-core verifier** `RS/execution-core/src/scheduled_step_decision.rs:36-60`: `Claims` is `#[serde(deny_unknown_fields)]`, `DECISION_VERSION "v2"`: **new claims break an old exec-core**; deploy order matters (S5).

**Reusable seams (opened).** `run_agent_with_tools` takes an explicit tool Vec, optional bearers and a `RunDeadline` (`RS/execution-core/src/runtime_loop/agent.rs:743-800`, private today); the purpose-lock allowlist is derived from that Vec (850); the per-call autonomy-rung check refuses effects before dispatch and feeds the refusal back to the model (agent.rs:1664-1700, `CallSlot::Rejected`); a gated call mints a durable approval (`pause_for_approval`, 2086-2100) which must never happen for a system-owned run; `finalize` writes the answer and a managed terminal receipt using service-minted tokens (`ManagedRunTokenProvider`, `session_terminal_auth.rs:84-100`); `RunDeadline::wall_clock_after` exists (`run_budget.rs:124`); per-run state is keyed by `run_id` in `StateStore` (`state.rs:101,214`). Scheduled inference uses a service credential scoped `inference:invoke` (`scheduled_inference_auth.rs:19-21`).

**Run record.** `persist_scheduled_run_bindings` writes immutable `runs.metadata` `{source:"scheduled_run", schedule_id, fire_key, space_id, subject_id, template_digest, policy_digest, idempotency_key}` (`RS/session-core/src/grpc.rs:1378-1446`); run owner is `service:orchestrator-core`; the thread `origin` is resolved server-side (`resolve_thread_origin`, 2314-2342: space_id set -> `space`). So a run-level tag partly exists as `metadata.source`; it is not a closed enum, not on chat runs, not surfaced.

**FE.** `ScheduledRunsPanel.tsx:135-142` "See results" is unconditionally `disabled`; `schedule-from-chat.ts:30-40` sends `task_template {title, description, ...}`; gateway create path is `FE/apps/gateway/src/domains/agent_actions.rs:457-575` calling `inject_personal_schedule_create_context` (`domains/spaces.rs:426-589`). No fires listing endpoint exists (grep `/fires`: none); `GET /v1/cron` returns schedules only (`GO/capability-core/internal/api/workplane_apis.go:894-933`).

**[live-only] files (must land first or be rebased on).**

| Live path | Why it matters |
|---|---|
| `MP/rust/crates/mp-contracts/src/zdr_refusal.rs` (??) | Live `authorize_scheduled_step_service` ZDR refusal (execution-core grpc.rs [live] 833-838) calls `Refusal::feature_not_supported` (392). Committing grpc.rs without it breaks the build. |
| `RS/execution-core/src/grpc.rs` (M 98), `runtime_loop/agent.rs` (M 207), `auth.rs` (M 631) | S5/S6 edit the same functions; shifts scheduled lines by +11. |
| `RS/session-core/src/grpc.rs` (M 861), `auth.rs`, `terminalization.rs` | S1/S2/S5 edit `persist_scheduled_run_bindings`, `start_scheduled_run` (wt 4414, live 4582). |
| `MP/proto/model_plane/v1/sessions.proto` (M +42, `CountWaivedRuns` 196-206, msgs 1219-1249) | S1/S5 append RPCs/fields after it. |
| session-core migrations `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql` (??) | New migrations are 0044 in wt but 0046 in live: integrator assigns. |
| `FE/src/features/chat/components/ScheduleDialog.tsx` (M +24, Task textarea + goal gate), `ScheduleDialog.test.tsx` (M +16) | S4 consent card goes in this file. |
| `MP/api/contracts/zdr-posture-matrix.json` + Rust matrix test (live-only) | S1/S6 ZDR golden rows. |

**Where audit/sweep text is stale (code wins).**
1. Card :610 "D3-D11 planned and unbuilt": D9 and D12 are built (plan :9, :119-120); D3-D8, D10, D11 have no code.
2. Plan D4 prescribes a "bounded read-only tool loop" inside the scheduled step; the audit says "same RunAgent path". They conflict; section 3 D3 resolves it.
3. `docs/system-run-context.md` (2026-07-15) proposes a generic `task-runner` service principal for headless runs. That is exactly the "service-principal shortcut" the audit rejects; the shipped lane is per-service credentials plus per-step Control decisions. Implementation status of that doc: not determined (no ledger entry).
4. "ADR unattended authority" (audit 9.1) does not exist; the draft-only ruling lives only at plan :68 and OD9 at :457 (recommends v1 "web only, draft-only, NeedsApproval fails the run").
5. The scheduled step output is not persisted anywhere readable (above).
6. `go.work` is tracked (`MP/go/go.work`): the memory note "no go.work" is stale.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates |
|---|---|---|---|---|
| D1 | ADR amending 2026-09-28 "scheduled runs are draft-only" (also settles audit 9.1 "unattended authority") | (a) stay tool-less; (b) stored-consent contract, class ceiling `read` in v1, widening only by a later ADR; (c) generic task-runner principal (`system-run-context.md`) | **(b)**. The ruling is amended, not reversed: v1 still never takes an effect; what changes is that read tools run. Reject (c) explicitly in the ledger (entry also archives the unimplemented parts of `system-run-context.md` for schedules). | S4-S8. S0-S3 proceed without it. |
| D2 | OD8 renewable bearer | needed now / decouple | **Decouple.** The scheduled lane mints only service credentials (session scheduled-step, inference) and carries no user-delegated data-plane/sandbox bearer, so the 300 s ceiling is met by a hard 240 s wall clock (`RunDeadline::wall_clock_after`). OD8 gates only S9. | S9 only |
| D3 | Where the Do loop runs | (a) new branch of `ExecuteScheduledStep` calling `run_agent_with_tools` with a contract-filtered tool Vec and read-only rung; (b) real `RunAgent` RPC (impossible: needs user bearers); (c) plan D4's separate bounded loop in the scheduled path | **(a).** One driver, one gate, no third loop (CLAUDE.md two-loops rule: the refusal boundary is the authority boundary; model-gateway `dispatch_tool` untouched). Supersedes plan D4's wording. | S6 |
| D4 | Contract home | (a) in the template, Control signs `contract_digest` + `contract_expires_at` + class ceiling, capability-core verifies body vs registry; (b) new Control table holding the body | **(a)** (audit 9.4 item 5 text; Control never needs content). | S4, S5 |
| D5 | Expiry | unset = never (Claude desktop) / required | **Required.** Default 30 d, hard max 90 d (`SCHEDULE_CONTRACT_MAX_DAYS` in user-core, env, no org-core dependency yet), renewal = re-consent. | S4, S8 |
| D6 | "Draft" meaning | (a) transcript-only: withheld effect + args written into the result, owner continues in chat under live approval; (b) durable approval rows resumed by WS03 | **(a).** (b) needs a human-decidable approval owned by a non-human run owner and OD8. No Allow action exists in any notification (they are content-free deep links, WS16). | S6, S7 |
| D7 | Notification vocabulary | reuse WS16 `modelplane.needs_approval` / new type | **One new type `modelplane.scheduled_run_outcome`**, `payload.outcome` in `{completed, needs_review, failed, paused_expired, paused_failures}`; "needs approval" is wrong (nothing is approvable in place). Rides WS16 S3/S4 plumbing and allowlist. | S7 |
| D8 | Instructions for an unattended run (WS02 handoff) | none / digest-committed snapshot in the template / live fetch with a service credential | **Snapshot in template** (`run_instructions`, <= `MAX_AUTHORED_INSTRUCTION_BYTES` 16384, composed by the same `compose_authored_instructions` WS02/Space use), tamper-proof via template digest, bounded by contract expiry, labelled "as of <date>". If WS02 S4 is late, v1 ships goal-only. | S6 step 6 |
| D9 | Run origin enum | per-plane vocabularies | One closed enum `{chat, schedule, trigger, api}` in `runs.metadata.origin`, owned by Model Plane (overlap C9); Control's `turn_origin {direct, ambient, automation}` maps onto it (Space session owns the mapping). Thread `origin` (6 values) unchanged. | S2 |
| D10 | Budget | - | `max_rounds` = min(contract, 8) (loop default 12, ceiling 32: `agent.rs:270-274`), `max_subagents` 0, wall clock 240 s, `max_questions` 0 (WS14), per-round inference cap from contract (<= 4096). WS27 cost cap consumed when it lands. | S6 |
| D11 | WS10 acts-for-user | exempt service / owner principal | Scheduled runs pin `verevon-balance` (not user-selectable), so WS10's service exemption (its D6) is safe; Control's step decision already re-checks `ScheduleFireEntitled` and `AgentActionEntitled` (`scheduled_step_decision.go:163`). Revisit if a scheduled run ever takes a model field. | none |

**Proceed without any decision:** S0 (writes the ADR), S1, S2, S3.

**Ledger reversal:** MP `docs/decisions/ledger.md` gets a new top entry "Scheduled runs: stored-consent read contract amends the draft-only ruling" (state `proposed`, flips to `implemented` after S6 evidence) and marks nothing else archived; the 2026-10-03 ZDR waiver entry's "never scheduled or unattended" clause stays true and is restated.

## 4. Slices (PR-sized, in build order)

### S0 [FINISH] Land-first, ADR, characterise (S, 1-2 d)
**Goal.** Pin today's behaviour and record the decision.
**Files**
- MP `docs/decisions/ledger.md` - ADR entry per D1-D6 (options, rejected `task-runner`, trigger to widen the class ceiling).
- (The repo has no `MP/docs/adr/` directory; the ledger is the ADR home.)
- `MP/docs/DEV_AUTHORITY_HANDOFF_MATRIX.md` - add contract/step-decision v3 rows (S5).
- Plan doc :68 and :457 - cross-reference the ledger entry.
- `RS/execution-core/src/grpc.rs` test `scheduled_step_runtime_is_tool_free_and_bounded_to_unknown_on_transport_loss` (1804) stays as the legacy-lane characterisation (kept green behind the flag in S6).
**Land first:** WS01 S0 commits `zdr_refusal.rs` and the live execution-core/session-core wave.
**Flag/ZDR/migration:** none; ZDR clause restated in the ledger.
**Tests:** none new. **DoD:** ledger entry reviewed by the owner; D1 answered. **Evidence:** n/a (docs). **Deps:** WS01 S0. **Risk:** owner does not accept D1 -> S4-S8 stay parked, S1-S3 still ship value.

### S1 [FINISH] Persist the result and make fire status truthful (M, 5-7 d)
**Goal.** The existing tool-less lane ends with a readable transcript and an honest `cron_fires` row (plan D5 + D6). Without this "See results" cannot work.
**Files**
- `MP/proto/model_plane/v1/sessions.proto` - append `rpc RecordScheduledRunResult(RecordScheduledRunResultRequest) returns (RecordScheduledRunResultResponse)` at the end of `SessionCore` (after the live `CountWaivedRuns`); request `{run_id=1, step_id=2, org_id=3, idempotency_key=4, receipt_id=5, status=6, output=7, error_code=8, unknown_outcome=9, rounds_executed=10, withheld_effects=11, tool_calls=12, input_tokens=13, output_tokens=14}`; response `{receipt_id=1, status=2, message_id=3}`. `output` is content: carried only on this direct hop.
- `RS/session-core/src/grpc.rs` - handler beside `claim_scheduled_step_inner`/`record_scheduled_step_receipt_inner` (wt 1142/1280): scope `session:scheduled-step` + service `service:execution-core` (reuse `auth.rs:257-277,312`); in ONE transaction: verify the claim row matches (same checks as the existing receipt path), refuse when the run's retention is ZDR/waived, append the assistant message to the run's thread through the existing append path with a `source_scope` that is not `conversation` (no user-scope memory extraction; overlap C6, `0038_thread_source_scope.sql`), complete the receipt, update the matching `cron_fires` row. Idempotent on `(run_id, step_id)`.
- `RS/session-core/src/terminalization.rs` - on terminal state of a run with `metadata.source='scheduled_run'` set `cron_fires.status/completed_at/error` for `(schedule_id, fire_key)`.
- NEW `RS/session-core/migrations/<next>_cron_fire_run_link.sql` - `ALTER TABLE cron_fires ADD COLUMN IF NOT EXISTS fire_key TEXT, run_id TEXT, thread_id TEXT, withheld_effects INT NOT NULL DEFAULT 0, outcome TEXT`; index `(schedule_id, fired_at DESC)` exists (0004:159); backfill none.
- `GO/capability-core/internal/cron/sweeper.go:260` - insert `fire_key` (same RFC3339Nano as the intent, 241) so the run can be matched.
- `RS/execution-core/src/grpc.rs` (`execute_scheduled_step_runtime` 238-451) - on `Ok`, call `RecordScheduledRunResult` with the output instead of `record_scheduled_step_receipt` (keep the old call as the flag-off path).
- `GO/orchestrator-core/cmd/activities/activities.go` - no change; `step_loop.go:134` summary stays count-only (content never enters Temporal history).
- `GO/capability-core/internal/api/workplane_apis.go` - NEW `GET /v1/cron/{id}/fires?limit=` returning `{id, fired_at, status, outcome, completed_at, run_id, thread_id, withheld_effects}`; org-scoped via `verifiedOrganizationID` (as 894-900) and the creator check used by `get` (995).
**Flag.** `SESSION_CORE_SCHEDULED_RESULT_PERSIST` default off; rollback: off returns to receipt-only. **Compat:** session-core ships first (new RPC unused), then execution-core.
**ZDR clause.** Persisted: assistant message in the scheduled thread (non-ZDR only; Control already refuses ZDR create/fire/run/step, orchestrator `DeniesZDR` at `workflowreg.go:117`, exec-core live refusal). Refused: the RPC returns `failed_precondition` for ZDR/waived runs. Numeric-only columns on `cron_fires`. Golden: add `scheduled_run` row to `MP/api/contracts/zdr-posture-matrix.json` (live-only, rebase) = refuse; Rust matrix test asserts the RPC refusal.
**Tests.** session-core: NEW `record_scheduled_run_result_appends_once_and_completes_receipt` (+ Postgres variant following `scheduled-step-receipt-postgres-test.sh`), `..._refuses_zdr_run`, `..._rejects_mismatched_claim`; extend `scheduled_step_claim_and_unknown_receipt_are_idempotent_against_real_pg` (wt 7276). execution-core: `scheduled_step_persists_output_through_session_core`. capability-core: NEW `internal/api/cron_fires_test.go`, sweeper_test `TestFireRowCarriesFireKey`. Commands: `cargo test --manifest-path "MP/rust/Cargo.toml" -p session-core -p execution-core scheduled --no-fail-fast`; `cd MP/go && go test -count=1 ./services/capability-core/internal/cron/... ./services/capability-core/internal/api/...`; `MP/scripts/tests/scheduled-step-proof.sh`; `scheduled-step-receipt-postgres-test.sh`.
**DoD.** After a fire, the scheduled thread contains the answer, `cron_fires` row is `completed` with `run_id`/`thread_id`, no content in Temporal history or logs.
**Evidence.** Rebuild session-core, execution-core, capability-core via `MP/scripts/compose.sh` (memory recipe); flag on; set `next_fire_at=now()` on a dev schedule; capture `psql` of `cron_fires`, `GET /v1/cron/{id}/fires`, thread messages via GetThread, and the Temporal history (no output text).
**Effort:** M, 5-7 d. **Deps:** WS01 S0 (live session-core wave), S0. **Risks:** session-core grpc.rs conflict hot-spot; matching `fire_key` for pre-existing fires (none are matchable: leave them `pending`, S8 sweeps them to `unknown`).

### S2 [BUILD] Run-level origin (S, 2-3 d)
**Goal.** `runs.metadata.origin` closed enum; surfaced; used by S3 chip and WS15's trigger chip (WS15 D7).
**Files**
- NEW `MP/rust/crates/mp-contracts/src/run_origin.rs` (+ `pub mod` in `lib.rs`) - `RunOrigin {Chat, Schedule, Trigger, Api}`, `from_wire` (unknown -> refuse, never default upward), Go twin NEW `GO/capability-core/internal/models/run_origin.go` with shared known-answer vectors (twin pattern as WS05).
- `RS/session-core/src/grpc.rs:1405-1414` - add `"origin":"schedule"` to `expected` and to the `exact` key list (1415-1425); StartRun for user principals defaults `origin:"chat"` when absent (additive JSON merge, no migration).
- `RS/session-core` `GetRun`/`ListRuns` already return `metadata`; no proto change. `MP/docs/sse-event-taxonomy.md` - note.
- FE `src/shared/api/*` run normalizer: read `metadata.origin` (name of the file: not determined; locate via `rg "metadata" FE/src/shared/api/orchestration-client.ts` in S3 step 0).
**Flag:** none (additive). **ZDR:** closed token, not content; present on ZDR runs as `chat` only. **Tests:** mp-contracts `run_origin_table`; session-core `scheduled_run_binding_stamps_origin`, `a_run_cannot_be_rebound_to_another_origin`; Go twin vector test. **DoD:** new scheduled runs carry `origin:"schedule"`; old runs read as absent -> UI treats as chat. **Evidence:** `GetRun` JSON of a scheduled and a chat run. **Effort:** S. **Deps:** S0. **Risks:** Space `turn_origin` mapping owned elsewhere (section 6).

### S3 [FINISH] "See results" (M, 4-6 d)
**Goal.** Enable the disabled control with real data.
**Files**
- `FE/src/shared/api/cron-client.ts` (44-104) - `listScheduleFires(scheduleId)`; Zod-style normalizer; envelope `{data}`.
- NEW `FE/src/shared/actions/` contract `schedule.fires.list` (action contract before UI, CLAUDE.md; the registry's schedule entries at `action-registry.ts:872-913` are the Ingestion schedule API, not this one).
- `FE/apps/gateway/src/domains/agent_actions.rs` (list_cron 421; add `list_cron_fires` beside it) - same-origin route `GET /api/v1/cron/{id}/fires` to model-gateway `/v1/cron/{id}/fires`; strips forged scoping headers as `list_cron` does.
- `MP/rust/services/model-gateway` cron proxy (route file: not determined; locate with `rg '"/v1/cron' MP/rust/services/model-gateway/src`) - forward route.
- `FE/src/features/chat/components/ScheduledRunsPanel.tsx:135-142` - replace the disabled button with an expander listing last 5 fires (time, status, outcome chip, origin chip "Schedule", withheld count) and a link to the transcript; i18n pairs via `i18n.tr`; remove `title="Coming soon"`.
- Transcript link target: the personal-Space read path used by `SpaceWorkPanel.tsx` (wave-2 spec); verify live in step 0 that the owner can read a `service:orchestrator-core`-owned thread (plan D12, claimed built).
**Flag.** FE `VITE_SCHEDULE_RESULTS` default on once S1 is on; rollback: button stays disabled. **ZDR:** no content cached in the SPA beyond the normal transcript path. **Tests:** `ScheduledRunsPanel.test.tsx` (extend), `cron-client.test.ts` (extend), gateway `cargo test --manifest-path "FE/apps/gateway/Cargo.toml" --all-targets` (`list_cron_fires_forwards_org_scope`); Playwright `tests/e2e/scheduled-results.spec.ts` (NEW). Commands: `cd FE && pnpm vitest run src/features/chat/components/ScheduledRunsPanel.test.tsx src/shared/api/cron-client.test.ts && pnpm typecheck && pnpm lint` (pre-existing failures per memory: diff failing sets vs HEAD).
**DoD.** Owner sees the answer of last night's fire; empty/failed states honest. **Evidence.** Rebuild gateway + restart the frontend container (memory: Vite bind mount needs restart; cache-bust global.css); screenshot of the expander with a real fire from S1. **Effort:** M. **Deps:** S1, S2. **Risks:** list route returns org-wide schedules today (workplane_apis.go:894 comment): keep the fires route creator-scoped.

### S4 [BUILD] Consent contract: schema, Control binding, create-time verification, consent card (L, 8-10 d)
**Goal.** The owner approves exactly which tools an unattended run may use and until when; the approval is committed by Control and verifiable later.
**Contract (canonical JSON, key `approval_contract` in `task_template`, flat top-level key so `check_digest_stable` passes, `schedule_template.rs:83-110`):**
`{"v":1,"tools":[{"tool":"web_search","capability":"cap.tool.http","class":"read","approved_at":"<RFC3339>"}],"expires_at":"<RFC3339>","max_rounds":6,"max_tokens_per_round":2048}`. `class` is declared by the registry; unknown/`unclassified` is refused.
**Files**
- NEW `MP/rust/crates/mp-contracts/src/scheduled_contract.rs` - parse, canonicalize, `digest()`, `delegable(class)` (v1: `read` only), `NON_DELEGABLE` = `{sandbox_compute?, context_write, external_write, destructive, owner_effect, unclassified}` or any tool with `approval_floor != none` (WS05 interface); explicit never-list: `browser_agent`, `browser.*`, `process_*`, `code_interpreter`, `shell`, `tickets.create`, `book_shipment`, `mcp__*` (v1), memory/skill-write tools, schedule tools. Known-answer vectors shared with Go.
- NEW `GO/capability-core/internal/cron/contract.go` - Go twin + `VerifyContractAgainstRegistry(ctx, contract, orgID)` using WS05 `capability_tools` read surface: tool exists, registry class equals declared class, class delegable, floor none, expiry in the future and <= Control-signed expiry.
- `GO/capability-core/internal/api/workplane_apis.go` (`listOrCreate` POST ~935+, `patch` 1011) - for scoped schedules whose template has a contract: recompute digest, require it equals the Control decision's `contract_digest`, run registry verification; reject a contract on a non-scoped (legacy) schedule; PATCH of the template re-requires a fresh decision (renewal).
- `GO/capability-core/internal/cron/control_authorizer.go` (147-170, 201-228, 358-394) - `controlDecision` += `ContractDigest`, `ContractExpiresAt`, `ContractMaxClass`; `CreateIntent` += the same; create payload digest includes them (length-prefixed field pattern, same as user-core 45-71). Absent contract => digest computed as today (back-compat for tool-less schedules).
- `CP/user-core/internal/spaces/schedule_create_decision.go` (22-97) - `ScheduleCreateRequest` += `ContractDigest`, `ContractExpiresAt`, `ContractClasses []string`; `Validate`: expiry set, `> now`, `<= now + SCHEDULE_CONTRACT_MAX_DAYS` (default 90), every class in the role's ceiling (v1 constant `read`); payload digest binds them; schema hash bump to `sha256:space-cron-create-v2` only when a contract is present (v1 decisions keep verifying). `CP/user-core/internal/http/server.go:363` handler decodes the new fields.
- `FE/apps/gateway/src/domains/spaces.rs:530-575` and `agent_actions.rs:457-575` - compute the contract digest with `schedule_template::go_compatible_json_bytes`, send the new fields to user-core, verify the returned decision echoes them, strip any browser-supplied `contract_*`/decision fields (extend `FORGED_SPACE_AUTHORITY_FIELDS`).
- `FE/src/features/chat/components/ScheduleDialog.tsx` [live M] - ConsentCard: lists delegable tools fetched from NEW `GET /v1/cron/delegable-tools` (capability-core, class-derived), default selection = none ("draft-only, no tools", today's behaviour), expiry picker (default 30 d), plain-language statement "will run without asking you, read-only, until <date>; anything that would change something is written up for you instead". `schedule-from-chat.ts:30-40` adds `approval_contract`. i18n no/en.
- `FE/src/shared/actions/` - action contract `schedule.create` updated to carry `approval_contract`.
**Proto:** none. **Migration:** NEW `RS/session-core/migrations/<next>_cron_schedule_contract.sql`: `ALTER TABLE cron_schedules ADD COLUMN IF NOT EXISTS contract_digest TEXT NOT NULL DEFAULT '', contract_expires_at TIMESTAMPTZ` (query/UI only; authority stays digest + Control).
**Flag.** `SCHEDULE_CONTRACT_ENABLED` (capability-core and user-core env, default false; gateway sends the fields only when true). Rollback: off; existing schedules (no contract) unchanged. **Deploy order:** capability-core and user-core accept, then gateway sends.
**ZDR.** Contract holds tool names and dates, no content; Control still refuses ZDR create (`schedule_create_decision.go:86`). Golden: user-core `TestIssueScheduleCreateDecisionRefusesZDRWithContract`.
**Tests.** user-core `schedule_create_decision_test.go` (extend: `...BindsContractDigestAndExpiry`, `...RejectsExpiryBeyondMax`, `...RejectsClassAboveCeiling`); capability-core `contract_test.go` NEW (vectors, registry verification), `control_authorizer_test.go` (extend 146: `...RejectsContractNotBoundByControl`); mp-contracts `scheduled_contract_vectors`; gateway tests; `ScheduleDialog.test.tsx`. Commands: `cd "CP/user-core" && go test ./internal/spaces/...`; `cd MP/go && go test -count=1 ./services/capability-core/...`; `cargo test --manifest-path "MP/rust/Cargo.toml" -p mp-contracts`; FE/gateway as S3.
**DoD.** A schedule can only be created with a contract Control signed and the registry confirms; a tampered template fails digest; an over-max expiry is refused. **Evidence.** Rebuild user-core (`scripts/run-control-plane.sh`, memory), capability-core, gateway, frontend; create via the dialog; show decision claims (`contract_digest`), the stored row, and three negative calls (tampered contract, `external_write` tool, expiry 400 d). **Effort:** L. **Deps:** D1, WS05 S1 (class vocabulary + `capability_tools` read surface), S0. **Risks:** digest canonicalization across Rust/Go (mitigated by existing vectors pattern, `schedule_template.rs:200,274`); consent-card comprehension.

### S5 [BUILD] Carry and re-verify the contract at fire and step (M, 5-7 d)
**Goal.** Fire and every step re-verify the same contract; nothing wider than the signed digest can run.
**Files**
- `GO/orchestrator-core/internal/orchestration/workflowreg.go:249-330` - `scheduledTemplateGoalAndPolicy` also returns `approval_contract` JSON (it already hashed the whole template, so the body is digest-bound) -> `ScheduledRunInput` (`cmd/workflows/interactive_run.go:35-49`) += `ContractJSON string`, `ContractDigest string`, `ContractExpiresAt string` (additive; replay-safe). Contract body is non-secret (tool names, dates).
- `GO/orchestrator-core/cmd/activities/activities.go:521-566` (`StartScheduledRunActivity`) and `scheduled_run_authorizer.go`/`scheduled_step_authorizer.go` (28-46, 28-44) - pass contract fields to `StartScheduledRun` and into `ScheduledStepExecutionIntent`; digest function (199, 206) includes them.
- `MP/proto/model_plane/v1/sessions.proto` - `StartScheduledRunRequest` (last field 13) += `contract_json = 14; contract_digest = 15; contract_expires_at = 16` (string RFC3339); `ResolveScheduledStepAuthorityResponse` (runs.proto, last 14) += `contract_digest = 15; contract_expires_at = 16`. `runs.proto ScheduledStepContext` (1-5) += `contract_json = 6; contract_digest = 7; contract_expires_at = 8; origin = 9`. `ExecuteScheduledStepRequest` is NOT changed (content-free by design, execution.proto:43-47).
- `RS/session-core/src/grpc.rs:1405-1425` - bind `contract_digest` and `contract_expires_at` into immutable metadata + `exact` list; store `contract` body under `metadata.contract`; `GetScheduledStepContext` returns them. `verify_scheduled_run_execution_decision` (wt 994) checks the Control run decision carries the same digest.
- `CP/user-core/internal/spaces/scheduled_step_decision.go` (18-146) and `scheduled_run_decision.go` - `ScheduledStepIntent`/`ScheduledStepAuthority` += `ContractDigest`, `ContractExpiresAt`; `IssueScheduledStepDecision` re-checks `now < ContractExpiresAt`, entitlement, role, ceiling and binds both in `scheduledStepPayloadDigest`; decision `v2` claims += `contract_digest`, `contract_expires_at` (empty for tool-less); new action schema hash `sha256:space-scheduled-step-v2` only when a contract exists.
- `RS/execution-core/src/scheduled_step_decision.rs:36-60` - `Claims` += the two fields with `#[serde(default)]` (keep `deny_unknown_fields`); verify payload digest recomputation; verifier then compares `claims.contract_digest` with the digest recomputed from `ScheduledStepContext.contract_json` (`mp_contracts::scheduled_contract::digest`) and refuses on mismatch or expiry (`failed_precondition`, closed token `scheduled_contract_invalid|expired`).
- `GO/capability-core/internal/taskexec/workflow_dispatcher.go:327-345` - at the fire re-authorization, refuse (task failed with reason `contract_expired`) when `contract_expires_at` is past, before any Temporal start.
**Rollout order (hard):** execution-core (accepts new claims) -> session-core -> user-core (emits) -> orchestrator -> capability-core. **Flag:** `EXECUTION_CORE_SCHEDULED_CONTRACT_REQUIRED` default false (when a context has a contract, verification is mandatory regardless; the flag only controls "a schedule created with a contract but a context without one" = refuse).
**ZDR:** unchanged (all four decisions refuse ZDR). **Tests:** orchestrator `workflowreg_template_digest_test.go:25,47` extend + `TestScheduledRunInputCarriesContractFromDigestBoundTemplate`; `startworkflow_test.go:452` (`...RejectsDetachedTemplateAndForgedExecutionFields`) add contract forgery case; user-core `scheduled_step_decision_test.go` (`...BindsContractAndRefusesExpired`); execution-core `scheduled_step_contract_rejects_digest_mismatch`, `scheduled_step_contract_rejects_expired`; `MP/scripts/tests/scheduled-step-config-contract-test.sh` extended; `scheduled-step-proof.sh`. **DoD.** A forged or expired contract cannot reach inference. **Evidence.** Rebuild all five services in the order above; fire a schedule with a 2-minute expiry, then after expiry: fire refused, `cron_fires` failed `contract_expired`. **Effort:** M. **Deps:** S4, S1. **Risks:** `deny_unknown_fields` skew (order above); orchestrator Temporal replay (additive fields only).

### S6 [BUILD] Scheduled Do run in execution-core (L, 8-12 d)
**Goal.** The step becomes a read-only, budgeted agent run through the same driver and gate.
**Files**
- `RS/execution-core/src/runtime_loop/agent.rs` - make `run_agent_with_tools` `pub(crate)` (743); add a small `unattended` posture read from `StateStore` by `run_id` (no signature change): at the rung-refusal and plan-mode refusal sites (1664-1700) and the `pending_pause` branch (2086-2100), when the run is unattended push a `WithheldEffect {tool, reason_code, args_sha256}` into the per-run ledger and return the same model-facing refusal; **never** call `pause_for_approval` for an unattended run (treated as withheld). Keep the diff to those three hunks (hot spot: WS02/WS03/WS05/WS24/WS25 edit this file; land after WS03's LoopState refactor).
- `RS/execution-core/src/state.rs` (101, 214) - `unattended_ledger(run_id)` accessor, same pattern as `sandbox_lease`.
- NEW `RS/execution-core/src/scheduled_agent.rs` - `run_scheduled_agent(ctx, req, decision, contract)`: builds the tool Vec as `offered_tool_defs()` filtered to contract tools AND `delegable` AND WS05 class `read` (bypasses `merged_tool_defs`: no MCP, no client tools, no `plan.propose`, no fan-out, no `process_*`, no server-resolved owner actions), a `RunAgentRequest {mode:"ask", plan_mode:true, autonomy_rung:READ_ONLY, max_rounds:min(contract,8), max_subagents:0, zdr:false, model:"verevon-balance", provider_hint:"", user_id: decision.subject_id (policy lookup only), space_id}`, `RunDeadline::wall_clock_after(now, 240 s)` bounded by contract expiry, `data_plane_bearer: None`, `sandbox_bearer: None`, `session_bearer` = service scheduled-step token, inference bearer = `ScheduledInferenceTokenProvider` token; no memory prefetch/write, `max_questions` 0 (WS14), RunContext from WS02 (goal + optional `run_instructions` snapshot, D8; status stamped).
- `RS/execution-core/src/grpc.rs:238-451` - after claim + context + Control verification (S5), branch: contract present AND `EXECUTION_SCHEDULED_AGENT_ENABLED` -> `run_scheduled_agent`; else legacy single inference (kept). Record via `RecordScheduledRunResult` with `rounds_executed`, `withheld_effects`, `tool_calls`, usage. Transport loss mid-run => `unknown_outcome` (existing semantics `scheduled_step_failure_status` 522-529), never auto-retried.
- Response/proto: `ExecuteScheduledStepResponse` (1-6) += `rounds_executed = 7; withheld_effects = 8; tool_calls = 9; input_tokens = 10; output_tokens = 11` (`execution.proto:66-73`).
- Result text composition: final answer + a "Withheld for your review" block listing each withheld effect (tool, closed reason, arguments) - in the transcript only (D6). Reason codes consumed from WS13's typed cause vocabulary when landed; v1 tokens `scheduled_not_covered`, `scheduled_non_delegable`, `scheduled_contract_expired`.
- `GO/orchestrator-core/cmd/workflows/interactive_run.go:188-259` + `step_loop.go:97-136` - `workflow.GetVersion(ctx, "scheduled-agent-lane", DefaultVersion, 1)`: new branch calls ONE `ExecuteScheduledStepActivity` (step 0) with activity options `StartToCloseTimeout 6 min, HeartbeatTimeout 60 s, MaximumAttempts 1`; a withheld count > 0 completes the run (`outcome=needs_review`), it no longer fails (242-243 stays only on the replay-compat path). `cmd/activities/activities.go:78-85` heartbeat goroutine while the unary RPC blocks; client deadline > 250 s.
- `MP/deploy/docker-compose.yml` - pass-through `EXECUTION_SCHEDULED_AGENT_ENABLED` (empty default = off) next to 1135-1136; `scheduled-step-config-contract-test.sh` and `dev-runtime-preflight.sh:105-118` extended.
**Flag.** `EXECUTION_SCHEDULED_AGENT_ENABLED` default false; schedule without contract never takes the branch; rollback: off => legacy lane. **Compat:** proto fields additive; old orchestrator keeps old workflow version.
**ZDR clause.** Same four refusals; additionally `run_scheduled_agent` refuses `zdr || waived` before claim; persisted = transcript message with withheld args (non-ZDR), receipts numeric/digest. Golden: `zdr-posture-matrix.json` row `scheduled_do_run` = refused for every ZDR/waived cell; Rust `scheduled_agent_refuses_zdr_and_waived`.
**Tests.** execution-core NEW `tests/scheduled_agent_contract.rs`: (1) only contract tools offered; (2) a call to a tool outside the contract is rejected by the purpose-lock; (3) a covered-class-but-write call is withheld, never paused (`no_approval_row_minted_for_unattended_run`); (4) `mcp__*`/`browser_agent` never offered; (5) wall clock stops the loop; (6) `max_questions` 0 / no `ask_user`; (7) no memory write; plus existing `autonomy_ladder_contract.rs` unchanged. Go: orchestrator `TestScheduledRun_AgentLaneCompletesWithWithheldEffects`, `..._UnknownOutcomeStopsWithoutRetry`, replay test for the old history. Commands: `cargo test --manifest-path "MP/rust/Cargo.toml" -p execution-core --all-targets` (two cargo targets fail on Windows: use `--lib` plus the named test, per memory), `cd MP/go && go test -count=1 ./services/orchestrator-core/...`, `MP/scripts/tests/scheduled-step-proof.sh`.
**DoD.** A schedule with `web_search` approved produces a transcript with real search-grounded content; a prompt-injected "send an email" is withheld and visible; cost rows exist (D1 attribution from WS02 S3). **Evidence.** Rebuild execution-core, orchestrator-core, session-core; flag on; fire a dev schedule whose goal needs a web read AND a write; capture the transcript (answer + withheld block), `scheduled_step_receipts` row (`completed`, digest), Temporal history (no content), `cost_entries` for the run id, and absence of any `approvals` row.
**Effort:** L, 8-12 d. **Deps:** S1, S5, WS02 S4+S7 (RunContext, seed), WS03 S6 (LoopState refactor merged first), WS05 S3+S4 (class lookup, `read` filter), WS01 S0. **Risks:** loop file conflicts; model hallucinating tool names (purpose-lock covers); long runs vs 240 s (deadline reply `RUN_TIME_LIMIT_REPLY` already honest); search results used as prompt-injection vector: execution-core `provenance.rs` already classes `web_search` as `ExternalWeb` (61), and reads cannot become effects because the contract ceiling is `read`.

### S7 [BUILD] Off-surface outcome notice (M, 4-6 d)
**Goal.** The owner learns of `needs_review`/`failed`/`expired` without opening the app; no Allow action.
**Files**
- `RS/session-core` outbox (WS01 S2/migration 0043 publisher; WS16 S3 envelope family) - on terminalization of a run with `origin=schedule` emit a content-free `SCHEDULED_RUN_OUTCOME` envelope `{org_id, run_id, thread_id, schedule_id, subject_id, outcome, zdr:false}`; recipient is `metadata.subject_id` (human), because the run/thread owner is `service:orchestrator-core` (WS16's `env.UserID` would notify the service).
- NEW `GO/capability-core/internal/runwatch/scheduled_outcome.go` + branch in `Notifier.process` (`internal/runwatch/consumer.go`, same place as WS16's attention branch): fail closed unless `env.Zdr != nil && !*env.Zdr`; call `notifyclient` (`internal/notifyclient/client.go:75-150`) with type `modelplane.scheduled_run_outcome`, idempotency `scheduled:{org}:{run}:{outcome}`, payload `{title, body, message_key, cta_href:"/chat?...thread", thread_id, run_id, outcome}` generic copy, `retention_mode:"standard"`.
- `AP/notification-core/internal/http/handlers.go:199-218` - allowlist entry for capability-core (one line; single allowlist shared with WS16 S4 and Space F-t1-008); NEW `AP/notification-core/migrations/<next>_register_scheduled_run_outcome.up/.down.sql` (channel rows: in_app on, push on via WS16 S6, email off; pattern 013).
- Ownership note: master matrix rule 11 (`apps/master-ownership-matrix.md:167`) names Ingestion and the Frontend gateway as notification-core callers; capability-core already calls it for run-watch notices (`runwatch/consumer.go:3,16`). Record capability-core as an accepted caller in the matrix (docs edit in this slice) instead of routing through the gateway.
- FE `notifications-client.ts` (live M) - localise by `message_key`.
**Flags.** `CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED` default false; reuses WS16's native-types switch. **ZDR:** envelope refused unless explicitly non-ZDR; payload carries no goal, answer, tool or argument text. Golden: NC `TestScheduledOutcomeNotificationsAreContentFree` (payload key allowlist).
**Tests.** CC `scheduled_outcome_test.go` (zdr nil/true refused, recipient = subject, idempotency, run_completed path unchanged: existing consumer tests); NC `server_test.go` allowlist accept/reject pair (as :535/:590); session-core `scheduled_outcome_emitted_once_per_terminal_run`. Commands: `cd MP/go && go test ./services/capability-core/internal/runwatch/...`; `cd "AP/notification-core" && go test ./...` (command not verified in sweep).
**DoD.** A fire that withheld an effect yields exactly one notification to the schedule owner with a deep link to the transcript. **Evidence.** Rebuild session-core, capability-core, notification-core; `NOTIFICATION_DELIVERY_MODE` recorded; `notification_feed_items` row type `modelplane.scheduled_run_outcome`; `GET /api/v1/notifications` through the BFF; click opens the transcript. **Effort:** M. **Deps:** S6, WS16 S3/S4 (soft: can ship the consumer branch standalone), WS01 S2 (outbox publisher deployed). **Risks:** notification fatigue (coalesce per schedule: at most one per fire, none for `completed` unless the owner enabled it; default `completed` = in-app only).

### S8 [BUILD] Guards: expiry and failure auto-pause, renewal (M, 3-5 d)
**Goal.** Stale consent cannot keep firing; failure storms stop.
**Files**
- `GO/capability-core/internal/cron/sweeper.go` (`RunOnce` 95, `fireOne` 226) - skip and auto-pause (`enabled=false`, reason `paused_expired`) schedules whose `contract_expires_at` is past (column from S4); after 3 consecutive failed fires (`cron_fires.outcome`, S1) pause with `paused_failures`; sweep legacy `pending` fires older than 1 h to `unknown`.
- `GO/capability-core/internal/api/workplane_apis.go:1011` PATCH - un-pausing an expired schedule requires renewal (new template + new Control create decision), never a bare `enabled=true`.
- `FE ScheduledRunsPanel.tsx` - "Renew approval" action opens the consent card prefilled; shows expiry badge ("expires in 3 days").
- Emits `SCHEDULED_RUN_OUTCOME{paused_expired|paused_failures}` via S7.
**Flag:** `CRON_CONTRACT_GUARDS_ENABLED` default true once S4 is on (guard only touches schedules with a contract). **ZDR:** metadata only. **Tests:** sweeper_test `TestExpiredContractPausesSchedule`, `TestThreeFailuresPause`, `TestBareEnableOfExpiredScheduleRefused`; FE panel test. **DoD/Evidence:** schedule with 2-minute contract pauses itself and notifies; renewal re-enables. **Effort:** M. **Deps:** S4, S7. **Risks:** sweeper is default-on with no compose passthrough (`flags-and-config.md:79,102`): the new guard must not add an off-switch gap; test in `CRON_SWEEPER_ENABLED` default path.

### S9 [BUILD, gated; not scheduled] Owner-bound reads
Design only: a Control `model.schedule.data-read` decision (new action, `owner_effect`-free) that lets execution-core exchange the verified step decision for a data-plane bearer bound to the owner subject for ONE step, plus the OD8 renewable-bearer class for runs over ~240 s. Needs D2 reversed, an adversarial security review (plan OD9, D10 gate) and a ledger entry. Files not listed; do not start before S6 evidence is in.

## 5. Interfaces

**PROVIDES**

| To | Contract |
|---|---|
| WS15 run lifecycle | `runs.metadata.origin` closed enum (S2) -> trigger/origin chip (their D7). Their taxonomy gate must list `schedule`. |
| WS16 attention | Envelope `SCHEDULED_RUN_OUTCOME` + type `modelplane.scheduled_run_outcome`; recipient rule `metadata.subject_id` for system-owned runs (their S3 derive must not use run owner for these). |
| WS13 approvals v2 | Typed reasons `scheduled_not_covered`, `scheduled_non_delegable`, `scheduled_contract_expired`; a scheduled run never mints an approval row (asserted by test). |
| WS14 hitl | Scheduled runs set `max_questions=0`; `ask_user` never offered. |
| WS02 | Scheduled lane consumes `RunContext` + `compose_authored_instructions`; supplies `run_instructions` snapshot (D8) and stamps RunContext status on the run. |
| WS05 | Consumes class vocabulary and `capability_tools`; provides the delegable-set rule `read` + `approval_floor none` and the `scheduled_contract` vectors. |
| WS03 | Parking contract unchanged; S9 is the first consumer of unattended resume. |
| WS27 | Run usage returned on `ExecuteScheduledStepResponse`; cost row keyed by run id. |
| WS06 audit | New audit events: contract created/renewed/expired, effect withheld (tool, reason, args digest), fire refused; include in the chain schema. |
| F-t3-019 / F-t3-047 / F-t3-037 | Contract + lane are the base for Automations "Run now" (must go through the same Control fire authorization), triggers and goals. |

**CONSUMES**: WS01 S0 (live ZDR wave committed) and S2 (outbox publisher); WS02 S4/S7; WS03 S6; WS05 S1/S3/S4; WS16 S3/S4 plumbing; WS13 cause vocabulary (soft); Control epic "shared-Space decision family" (overlap-space D8) for shared-room schedules (out of scope here).

**Shared files and edit order**

| File | Others | Rule |
|---|---|---|
| `RS/execution-core/src/grpc.rs`, `runtime_loop/agent.rs`, `auth.rs` | WS02, WS03, WS05, WS13, WS24, WS25 | WS01 lands the live M first; S1 touches only the scheduled function; S6 lands after WS03 S6 and keeps three small hunks |
| `RS/session-core/src/grpc.rs`, `terminalization.rs` | WS01, WS03, WS15, WS16, WS17 | live set first; S1/S2/S5 hunks confined to the scheduled functions |
| `MP/proto/.../sessions.proto`, `runs.proto`, `execution.proto` | WS02, WS03, WS14, WS17 | append-only; integrator assigns numbers (stated numbers are next free at HEAD) |
| session-core migrations | all | integrator numbers; wt next 0044, live next 0046 |
| `GO/capability-core/internal/api/workplane_apis.go`, `cron/control_authorizer.go` | WS05, WS23, WS25, Watch (S4.3) | S4 first for cron; coordinate migration numbers |
| `AP/notification-core/internal/http/handlers.go` allowlist | WS16, Space F-t1-008 | one map; first lander wins, others rebase |
| `FE ScheduleDialog.tsx` [live M], `ScheduledRunsPanel.tsx` | Space (Work tab) | land the live +24 first |

## 6. Cross-session interfaces

**(a) Verevon Space session.** Shared pieces: (1) the Control decision family: schedule create/fire/run/step are personal-only today (`validatePersonalAuthority`); shared-room schedules need the Space session's "shared-Space decision family" epic (overlap-space D8); WS20 assumes it extends `scheduled_step_decision.go` and `schedule_create_decision.go` with shared validators and carries `contract_digest` unchanged; owner: Space/Control. (2) Origin vocabulary (C9): Model owns `runs.metadata.origin`; Space maps Control's `turn_origin`. (3) Notification allowlist and type naming (single map, WS16 section 6). (4) The "Work" tab/SpaceWorkPanel is the Space session's surface; S3 only links to the transcript and must not fork the room Work UI. (5) Memory containment: scheduled results are appended with a non-`conversation` source scope; the Space session's audience predicate must treat `origin=schedule` threads as non-extractable. Assumes the Space session delivers none of this before S6.

**(b) Quarry v2/browser session.** No dependency. Browser tools are on the never-delegable list in v1 (`browser_agent`, `browser.*`); "Model proposes, Quarry executes" untouched. If WS21 later allows read-only browser observation unattended it must arrive as a class row (`browser.observe` = `read`) plus a Quarry grant kind valid without a human, which is outside this stream.

## 7. Regression guards (audit section 8)

| Strength | Touched how | Guard |
|---|---|---|
| Signed fail-closed capability gate | S6 runs through the same gate; tool Vec only narrows | execution-core test: unattended run still calls `EvaluatePolicy`; unknown tool fails closed; run `autonomy_ladder_contract.rs` |
| Ask/Do switch + autonomy ladder | Scheduled posture = `READ_ONLY` rung + `plan_mode`, never escalates | test that no code path raises the rung for `origin=schedule`; ladder contract test unchanged |
| Two loops separate | S6 reuses execution-core driver; model-gateway untouched | diff gate: no change under `model-gateway/src/tool_loop.rs`; `cross_service_loop_contract.rs` |
| ZDR by construction | Four refusals kept, one added (result RPC) | `zdr-posture-matrix.json` rows (S1, S6) via `contract-goldens.yml` and `zdr-contract-sync.test.sh`; `scheduled-step-config-contract-test.sh` |
| Honest outcomes | Receipts stay metadata + digest; `unknown_outcome` never retried; withheld effects shown, not hidden | existing `TestScheduledRun_UnknownOutcomeStopsWithoutAnotherStep` (interactive_run_test.go:179), new `..._AgentLane_UnknownOutcome...` |
| Credential isolation | No user bearer in the lane; Control bearer never in Temporal | `TestScheduledRunInputRejectsDetachedTemplateAndForgedExecutionFields` (startworkflow_test.go:452); assert Temporal history contains no tokens or output |
| Multi-tenant runtime/GDPR | New `cron_fires` columns and message are in session-core erasure scope | extend `gdpr.rs` order test (724-729) and summary counts |
| Typed event taxonomy | None added to ChatEvent | n/a |

## 8. Effort and risks

**Total:** S0 1-2 d, S1 5-7, S2 2-3, S3 4-6, S4 8-10, S5 5-7, S6 8-12, S7 4-6, S8 3-5 = **40-58 engineer-days (L-XL; audit's "L" understates because D5/D6 result persistence and the contract plumbing across five services are real work)**. **Critical path:** WS01 S0 -> S0 -> S1 -> S4 (needs D1, WS05 S1) -> S5 -> S6 (needs WS02, WS03 S6, WS05 S3/S4) -> S7 -> S8. S1-S3 are off the critical path of WS02/03/05 and deliver "See results" for today's lane.

**Top risks**
1. **Hot-file conflicts** in execution-core `agent.rs`/`grpc.rs` and session-core `grpc.rs` (live M, five other workstreams). Mitigation: land WS01 S0 first, keep S6 to three hunks, new logic in new files (`scheduled_agent.rs`, `scheduled_contract.rs`).
2. **Decision skew breaking the lane**: `deny_unknown_fields` on exec-core claims and Temporal replay. Mitigation: fixed rollout order in S5, `#[serde(default)]`, `GetVersion` branch, additive proto fields, replay test.
3. **Consent that goes stale or is never understood** (audit risk). Mitigation: mandatory expiry (D5), re-verification at fire and step, auto-pause (S8), plain-language card, zero effects possible in v1.
4. **Prompt injection through web reads in an unattended run.** Mitigation: ceiling `read`, no outbound-capable tools, withheld-effect block visible to the owner, `ExternalWeb` provenance already applied; adversarial review before enabling the flag in production (plan D10 gate language reused).
5. **Credential window and long runs.** Mitigation: D2 decouple, 240 s wall clock, no user-delegated bearers, heartbeat + 6 min activity timeout; anything needing more is S9.
6. **Ownership ambiguity for off-surface delivery** (capability-core as notification-core caller vs rule 11). Mitigation: matrix amendment in S7; content-free payloads.
