# Browser planner rollout (WS11)

How the LLM browser planner is switched on, per environment and per org, and how
to switch it off. The code is in `rust/services/execution-core`
(`src/planner_policy.rs`, `src/browser_agent/*`, `src/tool_bridge/mod.rs`) and
the org flag is written by model-gateway (`browser_run.rs`, `org_quota.rs`).

Status at the time of writing: the mechanism is built and unit/hermetic tested;
nothing here has been run on a rebuilt image, and no success-rate number exists.
Every "measured" cell below is empty on purpose until a live (Tier C) run fills it.

## What decides whether a run gets the planner

1. Deployment mode, `QUARRY_BROWSER_AGENT_PLANNER_MODE` (default `off`):
   `off`, `allowlist`, `org_opt_in`, `default_on`.
2. For `allowlist`: `QUARRY_BROWSER_AGENT_LLM_ORGS`, a comma-separated list of org ids.
3. The org's flag, written into `tool_input.planner_flag` by model-gateway from the
   org-core quota row `browser_agent_llm_planner` (limit `1` = on, `2` = off, anything
   else = unset). The gateway reads it only when
   `MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS` is `on`; a client-supplied `planner_flag`
   is ignored. A read that fails becomes `unavailable` (fail closed).

Order of rules (first match wins): mode `off` with no legacy variable -> `mode_off`;
ZDR run -> `zdr`; org flag `off` -> `org_off`; legacy variable -> planner; then the mode:

| Mode | Org gets the planner when | Otherwise (reason) |
|---|---|---|
| `allowlist` | its id is in `QUARRY_BROWSER_AGENT_LLM_ORGS` | `not_allowlisted` |
| `org_opt_in` | flag is `on` | `org_not_opted_in`, or `org_policy_unavailable` if the flag could not be read |
| `default_on` | flag is not `off` | `org_policy_unavailable` if the flag could not be read |

A ZDR run never gets the planner. An org that switched the planner off never gets it.
When the planner is selected but no inference credential can be built, the run fails
with `browser planner unavailable: ...`; it never falls back to the deterministic loop.

The legacy variable `QUARRY_BROWSER_AGENT_LLM=1` still selects the planner for every
org (after the ZDR and org-off rules) and logs one deprecation warning per process.
It is removed at stage 4.

## Other switches (all default off or unchanged)

| Variable | Default | Effect |
|---|---|---|
| `QUARRY_BROWSER_AGENT_PROCEDURE` | `legacy/0` | `browse/1` swaps the planner's instruction paragraph for the sectioned procedure and adds a per-step run-state line. `legacy/0` is byte-identical to the old prompt. |
| `QUARRY_BROWSER_AGENT_WIDENED_ACTIONS` | `0` | `1`, `true` or `on` lets the planner propose `press`, `select`, `back`, `wait_for`. Off: the verbs are absent from the schemas and the procedure text, and one the model sends anyway is read as `observe`. |
| `QUARRY_BROWSER_AGENT_MAX_CONSECUTIVE_FAILURES` / `_MAX_TOTAL_FAILURES` | 3 / 8 | Failed steps that end a planner run. |
| `QUARRY_BROWSER_AGENT_{DEFAULT,CEILING}_MAX_{STEPS,RUNTIME_S}` | 20/100, 120/900 | Per-run bounds; a caller can only tighten. |
| `QUARRY_BROWSER_AGENT_MAX_PLANNER_TOKENS` / `_CEILING` | 200000 / 1000000 | Planner token budget per run (separate from Quarry spend). |
| `QUARRY_BROWSER_AGENT_DEFAULT_ON_ACK` | empty | Boot guard for `default_on`, see below. |

## Stages

| Stage | Mode | Prerequisites (all must be recorded green, with date and commit) |
|---|---|---|
| 1 | `allowlist`, dev Quarry only | S0 planner seam and hygiene, S1 budgets, S2 policy. No production exposure. |
| 2 | `org_opt_in` | Stage 1, plus S3 recoverable failures and Tier A corpus green, S4 widened actions (if the flag is to be on), production browser grants (WS1), Quarry run budgets enforced (WS6 S1 `on`), planner view hygiene (S0 or WS12 S4). |
| 3 | `default_on` | Stage 2 for one release on staging, plus secret redaction at capture (WS12 S1-S4), minted per-run budgets (WS6 S4), Tier B green on the release candidate, a live Tier C floor, the routing probe table, and decision X-11 (planner route pinned and guarded). |
| 4 | remove the legacy variable | One release at stage 3 without a rollback. Separate change: grep and update every runbook and both compose files for `QUARRY_BROWSER_AGENT_LLM`. |

