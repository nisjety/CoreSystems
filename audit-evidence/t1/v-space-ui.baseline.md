# Verevon Space: frontend/UX baseline (Verevon v3, code as ground truth)

Auditor role: Verevon baseline auditor (Space, frontend/UX). Read-only audit, 2026-10-13.
Repo: `C:/dev/CoresSystem` (newest commit `afb83928`, 2026-10-10). The last commit that touched the Space frontend was `828d9d55` on 2026-09-28; the Space feature work was done mainly between 2026-08-13 and 2026-09-13 (`git log` on `src/features/spaces`).

Working-tree note (SRC, re-checked 2026-10-13): `C:/dev/CoresSystem` has about 100 uncommitted changes under `V3`, all dated 2026-10-12 or earlier, so they were in place when this baseline was read. Line numbers refer to the working tree, not HEAD. The only Space-folder change is a small one: `SpaceRoomComposer.tsx:465` now passes the refusal `feature` to `translateApiError`, which shows typed Zero-Data-Retention refusals (`V3/src/shared/api/zdr-refusals.ts`, also uncommitted). See capability `v-space-ui-118`.

Path shorthand used below (all relative to `C:/dev/CoresSystem`):
- `V3` = `apps/Frontend Plane/verevonv3`
- `SP` = `V3/src/features/spaces`
- `GW` = `V3/apps/gateway/src/domains/spaces.rs` (Rust BFF, 9,882 lines)
- `CVX` = `apps/Application Plane/convex-core/convex`

The capabilities JSON spells every path out in full.

Provenance tags: **SRC** = read directly in source. **DOC** = internal docs. **INF** = my inference (basis stated). No REA was used in this slice, because Verevon is source-available. Docs show intent. Where a doc and the code disagree, the code wins and the gap is listed in the last section.

---

## 0. One-paragraph verdict

A Verevon Space today is a single-page **workroom** at `/spaces/:spaceId` (SRC `V3/src/app/App.tsx:181-182`). It has a six-tab cockpit (Chat, Work, Knowledge, Activity, Agent, Members), the shared Core Sidebar as its room navigator, and a right-hand "room pulse" rail. It is strong on **authority honesty**: server-composed context, a fail-closed membership recheck every 30 s, Control-issued thread and append decisions, and "gap, not empty" rendering. It is also strong on **agents as governed room members**: create in the room, bind an existing agent, pause, resume or revoke, binding-policy enforcement at the gateway, in-room approvals, and receipts correlated through grants in Activity.

It is **thin as a chat product**:
- Every post is an AI invocation. There is no human-only message path.
- No DMs or group DMs, no edit/delete, no reactions, no attachments in the room, no message search, no notifications or push, and no calls.
- No push transport: the room polls every 6 s.
- No thread side-panel.
- No visibility of an agent's tools, browser or computer inside the room.
- Only `personal` and `room` kinds can be created.
- One agent per message, and no agent-to-agent delegation.

Maturity: it is source and unit-test backed (about 39 files under `SP`, 13 of them tests). There is one mocked Playwright spec (SRC `V3/tests/e2e/space-shell.spec.ts:9`). The docs themselves say that no release gate (R-1 to R-5) has been cleared (DOC `V3/docs/SPACE_PAGE_AUDIT_2026-09-06.md:70-73`).

---

## 1. What a Space looks like (structure walkthrough, SRC)

| Region | Component | What it shows | Evidence |
|---|---|---|---|
| Index resolver `/spaces` | `SpacesIndexPage` | Redirects to the org room, then any active room, then the personal room. Self-heals the org room if none exists, polling Control registration 8 x 2.5 s. | `SP/components/SpacesIndexPage.tsx:36-47, 85-113, 240-256` |
| Left: Core Sidebar "Spaces" panel | `CoreSidebarSpacesPanel` | Search box, "Channels" (org room pinned first), "+ New room" inline form, "Personal Space", and "All conversations" for the selected room, with working, unread and pinned dots | `V3/src/features/core/components/sidebar/CoreSidebarSpacesPanel.tsx:104-232, 245-321` |
| Header | `SpacePage` | Eyebrow ("Shared workroom" or "Personal room"), name, kind, lifecycle, "Your role", and a presence sentence ("Kari and Ola are here now") | `SP/components/SpacePage.tsx:358-381, 1445-1447` |
| Centre: cockpit tablist | `SpaceCockpit` | Six hash-driven tabs (`#chat #arbeid #kunnskap #aktivitet #agent #medlemmer`, plus legacy aliases), roving arrow-key focus | `SP/components/SpaceCockpit.tsx:61-124, 170-179` |
| Chat tab | `SpaceConversationPanel` → `SpaceRoomTimeline` and `SpaceRoomComposer` | Posts oldest-first, each post with up to 30 inline turns, author names, an Agent chip, Reply / Pin, "New" and "Pinned" flags, an approval card, and a live "is working…" row. Below that, the room composer. | `SP/components/SpacePage.tsx:497-607`; `SpaceRoomTimeline.tsx:134-416`; `SpaceRoomComposer.tsx:522-775` |
| Work tab | `SpaceWorkPanel` | Runs, schedules and background processes (S4.2) in the verb/object/outcome grammar, with named gaps and a link to Settings › Scheduled runs | `SP/components/SpaceWorkPanel.tsx:106-192` |
| Knowledge tab | `SpaceKnowledgePanel` | Bound workspace id, documents (truncation notice) and wiki pages, read-only | `SP/components/SpaceKnowledgePanel.tsx:82-243` |
| Activity tab | `SpaceActivityPanel` | Five sources (threads, runs with steps, approvals, owner operation receipts, authority grants) under one salience order. Lenses: Everything / Needs attention / Work and effects / Conversations / Authority, with a "N hidden by filter" note. | `SP/components/SpaceActivityPanel.tsx:56-83, 145-259`; `SP/lib/activity-grammar.ts:733-753` |
| Agent tab | `SpaceInstructionsSection` and `SpaceAgentPanel` | Space instructions (editable by editor and above, 4000 chars). Agent cards with role, status mark, policy chips, delivery targets, and Pause / Resume / Remove. Buttons for "Add existing agent" and "Create an agent". | `SP/components/SpacePage.tsx:632-738, 757-1066` |
| Members tab | `SpaceMembersPanel` | "You are confirmed as <role>". Roster from Control (person or agent, role). Add/remove people in named rooms. Personal-Space deletion danger zone with per-plane receipts. | `SP/components/SpacePage.tsx:1068-1251`; `SpaceMemberControls.tsx:34-210` |
| Right rail | `SpacePulse` | "Verevon is working: <thread> · <status>" or "No agent work is active", plus Conversations and Attention counts | `SP/components/SpacePage.tsx:1253-1324` |
| Dialogs | `SpaceCreateAgentDialog`, `SpaceBindAgentDialog` | Create: colour, name, instructions, 3 templates (Meeting Scribe, Status Agent, Knowledge Helper). Bind: org agent list with "Already added". | `SP/components/SpaceCreateAgentDialog.tsx:20-77, 87-239`; `SpaceBindAgentDialog.tsx:26-153` |

