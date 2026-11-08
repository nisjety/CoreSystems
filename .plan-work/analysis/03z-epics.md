# 03z Later waves as epics (section 3, part z)

Scope: the 56 audit items with no detailed design (audit Wave 3 and 4 non-priority items, all of Wave 5, plus items 036 and 047 which belong to no workstream) grouped into 12 epics E01 to E12. Residual halves of workstream items (F-t1-006 phases 2-3, F-t1-021 egress, F-t1-023 linked identities, F-t1-031 exports) are claimed by an epic and flagged in the index. Effort is engineer-weeks, converted from the audit's S/M/L/XL (about 1 / 3 / 6 / 12) and corrected by the verified state; all ranges are low-confidence (plus or minus 40 to 50 percent) because no epic has been sliced below its first slice.

Evidence base and limits. Designs existed only for WS01, WS02, WS03, WS04, WS09 and WS10 when this was written; prerequisites on WS05 to WS08 and WS11 to WS20 are cited at workstream level from `workstreams.md`, not at slice level. Code spot-checks were done in the live tree `C:/dev/CoresSystem` (states below: C = COMMITTED, M = MODIFIED-UNCOMMITTED, N = UNCOMMITTED-NEW).

Decisions taken here where sources disagree:
- The haiku live-tree delta says "the S4.3 watch primitive has no code in the live tree" (`haiku/live-tree-delta.md:338`). Wrong: `apps/Model Plane/go/services/capability-core/internal/watch/` has 14 tracked files (C), commits 690f91ae, 6316becb, d45e0269 (S4.3 steps 1 to 3); it stops at `recorded` (`watch.go:8`) and delivery waits for S4.4. E03 and E08 build on it.
- Audit puts F-t1-025 in W4 and F-t1-027 in W3; this plan keeps that order and makes cost attribution (E01-S1) the first thing any unattended or multi-agent epic needs.
- Audit cards cite `spaces.rs` lines that drifted: `apply_mention_binding_policy` is at `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:4500` (audit 4399-4482), `inject_authored_instructions` at `:4291` (audit 4225-4300), `watches_not_built` at `:1627` (matches).

## Start order and triggers (summary)

| Order | Epic | Earliest start | Effort (eng-wk) |
|---|---|---|---|
| 1 | E01 Agent operations and multi-agent rooms | cost slice any time after Wave 1; multi-agent after WS08, WS10-S3 | 26 to 42 |
| 2 | E08 Push, PWA, DND, digest, catch-up | attention slice after WS11; Web Push after the S4.4 ledger | 18 to 30 |
| 3 | E03 Room automations and webhooks | after E01-S1 and WS10-S3 | 16 to 25 |
| 4 | E04 Agent tools, forms, history answers, file knowledge | after WS13, WS14, WS15 | 20 to 32 |
| 5 | E07 Retention, moderation, compliance, assurance | retention slice after ADR 8 and WS04-S4 | 35 to 59 |
| 6 | E02 Agent computer, egress, credential custody | after WS06, E01-S1, WS17 | 20 to 32 (+12 to 20 parked) |
| 7 | E06 Guests, teams, external surfaces | after WS04-S4/S8/S10 and ADR 10 | 34 to 58 |
| 8 | E09 Lists, records, templates | after WS14 grants phase 1 | 8 to 16 |
| 9 | E10 Skills, plugins, public API, federation | API tokens any time; skills after S3.5 | 38 to 68 |
| 10 | E05 Calls, meetings, voice (decision-gated) | only after ADR 7 | 43 to 72 |
| 11 | E11 Desktop shell and multi-window | after E08 PWA and a product decision | 12 to 21 |
| 12 | E12 Client long tail | on demand | 22 to 40 |

Total about 290 to 500 engineer-weeks across all epics, excluding parked F-t1-096.

---

## E01 Agent operations and multi-agent rooms

