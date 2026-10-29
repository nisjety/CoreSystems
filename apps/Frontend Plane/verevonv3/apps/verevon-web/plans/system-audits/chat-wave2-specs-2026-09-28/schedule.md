# Implementation spec: "Schedule this" from the chat page

Scope: `C:/dev/CoresSystem`. Frontend paths are relative to `apps/Frontend Plane/verevonv3`; Model Plane Go paths to `apps/Model Plane/go/services`; Rust paths to `apps/Model Plane/rust/services`; Control Plane paths to `apps/Control Plane/user-core`. All line numbers below were read directly from the files at investigation time (2026-09-28) and may drift by a few lines with unrelated edits.

## 1. What exists today (file:line evidence)

**Frontend — admin-only Settings surface, not reachable from `/chat`.**
- `src/features/settings/components/CronSchedulesSection.tsx` — full CRUD UI (list/create/toggle/delete), gated by `hasWorkspaceAdminAccess(session)` (:152, :347, :381). Lets the author pick one of 7 workflow types (`WORKFLOW_CHOICES`, :80-130) and a raw JSON `workflow_input`. Default timezone is `'UTC'` (:161), not `Europe/Oslo`.
- `src/shared/api/cron-client.ts` — `listCronSchedules`/`createCronSchedule`/`updateCronSchedule`/`deleteCronSchedule` against `/api/v1/cron[/:id]`. `createCronSchedule` already accepts an optional `space_ref` (:74-77). `updateCronSchedule`'s type claims a `description` field (:90) that the backend silently ignores (see below) — a pre-existing dead field, not something to build on.
- No composer slash-command, no message action, no chat sidebar entry exists that touches this API at all today. `grep` across `src/features/chat` for `cron`/`schedule` returns nothing.

**Gateway BFF (Rust, `apps/gateway`).**
- `src/domains/agent_actions.rs`: router `:51-55` — `GET/POST /api/v1/cron`, `POST/DELETE /api/v1/cron/{cron_id}`. `create_cron`/`update_cron`/`delete_cron` are gated by `can_author_skills` (`has_authorized_org_role(user, &["owner","admin"])`, :66-69) — **org-admin only**, regardless of whose personal Space the schedule lives in (:428-437, :498-507, :538-547).
- `src/domains/spaces.rs::inject_personal_schedule_create_context` (:420-608) — the real authorization plumbing for create: strips any forged authority fields, resolves `space_ref` from the browser or (if absent) the caller's **personal Space** via `personal_space_lifecycle` (:4614-4627), computes a `template_digest`, and gets a one-time Control decision (`model.cron.create`, schema `sha256:space-cron-create-v1`) from user-core. This is exactly the path a chat-originated schedule (no Space picker) will go through unchanged.
- `src/domains/chat/history.rs::read_durable_threads` (:311-352) requests `GET {model_gateway}/v1/threads?limit=…&origin=chat` and `chat_history_sessions` (:170-175) re-filters to `session.origin == "chat"` — **by design**, to keep Agent-Run-Console/Support threads out of chat history.

**model-gateway (Rust) → capability-core (Go) proxy.**
- `http_routes.rs:317-323` — `/v1/cron[, /:id]` proxied 1:1 to capability-core's `/api/v1/cron`.

**capability-core (Go) — the durable schedule store and sweeper.**
- `internal/api/workplane_apis.go::CronHandler` (:836-1065) — CRUD on `cron_schedules`. `patch` (:1011-1058) accepts **only** `enabled` and `schedule_expr`; `description` is not updatable despite the frontend type. `listOrCreate` GET supports an optional `space_ref` filter (:898-914) that **narrows**, never widens, visibility.
- `internal/cron/sweeper.go` — `Sweeper.RunOnce` (:95-170) fires due schedules every minute (`FOR UPDATE SKIP LOCKED`), creates a `tasks` row whose `config_json` carries the schedule's `task_template` **verbatim plus** a non-secret `schedule_fire_intent` (`taskConfigJSON`, :194-221).
- `internal/taskexec/workflow_dispatcher.go::Dispatch` (:210-262): for **any** task carrying a `schedule_fire_intent`, `reauthorizeScheduleFire` (:327-396) re-asks Control for fresh fire authority and — critically — asks Session Core to prepare a **service-owned** thread (`PrepareScheduledRunThread`). Then:
  ```go
  if preparedRun != nil {
      if req.GetWorkflowType() != DefaultWorkflowType {  // "InteractiveRunSupervision"
          return fmt.Errorf("taskexec: scoped scheduled run %s requires %s, got %s", …)
      }
      req.WorkflowType = ScheduledRunWorkflowType // "ScheduledRunSupervision"
      …
  }
  ```
  (:228-236). **This means: today, of the 7 workflow types `CronSchedulesSection.tsx` lets an admin pick, only `InteractiveRunSupervision` (the default) can ever fire successfully.** The other six (`DeepTaskWorkflow`, `WideResearchWorkflow`, `EvaluatorOptimizerWorkflow`, `MemoryConsolidationWorkflow`, `SkillPromotionWorkflow`, `FeedbackPromotionWorkflow`) fail *every* fire with this exact error, once `SetScheduledRunSession` is wired — and it is: `cmd/main.go:679-697`. This is a real, pre-existing, independent defect; it happens to make the "Schedule this" design simpler (see §2) but is worth flagging separately.

