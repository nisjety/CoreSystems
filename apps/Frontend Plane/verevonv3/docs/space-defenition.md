# Space Definition

**Updated:** 2026-08-16
**Status:** Product model. Companion to `SPACE_AGENT_SCOPE_PLAN_2026-08-14.md`
(implementation plan) and `SPACE_COCKPIT_WIRING_2026-08-13.md` (UI shell).

## Purpose

This document defines the Verevon agent system model for Spaces and agents: what
a room is, what the Agent page is, how the two divide, and what an agent is
allowed to do in each.

The intended behavior is:

- A Space is a room where people talk to each other **and** to the agents added
  to it. Humans and agents are members of the same room, in the same thread.
- The Agent page is the master control plane for every agent definition, its
  policy, and every place it is installed.
- Agents do not answer unless invoked (`@` mention or an explicit action).
- Space context and authority are always resolved server-side, per invocation.

## What we are competing on

We are **not** building a better Slack or Microsoft Teams, and we should not try.
They have a decade of polish on presence, notifications, mobile, huddles,
threading and integrations, and the customer is usually already inside one of
them. Fighting there is unwinnable and would consume the roadmap.

We are competing on **the agent in the room**. Teams ships Copilot as an
assistant in a side panel; Slack ships Slackbot. Neither is a *member*: neither
holds a role in the channel, has its own audit trail as a participant, or can be
addressed like a colleague with memory of that room. That gap is the opening.

So the room needs enough of the familiar chat affordances that it feels like a
room — threads, mentions, members, activity — and its differentiation comes from
what the agent can actually do once it is in there, and from the fact that the
work leaves receipts.

