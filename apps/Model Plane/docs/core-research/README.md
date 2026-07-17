# Model Plane Core Research

Generated: 2026-06-09
Updated: 2026-07-16 (secure-MVP release-blocker audit, local Docker evidence,
and explicit no-production-artifact/no-rollback state)

This directory contains the current core-level research notes for the active Model Plane runtime services.

Latest plane audit: `plane-audit-2026-07-16.md`. The 2026-07-11 and
2026-07-13 audits are retained as historical evidence rather than overwritten.
See also `../../MODEL_PLANE_STATUS.md`, `../../MODEL_PLANE_ROADMAP.md`, and
`grpc-safe-rebuild-decision-2026-07-16.md`.

Current gate note (2026-07-16): the local integration stack has 21 Model Plane
containers running; all 19 configured health checks are healthy, and the
gateway, inference, and execution gRPC listeners are live on loopback ports
9090, 9092, and 9093. Control Auth and the Data retrieval/graph/wiki services
are also reachable and healthy. These are unsigned dirty-tree integration
images, not an immutable production candidate or accepted rollback artifact.
Live negative probes confirm protected Gateway, Cost, Capability, Session, and
Letta entry points deny missing or malformed identity. Capability execution is
fail-closed because no trusted health reporter has attested any tool; Letta is
live but explicitly semantic-degraded; signed all-ZDR inference skips every
unverified provider before network I/O. Source includes artifact-v3 release
tooling, cost/NATS authorization, approval and terminal outbox primitives,
capability-before-dispatch policy, typed hybrid multi-step retrieval, and
optional Letta `/v1/tools/search` ranking. Letta tool ranking is separate from
semantic memory and has no live external proof. Approval-required external
effects remain unavailable because exact-effect continuation and receipt
evidence are incomplete. Production promotion still requires a clean reviewed
revision, managed signing, non-placeholder identity/ZDR/rollback evidence, a
distinct verified rollback artifact, fixed-tenant Control service principals,
and the live gates described in `plane-audit-2026-07-16.md`. The final artifact,
runtime-partition, input-security, and Bash syntax contracts passed at 22:24
CEST; this is tooling evidence, not a produced artifact.

Rust services (initially re-verified 2026-07-11; current deltas are dated in
each service note):

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