- Goal: make per-room and per-agent spend visible and capped, screen member text, then let several agents cooperate, with a task tracker that shows what waits on a human.
- Items: F-t1-027, 045, 025, 056. Residual claimed: F-t1-006 phases 2-3 (agent/model allowlist, locked switches, autonomy posture, rollout flags; WS03 decisions D1, D5).
- Planes: Model (cost-core Go, model-gateway `budget.rs`, session-core, execution-core), Control (allow/block decision), Application (task projection), Frontend.
- Prerequisites: WS03-S1/S3 (effect-policy admin) for the allowlist; WS08 (addressed multi-agent wake-up); WS09-S1 (run guard); WS10-S3 and S8 (gate, quarantine); WS11 (read state for task unread); decisions ADR 5, conflicts C4 (one origin enum), C5 (policy registry), sibling F-t3-023 (Control owns `allowed_models`).
- First slice E01-S1 (M): carry `space_ref` and binding ref through the usage envelope into cost-core entries and fix the zero token totals, then a read-only room spend panel. Today cost-core dimensions are tenant, actor, run, request, model, producer (`apps/Model Plane/go/services/cost-core/internal/ledger/ledger.go:44`, C) and the room prints no tokens (`apps/Frontend Plane/verevonv3/src/features/spaces/lib/activity-grammar.ts:696-709`, C).
- Then: E01-S2 RoomMember trust class and reply redaction (F-t1-045 a, e; M); E01-S3 allowance enforcement, token based; E01-S4 one-hop `delegate_to_agent` re-entering the gateway under the original human's bearer (generic subagent delegation is depth 1 with the same identity today, `apps/Model Plane/rust/services/execution-core/src/subagent/mod.rs:29`; L); coordinator with nonce, deadline and shadow loop detector last (XL).
- Must not break: ZDR (aggregate usage only; ZDR refuses `max_cost_usd`), owner-bound runs, the org `budget_exceeded` refusal, Space access never widening an owner-plane ACL; every hop records the initiating human.
- Effort: 26 to 42 (cost 8 to 14, guardrails 2 to 4, multi-agent 10 to 16, tracker 4 to 7, allowlist residual 2 to 4).
- Exit: room spend and per-agent attribution visible; allowance warns and hard-stops; a two-agent task completes with hop cap and no loop in a scripted eval (eval-lab-py case); task state `needs_input` visible in the Work tab.
- Trigger: E01-S1 now (independent); E01-S4 only when WS08 and WS10-S3 are live and E01-S3 enforces caps.

## E02 Agent computer, egress and credential custody

- Goal: a per-(Space, agent) computer with room stop/kill and lifecycle, an authorising egress path, and a secrets broker with draft-first outward actions.
- Items: F-t1-028, 044, 096 (096 parked). Residual claimed: F-t1-021 egress slice (WS06 owns resource caps).
- Planes: Model (sandbox-manager, execution-core), Ingestion (integration-corev2 broker, Quarry dns_guard reuse), Control (effect decisions, grants), Endpoint Plane only for 096.
- Prerequisites: WS06 (resource caps, Quarry egress route, sandbox ZDR handling; sandbox-manager has none, only a process test mentions ZDR); WS10-S2/S3 (grants); WS17 (memory containment); E01-S1; ADR 6; conflicts C8 (one enforcement point), C19, C20 (register Endpoint Plane or drop the name).
- First slice E02-S1 (S to M): room stop/kill route for processes under a Control effect decision, through the process registry (`apps/Model Plane/go/services/sandbox-manager/internal/process/store.go`), plus surfacing watches through capability-core `/api/v1/watches` (the gateway still answers `watches_not_built`, `spaces.rs:1627`). Caution from a past finding: signalling the bwrap monitor does not reach the command, so the kill must go through the registry host-report path.
- Then: E02-S2 lifecycle states beyond SCRATCH, ACTIVE, SNAPSHOTTING, DESTROYED (`sandboxes.proto`, C) with per-agent home and an explicit ephemeral rule for ZDR (L); E02-S3 broker with host, method and path pinning on `egress.SafeClient` (L); E02-S4 authorising egress proxy (L); E02-S5 connector cockpit, draft-first sends, bounded cross-room posts (M each).
- Must not break: network off by default, Model proposes and Quarry executes, no credentials visible to the model, ZDR rooms get no durable home.
- Effort: 20 to 32; 096 adds 12 to 20 only after a registered plane and device-identity contract.
- Exit: manager can stop a runaway process from the room; an agent reaches only intersected-allowlist hosts; a secret is used without appearing in transcript or logs (scrub test); receipts written.
- Trigger: E02-S1 after WS06 caps; S2 onward after E01-S1 and WS17.

