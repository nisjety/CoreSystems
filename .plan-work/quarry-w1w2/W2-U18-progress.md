# W2-U18 progress (WS9.A, browser omnibox search, dark)

Status: BUILT to scope, compile-checked (gateway cargo check --tests, SPA tsc), NOT run. The flag is off by default; nothing is deployed. No Quarry, migration, proto or OpenAPI change in this slice.

## Committed

- a3c2b6f3c feat(browser): omnibox search for free text behind a dark flag (WS9.A)
  - Gateway (apps/Frontend Plane/verevonv3/apps/gateway): domains/actions/browser_omnibox.rs (new, two dispatchers); domains/actions.rs (mod line); domains/actions/handlers.rs (import, two arms before browser_profile.create); domains/actions/dispatchers.rs (owner_json_response_to_envelope pub(super)); domains/search.rs (search_web and search_suggestions pub(crate); suggestions return empty under request_zdr or org_zdr_enabled, checked first; 2 tests); domains/browser.rs (forward_quarry_failure maps the navigation-scope 403 to browser_navigation_outside_scope; 1 test).
  - SPA: features/browser/omnibox/ (new): omnibox-resolver.ts, omnibox-history.ts, omnibox-suggestions.ts, OmniboxField.tsx, BrowserSearchResults.tsx, browser-omnibox.css, and five test files. features/dashboard/home/browser-omnibox.ts (classification; never a search URL) and its test; BrowserChrome.tsx (wiring hunks only); KnowledgeScrapePreview.tsx (onAskVerevon pass-through, 4 lines); shared/session/home-local-content.ts (key and purge); shared/i18n/errors.ts (browser_navigation_outside_scope); shared/actions/action-registry.ts (browser_omnibox.search, browser_omnibox.suggest, human-only); vite-env.d.ts and .env.example (VITE_BROWSER_OMNIBOX_SEARCH, default 0 in the example, unset means off in code); tests/e2e/browser-workspace-omnibox.spec.ts (API-level); docs/BROWSER_OMNIBOX.md (new runbook section).

## Verified (what was actually run)

- `cargo check --tests` in apps/Frontend Plane/verevonv3/apps/gateway (native, no docker): rc 0. One pre-existing warning, unused import `ResponseTemplate` at src/domains/browser.rs:5780 (test module, not U18's hunk).
- `pnpm exec tsc --noEmit -p tsconfig.app.json` in apps/Frontend Plane/verevonv3, run twice (before and after the test fixes): 33 errors, all pre-existing (src/features/support/components/RemoteSupportPage.tsx, src/shared/api/quarry-client.ts). Zero errors in any file U18 touches or creates.
- `pnpm exec vitest run src/features/browser/omnibox/omnibox-resolver.test.ts`: NOT completed. The forks worker did not start: "Timeout waiting for worker to respond" after 60 s (shared machine). No test ran.

## Not run (build-first rule)

- All vitest files listed above (resolver, history, suggestions, OmniboxField, BrowserSearchResults, browser-omnibox).
- Gateway tests search_suggestions_empty_under_zdr_header, search_suggestions_empty_under_org_zdr, forward_quarry_failure_maps_navigation_scope.
- Playwright tests/e2e/browser-workspace-omnibox.spec.ts (three API-level tests).
- Deployment evidence (plan WS9.A): rebuild gateway and frontend, flag 1, the live search, the ZDR toggle, the cross-host click, curl with x-zdr.

## Deviations from the plan text (recorded)

- Gateway action module is src/domains/actions/browser_omnibox.rs, with its mod line in src/domains/actions.rs (the plan says domains/actions/actions.rs; that file does not exist).
- `createOmniboxSuggestions` takes `historyKey`, `zdr`, `searchEnabled` and `askEnabled` as options and returns `update(input)` and `dispose()`. The plan wrote `{input, zdr}`; the input is passed to `update`, so the component owns the reactive link.
- The docs file is docs/BROWSER_OMNIBOX.md (new). docs/BROWSER_WORKSPACE_PLAN.md is not in the tree.
- No Quarry RUNBOOKS.md section: this slice has no Quarry change (the RUNBOOKS rule is for Quarry units).
- The plan's VITE_DEMO guard has no existing flag in the tree; the resolver has a `demo` option that no caller sets.

## Left for the next batch (what they need to know)

- BrowserChrome.tsx (H12): the address field is now OmniboxField; navigation from the address bar and from search results goes through `submitOmnibox(intent)`. WS9.B (U27) should use that function for its retry and reload paths, and must not add a second navigate path. WS9.F/G/H hunks go next to the same block.
- action-registry.ts (H13): two ids added after browser_run.control's neighbour (before browser_session.create). WS9.C (U19) adds its own ids elsewhere in the registry; the dispatch arms for the two omnibox ids sit in handlers.rs before browser_profile.create.
- gateway search.rs: search_suggestions now takes a HeaderMap as its second argument. Any new caller must pass the headers.
- The flag is read at build time (`import.meta.env.VITE_BROWSER_OMNIBOX_SEARCH === '1'`). Dev and staging builds must set it to 1; docker-compose.yml does not pass it yet (depth, medium).
- Depth bullets are in .plan-work/quarry-w1w2/depth/W2-U18.md (13 items; the high ones are the test-run gap and the host-specific refusal row).
