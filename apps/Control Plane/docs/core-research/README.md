# Control Plane Core Research

Updated: 2026-07-15 (final secure-MVP engineering/isolated acceptance refresh)

This directory contains service-level research notes for the active Control Plane authority services.

Latest plane audit:

- `plane-audit-2026-07-10.md` (renamed from `plane-audit-2026-07-02.md`; includes the 2026-07-02 baseline as an appendix)

See also, at the plane root: `../../CONTROL_PLANE_STATUS.md` (current-state snapshot) and `../../CONTROL_PLANE_ROADMAP.md` (fix plan).

Service research notes (each has a current 2026-07-15 addendum; older audit chronology is retained below it):

- `auth-core.md`
- `user-core.md`
- `org-core.md`
- `billing-core.md`
- `session-core.md`
- `audit-core.md`

Current gate note: secure-MVP source, static, disposable-database, embedded-broker, current-image Control, and real-authority Data/Velion acceptance are green. Auth reaches structural RSA/RS256 JWKS readiness; the 4/4 lifecycle and three-bus Audit matrices pass; all five Go suites/vet, Auth 372-test/build/lint gates, gateway 288-test/coverage gates, and the 22-service zero-host-port release render pass. Auth→User TLS and plaintext rejection are covered in the real-authority integration. Each Control core now owns an independent `.env`/`.env.example` contract; Compose layers those files per service and production resets them in favor of secret-manager inputs. The pre-existing shared local stack is degraded because its ignored root `.env` supplies only 8/68 required credential/file inputs; isolated evidence remains green. This is still **not a production release certificate**: a deployment authority must inject/rotate the real 58 scoped credentials and 10 registry/key/TLS files, deploy reviewed image digests, prove post-deploy health/auth denial/PubAck/lag/outbox convergence, and revoke old credentials. See `CONTROL_PLANE_STATUS.md`.