## E03 Room automations and webhooks

- Goal: members create, pause and test schedules, watches and webhook endpoints from the room; each fire is re-authorised and shows an outcome.
- Items: F-t1-036, 047, 074 (belongs to no workstream: 036, 047).
- Planes: Control (user-core shared-audience decisions), Model (capability-core cron and watch, execution-core), Ingestion (integration-corev2 receive and verify), Frontend, Application (provenance).
- Prerequisites: a new Control shared-audience variant of schedule create, fire, run and step and watch create and observe (today all call `validatePersonalAuthority`: `apps/Control Plane/user-core/internal/spaces/schedule_fire_decision.go:90`, `schedule_create_decision.go:77`, C; ZDR refused); WS10-S2/S3 (grants, resume); E01-S1 caps; the S4.4 ledger (E08-S3) for delivery; decision C9 (schedule and watch owner; sibling F-t3-018 and F-t2-018 disagree).
- First slice E03-S1 (M, Control only): shared-audience schedule decisions plus tests; dark until a room surface exists. Next E03-S2 (S to M): fire log and outcome vocabulary, pause and resume in the Work tab (`SpaceWorkPanel.tsx:171-189`).
- Then: room-bound create with stored recipient acceptance (M); webhooks as a webhook source kind of the Watch primitive with an endpoint-id route family beside the mail-task route (`integration-corev2/internal/api/server.go:933`), notify-and-summarise through the tool-free lane first (L); calendar trigger after live proof (M); footer for automated posts (S).
- Must not break: per-slot re-authorisation and idempotent fire keys (`cron/sweeper.go`), payloads untrusted with no tools in the same turn, ZDR refusal with a typed reason, member skip notes at fire time.
- Effort: 16 to 25 (Control 3 to 4, fire log 2, create 3 to 4, webhooks 5 to 8, calendar 3, footer 1).
- Exit: a member schedules a room digest, it fires under the creator's authority, the fire log shows `ok` or `refused`; a signed webhook starts a summary turn once despite redelivery.
- Trigger: C9 decided, E01-S1 and WS10-S3 live.

## E04 Agent tools, forms, history answers and file knowledge

- Goal: rooms get typed cards and input requests, a per-room tool cockpit, agents that search room history under the asker's authority, and files that become agent knowledge.
- Items: F-t1-026, 037, 038, 046.
- Planes: Frontend (card registry), Model (registry, session-core index use, prompt builder), Data (extraction, proposals), Control (import contract, enablement).
- Prerequisites: WS15 (typed entries and card registry), WS10-S9 (resumable-kind registry, for `request_input`), WS13 (message index, ADR 9; reuse `model.thread.read`), WS14 (attachment pipeline and Control shared-import validator), WS18, S3.4 broker (E02-S3) for secret fields (dropped from 026), decisions C10, ADR 5, C19.
- First slice E04-S1 (M): default-deny MCP tools in Space turns, per-room enable as a human-only Control decision, and a write path for the stored `allowed_tools` list (read and enforced at `spaces.rs` binding code, nothing writes it). Tool exposure is resolved per org and user with no Space input (`model-gateway/src/runtime_registries.rs:743,830`, C). Cheap sibling: "Ask agent about this" composer action (S).
- Then: typed form, choice and link nodes in the chat-nodes union (M); `request_input` end-turn tool (M); history tools over the WS13 index (L); Control shared-import contract plus Data Plane upload into the retrieval binding (L); cited answers last.
- Must not break: private servers never widen by Space linkage, runs stay owner-bound so only the owner answers a form, retrieved messages and attachment text are untrusted, ZDR threads get nothing indexed and typed refusals.
- Effort: 20 to 32.
- Exit: MCP server enabled for one room only and refused in another; a form submit resumes a run once; an agent answer cites a message the asker can read and refuses one they cannot.
- Trigger: WS15 and WS13 slices shipped.

