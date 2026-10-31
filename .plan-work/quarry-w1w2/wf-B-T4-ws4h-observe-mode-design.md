# WS4.H stage A: design note (unit B-T4-ws4h-observe-mode)

## What ships
One helper, `quarry-edge/src/zdr_refusal.rs`, and a flag `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` (config field
`zdr_refuse_durable_resources`, default false) carried to the handlers as `AppState.zdr_refuse_durable_resources`.

Why an AppState field and not a process-wide OnceLock like `budget::current_mode`: that one has a thread-local test pin that
only exists under `cfg(test)`, so an integration binary (tests/*.rs) cannot run an observe test and an enforce test side
by side. A bool on the state lets each test pick its own mode. Cost: one added line in each full `AppState { .. }` literal
(main.rs, routes.rs tests, test_support.rs, tests/support/mod.rs and the other literals the compiler names).

## Helper
`refuse(state, org_id, zdr_active, request_id, row, action)` and `refuse_for_claims(state, claims, requested, request_id, row, action)`
(the second calls `claims.effective_zdr`, so the ZDR span mark and the token floor rule are the same as everywhere else).

* not ZDR: `Ok(())`, nothing logged.
* ZDR, flag off (stage A, observe): emits one content-free `zdr.would_refuse` event with `row`, `action`, `org_id`,
  and returns `Ok(())`. The request proceeds exactly as today (no behaviour change).
* ZDR, flag on (enforce): emits `zdr.refused` (same fields) and returns 403 `QuarryError::new(Forbidden, "ZDR active: ...")`
  with `details {"zdr_refusal": {"code": "zdr_durable_mutation_forbidden", "action": ...}}`. No `feature` key: the
  Model Plane catalogue has no key for these yet (X-16, WS4.I), and the code is already in the closed list.

org_id is in the event because the point of the week of observation is to learn which ZDR orgs are affected;
it is not content (existing logs carry it, and it is not in `SENSITIVE_FIELDS`). No URL, host, title, query or error text.

## Where it is called (before any validation, store access or forward to Control Plane)
* schedules: `create_schedule`, `unpause`, `trigger`, `backfill` refuse. `list` (resource_routes), `pause`, `delete` stay allowed
  (wind-down). There is no `get` route.
* change: `check` (POST, row change_history), `promote_snapshot`, `schedule_refresh` (row change_snapshot). `latest` and `history`
  are reads and stay allowed.
* internal change record: when the body declares `zdr: true`: observe emits and continues, enforce refuses before the baseline
  store is touched (nothing persisted).
* profiles: `save_profile` (POST /v1/profiles), `create_profile` (POST /v1/profiles/create), `update_profile` (PATCH). `list`,
  `load`, `delete` stay allowed. `restore_probe` is a POST but writes nothing (it reads the stored snapshot and counts it), so it
  stays allowed; the plan's "POST, PUT and PATCH" wording is read as "the writes". Judgement call, reported.

## Not in this slice
agent_routes (held by U3/U5), `details` on the already-refused rows (crawl, batch, profile at run start, resume), flipping
rows to Enforced in zdr_matrix.rs (golden + repo sync script would move), `/v1/search/suggest` (unassigned).
The gap tests in zdr_gap_rows.rs run with the flag off and are unchanged; the new binary `zdr_refusal_modes.rs` covers observe
(captured log) and enforce (403, control double sees nothing, wind-down 200, standard org unchanged).
