# D3-docs-and-residuals: continuation state (round 6 review fix, 2026-10-23; round 5 below)

## Round 6 (2026-10-23): review fix of the WS6.A self-test (newest)

- The one MEDIUM finding of the review is fixed and committed. The WS6.A `--self-test` (8f82b83d) never called `check()`; it re-implemented the comparison, so a broken `check()` passed.
  - `f15eda53` fix(quarry): the WS6.A recipe's --self-test calls check() itself. It drives the real `json_get()` and `check()` with the recipe's own paths and expected values, and asserts the verdict each one records and prints (six verdicts).
  - `49937150` docs(quarry): RUNBOOKS corrects the WS6.A self-test record and records the mutant runs. It appends one block at the end of `docs/RUNBOOKS.md`.
- Evidence (native, scratch copies, no network, no stack): on the blob of 8f82b83d (d1ab0ce7), three `check()` mutants each printed SELF-TEST OK with exit 0. On f15eda53, six mutants each print SELF-TEST FAILED with exit 1, and the committed blob prints SELF-TEST OK with exit 0.
- 8f82b83d's commit message overstated the self-test. It was not amended (history rewrites are forbidden in this tree); f15eda53 and 49937150 correct it.
- Still NOT RUN: the 7-page crawl recipe (it needs an exit-gate stack). Nothing else of this unit changed. The `.plan-work/` notes are not committed.
- Trailer: commits carry `Co-Authored-By: Claude Haiku 5.5` (the harness reminder), as in round 5. The computed text asked for Sonnet 5.5. Flagged to the owner, not resolved here.
- HEAD moves while other units commit. Check `git status` on your own paths before each commit, and commit only your own paths (qcommit.sh does that).

Read this first. Unit D3 (items k, j, p1, the mechanical ones). Worktree
C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b, branch quarryv2-next. Round 5 commits: ff8fb158, c3faf37b, 8b121261. Another unit committed fffe3627 on top during round 5.
Full state: D3-docs-and-residuals-progress.md. Hand-over: wf-D3-docs-and-residuals.md. Ledger text: D3-ledger-text.md.

## Where it stands: DONE for the in-scope code and doc items
- Round 5 (2026-10-23) finished the round 4 work that had been blocked on Docker:
  - `ff8fb158` test(quarry-edge): the ZDR PDF test checks the parsed text and adds a standard-org control.
    `cargo test -p quarry-edge --test zdr_claim_floor` through qtest.sh: 24 passed, 0 failed. Route-off mutant: 2 failed, RESTORED_OK.
  - `c3faf37b` docs(quarry): RUNBOOKS round 5 block (the PDF run, the native counts at HEAD b8b7495b, the audit re-run).
- Uncommitted D3-owned files: none. `git status` is clean for zdr_claim_floor.rs and docs/RUNBOOKS.md.
- Not D3's, left alone (other units' uncommitted hunks): docs/CONTRACTS.md and docs/openapi.yaml (taken_over_at and
  agent.expired initiatedBy hunks), the root scripts/tests/browser-grant-contract.test.mjs, and the quarry-control Go files
  (tombstones.go, gdpr_purge.go, zdr_tombstones.go, migration 024).
- What is left is stack evidence or CI (the 7-page crawl, the pre-fix capture, the recorded-history replay, the WS5.B to
  WS5.F probes, the WS2.E container check, GitHub CI). Not run here: the unit's rules keep them off the live stack.

## Facts a successor needs
- Docker answered in round 5 (`docker info` exit 0). qtest.sh is the only Rust path. The docker lock was held by another
  agent for a while; the PDF run then took about 5 seconds of test time.
- Native counts at HEAD b8b7495b, from a `git archive` export (not the shared worktree): orchestrator 106/0/0,
  quarry-control 233/0/20, pkg/quarrycontracts 35/0/0, node 23/23, crawl --self-test OK. Logs: scratchpad/D3-docs-and-residuals/r5/.
- TRAP: `go test` fails for every package with `fork/exec ... vet.exe: The directory name is invalid` when the tree path is
  about 250 characters or more (251 failed; 217 passed). Export to a short directory, for example
  scratchpad/D3-docs-and-residuals/r5/h, and not to .../r5/head/apps/Ingestion Plane/Quarry-v2.
- The pre-commit hook runs gitleaks through docker on every commit (it passed in round 5: "no leaks found").
- The whole quarry-edge test set (`cargo test -p quarry-edge --no-fail-fast`, qtest.sh): exit 0, 32 test-result blocks,
  1060 passed, 0 failed, 0 ignored. It ran on the shared tree with other units' uncommitted quarry-edge hunks. Recorded in
  RUNBOOKS (8b121261). Log summary: scratchpad/D3-docs-and-residuals/r5/qtest-quarry-edge-full.log.
- The scratchpad tool `qcommit-patch.sh` runs `git reset -q -- .` when its commit fails. Avoid it in a shared tree; `qcommit.sh` commits only its paths.
- Attribution: the harness reminder asks for "Co-Authored-By: Claude Haiku 5.5" and the commits use that line. The
  computed workflow text asked for "Claude Sonnet 5.5". The conflict is flagged to the owner; it was not resolved here.
- Stale text kept on purpose (append-only RUNBOOKS): the round D3 record says quarry-control 228/15 and zdr_claim_floor 23 of 23;
  the round 4 and round 5 blocks at the end of the file correct both.
