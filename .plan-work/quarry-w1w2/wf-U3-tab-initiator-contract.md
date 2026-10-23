# What a caller must send so Quarry can tell which agent opened a tab (for the Model Plane owner)

Unit U3-tab-initiator, 2026-10-19. Read this before wiring browser tab calls from execution-core or model-gateway.

## Finding: nothing in the Model Plane calls the tab routes today
`apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs` and `browser_agent.rs` call only
`POST /v1/agent/runs`, `POST /v1/agent/runs/{id}/step` and `DELETE /v1/agent/runs/{id}`. grep for `/tabs`,
`browser_tab`, `initiated_by` and `initiatedBy` under `apps/Model Plane/rust` finds nothing. The only senders of
`actor: "agent"` for tab calls are the gateway's action surface (`browser_tab.create|select|close` in
`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs`), which the model reaches with the
PERSON's session, so Quarry sees a person's token plus the fixed hint `agent`. I did not edit any Model Plane file.

## What Quarry now derives and records (never from the request)
| Token | Hint | Class | agent.basis | agent.principalId |
|---|---|---|---|---|
| person | none or `human` | human | (no agent) | |
| person | `agent` | agent | `user_session` | none (the token proves the person, not the agent) |
| service principal with `browser:execute` | none, `human` or `agent` | agent | `service_principal` | the token's signed service id (for example `service:execution-core`) |
| service principal without `browser:execute` | any | refused 403 before the handler | | |

A person's token can never loosen the take-over rule (the hint only tightens), and a service principal cannot claim human.

## If execution-core starts calling the tab routes with its own service token (recommended path)
Send nothing special: the verified service id is recorded as `agent.principalId`. To also say which AI run it acts for,
add the ADVISORY fields (recorded, never trusted, dropped unless `[A-Za-z0-9._:-]`, 1 to 64 chars):
- open tab: JSON body `{ "url": ..., "initiatedBy": "agent", "aiRunId": "<run id>", "agentLabel": "<short name>" }`
  (`initiated_by`, `ai_run_id`, `agent_label` are accepted too; both spellings of one field in one body is a 422)
- select / close (no body): query `?initiatedBy=agent&aiRunId=<run id>&agentLabel=<short name>`
- step: the same two extra body fields next to `initiated_by`.
Use the service token of the run owner: a run is owned by whoever started it, so a service-started run is driven by that service.

## If the call goes through the Verevon gateway on a person's session (today's path)
Set the headers `X-Agent-Run-Id` and `X-Agent-Label` on the gateway request (the action surface forwards the caller's
headers to the browser handlers). The gateway forwards them as `aiRunId` / `agentLabel` only when `actor` is `agent`.
For `browser_tab.*` through the action surface the input must still carry `actor: "human" | "agent"` (required since round 3).
There is no ai run id in the gateway's own state (its BrowserRunStore is test-only), so these headers are the only source.

## Where it shows up
- timeline items (`GET .../browser-session/timeline`): `agent` next to `initiatedBy` on `tab` and `action` items
- step receipts (`GET .../receipts`): `agent` next to `initiated_by` (column `initiated_agent`, migration 0011)
- owner projection tabs (`GET .../browser-session`): `openedBy: { initiatedBy, agent? }` for tabs opened through the tab routes
- ZDR runs write no timeline or receipt rows; `openedBy` stays in memory only (ids and classes, no URL or title).
