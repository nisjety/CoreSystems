# W2-U34 progress (WS7.G, backstops 6a)

Status 2026-10-24: 6a built to scope, compile-checked, tests written and NOT run (build-first rule). 6b not built (waits for WS14 and WS15 deployed). Local commits only.

## Committed (git log)
- fb64c4f0d core and security: `ResolvedPolicy::denied_hosts`, `SecurityEngine::recheck_final_url` (default false)
- 832ed171d quarry-runtime: `policy_gate.rs` `PolicySecurityEngine`, final-URL recheck in `pipeline.rs` `run_inner`, pipeline tests
- 7333d17d5 quarry-browser: `with_denied_domains`, `denied_domains`, `denies_url`, `is_unconstrained`, deny in `allows_url` for every mode (hunk-only commit: the `capture_plan` cfg hunk in the same file is another unit's)
- 958fdad17 `ObservationContext.egress_denied` merged into every action's egress policy; `tests/egress_deny_list.rs`; ignored live test in `chromiumoxide.rs`
- e1a87b4de edge: `QUARRY_EDGE__WEB_POLICY_FINAL_URL_CHECK`, `web_policy_edge::security_engine`, agent path deny list (`egress_policy`, `live_egress_policy`, `egress_denied`, tabs.rs, lifecycle.rs, human_browse.rs)
- 4b05fd570 edge routes.rs: the three `PageRunner` literals (hunk-only; the other routes.rs hunks are other units')
- 53df13da1 edge test `tests/web_policy_backstop.rs`
- f8a24e56e RUNBOOKS section (HEAD blob plus the section only)

## Not committed
Nothing of mine. The working tree still holds other units' hunks in `routes.rs`, `quarry-browser/src/lib.rs` (cfg line), `docs/RUNBOOKS.md`, `main.rs` and others (see `git status`). My edits to those files are also in the working copy, so the working tree and HEAD agree on my hunks.

## Verified (only what ran)
- `cargo check -p quarry-core -p quarry-security -p quarry-browser -p quarry-runtime -p quarry-edge --lib --tests` through qtest: everything compiles except the `quarry-edge-rs` bin test target, with 4 E0433 errors `cannot find scrape_actions in the crate root` (main.rs has no `mod scrape_actions;`; not a file I touched). No warnings in files I touched.
- NOT run: every test.

## For the next batch
- Flag default derives from `ENVIRONMENT` (dev-like names on, else off).
- `ObservationRunner::execute` still reinstalls the start-time egress each action (depth bullet, high).
- 6b needs `RunPageInput.PolicySnapshot` (orchestrator), `InternalRunPage.policy_snapshot`, `verify_run_binding` extension; `ResolvedPolicy::denied_hosts` and `digest` are the pieces to serialise.
