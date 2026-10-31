# W2-U43 progress (WS6.D-s, static LimitsResolver)

Status: built to the plan's static scope, committed, compile-checked, NOT run (wave strategy).

Commits (branch quarryv2-next):
- cfdc8a07a feat(edge): static organization limits resolver, allowance clamps on crawl, batch and agent start (WS6.D-s)
- b867a9fed feat(gateway): map the org_allowance 402 to crawl_allowance_exhausted with nb and en copy (WS6.D-s)
- (plan-work notes commit follows)

What exists
- quarry-edge `limits.rs`: LimitsResolver, StaticFloor, Allowance, OrgLimits, PlanCaps, FailMode + WithFailMode over LimitsBackend, LimitsConfig (QUARRY_EDGE__LIMITS_SOURCE, QUARRY_EDGE__LIMITS_FAIL_MODE), effective_capacity, tighten_ceiling, clamp_pages_to, clamp_batch_urls, HandoffLimits, UrlsBound, AllowanceExhausted (402 org_allowance).
- `AppState.limits` (state.rs; literals: main.rs, routes.rs test helper, test_support.rs, tests/support/mod.rs).
- `crawl_handoff` / `batch_handoff` clamp; `start_run` refusal and `RunBudget::with_entitlement` (budget.rs, lifecycle.rs).
- gateway `start_crawl` 402 mapping and SPA `errors.ts` row.
- openapi, CONTRACTS, BILLING, RUNBOOKS (appended).
- Tests written: limits.rs `resolver_tests` (15), budget.rs (4), `tests/limits_clamp.rs` (8), gateway (2), `errors.test.ts` (1).

Commands and results
- `cargo check -p quarry-edge --lib --test limits_clamp --test crawl_limits --test job_admission --test zdr_claim_floor` (qtest): exit 0.
- `cargo check --tests` in apps/gateway: exit 0.
- `npx vitest run src/shared/i18n/errors.test.ts`: 8 passed.
- `cargo check -p quarry-edge --bin quarry-edge-rs`: blocked by another unit's `scrape_actions` (not ours).

For the next batch
- U59 (WS6.G remainder) reads `state.limits.org_limits(org).await?.caps` and `effective_capacity(floor, plan, global_cap, origin)`.
- A billing_core source is a `LimitsBackend` wrapped in `WithFailMode`; add it in `LimitsConfig::build` and remove the boot refusal.
- main.rs and routes.rs were committed as HEAD blob plus this unit's hunks only; the working copies still carry other units' uncommitted hunks.
- RUNBOOKS.md in the working tree is shorter than HEAD (other units); my section was appended to the HEAD blob.
