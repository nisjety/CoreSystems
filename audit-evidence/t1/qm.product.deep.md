# QM product deep dive (Audit 1, facet "product")

Source: `C:/dev/qm` (open source, TypeScript, MIT), HEAD `1e975255` dated 2026-10-12. Role: deep-dive analyst for the Verevon "Space" audit. Feature records: `qm.product.features.json` (146 records; ids are cited below as `#NNN`, short for `qm-product-NNN`).

Provenance tags: **SRC** read in source or config, **DOC** repo documentation or a commit subject, **INF** my inference (basis stated), **REA** not used (QM is open source, no shipped binary was analyzed). Nothing was built, run or tested. Git was used read-only (log, show, ls-tree, grep). No credential, session or user-data store was read.

How the records were checked: every evidence path was resolved against the repo and every line range was validated to lie inside its file by a script; I then eyeballed the first line of each anchor against the claim and corrected the ones that drifted. Semantics beyond the cited lines were not exhaustively read (files such as `src/core/orchestrator.ts`, 4.2K lines, and `plugins/web-ui/src/chat.ts`, 3.4K lines, were read by region).

Calibration note on the `uniqueness` field: it is judged against Teams, Slack and typical personal-agent products. The Grok Bot and buzz audits may find the same idea elsewhere, so treat "unique" (16 records) as "not found in a chat product" rather than "no other agent product does this". I downgraded 33 first-pass "unique" ratings for that reason.

---

## 0. Verdict

