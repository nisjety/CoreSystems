# D2-A-openapi-internal: live continuation file (final state)

Status: DONE for the first half (decision 3.13). Committed hand-over: .plan-work/quarry-w1w2/D2-A-openapi-internal-progress.md.
Findings not fixed: .plan-work/quarry-w1w2/depth/D2-A-openapi-internal.md.

Commits (local, quarryv2-next, no push):
- cd17f8c5c docs(openapi): docs/openapi-internal.yaml (new), allowlist entries moved, coverage test knows the internal file
- 07af2adcf fix(quarry-sdk): run_page and the internal-only schemas leave docs/openapi.yaml; SDK internal docs and smoke cases removed (SDK regenerated); guard test; CONTRACTS.md rule
- ace7d0554 docs(plan-work): the progress, hand-over and depth notes

Last verification (final tree): node --test scripts/tests/*.test.mjs 53 tests, 41 pass, 0 fail, 12 todo;
tsc -p sdks/typescript --noEmit (TypeScript 5.9.3) exit 0; Python tests/ 25 passed, full sdks/python collection 338 passed.

Not started: the eight Go control internal routes (D2-050 split). Not touched: any Rust, Go or gateway code.
