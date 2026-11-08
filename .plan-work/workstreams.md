# Workstream catalogue (single source of truth for ids)

Detailed designs are required for every item below (all Priority items plus waves W1 and W2). Later waves (W3 non-priority, W4, W5) are epics only and are not in this file.
Item ids are the audit's F-t1-nnn. "Planes" are the owning planes per the audit, to be corrected by the code. "Wave" is the audit's wave.

| WS | Title | Items | Wave | Main planes / services |
|---|---|---|---|---|
| WS01 | Human posts, reliable send, thread pane | F-t1-001, F-t1-033 | W1 | Model (session-core AppendMessage, model-gateway), Control (user-core thread:append), Frontend (gateway spaces.rs, SPA SpaceRoomComposer, AuxPane, `?thread_id=` deep link) |
| WS02 | Space realtime substrate | F-t1-003 | W1 | Model (session-core outbox/events), Application (projection, if any), Frontend (gateway SSE, SPA client), Control (new stream-read audience) |
| WS03 | Space governance admin, phase 1 (effect-policy admin, typed denials) | F-t1-006 (phase 1 firm; phases 2-3 epic) | W1 | Control (user-core effect policy route, reason codes, audit), Frontend (admin UI, gateway route), Model (posture caps, later) |
| WS04 | Room administration, membership writer, directory | F-t1-022, F-t1-048, F-t1-049 | W1/W2 | Control (user-core roles/lifecycle), Application (convex spaces, spaceMembers, crons, org-room sync), Frontend (Members tab) |
| WS05 | Security fixes and enterprise identity | F-t1-031, F-t1-023 | W1 (fixes) / W2 | Control (audit-core read gating, org-core SCIM/SSO/domain verification, auth-core), Ingestion (integration-corev2 SCIM stub to 501) |
| WS06 | Model Plane safety hardening (ZDR, sandbox caps) | F-t1-005, F-t1-021 | W1/W2 | Model (session-core, model-gateway, execution-core, sandbox-manager), Ingestion (Quarry egress route), Control (egress decision) |
| WS07 | Frontend quick wins and agent artifacts in the room | F-t1-024, F-t1-030, F-t1-063, F-t1-014 | W1 | Frontend (App.tsx boundaries, nginx, version.json, gateway client-config; SpaceRoomTimeline; viewer registry; artifacts panel), Model (session-core room artifact aggregate read) |
| WS08 | Addressed agent wake-up and identity-bound mentions | F-t1-002, F-t1-007 | W2 | Frontend (composer, gateway apply_mention_binding_policy), Application (binding schema, mention producer), Model (post route), Control (turn_origin) |
| WS09 | Agent work visible to the whole room, run queue, steer | F-t1-004, F-t1-040 | W2 | Model (session-core stream_run_events, model-gateway SSE, Control run-read decision), Frontend (gateway attach route, cards, strip, rows) |
| WS10 | Approvals: scopes, durable resume, quarantine | F-t1-010 | W2 | Model (session-core approval_delivery, execution-core worker, model-gateway approvals.rs), Control (grants, ceilings, turn_origin), Frontend (cards, policy editor) |
| WS11 | Notifications, cross-room attention, read state | F-t1-008, F-t1-009, F-t1-042 | W2 | Application (notification-core, convex spaceReadMarkers, attention projection), Model (producers, latest-activity summary), Frontend (Home inbox, sidebar), Control |
| WS12 | Direct messages, message lifecycle, reactions | F-t1-016, F-t1-017 | W2 | Control (new Space kinds, participant_hash), Model (session-core revision entries), Application (reactions table), Frontend (toolbar, DM sidebar) |
| WS13 | Message search, permalinks, command layer | F-t1-018, F-t1-039, F-t1-041 | W2 | Model (session-core index/RPC), Frontend (gateway route, Ctrl+F search, entity router, palette/slash commands), Control (read decision reuse) |
| WS14 | Attachments, file library, artifact sharing grants | F-t1-013, F-t1-057 | W2 | Data (documents-api-go, retrieval-engine), Ingestion (imports-core), Control (shared import validator, resource_grants write surface), Frontend (composer, Files view) |
| WS15 | Composer for real work, scalable timeline, typed entries | F-t1-032, F-t1-034, F-t1-052 | W2 | Frontend (composer, virtualised timeline), Model (server windows, cursors, typed entries), Application, Ingestion (link previews via Quarry), Control (system-append class) |
| WS16 | UI quality baseline and sidebar organisation | F-t1-062, F-t1-055 | W2 | Frontend (i18n, preferences, announcer, sidebar), Application (per-user prefs), Control (user profile) |
| WS17 | Personal agents and audience-safe agent memory | F-t1-011, F-t1-012 | W3 (Priority) | Control (owner-derived admission, access list, declassify decision), Model (session-core memory, letta-bridge, labels), Data (retrieval entitlement), Application (binding schema), Frontend (private panel) |
| WS18 | Agent self-API on Space actions | F-t1-019 | W3 (Priority) | Model (capability-core, execution-core adapters), Control (run-action decisions), Frontend (action registry) |
| WS19 | Room canvas | F-t1-015 | W3 (Priority) | Data (wiki-store-go versions), Control (shared-write decision), Frontend (Canvas tab), Application (pointer), Model (agent tool) |
| WS20 | Room-scoped agent browser | F-t1-020 | W3 (Priority) | Ingestion (Quarry live view, challenge signal), Model (space_ref on browser runs, hand-off), Control (room decision), Frontend (card, modal) |

