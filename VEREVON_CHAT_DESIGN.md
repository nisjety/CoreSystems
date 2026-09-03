# Verevon Chat — Design Doc

**Status:** Proposed · **Date:** 2026-08-17 · **Scope:** `apps/Frontend Plane/verevonv3/src/features/chat` + gateway + Model Plane exposure

The chat page as the singular Verevon surface: a hybrid of ChatGPT/Claude
(recognition), Manus (long-running agent work), Perplexity (evidence), and
NotebookLM (grounded corpus) — built on the harness patterns from
`claude-hermes-deepseek.md`.

---

## 1. The principle

> **The chat page *is* Verevon** — the one agent with visibility into
> everything. It is not a page other surfaces redirect into when they need "an
> AI." Support tasks, Space room exchanges — none of that routes through it.
> Verevon *knowing about and being able to reach into* that state on request is
> different from *owning the UI* for it. Threads created outside this page must
> not be visible on it; they are reachable only when a user asks.

Three testable invariants follow. All three are **violated today** (§2.2).

| # | Invariant | Enforced by |
|---|---|---|
| **I1** | No surface navigates the user into `/chat` to "use AI" | Route audit + lint rule |
| **I2** | A thread created outside the chat page never appears in the chat thread list | A first-class `origin` dimension in session-core |
| **I3** | Verevon can *read* any org state the user may see, on request, without adopting it into chat's own history | Retrieval, not thread adoption |

---

## 2. Current state (verified 2026-08-17)

### 2.1 What is genuinely good

