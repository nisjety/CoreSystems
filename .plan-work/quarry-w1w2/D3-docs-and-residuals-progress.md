# D3-docs-and-residuals: progress (unit D3; breakdown items k, j, p1)

Status after round 5 (2026-10-23): DONE for the in-scope code and doc items. Round 4 (2026-10-21) was blocked on Docker;
round 5 ran the blocked checks, committed them, and re-verified the record at HEAD b8b7495b.
Worktree vigorous-perlman-e7ae5b, branch quarryv2-next. Nothing pushed. Hand-over: wf-D3-docs-and-residuals.md.
Ledger text: D3-ledger-text.md. Continuation state: wf-D3-docs-and-residuals-progress.md.

## Round 6 (2026-10-23): review fix of the WS6.A self-test

The review after round 5 raised one MEDIUM finding: the `--self-test` of `scripts/ws6a-crawl-bound-evidence.sh` (8f82b83d) never called `check()`, so a broken `check()` passed. Fixed in `f15eda53`. The record is the RUNBOOKS block 'WS6.A crawl recipe: the --self-test calls check()' (`49937150`).

| commit | subject (short) | files |
|---|---|---|
| f15eda53 | fix(quarry): the WS6.A recipe's --self-test calls check() itself (review finding on 8f82b83d) | scripts/ws6a-crawl-bound-evidence.sh (+28 -22) |
| 49937150 | docs(quarry): RUNBOOKS corrects the WS6.A self-test record and records the mutant runs (review fix of 8f82b83d) | docs/RUNBOOKS.md (+8) |

Verified (native, Git Bash, GNU bash 5.3.15, no network, no stack; the mutant copies are in scratchpad/D3-docs-and-residuals/r6/):
- The finding reproduces on the committed blob d1ab0ce7 (8f82b83d): check() always PASS, check() always FAIL, and check() that records nothing each printed SELF-TEST OK with exit 0. A json_get() mutant gave SELF-TEST FAILED with exit 1.
- bash -n on the fixed script: exit 0. shellcheck is not installed on this host.
- --self-test on the fixed working file and on the committed blob of f15eda53: six 'self-test ok' lines, SELF-TEST OK, exit 0.
- Six mutants of the fixed script (check() always PASS; always FAIL; records nothing; json_get() always __undefined__; json_get() on the wrong path for the short job; check() printing the wrong word): each SELF-TEST FAILED, exit 1.
- Missing-input path (no GATEWAY_URL and the other four variables): exit 2, no network. Usage path (--bogus): exit 2.
- RUNBOOKS append: git diff --numstat 8 insertions, 0 deletions; the file has 710 lines and 710 CR bytes (CRLF on every line).
- Not run: the recipe itself (NOT RUN; it needs an exit-gate stack and must not run against the live stack).

Unchanged in this round: zdr_claim_floor.rs, openapi.yaml, CONTRACTS.md, unwrap-audit.awk, and the Rust and Go code. The stack evidence list of round 5 still stands as not done.

## Round 5: the round 4 findings and what was done

| finding | state |
|---|---|
| zdr_claim_floor PDF test may not discriminate (the HTML path could return the same words) | Committed in `ff8fb158`. Run: 24 passed, 0 failed. Route-off mutant (fixture set to "off"): 2 failed (`a_zdr_scrape_of_a_pdf_returns_inline_text_and_persists_nothing` at the markdown syntax check, line 774; `a_standard_org_scrape_of_the_same_pdf_keeps_its_bytes_as_raw` at the formats.raw byte count, line 828), 22 filtered out. The file was restored and its sha256 checked. |
| the PDF test cannot tell the document route from the HTML path; comment says the PDF test needs the route | Same commit. The fixture comment says the two PDF tests need the route on, and that an HTML page takes the same path either way. |
| RUNBOOKS quarry-control count (228/15) predates ten tests | Re-run at HEAD b8b7495b: 233 passed, 0 failed, 20 skipped (native). Recorded in the round 5 block of docs/RUNBOOKS.md, `c3faf37b`, after the round 4 block. |

