# ADR-0001: Clean Feature-Sliced Next.js Architecture

**Date**: 2026-05-25  
**Status**: accepted  
**Deciders**: Codex, project owner

## Context

Velion v1 contains valuable product learning, but it also accumulated broad global providers, heavy dependencies, mixed ownership, and route surfaces that are difficult to reason about. Velion v2 needs to use v1 as product evidence without importing its dirty code or architecture.

## Decision

Velion v2 uses a clean Next.js App Router project with feature-sliced folders under `src/features/*`, shared primitives under `src/components/ui`, and typed contracts under `src/lib/api`. Server Components remain the default; Client Components are pushed to interactive leaves.

## Alternatives Considered

### Copy v1 and refactor in place
- **Pros**: Fastest way to preserve existing screens.
- **Cons**: Carries global state, stale route assumptions, skipped type validation, and old performance problems into v2.
- **Why not**: The user explicitly required a clean optimal architecture, not dirty code migration.

### Generic SaaS dashboard starter
- **Pros**: Quick clean baseline.
- **Cons**: Would miss Velion-specific support, knowledge, and agent workflows.
- **Why not**: V2 must compete in support and AI-agent operations, not look like a generic admin template.

## Consequences

### Positive
- Feature boundaries are clear and route-local.
- The app starts small and can add integrations without global provider sprawl.
- Product lessons from v1 are preserved as requirements, not code baggage.

### Negative
- Existing v1 backend wiring must be reintroduced intentionally behind contracts.
- Some parity work remains before replacing v1 in production.

### Risks
- Risk: Teams may bypass feature ownership as the app grows.
- Mitigation: Keep ADRs, API contracts, and tests near the first implementation.
