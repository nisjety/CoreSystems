# W2-U39a status (lane c)

Unit: DNS rebinding test and open-web egress checks (D2-059). Plane: Ingestion (quarry-runtime, quarry-edge).

## Built

Built in full, code only, not compiled (no cargo run, per the 2026-11-03 owner order; the integration step compile-checks).

Gap check: `PinnedDnsResolver` and a pin/resolve test already existed in `dns_guard.rs`, and the egress proxy already ran `resolve_public_url` for every request it admitted (including open-web), with no `allow_private_hosts` bypass. Missing were: a rebinding test, a way to test any of it without real DNS, the open-web rule under `allow_private_hosts` at admission, and a proxy test that proves open-web hosts are vetted after resolution.

- `dns_guard.rs`: `HostLookup` trait + `SystemHostLookup` (the only production lookup), `resolve_public_url_via(url, &dyn HostLookup)` (`resolve_public_url` delegates to it unchanged), `dns_guard_required(allow_private_hosts, open_web)` (open-web always guarded). Tests: second resolution to a private / metadata address refused; a mixed public+private answer refused; a refused rebind cannot replace a vetted pin; the guard truth table.
- `browser_egress_proxy.rs`: the proxy owns an `Arc<dyn HostLookup>` (`new()` = system resolver, `with_host_lookup` for tests) and resolves through `resolve_public_url_via`. Tests: open-web policy, a public name resolving to 10.0.0.5 is refused for HTTP and CONNECT with `dns_or_ssrf_blocked` receipts; a mixed public+metadata answer is refused.
- `agent_routes/admission.rs`: `validate_navigation_for_run(action, security, &RunNavPolicy)`; gate 12 of `admit_action` uses it with `entry.nav_policy`, so an open-web run is DNS-guarded even when the engine allows private hosts. `validate_navigation` (run-less) keeps its behaviour. Tests in a `dns_guard_tests` module in the same file.

## Commits (lane c, local, w23/lane-c)

- 929d083f7 fix(quarry-runtime): dns_guard lookup seam, rebinding tests and open-web guard rule
- a88fbf45d fix(quarry-runtime): egress proxy vets open-web hosts through the dns_guard lookup
- 626dcd1e7 fix(quarry-edge): open-web navigation stays DNS-guarded under allow_private_hosts
- status and depth rows: the commit that adds this file

## Flag and default

None (SSRF fix). Production behaviour changes only for an open-web run on an engine with `allow_private_hosts` on (dev/test), which is now refused private targets.

## Assumed decisions

- `allow_private_hosts` stays a relaxation for allow-list runs only (local fixtures in tests rely on it); open-web never inherits it.
- The proxy keeps vetting every mode with no bypass (it already did); the change is the lookup seam plus proof tests, not a new policy.
- Only gate 12 of `admit_action` moved to the run-aware check; the other `validate_navigation` callers are outside this unit's files (depth row).

## MP asks

None.

## Not verified

Not compiled, no tests run (`qtest -p quarry-runtime dns_guard egress` and the quarry-edge admission tests are for the integration step). No live run.
