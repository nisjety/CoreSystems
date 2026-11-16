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

## Round 2 (2026-11-16, post-compile-pass)

| # | Topic | Decision | Work |
|---|---|---|---|
| D16 | Browser grant revoke (was D5 open item) | Revoke immediately: give model-gateway a broker service account with `browser:write` | Service-account change (principal train) + model-gateway revoke call at run end |
| D17 | D9 old-notes Space-kind lookup | Option (a): Control signs the answer, gateway relays it to session-core | Control issuer for "is this space the caller's personal space"; gateway relay route; session-core RestoreMemory uses it instead of refusing unknown-kind threads |
| D18 | D8 room history for new members | Confirmed as described: default sees past conversation; invite option hides it; admin can later reveal, optionally from a date; canvas follows the same rule | New feature: Control per-member history visibility (all / since-join / since-date), invite option, admin control; session-core + canvas honour it; SPA invite + admin UI |
| D19 | Ownerless org agent admin | Yes: org admins get a management screen (edit/suspend/retire) | Control admin routes for org-visible ownerless agents; gateway + SPA admin UI |
| D20 | ZDR deletes (skills/plugins/schedules) | Registry wins: allow deletes under ZDR (deletion removes content, does not persist it) | Revert the W3-DEC-ZDR2 refusal guards on the DELETE routes for skills/plugins/schedules; keep guards on create/update/publish/restore/import; fix registry rows to match (delete rows stay/become allowed, non-delete rows stay refuse) |
