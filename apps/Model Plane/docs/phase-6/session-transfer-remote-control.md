# Session Transfer & Remote Control

## Product role

Two closely related operator primitives:

1. **Session transfer** — hand an in-flight session from one surface to another
   (e.g. CLI → Web shell, or user A → user B for escalation) without losing
   journal state.
2. **Remote control** — let an authorized operator drive another user’s active
   session (view + optional input injection), with full audit trail.

## Transport

- **Transfer**: HTTPS POST to `execution-core` via `model-gateway`, returning a
  short-lived claim token the target surface presents on attach.
- **Remote control**: WebSocket attach with `role=controller` claim; a second
  stream emits audit events to `capability-core`’s metadata sink.
- **Auth**: both flows require an elevated scope
  (`session:transfer` or `session:control`) in the bearer token.

## Backend ownership

| Concern | Owner |
|---------|-------|
| Claim token issuance, journal continuity | `execution-core` |
| Multiplexed attach, role arbitration | `model-gateway` |
| Authorization scope check | `capability-core` |
| Audit stream persistence | `capability-core` (metadata) |

## `capability-core` responsibilities

- **Policy**: which roles may transfer vs. control; whether cross-org transfer
  is permitted; maximum controller count per session.
- **Catalog**: no direct role — models do not change on transfer.
- **Metadata**: persist transfer/control audit records (actor, target,
  timestamp, reason); expose read-only via GET endpoints.
- **Scheduling hints**: preserve region affinity across transfer to avoid cold
  start.

## Acceptance properties

1. Transfer is atomic: either the target attaches and the origin detaches, or
   the session remains with the origin. No dual-ownership window.
2. Remote control is always visible to the controlled user (banner /
   notification), and the controlled user can revoke at any time.
3. Every transfer and every control-input frame produces one audit record.

## Reference inputs

- `openclaw` — remote-control UX and permission model.
- `claude-code-fork` — session handoff patterns in REPL.

## Out of scope

- Live pair-editing with cursor sharing — handled by a future collaboration
  plane.
