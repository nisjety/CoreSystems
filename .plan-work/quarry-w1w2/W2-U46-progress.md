# W2-U46 progress (WS9.D, evidence panels in BrowserChrome)

Status: built to scope and committed. Build-first wave: tests written and compile-checked; only use-browser-evidence.test.ts was run (4 of 4 passed). The other three component test files and the Playwright spec were compile-checked, not run.

Dependencies on disk (all committed): WS9.A (a3c2b6f3c), WS9.B (b57e3edb6), WS9.C (U19: 35688ef98, 95a2d00b2, 8db594349).

## Committed (branch quarryv2-next, local only)

- c36a6c071 feat(verevon-v3): the evidence module (`features/browser/evidence/`: use-browser-evidence.ts, receipts-view.ts, ConsoleFilterBar.tsx, EgressReceiptsPanel.tsx, ProcedurePanel.tsx, browser-evidence.css), BrowserChrome.tsx wiring hunks (Egress tab, console filter bar, egress arm, procedure section in the evidence drawer; all behind `hasEvidence()`), and shared/i18n/errors.ts rows (browser_procedure_not_compilable, browser_receipts_unavailable_zdr, invalid_browser_evidence_query, invalid_devtools_query).
- 7bfbc92f0 test(verevon-v3): ConsoleFilterBar.test.tsx, EgressReceiptsPanel.test.tsx, ProcedurePanel.test.tsx, use-browser-evidence.test.ts, and tests/e2e/browser-workspace-evidence.spec.ts.
- 74149951e feat(gateway): capability "evidence" added to the two static session capability lists in browser.rs (the flag).
- 2cb78a671 docs(quarry-v2): WS9.D runbook section appended to docs/RUNBOOKS.md (commit built from the HEAD blob; the working copy still carries another unit's uncommitted hunks).

## Compile and test evidence

- `pnpm exec tsc --noEmit -p tsconfig.app.json` (apps/Frontend Plane/verevonv3): zero errors in features/browser/evidence, BrowserChrome.tsx and errors.ts. 33 pre-existing errors remain in other files (RemoteSupportPage.tsx, quarry-client.ts, session/use-browser-session-controller.ts).
- `pnpm exec eslint src/features/browser/evidence src/features/dashboard/home/BrowserChrome.tsx tests/e2e/browser-workspace-evidence.spec.ts`: exit 0 after one fix (untrack on the initial draft). Two warnings in BrowserChrome.tsx pre-exist (line 541, selectedTimelineStep), not from this change.
- `pnpm exec tsc --ignoreConfig --noEmit --strict ... tests/e2e/browser-workspace-evidence.spec.ts`: exit 0.
- `pnpm exec vitest run src/features/browser/evidence/use-browser-evidence.test.ts`: 4 passed (4), rc 0.
- `cargo check --tests` in apps/Frontend Plane/verevonv3/apps/gateway (native): rc 0, finished in 51 s, one pre-existing warning in the gateway bin test target.

## Not done (see depth/W2-U46.md)

- The three component tests and the Playwright spec have not been run (test phase).
- Deployment evidence (Egress allow and block rows, Compile procedure quality, console DOM count) not run.
- The `evidence` capability must not reach production before quarry-edge (WS9.C) and the gateway are deployed in that order.
- Nothing from WS9.J (observed live view) or a server-side console filter.

## Traps for the next unit

- The attribution: the harness reminder asked for `Co-Authored-By: Claude Haiku 5.5`; the workflow text asked for Sonnet 5.5. The Haiku line was used on all four commits (the system-level instruction), as in U19.
- `BrowserChrome.tsx` has no CRLF (LF file). Its wiring hunks are the only lines this unit changed in it.
- The org id for the evidence routes comes from `getSessionContext().orgs[0]` (as KnowledgeComposer does); the hooks do not cache it.
- `tsc -p tsconfig.app.json` checks all of src, so one unit's broken file (a stray zz-debug.test.ts appeared during the build) shows up in every unit's compile check; filter the output by path.
- RUNBOOKS.md in the working copy carries another unit's 2,854-line diff. Use qcommit-blobs.sh (HEAD blob plus this unit's section) for any later append.
