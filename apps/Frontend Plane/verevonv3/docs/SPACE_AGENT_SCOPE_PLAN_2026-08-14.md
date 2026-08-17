# Space agents and system agents

## Implementation plan

**Date:** 2026-08-14 · **Revised:** 2026-08-16
**Owner:** Verevon Frontend Plane, with Application, Control, Model, Data, and
agent-surface owners  
**Status:** Product and contract plan. Phase UI-1 is built (see §7).
**Related research:**
`/Volumes/Lagring/Triodelab/CoreSystem/apps/VEREVON_UI_COWORK_RESEARCH_2026-08-13.md`  
**Related frontend docs:**
`docs/space-defenition.md` (product model — read first),
`docs/SPACE_COCKPIT_WIRING_2026-08-13.md`,
`docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md`

> This document defines the boundary between agents that collaborate inside a
> Space and agents that are installed on pages or system surfaces. It is a
> planning document, not proof that the proposed backend contracts exist.

## 0. What changed on 2026-08-16

The product intent sharpened, and Phase UI-1 shipped. Both change what this plan
should say.

**The room is a room.** A Space is a place where people talk to each other and to
the agents added to it — the buzz shape — with a Grok-Bot-quality teammate inside
it. We are not competing on chat with Slack or Teams and must not build toward
feature parity with them; we are competing on the agent, because Copilot is a
side-panel assistant and Slackbot is an automation, and neither is a *member* of
the channel. `space-defenition.md` carries the full framing.

**Agents can be created in the room.** The original plan implied creation belongs
to Agent Studio. It does not, exclusively. The room may create a *simple* agent
in one action, because the need arises in the room and the threshold is the
product. This does not relax §6: creation in a room is still two server-confirmed
steps (definition, then binding) behind one user action, and the registry remains
the owner. What the room must never do is show configuration.

**The surfaces divide by scope, not by verb.** Earlier drafts split them as
"configure vs. use", which broke as soon as rooms could create. The rule is now:
*if the question involves more than one room, it belongs on the Agent page.*

**Agent-to-agent collaboration is in scope**, with three constraints that were
implicit and are now explicit — delegation stays inside the room's roster, the
chain carries the initiating human, and fan-out is bounded. See §8.4 and §11.

**Binding-state vocabulary is resolved** against `space-defenition.md`; `muted`
folds into `paused`, and `inactive` is not a state (see §10.3).

## 1. Executive decision

Verevon has two different agent products:

1. **Space agents** are room-bound coworkers. They behave like the agents in
   Grok Bot or Buzz: they are visible participants in a Space, share the
   Space's conversation and work context, can be addressed by people, and can
   collaborate with other authorized Space participants.
2. **System/page agents** are reusable capabilities. They are configured in the
   Agent area and then installed on one or more pages, products, workflows, or
   external surfaces. A chatbot can be installed on a support page and an
   external website; an SEO agent can be installed in Studio; another agent
   can be attached to an inbox workflow.

The two products may share an underlying agent definition, but they must not
share an implicit audience, conversation, authority grant, or runtime state.

Both may be **created** from either surface — a room can produce a simple Space
agent in one action — but the registry owns every definition, and a binding is
the only thing that puts an agent anywhere. The surfaces divide by scope, not by
verb: *if the question involves more than one room, it belongs on the Agent
page.*

The canonical model is:

```text
AgentDefinition
  ├── SpaceAgentBinding       (collaborator in one Space)
  └── SurfaceAgentBinding     (capability installed on one page/system)
```

An `AgentDefinition` describes what the agent is. A binding describes where it
is allowed to appear, what context it may read, which tools it may use, which
people may invoke it, and where its work is recorded.

## 2. Why this distinction matters

The current UI risks collapsing two different mental models:

- `/spaces/:spaceId` is a shared workroom with a durable conversation record,
  membership, activity, and room context.
- `/agents` is currently an Agent blueprint/studio surface. Its role cards,
  Chatbot Studio, Workflow Builder, and Agent Run Console describe or launch
  governed agent work; they do not yet constitute a Space roster or a
  per-Space bot creation API.

If these are treated as one product, the UI can make false promises:

- A global chatbot blueprint may appear to be a member of a Space without a
  server binding.
- A Space member may appear to have access to an agent's private connector,
  memory, or computer merely because the agent is visible in the Agent page.
- A page-installed agent may accidentally inherit all Space conversations.
- A bot created from a Space may be presented as reusable everywhere without a
  promotion, grant, or lifecycle decision.

The boundary is therefore both a UX rule and an authorization rule.

## 3. Product definitions

### 3.1 Agent definition

An agent definition is the reusable identity and behavior blueprint. It may
include:

- stable `agent_ref`;
- display name, title, description, avatar/color/shape;
- owner organization and creator identity;
- instruction/document revision references;
- model/runtime profile;
- skill and connector references;
- availability and lifecycle state;
- version and provenance metadata.

The definition alone does **not** grant access to a Space, page, connector,
credential, private document, or runtime computer.

The initial implementation may continue to use the existing Agent blueprints
and Studio surfaces as non-deployable configuration references. A deployable
definition requires an owner-plane contract and should not be implied by a
disabled or preview control.

### 3.2 Space agent

A Space agent is an explicit binding between an agent definition and one
Space:

```text
SpaceAgentBinding
  binding_ref
  space_ref
  agent_ref
  display_override_ref?        # optional Space-local presentation revision
  membership_role              # participant, operator, or another governed role
  thread_policy_ref
  knowledge_scope_refs
  workspace_ref?
  allowed_connector_refs
  allowed_skill_refs
  delivery_target_ref?
  status                       # pending, active, paused, revoked, failed
  authority_revision
  membership_revision
  projection_version
  created_at
  updated_at
```

The binding is an Application projection. It references resources owned by
their respective planes:

- **Application:** Space identity, binding projection, room-visible status,
  conversation/activity projection.
- **Control:** membership, grants, privacy, entitlement, recipient audience,
  and authority revisions.
- **Model:** agent runtime, threads, runs, computer/workspace lease, skills at
  execution time, and run receipts.