Responsive CSS is present: three columns, collapsing at 1160, 1080 and 720 px, with dark-mode rules (SRC `V3/src/styles/global.css:44084-44110, 44989-45024, 46391-46404`). Several overlapping rule blocks target the same selectors (INF: layered overrides that are not consolidated).

---

## 2. Findings by taxonomy category

### 1. Messaging core

| Item | Status | How it works / notes | Evidence |
|---|---|---|---|
| Room timeline of thread-posts with inline turns | shipped | Posts are sorted **oldest-first by `updated_at`** in the browser. Each post fetches its own transcript through the Space route (`/threads/:id/transcript`), capped at 30 turns. Assistant turns render as Markdown (`ChatMarkdown`); human turns render as **plain text**. | SRC `SP/components/SpaceRoomTimeline.tsx:50, 137-141, 193-204, 329-336` |
| Post a message | shipped | Each new message from the composer becomes a new thread ("post") via `streamChat({spaceRef, …})`. The gateway exchanges `space_ref` for a Control-signed create decision. | SRC `SP/components/SpaceRoomComposer.tsx:376-421`; `GW:49-115` |
| **Human-only message (no AI reply)** | **absent** | The composer always calls `streamChat`, which invokes Model Plane, so the default "Verevon" assistant answers every post. A test even asserts that a "plain room turn" goes to a model. A repo-wide grep for `human_only` / `skip_assistant` / `no_assistant_reply` in `.rs/.go/.ts/.tsx` found nothing related. | SRC `SpaceRoomComposer.tsx:402-421`; `SpaceRoomComposer.test.tsx:206`; `SpaceRoomTimeline.tsx:143-146` (fallback name "Verevon") |
| Threads / replies | partial | "Reply" sets a reply target. The next message continues that thread under a content-bound Control `thread:append` decision. There is no thread side-panel, no reply count, no collapse, and no "also send to channel". | SRC `SP/components/SpacePage.tsx:519-533, 580-583`; `SpaceRoomComposer.tsx:385, 415, 630-643` |
| @mention people | partial | `@` lists roster humans and active agents together (max 6, prefix match). Picking a person only inserts `@Name` text: there is no notification, highlight or routing. | SRC `SpaceRoomComposer.tsx:184-211, 307-312` |
| Rich text / code | partial | Assistant output gets full Markdown, code and charts through the shared `ChatMarkdown`. Human input is plain text. There is no formatting toolbar. | SRC `SpaceRoomTimeline.tsx:329-336`; `SpaceRoomComposer.tsx:537-539` |
| Edit / delete message | absent | No client function or route exists. The only "delete" in `spaces-client.ts` is personal-Space deletion and agent revocation. | SRC `V3/src/shared/api/spaces-client.ts` (full read); `GW:649-736` route table |
| Reactions / emoji | absent | Grep for `reaction` in `SP`, `spaces-client.ts` and the Convex schema found nothing. | SRC |
| Attachments / link previews | absent in Space | The room composer has no file input. `/chat` has uploads, artifact panels and exports, but they are not reused here. | SRC `SpaceRoomComposer.tsx:629-772`; contrast `V3/src/features/chat/components/ChatPage.tsx:1-60` |
| Drafts / scheduled send | absent | The textarea state is component-local. There is no draft persistence and no send-later. | SRC `SpaceRoomComposer.tsx:147` |
| Pins | partial | Only the post's **author** may pin or unpin (Session Core presentation is owner-bound). The server orders pinned posts first, but the timeline re-sorts chronologically, so pinned posts are flagged rather than lifted. | SRC `SpaceRoomTimeline.tsx:231-258, 270-284, 396-409`; `spaces-client.ts:54-58, 191-200`; INF on ordering |
| Auto-title persisted for the room | shipped | The model's `title` event is written back through `PATCH …/presentation` when this browser opened the thread. | SRC `SpaceRoomComposer.tsx:427-439` |
| Saved / bookmarks, forwarding, polls, voice clips | absent | Not found in `SP` or the spaces client. A voice feature exists elsewhere (`V3/src/features/voice`) but is not wired into the room. | SRC |
| Read state | shipped (room-level) | A durable per-member `spaceReadMarkers.lastReadAt` exists. The page snapshots the arrival marker and badges posts that are new since the last visit. The marker advances on each visible listing. There are no per-message read receipts. | SRC `SP/components/SpacePage.tsx:137-190`; `SP/lib/space-unread.ts:23-43`; `CVX/schema.ts:845-851` |
| Typing indicator | shipped | A throttled (3 s) `typing` beat. Convex decays `typing` after 8 s. The composer line reads "Kari is writing…". | SRC `SpacePage.tsx:241-250`; `CVX/spacePresence.ts:42,50`; `SpaceRoomComposer.tsx:604-615` |
| Presence | shipped (room-scoped) | A heartbeat rides the 6 s poll and returns who else is present (30 s TTL). An `offline` goodbye is sent on leave. There is no global status, DND or custom status. | SRC `SpacePage.tsx:196-253`; `spaces-client.ts:233-256`; `CVX/schema.ts:1003-1012` |
| Stop a turn (recorded) | shipped | Stop aborts the SSE read, then calls `cancelInvocation(requestId)` so Session Core records `cancelled`. Two cases are kept apart honestly: a stop that was recorded, and one that was only local. | SRC `SpaceRoomComposer.tsx:488-520, 572-583` |
| Queue / steer while busy | absent in Space | The textarea is disabled while submitting. `/chat` does have mid-run queueing (`queueInvocationInput`). | SRC `SpaceRoomComposer.tsx:746`; `V3/src/features/chat/components/use-chat-controller.ts:79, 2184-2219` |
| Per-message model choice | shipped | `AiModelPicker`, a privacy-tier resolver per turn, and the ChatGPT-subscription route (text-only; it blocks agent and skill use). | SRC `SpaceRoomComposer.tsx:143-146, 373-374, 400-413, 652, 761-768` |

