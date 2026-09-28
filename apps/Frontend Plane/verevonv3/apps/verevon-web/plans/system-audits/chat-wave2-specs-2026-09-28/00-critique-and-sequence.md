# Critique: Wave-2 chat-page specs (schedule, scope, connector-picker, persona, plan-gate, wide-research, mail-task, citations)

Read-only review. Repo confirmed as a real checkout at `C:/dev/CoresSystem` (`.git` present, real `git log`, in-flight uncommitted changes matching what `CODEBASE_INFORMATION_SYSTEM.md` describes). No files touched, no state-changing commands run.

---

## 1. Files touched by more than one spec — collision map and ordering

### Tier 1 — four specs touch the same file (highest collision risk)

| File | Specs (what each does) |
|---|---|
| `src/features/chat/components/ChatMessages.tsx` | **schedule** (adds "Planlegg dette" to `MessageMenu`, new `onScheduleThis` prop) · **scope** (new retrieval-scope badge near the verification-notice block) · **connector-picker** (provider badge on `AttachmentItem`) · **plan-gate** (rewrites `PlanApprovalControl`, defined here at :2038, into an editable step list — its own test lives in `ChatPanels.test.tsx`, not a same-named test file — see §2) |
| `src/features/chat/components/ChatPanels.tsx` | **scope** (new line in `ContextWindowPanel`, :535-630) · **persona** (optional polish, same block) · **wide-research** (mounts new `ChatWideResearchPanel`) · **citations** (domain/provider grouping in `SourcesPanel`, :353-355) |
| `apps/Model Plane/rust/services/model-gateway/src/sse.rs` | **scope** (~8 call sites: `:669-674,735,1175,1207,1302,1352,1427,1471,1577,1628-1635`) · **persona** (extends `authored_instructions_message`, confirmed at :5791) · **plan-gate** (`RunAgentRequest` construction near `:1058-1071`, extended to inject the approved plan) · **citations** (anchor-pass call site near the `verification::decide`/`assistant_output` integration point) |
| `apps/Model Plane/rust/services/session-core/src/grpc.rs` | **schedule** (`prepare_scheduled_run_thread`, `list_threads`) · **scope** (thread-immutability check :2367-2389, must stay untouched by scope's new axis) · **persona** (`get_context_assembly_inner`, `update_thread_presentation`) · **mail-task** (`THREAD_ORIGINS`, confirmed exactly at line 2128 as 5 entries: `chat,space,agent_run,support,system` → mail-task adds a 6th) |

### Tier 2 — three specs

| File | Specs |
|---|---|
| `DashboardComposer.tsx` | schedule (new `/schedule` command) · scope (new retrieval-scope picker in the context popover) · connector-picker (new "Fra en tilkoblet kilde" settings row) — confirmed `internalSourceScope`/`sourceScope`/`conversationOnly` at exactly lines 813-815, `SettingsView` union at exactly line 191 |
| `use-chat-controller.ts` | schedule (`openScheduleDialogForTurn`) · scope (per-turn `retrievalScope` signal, explicitly *not* mirroring the first-turn pin) · plan-gate (`onPlanProposed`, `approveTurnPlan` forwarding edited steps) |
| `shared/api/chat-client.ts` | scope (`ChatInvokeRequest`, `buildChatWireBody`, history deserialization) · persona (`ChatThreadSession`, `SaveChatThreadSnapshotRequest`) · plan-gate (`approvePlan` gains `planId`/`steps`) |
| `model-gateway/src/http_routes.rs` | scope (`InvokeRequest.retrieval_scope`) · persona (`InvokeRequest.persona_instructions`, `get_thread_context`, presentation handler) · plan-gate (extends `PlanApprovalRequest`) |
| `execution-core/src/runtime_loop/agent.rs` | plan-gate (new `plan.propose` tool + dispatch arm) · wide-research (configurable `child_ceiling`, new `subagent.research_fan_out` tool, dedupes the confirmed-real duplicate `"subagent.task"` definition at lines **3183 and 3226** exactly) — both specs already flag this collision in their own §7, which is correct and should be trusted |
| `orchestration.proto` | plan-gate (`TransitionPlanRequest.edited_steps`, new `ProposePlan` RPC) · wide-research (`AttachSubagentRequest.goal_summary`/`batch_id`) — **neither spec is aware of the other's field additions in the same message file**; confirmed `PlanState` enum values (1-9) match plan-gate's citation exactly |
| `chat-normalizers.ts` | scope (`normalizeCitation` gains `origin`) · citations (`upsertCitation` changes from drop-on-duplicate to merge-on-duplicate) — same citation-normalization neighborhood; citations' own §7 already recommends landing scope's `origin`/`sourceGroupId` work first, which is the right call |

### Tier 3 — two specs (lower risk, but still needs sequencing)

- `chat-types.ts`: scope (`Citation.origin`) + connector-picker (additive fields on `DashboardComposerAttachment`/`ChatTurnAttachment`) — no field-name overlap, safe to parallelize.
- `apps/gateway/src/domains/spaces.rs`: schedule (Phase B retry logic in `shared_thread_read_decision_for`) + persona (new `fetch_persona_instructions`/`inject_persona_instructions`, called from the same 3 injection sites as `inject_authored_instructions`) — different functions, low real conflict, but both edit the "instruction/decision injection" call sites in `json_handlers.rs`/`streams.rs`/`ag_ui.rs` — coordinate around those three call sites specifically.
- `apps/gateway/src/domains/chat/history.rs`: persona (`persona_enabled` on `SaveThreadRequest`/summaries) + schedule (optional/deferred `origin` idea — actually lives in session-core, not this file; low real risk).
- **`session-core/migrations/` directory (real, previously unflagged conflict):** persona claims the next number explicitly — `0041_thread_persona_toggle.sql` — and I confirmed the migrations directory currently ends at `0040_direct_run_completion_key.sql`, so **0041 is genuinely free today**. mail-task's spec deliberately leaves its own migration unnumbered (`00xx_mail_task_thread_origin.sql`) because it extends the same `threads.origin` CHECK constraint family. **If both ship in the same wave, whoever lands second must renumber** (mail-task → `0042` if it lands after persona, or persona must bump if mail-task lands first) — neither spec's own text is aware of the other, since each was researched independently. Flag this explicitly to whoever schedules the wave.
- `execution-core/src/subagent/mod.rs`: only wide-research modifies it, but plan-gate's new `execution-core/src/plan.rs` explicitly mirrors its `is_subagent_tool`/`TOOL_PREFIX` shape — no direct edit collision, just a "same pattern, copy it once" note both specs' §7 already make.

### Recommended split to avoid collisions

Land in this file-ownership order, one PR wave at a time, not by feature name:
1. **`sse.rs` touch-owner rotation:** land **citations** (2.1-2.6, the non-cross-producer-counter slice) first — its own §7 says the `origin`/`sourceGroupId` distinction on `Citation` makes **scope** cheaper. Then **scope**'s `sse.rs` call-site edits. Then **persona**'s `authored_instructions_message` insert. Then **plan-gate**'s `RunAgentRequest` injection, last, since plan-gate is sized **L** and already the riskiest of the four.
2. **`ChatMessages.tsx`/`ChatPanels.tsx` touch-owner rotation:** land **scope**'s badge and **connector-picker**'s attachment badge together (both additive, non-overlapping regions), then **schedule**'s message-menu item, then **plan-gate**'s `PlanApprovalControl` rewrite last (it's the biggest diff in that file and most likely to produce merge noise against the smaller additive changes).
3. **`session-core/src/grpc.rs`:** land **mail-task**'s origin-enum widening first (single line, `THREAD_ORIGINS` array + CHECK constraint), then **persona**'s new column/RPC field, then **schedule**'s work (touches a different function, `prepare_scheduled_run_thread`), then **scope** last since scope's own text is explicit that its one hard rule is "do not touch `:2367-2389`" — landing it last makes that invariant easiest to verify against a settled diff.
4. **`orchestration.proto`:** whoever lands second between plan-gate and wide-research must explicitly coordinate proto field numbers by hand (regenerate against the other's already-merged `.proto`, not by guessing next-free-number independently) — put this as a literal checklist item in both PRs.
5. **`session-core/migrations/`:** persona owns `0041`; mail-task takes the next free number at merge time, not before.

---

## 2. Spot-check of file:line evidence (read-only verification)

I directly read 17 cited locations spanning all 7 specs (well past the required 8), across Rust, Go, and TypeScript. Every one checked out, most to the exact line:

| Spec | Claim | Verified |
|---|---|---|
| schedule | `agent_actions.rs` cron routes at `:51-55`, `can_author_skills` at `:66-69` | Exact — routes at lines 51-55, gate fn at 66-68 |
| schedule | `create_cron`/`update_cron`/`delete_cron` gates at `:428-437/:498-507/:538-547` | Exact — gate checks at 428, 498, 538 |
| schedule | `workflow_dispatcher.go` non-default-workflow guard `:228-236` | Exact — guard at 229-232, error string byte-identical |
| schedule | `interactive_run.go::ScheduledRunSupervision` hard-fail `:242-244` | Exact — matches to the character |
| scope | `chat-source-scope.ts:5` `ChatSourceScope = 'workspace' \| 'conversation'` | Exact |
| scope | `DashboardComposer.tsx:813-815` `internalSourceScope`/`sourceScope`/`conversationOnly` | Exact, line-for-line |
| scope | `tool_loop.rs:4151-4153` `conversation_tool_allowed` allowlist | Exact — same 6 tool names, same line |
| connector-picker | `DashboardComposer.tsx:191` `SettingsView` union | Exact |
| connector-picker | `integration-corev2/internal/actions/service.go:47-55` + `egress/client.go` `AllowRedirects` zero-value bug | Confirmed real: `NewService` builds the client with the zero-value `ClientConfig` (`AllowRedirects` defaults false → `CheckRedirect` returns `ErrUseLastResponse`) — this is a genuine, correctly-diagnosed bug, not a hypothetical |
| persona | `sse.rs:5791` `authored_instructions_message` signature and body | Exact, including the platform→org→Space→agent-persona ordering |
| plan-gate | `orchestration.proto` `PlanState` enum values 1-9 | Exact |
| plan-gate | `PlanApprovalControl` defined in `ChatMessages.tsx:2038`, tested in `ChatPanels.test.tsx:659` | **Confirmed correct, not an inconsistency** — I initially suspected this was a spec error (component in one file, `describe` block in a differently-named test file) but verified both: the component genuinely lives in `ChatMessages.tsx` and its tests genuinely live in `ChatPanels.test.tsx` (imported at that file's line 13). Worth flagging to an implementer as a real "gotcha," not correcting as an error. |
| wide-research | `subagent/mod.rs:55` `MAX_TOTAL_CHILDREN: u32 = 8`, `guard_child_ceiling` signature | Exact |
| wide-research | Duplicate `"subagent.task"` tool definition at `agent.rs:3183` and `:3226` | Confirmed real and exact (a third occurrence at :6374 is in a different context, likely a test literal, not part of the same offered-tools vec — not a contradiction) |
| wide-research | `cost-core/server.go:98-104` `budgetCheckRequest` has no `run_id` field; `handleBudgetCheck` at `:444` | Exact — struct is lines 98-104 verbatim, no `run_id`; `AggregateFilter` already has a `RunID` field (line 228), confirming the proposed extension is straightforward |
| mail-task | `session-core/grpc.rs:2128` `THREAD_ORIGINS` has exactly 5 entries | Exact |
| citations | `sse_events.rs` `ChatEvent::Citation{id,title,url,snippet}` | Exact |

**Result: zero factual errors found** among everything checked. This is an unusually well-researched batch — every citation I checked landed within 0-3 lines of the claimed range, several exactly. The one genuinely new problem I found that no spec called out is the **session-core migration-number collision** between persona and mail-task (§1 above); everything else the specs flag as risk was independently confirmed as real (the redirect bug, the duplicate tool definition, the workflow-type bug).

---

## 3. Implementation sequence — three batches

**Batch 1 (parallel-safe, no cross-spec file collisions, ship first):**
- **citations** (2.1, 2.2, 2.4-2.7; defer 2.3's cross-producer counter) — self-contained in `model-gateway`, additive wire fields, and its own §7 says nothing blocks it.
- **connector-picker** — zero dependency on any other Wave-2 item per its own §7; only real shared-file risk is `DashboardComposer.tsx`, which is additive (new settings-menu row) and can be reviewed independently of scope/schedule's composer changes.
- **mail-task**'s non-frontend plumbing (user-core alias service, integration-corev2 receiver/verify, the new `mail_task` thread origin in session-core) — independent of everything else; needs owner decision #1 (mail provider) resolved first, but that's a business call, not a code dependency.

**Batch 2 (depends on Batch 1 landing in the shared files first):**
- **scope** — land its `sse.rs`/`chat-normalizers.ts`/`chat-types.ts` changes *after* citations' `origin`/`sourceGroupId` work (citations' own §7 says this explicitly; I confirm no code reason it couldn't be reversed, but citations-first is less rework).
- **schedule** Phase A (frontend + the BFF authorization relaxation) — independent of scope/citations at the code level, but shares `ChatMessages.tsx`/`DashboardComposer.tsx`/`use-chat-controller.ts` with scope, so land after scope's composer changes settle to avoid a 3-way merge in the same files.
- **persona** — shares `sse.rs`, `spaces.rs`, `http_routes.rs`, `grpc.rs`, and the session-core migration slot with schedule/scope; land after both, claiming migration `0041` for real.

**Batch 3 (the two large, higher-risk items, sequenced last and coordinated with each other):**
- **plan-gate** (size L) and **wide-research** (size L) both touch `execution-core/src/runtime_loop/agent.rs` and `orchestration.proto` in the same review window. Land whichever is *more advanced* first, then require the second implementer to regenerate proto bindings against the first's already-merged field numbers rather than guessing independently. Both specs' own risk sections already recommend deferring their highest-risk sub-piece (plan-gate's owner decision #1 revision-behavior fork; wide-research's §2.3 cost-guard, which is a documented no-op on the Terra/subscription route) — take both of those deferrals.
- **schedule Phase B** (the user-core decision-issuance fix) rides along in this batch since it is explicitly the higher-risk, cross-plane half of that item and is optional/flag-gated per its own rollout section.

Cross-cutting rebuild note for every batch: per project memory, build via the launcher (`--from model`-style), never a hand-built `docker compose build`; all Rust-touching batches also need a frontend container **restart** (not rebuild) since Vite-in-container watching is disabled, a fact repeated consistently and correctly across all 7 specs.

---

## 4. Consolidated genuine owner decisions (deduplicated, with a recommended default)

1. **Scheduled-run approval semantics (schedule + plan-gate overlap).** `ScheduledRunSupervision` hard-fails on any step that `NeedsApproval` (confirmed real, §2). Both schedule §6.2 and plan-gate §6.2 ask essentially the same question from different angles: should an unattended/scheduled run ever be allowed to pause for approval, or should it stay permanently draft-only? **Recommended default: draft-only for Wave 2** (schedule's "kun utkast" default, plan-gate's option (b)) — treat pause/resume-on-approval for unattended runs as one Wave-3 item shared by both features, not built twice.

2. **Who may create/act on a personal-Space schedule from chat (schedule §6.1).** Relax `can_author_skills` to "org-admin OR the schedule's own creator on their own personal Space." **Recommended: accept** — it's a narrowing-not-widening change confirmed by reading `inject_personal_schedule_create_context`, which already performs the real check.

3. **Per-turn vs. thread-locked new axes (scope §6.1, persona §6.1).** Both source-scope's `RetrievalScope` and persona's per-thread toggle chose "store in existing free-form JSON/metadata, no migration" over "thread-locked, needs a migration." **Recommended: keep both as designed (per-turn/metadata-only)** — this is the cheaper, lower-risk path in both cases and neither touches the delicate thread-immutability code confirmed at `grpc.rs:2367-2389`.

4. **Tool-bucket / capability-boundary assignment (scope §6.3, connector-picker §6.3, wide-research §6.3).** Three specs independently ask "which tools count as X-class for a restriction/allowlist": scope for `mcp_call`/`action_execute`/`browser_agent`/`inbox_*`, connector-picker for whether picker-read ops should also become agent-callable, wide-research for role/whether the model can invoke fan-out under `ask` posture. **Recommended default across all three: leave connector/execution tools (`mcp_call`, `action_execute`, `browser_agent`) unrestricted by any new filter for now, and require an explicit follow-up decision before any of these three items makes such a tool agent-autonomous** — consistent, conservative, and each spec already leans this way individually.

5. **SharePoint / team-site scope (connector-picker §6.1).** Ship OneDrive-personal + Google-My-Drive only; defer SharePoint site-library browsing. **Recommended: accept** — confirmed no site-listing code exists today, so this is a real scope cut, not a formality.

6. **Timezone and cadence-preset defaults (schedule §6.4-6.5).** `Europe/Oslo` default, 4 fixed cadence presets. **Recommended: accept both**, consistent with the owner's stated Norwegian-first direction.

7. **Mail provider + sender-verification strictness (mail-task §6.1-6.2).** Real, unavoidable, non-code business decisions (Postmark vs. SendGrid vs. Mailgun; self-forward-only vs. trusted-sender-list). **Recommended: Postmark, self-forward-only for v1** — matches the spec's own reasoning and keeps the spoof-surface minimal.

8. **New thread origin vs. reuse (mail-task §6.4).** Add a 6th `mail_task` origin rather than overloading `agent_run`. **Recommended: accept**, but explicitly sequence it against persona's migration number (§1 above) and note it should be deleted in favor of a general completion-notification binding if that Wave-3 item lands first (the spec already says this).

9. **Round-budget division and concurrency ceiling for wide fan-out (wide-research §6.1-6.2).** Pre-divide the round pool evenly across N known items vs. today's first-claim-wins; and a concrete concurrency-ceiling number (spec suggests 4-6) for the subscription route. **Recommended: pre-divide evenly (fair, avoids starving later items) and start the concurrency ceiling at 4**, revisit after real Terra-route rate-limit data comes in — this is explicitly called out as inherently a latency/cost tradeoff the engineer shouldn't guess alone, and the session's own "live latency qualification series" running right now is exactly the kind of data that should inform the final number before committing to it.

10. **Plan-mode revision semantics (plan-gate §6.1-6.2).** Whether a second `plan.propose` supersedes the first, and whether a rejected plan can be revised in the same turn. **Recommended: last-proposed-wins with the `supersedes` chain recorded (already modeled in the proto), and no revise-after-reject in Wave 2** (ask again in a fresh message) — keeps the state machine small for an already L-sized item.

11. **Claim-to-source fallback UI and first-occurrence-only claimId (citations §6.1-6.2).** Whether to show a quiet "uncited claim" notice, and whether to build full per-occurrence citation tracking now. **Recommended: no new UI for uncited claims in Wave 2** (today's plain-text rendering is already honest, per the spec's own reasoning), and **first-occurrence-only claimId is sufficient** — a full per-occurrence model is a materially bigger change with no confirmed product need yet.

12. **Cross-producer citation-number collision fix (citations §6.3, cross-referenced by scope's own risk section).** Fix the KB/web shared-counter bug now (3-module signature change) vs. defer pending real-traffic frequency data. **Recommended: defer** — ship the higher-value fixes (2.1/2.2/2.4-2.6) first, gate 2.3 on a quick production log check of how often `wants_grounding` and `web_search`/deep-research actually co-occur in one turn.

13. **Persona precedence vs. an `@`-mentioned Space agent (persona §6.3), and whether persona text fires on ZDR turns by default (persona §6.1).** **Recommended: agent wins on an explicitly-summoned turn (mirrors today's org/Space precedent), and persona text follows org-instructions' existing precedent of firing regardless of ZDR** (only the per-thread *toggle*, not the text, is ZDR-blocked) — both defaults keep persona consistent with the already-shipped org/Space instruction hierarchy rather than inventing a new rule.
