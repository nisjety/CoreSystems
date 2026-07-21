# Velion Studio Core — Future Architecture

**Status:** Proposed architecture
**Date:** 2026-07-18
**Owner:** Application Plane (`studio-core`) with Frontend, Model, and Data Plane contracts

## Purpose

Velion Studio will combine:

- Open Design's agent-native creation loop: brief → plan → generate → preview → critique → revise → export.
- Penpot's collaborative design-document model: vector objects, frames, components, variants, design tokens, responsive layouts, inspectable code, and real-time collaboration.
- Velion's own frontend design, organization boundaries, approvals, business data, integrations, Social, Inbox, and agent workflows.

This is a Velion-native architecture. We are not copying either product's desktop shell, frontend, trust model, or service boundaries.

## Product definition

Velion Studio is a collaborative, AI-first workspace where a user can create an editable design, ask Velion to change it, review the result, connect it to real business data, and publish an approved artifact to another Velion workflow.

The canonical source of truth is a structured design document. Generated HTML, SVG, images, decks, PDFs, and videos are compiled artifacts and never the primary editable state.

## Ownership and service boundaries

```text
Velion Studio UI (Frontend Plane)
  ├─ Velion visual language and interaction design
  ├─ canvas/editor, inspector, comments, review states
  ├─ artifact preview and export controls
  └─ chat/copilot entry points

Frontend Gateway / BFF
  ├─ session and organization context
  ├─ typed API normalization
  ├─ signed preview/download URLs
  └─ no direct database access across planes

Application Plane: studio-core
  ├─ projects, workspaces, boards, and revisions
  ├─ canonical design documents and typed operations
  ├─ components, variants, tokens, and libraries
  ├─ collaboration events and presence projections
  ├─ comments, reviews, approvals, and publish intents
  ├─ artifact manifests and export-job state
  └─ ownership, retention, and audit metadata

Model Plane
  ├─ agent runs and design reasoning
  ├─ typed design-operation proposals
  ├─ critique and verification
  ├─ business-tool calls
  └─ HITL enforcement for side effects and publishing

Data Plane / object storage
  ├─ images, video, fonts, uploads, and source assets
  ├─ compiled artifacts and previews
  └─ signed, tenant-scoped object references
```

`studio-core` owns Studio domain state and collaboration projections. It does not own identity, billing, inference, retrieval, or arbitrary external provider credentials.

## Canonical design document

The document model should support, at minimum:

- pages, boards, frames, groups, and layers;
- text, vector paths, shapes, images, video placeholders, and data widgets;
- transforms, constraints, Flex/Grid layout, responsive breakpoints, and z-order;
- components, instances, variants, and overrides;
- references to design tokens rather than uncontrolled literal styling;
- accessibility metadata and semantic roles;
- imported/external asset references with provenance;
- stable node IDs and revision IDs for collaboration and AI patches.

Example AI operation:

```json
{
  "operation": "insert_node",
  "documentRevision": "rev_01",
  "parent": "frame_home",
  "node": {
    "id": "node_hero_title",
    "type": "text",
    "content": "Track your energy usage",
    "styleToken": "heading.large"
  }
}
```

The Model Plane must propose validated operations or operation batches. It must not mutate a project by emitting unvalidated raw HTML or arbitrary filesystem commands. `studio-core` validates the operation against the current revision, authorization, quotas, and document schema before applying it.

## Artifact model

Every generated or imported output gets an immutable artifact manifest containing:

- `artifactId`, `projectId`, `revisionId`, and optional `runId`;
- kind: `prototype`, `dashboard`, `image`, `deck`, `document`, `video`, or `social-pack`;
- source classification: `user`, `ai`, `imported`, `external`, `synthetic`, or `mixed`;
- renderer/compiler version and design-system version;
- MIME type, size, checksum, and object-storage reference;
- preview status, export status, and validation findings;
- creator, organization scope, retention class, and timestamps;
- provenance for external data and generated media.

Preview and download URLs must be short-lived, signed, organization-scoped, and served through an allowlisted renderer. HTML previews require sandboxing and must not inherit application cookies or unrestricted network access.

## Collaboration and revision semantics

The MVP may begin with optimistic revisions and an append-only operation log. It must provide:

1. monotonic revision identifiers;
2. conflict detection and a user-visible merge/retry path;
3. undo/redo as inverse operations or revision navigation;
4. durable audit events for AI and human changes;
5. comments anchored to stable node IDs or revision snapshots;
6. recovery after a disconnected client or worker restart.

CRDT-based multiplayer synchronization can follow once the document schema and operation algebra are stable. Do not introduce collaboration technology before the operation contract is testable.

## AI workflow

The Studio run lifecycle is:

```text
brief
  → discover constraints and data sources
  → select skill/design system/template
  → propose typed document operations
  → validate and apply to a draft revision
  → render sandbox preview
  → critique/accessibility/brand checks
  → user review and HITL approval when required
  → publish or export immutable artifact
```

