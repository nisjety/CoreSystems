# ADR-0001: Canonical Space identity and authority

**Date**: 2026-08-13  
**Status**: accepted for implementation; release remains gated by the
verification requirements below  
**Deciders**: CoreSystem product direction, Frontend, Application, Control,
Data, Model, Ingestion, and Infra implementation owners

## Context

Verevon has an organization selector, browser-local support-thread mappings,
Studio projects, Application projects, Data workspace filters, Control session
workspaces, and Model context workspace fields. They do not express the same
resource or authority boundary. QM demonstrates the product value of a single
person/room scope, but its string scopes and single-organization assumptions
are not acceptable for CoreSystem's multi-tenant privacy and exact-effect
authorization requirements.

The system needs a stable collaboration scope for the product promise “one
agent, one workspace, one set of permissions, one work queue, and one presence
across web and future surfaces” without moving identity, Data knowledge,
Model runs, or domain effects into a new cross-plane database.

## Decision

Use `Space` as the canonical collaboration identity:

```text
SpaceRef = { org_id, space_id, kind }
kind     = personal | room | project | case
```

Application Plane owns the canonical Space aggregate: immutable ID, kind,
name, lifecycle, and collaboration metadata. Control Plane registers each
immutable `SpaceRef`, owns memberships, grants, entitlements, privacy decisions
and their revisions, and issues signed target-service `SpaceAccessDecision`s.
Application may project Control events only to narrow access and must fail
closed when Control authority is unavailable.

Creation is an idempotent Application-to-Control registration saga. A Space is
`pending_registration` until Control acknowledges it; it is never usable on
the basis of an Application projection alone. Application also owns the
delete/export request, tombstone, outbox event, and receipt aggregation;
every plane owns its purge/export adapter and truthful result.

There is exactly one personal Space per `(org_id, principal_id)`, enforced with
a database uniqueness constraint. Personal Space content, credentials, memory,
and thread mappings are never reused across organization memberships.

### Authority decision requirements

Control decisions bind the authenticated subject, Space, target service,
recipient audience reference/hash/revision, aggregate authority revision,
membership/grant/entitlement/privacy revisions, purpose, lawful basis, privacy
class, third-party-processing allowance, retention, residency, deletion scope,
ZDR, and expiry. `service_audience` is the downstream workload audience; it is
not the conversation's human recipient list.

The owning conversation/case service supplies the current recipient audience.
Control verifies each recipient and binds the revision to the decision. Owner
planes must re-check the decision immediately before an effect, a resumed
approval, or a scheduled fire.

Space membership does not widen a linked resource's ACL. Effective access is:

```text
current Space decision
∩ current recipient-audience authorization
∩ current owner-plane resource authorization
```

The same privacy fields travel with retrieval, provider work, artifacts,
snapshots, delivery, schedules, and ingestion. Client headers and request body
fields select a Space but never establish actor, recipient, organization, or
authority.

## Current identifier inventory

| Existing identifier or concept | Current owner/source | Classification | Migration rule |
|---|---|---|---|
| V3 `activeOrg` and shell “workspace” | `Frontend Plane/verevonv3/src/shared/session/session-store.ts` | tenant partition | Remains the organization policy layer; it is not a Space and cannot grant a Space. |
| Browser support chat mapping | `src/shared/chat/support-chat-thread.ts` | maps to future case Space | Replace session-storage mapping with a server-owned case Space/thread binding; retain compatibility only during migration. |
| V3 Studio `projectId` | `apps/gateway/src/domains/studio.rs`, `src/shared/api/studio-client.ts` | resource inside Space | Studio project stays a creative artifact/campaign resource and carries a `space_id` reference after the contract exists. |
| Application Convex `projects` | `Application Plane/convex-core/convex/projects.ts`, `schema.ts` | migration candidate | Map each record explicitly to an Application Space aggregate or retain it as a domain project inside one; do not create another generic project authority. |
| Application planner `workspaceId` | `Application Plane/convex-core/convex/plannerDocuments.ts` | resource-local partition | Preserve existing document semantics and add a canonical Space mapping only after owner/resource authorization is explicit. |
| Data `workspace_id` / retrieval workspace filters | `Data Plane v2/proto/retrieval_v2.proto`, `services/wiki-store-go` | knowledge partition | Map through a versioned Data-owned `SpaceRef → workspace/collection` relation; never assume same string equals same authority. |
| Control session-core `workspace_id` | `Control Plane/session-core/migrations/001_init.up.sql` | session partition | Keep as its service-local partition until an explicit mapping is written; it cannot define Space membership. |
| Model Session Core `workspace_id` | `Model Plane/proto/model_plane/v1/sessions.proto` and Session Core context assembly | context-selection hint | Treat as content selection only. Add `space_id` and decision references separately; no implicit authorization migration. |
| Capability Core workspace scope | `Model Plane/go/services/capability-core/internal/api/workplane_apis.go` | unsupported capability scope | Do not treat as live Space support; complete a controlled Space resolver before enabling it. |
| Onboarding connector `workspaceId = org_id` | `Frontend Plane/verevonv3/apps/gateway/src/onboarding/actions/connectors.rs` | tenant convenience alias | Replace only with an explicit organization/Space target chosen and authorized server-side. |

## Alternatives considered

### Control-owned Space rows

- **Pros**: One store could hold lifecycle and grants.
- **Cons**: Makes Control a collaborative project/workspace owner and conflates
  resource existence with authorization.
- **Why not**: Application already owns collaboration projections and lifecycle
  behavior; Control must remain the exact authority, not the product aggregate.

### Reuse a Data `workspace_id` as `space_id`

- **Pros**: Existing retrieval partitioning.
- **Cons**: Couples collaboration membership to knowledge storage and changes
  the meaning of existing Data identifiers.
- **Why not**: Data owns durable knowledge, not identity or collaboration
  lifecycle. One Space can legitimately link several Data partitions.

### Keep current per-feature project/thread keys

- **Pros**: Lowest immediate migration effort.
- **Cons**: Keeps action, run, delivery, presence, and authorization fragmented.
- **Why not**: It cannot provide one durable shared agent context or safe
  cross-surface continuity.

## Consequences

### Positive

- Web, future Slack, Model runs, Data retrieval, and Application activity can
  converge on one typed identifier without direct database crossing.
- Revocation is bounded by decision expiry/revision and applies to human,
  workload, resumed, and scheduled effects.
- Domain-private documents, tickets, connectors, and credentials cannot become
  visible merely because they are linked to a Space.

### Negative

- Creation and deletion are cross-plane sagas with explicit pending/unknown
  states, not one transaction.
- Existing workspace/project callers need mapping tables, outbox consumers,
  dual reads, reconciliation, and a rollback window.
- Resource owners must expose decision references instead of assuming Space
  membership is enough.

### Risks and release gates

- No effectful Space milestone may bypass the Model/Application release gates
  recorded in `Frontend Plane/verevonv3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md`.
- R-1 through R-5 must prove canonical delegation, capability health, approval
  continuation/unknown-effect recovery, durable delivery, and immutable
  rollback against the exact release artifact.
- An ADR acceptance does not certify a deployed runtime. Each later sequence
  must provide its own source, integration, E2E, and live evidence before it
  is marked complete.
