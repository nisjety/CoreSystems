# Build rules for every Space W1 builder (read fully before editing)

You build ONE slice of the approved plan `docs/plans/space-hybrid-build-plan-2026-10-14.md` (Space hybrid room, wave W1 = milestones M0 + M1). The owner's strategy (binding): **build the wave to scope first; do not go back and build depth; log depth findings; tests are written and run at the END of the wave, not now; nothing may stall progress.** Other builders run at the same time in other lock groups on other files.

## Where you work
- Your current working directory is the build worktree (git branch `space`, cut from the sibling branch `model-plane` at e603fde78). ALL edits happen inside it. NEVER edit `C:/dev/CoresSystem` (the live main checkout), any other worktree, any `.env*` file, or anything outside this worktree except the files named below under `.plan-work/space-build/`.
- Plan paths and line numbers were taken from the live tree on 2026-10-14..20. This branch also contains the sibling Model Plane build (ZDR Z-series, durable loop, retention receipt, migrations up to 0068, user-core up to 032, org-core up to 024, etc.), so lines drift and some files differ: locate by SYMBOL with Grep, not by line number. If the plan's premise is false in this tree (symbol missing or renamed, or the slice is already built by a sibling), adapt minimally and record a `DEVIATION:` bullet in your depth file; if the slice is already present, set status `already-present` and finish.
- The plan overrides the per-workstream design file `.plan-work/design/WSnn.md` where they differ (section 2 decisions, section 4 sequencing/numbers, section 5 cross-session owners win over the workstream text).

## What to read (and only that)
1. Your slice in the plan: Grep the slice id in the plan; read the workstream subsection (`#### WSnn ...` up to the next `####`/`###` heading): slice table row, file-level change list for your slice, API/proto/data-model/migration paragraphs, flags, DoD. For `P-*` and `M0-*` rows the definition is in sections 4.1.1 (M0 exit criteria), 4.4.2 (prep refactors), 4.5 and 4.6; Grep the id.
2. Plan 4.2 (your row: lane, dependencies), 4.4.1 (hotspot rules for files you touch), 4.6 (migration, proto, flag reservations).
3. The design file for detail, and the unit notes of your dependencies: `.plan-work/space-build/units/<dep>.json` (what they changed).
4. The code you will change. Read each file only as deeply as needed.

## What to build
- Implement the slice completely per its row and file list: routes, RPCs, proto fields, migrations (up and, where the directory has a `revert/` convention, down), Convex functions, SPA components, flags, action-registry entries (an action contract in `src/shared/actions` BEFORE any UI operation), typed error envelopes `{error:{code,message,details}}`.
- Hard rules (a violation makes the slice wrong, so fix it inside the slice): Control owns authority/identity/audit/entitlements and every durable Space write carries a verified Control decision; ZDR fails closed with the typed refusal catalogue (`apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs`) on every content-persisting route, event, cache, index and notification; Space access never widens an owner-plane ACL; Model proposes and Quarry executes browser actions; no cross-plane database access; no durable Space state in the gateway or Convex where the plan assigns it to another plane; every new flag defaults OFF in production (dev may default on) and is never an authorization input; no permissive default.
- Do NOT write or run tests, and do NOT run lint/format passes over code you did not touch. Instead, write the tests the plan names for your slice into the `tests_owed` list of your unit result (file path + what it asserts); a test phase at the end of the wave writes and runs them.
- Do NOT chase depth. Anything that is not needed for your slice to be correct (edge cases, refactors, performance, UX polish, missing docs, adjacent bugs, doubts about the plan) goes in the depth file, not in the code. Exception: a defect that makes your slice's own behaviour wrong, or breaks a hard rule above, you fix.
- Keep diffs minimal and targeted. Use small `Edit` operations on large files; never rewrite or reformat a large file; re-read a shared file right before editing it because other builders edit other parts of the tree. Seam slices (`P-*`) are pure moves (`git mv` plus module declarations/re-exports, no logic change).

## Numbers (collision control)
- Before you create a migration, proto field/tag, event type or flag, check the highest existing number on THIS branch and the shared ledger `.plan-work/space-build/ledger.md`. Use the number reserved in plan 4.6 if it is free here, otherwise the next free number. Then APPEND one line to the ledger with `>>` (never rewrite it): `kind | number | slice | file`.