### 2. Org & membership

| Item | Status | Notes | Evidence |
|---|---|---|---|
| Space kinds | partial | The model has `personal / room / project / case`. The gateway create path refuses anything except `personal` and `room` (`unsupported_space_kind`). | SRC `CVX/schema.ts:546-556`; `GW:2860-2885` |
| Org-wide room ("#general") | shipped | `isOrganizationRoom`. Provisioned at onboarding and self-healed from `/spaces`. Its roster follows the org and cannot be edited in the room. | SRC `CVX/schema.ts:557-566`; `SpacesIndexPage.tsx:62-113`; `SpacePage.tsx:1148-1155` |
| Create a named room | shipped | Sidebar "+ New room" form → `POST /api/v1/spaces {kind:'room'}`. The room is `pending_registration` until Control registers it, and it may not appear in the list until then. | SRC `CoreSidebarSpacesPanel.tsx:49-74, 132-182`; `spaces-client.ts:945-951` |
| Personal Space | shipped | Idempotent, one per owner. **It cannot hold an agent or other members**, because Control refuses membership replacement. | SRC `spaces-client.ts:925-931`; DOC `V3/docs/space-defenition.md` (open question) |
| Roles | shipped | viewer / editor / manager / owner, shown in the header, roster and agent cards. The UI gates mirror the gateway: create/bind/govern an agent needs owner or manager; instructions need editor or above; member grants need owner or manager. | SRC `SpacePage.tsx:52-73, 1346-1353`; `GW:3261, 3720` |
| Add / remove people | shipped | Picker of active org members not already present → `POST/DELETE /members`. The grant role is **fixed to `editor`**. The owner cannot be removed. Confirmation happens in place. | SRC `SpaceMemberControls.tsx:34-210`; `CVX/schema.ts:821-835` |
| Change a member's role | absent | No route and no UI. Confirmed in DOC `SPACE_PAGE_AUDIT_2026-09-06.md:48`. | SRC `spaces-client.ts:965-977` (only `member_id`) |
| Guests / external / shared channels | absent | The picker only offers org members, and the gateway refuses non-members. | SRC `SpaceMemberControls.tsx:21-30` |
| Public/private channels, browse & join | absent | The listing is member-filtered. There is no discovery or "join" flow. | SRC `CoreSidebarSpacesPanel.tsx:35-43`; `GW` route table |
| Sidebar organization | shipped (basic) | Channels / Personal Space / All conversations, plus a text filter. There are no sections, starring, folders, drag ordering or muting. | SRC `CoreSidebarSpacesPanel.tsx:104-232` |
| Room settings (rename / archive / delete a room) | absent | Only a personal-Space deletion request exists. | SRC `SpacePage.tsx:1198-1248` |
| Space instructions (standing orders) | shipped | ADR-0003 Space layer. The gateway injects it into every turn after a role check. | SRC `SpacePage.tsx:632-738`; `GW:4291-4323` |
| Admin console for Spaces | absent | No admin route. `/agents/installations` and `/agents/chief-core` give a cross-Space **agent** view only. | SRC `V3/src/app/App.tsx:212-218`; `V3/src/features/agents/components/AgentInstallationsPage.tsx:22, 117` |
| Org invites, org switcher | shipped (org level, outside Space) | `inviteMember`, the `/accept-invitation/:id` route and a navbar `WorkspaceSwitcher`. | SRC `V3/src/shared/api/membership-client.ts:5-10`; `App.tsx:162`; `V3/src/features/core/components/CoreNavbar.tsx:672-729` |
| DMs / group DMs | absent | No DM kind and no 1:1 room flow. | SRC `CVX/schema.ts:549-554` |

### 3. Calls & meetings

| Item | Status | Notes | Evidence |
|---|---|---|---|
| Audio/video, huddles, screen share, recording, captions, whiteboard, calendar | absent | Grep for `huddle|webrtc|getUserMedia|video call|screen share` in `SP` and the spaces client found nothing. | SRC |
| AI meeting recap | partial (text only) | A "Meeting Scribe" agent template summarises the **text** conversation when it is mentioned. | SRC `SpaceCreateAgentDialog.tsx:43-54` |

### 4. Search & discovery

