# U8-doc-reconcile hand-over, second pass (2026-10-20, after Docker came back)

Commits (all Sonnet trailer, hook ran, no bypass): 64356ca4 (RUNBOOKS, ERROR_REASONS, PROGRESS), fe07ec5c (plan ledger rows and errata), acbd04b7 (stale-doc register third pass), bcd78023 (one-phrase RUNBOOKS fix).
The first pass (70f2c932, ed7f722e, 674e9e26, 31441f12) and U9's 2b7ac61b described most of round 4 as uncommitted; all of it is committed at c7429f2e, and the docs now say so.

Measured at HEAD c7429f2e on 2026-10-20 (every number in PROGRESS and RUNBOOKS):
- cargo test --workspace --no-fail-fast, quarry-rust-env:1, no Redis: 2305 passed, 0 failed, 7 ignored (53 binaries + 7 doc-test targets); quarry-edge lib 379, bin 373, 24 integration binaries / 214 tests; quarry-runtime lib 739.
- Live lane (Chromium 155.0.8059.79): quarry-browser 138/4 ignored, ssrf 4/1 ignored, egress 5, zdr_behavior 4, live_gate 9, ignored list 5; CHROME=/nonexistent rc 101 (119/19 failed, 2/3 failed); feature off 85.
- Postgres store tests (throwaway postgres:16-alpine, REQUIRE_CONTRACT=1): 23 passed; no DATABASE_URL: 5 fail.
- node: Quarry-v2 scripts/tests 23, browser-grant-contract 19, zdr-contract-schema 65; zdr-contract-sync.test.sh 50.
- bootstrap-broker-db.test.sh: 33 ok, ALL PASSED; with `CREATE ROLE other_role` removed from a copy: still 33 ok, ALL PASSED.
- gateway bin (git archive export): 751 passed, 0 failed, 1 ignored; browser 105, actions 30, knowledge::quarry 22, log_field_discipline 13.
- Live stack, read only: broker container image of 2026-10-02 without DATABASE_URL; no browser_broker role/database in ingestion-postgres; no browser-broker-db network. U1's deploy is still not applied.

Not done (for whoever continues):
- No edge image was rebuilt from a HEAD that contains the tab-initiator fix and WS6.E, so those are seen only through cargo tests (agent_run_control 28, grant_budget_claim 8, grant_budget_boot 2) and U9's working-tree image. Scripts: wf-U9-evidence-scripts/ (needs a docker build of Dockerfile.edge, long).
- SPA vitest/tsc/eslint, broker Go, model-gateway lib (1749), docker compose config renders: not re-run; the ledger rows name their source (commit message or note).
- Weak check left in scripts/tests/bootstrap-broker-db.test.sh: "another ordinary role cannot connect" passes without other_role (U1 file, not edited).
- A code comment in model-gateway browser_run.rs ("nothing is dispatched for a replay") is partly stale after 8410abea; listed in the errata, not edited (not an owned path).
- Nothing ran on GitHub Actions; branch protection for "Live browser tests (Chromium)" is a repository setting.

Traps: gateway export needs verevonv3 nginx.conf + the two docker-compose files + the Quarry zdr-sensitive-fields golden; Write tool trims trailing spaces; `node -e` inside Bash double quotes eats backticks (write scripts with Write); with-lock + nohup in run_in_background returns at once, wait on the log with qwait; PowerShell/Bash `grep -c` counts lines not occurrences.
