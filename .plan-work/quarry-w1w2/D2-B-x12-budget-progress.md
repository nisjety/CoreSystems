# D2-B-x12-budget progress and hand-over (2026-10-28)

Unit: decision 3.3 X-12 (list B: built and tested now; the flag stays `shadow`, nothing was flipped). Branch `quarryv2-next`, local commits only. Findings I did not fix: `depth/D2-B-x12-budget.md`.

## Committed

| Commit | What |
|---|---|
| 1b701f99b | the edge guard: `legacy_step_cap_applies(budget, mode, person_step)` (budget.rs) and its call site (agent_routes/admission.rs); in `on` an agent-initiated step inside a person's session keeps the request step cap (429), a person's own steps stay uncapped, an agent-class run is owned by the typed 402 gate; the pinned test at agent_routes/tests.rs was rewritten (it contradicted decision 3.3); the "placeholder" docs now say the numbers are ratified |
| 430ce8c0d | crates/quarry-edge/tests/agent_run_budget.rs: 10 tests + 3 child tests (numbers, defaults and ceilings, steps/runtime/cost 402, people uncapped, guard, takeover exempt, shadow/off/env-tuned in child processes) |
| b4eef31e5 | scripts/x12-budget-stack-proof.mjs (stack session; sets nothing; PASS/FAIL/NOT-RUN per bound) |
| (this commit) | this file and depth/D2-B-x12-budget.md |

## The numbers (item 3)

Already in `BudgetLimits::default` / `BudgetLimits::from_env` (budget.rs) with the ratified values, each its own env variable: `QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS` 60, `..._DEFAULT_MAX_RUNTIME_S` 900, `..._DEFAULT_MAX_COST_USD` 1.0, `..._CEILING_MAX_STEPS` 300, `..._CEILING_MAX_RUNTIME_S` 3600, `..._CEILING_MAX_COST_USD` 20.0, `QUARRY_EDGE__BUDGET_BROWSER_SECOND_USD` 0.00003 (an estimate, not a price), `QUARRY_EDGE__BUDGET_ATTEMPT_FACTOR` 2. Nothing in code changed for this; the test pins them.

## Checks run (all through qtest, rust:1 container)

- `cargo test -p quarry-edge --test agent_run_budget`: 10 passed, 0 failed, 3 ignored (the children, run by their two parent tests).
- `cargo test -p quarry-edge --lib`: 634 passed, 0 failed (includes the budget unit test and the agent_routes budget tests).
- `cargo test -p quarry-edge --test grant_budget_claim`: 8 passed. Also `takeover_exempt_counter` 1, `human_browse` 5, `agent_run_control` 35, `log_field_discipline` 22, all passed.
- `cargo check --workspace --all-targets`: rc=0 (warnings are other units' unused imports; none from my files).
- `node --check scripts/x12-budget-stack-proof.mjs` ok; `--self-test` prints SELF-TEST PASS (canned replies, no network); with no `QUARRY_TOKEN` it exits 2 without a request. Not run against any stack.
- `node --test scripts/tests/*.test.mjs` in Quarry-v2: 41 pass, 0 fail, 12 todo (pre-existing pending).

## Traps found

- The start body's `constraints` needs `max_steps` (0 = not asked); `{}` is a 422. "Asks for nothing" means leaving `constraints` out. Both the test helper and the script handle it.
- `BudgetMode::cached()` is read once per process, so a second mode needs a child process (the test re-runs its own binary with `--ignored --exact`).
- Two failed qtest attempts were another agent's mid-edit (`redaction::record_policy` not yet re-exported from quarry-core); the third compiled.
- The legacy request cap counts the run-wide step number, person steps included (see depth file, item 3).

## For the stack session (steps 3 to 6 of decision 3.3)

1. Scripted day in shadow, then set `QUARRY_EDGE__BUDGET_ENFORCEMENT=on` and `QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS=5` on the edge and restart it.
2. `QUARRY_TOKEN=<user token> node scripts/x12-budget-stack-proof.mjs` (see the header of the script for the optional variables). Fix whatever the network half finds (action choice, `meta.budget` field).
3. By hand: resume of an idle-expired run is refused at once; `psql` on `quarry_agent_run_checkpoints` shows `state->>'budget_exhausted' = 'steps'`; SSE shows one `agent.budget_exhausted`.
4. `on` in the Ingestion env; `off` is the rollback. Keep `GRANT_BUDGET_CLAIM` at `ignore`.
5. Model Plane session: map the 402 to a resource-limit terminal with partial results (depth item 1).
