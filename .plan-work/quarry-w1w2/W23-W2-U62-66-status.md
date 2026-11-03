# W23 W2-U62-66 status: gateway passes tabs[].openedBy, edge emits it, human-grant HTTP test (D2-141)

Lane c, branch w23/lane-c, 2026-11-03. **Code only, not compiled, no test run** (owner order: the integration step compile-checks).

## Built: partly

Gap check first. What already existed, and was not rebuilt:
- Edge projection: `agent_routes/projection.rs` `browser_session_tab` already sets `openedBy` from `entry.tab_origins`. The wire name is right (`BrowserSessionTab` is `rename_all = "camelCase"`) and the value is `{ initiatedBy, agent?, unattributed? }`. No edge change was needed.
- Keep-alive wiring (U66) is complete end to end: gateway route `POST /api/v1/browser/sessions/{id}/keepalive` (browser.rs, `classed_routes`); capability `human_browsing` from the projection's `grantKind` (projection.rs `grant_kind`, f92315cd0); SPA composer passes `onKeepAlive` down to BrowserChrome (03f52fae9). The U66 depth row "parent passes no handler" is stale. Nothing was changed for it.
- Human-browse HTTP tests: `tests/human_browse.rs` already pins the service, owner, allow-list-policy and ZDR-grant refusals. The new file covers what it did not.

What was built (the real gap):
- **Gateway, `browser.rs`.** The SPA's contract is the string `openedBy: 'agent' | 'human'` (`browser-client.ts` `BrowserTab`; `browser-session.ts` `normalizeBrowserTabs` keeps only those strings). Quarry emits an object, so the agent marker never rendered. Quarry's tab list (`GET/POST .../tabs`, select, close) also carries no opener, so every tab response would have dropped the marker after a tab action.
  - New helpers `opened_by_marker`, `tab_with_opened_by`, `tabs_with_opened_by`.
  - Applied to the owner projection's session tabs (`browser_response_from_owner_projection`), `get_tabs`, `new_tab` (the opened tab and the list) and `tab_mutation` (select and close). Those handlers already fetch the owner projection, so no extra Quarry call. A list tab takes its marker from the projection tab with the same id. A tab's own `openedBy` object wins if present. A system opener is removed.
  - Tests (not run): `the_tab_list_and_the_session_name_each_tabs_opener_as_the_spa_reads_it` (HTTP, wiremock, in `mod tests`); `mod tab_opened_by_tests` (pure, end of file).
- **Edge, `tests/human_browse_grant.rs` (new, D2-141).** Six `#[serial]` tests over the real router:
  1. switch off refuses a human grant (`human_browse_disabled`) before any browser is acquired;
  2. an admitted run takes no host list, step, time or cost bound (zeroed in the run entry), and its projection says `grantKind: human_browse`;
  3. a denied host and every host under it are refused, case-insensitively;
  4. loopback, private, link-local, metadata and single-label hosts are refused, and a public IP the deny list does not name is reached (positive control);
  5. a `run`-kind grant with `policy_mode: open_web` does not open the web (start keeps the broker's hosts, no `grantKind`, a public step is `SECURITY_BLOCKED`);
  6. a person's own tab on an open-web run names the person as its opener, and the browser's own tab names none.
- **Playwright.** `tests/e2e/browser-workspace-live-takeover.spec.ts` gets one test: "a tab the agent opened is marked in the tab strip and can be revealed". It is written, not run, and LF is kept because the file's eol attribute is `lf`.

## Commits (branch w23/lane-c, local only)
- `834ae14c1` feat(gateway): tab responses name each tab's opener as the SPA reads it (WS8.F), `browser.rs`
- `92a5ced7e` test(quarry-edge): HTTP-level human-grant constraints for a person's open-web run (D2-141), `tests/human_browse_grant.rs`
- `994174b2b` test(verevon-v3): the takeover spec marks an agent-opened tab (WS8.F), `browser-workspace-live-takeover.spec.ts`
- the docs commit with this file and the lane-c depth rows (see the commit log)

## Flag and default
- `BROWSER_HUMAN_BROWSE` (gateway) and `QUARRY_EDGE__HUMAN_BROWSE` (edge) are unchanged. Both are off unless set, and the new edge tests set and unset the edge switch themselves.
- No new flag. The marker corrects a contract mismatch and adds no data the person cannot already read in the owner projection, so it is a bug fix rather than new behaviour. It goes live only where a tab has an opener (an agent's tab call, or a person's tab on any run).
- The keep-alive route is not gated by `BROWSER_HUMAN_BROWSE` (unchanged from U66; see depth).

## Assumed decisions
- The queue names `tests/e2e/browser-workspace-takeover.spec.ts`, which does not exist. The takeover spec is `browser-workspace-live-takeover.spec.ts` (the U62 test already extends it), so that file was extended. No new spec file was created.
- The conversion lives in the gateway, so the SPA's string contract stands and no SPA file is touched. The SPA is not in this unit's file list.
- Tab-list responses take the marker from the owner projection the handler already fetches, not from an edge change. `agent_routes/tabs.rs` is outside this unit's files (see depth).
- The edge test's positive control is a public IP literal, which needs no DNS, with the driver reporting that page as the landed page. A hostname would need DNS in the test environment.
- Every test in the new edge file is `#[serial]`, because the switch is a process-wide environment variable read at each request.
- The keep-alive owner gate has no edge test, because the edge has no keep-alive route. The renew route is Model Gateway's. The gateway's own HTTP-level tests for it are still missing (see depth, D2-141 gateway half).

## MP asks
- None. No Model Plane file was touched. The Model Plane's agent tab calls already send `initiatedBy=agent`, which the gateway forwards and the edge records as the opener.

## Not verified
- Nothing compiled. `cargo check --tests` for the gateway and `cargo test --test human_browse_grant` for quarry-edge were not run (the integration step compile-checks them).
- The new gateway tests (`the_tab_list_and_the_session_name_each_tabs_opener_as_the_spa_reads_it`, `tab_opened_by_tests`) were not run.
- The six edge tests were not run. The positive control depends on the landed-page check and the web-policy visit gate passing a public IP with the driver's landed page set; I read them as permissive but did not execute them.
- The Playwright test was not run. It depends on the stack having this gateway change deployed, and on the "Ny fane" control being enabled in agent control.
- No stack evidence.
