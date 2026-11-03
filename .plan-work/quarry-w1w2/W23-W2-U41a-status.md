# W2-U41a status: org erasure hops beyond profiles, inventory test, 0017 release gate, tombstone ordering (D2-077)

Branch w23/lane-a. Date 2026-11-03. Plane: Ingestion (quarry-runtime, quarry-edge). Code only, not compiled, no test run.

## Built (to scope)
- Already existed: only `ProfileErasure` implemented `OrgErasable`; the route `DELETE /v1/internal/orgs/:org/profiles` walked every registered family (profiles only). Built the gap below.
- `org_erasure.rs`:
  - `PgTableErasure` (feature `postgres-queue`): one transaction per family, `DELETE FROM <const table> WHERE org_id = $1`, counts only. Families and tables (`PG_FAMILIES`, delete order children first): `step_receipts` (quarry_browser_timeline_events, quarry_step_receipts), `checkpoints` (quarry_run_checkpoints, quarry_agent_run_checkpoints), `frontier` (quarry_queue_items, quarry_retry_events, quarry_request_queues), `change_baselines` (quarry_change_diffs, quarry_baselines), `job_history` (quarry_job_history).
  - `ArtifactErasure`: filesystem (`{root}/org={org}/` removed, files counted, symlinks not followed) or S3 (`org={org}/` prefix, paged list + DeleteObjects, partial failure is an error). Org id with `/`, `\`, `..` or NUL refused.
  - Inventory (plan 3.6): `PROFILE_TABLES`, `PG_FAMILIES`, `TABLE_EXEMPTIONS` (empty: every org-keyed runtime table has a hop), `NON_TABLE_STORES` (Redis profile keys, artifacts, artifact index, CAS pages, in-memory stores, quarry-control tables, each with where it is erased or why not).
  - `FamilySelection` (flag parse) and `flag_on`.
  - D2-077 tombstone guard: `ProfileErasure::with_tombstone_guard(true)` lists the org's profiles, purges, then re-tombstones each through `delete` (after the purge, never before). `PurgeCounts.guard_tombstones` added.
- `profile_backfill.rs`: E3 release gate for 0017: `ReleaseGateMode` (off/observe/enforce), `release_gate` (probe one plaintext row), `check_release_gate` (enforce + encryption `on` + plaintext left = `RuntimeNotReady`, boot refuses).
- `internal_routes.rs`: `?scope=profiles|org` on the existing route. No scope or `profiles` walks the profile family only (unchanged behaviour); `org` walks every registered family; anything else is 400. `guard_tombstones_written` in the family report, left out of the JSON when zero (default response byte-identical).
- `main.rs` (one anchor, the existing org_erasables block): parses the three flags (unknown value = boot error), runs the release gate when the Postgres blob store exists, adds the guard to ProfileErasure, registers the table families over a new PgPool (when `database_url` is set) and the artifact family for the fs/s3 backend.

## Tests written (not run)
- org_erasure.rs `inventory_tests`: every_org_keyed_table_has_an_erasure_hop_or_a_written_exemption (reads migrations/*.sql, comment-stripped, CREATE TABLE with an org_id column or ALTER TABLE ADD org_id; sanity check on known tables), every_hop_table_exists_and_is_org_keyed, exemptions_have_reasons_and_never_overlap_a_hop, family_selection_parses_off_all_and_lists, tombstone_guard_refuses_a_late_save_after_the_purge (guard off recreates, guard on refuses), artifact_prefix_refuses_path_tricks, artifact_family_purges_one_org_on_the_filesystem (real FilesystemStore layout), table_families_purge_one_org_and_keep_another (postgres-queue + DATABASE_URL-gated canary rows in 5 tables, two orgs, idempotent).
- profile_backfill.rs: release_gate_mode_parses, release_gate_blocks_on_until_the_backfill_ran.
- internal_routes.rs: scope_defaults_to_profiles_and_refuses_unknown_values, a_zero_guard_count_leaves_the_response_shape_unchanged.
- The inventory regex was dry-run in Python over the 17 migration files (12 tables, all 12 org-keyed, all in a hop); no cargo run.

## Commits (local, w23/lane-a)
- 8cb481750 feat(quarry-runtime): org erasure families beyond profiles, table inventory and tombstone guard (W2-U41a)
- 4e72f777f feat(quarry-runtime): profile backfill release gate for migration 0017 (W2-U41a)
- d03914753 feat(quarry-edge): register the new org erasure families and add scope=org to the purge route (W2-U41a)
- this status file and the lane-a depth rows (docs commit)

## Flags and defaults
- `QUARRY_ORG_ERASURE_FAMILIES`: unset/empty/off/none = no extra family (default); `all`; or a comma list of step_receipts, checkpoints, frontier, change_baselines, job_history, artifacts. Unknown name refuses boot.
- `QUARRY_ORG_ERASURE_PROFILE_TOMBSTONE_GUARD`: off by default (1/true/on/yes).
- `QUARRY_PROFILE_BACKFILL_GATE`: off (default), observe, enforce. Unknown value refuses boot.
- Route: `?scope=org` opt-in; the default call is the old profile purge.

## Assumed decisions
- The new families ride the existing route behind `?scope=org` instead of a new route: routes.rs is not in the unit's files, and this keeps the profile call unchanged.
- "history" = quarry_job_history (own family `job_history`); change baselines and diffs are one family `change_baselines`; browser timeline events go with step receipts.
- Guard tombstones (org id + profile id, no content) are kept after an erasure for the store's tombstone TTL (7 days in Postgres); accepted as ids-only residue.
- The 0017 gate is a boot probe of one plaintext row, enforced only in encryption mode `on` (the no-rollback release named in 0017's header); compat/off only log.
- Artifact id index lines stay (written exemption); CAS pages are erased by the document path; quarry-control tables by its own PurgeOrg.
- Errors carry the step class only (no sqlx/S3 text, which can echo ids).

## MP asks
- None (no Model Plane change needed).

## Not done / not verified
- Not compiled, no test run (integration step compile-checks). Risky spots for the compile guard: aws-sdk-s3 `DeleteObjectsOutput::errors()` slice accessor, the `#[cfg]` block in main.rs, `Query<PurgeQuery>` extractor order.
- quarry-control's Go hop does not send `?scope=org` yet (D-U41a-1, Ingestion follow-up, outside the unit's files).
- RUNBOOKS/compose/openapi entries for the flags (D-U41a-7).
- Depth rows D-U41a-1..7 in W23-depth-lane-a.md.
