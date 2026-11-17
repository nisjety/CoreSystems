# Test-phase rules for every Space W3 test builder (read fully before writing or running anything)

The code phase for wave W3 (plan `docs/plans/space-hybrid-build-plan-2026-10-14.md`, section 3.E) is
done and committed on this branch: all 30 plan slices, 18 fix slices and 20 owner decisions (D1-D20),
plus one lead fix (`LEAD-FIX-GLOBALKILL-ZDR`, a ZDR leak in a platform-wide plugin kill). You are now in
the TEST phase: the "no tests, no builds" rule from the code phase is LIFTED for you. You write the
tests the code phase listed as owed, make them pass against the code as it stands, and run them for
real.

## Where you work
- Worktree `C:/dev/space-w3`, branch `space-w3`. Your FIRST command is `cd "C:/dev/space-w3"` and
  `git branch --show-current` must print `space-w3`. Use absolute paths with Read/Edit/Write/Grep/Glob.
  NEVER edit `C:/dev/CoresSystem` (the live main checkout) or any other worktree, any `.env*` file.
- You own ONE area (language/service), given to you by the lead. Only touch files inside it. Other
  builders run at the same time on other areas (other Go modules, other Rust crates, the SPA, Convex,
  scripts) -- never touch a file outside your assigned area even if a test you write would be more
  complete that way; note the gap instead (see depth file).

## What tests_owed means
- Every unit file at `.plan-work/space-build/units/<slice>.json` has a `tests_owed` array: each entry is
  `"<path>: <what it asserts>"`. The lead has told you which slice ids are yours. Read each one's json,
  but only act on `tests_owed` entries whose path falls inside YOUR area -- a slice can owe tests in
  several areas (Go, Rust, SPA) and another builder owns the rest.
- `.plan-work/space-build/depth/<slice>.md` lists the bugs/risks/gaps the code-phase builder already
  found for that slice (P0/P1 first). Read it for your slices: it tells you what a test should actually
  prove, and flags known pre-existing issues you should not re-report as new.

## What to do
1. For each `tests_owed` entry in your area: write the test (new file, or a new case in an existing test
   file named in the entry). Match the existing test style and helpers in that file/package exactly --
   grep for a sibling test first.
2. Run the real test command for your area (see "Run commands" below). Iterate until your new tests
   compile and the ones that should pass do.
3. If a test you write FAILS because the code is wrong (not because the test itself is wrong), fix the
   minimal code defect -- same discipline as the code phase: fix what makes your area's own behaviour
   wrong or breaks a hard rule (ZDR fails closed, Control-signed authority, no cross-plane DB access, no
   ACL widening); do not refactor or gold-plate past that.
4. If a test needs infrastructure you must not start (a live Postgres/NATS/etc. container, a running dev
   stack, a real CI runner) -- see "What NOT to run" -- write the test so it compiles and is correctly
   gated (e.g. Rust `#[ignore]`, a skip guard), but do not try to make it execute. Report it as
   `not-run: needs <what>` in your result file and final message, not as a pass or a bug.
5. Known pre-existing failures: some suites have pre-existing failures unrelated to this wave (see
   "Known pre-existing noise" below). Diff your new failures against that baseline before reporting a
   bug -- a failure that also fails on an unrelated file, or matches a listed pre-existing pattern, is
   noise, not a new finding. Say which is which in your report.
6. Keep diffs minimal. Never rewrite or reformat a file to add one test. Re-read a shared file right
   before editing it -- other builders edit other files in the tree concurrently (not yours, but the
   repo moves under you).

## What NOT to do
- Do not run `docker compose up`, `make up`, `./scripts/compose.sh up -d`, or start any other long-lived
  service/container. Several builders run in parallel; a shared dev stack is not yours to start or stop,
  and port collisions will break siblings. If a test needs one and it is not already running, mark it
  `not-run: needs a live stack` and move on.
- Do not run a full `pnpm build`, `cargo build --release`, or any slow release build. Use the test/check
  commands given below (they compile what they need).
- Do not touch another builder's area, even to fix a cross-area test.
- Do not add tests beyond what `tests_owed` lists for your area unless a hard rule is at risk (same
  "no depth beyond your slice" discipline as the code phase) -- note anything else you noticed as a P2/P3
  depth bullet instead.
- Never `git add -A`/`.`, never `--no-verify`, never `git reset`/`stash`/`checkout`/`clean`/`rebase`/
  `merge`/`push`/`amend`, never touch `.git/config`. If `.git/index.lock` exists, wait 8 seconds and
  retry up to 15 times.
- Never print, copy, log or write any API key, token, password or env value. Never read `.env*` files.

