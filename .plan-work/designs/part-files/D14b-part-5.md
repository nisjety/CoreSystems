
---

## 5. Dependencies and ordering, merge-conflict hotspots

### 5.1 Ordering

| Slice | Hard dependency | Soft dependency | Notes |
|---|---|---|---|
| S0 | none | none | Ships first; pure hiding |
| L1 | none (the signed-handoff runtime check is a precondition, not a dependency) | D6 S0 (effective `max_pages` in `params`, gives an honest `total`) | Migration 016 |
| L2 | L1 | none | Needs the orchestrator and control deployed together |
| L3 | L1 | L2 (terminal states in the stream) | |
| L5 | L1, L2 | L3 (live stream in the SPA) | Contract test forces registry and handler ids together |
| L4 | L1, decision D9 | F-t2-052 supersedes | Optional |
| S1 | none | D4 S-8 (shared refusal call site) | Closes the cross-tenant schedule exposure; ship early even if the rest slips |
| S2 | L1 (job columns, `workflow_id`), S1 | D6 S3 (entitlement leg) | Flag stays 0 |
| S3 | S2, ADR D14 | none | Deletes `QCT:internal/temporal/client.go` |
| S4 | S2 | D6 S3 (`LimitsResolver`), D4 S-8; Control Plane and Application Plane approvals (Section 9) | `observe` mode ships without approvals |
| S5 | S4 observed a week, **F-t2-016 scoped crawl deployed**, D6 S0 deployed | F-t2-008 budgets | The "scope before schedules go live" gate |

Sibling workstreams (names as in this planning wave): D2 quarry-run-control, D4 zdr-tenant-boundary, D6 budgets-capacity-metering, D14a crawl scope and robots (F-t2-016), F-t2-040 parallel batch, F-t2-031 webhooks, F-t2-014 run trace, F-t2-029 metrics, F-t2-042 agent web toolset, F-t2-052/053 result sets and saved jobs.

Suggested merge order on `main`: D6 S0 and D14a (they change the crawl input and the crawl loops that S2's shared builder and L2's `GetVersion` ids sit beside), then S0, L1, S1 (parallel), L2, S2, L3, L5, S3, S4, S5; L4 whenever D9 is decided.

### 5.2 Hotspots (files and ranges shared with other workstreams)