- **Data:** authored knowledge, memory, artifacts, and retrieval scope.
- **Gateway:** actor-filtered read and intent proxy; never a new authority
  owner.

The browser may select a Space agent, but it cannot mint or carry a binding
decision. Every run re-resolves the current Space membership, agent binding,
recipient audience, tool grants, and resource authority.

### 3.3 System/page agent

A system/page agent is an explicit binding between an agent definition and a
surface:

```text
SurfaceAgentBinding
  binding_ref
  agent_ref
  surface_ref                 # support page, external page, Studio, inbox, etc.
  surface_kind
  installation_status         # draft, pending, active, paused, revoked
  invocation_policy_ref
  audience_policy_ref
  knowledge_scope_refs
  allowed_connector_refs
  allowed_skill_refs
  delivery_target_ref?
  owner_org_ref
  authority_revision
  projection_version
  created_at
  updated_at
```

Examples:

| Agent definition | Surface binding | Result |
|---|---|---|
| Chatbot | Support page | Answers support visitors within that support surface's policy and context. |
| Chatbot | External page | Provides the separately configured external experience; it does not read Space conversations by default. |
| SEO agent | Studio | Reviews or proposes Studio/website work according to Studio grants. |
| Triage agent | Inbox workflow | Classifies or proposes actions against the inbox projection. |

The surface binding is not a Space membership. If the same definition is also
needed in a Space, a separate `SpaceAgentBinding` must be created and
authorized.

## 4. User-facing mental model

### Space page

The Space page should feel like a Slack/Teams room with Grok/Buzz-style agent
participants:

- the Core Sidebar lists confirmed Spaces and their conversations;
- the Space header identifies the room, lifecycle, role, and visible work pulse;
- the Space Agent view lists only agents explicitly bound to this Space;
- the Chat view records conversations shared by the Space audience;
- Activity shows human and agent work in readable causal order;
- Members shows the published roster and role, when the server provides it;
- an empty Space may offer a clearly labelled Agent Studio exploration entry,
  but must not claim that a bot was created or joined the Space without the
  binding contract.

### Agent page

The Agent page should feel like a configuration and distribution workspace:

- browse or search agent definitions and blueprints;
- configure identity, instructions, skills, connectors, model/runtime, and
  readiness;
- view existing Space and surface installations where the actor is allowed;
- install an agent on a page/system through a governed flow;
- create or edit a Space binding only through an authorized Space-aware flow;
- keep preview/blueprint state distinct from active/deployed state;
- show which context and tools are attached to each installation.

The Agent page must never imply that a global blueprint is automatically a
member of every Space or page.

## 5. Scope matrix

| Concern | Space agent | System/page agent |
|---|---|---|
| Primary user mental model | Coworker in a room | Reusable capability installed on a surface |
| Visibility | Space members with an actor-filtered roster | Surface administrators/users covered by surface policy |
| Context | Space conversations, Space-approved knowledge, Space work state | Surface request/context and installed resources |
| Conversation record | Space-scoped thread/history | Surface-specific session, support thread, or workflow record |
| Membership | Explicit Space binding plus Control membership | Explicit surface installation and audience policy |
| Tools/connectors | Space binding allowlist plus current run authorization | Surface binding allowlist plus current invocation authorization |
| Runtime computer/workspace | Optional Space-scoped Model lease | Optional surface/workflow-scoped lease; never inherited from a Space |
| Delegation | Only to agents/participants visible and authorized in the Space | Only to agents authorized for the surface/workflow |
| Sharing | Requires explicit promotion or second binding | Requires explicit installation on another surface |
| Deletion/revocation | Space binding and Space records are revoked according to Space lifecycle | Surface binding and surface records follow surface lifecycle |
| UI owner | Space cockpit and Core Sidebar | Agent Studio and surface configuration UI |
| Current implementation status | No durable roster/binding endpoint yet | Blueprints and preview/configuration surfaces exist |

## 6. Non-goals and guardrails

This plan does not authorize the frontend to:

- create a bot or bind it to a Space with a client-only click;
- infer Space members or agents from the action catalog;
- copy a page agent's private memory, credentials, or connectors into a Space;
- use `space_ref` as a bearer token or authority grant;
- make all Space members share one agent's private credentials;
- treat a successful binding request as proof that a runtime is provisioned;
- show a global agent blueprint as active in a Space;
- add a new frontend-owned authority aggregator;
- invent organization, team, external, or agent roster records before the
  actor-filtered projection exists;
- call Chat an embedded Space surface. Space remains separate from the Chat
  application; links preserve the explicit Space/thread query parameters.

## 7. Current-state inventory

### Existing Space capabilities

The current frontend has the following relevant contracts:

- `src/shared/api/spaces-client.ts`
  - `listSpaces()` returns actor-filtered `SpaceSummary` records;
  - `getSpaceContext(spaceRef)` returns the server-composed Space and current
    membership fact;
  - `getSpaceThreads(spaceRef)` returns the Space conversation projection;
  - `getSpaceActions(spaceRef)` returns an actor-filtered action catalog;
  - no Space agent roster or binding endpoint is present.
- `src/features/spaces/components/SpacePage.tsx`
  - rechecks Space context every 30 seconds;
  - fails closed when context is unavailable;
  - renders Chat, Activity, and Members through the presentational cockpit;
  - currently offers Agent Studio exploration on a successful empty
    conversation projection;
  - keeps Space and Chat as separate surfaces.
- `src/features/spaces/components/SpaceCockpit.tsx`
  - owns the six-view shell: Chat, Work, Knowledge, Activity, Agent, Members;
  - receives injected content and must not fetch or compose authority itself.
- `src/features/core/components/sidebar/CoreSidebarSpacesPanel.tsx`
  - is the Core Sidebar's Space navigator;
  - reads the server Space index and selected Space thread projection;
  - must remain the sole room/conversation navigator.

### Existing Agent capabilities

- `src/features/agents/components/AgentsPage.tsx` selects blueprint roles and
  routes to Chatbot Studio, Workflow Builder, or role workspaces.
- `src/features/agents/lib/use-agent-selection.tsx` uses URL state such as
  `agent=chatbot&view=playground`.