## E05 Calls, meetings and voice (decision-gated on ADR 7)

- Goal: only if ADR 7 reverses the huddle non-goal (`apps/Frontend Plane/verevonv3/docs/space-defenition.md`): room calls on a bought SFU, meeting intelligence with consent, agents in meetings, voice front for agents.
- Items: F-t1-029, 050, 066, 067, 068, 084, 085, 086.
- Planes: Application (call record), Control (join decision, meeting AI policy), Model (ASR, recap, realtime), Data (transcript, recording), Frontend, Infra (SFU), Ingestion only for 066.
- Prerequisites: ADR 7 (and capture ownership); WS01, WS02-S5 (push presence); E08; the Control shared-room import decision shared with WS14 and E04; ASR provider and ZDR attestation.
- First slice, decision free E05-S1 (M): voice front phase A (F-t1-067): BFF route minting a Space-scoped realtime session (model-gateway route `/v1/ai/realtime/session` is registered at `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:449`, M-state file; the request has no tools field, so tool schema is a proto change), with Wake Lock and busy guard from F-t1-086. If ADR 7 allows meetings, E05-S2 (L): shared-room import decision and upload-based recap saved as a Data Plane document (ZDR refused). Calls (029, 6 s poll-based card first) only after that.
- Must not break: ZDR fail-closed, consent default-deny, agent voices always labelled, transcript audience carried on units.
- Effort: 43 to 72, widest uncertainty of all epics; decision-free slices only 5 to 8.
- Exit: per phase; meeting MVP exits when an uploaded recording yields a recap visible only to attendees and no recap for a ZDR org.
- Trigger: ADR 7 accepted by the owner. Until then build only E05-S1.

## E06 Guests, teams and external surfaces

- Goal: guests with expiring room access, teams that expand into room membership with source tracking, then external channels, snapshot shares and org lifecycle.
- Items: F-t1-053, 054, 075, 087, 093. Residual claimed: F-t1-023 linked identities.
- Planes: Control (auth-core teams and identity, user-core membership), Application, Ingestion (075), Channel Plane docs-only.
- Prerequisites: WS04-S4 (source-tagged membership writer, D-WS04-1), S8 (directory), S10 (durable membership events), S11 (history window as the guest floor); WS05 (SCIM 501 fix and real SCIM, `integration-corev2/internal/api/server.go:1237` returns fake success today); ADR 10; billing seats unverified.
- First slice E06-S1 (M, Control): enable Better Auth teams and expand a team grant into room memberships tagged with source (dormant `team` tables, `auth-core/src/db/schema.ts:244-258`, C; reserved `team` subject in resource_grants). Parallel E06-S2 (M): finish wiring the mail-task pipeline now answering 503 (`gateway/src/domains/mail_task.rs:413`) before any per-agent email authority design.
- Then: guest subject type excluded from the declarative sync, room-targeted email invite (XL); link invites later; org lifecycle states (M); org-group UI (S to M); snapshot share, org scope only (L).
- Must not break: a guest never becomes an org member or sees org documents; ReplaceMemberships must not revoke team or guest rows; membership never widens an owner ACL; agents in a mentionable group opt-in.
- Effort: 34 to 58.
- Exit: removing a person from a team removes only team-derived access; an expired guest loses access through the normal change path.
- Trigger: D-WS04-1 resolved and WS04-S10 live; ADR 10 for guests.

## E07 Retention, moderation, compliance and assurance

