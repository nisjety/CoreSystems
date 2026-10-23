# W2-U21 progress (WS6.B remainder, agent-run budgets)

Status: BUILT to scope and compile-checked (commits below) (cargo check --tests for quarry-core, quarry-runtime, quarry-runtime with postgres-queue, quarry-edge: all rc 0 in the rust:1 container). NOT run: no test suite was executed, per the build-first rule. Flag default unchanged (`QUARRY_EDGE__BUDGET_ENFORCEMENT` = shadow). Nothing is deployed.

## Committed (quarryv2-next, local, not pushed)

- e2a4c0c03 feat(quarry): agent-run budget receipts, meta.budget and agent.budget_exhausted (WS6.B remainder)
  - quarry-core: EventType::AgentBudgetExhausted (appended at the end); contracts::BudgetMeta; EnvelopeMeta.budget (skipped when absent); Envelope::with_budget.
  - quarry-runtime: migrations/0012_step_receipt_cost_source.sql (new); StepReceipt.cost_source; ReceiptBuilder cost_source() and failed_cost(); Postgres INSERT, three SELECTs, decode; migration test extended to 0012 (DATABASE_URL-gated); NATS token agent_budget_exhausted; browser_procedure.rs test literal gets cost_source: None.
- a4d7a608c test(quarry-core): EnvelopeMeta literal carries the budget field (hunk-only commit; the file also carries another unit's WS7.F hunks, left uncommitted).
- f3b6515ed feat(quarry-edge): budget refusal announced, meta.budget, usage and receipt cost source (WS6.B remainder)
  - budget.rs (ledger announcement flag, refusal record, event_payload, meta, usage, legacy_step_cap_applies) and tests.
  - agent_routes/budget.rs: receipt_cost_source, cost_source_label, announce_budget_refusal.
  - agent_routes/admission.rs gate 9; agent_routes/step.rs (REST cost source, failed flat cost, meta.budget); agent_routes/close.rs (finish_run returns usage; DELETE returns it); agent_routes/run_sweeper.rs (Ok(_)).

## Committed in the docs commits

- 199961724 docs(quarry-v2): agent run budget contract, OpenAPI BudgetMeta (its message also names the RUNBOOKS section, which went in the next commit)

- docs/openapi.yaml: BudgetMeta component; Envelope_BrowserObservation.meta.budget; Envelope_ClosedRun.data.usage (additive).
- docs/CONTRACTS.md: section "Agent run budgets: meta.budget, cost_source, usage and agent.budget_exhausted" at the end.
- 2eae5f6be docs(quarry-v2): RUNBOOKS section for the agent run budgets: appended to the HEAD content (patch against HEAD; the working copy is behind HEAD because other units committed their RUNBOOKS sections, and its uncommitted hunks stay uncommitted).
- .plan-work/quarry-w1w2: W2-U21-progress.md, wf-W2-U21-progress.md, depth/W2-U21.md, wf-W2-U21.md.

## Tests named by the U21 row

- budget.rs, present from 77374adc: interactive_class_is_never_budgeted, manual_session_12_steps_120s_constraints_do_not_cap, agent_cap_counts_completed_steps, failed_attempts_are_bounded_by_the_attempt_allowance, runtime_clock_excludes_takeover_pause, cost_projection_refuses_before_overshoot, cost_includes_browser_time_term, exhaustion_latches_and_repeats_the_same_error.
- budget.rs, added here (written, compile-checked, not run): resolve_caps_takes_min_of_request_grant_ceiling (the plan's name says org_ceiling; the resolver has no per-org term), exhaustion_is_announced_once_per_segment, meta_and_usage_report_the_ledger_and_its_source, legacy_step_cap_applies_only_where_the_gate_does_not_own_the_bound.
- contracts.rs error matrix: BudgetExhausted 402 and not-retryable assertions were already committed (tests/contracts.rs lines 49, 76, 98). Nothing added.
- step_receipts.rs, added here: failure_receipt_carries_the_flat_cost_it_was_charged, receipt_roundtrips_cost_source_and_serialises_it_as_the_column_value, legacy_receipt_without_cost_source_deserialises_to_none, legacy_checkpoint_without_budget_fields_deserialises.
- envelope.rs, added here: budget_is_absent_unless_attached, interactive_budget_omits_the_bounds.
- postgres_step_receipts.rs: the migration test covers 0012 (4 columns, bad cost_source refused); DATABASE_URL-gated, not run.

## Not done (scope boundary, see depth/W2-U21.md)

- WebSocket reply: no meta.budget (ws.rs single-writer, reply shape not decided in the plan).
- Model Plane: execution-core and model-gateway do not map the 402 or the event (sibling b).
- WS10 trace projection row for agent_budget_exhausted (R-11).
- Concurrent agent-run cap; per-organization allowance (billing-core, WAIT-OTHER).
- tests/agent_run_budget.rs integration test named in the WS6.B detail (not in the U21 row); deployment evidence steps 1 to 5 not run.
- action_cost.rs one-name mapping (hot file, not needed for the gate).

## For the next batch

- The gate's shadow behaviour changed in one respect: a person's session is no longer refused by its own request step cap in shadow (and on). Agent runs keep the legacy 429 in shadow. Read depth/W2-U21.md before turning `on`.
- migration 0012 is taken; WS8.F keeps its planned 0018 (queue ruling 0.1); the next free runtime migration number is 0013 (WS12.S3) unless the ruling says otherwise. 0015 is already taken by WS17.P1 (U15).
- StepReceipt has a new field cost_source: any new StepReceipt literal must set it (a `..` spread or `cost_source: None`).
- ReceiptBuilder::fail now takes its cost from failed_cost(); callers that do not call it still record 0.
