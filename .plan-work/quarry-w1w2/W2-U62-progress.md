# W2-U62 progress (WS9.I, control strip, tab provenance markers, Reveal)

Unit: W2-U62, gated G1-c. Owner: Frontend (Verevon v3 SPA + gateway). Branch quarryv2-next.
Status: BUILT to scope. Tests written and compile-checked only (not run, per the build strategy).

## Committed
- 927956683 feat(gateway): compare-and-set browser control epoch (WS9.I)
  browser.rs (ControlBody expected_epoch, control_quarry_body, set_control_mode forwards it, owner projection control.epoch, CONFLICT with control_epoch details -> browser_control_epoch_stale, two tests) and actions/dispatchers.rs (expectedEpoch forwarded, non-integer 400).
- d15c9cd80 feat(verevon-v3): browser control strip, stale-epoch refetch, tab provenance (WS9.I)
  browser-client.ts (control.epoch, openedBy, setBrowserControlMode options, getBrowserSession), errors.ts row, action-registry H13 expectedEpoch, browser-session.ts (controlEpoch, openedBy pass-through, observation keeps control), BrowserChrome.tsx (wiring hunks: strip, agent marker, Reveal), KnowledgeComposer.tsx, KnowledgeScrapePreview.tsx (types), use-browser-session-controller.ts, features/browser/dock/ (browser-control.ts, BrowserControlStrip.tsx, browser-control-strip.css, two test files), browser-session-control.test.ts, tests/e2e/browser-workspace-live-takeover.spec.ts (one test added).
- 073e7a100 docs(quarry-v2): browser control strip and stale-epoch take over runbook (WS9.I) (hunk-only; the RUNBOOKS working copy carries another unit's uncommitted edits, left alone).

## Verified (ran)
- cargo check --tests --manifest-path apps/gateway/Cargo.toml (native): rc=0; one pre-existing unused-import warning (browser.rs test module, ResponseTemplate).
- node_modules/.bin/tsc6 -p tsconfig.app.json --noEmit: 34 errors in the whole app, all pre-existing (RemoteSupportPage.tsx, quarry-client.ts @quarry/client). One error was in my new test (browser-session-control.test.ts, possibly-undefined index), fixed before the commit; no errors remain in any file this unit touches.

## Not run (owed to the test phase)
- vitest: browser-control.test.ts, BrowserControlStrip.test.tsx, browser-session-control.test.ts.
- cargo test: control_body_forwards_expected_epoch, stale_epoch_maps_to_typed_conflict.
- Playwright: take over and give back move the control epoch, and a stale epoch is refused (needs the stack with WS2.C deployed).

## Not done (see depth/W2-U62.md)
- Stack evidence (WS2.C deployed).
- Agent marker and Reveal are inert until WS8.F reports openedBy.
- annotate capability and the fake comment anchor removal (ride-along, deferred).
- Agent-step USER_TOOK_OVER as a state, not a toast (not changed).
- The chip still uses the WS control path (no epoch).

## Path corrections
- features/browser/session/browser-session.ts in the plan = src/features/dashboard/home/browser-session.ts.
- BrowserChrome.tsx is 2320 lines.
