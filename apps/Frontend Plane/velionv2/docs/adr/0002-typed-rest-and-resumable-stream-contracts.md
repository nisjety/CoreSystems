# ADR-0002: Typed REST and Resumable Stream Contracts

**Date**: 2026-05-25  
**Status**: accepted  
**Deciders**: Codex, project owner

## Context

Verevon talks to multiple CoreSystem planes and external support systems. V2 needs predictable contracts, semantic errors, pagination, and stream resumability so UI surfaces can show retry and stale states instead of failing silently.

## Decision

V2 uses REST-style route handlers with resource-oriented URLs, typed response envelopes, Zod-backed error shapes, cursor pagination for collections, and SSE streams with event IDs plus `Last-Event-ID` resume shape.

## Alternatives Considered

### Ad hoc JSON per route
- **Pros**: Lower ceremony for prototypes.
- **Cons**: Inconsistent errors and no shared retry or validation model.
- **Why not**: Support and agent workflows need reliable retries and debuggability.

### GraphQL gateway first
- **Pros**: Strong schema and flexible reads.
- **Cons**: Adds gateway complexity before the v2 frontend boundary is stable.
- **Why not**: The current CoreSystem plane contracts are HTTP-oriented and Verevon's route handlers are the accepted L5 ingress.

## Consequences

### Positive
- API errors can be rendered consistently.
- Long-running agent tasks can resume from event IDs.
- Large queues can paginate without offset degradation.

### Negative
- Contract helpers must be kept stable and reviewed.
- Future backend integration needs adapter code to normalize upstream errors.

### Risks
- Risk: SSE event stores are not durable until backed by a real persistence layer.
- Mitigation: The frontend contract already includes IDs and cursors, so persistence can be added without changing the UI API.
