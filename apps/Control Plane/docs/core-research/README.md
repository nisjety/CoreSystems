# Control Plane Core Research

Updated: 2026-07-11 (production-readiness continuation of the 2026-07-10 audit)

This directory contains service-level research notes for the active Control Plane authority services.

Latest plane audit:

- `plane-audit-2026-07-10.md` (renamed from `plane-audit-2026-07-02.md`; includes the 2026-07-02 baseline as an appendix)

See also, at the plane root: `../../CONTROL_PLANE_STATUS.md` (current-state snapshot) and `../../CONTROL_PLANE_ROADMAP.md` (fix plan).

Service research notes (each has a current 2026-07-11 addendum; older audit chronology is retained below it):

- `auth-core.md`
- `user-core.md`
- `org-core.md`
- `billing-core.md`
- `session-core.md`
- `audit-core.md`

Current gate note: the earlier 2026-07-11 matrix had all six services and the gateway healthy, but the final build pass exposed Docker Desktop containerd/BuildKit filesystem I/O errors. User/Session/Audit are now unhealthy/unverified, Postgres metadata is contradictory, and the final gateway signing image was not built. Source/integration fixes include verified-context-only User handlers, signed Gateway→Session delegation, durable Audit consumers, stream-sequence inbox dedupe, and TERM-after-confirmed-DLQ. This is **not** an MVP release certificate: Docker recovery/redeploy, two ownerless Auth organizations, shared Org/Billing/Audit keys, producer-side audit durability/replay, isolated lifecycle E2E, Auth lint, and coverage remain. See `CONTROL_PLANE_STATUS.md`.
