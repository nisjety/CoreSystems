# P2-ws1b progress (WS1.B)

## Committed
- 0997b7c8 feat(browser-broker): DeriveRunGrant (proto + broker, tests) — by the first agent, go test green per its message.

## In progress (resume agent, 2026-10-18)
- model-gateway: browser_run.rs + browser_grants.rs (uncommitted, ours). Added Unimplemented -> 502
  browser_grant_derive_unavailable (plan: broker rollback maps 502). Test run via qtest (mp-target), log
  scratchpad/P2-ws1b/mg_test.log.
- frontend gateway: domains/browser.rs + config.rs (uncommitted, ours) plus one-line AppConfig literal edits
  in main.rs, chat/shared.rs, orchestration.rs, onboarding/crawl_preview/stream_e2e.rs (made by first agent).
- Quarry projection grantId: quarry-edge agent_routes/{projection,types,lifecycle,tests}.rs (first agent; mixed
  with other units' hunks? to check).

## Not ours
- model-gateway grpc.rs, quarry.rs (WS5.G); .plan-work/.

## Update 11:26
- COMMITTED d26c61a5 feat(gateway) (browser.rs, config.rs + 4 one-line AppState literals). gateway bin tests 694 pass.
- model-gateway: qtest must mount "apps/Model Plane" (proto/ is outside rust/): QROOT=".../apps/Model Plane" QTARGET=mp-target, command 'cd rust && export CARGO_TARGET_DIR=/src/target; cargo test -p model-gateway --lib'. Log P2-ws1b/mg_test2.log.
- Quarry projection (types.rs, projection.rs ours; lifecycle.rs/tests.rs "..Default::default()" hunks are NOT ours) still to test via qtest.

## Update (final)
- COMMITTED 6da7ffd8 model-gateway (41 browser_ tests pass), 27801ea1 quarry-edge grantId (75 agent_routes pass).
- Unit complete within its boundary; leftovers listed in wf-P2-ws1b.md. No uncommitted files of ours remain.

## Review-fix round (2026-10-18, third agent)
- COMMITTED 73ad8767 broker: approval host/expiry bound to parent, run grants per-parent cap (8) not owner cap, betweenLocks hook tests, MaxRunDomains server test. go test ./... green.
- NEXT: model-gateway browser_run.rs (revoke derived grant on start failure, correct run_key retry claim, idempotency?, router-level derive-failure test); then frontend gateway get_session grantId strip + per-audience token issuer + expired-session-grant; then Quarry live projection test.
- COMMITTED 9d835979 frontend gateway: get_session strips grantId, browser_session_grant_inactive mapping, per-audience test tokens. 712 bin tests pass.
- UNCOMMITTED (ours): model-gateway browser_run.rs (revoke derived grant on failed start / run end, corrected run_key doc, 3 router-level tests) -> qtest log P2-ws1b/mg_test4.log.
- UNCOMMITTED (ours): Quarry quarry-runtime grant_validator.rs + grpc/browser_broker.rs (kind/owner_id/zdr), quarry-edge agent_routes/lifecycle.rs (bind_validated_grant, zdr_grant_may_back_run), tests.rs (5 tests) -> qtest log P2-ws1b/q_test3.log.
- NOT OURS in quarry-runtime: contracts/zdr-feature-matrix.json(.sha256), quarry-core zdr_matrix.rs.
- COMMITTED 33a362f6 model-gateway, 6216f19f Quarry, plan ledger row. Review round complete; leftovers in wf-P2-ws1b.md / design note.

## Review round (verifier findings) - ecd7097e
- Finding 1 (no defect found): nothing to fix.
- Finding 2 (per-owner bound 20*(1+8)=180): intended, documented in 73ad8767; no change.
- Finding 3 (owner gate test only denial path): fixed in ecd7097e, owner now asserted Ok with data.grantId.
- qtest 'cargo test -p quarry-edge --lib grant': 9 passed, 0 failed.
