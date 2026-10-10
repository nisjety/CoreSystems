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

### 3.5 The model picker — reversed 2026-09-16

> **Superseded.** This section used to read *"No model picker."* It said: the
> composer exposes an effort dial and nothing else, the pinned
> Budget/Balance/Genius grouping "just needs to stop exposing the raw provider
> catalog underneath". The product went the other way, deliberately, and the
> catalog is in active use. The original text is kept below the decision,
> because the reasoning it carried is still half-right and the half that is
> wrong is worth being able to see.

**Decision: the composer keeps a model picker. The picker is right and the old
line was wrong.** It is recorded here as a reversal, not edited away, because
the reasons the original gave were real and had to be answered rather than
ignored.

**What the old line got right, and still governs.** The *default* is an intent
dial, not a model ID. The composer opens on the pinned Verevon modes
(Budget/Balance/Genius) and the response-mode selector (Raskt svar / Auto / Dyp
research → the wire's `effort`). A quality manager who never forms an opinion
about a model never has to. The raw catalog is behind a collapsed
`<details>` labelled *"Velg modell selv"* — one deliberate click away, never in
the default path. That much of §3.5 is enforced today, not aspirational.

**What the old line got wrong.** It treated model choice as pure preference —
"no Norwegian quality manager has an opinion about a model ID". Two things it
did not anticipate make that false here:

1. **Privacy tier is a governance property of the model, not of the tier
   label.** Picking a tiered catalog model is *how* a `minPrivacyTier` reaches
   the payload (`DashboardComposer.tsx`, `selectedPrivacyTier` →
   `createComposerSubmitPayload`). Hiding the catalog would delete the only
   control a sovereignty-constrained customer has. That is §3.3's own argument —
   a governance control, not a preference — applied to the thing §3.5 wanted to
   remove.
2. **A user-owned subscription is an entitlement, not a preference.** A customer
   who connects their own ChatGPT subscription is routing their own contract.
   Withholding that route because the design doc says "no picker" is not
   simplification.

**Why the 2026-09-15 audit objected, and why that objection no longer holds.**
`CHAT_PARITY_AUDIT_2026-09-15.md` F-08 was right at the time: the picker let a
user land on a provider that failed ~50 % of streams (F-01) and silently
stripped five capabilities (F-10). A picker that hands a user a worse product
without telling them is a trap, and "the design forbids it anyway" made the
trap look like an accident. Each of those has since been closed, and the
picker is safe *because of those specific protections* — not because the
objection was waved off:

| Protection | Where it lives | What it stops |
|---|---|---|
| **Capability gating** (F-10) | `DashboardComposer.tsx` `subscriptionBacked` mirrors `chat-client.ts`'s own `buildChatWireBody` condition; Søk / Bilde / Utfør / Dyp research render `disabled` with `title="Ikke tilgjengelig med denne modellen"` | A control that looks live while the wire sends `false`. Pinned by `DashboardComposer.subscription.test.tsx` |
| **Capability gating, `/` commands** | the `/` catalog withholds search / image / deep-research / skill commands on a subscription route and prints why | The same no-op re-entering through a slash command instead of a toggle |
| **Provider-failure surface** (F-01) | model-gateway `sse.rs::stream_ended_with_no_content`; `use-chat-controller.ts` retries once on an empty model (resolving to Balance) and otherwise renders `ErrorNotice` → *"Prøv igjen"* | A failed stream arriving as an empty bubble labelled "fant ingen dekning" — a fabricated *answer* where there was a *failure* |
| **Tool parity** (F-18) | `buildChatWireBody` now adds the `tools` feature for subscription turns; model-gateway routes the tool-decision round through Balance (`tool_round_model`) and hands the answer off only when a tool actually ran | A subscription turn being permanently unable to reach `get_weather` / `code_interpreter` while every other model could |
| **Tier-aware entitlement** | the Subscription group is filtered out of the catalog unless an *active* connection exists; privacy-tier and `$$` cost badges on each model; sovereign notice under the list | Offering a route the org is not entitled to, or a tier it must not use |
| **Stale-entitlement fallback** | `DashboardComposer.tsx`: a persisted subscription selection whose connection is no longer active falls back to Verevon Balance and says so | The one remaining stranding (below) |

**The stranding that was still there, and is now fixed.** The model selection is
persisted per org (`verevon.ai-model-selection.v1:<org>`); the connection that
makes a subscription model routable is not. When that connection lapsed, the
composer kept the dead selection: the Subscription group disappeared from the
catalog (so the picker showed neither the selection nor a way back), the F-10
mirror kept four controls disabled with "not available with this model", and the
payload kept naming a model the gateway could no longer route. The composer now
detects the lapse once the connection list has actually resolved — `undefined`
is "still loading", not "none" — falls back to Verevon Balance, and shows
*"ChatGPT-abonnementet er ikke lenger tilkoblet. Byttet til Verevon Balance."*

**The rule this reversal leaves behind, which is the part worth keeping:** a
model is offerable only when the composer can tell the truth about it. If a
route cannot do something, the control for that something is disabled *and says
why*; if a route stops existing, the selection does not silently survive it. A
picker that meets that bar is a governance control. One that does not is the
trap F-08 named — and the correct response to that trap is to fix the picker,
not to delete a working control on the strength of a doc line.

<details>
<summary>The superseded 2026-08-17 text</summary>

> The composer exposes an **effort dial** (Quick / Standard / Deep) and nothing
> else. Which model backs each tier is org policy set by the tenant admin,
> visible on hover for transparency. No Norwegian quality manager has an opinion
> about a model ID, and offering one invites a support ticket. This also makes
> the existing pinned Budget/Balance/Genius grouping the *right* shape already —
> it just needs to stop exposing the raw provider catalog underneath.

</details>

### 3.5.1 Slash commands are commands (2026-09-16)

`CHAT_PARITY_AUDIT_2026-09-15.md` F-09 called the composer's slash commands
"two upload/image shortcuts, not commands". It was worse than that: the
`slashCommands` array it pointed at (`/Last opp fil`, `/Generer bilde`) was
**dead** — `applyAutocompleteSelection` looked it up by an id
(`cmd-file`/`cmd-image`) the menu builder never produced, so the live menu had
quietly been the specialized-action list for some time and those two labels
appeared nowhere. Unparameterised *and* unreachable.

The composer now has a real command system. Its contract:

- **Discoverable.** `/` opens a filterable menu; each row shows the command's
  name, what it does, and the argument it takes. Backend skills, capabilities
  and connectors join the same catalog as commands.
- **Parameterised.** A command may consume the rest of the line.
  `/image en rød katt` sends *"en rød katt"* with the image tool set;
  `/dyp <spørsmål>` sends the question with deep research on. Norwegian and
  English aliases both resolve (`/bilde` = `/image`, `/søk` = `/search`).
- **Keyboard-first.** Arrows move, Enter or Tab confirms, Escape closes; the
  list is a `listbox` with a selected `option`.
- **Degrades to text.** The menu opens only when `/` *starts* the draft, so
  "kr 200/mnd" and "og/eller" are prose. An unrecognised `/name` is sent
  verbatim rather than swallowed as a failed command.
- **Subject to §3.5's rule.** On a route that cannot honour them, the
  capability-bearing commands are withheld and the menu says why.

**Constraint this had to respect (F-03).** Enter must submit a plain message.
The composer distinguishes a *deliberately opened* menu, which consumes Enter,
from the *ambient* date suggestion, which must not — an ordinary sentence ending
in "man", "fri" or "tor" prefix-matches a day name and used to swallow the send.
The command menu participates in that same single rule rather than inventing a
second one, and the argument hint strip is deliberately **not** menu state, so a
command that already has its argument sends on Enter like any other message.
`DashboardComposer.enter-to-send.test.tsx` (4 cases) still passes unchanged.

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
---

## 8. Chat-page audit against all three chat documents (2026-09-04)

The live `/chat` page was checked against this document, the UX design
specification (`VEREVON_CHAT_UX_DESIGN_SPEC.md`, 2026-08-31) and the workspace
implementation plan (`VEREVON_CHAT_WORKSPACE_IMPLEMENTATION_PLAN.md`,
2026-08-30). Method: the running stack (org AQUATIQ AS, signed-in user, viewport
1243px) driven through cold start, a plain Ask turn, a tool-calling turn and the
contextual canvas, with the code read alongside. Every finding below was
observed in the browser; the breakpoint geometry in item 24 was measured at four
viewport widths in a second pass after Docker Desktop was restarted.

The UX specification's own header says "Implementation: Deliberately not
started in this pass". That is now out of date -- most of it is built. Its
section 3 audit resolves as follows.

| UX spec section 3 finding | State on 2026-09-04 |
|---|---|
| 1. Composer disappears when a workspace destination is selected (Blocking) | **Closed.** Verified live: the composer stays mounted and usable with the canvas open. |
| 2. Workspace navigation owned by the wrong region (Blocking) | **Closed.** The tab strip renders inside the canvas head (`Arbeid`/`Kilder` + `Lukk arbeidsflate`); the conversation header holds only a workspace trigger, one overflow menu and New chat. |
| 3. Tasks presents an event exhaust rather than a work summary (Blocking) | **Open** -- item 19 below. |
| 4. Action hierarchy duplicated | **Closed.** One New chat control plus one overflow menu in the header; per-message actions stay on the message. |
| 5. Surface vocabulary mixes objects and actions | **Closed.** Chat is no longer a canvas tab (`includeChat={false}`) and Actions is not a tab. |
| 6. Empty state is generic | **Closed** by item 17 (greeting, scope line, resume offer, three starter rows). |
| 7. Provider and token metadata competes with the answer | **Partly open** -- item 20 below. |
| 8. Errors are too close to raw runtime language | **Not verified.** `ErrorNotice` renders a message plus "Prøv igjen"; no failure was induced in this pass. |

### 8.1 Conformant, verified live

| Rule | Evidence |
|---|---|
| Acceptance criteria 1, 10; THE ONE RULE (3.1) | Cold start showed no canvas. "Hva er hovedstaden i Norge?" answered with no panel and no `--canvas` class. |
| Acceptance criterion 2 | Composer mounted and visible with the canvas open. |
| Acceptance criteria 3, 4 | Exactly one tab strip, inside the canvas head. |
| Acceptance criterion 6 | Registry `available` / `claimsFocus` split intact (`chat-surfaces.ts`), precedence Work > Output > Sources > Trace. |
| UX 6 (composer) | `Spør`/`Utfør` is a real segmented control with `aria-pressed`; grounding reads "Kunnskap"; the effort dial reads Rask/Standard/Grundig. |
| Item 15 | Message action buttons measured 44x44 on screen. |
| Item 16 | The live region announced "Verevon svarer …" then "Svar fullført. Oslo." |
| Item 14 (partial) | Focus returned to the composer when the answer settled. |
| Item 17 | "God morgen, Ima", the scope line naming AQUATIQ AS, a resume offer from real thread history, three starter rows. |

Transcript measured 720px wide with the canvas closed and 640px with it open,
against the 680-760px comfort band in UX 5B. The narrower figure is the
split-pane case, where UX 10 only requires 480px for the conversation.

### 8.2 Additions to the plan

19. **The Work canvas is still an event exhaust, and the two Work renderers
    disagree.** UX section 7 wants four answers in order -- what is happening
    now, what is left, does Verevon need me, what happened underneath (collapsed)
    -- and forbids putting connection events, model selection, tool payloads and
    content steps in one flat timeline. The canvas Work surface
    (`ChatPanels.tsx:587`) opens with `<h2>Agent activity</h2>` plus run
    telemetry, then a per-answer group whose contents are flat: observed live on
    the shipping-quote thread as `Connect stream`, `Compose response`,
    `Model selected`, `Tool: Web search`, five `Source found` rows,
    `Tool: Shipping get quotes`, `Usage recorded`, all at equal weight. The live
    rail already implements the specified shape -- status, screenshots, and a
    collapsed `Teknisk aktivitet` disclosure at `ChatLiveRunPanel.tsx:655`. The
    fix is to give the canvas renderer that treatment: plan and typed pause
    above the timeline, runtime rows and telemetry behind the disclosure. This
    is UX finding 3, the one remaining Blocking item.

    **Done 2026-09-04.** The canvas renderer now answers the four questions in
    order: an `<h2>Arbeid/Work</h2>` with a plain-language status line derived
    from the steps themselves (waiting, the active step's title, "Siste steg
    feilet", "Ferdig", or "Ingen aktivitet nå"), the durable plan, a typed
    needs-you callout built only from a step the run actually reported as
    waiting, the work timeline, and one collapsed `Teknisk aktivitet`
    disclosure holding run telemetry, the lifecycle rows and the raw event
    stream. `isWorkStep` splits the two; a per-answer group with nothing but
    bookkeeping no longer renders at all. Reuses the live rail's disclosure
    classes, so the two Work hosts finally look alike; the only new CSS is the
    callout, which borrows the approval card's tint. Verified live on the
    shipping-quote thread: header "Arbeid / Siste steg feilet", two tool rows
    visible, `Teknisk aktivitet 15` closed by default, and no telemetry left in
    the reading flow.
20. **The composer still exposes the raw provider catalog, and the transcript
    still shows a provider name.** Section 3.5 says the composer offers an
    effort dial "and nothing else", and that the pinned Budget/Balance/Genius
    grouping "just needs to stop exposing the raw provider catalog underneath";
    UX 6 repeats it ("Do not show raw provider model IDs in the default
    composer"). The "Velg AI-modell" menu still renders `VEREVON_MODES` followed
    by `chatModelGroups()`. Separately the per-turn chip beside the answer reads
    "Claude Sonnet · 5 tokens"; it is the trigger for the reasoning popover, so
    the detail is disclosed, but the provider name and output-token count sit in
    the reading flow, which is UX finding 7.

    **Done 2026-09-04.** The catalog became a secondary control rather than
    being deleted: UX spec section 6 lists "advanced model transparency" among
    the controls revealed only when needed, and picking a tiered model is how a
    privacy tier reaches the payload. It now sits behind a closed "Velg modell
    selv" disclosure inside the model menu, so the default menu shows the three
    Verevon effort modes and nothing else (verified live). The sovereign-tier
    note stays outside the disclosure -- it describes the current selection. The
    transcript chip that read "Claude Sonnet · 5 tokens" now reads "Detaljer";
    the panel it opens already carried Modell, Input, Output, tid, Sikkerhet and
    Kostnad, which is where UX finding 7 puts them.

    **Superseded 2026-09-16.** The tension this item was working around — "make
    the catalog as invisible as §3.5 demands without deleting the privacy-tier
    control" — is resolved at the source: §3.5 was reversed, and the picker is
    now design-sanctioned rather than tolerated. The shape reached here (pinned
    modes by default, catalog behind "Velg modell selv") is what §3.5 now
    prescribes, so this item's outcome stands; only its framing changed.

21. **Mixed-language chrome.** Acceptance criterion 9 rejects it outright. 42
    Norwegian strings in `src/features/chat` bypass `i18n.tr` (visible text plus
    `aria-label`/`title`), against 44 that use it, so roughly half the chat
    chrome cannot switch to English. One header renders "Agent activity" over
    "Live oppgavestatus" in the same block. Distribution: `ChatPanels.tsx` 14,
    `ChatLiveRunPanel.tsx` 9, `ChatArtifactPanel.tsx` 5,
    `ChatAttachmentCanvas.tsx` 5, `ChatMessages.tsx` 5,
    `ChatWorkspaceCanvas.tsx` 2, `ChatPage.tsx` 1, `chat-media-markdown.tsx` 1.
    Examples: "Ingen kilder ennå", "Venter på agenten", "Lukk arbeidsflate",
    "Endre bredde på arbeidsflaten", "Godkjenning", the ZDR banner, and the
    citation chip's `Kilde N` label.

    **Done 2026-09-04.** 65 strings wrapped across eight files (50 found by the
    first sweep, 15 more once the detector stopped being case-sensitive about
    Norwegian words -- the `StepsPill` label read `total() === 1 ? 'steg' :
    'steg'`, which is right in Norwegian and needs a real plural in English).
    `i18n.tr` calls in `src/features/chat` went from 59 to 125, and a scan for
    Norwegian text or aria-labels outside `i18n.tr` now returns nothing. Two
    mechanical traps worth recording: the hook must be placed by the SHAPE of
    the line that opens a component body (`) {`, `}) {`, `=> {`), because a
    multi-line props TYPE opens a brace of its own and a depth-based rule put
    `const i18n = useI18n()` inside `InlineCitationMarker(props: {`; and the
    import has to follow the last complete import STATEMENT, not the last line
    starting with `import`, which is the opening line of a multi-line specifier
    list. Verified live: with the locale switched to English the chat chrome
    reads Work / More actions / New chat, tabs Work and Sources, "The last step
    failed", "Technical activity", and a DOM sweep of the header, canvas, run
    panel and composer found no Norwegian left. Only the assistant's own answer
    text stays Norwegian, which is content, not chrome.
22. **Generic curiosity follow-up chips are shipping.** Section 5 lists them
    under Explicitly rejected, to be replaced with permission-scoped next
    actions. Observed live after the answer "Oslo.": "Hva er befolkningen i
    Oslo?", "Hvilke kjente severdigheter finnes i Oslo?", "Hva er klimaet som
    vanligvis er i Oslo?" -- rendered by `FollowUpChips` from model-suggested
    text.

    **Done 2026-09-04.** `deriveChatNodes` no longer emits the node, so nothing
    renders. The node kind, its renderer and `FollowUpChips` are deliberately
    left in place with the reason on record: the replacement section 5 asks for
    -- permission-scoped next actions -- needs a server-side suggestion contract
    scoped to what this user may actually do, and inferring actions from free
    text in the browser is the fabrication this codebase forbids. Two derive
    tests pinned the chips as shipped behaviour and now assert the opposite, so
    restoring the push fails loudly rather than silently.

23. **The low-confidence hedge is now generalized to ordinary answers.** Section
    7.3 records it as present for deep research and deliberately *not*
    generalized. Observed live: "Usikkert svar (52% sikkerhet) — sjekk kilder
    før du stoler på dette" under the one-word answer "Oslo.", driven by
    `message.n`. A hedge on a trivially correct fact trains the user to ignore
    the badge; either scope it back to retrieval-backed turns or raise the
    threshold so it marks answers a reader should actually re-check.

    **Corrected and partly done 2026-09-04. The item's premise was wrong.**
    Section 7.3's "Not generalized to ordinary answers" is a status row in a
    recommendation table, not a prohibition, and the code carries a documented
    incident against gating the hedge on grounding: on 2026-07-20 a confidently
    wrong, uncited answer scored 0.72, computed but visible only inside the
    Reasoning popover. `LOW_CONFIDENCE_ANSWER_THRESHOLD = 0.75` exists to catch
    exactly that, so an ungrounded answer SHOULD be flagged and the hedge stays.
    What was genuinely broken is what the notice pointed at: it told the reader
    to "sjekk kilder" on a turn that had no sources. The node now carries
    `hasEvidence` and the wording follows it -- "sjekk kildene" when citations or
    grounding exist, "ingen kilder ble brukt, så bekreft det selv" when they do
    not. Verified live under the answer "Oslo." A caveat that sends the reader
    after nothing is how caveats get ignored.

24. **The mobile-sheet threshold is 40px short of the specification, so a band
    of widths shows the squeeze UX 10 forbids.** Measured with the Work canvas
    open on a durable thread:

    | Viewport | Canvas | Share | Conversation | Layout | UX 10 |
    |---|---|---|---|---|---|
    | 1400px | 396px | 28% | 576px | split | split, conversation >= 480px -- **met** |
    | 1000px | 396px | 40% | 481px | split | canvas 48-58% -- **missed**, conversation readable |
    | 745px | 340px | 46% | 372px | split | full-screen sheet -- **missed**, squeezed side by side |
    | 700px | 699px | 100% | covered | `position: absolute; inset: 0` | sheet with "Lukk arbeidsflate" -- **met** |

    The sheet itself is correct; it is keyed at `max-width: 720px` where UX 10
    says 760px, so 720-759px keeps a 372px conversation beside a 340px canvas --
    exactly the "never squeeze chat and preview side by side" case. Moving the
    media query to 760px closes it. The 1000px row is a separate, milder
    mismatch: the rail is `clamp(320px, 30vw, 440px)` and lands at 40% of the
    workspace where the specification asks for 48-58%. That deviation protects
    the conversation, so the specification may be the side to change -- but the
    two should not disagree silently. The composer stayed mounted and visible at
    all four widths.

    **Done 2026-09-04, the sheet half.** The canvas block moved to
    `max-width: 760px`, and `.verevon-chat-page--canvas .verevon-chat-run-panel`
    -- the same sheet rule for the live rail, which also hosts Work -- moved out
    of the 720px header-chrome block into it, so both Work hosts flip at one
    width instead of two. Re-measured at 745px: `position: absolute`, 744px of a
    745px viewport, "Lukk arbeidsflate" present and the composer still mounted.
    The mid-band share is deliberately NOT changed and stays open: the canvas is
    31-40% of a 1000px workspace (it varies with which host renders) where the
    specification asks for 48-58%. That deviation protects the conversation, so
    the specification is the more likely side to move -- but it needs a decision
    rather than silence.
25. **Two top-nav labels still point at `/chat`** ("Chat" and "Oppgaver",
    `shell-data.ts:131`). Already on the open list at the end of section 4; still
    true. The route-ownership guard (`chat-route-ownership.test.ts`) covers
    cross-surface adoption links, not this duplication.

    **Done 2026-09-04.** `CoreNavbar` gives both breadcrumb links the same href
    (`props.activeRoute`), so a second label for /chat was always a duplicate
    destination. `getNavbarLabels('/chat')` now returns an empty tab label and
    the breadcrumb omits the separator and the link when there is none. Verified
    live: one crumb, `Chat -> /chat`, no repeated href.

26. **Keyboard model, remaining half.** From item 14: arrow keys move across the
    canvas tab strip (roving tabindex, `ChatPanels.tsx:274`), resize the canvas
    (`ChatWorkspaceCanvas.tsx:101`) and move sidebar thread selection with
    Cmd/Ctrl+Shift (`use-chat-shortcuts.ts:25`), but nothing moves focus between
    transcript messages, and Tab order through answer -> actions -> next message
    is still unspecified. From item 16: focus does not move to an inline error
    when one appears.

    **Done 2026-09-04.** Each message is a focusable region (`tabindex="-1"`, so
    none of them join the Tab sequence and Tab still runs answer -> its actions
    -> next message through DOM order), and the message list handles a bare
    ArrowUp/ArrowDown. The handler refuses to act on any target inside an input,
    textarea, select, contenteditable, tablist, listbox, menu or separator, so
    the composer, the model menu, the workspace tab strip's roving tabindex and
    the canvas resize handle all keep their own arrows. Entering the transcript
    from elsewhere lands on the newest message going up and the oldest going
    down; past either end it does not swallow the key. Verified live on a
    four-message thread with the canvas open: the strip still moves Arbeid ->
    Kilder on ArrowRight while the transcript walk works. An inline error also
    takes focus when it appears, closing the open half of item 16.

27. **Work is offered on a plain Ask turn whose whole content is bookkeeping.**
    UX 4 says "Only evidence-backed destinations appear. The panel never opens
    empty in the ordinary Ask flow." Item 18 fixed the *opening* by giving the
    registry a stricter `claimsFocus` predicate over `isWorkStep`, but left
    `available: stepCount > 0 || hasRun`, which still counts bookkeeping. Measured
    on the "Hovedstaden i Norge" thread -- one question, one-word answer, no tool
    call: the conversation header advertised `Arbeidsflate -> Arbeid 4`, and the
    panel contained exactly `Connect stream`, `Compose response`,
    `Model selected`, `Usage recorded`. Nothing there is work. Gating `available`
    with the same `isWorkStep` classifier `claimsFocus` already uses would leave
    a plain Ask thread with no workspace trigger at all, which is what UX 4
    describes.

    **Done 2026-09-04.** The registry gained `hasWorkEvidence(state)` --
    `workStepCount > 0 || toolCallCount > 0` -- and `available` is now that or a
    durable run, which is exactly what `claimsFocus` reads. Both surface hosts
    had to be told: `ChatHeader` (the dropdown that advertised "Arbeid 4") and
    `ChatTabs` each build their own `ChatSurfaceAvailability`, and a caller that
    omits the counts now gets no Work destination rather than one keyed on the
    lifecycle total. The Work badge counts work too, so the shipping thread
    reads 3 rather than 18. ChatPage derives both counts once in a `createMemo`
    that the stale-tab guard, the auto-open rule, the header and the tab strip
    all share -- four inline `filter(isWorkStep)` copies is how they drift.
    Verified live: the one-question thread's header is down to "Flere
    handlinger" and "Ny samtale" with no workspace trigger, in both languages,
    while the tool-calling thread still offers Work. `chat-surfaces.test.ts` now
    asserts the inverted contract (12 pass), including the wiring guard, which
    had pinned the old inline expression.

### 8.3 Document integrity

Two of the three documents existed only in the retired OneDrive folder and were
absent from `C:\dev\CoresSystem`, the checkout the running stack now builds
from: `VEREVON_CHAT_UX_DESIGN_SPEC.md` and
`VEREVON_CHAT_WORKSPACE_IMPLEMENTATION_PLAN.md`. Both were copied into the clone
as part of this audit. The OneDrive copy of *this* document is 48 lines behind
the clone's -- it predates the 2026-09-03 closures of items 14 through 17 -- so
the clone is authoritative and the OneDrive copy should not be merged back.

### 8.4 Fix pass, 2026-09-04

Items 19, 21, 24 (sheet threshold) and 27 are implemented; each is recorded
against its own entry above. Items 20, 22, 23, 25, 26 and the mid-band half of
24 are untouched and still open.

Checks after the pass: `pnpm typecheck` clean, `pnpm lint` 0 errors (120
pre-existing `solid/reactivity` warnings), `chat-surfaces.test.ts` 12/12.

Fourteen tests fail in `src/features/chat`, none of them caused by this pass and
all worth their own fix:

- `ChatPanels.test.tsx` 10 and `ChatArtifactPanel.test.tsx` 2 -- both files fail
  identically with the committed component swapped back in, so they pre-date
  this work. The design doc's item-8 note already recorded this class of red for
  `ChatLiveRunPanel`.
- `mid-run-input.test.ts` 1 -- a source-scanning guard searching
  `use-chat-controller.ts` for an anchor written with `\n` line endings, in a
  file that has 2,813 CRLF pairs. It cannot match on this checkout regardless of
  the code, and the file is untouched by this pass. The guard should normalize
  line endings before searching.

### 8.5 Second fix pass, 2026-09-04

Items 20, 22, 23, 25, 26 and the mid-band half of 24 are resolved, each against
its own entry above. Every item this audit raised is now closed.

Two did not close the way the audit framed them, and their entries say so:
item 23's premise was a misread status row, so the hedge stayed and the
verification path it pointed at was fixed instead; item 24's mid band was the
conversation floor working as designed, so the specification moved rather than
the code.

Item 22 removes a shipped affordance without replacing it. What section 5 asks
for instead needs a server-side action-suggestion contract, so it joins
claim-level span binding, source freshness and the confidence marker on the list
of UI work blocked on a Model Plane contract.
---

## 9. Cross-document completion check (2026-09-06)

Section 8 audited the chat page. This section audits the three documents
themselves: what each one asks for, and whether the code actually does it.
Method: every claim below was checked against the code in this checkout, not
against the documents' own status lines. Where a document claimed something was
done and the code disagreed, the code wins and the row says so.

### 9.1 Design doc — items 1-27

All 27 are closed. Items 1-13 were verified against code on 2026-09-02 (the
table in section 4), items 14-18 in section 7.4, items 19-27 in sections 8.2,
8.4 and 8.5. Two spot-checks of the oldest claims held up:

- Item 4 (collapse the two pin systems): one system remains. The pin is
  server-owned (`ChatThreadSession.pinned`, written by `PUT`), and
  `withPinnedCarry` now preserves it across a snapshot upsert that omits it,
  guarded by a named regression test.
- Item 5 (`Last-Event-Id`): genuinely wired both ways.
  `use-chat-controller.ts` records every frame id onto the turn through
  `onFrameId` and passes `turn.lastFrameId` back as `streamChat`'s resume
  cursor; `ChatLiveRunPanel` does the same for the durable run stream.

### 9.2 Design doc — section 2.3 wired-but-dead inventory

Seven of nine rows are resolved. Two are not:

| Row | State on 2026-09-06 |
|---|---|
| `Last-Event-Id` resume | **Closed.** Recorded and passed; see 9.1. |
| Sidebar pin | **Closed.** One server-owned system. |
| Plan view | **Closed.** `listPlans` / `listTodos` / `getLineage` all have real callers (`ChatPanels.tsx`, `AgentRunConsole.tsx`). |
| Non-image attachments | **Closed** (item 7), with the documented async-indexing caveat. |
| `@`-mention on chat page | **Closed by removal.** `mentionedAgentRef` is sent only from `SpaceRoomComposer` after a roster selection; global Chat treats `@` as text. |
| `reason` response mode | **Closed by removal.** The composer sends no such tool and `chat-normalizers.ts` carries the reason. |
| `?space_ref=` | **Closed by removal**, with the reason in `use-chat-controller.ts`. |
| AG-UI | **Still open, and item 6 said "no third option".** `ag-ui-client.ts` has exactly one importer app-wide: its own test file. The gateway route is live (`/api/v1/ag-ui/stream`), the adapter it shares with the native path (`shared/chat/verevon-ui-events.ts`) is real and used — but by `chat-client.ts`, not by the AG-UI client. So a fully built, fully tested parallel transport ships with no way to reach it. Kept as a planned surface by owner decision on 2026-09-02; that decision is what "delete or fix" was meant to prevent, so it should be either reached or removed, not carried indefinitely. |
| Unproxied Model Plane | **Still open, untouched inventory.** Gateway route coverage measured today: realtime 0, translate 0, language 0, document-AI 0, `/v1/tasks` 0, `/v1/toon/encode` 0. Only video has any gateway mention. |

### 9.3 UX spec — section 11 acceptance criteria

| # | Criterion | State |
|---|---|---|
| 1 | Ask without Work/Output/Trace chrome | **Met** (verified live, 8.1). |
| 2 | A workspace destination never hides or resets the composer | **Met** (verified live, 8.1). |
| 3 | No tab strip in the conversation header | **Met** (verified live, 8.1). |
| 4 | Exactly one tab strip, inside the canvas | **Met** (verified live, 8.1). |
| 5 | Inspect a result and request a revision without closing it | **Met**, as a consequence of 2: the composer stays mounted and usable with the canvas open. Not separately exercised as a flow. |
| 6 | Deterministic first-summon precedence, sticky manual choice | **Met** (`chat-surfaces.ts`, 19 assertions). |
| 7 | Work presents outcomes first, runtime detail second | **Met** by item 19: heading, status line, plan, needs-you callout, then work sections; telemetry, bookkeeping steps and the event log behind one disclosure. |
| 8 | Resize, close, focus transfer, keyboard tabs, reduced motion, mobile sheet | **Met.** `role="separator"` handle with a persisted width clamped by `MIN_CONVERSATION_WIDTH = 480`; `Lukk arbeidsflate`; focus moves into the mounted tabpanel; roving tabindex on the strip; 8 `prefers-reduced-motion` blocks; the full-screen sheet at `max-width: 760px`. |
| 9 | Norwegian and English labels switch together | **NOT met.** Verified today by reading the components: the chat feature has 128 `i18n.tr(...)` calls and a residue of single-language literals that cannot follow the locale in either direction. English-only: `aria-label="Answer version"`, `"Previous version"`, `"Next version"`, `title="Open image"`, `"Download image"`, `aria-label="Scroll to bottom"`, `"Chat workspace views"`, `"Verevon chat workspace"`. Norwegian-only: `"Lukk arbeidsflaten"` / `"Vis live-panelet"` / `"Skjul live-panelet"` (`ChatLiveRunPanel.tsx:489-490`), `title="Resultatet ble avkortet"`, `aria-label="Fullmakt"`, `placeholder="Hvorfor trenger agenten denne fullmakten?"`, `` aria-label={`Last ned ${...}`} ``. Most of them are accessible names, which is why the live pass in section 8 did not catch it: the visible chrome does switch. A screen-reader user gets mixed-language chrome in both locales. This is mechanical to close and deserves a guard test, not just a fix. |
| 10 | No empty canvas for an ordinary Ask turn | **Met** (verified live, 8.1). |

Section 12's recommended implementation order (7 steps) is complete: steps 1-6
were closed before this pass and step 7 ("finish responsive sheet and
keyboard/focus behaviour") by items 24 and 26.

Section 3's own audit: 7 of 8 findings closed, finding 3 by item 19. Finding 8
(error language too close to raw runtime) is **still unverified** -- no failure
has been induced in the browser in any pass.

### 9.4 Implementation plan — phases and definition of finished

Phases 0-8 are substantially delivered and documented in the 2026-08-30
checkpoint. The remaining work is concentrated in three places.

- **Phase 9 (A2A) is not started**, exactly as the plan itself says it should
  not be. Confirmed today: no `a2a`, `agent_card` or agent-card route exists
  anywhere in the frontend plane or its gateway. The plan's own note is the
  right instruction -- Application-owned registry and Control authorization
  first, then the Rust SDK at the Model/Agent boundary. Definition-of-finished
  point 10 depends entirely on this.
- **Phase 10 is half delivered.** User-facing Trace is done (item 11, the
  5,000-event cap, the proof bundle, `effect_class`). The twelve evaluation
  suites are **not built**: `tests/e2e` holds 13 specs and 32 tests covering
  browser workspace, inbox/ticketing, spaces, cross-plane smoke and knowledge
  authority -- and not one of them drives `/chat`. The chat workspace's own
  coverage is 22 unit and component files, which cannot exercise suites 3, 4,
  8, 9 or 12 (upload and preview, browser return-after-navigation, reload after
  every event family, concurrent threads, keyboard/SR/zoom/mobile flows). This
  is the largest single gap in the plan and it is what the phase 4, 5, 10 and
  11 gates are all waiting on.
- **Phase 11 (staged release) has not begun.** Its gate needs replay parity,
  the accessibility target, and performance budgets on long threads.

Definition of finished, 14 points:

| # | Point | State |
|---|---|---|
| 1 | Calm, familiar chat | Done. |
| 2 | Sources only when evidence exists | Done (`available` / `claimsFocus`). |
| 3 | Work only on durable multi-step activity | Done (item 18 + item 27). |
| 4 | Correct native viewer for PDFs, files, tables, HTML | **Mostly.** Attachments cover image, PDF (iframe), sandboxed HTML, CSV table and text. A *generated* PDF artifact classifies as `binary` in `chat-artifacts.ts` -- a download, not a viewer. Worth closing if agents start emitting PDFs. |
| 5 | Browser sessions persistent, observable, safe to leave | Done (durable browser replay + cursor resume). |
| 6 | Trace explains work without raw reasoning | Done. |
| 7 | Ask/Do makes intent explicit | Done. |
| 8 | Durable, auditable approvals, effects, cancellations | Done (`CancelRun` receipt, `effect_class`). |
| 9 | Streams resume without loss or duplication | Done in code (id-deduped replay pages, per-run generation guards, resume cursors on both streams). **Not proven** -- that is evaluation suite 8. |
| 10 | A2A agents collaborate | **Not started** (phase 9). |
| 11 | Solid rendering meets performance targets under heavy streaming | **Unverified.** No performance measurement, budget or benchmark exists anywhere in the chat feature. There is no agreed stress profile to test against, which is a phase 0 artefact ("baseline measurements are reproducible") that was never produced. |
| 12 | Keyboard, screen reader, zoom, reduced motion, mobile | **Nearly.** Everything structural is in place after items 24 and 26; the accessible-name gap in 9.3 criterion 9 is the exception, and it is a screen-reader-only defect. |
| 13 | Tenant policy governs data, grounding, models, tools, agents, retention, effects | **Partly.** ZDR, grounding scope, privacy tier, effort and actions are all governed on the request path, and `TrustCenterSection` / `WorkspaceSettingsPage` expose tenant controls. Agents are not governable because phase 9 does not exist. |
| 14 | Legacy duplicate chat state and dead integration paths removed | **Open.** The two rows in 9.2 are exactly this point: an unreachable AG-UI client and seven unproxied Model Plane routes. |

### 9.5 What actually needs doing next

Ranked by what unblocks the most, with the reason each one is not already done:

1. **Chat-workspace e2e suites** (phase 10, evaluation suites 1-12). Four gates
   and three definition-of-finished points wait on this, and the code claims in
   point 9 are unprovable without it. Nothing blocks it.
2. **Accessible-name localisation** (criterion 9). Mechanical, plus a guard test
   so a new literal cannot slip in. Nothing blocks it.
3. **Resolve AG-UI one way or the other** (item 6, point 14). A decision, not a
   contract: reach the client from the app or delete it.
4. **A performance baseline and stress profile** (phase 0 artefact, point 11).
   Cannot be "met" until someone writes down the target.
5. **The three UI markers** -- claim-level span binding, source freshness,
   confidence/verification -- and the permission-scoped next actions that were
   meant to replace item 22's follow-up chips.

    *Corrected 2026-09-06, twice.* Two wrong claims were published about this
    item before it was measured properly. The first said all four markers need
    a new Model Plane contract. The second said the native chat path carries no
    claim field at all -- that came from a grep piped through `head -8`, which
    truncated two files before it reached `chat-client.ts`. What the code
    actually shows, checked on both sides of the wire:

    - **The client is already plumbed and waiting.** `Citation`
      (`chat-types.ts`) carries `claimId`, `sourceGroupId`, `start` and `end`,
      with a comment saying the gateway may omit them and the UI must never
      infer them from prose. `normalizeCitation` preserves all four, and all
      three transports read them -- the native `chat-client.ts`, the shared
      `verevon-ui-events.ts` adapter, and `ag-ui-client.ts`.
    - **No component renders any of them.** That is the whole UI gap.
    - **No producer emits them on a citation event.** Data Plane v2 owns claim
      identity (`GraphClaim` behind `GetClaims`), and model-gateway does carry
      `claim_id` at `retrieval_tools.rs` -- but inside a TOOL RESULT envelope
      addressed to the model, not a citation frame addressed to the browser.
      Character offsets do not exist anywhere in model-gateway.

    So this is blocked on a producer, not on a client contract, and the client
    half is done. Building the renderer now would add exactly the kind of
    wired-but-dead path section 2.3 exists to catalogue, so it stays unbuilt
    deliberately: the next step belongs to whoever emits a citation frame with
    a claim id and offsets. Source freshness and confidence/verification have
    neither a producer nor any client plumbing -- those two are genuinely
    contract-blocked.
6. **Message pinning UI** (pin one message into context; distinct from thread
   pins). Never addressed, no blocker recorded.
7. **The unproxied Model Plane routes** (2.3). Inventory only. Each needs a
   gateway domain and a use case; several may deserve deletion from the
   inventory instead.
8. **Phase 9 A2A backend contract spike**, in the order the plan prescribes.
9. **Induce a real failure and check the error language** (UX section 3
   finding 8). Never verified in any pass.

Open questions in section 6 stand as follows: Q3 (keyboard) and Q4 (Trace =
audit) are answered and built. Q1 (mobile) has its recommendation corroborated
and the sheet implemented, but no recorded decision. Q2 (Norwegian vocabulary)
is in use throughout without a written glossary -- criterion 9 is the visible
cost of that. Q5 (per-run cost) is now partly answered by accident: the
transcript's "Detaljer" panel carries Kostnad alongside Modell, Input, Output,
tid and Sikkerhet. Whether cost belongs anywhere more prominent is still open.
### 9.6 Fix pass on 9.5, 2026-09-06

Six of the nine items in 9.5 are closed, two are corrected to
contract-blocked, and one is left to its owner. Ranked as they were in 9.5:

**1. Chat-workspace e2e suites — written, registered, and blocked on a stack
credential.** `tests/e2e/chat-workspace.spec.ts` is nine tests covering
evaluation suite 1, most of suite 12, and acceptance criteria 1, 2, 3, 4, 8, 9
and 10, registered as a `chat` Playwright project against the dev stack the way
the `browser-workspace-*` family already is. It has not run: `local@verevon.dev`
and `e2e@verevon.dev` both answer 401, because `build-verevon-services.sh` now
defaults `SEED_DEV_ACCOUNT=0` and requires an operator-chosen
`SEED_DEV_PASSWORD`. Seeding it means creating an account and choosing a
password, which is the operator's call, so it was not done. To run:

```
SEED_DEV_ACCOUNT=1 SEED_DEV_PASSWORD=... bash "apps/Frontend Plane/verevonv3/build-verevon-services.sh"
pnpm exec playwright test --project=chat
```

Every selector and invariant the suite asserts was instead verified against the
live signed-in page, and doing so found three real defects in the spec that a
reading would not have: `/chat` rehydrates the last thread from `localStorage`,
so the "cold start" test was asserting an empty transcript on a four-message
thread (it now clears the five chat keys in an init script); the mobile resize
handle is `display: none`, still in the DOM, so `toHaveCount(0)` would have
failed against correct behaviour; and the reduced-motion test asserted zero
transition durations, which the CSS deliberately does not do — the reduce blocks
switch *animations* off and leave 140 ms hover transitions alone, so it now
checks the composer dock's `animation-name` from both sides. Live, the page
reports 1 chat page, 1 message list, 2 focusable messages, 0 canvases and
**0 tab strips** on a plain Ask thread, which is THE ONE RULE holding.

**2. Accessible-name localisation — done.** 28 names across five components now
route through `i18n.tr`; four components had no `i18n` at all. Guarded by
`chat-localization.test.ts`, which reads every `features/chat` `.tsx` and fails
on prose in `aria-label`, `aria-description`, `alt`, `placeholder` or `title`
that does not pass through `i18n.tr`. Verified live in both directions: at
`verevon.locale=en` the region is "Verevon chat workspace" and the whole chrome
reads English; at `no` it is "Verevon chat-arbeidsflate". The guard found six
sites a manual grep had missed, and its own first draft produced four false
positives by reading whole lines, so it now balances braces from the
attribute's own `{`.

*Not closed by this:* **visible text** has no automated guard. The artifact
viewer's kind labels were Norwegian-only prose (`'Fil'`, `'Kode'`, "N
versjoner") and are now bilingual, but others remain — the revision toggle's
"Vis endringer"/"Vis versjonen" among them, and several component tests assert
Norwegian strings, so a sweep has to move tests with it. A static guard for JSX
text nodes would drown in user content; the live e2e check in the spec above is
the better instrument once it can run.

**3. AG-UI — left to its owner, deliberately.** The 2026-09-02 owner decision
was to keep it as a planned surface. Reaching it means switching the app's
transport, which is not a call to make inside a fix pass, and deleting it
contradicts the decision. Recorded with one new fact from 9.5's correction:
`ag-ui-client.ts` is the only transport that projects `claim_id` onto a
citation, so whoever reaches it also gets the claim binding.

**4. Performance baseline — done, and it moved the conclusion.** Section 21 of
the implementation plan now carries the stress profile (40 deltas/second, a
20-paragraph answer with 12 citations, 8 tool calls, 4 artifacts and 4 files,
200-turn threads, the 5,000-event cap) and the budget it is judged against.
`derive.budget.test.ts` measures it and prints the numbers on every run: 0.45 ms
for 40 ticks on a heavy turn (441x headroom), 1.06 ms for a 200-turn mount
(944x), and 4x the answer length costing **1.0x** — the derivation is O(1) in
answer length. So the hot path is not where a streaming stall comes from, and
point 11's remaining risk is rendering and paint, which no unit test can see.
Point 11 is partly evidenced, not met. The first version of the scaling
assertion silently skipped itself, dividing two sub-resolution timings; it now
measures batches of 200.

**5. The three UI markers — corrected, see the amendment on 9.5 item 5.** Claim
binding is blocked on a producer with the client already plumbed; freshness and
confidence/verification have neither producer nor plumbing.

**6. Message pinning — corrected to contract-blocked.** 9.5 listed it as having
"no blocker recorded". There is one: no wire field for pinned message context
exists anywhere — not in `buildChatWireBody`, not in the gateway, not in the
Model Plane. A pin indicator would either change nothing about the next turn's
context, which is a lie about what the product does, or need a new
session-core/Model Plane contract first. Same reasoning as item 22's follow-up
chips.

**7. Unproxied Model Plane routes — still open, and still needs product
intent.** Seven routes, seven judgement calls about whether Verevon wants the
capability at all. Building gateway domains speculatively is how the 2.3
inventory got its entries.

**8. Phase 9 A2A — not started, as the plan prescribes.**

**9. Error language (UX section 3 finding 8) — still unverified.** Inducing a
real failure needs a stack fault to provoke, and the honest place to do it is
the e2e suite, which is blocked on item 1.

**Also closed in this pass, from the outstanding list rather than 9.5:**

- **The 13 long-standing chat test failures are gone**; `features/chat/components`
  is 152/152 across 9 files for the first time. All three causes were test-side:
  eleven were Solid 2 scheduling — a `fireEvent` followed by a synchronous
  `textContent` read observes the state before the update, and
  `CoreShell.test.tsx` had been importing `flush` for exactly this while these
  files never did; one asserted `srcdoc` was byte-identical to the artifact,
  pinning the behaviour the preview-isolation CSP wrapper deliberately replaced
  (it now asserts the isolation instead, which is what the test was for); and
  `mid-run-input.test.ts` searched a 2,813-CRLF file for LF anchors, so its
  guard could never match and reported the effect it protects as "gone".
- **Definition-of-finished point 4, the PDF half.** A generated `pdf` artifact
  rendered as a download card while an *attached* PDF previewed in an iframe —
  the same file, two answers. It now opens in a frame when its content is an
  addressable source (`data:application/pdf`, `blob:`, or an http(s) `.pdf`
  URL), and keeps the file card otherwise: bare base64 on a `pdf` artifact could
  equally be prose, and guessing would put an empty viewer where a working
  download was. A generic `file` kind carrying PDF bytes stays a download, which
  an existing test pins.

Verification for the pass: `pnpm typecheck` clean, `pnpm lint` 0 errors (121
pre-existing warnings), and per-directory `features/chat/components` 152/152,
`features/chat/lib` 133/133, `shared/chat-nodes` 48/48, `features/core` 57/57,
`features/dashboard` 70/70.
### 9.7 Message pinning: the blocker is gone, 2026-09-06

9.6 recorded message pinning as contract-blocked — no wire field for pinned
message context existed anywhere. The contract now exists, end to end, and the
UI is built on it.

**What a pin means mechanically.** Not "inject this text again": the client
sends IDS, and model-gateway re-expresses each resolved message as leading
`system` context. That position is the whole mechanism —
`compaction::plan_head_summary` (which collapses a long thread's head into a
summary at load time) and `compaction::drop_oldest_group` (which sheds the
oldest turns when a provider rejects the prompt for length) both begin at the
first NON-system message. So a pin means exactly one thing, and it is
falsifiable: *this message is not dropped when the conversation is shed for
length*. `DROPPED_HISTORY_NOTICE` already relied on that same guarantee, so
pinning reuses a property the code had rather than adding a second one.

The alternative — teaching both shedders to skip a set of indices — would have
to keep those indices correct across two `drain` calls and a `split_off`, in
two services that deploy separately (model-gateway and execution-core each
carry their own compaction by design). Hoisting does it once, structurally.

**Ids, never content.** A pin is a selector. The client names a message that
already exists in the durable thread; the server resolves it against what
session-core returns and ignores anything that matches nothing. Sending the
pinned TEXT would let a browser assert that the user said something earlier in
the conversation, which is the forgery this codebase forbids everywhere else.
A guard test asserts no pinned-content field can reach the wire.

**The layers, and what each needed.**

| Layer | Change |
|---|---|
| `sessions.proto` | `SessionMessage.message_id`. The durable conversation read returned role/content/agent_name/metadata and no identity at all, so there was literally nothing to name. The ids were always in the `messages` table; this read simply never selected them. |
| session-core | `SELECT m.id::text` and populate the field. |
| model-gateway | `pinned_message_ids` on the chat request; `compaction::hoist_pinned_messages`; ids carried alongside each loaded turn as far as pin resolution and no further — `ChatMessage` (what the provider sees) must not carry our identifiers. |
| model-gateway | `message_id` on the `/v1/threads/:id/messages` projection, so a client can learn the id it will later pin. |
| Frontend gateway | Relays the durable id as the turn's `id`, falling back to the old positional `canonical-N` for rows written before ids were selected. Also strips `pinned_message_ids` for support threads: a pin changes the prompt, and a support thread's history is customer-authored text this surface may only read. |
| SPA | `pinnedMessageIds` on the request, sent only when non-empty; `chat-pinned-messages.ts` for per-thread persistence; a Pin action and a "Festet i konteksten" indicator on each message. |

**Bounded on purpose.** `MAX_PINNED_MESSAGES = 5` and `MAX_PINNED_CHARS = 8000`
server-side, mirrored in the UI so the sixth pin is refused rather than
accepted and silently ignored. Without a cap the feature would crowd the live
conversation out of the context window through the one path built to prevent
exactly that. Over the cap, a message stays in the body where it was — droppable
like any other turn, which is honest, because it was not protected. An
oversized single pin is shortened with a notice rather than dropped.

**Deliberate limits, on record.**

- The pin list is per browser (`verevon.chat.pinnedMessages.v1`), not per user.
  Unlike THREAD pins (`ChatThreadSession.pinned`, server-owned), a message pin
  does not follow the user to another device. Making it durable needs a
  per-user pin contract in session-core; until that exists, promising
  cross-device pins would be a lie the storage cannot keep.
- A pin on a turn session-core has not persisted yet resolves to nothing and
  starts working once it has. The id simply does not match; nothing is invented.
- Editing a pinned message does not move the pin — the id is stable, so the pin
  follows the message, not the text.

**Verification.** Every layer is covered by its own suite: session-core and
model-gateway compile against the regenerated proto; `compaction` is 25/25,
including the two tests that prove the actual claim by composing the real
shedders — a pinned turn survives `plan_head_summary` + `apply_head_summary`,
and survives `drop_oldest_group` run to exhaustion, while an unpinned
neighbour does not. The frontend gateway is 460/460 with a new test pinning the
durable-id relay and its positional fallback. The SPA is 143/143 in
`features/chat/lib` (10 new), 152/152 in components, 48/48 in
`chat-client.test.ts` (2 new), typecheck clean, lint 0 errors. Verified live in
the browser: the Pin control renders, clicking it writes
`{"<threadId>":["msg-1"]}` and shows the indicator, the action flips to
"Løsne fra konteksten", and the composer then puts
`pinned_message_ids: ["msg-1"]` on the wire — and nothing else pin-shaped.

**The gap that remains.** The live check exercised the SPA against the
*running* containers, which still carry the pre-change model-gateway,
session-core and frontend-gateway binaries. So the id in that payload is the
old positional fallback, and the resolution half — a real durable id, hoisted
into protected context, surviving a compaction on a genuinely long thread — has
been proven by tests and not yet in the stack. Rebuilding three services' images
is the remaining step, and it is the one thing between this and "done in
production".

A note for whoever touches the controller: the pin state started as a
`createSignal` plus a `createEffect` reading `state.threadId`, and that effect
made TypeScript infer `state` as `never` throughout the *rest* of
`use-chat-controller.ts` — twenty-odd cascade errors with no obvious cause,
none of them at the effect. A `createMemo` over storage has no such effect and
is the better shape anyway: storage stays the single source of truth instead of
being mirrored into a second signal that can disagree with it.
