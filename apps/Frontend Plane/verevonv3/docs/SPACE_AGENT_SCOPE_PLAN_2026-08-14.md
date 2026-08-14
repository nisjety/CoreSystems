# Space agents and system agents

## Implementation plan

**Date:** 2026-08-14  
**Owner:** Verevon Frontend Plane, with Application, Control, Model, Data, and
agent-surface owners  
**Status:** Proposed product and contract plan  
**Related research:**
`/Volumes/Lagring/Triodelab/CoreSystem/apps/VEREVON_UI_COWORK_RESEARCH_2026-08-13.md`  
**Related frontend docs:**
`docs/SPACE_COCKPIT_WIRING_2026-08-13.md`,
`docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md`

> This document defines the boundary between agents that collaborate inside a
> Space and agents that are installed on pages or system surfaces. It is a
> planning document, not proof that the proposed backend contracts exist.

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

### Required backend gaps

Before the Space Agent tab can show active collaborators, the owner planes
must publish:

1. actor-filtered agent binding projections;
2. server-authorized create/update/revoke intents;
3. a binding-to-runtime resolution path;
4. agent-visible roster identity and status;
5. Space-scoped thread/run/activity correlation;
6. connector, skill, knowledge, and workspace scope references;
7. revocation and deletion receipts appropriate to each owner plane.

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

### Phase UI-3: Space binding flow

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
  Space installations
  Page/system installations
  Runs and receipts
```

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

## 11. Permissions and privacy model

### Space agents

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

1. Add this plan to the research-doc index and review it with Application,
   Control, Model, and Agent Studio owners.
2. Approve the scope matrix and the “definition versus binding” terminology.
3. Implement P2's read-only `SpaceAgentProjection` contract before adding any
   Space agent creation UI.
4. Land the frontend Agent tab with an honest unavailable state and fixture
   tests.
5. Only after P4 is accepted, expose “Add agent to Space.”

The product can look and feel like Grok Bot and Buzz while remaining truthful:
Grok/Buzz are inspirations for the Space coworker experience; they are not a
reason to bypass Verevon's Space membership, recipient-audience, owner-plane,
or approval boundaries.