The chat page consumes the **full 13-variant `ChatEvent` SSE contract** and is
the only surface rendering reasoning, citations, artifacts, follow-ups and
per-turn usage. It already has: a 4-tab header (Chat / Kilder / Artefakter /
Steg), a persistently-mounted composer that survives tab switches (so
attachments and model choice aren't lost), a collapsible live-run split panel
with **honest screenshot states** (ready/pending/withheld-ZDR/unavailable/failed),
artifact rendering with a revision stepper and a deliberately sandboxed HTML
iframe, per-message copy/edit/branch/regenerate, HITL approval cards, and inline
per-turn cost. **Deep research is real and reachable** — the Telescope toggle
maps to a distinct `deep_research` wire field driving a genuine multi-round
pipeline that emits real citations and a report artifact.

That is a strong foundation. The gaps below are not "build a chat UI" — they
are "close the leaks and add the missing panel discipline."

### 2.2 The principle is violated in three concrete ways

**Leak 1 — thread separation is one predicate, and it doesn't cover the cases
that matter.** The only filter anywhere is server-side and only excludes
Space-scoped threads: `chat_history_sessions` keeps a session iff
`space_ref.trim().is_empty()` (`apps/gateway/src/domains/chat/history.rs:133`).
Consequently:
- **Agent-run threads leak.** `AgentRunConsole` calls `streamChat({...})` with no
  `threadId` and no `sessionKey` (`AgentRunConsole.tsx:705`), so `session_flow.rs:413`
  mints a durable thread with an empty `space_id` — which the chat listing includes.
- **Support-assist threads leak.** `runAssist` POSTs with a `support_*` thread id
  that doesn't exist in session-core (`inbox-ai.ts:295`), so `session_flow.rs:429`
  creates a real unscoped thread — included.
- **Space threads leak back in via deep link.** Space Activity renders
  `/chat?thread_id=…` (`activity-grammar.ts:163`); `initializeChat` trusts the deep
  link over the filtered listing (`use-chat-controller.ts:647`), the transcript
  route is intentionally unfiltered, and `loadThread` then writes a *local*
  history entry that is merged into the sidebar permanently.

**Leak 2 — seven kinds of caller route into `/chat`.** Legitimate: the sidebar's
"Ny samtale", navbar search over chat's own threads. **Violating I1:** Ticketing's
`askVerevon` → `navigate('/chat')` (`TicketingPage.tsx:564`), three "Åpne i Chat"
anchors in Support/Inbox, Space Activity's `/chat?thread_id=` links, and five
more `href: '/chat'` entries planted in `sidebar-navigation.ts` under Agents,
Knowledge, Ingestions, Messages and Overview.

The codebase already contains the correct ruling and contradicts itself:
`CoreSidebarSpacesPanel.tsx:205` states that routing a Space conversation to
`/chat` "was the exact separation-of-concerns leak the product model forbids"
and routes to `/spaces/:ref` instead — while `activity-grammar.ts:163` still
emits a `/chat` link for the same thread.

**Root cause:** separation is inferred from `space_ref` emptiness rather than
declared. Any surface that invokes without a scope mints a thread the chat
sidebar will show.

### 2.3 Wired-but-dead inventory (fix or delete)

| Thing | State |
|---|---|
| `Last-Event-Id` resume | Fully plumbed model-gateway → gateway → `readSseStream`; the SPA never passes it (`chat-client.ts:552`) and never records `event.id`. Resume replays text from seq 0 and **loses every tool/citation/usage/grounding event**. The single clearest dead path. |
| Sidebar pin | localStorage-only, and **silently destroyed** on resync: `withPinnedCarry` sees server `pinned:false` and deletes the local pin. A second, server-owned pin exists in the composer's History panel. The unit test omits `pinned` from its inputs, so it never exercises the real caller. |
| Plan view | `listPlans`/`listTodos`/`getLineage` exported with **zero callers app-wide**. A plan-mode run shows a one-line "Plan" string. The user cannot see, approve, or step through the plan. |
| Non-image attachments | Composer accepts `*/*` and renders chips for PDFs; `toStreamAttachments` drops everything non-`image/*` (`chat-normalizers.ts:382`). The fixing route (`POST /api/v1/chat/documents` → Data Plane ingest) had zero callers AND was itself dead — model-gateway forwarded it to a gRPC method Data Plane v2 disabled, so it answered 502. Both are fixed; see item 7 in the status table. |
| `@`-mention on chat page | Inserts styled text from people-search; never sets `mentionedAgentRef`. Agent invocation by mention works **only** in Space rooms. |
| `reason` response mode | "Deep" pushes a `reason` tool that `handleComposerSubmit` never maps to any wire field. Composer tone setting likewise never sent. |
| `?space_ref=` on `/chat` | Read by the controller; no link in `src/` produces it. |
| AG-UI | Gateway route + full typed client, zero importers. |
| Unproxied Model Plane | Realtime, video, translate, language, document-AI, `/v1/tasks`, `/v1/toon/encode` are live in model-gateway with no `/api/v1/...` equivalent. |

---

## 3. Design

### 3.1 Layout — three zones, one rule

```
┌──────────┬────────────────────────────┬──────────────────────┐
│ LEFT     │ CENTRE                     │ RIGHT (summoned)     │
│ Verevon  │ conversation               │ ┌ Work ┬ Output ┬    │
│ threads  │ minimal by default:        │ │ Sources ┬ Trace │  │
│ + live   │ collapsed thinking,        │ └──────────────────┘  │
│ status   │ tool cards, claim-bound    │                      │
│ chips    │ citations, per-msg actions │                      │
└──────────┴────────────────────────────┴──────────────────────┘
           [ Ask ⇄ Do ]  Grounded in: Space corpus (12) ▾   composer
```

**THE ONE RULE: the right panel is never opened by the product, only by the
work.** Ask mode with no retrieval opens nothing, and the surface is
indistinguishable from ChatGPT. First citation → Sources slides in. First
durable artifact → Output appears and takes focus. First effectful or
multi-step run → Work appears and takes focus. Run completes → Trace becomes
available but does **not** steal focus. Once summoned, a tab stays available
for that thread.

Auto-focus precedence is deterministic: **Work > Output > Sources > Trace**,
recency within a tab. Never open two panels. Never let a tab open itself twice.
The user must be able to predict, before pressing send, what will appear.

This gives a newcomer ChatGPT on minute one and Manus on day thirty, with no
settings change and no tour. It resolves the minimalism-vs-density conflict **in
time rather than in space** — a permanently half-dense UI is worse than either.

### 3.2 Ask / Do

A visible mode switch above the composer. **Ask is the default and Do is a
deliberate act.** A B2B tool that silently escalates a question into an
autonomous run has made the same category error as an approval-free write.

- **Ask** — read-only. Retrieval, reasoning, citations. Right panel appears only
  for Sources/Output. Full consumer message actions (copy/regenerate/edit/branch).
- **Do** — effectful. Requires an **autonomy budget declared up front**
  alongside the plan: steps, expected duration, tools it may touch, and which
  action classes will pause. Approved once at run start, not interrupt-by-interrupt.

Do-mode pauses are **typed**, never a generic "waiting for you":
`BLOCKED` (needs a credential), `APPROVAL` (needs a human decision, with the
exact payload shown), `AMBIGUOUS` (needs a judgement call). Each gets its own
affordance and notification.

### 3.3 Grounding scope is a governance control, not a preference

A persistent, always-visible composer control reading e.g. **"Grounded in: Space
corpus (12 sources)"**, with web as an explicit, additive, per-message opt-in
that visually marks resulting citations as external. Closed-world is the default
(NotebookLM's enforced posture, and the right one for a Norwegian tenant). It
must be **admin-constrainable per org**; an org that forbids web grounding sees
the control locked *with a reason*, not hidden.

### 3.4 Effectful turns are immutable

Message actions bifurcate by side-effect class:
- **Read-only turns** — copy, regenerate, edit-and-resend, branch.
- **Turns that invoked a provider action** — no regenerate, no edit. Instead:
  *View receipt*, *Re-run as new turn*, *Branch from before this*.

Letting a user regenerate an effectful turn after gating it behind a confirm
dialog is exactly the "HITL approval is decorative" failure this codebase's own
audit history has already found once.

### 3.5 No model picker

The composer exposes an **effort dial** (Quick / Standard / Deep) and nothing
else. Which model backs each tier is org policy set by the tenant admin, visible
on hover for transparency. No Norwegian quality manager has an opinion about a
model ID, and offering one invites a support ticket. This also makes the
existing pinned Budget/Balance/Genius grouping the *right* shape already — it
just needs to stop exposing the raw provider catalog underneath.

### 3.6 Thread ownership — overriding the research

The UI research recommended "threads belong to a Space, not a person," on the
grounds that all four reference products are single-user and would mislead a
multi-tenant product. **That recommendation is rejected here**, because it
contradicts the stated product model: Space conversations belong to Spaces and
are shown in Spaces.

**Resolution:** chat threads are **personal to the user within the org** —
Verevon is *your* assistant. Space room threads are Space-scoped and live in
`/spaces/:ref`. Both are multi-tenant-correct; they differ in owner, not in
rigour. This is recorded as a deliberate override rather than an oversight.

### 3.7 "Reach into, don't own" — how I3 actually works

When a user asks *"what happened with the Nordfjord ticket?"*, Verevon answers by
**retrieval over org state the user is permitted to see** — not by loading that
ticket's thread into chat history. The answer cites the ticket and links to
*its own surface* (`/support/...`), it does not adopt it.

Concretely: kill the deep-link adoption path (§4.1), and route cross-surface
questions through the existing retrieval/grounding machinery with the source
rendered as a citation whose click-target is the owning surface.

---

## 4. Plan

### 4.1 Phase 1 — enforce the principle (do this first; it is a correctness fix, not a feature)

1. **Add a first-class `origin` dimension** to session-core threads
   (`chat` | `space` | `agent_run` | `support` | `system`), set at creation by
   the invoking surface, defaulting to a value that is **excluded** from chat
   listing. Replace the `space_ref.is_empty()` predicate with `origin == 'chat'`.
   This closes all three leaks at the root rather than case-by-case.
2. **Stop the deep-link adoption.** `initializeChat` must consult the filtered
   listing; a `thread_id` whose origin isn't `chat` renders read-only with a
   "this conversation lives in <Space>" banner and **never** writes local history.
3. **Route audit + lint rule.** Remove the Ticketing/Support/Inbox/Space
   `/chat` navigations and the five `sidebar-navigation.ts` entries; add a lint
   rule forbidding new `/chat` hrefs outside the chat feature. Fix the
   `activity-grammar.ts:163` self-contradiction.
4. **Collapse the two pin systems** onto the server-owned one; delete the
   localStorage pin and fix the test that omits `pinned` from its inputs.

### 4.2 Phase 2 — make the existing surface honest

5. **Wire `Last-Event-Id`** — record `event.id`, pass it to `resumeStream`. This
   turns resume from "text-only replay from zero" into real resume.
6. **Delete or fix**: `reason` mode, chat-page `@`-mention, `?space_ref=`,
   AG-UI. Each is either wired up or removed — no third option.
7. **Non-image attachments**: wire the composer to the existing
   `POST /api/v1/chat/documents` → Data Plane ingest route, or stop accepting
   `*/*` in the file picker.

### 4.3 Phase 3 — the panel model

8. Build the **single four-tab right panel** with the deterministic auto-open
   rule (§3.1), migrating the existing Kilder/Artefakter/Steg tabs into it and
   folding `ChatLiveRunPanel` into **Work**.
9. Build the **Ask/Do switch** and the **grounding-scope control**.
10. **Plan view** in the Work tab, using the already-exported-and-uncalled
    `listPlans`/`listTodos`/`getLineage`. Render the plan as an editable
    artifact keyed to the thread — and echo the user-edited plan back into
    the model's context labelled as edited, so what the human signed is what
    the agent executes.
11. **Trace tab** — the run replay/audit record. Decide explicitly whether it
    is an audit artefact or a sharing feature; they need different permission
    models.

### 4.4 Phase 4 — long-running work

12. **Thread-list status chips** so the left rail doubles as Manus's job queue:
    a run in flight shows live status, closing the tab is safe, and the rail is
    where you come back to. This requires runs to be listed per thread (chat
    currently derives `liveRunId` from in-memory state only, so navigating away
    drops it).
13. **Deep research gets a Work-tab progress surface** — it is real today but
    invisible while it runs, with no plan/sub-query view and no way to leave and
    return.


> **Verified status, 2026-09-02** (checked against the code, not the plan text; re-created after the file was deleted from the repository root the same day):
>
> | # | Item | Status |
> |---|---|---|
> | 1 | Origin dimension on session-core threads | Done before the 2026-09 pass. Guard confirmed in `use-chat-controller.ts` (deep links fail closed to read-only for foreign origins). |
> | 2 | Stop deep-link adoption | Done before the 2026-09 pass; confirmed against code 2026-09-02. |
> | 3 | Route audit + lint rule | **Done.** `chat-route-ownership.test.ts` forbids `/chat?thread_id=` outside `features/chat/` and allows exactly one sidebar `/chat` destination. A vitest guard, because `pnpm lint` cannot run (typescript-eslint vs pinned TypeScript 7). |
> | 4 | Collapse the two pin systems | Done before the 2026-09 pass. One system remains, in `chat-thread-history.ts`; no localStorage pin store exists. |
> | 5 | Wire `Last-Event-Id` / resume | **Done.** Resume now receives the full evidence handler set, and `carryInFlightAssistantTurn` keeps the in-flight assistant turn across leave-and-return (session-core records no assistant message until a turn finishes, so the server merge used to drop it). |
> | 6 | Delete or fix: `reason`, `@`-mention, `space_ref`, AG-UI | **Done.** `reason` removed from the composer tools and the tool-name mapping; `@`-mention and `?space_ref=` verified live (the inventory was stale); AG-UI kept as a planned surface by owner decision. |
> | 7 | Non-image attachments | **Done, with a timing caveat.** Text attachments (`.txt/.md/.csv/.json/.html`) persist to Data Plane v2 and ground later turns — verified live: a `.txt` reached `documents` as `d201f41c…`, reached `status = indexed`, and the next turn quoted it correctly. The route had to be fixed, not just called: `POST /api/v1/chat/documents` was dead end-to-end (502) because model-gateway calls `DocumentService.CreateDocument` over gRPC and Data Plane v2 disabled it ("use documents-api-go POST /v1/documents instead"). The Frontend Plane gateway now takes that route directly, reusing the documents-api leg the knowledge domain already proxies. Caveat: DP2 chunks asynchronously, so the turn carrying the file cannot use it (measured: turn one answered "I find no notes"); the notice says so. PDF/DOCX stay on the per-file "Add to knowledge base" route, since `create_document` takes text `content`, not bytes. |
> | 8 | Single four-tab right panel + auto-open rule | **Done.** Auto-open rule (Work > Output > Sources, once per thread, Trace never steals focus) implemented; `attachmentCount` no longer hardcoded to 0; the rail keeps ONE geometry whoever renders it. `ChatLiveRunPanel` already hosted Work during a live run but at the narrow basis meant for sitting beside the canvas — measured at 974px the rail was 234px on Work and 396px on Sources, so tab switches moved it 162px and the densest tab was the narrowest. Both rails now read `--verevon-workspace-rail` (verified equal at 974px and 1400px), and the panel reports itself as `Arbeidsflate` when hosting Work. Guarded by `workspace-rail-geometry.test.ts`. The literal component fold was deliberately not done: the 671-line panel is interwoven with its rail chrome, its own tests are already 8/10 red for unrelated reasons, and remounting the run stream on every tab switch would regress. |
> | 9 | Ask/Do switch + grounding-scope control | Done before the 2026-09 pass; confirmed against code 2026-09-02 (`DashboardComposer.tsx`, response-mode selector). |
> | 10 | Plan view in the Work tab | **Done.** Resource re-keyed on step progress so it stops going stale; `humanizePlanStepOperation` labels; `PLAN_(STEP_)STATE_` prefixes stripped; `CompleteStep` status mapping fixed in session-core (`skipped` + error-based fallback); `PlanStep.detail` added to the proto and surfaced. Amended 2026-09-02 (evening): the live audit found the panel stuck on "Laster plan og oppgaver …" for entire runs. Two causes, both fixed: the resource was keyed on the step count so every step restarted the fetch and the compat shim discarded each superseded result (now a stable key with a throttled, coalesced refetch and stale-while-revalidate rendering); and `ChatPage` handed the rail its Work content as an inline `contextualPanel()` call, which re-instantiated the panel on each of the rail's eight prop reads — seven orphaned panels fetching in parallel (now a `createMemo`). Measured after: one request per refresh, four seconds apart, quiet when no steps arrive. Still open, separate: follow-up turns in an existing thread and research-mode runs after reload get no `runId` on the turn, so the panel is absent for them. |
> | 11 | Trace tab as audit record | **Done.** Owner decision: audit, not sharing (see §6 Q4). `appendUiEvent` with a 5,000-event cap and a visible notice replaces three silent `.slice(-160)` truncations; the duplicated run-proof panel now lives only in Trace. |
> | 12 | Thread-list status chips | Done before the 2026-09 pass; confirmed 2026-09-02 (`CoreSidebar.tsx` renders `chatRunStatusChip(item.latestRunStatus)`). The "navigating away drops the run" half is closed by item 5. |
> | 13 | Deep-research progress surface | **Done.** `SubQueryOutcome` per sub-query, steps emitted active then resolved, `search_sub_queries` returns the outcomes; visible in Work while it runs. |
>
> **Outside the plan, still open:** three UI markers from the owner's own list were logged rather than built because each needs a new Model Plane contract — claim-level span binding, source-freshness indicator, confidence/verification marker. The message-pinning UI indicator (pin one message into context; distinct from item 4's thread pins) was not addressed. The §2.3 "Unproxied Model Plane" row (realtime, video, translate, language, document-AI, `/v1/tasks`, `/v1/toon/encode`) is inventory only and untouched. The top nav still has two labels pointing at `/chat` ("Chat" and "Oppgaver"). The 2026-09-02 cross-check (section 7) turned the externally corroborated gaps into plan items 14-18.

---

## 5. Harness mechanisms this design depends on

Drawn from the second-pass audit (`claude-hermes-deepseek.md` §13):

| Mechanism | Source | Where it lands here |
|---|---|---|
| **Tool presentation as a pure, non-persisted, card-tagged intent** owned by the tool (`generic`/`terminal`/`diff`/`read`/`search`, with `truncated`/`total` mandatory on search) — *never persist the view, recompute at emission* so improving a card retroactively improves all history | DeepSeek | Tool cards in the centre column; Work tab step rendering |
| **`ConversationNodeDefinition` + keyed renderer registry** — new message types ship as independent contributions instead of growing a central switch | DeepSeek | `src/shared/chat-nodes`; lets approvals, agent-run cards and provider-action cards each ship independently |
| **One generic keyed projection channel** (`{key, value, seq}`, higher-seq-wins) replacing bespoke live-state channels; absence of a key means "feature not composed — render nothing" | DeepSeek | Plan state, todos, approval policy, context meter, title |
| **Typed structured `details` side-channel on tool results**, separate from model-visible `content` | pi | Rich cards without the model paying tokens for presentation data |
| **No-throw stream envelope** — every provider failure becomes a terminal in-band error event with a well-formed zero-usage message, never a thrown exception | pi | One SSE writer code path; cost accounting never has a hole |
| **Session as a tree + append-only compaction** (record summary + first-kept-entry id; never delete summarized entries) | pi | Makes branch/fork a real feature and keeps the untruncated transcript for audit/erasure — directly serves GDPR posture |
| **Three-way steering** — enqueue-while-running with a pending chip, soft interrupt only when every in-flight action is cancellable, hard stop; every cancelled action still emits a result stating why | Claude Code | Composer behaviour during a run |
| **Approvals as a durable addressable queue, not modals**; accept-with-guidance and deny-with-guidance as text on the same control | Claude Code | Work tab + composer chain slot |
| **"Don't ask again" shows and lets the user edit the exact rule**, names its scope (session / space / org), org grant is an audited admin-gated policy change | Claude Code | Approval UX |
| **Context inspector** — itemise the window by category with per-item drill-down, computed against what the model will actually see after compaction | Claude Code | A Trace-tab or composer-adjacent affordance |
| **Prefetch/queue split for memory recall** (queue at end of turn N, serve from cache at head of N+1) + a `recall_status` return so the UI can render "recalled N items" deterministically | Hermes | Keeps memory off the time-to-first-token path and makes recall a visible trust surface |
| **Composer chain slot** — registered entries expose a pure selector over `(session, pendingWaits)`; first non-null match takes over the input area | DeepSeek | Approvals and questions land in the composer, and future confirm surfaces insert themselves with zero composer edits |

### Explicitly rejected
- **Manus's local-filesystem/desktop story, raw terminal and VS Code surfaces** — breaks browser-first and speaks to developers, not to a quality manager.
- **Always-on three-panel density as a cold start** — the specific thing that scares off the ChatGPT-fluent newcomer.
- **NotebookLM's Audio/Video Overview** — consumer delight, high generation cost; nobody forwards an AI podcast to a board.
- **Perplexity's generic curiosity follow-up chips** — replace with permission-scoped *next actions*.
- **Per-user gamification/leaderboards** (Hermes) — in a Norwegian works-council context, an artifact ranking one employee's activity against another's is a concrete labour-relations problem. Aggregate to org capability-adoption or don't ship it.

---

## 6. Open questions

1. **Mobile.** Every reference collapses to one column; the rule for which panel wins is unverified. Recommendation: the right panel becomes a full-screen sheet, never a squeeze. Needs a decision.
   **Corroborated 2026-09-02** by two of the articles in section 7 (designpixil: chat goes full screen on phones, history in a bottom sheet, never a side-sliding panel; uxstudioteam: action overlays over navigation). The recommendation stands; the decision is still open.
2. **Norwegian vocabulary.** Ask/Do, Work/Sources/Output/Trace, "Grounded in", and the typed pause-state labels all need Norwegian equivalents settled **before** components are built, not translated afterwards.
3. **Keyboard model.** Four tabs + a mode switch + a job-queue rail is a lot of focus surface; deserves its own pass (Claude's explicit `Cmd+;` branch is a useful precedent).
   **Answered 2026-09-02: adopt the model thefrontkit spells out** (section 7.3), as plan item 14: Tab flows composer, send, latest answer, its actions, next message; Escape closes the contextual panel and any popover and returns focus to what opened it; arrow keys move between messages and between suggestion chips; Enter sends, Shift+Enter breaks a line; focus returns to the composer when an answer completes and moves to an inline error when one appears. Nothing here is built yet.
4. **Trace = audit or sharing?** Determines the permission model.
   **Answered 2026-09-02: audit.** Trace is the run's audit record, not a sharing surface — owner decision. Applied: Trace never takes auto-focus; the UI-event buffer is capped at 5,000 events with a visible notice instead of the silent 160-event truncation it replaced; the run-proof panel lives only in Trace. Sharing, if ever wanted, is a separate permission model to design later.
5. **Per-run cost to the end user.** None of the four references show it; Verevon has a real cost story in Model Plane that may need a home on this surface.

---

## 7. Reference-ecosystem cross-check (2026-09-02)

An eight-platform "agent ecosystem matrix" (T3 Code, Claude Code, OpenAI Codex,
Cursor, Google Antigravity, Warp, Manus.ai, Perplexity, DeepSeek Harness) with a
"Universal Design Language" and a fused three-column "Web UI Matrix" mock-up was
checked against this document, the Codex implementation plan, the Fjordlys audit,
the code, and the live page. The three chat-UI articles it cites were read as
well: designpixil (July 2026, mixes cited research and opinion), uxstudioteam
(December 2025, opinion with case studies), thefrontkit (February 2026, opinion).
The matrix itself carries no sources, and several of its claims cannot be checked
("massive growth since early 2026", "Cordis graph topographies"), so it is treated
as a design prompt, not as evidence. The same list of products already appears in
the Codex plan's section 1 as the source of "persistence and control" -- nothing
new in the list, only in the conclusions drawn from it.

### 7.1 The "Universal Design Language": one tenet adopted, two rejected

| Tenet | Verdict | Why |
|---|---|---|
| **High-density layout** -- "screen real estate is never wasted on whitespace" | **Rejected at cold start; accepted inside Work.** | It is derived from five coding tools and describes what developers expect from developer tools. It contradicts section 3.1 (density resolved "in time rather than in space"), section 5's explicit rejection of always-on density, Fjordlys laws 1-2 -- and the matrix's own sources: designpixil, "a chat window that works in isolation routinely fails next to a dense dashboard"; uxstudioteam, one question at a time, large friendly type, progressive disclosure. Where the work is dense -- plan, steps, telemetry, trace -- it lives in the summoned Work and Trace tabs, which may be as dense as they need to be. |
| **Asymmetric 3-column shell** as the default frame, with a file explorer on the left | **Rejected as a default; already present when summoned.** | THE ONE RULE (section 3.1): the right panel is opened by the work, never by the product. The live check on 2026-09-02 found the product breaking its own rule in the other direction -- the Work panel auto-opens on a plain Ask turn because bookkeeping steps (model selected, memory recalled, usage recorded) count as "work" (ChatPage.tsx:267-309). That was a bug against this document (item 18, fixed the same evening -- see 7.4), not a reason to adopt the shell. The left column is the thread rail with status chips -- Manus's job queue -- not a file explorer; there are no files. |
| **Explicit permission gate** -- Approve / Stage / Reject / Halt before anything is written | **Adopted; built.** | Ask/Do (3.2), approval cards, typed pauses, effect-immutable turns (3.4), durable cancellation receipts (all verified 2026-09-02). "Stage" is a code-diff idea -- provider actions are atomic, so there is nothing to stage. Corroborated by uxstudioteam's "confirm before executing" cards. |

### 7.2 Per platform: steal, have, or reject

| Platform | The matrix says to steal | Verevon |
|---|---|---|
| T3 Code | Three-panel layout; one-click approve/reject | Approve/reject: **have** (approval cards, plan-approval control). Three panels: rejected as a default (7.1). |
| Claude Code | Dense monospaced, append-only text loops | **Have where it belongs**: tool cards and code blocks use `--font-mono` (Geist Mono); the transcript is not a terminal. Three-way steering and the approval queue were already taken from Claude Code in section 5. |
| OpenAI Codex | Async task loops, multi-worktree tracking | Not applicable to a chat product. The nearest concept is delegation -- Codex plan Phase 9, an unstarted backend spike. |
| Cursor | Multi-file composer canvas, file explorer, inline autocomplete | **Rejected**: no files to explore, no code to complete. The one autocomplete-like affordance -- Tab accepts the rotating placeholder -- is built. |
| Google Antigravity | Horizontal active-agent track, background scheduling | **Have, in calm form**: thread-rail status chips (`chatRunStatusChip` in CoreSidebar), quiet on completion by design. |
| Warp | Isolated command blocks | **Have**: `ToolCallCard` renders each tool run as one surface; Trace is the append-only log. |
| Manus.ai | Interactive browser canvas, sandbox persistence, /skill registry | **Have**: `ChatLiveRunPanel` with honest screenshot states and pause/resume; runs survive navigation (item 12); slash commands in the composer. Manus's desktop, terminal and VS Code surfaces stay rejected (section 5). |
| Perplexity | Inline citation matrices, structured charts, source tracking | **Partly**: numbered `[n]` chips open source cards, Sources tab with counts. Claim-level anchors, freshness and confidence markers: logged, need a Model Plane contract. Charts: not rendered -- markdown tables only. |
| DeepSeek Harness | Real-time token telemetry, cache metrics | **Have, in the Work tab**: tokens/sec, output tokens, prompt-cache reads (ChatPanels.tsx:606-614) plus per-turn cost in the transcript. Deliberately not in the header: Codex plan Phase 5 keeps developer telemetry off ordinary Ask turns. |

### 7.3 What the three cited articles recommend, against the page today

| Recommendation (source) | Status |
|---|---|
| Prominent Stop during streaming (thefrontkit, designpixil) | **Have**; Stop also records a durable cancellation receipt. |
| Copy, thumbs, retry; edit-and-resubmit with branching (all three) | **Have**; tiered feedback (thumbs, then a note) have. |
| Auto-scroll pauses when the reader scrolls up (designpixil) | **Have**: `autoFollow` releases at 80px, a scroll-down affordance appears at 160px (use-chat-controller.ts:1385-1396). |
| 3-5 specific example prompts in the empty state (designpixil) | **Have**: three rotating org-specific prompts, Tab to accept (verified live). |
| First message = "two sentences and a suggestion": scope plus a concrete next move (designpixil) | **Gap at the cross-check, CLOSED 2026-09-03.** The greeting was a static "Hva vil du få gjort?"; it is now a time-of-day greeting with the user's first name plus a resume offer taken from real thread history -- item 17 in 7.4. |
| AI-generated thread names, not timestamps (designpixil) | **Have**: generated titles are displayed and rehydrated. |
| Superscript citations opening source cards with title, URL, excerpt, domain (thefrontkit) | **Have at source level**; chips appear only when the model returns a structured citations array (a narrow trigger, observed live). |
| Freshness indicators and confidence markers (thefrontkit) | **Logged** -- needs a Model Plane contract (unchanged). |
| "Searching in:" scope indicator (uxstudioteam) | **Have**: the "Kunnskap" / "Kunnskap + nett" label and its context panel. |
| Breakdown of how the answer was produced (uxstudioteam) | **Have**: Trace tab, Work steps, the "Brukte N minner" recall notice. |
| Partial results for long tasks (uxstudioteam) | **Have**: per-sub-query deep-research progress (verified). |
| Low-confidence hedge plus a verification path (designpixil) | **Have for deep research**: the pipeline is instructed to say when the evidence base is thin (deep_research.rs:1191). Not generalized to ordinary answers. |
| Errors inline, next to the message (designpixil) | **Have** (`verevon-chat-error-notice`). |
| 44x44 minimum tap targets (designpixil) | **Gap at the cross-check, CLOSED 2026-09-03.** 32px x the app-wide `zoom: 0.9` measured 28.8px, and Fjordlys found the same; the target is now a zoom-compensated token measuring 44.0px -- item 15 in 7.4. |
| Streaming announced through `aria-live="polite"`, batched (thefrontkit) | **Gap at the cross-check, CLOSED 2026-09-03.** The transcript had no live region; it now announces the turn's lifecycle with a 10s heartbeat -- item 16 in 7.4. Focus-to-inline-error remains open. |
| Buffer incomplete markdown; defer code until the closing fence (thefrontkit) | **Have by construction**: an unclosed fence renders as a code block to end of content (chat-media-markdown.tsx:593-598), so partial tokens do not break the layout. |
| Full keyboard model (thefrontkit) | **Adopted** as the answer to section 6 question 3 -- item 14 in 7.4. **Partly built 2026-09-03**: Escape closes the contextual panel and focus returns to a stranded composer. Tab order through answer/actions and arrow keys between messages remain open. |
| Mobile: full screen on phones, bottom sheet rather than a side panel (designpixil, uxstudioteam) | **Corroborates** section 6 question 1; the decision is still open. |

### 7.4 Additions to the plan

14. **Keyboard model** -- the spec recorded under section 6 question 3.
    **Done 2026-09-03, partially.** Two real gaps closed: Escape now closes the
    contextual panel (it only worked inside menus/popovers before, so a keyboard
    user who opened Work had no way back without tabbing the whole panel), and
    focus returns to the composer when an answer settles. Both verified live.
    Two implementation notes worth keeping: the tab strip UNMOUNTS with the
    panel on a thread with no other evidence, so focusing "what opened it" left
    focus on `<body>` -- the composer is the fallback; and reading the previous
    value from `createEffect`'s second argument silently never fires on this
    Solid 2 RC, so the streaming flag is tracked explicitly. Still open from the
    question-3 spec: Tab ORDER through answer/actions/next message, and arrow
    keys BETWEEN messages. Arrow keys across the tab strip already worked
    (roving tabindex in `ChatPanels.tsx`), as does Enter/Shift+Enter.
15. **44px hit areas** for message actions: 32px visual, 44px hit, as Fjordlys
    already specifies -- and reconsider what the app-wide `zoom: 0.9` does to
    every control's real size.
    **Done 2026-09-03.** Measured 44.0px on screen, up from 28.8px. The target
    grows by padding, not an `::after` overlay: the action row's gap is 2px, so
    a 44px overlay on a 32px button would have adjacent targets overlapping by
    10px and the later button stealing its neighbour's edge -- worse than the
    small target. `background-clip: content-box` keeps the visible button at
    32px. On the zoom: rather than reconsider it globally (it is load-bearing
    for the whole shell's density), the token is divided by it --
    `calc(44px / 0.9)` -- so the target is a real 44px where the shell zooms
    and a plain 44px on mobile, which has no zoom and is where touch happens.
    Caveat found while fixing it: the `background` SHORTHAND resets
    `background-clip`, so both the base and hover rules must use
    `background-color`. Guarded, because that is invisible in review.
16. **Streaming accessibility**: an `aria-live="polite"` region for the assistant
    turn, announcements batched every few seconds; focus moves to an inline
    error when one appears.
    **Done 2026-09-03, live region only.** Verified live: "Verevon svarer …" on
    start, then "Svar fullført." plus the opening of the answer. Announces the
    turn's LIFECYCLE, not its tokens -- piping a growing answer into a live
    region re-reads the whole thing on every token, which is worse than the
    silence it replaces. A 10s heartbeat (alternating text, since a live region
    drops a repeat of what it already shows) keeps a multi-minute deep-research
    turn from reading as a hung page. Still open: moving focus to an inline
    error when one appears.
17. **Proactive first message**: scope plus a suggested first move, computed
    from org context, replacing the static greeting.
    **Done 2026-09-03.** Verified live: "God morgen, Ima" (time of day + the
    session's own user name) replaces the static "Hva vil du få gjort?", above
    the scope line that already named AQUATIQ AS, plus a suggested first move
    sourced from real state -- the most recent thread ("Fortsett der du slapp
    — Bitcoin-pris 2. september 2026"). Deliberately from `readChatThreadHistory()`
    rather than generated: a suggestion the product invented is just a fourth
    generic starter chip, and three of those already sit below it. A title over
    60 chars drops the offer rather than truncating it.
18. **Tighten the auto-open trigger** so bookkeeping steps do not count as
    work: the Work panel must not open for a plain Ask turn (ChatPage.tsx:267-309).
    This restores THE ONE RULE the live check found broken.
    **Done 2026-09-02 (evening).** The registry now has two predicates: `available`
    (may the tab be offered -- unchanged, fail-closed) and `claimsFocus` (may it
    interrupt): Work on the first tool call, work step or durable run, Output on
    the first artifact (never the user's attachments), Sources on the first
    citation (never a bare grounding summary), Trace never. `isWorkStep` in
    chat-normalizers.ts classifies steps by the id conventions the producers use
    (`:event-`, `:action-`, real `:tool-<call id>`; composer placeholders and
    unknown shapes stay bookkeeping). Verified live: "What is the capital of
    Norway?" answered with no panel and no canvas class while Work stayed offered
    in the header; a deep-research turn opened Work by itself at 3.1s. Guarded by
    chat-surfaces.test.ts (19 assertions incl. a wiring guard on ChatPage).

Deliberately not adopted, with the reason on record: a file explorer (nothing
to explore); terminal blocks or a terminal surface (the block idea already lives
in tool cards); an always-on telemetry header ("Agent Mesh Monitor [65 T/s]" --
telemetry lives in Work); Stage as a partial apply (provider actions are
atomic); Codex-style multi-worktree tracking (delegation is Phase 9 of the
Codex plan); charts in answers (tables suffice until a real need appears).
