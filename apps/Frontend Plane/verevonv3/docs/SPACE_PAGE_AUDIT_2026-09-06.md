# Space page audit — how far we are, what is next

Date: 2026-09-06
Scope: `src/features/spaces/*`, `src/shared/api/spaces-client.ts`,
`src/features/core/components/sidebar/CoreSidebarSpacesPanel.tsx`,
`apps/gateway/src/domains/spaces.rs`, and the Space-scoped surfaces the other
planes actually publish.
Method: code read of every Space file, diff against the eight planning docs
(ADR-0001, ADR-0002, the scope plan, the cockpit wiring handoff, the QM
comparison plan, the QM improvement plan, the cowork research, and
`space + qm style improvements`), a cross-plane grep for `space_ref`/`space_id`,
and a test run.

Verification state: `vitest run src/features/spaces src/features/core/components/sidebar`
passes, 8 files / 102 tests. No live stack was exercised for this audit; every
"works" below means source plus unit tests, not observed runtime.

## 1. Verdict in one paragraph

The room exists and is honest. In three weeks (2026-08-13 to 2026-09-02, 28
commits) the Space page went from a membership stub to a six-tab cockpit with a
real timeline, a room composer with `@`-mention invocation of bound agents,
Grok-style one-line agent creation, bind-existing, Space instructions, a
Control roster, personal-Space deletion receipts, and an org-wide agent
registry. Four of six tabs are wired to real server projections. What it is
**not** yet is a multi-person room: transcripts are owner-bound so a colleague's
post renders as a preview only, there is no realtime, no in-room approval, no
member management, no way to create a second shared room, and no
pause/remove for an agent once it is in. Work and Knowledge are blocked on
Model Plane and Data Plane, not on the frontend.

## 2. Progress against the plans

Phase names are the scope plan's (`SPACE_AGENT_SCOPE_PLAN_2026-08-14.md`) and
the comparison plan's (`VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md`).

| Planned item | Status | Evidence |
|---|---|---|
| UI-0 truthful shell, Core Sidebar sole navigator | Done | `SpaceCockpit.tsx`, `CoreSidebarSpacesPanel.tsx` |
| UI-1 read-only Agent tab from Control roster ∩ Application binding | Done | `getSpaceAgents`, `SpaceAgentPanel`, `identity_published` flag |
| UI-2b `@` mention invokes a bound agent, reply in same thread | Done | `SpaceRoomComposer.tsx` → `streamChat({ spaceRef, mentionedAgentRef, threadId })`; gateway `inject_mentioned_space_agent_persona` |
| Binding policy fields stored and enforced (`trigger_modes`, `allowed_tools`, `approval_mode`) | Done | typed in `spaces-client.ts`, rendered as chips; scope-plan text saying "not yet on the binding" is stale |
| UI-3 bind existing agent | Done | `SpaceBindAgentDialog`, `/agents/available`, `/agents/bind` |
| UI-3b create agent from room (two server steps, one action) | Done | `SpaceCreateAgentDialog`, `POST /spaces/:ref/agents` |
| UI-4 installations page + Chief/Core routing, ADR-0002 registry | Done (narrow slice) | `/agents/installations`, `/agents/chief-core`, `agentInstallationsForOrgForGateway` |
| ADR-0003 Space instructions layer | Done | `SpaceInstructionsSection`, `GET/PATCH /instructions` |
| Org room auto-provision + Slack-shaped landing | Done | `SpacesIndexPage.pickDefaultSpace`, `ensureOrganizationRoom` |
| Personal Space deletion with per-plane receipts | Done (frontend) | `SpaceMembersPanel` danger zone; migrations still `[ ]` in the tracker |
| Members tab: roster | Done, read-only | `getSpaceRoster` |
| Members tab: add and remove a person | Done (§10) | `POST/DELETE /api/v1/spaces/:ref/members[/:id]`, `addSpaceMember` / `removeSpaceMember`, owner/manager gated |
| Members tab: change a person's role in the room | Not built | `addSpaceMember` sends only `member_id`; no role field on the gateway route, no UI. Control owns the role, so this needs a membership-role write, not a projection change |
| Create a shared room from the UI | Done (§10) | `POST /api/v1/spaces` with `kind`, "Nytt rom" in the sidebar |
| Create a project / case Space from the UI | Not built | the client sends `kind: 'room'` only; Control accepts all four kinds, and nothing in the product yet distinguishes a project from a room |
| Pause / resume / remove an agent binding in the room | Done (§9) | `PATCH/DELETE /api/v1/spaces/:ref/agents/:binding_ref`, `setSpaceAgentState` / `revokeSpaceAgent`, agent card controls |
| UI-2c agent-to-agent delegation | Not started | plan says "no safe partial slice"; correct to leave |
| Activity tab from receipts, approvals, cost (S2.5) | Done (§14) | five sources, one ordering: threads, runs with token/step cost, approvals, owner receipts correlated through the grant that authorized them, and authority grants. `GET /spaces/:ref/activity` + Conversation Core's `/spaces/:ref/activity`. Closes S2.3 slice 5 |
| Work tab (S4.5) | Done (§12) | the Model Plane block was removed as part of it: `ListRunsRequest.space_id` and a `space_ref` filter on the cron listing, then `GET /spaces/:ref/work` |
| Knowledge tab | Done (§13) | `documents.space_ref` is the edge `documents-api` was verifying and discarding; `POST /v1/knowledge/space-sources`, `GET /spaces/:ref/knowledge`, `SpaceKnowledgePanel`. Enforcement widened from 3 paths to 10, each with real enforcement |
| Sub-routes `/spaces/:id/chat|work|…` | Deviated, accepted | hash tabs with legacy aliases; the wiring doc endorses this |
| Presence: who else is in the room, and who is writing | Done (§18) | `spacePresence` + `POST /spaces/:ref/presence` (one heartbeat that also answers "who else is here"); the older `conversationPresence` table stays dormant — it is keyed per conversation, not per room |
| Shared-thread continuity | Done (§7) | a visible-tab 6 s poll of the thread projection, published once and read by the room, the sidebar and the composer |
| Realtime as a push transport | Not built | everything live is polled. A server push (SSE or Convex subscription) would remove the 6 s floor on how fast the room reacts; nothing depends on it today |
| Skills: `/` picker in the room composer with a real scope badge (later tier) | Done (§17) | model-gateway `InvokeRequest.skill_ids` resolved server-side and injected ahead of keyword matches; `GET /api/v1/skills` already open to members; badges `Organisasjon` / `Personlig` — the two scopes the registry has |
| Cost: allowance with a hard stop (later tier) | Hard stop legible; allowance blocked (§17) | `budget_exceeded` rendered as a refusal with the way to Settings › Forbrukstak; no used-vs-limit pair reaches a member and no per-Space cost exists |
| Routines posting back into the room (later tier) | Read side only (§17) | schedules bound to the room already list on the Work tab; creation stays in Settings behind the scheduled-effect gate |
| Memory page with revision restore (later tier) | Blocked on a plane decision (§17) | no revision store anywhere; ADR-0003 deferred versioning; ownership split Model vs Data unresolved |
| Room hygiene: unread since last visit, pin, persisted auto-title (4b) | Done (§16) | Convex `spaceReadMarkers` + `GET /threads` `read_marker`, `POST /spaces/:ref/read`, `PATCH /spaces/:ref/threads/:id/presentation`; badges in timeline and sidebar from one derivation |

Release-gate reality has not changed since the 2026-08-17 reconciliation: R-1
to R-5 are `[~]`, the Model allowlist is empty, and the docs are explicit that
new *effectful* UI should wait for candidate evidence. Items 1 to 4 in §4 below
are read-side or reuse existing governed write paths, so they do not collide
with that rule.

## 3. Findings from the code

Ordered by consequence. None of these fail tests; they are product and quality
gaps.

### 3.1 A shared room is not readable by its other members

`SpaceRoomTimeline.tsx` fetches each post's turns through
`getChatThreadTranscript`, which the gateway resolves only against the
caller's own durable thread list. The component comment says so. In the
organization room, a colleague's thread therefore falls back to the one-line
`preview`, and author attribution assumes a single human in the roster
(`humanName` memo). Since the org room is now the default landing, this is the
first thing a second user sees. Fix needs Control shared-thread transcript
authority (recipient-audience gated) plus server-side author on each turn; the
frontend side is small once `turns[].author` exists.

### 3.2 No live updates

Nothing pushes into the room. The thread list refetches after the user's own
exchange and on the 30 s membership recheck. Two people in the same room do
not see each other's posts until a recheck fires. A cheap first step is a
visible-tab poll of the thread projection every 5 to 10 s; the durable answer
is the Application delivery/projection outbox already built at source (R-4).

### 3.3 "Attention" is counted but not actionable

`SpacePulse` counts `awaiting_approval` threads and `threadStatus` labels them,
but there is no approve or deny control in the room. The comparison plan's
S2.5 asks to reuse `AgentRunConsole.tsx` proof/approval panels here. Today the
human has to leave the room to act, which is the "Copilot side panel" failure
mode `space-defenition.md` warns about, inverted.

### 3.4 Agents can be added but never governed in the room

The product model says "Pause / mute / remove **here**". The room can create
and bind, but there is no pause, resume, or remove. A wrongly added agent stays
active until someone edits Convex. The states `paused` and `revoked` are typed
and rendered but unreachable from the UI.

### 3.5 One channel only

The sidebar draws "Kanaler" and "Personlig rom", but the only channel that can
exist is the org room. There is no create-room flow and no invite flow, so the
Slack shape is a single #general. Control already accepts `room|project|case`
and has membership replacement for shared Spaces.

### 3.6 Per-post transcript fan-out

Each `SpaceRoomPost` owns a `createResource` keyed on
`thread_id + updated_at`. A room with N threads issues N transcript requests
on load and again for each changed thread, and the thread list itself is not
paginated. Fine at 10 threads, not at 200. Either a batched
`GET /spaces/:ref/timeline` or lazy loading of collapsed posts.

### 3.7 `space_actions` is org-scoped with `space_ref` echoed back

`spaces.rs::space_actions` returns `human_owner_action_catalog_for(state, user)`
and only echoes the ref. The frontend does not currently consume it, so no
harm today, but if the Agent tab ever renders "actions permitted here" from
this endpoint it would present org authority as Space authority.

### 3.8 Hygiene

- `SpacesIndexPage.tsx` has zero `i18n.tr` calls; every string is hardcoded
  Norwegian, while `SpacePage.tsx` is fully translated. Three `<h2>` in
  `SpacePage.tsx` ("Samtaler", "Aktivitet", "Medlemmer" at lines 296, 369, 749)
  are also hardcoded.
- 22 `verevon-space-*` selectors in `global.css` (room rail, switcher,
  room card, conversation list, fresh-conversation, thread-rail) have no
  remaining TSX reference. They are the pre-cockpit layout.
- `formatWhen` exists twice with different output (date-only in
  `space-thread-presentation.ts`, date+time in `SpaceActivityFeed.tsx`).
- Deletion confirm uses `window.confirm`; the rest of the app uses dialogs.
- `SpaceAgentPanel` doc comment (line 396) still describes the pre-binding
  roster-only design.

## 4. What to do next

Ordered so each step is useful on its own and none requires the release gates
to move first.

**Status, 2026-09-08.** Items 1 to 6 are done — §7, §8, §9, §10, §11 and §12
respectively — and 7 and 8 are struck through below. One piece of item 4 is
not: per-member *role changes*. Rooms, add-person and remove-person shipped;
`addSpaceMember` sends only a `member_id`, so a member's role in a room is
still whatever Control assigned at join. The progress table above says so.

1. **Make the org room readable and live.** Server-side author attribution on
   transcript turns, shared-thread transcript read gated on Space membership
   and recipient audience, and a visible-tab poll of the thread projection in
   the room. This is the difference between "a page" and "a room", and every
   other item below is worth less until a second person can use it.
2. **In-room approvals.** Render `awaiting_approval` threads with the existing
   `ProofApproval` client and `AgentRunConsole` panels inside `SpaceRoomPost`
   and the Activity tab. Read plus an already-governed write path; no new
   authority.
3. **Agent lifecycle in the room.** Gateway `PATCH /spaces/:ref/agents/:binding_ref`
   for `paused|active` and `DELETE` for revoke, same `require_space_agent_grant_role`
   gate as create/bind, Convex mutation plus Control roster update. Card gets
   Pause / Resume / Remove.
4. **Members and rooms.** `POST /spaces` for `kind: room|project` (not only
   personal), and `PUT /spaces/:ref/memberships` through the gateway with the
   owner/manager gate, surfaced as "Add people" in the Members tab. Control
   already has both operations internally.
5. **Hygiene pass** (half a day): translate `SpacesIndexPage` and the three
   headings, delete the 22 dead selectors, unify `formatWhen`, replace
   `window.confirm`, refresh the stale doc comment.
6. **Unblock Work** (Model Plane): add `space_id` to `ListRunsRequest` and a
   `space_ref` filter to the cron listing in `workplane_apis.go`; then a
   gateway `GET /spaces/:ref/work` and the tab renders through the existing
   activity grammar with a `run`/`schedule` adapter.
7. ~~**Unblock Knowledge** (Data Plane): a space-filtered source/document
   listing that resolves through `space_retrieval_bindings`, and widen
   `space_decision_is_enforced_for_path` to the sources and wiki reads.~~
   Done — see §13. The listing needed a Space-to-document edge first:
   `documents-api` verified a Space import decision and then discarded the
   Space, so nothing could say which room a document belonged to.
