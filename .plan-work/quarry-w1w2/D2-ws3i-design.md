# D2-ws3i design note: WS3.I escalation, preset rename, default flip (rev 2, 2026-10-23)

Q = apps/Ingestion Plane/Quarry-v2. Read: plan "Slice WS3.I detail" (l.905), breakdown item d, baseline row, and the current code. **State: the slice is already built** (61906c93 mechanism, 64346fd1 preset, 83e920f3 docs, 89097598 flip LAST, review fixes 654806c2, 8cf01450, 8566f2c1, 39b0dec3). This revision checks the design against the code as it stands; a builder re-run must not redo it. `stealth.rs` is gone (WS3.E); no browser-crate change.

## 1. Contracts
- **`quarry-runtime/src/escalation.rs`** (lib.rs:27 `pub mod escalation;`). `ESCALATION_ENV = "QUARRY_BLOCK_ESCALATION"`; `BUILD_DEFAULT` (l.34, now `Honest`). `EscalationMode::from_value`: unset/blank = default, `legacy` = Legacy, anything else = Honest (a typo fails closed). `current()` is a `OnceLock`, logs only the mode name. `escalation_allowed(mode, trigger)`: Legacy allows all four triggers (`BlockStatus`, `StrongChallenge`, `CallerSignals`, `TransportError`); Honest only `TransportError`. `trigger_for_error`: `UpstreamBlocked|RateLimited` -> `BlockStatus(0)`, else transport.
- **Plumbing:** `StaticDriver`, `EgressBroker`, `FallbackDriver` hold a `mode` from `current()` with a `with_escalation_mode` builder; `driver_plan::plan_from_signals_with(mode, ..)` is pure and `plan_from_signals` delegates. Tests pass the mode, never set env (process-global).
- **Honest behaviour:** fetch.rs:278 a block status on a proxy refuses at once with the existing code (X-16), transport errors rotate (l.305). egress_broker.rs: a status no longer cools the proxy (otherwise the NEXT request changes identity); label `block_rotation` -> `health_rotation` in both modes (l.162; value change in `details.egress_attempts[].decision_reason`). fallback_driver.rs: block status returns without a next driver; the `Err` path (l.294) is gated by `trigger_for_error`, so `UpstreamBlocked` does not fall through but `Timeout` does; `challenge_forbids_escalation` (l.72) stops a 200 strong-challenge body from going to the browser, while content-based JS-shell escalation stays. driver_plan.rs:52 ignores `prior_block_signals`; openapi.yaml:5166 marks it `deprecated: true`.
- **Preset (presets.go):** `stealth` -> `careful` (l.99, tags `careful`,`slow`, description says its `Block` is advisory: no Rust reader). Additive fields `alias_of`, `deprecated`; `deprecatedPresetAliases = {"stealth":"careful"}` (l.33) emits an alias entry with the same policy; GET of the alias sends `Deprecation: true` and `Link: </v1/presets/careful>; rel="successor-version"` (l.48-50). No migration: jobs store the resolved policy, not the name.

## 2. Security, concurrency, ZDR, cache
- Fail closed = no identity change on a status, a block-class error, a strong challenge or a caller hint; refuse with an existing code. Nothing spoofs (no UA/TLS/header change, no stealth term).
- Known fail-closed side effect: the static driver reports a connect failure as `UpstreamBlocked`, so under Honest a connect failure does not fall through to the TLS driver (still rotates proxies inside StaticDriver). Documented in RUNBOOKS.
- `OnceLock` init is race-free; mode fixed per process, rollback = `QUARRY_BLOCK_ESCALATION=legacy` + restart. The health map keeps its Mutex; no new shared state, no new persistence.
- Logs carry mode, status, attempt index, egress class only; log-field lint baseline unchanged.
- Cache: under Honest `prior_block_signals>=2` yields a static plan, which may read/write the page cache; cache_policy.rs gates the Legacy case on `current()` (l.631) and adds `honest_prior_block_signals_do_not_bypass_the_page_cache` (l.692).
- Callers: model-gateway's browser-render retry moved to `profile_required` (8566f2c1). verevonv3 gateway knowledge/products.rs:216 still sends `prior_block_signals: 1`: its TLS retry now repeats the static fetch (documented regression, not fixed; that file is outside this unit).

## 3. Edit order (as shipped)
1. escalation.rs + one lib.rs line; existing rotation tests pinned to Legacy explicitly; default Legacy.
2. Honest branches in fetch/egress_broker/fallback_driver/driver_plan + tests; openapi one field.
3. Go rename + alias + tests. 4. Docs; RUNBOOKS append-only ("Block escalation").
5. LAST, separate: `BUILD_DEFAULT = Honest`, gated by `build_default_matches_the_documented_default`; message says no measurement was possible.
Hot files touched: lib.rs (one line), openapi (one field), RUNBOOKS (append; another unit's WS4.H hunks there were excluded by filtered `git apply --cached`). pipeline.rs, routes.rs, main.rs read only.

## 4. Tests that prove each rule
- Matrix/env: `matrix_legacy_allows_every_trigger`, `matrix_honest_allows_only_transport_error`, `unset_is_build_default_and_unknown_is_honest`, `block_class_errors_map_to_block_status`, `build_default_matches_the_documented_default`, `the_env_var_reaches_current_and_legacy_is_the_rollback` (child process).
- fetch.rs: `honest_mode_never_rotates_on_status`, `legacy_mode_unchanged`, `honest_mode_still_rotates_on_transport_error`, `honest_mode_does_not_move_the_next_request_to_another_proxy_after_a_403`.
- egress_broker.rs: `honest_status_does_not_cool_down_the_proxy`, `honest_transport_error_still_cools_the_proxy_down`, `rotation_label_is_health_rotation`.
- fallback_driver.rs: `block_status_rotates_to_next_fingerprint` (Legacy), `honest_block_status_returns_without_next_driver`, `honest_upstream_blocked_error_does_not_fall_through`, `legacy_upstream_blocked_error_still_falls_through`, `honest_timeout_still_falls_through`, `honest_js_shell_still_escalates_to_browser`, plus the 39b0dec3 TLS-to-browser cases.
- driver_plan.rs: `prior_block_signals_legacy_tiers_unchanged`, `prior_block_signals_ignored_in_honest_mode`.
- Go: `TestPresets_ListHasCarefulAndStealthAlias`, `TestPresets_StealthAliasResolvesToCarefulWithDeprecation`, `TestPresets_CarefulBlockIsAdvisoryAndSaysSo`, `TestPresets_CarefulAbortsOnChallenge`, `TestCreateJob_AppliesPresetPolicy`.
- Gates: qtest `cargo test -p quarry-runtime` (also with `QUARRY_BLOCK_ESCALATION=legacy`), `-p quarry-edge` incl. log_field_discipline and cache_policy (needs Redis, else skips), `-p quarry-core`, `cargo check --workspace --all-targets`; `GOWORK=off go test ./...` in quarry-control; `node --test scripts/tests/*.test.mjs`.

## 5. Will NOT do
No measurement claim and no live-stack 403 run (needs a deployed edge and live origins); no removal of `prior_block_signals`, the Legacy branches or the `stealth` alias (one release later); no WS3.J/SPA work; no fix of the pre-existing leak in fetch.rs `attempt_record` (l.435-456 puts `proxy_uri` and raw error text into `details.egress_attempts`; follow-up); no change to verevonv3 products.rs; no log baseline regeneration; no browser-crate change.
