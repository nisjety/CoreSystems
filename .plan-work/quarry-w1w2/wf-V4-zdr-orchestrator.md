# V4-zdr-orchestrator hand-over

## What the orchestrator lacks to send `zdr: true` on /v1/internal/change/record (task 1: NOT done, by design)
The record call is made by `ChangeMonitorWF -> CheckChange` (`services/quarry-orchestrator/internal/activities/activities.go`, `changeRecordRequest`: org_id, url, fresh_fingerprint, run_id). The workflow is fired by a Temporal schedule that quarry-control builds from its `schedules` row (`services/quarry-control/internal/resources/schedule_wire.go`, `changeMonitorArgs`: org_id, url, created_by). Nowhere on that path is the organization's ZDR posture:
- `store.Schedule` (quarry-control) has no zdr column; no migration carries one.
- The edge's schedule create forward (`ControlScheduleCreate`, `schedule_routes.rs`) sends no zdr. The edge knows the posture only from the caller's token (`Claims::effective_zdr`) at create time; a cron fire has no caller.
- The orchestrator has no Control Plane / Auth Core client to look the posture up.
Options (neither built, both cross a boundary): (a) a `zdr` column on the quarry-control schedule row stamped at create from `effective_zdr`, carried through `changeMonitorArgs` -> `ChangeMonitorInput` -> `CheckChangeInput` -> `changeRecordRequest` (schema change; schedules created before an org turned ZDR on do not carry it); (b) a posture lookup against Control Plane at fire time. No dormant field was added on the Go side: a field nothing fills would look like progress.
Note the real gap this leaves: with the flag on, the edge refuses a ZDR org's schedule CREATE, so no new ZDR schedule exists; but an old schedule created before the org turned ZDR on keeps firing and the record route (no zdr in the body) keeps writing baselines. The refusal at record time only closes after (a) or (b).

## Done
- bf5c7efd (Go): a 403 with `error.details.zdr_refusal.code == zdr_durable_mutation_forbidden` from the record route is a terminal clean outcome: activity returns `Status "zdr_refused", ZDRRefused true` and no error (one record call, no retry); `ChangeMonitorWF` emits one `run_failed` (`kind change_monitor`, `reason zdr_refused`, `refusal_code`, no URL) and completes without a workflow error. Any other 403 stays CategoryAuth.
- Rust (e32a7cf8): `zdr_refusal.rs` gains `refusal_details`, `forbidden_with_details`, `respond`; `refuse_durable_job_under_zdr` (crawl, batch) builds its 403 with them (message and status unchanged, still always enforced, old log line unchanged); `lifecycle.rs` adds `zdr_status_err` for the resume (403) and persistent profile (400) refusals (message and `BAD_REQUEST` code unchanged). Actions: `crawl`, `batch`, `profile`, `resume`.
- Docs: ERROR_REASONS (table of the pre-existing refusals + orchestrator behaviour), RUNBOOKS (stage A paragraph, "What the orchestrator lacks").

## Traps
- `refuse_durable_job_under_zdr` could NOT delegate to `zdr_refusal::refuse`: that helper only refuses with the flag on (observe mode lets the request through), the old refusal is unconditional. Only the error construction is shared.
- The agent refusals answer with `error.code BAD_REQUEST` even for the 403 resume (via `status_err`); kept, because "behaviour must not change".
- `cargo test` takes ONE positional filter; a second one is an "unexpected argument" error (the first run printed it and the lib tests did not compile at all).
- Matrix rows, flag default and compose defaults untouched (task 4).

## Round 6 review fixes (0286d382, b57c0be8)
- legacy_code `zdr_persistent_profile_forbidden` added to the persistent-profile refusal details (lifecycle.rs zdr_status_err takes Option<&str>); resume gets none (its message never had a token). Test asserts it.
- Known gap documented and pinned: run_started of ChangeMonitorWF carries the URL before the record call, so a refused ZDR run still leaves the URL in that event. Test pins both facts. Not fixed: needs the posture before the first event, or an event-contract change.
- Commit message e32a7cf8 claimed 27 binaries with --tests; correct is 26 (27th result line = Doc-tests without --tests). Corrected in RUNBOOKS (no history rewrite).
- Verified: go vet + go test all ok; cargo test -p quarry-edge --no-fail-fast 972 passed 0 failed (27 result lines, 26 Running).
