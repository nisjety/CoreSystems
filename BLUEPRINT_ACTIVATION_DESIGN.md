# Blueprint Activation — Design Doc

**Status:** Proposed · **Date:** 2026-08-17 · **Scope:** cross-plane (Application Plane / Model Plane / Frontend Plane)

Turning a Blueprint (a role template shown on the Agents page) into a real,
per-org, capability-granted, invocable agent.

---

## 1. Problem

`apps/Frontend Plane/verevonv3/src/features/agents/components/AgentsPage.tsx`
shows five role Blueprints — `service`, `sales`, `ecommerce`, `chatbot`,
`workflow` (`lib/agent-roles.ts:1`) — each with a disabled **Activate** button
and a `DesignPreviewBadge` reading *"Blueprint / not yet configured for this
org."* The code comment (`AgentsPage.tsx:232-237`) says the real per-org
agent-config store is "deferred to Phase 5."

That comment is now the only thing that's out of date. The stores exist. What
is missing is the **transaction that binds them** — and, more importantly, a
decision about **which store is the authority on what an agent may do**, because
there are currently three candidates and no ruling.

## 2. Current state (verified 2026-08-17)

### 2.1 What already exists — more than the badge implies

| Piece | Where | State |
|---|---|---|
| Per-org agent config store | Convex `agents` table (`apps/Application Plane/convex-core/convex/schema.ts:305`) | **Real.** Full CRUD in `agents.ts` (`create`/`update`/`getById`/`listByOrg`), fields for `model`, `temperature`, `systemPrompt`, `tone`, `greeting`, `tools`, `knowledgeSources`, `useCase`, `status` (`active`/`inactive`/`draft`), plus a public-embed widget with a rotating secret. |
| Agent-scoped capability grants | capability-core `capability_scopes` table, `registry.ScopeStore` (`go/services/capability-core/internal/registry/scope_store.go`) | **Real.** `ScopeKindAgent = "agent"` is a first-class scope kind (`:30`); `Grant`/`Revoke`/`IsGrantedForScope`/`ResolveForScopeForOrg` all implemented (`:72`–`:189`). |
| HTTP surface for grants | `/api/v1/capabilities/scopes{,/grant,/revoke,/resolve}` (`internal/api/capabilities.go:57-60`) | **Real and org-safe.** `grantScope` derives org from `verifiedOrganizationID(r)` and rejects anything else with 403 "only tenant-derived org grants are supported" (`:330`). |
| Grant enforcement at dispatch | capability-core policy engine, consulted fail-closed before every tool dispatch | **Real.** `engine_grant_test.go` covers agent-scoped grants both wildcard (`*`) and specific (`agent-7`). |
| Deployed-vs-chat execution posture | `agents.profile: 'chat' \| 'deployed_agent'` (`agents.ts:46`) | **Real and enforced.** `model-gateway/src/profile.rs:40` maps `deployed_agent` → `ask` posture; `execution-core/src/permission/mod.rs:60` gates risky/destructive tools under it. |

So both halves of "activation" — a config record and a capability grant —
already exist, are org-scoped, and are enforced. Activation is the missing
*seam*, not a missing subsystem.

### 2.2 The actual problem: three unreconciled notions of "what may this agent do"

1. **`capability_scopes` rows** (`scope_kind='agent'`, `scope_value=<agentId>`) —
   Postgres, org-scoped, **fail-closed at dispatch** by the policy engine.
2. **`spaceAgentBindings.allowedTools`** (Convex, `schema.ts:604`) — space-scoped,
   enforced at the *gateway* by rewriting the outbound request
   (`apps/gateway/src/domains/spaces.rs:1877` `apply_mention_binding_policy`).
3. **`agents.tools: string[]`** (Convex, `schema.ts:331`) — free-text array with
   **no confirmed enforcement anywhere**.

Three lists, two enforcement points, one of them unenforced. This is precisely
the duplication `docs/capability-ownership-matrix.md` exists to prevent, and it
is the same class of defect the harness audit kept surfacing (a field that
*looks* authoritative but nothing reads). Activation must **resolve** this, not
add a fourth list.

## 3. Design

### 3.1 Ruling: one authority, two projections

