# Velion v3 Core Research

Generated: 2026-06-09
Updated: 2026-07-02

## Scope

This pass covers `apps/Frontend Plane/velionv3` only. It intentionally skips the broader `velionv2` Frontend Plane audit.

Velion v3 is the current Frontend Plane target. It is a SolidJS + Vite + TypeScript app with a Rust Axum same-origin gateway under `apps/gateway`. It does not use the old Velion v2 Next.js route-handler BFF shape.

Latest plane audit: `plane-audit-2026-07-02.md`.

## Current Shape

- Runtime: SolidJS 1.9, Vite 8, TypeScript strict mode.
- Gateway/BFF: Rust Axum gateway under `apps/gateway`.
- Adjacent web app: nested Next.js app under `apps/velion-web`.
- Routes: dashboard, chat, inbox, agents, knowledge, auth/login, onboarding, settings.
- Cross-plane integration: browser traffic should go through the Rust gateway; the older Application Plane `velion-gateway-rs` overlap needs an ownership decision.
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

- Velion v3 browser -> Frontend Plane Rust gateway for same-origin `/api` and `/health` calls.
- Frontend Plane Rust gateway -> Control, Data, Ingestion, Model, and Application services by domain.
- Velion v3 -> Model Plane by action contract intent; action metadata should stay complete even when execution is proxied through the gateway.
- Velion v3 -> fallback data for some workspace surfaces until each feature is classified as live, planned, or preview-only.

## Highest-Signal Findings

1. Velion v3 is real as a Solid/Vite app and has a coherent feature-sliced structure.
2. It is not equivalent to `velionv2` operationally because the server boundary is a Rust gateway, not Next.js route handlers.
3. The current test gate failure is in Studio workspace loading when session context lacks `orgs`.
4. Auth is presentation-only: email/password, social buttons, and passkey all navigate to `/onboarding`.
5. The action registry is a useful contract surface, but action execution is local-only and returns synthetic run/audit IDs.
6. `fetchKnowledgeGraphSnapshot()` is a static in-memory GraphREST example, not a Data Plane graph client.
7. The README mentions `shared/api`, `shared/rpc`, and `shared/graphrest` as future transport areas, but `shared/api`, `shared/rpc`, and `shared/workers` are empty today.

## Stub, Mock, Placeholder, and TODO Summary

- `src/shared/mocks/velion-operating-model.ts` backs most non-onboarding workspace pages.
- `src/shared/actions/action-client.ts` synthesizes `run_*` and `audit_*` IDs locally.
- `src/shared/graphrest/graphrest-client.ts` returns hard-coded nodes and edges.
- `src/features/auth/components/AuthPage.tsx` performs no real auth call before routing to onboarding.
- `src/features/onboarding/lib/api.ts` injects dev actor headers by default in dev mode.
- Empty planned directories exist under `src/shared/api`, `src/shared/rpc`, and `src/shared/workers`.

## Files In This Set

- `runtime-shell.md`
- `onboarding-gateway.md`
- `auth-boundary.md`
- `action-system.md`
- `mock-backed-surfaces.md`
- `plane-audit-2026-07-02.md`