- `src/shared/actions/agent-tools.ts` filters actions by governed Model
  eligibility; registry presence is not execution authority.
- `src/shared/actions/preset-agents.ts` contains curated run presets whose
  action sets remain approval-gated.
- The current global Agent Studio link is an exploration/configuration shortcut,
  not a Space-scoped creation or binding flow.

### Built since this plan was written (2026-08-15/16)

Phase UI-1 is no longer proposed — it is live. Recorded here so the plan stops
describing a state that has passed.

- **Application:** `spaceAgentBindings` table and `convex/spaceAgents.ts`
  (`spaceAgentBindingsForGateway` query, `upsertSpaceAgentBinding` internal
  mutation). The write is deliberately internal: binding is a governed decision,
  so the browser cannot reach it.
- **Control:** already modelled agents correctly — `space_memberships` with
  `subject_type='service'`. No new Control contract was needed for the read path.
  `MembershipReplacement.ManagedSubjectTypes` was added so an org-roster sync,
  which knows only people, cannot revoke a room's agents as a side effect.
- **Gateway:** `GET /api/v1/spaces/:space_ref/agents`, joining Control's
  authoritative roster with the Application binding projection. The join is
  asymmetric on purpose: an agent Control authorizes but Application has not
  named still appears, flagged `identity_published: false`; a binding without a
  Control membership is dropped entirely.
- **Frontend:** `getSpaceAgents()` and the Agent tab as room participants, with
  lifecycle status (text plus shape, never colour alone) and delivery-target
  channels for Teams / Messenger / embed.

### Required backend gaps

Still missing before the room behaves as `space-defenition.md` describes:

1. **The invocation model** — `@` mention parsing, `trigger_modes` enforcement,
   and routing a mention to a run scoped to that Space and thread. Nothing
   currently invokes an agent from a room, so the Agent tab is a roster and not
   yet a teammate. This is the single largest gap.
2. **The policy half of the binding** — `trigger_modes`, `allowed_tools`,
   `approval_mode`, `knowledge_scope`, `default_thread_policy`,
   `audit_visibility`. Today's binding carries identity and delivery targets
   only, which makes it a label rather than a boundary.
3. Server-authorized create/update/revoke intents, including the two-step
   room-creation flow.
4. A binding-to-runtime resolution path.
5. Space-scoped thread/run/activity correlation.
6. Connector, skill, knowledge, and workspace scope references.
7. Revocation and deletion receipts appropriate to each owner plane.
8. **Agent-to-agent delegation** with roster containment, human-carrying chains,
   and a fan-out budget.
9. A **Chief/Core agent** projection on the Agent page — cross-Space visibility
   without cross-Space authority.

## 8. Target architecture

### 8.1 Read path

```text
SpacePage
  ├─ getSpaceContext(space_ref)
  ├─ getSpaceThreads(space_ref)
  ├─ getSpaceAgentBindings(space_ref)       # proposed
  └─ getSpaceActivity(space_ref)            # may remain thread-derived initially

Agent Studio
  ├─ listAgentDefinitions()                 # existing/owner-defined contract
  ├─ getAgentDefinition(agent_ref)
  ├─ listAgentBindings(agent_ref)           # proposed actor-filtered view
  └─ listInstallableSurfaces(agent_ref)     # proposed policy-filtered view
```

All reads are projections. The frontend renders what the server publishes and
does not combine a global agent catalog with a Space ref to manufacture a
binding.

### 8.2 Write path

```text
User chooses "Add agent to Space"
  → frontend sends an intent with agent_ref + space_ref only
  → Gateway resolves session, membership, policy, and recipient audience
  → owning services validate binding and resource grants
  → Application persists a binding projection / pending lifecycle
  → Model provisions or resolves runtime on the binding
  → frontend polls/subscribes to the binding projection and receipts
```

The browser must not send actor identity, organization identity, recipient
audience, service credentials, connector secrets, or execution decisions.

The analogous page installation flow uses `surface_ref` and a surface policy;
it does not silently reuse the Space path.

### 8.3 Runtime resolution

At invocation time, Model should receive a server-built resolved context that
contains references and signed decisions, not raw client scope fields:

```text
ResolvedAgentContext
  agent_ref
  binding_ref
  scope_kind                  # space or surface
  scope_ref
  authority_revision
  recipient_audience_revision
  owner_resource_decision_refs
  instruction_revision_refs
  knowledge_filter_refs
  memory_read/write_scopes
  workspace_lease_ref?
  skill_refs
  connector_refs
  action_catalog_version
  capability_budget
  approval_policy
  delivery_target_ref?
```

The exact signed decision schema belongs to Control and the consuming owner
planes. The frontend only displays safe projection fields.

## 9. UI plan

### Phase UI-0: preserve truthful current behavior

**Goal:** keep the current UI honest while backend contracts are missing.

- Keep the Core Sidebar as the only Space navigator.
- Keep Space and Chat separate; remove any header action that makes Chat look
  embedded in the Space shell.
- Keep the Agent tab as an honest unavailable/preview state until a Space
  agent projection exists.
- Keep the empty-room CTA labelled as Agent Studio exploration, not “bot added”
  or “create a Space bot,” unless the binding contract is live.
- Keep the CTA hidden when thread projection is unavailable or when the Space
  is not confirmed.
- Do not show fake agent cards, fake member counts, fake skills, or fake
  connector status.

**Exit criteria:** a user can understand that Agent Studio defines/reviews
blueprints and that the current Space does not yet have a published agent
roster.

### Phase UI-1: Space Agent read-only projection

**Goal:** render actual Space-bound agents in the existing cockpit.

Add a typed client function, proposed as:

```ts
getSpaceAgents(spaceRef: string): Promise<SpaceAgentProjection>
```

Proposed safe projection:

```ts
type SpaceAgentProjection = {
  space: SpaceSummary
  membership: SpaceMembership
  agents: readonly SpaceAgentSummary[]
  projection_version: string
}

type SpaceAgentSummary = {
  binding_ref: string
  agent_ref: string
  name: string
  title?: string
  description?: string
  avatar?: { kind: string; value: string }
  status: 'pending' | 'active' | 'paused' | 'revoked' | 'failed'
  availability?: 'available' | 'working' | 'needs_attention' | 'offline'
  latest_thread_id?: string
  latest_run_status?: string
  updated_at?: string
}
```

