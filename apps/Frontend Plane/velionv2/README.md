# Velion v2

Clean Next.js 16 App Router rebuild of Velion. The old app is used only as product and roadmap input; v2 intentionally does not import v1 code or architecture.

## Architecture

- Feature-sliced folders under `src/features/*`
- Server Components by default, Client Components only at interaction leaves
- Typed REST envelopes in `src/lib/api`
- Active ingestion/search target is Quarry-v2 (`quarry-edge`/`quarry-control`);
  legacy `apps/Ingestion Plane/Quarry` is deferred and not a Velion v2 dependency
- Cursor pagination for unbounded collections
- SSE stream IDs and `Last-Event-ID` resume shape
- Virtualized inbox/thread surfaces with TanStack Virtual
- Local-first drafts, undo stack, command palette, and explicit retry/status UI

## Commands

```bash
pnpm dev
pnpm lint
pnpm typecheck
pnpm test
pnpm build
pnpm test:e2e
```

## Docs

- `docs/api-contracts.md`
- `docs/adr/`

## Competitive Baseline

V2 starts with explicit UX targets from ChatGPT, Manus, Notion, Uber, GitHub, Airbnb, Linear, Stripe Dashboard, Intercom, Zendesk, Gorgias, and Chatbase: streaming continuity, visible long-running task progress, local-first edits, route intent prefetch-ready architecture, virtualization, command operation, customer context, macros, and lightweight embed-ready contracts.
