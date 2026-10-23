# Browser omnibox (WS9.A)

The address bar of the in-app browser. Free text can be searched through Verevon
search; addresses navigate as before. Plan: `docs/plans/quarry-browser-build-plan-2026-10-14.md`,
slice WS9.A (D9 S1).

## What it does

- Enter on an address or bare host (`example.com`, `localhost:5174`) navigates the frame.
- Enter on free text (`norwegian salmon exports`) shows Verevon search results in the
  frame stage. Results are http(s) links only; a click opens through the address bar's
  navigation. No third-party search engine URL is ever built.
- The dropdown offers remembered pages, query suggestions, "Search the web" and, when
  a chat hand-off is wired, "Ask Verevon".
- Keys inside the field: Up/Down move, Enter runs, Escape closes (the event stops
  there, so the chat's `dismiss` does not also fire), Shift+Delete forgets a remembered page.

## Zero data retention

A zero-data-retention session (the session's own flag, the org's retention verdict,
or a ZDR header) keeps no local history, shows no suggestions and makes no
suggestion request. The gateway answers `GET /api/v1/search/suggestions` with an empty
list for a ZDR caller, and the client checks again at every step. The search itself
still runs (it is a Verevon search, with no cache entry for ZDR callers).

## Flag

`VITE_BROWSER_OMNIBOX_SEARCH` (build time). Only `1` turns search on. Unset or `0`
keeps URL navigation and the local history, and free text shows "Søk er ikke slått på her."

- Dev and staging builds: `VITE_BROWSER_OMNIBOX_SEARCH=1`.
- Production: `0` until the gateway search cache carries the org key (WS4.A deployed).
- Rollback: set the flag to `0` and rebuild the frontend container. The ZDR gate only tightens.

## Gateway surface

- `GET /api/v1/search/suggestions` returns `[]` for a ZDR request (`x-zdr` header or org posture,
  failing closed).
- Action ids `browser_omnibox.search` (`{query, limit?}`) and `browser_omnibox.suggest`
  (`{query}`) on `POST /api/v1/actions/execute`. Human-only: neither is in the Model
  Plane's eligibility list.
- A cross-host navigation refused by Quarry (403, "outside the run's allowed domains")
  reaches the SPA as `browser_navigation_outside_scope`. WS8 replaces the message match with
  a typed code.

## Local storage

One key per user and workspace: `verevon.browser-omnibox.v1:{org}:{user}`. At most 200
entries of `{url, title, lastVisitedAt, visits}`. URLs are scrubbed (http(s) only, no
user name or password, no fragment, query dropped when it is longer than 256 characters
or has a credential-looking key). Page text, screenshots and form values are never stored.
Like the other per-account keys, sign-out leaves the record in place (the identity in the key is the
isolation). The zero-data-retention purge (`clearHomeLocalContent`) removes every account's copy.

## Checks

- Unit: `pnpm exec vitest run src/features/browser/omnibox src/features/dashboard/home/browser-omnibox.test.ts`.
- Gateway: `cargo test --manifest-path apps/gateway/Cargo.toml search_suggestions_empty forward_quarry_failure_maps_navigation_scope`.
- E2E (API level): `tests/e2e/browser-workspace-omnibox.spec.ts`.
- Manual: as a signed-in user on a dev build, type `norwegian salmon exports` in the
  address bar and press Enter. The frame shows results. Toggle ZDR on the session and
  reopen the bar: no history rows and no `GET /api/v1/search/suggestions` request.

## Not in this slice

- The "limited to {host}" row for a refused cross-host result click (the generic
  `browser_navigation_outside_scope` copy is shown instead).
- The `KnowledgeComposer` hand-off to chat (`onAskVerevon` is threaded through
  `KnowledgeScrapePreview` but not yet passed by the composer).
- The demo-build guard: no `VITE_DEMO` guard exists in the tree; the resolver takes a `demo` option for it.