8. ~~**Then** the plan's own order: Activity receipts (S2.5), watches (S4.3),
   UI-2c delegation, Slack adapter (S5.3).~~ **This flat list was wrong** — see
   §14. Only S2.5 was startable, and it is done. The other three are blocked
   two to four sequences upstream by the plan's own dependency graph:
   - **S4.3 watches** needs S4.2 process registry ← S3.3 durable workspace ←
     S3.2 sandbox lease, which is itself incomplete. Building the Watch record
     with no adapter and no S4.4 delivery would be a store that can never fire.
   - **S5.3 Slack** inherits that chain through S4.4, and Channel Plane is
     docs-only. Even the "open in Slack" banner has no truthful data source.
   - **UI-2c** still needs an execution-core → gateway network path;
     re-verified 2026-09-07 that none exists. The trap analysis holds.

## 5. Docs that need a touch

- `SPACE_AGENT_SCOPE_PLAN_2026-08-14.md` §UI-3b: the sentence that binding
  policy fields "still do not exist" is stale; they are stored, enforced, and
  rendered.
- `SPACE_COCKPIT_WIRING_2026-08-13.md` "Only Chat and Activity have a real
  space-scoped source" is stale for Agent and Members; the status banner at the
  top already says so, the body does not.
- `space-defenition.md` open question "Can a personal Space hold an agent?" is
  still open; Control still refuses personal-Space membership replacement.

## 6. Reference products, checked on 2026-09-06

The planning docs' QM and buzz analysis was written against 2026-08 snapshots.
All five references were re-read from their live repos, docs and product pages
(GitHub, docs.x.ai, warmwind.com FAQ). Maturity as of today:

| Project | Role for us | Stars | Last push | Notes |
|---|---|---|---|---|
| block/buzz | the room | 32.2k | 2026-09-06 | Rust relay + Tauri; v0.5.23; self-described "not finished" |
| yc-software/qm | scope as spine | 14.6k | 2026-09-06 | ~110 commits since our 2026-08-19 baseline `d719f540` |
| CopilotKit/openbot | agents in channels | 4.4k | 2026-09-06 | Alpha v0.0.8, "a template, not a product" |
| Grok Bot (x.ai) | the teammate | n/a | docs 2026-09-03 | beta since 2026-08-11; bundled in SuperGrok / Cursor plans |
| warmwind | the AI employee | n/a | launch 2026-08-26 | task + schedule + streamed desktop; no chat room at all |

Status tags: **have** = built in the Space page, **partial**, **no**.

### 6.1 Feature gap table

| Expectation | Who sets it | Verevon Space today | Gap |
|---|---|---|---|
| Agent is a member with durable identity (name, title, avatar, standing role), added the same way as a person | all five | **have** — roster with `subject_type: service`, name/title/description, colour, templates | Avatar is an initial only; no "duplicate / share as template / hide" |
| Multiple agents per room, one owner per result | Grok Bot, openbot, buzz | **have** (bind many) | One `@` target per message; no ownership on a result |
| `@` mention binds to an exact identity, is authorization-checked at send, never grants | buzz, QM, openbot | **have** — `subject_id`, gateway re-resolves binding | Unbound agent is simply absent; docs ask for "offer to add" |
| Visible working state everywhere (sidebar badge, header, composer bar, member row) with typing fallback | buzz, openbot (`busy`), Grok (purple icon, hover "current action") | **partial** — pulse rail and per-thread status from a poll | No live turn signal; other members never see "working" until a 30 s recheck |
| Interruptible turns: Stop recorded as a stop, message queue while busy, "Steer" | openbot, QM | **no** | Composer disables while streaming; no stop, no queue |
| Approvals as first-class messages with fixed verbs (`Allow once / Allow for session / Always allow / Deny`), composer locked until resolved | QM, Grok Bot, openbot, buzz (`46010→46011/46012`) | **no** — count only | Item 2 in §4; verbs and lock pattern are now industry-standard |
| Human takeover for secrets, 2FA, CAPTCHA; "never type a secret into chat" | Grok Bot, openbot ("Take the wheel") | **no** in room | Quarry live-view exists elsewhere; the research doc's pattern 2 |
| Agent actions are transcript events ("created a routine", "messaged @Bot") | Grok Bot, buzz (one event log), openbot Activity tab | **partial** — Activity tab renders threads and run status | No receipts, approvals, schedule creations, binding changes in the feed (S2.5) |
| Render-class taxonomy for agent activity, mutate in place, never go dark | buzz (15 classes in `agentSessionToolClassifier.ts`) | **partial** — 3 classes in `activity-grammar.ts` | Adapter seam exists; data does not |
| Owner-private tool transcript vs room-public output | buzz (`24200` frames to owner only) | **no** distinction | Worth adopting when receipts arrive: tool steps to the initiating human, results to the room |
| Threads and replies, replies excluded from the timeline unless broadcast | buzz, Grok Bot | **partial** — reply appends to a thread; timeline is flat posts with inline turns | No collapse, no reply count, no "also send to channel" |
| Unread state, pin, mute, auto-title from the opening exchange | buzz, openbot, Grok Bot | **no** | Sidebar shows all threads flat; titles come from `preview` |
| Realtime transport (WebSocket or relay) with authorization at subscribe | buzz, openbot, QM | **no** | Item 1 in §4 |
| Presence and typing | buzz, warmwind (streamed desktop) | **no** — dormant Convex schema | Non-goal per docs; a "working" signal is not |
| Per-room standing orders / instructions, ambient on/off, bot-poster handling | QM (context policy), Grok Bot (Description), openbot (standing instructions) | **have** instructions (ADR-0003) | No ambient policy; correct given "mention only" model |
| Routines created by talking, posting back into the same room, with test run, run history, consent for deliveries to others | QM, Grok Bot, openbot | **no** in room | Schedule-create decision exists at gateway; nothing in the room UI; QM's recipient consent notice is worth copying |
| Watches: "turn this running job into a notifier for this conversation" | QM (`background watch`) | **no** — named as unpublished in Work and Activity | S4.3, blocked behind S3.2 → S3.3 → S4.2 (§14) |
| Memory the human can see, edit, and restore by revision | QM (Memory page) | **no** | S3.x; Knowledge tab candidate |
| Skills as scoped files with `/` autocomplete and scope badge | QM, openbot, Grok Bot | **no** in room | S3.5; skills exist org-wide in Settings |
| Generative UI cards in the transcript (activity, decisions, charts) | openbot, Grok Bot ("inline cards and widgets"), QM miniapps (reverted) | **no** | Approval card and receipt card are the two we need first |
| Agent-to-agent handoff, depth-capped, one owner, collapsed rendering | Grok Bot (2 to 6 bots, Chief of Staff), openbot (`message_bot`, depth 1), QM (`[[ask-agent]]`, Slack only), buzz (job kinds, UX unverified) | **no** | UI-2c; nobody renders the chain as one collapsed unit, so our doc's design is still a differentiator |
| Cost visible to the user (per turn, allowance, hard pause) | QM budgets, Grok weekly allowance, warmwind euro balance, buzz `44200` turn metric | **no** in room | Cost accounting exists in Model Plane; Activity tab promises "what it cost" |
| Audit where every refusal names the rule; secrets never in transcript | openbot, buzz (hash-chained), Grok Enterprise | **partial** — receipts exist per plane, not surfaced in room | Activity tab once receipts land |
| Slack or Teams continuity for the same room | QM (read-only mirror), Grok (event triggers only) | **no** — Channel Plane docs-only | S5.3, blocked behind S4.4 → S4.3 (§14). Even the "open in Slack" banner has no data source: nothing records that a room has an external surface |
| Member management from the room (add people, roles, invite external with expiry) | buzz, QM ("Add people", external invite with role and expiry), openbot (plumbing only) | **no** | Item 4 in §4 |
| Create a second room, private rooms, room types | buzz (Stream/Forum/DM/Workflow), QM (projects), openbot (channels) | **no** | Item 4 in §4 |

### 6.2 What the references do not do, and where we already lead

- **No reference isolates agents as a security boundary.** Grok Bot: "Do not use separate Bots as a security boundary." openbot: one trust perimeter per deployment. warmwind: workers cannot talk to each other at all. Our per-binding Control membership, revision fencing and re-authorized delegation hops are ahead; `space-defenition.md` is right to keep them.
- **No reference collapses agent-to-agent chatter.** Grok's answer is a cap of six and a convention; flaviocopes documents the noise problem. Our "one collapsed unit of work" design is unbuilt but unmatched.
- **No reference has per-plane deletion receipts or recipient-audience-bound retrieval.** Ours are built.
- **QM still lacks** siblings, rewind, a notification inbox and in-web subagents, the same items our comparison plan listed as P2. No need to accelerate them.
- **openbot has no threads, attachments, presence or bridges**; buzz has no Allow/Deny card in code despite the kinds; so the "expected baseline" is narrower than the marketing suggests. The items every reference actually ships are: agents as members, working indicator, approvals with fixed verbs, actions as transcript events, and routines that post back to the room.

### 6.3 Adjustments to §4

The reference check confirms the order of items 1 to 4 and adds three items that
every reference ships and we lack. Insert them as follows:

- ~~**1b. Working indicator and interruptible turns.** A live "agent is working"
  signal (thread projection poll now, delivery outbox later) shown on the
  sidebar row, the post, and the composer bar, plus Stop recorded as a stop.
  Comes with item 1 since both need the same refresh path.~~ Done — see §15.
  The refresh path is shared exactly as predicted: the page publishes the
  projection it already polls, and the sidebar and composer read it.
- **2b. Approval card verbs.** Use `Allow once / Allow for this conversation /
  Always allow / Deny` and lock the composer with "Approve or deny to continue".
  Every reference converged on this wording; do not invent a fourth.
- ~~**4b. Room hygiene.** Unread per thread, auto-title from the opening
  exchange, pin. Small, and it is what makes the sidebar read as channels.~~
  Done — see §16. Unread is a room-level "since you last had it open" marker
  owned by Application Plane; pin is owner-bound because Session Core's
  presentation write is; the AI title is now persisted for the room instead
  of held in the sender's browser.