The projection must not contain bearer tokens, connector secrets, raw private
resource grants, recipient tokens, or an execution decision that the browser
could replay.

UI behavior:

- show a calm “Space agents” heading and short explanation;
- show one card/list row per confirmed binding;
- use text plus icon/shape for status, never color alone;
- link to the actual Space conversation when `latest_thread_id` is published;
- show pending/paused/failed truthfully;
- show a designed unavailable state when the endpoint is absent or fails;
- preserve the six-tab hash/deep-link behavior.

### Phase UI-2: Space agent collaboration

**Goal:** make a Space agent feel like a Grok/Buzz coworker while retaining
Verevon's authority model.

Add only after the read projection is stable:

- agent identity header with name, title, description, and current status;
- “Message agent” link that creates/opens a Space-scoped thread through the
  existing Chat route;
- delegation affordance listing only authorized Space agents;
- readable run/activity rows for queued, running, approval, completed, failed,
  and unknown outcomes;
- link to agent work/computer only when Model publishes an authorized lease or
  run projection;
- clear “needs your approval” state without exposing a grant or secret;
- optional Space-local instructions or skills when their revisions are
  published by the owner plane.

Do not implement a second chat composer inside Space. Use the existing Chat
surface and pass the exact encoded `space_ref` and `thread_id`.

### Phase UI-2b: invocation — the mention model

**Goal:** make a bound agent addressable. Until this exists, an agent card is a
name and nothing else.

This is now the highest-value phase, ahead of any creation UI: a room with agents
you cannot talk to does not deliver the product at all.

- `@` in the room composer offers **people and agents from the same roster** —
  one autocomplete, one mental model, distinguished by subject type.
- Mentioning a bound agent invokes it. The invocation carries `space_ref` and
  `thread_id` and nothing else the browser authored; Control re-resolves
  membership, binding, recipient audience and tool grants at execution time.
- Mentioning an agent that is **not** bound offers to add it — a governed action
  gated on the caller's Space role. It never grants by mention.
- `trigger_modes` is enforced server-side. `mention_only` is the default and the
  only mode the first release needs.
- An invoked agent's reply lands in the same thread. Nothing routes elsewhere.
- The composer shows plainly when an agent will be invoked, before sending.

**Exit criteria:** a person can address an agent in a room and get a reply in the
same thread, with the run visible in Activity and any gated action surfacing an
approval to the initiating human.

### Phase UI-2c: agent-to-agent delegation

**Status 2026-08-17: architecture mapped, NOT started — no safe partial
slice exists.** Scoped to "same-room, one hop, human present, from an
`approval_mode: auto` binding" (the user's explicit narrow choice). Full
investigation:

- The codebase's existing `subagent.*` mechanism (`execution-core/src/subagent/mod.rs`)
  is the WRONG shape: it re-enters the same driver with a fresh history under
  the SAME identity/authority (no re-resolution), capped at depth 1. UI-2c's
  actual requirement — "the delegate re-resolves its own authority; it never
  inherits the delegator's" — needs a genuinely different primitive.
