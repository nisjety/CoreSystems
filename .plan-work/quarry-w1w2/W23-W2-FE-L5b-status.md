# W2-FE-L5b status: WS15.L5 gateway proxy and dispatchers

Branch w23/lane-d. Date 2026-11-02. Built and tested (gateway cargo tests, host cargo), dark: the edge flag `QUARRY_EDGE__JOB_CONTROL` stays off, so every control call returns the edge's typed 501.

## Built
Check first: none of this existed (no `/crawl/{id}` family in the gateway, six run kinds not listed, no run-control or split schedule actions). All of it is new; the L5a registry and SPA client were already in place.
- Routes (`knowledge.rs`, new `crawl_control_routes()`, merged into the knowledge router): `GET /api/v1/knowledge/crawl/ongoing` (static, wins over `{id}`), `GET|DELETE /crawl/{id}`, `GET /crawl/{id}/errors`, `POST /crawl/{id}/pause`, `POST /crawl/{id}/resume`. The paths are the ones the L5a client calls.
- Handlers (`knowledge/quarry.rs`): `crawl_status`, `crawl_errors`, `crawl_ongoing`, `crawl_cancel`, `crawl_pause`, `crawl_resume` through `proxy_bearer_json` with the session-bound `aud=quarry` token (the org comes from its signed claims) and `x-user-id` from the session. Nothing from the browser is forwarded: no header, and of the query only `after_seq` and `limit` (digits, at most 12, no repeats) on the errors read. The job id must be letters, digits, `_` or `-` (up to 128), so a `.`, `/` or `%` cannot climb the upstream path. The edge's answer, a 501 included, returns unchanged.
- Run list (`ingestions/runs.rs`): `RUN_KINDS` adds `scrape` (reads `/v1/scrape/jobs`); every row carries `origin` (the origin `type` word only, the actor stays internal), `phase`, `desiredState` (row `desired_state`/`desiredState`, else `control.desired`), each null when Quarry sends none.
- Dispatchers (`actions/dispatchers.rs`, `handlers.rs`): `ingestions.get_run_status`, `pause_run`, `resume_run`, `cancel_run` call the crawl handlers (input `runId`, optional `kind` that must be `crawl`); `ingestions.pause_schedule` (only the pause verb, `scheduleId` only) and `ingestions.change_schedule_state` (unpause, trigger, backfill, delete; `pause_schedule` refused; only `action`, `scheduleId`, `startAt`, `endAt` forwarded; backfill needs both dates). `ingestions.run_schedule_action` is removed (an unknown id answers the 501 `not_implemented`). `ingestions.create_schedule` now accepts a batch with `urls` and no `targetUrl` (as the L5a registry does) and strips `activate`.

## Commits (w23/lane-d)
- c7bd51b4c feat(gateway): crawl status, errors, ongoing and pause/resume/cancel proxy (WS15.L5)
- 826e5590d feat(gateway): scrape runs and origin, phase and desiredState on the run list (WS15.L5)
- f5b515cc2 feat(gateway): run-control and schedule-state action dispatchers, run_schedule_action removed (WS15.L5)
- the status and depth commit follows.

## Tests (host cargo 1.97.1, `cargo test` in apps/gateway, target dir in the session scratchpad)
- New: 25 (quarry.rs `job_control_tests` 8, knowledge.rs `crawl_control_route_tests` 3 including a build of the real router, runs.rs `job_control_field_tests` 6, dispatchers.rs `ingestions_job_control_tests` 7, handlers.rs `ingestions_dispatch_tests` 1). The filtered run also matched 2 existing tests: 27 passed, 0 failed.
- Full gateway suite: 1065 passed, 0 failed, 1 ignored (bin) + 25 in the five integration test files (1, 7, 13, 3, 1); no failure anywhere.
- The wiremock tests assert: the edge path and verb of each route, `x-user-id` from the session, no `x-org-id`/forged bearer/foreign query reaching the edge, the 501 passed through, unsafe ids refused with zero upstream calls, wrong methods 405 with zero upstream calls, `ongoing` hitting `/v1/crawl/ongoing`.

## Flag and default
No new flag. `QUARRY_EDGE__JOB_CONTROL` (Quarry edge) stays off: pause, resume and cancel return the edge's typed 501 (`UNSUPPORTED`), which the L5a SPA already shows as a plain not-enabled message. Status, errors and ongoing reads are not behind that flag (they are the WS15.L1 reads).

## Assumed decisions
- The query on the errors read is whitelisted to `after_seq` and `limit` instead of being forwarded whole (the other proxies forward the raw query); this closes any `org_id`/`user_id` query smuggling.
- Control verbs get no rate class (they are rare and org-scoped by the edge); submit and search keep theirs.
- `origin` is shown as the origin `type` string; `desiredState` falls back to the `control.desired` block the lifecycle routes use.
- The removed `ingestions.run_schedule_action` is not kept as an alias: it would be an approval-free route to delete, and the registry no longer lists it.
- Batch control is not proxied (the L5a registry `kind` enum is crawl only).

## Model Plane asks
None new. The L5a re-vendor ask (admission golden, four copies and one digest) still stands in MP-ASKS.md; nothing here changes a Model Plane contract.

## Not verified
- No browser or live gateway run: the routes were driven through a test router with the session user injected (the real `require_session` layer was not exercised) against wiremock edges.
- The real Quarry edge was not called; its 501 shape comes from `quarry-core` (`ErrorCode::Unsupported`) and the edge source, not a live request.
- The `jobControl` capability on `GET /api/ingestions/schedules/capabilities` is NOT added: it needs `config.rs` (`ScheduleCapabilities`) and `schedules.rs`, which are outside this unit's file list. Until a unit adds it, the SPA keeps the controls hidden (depth row).
- Edge `list_jobs` rows do not carry `desired_state` today (JobSummary has origin and phase only), so `desiredState` stays null on the list until the edge or control adds it; the status read (`/crawl/{id}`) does carry `control.desired`.
- Gateway clippy and rustfmt were not run.
