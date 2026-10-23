# U9 - rebuilt-edge evidence, round 4, on the committed HEAD (2026-10-20 evening)

(The earlier run of this unit, on 31441f12 and a working-tree image, is kept as wf-U9-evidence.round3-old.md.)

Image `qv2v/quarry-edge:u9r4` built from HEAD `ddbd4b2c` (Quarry-v2 tree clean, so the build context is the tree itself): `docker build --build-arg SOURCE_REVISION=u9r4-ddbd4b2c -f deploy/compose/Dockerfile.edge .`, rc 0, `/version` `u9r4-ddbd4b2c`. Private network `qv2e-net` 11.250.0.0/24, `qv2e-edge` (127.0.0.1:38090, 1 GiB, dev bypass, `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=on`, log `takeover_enforced=true`), `qv2e-www` (python http.server, 11.250.0.10, page.html). Scripts and raw output: `wf-U9-evidence-scripts-r4/`.

| # | Check | Result | What was seen |
|---|---|---|---|
| 1 | Tab initiator over HTTP | PASS | Agent open with body `initiated_by`: 409 USER_TOOK_OVER, details {control_epoch 1, control_mode human_takeover}; same with `initiatedBy`: 409; agent select `?initiated_by=agent` 409, close `?initiatedBy=agent` 409, step 409; the person's own open (no hint; `initiated_by: human`) 200 and own select 200; before the take-over the agent opens were 200; both spellings in one body: plain-text 422 `duplicate field initiatedBy`. `GET /tabs`: no opener (documented). Owner projection `tabs[].openedBy`: `{initiatedBy: human}` / `{initiatedBy: agent, agent: {basis: user_session, aiRunId run_abc, label planner-1}}` (snake_case advisory) / run_def, planner-2 (camelCase). Timeline: tab and action items carry `initiatedBy` and `agent` (step in the take-over: `agent {basis: user_session}`, epoch 1); receipts `initiated_by: agent` + `agent`. No principalId (person token). `would_reject` 0. |
| 1b | service_principal identity, Postgres column `initiated_agent` | NOT-RUN | The dev bypass has one person identity; no database in this edge. |
| 2 | /ready | PASS | 200 with browser, scrape_browser, durable:true, status ready; before a run both `idle`, slot status `starting`; with one run open browser `healthy`, sessions_open 1, slot `ready`, healthy true, pages 5. |
| 3 | Live Chromium lane (CI job quarry-browser-live) | PASS | Chromium 155.0.8059.79 in quarry-rust-env:1, tree at ddbd4b2c, one run: quarry-browser 138 passed 4 ignored + tests/ssrf.rs 4 passed 1 ignored; browser_egress_boundary 5 passed; zdr_behavior 4 passed; `--ignored --list` 5 names; all rc 0. |
| 4 | U5 WS6.E | PARTIAL | PASS: `QUARRY_EDGE__GRANT_BUDGET_CLAIM=bogus` -> `Error: budget: QUARRY_EDGE__GRANT_BUDGET_CLAIM must be ignore or enforce, got "bogus"`, exit 1; `enforce` boots, log `grant budget claim mode=enforce`. NOT-RUN: a claim on a broker grant (by: grant / limit_source: grant): the edge has no broker standalone. |

Cleanup: containers, network, volume and the image tag removed; docker ps -a / network ls / volume ls / images show no qv2e or u9. Live stack untouched.
Commit: e7c321d5 (RUNBOOKS.md append only).
Traps: none new; 4th run of the recipe, no docker trouble this time (docker 29.7.2).
