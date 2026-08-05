# Channel Plane Deep Dive

## Executive Summary

The Channel Plane is not an active runtime plane in the current CoreSystem workspace. It is a docs-only future capability area.

At the moment, the on-disk surface is:

- `apps/Channel Plane/docs/vision.md`
- an incidental `.DS_Store` file

There are no current services, compose stacks, code modules, APIs, migrations, or tests under this plane. The only substantive artifact is a product and architecture vision document describing a deferred future runtime for external-facing deployed agents.

That means the current honest assessment is:

1. Channel Plane exists conceptually.
2. Channel Plane does not yet exist as a live system plane.
3. The current docs are aligned with that fact rather than pretending otherwise.

## Current Runtime Topology

There is no active runtime topology under `apps/Channel Plane`.

### Present on disk

| Path | Role |
|---|---|
| `docs/vision.md` | future product/runtime direction |
| `.DS_Store` | macOS metadata noise, not system content |

### Absent today

- compose files
- service directories
- API packages
- migrations
- tests
- deploy manifests
- runtime configuration

## Plane Boundary and Intended Ownership

Based on `docs/vision.md`, the intended future role of Channel Plane is the external-facing runtime and deployment surface for Verevon agents.

It is intended to own:

- widget bootstrap
- external visitor identity bootstrap
- public conversation runtime
- channel adapters such as website, Shopify, WooCommerce, and WordPress
- inbox and operator handoff operations for external-facing conversations
- realtime chat state

It is intended not to own:

- foundational identity and governance
- billing and quotas as a hot-path authority
- canonical organization/user/auth state
- core AI reasoning ownership

Those remain elsewhere in the architecture.

## Current Service-by-Service Understanding

There are currently no services to audit.

Instead, the future service intent named in `docs/vision.md` is:

- `adapter-core`
- `widget-core`
- `conversation-core`
- Convex-backed realtime layer
- Postgres canonical storage for compliance-sensitive records

These are proposals, not live code.

## Stub / Mock / Placeholder / TODO Audit

### Current state

- The entire plane is effectively a deferred placeholder in architectural terms.
- That placeholder is explicit and documented, not hidden.

### Concrete findings

1. `docs/vision.md`
   - explicitly says Phase 1 is to not build Channel Plane yet.
   - documents future scope, not current runtime truth.

2. `.DS_Store`
   - non-functional filesystem artifact.

There are no code TODOs, mocks, stubs, or runtime placeholders because there is no current runtime code here.

## Relationship Mapping Assessment

Relationship intent is documented, but relationship implementation is absent.

### Mapped conceptually

- Control Plane should own governance/configuration.
- Channel Plane should become the hot path for public visitor/channel runtime.
- Verevon workspace would remain the parent control surface.

### Not mapped in code

- no APIs
- no event contracts
- no persistence schema
- no runtime adapters
- no actual integration boundaries

## Stale-Doc Candidates

At this time, `docs/vision.md` is not a stale doc candidate. It is still accurate as a future-facing deferral document.

The only deletion-ready artifact is:

| Path | Why |
|---|---|
| `apps/Channel Plane/.DS_Store` | non-source metadata noise |

## Bottom Line

Channel Plane is currently a deferred architectural idea, not an implemented plane. The workspace reflects that honestly. There is nothing to map for active runtime relationships yet beyond the future intent captured in `docs/vision.md`.
