# Depth findings: D2-B-x12-budget (X-12, decision 3.3)

Findings I did not fix, each with where it lives and what it would take.

## 1. Model Plane 402 mapping (follow-up for the Model Plane session; not a blocker for the flip)

Quarry answers an exhausted run with `402 BUDGET_EXHAUSTED`, `details.dimension` one of steps, attempts, runtime or cost, `details.limit_source`, `details.resumable: false`. The Model Plane does not map it yet: `apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs` (`AgentClientError::Status`) and `browser_agent.rs` (WS11 `parse_typed_error`) treat it as a step error, so a run that reaches a runtime or cost bound shows as a failed step. Decision 3.3 step 5 says: map the 402 to a resource-limit terminal with partial results. Execution-core bounds its own runs first (tool_bridge defaults `max_steps` to 20, plan.rs `MAX_STEPS` 100), so the steps dimension is rarely reached from there; runtime and cost are the ones that surface. Not done here: it is another plane, and the owner scoped it as a follow-up.

## 2. An agent-initiated step in a person's session with no request cap is unbounded (also in `on`)

The guard keeps the *request's* step cap (`constraints.max_steps`, what the gateway sends as 12). A caller that starts a person-class session with `max_steps: 0` and then sends `initiated_by: agent` steps has no bound in `shadow`, `off` or `on`: the legacy cap treats 0 as unlimited and the typed gate has no caps for a person-class run. This is the same exposure the brief called "a direct API caller that sends 0"; the guard does not widen it, it only stops `on` from removing the cap that exists. Closing it means giving agent-initiated steps in a person's session the agent default (60 steps) from the ledger, which is a new rule the owner did not decide: the decision says people are uncapped, and says nothing about the model's steps in a person's own session when no cap is requested. Needs the owner.

## 3. The legacy cap counts the run-wide step number, person steps included

`enforce_step_budget` compares `entry.summary.step` (the lane's step counter, advanced by every step of every initiator) with `constraints.max_steps`. In a person's session capped at 12 by the gateway, once the person has taken 12 steps themselves, every model-proposed step is refused 429 from then on. That is how it behaves today in `shadow` and `off` too (it is not new with the guard), and `agent_run_budget.rs` pins the consequence ("the person's steps do not reopen the cap", "the model is still refused afterwards"). A cap on the model's own steps would use `budget.ledger.steps`, which already counts only the non-person steps of an interactive run. Worth deciding before the flip is announced to the Model Plane team.

## 4. The shadow would-exhaust metric does not see the legacy 429

Carried over from the decision's open risks. `quarry_agent_budget_exhausted_total{mode="shadow"}` counts typed-gate bounds only; a step the request's own cap refuses (429) is not in it, so the shadow review cannot see how often the guard's cap is what stopped a model step. Adding a `legacy_cap` series is small (one counter in `enforce_step_budget`'s caller) but it is a metric name change and was not asked for.

## 5. `BudgetMode` is read once per process

`BudgetMode::cached()` is a `OnceLock`, so two modes cannot be proved in one test process. `agent_run_budget.rs` therefore proves `on` in-process and `shadow`, `off` and the env-tuned numbers in child processes of the same test binary (`--ignored --exact child_*`, own environment). It works, but a child fails with the parent's output printed rather than a test failure of its own. If more per-mode tests are wanted, an injectable mode on `AppState` would be cleaner than more children. The unit tests keep using the thread-local `test_mode::pin` (cfg(test) only).

## 6. Stack proof preconditions (for the stack session)

`scripts/x12-budget-stack-proof.mjs` sets nothing. It exits 2 with "nothing was proved" when the edge reports `enforcement` other than `on`. It needs `QUARRY_EDGE__BUDGET_ENFORCEMENT=on` and `QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS=5` set by the stack session, a Quarry user token (`QUARRY_TOKEN`) and, if the stack requires grants for every action, `GRANT_ID`. Its network half has never run; only `node --check` and the canned-reply `--self-test` have. Two things the script cannot know until a real run: whether a `wait` action is accepted on a real session without a page (override with `ACTION_JSON`), and whether the step reply carries `meta.budget.steps` where it looks (the takeover leg tolerates its absence). The resume leg (plan WS6.B step 4), the checkpoint `psql` check and the SSE count are manual and listed at the top of the script.

## 7. Keep `GRANT_BUDGET_CLAIM` at `ignore`

Decision 3.3 step 6. Nothing here touches it; `agent_run_budget.rs` removes the variable from its own process so a claim cannot leak into the numbers.
