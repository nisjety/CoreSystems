# D-D-B3-agent-depth progress (2026-10-27)

All items done or recorded. Details of what is left: depth/D-B3-agent-depth.md.

Committed (branch quarryv2-next, local):
- fa143ecee D2-015 (EventSink::subscribe_open, live.rs, replay race test)
- 66b22037d D2-074 + D2-031 + O5 (attach.rs, tabs.rs, sweeper test)
- f92315cd0 D2-030 grantKind (types.rs, projection.rs)
- 208d6e4d1 O4 WS error frames (ws.rs, 2 test expectations corrected)
- e2809beda D2-019 + D1-021 (observation.rs, lifecycle.rs, run_registry.rs, tests)
- docs commit: RUNBOOKS section + notes

Already closed at HEAD (stale rows): F5-1..F5-4, D1-043, D1-061 alias test, D1-009 agent_routes half.

Test commands (all through qtest) and results:
- cargo test -p quarry-runtime --lib observation: 68 passed; --lib events: 22 passed
- cargo test -p quarry-edge --lib agent_routes: 157 passed
- cargo test -p quarry-edge --test agent_run_control: 35 passed; agent_events_replay 7; attach_cobrowse 4; run_sweeper_integration 26; sse_stream 1
- cargo check --workspace --all-targets: see end of file
Uncommitted strays that are not mine: lib.rs, main.rs, fixture_site.rs, step_receipts.rs, observation.rs observe_after hunk (challenge unit).
- cargo check --workspace --all-targets (qtest): rc 0 on the second run (the first run hit another unit's mid-edit tests/web_policy_routes.rs syntax error).