- Goal: time-based retention, moderation with ban cascade to agents, compliance export and holds, assurance facets.
- Items: F-t1-043, 051, 077, 094. Residual claimed: F-t1-031 SIEM and usage-ledger export (the role-gated read fix stays in WS05).
- Planes: Control (policy, holds, suspension), Model (Session Core purge, tombstone), Application (reports), Data, audit-core.
- Prerequisites: ADR 8 (hold over retention over ZDR, each plane its own sweeper); WS04-S4 (roles) and S10 (durable membership events, since ban rides `membership_changed`); WS05 (audit read gating); WS17 owner link (ADR 4); conflict C15 (one audit and SIEM design).
- First slice E07-S1 (L): numeric TTL on `space_effect_policies` (table from `018_space_effect_policies.up.sql`; user-core highest on disk is 029, WS10 reserves 031, WS03 032: take the next free) and one Session Core sweeper with honest receipts; nothing else sweeps today except audit-core's purge (`audit-core/cmd/server/main.go:150-185`). Parallel E07-S2 (L): report on a message, queue, Session Core tombstone write; reports content-free in Application.
- Then: ban and timeout as Control membership state that also stops the person's agents; shared-Space deletion on the existing saga (`space_legal_holds`, `022_space_deletion_authority.up.sql:14-19`); export and holds; hash-chained audit (audit-core has only `payload_hash`, `schema.sql:32`).
- Must not break: legal hold outranks any sweep; ZDR never weakened; Application only requests purges; a failed ban check alerts rather than locks users out.
- Effort: 35 to 59.
- Exit: a TTL room purges messages and returns a receipt per plane; a banned user's agents stop within the revocation SLO.
- Trigger: ADR 8 accepted; staging state of saga migrations 022, 023, 0026, 0027 verified first (open question 11).

## E08 Push, PWA, DND, digest and catch-up

- Goal: reach people outside the open tab without content leaks or night pages.
- Items: F-t1-035, 059, 060, 061, 092.
- Planes: Frontend (PWA, toast, badge), Application (notification-core, ledger, DND gate), Model (recap call), Control (quiet hours, ZDR wake carve-out).
- Prerequisites: WS11 (Space producers, read state), WS16 (preferences), WS07 (stale-tab handling F-t1-024 before a service worker), WS13 (typed refs, F-t1-039); ADR 2 content-free wake class for ZDR.
- First slice E08-S1 (S, Frontend only): tab-title count, favicon dot, `setAppBadge` and in-tab toast from the unread projection, honouring mute. Today `public/` has no manifest or service worker (C) and push is Novu vocabulary behind a disabled adapter (`notification-core/internal/runtime/client.go:218-219`, C).
- Then: E08-S2 manifest and shell-only service worker (M; own no-cache nginx location since `.js` is immutable); E08-S3 finish the S4.4 ledger: bounded retries, max-age, ZDR proof (`delivery_worker.go:91-94` rejects ZDR today) (L); E08-S4 DND tri-state gate extending the stored but unenforced `QuietHours` (`user-core/internal/http/handlers.go:999-1014`) (M); E08-S5 digest and Web Push (L); catch-up recap as a stateless gateway-composed call over the viewer's own reads (M); delivery-health banner last.
- Must not break: approvals never decided from a toast; no content in push payloads; ZDR orgs get refs only; mute is presentation only.
- Effort: 18 to 30.
- Exit: closed-tab approval wake arrives content-free and opens the right room; a DND user gets nothing until the window ends.
- Trigger: E08-S1 after WS11 producers; S5 after S3.

## E09 Lists, records and room templates

- Goal: one revisioned record substrate with Lists, plus templates and clone for room creation.
- Items: F-t1-058, 064.
- Planes: Application (records, templates), Control (kinds, decisions), Frontend gateway.
- Prerequisites: WS14 phase 1 (grant write surface for F-t1-057), WS11 inbox, WS04 roles; ADR-0001 planner migration; ADR for project and case kinds.
- First slice E09-S1 (M): Convex `spaceRecords` and revisions tables (absent in `convex-core/convex/schema.ts`) with compare-and-swap on previous revision, gateway routes whose read and write effects are Control decisions (Convex does not evaluate roles), human edits only.
- Then: table view with assignees (M); templates with agent roster through the pending_registration saga (M); invite-on-mention (M); case and project kinds after an ADR; agent write path excluded (it needs a new owner-action vertical, as Control resolves only `tickets.create`, `model_action_view.go:14`).
- Must not break: records hang off `spaceRef` never `plannerDocuments` or Convex projects; revisions excluded from chat unread; templates never pre-grant tools; mentions never grant access; register tables in the deletion flow.
- Effort: 8 to 16.
- Exit: two members co-edit a list row with a conflict on stale revision; a template room appears with its agents bound.
- Trigger: WS14 grants phase 1 live.