## Cross-session touchpoints per workstream (flag, do not plan the sibling's scope)
- Session Core, thread/run authority: WS01, WS02, WS09, WS12, WS13, WS15 (sibling b).
- Approvals and resume: WS10, WS18, WS20 (sibling b; sibling a for browser resume).
- ZDR: WS06 and every WS that persists or pushes content (siblings a and b).
- Agent-work visibility: WS09 (sibling b: run stream/AG-UI; sibling a: browser frames).
- Agent memory: WS17 (sibling b).
- BrowserBroker grants, agent browser path, ZDR-safe telemetry: WS20, WS06, WS02 (siblings a and b).

## Output templates

### Haiku sweep output (`.plan-work/haiku/<id>.md`)
1. `## Citation check`: a table with columns `audit item | cited path:line (as in audit) | state (COMMITTED / MODIFIED-UNCOMMITTED / UNCOMMITTED-NEW / MISSING) | verified path:line in live tree | what is there (one line)`. One row per path the item cards (and Appendix D rows) cite, plus every path:line named in the matching `verify/t1-vNN.verdicts.json` entries.
2. `## Affected files not cited by the audit`: files you found by grep that a change would have to touch (callers, DTO mirrors, TS types, proto, tests, i18n, compose, CI). Path, one-line role, state.
3. `## Existing tests`: test files and the exact command that runs them.
4. `## Migrations and contracts`: migration directories involved, highest existing number per directory (state whether the highest ones are untracked), proto files, OpenAPI/JSON schema, event subjects.
5. `## Owners and entry points`: service entry point, route registration site, config/env/flag sites.
6. `## Surprises`: anything contradicting the audit (missing file, drifted line, item already partly built, work in the uncommitted tree that overlaps).

### Sonnet design output (`.plan-work/design/<WS>.md`)
Fixed headings, in this order:
1. `# WSnn <title>`; `## Items resolved` (F-t1 ids with the audit tier/effort/status)
2. `## Verified current state` (what exists, with live-tree path:line and COMMITTED/MODIFIED/UNCOMMITTED-NEW tags; what the audit got wrong)
3. `## Design` (architecture for this workstream: data flow, owner plane and service per piece and why, exact API routes with request/response shapes and error codes, proto/RPC changes, Convex tables/mutations, Control decision types, event subjects and envelopes, data model and migration SQL sketches with the next free migration numbers, ZDR behaviour, authority checks at each hop, what is explicitly NOT built)
4. `## Slices` (PR-sized slices in build order; each: id `WSnn-Sk`, goal, size S/M/L, plane, depends on, flag)
5. `## File-level change list` (table per slice: path, change, new/modified, state of the file today)
6. `## Feature flags and rollout` (flag name, owner plane, default, rollout order, kill switch, ZDR rules)
7. `## Tests` (unit, integration, e2e; file paths to add; exact commands)
8. `## Definition of done and evidence` (including the repo rule: rebuilt local images, running stack, verified end-to-end flow; list the concrete flow to run and what to capture)
9. `## Effort, dependencies, risks` (S/M/L/XL plus an engineer-week range per slice, hard dependencies on other WS slices, risks with mitigation)
10. `## Decisions needed` (ADR ids from audit 9.3 or new; options in one line each; what can proceed without the decision)
11. `## Conflict hotspots` (the files that other workstreams will also edit, so sequencing can avoid merge conflicts)
12. `## Cross-session touchpoints` (pieces shared with sibling sessions a and b, who should own each, what contract is assumed)
13. `## Open questions for the user`
Length: as long as needed to be implementable, but no filler; target 2,500 to 6,000 words.