Later tier, unchanged in order but sharpened by the references: routines that
post back into the room (QM's consent notice and settings deep link), a Memory
page with revision restore (QM), skills with `/` autocomplete and scope badge
(QM, openbot, Grok), cost shown as an allowance with a hard stop (all four),
and the QM-style "lives in Slack, open in Slack" banner as the first Channel
Plane surface.

Worked through on 2026-09-08 — see §17. Skills: done, and the mechanism under
it made real first. Cost: the hard stop is now legible in the room and a false
figure in Activity is corrected; the allowance display is recorded as blocked
with the exact missing pair. Routines: the read side already existed; the room
now says where routines are managed, and creation from the room stays behind
the scheduled-effect gate. Memory: blocked on a plane decision, recorded.
Slack banner: no data source, not started.

## 7. Item 1 implementation, 2026-09-06

Item 1 ("make the org room readable and live") is built across four planes.
The audit above understated the defect: the room's thread LISTING was
owner-bound too, so members of one room did not merely fail to read each
other's turns, they saw disjoint rooms.

### What shipped

**Control Plane** — a new effect class for reading a shared room.

- Migration `026_space_thread_read_effect_policy` adds `thread_read_entitled`,
  deny-by-default, separate from `thread_create_entitled`. Being allowed to
  speak in a room is not the same permission as reading everyone else's turns.
- `IssueSharedThreadReadDecision` mints `model.thread.read` /
  `sha256:thread-read-v1` with permission `thread:read` only, at the `viewer`
  role floor.
- `ResolveSharedThreadReadDecisionEvidence` reuses the current-membership and
  current-recipient-audience joins, so a removed participant stops resolving.
- `POST /api/v1/internal/spaces/thread-read-decision`, scope `spaces:issue`.
  Personal Spaces are refused: their owner already reads them owner-bound.

**Model Plane** — reads can be authorized by Space instead of by owner.

- Migration `0035_message_author_attribution` adds `messages.author_subject_id`,
  written at append time from the verified caller, never a request field.
- `ListConversationRequest` and `ListThreadsRequest` accept a read decision;
  `SessionMessage` returns the author and `ThreadSummary` the thread owner.
- `verify_thread_read_space_decision` checks signature, key, subject, org,
  Space, action, schema, expiry, and refuses a token carrying write
  permissions. Rows stamped with a newer audience revision than the decision
  are dropped.
- Without a decision both reads stay exactly as they were.

**Frontend gateway** — a Space-scoped transcript route.

- `GET /api/v1/spaces/{space_ref}/threads/{thread_id}/transcript`, deliberately
  separate from Chat's route. Chat authorizes by finding the thread in the
  caller's own list, which is right for a personal history and precisely wrong
  for a room.
- The thread listing now mints a read decision too, so a room lists every
  member's posts.
- Control declining (403) or lacking the endpoint (404) means "no shared read":
  the listing degrades to owner-bound, and the transcript route refuses with
  `space_read_not_authorized` rather than silently returning a narrowed room.

**Frontend** — real attribution and a live room.

- Turns are named by the subject the server recorded, resolved against
  Control's roster. An unrecorded author renders as unnamed; the reader's name
  is never borrowed. The old "single human in the roster" heuristic is gone.
- The room polls its thread projection every 6 seconds while the tab is
  visible, and refetches immediately on becoming visible so a returning reader
  never sees a stale room.

### Verified

| Check | Result |
|---|---|
| Control `go build` + new issuer tests | pass |
| session-core read-decision tests | 4 pass |
| gateway Space tests | 61 pass |
| frontend Space + sidebar tests | 103 pass |
| `pnpm typecheck` | clean apart from two pre-existing chat errors |
| Migrations applied on deploy | `0035` and `026` confirmed in logs |
| Live route through the running stack | 403 `space_read_not_authorized`, Control endpoint reached |
| Live poll cadence | 6.00s intervals when visible, suppressed when hidden |

### Not done, and why

- **The org has no `space_effect_policies` row**, so Control declines every
  shared read and the room falls back to previews. Creating that row means
  choosing an organization's `privacy_policy_ref`, `lawful_basis`,
  `retention_class` and `residency`. Those are compliance values, not
  defaults to invent, so they are left to an operator. Note this also means
  shared thread CREATION cannot work in this environment today either, which
  predates this change.
- **Replies by a non-owner** still fail: `append_message` keeps
  `authorize_thread_owner(..., Mutate)`, so a member cannot post into another
  member's thread. Reading was the scope here; shared append is its own slice.
- **Polling, not push.** The durable delivery projection (R-4) remains the
  real answer; this is the fallback that makes the room usable now.

### Found while verifying, not fixed

`SpaceInstructionsSection` issues about ten identical `GET .../instructions`
requests in a single burst every time the Space context resolves. It is
unrelated to the poll (it does not track the 6-second cadence) and predates
this change, but it is worth its own fix.

## 8. Item 2 implementation, 2026-09-07

Item 2 ("in-room approvals") is built, frontend only. Model Plane already owned
the gate and the decision endpoints; the room simply had no surface for them,
so a member saw a count of work needing them and had to leave the page to act.

### What shipped

- **`space-approvals.ts`** wraps the existing orchestration client with the two
  things a room needs and the Agent Run Console never did. A refusal is an
  ordinary outcome, because a run belongs to the member who started it and
  another member can see it waiting without being the one who may answer.
  Nothing is optimistic, because a room shows other people's work and a card
  that vanishes before the server agreed would be an assertion the room cannot
  make. Outcomes are explicit: `granted`, `denied`, `already_decided`,
  `refused`, `unconfirmed`.
- **`SpaceApprovalPanel.tsx`** renders inside the post that is waiting, only
  when the projection names a paused run. It separates "could not load" from
  "nothing pending", says who decides when the caller may not, and reports an
  unconfirmed decision without offering a retry button, since deciding twice at
  a gate is worse than waiting to read the real state.
- **The composer refuses** to send into a thread whose run is waiting, with
  QM's wording ("Approve or deny to continue"). The lock reads the live
  projection rather than what was true when Reply was pressed, so an approval
  raised or settled mid-draft is followed correctly.
- **Activity rows now land on the Chat tab** (`#chat`), where the decision
  surface is. Previously a row for work needing a person dropped the reader on
  whichever tab the URL happened to carry.

### Two verbs, not four

§6.3 recommended the reference products' `Allow once / Allow for this
conversation / Always allow / Deny`. Model Plane records one decision for one
approval and has no scope for a standing answer, so three of those four would
be the same button wearing different labels. The room offers Approve and Deny,
which is what the mechanism supports. The four-verb pattern needs an approval
scope in Model Plane first; until then it would be a promise the backend cannot
keep. A test pins the absence of an "always" control.

### Verified

| Check | Result |
|---|---|
| `space-approvals` logic tests | 14 pass |
| `SpaceApprovalPanel` component tests | 5 pass |
| Room integration tests (decide, composer lock, no false surface) | 3 pass |
| Full Space + sidebar suite | 127 pass |
| `pnpm typecheck` / `pnpm lint` | clean, 0 errors |
| Contrast, measured in the running app, both themes | every string at or above 4.5:1 |

Two contrast defects were found and fixed by measuring rather than eyeballing.
White on `--verevon-accent` is 2.79:1 at 12px, so the Approve button uses
`--verevon-accent-ink` (4.99:1) — the palette's own comment already warned the
raw hue fails as small text. And because this sheet does dark mode by restating
component colours rather than swapping `--verevon-surface`, a token-only card
stayed light on a dark page; the dark rules are now restated the same way the
rest of the room's are.

### Not done, and why

- **Nothing was verified against a live paused run.** The room has no threads,
  and seeding one means fabricating a recipient-audience snapshot in Model
  Plane, which is the same class of invented authority data left alone in §7.
  The card was verified against the running stylesheet instead; its behaviour
  is covered by the tests above.
- **The decision surface is not duplicated into the Activity tab.** Both tabs
  stay mounted, so a second panel would mean two fetches and two sets of
  buttons for one decision. Activity links to the post instead.
- **A denial cancels the run.** That matches the Agent Run Console, and it is
  the honest reading of denying a gated step, but it is a product choice worth
  confirming: the alternative is to record the denial and let the agent try
  another route.

## 9. Item 3 implementation, 2026-09-07

Item 3 ("agent lifecycle in the room") is built across Application Plane, the
gateway and the room UI. Everything except the ask already existed: `paused`
and `revoked` were in the binding schema, the gateway already refused a mention
of a paused binding, and the roster convergence query already excluded revoked
ones. What was missing was any way for a person to request them, so an agent
added by mistake stayed invokable until someone edited the database.

### What shipped

- **`setSpaceAgentBindingStateForGateway`** (Convex) changes one binding's
  standing, and is deliberately narrow about which transitions it will make.
  `pending` is never touched, because it is waiting on Control and the room
  must not assert a membership Control has not granted. `revoked` is terminal,
  because reinstating an agent is a fresh grant that should go through the same
  role gate as the first one. `failed` is not repaired, because it records that
  provisioning did not work. It also re-checks that the binding belongs to the
  Space the caller was authorized for, so a manager of one room cannot govern
  another.
- **`PATCH` and `DELETE` on `/api/v1/spaces/{space_ref}/agents/{binding_ref}`**
  share one path behind the same `require_space_agent_grant_role` gate as
  adding an agent: governing an agent here is the same class of decision as
  granting one. Revocation gets its own verb rather than a third status,
  because it is the one irreversible option and the one that changes Control's
  roster. Only revocation re-declares the roster; pausing keeps the agent a
  member that may not be invoked.
- **The agent card gains Pause / Resume / Remove**, shown only for a settled
  binding and only to a role that may grant. Removal confirms first. A failed
  change says the agent is unchanged rather than leaving the room implying
  something happened.

### Verified

| Check | Result |
|---|---|
| Gateway Space tests | 65 pass, including 4 new lifecycle ones |
| Full Space + sidebar suite | 133 pass, including 6 new room tests |
| `pnpm typecheck` | clean |
| Convex mutation registered on the running backend | argument validation confirmed live |
| Routes live behind auth, `/agents/bind` unshadowed | 401 on PATCH and DELETE, `bind` still routes |
| Contrast, measured in the running app, both themes | every string at or above 4.5:1 |

### A measurement method that was wrong, and what it hid

The dark-mode contrast checks in §8 were taken by adding `dark` to the root
element by hand. The app re-derives that class from its theme preference and
strips it again, so those numbers were partly measured against a light theme.
Re-measured through the app's own theme control, item 2's panel holds up
(approve 4.99, everything else above 8). But the same method also surfaced two
real defects it had been hiding:

- `var(--verevon-dark-text, …)` was used in four places for a custom property
  this sheet never defines. A `var()` whose property is missing is invalid at
  computed-value time for `color`, which INHERITS rather than falling back to
  the earlier cascade value — so those elements silently kept the light theme's
  near-black text. Replaced with `--verevon-sidebar-text`, which every other
  dark rule in the Space block already uses.
- **`.verevon-space-agent` had no dark variant at all**, so the agent card
  rendered as a white card in a dark room. Every sibling surface in that block
  has one; this was simply missed. Fixed here because a control on that card
  cannot otherwise be readable in both themes at once, and the card's name,
  avatar and policy chips needed the same treatment.

### Not done, and why

- **Nothing was exercised against a real binding.** No agents are bound in this
  dev org, and binding one needs the Control policy rows §7 declined to
  fabricate. The controls' behaviour is covered by the tests above and their
  styling was measured against the running stylesheet.
- **Reinstating a revoked agent** is not a button. It is the bind flow, which
  already exists, and routing it through the same grant gate is the point.
- **`muted` from the older drafts is still not a state.** `space-defenition.md`
  resolved it to `paused`, and that is what shipped.

## 10. Item 4 implementation, 2026-09-07

Item 4 ("members and rooms") is built across Application Plane, the gateway and
the room UI. Both halves shipped: a room can be created, and a created room's
people can be managed.

### What shipped

**Rooms.** `POST /api/v1/spaces` now takes a `kind`. `personal` stays the
default so existing callers are unaffected; `room` creates a named channel
through a new Convex mutation. `project` and `case` are refused rather than
guessed at — they are real Space kinds with owners and lifecycles nothing in
the product creates yet. The sidebar gained "New room" beneath the channel
list, which until now could only ever hold one entry.

A created room is deliberately NOT an organization room: it carries no
`isOrganizationRoom` flag, which is what keeps the org-roster sync from
adopting it and replacing its members with the whole organization. It starts
with its creator and grows by explicit grant.

**Members.** A new `spaceMemberGrants` table records who a named room's people
are, and a new sync action declares them to Control with
`managed_subject_types: ["user"]` — so a human roster change can never revoke
the room's agents. `POST`/`DELETE` on `/api/v1/spaces/{ref}/members` sit behind
the same owner/manager gate as granting an agent: deciding who may read a
room's shared record is at least as consequential. The Members tab gained an
"Add people" picker over the organization's own roster, and a Remove control on
each roster row.

Three things it refuses, each because the alternative would be a control that
silently fails:

- **The organization channel has no editor.** Its roster is derived from
  org-core, so a manual list beside it would be overwritten on the next sync
  while appearing to work. The tab says so instead.
- **A personal Space has no editor.** Control refuses to replace its membership
  at all.
- **The registered owner cannot be removed.** Control keeps them as owner
  regardless, so dropping the grant would only make the list disagree with the
  roster it describes.

### Verified

| Check | Result |
|---|---|
| Gateway Space tests | 70 pass, including 6 new room and member ones |
| Full Space + sidebar suite | 142 pass, including 9 new ones |
| `pnpm typecheck` / `pnpm lint` | clean, 0 errors |
| Convex functions registered on the running backend | all three confirmed live |
| **A real room created end to end** | 202 `pending_registration`, Control registered it within seconds, it now lists as `active` with the caller as owner and appears as a second channel |
| Members tab on that room | editor shown, roster rendered once, owner has no Remove |
| Members tab on the organization channel | no editor, and the derived-roster notice instead |

### A bug this found in the existing projection

`GET /spaces/{ref}/context` did not return `is_organization_room` while
`GET /spaces` did, so the Members tab initially offered its editor on the
organization channel — a door the server then refuses to open. The client type
already warned that absence means "unknown, not no", and the first version of
this work ignored it.

Both sides are fixed: the gateway now projects the flag from every Space record
it reads (a real Convex record without the flag is a definite "not the
organization room"), and the UI gates on `=== false` rather than falsiness, so
a response that never carried the field withholds the editor rather than
assuming. A test pins each of the three cases.

### Not done, and why

- **Adding a second person was not exercised live.** This organization has
  exactly one active member, who is already the room's owner, so the picker
  correctly answers "everyone is already in this room". Creating a second user
  is a real identity change, not a test fixture. The add and remove paths are
  covered by the gateway and component tests.
- **The room "Leveranseprosjekt" is left in the dev organization.** It was
  created through the real route to prove the flow, and there is no delete flow
  for a shared room — only personal Spaces have one. Removing it would mean
  database surgery across Application and Control, which is worse than leaving
  a named room somebody can ignore.
- **No role management.** A grant is always `editor`. Promoting someone to
  manage a room is a separate decision that does not exist yet, and inventing
  it here would let anyone who can add a person also create another grantor.

## 11. Item 5 implementation, 2026-09-07

The hygiene pass from §4, plus the doc corrections from §5. Every item in §3.8
is closed.

### What shipped

- **`SpacesIndexPage` is translated.** It had zero `i18n.tr` calls and 15 now.
  It was the one Space surface left in Norwegian only, which put the wall of
  untranslated copy exactly where a first-time user starts.
- **The three hardcoded headings** ("Samtaler", "Aktivitet", "Medlemmer") go
  through `tr()` like the prose around them always did.
- **Native dialogs are gone.** Three `window.confirm` calls and one
  `window.prompt` are replaced: destructive actions use a new
  `SpaceConfirmButton` that arms in place, states the consequence beside itself,
  and disarms on blur or after six seconds — because an armed button that stays
  armed is a trap for the reader who thinks better of it and comes back later.
  Naming a room gets a real field in the sidebar instead of a prompt several
  browsers refuse outright. This follows the settings surface, which already
  answered the same problem by switching a button's own label to "Confirm".
- **The two `formatWhen` functions are one each.** A date-only formatter for the
  timeline and `formatWhenWithTime` for the activity feed, both in the
  presentation lib. They were previously two different functions with the same
  name in different files, which is how one surface starts disagreeing with
  another about what a timestamp means.
- **~12KB of dead CSS deleted**: 22 whole rules plus 35 dead members of rules
  shared with live selectors, across the light, dark and media blocks. All of
  it described the pre-cockpit layout — a room rail, a switcher, room cards, a
  conversation list, a thread rail — that cannot be rendered any more. The
  removal was mechanical and conservative: a selector counted as dead only when
  its base class appeared in no `.ts`/`.tsx` file, and a rule was dropped only
  when every member was dead.
- **The stale `SpaceAgentPanel` doc comment** described the pre-binding design
  where the tab read Control's roster alone.

### Docs corrected

- `SPACE_AGENT_SCOPE_PLAN_2026-08-14.md` claimed the `mention_only` and
  `approval_mode` policy fields "still do not exist on the binding". They do,
  they are enforced, and the room renders them. The three fields that really
  are unstored are named instead.
- `SPACE_COCKPIT_WIRING_2026-08-13.md` and `SpaceCockpit.tsx` both said only
  Chat and Activity had a real Space-scoped source. Four tabs do now; the two
  that still do not are named along with what blocks each.
- `space-defenition.md`'s open question about agents in a personal Space is
  annotated: still open, and now also the reason a personal Space has no member
  editor, since both paths stop at the same Control refusal.

### Verified

| Check | Result |
|---|---|
| Full Space + sidebar suite | 147 pass, including 4 new for the confirm control |
| `pnpm typecheck` / `pnpm lint` | clean, 0 errors |
| No `window.confirm` or `window.prompt` left in the Space surfaces | confirmed by grep |
| No dead Space/room/activity selector left in the stylesheet | confirmed by the same check the audit used |
| Room layout after the CSS deletion | grid, tabs, pulse and canvas all still compute; no horizontal overflow |
| Room-name form, live | opens, autofocuses, submit disabled while empty, Escape and Cancel both close it |
| Contrast, measured live | confirm consequence 5.75:1, form input and Cancel 10.96:1 |

One defect was introduced and caught by looking: the first version of the
room-form styling forced the `--verevon-sidebar-*` palette, which is the
sidebar's DARK-mode palette rather than a permanent one, so the buttons were
near-white text on a near-white panel. They now follow the same resting colour
the sidebar's own rows use, with a `.dark` override.

### Not done

- The two `window.confirm`-free surfaces outside Spaces were left alone. The
  settings page already uses its own in-place confirm; anything else is out of
  this audit's scope.

## 12. Item 6 implementation, 2026-09-07

Item 6 ("unblock Work") is built across Model Plane, the gateway and the Work
tab. The tab had rendered an honest "Model Plane has not published a Space
projection for this yet" since the cockpit shipped, and it was right: the run
listing was per-thread and owner-bound, and the schedule listing was org-wide
with no way to ask about one room.

### What shipped

**Model Plane.** `ListRunsRequest` gained `space_id` plus the same
`model.thread.read` decision fields the conversation read takes. The decision is
REUSED rather than given a permission of its own, and Session Core enforces
that by listing runs THROUGH the threads the decision admits — a join against
`threads`, not a filter on a run column. A run carries no audience snapshot of
its own, so the only honest way to decide whether a reader may see it is to ask
whether they may see the thread it belongs to. Work therefore reaches exactly as
far as Chat and no further. Without a decision the listing is unchanged:
`thread_id` is required and the caller must own it.

The cron listing in `workplane_apis.go` gained an optional `space_ref`. That is
a filter and not a grant: the endpoint has always returned every schedule in the
verified organization to any member of it, and the rows already carried the
column, so selecting a subset can only show less.

**The gateway** composes both into `GET /api/v1/spaces/{ref}/work`, behind the
same lifecycle and membership checks as the thread listing. Runs travel under
the room's read decision; schedules are narrowed by `space_ref`. Either upstream
can fail alone, so the response always carries an `unavailable` list naming what
is missing — present and empty when nothing is.

**The Work tab** renders through the existing Activity grammar, with two new
adapters. A run that is waiting for a person says the same words in Work as in
Activity, ordered by the same consequence rule, because it is the same fact seen
from a different question. Work asks what is in flight, Activity asks what has
happened; a shared vocabulary is what stops the two answers disagreeing about
one run.

Three judgements worth naming:

- **A schedule is not a run.** Nothing has happened yet, so its outcome is what
  it will do, and its row does not pulse as though something were moving.
- **A disabled schedule is shown, and called disabled.** It is part of the
  answer to "what is set up here", and it outranks a healthy one because it is
  the one that may need a person.
- **A partial answer is reported, not hidden.** Showing the runs that loaded
  while naming the missing schedules is the only honest option for a tab whose
  whole purpose is "what needs me". A room with pending work that looks idle is
  the failure this surface exists to avoid.

### Verified

| Check | Result |
|---|---|
| session-core, model-gateway, capability-core builds | clean |
| Gateway Space tests | 73 pass, including 3 new for the Work route |
| Full Space + sidebar suite | 165 pass, including 18 new for the adapters and the panel |
| `pnpm typecheck` | clean |

The gateway tests pin the three cases that matter: the composed answer forwards
the read decision for runs and the `space_ref` for schedules; a failed schedule
read still returns the runs with a named gap; and a declined shared read names
the gap without ever calling Model Plane for runs.

### Two things found by looking at the running app

Both would have passed every test and shipped wrong:

- **Go marshals an empty slice as `null`**, so a room with no schedules sent
  `"schedules": null` rather than `[]` all the way to the browser. The gateway
  now keeps only a real array; anything else becomes the empty list the
  endpoint promises.
- **A gap arrived as an English sentence inside Norwegian copy.** A prose reason
  cannot be translated by the client, so each gap now carries a stable `code`
  alongside it. A known code is said in the reader's language; an unknown one
  falls back to the server's own words, because a gap stated in the wrong
  language beats a gap not stated at all.

### One thing deliberately not encoded

`model-gateway` has no URL-encoding crate, and half an encoder is worse than
none — a `space_ref` carrying `&` or `=` would silently become different query
parameters. The cron proxy therefore refuses a ref outside the URL-safe set
rather than mangling it. Space refs are Convex ids, so this costs nothing real.

### Not done

- **The contracts cache bit again, exactly as documented.** `model-gateway`
  rebuilt after the `runs.proto` edit and `session-core` did not, so the room
  Work listing would have compiled against a contract missing the new fields.
  `mp-contracts/build.rs` already carried the warning and the escape hatch from
  the last time this happened; busting its fingerprint fixed it, and the note
  now records this occurrence too.
- **Not verified against live runs or schedules.** The dev organization has no
  Space-scoped threads and no schedules, for the same reason §7 gave: seeding
  one needs the Control policy rows this work has declined to fabricate. The
  composition is covered by the gateway tests and the adapters by unit tests.
- **No controls.** The tab reads. Cancel, retry and steer exist in the Agent Run
  Console and belong here eventually, but they are writes against runs the room
  may not own, which is a separate authority question from reading them.
- **Monitors and delivery rows remain absent**, as the cockpit's unavailable
  state says: nothing publishes them per Space yet (S4.2, S4.3, S4.4).

## 13. Item 7 implementation, 2026-09-07

Item 7 ("unblock Knowledge") is built across Data Plane v2, the gateway and the
Knowledge tab. The tab had rendered "Data Plane has not published a Space
projection for this yet" since the cockpit shipped, and reading the plane made
clear the gap was much further upstream than a missing endpoint.

### The actual blocker: the write path threw the Space away

`documents-api` already verified a Control-signed Space import decision on every
`POST /v1/documents` (`internal/handler/space_import.go`, ~150 lines of Ed25519
verification, audience, action-id, schema-hash, five revision floors and a ZDR
refusal), answered `X-Space-Import-Authority-Accepted: true` — and then
discarded the `space_ref` it had just proved. The resulting row was
indistinguishable from any other org document.

So there was nothing to list. Not a listing nobody had written: **no edge
existed** between a Space and a document. That is why no Space-filtered query
could be added without inventing an authority, which the binding table's own
contract forbids ("A name match or a client-supplied workspace filter is never a
mapping").

`documents.space_ref` is that edge, and it is deliberately narrow:

- written ONLY from the verified decision's own claims. `CreateDocumentInput`
  carries the field as `json:"-"`, following the `VisibilityFromSource`
  precedent already in that struct, so a body-supplied `space_ref` cannot
  deserialize at all. A test asserts exactly that.
- written on **create only**. A re-POST under Space authority refreshes content
  through the existing update branch but never re-homes an existing document
  into another room — that would move data across a membership boundary on the
  strength of an import, which is not what an import decision says.
- NULL for everything that came before. Org-wide documents are not
  retroactively assigned to a room.
- stamped identically in both repo create paths, so which one a caller happens
  to use cannot decide whether the edge is recorded.

### Two defects found by reading the retrieval path

Both were silent, both returned `200`, and both would have made the new listing
look like it worked.

**The Space binding named targets the document vertical does not carry.**
`resolve_space_retrieval_scope` pins `workspace_id` / `collection_id` into the
dense arm's Qdrant filter. Those are payload keys that **only the wiki consumer
writes** (`embedding-engine-rs/src/wiki_consumer.rs`); a document chunk's
payload is `{document_id, org_id, title, source, type, chunk_tokens}`. Every
Space-scoped document search was therefore filtering on a key no point has, and
answered every query with zero candidates while reporting success. The scope now
also carries the Space's own `document_ids` — the one predicate the payload does
carry — and an empty Space **refuses** rather than clearing the filter, because
an empty id list means "unconstrained" downstream and would search the whole org
under Space authority. Above the cap the vector search refuses too — keeping the
newest N would narrow a room's retrieval to a subset no reader can see or
predict. That refusal is deliberately raised where the prefix would be USED, not
at scope resolution: a large room must still list its archive and still answer a
by-id lookup, both of which read Postgres and are exact.

**A multi-value filter axis matched nothing at all.**
`RetrievalFilters::to_qdrant_conditions` pushed one condition per value, and
every condition lands in the dense arm's `must` list — so two document ids meant
"this chunk's `document_id` equals both", which no chunk satisfies. Any
multi-document, multi-source, multi-workspace or multi-`acl_tag` filter was
returning an empty result rather than the union. Each axis is now one any-of
condition: OR within an axis, AND across axes, which is what a filter list means
everywhere else in this codebase (`AuthContext::intersect_filter` builds exactly
that). Four tests pin it, including the two-ANDs regression by name.

The second one is worth dwelling on: it is not a Space bug. It was there for
every caller with more than one value on any axis, and it was invisible because
"no results" is a legitimate-looking answer.

### What shipped

**Data Plane v2.** `documents.space_ref` (migration + `init.sql`), stamped by
`documents-api` from the verified decision. `POST /v1/knowledge/space-sources`
in retrieval-engine lists a Space's documents (through `space_ref`) and the
published wiki pages of the workspace its binding names — under the same
Control `retrieval.read` decision a grounded room turn already uses. There is
no org-wide form of that endpoint: it cannot be asked without Space authority.

`space_decision_is_enforced_for_path` widened from three paths to ten, and each
addition came with real enforcement rather than an allowlist entry:

- `sources` and `freshness` carry the Space as a SQL predicate
  (`AND ($5 IS NULL OR space_ref = $5)`), so the enforcement is exact and
  unbounded. Narrowing the requested ids against a resolved set would have
  been simpler and wrong: an id list has to be capped, and a capped list
  silently drops documents that really are in the room;
- `wiki` pins the search to the workspace the binding names, and **refuses** a
  binding that names none — `wiki_search` treats an empty workspace list as
  unconstrained, so passing one through would widen a Space-scoped wiki read to
  the whole org;
- `space-sources` exists only under Space authority.

`graph`, `pack`, `timeline`, `contradictions`, `chunks` and `compare` are
deliberately still absent, so a decision presented to them is still refused at
the boundary. Presenting a decision to an endpoint that would drop it is how a
Space silently becomes a hint.

**The gateway** composes `GET /api/v1/spaces/{ref}/knowledge` behind the same
lifecycle and membership checks as the thread and work listings. It asks Control
for `retrieval.read`, relays the decision to Data as `x-space-decision`, and
relays Data's own per-section gaps rather than restating them — Data knows which
half of its answer is missing and why. Carrying that header needed a new
`proxy_user_bearer_json_with_extra_headers`, which refuses to let an extra
header overwrite `authorization`, `x-org-id`, `x-internal-api-key` or any
`x-user-*`: an upstream must never learn who the caller is from a value the
gateway was handed.

**The Knowledge tab** renders documents and wiki pages under separate headings,
each with its own gap, and shows the bound workspace on screen. That last one is
deliberate: which archive a room reads from is the most consequential fact about
its answers, and a room bound to a workspace nobody expected produces
confidently wrong grounding that is only diagnosable if the binding is visible.
`documents_truncated` says a capped list is a page rather than the room, because
quietly showing the first fifty of five hundred is how someone concludes a
document is not in the room.

### Three judgements worth naming

- **A declined read is not an error.** Retrieval is entitled separately from
  chat (`retrieval_read_entitled` is its own policy bit, default FALSE), so most
  rooms will answer exactly this way. Failing the request would make an unset
  entitlement look like a broken feature; the tab states it as a permission.
- **Control verified and Data refused** means the Space has no active binding —
  a nameable state, and the one the dev organization is actually in. It gets its
  own code, distinct from an outage.
- **A Space never widens an ACL.** The per-viewer ownership predicate runs in
  the listing exactly as it does in `retrieve_sources`. Being in a room is
  permission to ask; it is not permission to read a private document another
  member owns.

### Two defects found by looking at the running app

**Every tab panel was firing a burst of identical requests.** Measured live on
the org room: 17 `/knowledge` calls, 14 `/work`, 15 `/instructions` — six of
each inside 3ms per context resolve, and six more on every 30s membership
recheck. `SpaceCockpit` took its panels as a `tabs` object and read
`props.tabs?.[tab.id]` twice per tab (once for `<Show when>`, once for the
child) across six tabs. `props.tabs` is a getter over the caller's object
literal, and Solid JSX constructs a component eagerly — so each read
re-MOUNTED all six panels, and any panel that fetches on mount re-fetched.
Memoizing the tabs object and each tab's content took it to exactly one call
per panel per context change: `{"threads":4,"context":1,"work":1,
"knowledge":1,"instructions":1}`.

Two things worth naming about this. It is the same `/instructions` burst that
was already spun off as its own task — the cause was never in
`SpaceInstructionsSection`, it was in the component that mounts it. And I
shipped `SpaceWorkPanel` into this bug in item 6 and did not notice, because I
verified the route returned 200 and never counted the requests. A regression
test now asserts a panel is constructed once however many tabs read the object.

**The bound-workspace chip failed AA in both themes.** Measured through the
app's own toggle: 5.64:1 in light and 3.37:1 in dark at 11px, because the
`<code>` inherited the muted colour of the sentence around it while sitting on
a tinted background. The workspace id is the one part of that line a reader
actually compares against a config value, so it was the least readable thing on
the row. With its own ink: 16.01:1 light, 12.34:1 dark.

That measurement took three attempts, which is its own lesson. Chrome returns
`color-mix()` results as `color(srgb 0.87 0.48 0.12 / 0.22)`, and canvas
`fillStyle` does NOT normalize that form — reading its 0–1 components as 0–255
bytes made every tint look almost black and reported a false 1.20:1 failure on
a badge that was actually fine. A contrast check also has to composite
semi-transparent backgrounds over their first opaque ancestor rather than treat
them as opaque. Both are now handled.

### One defect of my own, caught by a test

`<Show when={document.status && document.status !== 'completed'}>` yields the
BOOLEAN `true`, not the status string, and `<Show>`'s callback form hands that
straight to the child — where a boolean renders as nothing. The status badge was
silently absent for every unready document. A ternary fixes it, and the comment
now says why the ternary is load-bearing.

### What remains open

- **Not verified against live documents.** The dev organization has no
  `space_effect_policies` row, no registered recipient audience and no
  `space_retrieval_bindings` row, so Control declines the retrieval decision and
  the tab renders `knowledge_read_not_authorized`. I have again declined to
  fabricate those rows: they carry an organization's `privacy_policy_ref`,
  `lawful_basis`, `retention_class` and `residency`, which are compliance
  values, not test fixtures. End-to-end behaviour is covered by tests.
- **No import yet writes a `space_ref`.** The column and the stamp exist; the
  Imports Core path that carries a Space import decision is what will populate
  it. Until then every room's document list is legitimately empty, and says so.
- **The vector-search fix needs an indexed Space predicate to scale.**
  Resolving a Space's documents into an id list is correct and works with
  existing vectors, but it refuses above 2 000 documents rather than
  truncating. Writing `space_ref` into the chunk payload at index time would
  remove the bound — at the cost of a reindex, which is why it is not this
  change. Listing and by-id lookups are unaffected: those read Postgres, where
  the predicate is a WHERE clause.
- **`documents` still has no workspace or collection column.** A binding that
  names only a collection can therefore bind wiki pages but no documents. That
  is now stated as a gap rather than rendering as an empty room.

### Verification

- retrieval-engine-rs: `cargo clippy --all-targets` clean; 11 tests across
  `space_scope` and `search::filters` pass (7 new).
- documents-api-go: `go vet` clean, handler and model suites pass, plus a new
  test that a body-supplied `space_ref` cannot deserialize.
- `check-tenant-isolation.sh`: none of the new SQL is flagged (the 14 findings
  are pre-existing, in `index-engine-rs` and `documents-api-go`).
- gateway: 78 `domains::spaces` tests pass (5 new).
- frontend: `pnpm typecheck` clean, `pnpm eslint` clean on the new files, 158
  `features/spaces` tests pass (10 new). The 7 failures elsewhere in the repo
  (`features/agents`, `features/ingestions`, `features/support`, and two files
  that fail to load) are pre-existing and in directories with no local
  modifications.
- Migration `20260907120000_document_space_ref` applied; column, partial index
  and check constraint confirmed in the running Postgres.
- Live at `/spaces/p574enc94gj1c99ejjtet33zrn8dx21e#kunnskap`: the route answers
  200 with `documents: []`, `wiki_pages: []`, `binding: null` and one gap
  `{section: "knowledge", code: "knowledge_read_not_authorized"}`, and the tab
  renders "Kunnskap: Du kan ikke lese kunnskapen i dette rommet. Tilgang til
  kunnskap gis separat fra samtale." One request per panel, and every new class
  measured in both themes.

### A deployment note, since it cost real time

Two hazards, both of which look like success:

- Data Plane v2's base compose file uses a PRIVATE `inter-plane-bus` bridge, so
  `docker compose up -d <service>` recreates a container that cannot resolve
  `control-shared-nats`. `documents-api` treats that as fatal and crash-loops;
  six other services log a retrying error and run with GDPR erasure silently
  dead. The file says so in a comment, and I did it anyway. The connected
  posture needs `-f docker-compose.yml -f docker-compose.cross-plane.yml`.
- The verevonv3 frontend container runs `pnpm dev` over a Windows bind mount,
  and file-watch events do not cross it — Vite stays happily up while serving
  the pre-edit modules. Every source or CSS change needs a container restart
  before it is on screen, which is why the first two live checks showed the old
  placeholder.

## 14. Item 8, 2026-09-07 — and why three quarters of it is not startable

Item 8 as I wrote it ("the plan's own order: Activity receipts (S2.5), watches
(S4.3), UI-2c delegation, Slack adapter (S5.3)") was under-specified. Checking
each against the plan's own dependency graph before writing anything: **one of
the four is startable, and the other three are blocked two to four sequences
upstream.** Naming them as a flat "next" list was my error in §4.

### The three that are gated, with the evidence

**UI-2c delegation** — gated, and the scope plan's own analysis still holds. I
re-verified the specific claim rather than trusting the note: execution-core's
only HTTP client is still `user_core_client.rs`, and it has no network path to
`verevon-gateway-rs`. The trap is unchanged too — flipping the gateway to
request the `agentic` feature for an `approval_mode: auto` binding would not
narrowly grant "may delegate", it would hand that room agent execution-core's
entire capability surface, because no delegation-scoped capability exists to
narrow it. Not started, deliberately.

**S4.3 watches** — blocked by `S4.2 Process registry`, which is blocked by
`S3.3 Durable workspace`, which is blocked by `S3.2 Space sandbox lease` —
whose own execution evidence still carries an unchecked box ("Bind this
substrate truth to a signed Space capability profile and backend-pinned lease").
S4.3 also depends on `S2.7 provenance/screening`. Its first two adapters are
process output and Model runs, and neither has a Space-projected event stream to
read.

I could have built slice 1's Watch record in Convex. I did not, and the reason
is the rule this codebase already holds itself to: with no adapter to feed it
and no `S4.4` delivery to send from it, a watch record can never fire. A durable
store whose only possible content is an intent nobody acts on is "built but
never wired" with a schema attached — strictly worse than the honest gap,
because the gap is at least legible. Activity and Work now name watches as
unpublished, with a stable code, in the reader's language.

**S5.3 Slack adapter** — `S5` depends on `S4.4 durable delivery`, which depends
on S4.3, so it inherits the whole chain above. S5.1's ADR *is* adopted, but its
own execution note says "the next adapter remains gated on the listed
security/contract tests and review", Channel Plane is docs-only, and CLAUDE.md
forbids building against it as if runtime exists. Even §6.3's cheap first form —
QM's "this conversation lives in Slack, open in Slack" banner — has no truthful
data source: nothing records that a room has an external surface, so the banner
could only ever be a decoration that lies.

### The one that was startable, and what unblocked it

`S2.5 Unified Activity` depends only on `S2.3`, whose slice 5 —
"correlate the owner event into Application Space Activity" — is the single
unchecked box on an otherwise-complete owner-receipt vertical. `tickets.create`
is a real deployed receipt with an operation ledger, a Control-audit outbox, a
reconcile route, and V3 already rendering it as an **Operation**.

The correlation looked impossible at first: `conversation_ticket_operations` has
no `space_ref`. It does not need one. An operation is bound to the exact owner
grant it committed against (`grant_ref`), and `conversation_agent_action_grants`
carries the `space_ref` Control decided. Joining those two answers "which
effects happened under this room's authority" exactly, with **no new column and
no invented edge** — the opposite of item 7, where the edge genuinely did not
exist and had to be created.

A human-created ticket has no grant and no Space, so it correctly does not
appear: it did not happen under a room's authority.

### What shipped

**Conversation Core** gained two Space-scoped reads and
`GET /api/v1/spaces/:space_ref/activity` on the existing `verevon-gateway`
reader group. Judgements worth naming:

- **Revoked grants are included.** The effect really happened while the
  authority was live, and dropping it once the grant was withdrawn would
  quietly rewrite the room's history. A withdrawn grant is the more useful row
  of the two: "this agent could write tickets here until Tuesday" is what
  explains a refusal after the fact.
- **The Space gate belongs to the caller.** Conversation Core does not know
  Space membership and must not grow a second opinion about it; V3's gateway
  checks lifecycle and current Control membership first, exactly as `/work` and
  `/knowledge` do. This is a NARROWING, not a grant:
  `GET /ticket-operations/:key` already answers to any org member for any
  operation in the org, so selecting the subset bound to one Space can only show
  less.
- **A `completed` receipt missing its ticket or audit id is refused**, not
  rendered. The ledger's CHECK constraint already forbids it; the service
  re-asserts it rather than trusting the read, because a receipt claiming an
  effect landed without naming it is the one thing this surface must never show.

**The gateway** composes `GET /api/v1/spaces/{ref}/activity` from five sources
under three different authorities: runs through Control's `model.thread.read`
decision (wider limit than Work, terminal runs kept), approvals per gated run,
and the owner receipts plus authority events from Conversation Core. Approvals
are asked for **only** on runs actually waiting on a person, capped at ten — a
fan-out bound, not a display limit, since approvals are a per-run read upstream.

**The Activity tab** is now a real component instead of an inline block in
`SpacePage`. Five sources, one salience ordering, so "what needs me" is the top
of the list regardless of which plane it came from. Two deliberate details:

- **`unknown` outranks failure.** It is the only state meaning "we cannot tell
  whether this effect happened"; a failure at least resolved. A test proves a
  newer completed operation still loses to an older unknown one.
- **The lens says what it hides.** Every filter shows the count it excluded. A
  supervisor who narrowed to one class and read the list as complete would have
  been misled by their own control, and Activity is where that matters most.

Runs also finally show their cost: `RunDetail` has carried `input_tokens`,
`output_tokens` and `steps_completed` since the contract was written and the
Work tab never read them. Deliberately tokens rather than a currency figure —
pricing is Control-owned, and a browser-side multiplication would be an invented
number.

> **Corrected 2026-09-08 (§17).** The token half of this was false in
> practice: session-core does not track token usage and reports both fields
> as a literal `0` for every run, so this line rendered "0 tokens" under
> every run in the room. Only `steps_completed` was ever real. The adapter now
> treats a zero total as "no figure" and prints nothing.

### Three defects found on the way, none of them in the new code

**Conversation Core could not verify a Control decision at all.** Its config
validator and both decision verifiers decoded the Control public key with
`RawStdEncoding`/`StdEncoding` only, while Control mints it base64**url** — and
while the very same files decode the token's own envelope with
`RawURLEncoding`. Every sibling consumer of that key is URL-safe: Data Plane's
`space_scope.rs` uses `URL_SAFE_NO_PAD`, `documents-api` uses
`base64.RawURLEncoding`. So a real key containing `-` or `_` refused to start
the service, and had it started, no run-action or owner-grant decision could
ever have verified. It surfaced because recreating the container finally gave it
a Control-derived key; it had been up for seventeen hours without one. After the
fix both lanes report configured for the first time:
"Control owner grant decision verifier configured" and "Control ticket decision
verifier, current-authority validator, and owner-effect reservation coordinator
configured".

**A nil slice marshals as `null`.** Caught by a test I wrote for the contract
rather than by reading the code — the same defect as item 6's `schedules: null`,
in a different language. Fixed in the service, which is the layer that makes the
promise.

**The Activity tab claimed the room was empty while still loading.** The
loading line and "no activity has been published" were briefly on screen
together: two contradictory claims about one room. A read in flight is not an
empty room, the same way a failed one is not — the panel had the second half of
that rule and not the first.

### One of my own, measured rather than assumed

The selected filter lens failed AA at 12px: `--verevon-accent-ink` on a 12%
accent tint measures **4.45:1**, and 4.72:1 even with the tint halved — a margin
one accent tweak would erase. Plain `--verevon-text` on the same tint is
15.51:1, so the accent now carries the selected state in the border and
background and the label keeps the panel's ordinary ink. Border, tint and weight
still make the selection unambiguous. Dark mode: 13.79:1 active, 8.28:1 idle.

### What remains open

- **Not verified against a real owner effect.** The dev organization has no
  `conversation_agent_action_grants` row, so no agent has ever been granted
  `tickets.create` in a room and there is nothing to correlate. Live, the tab
  answers 200 with `operations: []` and `authority: []` — real empty lists from
  Conversation Core, not gaps — plus three honest gaps. End-to-end behaviour is
  covered by tests at every layer.
- **Runs are still unreadable in the dev org**, for the same missing
  `thread_read_entitled` policy row as items 1, 6 and 7. The tab says so.
- **`tickets.create` is the only correlated action.** S2.3's own note is
  explicit that the remaining ticket actions and non-ticket dispatchers are not
  operation-envelope compliant, so nothing else has a receipt to correlate. The
  join is action-agnostic and picks up new ones for free.
- **§4's item 8 should be rewritten** as what it actually is: S2.5 done, and
  three items whose real blockers are S3.2 → S3.3 → S4.2 for watches, that
  chain plus S4.4 for surfaces, and an inter-plane network path for UI-2c.

### Verification

- conversation-core-go: `go build` and `go vet` clean, full `go test ./...`
  passes, 5 new Space-activity service tests plus 2 for the key encoding.
- gateway: 490 tests pass (6 new in `domains::spaces`).
- frontend: `pnpm typecheck` and `pnpm eslint` clean, 170 `features/spaces`
  tests pass (12 new in `SpaceActivityPanel`).
- Live at `/spaces/p574enc94gj1c99ejjtet33zrn8dx21e#aktivitet`: the route
  answers 200 with the three expected gaps, the tab renders them in Norwegian
  ("Kjøringer: Du kan ikke se det andre medlemmer har kjørt i dette rommet.",
  "Levering: Leveringstilstand publiseres ikke ennå.", "Overvåkinger:
  Overvåkinger publiseres ikke ennå."), the lens control works, and every new
  class was measured in both themes.

## 15. Item 1b, 2026-09-07 — the room is visibly working, and Stop is a stop

Item 1b as the reference check phrased it: a live "agent is working" signal on
the sidebar row, the post and the composer bar, plus Stop recorded as a stop.
Both halves turned out to need no backend work at all — everything they need
already existed and had simply never been wired together.

### What was true before

- A member who sent a message could only wait. The composer had no Stop.
- Every *other* member saw nothing while a turn streamed. The sender had a live
  exchange in their own composer; the rest of the room saw a finished reply
  arrive on the poll, and nothing before it. The room went dark exactly when it
  was busiest.
- The sidebar's "Jobber" label was true at the moment the panel mounted and
  stale for the rest of the session: it fetched the thread list once and never
  again.
- A recorded stop rendered as the untranslated enum token **"Cancelled"** in a
  Norwegian room — `threadStatus` had no case for it.
- The pulse signal never lit in Norwegian. Its lit style was keyed on
  `[aria-label="Active work"]` — the *English* label — so in the default locale
  the selector never matched, however busy the room was. Style must never read
  a translated string.
- `.verevon-space-status--active` had been defined since the cockpit shipped
  and was applied by nothing, so a working post read in the same grey as a
  finished one.

### Why Stop needs two calls, in this order

Model Plane's disconnect handling deliberately *detaches and finishes* a
stream whose client went away, so a closed tab can resume. That is correct for
resume and exactly wrong for Stop: an `AbortController` alone leaves the run
generating and its status `running` for every other member. The recorded stop
is the second call — `POST /api/v1/chat/invocations/{requestId}/cancel`, which
the gateway proxies to model-gateway's owner-bound cancel registry, which flips
the stream's cooperative flag, emits `stopped`, and calls
`cancel_direct_inference_run_authenticated` so Session Core writes
`cancelled`. That is what the room's projection shows on its next poll.

The `requestId` arrives on the stream's `connected` frame. If Stop lands before
it there is nothing to cancel with, and the composer says so plainly — "cut off
before the reply started; the run may still finish server-side" — rather than
claiming a stop it cannot prove. A cancel the gateway refuses stays in the
"stopped here, recording…" wording and never flips to recorded. And a trailing
`done` after `stopped` no longer overwrites it; a server-side stop used to be
routed to `onDone` and render a halted answer as a finished one.

### One shared refresh path, not a second poll

`space-live-work.ts` is a module-level Solid store. `SpacePage` publishes the
projection it already polls every six seconds; the sidebar and the composer
read it. Nothing fetches twice, nothing runs a second timer, and the two
surfaces cannot show two different answers to one question.

Two judgements in the store worth naming:

- **Absence is not idleness.** The store only holds rooms whose page has
  published. An unobserved room returns an empty list, and the sidebar draws no
  dot for it rather than a calm one — a test asserts exactly that, against a
  mounted thread list that still says `running`.
- **The browser's own stream and the server's projection stay separate.** The
  sender's indicator is immediate; everyone else's arrives on the poll. Merging
  them would let a local signal masquerade as a server fact.

"Working" is `queued | running`, deliberately *not* `awaiting_approval`. A run
paused on a person is active but nobody is producing anything; calling it
"working" would make a blocked room look busy — the opposite of the truth.

### Two defects found on the way

**Two literal NUL bytes were committed in `SpaceApprovalPanel.tsx`.** Where the
source meant the two-character escape `\0` as a resource-key separator, an
earlier Python patch had written real NUL bytes — the same root cause as the
`grpc.rs` incident. It ran fine (a NUL inside a template literal is a valid
string, and `split` on the same NUL matches), which is why every gate passed
and commit `aae03af5` carries it; but git treats the file as binary, so every
future diff of that component was blind. `grep` calling a `.tsx` file "binary"
was the tell. Fixed to `\u0000`, the form `SpaceRoomTimeline` already uses.

**The room post and the exchange are white cards in dark mode.** The whole
sheet has no `.dark .verevon-room-post` or `.dark .verevon-space-room-exchange`;
they stay `--verevon-surface` (white) with their own text correctly dark-on-
white. My first draft gave the new working row, Stop and stopped line white
text — measured 1.0 / 1.1 / 1.0 against the real page. The honest fix here is
to match the card they sit on (now 6.13 / 17.4 / 6.13 in dark); re-skinning the
card surfaces themselves is the same class of defect as item 5's agent card
and needs its own measurement pass over every existing turn, so it is recorded
here rather than done.

### A verification hazard, since it cost a false conclusion

After the CSS fix, `docker restart` plus a page reload measured the *old*
numbers to the pixel, and I nearly concluded the rules were wrong. Vite was
serving the new sheet — `curl` proved it — but the browser was holding
`global.css` in its HTTP cache. `fetch(url, { cache: 'reload' })` before the
reload cleared it and the new numbers appeared. The Vite-restart note in memory
now carries this second step.

### Verification

- Frontend: `pnpm typecheck` clean; lint 0 errors, and the three touched files
  carry 6 `solid/reactivity` warnings against 9 on their committed versions —
  none introduced. 255 tests pass across `features/spaces` and `features/core`
  (29 new: 6 store, 10 composer, 6 timeline, 3 sidebar, plus 4 lines of
  `flush()` where Solid 2's deferred writes would otherwise read stale).
- Live at `/spaces/p574enc94gj1c99ejjtet33zrn8dx21e#chat`: idle state renders
  exactly right — pulse `data-active` absent, no Stop, no working line, no
  dots. Every new class probed against the real stylesheet in both themes:
  light 6.13 / 4.99 / 17.4 / 6.13 / 6.13, dark 6.13 / 4.99 / 17.4 / 6.13 /
  8.28, pulse lit and animating under `data-active`, sidebar dot at the row's
  trailing edge. Not driven with a real message: sending one would write into
  the org room, and the stop chain is covered end to end by tests instead.
- Not addressed, deliberately: 2b's four approval verbs. Item 2 recorded why —
  Model Plane records one decision per approval and has no scope to remember a
  standing answer, so "Always allow" would be a button that lies.

## 16. Item 4b implementation, 2026-09-08

§6.3 called this "small, and it is what makes the sidebar read as channels".
Small is right for the UI. It was not small underneath: none of the three
facts — *new since you were last here*, *pinned*, *what this post is called*
— had an owner, and each turned out to belong to a different plane.

### What was true before

- Nothing anywhere recorded that a member had seen a room. There was no
  "new" to show, so the sidebar's thread rows were a flat list of names that
  looked identical whether the room had been quiet for a week or three
  colleagues had posted since lunch.
- `pinned_at` existed in Session Core's thread listing and ordering
  (`pinned_at IS NOT NULL DESC`) and the gateway's personal-chat path could
  set it, but the Space projection dropped the flag and no room surface
  showed or set it.
- The AI-generated title was an SSE `title` event and nothing more: Chat
  keeps it in a browser-local snapshot. In a room that meant the *sender*
  saw "Innkjøp av pallevekt" while every other member saw the raw first
  message as the post's name, because the room's listing reads
  `COALESCE(presentation_title, first_user.content)` and nobody had written
  the first half.

### Three owners, named

**Unread lives in Application Plane.** Threads are Model Plane's, membership
is Control's, and neither owns "what has this person seen" — that is a
workspace projection, which is Application's job by the ownership matrix.
`spaceReadMarkers` stores one row per member per room: `spaceRef`,
`externalOrgId`, `externalAuthId`, `lastReadAt`. Only a timestamp against
identifiers — never a thread list and never content — so it says nothing
about *what* was read, only *when*, and Zero Data Retention has nothing to
propagate through. One marker per room rather than per thread because the
room renders every post inline: a per-thread marker would be a write per
post per visit for no extra truth, and it is not what Slack does either.

**Pin stays owner-bound, on purpose.** Session Core's
`UpdateThreadPresentation` authorizes through `authorize_thread_owner …
OwnerIntent::Mutate`. The gateway's new `PATCH
/spaces/:ref/threads/:id/presentation` checks room lifecycle and membership
first, then relays an upstream 403/404 as `thread_presentation_owner_only`.
The room shows the pin to everyone and the pin *control* only to the post's
author: a button every other member could press and be refused every time
is a control that lies. A room-wide pin ("any member may pin any post") needs
a Space-authorized presentation write in Session Core — a decision-bearing
path like `model.thread.append` — and is recorded here rather than faked by
having the gateway impersonate the owner.

**The title becomes a room fact.** When the composer opened the thread
(`openedNewThread`) and the stream's `title` event arrives, the composer
writes it back through the same presentation route, then asks the page to
re-read. The sender is by construction the owner, so the owner-bound write
is exactly right here. Other members see the name on their next poll. Chat's
browser-local snapshot is untouched; a personal thread has no other reader
to disagree with.

### What shipped

- Application Plane: `convex/schema.ts` `spaceReadMarkers` with
  `by_space_and_subject`; `convex/spaceReadMarkers.ts` — `unreadThreadIds`
  (pure, tested), `advanceMarker` (never moves backwards),
  `spaceReadMarkerForGateway`, `markSpaceReadForGateway`, both behind
  `assertServiceKey` + `requireGatewayMember`.
- Gateway `domains/spaces.rs`: `GET /spaces/:ref/threads` now carries
  `read_marker` and, when Convex cannot answer, an `unavailable` entry with
  code `read_marker_unavailable` instead of a guessed marker; `POST
  /spaces/:ref/read` (503 `read_marker_unavailable` on failure); `PATCH
  /spaces/:ref/threads/:id/presentation` with `{ title?, pinned? }`, 400
  `invalid_presentation` for an empty body or a title over 200 characters.
  `chat/history.rs`'s `update_durable_presentation` is reused, not copied.
- `spaces-client.ts`: `SpaceThread.pinned`, `SpaceReadMarker`,
  `updateSpaceThreadPresentation`, `markSpaceRead`.
- `features/spaces/lib/space-unread.ts`: the browser mirror of the Convex
  derivation, so the room badges the moment the listing arrives rather than
  after a second round trip. The file says which side owns the rule.
- `SpacePage.tsx`: snapshots the marker on *arrival* in a room and derives
  the unread set against that snapshot; advances the durable marker on each
  visible listing (having the room open is reading it); publishes the set
  through the item-1b store so the sidebar badges the same rows.
- `SpaceRoomTimeline.tsx`: "Festet" and "Ny" flags stated *before* the
  post's content; author-only Fest/Løsne control that re-reads rather than
  flipping a local copy; a refused pin says "Ingenting ble endret".
- `CoreSidebarSpacesPanel.tsx`: pinned rows swap the glyph for a pin rather
  than adding one, so the row does not widen; a neutral unread dot that
  yields to the working dot when both apply — work in progress is the more
  urgent fact. The label reads "Festet. Ny siden sist." before the status.
- `SpaceRoomComposer.tsx`: persists the opening exchange's title as above.

### Four judgements worth naming

- **Badges are pinned to arrival, not to the live marker.** The marker
  advances every six seconds while the room is open. If the badges read the
  live marker they would all vanish on the first poll, before anyone had read
  anything. The page snapshots the marker once per room and holds it.
- **A first visit badges nothing.** There is no last visit to be new since,
  and badging a hundred posts at once teaches people to ignore the badge. The
  marker is still recorded so the *next* visit has one.
- **Your own posts are never new to you**, and a post with no readable
  timestamp is left alone rather than guessed at.
- **An unreadable marker is a named gap, not a zero.** Convex down must not
  read as "you have seen everything": the listing says `read_marker_unavailable`
  and the room badges nothing, the same shape items 6 to 8 use.

### What remains open

- **Cross-room unread in the sidebar.** The sidebar only badges the room whose
  page is open, because the derivation needs that room's thread listing and
  the sidebar must not start a poll per room. A "3 unread in Lager" badge on a
  room you are *not* in needs a per-Space activity aggregate from Model Plane
  (latest activity per Space, one call) joined to the markers — the same
  missing aggregate that blocks the Work tab. Recorded, not approximated.
- **Room-wide pin**, as above: Session Core presentation write with a Space
  decision instead of owner-only.
- **Unpin/retitle by anyone but the author**, same root.

### Found while verifying, and fixed

**In dark mode, every sidebar row except the active one was invisible.**
`.core-sidebar-panel-link` paints its text in `--verevon-dark-border`, which
resolves to `#34363d` — the same value as `--verevon-dark-sidebar`, the
sidebar's dark background. Measured 1.0:1 on the real "Nytt rom" link and the
real non-active org-room link, not only on my probe rows; the active row
read 9.41 only because `.dark .verevon-sidebar-panel-active` overrides it. It
went unnoticed because the org room has never had a thread, so the row
type this item adds is the first anyone would look for there in dark. Fixed
with a `.dark .core-sidebar-panel-link` rule placed *before* the active
override so the active row keeps winning at equal specificity, plus icon and
hover variants; rows now measure 10.99 in dark. The dashed empty-state
paragraph on the same surface read 1.88 in dark and got the same treatment
(9.13). Its *light* value is 3.22 against a 4.5 target — pre-existing, in the
light sidebar palette, and recorded here rather than changed in passing.

**The pinned flag cleared AA by 0.04.** Accent ink on a 10% accent tint over
the white card measured 4.54:1 at 11px. The tint is now 6% and measures
4.72. Not a failure, but a margin that thin is one palette tweak from one.

### Verification

- Application: 7 new `node:test` cases (`test/space-read-markers.test.cjs`),
  67 pass in the package; functions pushed and "Convex functions ready!"
  confirmed after a `convex-gateway` restart.
- Gateway: 7 new tests through a wiremock Convex routed on `body_partial_json`
  (marker present / null / unavailable, mark-read records the moment and
  nothing else, presentation validation, owner-only relay, membership gate);
  91 pass in `domains::spaces`.
- Frontend: `pnpm typecheck` clean; lint 0 errors, 7 `solid/reactivity`
  warnings of which 6 exist at HEAD and the seventh was mine and is fixed.
  142 tests pass across the nine touched files (6 `space-unread`, 7 timeline
  hygiene, 2 sidebar pin/unread, 3 composer title, 2 page arrival/mark-read).
  The two page tests first failed for a reason worth recording: they never
  set the context mock, so the page rendered an empty section — the
  fixture's fault, not the feature's — and then found the post's title twice,
  because the header's activity pulse names the same post. Scoped to the
  timeline list.
- Live at `/spaces/p574enc94gj1c99ejjtet33zrn8dx21e#chat`: the redeployed gateway answers `GET /threads` with
  `read_marker` and an empty `unavailable`; the marker was `null` on this
  member's first read, `POST /read` recorded the moment and returned it, and
  the next `GET` carried it back — the round trip through Convex is real.
  `PATCH …/presentation` with an empty body returns 400 `invalid_presentation`
  ("A title or a pin state is required"). The room has no threads, so the pin
  and title paths were not driven live; they are covered by the gateway and
  component tests and a real run would write into the org room. Every new
  class was probed against the live stylesheet in both themes after a
  cache-bypassed reload — light / dark: pinned flag 4.72 / 4.72, new flag
  14.82 / 14.82, pin control 6.13 / 6.13, unpin control 4.99 / 4.99, refusal
  line 6.48 / 6.48, sidebar thread label 10.96 / 10.99, sidebar pin glyph
  4.54 / 4.33 (graphic, 3.0 target), unread dot 6.13 / 6.29. The in-app
  browser pane reports `document.visibilityState === "hidden"`, so the page's
  own mark-read effect correctly did not fire there; the route was exercised
  directly instead.

## 17. The later tier, 2026-09-08

Four items, surveyed across every plane before a line was written. Three
turned out to be startable in some honest form and one is not. The order below
is the order of what has a real data source today, which is not the order the
references suggested.

### Skills: the `/` picker, and the mechanism that had to be made real first

The room composer's `/` now opens a picker of the skills this member may use,
each badged with its real scope, and a picked skill becomes a chip that rides
to the server as an id. That is the visible part. It could not be built as a
mirror of the dashboard composer's existing `/` picker, because that picker
did not do what it appeared to do.

**What was true before.** A skill picked in the dashboard composer was sent
as a *tool spec* named after the skill, with an empty parameter schema
(`chat-client.ts::buildToolSpecs` → `dynamicActionTool`). Model-gateway has
no idea what to do with a tool named after a skill; skills reach a turn only
through keyword matching (`sse.rs::fetch_skill_context` → `handle_match_skills`),
which scores the *message text* against name, tags and body and applies each
skill's `min_score`. Picking "Innkjøpsrutine" therefore steered nothing unless
the message happened to contain its trigger words — and in that case it would
have been injected anyway. The picker was a control that lied, and a room
picker built on top of it would have lied in a second place.

**What shipped, Model Plane.** `InvokeRequest` gains `skill_ids` (aliased
`skillIds`). `skills::resolve_requested_skills` resolves each id against the
org's catalogue — a stale or foreign id is dropped with a warning, never an
error, because a turn must not fail over a label — and against the same SKILL-1
ownership rule matching uses (`usable_or_untracked`), so a private skill cannot
be pulled into a turn by guessing its id. Explicit picks are injected first, in
the order picked, with no `min_score`; keyword matches fill in behind them
minus anything already picked; the existing size budget still bounds the total.
The cap is four (`MAX_REQUESTED_SKILLS`), above the three-match cap on purpose:
a deliberate pick outranks a guess. The turn registry records the resolved
explicit ids ahead of the matches, so a thumbs-up credits what actually shaped
the answer. 4 new unit tests; the crate's 1020 lib tests pass; deployed.

**What shipped, Frontend Plane.** `chat-client.ts` puts picked skills on the
wire as `skill_ids` and *removes* them from the tool specs — the key is absent
when none is picked, so a turn that names no skill is byte-for-byte what it
was. The dashboard composer's existing picker becomes truthful by this alone.
The Verevon gateway forwards the field untouched (only named fields are
stripped on the chat path; a forged id is harmless because the server decides).
`skills-client.ts` learns `scope`, `owner_user_id` and `shared_with`, which
capability-core has returned all along and the composer normalizer dropped.
`SpaceRoomComposer.tsx`: the `@` and `/` pickers share one list, one
highlighted row and one set of keys — arrows, Enter, Tab, Escape, with
`aria-activedescendant` — so the mention picker is no longer mouse-only. The
catalogue is fetched on the first `/`, never on mount. A picked skill is a
chip above the textarea with its scope and a remove control; the `/query`
leaves the message. The `/` box opens on zero matches too, to say *why*:
loading, could not be loaded, none available to you, none match.

**The scope badge states two scopes because two exist.** `agent_skills.scope`
is CHECK-constrained to `org | user`; `capability_scopes.scope_kind` to
`run | thread | workspace | user | org | global | agent`. There is no `space`
anywhere, and the adoption plan names that as the unresolved blocker for S3.5
(itself behind S1.4, S3.4 and S2.1). So the badge reads "Organisasjon" or
"Personlig", and there is deliberately no "dette rommet" — a badge for a scope
the registry cannot store would be the same lie as the old picker. Recorded,
not approximated: a Space-scoped skill needs a schema change in Model Plane
and a Space-aware resolver, and the plan says to consolidate the two existing
registries rather than add a third.

**Found on the way.** model-gateway's lib test target did not compile at HEAD:
a normalize test assigned `Vec<String>` to `InvokeRequest.tools`, which is
`Vec<ToolSpec>`. Fixed in the test so the suite runs; the "two known-broken
targets" note in memory was one short.

### Cost: the hard stop made legible, a false figure removed, the allowance recorded as blocked

**What exists.** Exactly one hard stop on spend exists anywhere: model-gateway's
pre-flight budget guard (`budget.rs` + `org_quota.rs` → cost-core
`/api/v1/budget/check`), which refuses a turn with 402 `budget_exceeded` before
it starts, against the ceiling an org admin sets under Settings › Forbrukstak.
It is pre-flight only — nothing stops a run mid-way on cost. Control Plane
enforces nothing spend-related: billing-core's `IsExceeded` is advisory and
its `PublishQuotaExceeded` has no producer; `org_quotas.quota_value` is never
written. No per-Space budget or per-Space cost exists: `cost_entries` has no
Space column.

**What shipped.** The refusal now reads as what it is. The room composer keeps
the gateway's error *code* beside its message; `budget_exceeded` renders as
"Forbrukstaket er nådd, så agenten startet ikke" with a link to Settings ›
Forbrukstak, and explicitly not as "the reply stopped before it finished" —
nothing was replied. `budget_unavailable` likewise says the check could not
run. Unknown codes keep the server's message rather than a generic line that
hides it. And the Activity feed no longer prints "0 tokens" under every run
(see the §14 correction): a zero total is no figure.

**Why the allowance itself is not built.** "Used of allowance" needs two
numbers on one surface for the member reading the room. The limit lives in
org-core and is readable only by org admins; the usage lives in cost-core and
is joined to the limit in exactly one place, the POST budget check, which
returns a boolean plus totals and is not exposed by the Verevon gateway. A
member-readable "X of Y" would need a new read route joining the two — a
reasonable read — but the number it would show is not the number the label
promises, which is the real blocker:

- the ceiling is *named* per run (`max_cost_per_run_usd_micros`, and the
  Settings label says "per kjøring") but cost-core compares it against a
  **lifetime** org-or-user aggregate with no time window and no honoured
  `reset_period` — once crossed, every turn is refused forever;
- reading the ceiling **fails open** (org-core down ⇒ uncapped) while checking
  it fails closed;
- the **ZDR** streaming path returns before the check and is never budgeted;
- a stored ceiling of `0` is treated as *unset*, contradicting the frontend's
  "zero means no allowance" copy.

Displaying an allowance over those semantics would either restate the label's
promise (false) or expose the mismatch as if it were the design. These are
Model and Control Plane defects, recorded here for their owners; the room
shows the refusal truthfully and nothing more.

### Routines: the read side existed; the room now says where the write side lives

The schedule spine is real and Space-bound end to end — Space-scoped cron
rows with a full authority envelope, fail-closed re-authorization at every
fire, and a fired run gets a thread in the Space owned by the orchestrator
service, which the room's listing can see under a shared read decision. The
Work tab (§12) already lists a room's schedules. What does not exist: any
write of a result *message* into the room, a "routine" concept above raw cron
rows, a consent surface, in-room creation (org-admin only, and the plan is
explicit that scheduled effects are not yet safe to make), and any link from a
room to where routines are managed.

**What shipped.** The Work tab now ends with where routines are created and
changed — Settings › Planlagte kjøringer, the existing page — and states that
a routine bound to the room shows here and its runs land in the room. A
footnote, not a form: offering "Ny rutine" in the room would be a scheduled
effect the release gates still hold closed, in a UI that could not show a run
history (`cron_fires` is never read by any API) or a consent notice (no
consent model exists). The `delivery_target_ref` and `approval_policy_ref`
columns on `cron_schedules` exist and are never written; posting back needs
S4.4 delivery, the same chain §14 traced for watches.

### Memory: blocked on a decision no frontend change can make

A per-user memory store exists end to end — Model Plane `agent_memory`, the
gateway's `GET/DELETE /api/v1/memory`, Settings › Minne with provenance and
ZDR-aware degradation. Nothing anywhere keeps revisions: memories are
mutable-in-place, Space and org instructions are last-write-wins Convex fields
(ADR-0003 deferred versioning "until a real versioning consumer is scoped"),
and the one revision store in the repo — wiki page versions in Data Plane v2 —
has list and diff but no restore. The adoption plan assigns revisioned authored
memory to Data Plane v2, which has no memory store; the ownership matrix gives
memory to Model Plane. A Memory page with revision restore therefore needs a
plane decision and a new history store first. Building the page over a store
that cannot restore anything would be a page with a button that lies. Recorded
with the dependency; not started.

### Verification

- Model Plane: `cargo test --lib` for model-gateway — 1020 passed (4 new);
  redeployed with `compose.sh`, container healthy.
- Frontend: `pnpm typecheck` clean; lint 0 errors on every touched file (the
  composer's one pre-existing `solid/reactivity` warning remains); 532 tests
  pass across `features/spaces`, `features/core`, `shared/api` and
  `shared/actions` — 7 new picker tests, 2 hard-stop tests, 3 wire-body tests,
  3 zero-token tests, 1 Work-tab test.
- Live at `/spaces/p574enc94gj1c99ejjtet33zrn8dx21e#chat` after a
  cache-bypassed reload: `GET /api/v1/skills` answers 200 with an empty list
  for this org, and typing `/` in the real composer opens the listbox labelled
  "Velg en ferdighet" reading "Ingen ferdigheter er tilgjengelige for deg";
  Escape closes it. Every new class probed against the live stylesheet in both
  themes — chip text 17.4, scope badge 14.82, remove control 6.13, highlighted
  row 15.81 with its badge 13.5, empty line 6.13, hard-stop line and link 6.0,
  routines footnote 5.77 with its link 16.38 — identical light and dark, since
  the composer's tokens resolve the same on both. Not driven with a real turn:
  a message would write into the org room, and the org has no skills to pick;
  the injection path is covered by the model-gateway unit tests and the wire
  shape by the client tests.

## 18. Presence, 2026-09-08 — and a correction to §2's own ledger

Before building this, the progress table in §2 was re-checked against §7
through §12, since those sections record work the table still called "Not
built". Six rows were stale:

| Row | Table said | Sections actually show |
|---|---|---|
| Members: invitations, removals, role changes | Not built | Add/remove shipped in §10; only role changes are open |
| Create a shared room / project / case | Not built | Room creation shipped in §10; project/case did not |
| Pause / resume / remove an agent binding | Not built | Shipped in §9 |
| Work tab (S4.5) | Blocked on Model Plane | The block was removed as part of §12 |
| Presence (`conversationPresence`) | Dormant | True of that table; the claim is now split into what this section builds |
| Realtime / shared-thread continuity | Not built | The 6 s poll (§7) is exactly this, as a poll rather than a push |

§4's status line and the table now say this. The one row that stayed open —
per-member role changes in a room — is real: `addSpaceMember` sends only a
`member_id`, with no role field anywhere on the path.

### What presence is, and what it deliberately is not

"Who is in the room" and "who is writing" — the two facts every reference
product states about a shared space and this one did not.

**Application Plane owns it**, for the same reason item 4b's read markers do:
threads are Model Plane's, membership is Control's, and "who is looking at
this right now" is a workspace projection that belongs to neither. A new
table, `spacePresence`, rather than reusing the dormant `conversationPresence`
— that one is keyed per *conversation*, and a room's presence is a fact about
the room, the same reasoning that made read markers room-level rather than
per-thread.

**Absence is derived, never stored.** A browser that crashes, sleeps, or is
killed by the OS sends no goodbye. So a row is a heartbeat with a 30-second
expiry — five times the room's 6-second poll, so one dropped request never
flickers a member out of the room — and "present" is a query-time computation
over freshness, not a stored boolean. `offline` exists for the tab that does
get to say it is leaving; nothing depends on it arriving.

**One request answers both directions.** `POST /spaces/:ref/presence` writes
the caller's own status and returns who else is present in the same round
trip, riding the six-second poll the room already runs. A heartbeat that only
wrote would need a second request on the same timer to be worth anything.

**Typing gets its own beat, throttled.** Waiting for the next scheduled poll
would put "is writing…" on screen up to six seconds after someone started —
worse than not having the feature. The composer reports typing through
`onTyping`, throttled to once per three seconds, so a keystroke is not a
network event. A `typing` status decays to plain presence after eight
seconds without a fresh one: the person is still in the room, they only
stopped typing, and saying "is writing" about them long after is a lie the
room would be telling on their behalf.

**Names come from the roster the room already has; presence carries only
identifiers.** A subject the roster does not know — someone who just left, or
a roster still loading — is counted and never labelled with a raw id.
"Kari, Ola and 2 others" states the truth; inventing a name would not.

**Unreadable presence renders as nothing, not as an empty room.** The two are
different facts, and the second is a claim about who is NOT there. A failed
beat sets the reading to `undefined` and the header line simply does not
appear — the same "named gap, not a zero" rule item 4b's read marker and
item 6's Knowledge and Work tabs already use.

### What shipped

- Application Plane: `convex/schema.ts` `spacePresence` with `by_space` and
  `by_space_and_subject` indexes; `convex/spacePresence.ts` —
  `presentMembers` (pure, tested: freshness window, typing decay, the
  viewer excluded from their own list, stable alphabetical order rather than
  recency so the line does not reshuffle on every beat), `normalizeStatus`,
  `recordSpacePresenceForGateway` (write-then-read-back in one mutation).
- Gateway `domains/spaces.rs`: `POST /spaces/:ref/presence`, body
  `{status?: "online"|"typing"|"offline"}` defaulting to `online`; 400
  `invalid_presence` for anything else; the same lifecycle-then-membership
  gate as every other Space write; 503 `presence_unavailable` when
  Application cannot be reached, worded so it never reads as "you are alone".
- `spaces-client.ts`: `SpacePresence`, `SpacePresentMember`,
  `recordSpacePresence`.
- `features/spaces/lib/space-presence.ts`: `readPresence` (roster lookup,
  viewer exclusion, count vs. names), `hereSentence`, `typingSentence`,
  `nameList` ("Kari", "Kari og Ola", "Kari, Ola og 2 andre").
- `SpacePage.tsx`: the heartbeat effect, riding the same tracked trigger as
  the read-marker effect; a header line ("Kari og Ola er her nå") beside the
  existing role/lifecycle facts; wires `typingSentence` and `onTyping` down
  to the composer.
- `SpaceRoomComposer.tsx`: a typing line above the textarea, matching the
  agent working line's geometry so the two read as one family — a colleague
  above, Verevon below, never merged into one sentence.

### What remains open

**No sidebar presence.** The sidebar shows a room's working and unread dots
for every room in the list, because item 1b's live-work store publishes a
projection for any room whose page has published — including rooms nobody
currently has open, as long as SOMEONE'S page is polling it. Presence has no
equivalent: `recordSpacePresenceForGateway` answers "who else is in *this*
room" only for the room the caller is beating, because a beat only makes
sense from inside the room you are looking at. A sidebar badge ("3 people in
Lager right now") for a room that is not open would need a cross-room
aggregate from Application Plane — the same missing piece §16 recorded for
cross-room unread. Recorded here rather than approximated by having the
sidebar beat presence for every room in its list, which would turn one
request per open room into one request per row.

### A pre-existing bug this work exposed, not caused

Item 4b's arrival-marker effect (`SpacePage.tsx`, `unreadIds`'s snapshot
logic) read its OWN signal — `arrival()?.ref !== ref` — from inside the
untracked half of a `createEffect`. Solid's dev build has a diagnostic for
exactly this (`STRICT_READ_UNTRACKED`) precisely because the read will not
update; here it did something worse under specific timing (a rejected
promise resolving on an early microtask, which a new presence test happened
to construct) — the page's render pass restarted, live at first alongside a
stale one, so `findByRole('heading', …)` briefly saw the title twice. Fixed
with the pattern this file already uses elsewhere for a fire-and-forget
continuation that needs to know "is this still the current room": a plain
variable set from the effect's tracked half, never a second signal read from
its untracked half. The same defect class as the composer's `settled`
callback fix in item 4b (§16) — a value captured once at the boundary,
instead of re-read across it.

Found alongside, and not a bug: the room's own empty-state intro card
(`SpaceRoomIntro`, `SpaceRoomTimeline.tsx`) names the room as an `<h3>`. A
personal Space with no threads yet legitimately has two elements reading
"Personlig rom" — the page's `<h1>` and the intro card's `<h3>` — and a test
query needs `level: 1` to mean the page title specifically.

### Verification

- Application: 8 new `node:test` cases (`test/space-presence.test.cjs`), 83
  pass in the package; functions pushed and confirmed ready after a
  `convex-gateway` restart.
- Gateway: 4 new tests (answers who-else from the same beat, defaults to
  online, refuses an unoffered status, fails as its own gap rather than
  drawing an empty room) plus a fix that outlasts this feature — see below.
- Frontend: `pnpm typecheck` clean (excluding a concurrent, unrelated,
  uncommitted change to `DashboardComposer.tsx` from another initiative in
  this worktree); lint 0 errors; the full suite passes with 8 new tests
  (4 page-level presence, 2 composer typing-line, plus the 8 pure-function
  tests above counted once).
- Live at `/spaces/p574enc94gj1c99ejjtet33zrn8dx21e#chat` after a
  cache-bypassed reload: `POST /presence` answers 200 with the caller's own
  status and an empty `present` array (the org has one member online — this
  session) and a `ttl_seconds` of 30; `typing` is accepted the same way; an
  invalid status (`away`, the older schema's word) is refused with
  `invalid_presence`. The in-app browser pane reports
  `document.visibilityState === "hidden"` regardless of front/back state —
  the same environment limit item 4b's live check already recorded — so the
  page's own heartbeat correctly never fired there; the route was exercised
  directly instead, and no threads mean nobody else was there to render.
  Every new class probed against the live stylesheet in both themes after
  the fix below: header presence line 6.13 light / 16.33 dark, presence dot
  5.04 / 6.53, composer typing line 6.13 / 16.33.

**A third instance of the dark-mode contrast bug items 4b and this section's
own sidebar fix already found twice.** `--verevon-text-muted` is redefined
for dark mode only inside `.dark .verevon-chat-page`'s own scope; everywhere
else — this header's meta row included — it stays the light-mode value
(`#66615b`, meant for a white card) against the near-black canvas, measuring
2.92:1. Pre-existing on "Rom · Aktiv · Din rolle: Eier" before presence
added a fourth item to the same line; the probe caught it because the new
line sits beside the old ones. Fixed with a scoped `.dark .verevon-space-meta`
override mirroring the sibling `.dark .verevon-space-header h1` rule two
lines above it in the sheet, rather than only for the new span — leaving
three neighbours broken while the fourth happened to inherit the same fix
would not have been a fix.

### A live outage found after this section shipped, unrelated to it

Shortly after this section's gateway redeploy, the SAME live org started
getting a real 503 on `GET /spaces/:ref/threads` and `GET /spaces/:ref/context`
— reported directly from the browser console, not caught by any test. Root
cause: `docker exec verevon-gateway-rs printenv` showed
`APPLICATION_CONVEX_SERVICE_KEY` empty in the running container, though
`APPLICATION_CONVEX_URL` was correct. Both are `${VAR:-default}` substitutions
in `docker-compose.yml`, sourced from `CONVEX_INTERNAL_SERVICE_KEY` in
`convex-core/.env` / `.env.local` — files the deploy script's env-file chain
already includes, and which hold the correct value (`docker compose config`
run with that exact chain resolves it correctly). The redeploy that shipped
presence evidently ran before that secret was in place, or against a state
where compose did not recompute the environment section; a second, otherwise
unchanged `deploy_frontend.sh gateway` run against the same files fixed it
immediately, confirmed by re-checking the container's environment and
re-requesting both routes (200/200). Not caused by the `AppState` refactor
below: a direct `docker compose config` check with the same file chain
resolved the secret correctly before the fix, which is what pointed at a
deploy-timing issue rather than a code path. Recorded here because the same
symptom — a route this section owns 503ing with no gateway-side WARN/ERROR
beyond the terminal status — is worth recognizing immediately as a secret-
provisioning gap rather than re-diagnosed as a logic bug.

### A repo-wide test-isolation defect, found and fixed as part of shipping this

`domains/spaces.rs`'s test module read `APPLICATION_CONVEX_URL` /
`APPLICATION_CONVEX_SERVICE_KEY` from **process environment** inside
production handlers (`personal_space_record`, the instructions handlers),
and 22 tests set and unset those same two process-global variables around
each request. Every other upstream URL and token lives on `AppState`; these
two did not. Adding four presence tests pushed the module over some
scheduling threshold and the suite started failing 1-5 tests per run,
non-deterministically, always ones that happened to run concurrently with
another test's `set_var`/`remove_var` pair — a real race, not flakiness in
the tests themselves.

Fixed at the root: `application_convex_url` and `application_convex_service_key`
moved onto `AppState`, read once from the environment at startup like every
other upstream, with the two production call sites and all 22 test fixtures
(across `domains/spaces.rs` and `domains/orgs/instructions.rs`) updated to
read and set them there instead. No test needed to start take the module's
`TEST_ENV_LOCK` for this specific hazard, because the hazard no longer
exists — the lock still guards the environment reads that remain genuinely
global (dev-auth bypass and similar). Two other `AppState` construction
sites (`domains/browser.rs`, `onboarding/crawl_preview/stream_e2e.rs`)
needed the two new fields to keep compiling, confirmed by `cargo build
--tests`. `cargo test domains::spaces` — the feature's own scope — passes
95/95 repeatably across every run in this session, including immediately
after the fix.

**A second, separate flake surfaced while confirming the first fix, and is
NOT this session's to claim fixed.** Running the entire gateway binary with
no filter (500+ tests) failed 1-2 tests per run, always with the same
signature — `assertion left == right failed, left: 503, right: 200`, the
gateway's own upstream-unreachable fallback — and always a DIFFERENT test
from the `room_4b_fixture` family, across three separate full-binary runs
(two at default parallelism, one at `--test-threads=4`): first
`space_threads_carry_the_readers_marker_and_pin_state` and
`space_threads_separate_never_caught_up_from_marker_unavailable` twice
identically, then `mark_space_read_records_the_moment_and_nothing_else`
under reduced parallelism. Every one of these tests passes 100% reliably
alone or scoped to `domains::spaces`. The pattern — a mocked upstream timing
out, a different test each time, present before and after this session's
`AppState` fix — points to the aggregate load of running 500+ tests that
each spin up several real `wiremock` servers in one binary, not to test
logic or to anything this session changed. Filed as its own task rather than
chased further inside a Space-page session: `apps/Frontend Plane/verevonv3/
apps/gateway`'s test suite needs either a higher client timeout in its test
profile, fewer concurrent mock servers per fixture, or a lower default
thread count, and none of those are a one-line fix worth making without its
own verification pass.