- Model Plane's own CLAUDE.md is explicit and non-negotiable here: `dispatch_tool`
  (model-gateway's plain-chat loop) "refuses anything side-effecting" — see the
  2026-08-14 HARN-1/2 withdrawal (`git show ac2be529`) for why this boundary is
  not casually crossable. Delegation is unambiguously a side effect, so it can
  only live inside execution-core's governed loop (`execute_step_inner`).
- Execution-core's governed loop is reachable ONLY via the `agentic`/`plan_mode`
  feature, which is real and live (`sse.rs`'s "chat-parity Phase 3 — agentic
  run", used today by `/chat`'s plan-mode toggle) but is **never requested by
  the room composer** — `approval_mode: auto` is currently 100% inert.
- **The trap**: naively flipping the gateway's `apply_mention_binding_policy` to
  request the `agentic` feature whenever `approval_mode: auto` would NOT
  narrowly grant "may delegate" — it would silently hand that room agent
  execution-core's ENTIRE capability surface (browser automation, sandboxed
  code execution, whatever the org has registered in capability-core), since
  no delegation-specific capability exists yet to scope it down to. That is
  exactly the silent authority-expansion the QM/buzz-derived security model
  this doc already commits to (see "Security model" below) forbids. There is
  no safe partial slice — only the full build or nothing.
- Confirmed via grep: execution-core has **zero** existing network path to
  `verevon-gateway-rs` (Frontend Plane), and the gateway has no non-streaming
  internal invoke endpoint. Execution-core DOES already hold a precedent for
  calling Control directly (`user_core_client.rs`, gRPC, service-credential
  auth) but has and should have no Convex-calling precedent (Convex is
  Application Plane's own store — cross-plane DB access is against this
  repo's architecture rules).
- **Recommended design for the eventual build** (reuse-heavy, not a rewrite):
  register a narrowly-scoped `delegate_to_agent` capability inside
  execution-core's loop, gated on (a) an active Space context, (b) same-Space
  Control membership check for the target (new minimal HTTP client, mirroring
  `user_core_client.rs`'s pattern but hitting Control's existing
  `/api/v1/internal/spaces/:space_ref/roster` HTTP endpoint rather than adding
  a new one), (c) depth=1 enforced for free by NOT granting the delegate's own
  turn the `agentic` feature (so it physically cannot delegate again — no
  counter needed). The actual re-invocation should NOT duplicate Control+Convex
  persona resolution in Rust; it should call back into the SAME, already-live,
  already-tested `inject_mentioned_space_agent_persona` pipeline via a new
  internal (non-streaming) endpoint on `verevon-gateway-rs`, using the
  ORIGINAL human's own session bearer — which trivially satisfies "every hop
  records the initiating human" (it IS the same authenticated human) and
  reuses today's per-turn attribution (task #11) for the room's "one collapsed
  unit" rendering, needing only a small grouping affordance in
  `SpaceRoomTimeline.tsx`, not a new component.
- Needs, in order: (1) an inter-plane network path from execution-core to
  `verevon-gateway-rs`; (2) the new internal invoke endpoint; (3) the Control
  membership-check client; (4) the capability registration (execution-core +
  likely capability-core); (5) the room's collapsed-unit affordance. This is a
  multi-service, multi-session effort at this project's established quality
  bar (tested + live-verified at every layer) — not a same-day addition.

**Goal:** let a chief-plus-specialists arrangement work without laundering
authority or flooding the room.

Add only after UI-2b is stable.

- An agent may delegate only to agents **bound to the same Space**. The delegate
  re-resolves its own authority; it never inherits the delegator's.
- Every hop records the **initiating human**. Approvals belong to that person.
- Delegation carries a depth limit, a per-invocation `capability_budget`, and
  loop detection. Exceeding any of them fails visibly rather than silently.
- The room renders a delegated chain as **one collapsed unit of work**, expandable
  to the hops. Never as N messages.
- Activity records who asked whom, for what, and on whose authority.

**Exit criteria:** a supervisor can read one row in Activity, expand it, and see
the whole chain with attribution intact — and a delegation to an unbound agent is
refused with an honest reason.

### Phase UI-3: Space binding flow

**Status 2026-08-17: BUILT and live-verified.** Authority decision from the
UI-2c/UI-3 planning round: **owner/manager only, the same room-role gate as
UI-3b's create flow** — reused directly rather than re-derived, via a new
shared `require_space_agent_grant_role` helper both `create_space_agent` and
the two new handlers now call.

- Convex: `listInstallableSpaceAgentsForGateway` (query, org's `agents` by
  `by_org` index, `alreadyBound` computed against this space's
  non-revoked bindings) and `bindExistingSpaceAgentForGateway` (mutation,
  creates a `pending` binding for an EXISTING `agentId` via the same
  `upsertSpaceAgentBinding` internal mutation UI-3b uses — same born-with
  policy: `["mention"]` / `[]` / `require_confirmation`. A bound
  definition's own, possibly broader, Agent Studio configuration is never
  inherited).
- Gateway: `GET /spaces/:space_ref/agents/available` and
  `POST /spaces/:space_ref/agents/bind` (body: `{agent_ref}` only — no actor
  or authority field is client-authored, matching the flow's step 4). Both
  routes run the same role-gate as creation, then the same
  confirm-with-Control step (`confirmSpaceAgentMembershipForGateway`) UI-3b
  already built, so an unconfirmed roster leaves the binding truthfully
  `pending`, exactly like a fresh creation. 8 new Rust tests (owner binds,
  editor is rejected for both list and bind, missing `agent_ref` is
  rejected, unconfirmed roster stays pending, browse reports
  `already_bound` truthfully instead of hiding it).
- Frontend: `SpaceBindAgentDialog` (list of the org's definitions, disabled
  "Allerede lagt til" state for ones already bound here, "Legg til" for the
  rest) alongside the existing `SpaceCreateAgentDialog`; a second
  "Legg til eksisterende agent" button next to "Opprett en agent" in the
  Agent tab header, same `canCreateAgent` role gate.
- **Live-verified end-to-end in the real org**: created "UI3
  Testbindingsagent" from the Personal room (Control declined that room's
  confirmation — a pre-existing gap, not a UI-3 defect, see the
  environment note below), then opened AQUATIQ AS's "Legg til eksisterende
  agent" list, saw it offered as unbound while Driftsassistent/Statusagent
  correctly showed "Allerede lagt til", bound it, watched Control confirm,
  and saw it render as a full active participant with the correct
  born-with policy chips ("Kun @-nevning", "Krever bekreftelse", "Uten
  verktøy") — proving the policy comes from the room binding, never from
  wherever the definition happened to be authored.

⚠⚠ **Environment landmine hit during this verification**: recreating the
`verevon-gateway-rs` container via `docker compose up -d --no-deps gateway`
reset `APPLICATION_CONVEX_SERVICE_KEY` to empty — `docker-compose.yml` maps
it from a host env var (`CONVEX_INTERNAL_SERVICE_KEY`) that is not in any
`.env` file, only ever exported ad hoc in a prior session's shell. Every
`convex_gateway_call` degrades or fails differently depending on the
caller: `space_agent_bindings` (identity lookup) swallows the error and
still returns 200 (Control-only view, names missing), while
`space_lifecycle_by_ref` (used by `/context` and `/threads`) correctly
fails closed with 503 `space_lifecycle_unavailable` — which is why the room
looked entirely broken ("Rom utilgjengelig") even though roster/agents
endpoints still worked. Recovered the live value with
`npx convex env get CONVEX_INTERNAL_SERVICE_KEY` against the self-hosted
deployment (do **not** trust the stale value sitting in
`.env.pre-per-service-bak`) and exported it before recreating the
container. **Pattern to remember: any gateway container recreate must carry
forward every host-env-sourced secret the compose file maps with a
`:-` empty default — a missing one degrades silently rather than refusing
to start.**

**Goal:** allow an authorized user to bind an existing agent definition to a
Space.

Prerequisites:

- Control-defined role/capability for binding an agent to a Space;
- Application binding write contract with idempotency and lifecycle states;
- Model resolution/provisioning contract;
- recipient audience and privacy policy;
- connector/skill/resource grant resolution;
- audit and receipt projection;
- non-owner and unsupported-capability behavior.

Flow:

1. User opens Space Agent tab and chooses “Add agent.”
2. UI loads only agent definitions the actor can see and install.
3. User reviews name, purpose, tools, knowledge scope, delivery target, and
   what Space members will see.
4. User submits `agent_ref` and `space_ref` plus an idempotency key; no actor or
   authority fields are client-authored.
5. UI displays `pending` until the binding projection confirms state.
6. UI surfaces owner-plane failures and partial provisioning explicitly.
7. Once active, the agent appears in the Space roster and may receive Space
   threads according to the binding policy.

The form must never say “created” until the server confirms the definition and
binding lifecycle. If creation of a new definition is introduced later, it is
a separate Agent Studio flow followed by an explicit Space binding step.

### Phase UI-3b: create an agent from the room

**Status 2026-08-16: BUILT and live-verified.** `SpaceCreateAgentDialog`
(Grok-style: color, name, template suggestions) → gateway
`POST /spaces/:space_ref/agents` (room role owner/manager gate, resolved from
Control under the caller's delegation) → Convex
`createSpaceAgentForGateway` (definition + `pending` binding in one
transaction) → Convex action `confirmSpaceAgentMembershipForGateway`
declares the room's full service roster to Control with
`managed_subject_types: ["service"]` (the counterpart of the human sync's
`["user"]`) → Control acceptance flips the binding `active`. Verified
end-to-end in the AQUATIQ AS room: created "Statusagent" from the dialog,
Control roster row appeared, the agent joined the mention list and answered
in its template persona. Deferred from the born-with list below: the
`mention_only`/`approval_mode` policy fields still do not exist on the
binding (tracked as the policy-fields task) — invocation is gated by binding
status + Control membership only, exactly as UI-2b left it.

**Goal:** let a non-technical person get a working teammate without opening a
studio. This is the threshold-lowering the product depends on.

- One action in the room: describe what you need in a line.
- Behind it, two server-confirmed steps — create the definition in the registry,
  then create the binding to this room. The UI does not say "created" until both
  confirm, and says which step failed when one does.
- Born with: this room only, `mention_only`, no tools,
  `approval_mode: require_confirmation`.
- No model picker, tool list, connector setup or policy form appears in the room.
  Everything beyond the default is a trip to the Agent page.
- The result appears in the roster as a member, immediately addressable.

**Exit criteria:** someone who has never opened Agent Studio can create an agent
and get useful work from it without leaving the room — and an admin can see, from
the Agent page, exactly what was created and with what defaults.

### Phase UI-4: Agent page installation management

**Goal:** make `/agents` the control center for reusable definitions and their
installations.

Add a clear separation between:

- **Blueprints:** reference/configuration state, not deployed;
- **Definitions:** saved agent identity and behavior;
- **Space installations:** explicit Space bindings;
- **Page/system installations:** explicit surface bindings;
- **Runs:** governed execution records and approvals.

Suggested Agent page areas:

```text
Agent Studio
  Overview / blueprints
  Definition editor
  Skills and connectors
  Policy (trigger modes, approval mode, allowed tools, audit visibility)
  Space installations
  Page/system installations
  Runs and receipts
  Chief/Core agent — cross-Space routing and discovery
```

The **Chief/Core agent** belongs here because this is the only surface with a
cross-Space view. It sees the registry and which agents are bound where, and it
routes and assigns. It does **not** act in a room it is not bound to: broad
visibility is not broad authority. Its own runtime actions require an ordinary
per-Space binding like any other agent.

Each installation row should show scope kind, scope name, status, revision,
and allowed capabilities. It should link to the scoped surface without
presenting the installation as a global membership.

### Phase UI-5: cross-scope promotion (later)

**Goal:** allow a Space agent or system agent to be reused safely.

Promotion is not a copy operation. It requires:

- a new binding in the target scope;
- target-scope audience and privacy review;
- explicit skill/connector/resource grants;
- new authority and projection revisions;
- independent lifecycle and revocation;
- provenance linking the new binding to the source definition/version.

The UI should say “Install in another surface” or “Add to Space,” not
“share access,” unless the backend contract defines exactly what is shared.

## 10. Data and API contract plan

### 10.1 Proposed read endpoints

Names are placeholders until the owning teams approve the API:

| Endpoint | Owner/projection | Purpose |
|---|---|---|
| `GET /api/v1/spaces/:space_ref/agents` | Application via Gateway | Actor-filtered Space agent bindings and safe identity/status projection |
| `GET /api/v1/agents/:agent_ref` | Agent owner via Gateway | Agent definition safe profile and lifecycle |
| `GET /api/v1/agents/:agent_ref/installations` | Application/owner projection | Actor-filtered Space and surface bindings |
| `GET /api/v1/surfaces/:surface_ref/agents` | Surface owner via Gateway | Agents installed on a page/system |
| `GET /api/v1/spaces/:space_ref/agent-activity` | Application projection | Correlated Space agent runs/events, when thread-derived activity is insufficient |

Every endpoint must use typed envelopes and preserve the current gateway rule:
no raw upstream tokens, forged scoping headers, or authority decisions in the
browser response.

### 10.2 Proposed intents

| Intent | Inputs allowed from browser | Server responsibilities |
|---|---|---|
| Add agent to Space | `agent_ref`, `space_ref`, idempotency key | Resolve actor/org, check Space role, check agent install policy, resolve audience/resources, create binding lifecycle |
| Remove agent from Space | `binding_ref`, idempotency key | Recheck authority, revoke future work, preserve honest in-flight/unknown outcomes |
| Install agent on surface | `agent_ref`, `surface_ref`, idempotency key | Apply surface policy and audience, create independent binding |
| Pause/resume binding | `binding_ref`, desired lifecycle, idempotency key | Reauthorize and fence new work before changing lifecycle |
| Create agent definition | Agent Studio fields only, when supported | Validate owner, version instructions, skills, connectors, provenance, and readiness |

The frontend must not submit `actor_id`, `org_id`, `recipient_audience`,
`service_audience`, `decision_ref`, bearer tokens, connector credentials, or
`owner_plane` as authority inputs.

### 10.3 Lifecycle states

Binding state and runtime state are separate:

```text
Binding: pending → active → paused → revoked
                     └──────→ failed
Runtime:  unresolved → provisioning → ready → working
                                      ├──────→ approval_waiting
                                      ├──────→ degraded
                                      └──────→ failed
```

The UI must not map `pending` to `active`, `authorized` to `deleted`, or
`working` to `completed`. Unknown and partial states are first-class.

**Vocabulary resolved (2026-08-16).** An earlier draft of `space-defenition.md`
used `inactive | active | muted | revoked`, which conflated two different
questions. The binding states above are canonical:

- `muted` was the same idea as `paused` — visible, not invokable. Use `paused`.
- `inactive` is **not** a binding state. An agent in the catalog with no binding
  in this room has no record here; that is an absence, not a state. Rendering it
  as a state is how a global blueprint starts looking like a room participant.
- `failed` has no equivalent in the older list and must survive: a binding that
  failed to provision is a real condition users need to see.

This is the set implemented in `convex/schema.ts` (`spaceAgentBindings.status`).

## 11. Permissions and privacy model

### 11.0 Principles borrowed from the reference projects

Buzz and QM have both published their security thinking. These are the parts
that transfer, with the Verevon consequence stated.

**Decisions that authorize *future* agent behavior must come from outside the
agent.** QM excludes three actions from its agent self-API — admin grant
changes, impersonation, and command-approval decisions — and notes that these
"look like capability-parity gaps in an audit; they are walls, not gaps."

*Verevon consequence:* an agent may never create or modify a Space binding,
change a role or grant, approve a gated action, or act as another principal.
`upsertSpaceAgentBinding` is an `internalMutation` for exactly this reason.
Parity work routes around these, not through them.

**Enforcement lives at the identity seam.** Buzz places moderation enforcement
where identity is established rather than scattering it as filters, "which is why
it can't be sidestepped."

*Verevon consequence:* binding and membership are checked in the gateway's
session/authority path and re-resolved by the owning plane at execution — not as
per-handler conditionals that a new endpoint can forget.

**The decision is recorded separately from its enforcement,** so the trail never
claims something happened that did not.

*Verevon consequence:* this is the postcondition/verified-outcome work already in
Model Plane. A binding revocation record and the fencing of queued work are two
facts, and the receipt must not assert the second from the first.

**Reports are signals, never triggers** — human judgment is the gate.

*Verevon consequence:* a mention invokes but never grants; adding an agent is a
human decision with a role check.

**The agent is not trusted to make authorization decisions,** and surface or
connector input is untrusted data even when authenticated — "authentication
proves the source; it does not make the content safe."

*Verevon consequence:* prompt injection arrives inside legitimate messages from
legitimate members. Tool grants and approval gates are the control, not content
inspection.

**Audit supports investigation; it does not prevent an action.** An approval
means a human accepted the displayed action on the information available at the
time, not that the behavior is safe.

**Compute is rented by attention.** Buzz's remote agents bound their own lifetime
and exit after silence.

*Verevon consequence:* an idle Space agent should release its
`workspace_lease_ref`. Cost control and honesty control at once — an agent that
is not working should not look ready.

### 11.1 Space agents

- A person must be a current Space member to see Space agent bindings unless a
  separately authorized operator projection says otherwise.
- Binding visibility and invocation authority are separate checks.
- Space membership is necessary but not sufficient for private documents,
  connectors, credentials, tickets, or external resources.
- Shared Space conversations use the current recipient audience. A member
  change forces fresh resolution before retrieval, resume, delivery, or effect.
- A Space agent must not be used as a security boundary. If the runtime shares
  a computer, credentials, or memory across agents, the UI must not imply
  isolation between them.

### System/page agents

- A surface installation is visible only under the surface's audience policy.
- A page agent receives only the surface context and explicitly granted
  resources.
- A support agent may be linked to a Space conversation only through a typed,
  server-owned support/thread binding; the page does not inherit the whole
  Space.
- External pages require an explicit external audience/privacy contract.
- Installing the same agent definition on two surfaces creates two bindings,
  not one shared authority context.

### Revocation

Revoking a Space or surface binding must:

- stop new invocations after a fresh authority check;
- fence queued/scheduled work where supported;
- preserve in-flight unknown outcomes visibly;
- revoke connector and workspace grants through their owning planes;
- return owner-specific receipts where deletion or cleanup is requested.

## 12. Testing strategy

### Unit tests

Add pure tests for:

- binding scope discriminators (`space` versus `surface`);
- safe projection normalization and unknown lifecycle/status handling;
- URL encoding for Space and thread links;
- filtering Space agents by server-provided projection only;
- no fallback from a failed agent projection to an empty roster;
- installation status copy and accessible text equivalents;
- separation of definition identity from binding identity;
- delegation candidate filtering by current Space binding;
- no client-authored authority fields in intent payloads.

### Component tests

#### `SpacePage.test.tsx`

- Agent tab renders the honest unavailable state when the binding endpoint is
  not published.
- Agent Studio exploration CTA appears only for a successful empty thread
  projection and has no `space_ref` authority claim.
- Failed thread or agent projection never renders “no agents” or fresh-start
  copy as if the room were empty.
- Successful `getSpaceAgents` renders only projected Space bindings.
- Pending, paused, failed, and unknown agent states remain visible and textual.
- Space agent links preserve encoded `space_ref` and `thread_id`.
- Space context failure removes all Space actions and agent content.
- A late failed request for an old Space cannot suppress the current Space's
  agent projection.
- Shared Spaces do not expose personal deletion controls.

#### `SpaceCockpit.test.tsx`

- Agent tab remains deep-linkable through hash aliases.
- Active panel and tab ARIA relationships resolve to existing DOM nodes.
- Stale injected content disappears when switching tabs.
- Roving keyboard focus moves to the selected tab.

#### New `SpaceAgentPanel.test.tsx`

- Renders projected agent identity, role/status, and latest conversation.
- Renders an honest unavailable state for 404/contract-not-published.
- Does not render raw IDs, bearer-like values, or fabricated members/tools.
- “Message agent” points to Chat with encoded Space/thread parameters.
- Delegation list excludes unbound or revoked agents.
- Add-agent action is absent/disabled for unsupported capability and non-owner
  roles until the write contract exists.

#### `AgentsPage.test.tsx` and Studio tests

- Blueprints are labelled as blueprint/preview when not configured.
- Space installations and page installations appear in separate groups.
- A definition without a binding is not shown as active in a Space.
- Installation links preserve the correct scope kind and identifier.
- Agent Studio creation/configuration does not claim Space membership.

#### Core Sidebar tests

- The Spaces panel remains the sole room navigator.
- Space agent rows are not duplicated into the Core Sidebar until the product
  explicitly chooses that placement; the initial roster belongs in the Space
  Agent tab.
- Search filters Spaces/conversations as currently specified and never turns a
  failed projection into “no conversations.”

### Integration and gateway tests

- Actor/org identity is derived from the authenticated session.
- Forged `space_ref`, `agent_ref`, surface IDs, recipient audiences, and owner
  headers cannot widen authority.
- A Space agent binding cannot be read by a non-member.
- A page agent cannot read Space threads without an explicit typed binding.
- A revoked binding refuses new work after authority revision changes.
- Same definition with two bindings receives independent scope/audience
  resolution.
- No upstream bearer, decision, connector credential, or recipient token is
  returned to the browser.

### E2E tests

Add fixture-backed flows to `tests/e2e/space-shell.spec.ts` or a dedicated
`space-agents.spec.ts`:

1. Open a Space with no agent projection and confirm the honest placeholder.
2. Open a Space with one projected agent and confirm the roster, status, and
   conversation link.
3. Switch between Space and Agent pages; confirm scope labels and URL state do
   not leak.
4. Install a chatbot on a support fixture; confirm it appears on support but
   not in the Space roster.
5. Bind the same fixture definition to a Space; confirm the Space binding is a
   separate row with separate context.
6. Revoke the binding; confirm new work is unavailable while historical
   activity remains truthful.
7. Run a 390px viewport smoke test for room navigation, Agent tab access, and
   no horizontal overflow.

## 13. Delivery phases and ownership

| Phase | Deliverable | Primary owner | Dependencies |
|---|---|---|---|
| P0 | Approve terminology, scope matrix, and non-goals | Product + Architecture | This plan |
| P1 | Publish `AgentDefinition` and binding ADR; define owner fields/revisions | Architecture + Control/Application/Model | P0 |
| P2 | Add actor-filtered Space agent read projection | Application + Gateway | Space membership/index contract |
| P3 | Render read-only Space Agent tab and honest states | Frontend | P2 |
| P4 | Define binding create/revoke intents and receipts | Control + Application + Model | P1/P2 |
| P5 | Add Space binding flow with capability gating | Frontend + backend owners | P4 |
| P6 | Add agent installation management to Agent Studio | Frontend + agent owner | P1/P4 |
| P7 | Add runtime, skills, connector, workspace, and activity projections | Model/Data/Application | P4 |
| P8 | Add cross-scope promotion and external surface installs | Product + owner planes | P5/P6/P7 |

Frontend work should be delivered in small slices:

1. tests and types for projection states;
2. Agent tab placeholder/read projection;
3. URL/link and accessibility wiring;
4. Agent Studio installation summaries;
5. gated write flow after backend readiness.

## 14. Acceptance criteria

The plan is complete when all of the following are true:

- A user can explain the difference between “agent in this Space” and “agent
  installed on this page” from the UI alone.
- A global blueprint never appears as an active Space participant without a
  server-confirmed `SpaceAgentBinding`.
- Space agent cards display only actor-filtered, server-published data.
- Page/system agents receive only their page/system context unless a separate
  binding exists.
- Space and Chat remain separate surfaces with encoded links.
- Agent Studio can show definitions and installations without claiming runtime
  readiness or Space membership.
- Binding lifecycle, runtime lifecycle, failures, approvals, and unknown
  outcomes are rendered truthfully.
- Context, membership, recipient audience, connector, skill, and resource
  authority are rechecked at execution time by the owner planes.
- No browser response contains bearer tokens, raw decisions, service
  credentials, or recipient audience authority.
- Revocation fences new work and leaves an honest historical record.
- Unit, component, integration, and E2E coverage protects both scope paths.
- The Core Sidebar remains the single room/conversation navigator.

## 15. Open decisions

These decisions must be resolved before P4:

1. Is `AgentDefinition` owned by Model, Application, or a dedicated agent
   registry, and which plane owns version promotion?
2. Is `SpaceAgentBinding` persisted by Application or another collaboration
   owner, and which service emits its lifecycle receipts?
3. Which Space roles may add, pause, or revoke an agent?
4. Does a Space agent have one shared thread per Space, one thread per member,
   or both according to `thread_policy`?
5. Which agent profile fields may be overridden per Space without creating a
   new definition version?
6. How are skills and connectors granted to a Space agent without unioning a
   human owner's private credentials into shared work?
7. What is the minimum safe Space Agent projection for the first release?
8. When is the Model computer/workspace durable enough to expose a “watch agent
   work” affordance?
9. How should support/external-page threads be linked to a Space, if at all?
10. What deletion/export receipts are required for definition, binding,
    thread, memory, artifact, connector, and workspace data?

## 16. Recommended next action

Revised 2026-08-16. Steps 3 and 4 of the original list are done; the ordering
below reflects what actually unblocks the product.

1. **Build the invocation model (UI-2b).** `@` mention, `trigger_modes`
   enforcement, mention-to-run routing scoped to Space and thread. Everything
   else is decoration until a person can talk to an agent in a room. This is the
   single highest-value piece of work outstanding.
2. **Add the policy half of the binding** — `trigger_modes`, `allowed_tools`,
   `approval_mode`, `knowledge_scope`, `default_thread_policy`,
   `audit_visibility`. Without it the binding is a label, not a boundary, and
   UI-2b has nothing to enforce.
3. **Settle the open decisions in §15 that block writes** — specifically which
   Space roles may add, pause or revoke an agent (§15.3). UI-3 and UI-3b are
   blocked on Control defining that capability.
4. **Then room creation (UI-3b)**, which is the threshold-lowering the product
   depends on, followed by delegation (UI-2c).
5. **Then Agent-page installation management (UI-4)** and the Chief/Core agent's
   cross-Space view.

The product can look and feel like Grok Bot and Buzz while remaining truthful.
Buzz is the reference for the room and for identity-scoped agents; Grok Bot is
the reference for the teammate ergonomics. Neither is a reason to bypass
Verevon's Space membership, recipient-audience, owner-plane or approval
boundaries — and the difference is not friction, it is the only reason we can
answer who asked whom to do what, on whose authority.