| File | Ranges | Also touched by | Handling |
|---|---|---|---|
| `QO:internal/workflows/workflows.go` [C] | `runPage` 174-232; `ChangeMonitorWF` 305-355; `BatchJobWF` 359-383; `CrawlJobWF` 396-512; durable loop 518-610 | D6 S0 (`GetVersion("crawl-bounded-v1")` at 456, 474, 555, 574), D14a (scope in both loops), F-t2-040 (batch loop 370-377), F-t2-031 (`run_completed` emits 289, 350, 379, 507, 607) | Unique `GetVersion` ids: `crawl-bounded-v1` (D6), `lifecycle-v1`, `batch-control-v1`, `monitor-preflight-v1` (here). `controlGate` is the single batch hook F-t2-040 calls between waves |
| `QO:internal/workflows/signals.go` [C] | 87-146 | F-t2-040 (batch handlers) | This design owns it |
| `QO:internal/jobs/dispatcher.go` [C] | `dispatch` 244-309, `markRunning` 313-342, input builders 417-469 | D6 S0 (`crawlInputFromJob` limits), D14a (scope fields), F-t2-040 (`batchInputFromJob`) | S2 moves the builders to `workflows/inputs.go` once; later fields land there |
| `QO:internal/schedules/schedules.go` [C] | 49-57, 136-204, 302-404 | F-t2-053 later | Single owner now |
| `QO:cmd/orchestrator/main.go` [C] | 94-152 | D6 (limits config into `jobs.Config`) | Adjacent hunks, no overlap |
| `QCT:internal/store/store.go` [C] | `Job` 174-300, `JobsStore` 119-148, `Schedule` 333-356 | F-t2-031 (`Webhook` 376-383), F-t2-040 (`FindByIdempotencyKey` 145-150), D4 (tombstone store) | Job wire structs must be edited in both places in one commit |
| `QCT:internal/store/pg/resources.go` [C] | jobs 54-460 (eight SELECT lists), schedules 742-820, events 990-1090 | D6 flags the same lists, F-t2-031, F-t2-040 | L1 does the `jobCols` refactor first and alone |
| `QCT:internal/store/pg/migrations/` | 015 (D4), 016, 017, 018 | F-t2-031 (org_id on webhooks), F-t2-040 | One allocation table kept by the integrating architect; renumber before first apply only |
| `QCT:internal/resources/resources.go` [C] | `MountJobs` 24-33, `MountEvents` 185-278, `markJobTerminalIfNeeded` 302-329, `createJob` 396-458, `updateJob` 513-549, `fanoutWebhooks` 612-650 | F-t2-031 (fanout), F-t2-040 (createJob idempotency) | Hunks kept small and separate |
| `QCT:cmd/control/main.go` [C] | notify block 53-62, mounts 156-197 | D4 S-10 (186-215), F-t2-031 (166-173) | Line 192 changes in S3 |
| `QE:routes.rs` [M] | route table 125-293; `refuse_durable_job_under_zdr` 563-588; `crawl_handoff` 590-667; `batch_handoff` 681-725; `HandoffAck` 728-738; `internal` router 100-123 | D4 (refusal body), D5 (`CrawlRequest` 533-553), D6 S0 (`HandoffAck.limits`), D14a, F-t2-040 (`BatchRequest`), D2 | New routes are added as small hunks after lines 134, 192, 232 and in the `internal` router; one-word visibility change at 563 |
| `QE:schedule_routes.rs` [C] | `create_schedule` 239-340, trigger and backfill 380-426 | D4 S-8 (ZDR), F-t2-053 | S1 and D4 share the refusal call; order agreed at merge |
| `QE:handoff.rs` [C] | 26-100 | F-t2-040 (`Idempotency-Key`), D6 | New function beside the old one |
| `QE:resource_routes.rs` [C] | `list_job_events` 1057-1130 | none | Single owner |
| `QE:state.rs` [M], `QE:main.rs` [M] | whole | QB-T17 Model Plane switches (uncommitted) | Not edited by this design |
| `GW:domains/knowledge/quarry.rs` [C] | `start_crawl` 306-376, events 1098-1158 | D6 S0 (`limits`), D14a (`normalize_crawl_body` 386-416) | Different functions |
| `GW:domains/ingestions/{schedules,runs}.rs` [C] | whole | F-t2-053 | Single owner |
| `FE:shared/actions/action-registry.ts` [C] and `GW:domains/actions/{handlers,dispatchers}.rs` [C] | `crawlSiteInput` 568-571, `ingestionsCreateScheduleInput` 887-914, `ingestions.*` 2563-2594; handler arms near 110 and 255-267 | D14a (crawl input), F-t2-042 | `action-surface-contract.test.mjs` forces both files in one PR |
| `FE:features/dashboard/home/KnowledgeComposer.tsx` [M] | 288-420, 482-531 | in-flight 10-line diff | Re-read before editing |
| `Q:docs/openapi.yaml` [M] | 1232-1700, 4361-4385 | D2, D5, D6, D14a, F-t2-031, F-t2-040, F-t2-064 | In-flight +78 lines are at 238-250 and 4012-4150: no overlap |

---

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

