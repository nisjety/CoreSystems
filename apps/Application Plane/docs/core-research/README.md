# Application Plane Core Research

Generated: 2026-06-09

## Scope

This pass covers the live and adjacent cores under `apps/Application Plane`:

- `convex-core`
- `conversation-core-go`
- `conversation-ingest-rs`
- `information-core`
- `notification-core`
- `velion-gateway-rs`
- `zammad-foundation`

It also tracks the missing `affine-core` build context because it is still declared in compose.

## Current Plane Shape

The Application Plane is broader than a single realtime workspace backend. It currently contains:

- a non-authoritative Convex projection runtime
- a first-party conversation and support API
- a thin Rust ingest adapter into the conversation API
- a small internal information API
- a first-party notification and feed service
- an onboarding BFF in Rust
- a separate Zammad support-stack foundation package

## Live Runtime Surfaces

Primary compose: `apps/Application Plane/docker-compose.yml`

Active/default services:

- `convex-backend`
- `convex-dashboard`
- `convex-gateway`
- `convex-subscriber`
- `velion-gateway-rs`
- `conversation-core-go`
- `conversation-ingest-rs`
- `information-core`
- `notification-core`
- `application-postgres`
- `application-redis`
- `nats`

Adjacent or partial surfaces:

- `affine-core` is declared in compose but `apps/Application Plane/affine-core` is missing on disk.
- `docker-compose.ui.yml` duplicates the `convex-dashboard` exposure already present in the main compose and only adds a small `affine-runtime` port overlay.
- `docker-compose.zammad.yml` is a separate support stack, not part of the always-on main compose.

## Relationships

- `convex-core` mirrors cross-plane state from Control Plane, Model Plane, Ingestion Plane, and Application Plane events for frontend subscriptions.
- `conversation-ingest-rs` normalizes inbound email-style events and forwards them into `conversation-core-go`.
- `conversation-core-go` persists support workflows in shared `application-postgres` and can publish events on NATS.
- `notification-core` persists request, feed, subscriber, and preference state in shared `application-postgres`, uses Redis, and consumes shared-bus identity and control-session events.
- `information-core` is called by `velionv2` through its BFF routes for weather, traffic, and news cards.
- `velion-gateway-rs` fans out to Control, Data, Ingestion, and Model Plane endpoints for onboarding flows.
- `zammad-foundation` provides a separate support stack and bootstrap path; it is adjacent to the main plane rather than embedded in the default runtime.

## Highest-Signal Findings

1. `notification-core` is materially broader than its README. It now owns feed, preferences, channel config, and subscriber sync, not just request intake and event publish.
2. `conversation-core-go` is a real first-party support API with inbox, queue, message, note, assignment, tag, and AI-action review surfaces.
3. `conversation-ingest-rs` is small but live. It validates and canonicalizes inbound mail events before forwarding them to `conversation-core-go`.
4. `convex-core` is live and important, but some webhook and integration surfaces are stale or placeholder-grade.
5. `velion-gateway-rs` is a real onboarding gateway, but current `velionv2` code already proxies the same lower-plane capabilities directly. In-repo callers for the Rust gateway are not evident outside its compose wiring, which makes it a likely transitional or redundant boundary.
6. `affine-core` is a concrete stale runtime surface because the build context is missing.
7. The stale-doc register already points at `apps/Application Plane/APPLICATION_PLANE_ARCHITECTURE.md`, but that file is not present in the current workspace. The register itself needs a truth pass before deletion work starts.

## Stub, Placeholder, and Redundancy Summary

- `convex-core/convex/http.ts` has placeholder webhook verification and references missing `api.jobs.*` functions.
- `convex-core/convex/nats.ts` still contains a placeholder subscriber path while the actual runtime subscriber lives in `nats-subscriber.js`.
- `notification-core` intentionally falls back to stub delivery mode when `NOVU_SECRET_KEY` is unset.
- `zammad-foundation` contains future webhook placeholder guidance and dry-run placeholder IDs, but those are confined to the bootstrap/foundation package.
- `docker-compose.ui.yml` looks redundant with the main compose.
- `velion-gateway-rs` overlaps substantially with `apps/Frontend Plane/velionv2/src/app/api/onboarding/_lib/onboarding-proxy.ts`.

## Files In This Set

- `convex-core.md`
- `conversation-core-go.md`
- `conversation-ingest-rs.md`
- `information-core.md`
- `notification-core.md`
- `velion-gateway-rs.md`
- `zammad-foundation.md`
