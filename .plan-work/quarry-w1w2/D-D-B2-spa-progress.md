# D-D-B2-spa progress (SPA: 16 vitest, 5 new tsc errors, D2-030 SPA half)

Unit: D-D-B2-spa (W2-green, not sensitive). Branch quarryv2-next, local commits only. Spec: DEPTH-PLAN-2026-10-27.md, section D-B2-spa.
Baseline at start (HEAD 262faf687): vitest on the 8 named files plus src/shared/i18n and src/shared/actions: 17 files, 16 failed, 261 passed. tsc -b: 38 errors (33 baseline + 4 BrowserChrome + 1 crawl-site-scope). Pre-existing failure outside the set: BrowserBlockedNotice "Ta over" (see depth/D-D-B2-spa.md SPA-B2-01).

## Commits (in order)

| Commit | Item | Files |
|---|---|---|
| eb8ff61a6 | copy for invalid_scrape_artifact, unsupported_schedule_kind, schedule_action_unavailable (nb, en) | src/shared/i18n/errors.ts |
| ab2721829 | the gateway code scans skip browser.rs's pub(crate) test module; 15 capability names and global_run_cap are not codes | src/shared/i18n/browser-errors.test.ts, gateway-error-codes.test.ts |
| cea80368a | keep-alive createEffect in the Solid 2 (compute, apply) form; Network rows moved to liveNetworkRows() | src/features/dashboard/home/BrowserChrome.tsx |
| b85e61ec2 | BrowserNavigationFailure.message renamed sentence (error-catch-sites ratchet unchanged) | browser-navigation-error.ts (+ .test.ts), BrowserNavigationError.tsx, BrowserChrome.reload.test.tsx |
| 1720bf7f2 | omnibox revision signal createSignal(0, { ownedWrite: true }) | src/features/browser/omnibox/omnibox-history.ts |
| c87d96d0d | flush() between two synchronous events in three test files (expectations unchanged) | OmniboxField.test.tsx, ConsoleFilterBar.test.tsx, BrowserApprovalDecisionCard.test.tsx |
| 3f3446fe7 | crawl-site-scope.test.ts: capture group guard (TS18048) | src/shared/actions/crawl-site-scope.test.ts |
| 03f52fae9 | D2-030 SPA half: keepAliveBrowser in KnowledgeComposer, passed through ScrapePreviewPanel and BrowserSessionSurface to BrowserChrome onKeepAlive | KnowledgeComposer.tsx, KnowledgeScrapePreview.tsx |

Root cause per failing test: see the commit messages (each names its cause and its evidence).

## Verification on the tip (after 03f52fae9)

- vitest, the named set (BrowserChrome.reload, OmniboxField, omnibox-history, BrowserApprovalDecisionCard, ConsoleFilterBar, browser-navigation-error, src/shared/i18n, src/shared/actions): 18 files, 282 tests, 0 failed (was 16 failed).
- vitest regression sweep, src/features/dashboard/home, src/features/browser, src/shared/i18n, src/shared/actions, ChatLiveRunPanel.test.tsx (run before the final doc-order comment move, which is comment-only): 78 files, 1008 passed, 1 failed (BrowserBlockedNotice "Ta over", pre-existing, not changed here).
- tsc -b: 33 errors, all baseline (RemoteSupportPage.tsx 12, quarry-client.ts 21); 0 new; the 4 BrowserChrome and the crawl-site-scope error are gone.
- pnpm lint: 0 errors, 200 warnings (limit 201).
- cargo check --workspace --all-targets (qtest): see the final message; this unit touches no Rust.

## Not done in this unit

- SPA-B2-01 "Ta over" expectation in BrowserBlockedNotice.test.tsx (outside the named set; owner of the blocked notice).
- Quarry half of D2-030 (grantKind projection): D-B3-agent-depth. The keep-alive stays dormant until then.
- Keep-alive policy (interval, gate, expiry notice): D2-185, D2-219, owner decisions.
- D2-175 settled-accessor read in the browser-session controller: not touched.
- Full list: depth/D-D-B2-spa.md.

## Uncommitted files of mine

None after the commits above (the unit's SPA paths were clean at the end of the run). Scratch files are in the session scratchpad, not in the repository.