> **`capability_scopes` (`scope_kind='agent'`) is the sole authority on what an
> activated agent may invoke.** Everything else narrows it or displays it.

- `agents.tools[]` becomes a **read-only projection** written *by* activation
  from the grants that actually succeeded — never hand-edited, never read for
  an authority decision. It exists so the UI can render "this agent can do X"
  without a Postgres round-trip.
- `spaceAgentBindings.allowedTools` stays, but is redefined as a **narrowing
  filter only**: it may subtract from the agent's grants for that Space; it can
  never add. A tool present in the binding but absent from the grant is denied
  (and logged as a misconfiguration).
- Effective authority for a call = `grants(agent) ∩ binding.allowedTools?` —
  with the intersection computed at the enforcement point, not the display
  point.

This makes the existing gateway request-rewriting a *defense-in-depth* layer
rather than the primary gate, which is the correct relationship: the gateway
narrows, capability-core decides.

### 3.2 What a Blueprint is

A Blueprint is a **versioned, code-defined template**, not a DB row and not
user-authored:

```
BlueprintTemplate {
  id: 'service' | 'sales' | 'ecommerce' | 'chatbot' | 'workflow'
  version: semver              // bumped when the requested set changes
  defaultModel, defaultSystemPrompt, defaultTone, defaultGreeting
  requestedCapabilities: CapabilityId[]   // FIXED ALLOWLIST, reviewed as code
  defaultKnowledgeScope
  profile: 'deployed_agent'    // Blueprints always activate ask-posture
}
```

**Blueprint capability requests MUST be a fixed, code-reviewed allowlist.** If a
Blueprint could request arbitrary capabilities, activation becomes a
privilege-escalation path: anyone who can click Activate could mint themselves
authority. Keeping the set in code means changing it goes through code review
and the risk-floor rules capability-core already enforces
(`ErrRiskFloorViolation`, `capabilities_store.go:28`).

### 3.3 Activation is a saga, ordered so every failure is safe

Cross-plane (Convex + Postgres) means no distributed transaction. Order the
steps so **every partial failure leaves a fail-closed state**:

```
1. CREATE   Convex agents row, status='draft', profile='deployed_agent',
            blueprintId + blueprintVersion + activationId (idempotency key)
            → a draft agent is not invocable, so it holds zero authority

2. GRANT    For each capability in the Blueprint's requested set:
              POST /api/v1/capabilities/scopes/grant
                { capability_id, scope_kind: 'agent', scope_value: <agentId> }
            Record which succeeded, which were withheld, and why
            → grants attached to a draft agent are inert

3. COMMIT   Convex agents.update:
              status='active', tools=<granted capability ids>,
              activationReport=<withheld + reasons>
            → this is the only step that makes the agent invocable
```

Failure analysis:

| Fails at | Resulting state | Safe? |
|---|---|---|
| 1 | Nothing created | Yes |
| 2 | Draft agent, partial grants | Yes — draft is not invocable; grants are inert |
| 3 | Draft agent, full grants | Yes — same; retry is idempotent on `activationId` |

**Deactivation reverses the order** so it is equally safe:

```
1. Convex agents.update status='inactive'   ← stops invocation immediately
2. Revoke every agent-scoped grant           ← cleanup
```

Failing between 1 and 2 leaves an inactive agent holding grants: inert, and
caught by the reconciler.

### 3.4 Honest partial activation

Following the pattern the harness audit recommended (report enforcement as a
fact, never assume it): if a Blueprint requests six capabilities and the org
only has four available, **activation succeeds with four and says so**. It does
not silently drop two, and it does not fail the whole activation.

`activationReport` records per withheld capability: `not_registered`,
`risk_floor_blocked`, `org_policy_denied`, or `unavailable`. The UI renders
this as "Active — 4 of 6 capabilities granted" with the reasons, rather than a
green checkmark that overstates what happened.

### 3.5 Reconciler

A periodic job (Model Plane already runs Temporal; this is a natural
`AgentGrantReconciliation` workflow) that detects and reports drift:

- grants whose `scope_value` names a nonexistent or non-active agent → revoke
- an `active` agent with zero grants → flag (activation died at step 2)
- `agents.tools[]` disagreeing with actual grants → rewrite the projection

