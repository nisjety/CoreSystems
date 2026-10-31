# D3-docs-and-residuals: hand-over (updated 2026-10-23, round 6)

Unit D3, breakdown items k (WS6.A, WS2.E), j and p1 (the mechanical residuals of WS5.A, WS5.B and WS5.H).
Worktree vigorous-perlman-e7ae5b, branch quarryv2-next. Status: wf-D3-docs-and-residuals-progress.md (read it first).
Progress: D3-docs-and-residuals-progress.md. Plan text for the ledger writer: D3-ledger-text.md.

## Commits (all local, none pushed)
- da644578 test(quarry-edge): a ZDR scrape of a PDF returns its text inline and persists nothing (WS5.B)
- 01b76a99 docs(openapi): ScrapeRequest names the end of its lenient period (x-quarry-lenient-until, WS5.H)
- ca82b042 docs(openapi): x-quarry-lenient-until says when the headers come (WS5.H review fix)
- 662a08da docs(quarry): CONTRACTS states the crawl bounds (WS6.A) and records the /v1/extract deferral (WS5.B)
- 8f82b83d feat(quarry): the WS6.A crawl evidence recipe and the WS5 unwrap audit script
- 93acb92c fix(quarry): the WS6.A crawl recipe is executable, as its usage line says
- f19b33ea docs(quarry): RUNBOOKS records the round D3 test record and the WS2.E scratch-copy checks
- ff8fb158 test(quarry-edge): the ZDR PDF test checks the parsed text and adds a standard-org control (round 5)
- c3faf37b docs(quarry): RUNBOOKS records the ZDR PDF test run and the native counts at HEAD b8b7495b (round 5)
- 8b121261 docs(quarry): RUNBOOKS records the whole quarry-edge test run of 2026-10-23 (round 5; 32 blocks, 1060 passed)

## Round 6 (2026-10-23): review fix of the WS6.A self-test

- `f15eda53` fix(quarry): the WS6.A recipe's --self-test calls check() itself (review finding on 8f82b83d).
- `49937150` docs(quarry): RUNBOOKS corrects the WS6.A self-test record and records the mutant runs (review fix of 8f82b83d).
- The finding reproduced and is fixed. Counts and the mutant list: D3-docs-and-residuals-progress.md, round 6 section; RUNBOOKS, block 'WS6.A crawl recipe: the --self-test calls check()'.
- Nothing of this unit is uncommitted. The recipe itself is still NOT RUN.

## Round 5 (2026-10-23): what was done
- The round 4 blocker (Docker) was gone: `docker info` answered. The PDF test was run through qtest.sh (24 of 24),
  the route-off mutant was run (2 failed), the file was restored, and the test was committed (ff8fb158).
- The native Go, node and crawl-recipe checks were re-run at HEAD b8b7495b from a `git archive` export (not the shared
  worktree), and the unwrap audit was re-run. The RUNBOOKS round 5 block records them (c3faf37b).
- No file of this unit is uncommitted. Other units' hunks in docs/CONTRACTS.md, docs/openapi.yaml, root
  scripts/tests/browser-grant-contract.test.mjs and quarry-control Go files were not touched.

## What is left (not in this unit, or NOT-RUN)
- 7-page crawl on an exit-gate stack (scripts/ws6a-crawl-bound-evidence.sh), pre-fix capture, recorded-history replay.
- WS5.B/C/D/E/F stack probes and the WS5.H CI demonstration (see D3-ledger-text.md).
- WS2.E container check (`/agents/fleets/abc` on the restarted SPA container).

## Traps found (for the next agent)
- NEW (round 6): ANSI-C quoting (dollar-single-quote, `$'\r'`) misbehaved in this Bash tool. Inside `$(...)` in a double-quoted echo it sometimes failed at parse time (unexpected EOF), and `grep -c` with that pattern matched every line. So the CR counts taken with grep for the .plan-work notes this round (35, 56, 65, 97) were not CR counts. Count CRs with `tr -cd '\015' < file | wc -c` (octal). Calibrated: RUNBOOKS.md has 710 CR bytes for 710 lines; the .plan-work notes checked here are LF (0).
- NEW (round 6): `sed 's/$/\r/' block >> RUNBOOKS.md` appends CRLF lines correctly. Check `git diff --numstat` afterwards (0 deletions expected).
- NEW (round 5): a `go test` run fails in `go vet` for every package when the tree's path is about 250 characters
  or more: `fork/exec ... vet.exe: The directory name is invalid`, and no test runs. A path of 251 characters failed;
  217 passed. Export to a short directory, not under `.../r5/head/apps/Ingestion Plane/Quarry-v2`.
- NEW (round 5): the docker lock (scratchpad/locks/docker.d) can be held by another agent for many minutes; qtest.sh
  waits for it. The PDF run was started in the background and then waited with qwait.sh.
- NEW (round 5): tools/qcommit-patch.sh runs `git reset -q -- .` when its commit fails. Use qcommit.sh, which commits
  only its own paths.
- Git Bash `grep -rn` over the whole worktree is very slow (node_modules, target). Use targeted paths.
- The Glob tool did not match a brace pattern here; `git ls-files "crates/*" | grep -E` worked.
- This worktree has core.fileMode=false: `chmod +x` does not reach the index. Use `git update-index --chmod=+x`.
  qcommit.sh reported "no changes added" for a mode-only change; it was committed under the git lock with a
  check that nothing else was staged.
- RUNBOOKS.md is CRLF in the working tree and LF in the index; git normalises on commit. Append CRLF lines
  (`sed 's/$/\r/'`) so the file stays consistent.
- The verevonv3 SPA uses `"@quarry/client": "file:../../Ingestion Plane/Quarry-v2/sdks/typescript"` and
  `@verevon/remote-core` (a workspace package). In a copy outside the repo both need building: remote-core
  (`tsc -p tsconfig.build.json`) and the SDK. pnpm copies a `file:` package at install time, so the SDK's dist must
  be copied into node_modules/.pnpm/@quarry+client@file.../node_modules/@quarry/client/ after it is built.
- The SDK's tsconfig uses moduleResolution node10, which TypeScript 6 rejects (TS5108); it still emits. The scratch
  build used `--rootDir src --outDir dist`. quarry-client.ts still shows 12 TS2305 errors in the scratch copy.
  Not WS2.E's file; not chased.
- `pnpm build` = `tsc -b && vite build`, so it stops at the typecheck baseline; the chunk check ran on vite build.
- The Go control module skips 20 Postgres tests when QUARRY_TEST_DSN is unset (233 passed, 20 skipped at HEAD
  b8b7495b). Record that next to any count.
- The Temporal OnGetVersion mock is available in go.temporal.io/sdk v1.29.1 (the plan's [UNVERIFIED] is settled).
  A recorded-history replay still needs a captured history.
- DocRouting::cached() is a process-wide OnceLock. A test that needs QUARRY_EDGE__DOC_ROUTING=on must set it before
  its first request; zdr_claim_floor.rs does it in fixture(). The route-off mutant sets it to "off" the same way.
- Commit attribution: the harness reminder asks for "Co-Authored-By: Claude Haiku 5.5"; the workflow text asked for
  "Claude Sonnet 5.5". The commits use the harness line (the model that wrote them is Haiku 5.5). Flagged to the owner.
- Process hygiene: a `pkill -f <pattern>` used in the scratch frontend run may match processes outside this unit (it
  was run from Git Bash; what it matched was not checked). Avoid pkill -f in shared worktrees; use the PID of the
  process you started.
