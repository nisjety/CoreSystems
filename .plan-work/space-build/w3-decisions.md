# Space W3 owner decisions (2026-11-16)

| # | Topic | Decision | Work |
|---|---|---|---|
| D1 | Shared memory when sharer removed/demoted | Auto-retract all their shared copies in that room | Control removal/demotion hook -> session-core retract (WS17-S7 path) |
| D2 | Canvas tools for agents | Own per-agent switch, separate from rename/pin | Binding field + gateway route + SPA toggle; model-gateway/execution-core offer canvas rows only under the canvas switch |
| D3 | Agent card posting | Enable now | capability-core migration flipping space.thread.post_card admission to enabled; WS18-S6 toggle copy mentions cards |
| D4 | Browser start from a new thread | Change Control: browser decision without an existing thread; the run creates the thread | Control browser-run decision accepts no thread; gateway R1 + model-gateway start create the thread |
| D5 | Parent browser grant | Narrow to the run's hosts AND revoke at run end | gateway browser.rs parent grant host-limited; revoke on terminal |
| D6 | Run count / ResolveRunSpace auth | Keep org-level (no change) | none |
| D7 | `browser` in agent_computer preset | Explicit-only: remove from preset | user-core org_policy_presets.go + explicitOnly list |
| D8 | Room history for new members (NEW FEATURE) | Default: new members SEE past conversation. Invite option "don't share past conversation": member joins but past is hidden. Room admin can later change it to show past, with a date filter (show from date X). Canvas follows the same rule (no blanket 403). | Control: per-member history visibility (all / since join / since date) set at invite and editable by admin; session-core + canvas honour it; SPA invite option + admin control |
| D9 | Old personal notes hidden by backfill | Hide from agents, owner sees them and can restore | session-core restore path + memory panel control |
| D10 | Org agents ownership | Org-owned, no owner (new Control path); leaving users don't suspend shared agents | user-core register org agent ownerless; gateway bind/create uses it; owner sweep skips |
| D11 | OWNER_LIST_SPACE_REFUSE default true | Ship as release item | release note only |
| D12 | Per-message personal memory opt-in permission | Own entitlement bit | user-core migration + catalogue row; WS17-S8 decision uses it |
| D13 | MODEL_ACTION_ADMISSION env | Each core controls its own .env; do not add to compose | runbook note only |
| D14 | Older routes missing ZDR (POST /api/v1/spaces/{ref}/agents, PUT /api/v1/agents/{id}/draft) | Fix now | gateway ZDR guards + registry rows |
| D15 | Registry scope | Extend registry to Control internal decision routes and Data-plane (wiki-store) routes, with lints | contract JSON + lint |
| Next | | Code decisions on space-w3, then compile pass (cargo check / go build / tsc) and fix compile errors. Still no tests. | |
