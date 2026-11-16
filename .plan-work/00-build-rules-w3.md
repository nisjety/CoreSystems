# Build rules for every Space W3 builder (read fully before editing)

You build ONE slice of the approved plan `docs/plans/space-hybrid-build-plan-2026-10-14.md`, wave W3 (section 3.E: WS17 personal agents and audience-safe memory, WS18 agent self-API, WS19 room canvas, WS20 room-scoped agent browser). Waves W1 and W2 are built and merged on main. WS17-S1 and WS17-S2 are already built (commits fad74082f, ec2ab2c65).

Owner's strategy (binding): **code completion only. Do NOT write tests, do NOT run tests, do NOT run builds, compile checks, linters or formatters.** Build the slice to scope; log depth findings; nothing may stall progress. Other builders run at the same time in other lock groups on other files.

Owner decisions for W3 (plan 2.6 recommendations accepted): SD-03 ADR 3 option A, SD-04 ADR 4 option A, SD-22 (personal agents in shared rooms only per-org opt-in `personal_agent_admit_entitled`, org-scope recall excluded, private panel conversation-scope in v1, declassifying owner retracts, legacy agents stay org-visible), SD-23 (room members do not see another member's browser frames in v1; any editor may start once `browser_entitled`; limits 10 min / 2 runs per Space / 8 hosts; dev flag), SD-08/SD-19 (canvas pilot-only until counsel rules: flags off in production).

## Where you work
- Worktree `C:/dev/space-w3`, branch `space-w3`. Your FIRST command is `cd "C:/dev/space-w3"` and `git branch --show-current` must print `space-w3`. Every relative path here is relative to it. Use absolute paths (`C:/dev/space-w3/...`) with Read/Edit/Write/Grep/Glob. ALL edits happen inside it. NEVER edit `C:/dev/CoresSystem` (the live main checkout) or any other worktree, any `.env*` file.
- Plan line numbers drift: locate code by SYMBOL with Grep, not by line number. If the plan's premise is false in this tree (symbol missing or renamed, or already built by an earlier wave or the Model Plane build), adapt minimally and record a `DEVIATION:` bullet; if the slice is already fully present, set status `already-present` and finish.
- The plan overrides the design file `.plan-work/design/WSnn.md` where they differ (section 2 decisions, section 4 numbers, section 5 cross-session owners win).

## What to read (and only that)
1. Your slice in the plan: Grep the slice id in the plan; read the workstream subsection (`#### WSnn ...` up to the next `####`/`###`): slice table row, file-level change list for your slice, API/proto/data-model/migration paragraphs, flags, DoD. Also read the "Conventions for this part" paragraph and number table at the start of section 3.E (Grep `### 3.E`).
2. Plan 4.2 (your row), 4.4.1 (hotspot rules for files you touch), 4.6 (migration, proto, flag reservations).
3. `.plan-work/design/WSnn.md` for detail, and the unit notes of your W3 dependencies: `.plan-work/space-build/units/<dep>.json`.
4. The code you will change, only as deeply as needed.

## What to build
- Implement the slice completely per its row and file list: routes, RPCs, proto fields, migrations (up, and down where the directory has a `revert/` convention), Convex functions, SPA components, flags, action-registry entries (an action contract in `src/shared/actions` BEFORE any UI operation), typed error envelopes `{error:{code,message,details}}`, i18n keys.
- Hard rules (a violation makes the slice wrong): Control owns authority/identity/audit/entitlements and every durable Space write carries a verified Control decision; ZDR fails closed with the typed refusal catalogue (`apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs`) on every content-persisting route, event, cache, index and notification; Space access never widens an owner-plane ACL; Model proposes and Quarry executes browser actions; no cross-plane database access; no durable Space state in the gateway; every new flag defaults OFF in production (dev may default on) and is never an authorization input; no permissive default.
- No tests: put the tests the plan names for your slice into `tests_owed` (file path + what it asserts). Do not edit existing test files except where your change would make an existing test fail to COMPILE (e.g. a new required struct field in a literal): then make the minimal fix and say so.
- No depth: anything not needed for your slice to be correct goes in the depth file. Exception: a defect that makes your slice's own behaviour wrong, or breaks a hard rule, you fix.
- Keep diffs minimal. Small `Edit` operations on large files; never rewrite or reformat a large file; re-read a shared file right before editing it because other builders edit other parts of the tree. Before adding a route, RPC, table, flag or component, Grep for it first (extend, do not duplicate).
- Since nothing is compiled, be careful: match existing signatures exactly, add `mod`/`pub use`/imports/registrations, update every `match` over an enum you extend, every struct literal that gains a field (or use `..Default::default()` where the type derives Default), every proto consumer. Read the callee before calling it.

## Numbers (collision control)
- Before you create a migration, proto field/tag, event type or flag, check the highest existing number on THIS branch and the shared ledger `.plan-work/space-build/ledger.md`. Use the number reserved in plan 3.E/4.6 if it is free here, otherwise the next free number. Then APPEND one line to the ledger with `>>` (never rewrite it): `kind | number | slice | file`.

## Commit (you commit your own slice)
`git add -- <your files>` then `git commit -m "<message>" -- <your files>` with message `feat(space): <slice id> <short what>`, a blank line, and the trailer `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>`. Path-limited commits only: never `git add -A`/`.`, never `--no-verify`, never stash/reset/checkout/clean/rebase/merge/push/amend. The pre-commit hook runs gitleaks: if it reports a "secret", change the literal in YOUR file (fake tokens: repeated letters) and recommit. If `.git/index.lock` exists (a sibling is committing), wait 8 seconds and retry up to 15 times. Do not commit the ledger, units or depth files (the lead does). If the commit cannot be made, leave the files and say so in `notes`.

## Secrets
Never print, copy, log or write any API key, token, password or env value. Never read `.env*` files.

## Result files (always write both)
1. `.plan-work/space-build/units/<slice>.json`: `{"slice":"...","status":"built|partial|blocked|already-present","commit":"<short sha or empty>","files":["repo-relative paths changed"],"compile":"not-checked: code-only wave","migrations":["kind number file"],"flags":["name default"],"tests_owed":["path: what it asserts"],"notes":"<=5 lines"}`.
2. `.plan-work/space-build/depth/<slice>.md`: at most 12 bullets, each `- P0|P1|P2|P3 kind: one sentence` (kinds: bug, risk, gap, test-gap, deviation, decision, follow-up). P0 = a hole in committed code or makes the slice unsafe; P1 = likely bug or missing authority/ZDR coverage; P2 = hardening; P3 = polish. Include every `DEVIATION:`, every UNVERIFIED assumption (e.g. a signature you could not confirm), and every pre-existing bug you noticed in code you read.

## Final message (your report to the lead)
Return exactly these sections, short:
- `STATUS:` built | partial | blocked | already-present, and the commit sha.
- `DONE:` what you implemented (3-8 bullets, file paths).
- `NOT DONE:` scope left out and why.
- `BUGS/ISSUES:` every bug, risk, compile uncertainty or plan contradiction you found (the depth bullets, P0/P1 first).

## Cost discipline
Read only what you need, never re-read a file you already read unless a sibling may have changed it, do not explore unrelated areas, stop as soon as the slice is built and committed.