QM is the most complete implementation in the reference set of **an agent that is a resident of a person and of a room**, with its own memory, files, keychain view, crons, web apps and computer per scope. It is **not a messaging product**. Human-to-human chat, presence, typing, read state, push notifications, calls and meetings are delegated to Slack (the product's primary room) or absent. The web UI is an agent chat in which several people can take turns in one shared thread, plus a very rich resource console around it. In QM every web post is a model turn; there is no "just talk to my colleague" path (only `POST /api/turn` posts messages; verified in the web server route list).

For Verevon the value is concentrated in six places, all authority and behaviour designs rather than chat UI:

1. one scope resolver that composes layers, instructions, policy, egress and grants per turn (#001, #015);
2. an explicit **turn origin** (direct, human, ambient, automation) that decides which authority a turn may borrow (#066, #065, #139);
3. **audience-bound context**: scope label on every transcript entry, fold(tape, audience), memory disclosure checked at read time and a replay cutoff when the audience changes (#060, #061, #100);
4. room etiquette: default-off ambient judging with standing orders, a bot ledger, thread "stake", solicited versus proactive wakes, stop/steer, overheard text as untrusted data, plus a Slack eval catalogue that tests it (#002, #063-065, #086, #095);
5. approvals and credentials for shared rooms: requester-only approval with once/session/always, keychain asks, secret-drop forms, owner-auth boxes (#070-072, #053, #058);
6. human-only authority walls (#112).

The weaknesses are equally clear: single-organisation by design (#115), policies that are speed bumps rather than boundaries (#110, #111, #109), a shipped-but-stubbed Calendar (#028), nine feature flags and several half-shipped subsystems (loops, swarms, spine, team scopes), no notification layer at all, and durable-by-default retention (#116).

---

## 1. What QM is, in Space terms

- Tagline: "a multiplayer agent harness for work, in Slack and on the web", run in the operator's own cloud with the operator's models and keys [DOC README.md:1-30]. Each person and each room has scoped memory, files, keychain view, permissions, crons, web apps and a durable sandbox [DOC README.md:25-27].
- North stars: unhobble, free the brain (state in Postgres), fast is the best feature, agent UX = human UX, delete before you add [DOC docs/SPEC.md:5-11].
- Surfaces are projections of one source of truth: "everything else is a projection" of the tokens sent to the model [DOC docs/SPEC.md:29]. Slack and web read the same sessions and present them as each host expects.
- Size at HEAD: about 138K lines in `src/`, 38K in `plugins/web-ui/src`, 10K in `plugins/admin/ui`, 1,102 test files [SRC file counts via find/wc].
- Origin matters: a single commit on 2026-09-07 ("Bulk upstream of internal fork changes (#963)") changed 846 files (+84K/-17K lines) [SRC git show --stat]. The commit title and the yc-software organisation suggest the public repo is an upstream of an internal fork (INF), so large product areas arrived in bulk. Between the Verevon baseline commit `d719f540` (2026-08-12) and HEAD there are 821 commits.

---

## 2. Architecture

### 2.1 Process and plane map [SRC README.md:53-72, docs/combined-services.md, plugins/*, src/slack/README.md:1-12]

```
 Slack (Socket Mode or HTTP events, N accounts)      Web SPA (Lit) · Admin (Lit) · Desktop (Electron)
        │ in-process plugin, direct client                   │ signed HTTP (HMAC, replay-checked) via portal
        └──────────────────────────────┬────────────────────┘
                                       ▼
                       CORE (Fastify, ~300 /v1 route literals) ── orchestrator, run workers, scheduler, policy
                          │            │                    │
                          │            │                    └── harness adapter: Pi | Claude Code | Codex | OpenCode (one tool catalogue)
                          │            └── per-scope sandbox: 10 backends behind AgentComputerProfile
                          └── Postgres: sessions + tape, runs/leases, deliveries, memory records, grants,
                              keychain, crons/triggers, loops, projects, teams, directory, flags, config
                              (+ S3 for blobs/snapshots, pg-boss for cron jobs, LISTEN/NOTIFY wakeups)
```

Surface plugins are separate packages that talk to core only over signed HTTP; AGENTS.md forbids importing core except through `plugins/chassis` [SRC AGENTS.md:33-38] (#054). Slack is the one in-process plugin core starts and supervises [DOC README.md:94-97].

### 2.2 Core data model

| Concept | Shape | Evidence |
|---|---|---|
| Scope | `kind:ref` string, kinds personal, channel, team, org, group | src/types.ts:19-53 (#015) |
| Conversation | kind dm/channel/group, threadRef, audience[], isPrivate, isMpim, publishMembers | src/types.ts:55-67 (#001) |
| Session | `{scopeId, threadRef, surface, title, pinned, archived, color, status, forkedFrom, parentSessionId, goal, ...}`; thread refs `dm:<ch>[:ts]`, `ch:<ch>:<root>`, `grp:<ch>:<root>`, `web:<user>:<uuid>`, `agent:main:subagent:*` | src/types.ts:98-125; src/slack/message-gating.ts:134-165 (#142) |
| Entry | `{seq, parentSeq, type, payload, scopeLabel}`; types user, assistant, thinking, tool_call, tool_result, soul, system, delivery, approval_request, approval_resolved | src/types.ts:127-148 |
| Resolution | layers, systemPrompt, egress, commandPolicy, securityPolicy, sharingPosture, approvalGrantModes, grantedHandles | src/types.ts:150-163; src/resolution/resolution-service.ts:45-112 |
| Grant | `{ownerScopeId, ref, granteeScopeId, permission, grantedBy}` over kinds file, skill, deploy, cron, service-cred | src/acl/resource-ref.ts:1-35 (#088) |
| Destination | delivery target with slack account/team, thread, edit ref, react/delete/pin ops, task list, approval ids, relaySender | src/types.ts:208-240 |
| Run | lease token, heartbeat, signals abort/steer/client_result, per-attempt tool ledger | src/runs/run-store.ts:34-86 (#099) |
| Trigger | cron, webhook, loop, monitor with owner, createdBy, destination, recipientConsent, runAs | src/types.ts:189-206, 376-401 |
| Keychain | credential (env/file/broker), grant (once/standing, audience scope), ask (24h TTL), secret drop (7d TTL) | src/credentials/keychain.ts:122-166; secret-drop.ts:39 |
| Project / Team | roster with owner and optional Slack channel; teams with members, admins, rooms, isolatedInOpen | src/projects/project-store.ts:9-60; src/teams/teams.ts:9-67 |
| Config per scope | soul (instructions, CAS versioned), command policy, security posture, sharing posture, approval grant modes, egress, runtime selection, ambient, header pin | src/resolution/config-store.ts:150-300 |

### 2.3 Turn lifecycle

1. **Ingress**: Slack event (durably staged before ack, #102), web `POST /v1/turns?async=1`, webhook, cron fire, ambient judge, subagent completion wake, Gmail/Slack loop ingestion.
2. **Origin**: classify direct / human / ambient / automation (#066). Only a live human (or a verified delegated human) in an all-internal audience gets personal authority; bots are never live humans (#139).
3. **Resolution**: scope from conversation kind; layers (org ro, team ro, scope rw); soul composed org-first; command policy = org floor + scope rules; security posture; sharing posture; granted handles intersected across the audience; egress intersected across the audience (#015, #111).
4. **Context**: `fold(tape, audience)`; entries not entitled to every audience member are dropped; an audience change cuts the replay (#061, #100); memory recalled through the disclosure view (#060).
5. **Harness** (#076): the router resolves a runtime (approved harness list, scope selection, purpose defaults) and drives one adapter with the QM tool catalogue (execute, sandbox, skills, files, apps, memory, history, subagents, sessions, background, cron, webhook, guidance, share, goal, runtime, context, admin, finish_silently, plus the surface tool).
6. **Effects**: tool calls pass the command policy and posture; gated calls pause for approval (#070); credentials come from the keychain through grants (#072-073).
7. **Output**: entries go to the tape; deliveries are enqueued with idempotency keys and projected to Slack or the web transcript (#036).

### 2.4 Protocols and transports

- HTTP/JSON `/v1/*` (about 285-307 route literals depending on how templates are counted), source-authenticated by HMAC with replay dedupe (#114); agent calls use run-bound capability tokens (#049).
- Server-Sent Events: run events shaped like AG-UI (`TOOL_CALL_START`, `TOOL_CALL_RESULT`, `RUN_FINISHED`, custom `delta` frames with text offsets), session state, delivery nudges, inbox item events (#098, #103).
- Slack: Socket Mode by default, HTTP Events as an alternative; Block Kit buttons for approvals; `task_card` blocks when flagged (#052, #082, #102).
- Postgres LISTEN/NOTIFY for wakeups, leader leases and advisory locks for singletons, pg-boss for crons (#101).
- No WebSocket to the browser, no offline store, no E2EE.

---

## 3. Findings by facet focus

### 3.1 Per-person and per-room scopes (memory, files, keychain view, permissions, crons, web apps, durable sandbox)

- **Scope is the unit of everything.** A DM is the actor's personal scope; a channel or group is the room's scope; a web "project" is a group scope [SRC resolution-service.ts:15-23, project-store.ts:70] (#001, #017). Resources (memory notebook, files, crons, skills, apps, keychain grants, sandbox default) hang off the scope. The web Projects page and the session top bar show all of them together (#128, #129).
- **Layering.** The org scope mounts read-only as `global/`, each applicable team as `team-<id>/`, the active scope read-write; grants to other scopes appear as `shared/<name>` handles (#059). External audiences get no org or team layer at all (resolution-service.ts:70-77).
- **Sandbox per scope, many per scope.** Each scope can own several computers, one default, listed/created/retired by the agent, on ten providers behind a capability profile (#056, #057). `execute` can run on the scoped box, a blank scratch box, an invocation-only owner-auth box carrying the speaker's credentials, another room's box (read-only by etiquette), or a named sandbox (#058).
- **Memory.** A per-scope notebook with record-level sensitivity (ordinary, unknown, sensitive, restricted) and provenance; classification takes the strictest of caller and helper model; disclosure is evaluated at read time against the actor and audience; unknown provenance stays narrow (#060, #061). The built-in recall is notebook tail injection (6,000 chars) plus deltas and an all-words search, not vector search (#032).
- **Keychain view.** Per person: connected accounts, credentials with expiry, which conversations hold grants, pending asks (#096). Sharing a credential with a room is an explicit grant to an audience scope (#072). In Open rooms the live speaker's own keychain can be used only through a disposable owner box (#058, #108).
- **Permissions.** Read/write/manage differ by scope kind and by channel privacy (#016). Org floor policies compose with narrower scope rules (#107, #110).
- **Crons.** Owned by a scope; modes owner, scopeShared (default in rooms, any member can edit, owner notified), scopeFloor (no personal keychain). Fires re-check owner status, room membership, destination visibility and recipient consent (#043, #074).
- **Web apps.** Published from a scope, private to the creation channel until shared, versioned with rollback, persistent `$DATA_DIR` (#089, #090).

### 3.2 Channels, group messages, projects, teams

- **Slack is the room.** Channels, private channels, group DMs and DMs map to scopes; each thread is a session of its scope with its own transcript (#142). The agent sees mirrored channel history in a Postgres cache with edits and deletes tracked and falls back to live Slack history with honest limits (#137, #005). It can DM people and open group DMs (#068).
- **Projects are the web room.** An owner and members; any member can continue a shared web thread, each message speaker-labelled; a project can be linked to a Slack channel so the roster follows the channel and the channel becomes the default delivery audience (#009, #017, #018). Writes into project threads are fenced by a roster version (#105).
- **Teams (flag `team_scopes`, new 2026-10-11)** are resource-sharing groups attached to rooms, not chat containers: a team contributes read-only files, skills and credentials when the speaker belongs to it and the room is configured for it (or everyone present is on the team, or Open posture and not `isolatedInOpen`) (#019).
- **Guests and externals.** Principals are internal or guest; guests and bots cannot start work. Mixed rooms are off by default; partner Slack workspaces get an isolated namespace and a private continuation that moves work to the employee's DM (#022, #136, #144).
- **Directory.** Slack-synced; used for visibility checks, pickers and reach (#020, #033). There is no channel creation, room discovery or self-join in QM.

### 3.3 Multiplayer behaviour: when the agent speaks and as whom

- **Direct address**: @mention, DM, thread where the agent has a stake (#002). Bare "stop" aborts (#003). A second person can steer a live run; steer authors are recorded (#004).
- **Ambient (channels and groups)**: a cheap judge reads batches of overheard messages and engages only when addressed, needed or a standing order matches; silence is the default; ambient is off until a channel has standing orders or action bots, per channel or org-wide (#063). Standing orders and a bot ledger (ignore, rollup, action, user) are versioned with CAS edits (#064).
- **Authority of the wake**: solicited (a verified asker who is the newest human author and still in the room) runs as that person with the room roster as audience; proactive runs as `system:ambient:<org>` with no personal authority and can steer into a live ambient run instead of starting another (#065). Bots can address the agent but are never live humans (#139).
- **Speaking contract**: in surface turns the only way words reach people is the surface tool (post, reach, react, edit, delete, read_thread, whats_new, search, read_members, read_file); ending a turn without a post is silence; post cannot address anyone outside the current conversation; reach opens group DMs and follows a narrowest-audience rule (#067, #068). Relays carry "Sent for @name" (#037).
- **Overheard text** is imported as flagged entries, labelled untrusted by the judge prompt and the screening rubric (#095).
- **Spine** (flag `responsive_spine`): a coordinator that cannot run commands, answers fast, delegates to subagents and is woken on completion (#079).
- **The behaviours are tested**: a live-Slack catalogue posts as alice/bob/carol and asserts: answer the asker not the chatter, attribute "what she suggested", reply in thread, stay silent while two humans talk or when someone else is mentioned, handle a second asker interrupting, follow the latest contradicting instruction, refuse to send on someone else's behalf, answer in channel, three-way addressing; a Slack digital-twin backend adds redelivery, missing-scope and rate-limit cases (#086).

### 3.4 Approvals and credentials

- **Approval card**: Allow once / Allow session / Allow always / Deny with purpose, command and "flagged as"; only the requester may decide; the run is re-executed with `{requestId, approved, scope}`; context is recoverable from core if the plugin lost memory; admins can switch session/always off per scope (#070). Security-screen flags use the same card with kind `input` (#071).
- **Placement**: in channels the card goes to the requester's DM with a thread pointer; in the web the pending approval panel blocks the composer and decisions appear as transcript notes (#071).
- **Keychain ask**: the agent asks the owner of a credential; the card appears where the conversation is; once/standing/deny; the blocked command re-runs on approval (#072).
- **Secret drop**: one-time forms so secrets never enter chat (#053). **Owner box** for Open rooms (#058). **Broker delivery** injects secrets into HTTP calls to allowed hosts, methods and path prefixes (#073).
- **Walls**: admin grant changes, impersonation and approval decisions are deliberately unavailable to the agent (#112).

### 3.5 Harness choice, runtimes and models

- Four adapters (Pi, Claude Code, Codex, OpenCode) plus a mock, each declaring control transport, tool transport and capabilities (abort, steer, images, thinking level, fast mode, provider sessions, native tape, goal enforcement); the same tool catalogue is bridged into each (#076).
- Runtime = harness + model + effort + fast mode; cascade is requested, purpose (cron, subagent, helper), scope, org; admin approves the harness list; the agent can switch runtime mid-task and resume on the new one; goals block switching (#075).
- Per-person AI account (Claude subscription, ChatGPT/Codex subscription or API key) replaces the company account for personal turns (#077). Model presets (8), effort levels up to "ultracode", an Ultrafast tier (#131).
- Caveat: `HARNESS` unset means the mock harness; production config warns about it (src/config.ts:1257-1259).

### 3.6 Plugins, skills and extensibility

- "Plugins" are repo packages, not a runtime plugin loader (#054). Extensibility is **skills** (25 seeds; scope-owned; draft/reviewed/published/archived; signed; promotable; packs from git; lazy materialisation) (#039-041), **crons/webhooks/loops/watches** (#042-045), **MCP client** with per-user credentials (#047), **OAuth/Composio** (#046), the **self-API** (#049), **client-executed tools** (#048), the **qm CLI and deployment directory** (#055).
- Not present: slash commands in Slack, outbound webhooks, an app marketplace or directory, an MCP server, forms beyond secret drops.

### 3.7 Slack and web surfaces

- Slack: manifest with 25 scopes, Agents top-bar pin, Block Kit approvals, reactions, pins, emoji, header pin, task cards (flagged), multi-account (#052, #082, #135, #144). Durable ingress (#102), outbox (#036), mirror-first context (#137).
- Web: Lit SPA with Home/Inbox/Search/Browse, Projects page, session top bar, sidebar with pinned/recent/archive/colour/multi-select, multiview up to 16 panes, Cmd+K, memory editor, keychain page, apps, skills, crons, webhooks, loops, settings (#127-135).
- Cross-surface cues: pinned header with model and settings link, "Follow via QM Web" link after five minutes, Slack sessions open read-only in the web (#135, #009).
- Desktop: Electron wrapper with browser sign-in handoff; no tray, notifications or auto-update (#121, #122).

### 3.8 Admin

- Separate admin service, `org_admin` the only role; users, invitations (24h single-use links), spend, transcripts, captured model requests, live runs, errors, audit, skills, crons, deployments, keychain metadata, governance per scope (posture, auto-flagger tester, approval options, command policy, ambient policy, sharing posture, egress), Slack integration (external audience, internal overrides, pinned header, acknowledgment emoji), model providers, branding, feature flags, onboarding (#025, #026, #107-113, #136).
- Admin reads of content are audited but not consent-gated (#113). The agent has a thin admin tool over the same handlers, with some actions portal-only (#050).

### 3.9 Background and autonomous work

- Crons (#043), watches (#045), loops with ship gates and a return-rate governor (#044), goals with floors and an independent verifier (#081), persistent subagents with a mailbox (#078), swarms with a shared board computer and worker controls (#080), suggested activities and a morning digest (#038). Fire-time authority re-checks and recipient consent are the standout design (#074).
- Inbox turns Gmail and Slack conversations into reviewable agent drafts with ship gates; drafts only, never sent without a human decision (#093, #140).

### 3.10 Collaborative artifacts

- Files with scope ownership and grants (#088); published apps with versions, general-access sharing, Slack access requests, annotation bar (#089-091); playgrounds in chat (#092); skills as shared procedures (#039); code flows with CI and independent review gates (#094).
- Absent: documents/wiki/canvas (a `ui_canvas` experiment was added 2026-10-09 and reverted 2026-10-10), human task boards, polls, whiteboards. "Tasks" in QM are the agent's own plan list shown in replies (#138).

### 3.11 UX and information architecture

- Scope-centred Projects page (conversations + resources + settings aside), session top bar with tool counts, Pinned/Recent sidebar grouped by project, multiview with density tiers, collapsed completed-work disclosure, suggested starters that fill a draft but never send (#127-134).
- Known IA debt acknowledged in SPEC: repeated Browse list patterns, each artifact page designed separately [DOC docs/SPEC.md:44-45].

---

## 4. Category matrix (what is present, partial, absent)

| Category | Present | Partial / flagged | Absent (verified by grep or route list) |
|---|---|---|---|
| 1 Messaging core | Slack threads/mentions/reactions/edits/deletes mirrored; Markdown, Mermaid, KaTeX; attachments as model inputs; queue/steer; pins (conversation); snapshot share; visibility-preserving fork; scheduled send via cron relay; drafts (browser-local) | Link previews (none user-facing) | Human-only messages in web, human DMs in web, polls, voice clips, read receipts, typing, presence, saved items |
| 2 Org & membership | Scopes, projects, roles read/write/manage, guests, external Slack, invites, principal links, directory, admin console | Teams (flag), mixed rooms (default off) | SCIM, passkeys, channel creation/discovery, role editor beyond org_admin |
| 3 Calls & meetings | none | Calendar nav row is a stub (#028) | Audio/video, huddles, screen share, recording, captions, meeting chat |
| 4 Search | Cmd+K sessions+resources; agent search (mirror, live-as-asker, sessions, history) | | AI answers with citations, people/file directory UI |
| 5 Notifications | Sidebar attention states, Inbox badge, outbox, approvals to DM, digests | | Push/mobile/desktop notifications, mute/DND, unread counts |
| 6 Apps & extensibility | Skills, packs, webhooks in, crons, loops, watches, MCP client, OAuth/Composio, self-API, Slack app, secret drops | Client-executed tools (consumer reverted) | Slash commands, outbound webhooks, marketplace, MCP server |
| 7 Agents in the room | Everything in section 3.3-3.5, 3.9 | spine, swarms, subagents behind flags | Voice/agent calls |
| 8 Artifacts | Files, apps, playgrounds, inbox review, code gates | | Docs/wiki/canvas, boards for humans |
| 9 Realtime | SSE multiplex, AG-UI-shaped stream, durable runs, tape, Postgres bus, durable Slack ack, UI-state sync | blue-green ownership (DOC) | WebSocket, offline, E2EE |
| 10 Governance | Postures, sharing posture, screening, command policy, egress, audit, signed requests | Admin content reads audited only; retention API-only | DLP, eDiscovery, residency, multi-tenancy, SCIM |
| 11 Client shell | Web, Electron prototype, deep links, themes, branding | | Tray, native notifications, auto-update, i18n |
| 12 UX | Projects page, top bar, sidebar, multiview, density tiers | | Space home page beyond Projects |

---

## 5. Best in class (relative to the reference set)

1. **Authority provenance per turn** (origin enum + live-human gating + solicited/proactive split) (#066, #065, #139).
2. **Audience-bound context and memory** (scopeLabel per entry, audience floor, disclosure at read time, replay cutoff on audience change) (#060, #061, #100).
3. **Fire-time authority re-check with recipient consent and owner edit notices** for scheduled work (#074).
4. **Approval design for shared rooms** (#070, #071, #072, #053, #058).
5. **Human-only authority walls and the explicit "do not fix as parity" statement** (#112).
6. **Room etiquette made testable**: the multi-user Slack catalogue (#086).
7. **Scope-centred resource UI** (#128, #129).
8. **Durable-before-ack Slack ingress** (#102) and the **outbox** (#036).
9. **Visibility-preserving fork** (#011) and **snapshot share** with read-time creator recheck (#010).
10. **Harness portability with one tool catalogue** (#076) and **sandbox portability with fail-closed capability routing** (#057; egress is the exception).

## 6. Weaknesses and limitations

- **No messaging core of its own**: nothing for human-human chat, presence, typing, read state, notifications, calls or mobile (section 4). QM is not a template for those.
- **Single organisation** and not a multi-tenant boundary (#115).
- **Policies are heuristics**: command policy is bypassable; screening incomplete and can proceed "unscreened"; egress enforcement is conditional and fail-open with a warning; browser actions bypass gates; sandbox credentials are plaintext while in use; credential "purpose" is advisory (#109-111, #058, #073, #085) [SRC SECURITY.md:131-200].
- **Admin can read everything**, audited but not consent-gated (#113). **Capture and retention**: exact model requests captured by default, artifacts never expire, retention UI removed (#116).
- **Shipped-but-stubbed or reverted**: Calendar (#028); `ui_canvas` (added and reverted in two days); admin Metrics/Retention tabs removed.
- **Config and flag sprawl**: nine flags and many env knobs; SPEC's own Wall of Shame lists loops, per-page UI designs, delivery retries and the admin tool as over-engineering (#026, #120).
- **Model-provider paths outside the gateway**: ambient judge call and the OpenCode sidecar key (SECURITY.md).
- **Identity by string**: `group:web-project-*` and `kind:ref` scope ids carry authority in the string; Verevon's plan already rejects that.
- **Uneven maturity**: strong core and Slack surface; thinner web human-collaboration features (drafts are browser-local, no unread, no mute).

## 7. Corrections to the scout inventory, and what the Aug-13 comparison missed or that changed

### 7.1 Corrections to `qm.inventory.md` (verified in source or git)

| Inventory claim | Verified state | Evidence |
|---|---|---|
| Calendar tab shipped (3.3, 7a) | Placeholder showing "Coming soon" | #028; plugins/web-ui/src/calendar.ts:1-18 |
| "AG-UI" string not in source, claim unverified (3.9, 7a) | Server emits `TOOL_CALL_START`, `TOOL_CALL_RESULT`, `RUN_FINISHED` and custom delta frames; web client feeds them to `@tanstack/ai`'s AG-UI `StreamProcessor` | #098; run-events.ts:16-31,98; core-bridge.ts:4-5,1583-1591 |
| Webhook verification: only `hmac-sha256` | github, slack, stripe, linear and hmac-sha256 | #042; verifiers.ts:42-140 |
| SPEC: default memory recall is vector search | Built-in: tail 6,000 chars plus deltas; search is all-words match (vector may exist in external providers) | #032; memory-service.ts:85-129 |
| Dark mode "limited" (low confidence) | `.dark` class and theme choices, plus imported themes | #123; settings.ts:180-230 |
| Slack default events mode unknown | Socket Mode when tokens are present; HTTP Events optional per account | src/slack/README.md:1-12; #144 |
| Baseline cites `pi-tools.ts` | File removed into `agent-tools.ts` by bulk upstream on 2026-09-07 | git ba457a22 |
| Retention "removed tabs" | Known only from the commit subject (git b7994395); the retention API still exists, and `plugins/admin/ui/metrics.ts` is still present | #116; src/admin/retention.ts |

### 7.2 Product deltas the Verevon comparison could not have seen or under-weighted

- **Scale**: files under `plugins/web-ui/src` 58 -> 141, `src/harness` 13 -> 28, `src/sandbox` 17 -> 36, `src/memory` 10 -> 24, `src/loops` 0 -> 23, `plugins/admin/ui` 0 -> 27, `src/api` 65 -> 96 between `d719f540` and HEAD [SRC git ls-tree]. The baseline read roughly half of today's product.
- **Memory privacy changed**: the baseline's reject item "shared-room facts auto-copy into personal memory" is now governed by sensitivity labels, provenance, read-time disclosure and the audience replay cutoff (git 58a9dc63, 2026-09-29) (#060, #061). The Open/Isolated sharing posture (git 1366726e, 2026-09-10) is new (#108).
- **New multiplayer machinery**: ambient judge details, standing orders and bot ledger, solicited/proactive wakes, overheard import, spine, persistent subagents, swarms, goals (git e0b8686c, 1bd53763, dc93931b) (#063-065, #078-081, #095).
- **New collaboration surfaces**: project-Slack channel link, team scopes, external Slack isolation with private continuation, multi-account Slack, mixed-room switch (#018, #019, #022, #136, #144).
- **New credential flows**: keychain asks, secret drops, owner-auth box, broker delivery, BYO AI account (#053, #058, #072, #073, #077).
- **New products inside the product**: Inbox with ship-gated drafts and loops, published apps with Google-style sharing and annotation bar, desktop client (#044, #089-091, #093, #121).
- **Reverted**: `ui_canvas` and `ui-state` skills (2026-10-09, reverted 2026-10-10; the original commit says canvas code ran with full page privileges and no sandbox, the revert states no reason); Cmd/Ctrl+1-9 session hotkeys; admin Metrics/Retention tabs [DOC git 19fdb8d0, f5e37f63; SRC revert stat].
- **Verevon plan items QM already ships in some form** (useful as reference implementations): visibility-preserving fork (S1.7, #011), standing orders (S5.2, #064), governed internal app publish (S5.4, #089), watches (S4.3, #045), a simpler delivery outbox (S4.4, #036), a Slack adapter and external identity links (S5.1/S5.3, #021, #102, #144), parts of the Work cockpit (S4.5, #082, #083, #128), approval resume (R-3, #070).

## 8. Recommendations for Verevon Space

Constraints applied: no cross-plane DB access; Control is the only authority; Space membership never widens an owner-plane ACL; ZDR must propagate; Model proposes browser actions and Quarry executes; Channel Plane is docs-only; deny-by-default effect policies.

### 8.1 Copy (semantics, not code)

1. **Turn origin as a signed claim** (direct, human, ambient, automation) carried in Control decisions and effect receipts; bots never live humans (#066, #139). Owner: Control + gateway.
2. **Shared-thread append and speaker labels** for non-owner posts, with per-entry audience labels (#009, #011). Owner: Model Plane Session Core with a Space-authorised append decision; this removes the "one thread per author" limit.
3. **Audience-change replay cutoff** and read-time memory disclosure: fingerprint scope plus recipient set, cut replay when it changes (#061). Verevon already stores recipient audience snapshots and revisions; add the cutoff in Session Core.
4. **Ambient policy per room**: default off, standing orders, bot ledger, solicited versus proactive authority, thread stake, stop/steer; adopt the ten multi-user scenarios (mu-*) as acceptance tests before enabling any unprompted agent speech (#002, #063-065, #086).
5. **Approval etiquette**: requester-only, three scopes, card to DM when requested in a room, resumable run, keychain asks and secret drops that route to Control credential grants (#070-072, #053).
6. **Surface tool contract** for agent speech (explicit post, explicit silence, narrowest-audience reach, relay attribution) (#067, #068, #037).
7. **Fire-time re-check, recipient consent and owner edit notices** for Space schedules (#074, #043).
8. **Channel adapter patterns**: durable-before-ack ingress, mirror-first context with honest limits, project-to-channel binding with one-way roster sync, header pin and follow link, external-workspace namespace with private continuation (#102, #137, #018, #135, #022). These are design inputs for the Channel Plane ADR (S5.1).
9. **UI**: scope-centred room page, session top bar with tool counts, Pinned/Recent grouped sidebar, completed-work disclosure, agent plan list, goal/swarm strips (#128-132, #138).
10. **Resource grants as first-class kinds** with owner-versus-created-in scope (#088). Owner: Data Plane for files, Control for grants.

### 8.2 Adapt

- Open/Isolated sharing posture: express as a Control-owned room policy that can only narrow, keep deny-by-default, never auto-enable (#108).
- Memory sensitivity labels: run classification under ZDR and decide Model-vs-Data ownership first (the Verevon Memory page is blocked on exactly this) (#060).
- Self-API with run-bound tokens: map onto Verevon's typed action registry; keep "always the asking person" (#049).
- Goals, subagents, swarms: only via Model Plane tasks with budget, depth and lifetime caps and visible human controls (#078, #080, #081).
- Team scopes: as org-core resource groups (skills, credentials, files), not chat containers (#019).
- Principal links: as Control identity links with evidence and anti-takeover rules (#021).
- BYO AI account: only if cost and ZDR policy can be modelled per person (#077).

### 8.3 Avoid

- Single-org tenancy shortcuts; string-encoded authority (`group:web-project-*`); fail-open egress or "unscreened but effectful"; heuristic command policy as a boundary; agent-written page code with full privileges (the reverted `ui_canvas`); admin content reads without consent policy; default capture of exact model requests; bearer public links as default; a second work queue; a stub nav row (Calendar) shipped to users; model or judge calls outside the gateway.
- Do not use QM as the model for human messaging, presence, notifications, calls, mobile, i18n or E2EE: it has none. Use the Teams and Slack audits for those, and treat QM only as the agent layer on top.

## 9. Verevon Space gap map (from `v-space-backend.baseline.md` known gaps)

| Verevon gap | QM answer | Adoption path |
|---|---|---|
| Every room post is a model turn; no human-only message | None (QM also invokes the model on every web post; humans talk in Slack) | Design in Verevon: Application-owned conversation store or non-AI Session Core post; borrow ambient/stake rules (#002, #063) |
| Non-owner replies refused | Shared-scope threads continuable by any member, speaker labels, participants (#009) | Space-authorised shared append in Session Core |
| No push realtime (6 s poll) | One SSE multiplex with resync, AG-UI-shaped resumable run stream (#098, #103) | Frontend/Application projection; re-authorise on subscribe |
| No DMs, group DMs, projects, discovery | DM = personal scope; group DM and channel via Slack; web projects; agent can open group DMs; no discovery (#001, #017, #068) | Personal Space with agent; project Spaces |
| No reactions, edit, delete, attachments | Slack-side reactions/edit/delete mirrored as transcript revisions; attachments as model inputs (#005, #006, #008) | Revision entries, typed reaction events |
| No Space-originated notifications | Outbox, approval DMs, digests; no push (#036, #070) | Application outbox (S4.4) |
| Personal agents impossible | A personal scope owns memory, files, keychain, crons, apps, computer (#015, #056) | Allow agent binding in the personal Space under Control |
| Granted approval does not resume | Resume with decision payload, three scopes, recoverable (#070) | R-3 |
| Agent PC/browser partial | Per-scope computers, scratch, owner box; browser as a skill (#056-058, #085) | Model sandbox lease per Space; keep Quarry for browser |
| No Space memory, cost, agent-to-agent | Per-scope notebook with provenance; spend by scope in admin; subagents/swarms (#060, #087, #078) | Decide plane ownership; add Space dimension to cost |
| Watches not exposed | `background` watch tool (#045) | Expose capability-core watches |
| Fixed editor role | read/write/manage by kind; owner and members (#016, #017) | Control role editor |
| Calls absent | Absent in QM too | Channel Plane meeting intelligence (docs only) |
| ZDR not acted on by Session Core | QM has no ZDR; retention durable (#116) | Verevon-only work |

## 10. Open questions and unverified items

- Which flagged features (spine, swarms, subagents, loops, team scopes, task cards) are in production use at the source organisation is unknown; flags default off.
- Reasons for reverting `ui_canvas` are not in the commit message (INF: privilege model).
- Ambient judge quality, latency and cost have no benchmark in the repo (`src/memory/bench.ts` covers memory only); memory sensitivity classification accuracy is explicitly unmeasured [DOC memory-records.md:94-100].
- Ambient judgments are listable by admins (decision filter act/ignore/fastlane, counts, pagination, audited reads) [SRC src/api/routes/admin/slack-mirror.ts:91-137]; their retention policy was not checked.
- Slack Connect and external flows are exercised mostly through the twin backend and unit tests, not live.
- Whether the web will ever get a human-only message mode: no signal in docs or commits.
- Uniqueness ratings need cross-checking against the Grok Bot and buzz audits.
- Not read in depth: `src/core/orchestrator.ts` outside the cited regions, `src/loops/loop-fire.ts`, `src/swarms/*`, `plugins/portal/src/*`, `cli/src/*`.

## 11. Record index by category

| Category | Records (qm-product-NNN) | Count |
|---|---|---|
| Messaging core | 001-014, 137, 142-143 | 17 |
| Org & membership | 015-027, 136, 144 | 15 |
| Calls & meetings | 028 | 1 |
| Search & discovery | 029-033 | 5 |
| Notifications & attention | 034-038 | 5 |
| Apps & extensibility | 039-055, 145 | 18 |
| Agents in the room | 056-087, 095-097, 138-141, 146 | 40 |
| Collaborative artifacts | 088-094 | 7 |
| Realtime & sync architecture | 098-106 | 9 |
| Governance, security & compliance | 107-120 | 14 |
| Client shell | 121-126 | 6 |
| UX patterns & layout | 127-135 | 9 |

Quality scale: 5 = unusually strong execution relative to peers, 3 = adequate or flagged, 1 = stub. Confidence: high = read in source or doc, medium = file and doc names verified but internals not read.
