# R2-agent-routes progress (round 3)
COMMITTED: 486bbd28 (Quarry F5-1..F5-4 + in-flight guard + tests + CONTRACTS hunk 1), start_run_grants commit (next hash in git log).
Quarry full suite passed (q2.log): lib 362, bin 356, all integ binaries ok.
IN PROGRESS: gateway commit (browser.rs, actions/dispatchers.rs, src/shared/actions/action-registry.ts; ALL hunks are mine) after `cargo test` in gateway dir (log scratchpad/R2-agent-routes/gw3.log, ends GW_DONE).
Do NOT stage agent_routes/run_sweeper.rs (R5's zdr-log-ok hunk).
Commit helper: scratchpad/R2-agent-routes/commit-hunks.sh <msgfile> <path[|hunks]>...

## Review-fix round (2026-10-19)
Committed 8954d752 (resume-class test, service-principal gap doc + pinned test) and 9545f063 (gateway: tab actor strict + required, dispatcher tests, registry schema).
Declined/partial: finding 4 (org check needs org_id on broker proto + Go broker: Model Plane, not mine); finding 5 (no model-origin channel in the gateway; made actor required so omission fails closed).
Tests: gateway cargo test 731 ok; quarry-edge agent_run_control 17, run_sweeper_integration 23, start_run_grants 5, lib agent_routes 80 ok. Full quarry-edge suite not re-run after this round (only tests/doc changed in Quarry).

## Round 3 review fixes (2026-10-19)
1bc5075a browser_action.run strict actor + test + blank line; next commit renames known_gap test. All 4 findings handled (1 acknowledged, 3 flipped-as-named gap).
