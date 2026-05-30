# ADR-0003: Performance-First Support Workflows

**Date**: 2026-05-25  
**Status**: accepted  
**Deciders**: Codex, project owner

## Context

The competitive benchmark includes ChatGPT, Manus, Notion, Uber, GitHub, Airbnb, Linear, Stripe Dashboard, Intercom, Zendesk, Gorgias, and Chatbase. These products make performance visible through streaming, local-first edits, virtualized lists, keyboard flows, and predictable retry states.

## Decision

V2 treats performance patterns as baseline architecture: virtualize conversation queues and threads, keep composer drafts local-first, support undo, expose command navigation, use explicit stream states, and avoid heavy editor/WebGL/motion dependencies in the core shell.

## Alternatives Considered

### Add performance improvements after feature parity
- **Pros**: Faster first-pass feature count.
- **Cons**: Recreates v1's cleanup problem and makes hot-path state expensive to unwind.
- **Why not**: The target products compete primarily on interaction speed and trust under load.

### Use animation-heavy product demos
- **Pros**: Strong initial visual impression.
- **Cons**: Adds layout and bundle pressure to repeated support workflows.
- **Why not**: Velion is an operational control room; clarity and latency matter more than spectacle.

## Consequences

### Positive
- Inbox and chat surfaces are ready for large histories.
- Keyboard operation is part of the shell from day one.
- Heavy integrations must justify their cost and load route-locally.

### Negative
- Some UI code is more explicit because virtualization and local-first state require deliberate structure.
- Visual language is restrained rather than marketing-heavy.

### Risks
- Risk: Mock data can hide real network latency.
- Mitigation: API routes expose the same pagination and stream shapes expected from real upstreams.
