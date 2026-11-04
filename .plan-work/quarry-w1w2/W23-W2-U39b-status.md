# W23 W2-U39b status: human-browse TTLs and RenewGrant wiring in the sweeper (D2-060)

Lane d, branch w23/lane-d, 2026-11-03. **Code only, not compiled, no test run.**

## Built: partly (edge side complete, broker call is a seam)
- Gap check first: none of it existed. The sweeper already had interactive defaults (900/1800/14400 s), but a human-browse lease got the 120 s default (`constraints.max_runtime_s.unwrap_or(120)` after the human branch zeroes the bounds), and nothing in Quarry knew RenewGrant.
- `lifecycle.rs`: constants `HUMAN_BROWSE_IDLE_TTL_S` 900, `HUMAN_BROWSE_TAKEOVER_IDLE_TTL_S` 1800, `HUMAN_BROWSE_ABSOLUTE_TTL_S` 14400; `human_browse_lease_ttl_s` (the 4 h backstop, or the time left to the grant's absolute cap, at least 1 s); the start uses it for a human-browse run. Two unit tests.
- `run_sweeper.rs`: an open-web (human-browse) run is judged by the human-browse lifetimes whatever its class. Behind `QUARRY_EDGE__HUMAN_GRANT_RENEW` each pass validates its grant (`validate_for_org`), renews it inside a lead of two sweep intervals (at least 60 s) but never when the expiry already equals the cap, and closes the run with a typed `HumanGrantEnd` (`grant_revoked`, `grant_expired`, `grant_absolute_cap`, `grant_renewal_denied`). A broker outage or an unavailable renewer keeps the run. New: `HumanGrantRenewer` trait, `UnwiredHumanGrantRenewer` (the edge default), `check_human_grant`, `human_browse_expiry`, `sweep_once_with` (explicit grant half; `sweep_once` keeps its signature). Eleven unit tests with a fake validator and renewer.

## Commits
- `c81963e2b` fix(quarry-edge): a human-browse lease lives the 4 h backstop (lifecycle.rs)
- `b197eac66` feat(quarry-edge): the run sweeper keeps human-browse grants alive (run_sweeper.rs)
- the docs commit with this file, the MP-ASKS section and the lane d depth rows

## Flag and default
- `QUARRY_EDGE__HUMAN_BROWSE` stays off (unchanged); without it no human-browse run exists, so none of this runs.
- New: `QUARRY_EDGE__HUMAN_GRANT_RENEW`, default off (unset/0/false/off), read at each sweep; unreadable is off with a warning. Off means today's sweeper plus the human-browse lifetimes and lease ttl (those two are the D2-060 fix, unflagged, dark behind HUMAN_BROWSE).

## Assumed decisions
- Lifetimes are fixed constants (plan values), not the operator's `interactive_*` RunLimits, and apply to every open-web run whatever its class.
- A grant end closes the run as an expiry (`CloseReason::Expired`, `ExpireReason::MaxAge` on the event; the typed reason is on the log line), because `run_live.rs`, `close.rs` and `metrics.rs` were outside the unit.
- A broker that cannot be reached never ends a person's session from the sweeper; per-step revalidation still fails closed.
- In a dry run the sweeper still validates and renews; only the release is skipped.
- A run whose entry is locked during the pass is judged by its ordinary verdict (non-human runs behave exactly as before).

## MP asks
- `.plan-work/quarry-w1w2/MP-ASKS.md`, section W2-U39b: let Quarry's service principal renew a `human_browse` grant (RenewGrant is user-only today) or say no; then vendor RenewGrant into Quarry's browser.proto and confirm the status-code mapping.

## Not verified
- Nothing compiled (`cargo check` / `cargo test -p quarry-edge` not run, per the owner order); the integration step compile-checks.
- No integration test of `sweep_once_with` over a registered open-web run (depth row); no live run.
- No real renewal: the edge renewer stays unwired until the MP ask is answered.