Runs must preserve the selected model, skill, design-system version, input references, operation list, tool calls, approvals, and artifact manifest. ZDR must propagate through prompts, design documents, assets, previews, traces, exports, and downstream handoffs.

## Integrations

Studio should publish explicit, typed handoffs rather than directly coupling to downstream databases:

- Social: create an approved draft or campaign asset;
- Inbox/conversation: attach an approved visual or response asset;
- Model Plane: launch a design run or critique run;
- Data Plane: retrieve authorized source data or persist source references;
- Ingestion Plane: import authorized external assets/evidence;
- Control Plane: enforce organization membership, role, entitlements, and retention policy.

External data must carry provenance, freshness, and synthetic/estimated/measured classification. Studio must never present generated values as observed facts.

## MVP scope

1. Durable organization-scoped Studio projects in `studio-core`.
2. Structured canvas document with frames, text, shapes, images, and components.
3. Design tokens and a versioned organization design system.
4. Typed AI operations with schema validation, revision checks, and audit events.
5. Sandboxed HTML/SVG preview.
6. Artifact manifests and signed downloads.
7. SVG and HTML export first; PDF/PPTX through asynchronous export jobs next.
8. Comments, review status, and HITL approval before external publication.
9. Social/campaign handoff through existing Application Plane contracts.
10. Unit, contract, integration, browser E2E, tenant-isolation, and ZDR tests.

## Future scope

- Penpot-compatible import/export where the licensing and format contract permit it;
- real-time multi-user editing with presence and conflict-free operations;
- component libraries and organization-level publishing workflows;
- plugins and MCP with strict allowlists and tenant authorization;
- Figma/import migration tools;
- data-connected dashboards with explicit provenance;
- decks, PDF/PPTX, video, HyperFrames, and media-generation workers;
- enterprise retention, legal hold, data residency, policy-as-code, and compliance exports.

## Security and trust requirements

- Derive organization and user scope from validated session identity; caller headers never grant scope.
- Enforce authorization on every project, revision, artifact, comment, export, and publish operation.
- Validate all node content, URLs, asset references, file sizes, and export parameters.
- Prevent SSRF, script escape, cookie access, and unrestricted network access from previews.
- Use idempotency keys for operation batches, exports, and downstream handoffs.
- Rate-limit AI operations and exports; enforce organization quotas and cancellation.
- Redact secrets and sensitive content from logs and telemetry.
- Make retention and ZDR behavior explicit at each persistence boundary.
- Require HITL for publishing, external messages, social posts, and other consequential actions.

## Initial API shape

The gateway should expose a typed BFF contract, while `studio-core` owns the domain API:

```text
GET    /api/v1/studio/projects
POST   /api/v1/studio/projects
GET    /api/v1/studio/projects/:projectId
POST   /api/v1/studio/projects/:projectId/operations
GET    /api/v1/studio/projects/:projectId/revisions
POST   /api/v1/studio/projects/:projectId/runs
GET    /api/v1/studio/projects/:projectId/artifacts
POST   /api/v1/studio/projects/:projectId/exports
POST   /api/v1/studio/projects/:projectId/publish-intents
POST   /api/v1/studio/projects/:projectId/comments
```

All mutating endpoints require an idempotency key, validated organization scope, revision/concurrency checks, and a consistent error envelope.

## Migration from the current Studio

The current Velion Studio canvas and gateway endpoints are a prototype surface. Migration should be staged:

1. Keep the existing UI route and API response shape behind a compatibility adapter.
2. Introduce the canonical document and artifact contracts in shared types.
3. Move persistence from gateway-local memory into `studio-core`.
4. Add revision-aware save and operation endpoints.
5. Replace block-only editing with the structured canvas incrementally.
6. Connect Model Plane runs and artifact manifests.
7. Remove the compatibility store only after live tenant-isolation and recovery tests pass.

Do not copy Open Design's local daemon or Penpot's entire frontend/backend into the monorepo. Reuse ideas and separately licensed components only after a file-level license review, while keeping Velion's own frontend design and plane ownership.

## Acceptance criteria for implementation

- A user can create, edit, undo, review, and reload a Studio project without losing revisions.
- AI changes are typed, attributable, reversible, and rejected when unauthorized or stale.
- Two organizations cannot read or mutate each other's projects, artifacts, comments, or exports.
- Preview and download links are signed and expire.
- A failed export or downstream publish cannot appear successful.
- Synthetic/generated data is visibly classified and never serialized as measured data.
- HITL cannot be bypassed through direct API, worker, retry, or queue paths.
- Changed critical modules have measured coverage and passing unit, contract, integration, and E2E suites.
- Deployment and rollback are documented before replacing the current gateway-local prototype.