**orchestrator-core (Go) — the Temporal workflow.**
- `cmd/workflows/interactive_run.go::ScheduledRunSupervision` (:183-259) is "the one owner-only lane for a Space cron fire." Unlike `InteractiveRunSupervision`, which pauses on a `SignalApproval` channel when a step `NeedsApproval` (:130-153), `ScheduledRunSupervision` **hard-fails** the run the instant a step needs approval:
  ```go
  if len(stepOutput.Steps) > 0 && stepOutput.Steps[len(stepOutput.Steps)-1].NeedsApproval {
      return handleFailure(ctx, input.RunID, input.OrgID, "", input.Retention,
          "scheduled runs cannot continue without a separately authorized approval")
  }
  ```
  (:242-244). **There is no pause/resume/notify path for an effectful tool call inside a scheduled run today.** This is the central backend gap behind the item's "approvals for effectful tools" requirement.
- `internal/orchestration/workflowreg.go:106-159` — `ScheduledRunSupervision` is separately allowlisted with `DeniesZDR: true` (:117): a ZDR org's scheduled runs are refused outright, consistent with "keep residency/ZDR intact."

**session-core (Rust) — where the result lands.**
- `src/grpc.rs::prepare_scheduled_run_thread` (:4123-4206) creates/reuses a thread via `create_thread_inner_preverified` with `user_id: "service:orchestrator-core"` (:4138-4144) and `session_key: req.system_thread_key` (= `"schedule/{schedule_id}/{fire_key}"`, built at :823). **`origin` is left at its zero-value** — the request struct at :4141-4157 never sets it.
- `src/grpc.rs::list_threads` (:4890-4984): returns rows for `t.user_id = req.user_id` **unless** a verified `space_read_decision` was supplied, in which case the filter becomes `recipient_audience_revision <= ceiling` instead (:4968-4974).
- Net effect, confirmed by three independent, compounding facts:
  1. `history.rs`'s BFF thread listing asks for `origin=chat` and re-filters on it (`chat_history_sessions`) — the scheduled thread's blank origin fails this.
  2. Without a space-read decision, `list_threads` filters to `user_id = <human>` — the scheduled thread is owned by `service:orchestrator-core`, so it's excluded regardless of origin.
  3. **The mechanism that *would* supply that space-read decision explicitly refuses to for a personal Space**: `apps/Control Plane/user-core/internal/spaces/personal_thread_decision.go::ValidateForSharedThreadRead` (:198-215):
     ```go
     if e.Membership.Kind == KindPersonal {
         return fmt.Errorf("shared thread read decision requires a non-personal Space")
     }
     ```
     A chat-originated schedule (no Space picker) always resolves to the caller's **personal** Space via `inject_personal_schedule_create_context`. So there is currently **no Control decision a personal-Space owner can obtain to read their own schedule's result thread** — this is the root blocker for "where results land," not merely a wiring gap.
- Reading a thread's transcript once you *have* a decision already supports it end-to-end: `list_thread_messages` (`http_routes.rs:7925-7955`) forwards `space_read_decision_ref/token` straight into `ListConversationRequest`. So fixing the decision-issuance gap is the one backend change needed to make transcript reads work; nothing else in the read path needs to change.
- `PrepareScheduledRunThreadResponse` already returns `thread_id` (:4201-4205), but **nothing persists it back onto `cron_fires`** — `workflow_dispatcher.go` only forwards it into Temporal's workflow input (`attachScheduledRunInput`, :421-466). There is today no cheap query in capability-core that maps a schedule/fire to its thread id.

