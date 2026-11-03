# W2-Q-WS7K status: WS7.K Quarry half, validator web_policy field and grant-layer gate (lane d, branch w23/lane-d)

## Built / partly
Partly. The Quarry half is built: the vendored proto, the validator field, the gRPC mapping, the L1 contract test, and the gate with its tests. Not built: the call sites (see Not done).

What existed before: nothing of WS7.K on the Quarry side. `quarry_core::web_policy::PolicyInputs` already had `grant: Option<PolicyLayer>` and `DecisionSource::Grant`, but every caller passed `None`. The vendored proto had no field 20 and `GrantValidation` had no snapshot.

- `crates/quarry-runtime/proto/browser.proto`: `message WebPolicy { uint32 v = 1; string rev = 2; map<string, string> caps = 3; string robots_max = 4; string proxy_max = 5; repeated string domains_allow = 6; repeated string domains_deny = 7; }` (plan D7 S10), plus `WebPolicy web_policy = 20;` on `AcquireGrantResponse` and `ValidateGrantResponse`. `DeriveRunGrantResponse` is not vendored, so the field is not added there.
- `scripts/tests/browser-grant-contract.test.mjs` (L1): the vendored-subset test accepts `WebPolicy`, `AcquireGrantResponse.20` and `ValidateGrantResponse.20` as `PENDING_UPSTREAM_*` while upstream lacks them. Once upstream has them they must match. A new test pins the vendored shape, including the `map` field, which `parseProto` does not see, so it is checked as text. It also checks that no request carries field 20.
- `crates/quarry-runtime/src/grant_validator.rs`: `GrantValidation.web_policy: Option<serde_json::Value>` (`serde(default, skip_serializing_if = none)`). The value is carried verbatim in the claim's shape. Noop, the HTTP 404 path and all test literals carry `None`. New tests: `noop_validator_carries_no_web_policy_snapshot` and `the_http_shim_json_may_omit_the_web_policy_snapshot_and_may_carry_it`.
- `crates/quarry-runtime/src/grpc/browser_broker.rs`: `validation_from_response` maps field 20 through `web_policy_json` into the claim's shape:
  - `{"v","rev","caps"{sorted},"robots_max"?,"proxy_max"?,"domains":{"allow","deny"}}`
  - An empty ceiling is left out.
  - An unknown version is carried through, so the edge refuses it. It is never dropped.
  - `inactive_validation` gives `None`.
  - Tests: `the_web_policy_snapshot_on_a_validate_response_is_carried_in_the_claim_shape`, `a_validate_response_without_a_snapshot_carries_none_and_an_unknown_version_is_kept`, and `the_web_policy_snapshot_survives_the_wire_from_a_real_grpc_peer` (scripted tonic broker).
