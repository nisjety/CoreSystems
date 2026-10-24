# W2-U59 progress (WS6.G remainder: plan-aware run caps)

Status: built to scope, committed, compile-checked, NOT run (wave strategy).

Commits (branch quarryv2-next)
- 254737fa7 feat(edge): plan-aware run caps, org_plan_cap 429 on start_run (limits.rs, run_registry.rs, lifecycle.rs, tests/org_capacity.rs, ERROR_REASONS.md, gateway browser.rs)
- 707c6837c docs(quarry): runbook section + openapi line. MISTAKE: built from a stale HEAD blob, it dropped U58's docs.
- f5a692976 fix: restores U58's profile-restore docs; net effect of the two is only the WS6.G docs hunks.
- (plan-work notes commit follows)

What exists
- `RunCapOverride` + `OrgLimits::run_override()` (limits.rs); `RunRegistry::try_reserve_with` (try_reserve is the no-plan wrapper); `start_run` reads limits once for both classes (a person's session falls back to the floor on a source error).
- A plan-bounded class answers to the plan value alone (reason `org_plan_cap`), capped by MAX_CONCURRENT_RUNS; unbounded class keeps static caps. Static source reports no plan caps, so nothing changes by default.
- Render permits (`QUARRY_EDGE__POOL_ORG_MAX_PERMITS`, default 0 = off) were already built and threaded (browser_driver passes the org id); not plan-aware.
- Tests written: run_registry.rs `plan_cap_tests` (8), limits.rs resolver_tests (3), tests/org_capacity.rs (7), gateway (1 new + 2 rows).

Commands and results
- qtest: `cargo check -p quarry-edge --lib --tests -v` (lib, lib test, org_capacity, bin) exit 0.
- gateway `cargo check --tests` exit 0 (one pre-existing unused-import warning in browser.rs test imports).
- Docker was briefly down mid-unit ("DOCKER IS NOT ANSWERING"); one retry later it answered. Native Windows check is impossible (boring-sys needs libclang).

For the next batch
- The plan test-names `org_cap_*` in lease_pool.rs already exist; the plan-named test list is covered by the new files above.
- TRAP: when committing docs through qcommit-blobs.sh, build the blob from `git show HEAD:` immediately before the commit, not minutes earlier; another unit's commit in between gets silently reverted.
- Depth bullets in depth/W2-U59.md.
