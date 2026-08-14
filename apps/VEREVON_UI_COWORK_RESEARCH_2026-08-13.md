# Verevon UI/UX & Cowork Research: x.ai/bot and block/buzz (2026-08-13)

> **Space UI update — 2026-08-14.** This addendum is the implementation
> direction for `src/features/spaces`. It narrows the research below into a
> desktop-first Space surface; it does not widen any plane's ownership or
> claim that unavailable projections already exist.

## Space UI direction: the calm control room

### The decision

Make a Space feel like a **shared room with a visible working pulse**, not a
settings page and not an agent command centre. The visual character is a warm,
high-contrast desk surface: one focused canvas, a quiet room rail, and a
compact right-hand pulse. It borrows Buzz's central idea — humans and agents
are first-class participants in the same room — while taking the useful part
of Grok's interaction model: a named agent should be understandable at a
glance and must visibly ask for a human when it cannot safely proceed.

The supplied reference images reinforce the same composition:

- a **persistent, scannable left rail** for rooms and people;
- a **large central conversation/work canvas** that earns most of the screen;
- a **compact contextual surface** rather than a permanently noisy dashboard;
- soft containers, hairline borders, and status that communicates with text
  and shape instead of colour alone.

This is deliberately not a clone of Buzz, Grok, Slack, or a generic dark
"agent cockpit." Verevon should retain its quiet neutral product palette and
use a small warm-orange signal only for action, human attention, and active
work. The memorable element is the *room pulse*: the Space header, agent
presence, and activity rows make it possible to see what is moving without
turning the page into a stream of opaque logs.

### Source notes and guardrails

