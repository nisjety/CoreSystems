# W2-U21a status: a refused navigation is charged the refusal rate (WS6.B remainder)

## Built, partly

Built in code and tests: the cost rule and its shadow counters. Not built: the switch of the three failed-step call sites to the error-aware function, so the driver-refused navigation (the SC4 D3a case, a navigate that the run's domain grant or the egress guard refuses before a page loads) also takes the rule. Those call sites are in `agent_routes/step.rs` and `agent_routes/attach.rs`, which are outside this unit's files, so they still call `failed_step_cost`.

Effect today: only the landed-refusal case changes (a navigate that ran, whose landed page the edge refused, and whose cost the driver did not meter). The D3a receipt stays at 1000 micro-USD until the call sites switch (see "Call-site switch" below).

## Commits (branch w23/lane-b, local, not pushed)

- `fd8f338a4` feat(quarry-runtime): refusal rate for a navigation refused by policy or egress (W2-U21a). `action_cost.rs`: `REFUSAL_RATE_USD`, `estimate_refused_action_cost`, 3 tests.
- `306abff39` feat(quarry-edge): charge a navigation refused by policy or egress the refusal rate (W2-U21a). `agent_routes/budget.rs`: `failed_step_cost` (landed refusal), `failed_step_cost_for_error` (new), the shadow counters, `refusal_cost_tests` (8 tests).
- this commit: this file and the depth rows in `.plan-work/quarry-w1w2/W23-depth-lane-b.md`.

## What the rule does

- Rate: a navigate refused by policy or egress costs 0.0001 USD (100 micro-USD). The flat navigate cost it replaces is 0.001 USD plus 0.1 micro-USD per byte (1000 micro-USD with no bytes). Other actions keep the flat table; back, forward and reload already sit at the base rate, so their refusal costs the same as before.
- Landed refusal (`ran` is `Some`): the navigate costs the rate unless the driver metered it. A driver figure is still kept (ProviderBilled), as T4b-1 intends.
- Driver refusal (`ran` is `None`, error `SECURITY_BLOCKED`, through `failed_step_cost_for_error`): the rate. A timeout, a `BadRequest` or any other failure keeps the flat navigate cost.
- The gate projection is unchanged: `budget_gate_with` still projects a navigate at the flat 1000 micro-USD, because whether it will be refused is not known before it runs.
- Receipt: a refused navigate's `cost_micro_usd` becomes 100 (was 1000) on the paths that switch; `cost_source` stays `flat_table`.
- Shadow counters, recorded in every mode and labelled by the BUDGET_ENFORCEMENT mode (`off`, `shadow`, `on`):
  - `quarry_agent_refused_navigation_total`
  - `quarry_agent_refused_navigation_flat_micro_usd_total` (what the flat table would have charged)
  - `quarry_agent_refused_navigation_charged_micro_usd_total` (what the rule charged)

## Flag and default

No new flag. BUDGET_ENFORCEMENT stays `shadow` (default, not flipped). The rule applies in every mode, because it changes only the amount charged to the ledger and the receipt. `spent` falls after a refused navigate, so the gate outcome changes only under `on` (and the shadow would-exhaust count can move).

## Assumed decisions

1. The refusal rate is 0.0001 USD. The scope names "the refusal rate" but the repo had no number. Taken: the table's base rate, which `wait` and every unlisted action cost. The live refused-landing receipt on a wait was 100 micro-USD (R3-SC4 S1b).
2. "Refused by policy or egress" means a navigate whose failure is `SECURITY_BLOCKED`. That code covers the driver's navigation guard (`domain_grant_blocked`, `url_or_dns_blocked`, the target guard in `quarry-browser/src/navigation.rs`) and the edge's landed-page check.
3. A landed refusal is a refusal by construction: `step.rs` and `attach.rs` set `ran` only when the action ran and the landing check then refused it. That is why `failed_step_cost`, which has no error argument, can apply the rule with no call-site change.
4. A driver-verified cost is kept for a refused navigate. Only the flat figure is replaced. This keeps the T4b-1 rule for metered actions (commit 304d6cb22). That commit also says a failed action stays on the flat table; this unit reverses that for navigations refused by policy or egress only, as the scope asks.
5. The rule is not flag-gated. The scope asks for the rate and for shadow counters that keep both numbers, and the change is a billing correction (a refused navigate was charged a full navigation). If the lead wants it observe-only, the switch is one line: have `failed_step_cost` and `failed_step_cost_for_error` return the flat table while the counters keep recording both numbers.
6. The source stays `flat_table`. A new `CostSource` variant would need the `cost_source` CHECK in migration 0012 changed and a wire change.
7. The counters count refused navigations of every class. A person's session gets the same rate in its receipt, since its receipt uses the same function. The class is not in the counter labels because `failed_step_cost` has no class argument; the call sites can add it with the wiring.
8. The new counter names are not in `metrics.rs` `describe_all` (WS6.I owns that list). They work unregistered.

## Call-site switch (not applied: outside this unit's files)

In each place below, replace `failed_step_cost(ran.as_ref(), &receipt_action)` with `failed_step_cost_for_error(ran.as_ref(), &receipt_action, &error)`. In the WebSocket path, keep the trailing `.0`.

- `agent_routes/step.rs`, REST `step`, `Err(error)` arm (about line 230).
- `agent_routes/step.rs`, WebSocket path in `execute_ws_action`, `Err(error)` arm (about line 402, `failed_step_cost(ran.as_ref(), &receipt_action).0`).
- `agent_routes/attach.rs`, delegate step, `Err(error)` arm (about line 834).

Then remove the `#[allow(dead_code)]` line above `failed_step_cost_for_error`. Until the switch, that function has no non-test caller and the attribute keeps the non-test build quiet.

## MP asks

None. Model Plane does not read these costs, so nothing is appended to `MP-ASKS.md` from this unit.

## Tests (written, not run)

- `quarry-runtime` `action_cost::tests`: `refusal_rate_is_the_base_rate_and_below_the_flat_navigate_cost`, `a_refused_navigation_is_charged_the_refusal_rate_whatever_the_bytes`, `other_refused_actions_keep_their_flat_table_cost`.
- `quarry-edge` `agent_routes/budget.rs`, module `refusal_cost_tests`: `a_navigation_the_driver_refuses_costs_the_refusal_rate`, `a_navigation_that_times_out_is_not_a_refusal_and_keeps_the_flat_cost`, `a_refused_action_that_is_not_a_navigation_keeps_its_flat_cost`, `a_refused_landing_costs_the_refusal_rate_unless_the_driver_metered_it`, `the_shadow_pair_keeps_the_flat_navigate_cost_beside_the_refusal_rate`, `refused_navigations_charge_the_refusal_rate_to_the_ledger` (cap 1500 micro-USD: six refusals run, spent 600), `under_the_flat_navigate_charge_the_same_cap_refuses_the_second_navigation`, `a_resumed_segment_keeps_the_refusal_charges_it_resumes`.
- For the integration step: `cargo test -p quarry-runtime action_cost` and `cargo test -p quarry-edge refusal_cost`, through qtest.

Existing tests that touch these paths and still hold by reading: `agent_routes/tests.rs` `a_refused_landing_keeps_the_driver_cost_and_a_failed_action_the_flat_table` (uses a `Wait`), and `tests/agent_landed_page.rs` line 180 (driver figure 250000, `provider_billed`). A grep of `tests/` and `src/` found no assertion of a flat navigate cost on a refused landing.

## Not verified

- Not compiled and not run (code only, per the owner order). The Rust was checked by reading it against the signatures it uses: `ErrorCode::SecurityBlocked` and `Timeout`, `QuarryError::with_details`, `AgentAction::Navigate { url }`, `RunBudget::start`, `gate`, `charge`, `snapshot`, and the public fields of `BudgetLedger` and `RunBudget`.
- The `metrics::counter!` calls use the form `run_live.rs` already uses.
- The live effect (D3a 1000 to 100) is not observed. It needs the call-site switch and a stack run.
- Dashboards that sum navigate costs will move for refused navigations (WS6.I).

## Evidence read for this unit

- `stack-2026-10-28/live/rerun-2026-10-29/R4-SC4.md` depth 3 and `CHECK.md` item 3: a refused navigate was charged 1000 micro-USD (D3a receipt, `cost_source` flat_table).
- `depth/W2-U21.md`, `W2-U21-progress.md`: failed-attempt receipts carry the flat cost since WS6.B (D2-199).
- `docs/plans/quarry-browser-build-plan-2026-10-14.md` WS6.B: the flat table and the budget ledger; no refusal rate was specified.
- Findings for the lead are in `W23-depth-lane-b.md` under "W2-U21a".
