# Velion v3 Core Research

Generated: 2026-06-09

## Scope

This pass covers `apps/Frontend Plane/velionv3` only. It intentionally skips the broader `velionv2` Frontend Plane audit.

Velion v3 is a standalone SolidJS + Vite + TypeScript SPA. It is smaller than `velionv2` and does not currently provide a Next.js-style server BFF.

## Current Shape

- Runtime: SolidJS 1.9, Vite 8, TypeScript strict mode.
- Routes: dashboard, chat, inbox, agents, knowledge, auth/login, onboarding, settings.
- Real cross-plane integration: onboarding calls the Application Plane `velion-gateway-rs`.
- Demo/local surfaces: dashboard, chat, inbox, agents, knowledge, settings, auth, action execution, and GraphREST are mostly local or mock-backed today.
- Directional architecture: shared action registry plus context packs are designed for Model Plane calls, but the transport is not wired yet.

## Entry Points

- App routes: `src/app/App.tsx`
- Shell: `src/app/shell/AppShell.tsx`
- Runtime mount: `src/index.tsx`
- Vite config: `vite.config.ts`
- Onboarding API client: `src/features/onboarding/lib/api.ts`
- Action registry: `src/shared/actions/action-registry.ts`
- Action execution: `src/shared/actions/action-client.ts`

## Relationship Map

- Velion v3 -> Application Plane `velion-gateway-rs` for onboarding bootstrap, state, Brreg search, graph preview, crawl preview, organization creation, connect sessions, source discovery, plan selection, checkout, and completion.
- Velion v3 -> Model Plane only by contract intent today. `ActionDescriptor` entries name model-owned actions, but `executeAction()` does not call Model Plane.
- Velion v3 -> Data/Ingestion/Control Plane indirectly through `velion-gateway-rs` during onboarding.
- Velion v3 -> local mock data for main workspace surfaces outside onboarding.

## Highest-Signal Findings

1. Velion v3 is real as a Solid/Vite app and has a coherent feature-sliced structure.
2. It is not equivalent to `velionv2` operationally because it has no server BFF and no auth/session enforcement.
3. The onboarding flow is the only substantial live external integration and depends on `VITE_VELION_GATEWAY_URL` or `http://127.0.0.1:3185`.
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