| Input | What we adopt | What we do not copy |
|---|---|---|
| [Block Buzz](https://github.com/block/buzz) | Agents are room members; a room is the durable record of conversation, work, review, and evidence; activity is expressed as a readable action trajectory. | Buzz's protocol, relay, tenancy, identity, or permission model. Verevon keeps its existing plane boundaries and server-authoritative Space checks. |
| [xAI Grok Bot overview](https://docs.x.ai/grok-bot/overview), [bots](https://docs.x.ai/grok-bot/bots), [chat and collaboration](https://docs.x.ai/grok-bot/chat-and-collaboration), and [computer and apps](https://docs.x.ai/grok-bot/computer-and-apps) | A named assistant with a concise status, a clear continuation point, and an unmistakable request for human intervention. | Treating chat as a secret channel, or implying that a person can grant execution authority from the UI. Computer takeover remains a separate Quarry/Model capability. |
| Current xAI product documentation — [Grok workspaces](https://docs.x.ai/grok/user-guide) and [connectors](https://docs.x.ai/grok/connectors) | Keep collaboration and connected work legible in the room, with visible scoping. | Importing data-source or connector affordances before Verevon has a published Space projection for them. |

The supplied `x.ai/bot` and earlier `docs.x.ai/grok-bot/*` URLs are retained
as requested reference material. Their public contents were not reliably
retrievable in this review pass, so no unsupported product claims from them
are used as implementation requirements.

### The screen architecture

```text
┌ Room rail ──────────┬ Main canvas ────────────────────────┬ Room pulse ──────┐
│ space switcher      │ breadcrumb / room title / members   │ agent is working │
│ room facts          │ view strip: Chat · Work · …         │ active work      │
│ conversation list   │                                     │ needs attention  │
│                      │ selected view                       │ recent movement  │
│                      │                                     │                  │
│                      │ contextual composer / next action   │                  │
└──────────────────────┴─────────────────────────────────────┴──────────────────┘
```

At narrow widths the right pulse moves below the canvas and the room rail
becomes a horizontally scrolling summary. The page must remain fully useful
without a large-screen layout.

### Content rules for the first implementation

1. **The page header answers orientation in one scan.** Show the room's name,
   kind, the signed-in member's role, a lifecycle label, and the small member
   / active-work signals. Do not display internal references, revisions, or
   plane names in the normal path.
2. **Mount the existing six-view Space cockpit.** `Chat`, `Work`, `Knowledge`,
   `Activity`, `Agent`, and `Members` remain deep-linkable. A view with no
   published projection is an honest, designed placeholder that explains what
   it will contain; it must never masquerade as empty data.
3. **Make conversation the centre of gravity.** The Chat view offers the
   existing, Space-scoped chat entry point and a short thread list. It does
   not reimplement the chat composer or duplicate chat state.
4. **Use a human-readable activity grammar.** The pulse and Activity view
   use the existing verb / object / outcome rendering. Running and
   approval-waiting work is surfaced above completed work; rows link to their
   actual conversations.
5. **Name the agent's state, never simulate it.** A running thread can say
   “Verevon is working” and link to its conversation. With no active run,
   say so. Do not invent members, agent capabilities, files, or approvals
   until their owner plane publishes a Space projection.
6. **Keep destructive controls out of the working rhythm.** Personal Space
   deletion remains available but is visually isolated in the Members view.
   Its authorization/purge distinction remains exactly as implemented.

### Accessibility and interaction bar

- Preserve the tablist's roving keyboard behavior and URL hash deep links.
- Keep labels as text; icons only support them. Status must have a text
  equivalent and sufficient contrast in both themes.
- Maintain the membership revalidation and fail-closed state. A redesign must
  not leave a previously authorized room visible after a failed recheck.
- Honour `prefers-reduced-motion`; the active pulse may be calm but cannot be
  the only indicator that work is ongoing.
- All links to chat retain the existing Space/thread query parameters. The UI
  is a projection over data, never an authority grant.

### First implementation scope and explicit non-goals

**In scope:** mount and polish the existing cockpit; add a room rail, a
conversation-forward Chat view, an Activity view, a truthful Agent view, a
membership view, responsive layout, and focused behavior tests.

**Out of scope:** a new API endpoint, new Space membership actions, a real
member roster, a new agent runtime, task teaching, a computer takeover,
cross-plane delivery receipts, or simulated data. Those need their own
owner-plane projections and contracts before they belong here.

---

## 0. Scope, method, and confidence

This document researches two external products for **UI/UX and collaborative
("cowork") working style only** — not backend architecture — and recommends
what Verevon v3 should adopt. It was produced in an isolated worktree and does
not change any product code.

Read first, and treated as binding context rather than raw material to
re-derive:

- `apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md` — headline finding: Verevon's
  recurring failure mode is features **built but never wired to a UI**.
- `apps/Frontend Plane/verevonv3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md`
  — the **Verevon Space** program (sequences S0–S1 underway). Every
  recommendation below is checked against this plan and tagged for
  compatibility with it.

**Confidence is asymmetric between the two sources, and that asymmetry matters:**

- **block/buzz** — high confidence. Primary sources only: the repo's
  `README.md`, `VISION.md`, `VISION_PROJECTS.md`, `VISION_ACTIVITY.md`,
  `VISION_REMOTE_AGENTS.md`, its live `Status` table, and its file/crate tree,
  fetched directly via `gh api`. Nothing below about Buzz is inferred from
  secondary reporting.
- **x.ai/bot ("Grok Bot")** — secondary-sourced, corroborated, but not
  primary. `https://x.ai/bot` returned **HTTP 403** on a direct fetch and
  again via `web.archive.org` — the live marketing page could not be read
  directly, at all, by this research pass. Everything below about Grok Bot
  comes from three independent industry-analysis articles published within
  1–2 days of its August 11, 2026 launch, which themselves quote the primary
  marketing page and the `docs.x.ai/grok-bot/*` documentation pages verbatim
  in multiple places, and in one case cross-check pricing claims against
  Cursor's own structured pricing data:
  - [Unite.AI — "xAI Launches Grok Bot..."](https://www.unite.ai/xai-launches-grok-bot-always-on-ai-teammates-with-their-own-cloud-computers/)
  - [Digital Applied — "Grok Bot: xAI's AI Teammates Get Their Own Shared Computer"](https://www.digitalapplied.com/blog/grok-bot-ai-teammates-launch-cloud-computer-2026)
  - [AYAutomate — "Grok Bot Explained"](https://www.ayautomate.com/blog/grok-bot-xai-ai-agents-explained)

  Treat facts below attributed to direct quotes as solid. Treat the "hands-on
  walkthrough" screen descriptions (mobile app roster, teach-a-task recording)
  as one reviewer's account relayed second-hand, not something this pass
  verified with its own eyes. One thing worth flagging explicitly: an early
  automated web search summarized this launch as a joint **"SpaceXAI"**
  product — that company does not exist. It conflated two separate,
  real facts: xAI (owned substantially by the same people as SpaceX) shipped
  Grok Bot, and **SpaceX separately agreed in June 2026 to acquire Cursor's
  maker, Anysphere, for ~$60B** — a deal still pending regulatory approval at
  launch time. The two companies are legally separate today even though Grok
  Bot's auth, billing, and desktop distribution run through Cursor's stack.
  This is called out because it is exactly the kind of confident-sounding,
  wrong synthesis this document's own standards require catching.

Verevon's current state below was verified directly against source — via
CodeGraph, `gh`, and direct file reads — not against memory or the prior
docs' claims. Section-by-section citations name real files. Where a claim in
one of the two prior docs turned out to be stale, that is called out in the
Appendix rather than silently corrected.

---

## 1. What each source actually is

### 1.1 x.ai/bot — "Grok Bot"

A beta product xAI shipped on **August 11, 2026**: "AI teammates you can give
real work to." Each named **Bot** is assigned a role (browser agent, LinkedIn
outreach agent, research agent, email assistant, scripting agent, etc.), gets
a persistent cloud computer with a real browser/filesystem/terminal, signs
into the tools and sites a user already relies on, and keeps working after
the human closes the app — reporting back and "only com[ing] back when
something needs your approval."

**Distribution is not what it looks like.** Grok Bot is not sold standalone.
Access is bundled into subscription tiers spanning **two separate companies**:
SuperGrok Heavy (xAI's own top plan), Cursor Ultra ($200/month, the one price
point independently corroborated by both vendors' own pages), and a tier
xAI's page calls "Cursor Premium Teams" ($120/seat) that **does not appear at
all** in Cursor's own structured pricing data (which shows a plain "Teams" at
$40/seat) — a real three-way name/price discrepancy across the vendor's own
surfaces, not a rounding difference. Authentication, account data settings,
training opt-out, and retention/deletion "follow the applicable Cursor
[terms]," and the desktop installer is served from a `downloads.cursor.com`
URL — because distribution, billing, and identity for this xAI-branded
product run through Cursor's stack. **Which model(s) power Grok Bot is not
disclosed anywhere** — not on the launch page, not across four checked
`docs.x.ai` pages.

**The interaction model** is a messenger-style contacts list of named Bots —
"a roster of coworkers you assign work to" — each with an ordinary chat
surface plus a live view of its cloud computer. Two ways a Bot acquires a new
capability:

1. **Teach a task** — click "teach a task," the Bot enters a recording state
   on its own screen, the human performs the workflow once (open this site,
   click here, fill this field), and the Bot generalizes the recording into a
   repeatable skill. Paired with a **routines panel** for scheduled re-runs.
   Program-by-demonstration instead of program-by-description.
2. **Plugin/OAuth connection** — when a clean integration already exists
   (the reviewed walkthrough's example: Gmail via an MCP plugin), the Bot
   uses that instead of driving a browser.

Full **desktop/mobile parity**, including the live "watch the Bot work" view
and the ability to take over a Bot's session from a phone. For secrets
specifically — passwords, passkeys, 2FA codes, CAPTCHAs, payment
confirmations — the Bot **hands control of the computer to the human**, who
completes the sensitive step and hands control back; the documentation states
outright, "do not send a password or one-time code in ordinary chat."

**The one fact worth treating as a real red flag, not marketing spin:**
xAI's own documentation states — verbatim, on two separate pages (FAQ and
the approvals-and-security page) — *"Do not use separate Bots as a security
boundary."* Every Bot on an account shares **one** persistent cloud
computer/identity surface; the "Bots have their own computer" language on the
marketing page is directly contradicted by the product's own docs. There is
no visible multi-**human** collaboration model in what was found anywhere —
Grok Bot is one account operating many single-owner "employees," not several
people and an agent sharing a room.

### 1.2 github.com/block/buzz — "Buzz"

A real, actively developed, Apache-2.0 open-source repo from Block, Inc.
(Square/Cash App's parent) — ~27,000 stars, created March 2026, pushed as
recently as today. Tagline: **"A workspace where humans and agents build
together, on a relay you own."** Desktop client is Tauri + React + Vite +
Tailwind + shadcn/ui; backend is a Rust workspace (`buzz-relay` on Axum,
Postgres for events + full-text search, Redis for pub/sub, S3/MinIO for
media). Every message, reaction, workflow step, review approval, and git
event is a cryptographically signed **Nostr event** in one append-only log —
same shape whether the author is a human or a process. A **community** (the
workspace reachable at one relay URL) is the tenancy boundary.

**The core proposition:** agents are room **members**, not bots bolted onto
the side. You add an agent to a channel exactly like you add a person; it
gets its own keypair, its own channel memberships, and the same audit trail
as everyone else. From inside a room, an agent can open repos, send patches,
review code, run YAML workflows, edit shared documents, orchestrate other
agents, join voice huddles, and create new channels.

Three genuinely distinctive interaction patterns, all verified against the
vision docs directly:

- **Branches as channels.** Creating a feature branch spins up a channel;
  that branch's patches (NIP-34 git events), CI results, review comments, and
  merge decision all accumulate there. The channel archives permanently on
  merge as "the permanent record of why that code exists." One thread, one
  search index — no separate issue tracker / CI dashboard / chat / review
  tool to reconcile.
- **Canvases.** "Living documents, collaboratively editable by humans and
  agents via MCP tools. Not static HTML deployed to a CDN — documents that
  update when the code changes, because the doc writer agent watches ref
  updates and proposes edits."
- **The agent activity feed.** A purpose-designed rendering discipline (full
  detail in §2 below) built around one question: how do you let a human
  *supervise* an agent — trust the routine, catch the one thing that needs
  them — without reading a raw transcript?

**Identity/continuity model:** an agent's identity (keypair, name, durable
history, reputation) lives entirely on the relay, decoupled from whatever
machine happens to run its process (its "body"). An agent can start a task
on a laptop, work through the evening, shut itself down when idle, and be
resumed the next morning on different hardware under the same identity — with
an explicit design axiom that **the desktop retains no control channel into
the remote substrate after deploy**. Steering a running agent means talking
to it on the relay — the same channel a human would use — not an admin/ops
panel; presence for a remote agent means "available for conversation," not
infrastructure telemetry.

**Buzz is honest about what isn't done**, and one gap is directly relevant to
Verevon's own headline finding: its own status table admits **"Workflow
approval gates — infrastructure exists (DB, API, UI); executor doesn't
persist/resume [ticket WF-08]."** Buzz independently rediscovered the exact
"built but never wired" failure shape that Verevon's own QM-inspired plan
names as its headline pattern. That is worth taking as mild reassurance
rather than an indictment of Verevon specifically — it looks like a common
failure mode of any system this ambitious about wiring everything into one
substrate, not a Verevon-only pathology. Also explicitly unfinished: the
mobile client (Flutter, "in active development"), remote-agent deployment to
Kubernetes ("spec in review"), and web-of-trust reputation / push
notifications / "culture features" (explicitly labeled "strong opinions,
pending code" — do not plan around them).

---

## 2. Interaction/UI patterns worth stealing

| # | Pattern | Source | Effort |
|---|---|---|---|
| 1 | Verb/Object/Outcome activity grammar + render-class taxonomy | Buzz | M |
| 2 | Human-takeover of a live agent session for secrets/2FA/CAPTCHA | Grok Bot | S–M |
| 3 | Teach-by-demonstration + routines for browser automation | Grok Bot | L |
| 4 | Unit-of-work-as-the-record ("branch as room") discipline | Buzz | folds into existing roadmap |
| 5 | Generalize the existing draft-lease pattern instead of inventing presence | Buzz (by contrast) + Verevon's own code | M |

### 2.1 Verb/Object/Outcome activity grammar (Buzz)

**What it is.** Every event in Buzz's agent activity feed resolves to one of
twelve fixed presentation classes, grouped by how often they're read and how
much consequence they carry: **the spine** (Message, relay operation,
File-edit, Shell command, Tool-status/turn-lifecycle — "if these are unclear,
the feed has failed"), **high-value context** (Thought, Plan/Todo, Permission,
Error), and an **ambient safety net** (Generic tool, Raw rail, Suppressed
noise). Every item is rendered as a sentence — *"the agent did [verb] to
[object] → [outcome]"* — with supporting detail (full args, raw output, the
unabridged diff) pushed into progressive disclosure. The governing design
rules, quoted directly because they're specific enough to implement from:

- *"Semantics over transport"* — render what the agent did, not which API it
  used.
- *"Mutate in place"* — a running action updates its own row from pending →
  executing → done, never a trail of duplicate status lines.
- *"Never go dark"* — silence, idle, and timeout are rendered states
  ("waiting…", "timed out"), never an empty void.
- *"Failures rise; reads recede"* — salience tracks consequence, not
  frequency.
- *"Resolve references"* — show `#design`, a name, a filename — never a raw
  event id or hash.
- *"Honesty over guessing"* — an unrecognized event degrades to a clean,
  truthful generic row; never fabricate semantic richness to look complete.

**Why it works.** It converts "trust a process you cannot see" into a
skimmable trajectory, organized around three questions a supervisor actually
asks: comprehension, confidence, control.

**In Verevon.** This is a rendering discipline to apply on top of data
Verevon mostly already has, not a new data model. `AgentRunConsole.tsx`
(`src/features/agents/components/AgentRunConsole.tsx`, routed at
`/agents/runs`) already exposes a `ProofApproval` shape
(`src/shared/api/run-console-client.ts`) with `approvalId`, `status`,
`requestedBy`/`decidedBy`, `decisionReason`, and a nullable `execution` —
real evidence/approval data, just not organized around a fixed render-class
taxonomy today. The adoption plan already earmarks reusing "its timeline,
proof, approval, and cost panels" for the Space **Activity** tab — this
pattern is the concrete rendering grammar to apply when that extraction
happens.

```tsx
// Illustrative sketch — SolidJS, no Tailwind, semantic classes.
// A render-class dispatcher over Verevon's existing run/approval event stream.
import { For, Match, Switch } from 'solid-js'

type RunEventClass =
  | 'message' | 'relay_op' | 'file_edit' | 'shell' | 'tool_status'
  | 'thought' | 'plan' | 'permission' | 'error'
  | 'generic' | 'raw' | 'suppressed'

interface RunEvent {
  id: string
  eventClass: RunEventClass
  verb: string
  object: string
  outcome: 'pending' | 'ok' | 'failed' | 'waiting'
}

function ActivityRow(props: { event: RunEvent }) {
  return (
    <li class="verevon-activity-row" data-outcome={props.event.outcome} data-class={props.event.eventClass}>
      <Switch fallback={<span class="verevon-activity-row__generic">{props.event.verb} {props.event.object}</span>}>
        <Match when={props.event.eventClass === 'error'}>
          <strong class="verevon-activity-row__error">{props.event.verb} {props.event.object} failed</strong>
        </Match>
        <Match when={props.event.eventClass === 'permission'}>
          <span class="verevon-activity-row__gate">Waiting on approval: {props.event.object}</span>
        </Match>
      </Switch>
    </li>
  )
}

export function ActivityFeed(props: { events: () => RunEvent[] }) {
  return (
    <ul class="verevon-activity-feed">
      <For each={props.events()}>{(event) => <ActivityRow event={event} />}</For>
    </ul>
  )
}
```

Space-compatible — works today on `AgentRunConsole` regardless of Space
timing, and is the plan's own proposed mechanism for Space Activity.

### 2.2 Human-takeover of a live agent session (Grok Bot, adapted)

**What it is.** When a browser-driving agent hits a password, passkey, 2FA
code, CAPTCHA, or payment confirmation, control of the live session view
hands to the human; they complete the sensitive step and hand control back.
Explicit rule: never move a secret through the ordinary chat channel.

**Why it works.** It solves "the agent needs my password" without the agent
ever touching the secret, and without breaking the task into stop-do-it-manually-restart.

**In Verevon.** The foundation already exists and is source-verified as a
Verevon strength, not a gap: per the adoption plan's own capability table,
"Quarry owns execution/evidence; Model proposes; grants and live-view
concepts exist" for browser/computer-use — its explicit recommendation is to
"keep the Quarry boundary and add human takeover separately." That is exactly
this pattern. The missing piece is narrow: the take-over/hand-back
interaction itself and the UI copy enforcing "never type a secret into chat"
— an additive feature on Quarry-v2's existing live-view, not new
infrastructure. Space-neutral (a Model/Ingestion-plane boundary concern,
orthogonal to Space scope).

### 2.3 Teach-by-demonstration + routines (Grok Bot)

**What it is.** Record a browser session once; the agent generalizes it into
a repeatable skill. Pair with a routines panel for scheduled re-runs.

**Why it works.** The bottleneck in workflow automation is rarely the
technology — it's the cost of specifying a workflow precisely enough for
software to execute reliably. A demonstration removes most of that
specification cost.

**In Verevon.** This is genuinely new capability, not a UI skin, and it must
be built to Verevon's own standard rather than Grok Bot's: every
recorded/generalized run should produce the same evidence/approval trail as
a live one — "learned" is not a license to skip the ledger. It sits on
Quarry-v2's existing browser execution/evidence boundary (per
`CLAUDE.md`: "Model may propose browser actions; Quarry-v2 executes or
rejects them") and should feed the existing scheduling surface —
`CronSchedulesSection.tsx` (`src/features/settings/components/`) plus the
QM-inspired plan's own AUTO-1 (wire `workflow_type` into the cron creation
form) and AUTO-2 (background-job "notify me" path) recommendations, which
are cheaper prerequisites for the same "routines" idea. Effort: L. Space-
compatible — ships standalone, natural long-term home is the Space **Work**
tab the adoption plan already specifies.

### 2.4 Unit-of-work-as-the-record ("branch as room") (Buzz)

**What it is.** The unit of work (a feature branch) automatically gets a
channel; every related event — patches, CI, review, merge — accumulates
there, and it archives permanently on completion as the record of "why this
exists." One thread, one search index, no tab-switching between an issue
tracker, a CI dashboard, and a chat log that all claim to describe the same
thing.

**Why it works.** It removes the reconciliation tax between four systems
that are nominally about the same unit of work but drift the moment two of
them disagree about state.

**In Verevon.** Verevon is not a software forge, so the direct pattern
doesn't transplant — but the underlying design principle transplants
cleanly onto the adoption plan's own **Support Case Space**, already its
recommended second vertical slice after Personal Space. The principle to
hold that work to: the case's Space **Activity** tab must be the literal,
archived record — the conversation, the ticket-state changes, the agent's
actions/approvals, and the resolution living in one timeline — not a ticket
record in Zammad/`conversation-core` plus a separate chat plus a separate
audit log that a UI merely displays side by side. This is not new scope; it's
a design bar for work already planned (S1.6 / "Support Case Space" in the
adoption plan). Space-dependent — the pattern requires Space's case scope to
exist at all.

### 2.5 Generalize the draft-lease pattern instead of inventing presence (Buzz, by contrast — and Verevon's own code)

**What it is.** Buzz treats presence (online/typing) as core, table-stakes
infrastructure for a shared room. Verevon does not have that — but it does
have something better-scoped and already working: `ConversationPanel.tsx`
(`src/features/inbox/components/`) implements a real draft-lease mechanism —
`getDraftLease`/`refreshDraftPresence`, polled every 15 seconds, with
`409`/`conflict` handling and a Norwegian/English "Another teammate is
composing a reply" indicator (`verevon-inbox-composer__draft-status--presence`
in `src/styles/global.css`) — that prevents two people from drafting a reply
to the same external conversation at once. This is a genuinely good,
narrowly-scoped conflict-avoidance primitive, not a toy.

**Why it matters as a "steal."** Rather than importing Buzz's generic
presence model wholesale, the higher-leverage move is recognizing Verevon
already built the right kind of primitive for one surface and generalizing
*it* — a scoped, conflict-avoidance lease, not an ambient "who's online" dot
— to other Space-shared surfaces (a shared document, a shared knowledge
edit) as they come online. See §5 for the closely-related, and cheaper,
question of what to do with the *dormant* general-purpose `presence` schema
that already exists unused (Appendix has the full finding). Space-dependent
— generalizing it requires Space-scoped shared resources to generalize to.

---

## 3. Cowork/collaboration model

| Dimension | Buzz | Grok Bot | Verevon today | Verevon Space (planned) |
|---|---|---|---|---|
| **Presence** | First-class: online/typing via NIP-29-based channel features, huddle lifecycle events | Not addressed for humans; the only multi-actor signal is one human taking over one Bot's screen | Schema exists (`presence`, `conversationPresence` in Convex: online/typing/away/offline, indexed) but **zero** query/mutation functions and no frontend reader anywhere in `src/`; the one real live signal is Inbox's unrelated draft-lease indicator | Not yet specified as a data model in the adoption plan; a **Members** tab lists roster/roles, not live presence |
| **Shared context / room** | The channel *is* the room — conversations, files, workflows, canvases, and git events all attach to one scope | None — a Bot is per-account, not shared between multiple humans; docs state explicitly bots on one account share one machine with **no security boundary** between them | No shared-room concept; chat has zero `participant` references anywhere in `src/features/chat/` (verified directly) | `SpaceRef` (`personal \| room \| project \| case`) is exactly this gap's proposed fix — the plan's own P0 |
| **Who-sees-what** | Channel membership (public/private); authorization internals not reviewed in this pass (out of scope — UI/UX only) | N/A — single account, single roster, explicitly not isolated bot-to-bot | Org-level (`activeOrg`) is the only real authority boundary today; `Data` has `workspace_id` filters not tied to a signed decision at chat/action time | `effective_access = SpaceAccessDecision ∩ recipient-audience-authorization ∩ owner-plane resource decision` — a materially stricter model than anything demonstrated by either external source |
| **Interruption / steering** | Message the agent in its channel, ordinary chat; approval-gate *workflow* steps exist end-to-end in schema/API/UI but the executor doesn't persist/resume them yet (Buzz's own admitted WF-08 gap) | The Bot "only comes back when something needs approval"; otherwise runs unsupervised | Runs/approvals are source-verified strong (`AgentRunConsole`, `ProofApproval`) but durable approval **continuation** is not release-proven per the adoption plan's own audit | Same approval/evidence machinery stays authoritative by design ("What Verevon must not regress" #4); Space adds the missing scope layer around it, doesn't replace it |
| **Handoff / continuity** | Agent identity (keypair, history, reputation) lives on the relay; the compute "body" is disposable and swappable across devices/substrates; steering is always a relay message, never a substrate control channel | Full desktop/mobile parity including live-view takeover from a phone — but this is continuity of **one human's** control of **one Bot**, not multi-human handoff | None — session/thread state is browser/session-store bound (`src/shared/session/session-store.ts`) | Not yet specified — cross-surface continuity is explicitly deferred (P2, "web first... Slack only after ownership is ratified") and agent-body portability isn't addressed by either internal document. Worth naming as an open question neither source resolves. |

**The headline comparison:** Buzz's cowork model is genuinely multiplayer by
default — a room assumes several humans and possibly several agents from the
start. Grok Bot's is not a cowork model at all in that sense — it's one
human operating several single-owner agents that explicitly must not be
trusted as isolated from each other. Verevon today sits closer to Grok Bot's
shape than Buzz's (single-user-per-thread, org-level-only sharing) but with
one real, working, narrowly-scoped exception — Inbox's draft-lease — that
already proves Verevon's engineering instinct for "shared work, avoid
collision" is sound where it's been applied. The Space plan's authorization
model, on paper, is already stricter and more correct than either external
source's demonstrated design; the gap is that almost none of Space's
UI is built yet (see Appendix: `SpacePage.tsx` is real but its Activity
section literally says "will appear here" and its Members section shows only
the current viewer's own role, not a roster).

---

## 4. What NOT to adopt

1. **Grok Bot's shared-computer / no-security-boundary-between-agents
   model.** Directly opposite to Verevon's approval/evidence/EU-residency
   positioning and to the Space plan's own resource-authorization-
   intersection principle. A single undifferentiated compute/identity
   surface per account, with the vendor's own docs warning "do not use
   separate Bots as a security boundary," is close to the inverse of what
   Verevon sells.
2. **Grok Bot's murky, cross-vendor data governance.** Whose privacy policy
   applies, whose retention/deletion terms govern, and which company's
   billing/identity stack you're actually trusting are three different
   answers depending on which page you read. Verevon's positioning is
   explicit EU residency and evidence; importing an arrangement where "which
   vendor's terms actually apply" is itself unclear is a non-starter, not a
   detail to clean up later.
3. **"Just drive the UI like a human" as a primary strategy, not a
   fallback.** Grok Bot leads with browser-driving over API integration.
   Verevon/CoreSystem's architecture inverts that priority on purpose — a
   typed action catalog through owner-plane contracts is the primary path,
   and Quarry's browser execution is explicitly the fallback for services
   with no clean integration (`CLAUDE.md`: provider actions go through
   integration-corev2's actions surface; the frozen contract is
   `docs/actions-surface-operations.md`). Adopting §2.3 (teach-by-
   demonstration) does not mean inverting this priority — it's additive to
   the fallback path, not a replacement for the catalog.
4. **Buzz's casual-social affordances as approval mechanisms.** Buzz's own
   README describes a release shipping because an agent "gets a 👍 reaction"
   — emoji reactions standing in for a merge/ship decision. Verevon already
   has a real signed-decision approval model (`ProofApproval`,
   `SpaceAccessDecision`); downgrading it to a reaction for the sake of
   feeling lightweight would be a regression dressed up as simplification.
5. **Buzz's default-visibility assumption for shared documents.** Buzz's
   canvases are, by the vision docs' own description, editable by anyone
   with channel access. The adoption plan is explicit that "Space membership
   is necessary but never sufficient" for a linked private resource — any
   Verevon canvas-like feature must run through the same
   resource-authorization intersection as everything else, not inherit a
   simpler "room membership is enough" default.
6. **Voice huddles and "culture features."** Real-time voice, and Buzz's own
   speculative "culture features" bucket, assume a communications-platform
   context Verevon does not have and is not trying to become. Out of scope
   for an approval/evidence-first enterprise tool; no Verevon surface or
   plane owns realtime audio today, and building one is a different product
   bet than anything in this document.
7. **Cross-surface (Slack-style) continuity or ambient/always-on background
   agents as an early move.** Both prior internal documents already warn
   against this sequencing, and this research doesn't surface a reason to
   override that: building Slack continuity, ambient presence, or a
   swappable coding harness before the Space contract lands would reproduce
   today's fragmentation on a new surface. Channel Plane remains
   docs-only — a Slack-shaped continuity story needs an ownership/contract
   ADR before any code, per `CLAUDE.md` and the adoption plan's own guardrail
   #8.
8. **Nostr/self-hosted-relay framing at the positioning level.** Backend
   protocol choice is out of scope for this document by design, but it's
   worth flagging that some of Buzz's marketing language — "a relay you
   own," "sovereign" — could tempt a "let's be decentralized too" instinct.
   Verevon's actual buyers want a vendor who owns uptime, compliance, and
   data residency guarantees, not a relay to self-host themselves; don't let
   an appealing OSS narrative pull positioning toward self-hosting.

---

## 5. Prioritized shortlist

| # | Item | Source | Effort | Impact | Space tag |
|---|---|---|---|---|---|
| 1 | Verb/Object/Outcome render-class grammar for `AgentRunConsole` → Space Activity | Buzz | M | High | Space-compatible |
| 2 | Human-takeover handoff in Quarry's existing live-view, for secrets/2FA/CAPTCHA/payment steps | Grok Bot (adapted) | S–M | Medium–High | Space-neutral |
| 3 | Teach-by-demonstration + routines for Quarry browser automation | Grok Bot (adapted) | L | High | Space-compatible |
| 4 | Hold Support Case Space to the "branch as room" bar: Activity tab is the literal record, not a parallel log | Buzz (principle) | marginal (already scoped) | High | Space-dependent |
| 5 | Generalize the existing, working Inbox draft-lease pattern to other Space-shared surfaces | Buzz (by contrast) + Verevon's own code | M | Medium | Space-dependent |
| 6 | Decide the fate of the dormant `presence`/`conversationPresence` Convex schema — wire a minimal viewer indicator or remove it | Buzz (by contrast) | S | Low–Medium | Space-neutral today, likely Space-compatible later |
| 7 | Treat the Space cockpit (`/spaces/:spaceId` unifying chat/work/knowledge/activity/agent/members) as externally validated, not just internally proposed | Buzz + Grok Bot (convergent) | already scoped | High | Space-dependent |

Notes on tagging: **Space-compatible** = useful with or without Space, and
fits cleanly once Space exists. **Space-dependent** = only makes sense once
Space's scope/authority model exists. **Space-neutral** = orthogonal to
Space either way.

Item 7 is not a new recommendation — it is the adoption plan's own P0
(`/spaces/:spaceId`, reusing existing chat/knowledge/run-timeline/approval/
evidence/cost/inbox/settings surfaces). It's included because both external
sources, independently and for different reasons, converge on the identical
shape: Buzz's channel-contains-everything room, and Grok Bot's
roster-plus-live-view per Bot, are the same underlying idea applied at
different scopes (a room vs. a single agent). That convergence is evidence
the plan's shape is right, not a reason to change it.

---

## Appendix: verification notes and corrections to the two prior documents

Per this document's own standard ("verify claims — this codebase has
repeatedly turned out to already have the thing, just unwired"), the
following were checked directly against current code rather than taken from
either prior document or from memory:

- **`SpacePage.tsx` is real but genuinely minimal.**
  (`src/features/spaces/components/SpacePage.tsx`, routed at
  `/spaces/:spaceId` in `src/app/App.tsx:149`.) It resolves membership via
  `getSpaceMembership`/`listSpaces` (`src/shared/api/spaces-client.ts`),
  shows a Space switcher, and links to Chat/Members/Activity anchors on the
  same page (not separate routes yet, contrary to the nav structure the
  adoption plan specifies at `/spaces/:spaceId/chat`, `/work`, etc.). Its
  Members section literally renders only "You are currently confirmed as
  `<role>`" — no roster. Its Activity section's entire content is the literal
  string "Run activity will appear here as owner-plane receipts are
  correlated into the Space timeline." This matches the adoption plan's own
  tracker marking S1.6 incomplete — consistent, not a contradiction.

- **Convex backend Space work is further along than the frontend shell
  suggests.** `apps/Application Plane/convex-core/convex/` has real
  `spaces.ts`, `spaceLifecycle.ts`, `spaceRegistration.ts`,
  `membershipProjection.ts`, and `authorityProjection.ts` files — consistent
  with the plan's own [x]-marked sub-items under S1.3 (e.g., `CreateThreadRequest`
  now carries a signed `space_id`/decision reference, verified in Session
  Core). Worth knowing this isn't purely aspirational; the projection layer
  genuinely exists.

- **A dormant `presence` data model was found that neither prior document
  mentions.** `apps/Application Plane/convex-core/convex/schema.ts` defines
  two presence tables — a general `presence` table (line ~218: `userId`,
  `conversationId`, `status: online|typing|away|offline`, `lastSeenAt`,
  device/userAgent metadata, indexed by user/conversation/status) and a
  `conversationPresence` table (line ~683, an external-user variant with the
  same status enum). **Neither has a single query or mutation function
  anywhere in `convex-core/convex/*.ts`**, and grepping the entire frontend
  (`src/`) and the gateway (`apps/gateway/src/`) for "presence" turns up
  exactly one hit — the *unrelated* draft-lease timer in
  `ConversationPanel.tsx` (§2.5). This is a clean, verified instance of
  Verevon's own headline failure pattern (schema built, zero callers, zero
  UI) that this research surfaced independently while comparing against
  Buzz's presence-is-core model — not previously flagged in either prior
  document. See shortlist item 6.

- **A likely-stale claim in the QM-inspired plan.** That document's ADM-1
  recommendation states "the admin form to actually set a [quota] limit
  doesn't exist." Direct inspection of
  `src/features/settings/components/OrgQuotasSection.tsx` (317 lines, wired
  into `WorkspaceSettingsPage.tsx`) found a real, working form: per-field
  `handleSave` handler, `disabled={savingKey() === field.key}` loading
  state, and quota-field/reset-period definitions — not a stub or read-only
  display. Both documents are dated the same day this one was produced;
  this is most likely a very recent close-out (possibly landed in a
  parallel workstream) that the QM-inspired plan's research pass predates,
  rather than an error in that pass. **Recommend a quick, cheap
  verification against the live ADM-1 backlog item before anyone re-does
  this work** — it may simply need to be marked done.

- **Trust Center is a Settings section, not a standalone in-app surface.**
  Two different, differently-implemented components share the name: a
  public marketing-site `TrustCenter.tsx`
  (`apps/verevon-web/src/components/trust/TrustCenter.tsx`, at the public
  `/trust` route) and an authenticated-app `TrustCenterSection.tsx`
  (`src/features/settings/components/`) that is imported into, and rendered
  as one section of, `WorkspaceSettingsPage.tsx`. There is no standalone
  `/trust`-equivalent route inside the authenticated V3 app today — worth
  knowing precisely if "trust center" is ever discussed as if it were a
  top-level nav item, because it isn't one.

- **Chat has no multi-participant concept in code today.** Grepping
  `src/features/chat/` for `participant` returns zero matches, directly
  confirming both this document's cowork comparison (§3) and the adoption
  plan's own claim that chat is user/org-scoped with no room identity.

- **Application Plane's actual service roster** (verified via direct
  directory listing, superseding a truncated earlier tool result):
  `conversation-core`, `convex-core`, `information-core`, `insight-core`,
  `leads-core`, `notification-core`, `social-core`, `studio-core`. No
  dedicated presence/realtime service exists — what realtime capability
  exists is Convex's inherent reactive-query plumbing, not a purpose-built
  presence feature, which is consistent with the dormant-schema finding
  above.