## E10 Skills, plugins, public API and federation

- Goal: governed Space skills, agent templates and revisions, a user-bound public API, federated search.
- Items: F-t1-065, 071, 072, 076, 081, 088, 083.
- Planes: Model (session-core skills, capability-core plugins), Control (publish and token decisions), Application (templates), Frontend gateway, Ingestion (provider reads).
- Prerequisites: S3.5 canonical skill record decision; WS18 (action registry opened to agents, C6); E06-S1 (team layer for 072); E01 allowlist; conflict C14 and sibling F-t3-058/059 (two manifest designs).
- First slice E10-S1 (M): gateway-side bearer authentication of user-bound scoped tokens over the existing org API keys (`gateway/src/domains/settings/api_keys.rs`, auth-core validate RPC `orpc-router.ts:2717-2790`), scopes = action ids plus optional Space allowlist, never wider than the user ACL. Extend, do not build.
- Then: Space skills (L): `scope=space`, review state, immutable reviewed versions on `session-core` `agent_skills` (`0031_agent_skills_ownership.sql`, org and user only today) plus a Control publish decision; agent definition revisions (M) in Application; templates and catalogue (M); federated search orchestrator (L); team layer (XL) and app lifecycle (XL) last.
- Must not break: skill body is prompt text so review covers the exact stored body; tokens never exceed user ACL; plugin hooks and MCP components refused.
- Effort: 38 to 68 (mostly optional).
- Exit: a token reads one allowed room and gets 403 elsewhere; a reviewed skill edit returns to draft.
- Trigger: E10-S1 any time; skills after S3.5.

## E11 Desktop shell and multi-window

- Goal: Tauri shell, shell-only conveniences, pop-out windows.
- Items: F-t1-073, 090, 089.
- Planes: Frontend, Control (PKCE handoff in auth-core).
- Prerequisites: E08 PWA and badge; WS07 (F-t1-024); WS13 (protocol links via entity router); product decision (open question 15: is web plus PWA enough).
- First slice E11-S1 (S): web `beforeunload` guard for unsent text or a run awaiting approval (none exists today). Then idle presence signal (S), cross-org unread endpoint (M), Tauri shell with one PKCE endpoint (XL).
- Must not break: single content security policy, no wider origin, one active org per request.
- Effort: 12 to 21.
- Exit: close guard fires; signed shell signs in through the system browser.
- Trigger: owner confirms a desktop need after E08.

## E12 Client long tail

- Goal: polish and operability items built on demand.
- Items: F-t1-069, 070, 078, 079, 080, 082, 091, 095.
- Planes: Frontend with small Application and Model parts.
- Prerequisites: WS02-S5 (091), WS13 (069), WS14 (082 clips), WS16 (080 server-held state), ADR 2 (080).
- First slice E12-S1 (S): pinned-first ordering (`SpaceRoomTimeline.tsx:137-141` re-sorts by time and ignores pins) with header counts, and the http/https/mailto scheme allowlist in `parseInline` (F-t1-095 link slice). Then ids-only report flow, traceparent marks, translation, ZDR-gated draft persistence re-keyed from `home-draft-storage.ts`.
- Must not break: ZDR (no persisted drafts for ZDR orgs), same-clock rule for latency marks.
- Effort: 22 to 40.
- Exit: per item.
- Trigger: user demand; E12-S1 immediately.

## Index: all 96 audit items to a workstream or epic

