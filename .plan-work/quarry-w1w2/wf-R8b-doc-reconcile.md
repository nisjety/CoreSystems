# R8b-doc-reconcile hand-over (round 3, 2026-10-19)

Commits on quarryv2-next (local only): 1a94267c ERROR_REASONS, e3dc52b0 RUNBOOKS, 13c6f5ac zdr-contract-sync digest fix,
8acfe426 plan ledger + errata + uncommitted-work state, ef94a195 PROGRESS snapshot, e79bb532 stale-doc register rows.

## What the run found that was not in any note
1. `scripts/tests/zdr-contract-sync.test.sh` was RED at HEAD (48 passed, 1 failed): 6b9c636a and 89c67974 regenerated the
   ZDR matrix golden without moving the digest the script pins. Fixed in 13c6f5ac (one line, 50 passed, --self-test ok).
   contract-goldens.yml would have failed on a push.
2. `quarry-runtime` lib test `pipeline::scrape_challenge_tests::the_scrape_path_logs_the_reason_and_the_status_and_never_the_host`
   (pipeline.rs:2869, count of would_refuse_scrape lines) is FLAKY: failed 1 of 3 full lib runs; the 2 reruns gave 734 passed;
   passes alone. Not diagnosed (per-thread tracing subscriber via set_default; probably tracing callsite-interest caching
   racing other test threads). Recorded in the plan ledger (WS3.H row) and PROGRESS.md; not fixed (not in this unit).
3. The live `quarry-edge` container still runs the image of 2026-10-01 (SOURCE_REVISION 2f2c3e83). Plan Status line says so.
4. The Model Plane compose file has no DATABASE_URL for browser-broker: a broker rebuilt from 7c5a564b exits 1. The plan row
   and RUNBOOKS grant section say so. The lines are in wf-R3-ws1c-compose.md and are still not applied.
5. browser-broker-ci.yml has no job for the tagged Postgres integration suite (jobs: broker, contract only).

## Fresh test numbers (code of e78b562c; Quarry Rust in quarry-rust-env:1, no Redis)
cargo test --workspace --no-fail-fast: 2230 passed, 1 failed (the flake), 11 ignored, 57 binaries. quarry-edge lib 364, bin 358,
21 integration binaries 188. quarry-browser 129 (4 ignored, no Chromium). quarry-core lib 119 / contracts 30. quarry-transform lib 156.
Node: 23 (route-coverage, scrape-parity, challenge-parity), 15 (browser-grant-contract), 65 (zdr-contract-schema), zdr-contract-sync 50.
Broker go test ok (4 packages). Gateway cargo test: bin 732 passed 1 ignored, log lint 13 (tree includes WS5.G's uncommitted hunks).

## Decisions I made (reversible, all in docs)
- Plan: the "uncommitted work" table is about the LIVE tree (X-3), so I kept it and put the branch's real git status and the list
  of table files the branch changed in front of it, instead of deleting rows.
- Plan: fixed the old test name a_service_principal_with_body_zdr_gets_the_same_refusals in BOTH places the plan names it
  (the task named one line; line ~902 and ~3779 both did).
- RUNBOOKS: deleted the three correction sections (c2b3d2e9, "At HEAD" supersession, R8a Redis correction) and one correction
  paragraph (5f90ae3a); their content is the plan's errata list; the Redis-gate sentence is fixed in place.
- R-26 boot check: documented as WARN only; the plan says "fail once the RSS is recorded"; RSS was recorded (5cfa4e3e) on a synthetic page
  and nobody decided to promote it. Left as an open decision, not changed.
- Errata: d26c61a5 ("694 passed") and d8a9708d ("709 passed") count claims and the rustfmt claims are NOT listed as errata because
  they could not be re-derived; the errata says so. 469e7c7d's "git grep described wrongly" (round-1 critic) could not be reproduced
  (git grep finds only the 63 pages) and is omitted.

## Not done / for the owner
- No image rebuilt; Redis-gated tests, --ignored Chromium tests, Postgres runtime tests, Model Plane Rust workspace not re-run.
- Flake above; SPA copy for the new gateway codes (errors.ts held by WS5.G); compose lines for the broker; promote quarry-browser-live
  to required; owner decisions listed in round1-leftovers.md (63 *-Aquatiq-Dev pages, squash df7feedf, trailer policy: 43 commits carry Haiku).