## Compile-check (once, at the end; fix errors in YOUR files only)
Use the shared target dirs below. NEVER create another `CARGO_TARGET_DIR`, never `cargo clean`, never build tests (`--tests`/`--all-targets` fail on known pre-existing targets), always `CARGO_INCREMENTAL=0`.
- Model Plane Rust (mp-contracts, session-core, model-gateway, execution-core): `cd "apps/Model Plane/rust" && CARGO_TARGET_DIR=C:/dev/space-target-mp CARGO_INCREMENTAL=0 cargo check -p <crate> --lib --bins` (inference-core's bin does not compile on Windows: ignore). Rust proto bindings regenerate at build time from `apps/Model Plane/proto/model_plane/v1/*.proto` (mp-contracts/build.rs). Checked-in Go bindings under `apps/Model Plane/go/gen` need `buf generate`: recipe in the file `C:/Users/ImaFernandesDaCosta/.claude/projects/C--dev-CoresSystem/memory/docker-wedge-and-native-buf.md` (read-only); only regenerate if your slice changes a proto that a Go service consumes.
- Frontend gateway (Rust): `cd "apps/Frontend Plane/verevonv3/apps/gateway" && CARGO_TARGET_DIR=C:/dev/space-target-gw CARGO_INCREMENTAL=0 cargo check` (a binary crate: no `--lib`).
- Go services (user-core, org-core, audit-core, auth-core is TS): `cd <service dir> && go build ./...` (Model Plane Go services: build per service directory; use `GOWORK=off` only when adding a dependency). Vet only the packages you touched.
- SPA (SolidJS, TS): `cd "apps/Frontend Plane/verevonv3" && pnpm exec tsc -b` (node_modules is installed in this worktree). The repo has pre-existing type errors: judge only errors in files you touched.
- Convex / Node packages (convex-core, auth-core): use that package's own typecheck (`pnpm exec tsc --noEmit` or `npx tsc --noEmit` in the package dir); if dependencies are missing, say so in `notes` instead of installing the world.
- A compile error caused by ANOTHER builder's half-finished edit in a file you do not own: wait about 2 minutes and retry once; if it persists, record it in `notes` and finish (do not fix their file).

## Commit (the lead does not commit for you)
When your compile-check is green (or only fails outside your files): `git add -- <your files>` then `git commit -m "<message>" -- <your files>` where the message is `feat(space): <slice id> <short what>` (or `chore(space)`/`refactor(space)` for seams) plus a blank line and the trailer line `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Path-limited commits only: never `git add -A`/`.`, never `--no-verify`, never stash/reset/checkout/clean/rebase/merge/push. The pre-commit hook runs gitleaks through Docker: if it reports a "secret", change the literal in YOUR file (fake tokens: repeated letters like `ssss...`) and recommit; if it says the Docker engine is unavailable, wait 60 s and retry up to 5 times. If `.git/index.lock` exists, wait 8 seconds and retry up to 15 times. If the commit cannot be made, leave the files in place and say so in `notes`.

## Secrets
Never print, copy, log or write any API key, token, password or env value. Never read `.env*` files. Use fake values in code and docs.

## Result files (always write both)
1. `.plan-work/space-build/units/<slice>.json`: `{"slice":"...","status":"built|partial|blocked|already-present","commit":"<short sha or empty>","files":["repo-relative paths changed"],"compile":"ok|errors: <one line>|not-checked: <why>","migrations":["kind number file"],"flags":["name default"],"tests_owed":["path: what it asserts"],"notes":"<=5 lines"}`.
2. `.plan-work/space-build/depth/<slice>.md`: at most 12 bullets, each `- P0|P1|P2|P3 kind: one sentence` (kinds: risk, gap, test-gap, deviation, decision, follow-up). P0 = a hole that already exists in committed code or makes the slice unsafe; P1 = likely bug or missing authority/ZDR coverage; P2 = hardening; P3 = polish. Include every `DEVIATION:` and every UNVERIFIED assumption.
Return the unit result object as your final message (the same fields). Status guide: `built` = slice scope implemented, compile ok or only foreign errors; `partial` = part of the scope missing (say which in notes); `blocked` = cannot proceed (missing dependency or an owner decision; say which).

## Cost discipline
Read only what you need, never re-read a file you already read, do not explore unrelated areas, stop as soon as the slice is built, compiled and committed.