**Frontend extension points to reuse (already-existing patterns).**
- Composer slash-commands: `src/features/dashboard/home/DashboardComposer.tsx` — `BUILTIN_COMMANDS: ComposerCommand[]` (:394-451), `runCommand()` dispatch (:1312-1337), `ComposerCommand.builtin` union (:214: `"deep_research" | "file" | "image" | "web_search"`).
- Message action menu: `src/features/chat/components/ChatMessages.tsx` — the assistant message's overflow `MessageMenu` (:738-757) already lists "Fortsett i ny chat" / "Les høyt" / the four export items; `onRerunAsNewTurn` (:699-703) is wired from `ChatPage.tsx:783` to `rerunAsNewTurn(row.turn.id)` in `use-chat-controller.ts` (per the gap plan, :2096, :1450-1669) — the existing turn→goal-text resolution to mirror for scheduling a past request.
- Space Work surface: `src/features/spaces/components/SpaceWorkPanel.tsx` + `SpacePage.tsx` already render for `kind === 'personal'` (:1198, :1330, :1446), backed by `GET /api/v1/spaces/{space_ref}/threads` → `list_space_threads` (`spaces.rs:1075-1233`), which is space-ref-generic (no room/personal branch in the handler itself — the branch lives in `shared_thread_read_decision`, i.e. exactly the gap above).
- i18n: every string above uses `i18n.tr(norsk, english)` — matches the task's `i18n.tr('norsk','English')` convention throughout.

## 2. Exact change list