**Ownership.**
- Ingestion Plane owns job and schedule state end to end: control stays the CRUD and record tier, the orchestrator stays the only Temporal client, the edge stays the public surface. No Temporal client is added to control; `QCT:internal/temporal/client.go` is deleted in S3. Rule holds.
- Control Plane: org posture is read through org-core's HTTP route and entitlements through the billing-core read that D6 S3 adds, both by API with a service principal; no database is read. **At risk:** the posture read needs `org:retention:read:any` for a second holder, which contradicts the invariant written in `CP:docker-compose.yml` 531-535 and pinned by `TestOrgRetentionReadScopeIsMachineOnly`. It is a Control Plane decision (Q2); observe mode ships without it.
- Application Plane: notification-core is reached by API only and its intake allow-list is edited by its owner (Q3). Quarry never writes its tables. **At risk:** until Q3 is answered the Quarry side runs inert; the existing change notification is already undeliverable (Section 2.2).
- Data Plane: untouched. Result pagination option C would read documents through Data Plane contracts only and is not part of phase 1.
- Model Plane: untouched. No grant, broker or budget change; Model service tokens stay default-deny on every lifecycle route (`QE:auth.rs` 504-545).
- Frontend Plane: the gateway proxies with the session's Quarry token, takes org and user from the session, never from the path or body, and forwards no upstream secret; registry and handler ids stay in lockstep.
- No direct database crossing: every cross-plane read and write above is an HTTP call. Channel Plane is not touched.

