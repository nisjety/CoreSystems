# ADR-0002: Cross-Space agent registry ownership

**Date**: 2026-08-19
**Status**: accepted for implementation
**Deciders**: CoreSystem product direction (Application/Frontend implementation
owner)
**Supersedes nothing; extends** `apps/SPACE_AUTHORITY_ADR_2026-08-13.md`
(ADR-0001) — this decision assumes ADR-0001's Space/authority model as given
and does not revisit it.

## Context

UI-4's narrow slice (`apps/Frontend Plane/verevonv3/docs/SPACE_AGENT_SCOPE_PLAN_2026-08-14.md`,
"Phase UI-4") shipped `GET /api/v1/agents/installations` and the
`/agents/installations` page by looping `control_space_index` +
`compose_space_agents` once per Space the caller belongs to (`join_all` over
N Spaces) — deliberately avoiding a new cross-Space index so the slice could
ship without a registry-ownership decision blocking it.

The rest of UI-4 needs an actual org-wide read, not a per-Space loop:

- The blueprint showcase restructure and Page/system installations view need
  to answer "which agent definitions are installed anywhere in this org" in
  one call, not N.
- The Chief/Core agent's cross-Space routing view needs the same org-wide
  answer to route a request to the right Space/agent without enumerating
  every Space first.

Every doc that touches this (`SPACE_AGENT_SCOPE_PLAN_2026-08-14.md`,
`VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md`'s "Post-UI-4 next
sequence", `space + qm style improvements.md`'s 2026-08-17 reconciliation)
independently arrives at the same blocker: decide who owns this registry
before building it, or the per-Space loop calcifies into the permanent
implementation by default.

## Decision

**Application Plane owns the cross-Space agent registry**, as a new
org-scoped Convex query alongside the tables it already owns.

Reasoning, in order of weight:

1. **Application Plane already owns both source tables.** The Space
   aggregate (`spaces`, `convex-core/convex/schema.ts:527`) and the agent
   binding/definition projections (`spaceAgentBindings`,
   `convex-core/convex/schema.ts:581`; `agents`,
   `convex-core/convex/schema.ts:305`) all live in the same Convex database.
   A registry query is a same-store extension, not a new cross-plane
   dependency.
2. **The aggregation pattern already exists in Application Plane.**
   `spaces:spacesForOrgForGateway` (read via
   `apps/gateway/src/domains/spaces.rs:2299-2315`) is already an org-wide,
   unfiltered read over the `spaces` table, keyed only on `externalOrgId`.
   The new registry is the same shape applied to `spaceAgentBindings`/
   `agents` instead of `spaces`.
3. **The alternative reads the wrong direction.** Control Plane
   (`user-core`) owns Space *membership and authorization*
   (`control_space_index`, the per-Space roster endpoint used inside
   `compose_space_agents`) but holds no binding/definition data at all.
   Control owning the registry would mean a new cross-plane read into
   Application's Convex store for data it doesn't have — backwards relative
   to ADR-0001's stated dependency direction ("Application may project
   Control events only to narrow access... must fail closed when Control
   authority is unavailable"; the dependency runs Application-depends-on-
   Control-for-authority, not Control-depends-on-Application-for-content).

### The registry answers presence, not authority

This is a design requirement of the decision, not a separate concern to
revisit later. Per ADR-0001, Space membership does not widen a linked
resource's ACL, and `compose_space_agents` already joins Control's roster
*before* returning any binding (`spaces.rs:1424-1435`: "Control answers who
may act in this room... Application answers what that agent is called").
The new org-wide registry inverts that order — it reads Application data
first, across every Space, before any single Space's roster is known — so
it MUST NOT be treated as an authorization surface on its own. Concretely:

- The registry's response is scoped to `org_id` only (which the caller's
  verified session already establishes) and returns binding/definition
  *presence* (name, title, kind, lifecycle, which Space it's bound in) —
  never an implicit "and therefore the caller may invoke it."
- Any UI or agent surface reading the registry (Page/system installations,
  Chief/Core routing) must still resolve the caller's actual Space
  membership/roster via Control before treating a listed binding as usable,
  exactly as the existing Chief/Core agent view is already scoped
  ("broad visibility is not broad authority",
  `SPACE_AGENT_SCOPE_PLAN_2026-08-14.md:835-839`).
- The registry is additive to `compose_space_agents`, not a replacement for
  it: single-Space, in-room agent invocation keeps resolving through the
  existing Control-roster-first path unchanged.

## Alternatives considered

### Control Plane owns the registry

- **Pros**: Colocates the registry with the plane that already answers "is
  this Space membership real" — arguably a natural home for an
  authorization-adjacent read.
- **Cons**: Control holds no agent binding/definition data today; it would
  need a new, standing cross-plane read into Application's Convex store
  purely to re-project content it doesn't own, then would still need to
  call back into itself for the roster check — two hops where Application
  ownership needs one.
- **Why not**: Violates "no direct database crossing between planes" in the
  more expensive direction, and duplicates data Application already
  projects correctly for the single-Space case.

### Keep the per-Space loop indefinitely

- **Pros**: Zero new code; already shipped and live-verified.
- **Cons**: O(N) calls to `user-core` per registry read, scales with an
  org's Space count, and gives the blueprint/Page-installations/Chief-Core
  surfaces no way to ask "anywhere in this org" without first knowing the
  full Space list.
- **Why not**: This is exactly the calcification risk every doc referencing
  this decision flagged — the workaround was explicitly scoped as narrow
  and temporary when UI-4's first slice shipped.

## Consequences

### Positive

- Unblocks the rest of UI-4 (blueprint restructure, Page/system
  installations, Chief/Core cross-Space routing) without a new cross-plane
  dependency.
- One query replaces an N-call loop for any surface that needs an org-wide
  view.
- Matches the existing `spacesForOrgForGateway` precedent exactly, so the
  gateway-side wiring, auth extraction, and error handling all mirror
  code already in production rather than inventing a new pattern.

### Negative

- A second read path (org-wide registry vs. per-Space `compose_space_agents`)
  now exists for agent-binding data; both must be kept in sync by hand if
  the binding schema changes (same class of duplication-discipline
  obligation already accepted elsewhere in this codebase, e.g.
  execution-core's `provenance.rs` deliberately duplicating
  `moderation.rs`'s vocabulary).
- Any future consumer of the registry that skips the "presence, not
  authority" requirement above would reintroduce the exact widened-ACL risk
  ADR-0001 was written to close. Code review on any new consumer of this
  registry must check for a Control roster/authorization call before the
  binding is treated as invokable.

### Risks and release gates

- Same release-gate obligation as every other Space milestone: no effectful
  surface built on this registry may bypass the Model/Application release
  gates in `Frontend Plane/verevonv3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md`.
- This ADR authorizes building the registry as a *read* surface. It does not
  authorize any new effectful (invoke/bind/unbind) cross-Space endpoint —
  those remain governed by the existing per-Space, Control-roster-first
  path until a separate decision says otherwise.