Given §1, the only workflow type that can ever fire successfully from a schedule is `InteractiveRunSupervision`, which happens to be exactly right for "rerun this request" — so **the chat-facing surface should not expose a workflow picker at all**; it always emits `workflow_type: "InteractiveRunSupervision"` (or omits it, since that's the dispatcher default) and puts the rerun text in `title`/`description`.

Split into a **Phase A** (frontend + a small authorization relaxation — everything needed for create/list/pause/delete) and **Phase B** (the Control-plane decision fix + a small persistence addition — needed only for "see the result"). Phase A alone is shippable and useful (a user can schedule a recurring request and pause/delete it) even before Phase B lands; the "where does it land" UI degrades to a "run history unavailable yet" state until Phase B ships. Recommend shipping both in Wave 2 as one unit, but the split is a real fallback if Phase B slips.

### 2.1 Frontend (`apps/Frontend Plane/verevonv3/src`)

**New file: `src/shared/api/schedule-from-chat.ts`** (thin wrapper over `cron-client.ts`, no new HTTP surface):
```ts
export interface ScheduleFromChatInput {
  orgId: string
  name: string                 // "Navn" — short, user-editable
  scheduleExpr: string          // built from a cadence preset or custom cron
  timezone: string              // default 'Europe/Oslo'
  goalText: string              // the exact request to rerun (draft or a past user turn)
  threadOriginHint: 'composer' | 'message-action'
}
export function createScheduleFromChat(input: ScheduleFromChatInput): Promise<{ id: string }> {
  return createCronSchedule(input.orgId, {
    name: input.name,
    schedule_expr: input.scheduleExpr,
    timezone: input.timezone,
    task_template: {
      kind: 'cron',
      title: input.name,
      description: input.goalText,   // taskGoal() renders title + "\n\n" + description
      // workflow_type omitted deliberately — DefaultWorkflowType is InteractiveRunSupervision,
      // the only type ScheduledRunSupervision's Dispatch guard accepts (workflow_dispatcher.go:228-236).
    },
    enabled: true,
    // space_ref omitted — resolves to the caller's personal Space (spaces.rs::inject_personal_schedule_create_context).
  })
}
export function listMySchedules(orgId: string, myUserId: string): Promise<CronSchedule[]> {
  return listCronSchedules(orgId).then((r) =>
    r.schedules.filter((s) => s.creator_subject_id === myUserId),
  )
}
```
No new cache/state layer: reuse `createResource` the same way `CronSchedulesSection.tsx` does (:154-157); no need to know the personal Space ref client-side since filtering is by `creator_subject_id`.

**New component: `src/features/chat/components/ScheduleDialog.tsx`.**
- Props: `{ open, onClose, seedTitle: string, seedGoal: string, onCreated?: (id: string) => void }`.
- Fields:
  - Navn (`VerevonInput`, prefilled from `seedTitle`, editable).
  - Gjentakelse — a preset `<select>` (Norwegian labels) mapping to cron expressions, e.g.:
    - "Hver dag kl. 09:00" → `0 9 * * *`
    - "Hver ukedag kl. 08:00" → `0 8 * * 1-5`
    - "Hver mandag kl. 09:00" → `0 9 * * 1`
    - "Den 1. i hver måned kl. 09:00" → `0 9 1 * *`
    - "Egendefinert (cron-uttrykk)" → reveals the raw `VerevonInput` `CronSchedulesSection.tsx` already uses (:404-419), for power users.
  - Tidssone: fixed text "Europe/Oslo" with a small "Endre" (advanced) toggle that reveals the free-text field — **default changes from `'UTC'` to `'Europe/Oslo'`** relative to the Settings form.
  - Hva kjøringen kan gjøre: two radio options —
    - "Kun utkast (standard)" — default, maps to `task_template.policy` set to whatever value execution-core's step loop already treats as read-only/no-effectful-tools (verify the exact policy string in `execution-core`'s policy config before wiring; do not invent one — see Owner decision #2 and Risk in §5).
    - "Tillat handlinger, med godkjenning" — disabled control (grayed, with a tooltip explaining scheduled runs can't yet pause for approval — see §1's `ScheduledRunSupervision` gap) until the backend work in Owner decision #2 lands. Shipping it disabled-but-visible documents the gap instead of hiding it.
  - Read-only preview line: "Kjører i din personlige arbeidsflate" (info, not a picker — matches today's single-space resolution).
  - Submit → `createScheduleFromChat`; on success, close + toast "Planen er opprettet" + call `onCreated`.
  - Validation mirrors `CronSchedulesSection.handleCreate` (:184-248): non-empty name/expr; no JSON workflow-input validation needed since this surface never sends one.

**Composer entry point — `DashboardComposer.tsx`:**
- Extend the `builtin` union (:214): `"deep_research" | "file" | "image" | "web_search" | "schedule"`.
- Add to `BUILTIN_COMMANDS` (:394-451):
  ```ts
  {
    aliases: ['planlegg', 'gjenta', 'recurring'],
    argument: { hint: { no: 'hva som skal gjentas (valgfritt — bruker utkastet)', en: 'what to repeat (optional — uses the draft)' }, required: false },
    builtin: 'schedule',
    description: { no: 'Gjør denne forespørselen om til en planlagt kjøring.', en: 'Turn this request into a recurring job.' },
    icon: CalendarClock,
    id: 'builtin:schedule',
    label: 'Schedule',
    name: 'schedule',
  },
  ```
- In `runCommand()` (:1312-1337), add:
  ```ts
  if (command.builtin === 'schedule') {
    setScheduleDialogSeed({ title: draftTitleFrom(props.message), goal: props.message })
    setScheduleDialogOpen(true)
    return
  }
  ```
  (`draftTitleFrom` = first ~40 chars of the draft, stripped of markdown — reuse whatever helper `chat-export`'s title derivation already uses, don't write a second one).

**Message action — `ChatMessages.tsx`:**
- Add prop `onScheduleThis?: () => void` next to `onRerunAsNewTurn` (:142, :360), rendered as a new `MessageMenu` item in the overflow menu (:738-757), right after "Fortsett i ny chat":
  ```tsx
  { label: i18n.tr('Planlegg dette', 'Schedule this'), icon: <CalendarClock size={16} />, onClick: () => props.onScheduleThis?.() },
  ```
- **`ChatPage.tsx`**: thread `onScheduleThis={() => openScheduleDialogForTurn(row.turn.id)}` next to `onRerunAsNewTurn={() => void rerunAsNewTurn(row.turn.id)}` (:783). Implement `openScheduleDialogForTurn` in (or alongside) `use-chat-controller.ts` by reusing whatever `rerunAsNewTurn` already does to resolve `turn.id` → the original user-turn text (don't re-derive it a second way); pipe that text into `ScheduleDialog`'s `seedGoal`.

**Listing/pause/delete from chat** — new lightweight panel, e.g. `src/features/chat/components/ScheduledRunsPanel.tsx`, opened from a header icon or a sidebar section "Planlagte kjøringer":
- `createResource(() => orgId(), (id) => listMySchedules(id, session.user!.id))`.
- Per row: name, cadence (a small `cronExprToLabel()` helper that reverse-maps the known presets and falls back to showing the raw expression — do not port `capability-core/internal/cron/cron.go`'s parser to JS; that's a duplication/drift risk item this codebase's own comments elsewhere warn against), next/last fire (`formatWhen`, reuse `CronSchedulesSection.tsx:26-33`), pause/resume via `updateCronSchedule(orgId, id, { enabled: !enabled })`, delete via `deleteCronSchedule` with the same `window.confirm` pattern (:270-275).
- "Se resultater" per schedule — **Phase B feature**; render disabled with "Kommer snart" until Phase B ships (see below), or once shipped, deep-link to `/spaces/{personal_space_ref}/work?schedule={id}` (reusing `SpaceWorkPanel.tsx`) rather than building a second transcript renderer inside `/chat`.

### 2.2 Gateway BFF (`apps/gateway/src`)

**Relax the admin-only gate for the caller's own personal-Space schedules** (`agent_actions.rs`):
```rust
async fn can_author_own_schedule(user: &AuthenticatedUser, schedule_creator_subject_id: Option<&str>) -> bool {
    has_authorized_org_role(user, &["owner", "admin"])
        || schedule_creator_subject_id.is_some_and(|id| id == user.user_id)
}
```
- `create_cron` (:422-488): today's `can_author_skills` gate blocks a non-admin from ever creating one — **relax to**: always allow when the resolved Space is the caller's own personal Space (this is exactly what `inject_personal_schedule_create_context` already verifies before minting the Control decision, so this is not weakening authorization, just removing a redundant, over-broad BFF-side pre-check for the personal-Space path). Keep the admin gate for an *explicit* non-personal `space_ref` (Space Room schedules stay admin-gated, unchanged).
- `update_cron`/`delete_cron` (:491-569): today they only take `cron_id`, with no lookup of the schedule's `creator_subject_id`/`space_ref` before gating. Add a lookup (`GET {model_gateway}/v1/cron/{id}` first, cheap and already proxied) and apply the same `can_author_own_schedule` rule.
- This is the único required backend authorization change for Phase A; everything else in Phase A is pure frontend, riding on already-working endpoints.

**Phase B — new read endpoint**, e.g. `GET /api/v1/cron/{cron_id}/runs` in `agent_actions.rs`, gated the same way (own schedule or admin):
- Proxies to a new capability-core endpoint (below) returning `{ runs: [{ fire_id, fired_at, status, thread_id }] }`.
- The frontend calls this to know *whether* a viewable thread exists per fire, then hands `thread_id` + the schedule's `space_ref` to the existing `list_space_threads`/transcript-read machinery (§2.4) once the Control decision fix lands.

### 2.3 capability-core (Go)

**Migration `0016_cron_fire_thread_id.up/down.sql`** (next number after `0015_background_process_capability`):
```sql
ALTER TABLE cron_fires ADD COLUMN thread_id TEXT;
```
**`internal/taskexec/workflow_dispatcher.go`**: after a successful `PrepareScheduledRunThread` call in `reauthorizeScheduleFire` (:370-385), persist `response.GetThreadId()` onto the corresponding `cron_fires` row (keyed by `task_id`, which the dispatcher already has via `task.ID`):
```go
_, _ = d.pool.Exec(ctx, `UPDATE cron_fires SET thread_id=$1 WHERE task_id=$2`, response.GetThreadId(), task.ID)
```
(best-effort, matching the existing "losing the audit row is worth a loud log" posture at :251-256 — a missing `thread_id` should log, not fail the run).

**New handler** `internal/api/workplane_apis.go::(*CronHandler) listRuns` mounted at `GET /api/v1/cron/{id}/runs`:
```go
SELECT cf.id, cf.fired_at, cf.status, cf.thread_id
FROM cron_fires cf JOIN cron_schedules cs ON cs.id = cf.schedule_id
WHERE cf.schedule_id = $1 AND cs.org_id = $2 AND cs.deleted_at IS NULL
ORDER BY cf.fired_at DESC LIMIT 50
```
Returns `{ runs: [{ fire_id, fired_at, status, thread_id }] }`; `thread_id` is `null` for fires that predate the migration or failed before `PrepareScheduledRunThread` ran (surface those rows with `status: "failed"` and no link, not an error).

### 2.4 Control Plane (`user-core`) — the Phase B unblock

**`internal/spaces/personal_thread_decision.go`**: add a sibling validator that authorizes a personal Space's **own owner** to read their own space's threads — deliberately not relaxing `ValidateForSharedThreadRead` itself (that stays "non-personal only," protecting the Space-Room multi-member semantics it was built for):
```go
// ValidateForOwnScheduledThreadRead authorizes a personal Space's own owner to
// read the system-owned (schedule) threads inside their own Space. Unlike
// ValidateForSharedThreadRead this never crosses a membership boundary — the
// caller IS the Space's only member — so it has no audience-join requirement.
func (e PersonalThreadDecisionEvidence) ValidateForOwnScheduledThreadRead() error {
    if err := e.Membership.Validate(); err != nil { return err }
    if e.Membership.Kind != KindPersonal { return fmt.Errorf("this decision is for a personal Space only") }
    if !matchesOneOf(e.Membership.Role, "owner") { return fmt.Errorf("only the Space's own owner may read it") }
    return e.Privacy.Validate()
}
```
Add the matching `IssueOwnScheduledThreadReadDecision` (mirrors `IssueSharedThreadReadDecision`, :139-166, same action id `model.thread.read` / schema `sha256:thread-read-v1` so Session Core's existing verifier needs **no change** — it already just checks the signed decision, not which issuance path produced it) and wire a new HTTP route in `internal/http/spaces.go` (e.g. `POST /api/v1/internal/spaces/{space_ref}/own-thread-read-decision`), or — smaller diff — extend the existing `thread-read-decision` handler to try `ValidateForOwnScheduledThreadRead` when `ValidateForSharedThreadRead` fails specifically with the "personal Space" error and the caller is that Space's own owner.

**Gateway** (`spaces.rs::shared_thread_read_decision_for`, :1274-1330): when the initial call to `/thread-read-decision` returns 403/404 **and** the target `space_ref` is the caller's own personal Space, retry against the new own-personal-space path (or the extended same endpoint) before falling back to `Ok(None)`.

Once this lands, `list_space_threads` (already generic over any `space_ref`, §1) and `list_thread_messages` (already accepts `space_read_decision_ref/token`, §1) need **zero further changes** to serve a schedule's result thread — confirmed by re-reading their bodies: both already thread a decision through unchanged.

**BFF `history.rs`** (optional, only if the design later wants scheduled threads to *also* appear inline in the normal `/chat` sidebar rather than only in the Work panel): set `origin: "chat"` on the `CreateThreadRequest` inside `prepare_scheduled_run_thread` and add an `is_system_owned`/`readonly` marker so `chat_history_sessions` doesn't need to change its filter semantics. **Recommend deferring this** — reusing the existing Work-panel view (§2.1) is far less risky than teaching the primary `/chat` history path to render a foreign-owner, read-only thread type it was never built for; revisit only if user testing shows people don't find the Work-panel link.

## 3. Tests to add

Frontend (vitest, colocated `*.test.tsx`, following the repo's own naming, e.g. `DashboardComposer.slash-commands.test.tsx`):
- `DashboardComposer.schedule-command.test.tsx`: `/schedule` and `/planlegg` open the dialog seeded from the draft; argument text seeds the goal; command matches by alias.
- `ChatMessages.schedule-action.test.tsx`: "Planlegg dette" appears in the overflow menu, calls `onScheduleThis`; absent when the prop is undefined (mirrors the `onBranch`-only fallback at :902).
- `ScheduleDialog.test.tsx`: preset → cron-expr mapping is exact for all 4 presets; custom toggle reveals the raw field; submit calls `createScheduleFromChat` with `workflow_type` omitted and `timezone: 'Europe/Oslo'` by default; validation errors match `CronSchedulesSection`'s Norwegian copy style.
- `ScheduledRunsPanel.test.tsx`: filters `listCronSchedules` results to `creator_subject_id === session.user.id`; pause/resume and delete call the existing `cron-client.ts` functions; "Se resultater" is disabled when `thread_id` is null for every run.
- `schedule-from-chat.test.ts`: `createScheduleFromChat` shapes `task_template` correctly (title/description, no `workflow_type`, no `space_ref`).

Gateway (Rust, `#[tokio::test]`, following the `wiremock` fixture style already in `spaces.rs`'s test module, e.g. around :5633-5680):
- `agent_actions.rs` tests: a non-admin caller creating a schedule with no `space_ref` succeeds (personal-Space path); a non-admin caller updating/deleting **someone else's** schedule is still 403; a non-admin caller updating/deleting **their own** schedule succeeds; an explicit non-personal `space_ref` still requires admin.
- New `list_cron_runs` proxy test (mirrors the existing `create_cron`/`update_cron` wiremock tests around :5124-5166, :6780-6860): request shape, 403 propagation, malformed upstream response.

capability-core (Go, table-style, following `sweeper_test.go`/`workflow_dispatcher_test.go` conventions):
- `internal/api/workplane_apis_test.go`: new `TestCronHandler_ListRuns` — org scoping, ordering, `thread_id NULL` rows.
- `internal/taskexec/workflow_dispatcher_test.go`: extend the existing scheduled-run test (uses `recordingScheduledRunAuthorizer`, :133) to assert `cron_fires.thread_id` gets written after a successful `PrepareScheduledRunThread`; assert it is *not* written (and the run still proceeds) when the update fails.
- **Regression test for the pre-existing bug**: `TestDispatch_ScheduledRun_RejectsNonDefaultWorkflowType` — a schedule template naming any workflow other than `InteractiveRunSupervision` must return the exact "requires %s, got %s" error today; this pins current (broken) behavior so a future fix to that bug is a deliberate, reviewed change, not an accidental one.

orchestrator-core (Go):
- `cmd/workflows/interactive_run_test.go`: if Owner decision #2 (§6) picks the "extend `ScheduledRunSupervision` to pause on approval" option, add a replay-safe test mirroring `InteractiveRunSupervision`'s existing approval-wait test, asserting the scheduled workflow now waits on `SignalApproval` instead of hard-failing, and that `handleFailure`'s old message path still fires on cancel-during-wait.

user-core (Go, Phase B):
- `internal/spaces/shared_thread_read_decision_test.go`-style new file `own_scheduled_thread_read_decision_test.go`: `ValidateForOwnScheduledThreadRead` accepts `KindPersonal` + `role: owner`; rejects `KindRoom`; rejects a non-owner role; rejects incomplete privacy evidence — mirroring the existing table tests for `ValidateForSharedThreadRead`.

session-core: no source change in Phase B beyond what's already generic (§2.4), so no new tests are strictly required there; add one integration test asserting a decision issued via the new user-core path is accepted by `verify_scheduled_run_decision`'s existing verifier code path (same action id/schema), to pin that assumption.

## 4. Rollout

- **Rebuild required**: frontend (verevonv3, Vite dev container — per the plan's Wave-1 note, a source edit reaches the running app only after a container restart), gateway BFF (Rust), model-gateway (no code change unless the `history.rs` origin change from §2.4 is taken — then also none, since that's gateway/session-core only), capability-core (Go — migration + handler + dispatcher change), orchestrator-core (Go — only if Owner decision #2 picks the pause/resume extension), session-core (no code change), user-core (Go — Phase B decision path).
- **Migration order**: run `0016_cron_fire_thread_id.up.sql` before deploying the new capability-core binary (the dispatcher write and the new `listRuns` handler both assume the column exists); it's additive/nullable so it's safe to run ahead of the code that uses it.
- **No feature flag strictly required** for Phase A: it's additive UI plus a narrowing-not-widening authorization change (a user could already create a schedule for their own personal Space by knowing the raw API; this just exposes it). Recommend gating the new UI entry points (composer command, message action, panel) behind the same environment/rollout mechanism Wave 1 used, if any exists, purely for staged exposure — no functional flag is needed in the backend.
- **Phase B is the one piece that should be flag-gated or held back independently**: shipping the "Se resultater" link before the user-core decision path exists means every click 403s. Ship Phase A fully, ship the "Se resultater" affordance disabled/"Coming soon" until Phase B's migration + user-core deploy + gateway change are all live together (they must deploy as one unit — the gateway's retry-on-403 logic in `shared_thread_read_decision_for` depends on the new user-core route existing).
- Per the launcher note in memory: build/deploy through the project's launcher (`--from model` style rebuild), not hand-rolled `docker compose build`, for the Model Plane services touched here.

## 5. Risks and edge cases

- **The pre-existing workflow-type bug (§1)** means this feature must *never* let the chat surface set `workflow_type` to anything but the default — a future "let me pick DeepTaskWorkflow for my scheduled run" ask would currently fail every fire. Flag this to whoever owns capability-core/orchestrator-core; it's out of scope to fix here but directly bounds this feature's design (already reflected in §2.1: no workflow picker in `ScheduleDialog`).
- **No approval-pause path for scheduled runs (§1)**: until `ScheduledRunSupervision` is extended (or the default policy is verified to never emit an effectful, approval-needing tool call), any schedule that reaches a `NeedsApproval` step **fails the entire run silently from the user's point of view** (the failure lands in `cron_fires.status='failed'`/`task_events`, not anywhere the chat UI surfaces today). The "kun utkast (standard)" policy default in §2.1 is there specifically to keep this from happening in the common case; ship the "tillat handlinger" option disabled until this is fixed, not silently allowed-but-broken.
- **ZDR orgs**: `ScheduledRunSupervision` is `DeniesZDR: true` at the orchestrator allowlist level — a ZDR org's "Schedule this" attempts will be refused at fire time (schedule created fine, then disabled by the sweeper's `AuthorizeFire` failure path, `sweeper.go:149-159`). The dialog should proactively check the org's ZDR posture (same signal `CronSchedulesSection.tsx` shows via `selectedWorkflow().deniesZdr` for other workflows, :482-487) and warn or block creation up front rather than let the user discover a silently-disabling schedule.
- **`session.user.id` vs `creator_subject_id` identity match**: the `listMySchedules` filter in §2.1 assumes these are the same identifier space; verify this against `AuthenticatedUser.user_id` (`agent_actions.rs`) before relying on it — if they diverge (e.g. one is an internal id, the other an external auth-provider id), the panel will silently show zero schedules to everyone.
- **Idempotent-create races**: `create_cron`'s `schedule_id`/`idempotency_key` generation (`agent_actions.rs:457-463`) uses a per-process atomic counter + nanosecond timestamp; this is fine for a single gateway instance but worth confirming behaves correctly (no collision) behind a multi-replica gateway deployment — not new to this feature, but this feature is what turns "cron create" from an admin-rare action into a common one, raising the collision-probability floor.
- **Cadence-preset ↔ cron-expression drift**: if `capability-core/internal/cron/cron.go`'s accepted grammar ever changes (e.g. adds seconds or `L`/`W` tokens), the frontend's hardcoded preset strings and the `cronExprToLabel()` reverse-mapper (§2.1) must be updated in lockstep — there is no shared source of truth between Go and TypeScript for this grammar today.
- **Personal-Space Work panel UX for a first-time visitor**: `SpaceWorkPanel.tsx` was built for a Space Room's shared record; verify it reads sensibly with exactly one member (the owner) and system-owned rows only, before linking to it from chat — this needs a quick manual look, not assumed from the code alone.
- **Deleting a schedule vs. its already-fired threads**: `delete_cron` soft-deletes the `cron_schedules` row (`deleted_at`); `taskexec/workflow_dispatcher.go::loadTask` (:299-325) already refuses to dispatch a task whose schedule was deleted after the task was claimed but before Temporal start — confirm the equivalent applies for a fire that's already `running` when the user pauses/deletes from chat (i.e., pause/delete does not cancel an in-flight scheduled run; that's a separate "cancel" action this spec does not add — call this out to the user in copy, e.g. a note under Pause: "Stopper fremtidige kjøringer; en kjøring som allerede er i gang fullføres").

## 6. Genuine owner decisions

1. **Who may create a personal-Space schedule from chat?** §2.2 proposes relaxing the admin gate to "admin OR own personal schedule." Confirm this is the intended posture — it's a real authorization loosening (today, only admins can create *any* schedule, personal or not) even though it's scoped tightly.
2. **How should "approvals for effectful tools" actually work for a scheduled run**, given `ScheduledRunSupervision` currently hard-fails on `NeedsApproval` (§1, §5)? Two real options with different sizes: (a) extend the workflow to pause on `SignalApproval` like `InteractiveRunSupervision`, and have the chat UI show a pending-approval affordance on the schedule's result thread (M-to-L: touches orchestrator-core, needs a notification path since the user isn't watching live, and needs the Phase-B thread-visibility work to even show the prompt) — or (b) keep scheduled runs permanently draft-only/read-only-tools for the foreseeable future and treat "approvals for effectful tools" as explicitly Wave-3 (S: just disable the option, as §2.1 already does by default). This changes the design materially — recommend picking (b) for Wave 2 and scoping (a) as its own Wave-3 item.
3. **Notification on completion/failure**: today nothing pings the user when a scheduled run finishes or fails (per the plan's own Wave-2 backlog: "background task with completion notice" is a separate T1 item, gap matrix row "Background task with completion notice"). Without it, "where results land" is a place the user must remember to go check. Decide whether Wave 2 ships without any notification (result sits in the Work panel until visited) or pulls a minimal notification in as a dependency (see §7).
4. **Timezone default**: confirm `Europe/Oslo` as the fixed default (per the item's own wording) vs. detecting the browser's timezone — Norwegian-first suggests the former, but worth an explicit yes.
5. **Cadence preset set**: the four presets proposed in §2.1 are a reasonable default set (daily/weekday/weekly/monthly), not derived from any existing spec — confirm against what Manus/Perplexity/ChatGPT's own preset sets look like if parity with those specifically matters, or approve as-is.

## 7. Size and dependencies

- **Size: M overall.** Frontend work (§2.1) is S on its own. The gateway authorization relaxation (§2.2) is S. Phase B (§2.3 migration + handler + dispatcher write, §2.4 user-core decision path + gateway retry) is the M-to-L core of the item — it is Control-Plane + two Model-Plane services, not a pure frontend wiring task, which is why the gap matrix's "M" estimate (row: `"Schedule this" from chat`) undersells it once the personal-Space decision gap is accounted for.
- **Dependencies on other Wave-2 items**: none of Wave 2's listed items block this one structurally, but:
  - the **editable-plan gate** and **subagent-ceiling** items are unrelated and independent;
  - **"forward an email to create a task"** shares the same `tasks`/`config_json` dispatch machinery (`workflow_dispatcher.go`) and would hit the *same* workflow-type bug (§1) if it ever lets a caller name a non-default workflow — worth flagging to whoever builds that item so the bug gets fixed once, not twice;
  - **background-task completion notice** (Wave 3 per the plan, but see Owner decision #3) is a soft dependency: without it, Phase B's "see results" is pull-only, which is a materially worse UX than the item's "posts back into a thread" framing implies. Recommend either pulling a minimal notification (e.g. a badge count on the new panel driven by polling `GET /api/v1/cron/{id}/runs`, no new backend push mechanism) into this item's Wave-2 scope, or explicitly documenting the pull-only limitation to the owner up front.