**ZDR, per boundary.**
- Job records and events (control): crawl and batch are refused for ZDR callers (`routes.rs` 563-588), so ZDR orgs create none. For jobs that predate an org's switch, status counts and the control verbs stay available (wind-down); `events`, `errors` URLs and `results` are refused for ZDR-floor callers (D10).
- Schedule rows: contain target URLs and config, so creation is refused for ZDR callers (S1, D15a) and every fire re-checks posture (S2 and S4, D15b). A schedule of an org that later turns ZDR on is paused with `skipped_zdr`; its row and Temporal args remain until deleted or erased.
- Temporal: schedule args and workflow histories carry the config URLs. **Open leak, flagged:** the schedule reaper refuses to delete a Temporal schedule whose org has no remaining desired rows (`schedules.go` 188-194, fail closed), so after an org-erasure purge (`gdpr_purge.go` deletes the control rows) its Temporal schedules and their URLs persist. Pre-existing for change monitors; this design adds more such schedules. Follow-up (not scheduled here): a purged-orgs tombstone list the reaper may use, owned with D4's tombstone work.
- `schedule_requests` rows hold ids, times and a result, no content; covered by org erasure (S3).
- Notifications: content-free payload (schedule id, kind, reason code, run id), `retention_mode: "standard"` because ZDR orgs cannot own schedules.
- Logs and telemetry: new log lines carry ids, status and counts only, never URLs or excerpts (D4's field-discipline lint applies to the new modules).
- Caches: none added; org posture is never cached.

**Model proposes, Quarry executes.** The Model proposes `ingestions.get_run_status`, `pause_run`, `resume_run`, `cancel_run` and `create_schedule` as registry actions; the gateway executes them under the user's session; Quarry authorises the org and applies the state machine, and rejects with 409 when the transition is illegal. `cancel_run` requires approval and is marked irreversible. A Model-proposed schedule is created disabled and waits for a human to enable it (D13). No Model Plane principal can call the lifecycle routes directly. Rule holds; the one place it could erode is a future gated tool (F-t2-042) that gives the Model a service scope for these routes: that needs a new `crawl:manage` scope, a route-scope entry and per-run attribution, and is left to that workstream.

---

## 7. Cross-session interfaces

### (a) Verevon Space (shared workspaces; agent PC and browser in Spaces; ZDR-safe telemetry)

| Shared piece | Proposed owner | What the Space session must reconcile |
|---|---|---|
| Run origin vocabulary (`origin.type`, `origin.actor`, `origin.ref`) | Quarry (this design) | A run an agent starts inside a Space: `type` is `agent` for a service token and `user` for a user session. `origin.ref` can carry a Space id, but from a user token it is advisory only (the edge cannot tell the gateway's label from a user's), marked unverified; if Space needs a verified Space id it must come from a gateway-signed claim, which does not exist today |
| Who may pause or cancel a run started in a Space | Space and Quarry together (Q4) | Today org-wide. Options: creator only, creator plus Space members, org admin. The decision lives in one function, `authorize_control` |
| Run status and live tail for presence and activity feeds | Quarry (`GET /v1/jobs/:id`, `?follow=true`) | Space should consume status and the tail, not poll the job list; status is metadata only and therefore ZDR-safe, content reads are refused for ZDR-floor callers |
| ZDR-safe telemetry | D4 and D10 | New Quarry log and event fields here are ids, statuses and counts; Space must not project `page_extracted` excerpts into shared surfaces |

### (b) Model Plane and chat/agents (BrowserBroker grants, agent browser path, delegated bearer, approvals)

| Shared piece | Proposed owner | What must be reconciled |
|---|---|---|
| Registry action ids and approval flags (`ingestions.*`) | Frontend registry file is the contract; Model Plane owns tool descriptors | Whether model-initiated pause and resume also need approval (this design: only cancel does); F-t2-033 owns the approval presentation |
| Single poll call for agents (`GET /v1/jobs/:id` with `phase`, `terminal`, `counts`, `deadline_at`, `warnings`) and cancel to honour budgets | Quarry | Model Plane budget logic (F-t2-008) keys on `origin.type = agent`; the `x-verevon-run-id` header is honoured only on service tokens |
| Event vocabulary (`run_paused`, `run_resumed`, `run_cancelled`, `run_timed_out`, `schedule_skipped`, `control_requested`) | Quarry | Run-trace envelope work (D10, F-t2-014) must map these; the `run_*` names already exist in Rust `EventType` except the three added here |
| Delegated bearer forwarding | Model Plane (D1) | Not needed for this work (the gateway executes under the user's session). Scheduled runs cannot borrow a human's bearer (audit ADR 9): `ScheduledJobWF` is acquisition-only and runs as the schedule creator's identity for ingest ownership, no browser grant |
| BrowserBroker grants, agent browser path | Model Plane | Untouched; bulk acquisition stays out of the agent's tab loop |
| Service-token access to lifecycle routes | Model Plane plus Control Plane | If F-t2-042 wants the Model to call these routes directly it needs a new scope in `CP:config/plane-service-principals.json` [C] and an entry in `require_service_route_scope` |

---

## 8. What can proceed without an ADR or decision, and what is blocked

**Proceed now (no decision):** S0, L1, L2, L3, L5 (without results), S1 (with the ZDR refusal, the recommended and audit-assumed behaviour), S2 (flag off), the `final`/`code` payload fix, the pause idempotency and `Await` fixes, the schedule org-scoping fixes (these close a cross-tenant exposure and should not wait for anything else).

**Blocked:**
- L4 results: the data-source decision D9 (new topic, not in audit 9.3).
- S3 trigger and backfill: ADR D14, the command transport (adjacent to audit topic 9); the table and routes are common to options 2 and 3, so only the transport changes if the owner overrules.
- S4 posture `enforce` and the cross-plane half: Control Plane approval of the narrow scope grant (audit topic 4, D4 question 3); `observe` mode and the preflight shape are not blocked.
- S4 failure notices: Application Plane approval (audit topic 9 adjacent); the Quarry signer ships inert.
- S5 go-live: F-t2-016 (scope) deployed, D6 S0 deployed, one week of `observe`.
- Not blocked but to confirm: Q4 (cancel authority), Q5 (timeouts).

---

## 9. Open questions for the user (real blockers marked)

1. **(Blocks L4)** Crawl output is not stored per job. Is it acceptable that phase 1 of `GET /v1/crawl/:id/results` is a page index (url, title, status, excerpt, counts, artifact id when available), replaced later by F-t2-052 result sets? Alternative: wait for F-t2-052 and ship no results route.
2. **(Blocks S4 enforce; audit topic 4)** May Quarry refuse schedules and change monitors for ZDR orgs (D4 matrix rows 6 and 7, recommended and assumed by the audit), and may a `quarry-edge` principal hold org-core `org:retention:read:any` so a fire can re-check posture (this changes a documented one-holder invariant and two contract tests)? Fallback if no: a Control Plane event or relay endpoint that tells Quarry when an org turns ZDR on.
3. **(Blocks failure notices; audit topic 9 adjacent)** May the Application Plane register `quarry-control` as a notification-core caller with the types `quarry.schedule_run_failed` and `quarry.schedule_run_skipped`? Today notification-core rejects Quarry entirely, which also leaves the existing change notification undelivered.
4. **(Blocks S3 only; new ADR)** Confirm the pull-model request-row transport for trigger and backfill (recommended) over a Temporal SDK client in control or an orchestrator HTTP endpoint.
5. Default confirmation, not a blocker: may any member of an org pause or cancel a colleague's run (recommended default), or only the creator or an org admin?
6. Default confirmation, not a blocker: raise the fixed 30 minute execution timeout to per-kind values (scrape 5 min, batch up to 2 h, crawl up to 6 h) so a 5000-page crawl can finish, with `timed_out` as the typed outcome when it does not?

---

## 10. INTERFACE SUMMARY (for the integrating architect)

**Files touched.** Ingestion, control: `store.go`, `store/pg/resources.go`, `resources.go`, `cycle23.go`, `schedule_wire.go`, `schedule_validate.go`, `job_wire.go`, `notify/sink.go`, `cron.go`, `gdpr_purge.go`, `cmd/control/main.go`, new `jobs_lifecycle.go`, `schedule_runs.go`, `schedule_requests.go`; delete `temporal/client.go`. Orchestrator: `dispatcher.go`, `workflows.go`, `signals.go`, `schedules.go`, `main.go`, new `jobs/control.go`, `workflows/scheduled.go`, `workflows/inputs.go`, `activities/scheduled.go`, `timeouts/`, `schedules/requests.go`. Edge: `routes.rs`, `handoff.rs`, `resource_routes.rs`, `schedule_routes.rs`, `quarry-core resources.rs`/`event.rs`, new `control_client.rs`, `job_lifecycle_routes.rs`, `job_stream.rs`, `schedule_preflight.rs`. Frontend: gateway `knowledge*.rs`, `ingestions/{schedules,runs}.rs`, `actions/*`, `config.rs`; SPA registry, two API clients, `KnowledgeComposer.tsx`, `VerevonIngestionsPage.tsx`. Control Plane: org-core credential block. Application Plane: notification-core allow-list and migration.

**New contracts.** Routes: `GET /v1/jobs/:id`, `GET|DELETE /v1/crawl|batch/:id`, `POST .../pause|resume`, `.../errors`, `.../events?follow=true`, `.../results` (flagged), `/ongoing`, `GET /v1/schedules/:id/requests/:rid`; internal control and edge routes listed per slice. Migrations 016 (job lifecycle columns), 017 (schedule definition), 018 (`schedule_requests`). Flags: `QUARRY_SCHEDULED_JOBS_ENABLED`, `QUARRY_EDGE__JOB_CONTROL`, `..._JOB_STREAM_FOLLOW`, `..._JOB_RESULTS`, `..._SCHEDULE_POSTURE`, `QUARRY_SCHEDULE_CAPABILITIES`. Events: `control_requested`, `run_timed_out`, `schedule_skipped`. Workflow versions: `lifecycle-v1`, `batch-control-v1`, `monitor-preflight-v1`.

**Dependencies.** Hard: L1 before L2, L3, L5, S2; S1 before S2; S2 before S3, S4; F-t2-016 and D6 S0 before go-live. Soft: D6 S3, D4 S-8.

**Effort (engineer-days).** S0 1, L1 4.5, L2 7, L3 4, L4 3.5, L5 4, S1 4.5, S2 6.5, S3 4, S4 5, S5 1.5; total about 46, two streams.

**ADR gates.** D9 results source (new topic); D14 command transport (topic 9 adjacent); D15 ZDR for schedules (topic 4); D17 notification principal (topic 9 adjacent).

**Parallelisable with.** F-t2-005, F-t2-009, F-t2-015, browser items. **Conflicts with.** D6 S0 and D14a (`workflows.go`, `dispatcher.go`, `routes.rs`), D4 S-8 (`schedule_routes.rs`, `routes.rs` 563), F-t2-040 (`BatchJobWF`), F-t2-031 (`store.go`, `resources.go`, migrations), D5/D2 (`routes.rs`, `openapi.yaml`).

**Findings to act on regardless:** cross-tenant schedule list and control (S1), unsigned handoff under required HMAC (L1 precondition), dead change notification (S4).

<!-- COMPLETE -->
