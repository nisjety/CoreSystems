# Velion v3 Runtime Shell

## Current State

Velion v3 is a client-side SolidJS app mounted from `src/index.tsx` and routed through `@solidjs/router`.

Routes are declared in `src/app/App.tsx`:

- `/` and `/dashboard`
- `/chat`
- `/inbox`
- `/agents`
- `/knowledge`
- `/auth`
- `/login`
- `/onboarding`
- `/settings`
- `*404`

The shell in `src/app/shell/AppShell.tsx` wraps non-auth and non-onboarding routes with navigation, a topbar, and an AI context rail.

## Relationships

- The shell builds a model context pack from the current route, visible items, and action registry.
- Visible items come from `src/shared/mocks/velion-operating-model.ts`.
- The shell displays `liveRuns` from the same mock source.

## Runtime Caveats

- There is no route-level authentication guard.
- There is no server-side session, token minting, or BFF normalization layer.
- The context rail is structurally useful, but it is currently fed by local mock state.

## Performance Notes

- Route components are lazy-loaded, which is good for a static SPA.
- The current app is small enough that performance risk is more likely to come from future data wiring than from the current bundle shape.
- A generated `dist/` tree and `.playwright-mcp/` artifacts exist on disk. Treat them as generated output, not source documentation.
