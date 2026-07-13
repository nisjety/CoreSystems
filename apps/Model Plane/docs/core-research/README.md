# Model Plane Core Research

Generated: 2026-06-09
Updated: 2026-07-13 (secure-MVP audit, read-only live evidence, source hardening, and explicit deployment-state separation)

This directory contains the current core-level research notes for the active Model Plane runtime services.

Latest plane audit: `plane-audit-2026-07-13.md`. The 2026-07-11 audit is retained as historical evidence rather than overwritten. See also `../../MODEL_PLANE_STATUS.md`, `../../MODEL_PLANE_ROADMAP.md`, and `grpc-safe-rebuild-decision-2026-07-13.md`.

Current gate note (2026-07-13): the agent/tool loop and approval gate are real, but the **running** gateway and inference gRPC listeners are absent, so current chat/tool compatibility is broken. Plain chat intentionally sends no tools; explicit tools, Plan, and Agent Run Console are distinct frontend-owned modes. There is no Velion Visma integration: the lone live record is malformed and the bridge is not deployed. Live cost-core remains unauthenticated; semantic memory remains unavailable. The earlier “session compaction is 100% failing” claim was disproved by current counters (479 successes, 0 errors, 24 checkpoints) and is withdrawn. Source hardening now includes restored authenticated gRPC listeners, exact Auth Core audiences/callers, terminal-safe approval replay, and a tested ordinary HTTP/SSE invoke chain, but is deliberately not deployed. Approval outbox/cache recovery, durable browser ownership, background callers, authoritative capability dispatch, release-database proof, a verified ZDR provider route, rollback artifacts, and live negative verification remain incomplete. Full evidence and limitations are in `plane-audit-2026-07-13.md`.

Rust services (all re-verified 2026-07-11):

Rust services:

- `model-gateway.md`
- `session-core.md`
- `inference-core.md`
- `execution-core.md`

Go services (all re-verified 2026-07-11):

- `orchestrator-core.md`
- `capability-core.md`
- `sandbox-manager.md`
- `browser-broker.md`
- `letta-bridge.md`
- `bridge-core.md`
- `cost-core.md`