| Item | Status | Notes | Evidence |
|---|---|---|---|
| Room / conversation filter | partial | A client-side filter over room names and over thread titles/previews **for the selected room only**. | SRC `CoreSidebarSpacesPanel.tsx:89-99, 346-360` |
| Full-text message search | absent | `search-client.ts` is web search (Quarry), not workspace search. No Space search route exists. | SRC `V3/src/shared/api/search-client.ts:78-130`; `GW:649-736` |
| AI answers grounded in room knowledge | partial (DOC) | The docs say room turns get whole-Space retrieval when the org's `retrieval_read_entitled` bit is set. This was **not verified in code** in this slice. | DOC `V3/docs/space-defenition.md` (binding model, knowledge_scope) |
| People directory | shipped (room-level) | Members-tab roster (display name or subject id, person/agent, role). | SRC `SpacePage.tsx:1157-1196` |
| File / knowledge directory | shipped (read-only) | Knowledge tab: documents via `documents.space_ref`, wiki pages via the retrieval binding, and a truncation notice. | SRC `SpaceKnowledgePanel.tsx:82-243` |
| History / pagination | partial | The full thread list loads on every poll. There is no paging or jump-to-date. Activity rows link to `?thread_id=…#chat`, but **SpacePage never reads `thread_id`**, so that deep link lands on the room only. | SRC `SP/lib/activity-grammar.ts:200, 326`; grep: no `useSearchParams` in `SP` |

### 5. Notifications & attention

| Item | Status | Notes | Evidence |
|---|---|---|---|
| Notification feed | absent for Space | A generic `/api/v1/notifications` client exists and is used by the chat "findings" page. No Space or mention events are produced (notification-core has no `space_ref` or mention references). | SRC `V3/src/shared/api/notifications-client.ts:28-62`; `V3/src/features/chat/components/ChatUtilityPage.tsx:36` |
| Push / desktop / mobile notifications | absent | No `Notification(`, `PushManager` or service worker in `src`. | SRC grep |
| Mention routing to a person | absent | See Messaging. | SRC |
| Unread badges | partial | Per-post "New", plus sidebar dots **only for the room open in this browser** (a browser-local store that is retracted on leave). There are no cross-room unread counts. | SRC `SP/lib/space-live-work.ts:52-99`; `CoreSidebarSpacesPanel.tsx:281-321`; DOC `SPACE_PAGE_AUDIT_2026-09-06.md:1454-1462` |
| Mute / DND | absent | | SRC |
| Attention surfaces | shipped | Pulse rail "Attention" count. Activity "Needs attention" lens with salience ordering (critical > attention > normal > muted). A working dot on sidebar rooms and threads. | SRC `SpacePage.tsx:1305-1314`; `SpaceActivityPanel.tsx:66-83`; `activity-grammar.ts:252-265` |

### 6. Apps & extensibility

| Item | Status | Notes | Evidence |
|---|---|---|---|
| `/` slash picker = skills | shipped | Lazily loaded skill catalogue. Shared keyboard picker with `@`. Chips carry an org/personal scope badge and are sent as `skill_ids`. Capped at `MAX_PICKED_SKILLS`. | SRC `SpaceRoomComposer.tsx:150-160, 213-267, 314-331, 653-673, 777-800` |
| Space action catalog (human-only) | stub | `GET /spaces/:ref/actions` exists and returns the org human catalog after a membership check. The client function `getSpaceActions` has **no UI consumer**. | SRC `GW:4885-4925`; `spaces-client.ts:742-744` |
| `spaces.*` typed actions | shipped (registry) | 14 actions with zod schemas, risk and owner plane. None is model-eligible (the allowlist covers only tickets/inbox). | SRC `V3/src/shared/actions/action-registry.ts:256-340, 1991-2145`; `model-eligibility.ts:1-60` |
| Workflows / routines | partial | The Work tab lists schedules bound to the room (read-only). Creation lives in Settings › Scheduled runs. | SRC `SpaceWorkPanel.tsx:171-189` |
| MCP / plugins / connectors per room | absent in Space | MCP and plugin sections exist org-wide in Settings. A Space cockpit for them is planned (S3.6). | SRC `V3/src/features/settings/components/McpServersSection.tsx`, `PluginsSection.tsx`; DOC plan S3.6 |
| Delivery targets (Teams / Messenger / embed) | partial (display only) | Agent cards list the channels an agent can reach, with a status. Nothing in the room configures them. | SRC `SpacePage.tsx:1040-1062, 1411-1417` |
| Webhooks, app directory, tabs/pinned apps, forms, custom emoji | absent | The six tabs are fixed. | SRC `SpaceCockpit.tsx:61-110` |

### 7. Agents in the room (Verevon's strongest area)

