# W2-U23 progress (batch 4): WS13.1 isolated scrape sessions and the heavy lane

Status: BUILT to the plan's scope (code, flags, tests written and compile-checked, RUNBOOKS section). Tests NOT run (wave test phase). Evidence owed (listed below).

## Commits (quarryv2-next, local, not pushed)

| Commit | Content |
|---|---|
| 752a0e7ff | `feat(quarry-runtime)`: isolated per-fetch session keys, no seeding under the host key, guard never returns an isolated lease; heavy Semaphore; unit tests in `browser_driver.rs`; live test `tests/browser_session_isolation_live.rs` (NEW) |
| 5e91d10f5 | `feat(quarry-edge)` compose: `QUARRY_SCRAPE_SESSION_ISOLATION` (default `on`) and `QUARRY_SCRAPE_HEAVY_PERMITS` (default `1`) in the quarry-edge env blocks of both compose files |
| 4e5527cda | `feat(quarry-edge)`: the local chromiumoxide registration log line carries `session_isolation` and `heavy_permits` (hunk-only commit; `main.rs` also carries other units' uncommitted hunks, left in the tree) |
| a07c96b67 | `docs(quarry)`: RUNBOOKS WS13.1 section (hunk-only; RUNBOOKS also carries other units' uncommitted hunks, left in the tree) |
| (this commit) | `docs(plan-work)`: this file, `wf-W2-U23-progress.md`, `wf-W2-U23.md`, `depth/W2-U23.md` |

## Verified (what was actually run)

- `cargo check -p quarry-runtime --tests` (qtest, quarry-rust-env:2): Finished, rc 0, no warnings from the new code. First attempt failed on a `tokio::join!` temporary borrow in the concurrent test; fixed.
- `cargo check -p quarry-runtime --features chromiumoxide --test browser_session_isolation_live` (qtest): Finished, rc 0. First attempt failed on the same borrow pattern in the live test; fixed.
- `cargo check -p quarry-edge` (default features, `browser-agent` on, qtest): Finished, rc 0. Warnings printed are pre-existing (`fleet_routes`, `run_live`, `agent_routes/types`, `handoff`, `profile_guard` `Attach`, `zdr_log`, `zdr_tombstone`), none in the lines this unit touched.

## Not run (by the build strategy)

- Unit tests `cargo test -p quarry-runtime browser_driver` (the names are in the plan; all written).
- Live test (needs `CHROMIUMOXIDE_TEST=1`, Chromium and outbound HTTPS to httpbin.org).
- Log-field lint (`crates/quarry-edge/tests/log_field_discipline.rs`): the new log fields are a count and a boolean; not run.

## Evidence owed (WS13.1 DoD, not done)

1. Kill-switch failing run: the live test with `QUARRY_SCRAPE_SESSION_ISOLATION=0` on the old code must fail. This is the proof of finding A.
2. p50 of 20 sequential plain scrapes before and after, within 10 percent; per-request proxy-listener setup cost (`endpoint_for_session`, `crates/quarry-browser/src/chromiumoxide.rs`).
3. Two concurrent `POST /v1/scrape` at a cookie-reflecting page with two org tokens; no `lease guard poisoned` in the edge log.

## Decisions taken (flag to the arbiter)

- Flag default `on` (queue row and plan), while the build brief says dark or off by default. The legacy behaviour shares cookies across tenants, so on is the plan's choice. Rollback is the env var. The arbiter confirms.
- The flag is read once, at adapter construction (plan wording: per fetch). `with_session_isolation` overrides it for tests.
- Managed providers (Browserbase) inherit isolation through the same adapter; each fetch may then open a new remote session. Unmeasured; the arbiter decides whether isolation applies there.

## Next batch needs to know

- WS13.4 (actions) and WS13.4a (captures) take the heavy permit via `BrowserDriverAdapter` (`heavy` field; `heavy_permits_available` for reads). Nothing takes it yet, so `QUARRY_SCRAPE_HEAVY_PERMITS` has no behaviour effect until then.
- WS13.4a (captures) also edits `browser_driver.rs` literal sites (H6): the `session_isolation` and `heavy` fields are set in both constructors; any new literal `BrowserDriverAdapter { .. }` would need them (none exists today; the constructors are the only builders).
- WS6 can drop the comment in the legacy branch about the empty org (the seeded lease now carries the request org, per-call copy still stamped).
- Other units' uncommitted hunks remain in `crates/quarry-edge/src/main.rs` (`profile_guard`, `profile_owner_enforce`) and in `docs/RUNBOOKS.md`. Those units commit their own hunks.
