# Rules for every Space W2 test-phase agent

Wave W2 of the Space plan is built (all slices committed on branch `space-w2` in the worktree `C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/w2`, which main has been fast-forwarded to). Your FIRST shell command is `cd "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/w2"`; every relative path here is relative to it, and after any `cd` into a subfolder you `cd` back. `w2/` is a git worktree nested inside the planning worktree practical-mccarthy-1dc567: run every git command inside `w2/` and check that `git branch --show-current` prints `space-w2` before you commit. Never edit the planning worktree outside `w2/`. You now write and run the tests that the builders deliberately postponed (`tests_owed` in `.plan-work/space-build/units/<slice>.json`). Owner strategy (binding): tests come at the END of the wave and must NOT stall progress: time-box everything, keep the tree green, log findings, never reopen the build.

## Where
- Work only inside the `w2` worktree. Never touch `C:/dev/CoresSystem` (live main checkout), other worktrees, `.env*` files, docker, or the running dev stack. No docker, no DB harness, no live LLM or network calls, no Playwright: those are a later tier. Never print secrets.
- Test agents run in parallel on different areas; edit only the test files and the minimal `#[cfg(test)]` / test-helper lines you need. Do NOT change production behaviour.

## What to do (per assigned slice list)
1. For each slice: read its `tests_owed` list and the slice's DoD/Tests paragraph in the plan (`docs/plans/space-hybrid-build-plan-2026-10-14.md`, workstream subsection; locate by Grep of the slice id) and the committed code (`git show --stat <commit>` from the unit JSON, then read the changed files you must test).
2. Write the tests in the repo's existing style and location (Rust: `#[cfg(test)] mod tests` or `tests/*.rs`; Go: `*_test.go`; Vitest: `*.test.ts(x)` next to the source; node `--test` scripts as the repo does). Pure-logic tests first (authority guards, parsers, state machines, normalisers, error mapping, ZDR refusals, flags default OFF, typed envelopes, idempotency). A test that needs a database, docker, network or a browser is still WRITTEN (so the later tier can run it) but gated: Rust `#[ignore = "needs DB harness"]`, Go build tag `integration`, Vitest `describe.skip`/`it.skip` with the reason; count them as `deferred`.
3. Run ONLY your new tests plus the existing tests of the files you touched, time-boxed (15 minutes per command, 45 in total). Commands:
   - Model Plane Rust: `cd "apps/Model Plane/rust" && CARGO_TARGET_DIR=C:/dev/space-target-mp CARGO_INCREMENTAL=0 cargo test -p <crate> --lib <filter>` (never `--all-targets`/`--tests` for model-gateway or inference-core; for a `tests/*.rs` target use `--test <name>` only if it compiles). Never create another target dir; never `cargo clean`.
   - Gateway: `cd "apps/Frontend Plane/verevonv3/apps/gateway" && CARGO_TARGET_DIR=C:/dev/space-target-gw CARGO_INCREMENTAL=0 cargo test <filter>`.
   - Go: `cd <service dir> && go test ./<pkg>/... -run <regex>` (no `-race`: cgo is off on this host).
   - SPA: `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run <files>`; repo scripts with `node --test <file>`.
   - Convex: the package's own test runner if it exists (`node --test` or vitest); otherwise typecheck only and say so.
4. Triage each failure: (a) a test-side mistake or a stale existing test/literal that W1 or W2 changed (struct literals, mocks, goldens, snapshots, admission tables): FIX it (test-side only, minimal); (b) a production defect or a behaviour that contradicts the plan: do NOT fix production code; mark that test ignored/skipped with `DEPTH-<n>` in the reason and log a depth row; (c) pre-existing failure unrelated to W2: log it as `pre-existing` and move on. A red test must never be committed un-skipped.
5. Commit: `git add -- <your test files>` then `git commit -m "test(space): <slices> <what>" -- <your files>` + blank line + trailer `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Path-limited only; no `--no-verify`, no stash/reset/checkout/clean/rebase/merge/push. gitleaks hook: use repeated letters for fake tokens; retry if the Docker engine is momentarily unavailable (60 s, up to 5 times); retry on `index.lock` (8 s, up to 15 times).

## Result files (always write both)
1. `.plan-work/space-build/tests/<agent id>.json`: `{"agent":"...","slices":[...],"written":N,"run":N,"passed":N,"failed_fixed":N,"skipped_deferred":N,"depth_rows":N,"commit":"<sha>","commands":["..."],"notes":"<=6 lines"}`.
2. `.plan-work/space-build/depth/tests-<agent id>.md`: at most 15 bullets `- P0|P1|P2|P3 test-fail|pre-existing|test-gap|deferred: <test or area>: one sentence` (P1 for a failing test that exposes a production defect, with the DEPTH id used in the skip reason).
Return the same object as your final message.

## Cost discipline
Read only what you need; do not explore unrelated code; stop when the assigned tests are written, run, triaged and committed; if the time-box runs out, commit what is green and list the rest as `deferred`.

## Wave W2 additions
- The Model Plane Go services build inside `apps/Model Plane/go/go.work`: do not set GOWORK=off for build or test there.
- SPA typecheck baseline is ONE error (schedule-contract.test.ts TS2352); keep it there.
- Solid 2: aria-pressed/aria-expanded take 'true'/'false' strings, there is no onMount (use onSettled), inputs use maxlength.
