# W2-U18 status: omnibox limitedToHost and the BrowserChrome Solid 2 effect fix (D2-046, D2-004/005)

Branch w23/lane-a. Date 2026-11-02. Plane: Frontend (verevonv3).

## Built
- D2-046 SPA side, built: a refusal outside the run's allowed domains now keeps the search results open and explains itself next to them. `browser-navigation-error.ts` gets a typed reason `outside_scope`, chosen from the gateway's typed code `browser_navigation_outside_scope` (never from message text). BrowserChrome.tsx holds a result click as pending (`pendingResult`): a failure for that URL with reason `outside_scope` keeps the results open, any other failure or a changed session URL closes them (the old behaviour closed them at once, so the notice could never be seen). BrowserChrome passes `limitedToHost` (the session host) and a new `refusedHost` (host of the refused result) to BrowserSearchResults.
- BrowserSearchResults: new optional `refusedHost` prop; the notice (`role=alert`, `data-reason="outside_scope"`) reads "Denne økten er begrenset til <host> og kan ikke åpne <refused host>. Åpne resultatet i en ny økt, eller ta over nettleseren selv." (Norwegian with the real letters in the source, as the rest of the component). Without a refused host the "og kan ikke åpne" part is left out.
- A result click while controls are disabled now does nothing (it used to call runAction, which returned early, but also closed the results).

## Already existed (no gap)
- D2-004 (BrowserChrome.tsx createEffect one-argument MISSING_EFFECT_FN at :534): already fixed in the tree: the keep-alive effect uses the (compute, apply) form. BrowserChrome.reload.test.tsx passes 7/7 in this lane.
- D2-005 (BrowserChrome.tsx 661/669 TS2349/TS7006): not present; tsc shows no BrowserChrome.tsx error.
- The `limitedToHost` prop and its generic notice already existed in BrowserSearchResults; it was never passed. Built only the gap (host naming, refused host, the wiring, the typed reason).

## Commits
See `git log w23/lane-a` (browser-navigation-error typed reason + test, BrowserSearchResults + test, BrowserChrome wiring + new BrowserChrome.omnibox-scope.test.tsx, this status/depth).

## Tests (host, vitest 4.1.11; node_modules is a junction to the vigorous-perlman checkout, untracked)
- `vitest run src/features/browser src/features/dashboard/home/BrowserChrome`: 20 files, 159 tests passed (before the unit: 19 files, 153 tests; the scoped command `omnibox` + `BrowserChrome.reload` alone: 6 files, 52 tests before, all pass after).
- New tests: BrowserSearchResults.test.tsx +2 (notice names limited and refused host; no notice without a limit, no refused part without a host); browser-navigation-error.test.ts +1 (typed code gives outside_scope, a reworded message with another code stays `failed`); BrowserChrome.omnibox-scope.test.tsx +3 (refusal keeps results open and shows both hosts; results close when the navigation goes through; results close on another failure). This extra test file and browser-navigation-error.ts(+test) are outside the queue's file list: they are needed for the typed reason and cover the chrome wiring; no other unit of the batch touches them.
- `tsc6 -p tsconfig.app.json --noEmit`: 34 errors before and after my change (RemoteSupportPage.tsx 12, quarry-client.ts 21, schedule-contract.test.ts 1). The 34th is not mine (schedule-contract.test.ts, another unit); my change adds none. Against the 33 baseline the lane is already at 34 for that reason.

## Flag and default
- No new flag. Omnibox search stays behind the existing build flag `VITE_BROWSER_OMNIBOX_SEARCH` (default off; only `1` turns it on) and the `searchEnabled` prop. X-18 default holds: omnibox in production stays off, dev-only. The new behaviour is only reachable from the search results, which exist only when that flag is on.

## Assumed decisions
- The host named as "limited to" is the session host (`session().host`, else the host of the session URL). The gateway sends no allowed-domain list on the refusal, so the notice cannot list several domains.
- The typed reason is mapped only from the gateway's existing code `browser_navigation_outside_scope`; no new gateway code was invented.
- A result click that fails for any other reason closes the results and leaves the existing error card to speak (unchanged behaviour).

## Model Plane asks
None. (Gateway ask recorded as a depth row, below: the gateway still derives the typed code from Quarry's message text.)

## Not verified
- Not run in a real browser or against a live gateway/Quarry; the refusal is simulated by setting `browserNavigationError` in jsdom.
- Whether the live use-browser-session-controller passes the gateway code through `ApiError.code` for a 403 outside-scope (read from code: `navigationFailureFromError` receives the same ApiError the form error uses, whose `code` comes from the error envelope; not exercised end to end).
- Gateway cargo tests not run (not in scope).
