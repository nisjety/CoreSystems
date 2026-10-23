# V9-doc-reconcile hand-over (2026-10-21)

Commits (local, quarryv2-next): a1b37b7d (RUNBOOKS, ERROR_REASONS, PROGRESS, one word in SECRET_REDACTION.md),
ecc2712f (plan: ledger rows, errata, Round 6 state paragraph), 36c66743 (stale-doc register).

## What is in
- Plan ledger: row "Live stack change, 2026-10-20 evening" (broker redeploy), rows Round 6 V1, V2, V3, V4, WS3.K, WS12.S0; a
  "Round 6 state" paragraph; ~15 errata lines (fe46954c, c25865fc, bf5c7efd, e32a7cf8/d49dfd48/1932afdf, 2daa3f0c, 1932afdf,
  36977011, eb3af8ac/88575b59/399d3f10, 10f91e58, d5a160eb/4f2b4442, d405aa87, SECRET_REDACTION "7 JSON cases"); superseded
  markers on the header status, WS1.C, U1, U3, U4, U5, U6 cells, Round 4 state paragraph, evidence paragraph, errata 111 and 118.
- RUNBOOKS: broker deploy record + traps, Postgres paragraph rewritten, WS6.E gaps, zdr_refusal_modes 8, WS3.K 172, lane re-run.
- ERROR_REASONS: 409 in the opening sentence; 422 section says what is pinned.
- PROGRESS: snapshot re-measured at 95f94e36; new rows for broker/e2e, SPA, orchestrator marked not re-run.
- Register: section "Round 6: statements replaced".

## V9's own runs (2026-10-21, HEAD 95f94e36)
workspace cargo test 2414 passed / 0 failed / 8 ignored (61 result lines, rc 0); live lane 172+4 ignored / ssrf 4+1 / egress 5 /
zdr_behavior 4; postgres_ 23 passed (17.7 s, default threads); bootstrap-broker-db 34 ok; zdr-contract-sync 52 + self-test ok;
node 23 and 19; gateway bin 758 passed 1 ignored (working tree incl. WS5.G hunks); broker go test ok, go vet -tags e2e clean.
Read-only live checks: docker inspect/logs/network inspect/images of the broker (no env read).

## Not re-run
e2e harness (docker, long), SPA vitest/tsc/eslint, orchestrator go test, anything on GitHub Actions.

## Traps
- The Write tool and node -e collapse backslashes; I used a pairs-file tool (scratchpad V9/apply.js) for exact-anchor edits.
- A backgrounded `( ... ) &` in the Bash tool reports "completed" immediately; the real job keeps running: wait on the log with qwait.sh.
- The 972 vs 975 quarry-edge totals: 975 is right at HEAD (lib 381, bin 375, 24 integration binaries 219).