| Id | Target | Short title | Residual or note |
|---|---|---|---|
| F-t1-001 | WS01 | Human-first room conversation |  |
| F-t1-002 | WS08 | Addressed agent wake-up |  |
| F-t1-003 | WS02 | Space realtime substrate |  |
| F-t1-004 | WS09 | Agent work visible to the whole room |  |
| F-t1-005 | WS06 | Make zero-data-retention binding in every plane, with typed refusals a |  |
| F-t1-006 | WS03 | Space governance admin | phases 2-3 (allowlist, posture, locked switches, flags) -> E01 |
| F-t1-007 | WS08 | Identity-bound @mentions for people and agents with mention records, r |  |
| F-t1-008 | WS11 | Space notifications |  |
| F-t1-009 | WS11 | Cross-room attention | attention aggregate used by E08 |
| F-t1-010 | WS10 | Approvals with grant scopes and durable resume, tool permission policy | grants used by E01, E02, E03, E04 |
| F-t1-011 | WS17 | Personal agents with an owner-controlled access list and add policy, a | owner link used by E07 ban cascade |
| F-t1-012 | WS17 | Audience-safe agent context and memory | labels gate E02 and E01 |
| F-t1-013 | WS14 | Room attachments and file library | files as agent input -> E04 (046) |
| F-t1-014 | WS07 | Bring agent-produced artifacts and the shared viewer registry into the |  |
| F-t1-015 | WS19 | Room canvas | canvas pointer reused by E09 seed artifacts |
| F-t1-016 | WS12 | Direct messages and group DMs on Space authority, with conversation ki |  |
| F-t1-017 | WS12 | Message lifecycle and reactions | tombstones reused by E07 |
| F-t1-018 | WS13 | Permission-checked message search |  |
| F-t1-019 | WS18 | Let room agents call Space actions themselves (agent self-API with run | agent self-API; registry reused by E10 |
| F-t1-020 | WS20 | Room-scoped agent browser | room browser; hand-off cues reused by E02 |
| F-t1-021 | WS06 | Agent computer confinement | egress slice -> E02; caps stay WS06 |
| F-t1-022 | WS04 | Room administration basics |  |
| F-t1-023 | WS05 | Enterprise identity | linked identities later -> E06 |
| F-t1-024 | WS07 | Client resilience |  |
| F-t1-025 | E01 | Multi-agent rooms |  |
| F-t1-026 | E04 | Typed cards and agent input requests (forms and secret drop) |  |
| F-t1-027 | E01 | Agent operations |  |
| F-t1-028 | E02 | Persistent per-agent computer with lifecycle UX and a governed egress  |  |
| F-t1-029 | E05 | Room calls on a bought SFU with Control-authorized join and a navigati |  |
| F-t1-030 | WS07 | Room content hardening |  |
| F-t1-031 | WS05 | Org audit trail for admins | SIEM and usage export -> E07; role-gated read fix stays WS05 |
| F-t1-032 | WS15 | Composer for real work |  |
| F-t1-033 | WS01 | SPA realtime client layer |  |
| F-t1-034 | WS15 | Scalable room timeline |  |
| F-t1-035 | E08 | Attention outside the open tab |  |
| F-t1-036 | E03 | Room automations |  |
| F-t1-037 | E04 | Space tool cockpit and reliable tool connections |  |
| F-t1-038 | E04 | Agents and AI answers over room history under the asker's authority |  |
| F-t1-039 | WS13 | Entry references and permalinks |  |
| F-t1-040 | WS09 | Per-thread serialized, leased agent run queue and member-safe mid-run  |  |
| F-t1-041 | WS13 | One command layer on the typed action registry |  |
| F-t1-042 | WS11 | Read state, presence and personal follow-ups | read frontier used by E08 |
| F-t1-043 | E07 | Room moderation |  |
| F-t1-044 | E02 | Credential custody and outward actions |  |
| F-t1-045 | E01 | Agent runtime guardrails for shared rooms |  |
| F-t1-046 | E04 | Files become agent input and room knowledge |  |
| F-t1-047 | E03 | Webhooks that wake room agents (Space-bound inbound endpoints) |  |
| F-t1-048 | WS04 | Room visibility, directory and self-join (org-public or private), with |  |
| F-t1-049 | WS04 | Searchable people-and-agent directory with an agent-readable resolver |  |
| F-t1-050 | E05 | Meeting intelligence with consent |  |
| F-t1-051 | E07 | Retention policies for rooms, files and agent sessions, plus shared-Sp |  |
| F-t1-052 | WS15 | Typed timeline entries |  |
| F-t1-053 | E06 | Guests and link invitations with mandatory expiry |  |
| F-t1-054 | E06 | Teams and sub-groups with source-tracked membership; mentionable group |  |
| F-t1-055 | WS16 | Personal sidebar organisation | sidebar; E11 cross-org badges |
| F-t1-056 | E01 | Agent task tracker |  |
| F-t1-057 | WS14 | Artifact sharing | phase 1 grants gate E09 and E04 |
| F-t1-058 | E09 | Typed room records and Lists |  |
| F-t1-059 | E08 | Do not disturb, quiet-hours schedules and urgent break-through, visibl |  |
| F-t1-060 | E08 | Finish the S4.4 delivery ledger and add a content-free missed-activity |  |
| F-t1-061 | E08 | 'Catch me up' recap of what I missed, scoped to my own read access |  |
| F-t1-062 | WS16 | Space UI quality baseline | reduced motion etc; E12 extras |
| F-t1-063 | WS07 | Agent identity in the room |  |
| F-t1-064 | E09 | Room creation flows |  |
| F-t1-065 | E10 | Governed Space skills and plugin bundles (ecosystem-compatible manifes |  |
| F-t1-066 | E05 | Agents attend Zoom, Meet and Teams meetings as visible guests through  |  |
| F-t1-067 | E05 | Duplex voice for agents |  |
| F-t1-068 | E05 | Screen sharing in calls with a sharing indicator, sensitive-surface re |  |
| F-t1-069 | E12 | Search operators with Verevon-native filters, typed result tabs and re |  |
| F-t1-070 | E12 | Room header and pulse rail extras |  |
| F-t1-071 | E10 | Shareable, versioned agent templates with an export policy, and an int |  |
| F-t1-072 | E10 | Scoped agent context and promotion path |  |
| F-t1-073 | E11 | Tauri desktop shell around the gateway origin |  |
| F-t1-074 | E03 | Provenance footer on relayed and automated messages and notifications; |  |
| F-t1-075 | E06 | Agent external channels |  |
| F-t1-076 | E10 | Public developer API with scoped tokens and rate limits (events, exter |  |
| F-t1-077 | E07 | Compliance export, custodian legal hold, information barriers and audi |  |
| F-t1-078 | E12 | Perceived-latency observability |  |
| F-t1-079 | E12 | Ids-only "report this" flow and hardened error-code registry |  |
| F-t1-080 | E12 | ZDR-safe client persistence |  |
| F-t1-081 | E10 | Long-tail polish |  |
| F-t1-082 | E12 | Long-tail polish |  |
| F-t1-083 | E10 | Federated search across connected providers under the asker's authorit |  |
| F-t1-084 | E05 | Meeting recording to governed object storage with a state machine, a m |  |
| F-t1-085 | E05 | Live in-meeting AI layer |  |
| F-t1-086 | E05 | Call polish and operability |  |
| F-t1-087 | E06 | Read-only snapshot share of a room or thread with an allowlisted proje |  |
| F-t1-088 | E10 | Versioned publish lifecycle for agent-built apps and shared agent defi |  |
| F-t1-089 | E11 | Multi-window coordination |  |
| F-t1-090 | E11 | Shell-only conveniences |  |
| F-t1-091 | E12 | Realtime and media operability after push ships |  |
| F-t1-092 | E08 | Delivery health |  |
| F-t1-093 | E06 | Multi-org and external collaboration horizon |  |
| F-t1-094 | E07 | Assurance long tail |  |
| F-t1-095 | E12 | Long-tail Space UX, build on demand |  |
| F-t1-096 | E02 | Reverse channel so a user's own computer can run agent tool requests w |  |

Count check: 96 ids, F-t1-001 to F-t1-096 with no gap or duplicate; 40 to workstreams WS01-WS20, 56 to epics E01-E12.
