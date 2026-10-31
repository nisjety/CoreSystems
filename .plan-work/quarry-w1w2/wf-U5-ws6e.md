# U5-ws6e hand-over: WS6.E, the grant-carried budget claim

Design note: wf-U5-ws6e-design.md. Progress file: wf-U5-ws6e-progress.md. Written 2026-10-19 ~19:15.

## State when this unit stopped

The docker engine stopped answering at about 17:51 (HTTP 500 from `docker info`, then hangs; Docker Desktop's vhdx is 315 GB and the
host disk touched 100% at 17:30). qtest printed "DOCKER IS NOT ANSWERING". The pre-commit hook (gitleaks through docker) cannot run, and
no local gitleaks exists, so **no commit could be made**; the rule is no --no-verify. Everything below is in the working tree, uncommitted.
Other units' commits are stuck the same way (their files show as staged: browser-broker-ci.yml, quarry-browser live_gate.rs, ssrf.rs).

Step 1 (proto) is STAGED, not committed (message at scratchpad/U5/msg1.txt). My killed gitleaks container made the hook print "found a secret";
that was the kill, not a finding.

## What was built (all in the tree)

1. Proto 40-44 on AcquireGrantRequest/Response, ValidateGrantResponse, DeriveRunGrantRequest/Response (`browser.proto`, Go regen with
   local protoc-gen-go through scratchpad/U5/buf.gen.local.yaml (remote plugins need the network), vendored Quarry proto, the L1 contract test).
   `node --test scripts/tests/browser-grant-contract.test.mjs`: 18 passed.
2. Broker: `internal/grant/budget.go` (Budget, NormalizeBudget, tightenBudget, ErrInvalidBudget, ErrDeriveWidensBudget), grant.go (Grant.Budget, params, memory
   store), postgres.go (columns, insert, derive), server.go/errors.go (parse, return, PermissionDenied), migrations/0003 up+down, tests (budget_test.go in grant and
   server, postgres_integration_test.go: budget contract, durable/nullable/constrained test, migration rerun now covers 0003).
   Native: `GOWORK=off go vet ./...` and `-tags integration` clean; `go test -count=1 ./...` ok; two mutants killed (see msg2.txt).
   NOT RUN: the tagged Postgres suite (needs docker): `bash scratchpad/tools/with-lock.sh docker 2700 -- go test -count=1 -tags integration ./internal/grant/...`
3. Quarry (NOT COMPILED, NOT TESTED: no docker, and a native build needs libclang for boring-sys2):
   runtime: grant_validator.rs (GrantBudget, GrantValidation.budget, from_wire), grpc/browser_broker.rs (validation_from_response), step_receipts.rs
   (BudgetCheckpoint.claim);
   edge: budget.rs (ClaimMode, resolve_caps_with_claim, tighter_claim, RunBudget::start_with_claim and snapshot, limit_source grant, tests), control.rs
   (checkpoint takes budget.snapshot), main.rs (boot check), agent_routes/lifecycle.rs (snapshot the grant's claim at start, run grant means agent class
   when enforcing), agent_routes/tests.rs (one `budget: None`), tests/grant_budget_claim.rs (new, 7 route-level tests), docs/openapi.yaml (`by: grant`).
   The code was read through twice for compile errors; run before trusting it:
   `bash scratchpad/tools/qtest.sh "$(cat scratchpad/U5/q1.sh)"` (runtime lib, runtime grpc, edge lib budget, edge grant_budget_claim and start_run_grants),
   then `cargo check --workspace --all-targets` and `cargo test -p quarry-edge` / `-p quarry-runtime`.
4. model-gateway `browser_run.rs`: BudgetClaim, BudgetMint, mint_budget (flag MODEL_GATEWAY_BROWSER_BUDGET_CLAIM=on), derive request fields, returned claim checked,
   tool_input tightened, 11 tests. Native (the host builds model-gateway; PROTOC_INCLUDE needed): 49 browser_run tests pass; whole lib 1740 pass, 1 fail (the known
   CRLF zdr_precheck test), 4 ignored; a mutant is killed.
5. Docs: RUNBOOKS.md appended (A grant carries a budget claim), STALE_DOC_DELETION_REGISTER.md appended (the stale Python/TS gens), plan ledger row WS6.E (no
   commit hashes yet: fill them in).

## How to commit when docker is back (nothing else stands in the way)

Shared files with another unit's uncommitted hunks: step_receipts.rs, lifecycle.rs, agent_routes/tests.rs, docs/openapi.yaml, RUNBOOKS.md may too.
Either wait for that unit's commit, or stage only my hunks: `node scratchpad/U5/stage-hunks.js <path> <regex-of-my-lines> [deny-regex]` (git apply --cached of
-U0 hunks), and commit with `git commit -F msg` after checking `git diff --cached --name-only` (other agents' staged files will be in the index right now:
commit with a pathspec for whole-file commits, as qcommit.sh does). Messages: scratchpad/U5/msg1.txt (proto, staged), msg2.txt (broker), msg4.txt (model-gateway).
Quarry and docs messages are not written yet: write them after the Quarry tests have run, with the real counts.

## Not done (and why)

- execution-core reading the claim, the planner token meter and the 402 mapping: sibling (b)'s files, the plan's +2 days.
- Flipping either flag, any rebuild or restart, the deployment evidence run.
- A claim on a run grant a caller presents (the broker cannot change an issued grant).
- `source: entitlement` minting (WS6.D).
- WS1.A2 (Quarry's broker credential, X-1): without it no production run carries a claim.

## Traps

- The disk hit 100% at 17:30 and recovered; a Write tool call failed with ENOSPC (no repo file was touched by it).
- `cargo test` takes ONE positional filter; use `cargo test ... -- a b`.
- The docker `gitleaks` child of a commit that hangs holds the shared git lock and `index.lock` for every agent. Kill that child (the commit then aborts and the files stay staged).
- Native Quarry builds fail on Windows (boring-sys2: long path then libclang). Model-gateway and the broker build natively.
- A quarry-edge integration test cannot pin the cached budget mode; `QUARRY_EDGE__GRANT_BUDGET_CLAIM` is read per start so tests set it, `QUARRY_EDGE__BUDGET_ENFORCEMENT` is set once for the test binary.
