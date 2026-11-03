# L-SC2a-authority (live dev stack)

- Leg: L-SC2a-authority (raw script denial, agent reload denial, takeover enforcement, takeover counters).
- Time window UTC: 2026-10-28 20:10 to 20:16.
- Code that runs: main + integration 7c082825a (live checkout, quarry-edge image rebuilt 19:45 UTC). Edge http://127.0.0.1:8082.
- Identity: the dev stub only (`Bearer dev-evidence` becomes org_placeholder / user_placeholder, principal user). No service token and no real Auth Core token was available.
- Runs used (all closed, each close answered `closed:true`): run_01M6225XPQ1WSAJ167934RSR06, run_01M6227E5Y9W3AKP0XMQV9N2WV, run_01M622957W0NM57S8VFKKJ4YVF, run_01M622AS2ZEQCT61SD1S1C2EQ0, run_01M622BMZTX9PDB8QAGS348KNP.
- Flags changed and reverted: see flags-log.md (TAKEOVER_ENFORCEMENT, METRICS_ENABLED). At the end live-flags.env is empty, the edge was recreated (healthy), and `live.sh flags` is identical to the baseline (L-SC2a-authority-flags-end.txt, same as L-SC7-billing-flags-end.txt).
- Principals tested: person (dev stub), with the `initiated_by` label unset, `human` and `agent`. Not tested: a service principal (execution-core / Model Plane). It needs a real service JWT, which the dev stub cannot produce.

## Checks

| id | what | result | evidence |
|---|---|---|---|
| E1 | Evaluate on REST step, no label (person) | PASS | `POST /v1/agent/runs/{run}/step {"action":{"type":"evaluate","script":"..."}}` gave 403, `error.code=FORBIDDEN`, `details={denied_action:evaluate, reason:raw_script_denied}`. The body does not echo the script. |
| E2 | Evaluate, step with `initiated_by` human and agent | PASS | both 403 raw_script_denied (no human exception) |
| E3 | Evaluate on the interact alias | PASS | `POST .../interact {"action":{evaluate}}` gave 403 raw_script_denied |
| E4 | Evaluate on the browser socket | PASS | `GET .../frames/ws` (node client, 101), text frame `{"type":"action","action":{"type":"evaluate",...}}` gave error frame `{"code":"FORBIDDEN","details":{"denied_action":"evaluate","reason":"raw_script_denied"},"type":"error"}` (sc2a-ws-evaluate2.log). The run needs a page first (see Depth 1). |
| E5 | A denial leaves nothing behind | PASS | receipts=2, timeline=3, session step=1 before and after all four denials; the next typed `wait` ran (step 2, receipts 3). |
| E6 | Log: one warn line per refusal, script not in log | PASS | `docker logs quarry-edge`: 5 lines "raw page script execution denied" for run 1 (step x3, interact x1, socket x1); 0 lines contain `pwned-by-evaluate`, `evil.invalid` or `document.cookie`. |
| E7 | Evaluate for a service principal | NOT-RUN | no service token on the dev stub. The wire test `a_service_principal_gets_the_same_refusal_on_both_routes` covers it. |
| E8 | SC2a overall (all principals) | PARTIAL | person only: PASS on all three surfaces. Service principal NOT-RUN. The runbook says "every principal", so the stub is not enough. |
| R1 | Agent reload on REST step (`initiated_by:agent`) | PASS | 403 FORBIDDEN `details={denied_action:reload, reason:agent_reload_denied}`, message "reload is available to people, not to agents; use navigate" |
| R2 | Agent reload on interact | PASS | same 403 agent_reload_denied |
| R3 | Agent reload on the socket (`"actor":"agent"`) | PASS | error frame FORBIDDEN agent_reload_denied (sc2a-ws-reload-agent.log) |
| R4 | Denied reloads consume nothing | PASS | receipts=3, timeline=4, step=2 before and after the three denials |
| R5 | Human reload allowed (label `human`, no label, interact, socket with no actor) | PASS | all four answered 200 / observation; receipts show 4 completed `reload` steps, `initiated_by:human` |
| R6 | MP-GATEWAY-AUTH: service principal reload through the Model Plane path | PARTIAL | A person token labelled `agent` is refused (R1 to R3): that is the label branch of `derive_initiator` (`run_live.rs:84`). The service-principal branch (`claims.is_service()`) needs a service token: NOT-RUN. The Model Plane does not drive this edge in this stack (MODEL_PLANE_URL empty, no BrowserBroker validator). |
| T1 | Takeover, enforcement=1: agent-labelled step during human takeover | PASS | start run (initiated_by agent), navigate, `POST .../browser-session/control {"mode":"human_takeover"}` (200, epoch 1), then step with `initiated_by:agent` gave 409 `USER_TOOK_OVER`, `details={control_mode:human_takeover, control_epoch:1, taken_over_at}` |
| T2 | Same on interact, socket and tab create | PASS | interact 409; socket error frame USER_TOOK_OVER; `POST .../tabs {"initiated_by":"agent"}` 409 |
| T3 | One skipped receipt for the refusal | PASS | receipts: `{step 1, wait, skipped, by agent, epoch 1}` (one line after the first refusal; later refusals add none) |
| T4 | Give back, then agent step | PASS | `control {"mode":"agent_control"}` 200 (epoch 2), then agent step 200; receipt `completed by agent epoch 2` |
| T5 | Person steps during the takeover on the same run | PASS | two person steps 200 while enforcement was on |
| T6 | Enforcement unset (observe): same agent step | PASS | step 200, interact 200, socket observation; `would_reject: agent request during human takeover (enforcement is off)` warn x3 (step, interact, socket), each with run_id and control_epoch 1 |
| T7 | Observe mode counter | PARTIAL | There is no dedicated would_reject counter in the code (`src/metrics.rs` has only the exempt-step counter and `quarry_agent_steps_total` in this area). What moved: `quarry_agent_steps_total{action_class="wait",outcome="ok"}` 3 (agent steps ran during takeover), and the run's budget ledger counted them (close: steps 4). `quarry_agent_takeover_exempt_steps_total` did not move for agent steps. See Depth 2. |
| T8 | Revert to unset | PASS | live-flags.env empty, edge recreated, `live.sh flags` shows no TAKEOVER_ENFORCEMENT, `docker exec quarry-edge env` shows none. |
| B1 | Counter moves for person steps during takeover (REST) | PASS | enforce run: `quarry_agent_takeover_exempt_steps_total 2`, `..._duration_seconds_count 2`, `_sum 0.0925`. Second run (observe, counters reset by the restart): 2 after two REST steps, `_sum 0.1126`. |
| B2 | Counter moves on the socket | PASS | third (socket) person step: total 3, count 3, `_sum` 0.1126 to 0.1684 |
| B3 | No move outside a takeover | PASS | person step before the takeover: counter series not present (0). Person step after the hand-back: stays 3, count 3. |
| B4 | Takeover steps do not count against the budget | PASS | close usage of the last run: `steps:3` = navigate + one person step before the takeover + one after the hand-back; the 3 takeover steps are not in the ledger. Enforce run: `steps:2` = navigate + the agent step after the hand-back, with 2 exempt person steps. |
| M1 | Does the run route call the Model Plane browser-broker? | PASS (nothing called) | No broker or model-plane call. The only matching edge log line is the start-up warn "no BrowserBroker validator is configured: browser grants are not enforced (not production)". The browser is quarry-edge's own Chromium (`execution_engine:chromium`). |

