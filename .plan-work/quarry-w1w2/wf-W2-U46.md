# W2-U46 hand-over (WS9.D, evidence panels)

What was done: the SPA evidence panels (Egress tab, console filter, procedure section) built in `apps/Frontend Plane/verevonv3/src/features/browser/evidence/`, wired into `BrowserChrome.tsx` behind capability `evidence`; gateway capability flag in `browser.rs`; error copy rows; tests; a RUNBOOKS section. Commits: c36a6c071, 7bfbc92f0, 74149951e, 2cb78a671.

What is left: run the component tests and the Playwright spec in the test phase; deployment evidence on a stack (the order quarry-edge, gateway, frontend); the procedure `expectedOutcome` check against real Quarry output. Full list in depth/W2-U46.md.

Traps found: see the Traps section of W2-U46-progress.md (attribution line, org id source, tsc filtering by path, RUNBOOKS working-copy diff).