Prerequisite status (fill in as each lands):

| Prerequisite | Status | Date | Commit |
|---|---|---|---|
| S0 seam and hygiene | built, hermetic tests green | | |
| S1 budgets and typed outcomes | built, hermetic tests green | | |
| S2 policy (execution-core half) | built, tests green | | |
| S2 org flag (model-gateway half) | built by the gateway lane | | |
| S3 recoverable failures, `browse/1`, Tier A | built, hermetic tests green | | |
| S4 widened actions | built behind a flag | | |
| Control Plane signs the quota-key convention | not done | | |
| WS1 production grants, WS6, WS12, Tier B, Tier C, X-11 | not done | | |

## Boot guard for `default_on`

`default_on` takes effect only when `QUARRY_BROWSER_AGENT_DEFAULT_ON_ACK` names all of
`grants,budgets,redaction,evals` (any order, any case, extra items allowed). Without
them execution-core logs one error and runs `org_opt_in` instead. The acknowledgement
is a ritual, not proof: the proof is the table above.

At startup execution-core logs one line, for example:

```
browser planner: mode=org_opt_in downgraded_from_default_on=false allowlist_size=0 legacy_all_orgs_env=false procedure=legacy/0 widened_actions=false steps=20/100 runtime_s=120/900 planner_tokens=200000/1000000 failures=3/8
```

It carries the mode in force, the size of the allowlist (never who is on it), the
procedure and the limits. It never carries org ids, credentials or URLs.

## Metrics (all labels from closed lists)

| Counter | Labels |
|---|---|
| `execution_browser_runs_total` | `planner` = `llm`, `deterministic`; `outcome` = `completed`, `failed`, `permission_denied`, `timed_out`, `resource_exhausted`, `cancelled`, `aborted`, `awaiting_approval` |
| `execution_browser_planner_tokens_total` | `direction` = `input`, `output` |
| `execution_browser_failures_total` | `kind` = `planner_unavailable`, `repeated_failure`, `no_progress`, `blocked`, `incomplete` |
| `execution_browser_deterministic_total` | `reason` = `mode_off`, `zdr`, `org_off`, `not_allowlisted`, `org_not_opted_in`, `org_policy_unavailable` |

A completed run whose org was kept off the planner for any reason other than
`mode_off` ends its output with ` usage=planner=deterministic reason=<reason>`.
A run that used the planner ends with ` usage=planner_calls=N planner_input_tokens=N planner_output_tokens=N`.

## Kill switch

Set `QUARRY_BROWSER_AGENT_PLANNER_MODE=off` (and unset `QUARRY_BROWSER_AGENT_LLM`) and
restart execution-core. A new run is deterministic; a run already in flight finishes
under its own limits. For one org, write the org's quota row `browser_agent_llm_planner`
to `2` (takes effect on the next run, no restart). To go back to the old prompt while
keeping the planner, set `QUARRY_BROWSER_AGENT_PROCEDURE=legacy/0`; to drop the widened
verbs, set `QUARRY_BROWSER_AGENT_WIDENED_ACTIONS=0`.

## Decisions and open items

- X-11 / OD13: the planner route stays on the Balance mode (`verevon-balance`) until
  Tier C numbers exist; pinning a model with a route guard is an open decision and gates
  stage 3 only.
- The org flag rides an org-core quota row as an interim carrier. Control Plane must sign
  the key convention (`browser_agent_llm_planner`: 1 on, 2 off) before production use; the
  row is removed when the final carrier (ADR-1) lands.
- Quarry does not yet validate `press` keys or destination-check `back`; execution-core's
  admission and the domain fence are what stand there until the Ingestion companion lands.
- Delegated bearers last about 300 s, so a run is effectively limited to about 240 s
  whatever `max_runtime_s` says.

## Measured numbers

None yet. Do not quote a success rate from this document.

| Measure | Value | Date | Run |
|---|---|---|---|
| Tier C task success, `legacy/0` | | | |
| Tier C task success, `browse/1` | | | |
| Connector-solvable routing compliance (S6 probe) | | | |
