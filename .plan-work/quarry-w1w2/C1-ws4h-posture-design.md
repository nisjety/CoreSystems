# C1 WS4.H posture signal: design note (stamp at create)

Reviewer: Opus design unit C1-ws4h-posture-design. First written 2026-10-21, revised 2026-10-23 against the tree after the build unit's rounds 1 to 6. No repository file edited, no commit. Paths are relative to `apps/Ingestion Plane/Quarry-v2`.

## Status in the tree (checked 2026-10-23)
Built and committed by unit C1-ws4h-posture: `2a7759f6` (control, migration 023), `3bfc6a0b` (orchestrator), `06959092` (edge), `8b3c108b` (matrix), `efebf3e0` (RUNBOOKS, plan), then review fixes `1101b6c9`, `47a2c189`, `65ccf059`, `c4510d88`, `96c81447`. This note records the design as it now stands. Three points of the first version were superseded by review and are corrected below: (1) the body may raise the stamp, (2) service principals never reach the create, (3) `run_page` carries the stamp too.

## Facts this rests on
- Edge record route: `change_routes.rs` `RecordRequest.zdr: Option<bool>`. `record_internal` calls `zdr_refusal::refuse(.., req.zdr.unwrap_or(false), .., "change_record_internal", "record")` before the store. With the flag off it logs `zdr.would_refuse` and persists. `emit_change_side_effects` suppresses NATS and the webhook whenever `zdr` is true, in both modes.
- Control builds the Temporal args in one place: `resources/schedule_wire.go` `workflowFor` → `changeMonitorArgs{OrgID, URL, CreatedBy, Zdr}` (line 52). The reconciler calls `update` on every schedule on every pass.
- There are two create paths: `POST /v1/schedules` with a preset (`schedule_routes.rs`), and `POST /v1/sources` with `monitor:true` (`resource_routes.rs` → control `createSourceHandler`). Before WS4.H the second path had no ZDR check.
- Migrations on disk: 001-015, 019, 020, 023, 024. R-2 reserves 016-018 (WS15) and 021-022 (epics). The runner applies migrations by file name against `schema_migrations(name)`, so gaps are tolerated (R-2 row, `pg.go` 162-204).

## Contracts that change
1. **`023_schedules_zdr.sql`**: `ALTER TABLE schedules ADD COLUMN IF NOT EXISTS zdr BOOLEAN;`. It is nullable with no default. NULL means "created before the stamp", which is not the same as false (stamped standard). The R-2 row says that WS15's `017_schedules_definition` must carry the column if it rebuilds the table.
2. **Control**: `store.Schedule.Zdr *bool` (pg and mem stores); `changeMonitorArgs.Zdr *bool \`json:"zdr,omitempty"\``. A NULL row produces byte-identical args. The source monitor copies `sourceCreateBody.Zdr` to `sched.Zdr`.
3. **Orchestrator** (`activities.go`): `ChangeMonitorInput.Zdr`, `CheckChangeInput.Zdr` and `RunPageInput.Zdr`, all `*bool` with omitempty. `zdr` goes on the record body and on the run_page body only when the schedule is stamped.
4. **Edge**: `let zdr = claims.effective_zdr(req.zdr.unwrap_or(false))` is computed once (`schedule_routes.rs` 262). The same value feeds the refusal and the forwarded `ControlScheduleCreate.zdr: bool`, which is always serialised. `create_source` does the same, but only when `monitor` is true. The legacy non-preset create never forwards `zdr`.

## Security argument
- **Authority**: the stamp is `effective_zdr` = (org posture from verified claims) OR (body declaration). A body can raise the stamp but can never lower it below the org floor. Control trusts its HMAC-signed caller, the orchestrator trusts control's args, and the edge record route trusts its internal token. No client-controlled path lowers the stamp.
- **Service principals**: `require_service_route_scope` (`auth.rs` 539) answers 403 to `/v1/schedules` and `/v1/sources` for every service principal before the handler runs. So "a service principal stamps false" is not a gap. It is pinned by `a_service_principal_cannot_create_a_monitor_in_either_mode`.
- **Fail closed**: with the flag on, a ZDR create is refused before any forward. So `zdr=true` rows come only from observe-mode creates or from a body declaration. Their record calls are refused under the flag. The orchestrator treats that refusal as the terminal status `zdr_refused` (`ZDRRefusalCode`).
- **Visible change with the flag off (owner to accept or veto)**: a ZDR monitor created after the deploy no longer gets its edge NATS event or webhook. This is the existing fail-closed rule and is not gated on the flag. NULL rows behave as before.
- **ZDR leaks that remain (documented, out of scope)**: `ChangeMonitorWF` still emits `run_started` and `change_detected` with the URL, and the URL sits in Temporal args and history.
- **Known gaps (in RUNBOOKS and the ledger)**: (a) an org that turns ZDR on later keeps its false or NULL stamp. The operator remedy is `UPDATE schedules SET zdr = true WHERE org_id = $1`, which the next reconcile pass propagates. The fire-time lookup (X-9) is not approved. (b) The legacy non-preset path is not stamped and never fires (`workflowFor` maps no workflow for it).