The reconciler **reports before it repairs** for anything destructive; only the
projection rewrite is automatic.

### 3.6 Authorization

Activation is an admin action. Precedent exists in both planes —
`hasWorkspaceAdminAccess` in the SPA, `can_author_skills`'s admin-role gate in
`apps/gateway/src/domains/agent_actions.rs:66`. Activation and deactivation
require the same gate, and every activation writes an audit row (capability-core
already has an audit log: `registry/audit_log_integration_test.go`).

### 3.7 Relationship to Spaces (answering the open question from the audit)

Activation is **org-scoped and Space-independent**. Binding an activated agent
into a Space is a separate, additive step that already exists
(`spaceAgentBindings`, `spaceAgents.ts:152`). An agent can be active and
invocable via the embed widget or a channel without ever being bound to a
Space, and a Space binding can only narrow its authority (§3.1).

This preserves the settled product rule that **a mention invokes, never
grants** — the mention path reaches an agent whose authority was decided at
activation time, not at mention time.

## 4. API surface

New (Application Plane, called by the gateway with the existing `serviceKey`
pattern — never directly from the browser):

- `agents.activateBlueprint({ orgId, blueprintId, blueprintVersion, activationId, overrides? })`
- `agents.deactivate({ orgId, agentId })`
- `agents.getActivationReport({ orgId, agentId })`

New (Frontend Plane gateway, admin-gated, org from session never from client):

- `POST /api/v1/agents/blueprints/:blueprintId/activate`
- `POST /api/v1/agents/:agentId/deactivate`

Reused as-is (Model Plane, no changes needed):

- `POST /api/v1/capabilities/scopes/grant`
- `POST /api/v1/capabilities/scopes/revoke`
- `GET  /api/v1/capabilities/scopes/resolve`

## 5. Out of scope

- **The Workflow Blueprint.** `workflow` is in the role list but
  `WorkflowCanvas.tsx:44-50` states plainly there is no DAG-runner substrate to
  activate into ("the model plane is an LLM agent loop, not an n8n DAG
  runner"). Activating it would produce an agent that cannot do the thing its
  Blueprint promises. **Ship activation for `service`, `sales`, `ecommerce`,
  and `chatbot`; leave `workflow` badged as a design preview** until a workflow
  substrate exists or the Blueprint is redefined as an agent rather than a DAG.
- Per-agent fine-tuning, per-agent cost budgets, agent versioning/rollback,
  multi-agent teams. All are reasonable follow-ups; none block activation.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Privilege escalation via Blueprint | Requested capability sets are fixed in code, reviewed, and subject to capability-core's existing risk floor |
| Two authorities drift again | §3.1 ruling recorded in `capability-ownership-matrix.md`; reconciler enforces the projection |
| Cross-plane partial failure | Saga ordering (§3.3) makes every partial state fail-closed; `activationId` makes retry idempotent |
| `agents.tools[]` keeps being hand-edited | Remove it from the `update` mutation's args; it becomes activation-written only |
| Activation looks successful but grants silently failed | `activationReport` + "N of M granted" UI (§3.4) |

## 7. Plan

**Phase 1 — resolve the authority question (do this first, independent of UI).**
Record the §3.1 ruling in `docs/capability-ownership-matrix.md`. Make
`spaceAgentBindings.allowedTools` narrowing-only at the enforcement point. Stop
accepting `tools` in `agents.update`. *This is worth doing even if activation
ships later — it closes a live inconsistency.* ~1 week.

**Phase 2 — activation saga.** Blueprint templates in code, the three Convex
mutations, gateway routes, admin gate, audit rows, `activationReport`.
~2 weeks.

**Phase 3 — UI.** Enable the Activate button, render "N of M granted" with
withheld reasons, wire deactivate, replace `DesignPreviewBadge` with a real
status badge on the four shippable roles. Per this repo's convention, badge
removal and control enablement land in the same change. ~1 week.

**Phase 4 — reconciler.** Temporal `AgentGrantReconciliation`. ~1 week.

Phases 2–4 assume Phase 1's ruling is settled. Total ≈ 5 weeks, one engineer,
touching three planes — the cross-plane coordination is the schedule risk, not
the code volume.
