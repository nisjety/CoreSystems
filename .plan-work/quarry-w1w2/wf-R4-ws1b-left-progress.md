# R4-ws1b-left progress

## Committed (branch quarryv2-next, local)
- 5fee6c16 refactor(quarry-runtime): delete GrpcGrantValidator::acquire + L1 (node test 12 pass; quarry-runtime --features grpc grpc 11 pass)
- 62f1b66a feat(gateway): WS3.F driver refusals -> typed codes (gateway domains::browser 97 pass)
- f7818066 feat(gateway): Idempotency-Key on the AI-run start, forwarded to model-gateway (97 pass)
  (both gateway commits staged with HEAD+my transform only: R2 has ~283 uncommitted lines in the same browser.rs; method:
   scratchpad/R4-ws1b-left/commit-staged.sh <msg> node stage-head.js <path> <transform.js...>)
- notes written: wf-R4-grant-renewal-design.md, wf-R4-ws1b-left-compose.md

## Uncommitted, mine (model-gateway; clean at HEAD otherwise, commit with qcommit.sh)
- apps/Model Plane/rust/services/model-gateway/src/browser_run.rs (br-edit.js impl + br-tests-edit.js tests)
- apps/Model Plane/rust/services/model-gateway/src/session_flow.rs (sf-edit.js: prepare_idempotent_browser_run)
- test status: first docker run (log R4-ws1b-left/mg1.log) was queued behind the docker lock for >30 min. NOTE the working browser_run.rs
  test helper uses HeaderValue::from_str: must be from_bytes (the "ünicode" case); apply the same replacement as br-tests.txt (already fixed there).

## Task 2
derive_failure_creates_no_thread_or_run already exists at HEAD (33a362f6, drives the real handler with a counting Session Core). Only to re-run.

## Test commands
- model-gateway: bash scratchpad/INTEGRATE/mp-test.sh 'apt-get update -qq >/dev/null 2>&1; apt-get install -y -qq libprotobuf-dev >/dev/null 2>&1; export CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/src/target; cd rust && cargo test -p model-gateway --lib browser_run 2>&1 | ...'
- gateway: cd gateway dir; cargo test --bin verevon-gateway-rs domains::browser
- quarry-edge --features grpc check blocked by another unit's chromiumoxide.rs E0599 (retry once at the end).
- go: cd browser-broker && go test -count=1 ./...  (not yet run)

## Next
run model-gateway tests, fix, commit; broker go test; final hand-over wf-R4-ws1b-left.md (SPA codes list).
- COMPLETE: all committed (e4af12cb last)
R4 round3: committed e36c879f (L1+CI). Uncommitted mine: browser_run.rs, session_flow.rs (detach keyed start, read-only replay via GetRun, tests). Running model-gateway tests -> R4-ws1b-left/mg3.log
- ROUND 3 COMPLETE: e36c879f, 860c83ce committed
R4 round4: committed stripper fix (same-line cfg(test), cfg(all(test,..))); other findings no code change