## Races, ordering, idempotence
Every new field is optional on every decoder: Go ignores unknown fields, and serde has no `deny_unknown_fields`. Any deploy order is safe. The recommended order is control, then the orchestrator, then the edge. Temporal replay is safe because the inputs gain an optional field and the workflow issues no new commands. The stamp is a single INSERT column, so there is no read-modify-write race. The DDL uses `IF NOT EXISTS`, so it can be applied twice.

## Hot files and order of edits
1. Control: migration, store, `pg/resources.go`, `schedule_wire.go`, the source handler, tests.
2. Orchestrator: `workflows.go`, `activities.go`, tests.
3. Edge: `schedule_routes.rs`, then `resource_routes.rs` (anchored edits), then `tests/zdr_posture_stamp.rs`.
4. `quarry-core/src/zdr_matrix.rs` texts, `QUARRY_UPDATE_GOLDEN=1 cargo test -p quarry-core zdr_matrix`, the digest pin in `scripts/tests/zdr-contract-sync.test.sh`, then append to RUNBOOKS (hunk-only commit, because RUNBOOKS carries other units' hunks), then the R-2 row and the ledger row in the plan.

## Tests that prove each rule
- **Control**: `TestCreateScheduleStampsZdr`, `TestWorkflowForCarriesZdr`, `TestListScheduleCarriesStampToReconciler`, `TestSourceMonitorCreateStampsZdr`, `TestPgScheduleZdrRoundTrip`, `TestMigration023IsIdempotent`. The Postgres tests have a required CI lane (`47a2c189`).
- **Orchestrator**: `TestCheckChangeSendsStampedZdr`, `TestChangeMonitorWFPassesZdr`, the run_page stamp tests in `zdr_posture_test.go`, and `schedules/schedules_zdr_test.go`.
- **Edge** (`tests/zdr_posture_stamp.rs`):
  - `observe_mode_stamps_the_zdr_posture_on_the_forwarded_schedule`
  - `a_client_body_can_raise_the_stamp_but_never_lower_it`
  - `observe_mode_a_declared_zdr_body_is_stamped_for_a_standard_user`
  - `enforced_a_declared_zdr_body_is_refused_for_a_standard_user`
  - `enforced_schedule_create_is_stamped_false_for_standard_and_never_forwarded_for_zdr`
  - `a_legacy_cron_create_never_forwards_a_declared_zdr`
  - `observe_mode_stamps_a_source_monitor`
  - `enforced_source_monitor_is_refused_but_a_plain_source_is_not`
  - `observe_internal_record_with_declared_zdr_logs_would_refuse_and_is_not_403`
  - the service-principal test
  - The existing `enforced_internal_change_record_refuses_a_declared_zdr_body_only` stays green.
- **Mutations** (run on a copy with its own QROOT and QTARGET, volume removed afterwards):
  - stamp hard-coded to false
  - `refuse` dropped from `record_internal`
  - `Zdr` dropped from the record body, from `workflowFor`, from the workflow, from the source monitor and from run_page
  - create and update args stripped

  Each mutation must turn its named test red. The results are in `wf-C1-ws4h-posture.md`.

## Matrix
`schedules`, `profiles` and `change_record_internal` **stay `Gap`**: ADR-4 stage A is dark and default off, and `Enforced` is the owner's stage B. Only their issue and basis texts change, together with the `control_records` basis for `monitor:true`. `zdr_gap_rows.rs` is not inverted.

## What I will NOT do
- Flip the flag default or any compose value, or mark a row `Enforced`.
- Add the fire-time lookup (X-9).
- Change `ChangeMonitorWF` event payloads.
- Touch the WS5.G hunks, the WS4.J migrations 015 and 024, or any live container.
