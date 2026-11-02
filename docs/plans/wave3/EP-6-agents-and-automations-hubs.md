# EP-6 - Agents and Automations hubs (Wave 3 workstream design)

Abbreviations: `MP` = `apps/Model Plane`; `CC` = `MP/go/services/capability-core`; `MG` = `MP/rust/services/model-gateway/src`; `EC` = `MP/rust/services/execution-core/src`; `SC` = `MP/rust/services/session-core`; `UC` = `apps/Control Plane/user-core`; `CVX` = `apps/Application Plane/convex-core/convex`; `FE` = `apps/Frontend Plane/verevonv3`; `BFF` = `FE/apps/gateway/src`; `FES` = `FE/src`. Every `path:line` below was opened in worktree `epic-shaw-4d613b` at HEAD `c18196af2` (branch `model-plane`, Waves 1 and 2 built). `[INF]` = inference, not read in code. `[verify live]` = only a running stack can prove it. Queue status = `.plan-work/execution/queue.json`; register rows = `docs/plans/model-plane-depth-register.md`.

Design scope rule from the computed task: phase 1 is designed in full, phase 2 at slice level, phase 3 as an outline that depends on EP-8.

## 1. Scope, items resolved, owner

**Item:** F-t3-019 "Real Agents and Automations hubs" (Priority, Partial, audit effort XL, W3). It is the only item in EP-6. Plan band: XL overall, phase 1 is M (plan 3.3.1 row EP-6).

**Owner.**
- Model Plane: schedules, fires and the new Run now route (capability-core), the fire pipeline it reuses (capability-core taskexec, orchestrator-core, execution-core, session-core), and in phase 2 the invoke-time tighten-only use of an agent definition (model-gateway, execution-core).
- Application Plane: agent definitions in Convex (`agents`, `spaceAgentBindings`, ADR-0002), and in phase 2 their version history. It stays the single definition store (plan 1.3, overlap-space row 14).
- Frontend Plane: the two hubs, the BFF routes and the action contracts.
- Control Plane: no change in phase 1. Run now reuses Control's existing schedule-fire and scheduled-run decisions unchanged (section 2.1). Control keeps identity, membership, entitlements and the signing of every fire.

| Slice | Phase | Tag | In | Startable now |
|---|---|---|---|---|
| S0 | 1 | [FINISH] | Decision records and ledger entries (docs only) | yes |
| S1 | 1 | [BUILD] | Automations hub over the existing list, PATCH, DELETE and fires routes, plus the missing action contracts. FE only | yes |
| S2 | 1 | [BUILD] | Run now: capability-core route through the same Control fire authorisation, MG proxy, BFF route, action contract (dark) | yes (build); live proof waits on WS20-S1 |
| S3 | 1 | [BUILD] | Run now button and manual-fire chip in the hub | after S1, S2 |
| S4 | 1 | [FINISH] | One schedule list: chat panel and Settings section reuse the hub list | after S1 |
| S5 | 1 | [FINISH] | Agents hub landing over real resources; blueprints labelled as templates | yes |
| S6 | 1 | [FINISH] | Phase 1 flip, live proof, docs | after S1-S5 and WS20-S1/S3 live |
| S7 | 2 | [BUILD] | Versioned agent definitions in Convex | after OD-4 |
| S8 | 2 | [BUILD] | Builder: draft, preview, publish, history | after S7 |
| S9 | 2 | [BUILD] | Invoke-time version pin, tighten-only tool allowlist and autonomy cap | after S7, OD-5 |
| S10 | 2 | [BUILD] | Role parameter on `subagent.task` from published definitions | after S9 |
| S11 | 2 | [FINISH] | Preset agents get real action sets from the admission table | after S9, WS24-S6 |
| P3 | 3 | outline | Persistent named agents with scoped memory | after EP-8 slice 1, OD-6 |