## Run commands (your area tells you which apply)
- Go (capability-core, user-core, and other Model/Control Plane Go services): a `go.work` exists at
  `C:/dev/space-w3` and at least one service needs it present (capability-core) -- run `go test ./...`
  from the SERVICE's own directory WITH the workspace active (do not set `GOWORK=off`). `-race` needs
  cgo, which is unavailable here -- do not pass `-race`.
- Rust (session-core, model-gateway, mp-contracts, execution-core, retrieval-engine-rs): from
  `apps/Model Plane/rust`, `cargo test -p <crate> --lib` for unit tests. Two cargo targets are known to
  fail to build on Windows for unrelated reasons -- if your crate is one of them, use `--lib` only (skip
  integration test binaries) and say so rather than treating it as your bug.
- Rust (verevonv3 gateway BFF): `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --lib`.
- TypeScript/SPA (`apps/Frontend Plane/verevonv3`): `pnpm vitest run <path>` for the files you touched,
  not the whole suite (it is large and has pre-existing failures -- see below). `tsc6 -p tsconfig.app.json`
  for a real type-check if you add/change types (plain `tsc -p tsconfig.json` checks nothing in this repo
  -- do not use it).
- Convex (`apps/Application Plane/convex-core`): use that package's own test command (check its
  `package.json`); do not restart the Convex gateway container.
- Shell contract tests (`scripts/tests/*.test.sh`, `scripts/gates/*`): run them directly with `bash`.
  Several are pinned-digest self-consistency checks against `apps/Model Plane/api/contracts/*.json` --
  if one fails only because a digest is stale, recompute and fix the pin (do not touch the registry's
  rows), and say so.

## Known pre-existing noise (do not re-report these)
- `apps/Control Plane/auth-core`: full suite has 15 pre-existing failures across 3 test suites, unrelated
  to this wave. Diff your failing set against a clean-checkout run before calling anything a new bug.
- `apps/Frontend Plane/verevonv3`: pre-existing typecheck/lint/vitest failures exist outside this wave's
  files; `tsc -p tsconfig.json` checks nothing (always "passes") -- use `tsc6 -p tsconfig.app.json`.
  Prettier needs `--end-of-line auto` to be EOL-agnostic (repo files are CRLF).
- session-core/model-gateway DB-backed tests marked `--ignored` need a live Postgres and are out of
  scope here (see "What NOT to run"); do not try to stand one up yourself.
- `nats-provisioner`, lint-ratchet ENOBUFS, and a couple of other pre-existing environmental failures are
  not this wave's concern unless your own new code is what trips them.

## Commit (you commit your own area's test changes)
`git add -- <your files>` then `git commit -m "<message>" -- <your files>` with message
`test(space): <slice-or-area id> <short what>`, a blank line, and the trailer
`Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>`. Path-limited commits only. The pre-commit
hook runs gitleaks: if it reports a "secret", change the literal in YOUR file (fake tokens: repeated
letters) and recommit. Do not commit the test-units/test-depth files (the lead does).

## Result files (always write both)
1. `.plan-work/space-build/test-units/<area>.json`:
   `{"area":"...","status":"tests-passing|tests-failing|partial|blocked","commit":"<short sha or empty>",
   "files":["repo-relative paths changed/added"],"run_command":"<exact command you ran>",
   "result":"<e.g. 14 written, 12 passing, 2 not-run (needs live stack)>",
   "bugs_fixed":["<slice>: one line"],"not_run":["<path>: needs <what>"],"notes":"<=6 lines"}`.
2. `.plan-work/space-build/test-depth/<area>.md`: at most 12 bullets, each
   `- P0|P1|P2|P3 kind: one sentence` (kinds: bug-fixed, bug-found-not-fixed, test-gap, blocked,
   pre-existing, follow-up). A bug your tests caught and you fixed is P0/P1 bug-fixed; one you found but
   could not fix in your area (e.g. it is actually another area's code) is bug-found-not-fixed and must
   name the area that owns it.

## Final message (your report to the lead)
Return exactly these sections, short:
- `STATUS:` tests-passing | tests-failing | partial | blocked, and the commit sha.
- `RAN:` the exact command(s), and the pass/fail counts.
- `BUGS FOUND AND FIXED:` every real defect your tests caught, with file path and one line each.
- `NOT RUN / BLOCKED:` anything you could not execute and why (needs a live stack, pre-existing failure,
  wrong area).
- `BUGS/ISSUES:` anything else worth the lead knowing (test-gaps you left, P0/P1 first).

## Cost discipline
Read only what you need. Do not re-run the full suite repeatedly -- run your new/changed tests directly,
then a final targeted run to confirm. Stop as soon as your area's owed tests are written, run (or
correctly marked not-run) and committed.
