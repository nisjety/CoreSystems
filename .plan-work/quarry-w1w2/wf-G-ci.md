# G-ci: stranded CI and bootstrap work of U1-broker-db and U2-live-ci (2026-10-20)

Commits (in order): 08d07ad2 (bootstrap_broker_db.sh, init-databases.sql, migration 0001 comment, bootstrap-broker-db.test.sh),
b40242f7 (browser-broker-ci.yml, bootstrap_runtime_environment.sh, U1 hunks of browser-grant-contract.test.mjs),
3496dd07 (quarry-browser live_gate.rs + tests/ssrf.rs), f1e3a19e (browser_egress_boundary.rs, quarry-v2-ci.yml, U2 paragraph of RUNBOOKS.md).

Evidence: bootstrap-broker-db.test.sh 33 ok / 0 FAIL on a throwaway postgres; actionlint rc 0 on both workflows; CI integration command
(GOWORK=off go test -tags integration ./internal/grant/...) ok 46 s; e2e on browser-broker-u1:test (log: wf-G-ci-e2e-evidence.log);
live lane x3 in quarry-rust-env:1: quarry-browser lib 138 passed/4 ignored, ssrf 4 passed/1 ignored, browser_egress_boundary 5 passed, zdr_behavior 4 passed;
CHROME=/nonexistent and no-chromium-on-PATH: browser and egress sets exit 101, zdr_behavior passes.

Traps found:
- The shared quarry-target docker volume is used by every agent; cargo trusts mtimes, so a loop run after another agent's build of a different tree
  reused STALE artifacts (135 lib tests, ssrf all ignored = HEAD versions). Fix: `touch` the sources inside the same locked command (scratch G-ci/loop2.sh).
- qcommit with a temporary GIT_INDEX_FILE (commit-b.sh) leaves the REAL index at the old HEAD blob, so `git status` shows the files staged "MM"; fix by
  `git update-index --cacheinfo` to the new HEAD blob (qcommit-blobs.sh does it; commit-b.sh does not).
- The contract test file still carries the WS6.E budget hunks (uncommitted, belong to group Q-chain); at HEAD alone, the two proto tests would fail only
  if the budget proto were reverted: the file at HEAD passes against the working-tree proto only partly (2 of 17 pass when run from outside the repo; irrelevant).
- The e2e used the broker image built earlier (browser-broker-u1:test), not a rebuild of the current tree (the tree holds another unit's uncommitted broker budget code).
Left: RUNBOOKS.md still carries two other units' sections (WS6.E budget claim, WS4.H ZDR flag); contract test carries the budget hunks.
