# Velion v3

Standalone SolidJS + Vite + TypeScript frontend for Velion, created beside `velionv2` in the Frontend Plane.

## Stack

- SolidJS 1.9 for fine-grained client rendering.
- Vite 8 for dev/build speed and static client deployment.
- TypeScript 6 strict mode with path aliases.
- `vite-plugin-mcp` enabled in `vite.config.ts`; Vite exposes the local MCP endpoint at `/__mcp/sse` during development.
- `zod` action schemas shared by human UI controls and Model Plane calls.

## Boundaries

- `src/app` owns routing, providers, and the application shell.
- `src/features/*` owns each product surface: dashboard, chat, inbox, agents, knowledge, onboarding, settings.
- `src/shared/actions` is the AI-first command registry. Every meaningful UI operation should be represented here before a feature calls it.
- `src/shared/context-packs` builds compact model context from route, visible records, draft input, and available actions.
- `src/shared/api`, `src/shared/rpc`, and `src/shared/graphrest` hold API-first transport shapes for the future Application Plane gateway.
- `src/shared/cost` owns model routing policy for cost-aware Model Plane calls.

## Scripts

```bash
pnpm dev
pnpm lint
pnpm typecheck
pnpm test
pnpm build
pnpm verify
```

## Rule for New Features

Add the action contract first, then render the UI control that calls it. The user and the AI should use the same action ID, input schema, approval rule, and audit path.
