# D-D-B3-egress-caps progress

Unit: WS6.H plan-cap stamp (D2-017), D2-013, D2-020, D2-018, D2-033. Branch quarryv2-next, local commits only.

## State (update after each milestone)

All five items are edited in the working tree; nothing committed yet (first test run of the combined tree queued, log in the scratchpad `D-D-B3-egress-caps/t2.log`).

Uncommitted files that are mine:
- crates/quarry-edge/src/handoff.rs (admission_stamp, with_admission + 3 unit tests)
- crates/quarry-edge/src/routes.rs (4 small hunks: crawl and batch params wrapped in `with_admission(.., &org_limits)`)
- crates/quarry-edge/tests/job_admission.rs (two-plan stamp test, floor test)
- crates/quarry-edge/src/agent_policy_gate.rs (`egress_for_narrowing`, empty Narrowed set = deny-all, 2 tests)
- crates/quarry-edge/src/agent_routes/human_browse.rs (one assertion: deny-all kept by open_web_egress)
- crates/quarry-browser/src/tab_control.rs (TabEgressPolicy meet, 5 tests), chromiumoxide.rs (comment only)
- crates/quarry-edge/src/extract_routes.rs (legacy search.query event keyed by request id + test)
- crates/quarry-edge/src/change_routes.rs (policy gate before `change/refresh` enqueue; postgres-queue feature only)
- crates/quarry-edge/tests/web_policy_routes.rs (route sweep and route-table manifest tests)
- TEMP file to delete before committing: apps/Ingestion Plane/Quarry-v2/.qrun-egress.sh

Strays left alone: quarry-edge lib.rs, main.rs, tests/support/fixture_site.rs, quarry-runtime step_receipts.rs (line-ending/mod-order changes of others).

## Test commands
- `QT 'bash /src/.qrun-egress.sh'` (the required set, see scratchpad run2.sh)
- first run (before the other edits): job_admission 5 passed, lib handoff 9 passed.

## Final (supersedes the state above)

All five items committed: 518584b41 (D2-017), 13654ea6e (D2-013), 893cbfe9d (D2-020), e7d290318 (D2-018), 989925c43 (D2-033).
Last run (t5.log): job_admission 5, log_field_discipline 22, web_policy_routes 11, human_browse 5, quarry-browser tab_control 15, quarry-edge lib handoff 9, extract 10 (filter `extract`), agent_policy_gate 8, cargo check --workspace --all-targets clean, cargo check -p quarry-edge --lib --features postgres-queue clean.
Temp file .qrun-egress.sh removed. Notes: wf-D-D-B3-egress-caps.md, depth/D-D-B3-egress-caps.md. The plan-work files are untracked/uncommitted.
