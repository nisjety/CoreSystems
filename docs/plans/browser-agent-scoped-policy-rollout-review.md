# Browser-agent policy rollout and recovery plan

Status: review only. No live migration or deployment has been performed. Both scoped enforcement flags default to `false`.

## Current deployment and migration boundary

- The running capability-core image ID at review time is `sha256:30d979328444699711ed27b2a42347029cea1db5bf5160a55a69c331d4100bb8`; execution-core is `sha256:a2f27d150121c378dde7d355bf0d11b4b34f81dcd10d4d36e42301567c0ba609`. Record current IDs again immediately before any change, and retain those exact images until rollback is no longer needed.
- Capability-core's entrypoint reapplies every embedded `*.up.sql`, without a migration ledger. The existing container image lacks source migrations 0027–0032 and 0034. Restarting the review image as-is would run all seven, not only 0030. Migration 0027 deletes two seed browser-tool bindings currently present and rewrites seeded capabilities; 0031 and other seed migrations may also update existing rows. Review and approve every such change before any broad migration.
- Migration 0030 adds nullable `tool_policies.expires_at` and a check that expiry applies only to `allow`; existing rows keep NULL. The live table currently lacks that column. The new policy query fails closed until the migration is present.
- The exact source of the live capability-core binary is not established: its image label names revision `91b9b8ddfd682d24432e96a46bb5ebebfc520849`, but a clean build of that revision produced a different binary SHA-256. Do not replace the live binary with a guessed transplant or the stock review image.

## Preflight and backup

1. Keep the browser-agent feature disabled or unavailable while working. Confirm the effective flags, service identity, tenant, running image IDs, and current health. The scoped flag is `BROWSER_AGENT_POLICY_ENFORCED`; it must not be enabled only on execution-core against an unverified capability-core.
2. Take a consistent, encrypted backup of the `session_core` database using the approved PostgreSQL backup procedure, and preserve current capability-core and execution-core image digests and configuration in the authorized backup location. Do not place passwords or tokens in shell history, logs, or the backup plan.
3. Restore the backup to a separate isolated PostgreSQL instance and verify that the restore is readable. Snapshot schema and active `tool_policies` rows, including org, user scope, decisions, digests, revoked state, and expiry. Verify counts and a secure digest before and after the proposed migration; never delete, revoke, or rewrite policy rows as part of the rollout.
4. Confirm maintenance scope and dependency owners. Inventory current and pending migrations and their exact effects against a fresh copy of the database; the earlier counts are only a point-in-time observation, not permission to modify newer rows.

## Candidate and verification

1. First establish a trustworthy baseline for the currently running capability-core binary. Only a reproduced binary or an explicitly reviewed, separately authorized broader upgrade resolves this blocker. Build a candidate with an explicitly controlled migration set; do not run the stock review image against live data as-is.
2. Rehearse migration 0030 alone against an isolated restore. Reapply it to verify idempotence, verify existing policy rows and decisions are unchanged, and check that a non-null expiry cannot be assigned to an `ask` or `never` row. Record lock duration and failure behavior from the rehearsal; schedule around any observed blocking.
3. After the compatible capability-core and schema are verified, enable `BROWSER_AGENT_POLICY_ENFORCED=true` for capability-core and execution-core through the normal deployment configuration. The execution side requires a verified signed v2 response carrying `tool_policy_checked`; a missing signer, unchecked policy store, mismatched user/tenant/run/tool, or old capability service must fail closed. Leave `TOOL_POLICY_ENFORCED` and `TOOL_CLASS_GATE` at their pre-existing settings.
4. Verify with controlled test tenants: org `never` denies all users, a user's `ask` affects only that user and is not replaced by a browser grant, cross-org requests are isolated, and unconfigured/missing policy storage denies. Confirm unrelated tools are unchanged, existing stored policies remain intact, and the browser agent's bounded action, frame, cursor, cancellation and final output work end to end.

## Recovery

- If preflight, migration, signature verification or end-to-end checks fail, stop new browser-agent runs and keep their feature unavailable. Preserve the database, both image IDs and the error evidence.
- Restore the exact recorded service images/configuration only after checking schema compatibility. Do not automatically drop `expires_at`: old code that ignores expiry may treat lapsed `allow` rules as active. Do not restore a whole database over newer user data without a separately reviewed recovery decision.
- If a database restore is required, use the verified backup in an isolated environment first, compare subsequent policy and user-data changes, and have the database owner execute a targeted, reviewed recovery. Keep the affected browser feature unavailable until policies, signatures and service health are verified.

## Isolated restored-snapshot rehearsal (2026-11-04)

A consistent custom-format PostgreSQL dump was taken without live writes, encrypted with Windows current-user DPAPI, verified by decrypting and comparing its SHA-256, and restored to network-disabled disposable PostgreSQL. The encrypted temporary local backup is `C:\Users\IMAFER~1\AppData\Local\Temp\verevon-capability-rehearsal-5xchhgo5\session-core.dump.dpapi`. It is bound to that Windows user and host and is not a verified independent disaster-recovery backup.

All 34 current startup migrations passed against the restored snapshot, including the seven missing from the running image. Comparing before and after in the disposable database, existing tool policies were unchanged after excluding the new nullable expiry field; one default safety policy was added; two preexisting seed tool bindings were removed; all other preexisting seed bindings were unchanged excluding timestamps. A policy expiry query succeeded and PostgreSQL was ready. The two removed bindings require owner review; successful SQL execution alone does not establish that the behavior change is acceptable.

The actual review image was also started against a second isolated restored copy. Its own migration entrypoint completed, and `/healthz` and `/readyz` passed with `BROWSER_AGENT_POLICY_ENFORCED=true`, a fake loopback JWKS, a throwaway signing key and background workers disabled. Both disposable containers were removed. This does not verify live authentication, inter-service signing, brokers, authorization enforcement end to end, service compatibility with production configuration, or an agent run. The live image source/reproducibility blocker and seed changes remain unresolved; no migration or deployment was performed.