## Compared with qv2e

No earlier result of this leg exists (the qv2e attempt only left a run id in sc2a-run1.txt). Nothing to compare.

## Depth

1. Socket on a run with no page: `GET .../frames/ws` on a run that has not navigated sends one error frame `DRIVER_FAILED` (reason `no_current_page`) and closes the loop, so an action message sent right after connect never reaches admission. This is not a defect in the denial. The frame ticker in `ws.rs` `browser_ws_loop` calls `live_frame` first and breaks on error. A tester who sees DRIVER_FAILED instead of FORBIDDEN on a fresh run should navigate first. Evidence: sc2a-ws-evaluate.log (page-less, DRIVER_FAILED) against sc2a-ws-evaluate2.log (after navigate, FORBIDDEN). A page-less REST evaluate on the same kind of run is 403 raw_script_denied (policy runs before the driver).
2. The task text expected "a would_reject warn plus a counter" in observe mode. The code has the warn (`admission.rs:342`) but no counter for it. If the owner wants to read how often enforcement would have fired before flipping (the runbook says the flip is after WS11 maps the refusal), the only source today is the warn log line. Likely fix: a counter next to the warn in `takeover_gate`.
3. A typed step on a page-less run (`wait` before any navigate, REST) answered 502 DRIVER_FAILED, left a `failed` receipt and counted one attempt (run 1 close: attempts 7 for 6 steps). This is expected. It is noted because it shows in the receipts list.
4. On an agent-class run, the person's first `navigate` (before any takeover) counts against the budget as a person step (class agent, ledger steps 1). Expected per TAKEOVER-BUDGET.

## Not run

- Service principal (execution-core / Model Plane) cases for E7 and R6: they need a real service token; the dev stub gives only the user principal. Covered by the wire tests `agent_evaluate_denied.rs` and `agent_reload_denied.rs`.
- A real signed-in user token: not needed for the checks above. The labelled-token legs are the lead's with the user.
