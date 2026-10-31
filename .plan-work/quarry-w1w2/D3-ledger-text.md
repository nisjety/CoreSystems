# D3-docs-and-residuals: plan-errata and ledger text (updated 2026-10-23, round 5)

For the single ledger writer of `docs/plans/quarry-browser-build-plan-2026-10-14.md`
(breakdown section 5: the ledger is written once, after the lanes). Every count below
comes from a command that was run on 2026-10-21 (round D3) or 2026-10-23 (round 5); the
commit hashes are on branch quarryv2-next. Nothing here is stack evidence unless it says so.

## WS6.A (breakdown item k)

Built and committed:
- Crawl bounds at the edge and in the orchestrator (committed before this unit; the edge
  and orchestrator tests are in the Go and cargo runs below).
- `docs/CONTRACTS.md` section 9.4, "`POST /v1/crawl` bounds (WS6.A)", commit 662a08da.
- The 7-page crawl recipe `scripts/ws6a-crawl-bound-evidence.sh`, commit 8f82b83d,
  executable in 93acb92c. `--self-test` passes (no network). The recipe itself is NOT RUN.

Go test record (native, go1.26.5, `go test ./... -count=1 -v`, every `--- PASS` and
`--- SKIP` line counted, subtests included), at HEAD b8b7495b (round 5, 2026-10-23; a
`git archive` export, so no other unit's uncommitted hunk is counted):
- `services/quarry-orchestrator`: 106 passed, 0 failed, 0 skipped.
- `services/quarry-control`: 233 passed, 0 failed, 20 skipped. The earlier 228/15 run (21:46,
  2026-10-21) predates ten tests of the ZDR tombstone receiver, each first added in 90634d87.
  The skips are the Postgres integration tests, which need `QUARRY_TEST_DSN` (the required CI
  lane runs them); all 20 print "QUARRY_TEST_DSN unset" locally.
- `pkg/quarrycontracts`: 35 passed, 0 failed, 0 skipped.
- The same counts were recorded at HEAD 7849ff1b in round 4.

Still open, stack evidence (SC7 crawl clause stays NOT-RUN):
- The 7-page crawl with `QUARRY_CRAWL_DEFAULT_MAX_PAGES=7` through the gateway:
  `limits.max_pages.effective == 7`, `clamped_by == ["default"]`, `params.max_pages == 7`,
  `statistics.pages_visited == 7`. The script above is the recipe; run it on a side-by-side
  exit-gate stack only.
- The pre-fix capture on an image built before 5995eeca.
- The recorded-history replay of `CrawlJobWF`. `TestCrawlJobWF_OldVersionHistoryKeepsLegacyBehaviour`
  still uses the `OnGetVersion` mock. The pinned SDK (`go.temporal.io/sdk v1.29.1`) has the
  mock (`TestWorkflowEnvironment.OnGetVersion`) and the worker replayer; the replay needs a
  history captured from a real run of the pre-fix workflow.

Errata for the plan text:
- WS6.A test list: "OnGetVersion hook availability in the pinned Temporal SDK is [UNVERIFIED]"
  is now verified. The mock exists in v1.29.1, and the existing test compiles and passes at
  HEAD b8b7495b (`--- PASS: TestCrawlJobWF_OldVersionHistoryKeepsLegacyBehaviour`).

## WS2.E (breakdown item k)

- The code was committed earlier (24ee3957): the fleet route sits behind
  `VITE_FLEET_CONSOLE` in `src/app/App.tsx`.
- Checks ran in a scratch copy of `apps/Frontend Plane/verevonv3` (no `node_modules` in the
  copy; `pnpm install --frozen-lockfile`, the same lockfile as the worktree), on 2026-10-21.
  The worktree got no install. Results and their caveats are in the RUNBOOKS section of round D3
  (commit f19b33ea): tsc -b 12 errors, all in quarry-client.ts; FleetRunConsole.test.tsx 1 of 1;
  vitest 3626 of 3632 tests; vite build exit 0, no fleet chunk.
- Route test: the plan adds one only if an App route harness exists. None exists (no test
  file under `src/app`), so no route test was added.
- The DoD "`pnpm build` emits no FleetRunConsole chunk": `pnpm build` runs `tsc -b` first, so
  it stops at the typecheck baseline. The chunk check was made on the output of `vite build`.
- Still open: the SPA container check (`/agents/fleets/abc` on a restarted container).

## WS5.A, WS5.B, WS5.H residuals (breakdown items j and p1)

- Unwrap audit of the scrape path: 0 production `unwrap`, `expect`, `panic!`, `unreachable!`,
  `todo!` or `unimplemented!` in the 13 files of the scrape path (`scripts/unwrap-audit.awk`,
  commit 8f82b83d). Re-run at HEAD b8b7495b on 2026-10-23: still 0, and the positive control
  reports its planted production calls. Nothing needed an annotation.
- `/v1/extract` content sniffing: explicit deferral, recorded in `docs/CONTRACTS.md` section 9.5
  (commit 662a08da) with its reason and its next step. The plan's default for X-14 and WS5.B
  is to record the deferral; it is recorded. Not built.
- `zdr_claim_floor` PDF test: `a_zdr_scrape_of_a_pdf_returns_inline_text_and_persists_nothing`
  added (commit da644578). Tightened and given a standard-org control in commit ff8fb158.
  Run on 2026-10-23 in the worktree at HEAD b8b7495b, with the test file that became ff8fb158: `cargo test -p quarry-edge --test zdr_claim_floor`
  through qtest.sh, 24 passed, 0 failed. The route-off mutant (fixture set to "off") makes the two PDF
  tests fail (2 failed, 22 filtered out), and the file was restored.
- quarry-edge, the whole test set: `cargo test -p quarry-edge --no-fail-fast` on 2026-10-23 through qtest.sh:
  32 test-result blocks, 1060 passed, 0 failed, 0 ignored (commit 8b121261). The shared worktree had other
  units' uncommitted quarry-edge hunks during the run, so the count is for that tree. The round D3 figure
  (29 blocks, 1016 passed) is superseded.
- openapi `ScrapeRequest`: the option list was already complete in the committed file (the
  parity test passes 23 of 23 node tests at HEAD b8b7495b). `x-quarry-lenient-until` added (commits
  01b76a99, corrected in ca82b042). `Envelope_Error.code` stays a free string; the 415 response names
  UNSUPPORTED_MEDIA.

Still open, stack evidence or CI (not D3's to close):
- WS5.B deployment probes (a) to (f) of the slice, including the crawl with a PDF and a PNG
  link and the 20 MB parse timing, and the staging re-ingest check.
- WS5.C, WS5.D, WS5.E and WS5.F evidence probes on the final stack (breakdown items j).
- WS5.H: the throwaway-failure demonstration and a green `quarry-v2-ci` run (CI).

## Suggested replacement lines for the plan (single writer decides)

- WS5.H DoD "openapi ScrapeRequest completion and x-quarry-lenient-until": "option list
  complete in the committed openapi.yaml; x-quarry-lenient-until added on 2026-10-21 (unit D3)".
- WS5.B DoD "the /v1/extract step landed or explicitly deferred in the PR": "explicitly
  deferred: CONTRACTS section 9.5 (2026-10-21)".
- WS5.B DoD "the zdr_claim_floor PDF test": "passes 24 of 24 with the route on; the route-off
  mutant fails the two PDF tests (2026-10-23, commit ff8fb158)".
- WS6.A test notes: "OnGetVersion availability verified (go.temporal.io/sdk v1.29.1);
  recorded-history replay still open (needs a captured history)".
