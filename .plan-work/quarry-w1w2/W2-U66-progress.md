# W2-U66 progress (WS8.D: gateway and SPA wiring for human browser sessions), 2026-10-24

Status: BUILT to the plan's scope, dark. Nothing deployed. Tests written; compile-checked or (one file) run as noted below. Not run: the Rust tests and the browser-client test (test phase).

## Committed (branch quarryv2-next)
- cebeb55c5 feat(gateway): human_browse session grants and session keep-alive, dark (WS8.D)
  `config.rs` (`BROWSER_HUMAN_BROWSE`, `human_browse_enabled`, `AppState.browser_human_browse`), `browser.rs` (`SessionGrantScope`, `session_grant_request_body`, `session_start_constraints`, `issue_session_grant` takes a scope, `create_session` human path, `keepalive_session` route and `keepalive_outcome`, `browser_session_capabilities`, tests `human_browse_session_tests`), AppState test literals in `main.rs`, `domains/chat/shared.rs`, `domains/orchestration.rs`, `onboarding/crawl_preview/stream_e2e.rs`, `config.rs` test `human_browse_flag_tests`.
- 04b000648 feat(gateway): browser_session.keepalive action contract and dispatch (WS8.D) (hunk-only: `action-registry.ts`, `dispatchers.rs`, `handlers.rs`)
- 3eb2cb70c feat(verevon-v3): human session keep-alive timer, client call and copy (WS8.D)
  `session-keepalive.ts` (new), `session-keepalive.test.ts` (new), `browser-client.ts` (`keepAliveBrowserSession`), `browser-client.keepalive.test.ts` (new), `BrowserChrome.tsx` (optional `onKeepAlive` prop and a memoised effect; wiring hunks only), `errors.ts` (`browser_session_expired`).
- b5553efe9 docs(quarry-v2): runbook section for human browser sessions and keep-alive (WS8.D) (hunk-only: `docs/RUNBOOKS.md`)
- This file and `depth/W2-U66.md` are committed in the next commit (plan-work).

## Verified (ran)
- `cargo check --tests` for `apps/Frontend Plane/verevonv3/apps/gateway` (native), after every gateway edit: rc=0. One warning, pre-existing (unused `ResponseTemplate` in the browser.rs test module).
- `node_modules/.bin/tsc6 -p tsconfig.app.json --noEmit`: no error in any file this unit touches. The project has pre-existing errors elsewhere (count 38 at the last run, includes other units' files).
- `vitest run src/features/browser/session/session-keepalive.test.ts`: 7 passed, 0 failed (the one test file that the build strategy allows to run).
- `git apply --check --cached` on each hunk-only patch: passed. Each hunk-only commit left the other units' hunks in the working tree (WS14.S6 hunks in `dispatchers.rs` and `action-registry.ts` were later committed by 614df6ad7, not by this unit).

## Written, not run (test phase)
- Rust, gateway: `human_browse_session_tests` (`create_session_requests_human_browse_grant_and_omits_budget_literals`, `create_session_off_keeps_legacy_first_host_policy`, `keepalive_requires_owner_and_returns_expiry_only`, `keepalive_maps_not_renewable_to_409`, `projection_never_exposes_grant_id`, `human_capabilities_are_not_advertised_without_quarry_saying_so`); `human_browse_flag_tests::human_browse_flag_defaults_off_in_every_environment`. Command: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" human_browse`.
- SPA: `src/shared/api/browser-client.keepalive.test.ts` (2 cases). Command: `pnpm vitest run src/shared/api/browser-client.keepalive.test.ts`.
- Playwright leg D: not written (needs a live stack and WS1's `browser-grants-prod-contract.spec.ts`).

## Not done (why)
- Quarry projection field `grantKind` (and `tabControl` / `attachAvailable` values): owned by Quarry projection work (WS8.F for projection.rs). Until it lands, `human_browsing` is never advertised and the keep-alive timer is dormant. This is the main gap for the next batch.
- Parent wiring of `BrowserChrome`'s `onKeepAlive` prop, and the reopen affordance after `browser_session_expired`: WS9 dock owner (H12).
- `BrowserChrome.keepalive.test.tsx` as the plan names it (the timer is tested in `session-keepalive.test.ts`).
- HTTP-level gateway tests (need doubles).

## What the next batch needs to know
- Migrations: none (WS8.D owns none). Proto: none. Openapi: none.
- Flags: `BROWSER_HUMAN_BROWSE` is off in every environment; unset means off. It only takes effect when `BROWSER_GRANT_ENFORCEMENT` is not `off`.
- Order: WS8.C (U61) is merged, so the broker `human_browse` kind and `POST .../renew` exist in model-gateway. The Quarry human-session admission (WS8.B, U39) is merged; the gateway's human path relies on Quarry zeroing the bounds of a `human_browse` run.
- Keep-alive interval is 300 s (`SESSION_KEEPALIVE_INTERVAL_MS`), set against the broker's 15-minute default TTL. The plan's evidence leg uses 20 s with a 45 s TTL. Owner to confirm.
- Decision taken (plan text ambiguous): with no grant, a human-flag session opens on the first-host policy. See depth bullet on `allowed_domains: []`.
- Rebuild note: the gateway is not rustfmt-clean at HEAD; new hunks were not checked in isolation.
- Working tree at the end: other units' uncommitted hunks remain in Quarry files and gateway `profiles.rs`; none of them is ours.