**Explicitly OUT.**
- Other epics: goals and event triggers, including the Watch primitive and webhook sources (EP-7, F-t3-037/047); scoped memory itself (EP-8, F-t3-060/061); plugin and authored-behaviour lifecycle (EP-11, F-t3-058/059); sharing, export and the public Agent API (EP-12); coordinator-mode orchestration (W5 F-t3-071, "after EP-6 phase 2", plan 3.3.4).
- WS20 work this epic consumes but does not do: the consent contract and its renewal (WS20-S4/S8), contract carry at fire and step (WS20-S5), the scheduled Do lane (WS20-S6), the outcome notice (WS20-S7), owner-bound reads (WS20-S9). Open WS20 register rows stay WS20's.
- One-shot "run once at" schedules and therefore a real "Completed" state (OD-3); Pin; per-conversation schedule caps (Perplexity's 15).
- A Fleet console. It stays hidden (OD-2). Quarry owns any fleet model (overlap-quarry row 12).
- Shared-room (Space Room) schedules and the room Work tab: Space session (overlap-space rows 7 and 15).
- A workflow product: no visual workflow builder, no multi-step chains, no branching. `WorkflowBuilder.tsx` stays a labelled preview.
- Channel Plane delivery of named agents (docs-only plane).

## 2. Verified current state

### 2.1 Automations (schedules, fires, the fire path)

- Schedule routes in capability-core: `CronHandler` (`CC/internal/api/workplane_apis.go:1002-1037`) mounts `/api/v1/cron` (list, create) and `/api/v1/cron/{id}` (get, PATCH, DELETE) plus `GET /api/v1/cron/{id}/fires` (`:1108-1131`). There is no run-now route.
- List is org-wide: any caller in the verified org gets every schedule with its `task_template`; `space_ref` is a filter only (`:1154-1191`). Get, PATCH and DELETE are scoped by `org_id` only (`:1333-1340`, `:1404-1423`). Fires listing checks only `id` and `org_id` (`:1286-1330`). Register row D2-121 (open, decision) records this.
- PATCH accepts `enabled` and `schedule_expr` only, and with `CRON_CONTRACT_GUARDS_ENABLED` refuses a bare re-enable of a lapsed approval (409, `:1366-1386`). The goal cannot be edited in place: the template digest is bound by Control at create (`:1213-1256`).
- Creator scoping lives in the BFF, not in capability-core: `can_author_own_schedule` = org owner/admin or `creator_subject_id == user.user_id` (`BFF/domains/agent_actions.rs:82-85`), applied by `authorize_own_schedule` to update, delete and fires (`:453-496`, `:596-620`). `list_cron` is "open to members" (`:421-447`).
- Fire path. The sweeper claims due schedules with `FOR UPDATE SKIP LOCKED` (`CC/internal/cron/sweeper.go:150-221`), asks Control for a fresh per-slot decision through `FireAuthorizer.AuthorizeFire` (`:263-279`; a refusal disables the schedule and writes a failed fire), then `fireOne` inserts a `tasks` row whose `config_json` carries the non-secret `schedule_fire_intent` (`:347-379`) and a `pending` `cron_fires` row whose `fire_key` is the slot time (`:381-418`).
- The task dispatcher asks Control again (`AuthorizeFire`, then `AuthorizeScheduledRun`) before it calls `StartWorkflow` (`CC/internal/taskexec/workflow_dispatcher.go:327-368`, call at `:239`). So a fire never reaches Temporal without two fresh Control decisions. This is the path Run now must reuse.
- `AuthorizeScheduledRun` binds the system thread key `schedule/<schedule_id>/<fire_key>` and the run id (`CC/internal/cron/control_authorizer.go:55-115`, check at `:101`). Both decisions refuse ZDR (`:105`, `validateFireDecision` `:353-366`, check at `:359`).
- Control does not check that a fire key is a cron slot. `ScheduleFireIntent.Validate` requires only non-empty fields and a sha256 template digest (`UC/internal/spaces/schedule_fire_decision.go:32-46`); `IssueScheduleFireDecision` re-checks role (editor/manager/owner), the schedule-fire entitlement, ZDR and that subject, org and Space match current authority (`:83-123`). The scheduled-run decision refuses a `/` in the fire key (`UC/internal/spaces/scheduled_run_decision.go:53`). Execution-core accepts `[A-Za-z0-9-_:.]`, max 256 bytes (`EC/grpc.rs:1328-1334`, applied to `fire_key` at `:1347`). So a manual fire key such as `manual:<uuid>` needs no Control or proto change. [verify live] that nothing downstream parses the fire key as a time.
- Fires storage: `cron_schedules` and `cron_fires` live in the session-core database (`SC/migrations/0004_tasks_cron_hooks_skills_memory.sql:120-159`); WS20-S1 added `fire_key`, `run_id`, `thread_id`, `withheld_effects`, `outcome` and a non-unique index on `(schedule_id, fire_key)` (`SC/migrations/0056_cron_fire_run_link.sql:7-16`). capability-core reads and writes them through its own `DATABASE_URL` pool (`CC/cmd/main.go:83-88`). [INF] Both services point at the same database in compose; the cron tables have always been shared this way, so EP-6 adds no new crossing.
- Wiring: the handler is built and registered at `CC/cmd/main.go:337-347`; the fire authorizer is built after it, at `:352`, and handed only to the sweeper (`:358`). Run now needs the authorizer on the handler, so S2 moves that line up.
- ZDR at capability-core: every non-read request from a ZDR principal (or one without a retention policy) is refused by the router (`CC/internal/authz/authz.go:81`). A POST run-now route inherits this.
- Two schedule UIs: Settings `CronSchedulesSection` (admin-only page, org-wide list, create with workflow fields, toggle, delete; `FES/features/settings/components/CronSchedulesSection.tsx:127-340`) and the chat `ScheduledRunsPanel` (own schedules only via `listMySchedules`, toggle, delete, "See results" expander behind `VITE_SCHEDULE_RESULTS`, "Renew approval"; `FES/features/chat/components/ScheduledRunsPanel.tsx:20-33, 182-335`; filter at `FES/shared/api/schedule-from-chat.ts:110-114`). The panel already labels `paused_expired` and `paused_failures` (`ScheduledRunsPanel.tsx:84-111`).
- Client: `FES/shared/api/cron-client.ts:56-133` has list, create, update (POST to the BFF, which proxies to MG PATCH), delete and `listScheduleFires`. No run-now call.
- MG proxies `/v1/cron`, `/v1/cron/:id` and `/v1/cron/:id/fires` to capability-core (`MG/http_routes.rs:544-551`, handlers `:6783-6875`).
- Action contracts: no `schedule.*` or cron contract is registered in `FES/shared/actions/action-registry.ts` (the only cron mention is a comment at `:831-834`). Register row D0-017 (open) says `schedule.fires.list` is still unregistered. The CLAUDE.md rule (contract before wiring) is unmet for every existing schedule operation.

### 2.2 Agents (definitions, pages, invocation)

- Definitions: Convex `agents` (`CVX/schema.ts:327-418`): name, description, model, systemPrompt, `tools: string[]` (free strings, not capability ids), knowledge bindings, status `active|inactive|draft`. No version field. `agents:update` overwrites in place (`CVX/agents.ts:78-150`).
- Bindings: `spaceAgentBindings` (`CVX/schema.ts:595-680`): one row per Space, with `allowedTools`, `approvalMode`, `triggerModes`, status; Control's service membership is the authority, the row is projection and policy only (comment `:603-611`).
- Invocation today: the BFF, not Model Plane, resolves a mentioned Space agent and injects `agent_name` and `agent_system_prompt` into the outbound turn (`BFF/domains/spaces.rs:4496-4568`), after applying the binding policy as a filter (`apply_mention_binding_policy`, `:4570-4600`, "a filter, never a grant"). The persona text comes from `spaceAgents:agentPersonaForGateway` (`CVX/spaceAgents.ts:288-308`, via `BFF/domains/spaces.rs:4299-4308`). MG only reads the two fields (`MG/sse.rs:7932-7945`).
- `subagent.task` takes `goal` and `max_rounds` only; no role (`EC/runtime_loop/agent.rs:6969-6973`). The proto already has `SubagentRole` (UNSPECIFIED, CODER, REVIEWER, RESEARCHER, EXPLORER, GENERIC; `MP/proto/model_plane/v1/orchestration.proto:274-287`), used today only by the research fan-out (`agent.rs:6975-6985`).
- Pages: `/agents` renders `AgentsPage` (`FES/app/App.tsx:238`): role blueprints with a `DesignPreviewBadge` "Blueprint / Not yet configured for this org" and disabled activate buttons (`FES/features/agents/components/AgentsPage.tsx:270-300`). Real data already exists at `/agents/installations` (`AgentInstallationsPage`, over `GET /api/v1/agents/installations`, `BFF/domains/spaces.rs:786-788`), `/agents/runs` (`AgentRunConsole`) and `/agents/customize` (`CustomizeHubPage`, WS25-S11).
- Presets: `FES/shared/actions/preset-agents.ts:1-40`: four presets, `actionIds` "empty until owner adapters are live".
- Fleet: `/agents/fleets/:fleetId` is mounted only when `VITE_FLEET_CONSOLE === '1'`; the comment says nothing starts a fleet (`FES/app/App.tsx:20-23, 242-244`). The console fetches `/v1/fleets/:id` directly (`FES/features/agents/components/FleetRunConsole.tsx:28-36`). No BFF route serves it (grep of `BFF` finds no fleet route).
- Org agent policy (WS25-S7, done): capability-core holds the per-org policy document with `mode_cap`, `rung_cap`, `lockdown`, `ext.mcp_servers`, `mcp_tool_approval` enforced and `ext.skills`, `ext.plugins`, `user_hooks`, `reference_uploads` reserved (`MP/api/contracts/org-agent-policy-schema.json`). It holds no agent definitions. Action admissions table: `CC/migrations/0022_agent_action_admissions.up.sql:19-32` (WS24-S6).

### 2.3 Prerequisite slices (queue status and register rows)

| Prerequisite | Queue | Open rows that matter | Can EP-6 start anyway? |
|---|---|---|---|
| WS20-S1 persist result, truthful fire status, fires listing | partial (committed d642fb252; `SESSION_CORE_SCHEDULED_RESULT_PERSIST` off) | D2-118/119 (sweeper inserts `fire_key` unconditionally: migration 0056 must be applied first), D2-121 (fires org-scoped, decision), D2-122/123 (DB and docker proofs not run), D2-124 (ZDR matrix row `scheduled_run` not added) | S1, S2 build: yes. S6 live proof: no, a Run now result is visible only once S1 is live |
| WS20-S2 run-level origin | done | D2-217..224 (birth stamp unflagged, ADR-0012 not written) | yes; Run now stamps `schedule` like any fire |
| WS20-S3 "See results" | partial (`VITE_SCHEDULE_RESULTS` off) | D2-325/326 (owner read of a `service:orchestrator-core` thread unproven), D2-327 / D0-017 (`schedule.fires.list` contract unregistered) | yes; S1 registers the contract and reuses the expander behind the same flag |
| WS20-S4 consent contract | partial (`SCHEDULE_CONTRACT_ENABLED` off) | D2-037/040 (owner must accept DEC-16 (b)), D2-273/275 (delegable tool list endpoint missing, consent card empty) | yes; Run now behaves the same for contract and contract-free schedules |
| WS20-S5 contract carry and re-verify | partial | D2-290 (Control does not decode contract fields yet), D2-296 | yes; Run now goes through the same dispatcher, so the same checks apply when they land |
| WS20-S6 scheduled Do lane | partial (off) | D0-014 (ZDR clause not wired), D2-316/317 | yes |
| WS20-S7 outcome notice | partial | D2-841/846 (no emitter) | yes; manual fires get the same notice once it exists |
| WS20-S8 guards | done (default off) | D2-877/882 (rollout default), D2-881 (renewal = new schedule) | yes; hub reuses Renew |
| WS24-S5 model-access classification | done | none blocking | n/a phase 1 |
| WS24-S6 admission table | partial (`MODEL_ACTION_ADMISSION` default `tickets_only`) | see W2-17 results | S11 waits |
| WS25-S6 agent modes, strictest-wins resolve | partial (`AGENT_MODES` off) | see W2-41 results | S9 waits |
| WS25-S7 org agent policy | done | none | DEC-14 join (OD-5) |
| WS05-S1/S3 class vocabulary and registry binding | done | none | S7 tool allowlist binds to capability ids |
| EP-8 slice 1 (F-t3-060 scoped memory key) | not started (W3) | n/a | phase 3 waits |

### 2.4 Audit staleness

1. "capability-core serves only list, create, get, patch and delete ... no run history": stale. WS20-S1/S3 added `GET /api/v1/cron/{id}/fires` and its MG and BFF proxies (`workplane_apis.go:1110-1118`, `MG/http_routes.rs:551`, `BFF/domains/agent_actions.rs:56`). Run now is still missing.
2. "FleetRunConsole fetches a route no gateway serves": still true of the code, but the route is now off by default behind `VITE_FLEET_CONSOLE` (`App.tsx:20-23`), which is overlap-quarry's "hide" default. Only the record is missing.
3. "Consumed by Model Plane at invoke time as `inject_mentioned_space_agent_persona` already does": the injection is in the Frontend BFF (`BFF/domains/spaces.rs:4496`). Model Plane receives finished text. Phase 2 keeps this split.
4. Line drift only (no content change): `workplane_apis.go:836-871` is now `:1002-1131`; `schema.ts:324-384` is `:327-418`; `AgentsPage.tsx:285` is `:279`; `agent.rs:4265-4310` is `:6969-6973`.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| OD-1 | Who may press Run now | (a) creator only; (b) creator or org admin, like update/delete; (c) any member | **(a), enforced in capability-core and in the BFF.** Every fire runs under the creator's subject (Control decision `subject_id` = `creator_subject_id`, `sweeper.go:263-266`). An admin pressing Run now would start a run under someone else's identity at a moment that person did not choose. Pause and delete only reduce authority; Run now spends it. Admins keep pause and delete | S2, S3 | No for S2 (default (a) is taken if not ruled; it is the strictest) |
| OD-2 | Fleet console | (a) proxy behind a Quarry-audience BFF; (b) keep hidden; (c) delete the code | **(b) now, (c) when Quarry rules it has no fleet model.** No producer starts a fleet (`App.tsx:20-21`). Proxying adds a cross-plane route with nothing behind it. S0 records it; S5 removes any link to it | S0, S5 | Yes |
| OD-3 | Hub states | Perplexity Active / Paused / Completed vs ours | **Active, Paused, Expired.** Our schedules are recurring cron; nothing completes. Expired = approval lapsed (`paused_expired`). "Completed" comes with one-shot schedules (OUT). Paused shows its reason (by you, after failures) | S1 copy | Yes (default) |
| OD-4 | Definition schema and version semantics | (a) mutable row + history log; (b) immutable published versions + one draft; (c) git-like branches | **(b).** `agentDefinitionVersions` rows are immutable, numbered per agent, with a canonical digest; `agents` keeps a pointer `publishedVersion` and an editable draft. A binding and a run pin a version; publish never changes a running turn. Rollback = publish an old version again as a new number. Matches the only versioned reference (ChatGPT Workspace Agents, audit 2072) | S7-S11 | No for S7+; S0-S6 free |
| OD-5 | DEC-14 join: where an agent's tool allowlist and autonomy cap live | (a) Convex definition, applied tighten-only at invoke; (b) a second record in capability-core | **(a).** The definition says what an agent is (Convex, ADR-0002). capability-core stays the policy of record (DEC-14) and gains no agent table. The agent cap is one more input to WS25-S6 strictest-wins resolve and can only lower the result. The allowlist narrows what WS05 bindings and the org policy already allow; it never grants. Settles the 3.3.3 [RECONCILE] | S9, S10 | No for S9/S10 |
| OD-6 | Named-agent authority (phase 3) | (a) runs as the invoking person; (b) service principal per agent; (c) creator's delegated authority stored | **(a) for interactive turns; WS20 stored consent for unattended runs; never (b) or (c).** A named agent is a definition plus memory, not a principal. It never holds a standing token and never widens an owner-plane ACL (audit 10.1 #3) | P3 | Yes for phase 1-2 |
| OD-7 | Org-wide schedule list | (a) keep: any member reads every schedule with its template; (b) capability-core filters to own schedules unless admin | **(b), as a WS20 depth fix, not EP-6 scope.** The template holds the goal text. EP-6 adds a `creator=self` filter (filter, never grant) so the hub never asks for more than it shows. Widen D2-121 to cover the list | S1 uses the filter once S2 adds it; client filter until then | Yes |
| OD-8 | Do manual fires count toward the 3-failure auto-pause (WS20-S8) | yes / no | **Yes.** A manual fire is a real fire of the same contract and template; `tripsFailureLimit` reads `cron_fires` and needs no change | S2 | Yes |
| OD-9 | Run now limits | none / cap | **One manual fire in flight per schedule; at most 6 manual fires per schedule per rolling hour.** Each fire costs inference (WS27 meters it like any fire). Limits are constants in v1, typed 409/429 refusals | S2 | Yes |

Ledger entries S0 adds (newest first, `MP/docs/decisions/ledger.md`, CRLF): "Run now is a manual fire through the same Control fire authorisation; creator only (EP-6 S2, OD-1, OD-8, OD-9)"; "Fleet console stays hidden until Quarry has a fleet model (EP-6, OD-2)"; "Agent definitions stay in Convex and are versioned there; capability-core keeps policy only (EP-6, OD-4, OD-5, DEC-14)" (proposed until the owner rules). Nothing is reversed.

## 4. Slices

Test commands (from the repo root unless stated). Go: `cd "apps/Model Plane/go/services/capability-core" && go test ./internal/cron/... ./internal/api/... ./internal/taskexec/...` (no `-race`: needs cgo). MG: `cd "apps/Model Plane/rust" && cargo test -q -p model-gateway --lib cron_` (two cargo targets fail pre-existing on Windows; use `--lib`). BFF: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets agent_actions`. FE from `FE`: `pnpm exec vitest run <paths>`, `pnpm lint`, `pnpm typecheck` (memory: the root tsconfig checks nothing; the real check is `tsc -p tsconfig.app.json` with the TS6 alias, and there are pre-existing failures to diff against). Convex from `apps/Application Plane/convex-core`: `pnpm test` (`tsx --test test/*.test.cjs`). ZDR: `bash scripts/tests/zdr-contract-sync.test.sh`, `cargo test -p model-gateway --test zdr_posture_matrix`.

### Phase 1

#### S0 - Decision records [FINISH]
**Goal.** Record OD-1, OD-2, OD-3, OD-8, OD-9 as decided at the defaults and OD-4, OD-5, OD-6, OD-7 as proposed. **Files.** `MP/docs/decisions/ledger.md` (three entries above, CRLF); `docs/plans/model-plane-depth-register.md` add rows: EP-6 OD-4/OD-5/OD-6 owner decisions (P1, decision), OD-7 widening of D2-121; `apps/master-ownership-matrix.md` and `MP/docs/capability-ownership-matrix.md`: add "Run now (manual schedule fire)" under capability-core with Control as signer. **Contract/proto/migration.** None. **Flag.** None. **Tests.** None. **DoD.** Entries reviewed by the lead; plan 3.3.2 EP-6 "must be designed before starting" items each point at an OD row. **Effort.** S, 0.5-1 d. **Deps.** none. **Risks.** none.

#### S1 - Automations hub over existing routes [BUILD]
**Goal.** One page, `/agents/automations`, lists the person's schedules with state filters (Active, Paused, Expired) and row actions Pause/Resume, Edit time, See results, Renew approval, Delete. This is the cheapest real value: it needs no backend change and gives schedules a home outside Settings.

**Files.**
- `FES/shared/actions/action-registry.ts` (+ `action-registry.test.ts`): register first (repo rule). `schedule.list` (ownerPlane `model`, risk `low`), `schedule.pause`, `schedule.resume` (risk `medium`; resume names the lapsed-approval refusal), `schedule.update_time` (`schedule_expr`, risk `medium`), `schedule.delete` (risk `high`, `requiresApproval: false`: the confirm dialog is the approval), `schedule.fires.list` (closes D0-017, text from the W2-13 shared edits). All `requiresApproval: false`, `modelAccess: none` in the WS24-S5 side map (`FES/shared/actions/model-access.ts`): the model must not drive these.
- NEW `FES/features/automations/lib/automation-state.ts`: pure `automationStateOf(schedule, lastFire, now)` returning `active | paused_by_owner | paused_failures | expired` (expired from `approvalOf` logic moved here from `ScheduledRunsPanel.tsx:59-69`, which then imports it), and `filterAutomations(list, filter)`.
- NEW `FES/features/automations/components/AutomationsHub.tsx`: header, filter tabs, list. Row shows name, cadence label (`cronExprToLabel` from `schedule-from-chat.ts`), next run, state chip, approval expiry badge. Row actions call `updateCronSchedule` / `deleteCronSchedule`; "See results" mounts the existing `ScheduleFires` (export it from `ScheduledRunsPanel.tsx`, no content change) behind the existing `VITE_SCHEDULE_RESULTS` rule; "Renew approval" opens `ScheduleDialog` exactly as the panel does (`ScheduledRunsPanel.tsx:328-364`). "Edit time" opens a small cadence picker that PATCHes `schedule_expr` only; editing the goal is "Duplicate with changes", which opens `ScheduleDialog` prefilled (a new Control create decision; no in-place goal edit, because the template digest is signed).
- NEW `FES/features/automations/components/AutomationsHubPage.tsx` (page frame, i18n `nb`/`en`); `FES/app/App.tsx` route `{ path: '/agents/automations', component: AutomationsHubPage }` beside `:241`.
- Data: `listMySchedules` (own schedules; `schedule-from-chat.ts:110`). Paused reason needs the last fire's outcome: the hub reads `listScheduleFires(org, id, 1)` lazily per paused row only (at most one call per paused schedule); S2 later adds `last_outcome` to the list so this call goes away.
- Empty and ZDR states: if the list call returns a ZDR refusal envelope, show the typed refusal copy for feature `scheduled_step` (existing key in `MP/api/contracts/zdr-refusals.json`); otherwise "No automations yet" with a link to how to schedule from chat.

**Contract/proto/migration.** None server-side. **Flag.** None: a new route over existing calls; no server behaviour changes (VITE flags are not used for server features, `flags-and-config.md` section 0). Rollback: revert.
**ZDR.** Reads only. No new store. ZDR orgs cannot create schedules (Control refuses), so the hub is empty and shows the refusal copy.
**Tests.** NEW `automation-state.test.ts` (state per case: enabled; disabled no fires; disabled with last outcome `paused_failures`; lapsed approval; contract-free schedule never expired; filter counts). NEW `AutomationsHub.test.tsx` (renders three filters with counts; Pause calls `updateCronSchedule(..., {enabled:false})`; Resume on a lapsed approval shows Renew, not a toggle; Delete asks to confirm; See results hidden when `VITE_SCHEDULE_RESULTS` is not `'true'`; keyboard focus order; 375 px layout without horizontal scroll). UPDATE `ScheduledRunsPanel.test.tsx` (imports moved, behaviour unchanged). `action-registry.test.ts` (six new ids, schemas, `modelAccess: none`). Commands: `pnpm exec vitest run src/features/automations src/features/chat/components/ScheduledRunsPanel.test.tsx src/shared/actions`, `pnpm lint`, `pnpm typecheck`.
**DoD.** A person with two schedules (one paused) sees both under the right filters, pauses and resumes one, deletes one with a confirm, and opens Renew on a lapsed one. No server change.
**Evidence.** Second dev server on the worktree (memory: `:5173` serves the main checkout and needs a restart for host edits); screenshots at 1280 and 375 px for each filter; vitest output.
**Effort.** M, 3-5 d. **Deps.** none to build. **Risks.** duplicate UI until S4 (accepted for one slice); the client-side "mine" filter still downloads the org list (OD-7).

#### S2 - Run now through the same Control fire authorisation [BUILD]
**Goal.** `POST /api/v1/cron/{id}/run` creates one manual fire. It goes through exactly the path a cron fire takes: a fresh Control `model.cron.fire` decision before the task exists, then the dispatcher's second `AuthorizeFire` and `AuthorizeScheduledRun` before `StartWorkflow`. Nothing in this route calls `StartWorkflow`.

**Files.**
- `CC/internal/cron/sweeper.go`: extract the body of `fireOne` (`:381-418`) into `InsertFire(ctx, tx, d DueSchedule, fireKey string, now time.Time) (fireID, taskID string, err error)`. `fireOne` calls it with the slot key, so cron behaviour is byte-identical (same inserts, same values). Export the minimal `DueSchedule` view (id, org, space, subject, template digest, template) or add a constructor; keep the sweeper's own struct private.
- NEW `CC/internal/cron/run_now.go`: `RunNow(ctx, pool, authorizer FireAuthorizer, req RunNowRequest, now) (RunNowResult, *RunNowRefusal)`. One transaction:
  1. `SELECT ... FROM cron_schedules WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL FOR UPDATE` (blocks a concurrent run-now; the sweeper skips the locked row for one tick).
  2. Refuse `not_found` (404) when absent; `run_now_not_creator` (403) when `actor_id != creator_subject_id` (OD-1); `approval_expired` (409) when `ApprovalContractLapsed(template, now)` (`sweeper.go:526`); `invalid_schedule` (409) when `NextFrom` fails.
  3. Idempotency: `fire_key = "manual:" + request_id`, where `request_id` is a client UUID (regex `^[A-Za-z0-9-]{16,64}$`, else 400 `invalid_request_id`). If a `cron_fires` row with `(schedule_id, fire_key)` exists, return it with 200 and `deduplicated: true` (the non-unique index from 0056 serves the lookup; the row lock serialises writers, so no unique index is needed).
  4. Limits (OD-9): 409 `run_now_in_flight` when a `manual:%` fire of this schedule is `pending` or `running` and younger than 15 minutes; 429 `run_now_rate_limited` when 6 manual fires exist in the last hour.
  5. `authorizer.AuthorizeFire(FireIntent{OrgID, SpaceRef, SubjectID: creator, ScheduleID, FireKey, TemplateDigest, IdempotencyKey: id + ":" + fireKey})`. On refusal: write a `failed` fire with error "fresh schedule authorization failed" and return 403 `fire_not_authorized`; do NOT disable the schedule (unlike the sweeper at `:269-279`: a manual refusal says nothing about the next slot) [decision recorded in S0].
  6. `InsertFire(..., fireKey, now)`; do not touch `next_fire_at` or `last_fire_at`.
  Note: step 5 holds the row lock across one Control HTTP call, as the sweeper already does inside its transaction; the HTTP client timeout bounds it.
- `CC/internal/api/workplane_apis.go`: `CronHandler` gains `runNow bool` and `authorizer cron.FireAuthorizer` with `WithRunNow(authorizer, enabled)`. In `Register` (`:1108-1131`) route `{id}/run` POST to `h.runNow` before the generic switch (same `CutSuffix` pattern as fires). Off or no authorizer = 404. Response 202 `{data:{fire_id, fire_key, task_id, status:"pending", deduplicated}}`; refusals `{error:{code, message, details}}` (new local helper `writeTypedError`; existing routes keep `jsonErr`). List response: add `features: {run_now: bool}` (additive key) and, per row, `last_outcome` and `last_fire_status` from a `LEFT JOIN LATERAL (SELECT status, outcome FROM cron_fires WHERE schedule_id = s.id ORDER BY fired_at DESC LIMIT 1)`; add optional `creator=self` filter (`AND ($3 = '' OR creator_subject_id = $3)` with `$3 = verifiedActorID(r)`), a filter only (OD-7).
- `CC/cmd/main.go`: build `cronAuthorizer` before the handler (move `:352` above `:337`) and call `cronHandler.WithRunNow(cronAuthorizer, os.Getenv("CRON_RUN_NOW_ENABLED") == "true")` only when `cronAuthErr == nil`.
- `MG/http_routes.rs`: route `.route("/v1/cron/:id/run", post(run_cron_now_proxy))` beside `:551`; handler forwards `cron/{id}/run` POST with the body (URL-safe id check as for fires). Add `"creator"` to the typed list query (only the literal `self` is forwarded).
- `BFF/domains/agent_actions.rs`: `.route("/api/v1/cron/{cron_id}/run", post(run_cron_now))`. The handler reads the schedule (as `authorize_own_schedule` does) and refuses with 403 unless `creator_subject_id == user.user_id`; no admin short-circuit (OD-1). Strips forged scoping headers like its siblings; generates nothing itself (the client supplies `request_id`).
- `FES/shared/api/cron-client.ts`: `runScheduleNow(orgId, cronId, requestId)`; `CronSchedule` gains optional `last_outcome`, `last_fire_status`; `listCronSchedules` returns `features`.
- `FES/shared/actions/action-registry.ts`: `schedule.run_now` (ownerPlane `model`, risk `high`, `requiresApproval: false`; description says it runs under the creator's existing approval and counts against limits), `modelAccess: none`.
- Docs: `MP/docs/ZDR.md` one line (Run now refused under ZDR at three layers); `MP/docs/capability-ownership-matrix.md` row (S0).

**Contract/proto/migration.** No proto. No migration (session-core head 0071, capability-core head 0030 stay). Wire: new route and two additive list fields.
**Flag.** `CRON_RUN_NOW_ENABLED` (capability-core, strict literal `true`, default off; compose passthrough `${CRON_RUN_NOW_ENABLED:-false}` in the capability-core block). Off: route 404, `features.run_now=false`, list rows still gain `last_outcome` (read-only, no behaviour change). Turned on by S6's live proof.
**ZDR.** Refused at Control (both decisions refuse ZDR, `schedule_fire_decision.go:99-101`), at the capability-core router (`authz.go:81`, non-read from a ZDR principal) and at the dispatcher (`control_authorizer.go:105`, `:359`). Waived runs: a scheduled run is never waived (WS20 ADR-0006: "never scheduled or unattended"). New persisted data: one `tasks` row and one `cron_fires` row, the same columns a cron fire writes, content-free except the template the schedule already stores.
**Tests.**
- Go `internal/cron/run_now_test.go`: `TestRunNowCreatesPendingFireWithManualKey`, `TestRunNowRefusesNonCreator`, `TestRunNowRefusesLapsedApproval`, `TestRunNowDeduplicatesSameRequestID`, `TestRunNowRefusesWhileManualFireInFlight`, `TestRunNowRateLimit`, `TestRunNowAuthorizerRefusalWritesFailedFireAndKeepsScheduleEnabled`, `TestRunNowDoesNotMoveNextFireAt`. DB-backed variants as integration tests in the style of `sweeper_failure_integration_test.go` (lead runs them with the throwaway Postgres harness).
- Go `internal/cron/sweeper_test.go`: `TestFireOneUnchangedAfterInsertFireExtract` (same SQL arguments for a slot fire).
- Go `internal/taskexec`: `TestDispatcherReauthorizesManualFire` (a manual-key intent goes through `AuthorizeFire` and `AuthorizeScheduledRun`; system thread key `schedule/<id>/manual:<uuid>`).
- Go `internal/api`: `TestCronRunRoute404WhenOff`, `TestCronRunRouteTypedErrors`, `TestCronListAddsLastOutcomeAndFeatures`, `TestCronListCreatorSelfFilter`, `TestCronRunRefusedForZDRPrincipal` (router).
- MG: `cron_run_path_forwards_only_the_id`, `cron_list_forwards_only_creator_self`.
- BFF: `run_cron_now_refuses_admin_who_is_not_creator`, `run_cron_now_forwards_for_creator`, `run_cron_now_strips_forged_scope_headers`.
- FE: `cron-client.test.ts` (`runScheduleNow` request shape), `action-registry.test.ts`.
**DoD.** With the flag on, the creator presses Run now once; exactly one task and one `manual:` fire appear; the dispatcher logs two fresh Control decisions; the fire reaches `completed` (or a truthful failure) with `run_id` and `thread_id`; a retry with the same `request_id` returns the same fire; a second member and an admin get 403; the cron slot still fires on time.
**Evidence.** See section 6, flow F2.
**Effort.** M, 4-6 d. **Deps.** OD-1 (default taken), WS20-S1 migration 0056 applied (D2-118/119) for the `fire_key` column. **Risks.** a refactor of `fireOne` touches the hot sweeper (mitigated by the byte-identical test and D1-756's savepoint tests staying green); Control rate on decisions per fire doubles for manual fires (bounded by OD-9).

#### S3 - Run now in the hub [BUILD]
**Goal.** A Run now button per row when `features.run_now` is true and the viewer is the creator; the fires list shows a "Manual" chip for `manual:` keys.
**Files.** `FES/features/automations/components/AutomationsHub.tsx` (button, busy state, typed error copy for `run_now_in_flight`, `run_now_rate_limited`, `approval_expired`, `fire_not_authorized`; generates `request_id` with `crypto.randomUUID()` once per click and reuses it on retry); `ScheduledRunsPanel.tsx` `ScheduleFires` (chip "Manuell"/"Manual" when `fire_key` starts with `manual:`; the fires row needs `fire_key`, so `cronFireRow` in `workplane_apis.go:1263-1272` gains `fire_key` (additive, content-free) in this slice); `automation-state.ts` (paused reason from `last_outcome` instead of the lazy fires call).
**Contract/proto/migration.** Additive `fire_key` on the fires JSON. **Flag.** Server capability `features.run_now` only.
**ZDR.** Same as S2. No new data.
**Tests.** `AutomationsHub.test.tsx`: `run now hidden when features.run_now is false`, `run now hidden for a schedule I did not create`, `run now reuses request id on retry`, `typed refusal copy per code`; `ScheduledRunsPanel.test.tsx`: `manual fire shows manual chip`; Go `TestListFiresIncludesFireKey`.
**DoD.** Click, see "Started", the new fire appears at the top of See results with the Manual chip and links to its transcript once done.
**Effort.** S, 1-2 d. **Deps.** S1, S2. **Risks.** none beyond S2.

#### S4 - One schedule list [FINISH]
**Goal.** Remove the second list without losing a WS20 guard. The chat panel becomes a compact mount of the hub list ("mine", max 5 rows, link "See all automations"). The Settings section keeps the admin-only create form with workflow fields and shows the hub list in org scope for admins.
**Files.** `FES/features/automations/components/AutomationsList.tsx` (NEW, extracted from S1 with props `scope: 'mine' | 'org'`, `compact`, `limit`); `ScheduledRunsPanel.tsx` (render `AutomationsList scope="mine" compact`; keep `ScheduleDialog` for renew); `CronSchedulesSection.tsx` (keep the create form `:200-260`; replace its list `:300-340` with `AutomationsList scope="org"`). Org scope uses the full list call; mine uses `creator=self` once S2 is deployed, else the client filter.
**Contract/flag/migration.** None.
**ZDR.** Unchanged.
**Tests.** UPDATE `ScheduledRunsPanel.test.tsx` and `CronSchedulesSection.test.tsx` so every WS20 assertion still holds against the shared list: Renew on lapsed approval, no bare re-enable, `paused_failures` label, See results flag, delete confirm. NEW `AutomationsList.test.tsx` (`mine never renders another creator's row`, `org scope shows creator name for admins`). Commands as S1.
**DoD.** One list component in the tree for schedules (grep shows a single `updateCronSchedule(` call site for toggling); all three surfaces behave as before.
**Effort.** S-M, 2-3 d. **Deps.** S1. **Risks.** regressing the WS20-S8 Renew path (risk 3 in plan EP-6); covered by the moved tests.

#### S5 - Agents hub landing over real resources [FINISH]
**Goal.** `/agents` opens on what exists: your agents (from installations), your automations (count and next run, from S1's data), recent runs (link to `/agents/runs`) and Customize. Role blueprints move under a "Templates (preview)" section and keep the existing Blueprint badge. No new backend.
**Files.** `FES/features/agents/components/AgentsPage.tsx` (`AllRolesOverview` fallback `:47-52`: new top section `AgentsHubSummary`; blueprint grid below a "Templates" heading); NEW `FES/features/agents/components/AgentsHubSummary.tsx` (uses `getAgentInstallations` from `FES/shared/api/spaces-client.ts` as `AgentInstallationsPage` does, and `listMySchedules`); `FES/app/shell/navigation.ts:73-78` description "Your agents, automations and runs." Nothing links to `/agents/fleets/*` (OD-2).
**Contract/flag/migration.** None. Action contracts: none new (reads only; the existing installations read is already wired without a contract, so S5 registers `agents.installations.list` with `modelAccess: none` to meet the repo rule).
**ZDR.** Reads only.
**Tests.** NEW `AgentsHubSummary.test.tsx` (empty states, counts, links); UPDATE `AgentsPage.test.tsx` (blueprints under Templates, badge still present, activate still disabled).
**DoD.** A new user sees honest empty states with next steps; a user with one bound agent and one schedule sees both.
**Effort.** S, 2-3 d. **Deps.** S1 (data hooks). **Risks.** none.

#### S6 - Phase 1 flip and live proof [FINISH]
**Goal.** Turn `CRON_RUN_NOW_ENABLED` on in dev after the flows in section 6 pass; record evidence.
**Files.** `MP/docker-compose.yml` passthrough (if not done in S2); `MP/docs/decisions/ledger.md` flip entry; `docs/plans/model-plane-depth-register.md` rows closed; plan Build ledger entry.
**Deps.** S1-S5; WS20-S1 live (D2-122/123: `SESSION_CORE_SCHEDULED_RESULT_PERSIST=true` on the proof stack) and WS20-S3's D2-325/326 (owner can open a service-owned scheduled thread). Without those, Run now still fires and records a truthful fire status, but "See results" has no transcript to open, so the flip waits.
**Effort.** S, 1-2 d.

### Phase 2 (slice level)

#### S7 - Versioned definitions in Convex [BUILD, gated OD-4]
- `CVX/schema.ts`: NEW table `agentDefinitionVersions` {`agentId`, `orgId`, `version` (int, per agent, from 1), `name`, `systemPrompt`, `model`, `toolCapabilityIds: string[]` (capability ids from the WS05 registry, not free tool names), `autonomyCap` (`read_only | ask_first | auto`, the WS25-S6 mode vocabulary; never `lockdown`-widening), `knowledgeBindings` (copied), `digest` (`sha256:` over a canonical JSON of the version body), `publishedBy`, `publishedAt`}, indexes `by_agent_version`, `by_org`. `agents` gains optional `publishedVersion`, `draft` (object with the same body fields). Old rows: `publishedVersion` absent = legacy, served as today.
- `CVX/agents.ts`: `saveDraftForGateway`, `publishForGateway` (copies the draft to a new immutable row, bumps the pointer, returns `{version, digest}`), `listVersionsForGateway`, `getVersionForGateway`. All `assertServiceKey`. `agents:update` stays for legacy fields but refuses `systemPrompt`/`tools` writes once `publishedVersion` exists (one write path for versioned content).
- `CVX/spaceAgents.ts`: `agentPersonaForGateway` returns `{name, systemPrompt, version, digest, toolCapabilityIds, autonomyCap}` for the published version (legacy shape unchanged when unversioned).
- `spaceAgentBindings` gains optional `pinnedVersion` (absent = follow published).
- Tests: `test/agent-definition-versions.test.cjs` (publish is immutable, digest stable across key order, legacy agent unaffected, cross-org read refused). Convex has no numbered migrations (`migrations-protos.md` row Convex).
- Flag: `AGENT_DEFINITION_VERSIONS` read by the BFF (Convex functions exist but are not called while off).
- Effort M, 4-6 d. Startable: no, needs OD-4.

#### S8 - Builder with draft preview [BUILD]
- BFF routes `GET/PUT /api/v1/agents/{agent_id}/draft`, `POST .../publish`, `GET .../versions`; admin-or-creator gate like skills (`can_author_skills`, `BFF/domains/agent_actions.rs:64-67`) for org agents, Space manager for Space-created agents [verify which role Control returns]. Action contracts `agents.draft.save`, `agents.publish`, `agents.versions.list` before wiring.
- FE builder page `/agents/:agentId/edit`: instructions, model (only models WS10 entitles), tool picker over capability ids (WS05 registry read), autonomy cap, publish history with diff of instruction text.
- Preview = an ordinary Ask turn with the draft persona injected by the BFF and `agent_mode: read_only` (WS25-S6), in a temporary chat, so it stores no transcript. No sandbox run in phase 2 ([INF] the audit's "sandbox preview" is better served by WS22's thread workspace later).
- Effort M, 5-7 d.

#### S9 - Invoke-time pin, tighten-only allowlist and autonomy cap [BUILD, gated OD-5]
- BFF `inject_mentioned_space_agent_persona` (`BFF/domains/spaces.rs:4496`): with `AGENT_VERSION_PINNING` on, resolve the binding's pinned or published version and add `agent_ref`, `agent_version`, `agent_digest`, `agent_tool_capability_ids`, `agent_autonomy_cap` to the body. Client-supplied values for these keys are stripped first (forged scoping, CLAUDE.md BFF rule).
- MG `InvokeRequest` (`MG/http_routes.rs:8856` area) gains the five fields (`serde(default)`); `sse.rs`: the allowlist intersects the tool set after WS05 bindings and the org policy, never adds; the cap joins WS25-S6 strictest-wins as one more input. The run's `runs.metadata.agent = {ref, version, digest}` is written at StartRun (JSONB, no migration).
- Proto: none if the Do path receives the narrowed tool list through existing fields [verify at design time of the slice: `RunAgentRequest` next free is 25 (`execution.proto:189` is 24); take 25 only if execution-core must see the agent ref].
- Flags `AGENT_VERSION_PINNING` (BFF, default off), `MODEL_GATEWAY_AGENT_TOOL_ALLOWLIST` (default off).
- Tests: a mention of a v2-pinned binding injects v2 even after v3 publishes; an allowlist naming a tool the org policy blocks offers nothing; cap `read_only` with user mode `auto` resolves `read_only`; forged `agent_tool_capability_ids` from the client are stripped.
- Effort M, 5-7 d. Deps: S7, WS25-S6 live (`AGENT_MODES`), WS05-S3.

#### S10 - Role parameter on `subagent.task` [BUILD]
- MG composes a bounded role catalogue for a Do run from the published definitions the caller may use (Space-bound agents of the thread's Space, max 8 entries, instructions capped at 4 KB each) and sends it in `RunContext`.
- Proto (additive, commented, owner tag in the doc comment): `execution.proto` `RunContext` add `repeated AgentRoleSpec agent_roles = 7;` (1-4 used at `:333-342`; 5 reserved for WS08's attachments fallback and 6 for WS20 run notes, plan 4.3). NEW message `AgentRoleSpec { string agent_ref = 1; uint32 version = 2; string digest = 3; string name = 4; string instructions = 5; repeated string tool_capability_ids = 6; string autonomy_cap = 7; }`. Run `buf lint` against the ratchet.
- EC: `subagent.task` parameters gain optional `role` (an `agent_ref` from the catalogue). The child gets that role's instructions and a tool set = parent's tools ∩ role allowlist; it stays read/analysis only and one level deep (unchanged contract, `agent.rs:6969-6973`). An unknown role is a tool error, never a fallback to a wider set. Lineage records `agent_ref@version` on the child run.
- Flag `EXECUTION_CORE_SUBAGENT_ROLES` (default off; off = the catalogue is ignored and the parameter is not offered).
- Tests: `subagent_role_narrows_tools_never_widens`, `subagent_unknown_role_is_tool_error`, `subagent_role_records_version_on_child`, cross-service `cross_service_loop_contract` stays green.
- Effort M (L if the catalogue must also reach the scheduled lane; it must not in v1: scheduled runs keep `max_subagents: 0`, DEC-16), 4-6 d.

#### S11 - Preset agents with real action sets [FINISH]
- `FES/shared/actions/preset-agents.ts`: `actionIds` filled from admission rows that are `enabled` and pinned (WS24-S6 `agent_action_admissions`, `OWNER_ACTION_PINS`); a preset whose actions are not admitted shows "not available in this org" instead of an empty run.
- Effort S, 1-2 d. Deps: WS24-S6 live (`MODEL_ACTION_ADMISSION=table`), S9.

### Phase 3 outline: persistent named agents with scoped memory (not sliced)

Depends on EP-8 slice 1 (F-t3-060: one `(scope_kind, scope_id)` memory key with an audience predicate, owner session-core with SPACE, plan 2.6 row 13) and OD-6.

- A named agent = a published definition (S7) + its own thread list + memory under `scope_kind='agent'`, `scope_id=<agent_ref>` with the audience predicate of the Space or person it serves. No new definition store, no new principal.
- Authority (OD-6): an interactive turn runs as the person present, with that person's delegated bearer; the agent's allowlist and cap narrow it. An unattended run of a named agent is a WS20 schedule whose template names the agent version; it carries the WS20 stored-consent contract and service credentials only. A named agent never holds a token between turns and never reads memory written for another audience.
- Memory reads and writes go through WS07 controls (user off/paused, org disable) and WS26 write policy. Writes are refused under ZDR and for waived runs (DEC-32).
- Must be designed before slicing: the agent memory audience rule (who may read an agent's memory in a shared Space), deletion when the definition is deleted (GDPR order in `SC/src/gdpr.rs`), and how the hub shows "what this agent remembers".
- Rough size: L, after EP-8 slice 1 is live.

## 5. ZDR clause

| New store or egress | Slice | ZDR / waived behaviour | Refusal | Golden case (proposed names, integrator aligns) |
|---|---|---|---|---|
| Manual fire: one `tasks` row, one `cron_fires` row (same columns as a cron fire) | S2 | Refused; nothing written | Control 403 at create and fire (existing); capability-core router 403 (`authz.go:81`); dispatcher refusal (`control_authorizer.go:105`, `:359`). FE shows feature `scheduled_step` copy (existing key: no change to `zdr-refusals.json` or its `.sha256`) | `zdr-posture-matrix.json` row `scheduled_run_now` (refuse for `zdr` and `zdr_waived` cells); Rust `zdr_posture_matrix.rs` case `e_zdr_token_schedule_run_now_refused`; Go `TestCronRunRefusedForZDRPrincipal`. Lands with or after WS20's `scheduled_run` row (D2-124) |
| List fields `last_outcome`, `last_fire_status`, `fire_key` | S2, S3 | Read-only, content-free tokens | n/a | none (no new store) |
| Agent definition versions (Convex) | S7 | Authored configuration, not conversation content; allowed for ZDR orgs like org instructions. The builder never stores example transcripts or preview output | n/a | `zdr-posture-matrix.json` note row `agent_definition_version` (content class: authored config); Convex test `versions store no transcript fields` |
| Builder preview turn | S8 | Temporary chat: no transcript stored; under ZDR it takes the ZDR direct stream with persona text and no tools [verify live] | existing ZDR chat path | `e_zdr_token_agent_preview_stores_nothing` |
| `runs.metadata.agent` {ref, version, digest} | S9 | Ids and digests only. ZDR runs record no run row today (persistence-free path), so nothing is written | n/a | `e_zdr_token_agent_pin_writes_no_run_metadata` |
| `RunContext.agent_roles` (in-flight only) | S10 | Do mode and delegation are refused under ZDR (`MP/docs/ZDR.md`), so the catalogue is never built for ZDR | existing `agent_run` refusal | `e_zdr_token_subagent_roles_not_offered` |
| Named-agent memory | P3 | Refused under ZDR and waived (DEC-32) | feature `named_agent` (proposed new key; adds a `zdr-refusals.json` entry and sha change in P3) | `e_zdr_token_named_agent_memory_refused`, `e_waived_named_agent_memory_refused` |

Notifications from manual fires reuse WS20-S7's content-free `SCHEDULED_RUN_OUTCOME` (no goal, answer or tool text). No content reaches Temporal history or logs from S2: the task carries the non-secret intent only (`sweeper.go:367-372`), the decision token is never persisted (`control_authorizer.go:46-53`).

## 6. Proof plan

Stack: Model Plane via `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build capability-core model-gateway` (memory: worktree builds must be followed by `recreate-with-live-env.sh`, never grep env files); Control via `scripts/run-control-plane.sh` (bare `docker compose build` does nothing); BFF image rebuild; FE second dev server on the worktree. Proof switches on the proof stack only: `CRON_RUN_NOW_ENABLED=true`, `SESSION_CORE_SCHEDULED_RESULT_PERSIST=true`, `VITE_SCHEDULE_RESULTS=true`, migration 0056 applied first (D2-118/119).

| Flow | Steps | Evidence |
|---|---|---|
| F1 hub (S1, S4, S5) | Create two schedules from chat; pause one; open `/agents/automations`; switch filters; Renew a schedule whose approval expires in 2 minutes (S8 guard on) | Screenshots 1280/375 px; network log shows only list, PATCH, fires calls; vitest output |
| F2 Run now (S2, S3) | As creator press Run now; capture the BFF request, capability-core 202, `psql` rows in `tasks` and `cron_fires` (`fire_key='manual:<uuid>'`, `pending` then `completed`), dispatcher log lines for `AuthorizeFire` and `AuthorizeScheduledRun`, the session-core thread `schedule/<id>/manual:<uuid>`, the transcript opened from See results | Saved JSON and SQL output; screenshot of the Manual chip |
| F3 negatives | Same `request_id` twice (one fire, `deduplicated:true`); second click while running (409 `run_now_in_flight`); another member and an org admin (403); lapsed approval (409); `CRON_RUN_NOW_ENABLED` off (404, no button); ZDR org token (403 at capability-core, no rows) | Response bodies and row counts |
| F4 no regression | The cron slot fires on time during F2; `next_fire_at` unchanged by Run now; the sweeper's savepoint tests and `TestFireOneUnchangedAfterInsertFireExtract` green | Go test output; `cron_fires` timeline |
| F5 phase 2 (S9) | Publish v1, bind, mention, publish v2, mention again with the binding pinned to v1 | MG request log shows `agent_version=1`; `runs.metadata.agent` |

Unit and contract gates per slice are listed in section 4. The scheduled-step proof script (`MP/scripts/tests/scheduled-step-proof.sh`) gains one case: a manual fire passes the same step decision checks.

## 7. Risks and owner decisions

**Risks.**
1. Authority drift. Run now spends the creator's stored authority on demand. Mitigation: creator only at two layers, the same two Control decisions, limits (OD-9), typed refusals, audit rows through WS06 when the emitter lands (D2-272 tracks the WS20 equivalent).
2. Org-wide schedule list leaks goal text to every member (OD-7, D2-121). EP-6 does not fix it but stops adding to it (`creator=self`). The owner should rule before the hub ships an org view to admins only.
3. Two schedule UIs consolidated wrongly (plan EP-6 risk 3). Mitigation: S4 moves, not rewrites, the WS20 tests.
4. Scope creep into a workflow product (plan EP-6 risk 2). Mitigation: OUT list; no chains, no visual builder; `WorkflowBuilder` stays a preview.
5. Second definition store (DEC-14 [RECONCILE]). Mitigation: OD-5 keeps every definition field in Convex and every policy field in capability-core; S9 consumes both read-only.
6. Hot files: `sweeper.go` (WS20 S1/S8 and D1-756 fixes), `workplane_apis.go` (WS20 S4/S8), `BFF/domains/agent_actions.rs`, `MG/http_routes.rs`, `BFF/domains/spaces.rs` (Space session). Land S2 after any open WS20 depth packet on these files; S9 coordinates with the Space session on `inject_mentioned_space_agent_persona`.
7. A Run now result is invisible until WS20-S1 and S3 are proven live; flipping S2 alone would show fires with no transcript. S6 is gated on those rows.

**Owner decisions needed** (defaults are taken where "proceed without" says yes):
- OD-1 Run now is creator only (blocks S2 if overruled toward admins).
- OD-4 definition versioning model (blocks S7-S11).
- OD-5 DEC-14 join, tighten-only agent cap and allowlist (blocks S9, S10).
- OD-6 named-agent authority model (blocks phase 3).
- OD-7 whether capability-core should restrict the schedule list and fires to creator or admin (WS20 depth; affects S4's org view).
- Carry-over: DEC-16 (b) explicit owner yes (D2-037/040) before any contract schedule is fired by Run now on a production-like stack.