- `crates/quarry-edge/src/agent_policy_gate.rs`:
  - `GrantSnapshot { Absent | Present{layer, rev} | Unreadable }::from_grant`. It parses with the claim's rules: v1 only, rev `"0"` or 1-12 hex, a mode or ceiling per cap key. Domain keys inside `caps` are refused. An empty allow list is no rule. Unknown keys are ignored and counted (`web_policy_grant_snapshot_unknown_key_total`). An unreadable snapshot is counted (`web_policy_grant_snapshot_unreadable_total`).
  - `StepLayers { grant, claim }` and `step_claim(baseline, claims)`.
  - `step_policy`: invariants, ZDR, catalogue defaults, the step claim as the org layer, and the snapshot as the grant layer. It returns `None` when the step states neither.
  - `gate_step_layers`, `gate_policy_for_step` (gate 8: the run's decision first, then the step layers) and `gate_visit_for_step`.
  - Because both checks must pass, a looser claim never loosens a tighter snapshot, and a tighter claim or snapshot tightens a running run on its next step.
  - Unit tests: `grant_snapshot_is_met_with_the_claim_per_step`, `looser_claim_does_not_loosen_a_tighter_grant_snapshot`, `tighter_claim_tightens_a_running_run_on_the_next_step`, `a_grant_without_a_snapshot_is_no_grant_layer_and_an_unknown_one_is_unreadable`, `an_unreadable_snapshot_fails_closed_under_enforce_and_the_claim_mode_bounds_the_layers`, and `a_human_is_not_held_to_autonomous_only_keys_of_the_snapshot`.
- Outside the unit's files, for compile only: `crates/quarry-edge/src/agent_routes/attach.rs` (2 literals) and `agent_routes/tests.rs` (1 literal) gained `web_policy: None`.

## Commits (w23/lane-d)
- 346efc29d feat(quarry-runtime): vendor the WS7.K WebPolicy snapshot as browser.proto field 20 (W2-Q-WS7K)
- 42dac1b66 feat(quarry-runtime): GrantValidation.web_policy and the gRPC field 20 mapping (W2-Q-WS7K)
- c3f894d41 feat(quarry-edge): read the broker grant's web-policy snapshot as the grant layer per step (W2-Q-WS7K)
- this status file, the MP ask and the depth rows (docs commit)

## Code only, not compiled
No cargo, node or go run (owner order 2026-11-03). The integration step compile-checks. Tests to run there:
- `cargo test -p quarry-runtime --features grpc grant_validator grpc::browser_broker`
- `cargo test -p quarry-edge --lib agent_policy_gate`
- `node --test scripts/tests/browser-grant-contract.test.mjs`

## Flag and default
No new flag. The plan says "none: absent snapshot means no grant layer", and no broker sends field 20 yet. The snapshot binds exactly when the claim does: `QUARRY_EDGE__WEB_POLICY_CLAIM_ENFORCEMENT` (default `observe`, so a denial is only counted), bounded by `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` (`off` on either skips the step layers). In any case nothing calls the new gate yet.

## Assumed decisions
- The snapshot's wire shape is the claim's. The broker copies the claim it verified, so the edge reads it with the claim's rules: unknown capability keys are ignored and counted (D2-053), never treated as unreadable. An unknown version, a bad rev or a bad mode fails closed.
- The snapshot follows the claim mode instead of a flag of its own. It is the claim, carried by the grant.
- The step layers are a second resolved policy that must also pass, rather than a re-resolution of the run's decision. Every layer only tightens, so this is the meet for every gate. The run's start decision and its egress boundary are unchanged.
- `v = 0` (an empty `WebPolicy{}`) is unreadable, not absent. "No snapshot" is the unset message (asked of MP).
- The L1 subset test allows only the named pending entries to be missing upstream. It does not weaken any other comparison.

## MP asks
Appended to `.plan-work/quarry-w1w2/MP-ASKS.md` under "W2-Q-WS7K":
- the upstream proto (`WebPolicy`, field 20 on `AcquireGrantResponse`, `ValidateGrantResponse` and `DeriveRunGrantResponse`, Go regen, L1 number lists, emptying `PENDING_UPSTREAM_*`)
- `authctx` `Principal.WebPolicy`
- `ClaimPolicyResolver` in `grant/policy.go`
- `Grant.WebPolicy` (derive only tightens)
- migration `0002_browser_grants_web_policy`, with `newTestDB` updated
- the six Go tests, and broker-first deploy

## Not done / not verified
- Not wired. `admission::validate_run_grant` drops the grant it revalidates, and `admit_action` has no step-caller claims. So `gate_policy_for_step` and `gate_visit_for_step` are not called from gate 8, step.rs, tabs.rs or attach.rs. Those files are outside the unit. This is the first depth row.
- The plan's HTTP-level `web_policy_agent` additions (`crates/quarry-edge/tests/web_policy_agent.rs`) are not written, because they need the wiring. The three plan-named cases are unit tests in agent_policy_gate.rs instead.
- The snapshot's deny list does not reach the browser egress boundary (depth row).
- Nothing compiled or run.