Buzz is the reference for the room; Grok Bot is the reference for the agent.
Neither is a reason to weaken Verevon's authority model — see
[Security model](#security-model).

### What buzz actually proves

Buzz *is* Slack-shaped: channels, threads, huddles, canvases, desktop and mobile
apps. It earned the right to be a workspace not by out-featuring Slack on chat,
but by owning **one workflow completely** — software development. A branch
becomes a channel; patch, CI, review and the merge decision live in the same log
as the conversation, so "the channel becomes the record of why the code exists."
That made Slack irrelevant for that workflow rather than merely worse.

The lesson: a room is justified when it owns a workflow end to end. Ours is
knowledge → cited answers → approval → effect with a receipt. Build that depth,
not chat parity.

## Core model

### 1) Agent page (global registry and control plane)

The Agent page is the authoritative source of truth for:

- all agent definitions,
- global capabilities, tools, connectors and knowledge scope,
- policy (trigger modes, approval mode, allowed tools, audit visibility),
- ownership and creator metadata,
- **every binding** — which Spaces and which surfaces an agent is installed on.

It is where you:

- create and edit agents in full detail,
- manage templates, memory policy and knowledge scope,
- inspect where an agent is available and revoke it,
- review cross-Space governance and run history.

It is the **only** surface that shows an agent across rooms.

### 2) Space page (the room)

A Space is a runtime collaboration context with:

- members — people and agents in one roster, distinguished by subject type,
- conversation threads shared by the room's audience,
- activity, approvals and receipts,
- a set of Space-bound agents.

Only agents explicitly bound to the Space are considered for interaction there.

Everything visible in a room is **scoped to that room**. A room never shows what
an agent does elsewhere, and never lets you change it.

### 3) The dividing rule

> **If the question involves more than one room, it belongs on the Agent page.
> Otherwise it belongs in the room.**

This is the test to apply when deciding where a control goes.

| In the room | On the Agent page |
|---|---|
| Talk, mention, delegate, approve | Define, configure, govern |
| Add an agent to *this* room | Install across rooms and surfaces |
| Pause / mute / remove **here** | Pause / revoke **everywhere** |
| This room's activity and receipts | Runs and receipts across all scopes |
| One-line creation of a simple agent | Full definition editing and specialization |

Two failure modes this rule prevents:

- A room that shows configuration becomes Copilot's side panel.
- An Agent page that shows conversation becomes a second inbox.

### 4) Orchestration roles

- **Chief/Core agent** — a global coordinator with broad visibility of the agent
  registry and cross-Space metadata. It lives on the Agent page. **Broad
  visibility is not broad authority:** it does not act in every room by default,
  and runtime actions still require an explicit per-Space binding. Its job is
  discovery, routing and assignment — knowing who exists, who fits, who is free.
- **Space agent** — an agent bound to one or more Spaces with explicit scope and
  policy.

## Creating agents

Creation happens on both surfaces, at two different depths. It is the same
object either way.

**In the room (low threshold, for non-technical people).** You describe what you
need in one line and the agent exists. No model picker, no tool list, no
connector setup, no policy form. This is deliberate: the need arises in the room,
not in a registry, and the threshold is the product.

An agent created this way is born:

- bound to that room only,
- `trigger_modes: mention_only`,
- no tools,
- `approval_mode: require_confirmation`.

**On the Agent page (depth).** Everything beyond the simplest agent: model,
instructions, tools, connectors, knowledge scope, policy, and installation
elsewhere.

An agent born in a room can later be opened in the studio and specialized. It
never becomes a different object. **The room gives birth; the studio raises.**

Creation in a room is still two server-confirmed steps behind one user action —
create the definition in the registry, create the binding to the room. The UI
must not say "created" until both are confirmed. The room is the entry point;
the registry remains the owner.

## Invocation rule

An agent must be called to participate:

1. `@` mention in the room composer, or
2. an explicit action from an agent card or button in the room.

Default behavior:

- No passive or unsolicited responses.
- No hidden room access from global page context.
- No always-on presence by default.

**A mention invokes; it never grants.** Mentioning an agent that is not bound to
the room does not give it access — it offers to *add* it, which is a governed
action requiring the right role. Binding is the only grant. If a mention could
grant access, anyone able to type in a room could authorize an agent to read it.

## Agent-to-agent collaboration

Agents may know of each other and hand work between themselves. This is what
makes a chief-plus-specialists arrangement useful rather than decorative. Three
rules make it safe, and they are far cheaper to state now than to retrofit.

**1. Delegation stays inside the room's roster.** An agent may only delegate to
agents that are themselves bound to the same Space, and the delegate re-resolves
its **own** authority before acting. Without this, an agent in the room can
forward room content to one that was never authorized for it, and the room's
recipient audience is bypassed through the agent graph.

**2. The chain carries the initiating human.** Every delegated hop records the
person who started it. Approvals belong to that person, not to the delegating
agent. Without this, responsibility launders into "the agent did it."

**3. Bound the fan-out.** Delegation has a depth limit, a per-invocation
capability budget, and loop detection. A chief that fans out to five specialists
who each fan out again is an unbounded bill and an unreadable room.

**Rendering.** Agent-to-agent coordination renders as **one collapsed unit of
work** with detail available on expansion — never as N messages in the thread.
Grok's own description is that bots "pass work, assign ownership, and only pull
you in for judgment calls," which implies collapsed, not streamed. If agent
chatter is streamed into the room, the human signal drowns.

This is where our model beats the reference products. Grok's bots share a
computer and credentials, which makes agent-to-agent trivially easy and
impossible to audit. Because each of our hops is re-authorized, we can answer
**who asked whom to do what, on whose authority** — a question the reference
products cannot answer. That is the receipt story extended to multiple agents.
It is not friction; it is the product.

## Binding model

A Space binding defines exact permissions and context limits.

Binding fields:

- `space_ref`
- `agent_ref`
- `binding_scope` (`space`)
- `trigger_modes` (`mention_only`, optionally `group_only`) — **stored and
  enforced** (2026-08-16): a present list without `mention` refuses the
  mention at the gateway. Absent means legacy behavior (mention allowed).
- `allowed_tools` — **stored and enforced** for the values that exist today:
  an explicit `[]` strips the turn's tool surface; absent means legacy.
  Per-tool filtering of a non-empty list is Model-plane work and no flow
  writes one yet.
- `approval_mode` (`auto`, `require_confirmation`, `blocked`) — **stored and
  enforced**: `blocked` refuses invocation; `require_confirmation` (and
  absent) forbids autonomous runs from a mention; only `auto` leaves them on.
- `knowledge_scope` (`space`, `thread`, `space_with_links`) — **not stored
  yet**: Control now has shared-Space retrieval authority (`ResolveSharedRetrievalDecisionEvidence`/
  `IssueSharedRetrievalDecision`, the unified `POST /api/v1/internal/spaces/retrieval-decision`
  endpoint room/project/case turns now use, gated on the org's independent
  `retrieval_read_entitled` policy bit), so a room turn with that entitlement
  active gets real, whole-Space grounding — no longer suppressed by kind.
  This field's own finer distinction (thread-only vs space-with-links) is
  still unbuilt; today retrieval is all-or-nothing at the whole-Space grain.
- `default_thread_policy` (`none`, `latest`, `inbox`) — **not stored yet**:
  awaits the delivery-routing surface it governs.
- `audit_visibility` (`owner`, `member`, `readonly`, `none`) — **not stored
  yet**: awaits the audit surface it governs. A stored-but-unenforced policy
  is a false promise, so these three stay out of the schema until their
  enforcement points exist.
- `delivery_targets` — channels the agent's work can reach beyond the room
  (Microsoft Teams, Messenger, embedded widget). An empty list means no channel
  is published, and the UI says exactly that.
- `status`

### Binding states

Earlier drafts of this document and the scope plan disagreed. Resolved:

| State | Meaning |
|---|---|
| `pending` | Binding requested; owner planes have not confirmed it yet. |
| `active` | Visible in the room and invokable. |
| `paused` | Visible but not invokable. (Supersedes the earlier `muted`.) |
| `failed` | Binding or provisioning failed. Shown truthfully, not hidden. |
| `revoked` | Removed. Retained for audit history; never rendered as a participant. |

`inactive` is **not** a binding state. An agent that exists in the catalog but is
not bound to this room simply has no binding here — that is the absence of a
record, not a state of one.

## Message routing behavior

When a user posts in a Space:

1. The message is posted to the Space thread.
2. Routing checks whether any bound Space agent was explicitly called.
3. If called, and the current authority check passes, the message is forwarded
   with Space-scoped context, the bound tool set, and the per-Space memory
   policy.
4. The response returns only in the same Space thread.

If no agent is called, the normal human-only flow continues.

## Security model

Verevon's authority split — Control owns authority, Application owns identity,
Model executes — is the reason we can offer approvals, EU residency and
owner-plane receipts. The reference products are useful for ergonomics and
should not be copied at the mechanism level.

### Principles

**Decisions that authorize future agent behavior must come from outside the
agent.** (Adapted from QM's deliberately portal-only actions.) An agent may
never create or modify a binding, change a role or grant, approve a gated
action, or act as another principal. These look like capability gaps in an
audit; they are walls. Route around them, not through them.

**The agent is not trusted to make authorization decisions.** Owner planes
enforce identity, scope, grants, delivery and effect gates around it.

**Enforcement lives at the identity seam.** (From buzz.) Authority is checked
where identity is established, not scattered through the codebase as filters —
which is why it cannot be sidestepped. A revoked binding bites on the next
authority check, everywhere.

**The decision is recorded separately from its enforcement**, so the trail never
claims something happened that did not.

**A mention is a signal, not a trigger.** Human judgment stays the gate for
anything gated. (Buzz applies the same rule to moderation reports.)

**Surface and connector input is untrusted data even when authenticated.**
Authentication proves the source; it does not make the content safe. Prompt
injection travels in legitimate messages.

**Audit supports investigation; it does not prevent an action.** An approval
means a human accepted the displayed action under the information available at
the time — not that the resulting behavior is safe.

**Revocation** must stop new invocations after a fresh authority check, fence
queued and scheduled work, keep in-flight unknown outcomes visible, and revoke
connector and workspace grants through their owning planes.

**Compute is rented by attention.** (From buzz's remote agents.) An idle Space
agent should not hold a workspace lease. This is both a cost control and an
honesty control: an agent that is not working should not look ready.

### Honest edges

Stated plainly, in the style both reference projects use, because a security
section that lists only what works is marketing.

- A Space agent is **not** a security boundary. If the runtime shares a computer,
  credentials or memory between agents, the UI must not imply isolation between
  them.
- Space membership is necessary but not sufficient for private documents,
  connectors, credentials, tickets or external resources.
- Browser-executed actions are governed by Quarry's grant checks, not by the
  same in-room approval gate as tool calls. That difference should be visible,
  not glossed.
- Delivery targets (Teams, Messenger) leave our boundary. What happens to
  content after delivery is the receiving platform's retention policy, not ours.
- An org admin is a privileged reader. Admin reads are scope-authorized and
  audited, not separately consent-gated.

## UX behavior

- The room's sidebar shows members, conversations, the agents available in that
  room, and explicit entry points for invoking them.
- The main room panel keeps agent configuration out of view; agents are
  participants, not settings.
- Activity uses the **verb / object / outcome** grammar, ordered by consequence
  rather than recency — the question a supervisor asks is "what needs me", not
  "what happened last".
- **Never go dark.** Silence, idle and timeout are rendered states, not empty
  space. If it was not shown, it did not happen.
- Status is carried by text and shape, never colour alone.
- Unrecognized events degrade to an honest generic row rather than being dressed
  up as understood.
- The Agent page remains the discovery, configuration and governance surface.

## Non-goals

- No auto-responding agents in rooms by default.
- No "global agent available everywhere" behavior.
- No cross-Space action execution without an explicit binding check.
- No chat-feature parity race with Slack or Teams (presence, emoji, huddles,
  notification polish). We lose that race and it consumes the roadmap.
- No client-authored authority: the browser never sends actor identity,
  organization identity, recipient audience, service credentials or execution
  decisions.

## Acceptance criteria

- An agent on the Agent page does not become active in a Space until a binding
  exists.
- A Space surfaces and invokes only agents explicitly bound to it.
- Agents answer only when explicitly invoked.
- A mention never grants access; adding an agent is a governed action.
- An agent may delegate only to agents bound to the same Space, and each hop
  re-resolves its own authority.
- Every delegated chain records the initiating human, and approvals belong to
  that person.
- Binding and state updates come from the server; local caches are never
  authority.
- Logging records who invoked which agent, in which Space and thread, and on
  whose authority.
- A user can explain from the UI alone why an agent is in this room and what it
  is allowed to do here.

## Open questions

- Does a Chief/Core invocation require the same confirmation policy as an
  ordinary agent?
- Should mention syntax carry an exact handle (`@agent-name`) and an alias
  (`@space-agent`)?
- Is there a default `trigger_modes` policy for admin-created shared agents?
- Which Space roles may add, pause or revoke an agent?
- Does a Space agent get one shared thread per Space, one per member, or both,
  per `default_thread_policy`?
- Can a personal Space hold an agent? Membership replacement currently refuses
  personal Spaces, so a personal assistant has no write path today.
