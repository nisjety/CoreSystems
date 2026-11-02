# Migration policy: expand, then contract

Context: Model Plane images roll back (rollback rehearsal, WS01 S10/S12) while the database does not. After a candidate image has applied a migration, the older image boots against a schema it has never seen. This policy keeps that safe for the services that own a Postgres schema: session-core, capability-core, cost-core and letta-bridge.

## The rule
Past each service's cutoff a migration is additive: new tables, new nullable or defaulted columns, new indexes, new functions. It never removes or rewrites what an older image still reads or writes. A change that must do so is a contract migration and is shipped in a later release than the code that stopped using the old shape.

A migration is non-additive when its executable SQL (comments, string literals and function bodies excluded) contains any of: `DROP`, `RENAME`, `TRUNCATE`, `DELETE FROM`, `ALTER COLUMN ... TYPE`, `ALTER COLUMN ... SET NOT NULL`, or `ADD COLUMN ... NOT NULL` without a `DEFAULT`.

A non-additive migration needs both:
- a line `-- contract: <what is removed and why it is safe>` in the file, and
- a matching revert file that restores the old shape.

Every new session-core migration needs a revert file whether or not it is additive.

## Where the files go
| Service | Migration | Revert | Additive rule from | Revert always from |
|---|---|---|---|---|
| session-core | `rust/services/session-core/migrations/NNNN_name.sql` | `migrations/revert/NNNN_name.down.sql` | 0049 | 0044 |
| capability-core | `go/services/capability-core/migrations/NNNN_name.up.sql` | `NNNN_name.down.sql` beside it | 0017 | contract only |
| cost-core | `go/services/cost-core/migrations/NNNN_name.up.sql` | `NNNN_name.down.sql` beside it | 0011 | contract only |
| letta-bridge | `go/services/letta-bridge/migrations/NNNN_name.up.sql` | `NNNN_name.down.sql` beside it | 0003 | contract only |

Grandfathered: session-core 0044-0048 predate the rule (they replace constraints they drop, and carry reverts); capability-core 0003-0005 are seed migrations with no down files. cost-core re-runs every `up.sql` at boot, so its SQL must be idempotent.

Session-core numbers are reserved per slice in section 4.4 of `docs/plans/model-plane-chat-build-plan-2026-10-14.md`; gaps in the numbering are intentional.

## The lint
`bash "apps/Model Plane/scripts/tests/migration-expand-contract-test.sh"` runs a fixture self-test of the rules, then lints the four directories. It needs only `python3`. Run it before committing any migration; WS01 S11 wires it into the ops-gates workflow.

## SESSION_CORE_MIGRATION_IGNORE_MISSING
sqlx refuses to start when the database has applied a version the binary does not embed. That is the drift detector, and it is on by default. With `SESSION_CORE_MIGRATION_IGNORE_MISSING=true` session-core skips that check and logs a WARN naming the unknown versions at boot.

- Default is `false`. Only the literal `true` enables it (`1`, `TRUE`, `yes` do not).
- Only the rollback overlay used in WS01 S10/S12 sets it. Do not set it in the base compose file, the release overlay or a developer `.env`.
- It weakens drift detection by design: a typo'd or deleted migration file would also be ignored. The lint above is the mitigation, because an older image over a newer additive schema is the only case where ignoring is safe.
- Flip the default only after the S12 rollback rehearsal evidence exists.

## Rolling back past a contract migration
An older image cannot run over a contract migration. Run the revert file, then delete the version row so the next boot re-applies it cleanly:

```sql
-- after running revert/NNNN_name.down.sql
DELETE FROM _sqlx_migrations WHERE version = NNNN;
```

Do this before starting the older image, and only after confirming nothing written under the new shape would be lost (for example `runs.retention_class` for a waived run under review).

## Verifying the switch
The ignored DB test applies migrations 0001-0043 into a scratch schema, then checks that a migrator knowing only up to 0042 errors in strict mode and boots with ignore-missing. It is run by the lead through the DB harness:

```
DATABASE_URL=... cargo test -p session-core --bin session-core older_migrator_boots_only_with_ignore_missing -- --ignored
```
