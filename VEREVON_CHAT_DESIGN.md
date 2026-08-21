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
| Non-image attachments | Composer accepts `*/*` and renders chips for PDFs; `toStreamAttachments` drops everything non-`image/*` (`chat-normalizers.ts:382`). The fixing route (`POST /api/v1/chat/documents` → Data Plane ingest) exists with zero callers. |
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
2. **Norwegian vocabulary.** Ask/Do, Work/Sources/Output/Trace, "Grounded in", and the typed pause-state labels all need Norwegian equivalents settled **before** components are built, not translated afterwards.
3. **Keyboard model.** Four tabs + a mode switch + a job-queue rail is a lot of focus surface; deserves its own pass (Claude's explicit `Cmd+;` branch is a useful precedent).
4. **Trace = audit or sharing?** Determines the permission model.
5. **Per-run cost to the end user.** None of the four references show it; Verevon has a real cost story in Model Plane that may need a home on this surface.
