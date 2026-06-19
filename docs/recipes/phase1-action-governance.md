# Phase 1 action governance — review_ai_action / privacy.erase_account / monitoring.check_url

The three Phase-1 actions and their governance classification. Each is dispatched
**today** through its dedicated, session-scoped human-UI client — not through the
generic `executeAction` → `/api/v1/actions/execute` registry path — for the safety
and honesty reasons noted below.

| Action | Risk | Requires approval | Reversible | Live dispatch today | Gateway route |
|---|---|---|---|---|---|
| `review_ai_action` | medium | no | yes (decision can be re-reviewed) | `inbox-client.reviewAiAction` → `POST /api/v1/inbox/ai-actions/{id}/{approve\|reject}` | `inbox.rs` (B2) |
| `monitoring.check_url` | low | no | yes (read-only check) | `monitoring-client.checkUrlNow` → `POST /api/v1/monitoring/check` | `monitoring.rs` (C2) |
| `privacy.erase_account` | **high** | **yes (typed-confirm + step-up re-auth)** | **no (irreversible)** | `privacy-client.eraseMyAccount` → `DELETE /api/v1/privacy/erase` (gated in `PrivacyDataSection`) | `privacy.rs` (D1) |

## Why these dispatch via dedicated clients, not the action registry

`agent-tools` exposes **every** entry in `actionRegistry` to the model as an
executable tool (it does not filter by `LIVE_ACTIONS`). Adding these three to the
registry **without** a matching `/api/v1/actions/execute` dispatcher arm would
advertise an agent tool that errors on call — a fake affordance. And wiring a
dispatcher arm for `privacy.erase_account` would make an **irreversible** erase
agent-auto-invokable, bypassing the typed-confirm + step-up re-auth gate the D2 UI
enforces — unacceptable.

The honest, safe state today:
- The human UIs (B3 review panel, C3 Monitoring tab, D2 Privacy & data) dispatch
  these via their dedicated clients, which are live and verified.
- No fake agent affordance is shipped: the actions are **not** advertised as
  agent-executable tools they cannot safely run.

## Deferred to Phase 2 (agent-executable dispatch)

Exposing `review_ai_action` and `monitoring.check_url` as governed **agent** tools
requires, together:
1. `/api/v1/actions/execute` dispatcher arms in the gateway `actions` domain
   forwarding to the dedicated endpoints; add the two ids to `LIVE_ACTIONS`.
2. An `agent-tools` change so only `LIVE_ACTIONS` entries are advertised as
   executable (closing the advertise-but-cannot-run gap generally).
3. `action-registry.test` + `agent-tools.test` coverage for the flags above.

`privacy.erase_account` must **never** be added to `LIVE_ACTIONS` / agent-executable
dispatch — erasure stays human-UI-only behind typed-confirm + re-auth.
