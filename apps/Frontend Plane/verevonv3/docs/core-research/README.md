# Verevon v3 Core Research

Generated: 2026-06-09
Updated: 2026-07-13 (Model Plane credential/capability correction; 2026-07-11
full-plane evidence retained)

> Plain chat is intentionally tool-free. Frontend/Auth Core source now forwards
> separate exact-audience credentials with fail-closed issuance, but the live
> Model inference path is down and the authoritative capability-state UX is not
> complete. See
> [MODEL_PLANE_CAPABILITY_HANDOFF_2026-07-13.md](../MODEL_PLANE_CAPABILITY_HANDOFF_2026-07-13.md).

## Scope

This pass covers `apps/Frontend Plane/verevonv3` only. verevonv2 is deprecated.

Verevon v3 is the canonical Frontend Plane target: a SolidJS + Vite + TypeScript SPA with a Rust Axum same-origin BFF gateway under `apps/gateway`.

Latest plane audit: `plane-audit-2026-07-11.md` (renamed from `-07-02`; 2026-07-11 synthesis leads). See also `../../FRONTEND_PLANE_STATUS.md` and `../../FRONTEND_PLANE_ROADMAP.md`.

**2026-07-11 headline — this supersedes the "Highest-Signal Findings" and "Stub/Mock" sections below, which are now OBSOLETE.** verevonv3 is **fully migrated off mocks**: `src/shared/mocks/` and `verevon-operating-model.ts` are gone (zero refs), `src/shared/graphrest/` and its hardcoded graph are gone, and `src/shared/api` has **56 real client files** (not empty). All 20 `src/features/*` areas call real `/api/v1/*` endpoints; the ~50-domain Rust gateway is real throughout; auth is real (not "presentation-only"); the `x-verevon-org-id` IDOR is fixed + tested. So `mock-backed-surfaces.md` is obsolete (→ delete). New findings this pass: a HIGH `onboarding/graph-preview` query-param cross-tenant IDOR, the chat tool-surfacing gap + a HITL-bypass on composer-selected writes, and two residual fabricated settings rows — all detailed in `plane-audit-2026-07-11.md`.

## Current Shape

- Runtime: SolidJS 1.9, Vite 8, TypeScript strict mode.
- Gateway/BFF: Rust Axum gateway under `apps/gateway`.
- Adjacent web app: nested Next.js app under `apps/verevon-web`.
- Routes: dashboard, chat, inbox, agents, knowledge, auth/login, onboarding, settings.
- Cross-plane integration: browser traffic should go through the Rust gateway; the older Application Plane `verevon-gateway-rs` overlap needs an ownership decision.
- Demo/fallback surfaces: several workspace views still use honest fallback or preview data and need live-wiring classification.
- Directional architecture: shared action registry plus context packs are designed for human and Model Plane calls through the same action contracts.

## Entry Points

- App routes: `src/app/App.tsx`
- Shell: `src/app/shell/AppShell.tsx`
- Runtime mount: `src/index.tsx`
- Vite config: `vite.config.ts`
- Gateway entry: `apps/gateway/src/main.rs`
- Onboarding API client: `src/features/onboarding/lib/api.ts`
- Action registry: `src/shared/actions/action-registry.ts`
- Action execution: `src/shared/actions/action-client.ts`

## Relationship Map

- Verevon v3 browser -> Frontend Plane Rust gateway for same-origin `/api` and `/health` calls.
- Frontend Plane Rust gateway -> Control, Data, Ingestion, Model, and Application services by domain.
- Verevon v3 -> Model Plane by action contract intent; action metadata should stay complete even when execution is proxied through the gateway.
- Verevon v3 -> fallback data for some workspace surfaces until each feature is classified as live, planned, or preview-only.

## Highest-Signal Findings (OBSOLETE — kept for history; see the 2026-07-11 headline above)

> The findings below reflect the 2026-06-09 state. Findings 4–7 and the entire "Stub/Mock" section are now FALSE (auth is real, action execution is real, graphrest is deleted, `shared/api` has 56 files, mocks are gone). The current source of truth is `plane-audit-2026-07-11.md`.

1. Verevon v3 is real as a Solid/Vite app with a coherent feature-sliced structure. *(still true)*
2. It is not equivalent to `verevonv2`; the server boundary is a Rust gateway, not Next.js route handlers. *(still true)*
3. ~~Studio workspace test gate fails when session context lacks `orgs`.~~ *(re-verify)*
4. ~~Auth is presentation-only.~~ **FALSE now** — real `completeAuth()` via `auth-client.ts`, session-store probe, onboarding-gated routing.
5. ~~Action execution is local-only with synthetic IDs.~~ **Stale** — action registry is ~22 real descriptors executing via the gateway.
6. ~~`fetchKnowledgeGraphSnapshot()` is a static GraphREST example.~~ **FALSE now** — graphrest deleted; knowledge calls `/api/v1/knowledge/*`.
7. ~~`shared/api`/`shared/rpc`/`shared/workers` are empty.~~ **FALSE now** — `shared/api` has 56 client files.

## Files In This Set

- `runtime-shell.md` (updated 2026-07-11)
- `onboarding-gateway.md` (updated 2026-07-11)
- `auth-boundary.md` (rewritten 2026-07-11 — auth is real)
- `action-system.md` (updated 2026-07-11 — ~22 descriptors)
- ~~`mock-backed-surfaces.md`~~ (OBSOLETE — flagged for deletion in `apps/STALE_DOC_DELETION_REGISTER.md`)
- `plane-audit-2026-07-11.md` (current)
