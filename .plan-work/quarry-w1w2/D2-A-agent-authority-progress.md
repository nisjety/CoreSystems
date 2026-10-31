# D2-A-agent-authority: progress and hand-over (2026-10-28)

Unit: decisions 3.9 MP-GATEWAY-AUTH (D2-012) and 3.4 TAKEOVER-BUDGET of `DECISIONS-ROUND-2026-10-27.md`, accepted by the owner on 2026-10-28. Branch `quarryv2-next`. List A, so nothing here is behind a flag: the reload refusal is always on (like raw script); the counter is a plain metric.

## Done

1. Quarry refuses `AgentAction::Reload` when `derive_initiator` gives Agent. One new gate, 7b of `admit_action` (`admission::deny_agent_reload`), right after the raw-script gate and before the web policy, the budget and the one-shot approval, so a refusal consumes nothing. Every action entry point goes through `admit_action` (step, its `interact` alias, the browser socket's `action` message via `execute_ws_action`, and the attached delegate lane in `attach.rs`). Answer: 403 `FORBIDDEN`, `details {reason: "agent_reload_denied", denied_action: "reload"}`; on the socket an error frame with the same code and details. The agent is a service principal (whatever it states) or a person's token labelled `agent`; a person's own reload (no label or `human`) runs.
2. Gateway rule table (below), every rule matched to a Quarry check.
3. Doc comments reworded: gateway `browser.rs` (`CallerClass`, `ServicePrincipal`) and the four D2-012 comments in `actions/dispatchers.rs` now state the decision. `quarry-core` `AgentAction::Reload` doc says the edge enforces it.
4. Takeover counter: `quarry_agent_takeover_exempt_steps_total` and the histogram `quarry_agent_takeover_exempt_step_duration_seconds` (`_sum` is the seconds, `_count` equals the counter). Incremented where 7d78ab2e8 exempts a step: a person's step during a takeover on a run whose class is the agent's (`human_browse::takeover_exempted_step`, which is true only where the takeover rule is what spared the step), on the REST step and the socket path. No label (a run id is unbounded); the per-run view is the run's `human` receipts under their control epoch.
5. Notes: D2-012 closed by decision in `DEPTH-FINDINGS-W2.md` (residual accepted; the row's suggested fix rejected because it loosens the gates); D2-207 row, STATUS 7.5, `depth/D-D-B1-agent-policy.md` item 1 corrected (they said a takeover step spends the agent's budget; the code since 7d78ab2e8 says it does not); one paragraph added to `CONTRACTS.md`; `ERROR_REASONS.md` and `RUNBOOKS.md` (metrics table) appended.

## Gateway "agent may not X" rules and their Quarry checks

The gateway's agent actor is the body or query `actor` under a person's session (the SPA labels a model's action `agent`). `CallerClass::ServicePrincipal` is never produced. Rules found in `apps/Frontend Plane/verevonv3/apps/gateway/src/domains`:

| # | Gateway rule | Where | Quarry check | Status |
|---|---|---|---|---|
| 1 | an agent may not reload (403 `browser_action_denied`) | `browser.rs` `run_action`, `reload_refused_for_actor` | none before this unit | FIXED: `admission::deny_agent_reload`, `agent_reload_denied`, all entry points; `tests/agent_reload_denied.rs` |
| 2 | an agent step during a takeover is refused (409 `browser_human_takeover_active`) | `browser.rs` `run_action` | `admission::takeover_gate`, 409 `USER_TOOK_OVER` | exists, but refuses only with `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` on; the default is observe (logs `would_reject`). Depth finding 1 |
| 3 | an agent tab open, select or close during a takeover is refused (409) | `browser.rs` `refuse_agent_tab_call_during_takeover` and the tab routes | `admission::admit_tab_operation` -> `takeover_gate` | same flag, same finding |
| 4 | the socket's agent action during a takeover (`prepare_client_ws_message`) | `browser.rs`, `#[cfg(test)]` only; the production proxy forwards client frames unchanged | `execute_ws_action` -> `admit_action` -> `takeover_gate` | the gateway rule is not live code; Quarry's check is the only one and has the same flag. Depth finding 5 |
| 5 | raw script (`evaluate`) is refused for everyone | `browser.rs` `sanitize_action` | `admission::deny_raw_script`, `raw_script_denied`, always on | covered (WS2.A, `agent_evaluate_denied`) |
| 6 | an agent label can only tighten: the gateway sends `initiated_by: agent` for an agent actor and never a stronger claim | `browser.rs` `step_request_body`, `tab_call_query` | `derive_initiator` (a hint can only tighten; a service principal is always the agent), always on | covered |
| 7 | only a person changes the control mode | no gateway rule (the route forwards to Quarry) | `set_browser_session_control`: 403 `control_transition_requires_human`, always on | Quarry-only, covered |
| 8 | `browser_omnibox.search/suggest` and `knowledge.upload_files` are human-only on the actions surface | `actions/browser_omnibox.rs`, `dispatchers.rs` | not browser authority; the Model Plane's eligibility list (`model-eligibility.ts`) keeps the model off them | not a Quarry concern, no finding |

Result: every rule that changes what an agent may do has a Quarry check; the only one that had none (1, reload) is fixed. Rules 2 to 4 have a check that is observe-only by default.

## Tests run (all via qtest unless said)

- `cargo test -p quarry-edge --test agent_reload_denied --test takeover_exempt_counter --test agent_evaluate_denied --test web_policy_agent --test metrics_recorder`: 4 + 1 + 6 + 7 + 9 passed, 0 failed.
- `cargo test -p quarry-edge --test log_field_discipline --test agent_run_control`: 35 + 22 passed (the new warn line passes the log lint).
- `cargo test -p quarry-edge --lib` (includes the `agent_routes` lib tests, the two new admission tests and the `human_browse` unit test): 627 passed, 0 failed.
- `cargo test -p quarry-core --lib`: 245 passed.
- `cargo check --workspace --all-targets`: Finished, no errors (warnings in `agent_routes/tests.rs` about an unused `MutexGuard` at lines ~2503 and ~2516 are not from this unit).
- Gateway, native: `cargo test --all-targets` in `apps/Frontend Plane/verevonv3/apps/gateway`: 830 passed 0 failed 1 ignored (unit), plus 1, 13, 3, 1 in its four integration binaries.
- No test was changed to fit the decision; `agent_evaluate_denied` and the other named tests pass untouched. One harness addition: `RecordingDriver` now implements `reload` (counts `reload_calls`), because the default driver answered Unsupported and a person's reload could not otherwise be shown to run.

## Not run

No live stack, no Docker compose, no frontend (SPA) tests (nothing in the SPA changed). The delegate lane (attached agent) reload refusal is covered through `admit_action` unit tests, not a wire test with a live attachment.