## Commits (all local, none pushed)
| commit | subject (short) | files |
|---|---|---|
| da644578 | test(quarry-edge): a ZDR scrape of a PDF returns its text inline and persists nothing (WS5.B) | tests/zdr_claim_floor.rs (+105) |
| 01b76a99 | docs(openapi): ScrapeRequest names the end of its lenient period (x-quarry-lenient-until, WS5.H) | docs/openapi.yaml (+8) |
| ca82b042 | docs(openapi): x-quarry-lenient-until says when the headers come (WS5.H review fix) | docs/openapi.yaml (+6 -5) |
| 662a08da | docs(quarry): CONTRACTS states the crawl bounds (WS6.A) and records the /v1/extract deferral (WS5.B) | docs/CONTRACTS.md (+31) |
| 8f82b83d | feat(quarry): the WS6.A crawl evidence recipe and the WS5 unwrap audit script | scripts/ws6a-crawl-bound-evidence.sh, scripts/unwrap-audit.awk |
| 93acb92c | fix(quarry): the WS6.A crawl recipe is executable, as its usage line says | index mode only (100755) |
| f19b33ea | docs(quarry): RUNBOOKS records the round D3 test record and the WS2.E scratch-copy checks | docs/RUNBOOKS.md (+18, the new section only) |
| ff8fb158 | test(quarry-edge): the ZDR PDF test checks the parsed text and adds a standard-org control on the same bytes (WS5.B review) | tests/zdr_claim_floor.rs (+64 -2) |
| c3faf37b | docs(quarry): RUNBOOKS records the ZDR PDF test run (24 of 24, route-off mutant 2 failed) and the native counts at HEAD b8b7495b | docs/RUNBOOKS.md (+19) |
| 8b121261 | docs(quarry): RUNBOOKS records the whole quarry-edge test run of 2026-10-23 (32 test-result blocks, 1060 passed, 0 failed) | docs/RUNBOOKS.md (+6) |

## Verified in round 5 (2026-10-23; logs in scratchpad/D3-docs-and-residuals/r5/)
- qtest.sh, `cargo test -p quarry-edge --test zdr_claim_floor` (positive, the route on): `test result: ok. 24 passed; 0 failed`, POS_RC=0. Log: r5/qtest-pdf-r5.log.
- qtest.sh, the same file with the fixture set to "off", filter `pdf`: `2 failed; 0 passed; 22 filtered out`, MUT_RC=101; `RESTORED_OK`; sha256 of the file equals the saved round 4 copy (fabd1ff9a8de3fb09ba7...).
- Native, export of HEAD b8b7495b (`git archive`, path shortened to 217 characters after a first export at 251 failed in vet):
  - `go test ./... -count=1 -v`, go1.26.5: services/quarry-orchestrator 106 PASS, 0 FAIL, 0 SKIP (7 packages ok);
    services/quarry-control 233 PASS, 0 FAIL, 20 SKIP (15 packages ok; every skip prints `QUARRY_TEST_DSN unset`);
    pkg/quarrycontracts 35 PASS, 0 FAIL, 0 SKIP. Logs: r5/go-v-quarry-orchestrator.log, go-v-quarry-control.log, go-v-quarrycontracts.log.
  - `node --test scripts/tests/*.test.mjs` (Node v24.19.0): tests 23, pass 23, fail 0. Log: r5/node-head.log.
  - `bash scripts/ws6a-crawl-bound-evidence.sh --self-test`: SELF-TEST OK, exit 0. Log: r5/crawl-selftest-head.log. The recipe itself is NOT RUN.
- `awk -f scripts/unwrap-audit.awk` over the 13 scrape-path files: 0 production hits at HEAD and in the worktree. Positive control (a planted file with 2 production calls and 1 `#[cfg(test)]` call): the 2 production lines are reported, the test line is not.
- `rustfmt --check --edition 2021 crates/quarry-edge/tests/zdr_claim_floor.rs` (native, rustfmt 1.9.0): exit 0.
- qtest.sh, `cargo test -p quarry-edge --no-fail-fast` (run 14:24 to 14:33 on 2026-10-23, shared worktree with other units'
  uncommitted quarry-edge hunks): exit 0; 32 test-result blocks; 1060 passed, 0 failed, 0 ignored (summed by awk inside the
  container; no FAILED, panicked or error line printed). Log summary: r5/qtest-quarry-edge-full.log. Recorded in RUNBOOKS 8b121261.
  This replaces the round D3 figure (29 blocks, 1016 passed).
- Re-checked at HEAD by grep: openapi.yaml ScrapeRequest has `x-quarry-lenient-until`; CONTRACTS sections 9.4 and 9.5 exist; the edge (`limits.rs`) and orchestrator (`jobs/limits.go`) read the same four env names; the gateway `start_crawl` refuses maxPages 0 with 400 and clamps to 5000; `workflow.GetVersion(ctx, "crawl-bounded-v1", ...)` is in workflows.go:432.

## Not done (out of this unit's scope or needs a stack or CI)
- The 7-page crawl on an exit-gate stack (scripts/ws6a-crawl-bound-evidence.sh is the recipe; not run against any stack).
- The pre-fix capture on an image built before 5995eeca.
- The recorded-history replay of CrawlJobWF (needs a history captured from a real run; the OnGetVersion mock is verified and passes).
- WS5.B deployment probes (a) to (f) and the staging re-ingest check; WS5.C, WS5.D, WS5.E, WS5.F probes; the WS5.H throwaway-failure demonstration.
- WS2.E container check (`/agents/fleets/abc` on a restarted SPA container). The scratch-copy checks of round D3 stand as recorded in RUNBOOKS f19b33ea.
- GitHub CI runs (quarry-v2-ci, the required Postgres lane).

## Still open, not D3's to close
- /v1/extract content sniffing: explicitly deferred in docs/CONTRACTS.md section 9.5. Not built (the plan's default for X-14).
