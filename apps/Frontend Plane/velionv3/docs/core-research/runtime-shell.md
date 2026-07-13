# Velion v3 Runtime Shell

> Verified 2026-07-11 (source-only pass; SPA :5173 + gateway :3185 containers down, no live curl).
> The prior version of this doc described a mock-fed context rail and "no auth guard / no BFF"
> shell. That is obsolete: the SPA is fully off mocks (`src/shared/mocks/` no longer exists),
> route guards are in place, and the shell hydrates from the gateway. Rewritten to match current
> source.

## Current State

Velion v3 is a client-side SolidJS app mounted from `src/index.tsx` (`render(() => <App />, root)`)
and routed through `@solidjs/router`.

Routes are declared in `src/app/App.tsx` with `AppShell` as the router root. The tree is:

- Standalone (no product chrome): `/auth`, `/login`, `/reset-password` (all `AuthPage`),
  and `/onboarding` (wrapped in `RequireOnboarding`).
- Authenticated app, nested under a `/` route wrapped in `RequireAuth`:
  - `/` and `/dashboard` (`DashboardPage`)
  - `/chat`
  - `/studio` + `/studio/{canvas,campaigns,templates}`
  - `/inbox`, `/tickets`
  - `/social` + `/social/{accounts,calendar,drafts,approvals,campaigns,competitors,trends,evergreen,commerce}`
  - `/insights` + `/insights/{overview,social,inbox,agents,campaigns,experiments}`
  - `/agents` + `/agents/{runs,cost,quality}`
  - `/ingestions`
  - `/knowledge` + `/knowledge/shared`
  - `/leads`
  - `/account`
  - `/settings` and `/settings/:section` (each wrapped in `RequireWorkspaceAdmin`)
- `*404` (`NotFoundPage`)

The shell in `src/app/shell/AppShell.tsx` wraps everything in `I18nProvider` + `QueryProvider`,
loads the session once on mount (`loadSession()`), and renders the product chrome
(`CoreShell`) for every route except the standalone surfaces (`/onboarding`, `/login`, `/auth`).

## Relationships

- `CoreShell` (`src/features/core/components/CoreShell.tsx`) is the real product chrome:
  `CoreNavbar` (topbar) + `CoreSidebar` (navigation) around the routed `<main>`.
- The shell reads the live session via `getSession()` and fetches navbar state from the gateway
  through `getNavbarData()` → `requestJson('/api/v1/navbar')` (`src/shared/api/navbar-client.ts`),
  held in a `createResource`. It re-fetches on a visibility-aware timer (60s foreground /
  180s hidden) and on tab focus. There is no mock operating model; `velion-operating-model.ts`
  and `src/shared/mocks/` have been removed.
- Workspace identity (name, initial, plan, role, accent, avatar) is derived by
  `resolveWorkspaceIdentity(session, navbarData)` and provided to descendants via
  `CoreWorkspaceContext`. Theme preference comes from the navbar payload and is applied to the
  document root.

## Runtime / Auth

- Route-level auth guards exist. `RequireAuth` sends unauthenticated users to `/login` and
  authenticated-but-not-onboarded users to `/onboarding`; `RequireOnboarding` guards the
  onboarding surface; `RequireWorkspaceAdmin` gates `/settings*` behind
  `hasWorkspaceAdminAccess(session)` (`src/shared/session/access.ts`).
- The session is a real server session loaded once at the shell root: `loadSession()`
  (`src/shared/session/session-store.ts`) calls `getCurrentSession()` / `getAuthSession()` in
  `src/shared/api/auth-client.ts`; guards read the reactive `getSession()` store.
- The same-origin BFF normalization layer is the Rust gateway at `apps/gateway`
  (`src/domains/*`), which the SPA reaches via `/api/*`. Org scoping is derived server-side from
  the verified session, not from a client-supplied `x-velion-org-id` header (the earlier
  cross-tenant IDOR fix); confirming that behaviour belongs to the gateway docs, not this SPA doc.

## Performance Notes

- Route components are lazy-loaded (`lazy(() => import(...))`), which is good for a static SPA.
- Navbar refetching is throttled and visibility-aware, so the always-on chrome does not poll
  aggressively when the tab is hidden.
- A generated `dist/` tree and `.playwright-mcp/` artifacts may exist on disk. Treat them as
  generated output, not source documentation.