| Item | Status | Notes | Evidence |
|---|---|---|---|
| Agents as members | shipped | A Control `service` subject in the roster, joined with the Application binding for its identity. A third state is shown honestly: "access granted, no published identity". | SRC `spaces-client.ts:704-740`; `SpacePage.tsx:965-975, 1170`; `CVX/schema.ts:606-680` |
| Create a room agent (Grok-style one step) | shipped | Name, colour, instructions and templates. Server side it is two steps (Application definition + pending binding, then Control roster confirmation). It is **born mention-only, with no tools, requiring confirmation**. The chosen **avatar colour is never rendered afterwards**: the card shows an initial only and `SpaceAgent` has no colour field. | SRC `SpaceCreateAgentDialog.tsx:106-132`; `CVX/spaceAgents.ts:463-468`; `SpacePage.tsx:911-915` |
| Bind an existing org agent | shipped | Same narrow policy. The agent's Studio configuration is not inherited. | SRC `SpaceBindAgentDialog.tsx:278-293`; `CVX/spaceAgents.ts:556-563` |
| Pause / resume / remove in the room | shipped | Owner/manager only. Revoke arms before acting (in-place confirm). | SRC `SpacePage.tsx:856-906, 990-1032` |
| @-invoke an agent | shipped | Only one agent per message (`mentionedAgentRef`). The gateway re-resolves membership and binding, enforces `trigger_modes` (mention), `approval_mode` (blocked refuses; anything other than `auto` strips `agentic` and `plan_mode`) and `allowed_tools` (filter), then injects the persona name and prompt. | SRC `SpaceRoomComposer.tsx:149, 310, 382, 414`; `GW:4399-4471, 4500-4560` |
| Binding-policy editor | absent | Policy fields render as read-only chips. No UI anywhere writes `trigger_modes`, `approval_mode` or `allowed_tools` (grep across `V3/src`). | SRC `SpacePage.tsx:943-963` |
| Default assistant | shipped (de facto) | With no mention, the turn is answered by the default "Verevon" persona (or by the single active agent's name in the fallback attribution heuristic). | SRC `SpaceRoomTimeline.tsx:143-146, 320-327` |
| Turn attribution | shipped | `author_subject_id` and `agent_name` are recorded server-side. The UI resolves them against the roster and keeps "unknown author" and "former member" apart. | SRC `SpaceRoomTimeline.tsx:78-103`; `spaces-client.ts:605-656` |
| Work / progress visibility | shipped (status-level) | A per-post "is working…" row (queued/running), the pulse card, the composer line "Verevon is working in «X»", and sidebar working dots. **No tool-call, plan, step or browser cards appear in the room.** Those exist only in `AgentRunConsole` and `/chat` (`ChatLiveRunPanel`). | SRC `SpaceRoomTimeline.tsx:349-369`; `SpaceRoomComposer.tsx:616-627`; `V3/src/features/agents/components/AgentRunConsole.tsx:551-569` |
| Approvals / human-in-the-loop | shipped | An approval card inside the paused post. Approve resumes the run; Deny cancels it. Only the run owner may decide; others read "theirs to decide". After an error it reconciles (`unconfirmed`, `already_decided`). The composer is locked when replying into a paused thread. **Only two verbs**: there is no allow-for-session or always-allow (deliberate). | SRC `SpaceApprovalPanel.tsx:23-36, 62-81, 110-118`; `SP/lib/space-approvals.ts:91-120`; `SpaceRoomComposer.tsx:372, 644-651` |
| Agent PC / browser / sandbox in the room | absent | A browser with human takeover exists in the dashboard (`BrowserChrome`). Agent runs show browser screenshots in `AgentRunConsole`. Neither is reachable from a Space. The Work tab lists background **processes** (read-only). | SRC `V3/src/features/dashboard/home/BrowserChrome.tsx:497, 624`; `SpaceWorkPanel.tsx:111-118` |
| Multi-agent coordination / delegation | absent | UI-2c has not started: "needs an execution-core → gateway network path". | DOC `SPACE_PAGE_AUDIT_2026-09-06.md:196-199`; SRC (single `mentionedAgentRef`) |
| Memory per room / person | partial | Only Space instructions and the user persona layer (gateway). There is no room memory and no learned memory. | SRC `GW:4291-4390` |
| Personal agents (Grok-style, own computer) | absent | A personal Space cannot bind an agent. There is no per-agent computer. | DOC `space-defenition.md` open questions; SRC |
| Scheduled agent work | partial | Schedules bound to the room are listed and their runs land as room threads. There is no creation from the room. | SRC `SpaceWorkPanel.tsx:171-189` |
| Agent permissions & secrets | stub | Gateway routes for per-conversation agent action grants (`…/agent-action-grants`) have **no frontend caller**. Activity renders grant and revoke events. | SRC `GW:667-674`; `activity-grammar.ts:592-620` |
| Cost / limits | partial | `budget_exceeded` renders as a refusal with a link to Settings › Spend ceiling. Token counts are suppressed because Session Core reports 0. There is no per-room spend. | SRC `SpaceRoomComposer.tsx:541-567`; `activity-grammar.ts:694-730` |
| Human ↔ agent hand-off | absent | | SRC |
| Cross-room agent registry | shipped (outside the room) | `/agents/installations` and `/agents/chief-core`. This is presence, not authority. | SRC `spaces-client.ts:871-911`; `AgentInstallationsPage.tsx:22` |

### 8. Collaborative artifacts

| Item | Status | Notes | Evidence |
|---|---|---|---|
| Knowledge (docs + wiki) | shipped (read-only) | Shows the binding workspace, status of non-completed documents, and a truncation flag. There is **no upload or add-source from the room**. | SRC `SpaceKnowledgePanel.tsx:145-243` |
| Canvases, docs, wiki editing, tasks, boards | absent in Space | Studio canvas, tickets and knowledge editing exist elsewhere, unlinked. | SRC `App.tsx:183-188, 220` |
| Chat artifacts (charts, exports, files) | absent in Space | They exist only in `/chat` (`ChatArtifactPanel`, chat-export docx/pptx/xlsx). Space Markdown can render a chart through `ChatMarkdown` (INF). | SRC `V3/src/features/chat/components/ChatArtifactPanel.tsx`; `V3/src/features/chat/lib/chat-export/*` |
| Review / approval flow | shipped (approvals only) | See Agents. | |

### 9. Realtime & sync architecture

| Item | Status | Notes | Evidence |
|---|---|---|---|
| Transport | polling | The thread projection is polled every 6 s **while the tab is visible**, refetched on `visibilitychange`, and authority is rechecked every 30 s. Only the sender's own turn streams (SSE via `streamChat`). There is no WebSocket/SSE/Convex subscription for the room. | SRC `SpacePage.tsx:78-87, 273-338`; DOC `SPACE_PAGE_AUDIT_2026-09-06.md:63` |
| Transcript fan-out | shipped (costly) | One transcript request per post, re-keyed when the post's `updated_at` changes (N+1). | SRC `SpaceRoomTimeline.tsx:193-204` |
| Shared client store | shipped | A browser-local `createRoot` store publishes the page's projection, its own stream and the unread set to the sidebar and composer. It is retracted on leave. | SRC `SP/lib/space-live-work.ts:35-145` |
| Optimistic UI | partial | The sender sees a local exchange block (their message plus the streamed reply) immediately. The timeline only updates on the next poll. Approvals and pins are deliberately non-optimistic. | SRC `SpaceRoomComposer.tsx:386, 524-595`; `space-approvals.ts:1-30` |
| Offline / local cache / multi-device | partial | The read marker is server-side, so it works across devices. There is no offline cache and no queued sends. | SRC `CVX/schema.ts:845-851` |
| E2EE | absent | It is not compatible with the server-side model and retrieval design (INF). | |

### 10. Governance, security & compliance

| Item | Status | Notes | Evidence |
|---|---|---|---|
| Server-authoritative context, fail-closed | shipped | `/context` is rechecked every 30 s. On error the whole room is hidden ("Space unavailable"). | SRC `SpacePage.tsx:317-353` |
| Forged-authority stripping | shipped | The gateway strips 12 authority fields and both spellings of `space_ref`, then rebuilds them only from Control's signed decision. | SRC `GW:34-115` |
| Shared-read gate | partial | The transcript route asks Control for a shared-read decision and is **deny-by-default** (403 `space_read_not_authorized`). The timeline swallows the error (`.catch(() => null)`) and silently shows the preview, so the reader is **not told** why. | SRC `GW:2616-2680`; `SpaceRoomTimeline.tsx:201` |
| Activity / audit in room | shipped | Runs, approvals, operation receipts correlated through the grant that carries `space_ref`, and authority grants/revocations. Absent evidence classes (delivery, watches) are always named as gaps. | SRC `spaces-client.ts:342-471`; `SpaceActivityPanel.tsx:85-143` |
| Deletion | partial | Personal Space only: request → per-plane receipts (authorization is separate from purge). No room deletion. | SRC `SpacePage.tsx:255-271, 1198-1248`; `spaces-client.ts:91-116` |
| Privacy tier / ZDR per turn | shipped | Every room turn carries `minPrivacyTier`, resolved from the model catalogue and failing closed to the strictest tier. Knowledge documents expose `zdr_classification` (not rendered). | SRC `SpaceRoomComposer.tsx:146, 400-412`; `spaces-client.ts:480` |
| Retention, export, eDiscovery, DLP, moderation | absent | No Space-level controls exist. | SRC |
| Lifecycle states | shipped | pending_registration / active / suspended / deleting / deleted / failed_registration are labelled. | SRC `SpacePage.tsx:1336-1344` |

### 11. Client shell

| Item | Status | Notes | Evidence |
|---|---|---|---|
| Routes and deep links | shipped / partial | `/spaces`, `/spaces/:id`, hash tabs with legacy aliases. Thread deep links are generated but not consumed (see Search). | SRC `SpaceCockpit.tsx:114-164` |
| Keyboard | shipped | Tablist arrows. The picker supports arrows, Enter, Tab and Escape. Enter sends and Shift+Enter adds a newline. | SRC `SpaceCockpit.tsx:170-179`; `SpaceRoomComposer.tsx:338-367` |
| Accessibility | shipped | `aria-live` / `role=status` on live rows, status shapes that do not rely on colour (`● ◔ ❙❙ ✕ ▲`), radiogroup lenses. | SRC `SpacePage.tsx:1387-1397`; `SpaceActivityFeed.tsx:79-112` |
| i18n | partial | Norwegian/English via `tr()` in components. **The activity grammar labels are hard-coded Norwegian** (e.g. "Kjøring", "venter på godkjenning"), and dates are forced to `nb-NO`. | SRC `activity-grammar.ts:122-137, 206, 222, 334`; `SP/lib/space-thread-presentation.ts:71-93` |
| Themes / responsive | shipped | | SRC `global.css` (see §1) |
| Command palette | absent for Space | Cmd/Ctrl+K opens web search. `ActionCommand` is a per-button component, not a palette. | SRC `V3/src/features/chat/lib/use-chat-shortcuts.ts:11`; `V3/src/shared/actions/ActionCommand.tsx:1-40` |
| Native shell (tray, protocol handler, auto-update, native notifications) | absent | Web SPA only. | SRC |

### 12. UX patterns & layout

| Item | Status | Notes | Evidence |
|---|---|---|---|
| Navigation model | shipped | Persistent Core Sidebar (rooms + conversations) → room cockpit with six tabs → right pulse rail. | SRC (§1) |
| Empty states | shipped | A room intro with "Start a conversation" and "Create an agent" cards. Every tab separates "nothing", "could not load" and "not published". | SRC `SpaceRoomTimeline.tsx:433-479`; `SpaceCockpit.tsx:233-246` |
| Activity grammar | shipped | Verb / Object → Outcome, consequence-ordered, live rows mutate in place, detail lines underneath. | SRC `activity-grammar.ts:1-35`; `SpaceActivityFeed.tsx:79-112` |
| Destructive confirm | shipped | Arm-in-place button that disarms after 6 s or on blur. | SRC `SpaceConfirmButton.tsx:24-104` |
| Thread panel | absent | Replies continue inline. There is no side pane. | SRC |
| Composer design | partial | Two-row textarea, model picker, skill chips, reply banner, typing and working lines. No attachments, tool toggles, voice, formatting or emoji. | SRC `SpaceRoomComposer.tsx:597-772` |

---

## 3. Planned / in-flight per docs (not yet in code)

From `V3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md` (QM plan), `V3/docs/SPACE_PAGE_AUDIT_2026-09-06.md` (audit), `V3/docs/space-defenition.md` (definition) and `V3/docs/SPACE_AGENT_SCOPE_PLAN_2026-08-14.md` (scope plan):

- **Member role changes**: needs a Control membership-role write (audit §2, §4).
- **Project and case Space creation, and the Support Case Space** (QM plan "First vertical slices"; audit §2).
- **Shared-thread completion (S1.7)**: per-entry audience replay filtering, visibility-preserving fork/share, participant/unread projections (QM plan S1.7 open checkboxes).
- **Push transport for the room**: "SSE or Convex subscription would remove the 6 s floor" (audit §2, "Realtime as a push transport: Not built").
- **Cross-room unread and presence aggregates**: need a per-Space latest-activity aggregate from Model and Application (audit §16 and §18 "What remains open").
- **Room-wide pin / retitle by non-authors** (audit §16).
- **Approval verbs** `Allow once / Allow for this conversation / Always allow / Deny` (audit §6.3 item 2b). This was deliberately *not* adopted in code (`SpaceApprovalPanel.tsx:23-36`).
- **"Offer to add" when an unbound agent is mentioned** (definition, Invocation rule).
- **UI-2c agent-to-agent delegation**: room-roster-only, records the initiating human, bounded fan-out, rendered as one collapsed unit (definition; scope plan §UI-2c). Blocked on an inter-plane network path.
- **Binding fields `knowledge_scope`, `default_thread_policy`, `audit_visibility`**: not stored yet (definition, Binding model).
- **S3.1-S3.6**: authored and learned memory with revisions, the Space sandbox lease and durable workspace, a credential broker, governed skills, and a Space MCP/plugin/connector cockpit (QM plan Sequence 3).
- **S4.3 Watches, S4.4 durable completion delivery, S4.5 unified Work cockpit with cancel/steer/reconcile** (QM plan Sequence 4). Blocked S3.2 → S3.3 → S4.2 (audit §14).
- **S5.2 standing orders with policy floor/kill switch, S5.3 first external surface (Slack), S5.4 internal app build/publish** (QM plan Sequence 5). Channel Plane is docs-only.
- **S6.1 Space deletion/export and retention proof** (QM plan Sequence 6).
- **Sub-routes `/spaces/:id/chat|work|…`**: deviated to hash tabs, which the cockpit-wiring doc accepts.
- **Memory page with revision restore**: blocked on a plane decision (audit §17).
- **Per-Space cost / allowance display**: blocked (audit §17).
- **Reuse `AgentRunConsole` timeline, proof, approval and cost panels inside Space Activity** (QM plan "Space home"). Not done; Activity uses its own grammar.
- **Human takeover of a live agent browser session from the room, and teach-by-demonstration routines** (`apps/VEREVON_UI_COWORK_RESEARCH_2026-08-13.md` §2.2-2.3).
- **Generalising the Inbox draft-lease** (`ConversationPanel.tsx:610-671`) to other Space-shared surfaces (cowork research §2.5).

## 4. Known gaps (frontend/UX, verified in code)

1. **No human-to-human messaging without an AI turn.** Every post is a Model Plane invocation answered by "Verevon". This contradicts the product's own "agents answer only when invoked" rule and makes the room expensive and noisy as a Slack/Teams substitute.
2. **No DMs or group DMs**, and no project/case kind creation.
3. **No message edit/delete, reactions, attachments, link previews, drafts, scheduled send, bookmarks, forwarding or polls.**
4. **No push realtime.** 6 s polling, plus an N+1 transcript fetch per post on each update. Other members see a reply only after a poll.
5. **No notifications** (in-app for Space, push, email, mention routing), no mute/DND, and no cross-room unread counts.
6. **No message search.** The sidebar filter covers titles and previews of the selected room only.
7. **Agent work is opaque inside the room.** It shows status only: no tool-call, step, plan or browser cards and no live view, although these exist in `/chat`, the AgentRunConsole and the dashboard browser.
8. **No agent PC/browser/sandbox per agent, and no personal agents.** A personal Space cannot hold agents.
9. **One agent per message, and no delegation or multi-agent coordination.**
10. **No binding-policy editor anywhere.** Chips are read-only and policy can only be set by back-end defaults.
11. **No member-role change, guests/external, channel browse/join, or room rename/archive/delete.**
12. **Silent shared-read refusal.** The timeline hides a 403 `space_read_not_authorized` behind the preview text (`SpaceRoomTimeline.tsx:201`).
13. **Thread deep links are generated but ignored** (`activity-grammar.ts:200,326`, with no reader in `SP`).
14. **Pinned posts are not shown first.** The client re-sorts chronologically (`SpaceRoomTimeline.tsx:137-141`).
15. **The activity grammar is Norwegian-only** and dates use `nb-NO`.
16. **The agent avatar colour is collected but never displayed.**
17. **`getSpaceActions` and the agent action-grant routes have no UI consumer.**
18. **No calls or meetings at all.**
19. **Thin test depth beyond unit tests.** One mocked e2e spec, and no live multi-user proof (DOC audit "No live stack was exercised").

## 5. Architecture constraints that limit what Verevon can adopt

- **Plane ownership (no cross-plane DB access).** Application (Convex) owns the Space aggregate, read markers, presence, bindings and member grants. Control owns membership, roles, decisions and registration. Model (Session Core) owns threads, messages, runs, titles and pins. Data owns knowledge. So any Slack-like feature (reactions, edits, read receipts, threads-of-replies) needs an explicit owner. A reaction is a message mutation in Session Core, or a projection in Application, and never a gateway-side store. (DOC `apps/SPACE_AUTHORITY_ADR_2026-08-13.md:24-48`; SRC `CVX/schema.ts:546-1012`; `GW:1-6`.)
- **Two-step lifecycle.** Rooms, agents and member grants are created in Application, then accepted by Control. The UI must render `pending` and `unconfirmed` honestly, which rules out instant "created" UX. (SRC `spaces-client.ts:919-951, 961-977`; `SpacesIndexPage.tsx:340-368`.)
- **Content-bound append decisions.** Each message needs a fresh Control `thread:append` decision bound to its exact bytes. That makes cheap edits, quick multi-send, and offline queues harder, because each requires a new decision. (DOC QM plan S1.7; SRC `GW:49-115`.)
- **Shared reads are deny-by-default per org** (`thread_read_entitled`). A member may take part in a room without being able to read colleagues' turns. Any "channel history" UX must handle a partial transcript. (SRC `GW:2665-2680`; `spaces-client.ts:668-671`.)
- **Space access never widens an owner-plane ACL.** Effective access is the intersection of Space authority, recipient audience and the owner resource decision. Knowledge, documents and agent tools are separately gated ("Knowledge access is granted separately from chat"). (DOC `VEREVON.md:15-23`; SRC `SpaceKnowledgePanel.tsx:57-60`.)
- **ZDR and privacy tier must propagate.** Every turn carries `minPrivacyTier`, and ZDR content may not go to external embeddings. External delivery (Teams/Slack) leaves the boundary. (SRC `SpaceRoomComposer.tsx:400-412`; DOC `space-defenition.md`, Honest edges.)
- **Model proposes, Quarry executes browser actions under BrowserBroker grants.** An in-room agent browser must use Quarry grant checks and cannot be a Grok-style shared computer with shared credentials. (DOC `CLAUDE.md`; `space-defenition.md`, Honest edges.)
- **Human-only "future authority" wall.** Agents may never create or modify bindings, roles or grants, or approve gated actions. Model-eligible actions are allowlisted per action after forged-call proof, and no `spaces.*` action is model-eligible. This limits "agent manages the channel" features. (SRC `V3/src/shared/actions/model-eligibility.ts:1-60`; DOC `space-defenition.md`, Security model.)
- **Channel Plane is docs-only.** Slack/Teams continuity and external surfaces are blocked behind S5.1 → S4.4 → S4.3. (DOC audit §4 item 8.)
- **The BFF stays a proxy.** The browser composes the cockpit from owner projections, and the gateway must not invent composite grants or activity semantics. (SRC `SpaceCockpit.tsx:10-22`; `activity-grammar.ts:9-15`.)
- **Release gates R-1 to R-5 are open.** New *effectful* UI is expected to wait for candidate evidence. (DOC audit §2 footnote.)
- **Product stance.** The docs explicitly reject a "chat-feature parity race with Slack or Teams (presence, emoji, huddles, notification polish)" (DOC `space-defenition.md`, Non-goals). This conflicts with the user's stated goal of a Teams + Slack + Grok hybrid, and the program needs to decide which wins.

## 6. Doc-vs-code discrepancies

1. **"If no agent is called, the normal human-only flow continues" and "Agents answer only when explicitly invoked"** (`space-defenition.md`, Message routing and Acceptance criteria) versus the code, where every room post is a model turn answered by the default Verevon persona (`SpaceRoomComposer.tsx:402-421`; test `SpaceRoomComposer.test.tsx:206`).
2. **"Mentioning an agent that is not bound … offers to add it"** (`space-defenition.md`, Invocation rule) versus the code, where unbound or paused agents are simply not candidates and nothing is offered (`SpaceRoomComposer.tsx:188-199`).
3. **"The room's sidebar shows members, conversations, the agents available in that room, and explicit entry points for invoking them"** (`space-defenition.md`, UX behavior) versus the code, where the sidebar shows rooms and conversations only. Agents and members live in cockpit tabs (`CoreSidebarSpacesPanel.tsx:104-232`).
4. **SPACE_PAGE_AUDIT §18 says the sidebar shows working and unread dots "for every room in the list … as long as SOMEONE'S page is polling it".** In the code the live-work store is a browser-local `createRoot` signal, filled only by this browser's open SpacePage and retracted on leave (`space-live-work.ts:52-83`). §16 of the same doc states the correct behaviour.
5. **`SpaceCockpit.tsx:26-33` header comment says "Work and Knowledge still have none [sources]".** In the code both tabs are supplied with real panels (`SpacePage.tsx:433-434`). The comment is stale.
6. **The `activity-grammar.ts:17-25` header says "Today the only Space activity a plane publishes is a conversation …".** In the code, runs, schedules, processes, approvals, operations and authority adapters all exist (`activity-grammar.ts:286-753`).
7. **The cowork research doc** (`apps/VEREVON_UI_COWORK_RESEARCH_2026-08-13.md:60-63, 140-147, 524-550`) says the gateway publishes Personal Space only, that there is no roster, and that presence is dormant with zero functions. In the code there are org/named rooms, a roster, add/remove members, and wired `spacePresence` (`CVX/spacePresence.ts`; `GW:649-736`). The doc is stale.
8. **The QM plan's "Navigation"** specifies `/spaces/:spaceId/chat|work|…` routes and a "Space switcher below organization selection". The code uses hash tabs and the sidebar as the switcher (accepted deviation per the cockpit-wiring doc).
9. **The QM plan's "Space home"** says to reuse and extract `AgentRunConsole` panels into Space Activity. In the code Activity is a separate grammar renderer, and there are no run timeline, proof or tool panels in the Space.
10. **The audit §6.3 recommends four approval verbs.** The code deliberately ships two (`SpaceApprovalPanel.tsx:23-36`). The code comment justifies this: there is no standing-scope mechanism.
11. **`spaces-client.ts:668-671` says a 403 transcript "is a real answer, not a glitch"** and should be told to the user. In the timeline it is swallowed and rendered as the preview (`SpaceRoomTimeline.tsx:201`).
12. **`spaces-client.ts:54-58` says Session Core orders pinned posts first.** The timeline discards that order (`SpaceRoomTimeline.tsx:137-141`). Pinned posts are flagged but not lifted.
13. **The audit §2 row "Create a project / case Space from the UI: Not built"** matches the code. However, the `SpaceCockpit` purpose text for the Agent tab ("The active model, its skills, and which actions are permitted here", `SpaceCockpit.tsx:94-100`) describes content the tab does not show: model choice is per message in the composer, and skills and actions are not listed.
14. **The audit §6.1 rows** for presence, unread, pin, auto-title, member management, second rooms and approvals say **no**. Later sections of the same doc (and the code) show them shipped. The table is a 2026-09-06 snapshot and is superseded inside the same document.
